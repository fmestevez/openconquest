//! What the wild creatures do (the manual's "Monsters"): the Gorgon turns
//! ships in her area to stone, sirens' songs draw ships in, and harpies
//! wound crews (`flyers`). Lairs, nests and sea monsters fight with the weapons given
//! them in `combat::creature_weapons`.

use openac_formats::map::TILE_SIZE;
use openac_formats::ObjectClass;

use super::{Caches, GameData, Scene};

/// The Gorgon (MedusaClassTy, gorgona.cpp): she rests 200-500 ticks, rises
/// (`GORG1`, 30 frames a tick), stares 1-3 times (`GORG2`, 28 frames, at
/// frame 13) over the 3, 5 then 7 cells square about her, and sinks
/// (`GORG3`, 30; `GORG4`, 21; 0x401f70, 0x4019d0). Each stare turns every
/// ship there to stone for 150 ticks more and strikes it with 88000 shared
/// out (22000 from five ships; message 0x158bf, 0x4dc630); Perseus and a
/// magic shield keep it off, the mirror shield cuts the blow by its
/// share.
const GORGON_STARE: [u32; 3] = [1, 2, 3];
/// Seconds between a sea monster's bites and its steps. Estimates.
const MONSTER_BITE_SECONDS: f32 = 2.0;
const MONSTER_STEP_SECONDS: f32 = 0.75;

/// A Gorgon's cycle: seconds until her next stare, and the stares left in
/// this rising (0 resting), and how many she has made.
#[derive(Debug, Clone, Copy)]
pub struct GorgonCycle {
    next: f32,
    left: u32,
    made: u32,
}
/// A siren (SirenaClassTy, sirena.cpp) hides 250-1000 ticks (0x403650),
/// surfaces and sings once (her `SIRENA`, 163 frames a tick, at frame 45;
/// 0x403ba0), then dives. Her song charms every ship within 7 cells (but
/// Odysseus's) for 750 ticks more (0x4a7390): it sails for her cell, no
/// faster than 7000 units, heading anew every 30 ticks, and adds her share
/// (88 / the ships charmed, 22 from five) each tick, taking a blow of 300
/// at each thousand (0x4a74c0; a fourth with Argus, none shielded from
/// magic).
pub const SONG_CYCLE_FRAMES: u32 = 163;
pub const CHARMED_SPEED: f32 = 7000.0 / 6000.0 / crate::clock::TICK;

/// A ship under a siren's song.
#[derive(Debug, Clone)]
pub struct Charm {
    pub cell: (i32, i32),
    share: u32,
    acc: u32,
    /// Seconds of song left, and until it heads for her anew.
    left: f32,
    replan: f32,
    clock: f32,
}

/// Treasure kind of the mirror shield of Athena, and the heroes who resist
/// the Gorgon and sirens.
const MIRROR_SHIELD: u8 = 14;
const ODYSSEUS: u8 = 7;
const PERSEUS: u8 = 8;

impl Scene {
    pub(super) fn run_creatures(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.creature_clock += dt;
        // Stone wears off.
        for c in self.objects.iter_mut().filter_map(|o| o.combat.as_mut()) {
            c.stone = (c.stone - dt).max(0.0);
        }
        self.run_gorgons(dt, data, caches);
        self.run_sea_monsters(dt, data, caches);
        self.run_sirens(dt, data, caches);
    }

    /// Sirens sing; their songs draw ships in and wear them down.
    fn run_sirens(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let tick = crate::clock::TICK;
        let sirens: Vec<(u32, (i32, i32))> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Monster(0x1fd))
            .map(|o| (o.id, ((o.x / t).floor() as i32, (o.y / t).floor() as i32)))
            .collect();
        for (id, cell) in sirens {
            let first = crate::clock::ticks(250 + self.rng.next() % 751 + 45);
            let left = self.siren_songs.entry(id).or_insert(first);
            *left -= dt;
            if *left > 0.0 {
                continue;
            }
            // The next song: the rest of her picture, a dive, a surfacing.
            *left = crate::clock::ticks(SONG_CYCLE_FRAMES - 45 + 250 + self.rng.next() % 751 + 45);
            let heard: Vec<u32> = self
                .objects
                .iter()
                .filter(|o| {
                    let (ox, oy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
                    (ox - cell.0).abs() <= 7
                        && (oy - cell.1).abs() <= 7
                        && o.ship.as_ref().is_some_and(|s| !s.has_hero(ODYSSEUS))
                })
                .map(|o| o.id)
                .collect();
            if heard.is_empty() {
                continue;
            }
            let share = if heard.len() < 5 {
                88 / heard.len() as u32
            } else {
                22
            };
            for ship in heard {
                let c = self.charmed.entry(ship).or_insert(Charm {
                    cell,
                    share,
                    acc: 0,
                    left: 0.0,
                    replan: 0.0,
                    clock: 0.0,
                });
                (c.cell, c.share) = (cell, share);
                c.left += crate::clock::ticks(750);
            }
            self.messages.push("A siren sings".into());
        }
        // The charmed: a tick at a time.
        let ids: Vec<u32> = self.charmed.keys().copied().collect();
        for id in ids {
            let Some(i) = self.object_index(id) else {
                self.charmed.remove(&id);
                continue;
            };
            let mut c = self.charmed.remove(&id).unwrap();
            c.left -= dt;
            c.replan -= dt;
            c.clock += dt;
            let mut blows = 0;
            while c.clock >= tick {
                c.clock -= tick;
                c.acc += c.share;
                if c.acc >= 1000 {
                    c.acc %= 1000;
                    blows += 1;
                }
            }
            if c.replan <= 0.0 {
                c.replan = crate::clock::ticks(30);
                let (x, y) = (c.cell.0 as f32 * t + t / 2.0, c.cell.1 as f32 * t + t / 2.0);
                if let Some(path) = self.plan_route(i, x, y) {
                    if let Some(s) = self.objects[i].ship.as_mut() {
                        s.path = path;
                        s.task = None;
                    }
                }
            }
            let o = &self.objects[i];
            let shielded = o.combat.as_ref().is_some_and(|c| c.protect_magic > 0.0);
            if blows > 0 && !shielded {
                let argus = o
                    .ship
                    .as_ref()
                    .is_some_and(|s| s.has_hero(crate::objects::ARGUS));
                let share = match (&o.ship, &o.combat) {
                    (Some(s), Some(cb)) => super::battle::ship_hull_share(s, cb),
                    _ => 1.0,
                };
                let blow = 300.0 / 15.0 * if argus { 0.25 } else { 1.0 } * share;
                self.hit(id, blow * blows as f32, data, caches);
            }
            if c.left > 0.0 && self.object_index(id).is_some() {
                self.charmed.insert(id, c);
            }
        }
    }

    /// The Gorgons rest, rise, stare and sink.
    fn run_gorgons(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let ticks = crate::clock::ticks;
        let gorgons: Vec<(u32, (i32, i32))> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Monster(0x1fa))
            .map(|o| (o.id, ((o.x / t).floor() as i32, (o.y / t).floor() as i32)))
            .collect();
        for (id, cell) in gorgons {
            let rest = |rng: &mut crate::combat::Rng| ticks(200 + rng.next() % 301 + 30 + 13);
            let first = rest(&mut self.rng);
            let mut g = *self.gorgons.entry(id).or_insert(GorgonCycle {
                next: first,
                left: 0,
                made: 0,
            });
            g.next -= dt;
            if g.next > 0.0 {
                self.gorgons.insert(id, g);
                continue;
            }
            if g.left == 0 {
                g.left = 1 + self.rng.next() % 3;
                g.made = 0;
            }
            // A stare.
            let half = GORGON_STARE[(g.made as usize).min(2)] as i32;
            let caught: Vec<u32> = self
                .objects
                .iter()
                .filter(|o| {
                    let (ox, oy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
                    (ox - cell.0).abs() <= half && (oy - cell.1).abs() <= half && o.ship.is_some()
                })
                .map(|o| o.id)
                .collect();
            let blow = if caught.len() < 5 {
                88_000.0 / caught.len().max(1) as f32
            } else {
                22_000.0
            } / 15.0;
            for ship in &caught {
                let Some(i) = self.object_index(*ship) else {
                    continue;
                };
                let o = &self.objects[i];
                let s = o.ship.as_ref().unwrap();
                let warded = o.combat.as_ref().is_some_and(|c| c.protect_magic > 0.0);
                if s.has_hero(PERSEUS) || warded {
                    continue;
                }
                let mirror = s
                    .treasures
                    .iter()
                    .find(|t| t.kind == MIRROR_SHIELD)
                    .map_or(0, |t| if t.amount > 0 { t.amount.min(100) } else { 100 });
                let share = match (&o.ship, &o.combat) {
                    (Some(s), Some(c)) => super::battle::ship_hull_share(s, c),
                    _ => 1.0,
                };
                if let Some(c) = self.objects[i].combat.as_mut() {
                    c.stone += ticks(150);
                }
                let hurt = blow * share * (100 - mirror) as f32 / 100.0;
                self.hit(*ship, hurt, data, caches);
            }
            if !caught.is_empty() {
                self.messages
                    .push("The Gorgon's stare turns ships to stone".into());
            }
            g.made += 1;
            g.left -= 1;
            g.next = if g.left > 0 {
                ticks(28)
            } else {
                // Sinking, resting, rising, and into the first stare.
                ticks(28 - 13 + 30 + 21) + rest(&mut self.rng)
            };
            self.gorgons.insert(id, g);
        }
    }

    /// Sea monsters (UMonsterClassTy, umonster.cpp) bite the enemy ships
    /// beside them: each bite (message 0xf166, 0x490830) strikes 37500 x
    /// their strength left in percent / 100, through the hull's share,
    /// three quarters off with Argus, to a fifth under a magic shield
    /// (0x4dc630), and costs them a twentieth of their strength. They go
    /// for the nearest within 7 cells a cell at a time. How often they
    /// bite and how fast they go are estimates.
    fn run_sea_monsters(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let cell = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
        let monsters: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Monster(0x1fb))
            .map(|o| o.id)
            .collect();
        for id in monsters {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let clock = self.sea_monsters.entry(id).or_insert((0.0, 0.0));
            clock.0 -= dt;
            clock.1 -= dt;
            let (bite_ready, step_ready) = (clock.0 <= 0.0, clock.1 <= 0.0);
            let me = &self.objects[i];
            let here = cell(me.x, me.y);
            // The nearest enemy ship within 7 cells.
            let prey = self
                .objects
                .iter()
                .filter(|o| o.ship.is_some() && Self::are_enemies(me, o))
                .filter(|o| {
                    !o.ship
                        .as_ref()
                        .is_some_and(|s| s.has_hero(crate::objects::ORPHEUS))
                })
                .map(|o| {
                    let c = cell(o.x, o.y);
                    (o.id, c, (c.0 - here.0).abs().max((c.1 - here.1).abs()))
                })
                .filter(|p| p.2 <= 7)
                .min_by_key(|p| p.2);
            let Some((target, at, d)) = prey else {
                continue;
            };
            if d <= 1 {
                if !bite_ready {
                    continue;
                }
                self.sea_monsters.get_mut(&id).unwrap().0 = MONSTER_BITE_SECONDS;
                let c = self.objects[i].combat.as_mut().unwrap();
                let strength = (c.hull / c.max_hull.max(1.0)).clamp(0.0, 1.0);
                c.hull -= c.max_hull * 0.05;
                let Some(ti) = self.object_index(target) else {
                    continue;
                };
                let o = &self.objects[ti];
                let (Some(s), Some(cb)) = (&o.ship, &o.combat) else {
                    continue;
                };
                let mut blow = 37_500.0 * strength / 15.0 * super::battle::ship_hull_share(s, cb);
                if s.has_hero(crate::objects::ARGUS) {
                    blow *= 0.25;
                }
                if cb.protect_magic > 0.0 {
                    blow *= 0.2;
                }
                let (mx, my) = (self.objects[i].x, self.objects[i].y);
                self.sounds.push(("htk1", mx, my));
                self.hit(target, blow, data, caches);
                if self.objects[i]
                    .combat
                    .as_ref()
                    .is_some_and(|c| c.hull <= 0.0)
                {
                    self.hit(id, 0.0, data, caches);
                }
            } else if step_ready {
                self.sea_monsters.get_mut(&id).unwrap().1 = MONSTER_STEP_SECONDS;
                let next = (
                    here.0 + (at.0 - here.0).signum(),
                    here.1 + (at.1 - here.1).signum(),
                );
                let free = self.nav.is_water(next.0, next.1)
                    && !self
                        .objects
                        .iter()
                        .any(|o| o.ship.is_some() && cell(o.x, o.y) == next);
                if free {
                    let o = &mut self.objects[i];
                    o.x = next.0 as f32 * t + t / 2.0;
                    o.y = next.1 as f32 * t + t / 2.0;
                }
            }
        }
    }
}
