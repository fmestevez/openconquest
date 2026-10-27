//! What the campaign's scripts put on the map when a mission starts
//! (message 0x8d1): the wizards' castles and the dragon's nest, with the
//! treasures they guard; and what the wizards do.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::{ObjectClass, ObjectRecord};

use super::Scene;
use crate::assets::GameData;
use crate::magic::Spell;
use crate::objects::{self, Caches, Treasure};

/// A wizard's casting (CastleClassTy, 0x410790): the next cast's time,
/// and who last attacked him and until when he hurries (3000 ticks).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Wizard {
    pub next: f32,
    pub attacker: Option<(u32, f32)>,
}

/// How long a wizard hurries after he is attacked.
const WIZARD_ANGER: u32 = 3000;

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

    /// Ticks to a wizard's next cast (0x410790, 0x410e6e): (1250 - 125 x
    /// level) x 80-100%.
    fn wizard_wait(&mut self, level: u32) -> f32 {
        let base = 1250u32.saturating_sub(125 * level);
        crate::clock::ticks(base * (80 + self.rng.next() % 21) / 100)
    }

    /// A shot from `shooter` at `target`: a wizard attacked hurries.
    pub(super) fn wizard_attacked(&mut self, target: u32, shooter: u32) {
        if let Some(w) = self.wizards.get_mut(&target) {
            w.attacker = Some((shooter, crate::clock::ticks(WIZARD_ANGER)));
        }
    }

    /// Wizards cast at what lies around them (0x410790): the wind wizard
    /// (sea castle) a waterspout, the fire wizard (war castle) a fireball,
    /// on a cell within fields 23 and 24 of his castle picked at random,
    /// weighted by what is around it (each ship or fort 10 less its
    /// distance from the castle, any other building 1, four times as much
    /// for his last attacker), every (1250 - 125 x level) x 80-100% ticks,
    /// twice as often for 3000 ticks after he is attacked.
    pub(super) fn run_wizards(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let castles: Vec<(u32, (i32, i32), u32, i32, i32, u32)> = self
            .objects
            .iter()
            .filter_map(|o| match o.class {
                ObjectClass::Castle(c) if o.combat.as_ref().is_some_and(|c| c.hull > 0.0) => {
                    let f = |k, d| o.record.field(k).unwrap_or(d);
                    let cell = o.cell.unwrap_or(((o.x / t) as i32, (o.y / t) as i32));
                    Some((o.id, cell, f(22, 5), f(23, 8) as i32, f(24, 8) as i32, c))
                }
                _ => None,
            })
            .collect();
        for (id, (cx, cy), level, rx, ry, class) in castles {
            if !self.wizards.contains_key(&id) {
                let next = self.wizard_wait(level);
                self.wizards.insert(
                    id,
                    Wizard {
                        next,
                        attacker: None,
                    },
                );
            }
            let w = self.wizards.get_mut(&id).unwrap();
            if let Some((_, left)) = w.attacker.as_mut() {
                *left -= dt;
                if *left <= 0.0 {
                    w.attacker = None;
                }
            }
            w.next -= dt;
            if w.next > 0.0 {
                continue;
            }
            let attacker = w.attacker.map(|a| a.0);
            let mut wait = self.wizard_wait(level);
            if attacker.is_some() {
                wait /= 2.0;
            }
            self.wizards.get_mut(&id).unwrap().next = wait;
            // What lies on each cell around him.
            let mut marks: Vec<((i32, i32), u32)> = Vec::new();
            for o in &self.objects {
                if o.owner.is_none() || o.owner == Some(crate::combat::MONSTERS) {
                    continue;
                }
                if o.combat.as_ref().is_none_or(|c| c.hull <= 0.0) {
                    continue;
                }
                let cell = match (&o.ship, o.cell) {
                    (None, Some(c)) => c,
                    _ => ((o.x / t).floor() as i32, (o.y / t).floor() as i32),
                };
                let far = (cell.0 - cx).abs().max((cell.1 - cy).abs()) as u32;
                let mut weight = match o.class {
                    ObjectClass::Ship => 10u32.saturating_sub(far),
                    ObjectClass::Building(b) if (0x34..=0x36).contains(&b) => {
                        10u32.saturating_sub(far)
                    }
                    ObjectClass::Building(_) => 1,
                    _ => 0,
                };
                if Some(o.id) == attacker {
                    weight *= 4;
                }
                if weight > 0 {
                    marks.push((cell, weight));
                }
            }
            let mut cells: Vec<((i32, i32), u32)> = Vec::new();
            for y in cy - ry..=cy + ry {
                for x in cx - rx..=cx + rx {
                    let weight: u32 = marks
                        .iter()
                        .filter(|(c, _)| (c.0 - x).abs() <= 1 && (c.1 - y).abs() <= 1)
                        .map(|m| m.1)
                        .sum();
                    if weight > 0 {
                        cells.push(((x, y), weight));
                    }
                }
            }
            let total: u32 = cells.iter().map(|c| c.1).sum();
            if total == 0 {
                continue;
            }
            let mut pick = self.rng.next() % total;
            let Some(&((x, y), _)) = cells.iter().find(|c| {
                if pick < c.1 {
                    true
                } else {
                    pick -= c.1;
                    false
                }
            }) else {
                continue;
            };
            let monsters = Some(crate::combat::MONSTERS);
            if class == 0x42 {
                let _ = self.spawn_hazard(Spell::Waterspout, x, y, monsters, data, caches);
            } else {
                self.spawn_fireball(x, y, monsters, data, caches);
            }
        }
    }
}
