//! The human's attack zone: the interface's fleet command (0x49d790 case
//! 10) makes the selection a group and gives it the block `{0x70, 0x20}`
//! (0x4d5f50). The block lists the enemies in the area, picks one to go
//! for, brings the group's ships near it in formation and deals the
//! targets out among them; each 70 ticks (0x4d61d0) it hears its ships'
//! notices, keeps them at their cells and plans the approach again. See
//! docs/FORMATS.md ("The human's attack zone").

use super::gather::{Cells, Notice};
use super::{walk_path, Order, Scene};
use crate::formation;
use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;
use serde::{Deserialize, Serialize};

/// An entry of the block's targets `+0x2b` (0x20 bytes: `{object, id,
/// class +0x50, strength, vt+0x10 & 0xffff, x, y, +0x64}`; the type word
/// vt+0x10 is never read here, so not kept).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub id: u32,
    pub class: u32,
    pub strength: i32,
    pub cell: (i32, i32),
    /// The object's `+0x64` (`worth`).
    pub value: i32,
}

/// An entry of the block's slots `+0x33` (0x18 bytes: `{ship, id, target,
/// target id, x, y}`), kept by the ship's id (0x4d4b10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub ship: u32,
    /// `+8`: the target dealt to it (the object; here its id).
    pub target: u32,
    /// `+0xc`: the target's id, written only with an attack sent.
    pub target_id: u32,
    pub cell: (i32, i32),
}

/// A human group running the attack block: the group's own fields the
/// block reads and the block (0x65 bytes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanAttack {
    /// The group's serial number (shared with the gather zones' human
    /// groups, `ZoneGroup::Human`).
    pub serial: u32,
    pub owner: Option<(u32, u32)>,
    /// Group `+0x31`: its ships, by id.
    pub ships: Vec<u32>,
    /// Group `+0x2d`: the notices not yet read, oldest first.
    pub notices: Vec<Notice>,
    /// Group `+8`: ticks since the last pass.
    pub elapsed: u32,
    /// Group `+0x19`, `+0x1d`, `+0x21`: the behaviour 0x4c4350 gave it.
    pub players: u32,
    pub classes: u32,
    pub third: u32,
    /// Block `+8`: the tick it was made or rewritten.
    pub made: u32,
    /// Block `+0xc..+0x18`: the area, `{x, y, w, h}` in cells.
    pub rect: Cells,
    /// Block `+0x20`: 1 approaching, 2 attacking (3, the formation of
    /// 0x4d4260(1), never comes about).
    pub mode: u8,
    /// Block `+0x21`: 0xb after a formation about the goal, 10 after a
    /// step of the approach.
    pub phase: u8,
    /// Block `+0x23 / +0x25`: the goal cell (shorts; -1 none).
    pub goal: (i32, i32),
    /// Block `+0x27 / +0x29`: the cell the formation was planned about.
    pub formation: (i32, i32),
    pub targets: Vec<Target>,
    pub slots: Vec<Slot>,
    /// Block `+0x45`: the tick of the next step of the approach.
    pub replan_at: u32,
    /// Block `+0x37`, `+0x3b`, `+0x3d`, `+0x41`: args[7], [10], [8], [9].
    pub flag: u32,
    pub kind: i16,
    pub area_players: u32,
    pub area_classes: u32,
    /// Block `+0x49` / `+0x4d` (0x4d41b0): the group's ships of some
    /// strength and their strengths' sum.
    pub armed: i32,
    pub strength: i32,
    /// Block `+0x61`: the route a Shift move click gives it (0x49de70 ->
    /// 0x4ce5e0 / 0x4ce6c0). The block's own code (0x4d5f50, 0x4d61d0
    /// and what they call) never reads it: it is only shown under Shift
    /// (0x4a2050) and freed with the block (0x4a1c60); a rewrite
    /// (0x4d5f50 on a `{0x70, 0x20}`) keeps it.
    #[serde(default)]
    pub route: Option<super::gather::Route>,
}

/// What 0x4d5100 makes of a slot: the ship, its cell and the cell the
/// planner gave it.
#[derive(Debug, Clone, Copy)]
struct Placed {
    ship: u32,
    cell: (i32, i32),
    to: (i32, i32),
}

/// Objects' `+0x64`, written as the navy takes them on (0x4a8150, players
/// 0-20): a ship 0x59b3b8[class - 200], the Argo (`+0x259`) [0x59b3e8];
/// classes 0x32-0x4a 0x59b3f0[class - 0x32]; nothing else writes it for
/// them here (the constructor 0x50b960 zeroes it).
const SHIP_WORTH: [i32; 12] = [230, 300, 330, 340, 240, 210, 150, 140, 300, 350, 350, 350];
const ARGO_WORTH: i32 = 500;
const BUILDING_WORTH: [i32; 25] = [
    120, 150, 300, 400, 500, 500, 500, 650, 750, 750, 300, 750, 750, 800, 900, 1000, 400, 300, 400,
    200, 330, 300, 300, 0, -1,
];

/// 0x4d61d0's period: the pass runs once group `+8` is over 0x46.
pub const PASS: u32 = 70;

impl Scene {
    /// The human's attack zone (0x49d790 case 10) for `ships` over the
    /// rectangle the drag gave (0x51cdd0: its start and end cells,
    /// ordered, `{x0, y0, x1 - x0 + 1, y1 - y0 + 1}`). The selection is
    /// the group (0x49e230 keeps the group while the selection is
    /// unchanged; else 0x49ca60 makes a new one, its ships leaving their
    /// old groups). First 0x4c4350 `{0, 0x4a2f10(player), -1, -1}`: group
    /// `+0x19`, `+0x1d`, `+0x21` and command `#` sub 1 to each ship
    /// (0x4c42f0, queued; 0x4a2f10 every player bit but its own and those
    /// of players 0-19 not at war with it: the port's players are all at
    /// war). Then 0x4d5f50 with args `{x, y, w, h, 0, 0, 0, 0, players,
    /// -1, -1}`: a block of another kind ended (0x4a1c60), none made
    /// (0x4d4c30), else rewritten.
    pub fn order_attack_zone(
        &mut self,
        ships: &[u32],
        a: (f32, f32),
        b: (f32, f32),
    ) -> Result<(), String> {
        let t = TILE_SIZE as f32;
        let cell = |v: f32| (v / t) as i32;
        let (ax, ay, bx, by) = (cell(a.0), cell(a.1), cell(b.0), cell(b.1));
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let (y0, y1) = (ay.min(by), ay.max(by));
        let rect = (x0, y0, x1 - x0 + 1, y1 - y0 + 1);
        let ships: Vec<u32> = ships
            .iter()
            .copied()
            .filter(|&id| {
                self.object_index(id)
                    .is_some_and(|i| self.objects[i].ship.is_some())
            })
            .collect();
        if ships.is_empty() {
            return Err("No such ship".into());
        }
        let owner = ships
            .iter()
            .find_map(|&id| self.object_index(id).and_then(|i| self.objects[i].owner));
        let Some((nation, slot)) = owner else {
            return Err("No such ship".into());
        };
        let players = !(1u32 << ((slot + nation * 4) & 0x1f));
        let (classes, third) = (u32::MAX, u32::MAX);
        // The group: the selection's own (its block of another kind ended,
        // 0x4a1c60, its serial and `+8` kept; its behaviour rewritten
        // below), or a new one.
        let z = match self.selection_attack(&ships) {
            Some(z) => Some(z),
            None => {
                let (serial, elapsed) = match self.end_selection_block(&ships) {
                    Some((serial, elapsed, _, _)) => (serial, elapsed),
                    None => {
                        self.leave_zones(&ships);
                        self.zone_serial += 1;
                        (self.zone_serial, 0)
                    }
                };
                self.human_attacks.push(HumanAttack {
                    serial,
                    owner,
                    ships: ships.clone(),
                    notices: Vec::new(),
                    elapsed,
                    players,
                    classes,
                    third,
                    made: 0,
                    rect,
                    mode: 0,
                    phase: 0,
                    goal: (0, 0),
                    formation: (0, 0),
                    targets: Vec::new(),
                    slots: Vec::new(),
                    replan_at: 0,
                    flag: 0,
                    kind: -1,
                    area_players: players,
                    area_classes: u32::MAX,
                    armed: 0,
                    strength: 0,
                    route: None,
                });
                let z = self.human_attacks.len() - 1;
                // 0x4d4c30: the block zeroed, `{0x70, 0x20, now}`, then as
                // a rewrite but `+0x21`; the notices made (0x4a1150, which
                // sends nothing for the human's side 1).
                let mut a = self.human_attacks.remove(z);
                a.made = self.ticks;
                a.mode = 1;
                self.ha_list_targets(&mut a);
                self.ha_strength(&mut a);
                self.human_attacks.insert(z, a);
                for &id in &ships {
                    self.post(Order::BehaviourCommand {
                        ship: id,
                        sub: 1,
                        players,
                        classes,
                        third,
                    });
                }
                let mut a = self.human_attacks.remove(z);
                // 0x4d4260(0) returns at once (`+0x51..+0x57` are never
                // set), then 0x4d5850.
                self.ha_place(&mut a);
                self.human_attacks.insert(z, a);
                return Ok(());
            }
        };
        let z = z.unwrap();
        for &id in &ships {
            self.post(Order::BehaviourCommand {
                ship: id,
                sub: 1,
                players,
                classes,
                third,
            });
        }
        let mut a = self.human_attacks.remove(z);
        a.players = players;
        a.classes = classes;
        a.third = third;
        // The rewrite: `{0x70, 0x20, now}`, the area, args[7], [10], [8],
        // [9], mode 1, `+0x21 = 0xb`; the targets listed again, the
        // strengths counted. The lists, goal and cells are kept.
        a.made = self.ticks;
        a.rect = rect;
        a.flag = 0;
        a.kind = -1;
        a.area_players = players;
        a.area_classes = u32::MAX;
        a.mode = 1;
        a.phase = 0xb;
        self.ha_list_targets(&mut a);
        self.ha_strength(&mut a);
        self.ha_place(&mut a);
        self.human_attacks.insert(z, a);
        Ok(())
    }

    /// The human attack group whose ships are exactly `ships`.
    pub fn selection_attack(&self, ships: &[u32]) -> Option<usize> {
        let mut sel: Vec<u32> = ships.to_vec();
        sel.sort_unstable();
        sel.dedup();
        self.human_attacks.iter().position(|a| {
            let mut s = a.ships.clone();
            s.sort_unstable();
            !s.is_empty() && s == sel
        })
    }

    /// The human attack group holding ship `id`.
    pub fn ship_attack(&self, id: u32) -> Option<usize> {
        self.human_attacks
            .iter()
            .position(|a| a.ships.contains(&id))
    }

    /// Ships leaving their attack groups (0x49ca60, or sunk: 0x4a8410 ->
    /// 0x49bef0) post notice 0x137 to the group (0x4a1770) before they
    /// are taken out; a group left with none goes.
    pub(super) fn leave_attacks(&mut self, ids: &[u32]) {
        let tick = self.ticks;
        for a in &mut self.human_attacks {
            for &s in &a.ships {
                if ids.contains(&s) {
                    a.notices.push(Notice {
                        code: 0x137,
                        tick,
                        ship: s,
                        class: 0,
                    });
                }
            }
            a.ships.retain(|s| !ids.contains(s));
        }
        self.human_attacks.retain(|a| !a.ships.is_empty());
    }

    /// Ships sunk or taken leave their groups (0x4a8410 -> 0x49bef0),
    /// before the navy's walk.
    pub(super) fn drop_gone_attacks(&mut self) {
        let gone: Vec<u32> = self
            .human_attacks
            .iter()
            .flat_map(|a| a.ships.iter().copied())
            .filter(|&id| {
                self.object_index(id)
                    .is_none_or(|i| self.objects[i].ship.is_none())
            })
            .collect();
        if !gone.is_empty() {
            self.leave_attacks(&gone);
        }
    }

    /// The navy's tick for the attack groups (0x4d61d0 each).
    #[cfg(test)]
    pub(super) fn run_human_attacks(&mut self) {
        self.drop_gone_attacks();
        for z in 0..self.human_attacks.len() {
            let mut a = self.human_attacks.remove(z);
            self.ha_tick(&mut a);
            self.human_attacks.insert(z, a);
        }
    }

    /// 0x4d61d0: with no notice acted on (0x4d4e50) and group `+8` over
    /// 70, `+8 = 0` and each ship of the group: lying still (`+0x58c` 0)
    /// with a slot (0x4d4bd0 by its id), in mode 1 with no goal 0x4d5850,
    /// else in modes 1 and 3 more than a cell (0x557440) from its slot's
    /// cell sent there (0x4cf070), in mode 2 0x4d60c0; under way, in mode
    /// 2 0x4d60c0. Then mode 1 plans again (0x4d5a80). `+8` grows by the
    /// tick.
    pub(super) fn ha_tick(&mut self, a: &mut HumanAttack) {
        let heard = if a.notices.is_empty() {
            0
        } else {
            self.ha_notices(a)
        };
        if heard == 0 && a.elapsed > PASS {
            a.elapsed = 0;
            if !a.ships.is_empty() {
                for id in a.ships.clone() {
                    let Some(i) = self.ha_ship(id) else {
                        continue;
                    };
                    let still = self.objects[i].ship.as_ref().unwrap().move_state == 0;
                    if !still {
                        if a.mode == 2 {
                            self.ha_retarget(a, i);
                        }
                        continue;
                    }
                    let Some(s) = a.slots.iter().position(|s| s.ship == id) else {
                        continue;
                    };
                    match a.mode {
                        1 if a.goal.0 == -1 || a.goal.1 == -1 => {
                            self.ha_place(a);
                        }
                        1 | 3 => {
                            let to = a.slots[s].cell;
                            if formation::cells_apart(self.ha_cell(i), to) > 1 {
                                self.ha_move(id, to);
                            }
                        }
                        2 => {
                            self.ha_retarget(a, i);
                        }
                        _ => {}
                    }
                }
                if a.mode == 1 {
                    self.ha_replan(a);
                }
            }
        }
        a.elapsed += 1;
    }

    /// 0x4d4e50: the notices, newest first, each removed; one whose ship
    /// is not in the group (0x4cc590) dropped, but 0x137. 300 (a ship
    /// joined): mode 1 with no slots 0x4d5850; modes 1 and 3 with slots a
    /// slot drawn `% count` (the navy's generator) whose ship is sent to
    /// its cell; mode 2 0x4d60c0 with the ship; then `#` sub 1 with the
    /// group's three (0x4c42f0). 0x136 (at its move goal): in mode 1 with
    /// 0x4d4d60 at 100, 0x4d5560 and mode 2. 0x137 (left): its slot and
    /// its id's target entry removed. 0x13a: the target entry of its id
    /// (`rec[4]`) removed and the area's target (0x4d6640) attacked by the
    /// ship (0x4d0140; 0x13a from 0x4c09c0, a target gone, and 0x4b6f20,
    /// a prize taken); others (0x13c, 0x13d) dropped. Returns what
    /// 0x4d61d0 tests: the sum of the cases' counts, which 0x136's
    /// 0x4d5560 overwrites with its -1.
    fn ha_notices(&mut self, a: &mut HumanAttack) -> i32 {
        let mut heard: i32 = 0;
        while let Some(n) = a.notices.pop() {
            let ship = a
                .ships
                .contains(&n.ship)
                .then(|| self.ha_ship(n.ship))
                .flatten();
            if ship.is_none() && n.code != 0x137 {
                continue;
            }
            match n.code {
                300 => {
                    let i = ship.unwrap();
                    match a.mode {
                        1 if a.slots.is_empty() => heard += self.ha_place(a),
                        1 | 3 if !a.slots.is_empty() => {
                            let k = self.navy_draw() % a.slots.len() as u32;
                            let s = a.slots[k as usize];
                            self.ha_move(s.ship, s.cell);
                            heard += 1;
                        }
                        2 => heard += self.ha_retarget(a, i),
                        _ => {}
                    }
                    heard += 1;
                    self.post(Order::BehaviourCommand {
                        ship: n.ship,
                        sub: 1,
                        players: a.players,
                        classes: a.classes,
                        third: a.third,
                    });
                }
                0x136 => {
                    if a.mode == 1 && self.ha_gathered(a) == 100 {
                        heard = self.ha_deal(a);
                        if heard != 0 {
                            a.mode = 2;
                        }
                    }
                }
                0x137 => {
                    if let Some(s) = a.slots.iter().position(|s| s.ship == n.ship) {
                        a.slots.remove(s);
                    }
                    if let Some(t) = a.targets.iter().position(|t| t.id == n.ship) {
                        a.targets.remove(t);
                    }
                }
                0x13a => {
                    if let Some(t) = a.targets.iter().position(|t| t.id == n.class) {
                        a.targets.remove(t);
                    }
                    let owner = a.owner.unwrap_or_default();
                    if let Some(k) = self.area_foe(owner, a.players, a.classes, a.rect) {
                        let target = self.objects[k].id;
                        self.post(Order::Attack {
                            ship: n.ship,
                            target,
                        });
                    }
                    heard += 1;
                }
                _ => {}
            }
        }
        heard
    }

    /// 0x4d4d60: the share of the slots, in percent, whose cell lies
    /// within 1 of the goal in x and in y (a slot counted with its ship
    /// there); 100 also when all but one are, for 4-9 slots.
    fn ha_gathered(&self, a: &HumanAttack) -> i32 {
        let n = a.slots.len() as i32;
        if n == 0 {
            return 0;
        }
        let c = a
            .slots
            .iter()
            .filter(|s| {
                self.ha_ship(s.ship).is_some()
                    && (s.cell.0 - a.goal.0).abs() < 2
                    && (s.cell.1 - a.goal.1).abs() < 2
            })
            .count() as i32;
        gathered(c, n)
    }

    /// 0x4d5850: a slot for each ship of the group (0x4d5360) and, with
    /// any, the goal (0x4d43b0): found, the targets dealt (0x4d5560, its
    /// -1 returned) and mode 2; else mode 1.
    fn ha_place(&mut self, a: &mut HumanAttack) -> i32 {
        if self.ha_slots(a) > 0 {
            if self.ha_goal(a) != 0 {
                let r = self.ha_deal(a);
                a.mode = 2;
                return r;
            }
            a.mode = 1;
        }
        0
    }

    /// 0x4d5360: each ship of the group gets its slot at its cell, its
    /// target cleared (0x4d4b10: the slot of its id rewritten, else one
    /// appended). Returns the slots written.
    fn ha_slots(&mut self, a: &mut HumanAttack) -> i32 {
        let mut n = 0;
        for id in a.ships.clone() {
            let Some(i) = self.ha_ship(id) else {
                continue;
            };
            Self::slot_put(a, id, self.ha_cell(i));
            n += 1;
        }
        n
    }

    /// 0x4d4b10(ship, 0, 0, cell).
    fn slot_put(a: &mut HumanAttack, ship: u32, cell: (i32, i32)) {
        let slot = Slot {
            ship,
            target: 0,
            target_id: 0,
            cell,
        };
        match a.slots.iter_mut().find(|s| s.ship == ship) {
            Some(s) => *s = slot,
            None => a.slots.push(slot),
        }
    }

    /// 0x4d43b0: the goal. No targets: `(-1, -1)`, 0. Two or more: two
    /// draws of the navy's generator, `a = % 3 + 1` and `b = % 3 + 1`, and
    /// 0x4cd120 puts one target first (failing, goal `(-1, -1)`, 0). The
    /// first target's cell the goal when water (0x4999e0); else the route
    /// from the group's centre (0x4c56e0) towards it (0x4d0e70), its last
    /// water step the goal (none, or no route: 0). Then the formation
    /// about the target's cell with slot class 6 (0x4d5100; none, 0), its
    /// cell `+0x27`. Every ship more than 6 (0x557440) from its cell: the
    /// centre's route to the goal (0x4d58b0) of 11 steps or more gives its
    /// 7th step, a class-0 formation there sent (0x4d59f0), `+0x27` that
    /// cell, `+0x21 = 10`, `+0x45 = now + 300 x` the planner's value;
    /// 0 (fewer steps: nothing). Else each slot takes its cell of the
    /// class-6 formation, `+0x21 = 0xb`, and the count of slots is
    /// returned (the threat count is never added: player `+0x40` is 0).
    fn ha_goal(&mut self, a: &mut HumanAttack) -> i32 {
        let n = a.targets.len();
        if n < 1 {
            a.goal = (-1, -1);
            return 0;
        }
        if n >= 2 {
            let x = self.navy_draw() % 3 + 1;
            let y = self.navy_draw() % 3 + 1;
            if !self.ha_pick(a, (y * 10) << 16 | x) {
                a.goal = (-1, -1);
                return 0;
            }
        }
        let at = a.targets[0].cell;
        if self.nav.is_sea(at.0, at.1) {
            a.goal = at;
        } else {
            let Some(c) = self.ha_centre(a) else {
                return 0;
            };
            let Some(route) = self.ha_route(c, at) else {
                return 0;
            };
            let Some(&(x, y, _)) = route.iter().rev().find(|r| self.nav.is_sea(r.0, r.1)) else {
                return 0;
            };
            a.goal = (x, y);
        }
        let Some((placed, _)) = self.ha_formation(a, at, 6) else {
            return 0;
        };
        a.formation = at;
        if placed
            .iter()
            .all(|p| formation::cells_apart(p.to, p.cell) > 6)
        {
            if let Some(c) = self.ha_centre(a) {
                if let (1, step) = self.ha_step(c, a.goal) {
                    if let Some((placed, value)) = self.ha_formation(a, step, 0) {
                        self.ha_send(a, &placed);
                        a.formation = step;
                        a.phase = 10;
                        a.replan_at = self.ticks.wrapping_add((value * 300) as u32);
                    }
                }
            }
            return 0;
        }
        let mut count = 0;
        for p in &placed {
            if let Some(s) = a.slots.iter_mut().find(|s| s.ship == p.ship) {
                s.cell = p.to;
                count += 1;
            }
        }
        a.phase = 0xb;
        count
    }

    /// 0x4cd120(targets, n, `mode`): `mode >> 16` 10 or 0x1e weighs each
    /// target by `mode & 0xffff` (1 its strength, 2 its `+0x64`, 3 both,
    /// else 0); 10 takes the greatest (the first of equals); 0x1e, with a
    /// sum of 0, one draw `% n`, else five roulette draws `% sum` keeping
    /// the greatest weight hit; 0x14 one draw `% n`. The one chosen is
    /// swapped to the front (index 0 left); none, false.
    fn ha_pick(&mut self, a: &mut HumanAttack, mode: u32) -> bool {
        let n = a.targets.len();
        let (hi, lo) = (mode >> 16, mode & 0xffff);
        let idx: i64 = match hi {
            10 | 0x1e => {
                let weights: Vec<i32> = a
                    .targets
                    .iter()
                    .map(|t| match lo {
                        1 => t.strength,
                        2 => t.value,
                        3 => t.value.wrapping_add(t.strength),
                        _ => 0,
                    })
                    .collect();
                let mut best = -1;
                let mut idx = -1i64;
                let mut sum = 0u32;
                for (k, &w) in weights.iter().enumerate() {
                    if best < w {
                        best = w;
                        idx = k as i64;
                    }
                    sum = sum.wrapping_add(w as u32);
                }
                if hi != 10 {
                    idx = -1;
                    if sum == 0 {
                        idx = (self.navy_draw() % n as u32) as i64;
                    } else {
                        let mut top = 0;
                        for _ in 0..5 {
                            let r = (self.navy_draw() % sum) as i32;
                            let mut cum = 0i32;
                            for (k, &w) in weights.iter().enumerate() {
                                cum = cum.wrapping_add(w);
                                if r < cum {
                                    if top < w {
                                        top = w;
                                        idx = k as i64;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                idx
            }
            0x14 => (self.navy_draw() % n as u32) as i64,
            _ => return true,
        };
        if idx >= 1 && (idx as usize) < n {
            a.targets.swap(0, idx as usize);
        } else if idx != 0 {
            return false;
        }
        true
    }

    /// 0x4c56e0: the mean cell of the group's ships (`+0x5a3 / +0x5a7`;
    /// its kind-0x32 objects, weighing 2, are not the port's: its human
    /// groups are ships only); none without ships.
    fn ha_centre(&self, a: &HumanAttack) -> Option<(i32, i32)> {
        let (mut n, mut sx, mut sy) = (0, 0, 0);
        for &id in &a.ships {
            let Some(i) = self.ha_ship(id) else {
                continue;
            };
            let (x, y) = self.ha_cell(i);
            n += 1;
            sx += x;
            sy += y;
        }
        (n != 0).then(|| (sx / n, sy / n))
    }

    /// 0x4d0e70 over a copy of the shallow grid (0x5c93bc): none from a
    /// closed start; to a closed goal the route 0x55ba50 (heading 0,
    /// tolerance 1), else 0x55ca10.
    fn ha_route(&self, from: (i32, i32), to: (i32, i32)) -> Option<walk_path::Route> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let mut m = self.ha_grid(false);
        let at = |c: (i32, i32)| (c.1 * w + c.0) as usize;
        if m[at(from)] == -1 {
            return None;
        }
        let route = if m[at(to)] == -1 {
            walk_path::route(&mut m, w, h, from, to, 0, 1).map(|r| r.0)
        } else {
            walk_path::route_exact(&mut m, w, h, from, to, 0)
        };
        route.filter(|r| !r.is_empty())
    }

    /// 0x4d58b0: the route 0x55ba50 (heading 0, no tolerance) over a copy
    /// of the shallow grid from `from` towards `to`: none, 0; fewer than
    /// 11 entries, 2; else 1 and its 7th entry.
    fn ha_step(&self, from: (i32, i32), to: (i32, i32)) -> (u8, (i32, i32)) {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let mut m = self.ha_grid(false);
        match walk_path::route(&mut m, w, h, from, to, 0, 0) {
            None => (0, (0, 0)),
            Some((r, _)) if r.len() < 11 => (2, (0, 0)),
            Some((r, _)) => (1, (r[6].0, r[6].1)),
        }
    }

    /// 0x4d5100: a 0x24-byte record for each slot whose ship is there
    /// (its cell `+0x5a3 / +0x5a7`, `+8` the class) to the planner
    /// 0x5614e0 about `to`. The draught flag is written after the record
    /// pointer has stepped on (0x4d51be, then 0x4d51d5 / 0x4d51e6): ship
    /// i's `+0x592 != 0` lands in record i + 1's `+0xc`, record 0's stays
    /// 0. The grids the other way about from 0x4cc970's: the first the
    /// deep grid (0x5c9398) when a ship has `+0x592` 0, the second the
    /// shallow one (0x5c93bc) when one has it set; no third (player
    /// `+0x40` is 0). The planner's value returned with the cells; none
    /// when it fails (a negative value clears the count). (Not decoded:
    /// the planner's `Fail::Unsettled` cases, which go on with stack words
    /// never written; taken here as a failure.)
    fn ha_formation(
        &self,
        a: &HumanAttack,
        to: (i32, i32),
        class: i32,
    ) -> Option<(Vec<Placed>, i32)> {
        let ships: Vec<(u32, (i32, i32), bool)> = a
            .slots
            .iter()
            .filter_map(|s| {
                let i = self.ha_ship(s.ship)?;
                let kind = self.objects[i].ship.as_ref()?.kind;
                Some((s.ship, self.ha_cell(i), crate::objects::deep_draught(kind)))
            })
            .collect();
        let recs: Vec<formation::Rec> = ships
            .iter()
            .enumerate()
            .map(|(k, s)| formation::Rec {
                x: s.1 .0,
                y: s.1 .1,
                class,
                second: k > 0 && ships[k - 1].2,
            })
            .collect();
        let (deep, shallow) = (self.ha_grid(true), self.ha_grid(false));
        let p1 = ships.iter().any(|s| !s.2).then_some(&deep[..]);
        let p2 = ships.iter().any(|s| s.2).then_some(&shallow[..]);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let planned = formation::plan_records(p1, p2, None, (w, h), to, &recs).ok()?;
        if planned.value < 0 {
            return None;
        }
        let placed = ships
            .iter()
            .zip(&planned.cells)
            .map(|(s, &to)| Placed {
                ship: s.0,
                cell: s.1,
                to,
            })
            .collect();
        Some((placed, planned.value))
    }

    /// 0x4d59f0: each ship's slot rewritten with its cell (0x4d4b10, its
    /// target cleared) and the ship sent there (0x4cf070).
    fn ha_send(&mut self, a: &mut HumanAttack, placed: &[Placed]) {
        for p in placed {
            Self::slot_put(a, p.ship, p.to);
            self.ha_move(p.ship, p.to);
        }
    }

    /// 0x4d5560: the targets listed again (0x4d4860) and checked
    /// (0x4d5430); with slots and targets, the targets' cells dealt to
    /// the slots' ships (0x561a30, `formation::deal_cells`; a slot's
    /// ship's cell, vt+0x28) and each ship given the first target of its
    /// cell: slot `+8`, a ship of class 0xd2 sent there (0x4cf070), the
    /// others `+0xc` the target's id and an attack on it (0x4d0140 with
    /// the block's players, 7). Returns -1.
    fn ha_deal(&mut self, a: &mut HumanAttack) -> i32 {
        self.ha_list_targets(a);
        let slots = a.slots.len();
        self.ha_check_targets(a);
        if slots == 0 || a.targets.is_empty() {
            return -1;
        }
        let ships: Vec<(u32, (i32, i32))> = a
            .slots
            .iter()
            .map(|s| {
                let cell = self.ha_ship(s.ship).map_or(s.cell, |i| self.ha_cell(i));
                (s.ship, cell)
            })
            .collect();
        let cells: Vec<(i32, i32)> = a.targets.iter().map(|t| t.cell).collect();
        let at: Vec<(i32, i32)> = ships.iter().map(|s| s.1).collect();
        let Some(dealt) = formation::deal_cells(&cells, &at) else {
            return -1;
        };
        for (k, &(ship, _)) in ships.iter().enumerate() {
            let Some(s) = a.slots.iter().position(|s| s.ship == ship) else {
                continue;
            };
            let Some(t) = a.targets.iter().find(|t| t.cell == dealt[k]).copied() else {
                continue;
            };
            let Some(i) = self.ha_ship(ship) else {
                continue;
            };
            a.slots[s].target = t.id;
            if self.objects[i].ship.as_ref().unwrap().kind == 10 {
                self.ha_move(ship, t.cell);
            } else {
                a.slots[s].target_id = t.id;
                self.post(Order::Attack { ship, target: t.id });
            }
        }
        -1
    }

    /// 0x4d4860 with no ids: with an area (`+0x14` not 0) each of
    /// 0x4cd300's records appended to the targets (0x5577d0) and then
    /// merged by id (0x4d49f0: the first entry of its id rewritten), so a
    /// target listed again is listed twice.
    fn ha_list_targets(&self, a: &mut HumanAttack) {
        if a.rect.2 == 0 {
            return;
        }
        for t in self.ha_area_targets(a) {
            a.targets.push(t);
            let k = a.targets.iter().position(|e| e.id == t.id).unwrap();
            a.targets[k] = t;
        }
    }

    /// 0x4cd300 for the block: the area's cells within the map, row by
    /// row, each cell's object `+0xc` (harpies and dragons), else its
    /// occupant `+4` (`cell_occupants`), else `+0x10` (the gorgon, the
    /// sea monster), of a player in the block's players (`+0x3d`) and at
    /// war (0x4efe20 < 0: the port's players are all at war; its
    /// ownerless creatures are no player's, as in 0x4d6640's port);
    /// classes 0x32 / 0x33 and a land creatures' jam (0x1f7, by its first
    /// member, 0x415df0 / 0x415db0, strength 0) listed once; the strength
    /// a ship's fire power (`+0x479`), a fort's (0x34-0x36, 0x3f-0x41)
    /// its units' fire (0x446710 summed), else 0; the cell its own (vt+0x28).
    /// (A ship is skipped on the cell it has reserved, `+0x5ab`: the port's
    /// occupants hold the cells the ships are in only. Not the port's:
    /// fireballs and tornadoes holding `+0xc`.)
    fn ha_area_targets(&self, a: &HumanAttack) -> Vec<Target> {
        self.area_targets(a.rect, a.owner, a.area_players)
    }

    /// 0x4cd300 over `rect` for a group of `own` with players `players`
    /// (`ha_area_targets`; the guard block's, `+0x19`).
    pub(super) fn area_targets(
        &self,
        rect: (i32, i32, i32, i32),
        own: Option<(u32, u32)>,
        players: u32,
    ) -> Vec<Target> {
        let (x0, y0, rw, rh) = rect;
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let occupants =
            self.cell_occupants((x0.max(0), y0.max(0)), ((x0 + rw).min(w), (y0 + rh).min(h)));
        let t = TILE_SIZE as f32;
        let at = |k: usize| {
            let o = &self.objects[k];
            ((o.x / t).floor() as i32, (o.y / t).floor() as i32)
        };
        // `+0xc`: harpies and dragons; `+0x10`: the gorgon, the sea
        // monster (`sea_monster_at`), each by its cell.
        let mut flyers: Vec<((i32, i32), usize)> = Vec::new();
        let mut deep: Vec<((i32, i32), usize)> = Vec::new();
        for (k, o) in self.objects.iter().enumerate() {
            match o.class {
                ObjectClass::Monster(0x1f8 | 0x1f9) => flyers.push((at(k), k)),
                ObjectClass::Monster(0x1fa) => deep.push((at(k), k)),
                ObjectClass::Monster(0x1fb) => {
                    if let Some(m) = self.sea_monster_at(at(k), 0) {
                        deep.push((at(k), m));
                    }
                }
                _ => {}
            }
        }
        let first =
            |list: &[((i32, i32), usize)], cell| list.iter().find(|c| c.0 == cell).map(|c| c.1);
        let mut out: Vec<Target> = Vec::new();
        let mut listed: Vec<usize> = Vec::new();
        for y in y0..y0 + rh {
            if y < 0 || y >= h {
                continue;
            }
            for x in x0..x0 + rw {
                if x < 0 || x >= w {
                    continue;
                }
                let cell = (x, y);
                let Some(k) = first(&flyers, cell)
                    .or_else(|| first(&occupants, cell))
                    .or_else(|| first(&deep, cell))
                else {
                    continue;
                };
                let o = &self.objects[k];
                let Some(p) = o.owner else {
                    continue;
                };
                if players & 1 << ((p.1 + p.0 * 4) & 0x1f) == 0 || Some(p) == own {
                    continue;
                }
                let class = Self::ha_class(o);
                let jam = self
                    .jams
                    .iter()
                    .any(|j| j.cell == cell && j.members.first() == Some(&o.id));
                if (jam || matches!(class, 0x32 | 0x33)) && listed.contains(&k) {
                    continue;
                }
                let strength = if jam {
                    0
                } else if o.ship.is_some() {
                    self.fire_power(k)
                } else if matches!(class, 0x34..=0x36 | 0x3f..=0x41) {
                    self.units_fire(k)
                } else {
                    0
                };
                out.push(Target {
                    id: o.id,
                    class,
                    strength,
                    cell: self.cell_of(k),
                    value: self.worth(k),
                });
                listed.push(k);
            }
        }
        out
    }

    /// 0x4d5430: the targets, newest first: one that still exists
    /// (0x51f1f0) and is at war (0x4efe20 < 0: another player's) or of a
    /// player in the group's players (`+0x19`) kept with its fields new
    /// (its class, strength 0x50ea70, cell, `+0x64`); else removed.
    fn ha_check_targets(&self, a: &mut HumanAttack) {
        let mut k = a.targets.len();
        while k > 0 {
            k -= 1;
            let keep = self.object_index(a.targets[k].id).filter(|&i| {
                self.objects[i].owner.is_some_and(|p| {
                    Some(p) != a.owner || a.players & 1 << ((p.1 + p.0 * 4) & 0x1f) != 0
                })
            });
            match keep {
                Some(i) => {
                    let t = &mut a.targets[k];
                    t.class = Self::ha_class(&self.objects[i]);
                    t.strength = self.strength(i);
                    t.cell = self.cell_of(i);
                    t.value = self.worth(i);
                }
                None => {
                    a.targets.remove(k);
                }
            }
        }
    }

    /// 0x4d41b0: the group's ships of some strength (0x50ea70) counted
    /// into `+0x49`, their strengths summed into `+0x4d`.
    fn ha_strength(&self, a: &mut HumanAttack) {
        let (mut n, mut sum) = (0, 0);
        for &id in &a.ships {
            let Some(i) = self.ha_ship(id) else {
                continue;
            };
            let s = self.strength(i);
            if s != 0 {
                n += 1;
                sum += s;
            }
        }
        a.armed = n;
        a.strength = sum;
    }

    /// 0x4d60c0 for ship `i`: one not boarding or ramming (`+0x25d &
    /// 0x10`) and off a fire task (`& 0x100`), or on one in fire state
    /// 2, or 1 with none of its four position bytes (`+0x645`, `+0x657`,
    /// `+0x669`, `+0x67b`) 3, looks for the area's target (0x4d6640 with
    /// the group's players and classes); found, a ship of type 10
    /// (`+0x25a`) is sent to its cell (0x4cf070), the others attack it
    /// (0x4d0140); 1.
    fn ha_retarget(&mut self, a: &HumanAttack, i: usize) -> i32 {
        let s = self.objects[i].ship.as_ref().unwrap();
        let b = &s.behaviour;
        let free = !s.fire_task()
            || b.fire_state == 2
            || b.fire_state == 1 && b.aims.iter().all(|a| a.state != 3);
        if !free || self.closing_in(i) {
            return 0;
        }
        let ship = self.objects[i].id;
        let kind = s.kind;
        let owner = a.owner.unwrap_or_default();
        if let Some(k) = self.area_foe(owner, a.players, a.classes, a.rect) {
            if kind == 10 {
                let cell = self.cell_of(k);
                self.ha_move(ship, cell);
            } else {
                let target = self.objects[k].id;
                self.post(Order::Attack { ship, target });
            }
        }
        1
    }

    /// 0x4d5a80 (nothing under 2 slots): the shallow grid flooded from
    /// the formation's cell `+0x27` (0x558fa0, limit 5000) and read at
    /// the slots' ships' cells, newest first: `lo`, `hi` and the ship at
    /// `lo` (the lead; the first of equals in that order). Under 2 ships
    /// placed: 0x4d5560 (mode 2), then with the formation's cell the goal
    /// 0x4d5560 again. Else `far = |flood(+0x27) - hi| / 2`, `half = (hi -
    /// lo) / 2`: the formation's cell the goal and `far` at most 9,
    /// 0x4d5560 (mode 2). Else with `half` above 0: 0x4d5970 true, at
    /// `+0x21 = 0xb` or with `+0x45` past, a class-0 formation about the
    /// lead's cell, its cells' flood spread halved; that not 0x4d5970-wide
    /// and 0x4d58b0(lead, goal) 1, a class-0 formation at the route's 7th
    /// step sent, `+0x27` there, `+0x45 = now + 300 x value`; otherwise
    /// `+0x21 = 10`. False: 0x4d58b0(lead, goal) 1, the formation there
    /// sent, `+0x21 = 10`, `+0x45`; 2, 0x4d5560 (mode 2); 0 nothing.
    fn ha_replan(&mut self, a: &mut HumanAttack) {
        if a.slots.len() < 2 {
            return;
        }
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let mut m = self.ha_grid(false);
        walk_path::wave(
            &mut m,
            w,
            (0, 0, w - 1, h - 1),
            a.formation,
            (-1, -1),
            5000,
            -1,
            0,
            0,
        );
        let flood = |c: (i32, i32)| m[(c.1 * w + c.0) as usize] as i32;
        let (mut lo, mut hi, mut lead, mut placed) = (0, 0, 0usize, 0);
        for s in (0..a.slots.len()).rev() {
            let Some(i) = self.ha_ship(a.slots[s].ship) else {
                continue;
            };
            let v = flood(self.ha_cell(i));
            if placed == 0 {
                (lo, hi, lead) = (v, v, s);
            } else if v < lo {
                (lo, lead) = (v, s);
            } else if v > hi {
                hi = v;
            }
            placed += 1;
        }
        let (far, half) = if placed < 2 {
            self.ha_attack(a);
            (0, 0)
        } else {
            ((flood(a.formation) - hi).abs() / 2, (hi - lo) / 2)
        };
        if a.formation == a.goal && far <= 9 {
            self.ha_attack(a);
            return;
        }
        if half < 1 {
            return;
        }
        let lead_cell = self.ha_ship(a.slots[lead].ship).map(|i| self.ha_cell(i));
        let now = self.ticks;
        if wide(placed, half) {
            if a.phase == 0xb || a.replan_at <= now {
                if let Some(c) = lead_cell {
                    if let Some((about, _)) = self.ha_formation(a, c, 0) {
                        let (mut lo2, mut hi2) = (0, 0);
                        for (k, p) in about.iter().enumerate() {
                            let v = flood(p.to);
                            if k == 0 {
                                (lo2, hi2) = (v, v);
                            } else if v < lo2 {
                                lo2 = v;
                            } else if v > hi2 {
                                hi2 = v;
                            }
                        }
                        if !wide(placed, (hi2 - lo2) / 2) {
                            if let (1, step) = self.ha_step(c, a.goal) {
                                if let Some((placed, value)) = self.ha_formation(a, step, 0) {
                                    self.ha_send(a, &placed);
                                    a.formation = step;
                                    a.replan_at = now.wrapping_add((value * 300) as u32);
                                    return;
                                }
                            }
                        }
                    }
                }
                a.phase = 10;
            }
            return;
        }
        let Some(c) = lead_cell else {
            return;
        };
        match self.ha_step(c, a.goal) {
            (1, step) => {
                if let Some((placed, value)) = self.ha_formation(a, step, 0) {
                    self.ha_send(a, &placed);
                    a.formation = step;
                    a.phase = 10;
                    a.replan_at = now.wrapping_add((value * 300) as u32);
                }
            }
            (2, _) => self.ha_attack(a),
            _ => {}
        }
    }

    /// 0x4d5560, then mode 2 when it gives non-zero (always: -1).
    fn ha_attack(&mut self, a: &mut HumanAttack) {
        if self.ha_deal(a) != 0 {
            a.mode = 2;
        }
    }

    /// 0x4d63d0, the block's status: with no ships `{0x1f5, result 1}`;
    /// else armed ships (a ship of type neither 0 nor 1 for which
    /// 0x4bc270(7) holds, as `Scene::report`'s) and the area's target
    /// (0x4d6640): both, 500 (running); else 0x1f5, result 10 with armed
    /// ships, 1 without. Returns the code and the result.
    // Its readers, the tasks' status calls (0x4a0d80 from 0x4a0f60), hold
    // brains' and computers' groups only.
    #[allow(dead_code)]
    pub(super) fn human_attack_status(&self, a: &HumanAttack) -> (u32, u32) {
        if a.ships.is_empty() {
            return (0x1f5, 1);
        }
        let armed = a.ships.iter().filter_map(|&id| self.ha_ship(id)).any(|i| {
            let o = &self.objects[i];
            let kind = o.ship.as_ref().unwrap().kind;
            !matches!(kind, 0 | 1)
                && (matches!(kind, 8..=11)
                    || o.combat.as_ref().is_some_and(|c| !c.weapons.is_empty()))
        });
        let owner = a.owner.unwrap_or_default();
        let target = self.area_foe(owner, a.players, a.classes, a.rect).is_some();
        match (armed, target) {
            (true, true) => (500, 0),
            (true, false) => (0x1f5, 10),
            _ => (0x1f5, 1),
        }
    }

    /// The object's class (`+0x50`): a ship's 200 + its type.
    fn ha_class(o: &crate::objects::MapObject) -> u32 {
        match o.ship.as_ref() {
            Some(s) => 200 + s.kind as u32,
            None => o.record.class_id,
        }
    }

    /// An object's `+0x64` (0x4a8150).
    pub(super) fn worth(&self, k: usize) -> i32 {
        let o = &self.objects[k];
        let Some((n, s)) = o.owner else {
            return 0;
        };
        if n * 4 + s > 20 {
            return 0;
        }
        if let Some(sh) = o.ship.as_ref() {
            return if sh.argo {
                ARGO_WORTH
            } else {
                SHIP_WORTH.get(sh.kind as usize).copied().unwrap_or(0)
            };
        }
        match o.record.class_id {
            c @ 0x32..=0x4a => BUILDING_WORTH[(c - 0x32) as usize],
            _ => 0,
        }
    }

    /// A ship of the group that is there, by id.
    fn ha_ship(&self, id: u32) -> Option<usize> {
        self.object_index(id)
            .filter(|&i| self.objects[i].ship.is_some())
    }

    /// The cell a ship is in (`+0x5a3`, `+0x5a7`).
    fn ha_cell(&self, i: usize) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        (
            (self.objects[i].x / t).floor() as i32,
            (self.objects[i].y / t).floor() as i32,
        )
    }

    /// A copy of the ships' grid by draught (0x5c9398 deep, 0x5c93bc
    /// shallow): 0 open, -1 closed.
    fn ha_grid(&self, deep: bool) -> Vec<i16> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| if self.nav.passable(x, y, deep) { 0 } else { -1 })
            .collect()
    }

    /// 0x4cf070(group, ship, x, y): command 0x14 `{x, y, 0}`, a plain move
    /// to the cell, through the outbox.
    fn ha_move(&mut self, ship: u32, (x, y): (i32, i32)) {
        let t = TILE_SIZE as f32;
        self.post(Order::Move {
            ship,
            x: x as f32 * t + t / 2.0,
            y: y as f32 * t + t / 2.0,
        });
    }
}

/// 0x4d4d60's share of `n` slots, `c` of them gathered: `c x 100 / n`,
/// or 100 for 4-9 slots with all but at most one gathered.
fn gathered(c: i32, n: i32) -> i32 {
    let pct = c * 100 / n;
    let most = (c - n).abs() < 2;
    if n < 10 {
        if n >= 7 && most {
            return 100;
        }
        if n >= 7 {
            return pct;
        }
    } else {
        return pct;
    }
    if n >= 4 && most {
        return 100;
    }
    pct
}

/// 0x4d5970(ships, spread): spread out for 1-5 ships over 3, 6-9 over 4,
/// more when the ships are fewer than half the spread.
fn wide(ships: i32, spread: i32) -> bool {
    match ships {
        ..=0 => false,
        1..=5 => spread > 3,
        6..=9 => spread > 4,
        _ => ships < spread / 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::GameData;
    use crate::objects::Caches;

    fn data() -> Option<GameData> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        Some(GameData::open(std::path::Path::new(&root)).unwrap())
    }

    /// The dwords of `ancient.exe` at `va` (file offset + 0x400000).
    fn exe_dwords(va: u32, n: usize) -> Option<Vec<i32>> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        let path = crate::assets::find_ci(std::path::Path::new(&root), "ancient.exe").ok()??;
        let bytes = std::fs::read(path).ok()?;
        let at = (va - 0x40_0000) as usize;
        Some(
            bytes[at..at + 4 * n]
                .chunks(4)
                .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
        )
    }

    #[test]
    fn worth_tables_are_the_exes() {
        let Some(ships) = exe_dwords(0x59b3b8, 12) else {
            return;
        };
        assert_eq!(ships, SHIP_WORTH);
        assert_eq!(exe_dwords(0x59b3e8, 1).unwrap(), [ARGO_WORTH]);
        assert_eq!(exe_dwords(0x59b3f0, 25).unwrap(), BUILDING_WORTH);
    }

    #[test]
    fn spread_as_0x4d5970() {
        assert!(!wide(0, 100));
        assert!(!wide(5, 3));
        assert!(wide(1, 4));
        assert!(!wide(9, 4));
        assert!(wide(6, 5));
        assert!(!wide(10, 21));
        assert!(wide(10, 22));
    }

    #[test]
    fn gathered_as_0x4d4d60() {
        assert_eq!(gathered(0, 1), 0);
        assert_eq!(gathered(1, 1), 100);
        assert_eq!(gathered(2, 3), 66);
        assert_eq!(gathered(3, 4), 100);
        assert_eq!(gathered(2, 4), 50);
        assert_eq!(gathered(6, 7), 100);
        assert_eq!(gathered(5, 7), 71);
        assert_eq!(gathered(8, 9), 100);
        assert_eq!(gathered(9, 10), 90);
        assert_eq!(gathered(8, 10), 80);
    }

    fn target(id: u32, strength: i32, value: i32) -> Target {
        Target {
            id,
            class: 200,
            strength,
            cell: (0, 0),
            value,
        }
    }

    fn block(targets: Vec<Target>) -> HumanAttack {
        HumanAttack {
            serial: 1,
            owner: Some((0, 0)),
            ships: Vec::new(),
            notices: Vec::new(),
            elapsed: 0,
            players: 0,
            classes: 0,
            third: 0,
            made: 0,
            rect: (0, 0, 0, 0),
            mode: 1,
            phase: 0,
            goal: (0, 0),
            formation: (0, 0),
            targets,
            slots: Vec::new(),
            replan_at: 0,
            flag: 0,
            kind: -1,
            area_players: 0,
            area_classes: 0,
            armed: 0,
            strength: 0,
            route: None,
        }
    }

    /// 0x4cd120: the greatest weight first, the first of equals; the
    /// roulette keeps the greatest weight hit; a draw on a sum of 0.
    #[test]
    fn pick_as_0x4cd120() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        let ids = |a: &HumanAttack| a.targets.iter().map(|t| t.id).collect::<Vec<_>>();
        let mut a = block(vec![target(1, 5, 0), target(2, 9, 0), target(3, 9, 0)]);
        assert!(scene.ha_pick(&mut a, 10 << 16 | 1));
        assert_eq!(ids(&a), [2, 1, 3]);
        let mut a = block(vec![target(1, 5, 300), target(2, 9, 0), target(3, 0, 340)]);
        assert!(scene.ha_pick(&mut a, 10 << 16 | 2));
        assert_eq!(ids(&a), [3, 2, 1]);
        let mut a = block(vec![target(1, 5, 300), target(2, 9, 0), target(3, 0, 340)]);
        assert!(scene.ha_pick(&mut a, 10 << 16 | 3));
        assert_eq!(ids(&a), [3, 2, 1]);
        // Every roulette draw lands on the one weight.
        let mut a = block(vec![target(1, 0, 0), target(2, 7, 0), target(3, 0, 0)]);
        assert!(scene.ha_pick(&mut a, 0x1e << 16 | 1));
        assert_eq!(ids(&a), [2, 1, 3]);
        // Weights of 0: one draw `% n`.
        scene.navy_lcg = Some(0);
        let n = crate::combat::Rng(0).next() % 3;
        let mut a = block(vec![target(1, 0, 0), target(2, 0, 0), target(3, 0, 0)]);
        assert!(scene.ha_pick(&mut a, 0x1e << 16 | 1));
        assert_eq!(a.targets[0].id, n + 1);
    }

    /// The order on miss000: two of the human's ships a cell or two from
    /// an enemy ship, the area its cell. The selection made a group, each
    /// ship sent `#` sub 1; the enemy listed (class, fire power, `+0x64`),
    /// the goal its cell (water), the class-6 formation about it close to
    /// the ships, so the targets are dealt: listed again (twice now, each
    /// checked to 0x50ea70's strength) and each ship sent at it, mode 2. A
    /// ship given another order leaves with notice 0x137, its slot
    /// removed at the next pass.
    #[test]
    fn human_attack_zone_on_miss000() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let foe_owner = (human.0 ^ 1, human.1);
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(3)
            .collect();
        let (i0, i1, f) = (ships[0], ships[1], ships[2]);
        let t = TILE_SIZE as f32;
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let around = |c0: (i32, i32)| -> Vec<(i32, i32)> {
            (-3..=3)
                .flat_map(|dy| (-3..=3).map(move |dx| (c0.0 + dx, c0.1 + dy)))
                .filter(|&c| c != c0 && water(&scene, c))
                .collect()
        };
        // An open stretch of sea: the first cell with all 48 about it water.
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let c0 = (3..h - 3)
            .flat_map(|y| (3..w - 3).map(move |x| (x, y)))
            .find(|&c| water(&scene, c) && around(c).len() == 48)
            .unwrap();
        let near = around(c0);
        let (c1, cf) = (near[0], near[near.len() - 1]);
        for (i, c, owner) in [(i0, c0, human), (i1, c1, human), (f, cf, foe_owner)] {
            let o = &mut scene.objects[i];
            o.owner = Some(owner);
            o.x = c.0 as f32 * t + t / 2.0;
            o.y = c.1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.caps = crate::objects::caps_mask(2);
            s.argo = false;
            s.move_state = 0;
        }
        let (id0, id1, idf) = (
            scene.objects[i0].id,
            scene.objects[i1].id,
            scene.objects[f].id,
        );
        scene.outbox.clear();
        let corner = (cf.0 as f32 * t + 1.0, cf.1 as f32 * t + 1.0);
        scene
            .order_attack_zone(&[id0, id1], corner, corner)
            .unwrap();
        assert_eq!(scene.human_attacks.len(), 1);
        let a = scene.human_attacks[0].clone();
        assert_eq!(a.rect, (cf.0, cf.1, 1, 1));
        assert_eq!(a.players, !(1 << ((human.1 + human.0 * 4) & 0x1f)));
        assert_eq!(a.goal, cf);
        assert_eq!(a.formation, cf);
        assert_eq!(a.mode, 2);
        assert_eq!(a.phase, 0xb);
        let want = Target {
            id: idf,
            class: 202,
            strength: scene.strength(f),
            cell: cf,
            value: 330,
        };
        assert_eq!(a.targets, [want, want]);
        assert_eq!(a.slots.len(), 2);
        assert!(a
            .slots
            .iter()
            .all(|s| s.target == idf && s.target_id == idf));
        let behave = |id| Order::BehaviourCommand {
            ship: id,
            sub: 1,
            players: a.players,
            classes: u32::MAX,
            third: u32::MAX,
        };
        let attack = |ship| Order::Attack { ship, target: idf };
        assert_eq!(
            scene.outbox,
            [behave(id0), behave(id1), attack(id0), attack(id1)]
        );
        // Another order: notice 0x137, then at the pass the slot goes.
        scene.leave_zones(&[id0]);
        assert_eq!(scene.human_attacks[0].ships, [id1]);
        assert_eq!(scene.human_attacks[0].notices[0].code, 0x137);
        scene.run_human_attacks();
        let a = &scene.human_attacks[0];
        assert!(a.notices.is_empty());
        assert_eq!(a.slots.iter().map(|s| s.ship).collect::<Vec<_>>(), [id1]);
        assert_eq!(a.elapsed, 1);
    }

    /// A ship whose target is gone posts notice 0x13a with it (0x4c09c0,
    /// 0x4a1800); the block (0x4d4e50) drops the target's entry and sends
    /// the ship at the area's target (0x4d6640, 0x4d0140).
    #[test]
    fn target_gone_notice_retargets() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let foe_owner = (human.0 ^ 1, human.1);
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(3)
            .collect();
        let t = TILE_SIZE as f32;
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let c0 = (3..h - 3)
            .flat_map(|y| (3..w - 3).map(move |x| (x, y)))
            .find(|&c| (-3..=3).all(|dy| (-3..=3).all(|dx| water(&scene, (c.0 + dx, c.1 + dy)))))
            .unwrap();
        let cells = [c0, (c0.0 + 2, c0.1), (c0.0 + 3, c0.1)];
        let owners = [human, foe_owner, foe_owner];
        for k in 0..3 {
            let o = &mut scene.objects[ships[k]];
            o.owner = Some(owners[k]);
            o.x = cells[k].0 as f32 * t + t / 2.0;
            o.y = cells[k].1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.caps = crate::objects::caps_mask(2);
            s.argo = false;
            s.move_state = 0;
        }
        let ids: Vec<u32> = ships.iter().map(|&i| scene.objects[i].id).collect();
        let a = (cells[1].0 as f32 * t + 1.0, cells[1].1 as f32 * t + 1.0);
        let b = (cells[2].0 as f32 * t + 1.0, cells[2].1 as f32 * t + 1.0);
        scene.order_attack_zone(&ids[..1], a, b).unwrap();
        let gone = scene.human_attacks[0].slots[0].target;
        let other = if gone == ids[1] { ids[2] } else { ids[1] };
        let k = scene.object_index(ids[0]).unwrap();
        scene.objects[k].ship.as_mut().unwrap().task = Some(crate::objects::Task::Attack {
            target: gone,
            jam: None,
        });
        // The attack's records (0x4ab3f0 kind 3) at it, then it is gone:
        // the fire pass's attack (0x4c09c0) finds nothing.
        let armed = scene.objects[k].armed_positions();
        scene.objects[k]
            .ship
            .as_mut()
            .unwrap()
            .take_attack(gone, -1, armed);
        scene.objects.retain(|o| o.id != gone);
        scene.ticks = (scene.ticks..).find(|t| (t ^ ids[0]) & 3 == 0).unwrap();
        scene.run_fire_pass(&data, &mut caches);
        let k = scene.object_index(ids[0]).unwrap();
        assert!(scene.objects[k].ship.as_ref().unwrap().task.is_none());
        let n = scene.human_attacks[0].notices.last().copied().unwrap();
        assert_eq!((n.code, n.ship, n.class), (0x13a, ids[0], gone));
        scene.outbox.clear();
        let listed = |scene: &Scene| {
            scene.human_attacks[0]
                .targets
                .iter()
                .filter(|t| t.id == gone)
                .count()
        };
        // The target gone, event 8 0x180's fire part: on guard, so the
        // fire task with fire state 2, which 0x4d60c0 sends on.
        let fired = |scene: &Scene| {
            let s = scene.objects[k].ship.as_ref().unwrap();
            (s.firing, s.behaviour.fire_state)
        };
        assert_eq!(fired(&scene), (true, 2));
        let before = listed(&scene);
        let heard = {
            let mut a = scene.human_attacks[0].clone();
            let heard = scene.ha_notices(&mut a);
            scene.human_attacks[0] = a;
            heard
        };
        assert_eq!(heard, 1);
        // Its first entry dropped (0x4d4ab0 with 1).
        assert_eq!(listed(&scene), before - 1);
        assert_eq!(
            scene.outbox,
            [Order::Attack {
                ship: ids[0],
                target: other
            }]
        );
        // On a fire task in state 3, or 1 with a position in 3, not sent
        // (the attack's records left every armed position in 3).
        let send = |scene: &mut Scene, state: u8, aim: u8| {
            let s = scene.objects[k].ship.as_mut().unwrap();
            s.behaviour.fire_state = state;
            for a in &mut s.behaviour.aims {
                a.state = aim;
            }
            let a = scene.human_attacks[0].clone();
            scene.ha_retarget(&a, k)
        };
        assert_eq!(send(&mut scene, 3, 0), 0);
        assert_eq!(send(&mut scene, 1, 3), 0);
        assert_eq!(send(&mut scene, 1, 1), 1);
    }
}
