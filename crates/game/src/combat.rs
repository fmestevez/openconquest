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
            // Barbarian strongholds, creatures and their lairs can be
            // destroyed too (victory conditions count them).
            ObjectClass::Barbarian(_)
            | ObjectClass::Castle(_)
            | ObjectClass::HarpyNest
            | ObjectClass::CreatureSpawn
            | ObjectClass::Monster(0x1fa | 0x1fb | 0x1fd) => (MONSTER_STRENGTH, 100, Vec::new()),
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

/// What a ship carries in its nine item slots (ship records, fields
/// 18-26; the building menus use the same numbering: commands 0x2d + item):
/// marines (0 bowmen ONION, 1 javelin men DART, 2 swordsmen SWORD), then
/// weapons (3 TRANS, 4 gastraphetes, 5 small catapult, 6 ballista, 7 big
/// catapult, 8 onager).
pub const ITEM_NAMES: [&str; 9] = [
    "bowmen",
    "javelin men",
    "swordsmen",
    "TRANS",
    "gastraphetes",
    "small catapult",
    "ballista",
    "big catapult",
    "onager",
];

pub const CREW_NAMES: [&str; 3] = ["bowmen", "javelin men", "swordsmen"];

/// Cost of training a squad of marines: 30 amber each (u16 at 0x597164)
/// and fish (0x597170 x 100).
pub const CREW_COST: [(u32, u32); 3] = [(30, 200), (30, 200), (30, 300)];

/// Seconds to train one squad. The settlement menu (0x43c740) shows
/// progress as elapsed x 100 / (70 x queued); reading the 70 as tenths of a
/// second is a guess.
pub const CREW_TRAIN_SECONDS: f32 = 7.0;

/// Squads of marines a ship can carry. Placeholder.
pub const CREW_CAPACITY: u32 = 6;
/// Weapons a ship can carry. Placeholder.
pub const WEAPON_CAPACITY: u32 = 4;

/// Cost of an item in amber and fish: marines as above; weapons 50 amber
/// (items 3-5) or 70 (6-8) (0x597164 by item / 3, workshop menu 0x4361c0).
pub fn item_cost(item: u8) -> (u32, u32) {
    match item {
        0..=2 => CREW_COST[item as usize],
        3..=5 => (50, 0),
        _ => (70, 0),
    }
}

/// How close (in cells) a ship must lie to a settlement to take on marines.
/// Placeholder.
pub const LOAD_REACH: f32 = 3.0;

/// Boarding strength of each marine kind (0x597d2c rows 0-2, third value
/// at level 0).
const CREW_MELEE: [u32; 3] = [35, 68, 100];

/// Crew quality used in ramming (0x598054, by crew class and level; level
/// 0).
const CREW_RAM: [u32; 3] = [100, 125, 160];

/// The people on a ship: its crew class (record field 14 minus 3; ramming
/// uses it for crew quality) and squads of marines by kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crew {
    pub class: u8,
    pub squads: [u32; 3],
}

impl Crew {
    /// From a ship record: the crew class in field 14, marine squads in
    /// item slots 0-2 (fields 18-20). Records too short for that (ships
    /// built in play) get their type's default load.
    pub fn from_record(fields: &[u32], kind: u8) -> Self {
        let class = fields
            .get(14)
            .filter(|&&k| (3..=5).contains(&k))
            .map_or(0, |&k| (k - 3) as u8);
        let mut squads = [0u32; 3];
        match fields.get(18..21) {
            Some(counts) => {
                for (s, &n) in squads.iter_mut().zip(counts) {
                    *s = n.min(CREW_CAPACITY);
                }
            }
            None => {
                for &item in DEFAULT_WEAPONS[kind as usize % 12] {
                    if item < 3 {
                        squads[item as usize] += 1;
                    }
                }
            }
        }
        Self { class, squads }
    }

    pub fn count(&self) -> u32 {
        self.squads.iter().sum()
    }

    /// Takes away the `n`-th squad (counting across kinds).
    pub fn lose(&mut self, mut n: u32) {
        for s in &mut self.squads {
            if n < *s {
                *s -= 1;
                return;
            }
            n -= *s;
        }
    }

    /// The kind of the `n`-th squad.
    pub fn kind_of(&self, mut n: u32) -> usize {
        for (k, &s) in self.squads.iter().enumerate() {
            if n < s {
                return k;
            }
            n -= s;
        }
        0
    }

    pub fn describe(&self) -> String {
        let parts: Vec<String> = self
            .squads
            .iter()
            .zip(CREW_NAMES)
            .filter(|(n, _)| **n > 0)
            .map(|(n, name)| format!("{n} {name}"))
            .collect();
        parts.join(", ")
    }
}

/// Boarding strength of a squad of marine kind `kind`.
pub fn melee(kind: usize) -> u32 {
    CREW_MELEE[kind % 3]
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
        (CREW_RAM[crew.class as usize % 3] * RAM_POWER[kind as usize % 12]) as u64
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

/// Hull points of barbarian strongholds, creatures and lairs. Placeholder.
pub const MONSTER_STRENGTH: f32 = 2000.0;

/// The owner given to creatures and their lairs: hostile to every player.
pub const MONSTERS: (u32, u32) = (99, 0);

/// Hull points of a building. Placeholder: three times its amber cost.
pub fn building_strength(class: u32) -> f32 {
    (crate::economy::building_cost(class).max(400) * 3) as f32
}

/// Forts (classes 0x34-0x36) shoot from their emplacements. Fort records
/// flag twelve emplacements in fields 22-33 (wood fort 3, stone fort 8);
/// what each one holds is not known yet, so each fires the lightest
/// weapon (item 3).
fn fort_weapons(class: u32, fields: &[u32]) -> Vec<u8> {
    if !(0x34..=0x36).contains(&class) {
        return Vec::new();
    }
    let n = fields
        .get(22..34)
        .map_or(0, |f| f.iter().filter(|&&v| v == 1).count());
    vec![3; n]
}

/// Items a ship type carries when built (0x5964d8: a count and up to nine
/// items per ship type; see `ITEM_NAMES`).
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

/// Weapons of a ship from its mission record: item slots 3-8 (fields
/// 21-26) hold how many of each it carries (fields 29.. their condition,
/// eight per item). Records too short for that get the ship type's default
/// load.
pub fn weapons_from_record(fields: &[u32], kind: u8) -> Vec<u8> {
    match fields.get(18..27) {
        Some(counts) => counts
            .iter()
            .enumerate()
            .skip(3)
            .flat_map(|(w, &n)| std::iter::repeat_n(w as u8, n.min(8) as usize))
            .collect(),
        None => DEFAULT_WEAPONS[kind as usize]
            .iter()
            .copied()
            .filter(|&w| w >= 3)
            .collect(),
    }
}

/// Damage of each item against hulls and against crews, at upgrade level
/// 0 (0x597d2c rows 0-8; each value has three upgrade levels; the third
/// value of the marine rows is their boarding strength).
const ITEM_DAMAGE: [(u32, u32); 9] = [
    (5, 45),
    (7, 25),
    (3, 5),
    (40, 80),
    (60, 60),
    (80, 40),
    (200, 80),
    (250, 100),
    (220, 80),
];

pub fn hull_damage(weapon: u8) -> f32 {
    ITEM_DAMAGE.get(weapon as usize).map_or(0, |w| w.0) as f32
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

/// The sound of a volley, by its heaviest item.
pub fn fire_sound(item: u8) -> &'static str {
    match item {
        0..=2 => "strel1",
        4 => "gastsn1",
        6 => "thrbal1",
        _ => "thrcat1",
    }
}

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

    const SWORDS: Crew = Crew {
        class: 2,
        squads: [0, 0, 1],
    };

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

    #[test]
    fn crews_lose_squads_in_order() {
        let mut crew = Crew {
            class: 0,
            squads: [1, 0, 2],
        };
        assert_eq!(
            (
                crew.count(),
                crew.kind_of(0),
                crew.kind_of(1),
                crew.kind_of(2)
            ),
            (3, 0, 2, 2)
        );
        crew.lose(0);
        assert_eq!(crew.squads, [0, 0, 2]);
        crew.lose(1);
        assert_eq!(crew.squads, [0, 0, 1]);
        assert_eq!(crew.describe(), "1 swordsmen");
    }

    #[test]
    fn items_read_from_records() {
        // A royal ship: bowmen, swordsmen, small catapult, onager.
        let mut fields = vec![0u32; 30];
        fields[14] = 5;
        fields[18..27].copy_from_slice(&[1, 0, 1, 0, 0, 1, 0, 0, 1]);
        assert_eq!(weapons_from_record(&fields, 2), vec![5, 8]);
        let crew = Crew::from_record(&fields, 2);
        assert_eq!((crew.class, crew.squads), (2, [1, 0, 1]));
        // Built in play: the type's default load (0, 2, 4, 8).
        assert_eq!(weapons_from_record(&fields[..13], 2), vec![4, 8]);
        assert_eq!(Crew::from_record(&fields[..13], 2).squads, [1, 0, 1]);
    }
}
