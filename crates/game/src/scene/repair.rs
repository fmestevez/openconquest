//! Repairs: ships at a dock, buildings by a settlement or town (the
//! manual's "Repairing a Ship" and "Repairing a Terrain Object").

use openconquest_formats::ObjectClass;

use super::Scene;
use crate::economy::{DOCK, SETTLEMENT, TOWN, WORKSHOP};
use crate::objects::Task;

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
fn structure_percent(c: &crate::combat::Combatant) -> u32 {
    ((c.hull / c.max_hull.max(1.0)) * 100.0).clamp(0.0, 100.0) as u32
}

impl Scene {
    /// Orders own ship `ship` to sail to own dock `dock` and be repaired.
    pub fn order_repair(&mut self, ship: u32, dock: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let d = self.object_index(dock).ok_or("Choose one of your docks")?;
        // A dock of the ship's own player.
        if self.objects[d].class != ObjectClass::Building(DOCK)
            || self.objects[d].owner != self.objects[i].owner
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
        if c.hull >= c.max_hull && !worn {
            return Err("The ship needs no repair".into());
        }
        // It sails alongside the dock (a water cell touching it).
        let path = self
            .route_beside(i, d)
            .ok_or("The ship cannot reach that dock")?;
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        s.path = path;
        s.task = Some(Task::Repair { dock });
        Ok(())
    }

    /// Has settlement or town `site` mend own building `target`.
    pub fn order_mend(&mut self, site: u32, target: u32) -> Result<(), String> {
        let s = self.object_index(site).ok_or("No such building")?;
        if !self.is_own(&self.objects[s]) {
            return Err("Only your settlements and towns repair".into());
        }
        self.mend_by(site, target)
    }

    /// Settlement or town `site` mends `target`, a building of the same
    /// player (command 0x1e, for any player).
    pub(super) fn mend_by(&mut self, site: u32, target: u32) -> Result<(), String> {
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
        if !self.mending.contains(&target) {
            self.mending.push(target);
            self.begin_unit_mend(t);
        }
        Ok(())
    }

    /// A fort or barbarian structure begins being mended: its units are
    /// noted with their conditions (0x418300) and the structure's
    /// percentage (`+0x114`).
    fn begin_unit_mend(&mut self, t: usize) {
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
    fn mend_fort_units(&mut self, i: usize, owner: (u32, u32)) {
        let site = self.objects[i].id;
        let Some(c) = self.objects[i].combat.as_ref() else {
            return;
        };
        let Some(k) = self.unit_mends.iter().position(|m| m.site == site) else {
            return;
        };
        let now = structure_percent(c);
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
        if !self.is_own(&self.objects[w]) || self.construction_at(workshop).is_some() {
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
    /// click on the workshop's command 0x15 (0x4f46c0).
    /// Order 10 to object `id` (no data: a right click on a command button
    /// whose code sends it, 0x4f46c0; Players.cpp 0x5228c0 delivers it as
    /// message 0x280f, order byte 10). What it calls off is the object's,
    /// whatever button sent it. Ported:
    ///
    /// - a settlement or town (TownObj 0x441ba0) mending its stock: called
    ///   off (0x4430f0, `cancel_stock_repair`); then, as every building
    ///   (LandObj 0x425990, 0x444f17), its own mending (job 0xff) stopped,
    ///   no refund (it is paid step by step);
    /// - a workshop (Studio 0x436ee0): its own mending stopped first
    ///   (0x427f50 -> 0x425990); else, busy with its settlement's stock
    ///   repair (job 5) and nothing else, the order goes on to the
    ///   settlement (`+0x1c8`);
    /// - the other buildings but forts (Smithy 0x434ed0, School 0x432210,
    ///   the temples 0x438300 / 0x439260, Dock 0x416f90 through 0x4e8660):
    ///   their own mending stopped.
    ///
    /// Not ported: the forts' and barbarian strongholds' (Fortress
    /// 0x419f80, Barb 0x40ef70: their 12 unit groups stop shooting, the
    /// unit mending list freed; whether 0x419f80 reaches LandObj not
    /// found), a building under construction's (LandObj's second dispatch,
    /// its guard `+0xe8` not identified), the town and dock upgrades' (their
    /// refund's `+0x7c` record +0x49 / +0x45 not tied to the port's
    /// construction), the research, learning and Flame cancels (which
    /// command starts which job, their `+0x120` ticks and the cost functions
    /// 0x42d9a0 / 0x42d980 not read), the smithy's and studio's sounds (how
    /// 0x50e570 picks among the events' sounds not read), and a ship's
    /// (0x4e2ba0 not read).
    pub fn halt(&mut self, id: u32) -> Result<(), String> {
        let i = self.object_index(id).ok_or("No such object")?;
        let class = self.objects[i].class;
        let forts = matches!(class, ObjectClass::Building(0x34..=0x36));
        let mending = !forts && self.mending.contains(&id);
        match class {
            ObjectClass::Building(SETTLEMENT | TOWN) if self.stock_repair_at(id).is_some() => {
                self.cancel_stock_repair(id)?;
            }
            ObjectClass::Building(WORKSHOP) if !mending && self.research_at(id).is_none() => {
                if let Some(home) = self
                    .home_settlement(i)
                    .filter(|&h| self.stock_repair_at(h).is_some())
                {
                    self.cancel_stock_repair(home)?;
                }
            }
            _ => {}
        }
        if mending {
            self.mending.retain(|&m| m != id);
        }
        Ok(())
    }

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
            // Its nation's call (events 0xdc, 0xdd, 0xdf, 0xde).
            if self.human.is_some_and(|h| (h.nation, h.slot) == owner) {
                let calls: &[&str] = match owner.0 {
                    1 => &["re_o2a", "re_o4a", "re_o6a", "re_o5a"],
                    3 => &["re_o2a", "re_o4a", "re_o5a", "re_o6a"],
                    _ => &[],
                };
                if !calls.is_empty() {
                    let pick = calls[self.gull_rng.next() as usize % calls.len()];
                    let (x, y) = (self.objects[i].x, self.objects[i].y);
                    self.sounds.push((pick, x, y));
                }
            }
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
        let mut work: Vec<(u32, bool)> = self.mending.iter().map(|&id| (id, false)).collect();
        for (i, o) in self.objects.iter().enumerate() {
            let Some(Task::Repair { dock }) = o.ship.as_ref().and_then(|s| s.task) else {
                continue;
            };
            // Mended once alongside the dock.
            match self.object_index(dock) {
                Some(d) if self.alongside(i, d) => work.push((o.id, true)),
                Some(_) => {}
                None => work.push((o.id, true)),
            }
        }
        let mut done = Vec::new();
        for (id, is_ship) in work {
            let Some(i) = self.object_index(id) else {
                done.push(id);
                continue;
            };
            let gone = is_ship
                && !matches!(
                    self.objects[i].ship.as_ref().and_then(|s| s.task),
                    Some(Task::Repair { dock }) if self.object_index(dock).is_some()
                );
            let (Some(owner), Some(c)) = (self.objects[i].owner, self.objects[i].combat.as_ref())
            else {
                done.push(id);
                continue;
            };
            // A limed building's mending stands still: no step, nothing
            // paid (0x425990 calls the step, vtable +0x70 = 0x425100, only
            // while +0x144 is 0).
            if !is_ship && c.lime > 0.0 {
                continue;
            }
            // Never below what it has (a hull above full is left alone).
            let (rate, price) = match self.objects[i].ship.as_ref().filter(|_| is_ship) {
                Some(ship) => (
                    c.max_hull * 0.1 / crate::clock::ticks(DOCK_TICKS),
                    DOCK_REPAIR_PRICE[ship.kind as usize % 12] as f32 / c.max_hull.max(1.0),
                ),
                None => {
                    let cost = match self.objects[i].class {
                        ObjectClass::Building(class) => crate::economy::building_cost(class),
                        _ => 0,
                    };
                    (
                        c.max_hull / crate::clock::ticks(MEND_CYCLE),
                        cost as f32 / c.max_hull.max(1.0),
                    )
                }
            };
            let heal = (rate * dt).min(c.max_hull - c.hull).max(0.0);
            let owed = self.repair_owed.entry(owner).or_default();
            *owed += heal * price;
            let pay = owed.floor() as u32;
            let (amber, _) = self.funds(owner);
            if gone || pay > amber {
                done.push(id);
                continue;
            }
            *self.repair_owed.get_mut(&owner).unwrap() -= pay as f32;
            self.earn(owner, -(pay as i64), 0);
            if is_ship {
                self.mend_units(i, owner);
            }
            let c = self.objects[i].combat.as_mut().unwrap();
            c.hull += heal;
            let whole = c.hull >= c.max_hull;
            if !is_ship {
                self.mend_fort_units(i, owner);
            }
            if whole {
                done.push(id);
            }
        }
        // A brain's ship mended sails out and joins its member (0x5d98).
        let leaving: Vec<u32> = done
            .iter()
            .copied()
            .filter(|&id| {
                self.object_index(id).is_some_and(|i| {
                    matches!(
                        self.objects[i].ship.as_ref().and_then(|s| s.task),
                        Some(Task::Repair { .. })
                    )
                })
            })
            .collect();
        for id in leaving {
            self.brain_repaired(id);
            self.brain_mended(id);
        }
        for &id in &done {
            self.rearm_fort(id);
        }
        for id in done {
            self.mending.retain(|&m| m != id);
            self.unit_mends.retain(|m| m.site != id);
            if let Some(i) = self.object_index(id) {
                if let Some(s) = self.objects[i].ship.as_mut() {
                    if matches!(s.task, Some(Task::Repair { .. })) {
                        s.task = None;
                    }
                }
            }
        }
    }
}
