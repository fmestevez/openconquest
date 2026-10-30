//! Shipyards' berths (DockClassTy, buildings 0x3a and 0x3b): an order
//! (0x416a40) builds the ship on the yard's berth at once if it is free,
//! else waits in the yard's queue (0x416f90 message 0); the ship rises on
//! the berth (task 4, 0x4b3ab0) and moves one cell off it when done
//! (0x4b46f0). Also the cell tests the moves off a cell use (0x499aa0,
//! 0x499990) and the town's berth clearing (command 0x28, 0x4415a0).

use super::*;

/// The eight directions' steps on the grid (0x5c4f4c), then the ninth
/// entry, none.
const DIR: [(i32, i32); 9] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (0, 0),
];

/// The direction from a ship to a cell of its window (0x59791d, indexed
/// `dx * 4 + dy`, here from `dx * 4 + dy = -5`); 0xff for none (the
/// window's third column or row at the map's edge).
const TOWARD: [u8; 16] = [
    3, 4, 5, 0xff, 2, 0xff, 6, 0xff, 1, 0, 7, 0xff, 0xff, 0xff, 0xff, 0xff,
];

/// A yard builds a queued order at most every 25 ticks (0x416f90 message
/// 0: `now >= +0x1fc + 25`).
const QUEUE_TICKS: u32 = 25;
/// A ship being built steps every 8 ticks (0x4b3ab0).
const BUILD_TICKS: u32 = 8;
/// The priority of the move off the berth when a ship is done (0x4b46f0
/// with 'e').
const LAUNCH_PRIORITY: u8 = 101;
/// The priority of the moves the town's berth clearing gives (0x4415a0).
const CLEAR_PRIORITY: u8 = 100;

/// A ship ordered at a yard whose berth was taken (0x416a40's queue
/// branch: a 24-byte entry `+0x200[+0x1f4]`), its price paid.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShipOrder {
    pub site: u32,
    pub ship: u8,
    /// The Argo (code 0x25), a royal ship.
    #[serde(default)]
    pub argo: bool,
    /// What it is armed with (`None`: the mission's standard load).
    #[serde(default)]
    pub arming: Option<Arming>,
    /// What was taken for it (amber, fish), given back if it is called off.
    pub paid: (u32, u32),
}

/// A ship 0x416a40 has built on a berth, made by the world's message
/// 0x1488 (the scene makes it before the next yard looks at the berth,
/// which it holds meanwhile).
#[derive(Debug, Clone)]
pub(super) struct NewShip {
    site: u32,
    owner: (u32, u32),
    kind: u8,
    argo: bool,
    cell: (i32, i32),
    heading: usize,
    arming: Option<Arming>,
    price: (u32, u32),
}

/// The launch voice of a nation's new ship (events 184, 185, 187, 186 by
/// nation, 0x4b3ab0): `SND_184` is empty, 185 and 186 `jb_d1`, 187
/// `121~2`.
fn launch_voice(nation: u32) -> Option<&'static str> {
    match nation {
        1 | 3 => Some("jb_d1"),
        2 => Some("121~2"),
        _ => None,
    }
}

/// A yard's answer to a ship order (events 228, 229, 231, 230 by nation,
/// 0x416a40): `SND_229` and `SND_230` hold three calls, 228 and 231 none.
const ORDER_CALLS: [&str; 3] = ["re_o2a", "re_o4a", "re_o6a"];

impl Scene {
    /// Yard `i`'s berth (its vtable +0xb8, 0x40e050): the anchor cell
    /// (`+0x74`, `+0x78`, record fields 5 and 6) a step toward its facing
    /// (`+0x1dc`, record field 10). (Not decoded: a facing below 0 gives
    /// the cell (-1, -1), whose grid index 0x4e8250 does not guard; the
    /// port has none.)
    pub(super) fn berth_cell(&self, i: usize) -> Option<(i32, i32)> {
        let o = &self.objects[i];
        let (x, y) = o.cell?;
        let f = *o.record.fields.get(10)? as i32;
        let (dx, dy) = *DIR.get(usize::try_from(f).ok()?)?;
        Some((x + dx, y + dy))
    }

    /// A cell's landscape (`grid + 8`, 0x499990): low nibble below 8,
    /// nibble 1 not 4, 5 or 6, nor 3 with `deep` (the draught flag).
    pub(super) fn sea_cell(&self, (x, y): (i32, i32), deep: bool) -> bool {
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return false;
        }
        let l = self.cell_types[(y * w + x) as usize];
        let n1 = (l >> 4) & 0xf;
        l & 0xf < 8 && !matches!(n1, 4..=6) && !(deep && n1 == 3)
    }

    /// The ship in a cell's object slot (`grid + 0xc`): the port's ships
    /// hold the cell they are in, but for one mended in a dock (off its
    /// slot, 0x4a3970 on entering).
    pub(super) fn slot_ship(&self, cell: (i32, i32)) -> Option<usize> {
        let t = TILE_SIZE as f32;
        (0..self.objects.len()).find(|&k| {
            let o = &self.objects[k];
            o.ship.is_some()
                && ((o.x / t).floor() as i32, (o.y / t).floor() as i32) == cell
                && !self.in_dock(k)
        })
    }

    /// Whether a cell's slot holds something: a ship, or one a yard has
    /// just built there.
    fn slot_taken(&self, cell: (i32, i32)) -> bool {
        self.slot_ship(cell).is_some() || self.new_ships.iter().any(|n| n.cell == cell)
    }

    /// Whether yard `i`'s berth is free (0x4e8250: 0x499a40 with flag 0):
    /// water and nothing in its slot.
    fn berth_free(&self, i: usize) -> bool {
        self.berth_cell(i)
            .is_some_and(|c| self.sea_cell(c, false) && !self.slot_taken(c))
    }

    /// Yard `i`'s answer to an order for a ship of type `kind` (0x4e82f0
    /// with 1; `None` for code 0xff or the Argo's 0x25): -5 while it is
    /// not idle (going up or being made a dock, `+0xa4 != 1`) or for a
    /// royal ship, bomber or rammer (0x597ce4, 0x4e8290) on a berth whose
    /// nibble 1 is 3; 0 with its berth free, else -1. (Not decoded: the
    /// reservation `+0x10c`, `+0x110`, `+0x114`, `+0x120`, which refuses
    /// a free berth to any other code; its writers were not read.)
    pub(super) fn yard_answer(&self, i: usize, kind: Option<u8>) -> i32 {
        if self.construction_at(self.objects[i].id).is_some() {
            return -5;
        }
        let free = self.berth_free(i);
        if let (Some(k), Some((x, y))) = (kind, self.berth_cell(i)) {
            let (w, h) = (self.grid.width as i32, self.grid.height as i32);
            let shallow = x >= 0
                && y >= 0
                && x < w
                && y < h
                && (self.cell_types[(y * w + x) as usize] >> 4) & 0xf == 3;
            if objects::deep_draught(k) && shallow {
                return -5;
            }
        }
        if free {
            0
        } else {
            -1
        }
    }

    /// Yard `site`'s answer to an order with code 0xff (0x4e82f0): 0 idle
    /// with its berth free.
    pub fn berth_answer(&self, site: u32) -> i32 {
        self.object_index(site)
            .filter(|&i| self.is_yard(i))
            .map_or(-5, |i| self.yard_answer(i, None))
    }

    /// Whether `site` is a standing shipyard or dock.
    pub(super) fn is_yard(&self, i: usize) -> bool {
        matches!(
            self.objects[i].class,
            ObjectClass::Building(SHIPYARD | DOCK)
        ) && self.objects[i].owner.is_some()
    }

    /// An order for a ship of type `kind` (the Argo with `argo`) at yard
    /// `site` (0x416a40; `pay` false for an order whose byte 0xf is 0xff,
    /// or one from the queue). With the yard idle and its berth free the
    /// ship is built there now: its price taken only with `pay`, the
    /// yard's call for the player's paid order. Else, with a town, it
    /// waits in the yard's queue, its full price taken whatever `pay`
    /// (one that cannot be paid for is dropped); without one it is lost.
    pub(super) fn yard_order(
        &mut self,
        site: u32,
        kind: u8,
        argo: bool,
        pay: bool,
        queued: Option<Option<Arming>>,
    ) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such shipyard")?;
        let owner = self.objects[i].owner.ok_or("No such shipyard")?;
        if self.home_settlement(i).is_none() {
            return Ok(());
        }
        // The price with the arming: the player's by the Building page
        // (0x43e5c0, the arms its settlement holds taken from its stock),
        // the computer players' their standard load; a queued order keeps
        // the arming it was given. (Not compared: 0x43ead0, the price of a
        // ship built now, against 0x43e5c0, the queued one's.)
        let human = Some(owner) == self.human.map(|h| (h.nation, h.slot));
        let (amber, fish, arming, stock) = if human {
            let p = self.ship_price(site, kind);
            let stock = p.home.map(|h| (h, p.stock_left));
            match queued.clone() {
                Some(arming) => (p.amber, p.fish, arming, None),
                None => (p.amber, p.fish, Some(p.arming), stock),
            }
        } else {
            let (amber, fish) = economy::ship_cost(kind);
            (amber, fish, queued.clone().flatten(), None)
        };
        let afford = |scene: &Scene| {
            let (have_amber, have_fish) = scene.funds(owner);
            if have_amber < amber || have_fish < fish {
                Err(format!(
                    "Not enough resources: {amber} amber and {fish} fish needed"
                ))
            } else {
                Ok(())
            }
        };
        if self.yard_answer(i, None) != 0 {
            afford(self)?;
            self.earn(owner, -(amber as i64), -(fish as i64));
            if let Some((home, left)) = stock {
                self.garrisons.insert(home, left);
            }
            self.ship_orders.push(ShipOrder {
                site,
                ship: kind,
                argo,
                arming,
                paid: (amber, fish),
            });
            self.yard_clocks.insert(site, 0);
            return Ok(());
        }
        if pay {
            afford(self)?;
            self.earn(owner, -(amber as i64), -(fish as i64));
        }
        if let Some((home, left)) = stock {
            self.garrisons.insert(home, left);
        }
        // The Argo takes the permission to build it (0x4a9810 with 0x50, 0).
        if argo && human {
            self.permits.set(crate::permits::ARGO, false);
        }
        let Some(cell) = self.berth_cell(i) else {
            return Ok(());
        };
        let facing = self.objects[i].record.fields.get(10).copied();
        let heading = if matches!(facing, Some(2 | 6)) { 6 } else { 0 };
        self.new_ships.push(NewShip {
            site,
            owner,
            kind,
            argo,
            cell,
            heading,
            arming,
            price: (amber, fish),
        });
        if human && pay && matches!(owner.0, 1 | 3) {
            let pick = ORDER_CALLS[self.gull_rng.next() as usize % ORDER_CALLS.len()];
            let (x, y) = (self.objects[i].x, self.objects[i].y);
            self.sounds.push((pick, x, y));
        }
        Ok(())
    }

    /// Makes the ships the yards have just built (message 0x1488, 0x4dc630
    /// message 2 with record +0x10 == 2, 0x4dfe00): on the berth, the hull
    /// at nothing, being built (task 4, `+0x251 = 2`). It takes the cell's
    /// slot if the cell suits its draught (0x499a40 with 0x597ce4[kind])
    /// and the slot is empty, with sound event 1 (`bildsh3`) for everyone;
    /// else it is removed, its price not given back (the Argo on a berth
    /// whose nibble 1 is 3: the queue's code 0x25 skips the draught test).
    pub(super) fn make_new_ships(&mut self, data: &GameData, caches: &mut Caches) {
        for n in std::mem::take(&mut self.new_ships) {
            if !self.sea_cell(n.cell, objects::deep_draught(n.kind))
                || self.slot_ship(n.cell).is_some()
            {
                continue;
            }
            let id = match self.make_ship(
                n.owner,
                n.kind,
                n.cell,
                n.heading,
                n.arming.as_ref(),
                data,
                caches,
            ) {
                Ok(Some(id)) => id,
                Ok(None) => continue,
                Err(e) => {
                    eprintln!("could not build ship: {e:#}");
                    continue;
                }
            };
            let Some(k) = self.object_index(id) else {
                continue;
            };
            let argo = n.argo && n.kind == Self::ROYAL_SHIP;
            let frames = objects::ship_build_frames(data, caches, n.kind, n.owner.0, argo)
                .ok()
                .filter(|f| !f.is_empty());
            let o = &mut self.objects[k];
            if let Some(c) = o.combat.as_mut() {
                c.hull = 0.0;
            }
            let frame = if n.heading == 6 { 40 } else { 0 };
            if let Some(frames) = frames {
                o.progress = Some((frame as f32 + 0.5) / frames.len() as f32);
                o.frames = frames;
            }
            let s = o.ship.as_mut().unwrap();
            s.argo = argo;
            s.build = Some(objects::Build {
                yard: n.site,
                since: BUILD_TICKS,
                frame,
                cancelled: false,
                price: n.price,
                shown: true,
            });
            let (x, y) = (o.x, o.y);
            self.sounds.push(("bildsh3", x, y));
        }
    }

    /// Puts a new ship of type `kind` for `owner` on cell `cell`, heading
    /// `heading`, armed as `arming` says (boats and merchants keep their
    /// one item).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn make_ship(
        &mut self,
        owner: (u32, u32),
        kind: u8,
        (x, y): (i32, i32),
        heading: usize,
        arming: Option<&Arming>,
        data: &GameData,
        caches: &mut Caches,
    ) -> anyhow::Result<Option<u32>> {
        let (nation, slot) = owner;
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 13];
        fields[0] = 0xc8;
        fields[1] = 1;
        fields[2] = nation;
        fields[3] = slot;
        fields[5] = nation;
        fields[6] = slot;
        fields[7] = kind as u32;
        fields[8] = u32::MAX;
        fields[9] = x as u32;
        fields[10] = y as u32;
        fields[11] = heading as u32;
        fields[12] = 100;
        let rec = ObjectRecord {
            id,
            class_id: 0xc8,
            fields,
        };
        let Some(mut obj) = objects::resolve(data, caches, &rec)? else {
            return Ok(None);
        };
        if let (Some(a), Some(ship), Some(fight)) = (arming, obj.ship.as_mut(), obj.combat.as_mut())
        {
            let mut squads = [0; 3];
            for &w in a.items.iter().filter(|&&w| w < 3) {
                squads[w as usize] += 1;
            }
            ship.crew.squads = squads;
            if combat::carries_weapons(kind) && kind > 1 {
                fight.weapons = a.items.iter().copied().filter(|&w| w >= 3).collect();
            }
            ship.armour = a.armour;
            ship.ram = a.ram;
        }
        self.objects.push(obj);
        self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        Ok(Some(id))
    }

    /// The mission's ships, as each is made (0x4dc630 message 2 with
    /// record +0x10 == 0, 0x4dc787): one whose cell does not suit its
    /// draught (0x499a40 with 0x597ce4[kind]) or whose slot an earlier one
    /// took is removed, its owner's ship counted lost (0x50e750 with -1).
    /// (Not traced: the order records reach message 2; here the list's.)
    /// The ids removed.
    pub(super) fn drop_misplaced_ships(&mut self) -> Vec<u32> {
        let t = TILE_SIZE as f32;
        let mut taken: Vec<(i32, i32)> = Vec::new();
        let mut gone = Vec::new();
        for k in 0..self.objects.len() {
            let o = &self.objects[k];
            let Some(s) = o.ship.as_ref() else {
                continue;
            };
            let cell = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
            if self.sea_cell(cell, objects::deep_draught(s.kind)) && !taken.contains(&cell) {
                taken.push(cell);
            } else {
                gone.push(o.id);
            }
        }
        for &id in &gone {
            if let Some(k) = self.object_index(id) {
                self.note_destroyed(k, None);
                self.objects.remove(k);
            }
        }
        gone
    }

    /// Whether ship `id` is being built on a berth.
    pub fn being_built(&self, id: u32) -> bool {
        self.object_index(id)
            .and_then(|i| self.objects[i].ship.as_ref())
            .is_some_and(|s| s.build.is_some())
    }

    /// Calls off the building of ship `i` (event 8 with no record while
    /// `+0x251 == 2`, 0x4aa540 at 0x4aa565): its picture runs back down.
    pub(super) fn cancel_build(&mut self, i: usize) {
        if let Some(b) = self.objects[i].ship.as_mut().and_then(|s| s.build.as_mut()) {
            b.cancelled = true;
        }
    }

    /// Ships ordered at yard `site` and waiting for its berth, of type
    /// `kind`.
    pub fn ships_waiting(&self, site: u32, kind: u8, argo: bool) -> u32 {
        self.ship_orders
            .iter()
            .filter(|p| p.site == site && p.ship == kind && p.argo == argo)
            .count() as u32
    }

    /// The ship being built on yard `site`'s berth, by index.
    pub fn ship_on_berth(&self, site: u32) -> Option<usize> {
        (0..self.objects.len()).find(|&k| {
            self.objects[k]
                .ship
                .as_ref()
                .and_then(|s| s.build)
                .is_some_and(|b| b.yard == site)
        })
    }

    /// The test the queue makes of its head entry (0x43da20 with 4, 1, 1,
    /// the kind, no arming record, byte 0xd, the price skipped, the yard
    /// test): -5 or -7 if the ship is not allowed (the permission, the
    /// buildings, the unique ships' study), -1 while the town is not idle,
    /// then the town's yards: 0 if one answers 0, else -1 if one answered
    /// -1, else -5. (Not ported: the sub-tests of the arming's items
    /// that byte 0xd runs.)
    fn queue_test(&self, owner: (u32, u32), town: u32, kind: u8) -> i32 {
        if self.may_build_ship_for(owner, kind).is_err() {
            return -5;
        }
        if self.construction_at(town).is_some() {
            return -1;
        }
        let mut r = -5;
        for y in self.town_yards(town) {
            match self.yard_answer(y, Some(kind)) {
                0 => return 0,
                -1 => r = -1,
                _ => {}
            }
        }
        r
    }

    /// The yards of settlement or town `town` (its buildings `+0x1d0`; the
    /// port's are those it is the nearest of), in the objects' order.
    pub(super) fn town_yards(&self, town: u32) -> Vec<usize> {
        (0..self.objects.len())
            .filter(|&k| self.is_yard(k) && self.home_settlement(k) == Some(town))
            .collect()
    }

    /// A game tick of the yards and the ships on their berths: each idle
    /// yard with orders waiting tries the first every 25 ticks (0x416f90
    /// message 0); each ship being built steps every 8 (0x4b3ab0).
    pub(super) fn run_yards(&mut self, data: &GameData, caches: &mut Caches) {
        let mut sites: Vec<u32> = Vec::new();
        for o in &self.ship_orders {
            if !sites.contains(&o.site) {
                sites.push(o.site);
            }
        }
        for site in sites {
            let clock = self.yard_clocks.entry(site).or_insert(0);
            *clock += 1;
            if *clock < QUEUE_TICKS {
                continue;
            }
            let Some(i) = self.object_index(site).filter(|&i| self.is_yard(i)) else {
                continue;
            };
            if self.construction_at(site).is_some() {
                continue;
            }
            self.yard_clocks.insert(site, 0);
            let Some(town) = self.home_settlement(i) else {
                continue;
            };
            let owner = self.objects[i].owner.unwrap();
            while let Some(k) = self.ship_orders.iter().position(|o| o.site == site) {
                let e = self.ship_orders[k].clone();
                match self.queue_test(owner, town, e.ship) {
                    0 => {
                        let code = (!e.argo).then_some(e.ship);
                        if self.yard_answer(i, code) == 0 {
                            self.ship_orders.remove(k);
                            let _ = self.yard_order(site, e.ship, e.argo, false, Some(e.arming));
                        }
                        break;
                    }
                    -5 | -7 => {
                        self.ship_orders.remove(k);
                        self.earn(owner, e.paid.0 as i64, e.paid.1 as i64);
                    }
                    _ => break,
                }
            }
        }
        self.yard_clocks
            .retain(|site, _| self.ship_orders.iter().any(|o| o.site == *site));
        self.make_new_ships(data, caches);
        let building: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.build.is_some()))
            .map(|o| o.id)
            .collect();
        for id in building {
            self.build_step(id, data, caches);
        }
    }

    /// Ship `id`'s building tick (0x4b3ab0, every 8 ticks): its hull rises
    /// a fortieth of the whole (0x5c76b0), the picture following
    /// (`base + (hull + 1) * 40 / whole`); it is done when the picture
    /// reaches the sequence's 40th frame (39 steps for most types, 40 for
    /// merchants, boarders, rammers and three of the unique ships), if
    /// its yard still stands. A yard gone calls it off: the picture runs
    /// back down a frame a step, then the price is given back, the
    /// owner's ship counted lost and the ship removed.
    fn build_step(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let nation = self.objects[i].record.fields.get(5).copied().unwrap_or(1);
        let Some(s) = self.objects[i].ship.as_mut() else {
            return;
        };
        let Some(mut b) = s.build else {
            return;
        };
        // Its events are not run while it is built (0x4ab720 with `+0x251
        // == 2`): orders given it meanwhile come to nothing here.
        s.task = None;
        s.zone = None;
        s.path.clear();
        let (kind, heading, argo) = (s.kind, s.heading, s.argo);
        if !b.shown {
            if let Ok(frames) = objects::ship_build_frames(data, caches, kind, nation, argo) {
                if !frames.is_empty() {
                    self.objects[i].frames = frames;
                }
            }
            b.shown = true;
        }
        b.since = b.since.saturating_add(1);
        let base = if heading == 6 { 40 } else { 0 };
        if b.since >= BUILD_TICKS {
            b.since = 0;
            if b.cancelled {
                b.frame -= 1;
                if (heading == 0 && b.frame < 1) || (heading == 6 && b.frame < 40) {
                    self.build_called_off(i, b);
                    return;
                }
            } else if !self.yard_stands(b.yard) {
                b.cancelled = true;
            } else {
                let whole = combat::hull_points(kind) as i64;
                let c = self.objects[i].combat.as_mut().unwrap();
                let hull = (((c.hull * 15.0).round() as i64) + whole / 40).min(whole);
                c.hull = hull as f32 / 15.0;
                b.frame = (base + (hull + 1) * 40 / whole) as i32;
                let done = (heading == 0 && b.frame >= 39) || (heading == 6 && b.frame >= 79);
                if done {
                    c.hull = c.max_hull;
                    if self.yard_stands(b.yard) {
                        self.build_done(i, b.yard);
                        return;
                    }
                    b.cancelled = true;
                }
            }
        }
        let o = &mut self.objects[i];
        let n = o.frames.len().max(1);
        o.progress = Some((b.frame.max(0) as f32 + 0.5) / n as f32);
        o.ship.as_mut().unwrap().build = Some(b);
    }

    /// Whether yard `site` still stands by its berth (0x49a4b0 with the
    /// yard's id; not decoded: which cells of the berth's neighbourhood
    /// list it searches).
    fn yard_stands(&self, site: u32) -> bool {
        self.object_index(site).is_some_and(|y| self.is_yard(y))
    }

    /// A building called off has run back down (0x4b3ab0 with `+0x73a <
    /// 0`): its price is given back (`+0x544`, whether or not 0x416a40
    /// took it), its owner's ship counted lost (0x50e750 with -1), the
    /// Argo's permission given back, the ship removed.
    fn build_called_off(&mut self, i: usize, b: objects::Build) {
        if let Some(owner) = self.objects[i].owner {
            self.earn(owner, b.price.0 as i64, b.price.1 as i64);
            let human = Some(owner) == self.human.map(|h| (h.nation, h.slot));
            if human && self.objects[i].ship.as_ref().is_some_and(|s| s.argo) {
                self.permits.set(crate::permits::ARGO, true);
            }
        }
        self.note_destroyed(i, None);
        self.objects.remove(i);
    }

    /// Ship `i` is built (0x4b3ab0): afloat with its whole hull, the
    /// launch voice for the viewing player's ship, the brains' and the
    /// computer players' "ship built" (0x4b3a20, message 0x5d95; not
    /// decoded: 0x4e7220's behaviour settings for a computer player's),
    /// then it moves one cell off the berth (0x4b46f0 with 'e').
    fn build_done(&mut self, i: usize, site: u32) {
        let id = self.objects[i].id;
        let o = &mut self.objects[i];
        o.progress = None;
        let s = o.ship.as_mut().unwrap();
        s.build = None;
        s.look = None;
        let (kind, argo) = (s.kind, s.argo);
        let (x, y) = (o.x, o.y);
        if self.is_own(&self.objects[i]) {
            if let Some(voice) = self.objects[i].owner.and_then(|o| launch_voice(o.0)) {
                self.sounds.push((voice, x, y));
            }
        }
        if argo {
            self.christen_argo(id);
        }
        self.ship_launched(site, kind, id);
        self.computer_launched(site, kind, id);
        if let Some(k) = self.object_index(id) {
            self.off_the_berth(k, LAUNCH_PRIORITY);
        }
    }

    /// Ship `k`'s next draw of its own generator (`+0x18`, the usual LCG;
    /// seeded `game +0x2ee5 x id`, ships 0x4dc6df). The game's seed is the
    /// original's time stamp (`RND_INIT`): here drawn from the scene's
    /// generator once.
    pub(super) fn ship_draw(&mut self, k: usize) -> u32 {
        let init = match self.rnd_init {
            Some(v) => v,
            None => {
                let v = self.rng.next() << 16 | self.rng.next();
                self.rnd_init = Some(v);
                v
            }
        };
        let id = self.objects[k].id;
        let s = self.objects[k].ship.as_mut().unwrap();
        let mut r = Rng(s.lcg.unwrap_or(init.wrapping_mul(id)));
        let v = r.next();
        s.lcg = Some(r.0);
        v
    }

    /// Ship `k`'s window of cells about it (0x4b46f0, 0x4b4b50): from one
    /// before its cell, clipped at 0, three on, clipped at the map's
    /// edge (so at the edge it reaches two cells off), rows from the top;
    /// its own cell left out. Each cell with its direction (0x59791d).
    fn window(&self, k: usize) -> Vec<((i32, i32), u8)> {
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        let t = TILE_SIZE as f32;
        let o = &self.objects[k];
        let (cx, cy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let (x0, y0) = ((cx - 1).max(0), (cy - 1).max(0));
        let (x1, y1) = ((x0 + 3).min(w), (y0 + 3).min(h));
        let mut cells = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                if (x, y) == (cx, cy) {
                    continue;
                }
                let at = (x - cx) * 4 + (y - cy) + 5;
                let dir = usize::try_from(at)
                    .ok()
                    .and_then(|a| TOWARD.get(a))
                    .copied()
                    .unwrap_or(0xff);
                cells.push(((x, y), dir));
            }
        }
        cells
    }

    /// Whether ship `k` may step into `cell`, lying in direction `dir`
    /// from it (0x499aa0): the landscape for its draught, the cell's slot
    /// empty or its own; straight on that is all; slanting, not past two
    /// ships lying grappled across the corner (the first corner's ship in
    /// state 6 with a ship contact in the second), nor past one object
    /// filling both corners. Its tests of the map's edges are kept.
    pub(super) fn may_step(&self, k: usize, cell: (i32, i32), dir: u8) -> bool {
        let deep = self.objects[k]
            .ship
            .as_ref()
            .is_some_and(|s| objects::deep_draught(s.kind));
        if !self.sea_cell(cell, deep) {
            return false;
        }
        if let Some(o) = self.slot_ship(cell) {
            if o != k {
                return false;
            }
        }
        if self.new_ships.iter().any(|n| n.cell == cell) {
            return false;
        }
        let dir = dir as i8;
        if dir % 2 == 0 {
            return true;
        }
        let (w, n) = (
            self.grid.width as i32,
            (self.grid.width * self.grid.height) as i32,
        );
        let c = cell.1 * w + cell.0;
        let x = c % w;
        let (a, b) = match dir {
            1 => {
                if x == 0 || c + w >= n {
                    return true;
                }
                (c - 1, c + w)
            }
            3 => {
                if c + w >= n || x == w - 1 {
                    return true;
                }
                (c + w, c + 1)
            }
            5 => {
                if c - w < 0 || x == w - 1 {
                    return true;
                }
                (c + 1, c - w)
            }
            7 => {
                if c - w < 0 || x == 0 {
                    return true;
                }
                (c - w, c - 1)
            }
            _ => {
                if c - 1 < 0 || c + w >= n {
                    return true;
                }
                (c - 1, c + w)
            }
        };
        let at = |c: i32| (c % w, c / w);
        let (sa, sb) = (self.slot_ship(at(a)), self.slot_ship(at(b)));
        if let Some(ka) = sa {
            let s = self.objects[ka].ship.as_ref().unwrap();
            let contact = s
                .grapple
                .map(|g| g.with)
                .or(s.ram_run.map(|r| r.target))
                .and_then(|id| self.object_index(id))
                .filter(|&q| self.objects[q].ship.is_some());
            let t = TILE_SIZE as f32;
            if s.move_state == 6
                && contact.is_some_and(|q| {
                    let o = &self.objects[q];
                    ((o.x / t).floor() as i32, (o.y / t).floor() as i32) == at(b)
                })
            {
                return false;
            }
        }
        match (sa, sb) {
            (Some(a), Some(b)) => a != b,
            _ => true,
        }
    }

    /// Ship `k` moves one cell off where it lies (0x4b46f0, at the end of
    /// a build with priority 101, leaving a port's berth with 91): to a
    /// cell of its window it may step into (0x499aa0), else, with none, to
    /// one holding a ship of its own or an allied player (the port has no
    /// alliances) of no higher priority than its own, the priority one
    /// higher for each row of the window after the first with one; drawn
    /// with its generator. The move is a plain go-to-cell (event 0x80).
    pub(super) fn off_the_berth(&mut self, k: usize, prio: u8) {
        let cells = self.window(k);
        let mut pool: Vec<(i32, i32)> = cells
            .iter()
            .filter(|&&(c, dir)| self.may_step(k, c, dir))
            .map(|&(c, _)| c)
            .collect();
        let mut prio = prio;
        if pool.is_empty() {
            let own = self.objects[k].owner;
            let mine = self.objects[k].ship.as_ref().map_or(0, |s| s.priority);
            let mut rows: Vec<i32> = cells.iter().map(|&((_, y), _)| y).collect();
            rows.dedup();
            for row in rows {
                for &(c, _) in cells.iter().filter(|&&((_, y), _)| y == row) {
                    let Some(q) = self.slot_ship(c) else {
                        continue;
                    };
                    if self.objects[q].owner == own
                        && self.objects[q]
                            .ship
                            .as_ref()
                            .is_some_and(|s| s.priority <= mine)
                    {
                        pool.push(c);
                    }
                }
                if !pool.is_empty() {
                    prio = prio.wrapping_add(1);
                }
            }
        }
        if pool.is_empty() {
            return;
        }
        let c = pool[self.ship_draw(k) as usize % pool.len()];
        self.go_to_cell(k, c, prio);
    }

    /// The cell ship `k` is told to move to when it lies in another's way
    /// (0x4b4b50 with priority `prio`): one of its window it may step into
    /// (0x499aa0), else a water cell (0x499990 with 0) holding a ship of
    /// its player of lower priority; drawn with its generator.
    pub(super) fn aside_cell(&mut self, k: usize, prio: u8) -> Option<(i32, i32)> {
        let own = self.objects[k].owner;
        let mut free = Vec::new();
        let mut weaker = Vec::new();
        for (c, dir) in self.window(k) {
            if self.may_step(k, c, dir) {
                free.push(c);
            } else if self.sea_cell(c, false) {
                if let Some(q) = self.slot_ship(c) {
                    if self.objects[q].owner == own
                        && self.objects[q]
                            .ship
                            .as_ref()
                            .is_some_and(|s| s.priority < prio)
                    {
                        weaker.push(c);
                    }
                }
            }
        }
        let pool = if free.is_empty() { weaker } else { free };
        if pool.is_empty() {
            return None;
        }
        Some(pool[self.ship_draw(k) as usize % pool.len()])
    }

    /// Ship `k`'s move event 0x80 (0x4ab250) to `cell`: dropped by a ship
    /// being built (it lacks capability 0x400); else the priority taken
    /// and a plain go-to-cell planned (0x4a5f30). A ship with a task goes
    /// on about it from there (the port's tasks plan their own routes).
    pub(super) fn go_to_cell(&mut self, k: usize, cell: (i32, i32), prio: u8) {
        if self.objects[k]
            .ship
            .as_ref()
            .is_none_or(|s| s.build.is_some())
        {
            return;
        }
        let t = TILE_SIZE as f32;
        let aside = (cell.0 as f32 * t + t / 2.0, cell.1 as f32 * t + t / 2.0);
        let s = self.objects[k].ship.as_ref().unwrap();
        let on = match (s.task, s.path.back()) {
            (Some(_), Some(&(lx, ly))) => self.plan_route_from(k, aside, lx, ly),
            _ => None,
        };
        let s = self.objects[k].ship.as_mut().unwrap();
        s.path = std::iter::once(aside).collect();
        if let Some(rest) = on {
            s.path.extend(rest.into_iter().skip_while(|&p| p == aside));
        }
        s.face = None;
        s.stalled = 0.0;
        s.priority = prio;
        s.patience = objects::PATIENCE;
        s.blocked = 0.0;
    }

    /// The town's command 0x28 with payload 3 (0x441ba0 case 0x28,
    /// 0x4415a0): each of `yards` idle with its berth taken has the ship
    /// lying there move one cell aside (0x4b4b50 with 100), unless it is
    /// one of `town_owner`'s sailing to that yard to be mended. A ship
    /// being built takes no notice (0x4ab720 skips its events).
    pub(super) fn clear_berths(&mut self, town_owner: Option<(u32, u32)>, yards: &[u32]) {
        for &site in yards {
            let Some(y) = self.object_index(site) else {
                continue;
            };
            if !self.is_yard(y) || self.yard_answer(y, None) != -1 {
                continue;
            }
            let Some(cell) = self.berth_cell(y) else {
                continue;
            };
            let Some(k) = self.slot_ship(cell) else {
                continue;
            };
            let s = self.objects[k].ship.as_ref().unwrap();
            // (The port's ship mended at a dock lies off the slot,
            // `slot_ship`, so this one is never found there.)
            let mending_here = self.objects[k].owner == town_owner
                && matches!(s.task, Some(Task::Repair { dock }) if dock == site);
            if mending_here || s.build.is_some() {
                continue;
            }
            if let Some(c) = self.aside_cell(k, CLEAR_PRIORITY) {
                self.go_to_cell(k, c, CLEAR_PRIORITY);
            }
        }
    }
}
