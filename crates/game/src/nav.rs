//! Navigation grid and path finding for ships.
//!
//! Ships move between map cells. A cell is navigable when its base terrain
//! is sea or shelf (tile sets 0 and 1); island ground, including coast cells,
//! blocks ships. Shoals are treated as navigable for now, although in the
//! original only some ship types can cross them.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use openac_formats::MapGrid;

const SEA: u8 = 0x00;
const SHELF: u8 = 0x01;

pub struct NavGrid {
    pub width: u32,
    pub height: u32,
    water: Vec<bool>,
}

impl NavGrid {
    pub fn from_map(grid: &MapGrid) -> Self {
        let mut water = Vec::with_capacity((grid.width * grid.height) as usize);
        for y in 0..grid.height {
            for x in 0..grid.width {
                water.push(matches!(grid.get(0, x, y).tileset, SEA | SHELF));
            }
        }
        Self {
            width: grid.width,
            height: grid.height,
            water,
        }
    }

    #[cfg(test)]
    fn from_rows(rows: &[&str]) -> Self {
        let water = rows
            .iter()
            .flat_map(|r| r.chars().map(|c| c == '.'))
            .collect();
        Self {
            width: rows[0].len() as u32,
            height: rows.len() as u32,
            water,
        }
    }

    pub fn is_water(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as u32) < self.width
            && (y as u32) < self.height
            && self.water[(y as u32 * self.width + x as u32) as usize]
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

    /// A* over 8-connected water cells. Diagonal moves may not cut past land
    /// corners. Returns the cells after `from` up to and including `to`.
    pub fn find_path(&self, from: (i32, i32), to: (i32, i32)) -> Option<Vec<(i32, i32)>> {
        if !self.is_water(to.0, to.1) {
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
                if !self.is_water(next.0, next.1) {
                    continue;
                }
                let diagonal = dx != 0 && dy != 0;
                if diagonal
                    && !(self.is_water(cell.0 + dx, cell.1) && self.is_water(cell.0, cell.1 + dy))
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
        let path = nav.find_path((0, 3), (2, 3)).unwrap();
        assert_eq!(path.last(), Some(&(2, 3)));
        assert!(path.iter().all(|&(x, y)| nav.is_water(x, y)));
        // Must go up and around the island: no shortcut through (1, 3).
        assert!(path.iter().any(|&(_, y)| y == 0));
    }

    #[test]
    fn no_path_to_land_or_enclosed_water() {
        let nav = NavGrid::from_rows(&["..#", "###", "#.#"]);
        assert!(nav.find_path((0, 0), (2, 0)).is_none());
        assert!(nav.find_path((0, 0), (1, 2)).is_none());
        assert_eq!(nav.nearest_water(2, 0), Some((1, 0)));
    }
}
