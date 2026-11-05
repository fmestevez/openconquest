//! Ships set to ram or board (`+0x684 & 6`) going in at will: in the
//! ship's tick (0x4ab930) after its behaviour, every fourth tick the
//! weapons' gate 0x4bf9b0 marks the ram and the board (`+0x710`,
//! `+0x711`) each once 50 ticks have passed since its time (`+0x704`,
//! `+0x708`), and the fire state's search (0x4c1240 -> 0x4bfc60) lists
//! the ships about it it could close with (0x4c0bc0 into 0x5c7d39), picks
//! one (0x4e8db0 mode 0x32) and sends itself the board or ram (0x4c0b30,
//! event 0x10). A game tick at a time.

use super::order::Order;
use super::{walk_path, Scene};
use crate::combat;
use crate::objects;

/// The reach 0x4bfc60 gives 0x4c0bc0 (0x557480 from the ship's cell).
const REACH: i32 = 2;
/// The human's prey must lie within this of its route's end (0x4c0bc0).
const ROUTE_REACH: i32 = 5;
/// At most so many on the list (0x4c0bc0: 0x31 is the last entry).
const LIST: usize = 50;
/// The odds the pick needs (0x4e8db0: below 0x66 nothing).
const ODDS: i32 = 0x66;

impl Scene {
    /// The at-will ram and board of every ship, a game tick at a time.
    pub(super) fn run_at_will(&mut self) {
        let now = self.ticks;
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship
                    .as_ref()
                    .is_some_and(|s| s.behaviour.ram || s.behaviour.board)
            })
            .map(|o| o.id)
            .collect();
        for id in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let Some((ram, board)) = self.at_will_marks(i, now) else {
                continue;
            };
            // 0x4c1240: the search in fire states 1 and 4 (0x4bfc60) and
            // 2 (0x4c0eb0); in 3 the attack (0x4c09c0) calls 0x4c0eb0 only
            // after 0x4bf9b0 again, which has cleared the marks just set.
            let s = self.objects[i].ship.as_ref().unwrap();
            let state = s.behaviour.fire_state;
            if !(ram || board) || !matches!(state, 1 | 2 | 4) {
                continue;
            }
            let list = self.at_will_list(i, state == 2);
            if list.is_empty() {
                continue;
            }
            let Some((target, board)) = self.at_will_pick(i, &list, ram, board) else {
                continue;
            };
            // 0x4c0b30: the record `{1 ram / 2 board, 1, id, 0xff, 0x46 /
            // 0x45}` to itself (vt+4 event 0x10; its intake 0x4a9f2d keeps
            // bytes 0-6, the order 0x4aa840 runs at the ship's next record
            // pass), the ram's time `+0x708` now, the board's `+0x704`.
            let s = self.objects[i].ship.as_mut().unwrap();
            if board {
                s.board_wait = now;
            } else {
                s.ram_wait = now;
            }
            self.post(Order::Close {
                ship: id,
                target,
                board,
            });
        }
    }

    /// The gate of 0x4ab930 to the fire state's call (0x4c1240) and the
    /// ram and board marks of 0x4bf9b0 (`+0x710`, `+0x711`): `None` when
    /// the gate is shut. Every fourth tick (`(now ^ id) & 3 == 0`), a
    /// ship that may fire (0x4e6700 0x15: capability `+0x261 & 0x800`,
    /// armed, not limed `+0x258`) or (type above 1) whose ram or board
    /// switch is on and is a royal ship, rammer or ship killer (0x597a30)
    /// or carries a hero who fights (0x4e66c0, 0x4b4dd0), afloat (`+0x251
    /// == 0`) and under no status (`+0x25c`: the siren's song); then
    /// 0x4bf9b0 (`Scene::ready_mask`) marks each switch on whose time (ram
    /// `+0x704`, board `+0x708`) is more than 50 ticks past and sets its
    /// time to now + 100, and its mask, a position loaded or a mark, opens
    /// the gate (0x4bfc60 lists nothing without a mark).
    fn at_will_marks(&mut self, i: usize, now: u32) -> Option<(bool, bool)> {
        let id = self.objects[i].id;
        if (now ^ id) & 3 != 0 || self.being_built(id) {
            return None;
        }
        let o = &self.objects[i];
        let c = o.combat.as_ref()?;
        let s = o.ship.as_ref()?;
        if c.hull <= 0.0 || self.charmed.contains_key(&id) {
            return None;
        }
        let held = self.hunt_holds(id) || self.landing_holds(id);
        let fires = !held && o.is_armed() && c.lime <= 0.0;
        let fights = s.kind > 1
            && (matches!(s.kind, 2 | 5 | 6) || s.heroes.iter().any(|&h| h != objects::ORPHEUS));
        if !fires && !fights {
            return None;
        }
        // 0x4bf9b0 (`Scene::ready_mask`): its mask, the marks among it,
        // shuts the gate when 0.
        let r = self.ready_mask(i);
        (r.mask != 0).then_some((r.ram, r.board))
    }

    /// The ships ship `i` could ram or board at will (0x4bfc60 over its
    /// box, 0x4c0bc0 into 0x5c7d39), row by row: none while it is on a
    /// ram or boarding (`+0x25d & 0x10`). The box (the range of its
    /// highest loaded position, 0x5980f4: 9 or 13, or 4 with none, about
    /// its cell) always holds the reach 2. A cell's first slot that holds
    /// anything (`+0x14` the flyers, then `+0xc`) gives a ship other than
    /// itself, not one that has reserved the cell (its `+0x5ab`), an
    /// enemy (side 2 but the sea monster, or another player at war:
    /// 0x4efe20 < 0; the port's players are all at war) whose player bit
    /// is in `+0x689`, there (vt+0x2c), within 2 of the ship's cell
    /// (0x557480) and taking a contact (`+0x261 & 0x1000`); a human's
    /// ship (side 1 or 2, `game +0x1a1d`) with a route end (`+0x5b2`, its
    /// own cell once stopped) on the map only takes those within 5 of it.
    /// At most 50. The classes mask (`+0x68d`, 0x50ebb0) shuts out an
    /// object too: always in fire state 2 (0x4c0eb0, `point`), and in 1
    /// and 4 with guard on while not every position's target (`+0x640 +
    /// 0x12k`) has been met earlier in the box (0x4bfc60's `local_5`, a
    /// bit a position, against 7 or 0xf). (Not ported: 0x4bfc60's box is
    /// its highest ready position's range, wider than the reach, and its
    /// cells beyond the reach can meet those targets first; the ready
    /// mask needs the fire pass's times, `+0x5e3`.)
    pub(super) fn at_will_list(&self, i: usize, point: bool) -> Vec<usize> {
        let mut list = Vec::new();
        if self.closing_in(i) {
            return list;
        }
        let me = &self.objects[i];
        let s = me.ship.as_ref().unwrap();
        let own = self.cell_of_object(i);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let human = me.owner.is_some_and(|(n, k)| self.is_human_id(n * 4 + k));
        let end = {
            let t = openconquest_formats::map::TILE_SIZE as f32;
            s.path.back().map_or(own, |&(x, y)| {
                ((x / t).floor() as i32, (y / t).floor() as i32)
            })
        };
        let end_index = w * end.1 + end.0;
        let route = human && end_index > 0 && end_index < w * h;
        let positions = s.positions();
        let full = (1u8 << positions) - 1;
        let mut met = 0u8;
        let (x0, y0) = ((own.0 - REACH).max(0), (own.1 - REACH).max(0));
        let (x1, y1) = ((own.0 + REACH + 1).min(w), (own.1 + REACH + 1).min(h));
        for y in y0..y1 {
            for x in x0..x1 {
                let cell = (x, y);
                if self.flyer_slot_held(cell) {
                    continue;
                }
                let Some(k) = self.slot_holder(cell) else {
                    continue;
                };
                let o = &self.objects[k];
                if k == i
                    || self.reserved_cell(k) == Some(cell)
                    || !o.combat.as_ref().is_some_and(|c| c.hull > 0.0)
                    || !Self::are_enemies(me, o)
                {
                    continue;
                }
                let Some((n, slot)) = o.owner else {
                    continue;
                };
                if s.behaviour.players & 1 << ((slot + n * 4) & 0x1f) == 0 {
                    continue;
                }
                if !point {
                    for (k, a) in s.behaviour.aims[..positions].iter().enumerate() {
                        if a.target == o.id {
                            met |= 1 << k;
                        }
                    }
                }
                if (point || s.behaviour.guard && met != full) && !self.picks_at_will(me, o) {
                    continue;
                }
                if walk_path::dist2(own.0, own.1, x, y) > REACH {
                    continue;
                }
                if route && walk_path::dist2(x, y, end.0, end.1) > ROUTE_REACH {
                    continue;
                }
                if self.takes_contact(k) && list.len() < LIST {
                    list.push(k);
                }
            }
        }
        list
    }

    /// 0x4e8db0 mode 0x32: of the list, the best to board (its boarding
    /// strength `+0x53c` x 100 over the prey's, at most 200, 200 against
    /// none and 0 with none of its own; 0 against a phantom, `+0x253`, and
    /// for a rammer at a fire rammer; the first greatest, a tie to the
    /// nearer by 0x557440) and the best to ram (`combat::at_will_ram_odds`,
    /// the first greatest above 0). Only a marked switch counts (`ram`,
    /// `board`). The ram when its odds pass the boarding's, else the
    /// boarding, if its odds reach 102: `(prey id, board)`.
    fn at_will_pick(
        &self,
        i: usize,
        list: &[usize],
        ram: bool,
        board: bool,
    ) -> Option<(u32, bool)> {
        let me = &self.objects[i];
        let s = me.ship.as_ref().unwrap();
        let own = self.cell_of_object(i);
        let mine = self.boarding_strength(i) as i32;
        let rammer = |k: usize| {
            let o = &self.objects[k];
            let p = o.ship.as_ref().unwrap();
            combat::Rammer {
                kind: p.kind,
                ram: p.ram,
                level: self.levels_of(o.owner).ram_kinds[p.ram.min(2) as usize],
                heading: p.heading,
                speed: 0.0,
                rigging: 0,
            }
        };
        let (mut best_board, mut board_odds, mut nearest) = (None, 0, 10000);
        let (mut best_ram, mut ram_odds) = (None, 0);
        for &k in list {
            let o = &self.objects[k];
            let p = o.ship.as_ref().unwrap();
            let his = self.boarding_strength(k) as i32;
            let mut b = if his == 0 {
                if mine == 0 {
                    0
                } else {
                    200
                }
            } else {
                (mine * 100 / his).min(200)
            };
            if o.combat.as_ref().is_some_and(|c| c.phantom > 0.0) || (s.kind == 5 && p.kind == 10) {
                b = 0;
            }
            let at = self.cell_of_object(k);
            let d = walk_path::dist(own.0, own.1, at.0, at.1);
            if board_odds < b || (board_odds <= b && d < nearest) {
                best_board = Some(k);
                nearest = d;
                board_odds = b;
            }
            let r = combat::at_will_ram_odds(&rammer(i), &rammer(k), s.way);
            if ram_odds < r {
                best_ram = Some(k);
                ram_odds = r;
            }
        }
        if !board {
            board_odds = 0;
        }
        if !ram {
            ram_odds = 0;
        }
        let pick = if ram_odds <= board_odds {
            (board_odds >= ODDS).then_some((best_board?, true))
        } else {
            (ram_odds >= ODDS).then_some((best_ram?, false))
        }?;
        Some((self.objects[pick.0].id, pick.1))
    }
}
