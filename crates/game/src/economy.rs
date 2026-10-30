//! Costs and requirements, from ANCIENT.EXE.

use crate::commands::Command;

/// Hull cost of each ship type in amber (0x5961d8). Weapons and crews add to
/// it in the original (0x43e5c0); ships built here come unarmed.
pub const HULL_AMBER: [u32; 12] = [200, 400, 1100, 1300, 900, 800, 700, 600, 700, 900, 700, 990];
/// Hull cost in fish, in hundreds (0x596208).
const HULL_FISH_HUNDREDS: [u32; 12] = [0, 2, 4, 4, 2, 3, 2, 2, 2, 2, 2, 10];
/// Buildings each ship type requires besides a shipyard and a settlement, as
/// a bit mask over building classes: bit n is class 0x32 + n (0x5961a8).
const SHIP_REQUIRES: [u32; 12] = [0, 2, 96, 32, 96, 2, 32, 32, 34, 34, 34, 98];

/// Seconds a brain's builder other than a yard (a barbarian structure)
/// takes over a ship here. Not decoded: how a barbarian structure builds.
/// A yard builds on its berth (`scene::yard`: 39 or 40 steps of 8 ticks,
/// 0x4b3ab0).
pub const SHIP_BUILD_SECONDS: f32 = 25.0 * crate::clock::TICK;

/// Seconds to put up a building or upgrade one: its construction picture
/// plays once, the first entry of the class's cycle table (+0xc0; 200
/// ticks for every player building), and progress is elapsed x 100 / that
/// cycle (0x416f90 for the dock).
pub const BUILDING_SECONDS: f32 = 200.0 * crate::clock::TICK;

pub const SETTLEMENT: u32 = 0x32;
pub const TOWN: u32 = 0x33;
pub const WORKSHOP: u32 = 0x37;
pub const SMITHY: u32 = 0x38;
pub const SCHOOL: u32 = 0x3c;
pub const PORT: u32 = 0x39;
pub const TEMPLE_OF_THE_SEA: u32 = 0x3d;
pub const TEMPLE_OF_WAR: u32 = 0x3e;
pub const SHIPYARD: u32 = 0x3a;
pub const DOCK: u32 = 0x3b;

pub fn ship_cost(kind: u8) -> (u32, u32) {
    let k = kind as usize;
    (HULL_AMBER[k], HULL_FISH_HUNDREDS[k] * 100)
}

/// Building classes (bit mask, bit n = class 0x32 + n) a ship type needs.
pub fn ship_requirements(kind: u8) -> u32 {
    SHIP_REQUIRES[kind as usize] | 1 << (SHIPYARD - SETTLEMENT)
}

/// Ship types a nation can build: the eight common ones and its own two.
pub fn buildable_ships(nation: u32) -> impl Iterator<Item = u8> {
    let own: &[u8] = match nation {
        1 => &[8, 9],
        3 => &[10, 11],
        _ => &[],
    };
    (0..8).chain(own.iter().copied())
}

/// The build command of a ship type (0x42d750).
pub fn ship_command(kind: u8) -> Command {
    const IDS: [u8; 12] = [
        0x21, 0x22, 0x28, 0x27, 0x25, 0x26, 0x24, 0x23, 0x29, 0x2a, 0x2b, 0x2c,
    ];
    Command(IDS[kind as usize])
}

pub fn ship_of_command(c: Command) -> Option<u8> {
    (0..12).find(|&k| ship_command(k) == c)
}

/// Amber cost of a building, or of the upgrade to it (0x42d8b0).
pub fn building_cost(class: u32) -> u32 {
    match class {
        0x32 | 0x3a => 400,
        0x33 => 500,
        0x34 | 0x3b => 600,
        0x35 | 0x3d | 0x3e => 1000,
        0x36 => 1500,
        0x37 => 700,
        0x38 => 800,
        0x39 => 200,
        0x3c => 900,
        _ => 0,
    }
}

/// The building class a build command puts up (command ids 0x16-0x20,
/// button suffixes TOWN, FORTW .. WAR; 0x16 is a ship's settlement).
pub fn building_of_command(c: Command) -> Option<u32> {
    Some(match c.0 {
        0x16 => SETTLEMENT,
        0x17 => 0x34,
        0x18 => 0x35,
        0x19 => 0x36,
        0x1a => PORT,
        0x1b => SHIPYARD,
        0x1c => 0x37,
        0x1d => 0x38,
        0x1e => 0x3c,
        0x1f => 0x3d,
        0x20 => 0x3e,
        _ => return None,
    })
}
