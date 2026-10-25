//! Locating and loading the installed game data.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{bail, Context, Result};
use openac_formats::{
    parse_palette_animation, Archive, Bitmap, MapGrid, Mask, Palette, PaletteCycle, ResourceKind,
    Sprite, TileSetLibrary,
};

/// Pixels with this bit set hold a global palette index (low byte) that is
/// resolved every frame, so palette cycling (water, amber) can animate.
pub const ANIMATED: u32 = 0x0100_0000;

/// Marks a shadow pixel: the pixel underneath is darkened instead of
/// overwritten. Object sprites use palette index 0xfe for this; the game
/// remaps the destination through a darkening table (0x5c1358 in
/// ANCIENT.EXE), which averages to about 3/4 brightness.
pub const SHADOW: u32 = 0x0200_0000;
const SHADOW_INDEX: u8 = 0xfe;

/// A decoded image in the renderer's pixel format: `0x00RRGGBB`, or
/// `ANIMATED | index`; `None` marks transparent pixels.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<Option<u32>>,
    /// Bounding box of the solid (non-shadow) pixels: `x0, y0, x1, y1`
    /// (exclusive), or `None` if there are none.
    pub solid_bounds: Option<(u32, u32, u32, u32)>,
}

impl Image {
    pub fn new(width: u32, height: u32, pixels: Vec<Option<u32>>) -> Self {
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for y in 0..height {
            for x in 0..width {
                if matches!(pixels[(y * width + x) as usize], Some(p) if p != SHADOW) {
                    b = Some(match b {
                        None => (x, y, x + 1, y + 1),
                        Some((x0, y0, x1, y1)) => {
                            (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1))
                        }
                    });
                }
            }
        }
        Self {
            width,
            height,
            pixels,
            solid_bounds: b,
        }
    }

    /// Whether the pixel at `(x, y)` is solid (opaque and not a shadow).
    pub fn is_solid(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as u32) < self.width
            && (y as u32) < self.height
            && matches!(self.pixels[(y as u32 * self.width + x as u32) as usize], Some(p) if p != SHADOW)
    }
}

/// The installed game data directory (see `openac-extract install`).
pub struct GameData {
    root: PathBuf,
    pub maplib: Archive,
    /// Units, buildings, monsters and effects (`data/objects.dkx`).
    pub objects: Archive,
    /// Interface graphics and fonts (`data/controlg.dkx`).
    pub controls: Archive,
    /// Sound effects (`sound/sounds.dkx`), if installed.
    pub sounds: Option<Archive>,
    pub palette: Palette,
    pub cycles: Vec<PaletteCycle>,
    pub tilesets: TileSetLibrary,
    animated: [bool; 256],
}

impl GameData {
    pub fn open(root: &Path) -> Result<Self> {
        let maplib_path = find_ci(root, "maplib.dkx")?.with_context(|| {
            format!(
                "no game data in {} (expected maplib.dkx); run `openac-extract install <cd> {}` first",
                root.display(),
                root.display()
            )
        })?;
        let maplib = Archive::open(&maplib_path)?;
        let objects_path = find_ci(&root.join("data"), "objects.dkx")?
            .context("data/objects.dkx missing from the game data")?;
        let objects = Archive::open(objects_path)?;
        let controls_path = find_ci(&root.join("data"), "controlg.dkx")?
            .context("data/controlg.dkx missing from the game data")?;
        let controls = Archive::open(controls_path)?;
        let sounds = match find_ci(&root.join("sound"), "sounds.dkx") {
            Ok(Some(p)) => Some(Archive::open(p)?),
            _ => None,
        };
        let palette = Palette::from_bitmap(&maplib.read("PALETTE", ResourceKind::Bitmap)?)?;
        let anim = maplib
            .find("ANIM_PALETTE", ResourceKind::PaletteAnimation)
            .context("ANIM_PALETTE missing from maplib")?;
        let cycles = parse_palette_animation(&maplib.read_entry(anim)?, anim.width as usize)?;
        let tilesets =
            TileSetLibrary::parse(&maplib.read("b_ALIB", ResourceKind::TileSetLibrary)?)?;
        let mut animated = [false; 256];
        for c in &cycles {
            animated[c.index as usize] = true;
        }
        Ok(Self {
            root: root.to_path_buf(),
            maplib,
            objects,
            controls,
            sounds,
            palette,
            cycles,
            tilesets,
            animated,
        })
    }

    /// Missions and scenarios with a terrain map, sorted by name.
    pub fn missions(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        for item in std::fs::read_dir(&self.root)? {
            let path = item?.path();
            let is_dkx = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dkx"));
            if !is_dkx {
                continue;
            }
            let Ok(archive) = Archive::open(&path) else {
                continue;
            };
            if archive.find("b_MAPA", ResourceKind::MapGrid).is_some() {
                let stem = path
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_ascii_lowercase();
                if stem != "demo" {
                    names.push(stem);
                }
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn open_mission(&self, mission: &str) -> Result<Archive> {
        let path = find_ci(&self.root, &format!("{mission}.dkx"))?
            .with_context(|| format!("mission {mission} not found in {}", self.root.display()))?;
        Ok(Archive::open(path)?)
    }

    pub fn load_map(&self, archive: &Archive, mission: &str) -> Result<MapGrid> {
        let entry = archive
            .find("b_MAPA", ResourceKind::MapGrid)
            .with_context(|| format!("{mission} has no terrain map"))?;
        Ok(MapGrid::parse(
            &archive.read_entry(entry)?,
            entry.width.into(),
            entry.height.into(),
        )?)
    }

    /// Converts a global palette index to a renderer pixel.
    fn global_pixel(&self, index: u8) -> u32 {
        if self.animated[index as usize] {
            ANIMATED | index as u32
        } else {
            rgb(self.palette.0[index as usize])
        }
    }

    /// Loads an image by name. Sprites are preferred; bitmaps are cut out
    /// with their mask, if any.
    pub fn load_image(&self, lib: &Archive, name: &str, shadows: bool) -> Result<Option<Image>> {
        if let Some(e) = lib.find(name, ResourceKind::Sprite) {
            let s = Sprite::parse(&lib.read_entry(e)?)?;
            let mut pixels = vec![None; (s.width * s.height) as usize];
            for y in 0..s.box_height {
                for x in 0..s.box_width {
                    let i = (y * s.box_width + x) as usize;
                    if s.opaque[i] != 0 {
                        pixels[((s.y + y) * s.width + s.x + x) as usize] =
                            Some(match s.pixels[i] {
                                SHADOW_INDEX if shadows => SHADOW,
                                index => self.global_pixel(index),
                            });
                    }
                }
            }
            return Ok(Some(Image::new(s.width, s.height, pixels)));
        }
        if let Some(e) = lib.find(name, ResourceKind::Bitmap) {
            let b = Bitmap::parse(&lib.read_entry(e)?)?;
            let mask = match lib.find(name, ResourceKind::Mask) {
                Some(m) => Some(Mask::parse(
                    &lib.read_entry(m)?,
                    m.width.into(),
                    m.height.into(),
                )?),
                None => None,
            };
            let mut pixels = Vec::with_capacity(b.pixels.len());
            for y in 0..b.height {
                for x in 0..b.width {
                    let visible = mask
                        .as_ref()
                        .is_none_or(|m| x < m.width && y < m.height && m.get(x, y));
                    let index = b.pixels[(y * b.width + x) as usize];
                    let color = b.palette.0[index as usize];
                    // Bitmaps carry their own palette; only colors that match the
                    // global palette's animated entries may cycle.
                    let px = if self.animated[index as usize]
                        && color == self.palette.0[index as usize]
                    {
                        ANIMATED | index as u32
                    } else {
                        rgb(color)
                    };
                    pixels.push(visible.then_some(px));
                }
            }
            return Ok(Some(Image::new(b.width, b.height, pixels)));
        }
        Ok(None)
    }
}

pub fn rgb(c: openac_formats::Rgb) -> u32 {
    (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32
}

/// Which archive an [`ImageCache`] loads from.
#[derive(Clone, Copy)]
pub enum Source {
    MapLib,
    Objects,
}

/// Caches decoded images of one archive by resource name.
pub struct ImageCache {
    source: Source,
    images: HashMap<String, Option<Rc<Image>>>,
}

impl ImageCache {
    pub fn new(source: Source) -> Self {
        Self {
            source,
            images: HashMap::new(),
        }
    }

    pub fn get(&mut self, data: &GameData, name: &str) -> Result<Option<Rc<Image>>> {
        if let Some(img) = self.images.get(name) {
            return Ok(img.clone());
        }
        let (archive, shadows) = match self.source {
            Source::MapLib => (&data.maplib, false),
            Source::Objects => (&data.objects, true),
        };
        let img = data.load_image(archive, name, shadows)?.map(Rc::new);
        self.images.insert(name.to_string(), img.clone());
        Ok(img)
    }
}

/// Finds a file in `dir` by case-insensitive name.
pub fn find_ci(dir: &Path, name: &str) -> Result<Option<PathBuf>> {
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        if item
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(name)
        {
            return Ok(Some(item.path()));
        }
    }
    Ok(None)
}
