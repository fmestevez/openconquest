//! Wind, as WeatherClassTy (Weather.cpp, 0x407a80) models it.
//!
//! The wind has a strength 0-10 and a direction 0-7 (the heading it blows
//! towards: 0 east, counting counter-clockwise in 45 degree steps, like ship
//! headings). Every 1.5 to 11.5 seconds it takes the value prepared at the
//! previous change and prepares the next one, which usually drifts by up to
//! two steps in strength and direction.

use crate::combat::Rng;

/// Sailing speed in percent by hull class (warships, merchants), wind
/// strength and the angle between the wind and the heading (0 with the wind
/// .. 4 against it) (0x597b2c). Both classes share the same values.
const SPEED: [[u32; 5]; 11] = [
    [60, 60, 60, 60, 60],
    [60, 60, 60, 60, 60],
    [60, 60, 60, 60, 60],
    [69, 66, 63, 60, 57],
    [77, 72, 66, 60, 54],
    [86, 77, 69, 60, 51],
    [94, 82, 71, 60, 49],
    [103, 88, 74, 60, 46],
    [111, 94, 77, 60, 43],
    [120, 100, 80, 60, 40],
    [125, 67, 28, 46, 62],
];

#[derive(Debug, Clone)]
pub struct Wind {
    pub strength: u32,
    pub direction: u32,
    next_strength: u32,
    next_direction: u32,
    elapsed_ms: f32,
    period_ms: f32,
    rng: Rng,
    /// A wind called up by magic: seconds left, and the wind to return to.
    forced: Option<(f32, u32, u32)>,
}

impl Wind {
    /// A random starting wind (0x407e60).
    pub fn new(seed: u32) -> Self {
        let mut rng = Rng(seed);
        let next_strength = (rng.next() & 7) + 2;
        let next_direction = rng.next() & 7;
        let mut wind = Self {
            strength: 0,
            direction: 0,
            next_strength,
            next_direction,
            elapsed_ms: 0.0,
            period_ms: 10_000.0,
            rng,
            forced: None,
        };
        wind.change();
        wind
    }

    /// Takes the prepared wind and prepares the next (0x407d20).
    fn change(&mut self) {
        self.strength = self.next_strength;
        self.direction = self.next_direction;
        if self.rng.next() % 100 > 4 {
            let base = if self.strength == 0 {
                3
            } else {
                self.strength as i32
            };
            let mut s = (self.rng.next() % 5) as i32 - 2 + base;
            if s < 0 {
                s = 11 - s;
            }
            if s > 10 {
                s -= 11;
            }
            let s = s.clamp(0, 10);
            self.next_strength = if s == 1 { 0 } else { s as u32 };
            let d = ((self.rng.next() % 5) as i32 - 2 + self.direction as i32).rem_euclid(8);
            self.next_direction = d as u32;
        } else {
            self.next_strength = self.rng.next() % 11;
            self.next_direction = self.rng.next() & 7;
        }
    }

    /// Advances the weather by `dt` seconds.
    pub fn update(&mut self, dt: f32) {
        if let Some((left, strength, direction)) = self.forced {
            if left > dt {
                self.forced = Some((left - dt, strength, direction));
            } else {
                self.forced = None;
                self.strength = strength;
                self.direction = direction;
            }
            return;
        }
        self.elapsed_ms += dt * 1000.0;
        if self.elapsed_ms >= self.period_ms {
            self.elapsed_ms = 0.0;
            self.period_ms = (self.rng.next() % 10_000 + 1_500) as f32;
            self.change();
        }
    }

    /// Raises a strong wind (strength 7) towards `direction` for `seconds`
    /// (message 0x1103), after which the previous wind returns.
    pub fn force(&mut self, direction: u32, seconds: f32) {
        let (strength, old) = match self.forced {
            Some((_, s, d)) => (s, d),
            None => (self.strength, self.direction),
        };
        self.forced = Some((seconds, strength, old));
        self.strength = 7;
        self.direction = direction & 7;
    }

    /// Speed factor for a ship sailing on `heading` (0x4a68b0).
    pub fn speed_factor(&self, merchant: bool, heading: usize) -> f32 {
        let _ = merchant; // The two tables are identical.
        let diff = (self.direction as i32 - heading as i32).unsigned_abs();
        let angle = if diff > 4 { 8 - diff } else { diff };
        SPEED[self.strength.min(10) as usize][angle as usize] as f32 / 100.0
    }

    /// The wind rose arrow: `None` for calm, else the `GAME_WIND_nn` frame
    /// (0x4079a0: eight directions at three strengths).
    pub fn rose_frame(&self) -> Option<u32> {
        if self.strength < 3 {
            return None;
        }
        let band = if self.strength >= 7 {
            2
        } else if self.strength >= 5 {
            1
        } else {
            0
        };
        Some(band * 8 + self.direction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_stays_in_range_and_changes() {
        let mut wind = Wind::new(42);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..20_000 {
            wind.update(0.1);
            assert!(wind.strength <= 10 && wind.direction <= 7);
            seen.insert((wind.strength, wind.direction));
        }
        assert!(seen.len() > 20);
    }

    #[test]
    fn sailing_with_the_wind_is_fastest() {
        let mut wind = Wind::new(1);
        wind.strength = 9;
        wind.direction = 2;
        assert_eq!(wind.speed_factor(false, 2), 1.2);
        assert_eq!(wind.speed_factor(false, 6), 0.4);
        assert_eq!(wind.speed_factor(false, 1), wind.speed_factor(false, 3));
        assert_eq!(wind.rose_frame(), Some(18));
        wind.strength = 2;
        assert_eq!(wind.speed_factor(true, 6), 0.6);
        assert_eq!(wind.rose_frame(), None);
    }

    #[test]
    fn a_called_wind_blows_then_returns() {
        let mut wind = Wind::new(7);
        let before = (wind.strength, wind.direction);
        wind.force(4, 10.0);
        for _ in 0..90 {
            wind.update(0.1);
            assert_eq!((wind.strength, wind.direction), (7, 4));
        }
        for _ in 0..20 {
            wind.update(0.1);
        }
        assert_eq!((wind.strength, wind.direction), before);
    }
}
