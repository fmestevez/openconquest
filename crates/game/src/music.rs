//! Music: the game's MIDI tunes (`sound/ancient1.mid`-`ancient4.mid`, the
//! playlist every mission's `TRACKS_MID` names), played through a General
//! MIDI SoundFont supplied by the player, since the game relied on the
//! sound card's synthesizer; or, if the player has ripped the game CD's
//! audio tracks (2-9, the playlist of `TRACKS_CD`; they are not on the
//! data image), those.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use rodio::{OutputStreamHandle, Sink, Source};
use rustysynth::{MidiFile, MidiFileSequencer, SoundFont, Synthesizer, SynthesizerSettings};

const SAMPLE_RATE: u32 = 44_100;
const BLOCK: usize = 1024;

/// Where General MIDI SoundFonts are commonly installed.
const SOUNDFONT_PLACES: [&str; 6] = [
    "/usr/share/sounds/sf2/FluidR3_GM.sf2",
    "/usr/share/sounds/sf2/default-GM.sf2",
    "/usr/share/soundfonts/FluidR3_GM.sf2",
    "/usr/share/soundfonts/default.sf2",
    "/opt/homebrew/share/soundfonts/default.sf2",
    "/usr/local/share/soundfonts/default.sf2",
];

/// The SoundFont to use: the one given, else the first commonly installed.
pub fn find_soundfont(given: Option<&Path>) -> Option<PathBuf> {
    given.map(Path::to_path_buf).or_else(|| {
        SOUNDFONT_PLACES
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
    })
}

/// The game's tunes in playlist order.
pub fn playlist(sound_dir: &Path) -> Vec<PathBuf> {
    (1..=4)
        .filter_map(|i| {
            crate::assets::find_ci(sound_dir, &format!("ancient{i}.mid"))
                .ok()
                .flatten()
        })
        .collect()
}

/// The game CD's audio tracks (2-9) ripped by the player into `dir`, in
/// order: files whose name holds the track number (`Track02.flac`,
/// `02 - Ancient Conquest.ogg`, ...), as WAV, FLAC or Ogg Vorbis.
pub fn cd_tracks(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(u32, PathBuf)> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "wav" | "flac" | "ogg"))
        })
        .filter_map(|p| {
            let stem = p.file_stem()?.to_str()?.to_string();
            let digits: String = stem
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            let n: u32 = digits.parse().ok()?;
            (2..=9).contains(&n).then_some((n, p))
        })
        .collect();
    files.sort();
    files.dedup_by_key(|f| f.0);
    files.into_iter().map(|(_, p)| p).collect()
}

/// What the music plays from.
enum Tunes {
    Midi(Arc<SoundFont>, Vec<Arc<MidiFile>>),
    Files(Vec<PathBuf>),
}

pub struct Music {
    sink: Sink,
    tunes: Tunes,
    next: usize,
}

impl Music {
    pub fn new(handle: &OutputStreamHandle, soundfont: &Path, tunes: &[PathBuf]) -> Result<Self> {
        let mut file =
            File::open(soundfont).with_context(|| format!("opening {}", soundfont.display()))?;
        let font = Arc::new(SoundFont::new(&mut file).map_err(|e| anyhow::anyhow!("{e:?}"))?);
        let tunes = tunes
            .iter()
            .map(|p| load_midi(p).map(Arc::new))
            .collect::<Result<Vec<_>>>()?;
        let sink = Sink::try_new(handle)?;
        Ok(Self {
            sink,
            tunes: Tunes::Midi(font, tunes),
            next: 0,
        })
    }

    /// Plays recorded tracks (see `cd_tracks`) in turn.
    pub fn from_files(handle: &OutputStreamHandle, files: Vec<PathBuf>) -> Result<Self> {
        Ok(Self {
            sink: Sink::try_new(handle)?,
            tunes: Tunes::Files(files),
            next: 0,
        })
    }

    pub fn set_volume(&self, v: f32) {
        self.sink.set_volume(v);
    }

    /// Holds the music (while the menus play their own) or lets it go on.
    pub fn set_paused(&self, paused: bool) {
        if paused {
            self.sink.pause();
        } else {
            self.sink.play();
        }
    }

    /// Queues the next tune when the current one ends.
    pub fn keep_playing(&mut self) {
        if !self.sink.empty() {
            return;
        }
        match &mut self.tunes {
            Tunes::Midi(font, tunes) => {
                if tunes.is_empty() {
                    return;
                }
                let tune = tunes[self.next % tunes.len()].clone();
                self.next += 1;
                match Tune::new(font, tune) {
                    Ok(source) => self.sink.append(source),
                    Err(e) => {
                        eprintln!("music: {e:#}");
                        tunes.clear();
                    }
                }
            }
            Tunes::Files(files) => {
                if files.is_empty() {
                    return;
                }
                let k = self.next % files.len();
                self.next += 1;
                let source = File::open(&files[k])
                    .map_err(anyhow::Error::from)
                    .and_then(|f| {
                        rodio::Decoder::new(std::io::BufReader::new(f)).map_err(Into::into)
                    });
                match source {
                    Ok(source) => self.sink.append(source),
                    Err(e) => {
                        eprintln!("music: {}: {e:#}", files[k].display());
                        files.remove(k);
                    }
                }
            }
        }
    }
}

pub fn load_midi(path: &Path) -> Result<MidiFile> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    MidiFile::new(&mut file).map_err(|e| anyhow::anyhow!("{}: {e:?}", path.display()))
}

/// One tune, rendered block by block as it plays.
struct Tune {
    sequencer: MidiFileSequencer,
    end: f64,
    left: Vec<f32>,
    right: Vec<f32>,
    /// Next sample in the current block (interleaved: 2 per frame).
    pos: usize,
}

impl Tune {
    fn new(font: &Arc<SoundFont>, midi: Arc<MidiFile>) -> Result<Self> {
        let settings = SynthesizerSettings::new(SAMPLE_RATE as i32);
        let synth = Synthesizer::new(font, &settings).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let end = midi.get_length();
        let mut sequencer = MidiFileSequencer::new(synth);
        sequencer.play(&midi, false);
        Ok(Self {
            sequencer,
            end,
            left: vec![0.0; BLOCK],
            right: vec![0.0; BLOCK],
            pos: 2 * BLOCK,
        })
    }
}

impl Iterator for Tune {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.pos >= 2 * BLOCK {
            if self.sequencer.get_position() > self.end + 1.0 {
                return None;
            }
            self.sequencer.render(&mut self.left, &mut self.right);
            self.pos = 0;
        }
        let i = self.pos / 2;
        let s = if self.pos % 2 == 0 {
            self.left[i]
        } else {
            self.right[i]
        };
        self.pos += 1;
        Some(s)
    }
}

impl Source for Tune {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ripped CD tracks are found by their number, in order, and decode.
    #[test]
    fn cd_tracks_found_and_decoded() {
        let dir = std::env::temp_dir().join(format!("openconquest-cd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A tenth of a second of silence as 16-bit mono PCM.
        let samples = 4410u32;
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + samples * 2).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(44_100u32.to_le_bytes());
        wav.extend((44_100u32 * 2).to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend((samples * 2).to_le_bytes());
        wav.extend(vec![0u8; samples as usize * 2]);
        for name in ["Track03.wav", "Track02.wav", "01 data.wav", "notes.txt"] {
            std::fs::write(dir.join(name), &wav).unwrap();
        }
        let tracks = cd_tracks(&dir);
        let names: Vec<_> = tracks
            .iter()
            .map(|p| p.file_name().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["Track02.wav", "Track03.wav"]);
        let f = std::io::BufReader::new(File::open(&tracks[0]).unwrap());
        let decoded = rodio::Decoder::new(f).unwrap();
        assert_eq!(decoded.count(), samples as usize);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The game's tunes parse (needs `OPENCONQUEST_DATA`).
    #[test]
    fn tunes_parse() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let tunes = playlist(&Path::new(&root).join("sound"));
        assert_eq!(tunes.len(), 4);
        for t in &tunes {
            let midi = load_midi(t).unwrap();
            assert!(
                midi.get_length() > 30.0,
                "{} is {}s",
                t.display(),
                midi.get_length()
            );
        }
    }

    /// A minimal SoundFont: one preset playing a looped sine wave.
    fn sine_soundfont() -> Vec<u8> {
        fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut out = id.to_vec();
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(body);
            if body.len() % 2 == 1 {
                out.push(0);
            }
            out
        }
        fn list(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
            let mut body = kind.to_vec();
            for c in chunks {
                body.extend(c);
            }
            chunk(b"LIST", &body)
        }
        fn name(s: &str) -> Vec<u8> {
            let mut n = s.as_bytes().to_vec();
            n.resize(20, 0);
            n
        }
        let u16s = |v: &[u16]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let u32s = |v: &[u32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        let len = 441u32;
        let mut samples: Vec<i16> = (0..len)
            .map(|i| ((i as f32 / len as f32 * std::f32::consts::TAU).sin() * 12000.0) as i16)
            .collect();
        samples.extend([0i16; 46]);
        let smpl: Vec<u8> = samples.iter().flat_map(|x| x.to_le_bytes()).collect();
        let info = list(
            b"INFO",
            &[
                chunk(b"ifil", &u16s(&[2, 1])),
                chunk(b"isng", b"EMU8000\0"),
                chunk(b"INAM", b"sine\0\0"),
            ],
        );
        let sdta = list(b"sdta", &[chunk(b"smpl", &smpl)]);
        let mut phdr = name("sine");
        phdr.extend(u16s(&[0, 0, 0]));
        phdr.extend(u32s(&[0, 0, 0]));
        phdr.extend(name("EOP"));
        phdr.extend(u16s(&[0, 0, 1]));
        phdr.extend(u32s(&[0, 0, 0]));
        let mut inst = name("sine");
        inst.extend(u16s(&[0]));
        inst.extend(name("EOI"));
        inst.extend(u16s(&[1]));
        let mut shdr = name("sine");
        shdr.extend(u32s(&[0, len, 0, len, 44100]));
        shdr.extend([69u8, 0]);
        shdr.extend(u16s(&[0, 1]));
        shdr.extend(name("EOS"));
        shdr.extend(vec![0u8; 26]);
        let pdta = list(
            b"pdta",
            &[
                chunk(b"phdr", &phdr),
                chunk(b"pbag", &u16s(&[0, 0, 1, 0])),
                chunk(b"pmod", &[0u8; 10]),
                // Instrument 0, then the terminal record.
                chunk(b"pgen", &u16s(&[41, 0, 0, 0])),
                chunk(b"inst", &inst),
                chunk(b"ibag", &u16s(&[0, 0, 2, 0])),
                chunk(b"imod", &[0u8; 10]),
                // Loop continuously, sample 0, then the terminal record.
                chunk(b"igen", &u16s(&[54, 1, 53, 0, 0, 0])),
                chunk(b"shdr", &shdr),
            ],
        );
        let mut body = b"sfbk".to_vec();
        body.extend(info);
        body.extend(sdta);
        body.extend(pdta);
        chunk(b"RIFF", &body)
    }

    /// A tune renders to sound (needs `OPENCONQUEST_DATA`).
    #[test]
    fn tunes_play() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let font = Arc::new(SoundFont::new(&mut std::io::Cursor::new(sine_soundfont())).unwrap());
        let tune = playlist(&Path::new(&root).join("sound")).remove(0);
        let midi = Arc::new(load_midi(&tune).unwrap());
        let samples: Vec<f32> = Tune::new(&font, midi)
            .unwrap()
            .take(SAMPLE_RATE as usize * 2 * 6)
            .collect();
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt();
        assert!(rms > 0.001, "silent: {rms}");
        assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 4.0));
    }
}
