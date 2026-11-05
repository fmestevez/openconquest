//! The ships' fire pass (Run_Ship.cpp), from the ship's tick (0x4ab930 at
//! 0x4abb39-0x4abba5): the weapons' reach (0x4bf5f0) and which of them
//! are ready (0x4bf9b0), then by the fire state `+0x5d1` (0x4c1240) the
//! search (0x4bfc60), the look about (0x4c0eb0) or the attack (0x4c09c0),
//! each firing an item a shot (0x4c0230). A game tick at a time.

use std::collections::HashMap;

use openconquest_formats::map::TILE_SIZE;

use super::leviathan::SHOWING;
use super::{walk_path, Caches, GameData, MapObject, ObjectClass, Scene};
use crate::objects::{self, Task};

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
/// The position boxes (0x5980f4), the aiming distances (0x598130).
const BOXES: u32 = 0x5980f4;
const AIMS: u32 = 0x598130;

/// The byte of `ancient.exe` at `va`, within `MEMORY`.
fn byte(va: u32) -> u8 {
    let at = (va - BASE) as usize;
    debug_assert!(at < MEMORY.len(), "0x{va:x} outside the dump");
    MEMORY.get(at).copied().unwrap_or(0)
}

/// The box a position's look covers (0x5980f4[level x 4 + position]):
/// 9, or 13 for position 2, at every level.
pub(super) fn look_box(level: u8, k: usize) -> u32 {
    byte(BOXES + level as u32 * 4 + k as u32) as u32
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

    /// Ship `i`'s fire power with item `item` (`+0x449 + 4i`, 0x4bc360),
    /// in the original's units: the cache 0x5c75cc[3i + L] (L the
    /// player's damage level of the item's position, `+0x9c + i / 3`) x
    /// its units x their mean condition / 100. Greek fire (item 9, `+0x46d`)
    /// only on a type 8 and quick lime (item 10, `+0x471`) only on a type
    /// 9: the cache's rows 12 and 13 (0x5c765c / 0x5c7668, L at `+0x9e`) x
    /// the hull `+0x271` / the type's full hull (0x5c76d0 / 0x5c76d4, which
    /// are 0x5c76b0[8] and [9], filled by 0x4ac210).
    pub(super) fn item_power(&self, i: usize, item: u8) -> u32 {
        let o = &self.objects[i];
        let level = self.levels_of(o.owner).arms_for(item);
        let cache = crate::object_info::cache(item, level);
        match item {
            9 | 10 => {
                let kind = item - 1;
                if o.ship.as_ref().map(|s| s.kind) != Some(kind) {
                    return 0;
                }
                // 0x597d2c's rows 12 (A) and 13 (no A: B), x 1500 / 100
                // (0x4ac140).
                let cache = crate::object_info::cache(item + 3, level) as u64;
                let hull = o
                    .combat
                    .as_ref()
                    .map_or(0, |c| (c.hull.max(0.0) * 15.0) as u64);
                (cache * hull / crate::combat::hull_points(kind) as u64) as u32
            }
            _ => {
                let units = o.unit_conditions();
                let Some(u) = units.get(item as usize).filter(|u| !u.is_empty()) else {
                    return 0;
                };
                let n = u.len() as u32;
                let mean = u.iter().map(|&c| c as u32).sum::<u32>() / n;
                cache * n * mean / 100
            }
        }
    }

    /// 0x4bf7f0, ship `i`'s roll with item `item` at object `t` (of
    /// family `family`, 0x50e600) `d` cells off: Greek fire 100 with no
    /// draw; else a draw of the ship's generator, and `43 + ease - far`
    /// with `far = d x 33 / (aim << 6)` (aim 0x598130 at the player's range
    /// level of the item's position, `+0x95 + min(i / 3, 2)`, no hero): a
    /// building (0x32) has ease 25, or with the phantom (vt+0xa4, 0x40dfd0
    /// `+0x134`) 0 and far 33; a ship its type's (0x597acc), 1 with Zetes
    /// aboard (hero 3), the phantom (`+0x253`) 0 and far 23; anything else
    /// 25. The draw `% 100` below it is a hit (100), else the chance
    /// itself. Either way the crew gains experience (0x4b4680): 50 a hit
    /// and 10 a miss, 170 and 20 at a ship (family 200).
    fn hit_roll(&mut self, i: usize, item: u8, d: i32, t: usize, family: u32) -> u8 {
        if item == 9 {
            return 100;
        }
        let draw = self.ship_draw(i);
        let level = self.levels_of(self.objects[i].owner).reach[(item as usize / 3).min(2)];
        let mut far = d * 33 / ((aim_cells(level, item) as i32) << 6);
        let o = &self.objects[t];
        let phantom = o.combat.as_ref().is_some_and(|c| c.phantom > 0.0);
        let mut ease = 25;
        if family == 0x32 {
            if phantom {
                far = 33;
                ease = 0;
            }
        } else if let (200, Some(s)) = (family, o.ship.as_ref()) {
            ease = crate::combat::SHIP_EASE[s.kind as usize % 12];
            if s.has_hero(objects::ZETES) {
                ease = 1;
            }
            if phantom {
                far = 0x17;
                ease = 0;
            }
        }
        let chance = ease - far + 0x2b;
        // The crew's experience (0x4b4680): a hit 50, 170 at a ship; a miss
        // 10, 20 at a ship.
        let hit = ((draw % 100) as i32) < chance;
        let gain = match (hit, family == 200) {
            (true, false) => 50,
            (true, true) => 170,
            (false, false) => 10,
            (false, true) => 20,
        };
        self.gain_experience(i, gain);
        if hit {
            100
        } else {
            chance as u8
        }
    }

    /// 0x4c0230: ship `i` (of type 2 and up) fires the positions in
    /// `positions` (bit k), each ready item of the position with a unit
    /// aboard once: at the position's target (`+0x63c`, fetched for its
    /// first such item, the others taking the same) or, `from_lists`, at
    /// entry 0 of the item's candidate list (0x5c7ed0, none: the item is
    /// skipped). The target's cell (vt+0x28) and family (0x50e600) give
    /// the reach (0x4bf5f0 at the distances across and down; a settlement
    /// or town, 0x32 / 0x33, from any of its 2 x 2 cells) and the distance
    /// of the roll (0x557440, not taken anew for a 2 x 2 building of
    /// family 0x32, `+0x56 > 1`: the last one stands); these are kept from
    /// one fetch to the next as the original's locals. A hit's blow is the
    /// power by the accuracy (0x4bfc20), x 150 / 100 with the player's
    /// Flame (`+0xa8`) and again with Theseus; a miss's (0) the power x 10
    /// / 100 / 5. One unit of the item and not a 100 at a target not of
    /// family 0x32: the accuracy is 0 and the shot flies wide. Greek fire
    /// flies flaming, x 150 / 100 more; quick lime never flaming. The
    /// item's next time is 0x5c76e0[3i + L] + a draw % 30 + now (L the
    /// player's rate level `+0x8e + k`, 2 with Atalanta aboard, hero 6, or
    /// for the fourth position). Returns the positions fired. (Not ported:
    /// a sea monster's target, 0x1fb, is not shot when the shooter's vt+8
    /// takes the aim point, `(x, y, 1, 0, 1)`, not decoded; a position's
    /// target gone, the original's pointer dangles: the port skips it.)
    #[allow(clippy::needless_range_loop)]
    pub(super) fn fire_items(
        &mut self,
        i: usize,
        positions: u8,
        from_lists: bool,
        data: &GameData,
        caches: &mut Caches,
    ) -> u8 {
        let Some(s) = self.objects[i].ship.as_ref() else {
            return 0;
        };
        if s.kind < 2 {
            return 0;
        }
        let n = s.positions();
        let counts = self.item_counts(i);
        let own = self.cell_of_object(i);
        let owner = self.objects[i].owner;
        let shooter = self.objects[i].id;
        let flame = self.flame_of(owner) > 0.0;
        let levels = self.levels_of(owner);
        let (mut target, mut family, mut cell) = (None::<usize>, 0u32, (0, 0));
        let (mut d, mut dist) = ((0, 0), 0);
        let mut fired = 0u8;
        for k in 0..n {
            if positions & 1 << k == 0 {
                continue;
            }
            let mut first = true;
            for item in 3 * k..3 * k + 3 {
                let s = self.objects[i].ship.as_ref().unwrap();
                if !s.items_ready[item] || counts[item] == 0 {
                    continue;
                }
                if from_lists || first {
                    first = false;
                    let id = if from_lists {
                        let l = &self.fire_lists[item];
                        if l.count == 0 {
                            continue;
                        }
                        l.entries[0].0
                    } else {
                        s.behaviour.aims[k].target
                    };
                    target = (id != 0).then(|| self.object_index(id)).flatten();
                    let Some(t) = target else {
                        continue;
                    };
                    family = family_of(&self.objects[t]);
                    cell = self.cell_of(t);
                    d = ((own.0 - cell.0).abs(), (own.1 - cell.1).abs());
                    let wide = matches!(self.objects[t].class, ObjectClass::Building(_))
                        && super::trade::building_cells(&self.objects[t]) > 1;
                    if !(family == 0x32 && wide) {
                        dist = super::walk_path::dist(own.0, own.1, cell.0, cell.1);
                    }
                }
                let Some(t) = target else {
                    continue;
                };
                let it = item as u8;
                let class = self.objects[t].record.class_id;
                let reach = if matches!(class, 0x32 | 0x33) && self.objects[t].ship.is_none() {
                    let (x1, y1) = ((own.0 - cell.0 - 1).abs(), (own.1 - cell.1 - 1).abs());
                    self.reaches(i, it, d.0, d.1)
                        || self.reaches(i, it, x1, d.1)
                        || self.reaches(i, it, d.0, y1)
                        || self.reaches(i, it, x1, y1)
                } else {
                    self.reaches(i, it, d.0, d.1)
                };
                if !reach {
                    continue;
                }
                let mut accuracy = self.hit_roll(i, it, dist, t, family);
                let power = self.item_power(i, it);
                let theseus = self.objects[i]
                    .ship
                    .as_ref()
                    .is_some_and(|s| s.has_hero(objects::THESEUS));
                let mut blow = if accuracy == 0 {
                    power * 10 / 100 / 5
                } else {
                    let mut b = power * accuracy as u32 / 100;
                    if flame {
                        b = b * 150 / 100;
                    }
                    if theseus {
                        b = b * 150 / 100;
                    }
                    b
                };
                if counts[item] == 1 && accuracy != 100 && family != 0x32 {
                    accuracy = 0;
                }
                let flaming = match it {
                    9 => {
                        blow = blow * 150 / 100;
                        true
                    }
                    10 => false,
                    _ => flame,
                };
                let shot = super::battle::ItemShot {
                    accuracy,
                    blow: blow as f32 / 15.0,
                    flaming,
                };
                let target_id = self.objects[t].id;
                self.send_volley(
                    (target_id, vec![it], owner, shooter, false, None, Some(shot)),
                    data,
                    caches,
                );
                let Some(s) = self.objects[i].ship.as_ref() else {
                    return fired;
                };
                let rate = if k < 3 && !s.has_hero(objects::ATALANTA) {
                    levels.rate[k]
                } else {
                    objects::HERO_LEVEL
                };
                let wait =
                    crate::combat::reload_ticks(it, rate) + self.ship_draw(i) % RELOAD_JITTER;
                let now = self.ticks;
                self.objects[i].ship.as_mut().unwrap().fire_times[item] = wait + now;
                fired |= 1 << k;
            }
        }
        fired
    }
}

/// What a cell's slot holds for the looks (`grid + 0x18 x cell`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Held {
    /// An object (index).
    Object(usize),
    /// A land creatures' jam (0x1f7): its first member (0x415df0), the
    /// jam's id.
    Jam(usize, u32),
    /// Something the port keeps no object for (a fireball, a tornado in
    /// the flyers' slot): the look takes it and it passes no test.
    Other,
}

/// A cell's slots in the looks' order: the flyers (`+0x14`: harpy,
/// dragon), the occupant (`+0xc`: a ship in it or under way into it, a
/// building on its footprint, a jam), the sea creatures (`+0x18`), and
/// the leviathans' (`+0x10`, the search's alone).
#[derive(Debug, Clone, Copy, Default)]
struct Slots {
    flyer: Option<Held>,
    occupant: Option<Held>,
    sea: Option<Held>,
    deep: Option<Held>,
}

impl Scene {
    /// Every cell's slots as the looks read them.
    fn slot_map(&self) -> HashMap<(i32, i32), Slots> {
        let mut map: HashMap<(i32, i32), Slots> = HashMap::new();
        let t = TILE_SIZE as f32;
        let at = |o: &MapObject| ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        for (k, o) in self.objects.iter().enumerate() {
            match o.class {
                ObjectClass::Ship => {
                    if !self.in_dock(k) {
                        map.entry(at(o)).or_default().occupant = Some(Held::Object(k));
                    }
                }
                ObjectClass::Building(_)
                | ObjectClass::Barbarian(_)
                | ObjectClass::Castle(_)
                | ObjectClass::HarpyNest
                | ObjectClass::CreatureSpawn
                | ObjectClass::HeroHouse => {
                    if let Some((bx, by)) = o.cell {
                        let n = super::trade::building_cells(o);
                        for y in by..by + n {
                            for x in bx..bx + n {
                                let e = map.entry((x, y)).or_default();
                                e.occupant.get_or_insert(Held::Object(k));
                            }
                        }
                    }
                }
                ObjectClass::Monster(0x1fa | 0x1fb | 0x1fd) => {
                    map.entry(at(o)).or_default().sea = Some(Held::Object(k));
                }
                ObjectClass::Monster(0x1f8 | 0x1f9) => {
                    map.entry(at(o)).or_default().flyer = Some(Held::Object(k));
                }
                _ => {}
            }
        }
        // A ship under way into a cell holds its slot too.
        for k in 0..self.objects.len() {
            if let Some(c) = self.reserved_cell(k) {
                map.entry(c)
                    .or_default()
                    .occupant
                    .get_or_insert(Held::Object(k));
            }
        }
        for jam in &self.jams {
            if let Some(k) = jam.members.first().and_then(|&m| self.object_index(m)) {
                let e = map.entry(jam.cell).or_default();
                e.occupant.get_or_insert(Held::Jam(k, jam.id));
            }
        }
        for f in &self.fireballs {
            map.entry(f.cell)
                .or_default()
                .flyer
                .get_or_insert(Held::Other);
        }
        for e in self.effects.iter().filter(|e| e.tornado.is_some()) {
            let c = ((e.x / t).floor() as i32, (e.y / t).floor() as i32);
            map.entry(c).or_default().flyer.get_or_insert(Held::Other);
        }
        for (&id, l) in &self.leviathans {
            if l.in_slot {
                if let Some(k) = self.object_index(id) {
                    map.entry(l.cell).or_default().deep = Some(Held::Object(k));
                }
            }
        }
        map
    }

    /// The fire pass of every ship, a game tick at a time: the gate of
    /// the ship's tick (0x4abb39-0x4abba0), the fire state's call
    /// (0x4c1240) with the ready mask's low 4 bits, then the positions'
    /// marks cleared (0x4abba5).
    pub(super) fn run_fire_pass(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.is_some())
            .map(|o| o.id)
            .collect();
        let mut slots: Option<HashMap<(i32, i32), Slots>> = None;
        for id in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            self.zone_aim(i);
            let Some(ready) = self.fire_gate(i) else {
                continue;
            };
            if slots.is_none() {
                slots = Some(self.slot_map());
            }
            let map = slots.as_ref().unwrap();
            let mask = ready.mask & 0xf;
            match self.objects[i].ship.as_ref().unwrap().behaviour.fire_state {
                1 | 4 => {
                    if self.fire_search(i, mask, ready, map, data, caches) != 0 {
                        if let Some(s) = self.objects[i].ship.as_mut() {
                            s.behaviour.fire_state = 3;
                        }
                    }
                }
                2 => {
                    self.fire_point(i, ready, map, data, caches);
                }
                3 => self.fire_attack(i, mask, map, data, caches),
                _ => {}
            }
            if let Some(i) = self.object_index(id) {
                if let Some(s) = self.objects[i].ship.as_mut() {
                    s.positions_ready = [false; 4];
                }
            }
        }
    }

    /// The port's own zone order (`Task::Zone`, not the original's): the
    /// enemy nearest in the zone becomes the attack's target, by its fire
    /// records (0x4ab3f0 kind 3, `Ship::take_attack`), when no position
    /// aims at it yet.
    fn zone_aim(&mut self, i: usize) {
        let Some(Task::Zone { rect, attack }) = self.objects[i].ship.as_ref().unwrap().task else {
            return;
        };
        let Some(t) = self.zone_target(i, rect, attack) else {
            return;
        };
        let armed = self.objects[i].armed_positions();
        let code = self.player_code(t);
        let s = self.objects[i].ship.as_mut().unwrap();
        if s.behaviour.aims.iter().all(|a| a.target != t) {
            s.take_attack(t, code, armed);
        }
    }

    /// The gate of the ship's tick to 0x4c1240 (0x4abb39-0x4abb99): every
    /// fourth tick (`(now ^ id) & 3 == 0`), a ship that may fire (0x4e6700
    /// 0x15: capability `+0x261 & 0x800`, armed, not limed `+0x258`) or
    /// (type above 1) is a royal ship, rammer or ship killer (0x597a30) or
    /// carries a hero who fights (0x4e66c0, 0x4b4dd0), with the fire task
    /// (`+0x25d & 0x100`) or a switch of guard, ram, board (`+0x684`) on,
    /// afloat (`+0x251 == 0`) and under no status (`+0x25c`: the siren's
    /// song); then 0x4bf9b0 (`ready_mask`), its mask not 0.
    fn fire_gate(&mut self, i: usize) -> Option<Ready> {
        let now = self.ticks;
        let id = self.objects[i].id;
        if (now ^ id) & 3 != 0 || self.being_built(id) {
            return None;
        }
        let o = &self.objects[i];
        let c = o.combat.as_ref()?;
        let s = o.ship.as_ref()?;
        if c.hull <= 0.0 || self.charmed.contains_key(&id) {
            return None;
        }
        let held = self.hunt_holds(id) || self.landing_holds(id);
        let fires = !held && o.is_armed() && c.lime <= 0.0;
        let fights = s.kind > 1
            && (matches!(s.kind, 2 | 5 | 6) || s.heroes.iter().any(|&h| h != objects::ORPHEUS));
        if !fires && !fights {
            return None;
        }
        let b = s.behaviour;
        if !s.firing && !(b.guard || b.ram || b.board) {
            return None;
        }
        let r = self.ready_mask(i);
        (r.mask != 0).then_some(r)
    }

    /// The box of a look (0x4bfc60, 0x4c0eb0): the highest position marked
    /// ready gives its range (0x5980f4[L x 4 + k], L `reach_level`); none,
    /// or 0, 4 with ram or board on (else 0). Its half each way about the
    /// ship's cell, clipped to the map: `(x0, y0, x1, y1)` (ends out) and
    /// the position (0 with none).
    fn look_bounds(&self, i: usize) -> ((i32, i32, i32, i32), usize) {
        let s = self.objects[i].ship.as_ref().unwrap();
        let mut r = 0;
        let mut top = 0;
        if let Some(k) = (0..s.positions()).rev().find(|&k| s.positions_ready[k]) {
            top = k;
            r = look_box(self.reach_level(i, k), k);
        }
        if r == 0 && (s.behaviour.ram || s.behaviour.board) {
            r = 4;
        }
        let half = (r / 2) as i32;
        let own = self.cell_of_object(i);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let bounds = (
            (own.0 - half).max(0),
            (own.1 - half).max(0),
            (own.0 + 1 + half).min(w),
            (own.1 + 1 + half).min(h),
        );
        (bounds, top)
    }

    /// The tests every look makes of what a cell holds: not the ship
    /// itself; an enemy (side 2 but the sea monster 0x1fb, or another
    /// player at war: 0x4efe20 < 0; the port's players are all at war);
    /// its player in `+0x689`; there (vt+0x2c: life left, not hidden);
    /// not a ship found on the cell it is under way into (`+0x5ab`).
    fn look_passes(&self, i: usize, k: usize, cell: (i32, i32)) -> bool {
        let (me, o) = (&self.objects[i], &self.objects[k]);
        if k == i {
            return false;
        }
        let side2 = Self::side_of(o) == 2 && o.record.class_id != 0x1fb;
        if !(side2 || Self::are_enemies(me, o)) {
            return false;
        }
        let Some((n, slot)) = o.owner else {
            return false;
        };
        let players = me.ship.as_ref().unwrap().behaviour.players;
        if players & 1 << ((slot + n * 4) & 0x1f) == 0 {
            return false;
        }
        if !o.combat.as_ref().is_some_and(|c| c.hull > 0.0)
            || self.creature_hidden(o.id)
            || self.shoal_gone(o.id)
        {
            return false;
        }
        !(o.ship.is_some() && self.reserved_cell(k) == Some(cell))
    }

    /// 0x50ebb0: whether `k` (a jam's member answering as the jam, class
    /// 0x1f7) is of a class in ship `i`'s `+0x68d`.
    fn look_class(&self, i: usize, k: usize, jam: bool) -> bool {
        let o = &self.objects[k];
        let bit = if jam {
            crate::combat::class_bit(0x1f7, None)
        } else {
            crate::combat::class_bit(o.record.class_id, o.ship.as_ref().map(|s| s.kind))
        };
        self.objects[i].ship.as_ref().unwrap().behaviour.classes & bit != 0
    }

    /// Clears the candidate lists (0x5c7ed0): `n` of them.
    fn clear_lists(&mut self, n: usize) {
        for l in &mut self.fire_lists[..n] {
            l.count = 0;
        }
    }

    /// 0x4c0cd0(object `k`, position `top`, cell): for the positions from
    /// `top` down marked ready, `k` an enemy of a player in `+0x689`, each
    /// item of the position ready with a unit aboard that reaches it
    /// (0x4bf5f0 at the ship's cell less `cell`, signed) goes on the item's
    /// list (a leviathan, 0x192, only for items 0-4: `i x 0x191 < 0x645`),
    /// with the cell `W x y + x`; the count stops at 49, the 50th entry
    /// written over.
    fn list_cell(&mut self, i: usize, k: usize, top: usize, cell: (i32, i32)) {
        let own = self.cell_of_object(i);
        let counts = self.item_counts(i);
        let (me, o) = (&self.objects[i], &self.objects[k]);
        let side2 = Self::side_of(o) == 2 && o.record.class_id != 0x1fb;
        let Some((n, slot)) = o.owner else {
            return;
        };
        let s = me.ship.as_ref().unwrap();
        if !(side2 || Self::are_enemies(me, o))
            || s.behaviour.players & 1 << ((slot + n * 4) & 0x1f) == 0
        {
            return;
        }
        let leviathan = o.record.class_id == 0x192;
        let (id, w) = (o.id, self.nav.width as i32);
        let (ready, marked) = (s.items_ready, s.positions_ready);
        for p in (0..=top).rev() {
            if !marked[p] {
                continue;
            }
            for item in 3 * p..3 * p + 3 {
                if !ready[item]
                    || counts[item] == 0
                    || !self.reaches(i, item as u8, own.0 - cell.0, own.1 - cell.1)
                    || (leviathan && item * 0x191 >= 0x645)
                {
                    continue;
                }
                let l = &mut self.fire_lists[item];
                l.entries[l.count as usize] = (id, w * cell.1 + cell.0);
                if l.count < 0x31 {
                    l.count += 1;
                }
            }
        }
    }

    /// 0x4e8af0 mode 10 over the 9 lists (11 for types 8 and 9): each
    /// list of a position in `+0x685` with entries draws the ship's
    /// generator, `% count`, and that entry goes to slot 0; the positions
    /// so drawn.
    fn draw_lists(&mut self, i: usize) -> u8 {
        let s = self.objects[i].ship.as_ref().unwrap();
        let n = if s.positions() == 4 { 11 } else { 9 };
        let positions = s.behaviour.positions;
        let mut out = 0u8;
        for item in 0..n {
            let k = item / 3;
            let count = self.fire_lists[item].count;
            if positions & 1 << k == 0 || count == 0 {
                continue;
            }
            let pick = (self.ship_draw(i) % count as u32) as usize;
            out |= 1 << k;
            let l = &mut self.fire_lists[item];
            l.entries[0] = l.entries[pick];
        }
        out
    }

    /// The cells of a look's box, row by row, each with what its first
    /// held slot gives (the leviathans' `+0x10` only with `deep`, and a
    /// leviathan there only while showing itself, `+0x74 == 1`).
    #[allow(clippy::type_complexity)]
    fn look_cells(
        &self,
        i: usize,
        map: &HashMap<(i32, i32), Slots>,
        deep: bool,
    ) -> (Vec<((i32, i32), Held)>, usize) {
        let ((x0, y0, x1, y1), top) = self.look_bounds(i);
        let mut cells = Vec::new();
        for y in y0..y1 {
            for x in x0..x1 {
                let Some(sl) = map.get(&(x, y)) else {
                    continue;
                };
                let deep_held = sl.deep.filter(|&h| match h {
                    Held::Object(k) => {
                        self.objects[k].record.class_id != 0x192
                            || self.leviathan_state(self.objects[k].id) == Some(SHOWING)
                    }
                    _ => true,
                });
                let held =
                    sl.flyer
                        .or(sl.occupant)
                        .or(sl.sea)
                        .or(if deep { deep_held } else { None });
                if let Some(h) = held {
                    cells.push(((x, y), h));
                }
            }
        }
        (cells, top)
    }

    /// 0x4bfc60(mask), the search of fire states 1 and 4. Over the box
    /// (`look_cells` with the leviathans' slot), each cell's object that
    /// passes (`look_passes`) meets each position whose target's id
    /// (`+0x640`) it is, the position's pointer set to it (a jam meets the
    /// positions whose target is among its members, the pointer kept);
    /// with guard on and not all three met (`local_5 != 7` or `0xf`) a
    /// side-1 object needs its player in `+0x689` and any its class in
    /// `+0x68d` (0x50ebb0) or the cell is done, and it goes on the lists
    /// (0x4c0cd0, a jam as its first member); with ram or board on and a
    /// switch marked, on the at-will list (0x4c0bc0). Then the met
    /// positions in `mask` fire at their targets (0x4c0230); the mask
    /// loses those fired, and the result is the positions in state 3 among
    /// those not fired (none when all fired). None met: with anything met
    /// the result is 0, else every position in state 3. With guard on,
    /// `local_5 != 7` and mask left, the lists of positions 0-2 and 9-10
    /// with entries give bits (8 for 9-10), drawn (`draw_lists`) and fired
    /// from (0x4c0230 with lists, its mask the draw's, the bits' and the
    /// mask's); then the at-will close (0x4e8db0, 0x4c0b30). The result
    /// sets the fire state 3 (0x4c1240).
    fn fire_search(
        &mut self,
        i: usize,
        mut mask: u8,
        ready: Ready,
        map: &HashMap<(i32, i32), Slots>,
        data: &GameData,
        caches: &mut Caches,
    ) -> u8 {
        let b = self.objects[i].ship.as_ref().unwrap().behaviour;
        let full = if self.objects[i].ship.as_ref().unwrap().positions() == 4 {
            0xf
        } else {
            7
        };
        if b.guard {
            self.clear_lists(12);
        }
        let ramboard = b.ram || b.board;
        let route = self.at_will_route(i);
        let mut at_will = Vec::new();
        let mut met = 0u8;
        let (cells, top) = self.look_cells(i, map, true);
        for (cell, held) in cells {
            let (k, jam) = match held {
                Held::Object(k) => (k, None),
                Held::Jam(k, j) => (k, Some(j)),
                Held::Other => continue,
            };
            if !self.look_passes(i, k, cell) {
                continue;
            }
            let id = self.objects[k].id;
            let members = jam.and_then(|j| {
                self.jams
                    .iter()
                    .find(|x| x.id == j)
                    .map(|x| x.members.clone())
            });
            let s = self.objects[i].ship.as_ref().unwrap();
            for p in 0..4 {
                let t = s.behaviour.aims[p].target;
                let hit = match &members {
                    Some(m) => t != 0 && m.contains(&t),
                    None => t == id,
                };
                if hit {
                    met |= 1 << p;
                }
            }
            if b.guard && met != full {
                let o = &self.objects[k];
                let player_ok = o
                    .owner
                    .is_some_and(|(n, sl)| b.players & 1 << ((sl + n * 4) & 0x1f) != 0);
                if (Self::side_of(o) == 1 && !player_ok) || !self.look_class(i, k, jam.is_some()) {
                    continue;
                }
                self.list_cell(i, k, top, cell);
            }
            if ramboard && (ready.ram || ready.board) {
                self.at_will_take(i, k, cell, route, &mut at_will);
            }
        }
        let fired_on = met & mask;
        let aims = self.objects[i].ship.as_ref().unwrap().behaviour.aims;
        let in3 = |bits: u8| {
            (0..4)
                .filter(|&p| aims[p].state == 3 && bits & 1 << p != 0)
                .fold(0u8, |a, p| a | 1 << p)
        };
        let ret = if fired_on != 0 {
            let f = self.fire_items(i, fired_on, false, data, caches);
            mask ^= f;
            if f == fired_on {
                0
            } else {
                in3(f ^ fired_on)
            }
        } else if met != 0 {
            0
        } else {
            in3(0xf)
        };
        if b.guard && met != 7 && mask != 0 {
            let c = |a: usize, n: usize| {
                (a..a + n)
                    .map(|x| self.fire_lists[x].count as u32)
                    .sum::<u32>()
            };
            let (p0, p1, p2, p3) = (c(0, 3), c(3, 3), c(6, 3), c(9, 2));
            if p0 + p1 + p2 + p3 != 0 {
                let bits = ((p2 != 0) as u8 * 4)
                    | ((p1 != 0) as u8 * 2)
                    | (p0 != 0) as u8
                    | ((p3 != 0) as u8 * 8);
                let f = self.draw_lists(i);
                self.fire_items(i, f & bits & mask, true, data, caches);
            }
        }
        if ramboard {
            self.at_will_close(i, &at_will, ready.ram, ready.board);
        }
        ret
    }

    /// 0x4c0eb0, fire state 2 (and the attack's end): with guard on the
    /// lists are cleared (9, 12 for types 8 and 9); over the box
    /// (`look_cells` without the leviathans' slot) each object that passes
    /// and, side 1, has its player in `+0x689`, and is of a class in
    /// `+0x68d` (a jam answering as its first member) goes on the lists
    /// with guard on (0x4c0cd0) and, a switch marked, on the at-will list.
    /// Then the lists are drawn (`draw_lists`, whether guard is on or not:
    /// off, they hold what the last look left) and those positions fire
    /// from them (0x4c0230); then the at-will close. The state stays.
    fn fire_point(
        &mut self,
        i: usize,
        ready: Ready,
        map: &HashMap<(i32, i32), Slots>,
        data: &GameData,
        caches: &mut Caches,
    ) -> u8 {
        let s = self.objects[i].ship.as_ref().unwrap();
        let b = s.behaviour;
        if b.guard {
            self.clear_lists(if s.positions() == 4 { 12 } else { 9 });
        }
        let ramboard = b.ram || b.board;
        let route = self.at_will_route(i);
        let mut at_will = Vec::new();
        let (cells, top) = self.look_cells(i, map, false);
        for (cell, held) in cells {
            let (k, jam) = match held {
                Held::Object(k) => (k, false),
                Held::Jam(k, _) => (k, true),
                Held::Other => continue,
            };
            if !self.look_passes(i, k, cell) || !self.look_class(i, k, jam) {
                continue;
            }
            if b.guard {
                self.list_cell(i, k, top, cell);
            }
            if ramboard && (ready.ram || ready.board) {
                self.at_will_take(i, k, cell, route, &mut at_will);
            }
        }
        let f = self.draw_lists(i);
        let fired = if f != 0 {
            self.fire_items(i, f, true, data, caches)
        } else {
            0
        };
        if ramboard {
            self.at_will_close(i, &at_will, ready.ram, ready.board);
        }
        fired
    }

    /// 0x4c07c0: over the positions in state 3 (the last gives the
    /// target's id `+0x640`), the aim, the largest aiming distance
    /// 0x598130 of their items aboard (at `reach_level`) halved, and the
    /// distance to keep, 1, 2 or 3 by the sum of the items' power
    /// (0x4bfc20 at 100, once a unit) reaching 1, 2, 3 cells across
    /// (0x4bf5f0(i, d, 0)): 1 when 1's sum beats 2's, else 2; then 3 when
    /// 3's beats that. `None` with no position in state 3 (not decoded:
    /// the original's id and player are then uninitialised).
    #[allow(clippy::needless_range_loop)]
    fn attack_range(&self, i: usize) -> Option<(i32, u32, i8, u32)> {
        let s = self.objects[i].ship.as_ref().unwrap();
        let counts = self.item_counts(i);
        let n = s.positions();
        let (mut sums, mut aim, mut id) = ([0u32; 3], 0u32, None);
        let mut code = -1;
        for p in 0..n {
            if s.behaviour.aims[p].state != 3 {
                continue;
            }
            id = Some(s.behaviour.aims[p].target);
            code = s.behaviour.aims[p].player;
            for item in 3 * p..3 * p + 3 {
                let it = item as u8;
                let r = [1, 2, 3].map(|d| self.reaches(i, it, d, 0));
                if counts[item] == 0 {
                    continue;
                }
                let level = if p < 3 {
                    self.reach_level(i, p)
                } else {
                    objects::HERO_LEVEL
                };
                aim = aim.max(aim_cells(level, it));
                let power = self.item_power(i, it);
                for _ in 0..counts[item] {
                    for d in 0..3 {
                        if r[d] {
                            sums[d] += power;
                        }
                    }
                }
            }
        }
        let id = id?;
        let (d, best) = if sums[1] < sums[0] {
            (1, sums[0])
        } else {
            (2, sums[1])
        };
        let d = if best < sums[2] { 3 } else { d };
        Some((d, id, code, aim / 2))
    }

    /// 0x4b9140's route to the target's cell `goal` (0x4a6270 mode 5,
    /// tolerance `d`: 0x50ebf0's, 0 for a ship and 1 for the others, made
    /// `d` when smaller). The first plan after the attack's records
    /// (`+0x593`) is made at once (0x4a6330); later ones (0x4a6150) once
    /// 25 ticks have passed since the last (`+0x57b`, `+0x57f`), if the
    /// ship changed cells, the goal changed or it lies still, and it is
    /// not turning (move state 2). (0x4a6150's goal-is-its-cell end is not
    /// met: the target within `d` stops the attack first. Its wait
    /// counter `+0x591` is the port's own mover's.) Then 0x4a51b0 with
    /// `keep` 0, the route within `d` of the goal; the plan's tick, cell
    /// and goal kept (0x4a6330, 0x4a6100).
    fn attack_plan(&mut self, i: usize, goal: (i32, i32), d: i32) {
        let now = self.ticks;
        let own = self.cell_of_object(i);
        let s = self.objects[i].ship.as_ref().unwrap();
        let p = s.attack_plan;
        if !p.first_plan {
            let changed = own != p.plan_from || goal != p.plan_goal || s.move_state == 0;
            if !changed || s.move_state == 2 || now.wrapping_sub(p.planned) < PLAN_TICKS {
                return;
            }
        }
        let t = TILE_SIZE as f32;
        let (x, y) = (goal.0 as f32 * t + t / 2.0, goal.1 as f32 * t + t / 2.0);
        let route = self.plan_route_within(i, x, y, d);
        let s = self.objects[i].ship.as_mut().unwrap();
        if let Some(path) = route {
            s.path = path;
        }
        s.attack_plan = objects::RoutePlan {
            planned: now,
            plan_from: own,
            plan_goal: goal,
            first_plan: false,
        };
    }

    /// 0x4c09c0(mask), fire state 3, the attack (mask 0: nothing). With
    /// `attack_range`'s distance `d`, 0x4b9140(d, d, target): the target
    /// found (0x4ac050; the port: there and still an enemy), its cell
    /// (0x50ebf0 with 1: vt+0x28's) and its own both within `d` of the
    /// ship's cell (0x557480): the move stopped (event 8 0x80) and the
    /// fire state 1. Else it routes to it (`attack_plan`) and: not found,
    /// the stop with its fire part (event 8 0x180) and notice 0x13a `{id,
    /// 0x16}` to its group (0x4a1800); found within the aim (0x557480 of
    /// its cell), the positions in state 3 fire at their targets (0x4c0230).
    /// Then with guard on and not 7 fired, 0x4bf9b0 again (its switches'
    /// marks now cleared) and, non-zero, 0x4c0eb0. It first sets `+0x594`
    /// (`attack_step`, read by the step's wait, 0x4a3d76); `+0x62f`, the
    /// ship's cell written when the route is planned, has no reader.
    fn fire_attack(
        &mut self,
        i: usize,
        mask: u8,
        map: &HashMap<(i32, i32), Slots>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        if mask == 0 {
            return;
        }
        if let Some(s) = self.objects[i].ship.as_mut() {
            s.attack_step = true;
        }
        let Some((d, target, code, aim)) = self.attack_range(i) else {
            return;
        };
        let own = self.cell_of_object(i);
        // 0x4b9140 -> 0x4ac050 with the record's player code (`+0x644`):
        // the target is found while it is that player's (anything of its
        // id for -1), whoever's ally it has become since.
        let found = self
            .find_by_code(code, target)
            .filter(|&t| target != 0 && self.objects[t].combat.is_some());
        let Some(t) = found else {
            let id = self.objects[i].id;
            let s = self.objects[i].ship.as_mut().unwrap();
            // Event 8 with 0x180 (0x4aa540): the move stopped and the
            // stop's fire part. Ordered at a jam, the ship waits for the
            // task to send it at the jam's next member (`run_jam_attacks`).
            s.path.clear();
            if matches!(s.task, Some(Task::Attack { jam: None, .. })) {
                s.task = None;
            }
            s.fire_wind_down();
            self.post_notice(id, 0x13a, target);
            return;
        };
        // 0x50ebf0 gives no cell for the other families (a leviathan,
        // 400; 700; a jam, 0x1f7, and the land creatures not listed):
        // 0x4b9140 returns 0 unrouted, its distance left unwritten (not
        // decoded: 0x4c09c0 then compares an uninitialised local with the
        // aim; the port fires nothing at them here).
        if no_cell_family(&self.objects[t]) {
            self.attack_guard(i, 0, map, data, caches);
            return;
        }
        let cell = self.cell_of(t);
        let dist = walk_path::dist2(cell.0, cell.1, own.0, own.1);
        if dist <= d {
            // Event 8 with 0x80: the move stops at the end of the step it
            // is on, ships keeping to the cells' middles.
            let s = self.objects[i].ship.as_mut().unwrap();
            s.path.truncate(1);
            s.behaviour.fire_state = 1;
            return;
        }
        self.attack_plan(i, cell, d);
        let mut fired = 0;
        if dist as u32 <= aim {
            let s = self.objects[i].ship.as_ref().unwrap();
            let in3 = (0..4)
                .filter(|&p| s.behaviour.aims[p].state == 3)
                .fold(0u8, |a, p| a | 1 << p);
            if in3 != 0 {
                fired = self.fire_items(i, in3, false, data, caches);
            }
        }
        self.attack_guard(i, fired, map, data, caches);
    }

    /// 0x4c09c0's end: with guard on and not 7 fired, 0x4bf9b0 again and,
    /// non-zero, 0x4c0eb0.
    fn attack_guard(
        &mut self,
        i: usize,
        fired: u8,
        map: &HashMap<(i32, i32), Slots>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(s) = self.objects[i].ship.as_ref() else {
            return;
        };
        if s.behaviour.guard && fired != 7 {
            let r = self.ready_mask(i);
            if r.mask != 0 {
                self.fire_point(i, r, map, data, caches);
            }
        }
    }
}

#[cfg(test)]
impl Scene {
    /// The at-will list of ship `i`'s look (0x4bfc60, or 0x4c0eb0 with
    /// `point`) as its loop builds it, nothing fired.
    pub(super) fn at_will_list(&self, i: usize, point: bool) -> Vec<usize> {
        let map = self.slot_map();
        let b = self.objects[i].ship.as_ref().unwrap().behaviour;
        let full = if self.objects[i].ship.as_ref().unwrap().positions() == 4 {
            0xf
        } else {
            7
        };
        let route = self.at_will_route(i);
        let (mut list, mut met) = (Vec::new(), 0u8);
        for (cell, held) in self.look_cells(i, &map, !point).0 {
            let (k, jam) = match held {
                Held::Object(k) => (k, false),
                Held::Jam(k, _) => (k, true),
                Held::Other => continue,
            };
            if !self.look_passes(i, k, cell) {
                continue;
            }
            for p in 0..4 {
                if b.aims[p].target == self.objects[k].id {
                    met |= 1 << p;
                }
            }
            if (point || b.guard && met != full) && !self.look_class(i, k, jam) {
                continue;
            }
            self.at_will_take(i, k, cell, route, &mut list);
        }
        list
    }
}

/// The random part of an item's reload (0x4c0230: a draw % 0x1e).
const RELOAD_JITTER: u32 = 0x1e;

/// The ticks between a route's plans (`+0x57f`, 25 as 0x4ab6ab sets it).
const PLAN_TICKS: u32 = 25;

/// Whether 0x50ebf0 gives no cell for `o` (its vt+0x28 cell for family
/// 0x32, ships and the creatures 500-0x1f6 and 0x1f8-0x1fb; none for the
/// others).
fn no_cell_family(o: &MapObject) -> bool {
    match family_of(o) {
        0x32 | 200 => false,
        500 => !matches!(o.record.class_id, 500..=0x1f6 | 0x1f8..=0x1fb),
        _ => true,
    }
}

/// An object's family (0x50e600 over its class `+0x50`): ships 200,
/// buildings 0x32-0x4a 0x32, creatures 500-0x1fe 500, 400-0x193 400,
/// 700-0x2c2 700, else 0.
pub(super) fn family_of(o: &MapObject) -> u32 {
    if o.ship.is_some() {
        return 200;
    }
    match o.record.class_id {
        200..=0xd3 => 200,
        0x32..=0x4a => 0x32,
        500..=0x1fe => 500,
        400..=0x193 => 400,
        700..=0x2c2 => 700,
        _ => 0,
    }
}

/// A candidate list (0x5c7ed0 + i x 0x191): its count, at most 49 (the
/// 50th entry is written over), and its entries, an object's id and its
/// cell (`W x y + x`).
#[derive(Debug, Clone, Copy)]
pub(super) struct FireList {
    pub count: u8,
    pub entries: [(u32, i32); 50],
}

impl Default for FireList {
    fn default() -> Self {
        Self {
            count: 0,
            entries: [(0, 0); 50],
        }
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
        // The boxes: 9, 13 for position 2; the aiming distances.
        assert_eq!(
            (0..4).map(|k| look_box(0, k)).collect::<Vec<_>>(),
            [9, 9, 13, 9]
        );
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
