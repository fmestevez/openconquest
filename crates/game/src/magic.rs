//! Temple magic. A Temple of the Sea or of War learns spells one at a time
//! ("Get magic", menus 0x437de0 and 0x438b80) in a fixed order (0x596da8,
//! 0x596ea8); known spells can then be cast for amber and fish.

use crate::commands::Command;
use crate::economy::{TEMPLE_OF_THE_SEA, TEMPLE_OF_WAR};

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
/// Seconds a called wind blows. Placeholder.
pub const WIND_SECONDS: f32 = 60.0;
/// A fireball burns enemy objects within this radius for this long at this
/// many hull points a second. Placeholders.
pub const FIREBALL_RADIUS: f32 = 96.0;
pub const FIREBALL_SECONDS: f32 = 15.0;
pub const FIREBALL_DPS: f32 = 120.0;
/// Flame makes the player's weapons this much stronger for this long.
/// Placeholders.
pub const FLAME_FACTOR: f32 = 1.5;
pub const FLAME_SECONDS: f32 = 60.0;
/// View clears the clouds within this many cells. Placeholder.
pub const VIEW_RADIUS: i32 = 8;
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
/// A summoned sea monster: how long it stays and what it strikes with
/// (as four gastraphetes, which strike at close quarters). Placeholders.
pub const MONSTER_SECONDS: f32 = 90.0;
pub const MONSTER_WEAPON: u8 = 4;
pub const MONSTER_WEAPONS: usize = 4;

/// Effect animations.
pub const FX_LIGHTNING: &str = "SLIGHT1";
pub const FX_FLAME: &str = "FIRE0";
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

    pub fn start_learning(&mut self, temple: Temple) {
        self.learning[temple.slot()] = Some(0.0);
    }

    pub fn update(&mut self, dt: f32) {
        for (learning, known) in self.learning.iter_mut().zip(&mut self.known) {
            if let Some(t) = learning.as_mut() {
                *t += dt;
                if *t >= LEARN_SECONDS {
                    *learning = None;
                    *known += 1;
                }
            }
        }
    }
}
