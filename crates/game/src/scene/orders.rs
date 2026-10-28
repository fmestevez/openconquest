//! Standing orders from a warship's behaviour page: scouting and patrols
//! (the manual's "Ship AI Options").

use super::Scene;
use crate::objects::Task;

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
        s.task = Some(Task::Convoy { ward });
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
        // Escorts keep within two cells of their ward.
        let escorts: Vec<(usize, u32)> = self
            .objects
            .iter()
            .enumerate()
            .filter_map(|(i, o)| match o.ship.as_ref()?.task? {
                Task::Convoy { ward } => Some((i, ward)),
                _ => None,
            })
            .collect();
        for (i, ward) in escorts {
            let Some(w) = self.object_index(ward) else {
                self.objects[i].ship.as_mut().unwrap().task = None;
                continue;
            };
            let (wx, wy) = (self.objects[w].x, self.objects[w].y);
            let o = &self.objects[i];
            let far = ((o.x - wx).powi(2) + (o.y - wy).powi(2)).sqrt() > 2.0 * 64.0;
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
                (s.task.is_none() && s.path.is_empty()).then_some((o.id, rect, resource))
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
}
