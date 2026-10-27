//! Harpies and dragons (HarpyClassTy, 0x4207b0): each nest (0x41d4a0)
//! sends out its creature when the mission starts and, while it has
//! creatures left (record field 9), a new one some time after the last
//! fell (55500 ticks less 6000 per field 10). The creature rests by its nest,
//! flies at a ship in reach, strikes (a harpy with a sure shot shared
//! out on the ship as any blow; a dragon sows its teeth, which rise as
//! land creatures or a sea monster) and flies home. (The original's
//! dragon picks a free cell beside it, land near buildings first; here it
//! goes for ships as harpies do.)

use openac_formats::map::TILE_SIZE;
use openac_formats::{ObjectClass, ObjectRecord};

use super::{heading_towards, Scene};
use crate::assets::GameData;
use crate::objects::{self, Caches};

/// How far from its nest a creature looks for ships, in cells; how fast it
/// flies (world pixels a second); how long it rests between flights and
/// strikes. Placeholders.
const REACH: f32 = 8.0;
const SPEED: f32 = 110.0;
const REST: f32 = 8.0;
const STRIKE: f32 = 1.0;
/// The nest's delay before a new creature (0x41d4a0): 55500 game ticks
/// less 6000 per field 10, from 37 minutes down to one at field 9.
/// Harpies and Odysseus: his crews resist them.
const ODYSSEUS: u8 = 7;

/// What a creature is doing.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Flight {
    Rest(f32),
    Chase(u32),
    Strike(u32, f32),
    Home,
}

/// A creature out of its nest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Flyer {
    pub id: u32,
    pub nest: u32,
    pub flight: Flight,
    /// The sprite state and heading shown.
    pub shown: (u8, usize),
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
                        self.spawn_flyer(class, id, x, y, data, caches);
                    }
                }
                None => {}
            }
        }
    }

    fn spawn_flyer(
        &mut self,
        class: u32,
        nest: u32,
        x: f32,
        y: f32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let t = TILE_SIZE as f32;
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 11];
        fields[0] = class;
        fields[4] = (x / t) as u32;
        fields[5] = (y / t) as u32;
        fields[7] = 1;
        let rec = ObjectRecord {
            id,
            class_id: class,
            fields,
        };
        if let Ok(Some(mut obj)) = objects::resolve(data, caches, &rec) {
            obj.x = x;
            obj.y = y - t / 2.0;
            self.objects.push(obj);
            self.flyers.push(Flyer {
                id,
                nest,
                flight: Flight::Rest(REST),
                shown: (0, 1),
            });
        }
    }

    /// Moves the creatures out of their nests and lets them strike.
    pub(super) fn run_flyers(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.run_nests(dt, data, caches);
        // Fallen creatures leave; their nests wait to send the next.
        let alive = |s: &Scene, id: u32| {
            s.object_index(id)
                .is_some_and(|i| s.objects[i].combat.as_ref().is_some_and(|c| c.hull > 0.0))
        };
        let (kept, fallen): (Vec<Flyer>, Vec<Flyer>) = std::mem::take(&mut self.flyers)
            .into_iter()
            .partition(|f| alive(self, f.id));
        self.flyers = kept;
        let gone: Vec<u32> = fallen.iter().map(|f| f.nest).collect();
        for nest in gone {
            if let Some(n) = self.nests.iter_mut().find(|n| n.id == nest) {
                if n.left > 0 && n.wait.is_none() {
                    n.left -= 1;
                    n.wait = Some(n.delay);
                }
            }
        }
        let t = TILE_SIZE as f32;
        for k in 0..self.flyers.len() {
            let f = self.flyers[k].clone();
            let i = self.object_index(f.id).unwrap();
            let class = self.objects[i].record.class_id;
            let (x, y) = (self.objects[i].x, self.objects[i].y);
            let home = self
                .object_index(f.nest)
                .map(|n| (self.objects[n].x, self.objects[n].y - t / 2.0));
            let (mut flight, mut target_at) = (f.flight, None);
            match f.flight {
                Flight::Rest(left) => {
                    if left > dt {
                        flight = Flight::Rest(left - dt);
                    } else {
                        let (hx, hy) = home.unwrap_or((x, y));
                        flight = match self.flyer_prey(class, hx, hy, REACH * t) {
                            Some(prey) => Flight::Chase(prey),
                            None => Flight::Rest(REST),
                        };
                    }
                }
                Flight::Chase(prey) => match self.prey_at(class, prey) {
                    Some((px, py)) if (px - x).hypot(py - y) <= t / 2.0 => {
                        flight = Flight::Strike(prey, STRIKE);
                        self.flyer_strikes(class, prey, (x, y), f.nest, data, caches);
                    }
                    Some(p) => target_at = Some(p),
                    None => flight = Flight::Home,
                },
                Flight::Strike(prey, left) => {
                    flight = if left > dt {
                        Flight::Strike(prey, left - dt)
                    } else {
                        Flight::Home
                    };
                }
                Flight::Home => match home {
                    Some((hx, hy)) if (hx - x).hypot(hy - y) > 4.0 => target_at = Some((hx, hy)),
                    _ => flight = Flight::Rest(REST),
                },
            }
            let mut heading = f.shown.1;
            if let Some((tx, ty)) = target_at {
                let (dx, dy) = (tx - x, ty - y);
                let d = dx.hypot(dy).max(1e-3);
                let step = (SPEED * dt).min(d);
                self.objects[i].x += dx / d * step;
                self.objects[i].y += dy / d * step;
                heading = heading_towards(dx, dy);
            }
            let state = match flight {
                Flight::Rest(_) => 0,
                Flight::Strike(..) => 5,
                _ => 3,
            };
            if (state, heading) != f.shown {
                let name = objects::flyer_sequence(class, state, heading);
                if let Ok(frames) = objects::flyer_frames(data, caches, &name) {
                    if !frames.is_empty() {
                        self.objects[i].frames = frames;
                    }
                }
            }
            self.flyers[k].flight = flight;
            self.flyers[k].shown = (state, heading);
        }
    }

    /// The ship nearest `(x, y)` within `reach` a creature would go for:
    /// harpies want crews (not Odysseus's).
    fn flyer_prey(&self, class: u32, x: f32, y: f32, reach: f32) -> Option<u32> {
        self.objects
            .iter()
            .filter(|o| self.is_prey(class, o))
            .map(|o| (o.id, (o.x - x).hypot(o.y - y)))
            .filter(|p| p.1 <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|p| p.0)
    }

    fn is_prey(&self, class: u32, o: &crate::objects::MapObject) -> bool {
        o.owner.is_some()
            && o.owner != Some(crate::combat::MONSTERS)
            && o.ship
                .as_ref()
                .is_some_and(|s| class == 0x1f9 || (s.crew.count() > 0 && !s.has_hero(ODYSSEUS)))
    }

    fn prey_at(&self, class: u32, prey: u32) -> Option<(f32, f32)> {
        let o = &self.objects[self.object_index(prey)?];
        self.is_prey(class, o).then_some((o.x, o.y))
    }

    fn flyer_strikes(
        &mut self,
        class: u32,
        prey: u32,
        from: (f32, f32),
        nest: u32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(i) = self.object_index(prey) else {
            return;
        };
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        if class == 0x1f9 {
            // A dragon sows its teeth on the cell ahead of it (0x41fbd0,
            // state 5 at frame 16): on land they rise as a cyclops, a
            // minotaur or an amazon (message 0x1497), on water as a sea
            // monster (0x493780), both wild.
            let t = TILE_SIZE as f32;
            let ahead = (
                from.0 + (x - from.0).clamp(-t, t),
                from.1 + (y - from.1).clamp(-t, t),
            );
            let cell = ((ahead.0 / t).floor() as i32, (ahead.1 / t).floor() as i32);
            if self.nav.is_water(cell.0, cell.1) {
                let free = (-1..=1)
                    .flat_map(|dy| (-1..=1).map(move |dx| (cell.0 + dx, cell.1 + dy)))
                    .find(|&(cx, cy)| {
                        self.nav.is_water(cx, cy)
                            && !self.objects.iter().any(|o| {
                                o.ship.is_some()
                                    && ((o.x / t).floor() as i32, (o.y / t).floor() as i32)
                                        == (cx, cy)
                            })
                    });
                if let Some((cx, cy)) = free {
                    let wild = Some(crate::combat::MONSTERS);
                    let _ =
                        self.spawn_hazard(crate::magic::Spell::Monster, cx, cy, wild, data, caches);
                }
            } else {
                let lair = 0x46 + self.rng.next() % 3;
                self.spawn_walker(nest, lair, ahead.0, ahead.1, data, caches);
            }
            return;
        }
        // A harpy's strike (0x41fbd0, state 5 at frame 7): a sure shot,
        // drawn as L, carrying 20 x item 4's level-1 blow (the A x 15
        // cache's copy, 0x5c73dc), shared out on the ship as any blow.
        let blow = crate::combat::hull_damage_at(4, 1) * 20.0;
        let o = &self.objects[i];
        let (Some(s), Some(c)) = (&o.ship, &o.combat) else {
            return;
        };
        let armour = crate::combat::armour_factor(s.armour);
        let loads = (s.cargo / 100.0).ceil().max(0.0) as u32;
        let damage = blow * armour * super::battle::ship_hull_share(s, c);
        let wounds = blow
            * 15.0
            * armour
            * crate::combat::crew_share(
                s.kind,
                s.armour,
                s.ram,
                c.weapons.len(),
                loads,
                s.crew.count(),
            );
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
            },
            data,
            caches,
        );
    }
}
