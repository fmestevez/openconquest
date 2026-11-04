//! A ship's way and its motion between route points, a game tick at a time,
//! in the original's integers (the move tick 0x4a3f40, the speeds of
//! 0x4ac210 / 0x4bece0 and the brake 0x4a2f60).
//!
//! The way (`+0x59b`) counts units of 1/6000 pixel a tick: a straight step
//! moves the picture half a pixel per 3000 units, a slanting one a third of
//! a pixel on each axis, the rest carried in `+0x597`.

/// Top speeds by ship type (0x5bb4e0), x 400 at start-up (0x4ac210).
const TOP: [i32; 12] = [150, 140, 100, 60, 90, 150, 120, 130, 90, 100, 120, 100];
/// Acceleration by ship type (0x5bb510), x 8 (0x4ac210, 0x4a3a70).
const ACCEL: [i32; 12] = [150, 140, 80, 40, 90, 120, 120, 130, 90, 100, 120, 100];
/// The way a charmed ship's top speed is set to (0x4a74c0 at 0x4a75af).
pub const CHARMED_TOP: i32 = 7000;

/// A ship type's top speed with its owner's rigging level (0x4ac210: the
/// rows 0x5c750c, x 115 / 100 at 0x5c753c and x 130 / 100 at 0x5c756c,
/// by the player block's `+0x51`).
pub fn base_top(kind: u8, rigging: u8) -> i32 {
    let top = TOP[kind as usize % 12] * 400;
    match rigging {
        0 => top,
        1 => top * 115 / 100,
        _ => top * 130 / 100,
    }
}

/// `+0x59f` as 0x4bece0 sets it: the base top speed less half the
/// percentage of hull lost, `d = 100 - hull x 100 / full` (0x5c76b0), in
/// integers. (Its armour (`+0x27a`) and feathers (`+0x489` kind 12) change
/// only `+0x27c`, which the mover does not read.)
pub fn top_speed(kind: u8, rigging: u8, hull: i32) -> i32 {
    let full = crate::combat::hull_points(kind) as i32;
    let d = 100 - hull * 100 / full.max(1);
    let top = base_top(kind, rigging);
    top - top * d / 200
}

/// `+0x280` as 0x4a3a70 sets it on every step it grants: 0x5bb510 x 8.
pub fn accel(kind: u8) -> i32 {
    ACCEL[kind as usize % 12] * 8
}

/// What the move tick steps the way by.
pub struct Sailing {
    /// `+0x59f`.
    pub top: i32,
    /// `+0x280`.
    pub accel: i32,
    /// `+0x284`.
    pub brake: i32,
    /// The wind's percentage on the ship's heading (0x597b2c), `None` with
    /// Zetes aboard (0x4ba790 with 3: neither scaled).
    pub wind: Option<i32>,
    /// Move state 4: braking by `+0x284`.
    pub braking: bool,
    /// Limed (`+0x258`): the way and its step halved after the cap.
    pub limed: bool,
}

/// The way after a tick (0x4a3f40 at 0x4a3fa8-0x4a423c, one tick an
/// update, `[clock+0xe2]` 1): the acceleration (braking, `+0x284`) and the
/// cap scaled by the wind / 100; braking to below 1 stops it (and clears
/// `+0x284`, the second value); capped; limed, halved, the step too; and
/// braking it is never brought to a tenth of the top speed or under (the
/// way is then kept).
pub fn next_way(way: i32, s: &Sailing) -> (i32, bool) {
    let scale = |v: i32| s.wind.map_or(v, |w| w.wrapping_mul(v) / 100);
    let cap = scale(s.top);
    let mut a = scale(if s.braking { s.brake } else { s.accel });
    let mut v = a.wrapping_add(way);
    let mut cleared = false;
    if v < 1 && s.braking {
        v = 0;
        cleared = true;
    }
    if cap < v {
        v = cap;
    }
    if s.limed {
        v /= 2;
        a /= 2;
    }
    if v <= s.top / 10 && a < 0 {
        v = way;
    }
    (v, cleared)
}

/// A tick's move at `way` with `rest` carried (`+0x597`) along heading
/// `heading` (0x5c4f4c: 0 east, counter-clockwise, y down): `way + rest`
/// in steps of 3000, the rest kept; straight on, half a pixel a step on its
/// axis, an odd step's half carried back; slanting, a third on each axis,
/// the steps left over carried back (0x4a4243-0x4a432f). The pixels moved
/// and the new rest.
pub fn step(way: i32, rest: i32, heading: usize) -> ((i32, i32), i32) {
    let n = way.wrapping_add(rest);
    let mut rest = n % 3000;
    let s = n / 3000;
    if s == 0 {
        return ((0, 0), rest);
    }
    let (ux, uy) = super::grapple::UNIT[heading % 8];
    if ux == 0 || uy == 0 {
        rest += s % 2 * 3000;
        if ux == 0 {
            ((0, s * uy / 2), rest)
        } else {
            ((s * ux / 2, 0), rest)
        }
    } else {
        rest += s % 3 * 3000;
        ((s * ux / 3, s * uy / 3), rest)
    }
}

/// The picture moved to `to` held at the route point `point` on the axes
/// it moves along (0x4a4335-0x4a43d1): not past it on either; moving
/// along the x axis only, its y is set to the point's.
pub fn clamp(to: (i32, i32), point: (i32, i32), heading: usize) -> (i32, i32) {
    let (ux, uy) = super::grapple::UNIT[heading % 8];
    let (mut x, mut y) = to;
    if (ux > 0 && x > point.0) || (ux < 0 && x < point.0) {
        x = point.0;
    }
    if uy == 0 {
        if ux != 0 && y != point.1 {
            y = point.1;
        }
    } else if (uy > 0 && y > point.1) || (uy < 0 && y < point.1) {
        y = point.1;
    }
    (x, y)
}

/// Whether a tick at the way `next_way` gives reaches `point` from
/// `at` (0x4a68b0 past its move-state test): the step's picture on or past
/// the point on an axis the heading moves along.
pub fn reaches(at: (i32, i32), point: (i32, i32), heading: usize, way: i32, rest: i32) -> bool {
    let ((dx, dy), _) = step(way, rest, heading);
    if (dx, dy) == (0, 0) {
        return false;
    }
    let (ux, uy) = super::grapple::UNIT[heading % 8];
    let (x, y) = (at.0 + dx, at.1 + dy);
    (ux > 0 && x >= point.0)
        || (ux < 0 && x <= point.0)
        || (uy > 0 && y >= point.1)
        || (uy < 0 && y <= point.1)
}

/// The brake 0x4a2f60 (move state not 4): the distance to stop in, the
/// larger axis of the picture's centre `at` to the middle of its cell
/// `own` plus a cell when the cell it has reserved (`+0x5b6`, `entering`)
/// is its own; to its own cell's middle when it is not between cells; else
/// to `+0x5b6`'s cell number (y x width + x) x 64 + 32 on both axes, as the
/// original reads it (0x4a2fff). `Some(+0x284)`, `way x way / (distance x
/// -6000)` in 32 bits, with the state then 4; `None` for no distance (the
/// state 0 with its extra cell released, then 4 all the same).
pub fn brake(
    way: i32,
    at: (i32, i32),
    own: (i32, i32),
    entering: (i32, i32),
    between: bool,
    width: i32,
) -> Option<i32> {
    let mid = |c: i32| c * 64 + 32;
    let d = if entering == own {
        ((at.0 - mid(own.0)).abs() + 64).max((at.1 - mid(own.1)).abs() + 64)
    } else {
        let (mx, my) = if between {
            let m = mid(entering.1.wrapping_mul(width).wrapping_add(entering.0));
            (m, m)
        } else {
            (mid(own.0), mid(own.1))
        };
        (at.0.wrapping_sub(mx).wrapping_abs()).max(at.1.wrapping_sub(my).wrapping_abs())
    };
    (d.wrapping_mul(3000) != 0).then(|| way.wrapping_mul(way) / d.wrapping_mul(-6000))
}

/// A ship's hull in the original's points (`+0x271`).
pub fn hull_of(c: &crate::combat::Combatant, kind: u8) -> i32 {
    let full = crate::combat::hull_points(kind) as f32;
    (c.hull / c.max_hull.max(1.0) * full).round() as i32
}

impl super::Scene {
    /// Ship `i`'s top speed `+0x59f` (`top_speed` by its owner's rigging
    /// and its hull; 7000 under a siren's song, 0x4a74c0).
    pub(super) fn ship_top(&self, i: usize) -> i32 {
        let o = &self.objects[i];
        let Some(s) = o.ship.as_ref() else {
            return 0;
        };
        if self.charmed.contains_key(&o.id) {
            return CHARMED_TOP;
        }
        let rigging = o
            .owner
            .and_then(|w| self.levels.get(&w))
            .map_or(0, |l| l.speed);
        let hull = o
            .combat
            .as_ref()
            .map_or(crate::combat::hull_points(s.kind) as i32, |c| {
                hull_of(c, s.kind)
            });
        top_speed(s.kind, rigging, hull)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0x4ac210's rows: top speed (0x5bb4e0 x 400) and acceleration
    /// (0x5bb510 x 8) by ship type; measured live, a boat 60000 / 1200 and
    /// a merchant 56000 / 1120.
    #[test]
    fn speeds_by_ship_type() {
        let tops: Vec<i32> = (0..12).map(|k| base_top(k, 0)).collect();
        assert_eq!(
            tops,
            [60000, 56000, 40000, 24000, 36000, 60000, 48000, 52000, 36000, 40000, 48000, 40000]
        );
        let accels: Vec<i32> = (0..12).map(accel).collect();
        assert_eq!(
            accels,
            [1200, 1120, 640, 320, 720, 960, 960, 1040, 720, 800, 960, 800]
        );
        assert_eq!(base_top(1, 1), 64400);
        assert_eq!(base_top(1, 2), 72800);
    }

    /// One tick's way and move (0x4a3f40): calm (60%) a boat gains 720 a
    /// tick up to 36000; braking never takes it to a tenth of its top
    /// speed; limed, halved; straight on 3000 units a half pixel, an odd
    /// half carried back; slanting a third on each axis.
    #[test]
    fn a_tick_of_way() {
        let mut s = Sailing {
            top: 60000,
            accel: 1200,
            brake: -500,
            wind: Some(60),
            braking: false,
            limed: false,
        };
        assert_eq!(next_way(0, &s), (720, false));
        assert_eq!(next_way(35900, &s), (36000, false));
        s.wind = None;
        assert_eq!(next_way(59000, &s), (60000, false));
        s.braking = true;
        assert_eq!(next_way(6400, &s), (6400, false));
        assert_eq!(next_way(7000, &s), (6500, false));
        s.limed = true;
        s.braking = false;
        assert_eq!(next_way(1000, &s), (1100, false));
        assert_eq!(step(9000, 0, 0), ((1, 0), 3000));
        assert_eq!(step(9000, 2999, 2), ((0, -1), 5999));
        assert_eq!(step(9000, 0, 5), ((-1, 1), 0));
        assert_eq!(step(2999, 0, 0), ((0, 0), 2999));
        assert_eq!(clamp((100, 40), (96, 32), 0), (96, 32));
        assert_eq!(clamp((90, 20), (96, 32), 7), (90, 20));
        assert!(reaches((90, 32), (96, 32), 0, 36000, 0));
        assert!(!reaches((80, 32), (96, 32), 0, 36000, 0));
    }

    /// 0x4a2f60: in its own cell the distance gains a cell; the brake is
    /// way x way / (distance x -6000).
    #[test]
    fn brakes_as_the_original() {
        assert_eq!(
            brake(13716, (100, 32), (1, 0), (1, 0), false, 50),
            Some(-461)
        );
        assert_eq!(
            brake(13716, (100, 32), (1, 0), (2, 0), false, 50),
            Some(-7838)
        );
        // Between cells the reserved cell's number (52) stands for both
        // axes: 3360 - 32.
        assert_eq!(brake(6000, (100, 32), (1, 0), (2, 1), true, 50), Some(-1));
    }
}
