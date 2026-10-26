//! What merchants are shown doing (Run_Ship.cpp, 0x4e1bb0): unloading at
//! a port or settlement they lay a walkway to the shore (the composite
//! `MOST0_`, one layer per facing) and
//! porters (`NOSIL_<loaded><0><heading>`) carry the goods to the building
//! and walk back, while the cargo comes in little by little; gathering
//! amber, a walkway to the deposit (`AMOST_<facing>`) with porters bringing
//! it aboard (`ANOSIL_<facing>`); fishing, their net (`SETKA_<facing>`).
//! All only shown; the goods are counted by the ship's task.

use super::{heading_towards, Scene};
use crate::assets::GameData;
use crate::objects::{self, Caches, Layer};

/// Units of cargo unloaded a second. An estimate (the original's looked
/// slower than our first 100).
pub const UNLOAD_RATE: f32 = 40.0;
/// Seconds between porters setting out, and how fast they walk (world
/// pixels a second). Placeholders.
const PORTER_EVERY: f32 = 1.2;
const PORTER_SPEED: f32 = 40.0;
/// How far below a fishing ship's middle its net hangs.
const NET_DROP: f32 = 16.0;
/// Milliseconds per frame of a walkway to amber, a net and the porters
/// bringing amber aboard: one a game tick (0x4b0590, 0x4ae760); a load
/// comes aboard each round.
const PLANK_FRAME_MS: f32 = 20.0;
/// Milliseconds per frame of the walkway laid down to unload: slower (an
/// estimate; only shown).
const WALKWAY_FRAME_MS: f32 = 60.0;
const PORTER_FRAME_MS: u64 = 100;

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
    /// Where porters set out from (the shore end) and go, and the time
    /// since the last set out.
    from: (f32, f32),
    to: (f32, f32),
    clock: f32,
}

/// A porter walking between a walkway and the building.
pub struct Porter {
    pub x: f32,
    pub y: f32,
    pub frames: Vec<Vec<Layer>>,
    from: (f32, f32),
    to: (f32, f32),
    loaded: bool,
}

impl Plank {
    /// Milliseconds per frame of its animation.
    fn frame_ms(&self) -> f32 {
        if self.activity == Activity::Unloading {
            WALKWAY_FRAME_MS
        } else {
            PLANK_FRAME_MS
        }
    }

    /// Seconds a round of the work takes (the porters' or the net's
    /// cycle), once the walkway is down; `None` before.
    pub fn round_seconds(&self) -> Option<f32> {
        let down = self.age_ms >= self.frames.len() as f32 * self.frame_ms();
        (down && !self.looping.is_empty())
            .then(|| self.looping.len() as f32 * self.frame_ms() / 1000.0)
    }

    /// The walkway's frame, and the looped work over it once it is down.
    pub fn frame(&self) -> Vec<&[Layer]> {
        let step = (self.age_ms / self.frame_ms()) as usize;
        let n = self.frames.len();
        let mut out = Vec::new();
        if n > 0 {
            out.push(self.frames[step.min(n - 1)].as_slice());
        }
        if step >= n && !self.looping.is_empty() {
            out.push(self.looping[(step - n) % self.looping.len()].as_slice());
        }
        out
    }
}

impl Porter {
    pub fn frame(&self, time_ms: u64) -> Option<&[Layer]> {
        let n = self.frames.len();
        (n > 0).then(|| self.frames[(time_ms / PORTER_FRAME_MS) as usize % n].as_slice())
    }
}

impl Scene {
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
        let (frames, looping) = match activity {
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
                (Vec::new(), seq(&format!("SETKA_{}", ship_heading / 2 % 4)))
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
        let (x, y) = (sx + ux * reach / 2.0, sy + uy * reach / 2.0);
        // The net hangs in the water below the hull (measured on the
        // original: its middle about 16 pixels under the ship's).
        let y = if activity == Activity::Fishing {
            y + NET_DROP
        } else {
            y
        };
        let shore = (sx + ux * reach, sy + uy * reach);
        self.planks.push(Plank {
            ship,
            activity,
            x,
            y,
            frames,
            looping,
            age_ms: 0.0,
            from: shore,
            to: (bx, by),
            clock: PORTER_EVERY,
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
                            Activity::Fishing => kind == Some(objects::Resource::Fish),
                            Activity::Unloading => false,
                        }
                    }
                    _ => false,
                }
            })
        };
        let planks = std::mem::take(&mut self.planks);
        self.planks = planks.into_iter().filter(|p| working(self, p)).collect();
        let mut new = Vec::new();
        for p in &mut self.planks {
            p.age_ms += dt * 1000.0;
            p.clock += dt;
            // Porters start once the walkway is down.
            if p.activity == Activity::Unloading
                && p.age_ms >= p.frames.len() as f32 * p.frame_ms()
                && p.clock >= PORTER_EVERY
            {
                p.clock = 0.0;
                new.push((p.from, p.to));
            }
        }
        for (from, to) in new {
            let frames = porter_frames(data, caches, true, from, to);
            self.porters.push(Porter {
                x: from.0,
                y: from.1,
                frames,
                from,
                to,
                loaded: true,
            });
        }
        let mut done = Vec::new();
        for (k, p) in self.porters.iter_mut().enumerate() {
            let (dx, dy) = (p.to.0 - p.x, p.to.1 - p.y);
            let d = dx.hypot(dy);
            let step = PORTER_SPEED * dt;
            if d <= step {
                if p.loaded {
                    // Goods delivered: back to the ship, empty-handed.
                    p.loaded = false;
                    std::mem::swap(&mut p.from, &mut p.to);
                    p.frames = porter_frames(data, caches, false, p.from, p.to);
                } else {
                    done.push(k);
                }
            } else {
                p.x += dx / d * step;
                p.y += dy / d * step;
            }
        }
        for k in done.into_iter().rev() {
            self.porters.remove(k);
        }
    }
}

/// A porter's walking frames: loaded or not, heading from `from` to `to`.
fn porter_frames(
    data: &GameData,
    caches: &mut Caches,
    loaded: bool,
    from: (f32, f32),
    to: (f32, f32),
) -> Vec<Vec<Layer>> {
    let heading = heading_towards(to.0 - from.0, to.1 - from.1);
    let name = format!("NOSIL_{}0{heading}", u8::from(loaded));
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
        if o.class != openac_formats::ObjectClass::Amber {
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
fn amber_level(left: f32, full: f32) -> usize {
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
            let Some((dep, empty_for)) = self.objects[i].dormant.as_mut() else {
                continue;
            };
            *empty_for += dt;
            if *empty_for < objects::REGROW_SECONDS || dep.regrows == 0 {
                continue;
            }
            let mut dep = *dep;
            dep.regrows -= 1;
            dep.remaining = Some(dep.refill);
            self.objects[i].dormant = None;
            self.objects[i].deposit = Some(dep);
            self.refresh_amber(i, data, caches);
        }
    }
}
