//! Temple magic. A Temple of the Sea or of War learns spells one at a time
//! ("Get magic", menus 0x437de0 and 0x438b80) in a fixed order (0x596da8,
//! 0x596ea8); known spells can then be cast for amber and fish.

use crate::commands::Command;
use crate::economy::{TEMPLE_OF_THE_SEA, TEMPLE_OF_WAR};
use crate::permits::Permits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Temple {
    Sea,
    War,
}

impl Temple {
    pub fn class(self) -> u32 {
        match self {
            Temple::Sea => TEMPLE_OF_THE_SEA,
            Temple::War => TEMPLE_OF_WAR,
        }
    }

    pub fn of_class(class: u32) -> Option<Self> {
        match class {
            TEMPLE_OF_THE_SEA => Some(Temple::Sea),
            TEMPLE_OF_WAR => Some(Temple::War),
            _ => None,
        }
    }

    fn slot(self) -> usize {
        self as usize
    }

    /// The order in which the temple learns its spells.
    pub fn order(self) -> &'static [Spell; 6] {
        match self {
            Temple::Sea => &[
                Spell::Wind,
                Spell::ViewMap,
                Spell::Waterspout,
                Spell::Whirlpool,
                Spell::Reef,
                Spell::Monster,
            ],
            Temple::War => &[
                Spell::ProtectWeapons,
                Spell::Phantom,
                Spell::Flame,
                Spell::ProtectMagic,
                Spell::Fireball,
                Spell::Lightning,
            ],
        }
    }
}

/// Spells, in the original's spell index order (0x42fd70 gives each its
/// command).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Spell {
    Wind,
    Reef,
    Whirlpool,
    Waterspout,
    Monster,
    ViewMap,
    Lightning,
    Fireball,
    Flame,
    Phantom,
    ProtectWeapons,
    ProtectMagic,
}

impl Spell {
    const ALL: [Spell; 12] = [
        Spell::Wind,
        Spell::Reef,
        Spell::Whirlpool,
        Spell::Waterspout,
        Spell::Monster,
        Spell::ViewMap,
        Spell::Lightning,
        Spell::Fireball,
        Spell::Flame,
        Spell::Phantom,
        Spell::ProtectWeapons,
        Spell::ProtectMagic,
    ];

    fn index(self) -> usize {
        self as usize
    }

    pub fn temple(self) -> Temple {
        if self.index() < 6 {
            Temple::Sea
        } else {
            Temple::War
        }
    }

    pub fn command(self) -> Command {
        const IDS: [u8; 12] = [
            0x56, 0x62, 0x61, 0x60, 0x63, 0x55, 0x69, 0x6a, 0x67, 0x66, 0x65, 0x68,
        ];
        Command(IDS[self.index()])
    }

    pub fn of_command(c: Command) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.command() == c)
    }

    /// Amber and fish to cast it (0x596844, 0x59687c x 100).
    pub fn cost(self) -> (u32, u32) {
        const AMBER: [u32; 12] = [
            400, 5000, 5000, 2000, 4000, 250, 10000, 3000, 6000, 1000, 1000, 1000,
        ];
        const FISH: [u32; 12] = [1, 12, 12, 5, 15, 0, 25, 7, 15, 1, 1, 1];
        (AMBER[self.index()], FISH[self.index()] * 100)
    }

    /// Whether this port casts it yet.
    /// The spell with index `i` (the original's numbering, as in the order
    /// of this enum).
    pub fn of_index(i: u32) -> Option<Spell> {
        Self::ALL.get(i as usize).copied()
    }

    pub fn implemented(self) -> bool {
        true
    }

    /// How far from one of the player's temples of its kind the spell
    /// reaches, in cells (the manual); `None` for anywhere.
    pub fn range(self) -> Option<i32> {
        match self {
            Spell::Wind | Spell::ViewMap | Spell::Flame => None,
            Spell::Reef
            | Spell::Whirlpool
            | Spell::Monster
            | Spell::Lightning
            | Spell::Fireball => Some(12),
            Spell::Waterspout | Spell::Phantom | Spell::ProtectWeapons | Spell::ProtectMagic => {
                Some(15)
            }
        }
    }

    /// How the spell is aimed.
    pub fn target(self) -> Target {
        match self {
            Spell::Wind => Target::Direction,
            Spell::Reef | Spell::Whirlpool | Spell::Waterspout | Spell::Monster => Target::Water,
            Spell::Lightning | Spell::Fireball | Spell::ViewMap => Target::Area,
            Spell::ProtectWeapons | Spell::ProtectMagic | Spell::Phantom => Target::OwnShip,
            Spell::Flame => Target::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Spell::Wind => "Call the wind",
            Spell::Reef => "Raise a reef",
            Spell::Whirlpool => "Summon a whirlpool",
            Spell::Waterspout => "Waterspout",
            Spell::Monster => "Sea monster",
            Spell::ViewMap => "View",
            Spell::Lightning => "Lightning",
            Spell::Fireball => "Fireball",
            Spell::Flame => "Flame",
            Spell::Phantom => "Phantom object",
            Spell::ProtectWeapons => "Weapon shield",
            Spell::ProtectMagic => "Magic shield",
        }
    }

    /// What to ask the player for when aiming it.
    pub fn prompt(self) -> &'static str {
        match self.target() {
            Target::Water => "Choose a place at sea",
            Target::Area => "Choose where to cast it",
            Target::OwnShip => "Choose one of your ships or buildings",
            Target::Direction | Target::None => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    None,
    Direction,
    Water,
    /// Any map cell.
    Area,
    /// One of the player's ships or buildings.
    OwnShip,
}

/// Amber to learn the next spell (0x437de0, 0x438b80).
pub const LEARN_COST: u32 = 1000;
/// Seconds to learn a spell: 200 ticks for the human, 100 for computer
/// players (the school, 0x439260).
pub const LEARN_SECONDS: f32 = 200.0 * crate::clock::TICK;
/// Flame makes the player's ships' weapons strike half again as hard
/// (0x4c0230: x 150 / 100 while player +0xa8 is set); it brews 250 ticks
/// at the Temple of War, then lights for 1500 ticks more each (0x439260,
/// player +0xb0).
pub const FLAME_FACTOR: f32 = 1.5;
pub const FLAME_SECONDS: f32 = 1500.0 * crate::clock::TICK;
pub const FLAME_BREW_SECONDS: f32 = 250.0 * crate::clock::TICK;
/// The shields' protection (the manual): weapon shield 80% against weapons;
/// magic shield neutralises lightning and 80% against fireballs,
/// waterspouts and sea monsters.
pub const SHIELD_FACTOR: f32 = 0.2;
/// Seconds a protection or phantom adds to what is left of it: 3000 ticks
/// (Magic.cpp 0x42f580; the ship adds them up on messages 0xda97 and
/// 0xda9a, 0x4dc630).
pub const PROTECTION_SECONDS: f32 = 3000.0 * crate::clock::TICK;

/// A tornado's blow to a ship on each cell it comes to: 30000 in the
/// original's units, a fifth of it on a ship shielded from magic (0x43a6c0,
/// 0x4dc630); over 15 in ours (see `combat::hull_strength`). The hull
/// takes its share.
pub const WATERSPOUT_STRIKE: f32 = 30000.0 / 15.0;
pub const FX_TORNADO: &str = "TRNDSPR";

/// Lightning's picture (LightningObjClassTy, `LTSPR_%1u`, 0x42cfa0): over
/// water `LTSPR_1`, over land `LTSPR_2`.
pub fn fx_lightning(water: bool) -> &'static str {
    if water {
        "LTSPR_1"
    } else {
        "LTSPR_2"
    }
}
/// Protection auras, for small and large ships.
pub fn fx_protection(spell: Spell, large: bool) -> String {
    let kind = if spell == Spell::ProtectWeapons {
        'W'
    } else {
        'M'
    };
    format!("PROT{kind}SPR_{}", if large { 128 } else { 64 })
}

/// Command to learn the next spell.
pub const GET_MAGIC: Command = Command(0x54);
/// Wind directions: command 0x58 + direction (0 east, counter-clockwise),
/// laid out as a compass around slot 5 (0x437de0, page 4).
pub const WIND_EAST: Command = Command(0x58);
pub const WIND_SLOTS: [usize; 8] = [6, 2, 1, 0, 4, 8, 9, 10];

/// What the human player's temples know.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Magic {
    /// How many spells of each temple's order are known.
    known: [usize; 2],
    /// Seconds spent learning the next spell, while learning.
    learning: [Option<f32>; 2],
    /// Seconds the Temple of War has brewed a Flame, while it does, and
    /// Flames brewed and not yet lit.
    #[serde(default)]
    flame_brew: Option<f32>,
    #[serde(default)]
    flames: u32,
    /// The temple (object id) each learning goes on at, and the Flame's:
    /// the work is the building's (+0x10c, +0x110, +0x124), standing still
    /// while it is limed (0x438300, 0x439260).
    #[serde(default)]
    at: [Option<u32>; 2],
    #[serde(default)]
    flame_at: Option<u32>,
}

impl Magic {
    pub fn known(&self, temple: Temple) -> &'static [Spell] {
        &temple.order()[..self.known[temple.slot()].min(6)]
    }

    pub fn knows(&self, spell: Spell) -> bool {
        self.known(spell.temple()).contains(&spell)
    }

    pub fn all_known(&self, temple: Temple) -> bool {
        self.known[temple.slot()] >= 6
    }

    pub fn learning(&self, temple: Temple) -> Option<f32> {
        self.learning[temple.slot()]
    }

    /// Drops any learning in progress.
    pub fn stop_learning(&mut self) {
        self.learning = [None; 2];
    }

    /// The temple a learning goes on at, if one was recorded.
    pub fn learning_site(&self, temple: Temple) -> Option<u32> {
        self.at[temple.slot()]
    }

    /// The temple a Flame brews at, if one was recorded.
    pub fn flame_site(&self) -> Option<u32> {
        self.flame_at
    }

    /// Calls off `temple`'s learning; the seconds it had gone on.
    pub fn cancel_learning(&mut self, temple: Temple) -> Option<f32> {
        self.learning[temple.slot()].take()
    }

    /// Calls off the Flame brewing; the seconds it had gone on.
    pub fn cancel_flame(&mut self) -> Option<f32> {
        self.flame_brew.take()
    }

    /// Starts learning the next spell of `temple` at the temple `site`.
    pub fn start_learning(&mut self, temple: Temple, site: Option<u32>) {
        self.learning[temple.slot()] = Some(0.0);
        self.at[temple.slot()] = site;
    }

    /// Starts brewing a Flame at the Temple of War `site` (its work 1,
    /// 0x439260: 250 ticks).
    pub fn brew_flame(&mut self, site: Option<u32>) {
        if self.flame_brew.is_none() {
            self.flame_brew = Some(0.0);
            self.flame_at = site;
        }
    }

    /// The lime on building `site` wore off (0x425990, before the temple's
    /// own tick in the same message): its work counts from when the lime
    /// struck, `since` ago (`+0x124` = `+0x14c`), and may end at once.
    pub fn lime_cleared(&mut self, site: u32, since: f32, permits: Permits) {
        for k in 0..2 {
            if self.at[k] == Some(site) && self.learning[k].is_some() {
                self.learning[k] = Some(since);
            }
        }
        if self.flame_at == Some(site) && self.flame_brew.is_some() {
            self.flame_brew = Some(since);
        }
        self.update(0.0, &[], permits);
    }

    /// Whether a Flame is brewing, and how far along (0-1).
    pub fn flame_brewing(&self) -> Option<f32> {
        self.flame_brew.map(|t| (t / FLAME_BREW_SECONDS).min(1.0))
    }

    /// Flames brewed since the last call: each lights the player's weapons
    /// for `FLAME_SECONDS` more.
    pub fn take_flames(&mut self) -> u32 {
        std::mem::take(&mut self.flames)
    }

    /// The temples' work, `dt` on; nothing goes on at a temple in
    /// `limed` (0x438300, 0x439260: `+0x144` set, they return).
    pub fn update(&mut self, dt: f32, limed: &[u32], permits: Permits) {
        self.update_learning(dt, limed, permits);
        self.update_flame(dt, limed);
    }

    /// The temples' learning, `dt` on (computer players learn twice as
    /// fast, 100 ticks; the Flame brews 250 ticks for every player,
    /// 0x439260).
    pub fn update_learning(&mut self, dt: f32, limed: &[u32], permits: Permits) {
        for temple in [Temple::Sea, Temple::War] {
            let k = temple.slot();
            if self.at[k].is_some_and(|s| limed.contains(&s)) {
                continue;
            }
            let Some(t) = self.learning[k].as_mut() else {
                continue;
            };
            *t += dt;
            if *t >= LEARN_SECONDS {
                self.learning[k] = None;
                self.known[k] += 1;
                // Then the spells whose permission is withheld are passed
                // over: while fewer than six, the spell at 0x596da4 (0x596ea4)
                // + known x 4, one before the order's own entry (0x596da8),
                // is tested; so the spell just learned, then each one
                // passed over (0x438300, 0x439260 by the bits at player
                // +0x45, 0x42fd70).
                let order = temple.order();
                while self.known[k] < 6 && !permits.spell(order[self.known[k] - 1]) {
                    self.known[k] += 1;
                }
            }
        }
    }

    /// The Flame brewing, `dt` on.
    pub fn update_flame(&mut self, dt: f32, limed: &[u32]) {
        if self.flame_at.is_some_and(|s| limed.contains(&s)) {
            return;
        }
        if let Some(t) = self.flame_brew.as_mut() {
            *t += dt;
            if *t >= FLAME_BREW_SECONDS {
                self.flame_brew = None;
                self.flames += 1;
            }
        }
    }
}
