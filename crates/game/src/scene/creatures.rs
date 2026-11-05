//! What the wild creatures do (the manual's "Monsters"): the Gorgon turns
//! ships in her area to stone, sirens' songs draw ships in, and harpies
//! wound crews (`flyers`). Lairs, nests and sea monsters fight with the weapons given
//! them in `combat::creature_weapons`.

use openconquest_formats::map::TILE_SIZE;

use super::{Caches, GameData, Scene};

/// A ship under a siren's song.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Charm {
    pub cell: (i32, i32),
    pub(super) share: u32,
    acc: u32,
    /// Seconds of song left, and until it heads for her anew.
    pub(super) left: f32,
    replan: f32,
    clock: f32,
}

/// The hero who resists the sirens.
const ODYSSEUS: u8 = 7;

impl Scene {
    pub(super) fn run_creatures(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.creature_clock += dt;
        // Stone wears off.
        for c in self.objects.iter_mut().filter_map(|o| o.combat.as_mut()) {
            c.stone = (c.stone - dt).max(0.0);
        }
        self.run_sea_monsters(dt, data, caches);
        self.run_charms(dt, data, caches);
    }

    /// A siren's song (message 0x15ca7: `ticks`, the share, her cell)
    /// heard by object `id`: a ship without hero 7 (Odysseus) is charmed
    /// (0x4a7390): already, the song's length grows by `ticks` and the
    /// share and cell are the new ones; else it starts now with nothing
    /// added up yet. Under it the ship heads for her cell every 30 ticks,
    /// no faster than 7000 units (`sail::CHARMED_TOP`), and adds the share
    /// each tick, taking a blow of 300 at each thousand (0x4a74c0; a
    /// fourth with Argus, none shielded from magic).
    pub(super) fn song_heard(&mut self, id: u32, ticks: u32, share: u32, cell: (i32, i32)) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        if !self.objects[i]
            .ship
            .as_ref()
            .is_some_and(|s| !s.has_hero(ODYSSEUS))
        {
            return;
        }
        let c = self.charmed.entry(id).or_insert(Charm {
            cell,
            share,
            acc: 0,
            left: 0.0,
            replan: 0.0,
            clock: 0.0,
        });
        (c.cell, c.share) = (cell, share);
        c.left += crate::clock::ticks(ticks);
    }

    /// The charmed ships (0x4a74c0).
    fn run_charms(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let tick = crate::clock::TICK;
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
                self.share_blow(id, blow * blows as f32, None, data, caches);
            }
            if c.left > 0.0 && self.object_index(id).is_some() {
                self.charmed.insert(id, c);
            } else {
                // The song over, the status ends (0x4df320): its stats
                // refreshed (0x4bece0), the top speed its own again.
                self.refresh_stats(id);
            }
        }
    }

    /// Whether a siren or the Gorgon is under the water now: while she
    /// shows no picture (from resting, 0x4019d0(0) / 0x403650(0), until
    /// she rises).
    pub fn creature_hidden(&self, id: u32) -> bool {
        if let Some(s) = self.sirens.get(&id) {
            return !s.picture;
        }
        if let Some(g) = self.gorgons.get(&id) {
            return !g.picture;
        }
        self.sea_monster_hidden(id)
    }
}
