//! Object commands and the command panel menus.
//!
//! The object panel shows a grid of 12 command buttons (4 x 3). The selected
//! object fills the menu when asked with message 0x10a0; each slot is a
//! command id and a state (enabled or not). Command ids and their button
//! images come from 0x4f08a0; the ship menus from 0x4e6e90 (merchants) and
//! 0x4e6980 (warships), with per-type removals in 0x4e42e0; the shipyard
//! menu from 0x4164e0 and the settlement menu from 0x43c740.

use openconquest_formats::ObjectClass;

use crate::economy::{
    self, DOCK, SCHOOL, SETTLEMENT, SHIPYARD, SMITHY, TEMPLE_OF_THE_SEA, TEMPLE_OF_WAR, TOWN,
    WORKSHOP,
};
use crate::magic::{self, Spell, Temple};
use crate::objects::{MapObject, SHIP_NAMES};
use crate::scene::Scene;

pub const SLOTS: usize = 12;

/// A command id as used by the original game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Command(pub u8);

impl Command {
    pub const STOP: Command = Command(0x01);
    pub const FIRE: Command = Command(0x03);
    pub const MOVE: Command = Command(0x04);
    pub const LOAD: Command = Command(0x05);
    pub const REPAIR_SHIP: Command = Command(0x06);
    pub const BUILD_GROUND: Command = Command(0x07);
    pub const TREASURE: Command = Command(0x08);
    pub const BOARD: Command = Command(0x09);
    pub const RAM: Command = Command(0x0a);
    pub const UNLOAD: Command = Command(0x0b);
    pub const ZONES: Command = Command(0x0c);
    pub const FISH: Command = Command(0x0d);
    pub const AMBER: Command = Command(0x0e);
    pub const WHALE: Command = Command(0x0f);
    pub const BUILD_STRUCTURES: Command = Command(0x10);
    pub const REPAIR_BUILDING: Command = Command(0x11);
    pub const SELL_ITEM: Command = Command(0x12);
    /// A workshop's settlement mends its stock's worn units.
    pub const REPAIR_STOCK: Command = Command(0x15);
    pub const SELL_TOWN: Command = Command(0x80);
    pub const TRAIN_BOWMEN: Command = Command(0x2d);
    pub const UPGRADE_TOWN: Command = Command(0x3a);
    pub const UPGRADE_DOCK: Command = Command(0x3b);
    /// Build the Argo (shipyards, once Argus has come).
    pub const BUILD_ARGO: Command = Command(0x7d);
    pub const BEHAVIOUR: Command = Command(0x6c);

    /// Name of the button image (`<name>_00`, `<name>_01`, ...). Build and
    /// upgrade buttons are composed as `CBUT_BLD<nation digit><suffix>`.
    pub fn button(self, nation: u32) -> Option<String> {
        let fixed = match self.0 {
            0x01 => "CBUT_STOP",
            0x02 => "CBUT_BREAK",
            0x03 => "CBUT_FIRE",
            0x04 => "CBUT_MOVE",
            0x05 => "CBUT_LOAD",
            0x06 => "CBUT_REPSHIP",
            0x07 => "CBUT_BUILDGRS",
            0x08 => "CBUT_TREAS",
            0x09 => "CBUT_BOARD",
            0x0a => "CBUT_RAM",
            0x0b => "CBUT_UNLOAD",
            0x0c | 0x0d => "CBUT_FISH",
            0x0e => "CBUT_AMBER",
            0x0f => "CBUT_WHALE",
            0x10 => "CBUT_BUILDGRG",
            0x11 | 0x14 => "CBUT_REPGRND",
            0x12 => "CBUT_SELL",
            0x15 => "CBUT_REPARMING",
            0x7e => "CBUT_SELLFISH",
            0x80 => "CBUT_SELLTOWN",
            0x6c => "CBUT_BEHMAIN",
            0x6d => "CBUT_BEHCONVEY",
            0x6e => "CBUT_BEHPATROL",
            0x6f => "CBUT_BEHSGUARD",
            0x70 => "CBUT_BEHFGUARD",
            0x71 => "CBUT_BEHRAID",
            0x72 => "CBUT_BEHSAGR",
            0x73 => "CBUT_BEHFAGR",
            0x74 => "CBUT_BEHGZONE",
            0x75 => "CBUT_BEHAZONE",
            0x76 => "CBUT_BEHSCOUT",
            0x78 => "CBUT_BEHGUARDBRD",
            0x79 => "CBUT_BEHGUARDRAM",
            0x7a => "CBUT_BEHAMBERZONE",
            0x7b => "CBUT_BEHFISHZONE",
            0x7c => "CBUT_BEHWHALEZONE",
            0x36 => "CBUT_UPARMORM",
            0x37 => "CBUT_UPARMORS",
            0x38 => "CBUT_UPRAMM",
            0x39 => "CBUT_UPRAMS",
            0x3c | 0x3d => "CBUT_UPARMING",
            0x50 => "CBUT_UPGFIRE",
            0x51 => "CBUT_UPQLIME",
            0x52 => "CBUT_UPFRAM",
            0x53 => "CBUT_UPDEAD",
            0x3e => "CBUT_UPGARMINGW3",
            0x3f => "CBUT_UPGARMINGW1",
            0x40 => "CBUT_UPGARMINGW2",
            0x41 => "CBUT_UPGARMINGM3",
            0x42 => "CBUT_UPGARMINGM1",
            0x43 => "CBUT_UPGARMINGM2",
            0x44 => "CBUT_UPGARMINGS3",
            0x45 => "CBUT_UPGARMINGS1",
            0x46 => "CBUT_UPGARMINGS2",
            0x47 => "CBUT_UPARMOR",
            0x48 => "CBUT_UPRAM",
            0x4f => "CBUT_MOVE",
            0x65 => "CBUT_PROTWEAPON",
            0x66 => "CBUT_GHOST",
            0x67 => "CBUT_FLAME",
            0x68 => "CBUT_PROTMAGIC",
            0x69 => "CBUT_LIGHTNING",
            0x6a => "CBUT_SLIGHTNING",
            0x54 => "CBUT_GETMAGIC",
            0x55 => "CBUT_VIEWMAP",
            0x56..=0x5f => "CBUT_WIND",
            0x60 => "CBUT_TORNADO",
            0x61 => "CBUT_WHIRL",
            0x62 => "CBUT_SREEF",
            // The sea monster button shows the nation's monster.
            0x63 => return Some(format!("CBUT_MONSTER{}", nation.min(3))),
            _ => {
                let suffix = match self.0 {
                    0x16 => "TOWN",
                    0x17 => "FORTW",
                    0x18 => "FORTM",
                    0x19 => "FORTS",
                    0x1a => "BERTH",
                    0x1b => "DOCK",
                    0x1c => "STUDIO",
                    0x1d => "SMITHY",
                    0x1e => "SCHOOL",
                    0x1f => "WATER",
                    0x20 => "WAR",
                    0x21 => "BOAT",
                    0x22 => "TRADE",
                    0x23 => "MKILL",
                    0x24 => "SKILL",
                    0x25 => "BOARDER",
                    0x26 => "RAMMER",
                    0x27 => "BOMBER",
                    0x28 => "KSHIP",
                    0x29 | 0x2b => "SPEC1",
                    0x2a | 0x2c => "SPEC2",
                    0x2d => "ONION",
                    0x2e => "DART",
                    0x2f => "SWORD",
                    0x30 => return Some("CBUT_BLDTRANS".into()),
                    0x31 => return Some("CBUT_BLDGASTR".into()),
                    0x32 => return Some("CBUT_BLDSMALKAT".into()),
                    0x33 => return Some("CBUT_BLDBALIST".into()),
                    0x34 => return Some("CBUT_BLDBIGKAT".into()),
                    0x35 => return Some("CBUT_BLDONAGR".into()),
                    0x3a => "UPGTOWN",
                    0x3b => "UPGDOCK",
                    0x7d => "ARGO",
                    _ => return None,
                };
                let digit = if nation == 3 { 3 } else { 1 };
                return Some(format!("CBUT_BLD{digit}{suffix}"));
            }
        };
        Some(fixed.to_string())
    }

    /// The name and, for things that cost amber or fish, the price (for
    /// research, at the human player's level in `scene`).
    pub fn describe(self, scene: Option<&Scene>) -> String {
        let cost = if let Some(kind) = economy::ship_of_command(self) {
            Some(economy::ship_cost(kind))
        } else if let Some(class) = economy::building_of_command(self) {
            Some((economy::building_cost(class), 0))
        } else if (0x2d..=0x35).contains(&self.0) {
            Some(crate::combat::item_cost(self.0 - 0x2d))
        } else if let Some(spell) = Spell::of_command(self) {
            Some(spell.cost())
        } else if let Some(upgrade) = crate::combat::Upgrade::of_command(self.0) {
            let level = scene.and_then(|s| s.next_level(upgrade)).unwrap_or(0);
            Some((upgrade.cost(level), 0))
        } else {
            match self {
                Command::UPGRADE_TOWN => Some((economy::building_cost(TOWN), 0)),
                Command::UPGRADE_DOCK => Some((economy::building_cost(DOCK), 0)),
                magic::GET_MAGIC => Some((magic::LEARN_COST, 0)),
                _ => None,
            }
        };
        match cost {
            Some((amber, 0)) => format!("{}: {amber} amber", self.name()),
            Some((amber, fish)) => format!("{}: {amber} amber, {fish} fish", self.name()),
            None => self.name(),
        }
    }

    /// The id of the caption on this command's button in the game's string
    /// table (0x4f21b0), e.g. " \nMOVE": two lines, centred, the first
    /// often blank.
    pub fn caption_id(self) -> u32 {
        match self.0 {
            0x01 | 0x02 => 0x1a1,
            0x03 => 0x1a3,
            0x04 => 0x1a2,
            0x05 => 0x1ad,
            0x06 => 0x1b0,
            0x07 | 0x10 => 0x1ae,
            0x08 => 0x1ea,
            0x09 => 0x1a7,
            0x0a => 0x1a8,
            0x0b => 0x1b1,
            0x0c => 0x1a9,
            0x0d | 0x7b => 0x1aa,
            0x0e | 0x7a => 0x1ab,
            0x0f | 0x7c => 0x1ac,
            0x11 => 0x208,
            0x12 => 0x1ed,
            0x13 => 0x1af,
            0x14 => 0x209,
            0x15 => 0x1ee,
            0x16 => 0x1b2,
            0x17..=0x19 => 0x1b3,
            0x1a => 0x1b4,
            0x1b => 0x1b5,
            0x1c => 0x1b6,
            0x1d => 0x1b7,
            0x1e => 0x1b8,
            0x1f => 0x1b9,
            0x20 => 0x1ba,
            0x21 => 0x1bb,
            0x22 => 0x1bc,
            0x23 => 0x1be,
            0x24 => 0x1bd,
            0x25 => 0x1f8,
            0x26 => 0x1f9,
            0x27 => 0x1fc,
            0x28 => 0x1bf,
            0x29 => 0x1f6,
            0x2a => 0x1fa,
            0x2b => 0x1f7,
            0x2c => 0x1fb,
            0x2d => 0x1c3,
            0x2e => 0x1c5,
            0x2f => 0x1c4,
            0x30 => 0x1c6,
            0x31 => 0x1c7,
            0x32 => 0x1c8,
            0x33 => 0x1c9,
            0x34 => 0x1ca,
            0x35 => 0x1cb,
            0x36 => 0x1cc,
            0x37 => 0x1cd,
            0x38 => 0x1ce,
            0x39 => 0x1cf,
            0x3a => 0x1d0,
            0x3b => 0x1d2,
            0x3c | 0x3d => 0x1d3,
            0x3e | 0x41 | 0x44 => 0x1d4,
            0x3f | 0x42 | 0x45 => 0x1d6,
            0x40 | 0x43 | 0x46 => 0x1d5,
            0x47 | 0x49 | 0x4a | 0x4b => 0x1d7,
            0x48 | 0x4c | 0x4d | 0x4e => 0x1d8,
            0x4f => 0x1eb,
            0x50 => 0x1fd,
            0x51 => 0x1ff,
            0x52 => 0x1fe,
            0x53 => 0x200,
            0x54 => 0x201,
            0x55 => 0x1f4,
            0x56 | 0x58 | 0x59 | 0x5a | 0x5b | 0x5c | 0x5d | 0x5e | 0x5f => 0x1e5,
            0x57 => 0x1e6,
            0x60 => 0x1dc,
            0x61 => 0x1db,
            0x62 => 0x1dd,
            0x63 => 0x1e7,
            0x64 => 0x1e4,
            0x65 => 0x1e8,
            0x66 => 0x1e3,
            0x67 => 0x1de,
            0x68 => 0x1e9,
            0x69 => 0x1e1,
            0x6a => 0x1e2,
            0x6b => 0x202,
            0x6c => 0x1ec,
            0x6d => 0x1ef,
            0x6e => 0x1f1,
            0x6f | 0x70 => 0x1f2,
            0x71 => 0x1f3,
            0x72 | 0x73 => 0x205,
            0x74 => 0x204,
            0x75 => 0x203,
            0x76 => 0x206,
            0x77 => 0x207,
            0x78 => 0x20a,
            0x79 => 0x20b,
            0x7d => 0x1c0,
            0x7e => 0x20c,
            0x7f => 0x20d,
            0x80 => 0x20e,
            _ => 0x28,
        }
    }

    /// Whether the button opens a page of further commands (drawn with an
    /// arrow, `CBUT_SPEC`).
    pub fn opens_page(self) -> bool {
        matches!(self.0, 0x07 | 0x0c | 0x10 | 0x3d | 0x56 | 0x6c)
    }

    pub fn name(self) -> String {
        if let Some(kind) = economy::ship_of_command(self) {
            return format!("Build {}", SHIP_NAMES[kind as usize]);
        }
        if let Some(class) = economy::building_of_command(self) {
            return format!("Build {}", crate::objects::building_label(class));
        }
        match self.0 {
            0x01 => "Stop",
            0x02 => "Break off",
            0x03 => "Fire",
            0x04 => "Move",
            0x05 => "Load",
            0x06 => "Repair",
            0x07 => "Build",
            0x08 => "Treasure",
            0x09 => "Board",
            0x0a => "Ram",
            0x0b => "Unload",
            0x0c => "Gathering zones",
            0x0d => "Fish",
            0x0e => "Collect amber",
            0x0f => "Hunt leviathan",
            0x10 => "Build structures",
            0x11 => "Repair",
            0x15 => "Repair arming",
            0x2d..=0x2f => {
                return format!(
                    "Train {}",
                    crate::combat::CREW_NAMES[(self.0 - 0x2d) as usize]
                )
            }
            0x30..=0x35 => {
                return format!(
                    "Build a {}",
                    crate::combat::ITEM_NAMES[(self.0 - 0x2d) as usize]
                )
            }
            0x36 | 0x37 => return format!("Fit armour, level {}", self.0 - 0x35),
            0x38 | 0x39 => return format!("Fit a ram, level {}", self.0 - 0x37),
            0x3c => "Study arming",
            0x3d => "Weapon research",
            0x3e..=0x46 => {
                return crate::combat::Upgrade::of_command(self.0)
                    .map_or("Research".into(), |u| format!("Improve the {}", u.name()))
            }
            0x47 => "Study better armour",
            0x48 => "Study better rams",
            0x4f => "Study better rigging",
            0x50..=0x53 => {
                return crate::combat::Upgrade::of_command(self.0)
                    .map_or("Research".into(), |u| format!("Study {}", u.name()))
            }
            0x54 => "Learn a spell",
            0x57 => "Back",
            0x58..=0x5f => {
                const DIRS: [&str; 8] = [
                    "east",
                    "north-east",
                    "north",
                    "north-west",
                    "west",
                    "south-west",
                    "south",
                    "south-east",
                ];
                return format!("Wind to the {}", DIRS[(self.0 - 0x58) as usize]);
            }
            0x55 | 0x56 | 0x60..=0x63 | 0x65..=0x6a => {
                return Spell::of_command(self)
                    .map_or("Spell", |s| s.name())
                    .to_string()
            }
            0x3a => "Upgrade to town",
            0x3b => "Upgrade to dock",
            0x6c => "Behaviour",
            0x6d => "Convoy",
            0x6e => "Patrol",
            0x6f | 0x70 => "Guard",
            0x71 => "Raid",
            0x72 | 0x73 => "Fire on aggressor",
            0x74 => "Guard zone",
            0x75 => "Attack zone",
            0x76 => "Scout",
            0x78 => "Board attack",
            0x79 => "Ram attack",
            0x7a => "Amber zone",
            0x7b => "Fish zone",
            0x7c => "Leviathan zone",
            0x7d => "Build the Argo",
            _ => "Command",
        }
        .to_string()
    }
}

/// One slot of the command grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub command: Command,
    pub enabled: bool,
    /// Progress 0..1 of an order running on this button (e.g. a ship being
    /// built); shown by the button's animation frames.
    pub progress: Option<f32>,
    /// Items ordered on this button and not yet made (0x4f2a90 draws the
    /// count at (3, 3) on buttons 0x21-0x39).
    pub count: u32,
    /// A behaviour switched on (drawn highlighted).
    pub checked: bool,
    /// Closes the page it is on (drawn with a red cross).
    pub closes: bool,
}

pub type Menu = [Option<Slot>; SLOTS];

/// Which of an object's menus is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Main,
    /// A settlement's buildings to put up.
    Structures,
    /// A temple's wind directions.
    Wind,
    /// A workshop's research.
    Research,
    /// A warship's behaviour (0x4e6980, page 5).
    Behaviour,
    /// A merchant's gathering zones (0x4e6e90, page 1).
    Zones,
    /// A ship's buildings: a settlement and forts (0x4e6980, page 2).
    Ground,
}

/// The command menu of an object on `page`.
pub fn menu(scene: &Scene, obj: &MapObject, page: Page) -> Menu {
    match page {
        Page::Main => main_menu(scene, obj),
        Page::Structures => structures_menu(scene, obj),
        Page::Wind => wind_menu(scene, obj),
        Page::Research => research_menu(scene, obj),
        Page::Behaviour => behaviour_menu(scene, obj),
        Page::Zones => zones_menu(scene, obj),
        Page::Ground => ground_menu(scene, obj),
    }
}

/// A ship's building page (0x4e6980, page 2): a settlement, then the forts
/// the player may build, packed in that order.
fn ground_menu(scene: &Scene, obj: &MapObject) -> Menu {
    let mut menu: Menu = [None; SLOTS];
    if !scene.is_own(obj) || obj.ship.is_none() {
        return menu;
    }
    let mut slot = 0;
    for id in [0x16, 0x17, 0x18, 0x19] {
        let command = Command(id);
        let class = economy::building_of_command(command).unwrap();
        if id != 0x16 && !scene.permits.building(class) {
            continue;
        }
        let cost = if id == 0x16 {
            economy::building_cost(class)
        } else {
            let home = scene
                .object_index(obj.id)
                .and_then(|i| scene.home_settlement(i));
            match scene.fort_price(home, class) {
                Some(p) => p.amber,
                None => continue,
            }
        };
        set(&mut menu, slot, command, scene.amber >= cost);
        slot += 1;
    }
    menu
}

/// The Temple of the Sea's compass (0x437de0, page 4): a direction button
/// around the centre, which goes back.
fn wind_menu(scene: &Scene, obj: &MapObject) -> Menu {
    let mut menu: Menu = [None; SLOTS];
    if !scene.is_own(obj) || obj.class != ObjectClass::Building(TEMPLE_OF_THE_SEA) {
        return menu;
    }
    let (amber, fish) = Spell::Wind.cost();
    let affordable = scene.amber >= amber && scene.fish >= fish;
    for (dir, &slot) in magic::WIND_SLOTS.iter().enumerate() {
        set(
            &mut menu,
            slot,
            Command(magic::WIND_EAST.0 + dir as u8),
            affordable,
        );
    }
    set(&mut menu, 5, Command(0x57), true);
    menu
}

/// A temple (Sea 0x437de0, War 0x438b80): learn the next spell, then one
/// button per known spell.
fn temple_menu(scene: &Scene, temple: Temple, menu: &mut Menu) {
    let mut slot = 0;
    // The next spell, if the campaign has revealed it yet.
    let next = temple.order().get(scene.magic.known(temple).len()).copied();
    if next.is_some_and(|s| scene.permits.spell(s)) {
        let learning = scene.magic.learning(temple);
        menu[0] = Some(Slot {
            checked: false,
            closes: false,
            command: magic::GET_MAGIC,
            enabled: learning.is_some() || scene.amber >= magic::LEARN_COST,
            progress: learning.map(|t| t / magic::LEARN_SECONDS),
            count: 0,
        });
        slot = 1;
    }
    // Known spells, then any the player may cast once from a treasure.
    let free = |s: Spell| scene.free_casts.get(&s).is_some_and(|&n| n > 0);
    let gifts = scene
        .free_casts
        .keys()
        .copied()
        .filter(|&s| s.temple() == temple && free(s) && !scene.magic.knows(s));
    let spells: Vec<Spell> = scene
        .magic
        .known(temple)
        .iter()
        .copied()
        .chain(gifts)
        .collect();
    for spell in spells {
        let (amber, fish) = spell.cost();
        let enabled =
            spell.implemented() && (free(spell) || scene.amber >= amber && scene.fish >= fish);
        if slot < SLOTS {
            set(menu, slot, spell.command(), enabled);
            // A brewing Flame shows its progress, and waits (0x439260).
            if spell == Spell::Flame {
                if let (Some(p), Some(b)) = (scene.magic.flame_brewing(), menu[slot].as_mut()) {
                    b.progress = Some(p);
                    b.enabled = false;
                }
            }
        }
        slot += 1;
    }
}

/// Buildings a settlement or town can put up (0x43c740, page 2): those the
/// player may build, packed in the original's order (port, the forts,
/// workshop, shipyard, smithy, school, the temples). Esc returns to the main
/// menu.
fn structures_menu(scene: &Scene, obj: &MapObject) -> Menu {
    const ORDER: [u8; 10] = [0x1a, 0x17, 0x18, 0x19, 0x1c, 0x1b, 0x1d, 0x1e, 0x1f, 0x20];
    let mut menu: Menu = [None; SLOTS];
    if !scene.is_own(obj) || !matches!(obj.class, ObjectClass::Building(SETTLEMENT | TOWN)) {
        return menu;
    }
    let mut slot = 0;
    for id in ORDER {
        let command = Command(id);
        let class = economy::building_of_command(command).unwrap();
        if !scene.permits.building(class) {
            continue;
        }
        // Forts only when their weapons can be had (with default arming;
        // 0x43d430), the stone fort only at a town.
        let cost = if (0x34..=0x36).contains(&class) {
            if class == 0x36 && obj.class != ObjectClass::Building(TOWN) {
                continue;
            }
            match scene.fort_price(Some(obj.id), class) {
                Some(p) => (p.amber, p.fish),
                None => continue,
            }
        } else {
            (economy::building_cost(class), 0)
        };
        set(
            &mut menu,
            slot,
            command,
            scene.amber >= cost.0 && scene.fish >= cost.1,
        );
        slot += 1;
    }
    // The page's own button, bottom right, closes it.
    menu[SLOTS - 1] = Some(Slot {
        command: Command::BUILD_STRUCTURES,
        enabled: true,
        progress: None,
        count: 0,
        checked: false,
        closes: true,
    });
    menu
}

/// The main command menu of an object, laid out as the original does.
/// Only the human player's objects have commands. Commands that are not
/// implemented yet are shown disabled.
pub fn main_menu(scene: &Scene, obj: &MapObject) -> Menu {
    let mut menu: Menu = [None; SLOTS];
    if !scene.is_own(obj) {
        return menu;
    }
    if let Some(c) = scene.construction_at(obj.id) {
        // A town or dock going up from a settlement or shipyard shows the
        // work on its upgrade button (0x43c740 slot 4, 0x4164e0 slot 11).
        let slot = match obj.class {
            ObjectClass::Building(TOWN) => Some((4, Command::UPGRADE_TOWN)),
            ObjectClass::Building(DOCK) => Some((11, Command::UPGRADE_DOCK)),
            _ => None,
        };
        if let Some((k, command)) = slot {
            menu[k] = Some(Slot {
                checked: false,
                closes: false,
                command,
                enabled: true,
                progress: Some((c.elapsed / c.total).min(1.0)),
                count: 0,
            });
        }
        return menu;
    }
    match obj.class {
        ObjectClass::Ship => ship_menu(obj, &mut menu),
        ObjectClass::Building(SHIPYARD | DOCK) => shipyard_menu(scene, obj, &mut menu),
        ObjectClass::Building(SETTLEMENT | TOWN) => settlement_menu(scene, obj, &mut menu),
        ObjectClass::Building(WORKSHOP) => workshop_menu(scene, obj, &mut menu),
        ObjectClass::Building(SCHOOL) => school_menu(scene, obj, &mut menu),
        ObjectClass::Building(SMITHY) => smithy_menu(scene, obj, &mut menu),
        ObjectClass::Building(c @ (TEMPLE_OF_THE_SEA | TEMPLE_OF_WAR)) => {
            temple_menu(scene, Temple::of_class(c).unwrap(), &mut menu)
        }
        _ => {}
    }
    menu
}

fn set(menu: &mut Menu, slot: usize, command: Command, enabled: bool) {
    menu[slot] = Some(Slot {
        checked: false,
        closes: false,
        command,
        enabled,
        progress: None,
        count: 0,
    });
}

fn ship_menu(obj: &MapObject, menu: &mut Menu) {
    let Some(ship) = obj.ship.as_ref() else {
        return;
    };
    let kind = ship.kind;
    // Commands that are not implemented yet are shown disabled.
    set(menu, 0, Command::MOVE, true);
    set(menu, 1, Command::STOP, true);
    if kind <= 1 {
        // Merchants (0x4e6e90): cargo and gathering.
        // Unload resource (0x4e6e90 slot 2): to one of the player's ports
        // or settlements, once there are goods aboard.
        set(menu, 2, Command::UNLOAD, ship.cargo >= 1.0);
        set(menu, 4, Command::AMBER, true);
        set(menu, 5, Command::FISH, true);
        set(menu, 6, Command::WHALE, true);
        set(menu, 7, Command::ZONES, true);
    } else {
        // Warships (0x4e6980): weapons and behaviour.
        set(menu, 4, Command::FIRE, obj.is_armed());
        set(menu, 5, Command::BOARD, ship.crew.count() > 0);
        set(menu, 6, Command::RAM, true);
        set(menu, 7, Command::BEHAVIOUR, true);
    }
    set(menu, 8, Command::LOAD, true);
    let damaged = obj.combat.as_ref().is_some_and(|c| c.hull < c.max_hull);
    set(menu, 9, Command::REPAIR_SHIP, damaged);
    // Every ship's capabilities allow building (0x4e6930 clears other bits).
    set(menu, 10, Command::BUILD_GROUND, true);
    set(menu, 11, Command::TREASURE, true);
    // Unique ships lose some commands (0x4e42e0).
    let removed: &[usize] = match kind {
        8 => &[5, 6, 11],
        9 => &[6, 11],
        10 => &[2, 3, 4, 5, 8, 10, 11],
        11 => &[2, 3, 6, 8, 10, 11],
        _ => &[],
    };
    for &slot in removed {
        menu[slot] = None;
    }
}

/// Shipyard and dock (0x4164e0): one button per ship type the nation can
/// build, then the upgrade to a dock.
fn shipyard_menu(scene: &Scene, obj: &MapObject, menu: &mut Menu) {
    let Some(human) = scene.human else { return };
    let building = scene.production_at(obj.id);
    // A ship type's slot is its number (0x42d750 gives slot n the command
    // of type n: boat, merchant, royal ship, bomber, boarder, rammer, ship
    // killer, men killer), the unique ships packed after; locked ones are
    // left out (0x43da20).
    let mut next = 8;
    for kind in 0u8..12 {
        let place = kind as usize;
        if !economy::buildable_ships(human.nation).any(|k| k == kind) || !scene.ship_unlocked(kind)
        {
            continue;
        }
        let slot = if place < 8 {
            place
        } else {
            next += 1;
            next - 1
        };
        let command = economy::ship_command(kind);
        let mut entry = Slot {
            checked: false,
            closes: false,
            command,
            // More may be ordered while one is built: they wait their turn
            // (0x4164e0 counts them on the button, 0x416f90).
            enabled: scene.can_build_ship(kind).is_ok() && {
                let p = scene.ship_price(obj.id, kind);
                scene.amber >= p.amber && scene.fish >= p.fish
            },
            progress: None,
            count: scene.ships_queued(obj.id, kind, false),
        };
        if let Some(p) = building.filter(|p| p.ship == kind && !p.argo) {
            entry.progress = Some(p.elapsed / p.total);
        }
        menu[slot] = Some(entry);
    }
    // The Argo, after the ship types (0x4164e0), once Argus has come.
    if scene.permits.argo() && crate::scene::is_campaign(&scene.mission) {
        let slot = next.min(10);
        let mut entry = Slot {
            checked: false,
            closes: false,
            command: Command::BUILD_ARGO,
            enabled: scene.ships_queued(obj.id, Scene::ROYAL_SHIP, true) == 0
                && scene.can_build_ship(2).is_ok(),
            progress: None,
            count: 0,
        };
        if let Some(p) = building.filter(|p| p.argo) {
            entry.progress = Some(p.elapsed / p.total);
            entry.enabled = true;
        }
        menu[slot] = Some(entry);
    }
    if obj.class == ObjectClass::Building(SHIPYARD) && scene.permits.dock() {
        let enabled = building.is_none() && scene.amber >= economy::building_cost(DOCK);
        set(menu, 11, Command::UPGRADE_DOCK, enabled);
    }
}

/// The smithy (0x433e60): refits a ship lying next to it with armour M/S
/// or ram M/S (slots 0, 1 and 4, 5), up to the studied level.
fn smithy_menu(scene: &Scene, obj: &MapObject, menu: &mut Menu) {
    let studied = scene.levels_of(scene.human.map(|h| (h.nation, h.slot)));
    for (slot, command) in [(0, 0x36u8), (1, 0x37), (4, 0x38), (5, 0x39)] {
        let (upgrade, level) = crate::combat::refit_of_command(command).unwrap();
        if studied.get(upgrade) < level || !scene.permits.fit(upgrade, level) {
            continue;
        }
        let affordable = scene
            .refit_candidate(obj.id, upgrade, level)
            .is_some_and(|i| {
                let kind = scene.objects[i].ship.as_ref().unwrap().kind;
                scene.amber >= crate::combat::refit_cost(upgrade, level, kind)
            });
        set(menu, slot, Command(command), affordable);
    }
}

/// The sage school (0x430f90): weapons (slot 0), rams (1), armour (2) and
/// rigging (3), each studied up to level 2; the one in progress shows its
/// progress. (Greek schools also offer the special-ship upgrades, not
/// implemented.)
fn school_menu(scene: &Scene, obj: &MapObject, menu: &mut Menu) {
    use crate::combat::Upgrade;
    let busy = scene.research_at(obj.id);
    // Slots 0-3: arming, ram, armour, rigging; once the arming is at level
    // 2, the nation's two unique ships in slots 4 and 5.
    let levels = scene.levels_of(scene.human.map(|h| (h.nation, h.slot)));
    let first = if scene.human.is_some_and(|h| h.nation == 1) {
        0
    } else {
        2
    };
    let specials = (levels.weapons >= 1)
        .then(|| {
            [
                (4, Upgrade::Special(first)),
                (5, Upgrade::Special(first + 1)),
            ]
        })
        .into_iter()
        .flatten();
    for (slot, upgrade) in Upgrade::ALL.into_iter().enumerate().chain(specials) {
        let entry = match (busy, scene.next_level(upgrade)) {
            (Some((u, progress)), _) if u == upgrade => Slot {
                checked: false,
                closes: false,
                command: Command(upgrade.command()),
                enabled: true,
                progress: Some(progress),
                count: 0,
            },
            (_, Some(level)) => Slot {
                checked: false,
                closes: false,
                command: Command(upgrade.command()),
                enabled: busy.is_none() && scene.amber >= upgrade.cost(level),
                progress: None,
                count: 0,
            },
            _ => continue,
        };
        menu[slot] = Some(entry);
    }
}

/// The workshop (StudioClassTy, menu 0x4361c0) builds weapons, items 3-8
/// (commands 0x30-0x35), in slots 0-2 and 4-6.
fn workshop_menu(scene: &Scene, obj: &MapObject, menu: &mut Menu) {
    let making = scene.training_at(obj.id);
    // Only what the player may make (0x4361c0 asks 0x43da20 first).
    for item in (3u8..9).filter(|&k| scene.can_make(k)) {
        let slot = (item % 3) as usize + (item as usize / 3) * 4 - 4;
        let (amber, fish) = crate::combat::item_cost(item);
        menu[slot] = Some(Slot {
            checked: false,
            closes: false,
            command: Command(0x2d + item),
            // While making, its buttons stay on (0x4361c0).
            enabled: scene.amber >= amber && scene.fish >= fish || making.is_some(),
            progress: making.and_then(|t| t.progress(item, true)),
            count: making.map_or(0, |t| t.count(item)),
        });
    }
    // Slot 8 opens the research page (command 0x3d, menu page 3).
    set(menu, 8, RESEARCH, true);
    // Slot 10 has the settlement mend its stock's worn units, with permit
    // bit 11 (0x4361c0): enabled while it is idle, something is worn and
    // the amber suffices, or while it mends (shown as progress).
    let Some(home) = scene
        .object_index(obj.id)
        .and_then(|i| scene.home_settlement(i))
    else {
        return;
    };
    if scene.permits.allows(11) {
        let units = scene.stock_units(home);
        let (amber, _) = crate::scene::repair::stock_repair_cost(&units);
        let repair = scene.stock_repair_at(home);
        let idle = repair.is_none() && scene.training_at(home).is_none();
        menu[10] = Some(Slot {
            checked: false,
            closes: false,
            command: Command::REPAIR_STOCK,
            enabled: repair.is_some()
                || idle && units.iter().flatten().any(|&c| c < 100) && scene.amber >= amber,
            progress: repair.map(|r| r.done() as f32 / 100.0),
            count: 0,
        });
    }
}

/// The behaviour switches on a warship's behaviour page.
pub const GUARD: Command = Command(0x6f);
pub const FIRE_ON_AGGRESSOR: Command = Command(0x72);
pub const BOARD_ATTACK: Command = Command(0x78);
pub const RAM_ATTACK: Command = Command(0x79);
pub const SCOUT: Command = Command(0x76);
pub const PATROL: Command = Command(0x6e);
pub const GUARD_ZONE: Command = Command(0x74);
pub const CONVOY: Command = Command(0x6d);
/// A merchant's gathering zones: amber, fish, leviathans.
pub const GATHER_ZONES: [Command; 3] = [Command(0x7a), Command(0x7b), Command(0x7c)];
pub const ATTACK_ZONE: Command = Command(0x75);

/// A warship's behaviour page (0x4e6980, page 5): guard, ram attack and
/// board attack in slots 0-2, fire on aggressor in slot 5 (only while
/// guarding), convoy in 4, scout in 6, patrol in 8, guard and attack zones
/// in 9 and 10.
fn behaviour_menu(scene: &Scene, obj: &MapObject) -> Menu {
    let mut menu: Menu = [None; SLOTS];
    let Some(ship) = obj.ship.as_ref().filter(|_| scene.is_own(obj)) else {
        return menu;
    };
    let b = ship.behaviour;
    let mut toggle = |slot: usize, command: Command, enabled: bool, checked: bool| {
        menu[slot] = Some(Slot {
            command,
            enabled,
            progress: None,
            count: 0,
            checked,
            closes: false,
        });
    };
    toggle(0, GUARD, true, b.guard);
    toggle(1, RAM_ATTACK, true, b.ram);
    toggle(2, BOARD_ATTACK, true, b.board);
    toggle(5, FIRE_ON_AGGRESSOR, b.guard, b.guard && b.aggressor);
    let scouting = matches!(ship.task, Some(crate::objects::Task::Scout { .. }));
    let patrolling = matches!(ship.task, Some(crate::objects::Task::Patrol { .. }));
    toggle(6, SCOUT, true, scouting);
    toggle(8, PATROL, true, patrolling);
    let zone = |attack| matches!(ship.task, Some(crate::objects::Task::Zone { attack: a, .. }) if a == attack);
    toggle(9, GUARD_ZONE, obj.is_armed(), zone(false));
    toggle(10, ATTACK_ZONE, obj.is_armed(), zone(true));
    let escorting = matches!(ship.task, Some(crate::objects::Task::Convoy { .. }));
    toggle(4, CONVOY, true, escorting);
    menu
}

/// A merchant's gathering zone page: amber, fish and leviathan zones in
/// slots 0-2, the one in use shown set.
fn zones_menu(scene: &Scene, obj: &MapObject) -> Menu {
    use crate::objects::Resource;
    let mut menu: Menu = [None; SLOTS];
    let Some(ship) = obj.ship.as_ref().filter(|_| scene.is_own(obj)) else {
        return menu;
    };
    for (slot, (command, resource)) in GATHER_ZONES
        .into_iter()
        .zip([Resource::Amber, Resource::Fish, Resource::Leviathan])
        .enumerate()
    {
        menu[slot] = Some(Slot {
            command,
            enabled: true,
            progress: None,
            count: 0,
            checked: ship.zone.is_some_and(|z| z.1 == resource),
            closes: false,
        });
    }
    menu
}

/// Opens a workshop's research page.
pub const RESEARCH: Command = Command(0x3d);

/// The workshop's research page (0x4361c0, page 3): one row per item group
/// (marines, medium, strong weapons), with rate of fire, damage and range
/// in slots 0-2 of the row.
fn research_menu(scene: &Scene, obj: &MapObject) -> Menu {
    use crate::combat::Upgrade;
    let mut menu: Menu = [None; SLOTS];
    if !scene.is_own(obj) || obj.class != ObjectClass::Building(WORKSHOP) {
        return menu;
    }
    let busy = scene.research_at(obj.id);
    for (g, column) in (0..3u8).flat_map(|g| (0..3).map(move |c| (g, c))) {
        let upgrade = [Upgrade::Rate(g), Upgrade::Arms(g), Upgrade::Reach(g)][column];
        let slot = g as usize * 4 + column;
        menu[slot] = match (busy, scene.next_level(upgrade)) {
            (Some((u, progress)), _) if u == upgrade => Some(Slot {
                checked: false,
                closes: false,
                command: Command(upgrade.command()),
                enabled: true,
                progress: Some(progress),
                count: 0,
            }),
            (_, Some(level)) => Some(Slot {
                checked: false,
                closes: false,
                command: Command(upgrade.command()),
                enabled: busy.is_none() && scene.amber >= upgrade.cost(level),
                progress: None,
                count: 0,
            }),
            _ => None,
        };
    }
    menu
}

/// Settlement and town (0x43c740): build structures, repair, and the
/// upgrade of a settlement to a town.
fn settlement_menu(scene: &Scene, obj: &MapObject, menu: &mut Menu) {
    set(menu, 0, Command::BUILD_STRUCTURES, true);
    // Reload Object with one of its forts (0x43c740 slot 1, button 5:
    // there when the player may have marines, player +0x46 bit 7; enabled
    // while the settlement stands idle and has a fort, +0x214-+0x21c).
    if scene.permits.item(0) {
        let idle = scene.training_at(obj.id).is_none()
            && scene.stock_repair_at(obj.id).is_none()
            && !scene.mending.contains(&obj.id)
            && scene.construction_at(obj.id).is_none();
        set(
            menu,
            1,
            Command::LOAD,
            idle && !scene.forts_of(obj.id).is_empty(),
        );
    }
    // Marines: one button per crew type in slots 8-10, showing the training
    // in progress.
    let training = scene.training_at(obj.id);
    // Only those the player may train (0x43c740 asks 0x43da20: the
    // settlement's workshops and smithies, +0x1fc and +0x200).
    let trainable = |k: u8| scene.can_make(k);
    for kind in (0..3u8).filter(|&k| trainable(k)) {
        let command = Command(Command::TRAIN_BOWMEN.0 + kind);
        let (amber, fish) = crate::combat::CREW_COST[kind as usize];
        menu[8 + kind as usize] = Some(Slot {
            checked: false,
            closes: false,
            command,
            // A kind in training keeps its button on (0x43c740).
            enabled: scene.amber >= amber && scene.fish >= fish
                || training.is_some_and(|t| t.count(kind) > 0),
            progress: training.and_then(|t| t.progress(kind, false)),
            count: training.map_or(0, |t| t.count(kind)),
        });
    }
    set(menu, 2, Command::REPAIR_BUILDING, true);
    // With the permit, selling the item chosen in its stock, or the
    // settlement itself, while it is not training (0x43c740 slots 5 and 6,
    // 0x441ba0).
    if scene.permits.selling() {
        let idle = training.is_none();
        set(menu, 5, Command::SELL_ITEM, idle);
        set(menu, 6, Command::SELL_TOWN, idle);
    }
    if obj.class == ObjectClass::Building(SETTLEMENT) && scene.permits.town() {
        set(
            menu,
            4,
            Command::UPGRADE_TOWN,
            scene.amber >= economy::building_cost(TOWN),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_name_prices() {
        assert_eq!(
            economy::ship_command(0).describe(None),
            "Build Boat: 200 amber"
        );
        assert_eq!(
            economy::ship_command(2).describe(None),
            "Build Royal Ship: 1100 amber, 400 fish"
        );
        assert_eq!(
            Command(0x2f).describe(None),
            "Train swordsmen: 30 amber, 300 fish"
        );
        assert_eq!(Command(0x33).describe(None), "Build a ballista: 70 amber");
        assert_eq!(
            Command::UPGRADE_TOWN.describe(None),
            "Upgrade to town: 500 amber"
        );
        assert_eq!(Command::STOP.describe(None), "Stop");
    }
}
