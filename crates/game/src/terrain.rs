//! Composition of a mission's layered terrain into a single image.

use anyhow::Result;
use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::MapGrid;

use crate::assets::{GameData, Image, ImageCache, ANIMATED};

/// Tile drawn under every cell before the map layers, so that tiles with
/// transparent parts (shelf, shores) show open sea beneath.
const SEA_TILE: &str = "b_sea00_001";
/// Shallow water all over: under island cells, whose ground tiles leave the
/// water around the shore open, so that it joins the shelf around them.
const SHELF_TILE: &str = "b_shelf_017";

/// Scale factor of the minimap relative to the full-size map.
pub const MINIMAP_DIVISOR: u32 = 8;

pub struct Terrain {
    pub width: u32,
    pub height: u32,
    /// Renderer pixels (`0x00RRGGBB` or `ANIMATED | index`), row-major.
    pub pixels: Vec<u32>,
    pub missing_tiles: Vec<String>,
}

impl Terrain {
    pub fn compose(data: &GameData, grid: &MapGrid, cache: &mut ImageCache) -> Result<Self> {
        let mut t = Self::paint(data, grid, cache, (0, 0, grid.width - 1, grid.height - 1))?;
        t.missing_tiles.sort();
        t.missing_tiles.dedup();
        Ok(t)
    }

    /// The cells `x0..=x1`, `y0..=y1` painted from the grid.
    fn paint(
        data: &GameData,
        grid: &MapGrid,
        cache: &mut ImageCache,
        (x0, y0, x1, y1): (u32, u32, u32, u32),
    ) -> Result<Self> {
        let (width, height) = ((x1 - x0 + 1) * TILE_SIZE, (y1 - y0 + 1) * TILE_SIZE);
        let mut t = Terrain {
            width,
            height,
            pixels: vec![0; (width * height) as usize],
            missing_tiles: Vec::new(),
        };
        let at = |cx: u32, cy: u32| ((cx - x0) * TILE_SIZE, (cy - y0) * TILE_SIZE);
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                // Layer 0 is sea (tile set 0), shelf (1) or island ground.
                let island = grid.get(0, cx, cy).tileset > 1;
                if let Some(water) = cache.get(data, if island { SHELF_TILE } else { SEA_TILE })? {
                    let (px, py) = at(cx, cy);
                    t.blit(&water, px, py);
                }
            }
        }
        for layer in 0..grid.layers {
            for cy in y0..=y1 {
                for cx in x0..=x1 {
                    let cell = grid.get(layer, cx, cy);
                    if cell.is_empty() {
                        continue;
                    }
                    let Some(set) = data.tilesets.by_id(cell.tileset) else {
                        t.missing_tiles
                            .push(format!("tile set {:#04x}", cell.tileset));
                        continue;
                    };
                    let name = set.tile_name(cell.variant);
                    match cache.get(data, &name)? {
                        Some(tile) => {
                            let (px, py) = at(cx, cy);
                            t.blit(&tile, px, py)
                        }
                        None => t.missing_tiles.push(name),
                    }
                }
            }
        }
        Ok(t)
    }

    /// Paints cells `x0..=x1`, `y0..=y1` again (a dig changed them), with
    /// what the tiles of the cells above and left of them draw into them.
    pub fn repaint(
        &mut self,
        data: &GameData,
        grid: &MapGrid,
        cache: &mut ImageCache,
        (x0, y0, x1, y1): (u32, u32, u32, u32),
    ) -> Result<()> {
        const MARGIN: u32 = 2;
        let (bx, by) = (x0.saturating_sub(MARGIN), y0.saturating_sub(MARGIN));
        let part = Self::paint(data, grid, cache, (bx, by, x1, y1))?;
        let t = TILE_SIZE;
        for py in y0 * t..(y1 + 1) * t {
            let src = ((py - by * t) * part.width + (x0 - bx) * t) as usize;
            let dst = (py * self.width + x0 * t) as usize;
            let n = ((x1 - x0 + 1) * t) as usize;
            self.pixels[dst..dst + n].copy_from_slice(&part.pixels[src..src + n]);
        }
        Ok(())
    }

    /// The map at 8 pixels a cell from the tiles' small versions (`s_`
    /// for `b_`), as the original's mission screens show it.
    /// `screen` gives a palette index's colour on the screen it is shown on.
    pub fn small(
        data: &GameData,
        grid: &MapGrid,
        cache: &mut ImageCache,
        screen: &[u32; 256],
    ) -> Result<(u32, u32, Vec<u32>)> {
        const S: u32 = 8;
        let mut t = Terrain {
            width: grid.width * S,
            height: grid.height * S,
            pixels: vec![0; (grid.width * S * grid.height * S) as usize],
            missing_tiles: Vec::new(),
        };
        let small = |name: &str| name.strip_prefix("b_").map(|rest| format!("s_{rest}"));
        let sea = small(SEA_TILE).unwrap_or_default();
        for cy in 0..grid.height {
            for cx in 0..grid.width {
                if let Some(tile) = cache.get(data, &sea)? {
                    t.blit(&tile, cx * S, cy * S);
                }
            }
        }
        for layer in 0..grid.layers {
            for cy in 0..grid.height {
                for cx in 0..grid.width {
                    let cell = grid.get(layer, cx, cy);
                    if cell.is_empty() {
                        continue;
                    }
                    let Some(set) = data.tilesets.by_id(cell.tileset) else {
                        continue;
                    };
                    let Some(name) = small(&set.tile_name(cell.variant)) else {
                        continue;
                    };
                    if let Some(tile) = cache.get(data, &name)? {
                        t.blit(&tile, cx * S, cy * S);
                    }
                }
            }
        }
        // Back to palette indices (the game palette's first entry of each
        // colour), then the screen's colours.
        let mut index = std::collections::HashMap::new();
        for (i, c) in data.palette.0.iter().enumerate().rev() {
            index.insert(crate::assets::rgb(*c), i);
        }
        let pixels = t
            .pixels
            .iter()
            .map(|&p| {
                let i = if p & ANIMATED != 0 {
                    (p & 0xff) as usize
                } else {
                    index.get(&p).copied().unwrap_or(0)
                };
                screen[i]
            })
            .collect();
        Ok((t.width, t.height, pixels))
    }

    fn blit(&mut self, img: &Image, x0: u32, y0: u32) {
        for y in 0..img.height.min(self.height.saturating_sub(y0)) {
            let src = &img.pixels[(y * img.width) as usize..][..img.width as usize];
            let row = ((y0 + y) * self.width + x0) as usize;
            let w = img.width.min(self.width.saturating_sub(x0)) as usize;
            for (dst, px) in self.pixels[row..row + w].iter_mut().zip(src) {
                if let Some(px) = px {
                    *dst = *px;
                }
            }
        }
    }

    /// A box-filtered overview of the map, `MINIMAP_DIVISOR` times smaller.
    pub fn minimap(&self, lut: &[u32; 256]) -> (u32, u32, Vec<u32>) {
        let d = MINIMAP_DIVISOR;
        let (w, h) = (self.width / d, self.height / d);
        let mut out = Vec::with_capacity((w * h) as usize);
        for my in 0..h {
            for mx in 0..w {
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for y in 0..d {
                    for x in 0..d {
                        let px = resolve(
                            self.pixels[((my * d + y) * self.width + mx * d + x) as usize],
                            lut,
                        );
                        r += px >> 16 & 0xff;
                        g += px >> 8 & 0xff;
                        b += px & 0xff;
                    }
                }
                let n = d * d;
                out.push((r / n) << 16 | (g / n) << 8 | (b / n));
            }
        }
        (w, h, out)
    }
}

/// Resolves a renderer pixel against the current palette lookup table.
#[inline]
pub fn resolve(px: u32, lut: &[u32; 256]) -> u32 {
    if px & ANIMATED != 0 {
        lut[(px & 0xff) as usize]
    } else {
        px
    }
}
