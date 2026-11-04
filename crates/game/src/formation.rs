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

/// The C library's `qsort` (0x58791f, Microsoft's): its order matters
/// where keys are equal. Ranges of more than 8 elements are partitioned
/// about the middle one (swapped to the front; from the left the first
/// element above it, from the right the first below it, exchanged until
/// they cross, then the pivot swapped to the crossing), the smaller side
/// sorted first by a stack of ranges; ranges of 8 or fewer go to 0x587a73,
/// which swaps the greatest (the first of equals) to the end, then the
/// greatest of the rest, and so on.
pub fn crt_qsort<T>(v: &mut [T], cmp: impl Fn(&T, &T) -> i32) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let (mut lo, mut hi) = (0usize, n - 1);
    loop {
        let size = hi - lo + 1;
        let mut next = None;
        if size <= 8 {
            let mut h = hi;
            while h > lo {
                let mut max = lo;
                for p in lo + 1..=h {
                    if cmp(&v[p], &v[max]) > 0 {
                        max = p;
                    }
                }
                v.swap(max, h);
                h -= 1;
            }
        } else {
            v.swap(lo + size / 2, lo);
            let (mut loguy, mut higuy) = (lo, hi + 1);
            loop {
                loop {
                    loguy += 1;
                    if loguy > hi || cmp(&v[loguy], &v[lo]) > 0 {
                        break;
                    }
                }
                loop {
                    higuy -= 1;
                    if higuy <= lo || cmp(&v[higuy], &v[lo]) < 0 {
                        break;
                    }
                }
                if higuy < loguy {
                    break;
                }
                v.swap(loguy, higuy);
            }
            v.swap(lo, higuy);
            // `higuy - 1 - lo < hi - loguy` in bytes: the left side the
            // smaller, the right one is put by.
            if ((higuy - lo) as isize) <= hi as isize - loguy as isize {
                if loguy < hi {
                    stack.push((loguy, hi));
                }
                if lo + 1 < higuy {
                    next = Some((lo, higuy - 1));
                }
            } else {
                if lo + 1 < higuy {
                    stack.push((lo, higuy - 1));
                }
                if loguy < hi {
                    next = Some((loguy, hi));
                }
            }
        }
        match next.or_else(|| stack.pop()) {
            Some(r) => (lo, hi) = r,
            None => return,
        }
    }
}

/// The cells between `a` and `b` (0x557440): the longer side and half the
/// shorter, `(2 * long + short) / 2`.
pub fn cells_apart(a: (i32, i32), b: (i32, i32)) -> i32 {
    let (dx, dy) = ((b.0 - a.0).abs(), (b.1 - a.1).abs());
    if dx >= dy {
        (dy + 2 * dx) / 2
    } else {
        (dx + 2 * dy) / 2
    }
}

/// 0x561a30: `cells` (targets' cells) dealt to `ships` (their cells), a
/// cell for each ship. With one cell or one ship every ship takes the
/// first cell. Else `k = min(cells, ships)`: the cells sorted by their
/// distance (0x557440) from the first (`crt_qsort`), the first `k` kept;
/// the cells' point the sum of their coordinates halved and the ships'
/// point the sum of the ships' halved (sic: not averaged; an odd sum
/// rounded up when its half lies below the first cell's coordinate);
/// quotas of ships, the first cell's `(2m - 1) / k`, each next `(r / 2 +
/// r) / left` of the `r` left, the last also the rest. Each cell's key is
/// how far it lies (0x5574d0) along the direction from the ships' point
/// to the cells' point turned about from the cells' point, each ship's
/// along the direction from the ships' point to the cells' point: the
/// cells bubble-sorted by key, the farther from the ships' point first on
/// a tie (carrying their quotas); the ships by key, the nearer the cell
/// whose quota the pass has reached first on a tie. Then the ships in
/// that order fill the cells' quotas in order. `None` for no cells or no
/// ships (the original divides by zero).
// Its caller, 0x4d5560 of the human's attack-zone block, is not ported.
#[allow(dead_code)]
pub fn deal_cells(cells: &[(i32, i32)], ships: &[(i32, i32)]) -> Option<Vec<(i32, i32)>> {
    let (n, m) = (cells.len(), ships.len());
    let k = n.min(m);
    if k == 0 {
        return None;
    }
    if k == 1 {
        return Some(vec![cells[0]; m]);
    }
    #[derive(Clone, Copy)]
    struct Cell {
        at: (i32, i32),
        key: i32,
        dist: i32,
        far: i32,
        quota: i32,
    }
    let mut recs: Vec<Cell> = cells
        .iter()
        .map(|&at| Cell {
            at,
            key: 0,
            dist: cells_apart(cells[0], at),
            far: 0,
            quota: 0,
        })
        .collect();
    crt_qsort(&mut recs, |a, b| a.dist - b.dist);
    recs.truncate(k);
    let half = |sum: i32, first: i32| {
        if sum & 1 != 0 && sum / 2 < first {
            (sum + 1) / 2
        } else {
            sum / 2
        }
    };
    let point = |xs: &mut dyn Iterator<Item = (i32, i32)>| {
        let (sx, sy) = xs.fold((0, 0), |(a, b), (x, y)| (a + x, b + y));
        (half(sx, cells[0].0), half(sy, cells[0].1))
    };
    let at = point(&mut recs.iter().map(|c| c.at));
    let from = point(&mut ships.iter().copied());
    let m = m as i32;
    recs[0].quota = (2 * m - 1) / k as i32;
    let mut rest = m - recs[0].quota;
    for (i, c) in recs.iter_mut().enumerate().skip(1) {
        let left = (k - i) as i32;
        c.quota = (rest / 2 + rest) / left;
        rest -= c.quota;
    }
    recs[k - 1].quota += rest;
    let toward = dir_to(from, at);
    let back = dir_to(at, from);
    for c in &mut recs {
        c.key = along(at, (back - 4) & 7, c.at);
        c.far = cells_apart(from, c.at);
    }
    for i in (0..k - 1).rev() {
        for j in 0..=i {
            let (a, b) = (recs[j], recs[j + 1]);
            if b.key < a.key || (a.key == b.key && a.far < b.far) {
                recs.swap(j, j + 1);
            }
        }
    }
    let key: Vec<i32> = ships.iter().map(|&s| along(from, toward, s)).collect();
    let mut order: Vec<usize> = (0..ships.len()).collect();
    for i in (0..ships.len() - 1).rev() {
        let (mut c, mut left) = (0, recs[0].quota);
        for j in 0..=i {
            let (a, b) = (order[j], order[j + 1]);
            if key[b] < key[a]
                || (key[a] == key[b]
                    && cells_apart(recs[c].at, ships[b]) < cells_apart(recs[c].at, ships[a]))
            {
                order.swap(j, j + 1);
            }
            left -= 1;
            while left < 1 {
                c += 1;
                left = recs[c].quota;
            }
        }
    }
    let mut out = vec![(0, 0); ships.len()];
    let (mut c, mut left) = (0, recs[0].quota);
    for (j, &s) in order.iter().enumerate() {
        out[s] = recs[c].at;
        left -= 1;
        // (After the last ship the original reads on past the cells.)
        while left < 1 && j + 1 < order.len() {
            c += 1;
            left = recs[c].quota;
        }
    }
    Some(out)
}

/// A slot a ship may take (the planner's 0x18-byte record): its cell, the
/// threat grid's value there (`+8`, which 0x55f870 then overwrites with
/// the sort key), ring distance from the goal, the centre flood's value
/// there and whether it lies where a ship of the fleet would, the fleet
/// moved whole.
#[derive(Clone, Copy)]
struct Slot {
    x: i32,
    y: i32,
    threat: i32,
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
    /// The third grid (state `+0xc`): 0x4cc970 gives none.
    threat: Option<&'a [i16]>,
}

/// 0x561360: slot `(x, y)` (ring distance `d`) put in its place among
/// `slots[start..]`, keeping at most `count`: one where a ship of the
/// fleet would lie goes before another unless that one is two nearer;
/// alike, the nearer first, then the lower threat, then the lower centre
/// flood.
fn add_slot(r: &Ring, slots: &mut Vec<Slot>, start: usize, count: usize, x: i32, y: i32, d: i32) {
    let (rx, ry) = (r.origin.0 - r.to.0 + x, r.origin.1 - r.to.1 + y);
    let shape = rx >= 0 && rx < r.size.0 && ry >= 0 && ry < r.size.1 && r.shape.contains(&(rx, ry));
    let i = (y * r.w + x) as usize;
    let threat = r.threat.map_or(0, |t| t[i] as i32);
    let flood = r.centre_flood[i] as i32;
    let mut pos = start;
    while pos < slots.len() {
        let e = slots[pos];
        let before = if shape == e.shape {
            d < e.dist
                || (d <= e.dist && ((threat == e.threat && flood < e.flood) || threat < e.threat))
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
                threat,
                key: 0,
                dist: d,
                flood,
                shape,
            },
        );
    }
}

/// 0x560f80 for slot class `class`: the open cells of `m` (still 0) in
/// square rings about the goal, from radius `2 * class / 3` to 10, each
/// edge walked as the original does (the right side bottom to top, the
/// top right to left, the left top to bottom, the bottom left to right;
/// an edge past the map left out, the others cut to it), ring distance
/// `d = (2r + offset) / 2`; taken when `2r < 5` and `class <= r`, or when
/// `class <= d < 11`.
fn ring_slots(r: &Ring, m: &[i16], class: i32, slots: &mut Vec<Slot>, start: usize, count: usize) {
    let (tx, ty) = r.to;
    let w = r.w;
    let ok = |rad: i32, d: i32| (2 * rad < 5 && class <= rad) || (class <= d && d < 11);
    let take = |slots: &mut Vec<Slot>, x: i32, y: i32, rad: i32, dist: i32| {
        if m[(y * w + x) as usize] == 0 && ok(rad, dist / 2) {
            add_slot(r, slots, start, count, x, y, dist / 2);
        }
    };
    for rad in (class * 2) / 3..=10 {
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

/// A ship's record as 0x560100 reads it (0x24 bytes): its cell, its slot
/// class (`+8`, below 1 counted 0) and `+0xc`, which puts it on the second
/// grid when both are given.
#[derive(Clone, Copy)]
pub struct Rec {
    pub x: i32,
    pub y: i32,
    pub class: i32,
    pub second: bool,
}

/// What the planner gives: a cell for each record (`+0x18`, `+0x1c`), its
/// value (0 when the fleet moved whole, else 0x55f870's: the greatest goal
/// flood at a ship's cell) and each ship's third (`+0x20`: 0 when moved
/// whole, else 1-3 by its slot's rank).
#[derive(Debug, PartialEq)]
pub struct Planned {
    pub cells: Vec<(i32, i32)>,
    pub value: i32,
    pub thirds: Vec<u8>,
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
    /// never wrote (`[esp+0x34]`, `[esp+0x24]`); or a slot class of 10 or
    /// more (bucket 10 is never planned, 0x560100 loops classes 0-9, and
    /// 0x55f870 then reads medians past the slots it sorted).
    Unsettled,
}

/// The formation planner 0x560100 (through 0x5614e0, as 0x4cc970 calls
/// it: no goal tolerance, the centre the ships' median, every ship of
/// class 0, no third grid): a cell for each of `ships` about `to`, on
/// `deep` (0x5c9398) and `shallow` (0x5c93bc), `w` x `h`.
pub fn plan(
    deep: &[i16],
    shallow: &[i16],
    (w, h): (i32, i32),
    to: (i32, i32),
    ships: &[Ship],
) -> Result<Vec<(i32, i32)>, Fail> {
    let recs: Vec<Rec> = ships
        .iter()
        .map(|s| Rec {
            x: s.x,
            y: s.y,
            class: 0,
            second: s.shallow,
        })
        .collect();
    // 0x4cc970 hands over 0x5c9398 with a deep ship, 0x5c93bc with a
    // shallow one.
    let first = ships.iter().any(|s| !s.shallow).then_some(deep);
    let second = ships.iter().any(|s| s.shallow).then_some(shallow);
    plan_records(first, second, None, (w, h), to, &recs).map(|p| p.cells)
}

/// The formation planner 0x560100 (0x5614e0: the centre the records'
/// median, no goal tolerance): a cell for each of `recs` about `to` on
/// the grids `p1` and `p2` (each null in the original when not given),
/// `w` x `h`, with `threat` the third grid (0x561360's tie). The records
/// go into buckets by class (0-9); with both grids each bucket's
/// second-grid ships get their slots first, on `p2`, marked taken in both
/// grids, then each bucket's others on `p1` (0x560f80 with the class).
/// Each ship keeps its place about the fleet's centre if every slot chosen
/// lies where one of the ships would (the fleet moved whole, value 0);
/// otherwise the slots are dealt out by 0x55f870.
pub fn plan_records(
    p1: Option<&[i16]>,
    p2: Option<&[i16]>,
    threat: Option<&[i16]>,
    (w, h): (i32, i32),
    to: (i32, i32),
    recs: &[Rec],
) -> Result<Planned, Fail> {
    let n = recs.len();
    if n == 0 {
        return Err(Fail::Refused);
    }
    if recs.iter().any(|r| r.class >= 10) {
        return Err(Fail::Unsettled);
    }
    // The first grid's ships (`+0xc` 0), all when there is no second
    // grid; none, or no first grid, the second serves all; all, the first.
    let (mut g1, mut g2) = (p1, p2);
    let mut firsts = if g2.is_some() {
        recs.iter().filter(|r| !r.second).count()
    } else {
        n
    };
    if firsts == 0 || g1.is_none() {
        g1 = g2;
        firsts = n;
    }
    if firsts == n {
        g2 = g1;
    }
    let mixed = firsts != n;
    let mut p1: Vec<i16> = g1.ok_or(Fail::Refused)?.to_vec();
    let mut p2: Vec<i16> = g2.ok_or(Fail::Refused)?.to_vec();
    // The centre: the median of the cells (the two middle ones' mean,
    // rounded down, for an even count).
    let mut xs: Vec<i32> = recs.iter().map(|s| s.x).collect();
    let mut ys: Vec<i32> = recs.iter().map(|s| s.y).collect();
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
    let shape: Vec<(i32, i32)> = recs.iter().map(|s| (s.x - minx, s.y - miny)).collect();
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
    // The buckets by class (state `+0xf0`): the ships in record order,
    // and those of the first grid (all when not mixed).
    let mut ships: [Vec<usize>; 10] = Default::default();
    let mut firsts_of = [0usize; 10];
    for (k, r) in recs.iter().enumerate() {
        let c = r.class.max(0) as usize;
        ships[c].push(k);
        if !r.second || !mixed {
            firsts_of[c] += 1;
        }
    }
    let mut slots: [Vec<Slot>; 10] = Default::default();
    let mut deep_only = [0i32; 10];
    let mut whole = true;
    let mark = |m: &mut Vec<i16>, s: &Slot| {
        let i = at(s.x, s.y);
        m[i] = (m[i] as u16 | 0x8000) as i16;
    };
    let ring = |centre_flood| Ring {
        w,
        h,
        to,
        origin: (cx - minx, cy - miny),
        size: (xs[n - 1] - minx + 1, ys[n - 1] - miny + 1),
        shape: &shape,
        centre_flood,
        threat,
    };
    // The second grid's ships' slots first, on their grid, marked taken
    // in both.
    if let Some(c2) = &c2 {
        let ring = ring(c2);
        for c in 0..10 {
            let k = ships[c].len() - firsts_of[c];
            if k == 0 {
                continue;
            }
            ring_slots(&ring, &p2, c as i32, &mut slots[c], 0, k);
            if slots[c].len() != k {
                return Err(Fail::Refused);
            }
            for s in &slots[c] {
                mark(&mut p1, s);
                mark(&mut p2, s);
                whole &= s.shape;
                if p2[at(s.x, s.y)] == -1 {
                    deep_only[c] += 1;
                }
            }
        }
    }
    let ring = ring(&c1);
    for c in 0..10 {
        if firsts_of[c] == 0 {
            continue;
        }
        let from = slots[c].len();
        let k = ships[c].len();
        ring_slots(&ring, &p1, c as i32, &mut slots[c], from, k);
        if slots[c].len() != k {
            return Err(Fail::Refused);
        }
        for s in &slots[c][from..] {
            mark(&mut p1, s);
            whole &= s.shape;
            if mixed && p2[at(s.x, s.y)] == -1 {
                deep_only[c] += 1;
            }
        }
    }
    if whole {
        return Ok(Planned {
            cells: recs
                .iter()
                .map(|s| (to.0 - cx + s.x, to.1 - cy + s.y))
                .collect(),
            value: 0,
            thirds: vec![0; n],
        });
    }
    let deep_left = firsts_of.map(|k| k as i32);
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
        recs,
        &ships,
        slots,
        deep_left,
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

/// 0x55f870: the slots dealt out. The slots' median, over every bucket
/// (the two middle ones' sum halved, an odd sum rounded toward the goal);
/// the direction from the centre downhill on the goal flood (`toward`, a
/// diagonal cell's value one more, the direction to the slots' median on
/// a tie) and from the slots' median downhill on the centre flood
/// (`back`). In each bucket the slots are sorted by how far they lie along
/// `back` turned about, the higher centre flood first on a tie; the ships
/// by how far they lie along `toward` from the first centre, the lower
/// goal flood first; the n-th ship takes the n-th slot. The value is the
/// greatest goal flood at a ship's cell. With both grids a second-grid
/// ship's slot that only a first-grid one may take is swapped for the
/// next it may, and a first-grid one takes the next such slot while there
/// are as many of those as first-grid ships left (per bucket). Each ship's
/// third: its slot's rank among all the slots by key (the earlier on a
/// tie, before the swaps), against `n / 3` and `((n - n / 3) + 1) / 2 + n
/// / 3`.
fn deal(
    d: DealIn,
    recs: &[Rec],
    ships: &[Vec<usize>; 10],
    mut slots: [Vec<Slot>; 10],
    mut deep_left: [i32; 10],
    mut deep_only: [i32; 10],
) -> Result<Planned, Fail> {
    let n = recs.len();
    let at = |x: i32, y: i32| (y * d.w + x) as usize;
    let mut sx: Vec<i32> = slots.iter().flatten().map(|s| s.x).collect();
    let mut sy: Vec<i32> = slots.iter().flatten().map(|s| s.y).collect();
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
    let mut order: [Vec<usize>; 10] = Default::default();
    let mut value = 0;
    for c in 0..10 {
        let slots = &mut slots[c];
        let m = slots.len();
        if m == 0 {
            continue;
        }
        for s in slots.iter_mut() {
            s.key = along(middle, (back - 4) & 7, (s.x, s.y));
        }
        // Bubble sorts, as the original's.
        for i in (0..m.saturating_sub(1)).rev() {
            for j in 0..=i {
                let (a, b) = (slots[j], slots[j + 1]);
                if b.key < a.key || (a.key == b.key && a.flood < b.flood) {
                    slots.swap(j, j + 1);
                }
            }
        }
        let order = &mut order[c];
        *order = ships[c].clone();
        let key = |k: usize| along(d.centre0, toward, (recs[k].x, recs[k].y));
        let g = |k: usize| gf(at(recs[k].x, recs[k].y));
        for i in (0..m.saturating_sub(1)).rev() {
            for j in 0..=i {
                let (a, b) = (order[j], order[j + 1]);
                if key(b) < key(a) || (key(a) == key(b) && g(b) < g(a)) {
                    order.swap(j, j + 1);
                }
            }
        }
        value = order.iter().map(|&k| g(k)).fold(value, i32::max);
    }
    // The slots' ranks by key over every bucket (an insertion sort that
    // keeps the earlier first on a tie).
    let keys: Vec<i32> = slots.iter().flatten().map(|s| s.key).collect();
    let mut rank: Vec<usize> = (0..keys.len()).collect();
    rank.sort_by_key(|&g| keys[g]);
    if d.mixed {
        let closed = |s: &Slot| d.shallow_grid[at(s.x, s.y)] == -1;
        for c in 0..10 {
            let (slots, order) = (&mut slots[c], &order[c]);
            for j in 0..slots.len() {
                let deep = !recs[order[j]].second;
                if closed(&slots[j]) {
                    if deep {
                        deep_left[c] -= 1;
                        deep_only[c] -= 1;
                    } else {
                        let t = (j + 1..slots.len())
                            .find(|&t| !closed(&slots[t]))
                            .ok_or(Fail::Refused)?;
                        let s = slots.remove(t);
                        slots.insert(j, s);
                    }
                } else if deep {
                    if deep_left[c] <= deep_only[c] {
                        let t = (j + 1..slots.len())
                            .find(|&t| closed(&slots[t]))
                            .ok_or(Fail::Refused)?;
                        let s = slots.remove(t);
                        slots.insert(j, s);
                        deep_only[c] -= 1;
                    }
                    deep_left[c] -= 1;
                }
            }
        }
    }
    let third = n / 3;
    let mut cells = vec![(0, 0); n];
    let mut thirds = vec![0u8; n];
    let mut g = 0;
    for c in 0..10 {
        for (j, &k) in order[c].iter().enumerate() {
            cells[k] = (slots[c][j].x, slots[c][j].y);
            let r = rank.iter().position(|&x| x == g).unwrap_or(rank.len());
            thirds[k] = if r < third {
                1
            } else if r < (n - third).div_ceil(2) + third {
                2
            } else {
                3
            };
            g += 1;
        }
    }
    Ok(Planned {
        cells,
        value,
        thirds,
    })
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

    /// 0x58791f: equal keys of a short run end as 0x587a73 leaves them
    /// (the first of the greatest to the end each pass); longer runs sort.
    #[test]
    fn crt_qsort_ties() {
        let mut v = [(5, 0), (5, 1), (5, 2)];
        crt_qsort(&mut v, |a, b| a.0 - b.0);
        assert_eq!(v, [(5, 1), (5, 2), (5, 0)]);
        let mut v = [(2, 0), (1, 1), (2, 2), (1, 3)];
        crt_qsort(&mut v, |a, b| a.0 - b.0);
        // Pass 1: the greatest first found (index 0) to the end; pass 2:
        // of (1, 3), (1, 1), (2, 2) the 2 to the end; pass 3: equal, swap.
        assert_eq!(v, [(1, 1), (1, 3), (2, 2), (2, 0)]);
        let mut v: Vec<i32> = (0..200).map(|i| (i * 7919) % 101).collect();
        crt_qsort(&mut v, |a, b| a - b);
        assert!(v.windows(2).all(|w| w[0] <= w[1]));
    }

    /// 0x561a30: one cell for all; two ships to two cells by their keys
    /// along the line between the halved sums (quotas 1 and 1).
    #[test]
    fn deal_cells_by_keys() {
        let one = deal_cells(&[(4, 4), (9, 9)], &[(0, 0)]).unwrap();
        assert_eq!(one, vec![(4, 4)]);
        let got = deal_cells(&[(10, 2), (10, 0)], &[(0, 0), (0, 2)]).unwrap();
        assert_eq!(got, vec![(10, 0), (10, 2)]);
        assert_eq!(deal_cells(&[], &[(0, 0)]), None);
        assert_eq!(cells_apart((0, 0), (3, 1)), 3);
        assert_eq!(cells_apart((0, 0), (1, 4)), 4);
    }

    /// Slot class 6 (0x560f80 from ring 4): no slot within ring distance
    /// 6 of the goal, none past 10, so none where the fleet would lie and
    /// the slots are dealt (0x55f870): the value is the greatest goal
    /// flood at a ship's cell, the thirds by slot rank.
    #[test]
    fn class_six_keeps_off() {
        let (w, h) = (30, 30);
        let sea = vec![0i16; 900];
        let to = (15, 15);
        let recs: Vec<Rec> = [(2, 2), (3, 2), (2, 3), (3, 3)]
            .iter()
            .map(|&(x, y)| Rec {
                x,
                y,
                class: 6,
                second: false,
            })
            .collect();
        let got = plan_records(Some(&sea), None, None, (w, h), to, &recs).unwrap();
        let mut g = sea.clone();
        flood(&mut g, w, h, to, (w + h) * 2);
        let most = recs
            .iter()
            .map(|r| g[(r.y * w + r.x) as usize] as i32 & 0x7fff)
            .max()
            .unwrap();
        assert_eq!(got.value, most);
        for (k, &(x, y)) in got.cells.iter().enumerate() {
            let (dx, dy) = ((x - to.0).abs(), (y - to.1).abs());
            // Ring distance: the ring's radius plus half the step along it.
            let d = (2 * dx.max(dy) + dx.min(dy)) / 2;
            assert!((6..11).contains(&d), "{:?} d {}", (x, y), d);
            assert!(!got.cells[..k].contains(&(x, y)));
        }
        let mut t = got.thirds.clone();
        t.sort();
        assert_eq!(t, vec![1, 2, 2, 3]);
        // The same fleet at class 0 moves whole: value 0, thirds 0.
        let zero: Vec<Rec> = recs.iter().map(|r| Rec { class: 0, ..*r }).collect();
        let got = plan_records(Some(&sea), None, None, (w, h), to, &zero).unwrap();
        assert_eq!(got.value, 0);
        assert_eq!(got.thirds, vec![0; 4]);
        assert_eq!(got.cells, vec![(15, 15), (16, 15), (15, 16), (16, 16)]);
    }
}
