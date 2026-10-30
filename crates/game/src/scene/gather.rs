//! The navy's gather zones: fleet order 0x78 / 0x3d (0x4c8a80), a group's
//! task over a rectangle of cells and a resource class. The group's ships
//! post notices to its list (`+0x2d`: 0x138 when a gathering ends, 0x139
//! when an unloading ends, 300 when a ship joins); at the navy's tick the
//! zone reads them, newest first (0x4ca8d0 -> 0x4c9e30), and sends its
//! ships to a deposit or a port through the outbox (0x4c89b0, 0x520cc0);
//! every 86 calls it plans its idle ships again (0x4ca290). See
//! docs/FORMATS.md ("Gather zones").

use super::{walk_path, Order, Scene};
use crate::objects::{cargo_capacity, deep_draught, GatherPhase, Resource, Task};
use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;
use serde::{Deserialize, Serialize};

/// A rectangle of cells: x, y, width, height.
pub type Cells = (i32, i32, i32, i32);

/// Whose group a zone is the task of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneGroup {
    /// A human's selection, made a group by the click (0x49ca60), by a
    /// serial number.
    Human(u32),
    /// A brain's fleet, by its index.
    Brain(usize),
    /// A computer player's flotilla: the player's id (nation x 4 + slot)
    /// and the flotilla's index.
    Computer(u32, usize),
}

/// A notice in a group's list (the posters 0x4a1240-0x4a1bf0: `{code,
/// tick, ship +0x60, ship id, rec[4]}`): `ship` the ship's (0x145: the
/// port's) id, `class` rec[4]: the class (0x138), a cell `y * W + x`
/// (0x133, and 0x145's rec[5]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub code: u32,
    pub tick: u32,
    pub ship: u32,
    pub class: u32,
}

/// A gather zone: the group's block `+0x25` (0x65 bytes) and the group's
/// own fields the zone reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatherZone {
    pub group: ZoneGroup,
    /// The group's player (from its ships).
    pub owner: Option<(u32, u32)>,
    /// Block `+0xc..+0x18`.
    pub rect: Cells,
    /// Block `+0x20`: 400 amber, 0x191 fish, 0x192 leviathan.
    pub class: Resource,
    /// Block `+0x24`: bit 0 sends laden ships to unload when no deposit
    /// is left; `>> 16` 1 keeps the zone from being marked done.
    pub flags: u32,
    /// Group `+0x10`: the order's key flags were not 0 (only the first
    /// deposit found counts, 0x4c5b90).
    pub first_only: bool,
    /// Block `+0x39`: the deposits known, by id.
    pub deposits: Vec<u32>,
    /// Block `+0x28`: the port list, by id (null until 0x4c6c30 fills it).
    pub ports: Option<Vec<u32>>,
    /// Group `+0x31`: its ships, by id.
    pub ships: Vec<u32>,
    /// Group `+0x2d`: the notices not yet read, oldest first.
    pub notices: Vec<Notice>,
    /// Block `+0x41`: the escorts (0x4c6100).
    #[serde(default)]
    pub escorts: Vec<Escort>,
    /// Block `+0x35`: the cells its ships were wrecked in (notice 0x133),
    /// duplicates kept.
    #[serde(default)]
    pub wrecks: Vec<u32>,
    /// Group `+8`: calls since the last re-plan.
    pub elapsed: u32,
    /// Block `+0x5d`: nothing left to do (0x4ca290).
    pub done: bool,
}

/// An entry of block `+0x41` (0xd bytes: `{escort, ward, target,
/// state}`; 0x4c6100 appends `{escort, ward, 0, 0}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Escort {
    pub ship: u32,
    pub ward: u32,
    /// `+0xc`: 10 following, 0xb after the ward's attacker.
    pub state: u8,
    /// `+8`: the ward's attacker it was sent after.
    #[serde(default)]
    pub target: u32,
}

/// The class word of a resource (the deposits' classes 400, 0x191, 0x192).
pub fn class_code(r: Resource) -> u32 {
    match r {
        Resource::Amber => 400,
        Resource::Fish => 0x191,
        Resource::Leviathan => 0x192,
    }
}

/// A deposit found by 0x4c5b90 / 0x4c95f0: its cell, amount `+0x78` and
/// id.
#[derive(Debug, Clone, Copy)]
pub(super) struct Cand {
    pub cell: (i32, i32),
    pub amount: i32,
    pub id: u32,
}

/// What 0x4c8f20 keeps for each candidate (0x18 bytes): its id, cell,
/// port, the port's score.
#[derive(Debug, Clone, Copy)]
pub(super) struct Rec {
    pub id: u32,
    pub cell: (i32, i32),
    pub port: u32,
    pub pscore: i32,
    pub amount: i32,
}

/// Ticks of the wait at a crowded berth before a merchant steps aside
/// (0x4adef0 step 1: `== 300`), and before it gives its port up (`> 349`).
pub const BERTH_ASIDE: u32 = 300;
pub const BERTH_GIVE_UP: u32 = 350;

/// What 0x4c7fc0 reads for its last port when that port failed the test:
/// the stack probe's return address 0x4c8068 left in the slot, read as an
/// object, gives the dword at 0x4c80b4 as its id.
pub const PROBE_PORT_ID: u32 = 0xffff_6c85;
/// The last port's `s` in 0x4c7fc0 before it is rated: the stack probe's
/// return address 0x4c8090.
pub const PROBE_SCORE: i32 = 0x004c_8090;

impl Scene {
    /// The navy's generator (`[0x5c7504] +0x18`, the usual LCG), seeded
    /// with the game's `+0x2ee5` (`RND_INIT`) at its first tick (0x49ac11
    /// / 0x49ac1f).
    pub(super) fn navy_draw(&mut self) -> u32 {
        let init = match self.rnd_init {
            Some(v) => v,
            None => {
                let v = self.rng.next() << 16 | self.rng.next();
                self.rnd_init = Some(v);
                v
            }
        };
        let mut r = crate::combat::Rng(self.navy_lcg.unwrap_or(init));
        let v = r.next();
        self.navy_lcg = Some(r.0);
        v
    }

    /// The zone whose group holds ship `id`.
    pub fn ship_zone(&self, id: u32) -> Option<usize> {
        self.gather_zones.iter().position(|z| z.ships.contains(&id))
    }

    /// Takes ships out of their groups' zones (a new group made of them,
    /// 0x49ca60); each posts notice 0x137 to the group it leaves
    /// (0x49bef0 -> 0x4a1770, before the removal); a human's zone left
    /// with none goes.
    pub(super) fn leave_zones(&mut self, ids: &[u32]) {
        let tick = self.ticks;
        for z in &mut self.gather_zones {
            if matches!(z.group, ZoneGroup::Human(_)) {
                for &s in &z.ships {
                    if ids.contains(&s) {
                        z.notices.push(Notice {
                            code: 0x137,
                            tick,
                            ship: s,
                            class: 0,
                        });
                    }
                }
                z.ships.retain(|s| !ids.contains(s));
            }
        }
        self.gather_zones
            .retain(|z| !(matches!(z.group, ZoneGroup::Human(_)) && z.ships.is_empty()));
    }

    /// Ends the gather zone of `group` (its task replaced, 0x4a1c60).
    pub(super) fn end_zone(&mut self, group: ZoneGroup) {
        self.gather_zones.retain(|z| z.group != group);
    }

    /// Posts a notice to ship `id`'s group (0x4a1430 / 0x4a1520 /
    /// 0x4a1340); nothing without one.
    pub(super) fn post_notice(&mut self, id: u32, code: u32, class: u32) {
        let tick = self.ticks;
        if let Some(z) = self.ship_zone(id) {
            self.gather_zones[z].notices.push(Notice {
                code,
                tick,
                ship: id,
                class,
            });
        }
    }

    /// A ship wrecked by a reef or by lightning (0x4dc630 on 0xda94, after
    /// the wreck event) posts notice 0x133 with its cell (0x4a1620).
    pub(super) fn post_wreck(&mut self, id: u32) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let t = TILE_SIZE as f32;
        let (x, y) = (
            (self.objects[i].x / t).floor() as i32,
            (self.objects[i].y / t).floor() as i32,
        );
        let cell = (y * self.nav.width as i32 + x) as u32;
        self.post_notice(id, 0x133, cell);
    }

    /// A port of the player's first standing (its vt+0x94, 0x42ff90 ->
    /// 0x4a12d0) posts notice 0x145 with its cell to every zone of its
    /// player.
    pub(super) fn post_new_port(&mut self, id: u32) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let (owner, Some((x, y))) = (self.objects[i].owner, self.objects[i].cell) else {
            return;
        };
        let cell = (y * self.nav.width as i32 + x) as u32;
        let tick = self.ticks;
        for z in &mut self.gather_zones {
            if owner.is_some() && z.owner == owner {
                z.notices.push(Notice {
                    code: 0x145,
                    tick,
                    ship: id,
                    class: cell,
                });
            }
        }
    }

    /// The human's gather click on a deposit (codes 0x19 fish, 0x1a amber,
    /// 0x1c / 0x1d leviathan; 0x49e230 -> 0x49d3c0) with the click's key
    /// flags. With Shift (4) on the selection's group (0x49e230 keeps the
    /// group while the selection is unchanged: here a zone of exactly
    /// these ships) running a zone: of the code's class (400 for amber,
    /// 0x191 for the others, so never a leviathan zone) the deposit is
    /// only known (0x4c6e30), no ship sent; of another, nothing. Else the
    /// selection made a group, a zone of the 3 x 3 cells about the
    /// deposit's cell (1 x 1 with Shift), clipped to the map, its class,
    /// `+0x24 = 0x100`, args[9] -1, started (0x4c8a80 with the flags:
    /// any set group `+0x10`, the first deposit only). (Not ported: the
    /// message 0xc45d 0x4a1f20 sends the interface on the Shift add; the
    /// kept `+0x61` of a group's earlier waypoint move, which skips the
    /// start's dispatch: the port's zones have none.)
    pub fn order_gather_at(&mut self, ships: &[u32], deposit: u32, keys: u8) -> Result<(), String> {
        let d = self.object_index(deposit).ok_or("No such deposit")?;
        let class = deposit_class(&self.objects[d]).ok_or("Nothing to gather there")?;
        let (cx, cy) = self.deposit_cell(d);
        let shift = keys & 4 != 0;
        if shift {
            let mut sel: Vec<u32> = ships.to_vec();
            sel.sort_unstable();
            let same = self.gather_zones.iter().position(|z| {
                let mut s = z.ships.clone();
                s.sort_unstable();
                matches!(z.group, ZoneGroup::Human(_)) && s == sel
            });
            if let Some(z) = same {
                let code = if class == Resource::Amber { 400 } else { 0x191 };
                if class_code(self.gather_zones[z].class) == code {
                    let id = self.objects[d].id;
                    if !self.gather_zones[z].deposits.contains(&id) {
                        self.gather_zones[z].deposits.push(id);
                    }
                }
                return Ok(());
            }
        }
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let u = i32::from(!shift);
        let (x0, y0) = ((cx - u).max(0), (cy - u).max(0));
        let rect = (x0, y0, (cx + u + 1).min(w) - x0, (cy + u + 1).min(h) - y0);
        self.human_zone(ships, rect, class, 0x100, keys != 0)
    }

    /// The merchants' amber, fish and leviathan zone buttons (0x23 sub
    /// 0x10-0x12, 0x49d790): a zone of the rectangle, flags 0x10000. The
    /// drag (0x51cdd0) takes its start and end cells (pixels / 64 toward
    /// zero), orders them and sends `{x0, y0, x1 - x0 + 1, y1 - y0 + 1}`
    /// (0x517c00, key flags 0): the cells dragged over, not clipped to
    /// the map (0x4c5b90 clips its walk).
    pub fn order_gather_area(
        &mut self,
        ships: &[u32],
        a: (f32, f32),
        b: (f32, f32),
        class: Resource,
    ) -> Result<(), String> {
        let t = TILE_SIZE as f32;
        let cell = |v: f32| (v / t) as i32;
        let (ax, ay, bx, by) = (cell(a.0), cell(a.1), cell(b.0), cell(b.1));
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let (y0, y1) = (ay.min(by), ay.max(by));
        self.human_zone(
            ships,
            (x0, y0, x1 - x0 + 1, y1 - y0 + 1),
            class,
            0x10000,
            false,
        )
    }

    fn human_zone(
        &mut self,
        ships: &[u32],
        rect: Cells,
        class: Resource,
        flags: u32,
        first_only: bool,
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
        self.leave_zones(&ships);
        self.zone_serial += 1;
        let group = ZoneGroup::Human(self.zone_serial);
        self.start_zone(group, ships, rect, class, flags, first_only);
        Ok(())
    }

    /// Fleet order 0x78 (0x4c8a80) to `group` of `ships`: a group running
    /// a zone already has its lists cleared (0x4c7650: deposits, port
    /// list, escorts; the notices kept) and its block rewritten; else a
    /// new block (0x4c87d0). Then the rectangle's deposits of the class
    /// are listed and known (0x4c5b90 mode 0) and, if any, the group's
    /// ships split (0x4c5f40) and sent (0x4c6ee0). No port list: null,
    /// for 0x4c6c30 to fill.
    pub(super) fn start_zone(
        &mut self,
        group: ZoneGroup,
        ships: Vec<u32>,
        rect: Cells,
        class: Resource,
        flags: u32,
        first_only: bool,
    ) {
        self.start_zone_ports(group, ships, rect, class, flags, first_only, Vec::new());
    }

    /// `start_zone` with the order's port list (args[7] / args[8]): each
    /// port added once by id (0x4c6400); none leaves the list null.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_zone_ports(
        &mut self,
        group: ZoneGroup,
        ships: Vec<u32>,
        rect: Cells,
        class: Resource,
        flags: u32,
        first_only: bool,
        listed: Vec<u32>,
    ) {
        let mut ports: Vec<u32> = Vec::new();
        for id in listed {
            if !ports.contains(&id) {
                ports.push(id);
            }
        }
        let ports = (!ports.is_empty()).then_some(ports);
        let owner = ships
            .iter()
            .find_map(|&id| self.object_index(id).and_then(|i| self.objects[i].owner));
        let z = match self.gather_zones.iter().position(|z| z.group == group) {
            Some(z) => {
                let zone = &mut self.gather_zones[z];
                zone.deposits.clear();
                zone.ports = ports;
                zone.escorts.clear();
                zone.wrecks.clear();
                zone.rect = rect;
                zone.class = class;
                zone.flags = flags;
                zone.ships = ships;
                if owner.is_some() {
                    zone.owner = owner;
                }
                zone.first_only |= first_only;
                z
            }
            None => {
                self.gather_zones.push(GatherZone {
                    group,
                    owner,
                    rect,
                    class,
                    flags,
                    first_only,
                    deposits: Vec::new(),
                    ports,
                    ships,
                    notices: Vec::new(),
                    escorts: Vec::new(),
                    wrecks: Vec::new(),
                    elapsed: 0,
                    done: false,
                });
                self.gather_zones.len() - 1
            }
        };
        let cands = self.zone_deposits(z, 0);
        if cands.is_empty() {
            return;
        }
        let (merchants, others) = self.zone_split(z);
        if merchants.is_empty() {
            return;
        }
        self.zone_dispatch(z, &cands, &merchants, &others);
    }

    /// 0x4c6ee0: fewer than 6 merchants, the port list filled if null
    /// (0x4c6c30) and each merchant in order to 0x4c8f20 with port
    /// argument 0; 6 or more, ship k to deposit k mod n, but a computer's
    /// (side 3 or 4) with less than half its room left, or a leviathan
    /// aboard, to the zone's port for it (command 0x1f) if there is one;
    /// then each other ship follows a merchant in turn (0x4d8b30 with 2,
    /// 0x4c6100).
    fn zone_dispatch(&mut self, z: usize, cands: &[Cand], merchants: &[u32], others: &[u32]) {
        if self.gather_zones[z].ports.is_none() {
            self.zone_port_list(z);
        }
        if merchants.len() < 6 {
            for &m in merchants {
                if let Some(i) = self.object_index(m) {
                    self.zone_weighted(z, i, cands, false);
                }
            }
        } else {
            let class = self.gather_zones[z].class;
            for (k, &m) in merchants.iter().enumerate() {
                let Some(i) = self.object_index(m) else {
                    continue;
                };
                let s = self.objects[i].ship.as_ref().unwrap();
                let cap = cargo_capacity(s.kind) as i32 / 100;
                let low = self.room(i) * 100 / cap.max(1) < 50
                    || s.cargo_kind == Some(Resource::Leviathan) && s.cargo > 0.0;
                if self.side_computer(self.objects[i].owner) && low {
                    if let Some(p) = self.zone_port_pick(z, Some(i)) {
                        self.zone_send_port(m, p);
                        continue;
                    }
                }
                let c = cands[k % cands.len()];
                if let Some(d) = self.layer_deposit(c.cell) {
                    if deposit_class(&self.objects[d]) == Some(class) {
                        let did = self.objects[d].id;
                        self.zone_send_gather(m, did, 0);
                    }
                }
            }
        }
        for (k, &o) in others.iter().enumerate() {
            let ward = merchants[k % merchants.len()];
            self.zone_follow(z, o, ward);
        }
    }

    /// Ship `o` follows `ward` within 2 cells (message `*`, 0x4d8b30 with
    /// 2 and priority 0) and is listed as its escort (0x4c6100: a new
    /// entry, or its ward changed). (Not ported: 0x4c6100's behaviour
    /// command 0x23 with -1s, 0x4c42f0, sent for a new escort.)
    fn zone_follow(&mut self, z: usize, o: u32, ward: u32) {
        self.post(Order::Follow {
            ship: o,
            ward,
            range: 2,
        });
        let e = &mut self.gather_zones[z].escorts;
        match e.iter_mut().find(|e| e.ship == o) {
            Some(x) => x.ward = ward,
            None => e.push(Escort {
                ship: o,
                ward,
                state: 0,
                target: 0,
            }),
        }
    }

    /// Whether `owner` is a computer's player (side `game +0x1a1d` 3 or
    /// 4): not the human's, nor a seat's (1 local, 2 remote), nor player
    /// 16 (0).
    fn side_computer(&self, owner: Option<(u32, u32)>) -> bool {
        let Some(o) = owner else {
            return false;
        };
        o != (4, 0)
            && Some(o) != self.human.map(|h| (h.nation, h.slot))
            && !self
                .seats
                .iter()
                .any(|s| (s.player.nation, s.player.slot) == o)
    }

    /// A deposit's class word by its object class.
    fn deposit_cell(&self, d: usize) -> (i32, i32) {
        let o = &self.objects[d];
        if o.class == ObjectClass::Amber {
            if let Some(f) = o.record.fields.get(6..8) {
                return (f[0] as i32, f[1] as i32);
            }
        }
        let t = TILE_SIZE as f32;
        ((o.x / t).floor() as i32, (o.y / t).floor() as i32)
    }

    /// The object in a cell's deposit slot (`grid +0x10`): an amber (its
    /// cell, fields 6 and 7, standing or lying fallow), a shoal or a
    /// leviathan (where it is).
    pub(super) fn layer_deposit(&self, cell: (i32, i32)) -> Option<usize> {
        (0..self.objects.len())
            .find(|&k| deposit_class(&self.objects[k]).is_some() && self.deposit_cell(k) == cell)
    }

    /// 0x4c95f0: deposit `id` still to be had (amber state `+0x74` below
    /// 2, fish below 4, a leviathan below 5): here, one with its deposit.
    /// (The port does not keep an exhausted amber's state 1, a tick
    /// before it lies fallow, apart.)
    fn deposit_valid(&self, id: u32) -> Option<Cand> {
        let d = self.object_index(id)?;
        deposit_class(&self.objects[d])?;
        let dep = self.objects[d].deposit?;
        Some(Cand {
            cell: self.deposit_cell(d),
            amount: dep.remaining.map_or(0, |r| r as i32),
            id,
        })
    }

    fn cand_at(&self, d: usize, cell: (i32, i32)) -> Cand {
        Cand {
            cell,
            amount: self.objects[d]
                .deposit
                .and_then(|x| x.remaining)
                .map_or(0, |r| r as i32),
            id: self.objects[d].id,
        }
    }

    /// 0x4c5b90(class, rect, mode): with mode 1 first the known deposits,
    /// last to first, those no longer to be had forgotten; then (modes 0
    /// and 1) the rectangle row by row, each deposit of the class (no
    /// state test): mode 0 knows and lists it, mode 1 lists it only if
    /// not known. With group `+0x10` set only the first counts (the last
    /// of the known copied over it).
    fn zone_deposits(&mut self, z: usize, mode: u8) -> Vec<Cand> {
        let mut list = Vec::new();
        let first_only = self.gather_zones[z].first_only;
        if mode != 0 {
            let mut k = self.gather_zones[z].deposits.len();
            while k > 0 {
                k -= 1;
                let id = self.gather_zones[z].deposits[k];
                match self.deposit_valid(id) {
                    Some(c) => list.push(c),
                    None => {
                        self.gather_zones[z].deposits.remove(k);
                    }
                }
            }
            if first_only && list.len() > 1 {
                list[0] = *list.last().unwrap();
            }
        }
        if mode < 2 {
            let (x0, y0, w, h) = self.gather_zones[z].rect;
            let class = self.gather_zones[z].class;
            let (mw, mh) = (self.nav.width as i32, self.nav.height as i32);
            for y in y0.max(0)..(y0 + h).min(mh) {
                for x in x0.max(0)..(x0 + w).min(mw) {
                    let Some(d) = self.layer_deposit((x, y)) else {
                        continue;
                    };
                    if deposit_class(&self.objects[d]) != Some(class) {
                        continue;
                    }
                    let id = self.objects[d].id;
                    let known = self.gather_zones[z].deposits.contains(&id);
                    if mode == 0 {
                        if !known {
                            self.gather_zones[z].deposits.push(id);
                        }
                    } else if known {
                        continue;
                    }
                    list.push(self.cand_at(d, (x, y)));
                }
            }
        }
        if first_only && list.len() > 1 {
            list.truncate(1);
        }
        list
    }

    /// 0x4c5f40: the group's ships, merchants (types 0 and 1) with their
    /// room (0x4be800 with 3), most room first by the original's exchange
    /// sort, and the others.
    fn zone_split(&self, z: usize) -> (Vec<u32>, Vec<u32>) {
        let mut merchants: Vec<(u32, i32)> = Vec::new();
        let mut others = Vec::new();
        for &id in &self.gather_zones[z].ships {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let Some(s) = self.objects[i].ship.as_ref() else {
                continue;
            };
            if s.kind < 2 {
                merchants.push((id, self.room(i)));
            } else {
                others.push(id);
            }
        }
        for i in 0..merchants.len() {
            for j in 0..i {
                if merchants[j].1 < merchants[i].1 {
                    merchants.swap(j, i);
                }
            }
        }
        (merchants.into_iter().map(|m| m.0).collect(), others)
    }

    /// 0x4be800(ship, 3): a merchant's free room in loads of 100 (its
    /// type's 4 or 10, less the loads aboard, a part load counting whole);
    /// 0 for other ships.
    pub(super) fn room(&self, i: usize) -> i32 {
        let Some(s) = self.objects[i].ship.as_ref() else {
            return 0;
        };
        if s.kind > 1 {
            return 0;
        }
        let cap = cargo_capacity(s.kind) as i32 / 100;
        (cap - (s.cargo / 100.0).ceil() as i32).max(0)
    }

    /// 0x4ad280(ship, class): `(100 - P) + room x 100`, P the fill of its
    /// top load of the class (`+0x527 + 4k`, 100 when whole or none); 0
    /// when full.
    fn fill_left(&self, i: usize, class: Resource) -> i32 {
        let s = self.objects[i].ship.as_ref().unwrap();
        let p = if s.cargo_kind == Some(class) && s.cargo > 0.0 {
            let n = (s.cargo / 100.0).ceil();
            (s.cargo - 100.0 * (n - 1.0)).round() as i32
        } else {
            100
        };
        (100 - p) + self.room(i) * 100
    }

    /// The navy's tick for the gather zones (message 0, 0x4c4f40 -> order
    /// 0x78 sub 0x3d -> 0x4ca8d0), each zone in turn: a brain's or
    /// computer's group takes its fleet's ships; its notices are read
    /// (0x4c9e30) and, when that sent nothing and more than 85 calls have
    /// passed, its ships planned again (0x4ca290); the count grows by
    /// the tick.
    pub(super) fn run_gather_zones(&mut self) {
        self.run_berth_waits();
        let mut z = 0;
        while z < self.gather_zones.len() {
            let ships: Option<Vec<u32>> = match self.gather_zones[z].group {
                ZoneGroup::Brain(k) => self
                    .brain
                    .as_ref()
                    .and_then(|b| b.fleets.get(k))
                    .map(|f| f.ships.clone()),
                ZoneGroup::Computer(p, f) => self.computer_flotilla_ships(p, f),
                ZoneGroup::Human(_) => None,
            };
            if let Some(mut ships) = ships {
                let mut seen = std::collections::HashSet::new();
                ships.retain(|id| seen.insert(*id));
                self.gather_zones[z].ships = ships;
            }
            let live: Vec<u32> = self.gather_zones[z]
                .ships
                .iter()
                .copied()
                .filter(|&id| {
                    self.object_index(id)
                        .is_some_and(|i| self.objects[i].ship.is_some())
                })
                .collect();
            if let Some(o) = live
                .iter()
                .find_map(|&id| self.objects[self.object_index(id).unwrap()].owner)
            {
                self.gather_zones[z].owner = Some(o);
            }
            self.gather_zones[z].ships = live;
            let mut sent = 0;
            if !self.gather_zones[z].notices.is_empty() {
                sent = self.zone_notices(z);
            }
            if sent == 0
                && self.gather_zones[z].elapsed > 0x55
                && !self.gather_zones[z].ships.is_empty()
            {
                self.gather_zones[z].elapsed = 0;
                self.zone_replan(z);
            }
            self.gather_zones[z].elapsed += 1;
            z += 1;
        }
        self.gather_zones
            .retain(|z| !(matches!(z.group, ZoneGroup::Human(_)) && z.ships.is_empty()));
    }

    /// 0x4c9e30: the notices newest first, each removed after its case;
    /// returns the last value a case wrote. The ship's cases, a notice of
    /// a ship no longer in the group dropped: 300 (a ship joined): its
    /// next deposit (0x4c9930); 0x138: 0x4c9ae0; 0x139: 0x4c9930; 0x136
    /// (arrived): only with a route. The others: 0x133 (wrecked) its cell
    /// kept (`+0x35`); 0x137 (a ship left) its escort entry removed
    /// (0x4c61d0); 0x145 (a new port) still standing at its cell: listed
    /// (0x4c6400) and 0x4c7b80. (Not ported: the deferred orders of a
    /// route, `+0x3d` with 0x4cabd0 / 0x4cac30, and 300's behaviour
    /// command 0x4c42f0; 0x12f, the takes, adds to `+0x30`, which nothing
    /// reads; 0x13e, the human's Shift unload click on a port, toggling
    /// it in the list (0x49dfb0), whose case 0x4c6ad0 sends nothing; no
    /// code posts 0x146-0x148.)
    fn zone_notices(&mut self, z: usize) -> i32 {
        let mut r = 0;
        while let Some(n) = self.gather_zones.get_mut(z).and_then(|zz| zz.notices.pop()) {
            match n.code {
                0x133 => {
                    self.gather_zones[z].wrecks.push(n.class);
                    continue;
                }
                0x137 => {
                    let e = &mut self.gather_zones[z].escorts;
                    if let Some(k) = e.iter().position(|e| e.ship == n.ship) {
                        e.remove(k);
                    }
                    continue;
                }
                0x145 => {
                    self.zone_new_port(z, n.ship, n.class);
                    continue;
                }
                300 | 0x138 | 0x139 => {}
                _ => continue,
            }
            if !self.gather_zones[z].ships.contains(&n.ship) {
                continue;
            }
            let Some(i) = self.object_index(n.ship) else {
                continue;
            };
            r = match n.code {
                300 | 0x139 => self.zone_next_deposit(z, i, false),
                _ => self.zone_heard_full(z, i),
            };
        }
        r
    }

    /// Notice 0x145: the port still the building of its cell: added to
    /// the list (made if null, once by id: 0x4c6400), then 0x4c7b80: for a
    /// human's zone (side 1 or 2), or when a listed port of class 0x39
    /// within 10 cells (0x557440) of the new one has more than 2 merchants
    /// bound (0x4c7ea0), each merchant unloading (sub-code 0xc) is sent at
    /// once to the zone's pick (0x4c6640, command 0x1f).
    fn zone_new_port(&mut self, z: usize, id: u32, cell: u32) {
        use crate::economy::PORT;
        let w = self.nav.width as i32;
        let at = ((cell as i32) % w, (cell as i32) / w);
        let Some(p) = self.object_index(id) else {
            return;
        };
        if self.objects[p].cell != Some(at)
            || !matches!(self.objects[p].class, ObjectClass::Building(_))
        {
            return;
        }
        let list = self.gather_zones[z].ports.get_or_insert_with(Vec::new);
        if !list.contains(&id) {
            list.push(id);
        }
        let list = self.gather_zones[z].ports.clone().unwrap_or_default();
        let owner = self.gather_zones[z].owner;
        let mut send = !self.side_computer(owner) && owner != Some((4, 0));
        for &e in &list {
            if e == id {
                continue;
            }
            let Some(k) = self.object_index(e) else {
                continue;
            };
            if self.objects[k].class != ObjectClass::Building(PORT) {
                continue;
            }
            let Some(c) = self.objects[k].cell else {
                continue;
            };
            if walk_path::dist(c.0, c.1, at.0, at.1) < 11 && self.port_load(z, e) > 2 {
                send = true;
            }
        }
        if !send {
            return;
        }
        for sid in self.gather_zones[z].ships.clone() {
            let Some(i) = self.object_index(sid) else {
                continue;
            };
            let s = self.objects[i].ship.as_ref().unwrap();
            let unloading = matches!(
                s.task,
                Some(Task::Gather {
                    phase: GatherPhase::ToPort(_) | GatherPhase::Unloading(_),
                    ..
                })
            );
            if s.kind < 2 && unloading {
                if let Some(port) = self.zone_port_pick(z, Some(i)) {
                    self.zone_send_port(sid, port);
                }
            }
        }
    }

    /// 0x4c9ae0 (notice 0x138): with room for more (0x4ad280 not 0, and
    /// not a leviathan aboard of a leviathan zone) and another deposit
    /// sent (0x4c9930 not 0), done; else its own port `+0x7ad` if in the
    /// zone's list (0x4c64b0) and its player's (0x4ac050), else the
    /// zone's pick (0x4c6640); command 0x1f to it. No port: nothing sent.
    fn zone_heard_full(&mut self, z: usize, i: usize) -> i32 {
        let class = self.gather_zones[z].class;
        let s = self.objects[i].ship.as_ref().unwrap();
        let lev = class == Resource::Leviathan
            && s.cargo_kind == Some(Resource::Leviathan)
            && s.cargo > 0.0;
        if self.fill_left(i, class) != 0 && !lev && self.zone_next_deposit(z, i, false) != 0 {
            return 1;
        }
        let home = self.objects[i].ship.as_ref().unwrap().home_port;
        let listed = home != 0 && self.zone_port_list(z).contains(&home);
        let own = self.object_index(home).is_some_and(|p| {
            self.objects[p].owner == self.objects[i].owner && self.takes_cargo(&self.objects[p])
        });
        let port = if listed && own {
            Some(home)
        } else {
            self.zone_port_pick(z, Some(i))
        };
        if let Some(p) = port {
            let id = self.objects[i].id;
            self.zone_send_port(id, p);
        }
        0
    }

    /// 0x4c9930(ship, 0, route): the same deposit again (0x4c96e0); none,
    /// the zone's deposits not known yet (0x4c5b90 mode 1) or, none of
    /// those, the known ones to be had (0x4c95f0, the others forgotten:
    /// the original's walk skips the one after each it forgets) to the
    /// weighted pick 0x4c8f20 with port argument 1, which counts 1 only
    /// when it returns non-zero.
    fn zone_next_deposit(&mut self, z: usize, i: usize, route: bool) -> i32 {
        let n = self.zone_same_deposit(z, i, route);
        if n >= 1 {
            return n;
        }
        let list = self.zone_deposits(z, 1);
        if !list.is_empty() {
            return i32::from(self.zone_weighted(z, i, &list, true) != 0);
        }
        let mut list = Vec::new();
        let mut count = self.gather_zones[z].deposits.len();
        let mut k = 0;
        while k < count {
            let id = self.gather_zones[z].deposits[k];
            match self.deposit_valid(id) {
                Some(c) => list.push(c),
                None => {
                    self.gather_zones[z].deposits.remove(k);
                    count -= 1;
                }
            }
            k += 1;
        }
        if !list.is_empty() && self.zone_weighted(z, i, &list, true) != 0 {
            return 1;
        }
        0
    }

    /// 0x4c96e0: no room left, to the zone's port for it (command 0x1f);
    /// else by the ship's class `+0x76e`: amber, the amber still in its
    /// cell `+0x797` (any amber there), else the nearest to the middle of
    /// its box (0x4ac610 mode 1); fish, one of its box's shoals drawn
    /// with the navy's generator (mode 2); a leviathan, mode 1, none
    /// giving 0. The deposit is known (0x4c6e30) and the ship sent there
    /// with port 0.
    fn zone_same_deposit(&mut self, z: usize, i: usize, _route: bool) -> i32 {
        let id = self.objects[i].id;
        if self.room(i) == 0 {
            if let Some(p) = self.zone_port_pick(z, Some(i)) {
                self.zone_send_port(id, p);
                return 1;
            }
        }
        let s = self.objects[i].ship.as_ref().unwrap();
        let Some(at) = s.gathered else {
            return 0;
        };
        let d = match at.class {
            Resource::Amber => self
                .layer_deposit(at.cell)
                .filter(|&d| self.objects[d].class == ObjectClass::Amber)
                .or_else(|| self.box_deposit(i, at.rect, Resource::Amber, 1, -1)),
            // (0x49a270, the shoal last fished in the ship's cell's or a
            // neighbour's object slot, never holds a shoal: those lie in
            // the deposit slot.)
            Resource::Fish => {
                let n = (self.navy_draw() % 1000) as i32;
                self.box_deposit(i, at.rect, Resource::Fish, 2, n)
            }
            Resource::Leviathan => self.box_deposit(i, at.rect, Resource::Leviathan, 1, -1),
        };
        let Some(d) = d else {
            return 0;
        };
        let did = self.objects[d].id;
        if !self.gather_zones[z].deposits.contains(&did) {
            self.gather_zones[z].deposits.push(did);
        }
        self.zone_send_gather(id, did, 0);
        1
    }

    /// 0x4ac610(ship, x0, y0, x1, y1, class, mode, n): the deposits of the
    /// class in the cells `[x0, x1) x [y0, y1)` row by row (an amber only
    /// standing, its picture `+0xd5`), at most 24; one, it; more: mode 0
    /// the most reached by the wavefront from the ship (0x545e20 with the
    /// map's width as limit, the first on a tie), mode 1 the nearest
    /// (0x557440) to the box's middle, mode 2 hit `n % count` (n below 0:
    /// one draw of the ship's generator).
    pub(super) fn box_deposit(
        &mut self,
        i: usize,
        (x0, y0, x1, y1): (i32, i32, i32, i32),
        class: Resource,
        mode: u8,
        n: i32,
    ) -> Option<usize> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let mut hits: Vec<(usize, (i32, i32))> = Vec::new();
        for y in y0..y1 {
            if y < 0 || y >= h {
                continue;
            }
            for x in x0..x1 {
                if hits.len() >= 24 || x < 0 || x >= w {
                    continue;
                }
                let Some(d) = self.layer_deposit((x, y)) else {
                    continue;
                };
                let o = &self.objects[d];
                if deposit_class(o) != Some(class) {
                    continue;
                }
                if class == Resource::Amber && o.deposit.is_none() {
                    continue;
                }
                hits.push((d, (x, y)));
            }
        }
        match hits.len() {
            0 => return None,
            1 => return Some(hits[0].0),
            _ => {}
        }
        match mode {
            0 => {
                let t = TILE_SIZE as f32;
                let at = (
                    (self.objects[i].x / t).floor() as i32,
                    (self.objects[i].y / t).floor() as i32,
                );
                let deep = deep_draught(self.objects[i].ship.as_ref()?.kind);
                let f = self.nav.wavefront(at, deep, w)?;
                let v = |c: (i32, i32)| f[(c.1 * w + c.0) as usize];
                let mut best = hits[0];
                for &e in &hits[1..] {
                    if v(best.1) < v(e.1) {
                        best = e;
                    }
                }
                Some(best.0)
            }
            1 => {
                let (cx, cy) = ((x1 - x0) / 2 + x0, (y1 - y0) / 2 + y0);
                let d = |c: (i32, i32)| walk_path::dist(cx, cy, c.0, c.1);
                let mut best = hits[0];
                for &e in &hits[1..] {
                    if d(e.1) < d(best.1) {
                        best = e;
                    }
                }
                Some(best.0)
            }
            2 => {
                let k = if n < 0 {
                    self.ship_draw(i) as usize % hits.len()
                } else {
                    n as usize % hits.len()
                };
                Some(hits[k].0)
            }
            _ => Some(hits[0].0),
        }
    }

    /// 0x4c8f20(ship, list, route, port_arg): one candidate, a ship not a
    /// merchant, or a leviathan zone: 0x4c9270. Else each deposit's port
    /// and score (0x4c7fc0); weights the port's score plus the deposit's
    /// amount; three draws of the navy's generator `% total`, each the
    /// first whose running sum passes it, keeping the heaviest (strict);
    /// no room, command 0x1f to its port; else the gather command with
    /// that port, or 0 without `port_arg`. Returns 0 on this path always
    /// (0x4c9257).
    fn zone_weighted(&mut self, z: usize, i: usize, cands: &[Cand], port_arg: bool) -> u32 {
        let kind = self.objects[i].ship.as_ref().map_or(0xff, |s| s.kind);
        if cands.len() < 2 || kind > 1 || self.gather_zones[z].class == Resource::Leviathan {
            return self.zone_one(z, i, cands, port_arg);
        }
        let mut recs: Vec<Rec> = cands
            .iter()
            .map(|c| Rec {
                id: c.id,
                cell: c.cell,
                port: 0,
                pscore: 0,
                amount: c.amount,
            })
            .collect();
        if self.zone_rate(z, &mut recs) < 1 {
            return 0;
        }
        let w: Vec<i32> = recs.iter().map(|r| r.pscore + r.amount).collect();
        let total: u32 = w.iter().fold(0u32, |a, &x| a.wrapping_add(x as u32));
        let mut best: i32 = -1;
        if total != 0 {
            let (mut bw, mut hit_w, mut hit_i) = (-1, -1, 0);
            for _ in 0..3 {
                let r = self.navy_draw() % total;
                let mut cum = 0i32;
                for (k, &x) in w.iter().enumerate() {
                    cum += x;
                    if (r as i32) < cum {
                        hit_w = x;
                        hit_i = k;
                        break;
                    }
                }
                if bw < hit_w {
                    best = hit_i as i32;
                    bw = hit_w;
                }
            }
        }
        if best < 0 {
            return 0;
        }
        let rec = recs[best as usize];
        let id = self.objects[i].id;
        if self.room(i) == 0 {
            self.zone_send_port(id, rec.port);
        } else {
            self.zone_send_gather(id, rec.id, if port_arg { rec.port } else { 0 });
        }
        0
    }

    /// 0x4c7fc0: each deposit's port and its score. The zone's ports
    /// (filled or pruned; more than 100 gives 0): each standing on its
    /// cell with its berth water (0x4999e0) is flooded from the berth
    /// over the shallow ships' grid (0x558fa0, limit 5000: + 2 a straight
    /// step, + 3 a slanting one) and has its load (0x4c7ea0). A deposit
    /// still in its cell's slot: one port, it with score 1; more, for
    /// each port with a load: an amber's cell becomes its approach on
    /// that port's flood (0x4c76f0; none: port and score 0 and the next
    /// port), `s = (B[cell] - B[berth]) x 10 + 20L (L < 3), 200L (3, 4),
    /// 100L^2`; the least and most `s` over all deposits and ports (set
    /// at the first deposit's first port only); with `T = (most + least)
    /// x m - sum > 0`, three draws `% T` walk the ports adding `most - s +
    /// least` (0 for `s < 1`) until past the draw (none: port 0), keeping
    /// the greatest value (strict, from 0). Returns the deposits matched.
    /// The four arrays (loads, port objects, berths, scores) are taken
    /// from the stack (0x5878f0) and only the loads set (-1): a port's
    /// object is written only when it passes the test, a port's `s` only
    /// when rated. The last entry of each holds what the stack probe
    /// left there, its return address: the last port's object 0x4c8068,
    /// so a failed last port reads the id `[0x4c80b4]` = 0xffff6c85 (no
    /// object), and the last `s` 0x4c8090. The other entries hold what
    /// earlier calls left on the stack (0x4c65c0's or 0x4c6c30's frame
    /// and registers): when the walk or the pick reads one of those, the
    /// outcome is not known and the rating gives up as at its own failure
    /// (0: 0x4c8f20 then sends nothing). (The deposits' ship counts
    /// 0x4c7a60, which 0x4c8f20 never reads, are left out.)
    #[allow(clippy::needless_range_loop)]
    pub(super) fn zone_rate(&mut self, z: usize, recs: &mut [Rec]) -> i32 {
        let ports = self.zone_port_list(z);
        let np = ports.len();
        if np > 100 {
            return 0;
        }
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let mut load = vec![-1i32; np];
        // (The port objects' ids: `None` for what the stack holds.)
        let mut pid: Vec<Option<u32>> = vec![None; np];
        if np > 0 {
            pid[np - 1] = Some(PROBE_PORT_ID);
        }
        let mut berth = vec![(0, 0); np];
        let mut flood: Vec<Vec<i16>> = vec![Vec::new(); np];
        let shallow: Vec<i16> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                if self.nav.passable(x, y, false) {
                    0
                } else {
                    -1
                }
            })
            .collect();
        for (u, &id) in ports.iter().enumerate() {
            let Some(p) = self.object_index(id) else {
                continue;
            };
            if self.objects[p].class != ObjectClass::Building(crate::economy::PORT) {
                continue;
            }
            let Some(b) = self.berth_cell(p) else {
                continue;
            };
            if !self.sea_cell(b, false) {
                continue;
            }
            pid[u] = Some(id);
            berth[u] = b;
            let mut g = shallow.clone();
            walk_path::wave(&mut g, w, (0, 0, w - 1, h - 1), b, (-1, -1), 5000, -1, 0, 0);
            flood[u] = g;
            load[u] = self.port_load(z, id);
        }
        let at = |g: &[i16], c: (i32, i32)| g[(c.1 * w + c.0) as usize] as i32;
        let mut s: Vec<Option<i32>> = vec![None; np];
        if np > 0 {
            s[np - 1] = Some(PROBE_SCORE);
        }
        let (mut least, mut most) = (0i32, 0i32);
        let mut matched = 0;
        for r in 0..recs.len() {
            let Some(d) = self.layer_deposit(recs[r].cell) else {
                continue;
            };
            if self.objects[d].id != recs[r].id {
                continue;
            }
            if np == 1 {
                recs[r].port = pid[0].unwrap_or(PROBE_PORT_ID);
                recs[r].pscore = 1;
            } else {
                let amber = self.objects[d].class == ObjectClass::Amber;
                let (mut m, mut sum) = (0, 0);
                for u in 0..np {
                    if load[u] == -1 {
                        continue;
                    }
                    let c = if amber {
                        match self.approach_cell(&flood[u], recs[r].cell) {
                            Some(c) => {
                                recs[r].cell = c;
                                c
                            }
                            None => {
                                recs[r].port = 0;
                                recs[r].pscore = 0;
                                continue;
                            }
                        }
                    } else {
                        recs[r].cell
                    };
                    m += 1;
                    let l = load[u];
                    let pen = match l {
                        ..=2 => l * 20,
                        3..=4 => l * 200,
                        _ => l * l * 100,
                    };
                    let v = (at(&flood[u], c) - at(&flood[u], berth[u])) * 10 + pen;
                    s[u] = Some(v);
                    if r == 0 && u == 0 {
                        least = v;
                        most = v;
                    } else {
                        least = least.min(v);
                        most = most.max(v);
                    }
                    sum += v;
                }
                let t = (most + least) * m - sum;
                if m > 0 && t > 0 {
                    let (mut best, mut bi) = (0, 0);
                    for _ in 0..3 {
                        let draw = (self.navy_draw() % t as u32) as i32;
                        let mut acc = 0;
                        let mut sel = 0;
                        for (u, &x) in s.iter().enumerate() {
                            let Some(x) = x else {
                                return 0;
                            };
                            acc += if x < 1 { 0 } else { most - x + least };
                            if acc > draw {
                                sel = u;
                                break;
                            }
                        }
                        let Some(x) = s[sel] else {
                            return 0;
                        };
                        let v = most - x + least;
                        if best < v {
                            best = v;
                            bi = sel;
                        }
                    }
                    let Some(port) = pid[bi] else {
                        return 0;
                    };
                    recs[r].port = port;
                    recs[r].pscore = best;
                }
            }
            matched += 1;
        }
        matched
    }

    /// 0x4c76f0(flood, cell): of the cell's east, west, south and north
    /// neighbours (in that order, each on the map) that are water
    /// (0x499990) with a value above 0, the least (strict; north returns
    /// at once when less).
    fn approach_cell(&self, f: &[i16], (x, y): (i32, i32)) -> Option<(i32, i32)> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let v = |c: (i32, i32)| f[(c.1 * w + c.0) as usize];
        let mut best: Option<(i32, i32)> = None;
        let take = |c: (i32, i32), best: &mut Option<(i32, i32)>| {
            if self.sea_cell(c, false) && v(c) > 0 && best.is_none_or(|b| v(c) < v(b)) {
                *best = Some(c);
                return true;
            }
            false
        };
        if x + 1 < w {
            take((x + 1, y), &mut best);
        }
        if x > 0 {
            take((x - 1, y), &mut best);
        }
        if y + 1 < h {
            take((x, y + 1), &mut best);
        }
        if y > 0 && take((x, y - 1), &mut best) {
            return best;
        }
        best
    }

    /// 0x4c9270: the candidate the wavefront from the ship (0x4a1e90,
    /// limit 5000) gives the least value at (an amber's approach on it,
    /// 0x4c76f0; the first on a tie, 0x4c5ec0). Amber or fish: a ship not
    /// a merchant follows a merchant of the group drawn with the navy's
    /// generator (0x4d8b30 with 2, 0x4c6100); a merchant, with
    /// `port_arg` and a port for it (0x4c6640), command 0x1f there if
    /// full, else the gather command with it; without, full to the port
    /// with the most ships bound (0x4c6640 with no ship; none: nothing),
    /// else the gather command with port 0. A leviathan: the port with
    /// the most bound; none, the gather command with 0; full or a
    /// leviathan aboard, 0x1f; else the gather command with it. Returns
    /// 0x1f, 0x19 or 0.
    pub(super) fn zone_one(&mut self, z: usize, i: usize, cands: &[Cand], port_arg: bool) -> u32 {
        if cands.is_empty() {
            return 0;
        }
        let t = TILE_SIZE as f32;
        let Some(s) = self.objects[i].ship.as_ref() else {
            return 0;
        };
        let (kind, deep) = (s.kind, deep_draught(s.kind));
        let at = (
            (self.objects[i].x / t).floor() as i32,
            (self.objects[i].y / t).floor() as i32,
        );
        let Some(a) = self.nav.wavefront(at, deep, 5000) else {
            return 0;
        };
        let w = self.nav.width as i32;
        let amber = self.gather_zones[z].class == Resource::Amber;
        // An amber with no approach (0x4c76f0 gives -1) makes 0x4c5ec0
        // read the word before the grid, the end of the heap's header of
        // the block 0x4a1e90 took (0x557770 -> malloc): not the game's,
        // not known. With one candidate it is never compared (the pick is
        // 0); with more the pick is not known, and 0x4c9270 is left as at
        // its own failure (nothing sent, 0).
        let value = |sc: &Scene, c: (i32, i32)| -> Option<i32> {
            let c = if amber { sc.approach_cell(&a, c)? } else { c };
            Some(a[(c.1 * w + c.0) as usize] as i32)
        };
        let mut pick = 0;
        if cands.len() > 1 {
            let Some(mut least) = value(self, cands[0].cell) else {
                return 0;
            };
            for (k, c) in cands.iter().enumerate().skip(1) {
                let Some(v) = value(self, c.cell) else {
                    return 0;
                };
                if v < least {
                    least = v;
                    pick = k;
                }
            }
        }
        let Some(d) = self.layer_deposit(cands[pick].cell) else {
            return 0;
        };
        let Some(class) = deposit_class(&self.objects[d]) else {
            return 0;
        };
        let (did, id) = (self.objects[d].id, self.objects[i].id);
        if class != Resource::Leviathan {
            if kind > 1 {
                let (merchants, _) = self.zone_split(z);
                if !merchants.is_empty() {
                    let r = self.navy_draw() as usize % merchants.len();
                    self.zone_follow(z, id, merchants[r]);
                }
                return 0;
            }
            let p = if port_arg {
                self.zone_port_pick(z, Some(i))
            } else {
                None
            };
            match p {
                None => {
                    if self.room(i) == 0 {
                        match self.zone_port_pick(z, None) {
                            Some(p) => {
                                self.zone_send_port(id, p);
                                0x1f
                            }
                            None => 0,
                        }
                    } else {
                        self.zone_send_gather(id, did, 0);
                        0x19
                    }
                }
                Some(p) => {
                    if self.room(i) == 0 {
                        self.zone_send_port(id, p);
                        0x1f
                    } else {
                        self.zone_send_gather(id, did, p);
                        0x19
                    }
                }
            }
        } else {
            let s = self.objects[i].ship.as_ref().unwrap();
            let lev = s.cargo_kind == Some(Resource::Leviathan) && s.cargo > 0.0;
            match self.zone_port_pick(z, None) {
                None => {
                    self.zone_send_gather(id, did, 0);
                    0x19
                }
                Some(p) if self.room(i) == 0 || lev => {
                    self.zone_send_port(id, p);
                    0x1f
                }
                Some(p) => {
                    self.zone_send_gather(id, did, p);
                    0x19
                }
            }
        }
    }

    /// 0x4c89b0 with code 0x1f: command 0x1f `{port, 1, 0x5a}` through
    /// the outbox.
    fn zone_send_port(&mut self, ship: u32, port: u32) {
        self.post(Order::Unload { ship, port });
    }

    /// 0x4c89b0 with a deposit: the gather command of its class (0x1a
    /// amber, 0x19 fish, 0x1d leviathan) `{deposit, port, 0x5a}` through
    /// the outbox.
    fn zone_send_gather(&mut self, ship: u32, deposit: u32, port: u32) {
        self.post(Order::Gather {
            ship,
            deposit,
            port,
        });
    }

    /// 0x4c7ea0: the player's merchants (types 0 and 1) in its gather
    /// zones bound to port `id` (`+0x7ad`).
    fn port_load(&self, z: usize, id: u32) -> i32 {
        let owner = self.gather_zones[z].owner;
        self.gather_zones
            .iter()
            .filter(|g| g.owner == owner)
            .flat_map(|g| g.ships.iter())
            .filter(|&&s| {
                self.object_index(s).is_some_and(|i| {
                    self.objects[i]
                        .ship
                        .as_ref()
                        .is_some_and(|s| s.kind < 2 && s.home_port == id)
                })
            })
            .count() as i32
    }

    /// The zone's port list (block `+0x28`): filled when there is none or
    /// it is empty (0x4c6c30: over the map's cells in order, each of the
    /// player's ports not asleep, `+0xa4 != 0x10`, by the first cell
    /// found; for player 16 its class-0x40 structures), then pruned when
    /// there is one (0x4c65c0, last to first: what is not a port, or
    /// whose vt+0xb4 0x40e020 fails: no facing, or a state `+0xa4` other
    /// than 1 standing, being built 0, mended 2, asleep 0x10). (Not
    /// identified: the flag `+0x1d8 & 0x10` with `+0x1c8 == 0` that
    /// 0x40e020 also refuses.)
    fn zone_port_list(&mut self, z: usize) -> Vec<u32> {
        use crate::economy::PORT;
        let owner = self.gather_zones[z].owner;
        let filled = self.gather_zones[z].ports.clone().filter(|l| !l.is_empty());
        let list = filled.unwrap_or_else(|| {
            let barbarian = owner == Some((4, 0));
            let mut found: Vec<((i32, i32), u32)> = self
                .objects
                .iter()
                .filter(|o| {
                    o.owner == owner
                        && owner.is_some()
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
        self.gather_zones[z].ports = Some(list.clone());
        list
    }

    /// The zone's port (0x4c6640). With no ship: its only port, or the
    /// one with the most ships bound (0x4c7ea0; the first on a tie). For
    /// ship `i`: its only port, reachable or not; of more, by the
    /// wavefront 0x4a1e90 from the ship (limit 5000, over its draught's
    /// grid) at each berth, the reached ones ranked (at most 9, a port
    /// before those with a lower value), their values less the least (at
    /// least 1); of those within 70% of the first's (`s * 100 / (s0 +
    /// 1)` at least 70), the one with the fewest bound, the first on a tie (the
    /// comparison taking the loads in list order, as 0x4c69c6 reads
    /// them); if its berth is crowded (0x49a330: water about it `a > 0`,
    /// unloading merchants waiting there `b > 0`, `a - b < 2`) the next
    /// ranked with a value and fewer bound. None reached: the list's
    /// first. A berth is 0x4c6520's: the player's port at the entry, its
    /// berth (vt+0xb8), else -1. The least starts at the list's first
    /// berth's value: when that is -1 the original reads the word before
    /// the wavefront (a heap header, not known), so with any port reached
    /// the pick is not known and none is given, as at 0x4c6640's own
    /// failure.
    pub(super) fn zone_port_pick(&mut self, z: usize, ship: Option<usize>) -> Option<u32> {
        let list = self.zone_port_list(z);
        if list.is_empty() {
            return None;
        }
        let load: Vec<i32> = if list.len() >= 2 {
            list.iter().map(|&p| self.port_load(z, p)).collect()
        } else {
            Vec::new()
        };
        let Some(i) = ship else {
            if list.len() < 2 {
                return Some(list[0]);
            }
            let mut k = 0;
            for j in 1..list.len() {
                if load[k] < load[j] {
                    k = j;
                }
            }
            return Some(list[k]);
        };
        if list.len() < 2 {
            return Some(list[0]);
        }
        let t = TILE_SIZE as f32;
        let s = self.objects[i].ship.as_ref()?;
        let deep = deep_draught(s.kind);
        let at = (
            (self.objects[i].x / t).floor() as i32,
            (self.objects[i].y / t).floor() as i32,
        );
        let a = self.nav.wavefront(at, deep, 5000)?;
        let w = self.nav.width as i32;
        let owner = self.gather_zones[z].owner;
        let berth = |sc: &Scene, id: u32| {
            sc.object_index(id)
                .filter(|&p| {
                    let o = &sc.objects[p];
                    o.owner == owner && o.class == ObjectClass::Building(crate::economy::PORT)
                })
                .and_then(|p| sc.berth_cell(p))
                .map(|(x, y)| (y * w + x) as usize)
        };
        let mut least = berth(self, list[0]).map(|b| a[b] as i32);
        // (index in the list, score, load, berth cell)
        let mut ranked: Vec<(usize, i32, i32, usize)> = Vec::new();
        for (k, &id) in list.iter().enumerate() {
            let Some(b) = berth(self, id) else {
                continue;
            };
            let v = a[b] as i32;
            if v < 1 {
                continue;
            }
            least = least.map(|l| l.min(v));
            let e = (k, v + 1, load[k], b);
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
        let least = least?;
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
        let b = ranked[pick].3 as i32;
        let (na, nb) = self.berth_crowd((b % w, b / w), self.gather_zones[z].owner);
        if na > 0 && nb > 0 && na - nb < 2 {
            if let Some(j) =
                (pick + 1..ranked.len()).find(|&j| ranked[j].1 > 0 && ranked[j].2 < ranked[pick].2)
            {
                pick = j;
            }
        }
        Some(list[ranked[pick].0])
    }

    /// 0x49a330(berth, player): over the berth's neighbours on the map
    /// (the table 0x49a1d0 picks by the cell's corner or edge, 0x597900
    /// counts 3, 3, 3, 3, 5, 5, 5, 5, 8, offsets 0x499ff0), `a` the water
    /// ones (0x4999e0) and `b` those holding a merchant (type below 2) of
    /// the player with a gathering task (`+0x25d & 0x40`) unloading (sub
    /// `+0x772` 0xc) still on its way in (step `+0x77d` 1). Crowded when
    /// `a - b < 2`.
    pub(super) fn berth_crowd(
        &self,
        (bx, by): (i32, i32),
        owner: Option<(u32, u32)>,
    ) -> (i32, i32) {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        if bx < 0 || by < 0 || bx >= w || by >= h {
            return (0, 0);
        }
        let (mut a, mut b) = (0, 0);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let c = (bx + dx, by + dy);
                if (dx, dy) == (0, 0) || c.0 < 0 || c.1 < 0 || c.0 >= w || c.1 >= h {
                    continue;
                }
                if !self.sea_cell(c, false) {
                    continue;
                }
                a += 1;
                if let Some(k) = self.slot_ship(c) {
                    let o = &self.objects[k];
                    let s = o.ship.as_ref().unwrap();
                    if o.owner == owner
                        && s.kind < 2
                        && matches!(
                            s.task,
                            Some(Task::Gather {
                                phase: GatherPhase::ToPort(_),
                                ..
                            })
                        )
                    {
                        b += 1;
                    }
                }
            }
        }
        (a, b)
    }

    /// The wait by a crowded berth (0x4adef0 step 1, each merchant sailing
    /// in to unload, not yet on its berth `+0x7bd`): the timer `+0x78e`
    /// starts at a distance of 1 (0x557440) from the berth; 300 ticks on,
    /// if the berth is crowded (0x49a330) it steps aside (0x4b4b50 with
    /// 0x5a); past 349, if crowded, it gives its port up (`+0x7ad = 0`)
    /// and posts notice 0x138 with class 0; the timer restarts either
    /// way.
    pub(super) fn run_berth_waits(&mut self) {
        let now = self.ticks;
        let t = TILE_SIZE as f32;
        for k in 0..self.objects.len() {
            let Some(s) = self.objects[k].ship.as_ref() else {
                continue;
            };
            let Some(Task::Gather {
                phase: GatherPhase::ToPort(port),
                ..
            }) = s.task
            else {
                continue;
            };
            let Some(b) = self.object_index(port).and_then(|p| self.berth_cell(p)) else {
                continue;
            };
            let here = (
                (self.objects[k].x / t).floor() as i32,
                (self.objects[k].y / t).floor() as i32,
            );
            if here == b {
                continue;
            }
            let since = s.berth_wait;
            if since == 0 {
                if walk_path::dist(here.0, here.1, b.0, b.1) == 1 {
                    self.objects[k].ship.as_mut().unwrap().berth_wait = now;
                }
                continue;
            }
            let e = now.wrapping_sub(since);
            let owner = self.objects[k].owner;
            if e == BERTH_ASIDE {
                let (a, n) = self.berth_crowd(b, owner);
                if a - n < 2 {
                    if let Some(c) = self.aside_cell(k, 0x5a) {
                        self.go_to_cell(k, c, 0x5a);
                    }
                }
            } else if e >= BERTH_GIVE_UP {
                let (a, n) = self.berth_crowd(b, owner);
                let id = self.objects[k].id;
                if a - n < 2 {
                    self.objects[k].ship.as_mut().unwrap().home_port = 0;
                    self.post_notice(id, 0x138, 0);
                }
                self.objects[k].ship.as_mut().unwrap().berth_wait = 0;
            }
        }
    }

    /// 0x4ca290, the re-plan. Deposits the zone does not know yet or
    /// still to be had (0x4c5b90 mode 1): each ship without a gathering
    /// task (`+0x25d & 0x40`): a merchant to the same deposit (0x4c96e0),
    /// else the weighted pick (0x4c8f20 with port argument 1); another
    /// ship found among the escorts (0x4c61d0) to 0x4c6270 (it never
    /// reaches 0x4c8f20, 0x4c6270's result never below 0). Before that,
    /// a ship with a blow marked (`+0x691`) forgets it at the next tick
    /// (command 0x28 sub 4, 0x4cf760), so its escorts in this pass still
    /// see it. None: each ship not unloading this zone's
    /// class with cargo aboard counts laden and, with the zone's flag bit
    /// 0 and a port for it, is sent to unload (0x1f); ships unloading
    /// count working; with none laden or working the zone is done
    /// (`+0x5d`) for a human's (side 1 or 2) when `+0x24 >> 16 != 1`.
    /// (Not ported: the route `+0x61`; the blow's report to the brain,
    /// message 0x5d9c (0x4a21c0, whose header words `[0]`, `[1]` are left
    /// unwritten on the stack): the port reports blows as they land
    /// (`fleet_attacked`).) After the ships, each wreck's cell (`+0x35`)
    /// goes to every ship as command 0x29 sub 2 (its path memory), every
    /// re-plan.
    fn zone_replan(&mut self, z: usize) -> i32 {
        let cands = self.zone_deposits(z, 1);
        let ships = self.gather_zones[z].ships.clone();
        let mut sent = 0;
        if !cands.is_empty() {
            for id in ships {
                let Some(i) = self.object_index(id) else {
                    continue;
                };
                if self.objects[i].ship.as_ref().unwrap().blow.is_some() {
                    self.post(Order::ClearBlow { ship: id });
                }
                let s = self.objects[i].ship.as_ref().unwrap();
                if matches!(s.task, Some(Task::Gather { .. })) {
                    continue;
                }
                if s.kind < 2 {
                    sent = self.zone_same_deposit(z, i, false);
                    if sent < 1 && self.zone_weighted(z, i, &cands, true) != 0 {
                        sent += 1;
                    }
                } else if let Some(e) = self.gather_zones[z]
                    .escorts
                    .iter()
                    .position(|e| e.ship == id)
                {
                    self.escort_again(z, e);
                }
            }
            // Each wreck's cell to every ship (command 0x29 `{2, cell,
            // 0xff}`), every re-plan.
            let w = self.nav.width as i32;
            for c in self.gather_zones[z].wrecks.clone() {
                for &ship in &self.gather_zones[z].ships.clone() {
                    self.post(Order::Remember {
                        ship,
                        cell: ((c as i32) % w, (c as i32) / w),
                    });
                }
            }
            return sent;
        }
        let class = self.gather_zones[z].class;
        let (mut laden, mut working) = (0, 0);
        for id in ships {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let s = self.objects[i].ship.as_ref().unwrap();
            let unloading = matches!(
                s.task,
                Some(Task::Gather {
                    phase: GatherPhase::ToPort(_) | GatherPhase::Unloading(_),
                    ..
                })
            );
            if unloading && s.cargo_kind.is_none_or(|k| k == class) {
                working += 1;
                continue;
            }
            if s.cargo > 0.0 {
                laden += 1;
                if !unloading && self.gather_zones[z].flags & 1 != 0 {
                    if let Some(p) = self.zone_port_pick(z, Some(i)) {
                        self.zone_send_port(id, p);
                        working += 1;
                    }
                }
            }
        }
        let owner = self.gather_zones[z].owner;
        if laden == 0
            && working == 0
            && !self.side_computer(owner)
            && owner != Some((4, 0))
            && self.gather_zones[z].flags >> 16 != 1
        {
            self.gather_zones[z].done = true;
        }
        0
    }

    /// 0x4c6270(escort entry): the escort not among the group's ships
    /// (0x4a0370): the entry goes, 0; a merchant, 0; the ward not among
    /// them: 0, the entry kept. After the ward's attacker (state 0xb) and
    /// more than 2 cells (0x557440) from the ward: back to following it
    /// (state 10, target 0, 0x4d8b30 with 2), 1. Else the ward with a blow
    /// marked (`+0x691`): after its giver `+0x692` (state 0xb, 0x4d0140:
    /// command 0x16 `{target, 7, 7, -1}`, the attack on a ship), 1. Else
    /// moving (`+0x58c`): 0; else following again (state 10), 1.
    fn escort_again(&mut self, z: usize, e: usize) -> i32 {
        let Escort {
            ship: o,
            ward,
            state,
            ..
        } = self.gather_zones[z].escorts[e];
        let in_group = |sc: &Scene, id: u32| {
            sc.gather_zones[z]
                .ships
                .contains(&id)
                .then(|| sc.object_index(id))
                .flatten()
                .filter(|&i| sc.objects[i].ship.is_some())
        };
        let Some(i) = in_group(self, o) else {
            self.gather_zones[z].escorts.remove(e);
            return 0;
        };
        if self.objects[i].ship.as_ref().unwrap().kind < 2 {
            return 0;
        }
        let Some(w) = in_group(self, ward) else {
            return 0;
        };
        let t = TILE_SIZE as f32;
        let cell = |sc: &Scene, k: usize| {
            (
                (sc.objects[k].x / t).floor() as i32,
                (sc.objects[k].y / t).floor() as i32,
            )
        };
        let (a, b) = (cell(self, i), cell(self, w));
        let follow = |sc: &mut Scene| {
            let x = &mut sc.gather_zones[z].escorts[e];
            x.state = 10;
            x.target = 0;
            sc.post(Order::Follow {
                ship: o,
                ward,
                range: 2,
            });
            1
        };
        if state == 0xb && walk_path::dist(a.0, a.1, b.0, b.1) > 2 {
            return follow(self);
        }
        if let Some(by) = self.objects[w].ship.as_ref().unwrap().blow {
            let x = &mut self.gather_zones[z].escorts[e];
            x.state = 0xb;
            x.target = by;
            self.post(Order::Attack {
                ship: o,
                target: by,
            });
            return 1;
        }
        if self.objects[i].ship.as_ref().unwrap().move_state != 0 {
            return 0;
        }
        follow(self)
    }

    /// The gather zone's state for 0x47a570's query 0x79 (0x4c8da0): no
    /// ships, result 1; no merchants (and not a leviathan zone with
    /// ships), result 1; deposits left (0x4c5b90 mode 1, forgetting those
    /// gone): under way (500), but result 0xb with no port list; none
    /// left: result 10. `None` when the group's task is not a zone (the
    /// query answers nothing).
    pub(super) fn zone_status(&mut self, group: ZoneGroup) -> Option<(u32, u32)> {
        let z = self.gather_zones.iter().position(|z| z.group == group)?;
        let ships: Vec<usize> = self.gather_zones[z]
            .ships
            .iter()
            .filter_map(|&id| self.object_index(id))
            .collect();
        if ships.is_empty() {
            return Some((0x1f5, 1));
        }
        let merchants = ships
            .iter()
            .filter(|&&i| self.objects[i].ship.as_ref().is_some_and(|s| s.kind < 2))
            .count();
        if merchants == 0 && self.gather_zones[z].class != Resource::Leviathan {
            return Some((0x1f5, 1));
        }
        if !self.zone_deposits(z, 1).is_empty() {
            if self.gather_zones[z]
                .ports
                .as_ref()
                .is_none_or(|p| p.is_empty())
            {
                return Some((0x1f5, 0xb));
            }
            return Some((500, 0));
        }
        Some((0x1f5, 10))
    }
}

/// A deposit object's resource: amber (0x190), a shoal (0x191), a
/// leviathan (0x192).
pub fn deposit_class(o: &crate::objects::MapObject) -> Option<Resource> {
    match o.class {
        ObjectClass::Amber => Some(Resource::Amber),
        ObjectClass::FishZone => Some(Resource::Fish),
        ObjectClass::WhaleZone => Some(Resource::Leviathan),
        _ => None,
    }
}
