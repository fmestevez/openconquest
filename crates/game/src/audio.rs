//! Sound effects, played through the default output device.
//!
//! The scene reports sounds at world positions; they play louder the nearer
//! they are to the centre of the view and not at all far outside it.

use std::collections::HashMap;
use std::sync::Arc;

use openac_formats::{load_sound, Archive, Sound};
use rodio::buffer::SamplesBuffer;
use rodio::{OutputStream, OutputStreamHandle, Source};

pub struct Audio {
    // Keeps the output device open.
    _stream: OutputStream,
    handle: OutputStreamHandle,
    cache: HashMap<String, Option<Arc<Sound>>>,
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
            muted: false,
            volume: 0.6,
        })
    }

    /// Plays sound `name` at `gain` (0..1, before the master volume).
    pub fn play(&mut self, sounds: &Archive, name: &str, gain: f32) {
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
        let source = SamplesBuffer::new(sound.channels, sound.sample_rate, sound.samples.clone())
            .amplify(gain * self.volume)
            .convert_samples::<f32>();
        if let Err(e) = self.handle.play_raw(source) {
            eprintln!("could not play {name}: {e}");
        }
    }
}

/// How loud a sound at distance `d` (world pixels) from the view's centre
/// is, given the view's half-diagonal `reach`.
pub fn falloff(d: f32, reach: f32) -> f32 {
    (1.0 - (d / (reach * 1.5)).powi(2)).clamp(0.0, 1.0)
}
