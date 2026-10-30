//! Standing orders from a warship's behaviour page: scouting and patrols
//! (the manual's "Ship AI Options").

use super::Scene;
use crate::objects::Task;
use openconquest_formats::map::TILE_SIZE;

/// A world rectangle: x0, y0, x1, y1.
type Rect = (f32, f32, f32, f32);

impl Scene {
    /// Sends own ship `ship` to scout the world point `(x, y)` and come back.
    pub fn order_scout(&mut self, ship: u32, x: f32, y: f32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let home = (self.objects[i].x, self.objects[i].y);
        let path = self
            .plan_route(i, x, y)
            .ok_or("The ship cannot sail there")?;
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        s.path = path;
        s.task = Some(Task::Scout { home });
        Ok(())
    }

    /// Sets own ship `ship` patrolling between where it is and `(x, y)`.
    pub fn order_patrol(&mut self, ship: u32, x: f32, y: f32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let from = (self.objects[i].x, self.objects[i].y);
        let path = self
            .plan_route(i, x, y)
            .ok_or("The ship cannot sail there")?;
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        s.path = path;
        s.task = Some(Task::Patrol { from, to: (x, y) });
        Ok(())
    }

    /// Has own ship `ship` guard (or with `attack`, attack) the world
    /// rectangle between the corners `a` and `b`.
    pub fn order_zone(
        &mut self,
        ship: u32,
        a: (f32, f32),
        b: (f32, f32),
        attack: bool,
    ) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        if !self.objects[i].is_armed() {
            return Err("The ship has no weapons".into());
        }
        let rect = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
        let (cx, cy) = ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
        let path = self
            .plan_route(i, cx, cy)
            .ok_or("The ship cannot sail there")?;
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        s.path = path;
        s.task = Some(Task::Zone { rect, attack });
        Ok(())
    }

    /// The enemy nearest to object `i` inside `rect`: ships, and with
    /// `attack` buildings too.
    pub(super) fn zone_target(
        &self,
        i: usize,
        rect: (f32, f32, f32, f32),
        attack: bool,
    ) -> Option<u32> {
        let me = &self.objects[i];
        self.objects
            .iter()
            .filter(|o| {
                (o.ship.is_some() || (attack && o.combat.is_some()))
                    && Self::are_enemies(me, o)
                    && !self.holds_fire(me, o)
                    && (rect.0..=rect.2).contains(&o.x)
                    && (rect.1..=rect.3).contains(&o.y)
                    && o.combat.as_ref().is_none_or(|c| c.phantom <= 0.0)
            })
            .min_by(|a, b| {
                let d = |o: &crate::objects::MapObject| (o.x - me.x).powi(2) + (o.y - me.y).powi(2);
                d(a).total_cmp(&d(b))
            })
            .map(|o| o.id)
    }

    /// Has own merchant `ship` gather `resource` on its own inside the
    /// world rectangle between the corners `a` and `b`.
    pub fn order_gather_zone(
        &mut self,
        ship: u32,
        a: (f32, f32),
        b: (f32, f32),
        resource: crate::objects::Resource,
    ) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        if crate::objects::cargo_capacity(s.kind) <= 0.0 {
            return Err("Only merchant ships can gather".into());
        }
        s.zone = Some((
            (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)),
            resource,
        ));
        s.task = None;
        s.path.clear();
        Ok(())
    }

    /// Has own warship `ship` escort the first other own ship in the world
    /// rectangle between the corners `a` and `b`.
    pub fn order_convoy(&mut self, ship: u32, a: (f32, f32), b: (f32, f32)) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        let ward = self
            .objects
            .iter()
            .find(|o| {
                o.id != ship
                    && o.ship.is_some()
                    && self.is_own(o)
                    && (x0..=x1).contains(&o.x)
                    && (y0..=y1).contains(&o.y)
            })
            .ok_or("Drag a rectangle around one of your ships")?
            .id;
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        s.task = Some(Task::Convoy { ward, range: 2 });
        s.path.clear();
        Ok(())
    }

    /// A scout that comes under fire turns for home.
    pub(super) fn scout_attacked(&mut self, ship: u32) {
        let Some(i) = self.object_index(ship) else {
            return;
        };
        let Some(Task::Scout { home }) = self.objects[i].ship.as_ref().and_then(|s| s.task) else {
            return;
        };
        if let Some(path) = self.plan_route(i, home.0, home.1) {
            let s = self.objects[i].ship.as_mut().unwrap();
            s.path = path;
            s.task = None;
        }
    }

    /// Scouts at their goal head home; patrols turn at each end; zone
    /// guards with nothing to fight return to the zone's middle, and an
    /// attack zone ends when it is clear.
    pub(super) fn run_orders(&mut self) {
        // Escorts keep within their range of their ward, in cells counted
        // as the longer side and half the shorter (0x4a5f30, 0x557440).
        let escorts: Vec<(usize, u32, u8)> = self
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| match o.ship.as_ref()?.task? {
                Task::Convoy { ward, range } => Some((i, ward, range)),
                _ => None,
            })
            .collect();
        for (i, ward, range) in escorts {
            let Some(w) = self.object_index(ward) else {
                self.objects[i].ship.as_mut().unwrap().task = None;
                continue;
            };
            let (wx, wy) = (self.objects[w].x, self.objects[w].y);
            let o = &self.objects[i];
            let t = TILE_SIZE as f32;
            let (dx, dy) = (
                ((o.x / t).floor() - (wx / t).floor()).abs() as i32,
                ((o.y / t).floor() - (wy / t).floor()).abs() as i32,
            );
            let far = (2 * dx.max(dy) + dx.min(dy)) / 2 > range as i32;
            let stale = o
                .ship
                .as_ref()
                .unwrap()
                .path
                .back()
                .is_none_or(|&(px, py)| ((px - wx).powi(2) + (py - wy).powi(2)).sqrt() > 64.0);
            if far && stale {
                if let Some(path) = self.plan_route(i, wx, wy) {
                    self.objects[i].ship.as_mut().unwrap().path = path;
                }
            }
        }
        // Merchants with a gathering zone and nothing to do pick the
        // nearest deposit in it.
        let idle: Vec<(u32, Rect, crate::objects::Resource)> = self
            .objects
            .iter()
            .filter_map(|o| {
                let s = o.ship.as_ref()?;
                let (rect, resource) = s.zone?;
                // Following a ship of the zone, it still looks out for
                // a deposit (0x4ca290 then gives it gathering again).
                let following = matches!(s.task, Some(Task::Convoy { range: 1, .. }));
                // One whose notice its zone has yet to hear waits for it.
                let heard = !self.zone_notices.contains(&o.id);
                (heard && ((s.task.is_none() && s.path.is_empty()) || following))
                    .then_some((o.id, rect, resource))
            })
            .collect();
        for (id, rect, resource) in idle {
            let i = self.object_index(id).unwrap();
            let (x, y) = (self.objects[i].x, self.objects[i].y);
            let deposit = self
                .objects
                .iter()
                .filter(|o| {
                    o.deposit.is_some_and(|d| {
                        d.resource == resource && d.remaining.is_none_or(|r| r > 1e-3)
                    }) && (rect.0..=rect.2).contains(&o.x)
                        && (rect.1..=rect.3).contains(&o.y)
                })
                .min_by(|a, b| {
                    let d = |o: &crate::objects::MapObject| (o.x - x).powi(2) + (o.y - y).powi(2);
                    d(a).total_cmp(&d(b))
                })
                .map(|o| o.id);
            if let Some(d) = deposit {
                let _ = self.order_gather(id, d);
                continue;
            }
            // None left: an idle one follows a ship of its zone picked at
            // random, keeping within a cell of it (0x4ca290: message `*`,
            // 0x4d8b30, priority 0); alone, it sails to the zone's middle.
            if matches!(
                self.objects[i].ship.as_ref().unwrap().task,
                Some(Task::Convoy { .. })
            ) {
                continue;
            }
            let mates: Vec<u32> = self
                .objects
                .iter()
                .filter(|o| {
                    o.id != id
                        && o.owner == self.objects[i].owner
                        && o.ship
                            .as_ref()
                            .is_some_and(|s| s.zone.is_some_and(|z| z.0 == rect))
                })
                .map(|o| o.id)
                .collect();
            if !mates.is_empty() {
                let ward = mates[self.rng.next() as usize % mates.len()];
                let s = self.objects[i].ship.as_mut().unwrap();
                s.task = Some(Task::Convoy { ward, range: 1 });
                s.priority = 0;
                continue;
            }
            let (cx, cy) = ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
            if ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() > 64.0 {
                if let Some(path) = self.plan_route(i, cx, cy) {
                    let s = self.objects[i].ship.as_mut().unwrap();
                    s.path = path;
                    s.priority = 0;
                }
            }
        }
        let zones: Vec<(usize, Rect, bool)> = self
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| match o.ship.as_ref()?.task? {
                Task::Zone { rect, attack } => Some((i, rect, attack)),
                _ => None,
            })
            .collect();
        for (i, rect, attack) in zones {
            if self.zone_target(i, rect, attack).is_some() {
                continue;
            }
            if attack {
                self.objects[i].ship.as_mut().unwrap().task = None;
                continue;
            }
            let (cx, cy) = ((rect.0 + rect.2) / 2.0, (rect.1 + rect.3) / 2.0);
            let o = &self.objects[i];
            let away = ((o.x - cx).powi(2) + (o.y - cy).powi(2)).sqrt() > 64.0;
            if away && o.ship.as_ref().unwrap().path.is_empty() {
                if let Some(path) = self.plan_route(i, cx, cy) {
                    self.objects[i].ship.as_mut().unwrap().path = path;
                }
            }
        }
        let arrived: Vec<(usize, Task)> = self
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| {
                let s = o.ship.as_ref()?;
                let task = s.task?;
                (s.path.is_empty() && matches!(task, Task::Scout { .. } | Task::Patrol { .. }))
                    .then_some((i, task))
            })
            .collect();
        for (i, task) in arrived {
            let (goal, next) = match task {
                Task::Scout { home } => (home, None),
                Task::Patrol { from, to } => (from, Some(Task::Patrol { from: to, to: from })),
                _ => continue,
            };
            let path = self.plan_route(i, goal.0, goal.1);
            let s = self.objects[i].ship.as_mut().unwrap();
            match path {
                Some(p) => {
                    s.path = p;
                    s.task = next;
                }
                None => s.task = None,
            }
        }
    }

    /// The gather zones' queues (0x4ca8d0 -> 0x4c9e30, the fleet tick):
    /// each notice 0x138, newest first, for a ship still afloat and a
    /// merchant: 0x4c9ae0. With room left (0x4ad280 not 0) the zone looks
    /// for a deposit for it (0x4c9930: not ported, so nothing is found);
    /// then its port: its own `+0x7ad` if that is in the zone's list and
    /// one of its player's ports (0x4c64b0, 0x4ac050), else the zone's pick
    /// 0x4c6640; command 0x1f `{port, 1, 0x5a}` (0x4c89b0, through
    /// 0x520cc0: delivered at the end of the tick). No port: nothing is
    /// sent and the ship lies idle. (Not ported: the zone's deposit picks
    /// 0x4c96e0 / 0x4c5b90 / 0x4c8f20 and notice 0x139 after unloading,
    /// after which the ship goes back to its deposit on its own here.)
    pub(super) fn run_zone_notices(&mut self) {
        for id in std::mem::take(&mut self.zone_notices).into_iter().rev() {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let Some(s) = self.objects[i].ship.as_ref() else {
                continue;
            };
            let home = s.home_port;
            let listed = |sc: &mut Scene| sc.zone_port_list(i).contains(&home);
            let own = |sc: &Scene| {
                sc.object_index(home).is_some_and(|p| {
                    sc.objects[p].owner == sc.objects[i].owner && sc.takes_cargo(&sc.objects[p])
                })
            };
            let port = if home != 0 && listed(self) && own(self) {
                Some(home)
            } else {
                self.zone_port_pick(i)
            };
            if let Some(port) = port {
                self.post(super::Order::Unload { ship: id, port });
            }
        }
    }

    /// The zone's port list (block `+0x28`): filled when there is none
    /// (0x4c6c30: over the map's cells in order, each of the player's
    /// ports not asleep, `+0xa4 != 0x10`, by the first cell found; for
    /// player 16 its class-0x40 structures), then pruned (0x4c65c0, last
    /// to first: what is not a port, or whose vt+0xb4 0x40e020 fails: no
    /// facing, or a state `+0xa4` other than 1 standing, being built 0,
    /// mended 2, asleep 0x10). (Not identified: the flag `+0x1d8 & 0x10`
    /// with `+0x1c8 == 0` that 0x40e020 also refuses.)
    fn zone_port_list(&mut self, i: usize) -> Vec<u32> {
        use crate::economy::PORT;
        use openconquest_formats::ObjectClass;
        let owner = self.objects[i].owner;
        let filled = self.objects[i]
            .ship
            .as_ref()
            .and_then(|s| s.zone_ports.clone())
            .filter(|l| !l.is_empty());
        let list = filled.unwrap_or_else(|| {
            let barbarian = owner == Some((4, 0));
            let mut found: Vec<((i32, i32), u32)> = self
                .objects
                .iter()
                .filter(|o| {
                    o.owner == owner
                        && if barbarian {
                            o.class == ObjectClass::Barbarian(0x40)
                        } else {
                            o.class == ObjectClass::Building(PORT) && o.asleep.is_none()
                        }
                })
                .filter_map(|o| Some(((o.cell?.1, o.cell?.0), o.id)))
                .collect();
            found.sort();
            found.into_iter().map(|(_, id)| id).collect()
        });
        let list: Vec<u32> = list
            .into_iter()
            .filter(|&id| {
                self.object_index(id).is_some_and(|p| {
                    let o = &self.objects[p];
                    o.class == ObjectClass::Building(PORT)
                        && self.berth_cell(p).is_some()
                        && o.asleep.is_none()
                        && self.construction_at(id).is_none()
                        && !self.mending.contains(&id)
                })
            })
            .collect();
        if let Some(s) = self.objects[i].ship.as_mut() {
            s.zone_ports = Some(list.clone());
        }
        list
    }

    /// The zone's port for ship `i` (0x4c6640): its only port, reachable or
    /// not; of more, by the wavefront 0x4a1e90 from the ship (limit 5000,
    /// over its draught's grid) at each berth, the reached ones ranked
    /// (at most 9, a port before those with a lower value), their values
    /// less the least (at least 1); of those within 70% of the first's
    /// (`s * 100 / (s0 + 1) >= 70`), the one with the fewest of the
    /// player's gathering ships bound to it (`+0x7ad`, 0x4c7ea0: here its
    /// merchants with that port), the first on a tie (the comparison
    /// taking the loads in list order, as 0x4c69c6 reads them). None
    /// reached: the list's first. (Not decoded: 0x49a330's test of a
    /// crowded berth, 0x49a1d0 and 0x4999e0, after which the next ranked
    /// port with fewer ships bound is taken: left out.)
    fn zone_port_pick(&mut self, i: usize) -> Option<u32> {
        let list = self.zone_port_list(i);
        match list.len() {
            0 => return None,
            1 => return Some(list[0]),
            _ => {}
        }
        let owner = self.objects[i].owner;
        let load: Vec<i32> = list
            .iter()
            .map(|&p| {
                self.objects
                    .iter()
                    .filter(|o| {
                        o.owner == owner
                            && o.ship
                                .as_ref()
                                .is_some_and(|s| s.kind < 2 && s.home_port == p)
                    })
                    .count() as i32
            })
            .collect();
        let t = TILE_SIZE as f32;
        let ship = self.objects[i].ship.as_ref()?;
        let deep = crate::objects::deep_draught(ship.kind);
        let at = (
            (self.objects[i].x / t).floor() as i32,
            (self.objects[i].y / t).floor() as i32,
        );
        let a = self.nav.wavefront(at, deep, 5000)?;
        let w = self.nav.width as i32;
        let berth = |sc: &Scene, id: u32| {
            sc.object_index(id)
                .and_then(|p| sc.berth_cell(p))
                .map(|(x, y)| (y * w + x) as usize)
        };
        let mut least = berth(self, list[0]).map_or(0, |b| a[b] as i32);
        // (index in the list, score, load)
        let mut ranked: Vec<(usize, i32, i32)> = Vec::new();
        for (k, &id) in list.iter().enumerate() {
            let Some(b) = berth(self, id) else {
                continue;
            };
            let v = a[b] as i32;
            if v < 1 {
                continue;
            }
            least = least.min(v);
            let e = (k, v + 1, load[k]);
            match ranked.iter().position(|r| r.1 < v) {
                Some(j) => {
                    ranked.insert(j, e);
                    ranked.truncate(9);
                }
                None if ranked.len() < 9 => ranked.push(e),
                None => {}
            }
        }
        if ranked.is_empty() {
            return Some(list[0]);
        }
        for r in &mut ranked {
            if r.1 > least {
                r.1 -= least;
            } else if r.1 > 0 {
                r.1 = 1;
            }
        }
        let first = ranked[0].1;
        let within = ranked
            .iter()
            .filter(|r| r.1 * 100 / (first + 1) > 0x45)
            .count();
        let mut lo = ranked[0].2;
        let mut pick = 0;
        for (j, r) in ranked.iter().enumerate().take(within) {
            if r.2 < lo {
                lo = load[j];
                pick = j;
            }
        }
        Some(list[ranked[pick].0])
    }
}
