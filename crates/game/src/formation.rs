//! Where a fleet's ships go when the fleet is ordered to a cell: the
//! navy's formation planner (0x4cc970 -> 0x5614e0 -> 0x560100), with the
//! wavefront 0x557910 and the fill 0x573350 it runs over the ships' grids
//! by draught (0 open, -1 closed: 0x5c9398 deep, 0x5c93bc shallow).

/// The eight directions of table 0x5c4f4c: 0 east, counter-clockwise.
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

/// The value of a flood cell as the wavefront reads it (`& 0x3fff`).
fn val(m: &[i16], i: usize) -> i32 {
    (m[i] as u16 & 0x3fff) as i32
}

/// One run of cells the wavefront advances (16 bytes in the original):
/// its side `dir` (0 a column moving east, its cells going north; 2 a row
/// moving north, going west; 4 a column moving west, going south; 6 a row
/// moving south, going east), its length and first cell.
#[derive(Clone, Copy)]
struct Span {
    dir: u8,
    len: i32,
    x: i32,
    y: i32,
}

/// The wavefront 0x557910 over `m` (`w` wide), as the planner calls it:
/// the whole map, from `(sx, sy)`, no goal, cells still 0 open to it. The
/// start gets 1; layer by layer each run steps one cell on, a cell taking
/// its neighbour behind + 2 (+ 3 at a run's diagonal ends) while that is
/// at most `2 * layers + 2`; the runs' ends turn the corners after (the
/// second pass). A run splits where it meets a filled or closed cell. The
/// next layer's runs are kept in order of the least value each wrote
/// (0x571770). Returns the layers run (at most `layers`): 0 when the
/// start's first layer reached nothing.
pub fn flood(m: &mut [i16], w: i32, h: i32, (sx, sy): (i32, i32), layers: i32) -> i32 {
    let cap = layers * 2 + 2;
    let (x0, y0, x1, y1) = (0, 0, w - 1, h - 1);
    let at = |x: i32, y: i32| (y * w + x) as usize;
    m[at(sx, sy)] = 1;
    let mut cur: Vec<Span> = [0, 2, 4, 6]
        .into_iter()
        .map(|dir| Span {
            dir,
            len: 1,
            x: sx,
            y: sy,
        })
        .collect();
    let mut n = 0;
    while n < layers {
        let mut next: Vec<(Span, i32)> = Vec::new();
        let mut low = 0x7fff;
        let push = |next: &mut Vec<(Span, i32)>, low: &mut i32, s: Span| {
            let mut k = next.len();
            while k > 0 && next[k - 1].1 > *low {
                k -= 1;
            }
            next.insert(k, (s, *low));
            *low = 0x7fff;
        };
        // The runs step on.
        for s in &cur {
            let (x, y, dir) = (s.x, s.y, s.dir);
            // Step along the run, offset of the cell behind, and the run's
            // first cell (the diagonal ahead of its start) when open.
            let (dx, dy, step, behind, nx, ny, open, start);
            match dir {
                0 => {
                    if x >= x1 {
                        continue;
                    }
                    (dx, dy, step, behind) = (0, -1, -w, -1);
                    (nx, ny) = (x + 1, y + 1);
                    open = ny <= y1;
                    start = (nx, y);
                }
                2 => {
                    if y <= y0 {
                        continue;
                    }
                    (dx, dy, step, behind) = (-1, 0, -1, w);
                    (nx, ny) = (x + 1, y - 1);
                    open = nx <= x1;
                    start = (x, ny);
                }
                4 => {
                    if x <= x0 {
                        continue;
                    }
                    (dx, dy, step, behind) = (0, 1, w, 1);
                    (nx, ny) = (x - 1, y - 1);
                    open = ny >= y0;
                    start = (nx, y);
                }
                _ => {
                    if y >= y1 {
                        continue;
                    }
                    (dx, dy, step, behind) = (1, 0, 1, -w);
                    (nx, ny) = (x - 1, y + 1);
                    open = nx >= x0;
                    start = (x, ny);
                }
            }
            let (mut sx_, mut sy_) = start;
            let mut k = 0;
            let mut p = at(x, y) as i32 + (nx - x) + (ny - y) * w;
            if open {
                let v = val(m, at(x, y)) + 3;
                if v <= cap && m[p as usize] == 0 {
                    m[p as usize] = v as i16;
                    low = v;
                    (sx_, sy_) = (nx, ny);
                    k = 1;
                }
            }
            let (mut cx, mut cy) = (nx + dx, ny + dy);
            p += step;
            let mut left = s.len;
            while left != 0 {
                let mut ended = false;
                while m[p as usize] == 0 && val(m, (p + behind) as usize) + 2 <= cap {
                    let v = val(m, (p + behind) as usize) + 2;
                    low = low.min(v);
                    m[p as usize] = v as i16;
                    p += step;
                    left -= 1;
                    k += 1;
                    (cx, cy) = (cx + dx, cy + dy);
                    if left == 0 {
                        ended = true;
                        break;
                    }
                }
                if ended {
                    break;
                }
                if k != 0 {
                    push(
                        &mut next,
                        &mut low,
                        Span {
                            dir,
                            len: k,
                            x: sx_,
                            y: sy_,
                        },
                    );
                }
                loop {
                    if val(m, (p + behind) as usize) + 2 <= cap && m[p as usize] == 0 {
                        break;
                    }
                    p += step;
                    left -= 1;
                    (cx, cy) = (cx + dx, cy + dy);
                    if left == 0 {
                        break;
                    }
                }
                (sx_, sy_) = (cx, cy);
                k = 0;
            }
            // The run's far end, diagonal from its last cell.
            if (x0..=x1).contains(&cx) && (y0..=y1).contains(&cy) {
                let v = val(m, (p + behind - step) as usize) + 3;
                if v <= cap && m[p as usize] == 0 {
                    low = low.min(v);
                    m[p as usize] = v as i16;
                    k += 1;
                }
            }
            if k != 0 {
                push(
                    &mut next,
                    &mut low,
                    Span {
                        dir,
                        len: k,
                        x: sx_,
                        y: sy_,
                    },
                );
            }
        }
        // The runs' ends turn: at its start a run's diagonal and the cell
        // beside it, the run turned a quarter back (dir + 6); at its end
        // the cell past it and the diagonal, turned a quarter on (dir + 2).
        for s in &cur {
            let (x, y, len) = (s.x, s.y, s.len);
            // (corner cell, its open test, the side cell, the source)
            let (back_dir, on_dir) = ((s.dir + 6) % 8, (s.dir + 2) % 8);
            // Start end.
            let first = match s.dir {
                0 => (y < y1).then_some(((x - 1, y + 1), x > x0, (x, y + 1), at(x, y))),
                2 => (x < x1).then_some(((x + 1, y + 1), y < y1, (x + 1, y), at(x, y))),
                4 => (y > y0).then_some(((x + 1, y - 1), x < x1, (x, y - 1), at(x, y))),
                _ => (x > x0).then_some(((x - 1, y - 1), y > y0, (x - 1, y), at(x, y))),
            };
            if let Some((c, inside, side, src)) = first {
                let u = val(m, src);
                let ci = (c.1 * w + c.0) as usize;
                let (mut wrote, mut k, mut start) = (0, 0, (x, y));
                // The run's start keeps the row or column it is on.
                match s.dir {
                    0 => start = (x, y + 1),
                    2 => start = (x + 1, y),
                    4 => start = (x, y - 1),
                    _ => start.0 = x - 1,
                }
                if inside && m[ci] == 0 && u + 3 <= cap {
                    m[ci] = (u + 3) as i16;
                    wrote = 1;
                    k = 1;
                    start = c;
                }
                if u + 2 <= cap {
                    let si = (side.1 * w + side.0) as usize;
                    if m[si] == 0 {
                        m[si] = (u + 2) as i16;
                        k += 1;
                        wrote = k;
                    }
                }
                if wrote != 0 {
                    push(
                        &mut next,
                        &mut low,
                        Span {
                            dir: back_dir,
                            len: k,
                            x: start.0,
                            y: start.1,
                        },
                    );
                }
            }
            // Far end.
            let last = match s.dir {
                0 => (y - len >= y0).then_some(((x, y - len), (x, y - len + 1), (-1, 0), x > x0)),
                2 => (x - len >= x0).then_some(((x - len, y), (x - len + 1, y), (0, 1), y < y1)),
                4 => (y + len <= y1).then_some(((x, y + len), (x, y + len - 1), (1, 0), x < x1)),
                _ => (x + len <= x1).then_some(((x + len, y), (x + len - 1, y), (0, -1), y > y0)),
            };
            if let Some((c, src, (ox, oy), inside)) = last {
                let u = val(m, at(src.0, src.1));
                let ci = at(c.0, c.1);
                let mut k;
                let start;
                if m[ci] == 0 && u + 2 <= cap {
                    m[ci] = (u + 2) as i16;
                    k = 1;
                    start = c;
                } else {
                    k = 0;
                    start = (c.0 + ox, c.1 + oy);
                }
                let di = at(c.0 + ox, c.1 + oy);
                if inside && u + 3 <= cap && m[di] == 0 {
                    m[di] = (u + 3) as i16;
                    k += 1;
                }
                if k != 0 {
                    push(
                        &mut next,
                        &mut low,
                        Span {
                            dir: on_dir,
                            len: k,
                            x: start.0,
                            y: start.1,
                        },
                    );
                }
            }
        }
        if next.is_empty() {
            break;
        }
        cur = next.into_iter().map(|(s, _)| s).collect();
        n += 1;
    }
    n
}

/// 0x573350 with value -1: a closed goal opens (0) the closed region it
/// lies in, its cells joined side or corner. (Its runs fill the same
/// cells, in their own order; only the set filled is used.)
pub fn open_region(m: &mut [i16], w: i32, h: i32, (gx, gy): (i32, i32)) {
    let at = |x: i32, y: i32| (y * w + x) as usize;
    if m[at(gx, gy)] != -1 {
        return;
    }
    m[at(gx, gy)] = 0;
    let mut todo = vec![(gx, gy)];
    while let Some((x, y)) = todo.pop() {
        for dx in -1..=1 {
            for dy in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < w && ny < h && m[at(nx, ny)] == -1 {
                    m[at(nx, ny)] = 0;
                    todo.push((nx, ny));
                }
            }
        }
    }
}

/// The direction from `a` to `b` (0x557560): 0x7f for the same cell.
fn dir_to((ax, ay): (i32, i32), (bx, by): (i32, i32)) -> i32 {
    let (dx, dy) = ((bx - ax).abs(), (by - ay).abs());
    if dx == 0 && dy == 0 {
        return 0x7f;
    }
    let b = |c: bool| c as i32;
    match (ay <= by, bx < ax) {
        (_, true) if dy * 2 <= dx => 4,
        (_, false) if dy * 2 <= dx => 0,
        (true, true) => 5 + b(dx * 2 <= dy),
        (true, false) => 6 + b(dy < dx * 2),
        (false, true) => 2 + b(dy < dx * 2),
        (false, false) => 1 + b(dx * 2 <= dy),
    }
}

/// How far `b` lies from `a` along direction `d` (0x5574d0): twice the
/// distance for a straight direction, a third of four times the sideways
/// difference for a diagonal, rounded as the original's division by 3.
fn along((ax, ay): (i32, i32), d: i32, (bx, by): (i32, i32)) -> i32 {
    let (a, mut b) = (bx - ax, by - ay);
    let mut c = a;
    match d {
        1 | 2 => (c, b) = (-b, a),
        3 | 4 => (c, b) = (-a, -b),
        5 | 6 => (c, b) = (b, -a),
        _ => {}
    }
    if d & 1 == 0 {
        return b * 2;
    }
    let t = (c - b).wrapping_mul(4).wrapping_add(1);
    let hi = ((t as i64 * 0x5555_5555) >> 32) as i32;
    let u = hi.wrapping_sub(t);
    (u >> 1) - (u >> 31)
}

/// A slot a ship may take (the planner's 0x18-byte record): its cell,
/// sort key, ring distance from the goal, the centre flood's value there
/// and whether it lies where a ship of the fleet would, the fleet moved
/// whole.
#[derive(Clone, Copy)]
struct Slot {
    x: i32,
    y: i32,
    key: i32,
    dist: i32,
    flood: i32,
    shape: bool,
}

/// What 0x561360 and 0x560f80 read of the planner's state.
struct Ring<'a> {
    w: i32,
    h: i32,
    to: (i32, i32),
    /// Centre less the ships' corner, the box's size, its ship cells.
    origin: (i32, i32),
    size: (i32, i32),
    shape: &'a [(i32, i32)],
    centre_flood: &'a [i16],
}

/// 0x561360: slot `(x, y)` (index `i`, ring distance `d`) put in its place
/// among `slots[start..]`, keeping at most `count`: one where a ship of
/// the fleet would lie goes before another unless that one is two nearer;
/// alike, the nearer first, then the lower centre flood.
fn add_slot(r: &Ring, slots: &mut Vec<Slot>, start: usize, count: usize, x: i32, y: i32, d: i32) {
    let (rx, ry) = (r.origin.0 - r.to.0 + x, r.origin.1 - r.to.1 + y);
    let shape = rx >= 0 && rx < r.size.0 && ry >= 0 && ry < r.size.1 && r.shape.contains(&(rx, ry));
    let flood = r.centre_flood[(y * r.w + x) as usize] as i32;
    let mut pos = start;
    while pos < slots.len() {
        let e = slots[pos];
        let before = if shape == e.shape {
            d < e.dist || (d <= e.dist && flood < e.flood)
        } else if !shape {
            d < e.dist - 1
        } else {
            d <= e.dist + 1
        };
        if before {
            break;
        }
        pos += 1;
    }
    if pos < count {
        slots.truncate(count - 1);
        slots.insert(
            pos,
            Slot {
                x,
                y,
                key: 0,
                dist: d,
                flood,
                shape,
            },
        );
    }
}

/// 0x560f80 for class 0 (every ship of 0x4cc970's records): the open
/// cells of `m` (still 0) in square rings about the goal, radius 0 to 10,
/// each edge walked as the original does (the right side bottom to top,
/// the top right to left, the left top to bottom, the bottom left to
/// right; an edge past the map left out, the others cut to it), ring
/// distance `(2r + offset) / 2`; within radius 2 any, beyond only those
/// under 11.
fn ring_slots(r: &Ring, m: &[i16], slots: &mut Vec<Slot>, start: usize, count: usize) {
    let (tx, ty) = r.to;
    let w = r.w;
    let ok = |rad: i32, d: i32| (2 * rad < 5) || d < 11;
    let take = |slots: &mut Vec<Slot>, x: i32, y: i32, rad: i32, dist: i32| {
        if m[(y * w + x) as usize] == 0 && ok(rad, dist / 2) {
            add_slot(r, slots, start, count, x, y, dist / 2);
        }
    };
    for rad in 0..=10 {
        let two = 2 * rad;
        let mut left = (tx - rad).max(0);
        let mut top = (ty - rad).max(0);
        let right = (tx + rad).min(w - 1);
        let bottom = (ty + rad).min(r.h - 1);
        let yb = if rad <= bottom - ty && two != 0 {
            bottom - 1
        } else {
            bottom
        };
        let mut xs = right;
        if rad <= right - tx {
            let mut dist = two - ty + yb;
            let mut y = yb;
            while top <= y {
                take(slots, right, y, rad, dist);
                dist += if ty < y { -1 } else { 1 };
                y -= 1;
            }
            xs = right - 1;
        }
        if two == 0 {
            continue;
        }
        if rad <= ty - top {
            let mut dist = two - tx + xs;
            let mut x = xs;
            while left <= x {
                take(slots, x, top, rad, dist);
                dist += if tx < x { -1 } else { 1 };
                x -= 1;
            }
            top += 1;
        }
        if rad <= tx - left {
            let mut dist = two - top + ty;
            for y in top..=bottom {
                take(slots, left, y, rad, dist);
                dist += if y < ty { -1 } else { 1 };
            }
            left += 1;
        }
        if rad <= bottom - ty {
            let mut dist = two - left + tx;
            for x in left..=right {
                take(slots, x, bottom, rad, dist);
                dist += if x < tx { -1 } else { 1 };
            }
        }
    }
}

/// A ship as 0x4cc970 hands it to the planner: its cell and whether it is
/// of shallow draught (`+0x592 == 0`).
#[derive(Clone, Copy)]
pub struct Ship {
    pub x: i32,
    pub y: i32,
    pub shallow: bool,
}

/// Why the planner gave no cells.
#[derive(Debug, PartialEq)]
pub enum Fail {
    /// The original's -4 (a cell off the map, no water about the centre,
    /// too few slots, nothing reached).
    Refused,
    /// Not decoded: 0x55f870 found no direction from the fleet's centre
    /// toward the goal, or from the slots' middle back to the centre (all
    /// eight cells about it unreached), and goes on with a stack word it
    /// never wrote (`[esp+0x34]`, `[esp+0x24]`).
    Unsettled,
}

/// The formation planner 0x560100 (through 0x5614e0, as 0x4cc970 calls
/// it: no goal tolerance, the centre the ships' median): a cell for each
/// of `ships` about `to`, on `deep` (0x5c9398) and `shallow` (0x5c93bc),
/// `w` x `h`. Each ship keeps its place about the fleet's centre if every
/// slot chosen lies where one of the ships would (the fleet moved whole);
/// otherwise the slots are dealt out by 0x55f870.
pub fn plan(
    deep: &[i16],
    shallow: &[i16],
    (w, h): (i32, i32),
    to: (i32, i32),
    ships: &[Ship],
) -> Result<Vec<(i32, i32)>, Fail> {
    let n = ships.len();
    if n == 0 {
        return Err(Fail::Refused);
    }
    let shallows = ships.iter().filter(|s| s.shallow).count();
    let any_deep = shallows < n;
    // p1 the deep ships' grid, p2 the shallow ones' (each null without
    // such ships); one of them alone, both are it.
    let mixed = any_deep && shallows > 0;
    let mut p1: Vec<i16> = if any_deep { deep } else { shallow }.to_vec();
    let mut p2: Vec<i16> = if mixed { shallow.to_vec() } else { p1.clone() };
    // The centre: the median of the cells (the two middle ones' mean,
    // rounded down, for an even count).
    let mut xs: Vec<i32> = ships.iter().map(|s| s.x).collect();
    let mut ys: Vec<i32> = ships.iter().map(|s| s.y).collect();
    xs.sort();
    ys.sort();
    let (mut cx, mut cy) = if n % 2 == 0 {
        (
            (xs[n / 2 - 1] + xs[n / 2]) / 2,
            (ys[n / 2 - 1] + ys[n / 2]) / 2,
        )
    } else {
        (xs[n / 2], ys[n / 2])
    };
    let inside = |(x, y): (i32, i32)| x >= 0 && y >= 0 && x < w && y < h;
    if !inside((cx, cy)) || !inside(to) {
        return Err(Fail::Refused);
    }
    let centre0 = (cx, cy);
    let at = |x: i32, y: i32| (y * w + x) as usize;
    let layers = (w + h) * 2;
    // Floods from the goal, a closed goal's region opened first.
    let goal_flood = |m: &[i16]| -> Result<Vec<i16>, Fail> {
        let mut f = m.to_vec();
        open_region(&mut f, w, h, to);
        if flood(&mut f, w, h, to, layers) < 1 {
            return Err(Fail::Refused);
        }
        Ok(f)
    };
    if mixed {
        goal_flood(&p2)?;
    }
    let g1 = goal_flood(&p1)?;
    // A closed centre moves to the open cell within three rings with the
    // lowest goal flood, the nearer on a tie. (The original tests each
    // edge's openness at the edge's first cell only: so here.)
    if p1[at(cx, cy)] < 0 {
        let mut best: Option<(i32, i32, (i32, i32))> = None;
        for r in 1..4 {
            let left = (cx - r).max(0);
            let top = (cy - r).max(0);
            let right = (cx + r).min(w - 1);
            let bottom = (cy + r).min(h - 1);
            let mut consider = |fixed: (i32, i32), c: (i32, i32), d: i32| {
                if p1[at(fixed.0, fixed.1)] == 0 {
                    let v = g1[at(c.0, c.1)] as i32;
                    if best.is_none_or(|(bv, bd, _)| v < bv || (v == bv && d < bd)) {
                        best = Some((v, d, c));
                    }
                }
            };
            let edge = |n0: i32, c: i32| if n0 < c { -1 } else { 1 };
            if r <= right - cx {
                let mut d = r * 2 - top + cy;
                for y in top..=bottom {
                    consider((right, top), (right, y), d);
                    d += edge(y, cy);
                }
            }
            if r <= cy - top {
                let mut d = r * 2 - left + cx;
                for x in left..=right {
                    consider((left, top), (x, top), d);
                    d += edge(x, cx);
                }
            }
            if r <= cx - left {
                let mut d = r * 2 - top + cy;
                for y in top..=bottom {
                    consider((left, top), (left, y), d);
                    d += edge(y, cy);
                }
            }
            if r <= bottom - cy {
                let mut d = r * 2 - left + cx;
                for x in left..=right {
                    consider((left, bottom), (x, bottom), d);
                    d += edge(x, cx);
                }
            }
        }
        let (_, _, c) = best.ok_or(Fail::Refused)?;
        (cx, cy) = c;
    }
    let minx = xs[0];
    let miny = ys[0];
    let shape: Vec<(i32, i32)> = ships.iter().map(|s| (s.x - minx, s.y - miny)).collect();
    // Floods from the (moved) centre; the goal keeps its closed value.
    let centre_flood = |m: &[i16]| -> Result<Vec<i16>, Fail> {
        let mut f = m.to_vec();
        if flood(&mut f, w, h, (cx, cy), layers) < 1 {
            return Err(Fail::Refused);
        }
        if m[at(to.0, to.1)] < 0 {
            f[at(to.0, to.1)] = m[at(to.0, to.1)];
        }
        Ok(f)
    };
    let c1 = centre_flood(&p1)?;
    let c2 = if mixed {
        Some(centre_flood(&p2)?)
    } else {
        None
    };
    let deep_count = if mixed { n - shallows } else { n } as i32;
    let mut slots: Vec<Slot> = Vec::new();
    let mut deep_only = 0i32;
    let mark = |m: &mut Vec<i16>, s: &Slot| {
        let i = at(s.x, s.y);
        m[i] = (m[i] as u16 | 0x8000) as i16;
    };
    // The shallow ships' slots first, on their grid, marked taken in both.
    if let Some(c2) = &c2 {
        let ring = Ring {
            w,
            h,
            to,
            origin: (cx - minx, cy - miny),
            size: (xs[n - 1] - minx + 1, ys[n - 1] - miny + 1),
            shape: &shape,
            centre_flood: c2,
        };
        ring_slots(&ring, &p2, &mut slots, 0, shallows);
        if slots.len() != shallows {
            return Err(Fail::Refused);
        }
        for s in &slots {
            mark(&mut p1, s);
            mark(&mut p2, s);
        }
    }
    let ring = Ring {
        w,
        h,
        to,
        origin: (cx - minx, cy - miny),
        size: (xs[n - 1] - minx + 1, ys[n - 1] - miny + 1),
        shape: &shape,
        centre_flood: &c1,
    };
    let from = slots.len();
    ring_slots(&ring, &p1, &mut slots, from, n);
    if slots.len() != n {
        return Err(Fail::Refused);
    }
    for s in &slots[from..] {
        mark(&mut p1, s);
        if mixed && p2[at(s.x, s.y)] == -1 {
            deep_only += 1;
        }
    }
    if slots.iter().all(|s| s.shape) {
        return Ok(ships
            .iter()
            .map(|s| (to.0 - cx + s.x, to.1 - cy + s.y))
            .collect());
    }
    deal(
        DealIn {
            w,
            h,
            to,
            centre0,
            centre: (cx, cy),
            goal_flood: &g1,
            centre_flood: &c1,
            shallow_grid: &p2,
            mixed,
        },
        ships,
        slots,
        deep_count,
        deep_only,
    )
}

struct DealIn<'a> {
    w: i32,
    h: i32,
    to: (i32, i32),
    centre0: (i32, i32),
    centre: (i32, i32),
    goal_flood: &'a [i16],
    centre_flood: &'a [i16],
    shallow_grid: &'a [i16],
    mixed: bool,
}

/// 0x55f870: the slots dealt out. The slots' median (the two middle
/// ones' sum halved, an odd sum rounded toward the goal); the direction
/// from the centre downhill on the goal flood (`toward`, a diagonal
/// cell's value one more, the direction to the slots' median on a tie)
/// and from
/// the slots' median downhill on the centre flood (`back`). The slots are
/// sorted by how far they lie along `back` turned about, the higher
/// centre flood first on a tie; the ships by how far they lie along
/// `toward` from the first centre, the lower goal flood first; the n-th
/// ship takes the n-th slot. With both draughts a shallow ship's slot that
/// only a deep one may take is swapped for the next it may, and a deep
/// one takes the next deep-only slot while there are as many of those as
/// deep ships left.
fn deal(
    d: DealIn,
    ships: &[Ship],
    mut slots: Vec<Slot>,
    mut deep_left: i32,
    mut deep_only: i32,
) -> Result<Vec<(i32, i32)>, Fail> {
    let n = slots.len();
    let at = |x: i32, y: i32| (y * d.w + x) as usize;
    let mut sx: Vec<i32> = slots.iter().map(|s| s.x).collect();
    let mut sy: Vec<i32> = slots.iter().map(|s| s.y).collect();
    sx.sort();
    sy.sort();
    let middle = if n % 2 == 0 {
        let (mut a, mut b) = (sx[n / 2 - 1] + sx[n / 2], sy[n / 2 - 1] + sy[n / 2]);
        if a & 1 != 0 && a / 2 < d.to.0 {
            a += 1;
        }
        if b & 1 != 0 && b / 2 < d.to.1 {
            b += 1;
        }
        (a / 2, b / 2)
    } else {
        (sx[n / 2], sy[n / 2])
    };
    let downhill = |from: (i32, i32), tie: i32, value: &dyn Fn(usize) -> i32| {
        let mut best = 30000;
        let mut pick = None;
        for (k, (dx, dy)) in DIRS.iter().enumerate() {
            let (x, y) = (from.0 + dx, from.1 + dy);
            if x < 0 || y < 0 || x >= d.w || y >= d.h {
                continue;
            }
            let mut v = value(at(x, y));
            if v <= 0 {
                continue;
            }
            if k & 1 != 0 {
                v += 1;
            }
            if v < best || (v == best && k as i32 == tie) {
                best = v;
                pick = Some(k as i32);
            }
        }
        pick
    };
    let gf = |i: usize| (d.goal_flood[i] as u16 & 0x7fff) as i32;
    let toward = downhill(d.centre, dir_to(d.centre, middle), &gf).ok_or(Fail::Unsettled)?;
    let cf = |i: usize| d.centre_flood[i] as i32;
    let back = downhill(middle, dir_to(middle, d.centre), &cf).ok_or(Fail::Unsettled)?;
    for s in &mut slots {
        s.key = along(middle, (back - 4) & 7, (s.x, s.y));
    }
    // Bubble sorts, as the original's.
    for i in (0..n.saturating_sub(1)).rev() {
        for j in 0..=i {
            let (a, b) = (slots[j], slots[j + 1]);
            if b.key < a.key || (a.key == b.key && a.flood < b.flood) {
                slots.swap(j, j + 1);
            }
        }
    }
    let mut order: Vec<usize> = (0..ships.len()).collect();
    let key: Vec<i32> = ships
        .iter()
        .map(|s| along(d.centre0, toward, (s.x, s.y)))
        .collect();
    for i in (0..n.saturating_sub(1)).rev() {
        for j in 0..=i {
            let (a, b) = (order[j], order[j + 1]);
            let (ga, gb) = (
                gf(at(ships[a].x, ships[a].y)),
                gf(at(ships[b].x, ships[b].y)),
            );
            if key[b] < key[a] || (key[a] == key[b] && gb < ga) {
                order.swap(j, j + 1);
            }
        }
    }
    if d.mixed {
        let closed = |s: &Slot| d.shallow_grid[at(s.x, s.y)] == -1;
        for j in 0..n {
            let deep = !ships[order[j]].shallow;
            if closed(&slots[j]) {
                if deep {
                    deep_left -= 1;
                    deep_only -= 1;
                } else {
                    let t = (j + 1..n)
                        .find(|&t| !closed(&slots[t]))
                        .ok_or(Fail::Refused)?;
                    let s = slots.remove(t);
                    slots.insert(j, s);
                }
            } else if deep {
                if deep_left <= deep_only {
                    let t = (j + 1..n)
                        .find(|&t| closed(&slots[t]))
                        .ok_or(Fail::Refused)?;
                    let s = slots.remove(t);
                    slots.insert(j, s);
                    deep_only -= 1;
                }
                deep_left -= 1;
            }
        }
    }
    let mut out = vec![(0, 0); ships.len()];
    for (j, &k) in order.iter().enumerate() {
        out[k] = (slots[j].x, slots[j].y);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Open water: the wavefront counts a straight step 2 and a diagonal
    /// 3 from the start's 1.
    #[test]
    fn flood_counts_steps() {
        let (w, h) = (5, 4);
        let mut m = vec![0i16; 20];
        assert!(flood(&mut m, w, h, (1, 1), (w + h) * 2) > 0);
        assert_eq!(m[(w + 1) as usize], 1);
        assert_eq!(m[(w + 2) as usize], 3);
        assert_eq!(m[(2 * w + 2) as usize], 4);
        assert_eq!(m[(w + 4) as usize], 7);
    }

    /// A fleet in open water keeps its shape about the goal.
    #[test]
    fn fleet_moves_whole() {
        let (w, h) = (20, 20);
        let sea = vec![0i16; 400];
        let ships = [
            Ship {
                x: 2,
                y: 2,
                shallow: false,
            },
            Ship {
                x: 3,
                y: 2,
                shallow: false,
            },
            Ship {
                x: 2,
                y: 3,
                shallow: false,
            },
        ];
        let got = plan(&sea, &sea, (w, h), (10, 10), &ships).unwrap();
        assert_eq!(got, vec![(10, 10), (11, 10), (10, 11)]);
    }
}
