//! Bitmap fonts (resource kind 3).
//!
//! ```text
//! +0x00  i16  atlas count n
//! +0x02  u32  absolute offsets in the .DKD of n atlas bitmaps (colour variants)
//! +0x42  u8   transparent palette index of the atlases
//! +0x64  u16  glyph count
//! +0x66  u16  unknown
//! +0x68  u16  unknown
//! +0x6a  u16  unknown (spacing?)
//! +0x6c  u16  line height
//! +0x6e  u16  last character code
//! +0x70  glyph records: u16 x, y, width, height, character
//! ```
//!
//! The atlases are ordinary 8-bit DIBs stored in the data file without an
//! index entry; the font loader (`mfcfnt.cpp`, 0x546aa0 in ANCIENT.EXE)
//! reads them by offset.

use std::collections::HashMap;

use crate::bytes::Reader;
use crate::{invalid, Archive, Bitmap, Result};

#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone)]
pub struct Font {
    pub line_height: u16,
    pub transparent: u8,
    pub glyphs: HashMap<u8, Glyph>,
    pub atlas_offsets: Vec<u32>,
}

impl Font {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data, "font");
        let atlases = r.u16()? as i16;
        if !(1..=8).contains(&atlases) {
            return Err(invalid("font", format!("{atlases} atlases")));
        }
        let atlas_offsets = (0..atlases).map(|_| r.u32()).collect::<Result<Vec<_>>>()?;
        let transparent = *data
            .get(0x42)
            .ok_or_else(|| invalid("font", "truncated header"))?;
        let mut r = Reader::at(data, 0x64, "font header");
        let count = r.u16()?;
        let _unknown = (r.u16()?, r.u16()?, r.u16()?);
        let line_height = r.u16()?;
        let _last_char = r.u16()?;
        let mut glyphs = HashMap::with_capacity(count as usize);
        for _ in 0..count {
            let (x, y, width, height, ch) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?, r.u16()?);
            if let Ok(ch) = u8::try_from(ch) {
                glyphs.insert(
                    ch,
                    Glyph {
                        x,
                        y,
                        width,
                        height,
                    },
                );
            }
        }
        Ok(Self {
            line_height,
            transparent,
            glyphs,
            atlas_offsets,
        })
    }

    /// Loads atlas `index` from the font's archive.
    pub fn atlas(&self, archive: &Archive, index: usize) -> Result<Bitmap> {
        let offset = *self
            .atlas_offsets
            .get(index)
            .ok_or_else(|| invalid("font", format!("no atlas {index}")))?;
        let data = archive
            .data_at(offset)
            .ok_or_else(|| invalid("font", format!("atlas offset {offset:#x} out of range")))?;
        Bitmap::parse(data)
    }
}
