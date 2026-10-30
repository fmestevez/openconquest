//! Playing the game's videos (`video/*.avi`): the company logos and title
//! sequence at start-up, and the story's opening and ending around the
//! campaign. Frames are decoded as they fall due and drawn letterboxed;
//! the sound plays alongside.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use openconquest_formats::avi::{Avi, Decoder};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStreamHandle, Sink};

use crate::render::Frame;

/// The logos and title sequence shown at start-up, in order.
pub const INTRO: [&str; 4] = ["logo1.avi", "logo2.avi", "logo3.avi", "title.avi"];
/// The story's opening, before the first campaign mission.
pub const OPENING: &str = "mis000.avi";

/// The video before campaign mission `n` (command 0x128 case 5,
/// `MIS%03d.AVI` with the mission + campaign x 100).
pub fn mission(n: u32) -> String {
    format!("MIS{n:03}.AVI")
}

/// The campaign's end (command 0x128 case 6, `OUTRO%d.AVI` with the
/// campaign).
pub fn outro(campaign: u32) -> String {
    format!("OUTRO{campaign}.AVI")
}

/// The video `name` in the data's video folder, if installed.
pub fn find(root: &Path, name: &str) -> Option<PathBuf> {
    crate::assets::find_ci(&root.join("video"), name)
        .ok()
        .flatten()
}

pub struct Player {
    avi: Avi,
    decoder: Decoder,
    /// Frames decoded so far.
    shown: usize,
    /// Seconds since it began.
    clock: f32,
    sink: Option<Sink>,
}

impl Player {
    /// Opens the video at `path`, its sound playing at `volume` on `audio`.
    pub fn open(path: &Path, audio: Option<&OutputStreamHandle>, volume: f32) -> Result<Self> {
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let avi = Avi::parse(bytes).with_context(|| format!("reading {}", path.display()))?;
        let sink = match (audio, &avi.audio) {
            (Some(handle), Some(sound)) => {
                let sink = Sink::try_new(handle)?;
                sink.set_volume(volume);
                sink.append(SamplesBuffer::new(
                    sound.channels,
                    sound.rate,
                    sound.samples.clone(),
                ));
                Some(sink)
            }
            _ => None,
        };
        let decoder = avi.decoder();
        Ok(Self {
            avi,
            decoder,
            shown: 0,
            clock: 0.0,
            sink,
        })
    }

    /// Moves the video on by `dt` seconds; false once it has ended.
    pub fn advance(&mut self, dt: f32) -> bool {
        self.clock += dt;
        let due = ((self.clock * 1e6 / self.avi.frame_us.max(1) as f32) as usize + 1)
            .min(self.avi.frame_count());
        while self.shown < due {
            self.decoder.decode(self.avi.frame(self.shown));
            self.shown += 1;
        }
        self.shown < self.avi.frame_count()
    }

    /// Draws the current picture as large as fits, on black.
    pub fn draw(&self, frame: &mut Frame) {
        frame.pixels.fill(0);
        let (vw, vh) = (self.avi.width as f32, self.avi.height as f32);
        let scale = (frame.width as f32 / vw).min(frame.height as f32 / vh);
        let (dw, dh) = ((vw * scale) as u32, (vh * scale) as u32);
        let (ox, oy) = ((frame.width - dw) / 2, (frame.height - dh) / 2);
        let src = &self.decoder.pixels;
        let w = self.avi.width as usize;
        // Which source column each window column shows.
        let cols: Vec<usize> = (0..dw)
            .map(|x| ((x as f32 / scale) as usize).min(w - 1))
            .collect();
        for y in 0..dh {
            let sy = ((y as f32 / scale) as usize).min(self.avi.height as usize - 1);
            let row = &src[sy * w..sy * w + w];
            let out = ((oy + y) * frame.width + ox) as usize;
            for (px, &sx) in frame.pixels[out..out + dw as usize].iter_mut().zip(&cols) {
                *px = row[sx];
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(s) = &self.sink {
            s.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A video fills the window's height or width, centred on black, and
    /// ends after its frames (needs `OPENCONQUEST_DATA`).
    #[test]
    fn plays_letterboxed() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let path = find(Path::new(&root), "title.avi").unwrap();
        let mut player = Player::open(&path, None, 0.0).unwrap();
        assert!(player.advance(10.0));
        let (w, h) = (800u32, 600u32);
        let mut pixels = vec![0xffffffu32; (w * h) as usize];
        player.draw(&mut Frame {
            pixels: &mut pixels,
            width: w,
            height: h,
        });
        // 400x184 doubles to 800x368: black bands above and below.
        assert_eq!(pixels[0], 0);
        assert_eq!(pixels[(h - 1) as usize * w as usize], 0);
        assert!(pixels[(h / 2 * w + w / 2) as usize] != 0xffffff);
        assert!(!player.advance(60.0), "over");
    }
}
