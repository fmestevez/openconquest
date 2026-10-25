//! `.DKX` / `.DKD` resource archives.
//!
//! An archive is a pair of files: the `.DKX` index (magic `DKBT`, a B-tree of
//! named entries) and the `.DKD` data file (magic `DKFM`) holding the payloads.
//! Entries are keyed by `(name, kind)`; the same name commonly exists with
//! several kinds (e.g. a terrain tile as a bitmap, a mask and a sprite).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::bytes::Reader;
use crate::{invalid, lz, Error, Result};

const INDEX_MAGIC: &[u8; 4] = b"DKBT";
const DATA_MAGIC: &[u8; 4] = b"DKFM";
const PAGE_HEADER: usize = 16;
const NO_PAGE: u32 = 0xffff_ffff;

/// The type tag stored with every archive entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ResourceKind {
    /// Untyped binary record (mission headers, descriptors).
    Raw,
    /// A Windows DIB (`BITMAPINFOHEADER` + palette + pixels).
    Bitmap,
    /// Audio sample.
    Sound,
    /// Bitmap font.
    Font,
    /// RLE-compressed palettized sprite.
    Sprite,
    /// Animation/sequence descriptor.
    Sequence,
    /// Layered animation (frames x layers of sprites).
    Composite,
    /// Game-state record (mission objects, settings).
    Record,
    /// Palette animation table.
    PaletteAnimation,
    /// Terrain tile set library.
    TileSetLibrary,
    /// Map cell grid.
    MapGrid,
    /// 1-bit transparency mask.
    Mask,
    /// Text or script data.
    Text,
    Other(u8),
}

impl ResourceKind {
    pub fn from_tag(tag: u8) -> Self {
        match tag {
            0 => Self::Raw,
            1 => Self::Bitmap,
            2 => Self::Sound,
            3 => Self::Font,
            6 => Self::Sprite,
            7 => Self::Sequence,
            8 => Self::Composite,
            12 => Self::Record,
            13 => Self::PaletteAnimation,
            16 => Self::TileSetLibrary,
            17 => Self::MapGrid,
            18 => Self::Mask,
            23 => Self::Text,
            other => Self::Other(other),
        }
    }

    pub fn tag(self) -> u8 {
        match self {
            Self::Raw => 0,
            Self::Bitmap => 1,
            Self::Sound => 2,
            Self::Font => 3,
            Self::Sprite => 6,
            Self::Sequence => 7,
            Self::Composite => 8,
            Self::Record => 12,
            Self::PaletteAnimation => 13,
            Self::TileSetLibrary => 16,
            Self::MapGrid => 17,
            Self::Mask => 18,
            Self::Text => 23,
            Self::Other(t) => t,
        }
    }
}

/// One entry of an archive index.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub kind: ResourceKind,
    /// Offset of the payload in the `.DKD` file.
    pub offset: u32,
    /// Size of the payload as stored.
    pub stored_size: u32,
    pub compressed: bool,
    /// Size of the payload after decompression.
    pub size: u32,
    /// Width for images; meaning varies for other kinds.
    pub width: u16,
    /// Height for images; meaning varies for other kinds.
    pub height: u16,
    /// Kind-specific parameters, not yet fully understood.
    pub param_a: u32,
    pub param_b: u32,
}

/// An opened `.DKX` / `.DKD` archive pair.
pub struct Archive {
    path: PathBuf,
    data: memmap2::Mmap,
    entries: Vec<Entry>,
    by_key: HashMap<(String, ResourceKind), usize>,
}

impl std::fmt::Debug for Archive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Archive")
            .field("path", &self.path)
            .field("entries", &self.entries.len())
            .finish()
    }
}

impl Archive {
    /// Opens an archive given the path of either file of the pair, or the
    /// path without extension. Extension case is matched against the files on
    /// disk.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let index_path = sibling_with_ext(path, "dkx")?;
        let data_path = sibling_with_ext(path, "dkd")?;
        let index = std::fs::read(&index_path)?;
        let file = File::open(&data_path)?;
        // SAFETY: the game data is treated as read-only; we never write to it,
        // and concurrent modification of the files by another process while
        // the game runs is not supported.
        let data = unsafe { memmap2::Mmap::map(&file)? };
        Self::from_parts(data_path, &index, data)
    }

    fn from_parts(path: PathBuf, index: &[u8], data: memmap2::Mmap) -> Result<Self> {
        if data.get(..4) != Some(DATA_MAGIC) {
            return Err(invalid("DKD file", "bad magic"));
        }
        let entries = parse_index(index)?;
        let mut by_key = HashMap::with_capacity(entries.len());
        for (i, e) in entries.iter().enumerate() {
            by_key
                .entry((e.name.to_ascii_uppercase(), e.kind))
                .or_insert(i);
        }
        Ok(Self {
            path,
            data,
            entries,
            by_key,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All entries in index (B-tree in-order) order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Finds an entry by name (case-insensitive) and kind.
    pub fn find(&self, name: &str, kind: ResourceKind) -> Option<&Entry> {
        self.by_key
            .get(&(name.to_ascii_uppercase(), kind))
            .map(|&i| &self.entries[i])
    }

    /// Returns the kinds stored under `name`.
    pub fn kinds_of<'a>(&'a self, name: &'a str) -> impl Iterator<Item = ResourceKind> + 'a {
        self.entries
            .iter()
            .filter(move |e| e.name.eq_ignore_ascii_case(name))
            .map(|e| e.kind)
    }

    /// Reads (and decompresses if needed) an entry's payload.
    pub fn read_entry(&self, e: &Entry) -> Result<Vec<u8>> {
        let start = e.offset as usize;
        let end = start + e.stored_size as usize;
        let raw = self.data.get(start..end).ok_or_else(|| {
            invalid(
                "archive entry",
                format!("{} lies outside the data file", e.name),
            )
        })?;
        if !e.compressed {
            return Ok(raw.to_vec());
        }
        let out = lz::decompress(raw, e.size as usize)?;
        if out.data.len() != e.size as usize {
            return Err(invalid(
                "archive entry",
                format!(
                    "{}: decompressed {} bytes, expected {}",
                    e.name,
                    out.data.len(),
                    e.size
                ),
            ));
        }
        Ok(out.data)
    }

    /// The raw data file from `offset` to its end. Some resources (font
    /// atlases) are referenced by offset rather than by name.
    pub fn data_at(&self, offset: u32) -> Option<&[u8]> {
        self.data.get(offset as usize..)
    }

    /// Reads a resource by name and kind.
    pub fn read(&self, name: &str, kind: ResourceKind) -> Result<Vec<u8>> {
        let e = self.find(name, kind).ok_or_else(|| {
            Error::NotFound(format!("{name} ({kind:?}) in {}", self.path.display()))
        })?;
        self.read_entry(e)
    }
}

/// Finds `<stem>.<ext>` next to `path`, trying lower and upper case extensions.
fn sibling_with_ext(path: &Path, ext: &str) -> Result<PathBuf> {
    let stem = match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("dkx") || e.eq_ignore_ascii_case("dkd") => {
            path.with_extension("")
        }
        _ => path.to_path_buf(),
    };
    for candidate in [ext.to_ascii_lowercase(), ext.to_ascii_uppercase()] {
        let p = stem.with_extension(&candidate);
        if p.exists() {
            return Ok(p);
        }
    }
    // Fall back to a case-insensitive directory scan.
    let dir = stem
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let want = format!(
        "{}.{}",
        stem.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        ext
    );
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        if item
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(&want)
        {
            return Ok(item.path());
        }
    }
    Err(Error::NotFound(
        stem.with_extension(ext).display().to_string(),
    ))
}

/// Parses a `.DKX` B-tree index into a flat, in-order entry list.
pub fn parse_index(index: &[u8]) -> Result<Vec<Entry>> {
    if index.get(..4) != Some(INDEX_MAGIC) {
        return Err(invalid("DKX index", "bad magic"));
    }
    let root = Reader::at(index, 0x24, "DKX header").u32()?;
    let mut entries = Vec::new();
    let mut visited = HashSet::new();
    walk_page(index, root, &mut entries, &mut visited)?;
    Ok(entries)
}

fn walk_page(
    index: &[u8],
    page: u32,
    out: &mut Vec<Entry>,
    visited: &mut HashSet<u32>,
) -> Result<()> {
    if !visited.insert(page) {
        return Err(invalid("DKX index", format!("cycle at page {page:#x}")));
    }
    let mut r = Reader::at(index, page as usize, "DKX page");
    let _parent = r.u32()?;
    let leftmost = r.u32()?;
    let used = r.u16()? as usize;
    let end = page as usize + PAGE_HEADER + used;
    r = Reader::at(index, page as usize + PAGE_HEADER, "DKX page");
    if leftmost != NO_PAGE {
        walk_page(index, leftmost, out, visited)?;
    }
    while r.pos() < end {
        let child = r.u32()?;
        let offset = r.u32()?;
        let kind = ResourceKind::from_tag(r.u8()?);
        let stored_size = r.u32()?;
        let compressed = r.u8()? != 0;
        let size = r.u32()?;
        let width = r.u16()?;
        let height = r.u16()?;
        let param_a = r.u32()?;
        let param_b = r.u32()?;
        let name_len = r.u16()? as usize;
        let name = r.bytes(name_len)?.iter().map(|&b| b as char).collect();
        out.push(Entry {
            name,
            kind,
            offset,
            stored_size,
            compressed,
            size,
            width,
            height,
            param_a,
            param_b,
        });
        if child != NO_PAGE {
            walk_page(index, child, out, visited)?;
        }
    }
    Ok(())
}
