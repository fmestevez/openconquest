//! The fleet attack task `{0x74, 0x15}`: the human's fleet commands
//! 0x15-0x18 (attack, attack, board, ram) at a target, from a selection
//! (0x49e230 -> 0x49dda0 -> 0x4d04a0; no other sender). The selection's
//! group keeps the target `+0xc`, the command `+0x14` and a queue of
//! targets `+0x30` (Shift), sends its ships at the target (0x4cfe40 ->
//! 0x4cfc90), hears their notices (0x4d0210) and sends the ships lying
//! still again every 81 ticks (0x4d0650). See docs/FORMATS.md ("The
//! fleet attack task").

use super::gather::Notice;
use super::{Order, Scene};
use openconquest_formats::map::TILE_SIZE;
use serde::{Deserialize, Serialize};

/// An entry of the block's target queue `+0x30` (0x4cfbc0: 0x18 bytes
/// `{0, target id, -1, command, 0, 0}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    /// `+4`.
    pub target: u32,
    /// `+0xc`.
    pub command: u8,
    /// `+0x10` (always 0 as 0x4cfbc0 writes it).
    pub extra: u32,
}

/// A human group running the fleet attack task: the group's own fields
/// the task reads and the block (0x65 bytes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetAttack {
    /// The group's serial number (shared with the other human groups).
    pub serial: u32,
    /// Group `+0x31`: its ships, by id.
    pub ships: Vec<u32>,
    /// Group `+0x2d`: the notices not yet read, oldest first.
    pub notices: Vec<Notice>,
    /// Group `+8`: the task's tick counter.
    pub elapsed: u32,
    /// Block `+8`: the tick it was made or rewritten (game `+0xde`).
    pub made: u32,
    /// Block `+0xc`: the target's id.
    pub target: u32,
    /// Block `+0x10`: a target's cell `x + y * W` (0x50ebf0), read only
    /// by the route's 0x4ceab0.
    pub cell: i32,
    /// Block `+0x14`: the command 0x15-0x18.
    pub command: u8,
    /// Block `+0x18`: the command's sub byte, 7.
    pub sub: u8,
    /// Block `+0x1c`.
    pub extra: u32,
    /// Block `+0x30`.
    pub queue: Vec<Queued>,
}

/// 0x4d0650's period: the ships are sent again once group `+8` is over
/// 0x50.
pub const PASS: u32 = 0x50;

impl Scene {
    /// The human's fleet command `command` (0x15 / 0x16 attack, 0x17
    /// board, 0x18 ram) at `target` for `ships`, with the click's key
    /// flags `keys` (0x49dda0: the payload `{command, target, cell, 0}`).
    /// Forts in the selection take the attack themselves (0x419f80). The
    /// port refuses a target the attack pointer would not let through (a
    /// friend, or for the board and the ram anything but a ship).
    ///
    /// With Shift (`keys & 4`) and the selection's group running this
    /// block, the target is only queued (0x4cfbc0). Else 0x4d04a0: the
    /// selection's group (0x49e230 keeps it while the selection is
    /// unchanged, its block of another kind ended by 0x4a1c60; else
    /// 0x49ca60 makes a new one, its ships leaving their old groups)
    /// gets the block made (0x4cf990) or rewritten in place (the queue
    /// kept): `+8` now, `+0xc` the target, `+0x14` the command, `+0x18`
    /// 7, `+0x1c` 0, `+0x10` the target's cell (not found: kept; a jam
    /// 0). The target queued (0x4cfbc0) and, found (0x51f1f0), each ship
    /// sent (0x4cfe40). Not ported: a route `+0x61` kept from a block of
    /// another kind under Shift, and with it 0x4cfe40's route branch,
    /// the held orders `+0x34` and notice 0x136's case.
    pub fn order_fleet_attack(
        &mut self,
        ships: &[u32],
        target: u32,
        command: u8,
        keys: u8,
    ) -> Result<(), String> {
        let mut result = Err("Nothing is selected".to_string());
        let mut group = Vec::new();
        for &id in ships {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            if self.objects[i].ship.is_some() {
                group.push(id);
            } else if command <= 0x16 && self.objects[i].holds_units() {
                result = self.order_attack(id, target).or(result);
            }
        }
        let Some(&first) = group.first() else {
            return result;
        };
        let jam = self.jams.iter().any(|j| j.id == target);
        let me = &self.objects[self.object_index(first).unwrap()];
        let ok = jam
            || self.object_index(target).is_some_and(|t| {
                let it = &self.objects[t];
                if command >= 0x17 {
                    it.ship.is_some() && Self::are_enemies(me, it)
                } else {
                    self.ruins_enemy(me, it) || (it.combat.is_some() && Self::are_enemies(me, it))
                }
            });
        if !ok {
            return Err(if command >= 0x17 {
                "Choose an enemy ship".into()
            } else {
                "Choose an enemy ship or building".into()
            });
        }
        let z = self.selection_fleet(&group);
        if keys & 4 != 0 {
            if let Some(z) = z {
                let k = fa_queue(&mut self.fleet_attacks[z], target, command);
                self.fa_mark(target, k, command);
                return Ok(());
            }
        }
        let z = match z {
            Some(z) => z,
            None => {
                // The group: the selection's own (its other block ended,
                // its counter kept), or a new one.
                let mut elapsed = 0;
                let serial = if let Some(g) = self.selection_zone(&group) {
                    let zone = self.gather_zones.remove(g);
                    elapsed = zone.elapsed;
                    match zone.group {
                        super::gather::ZoneGroup::Human(s) => s,
                        _ => unreachable!(),
                    }
                } else if let Some(g) = self.selection_attack(&group) {
                    let a = self.human_attacks.remove(g);
                    elapsed = a.elapsed;
                    a.serial
                } else {
                    self.leave_zones(&group);
                    self.zone_serial += 1;
                    self.zone_serial
                };
                self.fleet_attacks.push(FleetAttack {
                    serial,
                    ships: group.clone(),
                    notices: Vec::new(),
                    elapsed,
                    made: 0,
                    target: 0,
                    cell: 0,
                    command: 0,
                    sub: 0,
                    extra: 0,
                    queue: Vec::new(),
                });
                self.fleet_attacks.len() - 1
            }
        };
        let cell = if jam {
            Some(0)
        } else {
            self.object_index(target).map(|t| {
                let (x, y) = self.cell_of(t);
                x + y * self.nav.width as i32
            })
        };
        let now = self.ticks;
        let a = &mut self.fleet_attacks[z];
        a.made = now;
        a.target = target;
        a.command = command;
        a.sub = 7;
        a.extra = 0;
        if let Some(c) = cell {
            a.cell = c;
        }
        let k = fa_queue(a, target, command);
        if keys & 4 != 0 {
            self.fa_mark(target, k, command);
        }
        if self.fa_found(target) {
            let mut a = self.fleet_attacks.remove(z);
            for id in a.ships.clone() {
                self.fa_send(&mut a, id);
            }
            self.fleet_attacks.insert(z, a);
        }
        Ok(())
    }

    /// The fleet attack group whose ships are exactly `ships`.
    pub fn selection_fleet(&self, ships: &[u32]) -> Option<usize> {
        let mut sel: Vec<u32> = ships.to_vec();
        sel.sort_unstable();
        sel.dedup();
        self.fleet_attacks.iter().position(|a| {
            let mut s = a.ships.clone();
            s.sort_unstable();
            !s.is_empty() && s == sel
        })
    }

    /// The fleet attack group holding ship `id`.
    pub fn ship_fleet(&self, id: u32) -> Option<usize> {
        self.fleet_attacks
            .iter()
            .position(|a| a.ships.contains(&id))
    }

    /// Ships leaving their fleet attack groups post notice 0x137
    /// (0x4a1770; 0x4d0210 drops it); a group left with none goes.
    pub(super) fn leave_fleets(&mut self, ids: &[u32]) {
        let tick = self.ticks;
        for a in &mut self.fleet_attacks {
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
        self.fleet_attacks.retain(|a| !a.ships.is_empty());
    }

    /// The navy's tick for the fleet attack groups (0x4d0650 each, from
    /// 0x4c4f40); a block ended by its notices goes (0x4a1c60 -> 0x4cf940).
    pub(super) fn run_fleet_attacks(&mut self) {
        let gone: Vec<u32> = self
            .fleet_attacks
            .iter()
            .flat_map(|a| a.ships.iter().copied())
            .filter(|&id| {
                self.object_index(id)
                    .is_none_or(|i| self.objects[i].ship.is_none())
            })
            .collect();
        if !gone.is_empty() {
            self.leave_fleets(&gone);
        }
        let mut z = 0;
        while z < self.fleet_attacks.len() {
            let mut a = self.fleet_attacks.remove(z);
            if self.fa_tick(&mut a) {
                self.fleet_attacks.insert(z, a);
                z += 1;
            }
        }
    }

    /// 0x4d0650: with no notices, or none heard (0x4d0210), and group
    /// `+8` over 0x50 (unsigned) and the block still there: `+8 = 0` and
    /// each ship of the group lying still (`+0x58c` 0) sent (0x4cfe40)
    /// if, for 0x15 / 0x16, it has neither a fire task (`+0x25d & 0x100`)
    /// nor a ram or boarding (`& 0x10`); for 0x17 no ram or boarding and
    /// a boarding strength (`+0x53c > 0`); for 0x18 no ram or boarding.
    /// Then `+8` grows by the tick's `+0xe2` (1). No draws. Returns
    /// whether the block is still there.
    fn fa_tick(&mut self, a: &mut FleetAttack) -> bool {
        let heard = if a.notices.is_empty() {
            0
        } else {
            match self.fa_notices(a) {
                Some(heard) => heard,
                None => return false,
            }
        };
        if heard == 0 && a.elapsed > PASS {
            a.elapsed = 0;
            for id in a.ships.clone() {
                let Some(i) = self.object_index(id) else {
                    continue;
                };
                let Some(s) = self.objects[i].ship.as_ref() else {
                    continue;
                };
                if s.move_state != 0 {
                    continue;
                }
                let closing = self.closing_in(i);
                let send = match a.command {
                    0x15 | 0x16 => !s.fire_task() && !closing,
                    0x17 => !closing && self.boarding_strength(i) > 0,
                    0x18 => !closing,
                    _ => false,
                };
                if send {
                    self.fa_send(a, id);
                }
            }
        }
        a.elapsed = a.elapsed.wrapping_add(1);
        true
    }

    /// 0x4d0210: the notices, newest first, each removed after its case,
    /// while the block lasts; `None` when it ended (0x4a1c60, the notices
    /// freed with it), else the count heard. The record's ship is the
    /// group's (0x4cc590), or nothing is done. 300 (a join): 0x4cfad0
    /// true sends it (0x4cfe40), heard. (Not ported: the `#` sub 1 with
    /// the group's three it then sends, 0x4c42f0; the port posts no 300
    /// to a human group.) 0x136: only with a route `+0x61` (none here).
    /// 0x13a-0x13d (`class` the prey): its entry of the queue removed
    /// (0x4cfa70); none, or 0x4cfad0 true, the ship sent again
    /// (0x4cfe40), heard; else the block ends. Others dropped.
    fn fa_notices(&mut self, a: &mut FleetAttack) -> Option<u32> {
        let mut heard = 0;
        while let Some(n) = a.notices.pop() {
            let ship = a.ships.contains(&n.ship)
                && self
                    .object_index(n.ship)
                    .is_some_and(|i| self.objects[i].ship.is_some());
            match n.code {
                300 if ship => {
                    if self.fa_next(a) {
                        self.fa_send(a, n.ship);
                        heard += 1;
                    }
                }
                0x13a..=0x13d if ship => {
                    let i = a.queue.iter().position(|q| q.target == n.class);
                    if let Some(i) = i {
                        a.queue.remove(i);
                    }
                    if i.is_none() || self.fa_next(a) {
                        self.fa_send(a, n.ship);
                        heard += 1;
                    } else {
                        return None;
                    }
                }
                _ => {}
            }
        }
        Some(heard)
    }

    /// 0x4cfad0, the next target: none with the queue empty; else with
    /// the block's target `+0xc` still found (0x51f1f0) `+0x10` = that
    /// old target's cell, and from the queue's first entry (not removed)
    /// `+0xc`, `+0x14`, `+0x1c`, with `+0x18` 7. A sunk target is still
    /// found while it sinks (`Sinking`), so its 0x13a moves on; only a
    /// target removed (message 10 past its sinking, a prize gone) ends
    /// the block, however many targets are queued.
    fn fa_next(&self, a: &mut FleetAttack) -> bool {
        let Some(&q) = a.queue.first() else {
            return false;
        };
        if !self.fa_found(a.target) {
            return false;
        }
        if self.jams.iter().any(|j| j.id == a.target) {
            a.cell = 0;
        } else if let Some(t) = self.object_index(a.target) {
            let (x, y) = self.cell_of(t);
            a.cell = x + y * self.nav.width as i32;
        } else if let Some(k) = self.sinkings.iter().find(|k| k.id == a.target) {
            a.cell = k.cell.0 + k.cell.1 * self.nav.width as i32;
        }
        a.target = q.target;
        a.command = q.command;
        a.sub = 7;
        a.extra = q.extra;
        true
    }

    /// Whether object `id` is found (0x51f1f0): an object of the map, a
    /// land creatures' jam, or a ship sinking until its message 10. The
    /// lookup skips only entries whose `+0x14` is set: the base class
    /// 0x5425a0 sets it 1, the game objects' 0x50b960 then 0, and no game
    /// object's store of it was found, so a sunk ship stays found until
    /// removed.
    fn fa_found(&self, id: u32) -> bool {
        self.object_index(id).is_some()
            || self.jams.iter().any(|j| j.id == id)
            || self
                .sinkings
                .iter()
                .any(|k| k.id == id && k.until > self.ticks)
    }

    /// 0x4a1f20 (navy, target, index, command): message 0xc45d to the
    /// target, sub 1, `+0x18` the kind (8 for 0x15 / 0x16, 6 for 0x17, 7
    /// for 0x18), `+0x1a` the byte `index + 1`. Only the human orders this
    /// block, so its player is the viewer (0x4cfbc0's `navy +0x21` test).
    /// By the target's class (its GetMessage): a ship (0x4de040) with a
    /// picture `+0x8be` makes its mark (0x50c020) or redraws it
    /// (0x50c3d0), with none nothing; a LandObj (0x425990, 0x426de1) and
    /// a cyclops, minotaur or amazon (0x413d70) make it of the kind sent;
    /// the gorgon (0x402360) and a siren (0x403f60) always of kind 8,
    /// keeping it (`+0xbc` / `+0x7c`) and its number; a sea monster
    /// (0x492360) keeps it (`+0x12d`, `+0x131`) and makes it of kind 8
    /// only with its picture `+0x139` (shown by `wp_mark_layers` while it
    /// is up); a harpy or dragon (0x4207b0) makes it of the kind sent.
    pub(super) fn fa_mark(&mut self, target: u32, index: usize, command: u8) {
        use openconquest_formats::ObjectClass;
        let kind = match command {
            0x15 | 0x16 => 8,
            0x17 => 6,
            0x18 => 7,
            _ => return,
        };
        let Some(i) = self.object_index(target) else {
            return;
        };
        let o = &self.objects[i];
        let kind = match o.class {
            _ if o.ship.is_some() => {
                if o.frames.is_empty() {
                    return;
                }
                kind
            }
            ObjectClass::Building(_)
            | ObjectClass::Barbarian(_)
            | ObjectClass::Castle(_)
            | ObjectClass::HeroHouse
            | ObjectClass::CreatureSpawn
            | ObjectClass::Monster(0x1f4..=0x1f6 | 0x1f8 | 0x1f9) => kind,
            ObjectClass::Monster(0x1fa | 0x1fb | 0x1fd) => 8,
            _ => return,
        };
        self.wp_marks
            .insert(target, (kind, (index as u8).wrapping_add(1)));
    }

    /// 0x4a2050(navy, player, 0), at a new selection (0xc453 to the navy)
    /// and a ship added to or taken from it (0xc455, 0x49e850), before the
    /// selection changes: the selected group's block of this kind runs
    /// 0x4d07a0(queue, 1, 0), which for each entry whose target is found
    /// (0x51f1f0) posts 0xc45d sub 0 (0x4a2000): the ship's mark freed
    /// (0x50c320). The port calls it when it sees the selection `now`
    /// differ from the one before.
    pub fn fa_selection_seen(&mut self, now: &[u32]) {
        if self.wp_selection.as_slice() == now {
            return;
        }
        let before = std::mem::replace(&mut self.wp_selection, now.to_vec());
        let Some(z) = self.selection_fleet(&before) else {
            return;
        };
        let targets: Vec<u32> = self.fleet_attacks[z]
            .queue
            .iter()
            .map(|q| q.target)
            .collect();
        for t in targets {
            if self.fa_found(t) {
                self.wp_marks.remove(&t);
            }
        }
    }

    /// 0x4a2050(navy, player, show) from the navy's 0xc45d (0x49b73b,
    /// show its `+0x14`): Shift pressed (1) or let go (0), forwarded by
    /// the game screen's key object (0x4eff70). For the selected group's
    /// block (0x74 / 0x15) 0x4d07a0(queue, 1, show) gives each entry whose
    /// target is found (0x51f1f0) its mark, numbered by its place in the
    /// queue (0x4a1f20), or frees it (0x4a2000).
    pub fn fa_marks_shown(&mut self, selection: &[u32], show: bool) {
        let Some(z) = self.selection_fleet(selection) else {
            return;
        };
        let queue = self.fleet_attacks[z].queue.clone();
        for (k, q) in queue.iter().enumerate() {
            if !self.fa_found(q.target) {
                continue;
            }
            if show {
                self.fa_mark(q.target, k, q.command);
            } else {
                self.wp_marks.remove(&q.target);
            }
        }
    }

    /// The mark over `obj` (0x50c020's `WPFIRE` / `WPBOARD` / `WPRAM`,
    /// controlm, 36 x 29, kind 8 at depth 7, over the hull's 0xa): its
    /// layers by the mask `1 << mark / 10 | 1 << mark % 10 + 10 |
    /// 0x100000` (the tens' digit layers 0-9, the units' 10-19, the
    /// picture 20), only 0x100000 for a mark of 0 or 100 and more. Its
    /// top left (0x50c600) by the class: a ship, LandObj or cyclops
    /// `(w - 0x24) / 2 + x`, `(h - 0x1d) / 2 + y` of its picture (a ship
    /// each tick at 0x4dc9dd, a cyclops at 0x413d70's tick, a LandObj at
    /// its message and 0x425990's tick); the gorgon and a siren `(w -
    /// 0x24) / 2`, `(h - 0x1d) / 2` of their picture, its x and y not
    /// added (0x402360 / 0x403f60 at the message and each tick), so on
    /// the map's top left, and only with their picture; a sea monster
    /// its cell `x * tile w + 0xe`, `y * tile h + 0x11` (0x492360 at the
    /// message and each tick, 0x491830 states 1 and 4), shown only while
    /// up (state 0 frees it, keeping it); a harpy or dragon by its state
    /// from its picture (`Scene::flyer_mark_place`). Each layer with its
    /// own offset.
    pub fn wp_mark_layers(
        &self,
        obj: &crate::objects::MapObject,
    ) -> Vec<(&crate::objects::Layer, i32, i32)> {
        use openconquest_formats::ObjectClass;
        let Some(&(kind, mark)) = self.wp_marks.get(&obj.id) else {
            return Vec::new();
        };
        let Some((_, art)) = self.wp_art.iter().find(|(k, _)| *k == kind) else {
            return Vec::new();
        };
        let picture = obj.frames.first().and_then(|f| f.first());
        let (left, top) = match obj.class {
            ObjectClass::Monster(0x1fb) => {
                if self.sea_monster_hidden(obj.id) {
                    return Vec::new();
                }
                let t = TILE_SIZE as f32;
                let (cx, cy) = ((obj.x / t).floor() as i32, (obj.y / t).floor() as i32);
                (cx * t as i32 + 0xe, cy * t as i32 + 0x11)
            }
            ObjectClass::Monster(0x1f8 | 0x1f9) => {
                let Some(p) = self.flyer_mark_place(obj.id) else {
                    return Vec::new();
                };
                p
            }
            ObjectClass::Monster(c @ (0x1fa | 0x1fd)) => {
                let up = if c == 0x1fa {
                    self.gorgons.get(&obj.id).is_some_and(|g| g.picture)
                } else {
                    self.sirens.get(&obj.id).is_some_and(|s| s.picture)
                };
                let Some(l) = picture.filter(|_| up) else {
                    return Vec::new();
                };
                let (w, h) = (l.image.width as i32, l.image.height as i32);
                ((w - 0x24) / 2, (h - 0x1d) / 2)
            }
            _ => {
                let Some(l) = picture else {
                    return Vec::new();
                };
                let (x, y) = crate::render::layer_origin(obj, l);
                let (w, h) = (l.image.width as i32, l.image.height as i32);
                ((w - 0x24) / 2 + x, (h - 0x1d) / 2 + y)
            }
        };
        let mask: u32 = if mark != 0 && mark < 100 {
            1 << (mark / 10) | 1 << (mark % 10 + 10) | 0x100000
        } else {
            0x100000
        };
        art.iter()
            .filter(|(k, _)| mask & (1 << k) != 0)
            .map(|(_, l)| (l, left + l.dx, top + l.dy))
            .collect()
    }

    /// 0x4cfe40 without a route: the record `{ship, +0xc, +0x10, +0x14,
    /// +0x1c, +0x18}` (0x4cfdf0) given as the command 0x4cfc90: 0x15 /
    /// 0x16 a ship with no strength (0x50ea70 0) sent to the target's
    /// cell (0x51f1f0, vt+0x28; none: nothing) by a plain move
    /// (0x4cf070), else the command `{target, 7, command, 0x44}` (the
    /// outbox, 0x520cc0); 0x17 a ship with no boarding strength
    /// (`+0x53c < 1`) nothing, else 0x17 `{target, 0x45}`; 0x18 0x18
    /// `{target, 0x46}`.
    fn fa_send(&mut self, a: &mut FleetAttack, ship: u32) {
        let Some(i) = self.object_index(ship) else {
            return;
        };
        let target = a.target;
        match a.command {
            0x15 | 0x16 => {
                if self.strength(i) != 0 {
                    self.post(Order::Attack { ship, target });
                    return;
                }
                let (x, y) = match self.object_index(target) {
                    Some(t) => self.cell_of(t),
                    None => match self.sinkings.iter().find(|k| k.id == target) {
                        Some(k) if k.until > self.ticks => k.cell,
                        _ => return,
                    },
                };
                let s = TILE_SIZE as f32;
                self.post(Order::Move {
                    ship,
                    x: x as f32 * s + s / 2.0,
                    y: y as f32 * s + s / 2.0,
                });
            }
            0x17 if self.boarding_strength(i) >= 1 => self.post(Order::Close {
                ship,
                target,
                board: true,
            }),
            0x18 => self.post(Order::Close {
                ship,
                target,
                board: false,
            }),
            _ => {}
        }
    }
}

/// 0x4cfbc0: the first entry for `target` with the same command is kept
/// (its index returned); with another it is removed and the target
/// appended anew, as one not queued. For the viewing player with Shift
/// the caller shows the entry's mark (0x4a1f20, `Scene::fa_mark`).
fn fa_queue(a: &mut FleetAttack, target: u32, command: u8) -> usize {
    if let Some(k) = a.queue.iter().position(|q| q.target == target) {
        if a.queue[k].command == command {
            return k;
        }
        a.queue.remove(k);
    }
    a.queue.push(Queued {
        target,
        command,
        extra: 0,
    });
    a.queue.len() - 1
}

/// The marks' pictures (0x50c020: kind 6 `WPBOARD`, 7 `WPRAM`, 8
/// `WPFIRE`, composites of controlm): each with its layers as
/// `(layer index, layer)`, in the order drawn (the highest order first,
/// as `objects::composite_frames_with`).
pub(super) fn wp_art(data: &crate::assets::GameData) -> WpArt {
    use openconquest_formats::{anim::EMPTY_SPRITE, Composite, ResourceKind};
    let Some(menus) = data.menus.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (kind, name) in [(6, "WPBOARD"), (7, "WPRAM"), (8, "WPFIRE")] {
        let Some(e) = menus.find(name, ResourceKind::Composite) else {
            continue;
        };
        let Ok(c) = menus.read_entry(e).and_then(|b| Composite::parse(&b, e)) else {
            continue;
        };
        if c.frames == 0 {
            continue;
        }
        let mut cells: Vec<_> = c.frame(0).iter().enumerate().collect();
        cells.sort_by_key(|(_, c)| std::cmp::Reverse(c.order));
        let mut layers = Vec::new();
        for (k, cell) in cells {
            if cell.sprite == EMPTY_SPRITE {
                continue;
            }
            if let Ok(Some(img)) = data.load_image(menus, &cell.sprite, false) {
                layers.push((
                    k,
                    crate::objects::Layer {
                        image: std::rc::Rc::new(img),
                        dx: cell.dx,
                        dy: cell.dy,
                    },
                ));
            }
        }
        out.push((kind, layers));
    }
    out
}

/// The marks' pictures by kind (`wp_art`).
pub type WpArt = Vec<(u8, Vec<(usize, crate::objects::Layer)>)>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::GameData;
    use crate::objects::Caches;

    /// Two of the human's ships on miss000 ordered at an enemy ship, a
    /// second Shift-queued (0x4cfbc0): the block `{target, cell, command
    /// 0x16, 7}`, each ship sent at the first (0x4cfe40), the queue both.
    /// A 0x13a for the first while it is still there (a prize taken) moves
    /// the block to the queued one (0x4cfad0, `+0x10` the old target's
    /// cell) and sends the ship there; the first queued again and the
    /// second sunk, its notice ends the block though a target is queued
    /// (0x4cfad0 finds no old target).
    #[test]
    fn fleet_attack_queue_on_miss000() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let foe = (human.0 ^ 1, human.1);
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(4)
            .collect();
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let row = (3..h - 3)
            .flat_map(|y| (3..w - 6).map(move |x| (x, y)))
            .find(|&(x, y)| (0..4).all(|k| water(&scene, (x + k, y))))
            .unwrap();
        let t = TILE_SIZE as f32;
        for (k, &i) in ships.iter().enumerate() {
            let o = &mut scene.objects[i];
            o.owner = Some(if k < 2 { human } else { foe });
            o.x = (row.0 + k as i32) as f32 * t + t / 2.0;
            o.y = row.1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.argo = false;
            s.move_state = 0;
        }
        let id = |k: usize| scene.objects[ships[k]].id;
        let (id0, id1, f, g) = (id(0), id(1), id(2), id(3));
        scene.outbox.clear();
        scene.order_fleet_attack(&[id0, id1], f, 0x16, 0).unwrap();
        scene.order_fleet_attack(&[id0, id1], g, 0x16, 4).unwrap();
        assert_eq!(scene.fleet_attacks.len(), 1);
        let a = scene.fleet_attacks[0].clone();
        let fcell = row.0 + 2 + row.1 * w;
        assert_eq!((a.target, a.cell, a.command, a.sub), (f, fcell, 0x16, 7));
        let q = |target| Queued {
            target,
            command: 0x16,
            extra: 0,
        };
        assert_eq!(a.queue, [q(f), q(g)]);
        // Only the Shift-queued target shows its mark (0x4a1f20): WPFIRE
        // numbered 2, the tens' 0 and the units' 2 over the picture.
        assert_eq!(scene.wp_marks.get(&f), None);
        assert_eq!(scene.wp_marks.get(&g), Some(&(8, 2)));
        let gi = scene.object_index(g).unwrap();
        let ks: Vec<i32> = scene
            .wp_mark_layers(&scene.objects[gi])
            .iter()
            .map(|(l, _, _)| l.dx)
            .collect();
        assert_eq!(ks, [0, 29, 19]);
        // A selection changed hides the queue's marks (0x4a2050).
        scene.fa_selection_seen(&[id0, id1]);
        scene.fa_selection_seen(&[id0]);
        assert!(scene.wp_marks.is_empty());
        // Shift pressed with the group selected shows every queued
        // target's mark, numbered by its place (0xc45d 1, 0x4d07a0); let
        // go, they are hidden (0xc45d 0).
        scene.fa_marks_shown(&[id1, id0], true);
        assert_eq!(scene.wp_marks.get(&f), Some(&(8, 1)));
        assert_eq!(scene.wp_marks.get(&g), Some(&(8, 2)));
        scene.fa_marks_shown(&[id0], true);
        assert_eq!(scene.wp_marks.len(), 2);
        scene.fa_marks_shown(&[id0, id1], false);
        assert!(scene.wp_marks.is_empty());
        let at = |ship, target| Order::Attack { ship, target };
        assert_eq!(std::mem::take(&mut scene.outbox), [at(id0, f), at(id1, f)]);
        // A prize taken: the old target still there.
        scene.post_notice(id0, 0x13a, f);
        scene.run_fleet_attacks();
        let a = &scene.fleet_attacks[0];
        assert_eq!((a.target, a.cell), (g, fcell));
        assert_eq!(a.queue, [q(g)]);
        assert_eq!(std::mem::take(&mut scene.outbox), [at(id0, g)]);
        // f queued again behind g; g sunk, its hull still in the world's
        // table while it sinks (0x51f1f0 finds it): its 0x13a moves the
        // block on to f, `+0x10` g's cell.
        scene.order_fleet_attack(&[id0, id1], f, 0x16, 4).unwrap();
        assert_eq!(scene.fleet_attacks[0].queue, [q(g), q(f)]);
        let gi = scene.object_index(g).unwrap();
        scene.sink_drop(gi, 20, Vec::new());
        assert_eq!(scene.sinkings[0].until, scene.ticks + 60);
        scene.objects.retain(|o| o.id != g);
        scene.post_notice(id1, 0x13a, g);
        scene.run_fleet_attacks();
        let a = &scene.fleet_attacks[0];
        assert_eq!((a.target, a.cell), (f, row.0 + 3 + row.1 * w));
        assert_eq!(a.queue, [q(f)]);
        assert_eq!(std::mem::take(&mut scene.outbox), [at(id1, f)]);
        // g queued behind f, then f removed (gone from the table): its
        // 0x13a ends the block though g is queued (0x4cfad0 finds no old
        // target).
        scene.order_fleet_attack(&[id0, id1], id0, 0x16, 4).ok();
        scene.fleet_attacks[0].queue.push(q(g));
        scene.objects.retain(|o| o.id != f);
        scene.post_notice(id0, 0x13a, f);
        scene.run_fleet_attacks();
        assert!(scene.fleet_attacks.is_empty());
        assert!(scene.outbox.is_empty());
    }

    /// The marks on targets other than ships (0xc45d): a building on
    /// battle17 (LandObj 0x425990) takes the kind sent, placed as a
    /// ship's by its picture; the gorgon (0x402360) always WPFIRE, placed
    /// by her picture's size alone and only with her picture; a treasure
    /// (0x4074d0, the gather's) none from this block.
    #[test]
    fn fleet_attack_marks_on_other_classes() {
        use openconquest_formats::ObjectClass;
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle17").unwrap();
        scene.brain = None;
        let b = scene
            .objects
            .iter()
            .find(|o| matches!(o.class, ObjectClass::Building(_)) && !o.frames.is_empty())
            .unwrap()
            .id;
        scene.fa_mark(b, 2, 0x17);
        assert_eq!(scene.wp_marks.get(&b), Some(&(6, 3)));
        let bi = scene.object_index(b).unwrap();
        let o = &scene.objects[bi];
        let l = &o.frames[0][0];
        let (x, y) = crate::render::layer_origin(o, l);
        let (w, h) = (l.image.width as i32, l.image.height as i32);
        let got: Vec<(i32, i32)> = scene
            .wp_mark_layers(o)
            .iter()
            .map(|(l, a, b)| (a - l.dx, b - l.dy))
            .collect();
        assert_eq!(got, vec![((w - 0x24) / 2 + x, (h - 0x1d) / 2 + y); 3]);
        let g = scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::Monster(0x1fa))
            .unwrap()
            .id;
        scene.fa_mark(g, 0, 0x16);
        assert_eq!(scene.wp_marks.get(&g), Some(&(8, 1)));
        let gi = scene.object_index(g).unwrap();
        assert!(scene.wp_mark_layers(&scene.objects[gi]).is_empty());
        scene.gorgons.get_mut(&g).unwrap().picture = true;
        if scene.objects[gi].frames.is_empty() {
            scene.objects[gi].frames = scene.objects[bi].frames.clone();
        }
        let o = &scene.objects[gi];
        let l = &o.frames[0][0];
        let (w, h) = (l.image.width as i32, l.image.height as i32);
        let got: Vec<(i32, i32)> = scene
            .wp_mark_layers(o)
            .iter()
            .map(|(l, a, b)| (a - l.dx, b - l.dy))
            .collect();
        assert_eq!(got, vec![((w - 0x24) / 2, (h - 0x1d) / 2); 3]);
        if let Some(t) = scene.objects.iter().find(|o| o.treasure.is_some()) {
            let t = t.id;
            scene.fa_mark(t, 0, 0x16);
            assert_eq!(scene.wp_marks.get(&t), None);
        }
    }
}
