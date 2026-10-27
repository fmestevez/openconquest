//! Boarding and ramming as the original plays them (Run_Ship.cpp,
//! Pos_ship.cpp).
//!
//! A boarder lies in a cell next to its prey and throws a grappling hook
//! (bomb kind 9, `BOMBJ00`, with a line back to it) every so often
//! (0x4b84b0). A hook that holds (0x4bbfc0, the boarded ship's state 9 in
//! 0x4c3410) pulls both ships alongside: each slides a little towards the
//! other and turns parallel to it (0x4b54a0, 0x4b57e0), then furls its
//! sail (0x4b8aa0); the boarders lower a walkway (`MOST0_`), fight
//! (`ABORD_`, the outcome decided as it starts, 0x4b5b00, and dealt as it
//! ends), raise the walkway and both ships unfurl and slide back to the
//! middle of their cells (0x4b5df0 states 0xb, 3, 0xc, 6).
//!
//! A rammer next to its prey sails into it until its bow touches the hull
//! (0x4c2230 state 2, 0x4c1870), shows splinters there (`BOMBH30`, 0x4e0370),
//! recoils, deals the blow as they end (0x4c1c60) and backs off to the
//! middle of its cell (state 6).

use super::*;
use crate::objects::{Grapple, GrappleStage, RamRun, RamStage};

/// Ticks a frame of the sail furling while grappled: the hull's delay of
/// 4 (0x4b5df0 state 8), a frame once more than 4 ticks have passed; four
/// frames.
const FURL_STEP: u32 = 5;
const FURL_FRAMES: u32 = 4;
/// The walkway's frames, one a tick (0x4b8aa0: delay 0).
const PLANK_FRAMES: u32 = 18;
/// How long a rammer's impact lasts when its picture is missing.
const IMPACT_TICKS: u32 = 14;
/// A rammer backs off a pixel on each axis a frame of its hull (delay 2: a
/// frame every 3 ticks, 0x4c2230 state 6).
const BACK_STEP: u32 = 3;
/// How far back a rammer recoils at the impact's second frame, in pixels
/// along its heading (0x4c2230 state 3).
const RECOIL: f32 = 4.0;

/// The eight directions' steps on the grid (0x5c4f4c): 0 east,
/// counter-clockwise.
const UNIT: [(i32, i32); 8] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// The direction of a neighbouring cell (0x59791d).
fn direction(dx: i32, dy: i32) -> Option<usize> {
    UNIT.iter().position(|&u| u == (dx.signum(), dy.signum()))
}

/// How far a grappled ship slides towards the other from its cell's
/// middle (0x4b54a0: 0x59a7c8 across, 0x59a828 up and down, 0x59a888
/// slanting; alike for every type).
fn alongside_offset(dir: usize) -> (f32, f32) {
    let (ux, uy) = UNIT[dir % 8];
    match dir % 4 {
        0 => (10.0 * ux as f32, 0.0),
        2 => (0.0, 13.0 * uy as f32),
        _ => (19.0 * ux as f32, 19.0 * uy as f32),
    }
}

/// The heading a ship grappled from direction `dir` turns to: of the two
/// parallel to the other ship, the one fewer rotations away, the second on
/// a tie (0x4b54a0, 0x4b5390).
pub(super) fn parallel(dir: usize, rotation: i32) -> u8 {
    let (a, b) = match dir % 4 {
        0 => (2, 6),
        1 => (3, 7),
        2 => (0, 4),
        _ => (1, 5),
    };
    let away = |h: i32| {
        let d = (h * 3 - rotation).rem_euclid(24);
        d.min(24 - d)
    };
    if away(a) < away(b) {
        a as u8
    } else {
        b as u8
    }
}

/// Turns a ship's hull one of its 24 rotations towards heading `face`, the
/// shorter way (0x4b57e0); true once it lies so.
fn turn_tick(ship: &mut objects::Ship, face: u8) -> bool {
    let want = face as i32 * 3;
    let mut at = ship.rotation.map_or(ship.heading as i32 * 3, i32::from);
    if at != want {
        let mut d = want - at;
        if d.abs() > 12 {
            d -= 24 * d.signum();
        }
        at = (at + d.signum()).rem_euclid(24);
        ship.heading = (at / 3) as usize;
    }
    if at == want {
        ship.rotation = None;
        ship.heading = face as usize;
        true
    } else {
        ship.rotation = Some(at as u8);
        false
    }
}

/// Where a slide from `from` to `to` stands after `ticks`: along the
/// line's points (0x4b5220, padded to an even count), two a tick; and
/// whether it is over (0x4b57e0).
pub(super) fn slide(from: (f32, f32), to: (f32, f32), ticks: u32) -> ((f32, f32), bool) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let major = dx.abs().max(dy.abs()).round() as u32;
    let count = (major + 1).next_multiple_of(2);
    let k = 2 * ticks;
    let f = if major == 0 {
        1.0
    } else {
        k.min(major) as f32 / major as f32
    };
    ((from.0 + dx * f, from.1 + dy * f), k >= count)
}

/// A rammer's bow by type and hull rotation, from its picture's top-left
/// (0x599204; types 4-9 and 11 alike).
const BOW: [[(u8, u8); 24]; 5] = [
    [
        (56, 29),
        (54, 25),
        (52, 19),
        (47, 11),
        (42, 8),
        (35, 4),
        (28, 2),
        (21, 2),
        (15, 4),
        (10, 10),
        (6, 15),
        (2, 22),
        (1, 31),
        (1, 36),
        (4, 42),
        (7, 47),
        (14, 50),
        (22, 52),
        (28, 54),
        (36, 54),
        (42, 50),
        (47, 47),
        (52, 43),
        (54, 37),
    ],
    [
        (64, 36),
        (63, 27),
        (60, 21),
        (55, 14),
        (48, 10),
        (40, 6),
        (33, 4),
        (24, 5),
        (17, 9),
        (10, 14),
        (5, 23),
        (3, 30),
        (2, 37),
        (3, 44),
        (4, 48),
        (10, 53),
        (17, 58),
        (24, 60),
        (32, 60),
        (40, 60),
        (47, 57),
        (54, 53),
        (60, 45),
        (63, 43),
    ],
    [
        (79, 47),
        (78, 38),
        (75, 29),
        (68, 23),
        (60, 17),
        (51, 15),
        (41, 13),
        (29, 13),
        (20, 16),
        (11, 21),
        (5, 29),
        (2, 38),
        (0, 47),
        (2, 54),
        (4, 64),
        (10, 70),
        (19, 74),
        (30, 79),
        (40, 79),
        (51, 78),
        (61, 75),
        (69, 70),
        (75, 63),
        (78, 56),
    ],
    [
        (72, 42),
        (70, 35),
        (67, 28),
        (62, 23),
        (54, 17),
        (46, 14),
        (36, 12),
        (26, 13),
        (18, 15),
        (10, 21),
        (5, 26),
        (1, 35),
        (0, 42),
        (1, 50),
        (4, 57),
        (9, 63),
        (18, 68),
        (27, 71),
        (38, 72),
        (47, 71),
        (54, 67),
        (61, 63),
        (67, 57),
        (70, 50),
    ],
    [
        (66, 39),
        (65, 28),
        (63, 20),
        (57, 14),
        (50, 8),
        (42, 5),
        (32, 0),
        (21, 6),
        (16, 7),
        (9, 11),
        (4, 21),
        (2, 29),
        (0, 36),
        (2, 43),
        (3, 53),
        (5, 60),
        (13, 63),
        (25, 65),
        (34, 66),
        (45, 68),
        (54, 66),
        (62, 63),
        (65, 56),
        (67, 47),
    ],
];

fn bow_row(kind: u8) -> &'static [(u8, u8); 24] {
    &BOW[match kind % 12 {
        0 => 0,
        1 => 1,
        2 | 3 => 2,
        10 => 4,
        _ => 3,
    }]
}

impl Scene {
    fn cell_of_object(&self, k: usize) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        let o = &self.objects[k];
        ((o.x / t).floor() as i32, (o.y / t).floor() as i32)
    }

    fn cell_middle((cx, cy): (i32, i32)) -> (f32, f32) {
        let t = TILE_SIZE as f32;
        (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0)
    }

    /// Whether ship `k` is busy with another (its contact `+0x8e6`): lying
    /// grappled, ramming or being rammed, or with a hook of its own in the
    /// air.
    pub(super) fn in_contact(&self, k: usize) -> bool {
        let o = &self.objects[k];
        let Some(s) = o.ship.as_ref() else {
            return true;
        };
        s.grapple.is_some()
            || s.ram_run.is_some()
            || matches!(s.task, Some(Task::Board { hooking: true, .. }))
            || self.objects.iter().any(|p| {
                p.ship
                    .as_ref()
                    .and_then(|ps| ps.ram_run)
                    .is_some_and(|r| r.target == o.id)
            })
    }

    /// Ships kept still by a boarding or a ram: the grappled, rammers at
    /// the impact or backing off, and ships being rammed.
    pub(super) fn held_by_contact(&self) -> HashSet<u32> {
        let mut held = HashSet::new();
        for o in &self.objects {
            let Some(s) = o.ship.as_ref() else { continue };
            if s.grapple.is_some() {
                held.insert(o.id);
            }
            if let Some(r) = s.ram_run {
                if r.stage != RamStage::Charge {
                    held.insert(o.id);
                }
                if r.stage == RamStage::Impact {
                    held.insert(r.target);
                }
            }
        }
        held
    }

    /// Whether ships in neighbouring cells `i` and `t` may touch (0x499ca0):
    /// across a corner, not when the two cells beside it hold ships lying
    /// against each other.
    fn corner_free(&self, i: usize, t: usize) -> bool {
        let (a, b) = (self.cell_of_object(i), self.cell_of_object(t));
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        if dx == 0 || dy == 0 {
            return true;
        }
        let (c1, c2) = ((a.0 + dx, a.1), (a.0, a.1 + dy));
        let at = |c: (i32, i32)| {
            (0..self.objects.len())
                .find(|&k| self.objects[k].ship.is_some() && self.cell_of_object(k) == c)
        };
        let (Some(p), Some(q)) = (at(c1), at(c2)) else {
            return true;
        };
        let (pid, qid) = (self.objects[p].id, self.objects[q].id);
        let against = |k: usize, other: u32| {
            self.objects[k].ship.as_ref().is_some_and(|s| {
                s.grapple.is_some_and(|g| g.with == other)
                    || s.ram_run.is_some_and(|r| r.target == other)
            })
        };
        !(against(p, qid) || against(q, pid))
    }

    /// Ticks between a ship's grappling hooks: the reload of item 8 at the
    /// player's rate of fire level for the strong weapons (0x4b84b0 reads
    /// 0x5c7740, the 25th entry of the reload table 0x5c76e0, by player
    /// byte +0x90).
    fn hook_reload(&self, i: usize) -> f32 {
        let levels = self.objects[i]
            .owner
            .and_then(|o| self.levels.get(&o))
            .copied()
            .unwrap_or_default();
        combat::reload_seconds(8, levels.rate[2])
    }

    /// Boarder `i`, next to its prey `ti`, throws a hook if its last one was
    /// long enough ago (0x4b84b0). The throw itself always "hits" (item 9,
    /// 0x4bf7f0); it flies wide when the prey carries the magic wood
    /// (0x4ba190), lies across a blocked corner (0x499ca0), or is under way
    /// and about to leave its cell (`slipping`).
    pub(super) fn throw_hook(&mut self, i: usize, ti: usize, data: &GameData, caches: &mut Caches) {
        let Some(Task::Board { target, thrown, .. }) = self.objects[i].ship.as_ref().unwrap().task
        else {
            return;
        };
        // (Half a tick spared for the clock's rounding.)
        let reload = self.hook_reload(i) - crate::clock::TICK / 2.0;
        if thrown.is_some_and(|t| self.clock - t < reload) || self.in_contact(i) {
            return;
        }
        let wood = self.objects[ti]
            .ship
            .as_ref()
            .is_some_and(|s| s.has_treasure(objects::MAGIC_WOOD));
        let strikes = !wood && self.corner_free(i, ti) && !self.slipping(ti);
        let (id, from) = (self.objects[i].id, (self.objects[i].x, self.objects[i].y));
        self.objects[i].ship.as_mut().unwrap().task = Some(Task::Board {
            target,
            thrown: Some(self.clock),
            hooking: true,
        });
        self.fire_shot(
            shot::Firing {
                item: shot::HOOK,
                letter: None,
                flaming: false,
                from,
                target,
                strikes,
                damage: 0.0,
                wounds: 0.0,
                wear: 0.0,
                rope: Some(id),
            },
            data,
            caches,
        );
    }

    /// Whether ship `ti` is under way and about to leave its cell: a ship
    /// slipping away cannot be hooked (0x4b84b0 with 0x4b5140). Its way
    /// left to the cell's edge ahead (twice it straight on, three times on
    /// a diagonal; 0x4b5140 by heading +0x5af, the south-west one counted
    /// twice as the original does) times 3000 over its speed (+0x59b, in
    /// 1/6000 pixel a tick) under 17.
    pub(super) fn slipping(&self, ti: usize) -> bool {
        let o = &self.objects[ti];
        let Some(ship) = o.ship.as_ref() else {
            return false;
        };
        let speed = ship.velocity * crate::clock::TICK * 6000.0;
        if speed <= 0.0 {
            return false;
        }
        let t = TILE_SIZE as f32;
        let (cx, cy) = ((o.x / t).floor() * t, (o.y / t).floor() * t);
        let (east, west) = (cx + t - o.x, o.x - cx);
        let (north, south) = (o.y - cy, cy + t - o.y);
        let way = match ship.heading % 8 {
            0 => east * 2.0,
            1 => east * 3.0,
            2 => north * 2.0,
            3 => north * 3.0,
            4 | 5 => west * 2.0,
            6 => south * 2.0,
            _ => south * 3.0,
        };
        way * 3000.0 / speed < 17.0
    }

    /// A hook thrown by `from` lands, on ship `target` if `hit`. It holds
    /// if the prey is free and still lies in the next cell (0x4bbfc0, the
    /// boarded ship's state 9): both ships are then grappled.
    pub(super) fn hook_landed(&mut self, from: u32, target: u32, hit: bool) {
        let Some(i) = self.object_index(from) else {
            return;
        };
        let Some(Task::Board {
            target: prey,
            thrown,
            hooking: true,
        }) = self.objects[i].ship.as_ref().and_then(|s| s.task)
        else {
            return;
        };
        self.objects[i].ship.as_mut().unwrap().task = Some(Task::Board {
            target: prey,
            thrown,
            hooking: false,
        });
        let Some(ti) = self.object_index(target).filter(|_| hit && prey == target) else {
            return;
        };
        let (a, b) = (self.cell_of_object(i), self.cell_of_object(ti));
        let next = (a.0 - b.0).abs().max((a.1 - b.1).abs()) == 1;
        let afloat = self.objects[ti]
            .combat
            .as_ref()
            .is_none_or(|c| c.hull > 0.0);
        if !next
            || !afloat
            || self.objects[ti].ship.is_none()
            || !Self::are_enemies(&self.objects[i], &self.objects[ti])
            || self.in_contact(i)
            || self.in_contact(ti)
            || !self.corner_free(i, ti)
        {
            return;
        }
        for (k, other) in [(i, ti), (ti, i)] {
            let (here, there) = (self.cell_of_object(k), self.cell_of_object(other));
            let dir = direction(there.0 - here.0, there.1 - here.1).unwrap_or(0);
            let home = Self::cell_middle(here);
            let (ox, oy) = alongside_offset(dir);
            let with = self.objects[other].id;
            let o = &mut self.objects[k];
            let from = (o.x, o.y);
            let s = o.ship.as_mut().unwrap();
            let rotation = s.rotation.map_or(s.heading as i32 * 3, i32::from);
            s.path.clear();
            s.velocity = 0.0;
            s.face = None;
            s.grapple = Some(Grapple {
                with,
                boarder: k == i,
                stage: GrappleStage::Pull,
                at: (home.0 + ox, home.1 + oy),
                home,
                from,
                face: parallel(dir, rotation),
                facing: dir as u8,
                ticks: 0,
                clock: 0.0,
                fight: 0,
                won: false,
                rope: k == ti,
            });
        }
    }

    /// Grappled ships go on with their boarding, a game tick at a time.
    pub(super) fn run_grapples(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.grapple.is_some()))
            .map(|o| o.id)
            .collect();
        for id in ids {
            let Some(k) = self.object_index(id) else {
                continue;
            };
            let Some(g) = self.objects[k].ship.as_mut().unwrap().grapple.as_mut() else {
                continue;
            };
            g.clock += dt;
            while let Some(k) = self.object_index(id) {
                let Some(g) = self.objects[k]
                    .ship
                    .as_mut()
                    .and_then(|s| s.grapple.as_mut())
                else {
                    break;
                };
                if g.clock < crate::clock::TICK {
                    break;
                }
                g.clock -= crate::clock::TICK;
                self.grapple_tick(k, data, caches);
            }
        }
    }

    /// The other ship of `k`'s grapple, while it is grappled to `k` too.
    fn partner(&self, k: usize) -> Option<(usize, Grapple)> {
        let id = self.objects[k].id;
        let g = self.objects[k].ship.as_ref()?.grapple?;
        let p = self.object_index(g.with)?;
        let pg = self.objects[p].ship.as_ref()?.grapple?;
        (pg.with == id).then_some((p, pg))
    }

    fn grapple_tick(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        use GrappleStage::*;
        let id = self.objects[k].id;
        let mut g = self.objects[k].ship.as_ref().unwrap().grapple.unwrap();
        let partner = self.partner(k);
        g.ticks += 1;
        // A ship left alone (the other sunk or gone) leaves.
        if partner.is_none() && g.stage != Leave {
            g = self.leave(k, g);
        }
        match g.stage {
            Pull => {
                let (pos, slid) = slide(g.from, g.at, g.ticks);
                let o = &mut self.objects[k];
                (o.x, o.y) = pos;
                let turned = turn_tick(o.ship.as_mut().unwrap(), g.face);
                // The boarders furl once the boarded ship lies alongside
                // too (0x4b5df0 state 8: its state 10, 3 or 4).
                let ready = !g.boarder || partner.is_some_and(|(_, pg)| pg.stage != Pull);
                if slid && turned && ready {
                    g.stage = Furl;
                    g.ticks = 0;
                }
            }
            Furl => {
                if g.ticks >= FURL_STEP * FURL_FRAMES {
                    g.ticks = 0;
                    if g.boarder {
                        g.stage = Plank;
                    } else {
                        // The hook's line goes with the sail furled
                        // (0x4c3410 state 10).
                        g.rope = false;
                        g.stage = Lie;
                    }
                }
            }
            Lie => {
                if partner.is_some_and(|(_, pg)| pg.stage == Leave) {
                    g = self.leave(k, g);
                }
            }
            Plank => {
                if g.ticks >= PLANK_FRAMES {
                    g = self.start_fight(k, g, data, caches);
                }
            }
            Fight => {
                if g.ticks >= g.fight {
                    let with = g.with;
                    g.stage = Retract;
                    g.ticks = 0;
                    self.objects[k].ship.as_mut().unwrap().grapple = Some(g);
                    self.board_end(id, with, g.won, data, caches);
                    return;
                }
            }
            Retract => {
                if g.ticks >= PLANK_FRAMES {
                    g = self.leave(k, g);
                }
            }
            Leave => {
                let (pos, slid) = slide(g.from, g.home, g.ticks);
                let o = &mut self.objects[k];
                (o.x, o.y) = pos;
                let s = o.ship.as_mut().unwrap();
                // The sail unfurls a frame a tick (0x4b5df0 state 6).
                let furled = s.furl >= 1.0;
                if furled {
                    s.furl -= 1.0;
                }
                if slid && !furled {
                    s.furl = 0.0;
                    s.grapple = None;
                    return;
                }
            }
        }
        let s = self.objects[k].ship.as_mut().unwrap();
        if matches!(g.stage, Furl) {
            s.furl = (g.ticks / FURL_STEP).min(FURL_FRAMES - 1) as f32 + 0.5;
        }
        s.grapple = Some(g);
    }

    /// Ship `k` sets off back to the middle of its cell.
    fn leave(&mut self, k: usize, mut g: Grapple) -> Grapple {
        let o = &self.objects[k];
        g.stage = GrappleStage::Leave;
        g.ticks = 0;
        g.from = (o.x, o.y);
        g.rope = false;
        g
    }

    /// The walkway is down: the fight is decided (0x4b5b00) and shown.
    fn start_fight(
        &mut self,
        k: usize,
        mut g: Grapple,
        data: &GameData,
        caches: &mut Caches,
    ) -> Grapple {
        let Some((p, _)) = self.partner(k) else {
            return self.leave(k, g);
        };
        g.won = self.board_roll(k, p);
        g.stage = GrappleStage::Fight;
        g.ticks = 0;
        let (x, y) = (self.objects[k].x, self.objects[k].y);
        // The grappling's sound (event 83, 0x4b5df0 state 0xb).
        self.sounds.push(("abor6a", x, y));
        let name = format!("ABORD_{}{}", u8::from(!g.won), g.facing);
        let frames = objects::sequence_frames(data, caches, &name).unwrap_or_default();
        g.fight = if frames.is_empty() {
            (combat::BOARD_ROUND / crate::clock::TICK) as u32
        } else {
            frames.len() as u32
        };
        // On the walkway's place (slot 3 takes slot 1's corner, 0x4b5df0).
        if let (Some(tl), Some(l)) = (
            self.plank_corner(k, g),
            frames.first().and_then(|f| f.first()),
        ) {
            let (w, h) = (l.image.width as f32, l.image.height as f32);
            self.effects.push(Effect {
                x: tl.0 + w / 2.0 - l.dx as f32,
                y: tl.1 + h / 2.0 - l.dy as f32,
                frames,
                age_ms: 0.0,
                follow: None,
                life_ms: None,
                frame_ms: crate::clock::TICK_MS as f32,
                tornado: None,
                site: None,
            });
        }
        g
    }

    /// The top-left of boarder `k`'s walkway: from its picture's, by type,
    /// side and facing (0x4b8aa0 with 0x59a8e8, 0x4b8a40).
    fn plank_corner(&self, k: usize, g: Grapple) -> Option<(f32, f32)> {
        let o = &self.objects[k];
        let s = o.ship.as_ref()?;
        let kind = s.kind as usize % 12;
        let facing = g.facing as usize % 8;
        let side = unload::walkway_side(facing, s.heading);
        let (ox, oy) = unload::WALKWAY_AT[kind.min(4)][side][facing];
        let half = objects::SHIP_FRAME[kind] / 2.0;
        Some((o.x - half + ox as f32, o.y - half + oy as f32))
    }

    /// The boarders' walkways to draw: frame and the point drawn about.
    pub fn planks_shown(&self) -> Vec<(&[objects::Layer], f32, f32)> {
        let mut out = Vec::new();
        for (k, o) in self.objects.iter().enumerate() {
            let Some(g) = o.ship.as_ref().and_then(|s| s.grapple) else {
                continue;
            };
            let last = PLANK_FRAMES as i32 - 1;
            let f = match g.stage {
                GrappleStage::Plank => (g.ticks as i32).min(last),
                GrappleStage::Fight => last,
                GrappleStage::Retract => last - g.ticks as i32,
                _ => continue,
            };
            let frames = &self.plank_art[g.facing as usize % 8];
            let (Some(tl), Some(l)) = (
                self.plank_corner(k, g),
                frames.last().and_then(|f| f.first()),
            ) else {
                continue;
            };
            let Some(frame) = usize::try_from(f).ok().and_then(|f| frames.get(f)) else {
                continue;
            };
            let (w, h) = (l.image.width as f32, l.image.height as f32);
            out.push((frame.as_slice(), tl.0 + w / 2.0, tl.1 + h / 2.0));
        }
        out
    }

    /// Hooks' lines: from each hook in the air to its ship, and between
    /// grappled ships until the boarded one has furled.
    pub fn ropes(&self) -> Vec<((f32, f32), (f32, f32))> {
        let mut out: Vec<_> = self
            .shots
            .iter()
            .filter_map(|s| {
                let o = &self.objects[self.object_index(s.rope?)?];
                Some(((s.x, s.y), (o.x, o.y)))
            })
            .collect();
        for o in &self.objects {
            let Some(g) = o.ship.as_ref().and_then(|s| s.grapple) else {
                continue;
            };
            if let (true, Some(p)) = (g.rope, self.object_index(g.with)) {
                out.push(((o.x, o.y), (self.objects[p].x, self.objects[p].y)));
            }
        }
        out
    }

    /// Rammer `i` next to its prey `ti` charges it, if neither is busy with
    /// another ship (0x4c1760).
    pub(super) fn start_ram(&mut self, i: usize, ti: usize) {
        if self.in_contact(i) || self.in_contact(ti) {
            return;
        }
        let home = Self::cell_middle(self.cell_of_object(i));
        let (target, to) = (
            self.objects[ti].id,
            (self.objects[ti].x, self.objects[ti].y),
        );
        let s = self.objects[i].ship.as_mut().unwrap();
        s.ram_run = Some(RamRun {
            target,
            stage: RamStage::Charge,
            home,
            ticks: 0,
            clock: 0.0,
            frames: 0,
        });
        s.path = [to].into();
    }

    /// Where ship `i`'s bow lies (0x599204 by type and hull rotation).
    pub(super) fn bow(&self, i: usize) -> Option<(f32, f32)> {
        let o = &self.objects[i];
        let s = o.ship.as_ref()?;
        let rot = s.rotation.map_or(s.heading * 3, usize::from) % 24;
        let (bx, by) = bow_row(s.kind)[rot];
        let half = objects::SHIP_FRAME[s.kind as usize % 12] / 2.0;
        Some((o.x - half + bx as f32, o.y - half + by as f32))
    }

    /// Whether rammer `i`'s bow touches ship `ti`'s hull: a solid pixel of
    /// its picture as it lies (0x4c1870, 0x557640).
    fn bow_touches(&self, i: usize, ti: usize) -> bool {
        let Some((bx, by)) = self.bow(i) else {
            return false;
        };
        let t = &self.objects[ti];
        let Some(s) = t.ship.as_ref() else {
            return false;
        };
        let idx = match s.rotation {
            Some(r) => 32 + r as usize % 24,
            None => s.heading * 4,
        };
        match t.frames.get(idx).filter(|l| !l.is_empty()) {
            Some(layers) => layers.iter().any(|l| {
                let left = (t.x + l.dx as f32 - l.image.width as f32 / 2.0).round();
                let top = (t.y + l.dy as f32 - l.image.height as f32 / 2.0).round();
                l.image
                    .is_solid((bx - left).floor() as i32, (by - top).floor() as i32)
            }),
            None => (bx - t.x).hypot(by - t.y) < objects::SHIP_FRAME[s.kind as usize % 12] / 4.0,
        }
    }

    /// Rammers charge, strike and back off.
    pub(super) fn run_ram_runs(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.ram_run.is_some()))
            .map(|o| o.id)
            .collect();
        for id in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let r = self.objects[i].ship.as_ref().unwrap().ram_run.unwrap();
            if r.stage == RamStage::Charge {
                self.charge(i, r, data, caches);
                continue;
            }
            let mut r = r;
            r.clock += dt;
            while r.clock >= crate::clock::TICK {
                r.clock -= crate::clock::TICK;
                r.ticks += 1;
                let Some(next) = self.ram_tick(id, r, data, caches) else {
                    break;
                };
                r = next;
            }
            if let Some(s) = self
                .object_index(id)
                .and_then(|i| self.objects[i].ship.as_mut())
            {
                if s.ram_run.is_some() {
                    s.ram_run = Some(r);
                }
            }
        }
    }

    /// A charging rammer keeps after its prey until its bow touches it.
    fn charge(&mut self, i: usize, r: RamRun, data: &GameData, caches: &mut Caches) {
        let ti = self.object_index(r.target).filter(|&t| {
            self.objects[t].ship.is_some() && Self::are_enemies(&self.objects[i], &self.objects[t])
        });
        let still_ordered = matches!(
            self.objects[i].ship.as_ref().unwrap().task,
            Some(Task::Ram { target }) if target == r.target
        );
        let (Some(ti), true) = (ti, still_ordered) else {
            let s = self.objects[i].ship.as_mut().unwrap();
            s.ram_run = None;
            if still_ordered {
                s.task = None;
                s.path.clear();
            }
            return;
        };
        let to = (self.objects[ti].x, self.objects[ti].y);
        let arrived = self.objects[i].ship.as_ref().unwrap().path.is_empty();
        if !(arrived || self.bow_touches(i, ti)) {
            let s = self.objects[i].ship.as_mut().unwrap();
            if s.path
                .back()
                .is_none_or(|&p| (p.0 - to.0).hypot(p.1 - to.1) > 1.0)
            {
                s.path = [to].into();
            }
            return;
        }
        // The bow is in: the impact (0x4c2230 state 3, sound event 84).
        let bow = self
            .bow(i)
            .unwrap_or((self.objects[i].x, self.objects[i].y));
        self.sounds.push(("109", bow.0, bow.1));
        let frames = objects::sequence_frames(data, caches, "BOMBH30").unwrap_or_default();
        let n = if frames.is_empty() {
            IMPACT_TICKS
        } else {
            frames.len() as u32
        };
        if !frames.is_empty() {
            self.effects.push(Effect {
                x: bow.0,
                y: bow.1,
                frames,
                age_ms: 0.0,
                follow: None,
                life_ms: None,
                frame_ms: crate::clock::TICK_MS as f32,
                tornado: None,
                site: None,
            });
        }
        if let Some(t) = self.objects[ti].ship.as_mut() {
            t.path.clear();
        }
        let s = self.objects[i].ship.as_mut().unwrap();
        s.path.clear();
        s.rotation = None;
        s.ram_run = Some(RamRun {
            stage: RamStage::Impact,
            ticks: 0,
            clock: 0.0,
            frames: n,
            ..r
        });
    }

    /// One tick of a rammer at the impact or backing off; `None` once it
    /// is done.
    fn ram_tick(
        &mut self,
        id: u32,
        mut r: RamRun,
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<RamRun> {
        let i = self.object_index(id)?;
        match r.stage {
            RamStage::Charge => Some(r),
            RamStage::Impact => {
                // It recoils at the second frame.
                if r.ticks == 2 {
                    let o = &mut self.objects[i];
                    let h = o.ship.as_ref().unwrap().heading;
                    let (ux, uy) = UNIT[h % 8];
                    o.x -= RECOIL * ux as f32;
                    o.y -= RECOIL * uy as f32;
                }
                if r.ticks >= r.frames {
                    if let Some(ti) = self.object_index(r.target) {
                        self.ram(i, ti, data, caches);
                    }
                    r.stage = RamStage::Back;
                    r.ticks = 0;
                }
                Some(r)
            }
            RamStage::Back => {
                if r.ticks % BACK_STEP == 0 {
                    let o = &mut self.objects[i];
                    let step = |v: f32, to: f32| v + (to - v).clamp(-1.0, 1.0);
                    o.x = step(o.x, r.home.0);
                    o.y = step(o.y, r.home.1);
                    if (o.x, o.y) == r.home {
                        o.ship.as_mut().unwrap().ram_run = None;
                        return None;
                    }
                }
                Some(r)
            }
        }
    }
}
