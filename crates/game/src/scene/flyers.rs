//! Harpies and dragons (HarpyClassTy, 0x4207b0): each nest (0x41d4a0)
//! sends out its creature when the mission starts and, while it has
//! creatures left (record field 9), a new one some time after the last
//! fell (55500 ticks less 6000 per field 10). The creature keeps to the
//! nest's range (fields 7 and 8: so many cells either way) and goes
//! through the original's states (0x41f680, a tick at a time 0x41fbd0):
//! it rests on its nest for up to 1100 - 100 x field 11 ticks (a dragon
//! only takes off while someone is in its range, 0x41ef00), takes off,
//! and flies legs of up to 6 cells (0x41e840: a cell picked at random
//! within the round mask at 0x595a30, weighted 1 + 10 x the marines aboard
//! the ships beside it), turning first, a cell per cycle of its flying
//! picture. At each cell it looks around (0x41efb0): a harpy strikes a
//! ship beside it with a crew (a sure shot shared out on the ship as any
//! blow); a dragon, with someone beside it, sows its teeth on a free cell
//! one time in three (they rise as land creatures or a sea monster). Then
//! it flies home, lands and rests again.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::{ObjectClass, ObjectRecord};

use super::{heading_towards, Scene};
use crate::assets::GameData;
use crate::objects::{self, Caches};

/// Rest on the nest: up to `REST_TICKS - REST_STEP x` field 11 ticks.
const REST_TICKS: u32 = 1100;
const REST_STEP: u32 = 100;
/// Ticks to turn by one heading (the turning picture, 8 frames a heading
/// at a frame a tick).
const TURN_TICKS: u32 = 8;
/// The frame of the striking picture at which the blow falls: 7 for a
/// harpy, 16 for a dragon (0x41fbd0).
const HARPY_BLOW_FRAME: u32 = 7;
const DRAGON_BLOW_FRAME: u32 = 16;
/// The cells a leg may end on, around the creature (0x595a30, 13 x 13).
const LEG_MASK: [u16; 13] = [
    0b0001111111000,
    0b0011111111100,
    0b0111111111110,
    0b1111111111111,
    0b1111111111111,
    0b1111111111111,
    0b1111110111111,
    0b1111111111111,
    0b1111111111111,
    0b1111111111111,
    0b0111111111110,
    0b0011111111100,
    0b0001111111000,
];
/// Harpies and Odysseus: his crews resist them.
const ODYSSEUS: u8 = 7;

/// What a creature is doing.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Flight {
    /// On the nest, seconds left.
    Rest(f32),
    /// (Older saves.) Flown as a new leg.
    Chase(u32),
    /// (Older saves.) Flown home.
    Strike(u32, f32),
    /// (Older saves.) Flown home.
    Home,
    TakeOff(f32),
    /// Flying a leg to a cell; home when `home`.
    Fly {
        to: (i32, i32),
        home: bool,
    },
    /// Striking from its cell at a ship (a dragon: sowing on a cell),
    /// seconds into the picture.
    Striking {
        prey: u32,
        at: (i32, i32),
        elapsed: f32,
        struck: bool,
    },
    Land(f32),
}

/// A step from one cell to the next.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Step {
    from: (f32, f32),
    to: (f32, f32),
    heading: usize,
    elapsed: f32,
}

/// A creature out of its nest.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Flyer {
    pub id: u32,
    pub nest: u32,
    pub flight: Flight,
    /// The sprite state and heading shown.
    pub shown: (u8, usize),
    #[serde(default)]
    pub step: Option<Step>,
    /// Seconds spent turning towards the step's heading.
    #[serde(default)]
    pub turning: f32,
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
            let rest = self.flyer_rest(nest);
            self.flyers.push(Flyer {
                id,
                nest,
                flight: Flight::Rest(rest),
                shown: (0, 1),
                step: None,
                turning: 0.0,
            });
        }
    }

    /// A rest on nest `nest`'s creature: up to 1100 - 100 x field 11
    /// ticks (0x41f680 state 1).
    fn flyer_rest(&mut self, nest: u32) -> f32 {
        let f11 = self
            .object_index(nest)
            .map_or(0, |n| self.objects[n].record.field(11).unwrap_or(0));
        let most = REST_TICKS.saturating_sub(REST_STEP * f11).max(1);
        crate::clock::ticks(self.rng.next() % most)
    }

    /// The nest's cell and how far either way its creature keeps (fields
    /// 7 and 8).
    fn nest_range(&self, nest: u32) -> Option<((i32, i32), (i32, i32))> {
        let n = &self.objects[self.object_index(nest)?];
        let t = TILE_SIZE as f32;
        let cell = (
            (n.x / t).floor() as i32,
            ((n.y - t / 2.0) / t).floor() as i32,
        );
        let f = |k| n.record.field(k).unwrap_or(5) as i32;
        Some((cell, (f(7), f(8))))
    }

    /// How long a flyer's picture takes, a frame a tick (the delays at
    /// 0x595d10 and 0x595d30 are 0 or 1 but for a harpy's waiting).
    fn picture_time(
        &self,
        class: u32,
        state: u8,
        heading: usize,
        data: &GameData,
        caches: &mut Caches,
    ) -> f32 {
        let name = objects::flyer_sequence(class, state, heading);
        let n = objects::sequence_frames(data, caches, &name).map_or(15, |f| f.len().max(1));
        crate::clock::ticks(n as u32)
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
            let cell = ((x / t).floor() as i32, (y / t).floor() as i32);
            let Some((home, _)) = self.nest_range(f.nest) else {
                continue;
            };
            let mut heading = f.shown.1;
            let (mut step, mut turning) = (f.step, f.turning);
            let flight = match f.flight {
                Flight::Rest(left) if left > dt => Flight::Rest(left - dt),
                Flight::Rest(_) => {
                    if class == 0x1f9 && !self.someone_in_range(f.nest) {
                        Flight::Rest(self.flyer_rest(f.nest))
                    } else {
                        Flight::TakeOff(self.picture_time(class, 2, heading, data, caches))
                    }
                }
                Flight::TakeOff(left) if left > dt => Flight::TakeOff(left - dt),
                Flight::TakeOff(_) | Flight::Chase(_) => self.flyer_leg(class, f.nest, cell),
                Flight::Strike(..) | Flight::Home => Flight::Fly {
                    to: home,
                    home: true,
                },
                Flight::Fly {
                    to,
                    home: going_home,
                } => {
                    if step.is_none() && cell != to {
                        let next = (
                            cell.0 + (to.0 - cell.0).signum(),
                            cell.1 + (to.1 - cell.1).signum(),
                        );
                        let centre =
                            |c: (i32, i32)| (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
                        let (a, b) = (centre(cell), centre(next));
                        step = Some(Step {
                            from: (x, y),
                            to: b,
                            heading: heading_towards(b.0 - a.0, b.1 - a.1),
                            elapsed: 0.0,
                        });
                        turning = 0.0;
                    }
                    match step.as_mut() {
                        Some(s) if s.heading != heading => {
                            // Turning first, a heading at a time.
                            turning += dt;
                            while turning >= crate::clock::ticks(TURN_TICKS) && s.heading != heading
                            {
                                turning -= crate::clock::ticks(TURN_TICKS);
                                let d = (s.heading + 8 - heading) % 8;
                                heading = if d <= 4 {
                                    (heading + 1) % 8
                                } else {
                                    (heading + 7) % 8
                                };
                            }
                            Flight::Fly {
                                to,
                                home: going_home,
                            }
                        }
                        Some(s) => {
                            let cycle = self.picture_time(class, 3, heading, data, caches);
                            s.elapsed += dt;
                            let p = (s.elapsed / cycle).min(1.0);
                            self.objects[i].x = s.from.0 + (s.to.0 - s.from.0) * p;
                            self.objects[i].y = s.from.1 + (s.to.1 - s.from.1) * p;
                            if p < 1.0 {
                                Flight::Fly {
                                    to,
                                    home: going_home,
                                }
                            } else {
                                step = None;
                                let now = (
                                    (self.objects[i].x / t).floor() as i32,
                                    (self.objects[i].y / t).floor() as i32,
                                );
                                self.flyer_arrives(class, f.nest, now, to, going_home)
                            }
                        }
                        None => self.flyer_arrives(class, f.nest, cell, to, going_home),
                    }
                }
                Flight::Striking {
                    prey,
                    at,
                    elapsed,
                    mut struck,
                } => {
                    let elapsed = elapsed + dt;
                    let blow = if class == 0x1f9 {
                        DRAGON_BLOW_FRAME
                    } else {
                        HARPY_BLOW_FRAME
                    };
                    if !struck && elapsed >= crate::clock::ticks(blow) {
                        struck = true;
                        self.flyer_strikes(class, prey, at, (x, y), f.nest, data, caches);
                    }
                    if elapsed >= self.picture_time(class, 5, heading, data, caches) {
                        Flight::Fly {
                            to: home,
                            home: true,
                        }
                    } else {
                        Flight::Striking {
                            prey,
                            at,
                            elapsed,
                            struck,
                        }
                    }
                }
                Flight::Land(left) if left > dt => Flight::Land(left - dt),
                Flight::Land(_) => Flight::Rest(self.flyer_rest(f.nest)),
            };
            if let Flight::Striking { at, .. } = flight {
                if at != cell {
                    heading = heading_towards((at.0 - cell.0) as f32, (at.1 - cell.1) as f32);
                }
            }
            let state = match flight {
                Flight::Rest(_) => 0,
                Flight::TakeOff(_) => 2,
                Flight::Striking { .. } => 5,
                Flight::Land(_) => 6,
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
            let fl = &mut self.flyers[k];
            fl.flight = flight;
            fl.shown = (state, heading);
            fl.step = step;
            fl.turning = turning;
        }
    }

    /// A flyer of `class` from nest `nest` reaches cell `cell` on a leg to
    /// `to`: it strikes what it finds beside it (not on its way home), and
    /// at the leg's end flies another, or lands at home.
    fn flyer_arrives(
        &mut self,
        class: u32,
        nest: u32,
        cell: (i32, i32),
        to: (i32, i32),
        home: bool,
    ) -> Flight {
        if !home {
            if let Some((prey, at)) = self.flyer_target(class, cell) {
                return Flight::Striking {
                    prey,
                    at,
                    elapsed: 0.0,
                    struck: false,
                };
            }
        }
        if cell != to {
            return Flight::Fly { to, home };
        }
        if home {
            // The landing picture: 35 frames for a harpy, 20 for a dragon.
            return Flight::Land(crate::clock::ticks(if class == 0x1f9 { 20 } else { 35 }));
        }
        self.flyer_leg(class, nest, cell)
    }

    /// A new leg from `cell` (0x41e840): a cell within the mask around it
    /// and the nest's range, picked at random, weighted 1 + 10 x the
    /// marines (each kind aboard counting one more) on the ships beside it.
    fn flyer_leg(&mut self, class: u32, nest: u32, cell: (i32, i32)) -> Flight {
        let Some((home, (rx, ry))) = self.nest_range(nest) else {
            return Flight::Rest(1.0);
        };
        let t = TILE_SIZE as f32;
        let mut ships: Vec<((i32, i32), u32)> = Vec::new();
        for o in &self.objects {
            if !self.is_prey(class, o) {
                continue;
            }
            let s = o.ship.as_ref().unwrap();
            let marines: u32 = s
                .crew
                .squads
                .iter()
                .filter(|&&n| n > 0)
                .map(|&n| 1 + n)
                .sum();
            ships.push((
                ((o.x / t).floor() as i32, (o.y / t).floor() as i32),
                marines,
            ));
        }
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let mut cells: Vec<((i32, i32), u32)> = Vec::new();
        for (row, mask) in LEG_MASK.iter().enumerate() {
            for col in 0..13 {
                if mask >> (12 - col) & 1 == 0 {
                    continue;
                }
                let c = (cell.0 + col - 6, cell.1 + row as i32 - 6);
                if c.0 < 0 || c.1 < 0 || c.0 >= w || c.1 >= h {
                    continue;
                }
                if (c.0 - home.0).abs() > rx || (c.1 - home.1).abs() > ry {
                    continue;
                }
                let crews: u32 = ships
                    .iter()
                    .filter(|(sc, _)| {
                        *sc != c && (sc.0 - c.0).abs() <= 1 && (sc.1 - c.1).abs() <= 1
                    })
                    .map(|&(_, m)| m)
                    .sum();
                cells.push((c, 1 + 10 * crews));
            }
        }
        let total: u32 = cells.iter().map(|c| c.1).sum();
        if total == 0 {
            return Flight::Fly {
                to: home,
                home: true,
            };
        }
        let mut pick = self.rng.next() % total;
        for (c, weight) in cells {
            if pick < weight {
                return Flight::Fly { to: c, home: false };
            }
            pick -= weight;
        }
        Flight::Fly {
            to: home,
            home: true,
        }
    }

    /// Whether a dragon's nest has someone in its range (0x41ef00).
    fn someone_in_range(&self, nest: u32) -> bool {
        let Some((home, (rx, ry))) = self.nest_range(nest) else {
            return false;
        };
        let t = TILE_SIZE as f32;
        self.objects.iter().any(|o| {
            o.owner.is_some()
                && o.owner != Some(crate::combat::MONSTERS)
                && ((o.x / t).floor() as i32 - home.0).abs() <= rx
                && ((o.y / t).floor() as i32 - home.1).abs() <= ry
        })
    }

    /// What a flyer at `cell` strikes (0x41efb0, 0x41f1c0): a harpy a ship
    /// beside it with a crew; a dragon, with someone beside it, one time in
    /// three a free cell beside it (land by buildings first).
    fn flyer_target(&mut self, class: u32, cell: (i32, i32)) -> Option<(u32, (i32, i32))> {
        let t = TILE_SIZE as f32;
        let at =
            |o: &crate::objects::MapObject| ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let beside = |c: (i32, i32)| (c.0 - cell.0).abs() <= 1 && (c.1 - cell.1).abs() <= 1;
        if class != 0x1f9 {
            return self
                .objects
                .iter()
                .filter(|o| self.is_prey(class, o) && beside(at(o)))
                .map(|o| (o.id, at(o)))
                .next();
        }
        let near: Vec<&crate::objects::MapObject> = self
            .objects
            .iter()
            .filter(|o| {
                o.owner.is_some() && o.owner != Some(crate::combat::MONSTERS) && o.combat.is_some()
            })
            .filter(|o| match (o.ship.is_none(), o.cell) {
                (true, Some((cx, cy))) => {
                    let n = super::trade::building_cells(o);
                    cx - 1 <= cell.0 + 1
                        && cell.0 - 1 <= cx + n
                        && cy - 1 <= cell.1 + 1
                        && cell.1 - 1 <= cy + n
                }
                _ => beside(at(o)),
            })
            .collect();
        if near.is_empty() {
            return None;
        }
        let by_building = near.iter().any(|o| o.ship.is_none());
        let taken = |c: (i32, i32)| {
            self.objects
                .iter()
                .any(|o| match (o.ship.is_none(), o.cell) {
                    (true, Some((cx, cy))) => {
                        let n = super::trade::building_cells(o);
                        (cx..cx + n).contains(&c.0) && (cy..cy + n).contains(&c.1)
                    }
                    _ => o.combat.is_some() && at(o) == c,
                })
        };
        let mut cells: Vec<((i32, i32), u32)> = Vec::new();
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = (cell.0 + dx, cell.1 + dy);
                if (dx, dy) == (0, 0) || c.0 < 0 || c.1 < 0 || taken(c) {
                    continue;
                }
                let land = !self.nav.is_water(c.0, c.1);
                cells.push((c, if land && by_building { 50 } else { 1 }));
            }
        }
        let total: u32 = cells.iter().map(|c| c.1).sum();
        if total == 0 || self.rng.next() % 3 != 0 {
            return None;
        }
        let mut pick = self.rng.next() % total;
        for (c, w) in cells {
            if pick < w {
                return Some((0, c));
            }
            pick -= w;
        }
        None
    }

    fn is_prey(&self, class: u32, o: &crate::objects::MapObject) -> bool {
        o.owner.is_some()
            && o.owner != Some(crate::combat::MONSTERS)
            && o.ship
                .as_ref()
                .is_some_and(|s| class == 0x1f9 || (s.crew.count() > 0 && !s.has_hero(ODYSSEUS)))
    }

    #[allow(clippy::too_many_arguments)]
    fn flyer_strikes(
        &mut self,
        class: u32,
        prey: u32,
        at: (i32, i32),
        from: (f32, f32),
        nest: u32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        if class == 0x1f9 {
            // A dragon sows its teeth on its chosen cell (0x41fbd0, state
            // 5 at frame 16): on land they rise as a cyclops, a minotaur or
            // an amazon (message 0x1497), on water as a sea monster
            // (0x493780), both wild.
            let t = TILE_SIZE as f32;
            if self.nav.is_water(at.0, at.1) {
                let wild = Some(crate::combat::MONSTERS);
                let _ =
                    self.spawn_hazard(crate::magic::Spell::Monster, at.0, at.1, wild, data, caches);
            } else {
                let lair = 0x46 + self.rng.next() % 3;
                let (x, y) = (at.0 as f32 * t + t / 2.0, at.1 as f32 * t + t / 2.0);
                self.spawn_walker(nest, lair, x, y, data, caches);
            }
            return;
        }
        let Some(i) = self.object_index(prey) else {
            return;
        };
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
