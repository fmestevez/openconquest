//! The plain move `{100, 1}` of a human group: the human's move click
//! without Shift on a selection (0x49e230 case 0x14 -> 0x49de70 ->
//! 0x4ccc30). The selection's group holds the block, its ships each sent
//! to its cell of the formation about the click's cell (0x4cc970), and the
//! navy's tick (0x4c4f40 -> 0x4ccd80) sends those lying still back to
//! their cells every 101 ticks. See docs/FORMATS.md ("The plain move").

use super::gather::Notice;
use super::{Order, Scene};
use openconquest_formats::map::TILE_SIZE;
use serde::{Deserialize, Serialize};

/// The plain move block of a human group (0x65 bytes, 0x4cc500).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlainMove {
    /// Block `+8`: the tick it was made or rewritten (game `+0xde`).
    pub made: u32,
    /// Block `+0xc` / `+0x10`: the click's cell.
    pub to: (i32, i32),
    /// Block `+0x14`: the payload's third field (0x49de70: 1 when the
    /// group's block was null or a move's 100, else 0). Nothing of the
    /// move reads it.
    pub flags: i32,
    /// Block `+0x18`: the payload's fourth field (0 from 0x49de70).
    pub tol: i32,
    /// Block `+0x24`: each ship's cell in the formation (0x4cc7d0), by
    /// ship id; kept, and rewritten, while the block lasts.
    pub slots: Vec<(u32, (i32, i32))>,
    /// Group `+0x2d`: the notices not yet read, oldest first (made with
    /// the block, 0x4a1150).
    pub notices: Vec<Notice>,
}

/// 0x4ccd80's period: the ships are looked at once group `+8` is over
/// 100.
pub const PASS: u32 = 100;

impl Scene {
    /// The human's move click at cell `(x, y)` without Shift for the
    /// selection `ships` (0x49de70 -> 0x4ccc30), once for the selection.
    /// The selection's group (0x49e230: kept while the selection is
    /// unchanged, else a new one, 0x49ca60, its ships leaving their old
    /// groups); the payload `{x, y, 1, 0}` when its block is null or a
    /// move's 100, else `{x, y, 0, 0}` (0x49de70). 0x4ccc30: a block but
    /// `{100, 1}` ended (0x4a1c60: the group's serial, `+8` and behaviour
    /// kept, `end_selection_block`); with none, the block made (0x4cc500,
    /// `+8` now, `+0xc..+0x18` the payload, the notices 0x4a1150); a
    /// `{100, 1}` rewritten the same, its formation cells and notices
    /// kept. Then the formation planned (0x4cc970); when it sends none,
    /// each ship of the group sent to the cell (0x4cc650: command 0x14,
    /// priority byte 0xff).
    pub fn order_plain_move(&mut self, ships: &[u32], x: i32, y: i32) -> Result<(), String> {
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
        let g = match self.selection_group(&ships) {
            Some(g) => {
                // Null or `{100, 1}`: flags 1 (0x49de70).
                let pm = self.human_groups[g]
                    .plain_move
                    .get_or_insert_with(PlainMove::default);
                pm.flags = 1;
                g
            }
            None => {
                // A waypoint move (`{100, 2}`) or a new group's null
                // block gives 1; any other block 0.
                let flags = i32::from(self.selection_null_or_move(&ships));
                self.keep_selection_group(&ships);
                let g = self.selection_group(&ships).unwrap();
                self.human_groups[g].plain_move = Some(PlainMove {
                    flags,
                    ..Default::default()
                });
                g
            }
        };
        let now = self.ticks;
        let pm = self.human_groups[g].plain_move.as_mut().unwrap();
        pm.made = now;
        pm.to = (x, y);
        pm.tol = 0;
        if self.plain_move_formation(g) == 0 {
            let t = TILE_SIZE as f32;
            let (px, py) = (x as f32 * t + t / 2.0, y as f32 * t + t / 2.0);
            for id in self.human_groups[g].ships.clone() {
                if self.object_index(id).is_some() {
                    self.post(Order::MoveKeep {
                        ship: id,
                        x: px,
                        y: py,
                    });
                }
            }
        }
        Ok(())
    }

    /// Whether the selection `ships` is a waypoint move's group or no
    /// human block's (0x49ca60 makes it anew, its block null).
    fn selection_null_or_move(&self, ships: &[u32]) -> bool {
        self.selection_zone(ships).is_none()
            && self.selection_attack(ships).is_none()
            && self.selection_fleet(ships).is_none()
            && self.selection_guard(ships).is_none()
            && self.selection_found(ships).is_none()
    }

    /// 0x4cc970 for human group `g`'s plain move (`move_formation`).
    fn plain_move_formation(&mut self, g: usize) -> usize {
        let ships = self.human_groups[g].ships.clone();
        let pm = self.human_groups[g].plain_move.as_mut().unwrap();
        let to = pm.to;
        let mut slots = std::mem::take(&mut pm.slots);
        let sent = self.move_formation(&ships, to, &mut slots);
        self.human_groups[g].plain_move.as_mut().unwrap().slots = slots;
        sent
    }

    /// 0x4cc970: the group's ships `ships` each given its cell of the
    /// formation about the block's cell `to` (one ship, the cell itself;
    /// more, the planner `formation::plan` over the ships' grids by
    /// draught), noted in the block's `slots` (0x4cc7d0) and sent there
    /// (0x4cc8c0 -> 0x4cf070, command 0x14 priority 0). Returns the ships
    /// sent: 0 with none, or when the planner gives no cells.
    pub(super) fn move_formation(
        &mut self,
        ships: &[u32],
        to: (i32, i32),
        slots: &mut Vec<(u32, (i32, i32))>,
    ) -> usize {
        let t = TILE_SIZE as f32;
        let ships: Vec<(u32, crate::formation::Ship)> = ships
            .iter()
            .filter_map(|&id| {
                let o = &self.objects[self.object_index(id)?];
                let s = o.ship.as_ref()?;
                Some((
                    id,
                    crate::formation::Ship {
                        x: (o.x / t).floor() as i32,
                        y: (o.y / t).floor() as i32,
                        shallow: !crate::objects::deep_draught(s.kind),
                    },
                ))
            })
            .collect();
        let cells = match ships.len() {
            0 => return 0,
            1 => vec![to],
            _ => {
                let (w, h) = (self.nav.width as i32, self.nav.height as i32);
                let grid = |deep: bool| -> Vec<i16> {
                    (0..h)
                        .flat_map(|y| (0..w).map(move |x| (x, y)))
                        .map(|(x, y)| if self.nav.passable(x, y, deep) { 0 } else { -1 })
                        .collect()
                };
                let list: Vec<_> = ships.iter().map(|s| s.1).collect();
                match crate::formation::plan(&grid(true), &grid(false), (w, h), to, &list) {
                    Ok(c) => c,
                    Err(_) => return 0,
                }
            }
        };
        for (&(id, _), &cell) in ships.iter().zip(&cells) {
            match slots.iter_mut().find(|s| s.0 == id) {
                Some(s) => s.1 = cell,
                None => slots.push((id, cell)),
            }
        }
        let mut sent = 0;
        for ((id, _), cell) in ships.into_iter().zip(cells) {
            let (x, y) = (cell.0 as f32 * t + t / 2.0, cell.1 as f32 * t + t / 2.0);
            self.post(Order::Move { ship: id, x, y });
            sent += 1;
        }
        sent
    }

    /// The plain move's pass (0x4ccd80, from the navy's tick 0x4c4f40,
    /// each game tick) for human group `g`: first the notices, newest
    /// first, each taken off (0x4cc6a0): a join (300) of a ship of the
    /// group (0x4cc590) has the whole formation planned anew (0x4cc970),
    /// or, when that sends none, the ship sent to the block's cell
    /// (0x4cc650, priority kept) and the group's behaviour (0x4c42f0, sub
    /// 1 with `+0x19 / +0x1d / +0x21`: zeroes when never given); any
    /// other code dropped. When none was heard and group `+8` is over
    /// 100, `+8` goes back to 0 and each ship of the group: sent the
    /// group's behaviour again when its `+0x689 / +0x68d` are not the
    /// group's (group `+0x14` set); with a blow marked (`+0x691`) made to
    /// forget it (0x4cf760, command 0x28 sub 4; not ported: the blow's
    /// report 0x4a21c0, which the port makes as it lands); lying still
    /// (`+0x58c` 0) more than a cell off its cell in the block (in x or
    /// y; the block's cell without one, 0x4cc760), sent there again
    /// (0x4cf070). Then `+8` grows by the tick. The block's own code
    /// never sets its end flag `+0x5d` (0x4cc500, 0x4ccc30, 0x4ccd80,
    /// 0x4cc6a0, 0x4cc970, 0x4cc8c0): it lasts until another block
    /// replaces it (0x4a1c60) or the group goes.
    pub(super) fn plain_move_tick(&mut self, g: usize) {
        let t = TILE_SIZE as f32;
        let Some(pm) = self.human_groups[g].plain_move.as_mut() else {
            return;
        };
        let notes = std::mem::take(&mut pm.notices);
        let to = pm.to;
        let mut heard = 0;
        for n in notes.into_iter().rev() {
            if n.code != 300
                || !self.human_groups[g].ships.contains(&n.ship)
                || self.object_index(n.ship).is_none()
            {
                continue;
            }
            heard = self.plain_move_formation(g);
            if heard == 0 {
                self.post(Order::MoveKeep {
                    ship: n.ship,
                    x: to.0 as f32 * t + t / 2.0,
                    y: to.1 as f32 * t + t / 2.0,
                });
                let b = self.human_groups[g].behaviour.unwrap_or((0, 0, 0));
                self.send_behaviour(n.ship, b);
                heard += 1;
            }
        }
        if heard == 0 && self.human_groups[g].elapsed > PASS {
            self.human_groups[g].elapsed = 0;
            let pm = self.human_groups[g].plain_move.clone().unwrap();
            let behaviour = self.human_groups[g].behaviour;
            for id in self.human_groups[g].ships.clone() {
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
                let s = self.objects[i].ship.as_ref().unwrap();
                let (blow, still) = (s.blow.is_some(), s.move_state == 0);
                if blow {
                    self.post(Order::ClearBlow { ship: id });
                }
                if !still {
                    continue;
                }
                let cell = pm.slots.iter().find(|s| s.0 == id).map_or(pm.to, |s| s.1);
                let here = self.ship_cell(i);
                if (here.0 - cell.0).abs() > 1 || (here.1 - cell.1).abs() > 1 {
                    self.zone_move(id, cell.0, cell.1);
                }
            }
        }
        let g = &mut self.human_groups[g];
        g.elapsed = g.elapsed.wrapping_add(1);
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

    /// Two ships given a plain move on miss000 (0x49de70 -> 0x4ccc30): a
    /// new group with the block `{100, 1}` at the cell, flags 1, each ship
    /// sent to its own formation cell (0x4cc970 -> 0x4cf070); a second
    /// move rewrites it on the same group. The navy's walk runs it
    /// (0x4c4f40 -> 0x4ccd80): nothing until group `+8` is over 100, then
    /// `+8` back to 0 and each ship lying still more than a cell off its
    /// cell sent there again; one within a cell is not. A ship joining
    /// has the formation planned anew (300, 0x4cc6a0). The stop ends the
    /// block (0x4a1c60), the group kept.
    #[test]
    fn the_plain_move_runs_through_its_group_block() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(3)
            .collect();
        let t = TILE_SIZE as f32;
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let open = |scene: &Scene, c0: (i32, i32)| {
            (-5..=5).all(|dy| (-5..=5).all(|dx| water(scene, (c0.0 + dx, c0.1 + dy))))
        };
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let c0 = (6..h - 6)
            .flat_map(|y| (6..w - 6).map(move |x| (x, y)))
            .find(|&c| open(&scene, c))
            .unwrap();
        let at = [
            (c0.0 - 5, c0.1 - 5),
            (c0.0 - 5, c0.1 - 3),
            (c0.0 + 5, c0.1 + 5),
        ];
        for (&i, c) in ships.iter().zip(at) {
            let o = &mut scene.objects[i];
            o.owner = Some(human);
            o.x = c.0 as f32 * t + t / 2.0;
            o.y = c.1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.argo = false;
            s.move_state = 0;
            s.blow = None;
        }
        let ids: Vec<u32> = ships.iter().map(|&i| scene.objects[i].id).collect();
        let sel = vec![ids[0], ids[1]];
        scene.outbox.clear();
        scene.order_plain_move(&sel, c0.0, c0.1).unwrap();
        assert_eq!(scene.human_groups.len(), 1);
        let g = scene.human_groups[0].clone();
        let pm = g.plain_move.clone().unwrap();
        assert_eq!((g.ships.clone(), g.elapsed), (sel.clone(), 0));
        assert_eq!((pm.made, pm.to, pm.flags, pm.tol), (scene.ticks, c0, 1, 0));
        assert_eq!(pm.slots.len(), 2);
        assert_ne!(pm.slots[0].1, pm.slots[1].1);
        let centre = |c: (i32, i32)| (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
        let moves = |scene: &Scene| -> Vec<(u32, (f32, f32))> {
            scene
                .outbox
                .iter()
                .filter_map(|o| match *o {
                    Order::Move { ship, x, y } => Some((ship, (x, y))),
                    _ => None,
                })
                .collect()
        };
        let want: Vec<(u32, (f32, f32))> =
            pm.slots.iter().map(|&(id, c)| (id, centre(c))).collect();
        assert_eq!(moves(&scene), want);
        // A second move: the same group and block, rewritten.
        scene.outbox.clear();
        let c1 = (c0.0 + 1, c0.1);
        scene.order_plain_move(&sel, c1.0, c1.1).unwrap();
        assert_eq!(scene.human_groups.len(), 1);
        assert_eq!(scene.human_groups[0].serial, g.serial);
        let pm = scene.human_groups[0].plain_move.clone().unwrap();
        assert_eq!((pm.to, pm.slots.len()), (c1, 2));
        assert_eq!(moves(&scene).len(), 2);
        // The walk holds the group; its pass waits for `+8` over 100.
        assert!(scene.navy_walk_serials().contains(&g.serial));
        scene.outbox.clear();
        scene.human_groups[0].elapsed = PASS;
        scene.human_group_tick_for_test(g.serial);
        assert!(moves(&scene).is_empty());
        assert_eq!(scene.human_groups[0].elapsed, PASS + 1);
        // Past 100: ship 0 a cell off its cell stays, ship 1 far off goes.
        let slot = |id: u32| pm.slots.iter().find(|s| s.0 == id).unwrap().1;
        let s0 = slot(ids[0]);
        let o = &mut scene.objects[ships[0]];
        (o.x, o.y) = centre((s0.0 + 1, s0.1 - 1));
        scene.human_group_tick_for_test(g.serial);
        assert_eq!(moves(&scene), [(ids[1], centre(slot(ids[1])))]);
        assert_eq!(scene.human_groups[0].elapsed, 1);
        // A join: the formation planned anew for the three.
        scene.outbox.clear();
        scene.join_group(ids[0], ids[2]);
        assert_eq!(scene.human_groups[0].ships.len(), 3);
        let pm = scene.human_groups[0].plain_move.as_ref().unwrap();
        assert_eq!(pm.notices.len(), 1);
        scene.human_group_tick_for_test(g.serial);
        assert_eq!(moves(&scene).len(), 3);
        let pm = scene.human_groups[0].plain_move.as_ref().unwrap();
        assert!(pm.notices.is_empty());
        assert_eq!(pm.slots.len(), 3);
        // The stop ends the block, the group kept.
        let all = scene.human_groups[0].ships.clone();
        scene.keep_selection_group(&all);
        assert_eq!(scene.human_groups.len(), 1);
        assert!(scene.human_groups[0].plain_move.is_none());
        assert_eq!(scene.human_groups[0].serial, g.serial);
    }
}
