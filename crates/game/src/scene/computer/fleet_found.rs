//! The navy's founding task (fleet order 0x8c, block `{0x8c, 0x47}`, made
//! by 0x4cbcf0, its notices 0x4cba10, its tick 0x4cbe70, its status
//! 0x4cbfc0) for the computers' flotillas (0x4834c0) and the brains'
//! fleets (0x47b000 -> 0x479af0), both through `FoundGroup`.

use serde::{Deserialize, Serialize};

use super::fleet_guard::Flotilla;
use super::{Cells, DONE};
use crate::scene::{Order, Scene};

/// Fleet order 0x8c's block (`+0x25`, `{0x8c, 0x47}`, 0x4cbcf0; made
/// zeroed by 0x4cb000): the order's payload `+0xc..+0x24` (the computers'
/// 0x4834c0: the flotilla's flags `+0x154`, unread, its class `+0x70`,
/// its cell `+0x178..+0x184`, -1; the brains' 0x479af0: the call's flags,
/// the class, the area, a member's first word) and the sites it keeps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(in crate::scene) struct FoundBlock {
    /// `+0x10` the class to found.
    pub class: u32,
    /// `+0x14..+0x20` the area to found in.
    pub area: Cells,
    /// `+0x24` (payload 6): command 0x20's third word.
    pub third: i32,
    /// `+0x28`: the last site founded (0x130), read by the status 0x4cbfc0.
    pub done: bool,
    /// `+0x29` the sites (0x4cb4b0, 0x19 bytes: the area, `+0x24`, the
    /// class byte).
    pub sites: Vec<FoundSite>,
}

/// A site in the founding block's list (`+0x29`, 0x4cb4b0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub(in crate::scene) struct FoundSite {
    pub area: Cells,
    pub third: i32,
    pub class: u8,
}

/// What the founding block reads of the group holding it: a computer's
/// flotilla (`Flotilla`) or a brain's fleet (`brain::Fleet`).
pub(in crate::scene) trait FoundGroup {
    /// Group `+0x31`: its ships, by id.
    fn ships(&self) -> &[u32];
    /// Group `+0x25`: the block.
    fn block(&self) -> &Option<FoundBlock>;
    fn block_mut(&mut self) -> &mut Option<FoundBlock>;
    /// Group `+0x19`, `+0x1d`, `+0x21`: the behaviour 0x4c4350 gave it
    /// (zeroes when never given).
    fn behaviour(&self) -> (u32, u32, u32);
    /// Group `+8`.
    fn elapsed(&mut self) -> &mut u32;
    /// Whether the group's notices `+0x2d` hold any.
    fn has_notices(&self) -> bool;
    /// Group `+0x2d`: the block's notices `(ship, code, record)`, oldest
    /// first, taken.
    fn take_notices(&mut self) -> Vec<(u32, u32, u32)>;
}

impl FoundGroup for Flotilla<'_> {
    fn ships(&self) -> &[u32] {
        &self.ai.flotillas[self.f].ships
    }
    fn block(&self) -> &Option<FoundBlock> {
        &self.ai.flotillas[self.f].founding
    }
    fn block_mut(&mut self) -> &mut Option<FoundBlock> {
        &mut self.ai.flotillas[self.f].founding
    }
    fn behaviour(&self) -> (u32, u32, u32) {
        self.ai.flotillas[self.f].behaviour.unwrap_or((0, 0, 0))
    }
    fn elapsed(&mut self) -> &mut u32 {
        &mut self.ai.flotillas[self.f].elapsed
    }
    fn has_notices(&self) -> bool {
        !self.ai.flotillas[self.f].joined.is_empty()
    }
    /// The notices posted to the founding block (the port's `(ship,
    /// block, code, rec)`, those of an older block dropped).
    fn take_notices(&mut self) -> Vec<(u32, u32, u32)> {
        std::mem::take(&mut self.ai.flotillas[self.f].joined)
            .into_iter()
            .filter(|&(_, at, _, _)| at == 0x8c)
            .map(|(id, _, code, rec)| (id, code, rec))
            .collect()
    }
}

/// The founding block's status (0x4cbfc0, by 0x4a0e20 for `{0x8c,
/// 0x47}`): `+0x28` set, done (0x1f5) with result 10; else with a ship
/// (0x50e600 200) in the group running (500); else done with result 1.
pub(in crate::scene) fn found_status(
    scene: &Scene,
    ships: &[u32],
    b: Option<&FoundBlock>,
) -> (u32, u32) {
    if b.is_some_and(|b| b.done) {
        return (DONE, 10);
    }
    let any = ships.iter().any(|&id| {
        scene
            .object_index(id)
            .is_some_and(|i| scene.objects[i].ship.is_some())
    });
    if any {
        (500, 0)
    } else {
        (DONE, 1)
    }
}

impl Scene {
    /// 0x4cbcf0's block, the old one of another kind ended by the caller
    /// (`new_block`, `brain_block`): made zeroed (0x4cb000) or rewritten
    /// (its sites, `+0x28` and route kept), its class, area and `+0x24`
    /// the payload's. The area becomes a site (0x4cb110 with no ship,
    /// 0x4cb4b0): kept as it is when one of the same rect, class and
    /// `+0x24` is listed, else listed last (one of the same rect but
    /// another class or `+0x24` taken off first). Then every ship of the
    /// group is given its site (0x4cb1a0, `found_send`). (Its marks,
    /// 0x4cc1e0 / 0x4cc170, are the viewer's player's alone, and need
    /// flags 4.)
    pub(in crate::scene) fn found_make(
        &mut self,
        g: &mut dyn FoundGroup,
        class: u32,
        area: Cells,
        third: i32,
    ) {
        let b = g.block_mut().get_or_insert_with(Default::default);
        b.class = class;
        b.area = area;
        b.third = third;
        let site = FoundSite {
            area,
            third,
            class: class as u8,
        };
        match b.sites.iter().position(|e| e.area == area) {
            Some(p) if b.sites[p] == site => {}
            Some(p) => {
                b.sites.remove(p);
                b.sites.push(site);
            }
            None => b.sites.push(site),
        }
        for id in g.ships().to_vec() {
            self.found_send(g, id);
        }
    }

    /// 0x4cb1a0 for ship `id` of the group, its block without a route
    /// (`+0x61`, set only by the human's Shift founding, 0x49dc90 with
    /// flags 4): its site (`found_site`) and, with one, command 0x20
    /// `{x, y, +0x24, class, 1, 100}` (0x4cb0b0 -> 0x520cc0), delivered
    /// at the next tick. Whether it was sent.
    pub(in crate::scene) fn found_send(&mut self, g: &dyn FoundGroup, id: u32) -> bool {
        let Some(b) = g.block().as_ref() else {
            return false;
        };
        let Some((cx, cy)) = self.found_site(b, id) else {
            return false;
        };
        let class = b.class & 0xff;
        self.post(Order::Found {
            ship: id,
            cx,
            cy,
            class,
        });
        true
    }

    /// 0x4cb850: the site of ship `id` (an object of kind 200, a ship) in
    /// the founding block `b`. A 1 x 1 area: its cell, for class 0x32 or
    /// 0x34-0x36, where the ship may found it (0x4e4420, `check_ground`);
    /// else none. (Not ported: a wider area, which neither the computers'
    /// order nor the brains' (miss005's raiders, 1 x 1) gives: its cells
    /// the ship may found on with a berth (0x4b2710), sorted by the
    /// wavefront 0x4a1e90 at their berths (0x4cb790), the first taken;
    /// with no such cell the original reads a word it never wrote.)
    fn found_site(&self, b: &FoundBlock, id: u32) -> Option<(i32, i32)> {
        let i = self.object_index(id)?;
        self.objects[i].ship.as_ref()?;
        let (x, y, w, h) = b.area;
        if (w, h) != (1, 1) || !matches!(b.class, 0x32 | 0x34..=0x36) {
            return None;
        }
        self.check_ground(b.class, x, y).ok()?;
        Some((x, y))
    }

    /// The founding block's notices (0x4cba10), from the last: 300 of a
    /// ship of the group (0x4cc590), its site (`found_send`), then `#`
    /// sub 1 with the group's three (0x4c42f0), counted. 0x130, a ship of
    /// the group whose landing party went ashore (0x4a15a0, from 0x4b3637:
    /// `{tick, +0x60, id, x, y, class}`, the site `+0x73c` and class
    /// `+0x73b`): every other ship of the group stopped (command 10), and
    /// the first site holding the cell with that class taken off
    /// (0x4cb370); then with none left `+0x28` set, the founding done,
    /// else the block's class, area and `+0x24` the first site's
    /// (0x4cb450). (0x136 needs a route `+0x61`.) The count of 300s.
    fn found_notices(&mut self, g: &mut dyn FoundGroup) -> i32 {
        let b = g.behaviour();
        let mut n = 0;
        let mut heard = g.take_notices();
        while let Some((id, code, rec)) = heard.pop() {
            if !g.ships().contains(&id) {
                continue;
            }
            match code {
                300 => {
                    self.found_send(g, id);
                    self.send_behaviour(id, b);
                    n += 1;
                }
                0x130 => {
                    for other in g.ships().to_vec() {
                        if other != id && self.object_index(other).is_some() {
                            self.post(Order::Stop { id: other });
                        }
                    }
                    let w = self.nav.width as i32;
                    let (cell, class) = ((rec & 0xff_ffff) as i32, (rec >> 24) as u8);
                    let (x, y) = (cell % w, cell / w);
                    let Some(fb) = g.block_mut().as_mut() else {
                        continue;
                    };
                    let Some(p) = fb.sites.iter().position(|e| {
                        let (sx, sy, sw, sh) = e.area;
                        (sx..sx + sw).contains(&x) && (sy..sy + sh).contains(&y) && e.class == class
                    }) else {
                        continue;
                    };
                    fb.sites.remove(p);
                    match fb.sites.first().copied() {
                        Some(e) => {
                            fb.class = e.class as u32;
                            fb.area = e.area;
                            fb.third = e.third;
                        }
                        None => fb.done = true,
                    }
                }
                _ => {}
            }
        }
        n
    }

    /// The founding block's tick (0x4cbe70, the navy's 0x4c5047): the
    /// notices (`found_notices`); when no 300 was heard (or none were
    /// posted), group `+8` past 100 and the block there, `+8` back to 0
    /// and each ship of the group: a blow marked (`+0x691`) forgotten
    /// (0x4cf760, command 0x28 sub 4; its report 0x4a21c0 the port's as
    /// it lands); lying still (`+0x58c` 0), not landing (`+0x25d & 0x20`)
    /// and the block not done, its site again (`found_send`). Then `+8`
    /// grows by the tick.
    pub(in crate::scene) fn found_tick(&mut self, g: &mut dyn FoundGroup) {
        let heard = if g.has_notices() {
            self.found_notices(g)
        } else {
            0
        };
        if heard == 0 && *g.elapsed() > 100 && g.block().is_some() {
            *g.elapsed() = 0;
            for id in g.ships().to_vec() {
                let Some(i) = self.object_index(id) else {
                    continue;
                };
                let Some(s) = self.objects[i].ship.as_ref() else {
                    continue;
                };
                let (blow, still, landing) =
                    (s.blow.is_some(), s.move_state == 0, s.landing.is_some());
                if blow {
                    self.post(Order::ClearBlow { ship: id });
                }
                let done = g.block().as_ref().is_some_and(|b| b.done);
                if still && !landing && !done {
                    self.found_send(g, id);
                }
            }
        }
        *g.elapsed() += 1;
    }
}
