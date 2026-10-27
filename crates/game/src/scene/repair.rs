//! Repairs: ships at a dock, buildings by a settlement or town (the
//! manual's "Repairing a Ship" and "Repairing a Terrain Object").

use openac_formats::map::TILE_SIZE;
use openac_formats::ObjectClass;

use super::Scene;
use crate::economy::{DOCK, SETTLEMENT, TOWN};
use crate::objects::Task;

/// Hull points a settlement mends on a building a second, and amber paid
/// per point. Placeholders.
pub const REPAIR_RATE: f32 = 40.0;
pub const REPAIR_AMBER_PER_POINT: f32 = 0.2;
/// At a dock a ship gets up to a tenth of its hull back every 9 ticks, for
/// its type's repair price (0x59b388) times the share mended (0x4b1fc0).
const DOCK_TICKS: u32 = 9;
const DOCK_REPAIR_PRICE: [u32; 12] = [140, 280, 800, 910, 650, 560, 490, 420, 420, 630, 420, 630];
/// How near (cells) a ship must lie to the dock, and a building to the
/// settlement or town mending it. Placeholders.
pub const DOCK_REACH: f32 = 2.6;
pub const MEND_REACH: f32 = 8.0;

impl Scene {
    /// Orders own ship `ship` to sail to own dock `dock` and be repaired.
    pub fn order_repair(&mut self, ship: u32, dock: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let d = self.object_index(dock).ok_or("Choose one of your docks")?;
        if self.objects[d].class != ObjectClass::Building(DOCK)
            || !self.is_own(&self.objects[d])
            || self.construction_at(dock).is_some()
        {
            return Err("Choose one of your docks".into());
        }
        let c = self.objects[i].combat.as_ref().ok_or("Not a ship")?;
        if c.hull >= c.max_hull {
            return Err("The ship needs no repair".into());
        }
        let (x, y) = (self.objects[d].x, self.objects[d].y);
        let path = self
            .plan_route(i, x, y)
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
        let d = ((home.x - obj.x).powi(2) + (home.y - obj.y).powi(2)).sqrt();
        if d > MEND_REACH * TILE_SIZE as f32 {
            return Err("Too far from the settlement".into());
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

    /// Ships at their docks and buildings being mended get their strength
    /// back, at the owner's expense, until whole or the money runs out.
    pub(super) fn run_repairs(&mut self, dt: f32) {
        let t = TILE_SIZE as f32;
        let mut work: Vec<(u32, bool)> = self.mending.iter().map(|&id| (id, false)).collect();
        for o in &self.objects {
            let Some(Task::Repair { dock }) = o.ship.as_ref().and_then(|s| s.task) else {
                continue;
            };
            match self.object_index(dock) {
                Some(d) => {
                    let dd = &self.objects[d];
                    let near =
                        ((o.x - dd.x).powi(2) + (o.y - dd.y).powi(2)).sqrt() <= DOCK_REACH * t;
                    if near {
                        work.push((o.id, true));
                    }
                }
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
                None => (REPAIR_RATE, REPAIR_AMBER_PER_POINT),
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
