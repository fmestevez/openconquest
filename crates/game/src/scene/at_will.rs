//! Ships set to ram or board (`+0x684 & 6`) going in at will: the fire
//! pass's looks (0x4bfc60, 0x4c0eb0, `scene::fire_pass`) list each cell's
//! ship it could close with (0x4c0bc0 into 0x5c7d39) while a switch is
//! marked (0x4bf9b0), pick one (0x4e8db0 mode 0x32) and send themselves
//! the board or ram (0x4c0b30, event 0x10).

use super::order::Order;
use super::{walk_path, Scene};
use crate::combat;

/// The reach 0x4bfc60 gives 0x4c0bc0 (0x557480 from the ship's cell).
const REACH: i32 = 2;
/// The human's prey must lie within this of its route's end (0x4c0bc0).
const ROUTE_REACH: i32 = 5;
/// At most so many on the list (0x4c0bc0: 0x31 is the last entry).
const LIST: usize = 50;
/// The odds the pick needs (0x4e8db0: below 0x66 nothing).
const ODDS: i32 = 0x66;

impl Scene {
    /// The end of ship `i`'s route the at-will list keeps to (0x4c0bc0):
    /// a human's ship (side 1 or 2, `game +0x1a1d`) with a route end
    /// (`+0x5b2`, its own cell once stopped) on the map; else none.
    pub(super) fn at_will_route(&self, i: usize) -> Option<(i32, i32)> {
        let me = &self.objects[i];
        let s = me.ship.as_ref()?;
        let own = self.cell_of_object(i);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let human = me.owner.is_some_and(|(n, k)| self.is_human_id(n * 4 + k));
        let t = openconquest_formats::map::TILE_SIZE as f32;
        let end = s.path.back().map_or(own, |&(x, y)| {
            ((x / t).floor() as i32, (y / t).floor() as i32)
        });
        let end_index = w * end.1 + end.0;
        (human && end_index > 0 && end_index < w * h).then_some(end)
    }

    /// 0x4c0bc0 for object `k` the look found on `cell` (an enemy of a
    /// player in `+0x689`, there, not a ship on its reserved cell): a
    /// ship within 2 of ship `i`'s cell (0x557480) taking a contact
    /// (`+0x261 & 0x1000`), within 5 of the human's route end `route`,
    /// goes on the list, at most 50; none while ship `i` is on a ram or
    /// boarding (`+0x25d & 0x10`).
    pub(super) fn at_will_take(
        &self,
        i: usize,
        k: usize,
        cell: (i32, i32),
        route: Option<(i32, i32)>,
        list: &mut Vec<usize>,
    ) {
        if self.closing_in(i) {
            return;
        }
        let own = self.cell_of_object(i);
        if walk_path::dist2(own.0, own.1, cell.0, cell.1) > REACH {
            return;
        }
        if route.is_some_and(|end| walk_path::dist2(cell.0, cell.1, end.0, end.1) > ROUTE_REACH) {
            return;
        }
        if self.takes_contact(k) && list.len() < LIST {
            list.push(k);
        }
    }

    /// The end of a look with ram or board on and a list (0x4e8db0 mode
    /// 0x32 > 0, then 0x4c0b30): the record `{1 ram / 2 board, 1, id,
    /// 0xff, 0x46 / 0x45}` to itself (vt+4 event 0x10; its intake 0x4a9f2d
    /// keeps bytes 0-6, the order 0x4aa840 runs at the ship's next record
    /// pass), the ram's time `+0x708` now, the board's `+0x704`.
    pub(super) fn at_will_close(&mut self, i: usize, list: &[usize], ram: bool, board: bool) {
        if list.is_empty() {
            return;
        }
        let Some((target, board)) = self.at_will_pick(i, list, ram, board) else {
            return;
        };
        let now = self.ticks;
        let id = self.objects[i].id;
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
