//! The wake of ships under way (0x4a3f40): droplets (`TRAKS<type>`) thrown
//! off both sides of the stern that drift outwards and fade. Only shown.

use super::*;

/// Where the stern is, from the top-left of the ship's frame, by ship type
/// and heading (0x5bb540).
const STERN: [[(i8, i8); 8]; 12] = [
    [
        (3, 34),
        (9, 48),
        (28, 54),
        (48, 48),
        (54, 35),
        (49, 13),
        (28, 4),
        (9, 13),
    ],
    [
        (2, 36),
        (11, 53),
        (31, 60),
        (54, 54),
        (62, 39),
        (53, 15),
        (31, 4),
        (8, 16),
    ],
    [
        (9, 46),
        (16, 63),
        (40, 73),
        (67, 64),
        (75, 45),
        (64, 26),
        (41, 12),
        (16, 25),
    ],
    [
        (4, 39),
        (15, 64),
        (39, 70),
        (68, 63),
        (77, 41),
        (62, 20),
        (40, 10),
        (17, 20),
    ],
    [
        (6, 43),
        (17, 59),
        (36, 66),
        (57, 61),
        (65, 43),
        (56, 24),
        (36, 14),
        (17, 23),
    ],
    [
        (4, 38),
        (12, 55),
        (36, 67),
        (60, 54),
        (67, 42),
        (59, 17),
        (36, 3),
        (12, 14),
    ],
    [
        (5, 37),
        (13, 57),
        (36, 65),
        (61, 55),
        (68, 35),
        (58, 16),
        (36, 6),
        (13, 16),
    ],
    [
        (5, 39),
        (13, 57),
        (37, 63),
        (59, 57),
        (67, 40),
        (58, 21),
        (36, 11),
        (16, 20),
    ],
    [
        (4, 32),
        (11, 50),
        (33, 58),
        (54, 52),
        (61, 32),
        (51, 15),
        (33, 6),
        (13, 11),
    ],
    [
        (7, 39),
        (15, 56),
        (37, 64),
        (58, 60),
        (66, 41),
        (55, 22),
        (37, 14),
        (18, 21),
    ],
    [
        (3, 34),
        (11, 52),
        (32, 58),
        (53, 53),
        (60, 37),
        (53, 15),
        (32, 5),
        (11, 15),
    ],
    [
        (6, 38),
        (14, 61),
        (36, 67),
        (58, 61),
        (68, 44),
        (57, 21),
        (36, 9),
        (17, 17),
    ],
];
/// The speed above which a ship type throws a wake (0x5bb1f4).
const WAKE_FROM: [u32; 12] = [
    26000, 25000, 20000, 13000, 18000, 25000, 23000, 23000, 18000, 20000, 22000, 20000,
];
/// How far apart the two droplets start, by ship type (0x5bb840).
const SPREAD: [i8; 12] = [7, 9, 11, 13, 11, 9, 9, 9, 7, 8, 12, 8];
/// The droplets' sprites by ship type (0x5bb224): `TRAKS00` or `TRAKS14`.
const SMALL: [bool; 12] = [
    true, true, false, false, false, false, false, false, true, false, false, false,
];
/// A heading's way ahead and its two sides (0x4a76a0).
const AHEAD: [(i8, i8); 8] = [
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];
const SIDE: [(i8, i8); 8] = [
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];

/// A droplet steps a frame each tick and goes after its last
/// (TraksClassTy 0x48f440, frame delay 1); pairs come every tick.
const EVERY: f32 = crate::clock::TICK;

/// A droplet of a ship's wake (TraksClassTy): it stays where it was thrown
/// and drifts by v t / 10 + a t squared / 20 pixels after t ticks (0x48f440).
#[derive(Debug, Clone)]
pub struct Droplet {
    x0: f32,
    y0: f32,
    /// Pixels a tick and a tick squared (v / 10, a / 20 of the original).
    v: (f32, f32),
    a: (f32, f32),
    pub age: f32,
    /// `TRAKS00` (true) or `TRAKS14`.
    pub small: bool,
}

impl Droplet {
    /// Its frame of `frames` frames, while it lasts.
    pub fn frame(&self, frames: usize) -> Option<usize> {
        let f = (self.age / crate::clock::TICK) as usize;
        (f < frames).then_some(f)
    }

    /// Where it is now.
    pub fn at(&self) -> (f32, f32) {
        let t = self.age / crate::clock::TICK;
        (
            self.x0 + self.v.0 * t + self.a.0 * t * t,
            self.y0 + self.v.1 * t + self.a.1 * t * t,
        )
    }
}

impl Scene {
    /// Ships under way throw off droplets (0x4a3f40); droplets drift and
    /// fade.
    pub(super) fn run_wakes(&mut self, dt: f32) {
        let (small, big) = (self.wake_art.0.len(), self.wake_art.1.len());
        self.droplets.retain_mut(|d| {
            d.age += dt;
            d.frame(if d.small { small } else { big }).is_some()
        });
        self.wake_clock += dt;
        if self.wake_clock < EVERY {
            return;
        }
        self.wake_clock -= EVERY;
        let mut new = Vec::new();
        let rng = &mut self.gull_rng;
        for o in &self.objects {
            let Some(s) = &o.ship else { continue };
            // In the original's units: 1/6000 pixel a tick.
            let speed = (s.velocity * crate::clock::TICK * 6000.0) as i32;
            // Only going faster than its type's threshold (0x5bb1f4).
            if s.path.is_empty()
                || s.rotation.is_some()
                || speed <= WAKE_FROM[s.kind as usize % 12] as i32
            {
                continue;
            }
            let k = s.kind as usize % 12;
            let h = s.heading % 8;
            let (sx, sy) = STERN[k][h];
            let (ax, ay) = (AHEAD[h].0 as i32, AHEAD[h].1 as i32);
            let (px, py) = (SIDE[h].0 as i32, SIDE[h].1 as i32);
            // A little apart, by chance (0x4a3f40: rand & 3, less 2).
            let spread =
                (rng.next() & 3) as i32 - 2 + SPREAD[k] as i32 + i32::from(h == 2 || h == 6);
            let (ahead, out, accel) = (speed / 9000, speed / 3500, speed / 9500);
            let half = objects::SHIP_FRAME[k] / 2.0;
            let (x, y) = (o.x - half + sx as f32, o.y - half + sy as f32);
            for side in [1, -1] {
                let (px, py) = (px * side, py * side);
                new.push(Droplet {
                    x0: x + (px * spread) as f32,
                    y0: y + (py * spread) as f32,
                    v: (
                        ((ax * ahead + px * out) / 2) as f32 / 10.0,
                        ((ay * ahead + py * out) / 2) as f32 / 10.0,
                    ),
                    a: ((px * accel) as f32 / 20.0, (py * accel) as f32 / 20.0),
                    age: 0.0,
                    small: SMALL[k],
                });
            }
        }
        self.droplets.extend(new);
    }
}
