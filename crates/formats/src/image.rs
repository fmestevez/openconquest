//! Image resources: DIB bitmaps, RLE sprites, 1-bit masks and palettes.

use crate::bytes::Reader;
use crate::{invalid, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// A 256-color palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette(pub [Rgb; 256]);

impl Default for Palette {
    fn default() -> Self {
        Self([Rgb::default(); 256])
    }
}

impl Palette {
    /// Parses `count` `RGBQUAD` (B, G, R, reserved) entries.
    fn from_rgbquads(data: &[u8], count: usize) -> Result<Self> {
        let mut pal = Self::default();
        let mut r = Reader::new(data, "palette");
        for slot in pal.0.iter_mut().take(count) {
            let q = r.bytes(4)?;
            *slot = Rgb {
                r: q[2],
                g: q[1],
                b: q[0],
            };
        }
        Ok(pal)
    }

    /// Extracts the palette of a DIB bitmap resource (e.g. `PALETTE` in MAPLIB).
    pub fn from_bitmap(data: &[u8]) -> Result<Self> {
        Ok(Bitmap::parse(data)?.palette)
    }
}

/// One palette-cycling slot: palette entry `index` steps through `colors`,
/// advancing every `period_ms` milliseconds.
#[derive(Debug, Clone)]
pub struct PaletteCycle {
    pub index: u8,
    pub period_ms: u16,
    pub colors: Vec<Rgb>,
}

impl PaletteCycle {
    pub fn color_at(&self, time_ms: u64) -> Rgb {
        let step = time_ms / self.period_ms.max(1) as u64;
        self.colors[(step % self.colors.len() as u64) as usize]
    }
}

/// Parses a palette animation table (resource kind 13, e.g. MAPLIB
/// `ANIM_PALETTE`): `count` variable-size records of
///
/// ```text
/// +0   u32  unused (stale pointer)
/// +4   u8   palette index
/// +5   u16  unknown (1)
/// +7   u16  period in milliseconds
/// +9   8    unused
/// +17  u16  number of colors n
/// +19  n * (u8 r, u8 g, u8 b)
/// +19+3n    3 bytes padding
/// ```
pub fn parse_palette_animation(data: &[u8], count: usize) -> Result<Vec<PaletteCycle>> {
    let mut r = Reader::new(data, "palette animation");
    let mut cycles = Vec::with_capacity(count);
    for _ in 0..count {
        let _ptr = r.u32()?;
        let index = r.u8()?;
        let _unknown = r.u16()?;
        let period_ms = r.u16()?;
        r.bytes(8)?;
        let n = r.u16()? as usize;
        if n == 0 {
            return Err(invalid("palette animation", "cycle without colors"));
        }
        let colors = r
            .bytes(n * 3)?
            .chunks_exact(3)
            .map(|c| Rgb {
                r: c[0],
                g: c[1],
                b: c[2],
            })
            .collect();
        r.bytes(3)?;
        cycles.push(PaletteCycle {
            index,
            period_ms,
            colors,
        });
    }
    Ok(cycles)
}

/// An 8-bit palettized bitmap (stored on disk as a Windows DIB).
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub palette: Palette,
    /// Palette indices, top-down rows, `width * height` bytes.
    pub pixels: Vec<u8>,
}

impl Bitmap {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data, "bitmap header");
        let header_size = r.u32()? as usize;
        let width = r.i32()?;
        let height = r.i32()?;
        let _planes = r.u16()?;
        let bpp = r.u16()?;
        let compression = r.u32()?;
        let _image_size = r.u32()?;
        let _ppm_x = r.u32()?;
        let _ppm_y = r.u32()?;
        let colors_used = r.u32()? as usize;
        if bpp != 8 || compression != 0 {
            return Err(invalid(
                "bitmap",
                format!("unsupported format: {bpp} bpp, compression {compression}"),
            ));
        }
        if width <= 0 || height == 0 {
            return Err(invalid(
                "bitmap",
                format!("bad dimensions {width}x{height}"),
            ));
        }
        let table_len = if colors_used == 0 { 256 } else { colors_used };
        let pal_data = data
            .get(header_size..)
            .ok_or_else(|| invalid("bitmap", "truncated palette"))?;
        let palette = Palette::from_rgbquads(pal_data, table_len.min(256))?;

        let w = width as usize;
        let h = height.unsigned_abs() as usize;
        let stride = (w + 3) & !3;
        let start = header_size + table_len * 4;
        let rows = data
            .get(start..start + stride * h)
            .ok_or_else(|| invalid("bitmap", "truncated pixel data"))?;
        let mut pixels = Vec::with_capacity(w * h);
        for y in 0..h {
            // Positive height means bottom-up storage.
            let src_y = if height > 0 { h - 1 - y } else { y };
            pixels.extend_from_slice(&rows[src_y * stride..src_y * stride + w]);
        }
        Ok(Self {
            width: w as u32,
            height: h as u32,
            palette,
            pixels,
        })
    }
}

/// A 1-bit transparency mask (resource kind 18), MSB first, rows padded to
/// whole bytes.
#[derive(Debug, Clone)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    bits: Vec<u8>,
}

impl Mask {
    pub fn parse(data: &[u8], width: u32, height: u32) -> Result<Self> {
        let stride = width.div_ceil(8) as usize;
        if data.len() < stride * height as usize {
            return Err(invalid("mask", "truncated"));
        }
        Ok(Self {
            width,
            height,
            bits: data.to_vec(),
        })
    }

    pub fn get(&self, x: u32, y: u32) -> bool {
        let stride = self.width.div_ceil(8) as usize;
        let byte = self.bits[y as usize * stride + x as usize / 8];
        byte >> (7 - (x % 8)) & 1 != 0
    }

    /// Bit `n` of the mask's bytes, the first bit of each byte the
    /// highest; false past its end.
    pub fn bit(&self, n: usize) -> bool {
        self.bits
            .get(n / 8)
            .is_some_and(|&byte| byte >> (7 - (n % 8)) & 1 != 0)
    }
}

/// A palettized sprite with transparency (resource kind 6).
///
/// Only the bounding box of opaque pixels is stored; `x`/`y` place it within
/// the full `width` x `height` frame.
#[derive(Debug, Clone)]
pub struct Sprite {
    pub width: u32,
    pub height: u32,
    pub flags: u16,
    pub x: u32,
    pub y: u32,
    pub box_width: u32,
    pub box_height: u32,
    /// Palette indices of the bounding box, row-major.
    pub pixels: Vec<u8>,
    /// Opacity of each bounding box pixel (0 = transparent, 1 = opaque).
    pub opaque: Vec<u8>,
}

impl Sprite {
    /// Decodes a sprite.
    ///
    /// Header (22 bytes): `u32 size, u32 width, u32 height, u16 flags,
    /// u16 x, u16 y, u16 box_width, u16 box_height`, followed by
    /// `flags >> 10` u16 seek offsets and the RLE rows. Each row is a
    /// sequence of opcodes until `box_width` pixels are produced:
    ///
    /// * `0x00..=0x7f n`: skip `n` transparent pixels
    /// * `0x80 | n`: `n` (0..63) literal pixels follow
    /// * `0xc0 | n`: repeat the next byte `n` times
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data, "sprite header");
        let _size = r.u32()?;
        let width = r.u32()?;
        let height = r.u32()?;
        let flags = r.u16()?;
        let x = r.u16()? as u32;
        let y = r.u16()? as u32;
        let box_width = r.u16()? as u32;
        let box_height = r.u16()? as u32;
        let seek_entries = (flags >> 10) as usize;
        r.bytes(seek_entries * 2)?;

        let (bw, bh) = (box_width as usize, box_height as usize);
        let mut pixels = vec![0u8; bw * bh];
        let mut opaque = vec![0u8; bw * bh];
        let mut r = Reader::at(data, r.pos(), "sprite data");
        for row in 0..bh {
            let base = row * bw;
            let mut col = 0usize;
            while col < bw {
                let op = r.u8()?;
                let n = (op & 0x3f) as usize;
                if op & 0x80 == 0 {
                    col += op as usize;
                    continue;
                }
                if col + n > bw {
                    return Err(invalid(
                        "sprite",
                        format!("row {row} overflows box width {bw}"),
                    ));
                }
                let dst = base + col..base + col + n;
                if op & 0x40 != 0 {
                    pixels[dst.clone()].fill(r.u8()?);
                } else {
                    pixels[dst.clone()].copy_from_slice(r.bytes(n)?);
                }
                opaque[dst].fill(1);
                col += n;
            }
            if col != bw {
                return Err(invalid(
                    "sprite",
                    format!("row {row} overflows box width {bw}"),
                ));
            }
        }
        if x + box_width > width || y + box_height > height {
            return Err(invalid("sprite", "bounding box exceeds frame"));
        }
        Ok(Self {
            width,
            height,
            flags,
            x,
            y,
            box_width,
            box_height,
            pixels,
            opaque,
        })
    }

    /// Returns the palette index at frame coordinates, or `None` if transparent.
    pub fn pixel(&self, fx: u32, fy: u32) -> Option<u8> {
        if fx < self.x
            || fy < self.y
            || fx >= self.x + self.box_width
            || fy >= self.y + self.box_height
        {
            return None;
        }
        let i = ((fy - self.y) * self.box_width + (fx - self.x)) as usize;
        (self.opaque[i] != 0).then(|| self.pixels[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite_bytes(header: [u16; 5], body: &[u8], w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&((22 + body.len()) as u32).to_le_bytes());
        v.extend_from_slice(&w.to_le_bytes());
        v.extend_from_slice(&h.to_le_bytes());
        for f in header {
            v.extend_from_slice(&f.to_le_bytes());
        }
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn sprite_rle() {
        // From MAPLIB s_AMBER016: 4x6 box at (1,1), mixing skips and literals.
        let body = [
            0x81, 0xf7, 0x03, 0x01, 0x81, 0xf7, 0x02, 0x04, 0x04, 0x02, 0x81, 0xf8, 0x01, 0x03,
            0x81, 0xf7,
        ];
        let s = Sprite::parse(&sprite_bytes([0x80, 1, 1, 4, 6], &body, 8, 8)).unwrap();
        assert_eq!(s.pixel(1, 1), Some(0xf7));
        assert_eq!(s.pixel(2, 1), None);
        assert_eq!(s.pixel(2, 2), Some(0xf7));
        assert_eq!(s.pixel(3, 5), Some(0xf8));
        assert_eq!(s.pixel(4, 6), Some(0xf7));
        assert_eq!(s.opaque.iter().filter(|&&o| o != 0).count(), 4);
    }

    #[test]
    fn sprite_runs_and_seek_table() {
        // flags 0x480 => one u16 seek entry precedes the data.
        let body = [0x00, 0x00, 0xc3, 0x07, 0x81, 0x09];
        let s = Sprite::parse(&sprite_bytes([0x480, 0, 0, 4, 1], &body, 4, 1)).unwrap();
        assert_eq!(s.pixels, [7, 7, 7, 9]);
        assert!(s.opaque.iter().all(|&o| o == 1));
    }

    #[test]
    fn mask_bits() {
        let m = Mask::parse(&[0b1000_0001], 8, 1).unwrap();
        assert!(m.get(0, 0));
        assert!(!m.get(1, 0));
        assert!(m.get(7, 0));
    }
}
