//! Rammers and boarders closing in on their prey (the ram and board
//! behaviours, `+0x696` 1 and 2, run by 0x4abae6 while `+0x25d & 0x10`
//! is set by their order, 0x4aa840): sailing to it (`+0x6a1` 3: 0x4c2ec0
//! and 0x4b5d10 with 0x4b9140) until it is within 2 cells, then closing
//! in through the box of 5 cells about the ship (`+0x6a1` 1: the
//! rammer's 0x4c2230 states 1 and 2 with 0x4b8d80 and 0x4c1f60, the
//! boarder's 0x4b5df0 state 1 with 0x4b7f10). Within reach a rammer
//! charges (`Scene::start_ram`) and a boarder throws its hooks
//! (`Scene::throw_hook`). A game tick at a time.

use openconquest_formats::map::TILE_SIZE;

use super::grapple::{bow_row, UNIT};
use super::{sail, walk_path, Caches, GameData, Region, Scene};
use crate::economy::{DOCK, PORT, SHIPYARD};
use crate::objects::{self, Closing, Task};
use openconquest_formats::ObjectClass;

/// The box a rammer or boarder closes in through, and twice the distance
/// it sails to before (0x59aee8, 0x5991c8 and 0x5991d4 by ship type: 5
/// for every type).
const BOX: i32 = 5;
/// The prey's distance the closing in strikes from (the callers' reach
/// of 0x4b7f10 and 0x4b8d80).
const REACH: i32 = 1;
/// Ticks between a behaviour's plans (`+0x57f`, 25 from 0x4aa840).
const PLAN_TICKS: u32 = 25;

/// The bytes 0x4c1f60 reads at 0x59791d + dx x 4 + dy for a cell up to 2
/// off the ship's on each axis (dx x 4 + dy from -10 to 10): the
/// direction table at 0x597918 (`grapple::direction`), before it the
/// vtable 0x597914's first entry (0x4a78a0) and one byte more, after it
/// 0xff.
const FACING: [u8; 21] = [
    0, 0xa0, 0x78, 0x4a, 0, 3, 4, 5, 0xff, 2, 0xff, 6, 0xff, 1, 0, 7, 0xff, 0xff, 0xff, 0xff, 0xff,
];

/// Where 0x4c13b0 takes a rammer to strike from, by the prey's type and
/// a row by its heading (0x599688, 0x18 bytes a row, four rows a type):
/// x and y offsets from the cell's middle and a way, for a straight
/// direction and then a slanting one. Types 4-11 are alike.
const STRIKE: [[[i32; 6]; 4]; 4] = [
    [
        [0, 49, 49, 49, 49, 70],
        [0, 54, 54, 54, 54, 76],
        [47, 0, 47, 47, 47, 66],
        [0, 40, 40, 40, 0, 40],
    ],
    [
        [0, 45, 45, 45, 45, 64],
        [0, 56, 56, 56, 56, 79],
        [53, 0, 53, 53, 53, 66],
        [0, 40, 40, 40, 0, 40],
    ],
    [
        [0, 41, 41, 41, 41, 58],
        [0, 54, 54, 54, 54, 79],
        [47, 0, 47, 47, 47, 66],
        [0, 40, 40, 52, 52, 74],
    ],
    [
        [0, 41, 41, 41, 41, 58],
        [0, 54, 54, 54, 54, 76],
        [47, 0, 47, 47, 47, 66],
        [0, 40, 40, 52, 52, 74],
    ],
];

fn strike_rows(kind: u8) -> &'static [[i32; 6]; 4] {
    &STRIKE[match kind % 12 {
        0 => 0,
        1 => 1,
        2 | 3 => 2,
        _ => 3,
    }]
}

/// What the boarder's look (0x4b7f10) found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BoardLook {
    /// The prey is not in the box (0xff).
    Out,
    /// It is, further than the reach: a move to it, if any (0).
    Closing,
    /// Within reach (1).
    Reach,
}

/// What a behaviour's plan (0x4a6270 -> 0x4a6150 / 0x4a6330) gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Planned {
    /// No plan made, or one of no route to follow (0 to 3).
    Kept,
    /// A route.
    Routed,
    /// No route (0xff).
    NoRoute,
}

impl Scene {
    /// Rammers and boarders not yet in contact, a game tick at a time.
    pub(super) fn run_closing(&mut self, data: &GameData, caches: &mut Caches) {
        for o in &mut self.objects {
            if let Some(s) = o.ship.as_mut() {
                if !matches!(
                    s.task,
                    Some(
                        Task::Ram { .. }
                            | Task::Board { .. }
                            | Task::LoadAt { .. }
                            | Task::LoadRuins { .. }
                    )
                ) {
                    s.closing = None;
                }
            }
        }
        let closing: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship.as_ref().is_some_and(|s| {
                    s.grapple.is_none()
                        && s.ram_run.is_none()
                        && s.load_run.is_none()
                        && matches!(
                            s.task,
                            Some(
                                Task::Ram { .. }
                                    | Task::Board { .. }
                                    | Task::LoadAt { .. }
                                    | Task::LoadRuins { .. }
                            )
                        )
                })
            })
            .map(|o| o.id)
            .collect();
        for id in closing {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let task = self.objects[i].ship.as_ref().unwrap().task.unwrap();
            if let Task::LoadAt { target } | Task::LoadRuins { target } = task {
                self.load_tick(i, target, data, caches);
                continue;
            }
            let (Task::Ram { target } | Task::Board { target, .. }) = task else {
                continue;
            };
            // The prey as 0x4ac050 finds it (pointer and id): gone, the
            // closing in loses it (0x4b7f10, 0x4b8d80) and the sailing
            // ends the task with a notice (below).
            let prey = self
                .object_index(target)
                .filter(|&t| self.objects[t].ship.is_some());
            // A boarder waits for its hook to land (0x4b5df0 state 7); one
            // lost (a save does not keep it) frees it.
            if let Task::Board {
                hooking: true,
                thrown,
                ..
            } = task
            {
                if self.shots.iter().any(|s| s.rope == Some(id)) {
                    self.objects[i].ship.as_mut().unwrap().path.clear();
                    continue;
                }
                self.objects[i].ship.as_mut().unwrap().task = Some(Task::Board {
                    target,
                    thrown,
                    hooking: false,
                });
            }
            // The order (0x4aa840): closing in if the prey is within half
            // the box (0x4abf60: 0x557440 from the ship's cell), else
            // sailing to it; the first plan at once.
            // (A prey gone before the port's first look counts as far.)
            let own = self.cell_of_object(i);
            let near = prey.is_some_and(|ti| {
                let p = self.cell_of_object(ti);
                walk_path::dist(own.0, own.1, p.0, p.1) <= BOX / 2
            });
            let s = self.objects[i].ship.as_mut().unwrap();
            if s.closing.is_none_or(|c| c.target != target) {
                s.closing = Some(Closing {
                    target,
                    near,
                    from: None,
                    contact: -1,
                    planned: 0,
                    plan_from: own,
                    plan_goal: own,
                    first_plan: true,
                    contact_wait: false,
                    by_id: false,
                    thrown: None,
                    hook_cell: (0, 0),
                });
            }
            let near = s.closing.unwrap().near;
            match (task, near, prey) {
                (Task::Board { .. }, false, _) => self.board_sail(i, prey),
                // 0x4b5df0 state 1: out of the box (or gone) it sails to it
                // again; within reach it throws a hook (0x4b84b0) at a
                // prey at war, and waits by one that is not.
                (Task::Board { .. }, true, None) => self.closing_mut(i).near = false,
                (Task::Board { .. }, true, Some(ti)) => match self.board_look(i, ti) {
                    BoardLook::Out => self.closing_mut(i).near = false,
                    BoardLook::Closing => {}
                    BoardLook::Reach => {
                        if Self::are_enemies(&self.objects[i], &self.objects[ti]) {
                            self.throw_hook(i, ti, data, caches);
                        }
                    }
                },
                (_, false, _) => self.ram_sail(i, prey),
                // 0x4c2230: 0x4b8d80 does not find a prey gone.
                (_, true, None) => self.closing_mut(i).near = false,
                (_, true, Some(ti)) => self.ram_close(i, ti),
            }
        }
    }

    /// Ship `i`'s Reload Object at own port, yard, dock, fort or ship
    /// `target`, standing or in ruins (command 0x1f kind 2: event 0x10
    /// `{3, 1, id, 0xff}`, the order 0x4aa840, `+0x696` 3 run by
    /// 0x4b7370), a game tick at a time. The order finds the target by id
    /// (0x4ac050 with -1; gone, it does nothing) and measures (0x4abf60:
    /// 0x557440 from the ship's cell to a port's or dock's berth, vt+0xb8,
    /// else to the anchor `+0x74 / +0x78`, a ship's cell): within half
    /// the box (2) closing in (`+0x6a1` 1), keeping the target's player in
    /// `+0x6c6`; else sailing (3) with `+0x6c6` 0xff.
    ///
    /// Closing in (0x4b73a0 state 1) looks for it with 0x4b7f10(5,
    /// pointer, id, .., 1) in the cells' object slots (`Scene::load_look`;
    /// a ship's as the boarder's, `Scene::board_look`): not in the box
    /// (0xff; ruins never are, their fall emptied the slots, 0x423d60 on
    /// 0x138b -> 0x499860), sailing again with `+0x6c7` -1; within reach,
    /// the contact (0x4b84b0, `Scene::load_contact`).
    ///
    /// Sailing (0x4b5d10) on a new cell or held up: 0x4b9140(2, -1,
    /// `+0x6c6`, id) finds the target by player (0x4ac050 -> 0x4a8710,
    /// whose list holds a standing one and has lost ruins: notice 0x13c
    /// and the task stopped) or by id (0x51f1f0, found while it lasts;
    /// removed ruins, 0x4232ed, lost likewise): 0x50ebf0 gives the berth
    /// (vt+0xb8 for building classes below 0x44, a ship's cell), and the
    /// berth and the anchor (vt+0x28, 0x40dfa0) both within 2 (0x557480)
    /// close in again, else a plan (0x4a6270 mode 5) to the berth. So a
    /// ship far at ruins sails to their berth and lies there, the two
    /// states turning, until another order; one near ends at once with
    /// notice 0x13c.
    fn load_tick(&mut self, i: usize, target: u32, data: &GameData, caches: &mut Caches) {
        let found = self.object_index(target);
        let standing = found.filter(|_| !self.fallen(target));
        let own = self.cell_of_object(i);
        let s = self.objects[i].ship.as_mut().unwrap();
        if s.closing.is_none_or(|c| c.target != target) {
            // 0x4aa840: the target by id; gone, the order does nothing.
            let Some(t) = found else {
                s.task = None;
                return;
            };
            let at = match self.objects[t].class {
                ObjectClass::Building(PORT | DOCK) => self.vt_berth(t),
                _ => self.anchor_cell(t),
            };
            let near = walk_path::dist(own.0, own.1, at.0, at.1) <= BOX / 2;
            self.objects[i].ship.as_mut().unwrap().closing = Some(Closing {
                target,
                near,
                from: None,
                contact: -1,
                planned: 0,
                plan_from: own,
                plan_goal: own,
                first_plan: true,
                contact_wait: false,
                by_id: !near,
                thrown: None,
                hook_cell: (0, 0),
            });
        }
        let s = self.objects[i].ship.as_ref().unwrap();
        let c = s.closing.unwrap();
        if c.near {
            // 0x4b73a0 state 1.
            let look = match standing {
                Some(t) if self.objects[t].ship.is_some() => self.board_look(i, t),
                Some(t) => self.load_look(i, t),
                None => BoardLook::Out,
            };
            match look {
                BoardLook::Out => {
                    let c = self.closing_mut(i);
                    c.near = false;
                    c.from = None;
                }
                BoardLook::Closing => {}
                BoardLook::Reach => self.load_contact(i, target, data, caches),
            }
            return;
        }
        if c.from == Some(own) && s.waited == 0.0 {
            return;
        }
        let Some(t) = (if c.by_id { found } else { standing }) else {
            self.prey_lost(i, 0x13c);
            return;
        };
        let (berth, anchor) = if self.objects[t].ship.is_some() {
            let cell = self.cell_of_object(t);
            (cell, cell)
        } else {
            (self.vt_berth(t), self.anchor_cell(t))
        };
        if walk_path::dist2(berth.0, berth.1, own.0, own.1) <= BOX / 2
            && walk_path::dist2(anchor.0, anchor.1, own.0, own.1) <= BOX / 2
        {
            self.closing_mut(i).near = true;
            return;
        }
        self.closing_plan(i, berth, None);
        self.closing_mut(i).from = Some(own);
    }

    /// An object's anchor cell (`+0x74 / +0x78`; a ship's own cell).
    pub(super) fn anchor_cell(&self, t: usize) -> (i32, i32) {
        match self.objects[t].cell {
            Some(c) if self.objects[t].ship.is_none() => c,
            _ => self.cell_of_object(t),
        }
    }

    /// The loader's look for a building (0x4b7f10 with a box of 5 and a
    /// reach of 1, from 0x4b73a0 state 1): the box and its grid as the
    /// boarder's (`Scene::board_look`); the building is found where its
    /// cell lies in the box (its object slot). The point is its berth
    /// (vt+0xb8), but the anchor (`+0x74 / +0x78`) for the classes of the
    /// mask 0xe01c from 0x32 (the forts 0x34-0x36, and 0x3f-0x41), and
    /// the reach 0 for a port, yard or dock (0x39-0x3b): its distance the
    /// larger axis from the ship's cell, if within half the box on both.
    /// Not found, out; within reach, `+0x6aa` the cell one past the box's
    /// end corner (as the boarder's); further, a move (0x4a6270 mode 6)
    /// through the box to the point; out of half the box, none. (Not
    /// ported: the plan's building flag, 0x4a6330's second argument,
    /// which lets a goal on land through to the region finder 0x4a5a10,
    /// and `+0x594` set for the anchor's classes; the port's plan takes
    /// no goal on land.)
    fn load_look(&mut self, i: usize, t: usize) -> BoardLook {
        let s2 = BOX / 2;
        let own = self.cell_of_object(i);
        let (cx, cy) = self.reserved_cell(i).unwrap_or(own);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let (x0, x1) = ((cx - s2).max(0), (cx + s2 + 1).min(w));
        let (y0, y1) = ((cy - s2).max(0), (cy + s2 + 1).min(h));
        let deep = objects::deep_draught(self.objects[i].ship.as_ref().unwrap().kind);
        let mine = self.reserved_cell(i);
        let cell = self.anchor_cell(t);
        let mut found = false;
        let mut grid = Vec::with_capacity(((x1 - x0).max(0) * (y1 - y0).max(0)) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let c = (x, y);
                let mut v = if self.sea_cell(c, deep) { 0 } else { -1 };
                if let Some(k) = self.slot_holder(c) {
                    if k != i && Some(c) != mine {
                        v = -1;
                    }
                }
                found |= c == cell;
                grid.push(v);
            }
        }
        let ObjectClass::Building(class) = self.objects[t].class else {
            return BoardLook::Out;
        };
        if !found {
            return BoardLook::Out;
        }
        let (point, reach) = match class {
            PORT | SHIPYARD | DOCK => (self.vt_berth(t), 0),
            c if (0x32..0x52).contains(&c) && (1u32 << (c - 0x32)) & 0xe01c != 0 => (cell, REACH),
            _ => (self.vt_berth(t), REACH),
        };
        let (dx, dy) = ((own.0 - point.0).abs(), (own.1 - point.1).abs());
        if dx > s2 || dy > s2 {
            return BoardLook::Closing;
        }
        if dx.max(dy) <= reach {
            self.closing_mut(i).contact = w * y1 + x1;
            return BoardLook::Reach;
        }
        let region = Region {
            x0,
            y0,
            goal: (point.0 - x0, point.1 - y0),
            w: x1 - x0,
            h: y1 - y0,
            grid,
        };
        self.closing_plan(i, point, Some(&region));
        BoardLook::Closing
    }

    /// The loader within reach (0x4b73a0 state 1 -> 0x4b84b0, called with
    /// no enemy test): it throws its hook (`Scene::load_throw`), the run
    /// going on in state 7 (`Scene::run_load_runs`); else it stays in
    /// state 1 to try again the next tick.
    fn load_contact(&mut self, i: usize, target: u32, data: &GameData, caches: &mut Caches) {
        self.load_throw(i, target, data, caches);
    }

    /// A port's, yard's, dock's or fort's berth (vt+0xb8, 0x40e050): its
    /// anchor a step toward its facing `+0x1dc` (0x5c4f4c), (-1, -1) for
    /// none. A fort's facing is set as it is made (0x419f80 message 2):
    /// 6 when the cell below its anchor is water (landscape low nibble 7
    /// or 1), else 0 for the cell right, 4 left, 2 above; with none it is
    /// kept: the record's field 10 only with the class flag 8 (`+0x1d8`,
    /// 0x4280e4), which the fort's constructor 0x417810 (vtable 0x59592c)
    /// leaves out (0x20), so the base constructor's -1 (0x427780) and the
    /// berth (-1, -1). A port's, yard's or dock's (0x40ef70: 0xa, flag 8)
    /// facing is the record's field 10.
    pub(super) fn vt_berth(&self, t: usize) -> (i32, i32) {
        let o = &self.objects[t];
        if let (ObjectClass::Building(c), Some((x, y))) = (o.class, o.cell) {
            if (0x34..=0x36).contains(&c) {
                let (w, h) = (self.grid.width as i32, self.grid.height as i32);
                let water = |cx: i32, cy: i32| {
                    matches!(self.cell_types[(cy * w + cx) as usize] & 0xf, 7 | 1)
                };
                let facing = if y < h - 1 && water(x, y + 1) {
                    Some(6)
                } else if x < w - 1 && water(x + 1, y) {
                    Some(0)
                } else if x > 0 && water(x - 1, y) {
                    Some(4)
                } else if y > 0 && water(x, y - 1) {
                    Some(2)
                } else {
                    None
                };
                return match facing {
                    Some(f) => {
                        let (dx, dy) = UNIT[f];
                        (x + dx, y + dy)
                    }
                    None => (-1, -1),
                };
            }
        }
        self.berth_cell(t).unwrap_or((-1, -1))
    }

    fn closing_mut(&mut self, i: usize) -> &mut Closing {
        self.objects[i]
            .ship
            .as_mut()
            .unwrap()
            .closing
            .as_mut()
            .unwrap()
    }

    /// Ship `k`'s reserved next cell (`+0x5ab`) as the port keeps it: the
    /// cell it is going into while between cells under way (move state 4
    /// or 5).
    pub(super) fn reserved_cell(&self, k: usize) -> Option<(i32, i32)> {
        let s = self.objects[k].ship.as_ref()?;
        if matches!(s.move_state, 4 | 5) {
            s.under_way_into()
        } else {
            None
        }
    }

    /// The ship holding a cell's slot (`grid + 0xc`): one in it, or one
    /// that has reserved it.
    pub(super) fn slot_holder(&self, cell: (i32, i32)) -> Option<usize> {
        self.slot_ship(cell)
            .or_else(|| (0..self.objects.len()).find(|&k| self.reserved_cell(k) == Some(cell)))
    }

    /// A behaviour's move of ship `i` to `goal`, inside `region` if given
    /// (0x4a6270 modes 6 and 5). The first after the order at once
    /// (`+0x593`); later ones (0x4a6150) once 25 ticks have passed since
    /// the last plan (`+0x57b`, `+0x57f`), if the ship has changed cells
    /// or the goal changed or it lies still, and it is not turning (move
    /// state 2). (0x4a6150's goal-is-its-cell end, event 8 0x80, is never
    /// met: the goals are the prey's cells. Its wait counter `+0x591` is
    /// the port's own mover's.) Then 0x4a6330: no plan to its own cell
    /// (1) or to one 0x499990(cell, 0) refuses (0); none for a route of
    /// one cell (0 or 2); no route, 0xff. The plan's tick, cell and goal
    /// are kept (0x4a6330, 0x4a6100).
    fn closing_plan(&mut self, i: usize, goal: (i32, i32), region: Option<&Region>) -> Planned {
        let now = self.ticks;
        let own = self.cell_of_object(i);
        let s = self.objects[i].ship.as_ref().unwrap();
        let c = s.closing.unwrap();
        if !c.first_plan {
            let changed = own != c.plan_from || goal != c.plan_goal || s.move_state == 0;
            if !changed || s.move_state == 2 || now.wrapping_sub(c.planned) < PLAN_TICKS {
                return Planned::Kept;
            }
        }
        if goal == own || !self.nav.is_sea(goal.0, goal.1) {
            return Planned::Kept;
        }
        let start = if s.plans_from_own_cell() {
            own
        } else {
            s.entering
        };
        let route = match region {
            Some(r) => self.plan_region(i, goal, r),
            None => {
                let (x, y) = Self::cell_middle(goal);
                self.plan_route(i, x, y)
            }
        };
        let c = self.closing_mut(i);
        c.first_plan = false;
        c.planned = now;
        c.plan_from = own;
        c.plan_goal = goal;
        match route {
            Some(path) => {
                self.objects[i].ship.as_mut().unwrap().path = path;
                Planned::Routed
            }
            None if start == goal => Planned::Kept,
            None => Planned::NoRoute,
        }
    }

    /// The boarder sailing to its prey (`+0x6a1` 3, 0x4b5d10): when it is
    /// in another cell than at its last plan (`+0x6c7`) or held up
    /// (`+0x591`), 0x4b9140 with half the box: the prey within 2 of its
    /// cell (0x557480; vt+0x28 and 0x50ebf0 both give a ship's cell) and
    /// it closes in (`+0x6a1` 1); else it plans to the prey's cell
    /// (0x4a6270 mode 5, tolerance 0) and keeps the cell. No war test.
    /// The prey gone: notice 0x13c and the task stopped.
    fn board_sail(&mut self, i: usize, ti: Option<usize>) {
        let own = self.cell_of_object(i);
        let s = self.objects[i].ship.as_ref().unwrap();
        if s.closing.unwrap().from == Some(own) && s.waited == 0.0 {
            return;
        }
        let Some(ti) = ti else {
            self.prey_lost(i, 0x13c);
            return;
        };
        let prey = self.cell_of_object(ti);
        if walk_path::dist2(prey.0, prey.1, own.0, own.1) <= BOX / 2 {
            self.closing_mut(i).near = true;
            return;
        }
        self.closing_plan(i, prey, None);
        self.closing_mut(i).from = Some(own);
    }

    /// The rammer sailing to its prey (`+0x6a1` 3, 0x4c2ec0): 0x4b9140 as
    /// the boarder's, every tick; within 2 it closes in, its next plan at
    /// once (0x4c1370: `+0x593`), if the prey is at war (else, and with
    /// the prey gone, notice 0x13d and the task stopped, 0x4abc60).
    fn ram_sail(&mut self, i: usize, ti: Option<usize>) {
        let own = self.cell_of_object(i);
        let Some(ti) = ti else {
            self.prey_lost(i, 0x13d);
            return;
        };
        let prey = self.cell_of_object(ti);
        if walk_path::dist2(prey.0, prey.1, own.0, own.1) <= BOX / 2 {
            if !Self::are_enemies(&self.objects[i], &self.objects[ti]) {
                self.prey_lost(i, 0x13d);
                return;
            }
            let c = self.closing_mut(i);
            c.near = true;
            c.first_plan = true;
            return;
        }
        self.closing_plan(i, prey, None);
        self.closing_mut(i).from = Some(own);
    }

    /// A boarding or ram given up while sailing to the prey: notice
    /// `code` (0x13c the boarder's, 0x4a1900 from 0x4b5d10; 0x13d the
    /// rammer's, 0x4a1880 from 0x4c2ec0) with the prey to the ship's
    /// group, and the task stopped (the boarder's event 8, the rammer's
    /// 0x4abc60).
    fn prey_lost(&mut self, i: usize, code: u32) {
        let id = self.objects[i].id;
        let s = self.objects[i].ship.as_mut().unwrap();
        let prey = s.closing.map_or(0, |c| c.target);
        s.task = None;
        s.closing = None;
        // 0x4abc60's tail (the boarder's event 8 runs it through 0x4aa540).
        s.fire_wind_down();
        self.post_notice(id, code, prey);
    }

    /// The boarder's look (0x4b7f10 with a box of 5 and a reach of 1, from
    /// 0x4b5df0 state 1): the box of half 5 about its cell (the cell it is
    /// going into while under way, `+0x5b6`), clipped to the map, row by
    /// row: open where 0x499990(cell, draught) takes the cell, closed
    /// where another ship holds the slot but on its own reserved cell
    /// (`+0x5ab`) and for the prey. The prey's slot on its own cell gives
    /// that cell, and its distance from the ship's own cell (the larger
    /// axis) if within half the box on both; the slot of the cell it is
    /// under way to gives that cell, apart, with its distance from the
    /// box's middle by the box's rows and columns (as the original counts
    /// them, from the clipped corner), and the prey's own cell with its
    /// distance unbounded. Not found, out; within reach, `+0x6aa` is the
    /// cell one past the box's end corner (the disassembly at 0x4b83bf:
    /// W x the end row + the end column); else a move (0x4a6270 mode 6)
    /// through the box to the nearer of the two cells, the prey's own on a
    /// tie, or to the one with a distance; with neither, none. (Its search
    /// mode, `+0x6a1` 2, is never set: no sender of event 0x10 gives
    /// mode 2. Its building prey is not ported: every sender the port has
    /// names a ship.)
    pub(super) fn board_look(&mut self, i: usize, ti: usize) -> BoardLook {
        let s2 = BOX / 2;
        let own = self.cell_of_object(i);
        let (cx, cy) = self.reserved_cell(i).unwrap_or(own);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let (x0, x1) = ((cx - s2).max(0), (cx + s2 + 1).min(w));
        let (y0, y1) = ((cy - s2).max(0), (cy + s2 + 1).min(h));
        let deep = objects::deep_draught(self.objects[i].ship.as_ref().unwrap().kind);
        let mine = self.reserved_cell(i);
        let (prey, prey_next) = (self.cell_of_object(ti), self.reserved_cell(ti));
        let (mut found, mut d, mut d_next) = (false, -1, -1);
        let (mut at, mut next) = (prey, prey);
        let mut grid = Vec::with_capacity(((x1 - x0).max(0) * (y1 - y0).max(0)) as usize);
        for (row, y) in (y0..y1).enumerate() {
            for (col, x) in (x0..x1).enumerate() {
                let c = (x, y);
                let mut v = if self.sea_cell(c, deep) { 0 } else { -1 };
                if let Some(k) = self.slot_holder(c) {
                    if k != i && Some(c) != mine && k != ti {
                        v = -1;
                    }
                    if k == ti {
                        found = true;
                        let (dx, dy) = ((own.0 - prey.0).abs(), (own.1 - prey.1).abs());
                        if Some(c) != prey_next {
                            at = c;
                            if dx <= s2 && dy <= s2 {
                                d = dx.max(dy);
                            }
                        } else {
                            next = c;
                            d_next = (row as i32 - s2).abs().max((col as i32 - s2).abs());
                            d = dx.max(dy);
                            at = prey;
                        }
                    }
                }
                grid.push(v);
            }
        }
        if !found {
            return BoardLook::Out;
        }
        let goal = if d >= 0 {
            if d <= REACH {
                self.closing_mut(i).contact = w * y1 + x1;
                return BoardLook::Reach;
            }
            if d_next >= 0 && d_next < d {
                next
            } else {
                at
            }
        } else if d_next >= 0 {
            next
        } else {
            return BoardLook::Closing;
        };
        let region = Region {
            x0,
            y0,
            goal: (goal.0 - x0, goal.1 - y0),
            w: x1 - x0,
            h: y1 - y0,
            grid,
        };
        self.closing_plan(i, goal, Some(&region));
        BoardLook::Closing
    }

    /// The rammer closing in (`+0x6a1` 1, 0x4c2230 states 1 and 2): its
    /// look through the box of 5 about its cell (0x4b8d80, reach 1) and,
    /// the prey found, its goal (0x4c1f60). Out of reach, a move to the
    /// goal through the box (0x4a6270 mode 6); no route, or the prey not
    /// found, and it sails to it again (`+0x6a1` 3). Within reach and
    /// alongside (2) it charges (`Scene::start_ram`: 0x4c1760, state 2);
    /// else `+0x6aa` is the goal and the move made, kept only if its route
    /// ends there (`+0x5b2`) or it is under way (move state 4 or 5).
    /// (0x4c1f60 gives 0 only for prey that is no ship, which the port's
    /// rammers do not have.)
    pub(super) fn ram_close(&mut self, i: usize, ti: usize) {
        self.closing_mut(i).contact = -1;
        let own = self.cell_of_object(i);
        let b = (own.0 - BOX / 2, own.1 - BOX / 2, BOX, BOX);
        let Some((reach, region)) = self.ram_look(i, ti, b) else {
            let c = self.closing_mut(i);
            c.near = false;
            c.contact_wait = false;
            return;
        };
        let (alongside, goal) = self.ram_goal(i, ti);
        if !reach {
            if self.closing_plan(i, goal, Some(&region)) == Planned::NoRoute {
                let c = self.closing_mut(i);
                c.near = false;
                c.contact_wait = false;
            }
            return;
        }
        if alongside == 2 {
            self.ram_contact(i, ti);
            return;
        }
        self.closing_mut(i).contact_wait = false;
        let w = self.nav.width as i32;
        self.closing_mut(i).contact = w * goal.1 + goal.0;
        let planned = self.closing_plan(i, goal, Some(&region));
        let s = self.objects[i].ship.as_ref().unwrap();
        let t = TILE_SIZE as f32;
        let end = s.path.back().map_or(own, |&(x, y)| {
            ((x / t).floor() as i32, (y / t).floor() as i32)
        });
        if planned != Planned::NoRoute && (goal == end || matches!(s.move_state, 4 | 5)) {
            return;
        }
        self.closing_mut(i).near = false;
    }

    /// The rammer alongside asks for the contact (0x4c2230 states 1 and
    /// 2). In state 1 `+0x6ae` is the cell whose slot holds it: its own,
    /// else the one about it it has reserved (0x49a450 over the grid's
    /// offset list); none, it stays in state 1 this tick. Then state 2 and
    /// 0x4c1760: granted, the charge (`start_ram`). Refused (`ram_granted`):
    /// in state 2 already, state 1 and the move stopped (event 8 0x80);
    /// else not between cells (`+0x596` not 1) it stays in state 2 to ask
    /// at its next look; between cells with its picture at its cell's
    /// middle, state 1 and the move stopped; else its commands held
    /// (0x4b37d0), its back-off heading the opposite of its own (0x4a34f0)
    /// and state 6 (`RamStage::Back` to its cell's middle), lying still
    /// (move state 0). (Not ported: the reservation itself, the port keeps
    /// no slots between ticks; state 2's release with `+0x6a2` below 0,
    /// whose setter is not traced.)
    fn ram_contact(&mut self, i: usize, ti: usize) {
        let waiting = self.closing_mut(i).contact_wait;
        if !waiting {
            let own = self.cell_of_object(i);
            let reserved = self
                .reserved_cell(i)
                .is_some_and(|c| (c.0 - own.0).abs() <= 1 && (c.1 - own.1).abs() <= 1);
            if self.slot_holder(own) != Some(i) && !reserved {
                return;
            }
        }
        if self.ram_granted(i, ti) {
            self.closing_mut(i).contact_wait = false;
            self.start_ram(i, ti);
            return;
        }
        let home = Self::cell_middle(self.cell_of_object(i));
        let between = self.objects[i].ship.as_ref().unwrap().between.get();
        if !waiting && !between {
            self.closing_mut(i).contact_wait = true;
            return;
        }
        self.closing_mut(i).contact_wait = false;
        if waiting || (self.objects[i].x, self.objects[i].y) == home {
            self.objects[i].ship.as_mut().unwrap().path.clear();
            return;
        }
        // No contact made: the run names no prey (`+0x8e6` stays 0), the
        // prey free of it.
        let s = self.objects[i].ship.as_mut().unwrap();
        s.path.clear();
        s.move_state = 0;
        s.ram_run = Some(objects::RamRun {
            target: 0,
            stage: objects::RamStage::Back,
            home,
            ticks: 0,
            clock: 0.0,
            frames: 0,
        });
    }

    /// The rammer's look (0x4b8d80) through box `(x, y, w, h)`: its corner
    /// clipped to the map (none if then off it), its far side `w` and `h`
    /// on from the clipped corner, clipped too; row by row open where
    /// 0x499990(cell, draught) takes the cell, closed where a ship lies
    /// still (move state 0) on its own cell, but itself, the prey and on
    /// its own reserved cell, and a cell across a corner of its own while
    /// its heading (`+0x5af`, signed) is below 1 and 0x499ca0 refuses the
    /// corner. The prey found (only at war): the region (goal its cell),
    /// and whether it is within reach (0x557480 from the ship's cell) and
    /// not across a refused corner.
    pub(super) fn ram_look(
        &self,
        i: usize,
        ti: usize,
        (bx, by, bw, bh): (i32, i32, i32, i32),
    ) -> Option<(bool, Region)> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let (x0, y0) = (bx.max(0), by.max(0));
        if x0 >= w || y0 >= h {
            return None;
        }
        let (x1, y1) = ((x0 + bw).min(w), (y0 + bh).min(h));
        let s = self.objects[i].ship.as_ref().unwrap();
        let (deep, heading) = (objects::deep_draught(s.kind), s.heading);
        let own = self.cell_of_object(i);
        let mine = self.reserved_cell(i);
        let mut found = None;
        let mut refused = false;
        let mut grid = Vec::with_capacity(((x1 - x0) * (y1 - y0)) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let c = (x, y);
                let mut v = if self.sea_cell(c, deep) { 0 } else { -1 };
                if let Some(k) = self.slot_ship(c) {
                    let still = self.objects[k].ship.as_ref().unwrap().move_state == 0;
                    if k != i && Some(c) != mine && k != ti && still {
                        v = -1;
                    }
                    let corner = (own.0 - x).abs() == 1
                        && (own.1 - y).abs() == 1
                        && heading < 1
                        && !self.corner_free_cells(own, c);
                    if corner {
                        v = -1;
                    }
                    if k == ti && Self::are_enemies(&self.objects[i], &self.objects[ti]) {
                        found = Some(c);
                        refused = corner;
                    }
                }
                grid.push(v);
            }
        }
        let p = found?;
        let reach = walk_path::dist2(own.0, own.1, p.0, p.1) <= REACH && !refused;
        let region = Region {
            x0,
            y0,
            goal: (p.0 - x0, p.1 - y0),
            w: x1 - x0,
            h: y1 - y0,
            grid,
        };
        Some((reach, region))
    }

    /// The rammer's goal (0x4c1f60): from its cell (the one it is going
    /// into while between cells, 0x4e0b50 false) the prey's cell and,
    /// under way (move state 4 or 5), the cell it has reserved; the goal
    /// the nearer (0x557440), 0x4c13b0 choosing when both are 1 away, the
    /// prey's own on a tie. 2, alongside: 0 or 1 away and between cells,
    /// or 0 away, or 1 away and reaching its route point this tick
    /// (0x4a68b0), or facing it (0x59791d from its own cell = `+0x5af`),
    /// or in move state 3; also lying still 1 away facing it. Else 1.
    pub(super) fn ram_goal(&self, i: usize, ti: usize) -> (u8, (i32, i32)) {
        let s = self.objects[i].ship.as_ref().unwrap();
        let own = self.cell_of_object(i);
        let at_cell = s.plans_from_own_cell();
        let from = if at_cell { own } else { s.entering };
        let prey = self.cell_of_object(ti);
        let d0 = walk_path::dist(from.0, from.1, prey.0, prey.1);
        let next = self.reserved_cell(ti);
        let d1 = next.map_or(-1, |n| walk_path::dist(from.0, from.1, n.0, n.1));
        let facing = || {
            let (dx, dy) = (prey.0 - own.0, prey.1 - own.1);
            // The prey is within 1 of `from`, so within 2 of its own cell.
            (dx.abs() <= 2 && dy.abs() <= 2)
                && FACING[(10 + dx * 4 + dy) as usize] as usize == s.heading
        };
        let mut c = 0;
        if (d0 == 0 || d0 == 1)
            && (!at_cell || self.reaches_point(i) || d0 == 0 || facing() || s.move_state == 3)
        {
            c = 2;
        }
        let goal = match next {
            None => prey,
            Some(n) if d0 == 1 && d1 == 1 => {
                if self.strike_from(i, ti, [prey, n], from) {
                    n
                } else {
                    prey
                }
            }
            Some(n) if d1 < d0 => n,
            _ => prey,
        };
        if c == 0 {
            c = 1;
        }
        if s.move_state == 0 && d0 == 1 && facing() {
            c = 2;
        }
        (c, goal)
    }

    /// Whether ship `i` comes to its route point on this tick (0x4a68b0):
    /// not under way (move state 4 or 5), yes; else its way stepped a
    /// tick as the move tick steps it (`sail::next_way`, always by the
    /// wind and not halved by lime) and its picture moved so along its
    /// heading, reaching or passing the point on an axis it moves along.
    fn reaches_point(&self, i: usize) -> bool {
        let o = &self.objects[i];
        let s = o.ship.as_ref().unwrap();
        if !matches!(s.move_state, 4 | 5) {
            return true;
        }
        let Some(&(tx, ty)) = s.path.front() else {
            return false;
        };
        let sailing = sail::Sailing {
            top: self.ship_top(i),
            accel: sail::accel(s.kind),
            brake: s.brake,
            wind: Some(self.wind.speed_percent(s.heading)),
            braking: s.move_state == 4,
            limed: false,
        };
        let (way, _) = sail::next_way(s.way, &sailing);
        let at = (o.x.round() as i32, o.y.round() as i32);
        let point = (tx.round() as i32, ty.round() as i32);
        sail::reaches(at, point, s.heading, way, s.way_rest)
    }

    /// Which of `cells` (the prey's and the one it is going into, both 1
    /// from `from`) rammer `i` strikes prey `ti` sooner from (0x4c13b0,
    /// read in the disassembly): for each, the point off `from`'s middle
    /// in its direction by 0x599688 (by the prey's type and a row by its
    /// heading: east or west, 1 if it is not north of the rammer's row;
    /// north or south 2; slanting 3), the time to it (`t0`, the rammer's
    /// distance to `from`'s middle under way, plus the row's way less half
    /// its bow's x at rotation 0, 0x599204, each x 6000 straight on and
    /// 9000 slanting over its way `+0x59b`, at least 1; 0 for a way not
    /// above 0), and the prey's centre carried on its heading that long
    /// at its way (x `+0x59b` / 3000, halved straight on, a third
    /// slanting; 32-bit as the original); true (the second) when the
    /// first's point is no nearer it (0x557440).
    fn strike_from(&self, i: usize, ti: usize, cells: [(i32, i32); 2], from: (i32, i32)) -> bool {
        let (r, p) = (&self.objects[i], &self.objects[ti]);
        let (rs, ps) = (r.ship.as_ref().unwrap(), p.ship.as_ref().unwrap());
        let middle = (from.0 * 64 + 32, from.1 * 64 + 32);
        let dp = if matches!(rs.move_state, 0 | 2 | 3) {
            0
        } else {
            walk_path::dist(middle.0, middle.1, r.x as i32, r.y as i32)
        };
        let k = match ps.heading % 8 {
            0 | 4 => (self.cell_of_object(ti).1 >= self.cell_of_object(i).1) as usize,
            2 | 6 => 2,
            _ => 3,
        };
        let v = rs.way.max(0);
        let v = if v == 0 { 1 } else { v };
        let per = |slant: bool| if slant { 9000 } else { 6000 };
        let t0 = dp * per(rs.heading % 2 == 1) / v;
        let row = strike_rows(ps.kind)[k];
        let bow = bow_row(rs.kind)[0].0 as i32;
        let pv = ps.way;
        let (pux, puy) = UNIT[ps.heading % 8];
        let dist = cells.map(|c| {
            let (dx, dy) = (c.0 - from.0, c.1 - from.1);
            let dir = FACING[(10 + dx * 4 + dy) as usize] as usize % 8;
            let slant = dir % 2 == 1;
            let (ux, uy) = UNIT[dir];
            let [ox, oy, len] = if slant {
                [row[3], row[4], row[5]]
            } else {
                [row[0], row[1], row[2]]
            };
            let to = (ux * ox + middle.0, uy * oy + middle.1);
            let len = len - bow / 2;
            let t = if len > 0 {
                len * per(slant) / v + t0
            } else {
                0
            };
            let s = t.wrapping_mul(pv) / 3000 / if ps.heading % 2 == 1 { 3 } else { 2 };
            let at = (pux * s + p.x as i32, puy * s + p.y as i32);
            walk_path::dist(at.0, at.1, to.0, to.1)
        });
        dist[0] >= dist[1]
    }
}
