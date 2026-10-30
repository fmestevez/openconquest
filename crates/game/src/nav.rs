//! Navigation grid and route finding for ships.
//!
//! Ships move between map cells over the ships' grids by draught, built
//! from the landscape's tile types (0x49a950): a cell is open (0) when its
//! layer-0 type is below 8 (sea, shelf) and its layer-1 type is not 5 (a
//! raised reef); for a ship of deep draught (`+0x592`) not 3 either (a
//! sandbank, the shoals). Every other cell is closed (-1). 0x5c93bc is
//! the shallow grid, 0x5c9398 the deep one.

use crate::scene::walk_path;

pub struct NavGrid {
    pub width: u32,
    pub height: u32,
    water: Vec<bool>,
    shoal: Vec<bool>,
    /// 0x4999e0's water (layer-0 type below 8, layer-1 type not 4, 5 or 6).
    sea: Vec<bool>,
}

impl NavGrid {
    /// The grids from the cells' tile types (`Scene::cell_types`, a nibble
    /// a layer), as 0x49a950 builds them.
    pub fn from_types(width: u32, height: u32, types: &[u32]) -> Self {
        let layers = |t: u32| (t & 0xf, (t >> 4) & 0xf);
        Self {
            width,
            height,
            water: types
                .iter()
                .map(|&t| matches!(layers(t), (l0, l1) if l0 < 8 && l1 != 5))
                .collect(),
            shoal: types.iter().map(|&t| layers(t).1 == 3).collect(),
            sea: types
                .iter()
                .map(|&t| matches!(layers(t), (l0, l1) if l0 < 8 && !matches!(l1, 4..=6)))
                .collect(),
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
            sea: cells().map(|c| c == '.' || c == '~').collect(),
        }
    }

    /// Whether `(x, y)` is open on the shallow grid (0x5c93bc).
    pub fn is_water(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as u32) < self.width
            && (y as u32) < self.height
            && self.water[(y as u32 * self.width + x as u32) as usize]
    }

    /// Whether `(x, y)` is open on the grid of the ship's draught (0x5c93bc,
    /// or 0x5c9398 if `deep`).
    pub fn passable(&self, x: i32, y: i32, deep: bool) -> bool {
        self.is_water(x, y) && !(deep && self.shoal[(y as u32 * self.width + x as u32) as usize])
    }

    /// 0x4999e0(x, y, 0): water a move may end in (layer-0 type below 8,
    /// layer-1 type not 4, 5 or 6). A plain move to any other cell plans
    /// with a tolerance of 1 (0x4a5f30).
    pub fn is_sea(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && (x as u32) < self.width
            && (y as u32) < self.height
            && self.sea[(y as u32 * self.width + x as u32) as usize]
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

    /// The copy of the ship's grid a plan works on (0x5c93c0, copied from
    /// 0x5c93bc or 0x5c9398 by draught), the cells `shut` names closed
    /// (-1) too.
    fn plan_grid(&self, deep: bool, shut: impl Fn((i32, i32)) -> bool) -> Vec<i16> {
        let w = self.width as i32;
        (0..(self.width * self.height) as i32)
            .map(|i| (i % w, i / w))
            .map(|c| {
                if self.passable(c.0, c.1, deep) && !shut(c) {
                    0
                } else {
                    -1
                }
            })
            .collect()
    }

    /// The exact route from `from` to `to` on an open grid.
    #[cfg(test)]
    pub fn find_path(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        deep: bool,
    ) -> Option<Vec<(i32, i32)>> {
        self.find_path_avoiding(from, to, deep, |_| false)
    }

    /// The exact route 0x55ca10 from `from` to `to` with the heading 0 (as
    /// the ships' planners call it), over the ship's grid with the cells
    /// `avoid` names closed: the cells after `from`, up to and including
    /// `to`. `None` unless both ends are open and `to` is reached.
    pub fn find_path_avoiding(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        deep: bool,
        avoid: impl Fn((i32, i32)) -> bool,
    ) -> Option<Vec<(i32, i32)>> {
        let mut m = self.plan_grid(deep, avoid);
        let (w, h) = (self.width as i32, self.height as i32);
        let route = walk_path::route_exact(&mut m, w, h, from, to, 0)?;
        Some(cells_after(&route))
    }

    /// The route 0x55ba50 from `from` towards `to` with the heading 0 and
    /// the tolerance `tol` (as the ships' planners call it), over the
    /// ship's grid with the cells `avoid` names closed: the cells after
    /// `from` up to the goal, or to the reached cell it settles on
    /// (`walk_path::route`). `None` with no reached cell; empty when it
    /// gets no further than `from` (0x4a6330 then takes no route).
    pub fn find_path_near(
        &self,
        from: (i32, i32),
        to: (i32, i32),
        deep: bool,
        avoid: impl Fn((i32, i32)) -> bool,
        tol: i32,
    ) -> Option<Vec<(i32, i32)>> {
        let mut m = self.plan_grid(deep, avoid);
        let (w, h) = (self.width as i32, self.height as i32);
        let (route, _) = walk_path::route(&mut m, w, h, from, to, 0, tol)?;
        Some(cells_after(&route))
    }
}

/// A route's cells after its first (where the ship plans from), as
/// 0x4a6330 takes it: none when it holds one cell, or two the same.
pub(crate) fn cells_after(route: &[(i32, i32, u8)]) -> Vec<(i32, i32)> {
    let same = route.len() == 2 && (route[0].0, route[0].1) == (route[1].0, route[1].1);
    if route.len() < 2 || same {
        return Vec::new();
    }
    route[1..].iter().map(|&(x, y, _)| (x, y)).collect()
}

impl NavGrid {
    /// The wavefront 0x4a1e90 (0x545e20 -> 0x5459d0 with step 3) over a
    /// copy of the ships' grid by draught (0 open, -1 closed): `from` gets
    /// `(limit + 1) * 6`, then layer by layer, each cell of a layer in
    /// the order it was reached, its open unfilled neighbours east, north,
    /// west, south take its value less 6 while that stays above 0, then
    /// north-east, north-west, south-west, south-east its value less 9
    /// (corners may be cut); a cell keeps the value it was first given.
    /// At most `limit + 1` layers. `None` with `from` off the map (-4).
    pub fn wavefront(&self, from: (i32, i32), deep: bool, limit: i32) -> Option<Vec<i16>> {
        let (w, h) = (self.width as i32, self.height as i32);
        if from.0 < 0 || from.0 >= w || from.1 < 0 || from.1 >= h {
            return None;
        }
        const STEP: i32 = 3;
        let mut m: Vec<i16> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| if self.passable(x, y, deep) { 0 } else { -1 })
            .collect();
        let idx = |x: i32, y: i32| (y * w + x) as usize;
        m[idx(from.0, from.1)] = ((limit + 1) * STEP * 2) as i16;
        let mut layer = vec![from];
        for _ in 0..limit + 1 {
            let mut next = Vec::new();
            for &(x, y) in &layer {
                let v = m[idx(x, y)] as i32 - STEP * 2;
                if v <= 0 {
                    continue;
                }
                let mut fill = |c: (i32, i32), v: i32| {
                    if m[idx(c.0, c.1)] == 0 {
                        m[idx(c.0, c.1)] = v as i16;
                        next.push(c);
                    }
                };
                if x < w - 1 {
                    fill((x + 1, y), v);
                }
                if y > 0 {
                    fill((x, y - 1), v);
                }
                if x > 0 {
                    fill((x - 1, y), v);
                }
                if y < h - 1 {
                    fill((x, y + 1), v);
                }
                let v = v - STEP;
                if v <= 0 {
                    continue;
                }
                if x < w - 1 && y > 0 {
                    fill((x + 1, y - 1), v);
                }
                if x > 0 && y > 0 {
                    fill((x - 1, y - 1), v);
                }
                if x > 0 && y < h - 1 {
                    fill((x - 1, y + 1), v);
                }
                if x < w - 1 && y < h - 1 {
                    fill((x + 1, y + 1), v);
                }
            }
            if next.is_empty() {
                break;
            }
            layer = next;
        }
        Some(m)
    }
}

/// The eight directions: 0 east, counter-clockwise (north is up).
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

    /// A goal it cannot reach gives way to the nearest cell it can
    /// (0x55ba50 with a tolerance).
    #[test]
    fn unreachable_goal_gives_way_to_the_nearest() {
        let nav = NavGrid::from_rows(&[
            "....#..", //
            "....#..", //
            "....#..", //
        ]);
        assert!(nav.find_path((0, 1), (6, 1), false).is_none());
        let path = nav
            .find_path_near((0, 1), (6, 1), false, |_| false, 0)
            .unwrap();
        assert_eq!(path.last().map(|c| c.0), Some(3));
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

    /// The wavefront 0x5459d0 with step 3 and limit 5000: the start
    /// 30006, a straight step 6 less, a diagonal 9 less, land -1, water
    /// not reached 0.
    #[test]
    fn wavefront_counts_down_from_the_start() {
        let nav = NavGrid::from_rows(&[
            "...#.", //
            ".#.#.", //
            "...#.", //
        ]);
        let w = nav.wavefront((0, 0), false, 5000).unwrap();
        let at = |x: usize, y: usize| w[y * 5 + x];
        assert_eq!(at(0, 0), 30006);
        assert_eq!((at(1, 0), at(0, 1)), (30000, 30000));
        assert_eq!((at(2, 0), at(0, 2)), (29994, 29994));
        assert_eq!((at(2, 1), at(1, 2)), (29991, 29991));
        assert_eq!(at(2, 2), 29985);
        assert_eq!((at(1, 1), at(3, 0)), (-1, -1));
        assert_eq!(at(4, 1), 0);
        assert!(nav.wavefront((5, 0), false, 5000).is_none());
    }
}
