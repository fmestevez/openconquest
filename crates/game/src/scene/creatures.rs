//! What the wild creatures do (the manual's "Monsters"): the Gorgon turns
//! ships in her area to stone, sirens draw ships towards them, and harpies
//! wound crews (`flyers`). Lairs, nests and sea monsters fight with the weapons given
//! them in `combat::creature_weapons`.

use openac_formats::map::TILE_SIZE;
use openac_formats::ObjectClass;

use super::Scene;
use crate::objects::MapObject;

/// Seconds between the Gorgon's stares, and ship-seconds of stone she has
/// to share among the ships in her area each time. Placeholders.
pub const GORGON_PERIOD: f32 = 8.0;
pub const GORGON_POWER: f32 = 12.0;
/// How fast a siren draws ships (world pixels a second). Placeholder.
pub const SIREN_PULL: f32 = 18.0;
/// The Gorgon's and sirens' area when the record gives none, in cells.
const DEFAULT_AREA: u32 = 5;

/// A world rectangle: x0, y0, x1, y1.
type Rect = (f32, f32, f32, f32);

/// Treasure kind of the mirror shield of Athena, and the heroes who resist
/// the Gorgon and sirens.
const MIRROR_SHIELD: u8 = 14;
const ODYSSEUS: u8 = 7;
const PERSEUS: u8 = 8;

impl Scene {
    /// The world rectangle of a Gorgon's or siren's area: fields 7 and 8 are
    /// its size in cells, centred on the creature.
    fn creature_area(o: &MapObject) -> Rect {
        let t = TILE_SIZE as f32;
        let size = |i| match o.record.field(i).unwrap_or(0) {
            0 => DEFAULT_AREA,
            n => n,
        } as f32
            * t;
        let (w, h) = (size(7), size(8));
        (o.x - w / 2.0, o.y - h / 2.0, o.x + w / 2.0, o.y + h / 2.0)
    }

    pub(super) fn run_creatures(&mut self, dt: f32) {
        self.creature_clock += dt;
        let t = TILE_SIZE as f32;
        // Stone wears off.
        for c in self.objects.iter_mut().filter_map(|o| o.combat.as_mut()) {
            c.stone = (c.stone - dt).max(0.0);
        }
        let inside = |o: &MapObject, r: Rect| {
            o.ship.is_some() && (r.0..=r.2).contains(&o.x) && (r.1..=r.3).contains(&o.y)
        };
        let warded = |o: &MapObject| o.combat.as_ref().is_some_and(|c| c.protect_magic > 0.0);
        let gorgons: Vec<Rect> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Monster(0x1fa))
            .map(Self::creature_area)
            .collect();
        let sirens: Vec<(f32, f32, Rect)> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Monster(0x1fd))
            .map(|o| (o.x, o.y, Self::creature_area(o)))
            .collect();
        // The Gorgon's stare: her power is shared among the ships in her
        // area, so a group suffers less each.
        let stare = self.creature_clock % GORGON_PERIOD < dt;
        if stare {
            for r in gorgons {
                let caught: Vec<usize> = self
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| inside(o, r) && !warded(o))
                    .filter(|(_, o)| {
                        let s = o.ship.as_ref().unwrap();
                        !s.has_hero(PERSEUS) && !s.has_treasure(MIRROR_SHIELD)
                    })
                    .map(|(i, _)| i)
                    .collect();
                if caught.is_empty() {
                    continue;
                }
                let each = GORGON_POWER / caught.len() as f32;
                for i in caught {
                    if let Some(c) = self.objects[i].combat.as_mut() {
                        c.stone = c.stone.max(each);
                    }
                }
                self.messages
                    .push("The Gorgon's stare turns ships to stone".into());
            }
        }
        // Sirens draw ships towards them (over water only).
        for (sx, sy, r) in sirens {
            for o in self.objects.iter_mut() {
                if !inside(o, r) || warded(o) || o.ship.as_ref().unwrap().has_hero(ODYSSEUS) {
                    continue;
                }
                let (dx, dy) = (sx - o.x, sy - o.y);
                let d = (dx * dx + dy * dy).sqrt();
                if d < t / 2.0 {
                    continue;
                }
                let step = (SIREN_PULL * dt).min(d);
                let (nx, ny) = (o.x + dx / d * step, o.y + dy / d * step);
                if self.nav.is_water((nx / t) as i32, (ny / t) as i32) {
                    o.x = nx;
                    o.y = ny;
                }
            }
        }
    }
}
