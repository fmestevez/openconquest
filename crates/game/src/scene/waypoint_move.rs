//! The waypoint move `{100, 2}`: the human's Shift move clicks on a
//! selection (0x49e230 case 0x14 -> 0x49de70 with key flag 4 -> 0x4cf2c0).
//! The selection's group keeps a route `+0x61` of the cells clicked, its
//! ships sent to the first and on along it at each arrival (notice 0x136,
//! 0x4cf0b0), those lying still sent on every 101 ticks (0x4cf550). See
//! docs/FORMATS.md ("The waypoint move").

use super::gather::{Notice, Owner, Route};
use super::{Order, Scene};
use serde::{Deserialize, Serialize};

/// A human group running the waypoint move: the group's own fields the
/// block reads and the block (0x65 bytes, 0x4cef80).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaypointMove {
    /// The group's serial number (shared with the other human groups).
    pub serial: u32,
    /// Group `+0x31`: its ships, by id.
    pub ships: Vec<u32>,
    /// Group `+0x2d`: the notices not yet read, oldest first.
    pub notices: Vec<Notice>,
    /// Group `+8`: the block's tick counter.
    pub elapsed: u32,
    /// Block `+8`: the tick it was made or rewritten (game `+0xde`).
    pub made: u32,
    /// Block `+0xc` / `+0x10`: the last click's cell (0x49de70's payload
    /// `{x, y, 1, 0, 1, &cell}`).
    pub cell: (i32, i32),
    /// Block `+0x14`: the way 0x4ce8a0 gives the ships it sends: 1 for
    /// the Shift move, 2 (round) for the patrol.
    pub way: i8,
    /// Block `+0x61`.
    pub route: Route,
    /// Group `+0x14..+0x24` (`HumanBehaviour`).
    #[serde(default)]
    pub behaviour: super::gather::HumanBehaviour,
}

/// 0x4cf550's period: the ships lying still are sent on once group `+8`
/// is over 100.
pub const PASS: u32 = 100;

impl Scene {
    /// The human's Shift move click at cell `(x, y)` for the selection
    /// `ships` (0x49de70 with key flag 4), once for the selection. On a
    /// group whose block is neither null nor a move's 100 (a gather zone,
    /// the fleet attack task) the block's route is made if null and the
    /// cell appended unless listed (0x4ce6c0), no ship sent (0x4cec00
    /// needs the cell listed in a route just made: never); the human's
    /// attack block `{0x70, 0x20}` and the guard block `{0x6e, 0x29}`
    /// alike (their code never reads the route: the cells are only
    /// marked), the group kept. Else 0x4cf2c0 (payload
    /// `{x, y, 1, 0, 1, &cell}`, place 1, from the group 0): a move block
    /// of another kind (100 / 1) ended (0x4a1c60); the group's 100 / 2
    /// rewritten (`+8` now, `+0xc`-`+0x18` the payload) and the cell
    /// appended to its route (0x4ce6c0), no ship sent, the route not
    /// being new; else the block made (0x4cef80, its route 0x4ce5e0 with
    /// the cell, the notices 0x4a1150) on the selection's group (kept
    /// with its serial, `+8` and behaviour when the selection is a bare
    /// group's, `HumanGroup`, its plain move `{100, 1}` ended; else a new one, 0x49ca60, its ships leaving their old groups) and
    /// each ship sent to the route's
    /// first waypoint (0x4cf070, a plain move) with its progress `(-1,
    /// -1)`, way `+0x14` (0x4ce8a0). Each waypoint appended gets its mark
    /// (`Route::append`).
    pub fn order_shift_move(&mut self, ships: &[u32], x: i32, y: i32) -> Result<(), String> {
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
        if self.selection_zone(&ships).is_some() {
            return self.order_zone_waypoint(&ships, x, y);
        }
        if let Some(z) = self.selection_fleet(&ships) {
            self.fleet_attacks[z]
                .route
                .get_or_insert_with(Route::default)
                .append(x, y);
            return Ok(());
        }
        if let Some(z) = self.selection_attack(&ships) {
            self.human_attacks[z]
                .route
                .get_or_insert_with(Route::default)
                .append(x, y);
            return Ok(());
        }
        if let Some(z) = self.selection_guard(&ships) {
            self.human_guards[z]
                .route
                .get_or_insert_with(Route::default)
                .append(x, y);
            return Ok(());
        }
        if let Some(z) = self.selection_found(&ships) {
            self.human_founds[z]
                .route
                .get_or_insert_with(Route::default)
                .append(x, y);
            return Ok(());
        }
        let now = self.ticks;
        if let Some(z) = self.selection_waypoint_move(&ships) {
            let m = &mut self.waypoint_moves[z];
            m.made = now;
            m.cell = (x, y);
            m.way = 1;
            m.route.append(x, y);
            return Ok(());
        }
        let (serial, elapsed, behaviour) = match self.end_selection_block(&ships) {
            Some((serial, elapsed, _, behaviour)) => (serial, elapsed, behaviour),
            None => {
                self.leave_zones(&ships);
                self.zone_serial += 1;
                (self.zone_serial, 0, None)
            }
        };
        let mut route = Route::default();
        route.append(x, y);
        self.waypoint_moves.push(WaypointMove {
            serial,
            ships: ships.clone(),
            notices: Vec::new(),
            elapsed,
            made: now,
            cell: (x, y),
            way: 1,
            route,
            behaviour,
        });
        let z = self.waypoint_moves.len() - 1;
        let (fx, fy) = self.waypoint_moves[z].route.waypoints[0];
        for id in ships {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            self.zone_move(id, fx, fy);
            self.route_progress(Owner::Move(z), i, -1, -1, false, 1);
        }
        Ok(())
    }

    /// The patrol button's click (0x6e `CBUT_BEHPATROL`: command 0x23
    /// sub 8 with the cell, 0x49d790 case 8) for the selection `ships`.
    /// On a group whose block is a waypoint move of way `+0x14` 2 the
    /// route made if null (0x4ce5e0) and the cell appended unless listed,
    /// its mark made but not placed (0x4ce6c0 place 0, -1); no ship sent
    /// (0x4cec00 needs the cell listed in a route just made: never).
    /// Else the block ended (0x4a1c60) and 0x4cf2c0(payload `{x, y, 2, 0,
    /// 1, &cell}`, place 0, centre 1): the block made (0x4cef80, its route
    /// with the cell, not placed); the group's centre (0x4c56e0) put
    /// first (0x4ce6c0 at 0, not placed) unless listed; each ship sent to
    /// the route's second waypoint (0x4cf070) with progress `(-1, -1)`,
    /// way 2 (0x4ce8a0). The way 2 goes round the route at its ends
    /// (0x4ce7c0) and keeps a ship's progress at the end (0x4ce8a0).
    /// With the centre the click's cell the route holds one waypoint and
    /// 0x4cf2c0 reads the entry past it from a null pointer (the
    /// original's fault): here no ship is sent. The selection's group
    /// (0x49e230) is kept with its serial and counter `+8`, whatever its
    /// block (`end_selection_block`: a gather zone, the human's attack,
    /// the fleet attack, a waypoint move), its route freed; else a new
    /// one, its ships leaving their old groups.
    pub fn order_patrol_route(&mut self, ships: &[u32], x: i32, y: i32) -> Result<(), String> {
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
        let old = self.selection_waypoint_move(&ships);
        if let Some(z) = old.filter(|&z| self.waypoint_moves[z].way == 2) {
            self.waypoint_moves[z].route.add(x, y, false, None);
            return Ok(());
        }
        let (serial, elapsed, behaviour) = match self.end_selection_block(&ships) {
            Some((serial, elapsed, _, behaviour)) => (serial, elapsed, behaviour),
            None => {
                self.leave_zones(&ships);
                self.zone_serial += 1;
                (self.zone_serial, 0, None)
            }
        };
        let mut route = Route::default();
        route.add(x, y, false, None);
        // 0x4c56e0: the mean of the ships' cells (each weight 1; a
        // building's, weight 2, never in the human's groups, which would
        // test the centre at sea, 0x499990).
        let cells: Vec<(i32, i32)> = ships
            .iter()
            .filter_map(|&id| self.object_index(id))
            .map(|i| self.ship_cell(i))
            .collect();
        let n = cells.len() as i32;
        if n != 0 {
            let cx = cells.iter().map(|c| c.0).sum::<i32>() / n;
            let cy = cells.iter().map(|c| c.1).sum::<i32>() / n;
            route.add(cx, cy, false, Some(0));
        }
        self.waypoint_moves.push(WaypointMove {
            serial,
            ships: ships.clone(),
            notices: Vec::new(),
            elapsed,
            made: self.ticks,
            cell: (x, y),
            way: 2,
            route,
            behaviour,
        });
        let z = self.waypoint_moves.len() - 1;
        let Some(&(fx, fy)) = self.waypoint_moves[z].route.waypoints.get(1) else {
            return Ok(());
        };
        for id in ships {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            self.zone_move(id, fx, fy);
            self.route_progress(Owner::Move(z), i, -1, -1, false, 2);
        }
        Ok(())
    }

    /// The waypoint move group whose ships are exactly `ships`.
    pub fn selection_waypoint_move(&self, ships: &[u32]) -> Option<usize> {
        let mut sel: Vec<u32> = ships.to_vec();
        sel.sort_unstable();
        sel.dedup();
        self.waypoint_moves.iter().position(|m| {
            let mut s = m.ships.clone();
            s.sort_unstable();
            !s.is_empty() && s == sel
        })
    }

    /// The waypoint move group holding ship `id`.
    pub fn ship_waypoint_move(&self, id: u32) -> Option<usize> {
        self.waypoint_moves
            .iter()
            .position(|m| m.ships.contains(&id))
    }

    /// Ships leaving their waypoint move groups post notice 0x137
    /// (0x4a1770; 0x4cf0b0 drops it); a group left with none goes, its
    /// route and marks with it (0x4ce5a0).
    pub(super) fn leave_waypoint_moves(&mut self, ids: &[u32]) {
        let tick = self.ticks;
        for m in &mut self.waypoint_moves {
            for &s in &m.ships {
                if ids.contains(&s) {
                    m.notices.push(Notice {
                        code: 0x137,
                        tick,
                        ship: s,
                        class: 0,
                    });
                }
            }
            m.ships.retain(|s| !ids.contains(s));
        }
        self.waypoint_moves.retain(|m| !m.ships.is_empty());
    }

    /// Ships sunk or taken leave their groups (0x4a8410 -> 0x49bef0),
    /// before the navy's walk.
    pub(super) fn drop_gone_waypoint_moves(&mut self) {
        let gone: Vec<u32> = self
            .waypoint_moves
            .iter()
            .flat_map(|m| m.ships.iter().copied())
            .filter(|&id| {
                self.object_index(id)
                    .is_none_or(|i| self.objects[i].ship.is_none())
            })
            .collect();
        if !gone.is_empty() {
            self.leave_waypoint_moves(&gone);
        }
    }

    /// The navy's tick for the waypoint move groups (0x4cf550 each, from
    /// 0x4c4f40).
    #[cfg(test)]
    pub(super) fn run_waypoint_moves(&mut self) {
        self.drop_gone_waypoint_moves();
        for z in 0..self.waypoint_moves.len() {
            self.wm_tick(z);
        }
    }

    /// 0x4cf550: with no notices, or none heard (0x4cf0b0; it hears only
    /// a 300 with no route, which this block always has), and group `+8`
    /// over 100 (unsigned): `+8 = 0` and each ship of the group: a blow
    /// marked (`+0x691`) forgotten (0x4cf760, command 0x28 sub 4; its
    /// report 0x4a21c0 to the brain the port makes as the blow lands);
    /// lying still (`+0x58c` 0) and on the route (0x4cea20 at or above
    /// 0): off its entry (`(-1, -1)`, read as cell 0) 0x4ceab0 with cell
    /// -1, whose `closer` reads the word before its wavefront (a heap
    /// header, not the game's: not known, nothing sent); else the
    /// waypoint after the one it was sent to, its way (0x4ce7c0), not
    /// past the end and inside the map above cell 0: a plain move there
    /// (0x4cf070) and, that waypoint its own cell, its progress set to it
    /// (0x4ce8a0, which posts its 0x136). Then `+8` grows by the tick's
    /// `+0xe2` (1). No draws.
    pub(super) fn wm_tick(&mut self, z: usize) {
        if !self.waypoint_moves[z].notices.is_empty() {
            self.wm_notices(z);
        }
        let w = self.nav.width as i32;
        let n = w * self.nav.height as i32;
        if self.waypoint_moves[z].elapsed > PASS {
            self.waypoint_moves[z].elapsed = 0;
            for id in self.waypoint_moves[z].ships.clone() {
                let Some(i) = self.object_index(id) else {
                    continue;
                };
                let Some(s) = self.objects[i].ship.as_ref() else {
                    continue;
                };
                if s.blow.is_some() {
                    self.post(Order::ClearBlow { ship: id });
                }
                if self.objects[i].ship.as_ref().unwrap().move_state != 0 {
                    continue;
                }
                let (k, dir) = self.route_at(Owner::Move(z), id);
                if k <= 0 {
                    continue;
                }
                let (nx, end) = self.route_next(Owner::Move(z), k % w, k / w, dir);
                if !end && 0 < nx && nx < n {
                    self.zone_move(id, nx % w, nx / w);
                    if (nx % w, nx / w) == self.ship_cell(i) {
                        self.route_progress(Owner::Move(z), i, nx % w, nx / w, false, 0);
                    }
                }
            }
        }
        let m = &mut self.waypoint_moves[z];
        m.elapsed = m.elapsed.wrapping_add(1);
    }

    /// 0x4cf0b0: the notices, newest first, each removed after its case.
    /// (Not ported: 300, a ship joining, which the port posts to no human
    /// group.) 0x136 (a ship at its move goal, `class` the cell): with
    /// the ship in the group (0x4cc590) and either others in it or the
    /// cell its own (0x4a0230: the ship alone in its group, 0x49c100,
    /// takes only its own cell, the game screen's `navy +0x20` being 0),
    /// the waypoint after that cell its way (0x4cea20, 0x4ce7c0): inside
    /// the map above cell 0, a plain move there (0x4cf070) and its
    /// progress set to it (0x4ce8a0 with the end flag as `remove`, mark
    /// 0). Past the end: nothing, the ship staying. Others dropped.
    fn wm_notices(&mut self, z: usize) {
        let w = self.nav.width as i32;
        let n = w * self.nav.height as i32;
        // The notices posted meanwhile (0x4ce8a0's) wait for the next tick.
        let notes = std::mem::take(&mut self.waypoint_moves[z].notices);
        for note in notes.into_iter().rev() {
            if note.code != 0x136 || !self.waypoint_moves[z].ships.contains(&note.ship) {
                continue;
            }
            let Some(i) = self.object_index(note.ship) else {
                continue;
            };
            if self.objects[i].ship.is_none() {
                continue;
            }
            let c = note.class as i32;
            let (sx, sy) = self.ship_cell(i);
            if self.waypoint_moves[z].ships.len() == 1 && c != sy * w + sx {
                continue;
            }
            let (_, dir) = self.route_at(Owner::Move(z), note.ship);
            let (nx, end) = self.route_next(Owner::Move(z), c % w, c / w, dir);
            if 0 < nx && nx < n {
                self.zone_move(note.ship, nx % w, nx / w);
                self.route_progress(Owner::Move(z), i, nx % w, nx / w, end, 0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::GameData;
    use crate::objects::Caches;
    use openconquest_formats::map::TILE_SIZE;

    /// Two of the human's ships on miss000 given Shift move clicks at
    /// three cells (0x49de70 -> 0x4cf2c0): the first makes the block and
    /// sends both to it, progress `(-1, -1)` way 1; the others are only
    /// appended, a cell listed not again; each waypoint's mark `WPMOVE`
    /// numbered by its place at `(x*64 + 14, y*64 + 18)`, hidden at a new
    /// selection and shown with Shift (0x4a2050). An arrival (0x136)
    /// sends the ship on (0x4cf0b0), the pass sends a ship lying still
    /// on from the waypoint it was bound for (0x4cf550). A Shift attack
    /// then takes the route into the fleet attack (0x4d04a0): the ships
    /// on the route, their way not the count less 1, are only held
    /// (0x4cfe40); at the route's end the held command is given
    /// (0x4d0210's 0x136, 0x4d0090).
    #[test]
    fn shift_move_route_on_miss000() {
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
            .take(3)
            .collect();
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let row = (3..h - 3)
            .flat_map(|y| (3..w - 12).map(move |x| (x, y)))
            .find(|&(x, y)| (0..10).all(|k| water(&scene, (x + k, y))))
            .unwrap();
        let t = TILE_SIZE as f32;
        for (k, &i) in ships.iter().enumerate() {
            let o = &mut scene.objects[i];
            o.owner = Some(if k < 2 { human } else { foe });
            o.x = (row.0 + [0, 1, 9][k]) as f32 * t + t / 2.0;
            o.y = row.1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.argo = false;
            s.move_state = 0;
        }
        let id = |k: usize| scene.objects[ships[k]].id;
        let (id0, id1, f) = (id(0), id(1), id(2));
        let sel = [id0, id1];
        let (a, b, c) = ((row.0 + 3, row.1), (row.0 + 5, row.1), (row.0 + 7, row.1));
        let mv = |ship, p: (i32, i32)| Order::Move {
            ship,
            x: p.0 as f32 * t + t / 2.0,
            y: p.1 as f32 * t + t / 2.0,
        };
        scene.outbox.clear();
        scene.order_shift_move(&sel, a.0, a.1).unwrap();
        assert_eq!(scene.waypoint_moves.len(), 1);
        assert_eq!(std::mem::take(&mut scene.outbox), [mv(id0, a), mv(id1, a)]);
        let m = &scene.waypoint_moves[0];
        assert_eq!((m.cell, m.way, m.route.count), (a, 1, 2));
        assert!(m
            .route
            .progress
            .iter()
            .all(|p| p.at == (-1, -1) && p.dir == 1));
        scene.order_shift_move(&sel, b.0, b.1).unwrap();
        scene.order_shift_move(&sel, c.0, c.1).unwrap();
        scene.order_shift_move(&sel, b.0, b.1).unwrap();
        assert!(scene.outbox.is_empty());
        assert_eq!(scene.waypoint_moves[0].route.waypoints, [a, b, c]);
        let place = |p: (i32, i32), n| (p.0 * 64 + 14, p.1 * 64 + 18, n);
        let all = [place(a, 1), place(b, 2), place(c, 3)];
        assert_eq!(scene.route_marks(), all);
        // WPMOVE's picture and the number's two digits, each mark.
        assert_eq!(scene.route_mark_layers().len(), 9);
        scene.fa_selection_seen(&sel);
        scene.fa_selection_seen(&[id0]);
        assert!(scene.route_marks().is_empty());
        scene.fa_marks_shown(&sel, true);
        assert_eq!(scene.route_marks(), all);
        // At the first waypoint: on to the second.
        scene.post_notice(id0, 0x136, (a.1 * w + a.0) as u32);
        scene.run_waypoint_moves();
        assert_eq!(std::mem::take(&mut scene.outbox), [mv(id0, b)]);
        let at = |scene: &Scene, ship| {
            let m = &scene.waypoint_moves[0];
            m.route.progress.iter().find(|p| p.ship == ship).unwrap().at
        };
        assert_eq!(at(&scene, id0), b);
        // The pass: id1 lying still, bound for the first, sent on to the
        // second; id0 bound for the second sent on to the third.
        let m = &mut scene.waypoint_moves[0];
        m.route
            .progress
            .iter_mut()
            .find(|p| p.ship == id1)
            .unwrap()
            .at = a;
        m.elapsed = PASS + 1;
        scene.run_waypoint_moves();
        assert_eq!(std::mem::take(&mut scene.outbox), [mv(id0, c), mv(id1, b)]);
        assert_eq!(scene.waypoint_moves[0].elapsed, 1);
        assert_eq!((at(&scene, id0), at(&scene, id1)), (b, a));
        // A Shift attack takes the route: both on it, way 1, not the
        // count less 1 (2): held, nothing given.
        scene.order_fleet_attack(&sel, f, 0x16, 4).unwrap();
        assert!(scene.waypoint_moves.is_empty());
        let fa = &scene.fleet_attacks[0];
        assert_eq!(fa.route.as_ref().unwrap().waypoints, [a, b, c]);
        assert_eq!(fa.held.len(), 2);
        assert!(scene.outbox.is_empty());
        assert_eq!(scene.route_marks(), all);
        // At the second: on to the third; at the third, past the end:
        // off the route, the held command given.
        scene.post_notice(id0, 0x136, (b.1 * w + b.0) as u32);
        scene.run_fleet_attacks();
        assert_eq!(std::mem::take(&mut scene.outbox), [mv(id0, c)]);
        scene.post_notice(id0, 0x136, (c.1 * w + c.0) as u32);
        scene.run_fleet_attacks();
        let attack = Order::Attack {
            ship: id0,
            target: f,
        };
        assert_eq!(std::mem::take(&mut scene.outbox), [attack]);
        let fa = &scene.fleet_attacks[0];
        assert_eq!(fa.held.len(), 1);
        assert!(fa
            .route
            .as_ref()
            .unwrap()
            .progress
            .iter()
            .all(|p| p.ship != id0));
    }

    /// A Shift gather click (0x49d3c0 -> 0x4c8a80, key flag 4) on a
    /// waypoint move's group: the block's route taken (`local_8`) into
    /// the new zone's block, its marks kept, the group's serial and
    /// counter kept, no ship sent (0x4c5f40 / 0x4c6ee0 skipped). Without
    /// Shift the route is freed (0x4ce5a0).
    #[test]
    fn shift_gather_keeps_the_move_route() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(2)
            .collect();
        for &i in &ships {
            scene.objects[i].owner = Some(human);
        }
        let sel: Vec<u32> = ships.iter().map(|&i| scene.objects[i].id).collect();
        let deposit = (0..scene.objects.len())
            .find(|&i| super::super::gather::deposit_class(&scene.objects[i]).is_some())
            .map(|i| scene.objects[i].id)
            .unwrap();
        for keys in [4, 0] {
            scene.gather_zones.clear();
            scene.order_shift_move(&sel, 3, 4).unwrap();
            scene.order_shift_move(&sel, 5, 6).unwrap();
            scene.waypoint_moves[0].elapsed = 17;
            let serial = scene.waypoint_moves[0].serial;
            scene.outbox.clear();
            scene.order_gather_at(&sel, deposit, keys).unwrap();
            assert!(scene.waypoint_moves.is_empty());
            assert_eq!(scene.gather_zones.len(), 1);
            let z = &scene.gather_zones[0];
            let route = z.route.as_ref().map(|r| r.waypoints.clone());
            if keys == 4 {
                assert_eq!(z.group, super::super::gather::ZoneGroup::Human(serial));
                assert_eq!(z.elapsed, 17);
                assert_eq!(route, Some(vec![(3, 4), (5, 6)]));
                assert_eq!(scene.route_marks().len(), 2);
                assert!(scene.outbox.is_empty());
            } else {
                assert_eq!(route, None);
            }
        }
    }

    /// The patrol button's click (0x49d790 case 8 -> 0x4cf2c0 with place
    /// 0, centre 1) for two ships on miss000: the route the click's cell
    /// with the group's centre (0x4c56e0) put first, their marks made
    /// unplaced (the centre numbered 2, made second), both ships sent to
    /// the click's cell, way 2; at the route's end an arrival (0x136)
    /// goes round to the first waypoint, the progress kept. A second
    /// click on the patrol only appends its cell, no ship sent; a Shift
    /// move rewrites the way to 1.
    #[test]
    fn patrol_route_on_miss000() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(2)
            .collect();
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let row = (3..h - 3)
            .flat_map(|y| (3..w - 12).map(move |x| (x, y)))
            .find(|&(x, y)| (0..10).all(|k| water(&scene, (x + k, y))))
            .unwrap();
        let t = TILE_SIZE as f32;
        for (k, &i) in ships.iter().enumerate() {
            let o = &mut scene.objects[i];
            o.owner = Some(human);
            o.x = (row.0 + [0, 3][k]) as f32 * t + t / 2.0;
            o.y = row.1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.argo = false;
            s.move_state = 0;
        }
        let sel: Vec<u32> = ships.iter().map(|&i| scene.objects[i].id).collect();
        let (id0, id1) = (sel[0], sel[1]);
        let mv = |ship, p: (i32, i32)| Order::Move {
            ship,
            x: p.0 as f32 * t + t / 2.0,
            y: p.1 as f32 * t + t / 2.0,
        };
        // The centre: (x0 + x0 + 3) / 2 = x0 + 1.
        let (centre, c, d) = ((row.0 + 1, row.1), (row.0 + 7, row.1), (row.0 + 9, row.1));
        scene.outbox.clear();
        scene.order_patrol_route(&sel, c.0, c.1).unwrap();
        let m = &scene.waypoint_moves[0];
        assert_eq!((m.way, m.cell), (2, c));
        assert_eq!(m.route.waypoints, [centre, c]);
        assert!(m
            .route
            .progress
            .iter()
            .all(|p| p.dir == 2 && p.at == (-1, -1)));
        assert_eq!(std::mem::take(&mut scene.outbox), [mv(id0, c), mv(id1, c)]);
        assert!(scene.route_marks().is_empty());
        scene.fa_marks_shown(&sel, true);
        let place = |p: (i32, i32), n| (p.0 * 64 + 14, p.1 * 64 + 18, n);
        assert_eq!(scene.route_marks(), [place(centre, 2), place(c, 1)]);
        // At the end: round to the first, the progress kept.
        scene.post_notice(id0, 0x136, (c.1 * w + c.0) as u32);
        scene.run_waypoint_moves();
        assert_eq!(std::mem::take(&mut scene.outbox), [mv(id0, centre)]);
        let m = &scene.waypoint_moves[0];
        let p = m.route.progress.iter().find(|p| p.ship == id0).unwrap();
        assert_eq!((p.at, p.dir), (centre, 2));
        // Another click: appended, its mark not placed, nothing sent.
        scene.order_patrol_route(&sel, d.0, d.1).unwrap();
        let m = &scene.waypoint_moves[0];
        assert_eq!(m.route.waypoints, [centre, c, d]);
        assert!(scene.outbox.is_empty());
        assert_eq!(scene.route_marks().len(), 2);
        scene.order_shift_move(&sel, row.0 + 5, row.1).unwrap();
        assert_eq!(scene.waypoint_moves[0].way, 1);
    }

    /// The patrol button (0x49d790 case 8) and a Shift gather (0x4c8a80,
    /// key flag 4) on the selection's group (0x49e230): its block ended
    /// (0x4a1c60) but the group kept, its serial and counter `+8` with
    /// it. A fleet attack's group given the Shift gather keeps them as
    /// its route goes into the zone; a gather zone's given the patrol.
    /// The behaviour page (0x4e6980 page 5) never shows the patrol set.
    #[test]
    fn patrol_and_shift_gather_keep_the_group() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(2)
            .collect();
        for &i in &ships {
            scene.objects[i].owner = Some(human);
            scene.objects[i].ship.as_mut().unwrap().kind = 2;
        }
        let sel: Vec<u32> = ships.iter().map(|&i| scene.objects[i].id).collect();
        // The first enemy the attack takes (a refusal changes nothing).
        let ids: Vec<u32> = scene.objects.iter().map(|o| o.id).collect();
        assert!(ids
            .into_iter()
            .any(|t| scene.order_fleet_attack(&sel, t, 0x15, 0).is_ok()));
        scene.order_shift_move(&sel, 3, 4).unwrap();
        scene.fleet_attacks[0].elapsed = 23;
        let serial = scene.fleet_attacks[0].serial;
        let deposit = (0..scene.objects.len())
            .find(|&i| super::super::gather::deposit_class(&scene.objects[i]).is_some())
            .map(|i| scene.objects[i].id)
            .unwrap();
        scene.order_gather_at(&sel, deposit, 4).unwrap();
        assert!(scene.fleet_attacks.is_empty());
        let z = &scene.gather_zones[0];
        assert_eq!(z.group, super::super::gather::ZoneGroup::Human(serial));
        assert_eq!(z.elapsed, 23);
        assert_eq!(
            z.route.as_ref().map(|r| r.waypoints.clone()),
            Some(vec![(3, 4)])
        );
        scene.gather_zones[0].elapsed = 31;
        let (x, y) = scene.ship_cell(ships[0]);
        scene.order_patrol_route(&sel, x + 2, y).unwrap();
        assert!(scene.gather_zones.is_empty());
        let m = &scene.waypoint_moves[0];
        assert_eq!((m.serial, m.elapsed, m.way), (serial, 31, 2));
        let menu = crate::commands::menu(
            &scene,
            &scene.objects[ships[0]],
            crate::commands::Page::Behaviour,
        );
        let at = |k: usize| menu[k].map(|s| (s.command, s.enabled, s.checked));
        assert_eq!(at(8), Some((crate::commands::PATROL, true, false)));
        assert_eq!(at(9), Some((crate::commands::GUARD_ZONE, true, false)));
        assert_eq!(at(10), Some((crate::commands::ATTACK_ZONE, true, false)));
        // Type 10 (0x4e42e0): no guard nor board attack; fire on
        // aggressor off even while guarding.
        let s = scene.objects[ships[0]].ship.as_mut().unwrap();
        s.kind = 10;
        s.behaviour.guard = true;
        let menu = crate::commands::menu(
            &scene,
            &scene.objects[ships[0]],
            crate::commands::Page::Behaviour,
        );
        assert!(menu[0].is_none() && menu[2].is_none() && menu[1].is_some());
        assert!(menu[5].is_some_and(|s| !s.enabled && !s.checked));
    }
}
