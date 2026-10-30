//! The leviathan hunt (Run_Ship.cpp, the gather task of class 0x192,
//! routed by 0x4ac2d0): a ship sent to a leviathan sails to it (sub-code
//! 9, 0x4ad680) and watches over the cells about it (sub-code 10, 0x4aeef0
//! step 1 with 0x4ac8d0). Warships shoot at it while it shows itself
//! (0x4af6a0) and leave it once it is up; a boat or merchant goes onto its
//! cell when it comes up, turns east or west, lets its line down and nets
//! it a cast at a time until full or the leviathan goes down, takes the
//! line up and goes on (0x4ad510). A game tick at a time.

use openconquest_formats::map::TILE_SIZE;

use super::leviathan::{SHOWING, UP};
use super::{walk_path, Caches, GameData, Scene};
use crate::objects::{self, GatherPhase, Hunt, Resource, Task};

/// The square of cells a hunter looks over and sails within half of
/// (0x59af48 by ship type: 5 for every type).
const LOOK: i32 = 5;
/// Ticks between a behaviour's plans (`+0x57f`, 25 from 0x4aab00).
const PLAN_TICKS: u32 = 25;
/// The priority of the move a warship makes off a leviathan come up on
/// its cell (0x4b49a0: event 0x80 with 0x32).
const MOVE_OFF: u8 = 0x32;
/// The priority of the move asked of one of its player's ships lying on
/// the leviathan's cell while it is up (0x4ac8d0: 0x4b4b50 with 0x5a).
const MAKE_WAY: u8 = 0x5a;
/// The priority of the orders 0x4ad510 gives itself (event 0x40, 0x5a).
const NEXT: u8 = 0x5a;
/// The most a cast of the net takes (0x4ad0e0).
const CAST: i32 = 50;
/// The hull picture's turning frames (`+0x8be +0x45`): 0x20 to 0x37, 3 a
/// heading.
const TURN_FIRST: i32 = 0x20;
const TURN_LAST: i32 = 0x37;
/// The random part of a reload after a shot at a leviathan (0x4af6a0: a
/// draw `% 0x3c` ticks).
const RELOAD_JITTER: u32 = 60;

/// The cells items 3 and 4 reach (0x4bf5f0: 0x598324, 0x598418), by
/// range level, rows by 4 less the distance across, columns by 4 less the
/// distance down (half of 0x598130's 9 for both items).
const REACH: [[[[u8; 9]; 9]; 3]; 2] = [
    [
        [
            [0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 1, 1, 1, 0, 0, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 1, 1, 1, 2, 1, 1, 1, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 0, 0, 1, 1, 1, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0],
        ],
        [
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 2, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
        ],
        [
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 2, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
        ],
    ],
    [
        [
            [0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 1, 1, 1, 0, 0, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 1, 1, 1, 2, 1, 1, 1, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 0, 0, 1, 1, 1, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0],
        ],
        [
            [0, 0, 0, 0, 0, 0, 0, 0, 0],
            [0, 0, 0, 1, 1, 1, 0, 0, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 1, 1, 1, 2, 1, 1, 1, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 0, 0, 1, 1, 1, 0, 0, 0],
            [0, 0, 0, 0, 0, 0, 0, 0, 0],
        ],
        [
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 2, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [1, 1, 1, 1, 1, 1, 1, 1, 1],
            [0, 1, 1, 1, 1, 1, 1, 1, 0],
            [0, 0, 1, 1, 1, 1, 1, 0, 0],
        ],
    ],
];

/// What 0x4ac8d0 finds: nothing (0xff), the leviathan seen but not yet
/// near enough or the ship under way (0), seen within reach (1).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Lost,
    Seen,
    Near,
}

impl Scene {
    fn hunt_cell(&self, i: usize) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        (
            (self.objects[i].x / t).floor() as i32,
            (self.objects[i].y / t).floor() as i32,
        )
    }

    fn hunt_mut(&mut self, i: usize) -> &mut Hunt {
        self.objects[i]
            .ship
            .as_mut()
            .unwrap()
            .hunt
            .get_or_insert_with(Hunt::default)
    }

    /// Whether ship `k` hunts a leviathan (its gather task of class
    /// 0x192, sailing to it or watching over it).
    fn hunting(&self, k: usize) -> bool {
        self.objects[k].ship.as_ref().is_some_and(|s| {
            matches!(s.task, Some(Task::Gather { .. }))
                && s.gathered.is_some_and(|g| g.class == Resource::Leviathan)
        })
    }

    /// Whether ship `id` holds its commands while it nets a leviathan
    /// (0x4b37d0: its capabilities `+0x261` and 0x2000, till 0x4b3850 at
    /// step 5).
    pub fn hunt_holds(&self, id: u32) -> bool {
        self.object_index(id)
            .and_then(|i| self.objects[i].ship.as_ref())
            .and_then(|s| s.hunt)
            .is_some_and(|h| h.held)
    }

    /// Sends ship `i` to hunt leviathan `lev` (0x4aab00, class 0x192): its
    /// cell `at` and the box about it, the hunt from step 1 with its first
    /// plan at once (`+0x593`, `+0x57b` 0); sub-code 9 when the box's
    /// first corner lies more than 0x59af48[type] / 2 cells off
    /// (0x557440), else 10. Its route is kept till then.
    pub(super) fn order_hunt(
        &mut self,
        i: usize,
        lev: u32,
        at: (i32, i32),
        rect: (i32, i32, i32, i32),
        port: u32,
    ) -> Result<(), String> {
        let here = self.hunt_cell(i);
        let far = walk_path::dist(here.0, here.1, rect.0, rect.1) > LOOK / 2;
        let own_port = self.object_index(port).is_some_and(|p| {
            self.objects[p].owner == self.objects[i].owner && self.takes_cargo(&self.objects[p])
        });
        let ship = self.objects[i].ship.as_mut().unwrap();
        if ship.cargo_kind.is_some_and(|k| k != Resource::Leviathan) {
            ship.cargo = 0.0;
        }
        ship.task = Some(Task::Gather {
            deposit: lev,
            phase: if far {
                GatherPhase::ToDeposit
            } else {
                GatherPhase::Collecting
            },
        });
        ship.hunt = Some(Hunt::default());
        ship.home_port = if own_port { port } else { 0 };
        ship.last_deposit = lev;
        ship.gathered = Some(objects::Gathered {
            class: Resource::Leviathan,
            cell: at,
            rect,
        });
        Ok(())
    }

    /// The hunters' tick (0x4ac2d0, class 0x192: sub-code 9 0x4ad680, 10
    /// 0x4aeef0). A hunt whose task has ended leaves no state behind (a
    /// hull caught turning lies on the heading it reached). (Not settled:
    /// the order of the ships' and the leviathans' ticks.)
    pub(super) fn run_hunts(&mut self, data: &GameData, caches: &mut Caches) {
        let mut ids = Vec::new();
        for k in 0..self.objects.len() {
            let hunting = self.hunting(k);
            let Some(s) = self.objects[k].ship.as_mut() else {
                continue;
            };
            if !hunting {
                if s.hunt.take().is_some_and(|h| h.step == 10) {
                    s.rotation = None;
                }
                continue;
            }
            if let Some(Task::Gather { deposit, phase }) = s.task {
                if matches!(phase, GatherPhase::ToDeposit | GatherPhase::Collecting) {
                    s.hunt.get_or_insert_with(Hunt::default);
                    ids.push((self.objects[k].id, deposit));
                }
            }
        }
        for (id, lev) in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            match self.objects[i].ship.as_ref().and_then(|s| s.task) {
                Some(Task::Gather {
                    phase: GatherPhase::ToDeposit,
                    ..
                }) => self.hunt_sail(i, lev),
                Some(Task::Gather {
                    phase: GatherPhase::Collecting,
                    ..
                }) => self.hunt_watch(i, lev, data, caches),
                _ => {}
            }
        }
    }

    /// A behaviour's move to `goal` (0x4a6270 -> 0x4a6150): the first
    /// after the order at once (`+0x593`); later ones once 25 ticks have
    /// passed since the last plan (`+0x57b`, `+0x57f`), if the ship has
    /// changed cells or the goal changed or it lies still, and it is not
    /// turning (move state 2); a goal it is on ends the move (event 8
    /// 0x80). The route as the port's behaviours plan theirs. (The wait
    /// counter's 150 of 0x4a6150 is the port's own mover's.)
    fn hunt_plan(&mut self, i: usize, goal: (i32, i32)) {
        let now = self.ticks;
        let here = self.hunt_cell(i);
        let s = self.objects[i].ship.as_ref().unwrap();
        let h = s.hunt.unwrap_or_default();
        if !h.first_plan {
            let changed = here != h.plan_from || goal != h.plan_goal || s.move_state == 0;
            if !changed || s.move_state == 2 || now.wrapping_sub(h.planned) < PLAN_TICKS {
                return;
            }
            if here == goal {
                self.objects[i].ship.as_mut().unwrap().path.clear();
                return;
            }
        }
        let t = TILE_SIZE as f32;
        let (x, y) = (goal.0 as f32 * t + t / 2.0, goal.1 as f32 * t + t / 2.0);
        let route = self.plan_route(i, x, y);
        let s = self.objects[i].ship.as_mut().unwrap();
        if let Some(path) = route {
            s.path = path;
        }
        let h = self.hunt_mut(i);
        h.first_plan = false;
        h.planned = now;
        h.plan_from = here;
        h.plan_goal = goal;
    }

    /// Sub-code 9 (0x4ad680), at step 1: the leviathan by its id, at its
    /// cell (vt+0x28, 0x495ff0: none caught or coming up again); none
    /// posts notice 0x138 with class 0 to the ship's group (0x4a1430),
    /// every tick. Within 0x59af48[type] / 2 cells (0x557440) of it the
    /// ship watches over it (sub-code 10); and it sails to its cell.
    fn hunt_sail(&mut self, i: usize, lev: u32) {
        if self.objects[i]
            .ship
            .as_ref()
            .and_then(|s| s.hunt)
            .is_some_and(|h| h.step != 1)
        {
            return;
        }
        let id = self.objects[i].id;
        let Some(cell) = self.leviathan_where(lev) else {
            self.post_notice(id, 0x138, 0);
            return;
        };
        let here = self.hunt_cell(i);
        if walk_path::dist(here.0, here.1, cell.0, cell.1) <= LOOK / 2 {
            if let Some(s) = self.objects[i].ship.as_mut() {
                s.task = Some(Task::Gather {
                    deposit: lev,
                    phase: GatherPhase::Collecting,
                });
            }
        }
        self.hunt_plan(i, cell);
    }

    /// Whether ship `i` is between two cells (0x4ac8d0: `+0x596` and move
    /// state 4 or more), and the cell it is going into (`+0x5b6`).
    fn hunt_under_way(&self, i: usize) -> Option<(i32, i32)> {
        let s = self.objects[i].ship.as_ref()?;
        if s.move_state < 4 {
            return None;
        }
        let t = TILE_SIZE as f32;
        let &(fx, fy) = s.path.front()?;
        let next = ((fx / t).floor() as i32, (fy / t).floor() as i32);
        (next != self.hunt_cell(i)).then_some(next)
    }

    /// 0x4ac8d0 for a leviathan (sub-code 10): over the box of 0x59af48
    /// cells about the ship (the cell it is going into while under way),
    /// clipped to the map, row by row, the slot holding the hunted
    /// leviathan with its picture shown. Showing itself (state 1) it is
    /// taken at the ship's own cell, distance 0, and the ship's guns turn
    /// on it (0x4ac5d0, 0x4bf9b0, 0x4af6a0); coming up, up or going down,
    /// at its cell, and one of the ship's player's ships lying there that
    /// is not hunting a leviathan is asked to make way (0x4b4b50 with
    /// 0x5a; another player's not, 0x4efe20: the port's players are all at
    /// war). Seen further off than `reach`, the ship moves to it (0x4a6270
    /// mode 6; the box's grid of 0x4a5a10 not ported); else, not under
    /// way, it is near.
    fn hunt_look(
        &mut self,
        i: usize,
        lev: u32,
        reach: i32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Look {
        let under = self.hunt_under_way(i);
        let (cx, cy) = under.unwrap_or_else(|| self.hunt_cell(i));
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let half = LOOK / 2;
        let (x0, x1) = ((cx - half).max(0), (cx + half + 1).min(w));
        let (y0, y1) = ((cy - half).max(0), (cy + half + 1).min(h));
        let mut dist = -1;
        let mut to = (cx, cy);
        let mut seen = false;
        for y in y0..y1 {
            for x in x0..x1 {
                if self.leviathan_seen((x, y)) != Some(lev) {
                    continue;
                }
                seen = true;
                if self.leviathan_state(lev) == Some(SHOWING) {
                    to = if dist < 3 { (cx, cy) } else { (x, y) };
                    dist = walk_path::dist(cx, cy, to.0, to.1);
                    self.hunt_fire(i, lev, dist, data, caches);
                } else {
                    if let Some(k) = self.slot_ship((x, y)).filter(|&k| k != i) {
                        let same = self.objects[k].owner == self.objects[i].owner;
                        if same && !self.hunting(k) {
                            if let Some(c) = self.aside_cell(k, MAKE_WAY) {
                                self.go_to_cell(k, c, MAKE_WAY);
                            }
                        }
                    }
                    to = (x, y);
                    dist = walk_path::dist(cx, cy, x, y);
                }
            }
        }
        if !seen {
            return Look::Lost;
        }
        if dist >= 0 {
            if dist > reach {
                self.hunt_plan(i, to);
            } else if under.is_none() {
                return Look::Near;
            }
        }
        Look::Seen
    }

    /// A warship's guns on a leviathan showing itself (0x4bf9b0 with its
    /// groups' targets set by 0x4ac5d0, then 0x4af6a0): items 3 and 4 only,
    /// each loaded and aboard, if it reaches the leviathan's cell from the
    /// ship's (0x4bf5f0: level 2 with Theseus, else its range level). The
    /// hit roll (0x4bf7f0, distance `d`, one draw of the ship's generator)
    /// always gives a hit here: its miss is a chance above 0, which
    /// 0x4af6a0 takes as 100. Aimed at the leviathan's point (0x495b40) from
    /// the hull's middle, more than half the hull picture's width off: a
    /// shot of the item's fire power (0x4bc360, at 100, 0x4bfc20), never
    /// flaming; its reload 0x5c76e0[item x 3 + level] (level 2 with
    /// Atalanta, else its rate level) + a draw % 60 ticks. (Not kept by the
    /// port: the crew's experience, 0x4b4680, +50 a hit; the port keeps a
    /// reload a unit, the item's first standing for it and all set.)
    fn hunt_fire(&mut self, i: usize, lev: u32, d: i32, data: &GameData, caches: &mut Caches) {
        let Some(lc) = self.leviathan_where(lev) else {
            return;
        };
        let here = self.hunt_cell(i);
        let shooters = self.objects[i].shooters();
        let levels = self.levels_of(self.objects[i].owner);
        let Some(s) = self.objects[i].ship.as_ref() else {
            return;
        };
        let (kind, theseus, atalanta) = (
            s.kind,
            s.has_hero(objects::THESEUS),
            s.has_hero(objects::ATALANTA),
        );
        if let Some(c) = self.objects[i].combat.as_mut() {
            c.reloads.resize(shooters.len(), 0.0);
        }
        for item in [3u8, 4] {
            let units: Vec<usize> = (0..shooters.len())
                .filter(|&k| shooters[k] == item)
                .collect();
            let Some(&first) = units.first() else {
                continue;
            };
            let loaded = self.objects[i]
                .combat
                .as_ref()
                .is_some_and(|c| c.reloads[first] <= 0.0);
            if !loaded {
                continue;
            }
            let level = if theseus {
                objects::HERO_LEVEL
            } else {
                levels.reach_for(item)
            };
            let (dx, dy) = ((lc.0 - here.0).abs(), (lc.1 - here.1).abs());
            let (row, col) = (4 - dx, 4 - dy);
            if row < 0 || col < 0 {
                continue;
            }
            let table = &REACH[(item - 3) as usize][level.min(2) as usize];
            if table[row as usize][col as usize] == 0 {
                continue;
            }
            // 0x4bf7f0 (ease 25, 0x598130's 9 for both items): a hit or
            // not, 0x4af6a0 strikes at 100.
            let chance = 25 - d * 33 / (9 << 6) + 43;
            let _hit = (self.ship_draw(i) % 100) < chance as u32;
            let Some(aim) = self.leviathan_aim(lev) else {
                continue;
            };
            let o = &self.objects[i];
            let from = (o.x.floor() as i32, o.y.floor() as i32);
            let half = objects::SHIP_FRAME[kind as usize % 12] as i32 / 2;
            if walk_path::dist(from.0, from.1, aim.0, aim.1) <= half {
                continue;
            }
            let units_of = o.unit_conditions()[item as usize].clone();
            let n = units_of.len() as u32;
            let mean = units_of.iter().map(|&c| c as u32).sum::<u32>() / n.max(1);
            let power = crate::object_info::cache(item, levels.arms_for(item)) * n * mean / 100;
            let shot = super::shot::Firing {
                item,
                letter: None,
                flaming: false,
                from: (from.0 as f32, from.1 as f32),
                target: lev,
                strikes: true,
                damage: power as f32,
                wounds: 0.0,
                wear: 0.0,
                rope: None,
                shooter: Some(o.id),
                by: o.owner,
            };
            self.fire_shot_to(shot, (aim.0 as f32, aim.1 as f32), data, caches);
            let rate = if atalanta {
                objects::HERO_LEVEL
            } else {
                levels.rate_for(item)
            };
            let wait = crate::combat::reload_ticks(item, rate) + self.ship_draw(i) % RELOAD_JITTER;
            if let Some(c) = self.objects[i].combat.as_mut() {
                for &k in &units {
                    c.reloads[k] = crate::clock::ticks(wait);
                }
            }
        }
    }

    /// 0x4b49a0: a warship on a leviathan's cell when it is up moves off:
    /// to a cell of the three by three about it (from one before, clipped
    /// at 0, to two after, clipped at the map's last cell), not its own,
    /// that it may step into (0x499aa0, its direction 0x59791d) with no
    /// leviathan in its slot; one drawn with its generator (event 0x80,
    /// priority 0x32). None: it stays.
    fn hunt_move_off(&mut self, i: usize) {
        let (cx, cy) = self.hunt_cell(i);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let (x0, y0) = ((cx - 1).max(0), (cy - 1).max(0));
        let x1 = if x0 + 3 >= w { w - 1 } else { x0 + 3 };
        let y1 = if y0 + 3 >= h { h - 1 } else { y0 + 3 };
        let mut cells = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                if (x, y) == (cx, cy) {
                    continue;
                }
                let at = (x - cx) * 4 + (y - cy) + 5;
                let dir = usize::try_from(at)
                    .ok()
                    .and_then(|a| super::yard::TOWARD.get(a))
                    .copied()
                    .unwrap_or(0xff);
                if self.may_step(i, (x, y), dir) && !self.leviathan_slot((x, y)) {
                    cells.push((x, y));
                }
            }
        }
        if cells.is_empty() {
            return;
        }
        let c = cells[self.ship_draw(i) as usize % cells.len()];
        self.go_to_cell(i, c, MOVE_OFF);
    }

    /// 0x4aed30: lying on heading 0 or 4 it does not turn; else to the
    /// nearer of the two (0x4b5390: on its lying frames the headings'
    /// difference wrapped to -4..4; the nearer is 0 only when strictly
    /// so), a turning frame a tick, from its heading's (`h x 3 + 0x20`).
    fn hunt_turn_choice(&mut self, i: usize) {
        let s = self.objects[i].ship.as_ref().unwrap();
        let h = s.heading as i32 % 8;
        let away = |to: i32| {
            let d = to - h;
            if d.abs() > 4 {
                if d > 0 {
                    d - 8
                } else {
                    d + 8
                }
            } else {
                d
            }
        };
        let (to, turn) = match h {
            0 => (0, 0),
            4 => (4, 0),
            _ => {
                let (d0, d4) = (away(0), away(4));
                if d0.abs() < d4.abs() {
                    (0, d0.signum())
                } else {
                    (4, d4.signum())
                }
            }
        };
        let hunt = self.hunt_mut(i);
        hunt.turn_to = to as u8;
        hunt.turn = turn as i8;
        hunt.frame = h * 3 + TURN_FIRST;
    }

    /// 0x4ad300(ship, 1, n): the catch stored as leviathan (in its loads
    /// of 100), as much as there is room for; true when that filled it
    /// (the room no more than the catch). Nothing caught: false.
    fn hunt_store(&mut self, i: usize, n: i32) -> bool {
        if n == 0 {
            return false;
        }
        let s = self.objects[i].ship.as_mut().unwrap();
        let room = (objects::cargo_capacity(s.kind) - s.cargo).max(0.0) as i32;
        let full = room <= n;
        let n = n.min(room);
        s.cargo += n as f32;
        s.cargo_kind = Some(Resource::Leviathan);
        full
    }

    /// 0x4ad510 at the hunt's end: with room left and a leviathan in the
    /// box about its old cell (0x4ac610 mode 0: one, it; more, the one the
    /// ship's wavefront reaches most) it hunts that (event 0x40 {0x192, 10,
    /// it}); else to its port `+0x7ad`, or the nearest (0x4c5910), to
    /// unload (event 0x40 code 0xc), both with priority 0x5a. False when
    /// there is neither.
    fn hunt_next(&mut self, i: usize) -> bool {
        let id = self.objects[i].id;
        if self.room(i) > 0 {
            if let Some(r) = self.objects[i]
                .ship
                .as_ref()
                .and_then(|s| s.gathered)
                .map(|g| g.rect)
            {
                if let Some(d) = self.hunt_box(i, r) {
                    let _ = self.order_gather_with(id, d, 0);
                    return true;
                }
            }
        }
        let owner = self.objects[i].owner;
        let home = self.objects[i].ship.as_ref().map_or(0, |s| s.home_port);
        let port = self
            .object_index(home)
            .filter(|&p| {
                home != 0 && self.objects[p].owner == owner && self.objects[p].ship.is_none()
            })
            .or_else(|| self.unloading_site(i).and_then(|p| self.object_index(p)));
        let Some(p) = port else {
            return false;
        };
        // Refused by 0x4aab00 (not a port taking cargo): dropped, the
        // task kept.
        let _ = self.unload_event(i, p, NEXT);
        true
    }

    /// The leviathans in the slots of the box `[x0, x1) x [y0, y1)` (at most
    /// 24, row by row), as 0x4ac610 mode 0 picks: one, it; more, the one
    /// with the greatest wavefront value from the ship (the first on a
    /// tie).
    fn hunt_box(&self, i: usize, (x0, y0, x1, y1): (i32, i32, i32, i32)) -> Option<u32> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let mut hits = Vec::new();
        for y in y0.max(0)..y1.min(h) {
            for x in x0.max(0)..x1.min(w) {
                if hits.len() >= 24 {
                    continue;
                }
                if let Some(l) = self.leviathan_in((x, y)) {
                    hits.push((l, (x, y)));
                }
            }
        }
        match hits.len() {
            0 => return None,
            1 => return Some(hits[0].0),
            _ => {}
        }
        let here = self.hunt_cell(i);
        let deep = objects::deep_draught(self.objects[i].ship.as_ref()?.kind);
        let f = self.nav.wavefront(here, deep, w)?;
        let v = |c: (i32, i32)| f[(c.1 * w + c.0) as usize];
        let mut best = hits[0];
        for &e in &hits[1..] {
            if v(best.1) < v(e.1) {
                best = e;
            }
        }
        Some(best.0)
    }

    /// Sub-code 10 (0x4aeef0), by its step.
    fn hunt_watch(&mut self, i: usize, lev: u32, data: &GameData, caches: &mut Caches) {
        let Some(s) = self.objects[i].ship.as_ref() else {
            return;
        };
        let kind = s.kind;
        let step = s.hunt.map_or(1, |h| h.step);
        let here = self.hunt_cell(i);
        let to_sail = |sc: &mut Scene| {
            if let Some(s) = sc.objects[i].ship.as_mut() {
                s.task = Some(Task::Gather {
                    deposit: lev,
                    phase: GatherPhase::ToDeposit,
                });
                if let Some(h) = s.hunt.as_mut() {
                    h.step = 1;
                }
            }
        };
        match step {
            1 => {
                let reach = if kind <= 1 { 0 } else { 2 };
                match self.hunt_look(i, lev, reach, data, caches) {
                    Look::Lost => return to_sail(self),
                    Look::Seen => return,
                    Look::Near => {}
                }
                let boat = kind <= 1;
                if self.leviathan_state(lev) != Some(UP) || boat {
                    let s = self.objects[i].ship.as_ref().unwrap();
                    let contact = s.grapple.is_some() || s.ram_run.is_some();
                    if s.move_state != 0 || contact {
                        return;
                    }
                    if self.leviathan_up_in(here).is_none() || !boat {
                        return;
                    }
                    // Step 2 for 0x4aed30, then 10; its commands held
                    // (0x4b37d0 with 0x2000).
                    self.hunt_turn_choice(i);
                    let h = self.hunt_mut(i);
                    h.step = 10;
                    h.held = true;
                    return;
                }
                // A warship leaves it to the boats once it is up: off its
                // cell if it lies in it (0x4b49a0), the task over (event 8
                // 0x40); with none in its cell's slot the move too (0xc0).
                let s = self.objects[i].ship.as_mut().unwrap();
                s.task = None;
                if self.leviathan_slot(here) {
                    self.hunt_move_off(i);
                } else {
                    self.objects[i].ship.as_mut().unwrap().path.clear();
                }
            }
            10 => {
                // A turning frame a tick (`+0x79d`), the heading following
                // (`(frame - 0x20) / 3`), till on 0 or 4 at a heading's
                // frame. (The hull picture's y is where it was, `+0x797`.)
                let h = self.hunt_mut(i);
                if h.turn != 0 {
                    h.frame += h.turn as i32;
                    if h.frame > TURN_LAST {
                        h.frame = TURN_FIRST;
                    }
                    if h.frame < TURN_FIRST {
                        h.frame = TURN_LAST;
                    }
                    let at = h.frame - TURN_FIRST;
                    if at / 3 == h.turn_to as i32 && at % 3 == 0 {
                        h.turn = 0;
                    }
                    let (turn, to) = (h.turn, h.turn_to);
                    let s = self.objects[i].ship.as_mut().unwrap();
                    s.heading = (at / 3) as usize;
                    s.rotation = if turn == 0 {
                        s.heading = to as usize;
                        None
                    } else {
                        Some(at as u8)
                    };
                }
                let h = self.hunt_mut(i);
                if h.turn == 0 {
                    // The line goes down (slot 1, `WMOST_`), a frame a
                    // tick from the next.
                    h.step = 0xc;
                    h.line = Some(0);
                }
            }
            0xc => {
                let n = objects::sequence_frames(data, caches, "WMOST_").map_or(0, |f| f.len());
                let h = self.hunt_mut(i);
                let f = h.line.unwrap_or(0) + 1;
                if f >= n as i32 {
                    // Down: the net (slot 3, `WNOSIL_`) goes out.
                    h.line = Some(n as i32 - 1);
                    h.step = 2;
                    h.haul = Some(0);
                } else {
                    h.line = Some(f);
                }
            }
            2 => {
                // 0x4aee40: the hunted leviathan up in its cell's slot: the
                // cast takes up to 50 of it (0x4ad0e0, vt+4 mode 1; notice
                // 0x12f to the group, which the port's zones drop); else
                // the line comes up.
                if self.leviathan_up_in(here) == Some(lev) {
                    let left = self.leviathan_left(lev);
                    let take = if left < CAST + 1 { left } else { CAST };
                    self.leviathan_netted(lev, take);
                    let h = self.hunt_mut(i);
                    h.catch = take;
                    h.step = 6;
                } else {
                    let h = self.hunt_mut(i);
                    h.step = 0x12;
                    h.haul = None;
                }
            }
            6 => {
                // The haul, a frame a tick; at its end the catch is stored
                // (0x4ad300): room left, another cast; full, the line up.
                let n = objects::sequence_frames(data, caches, "WNOSIL_").map_or(0, |f| f.len());
                let h = self.hunt_mut(i);
                let f = h.haul.unwrap_or(0) + 1;
                if f < n as i32 {
                    h.haul = Some(f);
                    return;
                }
                h.haul = Some(0);
                let catch = h.catch;
                let full = self.hunt_store(i, catch);
                let h = self.hunt_mut(i);
                if full {
                    h.catch = 0;
                    h.step = 0x12;
                    h.haul = None;
                } else {
                    h.step = 2;
                }
            }
            0x12 => {
                // The line comes up a frame a tick; below 1 it is in.
                let h = self.hunt_mut(i);
                let f = h.line.unwrap_or(0) - 1;
                if f < 1 {
                    h.line = None;
                    h.step = 0x13;
                } else {
                    h.line = Some(f);
                }
            }
            0x13 => {
                self.hunt_mut(i).step = 5;
            }
            5 => {
                // 0x4b3850: its commands and flags back, the route cleared
                // (0x4a5b70); step 1, then 0x4ad510, or the task ends
                // (event 8 0xc0).
                let h = self.hunt_mut(i);
                h.held = false;
                h.step = 1;
                self.objects[i].ship.as_mut().unwrap().path.clear();
                if !self.hunt_next(i) {
                    let s = self.objects[i].ship.as_mut().unwrap();
                    s.task = None;
                    s.path.clear();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::leviathan::{RISING, SINKING};
    use crate::scene::Order;

    /// miss005: its leviathan (id 13, its cell (33, 1), bounds -/+ 5, an
    /// amount of 100, life 100), the player's boat 10 (item 3) and royal
    /// ship 72 (items 4, 8 and 0). No brain.
    fn miss005() -> Option<(GameData, Caches, Scene)> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
        scene.brain = None;
        Some((data, caches, scene))
    }

    const LEV: u32 = 13;
    const BOAT: u32 = 10;
    const WARSHIP: u32 = 72;

    /// A game tick of the leviathans and the hunters alone.
    fn tick(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.ticks += 1;
        scene.run_leviathans(data, caches);
        scene.run_hunts(data, caches);
    }

    /// Ship `id` lying still in cell `c` on `heading`.
    fn place(scene: &mut Scene, id: u32, c: (i32, i32), heading: usize) {
        let t = TILE_SIZE as f32;
        let k = scene.object_index(id).unwrap();
        scene.objects[k].x = c.0 as f32 * t + t / 2.0;
        scene.objects[k].y = c.1 as f32 * t + t / 2.0;
        let s = scene.objects[k].ship.as_mut().unwrap();
        s.path.clear();
        s.move_state = 0;
        s.rotation = None;
        s.heading = heading;
    }

    fn hunt(scene: &Scene, id: u32) -> Option<Hunt> {
        let k = scene.object_index(id).unwrap();
        scene.objects[k].ship.as_ref().unwrap().hunt
    }

    fn phase(scene: &Scene, id: u32) -> Option<GatherPhase> {
        let k = scene.object_index(id).unwrap();
        match scene.objects[k].ship.as_ref().unwrap().task {
            Some(Task::Gather { phase, .. }) => Some(phase),
            _ => None,
        }
    }

    /// The order reaching ship `id`, then the ticks till it watches over
    /// the leviathan (sub-code 10): one of sub-code 9 when the box's corner
    /// lies more than 2 off (0x4aab00), however near the leviathan.
    fn send(scene: &mut Scene, data: &GameData, caches: &mut Caches, id: u32) {
        scene.order_gather(id, LEV).unwrap();
        if phase(scene, id) == Some(GatherPhase::ToDeposit) {
            tick(scene, data, caches);
        }
        assert_eq!(phase(scene, id), Some(GatherPhase::Collecting));
    }

    /// Ticks till the leviathan is in state `s`.
    fn until(scene: &mut Scene, data: &GameData, caches: &mut Caches, s: u8) {
        for _ in 0..2000 {
            if scene.leviathan_state(LEV) == Some(s) {
                return;
            }
            tick(scene, data, caches);
        }
        panic!("the leviathan never reached state {s}");
    }

    /// Struck down to no life with `+0x119`: it comes up to be hunted.
    fn bring_up(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        tick(scene, data, caches);
        let l = scene.leviathans.get_mut(&LEV).unwrap();
        l.life = 0;
        l.comes_up = true;
        l.wait = 0;
        until(scene, data, caches, UP);
    }

    /// A water cell the warship may lie in, `d` cells across from `c` on
    /// its row or the next (0x557440 at most 2).
    fn beside(scene: &Scene, c: (i32, i32)) -> (i32, i32) {
        for (dx, dy) in [(2, 0), (-2, 0), (1, 1), (-1, 1), (0, 1), (1, 0), (-1, 0)] {
            let at = (c.0 + dx, c.1 + dy);
            if scene.sea_cell(at, true) && !scene.leviathan_slot(at) {
                return at;
            }
        }
        panic!("no water by {c:?}");
    }

    /// Any ship takes the order (capability 0x40): a warship far off sails
    /// to it first (sub-code 9, 0x4ad680), then watches over it once
    /// within 0x59af48 / 2 = 2 cells (sub-code 10).
    #[test]
    fn a_warship_takes_the_hunt() {
        let Some((_data, _caches, mut scene)) = miss005() else {
            return;
        };
        scene.order_gather(WARSHIP, LEV).unwrap();
        assert_eq!(phase(&scene, WARSHIP), Some(GatherPhase::ToDeposit));
        let h = hunt(&scene, WARSHIP).unwrap();
        assert_eq!((h.step, h.first_plan, h.held), (1, true, false));
        // Within 2 of the box's corner (its cell -/+ 2): watching at once.
        let (data, mut caches) = (_data, _caches);
        tick(&mut scene, &data, &mut caches);
        let lc = scene.leviathan_where(LEV).unwrap();
        place(
            &mut scene,
            WARSHIP,
            ((lc.0 - 2).max(0), (lc.1 - 2).max(0)),
            0,
        );
        scene.order_gather(WARSHIP, LEV).unwrap();
        assert_eq!(phase(&scene, WARSHIP), Some(GatherPhase::Collecting));
    }

    /// A warship within reach of a leviathan showing itself shoots item 4
    /// at its heading's point (0x4af6a0, 0x495b40): the item's fire power
    /// at level 0 (60 x 15 x 1 x 100 / 100 = 900), its reload 0x5c76e0 +
    /// 0-59 ticks; landing on its mask it takes `60 x 900 / 100 x 100 /
    /// 10000 = 5` of its life.
    #[test]
    fn a_warship_shoots_a_leviathan_showing_itself() {
        let Some((data, mut caches, mut scene)) = miss005() else {
            return;
        };
        until(&mut scene, &data, &mut caches, SHOWING);
        while scene.leviathans[&LEV].frame < 8 {
            tick(&mut scene, &data, &mut caches);
        }
        assert_eq!(scene.leviathan_state(LEV), Some(SHOWING));
        let lc = scene.leviathan_where(LEV).unwrap();
        let at = beside(&scene, lc);
        place(&mut scene, WARSHIP, at, 0);
        scene.order_gather(WARSHIP, LEV).unwrap();
        let mut fired = None;
        for _ in 0..3 {
            tick(&mut scene, &data, &mut caches);
            if let Some(s) = scene.shots.first() {
                fired = Some(s.bound());
                break;
            }
        }
        let (to, target, power) = fired.expect("no shot");
        let heading = scene.leviathans[&LEV].heading;
        let aim = scene.leviathan_aim(LEV).unwrap();
        assert_eq!(target, LEV);
        assert_eq!((to.0 as i32, to.1 as i32), aim);
        assert_eq!(
            aim,
            (
                lc.0 * 64 + AIM_TEST[heading as usize].0,
                lc.1 * 64 + AIM_TEST[heading as usize].1
            )
        );
        assert_eq!(power, 900.0);
        assert_eq!(scene.shots.len(), 1, "item 4 alone fires");
        let k = scene.object_index(WARSHIP).unwrap();
        let shooters = scene.objects[k].shooters();
        let w = shooters.iter().position(|&s| s == 4).unwrap();
        let r = scene.objects[k].combat.as_ref().unwrap().reloads[w];
        let base = crate::combat::reload_ticks(4, 0);
        let ticks = (r / crate::clock::TICK).round() as u32;
        assert!((base..base + 60).contains(&ticks), "{ticks}");
        // Its point is in its mask but for the loop's first frames.
        assert!(scene.leviathan_hit(LEV, aim));
        for _ in 0..40 {
            scene.run_shots(crate::clock::TICK, &data, &mut caches);
        }
        assert!(scene.shots.is_empty());
        let life = scene.leviathans[&LEV].life;
        assert_eq!(life, 95);
    }

    /// The aim points (0x5bace0) by heading, from the cell's corner.
    const AIM_TEST: [(i32, i32); 8] = [
        (42, 52),
        (40, 23),
        (31, 15),
        (22, 20),
        (23, 52),
        (25, 30),
        (33, 43),
        (35, 29),
    ];

    /// A shot's blow on a leviathan (0x494090 mode 0) comes off its life
    /// only while it shows itself.
    #[test]
    fn a_shot_strikes_only_a_leviathan_showing_itself() {
        let Some((data, mut caches, mut scene)) = miss005() else {
            return;
        };
        tick(&mut scene, &data, &mut caches);
        assert_eq!(scene.leviathan_state(LEV), Some(UNDER_TEST));
        scene.leviathan_shot(LEV, 4, 900, Some((1, 1)));
        assert_eq!(scene.leviathans[&LEV].life, 100);
        until(&mut scene, &data, &mut caches, SHOWING);
        scene.leviathan_shot(LEV, 4, 900, Some((1, 1)));
        assert_eq!(scene.leviathans[&LEV].life, 95);
        // Item 3 at level 0: 40 x 15 = 600, 60 x 600 / 100 x 100 / 10000.
        scene.leviathan_shot(LEV, 3, 600, Some((1, 1)));
        assert_eq!(scene.leviathans[&LEV].life, 92);
        // Letter 1 does nothing (0x598100[1] = 0).
        scene.leviathan_shot(LEV, 1, 600, Some((1, 1)));
        assert_eq!(scene.leviathans[&LEV].life, 92);
    }

    const UNDER_TEST: u8 = crate::scene::leviathan::UNDER;

    /// Its masks (`WHALEM_<row><heading>`): 22 frames on even headings,
    /// 27 on odd ones, one coming up, up or going down, each the size of
    /// its picture; the hit test needs its picture shown.
    #[test]
    fn leviathan_masks() {
        let Some((data, mut caches, mut scene)) = miss005() else {
            return;
        };
        tick(&mut scene, &data, &mut caches);
        let lc = scene.leviathans[&LEV].cell;
        assert!(!scene.leviathan_hit(LEV, (lc.0 * 64 + 32, lc.1 * 64 + 32)));
        until(&mut scene, &data, &mut caches, SHOWING);
        let l = &scene.leviathans[&LEV];
        let masks = l.masks.0.clone().unwrap();
        assert_eq!(masks.len(), if l.heading % 2 == 0 { 22 } else { 27 });
        let k = scene.object_index(LEV).unwrap();
        let img = &scene.objects[k].frames[0][0].image;
        assert!(masks
            .iter()
            .all(|m| (m.width, m.height) == (img.width, img.height)));
        // Off its picture's rect, never.
        let lc = l.cell;
        assert!(!scene.leviathan_hit(LEV, (lc.0 * 64 - 1, lc.1 * 64)));
        bring_up(&mut scene, &data, &mut caches);
        let l = &scene.leviathans[&LEV];
        assert_eq!(l.masks.0.as_ref().unwrap().len(), 1);
        let _ = (RISING, SINKING);
    }

    /// A boat on the cell of a leviathan up nets it (0x4aeef0): its
    /// commands held, the line down (`WMOST_`, 9 frames, a tick each), a
    /// cast of 50 (0x4ad0e0) hauled in over `WNOSIL_`'s 25 ticks and
    /// stored, again till the leviathan is out; then the line up and,
    /// with room left and the leviathan still in the box, it is sent to
    /// it again (0x4ad510).
    #[test]
    fn a_boat_nets_a_leviathan_up() {
        let Some((data, mut caches, mut scene)) = miss005() else {
            return;
        };
        bring_up(&mut scene, &data, &mut caches);
        let lc = scene.leviathan_where(LEV).unwrap();
        place(&mut scene, BOAT, lc, 0);
        send(&mut scene, &data, &mut caches, BOAT);
        let mut steps = Vec::new();
        let mut cargo = Vec::new();
        for _ in 0..70 {
            tick(&mut scene, &data, &mut caches);
            steps.push(hunt(&scene, BOAT).unwrap().step);
            let k = scene.object_index(BOAT).unwrap();
            cargo.push(scene.objects[k].ship.as_ref().unwrap().cargo as i32);
        }
        assert_eq!(steps[0], 10);
        assert!(scene.hunt_holds(BOAT));
        // On heading 0 it does not turn: the line goes down next tick.
        assert_eq!(&steps[1..10], &[0xc; 9]);
        assert_eq!(steps[10], 2);
        assert_eq!(&steps[11..36], &[6; 25]);
        assert_eq!(cargo[35], 0);
        assert_eq!(steps[36], 2);
        assert_eq!(cargo[36], 50);
        assert_eq!(&steps[37..62], &[6; 25]);
        assert_eq!(cargo[62], 100);
        assert_eq!(scene.leviathan_left(LEV), 0);
        // Held: the player's orders are refused.
        let r = scene.apply(
            &Order::Move {
                ship: BOAT,
                x: 0.0,
                y: 0.0,
            },
            &data,
            &mut caches,
        );
        assert!(r.is_err());
        // Casting for nothing till it goes down, then the line comes up
        // (8 ticks), 0x13, 5.
        for _ in 0..4000 {
            if steps.last() == Some(&5) {
                break;
            }
            tick(&mut scene, &data, &mut caches);
            steps.push(hunt(&scene, BOAT).map_or(0, |h| h.step));
        }
        let up = steps.iter().position(|&s| s == 0x12).unwrap();
        assert!(steps[63..up].iter().all(|&s| s == 2 || s == 6));
        assert_eq!(&steps[up..up + 8], &[0x12; 8]);
        assert_eq!(&steps[up + 8..], &[0x13, 5]);
        assert_ne!(scene.leviathan_state(LEV), Some(UP));
        assert!(scene.hunt_holds(BOAT));
        // Step 5: its commands back, and 0x4ad510 sends it to the
        // leviathan again (room left, still in its cell's slot).
        tick(&mut scene, &data, &mut caches);
        assert!(!scene.hunt_holds(BOAT));
        let h = hunt(&scene, BOAT).unwrap();
        assert_eq!((h.step, h.first_plan), (1, true));
        let k = scene.object_index(BOAT).unwrap();
        assert_eq!(scene.objects[k].ship.as_ref().unwrap().cargo, 100.0);
        assert!(phase(&scene, BOAT).is_some());
    }

    /// A boat lying on heading 2 turns to 4 (0x4aed30: equally near, the
    /// way to 4), a turning frame a tick from 0x26 to 0x2c.
    #[test]
    fn a_boat_turns_to_lie_west() {
        let Some((data, mut caches, mut scene)) = miss005() else {
            return;
        };
        bring_up(&mut scene, &data, &mut caches);
        let lc = scene.leviathan_where(LEV).unwrap();
        place(&mut scene, BOAT, lc, 2);
        send(&mut scene, &data, &mut caches, BOAT);
        tick(&mut scene, &data, &mut caches);
        let h = hunt(&scene, BOAT).unwrap();
        assert_eq!((h.step, h.turn_to, h.turn, h.frame), (10, 4, 1, 0x26));
        let mut frames = Vec::new();
        for _ in 0..6 {
            tick(&mut scene, &data, &mut caches);
            frames.push(hunt(&scene, BOAT).unwrap().frame);
        }
        assert_eq!(frames, [0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c]);
        let h = hunt(&scene, BOAT).unwrap();
        assert_eq!((h.step, h.turn), (0xc, 0));
        let k = scene.object_index(BOAT).unwrap();
        let s = scene.objects[k].ship.as_ref().unwrap();
        assert_eq!((s.heading, s.rotation), (4, None));
    }

    /// A warship on the cell of a leviathan up moves off it (0x4b49a0: a
    /// neighbouring cell, priority 0x32) and its hunt ends (event 8 0x40).
    #[test]
    fn a_warship_leaves_a_leviathan_up() {
        let Some((data, mut caches, mut scene)) = miss005() else {
            return;
        };
        bring_up(&mut scene, &data, &mut caches);
        let lc = scene.leviathan_where(LEV).unwrap();
        place(&mut scene, WARSHIP, lc, 0);
        send(&mut scene, &data, &mut caches, WARSHIP);
        tick(&mut scene, &data, &mut caches);
        let k = scene.object_index(WARSHIP).unwrap();
        let s = scene.objects[k].ship.as_ref().unwrap();
        assert_eq!(s.task, None);
        assert_eq!(s.priority, 0x32);
        let &(x, y) = s.path.front().unwrap();
        let to = ((x / 64.0).floor() as i32, (y / 64.0).floor() as i32);
        assert!((to.0 - lc.0).abs() <= 1 && (to.1 - lc.1).abs() <= 1 && to != lc);
    }

    /// A leviathan under water is not seen (0x4ac8d0 gives 0xff): the
    /// hunter turns to sailing to its cell (sub-code 9), and within 2 of
    /// it watches again (0x4ad680), a tick each.
    #[test]
    fn a_hunter_sails_to_a_leviathan_below() {
        let Some((data, mut caches, mut scene)) = miss005() else {
            return;
        };
        tick(&mut scene, &data, &mut caches);
        let lc = scene.leviathan_where(LEV).unwrap();
        assert_eq!(scene.leviathan_state(LEV), Some(UNDER_TEST));
        let at = beside(&scene, lc);
        place(&mut scene, BOAT, at, 0);
        scene.order_gather(BOAT, LEV).unwrap();
        let mut phases = Vec::new();
        for _ in 0..4 {
            tick(&mut scene, &data, &mut caches);
            phases.push(phase(&scene, BOAT).unwrap());
        }
        assert_eq!(scene.leviathan_state(LEV), Some(UNDER_TEST));
        assert!(phases.windows(2).all(|w| w[0] != w[1]), "{phases:?}");
    }

    /// The whole hunt as the game runs it: a warship by a leviathan that
    /// has come up whole after a catch (`+0x119`) with little life left
    /// strikes it down while it shows itself; it comes up to be hunted,
    /// the warship leaves it, and the boat sails onto its cell and nets
    /// all of it, 50 a cast.
    #[test]
    fn a_warship_and_a_boat_hunt_a_leviathan() {
        let Some((data, mut caches, mut scene)) = miss005() else {
            return;
        };
        tick(&mut scene, &data, &mut caches);
        {
            let l = scene.leviathans.get_mut(&LEV).unwrap();
            l.life = 5;
            l.comes_up = true;
        }
        let lc = scene.leviathan_where(LEV).unwrap();
        let at = beside(&scene, lc);
        place(&mut scene, WARSHIP, at, 0);
        let boat_at = (at.0, at.1 + 1);
        place(&mut scene, BOAT, boat_at, 0);
        scene.ticks = crate::scene::order::HOLD;
        scene.order_gather(WARSHIP, LEV).unwrap();
        scene.order_gather(BOAT, LEV).unwrap();
        let mut struck = false;
        let mut left = false;
        for _ in 0..20_000 {
            scene.update(crate::clock::TICK, &data, &mut caches);
            struck |= scene.leviathans[&LEV].life < 1;
            let k = scene.object_index(WARSHIP).unwrap();
            left |= struck && scene.objects[k].ship.as_ref().unwrap().task.is_none();
            let k = scene.object_index(BOAT).unwrap();
            if scene.objects[k].ship.as_ref().unwrap().cargo >= 100.0 {
                break;
            }
        }
        assert!(struck, "the warship struck it down");
        assert!(left, "the warship left it once it was up");
        let k = scene.object_index(BOAT).unwrap();
        assert_eq!(scene.objects[k].ship.as_ref().unwrap().cargo, 100.0);
        assert_eq!(scene.leviathan_left(LEV), 0);
    }
}
