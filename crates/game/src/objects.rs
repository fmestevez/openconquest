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

/// Length of a game tick at the default game speed (speed table at
/// 0x5bed7c, row 3).
const TICK_MS: u64 = 20;
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
    /// Hull and weapons, for objects that can fight or be attacked.
    pub combat: Option<crate::combat::Combatant>,
    /// Owning player as `(nation, colour slot)`, for player-owned objects.
    pub owner: Option<(u32, u32)>,
    /// Picture for the object panel (`INF_SHIPnn` / `INF_OBJnn` in CONTROLG)
    /// and its offset in the panel.
    pub picture: Option<(String, (u32, u32))>,
}

/// Movement state of a ship.
pub struct Ship {
    /// Ship type 0-11 (see `SHIP_NAMES`).
    pub kind: u8,
    /// 0 = east, counting counter-clockwise in 45 degree steps.
    pub heading: usize,
    /// Cruising speed in world pixels per second.
    pub speed: f32,
    /// Remaining waypoints (cell centres), nearest first.
    pub path: std::collections::VecDeque<(f32, f32)>,
    /// Goods on board.
    pub cargo: f32,
    pub cargo_kind: Option<Resource>,
    /// A standing order the ship carries out on its own.
    pub task: Option<Task>,
    /// Marines on board.
    pub crew: crate::combat::Crew,
}

/// Goods that can be gathered and carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Amber,
    Fish,
}

/// A deposit of a resource on the map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Deposit {
    pub resource: Resource,
    /// Units left, or `None` for an inexhaustible deposit.
    pub remaining: Option<f32>,
}

/// A ship's standing order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Task {
    /// Shuttle between a deposit and the nearest port.
    Gather { deposit: u32, phase: GatherPhase },
    /// Chase and fire at an enemy ship or building.
    Attack { target: u32 },
    /// Sail to a land cell and found a settlement there.
    Found { cx: i32, cy: i32 },
    /// Run into an enemy ship.
    Ram { target: u32 },
    /// Come alongside an enemy ship and fight for it; `round` counts down
    /// to the next exchange of blows.
    Board { target: u32, round: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GatherPhase {
    ToDeposit,
    Collecting,
    ToPort(u32),
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

impl MapObject {
    pub fn is_armed(&self) -> bool {
        self.combat.as_ref().is_some_and(|c| c.is_armed())
    }

    pub fn frame_at(&self, time_ms: u64) -> &[Layer] {
        let step = (time_ms / self.frame_ms.max(1)) as usize;
        let i = match &self.ship {
            Some(ship) => {
                let per = SHIP_FRAMES_PER_HEADING.min(self.frames.len().max(1));
                (ship.heading * per + step % per) % self.frames.len().max(1)
            }
            None => step % self.frames.len().max(1),
        };
        &self.frames[i]
    }
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
            (
                CellCentre(f(5), f(6)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Ship => {
            let letter = (b'A' + f(7).min(25) as u8) as char;
            let (nation, player) = (f(5), f(6));
            let name = format!("SHIP_{letter}{nation}{nation}{player}0");
            // The first 32 frames are 8 headings x 4 wake animation frames.
            let frames = composite_frames(
                data,
                caches,
                &name,
                0..SHIP_HEADINGS * SHIP_FRAMES_PER_HEADING,
            )?;
            (
                CellCentre(f(9), f(10)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
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
        ObjectClass::HeroHouse => {
            let name = format!("HHOUSE{}SPR_013", f(22).min(8));
            let frames = composite_frames(data, caches, &name, 0..usize::MAX)?;
            (
                CellCentre(f(5), f(6)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
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
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Monster(0x1fb) => {
            // Sea monster, `UMON<type><state><heading>` (0x490040); state 0 is idle.
            let frames = sequence_frames(data, caches, &format!("UMON{}00", f(4).min(3)))?;
            (
                CellCentre(f(7), f(8)),
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
            (
                CellCorner(f(6), f(7), 0, 0),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
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
    let Visual { frames, frame_ms } = visual;
    let Some(first) = frames.iter().flatten().next() else {
        return Ok(None);
    };
    let t = TILE_SIZE as f32;
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
        cargo: 0.0,
        cargo_kind: None,
        task: None,
        crew: crate::combat::Crew::from_record(&rec.fields),
    });
    let combat = crate::combat::Combatant::from_record(rec, class);
    // Deposits: amber holds field 4 x 100 units, negative for unlimited
    // (AmberClassTy init, 0x48a800); fishing zones are treated the same way.
    let deposit = match class {
        ObjectClass::Amber | ObjectClass::FishZone => {
            let amount = f(4) as i32;
            Some(Deposit {
                resource: if class == ObjectClass::Amber {
                    Resource::Amber
                } else {
                    Resource::Fish
                },
                remaining: (amount >= 0).then_some(amount as f32 * 100.0),
            })
        }
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
            _ => None,
        },
        label: label(rec),
        x,
        y,
        frames,
        frame_ms,
        ship,
        deposit,
        combat,
    }))
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

/// Maximum speeds from the manual's comparison chart, in type order.
const SHIP_SPEED: [u32; 12] = [150, 140, 100, 60, 90, 150, 130, 120, 90, 100, 120, 100];
/// World pixels per second per manual speed unit, before the wind factor
/// (0.6 in calm, up to 1.2 downwind; see `weather`). Chosen for feel; the
/// original's base speeds have not been recovered yet.
const SPEED_SCALE: f32 = 0.75;

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
        ObjectClass::HeroHouse => "Hero's house".into(),
        ObjectClass::Amber => "Amber".into(),
        ObjectClass::FishZone => "Fish".into(),
        ObjectClass::WhaleZone => "Leviathan".into(),
        ObjectClass::Monster(0x1fa) => "Medusa".into(),
        ObjectClass::Monster(0x1fb) => "Sea monster".into(),
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
    /// The standing state for a condition in percent (thirds, as the nests
    /// at 0x41d010 do).
    pub fn for_condition(percent: f32) -> Self {
        Self::Standing(if percent > 66.0 {
            3
        } else if percent > 33.0 {
            2
        } else {
            1
        })
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

/// A small flock: `count` sheep scattered around the cell centre. The
/// original spreads and moves them at random; this placement is fixed.
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

fn composite_frames(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
    range: std::ops::Range<usize>,
) -> Result<Vec<Vec<Layer>>> {
    let Some(comp) = caches.composite(data, name)? else {
        return Ok(Vec::new());
    };
    let mut frames = Vec::new();
    for i in range.start..range.end.min(comp.frames) {
        let mut layers = Vec::new();
        for cell in comp.frame(i) {
            if cell.sprite == EMPTY_SPRITE {
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
