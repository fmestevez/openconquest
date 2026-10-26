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
use crate::scene::{Construction, Outcome, Production, Training};
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
    pub productions: Vec<Production>,
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
    /// Free casts from treasures, by spell index.
    #[serde(default)]
    pub free_casts: Vec<(u32, u32)>,
    /// The player's settlements at the mission's start (for its story).
    #[serde(default)]
    pub start_sites: Vec<u32>,
    /// Treasure kinds handed over at a settlement.
    #[serde(default)]
    pub delivered: Vec<u8>,
    /// Treasures held by objects (holder id, treasure).
    #[serde(default)]
    pub held: Vec<(u32, crate::objects::Treasure)>,
    /// Heroes waiting at settlements (site id, heroes).
    #[serde(default)]
    pub site_heroes: Vec<(u32, Vec<u8>)>,
    /// The mission's brain.
    #[serde(default)]
    pub brain: Option<crate::scene::Brain>,
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
    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_vec(self)?;
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))
    }

    pub fn read(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let save: Self = serde_json::from_slice(&data).context("not a saved game")?;
        if save.version != VERSION {
            bail!("saved by an incompatible version ({})", save.version);
        }
        Ok(save)
    }
}
