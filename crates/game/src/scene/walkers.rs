//! Cyclops, minotaurs and amazons (CyclopClassTy) come out of their lairs
//! (CreaturesFlatClassTy, 0x414f00): a lair keeps up to record field 8 of
//! them about, sending another onto free land beside it now and then (the
//! quicker the higher field 9). They roam the land near the lair and throw
//! stones, axes or arrows at ships in reach (`combat::creature_weapons`).

use openac_formats::map::TILE_SIZE;
use openac_formats::{ObjectClass, ObjectRecord};

use super::{heading_towards, Scene};
use crate::assets::GameData;
use crate::objects::{self, Caches};

/// How far from the lair they roam (cells). A placeholder.
const ROAM: i32 = 5;
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
    pub lair: u32,
    pub walk: Walk,
    pub shown: (u8, usize),
}

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

    fn spawn_walker(
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
        let class = creature_of(lair_class);
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
        if let Ok(Some(obj)) = objects::resolve(data, caches, &rec) {
            // It comes out over its appearing picture, a frame a tick.
            let appear = obj.frames.len() as f32 * walker_frame_ms(0) as f32 / 1000.0;
            self.objects.push(obj);
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            self.walkers.push(Walker {
                id,
                lair,
                walk: Walk::Appear(appear),
                shown: (0, 0),
            });
        }
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
                    walk = match self.roam_target(w.lair) {
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

    /// A land cell near lair `lair` to wander to.
    fn roam_target(&mut self, lair: u32) -> Option<(i32, i32)> {
        let t = TILE_SIZE as f32;
        let i = self.object_index(lair)?;
        let (lx, ly) = (
            (self.objects[i].x / t) as i32,
            (self.objects[i].y / t) as i32,
        );
        for _ in 0..8 {
            let r = self.rng.next();
            let span = (2 * ROAM + 1) as u32;
            let (cx, cy) = (
                lx - ROAM + (r % span) as i32,
                ly - ROAM + (r / span % span) as i32,
            );
            if (cx, cy) != (lx, ly) && self.is_land(cx, cy) {
                return Some((cx, cy));
            }
        }
        None
    }
}
