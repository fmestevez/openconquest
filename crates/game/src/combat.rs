//! Ship combat: weapons, hull damage and the effects drawn for them.
//!
//! The weapon tables (damage, rate of fire, range) and ships' hull
//! strength come from ANCIENT.EXE.

use openconquest_formats::{ObjectClass, ObjectRecord};

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
    /// Unused: kept for older saves (units keep their own conditions,
    /// `objects::Ship::conditions`).
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
    /// Each unit's condition in percent by its index in `weapons`, for
    /// forts and barbarian structures (their places' units, `+0x4d0` and
    /// `+0x20c`; record fields from 34). Missing ones are whole. Ships keep
    /// theirs in `objects::Ship::conditions`.
    #[serde(default)]
    pub conditions: Vec<u8>,
    /// A fort's count towards its next healing by a Cup of Asclepius or a
    /// Golden Fleece it stores (every 1000 heals a point).
    #[serde(default)]
    pub healing: f32,
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
            // Barbarian structures hold units as forts do, and fire with
            // them (BarbarianClassTy mends them, 0x40eba0).
            ObjectClass::Barbarian(c) => (
                building_strength(rec.class_id),
                100,
                fort_weapons(c, &rec.fields),
            ),
            ObjectClass::Castle(_) | ObjectClass::HarpyNest | ObjectClass::CreatureSpawn => (
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
        // Forts' and barbarian structures' units have their conditions in
        // the fields from 34, one after another (FortressClassTy 0x419f80
        // message 2; saved by 0x4190a0, at most 30).
        let conditions = if matches!(class, ObjectClass::Building(_) | ObjectClass::Barbarian(_)) {
            (0..weapons.len())
                .map(|k| rec.fields.get(34 + k).map_or(100, |&c| c.min(100) as u8))
                .collect()
        } else {
            Vec::new()
        };
        Some(Self {
            hull: max_hull * condition.min(100) as f32 / 100.0,
            max_hull,
            weapons,
            conditions,
            healing: 0.0,
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

/// Boarding strength of each marine kind (0x597d2c rows 0-2, third value
/// at level 0).
const CREW_MELEE: [u32; 3] = [35, 68, 100];

/// Ram quality (0x598054, by the ram fitted and the player's level in it;
/// level 0).
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

/// A ship in a ram: its type, ram level, heading and speed (world pixels
/// a second).
pub struct Rammer {
    pub kind: u8,
    pub ram: u8,
    pub heading: usize,
    pub speed: f32,
}

/// The share of a ram's blow the hull takes, over its share (0x598094:
/// 150 for every type), and the crew's (0x598090: 40); the weapons take
/// theirs (0x4c1ad0).
pub const RAM_HULL_FACTOR: f32 = 1.5;
pub const RAM_CREW_FACTOR: f32 = 0.4;

/// The blows of a ram (0x4c1c60), to the rammed ship and to the rammer,
/// in our units (the original's over 15, see `hull_strength`). The
/// closing speed is the two headings' vectors times their speeds, less one
/// from the other, times 0.707, half of it in percent of a boat's top
/// speed; each blow is the striking side's ram quality (0x598054) x its
/// power over the other's (0x598024) x the smaller of 500000 and its angle
/// factor (0x598078 for the rammer, 0x598084 for the rammed) x its hull /
/// 100 x that percentage, over 2500.
pub fn ram_blows(a: Rammer, t: Rammer, angle: usize) -> (f32, f32) {
    const DIRS: [(f32, f32); 8] = [
        (1.0, 0.0),
        (1.0, -1.0),
        (0.0, -1.0),
        (-1.0, -1.0),
        (-1.0, 0.0),
        (-1.0, 1.0),
        (0.0, 1.0),
        (1.0, 1.0),
    ];
    // Speeds in the original's units, 1/6000 pixel a tick.
    let unit = crate::clock::TICK * 6000.0;
    let (da, dt) = (DIRS[a.heading % 8], DIRS[t.heading % 8]);
    let (va, vt) = (a.speed * unit, t.speed * unit);
    let rel = (
        (da.0 * va - dt.0 * vt) * 0.707,
        (da.1 * va - dt.1 * vt) * 0.707,
    );
    // A boat's top speed: its chart's 150 x 400.
    let impact = rel.0.hypot(rel.1) / 2.0 * 100.0 / 60_000.0;
    let power = |k: u8| RAM_POWER[k as usize % 12] as f32;
    let hull = |k: u8| hull_strength(k) * 15.0;
    let blow = |q: u8, own: u8, other: u8, angle_factor: u32| {
        let strike = (angle_factor as f32 * hull(own) / 100.0 * impact).min(500_000.0);
        CREW_RAM[q.min(2) as usize] as f32 * power(own) / power(other) * strike / 2500.0 / 15.0
    };
    let angle = angle.min(2);
    (
        blow(a.ram, a.kind, t.kind, RAM_ANGLE_ATTACK[angle]),
        blow(t.ram, t.kind, a.kind, RAM_ANGLE_DEFENCE[angle]),
    )
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

/// How long a boarding takes when its picture (`ABORD_`) cannot be found:
/// the picture's length decides it otherwise (a frame a tick).
pub const BOARD_ROUND: f32 = 1.0;

/// Strength of a creature, over 15 as hulls are: cyclopes 30000,
/// minotaurs 16000, amazons 12000 (CyclopClassTy 0x413d70), the gorgon
/// 150000 (MedusaClassTy 0x401000), sirens 100000 (SirenaClassTy 0x403050,
/// 0x404a20), sea monsters 50000 (UMonsterClassTy 0x492360, by the
/// record's percentage), harpies 200000 and dragons 250000 (HarpyClassTy
/// 0x4207b0).
pub fn creature_strength(class: u32) -> f32 {
    match class {
        0x1f4 => 30_000.0 / 15.0,
        0x1f5 => 16_000.0 / 15.0,
        0x1f6 => 12_000.0 / 15.0,
        0x1fa => 150_000.0 / 15.0,
        0x1fd => 100_000.0 / 15.0,
        0x1fb => 50_000.0 / 15.0,
        0x1f8 => 200_000.0 / 15.0,
        0x1f9 => 250_000.0 / 15.0,
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

/// Forts (classes 0x34-0x36) shoot with what they hold. Fort records give
/// the count of each item 0-11 in fields 22-33 and the units' conditions
/// from field 34 on, as a settlement's stock (FortressClassTy 0x419f80
/// builds a unit per count into its store). Items 9-11 (whose weights
/// 0x59729c has none for) are never there.
fn fort_weapons(class: u32, fields: &[u32]) -> Vec<u8> {
    // Forts and barbarian structures (whose records have the same layout:
    // the counts in fields 22-33, the units' conditions from field 34).
    if !holds_units(class) {
        return Vec::new();
    }
    fields.get(22..31).map_or(Vec::new(), |counts| {
        counts
            .iter()
            .enumerate()
            .flat_map(|(item, &n)| std::iter::repeat_n(item as u8, n.min(16) as usize))
            .collect()
    })
}

/// Whether objects of `class` hold units in places as a fort does: forts
/// 0x34-0x36 (`FortressClassTy`, places at `+0x4d0`) and barbarian
/// structures 0x3f-0x41 (`BarbarianClassTy`, `+0x20c`).
pub fn holds_units(class: u32) -> bool {
    (0x34..=0x36).contains(&class) || (0x3f..=0x41).contains(&class)
}

/// The room a fort of `class` has for units (0x5966d8 by class: small 4,
/// medium 7, big 10), counted as ships count theirs (`item_power`: 0x59729c,
/// marines and light weapons 1, heavy weapons 2); its default arming fills
/// it.
pub fn fort_room(class: u32) -> u32 {
    match class {
        0x34 => 4,
        0x35 => 7,
        0x36 => 10,
        _ => 0,
    }
}

/// The room units of `items` take in a fort (the byte 0x417ef0 sums).
pub fn fort_load(items: &[u8]) -> u32 {
    items.iter().map(|&w| item_power(w)).sum::<u32>() & 0xff
}

/// The units' share of a blow `blow` to a fort or barbarian structure of
/// `class` holding `items` (message 0x138e): `blow x load / (base + load)`,
/// the base by class (0x595920: forts 25, 30, 35, 0x4184f0; barbarian
/// structures 80, 0x40e0f0). The fort's supplies (`+0x4d4`, brought by its
/// settlement with command 0x2d, 0x419690) add to the base; nothing here
/// keeps them.
pub fn fort_share(class: u32, items: &[u8], blow: u32) -> u32 {
    let load = fort_load(items);
    if load == 0 {
        return 0;
    }
    let base = match class {
        0x34 => 25,
        0x35 => 30,
        0x36 => 35,
        _ => 80,
    };
    (blow as u64 * load as u64 / (base + load) as u64) as u32
}

/// Takes `blow` (in the units' points) off the units of a fort's or
/// barbarian structure's places (conditions by item) of items `items`
/// (0x4184f0, 0x40e0f0 inline for weapons 3-8; 0x40ff00 for marines 0-2):
/// a number is drawn below the units' points less the blow (`rand %
/// (total - blow)`, 0 when the blow is all they have) and the units are
/// walked index by index, each index through the places in item order,
/// adding up their points until the sum reaches it. From that unit on,
/// each takes what is left of the blow: one that holds is left with its
/// points less it (`x 100 / full` in percent), one that does not is gone
/// and the blow goes on less its points. A walk that ends with some of the
/// blow left draws again (the total kept as the original keeps it: a unit
/// gone does not lower it). Units at 0 are gone. Returns whether any unit
/// went.
pub fn fort_unit_blow(
    places: &mut [Vec<u8>; 9],
    items: std::ops::Range<usize>,
    blow: u32,
    rng: &mut Rng,
) -> bool {
    let points = |k: usize, c: u8| c as u32 * UNIT_POINTS[k] / 100;
    let mut total: u32 = items
        .clone()
        .flat_map(|k| places[k].iter().map(move |&c| (k, c)))
        .map(|(k, c)| points(k, c))
        .sum();
    if total == 0 {
        return false;
    }
    let mut left = blow.min(total);
    let mut gone = false;
    // The original draws until it lands; a bound keeps a stale total from
    // drawing for ever.
    for _ in 0..1000 {
        let r = if left < total {
            rng.next() % (total - left)
        } else {
            0
        };
        let most = items.clone().map(|k| places[k].len()).max().unwrap_or(0);
        if most == 0 {
            break;
        }
        let mut hit = false;
        let mut sum = 0;
        'walk: for index in 0..most {
            for k in items.clone() {
                let Some(&c) = places[k].get(index) else {
                    continue;
                };
                let p = points(k, c);
                if !hit {
                    sum += p;
                    hit = r <= sum;
                }
                if hit {
                    if left < p {
                        let rest = p - left;
                        left = 0;
                        total -= p;
                        let c = (rest * 100 / UNIT_POINTS[k]) as u8;
                        if c == 0 {
                            places[k].remove(index);
                            gone = true;
                        } else {
                            places[k][index] = c;
                            total += rest;
                        }
                    } else {
                        left -= p;
                        places[k].remove(index);
                        gone = true;
                    }
                }
                if left == 0 {
                    break 'walk;
                }
            }
        }
        if left == 0 {
            break;
        }
    }
    gone
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

/// The unique weapon built into a ship type, if any.
pub fn special_weapon(kind: u8) -> Option<u8> {
    match kind {
        8 => Some(GREEK_FIRE),
        9 => Some(QUICKLIME),
        10 => Some(POWDER),
        _ => None,
    }
}

/// What the wild creatures strike with. Cyclopes, minotaurs and amazons
/// throw a level-1 weapon's blow (0x4126f0 reads the A x 15 cache's copy
/// at 0x5c73a8): a ballista's (item 6), a small catapult's (5) and item
/// 3's, drawn as `throw_letter`. Their lairs send them out
/// (`scene::walkers`) and do not fight themselves. Sea monsters bite
/// (`scene::creatures`). (Harpies and dragons strike as they fly: `scene::flyers`.)
pub fn creature_weapons(class_id: u32) -> Vec<u8> {
    match class_id {
        0x1f4 => vec![6],
        0x1f5 => vec![5],
        0x1f6 => vec![3],
        _ => Vec::new(),
    }
}

/// The picture a land creature's throw flies as (0x4126f0): cyclopes G,
/// minotaurs O, amazons A.
pub fn throw_letter(class: u32) -> u8 {
    match class {
        0x1f4 => 6,
        0x1f5 => 14,
        _ => 0,
    }
}

/// What a land creature's throw keeps of its blow by the creature's health
/// (0x594ab8, by tenths lost).
pub fn throw_weakness(health: f32) -> f32 {
    const LEFT: [f32; 11] = [
        1000.0, 947.0, 894.0, 837.0, 775.0, 740.0, 532.0, 548.0, 447.0, 376.0, 30.0,
    ];
    let lost = ((1.0 - health.clamp(0.0, 1.0)) * 10.0) as usize;
    LEFT[lost.min(10)] / 1000.0
}

/// Hull points of a ship type: its chart value (0x5bb4b0) x 650 (0x4ac210),
/// over 15, as the original's shots deal A x 15 (0x4ac140) where ours deal
/// A.
pub fn hull_strength(kind: u8) -> f32 {
    const CHART: [u32; 12] = [28, 90, 100, 120, 65, 70, 36, 36, 65, 75, 110, 120];
    CHART[kind as usize % 12] as f32 * 650.0 / 15.0
}

/// A ship's shares of a blow (0x4beb50, ship +0x269..+0x270): its hull
/// (the type's share, 0x597ad8), its crew (the marines' room), its armour
/// and ram (their levels), its weapons (their room: items 3-8, 0x4be780
/// with 0x597cf0), its cargo (loads of goods and, on boats and merchants,
/// the units carried as cargo, `+0x47f..+0x484`), its treasures and its
/// heroes. A blow (0x4befc0, a shot's 0x4bf170, a ram's 0x4c1ad0) falls on
/// the hull, the crew and the weapons each by its share against the sum of
/// all but the crew's; what the other shares take is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shares {
    pub hull: u32,
    pub crew: u32,
    pub weapons: u32,
    pub sum: u32,
}

impl Shares {
    pub fn of(ship: &crate::objects::Ship, weapons: &[u8]) -> Self {
        const HULL: [u32; 12] = [7, 10, 21, 18, 10, 12, 6, 6, 3, 4, 2, 4];
        let kind = ship.kind as usize % 12;
        let hull = HULL[kind];
        // Boats and merchants carry their marines as cargo (`+0x4e5`).
        let marines = ship.crew.count();
        let (crew, as_cargo) = if kind <= 1 {
            (0, marines)
        } else {
            (marines, 0)
        };
        let weapons: u32 = weapons
            .iter()
            .filter(|&&w| (3..9).contains(&w))
            .map(|&w| item_size(w))
            .sum();
        let loads = (ship.cargo / 100.0).ceil().max(0.0) as u32;
        let sum = hull
            + ship.armour as u32
            + ship.ram as u32
            + weapons
            + loads
            + as_cargo
            + ship.treasures.len() as u32
            + ship.heroes.len() as u32;
        Self {
            hull,
            crew,
            weapons,
            sum,
        }
    }

    /// The part of `blow` falling on the hull.
    pub fn hull_part(&self, blow: f32) -> f32 {
        blow * self.hull as f32 / self.sum.max(1) as f32
    }

    pub fn crew_part(&self, blow: f32) -> f32 {
        blow * self.crew as f32 / self.sum.max(1) as f32
    }

    pub fn weapons_part(&self, blow: f32) -> f32 {
        blow * self.weapons as f32 / self.sum.max(1) as f32
    }
}

/// A unit's full strength by item (0x597cfc): its condition is kept in
/// percent of it (ship `+0x299`, eight u32 per item).
pub const UNIT_POINTS: [u32; 9] = [2860, 2990, 3120, 5450, 5590, 5720, 8840, 9360, 9100];

/// What a shot's blow does to the hull, in percent, by item (0x597fd8 by
/// the shot's kind): marines' shots spare it.
pub fn hull_percent(item: u8) -> u32 {
    if item < 3 {
        0
    } else {
        100
    }
}

/// The crew's part of a shot's blow (0x4ac1b0 at level 0): by the B over
/// the A of the shot's row of 0x597d2c, as is for a row without A. The
/// unique ships' shots have rows of their own: Greek fire row 12 (650,
/// 300), quick lime row 13 (no A).
pub fn crew_scale(item: u8) -> f32 {
    match item {
        GREEK_FIRE => 300.0 / 650.0,
        QUICKLIME => 1.0,
        _ => {
            let a = hull_damage_at(item, 0);
            if a == 0.0 {
                1.0
            } else {
                crew_damage_at(item, 0) / a
            }
        }
    }
}

/// Takes `blow` (in the units' points) off the units of items `items`
/// (0x4be410 marines 0-2, 0x4be110 weapons 3-8), whose conditions are in
/// `units` by item: the units are laid out at random (each at a random
/// place, the next free one if taken), one is picked at random weighted by
/// its points, and the blow comes off it; a unit it breaks takes twice its
/// points less the blow (as the original reckons) and the rest spills over
/// to the next places in turn. The last unit standing is never broken: its
/// condition is left as it was. A broken unit's condition is 0, or 10 for a
/// boat's or merchant's weapon (`small`). Returns whether any unit is at 0.
pub fn unit_blow(
    units: &mut [Vec<u8>; 9],
    items: std::ops::Range<usize>,
    blow: u32,
    small: bool,
    rng: &mut Rng,
) -> bool {
    let n: usize = items.clone().map(|k| units[k].len()).sum();
    if n == 0 {
        return false;
    }
    // (item, index, points, touched)
    let mut places: Vec<Option<(usize, usize, i64, bool)>> = vec![None; n];
    let mut total: i64 = 0;
    for k in items {
        for (j, &c) in units[k].iter().enumerate() {
            let pts = c as i64 * UNIT_POINTS[k] as i64 / 100;
            total += pts;
            let mut p = rng.next() as usize % n;
            for _ in 0..n {
                if places[p].is_none() {
                    break;
                }
                p = (p + 1) % n;
            }
            places[p] = Some((k, j, pts, false));
        }
    }
    if total == 0 {
        return false;
    }
    let pts = |places: &Vec<Option<(usize, usize, i64, bool)>>, i: usize| places[i].unwrap().2;
    let set =
        |places: &mut Vec<Option<(usize, usize, i64, bool)>>, i: usize, v: i64, mark: bool| {
            let p = places[i].as_mut().unwrap();
            p.2 = v;
            p.3 |= mark;
        };
    let r = (rng.next() as i64) % total;
    let mut sum = 0;
    let mut i = 0;
    while i < n {
        sum += pts(&places, i);
        if r <= sum {
            break;
        }
        i += 1;
    }
    if i < n {
        let blow = blow as i64;
        let left = pts(&places, i) - blow;
        if left >= 0 {
            set(&mut places, i, left, true);
        } else {
            let mut broken = 0;
            let mut v = pts(&places, i) + left;
            if v <= 0 {
                v = 0;
                broken = 1;
            }
            set(&mut places, i, v, false);
            if broken < n {
                set(&mut places, i, v, true);
                let mut over = -left;
                let mut count = 0;
                loop {
                    i = (i + 1) % n;
                    let left = pts(&places, i) - over;
                    if left >= 0 {
                        set(&mut places, i, left, true);
                        break;
                    }
                    let mut v = pts(&places, i) + left;
                    if v <= 0 {
                        v = 0;
                        broken += 1;
                    }
                    set(&mut places, i, v, false);
                    if broken >= n {
                        break;
                    }
                    over = -left;
                    count += 1;
                    set(&mut places, i, v, true);
                    if count >= n {
                        break;
                    }
                }
            }
        }
    }
    let mut any = false;
    for &(k, j, v, touched) in places.iter().flatten() {
        if !touched {
            continue;
        }
        let c = (v * 100 / UNIT_POINTS[k] as i64) as u8;
        units[k][j] = if c == 0 && small { 10 } else { c };
        any |= units[k][j] == 0;
    }
    any
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

/// Seconds between shots of item `item` at rate of fire level `level`:
/// 6000 / rate ticks (0x4ac0e0); the ship adds 0-29 ticks at random each
/// time (0x4c0230, `RELOAD_JITTER`).
pub fn reload_seconds(item: u8, level: u8) -> f32 {
    let rate = RATE
        .get(item as usize)
        .map_or(100, |r| r[level.min(2) as usize]);
    crate::clock::ticks(6000 / rate.max(1))
}

/// The random ticks added to each reload (0x4c0230: rand % 30).
pub const RELOAD_JITTER: u32 = 30;

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

/// What a shot is aimed at, for its chances (0x4bf7f0).
pub enum Aim {
    /// A ship of this type; borne by Zetes; a phantom.
    Ship {
        kind: u8,
        zetes: bool,
        phantom: bool,
    },
    /// Anything else.
    Other,
}

/// The percent chance that a shot with `item` from `dist` world pixels
/// away hits (0x4bf7f0): 43 + the target's ease - 33 x the distance in
/// cells / the item's aiming distance (0x598130: 9, 7, 7, 9, 9, 9, 13,
/// 11, 11, 7, 9 cells, alike at every level). The ease is 25 but for
/// ships, by type (0x597acc), 1 with Zetes aboard; a phantom is hit at
/// 43 - 23 percent at any range. Greek fire always hits.
pub fn hit_chance(item: u8, dist: f32, aim: Aim) -> u32 {
    const AIMING: [u32; 11] = [9, 7, 7, 9, 9, 9, 13, 11, 11, 7, 9];
    const EASE: [i32; 12] = [5, 8, 20, 25, 15, 15, 10, 10, 10, 15, 10, 15];
    if item == GREEK_FIRE {
        return 100;
    }
    let far = (dist * 33.0 / (AIMING[(item as usize).min(10)] as f32 * 64.0)) as i32;
    let (ease, far) = match aim {
        Aim::Ship { phantom: true, .. } => (0, 23),
        Aim::Ship { zetes: true, .. } => (1, far),
        Aim::Ship { kind, .. } => (EASE[kind as usize % 12], far),
        Aim::Other => (25, far),
    };
    (ease - far + 43).clamp(0, 100) as u32
}

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
    pub fn starting(s: &openconquest_formats::StartLevels) -> Self {
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

/// What a ship's state takes off its top speed and acceleration
/// (0x4bece0): its damage half of the hull lost, and its armour (fitted
/// kind 1: 95% and 85%, kind 2: 90% and 65%).
pub fn sailing_factors(hull: f32, max_hull: f32, armour: u8) -> (f32, f32) {
    let lost = 1.0 - (hull / max_hull.max(1.0)).clamp(0.0, 1.0);
    let damage = 1.0 - lost / 2.0;
    let (speed, accel) = match armour {
        1 => (0.95, 0.85),
        2 => (0.90, 0.65),
        _ => (1.0, 1.0),
    };
    (damage * speed, damage * accel)
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
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
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
    fn hit_chances() {
        use super::*;
        // A catapult (item 6, aiming 13 cells) at a skill ship (ease 25)
        // two cells off: 43 + 25 - 33 x 2 / 13.
        let ship = |kind| Aim::Ship {
            kind,
            zetes: false,
            phantom: false,
        };
        assert_eq!(hit_chance(6, 128.0, ship(3)), 68 - 5);
        // Farther, harder; a boat (ease 5) is harder than a skill ship.
        assert!(hit_chance(6, 640.0, ship(3)) < hit_chance(6, 128.0, ship(3)));
        assert!(hit_chance(6, 128.0, ship(0)) < hit_chance(6, 128.0, ship(3)));
        // A phantom: 20 at any range; Greek fire never misses.
        let phantom = Aim::Ship {
            kind: 3,
            zetes: false,
            phantom: true,
        };
        assert_eq!(hit_chance(6, 64.0, phantom), 20);
        assert_eq!(hit_chance(GREEK_FIRE, 640.0, Aim::Other), 100);
    }

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

    #[test]
    fn ram_angles() {
        assert_eq!(ram_angle(0, 2), 0, "east into a northbound ship: side on");
        assert_eq!(ram_angle(0, 1), 1);
        assert_eq!(ram_angle(0, 4), 2, "head on");
        assert_eq!(ram_angle(3, 3), 2, "astern");
    }

    #[test]
    fn rams_favour_rammers_speed_and_side_hits() {
        let ship = |kind, heading, speed| Rammer {
            kind,
            ram: 0,
            heading,
            speed,
        };
        // Head on between equals, both under way: alike.
        let (to_t, to_a) = ram_blows(ship(2, 0, 30.0), ship(2, 4, 30.0), 2);
        assert!((to_t - to_a).abs() < 1e-3 && to_t > 0.0);
        // Side on hits harder than head on; a rammer harder than a royal.
        let side = ram_blows(ship(2, 0, 30.0), ship(2, 2, 0.0), 0).0;
        let head = ram_blows(ship(2, 0, 30.0), ship(2, 4, 0.0), 2).0;
        assert!(side > head);
        // A rammer's power (140 against 100) makes the exchange its own.
        let (dealt, taken) = ram_blows(ship(5, 0, 30.0), ship(2, 2, 0.0), 0);
        let (dealt2, taken2) = ram_blows(ship(2, 0, 30.0), ship(2, 2, 0.0), 0);
        assert!(dealt / taken > dealt2 / taken2);
        // Faster, harder; standing still, nothing.
        assert!(ram_blows(ship(2, 0, 60.0), ship(2, 2, 0.0), 0).0 > side);
        assert_eq!(ram_blows(ship(2, 0, 0.0), ship(2, 2, 0.0), 0).0, 0.0);
    }

    /// A fort's units take its blow's share (0x4184f0): load 10 against a
    /// big fort's 35; a blow a unit holds comes off it, one larger takes
    /// units away until it is spent.
    #[test]
    fn forts_units_take_blows() {
        use super::*;
        assert_eq!(fort_share(0x36, &[0, 1, 4, 5, 6, 7, 8], 4500), 1000);
        assert_eq!(fort_share(0x34, &[], 4500), 0);
        assert_eq!(fort_share(0x40, &[4, 4], 820), 20);
        assert_eq!(fort_room(0x35), 7);
        let points = |p: &[Vec<u8>; 9]| -> u32 {
            p.iter()
                .enumerate()
                .flat_map(|(k, u)| u.iter().map(move |&c| c as u32 * UNIT_POINTS[k] / 100))
                .sum()
        };
        let mut rng = Rng(7);
        let mut places: [Vec<u8>; 9] = Default::default();
        places[3] = vec![100];
        places[6] = vec![100];
        let before = points(&places);
        assert!(!fort_unit_blow(&mut places, 3..9, 1000, &mut rng));
        let lost = before - points(&places);
        assert!((1000..1100).contains(&lost), "{lost}");
        // Marines are not touched by the weapons' blow.
        places[0] = vec![100];
        let before = places[0].clone();
        assert!(fort_unit_blow(&mut places, 3..9, 100_000, &mut rng));
        assert!(places[3].is_empty() && places[6].is_empty());
        assert_eq!(places[0], before);
        // Nothing to take it: nothing happens.
        assert!(!fort_unit_blow(&mut places, 3..9, 500, &mut rng));
    }

    /// Saves from before forts kept their units' conditions load with
    /// whole units.
    #[test]
    fn old_saves_have_whole_fort_units() {
        let json = r#"{"hull":1.0,"max_hull":2.0,"weapons":[4,4],"reload":0.0,
            "protect_weapons":0.0,"protect_magic":0.0}"#;
        let c: super::Combatant = serde_json::from_str(json).unwrap();
        assert!(c.conditions.is_empty());
    }

    #[test]
    fn blows_wear_units_down() {
        // A light blow wears one unit: 1000 of a swordsman's 3120 points
        // leave 67% (or 65% of a bowman's 2860).
        let mut units: [Vec<u8>; 9] = Default::default();
        units[0] = vec![100];
        units[2] = vec![100, 100];
        let mut rng = Rng(7);
        assert!(!unit_blow(&mut units, 0..3, 1000, false, &mut rng));
        let mut worn: Vec<u8> = units.iter().flatten().copied().collect();
        worn.sort();
        assert!(worn == [65, 100, 100] || worn == [67, 100, 100], "{worn:?}");
        // A unit broken takes twice its points less the blow, and the rest
        // spills over to the next: a lone unit is never broken.
        let mut units: [Vec<u8>; 9] = Default::default();
        units[6] = vec![50];
        assert!(!unit_blow(&mut units, 3..9, 100_000, false, &mut rng));
        assert_eq!(units[6], [50]);
        // Two units under a crushing blow: the first broken, the other
        // spared as the last standing.
        let mut units: [Vec<u8>; 9] = Default::default();
        units[3] = vec![100, 100];
        assert!(unit_blow(&mut units, 3..9, 100_000, false, &mut rng));
        let mut left = units[3].clone();
        left.sort();
        assert_eq!(left, [0, 100]);
        // A boat's or merchant's weapon is left at 10 instead.
        let mut units: [Vec<u8>; 9] = Default::default();
        units[4] = vec![100, 100];
        assert!(!unit_blow(&mut units, 3..9, 100_000, true, &mut rng));
        let mut left = units[4].clone();
        left.sort();
        assert_eq!(left, [10, 100]);
        // A worn unit (2725 points) picked by a 4000 blow keeps twice its
        // points less the blow, 1450 (26%), and the other (5450) takes the
        // 1275 over (76%); picked second, it is left alone and the whole
        // one takes the blow (26%).
        for seed in 0..20 {
            let mut u: [Vec<u8>; 9] = Default::default();
            u[3] = vec![50, 100];
            unit_blow(&mut u, 3..9, 4000, false, &mut Rng(seed));
            let mut left = u[3].clone();
            left.sort();
            assert!(left == [26, 76] || left == [26, 50], "{left:?}");
        }
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
