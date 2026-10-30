//! Sound effects, played through the default output device.
//!
//! The scene reports sounds at world positions; voices and battle sounds
//! are placed as the original places them (0x525370), the rest fade by
//! their distance from the centre of the view.

use std::collections::HashMap;
use std::sync::Arc;

use openconquest_formats::{load_sound, Archive, Sound};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Source};

pub struct Audio {
    // Keeps the output device open.
    _stream: OutputStream,
    pub handle: OutputStreamHandle,
    cache: HashMap<String, Option<Arc<Sound>>>,
    /// The sounds playing once, so they can all be stopped.
    playing: Vec<rodio::Sink>,
    pub muted: bool,
    pub volume: f32,
}

impl Audio {
    /// Opens the default output device; `None` if there is none.
    pub fn open() -> Option<Self> {
        let (stream, handle) = OutputStream::try_default()
            .map_err(|e| eprintln!("no sound: {e}"))
            .ok()?;
        Some(Self {
            _stream: stream,
            handle,
            cache: HashMap::new(),
            playing: Vec::new(),
            muted: false,
            volume: 0.6,
        })
    }

    /// A sink playing sound `name` over and over (the menus' music).
    pub fn loop_sound(&mut self, sounds: &Archive, name: &str) -> Option<rodio::Sink> {
        let sound = match load_sound(sounds, name) {
            Ok(Some(s)) => s,
            Ok(None) => return None,
            Err(e) => {
                eprintln!("could not load sound {name}: {e}");
                return None;
            }
        };
        let sink = rodio::Sink::try_new(&self.handle).ok()?;
        sink.append(
            SamplesBuffer::new(sound.channels, sound.sample_rate, sound.samples)
                .repeat_infinite()
                .convert_samples::<f32>(),
        );
        Some(sink)
    }

    /// Plays sound `name` at `gain` (0..1, before the master volume).
    pub fn play(&mut self, sounds: &Archive, name: &str, gain: f32) {
        self.play_panned(sounds, name, gain, 0);
    }

    /// Plays sound `name` at `gain` (before the master volume) and
    /// DirectSound pan `pan` (`pan_gains`), in stereo.
    pub fn play_panned(&mut self, sounds: &Archive, name: &str, gain: f32, pan: i32) {
        if self.muted || gain <= 0.0 {
            return;
        }
        let sound =
            self.cache
                .entry(name.to_string())
                .or_insert_with(|| match load_sound(sounds, name) {
                    Ok(s) => s.map(Arc::new),
                    Err(e) => {
                        eprintln!("could not load sound {name}: {e}");
                        None
                    }
                });
        let Some(sound) = sound else { return };
        let source = if pan == 0 {
            SamplesBuffer::new(sound.channels, sound.sample_rate, sound.samples.clone())
        } else {
            // Each frame to both sides (a mono sound copied), each side
            // at its gain.
            let (left, right) = pan_gains(pan);
            let n = sound.channels.max(1) as usize;
            let samples = sound
                .samples
                .chunks(n)
                .flat_map(|f| {
                    let (l, r) = (f[0], *f.get(1).unwrap_or(&f[0]));
                    [(l as f32 * left) as i16, (r as f32 * right) as i16]
                })
                .collect::<Vec<i16>>();
            SamplesBuffer::new(2, sound.sample_rate, samples)
        }
        .amplify(gain * self.volume)
        .convert_samples::<f32>();
        self.playing.retain(|s| !s.empty());
        match rodio::Sink::try_new(&self.handle) {
            Ok(sink) => {
                sink.append(source);
                self.playing.push(sink);
            }
            Err(e) => eprintln!("could not play {name}: {e}"),
        }
    }

    /// Stops every sound playing once (as 0x569070 stops every DirectSound
    /// buffer).
    pub fn stop_all(&mut self) {
        for s in self.playing.drain(..) {
            s.stop();
        }
    }
}

/// Where the original hears from (0x51a040-0x51a0b8 -> 0x524d90: the
/// sound object's `+0xd08` / `+0xd0c`): the view's left / top cell (view
/// `+0x2c` / `+0x30`) plus its cells across / down (screen `+0x17be` /
/// `+0x17c2`: 9 or, with the bar hidden, 12 across, 8 down), halved and
/// rounded up (`w / 2 + w % 2`). The port's view zooms: its left / top cell
/// is the cell under its top-left corner and its cells across / down the
/// whole cells its width / height holds (the original's 592 x 512 field
/// holds 9 x 8 of them, its 800-wide one 12), so the original's view gives
/// the original's cell.
pub fn listener(view_x: f32, view_y: f32, view_w: f32, view_h: f32) -> (i32, i32) {
    let cell = openconquest_formats::map::TILE_SIZE as f32;
    let (across, down) = ((view_w / cell) as i32, (view_h / cell) as i32);
    let (left, top) = (
        (view_x / cell).floor() as i32,
        (view_y / cell).floor() as i32,
    );
    (left + across / 2 + across % 2, top + down / 2 + down % 2)
}

/// A sound placed on cell `(x, y)` (the caller's object's cell) as the
/// original's player places its channels 1, 2 and 4 (0x525370): its
/// DirectSound attenuation and pan, in hundredths of a dB, from the
/// effects volume `master` (`[0x5ca56c]`, the slider's position less
/// 4000), or `None` when it is not heard. Off the map (a coordinate below
/// 0) at the master volume, in the middle. Else `d = (byte)
/// ftol(sqrt(dx^2 + dy^2))` cells from the listener, at most 20; 20 is
/// silent (the byte wraps at 256); the master within 5 cells, `master -
/// (master + 2000) x d / 12` from 6 to 12, -2000 from 13; the pan
/// `min(|dx|, 20) x 50`, to the side of the sound (negative to the left).
pub fn placed(master: i32, listener: (i32, i32), cell: (i32, i32)) -> Option<(i32, i32)> {
    placed_within(master, listener, cell, 20)
}

/// As `placed`, for the ambient channels 5 and 6 (0x525370 at 0x5255eb
/// and 0x525a70): the same volume and pan, silent from 12 cells.
pub fn placed_ambient(master: i32, listener: (i32, i32), cell: (i32, i32)) -> Option<(i32, i32)> {
    placed_within(master, listener, cell, 12)
}

fn placed_within(
    master: i32,
    listener: (i32, i32),
    (x, y): (i32, i32),
    cut: u8,
) -> Option<(i32, i32)> {
    if x < 0 || y < 0 {
        return Some((master, 0));
    }
    let (dx, dy) = (listener.0 - x, listener.1 - y);
    let square = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    let d = ((square as f64).sqrt() as i64 as u8).min(20);
    if d >= cut {
        return None;
    }
    let attenuation = match d {
        0..=5 => master,
        6..=12 => master - (master + 2000) * d as i32 / 12,
        _ => -2000,
    };
    let pan = (x - listener.0).abs().min(20) * (x - listener.0).signum() * 50;
    Some((attenuation, pan))
}

/// The channel gains DirectSound gives pan `pan` (hundredths of a dB): the
/// far side's channel is attenuated by it, the near side's kept.
pub fn pan_gains(pan: i32) -> (f32, f32) {
    let cut = 10f32.powf(-(pan.abs() as f32) / 2000.0);
    match pan.signum() {
        1 => (cut, 1.0),
        -1 => (1.0, cut),
        _ => (1.0, 1.0),
    }
}

/// How loud a sound at distance `d` (world pixels) from the view's centre
/// is, given the view's half-diagonal `reach`.
pub fn falloff(d: f32, reach: f32) -> f32 {
    (1.0 - (d / (reach * 1.5)).powi(2)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listener: the view's left / top cell plus half its cells
    /// across / down, rounded up (0x51a040): the original's 9 x 8 field at
    /// (10, 20) hears from (15, 24), its 12-wide one from (16, 24).
    #[test]
    fn listener_is_the_views_middle_cell() {
        assert_eq!(listener(640.0, 1280.0, 592.0, 512.0), (15, 24));
        assert_eq!(listener(640.0, 1280.0, 800.0, 512.0), (16, 24));
    }

    /// 0x525370's cut-off, volume and pan for channels 1, 2 and 4.
    #[test]
    fn placed_sounds_as_the_original() {
        let ear = (30, 30);
        assert_eq!(placed(-1000, ear, (30, 30)), Some((-1000, 0)));
        assert_eq!(placed(-1000, ear, (35, 30)), Some((-1000, 250)));
        // 6 cells: master - (master + 2000) x 6 / 12.
        assert_eq!(placed(-1000, ear, (24, 30)), Some((-1500, -300)));
        assert_eq!(placed(-1000, ear, (30, 42)), Some((-2000, 0)));
        assert_eq!(placed(-1000, ear, (30, 43)), Some((-2000, 0)));
        // sqrt(19^2 + 5^2) = 19.6: 19, heard; the pan stops at 20 cells.
        assert_eq!(placed(-1000, ear, (11, 25)), Some((-2000, -950)));
        assert_eq!(placed(-1000, ear, (50, 30)), None);
        assert_eq!(placed(-1000, ear, (-1, 3)), Some((-1000, 0)));
        // The byte wraps: 256 cells off is heard as 0.
        assert_eq!(placed(0, (0, 0), (256, 0)), Some((0, 1000)));
    }

    /// Channels 5 and 6: silent from 12 cells, else as above.
    #[test]
    fn ambient_sounds_cut_at_twelve() {
        let ear = (30, 30);
        assert_eq!(placed_ambient(-1000, ear, (24, 30)), Some((-1500, -300)));
        assert_eq!(placed_ambient(-1000, ear, (30, 41)), Some((-1916, 0)));
        assert_eq!(placed_ambient(-1000, ear, (30, 42)), None);
        assert_eq!(placed_ambient(-1000, ear, (-1, 3)), Some((-1000, 0)));
    }
}
