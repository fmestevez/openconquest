//! Sound effects (`sound/sounds.dkx`).
//!
//! Each sound (kind `Sound`) is a `WAVEFORMATEX` (18 bytes; the game's are
//! PCM, mono, 16-bit, 22050 Hz), the length of the sample data (u32), four
//! more bytes, then the samples. Text entries `SND_n` list the sounds that
//! may play for game event `n`: after a 24-byte header (u32s 0, flags,
//! count, 10, 10, 0) come `count` NUL-terminated names, the first preceded
//! by a byte with one high bit set per name (0x80, 0xc0, 0xe0, ...).

use crate::archive::{Archive, ResourceKind};
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Sound {
    pub channels: u16,
    pub sample_rate: u32,
    /// Interleaved 16-bit samples (8-bit sounds are converted).
    pub samples: Vec<i16>,
}

impl Sound {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let u32_at = |o: usize| {
            data.get(o..o + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        let bad = |why: &str| Error::Invalid {
            what: "sound",
            why: why.to_string(),
        };
        let (Some(format), Some(channels), Some(sample_rate), Some(bits), Some(len)) =
            (u16_at(0), u16_at(2), u32_at(4), u16_at(14), u32_at(18))
        else {
            return Err(bad("truncated header"));
        };
        if format != 1 {
            return Err(bad("not PCM"));
        }
        let body = data
            .get(26..26 + len as usize)
            .or_else(|| data.get(26..))
            .unwrap_or_default();
        let samples = match bits {
            16 => body
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect(),
            8 => body.iter().map(|&b| ((b as i16) - 128) << 8).collect(),
            _ => return Err(bad("unsupported sample size")),
        };
        Ok(Self {
            channels: channels.max(1),
            sample_rate,
            samples,
        })
    }
}

/// The sounds that may play for game event `n` (text `SND_n`).
pub fn sound_choices(archive: &Archive, event: u32) -> Result<Vec<String>> {
    let Some(e) = archive.find(&format!("SND_{event}"), ResourceKind::Text) else {
        return Ok(Vec::new());
    };
    let data = archive.read_entry(e)?;
    let body = data.get(24..).unwrap_or_default();
    let body = match body.first() {
        Some(&b) if b >= 0x80 => &body[1..],
        _ => body,
    };
    Ok(body
        .split(|&b| b == 0)
        .filter(|n| !n.is_empty())
        .map(|n| n.iter().map(|&b| b as char).collect())
        .collect())
}

/// Loads the sound named `name`.
pub fn load_sound(archive: &Archive, name: &str) -> Result<Option<Sound>> {
    match archive.find(name, ResourceKind::Sound) {
        Some(e) => Ok(Some(Sound::parse(&archive.read_entry(e)?)?)),
        None => Ok(None),
    }
}
