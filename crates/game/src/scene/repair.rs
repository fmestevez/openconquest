//! Repairs: ships at a dock, buildings by a settlement or town (the
//! manual's "Repairing a Ship" and "Repairing a Terrain Object").

use openconquest_formats::ObjectClass;

use super::Scene;
use crate::economy::{DOCK, SETTLEMENT, TOWN};
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
/// 0x441ba0): the time it takes and how long it has been at it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StockRepair {
    pub site: u32,
    pub elapsed: f32,
    pub seconds: f32,
}

impl StockRepair {
    /// How far along it is, 0-100 (`+0x120`).
    pub fn done(&self) -> u32 {
        if self.seconds <= 0.0 {
            return 100;
        }
        ((self.elapsed / self.seconds) * 100.0).min(100.0) as u32
    }
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
        let t = self
            .object_index(target)
            .ok_or("Choose one of your buildings")?;
        let (home, obj) = (&self.objects[s], &self.objects[t]);
        if !matches!(home.class, ObjectClass::Building(SETTLEMENT | TOWN)) || !self.is_own(home) {
            return Err("Only your settlements and towns repair".into());
        }
        if !matches!(obj.class, ObjectClass::Building(_)) || !self.is_own(obj) {
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
        }
        Ok(())
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
        if self.stock_repairs.iter().any(|r| r.site == home) || self.training_at(home).is_some() {
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
        self.stock_repairs.push(StockRepair {
            site: home,
            elapsed: 0.0,
            seconds: crate::clock::ticks(ticks),
        });
        Ok(())
    }

    /// The stock repair going on at settlement `site`, if any.
    pub fn stock_repair_at(&self, site: u32) -> Option<&StockRepair> {
        self.stock_repairs.iter().find(|r| r.site == site)
    }

    /// Settlements mending their stock: at the end every worn unit is
    /// whole.
    fn run_stock_repairs(&mut self, dt: f32) {
        let mut done = Vec::new();
        for r in &mut self.stock_repairs {
            r.elapsed += dt;
            if r.elapsed >= r.seconds {
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
            if c.hull >= c.max_hull {
                done.push(id);
            }
        }
        for id in done {
            self.mending.retain(|&m| m != id);
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
