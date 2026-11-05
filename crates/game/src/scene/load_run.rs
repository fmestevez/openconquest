//! The load's hook-to-link machine (0x4b73a0 states 7, 8, 10, 11, 4, 12
//! and 6; Run_Ship.cpp).
//!
//! A loader within reach of its target (state 1, `Scene::load_tick`)
//! throws a hook (0x4b84b0). Taken (at a building 0x4bc140, at a ship
//! 0x4bbfc0), it holds its commands, slides alongside and turns parallel
//! (0x4b54a0, 0x4b57e0), furls (0x4b8aa0), lowers its walkway `MOST0_` and
//! is linked for reloading (0x2875). Released (a stop, Finish reload, the
//! target lost) it raises the walkway, slides back to its cell's middle
//! and unfurls, and the order ends with notice 0x13f.

use super::grapple::{
    alongside_offset, direction, parallel, slide, turn_tick, FURL_FRAMES, FURL_STEP, PLANK_FRAMES,
};
use super::landing::line_point;
use super::shot;
use super::*;
use crate::objects::{GrappleStage, LoadContact, LoadRun, LoadStage};

/// The ports, yards and docks (0x39-0x3b), whose berth must be the
/// ship's cell, and the forts (0x34-0x36), reached within 1 of their
/// anchor.
fn is_port_like(class: u32) -> bool {
    (0x39..=0x3b).contains(&class)
}
fn is_fort(class: u32) -> bool {
    (0x34..=0x36).contains(&class)
}

/// 0x557440: the larger axis plus half the smaller (integer).
fn dist(a: (i32, i32), b: (i32, i32)) -> i32 {
    let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    if dy <= dx {
        (dy + dx * 2) / 2
    } else {
        (dx + dy * 2) / 2
    }
}

/// A line's point count from `from` to `to` (0x4b5220: one a pixel of the
/// longer side and the end, padded by one to an odd count).
fn line_count(from: (f32, f32), to: (f32, f32)) -> u32 {
    let major = (to.0 - from.0).abs().max((to.1 - from.1).abs()).round() as u32;
    (major + 1) | 1
}

impl Scene {
    /// Whether ship `id` holds its commands for a load (0x4b37d0 with
    /// 0x2000 at the hold, till 0x4b3850 at state 6's end): all but the
    /// stop are dropped.
    pub fn load_holds(&self, id: u32) -> bool {
        self.object_index(id)
            .and_then(|i| self.objects[i].ship.as_ref())
            .and_then(|s| s.load_run)
            .is_some_and(|r| r.stage != LoadStage::Hook)
    }

    /// Building `t`'s ship taken in (`+0x1e0`).
    pub(super) fn occupant(&self, t: u32) -> Option<u32> {
        self.occupants.iter().find(|o| o.0 == t).map(|o| o.1)
    }

    /// Building `t`'s vt+0xb0 (0x40dfe0 for ports, yards, docks and
    /// forts): it takes a hook when it faces water (`+0x1dc` >= 0, here
    /// its berth), stands (`+0xa4` 1) and holds no ship (`+0x1e0` 0).
    /// (Not ported: the town test for the class flag 0x10 of `+0x1d8`,
    /// `+0x1c8` != 0.)
    fn takes_hook(&self, t: usize) -> bool {
        let id = self.objects[t].id;
        self.vt_berth(t) != (-1, -1) && !self.fallen(id) && self.occupant(id).is_none()
    }

    /// The loader within reach throws its hook (0x4b84b0 from state 1,
    /// with no enemy test): not before its last hook's reload
    /// (0x5c7740[player byte +0x90], `Scene::hook_reload_ticks`), nor while
    /// it holds a contact (`+0x8e6`). At a building its anchor's direction
    /// (0x59791d), a port's, yard's or dock's berth (vt+0xb8) the ship's
    /// cell, the distance 0; at a ship one taking contacts (`+0x261`
    /// 0x1000), the distance 0x557440 between the hull centres. The chance
    /// is 100 (item 9, 0x4bf7f0, no draw), 0 across a blocked corner
    /// (0x499ca0) or at a ship slipping away (0x4b5140); the hook's time
    /// `+0x6c2` is 0x598de4[distance / 10] / 19 ticks. A building's aim is
    /// its vt+0xc (0x423520), aimed again for a miss when the point falls
    /// out of the hook's cell `+0x6d7`. Returns whether it threw (state 7).
    pub(super) fn load_throw(
        &mut self,
        i: usize,
        target: u32,
        data: &GameData,
        caches: &mut Caches,
    ) -> bool {
        let now = self.ticks;
        let c = self.objects[i].ship.as_ref().unwrap().closing.unwrap();
        if c.thrown
            .is_some_and(|t| now.wrapping_sub(t) < self.hook_reload_ticks(i))
        {
            return false;
        }
        if self.in_contact(i) {
            return false;
        }
        let Some(t) = self.object_index(target) else {
            return false;
        };
        let own = self.cell_of_object(i);
        let ship_target = self.objects[t].ship.is_some();
        let mut hook_cell = c.hook_cell;
        let (cell, d) = if ship_target {
            if !self.takes_contact(t) {
                return false;
            }
            let (a, b) = (&self.objects[i], &self.objects[t]);
            let d = dist((a.x as i32, a.y as i32), (b.x as i32, b.y as i32));
            (self.cell_of_object(t), d)
        } else {
            let ObjectClass::Building(class) = self.objects[t].class else {
                return false;
            };
            if is_port_like(class) && self.vt_berth(t) != own {
                return false;
            }
            (self.anchor_cell(t), 0)
        };
        let mut strikes = self.corner_free_cells(own, cell);
        if strikes {
            if ship_target && self.slipping(t) {
                strikes = false;
            }
            hook_cell = cell;
        }
        let flight = shot::ARC_LENGTH[(d / 10).clamp(0, 47) as usize] / 19;
        let from = (self.objects[i].x, self.objects[i].y);
        let to = if ship_target {
            None
        } else {
            let mut p = self.building_aim(t, strikes);
            if (p.0 >> 6, p.1 >> 6) != hook_cell {
                strikes = false;
                p = self.building_aim(t, false);
            }
            Some((p.0 as f32, p.1 as f32))
        };
        let id = self.objects[i].id;
        let s = self.objects[i].ship.as_mut().unwrap();
        let cl = s.closing.as_mut().unwrap();
        cl.thrown = Some(now);
        cl.hook_cell = hook_cell;
        s.load_run = Some(LoadRun {
            target,
            building: !ship_target,
            stage: LoadStage::Hook,
            contact: LoadContact::Flying,
            released: false,
            thrown: now,
            flight,
            at: from,
            home: from,
            from,
            face: 0,
            facing: 0,
            point: 0,
            count: 0,
            ticks: 0,
            plank: -1,
            sail: -1,
            rope: false,
            hook_at: to.unwrap_or(from),
        });
        self.fire_shot(
            shot::Firing {
                item: shot::HOOK,
                letter: None,
                flaming: false,
                from,
                target,
                strikes,
                damage: 0.0,
                wounds: 0.0,
                wear: 0.0,
                rope: Some(id),
                shooter: None,
                by: None,
                to,
            },
            data,
            caches,
        );
        true
    }

    /// A loader's hook lands, on `target` if `hit`; true if it was a
    /// loader's. At a building (0x138e -> 0x423d60 -> 0x4bc140) it is
    /// taken when the building takes a hook (vt+0xb0): the contact {0x32,
    /// 4}, the building's `+0x1e0` the ship (0xcb7, 0x4e7640) and the line
    /// moved to the ship. At a ship (0x4bbfc0) one free of contacts that
    /// takes them, with no player test: it is grappled as a boarded ship
    /// (prey state 9). Else the hook falls in the water (0x4983d0) and
    /// the contact is freed.
    pub(super) fn load_hook_landed(&mut self, from: u32, target: u32, hit: bool) -> bool {
        let Some(i) = self.object_index(from) else {
            return false;
        };
        let Some(mut r) = self.objects[i]
            .ship
            .as_ref()
            .and_then(|s| s.load_run)
            .filter(|r| r.contact == LoadContact::Flying && r.target == target)
        else {
            return false;
        };
        r.contact = LoadContact::Free;
        let t = self.object_index(target).filter(|_| hit);
        match t {
            Some(t) if r.building => {
                if self.takes_hook(t) {
                    r.contact = LoadContact::Taken;
                    r.rope = true;
                    self.occupants.push((target, from));
                }
            }
            Some(t) if !self.in_contact(t) && self.takes_contact(t) => {
                r.contact = LoadContact::Taken;
                self.grapple_prey(t, i);
            }
            _ => {}
        }
        self.objects[i].ship.as_mut().unwrap().load_run = Some(r);
        true
    }

    /// Loaders past their throw, a game tick at a time.
    pub(super) fn run_load_runs(&mut self) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.load_run.is_some()))
            .map(|o| o.id)
            .collect();
        for id in ids {
            if let Some(k) = self.object_index(id) {
                self.load_run_tick(k);
            }
        }
    }

    fn load_run_tick(&mut self, k: usize) {
        use LoadStage::*;
        let id = self.objects[k].id;
        let s = self.objects[k].ship.as_ref().unwrap();
        let mut r = s.load_run.unwrap();
        // The order gone before the hold (another order, a stop) takes
        // the run with it (0x4b8d10).
        if r.stage == Hook && !matches!(s.task, Some(Task::LoadAt { .. } | Task::LoadRuins { .. }))
        {
            self.objects[k].ship.as_mut().unwrap().load_run = None;
            return;
        }
        let now = self.ticks;
        // The target gone frees the contact.
        if r.contact == LoadContact::Taken && self.object_index(r.target).is_none() {
            r.contact = LoadContact::Free;
        }
        r.ticks += 1;
        match r.stage {
            Hook => {
                if r.released {
                    self.free_contact(id, &mut r);
                    self.objects[k].ship.as_mut().unwrap().load_run = None;
                    return;
                }
                match r.contact {
                    LoadContact::Free => {
                        self.objects[k].ship.as_mut().unwrap().load_run = None;
                        return;
                    }
                    LoadContact::Flying => {}
                    LoadContact::Taken => {
                        let hold = if r.building {
                            let t = self.object_index(r.target).unwrap();
                            let ObjectClass::Building(class) = self.objects[t].class else {
                                unreachable!()
                            };
                            let own = self.cell_of_object(k);
                            let timed = now.wrapping_sub(r.thrown) >= r.flight;
                            if is_fort(class) {
                                if dist(own, self.anchor_cell(t)) >= 2 {
                                    r.released = true;
                                    self.free_contact(id, &mut r);
                                    false
                                } else {
                                    timed
                                        && !self.takes_hook(t)
                                        && self.occupant(r.target) == Some(id)
                                }
                            } else if is_port_like(class) {
                                if self.vt_berth(t) != own {
                                    r.released = true;
                                    self.free_contact(id, &mut r);
                                    false
                                } else {
                                    timed
                                }
                            } else {
                                self.objects[k].ship.as_mut().unwrap().load_run = None;
                                return;
                            }
                        } else {
                            let t = self.object_index(r.target).unwrap();
                            if self.objects[t]
                                .ship
                                .as_ref()
                                .and_then(|s| s.grapple)
                                .is_some_and(|g| g.with == id)
                            {
                                true
                            } else {
                                self.objects[k].ship.as_mut().unwrap().load_run = None;
                                return;
                            }
                        };
                        if hold {
                            r = self.load_hold(k, r);
                        }
                    }
                }
            }
            Pull => {
                if r.contact == LoadContact::Free {
                    r.released = true;
                }
                if r.released {
                    r = self.load_back(k, r);
                    self.free_contact(id, &mut r);
                } else {
                    let (pos, slid) = slide(r.from, r.at, r.ticks);
                    let o = &mut self.objects[k];
                    (o.x, o.y) = pos;
                    let turned = turn_tick(o.ship.as_mut().unwrap(), r.face);
                    let ready = r.building
                        || self.object_index(r.target).is_some_and(|t| {
                            self.objects[t]
                                .ship
                                .as_ref()
                                .and_then(|s| s.grapple)
                                .is_some_and(|g| g.stage != GrappleStage::Pull)
                        });
                    if slid && turned && ready {
                        r.stage = Furl;
                        r.ticks = 0;
                    }
                }
            }
            Furl => {
                // A hull step every 5th tick (delay 4).
                if r.ticks % FURL_STEP == 0 {
                    r.sail = r.sail.max(0) + 1;
                    if r.sail >= FURL_FRAMES as i32 {
                        r.sail = FURL_FRAMES as i32 - 1;
                        r.rope = false;
                        r.ticks = 0;
                        if r.contact == LoadContact::Free {
                            r.stage = PlankUp;
                        } else {
                            r.stage = Plank;
                            r.plank = 0;
                        }
                    }
                }
            }
            Plank => {
                let last = self.plank_frames() - 1;
                if r.released {
                    if r.plank >= 1 {
                        r.plank -= 1;
                    } else {
                        r.plank = -1;
                        r = self.load_back(k, r);
                    }
                } else {
                    r.plank += 1;
                    if r.plank > last {
                        r.plank = last;
                        r.ticks = 0;
                        if r.contact == LoadContact::Free {
                            r.stage = PlankUp;
                            r.released = true;
                        } else {
                            // 0x2875 (0x4b3930(1)): taken up for
                            // reloading; at a building it unloads
                            // (0x4ad9f0).
                            self.link(id, r.target);
                            if r.building {
                                self.unload_at(k, r.target);
                            }
                            r.stage = Linked;
                        }
                    }
                }
            }
            Linked => {
                let owner = self.objects[k].owner;
                let human = owner.is_some_and(|(n, s)| self.is_human_id(n * 4 + s));
                // A human's 0x4b71f0(1) stays while the contact holds
                // and it is not released; a computer's 0x4bb3a0 only
                // with `+0x6e9` set, which is never set (zeroed by the
                // constructor 0x4dc3f0 and 0x4b8d10, saved and loaded
                // with its block; no other writer), so it leaves at once
                // (its 0x4e4120 / 0x4bda00 transfer never runs). No
                // computer orders a load anyway: the only sender of
                // command 0x1f sub 2 is the navy's 0xaa task (0x4d74a0,
                // made by 0x49dfb0 from the interface's 0xc458).
                let stays = r.contact == LoadContact::Taken && !r.released && human;
                if !stays {
                    // 0x2876 (0x4b3930(0)) to a building still in
                    // contact; the link ends.
                    self.loads.retain(|l| l.ship != id);
                    r.released = true;
                    r.stage = PlankUp;
                    r.ticks = 0;
                }
            }
            PlankUp => {
                if r.plank >= 0 {
                    r.plank -= 1;
                    if r.plank < 0 {
                        r.plank = -1;
                        r = self.load_back(k, r);
                    }
                } else {
                    r = self.load_back(k, r);
                }
            }
            Back => {
                if r.point < r.count.wrapping_sub(1) {
                    r.point += 2;
                    if r.count > 0 && r.point < r.count {
                        let p = line_point(r.from, r.home, r.point as i32);
                        let o = &mut self.objects[k];
                        (o.x, o.y) = p;
                    }
                }
                let done = if r.sail < 0 {
                    true
                } else {
                    if r.sail > 0 {
                        r.sail -= 1;
                    }
                    if r.sail > 0 {
                        r.sail -= 1;
                        false
                    } else {
                        true
                    }
                };
                if r.point == r.count.wrapping_sub(1) && done {
                    self.load_done(k, r);
                    return;
                }
            }
        }
        let s = self.objects[k].ship.as_mut().unwrap();
        s.furl = if r.sail >= 0 {
            r.sail as f32 + 0.5
        } else {
            0.0
        };
        s.load_run = Some(r);
    }

    /// Frees loader `id`'s contact (0x4e02a0): a building's with 0xcb8
    /// (0x4b3970(0): its `+0x1e0` cleared).
    fn free_contact(&mut self, id: u32, r: &mut LoadRun) {
        if r.building && r.contact == LoadContact::Taken {
            self.occupants.retain(|o| *o != (r.target, id));
        }
        r.contact = LoadContact::Free;
    }

    /// The walkway's frame count (`MOST0_`).
    fn plank_frames(&self) -> i32 {
        match self.plank_art[0].len() {
            0 => PLANK_FRAMES as i32,
            n => n as i32,
        }
    }

    /// The hold (0x4b74fb): the commands but the stop dropped (0x4b37d0
    /// with 0x2000), the slide and turn set (0x4b54a0: toward the hook's
    /// cell `+0x6d7`, the anchor or the target ship's cell), state 8.
    fn load_hold(&mut self, k: usize, mut r: LoadRun) -> LoadRun {
        let here = self.cell_of_object(k);
        let there = match self.object_index(r.target) {
            Some(t) if r.building => self.anchor_cell(t),
            Some(t) => self.cell_of_object(t),
            None => here,
        };
        let dir = direction(there.0 - here.0, there.1 - here.1).unwrap_or(0);
        let home = Self::cell_middle(here);
        let (ox, oy) = alongside_offset(dir);
        let o = &mut self.objects[k];
        let s = o.ship.as_mut().unwrap();
        let rotation = s.rotation.map_or(s.heading as i32 * 3, i32::from);
        s.path.clear();
        s.way = 0;
        s.face = None;
        s.move_state = 0;
        r.stage = LoadStage::Pull;
        r.ticks = 0;
        r.home = home;
        r.at = (home.0 + ox, home.1 + oy);
        r.from = (o.x, o.y);
        r.face = parallel(dir, rotation);
        r.facing = dir as u8;
        r
    }

    /// To state 6 (0x4c3320): the line back to the cell's middle, none
    /// when already there (the count 0, which state 6 never ends).
    fn load_back(&mut self, k: usize, mut r: LoadRun) -> LoadRun {
        let o = &self.objects[k];
        r.from = (o.x, o.y);
        r.count = if r.from == r.home {
            0
        } else {
            line_count(r.from, r.home)
        };
        r.point = 0;
        r.stage = LoadStage::Back;
        r.ticks = 0;
        r.rope = false;
        r
    }

    /// State 6 done (0x4b73a0): the hull at rest, a building's contact
    /// ended with 0xcb8 (`+0x1e0` 0), the commands back (0x4b3850), the
    /// order over (0x4b8d10), notice 0x13f with the target to the group
    /// (0x4a1980) and event 8 0x90.
    fn load_done(&mut self, k: usize, r: LoadRun) {
        // (A ship's contact freed, its prey lets go.)
        let id = self.objects[k].id;
        let mut r = r;
        self.free_contact(id, &mut r);
        self.loads.retain(|l| l.ship != id);
        let s = self.objects[k].ship.as_mut().unwrap();
        s.furl = 0.0;
        s.rotation = None;
        s.load_run = None;
        s.task = None;
        s.closing = None;
        s.path.clear();
        self.post_notice(id, 0x13f, r.target);
    }

    /// Ship `k` unloads at building `target` (0x4ad9f0, at state 11's
    /// end): fish (`+0x47f`) to its player through 0x4a8c30, one for one,
    /// but not at a fort (0x34-0x36), crew experience 10; a leviathan
    /// (`+0x480`) through 0x4a8fa0 class 0x192 (x 125 x 40 / 100) and
    /// 2500 fish, 400; amber (`+0x481`) through 0x4a8fa0 class 400 (x 3 x
    /// 40 / 100), 100. Then its treasures (`unload_treasures`). An
    /// order's priority 1-234 is drawn again, 91 + r % 10 of the ship's
    /// generator.
    pub(super) fn unload_at(&mut self, k: usize, target: u32) {
        let Some(t) = self.object_index(target) else {
            return;
        };
        let ObjectClass::Building(class) = self.objects[t].class else {
            return;
        };
        let Some(owner) = self.objects[k].owner else {
            return;
        };
        let s = self.objects[k].ship.as_ref().unwrap();
        let amount = s.cargo.floor() as i64;
        let gain = match s.cargo_kind {
            Some(Resource::Fish) if !is_fort(class) => {
                self.earn(owner, 0, amount);
                10
            }
            Some(Resource::Leviathan) => {
                self.earn(owner, amount * 125 * 40 / 100, 2500);
                400
            }
            Some(Resource::Amber) => {
                self.earn(owner, amount * 3 * 40 / 100, 0);
                100
            }
            _ => 0,
        };
        if gain > 0 {
            let s = self.objects[k].ship.as_mut().unwrap();
            s.cargo = 0.0;
            s.cargo_kind = None;
            self.gain_experience(k, gain);
        }
        self.unload_treasures(k, t);
        if (1..0xeb).contains(&self.objects[k].ship.as_ref().unwrap().priority) {
            let roll = 91 + (self.ship_draw(k) % 10) as u8;
            self.objects[k].ship.as_mut().unwrap().priority = roll;
        }
    }

    /// 0x4ad9f0's treasures: ship `k` hands those of category 4 (0x4b9740:
    /// kinds 6, 7, 27; every kind for player 16, 0x4bd0c0), or 0x10 (kinds
    /// 10-14) for a computer's (side 3 or 4), from the last, to building
    /// `t`, its contact (`+0x8e6 +5`; at a load link's end, or a port's
    /// unloading 0x4add10): goods (kind 6) credited through 0x4a8fa0 by
    /// field 7's class and taken off; others handed over by 0x4bdc10 (a
    /// fort or player 16's 0x40 0x424b30, a port, yard or dock its town
    /// 0x441480; elsewhere kept), taken off when taken (announced 0x5d94
    /// with 1).
    pub(super) fn unload_treasures(&mut self, k: usize, t: usize) {
        let Some(owner) = self.objects[k].owner else {
            return;
        };
        let (fort, port) = match self.objects[t].class {
            ObjectClass::Building(c) => (is_fort(c), is_port_like(c)),
            ObjectClass::Barbarian(0x40) => (true, false),
            _ => (false, false),
        };
        let id = self.objects[k].id;
        let target = self.objects[t].id;
        let barbarian = owner == (4, 0);
        let computer = self.side_computer(Some(owner));
        let count = self.objects[k].ship.as_ref().unwrap().treasures.len();
        for n in (0..count).rev() {
            let tr = self.objects[k].ship.as_ref().unwrap().treasures[n];
            let category = if barbarian {
                4
            } else {
                match tr.kind {
                    6 | 7 | 27 => 4,
                    10..=14 => 0x10,
                    _ => 0,
                }
            };
            if category != 4 && !(category == 0x10 && computer) {
                continue;
            }
            if tr.kind == 6 {
                self.credit_class(owner, tr.param, tr.amount as i64);
                self.objects[k].ship.as_mut().unwrap().remove_treasure(n);
                continue;
            }
            let to = if fort {
                Some(target)
            } else if port {
                self.home_settlement(t)
            } else {
                None
            };
            let Some(to) = to else { continue };
            let cargo = super::load::Cargo::Treasure {
                index: n as u8,
                kind: tr.kind,
            };
            if self.can_take(to, id, cargo).is_err() {
                continue;
            }
            let tr = self.objects[k].ship.as_mut().unwrap().remove_treasure(n);
            if fort {
                self.store_at(to, tr);
            } else {
                self.store_treasure(to, tr);
            }
        }
    }
}
