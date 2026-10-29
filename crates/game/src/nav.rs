//! Navigation grid and path finding for ships.
//!
//! Ships move between map cells. A cell is navigable when its base terrain
//! is sea or shelf (tile sets 0 and 1); island ground, including coast cells,
//! blocks ships. Shoals (layer 1, tile sets `b_ban01_`/`b_ban02_`) are
//! water too, but ships of deep draught cannot cross them.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use openconquest_formats::MapGrid;

const SEA: u8 = 0x00;
const SHELF: u8 = 0x01;
const SHOALS: [u8; 2] = [0x02, 0x03];

pub struct NavGrid {
    pub width: u32,
    pub height: u32,
    water: Vec<bool>,
    shoal: Vec<bool>,
}

impl NavGrid {
    pub fn from_map(grid: &MapGrid) -> Self {
        let mut water = Vec::with_capacity((grid.width * grid.height) as usize);
        let mut shoal = Vec::with_capacity(water.capacity());
        for y in 0..grid.height {
            for x in 0..grid.width {
                water.push(matches!(grid.get(0, x, y).tileset, SEA | SHELF));
                let over = grid.get(1, x, y);
                shoal.push(!over.is_empty() && SHOALS.contains(&over.tileset));
            }
        }
        Self {
            width: grid.width,
            height: grid.height,
            water,
            shoal,
        }
    }

    #[cfg(test)]
    fn from_rows(rows: &[&str]) -> Self {
        let cells = || rows.iter().flat_map(|r| r.chars());
        Self {
            width: rows[0].len() as u32,
            height: rows.len() as u32,
            water: cells().map(|c| c == '.' || c == '~').collect(),
            shoal: cells().map(|c| c == '~').collect(),
        }
    }

    pub fn is_water(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as u32) < self.width
            && (y as u32) < self.height
            && self.water[(y as u32 * self.width + x as u32) as usize]
    }

    /// Whether a ship can sail into `(x, y)`: water, and no shoal for a
    /// ship of deep draught.
    pub fn passable(&self, x: i32, y: i32, deep: bool) -> bool {
        self.is_water(x, y) && !(deep && self.shoal[(y as u32 * self.width + x as u32) as usize])
    }

    /// The water cell nearest to `(x, y)` (breadth-first), if any.
    pub fn nearest_water(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let max = self.width.max(self.height) as i32;
        for r in 0..max {
            let mut best: Option<(i32, (i32, i32))> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let (cx, cy) = (x + dx, y + dy);
                    let d = dx * dx + dy * dy;
                    if self.is_water(cx, cy) && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, (cx, cy)));
                    }
                }
            }
            if let Some((_, c)) = best {
                return Some(c);
            }
        }
        None
    }

    /// The original's route finder (0x55ba50 with 0x5718f0 and 0x557910)
    /// over 8-connected water cells: two straight runs if they are clear,
    /// else a flood from `from` (a straight step 2, a diagonal 3; diagonals
    /// may pass between closed cells) traced back downhill from `to`.
    /// Returns the cells after `from` up to and including `to`, avoiding
    /// shoals if `deep`.
    #[cfg(test)]
    pub fn find_path(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        deep: bool,
    ) -> Option<Vec<(i32, i32)>> {
        self.find_path_avoiding(from, to, deep, |_| false)
    }

    /// The same, keeping out of the cells `avoid` names (other ships).
    pub fn find_path_avoiding(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        deep: bool,
        avoid: impl Fn((i32, i32)) -> bool,
    ) -> Option<Vec<(i32, i32)>> {
        if !self.passable(to.0, to.1, deep) || avoid(to) {
            return None;
        }
        if from == to {
            return Some(Vec::new());
        }
        let open = |c: (i32, i32)| self.passable(c.0, c.1, deep) && !avoid(c);
        if let Some(path) = two_runs(from, to, &open) {
            return Some(path);
        }
        let w = self.width as i32;
        let h = self.height as i32;
        let idx = |(x, y): (i32, i32)| (y * w + x) as usize;
        let n = (self.width * self.height) as usize;
        // The flood: the start 1, a straight step 2 more, a diagonal 3.
        let mut value = vec![0u32; n];
        let mut queue = BinaryHeap::new();
        value[idx(from)] = 1;
        queue.push(Reverse((1u32, from)));
        while let Some(Reverse((v, cell))) = queue.pop() {
            if v > value[idx(cell)] {
                continue;
            }
            if cell == to {
                break;
            }
            for (d, &(dx, dy)) in DIRS.iter().enumerate() {
                let next = (cell.0 + dx, cell.1 + dy);
                if next.0 < 0 || next.1 < 0 || next.0 >= w || next.1 >= h || !open(next) {
                    continue;
                }
                let nv = v + if d % 2 == 1 { 3 } else { 2 };
                let k = idx(next);
                if value[k] == 0 || nv < value[k] {
                    value[k] = nv;
                    queue.push(Reverse((nv, next)));
                }
            }
        }
        if value[idx(to)] == 0 {
            return None;
        }
        // Back downhill from the goal: the diagonals first, each counted one
        // more, then the straights; a tie keeps to the way it was going.
        let mut path = vec![to];
        let mut cell = to;
        let mut prev: Option<usize> = None;
        while cell != from {
            let mut best: Option<(u32, usize)> = None;
            for d in [1, 3, 5, 7, 0, 2, 4, 6] {
                let (dx, dy) = DIRS[d];
                let next = (cell.0 + dx, cell.1 + dy);
                if next.0 < 0 || next.1 < 0 || next.0 >= w || next.1 >= h {
                    continue;
                }
                let v = value[idx(next)];
                if v == 0 {
                    continue;
                }
                let score = if d % 2 == 1 { v + 1 } else { v };
                let better = match best {
                    None => true,
                    Some((b, _)) => score < b || (score == b && prev == Some(d)),
                };
                if better {
                    best = Some((score, d));
                }
            }
            let (_, d) = best?;
            let next = (cell.0 + DIRS[d].0, cell.1 + DIRS[d].1);
            if value[idx(next)] >= value[idx(cell)] {
                return None;
            }
            prev = Some(d);
            cell = next;
            if cell != from {
                path.push(cell);
            }
        }
        path.reverse();
        Some(path)
    }
}

/// The eight directions: 0 east, counter-clockwise (north is up).
const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// A route of two straight runs from `from` to `to` (0x5718f0): one along
/// the axis the way leans and one diagonal, the axis first when it leans
/// twice as much; the run nearer east first, then the other order. Every
/// cell must be open.
fn two_runs(
    from: (i32, i32),
    to: (i32, i32),
    open: &impl Fn((i32, i32)) -> bool,
) -> Option<Vec<(i32, i32)>> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let (sx, sy) = (dx.signum(), dy.signum());
    let axis = if dx.abs() >= dy.abs() {
        (sx, 0)
    } else {
        (0, sy)
    };
    let diagonal = (sx, sy);
    let lean = dx.abs() >= 2 * dy.abs() || dy.abs() >= 2 * dx.abs();
    let (first, second) = if lean {
        (axis, diagonal)
    } else {
        (diagonal, axis)
    };
    let east = |(x, y): (i32, i32)| {
        let d = DIRS.iter().position(|&v| v == (x, y)).unwrap_or(0) as i32;
        d.min(8 - d)
    };
    let orders = if east(second) < east(first) {
        [(second, first), (first, second)]
    } else {
        [(first, second), (second, first)]
    };
    'order: for (a, b) in orders {
        let mut cell = from;
        let mut path = Vec::new();
        let mut leg = a;
        while cell != to {
            let (lx, ly) = (to.0 - cell.0, to.1 - cell.1);
            // A straight run ends when the rest is a diagonal; a diagonal
            // one when the rest is straight.
            let straight = leg.0 == 0 || leg.1 == 0;
            if leg == a
                && ((straight && lx.abs() == ly.abs()) || (!straight && (lx == 0 || ly == 0)))
            {
                leg = b;
            }
            let step = if leg.0 == 0 || leg.1 == 0 {
                if leg.0 == 0 {
                    (0, ly.signum())
                } else {
                    (lx.signum(), 0)
                }
            } else {
                (lx.signum(), ly.signum())
            };
            if step == (0, 0) {
                continue 'order;
            }
            cell = (cell.0 + step.0, cell.1 + step.1);
            if !open(cell) {
                continue 'order;
            }
            path.push(cell);
        }
        return Some(path);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_goes_around_land() {
        let nav = NavGrid::from_rows(&[
            ".....", //
            ".###.", //
            ".#...", //
            ".#.#.", //
        ]);
        let path = nav.find_path((0, 3), (2, 3), false).unwrap();
        assert_eq!(path.last(), Some(&(2, 3)));
        assert!(path.iter().all(|&(x, y)| nav.is_water(x, y)));
        // Must go up and around the island: no shortcut through (1, 3).
        assert!(path.iter().any(|&(_, y)| y == 0));
    }

    #[test]
    fn no_path_to_land_or_enclosed_water() {
        let nav = NavGrid::from_rows(&["..#", "###", "#.#"]);
        assert!(nav.find_path((0, 0), (2, 0), false).is_none());
        assert!(nav.find_path((0, 0), (1, 2), false).is_none());
        assert_eq!(nav.nearest_water(2, 0), Some((1, 0)));
    }

    #[test]
    fn deep_ships_go_around_shoals() {
        let nav = NavGrid::from_rows(&[
            ".~.", //
            ".~.", //
            "...", //
        ]);
        assert_eq!(nav.find_path((0, 0), (2, 0), false).unwrap().len(), 2);
        let deep = nav.find_path((0, 0), (2, 0), true).unwrap();
        assert!(deep.iter().all(|&(x, y)| nav.passable(x, y, true)));
        assert!(deep.len() > 2);
        assert!(nav.find_path((0, 0), (1, 0), true).is_none());
    }
}
