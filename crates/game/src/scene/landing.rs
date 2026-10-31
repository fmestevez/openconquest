//! A founding ship's landing (Run_Ship.cpp 0x4b3370, its ship tick) and
//! the landing party it puts ashore (LandingClassTy, class 0x1fc,
//! 0x4221c0).
//!
//! At its berth (state 1) the ship closes in on the cell it lands on and
//! turns side-on (0x4b2ff0, state 2: 0x4b2a40), furls its sail (state 8:
//! 0x4b89b0), lowers the walkway `AMOST_<dir>` (states 4, 5: 0x4b2b00,
//! 0x4b2f00), shows the party `ANOSIL_<dir>` going down it (state 6), at
//! whose end the party is made (0x4b2d60, message 0x1a67), raises the
//! walkway (state 7) and backs out to its cell's middle (state 3:
//! 0x4b1d40); then its order is done (message 8). The party walks a
//! quarter-cell route to the site, a `LNDSPR_<dir>` run a leg (0x421d00,
//! 0x422140), and founds there (0x421f00).

use super::*;
use crate::objects::Landing;

/// The direction of the cell `(dx, dy)` away (0x59791d, index `5 + dx x 4
/// + dy`); 0xff for the cell itself.
const NEIGHBOUR_DIR: [u8; 11] = [3, 4, 5, 0xff, 2, 0xff, 6, 0xff, 1, 0, 7];

/// The party's start from the walkway's top-left, by direction
/// (0x59a188, 8 bytes a direction).
const PARTY_FROM: [(i32, i32); 8] = [
    (46, 17),
    (0, 0),
    (6, 21),
    (0, 0),
    (6, 17),
    (0, 0),
    (7, 48),
    (0, 0),
];

/// The hull's frame delay (`+0x7d` = 2, set at the ship's creation,
/// 0x4dfe00, and by 0x4dec40 / 0x4e0160; 0x4a3150 restores it): a hull
/// step once more than 2 ticks have passed.
const HULL_DELAY: u32 = 2;

/// The shadow colour of the party's picture (0x50e130(0, 1, 0x2a)).
pub const PARTY_SHADE: u8 = 0x2a;

/// A landing party (0x4221c0): its record's kind (1 settlement, 2 fort),
/// class and cell (`+0x89`, `+0x8d`), where it stands (`+0xa06`,
/// `+0xa0a`), its route (`+0xa46`, quarter centres) and the leg it walks.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Party {
    pub owner: (u32, u32),
    pub class: u32,
    pub site: (i32, i32),
    pub point: (i32, i32),
    pub route: Vec<(i32, i32, u8)>,
    /// The entry it walks to (`+0xa42`), the leg's ends (`+0xa2e`..
    /// `+0xa3a`), direction (`+0xa05`) and frame (picture `+0x45`).
    pub entry: usize,
    pub from: (i32, i32),
    pub to: (i32, i32),
    pub dir: u8,
    pub frame: i32,
    /// A leg under way (`+0xa2a`); its first step taken (`+0xa12`); on the
    /// site's cell (`+0xa26`).
    pub walking: bool,
    pub started: bool,
    pub arrived: bool,
}

/// C's `v / 2^n` (truncating), as the original's shifts with the sign
/// fix-up.
fn tdiv(v: i32, d: i32) -> i32 {
    v / d
}

impl Scene {
    /// Whether ship `id` holds its commands for a landing (0x4b37d0 with
    /// 0x2000, till 0x4b3850 at its end).
    pub fn landing_holds(&self, id: u32) -> bool {
        self.object_index(id)
            .and_then(|i| self.objects[i].ship.as_ref())
            .is_some_and(|s| s.landing.is_some())
    }

    /// A stop reaching a landing ship (0x4aa540 -> 0x4abc60): `+0x73a`
    /// set, the landing called off. True when it was landing.
    pub(super) fn stop_landing(&mut self, i: usize) -> bool {
        match self.objects[i]
            .ship
            .as_mut()
            .and_then(|s| s.landing.as_mut())
        {
            Some(l) => {
                l.abort = true;
                true
            }
            None => false,
        }
    }

    /// The founding ships' and landing parties' ticks.
    pub(super) fn run_landings(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship
                    .as_ref()
                    .is_some_and(|s| matches!(s.task, Some(Task::Found { .. })))
            })
            .map(|o| o.id)
            .collect();
        for id in ids {
            if let Some(k) = self.object_index(id) {
                self.landing_tick(k, data, caches);
            }
        }
        // The parties' message 0 (0x4221c0).
        let mut k = 0;
        while k < self.parties.len() {
            if self.party_tick(k, data, caches) {
                self.parties.remove(k);
            } else {
                k += 1;
            }
        }
    }

    /// Ship `k`'s founding order at its tick (0x4b3370).
    fn landing_tick(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let o = &self.objects[k];
        let s = o.ship.as_ref().unwrap();
        let Some(Task::Found {
            cx,
            cy,
            class,
            berth,
            land,
        }) = s.task
        else {
            return;
        };
        let Some(mut l) = s.landing.clone() else {
            // State 1: away from the berth it is sent there again
            // (0x4a6270); there and lying still (`+0x58c` 0), the site
            // checked (0x4e4420): refused, the order is done (message 8);
            // else the close-in (0x4b2ff0), the commands held (0x4b37d0)
            // and state 2.
            if !s.path.is_empty() || s.move_state != 0 {
                return;
            }
            let here = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
            if here != berth {
                let (bx, by) = (berth.0 as f32 * t + t / 2.0, berth.1 as f32 * t + t / 2.0);
                if let Some(path) = self.plan_route(k, bx, by) {
                    self.objects[k].ship.as_mut().unwrap().path = path;
                }
                return;
            }
            if self.check_ground(class, cx, cy).is_err() {
                self.objects[k].ship.as_mut().unwrap().task = None;
                return;
            }
            let l = self.close_in(k, land);
            self.objects[k].ship.as_mut().unwrap().landing = Some(l);
            return;
        };
        l.hull_ticks += 1;
        let hull_step = l.hull_ticks > HULL_DELAY;
        let kind = s.kind as usize % 12;
        match l.state {
            // 0x4b2a40 -> 0x4b1a90.
            2 => {
                if self.close_in_step(k, &mut l) {
                    if l.abort {
                        l.back = l.count;
                    }
                    if self.check_ground(class, cx, cy).is_ok() {
                        l.back = l.at;
                        l.state = 8;
                    } else {
                        l.state = 3;
                        l.back = l.count;
                    }
                }
            }
            // 0x4b89b0: the sail furls, from `h x 8 + 0x58` a frame a
            // hull step; on the fourth (`+ 0x5c`) it stays at `+ 0x5b`,
            // furled (`+0x25b` = 5).
            8 => {
                if l.abort {
                    l.state = 3;
                } else if hull_step {
                    l.hull_ticks = 0;
                    l.furl = l.furl.max(0) + 1;
                    if l.furl >= 4 {
                        l.furl = 3;
                        l.state = 4;
                    }
                    let s = self.objects[k].ship.as_mut().unwrap();
                    s.rotation = None;
                    s.furl = l.furl as f32 + 0.5;
                }
            }
            // 0x4b2d50 -> 0x4b2b00: the walkway (slot 1, picture 0x3e6 set
            // to `AMOST_<+0x76d>`), at the hull picture's top-left plus
            // the type's, direction's and shore's offset (0x599e88,
            // 0x59a408 / 0x59a468, 0x59a128, 0x59a708 / 0x59a768), frame
            // 0, its clock now.
            4 => {
                if l.abort {
                    l.state = 3;
                } else {
                    let o = &self.objects[k];
                    let half = objects::SHIP_FRAME[kind] / 2.0;
                    let off =
                        unload::walkway_offset(kind, l.dir as usize, l.group.map(usize::from));
                    // No offset for the shore: the picture keeps its new
                    // place (0, 0) (0x545e70).
                    l.plank_at = off.map_or((0.0, 0.0), |(dx, dy)| {
                        (o.x - half + dx as f32, o.y - half + dy as f32)
                    });
                    l.plank = Some(0);
                    l.state = 5;
                }
            }
            // 0x4b2f00: a walkway frame a tick (delay 0); past its last
            // the party's picture (slot 3, 0x3de as `ANOSIL_<+0x76b>`) at
            // the walkway's top-left, frame 0, and state 6.
            5 => {
                if l.abort {
                    l.plank = None;
                    l.porters = None;
                    l.state = 3;
                } else {
                    let n = self.landing_art[l.walk_dir as usize % 8][0].len() as i32;
                    let f = l.plank.unwrap_or(0) + 1;
                    if n <= f {
                        l.plank = Some(n - 1);
                        l.porters = Some(0);
                        l.state = 6;
                    } else {
                        l.plank = Some(f);
                    }
                }
            }
            // A party frame a tick up to 0x17 (0x15 north); there the
            // party is made at the walkway's top-left plus 0x59a188[dir]
            // (0x4b2d60), and state 7.
            6 => {
                if l.abort {
                    l.porters = None;
                    l.state = 7;
                } else {
                    let last = if l.dir == 2 { 0x15 } else { 0x17 };
                    let f = l.porters.unwrap_or(0) + 1;
                    if last <= f {
                        let (dx, dy) = PARTY_FROM[l.dir as usize % 8];
                        let at = (
                            l.plank_at.0.round() as i32 + dx,
                            l.plank_at.1.round() as i32 + dy,
                        );
                        let owner = self.objects[k].owner;
                        l.porters = None;
                        l.state = 7;
                        if let Some(owner) = owner {
                            self.make_party(owner, class, (cx, cy), at, data, caches);
                        }
                    } else {
                        l.porters = Some(f);
                    }
                }
            }
            // The walkway comes up a frame a tick; at 0 it goes, state 3.
            7 => {
                let f = l.plank.unwrap_or(0) - 1;
                if f < 1 {
                    l.plank = None;
                    l.state = 3;
                } else {
                    l.plank = Some(f);
                }
            }
            // 0x4b1d40: a hull step backs it two points of its line and
            // unfurls a frame; below 1 it lies in its cell's middle, the
            // hull on its heading's first frame, and the order is done
            // (0x4b3850, message 8).
            3 if hull_step => {
                l.hull_ticks = 0;
                l.back -= 2;
                if l.furl > 0 {
                    l.furl -= 1;
                }
                let o = &mut self.objects[k];
                let s = o.ship.as_mut().unwrap();
                if l.furl >= 0 {
                    s.furl = l.furl as f32 + 0.5;
                }
                if l.back < 1 {
                    let (cx, cy) = ((o.x / t).floor(), (o.y / t).floor());
                    (o.x, o.y) = (cx * t + t / 2.0, cy * t + t / 2.0);
                    let s = o.ship.as_mut().unwrap();
                    s.furl = 0.0;
                    s.rotation = None;
                    s.landing = None;
                    s.task = None;
                    s.close_in = objects::CloseIn {
                        dir: l.dir,
                        group: l.group.unwrap_or(0),
                        face: l.face,
                        turn: l.turn,
                        count: l.count,
                    };
                    return;
                }
                if l.line && l.back < l.count {
                    (o.x, o.y) = line_point(l.from, l.to, l.back);
                }
            }
            _ => {}
        }
        if let Some(s) = self.objects[k].ship.as_mut() {
            s.landing = Some(l);
        }
    }

    /// 0x4b2ff0: towards the cell `land` (`+0x740` for a settlement, the
    /// site for a fort) from its own, its direction by 0x59791d
    /// (`+0x76d`), the shore tile's group 0x4b3770 (none: it returns, the
    /// last close-in's `+0x76b`, `+0x748`, `+0x76a`, `+0x76c`, `+0x75e`
    /// kept);
    /// straight across, a line (0x4b5220) from where the hull is to its
    /// cell's middle moved 0x599c44 / 0x59a1c8 / 0x599ee8 / 0x59a4c8[type
    /// x 12 + group] towards the cell, and of the two side-on headings
    /// (2 or 6 across, 0 or 4 up and down) the one fewer turns away
    /// (0x4b5390, the second on a tie), a hull rotation a tick (frame
    /// `h x 3 + 0x20` first).
    fn close_in(&mut self, k: usize, land: (i32, i32)) -> Landing {
        let t = TILE_SIZE as f32;
        let o = &self.objects[k];
        let s = o.ship.as_ref().unwrap();
        let here = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let (dx, dy) = (land.0 - here.0, land.1 - here.1);
        // Not decoded: cells further off read 0x59791d out of its table
        // (none of the berths 0x4b2710 gives is).
        let dir = if dx.abs() <= 1 && dy.abs() <= 1 {
            NEIGHBOUR_DIR[(5 + dx * 4 + dy) as usize]
        } else {
            0xff
        };
        let group = trade::shore_group(&self.grid, land.0, land.1).map(|g| g as u8);
        let kept = s.close_in;
        let mut l = Landing {
            state: 2,
            abort: false,
            dir: kept.dir,
            group: Some(kept.group),
            walk_dir: dir,
            line: false,
            from: (o.x, o.y),
            to: (o.x, o.y),
            count: kept.count,
            at: 0,
            back: 0,
            face: kept.face,
            turn: kept.turn,
            furl: -1,
            hull_ticks: 0,
            plank: None,
            porters: None,
            plank_at: (0.0, 0.0),
            clock: 0.0,
        };
        // No shore group (0x4b3770 < 0): it returns before the line,
        // keeping the direction, group, heading, way and point count of
        // the last close-in; that line was freed as its landing ended.
        let Some(g) = group else {
            return l;
        };
        l.dir = dir;
        l.group = Some(g);
        if dir == 0xff || dir % 2 == 1 {
            // Slanting: the line freed and no turn (`+0x76c` 0); the
            // count and heading stay.
            l.turn = 0;
            return l;
        }
        let kind = s.kind as usize % 12;
        let d = trade::APPROACH[dir as usize / 2][kind][g as usize] as f32;
        let (ux, uy) = walk_path::DIRS[dir as usize];
        let mid = (here.0 as f32 * t + t / 2.0, here.1 as f32 * t + t / 2.0);
        l.to = (mid.0 + ux as f32 * d, mid.1 + uy as f32 * d);
        let major = (l.to.0 - l.from.0)
            .abs()
            .max((l.to.1 - l.from.1).abs())
            .round() as i32;
        l.count = (major + 1 + 1) / 2 * 2;
        l.line = true;
        let (a, b) = if dir % 4 == 0 { (2u8, 6u8) } else { (0, 4) };
        let s = self.objects[k].ship.as_mut().unwrap();
        let way = |s: &objects::Ship, c: u8| -> i32 {
            match s.rotation {
                Some(r) => {
                    let u = c as i32 * 3 - r as i32;
                    if u.abs() > 12 {
                        u - 24 * u.signum()
                    } else {
                        u
                    }
                }
                None => {
                    let u = c as i32 - s.heading as i32;
                    if u.abs() > 4 {
                        u - 8 * u.signum()
                    } else {
                        u
                    }
                }
            }
        };
        // 0x4b5390 puts a hull past its rotations (lying, furled) on its
        // heading's first frame.
        if s.rotation.is_none() {
            s.furl = 0.0;
        }
        let (ua, ub) = (way(s, a), way(s, b));
        let (face, u) = if ua.abs() < ub.abs() {
            (a, ua)
        } else {
            (b, ub)
        };
        l.face = face;
        l.turn = u.signum() as i8;
        if l.turn != 0 && s.rotation.is_none() {
            s.rotation = Some(s.heading as u8 * 3);
        }
        l
    }

    /// 0x4b1a90, a tick: a point along the line (two back once called
    /// off) and a hull rotation the way it turns (called off, the turn
    /// stops on its rotation's heading); true once at the line's end and
    /// turned, or back at its start.
    fn close_in_step(&mut self, k: usize, l: &mut Landing) -> bool {
        if l.abort {
            l.at = if l.at > 1 { l.at - 2 } else { 0 };
        } else {
            l.at += 1;
        }
        let o = &mut self.objects[k];
        let s = o.ship.as_mut().unwrap();
        if l.turn != 0 {
            let r = s.rotation.map_or(s.heading as i32 * 3, i32::from);
            if l.abort {
                l.turn = 0;
                s.heading = (r / 3) as usize;
            } else {
                let r = (r + l.turn as i32).rem_euclid(24);
                s.rotation = Some(r as u8);
                s.heading = (r / 3) as usize;
                if s.heading == l.face as usize && r % 3 == 0 {
                    l.turn = 0;
                }
            }
        }
        let mut done = false;
        if l.at < l.count {
            if l.turn < 1 && l.at == 0 {
                done = true;
            }
            if l.line {
                (o.x, o.y) = line_point(l.from, l.to, l.at);
            }
        } else {
            l.at = l.count - 1;
            if l.turn == 0 {
                return true;
            }
        }
        done
    }

    /// The landing party's creation (0x4b2d60 -> message 0x1a67, message
    /// 2 of 0x4221c0): at `at`, for the site `site`. On the site's cell
    /// already it founds at once (0x421ea0, 0x421f00); else a route over
    /// the quarter-cell grid of the site's land region (0x42e590, 0x42e720
    /// with 4), the 4 x 4 quarters of its own cell and of the site's
    /// opened, from its quarter to the goal's (0x55ca10, heading -1): none
    /// and it goes (message 10); each entry the quarter's centre, and the
    /// first leg.
    pub(super) fn make_party(
        &mut self,
        owner: (u32, u32),
        class: u32,
        site: (i32, i32),
        at: (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        let mut p = Party {
            owner,
            class,
            site,
            point: at,
            route: Vec::new(),
            entry: 0,
            from: at,
            to: at,
            dir: 0,
            frame: 0,
            walking: false,
            started: false,
            arrived: false,
        };
        if self.party_on_site(&mut p) {
            self.found_by_party(owner, class, site, data, caches);
            return;
        }
        // The goal point: a settlement's 2 x 2 middle, else the cell's.
        let goal = if class == SETTLEMENT {
            ((site.0 + 1) * 64, (site.1 + 1) * 64)
        } else {
            (site.0 * 64 + 32, site.1 * 64 + 32)
        };
        let (ox, oy, bw, bh) = self.region_box(site);
        let (w, h) = (bw * 4, bh * 4);
        let mut m = vec![0i16; (w * h) as usize];
        for ry in 0..bh {
            for rx in 0..bw {
                let q = self.landing_quarters((ox + rx, oy + ry));
                for (n, &v) in q.iter().enumerate() {
                    let (x, y) = (rx * 4 + n as i32 % 4, ry * 4 + n as i32 / 4);
                    m[(y * w + x) as usize] = v;
                }
            }
        }
        // The two cells opened (indices as written: off the grid they
        // land elsewhere in it, or nowhere).
        let mut open = |(cx, cy): (i32, i32)| {
            for q in 0..16 {
                let i = q % 4 + (cx - ox) * 4 + (q / 4 + (cy - oy) * 4) * w;
                if let Some(c) = usize::try_from(i).ok().and_then(|i| m.get_mut(i)) {
                    *c = 0;
                }
            }
        };
        open((tdiv(at.0, 64), tdiv(at.1, 64)));
        open(site);
        let s = (tdiv(at.0 - ox * 64, 16), tdiv(at.1 - oy * 64, 16));
        let g = (tdiv(goal.0 - ox * 64, 16), tdiv(goal.1 - oy * 64, 16));
        let Some(route) = walk_path::route_exact(&mut m, w, h, s, g, -1) else {
            return;
        };
        p.route = route
            .into_iter()
            .map(|(x, y, d)| (x * 16 + 8 + ox * 64, y * 16 + 8 + oy * 64, d))
            .collect();
        if self.next_leg(&mut p, data, caches) {
            return;
        }
        if !p.arrived {
            p.frame = 0;
            p.started = false;
        }
        self.parties.push(p);
    }

    /// 0x421ea0: on the site's cell (`+0xa26`).
    fn party_on_site(&self, p: &mut Party) -> bool {
        p.arrived = (tdiv(p.point.0, 64), tdiv(p.point.1, 64)) == p.site;
        p.arrived
    }

    /// 0x422140: off the site's cell and short of the last entry, the leg
    /// to the next; else the founding (0x421f00) and the party goes
    /// (message 10): true.
    fn next_leg(&mut self, p: &mut Party, data: &GameData, caches: &mut Caches) -> bool {
        if !self.party_on_site(p) && p.entry + 1 != p.route.len() {
            p.entry += 1;
            p.from = p.point;
            let (x, y, d) = p.route[p.entry];
            p.to = (x, y);
            p.dir = d;
            p.walking = true;
            return false;
        }
        self.found_by_party(p.owner, p.class, p.site, data, caches);
        true
    }

    /// Party `k`'s tick (message 0): on its cell nothing; else a frame of
    /// `LNDSPR_<dir>` (delay 0: one a tick), its point from the leg's
    /// start towards its end by `frame / frames` (the end at frame 0);
    /// past the last frame the leg is over and the next begins (0x422140,
    /// then 0x421bb0: frame 0). True once it has gone.
    fn party_tick(&mut self, k: usize, data: &GameData, caches: &mut Caches) -> bool {
        let mut p = self.parties[k].clone();
        if p.route.is_empty() || p.arrived {
            return false;
        }
        let n = self.landing_art[p.dir as usize % 8][2].len() as i32;
        p.started = true;
        p.frame += 1;
        if n <= p.frame {
            p.walking = false;
            p.frame = 0;
        }
        let lerp = |a: i32, b: i32| {
            if p.frame == 0 {
                b
            } else {
                (b - a) * p.frame / n + a
            }
        };
        p.point = (lerp(p.from.0, p.to.0), lerp(p.from.1, p.to.1));
        if !p.walking {
            if self.next_leg(&mut p, data, caches) {
                return true;
            }
            if !p.arrived {
                p.frame = 0;
                p.started = false;
            }
        }
        self.parties[k] = p;
        false
    }

    /// A cell's 4 x 4 quarters on the landing party's grid (0x42e720 with
    /// 4), -1 shut: not land, all; else each shore edge its half (north
    /// rows 0-1, east columns 2-3, south rows 2-3, west columns 0-1); with
    /// nibble 5 at 1 or layers 6-7 set, all shut but the outer ring's
    /// quarters on the edges without a shore. (0x42e720's further test,
    /// a layer 6 or 7 of type 4 for which 0x42e6e0 finds no tile set of
    /// type 4, cannot hold on the port's types, which are those records'
    /// type bytes.)
    fn landing_quarters(&self, (x, y): (i32, i32)) -> [i16; 16] {
        let w = self.grid.width as i32;
        let l = self.cell_types[(y * w + x) as usize];
        if l & 0xf != 8 {
            return [-1; 16];
        }
        let e = self.tile_edges(x, y);
        let mut q = [0i16; 16];
        for (n, c) in q.iter_mut().enumerate() {
            let (col, row) = (n % 4, n / 4);
            let shut = (e & 1 != 0 && row < 2)
                || (e & 2 != 0 && col >= 2)
                || (e & 4 != 0 && row >= 2)
                || (e & 8 != 0 && col < 2);
            if shut {
                *c = -1;
            }
        }
        if (l >> 20) & 0xf == 1 || l >> 24 != 0 {
            q = [-1; 16];
            for j in 0..4 {
                if e & 1 == 0 {
                    q[j] = 0;
                }
                if e & 2 == 0 {
                    q[3 + j * 4] = 0;
                }
                if e & 4 == 0 {
                    q[12 + j] = 0;
                }
                if e & 8 == 0 {
                    q[j * 4] = 0;
                }
            }
        }
        q
    }

    /// The founding ships' walkways and party pictures to draw (behind
    /// the hulls: 0x4e0470 and 0x4e04a0 draw them at the hull's order
    /// less 0x46 and 0x47): frame and the point drawn about.
    pub fn landings_shown(&self) -> Vec<(&[objects::Layer], f32, f32)> {
        let mut out = Vec::new();
        for o in self.objects.iter().filter(|o| self.is_visible(o)) {
            let Some(l) = o.ship.as_ref().and_then(|s| s.landing.as_ref()) else {
                continue;
            };
            // The walkway `AMOST_<+0x76d>`, the party `ANOSIL_<+0x76b>`
            // (0x4e1bb0).
            let (walk, party) = (l.walk_dir as usize % 8, l.dir as usize % 8);
            for (frames, f) in [
                (&self.landing_art[walk][0], l.plank),
                (&self.landing_art[party][1], l.porters),
            ] {
                let Some(layers) = f
                    .and_then(|f| usize::try_from(f).ok())
                    .and_then(|f| frames.get(f))
                else {
                    continue;
                };
                if let Some(img) = layers.first() {
                    let (w, h) = (img.image.width as f32, img.image.height as f32);
                    out.push((
                        layers.as_slice(),
                        l.plank_at.0 + w / 2.0 - img.dx as f32,
                        l.plank_at.1 + h / 2.0 - img.dy as f32,
                    ));
                }
            }
        }
        out
    }

    /// The landing parties to draw: `LNDSPR_<dir>` at its point less half
    /// the picture's width and its height (0x421d00).
    pub fn parties_shown(&self) -> Vec<(&[objects::Layer], f32, f32)> {
        let mut out = Vec::new();
        for p in &self.parties {
            if self.seen(tdiv(p.point.0, 64), tdiv(p.point.1, 64)) != sight::Seen::InSight {
                continue;
            }
            let frames = &self.landing_art[p.dir as usize % 8][2];
            let Some(layers) = usize::try_from(p.frame).ok().and_then(|f| frames.get(f)) else {
                continue;
            };
            if let Some(img) = layers.first() {
                let h = img.image.height as f32;
                out.push((
                    layers.as_slice(),
                    p.point.0 as f32 - img.dx as f32,
                    p.point.1 as f32 - h / 2.0 - img.dy as f32,
                ));
            }
        }
        out
    }
}

/// Point `k` of a close-in line from `from` to `to` (0x4b5220: as many
/// points as its longer side, padded to an even count; past the end, the
/// end).
fn line_point(from: (f32, f32), to: (f32, f32), k: i32) -> (f32, f32) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let major = dx.abs().max(dy.abs()).round() as i32;
    if major == 0 {
        return to;
    }
    let f = k.clamp(0, major) as f32 / major as f32;
    (from.0 + dx * f, from.1 + dy * f)
}

/// The walkway, the party going down it and the party ashore, by
/// direction (`AMOST_<d>`, `ANOSIL_<d>`, `LNDSPR_<d>` shaded).
pub(super) fn landing_art(data: &GameData, caches: &mut Caches) -> [[Frames; 3]; 8] {
    std::array::from_fn(|d| {
        [
            objects::sequence_frames(data, caches, &format!("AMOST_{d}")).unwrap_or_default(),
            objects::sequence_frames(data, caches, &format!("ANOSIL_{d}")).unwrap_or_default(),
            caches
                .shaded_frames(data, &format!("LNDSPR_{d}"), PARTY_SHADE)
                .unwrap_or_default(),
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> Option<(GameData, Caches)> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        Some((
            GameData::open(std::path::Path::new(&root)).unwrap(),
            Caches::default(),
        ))
    }

    /// miss000's own boat sent to found `class` on the first site the
    /// ground takes, from the map's end.
    type Founding = (GameData, Caches, Scene, u32, (i32, i32));

    fn founding(class: u32) -> Option<Founding> {
        let (data, mut caches) = open()?;
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let boat = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
            .unwrap()
            .id;
        scene.objects.retain(|o| o.ship.is_none() || o.id == boat);
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let i = scene.object_index(boat).unwrap();
        let site = (0..h)
            .rev()
            .flat_map(|y| (0..w).rev().map(move |x| (x, y)))
            .find(|&(x, y)| {
                scene.check_ground(class, x, y).is_ok()
                    && scene.found_berth(i, class, (x, y)).is_some()
            })
            .unwrap();
        scene.order_found(boat, site.0, site.1, class).unwrap();
        Some((data, caches, scene, boat, site))
    }

    /// A tick's look at the founding: the ship's landing state (0 none),
    /// its order still on, the parties, the buildings going up and the
    /// player's amber.
    type Look = (u8, bool, usize, usize, u32);

    /// Runs `ticks` game ticks, a look after each (until the boat goes).
    fn run(
        data: &GameData,
        caches: &mut Caches,
        scene: &mut Scene,
        boat: u32,
        ticks: usize,
        mut each: impl FnMut(&mut Scene, usize),
    ) -> Vec<Look> {
        let mut out = Vec::new();
        for n in 0..ticks {
            scene.update(crate::clock::TICK, data, caches);
            each(scene, n);
            let Some(i) = scene.object_index(boat) else {
                break;
            };
            let s = scene.objects[i].ship.as_ref().unwrap();
            out.push((
                s.landing.as_ref().map_or(0, |l| l.state),
                s.task.is_some(),
                scene.parties.len(),
                scene.constructions.len(),
                scene.amber,
            ));
        }
        out
    }

    /// How many ticks in a row from the first in `state`.
    fn span(looks: &[Look], state: u8) -> (usize, usize) {
        let first = looks.iter().position(|l| l.0 == state).unwrap();
        let n = looks[first..].iter().take_while(|l| l.0 == state).count();
        (first, n)
    }

    /// A founding ship's landing as 0x4b3370 runs it: the close-in a point
    /// of its line a tick (the line's even count), the furling's four hull
    /// steps (every 3 ticks, the first at once after the close-in), the
    /// walkway a frame a tick (`AMOST_` 12 frames: 12 ticks), the party
    /// going down it to frame 0x17 (23 ticks), the party made and, for a
    /// fort it lands on, the fort at once, paid only then; the walkway up
    /// (11 ticks), backing out two points every 3 ticks, then the order
    /// done. Nothing said (needs `OPENCONQUEST_DATA`).
    #[test]
    fn founding_ship_lands_a_fort() {
        let Some((data, mut caches, mut scene, boat, site)) = founding(0x34) else {
            return;
        };
        scene.amber = 5000;
        let looks = run(&data, &mut caches, &mut scene, boat, 3000, |_, _| {});
        let (start, close) = span(&looks, 2);
        let i = scene.object_index(boat).unwrap();
        assert_eq!(scene.objects[i].ship.as_ref().unwrap().landing, None);
        // The close-in to the shore south of it: 21 pixels (0x59a4c8 for
        // a boat on group 0), 22 points.
        assert_eq!(close, 22);
        assert_eq!(span(&looks, 8), (start + 22, 10));
        assert_eq!(span(&looks, 4), (start + 32, 1));
        assert_eq!(span(&looks, 5), (start + 33, 12));
        assert_eq!(span(&looks, 6), (start + 45, 23));
        assert_eq!(span(&looks, 7), (start + 68, 11));
        let (back, n) = span(&looks, 3);
        assert_eq!((back, n), (start + 79, 31));
        // Paid and up as the party is made (its start on the site), the
        // tick the walkway starts up.
        let made = looks.iter().position(|l| l.3 == 1).unwrap();
        assert_eq!(made, start + 68);
        assert_eq!(looks[made - 1].4, 5000);
        assert_eq!(looks[made].4, 5000 - economy::building_cost(0x34));
        assert!(!looks[back + n].1, "the order done");
        assert!(scene.messages.is_empty(), "{:?}", scene.messages);
        let k = scene
            .objects
            .iter()
            .position(|o| o.class == ObjectClass::Building(0x34) && o.cell == Some(site))
            .unwrap();
        assert_eq!(scene.objects[k].record.fields[8], 0xff);
        assert!(scene.objects[k].fort_places().iter().all(|p| p.is_empty()));
    }

    /// A settlement's party walks its quarter-cell route (0x55ca10), a leg
    /// a `LNDSPR_` run of 8 frames, a frame a tick, and founds on reaching
    /// the site's cell (0x421ea0, 0x421f00) (needs `OPENCONQUEST_DATA`).
    #[test]
    fn landing_party_walks_to_its_settlement() {
        let Some((data, mut caches, mut scene, boat, site)) = founding(SETTLEMENT) else {
            return;
        };
        scene.amber = 1000;
        let mut walked = Vec::new();
        let looks = run(&data, &mut caches, &mut scene, boat, 3000, |s, n| {
            if let Some(p) = s.parties.first() {
                walked.push((n, p.entry, p.frame, p.point));
            }
        });
        let made = looks.iter().position(|l| l.2 == 1).unwrap();
        let (six, n) = span(&looks, 6);
        assert_eq!(made, six + n);
        // Each leg 8 ticks, its point by `frame / 8` of the way.
        let p = &walked;
        assert!(p.len() >= 8);
        for w in p.windows(2) {
            if w[1].1 == w[0].1 {
                assert_eq!(w[1].2, w[0].2 + 1);
            }
        }
        let founded = looks.iter().position(|l| l.3 == 1).unwrap();
        assert_eq!(founded, made + p.len());
        assert_eq!(looks[founded].4, 1000 - economy::building_cost(SETTLEMENT));
        assert!(scene
            .objects
            .iter()
            .any(|o| o.class == ObjectClass::Building(SETTLEMENT) && o.cell == Some(site)));
        let last = p.last().unwrap().3;
        assert_eq!(
            (last.0 / 64, last.1 / 64),
            site,
            "founds on the site's cell"
        );
        assert!(scene.messages.is_empty(), "{:?}", scene.messages);
    }

    /// Short of the price the party builds nothing at the site and says
    /// nothing (0x421f00); the amber is untouched (needs
    /// `OPENCONQUEST_DATA`).
    #[test]
    fn landing_party_without_the_price_builds_nothing() {
        let Some((data, mut caches, mut scene, boat, _)) = founding(0x34) else {
            return;
        };
        scene.amber = economy::building_cost(0x34) - 1;
        let looks = run(&data, &mut caches, &mut scene, boat, 3000, |_, _| {});
        assert!(looks.iter().any(|l| l.0 == 6));
        assert!(looks.iter().all(|l| l.3 == 0 && l.2 == 0));
        assert_eq!(scene.amber, economy::building_cost(0x34) - 1);
        assert!(scene.messages.is_empty(), "{:?}", scene.messages);
        assert!(!looks.last().unwrap().1, "the order done");
    }

    /// A landing leaves its close-in's direction, group, heading, way and
    /// point count on the ship; a close-in to a cell with no shore group
    /// (0x4b3770 < 0) keeps them, only `+0x76d` its own, and with the line
    /// freed (0x4b1d40) the hull stays put for the kept count of points
    /// (0x4b1a90) (needs `OPENCONQUEST_DATA`).
    #[test]
    fn close_in_without_a_shore_group_keeps_the_last() {
        let Some((data, mut caches, mut scene, boat, _)) = founding(0x34) else {
            return;
        };
        scene.amber = 5000;
        run(&data, &mut caches, &mut scene, boat, 3000, |_, _| {});
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_ref().unwrap();
        assert_eq!(s.landing, None);
        // The fort's landing: south (6) onto group 0, 22 points.
        let kept = s.close_in;
        assert_eq!((kept.dir, kept.group, kept.count), (6, 0, 22));
        let t = TILE_SIZE as f32;
        let o = &scene.objects[i];
        let here = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let at = (o.x, o.y);
        let (dx, dy) = (-1..=1)
            .flat_map(|dx| (-1..=1).map(move |dy| (dx, dy)))
            .find(|&(dx, dy)| {
                (dx, dy) != (0, 0)
                    && trade::shore_group(&scene.grid, here.0 + dx, here.1 + dy).is_none()
            })
            .unwrap();
        let mut l = scene.close_in(i, (here.0 + dx, here.1 + dy));
        assert_eq!(l.walk_dir, NEIGHBOUR_DIR[(5 + dx * 4 + dy) as usize]);
        assert_eq!(
            (l.dir, l.group, l.face, l.turn),
            (6, Some(0), kept.face, kept.turn)
        );
        assert_eq!((l.count, l.line), (22, false));
        let steps = (1..).find(|_| scene.close_in_step(i, &mut l)).unwrap();
        assert_eq!(steps, 22);
        let o = &scene.objects[i];
        assert_eq!((o.x, o.y), at);
    }

    /// A stop as the party goes down the walkway calls the landing off
    /// (0x4abc60, `+0x73a`): the party's picture goes, the walkway comes
    /// up, the ship backs out and no party lands; other orders are held
    /// meanwhile (needs `OPENCONQUEST_DATA`).
    #[test]
    fn stopped_landing_backs_out() {
        let Some((data, mut caches, mut scene, boat, _)) = founding(0x34) else {
            return;
        };
        scene.amber = 5000;
        let mut stopped = None;
        let looks = run(&data, &mut caches, &mut scene, boat, 3000, |s, n| {
            let i = s.object_index(boat).unwrap();
            let l = s.objects[i].ship.as_ref().unwrap().landing.clone();
            if stopped.is_none() && l.is_some_and(|l| l.state == 6) {
                assert!(s.landing_holds(boat));
                s.stop(boat);
                stopped = Some(n);
            }
        });
        let at = stopped.unwrap();
        assert_eq!(looks[at + 1].0, 7);
        assert!(looks.iter().all(|l| l.2 == 0 && l.3 == 0));
        assert!(looks.iter().any(|l| l.0 == 3));
        assert!(!looks.last().unwrap().1);
        assert_eq!(scene.amber, 5000);
    }

    /// The pictures (`AMOST_`, `ANOSIL_`, `LNDSPR_`) the landing shows.
    #[test]
    fn landing_pictures() {
        let Some((data, mut caches)) = open() else {
            return;
        };
        let art = landing_art(&data, &mut caches);
        let lens: Vec<_> = art
            .iter()
            .map(|a| (a[0].len(), a[1].len(), a[2].len()))
            .collect();
        assert_eq!(lens[0], (12, 49, 8));
        assert_eq!(lens[2], (10, 46, 8));
        assert_eq!(lens[4], (12, 49, 8));
        assert_eq!(lens[6], (12, 49, 8));
        assert!(lens.iter().all(|l| l.2 == 8));
    }
}
