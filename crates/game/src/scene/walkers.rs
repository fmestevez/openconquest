//! Cyclops, minotaurs and amazons (CyclopClassTy) come out of their lairs
//! (CreaturesFlatClassTy, 0x414f00): a lair keeps up to record field 8 of
//! them about, sending another onto free land beside it now and then (the
//! quicker the higher field 9). They walk their island from half cell to
//! half cell (0x412740, 0x413240, 0x411520) and throw stones, axes or
//! arrows at ships in reach (`combat::creature_weapons`).

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::{ObjectClass, ObjectRecord};

use super::walk_path::{half_path, near_last, wave, DIRS};
use super::Scene;
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
/// The most creatures a jam holds (0x415ee0, message 0x1879f).
const JAM_MEMBERS: usize = 100;
/// Their full life by class (0x413d70 message 2): cyclops, minotaur,
/// amazon.
const FULL_LIFE: [i32; 3] = [30000, 16000, 12000];
/// The throwing picture's frame the throw leaves on (0x412900): cyclops
/// 0xb, minotaur 10, amazon 0x12.
const THROW_FRAME: [i32; 3] = [0xb, 10, 0x12];
/// Where a throw leaves from, by heading, from the picture's top left as
/// the throw takes it (0x412a8d: `+0xb4 - w / 2`, `+0xb8 - h`): cyclops
/// 0x594ae0, minotaur 0x594b20, amazon 0x594b60.
const THROW_FROM: [[(i32, i32); 8]; 3] = [
    [
        (30, 14),
        (44, 11),
        (51, 6),
        (44, 0),
        (31, 0),
        (18, 0),
        (11, 4),
        (16, 11),
    ],
    [
        (44, 15),
        (48, 8),
        (40, 3),
        (27, 0),
        (27, 0),
        (14, 13),
        (20, 18),
        (31, 16),
    ],
    [
        (45, 19),
        (41, 14),
        (31, 10),
        (20, 11),
        (16, 17),
        (19, 23),
        (29, 24),
        (39, 24),
    ],
];
/// Their sounds (0x50e570 kind 5 at their cell), by class, each event's
/// `SND_n` list: appearing (events 0x59-0x5b, 0x413d70), dying when struck
/// (100-102, 0x413ad0), hit (0x6f-0x71, one time in ten, 0x411520),
/// throwing (0x7a-0x7c, one time in ten, 0x412740), walking (0x85-0x87,
/// 0x412740).
const APPEAR_SOUND: [&[&str]; 3] = [&["app3"], &["app3"], &["app3"]];
const DEATH_SOUND: [&[&str]; 3] = [&["deac1d"], &["deac1d"], &["destr1"]];
const HIT_SOUND: [&[&str]; 3] = [&["hc1", "hc2", "hc3", "hc4", "hc5", "hc6"], &[], &[]];
const THROW_SOUND: [&[&str]; 3] = [
    &["c1", "c2", "c3", "c4"],
    &["c5", "c5", "c5", "c5", "c5"],
    &["aas031"],
];
const WALK_SOUND: [&[&str]; 3] = [
    &["lc1", "lc2", "lc3"],
    &["limon2", "limon4", "limon1", "limon3"],
    &[],
];

/// The index of a land creature class in the tables above.
fn kind_of(class: u32) -> usize {
    match class {
        0x1f4 => 0,
        0x1f5 => 1,
        _ => 2,
    }
}

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
    /// Its cell's jam (`+0x8c`).
    #[serde(default)]
    pub jam: u32,
    /// Life in the original's points (`+0x7c`, of 30000 / 16000 / 12000)
    /// and its percentage (`+0x78`); 0 not yet set (an older save).
    #[serde(default)]
    pub life: i32,
    #[serde(default)]
    pub pct: i32,
    /// Struck since its picture last ended (`+0x144`), and the messages
    /// waiting for its tick (`+0x84`: 0x138e blows, 0x138b its death).
    #[serde(default)]
    pub hit: bool,
    #[serde(default)]
    pub queue: Vec<Struck>,
    /// What it throws at: the cell (absolute, `+0x128`, `+0x12c`), its
    /// priority `10 - d` (`+0x130`) and the object's id (`+0x13c`).
    #[serde(default)]
    pub target_cell: (i32, i32),
    #[serde(default)]
    pub prio: u32,
    #[serde(default)]
    pub target_id: u32,
    /// Throwing (`+0x134`), since tick `throw_at` (`+0x138`), thrown this
    /// time (`+0x140`).
    #[serde(default)]
    pub throwing: bool,
    #[serde(default)]
    pub throw_at: u32,
    #[serde(default)]
    pub thrown: bool,
    /// Removed (message 10) at the end of its dying picture.
    #[serde(default)]
    pub gone: bool,
}

/// A message waiting in a land creature's list (`+0x84`): a blow
/// (0x138e, `points` and the player it is put down to, the message's
/// `+0x15`) or its death (0x138b).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Struck {
    Blow { points: i32, by: Option<(u32, u32)> },
    Death,
}

/// A land creatures' jam (CyclopsJamClassTy 0x1f7, made by 0x416230 for
/// message 0x1498): the occupant of a cell they stand on, holding their
/// ids in the order they joined (`+0x74` of them, `+0x78` pairs).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Jam {
    pub id: u32,
    pub cell: (i32, i32),
    pub members: Vec<u32>,
}

/// A throw made this tick (message 0x14ba from 0x412740 state 3): at
/// `target` with the creature's weapon, hitting (`+0x3c` 100) or not
/// (0), by a creature at `pct` of its life, from `from` (`+0x25` /
/// `+0x29`) to `to` (`+0x1d` / `+0x21`, the target's vt+0xc; `None` where
/// the port does not have it).
#[derive(Debug, Clone, Copy)]
pub struct Throw {
    pub target: u32,
    pub weapon: u8,
    pub shooter: u32,
    pub strikes: bool,
    pub pct: i32,
    pub from: (i32, i32),
    pub to: Option<(i32, i32)>,
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

    /// Its cell (0x411ca0): its half over 2 from the grid's origin.
    fn cell(&self) -> (i32, i32) {
        let (ox, oy, _, _) = self.grid.unwrap_or_default();
        (ox + self.half.0 / 2, oy + self.half.1 / 2)
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
        let taken = |s: &Scene, cx: i32, cy: i32| s.walker_at((cx, cy));
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
    /// `(cx, cy)`, sent out by `lair`. Message 2 (0x413d70): its life full
    /// (30000 / 16000 / 12000), on its cell's north-west half (`+0x90 = 2
    /// (x - origin)`, 0x411e7f) of its walk grid (0x411ce0, 0x42e720); the
    /// cell's jam its own, a new one made there if the cell has none
    /// (message 0x1498, 0x416230), joined (0x1879f); its generator seeded
    /// `[game +0x2ee5] x id`, state 0, heading 0, its appearing sound
    /// (events 0x59-0x5b) at its cell, one draw (`+0x150`); its picture
    /// set and drawn (0x4111b0, 0x411520).
    pub(super) fn put_walker(
        &mut self,
        lair: u32,
        class: u32,
        (cx, cy): (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        let id = self.next_walker_id();
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
            let life = FULL_LIFE[kind_of(class)];
            let mut w = Walker {
                id,
                lair,
                grid: Some(grid),
                half: (2 * (cx - ox), 2 * (cy - oy)),
                life,
                pct: 100,
                ..Walker::default()
            };
            w.halves = self.terrain_halves(grid);
            w.pos = w.pixel(w.half);
            (obj.x, obj.y) = (w.pos.0 as f32, w.pos.1 as f32);
            self.objects.push(obj);
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            let cell = w.cell();
            self.walkers.push(w);
            let k = self.walkers.len() - 1;
            let jam = self.jam_for(cell);
            self.walkers[k].jam = jam;
            self.jam_join(jam, id);
            self.walkers[k].rng = self.objects_seed().wrapping_mul(id);
            self.walker_sound(k, class, &APPEAR_SOUND);
            self.walkers[k].draw();
            self.walker_picture_set(k, data, caches);
            self.walker_picture(k, data, caches);
        }
    }

    /// The next object id, past the jams' too (each jam is an object of
    /// the original's, its id taken from the same count).
    fn next_walker_id(&self) -> u32 {
        let objects = self.objects.iter().map(|o| o.id).max().unwrap_or(0);
        let jams = self.jams.iter().map(|j| j.id).max().unwrap_or(0);
        objects.max(jams) + 1
    }

    /// The jam on `cell`, a new one made there when it has none (message
    /// 0x1498 through Players vt+8 with `{x, y, 1}`, 0x416230: class
    /// 0x1f7, side 2, empty).
    fn jam_for(&mut self, cell: (i32, i32)) -> u32 {
        if let Some(j) = self.jams.iter().find(|j| j.cell == cell) {
            return j.id;
        }
        let id = self.next_walker_id();
        self.jams.push(Jam {
            id,
            cell,
            members: Vec::new(),
        });
        id
    }

    /// Creature `id` joins jam `jam` (message 0x1879f, 0x415ee0): at the
    /// end of its members unless already there, while it holds fewer than
    /// 100.
    fn jam_join(&mut self, jam: u32, id: u32) {
        if let Some(j) = self.jams.iter_mut().find(|j| j.id == jam) {
            if j.members.len() < JAM_MEMBERS && !j.members.contains(&id) {
                j.members.push(id);
            }
        }
    }

    /// Creature `id` leaves jam `jam` (message 0x187a0): the members after
    /// it close up; the last gone, the jam leaves its cell and is removed
    /// (0x499860, message 10).
    fn jam_leave(&mut self, jam: u32, id: u32) {
        if let Some(j) = self.jams.iter_mut().find(|j| j.id == jam) {
            j.members.retain(|&m| m != id);
        }
        self.jams.retain(|j| j.id != jam || !j.members.is_empty());
    }

    /// Walker `k` plays one of `sounds` for its class at its cell
    /// (0x50e570 kind 5). (Several to pick from: the original's player
    /// takes `rand() % count`, the C library's generator; the port's
    /// stand-in for it is `gull_rng`, as for the buildings' calls.)
    fn walker_sound(&mut self, k: usize, class: u32, sounds: &[&[&'static str]; 3]) {
        let list = sounds[kind_of(class)];
        let pick = match list.len() {
            0 => return,
            1 => list[0],
            n => list[self.gull_rng.next() as usize % n],
        };
        let t = TILE_SIZE as f32;
        let (cx, cy) = self.walkers[k].cell();
        self.sounds
            .push((pick, cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0));
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

    /// The object holding cell `(x, y)`'s occupant slot (`map + 8 +
    /// 24 n + 4`), but for a land creatures' jam: a ship (the port's ships hold
    /// only the cell they are in) or a building whose footprint covers
    /// the cell, while it stands (not ruins).
    fn slot_object(&self, (x, y): (i32, i32)) -> Option<usize> {
        if let Some(k) = self.slot_ship((x, y)) {
            return Some(k);
        }
        self.objects.iter().position(|o| {
            matches!(
                o.class,
                ObjectClass::Building(_)
                    | ObjectClass::Barbarian(_)
                    | ObjectClass::Castle(_)
                    | ObjectClass::HarpyNest
                    | ObjectClass::CreatureSpawn
                    | ObjectClass::HeroHouse
            ) && o.combat.is_some()
                && o.cell.is_some_and(|(bx, by)| {
                    let n = super::trade::building_cells(o);
                    (bx..bx + n).contains(&x) && (by..by + n).contains(&y)
                })
        })
    }

    /// Whether a land creature stands on cell `cell`: its jam fills the
    /// cell's object slot (0x499800).
    pub(super) fn walker_at(&self, cell: (i32, i32)) -> bool {
        self.jams.iter().any(|j| j.cell == cell)
    }

    fn is_land(&self, cx: i32, cy: i32) -> bool {
        let (w, h) = self.map_size();
        cx >= 0 && cy >= 0 && (cx as u32) < w && (cy as u32) < h && !self.nav.is_water(cx, cy)
    }

    /// Lairs send out creatures; those removed (message 10 at the end of
    /// their dying picture or at lightning's kill, or gone otherwise) leave
    /// their jams (message 3: 0x187a0) and the map.
    pub(super) fn run_walkers(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.run_lairs(dt, data, caches);
        let walkers = std::mem::take(&mut self.walkers);
        let (gone, kept): (Vec<Walker>, Vec<Walker>) = walkers.into_iter().partition(|w| {
            w.gone
                || !self.object_index(w.id).is_some_and(|i| {
                    self.objects[i]
                        .combat
                        .as_ref()
                        .is_some_and(|c| c.hull > 0.0)
                })
        });
        self.walkers = kept;
        for w in gone {
            self.jam_leave(w.jam, w.id);
            self.objects.retain(|o| o.id != w.id);
        }
    }

    /// Each land creature's tick (message 0, 0x413d70): every 5 ticks,
    /// unless hit or dying, its walk grid refreshed about it (0x411ef0)
    /// and a look about (0x412290), the throw given up when nothing is
    /// found; then its waiting messages (0x413ad0), its state's step
    /// (0x412740) and its picture's (0x411520).
    pub(super) fn tick_walkers(&mut self, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        for k in 0..self.walkers.len() {
            if self.walkers[k].gone {
                continue;
            }
            let Some(i) = self.object_index(self.walkers[k].id) else {
                continue;
            };
            self.walker_life_init(k, i);
            if now >= self.walkers[k].scan {
                self.walkers[k].scan = now + SCAN_TICKS;
                if !matches!(self.walkers[k].state, DYING | HIT) {
                    self.refresh_halves(k);
                    let found = self.walker_look(k);
                    let w = &mut self.walkers[k];
                    if !found && w.state == THROW {
                        w.target = false;
                        w.state = STAND;
                        self.walker_picture_set(k, data, caches);
                    }
                }
            }
            self.walker_messages(k, data, caches);
            self.walker_step(k, data, caches);
            self.walker_picture(k, data, caches);
            let Some(i) = self.object_index(self.walkers[k].id) else {
                continue;
            };
            let (x, y) = self.walkers[k].pos;
            let o = &mut self.objects[i];
            (o.x, o.y) = (x as f32, y as f32);
        }
    }

    /// A walker of an older save takes its life and percentage from its
    /// hull.
    fn walker_life_init(&mut self, k: usize, i: usize) {
        if self.walkers[k].life > 0 {
            return;
        }
        let max = FULL_LIFE[kind_of(self.objects[i].record.class_id)];
        let hull = self.objects[i].combat.as_ref().map_or(1.0, |c| c.hull);
        let w = &mut self.walkers[k];
        w.life = ((hull * 15.0).round() as i32).clamp(1, max);
        w.pct = (w.life * 100 / max).max(1);
    }

    /// Walker `k`'s life to its object's hull (the port's points, a
    /// fifteenth of the original's).
    fn walker_hull(&mut self, k: usize) {
        let (id, life) = (self.walkers[k].id, self.walkers[k].life);
        if let Some(i) = self.object_index(id) {
            if let Some(c) = self.objects[i].combat.as_mut() {
                c.hull = life as f32 / 15.0;
            }
        }
    }

    /// A blow of `damage` (the port's points) reaches land creature `id`
    /// (vt+4 0x4139e0 with 0x138e): kept for its tick.
    pub(super) fn walker_struck(&mut self, id: u32, damage: f32, by: Option<(u32, u32)>) {
        if let Some(w) = self.walkers.iter_mut().find(|w| w.id == id) {
            let points = (damage * 15.0).round() as i32;
            w.queue.push(Struck::Blow { points, by });
        }
    }

    /// Walker `k`'s waiting messages (0x413ad0), in order, each taken off
    /// as read: its death (0x138b), unless dying, sets it dying (state 5,
    /// percentage 1, the dying picture from its first frame), crying out
    /// (events 100-102) if a blow struck it since its picture last ended
    /// (`+0x144`); a blow (0x138e), unless dying, marks it struck and takes
    /// the points off its life while that leaves it a percentage above 0,
    /// else puts the death down to the blow's player (0x50e750), leaves it
    /// at life and percentage 1 and adds its death to the list.
    fn walker_messages(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let Some(i) = self.object_index(self.walkers[k].id) else {
            return;
        };
        let class = self.objects[i].record.class_id;
        let max = FULL_LIFE[kind_of(class)];
        while !self.walkers[k].queue.is_empty() {
            match self.walkers[k].queue.remove(0) {
                Struck::Death => {
                    if self.walkers[k].state == DYING {
                        continue;
                    }
                    let w = &mut self.walkers[k];
                    w.state = DYING;
                    w.pct = 1;
                    self.walker_picture_set(k, data, caches);
                    let w = &mut self.walkers[k];
                    w.frame = w.first;
                    if w.hit {
                        self.walker_sound(k, class, &DEATH_SOUND);
                    }
                }
                Struck::Blow { points, by } => {
                    let w = &mut self.walkers[k];
                    if w.state == DYING {
                        continue;
                    }
                    w.hit = true;
                    if points < w.life && (w.life - points) * 100 / max != 0 {
                        w.life -= points;
                        w.pct = w.life * 100 / max;
                    } else {
                        w.pct = 1;
                        w.life = 1;
                        w.queue.push(Struck::Death);
                        self.note_destroyed(i, by);
                    }
                }
            }
        }
        self.walker_hull(k);
    }

    /// Lightning struck at `struck` reaches land creature `id` through its
    /// jam (0x413d70 message 0xda93, 0x414b0f): its walk grid rebuilt about
    /// it (0x411ef0); on the struck cell, or its half shut now, its waiting
    /// messages dropped and it dies; else 25 points off its percentage,
    /// its life that percentage of the full (and its picture again,
    /// 0x4111b0), dying at 0 or less with life and percentage 1. Dying, its
    /// death is added and the messages read (0x413ad0), the death put down
    /// to the caster (0x50e750) and it is removed at once (message 10).
    pub(super) fn walker_lightning(
        &mut self,
        id: u32,
        struck: (i32, i32),
        caster: Option<(u32, u32)>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(k) = self.walkers.iter().position(|w| w.id == id && !w.gone) else {
            return;
        };
        let Some(i) = self.object_index(id) else {
            return;
        };
        let max = FULL_LIFE[kind_of(self.objects[i].record.class_id)];
        self.refresh_halves(k);
        let w = &mut self.walkers[k];
        if struck == w.cell() || w.at(w.half) != 0 {
            w.queue.clear();
        } else {
            w.pct -= 25;
            if w.pct > 0 {
                w.life = max * w.pct / 100;
                self.walker_hull(k);
                self.walker_picture_set(k, data, caches);
                return;
            }
            w.life = 1;
            w.pct = 1;
        }
        let w = &mut self.walkers[k];
        if w.state != DYING {
            w.queue.push(Struck::Death);
        }
        self.walker_messages(k, data, caches);
        if let Some(i) = self.object_index(id) {
            self.note_destroyed(i, caster);
        }
        self.walkers[k].gone = true;
    }

    /// Walker `k` looks about (0x412290; not while appearing or crowded,
    /// `+0x124`). Throwing, it first drops its target if the object on the
    /// target's cell is not it any more, or is a ship with the phantom
    /// (`+0x253`) or Orpheus aboard (0x4ba790 hero 2), or a building
    /// 0x32-0x3e with the phantom (vt+0xa4, 0x40dfd0: `+0x134`). Then the
    /// cells of its mask about its cell (value 1, row by row; on the map),
    /// each's occupant if there (vt+0x2c; the port: standing, life left)
    /// and none of a jam, a castle (0x42, 0x43), a harpy nest (0x44,
    /// 0x45) or a lair (0x46-0x48), nor a ship on its reserved cell, with
    /// the phantom or Orpheus, nor a building 0x32-0x3e with the phantom:
    /// whoever's it is. The first found, or one whose `10 - d` (`d` by
    /// 0x5576d8 from its cell) is above what it holds, unsigned, becomes
    /// its target, its throw started afresh. Something found, it tells its
    /// cell's jam (the mask's centre, inside its walk grid), which tells
    /// the others there (message 0x187a2 with the priority and the target
    /// cell).
    fn walker_look(&mut self, k: usize) -> bool {
        let Some(class) = self
            .object_index(self.walkers[k].id)
            .map(|i| self.objects[i].record.class_id)
        else {
            return false;
        };
        let w = &self.walkers[k];
        if w.state == APPEAR || w.crowded {
            return false;
        }
        if w.state == THROW && self.target_lost(k) {
            self.walkers[k].target = false;
        }
        let mask = if class == 0x1f4 {
            &CYCLOPS_MASK
        } else {
            &OTHER_MASK
        };
        let w = &self.walkers[k];
        let (ox, oy, gw, gh) = w.grid.unwrap_or_default();
        let (hx, hy) = (w.half.0 / 2, w.half.1 / 2);
        let own = (ox + hx, oy + hy);
        let (mw, mh) = (self.grid.width as i32, self.grid.height as i32);
        let mut found = false;
        for (j, row) in mask.iter().enumerate() {
            for (i, &on) in row.iter().enumerate() {
                if on != 1 {
                    continue;
                }
                let cell = (ox + hx - 6 + i as i32, oy + hy - 6 + j as i32);
                if cell.0 < 0 || cell.0 >= mw || cell.1 < 0 || cell.1 >= mh {
                    continue;
                }
                let Some(t) = self.slot_object(cell) else {
                    continue;
                };
                if !self.walker_prey(t) {
                    continue;
                }
                found = true;
                let (dx, dy) = ((cell.0 - own.0) as i64, (cell.1 - own.1) as i64);
                let d = super::fort::root((dx * dx + dy * dy) as u64);
                let prio = 10u32.wrapping_sub(d);
                let id = self.objects[t].id;
                let w = &mut self.walkers[k];
                if !w.target || prio > w.prio {
                    w.throwing = false;
                    w.throw_at = 0;
                    w.crowded = false;
                    w.target = true;
                    w.prio = prio;
                    w.target_cell = cell;
                    w.target_id = id;
                }
            }
        }
        if found && (0..gw).contains(&hx) && (0..gh).contains(&hy) {
            let w = &self.walkers[k];
            let (me, prio, cell) = (w.id, w.prio, w.target_cell);
            let members = self
                .jams
                .iter()
                .find(|j| j.cell == own)
                .map(|j| j.members.clone())
                .unwrap_or_default();
            for m in members {
                self.walker_alerted(m, me, prio, cell);
            }
        }
        found
    }

    /// Whether the object `t` in a cell's slot is one a land creature
    /// throws at (0x412290; see `walker_look`).
    fn walker_prey(&self, t: usize) -> bool {
        let o = &self.objects[t];
        if !o.combat.as_ref().is_some_and(|c| c.hull > 0.0) {
            return false;
        }
        if matches!(o.record.class_id, 0x42..=0x48) {
            return false;
        }
        !self.prey_veiled(t)
    }

    /// A ship with the phantom or Orpheus aboard, or a building 0x32-0x3e
    /// with the phantom (0x412290).
    fn prey_veiled(&self, t: usize) -> bool {
        let o = &self.objects[t];
        let phantom = o.combat.as_ref().is_some_and(|c| c.phantom > 0.0);
        match &o.ship {
            Some(s) => phantom || s.has_hero(objects::ORPHEUS),
            None => matches!(o.record.class_id, 0x32..=0x3e) && phantom,
        }
    }

    /// Whether walker `k`'s target is no longer on its cell (0x412290 in
    /// state 3: the cell's occupant gone or another, or veiled).
    fn target_lost(&self, k: usize) -> bool {
        let w = &self.walkers[k];
        match self.slot_object(w.target_cell) {
            None => true,
            Some(t) => self.objects[t].id != w.target_id || self.prey_veiled(t),
        }
    }

    /// Land creature `id` hears from `from`, of its jam, of a target at
    /// `cell` of priority `prio` (message 0x187a2, 0x413d70): unless it is
    /// the sender, with no target and not crowded, or holding a lower
    /// priority (unsigned), it pursues the cell (`+0xd0`, `+0xd4`,
    /// `+0xd8`; an absolute cell, read as a half of its grid, as written),
    /// takes the priority and is crowded (`+0x124`).
    fn walker_alerted(&mut self, id: u32, from: u32, prio: u32, cell: (i32, i32)) {
        if id == from {
            return;
        }
        let Some(w) = self.walkers.iter_mut().find(|w| w.id == id) else {
            return;
        };
        if (!w.target && !w.crowded) || w.prio < prio {
            w.pursue = true;
            w.crowded = true;
            w.prio = prio;
            w.pursue_to = (cell.0 as u16 as i32, cell.1 as u16 as i32);
        }
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
                    w.state = THROW;
                    self.walker_picture_set(k, data, caches);
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
            THROW => self.throw_step(k, data, caches),
            _ => {}
        }
    }

    /// Throwing, each tick (0x412740 state 3), a draw first. Not yet
    /// throwing (`+0x134`), it starts once `throw_at + 40 +- 4` (+ with
    /// bit 16 of the draw) is below the tick, and turns to its target: the
    /// last of the eight headings (0x5c4f4c) whose signs match those of
    /// the target cell less its own, its picture again if it changed, at
    /// the same frame. Throwing, at the picture's frame 0xb / 10 / 0x12
    /// and once (`+0x140`), with its target still there (0x51f1f0): a
    /// draw, its cry one time in ten (events 0x7a-0x7c); at `d` cells
    /// (0x5576d8) above 1 a draw, missing (`+0x3c` 0) when it is a
    /// multiple of `8 - d`, else hitting (100); the throw (message 0x14ba:
    /// its player 0xff, its weapon's level-1 blow by `throw_blow`). It
    /// aims where the target's vt+0xc gives with the hit byte (a ship's
    /// 0x4e07c0, `Scene::ship_aim`) and leaves from `+0xb4 - w / 2 +
    /// T[heading].x`, `+0xb8 - h + T[heading].y` (`THROW_FROM`), `w` and
    /// `h` its picture's size (`+0x61` / `+0x65`, from the sequence's
    /// `+0x29` / `+0x2b`, 0x5463bf: its first frame's width and height,
    /// 0x554bf0). (Not ported: a building's aim, 0x423520, draws from the
    /// building's own generator (`+0x18`, seeded `[game +0x2ee5] x id` at
    /// 0x4281a2), which the port does not keep, nor its other draws
    /// (0x4281de, 0x424185, 0x4244a4, the forts' 0x41a895 / 0x41bc61, the
    /// towns' 0x430449 / 0x441e9c / 0x4422ad); a throw at one aims as the
    /// port's other shots.)
    fn throw_step(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let Some(i) = self.object_index(self.walkers[k].id) else {
            return;
        };
        let class = self.objects[i].record.class_id;
        let w = &mut self.walkers[k];
        let r = w.draw();
        if !w.throwing {
            let late: i32 = if r & 1 != 0 { 4 } else { -4 };
            if w.throw_at.wrapping_add(40).wrapping_add_signed(late) < now {
                w.throwing = true;
                w.thrown = false;
                w.throw_at = now;
            }
            let (ox, oy, _, _) = w.grid.unwrap_or_default();
            let dx = w.target_cell.0 - w.half.0 / 2 - ox;
            let dy = w.target_cell.1 - w.half.1 / 2 - oy;
            let (old, frame) = (w.heading, w.frame);
            for (d, &(x, y)) in DIRS.iter().enumerate() {
                if dx.signum() == x.signum() && dy.signum() == y.signum() {
                    w.heading = d as u8;
                }
            }
            if w.heading != old {
                self.walker_picture_set(k, data, caches);
                self.walkers[k].frame = frame;
            }
            return;
        }
        if w.frame != THROW_FRAME[kind_of(class)] || w.thrown {
            return;
        }
        let target = w.target_id;
        if self.object_index(target).is_none() {
            return;
        }
        if self.walkers[k].draw() % 10 == 0 {
            self.walker_sound(k, class, &THROW_SOUND);
        }
        let w = &mut self.walkers[k];
        let own = w.cell();
        let (dx, dy) = (
            (w.target_cell.0 - own.0) as i64,
            (w.target_cell.1 - own.1) as i64,
        );
        let d = super::fort::root((dx * dx + dy * dy) as u64);
        // (`d` is at most 5 within the masks, so `8 - d` is not 0.)
        let strikes = !(d > 1 && w.draw() % (8 - d) == 0);
        w.thrown = true;
        let (shooter, pct, (x, y), heading) = (w.id, w.pct, w.pos, w.heading as usize);
        let to = self
            .object_index(target)
            .filter(|&t| self.objects[t].ship.is_some())
            .map(|t| self.ship_aim(t, strikes));
        let (pw, ph) = self.objects[i]
            .frames
            .first()
            .and_then(|f| f.first())
            .map_or((0, 0), |l| (l.image.width as i32, l.image.height as i32));
        let (tx, ty) = THROW_FROM[kind_of(class)][heading.min(7)];
        self.throws.push(Throw {
            target,
            weapon: crate::combat::creature_weapons(class)[0],
            shooter,
            strikes,
            pct,
            from: (x - pw / 2 + tx, ty - ph + y),
            to,
        });
    }

    /// Walking, each tick (0x412740 state 2): its walking sound when due
    /// (events 0x85-0x87), its next time a draw on; on the tick after a
    /// half step ends it takes the route's next half (if it is still open,
    /// else the route is shut), leaving its jam for the new cell's when
    /// the cell changes (0x412cf5), and its picture again; then, at the
    /// route's end, shut, or with a target or a pursuit, it stops
    /// (`walk_end`); else it starts the half step towards the entry
    /// after, turning its picture to that entry's direction.
    fn walk_step(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        if now >= self.walkers[k].sound_at {
            if let Some(i) = self.object_index(self.walkers[k].id) {
                let class = self.objects[i].record.class_id;
                self.walker_sound(k, class, &WALK_SOUND);
            }
            let w = &mut self.walkers[k];
            let r = w.draw();
            w.sound_at = now + ((r % 41 + 60) * 375) / 100;
        }
        let w = &mut self.walkers[k];
        if w.moving {
            return;
        }
        if w.step < w.path.len() {
            let (x, y, _) = w.path[w.step];
            if w.at((x, y)) == 0 {
                let changed = (w.half.0 / 2, w.half.1 / 2) != (x / 2, y / 2);
                w.half = (x, y);
                w.pos = w.pixel(w.half);
                if changed {
                    let (id, old, cell) = (w.id, w.jam, w.cell());
                    self.jam_leave(old, id);
                    let jam = self.jam_for(cell);
                    self.walkers[k].jam = jam;
                    self.jam_join(jam, id);
                }
                self.walkers[k].step += 1;
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

    /// The end of a walk (0x412740): another member of its jam on its half
    /// (0x415e40), it chooses a route at once, without turning, and is
    /// crowded (`+0x124`); else it throws if it has a target, or stands,
    /// choosing again in 40 ticks.
    fn walk_end(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        self.walkers[k].blocked = false;
        let me = &self.walkers[k];
        let crowded = self.jams.iter().find(|j| j.id == me.jam).is_some_and(|j| {
            j.members
                .iter()
                .any(|&m| m != me.id && self.walkers.iter().any(|o| o.id == m && o.half == me.half))
        });
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
            w.state = THROW;
            self.walker_picture_set(k, data, caches);
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
    /// amazon from 0, or 0x14 / 0x13 after, to the end) and turning (12
    /// or, for the amazon, 9 frames a heading, from the heading's to the
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
    /// it stands, its walking sound's time a draw on; walking, the half
    /// step is over (the frame back to the first, or to 0x16 / 0x14 / 0x13
    /// after the first steps); throwing, the throw is over (`+0x134`);
    /// hit, it goes back to the state it was in; dying, it is removed
    /// (message 10); turning, it stands with the new heading; else it
    /// loops. Then, struck since (`+0x144`) and not dying, it shows the
    /// hit (state 4, back to this state after), crying one time in ten
    /// (a draw; events 0x6f-0x71). Standing, after its first frame each
    /// frame takes 5 ticks, not 25. Throwing but not yet started, it holds
    /// its first frame and shows a hit at once (no draw). Walking, its
    /// place goes from the half step's start to its end with the frame
    /// (`(to - from) x (frame - f0) / (end - f0)`, the first frames holding
    /// it: cyclops below 7, f0 6 on the first steps; minotaur 9 / 8; amazon
    /// 10 / 9). Dying, a minotaur's or amazon's frames past 0xb take twice
    /// as long.
    fn walker_picture(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let Some(i) = self.object_index(self.walkers[k].id) else {
            return;
        };
        let class = self.objects[i].record.class_id;
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
                        self.walker_picture_set(k, data, caches);
                        let w = &mut self.walkers[k];
                        let r = w.draw();
                        w.sound_at = ((r % 41 + 60) * 375) / 100 + now;
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
                    THROW => {
                        w.frame = w.first;
                        w.throwing = false;
                    }
                    HIT => {
                        w.state = w.back;
                        self.walker_picture_set(k, data, caches);
                    }
                    DYING => w.gone = true,
                    TURN_ROUND => {
                        w.state = STAND;
                        w.heading = w.turn_to;
                        self.walker_picture_set(k, data, caches);
                    }
                    _ => w.frame = w.first,
                }
                let w = &mut self.walkers[k];
                if w.hit && w.state != DYING {
                    w.hit = false;
                    w.back = w.state;
                    w.state = HIT;
                    self.walker_picture_set(k, data, caches);
                    if self.walkers[k].draw() % 10 == 0 {
                        self.walker_sound(k, class, &HIT_SOUND);
                    }
                }
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
            THROW if !w.throwing => {
                w.frame = w.first;
                if w.hit {
                    w.hit = false;
                    w.back = THROW;
                    w.state = HIT;
                    self.walker_picture_set(k, data, caches);
                    let w = &mut self.walkers[k];
                    w.frame = w.first;
                }
            }
            _ => {}
        }
        let w = &mut self.walkers[k];
        w.last = now;
        w.drawn = true;
        let (frame, count) = (w.frame, w.count.max(1));
        self.objects[i].progress = Some((frame.clamp(0, count - 1) as f32 + 0.5) / count as f32);
    }
}
