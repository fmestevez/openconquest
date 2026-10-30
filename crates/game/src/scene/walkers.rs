//! Cyclops, minotaurs and amazons (CyclopClassTy) come out of their lairs
//! (CreaturesFlatClassTy, 0x414f00): a lair keeps up to record field 8 of
//! them about, sending another onto free land beside it now and then (the
//! quicker the higher field 9). They roam the land near the lair and throw
//! stones, axes or arrows at ships in reach (`combat::creature_weapons`).

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::{ObjectClass, ObjectRecord};

use super::{heading_towards, Scene};
use crate::assets::GameData;
use crate::objects::{self, Caches};

/// Where creatures may wander, around where they stand (0x594ba0: the
/// cyclopes'; 0x594e44 and 0x5950e8, alike: the minotaurs' and amazons').
const CYCLOPS_REACH: [&str; 11] = [
    "..#######..",
    ".#########.",
    "###########",
    "###########",
    "###########",
    "#####.#####",
    "###########",
    "###########",
    "###########",
    ".#########.",
    "..#######..",
];
const OTHER_REACH: [&str; 9] = [
    "..#####..",
    ".#######.",
    "#########",
    "#########",
    "####.####",
    "#########",
    "#########",
    ".#######.",
    "..#####..",
];
/// Walking speed, world pixels a second: a cell (64 pixels) a pass of the
/// 16-frame walking picture at 5 ticks a frame (0x411520, 0x59538c).
const SPEED: f32 = 64.0 / (16.0 * 5.0 * crate::clock::TICK);

/// What a creature is doing.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Walk {
    Appear(f32),
    Stand(f32),
    To(i32, i32),
    Throw,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Walker {
    pub id: u32,
    /// The lair it came out of (`NO_LAIR` for one a brain put down).
    pub lair: u32,
    pub walk: Walk,
    pub shown: (u8, usize),
    /// Its walk grid's box in cells (`+0xac` [0], [1], and [2], [3] over
    /// 2): origin and size, made once where it appeared (0x411ce0).
    #[serde(default)]
    pub grid: Option<(i32, i32, i32, i32)>,
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

/// A land creature's sprite sequence: state 0 appearing, 1 walking, 2
/// standing, 3 throwing, 4 turning, 5 vanishing; then the heading.
/// Milliseconds a frame of a land creature's picture in `state`: ticks by
/// state (0x59538c: 1, 25, 1, 1, 1, 2, 2; the 25 drops to 5 after the
/// first frame, 0x411520).
pub fn walker_frame_ms(state: u8) -> u64 {
    const TICKS: [u64; 7] = [1, 5, 1, 1, 1, 2, 2];
    TICKS[(state as usize).min(6)] * crate::clock::TICK_MS
}

pub fn walker_sequence(class: u32, state: u8, heading: usize) -> String {
    let prefix = match class {
        0x1f4 => "CLPSPR",
        0x1f5 => "MNTSPR",
        _ => "AMZSPR",
    };
    // The amazons' throwing (drawing the bow) comes in two parts.
    if class == 0x1f6 && state == 3 {
        return format!("{prefix}_3{}0", heading.min(7));
    }
    format!("{prefix}_{state}{}", heading.min(7))
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

    /// A land creature of `class` (CyclopClassTy, 0x1497) appears on cell
    /// `(cx, cy)`, sent out by `lair`. In the original each is an object
    /// of its own; those on one cell are held by its jam (0x1f7,
    /// CyclopsJamClassTy 0x415ee0: up to 100, joined by message 0x1879f,
    /// left by 0x187a0, gone with the last, and passing lightning and
    /// tornadoes on to each), which commands nothing.
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
            // It comes out over its appearing picture, a frame a tick.
            let appear = obj.frames.len() as f32 * walker_frame_ms(0) as f32 / 1000.0;
            // On its cell's north-west half: `+0x90 = 2 (x - origin)`
            // (0x411e7f), drawn at `origin x 64 + half x 32 + 16` (0x414359).
            let t = TILE_SIZE as f32;
            (obj.x, obj.y) = (cx as f32 * t + t / 4.0, cy as f32 * t + t / 4.0);
            let grid = Some(self.walk_grid_box((cx, cy)));
            self.objects.push(obj);
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            self.walkers.push(Walker {
                id,
                lair,
                walk: Walk::Appear(appear),
                shown: (0, 0),
                grid,
            });
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

    /// Whether the half cell land creature `id` stands on is shut in the
    /// walk grid 0x411ef0 rebuilds (lightning kills it then, 0x414b0f):
    /// its half by its position (`+0xb4 = half x 32 + 16`); on its cell, an
    /// object other than a jam in the cell's slot shuts all four, as does
    /// ground that is not land (low nibble 8), nibble 5 at 1, or nibbles 6
    /// or 7; else the halves along the tile's shores (0x5acf13: 1 the
    /// north two, 2 the east, 4 the south, 8 the west). A lair (0x46-0x48)
    /// or castle (0x42, 0x43) in a cell's slot also shuts, unless the cell
    /// is on the grid's last row, the south-west half of the cell east of
    /// it (when its column, from the grid's origin, is below the origin's
    /// x plus the grid's width less 1), the south-east half of the cell
    /// west of it (its column above the origin's x) and the north halves of
    /// the cell south of it (as written: grid-relative against absolute).
    /// (The port's creatures walk from cell middle to cell middle, the
    /// original's from half to half (0x412740, 0x413240): one that has
    /// walked stands on the south-east half here.)
    pub(super) fn walker_half_shut(&self, id: u32) -> bool {
        let Some(w) = self.walkers.iter().find(|w| w.id == id) else {
            return false;
        };
        let Some(i) = self.object_index(id) else {
            return false;
        };
        let half = TILE_SIZE as f32 / 2.0;
        let hx = (self.objects[i].x / half).floor() as i32;
        let hy = (self.objects[i].y / half).floor() as i32;
        let (cx, cy) = (hx.div_euclid(2), hy.div_euclid(2));
        let (ox, oy, gw, gh) = w.grid.unwrap_or_else(|| self.walk_grid_box((cx, cy)));
        let (rx, ry) = (cx - ox, cy - oy);
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
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return [true; 4];
        }
        if self.slot_class((x, y)).is_some() {
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

    /// Moves the land creatures and shows what they do.
    pub(super) fn run_walkers(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.run_lairs(dt, data, caches);
        let alive = |s: &Scene, id: u32| {
            s.object_index(id)
                .is_some_and(|i| s.objects[i].combat.as_ref().is_some_and(|c| c.hull > 0.0))
        };
        let walkers = std::mem::take(&mut self.walkers);
        self.walkers = walkers.into_iter().filter(|w| alive(self, w.id)).collect();
        let t = TILE_SIZE as f32;
        for k in 0..self.walkers.len() {
            let w = self.walkers[k].clone();
            let i = self.object_index(w.id).unwrap();
            let class = self.objects[i].record.class_id;
            let (x, y) = (self.objects[i].x, self.objects[i].y);
            let mut heading = w.shown.1;
            // A ship in reach draws the creature's fire.
            let reach = self.objects[i]
                .combat
                .as_ref()
                .and_then(|c| c.weapons.first())
                .map_or(0.0, |&w| crate::combat::reach(w, 0).1);
            let prey = self
                .objects
                .iter()
                .filter(|o| {
                    o.ship.is_some()
                        && o.owner.is_some()
                        && o.owner != Some(crate::combat::MONSTERS)
                })
                .map(|o| (o.x - x, o.y - y))
                .filter(|d| d.0.hypot(d.1) <= reach)
                .min_by(|a, b| a.0.hypot(a.1).total_cmp(&b.0.hypot(b.1)));
            let mut walk = w.walk;
            match w.walk {
                Walk::Appear(left) if left > dt => walk = Walk::Appear(left - dt),
                _ if prey.is_some() => {
                    let (dx, dy) = prey.unwrap();
                    heading = heading_towards(dx, dy);
                    walk = Walk::Throw;
                }
                Walk::Appear(_) | Walk::Throw => walk = Walk::Stand(2.0),
                Walk::Stand(left) if left > dt => walk = Walk::Stand(left - dt),
                Walk::Stand(_) => {
                    walk = match self.roam_target(class, ((x / t) as i32, (y / t) as i32)) {
                        Some((cx, cy)) => Walk::To(cx, cy),
                        None => Walk::Stand(4.0),
                    };
                }
                Walk::To(cx, cy) => {
                    let (tx, ty) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
                    let (dx, dy) = (tx - x, ty - y);
                    let d = dx.hypot(dy);
                    if d < 2.0 {
                        walk = Walk::Stand(3.0);
                    } else {
                        let step = (SPEED * dt).min(d);
                        let (nx, ny) = (x + dx / d * step, y + dy / d * step);
                        if self.is_land((nx / t) as i32, (ny / t) as i32) {
                            self.objects[i].x = nx;
                            self.objects[i].y = ny;
                            heading = heading_towards(dx, dy);
                        } else {
                            walk = Walk::Stand(1.0);
                        }
                    }
                }
            }
            let state = match walk {
                Walk::Appear(_) => 0,
                Walk::To(..) => 1,
                Walk::Stand(_) => 2,
                Walk::Throw => 3,
            };
            if (state, heading) != w.shown {
                let name = walker_sequence(class, state, heading);
                if let Ok(frames) = objects::sequence_frames(data, caches, &name) {
                    if !frames.is_empty() {
                        self.objects[i].frames = frames;
                        self.objects[i].frame_ms = walker_frame_ms(state);
                    }
                }
            }
            self.walkers[k].walk = walk;
            self.walkers[k].shown = (state, heading);
        }
    }

    /// A land cell to wander to (0x413240): any, alike, within the class's
    /// round reach of where the creature stands (0x594ba0 cyclopes,
    /// 0x594e44 minotaurs, 0x5950e8 amazons), that it can walk to.
    fn roam_target(&mut self, class: u32, (cx, cy): (i32, i32)) -> Option<(i32, i32)> {
        let reach: &[&str] = if class == 0x1f4 {
            &CYCLOPS_REACH
        } else {
            &OTHER_REACH
        };
        let n = reach.len() as i32 / 2;
        let inside = |x: i32, y: i32| {
            (x - cx).abs() <= n
                && (y - cy).abs() <= n
                && reach[(y - cy + n) as usize].as_bytes()[(x - cx + n) as usize] == b'#'
        };
        // Those it can reach on land (0x558fa0), within the window.
        let mut seen = std::collections::HashSet::from([(cx, cy)]);
        let mut todo = vec![(cx, cy)];
        let mut cells = Vec::new();
        while let Some((x, y)) = todo.pop() {
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let (nx, ny) = (x + dx, y + dy);
                if (nx - cx).abs() <= n
                    && (ny - cy).abs() <= n
                    && self.is_land(nx, ny)
                    && seen.insert((nx, ny))
                {
                    todo.push((nx, ny));
                    if inside(nx, ny) {
                        cells.push((nx, ny));
                    }
                }
            }
        }
        cells.sort_unstable();
        (!cells.is_empty()).then(|| cells[self.rng.next() as usize % cells.len()])
    }
}
