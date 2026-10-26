//! Readers for the data files of *Ancient Conquest* (Re:Action Entertainment, 1999).
//!
//! See `docs/FORMATS.md` in the repository for a description of every format
//! handled here.

pub mod anim;
pub mod archive;
pub mod avi;
pub mod font;
pub mod image;
pub mod install;
pub mod installer;
pub mod lz;
pub mod map;
pub mod mission;
pub mod pe;
pub mod sound;

mod bytes;

pub use anim::{Composite, CompositeCell, Sequence};
pub use archive::{Archive, Entry, ResourceKind};
pub use font::{Font, Glyph};
pub use image::{parse_palette_animation, Bitmap, Mask, Palette, PaletteCycle, Rgb, Sprite};
pub use map::{MapGrid, TileRef, TileSet, TileSetLibrary};
pub use mission::{
    briefing, briefing_lines, briefing_text, conditions, object_records, players, ObjectClass,
    ObjectRecord, PlayerInfo, StartLevels,
};
pub use pe::string_table;
pub use sound::{load_sound, sound_choices, Sound};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid {what}: {why}")]
    Invalid { what: &'static str, why: String },
    #[error("resource not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(what: &'static str, why: impl Into<String>) -> Error {
    Error::Invalid {
        what,
        why: why.into(),
    }
}
