//! Fog of war: what the human player has explored and sees right now.
//!
//! The original keeps two layers (`b_DARK` for unexplored cells, `b_FOG`
//! for explored ones out of sight; DarkTy, 0x48b3a0) drawn with dithered
//! edge masks. Here each map cell is unexplored, explored or in sight.
//! Ships and buildings see around them (0x50e7f0) through one of six
//! masks, radius 0-5 (0x48b200).

use super::*;
use crate::assets::Image;
use anyhow::Context;

/// What the player knows of a map cell.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Seen {
    Unexplored = 0,
    Explored = 1,
    InSight = 2,
}

/// The cells radius `r` (0-5) sees: rows of the mask's (2r+1)-wide square
/// as bit sets, bit `x` for column `x` (0x5b9bbc..).
fn mask(r: i32) -> &'static [u16] {
    const MASKS: [&[u16]; 6] = [
        &[0b1],
        &[0b111; 3],
        &[0b01110, 0b11111, 0b11111, 0b11111, 0b01110],
        &[
            0b0011100, 0b0111110, 0b1111111, 0b1111111, 0b1111111, 0b0111110, 0b0011100,
        ],
        &[
            0b000111000,
            0b011111110,
            0b011111110,
            0b111111111,
            0b111111111,
            0b111111111,
            0b011111110,
            0b011111110,
            0b000111000,
        ],
        &[
            0b00011111000,
            0b00111111100,
            0b01111111110,
            0b11111111111,
            0b11111111111,
            0b11111111111,
            0b11111111111,
            0b11111111111,
            0b01111111110,
            0b00111111100,
            0b00011111000,
        ],
    ];
    MASKS[r.clamp(0, 5) as usize]
}

/// How far a ship sees: by type (0x4e5910) or its crew's longest weapon
/// range (0x4bc360, ship +0x47d), whichever is more; fire ships and quick
/// lime ships see 3. Checked against the original's sight counts in
/// miss000 (DarkTy layer 2): boats and the men killer 3, buildings 2.
fn ship_sight(o: &MapObject, levels: &crate::combat::Levels) -> i32 {
    let Some(ship) = &o.ship else { return 0 };
    let base = match ship.kind {
        0 | 1 | 8 | 10 => 1,
        2 | 3 => 3,
        _ => 2,
    };
    let crew = if matches!(ship.kind, 8 | 9) {
        3
    } else {
        o.shooters()
            .iter()
            .map(|&w| crate::combat::range_cells(w, levels.reach_for(w)).1 as i32)
            .max()
            .unwrap_or(0)
    };
    base.max(crew)
}

/// A building's sight radius (the constructors' +0x1b0) and footprint
/// (+0x56 cells, two to a row, from its cell).
fn building_sight(class: u32) -> (i32, &'static [(i32, i32)]) {
    const ONE: &[(i32, i32)] = &[(0, 0)];
    const FOUR: &[(i32, i32)] = &[(0, 0), (1, 0), (0, 1), (1, 1)];
    match class {
        0x32 => (2, FOUR),
        0x33 => (3, FOUR),
        0x34..=0x36 => (4, ONE),
        0x3c..=0x3e => (3, ONE),
        _ => (2, ONE),
    }
}
/// Seconds between updates of what is in sight.
pub const SIGHT_PERIOD: f32 = 0.25;

/// The fog's art: the cloud picture over unexplored cells (`<n>OBL_BIG`,
/// 9 x 8 cells, tiled over the map; `n` from the mission's `N_CLOUD`), and
/// 64x64 edge masks by code: `b_DARK<code>` where the clouds show,
/// `b_FOG<code>` where black is stippled over explored cells out of sight.
pub struct FogArt {
    pub cloud: Image,
    pub dark: Vec<openconquest_formats::Mask>,
    pub fog: Vec<openconquest_formats::Mask>,
    /// The minimap's: `<n>OBL_SM` and the 8x8 `s_DARK`/`s_FOG` masks.
    pub small_cloud: Option<Image>,
    pub small_dark: Vec<openconquest_formats::Mask>,
    pub small_fog: Vec<openconquest_formats::Mask>,
    /// The minimap's object icons (`LMAP8X8`, `LMAP16X16`: columns of
    /// 8x8 and 16x16 icons), if the menus archive is there.
    pub icons: Option<(Image, Image)>,
}

impl FogArt {
    pub fn load(data: &GameData, archive: &openconquest_formats::Archive) -> Result<Self> {
        let theme = archive
            .find("N_CLOUD", openconquest_formats::ResourceKind::Record)
            .and_then(|e| archive.read_entry(e).ok())
            .and_then(|b| {
                b.get(..4)
                    .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            })
            .unwrap_or(0);
        let lib = &data.maplib;
        let cloud = data
            .load_image(lib, &format!("{theme}OBL_BIG"), false)?
            .or(data.load_image(lib, "0OBL_BIG", false)?)
            .context("no OBL_BIG in maplib")?;
        let masks = |prefix: &str| -> Result<Vec<openconquest_formats::Mask>> {
            (0..16)
                .map(|i| {
                    let e = lib
                        .find(
                            &format!("{prefix}{i:02}"),
                            openconquest_formats::ResourceKind::Mask,
                        )
                        .with_context(|| format!("no {prefix}{i:02} in maplib"))?;
                    Ok(openconquest_formats::Mask::parse(
                        &lib.read_entry(e)?,
                        e.width.into(),
                        e.height.into(),
                    )?)
                })
                .collect()
        };
        Ok(Self {
            cloud,
            dark: masks("b_DARK")?,
            fog: masks("b_FOG")?,
            small_cloud: data
                .load_image(lib, &format!("{theme}OBL_SM"), false)?
                .or(data.load_image(lib, "0OBL_SM", false)?),
            small_dark: masks("s_DARK").unwrap_or_default(),
            small_fog: masks("s_FOG").unwrap_or_default(),
            icons: data.menus.as_ref().and_then(|m| {
                let small = data.load_image(m, "LMAP8X8", false).ok().flatten()?;
                let big = data.load_image(m, "LMAP16X16", false).ok().flatten()?;
                Some((small, big))
            }),
        })
    }
}

/// Edge codes (DarkTy 0x48c520): every cell `on` marks, and its eight
/// neighbours, become 15 (clear), then each neighbour takes the code its
/// own neighbours give it (0x48c1d0, 0x48c470). Cells left alone stay 0
/// (covered). With `skip`, cells already clear are passed over, as the
/// explored layer is (0x48c5d0). Raster order reproduces the original's
/// explored layer at the start of miss000 exactly.
pub fn edge_codes(w: i32, h: i32, on: impl Fn(i32, i32) -> bool, skip: bool) -> Vec<u8> {
    let mut c = vec![0u8; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            if !on(x, y) || (skip && c[(y * w + x) as usize] == 15) {
                continue;
            }
            for (dx, dy) in std::iter::once((0, 0)).chain(NEIGHBOURS) {
                let (nx, ny) = (x + dx, y + dy);
                if (0..w).contains(&nx) && (0..h).contains(&ny) {
                    c[(ny * w + nx) as usize] = 15;
                }
            }
            for (dx, dy) in NEIGHBOURS {
                let (nx, ny) = (x + dx, y + dy);
                if (0..w).contains(&nx) && (0..h).contains(&ny) {
                    c[(ny * w + nx) as usize] = code_of(edge_bits(&c, w, h, nx, ny));
                }
            }
        }
    }
    c
}

/// The eight neighbours in the original's order (0x5bded8).
const NEIGHBOURS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];

/// What a neighbour with code `c` (below 15), in direction `d` (in
/// `NEIGHBOURS` order), adds to a cell's edge bits (0x5bdef8): bits 8-11
/// the corners, 0-7 the half sides.
const NEIGHBOUR_BITS: [[u16; 8]; 15] = [
    [0x400, 0xc00, 0x800, 0x900, 0x100, 0x300, 0x200, 0x600],
    [0x400, 0x810, 0, 0, 0, 0, 0, 0x208],
    [0, 0x420, 0x800, 0x140, 0, 0, 0, 0],
    [0, 0, 0, 0x880, 0x100, 0x201, 0, 0],
    [0, 0, 0, 0, 0, 0x102, 0x200, 0x404],
    [0x400, 0xc00, 0x800, 0x140, 0, 0, 0, 0x208],
    [0, 0x420, 0x800, 0x900, 0x100, 0x201, 0, 0],
    [0, 0, 0, 0x880, 0x100, 0x300, 0x200, 0x404],
    [0x400, 0x810, 0, 0, 0, 0x102, 0x200, 0x600],
    [0x400, 0xc00, 0x800, 0x140, 0, 0x102, 0x200, 0x600],
    [0x400, 0xc00, 0x800, 0x900, 0x100, 0x201, 0, 0x208],
    [0, 0x420, 0x800, 0x900, 0x100, 0x300, 0x200, 0x404],
    [0x400, 0x810, 0, 0x880, 0x100, 0x300, 0x200, 0x600],
    [0, 0x420, 0x800, 0x140, 0, 0x102, 0x200, 0x404],
    [0x400, 0x810, 0, 0x880, 0x100, 0x201, 0, 0x208],
];

/// The edge bits of each code 0-14 (0x5bdeb8).
const CODE_BITS: [u16; 15] = [
    0xf00, 0x142, 0x209, 0x424, 0x890, 0x348, 0x621, 0xc84, 0x912, 0xb18, 0x760, 0xe81, 0xd06,
    0xa99, 0x566,
];

/// A cell's edge bits from its neighbours' codes (0x48c1d0). On the map's
/// border, a side with only one of its two corners covered loses its half
/// sides.
fn edge_bits(c: &[u8], w: i32, h: i32, x: i32, y: i32) -> u16 {
    let mut u = 0u16;
    let mut at = |dx: i32, dy: i32, d: usize| {
        let v = c[((y + dy) * w + x + dx) as usize];
        if v < 15 {
            u |= NEIGHBOUR_BITS[v as usize][d];
        }
    };
    let mut borders = Vec::new();
    if x < 1 {
        borders.push((0x400, 0x800));
    } else {
        if y > 0 {
            at(-1, -1, 0);
        }
        at(-1, 0, 1);
    }
    if x > 0 && y < h - 1 {
        at(-1, 1, 2);
    }
    if y < h - 1 {
        at(0, 1, 3);
    } else {
        borders.push((0x800, 0x100));
    }
    if x < w - 1 && y < h - 1 {
        at(1, 1, 4);
    }
    if x < w - 1 {
        at(1, 0, 5);
    } else {
        borders.push((0x100, 0x200));
    }
    if y > 0 {
        if x < w - 1 {
            at(1, -1, 6);
        }
        at(0, -1, 7);
    } else {
        borders.push((0x200, 0x400));
    }
    for (a, b) in borders {
        if (u & a != 0) != (u & b != 0) {
            u &= 0xff00;
        }
    }
    u
}

/// The code for edge bits (0x48c470): none is 15 (clear), else the code
/// with exactly those bits, else the first with the same corners.
fn code_of(u: u16) -> u8 {
    if u == 0 {
        return 15;
    }
    CODE_BITS
        .iter()
        .position(|&e| e == u)
        .or_else(|| CODE_BITS.iter().position(|&e| (e ^ u) & 0xf00 == 0))
        .map_or(255, |i| i as u8)
}

impl Scene {
    /// What the player knows of cell `(cx, cy)`; everything is in sight
    /// when fog of war is off.
    pub fn seen(&self, cx: i32, cy: i32) -> Seen {
        if !self.fogged() {
            return Seen::InSight;
        }
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        if cx < 0 || cy < 0 || cx >= w || cy >= h {
            return Seen::Unexplored;
        }
        self.sight[(cy * w + cx) as usize]
    }

    /// Whether the player can see object `o` now: their own always, other
    /// players' ships and creatures only in sight, buildings and scenery
    /// once explored.
    pub fn is_visible(&self, o: &MapObject) -> bool {
        if self.shoal_gone(o.id) || self.creature_hidden(o.id) || self.hazard_hidden(o.id) {
            return false;
        }
        // A leviathan caught is below until it comes up again.
        if o.class == ObjectClass::WhaleZone && o.deposit.is_none() {
            return false;
        }
        if !self.fogged() || self.is_own(o) {
            return true;
        }
        let t = TILE_SIZE as f32;
        let (cx, cy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let moves = o.ship.is_some() || matches!(o.class, ObjectClass::Monster(_));
        if moves {
            self.seen(cx, cy) == Seen::InSight
        } else {
            self.seen(cx, cy) >= Seen::Explored
        }
    }

    /// Whether the fog is drawn: fog of war on and a viewing player
    /// (DarkTy, Grig\dark.cpp 0x48bbd0: the overlay is attached to the
    /// screen, its clouds redrawn and the minimap's fog made only when
    /// `[game+0x3212]` != 0xff; with no viewer the whole map shows).
    pub fn fogged(&self) -> bool {
        self.fog && self.human.is_some()
    }

    /// Recomputes what is in sight from the player's ships and buildings.
    pub(super) fn update_sight(&mut self) {
        self.refresh_sight();
    }

    /// The fog's edge codes of cell `(cx, cy)`: explored (clouds) and in
    /// sight (black stipple), 15 for clear.
    pub fn fog_codes(&self, cx: i32, cy: i32) -> (u8, u8) {
        let w = self.nav.width as i32;
        if cx < 0 || cy < 0 || cx >= w || cy >= self.nav.height as i32 {
            return (0, 0);
        }
        let i = (cy * w + cx) as usize;
        (
            self.dark_codes.get(i).copied().unwrap_or(15),
            self.fog_codes.get(i).copied().unwrap_or(15),
        )
    }

    /// Recomputes the fog's edge codes from what is explored and in sight.
    fn refresh_codes(&mut self) {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let sight = &self.sight;
        let at = |x: i32, y: i32| sight[(y * w + x) as usize];
        self.dark_codes = edge_codes(w, h, |x, y| at(x, y) != Seen::Unexplored, true);
        self.fog_codes = edge_codes(w, h, |x, y| at(x, y) == Seen::InSight, false);
    }

    /// Recomputes what the active player sees.
    pub fn refresh_sight(&mut self) {
        if !self.fogged() || self.sight.is_empty() {
            return;
        }
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        for s in &mut self.sight {
            if *s == Seen::InSight {
                *s = Seen::Explored;
            }
        }
        let t = TILE_SIZE as f32;
        let mut eyes: Vec<(i32, i32, i32)> = Vec::new();
        for o in self
            .objects
            .iter()
            .filter(|o| self.is_own(o) && o.combat.is_some() && o.asleep.is_none())
        {
            if o.ship.is_some() {
                let levels = self.levels_of(o.owner);
                eyes.push((
                    (o.x / t).floor() as i32,
                    (o.y / t).floor() as i32,
                    ship_sight(o, &levels),
                ));
            } else if let (ObjectClass::Building(class), Some((cx, cy))) = (o.class, o.cell) {
                let (r, cells) = building_sight(class);
                eyes.extend(cells.iter().map(|&(dx, dy)| (cx + dx, cy + dy, r)));
            }
        }
        // Spells' passing sight (dark.cpp 0x48b9e0).
        eyes.extend(self.sight_points.iter().map(|p| (p.0, p.1, p.2)));
        for (cx, cy, r) in eyes {
            let r = r.clamp(0, 5);
            for (row, bits) in mask(r).iter().enumerate() {
                let y = cy - r + row as i32;
                for col in 0..=2 * r {
                    let x = cx - r + col;
                    if bits >> col & 1 == 1 && (0..w).contains(&x) && (0..h).contains(&y) {
                        self.sight[(y * w + x) as usize] = Seen::InSight;
                    }
                }
            }
        }
        self.refresh_codes();
    }

    /// Lets the player see within 2 cells of `(cx, cy)` for 3000 ticks
    /// (the View, lightning and fireball casts: dark.cpp 0x48b9e0 with
    /// radius 2, gone at 0x48bae0).
    pub(super) fn add_sight_point(&mut self, cx: i32, cy: i32) {
        self.sight_points
            .push((cx, cy, 2, crate::clock::ticks(3000)));
        self.refresh_sight();
    }

    /// Sight points wear off.
    pub(super) fn run_sight_points(&mut self, dt: f32) {
        let before = self.sight_points.len();
        for p in &mut self.sight_points {
            p.3 -= dt;
        }
        self.sight_points.retain(|p| p.3 > 0.0);
        if self.sight_points.len() != before {
            self.refresh_sight();
        }
    }

    /// Lifts the darkness from the whole map.
    #[cfg(test)]
    pub fn explore_all(&mut self) {
        for s in &mut self.sight {
            if *s == Seen::Unexplored {
                *s = Seen::Explored;
            }
        }
        self.refresh_codes();
    }
}
