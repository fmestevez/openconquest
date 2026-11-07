//! The navy's founding task (fleet order 0x8c, block `{0x8c, 0x47}`, made
//! by 0x4cbcf0, its notices 0x4cba10, its tick 0x4cbe70, its status
//! 0x4cbfc0) for the computers' flotillas (0x4834c0) and the brains'
//! fleets (0x47b000 -> 0x479af0), both through `FoundGroup`.

use serde::{Deserialize, Serialize};

use super::fleet_guard::Flotilla;
use super::{Cells, DONE};
use crate::scene::gather::{HumanBehaviour, Notice, Route};
use crate::scene::{Order, Scene};

/// Fleet order 0x8c's block (`+0x25`, `{0x8c, 0x47}`, 0x4cbcf0; made
/// zeroed by 0x4cb000): the order's payload `+0xc..+0x24` (the computers'
/// 0x4834c0: the flotilla's flags `+0x154`, unread, its class `+0x70`,
/// its cell `+0x178..+0x184`, -1; the brains' 0x479af0: the call's flags,
/// the class, the area, a member's first word) and the sites it keeps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FoundBlock {
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
    /// `+0x2d` the ships waiting on the route (0x4cb5f0: 0x1b-byte
    /// records 0x4cb110 `{ship id, site x, y, 0, 0, +0x24, class, 1,
    /// 100}`, one per ship, its earlier one replaced, appended). Only the
    /// ship id is read (0x4cb670, 0x4cb6f0), so the port keeps the ids.
    #[serde(default)]
    pub waiting: Vec<u32>,
}

/// A site in the founding block's list (`+0x29`, 0x4cb4b0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct FoundSite {
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
    /// Block `+0x61`: the route (only a human group's, `HumanFound`).
    fn route(&self) -> Option<&Route> {
        None
    }
    fn route_mut(&mut self) -> Option<&mut Route> {
        None
    }
    /// Whether the group's player is the navy's viewing player (`navy
    /// +0x21`): the human's.
    fn viewer(&self) -> bool {
        false
    }
    /// A notice to the group's own list (0x4ce8a0's 0x136, 0x4a1700).
    fn push_notice(&mut self, _n: Notice) {}
}

/// A human group running the founding block: the human's fleet command
/// 0x20 (the found pointer's click, 0x5180e0 mode 0xe: payload `{x, y,
/// -1, +0xe1 the code, ...}`, 0xc458 -> 0x49e230 -> 0x49dc90). The
/// group's own fields the block reads, and the block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanFound {
    /// The group's serial number (shared with the other human groups).
    pub serial: u32,
    /// Group `+0x31`: its ships, by id.
    pub ships: Vec<u32>,
    /// Group `+0x2d`: the notices not yet read, oldest first.
    pub notices: Vec<Notice>,
    /// Group `+8`.
    pub elapsed: u32,
    /// Block `+0x61`: a route kept from the group's block of another kind
    /// under Shift (0x4cbcf0 with flags 4), or made by Shift move clicks
    /// on the group (0x49de70).
    #[serde(default)]
    pub route: Option<Route>,
    /// Group `+0x14..+0x24` (`HumanBehaviour`).
    #[serde(default)]
    pub behaviour: HumanBehaviour,
    pub block: Option<FoundBlock>,
}

impl FoundGroup for HumanFound {
    fn ships(&self) -> &[u32] {
        &self.ships
    }
    fn block(&self) -> &Option<FoundBlock> {
        &self.block
    }
    fn block_mut(&mut self) -> &mut Option<FoundBlock> {
        &mut self.block
    }
    fn behaviour(&self) -> (u32, u32, u32) {
        self.behaviour.unwrap_or((0, 0, 0))
    }
    fn elapsed(&mut self) -> &mut u32 {
        &mut self.elapsed
    }
    fn has_notices(&self) -> bool {
        !self.notices.is_empty()
    }
    fn take_notices(&mut self) -> Vec<(u32, u32, u32)> {
        std::mem::take(&mut self.notices)
            .into_iter()
            .map(|n| (n.ship, n.code, n.class))
            .collect()
    }
    fn route(&self) -> Option<&Route> {
        self.route.as_ref()
    }
    fn route_mut(&mut self) -> Option<&mut Route> {
        self.route.as_mut()
    }
    fn viewer(&self) -> bool {
        true
    }
    fn push_notice(&mut self, n: Notice) {
        self.notices.push(n);
    }
}

/// The class 0x49dc90 founds for the found pointer's code `+0xe1` (the
/// building page's command less 0x15, 0x4f34c0): 1 0x32, 3 0x35, 4 0x36,
/// any other 0x34 (the jump table at 0x49dd88).
pub(in crate::scene) fn found_class(code: u8) -> u32 {
    match code {
        1 => 0x32,
        3 => 0x35,
        4 => 0x36,
        _ => 0x34,
    }
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

/// 0x4cb4b0: `site` into the block's sites `+0x29`, kept as it is when
/// one of the same rect, class and `+0x24` is listed, else listed last
/// (one of the same rect but another class or `+0x24` taken off first).
fn found_list(b: &mut FoundBlock, site: FoundSite) {
    match b.sites.iter().position(|e| e.area == site.area) {
        Some(p) if b.sites[p] == site => {}
        Some(p) => {
            b.sites.remove(p);
            b.sites.push(site);
        }
        None => b.sites.push(site),
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
        found_list(
            b,
            FoundSite {
                area,
                third,
                class: class as u8,
            },
        );
        for id in g.ships().to_vec() {
            self.found_send(g, id);
        }
    }

    /// The human's fleet command 0x20 at cell `(x, y)` for `ships`, the
    /// found pointer's code `code` (`found_class`), with the click's key
    /// flags `keys` (0x49dc90, payload `{x, y, -1, code}`). With Shift
    /// (`keys & 4`) and the selection's group running this block, the
    /// cell is only listed as a site (0x4cb4b0: `{x, y, 1, 1, -1}` with
    /// the code itself as its class byte, 0x49dce8, so that 0x4cb850
    /// finds no site in it when it comes up, nor 0x4cb370 the landing's
    /// class). Else 0x4cbcf0 with `{0, class, (x, y, 1, 1), -1}` on the
    /// selection's group: its block rewritten in place (its sites, wait
    /// list and route kept); a block of another kind ended (0x4a1c60;
    /// with Shift its route `+0x61` kept, taken before), its serial,
    /// `+8` and behaviour kept; else a new group (0x49ca60, its ships
    /// leaving their old groups). Then `found_make`. (Not ported: the
    /// sites' marks, 0x4cc1e0 / 0x4cc170, the viewing player's.)
    pub fn order_human_found(
        &mut self,
        ships: &[u32],
        x: i32,
        y: i32,
        code: u8,
        keys: u8,
    ) -> Result<(), String> {
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
        let area = (x, y, 1, 1);
        let z = self.selection_found(&ships);
        if keys & 4 != 0 {
            if let Some(z) = z {
                if let Some(b) = self.human_founds[z].block.as_mut() {
                    let site = FoundSite {
                        area,
                        third: -1,
                        class: code,
                    };
                    found_list(b, site);
                }
                return Ok(());
            }
        }
        let z = match z {
            Some(z) => z,
            None => {
                let (serial, elapsed, route, behaviour) = match self.end_selection_block(&ships) {
                    Some(t) => t,
                    None => {
                        self.leave_zones(&ships);
                        self.zone_serial += 1;
                        (self.zone_serial, 0, None, None)
                    }
                };
                self.human_founds.push(HumanFound {
                    serial,
                    ships,
                    notices: Vec::new(),
                    elapsed,
                    route: route.filter(|_| keys & 4 != 0),
                    behaviour,
                    block: None,
                });
                self.human_founds.len() - 1
            }
        };
        let mut g = self.human_founds.remove(z);
        self.found_make(&mut g, found_class(code), area, -1);
        self.human_founds.insert(z, g);
        Ok(())
    }

    /// The human founding group whose ships are exactly `ships`.
    pub fn selection_found(&self, ships: &[u32]) -> Option<usize> {
        let mut sel: Vec<u32> = ships.to_vec();
        sel.sort_unstable();
        sel.dedup();
        self.human_founds.iter().position(|g| {
            let mut s = g.ships.clone();
            s.sort_unstable();
            !s.is_empty() && s == sel
        })
    }

    /// The human founding group holding ship `id`.
    pub fn ship_found(&self, id: u32) -> Option<usize> {
        self.human_founds.iter().position(|g| g.ships.contains(&id))
    }

    /// Ships leaving their founding groups post notice 0x137 (0x4a1770;
    /// 0x4cba10 drops it); a group left with none goes.
    pub(in crate::scene) fn leave_founds(&mut self, ids: &[u32]) {
        let tick = self.ticks;
        for g in &mut self.human_founds {
            for &s in &g.ships {
                if ids.contains(&s) {
                    g.notices.push(Notice {
                        code: 0x137,
                        tick,
                        ship: s,
                        class: 0,
                    });
                }
            }
            g.ships.retain(|s| !ids.contains(s));
        }
        self.human_founds.retain(|g| !g.ships.is_empty());
    }

    /// The navy's tick for the human's founding groups (0x4cbe70 each,
    /// from 0x4c4f40, `found_tick`). The block never ends itself: done,
    /// it lasts until the group is given another.
    #[cfg(test)]
    pub(in crate::scene) fn run_human_founds(&mut self) {
        self.drop_gone_founds();
        for z in 0..self.human_founds.len() {
            self.human_found_tick(z);
        }
    }

    /// The human's founding group `z`'s block tick (0x4cbe70).
    pub(in crate::scene) fn human_found_tick(&mut self, z: usize) {
        let mut g = self.human_founds.remove(z);
        self.found_tick(&mut g);
        self.human_founds.insert(z, g);
    }

    /// The sunk ships out of the human's founding groups.
    pub(in crate::scene) fn drop_gone_founds(&mut self) {
        let gone: Vec<u32> = self
            .human_founds
            .iter()
            .flat_map(|g| g.ships.iter().copied())
            .filter(|&id| {
                self.object_index(id)
                    .is_none_or(|i| self.objects[i].ship.is_none())
            })
            .collect();
        if !gone.is_empty() {
            self.leave_founds(&gone);
        }
    }

    /// 0x4cb1a0 for ship `id` of the group, its block without a route
    /// (`+0x61`, set only by the human's Shift founding, 0x49dc90 with
    /// flags 4): its site (`found_site`) and, with one, command 0x20
    /// `{x, y, +0x24, class, 1, 100}` (0x4cb0b0 -> 0x520cc0), delivered
    /// at the next tick. Whether it was sent.
    ///
    /// With a route (0x4cb1a0's other arm): a ship on it (0x4cea20 not
    /// -1) only waits (`found_wait`). A ship off it: for the viewing
    /// player its berth for the block's area cell (0x4b2710 by the ship's
    /// class `+0x73b`, `Task::Found`'s; none, nothing sent), then the
    /// waypoint nearest it (0x4ceab0 with that berth; the others' -1):
    /// one, the berth not nearer, a plain move there (0x4cf070), its
    /// progress `(-1, -1)` way 1 (0x4ce8a0) and the wait; the berth
    /// nearer, the command at once. (Not decoded: with no waypoint
    /// 0x4ceab0 leaves `closer` (ebp-0xc) unwritten and 0x4cb1a0 reads
    /// it; the port gives the command, as with no route.)
    pub(in crate::scene) fn found_send(&mut self, g: &mut dyn FoundGroup, id: u32) -> bool {
        let Some(b) = g.block().as_ref() else {
            return false;
        };
        if g.route().is_none() {
            return self.found_command(b, id);
        }
        let Some(i) = self.object_index(id) else {
            return false;
        };
        let w = self.nav.width as i32;
        let r = g.route().unwrap();
        let (k, _) = r.at(id, w);
        if k < 0 {
            let mut cell = -1;
            if g.viewer() {
                let class = match self.objects[i].ship.as_ref().and_then(|s| s.task) {
                    Some(crate::objects::Task::Found { class, .. }) => class,
                    _ => 0,
                };
                // 0x4b2710: -1 for a class but 0x32 and 0x34-0x36.
                if !matches!(class, 0x32 | 0x34..=0x36) {
                    return false;
                }
                match self.found_berth(i, class, (b.area.0, b.area.1)) {
                    Some(((bx, by), _)) => cell = by * w + bx,
                    None => return false,
                }
            }
            let (c, closer) = self.route_best(r, i, cell);
            if closer != Some(false) {
                let b = g.block().clone().unwrap();
                return self.found_command(&b, id);
            }
            if c >= 0 {
                self.zone_move(id, c % w, c / w);
                self.found_progress(g, i, -1, -1, false, 1);
            }
        }
        self.found_wait(g, id);
        true
    }

    /// 0x4cb850 and, in `[0, cells)`, command 0x20 `{x, y, +0x24, class,
    /// 1, 100}` (0x4cb0b0 -> 0x520cc0), delivered at the next tick.
    fn found_command(&mut self, b: &FoundBlock, id: u32) -> bool {
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

    /// 0x4cb27b: the ship's record (0x4cb110 with its site) into the
    /// block's `+0x2d` (0x4cb5f0: its earlier one taken off, appended).
    fn found_wait(&mut self, g: &mut dyn FoundGroup, id: u32) {
        if let Some(b) = g.block_mut().as_mut() {
            b.waiting.retain(|&s| s != id);
            b.waiting.push(id);
        }
    }

    /// 0x4ce8a0 on the group's route for ship `i` (`Route::set_progress`),
    /// the notice 0x136 it asks for posted to the group (0x4a1700).
    fn found_progress(
        &self,
        g: &mut dyn FoundGroup,
        i: usize,
        x: i32,
        y: i32,
        remove: bool,
        mark: i8,
    ) {
        let id = self.objects[i].id;
        let at = self.ship_cell(i);
        let Some(r) = g.route_mut() else {
            return;
        };
        if r.set_progress(id, at, x, y, remove, mark) {
            let w = self.nav.width as i32;
            g.push_notice(Notice {
                code: 0x136,
                tick: self.ticks,
                ship: id,
                class: (y * w + x) as u32,
            });
        }
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
    /// (0x4cb450). 0x136 (`rec` the cell; only with a route `+0x61`):
    /// the waypoint after the cell the ship's way (0x4cea20, 0x4ce7c0);
    /// inside the map above cell 0 a plain move there (0x4cf070) and its
    /// progress set (0x4ce8a0, the end flag as `remove`); else, past the
    /// end, off the route (0x4ce8a0 `(-1, -1)`, removing) and, not
    /// waiting (0x4cb670), the command (0x4cb850 in `[0, cells)`,
    /// 0x4cb0b0), counted; waiting, the command (0x4cb6f0, not counted)
    /// and its wait taken off. (Not decoded: 0x4cb6f0 sends 0x4cb850's -1
    /// as the cell (-1, 0) unchecked; the port sends no command then.)
    /// The count heard.
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
                0x136 if g.route().is_some() => {
                    let Some(i) = self.object_index(id) else {
                        continue;
                    };
                    let w = self.nav.width as i32;
                    let cells = w * self.nav.height as i32;
                    let r = g.route().unwrap();
                    let (_, dir) = r.at(id, w);
                    let c = rec as i32;
                    let (nx, end) = r.next(c % w, c / w, dir, w);
                    if 0 < nx && nx < cells {
                        self.zone_move(id, nx % w, nx / w);
                        self.found_progress(g, i, nx % w, nx / w, end, 0);
                    } else if end {
                        self.found_progress(g, i, -1, -1, true, 0);
                        let Some(b) = g.block().clone() else {
                            continue;
                        };
                        match b.waiting.iter().position(|&s| s == id) {
                            None => {
                                if self.found_command(&b, id) {
                                    n += 1;
                                }
                            }
                            Some(k) => {
                                self.found_command(&b, id);
                                if let Some(fb) = g.block_mut().as_mut() {
                                    fb.waiting.remove(k);
                                }
                            }
                        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::GameData;
    use crate::objects::Caches;

    /// miss000 with its one own boat left, the brain and computers off.
    fn boat_scene() -> Option<(Scene, u32)> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        scene.computers.on = false;
        let boat = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
            .unwrap()
            .id;
        scene.objects.retain(|o| o.ship.is_none() || o.id == boat);
        Some((scene, boat))
    }

    fn found_sent(s: &Scene, boat: u32) -> Vec<(i32, i32, u32)> {
        s.outbox
            .iter()
            .filter_map(|o| match *o {
                Order::Found {
                    ship,
                    cx,
                    cy,
                    class,
                } if ship == boat => Some((cx, cy, class)),
                _ => None,
            })
            .collect()
    }

    /// The fort sites of miss000, from the map's last cell back.
    fn fort_sites(scene: &Scene) -> Vec<(i32, i32)> {
        let (w, h) = scene.map_cells();
        (0..h)
            .rev()
            .flat_map(|y| (0..w).rev().map(move |x| (x, y)))
            .filter(|&(x, y)| scene.check_ground(0x34, x, y).is_ok())
            .take(2)
            .collect()
    }

    /// The found pointer's click (0x49dc90): the selection's group gets
    /// the block and the boat command 0x20 with the code's class; a Shift
    /// click lists the cell with the code itself as its class byte
    /// (0x49dce8), which, come up after the landing, gives no site.
    #[test]
    fn the_human_founds_through_the_block() {
        let Some((mut scene, boat)) = boat_scene() else {
            return;
        };
        let w = scene.map_cells().0;
        let s = fort_sites(&scene);
        let (a, b) = (s[0], s[1]);
        scene.order_human_found(&[boat], a.0, a.1, 2, 0).unwrap();
        assert_eq!(scene.human_founds.len(), 1);
        assert_eq!(found_sent(&scene, boat), vec![(a.0, a.1, 0x34)]);
        scene.outbox.clear();
        scene.order_human_found(&[boat], b.0, b.1, 2, 4).unwrap();
        assert!(found_sent(&scene, boat).is_empty());
        let fb = scene.human_founds[0].block.as_ref().unwrap();
        assert_eq!(fb.class, 0x34);
        let site = |c: (i32, i32), class| FoundSite {
            area: (c.0, c.1, 1, 1),
            third: -1,
            class,
        };
        assert_eq!(fb.sites, vec![site(a, 0x34), site(b, 2)]);
        // The landing at a: the Shift site comes up, class 2.
        let cell = (a.1 * w + a.0) as u32 | 0x34 << 24;
        scene.post_notice(boat, 0x130, cell);
        scene.run_human_founds();
        let fb = scene.human_founds[0].block.as_ref().unwrap();
        assert_eq!((fb.class, fb.area, fb.done), (2, (b.0, b.1, 1, 1), false));
        let mut g = scene.human_founds.remove(0);
        assert!(!scene.found_send(&mut g, boat));
    }

    /// A Shift found click on a group sailing a Shift move's route keeps
    /// the route (0x4cbcf0 with flags 4): the boat, on it, waits
    /// (0x4cb5f0); at the route's end (0x136, 0x4ce7c0 past the last
    /// waypoint) it is taken off the route and sent to found (0x4cb6f0).
    #[test]
    fn a_founding_group_sails_its_route_first() {
        let Some((mut scene, boat)) = boat_scene() else {
            return;
        };
        let w = scene.map_cells().0;
        let a = fort_sites(&scene)[0];
        let i = scene.object_index(boat).unwrap();
        let at = scene.ship_cell(i);
        scene.order_shift_move(&[boat], at.0, at.1).unwrap();
        scene.outbox.clear();
        scene.order_human_found(&[boat], a.0, a.1, 2, 4).unwrap();
        assert!(scene.waypoint_moves.is_empty());
        let g = &scene.human_founds[0];
        assert_eq!(g.route.as_ref().unwrap().waypoints, vec![at]);
        assert_eq!(g.block.as_ref().unwrap().waiting, vec![boat]);
        assert!(found_sent(&scene, boat).is_empty());
        scene.post_notice(boat, 0x136, (at.1 * w + at.0) as u32);
        scene.run_human_founds();
        let g = &scene.human_founds[0];
        assert!(g.block.as_ref().unwrap().waiting.is_empty());
        assert!(g.route.as_ref().unwrap().progress.is_empty());
        assert_eq!(found_sent(&scene, boat), vec![(a.0, a.1, 0x34)]);
    }
}
