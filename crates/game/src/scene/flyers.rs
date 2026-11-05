//! Harpies and dragons (HarpyClassTy, 0x4207b0): each nest (0x41d4a0)
//! sends out its creature when the mission starts and, while it has
//! creatures left (record field 9), a new one some time after the last
//! fell (55500 ticks less 6000 per field 10). A creature runs the
//! original's tick (message 0) every game tick: its states (0x41fbd0, set
//! by 0x41f680) then its picture and place (0x41e4b0), with its own
//! generator (`+0x18`, seeded `[game +0x2ee5] x id`). It rests on its
//! nest (state 1: `r % (1100 - 100 x` field 11`)` ticks; a harpy plays
//! its idle picture every `r % 50` ticks; a dragon only takes off while
//! someone is in its range, 0x41ef00), takes off (2), and flies legs
//! (3): a cell picked by weight within the 13 x 13 mask at 0x595a30 and
//! the nest's range (0x41e840), reached by a turning route (0x55c3d0,
//! one heading change a cell), a cell a cycle of its flying picture,
//! turning (4) where the route bends, hovering where another flyer holds
//! or is bound for the next cell (0x41ee20). At each cell it looks
//! about (0x41efb0) and strikes (5, 0x41f1c0); after a strike it heads
//! home (`+0xc6`), which it only flies straight for when off both the
//! nest's row and column, and lands (6) on the nest to rest again.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::{ObjectClass, ObjectRecord};

use super::fire_pass::Held;
use super::walk_path::DIRS;
use super::Scene;
use crate::assets::GameData;
use crate::objects::{self, Caches};

/// Harpy, dragon (record classes).
const HARPY: u32 = 0x1f8;
const DRAGON: u32 = 0x1f9;
/// The states (`+0x88`): appearing, resting, taking off, flying, turning,
/// striking, landing, dying (0x41f680).
const APPEAR: u8 = 0;
const REST: u8 = 1;
const TAKE_OFF: u8 = 2;
const FLY: u8 = 3;
const TURN: u8 = 4;
const STRIKE: u8 = 5;
const LAND: u8 = 6;
const DYING: u8 = 7;
/// Flying's substates (`+0xe0`): moving a step, holding.
const MOVING: u32 = 1;
const HOLDING: u32 = 3;
/// Ticks a frame by state, a harpy's (0x595d10) and a dragon's
/// (0x595d30); index 4 also serves a flyer holding (0x41e130).
const HARPY_DELAYS: [u32; 8] = [1, 2, 1, 1, 0, 1, 1, 1];
const DRAGON_DELAYS: [u32; 8] = [0, 0, 1, 1, 0, 1, 1, 1];
/// The cells a leg may end on, around the flyer (0x595a30, 13 x 13,
/// row-major; only 1s qualify, the centre is 2).
const LEG_MASK: [[u8; 13]; 13] = [
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0],
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0],
    [0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0],
    [0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0],
];
/// The striking picture's frame at which the blow falls: 7 for a harpy,
/// 16 for a dragon (0x41fbd0).
const HARPY_BLOW_FRAME: i32 = 7;

/// A harpy's aim offsets 0x595ae0 by frame `{x2, y2, x6, y6}` (taking off,
/// landing; resting the first `{x2, y2}`), dumped from the exe.
pub(super) const AIM_OFFSETS: [(i32, i32, i32, i32); 35] = [
    (40, 116, 40, 40),
    (40, 116, 40, 40),
    (40, 116, 40, 40),
    (40, 115, 40, 40),
    (40, 114, 40, 40),
    (40, 114, 40, 40),
    (40, 113, 40, 40),
    (40, 112, 40, 44),
    (40, 111, 40, 48),
    (40, 111, 40, 52),
    (40, 112, 40, 57),
    (40, 112, 40, 60),
    (40, 107, 40, 64),
    (40, 102, 40, 67),
    (40, 99, 40, 70),
    (40, 95, 40, 74),
    (40, 92, 40, 77),
    (40, 87, 40, 81),
    (40, 83, 40, 85),
    (40, 78, 40, 88),
    (40, 75, 40, 91),
    (40, 70, 40, 94),
    (40, 66, 40, 97),
    (40, 63, 40, 101),
    (40, 54, 40, 105),
    (40, 54, 40, 107),
    (40, 50, 40, 110),
    (40, 46, 40, 109),
    (40, 41, 40, 111),
    (40, 41, 40, 111),
    (40, 40, 40, 112),
    (40, 39, 40, 114),
    (40, 39, 40, 115),
    (40, 40, 40, 116),
    (40, 40, 40, 116),
];
const DRAGON_BLOW_FRAME: i32 = 16;
/// Harpies leave ships with Orpheus aboard alone (0x41efb0, 0x41f1c0), and
/// their shots do Odysseus's ship no harm (0x4bf170).
const ORPHEUS: u8 = 2;
const ODYSSEUS: u8 = 7;

/// A creature out of its nest: the original's fields, by offset.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Flyer {
    pub id: u32,
    /// Its nest (`+0xb2`) and whether it still has one (`+0xae`).
    pub nest: u32,
    #[serde(default)]
    pub has_nest: bool,
    /// Its own generator (`+0x18`).
    #[serde(default)]
    pub rng: u32,
    /// State `+0x88` and heading `+0xad` (0-7, 0x5c4f4c).
    #[serde(default)]
    pub state: u8,
    #[serde(default)]
    pub heading: u8,
    /// Its cell (`+0x8d`, `+0x91`): the one whose flyer slot it holds.
    #[serde(default)]
    pub cell: (i32, i32),
    /// The nest's cell (`+0xbe`, `+0xc2`) and range either way (`+0x95`,
    /// `+0x99`: the nest's fields 7, 8), the rest parameter (`+0x9d`:
    /// field 11).
    #[serde(default)]
    pub home: (i32, i32),
    #[serde(default)]
    pub range: (i32, i32),
    #[serde(default)]
    pub rest: u32,
    /// Where it is, world pixels (`+0xa1`, `+0xa5`).
    #[serde(default)]
    pub pos: (i32, i32),
    /// Orphaned: the tick and hp its decay started from (`+0xb6`, `+0xba`).
    #[serde(default)]
    pub orphan_at: u32,
    #[serde(default)]
    pub orphan_hp: u32,
    /// Heading home (`+0xc6`); hit since its rest began (`+0xca`).
    #[serde(default)]
    pub homing: bool,
    #[serde(default)]
    pub hit: bool,
    /// The nest's attacker it chases (`+0xce`, `+0xd2`) and whether that
    /// leg has been flown (`+0xd6`).
    #[serde(default)]
    pub forced: Option<u32>,
    #[serde(default)]
    pub forced_done: bool,
    /// Turning: the heading turned to (`+0xda`), done (`+0xdb`), the state
    /// to go back to (`+0xdf`).
    #[serde(default)]
    pub turn_to: u8,
    #[serde(default)]
    pub turned: bool,
    #[serde(default)]
    pub back: u8,
    /// `+0xe0`: a picture-done flag (states 0, 1, 2, 6), the substate
    /// (state 3), the target's id (state 5).
    #[serde(default)]
    pub sub: u32,
    /// A step's picture done (`+0xe1`, state 3).
    #[serde(default)]
    pub stepped: bool,
    /// `+0xe4`: the idle picture done (state 1), the strike's heading
    /// (state 5).
    #[serde(default)]
    pub e4: u32,
    /// The blow struck (`+0xe5`) and the strike's picture done (`+0xe9`),
    /// state 5.
    #[serde(default)]
    pub fired: bool,
    #[serde(default)]
    pub struck: bool,
    /// The next idle picture's tick (`+0xe8`) and the rest's end (`+0xec`).
    #[serde(default)]
    pub idle_at: u32,
    #[serde(default)]
    pub rest_end: u32,
    /// The route (`+0x80`, `+0x109` entries: x, y, the heading entered
    /// with) and the entry it flies to (`+0x10d`).
    #[serde(default)]
    pub route: Vec<(i32, i32, u8)>,
    #[serde(default)]
    pub index: usize,
    /// The step's ends, pixels (`+0x101`/`+0x105` from, `+0xf9`/`+0xfd`
    /// to).
    #[serde(default)]
    pub from: (i32, i32),
    #[serde(default)]
    pub to: (i32, i32),
    /// The next wing-flap sound's tick (`+0x111`).
    #[serde(default)]
    pub flap: u32,
    /// The picture: frame `+0x45`, first `+0x4d`, end `+0x51`, count
    /// `+0x49`, ticks a frame `+0x7d`, tick of the last step `+0x81`,
    /// drawn once `+0xa9`.
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
    /// Removed at its dying picture's end (message 10).
    #[serde(default)]
    pub gone: bool,
    /// Its picture's masks (`+0x7c`: `..M`, 0x41e130), loaded with it.
    #[serde(skip)]
    pub masks: super::leviathan::Masks,
}

impl Flyer {
    /// The own generator's draw (`+0x18`).
    fn draw(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        self.rng >> 16
    }
}

/// A nest's count of creatures still to come and when it may send the
/// next.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Nest {
    pub id: u32,
    pub left: u32,
    pub delay: f32,
    /// Seconds until the next creature, while none is out.
    pub wait: Option<f32>,
    /// The last creature it sent (`+0x1e4`), and the tick of its last
    /// blow while a chase is on (`+0x1e8`, `+0x1ec`).
    #[serde(default)]
    pub creature: u32,
    #[serde(default)]
    pub hit_at: Option<u32>,
}

/// A cell's centre, world pixels.
fn centre((x, y): (i32, i32)) -> (i32, i32) {
    (x * 64 + 32, y * 64 + 32)
}

/// The angular distance of two headings (0x55c3d0).
fn turn_distance(a: u32, b: u32) -> i32 {
    let d = (a as i32 - b as i32).abs();
    if d > 4 {
        8 - d
    } else {
        d
    }
}

/// A flyer's route (0x55c3d0) on a `w` x `h` map from `start`, heading
/// `h0`, to `end`: entries `(x, y, heading entered with)`, the first the
/// start. Up to three passes from the start: each turns the heading by
/// one (`turn`, its sense set at the first bend: towards the main
/// direction `D`) a cell until the heading is within one of `D` or its
/// side direction `A`, then runs straight along the closer and then the
/// other; a pass that would leave the map is charged the overshoot and
/// tried the other way round; the third keeps to the map, taking the
/// closer way, and is kept.
pub(super) fn flyer_route(
    w: i32,
    h: i32,
    start: (i32, i32),
    h0: u8,
    end: (i32, i32),
) -> Vec<(i32, i32, u8)> {
    let mut turn: i32 = 0;
    let mut p0 = 0;
    let mut out = Vec::new();
    let off = |v: i32, n: i32| {
        if v < 0 {
            -v
        } else if v >= n {
            1 + v - n
        } else {
            0
        }
    };
    for pass in 0..3 {
        out.clear();
        let mut p = 0;
        let (mut x, mut y, mut hd) = (start.0, start.1, h0 as u32);
        let (mut ax, mut ay);
        let (mut main, mut side) = (0u32, 0u32);
        loop {
            ax = (end.0 - x).abs();
            ay = (end.1 - y).abs();
            if ax == 0 && ay == 0 {
                break;
            }
            out.push((x, y, hd as u8));
            let (mut d, mut a): (u32, u32) = if end.1 < y {
                if end.0 < x {
                    if ax < ay * 2 {
                        if ay < ax * 2 {
                            (3, if ax < ay { 2 } else { 4 })
                        } else {
                            (2, 3)
                        }
                    } else {
                        (4, 3)
                    }
                } else if ax < ay * 2 {
                    if ay < ax * 2 {
                        (1, if ax < ay { 2 } else { 0 })
                    } else {
                        (2, 1)
                    }
                } else {
                    (0, 1)
                }
            } else if end.0 < x {
                if ax < ay * 2 {
                    if ay < ax * 2 {
                        (5, if ax < ay { 6 } else { 4 })
                    } else {
                        (6, 5)
                    }
                } else {
                    (4, 5)
                }
            } else if ax < ay * 2 {
                if ay < ax * 2 {
                    (7, if ax < ay { 6 } else { 0 })
                } else {
                    (6, 7)
                }
            } else {
                (0, 7)
            };
            if ax == 0 || ay == 0 || ax == ay {
                a = d;
            }
            // `D` the closer of the two to the heading.
            let (mut near, mut far) = (turn_distance(d, hd), turn_distance(a, hd));
            if far < near {
                std::mem::swap(&mut d, &mut a);
                std::mem::swap(&mut near, &mut far);
            }
            (main, side) = (d, a);
            if near < 2 {
                break;
            }
            if turn == 0 {
                let k = d as i32 - hd as i32;
                turn = if (-3..=-1).contains(&k) || (5..=7).contains(&k) {
                    -1
                } else {
                    1
                };
                if near < 4 && ((ax < 3 && ay == 0) || (ay < 3 && ax == 0) || (ax == 1 && ay == 1))
                {
                    hd = (d + 4) & 7;
                }
                let two_one = (ax == 2 && ay == 1) || (ay == 2 && ax == 1);
                if near == 2 && far == 3 && two_one {
                    turn = 0;
                }
            }
            let step = |hd: u32| (x + DIRS[hd as usize].0, y + DIRS[hd as usize].1);
            if pass < 2 {
                hd = (hd as i32 + 8 + turn) as u32 & 7;
                (x, y) = step(hd);
                p += off(x, w) + off(y, h);
            } else if (ax < 2 && ay < 2) || (ax == 2 && ay == 1) || (ay == 2 && ax == 1) {
                hd = d;
                (x, y) = step(hd);
            } else {
                let mut n = step(hd);
                for _ in 0..8 {
                    hd = (hd as i32 + 8 + turn) as u32 & 7;
                    n = step(hd);
                    if n.0 >= 0 && n.0 < w && n.1 >= 0 && n.1 < h {
                        break;
                    }
                }
                (x, y) = n;
            }
        }
        if pass > 1 || p == 0 {
            if ax == 0 && ay == 0 {
                out.push((x, y, hd as u8));
            } else {
                let (dx, dy) = DIRS[main as usize];
                loop {
                    (x, y) = (x + dx, y + dy);
                    out.push((x, y, main as u8));
                    ax -= dx.abs();
                    ay -= dy.abs();
                    let done = if main & 1 == 0 {
                        ax == ay
                    } else {
                        ax == 0 || ay == 0
                    };
                    if done {
                        break;
                    }
                }
                let (dx, dy) = DIRS[side as usize];
                while ax != 0 || ay != 0 {
                    (x, y) = (x + dx, y + dy);
                    out.push((x, y, side as u8));
                    ax -= dx.abs();
                    ay -= dy.abs();
                }
            }
            if p == 0 {
                break;
            }
        }
        if pass == 0 {
            p0 = p;
            turn = -turn;
        } else if pass == 1 && p0 <= p {
            turn = -turn;
        }
    }
    out
}

impl Scene {
    /// The nests' first creatures (message 0x8d1).
    pub(super) fn start_nests(&mut self, data: &GameData, caches: &mut Caches) {
        let nests: Vec<(u32, u32, u32)> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::HarpyNest)
            .map(|o| {
                let f = |i| o.record.field(i).unwrap_or(0);
                (o.id, f(9), f(10))
            })
            .collect();
        for (id, left, speed) in nests {
            let delay = crate::clock::ticks(55_500u32.saturating_sub(6000 * speed));
            self.nests.push(Nest {
                id,
                left,
                delay,
                wait: Some(0.0),
                creature: 0,
                hit_at: None,
            });
        }
        self.run_nests(0.0, data, caches);
    }

    fn run_nests(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        for k in 0..self.nests.len() {
            let id = self.nests[k].id;
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let out = self.flyers.iter().any(|f| f.nest == id);
            let n = &mut self.nests[k];
            match n.wait.as_mut() {
                None if !out && n.left > 0 => {
                    n.left -= 1;
                    n.wait = Some(n.delay);
                }
                Some(w) => {
                    *w -= dt;
                    if *w <= 0.0 {
                        n.wait = None;
                        let (x, y) = (self.objects[i].x, self.objects[i].y);
                        let class = if self.objects[i].record.class_id == 0x44 {
                            0x1f9
                        } else {
                            0x1f8
                        };
                        if let Some(c) = self.spawn_flyer(class, id, x, y, data, caches) {
                            self.nests[k].creature = c;
                        }
                    }
                }
                None => {}
            }
        }
    }

    /// A nest's creature (message 2, 0x4207b0, from the nest's 0x41d4a0):
    /// on the nest's cell, its generator seeded `[game +0x2ee5] x id`, the
    /// nest's range (fields 7, 8) and rest parameter (field 11), home the
    /// nest's cell, hp 200000 (harpy) or 250000 (dragon, the record's
    /// combat), state 0.
    fn spawn_flyer(
        &mut self,
        class: u32,
        nest: u32,
        x: f32,
        y: f32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<u32> {
        let t = TILE_SIZE as f32;
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let cell = ((x / t).floor() as i32, ((y - t / 2.0) / t).floor() as i32);
        let mut fields = vec![0u32; 11];
        fields[0] = class;
        fields[4] = cell.0 as u32;
        fields[5] = cell.1 as u32;
        fields[7] = 1;
        let rec = ObjectRecord {
            id,
            class_id: class,
            fields,
        };
        let Ok(Some(mut obj)) = objects::resolve(data, caches, &rec) else {
            return None;
        };
        let (cx, cy) = centre(cell);
        obj.x = cx as f32;
        obj.y = cy as f32;
        self.objects.push(obj);
        let n = self.object_index(nest).map(|n| &self.objects[n]);
        let f = |k| n.and_then(|n| n.record.field(k)).unwrap_or(0);
        let (range, rest) = ((f(7) as i32, f(8) as i32), f(11));
        let seed = self.objects_seed().wrapping_mul(id);
        self.flyers.push(Flyer {
            id,
            nest,
            has_nest: true,
            rng: seed,
            state: 0xff,
            cell,
            home: cell,
            range,
            rest,
            ..Flyer::default()
        });
        let k = self.flyers.len() - 1;
        self.set_flyer_state(k, APPEAR, data, caches);
        Some(id)
    }

    /// The nests' sending, and the creatures that fell or left leaving
    /// (their objects removed at their dying picture's end, message 10).
    pub(super) fn run_flyers(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.run_nests(dt, data, caches);
        let (kept, fallen): (Vec<Flyer>, Vec<Flyer>) = std::mem::take(&mut self.flyers)
            .into_iter()
            .partition(|f| !f.gone && self.object_index(f.id).is_some());
        self.flyers = kept;
        for f in fallen {
            self.objects.retain(|o| o.id != f.id);
            if let Some(n) = self.nests.iter_mut().find(|n| n.id == f.nest) {
                if n.left > 0 && n.wait.is_none() {
                    n.left -= 1;
                    n.wait = Some(n.delay);
                }
            }
        }
    }

    /// Each creature's tick (message 0): its states (0x41fbd0), then its
    /// picture and place (0x41e4b0).
    pub(super) fn tick_flyers(&mut self, data: &GameData, caches: &mut Caches) {
        self.tick_nests();
        for k in 0..self.flyers.len() {
            if self.flyers[k].gone || self.object_index(self.flyers[k].id).is_none() {
                continue;
            }
            self.flyer_states(k, data, caches);
            self.flyer_frame(k);
        }
    }

    /// A flyer's hp (`+0x89`), in the original's points: the port keeps
    /// it as its object's hull, a fifteenth of them.
    fn flyer_hp(&self, k: usize) -> u32 {
        self.object_index(self.flyers[k].id)
            .and_then(|i| self.objects[i].combat.as_ref())
            .map_or(0, |c| (c.hull.max(0.0) * 15.0).round() as u32)
    }

    fn flyer_class(&self, k: usize) -> u32 {
        self.object_index(self.flyers[k].id)
            .map_or(HARPY, |i| self.objects[i].record.class_id)
    }

    /// 0x41fbd0: the states' tick.
    fn flyer_states(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let class = self.flyer_class(k);
        // The nest's fall (message 0xdaf9 from 0x41d300): no chase, no
        // nest, not homing, its hp falling to 0 over 1000 ticks.
        let nest_gone = self.object_index(self.flyers[k].nest).is_none_or(|n| {
            self.objects[n]
                .combat
                .as_ref()
                .is_some_and(|c| c.hull <= 0.0)
        });
        if self.flyers[k].has_nest && nest_gone {
            let hp = self.flyer_hp(k);
            let f = &mut self.flyers[k];
            f.forced = None;
            f.has_nest = false;
            f.homing = false;
            f.orphan_at = now;
            f.orphan_hp = hp;
        }
        let f = &self.flyers[k];
        if !matches!(f.state, REST | TAKE_OFF | LAND | DYING)
            && f.frame == 0
            && self.flyer_hp(k) == 0
        {
            self.set_flyer_state(k, DYING, data, caches);
        }
        let f = &self.flyers[k];
        if !f.has_nest && self.flyer_hp(k) != 0 && f.orphan_hp != 0 {
            let ba = f.orphan_hp;
            let hp = ba.wrapping_sub(now.wrapping_sub(f.orphan_at).wrapping_mul(ba) / 1000);
            if let Some(c) = self
                .object_index(f.id)
                .and_then(|i| self.objects[i].combat.as_mut())
            {
                c.hull = hp as f32 / 15.0;
            }
        }
        match self.flyers[k].state {
            APPEAR => {
                if self.flyers[k].sub != 0 || class == DRAGON {
                    self.set_flyer_state(k, REST, data, caches);
                }
            }
            REST => {
                let hp = self.flyer_hp(k);
                let f = &self.flyers[k];
                let resting =
                    now < f.rest_end && f.forced.is_none() && f.has_nest && hp != 0 && !f.hit;
                if f.sub == 0 {
                    if resting {
                        if f.idle_at <= now && class == HARPY {
                            let f = &mut self.flyers[k];
                            f.sub = 1;
                            f.e4 = 0;
                        }
                        return;
                    }
                    let next = if class != DRAGON || self.someone_in_range(k) {
                        TAKE_OFF
                    } else {
                        REST
                    };
                    self.set_flyer_state(k, next, data, caches);
                    return;
                }
                let f = &mut self.flyers[k];
                if f.e4 != 0 {
                    f.sub = 0;
                    f.idle_at = f.draw() % 50 + now;
                }
            }
            TAKE_OFF => {
                if self.flyers[k].sub != 0 {
                    let next = if self.flyer_hp(k) == 0 { DYING } else { FLY };
                    self.set_flyer_state(k, next, data, caches);
                }
            }
            FLY if self.flyers[k].sub == MOVING => self.flyer_moving(k, class, data, caches),
            FLY => {
                if !self.flyers[k].stepped {
                    return;
                }
                self.set_flyer_state(k, FLY, data, caches);
                let f = &self.flyers[k];
                if f.state == FLY && f.sub == HOLDING && f.homing && self.bound_here(k) {
                    self.flyers[k].homing = false;
                    self.set_flyer_state(k, FLY, data, caches);
                }
            }
            TURN => {
                let f = &mut self.flyers[k];
                if !f.turned {
                    return;
                }
                f.state = f.back;
                f.heading = f.turn_to;
                match f.state {
                    FLY => {
                        let (x, y, _) = f.route[f.index];
                        let free = self.step_free(k, (x, y));
                        let f = &mut self.flyers[k];
                        f.sub = if free { MOVING } else { HOLDING };
                        f.stepped = false;
                    }
                    STRIKE => f.struck = false,
                    LAND => f.sub = 0,
                    _ => {}
                }
                self.flyer_picture(k, data, caches);
            }
            STRIKE => {
                let f = &self.flyers[k];
                let blow = if class == DRAGON {
                    DRAGON_BLOW_FRAME
                } else {
                    HARPY_BLOW_FRAME
                };
                if f.frame == blow && !f.fired {
                    if class == HARPY {
                        let prey = f.sub;
                        if self.object_index(prey).is_some() {
                            self.flyer_strikes(k, class, prey, (0, 0), data, caches);
                            self.flyers[k].fired = true;
                        }
                    } else {
                        if self.strike_pick(k, class) {
                            let f = &self.flyers[k];
                            let d = DIRS[f.e4 as usize & 7];
                            let at = (f.cell.0 + d.0, f.cell.1 + d.1);
                            self.flyer_strikes(k, class, 0, at, data, caches);
                        }
                        self.flyers[k].fired = true;
                    }
                }
                if self.flyers[k].struck {
                    self.flyers[k].homing = true;
                    self.set_flyer_state(k, FLY, data, caches);
                }
            }
            LAND if self.flyers[k].sub != 0 => self.set_flyer_state(k, REST, data, caches),
            _ => {}
        }
    }

    /// 0x41fbd0, state 3 moving: the wing flap's time, the cell's flyer
    /// slot following its place, and at a step's end the chase, a look
    /// about, or the route's next cell (entered if free, else held), and
    /// at the route's end another leg, or the landing at home.
    fn flyer_moving(&mut self, k: usize, class: u32, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        if self.flyers[k].flap <= now {
            // The flap, event 0x88 harpy (`harpy2`) / 0x89 dragon
            // (`disap1`), mode 5 at its cell.
            let flap = if class == DRAGON { "disap1" } else { "harpy2" };
            let (x, y) = centre(self.flyers[k].cell);
            self.sounds.push((flap, x as f32, y as f32));
        }
        let f = &mut self.flyers[k];
        if f.flap <= now {
            f.flap = (f.draw() % 41 + 60) * 375 / 100 + now;
        }
        let at = (f.pos.0.div_euclid(64), f.pos.1.div_euclid(64));
        if at != f.cell {
            f.cell = at;
        }
        if !f.stepped {
            return;
        }
        if f.forced.is_some() && !f.forced_done {
            f.forced_done = true;
            f.homing = true;
            self.set_flyer_state(k, FLY, data, caches);
            return;
        }
        if self.flyer_look(k, class) && !self.flyers[k].homing {
            self.set_flyer_state(k, STRIKE, data, caches);
            return;
        }
        let f = &mut self.flyers[k];
        f.index += 1;
        if f.index < f.route.len() {
            let (x, y, h) = f.route[f.index];
            f.from = centre(f.cell);
            f.to = centre((x, y));
            if h == f.heading {
                if self.step_free(k, (x, y)) {
                    self.flyers[k].sub = MOVING;
                    self.flyer_picture(k, data, caches);
                    self.flyers[k].stepped = false;
                    return;
                }
                self.flyers[k].sub = HOLDING;
                if self.bound_here(k) {
                    self.set_flyer_state(k, FLY, data, caches);
                }
            } else {
                self.set_flyer_state(k, TURN, data, caches);
            }
            self.flyer_picture(k, data, caches);
            self.flyers[k].stepped = false;
            return;
        }
        let f = &mut self.flyers[k];
        if !(f.homing && f.cell == f.home && f.has_nest) {
            self.set_flyer_state(k, FLY, data, caches);
        } else if f.forced.is_none() {
            self.set_flyer_state(k, LAND, data, caches);
        } else {
            f.homing = false;
            self.set_flyer_state(k, FLY, data, caches);
        }
    }

    /// 0x41f680: flyer `k` enters state `s`.
    fn set_flyer_state(&mut self, k: usize, s: u8, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        let f = &mut self.flyers[k];
        match s {
            APPEAR => {
                f.pos = centre(f.cell);
                let mut hd;
                loop {
                    hd = (f.draw() & 3) * 2 + 1;
                    let (x, y) = (
                        f.cell.0 + DIRS[hd as usize].0,
                        f.cell.1 + DIRS[hd as usize].1,
                    );
                    if x >= 0 && x < w && y >= 0 && y < h {
                        break;
                    }
                }
                f.heading = hd as u8;
                f.state = APPEAR;
                f.sub = 0;
                self.flyer_picture(k, data, caches);
                // A harpy's, event 0x5c (`app3`), mode 5 at its cell.
                if self.flyer_class(k) == HARPY {
                    let (x, y) = centre(self.flyers[k].cell);
                    self.sounds.push(("app3", x as f32, y as f32));
                }
            }
            REST => {
                f.state = REST;
                f.hit = false;
                f.sub = 0;
                f.idle_at = now + f.draw() % 50;
                let most = 1100u32.wrapping_sub(100u32.wrapping_mul(f.rest));
                f.rest_end = now.wrapping_add(f.draw().checked_rem(most).unwrap_or(0));
                self.flyer_picture(k, data, caches);
            }
            TAKE_OFF => {
                f.state = TAKE_OFF;
                f.sub = 0;
                // A dragon's, event 0x5d (`app3`), mode 5 at its cell.
                if self.flyer_class(k) == DRAGON {
                    let (x, y) = centre(self.flyers[k].cell);
                    self.sounds.push(("app3", x as f32, y as f32));
                }
                self.flyer_picture(k, data, caches);
            }
            FLY => {
                f.state = FLY;
                let built = self.flyer_leg(k).map(|leg| {
                    let f = &self.flyers[k];
                    flyer_route(w, h, f.cell, f.heading, leg)
                });
                let f = &mut self.flyers[k];
                let mut turned = false;
                match built {
                    Some(route) if route.len() > 1 => {
                        f.route = route;
                        f.index = 1;
                        f.from = centre(f.cell);
                        let (x, y, hd) = f.route[1];
                        f.to = centre((x, y));
                        if hd != f.heading {
                            self.set_flyer_state(k, TURN, data, caches);
                            turned = true;
                        } else {
                            let free = self.step_free(k, (x, y));
                            self.flyers[k].sub = if free { MOVING } else { HOLDING };
                        }
                    }
                    _ => f.sub = HOLDING,
                }
                if !turned {
                    self.flyer_picture(k, data, caches);
                }
                let f = &mut self.flyers[k];
                f.stepped = false;
                f.flap = (f.draw() % 41 + 60) * 375 / 100 + now;
            }
            TURN => {
                f.back = f.state;
                f.state = TURN;
                match f.back {
                    FLY => f.turn_to = f.route[f.index].2,
                    STRIKE => f.turn_to = f.e4 as u8,
                    LAND => f.turn_to = f.heading + 1,
                    _ => {}
                }
                self.flyer_picture(k, data, caches);
                self.flyers[k].turned = false;
            }
            STRIKE => {
                f.state = STRIKE;
                f.fired = false;
                let class = self.flyer_class(k);
                if !self.strike_pick(k, class) {
                    self.set_flyer_state(k, FLY, data, caches);
                } else if self.flyers[k].e4 as u8 == self.flyers[k].heading {
                    self.flyers[k].struck = false;
                    self.flyer_picture(k, data, caches);
                } else {
                    self.set_flyer_state(k, TURN, data, caches);
                }
            }
            LAND => {
                f.state = LAND;
                f.homing = false;
                f.sub = 0;
                if f.heading & 1 == 0 {
                    self.set_flyer_state(k, TURN, data, caches);
                } else {
                    self.flyer_picture(k, data, caches);
                }
            }
            _ => {
                if f.state == DYING {
                    return;
                }
                // The cry, event 0x67 harpy / 0x68 dragon, at its cell.
                let (x, y) = centre(f.cell);
                let cry = if self.flyer_class(k) == DRAGON {
                    "dedr2"
                } else {
                    "deadga1"
                };
                self.sounds.push((cry, x as f32, y as f32));
                let f = &mut self.flyers[k];
                f.state = DYING;
                self.flyer_picture(k, data, caches);
            }
        }
    }

    /// 0x41e130's pictures for flyer `k`'s state and heading, into its
    /// object, with its masks (`..M`, `+0x7c`); the count of frames, or
    /// `None` for a dragon resting (both pictures hidden, 0x50d470).
    fn flyer_art(&mut self, k: usize, data: &GameData, caches: &mut Caches) -> Option<i32> {
        let i = self.object_index(self.flyers[k].id)?;
        let class = self.objects[i].record.class_id;
        let f = &self.flyers[k];
        if class == DRAGON && f.state <= REST {
            self.objects[i].frames = Vec::new();
            return None;
        }
        let prefix = if class == DRAGON { "DRGSPR" } else { "HRPSPR" };
        let name = if f.state == TURN {
            format!("{prefix}_4")
        } else {
            format!("{prefix}_{}{}", f.state, f.heading)
        };
        let frames = objects::flyer_frames(data, caches, &name).unwrap_or_default();
        let count = frames.len() as i32;
        self.objects[i].frames = frames;
        self.flyers[k].masks = super::leviathan::load_masks(data, &format!("{name}M"));
        Some(count)
    }

    /// The flyers' pictures and masks made again after a restore (neither
    /// is saved), at their place.
    pub(super) fn flyers_restored(&mut self, data: &GameData, caches: &mut Caches) {
        for k in 0..self.flyers.len() {
            self.flyer_art(k, data, caches);
            let f = &self.flyers[k];
            if let Some(i) = self.object_index(f.id) {
                self.objects[i].x = f.pos.0 as f32;
                self.objects[i].y = f.pos.1 as f32;
            }
        }
    }

    /// 0x41e130: the picture for the state and heading,
    /// `HRPSPR_<state><heading>` / `DRGSPR_..` (turning `..SPR_4`, eight
    /// frames a heading, from the heading's to the one turned to, the
    /// short way through 0); a dragon resting is not drawn.
    fn flyer_picture(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let Some(count) = self.flyer_art(k, data, caches) else {
            return;
        };
        let Some(i) = self.object_index(self.flyers[k].id) else {
            return;
        };
        let class = self.objects[i].record.class_id;
        let f = &mut self.flyers[k];
        let delays = if class == DRAGON {
            &DRAGON_DELAYS
        } else {
            &HARPY_DELAYS
        };
        f.count = count;
        f.delay = if f.state == FLY && f.sub == HOLDING {
            delays[4]
        } else {
            delays[f.state as usize & 7]
        };
        f.first = 0;
        f.end = count;
        if f.state == TURN {
            f.first = f.heading as i32 * 8;
            f.end = f.turn_to as i32 * 8;
            if f.first == 0 && count - f.end < f.end {
                f.first = count - 1;
            }
            if f.end == 0 && count - f.first < f.first {
                f.end = count - 1;
            }
        }
        f.frame = f.first;
        f.drawn = false;
    }

    /// 0x41e4b0: once its frame's ticks have passed (or not yet drawn),
    /// moving, its place goes from the step's start to its end with the
    /// frame (`(to - from) x frame / (end - first - 1) + from`, the end
    /// frame at the end); it is drawn there; the frame steps towards the
    /// end (resting, not idle, it holds frame 0), and at the end the
    /// state's picture is done (taking off holds its last frame; dying,
    /// it is removed).
    fn flyer_frame(&mut self, k: usize) {
        let now = self.ticks;
        let Some(i) = self.object_index(self.flyers[k].id) else {
            return;
        };
        let f = &mut self.flyers[k];
        if now < f.last.wrapping_add(f.delay) && f.drawn {
            return;
        }
        if f.state == FLY && f.sub == MOVING {
            let (frame, first, end) = (f.frame, f.first, f.end);
            let along = |a: i32, b: i32| {
                if frame == end {
                    b
                } else {
                    (b - a) * frame / (end - first - 1) + a
                }
            };
            f.pos = (along(f.from.0, f.to.0), along(f.from.1, f.to.1));
        }
        self.objects[i].x = f.pos.0 as f32;
        self.objects[i].y = f.pos.1 as f32;
        if f.state == REST && f.sub == 0 {
            f.frame = 0;
        } else {
            f.frame += if f.first < f.end { 1 } else { -1 };
        }
        if (f.first < f.end && f.end <= f.frame) || (f.end < f.first && f.frame <= f.end) {
            match f.state {
                APPEAR | LAND => {
                    f.sub = 1;
                    f.frame = 0;
                }
                REST => {
                    f.e4 = 1;
                    f.frame = 0;
                }
                TAKE_OFF => {
                    f.sub = 1;
                    f.frame -= 1;
                }
                FLY => {
                    f.stepped = true;
                    f.frame = 0;
                }
                TURN => {
                    f.turned = true;
                    f.frame = 0;
                }
                STRIKE => {
                    f.struck = true;
                    f.frame = 0;
                }
                DYING => f.gone = true,
                _ => {}
            }
        }
        f.last = now;
        f.drawn = true;
        let (frame, count) = (f.frame, f.count.max(1));
        self.objects[i].progress = Some((frame.clamp(0, count - 1) as f32 + 0.5) / count as f32);
    }

    /// 0x406830: cell `c`'s flyer slot (`+0x14`) is empty or holds flyer
    /// `k` itself (a fireball or a tornado holds it too).
    fn flyer_slot_free(&self, k: usize, c: (i32, i32)) -> bool {
        let me = self.flyers[k].id;
        let t = TILE_SIZE as f32;
        !self
            .flyers
            .iter()
            .any(|g| g.id != me && !g.gone && g.cell == c)
            && !self.fireballs.iter().any(|f| f.cell == c)
            && !self.effects.iter().any(|e| {
                e.tornado.is_some() && ((e.x / t).floor() as i32, (e.y / t).floor() as i32) == c
            })
    }

    /// 0x41ee20: cell `c` may be entered: its flyer slot free, and no
    /// other flyer in the 3 x 3 about it flying a step into it.
    fn step_free(&self, k: usize, c: (i32, i32)) -> bool {
        let me = self.flyers[k].id;
        self.flyer_slot_free(k, c)
            && !self.flyers.iter().any(|g| {
                g.id != me
                    && !g.gone
                    && g.state == FLY
                    && (g.cell.0 - c.0).abs() <= 1
                    && (g.cell.1 - c.1).abs() <= 1
                    && g.route.get(g.index).is_some_and(|e| (e.0, e.1) == c)
            })
    }

    /// 0x41fbd0: the flyer in the slot of the cell flyer `k` flies to is
    /// flying and bound for `k`'s own cell (the two would swap).
    fn bound_here(&self, k: usize) -> bool {
        let f = &self.flyers[k];
        let Some(&(x, y, _)) = f.route.get(f.index) else {
            return false;
        };
        self.flyers.iter().any(|g| {
            g.id != f.id
                && !g.gone
                && g.cell == (x, y)
                && g.state == FLY
                && g.route.get(g.index).is_some_and(|e| (e.0, e.1) == f.cell)
        })
    }

    /// 0x41e840: the cell a leg flies to. Homing, off both the nest's row
    /// and column: the nest (weight 100). Else each cell of the mask
    /// about it on the map, within the nest's range and with its flyer
    /// slot free: weight 1, 0xff beside the attacker it chases, else more
    /// by 10 x the marines (`harpy_marines`) of each side-1 ship beside it
    /// (in its cell, not under way into it), settlement, town or fort.
    /// One draw, `r % total + 1`, the first cell whose running sum reaches
    /// it.
    fn flyer_leg(&mut self, k: usize) -> Option<(i32, i32)> {
        let f = &self.flyers[k];
        let (cell, home, range, forced) = (f.cell, f.home, f.range, f.forced);
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        let mut cells: Vec<((i32, i32), u32)> = Vec::new();
        if !f.has_nest || !f.homing || f.cell.0 == f.home.0 || f.cell.1 == f.home.1 {
            let slots = self.slot_map();
            for dy in -6..=6 {
                for dx in -6..=6 {
                    let c = (cell.0 + dx, cell.1 + dy);
                    if c.0 < 0 || c.0 >= w || c.1 < 0 || c.1 >= h {
                        continue;
                    }
                    if (c.0 - home.0).abs() > range.0
                        || (c.1 - home.1).abs() > range.1
                        || LEG_MASK[(dy + 6) as usize][(dx + 6) as usize] != 1
                        || !self.flyer_slot_free(k, c)
                    {
                        continue;
                    }
                    let mut weight = 1u32;
                    for n in 0..9 {
                        if n == 4 {
                            continue;
                        }
                        let b = (c.0 + n % 3 - 1, c.1 + n / 3 - 1);
                        if b.0 < 0 || b.0 >= w || b.1 < 0 || b.1 >= h {
                            continue;
                        }
                        let Some(Held::Object(j)) = slots.get(&b).and_then(|s| s.occupant) else {
                            continue;
                        };
                        let o = &self.objects[j];
                        if Self::side_of(o) != 1 {
                            continue;
                        }
                        if forced == Some(o.id) {
                            weight = 0xff;
                            continue;
                        }
                        if o.ship.is_some() && self.reserved_cell(j) == Some(b) {
                            continue;
                        }
                        weight += self.harpy_marines(j) * 10;
                    }
                    cells.push((c, weight));
                }
            }
        } else {
            cells.push((home, 100));
        }
        if cells.is_empty() {
            return None;
        }
        let total: u32 = cells.iter().map(|c| c.1).sum();
        let t = self.flyers[k].draw() % total + 1;
        let mut sum = 0;
        for &(c, w) in &cells {
            sum += w;
            if sum >= t {
                return Some(c);
            }
        }
        cells.last().map(|c| c.0)
    }

    /// The marines in object `j`'s list, as 0x41e840 counts them: each
    /// entry of category 1 (a unit) with item 0-2 counts 1 + its count
    /// (`+8`). A ship lists each marine squad aboard alone, with no count
    /// (0x4e3960 through 0x4e5e40). A settlement or town (class 0x1491)
    /// lists its stock (vt+0x5c 0x43bfe0 page 0: its nine items' places
    /// with their counts, so each of items 0-2 counts one more), none in
    /// states `+0xa4` 2 (fallen), 3 (sold) or 0xff; on its way up as an
    /// upgrade (`+0xa4` 0 with `+0x23c`), 0x43bfe0 first makes its life
    /// `+0xa8` whole. A fort (0x1494, vt+0x5c 0x4179b0) lists each squad of
    /// its places alone, with no count, only while it stands (`+0xa4` 1);
    /// being mended (`+0x10c` with job 0xff, still state 1) only changes
    /// its entries' flags, so they count. Anything else lists nothing.
    /// (Not identified: the town state 0xff.)
    pub(super) fn harpy_marines(&mut self, j: usize) -> u32 {
        use crate::economy::{SETTLEMENT, TOWN};
        let id = self.objects[j].id;
        let list = match self.objects[j].class {
            _ if self.objects[j].ship.is_some() => self.ship_list(j),
            ObjectClass::Building(SETTLEMENT | TOWN) => {
                if self.fallen(id) || self.selling.contains_key(&id) || self.sold.contains(&id) {
                    return 0;
                }
                if self.construction_at(id).is_some_and(|c| c.upgrade) {
                    if let Some(c) = self.objects[j].combat.as_mut() {
                        c.hull = c.max_hull;
                    }
                }
                self.load_list(id, None)
            }
            ObjectClass::Building(0x34..=0x36) => {
                if !self.standing(id) || self.fallen(id) {
                    return 0;
                }
                self.fort_list(j)
            }
            _ => return 0,
        };
        list.iter()
            .filter(|e| e.cat == super::load::UNIT && e.sub < 3)
            .map(|e| 1 + e.count)
            .sum()
    }

    /// Whether object `j` is a settlement, town (class 0x1491) or fort
    /// (0x1494).
    fn town_or_fort(&self, j: usize) -> bool {
        matches!(self.objects[j].class, ObjectClass::Building(0x32..=0x36))
    }

    /// Whether ship `j` is one a harpy goes for (0x41efb0, 0x41f1c0): of a
    /// nation other than 4, without Orpheus aboard.
    fn harpy_prey(&self, j: usize) -> bool {
        let o = &self.objects[j];
        o.owner
            .is_some_and(|w| w.0 != 4 && w != crate::combat::MONSTERS)
            && o.ship.as_ref().is_some_and(|s| !s.has_hero(ORPHEUS))
    }

    /// 0x42ebf0's answer for cell `c`: 0xf land creatures may be put down
    /// on (`landing_ground`), 0x2c0 a cell whose landscape nibble is not 8
    /// (water), else 0.
    fn sow_ground(&self, c: (i32, i32)) -> u32 {
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        if c.0 < 0 || c.1 < 0 || c.0 >= w || c.1 >= h {
            return 0;
        }
        if self.cell_types[(c.1 * w + c.0) as usize] & 0xf != 8 {
            0x2c0
        } else if self.landing_ground(c) {
            0xf
        } else {
            0
        }
    }

    /// 0x41efb0, at each cell of a leg: the 3 x 3 about it (its own cell
    /// too), row by row. A harpy stops at a ship it goes for, at any
    /// settlement or town (0x1491) and at a fort (0x1494) without the
    /// phantom (vt+0xa4, `+0x134`), whose lists it fills unused
    /// (`harpy_marines`, for what that does to an upgrading town); a dragon
    /// at the attacker it chases, else at the first empty cell it could
    /// sow on: none if no object came before it, else one time in three (a
    /// draw). (Not decoded: the objects' vt+0x2c, taken as 1.)
    pub(super) fn flyer_look(&mut self, k: usize, class: u32) -> bool {
        let slots = self.slot_map();
        let (cell, forced) = (self.flyers[k].cell, self.flyers[k].forced);
        let mut seen = false;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = (cell.0 + dx, cell.1 + dy);
                match slots.get(&c).and_then(|s| s.occupant) {
                    None | Some(Held::Other) => {
                        if class == DRAGON && self.sow_ground(c) != 0 {
                            if !seen {
                                return false;
                            }
                            return self.flyers[k].draw() % 3 == 0;
                        }
                    }
                    Some(Held::Object(j)) | Some(Held::Jam(j, _)) => {
                        seen = true;
                        if forced == Some(self.objects[j].id) {
                            return true;
                        }
                        if class != HARPY {
                            continue;
                        }
                        if self.harpy_prey(j) {
                            return true;
                        }
                        let o = &self.objects[j];
                        let phantom = o.combat.as_ref().is_some_and(|c| c.phantom > 0.0);
                        if matches!(o.class, ObjectClass::Building(0x32 | 0x33))
                            || matches!(o.class, ObjectClass::Building(0x34..=0x36)) && !phantom
                        {
                            self.harpy_marines(j);
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// 0x41f1c0, on striking: the eight cells about it, row by row. A
    /// harpy takes the last ship it goes for (on its own cell, not under
    /// way into it, not a phantom), the attacker it chases before all,
    /// into `sub`, its heading into `e4`. A dragon picks an empty cell it
    /// could sow on: weight 0x32 on land (landscape nibble 8) after a
    /// building (classes 0x32-0x3e) was met in the scan, else 0; one draw,
    /// by count if all weigh 0. A harpy passes over the buildings
    /// 0x32-0x3e with the phantom (vt+0xa4, `+0x134`), and takes a
    /// settlement, town or fort as it takes a ship, filling its list
    /// unused (`harpy_marines`). (Not decoded: the objects' vt+0x2c, taken
    /// as 1.)
    pub(super) fn strike_pick(&mut self, k: usize, class: u32) -> bool {
        let slots = self.slot_map();
        let w = self.grid.width as i32;
        let f = &self.flyers[k];
        let mut building = false;
        let mut found = false;
        let mut best = 0;
        let (mut target, mut heading) = (f.sub, f.e4);
        let mut sow: Vec<(u32, u32)> = Vec::new();
        let mut town_lists = Vec::new();
        for d in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let c = (f.cell.0 + d.0, f.cell.1 + d.1);
            let dir = DIRS.iter().position(|&e| e == d).unwrap_or(0) as u32;
            match slots.get(&c).and_then(|s| s.occupant) {
                None | Some(Held::Other) => {
                    if class == DRAGON && self.sow_ground(c) != 0 {
                        let land = self.cell_types[(c.1 * w + c.0) as usize] & 0xf == 8;
                        sow.push((dir, if building && land { 0x32 } else { 0 }));
                    }
                }
                Some(Held::Jam(..)) => {}
                Some(Held::Object(j)) => {
                    let o = &self.objects[j];
                    if (0x32..=0x3e).contains(&o.record.class_id) {
                        building = true;
                    }
                    if class != HARPY {
                        continue;
                    }
                    let phantom = o.combat.as_ref().is_some_and(|c| c.phantom > 0.0);
                    let structure = (0x32..=0x3e).contains(&o.record.class_id);
                    if (o.ship.is_some() || structure) && phantom {
                        continue;
                    }
                    let score = if f.forced == Some(o.id) {
                        0xff
                    } else if self.harpy_prey(j) && self.reserved_cell(j) != Some(c) {
                        0
                    } else if self.town_or_fort(j) {
                        town_lists.push(j);
                        0
                    } else {
                        continue;
                    };
                    if best <= score {
                        (target, heading, best, found) = (o.id, dir, score, true);
                    }
                }
            }
        }
        for j in town_lists {
            self.harpy_marines(j);
        }
        if class == HARPY {
            let f = &mut self.flyers[k];
            (f.sub, f.e4) = (target, heading);
            return found;
        }
        if sow.is_empty() {
            return false;
        }
        let f = &mut self.flyers[k];
        let total: u32 = sow.iter().map(|e| e.1).sum();
        let r = f.draw();
        let pick = if total == 0 {
            sow[(r % sow.len() as u32) as usize].0
        } else {
            let t = r % total;
            let mut sum = 0;
            sow.iter()
                .find(|e| {
                    sum += e.1;
                    sum >= t
                })
                .map_or(sow[0].0, |e| e.0)
        };
        f.e4 = pick;
        true
    }

    /// 0x41ef00: a dragon takes off while an object of side 1 and a nation
    /// other than 4 is in its nest's range.
    fn someone_in_range(&self, k: usize) -> bool {
        let f = &self.flyers[k];
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        let slots = self.slot_map();
        slots.iter().any(|(c, s)| {
            c.0 >= 0
                && c.1 >= 0
                && c.0 < w
                && c.1 < h
                && (c.0 - f.home.0).abs() <= f.range.0
                && (c.1 - f.home.1).abs() <= f.range.1
                && matches!(s.occupant, Some(Held::Object(j)) if {
                    let o = &self.objects[j];
                    Self::side_of(o) == 1 && o.owner.is_some_and(|w| w.0 != 4)
                })
        })
    }

    /// The shot of state 5 (message 0x14ba from 0x41fbd0), from its
    /// picture's corner + 0x27 (picture 0 at the place less 0x28, 0x78; a
    /// dragon's less 0x43, 0x92). A harpy's at frame 7 flies to the prey's
    /// aim point as a hit (vt+0xc with 100), drawn as L (0xb), a sure
    /// shot carrying 20 x item 4's level-1 blow (the A x 15 cache's copy,
    /// 0x5c73dc), shared out on a ship as any blow, taken by a settlement,
    /// town or fort as any shot's (0x138e). A dragon's at frame
    /// 16 flies to the middle of its chosen cell, drawn as P (0xf), with
    /// no target and no blow; then its teeth are sown there by the cell's
    /// ground (0x42ebf0): on land (0xf) a cyclops, minotaur or amazon
    /// (class 500 + its generator's `r % 3`, message 0x1497 with `{class,
    /// 1, 0, 0, 0, x, y}`), on water (0x2c0) a sea monster of type `r & 3`
    /// for nation 4 slot 0 (0x493780(scene, 4, 0, r & 3, x, y, 4, 4)),
    /// else nothing. First the sound, mode 5 at its cell: a harpy's event
    /// 0x7d (`atgarp1`), a dragon's 0x7e (`dr1` or `dr3`, `rand() % 2`
    /// by 0x554820).
    pub(super) fn flyer_strikes(
        &mut self,
        k: usize,
        class: u32,
        prey: u32,
        at: (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        let cry = if class == DRAGON {
            ["dr1", "dr3"][self.gull_rng.next() as usize % 2]
        } else {
            "atgarp1"
        };
        let (x, y) = centre(self.flyers[k].cell);
        self.sounds.push((cry, x as f32, y as f32));
        let (px, py) = self.flyers[k].pos;
        let (ox, oy) = if class == DRAGON {
            (0x43, 0x92)
        } else {
            (0x28, 0x78)
        };
        let from = ((px - ox + 0x27) as f32, (py - oy + 0x27) as f32);
        if class == DRAGON {
            let (cx, cy) = centre(at);
            self.fire_shot_to(
                super::shot::Firing {
                    item: 4,
                    letter: Some(15),
                    flaming: false,
                    from,
                    target: 0,
                    strikes: false,
                    damage: 0.0,
                    wounds: 0.0,
                    wear: 0.0,
                    rope: None,
                    shooter: None,
                    by: None,
                    to: None,
                },
                (cx as f32, cy as f32),
                data,
                caches,
            );
            let t = self.sow_ground(at);
            if t & 0xf != 0 {
                let class = 500 + self.flyers[k].draw() % 3;
                self.put_walker(super::walkers::NO_LAIR, class, at, data, caches);
            } else if t & 0x2c0 != 0 {
                let r = self.flyers[k].draw();
                let _ = self.spawn_hazard_drawn(
                    crate::magic::Spell::Monster,
                    at,
                    Some((4, 0)),
                    Some(r),
                    data,
                    caches,
                );
            }
            return;
        }
        let Some(i) = self.object_index(prey) else {
            return;
        };
        // A settlement, town or fort takes the blow as any shot's
        // (message 0x138e): a fort shares it out on its units
        // (`fort_blow`), the others' LandObj (0x423d60) takes a fifth of
        // it under a weapon shield (`+0x17c`); it flies to the building's aim point (vt+0xc
        // 0x423520) as a hit.
        if self.town_or_fort(i) {
            let blow = crate::combat::hull_damage_at(4, 1) * 20.0;
            let o = &self.objects[i];
            let warded =
                !o.holds_units() && o.combat.as_ref().is_some_and(|c| c.protect_weapons > 0.0);
            let damage = if warded {
                blow * crate::magic::SHIELD_FACTOR
            } else {
                blow
            };
            let to = self.building_aim(i, true);
            self.fire_shot(
                super::shot::Firing {
                    item: 4,
                    letter: Some(11),
                    flaming: false,
                    from,
                    target: prey,
                    strikes: true,
                    damage,
                    wounds: 0.0,
                    wear: 0.0,
                    rope: None,
                    shooter: None,
                    by: None,
                    to: Some((to.0 as f32, to.1 as f32)),
                },
                data,
                caches,
            );
            return;
        }
        let o = &self.objects[i];
        let (Some(s), Some(c)) = (&o.ship, &o.combat) else {
            return;
        };
        let blow = if s.has_hero(ODYSSEUS) {
            0.0
        } else {
            crate::combat::hull_damage_at(4, 1) * 20.0
        };
        let blow = blow * crate::combat::armour_factor(s.armour, &self.levels_of(o.owner));
        let sh = crate::combat::Shares::of(s, &c.weapons);
        let damage = sh.hull_part(blow);
        let wounds = sh.crew_part(blow) * 15.0;
        let wear = sh.weapons_part(blow) * 15.0;
        let to = self.ship_aim(i, true);
        self.fire_shot(
            super::shot::Firing {
                item: 4,
                letter: Some(11),
                flaming: false,
                from,
                target: prey,
                strikes: true,
                damage,
                wounds,
                wear,
                rope: None,
                shooter: None,
                by: None,
                to: Some((to.0 as f32, to.1 as f32)),
            },
            data,
            caches,
        );
    }

    /// A shot's blow on harpy or dragon `id` (vt+4 0x420670, message
    /// 0x138e, `damage` the shot's `[+9]` in hull units): less than its
    /// life, it takes it off and, a draw of its own generator `r % 7 ==
    /// 0`, cries (mode 5 at its cell: event 0x72 harpy, `hgar4` or
    /// `hgar5` by `rand() % 2` (0x554820); 0x73 dragon, `hdr2`); else its
    /// life is 0, the tick head then setting its dying state (its picture,
    /// then removed). An orphan's decay start `+0xba` loses the blow too
    /// (0 if not less). Either way it has been hit since its rest began
    /// (`+0xca`). No hit shown and no death reported.
    pub(super) fn flyer_struck(&mut self, id: u32, damage: f32) {
        let Some(k) = self.flyers.iter().position(|f| f.id == id) else {
            return;
        };
        let dmg = (damage.max(0.0) * 15.0).round() as u32;
        let hp = self.flyer_hp(k);
        let Some(c) = self
            .object_index(id)
            .and_then(|i| self.objects[i].combat.as_mut())
        else {
            return;
        };
        if dmg < hp {
            c.hull = (hp - dmg) as f32 / 15.0;
            if self.flyers[k].draw() % 7 == 0 {
                let cry = if self.flyer_class(k) == DRAGON {
                    "hdr2"
                } else {
                    ["hgar4", "hgar5"][self.gull_rng.next() as usize % 2]
                };
                let (x, y) = centre(self.flyers[k].cell);
                self.sounds.push((cry, x as f32, y as f32));
            }
        } else {
            c.hull = 0.0;
        }
        let f = &mut self.flyers[k];
        if !f.has_nest {
            // Less than it, the blow comes off; else 0.
            f.orphan_hp = f.orphan_hp.saturating_sub(dmg);
        }
        f.hit = true;
    }

    /// A blow on harpy nest `nest` while standing (0x41d300, message
    /// 0x138e), before it is taken: one less than its life marks the
    /// nest hit at this tick (`+0x1e8`, `+0x1ec`) and sends its last
    /// creature (`+0x1e4`) message 0xdaf7 with the shooter's id (`[+0x16]`):
    /// the creature chases it (`+0xce` 1, `+0xd2` the id). (Not decoded:
    /// `[+0x16]` of a shot with no shooter: no chase is sent.)
    pub(super) fn nest_struck(&mut self, nest: u32, damage: f32, shooter: Option<u32>) {
        let Some(hull) = self
            .object_index(nest)
            .and_then(|i| self.objects[i].combat.as_ref())
            .map(|c| c.hull)
            .filter(|&h| h > 0.0)
        else {
            return;
        };
        let dmg = (damage.max(0.0) * 15.0).round() as u32;
        if dmg >= (hull * 15.0).round() as u32 {
            return;
        }
        let now = self.ticks;
        let Some(n) = self.nests.iter_mut().find(|n| n.id == nest) else {
            return;
        };
        n.hit_at = Some(now);
        let creature = n.creature;
        if let (Some(by), Some(f)) = (shooter, self.flyers.iter_mut().find(|f| f.id == creature)) {
            f.forced = Some(by);
        }
    }

    /// The nests' tick (0x41d4a0, message 0): 3000 ticks after a nest's
    /// last blow its chase ends, message 0xdaf8 to its last creature
    /// (`+0xce` and `+0xd6` cleared).
    pub(super) fn tick_nests(&mut self) {
        let now = self.ticks;
        for k in 0..self.nests.len() {
            let n = &mut self.nests[k];
            if n.hit_at.is_some_and(|t| t.wrapping_add(3000) <= now) {
                n.hit_at = None;
                let creature = n.creature;
                if let Some(f) = self.flyers.iter_mut().find(|f| f.id == creature) {
                    f.forced = None;
                    f.forced_done = false;
                }
            }
        }
    }

    /// Whether world pixel `(x, y)` is on flyer `id` (its pixel test,
    /// vt+8 0x41deb0, as the pointer's pick 0x49a580 asks it): never a
    /// dragon in state 0 or 1; else within picture 0's rectangle (`+0x59`,
    /// `+0x5d`, `+0x61` x `+0x65`: the creature, not its shadow), the bit
    /// of the mask frame at its frame `+0x45` (0x543240, none past the
    /// mask's count: no hit; 0x557640, rows of the picture's width rounded
    /// up to 8 bits, the first bit of a byte the highest).
    pub fn flyer_hit(&self, id: u32, x: i32, y: i32) -> bool {
        let Some(f) = self.flyers.iter().find(|f| f.id == id) else {
            return false;
        };
        let Some(i) = self.object_index(id) else {
            return false;
        };
        let o = &self.objects[i];
        if o.record.class_id == DRAGON && f.state <= REST {
            return false;
        }
        let Some(body) = o.frames.first().and_then(|l| l.last()) else {
            return false;
        };
        let (left, top) = crate::render::layer_origin(o, body);
        let (w, h) = (body.image.width as i32, body.image.height as i32);
        if x < left || x >= left + w || y < top || y >= top + h {
            return false;
        }
        let Some(mask) = f
            .masks
            .0
            .as_ref()
            .and_then(|m| usize::try_from(f.frame).ok().and_then(|k| m.get(k)))
        else {
            return false;
        };
        mask.bit(((w + 7) & !7) as usize * (y - top) as usize + (x - left) as usize)
    }

    /// Whether flyer `id` can be taken (vt+0x2c, 0x41de90): not a dragon
    /// resting (state 1); its life is not looked at.
    pub(super) fn flyer_there(&self, id: u32) -> bool {
        let class = self
            .object_index(id)
            .map(|i| self.objects[i].record.class_id);
        !self
            .flyers
            .iter()
            .any(|f| f.id == id && f.state == REST && class == Some(DRAGON))
    }

    /// The point a shot at harpy or dragon `id` flies to (vt+0xc,
    /// 0x41df70, by the hit byte), from its picture's top left (picture 0
    /// drawn at the place less 0x28, 0x78; a dragon's less 0x43, 0x92;
    /// `+0x59`, `+0x5d`): a hit on a dragon (0x43, 0x3c); on a harpy by
    /// state, 0 and 1 0x595ae0's first `{x, y}`, 2 its `{x2, y2}` and 6
    /// its `{x6, y6}` at the frame (`+0x45`), any other (0x28, 0x28); a
    /// miss two draws of its own generator, `(r & 1) x w`, `(r & 1) x h`
    /// (the picture's `+0x61`, `+0x65`: the port takes its first frame's
    /// size, as for the land creatures). Clamped to the map. (Not decoded:
    /// a dragon's picture place before its first draw, states 0 and 1,
    /// where it is not drawn: the port takes the draw's.)
    pub(super) fn flyer_aim(&mut self, id: u32, hit: bool) -> Option<(i32, i32)> {
        let i = self.object_index(id)?;
        let k = self.flyers.iter().position(|f| f.id == id)?;
        let class = self.objects[i].record.class_id;
        let (w, h) = self.objects[i]
            .frames
            .first()
            .and_then(|f| f.last())
            .map_or((0, 0), |l| (l.image.width as i32, l.image.height as i32));
        let f = &mut self.flyers[k];
        let (ox, oy) = if class == DRAGON {
            (0x43, 0x92)
        } else {
            (0x28, 0x78)
        };
        let (px, py) = (f.pos.0 - ox, f.pos.1 - oy);
        let frame = f.frame.max(0) as usize;
        let (x, y) = if !hit {
            let x = (f.draw() & 1) as i32 * w;
            (x, (f.draw() & 1) as i32 * h)
        } else if class == DRAGON {
            (0x43, 0x3c)
        } else {
            match f.state {
                APPEAR | REST => (AIM_OFFSETS[0].0, AIM_OFFSETS[0].1),
                TAKE_OFF => AIM_OFFSETS.get(frame).map_or((0, 0), |o| (o.0, o.1)),
                LAND => AIM_OFFSETS.get(frame).map_or((0, 0), |o| (o.2, o.3)),
                _ => (0x28, 0x28),
            }
        };
        let (mw, mh) = self.map_size();
        let cap = |v: i32, n: u32| if v < 0 { 0 } else { v.min(n as i32 * 64 - 1) };
        Some((cap(px + x, mw), cap(py + y, mh)))
    }
}
