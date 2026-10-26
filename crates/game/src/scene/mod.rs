//! The currently loaded mission.

use anyhow::Result;
use openac_formats::map::TILE_SIZE;
use openac_formats::{players, MapGrid, ObjectClass, ObjectRecord, PlayerInfo};

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
use std::collections::VecDeque;

/// Marines being trained at a settlement, one at a time.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Training {
    pub site: u32,
    /// The item being made (see `combat::ITEM_NAMES`).
    pub kind: u8,
    /// Marines ordered and not yet trained.
    pub queued: u32,
    /// Seconds spent on the current one.
    pub elapsed: f32,
}

impl Training {
    /// Progress 0..1 of the marine in training.
    pub fn progress(&self) -> f32 {
        (self.elapsed / combat::train_seconds(self.kind)).min(1.0)
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

/// Seconds between steps of the resource counters (a game tick).
const COUNTER_TICK: f32 = 0.02;

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

/// A building going up.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Construction {
    pub site: u32,
    pub elapsed: f32,
    pub total: f32,
}

/// How far from the settlement that orders it a building may go, in cells.
/// Placeholder.
pub const BUILD_RADIUS: i32 = 6;
/// How close a ship must lie to the site of the settlement it founds.
pub const FOUND_REACH: i32 = 2;

/// A ship under construction at a shipyard.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Production {
    pub site: u32,
    pub ship: u8,
    /// The Argo (command 0x7d), a royal ship.
    #[serde(default)]
    pub argo: bool,
    pub elapsed: f32,
    pub total: f32,
}

pub struct Scene {
    pub mission: String,
    pub grid: MapGrid,
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
    /// Ships being built.
    pub productions: Vec<Production>,
    /// Buildings going up.
    pub constructions: Vec<Construction>,
    /// Amber and fish of the computer players.
    pub purses: std::collections::BTreeMap<(u32, u32), (u32, u32)>,
    /// Research levels per player (missing: all 0).
    pub levels: std::collections::BTreeMap<(u32, u32), combat::Levels>,
    /// Research at a sage school: site, upgrade, level, seconds spent.
    pub research: Vec<(u32, combat::Upgrade, u8, f32)>,
    /// Summoned creatures and the seconds they have left.
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
    /// Running one-shot animations (explosions, splashes).
    pub effects: Vec<Effect>,
    rng: Rng,
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
    /// The campaign mission's ported brain, if any, and time owed to it.
    pub brain: Option<brain::Brain>,
    brain_clock: f32,
    /// Heroes the campaign brought to this mission, and heroes waiting at
    /// settlements and towns (by site).
    pub carried: Vec<u8>,
    pub site_heroes: std::collections::BTreeMap<u32, Vec<u8>>,
    /// Treasures held by objects, dropped when they are destroyed.
    pub held: Vec<(u32, crate::objects::Treasure)>,
    /// Harpies and dragons out of their nests, and the nests' counts.
    pub flyers: Vec<flyers::Flyer>,
    pub nests: Vec<flyers::Nest>,
    /// Cyclops, minotaurs and amazons out of their lairs, and the seconds
    /// since each lair last sent one.
    pub walkers: Vec<walkers::Walker>,
    pub lair_clocks: std::collections::BTreeMap<u32, f32>,
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
    /// Spells the player may cast once for nothing (disposable magic).
    pub free_casts: std::collections::BTreeMap<Spell, u32>,
    /// Buildings being repaired by a settlement or town.
    pub mending: Vec<u32>,
    /// Fractions of amber owed for repairs, per player.
    pub repair_owed: std::collections::BTreeMap<(u32, u32), f32>,
    /// Fireballs burning: position and seconds left.
    pub fires: Vec<(f32, f32, f32)>,
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
    /// The mission briefing, as paragraphs.
    pub briefing: Vec<String>,
    /// The briefing as the original's "Your mission" screen lays it out
    /// (`openac_formats::briefing_text`).
    pub briefing_text: Vec<String>,
    /// The briefing's lines as stored (the help panel builds on them).
    pub briefing_lines: Vec<String>,
    /// The human player's temple magic.
    pub magic: Magic,
}

mod ai;
mod battle;
mod brain;
mod creatures;
mod orders;
mod repair;
mod sight;
mod treasure;
pub use sight::{FogArt, Seen};
mod flyers;
mod order;
mod script;
mod seats;
mod spells;
mod stats;
mod story;
mod unload;
mod walkers;
pub use brain::{Brain, EASY, HARD, NORMAL};
pub use flyers::{Flyer, Nest};
pub use order::Order;
pub use stats::Stats;
pub use story::{goal_name, Carry, Goal};
pub use walkers::{walker_sequence, Walker};
#[cfg(test)]
mod tests;
mod trade;

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
        let archive = data.open_mission(mission)?;
        let grid = data.load_map(&archive, mission)?;
        let terrain = Terrain::compose(data, &grid, &mut caches.tiles)?;
        if !terrain.missing_tiles.is_empty() {
            eprintln!(
                "warning: {mission}: missing tiles: {}",
                terrain.missing_tiles.join(", ")
            );
        }
        let objs = objects::load(data, caches, &archive)?;
        if !objs.unresolved.is_empty() {
            let mut classes: Vec<_> = objs.unresolved.iter().collect();
            classes.sort();
            let list: Vec<String> = classes.iter().map(|(c, n)| format!("{c:#x}x{n}")).collect();
            eprintln!(
                "{mission}: {} objects drawn; not drawn yet (class x count): {}",
                objs.drawn.len(),
                list.join(" ")
            );
        }
        let (mw, mh, minimap) = terrain.minimap(&palette_lut(data, 0));
        let nav = NavGrid::from_map(&grid);
        let cells = (nav.width * nav.height) as usize;
        let players = players(&archive).unwrap_or_default();
        let human = players.iter().copied().find(|p| !p.computer);
        let conditions = openac_formats::conditions(&archive)?;
        let mut scene = Self {
            mission: mission.to_string(),
            grid,
            terrain,
            objects: objs.drawn,
            nav,
            productions: Vec::new(),
            constructions: Vec::new(),
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
            effects: Vec::new(),
            rng: Rng(0x1234_5678),
            ai_timer: 0.0,
            clock: 0.0,
            fog: true,
            fires: Vec::new(),
            mending: Vec::new(),
            free_casts: Default::default(),
            creature_clock: 0.0,
            story: Vec::new(),
            start_sites: Vec::new(),
            delivered: Vec::new(),
            held: Vec::new(),
            permits: crate::permits::Permits::ALL,
            seats: Vec::new(),
            active: 0,
            stats: Default::default(),
            planks: Vec::new(),
            porters: Vec::new(),
            views: (0..20)
                .map(|id| openac_formats::mission::start_view(&archive, id / 4, id % 4))
                .collect(),
            flyers: Vec::new(),
            nests: Vec::new(),
            walkers: Vec::new(),
            lair_clocks: Default::default(),
            carried: Vec::new(),
            site_heroes: Default::default(),
            brain: None,
            brain_clock: 0.0,
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
            briefing: openac_formats::briefing(&archive).unwrap_or_default(),
            briefing_text: openac_formats::briefing_text(&archive).unwrap_or_default(),
            briefing_lines: openac_formats::briefing_lines(&archive).unwrap_or_default(),
            magic: Magic::default(),
        };
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
        // The campaign's missions are judged by their story.
        if is_campaign(mission) {
            scene.permits = crate::permits::Permits::campaign(mission[4..].parse().unwrap_or(0));
            // The campaign's heroes so far wait at the starting settlement.
            scene.carried = story::carried_heroes(data, mission);
            scene.start_sites = scene.own_sites();
            // After miss002 they sail in the Argo; before, or without a
            // ship, they wait at the starting settlement.
            let argo = if mission > "miss002" {
                scene.ready_argo(data, caches)
            } else {
                None
            };
            if !scene.carried.is_empty() {
                let human = scene.human.map(|h| (h.nation, h.slot));
                let first_ship = || {
                    scene
                        .objects
                        .iter()
                        .position(|o| o.ship.is_some() && o.owner == human)
                };
                match (argo, scene.start_sites.first()) {
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
            let heroes = openac_formats::mission::heroes(&archive).unwrap_or_default();
            scene.story = scene.story_goals(heroes);
            let wanted: Vec<u8> = (0..9u8).filter(|&h| heroes[h as usize]).collect();
            scene.hide_hero_items(&wanted, data, caches);
            scene.start_brain(data, caches, difficulty);
            scene.start_sites = scene.own_sites();
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
            productions: self.productions.clone(),
            constructions: self.constructions.clone(),
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
            research_levels: self.levels.iter().map(|(k, v)| (*k, *v)).collect(),
            sight: self.sight.iter().map(|&s| s as u8).collect(),
            research: self.research.clone(),
            mending: self.mending.clone(),
            free_casts: self
                .free_casts
                .iter()
                .map(|(&s, &n)| (s as u32, n))
                .collect(),
            start_sites: self.start_sites.clone(),
            delivered: self.delivered.clone(),
            held: self.held.clone(),
            permits: Some(self.permits),
            stats: self.stats.clone(),
            flyers: self.flyers.clone(),
            nests: self.nests.clone(),
            walkers: self.walkers.clone(),
            lair_clocks: self.lair_clocks.clone(),
            site_heroes: self
                .site_heroes
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            brain: self.brain.clone(),
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
        scene.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        scene.amber = save.amber;
        scene.fish = save.fish;
        scene.productions = save.productions.clone();
        scene.constructions = save.constructions.clone();
        scene.trainings = save.trainings.clone();
        scene.garrisons = save.garrisons.clone();
        scene.purses = save.purses.iter().copied().collect();
        scene.summons = save.summons.clone();
        scene.magic = save.magic.clone();
        scene.wind = save.wind.clone();
        scene.outcome = save.outcome;
        scene.clock = save.clock;
        scene.ai_timer = save.ai_timer;
        scene.rng = save.rng.clone();
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
        scene.delivered = save.delivered.clone();
        scene.held = save.held.clone();
        scene.flyers = save.flyers.clone();
        scene.stats = save.stats.clone();
        for i in 0..scene.objects.len() {
            scene.refresh_amber(i, data, caches);
        }
        scene.nests = save.nests.clone();
        scene.walkers = save.walkers.clone();
        scene.lair_clocks = save.lair_clocks.clone();
        if let Some(p) = save.permits {
            scene.permits = p;
        }
        scene.site_heroes = save.site_heroes.iter().cloned().collect();
        if save.brain.is_some() {
            scene.brain = save.brain.clone();
        }
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
        let Some(path) = self.plan_route(i, x, y) else {
            return false;
        };
        let Some(ship) = self.objects[i].ship.as_mut() else {
            return false;
        };
        ship.path = path;
        ship.task = None;
        ship.zone = None;
        true
    }

    /// Adds a leg from the end of ship `id`'s route (or where it is) to the
    /// world point `(x, y)`: a waypoint.
    pub fn order_waypoint(&mut self, id: u32, x: f32, y: f32) -> bool {
        let Some(i) = self.object_index(id) else {
            return false;
        };
        let Some(ship) = self.objects[i].ship.as_ref() else {
            return false;
        };
        let from = ship
            .path
            .back()
            .copied()
            .unwrap_or((self.objects[i].x, self.objects[i].y));
        let Some(leg) = self.plan_route_from(i, from, x, y) else {
            return false;
        };
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path.extend(leg);
        ship.task = None;
        true
    }

    /// A route for object `i` to the world point `(x, y)`, or to the nearest
    /// water if the point is not navigable.
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
        let t = TILE_SIZE as f32;
        let obj = &self.objects[i];
        let start_point = start;
        let from = ((start.0 / t).floor() as i32, (start.1 / t).floor() as i32);
        let target = ((x / t).floor() as i32, (y / t).floor() as i32);
        let to = self.nav.nearest_water(target.0, target.1)?;
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
        let cells = self.nav.find_path(start, to, deep)?;
        let mut points: Vec<(f32, f32)> = cells
            .into_iter()
            .map(|(cx, cy)| (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0))
            .collect();
        // Finish on the exact point if it is in open water.
        if to == target {
            if let Some(last) = points.last_mut() {
                *last = (x, y);
            }
        }
        // Straight courses where the water allows, not the grid's steps.
        Some(self.nav.smooth(start_point, &points, t, deep).into())
    }

    /// Cancels a ship's route.
    pub fn stop(&mut self, id: u32) {
        if let Some(i) = self.object_index(id) {
            if let Some(ship) = self.objects[i].ship.as_mut() {
                ship.path.clear();
                ship.zone = None;
                ship.task = None;
            }
        }
    }

    /// Advances the simulation by `dt` seconds.
    pub fn update(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
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
        for p in &mut self.productions {
            p.elapsed += dt;
        }
        self.show_productions(data, caches);
        let (done, running): (Vec<_>, Vec<_>) = self
            .productions
            .drain(..)
            .partition(|p| p.elapsed >= p.total);
        self.productions = running;
        for p in done {
            match self.launch_ship(p.site, p.ship, data, caches) {
                Ok(Some(id)) => {
                    if p.argo {
                        self.christen_argo(id);
                    }
                    self.ship_launched(p.site, p.ship, id)
                }
                Ok(None) => {}
                Err(e) => eprintln!("could not launch ship: {e:#}"),
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
        while self.brain_clock >= 0.02 {
            self.brain_clock -= 0.02;
            self.run_brain();
        }
        self.ai_timer -= dt;
        if self.ai_timer <= 0.0 {
            self.ai_timer = 1.0;
            self.run_ai();
            // Every few seconds the computer players manage their economy.
            if (self.clock as u32) % ai::ECONOMY_PERIOD == 0 {
                self.run_computer_economy();
            }
            if self.outcome.is_none() {
                self.outcome = self.judge();
            }
        }
        self.wind.update(dt);
        // Sounds nobody played (no front end) are dropped.
        if self.sounds.len() > 64 {
            self.sounds.clear();
        }
        self.run_training(dt);
        self.run_research(dt);
        self.magic.update(dt);
        self.seat_upkeep(dt);
        self.run_statuses(dt, data, caches);
        self.run_fires(dt, data, caches);
        self.run_repairs(dt);
        self.run_orders();
        self.run_treasures(dt, data, caches);
        self.run_creatures(dt);
        self.run_wizards(dt, data, caches);
        self.run_flyers(dt, data, caches);
        self.run_walkers(dt, data, caches);
        self.run_porters(dt, data, caches);
        self.run_regrowth(dt, data, caches);
        self.offer_argo();
        self.run_hazards(dt, data, caches);
        self.run_constructions(dt, data, caches);
        self.run_combat(dt, data, caches);
        self.run_contact(dt, data, caches);
        let mut moved = false;
        for obj in &mut self.objects {
            let Some(ship) = obj.ship.as_mut() else {
                continue;
            };
            // A ship turned to stone does not move.
            if obj.combat.as_ref().is_some_and(|c| c.stone > 0.0) {
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
            let feather = if ship.has_treasure(objects::FEATHER) {
                objects::FEATHER_SPEED
            } else {
                1.0
            };
            let mut budget = ship.speed * wind * rigging * feather * dt;
            while budget > 0.0 {
                let Some(&(tx, ty)) = ship.path.front() else {
                    break;
                };
                let (dx, dy) = (tx - obj.x, ty - obj.y);
                let dist = (dx * dx + dy * dy).sqrt();
                if dist > 0.5 {
                    ship.heading = heading_towards(dx, dy);
                }
                if dist <= budget {
                    obj.x = tx;
                    obj.y = ty;
                    budget -= dist;
                    ship.path.pop_front();
                } else {
                    obj.x += dx / dist * budget;
                    obj.y += dy / dist * budget;
                    budget = 0.0;
                }
                moved = true;
            }
        }
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
}

/// The ship heading (0 = east, counter-clockwise in 45 degree steps) closest
/// to the screen-space direction `(dx, dy)`.
pub fn heading_towards(dx: f32, dy: f32) -> usize {
    // Screen y grows downwards; headings count counter-clockwise from east.
    let angle = (-dy).atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    ((angle / step).round() as i32).rem_euclid(8) as usize
}
