//! A ship's fish gather (Run_Ship.cpp, the gather task of class 0x191,
//! routed by 0x4ac2d0), a game tick at a time, in the ship's `Hunt` (its
//! step `+0x77d`): sub-code 9 (0x4ad680) sails to the shoal's cell till
//! within 2 cells; sub-code 10 (0x4ae760) by its step:
//!
//! - **1**: the look 0x4ac8d0 over the 5 x 5 cells (0x59af48) about the
//!   ship for the shoal in a cell's slot with its picture (`+0x111`, states
//!   1-3). Not seen: sub-code 9. Seen elsewhere: a move there (0x4a6270
//!   mode 6, inside the box). On it and not under way: no room (0x4be800
//!   mode 3 0 and the top load whole, `+0x527` 0 or 100) posts notice
//!   0x138 to its group (0x4a1430, class 0x191) and waits; else the net
//!   (slot 1, 0x50cbf0 depth 0x3e6 kind 8, `SETKA_<facing>` by `+0x75` 3
//!   through 0x4e1bb0(1)) at the hull picture's middle, frame 0, step 2.
//! - **2**: the take 0x4ad4a0 (the shoal still in the ship's cell's slot
//!   with its picture: 0x4ad0e0 class 0x191 into `+0x782`), none or called
//!   off (`+0x77c < 0`) the net freed (0x4e0220) and step 5; else, the ship
//!   not turning (move state 2), step 6, the net made again if gone.
//! - **6**: a frame a tick (`+0x7d` 0 against the game time `+0x81`): the
//!   net's top left the hull's plus 0x59afa8; turning, the net freed and
//!   step 2; the facing changed, the picture again; drawn (0x4e0470) and
//!   its frame up `[game +0xe2]`; at its count, frame 0, step 2 and the
//!   catch stored (0x4ad300(ship, 0, catch)): full, the catch 0, notice
//!   0x138 (0x4ad640, not when called off), step 1, the order ends (event
//!   8 0xc0) and the net freed.
//! - **5**: called off, or after notice 0x138 (0x4ad640), step 1 and the
//!   order ends (event 8 0xc0).
//!
//! Nothing holds the ship's commands (no 0x4b37d0): `+0x77b` 1 in steps 2
//! and 6 is the task's busy flag. A stop past step 1 calls it off
//! (0x4abc60).

use openconquest_formats::ObjectClass;

use super::hunt::Look;
use super::{walk_path, Scene};
use crate::objects::{GatherPhase, Net, Resource, Task};

/// The square of cells the fishing look covers (0x59af48 by ship type: 5
/// for every type).
const LOOK: i32 = 5;

/// Where a fishing ship's net lies: its picture's top left from the hull
/// picture's, by ship type and heading (0x59afa8[type x 8 + heading];
/// headings four apart alike).
const NET_AT: [[(i8, i8); 4]; 12] = [
    [(9, 32), (21, 29), (-3, 29), (-4, 27)],
    [(14, 34), (28, 31), (0, 29), (-1, 33)],
    [(19, 43), (35, 40), (8, 39), (10, 40)],
    [(19, 43), (35, 40), (8, 39), (10, 40)],
    [(16, 34), (31, 33), (7, 35), (3, 31)],
    [(15, 37), (31, 33), (6, 32), (2, 31)],
    [(16, 34), (31, 33), (7, 35), (3, 31)],
    [(15, 37), (31, 33), (6, 32), (2, 31)],
    [(16, 34), (31, 33), (7, 35), (3, 31)],
    [(15, 37), (31, 33), (6, 32), (2, 31)],
    [(16, 34), (31, 33), (7, 35), (3, 31)],
    [(15, 37), (31, 33), (6, 32), (2, 31)],
];

impl Scene {
    /// Whether ship `k` gathers fish (its gather task of class 0x191).
    pub(super) fn fishing(&self, k: usize) -> bool {
        self.objects[k].ship.as_ref().is_some_and(|s| {
            matches!(s.task, Some(Task::Gather { .. }))
                && s.gathered.is_some_and(|g| g.class == Resource::Fish)
        })
    }

    /// The shoal's cell (vt+0x28, 0x48eb60: `+0x9d` / `+0xa1` but in
    /// states 4, 5 and 0xff): one standing, below or not, not lying empty
    /// to grow back.
    fn shoal_where(&self, shoal: u32) -> Option<(i32, i32)> {
        let k = self.object_index(shoal)?;
        let o = &self.objects[k];
        (o.class == ObjectClass::FishZone && o.deposit.is_some()).then(|| self.hunt_cell(k))
    }

    /// Whether shoal `shoal` lies in `cell`'s slot with its picture
    /// (`+0x111`, its states 1-3: not gone below).
    fn shoal_shown_in(&self, shoal: u32, cell: (i32, i32)) -> bool {
        self.shoal_where(shoal) == Some(cell) && !self.shoal_gone(shoal)
    }

    /// The hull picture's top left (`+0x8be +0x59` / `+0x5d`) and size
    /// (`+0x61` / `+0x65`), as the port places it (`render::layer_origin`).
    fn hull_rect(&self, i: usize) -> (i32, i32, i32, i32) {
        let o = &self.objects[i];
        match o.frames.first().and_then(|f| f.first()) {
            Some(l) => {
                let (x, y) = crate::render::layer_origin(o, l);
                (x, y, l.image.width as i32, l.image.height as i32)
            }
            None => (o.x as i32, o.y as i32, 0, 0),
        }
    }

    /// The net 0x4ae760 makes (steps 1 and 2): frame 0, its top left at
    /// the hull picture's middle, its facing the ship's heading less 4
    /// above 3.
    fn new_net(&self, i: usize) -> Net {
        let (x, y, w, h) = self.hull_rect(i);
        let heading = self.objects[i].ship.as_ref().map_or(0, |s| s.heading);
        Net {
            frame: 0,
            shown: 0,
            facing: (heading % 4) as u8,
            at: (w / 2 + x, h / 2 + y),
        }
    }

    /// Sub-code 9 (0x4ad680) for a shoal, at step 1: its cell (0x48eb60);
    /// none posts notice 0x138 with class 0 to the ship's group
    /// (0x4a1430), every tick. Within 0x59af48[type] / 2 cells (0x557440)
    /// sub-code 10; and a move to its cell (0x4a6270 mode 5).
    pub(super) fn fish_sail(&mut self, i: usize, shoal: u32) {
        if self.objects[i]
            .ship
            .as_ref()
            .and_then(|s| s.hunt)
            .is_some_and(|h| h.step != 1)
        {
            return;
        }
        let id = self.objects[i].id;
        let Some(cell) = self.shoal_where(shoal) else {
            self.post_notice(id, 0x138, 0);
            return;
        };
        let here = self.hunt_cell(i);
        if walk_path::dist(here.0, here.1, cell.0, cell.1) <= LOOK / 2 {
            if let Some(s) = self.objects[i].ship.as_mut() {
                s.task = Some(Task::Gather {
                    deposit: shoal,
                    phase: GatherPhase::Collecting,
                });
            }
        }
        self.hunt_plan(i, cell, None);
    }

    /// 0x4ac8d0 for a shoal (size 5, reach 0): over the box about the ship
    /// (the cell it is going into while under way), clipped to the map, row
    /// by row, the shoal in a cell's slot with its picture, its distance
    /// 0x557440 (the last such cell kept). None: lost. Further than 0: a
    /// move there inside the box. On it and not under way: near.
    fn fish_look(&mut self, i: usize, shoal: u32) -> Look {
        let under = self.hunt_under_way(i);
        let (cx, cy) = under.unwrap_or_else(|| self.hunt_cell(i));
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let half = LOOK / 2;
        let (x0, x1) = ((cx - half).max(0), (cx + half + 1).min(w));
        let (y0, y1) = ((cy - half).max(0), (cy + half + 1).min(h));
        let mut found = None;
        for y in y0..y1 {
            for x in x0..x1 {
                if self.shoal_shown_in(shoal, (x, y)) {
                    found = Some(((x, y), walk_path::dist(cx, cy, x, y)));
                }
            }
        }
        let Some((to, dist)) = found else {
            return Look::Lost;
        };
        if dist > 0 {
            self.look_move(i, to, (x0, y0, x1, y1));
        } else if under.is_none() {
            return Look::Near;
        }
        Look::Seen
    }

    /// 0x4ad4a0: the order's shoal in the slot of the ship's cell
    /// (`+0x5a3` / `+0x5a7`) with its picture: the take 0x4ad0e0 class
    /// 0x191 (0x59b2a8 by ship type: 33, 50, 20, 20; a third for player
    /// 16; at most what it holds), the shoal's amount less it (vt+4 mode
    /// 0, 0x48d6e0). None otherwise.
    fn fish_take(&mut self, i: usize, shoal: u32) -> Option<i32> {
        if !self.shoal_shown_in(shoal, self.hunt_cell(i)) {
            return None;
        }
        let kind = self.objects[i].ship.as_ref()?.kind;
        let barbarian = self.objects[i].owner == Some((4, 0));
        let mut n = super::trade::take_per_round(Resource::Fish, kind, barbarian) as i32;
        let d = self.object_index(shoal)?;
        if let Some(left) = self.objects[d]
            .deposit
            .as_mut()
            .and_then(|dep| dep.remaining.as_mut())
        {
            n = n.min(*left as i32);
            *left -= n as f32;
        }
        Some(n)
    }

    /// The order's end (event 8 0xc0), after notice 0x138 with class
    /// 0x191 to the ship's group unless called off (0x4ad640).
    fn fish_end(&mut self, i: usize, shoal: u32) {
        let called_off = self.objects[i]
            .ship
            .as_ref()
            .and_then(|s| s.hunt)
            .is_some_and(|h| h.called_off);
        if called_off {
            let s = self.objects[i].ship.as_mut().unwrap();
            s.task = None;
            s.path.clear();
        } else {
            self.gathering_done(i, shoal);
        }
    }

    /// Sub-code 10 (0x4ae760), by its step.
    pub(super) fn fish_watch(&mut self, i: usize, shoal: u32) {
        let Some(s) = self.objects[i].ship.as_ref() else {
            return;
        };
        let (kind, heading, turning) = (s.kind as usize % 12, s.heading, s.move_state == 2);
        let step = s.hunt.map_or(1, |h| h.step);
        match step {
            1 => {
                // `+0x77b` 0 (0x4ae7b6).
                self.objects[i].ship.as_mut().unwrap().net_busy = false;
                match self.fish_look(i, shoal) {
                    Look::Lost => {
                        let s = self.objects[i].ship.as_mut().unwrap();
                        s.task = Some(Task::Gather {
                            deposit: shoal,
                            phase: GatherPhase::ToDeposit,
                        });
                        return;
                    }
                    Look::Seen => return,
                    Look::Near => {}
                }
                // No room and the top load whole: notice 0x138, every
                // tick, the order kept.
                if self.fill_left(i, Resource::Fish) == 0 {
                    let id = self.objects[i].id;
                    self.post_notice(id, 0x138, super::gather::class_code(Resource::Fish));
                    return;
                }
                let net = self.new_net(i);
                let h = self.hunt_mut(i);
                h.step = 2;
                h.net = Some(net);
            }
            2 => {
                let take = self.fish_take(i, shoal);
                let net = self.new_net(i);
                let h = self.hunt_mut(i);
                if let Some(n) = take {
                    h.catch = n;
                }
                if take.is_none() || h.called_off {
                    h.step = 5;
                    h.net = None;
                    // `+0x77b` 0 (0x4ae9d1).
                    self.objects[i].ship.as_mut().unwrap().net_busy = false;
                    return;
                }
                // `+0x77b` 1 (0x4ae8f4).
                self.objects[i].ship.as_mut().unwrap().net_busy = true;
                let h = self.hunt_mut(i);
                if !turning {
                    h.step = 6;
                    h.net.get_or_insert(net);
                }
            }
            6 => {
                // `+0x77b` 1 (0x4ae9e6).
                self.objects[i].ship.as_mut().unwrap().net_busy = true;
                let (x, y, _, _) = self.hull_rect(i);
                let (ox, oy) = NET_AT[kind][heading % 4];
                let h = self.hunt_mut(i);
                let Some(mut net) = h.net else {
                    return;
                };
                net.at = (x + ox as i32, y + oy as i32);
                if turning {
                    h.net = None;
                    h.step = 2;
                    return;
                }
                // A new facing loads its picture again (0x4e1bb0(1); its
                // frame kept: not decoded whether 0x50d080 resets it).
                net.facing = (heading % 4) as u8;
                net.shown = net.frame;
                net.frame += 1;
                h.net = Some(net);
                let count = self.net_art[net.facing as usize].len() as i32;
                if net.frame < count {
                    return;
                }
                let h = self.hunt_mut(i);
                h.net.as_mut().unwrap().frame = 0;
                h.step = 2;
                let catch = h.catch;
                if self.store_take(i, Resource::Fish, catch) {
                    let h = self.hunt_mut(i);
                    h.catch = 0;
                    h.step = 1;
                    h.net = None;
                    self.fish_end(i, shoal);
                }
            }
            5 => {
                // `+0x77b` 0 (0x4aeb3a).
                self.objects[i].ship.as_mut().unwrap().net_busy = false;
                self.hunt_mut(i).step = 1;
                self.fish_end(i, shoal);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects;
    use crate::scene::{Caches, GameData, Order};

    /// miss000's own boat sent to the shoal nearest it, the shoals held up
    /// (their own dives left out): it sails there (sub-code 9), the look
    /// finds it on its cell, the net goes out (step 2) and a cast takes 33
    /// (0x59b2a8[0]) each round of the net's frames (step 6, a frame a
    /// tick, then a tick of step 2) till its 4 loads are full: 13 casts,
    /// the last stored 4 (0x4ad300: up to the room left), notice 0x138
    /// and the order ends.
    #[test]
    fn the_boat_fishes_by_its_steps() {
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
        let shoal = scene
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::FishZone && o.deposit.is_some())
            .min_by(|a, b| {
                let d = |o: &&objects::MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
                d(a).total_cmp(&d(b))
            })
            .unwrap()
            .id;
        let k = scene.object_index(shoal).unwrap();
        let before = scene.objects[k].deposit.unwrap().remaining.unwrap();
        scene.order_gather(boat, shoal).unwrap();
        let mut steps: Vec<u8> = Vec::new();
        let mut catches = Vec::new();
        let (mut round, mut rounds) = (0, Vec::new());
        let mut facing = 0;
        for _ in 0..20_000 {
            // The shoal stays up (`Scene::move_shoals` left out).
            scene.shoals_shown.insert(shoal, f32::MAX);
            scene.update(crate::clock::TICK, &data, &mut caches);
            let i = scene.object_index(boat).unwrap();
            let s = scene.objects[i].ship.as_ref().unwrap();
            if s.task.is_none() {
                break;
            }
            let Some(h) = s.hunt else {
                continue;
            };
            if !matches!(
                s.task,
                Some(Task::Gather {
                    phase: GatherPhase::Collecting,
                    ..
                })
            ) {
                continue;
            }
            assert!(!scene.hunt_holds(boat), "not held");
            if steps.last() != Some(&h.step) {
                if h.step == 6 {
                    catches.push(h.catch);
                }
                if steps.last() == Some(&6) {
                    rounds.push(round);
                }
                round = 0;
                steps.push(h.step);
            }
            round += 1;
            if let Some(n) = h.net {
                facing = n.facing;
            }
        }
        let count = scene.net_art[facing as usize].len();
        assert!(count > 0);
        let mut want = vec![1u8];
        for _ in 0..13 {
            want.extend([2, 6]);
        }
        assert_eq!(steps, want, "the steps");
        assert_eq!(catches, [33; 13]);
        assert_eq!(rounds, [count; 12], "a round the net's frames");
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_ref().unwrap();
        assert!(s.task.is_none());
        assert_eq!(s.cargo, 400.0);
        let k = scene.object_index(shoal).unwrap();
        assert_eq!(
            scene.objects[k].deposit.unwrap().remaining,
            Some(before - 13.0 * 33.0)
        );
        // The haul ended full at step 6: `+0x77b` stays set.
        assert!(s.net_busy);
    }

    /// 0x4abe50 on miss000's boat at its shoal: sailing there (`+0x77b`
    /// 0) a move runs at the group's turn; casting (step 6, `+0x77b` 1)
    /// the move is lost and the gather called off (0x4abc60: `+0x77c`
    /// 0xff), the net's round played out, then step 2 to 5 and the end
    /// with less than full aboard; the stop or a later order not needed.
    #[test]
    fn a_casting_boats_order_is_lost() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let fresh = |caches: &mut Caches| {
            let mut scene = Scene::load(&data, caches, "miss000").unwrap();
            scene.brain = None;
            let (boat, bx, by) = scene
                .objects
                .iter()
                .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
                .map(|o| (o.id, o.x, o.y))
                .unwrap();
            let shoal = scene
                .objects
                .iter()
                .filter(|o| o.class == ObjectClass::FishZone && o.deposit.is_some())
                .min_by(|a, b| {
                    let d = |o: &&objects::MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
                    d(a).total_cmp(&d(b))
                })
                .unwrap()
                .id;
            scene.order_gather(boat, shoal).unwrap();
            (scene, boat, shoal, bx, by)
        };
        let gathers = |scene: &Scene, boat: u32| {
            let i = scene.object_index(boat).unwrap();
            matches!(
                scene.objects[i].ship.as_ref().unwrap().task,
                Some(Task::Gather { .. })
            )
        };
        // Sailing: not busy, the move runs.
        let (mut scene, boat, shoal, bx, by) = fresh(&mut caches);
        for _ in 0..5 {
            scene.shoals_shown.insert(shoal, f32::MAX);
            scene.update(crate::clock::TICK, &data, &mut caches);
        }
        let i = scene.object_index(boat).unwrap();
        assert!(!scene.task_busy(i));
        scene
            .apply(
                &Order::Move {
                    ship: boat,
                    x: bx,
                    y: by,
                },
                &data,
                &mut caches,
            )
            .unwrap();
        assert!(!gathers(&scene, boat), "the move ran");
        // Casting: busy, the move lost.
        let (mut scene, boat, shoal, bx, by) = fresh(&mut caches);
        let step = |scene: &Scene| {
            let i = scene.object_index(boat).unwrap();
            scene.objects[i].ship.as_ref().unwrap().hunt.map(|h| h.step)
        };
        for _ in 0..20_000 {
            scene.shoals_shown.insert(shoal, f32::MAX);
            scene.update(crate::clock::TICK, &data, &mut caches);
            if step(&scene) == Some(6) {
                break;
            }
        }
        assert_eq!(step(&scene), Some(6));
        let i = scene.object_index(boat).unwrap();
        assert!(scene.task_busy(i));
        scene
            .apply(
                &Order::Move {
                    ship: boat,
                    x: bx,
                    y: by,
                },
                &data,
                &mut caches,
            )
            .unwrap();
        // A second order of the tick goes with it.
        scene
            .apply(
                &Order::Gather {
                    ship: boat,
                    deposit: shoal,
                    port: 0,
                },
                &data,
                &mut caches,
            )
            .unwrap();
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_ref().unwrap();
        assert!(gathers(&scene, boat), "the move lost");
        assert!(s.hunt.unwrap().called_off);
        let mut steps = vec![6u8];
        while gathers(&scene, boat) {
            scene.shoals_shown.insert(shoal, f32::MAX);
            scene.update(crate::clock::TICK, &data, &mut caches);
            if let Some(h) = step(&scene).filter(|&h| steps.last() != Some(&h)) {
                steps.push(h);
            }
        }
        assert_eq!(steps, [6, 2, 5, 1]);
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_ref().unwrap();
        assert!(s.cargo < 400.0);
        assert!(!s.net_busy);
    }

    /// `+0x77b` left set by a full haul (`the_boat_fishes_by_its_steps`)
    /// makes the next 0x40 task busy whatever it is (0x4abe50): miss000's
    /// boat sent to unload sails to its berth (0x4adef0 step 1); a move
    /// then is lost and the unload ends there (0x4abc60: `+0x77d` 1),
    /// the ship stopping in the cell it is going into. With `+0x77b` 0
    /// the move runs.
    #[test]
    fn a_stale_net_makes_the_unload_busy() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let run = |net: bool, caches: &mut Caches| {
            let mut scene = Scene::load(&data, caches, "miss000").unwrap();
            scene.brain = None;
            let boat = scene
                .objects
                .iter()
                .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
                .unwrap()
                .id;
            let i = scene.object_index(boat).unwrap();
            let port = scene.unloading_site(i).unwrap();
            let s = scene.objects[i].ship.as_mut().unwrap();
            s.cargo = 100.0;
            s.cargo_kind = Some(Resource::Fish);
            s.net_busy = net;
            scene
                .apply(
                    &Order::Unload {
                        ship: boat,
                        port,
                        prio: 75,
                    },
                    &data,
                    caches,
                )
                .unwrap();
            for _ in 0..5 {
                scene.update(crate::clock::TICK, &data, caches);
            }
            let i = scene.object_index(boat).unwrap();
            let s = scene.objects[i].ship.as_ref().unwrap();
            assert!(matches!(
                s.task,
                Some(Task::Gather {
                    phase: GatherPhase::ToPort(_),
                    ..
                })
            ));
            assert_eq!(scene.task_busy(i), net);
            // A far water cell: the shoal's.
            let (x, y) = scene
                .objects
                .iter()
                .find(|o| o.class == ObjectClass::FishZone && o.deposit.is_some())
                .map(|o| (o.x, o.y))
                .unwrap();
            scene
                .apply(&Order::Move { ship: boat, x, y }, &data, caches)
                .unwrap();
            let s = scene.objects[i].ship.clone().unwrap();
            (s, scene.cell_of_object(i))
        };
        let (s, _) = run(false, &mut caches);
        assert!(s.task.is_none());
        assert!(s.path.len() > 1, "the move ran");
        let (s, _) = run(true, &mut caches);
        assert!(s.task.is_none(), "the unload ended at step 1");
        assert!(s.path.len() <= 1, "the move lost, the ship stopping");
        assert!(s.net_busy, "nothing clears it but a fish step");
    }

    /// A refused order after 0x4abe50 took the tasks 0x10-0x80 off
    /// `+0x25d` (0x4abf41): miss000's boat sailing to its shoal (sub-code
    /// 9) told to unload at the shoal, no port of its player's (0x4aab00
    /// code 0xc: 0x4ac050 finds none, returns), is left with no task, its
    /// gather's machine stopped where it stood and the route it was on
    /// sailed on (nothing wound down).
    #[test]
    fn a_refused_order_ends_the_old_task() {
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
        let shoal = scene
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::FishZone && o.deposit.is_some())
            .min_by(|a, b| {
                let d = |o: &&objects::MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
                d(a).total_cmp(&d(b))
            })
            .unwrap()
            .id;
        scene.order_gather(boat, shoal).unwrap();
        for _ in 0..5 {
            scene.shoals_shown.insert(shoal, f32::MAX);
            scene.update(crate::clock::TICK, &data, &mut caches);
        }
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_mut().unwrap();
        assert!(matches!(s.task, Some(Task::Gather { .. })));
        assert!(!s.path.is_empty(), "sailing");
        let path = s.path.clone();
        s.cargo = 100.0;
        s.cargo_kind = Some(Resource::Fish);
        let r = scene.apply(
            &Order::Unload {
                ship: boat,
                port: shoal,
                prio: 75,
            },
            &data,
            &mut caches,
        );
        assert!(r.is_err(), "refused");
        let s = scene.objects[i].ship.as_ref().unwrap();
        assert!(s.task.is_none(), "the old task over");
        assert_eq!(s.path, path, "the route kept");
        assert!(s.adrift, "no machine to plan it again");
    }

    /// A refused order takes the 0x80 move's machine off too (0x4abe50 at
    /// 0x4abf41; the ship tick 0x4ab930 runs 0x4a5f30 only by `+0x25d &
    /// 0x80`): miss000's boat whose plain move found no route yet (its
    /// waited re-plan pending, 0x4a6150) is told to unload at a shoal
    /// (refused, 0x4aab00 code 0xc): the re-plan never comes, the route
    /// it has kept; a move given after runs and plans again.
    #[test]
    fn a_refused_order_stops_the_move_machine() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let boat = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
            .unwrap()
            .id;
        let (shoal, sx, sy) = scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::FishZone && o.deposit.is_some())
            .map(|o| (o.id, o.x, o.y))
            .unwrap();
        for _ in 0..3 {
            scene.update(crate::clock::TICK, &data, &mut caches);
        }
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_mut().unwrap();
        // The first plan's none (0x4a5f30 -> 0x4a6330 0xff): pending.
        s.pending = Some((sx, sy));
        s.waited = crate::clock::TICK;
        s.cargo = 100.0;
        s.cargo_kind = Some(Resource::Fish);
        let r = scene.apply(
            &Order::Unload {
                ship: boat,
                port: shoal,
                prio: 75,
            },
            &data,
            &mut caches,
        );
        assert!(r.is_err(), "refused");
        for _ in 0..40 {
            scene.update(crate::clock::TICK, &data, &mut caches);
        }
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_ref().unwrap();
        assert!(s.pending.is_none(), "0x4a5f30 no longer runs");
        assert!(s.adrift);
        assert!(s.path.is_empty(), "no plan made for it");
        scene
            .apply(
                &Order::Move {
                    ship: boat,
                    x: sx,
                    y: sy,
                },
                &data,
                &mut caches,
            )
            .unwrap();
        let s = scene.objects[i].ship.as_ref().unwrap();
        assert!(!s.adrift, "its move machine on again");
        assert!(s.path.len() > 1);
    }

    /// A machine's own event (vt+4(0x40, ..)) is read by the ship's next
    /// tick's 0x4ab720 like any of the group: miss000's boat sent to
    /// unload loses its port on the way; at its step 1 (0x4adef0, the
    /// port's slot empty) it sends itself to the port 0x4c5910 finds
    /// (event 0x40 code 0xc, priority 0x5a), not carried out in that
    /// tick; with `+0x77b` left set by a full haul, 0x4abe50 finds the
    /// unload busy at that next tick and drops the event, the unload
    /// winding down (0x4abc60, `+0x77d` 1: ended).
    #[test]
    fn a_machines_event_waits_for_the_next_tick() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        // The first mission whose ship has two ports of its player's to
        // unload at (0x4c5910's).
        let pick = |caches: &mut Caches| {
            for m in 0..=20 {
                let name = format!("miss{m:03}");
                let Ok(mut scene) = Scene::load(&data, caches, &name) else {
                    continue;
                };
                scene.brain = None;
                for _ in 0..3 {
                    scene.update(crate::clock::TICK, &data, caches);
                }
                for i in 0..scene.objects.len() {
                    let Some(s) = scene.objects[i].ship.as_ref() else {
                        continue;
                    };
                    if crate::objects::cargo_capacity(s.kind) <= 0.0 {
                        continue;
                    }
                    let Some(gone) = scene.unloading_site(i) else {
                        continue;
                    };
                    let id = scene.objects[i].id;
                    let p = scene.object_index(gone).unwrap();
                    let o = scene.objects.remove(p);
                    let k = scene.object_index(id).unwrap();
                    let other = scene.unloading_site(k).is_some();
                    scene.objects.insert(p, o);
                    if other {
                        return (name, id);
                    }
                }
            }
            panic!("no ship with two ports");
        };
        let (name, boat) = pick(&mut caches);
        let run = |net: bool, caches: &mut Caches| {
            let mut scene = Scene::load(&data, caches, &name).unwrap();
            scene.brain = None;
            for _ in 0..3 {
                scene.update(crate::clock::TICK, &data, caches);
            }
            let i = scene.object_index(boat).unwrap();
            let gone = scene.unloading_site(i).unwrap();
            let s = scene.objects[i].ship.as_mut().unwrap();
            s.cargo = 100.0;
            s.cargo_kind = Some(Resource::Fish);
            s.net_busy = net;
            s.task = Some(Task::Gather {
                deposit: 0,
                phase: GatherPhase::ToPort(gone),
            });
            s.path.clear();
            let p = scene.object_index(gone).unwrap();
            scene.objects.remove(p);
            let i = scene.object_index(boat).unwrap();
            let next = scene.unloading_site(i);
            scene.outbox.push(Order::Stop { id: 0 });
            scene.run_task(i);
            (scene, boat, gone, next)
        };
        let (mut scene, boat, gone, next) = run(false, &mut caches);
        let next = next.unwrap();
        // Posted before the commands already in the outbox.
        assert!(matches!(
            scene.outbox[0],
            Order::UnloadEvent { ship, port, prio: 0x5a } if ship == boat && port == next
        ));
        let i = scene.object_index(boat).unwrap();
        let task = |scene: &Scene| scene.objects[i].ship.as_ref().unwrap().task;
        assert!(matches!(
            task(&scene),
            Some(Task::Gather { phase: GatherPhase::ToPort(p), .. }) if p == gone
        ));
        scene.outbox.remove(1);
        scene.deliver(&data, &mut caches);
        assert!(matches!(
            task(&scene),
            Some(Task::Gather { phase: GatherPhase::ToPort(p), .. }) if p == next
        ));
        let (mut scene, boat, _, _) = run(true, &mut caches);
        scene.outbox.remove(1);
        scene.deliver(&data, &mut caches);
        let i = scene.object_index(boat).unwrap();
        assert!(scene.objects[i].ship.as_ref().unwrap().task.is_none());
    }
}
