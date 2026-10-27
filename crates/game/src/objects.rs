//! Mission objects: turning object records into drawable, animated visuals.
//!
//! Sprite names, record fields, placement and animation timing come from
//! ANCIENT.EXE (see `docs/FORMATS.md`, "Mission objects"):
//!
//! * Buildings (0x4288c0): `prefix + nation [+ class] + state [+ facing] + damage`,
//!   where state 0 is under construction, 1 standing and 2 destroyed, and
//!   damage 3 is intact.
//! * Ships (0x4e0120): `SHIP_<type><nation><nation><player><mode>`; the
//!   owner's nation and player slot select the sail colours.
//! * Nature objects (reefs 0x4300a0, whirlpools 0x447930, castles 0x4101d0)
//!   are sequences whose top-left corner sits at the cell's top-left corner
//!   (minus a per-class offset), with a frame period of
//!   `cycle_ticks[state] / frame_count` game ticks.

use std::collections::HashMap;
use std::rc::Rc;

use anyhow::Result;
use openac_formats::anim::EMPTY_SPRITE;
use openac_formats::map::TILE_SIZE;
use openac_formats::mission::field;
use openac_formats::{
    object_records, Archive, Composite, ObjectClass, ObjectRecord, ResourceKind, Sequence,
};

use crate::assets::{GameData, Image, ImageCache, Source};

use crate::clock::TICK_MS;
/// Frame period for objects whose timing has not been recovered yet.
const DEFAULT_FRAME_MS: u64 = 100;

/// Decoded-resource caches shared across missions.
pub struct Caches {
    pub tiles: ImageCache,
    pub sprites: ImageCache,
    composites: HashMap<String, Option<Rc<Composite>>>,
    sequences: HashMap<String, Rc<Vec<String>>>,
}

impl Default for Caches {
    fn default() -> Self {
        Self {
            tiles: ImageCache::new(Source::MapLib),
            sprites: ImageCache::new(Source::Objects),
            composites: HashMap::new(),
            sequences: HashMap::new(),
        }
    }
}

impl Caches {
    fn composite(&mut self, data: &GameData, name: &str) -> Result<Option<Rc<Composite>>> {
        if let Some(c) = self.composites.get(name) {
            return Ok(c.clone());
        }
        let c = match data.objects.find(name, ResourceKind::Composite) {
            Some(e) => Some(Rc::new(Composite::parse(&data.objects.read_entry(e)?, e)?)),
            None => None,
        };
        self.composites.insert(name.to_string(), c.clone());
        Ok(c)
    }

    fn sequence(&mut self, data: &GameData, name: &str) -> Result<Rc<Vec<String>>> {
        if let Some(s) = self.sequences.get(name) {
            return Ok(s.clone());
        }
        let frames = match data.objects.find(name, ResourceKind::Sequence) {
            Some(e) => Sequence::parse(&data.objects.read_entry(e)?, e)?.frame_names(&data.objects),
            None => Vec::new(),
        };
        let frames = Rc::new(frames);
        self.sequences.insert(name.to_string(), frames.clone());
        Ok(frames)
    }
}

/// One sprite of an animation frame, offset from the object's position.
#[derive(Clone)]
pub struct Layer {
    pub image: Rc<Image>,
    pub dx: i32,
    pub dy: i32,
}

/// A placed, drawable object.
pub struct MapObject {
    /// Object id from the mission (`OBJ_xxxxxxxx`).
    pub id: u32,
    pub class: ObjectClass,
    /// The record the object was created from, kept so it can be rebuilt
    /// after a change (e.g. an upgrade).
    pub record: ObjectRecord,
    /// Human-readable description.
    pub label: String,
    /// World position of the object's centre, in pixels.
    pub x: f32,
    pub y: f32,
    /// Animation frames, each a stack of layers drawn in order. For ships,
    /// 8 headings x 4 frames.
    pub frames: Vec<Vec<Layer>>,
    pub frame_ms: u64,
    pub ship: Option<Ship>,
    pub deposit: Option<Deposit>,
    /// An exhausted amber deposit growing back: its deposit and the
    /// seconds it has lain empty.
    pub dormant: Option<(Deposit, f32)>,
    /// Hull and weapons, for objects that can fight or be attacked.
    pub combat: Option<crate::combat::Combatant>,
    /// Owning player as `(nation, colour slot)`, for player-owned objects.
    pub owner: Option<(u32, u32)>,
    /// Picture for the object panel (`INF_SHIPnn` / `INF_OBJnn` in CONTROLG)
    /// and its offset in the panel.
    pub picture: Option<(String, (u32, u32))>,
    /// A treasure lying on the map.
    pub treasure: Option<Treasure>,
    /// Shows the frame this far (0..1) through the animation instead of
    /// playing it: a building going up.
    pub progress: Option<f32>,
    /// A building's banner (`FLAGSPR_<nation><slot>`, its first frame) and
    /// where its top left lies from the building's.
    pub banner: Option<(Rc<Image>, i32, i32)>,
    /// The map cell a building stands on (its record's).
    pub cell: Option<(i32, i32)>,
}

/// A treasure (`TreasClassTy` record: kind in field 4, parameters in
/// fields 7 and 8; see `treasure_name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Treasure {
    pub kind: u8,
    /// Field 7: a spell index, resource class or packed item and count.
    pub param: u32,
    /// Field 8: an amount.
    pub amount: u32,
}

/// The nine heroes, by house number (`HeroHouseClassTy` field 22) and
/// portrait (`COBJ_HEROYnn`).
pub const HERO_NAMES: [&str; 9] = [
    "Jason", "Theseus", "Orpheus", "Zetes", "Hercules", "Argus", "Atalanta", "Odysseus", "Perseus",
];

/// What a treasure is, from the original's descriptions (string resources)
/// in the order the treasure routine 0x4b9740 groups the kinds: 0-2 amber
/// and 3-5 fish (0x4a8de0, 0x4a8c30), 6-7 goods to deliver, 8 a spell
/// cast where it is taken (0x4b9bf0), 9 a spell kept for later, 10-17
/// lasting gifts, 18-26 the heroes' magic items, 27 a scroll.
pub fn treasure_name(kind: u8) -> String {
    match kind {
        0..=2 => "Barrel of amber".into(),
        3..=5 => "Barrel of fish".into(),
        6 => "Goods for your settlement".into(),
        7 => "Weapons for your settlement".into(),
        8 => "Gift of Poseidon".into(),
        9 => "Disposable magic".into(),
        10 => "Torch of Ares (protection from fire)".into(),
        11 => "Armour of Ares".into(),
        12 => "Feather of Boreas (speed and ram)".into(),
        13 => "Magic wood (no boarding)".into(),
        14 => "Mirror shield of Athena (protection from Gorgon)".into(),
        15 => "Cup of Asclepius (heals crews)".into(),
        16 => "Golden Fleece".into(),
        17 => "Gorgon's head".into(),
        18..=26 => {
            const ITEMS: [&str; 9] = [
                "Magic oar",
                "Ariadne's clew",
                "Magic harp",
                "Magic horn",
                "Magic sword",
                "Hammer",
                "Magic bow",
                "Golden statuette",
                "Mirror shield",
            ];
            let h = (kind - 18) as usize;
            format!("{} for {}", ITEMS[h], HERO_NAMES[h])
        }
        _ => "Scroll of knowledge".into(),
    }
}

/// The treasures an object's record says it holds (its inventory: a count,
/// then entries of five dwords like a treasure record's fields 4-8: kind,
/// holder -1, 0, parameter, amount). Lairs, nests, barbarian structures,
/// fortresses, towns and sea monsters keep theirs at different fields.
pub fn inventory(rec: &ObjectRecord) -> Vec<Treasure> {
    let count_at = match rec.class_id {
        0x46..=0x48 => 10,
        0x34..=0x36 | 0x3f..=0x41 => 11,
        0x44 | 0x45 => 12,
        0x1fb => 17,
        0x32 | 0x33 => 60,
        _ => return Vec::new(),
    };
    let f = |i: usize| rec.field(i);
    let Some(n) = f(count_at).filter(|&n| n <= 8) else {
        return Vec::new();
    };
    (0..n as usize)
        .map_while(|k| {
            let at = count_at + 1 + 5 * k;
            let kind = f(at).filter(|&kind| kind <= 27)?;
            (f(at + 1)? == u32::MAX).then_some(Treasure {
                kind: kind as u8,
                param: f(at + 3)?,
                amount: f(at + 4)?,
            })
        })
        .collect()
}

/// The hero a treasure kind calls, if it is a hero's item.
pub fn treasure_hero(kind: u8) -> Option<u8> {
    (18..=26).contains(&kind).then(|| kind - 18)
}

/// Treasures and heroes a ship type can carry (the manual's chart:
/// "displacement for treasures" and "for heroes").
pub fn treasure_room(kind: u8) -> usize {
    [1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 0, 0][kind as usize % 12]
}

pub fn hero_room(kind: u8) -> usize {
    [0, 0, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0][kind as usize % 12]
}

/// Movement state of a ship.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ship {
    /// Ship type 0-11 (see `SHIP_NAMES`).
    pub kind: u8,
    /// 0 = east, counting counter-clockwise in 45 degree steps.
    pub heading: usize,
    /// Cruising speed in world pixels per second.
    pub speed: f32,
    /// Remaining waypoints (cell centres), nearest first.
    pub path: std::collections::VecDeque<(f32, f32)>,
    /// The hull's rotation (of 24, 3 a heading) while it turns; `None`
    /// when it lies on `heading`.
    #[serde(default)]
    pub rotation: Option<u8>,
    #[serde(default)]
    pub turn_clock: f32,
    /// Seconds it has waited for another ship to clear the cell ahead.
    #[serde(default)]
    pub blocked: f32,
    /// How fast it is going now, in world pixels a second: it gathers way
    /// up to its speed and loses it to stop where it is bound or to turn.
    #[serde(default)]
    pub velocity: f32,
    /// The heading it turns to while closing in on a shore or pier, to lie
    /// side-on (0x4afe50).
    #[serde(default)]
    pub face: Option<usize>,
    /// Goods on board.
    pub cargo: f32,
    pub cargo_kind: Option<Resource>,
    /// A standing order the ship carries out on its own.
    pub task: Option<Task>,
    /// Marines on board.
    pub crew: crate::combat::Crew,
    /// Armour and ram fitted at a smithy, levels 0-2.
    #[serde(default)]
    pub armour: u8,
    #[serde(default)]
    pub ram: u8,
    /// What the ship does on its own (warships).
    #[serde(default)]
    pub behaviour: Behaviour,
    /// A zone a merchant gathers in on its own: world rectangle (x0, y0,
    /// x1, y1) and what it gathers.
    #[serde(default)]
    pub zone: Option<((f32, f32, f32, f32), Resource)>,
    /// Treasures aboard.
    #[serde(default)]
    pub treasures: Vec<Treasure>,
    /// Heroes aboard (see `HERO_NAMES`).
    #[serde(default)]
    pub heroes: Vec<u8>,
    /// This is the Argo (a royal ship built with "Build Argo", 0x7d).
    #[serde(default)]
    pub argo: bool,
}

/// A warship's behaviour switches (ship byte +0x684 bits 0-2 and +0x633;
/// the manual's "Ship AI options").
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Behaviour {
    /// Fire at enemies in range.
    pub guard: bool,
    /// Ram enemy ships when attacking them.
    pub ram: bool,
    /// Board enemy ships when attacking them.
    pub board: bool,
    /// Go after whoever fires at the ship (needs `guard`).
    pub aggressor: bool,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            guard: true,
            ram: false,
            board: false,
            aggressor: false,
        }
    }
}

impl Ship {
    /// Whether a treasure of `kind` is aboard.
    pub fn has_treasure(&self, kind: u8) -> bool {
        self.treasures.iter().any(|t| t.kind == kind)
    }

    /// Whether hero `hero` (see `HERO_NAMES`) is aboard.
    pub fn has_hero(&self, hero: u8) -> bool {
        self.heroes.contains(&hero)
    }
}

/// Heroes by number.
pub const JASON: u8 = 0;
pub const THESEUS: u8 = 1;
pub const ORPHEUS: u8 = 2;
pub const ZETES: u8 = 3;
pub const HERCULES: u8 = 4;
pub const ARGUS: u8 = 5;
pub const ATALANTA: u8 = 6;

/// Treasure kinds with lasting effects aboard.
pub const FIRE_TORCH: u8 = 10;
pub const ARMOUR_OF_ARES: u8 = 11;
pub const FEATHER: u8 = 12;
pub const MAGIC_WOOD: u8 = 13;

/// How much the gifts and heroes help. Placeholders: the original's
/// figures have not been found.
pub const FEATHER_SPEED: f32 = 1.25;
pub const THESEUS_DAMAGE: f32 = 1.25;
pub const THESEUS_RANGE: f32 = 64.0;
pub const ATALANTA_RATE: f32 = 1.25;
pub const ATALANTA_HIT: u32 = 15;
pub const ARGUS_MONSTER_DAMAGE: f32 = 0.5;
/// The Armour of Ares takes this share of hull damage when its record
/// gives none (miss001's gives 40).
pub const ARMOUR_OF_ARES_PERCENT: u32 = 40;

/// Goods that can be gathered and carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Resource {
    Amber,
    Fish,
    /// A leviathan, caught whole.
    Leviathan,
}

/// What a leviathan brought home is worth: 5000 amber and 2500 fish (the
/// manual).
pub const LEVIATHAN_WORTH: (i64, i64) = (5000, 2500);
/// Seconds to catch a leviathan. Placeholder.
pub const HUNT_SECONDS: f32 = 30.0;

/// A deposit of a resource on the map.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Deposit {
    pub resource: Resource,
    /// Units left, or `None` for an inexhaustible deposit.
    pub remaining: Option<f32>,
    /// Times an exhausted amber deposit grows back (record field 9), and
    /// to how much (0x48a430).
    #[serde(default)]
    pub regrows: u32,
    #[serde(default)]
    pub refill: f32,
}

/// Seconds an exhausted amber deposit lies empty before it grows back: 100
/// ticks of its state machine (0x48a430, state 3).
pub const REGROW_SECONDS: f32 = 100.0 * crate::clock::TICK;

/// A ship's standing order.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Task {
    /// Shuttle between a deposit and the nearest port.
    Gather { deposit: u32, phase: GatherPhase },
    /// Chase and fire at an enemy ship or building.
    Attack { target: u32 },
    /// Sail to a land cell and put up a building there: a settlement or a
    /// fort (the ships' building page, 0x4e6980 page 2).
    Found {
        cx: i32,
        cy: i32,
        #[serde(default = "settlement_class")]
        class: u32,
    },
    /// Run into an enemy ship.
    Ram { target: u32 },
    /// Come alongside an enemy ship and fight for it: once grappled, the
    /// fight is decided (`won`) and shown, and `round` counts down to its
    /// end.
    Board {
        target: u32,
        round: f32,
        #[serde(default)]
        won: Option<bool>,
    },
    /// Sail to an own dock and be repaired there.
    Repair { dock: u32 },
    /// Sail out to scout, then back to `home`.
    Scout { home: (f32, f32) },
    /// Sail to `to`, then back to `from`, and so on.
    Patrol { from: (f32, f32), to: (f32, f32) },
    /// Hold the world rectangle `rect` (x0, y0, x1, y1): fight enemy ships
    /// that come into it (a guard zone), or with `attack`, every enemy
    /// ship and building in it until none is left (an attack zone).
    Zone {
        rect: (f32, f32, f32, f32),
        attack: bool,
    },
    /// Keep close to own ship `ward` and guard it.
    Convoy { ward: u32 },
    /// Sail to a treasure or a hero's house and take it.
    Take { target: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GatherPhase {
    ToDeposit,
    Collecting,
    ToPort(u32),
    /// Porters carrying the goods into the building.
    Unloading(u32),
}

/// Cargo capacity of a merchant ship type, in units of goods: the manual's
/// cargo displacement (boat 4, merchant 10) times 100.
pub fn cargo_capacity(kind: u8) -> f32 {
    match kind {
        0 => 400.0,
        1 => 1000.0,
        _ => 0.0,
    }
}

pub const SHIP_HEADINGS: usize = 8;
const SHIP_FRAMES_PER_HEADING: usize = 4;
/// First of a ship's 24 turning frames and of its 32 lying-still frames.
const SHIP_TURN_FRAMES: usize = 32;
const SHIP_IDLE_FRAMES: usize = 56;
/// Milliseconds a frame of a ship's animation, under way, turning or
/// rocking at rest: its picture's frame delay is 2 (0x4e0160 and the ship
/// loader), a frame once more than 2 ticks have passed (0x4a3320).
pub const SHIP_FRAME_MS: u64 = 3 * TICK_MS;
const SHIP_ROCK_MS: u64 = SHIP_FRAME_MS;

impl MapObject {
    pub fn is_armed(&self) -> bool {
        !self.shooters().is_empty()
    }

    /// What fires from the object: its weapons, then (on ships) a shot for
    /// each squad of bowmen and javelin men.
    pub fn shooters(&self) -> Vec<u8> {
        let Some(c) = self.combat.as_ref() else {
            return Vec::new();
        };
        let mut items: Vec<u8> = c
            .weapons
            .iter()
            .copied()
            .filter(|&w| crate::combat::fires(w))
            .collect();
        if let Some(s) = &self.ship {
            for kind in 0..2u8 {
                items.extend(std::iter::repeat_n(
                    kind,
                    s.crew.squads[kind as usize] as usize,
                ));
            }
        }
        items
    }

    pub fn frame_at(&self, time_ms: u64) -> &[Layer] {
        if self.frames.is_empty() {
            return &[];
        }
        if let (Some(p), false) = (self.progress, self.frames.is_empty()) {
            let n = self.frames.len();
            return &self.frames[((p * n as f32) as usize).min(n - 1)];
        }
        let step = (time_ms / self.frame_ms.max(1)) as usize;
        let n = self.frames.len();
        let i = match &self.ship {
            // A ship's composite: 8 headings x 4 frames under way (oars and
            // wake), the 24 hull rotations it turns through, then 8
            // headings x 4 frames lying still: at rest, rocking one way, at
            // rest, rocking the other.
            Some(ship) if n >= SHIP_IDLE_FRAMES + 32 => match ship.rotation {
                Some(r) => SHIP_TURN_FRAMES + r as usize % 24,
                None if ship.path.is_empty() => {
                    let rock = (time_ms / SHIP_ROCK_MS) as usize;
                    SHIP_IDLE_FRAMES + ship.heading * 4 + rock % 4
                }
                None => ship.heading * SHIP_FRAMES_PER_HEADING + step % SHIP_FRAMES_PER_HEADING,
            },
            Some(ship) => {
                let per = SHIP_FRAMES_PER_HEADING.min(n.max(1));
                (ship.heading * per + step % per) % n.max(1)
            }
            None => step % n.max(1),
        };
        &self.frames[i]
    }
}

/// A harpy's (0x1f8) or dragon's sprite sequence for a state (0 resting,
/// 1 waiting, 2 taking off, 3 flying, 4 turning, 5 striking, 6 landing, 7
/// dying) and heading; resting, taking off and landing have only the odd
/// headings. Dragons never rest: they circle (flying).
pub fn flyer_sequence(class: u32, state: u8, heading: usize) -> String {
    let (prefix, state) = if class == 0x1f9 {
        ("DRGSPR", if state < 2 { 3 } else { state })
    } else {
        ("HRPSPR", state)
    };
    let heading = if matches!(state, 0 | 2 | 6) {
        heading | 1
    } else {
        heading
    };
    format!("{prefix}_{state}{}", heading.min(7))
}

/// Amber deposits (Grig/amber.cpp) match the shore they lie on: the
/// base-layer tile codes of each of the 20 variants (0x5b97c8, 0x24 bytes
/// each: four codes, then two rectangles in the cell where the sparkle may
/// show, x, y, width, height).
/// A variant's tile codes and sparkle rectangles.
pub type AmberVariant = ([u8; 4], [(i32, i32, i32, i32); 2]);
pub const AMBER_VARIANTS: [AmberVariant; 20] = [
    ([2, 3, 4, 5], [(37, 28, 26, 30), (14, 41, 23, 22)]),
    ([6, 0, 0, 0], [(12, 17, 16, 46), (28, 17, 36, 20)]),
    ([7, 8, 9, 10], [(0, 27, 24, 29), (25, 40, 24, 23)]),
    ([11, 0, 0, 0], [(0, 20, 38, 17), (37, 20, 18, 43)]),
    ([12, 13, 14, 15], [(16, 0, 25, 23), (40, 11, 20, 27)]),
    ([16, 0, 0, 0], [(11, 0, 17, 36), (13, 36, 50, 19)]),
    ([17, 18, 19, 20], [(0, 16, 26, 23), (18, 2, 31, 22)]),
    ([21, 0, 0, 0], [(0, 39, 42, 17), (35, 3, 19, 45)]),
    ([30, 0, 0, 0], [(7, 3, 20, 60), (7, 3, 20, 60)]),
    ([31, 0, 0, 0], [(12, 3, 20, 60), (12, 3, 20, 60)]),
    ([32, 0, 0, 0], [(39, 3, 20, 60), (39, 3, 20, 60)]),
    ([33, 0, 0, 0], [(31, 3, 24, 60), (31, 3, 24, 60)]),
    ([34, 0, 0, 0], [(2, 30, 61, 22), (2, 30, 61, 22)]),
    ([35, 0, 0, 0], [(0, 27, 61, 15), (0, 27, 61, 15)]),
    ([36, 0, 0, 0], [(0, 37, 61, 23), (0, 37, 61, 23)]),
    ([37, 0, 0, 0], [(1, 27, 61, 21), (1, 27, 61, 21)]),
    ([38, 0, 0, 0], [(2, 36, 61, 17), (2, 36, 61, 17)]),
    ([39, 0, 0, 0], [(1, 27, 61, 21), (1, 27, 61, 21)]),
    ([40, 0, 0, 0], [(2, 35, 60, 16), (2, 35, 60, 16)]),
    ([41, 0, 0, 0], [(1, 21, 61, 23), (1, 21, 61, 23)]),
];

/// The amber variant for a cell whose base tile has code `tile` (0x489a10;
/// variant 0 when none matches).
pub fn amber_variant(tile: u8) -> usize {
    AMBER_VARIANTS
        .iter()
        .position(|(codes, _)| tile != 0 && codes.contains(&tile))
        .unwrap_or(0)
}

/// An amber deposit's frames (0x489cd0): its variant's picture for how
/// much is left (`level` 3 full to 1, frame (variant + 1) x 3 - level of
/// `AMBER_`), still.
pub fn amber_frames(
    data: &GameData,
    caches: &mut Caches,
    variant: usize,
    level: usize,
) -> Result<Vec<Vec<Layer>>> {
    let all = sequence_frames(data, caches, "AMBER_")?;
    let v = variant.min(AMBER_VARIANTS.len() - 1);
    let i = ((v + 1) * 3).saturating_sub(level.clamp(1, 3));
    // The sparkle comes in bursts of its own (`scene::sparkle`).
    Ok(all.get(i).or(all.first()).cloned().into_iter().collect())
}

/// A flyer's frames: each its shadow on the water (sequence + `S`), then
/// the creature in the air (drawn high in the frame).
pub fn flyer_frames(data: &GameData, caches: &mut Caches, name: &str) -> Result<Vec<Vec<Layer>>> {
    let mut frames = sequence_frames(data, caches, name)?;
    if let Ok(shadows) = sequence_frames(data, caches, &format!("{name}S")) {
        for (frame, shadow) in frames.iter_mut().zip(shadows) {
            // The shadow sprites are drawn in one colour: darken instead.
            let shadow = shadow.into_iter().map(|l| {
                let img = &l.image;
                let pixels = img
                    .pixels
                    .iter()
                    .map(|p| p.map(|_| crate::assets::SHADOW))
                    .collect();
                Layer {
                    image: Rc::new(Image::new(img.width, img.height, pixels)),
                    dx: l.dx,
                    dy: l.dy,
                }
            });
            let body = std::mem::take(frame);
            *frame = shadow.chain(body).collect();
        }
    }
    Ok(frames)
}

/// Result of loading a mission's objects.
pub struct Objects {
    /// Drawable objects, sorted back to front.
    pub drawn: Vec<MapObject>,
    /// Records that could not be drawn yet, by class id.
    pub unresolved: HashMap<u32, usize>,
}

pub fn load(data: &GameData, caches: &mut Caches, mission: &Archive) -> Result<Objects> {
    let mut drawn = Vec::new();
    let mut unresolved = HashMap::<u32, usize>::new();
    for rec in object_records(mission)? {
        match resolve(data, caches, &rec)? {
            Some(obj) => drawn.push(obj),
            None => *unresolved.entry(rec.class_id).or_default() += 1,
        }
    }
    // Painter's order: objects further down the map are in front.
    drawn.sort_by(|a, b| a.y.total_cmp(&b.y));
    Ok(Objects { drawn, unresolved })
}

/// Where an object's frames are anchored on the map.
enum Anchor {
    /// Centred on the centre of a cell.
    CellCentre(u32, u32),
    /// Frame top-left at the cell's top-left corner minus an offset.
    CellCorner(u32, u32, i32, i32),
}

/// What to draw and how fast it animates.
struct Visual {
    frames: Vec<Vec<Layer>>,
    frame_ms: u64,
}

/// Builds the drawable object for a mission record (also used to create
/// objects during play, from synthetic records).
pub fn resolve(
    data: &GameData,
    caches: &mut Caches,
    rec: &ObjectRecord,
) -> Result<Option<MapObject>> {
    let f = |i| rec.field(i).unwrap_or(0);
    let class = rec.class();
    // Frame period from a per-state animation cycle length in game ticks.
    let cycle =
        |cycle_ticks: u64, frames: usize| (cycle_ticks / frames.max(1) as u64).max(1) * TICK_MS;

    let (anchor, visual) = match class {
        ObjectClass::Building(cls) | ObjectClass::Barbarian(cls) => {
            let condition = rec.fields.get(7).map_or(100, |&c| c.min(100)) as f32;
            let state = BuildingState::for_condition(condition);
            let Some(name) = building_name(cls, f(field::NATION), f(10), state) else {
                return Ok(None);
            };
            let frames = composite_frames(data, caches, &name, 0..usize::MAX)?;
            let frame_ms = building_frame_ms(frames.len());
            // The picture's top left is the cell's corner less a per-class
            // offset (LandObjClassTy 0x422e30; the constructors' +0xc4,
            // +0xc8): ports and docks (0, 7), forts (4, 12), settlements and
            // towns none (measured on the original), the rest (4, 4).
            let anchor = match cls {
                0x39..=0x3b => CellCorner(f(5), f(6), 0, 7),
                0x34..=0x36 => CellCorner(f(5), f(6), 4, 12),
                0x32 | 0x33 => CellCorner(f(5), f(6), 0, 0),
                0x37 | 0x38 | 0x3c | 0x3d | 0x3e => CellCorner(f(5), f(6), 4, 4),
                _ => CellCentre(f(5), f(6)),
            };
            (anchor, Visual { frames, frame_ms })
        }
        ObjectClass::Ship => {
            let letter = (b'A' + f(7).min(25) as u8) as char;
            let (nation, player) = (f(5), f(6));
            let name = format!("SHIP_{letter}{nation}{nation}{player}0");
            // 8 headings x 4 frames under sail, then (frames 88-151) 8
            // headings x 8 frames furling the sail. The layers are the wake
            // and pairs (hull, ram, armour, sail); the original shows the
            // first of each pair, and the ram only on rammers (5) and fire
            // rammers (10) (fitted to the original's first frame of miss000).
            let rammer = matches!(f(7), 5 | 10);
            let frames = composite_frames_with(data, caches, &name, 0..usize::MAX, |k, s| {
                (k == 0 || k % 2 == 1) && (rammer || !s.starts_with("TARAN_"))
            })?;
            (
                CellCentre(f(9), f(10)),
                Visual {
                    frames,
                    frame_ms: SHIP_FRAME_MS,
                },
            )
        }
        ObjectClass::Castle(cls) => {
            // CastleClassTy init (0x410ca0): prefix, cycle table and offsets.
            let (prefix, dx, dy) = if cls == 0x42 {
                ("CSTLSEA", 6, 12)
            } else {
                ("CSTLWAR", 4, 13)
            };
            const LIVE: u64 = 1;
            let frames = sequence_frames(data, caches, &format!("{prefix}SPR_{LIVE}3"))?;
            let frame_ms = cycle(CASTLE_CYCLE[LIVE as usize], frames.len());
            (CellCorner(f(5), f(6), dx, dy), Visual { frames, frame_ms })
        }
        // Treasures lying on the map (field 5 is -1; otherwise the id of
        // the object holding them) sparkle at the cell in fields 9 and 10.
        ObjectClass::Treasure if f(5) == u32::MAX => {
            // Afloat it bobs (state 2, 0x406a10): frames 8-22 (30-59 for
            // the big 16 and 27), a frame every 3 ticks (2 for those;
            // 0x4068e0). Frames before rise into view, after sink.
            let kind = f(4).min(27);
            let all = sequence_frames(data, caches, &format!("TREAS{kind:02}"))?;
            let big = kind == 16 || kind == 27;
            let (from, to, ticks) = if big { (30, 59, 2) } else { (8, 22, 3) };
            let frames = if all.len() > from {
                all[from..=to.min(all.len() - 1)].to_vec()
            } else {
                all
            };
            (
                CellCentre(f(9), f(10)),
                Visual {
                    frames,
                    frame_ms: ticks * TICK_MS,
                },
            )
        }
        ObjectClass::HeroHouse => {
            let name = format!("HHOUSE{}SPR_013", f(22).min(8));
            let frames = composite_frames(data, caches, &name, 0..usize::MAX)?;
            // A land building (0x422e30): its cycle 50, 75, 75, 57 ticks
            // (0x595e80, constructor 0x420fd0), standing 75.
            let frame_ms = building_frame_ms(frames.len());
            (CellCentre(f(5), f(6)), Visual { frames, frame_ms })
        }
        ObjectClass::HarpyNest => {
            // 0x41d010: dragon (0x44) or harpy (0x45) nest; the frame shows
            // damage (0 intact, 1 below 66 %, 2 below 33 %).
            let name = if rec.class_id == 0x44 {
                "DNIDSPR"
            } else {
                "HNIDSPR"
            };
            let mut frames = sequence_frames(data, caches, name)?;
            frames.truncate(1);
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::CreatureSpawn => {
            // Lairs of the cyclopes (0x46), minotaurs (0x47) and amazons
            // (0x48), named like buildings: nation 4, standing, intact.
            let prefix = match rec.class_id {
                0x46 => "CLPF",
                0x47 => "MNTF",
                _ => "AMZF",
            };
            let frames =
                composite_frames(data, caches, &format!("{prefix}SPR_413"), 0..usize::MAX)?;
            // Land buildings too (0x422e30): standing, 75 ticks a round.
            let frame_ms = building_frame_ms(frames.len());
            (CellCentre(f(4), f(5)), Visual { frames, frame_ms })
        }
        ObjectClass::Monster(0x1fb) => {
            // Sea monster, `UMON<type><state><heading>` (0x490040): states 0
            // surfacing, 1 swimming, 2 diving, 3 striking, 4 dying.
            let frames = sequence_frames(data, caches, &format!("UMON{}10", f(4).min(3)))?;
            (
                CellCentre(f(7), f(8)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Monster(cls @ 0x1f4..=0x1f6) => {
            // A cyclops, minotaur or amazon out of its lair (CyclopClassTy),
            // appearing.
            let name = crate::scene::walker_sequence(cls, 0, 0);
            let frames = sequence_frames(data, caches, &name)?;
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: crate::scene::walker_frame_ms(0),
                },
            )
        }
        ObjectClass::Monster(cls @ (0x1f8 | 0x1f9)) => {
            // A harpy or dragon (HarpyClassTy, 0x4207b0) out of its nest:
            // `HRPSPR_`/`DRGSPR_` + state + heading; resting at first.
            let frames = flyer_frames(data, caches, &flyer_sequence(cls, 0, 1))?;
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Monster(cls @ (0x1fa | 0x1fd)) => {
            // Medusa (`GORG%1d`) and siren, at the centre of their area.
            let name = if cls == 0x1fa { "GORG1" } else { "SIRENA" };
            let frames = sequence_frames(data, caches, name)?;
            (
                CellCentre(f(5), f(6)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Amber => {
            let frames = sequence_frames(data, caches, "AMBER_")?;
            (
                CellCorner(f(6), f(7), 0, 0),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::FishZone | ObjectClass::WhaleZone => {
            // Zones are areas the creatures roam; show one in the middle.
            let (x, y) = (f(6) + f(8) / 2, f(7) + f(9) / 2);
            let frames = if class == ObjectClass::FishZone {
                sequence_frames(data, caches, "FISH_12")?
            } else {
                composite_frames(data, caches, "WHALE_00", 0..usize::MAX)?
            };
            (
                CellCorner(x, y, 0, 0),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Whirlpool => {
            const LIVE: usize = 1;
            let frames = sequence_frames(data, caches, &format!("WHIRLSPR_{LIVE}"))?;
            let frame_ms = cycle(WHIRL_CYCLE[LIVE], frames.len());
            (CellCorner(f(5), f(6), 0, 0), Visual { frames, frame_ms })
        }
        ObjectClass::Reef(cls) => {
            const LIVE: usize = 1;
            let size = if cls == 0x259 { 'B' } else { 'M' };
            let kind = f(7).clamp(1, 3);
            let frames = sequence_frames(data, caches, &format!("{size}REEF{kind}SPR_{LIVE}"))?;
            let frame_ms = cycle(REEF_CYCLE[LIVE], frames.len());
            (CellCorner(f(5), f(6), 0, 0), Visual { frames, frame_ms })
        }
        ObjectClass::Lava => {
            let frames = sequence_frames(data, caches, &format!("VOLCANOLAVA{:02}", f(4)))?;
            // A frame every 3 ticks (the volcano, 0x445630).
            (
                CellCorner(f(6), f(7), 0, 0),
                Visual {
                    frames,
                    frame_ms: 3 * TICK_MS,
                },
            )
        }
        ObjectClass::Sheep => {
            let frames = sheep_frames(data, caches, f(7))?;
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS * 2,
                },
            )
        }
        _ => return Ok(None),
    };
    let banner = match class {
        ObjectClass::Building(cls) if f(field::NATION) < 4 => {
            let (nation, slot) = (f(field::NATION), f(field::PLAYER));
            let at = banner_offset(cls, nation);
            match (
                at,
                caches.composite(data, &format!("FLAGSPR_{nation}{slot}"))?,
            ) {
                (Some((dx, dy)), Some(comp)) => comp
                    .frame(0)
                    .first()
                    .map(|c| c.sprite.clone())
                    .and_then(|s| caches.sprites.get(data, &s).ok().flatten())
                    .map(|img| (img, dx, dy)),
                _ => None,
            }
        }
        _ => None,
    };
    let Visual { frames, frame_ms } = visual;
    let Some(first) = frames.iter().flatten().next() else {
        return Ok(None);
    };
    let t = TILE_SIZE as f32;
    let cell = match (class, &anchor) {
        (ObjectClass::Building(_), CellCorner(cx, cy, ..) | CellCentre(cx, cy)) => {
            Some((*cx as i32, *cy as i32))
        }
        _ => None,
    };
    let (x, y) = match anchor {
        CellCentre(cx, cy) => (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0),
        CellCorner(cx, cy, dx, dy) => (
            cx as f32 * t - dx as f32 + first.image.width as f32 / 2.0,
            cy as f32 * t - dy as f32 + first.image.height as f32 / 2.0,
        ),
    };
    let ship = (class == ObjectClass::Ship).then(|| Ship {
        kind: f(7).min(11) as u8,
        heading: (f(11) as usize) % SHIP_HEADINGS,
        speed: SHIP_SPEED.get(f(7) as usize).copied().unwrap_or(100) as f32 * SPEED_SCALE,
        path: Default::default(),
        rotation: None,
        turn_clock: 0.0,
        blocked: 0.0,
        velocity: 0.0,
        face: None,
        cargo: 0.0,
        cargo_kind: None,
        task: None,
        crew: crate::combat::Crew::from_record(&rec.fields, f(7).min(11) as u8),
        // Armour level in field 15, ram level in field 14 less 3 (the
        // loader 0x4dfa90: ship +0x27a, +0x27b).
        armour: rec.fields.get(15).map_or(0, |&v| v.min(2) as u8),
        ram: crate::combat::Crew::from_record(&rec.fields, 0).class,
        behaviour: Behaviour::default(),
        zone: None,
        treasures: Vec::new(),
        heroes: Vec::new(),
        argo: false,
    });
    let combat = crate::combat::Combatant::from_record(rec, class);
    // Deposits: amber holds field 4 x 100 units; a negative amount starts
    // empty and grows to that much; it grows back field 9 times
    // (AmberClassTy init, 0x48a800). Fishing zones: field 4 x 100,
    // negative for unlimited.
    let mut dormant = None;
    let deposit = match class {
        ObjectClass::Amber => {
            let amount = f(4) as i32;
            let dep = Deposit {
                resource: Resource::Amber,
                remaining: Some(amount.max(0) as f32 * 100.0),
                regrows: f(9),
                refill: amount.unsigned_abs() as f32 * 100.0,
            };
            if amount > 0 {
                Some(dep)
            } else {
                dormant = Some((dep, 0.0));
                None
            }
        }
        ObjectClass::FishZone => {
            let amount = f(4) as i32;
            Some(Deposit {
                resource: Resource::Fish,
                remaining: (amount >= 0).then_some(amount as f32 * 100.0),
                regrows: 0,
                refill: 0.0,
            })
        }
        // One leviathan to a zone (a placeholder: whether zones refill is
        // not known).
        ObjectClass::WhaleZone => Some(Deposit {
            resource: Resource::Leviathan,
            remaining: Some(1.0),
            regrows: 0,
            refill: 0.0,
        }),
        _ => None,
    };
    Ok(Some(MapObject {
        id: rec.id,
        class,
        record: rec.clone(),
        picture: picture(rec),
        owner: match class {
            ObjectClass::Ship => Some((f(5), f(6))),
            ObjectClass::Building(_) | ObjectClass::Barbarian(_) => {
                Some((f(field::NATION), f(field::PLAYER)))
            }
            ObjectClass::Castle(_)
            | ObjectClass::HarpyNest
            | ObjectClass::CreatureSpawn
            | ObjectClass::Monster(0x1f4..=0x1f6 | 0x1f8..=0x1fb | 0x1fd) => {
                Some(crate::combat::MONSTERS)
            }
            _ => None,
        },
        label: label(rec),
        x,
        y,
        frames,
        frame_ms,
        ship,
        deposit,
        dormant,
        combat,
        treasure: (class == ObjectClass::Treasure).then(|| Treasure {
            kind: f(4).min(27) as u8,
            param: f(7),
            amount: f(8),
        }),
        progress: None,
        banner,
        cell,
    }))
}

/// Where a building's banner stands from the building's top left, by class
/// and nation (0-3): the tables each class's constructor sets (+0xdc, read
/// by 0x4247b0); settlements, towns, ports and yards fitted to the
/// original's first frame of miss000 (Greek).
fn banner_offset(class: u32, nation: u32) -> Option<(i32, i32)> {
    let table: [(i32, i32); 4] = match class {
        0x32 | 0x33 => [(37, 33); 4],
        0x39 => [(20, -15); 4],
        0x3a | 0x3b => [(36, -19); 4],
        0x34 => [(36, -16), (35, -9), (37, -22), (35, 1)],
        0x35 => [(36, -16), (32, -23), (37, -22), (34, 2)],
        0x36 => [(36, -16), (41, -23), (37, -22), (35, -22)],
        0x37 => [(37, -15), (44, 3), (52, -20), (49, -20)],
        0x38 => [(35, -17), (48, -16), (52, -3), (49, -20)],
        0x3c => [(38, -21), (37, -11), (38, -22), (34, -12)],
        0x3d => [(42, -13), (41, -13), (45, -26), (41, -14)],
        0x3e => [(56, -16), (48, -21), (37, -23), (42, -14)],
        _ => return None,
    };
    table.get(nation as usize).copied()
}

/// Ship type names, indexed by the type in mission records (field 7),
/// which is also the sprite letter (`'A' + type`) and the index of the cost
/// tables. The barbarians' five types (1, 2, 3, 5, 6) are the manual's
/// merchant, royal ship, bomber, rammer and ship killer. Build commands map
/// to types in 0x42d750.
pub const SHIP_NAMES: [&str; 12] = [
    "Boat",
    "Merchant",
    "Royal Ship",
    "Bomber",
    "Boarder",
    "Rammer",
    "Ship Killer",
    "Men Killer",
    "Fire Ship",
    "Quick Lime Ship",
    "Fire Rammer",
    "Ship of the Dead",
];

/// The side of a ship type's frame (its `SHIP_` composite), centred on the
/// ship: offsets in the original count from its top-left.
pub const SHIP_FRAME: [f32; 12] = [
    56.0, 64.0, 80.0, 80.0, 72.0, 72.0, 72.0, 72.0, 64.0, 72.0, 72.0, 72.0,
];

/// Game ticks a player building's picture takes to play through in each
/// state (construction, standing, ruins, ...): the constructors' cycle
/// tables (+0xc0, e.g. 0x596ab8), alike for all of them.
pub const BUILDING_CYCLE: [u32; 4] = [200, 75, 75, 75];

/// Milliseconds a frame of a standing building's `frames`-frame picture:
/// the cycle over the frames, in whole ticks (0x422e30).
pub fn building_frame_ms(frames: usize) -> u64 {
    (BUILDING_CYCLE[1] as u64 / frames.max(1) as u64).max(1) * TICK_MS
}

/// Top speeds by ship type (0x5bb4e0; the manual's chart, which swaps the
/// sixth and seventh): x 400 in the original's units of 1/6000 pixel a
/// tick (0x4ac210).
/// Whether ships of type `kind` cannot pass over shoals: the royal ship,
/// bomber and rammer (the manual's comparison chart).
pub fn deep_draught(kind: u8) -> bool {
    matches!(kind, 2 | 3 | 5)
}

const SHIP_SPEED: [u32; 12] = [150, 140, 100, 60, 90, 150, 120, 130, 90, 100, 120, 100];
/// World pixels per second per chart unit, before the wind factor (0.6 in
/// calm, up to 1.25 downwind; see `weather`): 400 units of 1/6000 pixel a
/// tick (0x4a3f40 moves a straight step half a pixel per 3000 units).
const SPEED_SCALE: f32 = 400.0 / 6000.0 / crate::clock::TICK;
/// Acceleration by ship type (0x5bb510, x 8 units a tick, 0x4a3a70).
const SHIP_ACCEL: [u32; 12] = [150, 140, 80, 40, 90, 120, 120, 130, 90, 100, 120, 100];

/// How fast a ship of type `kind` gathers way, in world pixels a second
/// per second (before the wind).
pub fn ship_accel(kind: u8) -> f32 {
    let tick = crate::clock::TICK;
    SHIP_ACCEL[kind as usize % 12] as f32 * 8.0 / 6000.0 / (tick * tick)
}

/// The object panel picture (0x4f5800, 0x5004d0).
fn picture(rec: &ObjectRecord) -> Option<(String, (u32, u32))> {
    let f = |i| rec.field(i).unwrap_or(0);
    match rec.class() {
        ObjectClass::Building(cls) => {
            let n = cls + f(field::NATION) * 13 - 0x32;
            Some((format!("INF_OBJ{n:02}"), (8, 6)))
        }
        ObjectClass::Ship => {
            const BASE: [u32; 12] = [0, 8, 16, 32, 40, 48, 56, 64, 72, 80, 72, 80];
            let (nation, player) = (f(5), f(6));
            let mut n = BASE.get(f(7) as usize).copied()?;
            match nation {
                1 => n += player,
                3 => n += 4 + player,
                _ => {}
            }
            Some((format!("1INF_SHIP{n:02}"), (10, 9)))
        }
        _ => None,
    }
}

fn nation_name(n: u32) -> &'static str {
    match n {
        1 => "Greek",
        3 => "Persian",
        4 => "Barbarian",
        _ => "Neutral",
    }
}

/// Name of a player building class.
pub fn building_label(class: u32) -> &'static str {
    match class {
        0x32 => "Settlement",
        0x33 => "Town",
        0x34 => "Small Fortress",
        0x35 => "Medium Fortress",
        0x36 => "Big Fortress",
        0x37 => "Workshop",
        0x38 => "Smithy",
        0x39 => "Port",
        0x3a => "Shipyard",
        0x3b => "Dock",
        0x3c => "Sage School",
        0x3d => "Temple of the Sea",
        _ => "Temple of War",
    }
}

fn label(rec: &ObjectRecord) -> String {
    let f = |i| rec.field(i).unwrap_or(0);
    let owner =
        |nation: u32, player: u32| format!("{}, player {}", nation_name(nation), player + 1);
    match rec.class() {
        ObjectClass::Ship => {
            let name = SHIP_NAMES.get(f(7) as usize).copied().unwrap_or("Ship");
            format!("{name} ({})", owner(f(5), f(6)))
        }
        ObjectClass::Building(cls) => {
            let name = building_label(cls);
            format!("{name} ({})", owner(f(field::NATION), f(field::PLAYER)))
        }
        ObjectClass::Barbarian(_) => "Barbarian stronghold".into(),
        ObjectClass::Castle(c) => if c == 0x42 {
            "Sea castle"
        } else {
            "War castle"
        }
        .into(),
        ObjectClass::HarpyNest => if rec.class_id == 0x44 {
            "Dragon nest"
        } else {
            "Harpy nest"
        }
        .into(),
        ObjectClass::CreatureSpawn => match rec.class_id {
            0x46 => "Cyclops lair",
            0x47 => "Minotaur lair",
            _ => "Amazon village",
        }
        .into(),
        ObjectClass::HeroHouse => format!("House of {}", HERO_NAMES[f(22).min(8) as usize]),
        ObjectClass::Treasure => treasure_name(f(4).min(27) as u8),
        ObjectClass::Amber => "Amber".into(),
        ObjectClass::FishZone => "Fish".into(),
        ObjectClass::WhaleZone => "Leviathan".into(),
        ObjectClass::Monster(0x1fa) => "Medusa".into(),
        ObjectClass::Monster(0x1fb) => "Sea monster".into(),
        ObjectClass::Monster(0x1f4) => "Cyclops".into(),
        ObjectClass::Monster(0x1f5) => "Minotaur".into(),
        ObjectClass::Monster(0x1f6) => "Amazon".into(),
        ObjectClass::Monster(0x1f8) => "Harpy".into(),
        ObjectClass::Monster(0x1f9) => "Dragon".into(),
        ObjectClass::Monster(0x1fd) => "Siren".into(),
        ObjectClass::Whirlpool => "Whirlpool".into(),
        ObjectClass::Reef(_) => "Reef".into(),
        ObjectClass::Lava => "Volcano".into(),
        ObjectClass::Sheep => "Sheep".into(),
        other => format!("{other:?}"),
    }
}

use Anchor::{CellCentre, CellCorner};

/// Animation cycle lengths in game ticks, per state (0 appear, 1 live, ...).
const CASTLE_CYCLE: [u64; 4] = [300, 20, 100, 75]; // 0x5949e8
const WHIRL_CYCLE: [u64; 3] = [60, 60, 60]; // 0x5972b0
const REEF_CYCLE: [u64; 3] = [30, 60, 30]; // 0x596a50

/// How a building looks: being built, standing or in ruins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingState {
    Construction,
    /// Standing, with damage level 3 (intact), 2 or 1.
    Standing(u32),
    Ruins,
}

impl BuildingState {
    /// The standing state for a condition in percent (LandObjClassTy
    /// 0x422e30: above 67 intact, above 33 level 2, else 1).
    pub fn for_condition(percent: f32) -> Self {
        // ceil(health x 2 / 67) of the whole percent (0x594ab0), at least 1.
        let level = (percent.floor() * 2.0 / 67.0).ceil() as u32;
        Self::Standing(level.clamp(1, 3))
    }
}

/// Frames of a building record drawn in `state`.
pub fn building_frames(
    data: &GameData,
    caches: &mut Caches,
    rec: &ObjectRecord,
    state: BuildingState,
) -> Result<Vec<Vec<Layer>>> {
    let f = |i| rec.field(i).unwrap_or(0);
    let cls = rec.class_id;
    match building_name(cls, f(field::NATION), f(10), state) {
        Some(name) => composite_frames(data, caches, &name, 0..usize::MAX),
        None => Ok(Vec::new()),
    }
}

/// Composite name of a building (0x4288c0): prefix, nation, for some the
/// class, then the state (0 construction, 1 standing, 2 ruins), for ports
/// and docks the facing (0 water to the east, 2 north, 4 west, 6 south),
/// and the damage level (3 intact .. 1; construction 3, ruins 1).
fn building_name(cls: u32, nation: u32, facing: u32, state: BuildingState) -> Option<String> {
    let (state, damage) = match state {
        BuildingState::Construction => (0, 3),
        BuildingState::Standing(d) => (1, d.clamp(1, 3)),
        BuildingState::Ruins => (2, 1),
    };
    let (prefix, with_class, with_facing) = match cls {
        0x32 | 0x33 => ("TSPR_", true, false),
        0x34 => ("FORTSSPR_", false, false),
        0x35 => ("FORTSPR_", false, false),
        0x36 => ("FORTBSPR_", false, false),
        0x37 => ("STSPR_", false, false),
        0x38 => ("SMSPR_", false, false),
        0x39 => ("PTSPR_", true, true),
        0x3a | 0x3b => ("DKSPR_", true, true),
        0x3c => ("SLSPR_", false, false),
        0x3d => ("TSSPR_", false, false),
        0x3e => ("TWSPR_", false, false),
        // Barbarian structures. 0x40 embeds its class and a facing like
        // ports do; which of 0x3f/0x41 is the small and which the big
        // fort is inferred, not confirmed.
        0x3f => ("BARBSSPR_", false, false),
        0x40 => ("BARBSPR_", true, true),
        0x41 => ("BARBBSPR_", false, false),
        _ => return None,
    };
    let mut name = format!("{prefix}{nation}");
    if with_class {
        name += &format!("{cls:2}");
    }
    name += &state.to_string();
    if with_facing {
        name += &facing.to_string();
    }
    name += &damage.to_string();
    Some(name)
}

/// A small flock's still picture: `count` sheep around the cell centre
/// (what is picked and its bounds; the flock itself is drawn and moved by
/// `scene::sheep`).
fn sheep_frames(data: &GameData, caches: &mut Caches, count: u32) -> Result<Vec<Vec<Layer>>> {
    const SPOTS: [(i32, i32); 8] = [
        (-14, -6),
        (8, -12),
        (16, 6),
        (-4, 10),
        (-20, 14),
        (4, -2),
        (22, -8),
        (-10, -18),
    ];
    let mut frames: Vec<Vec<Layer>> = Vec::new();
    for (i, &(dx, dy)) in SPOTS.iter().take(count.clamp(1, 8) as usize).enumerate() {
        let names = caches.sequence(data, &format!("SHEEP_{}1", i % 4))?;
        for (f, n) in names.iter().enumerate() {
            if let Some(image) = caches.sprites.get(data, n)? {
                if frames.len() <= f {
                    frames.resize_with(f + 1, Vec::new);
                }
                frames[f].push(Layer { image, dx, dy });
            }
        }
    }
    Ok(frames)
}

/// A ship type's sinking (mode 1, 20 frames) or under-construction (mode
/// 2, 80 frames: 40 lying east-west then 40 north-south) animation:
/// `SHIP_<letter><hull nation>00<mode>`.
pub fn ship_mode_frames(
    data: &GameData,
    caches: &mut Caches,
    kind: u8,
    nation: u32,
    mode: u32,
) -> Result<Vec<Vec<Layer>>> {
    let letter = (b'A' + kind.min(25)) as char;
    composite_frames(
        data,
        caches,
        &format!("SHIP_{letter}{nation}00{mode}"),
        0..usize::MAX,
    )
}

/// The frames of one layer of composite `name`, e.g. one facing of the
/// unloading walkway `MOST0_`, whose eight layers are its eight facings.
pub fn composite_layer_frames(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
    layer: usize,
) -> Result<Vec<Vec<Layer>>> {
    let Some(comp) = caches.composite(data, name)? else {
        return Ok(Vec::new());
    };
    let mut frames = Vec::new();
    for i in 0..comp.frames {
        let mut layers = Vec::new();
        if let Some(cell) = comp.frame(i).get(layer) {
            if cell.sprite != EMPTY_SPRITE {
                if let Some(image) = caches.sprites.get(data, &cell.sprite)? {
                    layers.push(Layer {
                        image,
                        dx: cell.dx,
                        dy: cell.dy,
                    });
                }
            }
        }
        frames.push(layers);
    }
    Ok(frames)
}

fn composite_frames(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
    range: std::ops::Range<usize>,
) -> Result<Vec<Vec<Layer>>> {
    composite_frames_with(data, caches, name, range, |_, _| true)
}

/// A composite's frames, its layers drawn in their order (the wake, order
/// -21000, first; then from the highest order to the lowest, so a ship's
/// sails, 4, lie over its hull, 10 and 11), keeping the sprites `keep`
/// accepts.
fn composite_frames_with(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
    range: std::ops::Range<usize>,
    keep: impl Fn(usize, &str) -> bool,
) -> Result<Vec<Vec<Layer>>> {
    let Some(comp) = caches.composite(data, name)? else {
        return Ok(Vec::new());
    };
    let mut frames = Vec::new();
    for i in range.start..range.end.min(comp.frames) {
        let mut layers = Vec::new();
        let mut cells: Vec<_> = comp.frame(i).iter().enumerate().collect();
        cells.sort_by_key(|(_, c)| {
            if c.order < 0 {
                (0, c.order)
            } else {
                (1, -c.order)
            }
        });
        for (k, cell) in cells {
            if cell.sprite == EMPTY_SPRITE || !keep(k, &cell.sprite) {
                continue;
            }
            if let Some(image) = caches.sprites.get(data, &cell.sprite)? {
                layers.push(Layer {
                    image,
                    dx: cell.dx,
                    dy: cell.dy,
                });
            }
        }
        frames.push(layers);
    }
    Ok(frames)
}

pub fn sequence_frames(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
) -> Result<Vec<Vec<Layer>>> {
    let names = caches.sequence(data, name)?;
    let mut frames = Vec::with_capacity(names.len());
    for n in names.iter() {
        if let Some(image) = caches.sprites.get(data, n)? {
            frames.push(vec![Layer {
                image,
                dx: 0,
                dy: 0,
            }]);
        }
    }
    Ok(frames)
}

fn settlement_class() -> u32 {
    crate::economy::SETTLEMENT
}
