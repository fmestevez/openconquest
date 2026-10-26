//! Fog of war: what the human player has explored and sees right now.
//!
//! The original keeps two layers (`b_DARK` for unexplored cells, `b_FOG`
//! for explored ones out of sight; DarkTy, 0x48b3a0) drawn with dithered
//! edge masks. Here each map cell is unexplored, explored or in sight;
//! ships and buildings see a few cells around them (placeholder ranges,
//! the original's have not been traced).

use super::*;

/// What the player knows of a map cell.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Seen {
    Unexplored = 0,
    Explored = 1,
    InSight = 2,
}

/// Cells a ship and a building see around them. Placeholders.
pub const SHIP_SIGHT: i32 = 4;
pub const BUILDING_SIGHT: i32 = 3;
/// Seconds between updates of what is in sight.
pub const SIGHT_PERIOD: f32 = 0.25;

impl Scene {
    /// What the player knows of cell `(cx, cy)`; everything is in sight
    /// when fog of war is off.
    pub fn seen(&self, cx: i32, cy: i32) -> Seen {
        if !self.fog {
            return Seen::InSight;
        }
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        if cx < 0 || cy < 0 || cx >= w || cy >= h {
            return Seen::Unexplored;
        }
        self.sight[(cy * w + cx) as usize]
    }

    /// Whether the player can see object `o` now: their own always, other
    /// players' ships and creatures only in sight, buildings and scenery
    /// once explored.
    pub fn is_visible(&self, o: &MapObject) -> bool {
        if !self.fog || self.is_own(o) {
            return true;
        }
        let t = TILE_SIZE as f32;
        let seen = self.seen((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let moves = o.ship.is_some() || matches!(o.class, ObjectClass::Monster(_));
        if moves {
            seen == Seen::InSight
        } else {
            seen >= Seen::Explored
        }
    }

    /// Recomputes what is in sight from the player's ships and buildings.
    pub(super) fn update_sight(&mut self) {
        self.refresh_sight();
    }

    /// Recomputes what the active player sees.
    pub fn refresh_sight(&mut self) {
        if !self.fog || self.sight.is_empty() {
            return;
        }
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        for s in &mut self.sight {
            if *s == Seen::InSight {
                *s = Seen::Explored;
            }
        }
        let t = TILE_SIZE as f32;
        let eyes: Vec<(i32, i32, i32)> = self
            .objects
            .iter()
            .filter(|o| self.is_own(o) && o.combat.is_some())
            .map(|o| {
                let r = if o.ship.is_some() {
                    SHIP_SIGHT
                } else {
                    BUILDING_SIGHT
                };
                ((o.x / t).floor() as i32, (o.y / t).floor() as i32, r)
            })
            .collect();
        for (cx, cy, r) in eyes {
            for y in (cy - r).max(0)..=(cy + r).min(h - 1) {
                for x in (cx - r).max(0)..=(cx + r).min(w - 1) {
                    if (x - cx).pow(2) + (y - cy).pow(2) <= r * r + r {
                        self.sight[(y * w + x) as usize] = Seen::InSight;
                    }
                }
            }
        }
    }

    /// Clears the clouds within `r` cells of `(cx, cy)` (the View spell).
    pub fn reveal(&mut self, cx: i32, cy: i32, r: i32) {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        for y in (cy - r).max(0)..=(cy + r).min(h - 1) {
            for x in (cx - r).max(0)..=(cx + r).min(w - 1) {
                let cell = &mut self.sight[(y * w + x) as usize];
                if *cell == Seen::Unexplored && (x - cx).pow(2) + (y - cy).pow(2) <= r * r + r {
                    *cell = Seen::Explored;
                }
            }
        }
    }

    /// Lifts the darkness from the whole map.
    #[cfg(test)]
    pub fn explore_all(&mut self) {
        for s in &mut self.sight {
            if *s == Seen::Unexplored {
                *s = Seen::Explored;
            }
        }
    }
}
