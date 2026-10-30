//! Saved games: what a mission's state is beyond what its archive gives.
//!
//! A save names the mission and lists every object by its record plus the
//! state it has gained in play; loading rebuilds the mission and its
//! objects from the game data and then applies that state. Effects in
//! flight (explosions, tornadoes, auras) are not kept.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::combat::{Combatant, Levels, Rng, Upgrade};
use crate::magic::Magic;
use crate::objects::{Deposit, Ship};
use crate::scene::{Construction, Outcome, Training};
use crate::weather::Wind;

/// Bumped when the layout changes incompatibly.
pub const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub struct SaveGame {
    pub version: u32,
    pub mission: String,
    pub amber: u32,
    pub fish: u32,
    pub objects: Vec<SavedObject>,
    /// Ships waiting for a yard's berth, and the yards' queue clocks.
    #[serde(default)]
    pub ship_orders: Vec<crate::scene::ShipOrder>,
    #[serde(default)]
    pub yard_clocks: Vec<(u32, u32)>,
    /// The ships' generators' seed.
    #[serde(default)]
    pub rnd_init: Option<u32>,
    pub constructions: Vec<Construction>,
    pub trainings: Vec<Training>,
    pub garrisons: BTreeMap<u32, [u32; 9]>,
    pub purses: Vec<((u32, u32), (u32, u32))>,
    pub summons: Vec<(u32, f32)>,
    pub magic: Magic,
    pub wind: Wind,
    pub outcome: Option<Outcome>,
    pub clock: f32,
    pub ai_timer: f32,
    pub rng: Rng,
    /// Reefs' and whirlpools' cycles (0x4306d0, 0x447ee0).
    #[serde(default)]
    pub hazards: std::collections::BTreeMap<u32, crate::scene::Hazard>,
    /// Fish shoals below and in sight: zone and seconds left.
    #[serde(default)]
    pub shoals_gone: std::collections::BTreeMap<u32, f32>,
    #[serde(default)]
    pub shoals_shown: std::collections::BTreeMap<u32, f32>,
    #[serde(default)]
    pub shoals_up: std::collections::BTreeMap<u32, f32>,
    #[serde(default = "default_true")]
    pub default_arming: bool,
    /// Research levels (saves from before the sage school's research had
    /// a different `levels`, which is ignored).
    #[serde(default)]
    pub research_levels: Vec<((u32, u32), Levels)>,
    #[serde(default)]
    pub research: Vec<(u32, Upgrade, u8, f32)>,
    /// What the player had explored, per map cell (0 unexplored, 1
    /// explored, 2 in sight).
    #[serde(default)]
    pub sight: Vec<u8>,
    /// Buildings being repaired.
    #[serde(default)]
    pub mending: Vec<u32>,
    /// Forts mending their units with their structure.
    #[serde(default)]
    pub unit_mends: Vec<crate::scene::UnitMend>,
    /// Settlements' stock units' conditions, and their repairs.
    #[serde(default)]
    pub stock_conditions: BTreeMap<u32, [Vec<u8>; 9]>,
    #[serde(default)]
    pub stock_repairs: Vec<crate::scene::StockRepair>,
    /// Smithies' refit batches, and settlements' fish shares from
    /// cancelled trainings (`+0x252`, `+0x253`).
    #[serde(default)]
    pub refit_batches: BTreeMap<u32, crate::scene::Batch>,
    #[serde(default)]
    pub town_fish: BTreeMap<u32, (u8, u8)>,
    /// The ships' orders of the 0x1f0 group since the last tick.
    #[serde(default)]
    pub tick_orders: BTreeMap<u32, crate::scene::TickOrder>,
    /// Commands given and not yet delivered, and the ticks played.
    #[serde(default)]
    pub outbox: Vec<crate::scene::Order>,
    #[serde(default)]
    pub ticks: u32,
    /// Free casts from treasures, by spell index.
    #[serde(default)]
    pub free_casts: Vec<(u32, u32)>,
    /// The player's settlements at the mission's start (for its story).
    #[serde(default)]
    pub start_sites: Vec<u32>,
    /// Treasure kinds handed over at a settlement.
    #[serde(default)]
    pub delivered: Vec<u8>,
    #[serde(default)]
    pub joined: Vec<u8>,
    #[serde(default)]
    pub had_argo: bool,
    #[serde(default)]
    pub woken: bool,
    #[serde(default)]
    pub dug: Vec<(u32, u32, u32, u8, u8)>,
    #[serde(default)]
    pub strait_struck: bool,
    #[serde(default)]
    pub sold: Vec<u32>,
    #[serde(default)]
    pub strait_checked: bool,
    #[serde(default)]
    pub winning: u32,
    #[serde(default)]
    pub losing: Option<u32>,
    /// Treasures held by objects (holder id, treasure).
    #[serde(default)]
    pub held: Vec<(u32, crate::objects::Treasure)>,
    /// Armour and rams stored at settlements (site id, (armour, rams)).
    #[serde(default)]
    pub fittings: Vec<(u32, crate::scene::load::Fittings)>,
    /// Ships reloading alongside a port, dock or ship.
    #[serde(default)]
    pub loads: Vec<crate::scene::LoadLink>,
    /// Heroes waiting at settlements (site id, heroes).
    #[serde(default)]
    pub site_heroes: Vec<(u32, Vec<u8>)>,
    /// The mission's brain.
    #[serde(default)]
    pub brain: Option<crate::scene::Brain>,
    /// The computer players of a battle or single mission.
    #[serde(default)]
    pub computers: Option<crate::scene::Computers>,
    /// The human player (nation, slot), when Custom missions' players
    /// made another one human than the mission's (0x53b940, 0x178).
    #[serde(default)]
    pub human: Option<(u32, u32)>,
    /// The human player's technology permissions.
    #[serde(default)]
    pub permits: Option<crate::permits::Permits>,
    /// The player's statistics.
    #[serde(default)]
    pub stats: crate::scene::Stats,
    /// Harpies and dragons out, and their nests' counts.
    #[serde(default)]
    pub flyers: Vec<crate::scene::Flyer>,
    #[serde(default)]
    pub nests: Vec<crate::scene::Nest>,
    /// Cyclops, minotaurs and amazons out.
    #[serde(default)]
    pub walkers: Vec<crate::scene::Walker>,
    #[serde(default)]
    pub lair_clocks: std::collections::BTreeMap<u32, f32>,
    /// What was going on that the scene keeps besides: walkways and
    /// porters, flames, refits, the Flame, repairs owed, clocks.
    #[serde(default)]
    pub planks: Vec<crate::scene::unload::SavedPlank>,
    #[serde(default)]
    pub porters: Vec<crate::scene::unload::SavedPorter>,
    #[serde(default)]
    pub unloaded: Vec<u32>,
    #[serde(default)]
    pub fires: Vec<crate::scene::fire::SavedFire>,
    #[serde(default)]
    pub refits: Vec<(u32, Upgrade, u8, f32)>,
    #[serde(default)]
    pub flame: f32,
    #[serde(default)]
    pub repair_owed: Vec<((u32, u32), f32)>,
    #[serde(default)]
    pub creature_clock: f32,
    #[serde(default)]
    pub gull_rng: Option<Rng>,
    #[serde(default)]
    pub last_attack: Option<(f32, f32)>,
    /// The brains' players' Flame (brewing, burning).
    #[serde(default)]
    pub brain_flames: Vec<((u32, u32), (f32, f32))>,
    /// Volcanoes' sleep and eruptions.
    #[serde(default)]
    pub volcanoes: std::collections::HashMap<u32, crate::scene::volcano::Volcano>,
    /// What was under way in battle: shots in flight, fireballs, lightning
    /// to strike, spells' sight, sirens' songs and charms, the Gorgons' and
    /// sea monsters' timing.
    #[serde(default)]
    pub battle: crate::scene::Battle,
    /// The play-through's id (game + 0x1a0d, the original's `ID_GAME`
    /// resource, written by 0x511926); the front end's, filled in by the
    /// app.
    #[serde(default)]
    pub game_id: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct SavedObject {
    pub id: u32,
    pub class_id: u32,
    pub fields: Vec<u32>,
    pub x: f32,
    pub y: f32,
    pub label: String,
    pub owner: Option<(u32, u32)>,
    pub ship: Option<Ship>,
    pub deposit: Option<Deposit>,
    #[serde(default)]
    pub dormant: Option<(Deposit, f32)>,
    pub combat: Option<Combatant>,
}

impl SaveGame {
    /// Writes the game, packed (gzip) when `pack` (the configuration's
    /// "pack save file": the original's saves compress their entries,
    /// 0x51f4c0 passing `[0x5ca791]` to its archive writer).
    pub fn write(&self, path: &Path, pack: bool) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec(self)?;
        let bytes = if pack {
            use std::io::Write;
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gz.write_all(&json)?;
            gz.finish()?
        } else {
            json
        };
        std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
    }

    /// Reads a saved game, packed or not.
    pub fn read(path: &Path) -> Result<Self> {
        let mut data =
            std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        if data.starts_with(&[0x1f, 0x8b]) {
            use std::io::Read;
            let mut json = Vec::new();
            flate2::read::GzDecoder::new(&data[..])
                .read_to_end(&mut json)
                .context("not a saved game")?;
            data = json;
        }
        let save: Self = serde_json::from_slice(&data).context("not a saved game")?;
        if save.version != VERSION {
            bail!("saved by an incompatible version ({})", save.version);
        }
        Ok(save)
    }
}

fn default_true() -> bool {
    true
}
