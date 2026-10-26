//! The game's videos (`video/*.avi`): RIFF AVI files with one video stream,
//! Cinepak (`cvid`) or Microsoft Video 1 (`CRAM`, 16-bit), and PCM sound.
//! Frames are decoded to 0RGB pixels, top row first.

use crate::{invalid, Result};

/// An AVI file's streams, ready to decode.
pub struct Avi {
    pub width: u32,
    pub height: u32,
    /// Microseconds a frame is shown.
    pub frame_us: u32,
    codec: Codec,
    data: Vec<u8>,
    /// Byte ranges of the video chunks, in order.
    frames: Vec<(usize, usize)>,
    /// The sound, if any.
    pub audio: Option<Audio>,
}

/// PCM sound.
pub struct Audio {
    pub channels: u16,
    pub rate: u32,
    /// Interleaved 16-bit samples (8-bit sound is widened).
    pub samples: Vec<i16>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Codec {
    Cinepak,
    Video1,
}

fn u16_at(d: &[u8], o: usize) -> u16 {
    d.get(o..o + 2)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

impl Avi {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        if data.get(0..4) != Some(b"RIFF") || data.get(8..12) != Some(b"AVI ") {
            return Err(invalid("AVI", "not a RIFF AVI file"));
        }
        let mut avi = Avi {
            width: 0,
            height: 0,
            frame_us: 66_667,
            codec: Codec::Cinepak,
            data: Vec::new(),
            frames: Vec::new(),
            audio: None,
        };
        let mut audio_format = None;
        let mut audio_bytes = Vec::new();
        // Which stream is which: the order of the `strl` lists.
        let mut kinds: Vec<[u8; 4]> = Vec::new();
        let mut stack = vec![(12usize, data.len())];
        while let Some((mut pos, end)) = stack.pop() {
            while pos + 8 <= end {
                let id: [u8; 4] = data[pos..pos + 4].try_into().unwrap();
                let size = u32_at(&data, pos + 4) as usize;
                let body = pos + 8;
                let stop = (body + size).min(end);
                match &id {
                    b"LIST" => {
                        // Lists are walked depth first, in order.
                        stack.push((stop + (size & 1), end));
                        stack.push((body + 4, stop));
                        break;
                    }
                    b"avih" => avi.frame_us = u32_at(&data, body),
                    b"strh" => kinds.push(data[body..body + 4].try_into().unwrap()),
                    b"strf" => match kinds.last() {
                        Some(b"vids") => {
                            avi.width = u32_at(&data, body + 4);
                            avi.height = u32_at(&data, body + 8);
                            avi.codec = match &data[body + 16..body + 20] {
                                b"cvid" => Codec::Cinepak,
                                b"CRAM" | b"MSVC" | b"msvc" | b"cram" => Codec::Video1,
                                other => {
                                    return Err(invalid(
                                        "AVI",
                                        format!(
                                            "unsupported codec {:?}",
                                            String::from_utf8_lossy(other)
                                        ),
                                    ))
                                }
                            };
                        }
                        Some(b"auds") => {
                            audio_format = Some((
                                u16_at(&data, body),
                                u16_at(&data, body + 2),
                                u32_at(&data, body + 4),
                                u16_at(&data, body + 14),
                            ))
                        }
                        _ => {}
                    },
                    _ => {
                        let stream = (id[0] as char)
                            .to_digit(10)
                            .zip((id[1] as char).to_digit(10));
                        if let Some((a, b)) = stream {
                            let kind = kinds.get((a * 10 + b) as usize);
                            match (kind, &id[2..4]) {
                                (Some(b"vids"), b"dc" | b"db") => avi.frames.push((body, stop)),
                                (Some(b"auds"), b"wb") => {
                                    audio_bytes.extend_from_slice(&data[body..stop])
                                }
                                _ => {}
                            }
                        }
                    }
                }
                pos = stop + (size & 1);
            }
        }
        if avi.width == 0 || avi.height == 0 {
            return Err(invalid("AVI", "no video stream"));
        }
        if let Some((1, channels, rate, bits)) = audio_format {
            let samples = if bits == 8 {
                audio_bytes
                    .iter()
                    .map(|&b| ((b as i16) - 128) << 8)
                    .collect()
            } else {
                audio_bytes
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect()
            };
            avi.audio = Some(Audio {
                channels: channels.max(1),
                rate,
                samples,
            });
        }
        avi.data = data;
        Ok(avi)
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// A decoder for the video, which must be fed every frame in order.
    pub fn decoder(&self) -> Decoder {
        let pixels = vec![0u32; (self.width * self.height) as usize];
        Decoder {
            codec: self.codec,
            width: self.width as usize,
            height: self.height as usize,
            pixels,
            strips: Vec::new(),
        }
    }

    /// The compressed bytes of frame `n`.
    pub fn frame(&self, n: usize) -> &[u8] {
        self.frames
            .get(n)
            .map_or(&[][..], |&(a, b)| &self.data[a..b])
    }
}

/// Decodes a video's frames in turn into `pixels` (0RGB, top row first).
pub struct Decoder {
    codec: Codec,
    width: usize,
    height: usize,
    pub pixels: Vec<u32>,
    strips: Vec<Codebooks>,
}

#[derive(Clone)]
struct Codebooks {
    v1: [[u32; 4]; 256],
    v4: [[u32; 4]; 256],
}

impl Default for Codebooks {
    fn default() -> Self {
        Self {
            v1: [[0; 4]; 256],
            v4: [[0; 4]; 256],
        }
    }
}

fn be16(d: &[u8], o: usize) -> usize {
    d.get(o..o + 2)
        .map_or(0, |b| u16::from_be_bytes([b[0], b[1]]) as usize)
}

fn be24(d: &[u8], o: usize) -> usize {
    d.get(o..o + 3).map_or(0, |b| {
        ((b[0] as usize) << 16) | ((b[1] as usize) << 8) | b[2] as usize
    })
}

fn be32(d: &[u8], o: usize) -> u32 {
    d.get(o..o + 4)
        .map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Cinepak's colour: four luma values and shared chroma to 0RGB.
fn cinepak_rgb(y: u8, u: i8, v: i8) -> u32 {
    let (y, u, v) = (y as i32, u as i32, v as i32);
    let c = |x: i32| x.clamp(0, 255) as u32;
    let (r, g, b) = (c(y + 2 * v), c(y - u / 2 - v), c(y + 2 * u));
    (r << 16) | (g << 8) | b
}

impl Decoder {
    /// Decodes the next frame's bytes over the previous picture.
    pub fn decode(&mut self, data: &[u8]) {
        if data.is_empty() {
            return; // a repeated frame
        }
        match self.codec {
            Codec::Cinepak => self.cinepak(data),
            Codec::Video1 => self.video1(data),
        }
    }

    fn cinepak(&mut self, data: &[u8]) {
        if data.len() < 10 {
            return;
        }
        let flags = data[0];
        let strips = be16(data, 8);
        let mut pos = 10;
        let mut y0 = 0;
        for i in 0..strips {
            if pos + 12 > data.len() {
                break;
            }
            let size = be24(data, pos + 1).max(12);
            let y1 = y0 + be16(data, pos + 8);
            if self.strips.len() <= i {
                let books = self.strips.last().cloned().unwrap_or_default();
                self.strips.push(books);
            }
            // Strips after the first start from the one before's codebooks
            // unless the frame says to keep their own.
            if i > 0 && flags & 1 == 0 {
                let prev = self.strips[i - 1].clone();
                self.strips[i] = prev;
            }
            let end = (pos + size).min(data.len());
            self.cinepak_strip(i, &data[pos + 12..end], y0, y1);
            pos += size;
            y0 = y1;
        }
    }

    fn cinepak_strip(&mut self, strip: usize, data: &[u8], y0: usize, y1: usize) {
        let mut pos = 0;
        while pos + 4 <= data.len() {
            let id = data[pos];
            let size = be24(data, pos + 1).max(4);
            let body = &data[pos + 4..(pos + size).min(data.len())];
            match id {
                0x20 | 0x21 | 0x24 | 0x25 => Self::codebook(&mut self.strips[strip].v4, id, body),
                0x22 | 0x23 | 0x26 | 0x27 => Self::codebook(&mut self.strips[strip].v1, id, body),
                0x30..=0x32 => {
                    self.vectors(strip, id, body, y0, y1);
                    return;
                }
                _ => {}
            }
            pos += size;
        }
    }

    fn codebook(book: &mut [[u32; 4]; 256], id: u8, d: &[u8]) {
        let n = if id & 0x04 != 0 { 4 } else { 6 };
        let selective = id & 0x01 != 0;
        let (mut pos, mut flag, mut mask) = (0, 0u32, 0u32);
        for entry in book.iter_mut() {
            if selective {
                mask >>= 1;
                if mask == 0 {
                    if pos + 4 > d.len() {
                        return;
                    }
                    flag = be32(d, pos);
                    pos += 4;
                    mask = 0x8000_0000;
                }
            }
            if !selective || flag & mask != 0 {
                if pos + n > d.len() {
                    return;
                }
                let (u, v) = if n == 6 {
                    (d[pos + 4] as i8, d[pos + 5] as i8)
                } else {
                    (0, 0)
                };
                for k in 0..4 {
                    entry[k] = cinepak_rgb(d[pos + k], u, v);
                }
                pos += n;
            }
        }
    }

    fn vectors(&mut self, strip: usize, id: u8, d: &[u8], y0: usize, y1: usize) {
        let books = &self.strips[strip];
        let (w, h) = (self.width, self.height);
        let (mut pos, mut flag, mut mask) = (0usize, 0u32, 0u32);
        let next_bit = |pos: &mut usize, flag: &mut u32, mask: &mut u32| -> Option<bool> {
            *mask >>= 1;
            if *mask == 0 {
                if *pos + 4 > d.len() {
                    return None;
                }
                *flag = be32(d, *pos);
                *pos += 4;
                *mask = 0x8000_0000;
            }
            Some(*flag & *mask != 0)
        };
        let pixels = &mut self.pixels;
        let mut put = |x: usize, y: usize, c: u32| {
            if x < w && y < h {
                pixels[y * w + x] = c;
            }
        };
        for by in (y0..y1).step_by(4) {
            for bx in (0..w).step_by(4) {
                if id & 0x01 != 0 {
                    match next_bit(&mut pos, &mut flag, &mut mask) {
                        Some(true) => {}
                        Some(false) => continue,
                        None => return,
                    }
                }
                let v4 = if id & 0x02 != 0 {
                    false
                } else {
                    match next_bit(&mut pos, &mut flag, &mut mask) {
                        Some(b) => b,
                        None => return,
                    }
                };
                if v4 {
                    if pos + 4 > d.len() {
                        return;
                    }
                    for (q, &code) in d[pos..pos + 4].iter().enumerate() {
                        let e = books.v4[code as usize];
                        let (qx, qy) = (bx + (q & 1) * 2, by + (q >> 1) * 2);
                        put(qx, qy, e[0]);
                        put(qx + 1, qy, e[1]);
                        put(qx, qy + 1, e[2]);
                        put(qx + 1, qy + 1, e[3]);
                    }
                    pos += 4;
                } else {
                    let Some(&code) = d.get(pos) else { return };
                    pos += 1;
                    let e = books.v1[code as usize];
                    for (q, &c) in e.iter().enumerate() {
                        let (qx, qy) = (bx + (q & 1) * 2, by + (q >> 1) * 2);
                        put(qx, qy, c);
                        put(qx + 1, qy, c);
                        put(qx, qy + 1, c);
                        put(qx + 1, qy + 1, c);
                    }
                }
            }
        }
    }

    /// Microsoft Video 1, 16-bit: 4x4 blocks, bottom row of blocks first,
    /// each a skip, a fill, or two or eight colours chosen by a bit mask.
    fn video1(&mut self, d: &[u8]) {
        let (w, h) = (self.width, self.height);
        let (bw, bh) = (w / 4, h / 4);
        let rgb = |c: u16| {
            let c = c as u32;
            let (r, g, b) = ((c >> 10) & 31, (c >> 5) & 31, c & 31);
            ((r << 3 | r >> 2) << 16) | ((g << 3 | g >> 2) << 8) | (b << 3 | b >> 2)
        };
        let mut pos = 0;
        let mut skip = 0usize;
        let next = |pos: &mut usize| -> Option<u16> {
            let v = d
                .get(*pos..*pos + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]));
            *pos += 2;
            v
        };
        for row in (0..bh).rev() {
            for col in 0..bw {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                let Some(code) = next(&mut pos) else { return };
                let (a, b) = ((code & 0xff) as usize, (code >> 8) as usize);
                if a == 0 && b == 0 && pos >= d.len() {
                    return;
                }
                // Pixel (x, y) of the block, y counted from its bottom row.
                let mut set = |x: usize, y: usize, c: u32| {
                    let py = row * 4 + 3 - y;
                    self.pixels[py * w + col * 4 + x] = c;
                };
                if b & 0xfc == 0x84 {
                    skip = ((b - 0x84) << 8) + a - 1;
                } else if b < 0x80 {
                    let mut flags = code;
                    let (Some(ca), Some(cb)) = (next(&mut pos), next(&mut pos)) else {
                        return;
                    };
                    if ca & 0x8000 != 0 {
                        let mut c = [ca, cb, 0, 0, 0, 0, 0, 0];
                        for k in c.iter_mut().skip(2) {
                            *k = next(&mut pos).unwrap_or(0);
                        }
                        for y in 0..4 {
                            for x in 0..4 {
                                let k = ((y & 2) << 1) + (x & 2) + ((flags & 1) ^ 1) as usize;
                                set(x, y, rgb(c[k]));
                                flags >>= 1;
                            }
                        }
                    } else {
                        for y in 0..4 {
                            for x in 0..4 {
                                set(x, y, rgb(if flags & 1 != 0 { ca } else { cb }));
                                flags >>= 1;
                            }
                        }
                    }
                } else {
                    let c = rgb(code);
                    for y in 0..4 {
                        for x in 0..4 {
                            set(x, y, c);
                        }
                    }
                }
            }
        }
    }
}
