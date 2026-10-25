//! Animation resources: sequences (kind 7) and layered composites (kind 8).

use crate::bytes::{latin1_cstr, Reader};
use crate::{invalid, Archive, Entry, ResourceKind, Result};

/// A sequence (kind 7): an alias naming a run of numbered sprite frames,
/// e.g. `AMBER_` -> `AMBER_00`, `AMBER_01`, ...
///
/// The record starts with the NUL-terminated frame prefix; the rest is
/// stale buffer contents. The frame count is the entry's width.
#[derive(Debug, Clone)]
pub struct Sequence {
    pub prefix: String,
    pub frames: u16,
}

impl Sequence {
    pub fn parse(data: &[u8], entry: &Entry) -> Result<Self> {
        let prefix = latin1_cstr(data);
        if prefix.is_empty() {
            return Err(invalid("sequence", format!("{}: empty prefix", entry.name)));
        }
        Ok(Self {
            prefix,
            frames: entry.width,
        })
    }

    /// Resolves the frame sprite names. The number of digits in the frame
    /// suffix is not stored, so it is inferred from the sprites that exist.
    pub fn frame_names(&self, sprites: &Archive) -> Vec<String> {
        for digits in [2usize, 1, 3, 4] {
            if sprites
                .find(
                    &format!("{}{:0digits$}", self.prefix, 0),
                    ResourceKind::Sprite,
                )
                .is_some()
            {
                return (0..self.frames as usize)
                    .map(|i| format!("{}{:0digits$}", self.prefix, i))
                    .collect();
            }
        }
        if sprites.find(&self.prefix, ResourceKind::Sprite).is_some() {
            return vec![self.prefix.clone()];
        }
        Vec::new()
    }
}

/// One cell of a composite: a sprite drawn at an offset.
#[derive(Debug, Clone)]
pub struct CompositeCell {
    pub dx: i32,
    pub dy: i32,
    /// Draw-order key of the layer.
    pub order: i32,
    /// Sprite resource name (`$EMPTY$` for nothing).
    pub sprite: String,
}

/// A layered animation (kind 8), e.g. a ship (wake, hull halves, sails) or a
/// building. Stored as `frames` rows of `layers` 44-byte cells:
///
/// ```text
/// +0   i32  dx
/// +4   i32  dy
/// +8   i32  draw order
/// +12  char sprite name[16]
/// +28  16 bytes unused
/// ```
///
/// The entry's width is the layer count, its height the frame count, and
/// parameter A packs the frame size as `width << 16 | height`.
#[derive(Debug, Clone)]
pub struct Composite {
    pub frame_width: u32,
    pub frame_height: u32,
    pub layers: usize,
    pub frames: usize,
    cells: Vec<CompositeCell>,
}

pub const EMPTY_SPRITE: &str = "$EMPTY$";
const CELL_SIZE: usize = 44;

impl Composite {
    pub fn parse(data: &[u8], entry: &Entry) -> Result<Self> {
        let layers = entry.width as usize;
        let frames = entry.height as usize;
        if layers == 0 || data.len() < layers * frames * CELL_SIZE {
            return Err(invalid(
                "composite",
                format!(
                    "{}: {} bytes for {layers}x{frames} cells",
                    entry.name,
                    data.len()
                ),
            ));
        }
        let mut cells = Vec::with_capacity(layers * frames);
        for i in 0..layers * frames {
            let mut r = Reader::at(data, i * CELL_SIZE, "composite cell");
            let dx = r.i32()?;
            let dy = r.i32()?;
            let order = r.i32()?;
            let sprite = latin1_cstr(r.bytes(16)?);
            cells.push(CompositeCell {
                dx,
                dy,
                order,
                sprite,
            });
        }
        Ok(Self {
            frame_width: entry.param_a >> 16,
            frame_height: entry.param_a & 0xffff,
            layers,
            frames,
            cells,
        })
    }

    /// The layers of `frame` (wrapping), in stored order.
    pub fn frame(&self, frame: usize) -> &[CompositeCell] {
        let f = frame % self.frames.max(1);
        &self.cells[f * self.layers..(f + 1) * self.layers]
    }
}
