//! The navy's treasure task (fleet order 0xbe, block `{0xbe, 0x79}`,
//! made by 0x4da550, its notices 0x4da0a0, its tick 0x4da6a0, its status
//! 0x4da900) for the computers' flotillas (0x483350) and the brains'
//! fleets (0x47aed0 -> 0x479c90), both through `TreasureGroup`.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;
use serde::{Deserialize, Serialize};

use super::fleet_guard::Flotilla;
use super::{Cells, DONE, PORT};
use crate::scene::{Order, Scene};

/// Fleet order 0xbe's block (`+0x25`, `{0xbe, 0x79}`, 0x4da550): the
/// order's payload (0x483350: the area, kinds -1, the town's ports, the
/// manager's group) and the lists the block builds.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(in crate::scene) struct TreasureBlock {
    /// `+0xc..+0x18` the area (payload 0-3).
    pub area: Cells,
    /// `+0x1c` the kinds taken, a bit each (payload 4).
    pub kinds: u32,
    /// `+0x34`: 4 a treasure used at once (0x142), 5 none in the area;
    /// read by the status 0x4da900.
    pub status: u32,
    /// `+0x28` the ships' records (0x4d9690, 0x20 bytes, by ship id).
    pub ships: Vec<TreasureRec>,
    /// `+0x2c` the targets (0x4d9810, 0x18 bytes, by treasure id).
    pub targets: Vec<TreasureTarget>,
    /// `+0x24` the ports to bring treasures to (0x4d9600, 8 bytes: the
    /// building's id, its cell index), by id.
    #[serde(default)]
    pub ports: Vec<(u32, i32)>,
    /// `+0x30` (payload 8), read by the ports' mode 0 (0x4d9a70 at
    /// 0x4d9b73, 0x49c020): the brains' the first word of member p10 of
    /// the group (0x479c90; the port's fleet index of that member), none
    /// with p10 -1; the computers' the manager's group, read through the
    /// flotilla's bases.
    #[serde(default)]
    pub manager: Option<usize>,
}

/// A ship's record in the treasure block (`+0x28`, 0x4d9690).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub(in crate::scene) struct TreasureRec {
    /// `+4` the ship's id, `+8`, `+0xc` its cell when sent.
    pub ship: u32,
    pub x: i32,
    pub y: i32,
    /// `+0x10` the treasure to take (0 for a plain move), `+0x14` the
    /// target's cell index (`y x W + x`).
    pub target: u32,
    pub cell: i32,
    /// `+0x18`: 1 sent, 2 carrying home, 3 escorting (0x141's).
    pub state: u32,
    /// `+0x1c` the treasure's kind (0 for a move).
    pub kind: u32,
}

/// A target in the treasure block's list (`+0x2c`, 0x4d9810: `+0` the
/// object, `+0x10` -1, unread).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub(in crate::scene) struct TreasureTarget {
    /// `+4` the treasure's id, `+8` its kind (`+0xb0`), `+0xc` its cell
    /// index.
    pub id: u32,
    pub kind: u32,
    pub cell: i32,
}

/// What the treasure block reads of the group holding it: a computer's
/// flotilla (`Flotilla`) or a brain's fleet (`brain::Fleet`).
pub(in crate::scene) trait TreasureGroup {
    /// Group `+0x31`: its ships, by id.
    fn ships(&self) -> &[u32];
    /// Group `+0x25`: the block.
    fn block(&self) -> &Option<TreasureBlock>;
    fn block_mut(&mut self) -> &mut Option<TreasureBlock>;
    /// Group `+0x19`, `+0x1d`, `+0x21`: the behaviour 0x4c4350 gave it
    /// (zeroes when never given).
    fn behaviour(&self) -> (u32, u32, u32);
    /// Group `+8`.
    fn elapsed(&mut self) -> &mut u32;
    /// The group's player (the block's calls' first argument).
    fn owner(&self, scene: &Scene) -> (u32, u32);
    /// The list of the manager's land fleet (block `+0x30`, 0x49c020: a
    /// group of kind 0xb with `+0x29`), its members gone or in state
    /// `+0xa4` 2, 3 or 0xff dropped (0x42c9a0); none when `+0x30` names
    /// no such group.
    fn managers(&mut self, scene: &mut Scene) -> Option<Vec<u32>>;
    /// Whether the group's notices `+0x2d` hold any.
    fn has_notices(&self) -> bool;
    /// Group `+0x2d`: the block's notices `(ship, code, record)`, oldest
    /// first, taken.
    fn take_notices(&mut self) -> Vec<(u32, u32, u32)>;
}

impl TreasureGroup for Flotilla<'_> {
    fn ships(&self) -> &[u32] {
        &self.ai.flotillas[self.f].ships
    }
    fn block(&self) -> &Option<TreasureBlock> {
        &self.ai.flotillas[self.f].treasure
    }
    fn block_mut(&mut self) -> &mut Option<TreasureBlock> {
        &mut self.ai.flotillas[self.f].treasure
    }
    fn behaviour(&self) -> (u32, u32, u32) {
        self.ai.flotillas[self.f].behaviour.unwrap_or((0, 0, 0))
    }
    fn elapsed(&mut self) -> &mut u32 {
        &mut self.ai.flotillas[self.f].elapsed
    }
    fn owner(&self, _: &Scene) -> (u32, u32) {
        self.ai.owner
    }
    /// The manager's land fleet's list: the bases' sites (0x487230), the
    /// gone, fallen and sold left out.
    fn managers(&mut self, scene: &mut Scene) -> Option<Vec<u32>> {
        Some(
            self.ai
                .bases
                .iter()
                .filter_map(|b| b.site)
                .filter(|&s| scene.object_index(s).is_some() && !scene.fallen_or_sold(s))
                .collect(),
        )
    }
    fn has_notices(&self) -> bool {
        !self.ai.flotillas[self.f].joined.is_empty()
    }
    /// The notices posted to the treasure block (the port's `(ship,
    /// block, code, rec)`, those of an older block dropped).
    fn take_notices(&mut self) -> Vec<(u32, u32, u32)> {
        std::mem::take(&mut self.ai.flotillas[self.f].joined)
            .into_iter()
            .filter(|&(_, at, _, _)| at == 0xbe)
            .map(|(id, _, code, rec)| (id, code, rec))
            .collect()
    }
}

/// The treasure block's status (0x4da900, by 0x4a0e20 for `{0xbe,
/// 0x79}`): with no members (`+0x31` count 0) done (0x1f5) with result 1;
/// else done with result 1 when none is in the player's objects, running
/// (500) when one is; then `+0x34` 4 done with result 0xc, 5 done with
/// result 0xd.
pub(in crate::scene) fn treasure_status(
    scene: &Scene,
    ships: &[u32],
    b: Option<&TreasureBlock>,
) -> (u32, u32) {
    if ships.is_empty() {
        return (DONE, 1);
    }
    let mut out = if ships.iter().any(|&id| scene.object_index(id).is_some()) {
        (500, 0)
    } else {
        (DONE, 1)
    };
    match b.map_or(0, |b| b.status) {
        4 => out = (DONE, 0xc),
        5 => out = (DONE, 0xd),
        _ => {}
    }
    out
}

impl Scene {
    /// The treasure block's list of targets (0x4d9920 over `area`): the
    /// treasures on the map's cells row by row (the cell's object, class
    /// 700, `+0xa4` not 5), of the block's kinds (`+0x1c & 1 << (+0xb0 &
    /// 31)`), each put in `+0x2c` by 0x4d9810: the entry of its id updated
    /// in place, else appended; entries are never dropped here. An area
    /// starting off the map, or with no rows on it, lists none. With
    /// `first` the index + 1 of the first listed, else the count. (Not
    /// ported: the treasure's state 5, which the port's treasures never
    /// take; the port's cells hold a treasure each, the first in object
    /// order where two lie on one.)
    pub(in crate::scene) fn treasure_list(
        &self,
        g: &mut dyn TreasureGroup,
        area: Cells,
        first: bool,
    ) -> i32 {
        let (w, h) = self.map_cells();
        let (x0, y0, aw, ah) = area;
        if x0 < 0 || y0 < 0 || x0 >= w || y0 >= h {
            return 0;
        }
        let (x1, y1) = ((x0 + aw).min(w), (y0 + ah).min(h));
        let mut cells: Vec<(i32, i32, usize)> = (0..self.objects.len())
            .filter(|&k| self.objects[k].class == ObjectClass::Treasure)
            .map(|k| {
                let (cx, cy) = self.cell_of(k);
                (cy, cx, k)
            })
            .filter(|&(cy, cx, _)| cx >= x0 && cx < x1 && cy >= y0 && cy < y1)
            .collect();
        cells.sort_by_key(|&(cy, cx, _)| (cy, cx));
        cells.dedup_by_key(|c| (c.0, c.1));
        let b = g.block_mut().get_or_insert_with(Default::default);
        let mut n = 0;
        for (cy, cx, k) in cells {
            let kind = self.objects[k].treasure.map_or(0, |t| u32::from(t.kind));
            if b.kinds & 1 << (kind & 31) == 0 {
                continue;
            }
            let e = TreasureTarget {
                id: self.objects[k].id,
                kind,
                cell: cy * w + cx,
            };
            let at = match b.targets.iter().position(|t| t.id == e.id) {
                Some(p) => {
                    b.targets[p] = e;
                    p
                }
                None => {
                    b.targets.push(e);
                    b.targets.len() - 1
                }
            };
            if first {
                return at as i32 + 1;
            }
            n += 1;
        }
        n
    }

    /// The treasure block's send (0x4da3e0): every ship of the member
    /// (class 200), in its order, after target `t`. One that can take a
    /// treasure (0x4e6700 command 0x22 kind 0x10: capability `+0x261 &
    /// 0x8000`, which every finished ship has, 0x4e6930, but one being
    /// built, sold or holding its commands; and room, 0x4be800 mode 6 or
    /// 7) with room aboard (mode 6: 0x597afc[type] less the treasures
    /// aboard `+0x482`; none on types 10 and 11) is recorded in `+0x28`
    /// (0x4d9690: its cell, the target, its cell, state 1, the kind) and
    /// sent to take it (0x4d9f50: command 0x22 kind 0x10 {cell x, cell y,
    /// the treasure}, priority 0x5a, which the ship's 0x4e2ba0 turns into
    /// event 0x40 {700, 10, the treasure, 0x5a}, as the player's take,
    /// 0x49e0e0 giving it priority 0; the port's take refused on a
    /// treasure gone); any other is recorded with target 0 and kind 0 and
    /// given a plain move to the target's cell (0x4cf070, command 0x14).
    /// The ships sent.
    pub(in crate::scene) fn treasure_send(
        &mut self,
        g: &mut dyn TreasureGroup,
        t: TreasureTarget,
    ) -> i32 {
        let (w, _) = self.map_cells();
        let mut n = 0;
        for id in g.ships().to_vec() {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            if self.objects[i].ship.is_none() {
                continue;
            }
            let held = self.being_built(id)
                || self.being_sold(id)
                || self.hunt_holds(id)
                || self.landing_holds(id)
                || self.load_holds(id);
            let room = self.objects[i].ship.as_ref().is_some_and(|s| {
                crate::scene::chest::SINK_TREASURES
                    .get(s.kind as usize)
                    .is_some_and(|&n| usize::from(n) > s.treasures.len())
            });
            let take = !held && room;
            let (x, y) = self.cell_of(i);
            let rec = TreasureRec {
                ship: id,
                x,
                y,
                target: if take { t.id } else { 0 },
                cell: t.cell,
                state: 1,
                kind: if take { t.kind } else { 0 },
            };
            let b = g.block_mut().get_or_insert_with(Default::default);
            match b.ships.iter().position(|r| r.ship == id) {
                Some(p) => b.ships[p] = rec,
                None => b.ships.push(rec),
            }
            if take {
                let _ = self.order_take(id, t.id);
            } else {
                // 0x4cf070: a move to the target's cell, priority 0.
                let s = TILE_SIZE as f32;
                let (cx, cy) = (t.cell % w, t.cell / w);
                self.post(Order::Move {
                    ship: id,
                    x: cx as f32 * s + s / 2.0,
                    y: cy as f32 * s + s / 2.0,
                });
            }
            n += 1;
        }
        n
    }

    /// Fleet order 0xbe (0x4da550, block `{0xbe, 0x79}`): a block of
    /// another kind is ended (0x4a1c60, `new_block`), one of this kind
    /// kept with its lists (0x4d9e90 makes one zeroed); the area `+0xc`,
    /// the kinds `+0x1c` (payload 4), the manager `+0x30` (payload 8) and
    /// the status `+0x34` 0 set. Then the area's list (0x4d9920): none,
    /// status 5 (the order done); else every ship is sent after the
    /// list's first entry (0x4da3e0), the same for all. Before the list,
    /// the ports `+0x24` (0x4d9a70 mode 1) of the payload's (payload 6,
    /// 7). The computers' payload (0x483350): kinds -1, every kind; the
    /// flotilla's base town's ports (0x43d220 / 0x440c60 with 0x39, as
    /// the gather zone's, `flotilla_ports`). The brains' (0x479c90): the
    /// call's kinds, no ports (count 0, none listed), the manager the
    /// first word of a member of the group (`TreasureBlock::manager`).
    pub(in crate::scene) fn treasure_order(
        &mut self,
        g: &mut dyn TreasureGroup,
        area: Cells,
        kinds: u32,
        ports: &[u32],
        manager: Option<usize>,
    ) {
        let b = g.block_mut().get_or_insert_with(Default::default);
        b.area = area;
        b.kinds = kinds;
        b.manager = manager;
        b.status = 0;
        self.treasure_ports(g, Some(ports));
        if self.treasure_list(g, area, false) < 1 {
            g.block_mut().as_mut().unwrap().status = 5;
            return;
        }
        if let Some(&t) = g.block().as_ref().unwrap().targets.first() {
            self.treasure_send(g, t);
        }
    }

    /// 0x4d9690: ship `r.ship`'s record in the treasure block `+0x28`,
    /// that of its id rewritten whole, else appended.
    pub(in crate::scene) fn treasure_record(g: &mut dyn TreasureGroup, r: TreasureRec) {
        let b = g.block_mut().get_or_insert_with(Default::default);
        match b.ships.iter().position(|e| e.ship == r.ship) {
            Some(p) => b.ships[p] = r,
            None => b.ships.push(r),
        }
    }

    /// The treasure block's ports `+0x24` (0x4d9a70; each by 0x4d9600:
    /// `{id, cell}`, the cell of that id rewritten, else appended). With
    /// `given` (mode 1, the order's payload), those of them on the map
    /// (vt+0x28, the building's cell): one of player 16's (`+0x54` 4,
    /// `+0x55` 0) of class 0x40, another's of class 0x39 not asleep
    /// (vt+0x2c, 0x40dfc0), of whatever player; nothing more. Else (mode 0)
    /// the ports of the towns of the manager's land fleet (block `+0x30`,
    /// the payload's manager group, of kind 0xb with `+0x29`: 0x42cc20 ->
    /// 0x42c9a0 with 0x39, player 16 0x42c9a0 with 0x40: the land fleet's
    /// members not fallen, sold or 0xff, `TreasureGroup::managers`; for
    /// 0x39 each settlement or town's buildings of the class in its list
    /// `+0x1d0`, for 0x40 the members of the class themselves, by their
    /// cell `+0x74`, `+0x78`);
    /// none there, every map cell's object (row by row): player 16's of
    /// class 0x40, or a port of the player not asleep. The ports listed.
    /// (Not decoded, as the dock block's: which cells of a building hold
    /// it in their slot, taken as its anchor.)
    pub(in crate::scene) fn treasure_ports(
        &mut self,
        g: &mut dyn TreasureGroup,
        given: Option<&[u32]>,
    ) -> i32 {
        use crate::economy::{SETTLEMENT, TOWN};
        let w = self.nav.width as i32;
        let owner = g.owner(self);
        let class = if owner == (4, 0) {
            ObjectClass::Barbarian(0x40)
        } else {
            ObjectClass::Building(PORT)
        };
        let mut found: Vec<(u32, i32)> = Vec::new();
        if let Some(ids) = given {
            for &id in ids {
                let Some(k) = self.object_index(id) else {
                    continue;
                };
                let o = &self.objects[k];
                let Some((x, y)) = o.cell else {
                    continue;
                };
                let takes = if o.owner == Some((4, 0)) {
                    o.class == ObjectClass::Barbarian(0x40)
                } else {
                    o.class == ObjectClass::Building(PORT) && o.asleep.is_none()
                };
                if takes {
                    found.push((id, y * w + x));
                }
            }
            return Self::treasure_put_ports(g, found);
        }
        for site in g.managers(self).unwrap_or_default() {
            let Some(t) = self.object_index(site) else {
                continue;
            };
            // 0x42c9a0's class bit (1 << (class - 0x32)): 0x40's (0x4000,
            // in 0xe000) takes the member itself; 0x39's a town's
            // (type 0x1491) buildings of the class (0x43d220, 0x440c60).
            if owner == (4, 0) {
                let o = &self.objects[t];
                if let (true, Some((x, y))) = (o.class == class, o.cell) {
                    found.push((o.id, y * w + x));
                }
                continue;
            }
            if !matches!(
                self.objects[t].class,
                ObjectClass::Building(SETTLEMENT | TOWN)
            ) {
                continue;
            }
            for k in self.town_buildings(site) {
                let o = &self.objects[k];
                if let (true, Some((x, y))) = (o.class == class, o.cell) {
                    found.push((o.id, y * w + x));
                }
            }
        }
        if !found.is_empty() {
            return Self::treasure_put_ports(g, found);
        }
        for o in &self.objects {
            let takes = if o.owner == Some((4, 0)) {
                o.class == ObjectClass::Barbarian(0x40)
            } else {
                o.class == ObjectClass::Building(PORT)
                    && o.owner == Some(owner)
                    && o.asleep.is_none()
            };
            if let (true, Some((x, y))) = (takes, o.cell) {
                found.push((o.id, y * w + x));
            }
        }
        found.sort_by_key(|p| p.1);
        Self::treasure_put_ports(g, found)
    }

    /// 0x4d9600 for each of `found`; the count.
    pub(in crate::scene) fn treasure_put_ports(
        g: &mut dyn TreasureGroup,
        found: Vec<(u32, i32)>,
    ) -> i32 {
        let b = g.block_mut().get_or_insert_with(Default::default);
        let n = found.len() as i32;
        for (id, cell) in found {
            match b.ports.iter_mut().find(|p| p.0 == id) {
                Some(p) => p.1 = cell,
                None => b.ports.push((id, cell)),
            }
        }
        n
    }

    /// 0x4d9cf0: the ports gone from their cell taken off `+0x24` (from
    /// the last): kept, the cell's object of that id, for player 16 its
    /// own of class 0x40, else a port (0x39) of the player not asleep
    /// (vt+0x2c).
    pub(in crate::scene) fn treasure_ports_check(&self, g: &mut dyn TreasureGroup) {
        let w = self.nav.width as i32;
        let owner = g.owner(self);
        let Some(b) = g.block_mut().as_mut() else {
            return;
        };
        b.ports.retain(|&(id, cell)| {
            self.object_index(id).is_some_and(|k| {
                let o = &self.objects[k];
                o.cell.is_some_and(|(x, y)| y * w + x == cell)
                    && o.owner == Some(owner)
                    && if owner == (4, 0) {
                        o.class == ObjectClass::Barbarian(0x40)
                    } else {
                        o.class == ObjectClass::Building(PORT) && o.asleep.is_none()
                    }
            })
        });
    }

    /// 0x4d9db0: of `ports`, the one with the least `W[ship] - W[cell]`
    /// of the wavefront 0x4a1e90 (the ship's draught `+0x592`, from its
    /// cell, limit 5000) at its cell (the building's, not its berth), the
    /// first on a tie (strict `<` from 100000); 0 none.
    pub(in crate::scene) fn treasure_port_pick(&self, ports: &[(u32, i32)], i: usize) -> u32 {
        let Some(s) = self.objects[i].ship.as_ref() else {
            return 0;
        };
        let deep = crate::objects::deep_draught(s.kind);
        let (x, y) = self.ship_cell(i);
        let Some(map) = self.nav.wavefront((x, y), deep, 5000) else {
            return 0;
        };
        let w = self.nav.width as i32;
        let start = map.get((y * w + x) as usize).copied().unwrap_or(0) as i32;
        let (mut best, mut pick) = (100_000, 0);
        for &(id, cell) in ports {
            let d = start - map.get(cell as usize).copied().unwrap_or(0) as i32;
            if d < best {
                best = d;
                pick = id;
            }
        }
        pick
    }

    /// The trip home of ship `i` with a treasure aboard (0x4da0a0's 0x141,
    /// 0x4da6a0): the ports listed again (0x4d9a70 mode 0), the gone taken
    /// off (0x4d9cf0), the nearest (0x4d9db0); with one, the ship recorded
    /// in state 2 with it (cell -1) and sent command 0x1f `{port, 1, 0x5a}`
    /// (0x4d9fa0 -> 0x520cc0: the unload at the port, whose end hands the
    /// treasures over, 0x4ad9f0). Whether it was sent.
    pub(in crate::scene) fn treasure_home(&mut self, g: &mut dyn TreasureGroup, i: usize) -> bool {
        self.treasure_ports(g, None);
        self.treasure_ports_check(g);
        let ports = g
            .block()
            .as_ref()
            .map(|b| b.ports.clone())
            .unwrap_or_default();
        let port = self.treasure_port_pick(&ports, i);
        if port == 0 {
            return false;
        }
        let ship = self.objects[i].id;
        Self::treasure_record(
            g,
            TreasureRec {
                ship,
                x: -1,
                y: -1,
                target: port,
                cell: -1,
                state: 2,
                kind: 0,
            },
        );
        self.post(Order::Unload {
            ship,
            port,
            prio: 0x5a,
        });
        true
    }

    /// Ship `id` escorts carrier `ward` (0x4d9690 state 3, target the
    /// carrier, at `(x, y)`; message `*` 0x4d8b30 with 1 and priority 0:
    /// keep within a cell of it).
    pub(in crate::scene) fn treasure_escort(
        &mut self,
        g: &mut dyn TreasureGroup,
        id: u32,
        ward: u32,
        at: (i32, i32),
    ) {
        Self::treasure_record(
            g,
            TreasureRec {
                ship: id,
                x: at.0,
                y: at.1,
                target: ward,
                cell: -1,
                state: 3,
                kind: 0,
            },
        );
        self.post(Order::Follow {
            ship: id,
            ward,
            range: 1,
        });
    }

    /// 0x4d9fe0: every record (as many as there were) of a ship other than
    /// carrier `i` and not in state 2 escorts it, at the carrier's cell
    /// (vt+0x28). The ships sent.
    pub(in crate::scene) fn treasure_escorts(
        &mut self,
        g: &mut dyn TreasureGroup,
        i: usize,
    ) -> i32 {
        let ward = self.objects[i].id;
        let at = self.ship_cell(i);
        let recs = g
            .block()
            .as_ref()
            .map(|b| b.ships.clone())
            .unwrap_or_default();
        let mut n = 0;
        for r in recs {
            if r.ship != ward && r.state != 2 {
                self.treasure_escort(g, r.ship, ward, at);
                n += 1;
            }
        }
        n
    }

    /// The treasure block's notices (0x4da0a0, from the last posted); the
    /// ships it sent. 300, a ship joined and still in the group (0x4cc590):
    /// with a record of state 2 (0x4d97c0, the first), the joiner escorts
    /// its ship (cell -1); else, with the records `+0x28` not empty, the
    /// area listed again (0x4d9920) and, with any, every ship sent after
    /// the list's first entry (0x4da3e0); then `#` sub 1 with the group's
    /// three (0x4c42f0). 0x137, a ship left: its record dropped (0x4d9760);
    /// one of state 2, the list about its cell (0x4a1770's record: the
    /// rect `{x - 2, y - 2, x + 2, y + 2}` read as x, y, w, h) with
    /// `first`: none, status 5; else every ship sent after that entry.
    /// 0x141, a treasure put aboard by a ship of the group (0x4a1a60 from
    /// 0x4a5d26: rec[5] 0, rec[7] 1, so 0x4d98c0 drops the target of id
    /// 0, none): the trip home (`treasure_home`) and, sent, the others
    /// escorting it (0x4d9fe0). 0x142, a treasure used at once by a ship
    /// of the group (0x4a1af0): status 4, the order done. (Not ported: the
    /// 0x137 of a carrier gone from the objects, whose cell the port no
    /// longer has: its record is dropped, no list made.)
    pub(in crate::scene) fn treasure_notices(&mut self, g: &mut dyn TreasureGroup) -> i32 {
        let b = g.behaviour();
        let mut sent = 0;
        let mut heard = g.take_notices();
        while let Some((id, code, _)) = heard.pop() {
            match code {
                300 if g.ships().contains(&id) => {
                    let carrier = g
                        .block()
                        .as_ref()
                        .and_then(|t| t.ships.iter().find(|r| r.state == 2))
                        .map(|r| r.ship);
                    if let Some(ward) = carrier {
                        self.treasure_escort(g, id, ward, (-1, -1));
                        sent += 1;
                    } else {
                        let area = g.block().as_ref().map(|t| t.area);
                        let any = g.block().as_ref().is_some_and(|t| !t.ships.is_empty());
                        if let (Some(area), true) = (area, any) {
                            if self.treasure_list(g, area, false) > 0 {
                                let t = g.block().as_ref().unwrap().targets.first();
                                if let Some(&t) = t {
                                    sent = self.treasure_send(g, t);
                                }
                            }
                        }
                    }
                    self.send_behaviour(id, b);
                }
                0x137 => {
                    let Some(t) = g.block_mut().as_mut() else {
                        continue;
                    };
                    let Some(p) = t.ships.iter().position(|r| r.ship == id) else {
                        continue;
                    };
                    if t.ships.remove(p).state != 2 {
                        continue;
                    }
                    let Some(k) = self.object_index(id) else {
                        continue;
                    };
                    let (x, y) = self.ship_cell(k);
                    let n = self.treasure_list(g, (x - 2, y - 2, x + 2, y + 2), true);
                    let t = g.block_mut().as_mut().unwrap();
                    if n < 1 {
                        t.status = 5;
                    } else if let Some(&e) = t.targets.get(n as usize - 1) {
                        sent = self.treasure_send(g, e);
                    }
                }
                0x141 if g.ships().contains(&id) => {
                    let Some(i) = self
                        .object_index(id)
                        .filter(|&i| self.objects[i].ship.is_some())
                    else {
                        continue;
                    };
                    if self.treasure_home(g, i) {
                        sent += 1 + self.treasure_escorts(g, i);
                    }
                }
                0x142 => {
                    g.block_mut().get_or_insert_with(Default::default).status = 4;
                }
                _ => {}
            }
        }
        sent
    }

    /// The treasure block's tick (0x4da6a0, the navy's 0x4c5088): the
    /// notices (`treasure_notices`); when none sent a ship (or none were
    /// heard), group `+8` past 0x5a and the block there, `+8` back to 0
    /// and each ship of the group in turn (index `n`): a blow marked
    /// (`+0x691`) forgotten (0x4cf760, command 0x28 sub 4) after its
    /// report 0x4a21c0 (`report_blow`); lying still (`+0x58c` 0) and
    /// recorded (0x4d9760): with record `n` (the loop's index, not the
    /// ship's) of state 2 and treasures aboard (`+0x482`), the trip home
    /// again (`treasure_home`); else, with a record of state 2 (the
    /// first), it escorts that ship (cell -1). Then `+8` grows by the
    /// tick. (Not decoded: an index `n` past the records, which the
    /// original reads at address 0x18; taken as not state 2.)
    pub(in crate::scene) fn treasure_tick(&mut self, g: &mut dyn TreasureGroup) {
        let sent = if !g.has_notices() {
            0
        } else {
            self.treasure_notices(g)
        };
        if sent == 0 && *g.elapsed() > 0x5a && g.block().is_some() {
            *g.elapsed() = 0;
            for (n, id) in g.ships().to_vec().into_iter().enumerate() {
                let Some(i) = self.object_index(id) else {
                    continue;
                };
                let Some(s) = self.objects[i].ship.as_ref() else {
                    continue;
                };
                let (still, aboard) = (s.move_state == 0, s.treasures.len());
                self.report_blow(id);
                let b = g.block().as_ref().unwrap();
                if !still || !b.ships.iter().any(|r| r.ship == id) {
                    continue;
                }
                let home = b.ships.get(n).is_some_and(|r| r.state == 2) && aboard != 0;
                let carrier = b.ships.iter().find(|r| r.state == 2).map(|r| r.ship);
                if home {
                    self.treasure_home(g, i);
                } else if let Some(ward) = carrier {
                    self.treasure_escort(g, id, ward, (-1, -1));
                }
            }
        }
        *g.elapsed() += 1;
    }
}
