//! What merchants are shown doing (Run_Ship.cpp, 0x4e1bb0): unloading at
//! a port or settlement they lay a walkway to the shore (the composite
//! `MOST0_`, one layer per facing) and porters
//! (`NOSIL_<loaded><0><heading>`) walk it, laden ashore and back empty,
//! while the cargo comes in little by little; gathering
//! amber, a walkway to the deposit (`AMOST_<facing>`) with porters bringing
//! it aboard (`ANOSIL_<facing>`); fishing, their net (`SETKA_<facing>`).
//! All only shown; the goods are counted by the ship's task.

use super::{heading_towards, Scene};
use crate::assets::GameData;
use crate::objects::{self, Caches, Layer};

/// Goods a laden porter trip carries ashore: a cargo slot of 100 (the ship
/// holds its load in slots of 100, 0x4ad300, and each trip is started by
/// one message, 0x4aab00).
pub const TRIP_LOAD: f32 = 100.0;
/// Units of cargo unloaded a second where there is no walkway to show
/// (a slot a laden and an empty trip, 2 x 38 ticks).
pub const UNLOAD_RATE: f32 = TRIP_LOAD / (76.0 * crate::clock::TICK);
/// Where the unloading walkway lies: its picture's top-left from the
/// top-left of the ship's frame, by ship type, side (see `walkway_side`)
/// and facing (0x59a8e8, 0x4adef0 state 0xb). Types 4-11 share a row.
const WALKWAY_AT: [[[(i8, i8); 8]; 2]; 5] = [
    [
        [
            (19, 5),
            (25, -8),
            (3, -33),
            (-13, -14),
            (-29, 5),
            (-6, 14),
            (1, 7),
            (10, 10),
        ],
        [
            (19, 5),
            (25, -8),
            (19, -33),
            (-13, -14),
            (-29, 5),
            (-6, 14),
            (26, 12),
            (10, 18),
        ],
    ],
    [
        [
            (20, 8),
            (29, -3),
            (10, -29),
            (-10, 0),
            (-26, 12),
            (-2, 20),
            (1, 18),
            (14, 17),
        ],
        [
            (23, 8),
            (29, -3),
            (20, -29),
            (-10, 0),
            (-26, 12),
            (-2, 20),
            (28, 18),
            (14, 17),
        ],
    ],
    [
        [
            (31, 17),
            (33, 2),
            (7, -21),
            (-6, 3),
            (-22, 17),
            (8, 22),
            (10, 29),
            (22, 26),
        ],
        [
            (31, 17),
            (32, 2),
            (37, -21),
            (-6, 3),
            (-22, 17),
            (8, 22),
            (33, 29),
            (22, 26),
        ],
    ],
    [
        [
            (31, 17),
            (33, 2),
            (7, -21),
            (-6, 3),
            (-22, 17),
            (8, 22),
            (10, 29),
            (22, 26),
        ],
        [
            (31, 17),
            (32, 2),
            (37, -21),
            (-6, 3),
            (-22, 17),
            (8, 22),
            (33, 29),
            (22, 26),
        ],
    ],
    [
        [
            (22, 13),
            (28, -3),
            (9, -25),
            (-9, -2),
            (-21, 15),
            (4, 26),
            (6, 23),
            (18, 18),
        ],
        [
            (22, 13),
            (28, -3),
            (30, -25),
            (-9, -2),
            (-21, 15),
            (4, 26),
            (30, 22),
            (18, 18),
        ],
    ],
];

/// Where the walkway to amber lies: its top-left from the top-left of the
/// ship's frame, by ship type, for a shore east, west, north (shore groups
/// 4, 5 and 11, or 6 and 7) or south (0, 1 and 10, or 2 and 3) (0x4b0590,
/// state 0xb).
const AMBER_WALKWAY_E: [(i8, i8); 12] = [
    (30, 26),
    (38, 26),
    (45, 34),
    (45, 34),
    (38, 33),
    (38, 33),
    (38, 33),
    (38, 33),
    (38, 33),
    (38, 33),
    (38, 33),
    (38, 33),
];
const AMBER_WALKWAY_W: [(i8, i8); 12] = [
    (-25, 26),
    (-23, 26),
    (-20, 34),
    (-20, 33),
    (-21, 33),
    (-20, 34),
    (-21, 33),
    (-21, 33),
    (-21, 33),
    (-21, 33),
    (-21, 33),
    (-21, 33),
];
const AMBER_WALKWAY_N: [[(i8, i8); 12]; 2] = [
    [
        (37, -22),
        (40, -20),
        (52, -9),
        (52, -9),
        (52, -12),
        (51, -14),
        (52, -12),
        (52, -12),
        (52, -12),
        (52, -12),
        (52, -12),
        (52, -12),
    ],
    [
        (4, -18),
        (12, -21),
        (16, -6),
        (16, -6),
        (10, -14),
        (10, -15),
        (12, -6),
        (12, -6),
        (12, -6),
        (12, -6),
        (12, -6),
        (12, -6),
    ],
];
const AMBER_WALKWAY_S: [[(i8, i8); 12]; 2] = [
    [
        (38, 26),
        (44, 29),
        (52, 39),
        (52, 39),
        (50, 32),
        (47, 31),
        (50, 32),
        (50, 32),
        (50, 32),
        (50, 32),
        (50, 32),
        (50, 32),
    ],
    [
        (4, 24),
        (7, 28),
        (15, 37),
        (15, 37),
        (10, 32),
        (12, 32),
        (10, 32),
        (12, 32),
        (10, 32),
        (12, 32),
        (10, 32),
        (12, 32),
    ],
];

/// Which of a walkway's two placings a ship on `heading` uses for `facing`
/// (0x4b8a40).
fn walkway_side(facing: usize, heading: usize) -> usize {
    let not = [2, 3, 0, 1][facing % 4];
    usize::from(heading != not)
}
/// Where a fishing ship's net lies: its picture's top-left from the top-left
/// of the ship's frame, by ship type and heading (0x59afa8; headings four
/// apart alike).
const NET_AT: [[(i8, i8); 4]; 12] = [
    [(9, 32), (21, 29), (-3, 29), (-4, 27)],
    [(14, 34), (28, 31), (0, 29), (-1, 33)],
    [(19, 43), (35, 40), (8, 39), (10, 40)],
    [(19, 43), (35, 40), (8, 39), (10, 40)],
    [(16, 34), (31, 33), (7, 35), (3, 31)],
    [(15, 37), (31, 33), (6, 32), (2, 31)],
    [(16, 34), (31, 33), (7, 35), (3, 31)],
    [(15, 37), (31, 33), (6, 32), (2, 31)],
    [(16, 34), (31, 33), (7, 35), (3, 31)],
    [(15, 37), (31, 33), (6, 32), (2, 31)],
    [(16, 34), (31, 33), (7, 35), (3, 31)],
    [(15, 37), (31, 33), (6, 32), (2, 31)],
];
/// Milliseconds per frame of a walkway, a net and the porters bringing
/// amber aboard: one a game tick (0x4b0590, 0x4ae760); a load comes aboard
/// each round.
const PLANK_FRAME_MS: f32 = crate::clock::TICK_MS as f32;

/// What a merchant is shown doing (the ship's activity in Run_Ship.cpp,
/// 0x4e1bb0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    /// At a port or settlement: the walkway `MOST0_` (a composite of 18
    /// frames whose eight layers are the facings) and porters carrying the
    /// goods ashore.
    Unloading,
    /// At an amber deposit: the walkway `AMOST_<facing>` to the shore and
    /// porters bringing amber aboard (`ANOSIL_<facing>`).
    Amber,
    /// At a fishing ground: the net (`SETKA_<facing>`, four facings).
    Fishing,
}

/// A merchant's walkway or net: its going down played once, then what it
/// loops while the work goes on.
pub struct Plank {
    pub ship: u32,
    pub activity: Activity,
    pub x: f32,
    pub y: f32,
    pub frames: Vec<Vec<Layer>>,
    pub looping: Vec<Vec<Layer>>,
    pub age_ms: f32,
    /// The walkway's top-left (porters walk it from there) and facing,
    /// and how many porter trips it has seen: laden ashore on even ones,
    /// back empty on odd ones.
    top_left: (f32, f32),
    facing: usize,
    pub trips: u32,
    /// Where the looped work (porters bringing amber aboard, the net) is
    /// drawn about.
    loop_at: (f32, f32),
}

/// A trip of porters over a walkway (0x4adda0): one picture laid on it
/// (71 by 38), the men walking across it as it plays, a frame a tick.
pub struct Porter {
    /// The ship whose goods it carries.
    pub ship: u32,
    pub x: f32,
    pub y: f32,
    pub frames: Vec<Vec<Layer>>,
    age_ms: f32,
    laden: bool,
}

impl Plank {
    /// Milliseconds per frame of its animation.
    fn frame_ms(&self) -> f32 {
        PLANK_FRAME_MS
    }

    /// Seconds a round of the work takes (the porters' or the net's
    /// cycle), once the walkway is down; `None` before.
    pub fn round_seconds(&self) -> Option<f32> {
        let down = self.age_ms >= self.frames.len() as f32 * self.frame_ms();
        (down && !self.looping.is_empty())
            .then(|| self.looping.len() as f32 * PLANK_FRAME_MS / 1000.0)
    }

    /// The walkway's frame, and the looped work over it once it is down.
    /// The walkway's frame, and the looped work over it once it is down,
    /// each with the point it is drawn about.
    pub fn frame(&self) -> Vec<(&[Layer], f32, f32)> {
        let step = (self.age_ms / self.frame_ms()) as usize;
        let n = self.frames.len();
        let mut out = Vec::new();
        if n > 0 {
            out.push((self.frames[step.min(n - 1)].as_slice(), self.x, self.y));
        }
        if step >= n && !self.looping.is_empty() {
            let (lx, ly) = self.loop_at;
            out.push((
                self.looping[(step - n) % self.looping.len()].as_slice(),
                lx,
                ly,
            ));
        }
        out
    }
}

impl Porter {
    /// Its frame, while the trip lasts.
    pub fn frame(&self, _time_ms: u64) -> Option<&[Layer]> {
        let f = (self.age_ms / PLANK_FRAME_MS) as usize;
        self.frames.get(f).map(|l| l.as_slice())
    }
}

impl Scene {
    /// The direction of the shore and the top-left offset of the walkway
    /// ship `i` lays to amber deposit `d`, when the deposit's cell lies
    /// straight beside the ship's.
    fn amber_walkway(&self, i: usize, d: usize) -> Option<(usize, (i8, i8))> {
        let t = openconquest_formats::map::TILE_SIZE as f32;
        let o = &self.objects[i];
        let k = o.ship.as_ref()?.kind as usize % 12;
        let f = self.objects[d].record.fields.get(6..8)?;
        let (tx, ty) = (f[0] as i32, f[1] as i32);
        let (cx, cy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let group = super::trade::shore_group(&self.grid, tx, ty);
        let in_group = |gs: &[usize]| group.is_some_and(|g| gs.contains(&g));
        match (tx - cx, ty - cy) {
            (1, 0) => Some((0, AMBER_WALKWAY_E[k])),
            (-1, 0) => Some((4, AMBER_WALKWAY_W[k])),
            (0, -1) if in_group(&[4, 5, 11]) => Some((2, AMBER_WALKWAY_N[0][k])),
            (0, -1) if in_group(&[6, 7]) => Some((2, AMBER_WALKWAY_N[1][k])),
            (0, 1) if in_group(&[0, 1, 10]) => Some((6, AMBER_WALKWAY_S[0][k])),
            (0, 1) if in_group(&[2, 3]) => Some((6, AMBER_WALKWAY_S[1][k])),
            _ => None,
        }
    }

    /// Ship `i` works at `target` (a building it unloads at or a deposit):
    /// a walkway to the shore on the way there, or its net, if it has none
    /// yet.
    pub(super) fn show_activity(
        &mut self,
        i: usize,
        target: u32,
        activity: Activity,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let ship = self.objects[i].id;
        if self
            .planks
            .iter()
            .any(|p| p.ship == ship && p.activity == activity)
        {
            return;
        }
        self.planks.retain(|p| p.ship != ship);
        let Some(s) = self.object_index(target) else {
            return;
        };
        let (sx, sy) = (self.objects[i].x, self.objects[i].y);
        let (bx, by) = (self.objects[s].x, self.objects[s].y);
        let (dx, dy) = (bx - sx, by - sy);
        let heading = heading_towards(dx, dy);
        let mut seq = |name: &str| objects::sequence_frames(data, caches, name).unwrap_or_default();
        let (mut frames, mut looping) = match activity {
            Activity::Unloading => (
                objects::composite_layer_frames(data, caches, "MOST0_", heading)
                    .unwrap_or_default(),
                Vec::new(),
            ),
            Activity::Amber => {
                let facing = heading / 2 * 2;
                (
                    seq(&format!("AMOST_{facing}")),
                    seq(&format!("ANOSIL_{facing}")),
                )
            }
            Activity::Fishing => {
                let ship_heading = self.objects[i].ship.as_ref().map_or(0, |s| s.heading);
                // Its long side along the hull: 0 east-west, 1 north-east,
                // 2 north-south, 3 north-west.
                (Vec::new(), seq(&format!("SETKA_{}", ship_heading % 4)))
            }
        };
        // A walkway reaches from the ship towards the target: its far end
        // is as far out as the laid-down picture is long that way. A net
        // hangs at the ship.
        let (ux, uy) = {
            let d = dx.hypot(dy).max(1.0);
            (dx / d, dy / d)
        };
        let reach = match (activity, frames.last().and_then(|f| f.first())) {
            (Activity::Fishing, _) | (_, None) => 0.0,
            (_, Some(l)) => {
                (ux.abs() * l.image.width as f32 + uy.abs() * l.image.height as f32) * 0.8
            }
        };
        let (mut x, mut y) = (sx + ux * reach / 2.0, sy + uy * reach / 2.0);
        let mut top_left = (x, y);
        // A walkway to a port lies where the ship type's table puts it for
        // its facing (0x4adef0, state 0xb); drawn by its middle.
        if activity == Activity::Unloading {
            if let (Some(s), Some(l)) = (
                self.objects[i].ship.as_ref(),
                frames.last().and_then(|f| f.first()),
            ) {
                let k = s.kind as usize % 12;
                let side = walkway_side(heading, s.heading);
                let (ox, oy) = WALKWAY_AT[k.min(4)][side][heading];
                let half = objects::SHIP_FRAME[k] / 2.0;
                top_left = (sx - half + ox as f32, sy - half + oy as f32);
                x = top_left.0 + l.image.width as f32 / 2.0;
                y = top_left.1 + l.image.height as f32 / 2.0;
            }
        }
        // A walkway to amber likewise (0x4b0590, state 0xb), by the
        // direction of the shore and its shape; its porters on it.
        let mut facing = heading;
        if activity == Activity::Amber {
            if let Some((dir, off)) = self.amber_walkway(i, s) {
                facing = dir;
                frames = objects::sequence_frames(data, caches, &format!("AMOST_{dir}"))
                    .unwrap_or_default();
                looping = objects::sequence_frames(data, caches, &format!("ANOSIL_{dir}"))
                    .unwrap_or_default();
                if let Some(l) = frames.last().and_then(|f| f.first()) {
                    let k = self.objects[i]
                        .ship
                        .as_ref()
                        .map_or(0, |s| s.kind as usize % 12);
                    let half = objects::SHIP_FRAME[k] / 2.0;
                    top_left = (sx - half + off.0 as f32, sy - half + off.1 as f32);
                    x = top_left.0 + l.image.width as f32 / 2.0;
                    y = top_left.1 + l.image.height as f32 / 2.0;
                }
            }
        }
        // The net lies where the ship type's table puts it for its heading
        // (0x4ae760, state 6): its picture's top-left that far from the
        // ship frame's; drawn by its middle.
        if activity == Activity::Fishing {
            if let (Some(s), Some(l)) = (
                self.objects[i].ship.as_ref(),
                looping.first().and_then(|f| f.first()),
            ) {
                let k = s.kind as usize % 12;
                let (ox, oy) = NET_AT[k][s.heading % 4];
                let half = objects::SHIP_FRAME[k] / 2.0;
                x = sx - half + ox as f32 - l.dx as f32 + l.image.width as f32 / 2.0;
                y = sy - half + oy as f32 - l.dy as f32 + l.image.height as f32 / 2.0;
            }
        }
        let loop_at = match looping.first().and_then(|f| f.first()) {
            Some(l) if activity == Activity::Amber => (
                top_left.0 + l.image.width as f32 / 2.0,
                top_left.1 + l.image.height as f32 / 2.0,
            ),
            _ => (x, y),
        };
        self.planks.push(Plank {
            ship,
            activity,
            x,
            y,
            frames,
            looping,
            age_ms: 0.0,
            top_left,
            facing,
            trips: 0,
            loop_at,
        });
    }

    /// Walkways go once their ship has unloaded; porters come and go.
    pub(super) fn run_porters(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        // A walkway or net stays while its ship does that work.
        let working = |s: &Scene, p: &Plank| {
            s.object_index(p.ship).is_some_and(|i| {
                let ship = s.objects[i].ship.as_ref();
                match ship.and_then(|sh| sh.task) {
                    Some(objects::Task::Gather {
                        phase: objects::GatherPhase::Unloading(_),
                        ..
                    }) => p.activity == Activity::Unloading,
                    Some(objects::Task::Gather {
                        phase: objects::GatherPhase::Collecting,
                        ..
                    }) => {
                        let kind = ship.and_then(|sh| sh.cargo_kind);
                        match p.activity {
                            Activity::Amber => kind != Some(objects::Resource::Fish),
                            Activity::Fishing => matches!(
                                kind,
                                Some(objects::Resource::Fish | objects::Resource::Leviathan)
                            ),
                            Activity::Unloading => false,
                        }
                    }
                    _ => false,
                }
            })
        };
        let planks = std::mem::take(&mut self.planks);
        self.planks = planks.into_iter().filter(|p| working(self, p)).collect();
        // Ships with goods still aboard.
        let laden: std::collections::HashSet<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.cargo >= 1.0))
            .map(|o| o.id)
            .collect();
        // Trips play one after another once the walkway is down: laden
        // ashore while there are goods aboard, and back empty after each.
        for p in &mut self.porters {
            p.age_ms += dt * 1000.0;
        }
        // A laden trip done puts a slot of goods ashore.
        for p in self
            .porters
            .iter()
            .filter(|p| p.laden && p.frame(0).is_none())
        {
            self.unloaded.push(p.ship);
        }
        self.porters.retain(|p| p.frame(0).is_some());
        let mut new = Vec::new();
        for p in &mut self.planks {
            p.age_ms += dt * 1000.0;
            let down = p.age_ms >= p.frames.len() as f32 * p.frame_ms();
            let busy = self.porters.iter().any(|t| t.ship == p.ship);
            let back = p.trips % 2 == 1;
            if p.activity == Activity::Unloading
                && down
                && !busy
                && (back || laden.contains(&p.ship))
            {
                p.trips += 1;
                new.push((p.ship, p.top_left, !back, p.facing));
            }
        }
        for (ship, top_left, loaded, facing) in new {
            let frames = porter_frames(data, caches, loaded, facing);
            let (w, h) = frames
                .iter()
                .find_map(|f| f.first())
                .map_or((0.0, 0.0), |l| {
                    (l.image.width as f32, l.image.height as f32)
                });
            self.porters.push(Porter {
                ship,
                x: top_left.0 + w / 2.0,
                y: top_left.1 + h / 2.0,
                frames,
                age_ms: 0.0,
                laden: loaded,
            });
        }
    }
}

/// A porter trip's frames (`NOSIL_<laden><way><facing>`, 0x4b6ca0): laden
/// from the ship ashore (way 0), or empty back aboard (way 1), over a
/// walkway facing `facing`.
fn porter_frames(
    data: &GameData,
    caches: &mut Caches,
    loaded: bool,
    facing: usize,
) -> Vec<Vec<Layer>> {
    let name = format!("NOSIL_{}{}{facing}", u8::from(loaded), u8::from(!loaded));
    let mut frames = objects::sequence_frames(data, caches, &name).unwrap_or_default();
    // The first frame of a walk is empty.
    if frames.len() > 1 {
        frames.remove(0);
    }
    frames
}

impl Scene {
    /// Shows amber deposit `i` as the original does: its shore variant's
    /// picture for how much is left (of 1000), with its sparkle; nothing
    /// while it lies empty.
    pub(super) fn refresh_amber(&mut self, i: usize, data: &GameData, caches: &mut Caches) {
        let o = &self.objects[i];
        if o.class != openconquest_formats::ObjectClass::Amber {
            return;
        }
        let f = |k| o.record.field(k).unwrap_or(0);
        let (cx, cy) = (f(6), f(7));
        let tile = if cx < self.grid.width && cy < self.grid.height {
            self.grid.get(0, cx, cy).variant
        } else {
            0
        };
        // Lying empty, growing back: nothing to show.
        if o.deposit.is_none() {
            self.objects[i].frames = Vec::new();
            return;
        }
        // Against 1000, whatever it started with (0x48a800).
        let level = o
            .deposit
            .and_then(|d| d.remaining)
            .map_or(3, |left| amber_level(left, 1000.0));
        let variant = objects::amber_variant(tile);
        if let Ok(frames) = objects::amber_frames(data, caches, variant, level) {
            if !frames.is_empty() {
                self.objects[i].frames = frames;
            }
        }
    }
}

/// The picture size of an amber deposit with `left` of `full` (0x48d2f0):
/// 3 above 70 percent, 2 above 35, else 1 (0, gone, is not shown).
pub(super) fn amber_level(left: f32, full: f32) -> usize {
    match (left * 100.0 / full) as i32 {
        71.. => 3,
        36.. => 2,
        _ => 1,
    }
}

impl Scene {
    /// Exhausted amber grows back to what it held, as often as it may.
    pub(super) fn run_regrowth(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        for i in 0..self.objects.len() {
            let f40 = self.objects[i].record.field(40).unwrap_or(0).min(9);
            let Some((dep, empty_for)) = self.objects[i].dormant.as_mut() else {
                continue;
            };
            *empty_for += dt;
            let leviathan = dep.resource == objects::Resource::Leviathan;
            // A leviathan caught comes up again 27750 - 3000 x field 40
            // ticks later (0x494400 state 5), on a water cell of its zone.
            let wait = if leviathan {
                crate::clock::ticks(27_750 - 3000 * f40)
            } else {
                objects::REGROW_SECONDS
            };
            if *empty_for < wait || dep.regrows == 0 {
                continue;
            }
            let mut dep = *dep;
            dep.regrows -= 1;
            dep.remaining = Some(dep.refill);
            self.objects[i].dormant = None;
            self.objects[i].deposit = Some(dep);
            if leviathan {
                let f = |k: usize| self.objects[i].record.field(k).unwrap_or(0) as i32;
                let (zx, zy, zw, zh) = (f(7), f(8), f(10).max(1), f(11).max(1));
                let t = openconquest_formats::map::TILE_SIZE as f32;
                for _ in 0..100 {
                    let cx = zx + (self.rng.next() % zw as u32) as i32;
                    let cy = zy + (self.rng.next() % zh as u32) as i32;
                    if self.nav.is_water(cx, cy) {
                        self.objects[i].x = cx as f32 * t + t / 2.0;
                        self.objects[i].y = cy as f32 * t + t / 2.0;
                        break;
                    }
                }
            }
            self.refresh_amber(i, data, caches);
        }
    }
}
