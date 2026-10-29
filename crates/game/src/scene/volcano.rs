//! Volcanoes erupting (VolcanoClassTy, `Volcano.cpp`): asleep for a while,
//! then the eruption (`VOLCANOERAPTION`) throws burning stones about the
//! crater in volleys (0x445ad0), and it sleeps again.

use super::*;
use crate::clock::ticks;
use crate::combat::Rng;

/// Where the stones leave the crater, by the volcano's lava picture
/// (record field 4, 0x5970c8), from its cell's corner plus (0x46, 0x82).
const CRATER: [(f32, f32); 11] = [
    (-6.0, -100.0),
    (16.0, -110.0),
    (-2.0, -130.0),
    (-19.0, -112.0),
    (-3.0, -95.0),
    (-16.0, -126.0),
    (13.0, -120.0),
    (-13.0, -93.0),
    (12.0, -98.0),
    (0.0, -83.0),
    (-17.0, -120.0),
];

/// The eruption's frames: it loops 0x3c-0x77 while it lasts, a frame every
/// 2 ticks (0x445630), then plays out.
const LOOP: (usize, usize) = (0x3c, 0x77);

/// A volcano's state.
#[derive(Debug, Clone, Default)]
pub struct Volcano {
    /// Erupting.
    pub erupting: bool,
    /// Seconds to the end of its sleep or of its eruption.
    pub left: f32,
    /// Seconds to the next volley.
    pub volley: f32,
    /// The eruption's frame, and seconds owed to the next.
    pub frame: usize,
    pub clock: f32,
    /// Its own generator (+0x18).
    pub rng: Rng,
}

impl Scene {
    /// Volcanoes sleep (45000 - 4500 N) x 80-100% ticks, erupt (15000 -
    /// 1500 N) x 80-100%, throwing a volley every (50 - 5 N) x 80-100%
    /// while the eruption shows; N its record field 8, 0 never erupting.
    pub(super) fn run_volcanoes(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let ids: Vec<(u32, u32)> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Lava)
            .map(|o| (o.id, o.record.field(8).unwrap_or(0)))
            .filter(|&(_, n)| n > 0)
            .collect();
        for (id, n) in ids {
            let n = n.min(9);
            let scaled = |v: &mut Volcano, base: u32| ticks(base * (v.rng.next() % 21 + 80) / 100);
            let v = self.volcanoes.entry(id).or_insert_with(|| {
                let mut v = Volcano {
                    rng: Rng(id.wrapping_mul(0x9e37_79b9)),
                    ..Default::default()
                };
                v.left = scaled(&mut v, 45000 - 4500 * n);
                v
            });
            let mut volley = false;
            let mut started = false;
            let mut ended = false;
            v.left -= dt;
            if !v.erupting {
                if v.left <= 0.0 {
                    v.erupting = true;
                    v.left = scaled(v, 15000 - 1500 * n);
                    v.volley = scaled(v, 50 - 5 * n);
                    v.frame = 0;
                    v.clock = 0.0;
                    started = true;
                }
            } else {
                v.clock += dt;
                let step = ticks(2);
                while v.clock >= step {
                    v.clock -= step;
                    v.frame += 1;
                    if v.frame > LOOP.1 && v.left > 0.0 {
                        v.frame = LOOP.0;
                    }
                }
                v.volley -= dt;
                if v.volley <= 0.0 && v.frame < 180 {
                    v.volley = scaled(v, 50 - 5 * n);
                    volley = true;
                }
                if v.frame >= 180 {
                    v.erupting = false;
                    v.left = scaled(v, 45000 - 4500 * n);
                    ended = true;
                }
            }
            let frame = v.frame;
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let (x, y) = (self.objects[i].x, self.objects[i].y);
            if started {
                self.sounds.push(("shum2", x, y));
                if let Ok(frames) = objects::sequence_frames(data, caches, "VOLCANOERAPTION") {
                    self.effects.push(Effect {
                        x,
                        y,
                        frames,
                        age_ms: 0.0,
                        follow: None,
                        life_ms: Some(f32::MAX),
                        frame_ms: crate::clock::TICK_MS as f32 * 2.0,
                        tornado: None,
                        site: Some(id),
                    });
                }
            }
            if let Some(fx) = self.effects.iter_mut().find(|fx| fx.site == Some(id)) {
                fx.show_frame(frame);
            }
            if ended {
                self.effects.retain(|fx| fx.site != Some(id));
            }
            if volley {
                self.erupt(i, data, caches);
            }
        }
    }

    /// A volley (0x445ad0): up to N - 1 stones, each on another cell within
    /// the radius (field 9) but the crater's 2 x 2; Q, R or S burning, G
    /// or F burning by chance, or (one in six) none.
    fn erupt(&mut self, i: usize, data: &GameData, caches: &mut Caches) {
        let o = &self.objects[i];
        let f = |k| o.record.field(k).unwrap_or(0) as i32;
        let (pic, cx, cy, n, r) = (f(4), f(6), f(7), f(8), f(9));
        let id = o.id;
        let t = TILE_SIZE as f32;
        let (x, y) = (o.x, o.y);
        let mut cells: Vec<(i32, i32)> = (cy - r..=cy + r + 1)
            .flat_map(|yy| (cx - r..=cx + r + 1).map(move |xx| (xx, yy)))
            .filter(|&(xx, yy)| !((cx..=cx + 1).contains(&xx) && (cy..=cy + 1).contains(&yy)))
            .collect();
        if cells.is_empty() {
            return;
        }
        let (dx, dy) = CRATER[(pic as usize).min(10)];
        let from = (cx as f32 * t + 70.0 + dx, cy as f32 * t + 130.0 + dy);
        let Some(v) = self.volcanoes.get_mut(&id) else {
            return;
        };
        let count = v.rng.next() % n.max(1) as u32;
        let mut stones = Vec::new();
        for _ in 0..count {
            if cells.is_empty() {
                break;
            }
            let (tx, ty) = cells.remove(v.rng.next() as usize % cells.len());
            let (letter, flaming) = match v.rng.next() % 6 {
                0 => (Some(16), true),
                1 => (Some(17), true),
                2 => (Some(18), true),
                3 => (Some(6), v.rng.next() & 1 == 1),
                4 => (Some(5), v.rng.next() & 1 == 1),
                _ => {
                    v.rng.next();
                    (None, false)
                }
            };
            v.rng.next();
            v.rng.next();
            let last = v.rng.next();
            let off = (last & 0x3f) as f32;
            if let Some(l) = letter {
                stones.push((
                    l,
                    flaming,
                    (tx as f32 * t + off, ty as f32 * t + off),
                    last % 5 == 0,
                ));
            }
        }
        for (letter, flaming, to, trail) in stones {
            self.fire_volcanic(letter, flaming, from, to, trail, data, caches);
        }
        let pick = if self.gull_rng.next() & 1 == 0 {
            "eject"
        } else {
            "eject1"
        };
        self.sounds.push((pick, x, y));
    }
}
