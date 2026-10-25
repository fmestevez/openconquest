//! Object commands and the command panel menus.
//!
//! The object panel shows a grid of 12 command buttons (4 x 3). The selected
//! object fills the menu when asked with message 0x10a0; each slot is a
//! command id and a state (enabled or not). Command ids and their button
//! images come from 0x4f08a0; the ship menus from 0x4e6e90 (merchants) and
//! 0x4e6980 (warships), with per-type removals in 0x4e42e0; the shipyard
//! menu from 0x4164e0 and the settlement menu from 0x43c740.

use openac_formats::ObjectClass;

use crate::economy::{self, DOCK, SETTLEMENT, SHIPYARD, TEMPLE_OF_THE_SEA, TEMPLE_OF_WAR, TOWN};
use crate::magic::{self, Spell, Temple};
use crate::objects::{MapObject, SHIP_NAMES};
use crate::scene::Scene;

pub const SLOTS: usize = 12;

/// A command id as used by the original game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub const TRAIN_BOWMEN: Command = Command(0x2d);
    pub const TRAIN_SWORDSMEN: Command = Command(0x2f);
    pub const UPGRADE_TOWN: Command = Command(0x3a);
    pub const UPGRADE_DOCK: Command = Command(0x3b);
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
            0x6c => "CBUT_BEHMAIN",
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
                    0x3a => "UPGTOWN",
                    0x3b => "UPGDOCK",
                    _ => return None,
                };
                let digit = if nation == 3 { 3 } else { 1 };
                return Some(format!("CBUT_BLD{digit}{suffix}"));
            }
        };
        Some(fixed.to_string())
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
            0x2d..=0x2f => {
                return format!(
                    "Train {}",
                    crate::combat::CREW_NAMES[(self.0 - 0x2d) as usize]
                )
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
}

/// The command menu of an object on `page`.
pub fn menu(scene: &Scene, obj: &MapObject, page: Page) -> Menu {
    match page {
        Page::Main => main_menu(scene, obj),
        Page::Structures => structures_menu(scene, obj),
        Page::Wind => wind_menu(scene, obj),
    }
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
    if !scene.magic.all_known(temple) {
        let learning = scene.magic.learning(temple);
        menu[0] = Some(Slot {
            command: magic::GET_MAGIC,
            enabled: learning.is_some() || scene.amber >= magic::LEARN_COST,
            progress: learning.map(|t| t / magic::LEARN_SECONDS),
        });
        slot = 1;
    }
    for &spell in scene.magic.known(temple) {
        let (amber, fish) = spell.cost();
        let enabled = spell.implemented() && scene.amber >= amber && scene.fish >= fish;
        set(menu, slot, spell.command(), enabled);
        slot += 1;
    }
}

/// Buildings a settlement or town can put up (own layout: the original's
/// submenu has not been decoded). Esc returns to the main menu.
fn structures_menu(scene: &Scene, obj: &MapObject) -> Menu {
    const LAYOUT: [Option<u8>; SLOTS] = [
        Some(0x1a),
        Some(0x1b),
        Some(0x1c),
        Some(0x1d),
        Some(0x1e),
        Some(0x1f),
        Some(0x20),
        None,
        Some(0x17),
        Some(0x18),
        Some(0x19),
        None,
    ];
    let mut menu: Menu = [None; SLOTS];
    if !scene.is_own(obj) || !matches!(obj.class, ObjectClass::Building(SETTLEMENT | TOWN)) {
        return menu;
    }
    for (slot, id) in LAYOUT.iter().enumerate() {
        if let Some(id) = id {
            let command = Command(*id);
            let class = economy::building_of_command(command).unwrap();
            set(
                &mut menu,
                slot,
                command,
                scene.amber >= economy::building_cost(class),
            );
        }
    }
    menu
}

/// The main command menu of an object, laid out as the original does.
/// Only the human player's objects have commands. Commands that are not
/// implemented yet are shown disabled.
pub fn main_menu(scene: &Scene, obj: &MapObject) -> Menu {
    let mut menu: Menu = [None; SLOTS];
    if !scene.is_own(obj) || scene.construction_at(obj.id).is_some() {
        return menu;
    }
    match obj.class {
        ObjectClass::Ship => ship_menu(obj, &mut menu),
        ObjectClass::Building(SHIPYARD | DOCK) => shipyard_menu(scene, obj, &mut menu),
        ObjectClass::Building(SETTLEMENT | TOWN) => settlement_menu(scene, obj, &mut menu),
        ObjectClass::Building(c @ (TEMPLE_OF_THE_SEA | TEMPLE_OF_WAR)) => {
            temple_menu(scene, Temple::of_class(c).unwrap(), &mut menu)
        }
        _ => {}
    }
    menu
}

fn set(menu: &mut Menu, slot: usize, command: Command, enabled: bool) {
    menu[slot] = Some(Slot {
        command,
        enabled,
        progress: None,
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
        set(menu, 2, Command::UNLOAD, false);
        set(menu, 4, Command::AMBER, true);
        set(menu, 5, Command::FISH, true);
        set(menu, 6, Command::WHALE, false);
        set(menu, 7, Command::ZONES, false);
    } else {
        // Warships (0x4e6980): weapons and behaviour.
        set(menu, 4, Command::FIRE, obj.is_armed());
        set(menu, 5, Command::BOARD, ship.crew.count > 0);
        set(menu, 6, Command::RAM, true);
        set(menu, 7, Command::BEHAVIOUR, false);
    }
    set(menu, 8, Command::LOAD, true);
    set(menu, 9, Command::REPAIR_SHIP, false);
    set(menu, 10, Command::BUILD_GROUND, kind <= 1);
    set(menu, 11, Command::TREASURE, false);
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
    for (slot, kind) in economy::buildable_ships(human.nation).enumerate() {
        let command = economy::ship_command(kind);
        let mut entry = Slot {
            command,
            enabled: building.is_none() && scene.can_build_ship(kind).is_ok(),
            progress: None,
        };
        if let Some(p) = building.filter(|p| p.ship == kind) {
            entry.progress = Some(p.elapsed / p.total);
            entry.enabled = true;
        }
        menu[slot] = Some(entry);
    }
    if obj.class == ObjectClass::Building(SHIPYARD) {
        let enabled = building.is_none() && scene.amber >= economy::building_cost(DOCK);
        set(menu, 11, Command::UPGRADE_DOCK, enabled);
    }
}

/// Settlement and town (0x43c740): build structures, repair, and the
/// upgrade of a settlement to a town.
fn settlement_menu(scene: &Scene, obj: &MapObject, menu: &mut Menu) {
    set(menu, 0, Command::BUILD_STRUCTURES, true);
    // Marines: one button per crew type in slots 8-10, showing the training
    // in progress.
    let training = scene.training_at(obj.id);
    for kind in 0..3u8 {
        let command = Command(Command::TRAIN_BOWMEN.0 + kind);
        let (amber, fish) = crate::combat::CREW_COST[kind as usize];
        menu[8 + kind as usize] = Some(Slot {
            command,
            enabled: scene.amber >= amber && scene.fish >= fish,
            progress: training.filter(|t| t.kind == kind).map(|t| t.progress()),
        });
    }
    set(menu, 2, Command::REPAIR_BUILDING, false);
    if obj.class == ObjectClass::Building(SETTLEMENT) {
        set(
            menu,
            4,
            Command::UPGRADE_TOWN,
            scene.amber >= economy::building_cost(TOWN),
        );
    }
}
