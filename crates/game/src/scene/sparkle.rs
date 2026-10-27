//! Amber deposits glint now and then (AmberClassTy 0x48a430, state 0;
//! 0x489bd0): after a wait that is shorter the bigger the deposit, the
//! sparkle `ASTAR_` plays once at a random spot of one of the variant's two
//! rectangles, a frame every 1 to 4 ticks. Only shown.

use std::collections::HashMap;

use super::*;
use crate::clock;

/// Ticks between glints by deposit size 1-3, from and to (0x5b97a8).
const WAIT: [(u32, u32); 3] = [(10, 60), (5, 30), (1, 10)];

#[derive(Debug, Clone, Default)]
pub struct Sparkle {
    /// Seconds until the next glint.
    wait: f32,
    /// A glint playing: its age, seconds a frame, and where.
    pub playing: Option<(f32, f32, f32, f32)>,
}

impl Sparkle {
    /// The frame of `frames` it shows, and where, while it plays.
    pub fn frame(&self, frames: usize) -> Option<(usize, f32, f32)> {
        let (age, step, x, y) = self.playing?;
        let f = (age / step) as usize;
        (f < frames).then_some((f, x, y))
    }
}

impl Scene {
    /// Times the deposits' glints.
    pub(super) fn run_sparkles(&mut self, dt: f32) {
        let t = TILE_SIZE as f32;
        let frames = self.sparkle_art.len();
        let mut seen = Vec::new();
        for o in &self.objects {
            let Some(dep) = o.deposit.filter(|_| o.class == ObjectClass::Amber) else {
                continue;
            };
            if o.frames.is_empty() {
                continue;
            }
            let Some(f) = o.record.fields.get(6..8) else {
                continue;
            };
            let (cx, cy) = (f[0] as i32, f[1] as i32);
            seen.push(o.id);
            let size = dep
                .remaining
                .map_or(3, |left| unload::amber_level(left, 1000.0))
                .clamp(1, 3);
            let rng = &mut self.gull_rng;
            let wait = |rng: &mut Rng| {
                let (lo, hi) = WAIT[size - 1];
                clock::ticks(rng.next() % (hi - lo + 1) + lo)
            };
            let s = self.sparkles.entry(o.id).or_insert_with(|| Sparkle {
                wait: wait(rng),
                playing: None,
            });
            if let Some(p) = s.playing.as_mut() {
                p.0 += dt;
                if s.frame(frames).is_none() {
                    s.playing = None;
                    s.wait = wait(rng);
                }
                continue;
            }
            s.wait -= dt;
            if s.wait > 0.0 {
                continue;
            }
            let tile = if cx >= 0
                && cy >= 0
                && (cx as u32) < self.grid.width
                && (cy as u32) < self.grid.height
            {
                self.grid.get(0, cx as u32, cy as u32).variant
            } else {
                0
            };
            let variant = objects::amber_variant(tile);
            let (x0, y0, w, h) = objects::AMBER_VARIANTS[variant].1[(rng.next() & 1) as usize];
            let step = clock::ticks((rng.next() & 3) + 1);
            let x = cx as f32 * t + x0 as f32 + (rng.next() % w.max(1) as u32) as f32;
            let y = cy as f32 * t + y0 as f32 + (rng.next() % h.max(1) as u32) as f32;
            s.playing = Some((0.0, step, x, y));
        }
        self.sparkles.retain(|id, _| seen.contains(id));
    }
}

pub type Sparkles = HashMap<u32, Sparkle>;
