//! Lightning digs land into sea (0x5030c0, the map editor's erase run on
//! the struck cell): the cell's ground goes, the coast about it is cut to
//! match, and the shelf about it goes deep (0x502550, 0x5028b0, 0x502a80,
//! 0x5029d0). Then the ships' ways are rebuilt (0x49a950).

use openconquest_formats::map::TileRef;

use super::*;

/// The eight directions, in the editor's order (0x5bded8).
const DIRS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];

/// A tile shape's edge bits (0x5bdeb8).
const SHAPE: [u16; 15] = [
    0xf00, 0x142, 0x209, 0x424, 0x890, 0x348, 0x621, 0xc84, 0x912, 0xb18, 0x760, 0xe81, 0xd06,
    0xa99, 0x566,
];

/// What a neighbour of each shape, in each direction, asks of a cell
/// (0x5bdef8).
const EDGE: [[u16; 8]; 15] = [
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

/// The same of land about a shelf cell, as the shelf is laid again
/// (0x5bdfe8).
const EDGE2: [[u16; 8]; 15] = [
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0x400, 0x800, 0x900, 0x100, 0x300, 0x200, 0x400],
    [0x400, 0x800, 0, 0x800, 0x100, 0x300, 0x200, 0x600],
    [0x400, 0xc00, 0x800, 0x900, 0x100, 0x100, 0x200, 0x600],
    [0x400, 0xc00, 0x800, 0x900, 0x100, 0x200, 0, 0x200],
    [0, 0, 0, 0x800, 0x100, 0x300, 0x200, 0x400],
    [0x400, 0x800, 0, 0, 0, 0x100, 0x200, 0x600],
    [0x400, 0xc00, 0x800, 0x100, 0, 0, 0, 0x200],
    [0, 0x400, 0x800, 0x900, 0x100, 0x200, 0, 0],
    [0, 0, 0, 0x800, 0x100, 0x200, 0, 0],
    [0, 0, 0, 0, 0, 0x100, 0x200, 0x400],
    [0x400, 0x800, 0, 0, 0, 0, 0, 0x200],
    [0, 0x400, 0x800, 0x100, 0, 0, 0, 0],
    [0x400, 0x800, 0, 0x800, 0x100, 0x200, 0, 0x200],
    [0, 0x400, 0x800, 0x100, 0, 0x100, 0x200, 0x400],
];

/// The tile kinds' tables (record byte 1): a variant's shape, and each
/// shape's variants, one picked at random (0x501bb0-0x501cb0).
/// Kind 4: a variant's shape (0x5be0d6).
const SHAPE_4: [i16; 45] = [
    1024, 0, 1, 1, 1, 1, 1, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 9, 9, 12, 12, 10, 10, 11,
    11, 5, 5, 7, 7, 8, 8, 8, 6, 6, 6, 6, 6, 13, 14, 0,
];
/// Kind 4: each shape's variants (0x5be150, counts 0x5be130).
const VARIANTS_4: [&[u8]; 15] = [
    &[1],
    &[2, 3, 4, 5, 6],
    &[12, 13, 14, 15, 16],
    &[17, 18, 19, 20, 21],
    &[7, 8, 9, 10, 11],
    &[30, 31],
    &[37, 38, 39, 40, 41],
    &[32, 33],
    &[34, 35, 36],
    &[22, 23],
    &[26, 27],
    &[28, 29],
    &[24, 25],
    &[42],
    &[43],
];
/// Kind 5: a variant's shape (0x5be1e6).
const SHAPE_5: [i16; 35] = [
    0, 0, 1, 1, 1, 4, 4, 4, 2, 2, 2, 3, 3, 3, 9, 9, 12, 12, 10, 10, 11, 11, 5, 5, 7, 7, 8, 8, 8, 6,
    6, 6, 13, 14, 0,
];
/// Kind 5: each shape's variants (0x5be24c, counts 0x5be22c).
const VARIANTS_5: [&[u8]; 15] = [
    &[1],
    &[2, 3, 4],
    &[8, 9, 10],
    &[11, 12, 13],
    &[5, 6, 7],
    &[22, 23],
    &[29, 30, 31],
    &[24, 25],
    &[26, 27, 28],
    &[14, 15],
    &[18, 19],
    &[20, 21],
    &[16, 17],
    &[32],
    &[33],
];
/// Kind 6: a variant's shape (0x5be2a6).
const SHAPE_6: [i16; 31] = [
    0, 0, 1, 1, 1, 4, 4, 4, 2, 2, 2, 3, 3, 3, 9, 12, 10, 11, 5, 5, 7, 7, 8, 8, 8, 6, 6, 6, 13, 14,
    0,
];
/// Kind 6: each shape's variants (0x5be304, counts 0x5be2e4).
const VARIANTS_6: [&[u8]; 15] = [
    &[1],
    &[2, 3, 4],
    &[8, 9, 10],
    &[11, 12, 13],
    &[5, 6, 7],
    &[18, 19],
    &[25, 26, 27],
    &[20, 21],
    &[22, 23, 24],
    &[14],
    &[16],
    &[17],
    &[15],
    &[28],
    &[29],
];
/// Kind 7: a variant's shape (0x5be35e).
const SHAPE_7: [i16; 23] = [
    0, 1, 4, 8, 8, 2, 3, 10, 11, 1, 4, 9, 12, 2, 3, 6, 6, 0, 5, 7, 13, 14, 0,
];
/// Kind 7: each shape's variants (0x5be3ac, counts 0x5be38c).
const VARIANTS_7: [&[u8]; 15] = [
    &[17],
    &[1, 9],
    &[5, 13],
    &[6, 14],
    &[2, 10],
    &[18],
    &[15, 16],
    &[19],
    &[3, 4],
    &[11],
    &[7],
    &[8],
    &[12],
    &[20],
    &[21],
];
/// Kind 8: a variant's shape (0x5be3e6).
const SHAPE_8: [i16; 25] = [
    0, 0, 6, 8, 5, 7, 2, 3, 1, 4, 12, 9, 11, 10, 2, 3, 1, 4, 5, 7, 6, 8, 13, 14, 0,
];
/// Kind 8: each shape's variants (0x5be438, counts 0x5be418).
const VARIANTS_8: [&[u8]; 15] = [
    &[1],
    &[8, 16],
    &[6, 14],
    &[7, 15],
    &[9, 17],
    &[4, 18],
    &[2, 20],
    &[5, 19],
    &[3, 21],
    &[11],
    &[13],
    &[12],
    &[10],
    &[22],
    &[23],
];
/// The directions 0x502b20 looks at a border cell's outer neighbours.
const EDGE_TOP: [usize; 3] = [6, 7, 0];
const EDGE_LEFT: [usize; 3] = [0, 1, 2];
const EDGE_BOTTOM: [usize; 3] = [2, 3, 4];
const EDGE_RIGHT: [usize; 3] = [4, 5, 6];

const SEA: u8 = 0;
const SHELF: u8 = 1;
/// The ground's cell type, shut to ships; the shelf's.
const GROUND_TYPE: u32 = 8;
const SHELF_TYPE: u32 = 1;

/// The shape of variant `v` of a tile set of kind `kind` (0x501cf0).
fn shape_of(v: u8, kind: u8) -> Option<usize> {
    if v == 0 {
        return None;
    }
    let table: &[i16] = match kind {
        0 => return Some(0),
        4 => &SHAPE_4,
        5 => &SHAPE_5,
        6 => &SHAPE_6,
        7 => &SHAPE_7,
        8 => &SHAPE_8,
        _ => return None,
    };
    let s = *table.get(v as usize)?;
    (0..15).contains(&s).then_some(s as usize)
}

/// A random variant of shape `shape` of kind `kind` (0x501dc0).
fn pick(shape: usize, kind: u8, rng: &mut Rng) -> Option<u8> {
    let lists: &[&[u8]; 15] = match kind {
        4 => &VARIANTS_4,
        5 => &VARIANTS_5,
        6 => &VARIANTS_6,
        7 => &VARIANTS_7,
        8 => &VARIANTS_8,
        _ => return None,
    };
    let list = lists.get(shape)?;
    (!list.is_empty()).then(|| list[rng.next() as usize % list.len()])
}

/// A tile set's record: its id (byte 0), kind (byte 1) and cell type
/// (bytes 4-7).
#[derive(Clone, Copy)]
struct Rec {
    id: u8,
    kind: u8,
    ty: u32,
}

/// A dig on the scene's grid (the map editor's erase, 0x502e60).
struct Dig<'a> {
    grid: &'a mut MapGrid,
    lib: &'a openconquest_formats::TileSetLibrary,
    rng: &'a mut Rng,
    /// The edges a border cell's outer neighbours give it (0x5ca798: by
    /// direction, the bits and the cell), set by 0x502da0.
    border: [(u16, i32, i32); 9],
    changed: Vec<(u32, u32, u32, TileRef)>,
}

impl Dig<'_> {
    fn rec(&self, id: u8) -> Option<Rec> {
        self.lib.by_id(id).map(|s| Rec {
            id: s.raw[0],
            kind: s.raw[1],
            ty: u32::from_le_bytes([s.raw[4], s.raw[5], s.raw[6], s.raw[7]]),
        })
    }

    fn inside(&self, (x, y): (i32, i32)) -> bool {
        x >= 0 && y >= 0 && x < self.grid.width as i32 && y < self.grid.height as i32
    }

    /// On the ring one in from the map's side.
    fn on_border(&self, (x, y): (i32, i32)) -> bool {
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        x == 1 || y == 1 || x == w - 2 || y == h - 2
    }

    fn at(&self, layer: u32, (x, y): (i32, i32)) -> TileRef {
        self.grid.get(layer, x as u32, y as u32)
    }

    fn put(&mut self, layer: u32, (x, y): (i32, i32), tile: TileRef) {
        self.grid.set(layer, x as u32, y as u32, tile);
        self.changed.push((layer, x as u32, y as u32, tile));
    }

    /// Nothing to dig: layer 0 water or bare, another layer empty
    /// (0x5024f0).
    fn bare(&self, layer: u32, c: (i32, i32)) -> bool {
        let t = self.at(layer, c);
        t.variant == 0 || (layer == 0 && (t.tileset == SEA || t.tileset == SHELF))
    }

    /// Open sea on layer 0, nothing on the others (0x502350).
    fn erase(&mut self, layer: u32, c: (i32, i32)) {
        let t = if layer == 0 {
            TileRef {
                variant: 1,
                tileset: SEA,
            }
        } else {
            TileRef {
                variant: 0,
                tileset: 0,
            }
        };
        self.put(layer, c, t);
    }

    /// The edges cell `c`'s neighbours ask of it as a tile of `set`
    /// (0x501e70; `border` if the cell being dug lies on the map's border
    /// ring). Off the map's side, the edges are made up from its own, or
    /// for a ring cell's neighbours, the border's given.
    fn wants(&self, layer: u32, c: (i32, i32), set: Rec, border: bool) -> Option<u16> {
        let mut m = 0u16;
        let mut off = Vec::new();
        for (k, (dx, dy)) in DIRS.iter().enumerate() {
            let n = (c.0 + dx, c.1 + dy);
            if !self.inside(n) {
                if k & 1 == 1 {
                    off.push(k);
                }
                continue;
            }
            let t = self.at(layer, n);
            if t.variant == 0 {
                continue;
            }
            let rn = self.rec(t.tileset)?;
            if rn.id == set.id {
                m |= EDGE[shape_of(t.variant, set.kind)?][k];
            }
            if layer == 0 && set.ty == SHELF_TYPE && rn.ty == GROUND_TYPE {
                m |= EDGE2[shape_of(t.variant, rn.kind)?][k];
            }
        }
        for k in off {
            if !border || set.id == SHELF {
                let (a, b, lone, other) = match k {
                    1 => (0x400, 0x800, 0x20, 0x10),
                    3 => (0x800, 0x100, 0x80, 0x40),
                    5 => (0x100, 0x200, 2, 1),
                    _ => (0x200, 0x400, 8, 4),
                };
                if m & a != 0 {
                    if m & b == 0 {
                        m |= lone;
                    }
                } else if m & b != 0 {
                    m |= other;
                }
            } else {
                for &(bits, x, y) in &self.border {
                    if (x, y) == c {
                        m |= bits;
                    }
                }
            }
        }
        Some(m)
    }

    /// Whether cell `c`'s tile already fits (0x502270).
    fn fits(&self, layer: u32, c: (i32, i32), set: Rec, border: bool) -> bool {
        let t = self.at(layer, c);
        let v = if layer == 0 && t.tileset == 0 {
            0
        } else {
            t.variant
        };
        let Some(m) = self.wants(layer, c, set, border) else {
            return false;
        };
        if v == 0 {
            return m == 0;
        }
        let shape = SHAPE.iter().position(|&s| s == m);
        shape.is_some() && shape == shape_of(v, set.kind)
    }

    /// Re-tiles cell `c` as `set` to fit about it (0x5027e0); false to
    /// stop.
    fn reshape(&mut self, layer: u32, c: (i32, i32), set: Rec, border: bool) -> bool {
        let Some(m) = self.wants(layer, c, set, border) else {
            return false;
        };
        if m == 0 || self.fits(layer, c, set, border) {
            return true;
        }
        if !(4..=8).contains(&set.kind) {
            return false;
        }
        let shape = SHAPE
            .iter()
            .position(|&s| s == m)
            .or_else(|| SHAPE.iter().position(|&s| (s ^ m) & 0xf00 == 0));
        match shape.and_then(|s| pick(s, set.kind, self.rng)) {
            Some(v) if v != 0 => {
                self.put(
                    layer,
                    c,
                    TileRef {
                        variant: v,
                        tileset: set.id,
                    },
                );
                true
            }
            _ => false,
        }
    }

    /// Re-tiles the cells about `c` as `set` (0x5028b0); on layer 0, shelf
    /// leaves the ground alone.
    fn around(&mut self, layer: u32, c: (i32, i32), set: Rec) -> bool {
        let border = self.on_border(c);
        for (dx, dy) in DIRS {
            let n = (c.0 + dx, c.1 + dy);
            if !self.inside(n) {
                continue;
            }
            if layer == 0 && set.ty == SHELF_TYPE {
                let Some(rn) = self.rec(self.at(0, n).tileset) else {
                    return false;
                };
                if rn.ty == GROUND_TYPE {
                    continue;
                }
            }
            if !self.reshape(layer, n, set, border) {
                return false;
            }
        }
        true
    }

    /// A border cell: the edges its outer neighbours give it (0x502da0,
    /// 0x502b20).
    fn note_border(&mut self, layer: u32, c: (i32, i32)) -> Option<()> {
        if !self.on_border(c) {
            return Some(());
        }
        self.border = [(0, 0, 0); 9];
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        let centre = self.rec(self.at(layer, c).tileset)?;
        let side = |dirs: [usize; 3], column: usize, turn: fn(u16) -> u16, dig: &mut Self| {
            for d in dirs {
                let n = (c.0 + DIRS[d].0, c.1 + DIRS[d].1);
                if !dig.inside(n) {
                    continue;
                }
                let t = dig.at(layer, n);
                let mut u = 0;
                if t.variant != 0 {
                    let rn = dig.rec(t.tileset)?;
                    if rn.id == centre.id {
                        u = EDGE[shape_of(t.variant, rn.kind)?][column];
                    }
                }
                if u == 0 {
                    continue;
                }
                let u = turn(u);
                if u != 0 {
                    dig.border[d] = (dig.border[d].0 | u, n.0, n.1);
                }
            }
            Some(())
        };
        if c.1 == 1 {
            side(
                EDGE_TOP,
                3,
                |u| {
                    if u & 0x40 != 0 {
                        0x208
                    } else if u & 0x80 != 0 {
                        0x404
                    } else {
                        0x600
                    }
                },
                self,
            )?;
        }
        if c.0 == 1 {
            side(
                EDGE_LEFT,
                5,
                |u| {
                    if u & 1 != 0 {
                        0x420
                    } else if u & 2 != 0 {
                        0x810
                    } else {
                        0xc00
                    }
                },
                self,
            )?;
        }
        if c.0 == w - 2 {
            side(
                EDGE_RIGHT,
                1,
                |u| {
                    if u & 0x10 != 0 {
                        0x102
                    } else if u & 0x20 != 0 {
                        0x201
                    } else {
                        0x300
                    }
                },
                self,
            )?;
        }
        if c.1 == h - 2 {
            side(
                EDGE_BOTTOM,
                7,
                |u| {
                    if u & 4 != 0 {
                        0x880
                    } else if u & 8 != 0 {
                        0x140
                    } else {
                        0x900
                    }
                },
                self,
            )?;
        }
        Some(())
    }

    /// Erases cell `c` on `layer` and what it held the edges of (0x502550).
    fn cut(&mut self, layer: u32, c: (i32, i32), rec: Rec) -> bool {
        let Some(shape) = shape_of(self.at(layer, c).variant, rec.kind) else {
            return false;
        };
        self.erase(layer, c);
        for (k, (dx, dy)) in DIRS.iter().enumerate() {
            let n = (c.0 + dx, c.1 + dy);
            if !self.inside(n) {
                continue;
            }
            let t = self.at(layer, n);
            if t.variant == 0 {
                continue;
            }
            let Some(rn) = self.rec(t.tileset) else {
                return false;
            };
            if rn.id != rec.id {
                continue;
            }
            let Some(ns) = shape_of(t.variant, rn.kind) else {
                return false;
            };
            if EDGE[ns][k] & SHAPE[shape] & 0xf00 != 0 {
                self.erase(layer, n);
            }
        }
        true
    }

    /// One layer of the erase on cell `c` (0x502e60): false to stop.
    fn layer(&mut self, layer: u32, c: (i32, i32)) -> bool {
        if self.bare(layer, c) {
            return true;
        }
        let Some(rec) = self.rec(self.at(layer, c).tileset) else {
            return false;
        };
        if rec.kind == 0 || rec.kind > 0x7f {
            self.erase(layer, c);
            return true;
        }
        if self.note_border(layer, c).is_none()
            || !self.cut(layer, c, rec)
            || !self.around(layer, c, rec)
        {
            return false;
        }
        if layer == 0 && rec.ty == GROUND_TYPE {
            // The shelf about it goes deep (0x502a80, 0x502720), then is
            // laid again about the coast (0x5029d0).
            for (dx, dy) in DIRS {
                let n = (c.0 + dx, c.1 + dy);
                if !self.inside(n) {
                    continue;
                }
                for (ex, ey) in DIRS {
                    let q = (n.0 + ex, n.1 + ey);
                    if self.inside(q)
                        && self
                            .rec(self.at(0, q).tileset)
                            .is_some_and(|r| r.id == SHELF)
                    {
                        self.erase(0, q);
                    }
                }
            }
            let Some(shelf) = self.rec(SHELF) else {
                return false;
            };
            for (dx, dy) in DIRS {
                let n = (c.0 + dx, c.1 + dy);
                if self.inside(n) && !self.around(0, n, shelf) {
                    return false;
                }
            }
        }
        true
    }

    /// Lightning's erase (0x5030c0 with layer 0): layers 0 and 2-7 in turn,
    /// stopping where one fails; then what lies loose on layers 5-7 within
    /// two goes, but on whole ground (0x502fb0).
    fn strike(&mut self, c: (i32, i32)) {
        let layers = self.grid.layers.min(8);
        for layer in (0..layers).filter(|&l| l != 1) {
            if !self.layer(layer, c) {
                return;
            }
        }
        for layer in 5..layers {
            for x in c.0 - 2..=c.0 + 2 {
                for y in c.1 - 2..=c.1 + 2 {
                    let q = (x, y);
                    if !self.inside(q) || self.bare(layer, q) {
                        continue;
                    }
                    let loose = self
                        .rec(self.at(layer, q).tileset)
                        .is_some_and(|r| r.kind == 0);
                    let ground = self.at(0, q);
                    let whole = ground.variant == 1
                        && self.rec(ground.tileset).is_some_and(|r| r.kind == 4);
                    if loose && !whole {
                        self.erase(layer, q);
                    }
                }
            }
        }
    }
}

impl Scene {
    /// Lightning on cell `(cx, cy)` digs it (0x5030c0 with layer 0): each
    /// layer but 1 erased and the coast about it cut to fit, layer 0's
    /// ground to sea; the ships' ways, the cells' types and the pictures
    /// follow.
    pub(super) fn dig(&mut self, cx: i32, cy: i32, data: &GameData, caches: &mut Caches) {
        let mut dig = Dig {
            grid: &mut self.grid,
            lib: &data.tilesets,
            rng: &mut self.gull_rng,
            border: [(0, 0, 0); 9],
            changed: Vec::new(),
        };
        if !dig.inside((cx, cy)) {
            return;
        }
        dig.strike((cx, cy));
        let changed = std::mem::take(&mut dig.changed);
        self.dug.extend(changed.iter().copied());
        self.after_dig(&changed, data, caches);
    }

    /// The ships' ways, the cells' types and the pictures after cells
    /// changed (a dig, or a saved one laid again).
    pub(super) fn after_dig(
        &mut self,
        changed: &[(u32, u32, u32, TileRef)],
        data: &GameData,
        caches: &mut Caches,
    ) {
        if changed.is_empty() {
            return;
        }
        let w = self.grid.width;
        for &(_, x, y, _) in changed {
            self.cell_types[(y * w + x) as usize] = cell_type(data, &self.grid, x, y);
        }
        self.nav = NavGrid::from_types(self.grid.width, self.grid.height, &self.cell_types);
        let x0 = changed.iter().map(|c| c.1).min().unwrap();
        let x1 = changed.iter().map(|c| c.1).max().unwrap();
        let y0 = changed.iter().map(|c| c.2).min().unwrap();
        let y1 = changed.iter().map(|c| c.2).max().unwrap();
        let _ = self
            .terrain
            .repaint(data, &self.grid, &mut caches.tiles, (x0, y0, x1, y1));
        let indices: [u32; 256] = std::array::from_fn(|i| {
            if data.animated[i] {
                crate::assets::ANIMATED | i as u32
            } else {
                crate::assets::rgb(data.palette.0[i])
            }
        });
        if let Ok((_, _, small)) = Terrain::small(data, &self.grid, &mut caches.tiles, &indices) {
            self.minimap = small;
        }
    }
}
