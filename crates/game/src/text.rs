//! Text drawing with the game's bitmap fonts.

use std::collections::HashMap;

use anyhow::{Context, Result};
use openconquest_formats::{Archive, Font, Glyph, ResourceKind};

use crate::assets::rgb;
use crate::render::Frame;

/// Horizontal gap between glyphs, in font pixels, unless set otherwise
/// (the original passes it with each text, mfcfnt.cpp 0x5470f0).
const SPACING: u32 = 1;

#[derive(Clone)]
pub struct TextRenderer {
    glyphs: HashMap<u8, Glyph>,
    /// How wide a character the font lacks is (drawn blank).
    missing_width: u32,
    line_height: u32,
    atlas_width: u32,
    /// Atlas pixels as `0x00RRGGBB`, `None` where transparent.
    atlas: Vec<Option<u32>>,
    spacing: u32,
}

impl TextRenderer {
    /// Loads font `name` from `archive`, using colour variant `atlas`.
    pub fn load(archive: &Archive, name: &str, atlas: usize) -> Result<Self> {
        Self::load_in(archive, name, atlas, None)
    }

    /// Loads font `name` drawn in `palette` (the screen's, as the
    /// original's 256-colour display shows its atlas indices) rather than
    /// the atlas's own.
    pub fn load_in(
        archive: &Archive,
        name: &str,
        atlas: usize,
        palette: Option<&openconquest_formats::Palette>,
    ) -> Result<Self> {
        let font = Font::parse(&archive.read(name, ResourceKind::Font)?)?;
        let bitmap = font
            .atlas(archive, atlas)
            .with_context(|| format!("loading {name} atlas {atlas}"))?;
        let palette = palette.unwrap_or(&bitmap.palette);
        let pixels = bitmap
            .pixels
            .iter()
            .map(|&i| (i != font.transparent).then(|| rgb(palette.0[i as usize])))
            .collect();
        Ok(Self {
            glyphs: font.glyphs,
            missing_width: font.missing_width as u32,
            line_height: font.line_height as u32,
            atlas_width: bitmap.width,
            atlas: pixels,
            spacing: SPACING,
        })
    }

    /// The same font with `spacing` pixels between glyphs.
    pub fn with_spacing(mut self, spacing: u32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn line_height(&self) -> u32 {
        self.line_height
    }

    /// Width of `text` in font pixels: the glyphs and the gaps between
    /// them (as mfcfnt.cpp's 0x5472b0 measures it).
    pub fn measure(&self, text: &str) -> u32 {
        let glyphs: u32 = text
            .bytes()
            .map(|c| {
                self.glyphs
                    .get(&c)
                    .map_or(self.missing_width, |g| g.width as u32)
            })
            .sum();
        glyphs + self.spacing * (text.len() as u32).saturating_sub(1)
    }

    /// Height of `text` in font pixels: its tallest glyph (mfcfnt.cpp's
    /// 0x547350, which centres text by it).
    pub fn height(&self, text: &str) -> u32 {
        text.bytes()
            .map(|c| self.glyphs.get(&c).map_or(0, |g| g.height as u32))
            .max()
            .unwrap_or(0)
    }

    /// Draws `text` with its top-left corner at `(x, y)`, magnified by `scale`.
    pub fn draw(&self, frame: &mut Frame, x: i32, y: i32, scale: u32, text: &str) {
        let s = scale.max(1) as i32;
        let mut pen = x;
        for c in text.bytes() {
            let Some(g) = self.glyphs.get(&c) else {
                pen += (self.missing_width + self.spacing) as i32 * s;
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
            pen += (g.width as u32 + self.spacing) as i32 * s;
        }
    }
}
