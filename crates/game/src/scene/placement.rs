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
pub(super) fn distance(dx: i32, dy: i32) -> i32 {
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

/// A set of cells.
pub(super) type CellSet = HashSet<(i32, i32)>;

impl Scene {
    /// The cell and facing for building `class` of settlement `site`
    /// (0x42d9c0): forts (flag 0x20), ports, shipyards and docks (flag 2) on
    /// the shore, the rest (flag 1) inland; the best scored, as the
    /// original's sort leaves it first.
    /// The cells with something on their third place (`+0x1c`): a
    /// treasure lying there, a reef, a whirlpool, a flock, a volcano's own
    /// cell; and a volcano's four cells (layer-6 type 4, 0x445e20).
    pub(super) fn blocked_cells(&self) -> (CellSet, CellSet) {
        let t = TILE_SIZE as f32;
        let (mut third, mut volcano) = (HashSet::new(), HashSet::new());
        for o in &self.objects {
            let f = |k| o.record.field(k).unwrap_or(0) as i32;
            match o.class {
                ObjectClass::Treasure => {
                    third.insert(((o.x / t).floor() as i32, (o.y / t).floor() as i32));
                }
                ObjectClass::Reef(_) | ObjectClass::Whirlpool => {
                    third.insert((f(5), f(6)));
                }
                ObjectClass::Sheep => {
                    third.insert((f(4), f(5)));
                }
                ObjectClass::Lava => {
                    third.insert((f(6), f(7)));
                    for k in 0..4 {
                        volcano.insert((f(6) + k % 2, f(7) + k / 2));
                    }
                }
                _ => {}
            }
        }
        (third, volcano)
    }

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
        // Classify every cell: a volcano's ground as unbuildable, a cell
        // with something on its third place closed but passable (0x40cca0).
        let (third, volcano) = self.blocked_cells();
        let mut code = vec![0u8; (w * h) as usize];
        let mut bytes = vec![0xffu8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let k = at(x, y);
                if !buildable(x, y) || volcano.contains(&(x, y)) {
                    if code[k] == 0 {
                        code[k] = BLOCKED;
                    }
                    continue;
                }
                bytes[k] = shape(x, y);
                if third.contains(&(x, y)) {
                    if code[k] == 0 {
                        code[k] = BLOCKED;
                    }
                    continue;
                }
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
        let label = regions(&bytes, w, h);
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

impl Scene {
    /// The map's land regions (`[0x5c74f4]`, made by 0x49a950): over each
    /// cell's shape byte (0x5acf3f by its layer-0 variant) where its
    /// landscape's low nibble is 8, 0xff elsewhere, joined by 0x5597b0.
    pub(super) fn land_regions(&self) -> Vec<u32> {
        let g = &self.grid;
        let (w, h) = (g.width as i32, g.height as i32);
        let bytes: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| {
                if self.cell_types[(y * w + x) as usize] & 0xf == 8 {
                    let v = g.get(0, x as u32, y as u32).variant as usize;
                    SHAPE.get(v).copied().unwrap_or(0)
                } else {
                    0xff
                }
            })
            .collect();
        regions(&bytes, w, h)
    }
}

/// The land's regions (0x5597b0) over edge masks (0xff impassable): cells
/// joined across edges both leave open, diagonals included; 0 for none.
fn regions(bytes: &[u8], w: i32, h: i32) -> Vec<u32> {
    let at = |x: i32, y: i32| (y * w + x) as usize;
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
                    if !(0..w).contains(&nx) || !(0..h).contains(&ny) {
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
    label
}

/// Where a fort's weapons of `item` reach at range level `level`, about
/// its cell (0x447840: the tables at 0x59819c-0x598968).
pub(super) fn reach_mask(item: u8, level: u8) -> Option<&'static [&'static str]> {
    const A9: [&str; 9] = [
        ".........",
        "...###...",
        "..#####..",
        ".#######.",
        ".###.###.",
        ".#######.",
        "..#####..",
        "...###...",
        ".........",
    ];
    const B9: [&str; 9] = [
        "..#####..",
        ".#######.",
        "#########",
        "#########",
        "####.####",
        "#########",
        "#########",
        ".#######.",
        "..#####..",
    ];
    const A9H: [&str; 9] = [
        ".........",
        "...###...",
        "..#####..",
        ".##...##.",
        ".##...##.",
        ".##...##.",
        "..#####..",
        "...###...",
        ".........",
    ];
    const B9H: [&str; 9] = [
        "..#####..",
        ".#######.",
        "#########",
        "###...###",
        "###...###",
        "###...###",
        "#########",
        ".#######.",
        "..#####..",
    ];
    const A7: [&str; 7] = [
        ".......", "..###..", ".#####.", ".##.##.", ".#####.", "..###..", ".......",
    ];
    const B7: [&str; 7] = [
        "..###..", ".#####.", "#######", "###.###", "#######", ".#####.", "..###..",
    ];
    const E11: [&str; 11] = [
        "...........",
        "...........",
        "....###....",
        "...#####...",
        "..##...##..",
        "..##...##..",
        "..##...##..",
        "...#####...",
        "....###....",
        "...........",
        "...........",
    ];
    const F11: [&str; 11] = [
        "...........",
        "...#####...",
        "..#######..",
        ".#########.",
        ".###...###.",
        ".###...###.",
        ".###...###.",
        ".#########.",
        "..#######..",
        "...#####...",
        "...........",
    ];
    const G11: [&str; 11] = [
        "...#####...",
        "..#######..",
        ".#########.",
        "###########",
        "####...####",
        "####...####",
        "####...####",
        "###########",
        ".#########.",
        "..#######..",
        "...#####...",
    ];
    const C13: [&str; 13] = [
        ".............",
        "....#####....",
        "...#######...",
        "..#########..",
        ".####...####.",
        ".###.....###.",
        ".###.....###.",
        ".###.....###.",
        ".####...####.",
        "..#########..",
        "...#######...",
        "....#####....",
        ".............",
    ];
    const D13: [&str; 13] = [
        "....#####....",
        "...#######...",
        "..#########..",
        ".###########.",
        "#####...#####",
        "####.....####",
        "####.....####",
        "####.....####",
        "#####...#####",
        ".###########.",
        "..#########..",
        "...#######...",
        "....#####....",
    ];
    let top = level >= 2;
    Some(match item {
        0 | 4 => {
            if top {
                &B9
            } else {
                &A9
            }
        }
        1 | 2 => {
            if top {
                &B7
            } else {
                &A7
            }
        }
        3 => {
            if level >= 1 {
                &B9
            } else {
                &A9
            }
        }
        5 => {
            if top {
                &B9H
            } else {
                &A9H
            }
        }
        6 => {
            if top {
                &D13
            } else {
                &C13
            }
        }
        7 => match level {
            0 => &E11,
            1 => &F11,
            _ => &G11,
        },
        8 => {
            if top {
                &G11
            } else {
                &F11
            }
        }
        _ => return None,
    })
}

/// A guarding fort's candidate (0x409b30's 0x24-byte record).
#[derive(Clone, Copy)]
struct FortSpot {
    x: i32,
    y: i32,
    band: i32,
    water: i32,
    cover: i32,
    town: i32,
}

impl Scene {
    /// Where a computer player's builders found a fort guarding the town
    /// `site` (`LAiFindPlaceGuardianFortress`, 0x409b30, over the whole
    /// map): a free shore cell off the town's land or more than 8 from it;
    /// the least covered by the player's forts already, then the nearest
    /// band of 5 cells, then the most sea ships reach within 6, then some
    /// nearness to the town, the least (0x409ad0).
    pub(super) fn guard_fort_place(&self, site: u32) -> Option<(i32, i32)> {
        use crate::object_info::cache;
        let s = self.object_index(site)?;
        let owner = self.objects[s].owner?;
        let (tx, ty) = self.objects[s].cell?;
        let g = &self.grid;
        let (w, h) = (g.width as i32, g.height as i32);
        let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
        let at = |x: i32, y: i32| (y * w + x) as usize;
        let water = |x: i32, y: i32| self.nav.is_water(x, y);
        let buildable = |x: i32, y: i32| {
            !water(x, y)
                && (5..g.layers.min(8)).all(|l| {
                    let t = g.get(l, x as u32, y as u32);
                    t.is_empty() || !matches!(t.tileset, 0x1a | 0x30)
                })
        };
        let shape = |x: i32, y: i32| {
            SHAPE
                .get(g.get(0, x as u32, y as u32).variant as usize)
                .copied()
                .unwrap_or(0)
        };
        let disc = |x: i32, y: i32| {
            (y - 6..=y + 6)
                .flat_map(move |cy| (x - 6..=x + 6).map(move |cx| (cx, cy)))
                .filter(move |&(cx, cy)| inside(cx, cy) && distance(cx - x, cy - y) <= 6)
        };
        let mut standing = vec![false; (w * h) as usize];
        for o in &self.objects {
            if let (ObjectClass::Building(_), Some((cx, cy))) = (o.class, o.cell) {
                let n = building_cells(o);
                for y in cy..cy + n {
                    for x in cx..cx + n {
                        if inside(x, y) {
                            standing[at(x, y)] = true;
                        }
                    }
                }
            }
        }
        // Free ground and the town's own cell may be passed; not a cell
        // with something on its third place, nor a volcano's (0x409b30).
        let (third, volcano) = self.blocked_cells();
        let mut bytes = vec![0xffu8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let open = !third.contains(&(x, y)) && !volcano.contains(&(x, y));
                if (x, y) == (tx, ty) || (buildable(x, y) && !standing[at(x, y)] && open) {
                    bytes[at(x, y)] = shape(x, y);
                }
            }
        }
        // The sea ships reach: from every ship and the berth of every
        // shipyard, dock and barbarian structure (not ports; 0x558fa0).
        let mut sea = vec![false; (w * h) as usize];
        let mut seeds = Vec::new();
        for (i, o) in self.objects.iter().enumerate() {
            if o.ship.is_some() {
                seeds.push(self.cell_of(i));
            } else if matches!(o.class, ObjectClass::Building(SHIPYARD | DOCK)) {
                seeds.extend(self.front_cell(i));
            }
        }
        for (sx, sy) in seeds {
            if sx <= 0 || sy <= 0 || !inside(sx, sy) || !water(sx, sy) || sea[at(sx, sy)] {
                continue;
            }
            sea[at(sx, sy)] = true;
            let mut stack = vec![(sx, sy)];
            while let Some((cx, cy)) = stack.pop() {
                for &(dx, dy) in &DIR {
                    let (nx, ny) = (cx + dx, cy + dy);
                    if inside(nx, ny) && water(nx, ny) && !sea[at(nx, ny)] {
                        sea[at(nx, ny)] = true;
                        stack.push((nx, ny));
                    }
                }
            }
        }
        // The player's forts' fire on each cell about them, the last fort
        // scanned writing over the rest (0x446800, 0x446710).
        let levels = self.levels_of(Some(owner));
        let flame = self.flame_of(Some(owner)) > 0.0;
        let mut fire = vec![0i32; (w * h) as usize];
        let mut forts: Vec<usize> = (0..self.objects.len())
            .filter(|&i| {
                self.objects[i].owner == Some(owner)
                    && matches!(self.objects[i].class, ObjectClass::Building(0x34..=0x36))
                    && self.objects[i].cell.is_some_and(|(x, y)| inside(x, y))
            })
            .collect();
        forts.sort_by_key(|&i| {
            let (x, y) = self.objects[i].cell.unwrap();
            (y, x)
        });
        for i in forts {
            let (fx, fy) = self.objects[i].cell.unwrap();
            let places = self.objects[i].fort_places();
            for (cx, cy) in disc(fx, fy) {
                let mut sum = 0i32;
                for item in 0..9u8 {
                    let units = &places[item as usize];
                    let Some(mask) = reach_mask(item, levels.reach_for(item)) else {
                        continue;
                    };
                    let half = mask.len() as i32 / 2;
                    let (mx, my) = (cx - fx + half, cy - fy + half);
                    let hit = !units.is_empty()
                        && (0..mask.len() as i32).contains(&mx)
                        && (0..mask.len() as i32).contains(&my)
                        && mask[my as usize].as_bytes()[mx as usize] == b'#';
                    if hit {
                        let a = cache(item, levels.arms_for(item));
                        let f: u32 = units.iter().map(|&c| c as u32 * a / 100).sum();
                        sum += if flame { f * 150 / 100 } else { f } as i32;
                    }
                }
                fire[at(cx, cy)] = sum / 100;
            }
        }
        let label = regions(&bytes, w, h);
        let home = label[at(tx, ty)];
        if home == 0 {
            return None;
        }
        // How many of the town's four cells lie within 6.
        let mut near = vec![0i32; (w * h) as usize];
        for (ux, uy) in [(tx, ty), (tx + 1, ty), (tx, ty + 1), (tx + 1, ty + 1)] {
            for (cx, cy) in disc(ux, uy) {
                near[at(cx, cy)] += 1;
            }
        }
        let mut spots = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let k = at(x, y);
                let d = distance(x - tx, y - ty);
                if label[k] == 0 || bytes[k] == 0 || (label[k] == home && d <= 8) {
                    continue;
                }
                let (mut wet, mut cover, mut town) = (0, 0, 0);
                for (cx, cy) in disc(x, y) {
                    let q = at(cx, cy);
                    wet += sea[q] as i32;
                    cover += fire[q];
                    town += near[q];
                }
                spots.push(FortSpot {
                    x,
                    y,
                    band: d / 5,
                    water: wet,
                    cover,
                    town,
                });
            }
        }
        vc6_qsort(&mut spots, |a, b| {
            if a.cover != b.cover {
                return a.cover - b.cover;
            }
            if a.band != b.band {
                return a.band - b.band;
            }
            if a.water != b.water {
                return b.water - a.water;
            }
            match (a.town, b.town) {
                (0, t) if t != 0 => 1,
                (t, 0) if t != 0 => -1,
                (p, q) => p - q,
            }
        });
        spots.first().map(|s| (s.x, s.y))
    }
}

/// The 13 x 13 disc a lair's island spreads its threat by (0x594ba0).
const DISK13: [&str; 13] = [
    ".............",
    "...#######...",
    "..#########..",
    ".###########.",
    ".###########.",
    ".###########.",
    ".###########.",
    ".###########.",
    ".###########.",
    ".###########.",
    "..#########..",
    "...#######...",
    ".............",
];

impl Scene {
    /// Where a computer player `owner` founds a settlement
    /// (`LAiFindTownPlace`, `_L_Ai_tn.cpp`): `keep`, a site chosen before,
    /// while it still serves (0x40ca90 with 0x42e1e0); else the best over
    /// the whole map (0x40c5f0), among those within
    /// `ceil(sqrt(cells / players))` of `near` when any is (0x40cb60).
    /// A site is the top-left of 2 x 2 free inland cells beside the shore,
    /// on land of 9 inland and 2 shore cells at least, scored by what lies
    /// in the 10 x 10 cells about it: water with fish and amber draws,
    /// others' towns, forts and ships, its own towns, monsters, nests,
    /// castles and lairs repel.
    pub(super) fn town_site(
        &self,
        owner: (u32, u32),
        keep: Option<(i32, i32)>,
        near: Option<(i32, i32)>,
    ) -> Option<(i32, i32)> {
        let g = &self.grid;
        let (w, h) = (g.width as i32, g.height as i32);
        let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
        let at = |x: i32, y: i32| (y * w + x) as usize;
        let water = |x: i32, y: i32| self.nav.is_water(x, y);
        let buildable = |x: i32, y: i32| {
            !water(x, y)
                && (5..g.layers.min(8)).all(|l| {
                    let t = g.get(l, x as u32, y as u32);
                    t.is_empty() || !matches!(t.tileset, 0x1a | 0x30)
                })
        };
        let shape = |x: i32, y: i32| {
            SHAPE
                .get(g.get(0, x as u32, y as u32).variant as usize)
                .copied()
                .unwrap_or(0)
        };
        // What stands on each cell.
        let mut standing: Vec<Option<usize>> = vec![None; (w * h) as usize];
        for (i, o) in self.objects.iter().enumerate() {
            let fixed = matches!(
                o.class,
                ObjectClass::Building(_)
                    | ObjectClass::Barbarian(_)
                    | ObjectClass::Castle(_)
                    | ObjectClass::HarpyNest
                    | ObjectClass::CreatureSpawn
                    | ObjectClass::HeroHouse
            );
            if let (true, Some((cx, cy))) = (fixed, o.cell) {
                let n = building_cells(o);
                for y in cy..cy + n {
                    for x in cx..cx + n {
                        if inside(x, y) {
                            standing[at(x, y)] = Some(i);
                        }
                    }
                }
            }
        }
        // A cell with something on its third place, or a volcano's, is
        // closed (0x40c5f0).
        let (third, volcano) = self.blocked_cells();
        if let Some((kx, ky)) = keep.filter(|&(x, y)| x >= 0 && y >= 0) {
            // 0x42e1e0: still 2 x 2 free inland, with shore about it (its
            // third place tested on the first cell only, a volcano's
            // ground on all four).
            let ok = |x: i32, y: i32| {
                inside(x, y)
                    && buildable(x, y)
                    && standing[at(x, y)].is_none()
                    && shape(x, y) == 0
                    && !volcano.contains(&(x, y))
            };
            let block = (0..4).all(|k| ok(kx + k % 2, ky + k / 2)) && !third.contains(&(kx, ky));
            let shore = (ky - 1..=ky + 2).any(|y| {
                (kx - 1..=kx + 2).any(|x| {
                    let ring = x == kx - 1 || x == kx + 2 || y == ky - 1 || y == ky + 2;
                    ring && inside(x, y) && !water(x, y) && shape(x, y) != 0
                })
            });
            if block && shore {
                return Some((kx, ky));
            }
        }
        let mut bytes = vec![0xffu8; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let open = !third.contains(&(x, y)) && !volcano.contains(&(x, y));
                if buildable(x, y) && standing[at(x, y)].is_none() && open {
                    bytes[at(x, y)] = shape(x, y);
                }
            }
        }
        let label = regions(&bytes, w, h);
        let interior = |x: i32, y: i32| label[at(x, y)] != 0 && bytes[at(x, y)] == 0;
        let shore_cell = |x: i32, y: i32| label[at(x, y)] != 0 && bytes[at(x, y)] != 0;
        // Candidates, and the influences of the cells that are not.
        let mut sites: Vec<Spot> = Vec::new();
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let site = (0..4).all(|q| interior(x + q % 2, y + q / 2))
                    && (y - 1..=y + 2)
                        .any(|ry| (x - 1..=x + 2).any(|rx| inside(rx, ry) && shore_cell(rx, ry)));
                if site {
                    sites.push(Spot {
                        x,
                        y,
                        dir: 0,
                        score: 0,
                    });
                }
            }
        }
        let is_site: HashSet<(i32, i32)> = sites.iter().map(|s| (s.x, s.y)).collect();
        let wt = self.influence_map(owner, &SITE_WEIGHTS, (0, 0, w - 1, h - 1), 0, &|x, y| {
            is_site.contains(&(x, y))
        });
        // Regions big enough: 9 inland and 2 shore cells.
        let mut counts: std::collections::HashMap<u32, (i32, i32)> = Default::default();
        for y in 0..h {
            for x in 0..w {
                let l = label[at(x, y)];
                if l != 0 {
                    let c = counts.entry(l).or_default();
                    if bytes[at(x, y)] == 0 {
                        c.0 += 1
                    } else {
                        c.1 += 1
                    }
                }
            }
        }
        sites.retain(|s| {
            counts
                .get(&label[at(s.x, s.y)])
                .is_some_and(|&(i, sh)| i >= 9 && sh >= 2)
        });
        for s in &mut sites {
            let xs = (s.x - 4).max(0).min(w - 10).max(0);
            let ys = (s.y - 4).max(0).min(h - 10).max(0);
            s.score = (ys..(ys + 10).min(h))
                .flat_map(|y| (xs..(xs + 10).min(w)).map(move |x| (x, y)))
                .map(|(x, y)| wt[at(x, y)])
                .sum();
        }
        vc6_qsort(&mut sites, |a, b| b.score - a.score);
        if let Some((px, py)) = near {
            let players = self
                .objects
                .iter()
                .filter_map(|o| o.owner)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                .max(1) as i32;
            let r = (((w * h) / players) as f64).sqrt().ceil() as i32;
            if let Some(s) = sites.iter().find(|s| distance(s.x - px, s.y - py) <= r) {
                return Some((s.x, s.y));
            }
        }
        sites.first().map(|s| (s.x, s.y))
    }

    /// The weight map of what lies about (0x40b050 collecting over the
    /// cells of `scan` but those `skip`ped, 0x40bbf0 painting) by `table`
    /// (0x5945c0 for settlements' sites, 0x594470 for waiting zones): a
    /// (weight, radius, falloff) for each kind, a weight of 0 left out.
    /// `nest_left`: the painted area's left column, which a nest's reach
    /// paints only to the right of (0x40b920).
    pub(super) fn influence_map(
        &self,
        owner: (u32, u32),
        table: &Influence,
        scan: (i32, i32, i32, i32),
        nest_left: i32,
        skip: &dyn Fn(i32, i32) -> bool,
    ) -> Vec<i32> {
        let g = &self.grid;
        let (w, h) = (g.width as i32, g.height as i32);
        let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
        let at = |x: i32, y: i32| (y * w + x) as usize;
        let water = |x: i32, y: i32| self.nav.is_water(x, y);
        let shape = |x: i32, y: i32| {
            SHAPE
                .get(g.get(0, x as u32, y as u32).variant as usize)
                .copied()
                .unwrap_or(0)
        };
        let mut wt = vec![0i32; (w * h) as usize];
        let paint = |wt: &mut Vec<i32>, x: i32, y: i32, (weight, r, fall): (i32, i32, i32)| {
            if weight == 0 {
                return;
            }
            for cy in y - r..=y + r {
                for cx in x - r..=x + r {
                    let (dx, dy) = (cx - x, cy - y);
                    if inside(cx, cy) && dx * dx + dy * dy <= r * r {
                        let d = if fall == 0 { 0 } else { distance(dx, dy) };
                        wt[at(cx, cy)] += weight - fall * d;
                    }
                }
            }
        };
        let mut standing: Vec<Option<usize>> = vec![None; (w * h) as usize];
        let mut lava: Vec<Option<usize>> = vec![None; (w * h) as usize];
        for (i, o) in self.objects.iter().enumerate() {
            let fixed = matches!(
                o.class,
                ObjectClass::Building(_)
                    | ObjectClass::Barbarian(_)
                    | ObjectClass::Castle(_)
                    | ObjectClass::HarpyNest
                    | ObjectClass::CreatureSpawn
                    | ObjectClass::HeroHouse
            );
            if let (true, Some((cx, cy))) = (fixed, o.cell) {
                let n = building_cells(o);
                for y in cy..cy + n {
                    for x in cx..cx + n {
                        if inside(x, y) {
                            standing[at(x, y)] = Some(i);
                        }
                    }
                }
            }
            // A volcano is found at its own cell only, its +0x1c
            // (0x445e20), where 0x40b050 collects it.
            if o.class == ObjectClass::Lava {
                let f = |k| o.record.field(k).unwrap_or(0) as i32;
                if inside(f(6), f(7)) {
                    lava[at(f(6), f(7))] = Some(i);
                }
            }
        }
        // Water reached from a cell (0x558fa0 over the ships' grid).
        let mut sea_label = vec![0u32; (w * h) as usize];
        let mut seas = 0;
        for y in 0..h {
            for x in 0..w {
                if !water(x, y) || sea_label[at(x, y)] != 0 {
                    continue;
                }
                seas += 1;
                sea_label[at(x, y)] = seas;
                let mut stack = vec![(x, y)];
                while let Some((cx, cy)) = stack.pop() {
                    for &(dx, dy) in &DIR {
                        let (nx, ny) = (cx + dx, cy + dy);
                        if inside(nx, ny) && water(nx, ny) && sea_label[at(nx, ny)] == 0 {
                            sea_label[at(nx, ny)] = seas;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
        }
        let reached_from = |x: i32, y: i32| -> Vec<u32> {
            if water(x, y) {
                return vec![sea_label[at(x, y)]];
            }
            DIR.iter()
                .map(|&(dx, dy)| (x + dx, y + dy))
                .filter(|&(cx, cy)| inside(cx, cy))
                .map(|(cx, cy)| sea_label[at(cx, cy)])
                .filter(|&l| l != 0)
                .collect()
        };
        let mut movers: std::collections::HashMap<(i32, i32), Vec<usize>> = Default::default();
        for (i, o) in self.objects.iter().enumerate() {
            if matches!(o.class, ObjectClass::Ship | ObjectClass::Monster(_)) {
                movers.entry(self.cell_of_obj(o)).or_default().push(i);
            }
        }
        // Deposits by cell (amber 1 cell; fish zones; a leviathan the
        // cell whose slot `+0x10` holds it, 0x40b710's res slot).
        let mut deposit: Vec<Option<(i32, i32, i32)>> = vec![None; (w * h) as usize];
        for o in &self.objects {
            if o.class == ObjectClass::WhaleZone {
                if let Some((x, y)) = self.leviathan_cell(o.id).filter(|&(x, y)| inside(x, y)) {
                    deposit[at(x, y)] = Some(table.whale);
                }
                continue;
            }
            let f = &o.record.fields;
            let (wrf, x0, y0, dw, dh) = match o.class {
                ObjectClass::Amber => (table.amber, f.get(6), f.get(7), 1, 1),
                ObjectClass::FishZone => (
                    table.fish,
                    f.get(6),
                    f.get(7),
                    f.get(8).copied().unwrap_or(1) as i32,
                    f.get(9).copied().unwrap_or(1) as i32,
                ),
                _ => continue,
            };
            let (Some(&x0), Some(&y0)) = (x0, y0) else {
                continue;
            };
            for y in y0 as i32..y0 as i32 + dh.max(1) {
                for x in x0 as i32..x0 as i32 + dw.max(1) {
                    if inside(x, y) {
                        deposit[at(x, y)] = Some(wrf);
                    }
                }
            }
        }
        // The cells of each lair's island and its spread (0x40b5a0).
        let land_bytes: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| if water(x, y) { 0xff } else { shape(x, y) })
            .collect();
        let islands = regions(&land_bytes, w, h);
        let (sx, sy, sw, sh) = scan;
        for y in sy.max(0)..(sy + sh).min(h) {
            for x in sx.max(0)..(sx + sw).min(w) {
                if skip(x, y) {
                    continue;
                }
                let k = at(x, y);
                if let Some(i) = standing[k] {
                    let o = &self.objects[i];
                    let anchor = o.cell == Some((x, y));
                    match (o.class, o.owner) {
                        (ObjectClass::Building(c), Some(ow)) if ow != owner => {
                            if matches!(c, SETTLEMENT | TOWN) {
                                if anchor {
                                    paint(&mut wt, x, y, table.foe_town);
                                }
                            } else if (0x34..=0x36).contains(&c) {
                                paint(&mut wt, x, y, table.foe_fort);
                            } else {
                                paint(&mut wt, x, y, table.foe_building);
                            }
                        }
                        (ObjectClass::Barbarian(_), _) => paint(&mut wt, x, y, table.foe_fort),
                        (ObjectClass::Building(_), Some(_)) if anchor => {
                            paint(&mut wt, x, y, table.own)
                        }
                        (ObjectClass::Castle(_), _) => paint(&mut wt, x, y, table.castle),
                        (ObjectClass::HarpyNest, _) if table.nest.0 != 0 => {
                            // Over its reach, record fields 7 and 8, one more
                            // (0x40b920, nest +0x200, +0x204).
                            let rx = o.record.field(7).unwrap_or(1) as i32 + 1;
                            let ry = o.record.field(8).unwrap_or(1) as i32 + 1;
                            for cy in y - ry..=y + ry {
                                for cx in x - rx..=x + rx {
                                    if inside(cx, cy) && cx > nest_left {
                                        wt[at(cx, cy)] +=
                                            table.nest.0 - table.nest.2 * distance(cx - x, cy - y);
                                    }
                                }
                            }
                        }
                        (ObjectClass::CreatureSpawn, _) if table.lair != 0 => {
                            let f = self.lair_footprint(x, y, &islands);
                            for (v, fv) in wt.iter_mut().zip(&f) {
                                *v += table.lair * fv;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                if let Some((weight, r, fall)) = deposit[k] {
                    if weight != 0 {
                        let reach = reached_from(x, y);
                        for cy in y - r..=y + r {
                            for cx in x - r..=x + r {
                                if !inside(cx, cy) {
                                    continue;
                                }
                                let d = distance(cx - x, cy - y);
                                let wet =
                                    (cx, cy) == (x, y) || reach.contains(&sea_label[at(cx, cy)]);
                                if d <= r && wet && (water(cx, cy) || (cx, cy) == (x, y)) {
                                    wt[at(cx, cy)] += weight - fall * d;
                                }
                            }
                        }
                    }
                    continue;
                }
                // Others' ships, the monsters, and a volcano's lava: its
                // strength times the weight over the square of its radius.
                for &i in movers.get(&(x, y)).into_iter().flatten() {
                    let o = &self.objects[i];
                    match (o.class, o.owner) {
                        (ObjectClass::Ship, Some(ow)) if ow != owner => {
                            paint(&mut wt, x, y, table.foe_ship)
                        }
                        (ObjectClass::Monster(0x1fa | 0x1fd), _) => {
                            paint(&mut wt, x, y, table.medusa)
                        }
                        (ObjectClass::Monster(0x1fb), _) => {
                            // Over its waters: its fields 11 and 12 about it,
                            // or the whole map (0x492403, 0x4936c0).
                            let f = |n| o.record.field(n).unwrap_or(0) as i32;
                            let (ax, ay) = (f(11), f(12));
                            let (x0, x1, y0, y1) = if ax > 0 && ay > 0 {
                                (
                                    (x - ax).max(0),
                                    (x + ax).min(w - 1),
                                    (y - ay).max(0),
                                    (y + ay).min(h - 1),
                                )
                            } else {
                                (0, w - 1, 0, h - 1)
                            };
                            for cy in y0..=y1 {
                                for cx in x0..=x1 {
                                    wt[at(cx, cy)] += table.medusa.0;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(i) = lava[k] {
                    let f = |n| self.objects[i].record.field(n).unwrap_or(0) as i32;
                    let (cx, cy, strength, r) = (f(6), f(7), f(8), f(9));
                    if strength != 0 && table.lava != 0 {
                        for yy in cy - r..=cy + r {
                            for xx in cx - r..=cx + r {
                                if inside(xx, yy) {
                                    wt[at(xx, yy)] += strength * table.lava;
                                }
                            }
                        }
                    }
                }
            }
        }
        wt
    }

    /// How strongly a lair at `(lx, ly)` threatens each cell (0x40b5a0,
    /// 0x4080e0): over the box of its land region (`islands`), a walk of
    /// half cells (0x42e720: a cell not land, of type 1 on layer 5 or with
    /// anything on layers 6-7 shut; else the half cells along its shore
    /// edges, 0x5acf13) flooded from the lair's cell, which itself does not
    /// count; each cell reached counts 1, and 1 more for every cell reached
    /// whose 13 x 13 disc (0x594ba0) holds it.
    fn lair_footprint(&self, lx: i32, ly: i32, islands: &[u32]) -> Vec<i32> {
        const EDGES: [u8; 45] = [
            0, 0, 9, 9, 9, 9, 9, 3, 3, 3, 3, 3, 12, 12, 12, 12, 12, 6, 6, 6, 6, 6, 0, 0, 0, 0, 0,
            0, 0, 0, 8, 8, 2, 2, 1, 1, 1, 4, 4, 4, 4, 4, 0, 0, 0,
        ];
        let g = &self.grid;
        let (w, h) = (g.width as i32, g.height as i32);
        let at = |x: i32, y: i32| (y * w + x) as usize;
        let mut f = vec![0i32; (w * h) as usize];
        let isle = islands[at(lx, ly)];
        if isle == 0 {
            return f;
        }
        let (mut bx0, mut by0, mut bx1, mut by1) = (lx, ly, lx, ly);
        for y in 0..h {
            for x in 0..w {
                if islands[at(x, y)] == isle {
                    bx0 = bx0.min(x);
                    by0 = by0.min(y);
                    bx1 = bx1.max(x);
                    by1 = by1.max(y);
                }
            }
        }
        let (sw, sh) = (2 * (bx1 - bx0 + 1), 2 * (by1 - by0 + 1));
        let sub = |x: i32, y: i32| (y * sw + x) as usize;
        let mut shut = vec![false; (sw * sh) as usize];
        for y in by0..=by1 {
            for x in bx0..=bx1 {
                let types = self.cell_types[at(x, y)];
                let (sx, sy) = (2 * (x - bx0), 2 * (y - by0));
                let closed = types & 0xf != 8
                    || types & 0x00f0_0000 == 0x0010_0000
                    || types & 0xff00_0000 != 0;
                let e = EDGES
                    .get(g.get(0, x as u32, y as u32).variant as usize)
                    .copied()
                    .unwrap_or(0);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let edge = (dy == 0 && e & 1 != 0)
                        || (dx == 1 && e & 2 != 0)
                        || (dy == 1 && e & 4 != 0)
                        || (dx == 0 && e & 8 != 0);
                    shut[sub(sx + dx, sy + dy)] = closed || edge;
                }
            }
        }
        let (ox, oy) = (2 * (lx - bx0), 2 * (ly - by0));
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            shut[sub(ox + dx, oy + dy)] = false;
        }
        let mut reached = vec![false; (sw * sh) as usize];
        reached[sub(ox, oy)] = true;
        let mut stack = vec![(ox, oy)];
        while let Some((cx, cy)) = stack.pop() {
            for &(dx, dy) in &DIR {
                let (nx, ny) = (cx + dx, cy + dy);
                if nx < 0 || ny < 0 || nx >= sw || ny >= sh {
                    continue;
                }
                let k = sub(nx, ny);
                if !shut[k] && !reached[k] {
                    reached[k] = true;
                    stack.push((nx, ny));
                }
            }
        }
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            reached[sub(ox + dx, oy + dy)] = false;
        }
        let mut cells = Vec::new();
        for y in by0..=by1 {
            for x in bx0..=bx1 {
                let (sx, sy) = (2 * (x - bx0), 2 * (y - by0));
                let any = [(0, 0), (1, 0), (0, 1), (1, 1)]
                    .iter()
                    .any(|&(dx, dy)| reached[sub(sx + dx, sy + dy)]);
                if any {
                    f[at(x, y)] += 1;
                    cells.push((x, y));
                }
            }
        }
        for (cx, cy) in cells {
            for (dy, row) in DISK13.iter().enumerate() {
                for (dx, c) in row.bytes().enumerate() {
                    let (px, py) = (cx + dx as i32 - 6, cy + dy as i32 - 6);
                    if c == b'#' && px >= 0 && py >= 0 && px < w && py < h {
                        f[at(px, py)] += 1;
                    }
                }
            }
        }
        f
    }

    fn cell_of_obj(&self, o: &MapObject) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        o.cell
            .unwrap_or(((o.x / t).floor() as i32, (o.y / t).floor() as i32))
    }
}

/// The weights of what lies about, each (weight, radius, falloff) but
/// the lair's, the nest's (weight and falloff) and the lava's weights.
pub(super) struct Influence {
    pub own: (i32, i32, i32),
    pub foe_town: (i32, i32, i32),
    pub foe_fort: (i32, i32, i32),
    pub foe_building: (i32, i32, i32),
    pub foe_ship: (i32, i32, i32),
    pub amber: (i32, i32, i32),
    pub fish: (i32, i32, i32),
    pub whale: (i32, i32, i32),
    pub lair: i32,
    pub nest: (i32, i32, i32),
    pub castle: (i32, i32, i32),
    pub medusa: (i32, i32, i32),
    pub lava: i32,
}

/// For settlements' sites (0x5945c0).
pub(super) const SITE_WEIGHTS: Influence = Influence {
    own: (-5, 5, -1),
    foe_town: (-10, 10, 0),
    foe_fort: (-10, 10, 0),
    foe_building: (-10, 10, 0),
    foe_ship: (-5, 5, 0),
    amber: (5, 5, 1),
    fish: (10, 10, 1),
    whale: (10, 10, 1),
    lair: -30,
    nest: (-20, 0, -1),
    castle: (-20, 0, -1),
    medusa: (-20, 5, -1),
    lava: -10,
};

/// For the computer's ranking of a flotilla's deposits (0x594520).
pub(super) const DEPOSIT_WEIGHTS: Influence = Influence {
    own: (0, 5, 1),
    foe_town: (-10, 10, 0),
    foe_fort: (-10, 10, 0),
    foe_building: (-10, 10, 0),
    foe_ship: (-5, 5, 0),
    amber: (0, 0, 0),
    fish: (0, 0, 0),
    whale: (0, 0, 0),
    lair: -30,
    nest: (-20, 0, -1),
    castle: (-20, 0, -1),
    medusa: (-20, 5, -1),
    lava: -1,
};

/// For waiting zones (0x594470).
pub(super) const ZONE_WEIGHTS: Influence = Influence {
    own: (20, 10, 0),
    foe_town: (-20, 10, 0),
    foe_fort: (-20, 10, 0),
    foe_building: (0, 10, 0),
    foe_ship: (-20, 10, 0),
    amber: (0, 1, 1),
    fish: (0, 1, 1),
    whale: (0, 1, 1),
    lair: -10,
    nest: (-10, 0, -1),
    castle: (-10, 0, -1),
    medusa: (-10, 5, -1),
    lava: -1,
};

/// The C runtime's `qsort` (0x58791f, Visual C++ 6): not stable, so ties
/// fall where it leaves them.
pub(super) fn vc6_qsort<T: Copy>(v: &mut [T], cmp: impl Fn(&T, &T) -> i32) {
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
