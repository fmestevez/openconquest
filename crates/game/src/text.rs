//! Text drawing with the game's bitmap fonts.

use std::collections::HashMap;

use anyhow::{Context, Result};
use openac_formats::{Archive, Font, Glyph, ResourceKind};

use crate::assets::rgb;
use crate::render::Frame;

/// Horizontal gap between glyphs, in font pixels.
const SPACING: u32 = 1;

pub struct TextRenderer {
    glyphs: HashMap<u8, Glyph>,
    line_height: u32,
    atlas_width: u32,
    /// Atlas pixels as `0x00RRGGBB`, `None` where transparent.
    atlas: Vec<Option<u32>>,
}

impl TextRenderer {
    /// Loads font `name` from `archive`, using colour variant `atlas`.
    pub fn load(archive: &Archive, name: &str, atlas: usize) -> Result<Self> {
        let font = Font::parse(&archive.read(name, ResourceKind::Font)?)?;
        let bitmap = font
            .atlas(archive, atlas)
            .with_context(|| format!("loading {name} atlas {atlas}"))?;
        let pixels = bitmap
            .pixels
            .iter()
            .map(|&i| (i != font.transparent).then(|| rgb(bitmap.palette.0[i as usize])))
            .collect();
        Ok(Self {
            glyphs: font.glyphs,
            line_height: font.line_height as u32,
            atlas_width: bitmap.width,
            atlas: pixels,
        })
    }

    pub fn line_height(&self) -> u32 {
        self.line_height
    }

    /// Width of `text` in font pixels.
    pub fn measure(&self, text: &str) -> u32 {
        text.bytes()
            .map(|c| {
                self.glyphs
                    .get(&c)
                    .map_or(self.line_height / 3, |g| g.width as u32)
                    + SPACING
            })
            .sum()
    }

    /// Draws `text` with its top-left corner at `(x, y)`, magnified by `scale`.
    pub fn draw(&self, frame: &mut Frame, x: i32, y: i32, scale: u32, text: &str) {
        let s = scale.max(1) as i32;
        let mut pen = x;
        for c in text.bytes() {
            let Some(g) = self.glyphs.get(&c) else {
                pen += (self.line_height / 3 + SPACING) as i32 * s;
                continue;
            };
            for gy in 0..g.height as i32 {
                let row = (g.y as i32 + gy) as usize * self.atlas_width as usize;
                for gx in 0..g.width as i32 {
                    let Some(px) = self.atlas[row + (g.x as i32 + gx) as usize] else {
                        continue;
                    };
                    for dy in 0..s {
                        for dx in 0..s {
                            let (fx, fy) = (pen + gx * s + dx, y + gy * s + dy);
                            if fx >= 0
                                && fy >= 0
                                && (fx as u32) < frame.width
                                && (fy as u32) < frame.height
                            {
                                frame.pixels[(fy as u32 * frame.width + fx as u32) as usize] = px;
                            }
                        }
                    }
                }
            }
            pen += (g.width as u32 + SPACING) as i32 * s;
        }
    }
}
