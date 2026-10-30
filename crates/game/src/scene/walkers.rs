//! Cyclops, minotaurs and amazons (CyclopClassTy) come out of their lairs
//! (CreaturesFlatClassTy, 0x414f00): a lair keeps up to record field 8 of
//! them about, sending another onto free land beside it now and then (the
//! quicker the higher field 9). They walk their island from half cell to
//! half cell (0x412740, 0x413240, 0x411520) and throw stones, axes or
//! arrows at ships in reach (`combat::creature_weapons`).

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::{ObjectClass, ObjectRecord};

use super::walk_path::{half_path, near_last, wave};
use super::{heading_towards, Scene};
use crate::assets::GameData;
use crate::combat::Rng;
use crate::objects::{self, Caches};

/// Where creatures choose to walk to, and the cells their walk grid is
/// refreshed on, about the cell they stand on (13 x 13 dwords: 0x594ba0
/// the cyclopes', 0x594e44 and 0x5950e8, alike, the minotaurs' and
/// amazons'; 2 the centre).
const CYCLOPS_MASK: [[u8; 13]; 13] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0],
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    [0, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 0],
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];
const OTHER_MASK: [[u8; 13]; 13] = [
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0],
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
    [0, 0, 1, 1, 1, 1, 2, 1, 1, 1, 1, 0, 0],
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0],
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];
/// Ticks a frame of the picture, by state (0x59538c).
const FRAME_TICKS: [u32; 7] = [1, 25, 1, 1, 1, 2, 2];
/// The signed turn from one heading to another (0x5c4f74, 8 x 8 dwords).
const TURN: [[i32; 8]; 8] = [
    [0, 1, 2, 3, 4, -3, -2, -1],
    [-1, 0, 1, 2, 3, 4, -3, -2],
    [-2, -1, 0, 1, 2, 3, 4, -3],
    [-3, -2, -1, 0, 1, 2, 3, 4],
    [4, -3, -2, -1, 0, 1, 2, 3],
    [3, 4, -3, -2, -1, 0, 1, 2],
    [2, 3, 4, -3, -2, -1, 0, 1],
    [1, 2, 3, 4, -3, -2, -1, 0],
];
/// Ticks a standing creature waits before choosing a route again (0x412740
/// state 1, `+0xf0 = now + 0x28`).
const WAKE_TICKS: u32 = 0x28;
/// Ticks between the creature's looks about (0x413d70, `+0xf4 = now + 5`).
const SCAN_TICKS: u32 = 5;

/// What a creature is doing (`+0x154`, which is also its picture's state
/// digit): 0 appearing, 1 standing, 2 walking, 3 throwing, 4 hit, 5 dying,
/// 6 turning.
pub const APPEAR: u8 = 0;
pub const STAND: u8 = 1;
pub const WALK: u8 = 2;
pub const THROW: u8 = 3;
pub const HIT: u8 = 4;
pub const DYING: u8 = 5;
pub const TURN_ROUND: u8 = 6;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Walker {
    pub id: u32,
    /// The lair it came out of (`NO_LAIR` for one a brain put down).
    pub lair: u32,
    /// Its walk grid's box in cells (`+0xac` [0], [1], and [2], [3] over
    /// 2): origin and size, made once where it appeared (0x411ce0).
    #[serde(default)]
    pub grid: Option<(i32, i32, i32, i32)>,
    /// The walk grid's shorts, a half cell each (`+0xac` [4]): 0 open.
    #[serde(default)]
    pub halves: Vec<i16>,
    /// State `+0x154`.
    #[serde(default)]
    pub state: u8,
    /// The state to go back to after a hit (`+0x148`).
    #[serde(default)]
    pub back: u8,
    /// Its half cell, from the grid's origin (`+0x90`, `+0x94`).
    #[serde(default)]
    pub half: (i32, i32),
    /// Heading 0-7 (`+0xa0`) and the one it turns to (`+0xa4`).
    #[serde(default)]
    pub heading: u8,
    #[serde(default)]
    pub turn_to: u8,
    /// Its own generator (`+0x18`).
    #[serde(default)]
    pub rng: u32,
    /// The picture: frame `+0x45`, first `+0x4d`, end `+0x51`, count
    /// `+0x49`, ticks a frame `+0x7d`, tick of the last step `+0x81`,
    /// drawn once `+0xa8`.
    #[serde(default)]
    pub frame: i32,
    #[serde(default)]
    pub first: i32,
    #[serde(default)]
    pub end: i32,
    #[serde(default)]
    pub count: i32,
    #[serde(default)]
    pub delay: u32,
    #[serde(default)]
    pub last: u32,
    #[serde(default)]
    pub drawn: bool,
    /// Where it is drawn, world pixels (`+0xb4`, `+0xb8`), and the half
    /// step it is on (`+0x10c..+0x118`).
    #[serde(default)]
    pub pos: (i32, i32),
    #[serde(default)]
    pub from: (i32, i32),
    #[serde(default)]
    pub to: (i32, i32),
    /// The route (`+0x108`, `+0xfc` entries: half x, y, direction), the
    /// entry it is at (`+0xf8`), moving between two halves (`+0x104`).
    #[serde(default)]
    pub path: Vec<(i32, i32, u8)>,
    #[serde(default)]
    pub step: usize,
    #[serde(default)]
    pub moving: bool,
    /// The route shut ahead (`+0x11c`).
    #[serde(default)]
    pub blocked: bool,
    /// Something to throw at (`+0x120`); crowded, no looking about
    /// (`+0x124`).
    #[serde(default)]
    pub target: bool,
    #[serde(default)]
    pub crowded: bool,
    /// Pursuing (`+0xd0`) towards `pursue_to` (`+0xd4`, `+0xd8`).
    #[serde(default)]
    pub pursue: bool,
    #[serde(default)]
    pub pursue_to: (i32, i32),
    /// The route's destination (`+0xdc`, `+0xe0`).
    #[serde(default)]
    pub dest: (i32, i32),
    /// Ticks: the next route choice (`+0xf0`), the next look about
    /// (`+0xf4`), the next walking sound (`+0x14c`).
    #[serde(default)]
    pub wake: u32,
    #[serde(default)]
    pub scan: u32,
    #[serde(default)]
    pub sound_at: u32,
    /// The ship it last saw in reach (the port's throw).
    #[serde(default)]
    pub prey: Option<(f32, f32)>,
}

/// The `lair` of a creature no lair sent out.
pub const NO_LAIR: u32 = u32::MAX;

/// The creature class of a lair class.
fn creature_of(lair: u32) -> u32 {
    match lair {
        0x46 => 0x1f4,
        0x47 => 0x1f5,
        _ => 0x1f6,
    }
}

/// Milliseconds a frame of a land creature's picture in `state` (0x59538c:
/// 1, 25, 1, 1, 1, 2, 2 ticks).
pub fn walker_frame_ms(state: u8) -> u64 {
    FRAME_TICKS[(state as usize).min(6)] as u64 * crate::clock::TICK_MS
}

/// A land creature's sprite sequence (0x4111b0: `<CLP|MNT|AMZ>SPR_` and
/// state and heading digits; state 6 has no heading). The amazons' throw
/// (`AMZSPR_3<h><b>`) takes its variant `b` from a draw.
pub fn walker_sequence(class: u32, state: u8, heading: usize) -> String {
    let prefix = match class {
        0x1f4 => "CLPSPR",
        0x1f5 => "MNTSPR",
        _ => "AMZSPR",
    };
    if state == TURN_ROUND {
        return format!("{prefix}_6");
    }
    format!("{prefix}_{state}{}", heading.min(7))
}

impl Walker {
    /// A draw of its generator (`+0x18`, the usual LCG).
    fn draw(&mut self) -> u32 {
        let mut r = Rng(self.rng);
        let v = r.next();
        self.rng = r.0;
        v
    }

    /// The walk grid's width and height in halves.
    fn size(&self) -> (i32, i32) {
        self.grid.map_or((0, 0), |(_, _, w, h)| (w * 2, h * 2))
    }

    /// The pixel of half `(x, y)` (`origin x 64 + half x 32 + 16`).
    fn pixel(&self, (x, y): (i32, i32)) -> (i32, i32) {
        let (ox, oy, _, _) = self.grid.unwrap_or_default();
        ((x * 64) / 2 + 16 + ox * 64, (y * 64) / 2 + 16 + oy * 64)
    }

    /// The grid's short at half `(x, y)`.
    fn at(&self, (x, y): (i32, i32)) -> i16 {
        let (w, _) = self.size();
        self.halves.get((y * w + x) as usize).copied().unwrap_or(-1)
    }

    /// The walking frames that hold the position, and the first moving one
    /// of the first steps (0x411520: cyclops 7 / 6, minotaur 9 / 8, amazon
    /// 10 / 9).
    fn holds(class: u32) -> (i32, i32) {
        match class {
            0x1f4 => (7, 6),
            0x1f5 => (9, 8),
            _ => (10, 9),
        }
    }
}

impl Scene {
    /// Lairs send out their creatures.
    pub(super) fn run_lairs(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let lairs: Vec<(u32, u32, f32, f32, u32, u32)> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::CreatureSpawn)
            .filter(|o| o.combat.as_ref().is_some_and(|c| c.hull > 0.0))
            .map(|o| {
                let f = |i| o.record.field(i).unwrap_or(0);
                (o.id, o.record.class_id, o.x, o.y, f(8), f(9))
            })
            .collect();
        for (id, class, x, y, max, speed) in lairs {
            let out = self.walkers.iter().filter(|w| w.lair == id).count() as u32;
            // Another comes out once (10 - field 9) x 20-25 ticks have
            // passed since the last, drawn afresh at each look (0x414f00).
            let ticks = (10 - speed.min(9)) * ((self.rng.next() % 21 + 80) * 25 / 100);
            let clock = self.lair_clocks.entry(id).or_insert(0.0);
            *clock += dt;
            if out >= max || *clock <= crate::clock::ticks(ticks) {
                continue;
            }
            *clock = 0.0;
            self.spawn_walker(id, class, x, y, data, caches);
        }
    }

    pub(super) fn spawn_walker(
        &mut self,
        lair: u32,
        lair_class: u32,
        x: f32,
        y: f32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let t = TILE_SIZE as f32;
        let (lx, ly) = ((x / t) as i32, (y / t) as i32);
        // A free land cell beside the lair (0x414f00 looks at the eight).
        let taken = |s: &Scene, cx: i32, cy: i32| {
            s.walkers.iter().any(|w| {
                s.object_index(w.id).is_some_and(|i| {
                    let o = &s.objects[i];
                    (o.x / t) as i32 == cx && (o.y / t) as i32 == cy
                })
            })
        };
        let cells: Vec<(i32, i32)> = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (lx + dx, ly + dy)))
            .filter(|&(cx, cy)| {
                (cx, cy) != (lx, ly) && self.is_land(cx, cy) && !taken(self, cx, cy)
            })
            .collect();
        if cells.is_empty() {
            return;
        }
        let (cx, cy) = cells[self.rng.next() as usize % cells.len()];
        self.put_walker(lair, creature_of(lair_class), (cx, cy), data, caches);
    }

    /// The game's seed of the objects' own generators (`[game +0x2ee5]`,
    /// the original's time stamp `RND_INIT`; drawn once from the scene's
    /// generator, as for the ships, `ship_draw`).
    fn objects_seed(&mut self) -> u32 {
        match self.rnd_init {
            Some(v) => v,
            None => {
                let v = self.rng.next() << 16 | self.rng.next();
                self.rnd_init = Some(v);
                v
            }
        }
    }

    /// A land creature of `class` (CyclopClassTy, 0x1497) appears on cell
    /// `(cx, cy)`, sent out by `lair`. In the original each is an object
    /// of its own; those on one cell are held by its jam (0x1f7,
    /// CyclopsJamClassTy 0x415ee0: up to 100, joined by message 0x1879f,
    /// left by 0x187a0, gone with the last, and passing lightning and
    /// tornadoes on to each), which commands nothing. Message 2 (0x413d70):
    /// its generator seeded `[game +0x2ee5] x id`, state 0, heading 0, one
    /// draw (`+0x150`), on its cell's north-west half (`+0x90 = 2 (x -
    /// origin)`, 0x411e7f), its walk grid made (0x411ce0, 0x42e720), its
    /// picture set and drawn (0x4111b0, 0x411520).
    pub(super) fn put_walker(
        &mut self,
        lair: u32,
        class: u32,
        (cx, cy): (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 8];
        fields[0] = class;
        fields[4] = cx as u32;
        fields[5] = cy as u32;
        let rec = ObjectRecord {
            id,
            class_id: class,
            fields,
        };
        if let Ok(Some(mut obj)) = objects::resolve(data, caches, &rec) {
            let grid = self.walk_grid_box((cx, cy));
            let (ox, oy, _, _) = grid;
            let mut w = Walker {
                id,
                lair,
                grid: Some(grid),
                rng: self.objects_seed().wrapping_mul(id),
                half: (2 * (cx - ox), 2 * (cy - oy)),
                ..Walker::default()
            };
            w.halves = self.terrain_halves(grid);
            w.draw();
            w.pos = w.pixel(w.half);
            (obj.x, obj.y) = (w.pos.0 as f32, w.pos.1 as f32);
            self.objects.push(obj);
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            self.walkers.push(w);
            let k = self.walkers.len() - 1;
            self.walker_picture_set(k, data, caches);
            self.walker_picture(k, data, caches);
        }
    }

    /// A land creature's walk grid's box (0x411ce0): the bounding box of
    /// the cells of the land region (0x42e590, `[0x5c74f4]`) of cell
    /// `(x, y)`, 6 more each way, kept on the map (origin at least 0,
    /// origin + size at most the map's size). Origin and size, in cells.
    pub(super) fn walk_grid_box(&self, (x, y): (i32, i32)) -> (i32, i32, i32, i32) {
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        let regions = self.land_regions();
        let (mut ox, mut oy, mut bw, mut bh) = (x, y, 1, 1);
        if (0..w).contains(&x) && (0..h).contains(&y) {
            let id = regions[(y * w + x) as usize];
            for row in 0..h {
                for col in 0..w {
                    if regions[(row * w + col) as usize] != id {
                        continue;
                    }
                    if col < ox {
                        bw += ox - col;
                        ox = col;
                    } else if bw + ox <= col {
                        bw = col - ox + 1;
                    }
                    if row < oy {
                        bh += oy - row;
                        oy = row;
                    } else if oy + bh <= row {
                        bh = row - oy + 1;
                    }
                }
            }
        }
        let (ox, oy) = ((ox - 6).max(0), (oy - 6).max(0));
        let (bw, bh) = ((bw + 13).min(w - ox), (bh + 13).min(h - oy));
        (ox, oy, bw, bh)
    }

    /// The walk grid as made at the creature's creation (0x42e720 with
    /// `param_4` 2): the ground only, each cell by `terrain_quarters`, -1
    /// shut.
    fn terrain_halves(&self, (ox, oy, gw, gh): (i32, i32, i32, i32)) -> Vec<i16> {
        let w2 = gw * 2;
        let mut m = vec![0i16; (w2 * gh * 2) as usize];
        for ry in 0..gh {
            for rx in 0..gw {
                let q = self.terrain_quarters((ox + rx, oy + ry));
                for (k, &shut) in q.iter().enumerate() {
                    let (hx, hy) = (2 * rx + k as i32 % 2, 2 * ry + k as i32 / 2);
                    if shut {
                        m[(hy * w2 + hx) as usize] = -1;
                    }
                }
            }
        }
        m
    }

    /// Refreshes walker `k`'s walk grid about it (0x411ef0, every 5
    /// ticks): over the 13 x 13 cells of its mask about its cell, on the
    /// grid, row by row, each cell's four halves by `walk_cell_quarters`;
    /// a lair (0x46-0x48) or castle (0x42, 0x43) in a cell's slot, unless
    /// the cell is on the grid's last row, also marks -2 the south-west
    /// half of the cell east of it (its column, from the grid's origin,
    /// below the origin's x plus the grid's width less 1), the south-east
    /// half of the cell west of it (its column above the origin's x) and
    /// the north halves of the cell south of it (its row below the
    /// origin's y plus the grid's height less 1; grid-relative against
    /// absolute, as written); a half written later over a -2 becomes -1.
    /// Then, walking, the first half of its route ahead that is -1 shuts
    /// the route (`+0x11c`).
    fn refresh_halves(&mut self, k: usize) {
        let w = &self.walkers[k];
        let Some((ox, oy, gw, gh)) = w.grid else {
            return;
        };
        let class = self
            .object_index(w.id)
            .map_or(500, |i| self.objects[i].record.class_id);
        let mask = if class == 0x1f4 {
            &CYCLOPS_MASK
        } else {
            &OTHER_MASK
        };
        let (hx, hy) = w.half;
        let w2 = gw * 2;
        let mut m = std::mem::take(&mut self.walkers[k].halves);
        let set = |m: &mut Vec<i16>, x: i32, y: i32, v: i16| {
            if let Some(c) = m.get_mut((y * w2 + x) as usize) {
                *c = v;
            }
        };
        for (j, row) in mask.iter().enumerate() {
            for (i, &on) in row.iter().enumerate() {
                if on == 0 {
                    continue;
                }
                let (rx, ry) = (hx / 2 - 6 + i as i32, hy / 2 - 6 + j as i32);
                if rx < 0 || rx >= gw || ry < 0 || ry >= gh {
                    continue;
                }
                let cell = (ox + rx, oy + ry);
                let q = self.walk_cell_quarters(cell);
                let fortress = self
                    .slot_class(cell)
                    .is_some_and(|c| matches!(c, 0x42 | 0x43 | 0x46..=0x48));
                if fortress && ry < gh - 1 {
                    if rx < ox - 1 + gw {
                        set(&mut m, 2 * rx + 2, 2 * ry + 1, -2);
                    }
                    if ox < rx {
                        set(&mut m, 2 * rx - 1, 2 * ry + 1, -2);
                    }
                    if ry < gh - 1 + oy {
                        set(&mut m, 2 * rx, 2 * ry + 2, -2);
                        set(&mut m, 2 * rx + 1, 2 * ry + 2, -2);
                    }
                }
                for (n, &shut) in q.iter().enumerate() {
                    let (x, y) = (2 * rx + n as i32 % 2, 2 * ry + n as i32 / 2);
                    let i = (y * w2 + x) as usize;
                    let v = if m.get(i) == Some(&-2) || shut { -1 } else { 0 };
                    set(&mut m, x, y, v);
                }
            }
        }
        let w = &mut self.walkers[k];
        w.halves = m;
        if w.state == WALK && !w.path.is_empty() {
            let ahead = w.path.iter().skip(w.step);
            if ahead.clone().any(|&(x, y, _)| w.at((x, y)) == -1) {
                w.blocked = true;
            }
        }
    }

    /// Whether the half cell land creature `id` stands on is shut in the
    /// walk grid 0x411ef0 rebuilds (lightning kills it then, 0x414b0f):
    /// its half (`+0x90`, `+0x94`); on its cell, an object other than a
    /// jam in the cell's slot shuts all four, as does ground that is not
    /// land (low nibble 8), nibble 5 at 1, or nibbles 6 or 7; else the
    /// halves along the tile's shores (0x5acf13: 1 the north two, 2 the
    /// east, 4 the south, 8 the west). A lair (0x46-0x48) or castle (0x42,
    /// 0x43) in a cell's slot also shuts, unless the cell is on the grid's
    /// last row, the south-west half of the cell east of it (when its
    /// column, from the grid's origin, is below the origin's x plus the
    /// grid's width less 1), the south-east half of the cell west of it
    /// (its column above the origin's x) and the north halves of the cell
    /// south of it (as written: grid-relative against absolute).
    pub(super) fn walker_half_shut(&self, id: u32) -> bool {
        let Some(w) = self.walkers.iter().find(|w| w.id == id) else {
            return false;
        };
        if self.object_index(id).is_none() {
            return false;
        }
        let (ox, oy, gw, gh) = w.grid.unwrap_or_else(|| {
            let t = TILE_SIZE as f32;
            let i = self.object_index(id).unwrap();
            let o = &self.objects[i];
            self.walk_grid_box(((o.x / t) as i32, (o.y / t) as i32))
        });
        let (hx, hy) = w.half;
        let (rx, ry) = (hx.div_euclid(2), hy.div_euclid(2));
        let (cx, cy) = (ox + rx, oy + ry);
        let inside = |x: i32, y: i32| (0..gw).contains(&x) && (0..gh).contains(&y);
        if !inside(rx, ry) {
            return false;
        }
        // Quarter: 0 north-west, 1 north-east, 2 south-west, 3 south-east.
        let q = hx.rem_euclid(2) + 2 * hy.rem_euclid(2);
        if self.walk_cell_quarters((cx, cy))[q as usize] {
            return true;
        }
        let fortress = |x: i32, y: i32| {
            self.slot_class((x, y))
                .is_some_and(|c| matches!(c, 0x42 | 0x43 | 0x46..=0x48))
        };
        match q {
            2 => inside(rx - 1, ry) && ry < gh - 1 && rx - 1 < ox + gw - 1 && fortress(cx - 1, cy),
            3 => inside(rx + 1, ry) && ry < gh - 1 && rx + 1 > ox && fortress(cx + 1, cy),
            _ => inside(rx, ry - 1) && fortress(cx, cy - 1),
        }
    }

    /// The four halves of cell `(x, y)` in a land creature's walk grid
    /// (0x411ef0), shut or not: north-west, north-east, south-west,
    /// south-east.
    fn walk_cell_quarters(&self, (x, y): (i32, i32)) -> [bool; 4] {
        if self.slot_class((x, y)).is_some() {
            return [true; 4];
        }
        self.terrain_quarters((x, y))
    }

    /// The same by the ground alone (0x42e720 with `param_4` 2): ground
    /// that is not land, nibble 5 at 1, or nibbles 6 or 7, shuts all four;
    /// else the halves along the tile's shores.
    fn terrain_quarters(&self, (x, y): (i32, i32)) -> [bool; 4] {
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return [true; 4];
        }
        let l = self.cell_types[(y * w + x) as usize];
        if l & 0xf != 8 || (l >> 20) & 0xf == 1 || l >> 24 != 0 {
            return [true; 4];
        }
        let e = self.tile_edges(x, y);
        [
            e & (1 | 8) != 0,
            e & (1 | 2) != 0,
            e & (4 | 8) != 0,
            e & (2 | 4) != 0,
        ]
    }

    /// The class of what holds cell `(x, y)`'s object slot (`grid + 0xc`),
    /// but for a land creature's jam (0x1f7), which the walk grid passes
    /// over: a building whose footprint covers the cell, or a ship (class
    /// 200 + its type).
    fn slot_class(&self, (x, y): (i32, i32)) -> Option<u32> {
        if let Some(k) = self.slot_ship((x, y)) {
            return self.objects[k].ship.as_ref().map(|s| 200 + s.kind as u32);
        }
        self.objects
            .iter()
            .filter(|o| {
                matches!(
                    o.class,
                    ObjectClass::Building(_)
                        | ObjectClass::Barbarian(_)
                        | ObjectClass::Castle(_)
                        | ObjectClass::HarpyNest
                        | ObjectClass::CreatureSpawn
                        | ObjectClass::HeroHouse
                )
            })
            .find(|o| {
                o.cell.is_some_and(|(bx, by)| {
                    let n = super::trade::building_cells(o);
                    (bx..bx + n).contains(&x) && (by..by + n).contains(&y)
                })
            })
            .map(|o| o.record.class_id)
    }

    /// Whether a land creature stands on cell `(cx, cy)` (its jam fills
    /// the cell's object slot, 0x499800).
    pub(super) fn walker_at(&self, (cx, cy): (i32, i32)) -> bool {
        let t = TILE_SIZE as f32;
        self.walkers.iter().any(|w| {
            self.object_index(w.id).is_some_and(|i| {
                let o = &self.objects[i];
                (o.x / t) as i32 == cx && (o.y / t) as i32 == cy
            })
        })
    }

    fn is_land(&self, cx: i32, cy: i32) -> bool {
        let (w, h) = self.map_size();
        cx >= 0 && cy >= 0 && (cx as u32) < w && (cy as u32) < h && !self.nav.is_water(cx, cy)
    }

    /// Lairs send out creatures; those killed are gone.
    pub(super) fn run_walkers(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.run_lairs(dt, data, caches);
        let alive = |s: &Scene, id: u32| {
            s.object_index(id)
                .is_some_and(|i| s.objects[i].combat.as_ref().is_some_and(|c| c.hull > 0.0))
        };
        let walkers = std::mem::take(&mut self.walkers);
        self.walkers = walkers.into_iter().filter(|w| alive(self, w.id)).collect();
    }

    /// Each land creature's tick (message 0, 0x413d70): every 5 ticks,
    /// unless hit or dying, its walk grid refreshed about it (0x411ef0)
    /// and a look about (the port's: a ship in its reach to throw at; the
    /// original's 0x412290 not ported), the throw given up when nothing is
    /// there; then its state's step (0x412740) and its picture's
    /// (0x411520).
    pub(super) fn tick_walkers(&mut self, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        for k in 0..self.walkers.len() {
            let Some(i) = self.object_index(self.walkers[k].id) else {
                continue;
            };
            if now >= self.walkers[k].scan {
                self.walkers[k].scan = now + SCAN_TICKS;
                if !matches!(self.walkers[k].state, DYING | HIT) {
                    self.refresh_halves(k);
                    let found = self.walker_look(k, i);
                    let w = &mut self.walkers[k];
                    if !found && w.state == THROW {
                        w.target = false;
                        w.state = STAND;
                        self.walker_picture_set(k, data, caches);
                    }
                }
            }
            self.walker_step(k, data, caches);
            self.walker_picture(k, data, caches);
            let (x, y) = self.walkers[k].pos;
            let o = &mut self.objects[i];
            (o.x, o.y) = (x as f32, y as f32);
        }
    }

    /// The port's look about (in place of 0x412290, not ported): the
    /// nearest ship of a player in its first weapon's reach becomes its
    /// target (`+0x120`), which it turns to when it throws. Not while
    /// appearing or crowded (`+0x124`), as 0x412290.
    fn walker_look(&mut self, k: usize, i: usize) -> bool {
        let w = &self.walkers[k];
        if w.state == APPEAR || w.crowded {
            return false;
        }
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        let reach = self.objects[i]
            .combat
            .as_ref()
            .and_then(|c| c.weapons.first())
            .map_or(0.0, |&w| crate::combat::reach(w, 0).1);
        let prey = self
            .objects
            .iter()
            .filter(|o| {
                o.ship.is_some() && o.owner.is_some() && o.owner != Some(crate::combat::MONSTERS)
            })
            .map(|o| (o.x - x, o.y - y))
            .filter(|d| d.0.hypot(d.1) <= reach)
            .min_by(|a, b| a.0.hypot(a.1).total_cmp(&b.0.hypot(b.1)));
        let w = &mut self.walkers[k];
        w.prey = prey;
        if prey.is_some() {
            w.target = true;
        }
        prey.is_some()
    }

    /// Walker `k`'s state step (0x412740).
    fn walker_step(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        match self.walkers[k].state {
            STAND => {
                let w = &mut self.walkers[k];
                if now < w.wake {
                    return;
                }
                if w.target {
                    self.walker_throw(k, data, caches);
                    return;
                }
                // It goes on from its standing loop's first frame.
                if w.frame != w.first {
                    return;
                }
                w.wake = now + WAKE_TICKS;
                self.choose_route(k, true, data, caches);
                if self.walkers[k].state == WALK {
                    self.walker_step(k, data, caches);
                }
            }
            WALK => self.walk_step(k, data, caches),
            // Its aim and throw are the port's (in `run_combat`); the
            // step's first draw a tick is kept. (Not decoded:
            // the rest of the throw's draws and timers in 0x412740 state 3.)
            THROW => {
                self.walkers[k].draw();
            }
            _ => {}
        }
    }

    /// Goes to throwing (0x412740: state 3), turned to its prey (the
    /// port's: the original's aim is not decoded).
    fn walker_throw(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let w = &mut self.walkers[k];
        w.state = THROW;
        if let Some((dx, dy)) = w.prey {
            w.heading = heading_towards(dx, dy) as u8;
        }
        self.walker_picture_set(k, data, caches);
    }

    /// Walking, each tick (0x412740 state 2): on the tick after a half
    /// step ends it takes the route's next half (if it is still open,
    /// else the route is shut), changing jams when the cell changes, and
    /// its picture again; then, at the route's end, shut, or with a target
    /// or a pursuit, it stops (`walk_end`); else it starts the half step
    /// towards the entry after, turning its picture to that entry's
    /// direction.
    fn walk_step(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let w = &mut self.walkers[k];
        // The walking sound (0x85 / 0x86 / 0x87 by 0x50e570 kind 5; not
        // played: the port has no list of their samples), its next time a
        // draw on.
        if now >= w.sound_at {
            let r = w.draw();
            w.sound_at = now + ((r % 41 + 60) * 375) / 100;
        }
        if w.moving {
            return;
        }
        if w.step < w.path.len() {
            let (x, y, _) = w.path[w.step];
            if w.at((x, y)) == 0 {
                w.half = (x, y);
                w.pos = w.pixel(w.half);
                // (A new cell: the jam left and joined, 0x412cf5; the
                // port keeps no jams.)
                w.step += 1;
                self.walker_picture_set(k, data, caches);
            } else {
                w.pos = w.pixel(w.half);
                w.blocked = true;
            }
        }
        let w = &mut self.walkers[k];
        if w.step >= w.path.len() || w.target || w.pursue || w.blocked {
            self.walk_end(k, data, caches);
            return;
        }
        let (x, y, dir) = w.path[w.step];
        if w.half == (x, y) {
            w.state = STAND;
            w.crowded = false;
            self.walker_picture_set(k, data, caches);
            return;
        }
        w.moving = true;
        w.frame = w.first;
        w.from = w.pos;
        w.to = w.pixel((x, y));
        if dir != w.heading {
            w.heading = dir;
            self.walker_picture_set(k, data, caches);
        }
    }

    /// The end of a walk (0x412740): another creature of its
    /// cell on its half (0x415e40), it chooses a route at once, without
    /// turning, and is crowded (`+0x124`); else it throws if it has a
    /// target, or stands, choosing again in 40 ticks.
    fn walk_end(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        self.walkers[k].blocked = false;
        let me = &self.walkers[k];
        let t = TILE_SIZE as f32;
        let cell = |s: &Scene, id: u32| {
            s.object_index(id).map(|i| {
                let o = &s.objects[i];
                ((o.x / t) as i32, (o.y / t) as i32)
            })
        };
        let mine = cell(self, me.id);
        let crowded = self
            .walkers
            .iter()
            .any(|o| o.id != me.id && o.half == me.half && cell(self, o.id) == mine);
        if crowded {
            let w = &mut self.walkers[k];
            w.pursue = false;
            w.target = false;
            w.crowded = true;
            self.choose_route(k, false, data, caches);
            let w = &mut self.walkers[k];
            w.frame = w.first;
            w.from = w.pos;
            w.to = w.pos;
            return;
        }
        let w = &mut self.walkers[k];
        w.crowded = false;
        if w.target {
            self.walker_throw(k, data, caches);
            return;
        }
        w.state = STAND;
        w.wake = now + WAKE_TICKS;
        self.walker_picture_set(k, data, caches);
    }

    /// Walker `k` chooses a route (0x413240 with `pursue_only` 0): a
    /// pursuit (`+0xd0`) goes to the last cell of 0x55ba50's route within
    /// 5 of its goal; else, or with no route there, a destination among
    /// the halves of its mask about it (26 x 26, the mask's column and row
    /// by the half over 2, value 1) that a flood of its walk grid from
    /// where it stands reaches in 14 (0x558fa0), each weighed 10 on land
    /// (0x42eb20: the cell's layer-0 tile of type 8) else 0, picked by a
    /// draw (`r % total`, the first whose running sum is at least `r`),
    /// and a route there (0x55b560); no route drops it, its weight off the
    /// total, and draws again. With `turn` and a route whose first step
    /// is not its heading, it turns first (state 6, a pursuit of the
    /// destination set); else it walks; with no route it stands.
    fn choose_route(&mut self, k: usize, turn: bool, data: &GameData, caches: &mut Caches) {
        let class = self
            .object_index(self.walkers[k].id)
            .map_or(500, |i| self.objects[i].record.class_id);
        let mask = if class == 0x1f4 {
            &CYCLOPS_MASK
        } else {
            &OTHER_MASK
        };
        let (ox, oy, _, _) = self.walkers[k].grid.unwrap_or_default();
        let w = &mut self.walkers[k];
        let (w2, h2) = w.size();
        let heading = w.heading as i32;
        w.path.clear();
        let mut path = None;
        if w.pursue {
            w.dest = w.pursue_to;
            let mut g = w.halves.clone();
            if let Some(c) = near_last(&mut g, w2, h2, w.half, w.dest, heading, 5) {
                w.dest = c;
                if w.dest != w.half {
                    let mut g = w.halves.clone();
                    path = half_path(&mut g, w2, h2, w.half, w.dest, heading);
                }
            }
            w.pursue = false;
        }
        if path.is_none() {
            let half = w.half;
            let mut g = w.halves.clone();
            wave(
                &mut g,
                w2,
                (0, 0, w2 - 1, h2 - 1),
                half,
                (-1, -1),
                14,
                -1,
                0,
                0,
            );
            let cw = self.grid.width as i32;
            let mut list: Vec<((i32, i32), u32)> = Vec::new();
            let mut total = 0u32;
            for c in 0..26 {
                for i in 0..26 {
                    if mask[c / 2][i / 2] != 1 {
                        continue;
                    }
                    let (hx, hy) = (half.0 - 13 + i as i32, half.1 - 13 + c as i32);
                    if !(0..w2).contains(&hx) || !(0..h2).contains(&hy) {
                        continue;
                    }
                    if g[(hy * w2 + hx) as usize] <= 0 {
                        continue;
                    }
                    let (cx, cy) = (hx / 2 + ox, oy + hy / 2);
                    let weight = if self.cell_types[(cy * cw + cx) as usize] & 0xf == 8 {
                        10
                    } else {
                        0
                    };
                    total += weight;
                    list.push(((hx, hy), weight));
                }
            }
            let w = &mut self.walkers[k];
            while !list.is_empty() {
                // (Not decoded: every weight 0, where 0x413622 divides by
                // zero; the port stands.)
                if total == 0 {
                    break;
                }
                let r = w.draw() % total;
                let mut sum = 0;
                for &(c, weight) in &list {
                    sum += weight;
                    if r <= sum {
                        w.dest = c;
                        break;
                    }
                }
                let mut g = w.halves.clone();
                path = half_path(&mut g, w2, h2, w.half, w.dest, heading);
                if path.is_none() {
                    if let Some(n) = list.iter().position(|e| e.0 == w.dest) {
                        total -= list[n].1;
                        list.remove(n);
                    }
                }
                if path.is_some() || list.is_empty() || total == 0 {
                    break;
                }
            }
        }
        let w = &mut self.walkers[k];
        match path {
            None => w.state = STAND,
            Some(p) => {
                let first = p.get(1).map_or(0, |e| e.2) as usize;
                let turned = TURN[w.heading as usize & 7][first & 7];
                if turn && turned != 0 {
                    // Clamped, not wrapped (0x413796).
                    let h = w.heading as i32 + turned;
                    w.turn_to = if h > 7 {
                        0
                    } else if h < 0 {
                        7
                    } else {
                        h as u8
                    };
                    w.state = TURN_ROUND;
                    w.pursue = true;
                    w.crowded = false;
                    w.pursue_to = w.dest;
                } else {
                    w.state = WALK;
                    w.path = p;
                }
            }
        }
        let w = &mut self.walkers[k];
        w.step = 0;
        w.moving = false;
        self.walker_picture_set(k, data, caches);
    }

    /// Walker `k`'s picture for its state and heading (0x4111b0): its
    /// frames, ticks a frame, the whole sequence, but walking (cyclops
    /// frames 0-0x26 on the first steps, 0x16-0x36 after; minotaur and
    /// amazon from 0, or 0x14 / 0x13 after, to the end) and turning (9 or,
    /// for the amazon, 12 frames a heading, from the heading's to the
    /// one turned to, the nearer end of the sequence for heading 0); the
    /// frame at the first.
    fn walker_picture_set(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let Some(i) = self.object_index(self.walkers[k].id) else {
            return;
        };
        let class = self.objects[i].record.class_id;
        let w = &mut self.walkers[k];
        let name = if class == 0x1f6 && w.state == THROW {
            let b = w.draw() & 1;
            format!("AMZSPR_3{}{b}", w.heading.min(7))
        } else {
            walker_sequence(class, w.state, w.heading as usize)
        };
        if let Ok(frames) = objects::sequence_frames(data, caches, &name) {
            if !frames.is_empty() {
                w.count = frames.len() as i32;
                self.objects[i].frames = frames;
            }
        }
        let w = &mut self.walkers[k];
        w.delay = FRAME_TICKS[(w.state as usize).min(6)];
        w.drawn = false;
        w.first = 0;
        w.end = w.count;
        let early = w.step <= 1;
        if w.state == WALK {
            match class {
                0x1f4 => (w.first, w.end) = if early { (0, 0x26) } else { (0x16, 0x36) },
                0x1f5 => w.first = if early { 0 } else { 0x14 },
                _ => w.first = if early { 0 } else { 0x13 },
            }
        }
        if w.state == TURN_ROUND {
            let n = if class == 0x1f6 { 9 } else { 12 };
            w.first = n * w.heading as i32;
            w.end = n * w.turn_to as i32;
            if w.first == 0 && w.count - w.end < w.end {
                w.first = w.count - 1;
            }
            if w.end == 0 && w.count - w.first < w.first {
                w.end = w.count - 1;
            }
        }
        w.frame = w.first;
    }

    /// Walker `k`'s picture's tick (0x411520): once its frame's ticks have
    /// passed it steps a frame towards the end, and at the end: appearing,
    /// it stands; walking, the half step is over (the frame back to the
    /// first, or to 0x16 / 0x14 / 0x13 after the first steps); turning,
    /// it stands with the new heading; else it loops. Standing, after its
    /// first frame each frame takes 5 ticks, not 25. Walking, its place
    /// goes from the half step's start to its end with the frame (`(to -
    /// from) x (frame - f0) / (end - f0)`, the first frames holding it:
    /// cyclops below 7, f0 6 on the first steps; minotaur 9 / 8; amazon 10
    /// / 9). (Not ported: the hit reaction, state 4, and the dying
    /// picture: the port's creatures are gone when killed.)
    fn walker_picture(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let Some(i) = self.object_index(self.walkers[k].id) else {
            return;
        };
        let class = self.objects[i].record.class_id;
        let (mut stand, mut appeared) = (false, false);
        let w = &mut self.walkers[k];
        if (class == 0x1f5 || class == 0x1f6)
            && w.state == DYING
            && w.frame > 0xb
            && w.delay == FRAME_TICKS[DYING as usize]
        {
            w.delay *= 2;
        }
        if w.drawn && now < w.last + w.delay {
            return;
        }
        if w.count < 2 && w.drawn {
            return;
        }
        if w.drawn {
            w.frame += if w.first < w.end { 1 } else { -1 };
            if (w.first < w.end && w.frame >= w.end) || (w.end < w.first && w.frame <= w.end) {
                match w.state {
                    APPEAR => {
                        w.frame = w.first;
                        w.state = STAND;
                        stand = true;
                        appeared = true;
                    }
                    WALK => {
                        w.moving = false;
                        w.frame = if w.step <= 1 {
                            match class {
                                0x1f4 => 0x16,
                                0x1f5 => 0x14,
                                _ => 0x13,
                            }
                        } else {
                            w.first
                        };
                    }
                    TURN_ROUND => {
                        w.state = STAND;
                        w.heading = w.turn_to;
                        stand = true;
                    }
                    _ => w.frame = w.first,
                }
            }
        }
        if stand {
            // Standing now: its picture (0x4111b0); having appeared, the
            // walking sound's time a draw on (0x411520).
            self.walker_picture_set(k, data, caches);
            if appeared {
                let w = &mut self.walkers[k];
                let r = w.draw();
                w.sound_at = ((r % 41 + 60) * 375) / 100 + now;
            }
        }
        let w = &mut self.walkers[k];
        match w.state {
            STAND if w.frame > 0 && w.delay == FRAME_TICKS[STAND as usize] => w.delay = 5,
            WALK => {
                let (hold, early) = Walker::holds(class);
                if !(0..hold).contains(&w.frame) {
                    let f0 = if w.step <= 1 { early } else { w.first };
                    if w.moving && w.end != f0 {
                        let along = |a: i32, b: i32| (b - a) * (w.frame - f0) / (w.end - f0) + a;
                        w.pos = (along(w.from.0, w.to.0), along(w.from.1, w.to.1));
                    } else {
                        w.pos = w.to;
                    }
                }
            }
            _ => {}
        }
        w.last = now;
        w.drawn = true;
        let (frame, count) = (w.frame, w.count.max(1));
        self.objects[i].progress = Some((frame.clamp(0, count - 1) as f32 + 0.5) / count as f32);
    }
}
