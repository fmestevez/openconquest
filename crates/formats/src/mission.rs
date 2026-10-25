//! Mission object records (`OBJ_xxxxxxxx`, kind 12).
//!
//! Each record is a little-endian dword array whose first element is the
//! object class; the meaning of the remaining fields depends on the class.
//! The class-to-implementation mapping comes from the game's loader
//! (`PlaySystemClassTy::CreateSystemObjects` at 0x51e810 in ANCIENT.EXE).

use crate::{Archive, ResourceKind, Result};

/// Object classes, named after the game's implementation classes. The
/// mapping from record class ids comes from the loader and the factory
/// registry at 0x5c0e24 (pairs of internal type id and factory function).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectClass {
    /// 0xc8..=0xd3 (`ShipClassTy`). Missions only use 0xc8; the ship type
    /// (0-11: boat, merchant, ship killer, men killer, rammer, boarder,
    /// bomber, royal ship, then the Greek and Persian unique ships) is field 7.
    Ship,
    /// 0x32..=0x3e: Greek/Persian buildings (`TownObjClassTy`,
    /// `LandObjClassTy`, `StudioClassTy`, `SmithyClassTy`, `DockClassTy`,
    /// `SchoolClassTy`, `TempleSeaClassTy`, `TempleWarClassTy`).
    Building(u32),
    /// 0x3f..=0x41 (`BarbarianClassTy`).
    Barbarian(u32),
    /// 0x42 sea castle, 0x43 war castle (`CastleClassTy`).
    Castle(u32),
    /// 0x44, 0x45 (`HarpyNestClassTy`).
    HarpyNest,
    /// 0x46..=0x48 (`CreaturesFlatClassTy`): creature spawn areas.
    CreatureSpawn,
    /// 0x49 (`HeroHouseClassTy`).
    HeroHouse,
    /// 0x190 (`AmberClassTy`): amber deposit.
    Amber,
    /// 0x191 (`FishClassTy`): fishing zone (x, y, w, h).
    FishZone,
    /// 0x192 (`WhaleClassTy`): leviathan zone (x, y, w, h).
    WhaleZone,
    /// 0x1f4..=0x1fd: monsters (cyclops, harpy, medusa, sea monster, siren)
    /// and the landing party.
    Monster(u32),
    /// 0x258 (`WhirlObjClassTy`).
    Whirlpool,
    /// 0x259 big, 0x25a medium (`ReefObjClassTy`).
    Reef(u32),
    /// 0x25b..=0x25d: lightning, tornado, `SlightClassTy`.
    Weather(u32),
    /// 0x25e (`VolcanoClassTy`): lava flow.
    Lava,
    /// 0x2bc (`TreasClassTy`): treasure bookkeeping, not placed on the map.
    Treasure,
    /// 0x2be (`SheepsObjClassTy`): sheep flock.
    Sheep,
    /// Anything else, e.g. 0x2bf (`TraksClassTy`, music tracks) and 0x2c1
    /// (`SkarbClassTy`).
    Other(u32),
}

impl ObjectClass {
    pub fn from_id(id: u32) -> Self {
        match id {
            0xc8..=0xd3 => Self::Ship,
            0x32..=0x3e => Self::Building(id),
            0x3f..=0x41 => Self::Barbarian(id),
            0x42 | 0x43 => Self::Castle(id),
            0x44 | 0x45 => Self::HarpyNest,
            0x46..=0x48 => Self::CreatureSpawn,
            0x49 => Self::HeroHouse,
            0x190 => Self::Amber,
            0x191 => Self::FishZone,
            0x192 => Self::WhaleZone,
            0x1f4..=0x1fd => Self::Monster(id),
            0x258 => Self::Whirlpool,
            0x259 | 0x25a => Self::Reef(id),
            0x25b..=0x25d => Self::Weather(id),
            0x25e => Self::Lava,
            0x2bc => Self::Treasure,
            0x2be => Self::Sheep,
            other => Self::Other(other),
        }
    }
}

/// Field indices shared by player-owned objects (buildings and ships).
pub mod field {
    /// 1 = Greek, 3 = Persian, 4 = barbarian (also the sprite name digit).
    pub const NATION: usize = 2;
    /// Owning player slot (0-3).
    pub const PLAYER: usize = 3;
}

#[derive(Debug, Clone)]
pub struct ObjectRecord {
    /// Object id, from the record name.
    pub id: u32,
    pub class_id: u32,
    pub fields: Vec<u32>,
}

impl ObjectRecord {
    pub fn class(&self) -> ObjectClass {
        ObjectClass::from_id(self.class_id)
    }

    /// Dword `i` of the record (0 is the class id).
    pub fn field(&self, i: usize) -> Option<u32> {
        self.fields.get(i).copied()
    }
}

/// Reads all object records of a mission archive, ordered by id.
pub fn object_records(archive: &Archive) -> Result<Vec<ObjectRecord>> {
    let mut out = Vec::new();
    for e in archive.entries() {
        if e.kind != ResourceKind::Record {
            continue;
        }
        let Some(hex) = e.name.strip_prefix("OBJ_") else {
            continue;
        };
        let Ok(id) = u32::from_str_radix(hex, 16) else {
            continue;
        };
        let data = archive.read_entry(e)?;
        let fields: Vec<u32> = data
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let Some(&class_id) = fields.first() else {
            continue;
        };
        out.push(ObjectRecord {
            id,
            class_id,
            fields,
        });
    }
    out.sort_by_key(|r| r.id);
    Ok(out)
}

/// One player slot of a mission (from the `DESCRIPTOR` record).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerInfo {
    /// Controlled by the computer.
    pub computer: bool,
    /// 1 = Greek, 3 = Persian, 4 = barbarian.
    pub nation: u32,
    /// Colour slot; together with the nation it identifies the owner of
    /// objects (`ObjectRecord` fields `NATION` and `PLAYER`).
    pub slot: u32,
    /// Starting amber (money).
    pub amber: u32,
    /// Starting fish.
    pub fish: u32,
}

/// Players of a mission.
///
/// `DESCRIPTOR` (kind 0) layout, as far as known:
///
/// ```text
/// +0x08  u32  map width (cells)
/// +0x0c  u32  map height (cells)
/// +0x10  u32  player count n
/// +0x14  n x 104-byte player blocks:
///        +0x00 u8  0 = human, 1 = computer (upper bytes unknown)
///        +0x04 u32 amber
///        +0x08 u32 nation
///        +0x0c u32 colour slot
///        +0x10 u32 fish
/// ```
pub fn players(archive: &Archive) -> Result<Vec<PlayerInfo>> {
    const BLOCK: usize = 0x68;
    let data = archive.read("DESCRIPTOR", ResourceKind::Raw)?;
    let u32_at = |o: usize| {
        data.get(o..o + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| crate::invalid("mission descriptor", format!("truncated at {o:#x}")))
    };
    let count = u32_at(0x10)? as usize;
    (0..count.min(8))
        .map(|i| {
            let b = 0x14 + i * BLOCK;
            Ok(PlayerInfo {
                computer: data.get(b).copied().unwrap_or(1) != 0,
                amber: u32_at(b + 4)?,
                nation: u32_at(b + 8)?,
                slot: u32_at(b + 0xc)?,
                fish: u32_at(b + 0x10)?,
            })
        })
        .collect()
}
