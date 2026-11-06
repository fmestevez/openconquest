//! Repairs: ships at a dock, buildings by a settlement or town (the
//! manual's "Repairing a Ship" and "Repairing a Terrain Object").

use openconquest_formats::ObjectClass;

use super::{Scene, TILE_SIZE};
use crate::assets::GameData;
use crate::economy::{DOCK, SETTLEMENT, SHIPYARD, TOWN};
use crate::objects::{self, Caches, Resource, Task};

/// A settlement mends a building (message 0x1389, 0x425100) a hundredth of
/// its strength every hundredth of its class's cycle (+0xc0: 200 ticks for
/// every player building), each step costing a hundredth of the building's
/// price; it stops when whole or when the amber runs out.
const MEND_CYCLE: u32 = 200;
/// At a dock a ship gets up to a tenth of its hull back every 9 ticks, for
/// its type's repair price (0x59b388) times the share mended (0x4b1fc0).
const DOCK_TICKS: u32 = 9;
const DOCK_REPAIR_PRICE: [u32; 12] = [140, 280, 800, 910, 650, 560, 490, 420, 420, 630, 420, 630];
/// Amber and ticks to make a unit, by item group (0x597164, 0x5971a0):
/// what mending a unit costs is reckoned from them.
const UNIT_PRICE: [u32; 3] = [30, 50, 70];
const UNIT_TICKS: [u32; 3] = [70, 80, 95];
/// Fish a unit of items 0-8 sells for at its full condition, a hundredth
/// (0x597170 raw, 0x4e4c80).
const SALE_UNIT_FISH: [i64; 9] = [2, 2, 3, 0, 0, 0, 0, 0, 0];
/// A hull's sale price before the twentieth taken (0x59b358, 0x4e4ce0).
const SALE_BASE: [i64; 12] = [200, 400, 1100, 1300, 900, 800, 700, 600, 600, 900, 600, 900];

/// A settlement mending its stock (message 0x1e, sub-order 2; state 2 of
/// 0x441ba0): how long it has been at it, the ticks it takes (`+0x114`),
/// the amber paid (`+0x118`) and how far along the last tick found it, in
/// whole percent (`+0x120`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StockRepair {
    pub site: u32,
    pub elapsed: f32,
    pub seconds: f32,
    #[serde(default)]
    pub paid: u32,
    #[serde(default)]
    pub progress: u32,
}

impl StockRepair {
    /// How far along it is, 0-100 (`+0x120`).
    pub fn done(&self) -> u32 {
        self.progress
    }

    fn ticks(&self) -> u32 {
        (self.seconds / crate::clock::TICK).round() as u32
    }

    fn elapsed_ticks(&self) -> u32 {
        (self.elapsed / crate::clock::TICK + 1e-3) as u32
    }
}

/// Fish to make a unit of each item (0x597170 x 100: marines 200, 200,
/// 300; weapons none).
const UNIT_FISH: [u32; 9] = [200, 200, 300, 0, 0, 0, 0, 0, 0];

/// A fort or barbarian structure mending its units along with its
/// structure (message 0x1389: 0x418300 notes each unit, place and index,
/// with its condition twice; 0x419ba0 and 0x40eba0 mend them as the
/// structure's repair goes on): the structure's percentage when it began
/// and its units.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnitMend {
    pub site: u32,
    pub start: u32,
    pub units: Vec<MendingUnit>,
}

/// A unit being mended: its item and index in its place, the condition it
/// is mended from and the one it had last.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MendingUnit {
    pub item: u8,
    pub index: u8,
    pub from: u8,
    pub last: u8,
}

/// What mending a ship would cost, as its main page stores it on the
/// repair button (0x4b1ed0, from 0x4e6980 / 0x4e6e90 into slot 9's
/// `+0x56`): the hull's `(full - hull) x 0x59b388[type] / full` (full
/// 0x5c76b0[type], hull `+0x271`), then for types other than 0 and 1 each
/// unit below 100, items 9 down to 0 (`+0x292` down, conditions `+0x3b9`
/// down by 32), `0x597164[item / 3] x (100 - condition) x 9 / 1000`. The
/// dock's own charge is reckoned step by step (0x4b1fc0,
/// `dock_repair_step`).
pub fn ship_repair_price(obj: &objects::MapObject) -> u32 {
    let (Some(s), Some(c)) = (obj.ship.as_ref(), obj.combat.as_ref()) else {
        return 0;
    };
    let kind = s.kind as usize % 12;
    let full = crate::combat::hull_points(kind as u8) as i64;
    let hull = (c.hull * 15.0).round() as i64;
    let mut price = (full - hull) * DOCK_REPAIR_PRICE[kind] as i64 / full;
    if kind != 0 && kind != 1 {
        // Item 9 (the unique ships' own) is never worn (0x4be110), so its
        // price past the table (0x597164[3] = 2) never counts here.
        for (k, u) in obj.unit_conditions().iter().enumerate().rev() {
            for &c in u.iter().filter(|&&c| c < 100) {
                price += (UNIT_PRICE[k / 3] * (100 - c as u32) * 9 / 1000) as i64;
            }
        }
    }
    price as u32
}

/// What mending the worn `units` (by item) costs at a settlement
/// (0x440fd0): amber and ticks, each `(100 - condition) / 100` of the
/// unit's price and making time.
pub fn stock_repair_cost(units: &[Vec<u8>; 9]) -> (u32, u32) {
    let (mut amber, mut ticks) = (0, 0);
    for (k, u) in units.iter().enumerate() {
        for &c in u.iter().filter(|&&c| c < 100) {
            let worn = 100 - c as u32;
            amber += worn * UNIT_PRICE[k / 3] / 100;
            ticks += worn * UNIT_TICKS[k / 3] / 100;
        }
    }
    (amber, ticks)
}

/// A building's strength in whole percent (`+0xa8`).
pub(super) fn structure_percent(c: &crate::combat::Combatant) -> u32 {
    ((c.hull / c.max_hull.max(1.0)) * 100.0).clamp(0.0, 100.0) as u32
}

impl Scene {
    /// Orders own ship `ship` to sail to own dock `dock` and be repaired
    /// (the order 0x4aa990, kind 1): the berth `+0x756` is 0x4b23a0's, the
    /// berth (vt+0xb8, 0x40e050) of a yard or dock (class 0x3a or 0x3b) of
    /// the ship's own player in the cell; else the order is dropped.
    pub fn order_repair(&mut self, ship: u32, dock: u32) -> Result<(), String> {
        self.send_to_dock(ship, dock, false)
    }

    /// Orders ship `ship` to own dock `dock` to be sold (ship command 0x1e
    /// with byte 0 3, `+0x749`; only the computers' order 200 blocks of
    /// mode 1 send it, 0x4dbb40). The order (0x4aa990) asks nothing of the
    /// hull, so an unhurt ship goes too.
    pub fn order_sell(&mut self, ship: u32, dock: u32) -> Result<(), String> {
        self.send_to_dock(ship, dock, true)
    }

    fn send_to_dock(&mut self, ship: u32, dock: u32, sell: bool) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let d = self.object_index(dock).ok_or("Choose one of your docks")?;
        // Own dock ruins (the repair pointer is offered there, 0x4dd486):
        // command 0x1e finds them by id (0x4e2ba0, 0x51f1f0) and sends
        // event 0x20, but the order's berth (0x4aa990 -> 0x4b23a0) reads
        // the object slot of their cell, which the fall emptied (0x423d60
        // on 0x138b -> 0x499860): -1, the order dropped, no notice.
        if self.fallen(dock) {
            return Err(String::new());
        }
        if !matches!(
            self.objects[d].class,
            ObjectClass::Building(SHIPYARD | DOCK)
        ) || self.objects[d].owner != self.objects[i].owner
            || self.construction_at(dock).is_some()
        {
            return Err("Choose one of your docks".into());
        }
        let c = self.objects[i].combat.as_ref().ok_or("Not a ship")?;
        let worn = self.objects[i]
            .unit_conditions()
            .iter()
            .flatten()
            .any(|&c| c < 100);
        if c.hull >= c.max_hull && !worn && !sell {
            return Err("The ship needs no repair".into());
        }
        let berth = self.berth_cell(d).ok_or("Choose one of your docks")?;
        let t = TILE_SIZE as f32;
        let (bx, by) = (berth.0 as f32 * t + t / 2.0, berth.1 as f32 * t + t / 2.0);
        let path = self
            .plan_route(i, bx, by)
            .ok_or("The ship cannot reach that dock")?;
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        s.path = path;
        s.task = Some(Task::Repair { dock, sell });
        s.dock_step = None;
        Ok(())
    }

    /// Whether yard or dock `d` takes a ship in (its vt+0xb0, 0x40dfe0): a
    /// facing (`+0x1dc >= 0`), standing (`+0xa4 == 1`: not going up,
    /// fallen, sold or asleep; `+0x10c` is not read, so a yard or dock
    /// being mended takes one), its town (`+0x1d8 & 0x10` with `+0x1c8`, set for
    /// yards and docks) and no ship in it (`+0x1e0`, set by 0xcb7 as one
    /// enters, cleared by 0xcb8 as it leaves).
    pub(super) fn dock_free(&self, d: usize) -> bool {
        let id = self.objects[d].id;
        self.berth_cell(d).is_some()
            && self.standing(id)
            && !self.fallen(id)
            && self.home_settlement(d).is_some()
            && !self.objects.iter().any(|o| {
                o.ship
                    .as_ref()
                    .and_then(|s| s.landing.as_ref())
                    .is_some_and(|l| l.dock == Some(id))
            })
    }

    /// Yard or dock `dock` lets its ship go (0x4e7640 on 0x2874 {its
    /// town}: 0xcb9 to `+0x1e0`, then `+0x1e0 = 0`): the ship frees its
    /// contact `+0x8e6` (0x4dccc5 -> 0x4e06a0), so its repair machine
    /// goes on without the dock (state 2 ends in 3, state 4 at once).
    pub(super) fn dock_lets_go(&mut self, dock: u32) {
        for o in self.objects.iter_mut() {
            if let Some(l) = o.ship.as_mut().and_then(|s| s.landing.as_mut()) {
                if l.dock == Some(dock) {
                    l.dock = None;
                }
            }
        }
    }

    /// Has settlement or town `site` mend own building `target`.
    pub fn order_mend(&mut self, site: u32, target: u32) -> Result<(), String> {
        let s = self.object_index(site).ok_or("No such building")?;
        if !self.is_own(&self.objects[s]) {
            return Err("Only your settlements and towns repair".into());
        }
        // The town's call on order 0x1e sub-order 1 (0x441ba0), whatever
        // its message 0x1389 to the building makes of it.
        self.town_voice(site);
        self.mend_by(site, target)
    }

    /// Settlement or town `site` mends `target`, a building of the same
    /// player (command 0x1e, for any player).
    pub(super) fn mend_by(&mut self, site: u32, target: u32) -> Result<(), String> {
        self.mend_check(site, target)?;
        let t = self.object_index(target).unwrap();
        if !self.mending.contains(&target) {
            self.mending.push(target);
            self.begin_mend_steps(t);
            self.begin_unit_mend(t);
        }
        Ok(())
    }

    /// A fort or barbarian structure's command 0x1e (0x419f80, message
    /// 0x1389): standing and idle (0x423d60) it mends itself, its units
    /// noted; no worn test there.
    pub(super) fn self_mend(&mut self, id: u32) -> Result<(), String> {
        let i = self.object_index(id).ok_or("No such building")?;
        if self.construction_at(id).is_none() && !self.mending.contains(&id) {
            self.mending.push(id);
            self.begin_mend_steps(i);
            self.begin_unit_mend(i);
        }
        // A fort's call after it (0x41bd2f, `fort_voice`).
        self.fort_voice(id);
        Ok(())
    }

    /// Whether settlement or town `site` may mend `target` (`mend_by`'s
    /// tests).
    pub(super) fn mend_check(&self, site: u32, target: u32) -> Result<(), String> {
        let s = self.object_index(site).ok_or("No such building")?;
        let t = self
            .object_index(target)
            .ok_or("Choose one of your buildings")?;
        let (home, obj) = (&self.objects[s], &self.objects[t]);
        if !matches!(home.class, ObjectClass::Building(SETTLEMENT | TOWN)) {
            return Err("Only your settlements and towns repair".into());
        }
        if !matches!(obj.class, ObjectClass::Building(_)) || obj.owner != home.owner {
            return Err("Choose one of your buildings".into());
        }
        // A settlement repairs itself and its own buildings (0x441350),
        // those in the ground it builds on.
        if site != target {
            let (Some((hx, hy)), Some((cx, cy))) = (home.cell, obj.cell) else {
                return Err("Not one of this settlement's buildings".into());
            };
            let size = super::trade::building_cells(home);
            let dx = (hx - cx).max(cx - (hx + size - 1)).max(0);
            let dy = (hy - cy).max(cy - (hy + size - 1)).max(0);
            if dx.max(dy) > super::BUILD_RADIUS {
                return Err("Not one of this settlement's buildings".into());
            }
        }
        let c = obj.combat.as_ref().ok_or("Nothing to repair")?;
        if c.hull >= c.max_hull || self.construction_at(target).is_some() {
            return Err("It needs no repair".into());
        }
        Ok(())
    }

    /// The mending's start (0x423d60): `+0x124` = now, the condition
    /// `+0xa8` the building's.
    fn begin_mend_steps(&mut self, t: usize) {
        let o = &self.objects[t];
        if let Some(c) = o.combat.as_ref() {
            self.mend_steps
                .insert(o.id, (self.ticks, structure_percent(c)));
        }
    }

    /// A fort or barbarian structure begins being mended: its units are
    /// noted with their conditions (0x418300) and the structure's
    /// percentage (`+0x114`).
    pub(super) fn begin_unit_mend(&mut self, t: usize) {
        let o = &self.objects[t];
        let Some(c) = o.combat.as_ref().filter(|_| o.holds_units()) else {
            return;
        };
        let start = structure_percent(c);
        let mut units = Vec::new();
        for (k, place) in o.fort_places().iter().enumerate() {
            for (j, &cond) in place.iter().enumerate() {
                units.push(MendingUnit {
                    item: k as u8,
                    index: j as u8,
                    from: cond,
                    last: cond,
                });
            }
        }
        let site = o.id;
        self.unit_mends.retain(|m| m.site != site);
        self.unit_mends.push(UnitMend { site, start, units });
    }

    /// Drops the units mending at `site` that are no longer there, at
    /// their place and index (0x418430, 0x40eae0: after a blow, or when one
    /// is missed).
    pub(super) fn forget_gone_units(&mut self, site: u32) {
        let Some(i) = self.object_index(site) else {
            return;
        };
        let places = self.objects[i].fort_places();
        if let Some(m) = self.unit_mends.iter_mut().find(|m| m.site == site) {
            m.units.retain(|u| {
                places
                    .get(u.item as usize)
                    .is_some_and(|p| (u.index as usize) < p.len())
            });
        }
    }

    /// A fort or barbarian structure being mended mends its units as far
    /// as the structure's repair has gone (0x419ba0, 0x40eba0): the repair's
    /// progress (`+0x120`, the share of what was lost that is back) of what
    /// each unit lacked, a unit worn meanwhile counting from that much less,
    /// each step paid `0x597164[item / 3]` amber and `0x597170[item]` fish
    /// x 100 a hundredth of the unit (the owner's, barbarians too) and done
    /// only when both suffice.
    fn mend_fort_units(&mut self, i: usize, owner: (u32, u32), now: u32) {
        let site = self.objects[i].id;
        if self.objects[i].combat.is_none() {
            return;
        }
        let Some(k) = self.unit_mends.iter().position(|m| m.site == site) else {
            return;
        };
        let start = self.unit_mends[k].start.min(99);
        let progress = now.saturating_sub(start) * 100 / (100 - start);
        let mut places = self.objects[i].fort_places();
        let mut units = std::mem::take(&mut self.unit_mends[k].units);
        let mut changed = false;
        for u in &mut units {
            let Some(cond) = places
                .get_mut(u.item as usize)
                .and_then(|p| p.get_mut(u.index as usize))
            else {
                // One is gone: the rest wait for the next tick (0x418430).
                self.unit_mends[k].units = units;
                self.forget_gone_units(site);
                return;
            };
            if *cond < u.last {
                u.from = u.from.saturating_sub(u.last - *cond);
                u.last = *cond;
            }
            let from = u.from as u32;
            let goal = (100 - from) * progress / 100 + from;
            if (*cond as u32) < goal {
                let step = goal - *cond as u32;
                let amber = UNIT_PRICE[u.item as usize / 3] * step / 100;
                let fish = UNIT_FISH[u.item as usize] * step / 100;
                let (have_amber, have_fish) = self.funds(owner);
                if amber <= have_amber && fish <= have_fish {
                    self.earn(owner, -(amber as i64), -(fish as i64));
                    *cond = goal as u8;
                    u.last = goal as u8;
                    changed = true;
                }
            }
        }
        self.unit_mends[k].units = units;
        if changed {
            self.objects[i].set_fort_places(places);
        }
    }

    /// Has the settlement of own workshop `workshop` mend its stock's worn
    /// units (the workshop's command 0x15, `CBUT_REPARMING`, sends message
    /// 0x1e with sub-order 2): it pays at once (0x440fd0) and is busy until
    /// the time has passed, when they are all whole (0x441ba0 state 2).
    pub fn order_repair_stock(&mut self, workshop: u32) -> Result<(), String> {
        let w = self.object_index(workshop).ok_or("No such building")?;
        if !self.is_own(&self.objects[w]) {
            return Err("Not your workshop".into());
        }
        // The workshop's call on order 0x1e (0x436ee0, every order but
        // 0x3c), even under construction; the settlement's sub-order 2
        // clears its own (0x441ba0).
        let (owner, x, y) = (self.objects[w].owner, self.objects[w].x, self.objects[w].y);
        self.building_voice(owner, &super::halt::STUDIO_CALLS, x, y);
        if self.construction_at(workshop).is_some() {
            return Err("Not your workshop".into());
        }
        let home = self
            .home_settlement(w)
            .ok_or("The workshop belongs to no settlement")?;
        // (Not ported: the original lets it begin while the settlement
        // trains, overwriting that job, 0x4438a2; what its queue then does
        // is not decoded.) Ordered again while mending, it pays again and
        // starts over (no busy test there).
        if self.training_at(home).is_some() {
            return Err("The settlement is busy".into());
        }
        let units = self.stock_units(home);
        if units.iter().flatten().all(|&c| c >= 100) {
            return Err("Nothing needs repair".into());
        }
        let (amber, ticks) = stock_repair_cost(&units);
        let owner = self.objects[w].owner.ok_or("Not your workshop")?;
        if self.funds(owner).0 < amber {
            return Err(format!("Repairing costs {amber} amber"));
        }
        self.earn(owner, -(amber as i64), 0);
        self.stock_repairs.retain(|r| r.site != home);
        self.stock_repairs.push(StockRepair {
            site: home,
            elapsed: 0.0,
            seconds: crate::clock::ticks(ticks),
            paid: amber,
            progress: 0,
        });
        Ok(())
    }

    /// Calls off settlement `site`'s stock repair (order 10, 0x4430f0): the
    /// part of the price not done comes back (`(100 - progress) x paid /
    /// 100`), and the worn units are made whole in their order, kinds in
    /// turn and least worn first, as long as each one's time still fits in
    /// the time spent, the first that does not ending it. Given by a right
    /// click on the workshop's command 0x15 (0x4f46c0); the settlement's
    /// call is `town_halt`'s.
    pub fn cancel_stock_repair(&mut self, site: u32) -> Result<(), String> {
        let k = self
            .stock_repairs
            .iter()
            .position(|r| r.site == site)
            .ok_or("Nothing to call off")?;
        let r = self.stock_repairs.remove(k);
        let i = self.object_index(site).ok_or("No such settlement")?;
        let refund = (100 - r.progress.min(100) as i64) * r.paid as i64 / 100;
        if let Some(owner) = self.objects[i].owner {
            self.earn(owner, refund, 0);
        }
        let spent = r.elapsed_ticks();
        let mut units = self.stock_units(site);
        let mut sum = 0;
        'walk: for (kind, list) in units.iter_mut().enumerate() {
            for c in list.iter_mut().filter(|c| **c < 100) {
                let t = UNIT_TICKS[kind / 3] * (100 - *c as u32) / 100;
                if sum + t > spent {
                    break 'walk;
                }
                sum += t;
                *c = 100;
            }
        }
        self.set_stock_units(site, units);
        Ok(())
    }

    /// The stock repair going on at settlement `site`, if any.
    pub fn stock_repair_at(&self, site: u32) -> Option<&StockRepair> {
        self.stock_repairs.iter().find(|r| r.site == site)
    }

    /// Settlements mending their stock: each tick it notes how far along it
    /// is, in whole percent; nothing is mended till the end, when every
    /// worn unit is whole (0x441ba0 state 2).
    fn run_stock_repairs(&mut self, dt: f32) {
        let mut done = Vec::new();
        for r in &mut self.stock_repairs {
            r.elapsed += dt;
            let (spent, ticks) = (r.elapsed_ticks(), r.ticks());
            if spent < ticks {
                r.progress = spent * 100 / ticks;
            } else {
                done.push(r.site);
            }
        }
        self.stock_repairs.retain(|r| !done.contains(&r.site));
        for site in done {
            let mut units = self.stock_units(site);
            units.iter_mut().flatten().for_each(|c| *c = 100);
            self.set_stock_units(site, units);
        }
    }

    /// A ship at a dock has its worn units made whole, heavy weapons
    /// first, each for `price[item / 3] x (100 - condition) x 9 / 1000`
    /// while the amber lasts (0x4b1fc0).
    fn mend_units(&mut self, i: usize, owner: (u32, u32)) {
        let Some(mut units) = self.objects[i]
            .ship
            .as_ref()
            .map(|_| self.objects[i].unit_conditions())
        else {
            return;
        };
        let mut changed = false;
        for k in (0..9).rev() {
            for c in units[k].iter_mut().filter(|c| **c < 100) {
                let cost = UNIT_PRICE[k / 3] * (100 - *c as u32) * 9 / 1000;
                if cost <= self.funds(owner).0 {
                    *c = 100;
                    changed = true;
                    self.earn(owner, -(cost as i64), 0);
                }
            }
        }
        if changed {
            if let Some(u) = self.objects[i].units() {
                *u = units;
            }
        }
    }

    /// Ships at their docks and buildings being mended get their strength
    /// back, at the owner's expense, until whole or the money runs out.
    /// A computer player's fort mended fills its room again (0x419910,
    /// for sides 3 and 4): down its class's default arming (0x5966b8), each
    /// unit that fits is added whole, until the room is full or the list
    /// is done; their amber and fish are paid (0x597164, 0x597170 x 100).
    pub(super) fn rearm_fort(&mut self, id: u32) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let o = &self.objects[i];
        let (ObjectClass::Building(class @ 0x34..=0x36), Some(owner)) = (o.class, o.owner) else {
            return;
        };
        let human = Some(owner) == self.human.map(|h| (h.nation, h.slot));
        if human || self.seat_of(owner).is_some() {
            return;
        }
        let room = crate::combat::fort_room(class);
        let mut places = o.fort_places();
        let items = |p: &[Vec<u8>; 9]| -> Vec<u8> {
            p.iter()
                .enumerate()
                .flat_map(|(k, v)| std::iter::repeat_n(k as u8, v.len()))
                .collect()
        };
        let (list, _) = crate::combat::fort_arming(class);
        let (mut amber, mut fish) = (0i64, 0i64);
        for &item in list.iter().rev() {
            let load = crate::combat::fort_load(&items(&places));
            if load >= room {
                break;
            }
            if load + crate::combat::item_power(item) > room {
                continue;
            }
            places[item as usize].push(100);
            let (a, f) = crate::combat::item_cost(item);
            amber += a as i64;
            fish += f as i64;
        }
        if amber > 0 || fish > 0 {
            self.objects[i].set_fort_places(places);
            self.earn(owner, -amber, -fish);
        }
    }

    pub(super) fn run_repairs(&mut self, dt: f32) {
        self.run_stock_repairs(dt);
    }

    /// Buildings being mended at the game tick (message 0 of 0x425990:
    /// while busy with job 0xff, standing idle and not limed, `+0x144`
    /// 0, vtable +0x70; a fort's 0x419ba0 mends its units after it). The
    /// step 0x425100: once `cycle / 100` ticks (`+0xc0`, 200 for every
    /// player building) have passed since the last (`+0x124`, set as the
    /// mending begins, 0x423d60), below 100% it pays a hundredth of the
    /// building's price (0x42d8b0, `price / 100`; short of it the mending
    /// stops) and raises the condition `+0xa8` 1%, the strength `+0xac`
    /// following (`full x condition / 100`); at 100% the strength is full,
    /// vtable +0x74 runs (a fort's 0x419910, `rearm_fort`) and it stops.
    pub(super) fn step_mending(&mut self) {
        let now = self.ticks;
        let work: Vec<u32> = self.mending.clone();
        let mut done = Vec::new();
        for id in work {
            let Some(i) = self.object_index(id) else {
                done.push(id);
                continue;
            };
            let (Some(owner), Some(c)) = (self.objects[i].owner, self.objects[i].combat.as_ref())
            else {
                done.push(id);
                continue;
            };
            if c.lime > 0.0 {
                continue;
            }
            let max = c.max_hull;
            let full = max as u32;
            let strength = |n: u32| (full * n / 100) as f32;
            let (last, mut cond) = *self
                .mend_steps
                .entry(id)
                .or_insert((now, structure_percent(c)));
            // A blow meanwhile lowers the condition (0x423f9c).
            if c.hull < strength(cond) {
                cond = structure_percent(c);
            }
            if now >= last + MEND_CYCLE / 100 {
                if cond < 100 {
                    let price = match self.objects[i].class {
                        ObjectClass::Building(class) => crate::economy::building_cost(class),
                        _ => 0,
                    } / 100;
                    if self.funds(owner).0 < price {
                        done.push(id);
                    } else {
                        self.earn(owner, -(price as i64), 0);
                        cond += 1;
                        self.objects[i].combat.as_mut().unwrap().hull = strength(cond);
                    }
                } else {
                    self.objects[i].combat.as_mut().unwrap().hull = max;
                    done.push(id);
                    self.rearm_fort(id);
                }
                self.mend_steps.insert(id, (now, cond));
            } else {
                self.mend_steps.insert(id, (last, cond));
            }
            self.mend_fort_units(i, owner, cond);
        }
        for id in done {
            self.mending.retain(|&m| m != id);
            self.unit_mends.retain(|m| m.site != id);
            self.mend_steps.remove(&id);
        }
    }

    /// The repair order's machine at the ship tick (0x4b2420, `+0x755`;
    /// 0x4b17b0 runs it for task `+0x72f` 1):
    ///
    /// - **1**: off the berth `+0x756` it is sent there again (0x4a6270);
    ///   there, lying still (`+0x58c` 0): the dock gone, the order is done
    ///   (message 8) and notice 0x143 posted (0x4a1b80); not a yard or
    ///   dock of its player, it waits, posting 0x143 each tick (read by the
    ///   computers' dock block, 0x4db110); the dock
    ///   free (vt+0xb0, `dock_free`), the close-in (0x4b17e0): in contact
    ///   (`+0x8e6`, 0xcb7 to the dock), commands held (0x4b37d0), state 2.
    /// - **2** (0x4b1c60 -> 0x4b1a90): a point of the line a tick and a
    ///   hull rotation; at its end (or back at its start) called off it
    ///   leaves the dock (0xcb8), lies on its heading's frame and the order
    ///   is done (0x4b3850, message 8), else state 8 (3 without the dock).
    /// - **8** (0x4b89b0, no stop test): the sail furls; furled, state 4
    ///   with `+0x751` = now.
    /// - **4** (0x4b1fc0): mended in steps (`dock_repair_step`); done,
    ///   `+0x751` = the line's points, state 3.
    /// - **3** (0x4b1d40): it backs out; out, the dock left (0xcb8), the
    ///   commands back (0x4b3850), the order done (message 8), a move off
    ///   the berth (0x4b46f0 with 'Q'), notice 0x134 (0x4a1690) and the
    ///   brains' 0x5d98 (0x4b6d60).
    ///
    /// The dock's 0xcb9 as its town falls is `dock_lets_go`. In state 1
    /// the sale (`+0x749 == 3`, `order_sell`) makes it task 5 instead
    /// (`sale_tick`).
    pub(super) fn step_dock_repairs(&mut self, data: &GameData, caches: &mut Caches) {
        for o in self.objects.iter_mut() {
            if let Some(s) = o.ship.as_mut() {
                if !matches!(s.task, Some(Task::Repair { .. })) {
                    s.dock_step = None;
                }
            }
        }
        // A ship the dock takes for sale runs task 5 from its next tick.
        let sold: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship
                    .as_ref()
                    .is_some_and(|s| matches!(s.task, Some(Task::Sell(_))))
            })
            .map(|o| o.id)
            .collect();
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship
                    .as_ref()
                    .is_some_and(|s| matches!(s.task, Some(Task::Repair { .. })))
            })
            .map(|o| o.id)
            .collect();
        for id in ids {
            if let Some(k) = self.object_index(id) {
                self.dock_tick(k);
            }
        }
        for id in sold {
            if let Some(k) = self.object_index(id) {
                self.sale_tick(k, data, caches);
            }
        }
    }

    /// Ship `k` being sold, a tick (task 5, 0x4b4380, run by 0x4ab930
    /// while `+0x251 == 2`):
    ///
    /// - **1**: its capabilities and task bits cleared (`+0x261`,
    ///   `+0x25d` 0), its walkway, party and hooks taken off (0x4e0240,
    ///   0x4df0c0); the line to its cell's middle (0x4b40d0), state 2.
    ///   (A walkway shown, `+0x8c2`, would first run down a frame a tick,
    ///   state 7: the dock takes a ship lying with none, and one docked
    ///   for repair shows none.) A contact is let go: a building's
    ///   (0x32: a ship its settlement sells from a yard or dock,
    ///   `sell_object`) with 0xcb8 and 0x2876 to it; a ship's (200,
    ///   0x4e02a0) is not reached.
    /// - **2** (0x4b4220): a hull rotation the way it turns and two
    ///   points of the line a tick; at the line's end and turned, its
    ///   building picture (0x4e1bb0(4) with `+0x251 == 2`: 0x4e0120 mode
    ///   2) on frame 0x27 (heading 0) or 0x4f (6), state 3.
    /// - **3**: the frame down one a tick; below 1 (heading 0) or 0x28
    ///   (6) its player gets 0x4e4ce0 amber (0x4a8de0) and 0x4e4c80 fish
    ///   (0x4a8c30) (`sale_value`), the ship is counted destroyed by its
    ///   own player (0x50e750 with `+0x24f`: nobody's loss or gain), its
    ///   group told 0x144 (0x4a1bf0) and it is removed (message 10).
    ///   (Not ported: 0x50e830's viewer left on its cell, as for a
    ///   building called off.)
    fn sale_tick(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let Some(Task::Sell(mut sale)) = self.objects[k].ship.as_ref().and_then(|s| s.task) else {
            return;
        };
        match sale.state {
            1 => {
                let s = self.objects[k].ship.as_mut().unwrap();
                s.fire_wind_down();
                s.path.clear();
                // A building contact (a ship its settlement sells from a
                // yard or dock, 0x4dcf1b) is let go: 0xcb8 to the dock
                // (0x4b3970, its `+0x1e0` cleared), 0x2876 to it (0x4b3930,
                // which ends no reload here: the dock's contact is not a
                // load link) and the contact freed (0x4e06a0).
                s.landing = None;
                s.dock_step = None;
                self.sale_line(k, &mut sale);
                sale.state = 2;
            }
            2 => {
                if self.sale_step(k, &mut sale) {
                    let o = &mut self.objects[k];
                    let nation = o.record.fields.get(5).copied().unwrap_or(1);
                    let s = o.ship.as_mut().unwrap();
                    let (kind, argo) = (s.kind, s.argo);
                    s.rotation = None;
                    s.furl = 0.0;
                    sale.frame = if s.heading == 6 { 0x4f } else { 0x27 };
                    if let Ok(frames) = objects::ship_build_frames(data, caches, kind, nation, argo)
                    {
                        if !frames.is_empty() {
                            o.frames = frames;
                        }
                    }
                    sale.state = 3;
                    let n = o.frames.len().max(1);
                    o.progress = Some((sale.frame as f32 + 0.5) / n as f32);
                }
            }
            3 => {
                sale.frame -= 1;
                let heading = self.objects[k].ship.as_ref().unwrap().heading;
                if (heading == 0 && sale.frame < 1) || (heading == 6 && sale.frame < 0x28) {
                    let (amber, fish) = self.sale_value(k);
                    let owner = self.objects[k].owner;
                    if let Some(owner) = owner {
                        self.earn(owner, amber, fish);
                    }
                    self.note_destroyed(k, owner);
                    let id = self.objects[k].id;
                    self.post_notice(id, 0x144, 0);
                    self.objects.remove(k);
                    return;
                }
                let o = &mut self.objects[k];
                let n = o.frames.len().max(1);
                o.progress = Some((sale.frame.max(0) as f32 + 0.5) / n as f32);
            }
            _ => {}
        }
        if let Some(s) = self.objects[k].ship.as_mut() {
            s.task = Some(Task::Sell(sale));
        }
    }

    /// 0x4b40d0: a line (0x4b5220, `+0x766`; none when there already)
    /// from where the hull is to its cell's middle (the picture's corner
    /// to `cell x 64 - (w - 64) / 2`: the picture centred on the cell),
    /// and of headings 0 and 6 the one fewer turns away (0x4b5420, as
    /// 0x4b5390 without writing the frame; 6 on a tie), its way; turning,
    /// a hull below the rotations on `h x 3 + 0x20`. (`+0x25b` 5, furled,
    /// which stops the turn, is not reached: the ship lies unfurled.)
    fn sale_line(&mut self, k: usize, sale: &mut objects::Sale) {
        let t = TILE_SIZE as f32;
        let o = &mut self.objects[k];
        let (cx, cy) = ((o.x / t).floor(), (o.y / t).floor());
        sale.from = (o.x, o.y);
        sale.to = (cx * t + t / 2.0, cy * t + t / 2.0);
        sale.count = 0;
        if sale.from != sale.to {
            let major = (sale.to.0 - sale.from.0)
                .abs()
                .max((sale.to.1 - sale.from.1).abs())
                .round() as i32;
            sale.count = (major + 1 + 1) / 2 * 2;
        }
        let s = o.ship.as_mut().unwrap();
        let way = |c: i32| -> i32 {
            match s.rotation {
                Some(r) => {
                    let u = c * 3 - r as i32;
                    if u.abs() > 12 {
                        u - 24 * u.signum()
                    } else {
                        u
                    }
                }
                None => {
                    let u = c - s.heading as i32;
                    if u.abs() > 4 {
                        u - 8 * u.signum()
                    } else {
                        u
                    }
                }
            }
        };
        let (u0, u6) = (way(0), way(6));
        let (face, u) = if u0.abs() < u6.abs() {
            (0, u0)
        } else {
            (6, u6)
        };
        sale.face = face;
        sale.turn = u.signum() as i8;
        if sale.turn != 0 && s.rotation.is_none() {
            s.rotation = Some(s.heading as u8 * 3);
        }
        sale.at = 0;
    }

    /// 0x4b4220, a tick: a hull rotation the way it turns (frames 0x20 to
    /// 0x37, round), stopping on the heading's first; two points of the
    /// line, its last kept; true at the line's end with no turn left.
    fn sale_step(&mut self, k: usize, sale: &mut objects::Sale) -> bool {
        let o = &mut self.objects[k];
        let s = o.ship.as_mut().unwrap();
        if sale.turn != 0 {
            let r = s.rotation.map_or(s.heading as i32 * 3, i32::from);
            let r = (r + sale.turn as i32).rem_euclid(24);
            s.rotation = Some(r as u8);
            s.heading = (r / 3) as usize;
            if s.heading == sale.face as usize && r % 3 == 0 {
                sale.turn = 0;
            }
        }
        let mut done = false;
        if sale.at < sale.count - 2 {
            sale.at += 2;
        } else {
            sale.at = sale.count - 1;
            done = sale.turn == 0;
        }
        if (0..sale.count).contains(&sale.at) {
            (o.x, o.y) = super::landing::line_point(sale.from, sale.to, sale.at);
        }
        done
    }

    /// What ship `k` sells for (0x4e4ce0 amber, 0x4e4c80 fish): each load
    /// aboard `(+0x527 + +0x47f x 100) x rate / 100` when `+0x527` is not
    /// 0, which with the loaders' fields (0x4ad445, 0x4bd8e7: `+0x47f` the
    /// hundreds rounded up, `+0x527` the last one's fill, 1-100) is
    /// `(load + 100) x rate / 100` for a load above 0: fish 10, a leviathan 5000,
    /// amber 0x78; each unit of items 0-8 its condition x
    /// 0x597164[item / 3] / 100 amber and condition x 0x597170[item] fish;
    /// and 0x59b358[type] / 20 amber.
    pub(super) fn sale_value(&self, k: usize) -> (i64, i64) {
        let o = &self.objects[k];
        let Some(s) = o.ship.as_ref() else {
            return (0, 0);
        };
        let mut amber = 0i64;
        let load = s.cargo.max(0.0) as i64;
        if load > 0 {
            let rate = match s.cargo_kind {
                Some(Resource::Fish) => 10,
                Some(Resource::Leviathan) => 5000,
                Some(Resource::Amber) => 0x78,
                None => 0,
            };
            amber += (load + 100) * rate / 100;
        }
        let mut fish = 0i64;
        for (item, units) in o.unit_conditions().iter().enumerate() {
            for &c in units {
                amber += c as i64 * UNIT_PRICE[item / 3] as i64 / 100;
                fish += c as i64 * SALE_UNIT_FISH[item];
            }
        }
        amber += SALE_BASE[s.kind.min(11) as usize] / 20;
        (amber, fish)
    }

    /// Ship `k`'s repair order at its tick (0x4b2420).
    fn dock_tick(&mut self, k: usize) {
        let t = TILE_SIZE as f32;
        let id = self.objects[k].id;
        let o = &self.objects[k];
        let s = o.ship.as_ref().unwrap();
        let Some(Task::Repair { dock, sell }) = s.task else {
            return;
        };
        let Some(mut l) = s.landing.clone() else {
            if !s.path.is_empty() || s.move_state != 0 {
                return;
            }
            let here = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
            // The dock gone: the order done (message 8) and notice 0x143
            // to the ship's group (0x4a1b80).
            let Some(d) = self.object_index(dock) else {
                self.objects[k].ship.as_mut().unwrap().task = None;
                self.post_notice(id, 0x143, 0);
                return;
            };
            let Some(berth) = self.berth_cell(d) else {
                return;
            };
            if here != berth {
                let (bx, by) = (berth.0 as f32 * t + t / 2.0, berth.1 as f32 * t + t / 2.0);
                if let Some(path) = self.plan_route(k, bx, by) {
                    self.objects[k].ship.as_mut().unwrap().path = path;
                }
                return;
            }
            // Fallen on the way: the fall emptied its cell's object slot
            // (0x423d60 -> 0x499860), which is what 0x4b2420 reads at the
            // berth: as gone, the order done and notice 0x143.
            if self.fallen(dock) {
                self.objects[k].ship.as_mut().unwrap().task = None;
                self.post_notice(id, 0x143, 0);
                return;
            }
            let dk = &self.objects[d];
            // Not a yard or dock of its player: it waits, notice 0x143 to
            // its group each tick (0x4a1b80).
            if dk.owner != self.objects[k].owner
                || !matches!(dk.class, ObjectClass::Building(SHIPYARD | DOCK))
            {
                self.post_notice(id, 0x143, 0);
                return;
            }
            let Some(cell) = dk.cell else {
                return;
            };
            if !self.dock_free(d) {
                return;
            }
            // `+0x749 == 3` (0x4b2506): the sale, `+0x251 = 2`, task 5 in
            // state 1 (0x4b2512); no contact is made and the dock stays
            // free.
            if sell {
                let s = self.objects[k].ship.as_mut().unwrap();
                s.task = Some(Task::Sell(objects::Sale {
                    state: 1,
                    from: (0.0, 0.0),
                    to: (0.0, 0.0),
                    count: 0,
                    at: 0,
                    face: 0,
                    turn: 0,
                    frame: 0,
                }));
                s.path.clear();
                s.dock_step = None;
                return;
            }
            if let Some(mut l) = self.dock_close_in(k, cell) {
                l.dock = Some(dock);
                let s = self.objects[k].ship.as_mut().unwrap();
                s.landing = Some(l);
                s.dock_step = None;
            }
            return;
        };
        l.hull_ticks += 1;
        let hull_step = l.hull_ticks > super::landing::HULL_DELAY;
        let contact = l.dock.is_some_and(|d| self.object_index(d).is_some());
        let state = l.state;
        match state {
            2 => {
                if self.close_in_step(k, &mut l, 1) {
                    if l.abort {
                        let s = self.objects[k].ship.as_mut().unwrap();
                        s.furl = 0.0;
                        s.rotation = None;
                        s.landing = None;
                        s.task = None;
                        s.close_in.face = l.face;
                        s.close_in.turn = l.turn;
                        s.close_in.count = l.count;
                        return;
                    }
                    l.state = if contact { 8 } else { 3 };
                }
            }
            8 => {
                if self.furl_step(k, &mut l, hull_step) {
                    l.state = 4;
                    let now = self.ticks;
                    self.objects[k].ship.as_mut().unwrap().dock_step = Some(now);
                }
            }
            4 => {
                let done = if !contact || l.abort {
                    self.refresh_stats(id);
                    true
                } else {
                    self.dock_repair_step(k)
                };
                if done {
                    l.back = l.count;
                    l.state = 3;
                    self.objects[k].ship.as_mut().unwrap().dock_step = None;
                }
            }
            3 if self.back_out_step(k, &mut l, hull_step) => {
                self.objects[k].ship.as_mut().unwrap().task = None;
                self.off_the_berth(k, b'Q');
                self.brain_repaired(id);
                self.post_notice(id, 0x134, 0);
                self.brain_mended(id);
                return;
            }
            _ => {}
        }
        if let Some(s) = self.objects[k].ship.as_mut() {
            s.landing = Some(l);
        }
    }

    /// A step of the mending at a dock (0x4b1fc0, state 4; `+0x751` the
    /// last step's tick): every 9 ticks (`now - +0x751 < 9` returns) the
    /// step `min(full - hull, full x 10 / 100)` costs `0x59b388[type] x
    /// step / full` amber; short of it a computer player's ship (side 3
    /// or 4) is done unmended, any other gets `amber x full /
    /// 0x59b388[type]` (to full) for all its amber; then each worn unit,
    /// items 9 down to 0, is made whole for `0x597164[item / 3] x (100 -
    /// condition) x 9 / 1000` while the amber lasts (item 9, the unique
    /// ships' own, is never worn: 0x4be110 wears items 3-8); the stats
    /// refresh 0x4bee60 at every step. At a full hull the refits
    /// (`dock_refits`) and done: true.
    fn dock_repair_step(&mut self, i: usize) -> bool {
        let now = self.ticks;
        let id = self.objects[i].id;
        let s = self.objects[i].ship.as_ref().unwrap();
        let last = s.dock_step.unwrap_or(now);
        if now.wrapping_sub(last) < DOCK_TICKS {
            return false;
        }
        let (Some(owner), Some(c)) = (self.objects[i].owner, self.objects[i].combat.as_ref())
        else {
            return true;
        };
        let kind = s.kind as usize % 12;
        let mut hull = (c.hull * 15.0).round() as i64;
        self.objects[i].ship.as_mut().unwrap().dock_step = Some(now);
        let full = crate::combat::hull_points(kind as u8) as i64;
        let step = (full - hull).min(full * 10 / 100);
        let price = DOCK_REPAIR_PRICE[kind] as i64 * step / full;
        let amber = self.funds(owner).0 as i64;
        if amber < price {
            if self.side_computer(Some(owner)) {
                return true;
            }
            hull = (hull + amber * full / DOCK_REPAIR_PRICE[kind] as i64).min(full);
            self.earn(owner, -amber, 0);
        } else {
            hull += step;
            self.earn(owner, -price, 0);
        }
        self.objects[i].combat.as_mut().unwrap().hull = hull as f32 / 15.0;
        self.mend_units(i, owner);
        let whole = hull == full;
        if whole {
            self.dock_refits(i);
        }
        self.refresh_stats(id);
        whole
    }

    /// The refits at a full hull (0x4b1fc0 at 0x4b2276): a boat (type 0)
    /// without its item 3, a merchant (1) without its item 4, gets one
    /// whole (0x4bd920 with 100); types 2-7 in contact with their yard or
    /// dock (`+0x8e6`, its `+5`) are fitted by 0x4bbca0 (`dock_refit`)
    /// with the free room 0x4be800(5, 0xff). The `eax` 0x4b22f7 tests
    /// against 0x100 and 0x200 is the type byte zero-extended (0x4b227c),
    /// so types 8-11 get none.
    fn dock_refits(&mut self, i: usize) {
        let o = &mut self.objects[i];
        let item = match o.ship.as_ref().map(|s| s.kind) {
            Some(0) => 3,
            Some(1) => 4,
            Some(2..=7) => {
                let dock = o.ship.as_ref().and_then(|s| s.landing.as_ref()?.dock);
                if let Some(dock) = dock {
                    let room = free_room(o);
                    self.dock_refit(i, dock, room);
                }
                return;
            }
            _ => return,
        };
        let Some(c) = o.combat.as_mut() else {
            return;
        };
        if !c.weapons.contains(&item) {
            c.weapons.push(item);
            // The new unit's condition is 100 (`units` fills it whole).
            o.units();
        }
    }

    /// A ship of type 2-7 fitted at a dock (0x4bbca0, `room` its free
    /// room): only at a dock (class 0x3b) whose town (`+0x1c8`, 0x51f1f0)
    /// is found. Each item 0-8 the town may have made (0x43da20 `town, 1,
    /// 1, 0, item, 0, 1, 0, 1`: `town_may_make`) is offered `room` times
    /// (items 6-8 `room / 2`), each whole (100); the type's weights of the
    /// three values (2: 100, 60, 80; 3: 100, 10, 30; 4: 0, 50, 100; 5 and
    /// 6: 100, 50, 0; 7: 0, 100, 50) pick the units (0x4bb670 with 5
    /// tries, `fit_units`). Their price, `0x597164[item / 3]` amber and
    /// `0x597170[item]` fish each: short of the amber (`+0x20`) or of the
    /// fish in hundreds (0x4a8b70, `+0x2c / 100`), a computer's ship (side
    /// 3 or 4) is fitted free and any other not at all; else the amber
    /// and the fish are paid (0x4a8d60, 0x4a8be0: the fish as the table
    /// has it, not in hundreds). Each unit is added with its condition
    /// (0x4bd920). Returns the units fitted.
    pub(super) fn dock_refit(&mut self, k: usize, dock: u32, room: i32) -> u32 {
        let kind = self.objects[k].ship.as_ref().map_or(0, |s| s.kind);
        let Some(d) = self.object_index(dock) else {
            return 0;
        };
        if !(2..=7).contains(&kind) || room == 0 {
            return 0;
        }
        if self.objects[d].class != ObjectClass::Building(DOCK) {
            return 0;
        }
        let Some(town) = self.home_settlement(d) else {
            return 0;
        };
        let mut offer: [Vec<u32>; 9] = Default::default();
        for (item, o) in offer.iter_mut().enumerate() {
            if self.town_may_make(town, item as u8) {
                let n = if item > 5 { room / 2 } else { room };
                *o = vec![100; n.max(0) as usize];
            }
        }
        let weights = match kind {
            2 => [100, 60, 80],
            3 => [100, 10, 30],
            4 => [0, 50, 100],
            5 | 6 => [100, 50, 0],
            _ => [0, 100, 50],
        };
        let owner = self.objects[k].owner;
        let levels = self.levels_of(owner).arms;
        let out = self.fit_units(k, &offer, room, weights, 5, levels);
        let (mut amber, mut fish) = (0i64, 0i64);
        for (item, &n) in out.iter().enumerate() {
            let (a, f) = crate::combat::item_cost(item as u8);
            amber += a as i64 * n as i64;
            fish += (f / 100) as i64 * n as i64;
        }
        let Some(owner) = owner else {
            return 0;
        };
        let (have_amber, have_fish) = self.funds(owner);
        if (have_amber as i64) < amber || ((have_fish / 100) as i64) < fish {
            if !self.side_computer(Some(owner)) {
                return 0;
            }
        } else {
            self.earn(owner, -amber, -fish);
        }
        let o = &mut self.objects[k];
        o.units();
        let mut fitted = 0;
        for (item, &n) in out.iter().enumerate() {
            for &cond in &offer[item][..n] {
                if item < 3 {
                    o.ship.as_mut().unwrap().crew.squads[item] += 1;
                } else if let Some(c) = o.combat.as_mut() {
                    c.weapons.push(item as u8);
                }
                o.ship.as_mut().unwrap().conditions[item].push(cond as u8);
                fitted += 1;
            }
        }
        fitted
    }

    /// Whether town `town` may have `item` made, the price aside (0x43da20
    /// with `param_3` 0): it stands (`+0xa4` 1; a town, class 0x33, also
    /// while it goes up, `+0xa4` 0; not fallen, 2, sold, 3, nor asleep,
    /// 0x10; being mended is state 1, `+0x10c`, and passes), then case 1
    /// for its own player (`can_make_by`).
    pub(super) fn town_may_make(&self, town: u32, item: u8) -> bool {
        let Some(t) = self.object_index(town) else {
            return false;
        };
        let o = &self.objects[t];
        let Some(owner) = o.owner else {
            return false;
        };
        let standing = !self.fallen_or_sold(town)
            && o.asleep.is_none()
            && (self.construction_at(town).is_none() || o.class == ObjectClass::Building(TOWN));
        standing && self.can_make_by(town, owner, item)
    }

    /// The fitter 0x4bb670 (ship `k`, its own random numbers `+0x18`):
    /// the units of `offer` (by item, each its condition) to take into
    /// `room` places, `tries` times at random, the best by `weights` of
    /// the three values kept. `full` is the last weight of 100 (0 without
    /// one), `partial` whether one lies between 0 and 100. From the first
    /// item offered to one past the last (`end`): without `partial` and
    /// with `full` 2 `end` is at most 3; a half (`room / 2`) of 0 makes
    /// `end` 6. A try: with a half and `end` past 6, `r = draw % (half +
    /// 1)` heavy units (items 6 to `end`, 2 places each, `room - 2r`
    /// left), each picked at random by its score (`unit_score` at the
    /// player's level `+0x9e`), stopping when none scores; then while
    /// places are left, a unit of the items below 6 by score (levels
    /// `+0x9c + item / 3`). The try's values (`unit_score` of each unit
    /// taken, unweighted, per value) replace the best's when better:
    /// without `partial` the `full` value higher, else the weighted gain
    /// above 0. Returns the best try's units by item.
    fn fit_units(
        &mut self,
        k: usize,
        offer: &[Vec<u32>; 9],
        room: i32,
        weights: [i32; 3],
        tries: u32,
        levels: [u8; 3],
    ) -> [usize; 9] {
        let mut full = 0usize;
        let mut partial = false;
        for (c, &w) in weights.iter().enumerate() {
            if w == 100 {
                full = c;
            }
            if w > 0 && w < 100 {
                partial = true;
            }
        }
        let mut out = [0; 9];
        let Some(first) = offer.iter().position(|o| !o.is_empty()) else {
            return out;
        };
        let mut end = offer.iter().rposition(|o| !o.is_empty()).unwrap() + 1;
        let half = room / 2;
        if !partial && full == 2 {
            end = end.min(3);
        }
        if half == 0 {
            end = 6;
        }
        let score = |item: usize, n: usize, weighted: bool, value: usize| {
            unit_score(
                item,
                levels[item / 3],
                offer[item][n],
                weighted,
                value,
                weights,
            )
        };
        // A unit of items `from..to` picked by its score (one draw); false
        // when none scores.
        let pick = |scene: &mut Self, picks: &mut [usize; 9], from: usize, to: usize| {
            let total: u32 = (from..to)
                .filter(|&i| offer[i].len() > picks[i])
                .map(|i| score(i, picks[i], partial, full))
                .sum();
            if total == 0 {
                return false;
            }
            let p = scene.ship_draw(k) % total;
            let mut sum = 0;
            for i in from..to {
                if offer[i].len() > picks[i] {
                    sum += score(i, picks[i], partial, full);
                    if p < sum {
                        picks[i] += 1;
                        break;
                    }
                }
            }
            true
        };
        let mut best = [0i64; 3];
        let mut best_picks = [0usize; 9];
        for _ in 0..tries {
            let mut picks = [0usize; 9];
            let mut left = room;
            if half != 0 && end >= 7 {
                let r = self.ship_draw(k) % (half as u32 + 1);
                left = room - 2 * r as i32;
                for _ in 0..r {
                    if !pick(self, &mut picks, 6, end) {
                        break;
                    }
                }
            }
            let cap = end.min(6);
            while left > 0 && first < cap {
                if !pick(self, &mut picks, first, cap) {
                    break;
                }
                left -= 1;
            }
            let mut cur = [0i64; 3];
            for (i, &n) in picks.iter().enumerate().take(end).skip(first) {
                for u in 0..n {
                    for (c, v) in cur.iter_mut().enumerate() {
                        *v += score(i, u, false, c) as i64;
                    }
                }
            }
            let better = if !partial {
                best[full] < cur[full]
            } else {
                (0..3)
                    .map(|c| (cur[c] - best[c]) * weights[c] as i64)
                    .sum::<i64>()
                    > 0
            };
            if better {
                if !partial {
                    best[full] = cur[full];
                } else {
                    best = cur;
                }
                best_picks = picks;
            }
        }
        out[first..end].copy_from_slice(&best_picks[first..end]);
        out
    }
}

/// A ship's free room for units (0x4be800 with 5, 0xff): its type's room
/// (0x597ae4) less each unit it carries, items 0-8 (0x4be780 with 0),
/// by its places (0x597cf0: 1, heavy weapons 6-8 2), at least 0; none
/// for types 10 and 11.
fn free_room(o: &crate::objects::MapObject) -> i32 {
    let kind = o.ship.as_ref().map_or(0, |s| s.kind) as usize;
    if kind == 10 || kind == 11 {
        return 0;
    }
    let used: i32 = o
        .unit_counts()
        .iter()
        .enumerate()
        .map(|(i, &n)| n as i32 * if i >= 6 { 2 } else { 1 })
        .sum();
    (crate::combat::MIN_POWER[kind % 12] as i32 - used).max(0)
}

/// A unit's score to the fitter (0x4bb490): its item's three values A, B
/// and C at `level` (0x597d2c) times its condition over the item's reload
/// at that level (0x5c76e0[item x 3 + level]); `weighted`, their sum each
/// by its weight / 100, else value `value` alone (none past 2).
fn unit_score(item: usize, level: u8, cond: u32, weighted: bool, value: usize, w: [i32; 3]) -> u32 {
    let mut power = crate::info_panel::POWER[item]
        .iter()
        .map(|v| v[level.min(2) as usize]);
    let reload = crate::combat::reload_ticks(item as u8, level);
    let of = |t: u32| t * cond / reload;
    if weighted {
        power
            .zip(w)
            .map(|(t, w)| (of(t) as i32 * w) / 100)
            .sum::<i32>() as u32
    } else {
        power.nth(value).map_or(0, of)
    }
}
