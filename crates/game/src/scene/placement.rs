//! Where a computer town puts up a building (`m_build.cpp` 0x42d9c0 with
//! the terrain helper 0x40cca0): a cell of its land touching one of its own
//! cells, scored by the kind of building.

use super::trade::building_cells;
use super::*;
use crate::economy::{DOCK, PORT, SETTLEMENT, SHIPYARD, TOWN};

/// The edges a land tile closes, by its variant (0x5acf3f): bit d closes
/// direction d (0 east, counter-clockwise, 2 north).
const SHAPE: [u8; 45] = [
    0x00, 0x00, 0x1c, 0x1c, 0x1c, 0x1c, 0x1c, 0x07, 0x07, 0x07, 0x07, 0x07, 0x70, 0x70, 0x70, 0x70,
    0x70, 0xc1, 0xc1, 0xc1, 0xc1, 0xc1, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x10,
    0x01, 0x01, 0x04, 0x04, 0x04, 0x40, 0x40, 0x40, 0x40, 0x40, 0x00, 0x00, 0x00,
];

/// The eight directions (0x5c4f4c).
const DIR: [(i32, i32); 8] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

// Cell codes (0x40cca0).
const BLOCKED: u8 = 1;
const SHORE: u8 = 3;
const INLAND: u8 = 4;
const BERTH: u8 = 8;
const OWN: u8 = 0xb;

fn is_fort(class: u32) -> bool {
    (0x34..=0x36).contains(&class)
}

fn is_water_building(class: u32) -> bool {
    matches!(class, PORT | SHIPYARD | DOCK | 0x40)
}

/// Distance as 0x557440 reckons it: the longer side and half the shorter.
fn distance(dx: i32, dy: i32) -> i32 {
    let (a, b) = (dx.abs(), dy.abs());
    (a.min(b) + 2 * a.max(b)) / 2
}

/// A candidate: cell, facing (-1 none) and score.
#[derive(Clone, Copy)]
struct Spot {
    x: i32,
    y: i32,
    dir: i32,
    score: i32,
}

impl Scene {
    /// The cell and facing for building `class` of settlement `site`
    /// (0x42d9c0): forts (flag 0x20), ports, shipyards and docks (flag 2) on
    /// the shore, the rest (flag 1) inland; the best scored, as the
    /// original's sort leaves it first.
    pub(super) fn town_build_place(&self, site: u32, class: u32) -> Option<(i32, i32, u32)> {
        let s = self.object_index(site)?;
        let owner = self.objects[s].owner?;
        let (tx, ty) = self.objects[s].cell?;
        let flags: u8 = if is_fort(class) {
            0x20
        } else if matches!(class, PORT | SHIPYARD | DOCK) {
            0x0e
        } else {
            0x05
        };
        let g = &self.grid;
        let (w, h) = (g.width as i32, g.height as i32);
        let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
        let at = |x: i32, y: i32| (y * w + x) as usize;
        let water = |x: i32, y: i32| self.nav.is_water(x, y);
        // Ground a building may stand on (0x503150): island ground, with
        // none of tile sets 0x1a and 0x30 on layers 5-7.
        let buildable = |x: i32, y: i32| {
            !water(x, y)
                && (5..g.layers.min(8)).all(|l| {
                    let t = g.get(l, x as u32, y as u32);
                    t.is_empty() || !matches!(t.tileset, 0x1a | 0x30)
                })
        };
        let terrain = |x: i32, y: i32| g.get(0, x as u32, y as u32).variant;
        let shape = |x: i32, y: i32| SHAPE.get(terrain(x, y) as usize).copied().unwrap_or(0);
        // The buildings standing on each cell, and the amber.
        let mut standing: Vec<Option<usize>> = vec![None; (w * h) as usize];
        let mut amber = vec![false; (w * h) as usize];
        for (i, o) in self.objects.iter().enumerate() {
            if let (ObjectClass::Building(_), Some((cx, cy))) = (o.class, o.cell) {
                let n = building_cells(o);
                for y in cy..cy + n {
                    for x in cx..cx + n {
                        if inside(x, y) {
                            standing[at(x, y)] = Some(i);
                        }
                    }
                }
            } else if o.deposit.is_some_and(|d| d.resource == Resource::Amber) {
                let (cx, cy) = self.cell_of(i);
                if inside(cx, cy) {
                    amber[at(cx, cy)] = true;
                }
            }
        }
        let class_at = |i: usize| match self.objects[i].class {
            ObjectClass::Building(c) => c,
            _ => 0,
        };
        let own = |i: usize| {
            let c = class_at(i);
            self.objects[i].owner == Some(owner)
                && if matches!(c, SETTLEMENT | TOWN) {
                    self.objects[i].id == site
                } else {
                    (0x34..=0x3e).contains(&c) && self.home_settlement(i) == Some(site)
                }
        };
        // Classify every cell.
        let mut code = vec![0u8; (w * h) as usize];
        let mut bytes = vec![0xffu8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let k = at(x, y);
                if !buildable(x, y) {
                    if code[k] == 0 {
                        code[k] = BLOCKED;
                    }
                    continue;
                }
                bytes[k] = shape(x, y);
                match standing[k] {
                    None => code[k] = if bytes[k] == 0 { INLAND } else { SHORE },
                    Some(i) => {
                        if own(i) {
                            code[k] = OWN;
                        }
                        if is_water_building(class_at(i)) {
                            if let Some((fx, fy)) = self.front_cell(i) {
                                if inside(fx, fy) {
                                    code[at(fx, fy)] = BERTH;
                                }
                            }
                        }
                    }
                }
            }
        }
        // The land's regions (0x5597b0): cells joined across edges both
        // leave open, diagonals included.
        let mut label = vec![0u32; (w * h) as usize];
        let mut next = 0;
        for y in 0..h {
            for x in 0..w {
                if bytes[at(x, y)] == 0xff || label[at(x, y)] != 0 {
                    continue;
                }
                next += 1;
                label[at(x, y)] = next;
                let mut stack = vec![(x, y)];
                while let Some((cx, cy)) = stack.pop() {
                    let m = bytes[at(cx, cy)];
                    for (d, &(dx, dy)) in DIR.iter().enumerate() {
                        let (nx, ny) = (cx + dx, cy + dy);
                        if !inside(nx, ny) {
                            continue;
                        }
                        let n = at(nx, ny);
                        if bytes[n] == 0xff || label[n] != 0 {
                            continue;
                        }
                        if m & (1 << d) != 0 || bytes[n] & (1 << ((d + 4) % 8)) != 0 {
                            continue;
                        }
                        label[n] = next;
                        stack.push((nx, ny));
                    }
                }
            }
        }
        if !inside(tx, ty) || label[at(tx, ty)] == 0 {
            return None;
        }
        let home = label[at(tx, ty)];
        // The facing toward open water: south, north, west, east, first
        // keeping off another building's berth.
        let facing = |x: i32, y: i32, m: u8| -> Option<i32> {
            for avoid_berth in [true, false] {
                for d in [6, 2, 4, 0] {
                    let (dx, dy) = DIR[d as usize];
                    let (nx, ny) = (x + dx, y + dy);
                    let fits = m & (1 << d) != 0
                        && inside(nx, ny)
                        && water(nx, ny)
                        && !(avoid_berth && code[at(nx, ny)] == BERTH);
                    if fits {
                        return Some(d);
                    }
                }
            }
            None
        };
        let around = |x: i32, y: i32| {
            (y - 1..=y + 1)
                .flat_map(move |qy| (x - 1..=x + 1).map(move |qx| (qx, qy)))
                .filter(move |&(qx, qy)| inside(qx, qy))
        };
        let evaluate = |x: i32, y: i32| -> Option<Spot> {
            let k = at(x, y);
            let (c, t, m) = (code[k], terrain(x, y), bytes[k]);
            if flags & 1 != 0 && c == INLAND {
                let mut score = if t == 1 { 100 } else { 0 };
                score += 30
                    * around(x, y)
                        .filter(|&(qx, qy)| label[at(qx, qy)] == home && code[at(qx, qy)] == SHORE)
                        .count() as i32;
                return Some(Spot {
                    x,
                    y,
                    dir: -1,
                    score,
                });
            }
            if flags & 2 != 0 && c == SHORE {
                let dir = facing(x, y, m)?;
                let (fx, fy) = (x + DIR[dir as usize].0, y + DIR[dir as usize].1);
                let mut score = if code[at(fx, fy)] == BERTH { -100 } else { 0 };
                for (qx, qy) in around(fx, fy) {
                    let q = at(qx, qy);
                    if label[q] == 0 {
                        score += if code[q] == BERTH { -100 } else { 10 };
                    } else if code[q] == BLOCKED {
                        if standing[q].is_some_and(|i| is_water_building(class_at(i))) {
                            score -= 20;
                        }
                        if amber[q] {
                            score -= 15;
                        }
                    }
                }
                return Some(Spot { x, y, dir, score });
            }
            if flags & 0x20 != 0 && matches!(c, SHORE | INLAND) && t != 1 {
                let dir = facing(x, y, m).unwrap_or(0);
                let mut score = if (22..=29).contains(&t) || (42..=43).contains(&t) {
                    130
                } else {
                    50
                };
                for (qx, qy) in around(x, y) {
                    let Some(i) = standing[at(qx, qy)] else {
                        continue;
                    };
                    if self.objects[i].owner != Some(owner) {
                        continue;
                    }
                    let oc = class_at(i);
                    if is_fort(oc) {
                        score -= 50;
                    }
                    if matches!(oc, SETTLEMENT | TOWN) {
                        score += 75;
                    }
                    if is_water_building(oc) {
                        score += 50;
                    }
                }
                for cy in y - 6..=y + 6 {
                    for cx in x - 6..=x + 6 {
                        if inside(cx, cy) && distance(cx - x, cy - y) < 7 && water(cx, cy) {
                            score += 5;
                        }
                    }
                }
                return Some(Spot { x, y, dir, score });
            }
            None
        };
        // Candidates: cells of the town's land touching its own cells, each
        // weighed once, in the order the scan reaches them.
        let mut seen = vec![false; (w * h) as usize];
        let mut spots = Vec::new();
        for y in 0..h {
            for x in 0..w {
                if code[at(x, y)] != OWN || label[at(x, y)] != home {
                    continue;
                }
                for (nx, ny) in around(x, y) {
                    let n = at(nx, ny);
                    if label[n] != home || seen[n] {
                        continue;
                    }
                    seen[n] = true;
                    spots.extend(evaluate(nx, ny));
                }
            }
        }
        vc6_qsort(&mut spots, |a, b| b.score - a.score);
        spots.first().map(|s| (s.x, s.y, s.dir as u32 & 0xff))
    }
}

/// The C runtime's `qsort` (0x58791f, Visual C++ 6): not stable, so ties
/// fall where it leaves them.
fn vc6_qsort<T: Copy>(v: &mut [T], cmp: impl Fn(&T, &T) -> i32) {
    if v.len() < 2 {
        return;
    }
    let (mut lo, mut hi) = (0usize, v.len() - 1);
    let mut stack = Vec::new();
    loop {
        let size = hi - lo + 1;
        if size <= 8 {
            // Short sort (0x587a73): the greatest to the end, repeatedly.
            while hi > lo {
                let mut max = lo;
                for p in lo + 1..=hi {
                    if cmp(&v[p], &v[max]) > 0 {
                        max = p;
                    }
                }
                v.swap(max, hi);
                hi -= 1;
            }
        } else {
            let mid = lo + size / 2;
            v.swap(mid, lo);
            let (mut loguy, mut higuy) = (lo, hi + 1);
            loop {
                loop {
                    loguy += 1;
                    if !(loguy <= hi && cmp(&v[loguy], &v[lo]) <= 0) {
                        break;
                    }
                }
                loop {
                    higuy -= 1;
                    if !(higuy > lo && cmp(&v[higuy], &v[lo]) >= 0) {
                        break;
                    }
                }
                if higuy < loguy {
                    break;
                }
                v.swap(loguy, higuy);
            }
            v.swap(lo, higuy);
            if higuy as isize - 1 - lo as isize >= hi as isize - loguy as isize {
                if lo + 1 < higuy {
                    stack.push((lo, higuy - 1));
                }
                if loguy < hi {
                    lo = loguy;
                    continue;
                }
            } else {
                if loguy < hi {
                    stack.push((loguy, hi));
                }
                if lo + 1 < higuy {
                    hi = higuy - 1;
                    continue;
                }
            }
        }
        match stack.pop() {
            Some((l, h)) => (lo, hi) = (l, h),
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qsort_puts_the_highest_first() {
        let mut v: Vec<i32> = (0..40).map(|i| (i * 37) % 23).collect();
        vc6_qsort(&mut v, |a, b| b - a);
        assert!(v.windows(2).all(|p| p[0] >= p[1]));
    }

    #[test]
    fn distance_is_the_longer_side_and_half_the_shorter() {
        assert_eq!(distance(6, 0), 6);
        assert_eq!(distance(4, 4), 6);
        assert_eq!(distance(-3, 6), 7);
    }
}
