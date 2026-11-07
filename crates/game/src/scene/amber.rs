//! A ship's amber gather (Run_Ship.cpp 0x4b0590: 0x4ac2d0's sub-codes 10
//! and 11 for class 400), a game tick at a time, its step `+0x77d`:
//!
//! - **1**: the cell `+0x797` holding no amber (class 400), another in the
//!   box `+0x7c2..+0x7ce` (0x4ac610 mode 1) is sent for (event 0x40 {400,
//!   10, its id, priority 0x5a}), none and the order ends (event 8, 0);
//!   away from the seat `+0x792` (0x4afb80) it sails there (0x4a6270);
//!   there, lying still (`+0x58c` 0), with no room (0x4be800 mode 3) the
//!   order ends (event 8, 0x40), else the close-in 0x4afe50(ship, 0) asked
//!   each tick: holding, step 10 and the commands held (0x4b37d0 with
//!   0x2000, 0x4b067f).
//! - **10** (0x4b0470(0) -> 0x4b02a0): the slide, two points a tick; at its
//!   end step 0xb; called off and back at its start, the commands back
//!   (0x4b3850) and event 8 0xc0.
//! - **0xb** (0x4b89b0): the sail furls a frame a hull step; furled, the
//!   walkway (slot 1, 0x3e6) at the ship frame's top-left plus 0x599e88 /
//!   0x59a128 / 0x59a408 / 0x59a468 / 0x59a708 / 0x59a768 and step 0xc.
//! - **0xc**: the walkway a frame a tick; at its last the porters (slot 3,
//!   0x3de) on its top-left and step 0xf; called off, down a frame a tick
//!   and below 1 step 5.
//! - **0xf** (0x4afdd0): the cell's amber still the order's and standing
//!   (`+0x74` 0), a take (0x4ad0e0) into `+0x782` and, not called off,
//!   step 6; else the porters gone and step 0xe.
//! - **6**: called off, the porters gone and step 0xe; else their frame a
//!   tick (the map's `+0xe2`); at the last, frame 0, step 0xf and the take
//!   stored (0x4ad300 class 2): full, the porters gone and step 0xe.
//! - **0xe**: the walkway down a frame a tick; below 0, step 5.
//! - **5**: two points back along the line a hull step, unfurling; at its
//!   start the hull on its cell's middle, the commands back (0x4b3850),
//!   notice 0x138 to its group (0x4a1430, class 400) and the order ends
//!   (event 8 0xc0).
//!
//! The close-in, slide, furl, walkway and back-out are the treasure take's
//! (`Scene::take_close_in`, `Scene::take_landing_tick`): the machine runs
//! in the ship's `Landing` (its `state` the step), whose presence holds
//! the commands (`Scene::landing_holds`).

use super::*;
use crate::objects::{deep_draught, GatherPhase, Resource, Task};

impl Scene {
    /// The deposit of ship `i`'s amber order while 0x4b0590 runs it (the
    /// gather order of class 400, sailing to its amber or at it).
    pub(super) fn amber_order(&self, i: usize) -> Option<u32> {
        let s = self.objects[i].ship.as_ref()?;
        let Some(Task::Gather { deposit, phase }) = s.task else {
            return None;
        };
        let amber = s.gathered.is_some_and(|g| g.class == Resource::Amber);
        (amber
            && s.hunt.is_none()
            && matches!(phase, GatherPhase::ToDeposit | GatherPhase::Collecting))
        .then_some(deposit)
    }

    /// Every ship's amber gather, a game tick (0x4b0590).
    pub(super) fn run_amber(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = (0..self.objects.len())
            .filter(|&i| self.amber_order(i).is_some())
            .map(|i| self.objects[i].id)
            .collect();
        for id in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let Some(deposit) = self.amber_order(i) else {
                continue;
            };
            if self.objects[i].ship.as_ref().unwrap().landing.is_some() {
                self.amber_landing_tick(i, deposit, data, caches);
            } else {
                self.amber_step_one(i);
            }
        }
    }

    /// 0x4afb80(ship, amber, &seat, 1, -1): of the amber cell's straight
    /// neighbours, east, west, south and north (each on the map), those
    /// open to the ship (0x499aa0: the port reads its draught's grid; the
    /// cell's object slot and the heading's corner tests not ported), one
    /// taken as it is, more the nearest by 0x557440 from the ship's cell
    /// (strict `<`, the first on a tie; the map's cell 0 passed over, sic).
    /// None: no seat.
    pub(super) fn amber_seat(&self, i: usize, cell: (i32, i32)) -> Option<(i32, i32)> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let total = w * h;
        let c = cell.1 * w + cell.0;
        let deep = deep_draught(self.objects[i].ship.as_ref()?.kind);
        let open = |n: i32| self.nav.passable(n % w, n / w, deep);
        let mut seats = Vec::new();
        if c + 1 < total && cell.0 + 1 < w && open(c + 1) {
            seats.push(c + 1);
        }
        if c > 0 && cell.0 > 0 && open(c - 1) {
            seats.push(c - 1);
        }
        if c + w < total && open(c + w) {
            seats.push(c + w);
        }
        if c - w >= 0 && c - w < total && open(c - w) {
            seats.push(c - w);
        }
        let pick = match seats.len() {
            0 => return None,
            1 => seats[0],
            _ => {
                let here = self.cell_of(i);
                let mut best = (20_000_000, None);
                for &n in &seats {
                    if n == 0 {
                        continue;
                    }
                    let d = super::placement::distance(n % w - here.0, n / w - here.1);
                    if d < best.0 {
                        best = (d, Some(n));
                    }
                }
                best.1?
            }
        };
        Some((pick % w, pick / w))
    }

    /// 0x4b0590 step 1 for ship `i`.
    fn amber_step_one(&mut self, i: usize) {
        let s = self.objects[i].ship.as_ref().unwrap();
        let Some(g) = s.gathered else {
            return;
        };
        let id = self.objects[i].id;
        let here = self
            .layer_deposit(g.cell)
            .filter(|&d| self.objects[d].class == ObjectClass::Amber);
        let Some(d) = here else {
            match self.box_deposit(i, g.rect, Resource::Amber, 1, -1) {
                Some(n) => {
                    let n = self.objects[n].id;
                    if self.order_gather_with(id, n, 0).is_err() {
                        self.objects[i].ship.as_mut().unwrap().task = None;
                    }
                }
                None => self.objects[i].ship.as_mut().unwrap().task = None,
            }
            return;
        };
        let cell = self.deposit_cell(d);
        let Some(seat) = self.amber_seat(i, cell) else {
            return;
        };
        let s = self.objects[i].ship.as_ref().unwrap();
        if self.cell_of(i) != seat {
            // 0x4a6270 to the seat (the port plans once it has no route;
            // a seat it cannot reach is not decoded: it waits).
            if s.path.is_empty() && s.move_state == 0 {
                let t = TILE_SIZE as f32;
                let to = ((seat.0 as f32 + 0.5) * t, (seat.1 as f32 + 0.5) * t);
                if let Some(path) = self.plan_route(i, to.0, to.1) {
                    self.store_route(i, path);
                }
            }
            return;
        }
        if !s.path.is_empty() || s.move_state != 0 {
            return;
        }
        if self.room(i) == 0 {
            self.objects[i].ship.as_mut().unwrap().task = None;
            return;
        }
        if let Some(l) = self.take_close_in(i, g.cell) {
            let s = self.objects[i].ship.as_mut().unwrap();
            s.landing = Some(l);
            if let Some(Task::Gather { phase, .. }) = s.task.as_mut() {
                *phase = GatherPhase::Collecting;
            }
        }
    }

    /// 0x4afdd0: the amber in the order's cell `+0x797` still the order's
    /// deposit `+0x78a` and standing (`+0x74` 0): a take (0x4ad0e0 class
    /// 400: 100 for boats and merchants, 20 for others, a third for player
    /// 16, at most what is left), the amber drained by it (vt+4 mode 0:
    /// its picture a third smaller each 100 gone, emptied it lies fallow
    /// or goes). None otherwise.
    fn amber_take(
        &mut self,
        k: usize,
        deposit: u32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<i32> {
        let cell = self.objects[k].ship.as_ref()?.gathered?.cell;
        let d = self.layer_deposit(cell)?;
        if self.objects[d].id != deposit || self.objects[d].class != ObjectClass::Amber {
            return None;
        }
        let dep = self.objects[d].deposit?;
        let kind = self.objects[k].ship.as_ref()?.kind;
        let barbarian = self.objects[k].owner == Some((4, 0));
        let mut n = super::trade::take_per_round(Resource::Amber, kind, barbarian) as i32;
        let Some(left) = dep.remaining else {
            return Some(n);
        };
        n = n.min(left as i32);
        let third = |v: f32| (v / 100.0).ceil() as i64;
        let after = left - n as f32;
        self.objects[d].deposit.as_mut().unwrap().remaining = Some(after.max(0.0));
        if after <= 1e-3 {
            if dep.regrows > 0 {
                self.objects[d].deposit = None;
                self.objects[d].dormant = Some((dep, 0.0));
                self.refresh_amber(d, data, caches);
            } else {
                self.objects.remove(d);
            }
        } else if third(left) != third(after) {
            self.refresh_amber(d, data, caches);
        }
        Some(n)
    }

    /// 0x4ad300(ship, class slot, n) (amber 2, fish 0): the take stored
    /// aboard, up to its room (0x4ad280: the top load's part left and the
    /// free loads of 100); true when that room was no more than the take
    /// (full). Nothing taken, nothing stored and not full.
    pub(super) fn store_take(&mut self, k: usize, class: Resource, n: i32) -> bool {
        if n == 0 {
            return false;
        }
        let left = self.fill_left(k, class);
        let s = self.objects[k].ship.as_mut().unwrap();
        s.cargo += n.min(left) as f32;
        s.cargo_kind = Some(class);
        left <= n
    }

    /// 0x4b0590 past step 1 for ship `k`, its machine in its landing.
    fn amber_landing_tick(
        &mut self,
        mut k: usize,
        deposit: u32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let s = self.objects[k].ship.as_ref().unwrap();
        let Some(mut l) = s.landing.clone() else {
            return;
        };
        let kind = s.kind as usize % 12;
        l.hull_ticks += 1;
        let hull_step = l.hull_ticks > super::landing::HULL_DELAY;
        let id = self.objects[k].id;
        match l.state {
            10 => {
                if self.close_in_step(k, &mut l, 2) {
                    if l.abort {
                        let s = self.objects[k].ship.as_mut().unwrap();
                        s.furl = 0.0;
                        s.rotation = None;
                        s.landing = None;
                        s.task = None;
                        return;
                    }
                    l.state = 0xb;
                }
            }
            0xb => {
                if self.furl_step(k, &mut l, hull_step) {
                    let o = &self.objects[k];
                    let half = objects::SHIP_FRAME[kind] / 2.0;
                    let off =
                        unload::walkway_offset(kind, l.dir as usize, l.group.map(usize::from));
                    l.plank_at = off.map_or((0.0, 0.0), |(dx, dy)| {
                        (o.x - half + dx as f32, o.y - half + dy as f32)
                    });
                    l.plank = Some(0);
                    l.back = 0;
                    l.state = 0xc;
                }
            }
            0xc => {
                let f = l.plank.unwrap_or(0);
                if l.abort {
                    if f > 0 {
                        l.plank = Some(f - 1);
                    } else {
                        l.plank = None;
                        l.back = l.count;
                        l.state = 5;
                    }
                } else {
                    let n = self.landing_art[l.walk_dir as usize % 8][0].len() as i32;
                    if n <= f + 1 {
                        l.plank = Some(n - 1);
                        l.porters = Some(0);
                        l.state = 0xf;
                    } else {
                        l.plank = Some(f + 1);
                    }
                }
            }
            0xf => {
                let take = self.amber_take(k, deposit, data, caches);
                let Some(i) = self.object_index(id) else {
                    return;
                };
                k = i;
                if let Some(n) = take {
                    l.load = n;
                }
                if take.is_some() && !l.abort {
                    l.state = 6;
                } else {
                    l.porters = None;
                    l.state = 0xe;
                }
            }
            6 => {
                if l.abort {
                    l.porters = None;
                    l.state = 0xe;
                } else {
                    let n = self.landing_art[l.dir as usize % 8][1].len() as i32;
                    let f = l.porters.unwrap_or(0) + 1;
                    if n <= f {
                        l.porters = Some(0);
                        l.state = 0xf;
                        if self.store_take(k, Resource::Amber, l.load) {
                            l.load = 0;
                            l.porters = None;
                            l.state = 0xe;
                        }
                    } else {
                        l.porters = Some(f);
                    }
                }
            }
            0xe => {
                let f = l.plank.unwrap_or(0) - 1;
                if f < 0 {
                    l.plank = None;
                    l.back = l.count;
                    l.state = 5;
                } else {
                    l.plank = Some(f);
                }
            }
            5 => {
                let kept = self.objects[k].ship.as_ref().unwrap().close_in;
                if self.back_out_step(k, &mut l, hull_step) {
                    let s = self.objects[k].ship.as_mut().unwrap();
                    s.close_in = kept;
                    s.task = None;
                    s.path.clear();
                    s.last_deposit = deposit;
                    self.post_notice(id, 0x138, super::gather::class_code(Resource::Amber));
                    return;
                }
            }
            _ => {}
        }
        self.objects[k].ship.as_mut().unwrap().landing = Some(l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// miss000's own boat sent to the amber nearest it (0x4b0590): it sails
    /// to the seat east of the amber's cell (0x4afb80), closes in, furls,
    /// lowers the walkway, and the porters take 100 a round of their
    /// picture (a frame a tick) till its 4 loads are full (the fourth store
    /// 0x4ad300 full: step 0xe), the commands held throughout; backed out,
    /// the order ends.
    #[test]
    fn the_boat_gathers_amber_by_its_steps() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let (boat, bx, by) = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
            .map(|o| (o.id, o.x, o.y))
            .unwrap();
        let d = scene
            .objects
            .iter()
            .filter(|o| o.deposit.is_some_and(|d| d.resource == Resource::Amber))
            .min_by(|a, b| {
                let d = |o: &&MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
                d(a).total_cmp(&d(b))
            })
            .unwrap()
            .id;
        let k = scene.object_index(d).unwrap();
        let cell = scene.deposit_cell(k);
        let before = scene.objects[k].deposit.unwrap().remaining.unwrap();
        scene.order_gather(boat, d).unwrap();
        let i = scene.object_index(boat).unwrap();
        assert_eq!(scene.amber_seat(i, cell), Some((cell.0 + 1, cell.1)));
        let mut steps: Vec<u8> = Vec::new();
        let mut loads = Vec::new();
        let (mut round, mut rounds) = (0, Vec::new());
        let mut plank_top = 0;
        let mut walk_dir = 0;
        for _ in 0..20_000 {
            scene.update(crate::clock::TICK, &data, &mut caches);
            let i = scene.object_index(boat).unwrap();
            let s = scene.objects[i].ship.as_ref().unwrap();
            if s.task.is_none() {
                break;
            }
            let Some(l) = s.landing.as_ref() else {
                continue;
            };
            let (state, load, plank, wd) = (l.state, l.load, l.plank, l.walk_dir);
            assert_eq!(scene.capabilities(i), s.caps & 0x2000, "held");
            if steps.last() != Some(&state) {
                if state == 6 {
                    loads.push(load);
                }
                if steps.last() == Some(&6) {
                    rounds.push(round);
                }
                round = 0;
                steps.push(state);
            }
            round += 1;
            plank_top = plank_top.max(plank.unwrap_or(0));
            walk_dir = wd;
        }
        assert_eq!(
            steps,
            [10, 0xb, 0xc, 0xf, 6, 0xf, 6, 0xf, 6, 0xf, 6, 0xe, 5],
            "the steps"
        );
        assert_eq!(loads, [100; 4]);
        let porters = scene.landing_art[walk_dir as usize % 8][1].len();
        assert_eq!(rounds, [porters; 4]);
        let walkway = scene.landing_art[walk_dir as usize % 8][0].len() as i32;
        assert_eq!(plank_top, walkway - 1);
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_ref().unwrap();
        assert!(s.task.is_none() && s.landing.is_none());
        assert_eq!(s.cargo, 400.0);
        let k = scene.object_index(d).unwrap();
        assert_eq!(
            scene.objects[k].deposit.unwrap().remaining,
            Some(before - 400.0)
        );
    }
}
