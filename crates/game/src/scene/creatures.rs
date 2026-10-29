//! What the wild creatures do (the manual's "Monsters"): the Gorgon turns
//! ships in her area to stone, sirens' songs draw ships in, and harpies
//! wound crews (`flyers`). Lairs, nests and sea monsters fight with the weapons given
//! them in `combat::creature_weapons`.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;

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

/// A Gorgon's cycle: seconds until her next stare, and the stares left in
/// this rising (0 resting), and how many she has made.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct GorgonCycle {
    next: f32,
    left: u32,
    made: u32,
    /// Seconds she stays up sinking after her last stare.
    shown: f32,
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
            if let Some(v) = self.siren_shown.get_mut(&id) {
                *v = (*v - dt).max(0.0);
            }
            let left = self.siren_songs.entry(id).or_insert(first);
            *left -= dt;
            if *left > 0.0 {
                continue;
            }
            // The next song: the rest of her picture, a dive, a surfacing.
            *left = crate::clock::ticks(SONG_CYCLE_FRAMES - 45 + 250 + self.rng.next() % 751 + 45);
            self.siren_shown
                .insert(id, crate::clock::ticks(SONG_CYCLE_FRAMES - 45));
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
                let blow = 300.0 / 15.0 * if argus { 0.25 } else { 1.0 };
                self.share_blow(id, blow * blows as f32, data, caches);
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
                shown: 0.0,
            });
            g.next -= dt;
            g.shown = (g.shown - dt).max(0.0);
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
                if let Some(c) = self.objects[i].combat.as_mut() {
                    c.stone += ticks(150);
                }
                let hurt = blow * (100 - mirror) as f32 / 100.0;
                self.share_blow(*ship, hurt, data, caches);
            }
            if !caught.is_empty() {
                self.messages
                    .push("The Gorgon's stare turns ships to stone".into());
            }
            g.made += 1;
            g.left -= 1;
            if g.left == 0 {
                g.shown = ticks(28 - 13 + 30 + 21);
            }
            g.next = if g.left > 0 {
                ticks(28)
            } else {
                // Sinking, resting, rising, and into the first stare.
                ticks(28 - 13 + 30 + 21) + rest(&mut self.rng)
            };
            self.gorgons.insert(id, g);
        }
    }

    /// Whether a siren or the Gorgon is under the water now: a siren from
    /// the end of her picture until 45 frames before her next song
    /// (0x403650 hides her picture at state 0), the Gorgon from the end of
    /// her sinking until 43 ticks before her next stare.
    pub fn creature_hidden(&self, id: u32) -> bool {
        let ticks = crate::clock::ticks;
        if let Some(&left) = self.siren_songs.get(&id) {
            let up = self.siren_shown.get(&id).copied().unwrap_or(0.0) > 0.0;
            return !up && left > ticks(45);
        }
        if let Some(g) = self.gorgons.get(&id) {
            return g.left == 0 && g.shown <= 0.0 && g.next > ticks(43);
        }
        self.sea_monster_hidden(id)
    }
}
