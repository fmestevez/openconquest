//! The original's route finding over a grid of shorts (0 open, anything
//! else shut), as the land creatures use it on their half-cell walk grid:
//! the wavefront 0x557910 (a sweep of segments, not a shortest-path
//! search), the two straight legs 0x5718f0, the route 0x55b560 and the
//! last cell of 0x55ba50 with a tolerance.

/// The eight directions (0x5c4f4c): 0 east, counter-clockwise, y down.
pub const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// A step's cost in 0x55b560's back-trace (0x5c5074).
const STEP_COST: [i32; 8] = [2, 3, 2, 3, 2, 3, 2, 3];

/// 0x557440: `max + min / 2` of the offsets.
pub fn dist(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let (ax, ay) = ((x2 - x1).abs(), (y2 - y1).abs());
    if ax >= ay {
        (ay + 2 * ax) / 2
    } else {
        (ax + 2 * ay) / 2
    }
}

/// 0x557480: 0x557440 but (2, 2) is 2.
fn dist2(x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
    let (ax, ay) = ((x2 - x1).abs(), (y2 - y1).abs());
    if ax == 2 && ay == 2 {
        return 2;
    }
    if ay <= ax {
        (ay + 2 * ax) / 2
    } else {
        (ax + 2 * ay) / 2
    }
}

/// The way between two headings, at most 4.
pub fn circ(a: i32, b: i32) -> i32 {
    let d = (a - b).abs();
    if d > 4 {
        8 - d
    } else {
        d
    }
}

/// A segment of the wavefront (0x557910's 16-byte records).
#[derive(Clone, Copy)]
struct Seg {
    dir: i32,
    old: i16,
    len: i32,
    key: i32,
    x: i32,
    y: i32,
}

/// The wavefront 0x557910 over `m` (`w` wide), within the rectangle
/// `(x0, y0)-(x1, y1)`, from `(sx, sy)` (set to 1), each cell filled with
/// its neighbour's value + 2 straight or + 3 diagonally while that is at
/// most `2 lim + 2`, a value written over a non-zero one carrying 0x4000.
/// It sweeps segments, generation by generation (at most `lim`), each
/// generation's new segments kept in order of their least value; reaching
/// `(tx, ty)` ends it (`extra` < 1) or leaves `extra` more generations.
/// `d12` gives the initial heading a head start; `alt` a second value the
/// sweep may fill. Returns the generations run.
#[allow(clippy::too_many_arguments)]
pub fn wave(
    m: &mut [i16],
    w: i32,
    (x0, y0, x1, y1): (i32, i32, i32, i32),
    (sx, sy): (i32, i32),
    (tx, ty): (i32, i32),
    lim: i32,
    d12: i32,
    alt: i16,
    extra: i32,
) -> i32 {
    let cap = lim * 2 + 2;
    // Reads past the grid (which the original never makes on these
    // grids) count as shut.
    let at = |m: &[i16], i: i32| -> i16 {
        if i < 0 {
            -1
        } else {
            m.get(i as usize).copied().unwrap_or(-1)
        }
    };
    let val = |m: &[i16], i: i32| -> i32 { (at(m, i) as u16 & 0x3fff) as i32 };
    let put = |m: &mut [i16], i: i32, v: i32, c: i16| {
        if let Some(cell) = m.get_mut(i as usize) {
            *cell = if c != 0 {
                (v | 0x4000) as i16
            } else {
                v as i16
            };
        }
    };
    let mut cd = 30000;
    let mut key;
    if let Some(c) = m.get_mut((sy * w + sx) as usize) {
        *c = 1;
    }
    let mut cur: Vec<Seg> = [0, 2, 4, 6]
        .iter()
        .map(|&dir| Seg {
            dir,
            old: 0,
            len: 1,
            key: 0,
            x: sx,
            y: sy,
        })
        .collect();
    let mut gen = 0;
    if lim <= 0 {
        return 0;
    }
    // Filling or passing over the target: true ends the call.
    let hit = |x: i32, y: i32, cd: &mut i32| -> bool {
        if x == tx && y == ty {
            if extra < 1 {
                return true;
            }
            *cd = extra + 1;
        }
        false
    };
    macro_rules! hit {
        ($x:expr, $y:expr) => {
            if hit($x, $y, &mut cd) {
                return gen + 1;
            }
        };
    }
    loop {
        let mut nxt: Vec<Seg> = Vec::new();
        key = 0x7fff;
        // Inserted after the last segment whose key is not above it; the
        // key goes back to 0x7fff (0x57187c, 0x5718cc).
        macro_rules! push {
            ($d:expr, $old:expr, $n:expr, $x:expr, $y:expr) => {{
                let e = Seg {
                    dir: $d,
                    old: $old,
                    len: $n,
                    key,
                    x: $x,
                    y: $y,
                };
                let mut k = nxt.len();
                while k > 0 && nxt[k - 1].key > e.key {
                    k -= 1;
                }
                nxt.insert(k, e);
                key = 0x7fff;
            }};
        }
        // Pass 1: every segment steps one cell its way.
        for s in cur.clone() {
            let (old, d) = (s.old, s.dir);
            let mut n = s.len;
            let mut cnt = 0;
            let mut c = old;
            let (dx, dy, step, off);
            let (mut x, mut y, mut ptr, mut sx14, mut sy10);
            match d {
                0 => {
                    if s.x >= x1 {
                        continue;
                    }
                    (dx, off, dy, step) = (0, -1, -1, -w);
                    x = s.x + 1;
                    sx14 = x;
                    sy10 = s.y;
                    y = s.y + 1;
                    ptr = y * w + x;
                    let v = val(m, ptr - w - 1) + 3;
                    if y <= y1 && v <= cap && at(m, ptr) == old {
                        put(m, ptr, v, old);
                        sy10 = y;
                        key = v;
                        cnt = 1;
                    }
                }
                2 => {
                    if s.y <= y0 {
                        continue;
                    }
                    (dx, step, off, dy) = (-1, -1, w, 0);
                    sx14 = s.x;
                    x = s.x + 1;
                    y = s.y - 1;
                    ptr = y * w + x;
                    let v = val(m, ptr + w - 1) + 3;
                    sy10 = y;
                    if !(x1 < x || cap < v || at(m, ptr) != old) {
                        put(m, ptr, v, old);
                        sx14 = x;
                        key = v;
                        cnt = 1;
                    }
                }
                4 => {
                    if s.x <= x0 {
                        continue;
                    }
                    (off, dy, dx, step) = (1, 1, 0, w);
                    x = s.x - 1;
                    sy10 = s.y;
                    y = s.y - 1;
                    ptr = y * w + x;
                    let v = val(m, ptr + w + 1) + 3;
                    sx14 = x;
                    if y0 <= y && v <= cap && at(m, ptr) == old {
                        put(m, ptr, v, old);
                        sy10 = y;
                        key = v;
                        cnt = 1;
                    }
                }
                6 => {
                    if y1 <= s.y {
                        continue;
                    }
                    (off, dx, step, dy) = (-w, 1, 1, 0);
                    sx14 = s.x;
                    x = s.x - 1;
                    y = s.y + 1;
                    sy10 = y;
                    ptr = y * w + x;
                    let v = val(m, ptr + 1 - w) + 3;
                    if !(x < x0 || cap < v || at(m, ptr) != old) {
                        put(m, ptr, v, old);
                        sx14 = x;
                        key = v;
                        cnt = 1;
                    }
                }
                _ => continue,
            }
            x += dx;
            y += dy;
            ptr += step;
            while n != 0 {
                while at(m, ptr) == c {
                    let v = val(m, ptr + off) + 2;
                    if v > cap {
                        break;
                    }
                    if v < key {
                        key = v;
                    }
                    put(m, ptr, v, c);
                    ptr += step;
                    n -= 1;
                    cnt += 1;
                    hit!(x, y);
                    x += dx;
                    y += dy;
                    if n == 0 {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
                if cnt != 0 {
                    push!(d, c, cnt, sx14, sy10);
                }
                loop {
                    if val(m, ptr + off) + 2 <= cap && (at(m, ptr) == old || at(m, ptr) == alt) {
                        break;
                    }
                    ptr += step;
                    n -= 1;
                    hit!(x, y);
                    x += dx;
                    y += dy;
                    if n == 0 {
                        break;
                    }
                }
                sx14 = x;
                cnt = 0;
                sy10 = y;
                if n == 0 {
                    break;
                }
                c = at(m, ptr);
            }
            // The far end, diagonally.
            if x0 <= x && x <= x1 && y0 <= y && y <= y1 {
                let v = val(m, ptr + off - step) + 3;
                if v <= cap && at(m, ptr) == c {
                    if v < key {
                        key = v;
                    }
                    put(m, ptr, v, c);
                    cnt += 1;
                    hit!(x, y);
                }
            }
            if cnt != 0 {
                push!(d, c, cnt, sx14, sy10);
            }
        }
        // Pass 2: every segment spreads round its two ends.
        for s in cur.clone() {
            let (d, old) = (s.dir, s.old);
            match d {
                0 => {
                    let ny = s.y + 1;
                    if ny <= y1 {
                        let sx_ = s.x;
                        let x = s.x - 1;
                        let ptr = ny * w + x;
                        let u = val(m, ptr + 1 - w);
                        let v3 = u + 3;
                        let (mut c, f, mut cnt, sx14);
                        if x < x0 || at(m, ptr) != old || cap < v3 {
                            (f, cnt, c, sx14) = (0, 0, old, sx_);
                        } else {
                            c = at(m, ptr);
                            put(m, ptr, v3, c);
                            (f, cnt, sx14) = (1, 1, x);
                            hit!(x, ny);
                        }
                        let v2 = u + 2;
                        let mut go = f != 0;
                        if cap >= v2 {
                            let q = ptr + 1;
                            if at(m, q) == c {
                                put(m, q, v2, c);
                                cnt += 1;
                                go = true;
                                hit!(sx_, ny);
                            } else if at(m, q) == alt {
                                if f != 0 {
                                    push!(6, c, cnt, sx14, ny);
                                }
                                c = alt;
                                put(m, q, v2, alt);
                                cnt = 1;
                                go = true;
                                hit!(x + 1, ny);
                            }
                        }
                        if go {
                            push!(6, c, cnt, sx14, ny);
                        }
                    }
                    let ny = s.y - s.len;
                    if y0 <= ny {
                        let x = s.x;
                        let ptr = ny * w + x;
                        let u = val(m, ptr + w);
                        let mut c = at(m, ptr);
                        let v2 = u + 2;
                        let (mut cnt, sx14);
                        if (c == old || c == alt) && v2 <= cap {
                            put(m, ptr, v2, c);
                            (cnt, sx14) = (1, x);
                            hit!(x, ny);
                        } else {
                            (sx14, cnt, c) = (x - 1, 0, old);
                        }
                        let v3 = u + 3;
                        if x0 < x && v3 <= cap && at(m, ptr - 1) == c {
                            put(m, ptr - 1, v3, c);
                            cnt += 1;
                            hit!(x - 1, ny);
                        }
                        if cnt != 0 {
                            push!(2, c, cnt, sx14, ny);
                        }
                    }
                }
                2 => {
                    let nx = s.x + 1;
                    if nx <= x1 {
                        let sy_ = s.y;
                        let ny = s.y + 1;
                        let ptr = ny * w + nx;
                        let u = val(m, ptr - w - 1);
                        let v3 = u + 3;
                        let (mut c, f, mut cnt, sy10);
                        if y1 < ny || at(m, ptr) != old || cap < v3 {
                            (f, cnt, sy10, c) = (0, 0, sy_, old);
                        } else {
                            c = at(m, ptr);
                            put(m, ptr, v3, c);
                            (f, cnt, sy10) = (1, 1, ny);
                            hit!(nx, ny);
                        }
                        let v2 = u + 2;
                        let mut go = f != 0;
                        if cap >= v2 {
                            let q = ptr - w;
                            if at(m, q) == c {
                                put(m, q, v2, c);
                                cnt += 1;
                                go = true;
                                hit!(nx, sy_);
                            } else if at(m, q) == alt {
                                if f != 0 {
                                    push!(0, c, cnt, nx, sy10);
                                }
                                c = alt;
                                put(m, q, v2, alt);
                                cnt = 1;
                                go = true;
                                hit!(nx, ny - 1);
                            }
                        }
                        if go {
                            push!(0, c, cnt, nx, sy10);
                        }
                    }
                    let nx = s.x - s.len;
                    if x0 <= nx {
                        let y = s.y;
                        let ptr = y * w + nx;
                        let mut c = at(m, ptr);
                        let u = val(m, ptr + 1);
                        let v2 = u + 2;
                        let (mut cnt, sy10);
                        if (c == old || c == alt) && v2 <= cap {
                            put(m, ptr, v2, c);
                            (cnt, sy10) = (1, y);
                            hit!(nx, y);
                        } else {
                            (sy10, cnt, c) = (y + 1, 0, old);
                        }
                        let v3 = u + 3;
                        if y < y1 && v3 <= cap && at(m, ptr + w) == c {
                            put(m, ptr + w, v3, c);
                            cnt += 1;
                            hit!(nx, y + 1);
                        }
                        if cnt != 0 {
                            push!(4, c, cnt, nx, sy10);
                        }
                    }
                }
                4 => {
                    let ny = s.y - 1;
                    if y0 <= ny {
                        let sx_ = s.x;
                        let x = s.x + 1;
                        let ptr = ny * w + x;
                        let u = val(m, ptr + w - 1);
                        let v3 = u + 3;
                        let (mut c, f, mut cnt, sx14);
                        if x1 < x || at(m, ptr) != old || cap < v3 {
                            (f, cnt, c, sx14) = (0, 0, old, sx_);
                        } else {
                            c = at(m, ptr);
                            put(m, ptr, v3, c);
                            (f, cnt, sx14) = (1, 1, x);
                            hit!(x, ny);
                        }
                        let v2 = u + 2;
                        let mut go = f != 0;
                        if cap >= v2 {
                            let q = ptr - 1;
                            if at(m, q) == c {
                                put(m, q, v2, c);
                                cnt += 1;
                                go = true;
                                hit!(sx_, ny);
                            } else if at(m, q) == alt {
                                if f != 0 {
                                    push!(2, c, cnt, sx14, ny);
                                }
                                c = alt;
                                put(m, q, v2, alt);
                                cnt = 1;
                                go = true;
                                hit!(x - 1, ny);
                            }
                        }
                        if go {
                            push!(2, c, cnt, sx14, ny);
                        }
                    }
                    let ny = s.y + s.len;
                    if ny <= y1 {
                        let x = s.x;
                        let ptr = ny * w + x;
                        let u = val(m, ptr - w);
                        let mut c = at(m, ptr);
                        let v2 = u + 2;
                        let (mut cnt, sx14);
                        if (c == old || c == alt) && v2 <= cap {
                            put(m, ptr, v2, c);
                            (cnt, sx14) = (1, x);
                            hit!(x, ny);
                        } else {
                            (sx14, cnt, c) = (x + 1, 0, old);
                        }
                        let v3 = u + 3;
                        if x < x1 && v3 <= cap && at(m, ptr + 1) == c {
                            put(m, ptr + 1, v3, c);
                            cnt += 1;
                            hit!(x + 1, ny);
                        }
                        if cnt != 0 {
                            push!(6, c, cnt, sx14, ny);
                        }
                    }
                }
                6 => {
                    let nx = s.x - 1;
                    if x0 <= nx {
                        let sy_ = s.y;
                        let ny = s.y - 1;
                        let ptr = ny * w + nx;
                        let u = val(m, ptr + w + 1);
                        let v3 = u + 3;
                        let (mut c, f, mut cnt, sy10);
                        if ny < y0 || at(m, ptr) != old || cap < v3 {
                            (f, cnt, c, sy10) = (0, 0, old, sy_);
                        } else {
                            c = at(m, ptr);
                            put(m, ptr, v3, c);
                            (f, cnt, sy10) = (1, 1, ny);
                            hit!(nx, ny);
                        }
                        let v2 = u + 2;
                        let mut go = f != 0;
                        if cap >= v2 {
                            let q = ptr + w;
                            if at(m, q) == c {
                                put(m, q, v2, c);
                                cnt += 1;
                                go = true;
                                hit!(nx, sy_);
                            } else if at(m, q) == alt {
                                if f != 0 {
                                    push!(4, c, cnt, nx, sy10);
                                }
                                c = alt;
                                put(m, q, v2, alt);
                                cnt = 1;
                                go = true;
                                hit!(nx, ny + 1);
                            }
                        }
                        if go {
                            push!(4, c, cnt, nx, sy10);
                        }
                    }
                    let nx = s.x + s.len;
                    if nx <= x1 {
                        let y = s.y;
                        let ptr = y * w + nx;
                        let mut c = at(m, ptr);
                        let u = val(m, ptr - 1);
                        let v2 = u + 2;
                        let (mut cnt, sy10);
                        if (c == old || c == alt) && v2 <= cap {
                            put(m, ptr, v2, c);
                            (cnt, sy10) = (1, y);
                            hit!(nx, y);
                        } else {
                            (sy10, cnt, c) = (y - 1, 0, old);
                        }
                        let v3 = u + 3;
                        if y0 < y && v3 <= cap && at(m, ptr - w) == c {
                            put(m, ptr - w, v3, c);
                            cnt += 1;
                            hit!(nx, y - 1);
                        }
                        if cnt != 0 {
                            push!(0, c, cnt, nx, sy10);
                        }
                    }
                }
                _ => {}
            }
        }
        if nxt.is_empty() {
            break;
        }
        cd -= 1;
        if cd < 1 {
            gen += 1;
            break;
        }
        cur = nxt;
        if gen == 0 && (0..8).contains(&d12) {
            let (x, y) = (sx + DIRS[d12 as usize].0, sy + DIRS[d12 as usize].1);
            if x0 <= x && x <= x1 && y0 <= y && y <= y1 {
                let i = (y * w + x) as usize;
                if m[i] as u16 & 0x3fff != 0 {
                    m[i] = m[i].wrapping_sub(1);
                }
            }
        }
        gen += 1;
        if gen >= lim {
            break;
        }
    }
    gen
}

/// 0x5718f0: two straight legs from `s` to `g`, the axis or the diagonal
/// the way leans first (the one nearer `heading` first), every cell
/// stepped on 0; a leg also ends within `tol` of the goal (0x557440).
/// `(clear, break cell, first direction, second direction)`.
fn two_legs(
    m: &[i16],
    w: i32,
    (sx, sy): (i32, i32),
    (gx, gy): (i32, i32),
    heading: i32,
    tol: i32,
) -> (bool, (i32, i32), i32, i32) {
    let (ax, ay) = ((gx - sx).abs(), (gy - sy).abs());
    if ax == 0 && ay == 0 {
        return (true, (sx, sy), heading, heading);
    }
    let pick = |diag: i32, vert: i32, horiz: i32| -> (i32, i32) {
        if ax < 2 * ay {
            if ay < 2 * ax {
                (diag, if ax < ay { vert } else { horiz })
            } else {
                (vert, diag)
            }
        } else {
            (horiz, diag)
        }
    };
    let (mut a, mut b) = match (gy < sy, gx < sx) {
        (true, true) => pick(3, 2, 4),
        (true, false) => pick(1, 2, 0),
        (false, true) => pick(5, 6, 4),
        (false, false) => pick(7, 6, 0),
    };
    if ax == 0 || ay == 0 || ax == ay {
        b = a;
    }
    if (0..8).contains(&heading) && circ(b, heading) < circ(a, heading) {
        std::mem::swap(&mut a, &mut b);
    }
    let run = |d1: i32, d2: i32| -> (bool, (i32, i32)) {
        let (mut x, mut y, mut rx, mut ry) = (sx, sy, ax, ay);
        let (dx, dy) = DIRS[d1 as usize];
        let shut = |x: i32, y: i32| m.get((y * w + x) as usize).is_none_or(|&v| v != 0);
        loop {
            if tol > 0 && dist(x, y, gx, gy) <= tol {
                break;
            }
            x += dx;
            y += dy;
            rx -= dx.abs();
            ry -= dy.abs();
            if shut(x, y) {
                return (false, (x, y));
            }
            if d1 & 1 == 0 {
                if rx == ry {
                    break;
                }
            } else if rx == 0 || ry == 0 {
                break;
            }
        }
        let b = (x, y);
        let (dx, dy) = DIRS[d2 as usize];
        loop {
            if (rx == 0 && ry == 0) || (tol > 0 && dist(x, y, gx, gy) <= tol) {
                return (true, b);
            }
            x += dx;
            y += dy;
            ry -= dy.abs();
            rx -= dx.abs();
            if shut(x, y) {
                return (false, b);
            }
        }
    };
    let (ok, br) = run(a, b);
    if ok || a == b {
        return (ok, br, a, b);
    }
    let (ok, br) = run(b, a);
    (ok, br, b, a)
}

/// The route 0x55b560 over `m` (`w` x `h`, changed: the start opened, the
/// flood written) from `s` to `g` for a walker heading `heading`: entries
/// `(x, y, direction)`, the start first. `None` with an end off the grid,
/// the goal shut or not reached. Two clear legs whose first direction is
/// within 45 degrees of the heading are taken as they are; else a flood
/// from the goal (0x557910, `2 (w + h)` generations, 3 more after the
/// start) and a back-trace from the start, turning at most 45, 90, 135
/// degrees a step, then any way, each step to the least flood value plus
/// its cost, the nearer the goal on a tie, at most `(start - goal) / 2 +
/// 10` entries. (Not decoded: a first step that finds no cell, where
/// 0x55b560 writes its uninitialised best cell, `[esp+0x28]` /
/// `[esp+0x38]` read at 0x55b809 and 0x55b9c1: the port takes it as no
/// route.)
pub fn half_path(
    m: &mut [i16],
    w: i32,
    h: i32,
    (sx, sy): (i32, i32),
    (gx, gy): (i32, i32),
    heading: i32,
) -> Option<Vec<(i32, i32, u8)>> {
    let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
    if !inside(sx, sy) || !inside(gx, gy) {
        return None;
    }
    m[(sy * w + sx) as usize] = 0;
    if m[(gy * w + gx) as usize] != 0 {
        return None;
    }
    let (ok, br, d1, d2) = two_legs(m, w, (sx, sy), (gx, gy), heading, 0);
    if ok && circ(d1, heading) < 2 {
        let (ax, ay) = ((gx - sx).abs(), (gy - sy).abs());
        if ax == 0 && ay == 0 {
            return Some(vec![(sx, sy, heading as u8), (gx, gy, heading as u8)]);
        }
        let mut out = Vec::new();
        let (mut x, mut y, mut d) = (sx, sy, d1);
        for _ in 0..ax.max(ay) + 1 {
            out.push((x, y, d as u8));
            if (x, y) == br {
                d = d2;
            }
            x += DIRS[d as usize].0;
            y += DIRS[d as usize].1;
        }
        return Some(out);
    }
    wave(
        m,
        w,
        (0, 0, w - 1, h - 1),
        (gx, gy),
        (sx, sy),
        (w + h) * 2,
        -1,
        0,
        3,
    );
    let vs = m[(sy * w + sx) as usize] as i32;
    if vs == 0 {
        return None;
    }
    let limit = ((vs - m[(gy * w + gx) as usize] as i32) / 2 + 10) as usize;
    let mut out = Vec::new();
    // The best cell is kept from step to step and pass to pass; its
    // direction starts each pass at 0.
    let mut found: Option<(i32, i32)> = None;
    for (lo, hi) in [(-1, 1), (-2, 2), (-3, 3), (0, 7)] {
        out = vec![(sx, sy, 0u8)];
        let mut prev = heading;
        let (mut cx, mut cy) = (sx, sy);
        let mut bd = 0;
        if (sx, sy) == (gx, gy) {
            continue;
        }
        loop {
            let mut best = 30000;
            for k in lo..=hi {
                let d = (prev + k) & 7;
                let (nx, ny) = (cx + DIRS[d as usize].0, cy + DIRS[d as usize].1);
                if !inside(nx, ny) {
                    continue;
                }
                if (nx, ny) == (gx, gy) {
                    out.push((gx, gy, d as u8));
                    return Some(out);
                }
                let v = m[(ny * w + nx) as usize] as i32;
                if v <= 0 {
                    continue;
                }
                let v = v + STEP_COST[d as usize];
                let nearer = found.is_some_and(|(bx, by)| {
                    v == best && dist(gx, gy, nx, ny) < dist(gx, gy, bx, by)
                });
                if v < best || nearer {
                    best = v;
                    found = Some((nx, ny));
                    bd = d;
                }
            }
            // A first step that found nothing writes the uninitialised
            // best cell (not decoded): no route here. Later ones repeat
            // the cell kept.
            let (bx, by) = found?;
            out.push((bx, by, bd as u8));
            prev = bd;
            if out.len() >= limit {
                break;
            }
            (cx, cy) = (bx, by);
        }
    }
    Some(out)
}

/// The last cell of 0x55ba50's route from `s` towards `g` with tolerance
/// `tol` (all the land creatures use of it): along two clear legs the
/// first cell within `tol` (0x557480) of the goal; else, after a flood
/// from the start (0x557910 with the heading's head start), the ring
/// search about the goal for the reached cell it settles on. `None` for
/// none.
pub fn near_last(
    m: &mut [i16],
    w: i32,
    h: i32,
    (sx, sy): (i32, i32),
    (gx, gy): (i32, i32),
    heading: i32,
    tol: i32,
) -> Option<(i32, i32)> {
    let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
    if !inside(sx, sy) || !inside(gx, gy) {
        return None;
    }
    let (ok, br, d1, d2) = two_legs(m, w, (sx, sy), (gx, gy), heading, tol);
    if ok {
        let (ax, ay) = ((gx - sx).abs(), (gy - sy).abs());
        if ax == 0 && ay == 0 {
            return Some((gx, gy));
        }
        let (mut x, mut y, mut d) = (sx, sy, d1);
        let mut last = (x, y);
        for _ in 0..ax.max(ay) + 1 {
            last = (x, y);
            if dist2(x, y, gx, gy) <= tol {
                return Some((x, y));
            }
            if (x, y) == br {
                d = d2;
            }
            x += DIRS[d as usize].0;
            y += DIRS[d as usize].1;
        }
        return Some(last);
    }
    wave(
        m,
        w,
        (0, 0, w - 1, h - 1),
        (sx, sy),
        (gx, gy),
        (w + h) * 2,
        heading,
        0,
        0,
    );
    if !(m[(gy * w + gx) as usize] < 1 || tol > 0) {
        return Some((gx, gy));
    }
    let (mut best, mut bestv, mut r) = (i32::MAX, i32::MAX, 0);
    let mut chosen = None;
    loop {
        r += 1;
        if !(r <= tol || (r <= best / 2 && (r != 3 || 6 < best))) {
            break;
        }
        let (x0, y0) = ((gx - r).max(0), (gy - r).max(0));
        let (x1, y1) = ((gx + r).min(w - 1), (gy + r).min(h - 1));
        let mut look = |x: i32, y: i32, rd: i32| {
            let v = m[(y * w + x) as usize] as i32;
            if v <= 0 {
                return;
            }
            let take = if r < 3 && r <= tol {
                v < bestv
            } else {
                let q = rd / 2;
                if tol < q {
                    q < best / 2 || (q == best / 2 && v < bestv)
                } else {
                    v < bestv || tol < best / 2
                }
            };
            if take {
                (bestv, best, chosen) = (v, rd, Some((x, y)));
            }
        };
        if r <= x1 - gx {
            let mut rd = 2 * r - y0 + gy;
            for y in y0..=y1 {
                look(x1, y, rd);
                rd += if y < gy { -1 } else { 1 };
            }
        }
        if r <= gy - y0 {
            let mut rd = 2 * r - x0 + gx;
            for x in x0..=x1 {
                look(x, y0, rd);
                rd += if x < gx { -1 } else { 1 };
            }
        }
        if r <= gx - x0 {
            let mut rd = 2 * r - y0 + gy;
            for y in y0..=y1 {
                look(x0, y, rd);
                rd += if y < gy { -1 } else { 1 };
            }
        }
        if r <= y1 - gy {
            let mut rd = 2 * r - x0 + gx;
            for x in x0..=x1 {
                look(x, y1, rd);
                rd += if x < gx { -1 } else { 1 };
            }
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture of walk_half_spec §7 (`#` shut), 10 x 7.
    fn grid() -> Vec<i16> {
        [
            "..........",
            "...##.....",
            "....#..#..",
            ".#..#..#..",
            ".#.....#..",
            "......##..",
            "..........",
        ]
        .iter()
        .flat_map(|r| r.bytes().map(|b| if b == b'#' { -1 } else { 0 }))
        .collect()
    }

    /// 0x558fa0 (0x557910 over the whole grid, no target) from (2, 3),
    /// as the original's machine code fills it.
    #[test]
    fn wave_fills_as_the_original() {
        let mut m = grid();
        wave(&mut m, 10, (0, 0, 9, 6), (2, 3), (-1, -1), 14, -1, 0, 0);
        let want: [[i16; 10]; 7] = [
            [9, 8, 7, 8, 10, 12, 14, 17, 18, 20],
            [7, 6, 5, -1, -1, 13, 14, 15, 17, 19],
            [6, 4, 3, 4, -1, 11, 12, -1, 18, 20],
            [7, -1, 1, 3, -1, 9, 11, -1, 20, 21],
            [9, -1, 3, 4, 6, 8, 10, -1, 19, 20],
            [8, 6, 5, 6, 7, 9, -1, -1, 17, 19],
            [9, 8, 7, 8, 9, 10, 12, 14, 16, 18],
        ];
        assert_eq!(m, want.concat());
        let mut m = grid();
        wave(&mut m, 10, (0, 0, 9, 6), (2, 3), (-1, -1), 4, -1, 0, 0);
        let want: [[i16; 10]; 7] = [
            [9, 8, 7, 8, 10, 0, 0, 0, 0, 0],
            [7, 6, 5, -1, -1, 0, 0, 0, 0, 0],
            [6, 4, 3, 4, -1, 0, 0, -1, 0, 0],
            [7, -1, 1, 3, -1, 9, 0, -1, 0, 0],
            [9, -1, 3, 4, 6, 8, 10, -1, 0, 0],
            [8, 6, 5, 6, 7, 9, -1, -1, 0, 0],
            [9, 8, 7, 8, 9, 10, 0, 0, 0, 0],
        ];
        assert_eq!(m, want.concat());
    }

    /// 0x55b560's routes on the fixture, as the original's code gives them.
    #[test]
    fn half_path_as_the_original() {
        let route = |s, g, h| half_path(&mut grid(), 10, 7, s, g, h).unwrap();
        assert_eq!(
            route((2, 3), (8, 3), 0),
            [
                (2, 3, 0),
                (3, 4, 7),
                (4, 4, 0),
                (5, 3, 1),
                (6, 2, 1),
                (7, 1, 1),
                (8, 2, 7),
                (8, 3, 6)
            ]
        );
        assert_eq!(
            route((2, 3), (8, 3), 4),
            [
                (2, 3, 0),
                (1, 2, 3),
                (1, 1, 2),
                (2, 0, 1),
                (3, 0, 0),
                (4, 0, 0),
                (5, 1, 7),
                (6, 1, 0),
                (7, 1, 0),
                (8, 2, 7),
                (8, 3, 6)
            ]
        );
        assert_eq!(
            route((0, 0), (9, 6), 7),
            [
                (0, 0, 7),
                (1, 1, 7),
                (2, 2, 7),
                (3, 3, 7),
                (4, 4, 7),
                (5, 5, 7),
                (6, 6, 7),
                (7, 6, 0),
                (8, 6, 0),
                (9, 6, 0)
            ]
        );
        let down: Vec<(i32, i32, u8)> = (0..7).map(|y| (5, y, 6)).collect();
        assert_eq!(route((5, 0), (5, 6), 6), down);
        assert_eq!(
            route((9, 6), (0, 3), 2),
            [
                (9, 6, 0),
                (8, 5, 3),
                (8, 4, 2),
                (8, 3, 2),
                (8, 2, 2),
                (8, 1, 2),
                (7, 0, 3),
                (6, 0, 4),
                (5, 0, 4),
                (4, 0, 4),
                (3, 0, 4),
                (2, 1, 5),
                (1, 2, 5),
                (0, 3, 5)
            ]
        );
    }

    /// 0x55ba50's last cell with tolerance 5 on the fixture.
    #[test]
    fn near_last_as_the_original() {
        let near = |s, g, h| near_last(&mut grid(), 10, 7, s, g, h, 5);
        assert_eq!(near((0, 0), (9, 6), 0), Some((5, 5)));
        assert_eq!(near((2, 3), (8, 3), 4), Some((3, 3)));
        assert_eq!(near((9, 0), (2, 3), 4), Some((6, 0)));
    }
}
