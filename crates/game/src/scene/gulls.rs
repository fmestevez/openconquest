//! Seagulls crossing the map now and then, their shadows on the water below
//! (GullObjClassTy, artem/Gull.cpp 0x41c700). Only shown.

use super::*;

/// Seconds a step: a gull moves every other tick.
const STEP_SECONDS: f32 = 2.0 * crate::clock::TICK;
/// How far below a gull its shadow falls.
const SHADOW_DROP: f32 = 120.0;

/// One gull: waiting off the map, or crossing it.
#[derive(Debug, Clone)]
pub struct Gull {
    /// Seconds until it sets off, while waiting.
    wait: f32,
    /// Its heading (1, 3, 5 or 7: the diagonals, 0 east counting
    /// counter-clockwise), while flying.
    heading: Option<usize>,
    pub x: f32,
    pub y: f32,
    step: usize,
    clock: f32,
}

impl Gull {
    /// The frame of `GULLSPR` and `GULLSHD_SPR` it shows (ten a heading),
    /// if flying.
    pub fn frame(&self) -> Option<usize> {
        self.heading.map(|h| h * 10 + self.step % 10)
    }
}

impl Scene {
    /// The gulls of a map `w` x `h` cells: one for every 225 cells and one
    /// more, at most 25, each waiting 7.5 to 15 seconds.
    pub(super) fn new_gulls(w: u32, h: u32, rng: &mut Rng) -> Vec<Gull> {
        let n = ((w * h) / 225 + 1).min(25);
        (0..n)
            .map(|_| Gull {
                wait: gull_wait(rng),
                heading: None,
                x: 0.0,
                y: 0.0,
                step: 0,
                clock: 0.0,
            })
            .collect()
    }

    /// Moves the gulls: a waiting one sets off along a diagonal from the
    /// map's edge it comes from; a flying one goes 2 pixels each way a step
    /// until it and its shadow are off the map.
    pub(super) fn move_gulls(&mut self, dt: f32) {
        let t = TILE_SIZE as f32;
        let (mw, mh) = (self.nav.width as f32 * t, self.nav.height as f32 * t);
        let rng = &mut self.gull_rng;
        for g in &mut self.gulls {
            let Some(heading) = g.heading else {
                g.wait -= dt;
                if g.wait <= 0.0 {
                    let heading = (rng.next() as usize & 3) * 2 + 1;
                    let along = |rng: &mut Rng| (rng.next() % mh.max(1.0) as u32) as f32;
                    (g.x, g.y) = match heading {
                        1 => (0.0, along(rng)),
                        3 => (along(rng), mh),
                        5 => (mw, along(rng)),
                        _ => (along(rng), 0.0),
                    };
                    g.heading = Some(heading);
                    g.step = 0;
                    g.clock = 0.0;
                }
                continue;
            };
            let (dx, dy) = [
                (1, 0),
                (1, -1),
                (0, -1),
                (-1, -1),
                (-1, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
            ][heading];
            g.clock += dt;
            while g.clock >= STEP_SECONDS {
                g.clock -= STEP_SECONDS;
                g.x += dx as f32 * 2.0;
                g.y += dy as f32 * 2.0;
                g.step += 1;
            }
            let off = |x: f32, y: f32| x < -32.0 || y < -32.0 || x > mw + 32.0 || y > mh + 32.0;
            if off(g.x, g.y) && off(g.x, g.y + SHADOW_DROP) {
                g.heading = None;
                g.wait = gull_wait(rng);
            }
        }
    }

    /// Where gull `g`'s shadow lies.
    pub fn gull_shadow(g: &Gull) -> (f32, f32) {
        (g.x, g.y + SHADOW_DROP)
    }
}

/// 50 to 100 times 7.5 ticks.
fn gull_wait(rng: &mut Rng) -> f32 {
    crate::clock::ticks((rng.next() % 51 + 50) * 750 / 100)
}
