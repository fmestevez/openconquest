//! The ships' fire pass (Run_Ship.cpp): the weapons' reach (0x4bf5f0) and
//! which of them are ready (0x4bf9b0), from the ship's tick (0x4ab930 at
//! 0x4abb39-0x4abba5). A game tick at a time.

use super::Scene;
use crate::objects;

/// `ancient.exe`'s bytes from 0x5980f4 to 0x598ade, dumped from the exe
/// (`ready_tables_are_the_exes`): the box of each position's look
/// (0x5980f4, `level x 4 + position`), the aiming distances (0x598130,
/// `level x 12 + item`) and the reach shapes 0x4bf5f0 reads (0x59819c
/// items 0 and 2, 0x598290 item 1, 0x598324 item 3, 0x598418 item 4,
/// 0x59850c item 5, 9 x 9 or 7 x 7 a level; 0x598600 item 6, 13 x 13;
/// 0x5987fc item 7, 0x598968 item 8, 11 x 11). Kept as one block as the
/// original reads it: 0x4bf5f0 bounds its row and column only below, so
/// a target beyond a shape's side (0x4c0cd0's signed distances) reads on
/// into the next row, level or table.
const MEMORY: [u8; 2539] = [
    9, 9, 13, 9, 9, 9, 13, 9, 9, 9, 13, 9, 60, 0, 0, 0, 0, 0, 0, 0, 60, 0, 0, 0, 60, 0, 0, 0, 60,
    0, 0, 0, 40, 0, 0, 0, 40, 0, 0, 0, 44, 0, 0, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 9, 7, 7, 9, 9, 9, 13, 11, 11, 7, 9, 0, 9, 7, 7, 9, 9, 9, 13, 11, 11, 7, 9, 0, 9, 7, 7, 9, 9,
    9, 13, 11, 11, 7, 9, 0, 3, 2, 2, 3, 3, 3, 5, 3, 4, 2, 3, 0, 3, 2, 2, 4, 3, 3, 5, 4, 4, 2, 3, 0,
    4, 3, 3, 4, 4, 4, 6, 5, 5, 3, 4, 0, 0, 0, 0, 0, 0, 1, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 1,
    1, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0,
    0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 0, 0, 1, 1,
    1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1,
    1, 1, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0,
    0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1,
    1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1,
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 2, 1, 1, 0,
    0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1,
    1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 1, 2, 1, 1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1,
    2, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1,
    1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1,
    1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0,
    1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1,
    1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1,
    0, 0, 1, 1, 1, 2, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1,
    1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0,
    0, 1, 1, 0, 0, 1, 1, 0, 2, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0,
    0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1,
    0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 0, 1, 1, 0, 0, 1,
    1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 2, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1,
    1, 0, 0, 1, 1, 1, 0, 0, 2, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1,
    1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1,
    0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1,
    1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1,
    1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 2, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 0,
    0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0,
    0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0,
    0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 0,
    0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 2, 0, 0, 1, 1, 1,
    1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0,
    0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1,
    0, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 2, 0, 1, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0,
    1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0,
    0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0,
    0, 1, 1, 1, 0, 2, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 2, 0,
    1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 2, 0, 1, 1, 1, 0, 0, 1,
    1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0,
    0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 2, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1,
    1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1,
    1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0,
    0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 2, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 1,
    1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 26, 0, 0,
];

/// The address `MEMORY` starts at.
const BASE: u32 = 0x5980f4;
/// The aiming distances (0x598130).
const AIMS: u32 = 0x598130;

/// The byte of `ancient.exe` at `va`, within `MEMORY`.
fn byte(va: u32) -> u8 {
    let at = (va - BASE) as usize;
    debug_assert!(at < MEMORY.len(), "0x{va:x} outside the dump");
    MEMORY.get(at).copied().unwrap_or(0)
}

/// Item `item`'s aiming distance at range level `level` (0x598130[level
/// x 12 + item]): 9, 7, 7, 9, 9, 9, 13, 11, 11, 7, 9, 0 at every level.
pub(super) fn aim_cells(level: u8, item: u8) -> u32 {
    byte(AIMS + level as u32 * 12 + item as u32) as u32
}

/// 0x4bf5f0's shape of item `item` at range level `level`, `dx` / `dy`
/// cells off (the ship's cell less the target's): with `s` half the
/// item's aiming distance, `a = s - dx` and `b = s - dy` both at least 0,
/// the shape's byte `b + (a + level x n) x n` (n its side) is not 0.
/// Items 9 and 10 (Greek fire, quick lime) take item 1's at level 2;
/// item 11 none.
pub(super) fn shape_reaches(item: u8, level: u8, dx: i32, dy: i32) -> bool {
    let (item, level) = match item {
        9 | 10 => (1, 2),
        _ => (item, level),
    };
    let (base, n) = match item {
        0 | 2 => (0x59819c, 9),
        1 => (0x598290, 7),
        3 => (0x598324, 9),
        4 => (0x598418, 9),
        5 => (0x59850c, 9),
        6 => (0x598600, 13),
        7 => (0x5987fc, 11),
        8 => (0x598968, 11),
        _ => return false,
    };
    let s = (aim_cells(level, item) >> 1) as i32;
    let (a, b) = (s - dx, s - dy);
    if a < 0 || b < 0 {
        return false;
    }
    byte((base + b + (a + level as i32 * n) * n) as u32) != 0
}

/// What 0x4bf9b0 returns: the positions with an item ready (bit k), and
/// the ram and board marks (`+0x710`, `+0x711`; bits 0x10, 0x20).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Ready {
    pub mask: u8,
    pub ram: bool,
    pub board: bool,
}

/// Ticks that must pass after a switch's time (`+0x704`, `+0x708`)
/// before its next look (0x4bf9b0: more than 0x32).
const LOOK_WAIT: i32 = 0x32;
/// What 0x4bf9b0 sets the time to as it marks a switch: now + 100.
const LOOK_AGAIN: u32 = 100;

impl Scene {
    /// Ship `i`'s units of each item (`+0x289 + i`): marines 0-2 by
    /// squad, weapons 3-8, the unique ships' 9-11.
    pub(super) fn item_counts(&self, i: usize) -> [u8; 12] {
        let o = &self.objects[i];
        let mut n = [0u8; 12];
        if let Some(s) = &o.ship {
            n[..3].copy_from_slice(&s.crew.squads.map(|q| q.min(255) as u8));
        }
        if let Some(c) = &o.combat {
            for &w in &c.weapons {
                if (3..12).contains(&w) {
                    n[w as usize] = n[w as usize].saturating_add(1);
                }
            }
        }
        n
    }

    /// The range level 0x4bf5f0 and the looks take for ship `i`'s
    /// position `k` (player `+0x95 + k`; 2 with Theseus aboard, 0x4ba790
    /// hero 1, or for the fourth position).
    pub(super) fn reach_level(&self, i: usize, k: usize) -> u8 {
        let o = &self.objects[i];
        let theseus = o
            .ship
            .as_ref()
            .is_some_and(|s| s.has_hero(objects::THESEUS));
        if theseus || k >= 3 {
            objects::HERO_LEVEL
        } else {
            self.levels_of(o.owner).reach[k]
        }
    }

    /// 0x4bf5f0: whether ship `i`'s item `item` reaches a target `dx` /
    /// `dy` cells off (its cell less the target's): items 9 and 10 by item
    /// 1's shape at level 2, the others by their own at the range level of
    /// their position (`reach_level`).
    pub(super) fn reaches(&self, i: usize, item: u8, dx: i32, dy: i32) -> bool {
        if item >= 11 {
            return false;
        }
        let level = if item >= 9 {
            objects::HERO_LEVEL
        } else {
            self.reach_level(i, item as usize / 3)
        };
        shape_reaches(item, level, dx, dy)
    }

    /// 0x4bf9b0: which of ship `i`'s weapons are ready. The positions'
    /// marks (`+0x62b`, 3 or 4) are cleared; then over the items (9, 11
    /// for types 8 and 9) with a unit aboard, in fire states 1, 3 and 4 an
    /// item is ready (`+0x613`, its position's mark and bit) once its time
    /// (`+0x5e3 + 4i`) has come, if its position has a target (`+0x63c`)
    /// or guard is on; in state 2 by its time alone; in 0 none is looked
    /// at. With ram or board on both marks (`+0x710`, `+0x711`) are
    /// cleared, and each switch on whose time (`+0x704`, `+0x708`) is more
    /// than 50 ticks past is marked and its time set to now + 100.
    pub(super) fn ready_mask(&mut self, i: usize) -> Ready {
        let now = self.ticks;
        let counts = self.item_counts(i);
        let s = self.objects[i].ship.as_mut().unwrap();
        let positions = s.positions();
        let items = if positions == 4 { 11 } else { 9 };
        let mut ready = Ready::default();
        s.positions_ready = [false; 4];
        let state = s.behaviour.fire_state;
        let guard = s.behaviour.guard;
        if matches!(state, 1..=4) {
            for (item, &n) in counts.iter().enumerate().take(items) {
                if n == 0 {
                    continue;
                }
                let k = item / 3;
                let due = now.wrapping_sub(s.fire_times[item]) as i32 >= 0;
                let aimed = state == 2 || s.behaviour.aims[k].target != 0 || guard;
                s.items_ready[item] = due && aimed;
                if due && aimed {
                    s.positions_ready[k] = true;
                    ready.mask |= 1 << k;
                }
            }
        }
        let b = s.behaviour;
        if b.ram || b.board {
            if b.ram && now.wrapping_sub(s.ram_wait) as i32 > LOOK_WAIT {
                ready.ram = true;
                ready.mask |= 0x10;
                s.ram_wait = now + LOOK_AGAIN;
            }
            if b.board && now.wrapping_sub(s.board_wait) as i32 > LOOK_WAIT {
                ready.board = true;
                ready.mask |= 0x20;
                s.board_wait = now + LOOK_AGAIN;
            }
        }
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes of `ancient.exe` at `va` (file offset + 0x400000).
    fn exe_bytes(va: u32, n: usize) -> Option<Vec<u8>> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        let path = crate::assets::find_ci(std::path::Path::new(&root), "ancient.exe").ok()??;
        let bytes = std::fs::read(path).ok()?;
        let at = (va - 0x40_0000) as usize;
        Some(bytes[at..at + n].to_vec())
    }

    #[test]
    fn ready_tables_are_the_exes() {
        let Some(b) = exe_bytes(BASE, MEMORY.len()) else {
            return;
        };
        assert_eq!(b, MEMORY);
    }

    #[test]
    fn shapes_as_0x4bf5f0() {
        // The aiming distances.
        assert_eq!(
            (0..12).map(|i| aim_cells(1, i)).collect::<Vec<_>>(),
            [9, 7, 7, 9, 9, 9, 13, 11, 11, 7, 9, 0]
        );
        // Every shape holds its own cell; none beyond its half aim below.
        for item in 0..11 {
            for level in 0..3 {
                assert!(shape_reaches(item, level, 0, 0), "{item} {level}");
                let (it, lv) = if item >= 9 { (1, 2) } else { (item, level) };
                let s = (aim_cells(lv, it) >> 1) as i32;
                assert!(!shape_reaches(item, level, s + 1, 0));
                assert!(!shape_reaches(item, level, 0, s + 1));
            }
        }
        assert!(!shape_reaches(11, 2, 0, 0));
    }
}
