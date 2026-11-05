//! What the wild creatures do (the manual's "Monsters"): the Gorgon turns
//! ships in her area to stone, sirens' songs draw ships in, and harpies
//! wound crews (`flyers`). Lairs, nests and sea monsters fight with the weapons given
//! them in `combat::creature_weapons`.

use openconquest_formats::map::TILE_SIZE;

use super::{Caches, GameData, Scene};

/// A ship under a siren's song (the status `+0x25c`, 0x4a7390).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Charm {
    /// Her cell (`+0xad`).
    pub cell: (i32, i32),
    /// The share added each tick (`+0xc9`).
    pub(super) share: u32,
    /// The shares added up (`+0xc5`).
    pub(super) acc: u32,
    /// The tick the song took it (`+0xb1`).
    #[serde(default)]
    pub(super) since: u32,
    /// Its length in ticks (`+0xb5`).
    #[serde(default)]
    pub(super) len: u32,
    /// The move event 0x80 to her cell sent and not yet taken (0x4ab720).
    #[serde(default)]
    pub(super) sent: Option<(i32, i32)>,
}

/// The hero who resists the sirens.
const ODYSSEUS: u8 = 7;

/// The priority of the charm's move (0x4a7390, 0x4a74c0: +0x19 0x32).
const CHARM_PRIORITY: u8 = 50;

impl Scene {
    pub(super) fn run_creatures(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.creature_clock += dt;
        self.run_sea_monsters(dt, data, caches);
    }

    /// A siren's song (message 0x15ca7: `ticks`, the share, her cell)
    /// heard by object `id`: a ship without hero 7 (Odysseus) is charmed
    /// (0x4a7390): already, the song's length `+0xb5` grows by `ticks`
    /// and the share and cell are the new ones; else it starts now
    /// (`+0xb1`) with `ticks` and nothing added up (`+0xc5` 0). Either way
    /// the move event 0x80 (kind 0x17, priority 50) to her cell is sent.
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
        let now = self.ticks;
        let c = self.charmed.entry(id).or_insert(Charm {
            cell,
            share,
            acc: 0,
            since: now,
            len: 0,
            sent: None,
        });
        (c.cell, c.share) = (cell, share);
        c.len += ticks;
        c.sent = Some(cell);
    }

    /// The ships' statuses each game tick, in the objects' order, as the
    /// ship's tick (0x4dc630) has them before its event and move: stone
    /// freed (0x4df260: `now - +0xa1 >= +0xa5`), then the song over
    /// (0x4df320: `now - +0xb1 >= +0xb5`; its stats refreshed, 0x4bece0,
    /// the top speed its own again). A ship still charmed then takes its
    /// pending move (0x4ab720 -> 0x4ab250, which takes kind 0x17 without
    /// the capabilities the song cleared) and, unless stone left it no
    /// move tick (`+0x25d & 0x400` clear), runs 0x4a74c0. (The port ticks
    /// the ships after the creatures: one heard this tick moves at once.)
    pub(super) fn tick_statuses(&mut self, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        for o in self.objects.iter_mut() {
            if let Some(c) = o.combat.as_mut() {
                if c.stone_len > 0 && now.wrapping_sub(c.stone_since) >= c.stone_len {
                    (c.stone_since, c.stone_len) = (0, 0);
                }
            }
        }
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| self.charmed.contains_key(&o.id))
            .map(|o| o.id)
            .collect();
        self.charmed.retain(|id, _| ids.contains(id));
        for id in ids {
            let Some(mut c) = self.charmed.remove(&id) else {
                continue;
            };
            if now.wrapping_sub(c.since) >= c.len {
                self.refresh_stats(id);
                continue;
            }
            if let Some(cell) = c.sent.take() {
                self.charm_move(id, cell);
            }
            let stone = self
                .object_index(id)
                .and_then(|i| self.objects[i].combat.as_ref())
                .is_some_and(|c| c.stone_len > 0);
            if !stone {
                self.charm_tick(id, &mut c, data, caches);
            }
            if self.object_index(id).is_some() {
                self.charmed.insert(id, c);
            }
        }
    }

    /// A charmed ship's move tick (0x4a74c0): the share added up
    /// (`+0xc5`); at a thousand or more it is cut to the remainder and,
    /// unless shielded from magic (`+0x255`), the ship takes a blow of 300
    /// (code 4, type 0x1fd, no one's), 300 - 300 x 75 / 100 = 75 with hero
    /// 5 (Argus). Its top speed is 7000 (`+0x59f`, `sail::CHARMED_TOP`),
    /// and the move event 0x80 to her cell (kind 0x17, priority 50) is
    /// sent again: on every tick, as its stamp `+0xc1` is never written
    /// (0 - now > 30). Then the plain move tick 0x4a3f40.
    fn charm_tick(&mut self, id: u32, c: &mut Charm, data: &GameData, caches: &mut Caches) {
        c.acc += c.share;
        if c.acc >= 1000 {
            c.acc %= 1000;
            let Some(i) = self.object_index(id) else {
                return;
            };
            let o = &self.objects[i];
            let shielded = o.combat.as_ref().is_some_and(|c| c.protect_magic > 0.0);
            if !shielded {
                let argus = o
                    .ship
                    .as_ref()
                    .is_some_and(|s| s.has_hero(crate::objects::ARGUS));
                let blow = if argus { 300 + 300 * -75 / 100 } else { 300 };
                self.share_blow(id, blow as f32 / 15.0, None, data, caches);
            }
        }
        c.sent = Some(c.cell);
    }

    /// The charm's move event 0x80 taken (0x4ab250): the route made anew
    /// to her cell as a plain move's (0x4a5f30), the wait (`+0x591`) 0,
    /// the priority 50 (`+0x5c7`), the patience 5 (`+0x5c8`).
    fn charm_move(&mut self, id: u32, cell: (i32, i32)) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let t = TILE_SIZE as f32;
        let (x, y) = (cell.0 as f32 * t + t / 2.0, cell.1 as f32 * t + t / 2.0);
        let here = (self.objects[i].x, self.objects[i].y);
        let path = self.plan_route_order(i, here, x, y);
        let Some(s) = self.objects[i].ship.as_mut() else {
            return;
        };
        s.planned = 0.0;
        match path {
            Some(path) => {
                s.path = path;
                s.pending = None;
                s.waited = 0.0;
            }
            None => {
                s.pending = Some((x, y));
                s.waited = crate::clock::TICK;
            }
        }
        s.task = None;
        s.goal = Some(cell);
        s.priority = CHARM_PRIORITY;
        s.patience = crate::objects::PATIENCE;
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
