//! What the campaign's scripts put on the map when a mission starts
//! (message 0x8d1): the wizards' castles and the dragon's nest, with the
//! treasures they guard; and what the wizards do.

use openac_formats::map::TILE_SIZE;
use openac_formats::{ObjectClass, ObjectRecord};

use super::Scene;
use crate::assets::GameData;
use crate::objects::{self, Caches, Treasure};

/// Seconds between a wizard's strikes at level 0, less a second a level,
/// and the hull points a strike takes. Placeholders.
const WIZARD_PERIOD: f32 = 20.0;
const WIZARD_DAMAGE: f32 = 150.0;
/// Effects of the wind wizard's lightning and the fire wizard's fire.
const FX_WIND: &str = "SLIGHT1";
const FX_FIRE: &str = "FIRE0";

impl Scene {
    /// Places mission `mission`'s scripted objects at `difficulty`.
    pub(super) fn place_scripted(
        &mut self,
        difficulty: usize,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let d = difficulty as u32;
        match self.mission.as_str() {
            // The wind wizard's castle at one of two places (0x45fe58).
            "miss007" => {
                let (x, y) = if self.rng.next() & 1 == 0 {
                    (46, 73)
                } else {
                    (16, 74)
                };
                self.place_castle(0x42, x, y, [d + 5, 2 * d + 8, 2 * d + 8], data, caches);
            }
            // The fire wizard's castle, which holds the scroll of Greek
            // fire and a disposable magic (0x466eb2).
            "miss009" => {
                if let Some(id) = self.place_castle(
                    0x43,
                    30,
                    1,
                    [2 * d + 5, 5 * d + 10, 5 * d + 10],
                    data,
                    caches,
                ) {
                    self.held.push((
                        id,
                        Treasure {
                            kind: 27,
                            param: 13,
                            amount: 0,
                        },
                    ));
                    self.held.push((
                        id,
                        Treasure {
                            kind: 9,
                            param: 6,
                            amount: 0,
                        },
                    ));
                }
            }
            // The dragon's nest, which holds the Golden Fleece (0x46a86c).
            "miss010" => {
                let mut fields = vec![0u32; 13];
                fields[..4].copy_from_slice(&[0x44, 2, 4, 1]);
                fields[4] = 59;
                fields[5] = 4;
                fields[7..12].copy_from_slice(&[
                    5 * d + 10,
                    5 * d + 10,
                    d + 1,
                    2 * d + 5,
                    2 * d + 5,
                ]);
                if let Some(id) = self.spawn_record(0x44, fields, data, caches) {
                    self.held.push((
                        id,
                        Treasure {
                            kind: 16,
                            param: 0,
                            amount: 10,
                        },
                    ));
                }
            }
            _ => {}
        }
    }

    /// A wizard's castle (0x411010): its cell in fields 5 and 6, its level
    /// and two parameters in fields 22-24.
    fn place_castle(
        &mut self,
        class: u32,
        x: u32,
        y: u32,
        params: [u32; 3],
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<u32> {
        let mut fields = vec![0u32; 25];
        fields[..4].copy_from_slice(&[class, 2, 4, 1]);
        fields[5] = x;
        fields[6] = y;
        fields[7] = 100;
        fields[22..25].copy_from_slice(&params);
        self.spawn_record(class, fields, data, caches)
    }

    fn spawn_record(
        &mut self,
        class_id: u32,
        fields: Vec<u32>,
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<u32> {
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let rec = ObjectRecord {
            id,
            class_id,
            fields,
        };
        match objects::resolve(data, caches, &rec) {
            Ok(Some(obj)) => {
                self.objects.push(obj);
                self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
                Some(id)
            }
            Ok(None) => None,
            Err(e) => {
                eprintln!("could not place a scripted {class_id:#x}: {e:#}");
                None
            }
        }
    }

    /// Wizards strike at the player's ships within their reach (field 23,
    /// in cells), the more often the higher their level (field 22).
    pub(super) fn run_wizards(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let wizards: Vec<(f32, f32, u32, u32, &'static str)> = self
            .objects
            .iter()
            .filter_map(|o| match o.class {
                ObjectClass::Castle(c) if o.combat.as_ref().is_some_and(|c| c.hull > 0.0) => {
                    Some((
                        o.x,
                        o.y,
                        o.record.field(22).unwrap_or(5),
                        o.record.field(23).unwrap_or(8),
                        if c == 0x42 { FX_WIND } else { FX_FIRE },
                    ))
                }
                _ => None,
            })
            .collect();
        for (x, y, level, reach, fx) in wizards {
            let period = (WIZARD_PERIOD - level as f32).max(4.0);
            if self.creature_clock % period >= dt {
                continue;
            }
            let reach = reach as f32 * t;
            let target = self
                .objects
                .iter()
                .filter(|o| o.ship.is_some() && self.is_own(o))
                .map(|o| (o.id, o.x, o.y, (o.x - x).hypot(o.y - y)))
                .filter(|s| s.3 <= reach)
                .min_by(|a, b| a.3.total_cmp(&b.3));
            if let Some((id, tx, ty, _)) = target {
                self.spawn_effect(data, caches, fx, tx, ty);
                self.sounds.push((
                    if fx == FX_WIND {
                        "molniya2"
                    } else {
                        "fireball"
                    },
                    tx,
                    ty,
                ));
                self.hit(id, WIZARD_DAMAGE, data, caches);
            }
        }
    }
}
