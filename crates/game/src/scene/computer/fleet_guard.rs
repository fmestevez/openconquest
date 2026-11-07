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

    /// 0x4d1670: ship `id`'s patrol entry at `(x, y)` (the list made if
    /// null): its entry updated with the tick `now`, else appended. Its
    /// index.
    pub fn patrol_add(&mut self, id: u32, x: i32, y: i32, now: u32) -> usize {
        let l = self.patrol.get_or_insert_with(Vec::new);
        if let Some(k) = l.iter().position(|p| p.ship == id) {
            l[k] = Post {
                ship: id,
                x,
                y,
                tick: now,
            };
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

/// 0x4d2160: corner `c` of `rect`: 1 the top right, 2 the bottom right,
/// 3 the bottom left, 0 and any other the top left.
pub(super) fn corner((rx, ry, w, h): Cells, c: u32) -> (i32, i32) {
    match c {
        1 => (rx + w - 1, ry),
        2 => (rx + w - 1, ry + h - 1),
        3 => (rx, ry + h - 1),
        _ => (rx, ry),
    }
}

/// 0x4d2110: the patrol's size for a group of `n` ships: 0, 1, 2, and from
/// 3 on 2, or 3 when `n / 3` passes 3.
pub(super) fn patrol_size(n: usize) -> usize {
    match n {
        0..=2 => n,
        _ => 2 + usize::from(n / 3 > 3),
    }
}

/// 0x4d25a0's choice of the posts to send on patrol, from `start` (its
/// draw `% posts`): `k` indices on from it, wrapping, at most `posts`; then
/// sorted largest first by the original's exchange (a swap whenever
/// `a[j] <= a[i]`, `j < i`), whose swap writes the smaller index into
/// `ebx` (0x4d2652 `mov ebx, esi`), the register holding the corner `c`
/// drawn: the corner the last swap's smaller index, else `c`.
pub(super) fn restaff_pick(posts: usize, start: usize, k: usize, c: u32) -> (Vec<usize>, u32) {
    let mut idx = Vec::new();
    let mut n = start;
    for _ in 0..k {
        if idx.len() >= posts {
            break;
        }
        idx.push(n);
        n += 1;
        if n >= posts {
            n = 0;
        }
    }
    let mut ebx = c;
    for i in 0..idx.len() {
        for j in 0..i {
            if idx[j] <= idx[i] {
                ebx = idx[j] as u32;
                idx.swap(j, i);
            }
        }
    }
    (idx, ebx)
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
    /// Fleet order 0x6e from a computer's flotilla (the only sender of
    /// 0x6e through 0x4a09d0: no mission's brain guards; 0x4d1ce0's other
    /// caller is the human's guard zone, 0x49d790 case 9, mode 2, not
    /// ported) (0x483570 mode 3, at
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
    /// grows by the tick.
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
                    if self.guard_watch(ai, f) == 0 {
                        self.guard_patrol(ai, f);
                    }
                }
                2 | 3 => {
                    self.guard_watch(ai, f);
                }
                _ => {}
            }
            if ai.flotillas[f]
                .guard
                .as_ref()
                .is_some_and(|b| b.engaged.is_some())
            {
                self.guard_strike(ai, f);
            }
            ai.flotillas[f].elapsed = 0;
        }
        ai.flotillas[f].elapsed += 1;
    }

    /// 0x4d1ea0: the group's notices newest first, each removed; one whose
    /// ship (0x4cc590) is not in the group dropped uncounted but a 0x137;
    /// the others counted heard: 300 (a join) mode 1, 2, or 3 with no
    /// second rect (w or h 0) posted by 0x4d1a50, mode 3 with it the
    /// formation into it for the ship (0x4d1080), then `#` sub 1 with the
    /// group's three (0x4c42f0); 0x136 (at its move goal) with the ship on
    /// the patrol (0x4d1750 mode 0 bit 4) and the patrol settled (0x4d2200),
    /// the next corner (`+0x29` round 0-3, 0x4d2160, 0x4d22f0); 0x137
    /// (left) the ship out of every list (0x4d1750 mode 1), one only on
    /// the patrol (kind 4) restaffed by 0x4d25a0 with 0x4d2110's size;
    /// 0x13a / 0x13c / 0x13d (target gone, a boarding or ram
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
                0x136 => {
                    if b.find(id) >> 16 & 4 != 0 && self.guard_settled(ai, f) {
                        self.guard_next_corner(ai, f);
                    }
                }
                0x137 => {
                    if b.remove(id) >> 16 == 4 {
                        let k = patrol_size(ai.flotillas[f].ships.len());
                        self.guard_restaff(ai, f, k);
                    }
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
    /// off its entry (0x557440) sent back there (0x4cf070). Busy: the
    /// retreat test 0x4d3080. The zone's targets (0x4cd300 with
    /// the group's players `+0x19`). No targets, not busy and the engaged
    /// list: its entries from the last, each ship out of every list
    /// (0x4d1750 mode 1), mode 1 or 2 posted again by 0x4d1a50 (forced),
    /// mode 3 posted at the entry's cell (0x4d15a0) and sent there. The sum
    /// of the hit patrol ships, hit posts and targets: 0 with `+0x2b`, posts
    /// and engaged entries, one engaged entry **drawn** (navy LCG) and with
    /// its target found every post from the last removed and engaged on it
    /// at its cell (0x4d18d0); above 0 the sharing out 0x4d2b00 with the
    /// targets, the hit patrol ships' list (A4) and the hit posts' (A1).
    fn guard_watch(&mut self, ai: &mut Computer, f: usize) -> i32 {
        let Some(b) = ai.flotillas[f].guard.clone() else {
            return 0;
        };
        let mut last: Option<u32> = None;
        // A4 and A1: `{ship, attacker}`.
        let mut a4: Vec<(u32, u32)> = Vec::new();
        let mut a1: Vec<(u32, u32)> = Vec::new();
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
                        if let Some(a) = attacker.filter(|&a| self.object_index(a).is_some()) {
                            if patrol {
                                a4.push((e.ship, a));
                            } else {
                                a1.push((e.ship, a));
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
        let busy = self.guard_retreat(ai, f);
        let players = ai.flotillas[f].behaviour.map_or(0, |b| b.0);
        let found: Vec<u32> = self
            .area_targets(b.rect, Some(ai.owner), players)
            .iter()
            .map(|t| t.id)
            .collect();
        let targets = found.len() as i32;
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
        let sum = a4.len() as i32 + a1.len() as i32 + targets;
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
            self.guard_share(ai, f, &found, &a4, &a1);
        }
        sum
    }

    /// The strength (0x50ea70) of the ship or object `id`, 0 when gone.
    fn guard_strength(&self, id: u32) -> i32 {
        self.object_index(id).map_or(0, |i| self.strength(i))
    }

    /// The guard's period P (0x4d3b30, 0x4d3080): by the computer's level
    /// 0x69, 0x4b, 0x3c.
    fn guard_period(ai: &Computer) -> u32 {
        [0x69, 0x4b, 0x3c][ai.difficulty.min(2)]
    }

    /// 0x4d3080(G, 2), the retreat test, on the engaged list: `+0x2b`
    /// cleared. With the list, its entries from the last: a ship outside
    /// the zone (`x <= rx`, `x > rx + w`, `y <= ry` or `y > ry + h`) whose
    /// nearer edge lies more than 2 off on x or on y and more than 6 from
    /// its entry (0x557480) a stray; its strength (0x50ea70) summed; hit
    /// (`+0x691`), its attacker found (0x51f1f0) sets `+0x2c` to now and
    /// the strongest attacker (strictly) is kept, the blow forgotten either
    /// way (0x4cf760); an entry with no target idle. The strongest
    /// attacker's group (0x408b70) weighs `fire + board / 10`. All idle
    /// (an empty list too): 0; strays over 90% of fewer than 8 entries, or
    /// over 70% of 8 or more: 0; else the strays, with `+0x2b` set when
    /// there are some and the own strength is below twice the enemy's.
    /// Neither of the two first and 0, with now - `+0x2c` below 3 P
    /// (unsigned; also with no list): 1.
    fn guard_retreat(&mut self, ai: &mut Computer, f: usize) -> i32 {
        let now = self.now();
        let Some(b) = ai.flotillas[f].guard.as_mut() else {
            return 0;
        };
        b.pull_back = false;
        let (rx, ry, rw, rh) = b.rect;
        let mut result = 0;
        let mut settled = false;
        if let Some(engaged) = b.engaged.clone() {
            let n = engaged.len() as i32;
            let (mut stray, mut own, mut idle) = (0, 0, 0);
            let (mut best, mut best_strength) = (None, 0);
            for e in engaged.iter().rev() {
                let Some(i) = self.object_index(e.ship) else {
                    continue;
                };
                let (x, y) = self.ship_cell(i);
                if x <= rx || x > rx + rw || y <= ry || y > ry + rh {
                    let dx = (x - rw - rx).abs().min((x - rx).abs());
                    let dy = (y - rh - ry).abs().min((y - ry).abs());
                    if (dx > 2 || dy > 2) && steps((x, y), (e.x, e.y)) > 6 {
                        stray += 1;
                    }
                }
                own += self.strength(i);
                if let Some(by) = self.objects[i].ship.as_ref().and_then(|s| s.blow) {
                    if let Some(k) = self.object_index(by) {
                        if let Some(b) = ai.flotillas[f].guard.as_mut() {
                            b.engaged_at = now;
                        }
                        let st = self.strength(k);
                        if best_strength < st {
                            best = Some(by);
                            best_strength = st;
                        }
                    }
                    self.post(Order::ClearBlow { ship: e.ship });
                }
                if e.target == 0 {
                    idle += 1;
                }
            }
            let enemy = best.map_or(0, |a| self.enemy_power(ai.owner, a));
            if idle == n
                || (n < 8 && n > 0 && stray * 100 / n > 90)
                || (n >= 8 && stray * 100 / n > 70)
            {
                settled = true;
            } else {
                result = stray;
                if stray != 0 && own < enemy * 2 {
                    ai.flotillas[f].guard.as_mut().unwrap().pull_back = true;
                }
            }
        }
        let at = ai.flotillas[f].guard.as_ref().map_or(0, |b| b.engaged_at);
        if result == 0 && !settled && now.wrapping_sub(at) < 3 * Self::guard_period(ai) {
            result = 1;
        }
        result
    }

    /// 0x4d2980(list, n, own, kind): the strength (0x50ea70) of the list's
    /// targets summed; at most twice `own`, that sum; else one entry
    /// **drawn** (navy LCG `% n`) and its target's group (0x408b70) weighed
    /// `fire + board / 10`. An empty list 0.
    fn guard_weigh(&mut self, ai: &Computer, targets: &[u32], own: i32) -> i32 {
        if targets.is_empty() {
            return 0;
        }
        let sum: i32 = targets.iter().map(|&t| self.guard_strength(t)).sum();
        if sum <= own * 2 {
            return sum;
        }
        let r = self.navy_draw() as usize % targets.len();
        let t = targets[r];
        if t == 0 {
            return 0;
        }
        self.enemy_power(ai.owner, t)
    }

    /// The strength summed of the patrol's ships.
    fn guard_patrol_strength(&self, ai: &Computer, f: usize) -> i32 {
        let b = ai.flotillas[f].guard.as_ref();
        b.and_then(|b| b.patrol.as_ref())
            .into_iter()
            .flatten()
            .map(|p| self.guard_strength(p.ship))
            .sum()
    }

    /// Every entry of the posts (`patrol` false) or the patrol, from the
    /// last, removed and engaged (0x4d18d0) at its cell: on `targets` round
    /// robin from the first (0x4d2b00's own < 3 v cases), or, `drawn`, on a
    /// target **drawn** for each (navy LCG `% t`, its case 1).
    fn guard_send_all(
        &mut self,
        ai: &mut Computer,
        f: usize,
        patrol: bool,
        targets: &[u32],
        drawn: bool,
    ) {
        let n = {
            let b = ai.flotillas[f].guard.as_ref().unwrap();
            let l = if patrol { &b.patrol } else { &b.posts };
            match l {
                Some(l) => l.len(),
                None => return,
            }
        };
        let mut r = 0;
        for k in (0..n).rev() {
            let b = ai.flotillas[f].guard.as_mut().unwrap();
            let l = if patrol { &mut b.patrol } else { &mut b.posts };
            let Some(l) = l.as_mut().filter(|l| k < l.len()) else {
                continue;
            };
            let p = l.remove(k);
            let t = if drawn {
                targets[self.navy_draw() as usize % targets.len()]
            } else {
                let t = targets[r];
                r += 1;
                if r >= targets.len() {
                    r = 0;
                }
                t
            };
            self.guard_engage(ai, f, p.ship, t, p.x, p.y);
        }
    }

    /// 0x4d2b00, the sharing out, with the zone's targets T, the hit patrol
    /// ships' A4 and the hit posts' A1 (`{ship, attacker}`):
    /// 1. targets: own the patrol ships' strength; v = 0x4d2980(T, 0x1e);
    ///    own below 3 v with the posts: each post from the last removed and
    ///    engaged (0x4d18d0) at its cell on a T entry **drawn** (navy LCG
    ///    `% t`, one each); then, whatever the test, each patrol entry so.
    /// 2. A1, patrol and posts not both empty, the posts: own the hit post
    ///    ships' strength, v = 0x4d2980(A1's attackers, 0x14); own below
    ///    3 v: every post then every patrol entry from the last engaged at
    ///    its cell on A1's attackers round robin (from the first again for
    ///    the patrol); else each hit post ship (from the first) out of the
    ///    lists (0x4d1750 mode 1) and, listed, engaged on its attacker at
    ///    the ship's cell (`+0x5a3 / +0x5a7`).
    /// 3. A4, the patrol, both not empty: own the patrol ships' strength,
    ///    v = 0x4d2980(A4's attackers, 10); own below 3 v: as in 2 from A4;
    ///    else each hit patrol ship from the last out of the lists and,
    ///    listed, engaged on its attacker at A4's words 4 and 5, which
    ///    0x4d33c0 never writes (the original reads uninitialised heap).
    ///    **Stand-in, not the original's:** the ship's own cell, as 2 uses.
    fn guard_share(
        &mut self,
        ai: &mut Computer,
        f: usize,
        targets: &[u32],
        a4: &[(u32, u32)],
        a1: &[(u32, u32)],
    ) {
        if ai.flotillas[f].guard.is_none() {
            return;
        }
        let counts = |ai: &Computer| {
            let b = ai.flotillas[f].guard.as_ref().unwrap();
            let n = b.patrol.as_ref().map_or(0, Vec::len) + b.posts.as_ref().map_or(0, Vec::len);
            (n, b.patrol.is_some(), b.posts.is_some())
        };
        if !targets.is_empty() {
            let own = self.guard_patrol_strength(ai, f);
            let v = self.guard_weigh(ai, targets, own);
            if own < v * 3 {
                self.guard_send_all(ai, f, false, targets, true);
            }
            self.guard_send_all(ai, f, true, targets, true);
        }
        let (n, _, posts) = counts(ai);
        if !a1.is_empty() && n > 0 && posts {
            let own: i32 = a1.iter().map(|&(s, _)| self.guard_strength(s)).sum();
            let attackers: Vec<u32> = a1.iter().map(|&(_, a)| a).collect();
            let v = self.guard_weigh(ai, &attackers, own);
            if own < v * 3 {
                self.guard_send_all(ai, f, false, &attackers, false);
                self.guard_send_all(ai, f, true, &attackers, false);
            } else {
                for &(ship, attacker) in a1 {
                    self.guard_reengage(ai, f, ship, attacker);
                }
            }
        }
        let (n, patrol, _) = counts(ai);
        if !a4.is_empty() && patrol && n > 0 {
            let own = self.guard_patrol_strength(ai, f);
            let attackers: Vec<u32> = a4.iter().map(|&(_, a)| a).collect();
            let v = self.guard_weigh(ai, &attackers, own);
            if own < v * 3 {
                self.guard_send_all(ai, f, false, &attackers, false);
                self.guard_send_all(ai, f, true, &attackers, false);
            } else {
                for &(ship, attacker) in a4.iter().rev() {
                    // Stand-in (see above): the original's cell is A4[4],
                    // A4[5], never written.
                    self.guard_reengage(ai, f, ship, attacker);
                }
            }
        }
    }

    /// 0x4d2b00's single sends: ship `ship` out of every list (0x4d1750
    /// mode 1) and, listed, engaged on `attacker` at its own cell.
    fn guard_reengage(&mut self, ai: &mut Computer, f: usize, ship: u32, attacker: u32) {
        if ai.flotillas[f].guard.as_mut().unwrap().remove(ship) == 0 {
            return;
        }
        let Some(i) = self.object_index(ship) else {
            return;
        };
        let (x, y) = self.ship_cell(i);
        self.guard_engage(ai, f, ship, attacker, x, y);
    }

    /// 0x4d2720, the patrol rounds (mode 1, the watch giving 0), only with
    /// the engaged list null or empty: no patrol, with the posts and
    /// 0x4d28e0, 0x4d25a0 with 0x4d2110's size; else the patrol's first
    /// entry 10001 ticks old or more (unsigned, `>= 0x2711`), or 0x4d2200,
    /// the next corner. (Its branch for an age of 65000 or more, a 1 in 3
    /// draw recalling the patrol, lies under `age < 0x2711`: never taken.)
    fn guard_patrol(&mut self, ai: &mut Computer, f: usize) {
        let now = self.now();
        let Some(b) = ai.flotillas[f].guard.as_ref() else {
            return;
        };
        if b.engaged.as_ref().is_some_and(|l| !l.is_empty()) {
            return;
        }
        match b.patrol.as_ref().filter(|l| !l.is_empty()) {
            None => {
                if b.posts.is_some() && self.guard_open_cells(ai, f) {
                    let k = patrol_size(ai.flotillas[f].ships.len());
                    self.guard_restaff(ai, f, k);
                }
            }
            Some(l) => {
                if now.wrapping_sub(l[0].tick) >= 0x2711 || self.guard_settled(ai, f) {
                    self.guard_next_corner(ai, f);
                }
            }
        }
    }

    /// 0x4d28e0: the cells 0x499990(cell, 0) takes counted over x from 0
    /// to the zone's right edge and y from 0 to its bottom edge (from the
    /// map's origin, not the zone's: the original's), the cell `y x W + x`;
    /// whether the count passes 3 x the group's ships.
    fn guard_open_cells(&self, ai: &Computer, f: usize) -> bool {
        let Some(b) = ai.flotillas[f].guard.as_ref() else {
            return false;
        };
        let (rx, ry, rw, rh) = b.rect;
        let w = self.nav.width as i32;
        let mut n = 0;
        for y in 0..ry + rh {
            for x in 0..rx + rw {
                let k = y * w + x;
                if self.sea_cell((k % w, k / w), false) {
                    n += 1;
                }
            }
        }
        n > 3 * ai.flotillas[f].ships.len() as i32
    }

    /// 0x4d2200: the patrol not empty, every ship within 1 of its entry on
    /// both axes, and of those at least one entry older than 200 ticks
    /// (unsigned).
    fn guard_settled(&self, ai: &Computer, f: usize) -> bool {
        let now = self.now();
        let Some(l) = ai.flotillas[f]
            .guard
            .as_ref()
            .and_then(|b| b.patrol.as_ref())
            .filter(|l| !l.is_empty())
        else {
            return false;
        };
        let (mut near, mut old) = (0, false);
        for p in l {
            let Some(i) = self.object_index(p.ship) else {
                continue;
            };
            let (x, y) = self.ship_cell(i);
            if (x - p.x).abs() < 2 && (y - p.y).abs() < 2 {
                near += 1;
                if now.wrapping_sub(p.tick) > 200 {
                    old = true;
                }
            }
        }
        near == l.len() && old
    }

    /// `+0x29` on round 0-3 and the patrol sent to that corner of the zone
    /// (0x4d2160, 0x4d22f0): 0x4d2720's and notice 0x136's.
    fn guard_next_corner(&mut self, ai: &mut Computer, f: usize) {
        let Some(b) = ai.flotillas[f].guard.as_mut() else {
            return;
        };
        b.corner = b.corner.wrapping_add(1);
        if b.corner > 3 {
            b.corner = 0;
        }
        let to = corner(b.rect, u32::from(b.corner));
        self.guard_patrol_go(ai, f, to);
    }

    /// 0x4d22f0(x, y): the patrol empty, 0. One ship: its entry at `(x, y)`
    /// (0x4d1670) and moved there (0x4cf070), 0. More: their records (cell,
    /// class 0, the draught `+0x592` in `+0xc`) planned by 0x5614e0 about
    /// `(x, y)` on 0x4d50b0's grids, the first 0x5c9398 when a ship has
    /// `+0x592` 0, the second 0x5c93bc when one has it set; planned, each
    /// ship's entry at its cell and moved there. The count sent.
    fn guard_patrol_go(&mut self, ai: &mut Computer, f: usize, (x, y): (i32, i32)) -> i32 {
        let now = self.now();
        let Some(l) = ai.flotillas[f]
            .guard
            .as_ref()
            .and_then(|b| b.patrol.clone())
            .filter(|l| !l.is_empty())
        else {
            return 0;
        };
        if l.len() < 2 {
            let ship = l[0].ship;
            if let Some(b) = ai.flotillas[f].guard.as_mut() {
                b.patrol_add(ship, x, y, now);
            }
            self.guard_move(ship, (x, y));
            return 0;
        }
        let ships: Vec<(u32, (i32, i32), bool)> = l
            .iter()
            .filter_map(|p| {
                let i = self.object_index(p.ship)?;
                let kind = self.objects[i].ship.as_ref()?.kind;
                Some((
                    p.ship,
                    self.ship_cell(i),
                    crate::objects::deep_draught(kind),
                ))
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
        let (deep, shallow) = (self.guard_grid(true), self.guard_grid(false));
        let p1 = ships.iter().any(|s| !s.2).then_some(&deep[..]);
        let p2 = ships.iter().any(|s| s.2).then_some(&shallow[..]);
        let dims = (self.nav.width as i32, self.nav.height as i32);
        let Ok(planned) = formation::plan_records(p1, p2, None, dims, (x, y), &recs) else {
            return 0;
        };
        if planned.value < 0 {
            return 0;
        }
        for (s, &cell) in ships.iter().zip(&planned.cells) {
            if let Some(b) = ai.flotillas[f].guard.as_mut() {
                b.patrol_add(s.0, cell.0, cell.1, now);
            }
            self.guard_move(s.0, cell);
        }
        ships.len() as i32
    }

    /// A copy of the ships' grid by draught (0x5c9398 deep, 0x5c93bc
    /// shallow): 0 open, -1 closed.
    fn guard_grid(&self, deep: bool) -> Vec<i16> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| if self.nav.passable(x, y, deep) { 0 } else { -1 })
            .collect()
    }

    /// 0x4d25a0(k): the posts empty, nothing. A start **drawn** (navy LCG
    /// `% posts`) and `k` posts on from it (`restaff_pick`), a corner
    /// **drawn** (`& 3`) into `+0x29`; each chosen post's ship onto the
    /// patrol at (-1, -1) (0x4d1670) and its post removed, largest index
    /// first; then the patrol sent (0x4d22f0) to the corner `restaff_pick`
    /// leaves in `ebx` (an index above 3 the top left). Whether any went.
    fn guard_restaff(&mut self, ai: &mut Computer, f: usize, k: usize) -> bool {
        let now = self.now();
        let posts = ai.flotillas[f]
            .guard
            .as_ref()
            .and_then(|b| b.posts.as_ref())
            .map_or(0, Vec::len);
        if posts == 0 {
            return false;
        }
        let start = self.navy_draw() as usize % posts;
        let c = self.navy_draw() & 3;
        let (idx, ebx) = restaff_pick(posts, start, k, c);
        let b = ai.flotillas[f].guard.as_mut().unwrap();
        b.corner = c as u8;
        let mut moved = false;
        for i in idx {
            let ship = b.posts.as_ref().unwrap()[i].ship;
            b.patrol_add(ship, -1, -1, now);
            b.posts.as_mut().unwrap().remove(i);
            moved = true;
        }
        let to = corner(b.rect, ebx);
        if self.guard_patrol_go(ai, f, to) > 0 {
            moved = true;
        }
        moved
    }

    /// 0x4d3990, the engaged ships' boarding and ramming: each entry in
    /// order whose ship is no phantom (`+0x253`): a **draw** (navy LCG
    /// `% 100`), the block's level (`+0x20 >> 16`) 1 passing it over above
    /// 60, 2 above 30; then taking a contact (`+0x261 & 0x1000`) and on no
    /// boarding or ram (`+0x25d != 0x10`), 0x4baae0 (reach 2, the flags'
    /// low half as 0x4ba810's mode, `square_pick`) finding one: boarding
    /// odds 101 or more board it (0x4d0190, command 0x17), else ramming
    /// odds 101 or more ram it (0x4d01d0, 0x18), and from then on no ship
    /// takes the next step; else, none sent so yet this pass, a ship of
    /// kind 10 rams and one of kind 11 boards what 0x4baae0 finds again.
    fn guard_strike(&mut self, ai: &mut Computer, f: usize) {
        let Some(b) = ai.flotillas[f].guard.as_ref() else {
            return;
        };
        let Some(engaged) = b.engaged.clone() else {
            return;
        };
        let (level, mode) = (b.flags >> 16, b.flags & 0xffff);
        let mut done = false;
        for e in engaged {
            let Some(i) = self.object_index(e.ship) else {
                continue;
            };
            if self.objects[i]
                .combat
                .as_ref()
                .is_some_and(|c| c.phantom > 0.0)
            {
                continue;
            }
            let r = self.navy_draw() % 100;
            if (level == 1 && r > 60) || (level == 2 && r > 30) {
                continue;
            }
            if self.takes_contact(i) && !self.closing_in(i) {
                let (pick, board, ram) = self.square_pick(i, mode);
                if pick != 0 && (board >= 0x65 || ram >= 0x65) {
                    self.post(Order::Close {
                        ship: e.ship,
                        target: pick,
                        board: board >= 0x65,
                    });
                    done = true;
                    continue;
                }
            }
            let kind = self.objects[i].ship.as_ref().map_or(0, |s| s.kind);
            if !done && matches!(kind, 10 | 11) {
                let (pick, ..) = self.square_pick(i, mode);
                if pick != 0 {
                    self.post(Order::Close {
                        ship: e.ship,
                        target: pick,
                        board: kind == 11,
                    });
                }
            }
        }
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

    /// 0x4d2110's sizes, 0x4d2160's corners and 0x4d25a0's choice with the
    /// sort that overwrites the corner register.
    #[test]
    fn guard_patrol_helpers() {
        let sizes: Vec<usize> = [0, 1, 2, 3, 11, 12, 20].map(patrol_size).to_vec();
        assert_eq!(sizes, vec![0, 1, 2, 2, 2, 3, 3]);
        let r = (10, 20, 5, 4);
        let corners: Vec<_> = [0, 1, 2, 3, 7].map(|c| corner(r, c)).to_vec();
        assert_eq!(
            corners,
            vec![(10, 20), (14, 20), (14, 23), (10, 23), (10, 20)]
        );
        // One index: no swap, the drawn corner kept.
        assert_eq!(restaff_pick(5, 3, 1, 2), (vec![3], 2));
        // [3, 4]: swapped, the corner the smaller index 3.
        assert_eq!(restaff_pick(5, 3, 2, 1), (vec![4, 3], 3));
        // Wrapping [4, 0, 1]: 0 <= 1 swapped last, corner 0.
        assert_eq!(restaff_pick(5, 4, 3, 2), (vec![4, 1, 0], 0));
        // No more than the posts.
        assert_eq!(restaff_pick(2, 1, 3, 3), (vec![1, 0], 3));
        // An index above 3 as the corner: the top left.
        assert_eq!(restaff_pick(9, 5, 2, 1), (vec![6, 5], 5));
        assert_eq!(corner(r, 5), (10, 20));
    }

    /// The watch on battle01 with the zone the whole map: the foes there
    /// are its targets, so the sharing out (0x4d2b00 case 1) takes every
    /// post off (the patrol null: own 0 below 3 v) and engages it at its
    /// post on a drawn target, the attack sent; the retreat test then
    /// finds the engagement fresh (`+0x2c` now): busy 1, `+0x2b` clear.
    #[test]
    fn guard_shares_out_on_battle01() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        for _ in 0..1500 {
            scene.update(0.1, &data, &mut caches);
        }
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        let owner = ai.owner;
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some() && scene.objects[i].owner == Some(owner))
            .take(3)
            .collect();
        let ids: Vec<u32> = ships.iter().map(|&i| scene.objects[i].id).collect();
        // The third the human's, a foe of some strength in the zone.
        let human = scene.human.map(|h| (h.nation, h.slot));
        scene.objects[ships[2]].owner = human;
        let ids = ids[..2].to_vec();
        for fl in &mut ai.flotillas {
            fl.ships.retain(|x| !ids.contains(x));
        }
        let g = ai.by_role[GUARD as usize];
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let fl = &mut ai.flotillas[g];
        fl.ships = ids.clone();
        fl.block = 0;
        fl.home = (0, 0, w, h);
        fl.zone = (0, 0, 0, 0);
        fl.order = FleetOrder::Guard((0, 0, 0, 0));
        scene.give_order(&mut ai, g);
        let players = ai.flotillas[g].behaviour.map_or(0, |b| b.0);
        let targets: Vec<u32> = scene
            .area_targets((0, 0, w, h), Some(owner), players)
            .iter()
            .map(|t| t.id)
            .collect();
        assert!(!targets.is_empty());
        let posts = ai.flotillas[g]
            .guard
            .as_ref()
            .unwrap()
            .posts
            .clone()
            .unwrap();
        assert_eq!(posts.len(), 2);
        scene.outbox.clear();
        let sum = scene.guard_watch(&mut ai, g);
        assert_eq!(sum, targets.len() as i32);
        let b = ai.flotillas[g].guard.clone().unwrap();
        assert!(b.posts.unwrap().is_empty());
        assert!(b.patrol.is_none());
        let engaged = b.engaged.unwrap();
        // Posts from the last.
        assert_eq!(
            engaged.iter().map(|e| e.ship).collect::<Vec<_>>(),
            vec![posts[1].ship, posts[0].ship]
        );
        for (e, p) in engaged.iter().zip(posts.iter().rev()) {
            assert!(targets.contains(&e.target));
            assert_eq!((e.x, e.y), (p.x, p.y));
            assert!(scene.outbox.contains(&Order::Attack {
                ship: e.ship,
                target: e.target,
            }));
        }
        assert_eq!(b.engaged_at, scene.now());
        assert_eq!(scene.guard_retreat(&mut ai, g), 1);
        assert!(!ai.flotillas[g].guard.as_ref().unwrap().pull_back);
        scene.computers.players[0] = ai;
    }
}
