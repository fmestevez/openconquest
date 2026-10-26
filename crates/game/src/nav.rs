//! Navigation grid and path finding for ships.
//!
//! Ships move between map cells. A cell is navigable when its base terrain
//! is sea or shelf (tile sets 0 and 1); island ground, including coast cells,
//! blocks ships. Shoals (layer 1, tile sets `b_ban01_`/`b_ban02_`) are
//! water too, but ships of deep draught cannot cross them.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use openac_formats::MapGrid;

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

    /// Whether a ship can sail straight from world point `a` to `b` (cells
    /// of `cell` pixels): every point along the way, and beside it by a
    /// third of a cell on either side, is passable, so the ship keeps clear
    /// of the coast.
    pub fn clear_line(&self, a: (f32, f32), b: (f32, f32), cell: f32, deep: bool) -> bool {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1.0 {
            return true;
        }
        let (nx, ny) = (-dy / len * cell / 3.0, dx / len * cell / 3.0);
        let steps = (len / (cell / 4.0)).ceil() as i32;
        (0..=steps).all(|k| {
            let f = k as f32 / steps as f32;
            let (x, y) = (a.0 + dx * f, a.1 + dy * f);
            [(0.0, 0.0), (nx, ny), (-nx, -ny)].iter().all(|&(ox, oy)| {
                let (px, py) = (x + ox, y + oy);
                self.passable((px / cell).floor() as i32, (py / cell).floor() as i32, deep)
            })
        })
    }

    /// Straightens a route of world points from `start`: from each point
    /// it goes straight to the furthest later one in clear sight, so ships
    /// sail direct courses instead of the grid's 45-degree steps.
    pub fn smooth(
        &self,
        start: (f32, f32),
        points: &[(f32, f32)],
        cell: f32,
        deep: bool,
    ) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        let mut from = start;
        let mut i = 0;
        while i < points.len() {
            let mut far = i;
            for j in (i + 1..points.len()).rev() {
                if self.clear_line(from, points[j], cell, deep) {
                    far = j;
                    break;
                }
            }
            out.push(points[far]);
            from = points[far];
            i = far + 1;
        }
        out
    }

    /// A* over 8-connected water cells. Diagonal moves may not cut past land
    /// corners. Returns the cells after `from` up to and including `to`:
    /// the shortest route from `from` to `to`, avoiding shoals if `deep`.
    pub fn find_path(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        deep: bool,
    ) -> Option<Vec<(i32, i32)>> {
        if !self.passable(to.0, to.1, deep) {
            return None;
        }
        let w = self.width as i32;
        let idx = |(x, y): (i32, i32)| (y * w + x) as usize;
        let n = (self.width * self.height) as usize;
        const STRAIGHT: u32 = 10;
        const DIAGONAL: u32 = 14;
        let heuristic = |(x, y): (i32, i32)| {
            let (dx, dy) = ((x - to.0).unsigned_abs(), (y - to.1).unsigned_abs());
            STRAIGHT * dx.max(dy) + (DIAGONAL - STRAIGHT) * dx.min(dy)
        };
        let mut cost = vec![u32::MAX; n];
        let mut came_from = vec![usize::MAX; n];
        let mut open = BinaryHeap::new();
        cost[idx(from)] = 0;
        open.push(Reverse((heuristic(from), 0u32, from)));
        while let Some(Reverse((_, g, cell))) = open.pop() {
            if cell == to {
                let mut path = vec![cell];
                let mut i = idx(cell);
                while came_from[i] != usize::MAX && came_from[i] != idx(from) {
                    i = came_from[i];
                    path.push((i as i32 % w, i as i32 / w));
                }
                path.reverse();
                return Some(path);
            }
            if g > cost[idx(cell)] {
                continue;
            }
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let next = (cell.0 + dx, cell.1 + dy);
                if !self.passable(next.0, next.1, deep) {
                    continue;
                }
                let diagonal = dx != 0 && dy != 0;
                if diagonal
                    && !(self.passable(cell.0 + dx, cell.1, deep)
                        && self.passable(cell.0, cell.1 + dy, deep))
                {
                    continue;
                }
                let ng = g + if diagonal { DIAGONAL } else { STRAIGHT };
                if ng < cost[idx(next)] {
                    cost[idx(next)] = ng;
                    came_from[idx(next)] = idx(cell);
                    open.push(Reverse((ng + heuristic(next), ng, next)));
                }
            }
        }
        None
    }
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
    fn open_water_routes_are_straight() {
        let nav = NavGrid::from_rows(&[
            "..........", //
            "..........", //
            "..........", //
            "..........", //
        ]);
        let cells = nav.find_path((0, 0), (9, 3), false).unwrap();
        let points: Vec<(f32, f32)> = cells
            .iter()
            .map(|&(x, y)| (x as f32 * 64.0 + 32.0, y as f32 * 64.0 + 32.0))
            .collect();
        let route = nav.smooth((32.0, 32.0), &points, 64.0, false);
        assert_eq!(route, vec![(9.0 * 64.0 + 32.0, 3.0 * 64.0 + 32.0)]);
    }

    #[test]
    fn smoothed_routes_keep_off_land() {
        let nav = NavGrid::from_rows(&[
            ".....", //
            ".###.", //
            ".#...", //
            ".#.#.", //
        ]);
        let cells = nav.find_path((0, 3), (2, 3), false).unwrap();
        let points: Vec<(f32, f32)> = cells
            .iter()
            .map(|&(x, y)| (x as f32 * 64.0 + 32.0, y as f32 * 64.0 + 32.0))
            .collect();
        let route = nav.smooth((32.0, 3.0 * 64.0 + 32.0), &points, 64.0, false);
        let mut from = (32.0, 3.0 * 64.0 + 32.0);
        for &p in &route {
            assert!(nav.clear_line(from, p, 64.0, false));
            from = p;
        }
        assert!(route.len() < points.len());
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
