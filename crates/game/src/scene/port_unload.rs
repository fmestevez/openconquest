//! A ship's unload at a port (Run_Ship.cpp 0x4adef0: the unloading order
//! 0xc, event 0x40 `{400, 0xc, port, prio}`, 0x4aab00), a game tick at a
//! time, its step `+0x77d`:
//!
//! - **1**: on its berth `+0x7bd` (the port's vt+0xb8) and lying still
//!   (`+0x58c` 0), the port `+0x7b1` still standing on its cell, its
//!   vt+0xb0 (0x40dfe0) true and the close-in 0x4afe50(ship, 1) on the
//!   port's cell true: the contact `+0x8e6` {0x32, 5, port id, port} made
//!   (no 0xcb7 sent), or a contact of kind 0x32 with another object freed
//!   (0xcb8 to it, 0x4e06a0) and the step ended, another kind waited on;
//!   `+0x78e` and `+0x79f` 0, step 10 and the commands held (0x4b37d0
//!   with 0x2000, 0x4ae191); the port's slot empty there, nothing. Off
//!   the berth: the wait by a crowded berth (`Scene::run_berth_waits`);
//!   then, every tick, under way too: the port's anchor `+0x7b5` with its
//!   slot empty, the ship sends itself to 0x4c5910's port (event 0x40
//!   `{400, 0xc, id, player, 0x5a}`), none, the stop (event 8 0), both
//!   read at its next tick (`Scene::post_event`); the slot holding its
//!   port `+0x7ad`, a plan to that port's berth (0x4a6270 mode 5); held
//!   by another (`+0x7ad` 0 after the berth wait's give-up), nothing.
//!   Player 16's
//!   barbarian structure (class 0x40, vtable 0x594920) has the ports'
//!   vt+0xb0 0x40dfe0 and vt+0xb8 0x40e050.
//! - **10** (0x4b0470(1) -> 0x4b02a0), no contact the order called off:
//!   the slide, two points a tick; at its end step 0xb; called off and
//!   back at its start, the contact freed (0xcb8 to the port), the commands
//!   back (0x4b3850) and event 8 0xc0.
//! - **0xb** (0x4b89b0): the sail furls a frame a hull step; furled
//!   (`+0x25b` 5), the walkway (slot 1, 0x3e6, a frame a tick) at the hull
//!   picture's top-left plus 0x59a8e8[type][0x4b8a40][direction] and step
//!   0x10.
//! - **0x10**: called off (or no contact), down a frame a tick and below 1
//!   step 0x13; else up a frame a tick, at its last the porters (slot 3,
//!   0x3de, direction `+0x79c`) on the walkway's top-left and step 0x11.
//! - **0x11**: not called off and in contact, `+0x58c` = 6 and 0x4add10:
//!   the porters' frame a tick; at their last the goods handed over
//!   (0x4ad9f0, `Scene::unload_at`), notice 0x13f (0x4a1980), the porters
//!   gone and step 0xe. Else the porters gone and step 0xe.
//! - **0xe**: the walkway down a frame a tick (the map's `+0xe2`); below
//!   0, step 0x13.
//! - **0x13**: two points back along the line a hull step, unfurling; at
//!   its start the hull on its cell's middle, the contact freed (0xcb8:
//!   the port's `+0x1e0` cleared), the commands back (0x4b3850), the route
//!   dropped (0x4a5b70), event 8 0x40, a cell off the berth (0x4b46f0
//!   with '[') and notice 0x139 to its group (0x4a1520).
//!
//! The slide, furl and back-out are the treasure take's; the machine runs
//! in the ship's `Landing` (its `state` the step, `dock` the contact),
//! whose presence holds the commands (`Scene::landing_holds`), the task in
//! `GatherPhase::Unloading` meanwhile.

use super::*;
use crate::objects::{GatherPhase, Task};

/// 0x4a6150's wait between plans (`+0x57f`, 0x19 from 0x4aab00).
const PLAN_TICKS: u32 = 25;

impl Scene {
    /// Every ship's unload at a port, a game tick (0x4adef0).
    pub(super) fn run_unloads(&mut self) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship.as_ref().is_some_and(|s| {
                    matches!(
                        s.task,
                        Some(Task::Gather {
                            phase: GatherPhase::ToPort(_) | GatherPhase::Unloading(_),
                            ..
                        })
                    )
                })
            })
            .map(|o| o.id)
            .collect();
        for id in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let s = self.objects[i].ship.as_ref().unwrap();
            match s.task {
                Some(Task::Gather {
                    phase: GatherPhase::Unloading(port),
                    ..
                }) if s.landing.is_some() => self.unload_landing_tick(i, port),
                Some(Task::Gather {
                    phase: GatherPhase::ToPort(port),
                    ..
                }) => self.unload_step_one(i, port),
                _ => {}
            }
        }
    }

    /// The unload's anchor and berth cells (`+0x7b5`, `+0x7bd`, set by
    /// 0x4aab00 code 0xc), as kept; from a save before they were, the
    /// port's while it stands.
    pub(super) fn unload_cells(&self, i: usize, port: u32) -> Option<((i32, i32), (i32, i32))> {
        let s = self.objects[i].ship.as_ref()?;
        s.unload_cells.or_else(|| {
            let p = self.object_index(port)?;
            Some((self.objects[p].cell?, self.berth_cell(p)?))
        })
    }

    /// 0x4adef0 step 1 for ship `i` bound for `port`. On the berth
    /// `+0x7bd` (0x4adef0 at 0x4adf1a) it lands; the port's slot empty
    /// there, nothing (the test of `+0x7b1` against the slot fails).
    /// Off it (the berth wait is `run_berth_waits`'), the slot of the
    /// anchor `+0x7b5` (0x5c74fc `+0xc`): empty, 0x4c5910's port gets the
    /// event 0x40 `{400, 0xc, id, player, 0x5a}`, none the stop (vt+4(8,
    /// 0)); holding the port `+0x7ad`, `+0x7b1` = it and the plan to its
    /// berth (0x4a6270 mode 5, vt+0xb8); held by another, nothing. (A
    /// save without the cells: none known, the slot is taken as empty.)
    fn unload_step_one(&mut self, i: usize, port: u32) {
        let cells = self.unload_cells(i, port);
        if cells.is_none_or(|(_, berth)| self.cell_of(i) != berth) {
            let id = self.objects[i].id;
            match cells.and_then(|(anchor, _)| self.slot_object(anchor)) {
                None => match self.unloading_site(i) {
                    Some(port) => self.post_event(super::Order::UnloadEvent {
                        ship: id,
                        port,
                        prio: 0x5a,
                    }),
                    None => self.post_event(super::Order::StopEvent { ship: id }),
                },
                Some(k)
                    if self.objects[k].id == self.objects[i].ship.as_ref().unwrap().home_port =>
                {
                    if let Some(b) = self.berth_cell(k) {
                        self.unload_plan(i, b);
                    }
                }
                Some(_) => {}
            }
            return;
        }
        if self.object_index(port).is_none() {
            return;
        }
        let s = self.objects[i].ship.as_ref().unwrap();
        if !s.path.is_empty() || s.move_state != 0 {
            return;
        }
        let Some(p) = self.object_index(port) else {
            return;
        };
        let Some(cell) = self.objects[p].cell else {
            return;
        };
        if !self.takes_hook(p) {
            return;
        }
        let Some(mut l) = self.close_in_on(i, cell, true) else {
            return;
        };
        // A contact with something else (`+0x8e6`): kind 0x32 is freed
        // (0x4b3970(0): 0xcb8 to it, its `+0x1e0` cleared; 0x4e06a0) and
        // the step ends, to make the port's the next tick; another kind
        // waits. A building's 0x32 left by a load run whose order went
        // before the hold (0x4b8d10 frees the line, not `+0x8e6`) is its
        // `occupants` entry.
        let id = self.objects[i].id;
        if let Some(&(other, _)) = self.occupants.iter().find(|o| o.1 == id && o.0 != port) {
            self.occupants.retain(|o| o.0 != other);
            return;
        }
        if self.in_contact(i) {
            return;
        }
        l.dock = Some(port);
        let s = self.objects[i].ship.as_mut().unwrap();
        s.berth_wait = 0;
        if let Some(Task::Gather { phase, .. }) = s.task.as_mut() {
            *phase = GatherPhase::Unloading(port);
        }
        s.landing = Some(l);
    }

    /// 0x4adef0 step 1's plan to the berth `goal` (0x4a6270 mode 5 ->
    /// 0x4a6150, the first at once with `+0x593`, set by the order): once
    /// 25 ticks have passed since the last plan (`+0x57b`), if the ship
    /// changed cells, the goal changed or it lies still, and not turning
    /// (move state 2). (0x4a6150's goal-is-its-cell end is not met: off
    /// the berth. Its wait counter `+0x591` is the port's own mover's.)
    /// Then 0x4a6330: the route stored if found; the plan's tick and cell
    /// kept, its goal only with a route (0x4a6100).
    fn unload_plan(&mut self, i: usize, goal: (i32, i32)) {
        let now = self.ticks;
        let own = self.cell_of_object(i);
        let s = self.objects[i].ship.as_ref().unwrap();
        let p = s.unload_plan;
        if !p.first_plan {
            let changed = own != p.plan_from || goal != p.plan_goal || s.move_state == 0;
            if !changed || s.move_state == 2 || now.wrapping_sub(p.planned) < PLAN_TICKS {
                return;
            }
        }
        let t = TILE_SIZE as f32;
        let (x, y) = (goal.0 as f32 * t + t / 2.0, goal.1 as f32 * t + t / 2.0);
        let route = self.plan_route_to_water(i, x, y);
        let plan_goal = if route.is_some() { goal } else { p.plan_goal };
        if let Some(path) = route {
            self.store_route(i, path);
        }
        self.objects[i].ship.as_mut().unwrap().unload_plan = crate::objects::RoutePlan {
            planned: now,
            plan_from: own,
            plan_goal,
            first_plan: false,
        };
    }

    /// The contact freed (0x4b3970(0): 0xcb8 to the port, 0x4e7640 clears
    /// its `+0x1e0` whatever ship it holds).
    fn unload_free_contact(&mut self, port: u32) {
        self.occupants.retain(|o| o.0 != port);
    }

    /// 0x4adef0 past step 1 for ship `k`, its machine in its landing.
    pub(super) fn unload_landing_tick(&mut self, k: usize, port: u32) {
        let s = self.objects[k].ship.as_ref().unwrap();
        let Some(mut l) = s.landing.clone() else {
            return;
        };
        let kind = s.kind as usize % 12;
        let heading = s.heading;
        l.hull_ticks += 1;
        let hull_step = l.hull_ticks > super::landing::HULL_DELAY;
        let id = self.objects[k].id;
        // The contact `+0x8e6`: the port while it stands.
        let contact = l.dock.is_some_and(|d| self.object_index(d).is_some());
        match l.state {
            10 => {
                if !contact {
                    l.abort = true;
                }
                if self.close_in_step(k, &mut l, 2) {
                    if l.abort {
                        if contact {
                            self.unload_free_contact(port);
                        }
                        let s = self.objects[k].ship.as_mut().unwrap();
                        s.furl = 0.0;
                        s.rotation = None;
                        s.landing = None;
                        s.task = None;
                        return;
                    }
                    l.state = 0xb;
                }
            }
            0xb => {
                if self.furl_step(k, &mut l, hull_step) {
                    if !contact {
                        l.abort = true;
                    }
                    let o = &self.objects[k];
                    let half = objects::SHIP_FRAME[kind] / 2.0;
                    let dir = l.walk_dir as usize % 8;
                    let side = unload::walkway_side(dir, heading);
                    let (dx, dy) = unload::WALKWAY_AT[kind.min(4)][side][dir];
                    l.plank_at = (o.x - half + dx as f32, o.y - half + dy as f32);
                    l.plank = Some(0);
                    l.back = 0;
                    l.state = 0x10;
                }
            }
            0x10 => {
                if !contact {
                    l.abort = true;
                }
                let f = l.plank.unwrap_or(0);
                if l.abort {
                    if f > 0 {
                        l.plank = Some(f - 1);
                    } else {
                        l.plank = None;
                        l.back = l.count;
                        l.state = 0x13;
                    }
                } else {
                    let n = self.landing_art[l.walk_dir as usize % 8][0].len() as i32;
                    if n <= f + 1 {
                        l.plank = Some(n - 1);
                        l.porters = Some(0);
                        l.state = 0x11;
                    } else {
                        l.plank = Some(f + 1);
                    }
                }
            }
            0x11 => {
                if !l.abort && contact {
                    self.objects[k].ship.as_mut().unwrap().move_state = 6;
                    let n = self.landing_art[l.dir as usize % 8][1].len() as i32;
                    let f = l.porters.unwrap_or(0) + 1;
                    if n <= f {
                        self.unload_at(k, port);
                        // Notice 0x13f with `+0x6cb` (a load order's
                        // target; not carried for a gatherer: 0).
                        self.post_notice(id, 0x13f, 0);
                        l.porters = None;
                        l.state = 0xe;
                    } else {
                        l.porters = Some(f);
                    }
                } else {
                    l.porters = None;
                    l.state = 0xe;
                }
            }
            0xe => {
                let f = l.plank.unwrap_or(0) - 1;
                if f < 0 {
                    l.plank = None;
                    l.back = l.count;
                    l.state = 0x13;
                } else {
                    l.plank = Some(f);
                }
            }
            0x13 => {
                let kept = self.objects[k].ship.as_ref().unwrap().close_in;
                if self.back_out_step(k, &mut l, hull_step) {
                    if contact {
                        self.unload_free_contact(port);
                    }
                    let s = self.objects[k].ship.as_mut().unwrap();
                    s.close_in = kept;
                    s.path.clear();
                    s.move_state = 0;
                    s.task = None;
                    self.off_the_berth(k, 91);
                    self.post_notice(id, 0x139, 0);
                    return;
                }
            }
            _ => {}
        }
        self.objects[k].ship.as_mut().unwrap().landing = Some(l);
    }
}
