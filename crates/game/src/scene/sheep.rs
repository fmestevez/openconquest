//! Sheep (0x4331c0): each flock's sheep stand on a 4 x 4 grid of 16-pixel
//! spots in their cell, one to a spot. A sheep grazes (`SHEEP_<dir>1`) a
//! pass at a time, a frame every 2-4 ticks, resting 12-25 ticks between
//! passes; every 125-250 ticks it may walk (`SHEEP_<dir>2`, a frame every
//! 3 ticks) to a free spot beside it. Only shown.

use std::collections::HashMap;

use super::*;

/// The eight directions (0x5c4f4c): 0 east, counter-clockwise.
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

#[derive(Debug, Clone)]
pub struct Sheep {
    /// Its spot on the cell's grid, and where it walks to.
    spot: (i32, i32),
    to: (i32, i32),
    /// Where it is, in world pixels (the spot's middle while it stands).
    pub x: f32,
    pub y: f32,
    pub dir: usize,
    pub walking: bool,
    pub frame: usize,
    /// Ticks: its last frame, its next grazing pass, its next walk.
    last: u32,
    next_pass: u32,
    next_walk: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Flock {
    pub sheep: Vec<Sheep>,
    taken: [bool; 16],
}

/// 50 to 100 percent of `ticks`, at random.
fn about(rng: &mut Rng, ticks: u32) -> u32 {
    (rng.next() % 51 + 50) * ticks / 100
}

impl Scene {
    /// Moves the flocks a tick at a time.
    pub(super) fn run_sheep(&mut self, dt: f32) {
        self.sheep_clock += dt;
        while self.sheep_clock >= crate::clock::TICK {
            self.sheep_clock -= crate::clock::TICK;
            self.sheep_ticks += 1;
            self.sheep_tick();
        }
    }

    fn sheep_tick(&mut self) {
        let now = self.sheep_ticks;
        let t = TILE_SIZE as f32;
        let art = self.sheep_art.clone();
        let flocks: Vec<(u32, (i32, i32), usize)> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Sheep)
            .filter_map(|o| {
                let f = o.record.fields.get(4..8)?;
                Some((o.id, (f[0] as i32, f[1] as i32), f[3].clamp(1, 16) as usize))
            })
            .collect();
        let rng = &mut self.gull_rng;
        for (id, (cx, cy), count) in flocks {
            let flock = self.flocks.entry(id).or_insert_with(|| {
                // Each on a free spot, facing anywhere (0x4331c0, state 2).
                let mut flock = Flock::default();
                for _ in 0..count {
                    let spot = loop {
                        let s = ((rng.next() & 3) as i32, (rng.next() & 3) as i32);
                        if !flock.taken[(s.0 + s.1 * 4) as usize] {
                            break s;
                        }
                    };
                    flock.taken[(spot.0 + spot.1 * 4) as usize] = true;
                    let dir = (rng.next() & 7) as usize;
                    flock.sheep.push(Sheep {
                        spot,
                        to: spot,
                        x: cx as f32 * t + spot.0 as f32 * 16.0 + 8.0,
                        y: cy as f32 * t + spot.1 as f32 * 16.0 + 8.0,
                        dir,
                        walking: false,
                        frame: 0,
                        last: now,
                        next_pass: now + about(rng, 25),
                        next_walk: now + about(rng, 250),
                    });
                }
                flock
            });
            for k in 0..flock.sheep.len() {
                let s = &mut flock.sheep[k];
                let frames =
                    |walking: bool, dir: usize| art[dir][usize::from(walking)].len().max(1);
                if !s.walking {
                    if now < s.next_walk || s.frame != 0 {
                        // Grazing passes, a frame every 2-4 ticks.
                        if s.next_pass <= now && s.last + rng.next() % 3 + 2 <= now {
                            s.last = now;
                            s.frame += 1;
                            if s.frame >= frames(false, s.dir) {
                                s.frame = 0;
                                s.next_pass = now + about(rng, 25);
                            }
                        }
                        continue;
                    }
                    // Time to walk: to a free spot beside it, if any.
                    let free: Vec<usize> = (0..8)
                        .filter(|&d| {
                            let (x, y) = (s.spot.0 + DIRS[d].0, s.spot.1 + DIRS[d].1);
                            (0..4).contains(&x)
                                && (0..4).contains(&y)
                                && !flock.taken[(x + y * 4) as usize]
                        })
                        .collect();
                    if free.is_empty() {
                        s.next_walk = now + about(rng, 250);
                        continue;
                    }
                    let d = free[rng.next() as usize % free.len()];
                    s.to = (s.spot.0 + DIRS[d].0, s.spot.1 + DIRS[d].1);
                    flock.taken[(s.to.0 + s.to.1 * 4) as usize] = true;
                    s.dir = d;
                    s.walking = true;
                    s.frame = 0;
                    s.last = now;
                } else if s.last + 3 <= now {
                    // A step a frame, 16 / (frames + 1) pixels.
                    s.last = now;
                    s.frame += 1;
                    let n = frames(true, s.dir);
                    if s.frame < n {
                        let step = (16 / (n as i32 + 1)) as f32;
                        s.x += DIRS[s.dir].0 as f32 * step;
                        s.y += DIRS[s.dir].1 as f32 * step;
                        let at = (
                            ((s.x - cx as f32 * t) / 16.0).floor() as i32,
                            ((s.y - cy as f32 * t) / 16.0).floor() as i32,
                        );
                        if at != s.spot {
                            flock.taken[(s.spot.0 + s.spot.1 * 4) as usize] = false;
                            s.spot = s.to;
                        }
                    } else {
                        flock.taken[(s.spot.0 + s.spot.1 * 4) as usize] = false;
                        s.spot = s.to;
                        flock.taken[(s.spot.0 + s.spot.1 * 4) as usize] = true;
                        s.x = cx as f32 * t + s.spot.0 as f32 * 16.0 + 8.0;
                        s.y = cy as f32 * t + s.spot.1 as f32 * 16.0 + 8.0;
                        s.walking = false;
                        s.frame = 0;
                        s.next_pass = now + about(rng, 25);
                        s.next_walk = now + about(rng, 250);
                    }
                }
            }
        }
    }
}

pub type Flocks = HashMap<u32, Flock>;
