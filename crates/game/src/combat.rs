//! Ship combat: weapons, hull damage and the effects drawn for them.
//!
//! The weapon tables come from ANCIENT.EXE; hull strength, range and reload
//! times have not been recovered yet and are placeholders.

use openac_formats::{ObjectClass, ObjectRecord};

use crate::objects::Layer;

/// Hull and weapons of an object that can fight or be attacked.
#[derive(Debug, Clone, PartialEq)]
pub struct Combatant {
    pub hull: f32,
    pub max_hull: f32,
    /// Weapon kinds carried.
    pub weapons: Vec<u8>,
    /// Seconds until the next volley.
    pub reload: f32,
    /// Seconds left burning (Flame spell).
    pub burning: f32,
    /// Seconds left of protection from weapons and from magic.
    pub protect_weapons: f32,
    pub protect_magic: f32,
}

impl Combatant {
    /// Ships and player buildings fight; other objects do not.
    pub fn from_record(rec: &ObjectRecord, class: ObjectClass) -> Option<Self> {
        let f = |i: usize| rec.fields.get(i).copied().unwrap_or(0);
        let (max_hull, condition, weapons) = match class {
            ObjectClass::Ship => {
                let kind = f(7).min(11) as u8;
                // Synthetic records of newly built ships are short.
                let condition = if rec.fields.len() > 12 { f(12) } else { 100 };
                (
                    hull_strength(kind),
                    condition,
                    weapons_from_record(&rec.fields, kind),
                )
            }
            ObjectClass::Building(c) => (building_strength(c), f(7), fort_weapons(c, &rec.fields)),
            _ => return None,
        };
        Some(Self {
            hull: max_hull * condition.min(100) as f32 / 100.0,
            max_hull,
            weapons,
            reload: 0.0,
            burning: 0.0,
            protect_weapons: 0.0,
            protect_magic: 0.0,
        })
    }

    pub fn is_armed(&self) -> bool {
        self.weapons.iter().any(|&w| hull_damage(w) > 0.0)
    }
}

/// The marines of a ship: bowmen, javelin men or swordsmen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crew {
    /// 0 bowmen, 1 javelin men, 2 swordsmen.
    pub kind: u8,
    pub count: u32,
}

pub const CREW_NAMES: [&str; 3] = ["bowmen", "javelin men", "swordsmen"];

/// Cost of training one marine of each type: amber and fish (0x597164,
/// 0x597170 x 100).
pub const CREW_COST: [(u32, u32); 3] = [(30, 200), (50, 200), (70, 300)];

/// Seconds to train one marine. The settlement menu (0x43c740) shows
/// progress as elapsed x 100 / (70 x queued); reading the 70 as tenths of a
/// second is a guess.
pub const CREW_TRAIN_SECONDS: f32 = 7.0;

/// Marines a ship type can carry. Assumed to be the per-type values at
/// 0x596110 (boat 10, merchant 20, royal 25, ...); not confirmed.
pub const CREW_CAPACITY: [u32; 12] = [10, 20, 25, 20, 10, 10, 10, 10, 10, 10, 10, 20];

/// How close (in cells) a ship must lie to a settlement to take on marines.
/// Placeholder.
pub const LOAD_REACH: f32 = 3.0;

/// Boarding strength of each crew type (0x597d2c rows 0-2, third value at
/// level 0).
const CREW_MELEE: [u32; 3] = [35, 68, 100];

/// Crew quality used in ramming (0x598054, by crew type and level; level 0).
const CREW_RAM: [u32; 3] = [100, 125, 160];

impl Crew {
    /// Ship records hold the crew type plus 3 in field 14 and the number of
    /// marines in field 16. Newly built ships get a placeholder crew.
    pub fn from_record(fields: &[u32]) -> Self {
        match (fields.get(14), fields.get(16)) {
            (Some(&k), Some(&n)) if (3..=5).contains(&k) => Self {
                kind: (k - 3) as u8,
                count: n.min(50),
            },
            _ => Self { kind: 2, count: 4 },
        }
    }

    pub fn melee(&self) -> u32 {
        CREW_MELEE[self.kind as usize % 3]
    }
}

/// Ramming power of each ship type (0x598024).
const RAM_POWER: [u32; 12] = [10, 12, 100, 60, 30, 140, 20, 40, 10, 10, 190, 10];
/// Angle factors for the rammer and the rammed (0x598078, 0x598084): side
/// on, oblique, head on.
const RAM_ANGLE_ATTACK: [u32; 3] = [90, 55, 20];
const RAM_ANGLE_DEFENCE: [u32; 3] = [4, 12, 20];

/// Odds of a ram in percent, capped at 200 (0x4ba950): ramming power, crew
/// quality and the angle of approach of both ships; rammers (type 5) hit
/// half again as hard.
pub fn ram_odds(attacker: (u8, Crew), target: (u8, Crew), angle: usize) -> u32 {
    let power = |(kind, crew): (u8, Crew), angle_factor: u32| {
        (CREW_RAM[crew.kind as usize % 3] * RAM_POWER[kind as usize % 12]) as u64
            * angle_factor as u64
    };
    let mut a = power(attacker, RAM_ANGLE_ATTACK[angle.min(2)]);
    if attacker.0 == 5 {
        a = a * 15 / 10;
    }
    let d = power(target, RAM_ANGLE_DEFENCE[angle.min(2)]);
    if d == 0 {
        return 200;
    }
    (a * 100 / d).min(200) as u32
}

/// The angle class between two headings (0-7): 0 side on, 1 oblique, 2
/// head on or astern.
pub fn ram_angle(attacker_heading: usize, target_heading: usize) -> usize {
    match (attacker_heading as i32 - target_heading as i32).rem_euclid(4) {
        2 => 0,
        1 | 3 => 1,
        _ => 2,
    }
}

/// Seconds between exchanges of blows when boarding. Placeholder.
pub const BOARD_ROUND: f32 = 1.0;

/// Distance at which ships touch, for ramming and boarding. Placeholder.
pub const CONTACT: f32 = 48.0;

/// Hull points of a building. Placeholder: three times its amber cost.
pub fn building_strength(class: u32) -> f32 {
    (crate::economy::building_cost(class).max(400) * 3) as f32
}

/// Forts (classes 0x34-0x36) shoot from their emplacements. Fort records
/// flag twelve emplacements in fields 22-33 (wood fort 3, stone fort 8);
/// what each one holds is not known yet, so each fires a weapon of kind 0.
fn fort_weapons(class: u32, fields: &[u32]) -> Vec<u8> {
    if !(0x34..=0x36).contains(&class) {
        return Vec::new();
    }
    let n = fields
        .get(22..34)
        .map_or(0, |f| f.iter().filter(|&&v| v == 1).count());
    vec![0; n]
}

/// Weapons a ship type carries when built (0x5964d8: a count and up to nine
/// weapon kinds per ship type).
pub const DEFAULT_WEAPONS: [&[u8]; 12] = [
    &[],
    &[],
    &[0, 2, 4, 8],
    &[0, 1, 4, 6, 7],
    &[1, 2, 2, 3],
    &[1, 4, 5],
    &[5, 4],
    &[3, 0],
    &[],
    &[1],
    &[],
    &[],
];

/// Weapons of a ship from its mission record: fields 18-26 hold how many of
/// each of the nine weapon kinds it carries (fields 29.. hold their
/// condition, eight per kind). Records too short for that get the ship
/// type's default loadout.
pub fn weapons_from_record(fields: &[u32], kind: u8) -> Vec<u8> {
    match fields.get(18..27) {
        Some(counts) => counts
            .iter()
            .enumerate()
            .flat_map(|(w, &n)| std::iter::repeat_n(w as u8, n.min(8) as usize))
            .collect(),
        None => DEFAULT_WEAPONS[kind as usize].to_vec(),
    }
}

/// Damage of a weapon kind against hulls and against crews, at upgrade
/// level 0 (0x597d2c, rows 3-11; rows 0-2 are the three crew types, and
/// each value has three upgrade levels).
const WEAPON_DAMAGE: [(u32, u32); 9] = [
    (40, 80),
    (60, 60),
    (80, 40),
    (200, 80),
    (250, 100),
    (220, 80),
    (0, 0),
    (200, 80),
    (200, 80),
];

pub fn hull_damage(weapon: u8) -> f32 {
    WEAPON_DAMAGE.get(weapon as usize).map_or(0, |w| w.0) as f32
}

/// Hull points of a ship type. Placeholder: twice the hull's amber cost.
pub fn hull_strength(kind: u8) -> f32 {
    crate::economy::HULL_AMBER[kind as usize] as f32 * 2.0
}

/// Firing range in world pixels. Placeholder, kept below four cells:
/// miss007 starts with a barbarian fleet four cells from a Greek fortress,
/// presumably out of its range.
pub const RANGE: f32 = 3.5 * 64.0;
/// Seconds between volleys. Placeholder.
pub const RELOAD_SECONDS: f32 = 3.0;
/// Chance that a shot hits. Placeholder.
pub const HIT_CHANCE: u32 = 70;

/// Effect animations (BOMB sequences): a hit, a splash and a sinking.
pub const FX_HIT: &str = "BOMBA20";
pub const FX_SPLASH: &str = "BOMBA50";
pub const FX_SINK: &str = "BOMBG10";
const FX_FRAME_MS: f32 = 70.0;

/// An animation at a world position: played once, or looped for a while
/// over an object it follows.
pub struct Effect {
    pub x: f32,
    pub y: f32,
    pub frames: Vec<Vec<Layer>>,
    pub age_ms: f32,
    /// The object it stays on, if any.
    pub follow: Option<u32>,
    /// Loop until this age; `None` plays the frames once.
    pub life_ms: Option<f32>,
}

impl Effect {
    /// The current frame, or `None` once the animation has finished.
    pub fn frame(&self) -> Option<&[Layer]> {
        let step = (self.age_ms / FX_FRAME_MS) as usize;
        match self.life_ms {
            Some(life) if self.age_ms < life && !self.frames.is_empty() => {
                Some(&self.frames[step % self.frames.len()])
            }
            Some(_) => None,
            None => self.frames.get(step).map(Vec::as_slice),
        }
    }
}

/// The original's random generator (0x4bb670): a 32-bit LCG whose high
/// half is used.
#[derive(Debug, Clone)]
pub struct Rng(pub u32);

impl Rng {
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        self.0 >> 16
    }

    /// True with probability `percent`%.
    pub fn chance(&mut self, percent: u32) -> bool {
        self.next() % 100 < percent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SWORDS: Crew = Crew { kind: 2, count: 8 };

    #[test]
    fn ram_angles() {
        assert_eq!(ram_angle(0, 2), 0, "east into a northbound ship: side on");
        assert_eq!(ram_angle(0, 1), 1);
        assert_eq!(ram_angle(0, 4), 2, "head on");
        assert_eq!(ram_angle(3, 3), 2, "astern");
    }

    #[test]
    fn ram_odds_favour_rammers_and_side_hits() {
        let royal = (2, SWORDS);
        assert_eq!(ram_odds(royal, royal, 2), 100, "head on between equals");
        assert_eq!(ram_odds(royal, royal, 0), 200, "side on, capped");
        let rammer = (5, SWORDS);
        assert!(ram_odds(rammer, royal, 2) > ram_odds(royal, rammer, 2));
    }
}
