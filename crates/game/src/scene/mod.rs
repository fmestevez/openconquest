//! The currently loaded mission.

use anyhow::Result;
use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::{players, MapGrid, ObjectClass, ObjectRecord, PlayerInfo};

use crate::assets::GameData;
use crate::combat::{self, Effect, Rng};
use crate::economy::{self, DOCK, PORT, SETTLEMENT, SHIPYARD, TOWN, WORKSHOP};
use crate::magic::{self, Magic, Spell, Temple};
use crate::nav::NavGrid;
use crate::objects::{
    self, cargo_capacity, BuildingState, Caches, GatherPhase, MapObject, Resource, Task, SHIP_NAMES,
};
use crate::render::palette_lut;
use crate::save::{self, SaveGame, SavedObject};
use crate::terrain::Terrain;
use crate::weather::Wind;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// What a settlement (marines) or a workshop (weapons) has been ordered to
/// make: one queue of items of any kinds, paid when ordered, made one after
/// another (0x43ba60 appends to the settlement's byte array at +0x260,
/// head +0x254, tail +0x258; the workshop's is at +0x1ec), the run timed
/// from its start (+0x124).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Training {
    pub site: u32,
    /// The items ordered, in order (see `combat::ITEM_NAMES`).
    #[serde(default)]
    pub queue: Vec<u8>,
    /// How many of them are done.
    #[serde(default)]
    pub head: usize,
    /// Seconds since the run started.
    pub elapsed: f32,
    /// Saves from before the queue held kinds: one kind, `queued` of it.
    #[serde(default, skip_serializing)]
    kind: Option<u8>,
    #[serde(default, skip_serializing)]
    queued: Option<u32>,
}

impl Training {
    pub fn new(site: u32) -> Self {
        Self {
            site,
            queue: Vec::new(),
            head: 0,
            elapsed: 0.0,
            kind: None,
            queued: None,
        }
    }

    /// Converts a save from before the queue held kinds.
    pub(crate) fn upgrade_save(&mut self) {
        if let (Some(kind), Some(n)) = (self.kind.take(), self.queued.take()) {
            if self.queue.is_empty() {
                self.queue = vec![kind; n as usize];
                self.head = 0;
            }
        }
    }

    /// Items of kind `kind` ordered and not yet made.
    pub fn count(&self, kind: u8) -> u32 {
        self.queue[self.head.min(self.queue.len())..]
            .iter()
            .filter(|&&k| k == kind)
            .count() as u32
    }

    /// The share 0..1 of the items of kind `kind` made, as the button shows
    /// it, for a settlement (0x43c740: the time since the run started over
    /// the time when the last of them is done) or a workshop (0x435ec0:
    /// the time spent on them over all their time); `None` with none left.
    pub fn progress(&self, kind: u8, workshop: bool) -> Option<f32> {
        if self.count(kind) == 0 {
            return None;
        }
        let time = |k: u8| combat::train_seconds(k);
        if !workshop {
            let last = self.queue.iter().rposition(|&k| k == kind)?;
            return Some((self.elapsed / (time(kind) * (last + 1) as f32)).min(1.0));
        }
        let (mut done, mut left, mut start) = (0.0, 0.0, 0.0);
        for (i, &k) in self.queue.iter().enumerate() {
            if k == kind {
                let spent = (self.elapsed - start).clamp(0.0, time(k));
                if i < self.head {
                    done += time(k);
                } else {
                    done += spent;
                    left += time(k) - spent;
                }
            }
            start += time(k);
        }
        Some(done / (done + left).max(f32::EPSILON))
    }
}

/// How a mission ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Outcome {
    Victory,
    Defeat,
}

/// The mission objectives (string resources 3000-3010), in `CONDITIONS`
/// order, with the record classes each one counts.
/// Whether `mission` is one of the campaign's (`miss000`-`miss013`).
/// Whether `mission` is a custom one (`battle..` or `single..`), which the
/// original starts in its own mode (game +0xd80 = 2).
pub fn is_custom(mission: &str) -> bool {
    mission.starts_with("battle") || mission.starts_with("single")
}

pub fn is_campaign(mission: &str) -> bool {
    mission.len() == 7
        && mission.starts_with("miss")
        && mission[4..].bytes().all(|b| b.is_ascii_digit())
}

/// A shoal's clock (Fish.cpp 0x48dd80): a frame of its animation each
/// game tick (timed on the original: shoals up 4-8 seconds, gone a few).
const SHOAL_UNIT: f32 = crate::clock::TICK;
/// It rises (`FISH_<size>1`, 33 frames), lives 0-3 passes of `FISH_<size>2`
/// (60 frames), dives (`FISH_<size>3`, 33 frames) and stays below 25-75
/// units before it comes up elsewhere in its zone.
const SHOAL_SURFACING: f32 = 33.0 * SHOAL_UNIT;
const SHOAL_LIVING: f32 = 60.0 * SHOAL_UNIT;
const SHOAL_DIVING: f32 = 33.0 * SHOAL_UNIT;

/// An animation's frames, each a stack of layers.
pub type Frames = Vec<Vec<objects::Layer>>;

/// A spell a brain casts on a cell for nothing: the spell, the cell and
/// the casting player (nation, slot), if it matters.
type BrainCast = (Spell, i32, i32, Option<(u32, u32)>);

/// Cells the minimap shows across and down (Mapa.cpp, mode 3).
pub const MINIMAP_CELLS: i32 = 24;

/// Seconds between steps of the resource counters (a game tick).
const COUNTER_TICK: f32 = crate::clock::TICK;

pub const OBJECTIVES: [(&str, &[u32]); 11] = [
    ("Kill all enemies", &[]),
    ("Kill all dragons", &[0x44]),
    ("Kill all wizards of fire", &[0x43]),
    ("Kill all wizards of wind", &[0x42]),
    ("Kill all sirens", &[0x1fd]),
    ("Kill all gorgons", &[0x1fa]),
    ("Kill all harpies", &[0x45]),
    ("Kill all sea creatures", &[0x1fb]),
    ("Kill all amazons", &[0x48]),
    ("Kill all minotaurs", &[0x47]),
    ("Kill all cyclops", &[0x46]),
];

/// A building going up, or a settlement or shipyard being made a town or
/// dock (the same picture, state 0). Its construction picture steps a
/// frame every `BUILDING_CYCLE[0] / n` ticks, `n` its frame count (0x422e30
/// sets the delay `+0x7c->+0x7d`; 0x4230f0 steps it, the first step on the
/// first tick); it stands at the step that reaches `n`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Construction {
    pub site: u32,
    /// An upgrade (TownObj `+0x23c`, Dock `+0x1e8`), not a new building.
    #[serde(default)]
    pub upgrade: bool,
    /// The picture's frame (`+0x7c->+0x45`) and frame count (`+0x49`).
    #[serde(default)]
    pub frame: u32,
    #[serde(default)]
    pub frames: u32,
    /// The first step is made (`+0xbc`), and the ticks since the last.
    #[serde(default)]
    pub started: bool,
    #[serde(default)]
    pub wait: u32,
    /// Saves from before the frames were counted: seconds of a whole.
    #[serde(default, skip_serializing)]
    elapsed: f32,
    #[serde(default, skip_serializing)]
    total: f32,
}

impl Construction {
    pub fn new(site: u32, upgrade: bool) -> Self {
        Self {
            site,
            upgrade,
            ..Self::default()
        }
    }

    /// How far it is, 0..1: the frame over the frame count.
    pub fn done(&self) -> f32 {
        if self.frames == 0 {
            return 0.0;
        }
        (self.frame as f32 / self.frames as f32).min(1.0)
    }

    /// Converts a save from before the frames were counted.
    pub(crate) fn upgrade_save(&mut self, frames: u32) {
        if !self.started && self.total > 0.0 && frames > 0 {
            self.frame = ((self.elapsed / self.total).min(1.0) * frames as f32) as u32;
            self.frames = frames;
            self.started = self.frame > 0;
            self.total = 0.0;
        }
    }
}

/// How far from the settlement's own cells a moorage or shipyard may go, in
/// cells; other buildings one less (probed on the original in miss000).
pub const BUILD_RADIUS: i32 = 2;
/// How close a ship must lie to the site of the settlement it founds.
pub const FOUND_REACH: i32 = 2;

pub struct Scene {
    /// "Default arming" (the Building page's checkbox, 0x1fdb): new ships
    /// and forts come armed, the weapons paid for or taken from stock.
    pub default_arming: bool,
    pub mission: String,
    pub grid: MapGrid,
    /// Each cell's tile types, a nibble a layer (the landscape grid's
    /// dword at +8, 0x5456c0: the tile set record's type, byte 4; 0 where
    /// the layer is empty).
    pub cell_types: Vec<u32>,
    pub terrain: Terrain,
    pub objects: Vec<MapObject>,
    pub nav: NavGrid,
    /// The human player, if the mission defines one.
    pub human: Option<PlayerInfo>,
    /// The human player's current amber and fish.
    pub amber: u32,
    pub fish: u32,
    /// The amber and fish the counters show: they roll towards the real
    /// amounts by 1, 10 or 100 a tick, from 1 at the start (0x505530).
    pub shown: [u32; 2],
    shown_clock: f32,
    /// Ships ordered at yards, waiting for a free berth (0x416a40's queue),
    /// and the ticks since each yard's queue was last looked at (`+0x1fc`).
    pub ship_orders: Vec<yard::ShipOrder>,
    pub yard_clocks: BTreeMap<u32, u32>,
    /// Ships just built on berths, made before the next tick's yards look
    /// (not saved: made at once).
    new_ships: Vec<yard::NewShip>,
    /// The ships' generators' seed (the game's `+0x2ee5`; see
    /// `Scene::ship_draw`).
    rnd_init: Option<u32>,
    /// Buildings going up.
    pub constructions: Vec<Construction>,
    /// The frame a standing settlement or shipyard's picture is at
    /// (`+0x7c->+0x45`, stepped by 0x4230f0 in state 1; its `frame`,
    /// `started` (`+0xbc`) and `wait`), which an upgrade starts from.
    pub standing_frames: BTreeMap<u32, Construction>,
    /// Amber and fish of the computer players.
    pub purses: std::collections::BTreeMap<(u32, u32), (u32, u32)>,
    /// Research levels per player (missing: all 0).
    pub levels: std::collections::BTreeMap<(u32, u32), combat::Levels>,
    /// Research at a sage school: site, upgrade, level, seconds spent.
    pub research: Vec<(u32, combat::Upgrade, u8, f32)>,
    /// Refits queued at a smithy, done in turn: site, armour or ram,
    /// level, seconds spent.
    pub refits: Vec<(u32, combat::Upgrade, u8, f32)>,
    /// Summoned creatures and cast whirlpools and reefs, and the seconds
    /// they have left.
    pub summons: Vec<(u32, f32)>,
    /// Marines in training.
    pub trainings: Vec<Training>,
    /// Trained marines waiting at each settlement, by crew type.
    pub garrisons: std::collections::BTreeMap<u32, [u32; 9]>,
    /// Sounds to play: name (in `sounds.dkx`) and world position. The
    /// front end drains this every frame.
    pub sounds: Vec<(&'static str, f32, f32)>,
    /// News for the player (e.g. an order that could not be carried out).
    pub messages: Vec<String>,
    pub minimap: Vec<u32>,
    pub minimap_size: (u32, u32),
    /// The minimap's first cell shown: it shows 24 x 24 cells and follows
    /// the view (Mapa.cpp 0x51b8c0).
    pub minimap_origin: std::cell::Cell<(i32, i32)>,
    /// Running one-shot animations (explosions, splashes).
    pub effects: Vec<Effect>,
    /// Projectiles in flight (not saved: a save lets them land at once).
    pub shots: Vec<shot::Shot>,
    /// Ships burning (not saved: flames are short-lived).
    pub fires: Vec<fire::Fire>,
    /// Volcanoes' sleep and eruptions.
    pub volcanoes: HashMap<u32, volcano::Volcano>,
    /// Where one of the player's objects last took a blow.
    pub last_attack: Option<(f32, f32)>,
    rng: Rng,
    /// Fish shoals out of sight between moves: the zone and seconds until
    /// they surface again; and those in sight: seconds until they leave
    /// (FishClassTy, Fish.cpp 0x48dd80).
    pub shoals_gone: std::collections::BTreeMap<u32, f32>,
    /// Reefs' and whirlpools' cycles (0x4306d0, 0x447ee0), by object.
    pub hazards: std::collections::BTreeMap<u32, hazards::Hazard>,
    /// Leviathans' states (WhaleClassTy 0x494f90), by object id.
    pub leviathans: std::collections::BTreeMap<u32, leviathan::Leviathan>,
    shoals_shown: std::collections::BTreeMap<u32, f32>,
    /// Seconds since a shoal surfaced (it rises with `FISH_11`).
    shoals_up: std::collections::BTreeMap<u32, f32>,
    /// Shoals of each size (1-3) surfacing, living and diving:
    /// `FISH_<size><1-3>`.
    pub fish_art: std::rc::Rc<[[Frames; 3]; 3]>,
    /// Seagulls (only shown) and their pictures: `GULLSPR`, `GULLSHD_SPR`.
    pub gulls: Vec<gulls::Gull>,
    gull_rng: Rng,
    pub gull_art: std::rc::Rc<(Frames, Frames)>,
    /// Ships' wakes (only shown) and their sprites: `TRAKS00`, `TRAKS14`.
    pub droplets: Vec<wake::Droplet>,
    /// The configuration's "graphic trails": ships throw off droplets,
    /// some projectiles leave trails.
    pub trails: bool,
    /// Projectiles' trail particles by kind (`TRAKS<kind>`), and the
    /// tick clock they come by.
    pub trail_art: BTreeMap<u8, Frames>,
    trail_clock: f32,
    /// Flocks of sheep (only shown), their clock, and their pictures by
    /// direction, grazing and walking (`SHEEP_<dir><1|2>`).
    pub flocks: sheep::Flocks,
    sheep_clock: f32,
    sheep_ticks: u32,
    pub sheep_art: std::rc::Rc<[[Frames; 2]; 8]>,
    /// Amber deposits' glints and the sparkle's frames (`ASTAR_`).
    pub sparkles: sparkle::Sparkles,
    pub sparkle_art: std::rc::Rc<Frames>,
    /// The boarders' walkway `MOST0_` by facing (a layer each).
    pub plank_art: std::rc::Rc<[Frames; 8]>,
    /// Ships whose laden porter trips have just put a slot ashore.
    unloaded: Vec<u32>,
    wake_clock: f32,
    pub wake_art: std::rc::Rc<(Frames, Frames)>,
    /// Seconds until the computer players next decide.
    ai_timer: f32,
    /// Seconds since the mission started.
    pub clock: f32,
    /// The campaign mission's story objectives (empty elsewhere: the
    /// `CONDITIONS` decide).
    pub story: Vec<Goal>,
    /// The player's settlements and towns at the start.
    pub start_sites: Vec<u32>,
    /// Kinds of treasure the player has handed over at a settlement.
    pub delivered: Vec<u8>,
    /// The campaign judge's record: heroes joined (0x5d99), the Argo once
    /// afloat, ticks the win has held, ticks to a loss once seen.
    pub joined: Vec<u8>,
    pub had_argo: bool,
    /// One of the player's buildings has woken (0x5d9a).
    pub woken: bool,
    /// The lime's whitening as colour to colour (`render::LIME` through
    /// the palette, its first entry for each colour).
    pub lime_remap: std::collections::HashMap<u32, u32>,
    /// Map cells lightning has dug (layer, x, y, the tile now), laid again
    /// on a saved game's load.
    pub dug: Vec<(u32, u32, u32, openconquest_formats::map::TileRef)>,
    /// miss009's strait (0x4663d0, 0x464340): the player's lightning has
    /// struck in it, and the way through was found open after.
    pub strait_struck: bool,
    pub strait_checked: bool,
    pub winning: u32,
    pub losing: Option<u32>,
    /// The campaign mission's ported brain, if any, and time owed to it.
    pub brain: Option<brain::Brain>,
    brain_clock: f32,
    /// The computer players of a battle or single mission.
    pub computers: computer::Computers,
    /// Heroes the campaign brought to this mission, and heroes waiting at
    /// settlements and towns (by site).
    pub carried: Vec<u8>,
    pub site_heroes: std::collections::BTreeMap<u32, Vec<u8>>,
    /// Treasures held by objects, dropped when they are destroyed.
    pub held: Vec<(u32, crate::objects::Treasure)>,
    /// Armour and rams stored at settlements (levels: armour, rams), put
    /// ashore from ships in the load panel.
    pub fittings: BTreeMap<u32, load::Fittings>,
    /// The towns' buildings (`+0x1c8` of each, `+0x1d0` of each town).
    pub towns: towns::Towns,
    /// Settlements and towns sold (state 3).
    pub sold: Vec<u32>,
    /// Ships lying alongside an own port, dock or ship to reload.
    pub loads: Vec<load::LoadLink>,
    /// Harpies and dragons out of their nests, and the nests' counts.
    pub flyers: Vec<flyers::Flyer>,
    pub nests: Vec<flyers::Nest>,
    /// Cyclops, minotaurs and amazons out of their lairs, and the seconds
    /// since each lair last sent one.
    pub walkers: Vec<walkers::Walker>,
    pub lair_clocks: std::collections::BTreeMap<u32, f32>,
    /// The land creatures' jams (CyclopsJamClassTy 0x1f7), one a cell
    /// they stand on.
    pub jams: Vec<walkers::Jam>,
    /// Throws land creatures made this tick, for `run_combat` (not saved:
    /// fired in the same update).
    throws: Vec<walkers::Throw>,
    /// The human players of a multiplayer game (empty otherwise), and the
    /// one whose state is in the fields above now.
    pub seats: Vec<seats::Seat>,
    pub active: usize,
    /// Each player's starting view (`WIN_VIEW`): the top-left cell of the
    /// original's view, by player id `nation * 4 + slot`.
    pub views: Vec<Option<(u32, u32)>>,
    /// Walkways and porters of ships unloading (only shown).
    pub planks: Vec<unload::Plank>,
    pub porters: Vec<unload::Porter>,
    /// The human player's statistics for the outcome screen.
    pub stats: stats::Stats,
    /// What the human player may build, fit, research and learn.
    pub permits: crate::permits::Permits,
    /// Seconds since the mission began, for the creatures' rhythms.
    pub creature_clock: f32,
    /// Wizards' casting.
    pub wizards: BTreeMap<u32, script::Wizard>,
    /// Big reefs a brain raises this tick: cell and kind (not saved: raised
    /// at once).
    pending_reefs: Vec<(i32, i32, u32)>,
    /// Land creatures a brain puts down this tick (0x414db0): class and
    /// cell (not saved: put down at once).
    pending_walkers: Vec<(u32, (i32, i32))>,
    /// Spells a brain casts this tick for nothing (0x42f580): the spell,
    /// its cell and the caster (not saved: cast at once).
    pending_casts: Vec<BrainCast>,
    /// The brains' players' Flame: (seconds left brewing, from saves made
    /// before the brews were kept by temple; seconds of it burning).
    pub brain_flames: std::collections::BTreeMap<(u32, u32), (f32, f32)>,
    /// The Flames the brains' war temples brew (0x439260): the temple, its
    /// player and the seconds left.
    pub brain_brews: Vec<(u32, (u32, u32), f32)>,
    /// Spells the player may cast once for nothing (disposable magic).
    pub free_casts: std::collections::BTreeMap<Spell, u32>,
    /// Buildings being repaired by a settlement or town.
    pub mending: Vec<u32>,
    /// Forts and barbarian structures being mended mending their units.
    pub unit_mends: Vec<repair::UnitMend>,
    /// The conditions of each settlement's stock units by item, in their
    /// order (kept as long as `garrisons` counts; see `stock_units`).
    pub stock_conditions: std::collections::BTreeMap<u32, [Vec<u8>; 9]>,
    /// Settlements mending their stock (state 2, 0x441ba0).
    pub stock_repairs: Vec<repair::StockRepair>,
    /// Each smithy's refits as the one batch the original runs (`+0x114`
    /// ticks, `+0x118` amber summed, from `+0x124`; 0x434a00 message
    /// 0x138c), for order 10's refund.
    pub refit_batches: BTreeMap<u32, halt::Batch>,
    /// The ships' orders of the 0x1f0 group given since the last tick
    /// (one a tick, `Scene::one_event_a_tick`).
    pub tick_orders: BTreeMap<u32, order::TickOrder>,
    /// Commands given and not yet delivered: the Players object's outbox
    /// (`+0x2e`, 0x520cc0), delivered in order at the next tick
    /// (`Scene::deliver`).
    pub outbox: Vec<Order>,
    /// The groups' gather zones (fleet order 0x78, `gather`), in the
    /// order the navy's tick runs them.
    pub gather_zones: Vec<gather::GatherZone>,
    /// The last human group's serial number.
    pub zone_serial: u32,
    /// The navy's generator (`[0x5c7504] +0x18`), until its first draw.
    pub navy_lcg: Option<u32>,
    /// Game ticks played (`Players +0xde`), for the outbox's first hold.
    pub ticks: u32,
    /// What a cancelled training's fish share leaves in a settlement or
    /// town: its bytes `+0x252` (count) and `+0x253` (last part), 0x44316a.
    /// (Not decoded: what they mean; 0x43c650 reads `+0x252` into its info
    /// record, 0x4416c0 saves it.)
    pub town_fish: BTreeMap<u32, (u8, u8)>,
    /// Fractions of amber owed for repairs, per player.
    pub repair_owed: std::collections::BTreeMap<(u32, u32), f32>,
    /// Ships under a siren's song (not saved), and seconds until each
    /// siren sings.
    pub charmed: HashMap<u32, creatures::Charm>,
    pub siren_songs: HashMap<u32, f32>,
    /// Seconds each siren stays up after her song (not saved).
    pub siren_shown: HashMap<u32, f32>,
    /// Each Gorgon's cycle (not saved).
    pub gorgons: HashMap<u32, creatures::GorgonCycle>,
    /// Each sea monster's seconds to its next bite and step (not saved).
    /// Sea monsters' states (umonster.cpp).
    pub sea_monster_states: HashMap<u32, sea_monster::SeaMonster>,
    /// Places seen for a while by spells: cell, radius, seconds left (not
    /// saved).
    pub sight_points: Vec<(i32, i32, i32, f32)>,
    /// Lightning cast: where, seconds until it strikes, and its caster
    /// (a player's spell; None the game's, as a brain casts).
    pub lightning: Vec<(f32, f32, f32, Option<Owner>)>,
    /// Fireballs over the map (not saved).
    pub fireballs: Vec<fireball::Fireball>,
    /// Seconds left of the player's Flame (stronger weapons).
    pub flame: f32,
    /// Fog of war on, and what the player knows of each map cell.
    pub fog: bool,
    sight: Vec<Seen>,
    sight_timer: f32,
    /// The fog's edge codes per cell (see `sight::edge_codes`).
    dark_codes: Vec<u8>,
    fog_codes: Vec<u8>,
    /// The fog's art, if the data has it.
    pub fog_art: Option<std::rc::Rc<FogArt>>,
    pub wind: Wind,
    /// The mission's victory conditions (see `OBJECTIVES`).
    pub conditions: [bool; 11],
    /// Set once the mission is won or lost.
    pub outcome: Option<Outcome>,
    /// A Custom game with no human player is over (0x448610: command
    /// 0x100, the main menu with no end screen); the front end leaves it
    /// at once.
    pub over: bool,
    /// The mission briefing, as paragraphs.
    pub briefing: Vec<String>,
    /// The briefing as the original's "Your mission" screen lays it out
    /// (`openconquest_formats::briefing_text`).
    pub briefing_text: Vec<String>,
    /// The briefing's lines as stored (the help panel builds on them).
    pub briefing_lines: Vec<String>,
    /// The human player's temple magic.
    pub magic: Magic,
}

mod ai;
mod battle;
mod brain;
mod castq;
mod computer;
mod creatures;
mod dig;
pub mod fire;
mod gulls;
mod halt;
mod hazards;
mod hunt;
mod leviathan;
pub mod load;
mod orders;
pub mod repair;
mod sea_monster;
mod sheep;
mod shot;
mod sight;
mod sleep;
mod sparkle;
mod treasure;
mod wake;
pub use sight::{FogArt, Seen};
mod fireball;
mod flyers;
mod fort;
mod gather;
mod grapple;
mod order;
mod placement;
mod script;
mod seats;
mod spells;
mod stats;
mod story;
mod towns;
pub mod unload;
pub mod volcano;
pub(crate) mod walk_path;
mod walkers;
mod yard;
pub use brain::{Brain, EASY, HARD, NORMAL};
pub use computer::Computers;
pub use flyers::{Flyer, Nest};
pub use gather::GatherZone;
pub use halt::Batch;
pub use hazards::Hazard;
pub use leviathan::Leviathan;
pub use load::{Cargo, Entry, LoadLink};
pub use order::{Order, TickOrder};
pub use repair::{StockRepair, UnitMend};
pub use stats::Stats;
#[cfg(test)]
pub use story::goal_name;
pub use story::{Carry, Goal};
pub use towns::Towns;
pub use walkers::{anchor_walker_frames, walker_frame_ms, walker_sequence, Jam, Walker};
pub use yard::ShipOrder;
#[cfg(test)]
mod tests;
mod trade;
pub use trade::Arming;

/// A ship held up: its id, the cell ahead, whether the ship there lies
/// still (state 0) and whether it lies idle without a route, and its id.
type Held = (u32, (i32, i32), bool, bool, u32);

/// A region of 0x4a5a10 (built on its caller's stack): the box's corner
/// cell, the goal relative to it, its size, and its grid row by row (0
/// open, -1 closed).
pub(super) struct Region {
    pub x0: i32,
    pub y0: i32,
    pub goal: (i32, i32),
    pub w: i32,
    pub h: i32,
    pub grid: Vec<i16>,
}

/// Which of 0x4a6330's planners a ship's route comes from: not waited
/// with a tolerance of 0 (0x4a5740), a plain move's first plan
/// (0x4a5f30), or waited (0x4a51b0). `ToWater` is the port's own: an
/// unwaited plan to the nearest water of a goal 0x4a6330 would refuse,
/// for the callers whose own water cell is not decoded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Plan {
    Unwaited,
    ToWater,
    Order,
    Waited,
}

impl Scene {
    pub fn load(data: &GameData, caches: &mut Caches, mission: &str) -> Result<Self> {
        Self::load_at(data, caches, mission, brain::NORMAL)
    }

    /// Loads a mission, its computer players' brain playing at the given
    /// difficulty (`brain::EASY`, `NORMAL` or `HARD`).
    pub fn load_at(
        data: &GameData,
        caches: &mut Caches,
        mission: &str,
        difficulty: usize,
    ) -> Result<Self> {
        Self::load_carried(data, caches, mission, difficulty, None)
    }

    /// Loads a campaign mission continued from the one before: what that
    /// one's victory carried (`carry`) arrives before the brain starts.
    pub fn load_carried(
        data: &GameData,
        caches: &mut Caches,
        mission: &str,
        difficulty: usize,
        carry: Option<&Carry>,
    ) -> Result<Self> {
        Self::load_with(data, caches, mission, difficulty, carry, None)
    }

    /// Loads a Custom mission whose players are `sides` (the side table
    /// [game+0x1a1d] the Custom screen's Start fills, 0x53b940: every row
    /// but the disabled ones): a player-owned record (dword 1 == 1) of an
    /// owner (dwords 2, 3) not among them is never created
    /// (PlaySystemClassTy::CreateSystemObjects 0x51e810, 0x51e82b..0x51e845).
    pub fn load_custom(
        data: &GameData,
        caches: &mut Caches,
        mission: &str,
        difficulty: usize,
        sides: &[(u32, u32)],
    ) -> Result<Self> {
        Self::load_with(data, caches, mission, difficulty, None, Some(sides))
    }

    fn load_with(
        data: &GameData,
        caches: &mut Caches,
        mission: &str,
        difficulty: usize,
        carry: Option<&Carry>,
        sides: Option<&[(u32, u32)]>,
    ) -> Result<Self> {
        let archive = data.open_mission(mission)?;
        let grid = data.load_map(&archive, mission)?;
        let terrain = Terrain::compose(data, &grid, &mut caches.tiles)?;
        let cell_types: Vec<u32> = (0..grid.height)
            .flat_map(|y| (0..grid.width).map(move |x| (x, y)))
            .map(|(x, y)| cell_type(data, &grid, x, y))
            .collect();
        if !terrain.missing_tiles.is_empty() {
            eprintln!(
                "warning: {mission}: missing tiles: {}",
                terrain.missing_tiles.join(", ")
            );
        }
        let mut objs = objects::load(data, caches, &archive)?;
        if let Some(sides) = sides {
            objs.drawn.retain(|o| {
                let f = |i: usize| o.record.field(i).unwrap_or(0);
                f(1) != 1 || sides.contains(&(f(2), f(3)))
            });
        }
        // The particles' keeper (0x2bf) has nothing of its own to draw.
        if objs.unresolved.keys().any(|&c| c != 0x2bf) {
            let mut classes: Vec<_> = objs
                .unresolved
                .iter()
                .filter(|(&c, _)| c != 0x2bf)
                .collect();
            classes.sort();
            let list: Vec<String> = classes.iter().map(|(c, n)| format!("{c:#x}x{n}")).collect();
            eprintln!(
                "{mission}: {} objects drawn; not drawn yet (class x count): {}",
                objs.drawn.len(),
                list.join(" ")
            );
        }
        // The minimap: the map in its small (`s_`) tiles, 8 pixels a cell
        // (Mapa.cpp), animated colours kept as such.
        let indices: [u32; 256] = std::array::from_fn(|i| {
            if data.animated[i] {
                crate::assets::ANIMATED | i as u32
            } else {
                crate::assets::rgb(data.palette.0[i])
            }
        });
        let (mw, mh, minimap) = match Terrain::small(data, &grid, &mut caches.tiles, &indices) {
            Ok(small) => small,
            Err(_) => terrain.minimap(&palette_lut(data, 0)),
        };
        let nav = NavGrid::from_types(grid.width, grid.height, &cell_types);
        let gulls = Scene::new_gulls(nav.width, nav.height, &mut Rng(0x6011));
        let cells = (nav.width * nav.height) as usize;
        let players = players(&archive).unwrap_or_default();
        let human = players.iter().copied().find(|p| !p.computer);
        let conditions = openconquest_formats::conditions(&archive)?;
        let mut scene = Self {
            mission: mission.to_string(),
            grid,
            cell_types,
            terrain,
            objects: objs.drawn,
            nav,
            ship_orders: Vec::new(),
            yard_clocks: BTreeMap::new(),
            new_ships: Vec::new(),
            rnd_init: None,
            constructions: Vec::new(),
            standing_frames: BTreeMap::new(),
            messages: Vec::new(),
            sounds: Vec::new(),
            trainings: Vec::new(),
            summons: Vec::new(),
            levels: players
                .iter()
                .filter(|p| p.levels != Default::default())
                .map(|p| ((p.nation, p.slot), combat::Levels::starting(&p.levels)))
                .collect(),
            research: Vec::new(),
            refits: Vec::new(),
            purses: players
                .iter()
                .filter(|p| p.computer)
                .map(|p| ((p.nation, p.slot), (p.amber, p.fish)))
                .collect(),
            garrisons: Default::default(),
            amber: human.map_or(0, |p| p.amber),
            fish: human.map_or(0, |p| p.fish),
            human,
            minimap,
            minimap_size: (mw, mh),
            minimap_origin: Default::default(),
            effects: Vec::new(),
            shots: Vec::new(),
            fires: Vec::new(),
            volcanoes: HashMap::new(),
            last_attack: None,
            rng: Rng(0x1234_5678),
            shoals_gone: Default::default(),
            hazards: Default::default(),
            leviathans: Default::default(),
            shoals_shown: Default::default(),
            shoals_up: Default::default(),
            fish_art: std::rc::Rc::new([1, 2, 3].map(|size| {
                [1, 2, 3].map(|state| {
                    objects::sequence_frames(data, caches, &format!("FISH_{size}{state}"))
                        .unwrap_or_default()
                })
            })),
            default_arming: true,
            gulls,
            gull_rng: Rng(0x6011_5eed),
            droplets: Vec::new(),
            trails: true,
            trail_art: BTreeMap::new(),
            trail_clock: 0.0,
            unloaded: Vec::new(),
            sparkles: Default::default(),
            flocks: Default::default(),
            sheep_clock: 0.0,
            sheep_ticks: 0,
            sheep_art: std::rc::Rc::new(std::array::from_fn(|dir| {
                [1, 2].map(|state| {
                    objects::sequence_frames(data, caches, &format!("SHEEP_{dir}{state}"))
                        .unwrap_or_default()
                })
            })),
            plank_art: std::rc::Rc::new(std::array::from_fn(|facing| {
                objects::composite_layer_frames(data, caches, "MOST0_", facing).unwrap_or_default()
            })),
            sparkle_art: std::rc::Rc::new(
                objects::sequence_frames(data, caches, "ASTAR_").unwrap_or_default(),
            ),
            wake_clock: 0.0,
            wake_art: std::rc::Rc::new((
                objects::sequence_frames(data, caches, "TRAKS00").unwrap_or_default(),
                objects::sequence_frames(data, caches, "TRAKS14").unwrap_or_default(),
            )),
            gull_art: std::rc::Rc::new((
                objects::sequence_frames(data, caches, "GULLSPR").unwrap_or_default(),
                objects::sequence_frames(data, caches, "GULLSHD_SPR").unwrap_or_default(),
            )),
            ai_timer: 0.0,
            clock: 0.0,
            fog: true,
            fireballs: Vec::new(),
            lightning: Vec::new(),
            sight_points: Vec::new(),
            charmed: HashMap::new(),
            siren_songs: HashMap::new(),
            siren_shown: HashMap::new(),
            gorgons: HashMap::new(),
            sea_monster_states: HashMap::new(),
            mending: Vec::new(),
            unit_mends: Vec::new(),
            stock_conditions: Default::default(),
            stock_repairs: Vec::new(),
            refit_batches: BTreeMap::new(),
            tick_orders: BTreeMap::new(),
            outbox: Vec::new(),
            gather_zones: Vec::new(),
            zone_serial: 0,
            navy_lcg: None,
            ticks: 0,
            town_fish: BTreeMap::new(),
            free_casts: Default::default(),
            creature_clock: 0.0,
            wizards: BTreeMap::new(),
            pending_reefs: Vec::new(),
            pending_walkers: Vec::new(),
            pending_casts: Vec::new(),
            brain_flames: Default::default(),
            brain_brews: Vec::new(),
            story: Vec::new(),
            start_sites: Vec::new(),
            delivered: Vec::new(),
            joined: Vec::new(),
            had_argo: false,
            woken: false,
            lime_remap: {
                let mut map = std::collections::HashMap::new();
                for (i, &to) in crate::render::LIME.iter().enumerate() {
                    if data.animated[i] {
                        map.insert(
                            crate::assets::ANIMATED | i as u32,
                            crate::assets::ANIMATED | to as u32,
                        );
                    } else {
                        map.entry(crate::assets::rgb(data.palette.0[i]))
                            .or_insert(crate::assets::rgb(data.palette.0[to as usize]));
                    }
                }
                map
            },
            dug: Vec::new(),
            sold: Vec::new(),
            strait_struck: false,
            strait_checked: false,
            winning: 0,
            losing: None,
            held: Vec::new(),
            fittings: Default::default(),
            towns: Default::default(),
            loads: Vec::new(),
            permits: crate::permits::Permits::ALL,
            seats: Vec::new(),
            active: 0,
            stats: Default::default(),
            planks: Vec::new(),
            porters: Vec::new(),
            views: (0..20)
                .map(|id| openconquest_formats::mission::start_view(&archive, id / 4, id % 4))
                .collect(),
            flyers: Vec::new(),
            nests: Vec::new(),
            walkers: Vec::new(),
            jams: Vec::new(),
            throws: Vec::new(),
            lair_clocks: Default::default(),
            carried: Vec::new(),
            site_heroes: Default::default(),
            brain: None,
            brain_clock: 0.0,
            computers: computer::Computers {
                on: is_custom(mission),
                level: difficulty,
                ..Default::default()
            },
            repair_owed: Default::default(),
            flame: 0.0,
            shown: [1, 1],
            shown_clock: 0.0,
            sight: vec![Seen::Unexplored; cells],
            sight_timer: 0.0,
            dark_codes: Vec::new(),
            fog_codes: Vec::new(),
            fog_art: FogArt::load(data, &archive).ok().map(std::rc::Rc::new),
            wind: Wind::new(0x5eed_0001),
            conditions,
            outcome: None,
            over: false,
            briefing: openconquest_formats::briefing(&archive).unwrap_or_default(),
            briefing_text: openconquest_formats::briefing_text(&archive).unwrap_or_default(),
            briefing_lines: openconquest_formats::briefing_lines(&archive).unwrap_or_default(),
            magic: Magic::default(),
        };
        // The mission's ships that cannot lie where they are put are
        // removed as they are made (0x4dc787).
        scene.drop_misplaced_ships();
        // What buildings, lairs and monsters hold, dropped when they fall.
        scene.held = scene
            .objects
            .iter()
            .flat_map(|o| {
                crate::objects::inventory(&o.record)
                    .into_iter()
                    .map(move |t| (o.id, t))
            })
            .collect();
        // Settlements and towns start with the marines and weapons their
        // records hold (fields 14-22, one count per item).
        for o in &scene.objects {
            if matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)) {
                if let Some(counts) = o.record.fields.get(14..23) {
                    let mut stock = [0u32; 9];
                    for (s, &n) in stock.iter_mut().zip(counts) {
                        *s = n.min(99);
                    }
                    if stock.iter().any(|&n| n > 0) {
                        scene.garrisons.insert(o.id, stock);
                    }
                }
                // And the rams (fields 8-10: so many of 3 (none), 4, 5) and
                // armour (11-13: levels 0, 1, 2) their store holds, each
                // whole (0x441ba0 message 2, 0x441d3f-0x441e12).
                let f = |k: usize| o.record.fields.get(k).copied().unwrap_or(0);
                let fittings: load::Fittings = (
                    (0..3u8)
                        .flat_map(|l| std::iter::repeat_n(l, f(11 + l as usize) as usize))
                        .collect(),
                    (0..3u8)
                        .flat_map(|l| std::iter::repeat_n(l, f(8 + l as usize) as usize))
                        .collect(),
                );
                if !fittings.0.is_empty() || !fittings.1.is_empty() {
                    scene.fittings.insert(o.id, fittings);
                }
            }
        }
        // The campaign's missions are judged by their story.
        if is_campaign(mission) {
            scene.permits = crate::permits::Permits::campaign(mission[4..].parse().unwrap_or(0));
            // Continued, the Argo, the town and the heroes the last victory
            // carried arrive (0x522d50); picked from the list, the
            // campaign's heroes so far wait at the start.
            let arrived = carry.is_some_and(|c| scene.arrive(c, data, caches));
            // Picked from the list without a town of its own, a bare one
            // where the carried town would stand; the heroes wait there.
            let bare = if arrived {
                None
            } else {
                scene.carried = story::carried_heroes(data, mission);
                scene.bare_town(data, caches)
            };
            scene.start_sites = scene.own_sites();
            // After miss002 they sail in the Argo; before, or without a
            // ship, they wait at the starting settlement.
            let argo = if mission > "miss002" && !arrived {
                scene.ready_argo(data, caches)
            } else {
                None
            };
            // The Argo moves in the list (spawn_ship sorts the objects).
            let argo = argo
                .and_then(|i| scene.objects.get(i).map(|o| o.id))
                .map(|id| {
                    if !arrived {
                        let i = scene.object_index(id).unwrap();
                        scene.list_treasures(Some(i), data, caches);
                    }
                    scene.object_index(id).unwrap()
                });
            if !arrived && argo.is_none() {
                scene.list_treasures(None, data, caches);
            }
            if !arrived && !scene.carried.is_empty() {
                let human = scene.human.map(|h| (h.nation, h.slot));
                let first_ship = || {
                    scene
                        .objects
                        .iter()
                        .position(|o| o.ship.is_some() && o.owner == human)
                };
                match (argo, scene.start_sites.first()) {
                    (_, Some(&site)) if bare == Some(site) => {
                        scene.site_heroes.insert(site, scene.carried.clone());
                    }
                    (Some(i), _) => {
                        scene.objects[i].ship.as_mut().unwrap().heroes = scene.carried.clone()
                    }
                    (None, Some(&site)) => {
                        scene.site_heroes.insert(site, scene.carried.clone());
                    }
                    (None, None) => {
                        if let Some(i) = first_ship() {
                            scene.objects[i].ship.as_mut().unwrap().heroes = scene.carried.clone();
                        }
                    }
                }
            }
            scene.place_scripted(difficulty, data, caches);
            let heroes = openconquest_formats::mission::heroes(&archive).unwrap_or_default();
            scene.story = scene.story_goals(heroes);
            let wanted: Vec<u8> = (0..9u8).filter(|&h| heroes[h as usize]).collect();
            scene.hide_hero_items(&wanted, data, caches);
            // The start's message 0x8d1 reaches the map's objects before
            // the brain made after them.
            scene.towns_start();
            scene.start_brain(data, caches, difficulty);
            scene.start_sites = scene.own_sites();
        }
        if !is_campaign(mission) {
            scene.towns_start();
        }
        scene.start_nests(data, caches);
        for i in 0..scene.objects.len() {
            scene.refresh_amber(i, data, caches);
        }
        scene.update_sight();
        Ok(scene)
    }

    /// The state to save (see `save`).
    pub fn save(&self) -> SaveGame {
        SaveGame {
            version: save::VERSION,
            mission: self.mission.clone(),
            amber: self.amber,
            fish: self.fish,
            objects: self
                .objects
                .iter()
                .map(|o| SavedObject {
                    id: o.id,
                    class_id: o.record.class_id,
                    fields: o.record.fields.clone(),
                    x: o.x,
                    y: o.y,
                    label: o.label.clone(),
                    owner: o.owner,
                    ship: o.ship.clone(),
                    deposit: o.deposit,
                    dormant: o.dormant,
                    combat: o.combat.clone(),
                })
                .collect(),
            ship_orders: self.ship_orders.clone(),
            yard_clocks: self.yard_clocks.iter().map(|(&k, &v)| (k, v)).collect(),
            rnd_init: self.rnd_init,
            constructions: self.constructions.clone(),
            standing_frames: self
                .standing_frames
                .iter()
                .map(|(&k, v)| (k, v.clone()))
                .collect(),
            trainings: self.trainings.clone(),
            garrisons: self.garrisons.clone(),
            purses: self.purses.iter().map(|(k, v)| (*k, *v)).collect(),
            summons: self.summons.clone(),
            magic: self.magic.clone(),
            wind: self.wind.clone(),
            outcome: self.outcome,
            clock: self.clock,
            ai_timer: self.ai_timer,
            rng: self.rng.clone(),
            shoals_gone: self.shoals_gone.clone(),
            hazards: self.hazards.clone(),
            leviathans: self.leviathans.clone(),
            shoals_shown: self.shoals_shown.clone(),
            shoals_up: self.shoals_up.clone(),
            default_arming: self.default_arming,
            research_levels: self.levels.iter().map(|(k, v)| (*k, *v)).collect(),
            sight: self.sight.iter().map(|&s| s as u8).collect(),
            research: self.research.clone(),
            mending: self.mending.clone(),
            unit_mends: self.unit_mends.clone(),
            stock_conditions: self.stock_conditions.clone(),
            stock_repairs: self.stock_repairs.clone(),
            refit_batches: self.refit_batches.clone(),
            tick_orders: self.tick_orders.clone(),
            outbox: self.outbox.clone(),
            gather_zones: self.gather_zones.clone(),
            zone_serial: self.zone_serial,
            navy_lcg: self.navy_lcg,
            ticks: self.ticks,
            town_fish: self.town_fish.clone(),
            free_casts: self
                .free_casts
                .iter()
                .map(|(&s, &n)| (s as u32, n))
                .collect(),
            start_sites: self.start_sites.clone(),
            delivered: self.delivered.clone(),
            joined: self.joined.clone(),
            had_argo: self.had_argo,
            woken: self.woken,
            dug: self
                .dug
                .iter()
                .map(|&(l, x, y, t)| (l, x, y, t.variant, t.tileset))
                .collect(),
            strait_struck: self.strait_struck,
            sold: self.sold.clone(),
            strait_checked: self.strait_checked,
            winning: self.winning,
            losing: self.losing,
            held: self.held.clone(),
            fittings: self.fittings.iter().map(|(k, v)| (*k, v.clone())).collect(),
            towns: Some(self.towns.clone()),
            loads: self.loads.clone(),
            permits: Some(self.permits),
            stats: self.stats.clone(),
            flyers: self.flyers.clone(),
            nests: self.nests.clone(),
            walkers: self.walkers.clone(),
            jams: self.jams.clone(),
            lair_clocks: self.lair_clocks.clone(),
            volcanoes: self.volcanoes.clone(),
            brain_flames: self.brain_flames.iter().map(|(&k, &v)| (k, v)).collect(),
            brain_brews: self.brain_brews.clone(),
            planks: self.saved_planks().0,
            porters: self.saved_planks().1,
            unloaded: self.unloaded.clone(),
            fires: self.saved_fires(),
            refits: self.refits.clone(),
            flame: self.flame,
            repair_owed: self.repair_owed.iter().map(|(&k, &v)| (k, v)).collect(),
            creature_clock: self.creature_clock,
            gull_rng: Some(self.gull_rng.clone()),
            last_attack: self.last_attack,
            site_heroes: self
                .site_heroes
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            brain: self.brain.clone(),
            computers: Some(self.computers.clone()),
            human: self.human.map(|h| (h.nation, h.slot)),
            battle: Battle {
                shots: self.shots.iter().map(|s| s.saved()).collect(),
                fireballs: self.fireballs.iter().map(|f| f.saved()).collect(),
                lightning: self.lightning.iter().map(|l| (l.0, l.1, l.2)).collect(),
                lightning_by: self.lightning.iter().map(|l| l.3).collect(),
                sight_points: self.sight_points.clone(),
                charmed: self.charmed.iter().map(|(k, v)| (*k, v.clone())).collect(),
                siren_songs: self.siren_songs.iter().map(|(k, v)| (*k, *v)).collect(),
                siren_shown: self.siren_shown.iter().map(|(k, v)| (*k, *v)).collect(),
                gorgons: self.gorgons.iter().map(|(k, v)| (*k, *v)).collect(),
                sea_monsters: Vec::new(),
                sea_monster_states: self
                    .sea_monster_states
                    .iter()
                    .map(|(k, v)| (*k, v.clone()))
                    .collect(),
                wizards: self.wizards.iter().map(|(k, v)| (*k, v.clone())).collect(),
            },
            game_id: None,
        }
    }

    /// Rebuilds a saved game: the mission from the game data, its objects
    /// from their saved records, then the state they had.
    pub fn restore(data: &GameData, caches: &mut Caches, save: &SaveGame) -> Result<Self> {
        let mut scene = Self::load(data, caches, &save.mission)?;
        scene.objects.clear();
        for s in &save.objects {
            let rec = ObjectRecord {
                id: s.id,
                class_id: s.class_id,
                fields: s.fields.clone(),
            };
            let Some(mut obj) = objects::resolve(data, caches, &rec)? else {
                continue;
            };
            obj.x = s.x;
            obj.y = s.y;
            obj.label = s.label.clone();
            obj.owner = s.owner;
            obj.ship = s.ship.clone();
            obj.deposit = s.deposit;
            obj.dormant = s.dormant;
            obj.combat = s.combat.clone();
            scene.objects.push(obj);
        }
        // Kept in the saved order: the drawing order as it stood, which
        // also decides who goes first where ships meet.
        scene.amber = save.amber;
        scene.fish = save.fish;
        scene.ship_orders = save.ship_orders.clone();
        scene.yard_clocks = save.yard_clocks.iter().copied().collect();
        scene.rnd_init = save.rnd_init;
        scene.constructions = save.constructions.clone();
        for k in 0..scene.constructions.len() {
            let site = scene.constructions[k].site;
            let n = scene
                .object_index(site)
                .map_or(0, |i| scene.objects[i].frames.len() as u32);
            scene.constructions[k].upgrade_save(n);
        }
        scene.standing_frames = save.standing_frames.iter().cloned().collect();
        scene.trainings = save.trainings.clone();
        scene.trainings.iter_mut().for_each(Training::upgrade_save);
        scene.garrisons = save.garrisons.clone();
        scene.purses = save.purses.iter().copied().collect();
        if let Some(owner) = save.human {
            let players = openconquest_formats::players(&data.open_mission(&save.mission)?)?;
            if let Some(p) = players.iter().find(|p| (p.nation, p.slot) == owner) {
                scene.human = Some(PlayerInfo {
                    computer: false,
                    ..*p
                });
            }
        }
        scene.summons = save.summons.clone();
        scene.magic = save.magic.clone();
        scene.wind = save.wind.clone();
        scene.outcome = save.outcome;
        scene.clock = save.clock;
        scene.ai_timer = save.ai_timer;
        scene.rng = save.rng.clone();
        scene.shoals_gone = save.shoals_gone.clone();
        scene.hazards = save.hazards.clone();
        scene.leviathans = save.leviathans.clone();
        scene.shoals_shown = save.shoals_shown.clone();
        scene.shoals_up = save.shoals_up.clone();
        scene.default_arming = save.default_arming;
        scene.levels = save.research_levels.iter().copied().collect();
        if save.sight.len() == scene.sight.len() {
            for (cell, &v) in scene.sight.iter_mut().zip(&save.sight) {
                *cell = match v {
                    0 => Seen::Unexplored,
                    1 => Seen::Explored,
                    _ => Seen::InSight,
                };
            }
        }
        scene.update_sight();
        scene.research = save.research.clone();
        scene.mending = save.mending.clone();
        scene.unit_mends = save.unit_mends.clone();
        scene.stock_conditions = save.stock_conditions.clone();
        scene.stock_repairs = save.stock_repairs.clone();
        scene.refit_batches = save.refit_batches.clone();
        scene.tick_orders = save.tick_orders.clone();
        scene.outbox = save.outbox.clone();
        scene.gather_zones = save.gather_zones.clone();
        scene.zone_serial = save.zone_serial;
        scene.navy_lcg = save.navy_lcg;
        scene.ticks = save.ticks;
        scene.town_fish = save.town_fish.clone();
        scene.delivered = save.delivered.clone();
        scene.joined = save.joined.clone();
        scene.had_argo = save.had_argo;
        scene.woken = save.woken;
        scene.strait_struck = save.strait_struck;
        scene.sold = save.sold.clone();
        for site in scene.sold.clone() {
            scene.show_sold(site, data, caches);
        }
        scene.strait_checked = save.strait_checked;
        let dug: Vec<_> = save
            .dug
            .iter()
            .map(|&(l, x, y, variant, tileset)| {
                let t = openconquest_formats::map::TileRef { variant, tileset };
                scene.grid.set(l, x, y, t);
                (l, x, y, t)
            })
            .collect();
        scene.after_dig(&dug, data, caches);
        scene.dug = dug;
        scene.mark_hazards();
        scene.winning = save.winning;
        scene.losing = save.losing;
        scene.held = save.held.clone();
        scene.fittings = save.fittings.iter().cloned().collect();
        // The towns' links as saved; a save from before them has them made
        // again as at the start (0x8d1), as the original's own load does
        // (its saves keep neither, 0x4277c0, 0x4416c0).
        match &save.towns {
            Some(t) => scene.towns = t.clone(),
            None => scene.towns_start(),
        }
        scene.loads = save.loads.clone();
        scene.flyers = save.flyers.clone();
        scene.stats = save.stats.clone();
        for i in 0..scene.objects.len() {
            scene.refresh_amber(i, data, caches);
        }
        scene.nests = save.nests.clone();
        scene.walkers = save.walkers.clone();
        scene.jams = save.jams.clone();
        scene.lair_clocks = save.lair_clocks.clone();
        scene.volcanoes = save.volcanoes.clone();
        scene.brain_flames = save.brain_flames.iter().copied().collect();
        scene.brain_brews = save.brain_brews.clone();
        scene.unloaded = save.unloaded.clone();
        scene.refits = save.refits.clone();
        scene.flame = save.flame;
        scene.repair_owed = save.repair_owed.iter().copied().collect();
        scene.creature_clock = save.creature_clock;
        if let Some(r) = &save.gull_rng {
            scene.gull_rng = r.clone();
        }
        scene.last_attack = save.last_attack;
        scene.restore_planks(&save.planks, &save.porters, data, caches);
        scene.restore_fires(&save.fires, data, caches);
        if let Some(p) = save.permits {
            scene.permits = p;
        }
        scene.site_heroes = save.site_heroes.iter().cloned().collect();
        if save.brain.is_some() {
            scene.brain = save.brain.clone();
            // A brain saved with fewer members lines up with the mission's
            // fleets again.
            if let Some(b) = scene.brain.as_mut() {
                b.upgrade();
            }
        }
        if let Some(c) = &save.computers {
            scene.computers = c.clone();
        }
        let b = &save.battle;
        scene.shots = b
            .shots
            .iter()
            .map(|s| shot::Shot::restored(s, data, caches))
            .collect();
        scene.fireballs = b
            .fireballs
            .iter()
            .map(|f| fireball::Fireball::restored(f, data, caches))
            .collect();
        scene.lightning = b
            .lightning
            .iter()
            .enumerate()
            .map(|(i, l)| (l.0, l.1, l.2, b.lightning_by.get(i).copied().flatten()))
            .collect();
        scene.sight_points = b.sight_points.clone();
        scene.charmed = b.charmed.iter().cloned().collect();
        scene.siren_songs = b.siren_songs.iter().copied().collect();
        scene.siren_shown = b.siren_shown.iter().copied().collect();
        scene.gorgons = b.gorgons.iter().copied().collect();
        scene.sea_monster_states = b.sea_monster_states.iter().cloned().collect();
        scene.wizards = b.wizards.iter().cloned().collect();
        scene.refresh_sight();
        if !save.start_sites.is_empty() {
            scene.start_sites = save.start_sites.clone();
        }
        scene.free_casts = save
            .free_casts
            .iter()
            .filter_map(|&(s, n)| Some((Spell::of_index(s)?, n)))
            .collect();
        // Buildings look as they stood: going up, damaged or in ruins.
        let buildings: Vec<(u32, bool, bool)> = scene
            .objects
            .iter()
            .filter(|o| {
                matches!(
                    o.class,
                    ObjectClass::Building(_) | ObjectClass::Barbarian(_)
                )
            })
            .map(|o| {
                (
                    o.id,
                    o.combat.is_none() && o.owner.is_none(),
                    scene.construction_at(o.id).is_some(),
                )
            })
            .collect();
        for (id, ruined, building) in buildings {
            let i = scene.object_index(id).unwrap();
            let state = if ruined {
                Some(BuildingState::Ruins)
            } else if building {
                Some(BuildingState::Construction)
            } else {
                None
            };
            match state {
                Some(state) => {
                    if let Ok(frames) =
                        objects::building_frames(data, caches, &scene.objects[i].record, state)
                    {
                        if !frames.is_empty() {
                            scene.objects[i].frames = frames;
                        }
                    }
                    if ruined {
                        scene.objects[i].picture = None;
                        // Fallen before the mission: the last frame.
                        scene.objects[i].progress = Some(1.0);
                    }
                }
                None => scene.refresh_building(id, data, caches),
            }
        }
        Ok(scene)
    }

    /// Whether the human player owns `obj`.
    pub fn is_own(&self, obj: &MapObject) -> bool {
        match (self.human, obj.owner) {
            (Some(h), Some(owner)) => (h.nation, h.slot) == owner,
            _ => false,
        }
    }

    /// The cells whose occupants the pointer at world pixel `(x, y)` tries
    /// (0x49a580 with the main view's cursor `+0xbd` 1): its own cell, then
    /// those about it in the list 0x499ff0 builds for its place by the
    /// map's edges (0x49a1d0): inside, north-west, north, north-east, east,
    /// south-east, south, south-west, west; on the edges and in the corners
    /// the cells on the map, from east (top edge, top-left corner), south
    /// (right edge, top-right), west (bottom edge, bottom-right) or north
    /// (left edge, bottom-left) round that way.
    pub fn pick_cells(&self, x: i32, y: i32) -> Vec<(i32, i32)> {
        let t = TILE_SIZE as i32;
        let (cx, cy) = (x.div_euclid(t), y.div_euclid(t));
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        const NW: (i32, i32) = (-1, -1);
        const N: (i32, i32) = (0, -1);
        const NE: (i32, i32) = (1, -1);
        const E: (i32, i32) = (1, 0);
        const SE: (i32, i32) = (1, 1);
        const S: (i32, i32) = (0, 1);
        const SW: (i32, i32) = (-1, 1);
        const W: (i32, i32) = (-1, 0);
        let (left, right, top, bottom) = (cx == 0, cx == w - 1, cy == 0, cy == h - 1);
        let about: &[(i32, i32)] = match (left, right, top, bottom) {
            (true, _, true, _) => &[E, SE, S],
            (true, _, _, true) => &[N, NE, E],
            (true, ..) => &[N, NE, E, SE, S],
            (_, true, true, _) => &[S, SW, W],
            (_, true, _, true) => &[W, NW, N],
            (_, true, ..) => &[S, SW, W, NW, N],
            (.., true, _) => &[E, SE, S, SW, W],
            (.., true) => &[W, NW, N, NE, E],
            _ => &[NW, N, NE, E, SE, S, SW, W],
        };
        std::iter::once((cx, cy))
            .chain(about.iter().map(|&(dx, dy)| (cx + dx, cy + dy)))
            .collect()
    }

    /// Whether the pointer at world pixel `(x, y)` picks the land
    /// creatures' jam on `cell` (its vt+8, 0x415c70: one of its members,
    /// not dying, with the point on its picture, 0x411b50), giving the jam
    /// as its first member (orders naming a jam reach that one, 0x415df0).
    pub fn jam_at(&self, cell: (i32, i32), x: i32, y: i32) -> Option<u32> {
        let jam = self.jams.iter().find(|j| j.cell == cell)?;
        let hit = jam.members.iter().any(|&m| {
            self.walkers
                .iter()
                .find(|w| w.id == m)
                .is_some_and(|w| walkers::on_walker(self, w, x, y))
        });
        hit.then(|| jam.members.first().copied()).flatten()
    }

    /// Index of the object with mission id `id`.
    pub fn object_index(&self, id: u32) -> Option<usize> {
        self.objects.iter().position(|o| o.id == id)
    }

    /// Orders ship `id` to sail to the world position `(x, y)`. Returns
    /// false if the ship cannot get there.
    pub fn order_move(&mut self, id: u32, x: f32, y: f32) -> bool {
        let Some(i) = self.object_index(id) else {
            return false;
        };
        // A ship being built takes no move (0x4ab720 skips its events,
        // 0x4ab250 drops them without capability 0x400).
        if self.being_built(id) {
            return false;
        }
        // The exact route (0x55ca10; to a cell not water 0x55ba50 within
        // 1). None, the ship keeps the route it had and waits: one more of
        // its wait (+0x591), and 25 ticks on it plans again, waited, as
        // near as it gets (0x4a6330 returns 0xff, then 0x4a6150).
        let t = TILE_SIZE as f32;
        let (cx, cy) = ((x / t).floor() as i32, (y / t).floor() as i32);
        if cx < 0 || cy < 0 || cx >= self.nav.width as i32 || cy >= self.nav.height as i32 {
            return false;
        }
        let here = (self.objects[i].x, self.objects[i].y);
        let path = self.plan_route_order(i, here, x, y);
        let Some(ship) = self.objects[i].ship.as_mut() else {
            return false;
        };
        ship.planned = 0.0;
        match path {
            Some(path) => {
                ship.path = path;
                ship.pending = None;
                ship.waited = 0.0;
            }
            None => {
                ship.pending = Some((x, y));
                ship.waited = crate::clock::TICK;
            }
        }
        ship.task = None;
        // A plain move carries priority 0 (0x4cf070).
        ship.priority = 0;
        ship.patience = objects::PATIENCE;
        ship.order_tag = 0;
        true
    }

    /// Adds a leg from the end of ship `id`'s route (or where it is) to the
    /// world point `(x, y)`: a waypoint.
    pub fn order_waypoint(&mut self, id: u32, x: f32, y: f32) -> bool {
        let Some(i) = self.object_index(id) else {
            return false;
        };
        if self.being_built(id) {
            return false;
        }
        let Some(ship) = self.objects[i].ship.as_ref() else {
            return false;
        };
        let from = ship
            .path
            .back()
            .copied()
            .unwrap_or((self.objects[i].x, self.objects[i].y));
        let Some(leg) = self.plan_route_to_water_from(i, from, x, y) else {
            return false;
        };
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path.extend(leg);
        ship.task = None;
        true
    }

    /// A route for object `i` to the world point `(x, y)` (0x4a6330 not
    /// waited): none to a cell 0x499990 refuses (not water, or layer-1
    /// type 4, 5 or 6), the original's callers choosing their own water
    /// cell.
    fn plan_route(&self, i: usize, x: f32, y: f32) -> Option<VecDeque<(f32, f32)>> {
        let obj = &self.objects[i];
        self.plan_route_from(i, (obj.x, obj.y), x, y)
    }

    /// The same, for object `i` setting out from the world point `start`.
    fn plan_route_from(
        &self,
        i: usize,
        start: (f32, f32),
        x: f32,
        y: f32,
    ) -> Option<VecDeque<(f32, f32)>> {
        self.plan_route_avoiding(i, start, x, y, &HashSet::new())
    }

    /// The same, keeping out of the cells in `avoid`: a plan not waited
    /// with a tolerance of 0 (0x4a6330 -> 0x4a5740), as the ships'
    /// behaviours make (0x4a6270).
    fn plan_route_avoiding(
        &self,
        i: usize,
        start: (f32, f32),
        x: f32,
        y: f32,
        avoid: &HashSet<(i32, i32)>,
    ) -> Option<VecDeque<(f32, f32)>> {
        self.plan_route_tolerant(i, start, x, y, avoid, Plan::Unwaited)
    }

    /// The same to the nearest water of `(x, y)` (`nav.nearest_water`): the
    /// port's own, for callers whose water cell is not decoded (not
    /// decoded: the scout, patrol, zone and convoy orders' goals, a
    /// waypoint, a treasure's or hero's house, a building or creature
    /// attacked on land, an amber deposit and a port without a moorage
    /// front; see "Ships' routes" in FORMATS.md).
    pub(super) fn plan_route_to_water(
        &self,
        i: usize,
        x: f32,
        y: f32,
    ) -> Option<VecDeque<(f32, f32)>> {
        let obj = &self.objects[i];
        self.plan_route_to_water_from(i, (obj.x, obj.y), x, y)
    }

    /// The same, setting out from the world point `start`.
    fn plan_route_to_water_from(
        &self,
        i: usize,
        start: (f32, f32),
        x: f32,
        y: f32,
    ) -> Option<VecDeque<(f32, f32)>> {
        let none = HashSet::new();
        self.plan_route_tolerant(i, start, x, y, &none, Plan::ToWater)
    }

    /// A plain move's first plan (0x4a5f30 -> 0x4a6330, not waited): to
    /// water (0x4999e0) as `plan_route_avoiding`; to any other cell
    /// 0x4a51b0 with a tolerance of 1, the nearest reached cell within 1
    /// of it. A goal it cannot reach is planned again 25 ticks later,
    /// waited (`Scene::plan_pending`).
    fn plan_route_order(
        &self,
        i: usize,
        start: (f32, f32),
        x: f32,
        y: f32,
    ) -> Option<VecDeque<(f32, f32)>> {
        let none = HashSet::new();
        self.plan_route_tolerant(i, start, x, y, &none, Plan::Order)
    }

    /// Ships with a plain move they found no route for (`Ship::pending`)
    /// plan again 25 ticks after the last (0x4a6150: the ship lying still
    /// counts as changed; `+0x57b`, `+0x57f`), waited, with its wait
    /// (0x4a6330 -> 0x4a51b0, 0x55ba50 with a tolerance of 0 to water, 1
    /// to anything else): a route of two cells or more is taken; else
    /// its wait grows by one (0x4a6330's 0xff and 2 both count it) and it
    /// tries again. At a wait of 150, or on the goal's cell, the move is
    /// given up (0x4a618d). A ship given a task meanwhile drops it.
    fn plan_pending(&mut self, dt: f32) {
        let t = TILE_SIZE as f32;
        let every = crate::clock::ticks(25);
        for i in 0..self.objects.len() {
            let o = &self.objects[i];
            let Some(s) = o.ship.as_ref() else { continue };
            let Some(to) = s.pending else { continue };
            if s.task.is_some() {
                self.objects[i].ship.as_mut().unwrap().pending = None;
                continue;
            }
            if s.planned + dt < every {
                continue;
            }
            let here = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
            let goal = ((to.0 / t).floor() as i32, (to.1 / t).floor() as i32);
            if s.waited >= crate::clock::ticks(150) || here == goal {
                let s = self.objects[i].ship.as_mut().unwrap();
                (s.pending, s.waited) = (None, 0.0);
                continue;
            }
            let cell = |&(x, y): &(f32, f32)| ((x / t).floor() as i32, (y / t).floor() as i32);
            let route = self.replan(i, to).filter(|r| {
                let first = r.front().map(cell);
                r.iter().any(|p| Some(cell(p)) != first)
            });
            let s = self.objects[i].ship.as_mut().unwrap();
            s.planned = 0.0;
            match route {
                Some(path) => {
                    (s.path, s.pending) = (path, None);
                }
                None => s.waited += crate::clock::TICK,
            }
        }
    }

    /// A waited plan (0x4a6330 -> 0x4a51b0 with a plain move's arguments):
    /// the goal left closed if `avoid` names it, the route 0x55ba50 with a
    /// tolerance of 0 to water (0x4999e0), 1 to anything else: a goal it
    /// cannot reach gives way to the nearest cell it can.
    fn plan_route_near(
        &self,
        i: usize,
        start: (f32, f32),
        x: f32,
        y: f32,
        avoid: &HashSet<(i32, i32)>,
    ) -> Option<VecDeque<(f32, f32)>> {
        self.plan_route_tolerant(i, start, x, y, avoid, Plan::Waited)
    }

    fn plan_route_tolerant(
        &self,
        i: usize,
        start: (f32, f32),
        x: f32,
        y: f32,
        avoid: &HashSet<(i32, i32)>,
        plan: Plan,
    ) -> Option<VecDeque<(f32, f32)>> {
        let t = TILE_SIZE as f32;
        let obj = &self.objects[i];
        let start_point = start;
        let from = ((start.0 / t).floor() as i32, (start.1 / t).floor() as i32);
        let target = ((x / t).floor() as i32, (y / t).floor() as i32);
        let on_map = |(x, y): (i32, i32)| {
            x >= 0 && y >= 0 && x < self.nav.width as i32 && y < self.nav.height as i32
        };
        // A plain move plans to the cell itself, with a tolerance of 1 if
        // it is not water (0x4a5f30, 0x4999e0). Not waited, 0x4a6330 makes
        // no plan to a cell 0x499990 refuses (after the goal-is-its-own-cell
        // test, which makes none either).
        let sea = self.nav.is_sea(target.0, target.1);
        let to = match plan {
            Plan::Unwaited if sea => target,
            Plan::Unwaited => return None,
            Plan::ToWater => self.nav.nearest_water(target.0, target.1)?,
            Plan::Order | Plan::Waited if on_map(target) => target,
            _ => return None,
        };
        if plan == Plan::Order && !sea {
            return self.plan_route_tolerant(i, start, x, y, avoid, Plan::Waited);
        }
        // Setting out from where it is, a ship between cells plans from
        // the cell it is entering (0x4a6330: 0x4e0b50 false, `+0x5b6`),
        // sailing on into it; else from its own cell (`+0x5a3`, `+0x5a7`).
        // Its goal its own cell, 0x4a6330 makes no plan, and a route of
        // one cell is none: the ship sails on as it was.
        let entering = obj
            .ship
            .as_ref()
            .filter(|_| start == (obj.x, obj.y))
            .and_then(|s| (!s.plans_from_own_cell()).then_some(s.entering));
        if entering.is_some() && to == from {
            return None;
        }
        let from = entering.unwrap_or(from);
        // A ship left on a non-water cell (e.g. placed at a dock) starts from
        // the nearest water.
        let start = if self.nav.is_water(from.0, from.1) {
            from
        } else {
            self.nav.nearest_water(from.0, from.1)?
        };
        let deep = obj
            .ship
            .as_ref()
            .is_some_and(|s| crate::objects::deep_draught(s.kind));
        // Whirlpools and reefs hold their cells while not under the water
        // (the landscape's nibble 1, 0x430320 / 0x447b80): routes keep off
        // them.
        let traps: HashSet<(i32, i32)> = self.hazard_cells().into_keys().collect();
        // The ship's own cell is never closed to it (0x4a4e20 and 0x4a7290
        // pass over cells it is in).
        let base = |c: (i32, i32)| self.nav.passable(c.0, c.1, deep) && !traps.contains(&c);
        let cells = if plan == Plan::Waited {
            // 0x4a51b0: no route from a closed start; the goal stays as the
            // plan's grid has it.
            let shut = |c: (i32, i32)| (c != start && avoid.contains(&c)) || traps.contains(&c);
            if !base(start) {
                return None;
            }
            let tol = if sea { 0 } else { 1 };
            self.nav.find_path_near(start, to, deep, shut, tol)?
        } else {
            // 0x4a5740: the goal opened again as the ships' grid has it;
            // closed there, 0x55ba50 with a tolerance of 1, else the exact
            // route 0x55ca10.
            let shut =
                |c: (i32, i32)| (c != start && c != to && avoid.contains(&c)) || traps.contains(&c);
            if base(to) {
                self.nav.find_path_avoiding(start, to, deep, shut)?
            } else {
                self.nav.find_path_near(start, to, deep, shut, 1)?
            }
        };
        if entering.is_some() && cells.is_empty() {
            return None;
        }
        let mut points: Vec<(f32, f32)> = cells
            .into_iter()
            .map(|(cx, cy)| (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0))
            .collect();
        // Ships keep to the grid: cell centre to cell centre, straight or
        // diagonal, the first leg to the centre of the cell they are in.
        let here = (start.0 as f32 * t + t / 2.0, start.1 as f32 * t + t / 2.0);
        if (here.0 - start_point.0).hypot(here.1 - start_point.1) > 0.5 {
            points.insert(0, here);
        }
        Some(points.into())
    }

    /// 0x4a6330 with a region (0x4a5a10, ShipClassTy::GetWayNew): ship
    /// `i`'s plan to `goal` inside `region` only. 0x4a6330's checks first:
    /// no plan to its own cell, nor to one 0x499990(cell, 0) refuses (not
    /// waited). From the cell 0x4a6330 plans from (the one it is entering
    /// while between cells), relative to the box with no bounds test: on
    /// a closed cell no route; the region's goal open, the exact route
    /// 0x55ca10, else 0x55ba50 with a tolerance of 1, both with the
    /// heading 0 and none for an end off the box; the entries moved back
    /// by the box's corner (`x0`, `y0`). The route then as any other.
    pub(super) fn plan_region(
        &self,
        i: usize,
        goal: (i32, i32),
        region: &Region,
    ) -> Option<VecDeque<(f32, f32)>> {
        let t = TILE_SIZE as f32;
        let obj = &self.objects[i];
        let own = ((obj.x / t).floor() as i32, (obj.y / t).floor() as i32);
        if goal == own || !self.nav.is_sea(goal.0, goal.1) {
            return None;
        }
        let entering = obj
            .ship
            .as_ref()
            .and_then(|s| (!s.plans_from_own_cell()).then_some(s.entering));
        let start = entering.unwrap_or(own);
        let (w, h) = (region.w, region.h);
        let (rx, ry) = (start.0 - region.x0, start.1 - region.y0);
        let (gx, gy) = region.goal;
        let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
        // Off the box the original reads past its grid; the finders give
        // no route for such a start, taken here at once.
        if !inside(rx, ry) || region.grid[(ry * w + rx) as usize] == -1 {
            return None;
        }
        let mut m = region.grid.clone();
        let route = if inside(gx, gy) && m[(gy * w + gx) as usize] != -1 {
            walk_path::route_exact(&mut m, w, h, (rx, ry), (gx, gy), 0)?
        } else {
            walk_path::route(&mut m, w, h, (rx, ry), (gx, gy), 0, 1)?.0
        };
        let route: Vec<(i32, i32, u8)> = route
            .into_iter()
            .map(|(x, y, d)| (x + region.x0, y + region.y0, d))
            .collect();
        let cells = crate::nav::cells_after(&route);
        if entering.is_some() && cells.is_empty() {
            return None;
        }
        let mut points: Vec<(f32, f32)> = cells
            .into_iter()
            .map(|(cx, cy)| (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0))
            .collect();
        let here = (start.0 as f32 * t + t / 2.0, start.1 as f32 * t + t / 2.0);
        if (here.0 - obj.x).hypot(here.1 - obj.y) > 0.5 {
            points.insert(0, here);
        }
        Some(points.into())
    }

    /// Ship `mover`, held up by ship `still` lying in its way, pushes it
    /// aside (0x4a3a70, 0x4b4b50) once it has waited out its patience, if
    /// both are the same player's and `still`'s priority is the lower:
    /// `still` sails to a cell about it it may step into (0x499aa0), drawn
    /// with its own generator, or failing one, into a cell of another of
    /// theirs of lower priority (which is pushed in turn), taking
    /// `mover`'s priority (`Scene::aside_cell`). The patience triples.
    /// True if it pushed.
    fn push_aside(&mut self, still: u32, mover: u32) -> bool {
        let t = TILE_SIZE as f32;
        let (Some(k), Some(m)) = (self.object_index(still), self.object_index(mover)) else {
            return false;
        };
        let (o, mo) = (&self.objects[k], &self.objects[m]);
        let (Some(s), Some(ms)) = (o.ship.as_ref(), mo.ship.as_ref()) else {
            return false;
        };
        // Ships busy where they lie (collecting, unloading, grappled,
        // ramming) stay.
        let busy = s.grapple.is_some()
            || s.ram_run.is_some()
            || s.build.is_some()
            || matches!(
                s.task,
                Some(Task::Gather {
                    phase: objects::GatherPhase::Collecting | objects::GatherPhase::Unloading(_),
                    ..
                })
            );
        if o.owner != mo.owner
            || o.owner.is_none()
            || busy
            || s.priority >= ms.priority
            || ms.waited <= crate::clock::ticks(ms.patience as u32)
        {
            return false;
        }
        let priority = ms.priority;
        let Some(c) = self.aside_cell(k, priority) else {
            return false;
        };
        let aside = (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
        // A ship lying still on its way (waiting) goes on from there about
        // its task; one idle simply moves.
        let s = self.objects[k].ship.as_ref().unwrap();
        let on = match (s.task, s.path.back()) {
            (Some(_), Some(&(lx, ly))) => self.plan_route_from(k, aside, lx, ly),
            _ => None,
        };
        let s = self.objects[k].ship.as_mut().unwrap();
        s.path = std::iter::once(aside).collect();
        match on {
            Some(rest) => s.path.extend(rest.into_iter().skip_while(|&p| p == aside)),
            None => {
                s.task = None;
                s.order_tag = 0;
            }
        }
        s.face = None;
        s.stalled = 0.0;
        s.priority = priority;
        s.patience = objects::PATIENCE;
        s.blocked = 0.0;
        let ms = self.objects[m].ship.as_mut().unwrap();
        if ms.patience < 200 {
            ms.patience = ms.patience.wrapping_mul(3);
        }
        ms.waited = 0.0;
        true
    }

    /// Ships held up by others: one whose last cell is taken by a ship
    /// lying there makes for the nearest free cell instead; one held up on
    /// the way long enough goes round.
    fn go_round(&mut self, held: Vec<Held>) {
        const PATIENCE: f32 = 1.0;
        let t = TILE_SIZE as f32;
        let cell_of = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
        let centre = |(cx, cy): (i32, i32)| (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
        for (id, cell, by_lying, by_idle, other) in held {
            // An idle ship of the same player in the way makes room: it
            // moves to the nearest free cell off the other's way.
            if by_lying && self.push_aside(other, id) {
                continue;
            }
            let Some(i) = self.object_index(id) else {
                continue;
            };
            // Held up 150 ticks, it gives the move up and lies still
            // (0x4a618d: message 0x80, sub 8), where its player's ships of
            // higher priority may push it aside; a gatherer's task sends
            // it on again. Ships with other tasks here keep waiting.
            {
                let s = self.objects[i].ship.as_mut().unwrap();
                let gives_up = matches!(s.task, None | Some(Task::Gather { .. }));
                if gives_up && s.waited >= crate::clock::ticks(150) {
                    s.path.clear();
                    s.waited = 0.0;
                    s.stalled = 0.0;
                    s.blocked = 0.0;
                    continue;
                }
            }
            let (x, y) = (self.objects[i].x, self.objects[i].y);
            let ship = self.objects[i].ship.as_ref().unwrap();
            let Some(&(lx, ly)) = ship.path.back() else {
                continue;
            };
            let last = cell_of(lx, ly);
            let retarget = last == cell && by_idle;
            if !retarget && ship.blocked <= PATIENCE {
                continue;
            }
            // Cells other ships lie in or make for.
            let others: HashSet<(i32, i32)> = self
                .objects
                .iter()
                .filter(|o| o.id != id)
                .filter_map(|o| {
                    let s = o.ship.as_ref()?;
                    Some([
                        Some(cell_of(o.x, o.y)),
                        s.path.back().map(|&(a, b)| cell_of(a, b)),
                    ])
                })
                .flatten()
                .flatten()
                .collect();
            let deep = objects::deep_draught(ship.kind);
            // Bound for a dock, another free cell of its berth ring.
            let berth = match ship.task {
                Some(Task::Repair { dock }) if retarget => self.object_index(dock).and_then(|d| {
                    self.beside_cells(d, (x, y))
                        .into_iter()
                        .find(|&(bx, by)| !others.contains(&cell_of(bx, by)))
                }),
                _ => None,
            };
            let route = if let Some((bx, by)) = berth {
                self.plan_route_avoiding(i, (x, y), bx, by, &others)
                    .or(Some(VecDeque::new()))
            } else if retarget {
                let free = (1..=8).find_map(|r: i32| {
                    let mut ring: Vec<(i32, i32)> = (-r..=r)
                        .flat_map(|dx| (-r..=r).map(move |dy| (cell.0 + dx, cell.1 + dy)))
                        .filter(|&(cx, cy)| (cx - cell.0).abs().max((cy - cell.1).abs()) == r)
                        .filter(|&c| self.nav.passable(c.0, c.1, deep) && !others.contains(&c))
                        .collect();
                    ring.sort_by(|a, b| {
                        let d = |c: &(i32, i32)| {
                            let (px, py) = centre(*c);
                            (px - x).hypot(py - y)
                        };
                        d(a).total_cmp(&d(b))
                    });
                    ring.first().copied()
                });
                free.and_then(|c| {
                    let (fx, fy) = centre(c);
                    self.plan_route_avoiding(i, (x, y), fx, fy, &others)
                })
                .or(Some(VecDeque::new()))
            } else {
                self.replan(i, (lx, ly))
            };
            // Try again a while later either way.
            let ship = self.objects[i].ship.as_mut().unwrap();
            ship.blocked = 0.0;
            if let Some(path) = route {
                ship.path = path;
            }
        }
    }

    /// A held-up ship's new route to `to` (0x4a6330 with 0x4a51b0): the
    /// ships within 3 cells lying still are closed (when it is impatient,
    /// past 50 + rand % 30 of wait, all not under way) and remembered; with
    /// a priority of 51 or more, those of lower priority not under way and
    /// those of its priority with a later id are listed instead (0x4a4e20).
    /// It then plans through the listed ones (R1) and round those of them
    /// not under way and of at least its priority (R2, 0x4a5540), keeping
    /// round unless that is about twice as long and 5-8 cells longer
    /// (0x4a56a0); going through, it remembers them. Remembered cells stay
    /// closed while their ship lies there.
    fn replan(&mut self, i: usize, to: (f32, f32)) -> Option<VecDeque<(f32, f32)>> {
        let t = TILE_SIZE as f32;
        let cell_of = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
        let me = &self.objects[i];
        let ship = me.ship.as_ref()?;
        let here = cell_of(me.x, me.y);
        let start = (me.x, me.y);
        let (id, prio) = (me.id, ship.priority);
        let wait = (ship.waited / crate::clock::TICK) as u32;
        let impatient = wait > 50 && self.rng.next() % 30 + 50 < wait;
        // Each other ship: its cell, priority, id, whether under way and
        // whether lying still or held.
        let under_way = |s: &objects::Ship| !s.path.is_empty() && s.stalled <= 0.0;
        let mut closed: HashSet<(i32, i32)> = HashSet::new();
        let mut listed: Vec<((i32, i32), u8, bool)> = Vec::new();
        let mut remember = Vec::new();
        for o in &self.objects {
            let Some(s) = o.ship.as_ref() else { continue };
            if o.id == id {
                continue;
            }
            let c = cell_of(o.x, o.y);
            if (c.0 - here.0).abs() > 3 || (c.1 - here.1).abs() > 3 {
                continue;
            }
            let moving = under_way(s);
            let still =
                s.path.is_empty() || s.stalled >= crate::clock::ticks(50) || s.grapple.is_some();
            let lower = s.priority < prio && !moving;
            let later = s.priority == prio && o.id > id;
            if prio >= 51 && (lower || later) {
                listed.push((c, s.priority, moving));
            } else if (impatient && !moving) || (!impatient && still) {
                closed.insert(c);
                remember.push(c);
            }
        }
        // Remembered cells whose ship still lies there stay closed.
        let lying_at: HashMap<(i32, i32), bool> = self
            .objects
            .iter()
            .filter(|o| o.id != id)
            .filter_map(|o| Some((cell_of(o.x, o.y), under_way(o.ship.as_ref()?))))
            .collect();
        let mut memory = std::mem::take(&mut self.objects[i].ship.as_mut()?.remembered);
        memory.retain(|c| lying_at.get(c).is_some_and(|&moving| !moving));
        closed.extend(memory.iter().copied());
        for c in remember {
            if !memory.contains(&c) {
                memory.push(c);
            }
        }
        let cells = |r: &VecDeque<(f32, f32)>| r.len() as i32 + 1;
        let route = if listed.is_empty() {
            self.plan_route_near(i, start, to.0, to.1, &closed)
        } else {
            let mut round = closed.clone();
            round.extend(
                listed
                    .iter()
                    .filter(|&&(_, p, moving)| !moving && p >= prio)
                    .map(|l| l.0),
            );
            let r2 = self.plan_route_near(i, start, to.0, to.1, &round);
            let r1 = self.plan_route_near(i, start, to.0, to.1, &closed);
            let go_round = match (&r1, &r2) {
                (None, None) => true,
                (Some(_), None) => false,
                (r1, Some(r2)) => {
                    let (l1, l2) = (r1.as_ref().map_or(0, cells), cells(r2));
                    let d = l2 - l1;
                    l2 <= l1
                        || d < 5
                        || (l2 <= 10 && d < 6)
                        || (l2 <= 15 && d < 7)
                        || (l2 <= 20 && d < 8)
                        || 2 * d < l2
                }
            };
            if go_round {
                // With no route through, a long way round is thrown away.
                r2.filter(|r| r1.is_some() || cells(r) <= 7)
            } else {
                for l in &listed {
                    if !memory.contains(&l.0) {
                        memory.push(l.0);
                    }
                }
                r1
            }
        };
        self.objects[i].ship.as_mut()?.remembered = memory;
        route
    }

    /// Ships under way plan their route again every 25 ticks (0x4a6150
    /// with 0x4a5740, not waited): the ships lying still within 3 cells
    /// and those it remembers are closed (and remembered), and a route
    /// round them taken when the present one runs into one.
    fn refresh_routes(&mut self, dt: f32) {
        let t = TILE_SIZE as f32;
        let cell_of = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
        let every = crate::clock::ticks(25);
        let due: Vec<usize> = (0..self.objects.len())
            .filter(|&i| {
                let Some(s) = self.objects[i].ship.as_mut() else {
                    return false;
                };
                s.planned += dt;
                let moving = !s.path.is_empty() && s.blocked <= 0.0 && s.grapple.is_none();
                if moving && s.planned >= every {
                    s.planned = 0.0;
                    return true;
                }
                false
            })
            .collect();
        if due.is_empty() {
            return;
        }
        // Ships lying still or held, and those not under way.
        let still: HashMap<(i32, i32), bool> = self
            .objects
            .iter()
            .filter_map(|o| {
                let s = o.ship.as_ref()?;
                let lying = s.path.is_empty() || s.stalled >= crate::clock::ticks(50);
                let under_way = !s.path.is_empty() && s.stalled <= 0.0;
                Some((
                    cell_of(o.x, o.y),
                    lying || s.grapple.is_some() || !under_way,
                ))
            })
            .collect();
        for i in due {
            let o = &self.objects[i];
            let here = cell_of(o.x, o.y);
            let s = o.ship.as_ref().unwrap();
            if s.ram_run.is_some() || s.face.is_some() {
                continue;
            }
            let mut memory = s.remembered.clone();
            memory.retain(|c| still.get(c).copied().unwrap_or(false));
            let mut closed: HashSet<(i32, i32)> = memory.iter().copied().collect();
            for (&c, &lying) in &still {
                if lying && c != here && (c.0 - here.0).abs() <= 3 && (c.1 - here.1).abs() <= 3 {
                    closed.insert(c);
                    if !memory.contains(&c) {
                        memory.push(c);
                    }
                }
            }
            let Some(&(lx, ly)) = s.path.back() else {
                continue;
            };
            // Its goal stays open (0x4a5740 reopens it).
            closed.remove(&cell_of(lx, ly));
            let blocked = s
                .path
                .iter()
                .any(|&(px, py)| closed.contains(&cell_of(px, py)));
            let start = (o.x, o.y);
            let route = if blocked {
                self.plan_route_avoiding(i, start, lx, ly, &closed)
            } else {
                None
            };
            let s = self.objects[i].ship.as_mut().unwrap();
            s.remembered = memory;
            if let Some(path) = route.filter(|p| !p.is_empty()) {
                s.path = path;
            }
        }
    }

    /// Cancels a ship's route.
    pub fn stop(&mut self, id: u32) {
        if let Some(i) = self.object_index(id) {
            // A ship being built is called off (event 8 with no record,
            // 0x4aa540 at 0x4aa565).
            if self.being_built(id) {
                self.cancel_build(i);
                return;
            }
            if let Some(ship) = self.objects[i].ship.as_mut() {
                ship.pending = None;
                // A stop under way forgets the cells it kept closed
                // (0x4a6330 with 1).
                if !ship.path.is_empty() {
                    ship.remembered.clear();
                }
                // Turning or at a cell (2, 3) it lies still at once; under
                // way or braking (5, 4, 1) it is told to stop (1) and stops
                // in the middle of the cell it is going into: at once if it
                // has entered it, else at the crossing (0x4a6330 with the
                // stop flag, then 0x4a38f0 and 0x4a3f40).
                match ship.move_state {
                    0 => ship.path.clear(),
                    2 | 3 => {
                        ship.path.clear();
                        ship.move_state = 0;
                    }
                    _ => {
                        ship.path.truncate(1);
                        ship.move_state = 1;
                    }
                }
                ship.task = None;
            }
        }
    }

    /// Advances the simulation by `dt` seconds.
    /// Fish shoals stay a while (25-75 ticks, and their surfacing), dive,
    /// and come up a moment later on another free water cell of their zone
    /// (Fish.cpp 0x48dd80, states 0, 4 and 5). One being fished stays.
    fn move_shoals(&mut self, dt: f32) {
        let t = TILE_SIZE as f32;
        // Shoals move on whether fished or not; their ships follow.
        let zones: Vec<usize> = (0..self.objects.len())
            .filter(|&i| self.objects[i].class == ObjectClass::FishZone)
            .collect();
        let mut emptied = Vec::new();
        for i in zones {
            let id = self.objects[i].id;
            if let Some(up) = self.shoals_up.get_mut(&id) {
                *up += dt;
            }
            if let Some(left) = self.shoals_gone.get_mut(&id) {
                *left -= dt;
                if *left > 0.0 {
                    continue;
                }
                self.shoals_gone.remove(&id);
                // Its wait over, one with nothing left (lightning's, 0x48e7ef)
                // is done for (0x48dd80 state 0: 0x48d2f0 gives 0): it
                // goes, or lies fallow while it may grow back.
                if self.objects[i]
                    .deposit
                    .is_some_and(|d| d.remaining == Some(0.0))
                {
                    emptied.push(id);
                    continue;
                }
                // Up to a hundred tries for a free water cell in the zone.
                let f =
                    |k: usize| self.objects[i].record.fields.get(k).copied().unwrap_or(0) as i32;
                let (zx, zy, zw, zh) = (f(6), f(7), f(8).max(1), f(9).max(1));
                for _ in 0..100 {
                    let cx = zx + (self.rng.next() % zw as u32) as i32;
                    let cy = zy + (self.rng.next() % zh as u32) as i32;
                    if self.nav.is_water(cx, cy) {
                        self.objects[i].x = cx as f32 * t + t / 2.0;
                        self.objects[i].y = cy as f32 * t + t / 2.0;
                        break;
                    }
                }
                let stay = (self.rng.next() & 3) as f32 * SHOAL_LIVING;
                self.shoals_shown
                    .insert(id, SHOAL_SURFACING + stay + SHOAL_DIVING);
                self.shoals_up.insert(id, 0.0);
                continue;
            }
            // Up when the mission starts: it stays a while, then dives.
            if !self.shoals_shown.contains_key(&id) {
                let stay = (self.rng.next() & 3) as f32 * SHOAL_LIVING;
                self.shoals_shown.insert(id, stay + SHOAL_DIVING);
            }
            let left = self.shoals_shown.get_mut(&id).unwrap();
            *left -= dt;
            if *left <= 0.0 {
                self.dive_shoal(id);
            }
        }
        for id in emptied {
            if let Some(k) = self.object_index(id) {
                if let Some(dep) = self.objects[k].deposit.take() {
                    if dep.regrows > 0 {
                        self.objects[k].dormant = Some((dep, 0.0));
                    } else {
                        self.objects.remove(k);
                    }
                }
            }
        }
    }

    /// Fish shoal `id` goes down for 25-75 ticks (0x48d910(0)).
    pub(super) fn dive_shoal(&mut self, id: u32) {
        self.shoals_shown.remove(&id);
        self.shoals_up.remove(&id);
        let away = (self.rng.next() % 51 + 25) as f32 * SHOAL_UNIT;
        self.shoals_gone.insert(id, away);
    }

    /// What object `obj` shows at `time_ms`: a fish shoal rising, living
    /// or diving as it is; anything else its own animation.
    pub fn frame_of<'a>(
        &'a self,
        obj: &'a objects::MapObject,
        time_ms: u64,
    ) -> &'a [objects::Layer] {
        if obj.class == ObjectClass::FishZone {
            // Its size by how much fish is left (0x48d2f0, as amber).
            let size = obj
                .deposit
                .and_then(|d| d.remaining)
                .map_or(3, |left| unload::amber_level(left, 1000.0))
                .clamp(1, 3);
            let [rise, live, dive] = &self.fish_art[size - 1];
            let pick = |frames: &'a Frames, done: f32| {
                let n = frames.len();
                (n > 0).then(|| {
                    frames[((done.clamp(0.0, 1.0) * n as f32) as usize).min(n - 1)].as_slice()
                })
            };
            let up = self.shoals_up.get(&obj.id).copied();
            let left = self.shoals_shown.get(&obj.id).copied();
            if let Some(f) = up
                .filter(|&u| u < SHOAL_SURFACING)
                .and_then(|u| pick(rise, u / SHOAL_SURFACING))
            {
                return f;
            }
            if let Some(f) = left
                .filter(|&l| l < SHOAL_DIVING)
                .and_then(|l| pick(dive, 1.0 - l / SHOAL_DIVING))
            {
                return f;
            }
            if !live.is_empty() {
                let unit = (SHOAL_UNIT * 1000.0) as u64;
                return &live[(time_ms / unit) as usize % live.len()];
            }
        }
        // A reef or whirlpool: the frame its cycle is at (0x4301f0); a
        // leviathan the frame it last drew (0x493f10).
        if let Some(f) = self
            .hazard_frame(obj.id)
            .or_else(|| self.leviathan_frame(obj.id))
        {
            if let Some(layers) = obj.frames.get(f as usize).or(obj.frames.last()) {
                return layers;
            }
        }
        obj.frame_at(time_ms)
    }

    /// Whether object `id` is a fish shoal gone below.
    pub fn shoal_gone(&self, id: u32) -> bool {
        self.shoals_gone.contains_key(&id)
    }

    /// Whether object `id` is a fish shoal diving or gone below: there is
    /// nothing to fish there.
    pub fn shoal_down(&self, id: u32) -> bool {
        self.shoal_gone(id)
            || self
                .shoals_shown
                .get(&id)
                .is_some_and(|&l| l < SHOAL_DIVING)
    }

    /// Scrolls the minimap only as far as keeps the view, whose top-left
    /// cell is `cell` and size `size` cells, inside it (0x51b8c0).
    pub fn follow_view(&self, cell: (i32, i32), size: (i32, i32)) {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let follow = |origin: i32, at: i32, view: i32, map: i32| {
            let shown = MINIMAP_CELLS.min(map);
            let mut o = origin;
            if at < o {
                o = at;
            }
            if o + shown - view < at {
                o = at - shown + view;
            }
            o.clamp(0, (map - shown).max(0))
        };
        let (ox, oy) = self.minimap_origin.get();
        self.minimap_origin
            .set((follow(ox, cell.0, size.0, w), follow(oy, cell.1, size.1, h)));
    }

    pub fn update(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.sync_units();
        self.move_shoals(dt);
        self.move_gulls(dt);
        self.run_wakes(dt);
        self.run_sparkles(dt);
        self.run_sheep(dt);
        self.shown_clock += dt;
        while self.shown_clock >= COUNTER_TICK {
            self.shown_clock -= COUNTER_TICK;
            for (shown, real) in self.shown.iter_mut().zip([self.amber, self.fish]) {
                let step = match shown.abs_diff(real) {
                    0 => 0,
                    1..=10 => 1,
                    11..=100 => 10,
                    _ => 100,
                };
                *shown = if *shown < real {
                    *shown + step
                } else {
                    *shown - step
                };
            }
        }
        let ships: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.task.is_some()))
            .map(|o| o.id)
            .collect();
        for id in ships {
            if let Some(i) = self.object_index(id) {
                self.run_task(i, dt, data, caches);
            }
        }
        self.clock += dt;
        self.sight_timer -= dt;
        if self.sight_timer <= 0.0 {
            self.sight_timer = sight::SIGHT_PERIOD;
            self.update_sight();
        }
        // The brain runs every game tick (scaled to this update's length).
        self.brain_clock += dt;
        while self.brain_clock >= crate::clock::TICK {
            self.brain_clock -= crate::clock::TICK;
            self.ticks += 1;
            // The navy's tick (message 0, 0x49ab30 -> 0x4c4f40): the gather
            // zones hear their ships' notices and plan again. (Not settled:
            // whether before or after the ships' own ticks.)
            self.run_gather_zones();
            self.run_jam_attacks();
            self.run_brain();
            self.step_fort_draws();
            for (class, cell) in std::mem::take(&mut self.pending_walkers) {
                self.put_walker(walkers::NO_LAIR, class, cell, data, caches);
            }
            self.tick_walkers(data, caches);
            self.run_computers();
            self.run_brain_jobs();
            self.run_brain_casts();
            self.run_town_links();
            self.run_yards(data, caches);
            // Standing pictures before construction, so one finished this
            // tick takes its first standing step on the next (+0xbc).
            self.step_standing_frames();
            self.step_constructions(data, caches);
            for (x, y, kind) in std::mem::take(&mut self.pending_reefs) {
                self.big_reef(x, y, kind, data, caches);
            }
            for (spell, x, y, owner) in std::mem::take(&mut self.pending_casts) {
                self.brain_cast(spell, x, y, owner, data, caches);
            }
            self.run_brain_flames(dt);
            let limed = self.limed();
            self.run_brain_brews(dt, &limed);
            self.run_hazard_cycles(data, caches);
            self.run_leviathans(data, caches);
            self.run_hunts(data, caches);
            // The commands of this tick reach their objects at the start
            // of the next, before anything runs (0x51fe20: 0x521570,
            // 0x5228c0, then the tick 0x542390): delivered here, before
            // the player can add his own after them.
            self.deliver(data, caches);
        }
        // The mission is judged every 250 ticks (0x448d40), a campaign
        // mission by its own judge every 125 or 75.
        self.ai_timer -= dt;
        if self.ai_timer <= 0.0 {
            let period = story::judge_of(&self.mission).map(|j| j.period);
            self.ai_timer = crate::clock::ticks(period.unwrap_or(250));
            if self.outcome.is_none() {
                self.outcome = match period {
                    Some(p) => self.judge_campaign(p),
                    None => self.judge(),
                };
                if period.is_none() && self.human.is_none() && is_custom(&self.mission) {
                    self.over |= self.spectators_done();
                }
            }
        }
        self.wind.update(dt);
        // Sounds nobody played (no front end) are dropped.
        if self.sounds.len() > 64 {
            self.sounds.clear();
        }
        self.run_training(dt);
        self.run_research(dt);
        self.run_refits(dt);
        // A limed temple's learning and Flame stand still (0x438300,
        // 0x439260).
        let limed = self.limed();
        self.magic.update(dt, &limed, self.permits);
        // Computer players learn spells in 100 ticks, not 200; their Flame
        // brews 250 ticks as anyone's, then burns 1500 more (0x439260).
        // (The port keeps no permission bits for them: all allowed.)
        for c in &mut self.computers.players {
            c.magic
                .update_learning(dt * 2.0, &limed, crate::permits::Permits::ALL);
            c.magic.update_flame(dt, &limed);
            c.flame = (c.flame - dt).max(0.0);
            c.flame += c.magic.take_flames() as f32 * crate::magic::FLAME_SECONDS;
        }
        self.seat_upkeep(dt, &limed);
        self.run_statuses(dt, data, caches);
        self.run_fires(dt, data, caches);
        self.run_fireballs(dt, data, caches);
        self.run_lightning(dt, data, caches);
        self.run_sight_points(dt);
        self.run_repairs(dt);
        self.run_collapses(dt);
        self.refresh_ship_looks(data, caches);
        self.run_orders();
        self.run_treasures(dt, data, caches);
        self.run_creatures(dt, data, caches);
        self.run_lime(dt);
        self.run_wizards(dt, data, caches);
        self.run_fetches();
        self.run_loads();
        self.run_flyers(dt, data, caches);
        self.run_walkers(dt, data, caches);
        self.run_porters(dt, data, caches);
        self.run_furling(dt);
        self.run_rocking(dt);
        self.run_regrowth(dt, data, caches);
        self.offer_argo();
        self.run_hazards(dt, data, caches);
        self.run_ruins(dt);
        self.run_combat(dt, data, caches);
        self.run_shots(dt, data, caches);
        self.run_burning(dt);
        self.run_volcanoes(dt, data, caches);
        self.run_wakeups(dt);
        self.run_contact(dt, data, caches);
        let mut moved = false;
        // One ship to a cell: the cells ships lie in, and those they are
        // already sailing into, are taken.
        let t = TILE_SIZE as f32;
        let cell_of = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
        let mut taken: HashMap<(i32, i32), u32> = HashMap::new();
        for o in self.objects.iter().filter(|o| o.ship.is_some()) {
            taken.entry(cell_of(o.x, o.y)).or_insert(o.id);
        }
        for o in &self.objects {
            let Some(&(nx, ny)) = o.ship.as_ref().and_then(|s| s.path.front()) else {
                continue;
            };
            let here = cell_of(o.x, o.y);
            let centre = (here.0 as f32 * t + t / 2.0, here.1 as f32 * t + t / 2.0);
            if (o.x - centre.0).hypot(o.y - centre.1) > 0.5 {
                taken.entry(cell_of(nx, ny)).or_insert(o.id);
            }
        }
        // The original's move states (+0x58c): 0 lying still, 3 waiting at
        // a cell for the next (0x4a3560), 6 held by a boarding, ram or
        // dock; the rest under way (2 turning, 4 stopping, 5 sailing). A
        // ship waiting 50 ticks falls to lying still, its route kept, and
        // may be pushed aside like any (0x4a374a).
        let rest = crate::clock::ticks(50);
        let lying: HashSet<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship
                    .as_ref()
                    .is_some_and(|s| s.path.is_empty() || s.stalled >= rest)
            })
            .map(|o| o.id)
            .collect();
        let idle: HashSet<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.path.is_empty()))
            .map(|o| o.id)
            .collect();
        let waiting: HashSet<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.stalled > 0.0))
            .map(|o| o.id)
            .collect();
        // Ships under a siren's song go no faster than 7000 units (0x4a74c0).
        let charmed: HashSet<u32> = self.charmed.keys().copied().collect();
        // Ships grappled, ramming or being rammed keep still (they are
        // moved by the boarding and the ram); a charging rammer may sail
        // into its target's cell.
        let held_still = self.held_by_contact();
        let charging: HashMap<u32, u32> = self
            .objects
            .iter()
            .filter_map(|o| {
                let r = o.ship.as_ref()?.ram_run?;
                (r.stage == objects::RamStage::Charge).then_some((o.id, r.target))
            })
            .collect();
        // Ships held up: the cell ahead, and whether a ship lies in it.
        let mut held = Vec::new();
        // Cells reefs and whirlpools hold, and the ones Jason sinks.
        let hazard_cells = self.hazard_cells();
        let mut jason_sinks = Vec::new();
        let players: Vec<Option<(u32, u32)>> = self
            .human
            .iter()
            .map(|h| Some((h.nation, h.slot)))
            .chain(
                self.seats
                    .iter()
                    .map(|s| Some((s.player.nation, s.player.slot))),
            )
            .collect();
        for obj in &mut self.objects {
            let Some(ship) = obj.ship.as_mut() else {
                continue;
            };
            // An order sets the ship's priority and patience (see
            // `Task::priority`).
            match ship.task {
                Some(task) if task.tag() != ship.order_tag => {
                    ship.order_tag = task.tag();
                    ship.patience = objects::PATIENCE;
                    if let Some(p) = task.priority() {
                        // The player's treasure hunts go out with 0.
                        let player = players.contains(&obj.owner);
                        ship.priority = if player && matches!(task, Task::Take { .. }) {
                            0
                        } else {
                            p
                        };
                    }
                }
                None if ship.path.is_empty() => ship.order_tag = 0,
                _ => {}
            }
            // A ship turned to stone does not move.
            if obj.combat.as_ref().is_some_and(|c| c.stone > 0.0) {
                continue;
            }
            if held_still.contains(&obj.id) {
                ship.velocity = 0.0;
                continue;
            }
            // The wind speeds or slows the ship by its heading.
            let merchant = cargo_capacity(ship.kind) > 0.0;
            let rigging = obj
                .owner
                .and_then(|o| self.levels.get(&o))
                .map_or(1.0, |l| combat::speed_factor(l.speed));
            // Zetes's brothers carry his ship as if before the wind; the
            // feather of Boreas speeds it.
            let wind = if ship.has_hero(objects::ZETES) {
                (0..8)
                    .map(|h| self.wind.speed_factor(merchant, h))
                    .fold(0.0, f32::max)
            } else {
                self.wind.speed_factor(merchant, ship.heading)
            };
            let feather = 1.0 + ship.feather_percent() as f32 / 100.0;
            let (worn, worn_accel) = obj.combat.as_ref().map_or((1.0, 1.0), |c| {
                combat::sailing_factors(c.hull, c.max_hull, ship.armour)
            });
            // A ship first turns to its next course, through the hull's
            // rotations, then sails it.
            if !turn_ship(ship, obj.x, obj.y, dt) {
                ship.velocity = 0.0;
                // Turning (move state 2, 0x4a3320).
                ship.move_state = 2;
                continue;
            }
            // Turned, it is at its cell for the next step (3), or lying
            // still with no route (0), set to leave it (0x4a3320: `+0x596`).
            if ship.move_state == 2 {
                ship.move_state = if ship.path.is_empty() { 0 } else { 3 };
                ship.between.set(true);
            }
            // No route (dropped, 0x4a5b70, or run to its end): it lies
            // still (0); but held (6) where it lies, as at a port's berth
            // unloading, until it leaves.
            let unloading = matches!(
                ship.task,
                Some(Task::Gather {
                    phase: objects::GatherPhase::Unloading(_),
                    ..
                })
            );
            if ship.path.is_empty() && !(unloading && ship.move_state == 6) {
                ship.move_state = 0;
            }
            // It gathers way up to its speed, and loses it in time to stop
            // where it is bound or before a turn of more than 45 degrees
            // (0x4a3f40, 0x4a2f60).
            let mut top = ship.speed * wind * rigging * worn * feather;
            if charmed.contains(&obj.id) {
                top = top.min(creatures::CHARMED_SPEED);
            }
            let accel = objects::ship_accel(ship.kind) * worn_accel * wind.max(0.1);
            let ahead = {
                let mut d = 0.0;
                let mut from = (obj.x, obj.y);
                let mut heading = None;
                for &(px, py) in &ship.path {
                    let (lx, ly) = (px - from.0, py - from.1);
                    if lx.hypot(ly) > 0.5 {
                        let h = heading_towards(lx, ly);
                        if let Some(prev) = heading {
                            let steps = (h as i32 - prev as i32).rem_euclid(8);
                            if steps != 0 && steps != 1 && steps != 7 {
                                break;
                            }
                        }
                        heading = Some(h);
                    }
                    d += lx.hypot(ly);
                    from = (px, py);
                }
                d
            };
            let resting = ship.velocity == 0.0;
            let stopping = (2.0 * accel * ahead).sqrt().max(MIN_WAY);
            let target = top.min(stopping);
            let limed = obj.combat.as_ref().is_some_and(|c| c.lime > 0.0);
            ship.velocity = if limed {
                // Limed, each tick its way is halved after the cap
                // (0x4a3f40): it crawls at about a tick's gain.
                let tick = crate::clock::TICK;
                ship.lime_clock += dt;
                let mut v = ship.velocity;
                while ship.lime_clock >= tick {
                    ship.lime_clock -= tick;
                    v = (v + accel * tick).min(target) / 2.0;
                }
                v.min(target)
            } else if ship.velocity < target {
                (ship.velocity + accel * dt).min(target)
            } else {
                target
            };
            // A diagonal step goes a third of the way on each axis for a
            // half on one (0x4a3f40).
            let diagonal = ship
                .path
                .front()
                .is_some_and(|&(tx, ty)| (tx - obj.x).abs() > 0.5 && (ty - obj.y).abs() > 0.5);
            let along = if diagonal { DIAGONAL_PACE } else { 1.0 };
            let mut budget = ship.velocity * along * dt;
            // Closing in on a shore or pier, and backing out from it to its
            // cell's middle, it slides along a line of points at a steady
            // pace (0x4b02a0, state 0x13). Routes run between cell centres;
            // a point off them is a shore or pier closed in on.
            let centre = |v: f32| (v.rem_euclid(t) - t / 2.0).abs() < 0.5;
            let sliding = ship.path.front().is_some_and(|&(tx, ty)| {
                let (to_mid, at_mid) = (centre(tx) && centre(ty), centre(obj.x) && centre(obj.y));
                cell_of(tx, ty) == cell_of(obj.x, obj.y)
                    && ((ship.path.len() == 1 && !to_mid) || (to_mid && !at_mid && resting))
            });
            if sliding {
                ship.velocity = 0.0;
                budget = CLOSING_PACE * dt;
            }
            while budget > 0.0 {
                let Some(&(tx, ty)) = ship.path.front() else {
                    break;
                };
                let next = cell_of(tx, ty);
                // A reef or whirlpool up there: 0x499aa0 refuses the step
                // (nibble 1 of 4-6). With no ship there, one with Jason
                // aboard sinks a whirlpool or medium reef (0x4a3a70 ->
                // 0x4a7630: 0xda9f / 0xda9e) and steps in; others wait
                // there as for any refused step.
                let hazard = hazard_cells.get(&next).copied();
                if next != cell_of(obj.x, obj.y) && hazard.is_some() && !taken.contains_key(&next) {
                    if hazard == Some(true) && ship.has_hero(objects::JASON) {
                        jason_sinks.push(next);
                    } else {
                        ship.blocked += dt;
                        ship.stalled += dt;
                        let last = ship.path.back().map(|&(lx, ly)| cell_of(lx, ly));
                        if last != Some(next) {
                            ship.waited += dt;
                        }
                        ship.velocity = 0.0;
                        ship.move_state = if ship.stalled >= crate::clock::ticks(50) {
                            0
                        } else {
                            3
                        };
                        held.push((obj.id, next, false, false, 0));
                        break;
                    }
                }
                if next != cell_of(obj.x, obj.y) {
                    match taken.get(&next) {
                        Some(&other)
                            if other != obj.id && charging.get(&obj.id) != Some(&other) =>
                        {
                            ship.blocked += dt;
                            ship.stalled += dt;
                            // The wait grows a tick for a step refused
                            // short of the route's last cell, and a tick
                            // more when the ship there lies still, waits
                            // or is held (0x4a3a70).
                            if lying.contains(&other)
                                || waiting.contains(&other)
                                || held_still.contains(&other)
                            {
                                ship.waited += dt;
                            }
                            let last = ship.path.back().map(|&(lx, ly)| cell_of(lx, ly));
                            if last != Some(next) {
                                ship.waited += dt;
                            }
                            ship.velocity = 0.0;
                            // Held at its cell (3) until 50 ticks without
                            // a step lay it still (0, 0x4a3560).
                            ship.move_state = if ship.stalled >= crate::clock::ticks(50) {
                                0
                            } else {
                                3
                            };
                            held.push((
                                obj.id,
                                next,
                                lying.contains(&other),
                                idle.contains(&other),
                                other,
                            ));
                            break;
                        }
                        _ => {
                            taken.insert(next, obj.id);
                            ship.blocked = 0.0;
                            ship.stalled = 0.0;
                            ship.waited = 0.0;
                            // The next cell reserved (`+0x5b6`,
                            // 0x4a3a70): under way (5, 0x4a3560), between
                            // cells as it leaves its cell's middle; told
                            // to stop it stays 1 till it crosses into it.
                            ship.entering = next;
                            if matches!(ship.move_state, 0 | 2 | 3) {
                                ship.between.set(true);
                            }
                            if ship.move_state != 1 {
                                ship.move_state = 5;
                            }
                        }
                    }
                }
                let (dx, dy) = (tx - obj.x, ty - obj.y);
                let dist = (dx * dx + dy * dy).sqrt();
                if dist <= budget {
                    obj.x = tx;
                    obj.y = ty;
                    budget -= dist;
                    ship.path.pop_front();
                    moved = true;
                    // Into the cell it was entering, it is in it
                    // (0x4a3f40: `+0x596` cleared); at a cell's middle
                    // it is set to leave it (`+0x596` set again).
                    if ship.between.get() && cell_of(tx, ty) == ship.entering {
                        ship.between.set(false);
                    }
                    if centre(tx) && centre(ty) {
                        ship.between.set(true);
                    }
                    // At a route point: the last lays it still (0), any
                    // other has it about to step (3, 0x4a3f40).
                    ship.move_state = if ship.path.is_empty() { 0 } else { 3 };
                    // A new course: under way, a ship bears off 45 degrees at
                    // once; more, it stops and turns to it first (0x4a3560).
                    if let Some(&(nx, ny)) = ship.path.front() {
                        let (ex, ey) = (nx - obj.x, ny - obj.y);
                        let next = heading_towards(ex, ey);
                        if ex.hypot(ey) > 0.5 && next != ship.heading {
                            let steps = (next as i32 - ship.heading as i32).rem_euclid(8);
                            if steps == 1 || steps == 7 {
                                ship.heading = next;
                            } else {
                                break;
                            }
                        }
                    }
                } else {
                    obj.x += dx / dist * budget;
                    obj.y += dy / dist * budget;
                    budget = 0.0;
                    if ship.between.get() && cell_of(obj.x, obj.y) == ship.entering {
                        ship.between.set(false);
                    }
                }
                moved = true;
            }
            // Come into its route's last cell, it brakes to a stop there
            // (4, 0x4a2f60 from 0x4a3f40).
            let last = ship.path.back().map(|&(lx, ly)| cell_of(lx, ly));
            // Told to stop (1), it brakes there once it has entered the
            // cell (0x4a38f0: `+0x596 == 0`).
            if matches!(ship.move_state, 1 | 5) && last == Some(cell_of(obj.x, obj.y)) {
                ship.move_state = 4;
            }
        }
        for c in jason_sinks {
            self.sink_hazard_at(c, data, caches);
        }
        self.go_round(held);
        self.plan_pending(dt);
        self.refresh_routes(dt);
        if moved {
            // Keep painter's order as ships move.
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        }
    }

    /// The top-left world corner of the view when the mission starts, as
    /// the original sets it (`WIN_VIEW`): the cell's corner (checked against
    /// the original's first frame of miss000).
    pub fn start_corner(&self) -> Option<(f32, f32)> {
        let t = TILE_SIZE as f32;
        self.human
            .and_then(|h| self.views.get((h.nation * 4 + h.slot) as usize))
            .copied()
            .flatten()
            .map(|(x, y)| (x as f32 * t, y as f32 * t))
    }

    /// The world point to centre the view on when the mission starts: the
    /// middle of the original's view (its 592x527 game field) as the
    /// mission sets it for the active player; the map's middle without one.
    pub fn start_view(&self) -> (f32, f32) {
        let t = TILE_SIZE as f32;
        let at = self
            .human
            .and_then(|h| self.views.get((h.nation * 4 + h.slot) as usize))
            .copied()
            .flatten();
        match at {
            Some((x, y)) => (x as f32 * t + 296.0, y as f32 * t + 263.0),
            None => {
                let (w, h) = self.map_size();
                (w as f32 / 2.0, h as f32 / 2.0)
            }
        }
    }

    pub fn map_size(&self) -> (u32, u32) {
        (self.terrain.width, self.terrain.height)
    }

    /// Ships at rest rock (0x4a3150): each tick a fresh delay of 15 +
    /// rand % 7 ticks is drawn, and the frame moves once more ticks than
    /// that have passed since it last did, swinging over its four frames
    /// and back (0, 1, 2, 3, 3, 2, 1, 0, 0, ...): its at-rest frames, or
    /// lying with its sail furled, the furled ones. Only shown.
    fn run_rocking(&mut self, dt: f32) {
        let tick = crate::clock::ticks(1);
        for o in &mut self.objects {
            let Some(s) = o.ship.as_mut() else {
                continue;
            };
            if !s.path.is_empty() || s.rotation.is_some() {
                s.rock = 0;
                s.rock_back = false;
                s.rock_ticks = 0;
                s.rock_clock = 0.0;
                continue;
            }
            s.rock_clock += dt;
            while s.rock_clock >= tick {
                s.rock_clock -= tick;
                s.rock_ticks += 1;
                let delay = 15 + self.gull_rng.next() % 7;
                if s.rock_ticks > delay {
                    s.rock_ticks = 0;
                    if s.rock_back {
                        if s.rock == 0 {
                            s.rock_back = false;
                        } else {
                            s.rock -= 1;
                        }
                    } else if s.rock >= 3 {
                        s.rock_back = true;
                    } else {
                        s.rock += 1;
                    }
                }
            }
        }
    }

    /// Ships lying loading or unloading furl their sails a frame every 5
    /// ticks, and unfurl as they leave.
    fn run_furling(&mut self, dt: f32) {
        let lying: HashSet<u32> = self
            .planks
            .iter()
            .filter(|p| {
                matches!(
                    p.activity,
                    unload::Activity::Unloading | unload::Activity::Amber
                )
            })
            .map(|p| p.ship)
            .collect();
        let step = dt / objects::SHIP_FURL_SECONDS;
        for o in &mut self.objects {
            let (id, Some(s)) = (o.id, o.ship.as_mut()) else {
                continue;
            };
            // A grappled ship's sail is the boarding's to furl.
            if s.grapple.is_some() {
                continue;
            }
            s.furl = if lying.contains(&id) {
                (s.furl.max(1.0) + step).min(objects::SHIP_FURLED)
            } else {
                (s.furl - step).max(0.0)
            };
        }
    }
}

/// What was under way in battle when the game was saved (see
/// `SaveGame::battle`).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Battle {
    #[serde(default)]
    pub shots: Vec<shot::SavedShot>,
    #[serde(default)]
    pub fireballs: Vec<fireball::SavedFireball>,
    #[serde(default)]
    pub lightning: Vec<(f32, f32, f32)>,
    #[serde(default)]
    pub lightning_by: Vec<Option<(u32, u32)>>,
    #[serde(default)]
    pub sight_points: Vec<(i32, i32, i32, f32)>,
    #[serde(default)]
    pub charmed: Vec<(u32, creatures::Charm)>,
    #[serde(default)]
    pub siren_songs: Vec<(u32, f32)>,
    #[serde(default)]
    pub siren_shown: Vec<(u32, f32)>,
    #[serde(default)]
    pub gorgons: Vec<(u32, creatures::GorgonCycle)>,
    /// Unused: kept for older saves.
    #[serde(default)]
    pub sea_monsters: Vec<(u32, (f32, f32))>,
    #[serde(default)]
    pub sea_monster_states: Vec<(u32, sea_monster::SeaMonster)>,
    #[serde(default)]
    pub wizards: Vec<(u32, script::Wizard)>,
}

/// The least way a ship keeps while bound somewhere, in world pixels a
/// second, so braking ends.
const MIN_WAY: f32 = 10.0;
/// A diagonal step's pace against a straight one's: 2 x sqrt(2) / 3.
const DIAGONAL_PACE: f32 = 0.9428;

/// World pixels a second a ship slides closing in on a shore or pier, or
/// backing out: two points of its line a frame of its picture, every 3
/// ticks (0x4b02a0).
const CLOSING_PACE: f32 = 2.0 / (3.0 * crate::clock::TICK);

/// Seconds a ship takes to turn its hull one of 24 rotations: a frame of
/// its picture (0x4a3320).
const TURN_STEP_SECONDS: f32 = objects::SHIP_FRAME_MS as f32 / 1000.0;

/// Turns a ship at `(x, y)` towards its next waypoint, a rotation at a
/// time; true once it lies on that course.
fn turn_ship(ship: &mut objects::Ship, x: f32, y: f32, dt: f32) -> bool {
    let short = ship
        .path
        .front()
        .is_none_or(|&(tx, ty)| (tx - x).hypot(ty - y) < 24.0);
    // Closing in on a shore or pier it turns side-on as it goes
    // (0x4afe50); otherwise short shifts (back to the cell's centre) it
    // makes without turning.
    if short {
        match ship.face {
            Some(face) => {
                rotate_ship(ship, face as i32 * 3, dt);
            }
            None => ship.rotation = None,
        }
        return true;
    }
    ship.face = None;
    let &(tx, ty) = ship.path.front().unwrap();
    let (dx, dy) = (tx - x, ty - y);
    rotate_ship(ship, heading_towards(dx, dy) as i32 * 3, dt)
}

/// Turns a ship's hull a rotation of 24 at a time towards `want`; true
/// once it lies so.
/// A player: nation and slot.
pub type Owner = (u32, u32);

/// A cell's types, a nibble a layer (the tile sets' record byte 4).
fn cell_type(data: &GameData, grid: &MapGrid, x: u32, y: u32) -> u32 {
    (0..grid.layers.min(8)).fold(0u32, |acc, l| {
        let t = grid.get(l, x, y);
        let kind = if t.variant == 0 {
            0
        } else {
            data.tilesets
                .by_id(t.tileset)
                .and_then(|s| s.raw.get(4))
                .map_or(0, |&b| b as u32 & 0xf)
        };
        acc | kind << (4 * l)
    })
}

fn rotate_ship(ship: &mut objects::Ship, want: i32, dt: f32) -> bool {
    let mut at = ship.rotation.map_or(ship.heading as i32 * 3, i32::from);
    if at == want {
        ship.rotation = None;
        ship.heading = (want / 3) as usize;
        return true;
    }
    ship.turn_clock += dt;
    while ship.turn_clock >= TURN_STEP_SECONDS && at != want {
        ship.turn_clock -= TURN_STEP_SECONDS;
        // The shorter way round.
        at = (at
            + if (want - at).rem_euclid(24) <= 12 {
                1
            } else {
                -1
            })
        .rem_euclid(24);
        if at % 3 == 0 {
            ship.heading = (at / 3) as usize;
        }
    }
    if at == want {
        ship.rotation = None;
        ship.turn_clock = 0.0;
        true
    } else {
        ship.rotation = Some(at as u8);
        false
    }
}

/// The ship heading (0 = east, counter-clockwise in 45 degree steps) closest
/// to the screen-space direction `(dx, dy)`.
pub fn heading_towards(dx: f32, dy: f32) -> usize {
    // Screen y grows downwards; headings count counter-clockwise from east.
    let angle = (-dy).atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    ((angle / step).round() as i32).rem_euclid(8) as usize
}

/// Which of the sound panel's switches a sound answers to: the original's
/// player (0x525370) plays channel 2 (voices) only with unit reports on
/// (bit 1 of `[0x5ca55f]`), channels 1 and 4 (firing, shots landing,
/// boarding, ramming, lightning) only with battle sounds (bit 2), and
/// channels 3, 5 and 6 (the world's other sounds: buildings, hits, spells,
/// creatures, treasures, wrecks) only with ambient sounds (bit 4); the
/// rest always.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundKind {
    Voice,
    Battle,
    Ambient,
    Always,
}

/// The ambient sounds the original places on channels 5 and 6 (0x50e570
/// with mode 5 or 6, every caller's mode and event read in the
/// disassembly; the names are those events' `SND_n` lists): placed as
/// the voices but silent from 12 cells (`audio::placed_ambient`). Mode
/// 5: events 85-87, 89, 92-94, 96, 99, 103, 105, 107, 110, 111, 114-116,
/// 122, 125-127, 129, 133, 136, 149-155, 169, 170, 174, 274, event 1 (a
/// ship, 0x4e00bc) and a building's fall `+0x1a8` (event 4 by default,
/// 0x422dec; 0x424492) and the land creatures' death (events 100-102,
/// 0x413c66: the mode in `edi`, 5 from 0x413be7); mode 6: events 88, 95,
/// 106, 128, 146-148, 271, 272. Mode 3 is only the map's ambient loop
/// (event 273, 0x51028c, 0x524ffb, the front end's channel 9).
pub const AMBIENT_PLACED: &[&str] = &[
    "hitmed", "apmed3", "deamons", "atmed1c", "hc1", "hc2", "hc3", "hc4", "hc5", "hc6", "c1", "c2",
    "c3", "c4", "lc1", "lc2", "lc3", "app3", "deadga1", "harpy2", "dr1", "dr3", "atgarp1", "hdr2",
    "hgar4", "hgar5", "prote3", "spider", "riff1wav", "wind8", "shum2", "eject", "eject1", "apse",
    "atmons3", "kit4", "prote1", "piw1a", "piw3a", "piw4a", "pit3", "crw1a", "crw2a", "crw3a",
    "crw4a", "weter1a", "bildsh3", "razru2", "sir1", "wreck1", "bf1", "deafir1", "fireball",
    "trea2", "whirl3", "deac1d", "destr1",
];

/// The kind of scene sound `name`.
pub fn sound_kind(name: &str) -> SoundKind {
    let voice = order::AMBER_VOICES
        .iter()
        .chain(&order::FISH_VOICES)
        .chain(&order::LEVIATHAN_VOICES)
        .any(|v| *v == name)
        || ["jb_d1", "121~2", "pe1", "bi_c4"].contains(&name)
        || name.starts_with("re_o")
        || name.starts_with("bi_c")
        || name.starts_with("come");
    let battle = shot::LAUNCH.contains(&name)
        || shot::LAND.iter().flatten().any(|l| l.contains(&name))
        || ["firee1", "abor6a", "109", "flash", "molniya2"].contains(&name);
    if voice {
        SoundKind::Voice
    } else if battle {
        SoundKind::Battle
    } else if ["deadsh", "deny"].contains(&name) {
        SoundKind::Always
    } else {
        SoundKind::Ambient
    }
}
