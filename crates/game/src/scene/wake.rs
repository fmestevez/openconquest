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
/// The side of a ship type's frame (its `SHIP_` composite), centred on the
/// ship.
const FRAME: [f32; 12] = [
    56.0, 64.0, 80.0, 80.0, 72.0, 72.0, 72.0, 72.0, 64.0, 72.0, 72.0, 72.0,
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

/// Seconds between pairs of droplets, a droplet's frame, and how fast
/// droplets drift outwards and along. Estimates.
const EVERY: f32 = 0.04;
const FRAME_SECONDS: f32 = 0.06;
const OUTWARDS: f32 = 30.0;
/// Droplets start along with the ship at this share of its speed (the
/// original gives them its speed ahead, 0x4a3f40), so they leave the stern
/// and fall behind as they slow.
const KEEP_UP: f32 = 0.8;

#[derive(Debug, Clone)]
pub struct Droplet {
    pub x: f32,
    pub y: f32,
    vx: f32,
    vy: f32,
    pub age: f32,
    /// `TRAKS00` (true) or `TRAKS14`.
    pub small: bool,
}

impl Droplet {
    /// Its frame of `frames` frames, while it lasts.
    pub fn frame(&self, frames: usize) -> Option<usize> {
        let f = (self.age / FRAME_SECONDS) as usize;
        (f < frames).then_some(f)
    }
}

impl Scene {
    /// Ships under way throw off droplets; droplets drift and fade.
    pub(super) fn run_wakes(&mut self, dt: f32) {
        let (small, big) = (self.wake_art.0.len(), self.wake_art.1.len());
        self.droplets.retain_mut(|d| {
            d.age += dt;
            d.x += d.vx * dt;
            d.y += d.vy * dt;
            // Slowing as they spread.
            d.vx *= 1.0 - 2.0 * dt;
            d.vy *= 1.0 - 2.0 * dt;
            d.frame(if d.small { small } else { big }).is_some()
        });
        self.wake_clock += dt;
        if self.wake_clock < EVERY {
            return;
        }
        self.wake_clock -= EVERY;
        let mut new = Vec::new();
        for o in &self.objects {
            let Some(s) = &o.ship else { continue };
            if s.path.is_empty() || s.rotation.is_some() {
                continue;
            }
            let k = s.kind as usize % 12;
            let h = s.heading % 8;
            let (sx, sy) = STERN[k][h];
            let (ax, ay) = AHEAD[h];
            let norm = (ax as f32).hypot(ay as f32).max(1.0);
            let along = s.speed * KEEP_UP / norm;
            let (px, py) = SIDE[h];
            let spread = SPREAD[k] as f32 + if h == 2 || h == 6 { 1.0 } else { 0.0 };
            let half = FRAME[k] / 2.0;
            let (x, y) = (o.x - half + sx as f32, o.y - half + sy as f32);
            for side in [1.0, -1.0] {
                let (px, py) = (px as f32 * side, py as f32 * side);
                new.push(Droplet {
                    x: x + px * spread,
                    y: y + py * spread,
                    vx: px * OUTWARDS + ax as f32 * along,
                    vy: py * OUTWARDS + ay as f32 * along,
                    age: 0.0,
                    small: SMALL[k],
                });
            }
        }
        self.droplets.extend(new);
    }
}
