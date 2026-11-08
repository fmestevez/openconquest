//! The navy's attack zone (fleet order 0x70, block `{0x70, 0x21}`, made
//! or rewritten by 0x4d6a90, its pass 0x4d6cd0 with its notices 0x4d6960,
//! its status 0x4d70d0) for the computers' flotillas (0x483650) and the
//! brains' fleets (0x47a6a0 -> 0x479a40), both through `AttackGroup`.

use serde::{Deserialize, Serialize};

use super::fleet_guard::Flotilla;
use super::{Cells, FleetOrder, DONE};
use crate::scene::{Order, Scene};

/// Fleet order 0x70's block (`+0x25`, `{0x70, 0x21}`) as a brain's fleet
/// keeps it (a computer's flotilla keeps its area in its order and its
/// target and freed notices in its own fields).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(in crate::scene) struct AttackBlock {
    /// `+0xc..+0x18`: the area (payload 0-3).
    pub area: Cells,
    /// `+0x1c`: the target 0x4d6640 found, 0 none.
    pub target: u32,
    /// `+0x2c`: payload 7, the flags (the high half the pass's draw
    /// level, the low half 0x4bad90 / 0x4baae0's mode).
    pub flags: u32,
    /// The group's notices `+0x2d` freed by the pass (0x4d6520) while
    /// the block stays: no notice is posted (0x4a1340) until a block of
    /// another kind makes them again (0x4a1150).
    pub freed: bool,
}

/// What the attack block reads of the group holding it: a computer's
/// flotilla (`Flotilla`) or a brain's fleet (`brain::Fleet`).
pub(in crate::scene) trait AttackGroup {
    /// Group `+0x31`: its ships, by id.
    fn ships(&self) -> &[u32];
    /// Block `+0xc..+0x18`: the area; none without a block.
    fn area(&self) -> Option<Cells>;
    /// Block `+0x1c`: the target.
    fn target(&mut self) -> &mut u32;
    /// Block `+0x2c`: the flags.
    fn flags(&self) -> u32;
    /// Group `+0x14` set and `+0x19`, `+0x1d`, `+0x21`: the behaviour
    /// 0x4c4350 gave it; none until the first.
    fn behaviour(&self) -> Option<(u32, u32, u32)>;
    /// Group `+8`.
    fn elapsed(&mut self) -> &mut u32;
    /// The group's player (0x4a09d0's p1, member `+4`).
    fn owner(&self, scene: &Scene) -> (u32, u32);
    /// The pass's period (0x4d6cd0): 0x5a, but for a player of side 3 or
    /// 4 out of the campaign (game `+0xd80 != 1`) 0x78, 0x4b, 0x3c by
    /// its level `+0x1a31`.
    fn period(&self) -> u32;
    /// Whether the group's notices `+0x2d` were freed (0x4d6520).
    fn freed(&self) -> bool;
    /// Group `+0x2d`: the block's notices `(ship, code, record)`, oldest
    /// first, taken.
    fn take_notices(&mut self) -> Vec<(u32, u32, u32)>;
    /// 0x4d6520: the group's notices freed.
    fn free_notices(&mut self);
}

impl AttackGroup for Flotilla<'_> {
    fn ships(&self) -> &[u32] {
        &self.ai.flotillas[self.f].ships
    }
    fn area(&self) -> Option<Cells> {
        match self.ai.flotillas[self.f].order {
            FleetOrder::Attack(area) => Some(area),
            _ => None,
        }
    }
    fn target(&mut self) -> &mut u32 {
        &mut self.ai.flotillas[self.f].target
    }
    /// The flotilla's `+0x154`, which each attack's caller (tasks 8, 9,
    /// 10 state 0xc: 0x47ed7b, 0x47e5e3, 0x47e992) sets to `(difficulty
    /// == 0) << 16` just before.
    fn flags(&self) -> u32 {
        u32::from(self.ai.difficulty == 0) << 16
    }
    fn behaviour(&self) -> Option<(u32, u32, u32)> {
        self.ai.flotillas[self.f].behaviour
    }
    fn elapsed(&mut self) -> &mut u32 {
        &mut self.ai.flotillas[self.f].elapsed
    }
    fn owner(&self, _: &Scene) -> (u32, u32) {
        self.ai.owner
    }
    /// The skirmish's computers (side 3 or 4, not the campaign).
    fn period(&self) -> u32 {
        [0x78, 0x4b, 0x3c][self.ai.difficulty.min(2)]
    }
    fn freed(&self) -> bool {
        self.ai.flotillas[self.f].no_notices
    }
    /// The notices posted to the attack block (the port's `(ship, block,
    /// code, rec)`, those of an older block dropped).
    fn take_notices(&mut self) -> Vec<(u32, u32, u32)> {
        std::mem::take(&mut self.ai.flotillas[self.f].joined)
            .into_iter()
            .filter(|&(_, at, _, _)| at == 0x70)
            .map(|(id, _, code, rec)| (id, code, rec))
            .collect()
    }
    fn free_notices(&mut self) {
        self.ai.flotillas[self.f].no_notices = true;
        self.ai.flotillas[self.f].joined.clear();
    }
}

/// The attack block's status (0x4d70d0, by 0x4a0d80 for `{0x70, 0x21}`):
/// done (0x1f5) with no ships, result 1; else each ship of the list that
/// exists counted armed (a ship, 0x50e600 200, of type neither 0 nor 1,
/// for which 0x4bc270(7) holds: types 8-11 always, the others with a
/// weapon in a position `+0x289..+0x291`) or not; then the area's target
/// (0x4d6640): armed ships and a target, running (500); else done, result
/// 10 with armed ships, 1 without. (`+1`, the ticks since the block's
/// `+8`, and the counts are not read by the callers.)
pub(in crate::scene) fn attack_status(
    scene: &Scene,
    ships: &[u32],
    owner: (u32, u32),
    behaviour: Option<(u32, u32, u32)>,
    area: Option<Cells>,
) -> (u32, u32) {
    if ships.is_empty() {
        return (DONE, 1);
    }
    let armed = ships
        .iter()
        .filter_map(|&id| scene.object_index(id))
        .any(|i| {
            scene.objects[i].ship.as_ref().is_some_and(|s| {
                !matches!(s.kind, 0 | 1)
                    && (matches!(s.kind, 8..=11)
                        || scene.objects[i]
                            .combat
                            .as_ref()
                            .is_some_and(|c| !c.weapons.is_empty()))
            })
        });
    let (players, classes, _) = behaviour.unwrap_or((0, 0, 0));
    let target = area.and_then(|a| scene.area_foe(owner, players, classes, a));
    match (armed, target) {
        (true, Some(_)) => (500, 0),
        (true, None) => (DONE, 10),
        (false, _) => (DONE, 1),
    }
}

impl Scene {
    /// The area's target for group `g` (0x4d6640 with the group's
    /// players `+0x19` and classes `+0x1d`, `area_foe`), 0 none.
    fn block_target(&self, g: &dyn AttackGroup, area: Cells) -> u32 {
        let (players, classes, _) = g.behaviour().unwrap_or((0, 0, 0));
        self.area_foe(g.owner(self), players, classes, area)
            .map_or(0, |k| self.objects[k].id)
    }

    /// 0x4d6a90's end, its block made or rewritten by the caller: the
    /// target found in the area (0x4d6640) kept in the block (`+0x1c`);
    /// with one, each ship of the group with no strength (0x50ea70 0)
    /// sent to the target's cell (0x51f1f0, its vt+0x28; 0x4cf070), the
    /// others to attack it (0x4d0140: command 0x16). None: nothing.
    pub(in crate::scene) fn attack_make(&mut self, g: &mut dyn AttackGroup) {
        let Some(area) = g.area() else {
            return;
        };
        let target = self.block_target(g, area);
        *g.target() = target;
        if target == 0 {
            return;
        }
        let mut cell = None;
        for id in g.ships().to_vec() {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            self.attack_one(i, target, &mut cell);
        }
    }

    /// The attack block's pass (0x4d6cd0, from the navy's tick 0x4c4f40,
    /// each game tick). With notices, they are read newest first
    /// (0x4d6960): a ship that joined (300) and is in the group is sent
    /// at the target (0x4d0140; not with none: command 0x16 with target 0,
    /// whose record 0x4ab3f0 keeps, 0x4ac050 finding nothing, after the
    /// pending group's turn in 0x4abe50: not traced) and the group's
    /// three (0x4c42f0); a ship whose target is gone or that took a prize
    /// (0x13a, from 0x4c09c0 and 0x4b6f20) and is in the group: the
    /// target looked for again (0x4d6640) when the notice names the
    /// block's (`+0x1c`), then the ship sent at it if there is one, heard
    /// either way; every other code is taken off unread. When none was
    /// read, group `+8` is over the period (`AttackGroup::period`) and
    /// the group has ships, the target is looked for again (0x4d6640).
    /// None: the notices are freed (0x4d6520), the block left as it is,
    /// so no join is heard again and the area looked at again each tick.
    /// Else `+8` goes back to 0 and each ship of the group: sent the
    /// group's three when its `+0x689 / +0x68d` are not the group's
    /// (`+0x14` set); with a blow marked, reported (0x4a21c0) and made to
    /// forget it (0x4cf760, `report_blow`); then a draw of the navy's generator
    /// `% 100` and, the block's flags `+0x2c >> 16` 1, above 60 nothing
    /// more (2: above 30); a phantom (`+0x253`) nothing more; one boarding
    /// or ramming (`+0x25d == 0x10`, `closing_in`) nothing more; one that
    /// takes a contact (`+0x261 & 0x1000`, `takes_contact`) looks about it
    /// (`close_pick`: 0x4bad90 for kinds 10 and 11, else 0x4baae0, both
    /// with reach 2 and the flags' low half, 0, as the mode) and with a
    /// find whose boarding odds are 101 or more is sent to board it
    /// (0x4d0190, command 0x17), else with ramming odds 101 or more to ram
    /// it (0x4d01d0, command 0x18); else, and without the contact, sent at
    /// the target as at the order (`attack_one`). Then `+8` grows by the
    /// tick.
    pub(in crate::scene) fn attack_tick(&mut self, g: &mut dyn AttackGroup) {
        let behaviour = g.behaviour();
        let mut heard = 0;
        if !g.freed() {
            for (id, code, rec) in g.take_notices().into_iter().rev() {
                if !g.ships().contains(&id) || self.object_index(id).is_none() {
                    continue;
                }
                match code {
                    300 => {
                        let target = *g.target();
                        if target != 0 {
                            self.post(Order::Attack { ship: id, target });
                        }
                        heard += 1;
                        self.send_behaviour(id, behaviour.unwrap_or((0, 0, 0)));
                    }
                    0x13a => {
                        if rec == *g.target() {
                            if let Some(area) = g.area() {
                                *g.target() = self.block_target(g, area);
                            }
                        }
                        let target = *g.target();
                        if target != 0 {
                            self.post(Order::Attack { ship: id, target });
                        }
                        heard += 1;
                    }
                    _ => {}
                }
            }
        }
        let period = g.period();
        if heard == 0 && period < *g.elapsed() && !g.ships().is_empty() {
            let Some(area) = g.area() else {
                return;
            };
            let target = self.block_target(g, area);
            *g.target() = target;
            if target == 0 {
                g.free_notices();
            } else {
                *g.elapsed() = 0;
                let level = g.flags() >> 16;
                let mut cell = None;
                for id in g.ships().to_vec() {
                    let Some(i) = self.object_index(id) else {
                        continue;
                    };
                    let Some(s) = self.objects[i].ship.as_ref() else {
                        continue;
                    };
                    if let Some(b @ (players, classes, _)) = behaviour {
                        if s.behaviour.players != players || s.behaviour.classes != classes {
                            self.send_behaviour(id, b);
                        }
                    }
                    self.report_blow(id);
                    let roll = self.navy_draw() % 100;
                    if (level == 1 && roll > 60) || (level == 2 && roll > 30) {
                        continue;
                    }
                    if self.objects[i]
                        .combat
                        .as_ref()
                        .is_some_and(|c| c.phantom > 0.0)
                    {
                        continue;
                    }
                    if self.closing_in(i) {
                        continue;
                    }
                    if self.takes_contact(i) {
                        let (pick, board, ram) = self.close_pick(i);
                        if pick != 0 && (board >= 0x65 || ram >= 0x65) {
                            self.post(Order::Close {
                                ship: id,
                                target: pick,
                                board: board >= 0x65,
                            });
                            continue;
                        }
                    }
                    self.attack_one(i, target, &mut cell);
                }
            }
        }
        *g.elapsed() += 1;
    }
}
