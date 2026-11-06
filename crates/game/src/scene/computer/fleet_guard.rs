//! The navy's guard task (fleet order 0x6e, block `{0x6e, 0x29}`, made by
//! 0x4d1ce0, its notices 0x4d1ea0, its tick 0x4d3b30) for the computers'
//! flotillas (0x483570, mode 3).

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;
use serde::{Deserialize, Serialize};

use super::{Cells, Computer};
use crate::formation;
use crate::scene::{Order, Scene};

/// The guard block `{0x6e, 0x29}` (group `+0x25`, 0x65 bytes). The lists
/// are `None` while null (made at their first entry, freed by a rewrite).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct GuardBlock {
    /// `+8` made (the game's tick).
    pub made: u32,
    /// `+0xc..+0x18` the zone (args 0-3: the flotilla's home `+0x178`).
    pub rect: Cells,
    /// `+0x1c` the mask (args 8: W x H shorts, -1 closed), 0 for the
    /// computers'.
    pub mask: Option<Vec<i16>>,
    /// `+0x20` (args 9, flotilla `+0x154`): low word the picker flags,
    /// high word the chance level.
    pub flags: u32,
    /// `+0x24` the mode (args 10).
    pub mode: u16,
    /// `+0x29` the patrol's corner (0-3).
    pub corner: u8,
    /// `+0x2b` the pull-back flag (0x4d3080).
    pub pull_back: bool,
    /// `+0x2c` the last engagement's tick (0x4d18d0).
    pub engaged_at: u32,
    /// `+0x30..+0x3c` the second rect (args 4-7: the flotilla's zone
    /// `+0x168`).
    pub rect2: Cells,
    /// `+0x40` the spots (8 bytes `{x, y}`, 0x4d1540).
    pub spots: Option<Vec<(i32, i32)>>,
    /// `+0x44` the posts (kind 1, 0x14 bytes, 0x4d15a0).
    pub posts: Option<Vec<Post>>,
    /// `+0x48` the engaged (kind 2, 0x18 bytes, 0x4d18d0).
    pub engaged: Option<Vec<Engaged>>,
    /// `+0x4c` the patrol (kind 4, 0x14 bytes, 0x4d1670).
    pub patrol: Option<Vec<Post>>,
}

/// A post or patrol entry: `{ship*, ship id, x, y, tick}` (the port keeps
/// the id for both).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Post {
    pub ship: u32,
    pub x: i32,
    pub y: i32,
    pub tick: u32,
}

/// An engaged entry: `{ship*, ship id, target*, target id, x, y}`; the
/// target 0 when cleared (0x4d1ea0's 0x13a / 0x13c / 0x13d).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Engaged {
    pub ship: u32,
    pub target: u32,
    pub x: i32,
    pub y: i32,
}

/// 0x557480: the cells between two, the greater of the two steps.
fn steps(a: (i32, i32), b: (i32, i32)) -> i32 {
    (a.0 - b.0).abs().max((a.1 - b.1).abs())
}

impl GuardBlock {
    /// 0x4d1750 mode 0: `kind << 16 | index` of ship `id`'s last entry in
    /// the posts (1), then the engaged (2), then the patrol (4), a later
    /// list's overriding; 0 when in none (or `id` 0).
    pub fn find(&self, id: u32) -> u32 {
        let mut r = 0;
        if id == 0 {
            return 0;
        }
        for (k, p) in self.posts.iter().flatten().enumerate() {
            if p.ship == id {
                r = 0x10000 | (k as u32 & 0xffff);
            }
        }
        for (k, e) in self.engaged.iter().flatten().enumerate() {
            if e.ship == id {
                r = 0x20000 | (k as u32 & 0xffff);
            }
        }
        for (k, p) in self.patrol.iter().flatten().enumerate() {
            if p.ship == id {
                r = 0x40000 | (k as u32 & 0xffff);
            }
        }
        r
    }

    /// 0x4d1750 mode 1: ship `id`'s first post and first engaged entry
    /// removed, and every patrol entry of it, the index stepping on past a
    /// removal (0x55b500 closes the gap, so the entry after a removed one
    /// is skipped); the first list removed from's `kind << 16 | index`,
    /// else 0.
    pub fn remove(&mut self, id: u32) -> u32 {
        if id == 0 {
            return 0;
        }
        let mut r = 0;
        if let Some(l) = self.posts.as_mut() {
            if let Some(k) = l.iter().position(|p| p.ship == id) {
                l.remove(k);
                r = 0x10000 | (k as u32 & 0xffff);
            }
        }
        if let Some(l) = self.engaged.as_mut() {
            if let Some(k) = l.iter().position(|e| e.ship == id) {
                l.remove(k);
                if r == 0 {
                    r = 0x20000 | (k as u32 & 0xffff);
                }
            }
        }
        if let Some(l) = self.patrol.as_mut() {
            let mut k = 0;
            while k < l.len() {
                if l[k].ship == id {
                    l.remove(k);
                    if r == 0 {
                        r = 0x40000 | (k as u32 & 0xffff);
                    }
                }
                k += 1;
            }
        }
        r
    }

    /// 0x4d15a0: ship `id`'s post at `(x, y)` (the list made if null): its
    /// entry updated (the tick kept), else appended with `now`. Its index.
    pub fn post(&mut self, id: u32, x: i32, y: i32, now: u32) -> usize {
        let l = self.posts.get_or_insert_with(Vec::new);
        if let Some(k) = l.iter().position(|p| p.ship == id) {
            l[k].x = x;
            l[k].y = y;
            return k;
        }
        l.push(Post {
            ship: id,
            x,
            y,
            tick: now,
        });
        l.len() - 1
    }

    /// Whether `(x, y)` lies in the zone, its low edges outside (0x4d33c0:
    /// `rx < x <= rx + w`, `ry < y <= ry + h`).
    fn holds(&self, (x, y): (i32, i32)) -> bool {
        let (rx, ry, w, h) = self.rect;
        rx < x && x <= rx + w && ry < y && y <= ry + h
    }
}

/// 0x561530 over `rect` with the mask `mask` (indexed `y x w + x` by the
/// absolute cell, the original's slip) on a `W` x `H` map, the cells where
/// `open` holds: rings out from the rect's middle, each the right column
/// up, the top row leftwards, the left column down and the bottom row
/// rightwards, each clamped to the rect (a clamped side skipped) until all
/// four clamp; then the same rings past the rect within the map, with no
/// mask. Up to `n` cells appended to `out`; the map run out: -4, else 0
/// when the rect gave `n`, else `n` less the rect's count.
pub(super) fn spiral(
    rect: Cells,
    mask: Option<&[i16]>,
    (mw, mh): (i32, i32),
    n: i32,
    open: impl Fn(i32, i32) -> bool,
    out: &mut Vec<(i32, i32)>,
) -> i32 {
    let (x, y, w, h) = rect;
    let (rmax, bmax) = (x + w - 1, y + h - 1);
    let (mut l, mut t) = ((w - 1) / 2 + x, (h - 1) / 2 + y);
    let (mut r, mut b) = (w / 2 + x, h / 2 + y);
    let mut found = 0;
    let masked = |cx: i32, cy: i32| {
        mask.is_none_or(|m| m.get((cy * w + cx) as usize).is_some_and(|&v| v != -1))
    };
    let take = |cx: i32, cy: i32, found: &mut i32, out: &mut Vec<(i32, i32)>| {
        out.push((cx, cy));
        *found += 1;
        *found >= n
    };
    loop {
        let mut flags = 0;
        if rmax < r {
            flags |= 1;
            r = rmax;
        }
        if t < y {
            flags |= 2;
            t = y;
        }
        if l < x {
            flags |= 4;
            l = x;
        }
        if bmax < b {
            flags |= 8;
            b = bmax;
        }
        if flags == 0xf {
            break;
        }
        let mut cy = b;
        if flags & 8 == 0 && (l != r || t != b) {
            cy = b - 1;
        }
        let mut cx = r;
        if flags & 1 == 0 {
            while t <= cy {
                if masked(r, cy) && open(r, cy) && take(r, cy, &mut found, out) {
                    return 0;
                }
                cy -= 1;
            }
            cx = r - 1;
        }
        let mut cy = t;
        if flags & 2 == 0 {
            while l <= cx {
                if masked(cx, t) && open(cx, t) && take(cx, t, &mut found, out) {
                    return 0;
                }
                cx -= 1;
            }
            cy = t + 1;
        }
        let mut cx = l;
        if flags & 4 == 0 {
            while cy <= b {
                if masked(l, cy) && open(l, cy) && take(l, cy, &mut found, out) {
                    return 0;
                }
                cy += 1;
            }
            cx = l + 1;
        }
        if flags & 8 == 0 {
            while cx <= r {
                if masked(cx, b) && open(cx, b) && take(cx, b, &mut found, out) {
                    return 0;
                }
                cx += 1;
            }
        }
        l -= 1;
        t -= 1;
        r += 1;
        b += 1;
    }
    let left = n - found;
    loop {
        l -= 1;
        b += 1;
        t -= 1;
        r += 1;
        let mut flags = 0;
        if mw <= r {
            flags |= 1;
            r = mw - 1;
        }
        if t < 0 {
            flags |= 2;
            t = 0;
        }
        if l < 0 {
            flags |= 4;
            l = 0;
        }
        if mh <= b {
            flags |= 8;
            b = mh - 1;
        }
        if flags == 0xf {
            break;
        }
        let mut cx = r;
        if flags & 1 == 0 {
            let mut cy = b;
            while t <= cy {
                if open(r, cy) && take(r, cy, &mut found, out) {
                    return left;
                }
                cy -= 1;
            }
            cx = r - 1;
        }
        let mut cy = t;
        if flags & 2 == 0 {
            while l <= cx {
                if open(cx, t) && take(cx, t, &mut found, out) {
                    return left;
                }
                cx -= 1;
            }
            cy = t + 1;
        }
        let mut cx = l;
        if flags & 4 == 0 {
            while cy <= b {
                if open(l, cy) && take(l, cy, &mut found, out) {
                    return left;
                }
                cy += 1;
            }
            cx = l + 1;
        }
        if flags & 8 == 0 {
            while cx <= r {
                if open(cx, b) && take(cx, b, &mut found, out) {
                    return left;
                }
                cx += 1;
            }
        }
    }
    -4
}

impl Scene {
    /// Fleet order 0x6e from a computer's flotilla (0x483570 mode 3, at
    /// 0x47e48d, 0x47e88d, 0x47ecbd, 0x47ef37): the args the flotilla's
    /// home `+0x178` (the zone), its zone `+0x168` (the second rect), no
    /// mask, `+0x154` = (AIPlayer `+0x20` == 0) << 16, task 7's (0x47ef29)
    /// also `| 1`; to 0x4d1ce0. The maker ends the old block only when
    /// both its kinds differ from `{0x6e, 0x29}` (an `&&` where `||` was
    /// meant; the port's flotillas hold no other 0x6e or 0x29 block, so a
    /// guard block is rewritten: 0x4d0c90 with 1 frees the mask and the
    /// lists, the notices, `+0x29`, `+0x2b` and `+0x2c` kept), else makes
    /// it (0x4d1bc0, `new_block`). The fields, then by the mode: 1 and 2
    /// the formation into the zone (0x4d1080), 3 into the second rect
    /// when its w and h are both set, else each ship posted at its cell
    /// (0x4d1450); 0 nothing.
    pub(super) fn guard_order(&mut self, ai: &mut Computer, f: usize) {
        let now = self.now();
        let fl = &mut ai.flotillas[f];
        let flags = u32::from(ai.difficulty == 0) << 16 | u32::from(fl.task == 7);
        let (rect, rect2) = (fl.home, fl.zone);
        let b = fl.guard.get_or_insert_with(Default::default);
        b.spots = None;
        b.posts = None;
        b.engaged = None;
        b.patrol = None;
        b.made = now;
        b.rect = rect;
        b.mask = None;
        b.flags = flags;
        b.mode = 3;
        b.rect2 = rect2;
        match b.mode {
            1 | 2 => {
                self.guard_formation(ai, f, rect, None);
            }
            3 if rect2.2 != 0 && rect2.3 != 0 => {
                self.guard_formation(ai, f, rect2, None);
            }
            3 => self.guard_post_here(ai, f),
            _ => {}
        }
    }

    /// 0x4cf070: command 0x14 `{x, y, 0}`, a plain move to the cell,
    /// through the outbox.
    fn guard_move(&mut self, ship: u32, (x, y): (i32, i32)) {
        let t = TILE_SIZE as f32;
        self.post(Order::Move {
            ship,
            x: x as f32 * t + t / 2.0,
            y: y as f32 * t + t / 2.0,
        });
    }

    /// 0x4d19b0 kind 1: the ship posted at `cell` (0x4d15a0) and sent
    /// there (0x4cf070). Whether listed (always, the block being there).
    fn guard_post_go(&mut self, ai: &mut Computer, f: usize, ship: u32, cell: (i32, i32)) -> bool {
        let now = self.now();
        let Some(b) = ai.flotillas[f].guard.as_mut() else {
            return false;
        };
        b.post(ship, cell.0, cell.1, now);
        self.guard_move(ship, cell);
        true
    }

    /// 0x4d1450: each ship of the group (class 200) posted at its own cell
    /// (0x4d15a0), not moved.
    fn guard_post_here(&mut self, ai: &mut Computer, f: usize) {
        let now = self.now();
        for id in ai.flotillas[f].ships.clone() {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            if self.objects[i].ship.is_none() {
                continue;
            }
            let (x, y) = self.ship_cell(i);
            if let Some(b) = ai.flotillas[f].guard.as_mut() {
                b.post(id, x, y, now);
            }
        }
    }

    /// 0x4d1540: n = the group's ship count; the spots list made if null
    /// (never cleared) and up to n spots appended by 0x561530 over the zone
    /// with the mask, the cells water (0x4d1520: 0x4999e0(x, y, 0)).
    /// Returns n.
    fn guard_spots(&mut self, ai: &mut Computer, f: usize) -> i32 {
        let n = ai.flotillas[f].ships.len() as i32;
        let dims = (self.nav.width as i32, self.nav.height as i32);
        let Some(b) = ai.flotillas[f].guard.as_mut() else {
            return n;
        };
        let mut spots = b.spots.take().unwrap_or_default();
        let nav = &self.nav;
        spiral(
            b.rect,
            b.mask.as_deref(),
            dims,
            n,
            |x, y| nav.is_sea(x, y),
            &mut spots,
        );
        b.spots = Some(spots);
        n
    }

    /// 0x4d1a50(ship, force): unless `force`, only a ship in no list
    /// (0x4d1750 mode 0 is 0); 0x4d1540 above 0; then with the posts null
    /// a spot drawn (navy LCG `>> 16` % the spots) and the ship posted
    /// there and sent (0x4d19b0 kind 1); else the spots in order, each not
    /// already a post's cell, the first that lists the ship. Whether one did.
    pub(super) fn guard_post_one(
        &mut self,
        ai: &mut Computer,
        f: usize,
        ship: u32,
        force: bool,
    ) -> bool {
        let Some(b) = ai.flotillas[f].guard.as_ref() else {
            return false;
        };
        if !force && b.find(ship) != 0 {
            return false;
        }
        if self.guard_spots(ai, f) <= 0 {
            return false;
        }
        let b = ai.flotillas[f].guard.as_ref().unwrap();
        let spots = b.spots.clone().unwrap_or_default();
        match &b.posts {
            None => {
                // (The original divides by the spots' count: none, a
                // fault; the port does nothing.)
                if spots.is_empty() {
                    return false;
                }
                let r = (self.navy_draw() as usize) % spots.len();
                self.guard_post_go(ai, f, ship, spots[r])
            }
            Some(posts) => {
                let posts = posts.clone();
                for s in spots {
                    if posts.iter().any(|p| (p.x, p.y) == s) {
                        continue;
                    }
                    if self.guard_post_go(ai, f, ship, s) {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// 0x4d1080(rect, ship): the formation into the posts. Copies of the
    /// grids 0x5c9398 (deep) and 0x5c93bc (shallow); every cell of the rect
    /// within the map whose object (`+0xc`) is of class 0x3a or 0x3b (a
    /// yard, a dock; also 0xb2, no class of the port's) closes the 5 x 5
    /// about it in both. The centre 0x4d0f40: the rect's middle when water
    /// (0x4999e0), else the water cell of the rect within the map nearest
    /// it (0x557440, strictly less, from w + h), none nothing. The group's
    /// ships (0x4d0d20: class 200, their cells `+0x5a3 / +0x5a7`, class 0,
    /// `+0xc` 1 with a deep draught `+0x592`) planned by 0x5614e0 on the
    /// copies (deep the first grid, shallow the second, so the deep ships
    /// go on the shallow grid). Planned (>= 0): with no ship given each
    /// posted and sent (0x4d19b0 kind 1); with `ship`, that one so, each
    /// other by its list (0x4d1750 mode 0): a post posted and sent again,
    /// an engaged entry's x, y the cell, a patrol's **the entry at the
    /// record's index** (not the one found) given the cell; one in no list
    /// left alone. Returns the count posted.
    /// (Not decoded: the planner's `Fail::Unsettled` cases, taken here as
    /// a failure; the original goes on with stack words never written.)
    pub(super) fn guard_formation(
        &mut self,
        ai: &mut Computer,
        f: usize,
        rect: Cells,
        ship: Option<u32>,
    ) -> i32 {
        let (mw, mh) = (self.nav.width as i32, self.nav.height as i32);
        let (rx, ry, rw, rh) = rect;
        let (x0, x1) = (rx.max(0), (rx + rw).min(mw));
        let (y0, y1) = (ry.max(0), (ry + rh).min(mh));
        let grid = |deep: bool| -> Vec<i16> {
            (0..mh)
                .flat_map(|y| (0..mw).map(move |x| (x, y)))
                .map(|(x, y)| if self.nav.passable(x, y, deep) { 0 } else { -1 })
                .collect()
        };
        let (mut deep, mut shallow) = (grid(true), grid(false));
        // The yards' and docks' cells (each cell of the footprint holds
        // the building, `cell_occupants`).
        for o in &self.objects {
            if !matches!(o.class, ObjectClass::Building(0x3a | 0x3b)) {
                continue;
            }
            let Some((bx, by)) = o.cell else {
                continue;
            };
            let n = crate::scene::trade::building_cells(o);
            for cy in by..by + n {
                for cx in bx..bx + n {
                    if cx < x0 || cx >= x1 || cy < y0 || cy >= y1 {
                        continue;
                    }
                    for yy in cy - 2..=cy + 2 {
                        for xx in cx - 2..=cx + 2 {
                            if yy >= 0 && yy < mh && xx >= 0 && xx < mw {
                                let k = (yy * mw + xx) as usize;
                                deep[k] = -1;
                                shallow[k] = -1;
                            }
                        }
                    }
                }
            }
        }
        let Some(centre) = self.guard_centre(rect) else {
            return 0;
        };
        let ships: Vec<(u32, (i32, i32), bool)> = ai.flotillas[f]
            .ships
            .iter()
            .filter_map(|&id| {
                let i = self.object_index(id)?;
                let kind = self.objects[i].ship.as_ref()?.kind;
                Some((id, self.ship_cell(i), crate::objects::deep_draught(kind)))
            })
            .collect();
        if ships.is_empty() {
            return 0;
        }
        let recs: Vec<formation::Rec> = ships
            .iter()
            .map(|s| formation::Rec {
                x: s.1 .0,
                y: s.1 .1,
                class: 0,
                second: s.2,
            })
            .collect();
        let Ok(planned) =
            formation::plan_records(Some(&deep), Some(&shallow), None, (mw, mh), centre, &recs)
        else {
            return 0;
        };
        if planned.value < 0 {
            return 0;
        }
        let mut moved = 0;
        for (k, (s, &cell)) in ships.iter().zip(&planned.cells).enumerate() {
            let Some(given) = ship else {
                moved += i32::from(self.guard_post_go(ai, f, s.0, cell));
                continue;
            };
            if s.0 == given {
                moved += i32::from(self.guard_post_go(ai, f, s.0, cell));
                continue;
            }
            let Some(b) = ai.flotillas[f].guard.as_mut() else {
                continue;
            };
            let found = b.find(s.0);
            match found >> 16 {
                1 => moved += i32::from(self.guard_post_go(ai, f, s.0, cell)),
                2 => {
                    let idx = (found & 0xffff) as usize;
                    if let Some(e) = b.engaged.as_mut().and_then(|l| l.get_mut(idx)) {
                        (e.x, e.y) = cell;
                    }
                }
                4 => {
                    if let Some(p) = b.patrol.as_mut().and_then(|l| l.get_mut(k)) {
                        (p.x, p.y) = cell;
                    }
                }
                _ => {}
            }
        }
        moved
    }

    /// 0x4d0f40: the rect's middle `(x + w/2, y + h/2)` when water
    /// (0x4999e0), else the rect's water cell within the map nearest it
    /// (0x557440, strictly less, from `w + h`), row by row; none: `None`.
    fn guard_centre(&self, (rx, ry, rw, rh): Cells) -> Option<(i32, i32)> {
        let mid = (rx + rw / 2, ry + rh / 2);
        if self.nav.is_sea(mid.0, mid.1) {
            return Some(mid);
        }
        let (mw, mh) = (self.nav.width as i32, self.nav.height as i32);
        let mut best = rw + rh;
        let mut found = None;
        for y in ry..ry + rh {
            if y < 0 || y >= mh {
                continue;
            }
            for x in rx..rx + rw {
                if x >= 0 && x < mw && self.nav.is_sea(x, y) {
                    let d = formation::cells_apart((x, y), mid);
                    if d < best {
                        best = d;
                        found = Some((x, y));
                    }
                }
            }
        }
        found
    }

    /// 0x4d3b30, the guard's tick (from the navy's 0x4c4f40): its notices
    /// heard (0x4d1ea0) when there are any; the period P, for a computer
    /// side (3 or 4) out of the campaign by its level `+0x1a31` 0x69, 0x4b,
    /// 0x3c, else 0x4b (the port's computers play only out of the
    /// campaign). None heard, group `+8` past P (unsigned), a block and
    /// ships: mode 1 the watch (0x4d33c0) and, it giving 0, the patrol
    /// rounds (0x4d2720), mode 2 or 3 the watch; then with the engaged
    /// list the boarding and ramming picks (0x4d3990); `+8` = 0. Then `+8`
    /// grows by the tick. (Not ported: part 2, 0x4d2720 and 0x4d3990.)
    pub(super) fn guard_pass(&mut self, ai: &mut Computer, f: usize) {
        let heard = if ai.flotillas[f].joined.is_empty() || ai.flotillas[f].no_notices {
            0
        } else {
            self.guard_notices(ai, f)
        };
        let period = [0x69, 0x4b, 0x3c][ai.difficulty.min(2)];
        if heard == 0
            && ai.flotillas[f].elapsed > period
            && ai.flotillas[f].guard.is_some()
            && !ai.flotillas[f].ships.is_empty()
        {
            match ai.flotillas[f].guard.as_ref().unwrap().mode {
                1 => {
                    // (Not ported: part 2, the patrol rounds 0x4d2720
                    // when the watch gives 0.)
                    self.guard_watch(ai, f);
                }
                2 | 3 => {
                    self.guard_watch(ai, f);
                }
                _ => {}
            }
            // (Not ported: part 2, 0x4d3990 with the engaged list.)
            ai.flotillas[f].elapsed = 0;
        }
        ai.flotillas[f].elapsed += 1;
    }

    /// 0x4d1ea0: the group's notices newest first, each removed; one whose
    /// ship (0x4cc590) is not in the group dropped uncounted but a 0x137;
    /// the others counted heard: 300 (a join) mode 1, 2, or 3 with no
    /// second rect (w or h 0) posted by 0x4d1a50, mode 3 with it the
    /// formation into it for the ship (0x4d1080), then `#` sub 1 with the
    /// group's three (0x4c42f0); 0x136 (at a goal) with the ship on the
    /// patrol and 0x4d2200, the next corner (not ported: part 2, and the
    /// port posts no 0x136); 0x137 (left) the ship out of every list
    /// (0x4d1750 mode 1), a patrol's restaffed by 0x4d25a0 (not ported:
    /// part 2); 0x13a / 0x13c / 0x13d (target gone, a boarding or ram
    /// ended) the ship's engaged entry, found by mode 0, its target zeroed.
    fn guard_notices(&mut self, ai: &mut Computer, f: usize) -> u32 {
        let block = ai.flotillas[f].block;
        let three = ai.flotillas[f].behaviour.unwrap_or((0, 0, 0));
        let mut heard = 0;
        for (id, at, code, _) in std::mem::take(&mut ai.flotillas[f].joined)
            .into_iter()
            .rev()
        {
            let in_group = ai.flotillas[f].ships.contains(&id) && self.object_index(id).is_some();
            if at != block || (!in_group && code != 0x137) {
                continue;
            }
            heard += 1;
            let Some(b) = ai.flotillas[f].guard.as_mut() else {
                continue;
            };
            match code {
                300 => {
                    let (mode, rect2) = (b.mode, b.rect2);
                    match mode {
                        1 | 2 => {
                            self.guard_post_one(ai, f, id, false);
                        }
                        3 if rect2.2 == 0 || rect2.3 == 0 => {
                            self.guard_post_one(ai, f, id, false);
                        }
                        3 => {
                            self.guard_formation(ai, f, rect2, Some(id));
                        }
                        _ => {}
                    }
                    self.send_behaviour(id, three);
                }
                0x137 => {
                    b.remove(id);
                }
                0x13a | 0x13c | 0x13d => {
                    let found = b.find(id);
                    if found >> 16 == 2 {
                        let idx = (found & 0xffff) as usize;
                        if let Some(e) = b.engaged.as_mut().and_then(|l| l.get_mut(idx)) {
                            e.target = 0;
                        }
                    }
                }
                _ => {}
            }
        }
        heard
    }

    /// 0x4d18d0: ship `ship` engaged on `target` at `(x, y)` (the list made
    /// if null, `+0x2c` = now): its entry updated, else appended and the
    /// attack sent (0x4d0140: command 0x16 `{target, 7, 7, -1}`).
    fn guard_engage(
        &mut self,
        ai: &mut Computer,
        f: usize,
        ship: u32,
        target: u32,
        x: i32,
        y: i32,
    ) {
        let now = self.now();
        let Some(b) = ai.flotillas[f].guard.as_mut() else {
            return;
        };
        b.engaged_at = now;
        let l = b.engaged.get_or_insert_with(Vec::new);
        if let Some(e) = l.iter_mut().find(|e| e.ship == ship) {
            *e = Engaged { ship, target, x, y };
            return;
        }
        l.push(Engaged { ship, target, x, y });
        self.post(Order::Attack { ship, target });
    }

    /// 0x4d33c0, the watch; returns the busy count. The patrol's entries,
    /// then the posts', in order: a ship hit (`+0x691`) within 3 of its
    /// entry (0x557480) or inside the zone (`GuardBlock::holds`) has its
    /// attacker (`+0x692`, 0x51f1f0) listed when it has a cell (the
    /// patrol's reuse the last attacker found when the lookup fails, the
    /// original's `local_c` never reset; the posts' need it found) and
    /// its blow forgotten (0x4cf760); lying still (`+0x58c` 0) more than 1
    /// off its entry (0x557440) sent back there (0x4cf070). Busy: 0x4d3080
    /// (not ported: part 2, taken as 0). The zone's targets (0x4cd300 with
    /// the group's players `+0x19`). No targets, not busy and the engaged
    /// list: its entries from the last, each ship out of every list
    /// (0x4d1750 mode 1), mode 1 or 2 posted again by 0x4d1a50 (forced),
    /// mode 3 posted at the entry's cell (0x4d15a0) and sent there. The sum
    /// of the hit patrol ships, hit posts and targets: 0 with `+0x2b`, posts
    /// and engaged entries, one engaged entry **drawn** (navy LCG) and with
    /// its target found every post from the last removed and engaged on it
    /// at its cell (0x4d18d0); above 0 the sharing out 0x4d2b00 (not
    /// ported: part 2; the port's former guard stands in: each idle post
    /// ship guards the flotilla's zone, `order_zone`).
    fn guard_watch(&mut self, ai: &mut Computer, f: usize) -> i32 {
        let Some(b) = ai.flotillas[f].guard.clone() else {
            return 0;
        };
        let mut last: Option<u32> = None;
        let mut hit_patrol = 0;
        let mut hit_posts = 0;
        for (list, patrol) in [(&b.patrol, true), (&b.posts, false)] {
            for e in list.iter().flatten() {
                let Some(i) = self.object_index(e.ship) else {
                    continue;
                };
                let Some(s) = self.objects[i].ship.as_ref() else {
                    continue;
                };
                let (blow, still) = (s.blow, s.move_state == 0);
                let cell = self.ship_cell(i);
                if let Some(by) = blow {
                    if steps(cell, (e.x, e.y)) < 3 || b.holds(cell) {
                        let found = self.object_index(by).map(|_| by);
                        let attacker = if patrol {
                            if found.is_some() {
                                last = found;
                            }
                            last
                        } else {
                            if found.is_some() {
                                last = found;
                            }
                            found
                        };
                        if attacker.is_some_and(|a| self.object_index(a).is_some()) {
                            if patrol {
                                hit_patrol += 1;
                            } else {
                                hit_posts += 1;
                            }
                        }
                        self.post(Order::ClearBlow { ship: e.ship });
                    }
                }
                if still && formation::cells_apart(cell, (e.x, e.y)) > 1 {
                    self.guard_move(e.ship, (e.x, e.y));
                }
            }
        }
        let busy = 0;
        let players = ai.flotillas[f].behaviour.map_or(0, |b| b.0);
        let targets = self.area_targets(b.rect, Some(ai.owner), players).len() as i32;
        if targets == 0 && busy == 0 {
            if let Some(engaged) = b.engaged.clone() {
                for e in engaged.iter().rev() {
                    let g = ai.flotillas[f].guard.as_mut().unwrap();
                    g.remove(e.ship);
                    match g.mode {
                        1 | 2 => {
                            self.guard_post_one(ai, f, e.ship, true);
                        }
                        3 => {
                            let now = self.now();
                            let g = ai.flotillas[f].guard.as_mut().unwrap();
                            g.post(e.ship, e.x, e.y, now);
                            self.guard_move(e.ship, (e.x, e.y));
                        }
                        _ => {}
                    }
                }
            }
        }
        let sum = hit_patrol + hit_posts + targets;
        let g = ai.flotillas[f].guard.as_ref().unwrap();
        if sum == 0 {
            let posts = g.posts.as_ref().map_or(0, Vec::len);
            let engaged = g.engaged.as_ref().map_or(0, Vec::len);
            if g.pull_back && posts > 0 && engaged > 0 {
                let r = self.navy_draw() as usize % engaged;
                let g = ai.flotillas[f].guard.as_ref().unwrap();
                let target = g.engaged.as_ref().unwrap()[r].target;
                if target != 0 && self.object_index(target).is_some() {
                    let mut k = posts;
                    while k > 0 {
                        k -= 1;
                        let g = ai.flotillas[f].guard.as_mut().unwrap();
                        let p = g.posts.as_mut().unwrap().remove(k);
                        self.guard_engage(ai, f, p.ship, target, p.x, p.y);
                    }
                }
            }
        } else {
            let zone = ai.flotillas[f].zone;
            let t = TILE_SIZE as f32;
            let a = (zone.0 as f32 * t, zone.1 as f32 * t);
            let z = ((zone.0 + zone.2) as f32 * t, (zone.1 + zone.3) as f32 * t);
            for p in g.posts.clone().into_iter().flatten() {
                let idle = self
                    .object_index(p.ship)
                    .and_then(|i| self.objects[i].ship.as_ref())
                    .is_some_and(|s| s.task.is_none() && s.path.is_empty());
                if idle {
                    let _ = self.order_zone(p.ship, a, z, false);
                }
            }
        }
        sum
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FleetOrder, GUARD};
    use super::*;
    use crate::assets::GameData;
    use crate::objects::Caches;

    /// 0x561530 on an open 10 x 10 map over the rect (2, 2, 3, 3): the
    /// middle, then the ring about it (the right column up from one above
    /// the bottom, the top row leftwards, the left column down, the bottom
    /// row rightwards); 12 asked, past the rect the next ring's right
    /// column from its bottom corner up, giving `n` less the rect's 9.
    #[test]
    fn guard_spiral_order() {
        let mut out = Vec::new();
        let r = spiral((2, 2, 3, 3), None, (10, 10), 12, |_, _| true, &mut out);
        assert_eq!(r, 3);
        assert_eq!(
            out,
            [
                (3, 3),
                (4, 3),
                (4, 2),
                (3, 2),
                (2, 2),
                (2, 3),
                (2, 4),
                (3, 4),
                (4, 4),
                (5, 5),
                (5, 4),
                (5, 3)
            ]
        );
        // Reached in the rect: 0.
        let mut out = Vec::new();
        assert_eq!(
            spiral((2, 2, 3, 3), None, (10, 10), 4, |_, _| true, &mut out),
            0
        );
        assert_eq!(out.len(), 4);
        // The map run out: -4. Past the rect the right column runs from
        // the bottom corner and the bottom row to it, so the corner comes
        // twice.
        let mut out = Vec::new();
        assert_eq!(
            spiral((0, 0, 2, 2), None, (3, 3), 20, |_, _| true, &mut out),
            -4
        );
        assert_eq!(
            out,
            [
                (1, 0),
                (0, 0),
                (0, 1),
                (1, 1),
                (2, 2),
                (2, 1),
                (2, 0),
                (0, 2),
                (1, 2),
                (2, 2)
            ]
        );
    }

    /// 0x4d1750: mode 0 the last list's entry; mode 1 the first post and
    /// engaged entry and every patrol entry removed but the one right
    /// after a removal, the first list's kind and index returned.
    #[test]
    fn guard_lists_find_and_remove() {
        let p = |ship| Post {
            ship,
            x: 0,
            y: 0,
            tick: 0,
        };
        let mut b = GuardBlock {
            posts: Some(vec![p(5), p(7), p(7)]),
            engaged: Some(vec![Engaged {
                ship: 7,
                target: 9,
                x: 0,
                y: 0,
            }]),
            patrol: Some(vec![p(7), p(7), p(5), p(7)]),
            ..Default::default()
        };
        assert_eq!(b.find(7), 0x40003);
        assert_eq!(b.find(5), 0x40002);
        assert_eq!(b.find(0), 0);
        assert_eq!(b.remove(7), 0x10001);
        assert_eq!(b.posts.as_ref().unwrap().len(), 2);
        assert!(b.engaged.as_ref().unwrap().is_empty());
        let left: Vec<u32> = b.patrol.as_ref().unwrap().iter().map(|e| e.ship).collect();
        assert_eq!(left, [7, 5]);
        assert_eq!(b.find(7), 0x40000);
        assert_eq!(b.remove(7), 0x10001);
        assert_eq!(b.remove(7), 0);
    }

    /// A computer's guard order on battle01 (0x483570 mode 3 -> 0x4d1ce0):
    /// the block's zone the home, its second rect the flotilla's zone, the
    /// ships posted by the formation in their group's order and sent to
    /// their posts. Its notices: 0x13a zeroes the engaged entry's target,
    /// 0x137 takes the ship off its post; heard, the tick does no more.
    /// The tick past the period (by the level: 0x69, 0x4b, 0x3c) watches:
    /// a ship lying still off its post is sent back.
    #[test]
    fn guard_block_on_battle01() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        for _ in 0..1500 {
            scene.update(0.1, &data, &mut caches);
        }
        let owner = scene.computers.players[0].owner;
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(2)
            .collect();
        let ids: Vec<u32> = ships.iter().map(|&i| scene.objects[i].id).collect();
        for &i in &ships {
            scene.objects[i].owner = Some(owner);
        }
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        for fl in &mut ai.flotillas {
            fl.ships.retain(|x| !ids.contains(x));
        }
        let g = ai.by_role[GUARD as usize];
        let (cx, cy) = scene.ship_cell(ships[0]);
        let zone = (cx - 3, cy - 3, 7, 7);
        let fl = &mut ai.flotillas[g];
        fl.ships = ids.clone();
        fl.block = 0;
        fl.home = (cx - 5, cy - 5, 11, 11);
        fl.zone = zone;
        fl.order = FleetOrder::Guard(zone);
        scene.outbox.clear();
        scene.give_order(&mut ai, g);
        assert_eq!(ai.flotillas[g].block, 0x6e);
        let b = ai.flotillas[g].guard.clone().unwrap();
        assert_eq!(
            (b.mode, b.rect, b.rect2),
            (3, (cx - 5, cy - 5, 11, 11), zone)
        );
        let task7 = u32::from(ai.flotillas[g].task == 7);
        assert_eq!(b.flags, u32::from(ai.difficulty == 0) << 16 | task7);
        let posts = b.posts.unwrap();
        assert_eq!(posts.iter().map(|p| p.ship).collect::<Vec<_>>(), ids);
        let t = TILE_SIZE as f32;
        for p in &posts {
            assert!(scene.outbox.contains(&Order::Move {
                ship: p.ship,
                x: p.x as f32 * t + t / 2.0,
                y: p.y as f32 * t + t / 2.0,
            }));
        }
        // Notices: an engaged entry's target zeroed, a leaver unposted.
        ai.flotillas[g].guard.as_mut().unwrap().engaged = Some(vec![Engaged {
            ship: ids[0],
            target: 99,
            x: 0,
            y: 0,
        }]);
        assert!(ai.post_notice(ids[0], 0x13a, 99));
        ai.leave(g, ids[1]);
        let period = [0x69, 0x4b, 0x3c][ai.difficulty.min(2)];
        ai.flotillas[g].elapsed = period + 1;
        scene.guard_pass(&mut ai, g);
        let b = ai.flotillas[g].guard.clone().unwrap();
        assert_eq!(b.engaged.unwrap()[0].target, 0);
        assert_eq!(b.posts.as_ref().unwrap().len(), 1);
        assert_eq!(ai.flotillas[g].elapsed, period + 2);
        // At the period nothing; past it the watch, `+8` back to 0.
        ai.flotillas[g].guard.as_mut().unwrap().engaged = None;
        let far = (cx + 2, cy + 2);
        let p = &mut ai.flotillas[g]
            .guard
            .as_mut()
            .unwrap()
            .posts
            .as_mut()
            .unwrap()[0];
        (p.x, p.y) = far;
        scene.objects[ships[0]].ship.as_mut().unwrap().move_state = 0;
        scene.outbox.clear();
        ai.flotillas[g].elapsed = period;
        scene.guard_pass(&mut ai, g);
        assert!(scene.outbox.is_empty());
        scene.guard_pass(&mut ai, g);
        assert_eq!(ai.flotillas[g].elapsed, 1);
        if formation::cells_apart(scene.ship_cell(ships[0]), far) > 1 {
            assert!(scene.outbox.contains(&Order::Move {
                ship: ids[0],
                x: far.0 as f32 * t + t / 2.0,
                y: far.1 as f32 * t + t / 2.0,
            }));
        }
        scene.computers.players[0] = ai;
    }
}
