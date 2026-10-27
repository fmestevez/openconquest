//! Ship combat: weapons, hull damage and the effects drawn for them.
//!
//! The weapon tables (damage, rate of fire, range) and ships' hull
//! strength come from ANCIENT.EXE.

use openac_formats::{ObjectClass, ObjectRecord};

use crate::objects::Layer;

/// Hull and weapons of an object that can fight or be attacked.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Combatant {
    pub hull: f32,
    pub max_hull: f32,
    /// Weapon kinds carried.
    pub weapons: Vec<u8>,
    /// Seconds until the next volley (unused: kept for older saves).
    pub reload: f32,
    /// Seconds until each weapon fires again, by its index in `weapons`.
    #[serde(default)]
    pub reloads: Vec<f32>,
    /// Seconds left burning (unused: kept for older saves).
    #[serde(default)]
    pub burning: f32,
    /// Wounds of the crew aboard: every `SQUAD_LIFE` costs a squad.
    #[serde(default)]
    pub wounds: f32,
    /// Seconds left turned to stone by the Gorgon: no moving, no firing.
    #[serde(default)]
    pub stone: f32,
    /// Seconds left invisible to other players (Phantom object).
    #[serde(default)]
    pub phantom: f32,
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
                // The unique ships' own weapon comes with the ship.
                let mut weapons = weapons_from_record(&rec.fields, kind);
                weapons.extend(special_weapon(kind));
                (hull_strength(kind), condition, weapons)
            }
            ObjectClass::Building(c) => (building_strength(c), f(7), fort_weapons(c, &rec.fields)),
            // Barbarian strongholds, creatures and their lairs can be
            // destroyed too (victory conditions count them).
            ObjectClass::Barbarian(_)
            | ObjectClass::Castle(_)
            | ObjectClass::HarpyNest
            | ObjectClass::CreatureSpawn => (
                building_strength(rec.class_id),
                100,
                creature_weapons(rec.class_id),
            ),
            ObjectClass::Monster(0x1f4..=0x1f6 | 0x1f8..=0x1fb | 0x1fd) => (
                creature_strength(rec.class_id),
                100,
                creature_weapons(rec.class_id),
            ),
            _ => return None,
        };
        Some(Self {
            hull: max_hull * condition.min(100) as f32 / 100.0,
            max_hull,
            weapons,
            reload: 0.0,
            reloads: Vec::new(),
            burning: 0.0,
            phantom: 0.0,
            stone: 0.0,
            wounds: 0.0,
            protect_weapons: 0.0,
            protect_magic: 0.0,
        })
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

/// The name of any item, the unique ships' weapons included.
pub fn item_name(item: u8) -> &'static str {
    match item {
        GREEK_FIRE => "Greek fire",
        QUICKLIME => "quicklime",
        POWDER => "barrel of powder",
        _ => ITEM_NAMES[item as usize % 9],
    }
}

pub const CREW_NAMES: [&str; 3] = ["bowmen", "javelin men", "swordsmen"];

/// Cost of training a squad of marines: 30 amber each (u16 at 0x597164)
/// and fish (0x597170 x 100).
pub const CREW_COST: [(u32, u32); 3] = [(30, 200), (30, 200), (30, 300)];

/// Seconds to train a squad of marines or build a weapon `item`: game
/// ticks by item group, 70 marines, 80 medium and 95 heavy
/// weapons (0x5971a0; the settlement 0x43c740 and the workshop 0x4369c0
/// show progress as elapsed x 100 / (ticks x queued)).
pub fn train_seconds(item: u8) -> f32 {
    crate::clock::ticks([70, 80, 95][(item / 3).min(2) as usize])
}

/// Room aboard a ship type for marines and weapons, from the manual's
/// comparison chart ("displacement for weapons or for cargo"); it matches
/// the types' default armings. The special ships' own weapons fill their
/// room, except one more weapon on the quick lime ship.
const CAPACITY: [u32; 12] = [4, 10, 5, 7, 4, 3, 2, 2, 0, 1, 0, 3];

pub fn capacity(kind: u8) -> u32 {
    CAPACITY[kind as usize % 12]
}

/// Room an item takes: one for a squad of marines or a medium weapon, two
/// for a strong one (the manual's chart).
pub fn item_size(item: u8) -> u32 {
    if (6..=8).contains(&item) {
        2
    } else {
        1
    }
}

/// Room taken aboard by `crew` and `weapons`.
pub fn load(crew: &Crew, weapons: &[u8]) -> u32 {
    crew.count() + weapons.iter().map(|&w| item_size(w)).sum::<u32>()
}

/// Whether ships of type `kind` can carry weapons: merchant ships cannot.
pub fn carries_weapons(kind: u8) -> bool {
    kind >= 2
}

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
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
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
                    *s = n.min(capacity(kind));
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

/// Strength of a creature, over 15 as hulls are: cyclopes 30000,
/// minotaurs 16000, amazons 12000 (CyclopClassTy 0x413d70), the gorgon
/// 150000 (MedusaClassTy 0x401000), sirens 100000 (SirenaClassTy 0x403050,
/// 0x404a20), sea monsters 50000 (UMonsterClassTy 0x492360, by the
/// record's percentage); harpies and dragons not yet found (2000, a
/// placeholder).
pub fn creature_strength(class: u32) -> f32 {
    match class {
        0x1f4 => 30_000.0 / 15.0,
        0x1f5 => 16_000.0 / 15.0,
        0x1f6 => 12_000.0 / 15.0,
        0x1fa => 150_000.0 / 15.0,
        0x1fd => 100_000.0 / 15.0,
        0x1fb => 50_000.0 / 15.0,
        _ => 2000.0,
    }
}

/// The owner given to creatures and their lairs: hostile to every player.
pub const MONSTERS: (u32, u32) = (99, 0);

/// Strength of a building, barbarian stronghold, castle, nest or lair, by
/// class (LandBuilding 0x424910), over 15 as hulls are (see
/// `hull_strength`). It takes blows whole, a fifth of them under a magic
/// shield (0x425990).
pub fn building_strength(class: u32) -> f32 {
    let points = match class {
        0x32 => 160_000,
        0x33 => 220_000,
        0x34..=0x36 | 0x3f..=0x41 => 180_000,
        0x37 | 0x3a | 0x46 => 100_000,
        0x38 | 0x39 => 130_000,
        0x3b => 140_000,
        0x3c => 81_500,
        0x3d => 67_500,
        0x3e => 110_000,
        0x42 | 0x43 => 300_000,
        0x44 => 350_000,
        0x45 => 280_000,
        0x47 => 80_000,
        0x48 => 60_000,
        0x49 => 40_000,
        _ => 100_000,
    };
    points as f32 / 15.0
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

/// What a ship type takes instead of each of its `DEFAULT_WEAPONS` when
/// that one is not to be had (0x5962f8), and more of these, from the
/// last, until it is as strong as `MIN_POWER` (0x597ae4).
pub const FALLBACK_WEAPONS: [&[u8]; 12] = [
    &[],
    &[],
    &[0, 2, 4, 5],
    &[0, 1, 4, 6, 7],
    &[1, 1, 2, 2],
    &[1, 4, 4],
    &[4, 4],
    &[3, 0],
    &[],
    &[1],
    &[],
    &[],
];
pub const MIN_POWER: [u32; 12] = [0, 0, 5, 7, 4, 3, 2, 2, 0, 1, 1, 3];

/// The strength an item adds (0x59729c, by group): marines and light
/// weapons 1, heavy weapons 2.
pub fn item_power(item: u8) -> u32 {
    [1, 1, 2][(item / 3).min(2) as usize]
}

/// Armour level (0-2) a ship type is built with, and the one it falls
/// back to (0x596268, 0x596238).
pub const DEFAULT_ARMOUR: [(u8, u8); 12] = [
    (0, 0),
    (0, 0),
    (2, 0),
    (2, 1),
    (1, 0),
    (1, 0),
    (0, 0),
    (1, 0),
    (0, 0),
    (1, 0),
    (0, 0),
    (0, 0),
];
/// Ram level (0-2) a ship type is built with, and its fallback (0x5962c8,
/// 0x596298, which count from 3).
pub const DEFAULT_RAM: [(u8, u8); 12] = [
    (0, 0),
    (0, 0),
    (2, 0),
    (0, 0),
    (0, 0),
    (2, 1),
    (1, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
    (0, 0),
];

/// Weapons a fort (class 0x34-0x36) is built with and their fallbacks
/// (0x5966b8, 0x596730: 40 bytes a fort). A fort can only be put up when
/// each fallback is in stock or may be made.
pub fn fort_arming(class: u32) -> (&'static [u8], &'static [u8]) {
    match class {
        0x34 => (&[4, 5, 6], &[4, 4, 4, 4]),
        0x35 => (&[0, 4, 4, 6, 7], &[0, 0, 4, 4, 4, 4, 4]),
        0x36 => (&[0, 1, 4, 5, 6, 7, 8], &[0, 0, 4, 4, 4, 4, 4, 6, 6]),
        _ => (&[], &[]),
    }
}

/// Weapons of a ship from its mission record: item slots 3-8 (fields
/// 21-26) hold how many of each it carries (fields 29.. their condition,
/// eight per item). Records too short for that get the ship type's default
/// load. Boats always carry one item 3 and merchants one item 4, whatever
/// the record holds (the ship loaders 0x4dfa90, 0x4dfe00).
pub fn weapons_from_record(fields: &[u32], kind: u8) -> Vec<u8> {
    match kind {
        0 => return vec![3],
        1 => return vec![4],
        _ => {}
    }
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

pub fn hull_damage(weapon: u8) -> f32 {
    hull_damage_at(weapon, 0)
}

/// The unique ships' weapons: Greek fire (the fire ship's ballista), the
/// quick lime ship's catapult, and the fire rammer's barrel of powder.
pub const GREEK_FIRE: u8 = 9;
pub const QUICKLIME: u8 = 10;
pub const POWDER: u8 = 11;

/// Hull damage of an item at weapons level 0-2 (0x597d2c, A values; the
/// unique ships' weapons from the manual's chart, which gives no upgraded
/// values for them).
pub fn hull_damage_at(weapon: u8, level: u8) -> f32 {
    const A: [[u32; 3]; 12] = [
        [5, 7, 10],
        [7, 9, 12],
        [3, 5, 7],
        [40, 48, 56],
        [60, 72, 84],
        [80, 96, 112],
        [200, 240, 280],
        [250, 300, 350],
        [220, 264, 330],
        [350, 350, 350],
        [0, 0, 0],
        [400, 400, 400],
    ];
    A.get(weapon as usize)
        .map_or(0, |a| a[level.min(2) as usize]) as f32
}

/// Damage to crews of an item at weapons level 0-2 (0x597d2c, B values;
/// the unique ships' weapons from the manual).
pub fn crew_damage_at(weapon: u8, level: u8) -> f32 {
    const B: [[u32; 3]; 12] = [
        [45, 56, 68],
        [25, 31, 38],
        [5, 8, 11],
        [80, 96, 112],
        [60, 72, 84],
        [40, 48, 56],
        [80, 96, 112],
        [100, 120, 140],
        [80, 96, 112],
        [100, 100, 100],
        [300, 300, 300],
        [150, 150, 150],
    ];
    B.get(weapon as usize)
        .map_or(0, |b| b[level.min(2) as usize]) as f32
}

/// Whether item `weapon` is fired (the barrel of powder is not: it goes off
/// when its ship rams).
pub fn fires(weapon: u8) -> bool {
    weapon != POWDER
        && (hull_damage(weapon) > 0.0 || crew_damage_at(weapon, 0) > 0.0)
        && weapon != 2
}

/// Wound points that cost a crew one squad. Placeholder.
pub const SQUAD_LIFE: f32 = 150.0;
/// Crew wound points healed a second by the Cup of Asclepius and the Golden
/// Fleece. Placeholder.
pub const CREW_HEALING: f32 = 5.0;
/// The ship of the dead's undead crews are this much stronger in boarding.
/// Placeholder.
pub const UNDEAD_MELEE: u32 = 2;

/// The unique weapon built into a ship type, if any.
pub fn special_weapon(kind: u8) -> Option<u8> {
    match kind {
        8 => Some(GREEK_FIRE),
        9 => Some(QUICKLIME),
        10 => Some(POWDER),
        _ => None,
    }
}

/// What the wild creatures strike with, as the nearest weapons (the
/// manual: cyclops throw stones, minotaurs axes "similar to a Scorpion's",
/// amazons arrows; sea monsters bite). Their lairs send them out
/// (`scene::walkers`) and do not fight themselves. Placeholders. (Harpies and dragons
/// strike as they fly: `scene::flyers`.)
pub fn creature_weapons(class_id: u32) -> Vec<u8> {
    match class_id {
        0x1f4 => vec![5],
        0x1f5 => vec![3],
        0x1f6 => vec![0, 0],
        0x1fb => vec![4; 4],
        _ => Vec::new(),
    }
}

/// Hull points of a ship type: its chart value (0x5bb4b0) x 650 (0x4ac210),
/// over 15, as the original's shots deal A x 15 (0x4ac140) where ours deal
/// A.
pub fn hull_strength(kind: u8) -> f32 {
    const CHART: [u32; 12] = [28, 90, 100, 120, 65, 70, 36, 36, 65, 75, 110, 120];
    CHART[kind as usize % 12] as f32 * 650.0 / 15.0
}

/// The share of a blow a ship's hull takes (0x4befc0, shares 0x4beb50):
/// the type's hull share (0x597ad8) against it with the armour, the ram,
/// the weapons aboard and the loads of cargo; the rest falls on what is
/// aboard.
pub fn hull_share(kind: u8, armour: u8, ram: u8, weapons: usize, loads: u32) -> f32 {
    const SHARE: [u32; 12] = [7, 10, 21, 18, 10, 12, 6, 6, 3, 4, 2, 4];
    let hull = SHARE[kind as usize % 12];
    hull as f32 / (hull + armour as u32 + ram as u32 + weapons as u32 + loads) as f32
}

/// Rate of fire per item and level (0x597a3c, item x 3 + level; the
/// manual's chart lists the same figures).
const RATE: [[u32; 3]; 11] = [
    [120, 150, 150],
    [120, 150, 150],
    [120, 150, 150],
    [100, 130, 130],
    [120, 120, 120],
    [90, 110, 110],
    [70, 90, 90],
    [60, 80, 80],
    [70, 90, 90],
    [50, 70, 80],
    [60, 80, 80],
];

/// Seconds between shots of item `item` at rate of fire level `level`. The
/// unit of the rate is not known: a rate of 100 is taken as one shot every
/// three seconds.
pub fn reload_seconds(item: u8, level: u8) -> f32 {
    let rate = RATE
        .get(item as usize)
        .map_or(100, |r| r[level.min(2) as usize]);
    300.0 / rate.max(1) as f32
}

/// Longest and shortest range of item `item` in cells, at range level
/// `level` (0x598154: maximum, 12 items per level; 0x598178: minimum).
pub fn range_cells(item: u8, level: u8) -> (u8, u8) {
    const MAX: [[u8; 12]; 3] = [
        [3, 2, 2, 3, 3, 3, 5, 3, 4, 2, 3, 0],
        [3, 2, 2, 4, 3, 3, 5, 4, 4, 2, 3, 0],
        [4, 3, 3, 4, 4, 4, 6, 5, 5, 3, 4, 0],
    ];
    const MIN: [u8; 12] = [0, 0, 0, 0, 0, 1, 2, 1, 1, 0, 0, 0];
    let i = (item as usize).min(11);
    (MIN[i], MAX[level.min(2) as usize][i])
}

/// The world distances, in pixels, at which item `item` reaches: within
/// its longest range in cells (counted to the far edge of the cell) and
/// beyond its shortest.
pub fn reach(item: u8, level: u8) -> (f32, f32) {
    let (min, max) = range_cells(item, level);
    (min as f32 * 64.0, (max as f32 + 0.5) * 64.0)
}

/// Chance that a shot hits. Placeholder.
pub const HIT_CHANCE: u32 = 70;

/// Research at the sage school (SchoolClassTy, menu 0x430f90): each
/// player's weapons, ram, armour and movement levels, 0 to 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Upgrade {
    /// The school's "arming" (+0xa3); in the original it opens the Greek
    /// special-ship upgrades.
    Weapons,
    Ram,
    Armour,
    Speed,
    /// The workshop's damage level for item group 0 (marines), 1 (medium
    /// weapons, items 3-5) or 2 (strong weapons, 6-8): player bytes
    /// +0x9c + group, read by the damage routine 0x4bb670 as the column
    /// level of every item in the group.
    Arms(u8),
    /// The workshop's rate of fire level per group (+0x8e, table 0x597a3c).
    Rate(u8),
    /// The workshop's range level per group (+0x95, tables 0x598154).
    Reach(u8),
    /// The sage school's study of a unique ship, which lets it be built:
    /// 0 Greek fire ship, 1 quick lime ship, 2 fire rammer, 3 ship of the
    /// dead (commands 0x50-0x53, 2000 amber each, offered once the arming
    /// is at level 2; menu 0x430f90).
    Special(u8),
}

impl Upgrade {
    /// In menu order (slots 0-3).
    pub const ALL: [Upgrade; 4] = [
        Upgrade::Weapons,
        Upgrade::Ram,
        Upgrade::Armour,
        Upgrade::Speed,
    ];

    /// Command ids: 0x3c UPARMING, 0x48 UPRAM, 0x47 UPARMOR, 0x4f (the move
    /// button).
    pub fn command(self) -> u8 {
        match self {
            Upgrade::Weapons => 0x3c,
            Upgrade::Ram => 0x48,
            Upgrade::Armour => 0x47,
            Upgrade::Speed => 0x4f,
            // `CBUT_UPGARMING<W|M|S><1|2|3>` (workshop menu page 3).
            Upgrade::Arms(g) => 0x3f + 3 * g.min(2),
            Upgrade::Rate(g) => 0x3e + 3 * g.min(2),
            Upgrade::Reach(g) => 0x40 + 3 * g.min(2),
            Upgrade::Special(k) => 0x50 + k.min(3),
        }
    }

    pub fn of_command(c: u8) -> Option<Self> {
        Self::ALL
            .into_iter()
            .chain((0..3).flat_map(|g| [Upgrade::Arms(g), Upgrade::Rate(g), Upgrade::Reach(g)]))
            .chain((0..4).map(Upgrade::Special))
            .find(|u| u.command() == c)
    }

    /// The highest level: 2, except rate of fire, which marines cannot
    /// improve and medium weapons only once (0x59715c).
    pub fn max_level(self) -> u8 {
        match self {
            Upgrade::Rate(g) => g.min(2),
            // Arming is studied once, from the original's 1 to 2 (the
            // school offers it while +0xa3 < 2).
            Upgrade::Weapons | Upgrade::Special(_) => 1,
            _ => 2,
        }
    }

    /// Amber to raise the level from `level` (0x5971ac for weapons, 0x42d9a0
    /// for ram and armour, 0x42d980 for movement; for the workshop, by group
    /// x 3 + level at 0x5971c4 rate, 0x597254 damage, 0x59720c range).
    pub fn cost(self, level: u8) -> u32 {
        let g = |g: u8| g.min(2) as usize;
        let costs = match self {
            // Indexed by the original's arming, which starts at 1.
            Upgrade::Weapons => [1300, 1300],
            Upgrade::Ram | Upgrade::Armour => [1000, 1500],
            Upgrade::Speed => [2000, 3000],
            Upgrade::Arms(n) => [[1500, 1700], [1700, 1800], [1800, 2600]][g(n)],
            Upgrade::Rate(n) => [[0, 0], [1500, 0], [1700, 1900]][g(n)],
            Upgrade::Reach(n) => [[1000, 1900], [1800, 2400], [1900, 2800]][g(n)],
            Upgrade::Special(_) => [2000, 2000],
        };
        costs[level.min(1) as usize]
    }

    /// Seconds to study the level after `from`: game ticks. The
    /// sage school (0x432210): arming 0x5971b8[+0xa3] (450, the original's
    /// arming starting at 1), ram, armour and rigging 600 then 700, a
    /// unique ship 400. The workshop (0x436ee0): the second half of each
    /// cost table, by group x 3 + level (0x5971e8 rate of fire, 0x597230
    /// range, 0x597278 damage).
    pub fn seconds(self, from: u8) -> f32 {
        let l = from.min(1) as usize;
        let g = |g: u8| g.min(2) as usize * 3 + l;
        let ticks = match self {
            Upgrade::Weapons => [400, 450][(from + 1).min(1) as usize],
            Upgrade::Ram | Upgrade::Armour | Upgrade::Speed => [600, 700][l],
            Upgrade::Special(_) => 400,
            Upgrade::Rate(n) => [0, 0, 0, 200, 0, 0, 250, 290, 0][g(n)],
            Upgrade::Reach(n) => [150, 285, 0, 270, 360, 0, 240, 420, 0][g(n)],
            Upgrade::Arms(n) => [100, 125, 0, 100, 170, 0, 170, 150, 0][g(n)],
        };
        crate::clock::ticks(ticks)
    }

    pub fn name(self) -> &'static str {
        match self {
            Upgrade::Weapons => "weapons",
            Upgrade::Ram => "rams",
            Upgrade::Armour => "armour",
            Upgrade::Speed => "rigging",
            Upgrade::Arms(0) => "marines' strength",
            Upgrade::Arms(1) => "medium weapons' strength",
            Upgrade::Arms(_) => "strong weapons' strength",
            Upgrade::Rate(0) => "marines' rate of fire",
            Upgrade::Rate(1) => "medium weapons' rate of fire",
            Upgrade::Rate(_) => "strong weapons' rate of fire",
            Upgrade::Reach(0) => "marines' range",
            Upgrade::Reach(1) => "medium weapons' range",
            Upgrade::Reach(_) => "strong weapons' range",
            Upgrade::Special(0) => "Greek fire ships",
            Upgrade::Special(1) => "quick lime ships",
            Upgrade::Special(2) => "fire rammers",
            Upgrade::Special(_) => "ships of the dead",
        }
    }
}

/// A player's research levels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Levels {
    pub weapons: u8,
    pub ram: u8,
    pub armour: u8,
    pub speed: u8,
    /// Damage level per item group (see `Upgrade::Arms`).
    #[serde(default)]
    pub arms: [u8; 3],
    /// Rate of fire and range levels per item group.
    #[serde(default)]
    pub rate: [u8; 3],
    #[serde(default)]
    pub reach: [u8; 3],
    /// Unique ships studied (see `Upgrade::Special`).
    #[serde(default)]
    pub special: [u8; 4],
}

impl Levels {
    /// The levels a mission gives a player at the start. The arming is
    /// one less here: the original's starts at 1.
    pub fn starting(s: &openac_formats::StartLevels) -> Self {
        let cap = |v: u8| v.min(2);
        Self {
            weapons: s.arming.saturating_sub(1).min(1),
            ram: cap(s.ram),
            armour: cap(s.armour),
            speed: 0,
            arms: s.arms.map(cap),
            rate: s.rate.map(cap),
            reach: s.reach.map(cap),
            special: [0; 4],
        }
    }

    pub fn get(&self, u: Upgrade) -> u8 {
        match u {
            Upgrade::Weapons => self.weapons,
            Upgrade::Ram => self.ram,
            Upgrade::Armour => self.armour,
            Upgrade::Speed => self.speed,
            Upgrade::Arms(g) => self.arms[g.min(2) as usize],
            Upgrade::Rate(g) => self.rate[g.min(2) as usize],
            Upgrade::Reach(g) => self.reach[g.min(2) as usize],
            Upgrade::Special(k) => self.special[k.min(3) as usize],
        }
    }

    /// The damage level of item `item`.
    pub fn arms_for(&self, item: u8) -> u8 {
        self.arms[(item / 3).min(2) as usize]
    }

    /// The rate of fire and range levels of item `item`.
    pub fn rate_for(&self, item: u8) -> u8 {
        self.rate[(item / 3).min(2) as usize]
    }

    pub fn reach_for(&self, item: u8) -> u8 {
        self.reach[(item / 3).min(2) as usize]
    }

    pub fn set(&mut self, u: Upgrade, v: u8) {
        let slot = match u {
            Upgrade::Weapons => &mut self.weapons,
            Upgrade::Ram => &mut self.ram,
            Upgrade::Armour => &mut self.armour,
            Upgrade::Speed => &mut self.speed,
            Upgrade::Arms(g) => &mut self.arms[g.min(2) as usize],
            Upgrade::Rate(g) => &mut self.rate[g.min(2) as usize],
            Upgrade::Reach(g) => &mut self.reach[g.min(2) as usize],
            Upgrade::Special(k) => &mut self.special[k.min(3) as usize],
        };
        *slot = (*slot).max(v.min(u.max_level()));
    }
}

/// A smithy refit: armour or ram for one ship, to level 1 or 2 (commands
/// 0x36, 0x37 armour M/S and 0x38, 0x39 ram M/S, menu 0x433e60).
pub fn refit_of_command(c: u8) -> Option<(Upgrade, u8)> {
    match c {
        0x36 | 0x37 => Some((Upgrade::Armour, c - 0x36 + 1)),
        0x38 | 0x39 => Some((Upgrade::Ram, c - 0x38 + 1)),
        _ => None,
    }
}

/// What a refit to `level` costs for a ship of type `kind`: 30/60 amber
/// (armour, 0x42d960) or 25/50 (ram, 0x42d940) times the ship's size. The
/// size is taken from the per-type values at 0x596110 (boat 10, merchant
/// 20, royal ship 25, ...), which is an assumption.
pub fn refit_cost(upgrade: Upgrade, level: u8, kind: u8) -> u32 {
    const SIZE: [u32; 12] = [10, 20, 25, 20, 10, 10, 10, 10, 10, 10, 10, 20];
    let base = match (upgrade, level) {
        (Upgrade::Armour, 1) => 30,
        (Upgrade::Armour, _) => 60,
        (_, 1) => 25,
        _ => 50,
    };
    base * SIZE[kind as usize % 12]
}

/// What armour leaves of a blow (0x4befc0): the cut at 0x597a00 by the
/// fitted kind's row (kind x 3) and the player's level in it, taken here at
/// 0: none, medium and strong armour cut 0, 20 and 37 percent.
pub fn armour_factor(level: u8) -> f32 {
    [1.0, 0.8, 0.63][level.min(2) as usize]
}

/// Rigging: 115 and 130 percent of the top speed (0x4ac210).
pub fn speed_factor(level: u8) -> f32 {
    [1.0, 1.15, 1.3][level.min(2) as usize]
}

/// Effect animations: a heavy shot striking a ship and falling in the
/// water (`BOMB<letter><state>0`, see `scene::shot`), used for blows
/// without a shot (rams, hazards).
pub const FX_HIT: &str = "BOMBG10";
pub const FX_SPLASH: &str = "BOMBG20";
pub const FX_SINK: &str = "BOMBG10";
pub const FX_FRAME_MS: f32 = 70.0;

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
    /// A tornado: its course, which also picks its frame.
    pub tornado: Option<Tornado>,
    /// Milliseconds a frame.
    pub frame_ms: f32,
    /// Shows the ship being built at this shipyard (its frame is set from
    /// the production's progress).
    pub site: Option<u32>,
}

/// A tornado (Tornado.cpp 0x43a380), a frame every 2 ticks: it rises
/// (frames 0-41), then hops from cell to cell, 16 frames a hop, looping
/// frames 42-83, until 250 ticks have passed at the end of a hop, and sinks
/// (84-124).
#[derive(Debug, Clone)]
pub struct Tornado {
    /// Seconds owed to its next frame.
    pub clock: f32,
    pub frame: usize,
    /// 1 rising, 2 moving, 3 sinking, 0 gone.
    pub phase: u8,
    /// Seconds since it began to move.
    pub moving: f32,
    /// Frames into the hop, and its ends (world pixels).
    pub hop: u32,
    pub from: (f32, f32),
    pub to: (f32, f32),
}

impl Effect {
    /// Holds frame `n` (for animations driven by something else).
    pub fn show_frame(&mut self, n: usize) {
        self.age_ms = n as f32 * self.frame_ms + 1.0;
        self.life_ms = Some(f32::MAX);
    }

    /// The current frame, or `None` once the animation has finished.
    pub fn frame(&self) -> Option<&[Layer]> {
        if let Some(t) = &self.tornado {
            let last = self.frames.len().checked_sub(1)?;
            return (t.phase != 0).then(|| self.frames[t.frame.min(last)].as_slice());
        }
        let step = (self.age_ms / self.frame_ms) as usize;
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

    #[test]
    fn ranges_and_rates() {
        // Ballistae outrange scorpions; catapults cannot fire point-blank.
        assert!(reach(6, 0).1 > reach(3, 0).1);
        assert_eq!(range_cells(7, 0), (1, 3));
        assert_eq!(reach(3, 0).0, 0.0);
        // Faster with upgrades; heavy engines slower than light ones.
        assert!(reload_seconds(3, 1) < reload_seconds(3, 0));
        assert!(reload_seconds(7, 0) > reload_seconds(3, 0));
        assert_eq!(Upgrade::of_command(0x46), Some(Upgrade::Reach(2)));
        assert_eq!(Upgrade::of_command(0x3e), Some(Upgrade::Rate(0)));
    }

    #[test]
    fn default_armings_fill_the_room() {
        // The manual's displacement matches the default armings exactly.
        for kind in 2..8u8 {
            let load: u32 = DEFAULT_WEAPONS[kind as usize]
                .iter()
                .map(|&w| item_size(w))
                .sum();
            assert_eq!(load, capacity(kind), "kind {kind}");
        }
    }

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
