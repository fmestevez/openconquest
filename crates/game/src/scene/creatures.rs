//! What the wild creatures do (the manual's "Monsters"): the Gorgon turns
//! ships in her area to stone, sirens' songs draw ships in, and harpies
//! wound crews (`flyers`). Lairs, nests and sea monsters fight with the weapons given
//! them in `combat::creature_weapons`.

use openac_formats::map::TILE_SIZE;
use openac_formats::ObjectClass;

use super::{Caches, GameData, Scene};
use crate::objects::MapObject;

/// Seconds between the Gorgon's stares, and ship-seconds of stone she has
/// to share among the ships in her area each time. Placeholders.
pub const GORGON_PERIOD: f32 = 8.0;
pub const GORGON_POWER: f32 = 12.0;
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
/// The Gorgon's area when the record gives none, in cells.
const DEFAULT_AREA: u32 = 5;

/// A world rectangle: x0, y0, x1, y1.
type Rect = (f32, f32, f32, f32);

/// Treasure kind of the mirror shield of Athena, and the heroes who resist
/// the Gorgon and sirens.
const MIRROR_SHIELD: u8 = 14;
const ODYSSEUS: u8 = 7;
const PERSEUS: u8 = 8;

impl Scene {
    /// The world rectangle of a Gorgon's area: fields 7 and 8 are
    /// its size in cells, centred on the creature.
    fn creature_area(o: &MapObject) -> Rect {
        let t = TILE_SIZE as f32;
        let size = |i| match o.record.field(i).unwrap_or(0) {
            0 => DEFAULT_AREA,
            n => n,
        } as f32
            * t;
        let (w, h) = (size(7), size(8));
        (o.x - w / 2.0, o.y - h / 2.0, o.x + w / 2.0, o.y + h / 2.0)
    }

    pub(super) fn run_creatures(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.creature_clock += dt;
        // Stone wears off.
        for c in self.objects.iter_mut().filter_map(|o| o.combat.as_mut()) {
            c.stone = (c.stone - dt).max(0.0);
        }
        let inside = |o: &MapObject, r: Rect| {
            o.ship.is_some() && (r.0..=r.2).contains(&o.x) && (r.1..=r.3).contains(&o.y)
        };
        let warded = |o: &MapObject| o.combat.as_ref().is_some_and(|c| c.protect_magic > 0.0);
        let gorgons: Vec<Rect> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Monster(0x1fa))
            .map(Self::creature_area)
            .collect();
        // The Gorgon's stare: her power is shared among the ships in her
        // area, so a group suffers less each.
        let stare = self.creature_clock % GORGON_PERIOD < dt;
        if stare {
            for r in gorgons {
                let caught: Vec<usize> = self
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| inside(o, r) && !warded(o))
                    .filter(|(_, o)| {
                        let s = o.ship.as_ref().unwrap();
                        !s.has_hero(PERSEUS) && !s.has_treasure(MIRROR_SHIELD)
                    })
                    .map(|(i, _)| i)
                    .collect();
                if caught.is_empty() {
                    continue;
                }
                let each = GORGON_POWER / caught.len() as f32;
                for i in caught {
                    if let Some(c) = self.objects[i].combat.as_mut() {
                        c.stone = c.stone.max(each);
                    }
                }
                self.messages
                    .push("The Gorgon's stare turns ships to stone".into());
            }
        }
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
}
