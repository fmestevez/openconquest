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

/// A ground tile's shape by its variant (0x5be0d6, tile sets of kind 4).
const GROUND_SHAPE: [u8; 44] = [
    0xff, 0, 1, 1, 1, 1, 1, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 9, 9, 12, 12, 10, 10, 11,
    11, 5, 5, 7, 7, 8, 8, 8, 6, 6, 6, 6, 6, 13, 14,
];
/// A shape's first ground variant (0x5be150).
const GROUND_VARIANT: [u8; 15] = [1, 2, 12, 17, 7, 30, 37, 32, 34, 22, 26, 28, 24, 42, 43];
/// A shelf tile's shape by its variant (0x5be35e, kind 7).
const SHELF_SHAPE: [u8; 22] = [
    0, 1, 4, 8, 8, 2, 3, 10, 11, 1, 4, 9, 12, 2, 3, 6, 6, 0, 5, 7, 13, 14,
];
/// A shape's first shelf variant (0x5be3ac).
const SHELF_VARIANT: [u8; 15] = [17, 1, 5, 6, 2, 18, 15, 19, 3, 11, 7, 8, 12, 20, 21];

const SEA: u8 = 0;
const SHELF: u8 = 1;
/// The ground's cell type, shut to ships.
const GROUND_TYPE: u32 = 8;

/// The shape of variant `v` of a tile set of kind `kind`.
fn shape_of(v: u8, kind: u8) -> Option<usize> {
    if v == 0 {
        return None;
    }
    let s = match kind {
        0 => 0,
        4 => *GROUND_SHAPE.get(v as usize)?,
        7 => *SHELF_SHAPE.get(v as usize)?,
        _ => return None,
    };
    (s < 15).then_some(s as usize)
}

fn variant_of(shape: usize, kind: u8) -> Option<u8> {
    match kind {
        4 => GROUND_VARIANT.get(shape).copied(),
        7 => SHELF_VARIANT.get(shape).copied(),
        _ => None,
    }
}

/// A dig on the scene's grid.
struct Dig<'a> {
    grid: &'a mut MapGrid,
    lib: &'a openconquest_formats::TileSetLibrary,
    changed: Vec<(u32, u32, u32, TileRef)>,
}

impl Dig<'_> {
    /// A tile set's kind (record byte 1) and cell type (byte 4).
    fn rec(&self, id: u8) -> (u8, u32) {
        self.lib
            .by_id(id)
            .map_or((0, 0), |s| (s.raw[1], s.raw[4] as u32 & 0xf))
    }

    fn inside(&self, (x, y): (i32, i32)) -> bool {
        x >= 0 && y >= 0 && x < self.grid.width as i32 && y < self.grid.height as i32
    }

    fn at(&self, (x, y): (i32, i32)) -> TileRef {
        self.grid.get(0, x as u32, y as u32)
    }

    fn put(&mut self, layer: u32, (x, y): (i32, i32), tile: TileRef) {
        self.grid.set(layer, x as u32, y as u32, tile);
        self.changed.push((layer, x as u32, y as u32, tile));
    }

    /// Open sea (0x502350).
    fn erase(&mut self, c: (i32, i32)) {
        self.put(
            0,
            c,
            TileRef {
                variant: 1,
                tileset: SEA,
            },
        );
    }

    /// The edges cell `c`'s neighbours ask of it as a tile of set `id`
    /// (0x501e70); None off the map's side.
    fn wants(&self, c: (i32, i32), id: u8) -> Option<u16> {
        let (kind, ty) = self.rec(id);
        let mut m = 0;
        for (k, (dx, dy)) in DIRS.iter().enumerate() {
            let n = (c.0 + dx, c.1 + dy);
            if !self.inside(n) {
                if k & 1 == 1 {
                    return None;
                }
                continue;
            }
            let t = self.at(n);
            if t.variant == 0 {
                continue;
            }
            if t.tileset == id {
                m |= EDGE[shape_of(t.variant, kind)?][k];
            }
            let (nkind, nty) = self.rec(t.tileset);
            if ty == 1 && nty == GROUND_TYPE {
                m |= EDGE2[shape_of(t.variant, nkind)?][k];
            }
        }
        Some(m)
    }

    /// Whether cell `c`'s tile already fits (0x502270).
    fn fits(&self, c: (i32, i32), id: u8, m: u16) -> bool {
        let t = self.at(c);
        let v = if t.tileset == 0 { 0 } else { t.variant };
        if v == 0 {
            return m == 0;
        }
        let shape = SHAPE.iter().position(|&s| s == m);
        shape.is_some() && shape == shape_of(v, self.rec(id).0)
    }

    /// Re-tiles cell `c` as set `id` to fit about it (0x5027e0); false to
    /// stop.
    fn reshape(&mut self, c: (i32, i32), id: u8) -> bool {
        let Some(m) = self.wants(c, id) else {
            return false;
        };
        if m == 0 || self.fits(c, id, m) {
            return true;
        }
        let shape = SHAPE
            .iter()
            .position(|&s| s == m)
            .or_else(|| SHAPE.iter().position(|&s| (s ^ m) & 0xf00 == 0));
        match shape.and_then(|s| variant_of(s, self.rec(id).0)) {
            Some(v) if v != 0 => {
                self.put(
                    0,
                    c,
                    TileRef {
                        variant: v,
                        tileset: id,
                    },
                );
                true
            }
            _ => false,
        }
    }

    /// Re-tiles the cells about `c` as set `id` (0x5028b0), shelf leaving
    /// the ground alone.
    fn around(&mut self, c: (i32, i32), id: u8) -> bool {
        for (dx, dy) in DIRS {
            let n = (c.0 + dx, c.1 + dy);
            if !self.inside(n) {
                continue;
            }
            if self.rec(id).1 == 1 && self.rec(self.at(n).tileset).1 == GROUND_TYPE {
                continue;
            }
            if !self.reshape(n, id) {
                return false;
            }
        }
        true
    }

    /// Layer 0 of 0x5030c0 on cell `c`: false if it was water.
    fn strike(&mut self, c: (i32, i32)) -> bool {
        let t = self.at(c);
        if t.tileset == SEA || t.tileset == SHELF || t.variant == 0 {
            return false;
        }
        let id = t.tileset;
        let (kind, ty) = self.rec(id);
        if kind == 0 || kind > 0x7f {
            self.erase(c);
            return true;
        }
        // The map's edge has bookkeeping of its own (0x502da0), left out.
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        if c.0 <= 1 || c.1 <= 1 || c.0 >= w - 2 || c.1 >= h - 2 {
            return false;
        }
        let Some(shape) = shape_of(t.variant, kind) else {
            return false;
        };
        self.erase(c);
        for (k, (dx, dy)) in DIRS.iter().enumerate() {
            let n = (c.0 + dx, c.1 + dy);
            if !self.inside(n) {
                continue;
            }
            let nt = self.at(n);
            if nt.variant == 0 || nt.tileset != id {
                continue;
            }
            let Some(ns) = shape_of(nt.variant, self.rec(nt.tileset).0) else {
                continue;
            };
            if EDGE[ns][k] & SHAPE[shape] & 0xf00 != 0 {
                self.erase(n);
            }
        }
        if !self.around(c, id) {
            return true;
        }
        if ty == GROUND_TYPE {
            // The shelf within two goes deep (0x502a80), then is laid
            // again about the coast (0x5029d0).
            for (dx, dy) in DIRS {
                let n = (c.0 + dx, c.1 + dy);
                if !self.inside(n) {
                    continue;
                }
                for (ex, ey) in DIRS {
                    let q = (n.0 + ex, n.1 + ey);
                    if self.inside(q) && self.at(q).tileset == SHELF {
                        self.erase(q);
                    }
                }
            }
            for (dx, dy) in DIRS {
                let n = (c.0 + dx, c.1 + dy);
                if self.inside(n) && !self.around(n, SHELF) {
                    return true;
                }
            }
        }
        true
    }
}

impl Scene {
    /// Lightning on cell `(cx, cy)` digs its ground into sea (0x5030c0
    /// with layer 0): the ground and coast of layer 0, what grows or stands
    /// on the cell (layers 2-7), and the loose things of layers 5-7 within
    /// two of it left in the sea (0x502fb0). The ships' ways, the cells'
    /// types and the pictures follow.
    pub(super) fn dig(&mut self, cx: i32, cy: i32, data: &GameData, caches: &mut Caches) {
        let mut dig = Dig {
            grid: &mut self.grid,
            lib: &data.tilesets,
            changed: Vec::new(),
        };
        if !dig.inside((cx, cy)) || !dig.strike((cx, cy)) {
            return;
        }
        let layers = dig.grid.layers.min(8);
        for l in 2..layers {
            let t = dig.grid.get(l, cx as u32, cy as u32);
            if !t.is_empty() {
                dig.put(
                    l,
                    (cx, cy),
                    TileRef {
                        variant: 0,
                        tileset: 0,
                    },
                );
            }
        }
        for y in cy - 2..=cy + 2 {
            for x in cx - 2..=cx + 2 {
                if !dig.inside((x, y)) || dig.at((x, y)).tileset != SEA {
                    continue;
                }
                for l in 5..layers {
                    let t = dig.grid.get(l, x as u32, y as u32);
                    if !t.is_empty() && dig.rec(t.tileset).0 == 0 {
                        dig.put(
                            l,
                            (x, y),
                            TileRef {
                                variant: 0,
                                tileset: 0,
                            },
                        );
                    }
                }
            }
        }
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
        self.nav = NavGrid::from_map(&self.grid);
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
