//! The LZ77 variant used by the installer (`SETUP.DAT`) and by compressed
//! archive resources.
//!
//! The stream interleaves 16-bit little-endian flag words with token bytes.
//! Flag bits are consumed LSB first, and a new flag word is read as soon as
//! the previous one is exhausted (i.e. *before* the token bytes that follow).
//!
//! ```text
//! 1                      literal: next byte
//! 0 0 l1 l0  off8        short match: length = 2 + (l1 l0), distance = 256 - off8
//! 0 1        lo hi       long match:  distance = 0x2000 - (((hi & 0xf8) << 5) | lo)
//!                          length = (hi & 7) + 2, or if (hi & 7) == 0:
//!                          next byte n; n == 0 terminates the stream, else length = n + 1
//! ```
//!
//! The sliding window is 8 KiB, and the maximum match length is 256.

use crate::{invalid, Result};

/// Result of decompressing one stream.
pub struct Decompressed {
    pub data: Vec<u8>,
    /// Number of input bytes consumed, including the end marker.
    pub consumed: usize,
}

struct BitReader<'a> {
    src: &'a [u8],
    pos: usize,
    bits: u16,
    left: u8,
}

impl<'a> BitReader<'a> {
    fn byte(&mut self) -> Result<u8> {
        let b = *self
            .src
            .get(self.pos)
            .ok_or_else(|| invalid("LZ stream", "unexpected end of input"))?;
        self.pos += 1;
        Ok(b)
    }

    fn refill(&mut self) -> Result<()> {
        let lo = self.byte()? as u16;
        let hi = self.byte()? as u16;
        self.bits = lo | (hi << 8);
        self.left = 16;
        Ok(())
    }

    fn bit(&mut self) -> Result<bool> {
        let b = self.bits & 1 != 0;
        self.bits >>= 1;
        self.left -= 1;
        if self.left == 0 {
            self.refill()?;
        }
        Ok(b)
    }
}

/// Decompresses a single stream starting at the beginning of `src`.
///
/// `size_hint` is used to preallocate the output buffer.
pub fn decompress(src: &[u8], size_hint: usize) -> Result<Decompressed> {
    let mut r = BitReader {
        src,
        pos: 0,
        bits: 0,
        left: 0,
    };
    r.refill()?;
    let mut out = Vec::with_capacity(size_hint);
    loop {
        if r.bit()? {
            let b = r.byte()?;
            out.push(b);
            continue;
        }
        let (len, dist) = if !r.bit()? {
            let len = 2 + ((r.bit()? as usize) << 1 | r.bit()? as usize);
            let dist = 0x100 - r.byte()? as usize;
            (len, dist)
        } else {
            let lo = r.byte()? as usize;
            let hi = r.byte()? as usize;
            let dist = 0x2000 - (((hi & 0xf8) << 5) | lo);
            let len = if hi & 7 != 0 {
                (hi & 7) + 2
            } else {
                match r.byte()? {
                    0 => break,
                    n => n as usize + 1,
                }
            };
            (len, dist)
        };
        if dist > out.len() {
            return Err(invalid(
                "LZ stream",
                format!("match distance {dist} exceeds output length {}", out.len()),
            ));
        }
        let start = out.len() - dist;
        for i in 0..len {
            let b = out[start + i];
            out.push(b);
        }
    }
    Ok(Decompressed {
        data: out,
        consumed: r.pos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals_and_end_marker() {
        // Bits: 1, 1 (two literals), 0, 1 (long match) -> lo=0, hi=0xf0, n=0 (end).
        let stream = [0x0b, 0x00, b'h', b'i', 0x00, 0xf0, 0x00];
        let d = decompress(&stream, 0).unwrap();
        assert_eq!(d.data, b"hi");
        assert_eq!(d.consumed, stream.len());
    }

    #[test]
    fn truncated_input_is_an_error() {
        // Start of the compressed `0BKGND_LMAPA` resource from CONTROLG.DKD.
        let src = [
            0x43, 0x8f, 0x28, 0x00, 0xff, 0xc8, 0xfc, 0xfd, 0x01, 0x00, 0x08,
        ];
        assert!(decompress(&src, 0).is_err());
    }

    #[test]
    fn short_and_long_matches() {
        // literal 'a', short match dist 1 len 5, long match dist 1 len 10 (via
        // extended length byte 9), end marker.
        // Bits: 1 | 0 0 1 1 | 0 1 (long, hi&7=0, n=9) | 0 1 (end)
        //     = 1,0,0,1,1,0,1,0,1 -> 0b1_0101_1001 = 0x0159
        let stream = [0x59, 0x01, b'a', 0xff, 0xff, 0xf8, 0x09, 0x00, 0xf0, 0x00];
        let d = decompress(&stream, 0).unwrap();
        assert_eq!(d.data, vec![b'a'; 16]);
        assert_eq!(d.consumed, stream.len());
    }
}
