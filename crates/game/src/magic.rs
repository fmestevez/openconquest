//! Temple magic. A Temple of the Sea or of War learns spells one at a time
//! ("Get magic", menus 0x437de0 and 0x438b80) in a fixed order (0x596da8,
//! 0x596ea8); known spells can then be cast for amber and fish.

use crate::commands::Command;
use crate::economy::{TEMPLE_OF_THE_SEA, TEMPLE_OF_WAR};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
                Spell::Tornado,
                Spell::Whirlpool,
                Spell::Reef,
                Spell::Monster,
            ],
            Temple::War => &[
                Spell::ProtectWeapons,
                Spell::Ghost,
                Spell::Flame,
                Spell::ProtectMagic,
                Spell::SmallLightning,
                Spell::Lightning,
            ],
        }
    }
}

/// Spells, in the original's spell index order (0x42fd70 gives each its
/// command).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spell {
    Wind,
    Reef,
    Whirlpool,
    Tornado,
    Monster,
    ViewMap,
    Lightning,
    SmallLightning,
    Flame,
    Ghost,
    ProtectWeapons,
    ProtectMagic,
}

impl Spell {
    const ALL: [Spell; 12] = [
        Spell::Wind,
        Spell::Reef,
        Spell::Whirlpool,
        Spell::Tornado,
        Spell::Monster,
        Spell::ViewMap,
        Spell::Lightning,
        Spell::SmallLightning,
        Spell::Flame,
        Spell::Ghost,
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
    pub fn implemented(self) -> bool {
        !matches!(
            self,
            Spell::Tornado | Spell::Monster | Spell::ViewMap | Spell::Ghost
        )
    }

    /// How the spell is aimed.
    pub fn target(self) -> Target {
        match self {
            Spell::Wind => Target::Direction,
            Spell::Reef | Spell::Whirlpool | Spell::Tornado | Spell::Monster => Target::Water,
            Spell::Lightning | Spell::SmallLightning | Spell::Flame => Target::Enemy,
            Spell::ProtectWeapons | Spell::ProtectMagic | Spell::Ghost => Target::OwnShip,
            Spell::ViewMap => Target::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Spell::Wind => "Call the wind",
            Spell::Reef => "Raise a reef",
            Spell::Whirlpool => "Summon a whirlpool",
            Spell::Tornado => "Tornado",
            Spell::Monster => "Sea monster",
            Spell::ViewMap => "View the map",
            Spell::Lightning => "Lightning",
            Spell::SmallLightning => "Small lightning",
            Spell::Flame => "Flame",
            Spell::Ghost => "Ghost",
            Spell::ProtectWeapons => "Protection from weapons",
            Spell::ProtectMagic => "Protection from magic",
        }
    }

    /// What to ask the player for when aiming it.
    pub fn prompt(self) -> &'static str {
        match self.target() {
            Target::Water => "Choose a place at sea",
            Target::Enemy => "Choose an enemy ship or building",
            Target::OwnShip => "Choose one of your ships",
            Target::Direction | Target::None => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    None,
    Direction,
    Water,
    Enemy,
    OwnShip,
}

/// Amber to learn the next spell (0x437de0, 0x438b80).
pub const LEARN_COST: u32 = 1000;
/// Seconds to learn a spell. Placeholder.
pub const LEARN_SECONDS: f32 = 20.0;
/// Seconds a called wind blows. Placeholder.
pub const WIND_SECONDS: f32 = 60.0;
/// Hull damage of a lightning strike and a small one. Placeholders.
pub const LIGHTNING_DAMAGE: f32 = 2000.0;
pub const SMALL_LIGHTNING_DAMAGE: f32 = 700.0;
/// A flame burns for this long at this many hull points a second.
/// Placeholders.
pub const FLAME_SECONDS: f32 = 15.0;
pub const FLAME_DPS: f32 = 60.0;
/// Seconds a protection lasts. Placeholder.
pub const PROTECTION_SECONDS: f32 = 60.0;

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
#[derive(Debug, Clone, Default)]
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

/// Damage per second a ship takes inside a whirlpool or on a reef.
/// Placeholder.
pub const WHIRLPOOL_DPS: f32 = 150.0;
pub const REEF_DPS: f32 = 60.0;
/// How close to a hazard's centre a ship is caught. Placeholder.
pub const HAZARD_RADIUS: f32 = 32.0;
