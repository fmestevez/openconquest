//! The front end's fades between screens (Ourlib's Palette.cpp): the
//! display palette taken down to black and back up to the new screen's
//! backdrop palette, its lightness scaled in HLS space.
//!
//! On every screen change (startsys.cpp's dispatcher 0x53e1e0) the old
//! picture fades out with 0x4ef350(50, 5) (0x54a460), the new screen
//! paints under the black palette and its message 0x102 fades in with
//! 0x4ef530(game, backdrop, 50, 5) (0x54a230). Each fade busy-waits on
//! `timeGetTime()`: a step whenever 5 ms have passed since the last, 50
//! steps, then the target palette exactly (fade in) or black (fade out).
//! The palette is set from entry 10 to 245 only (0x56c360, 0x56c230 clip
//! the range in 8-bit mode): the 20 Windows colours keep theirs. The port
//! draws in true colour, so the scaling is applied to each pixel whose
//! colour is one of the palette's entries 10-245 (or none of its entries).

/// Steps of a fade and the milliseconds each waits at least (0x4ef350,
/// 0x4ef530: 0x32, 5).
pub const STEPS: u32 = 50;
pub const STEP_MS: u64 = 5;

/// The first and one past the last palette entry a fade sets (0x56c360).
const FIRST: usize = 10;
const END: usize = 246;

/// 1/255 (f32 at 0x59bb44), 1/60 (f32 at 0x59bb50).
fn byte() -> f32 {
    f32::from_bits(0x3b80_8081)
}
fn sixtieth() -> f32 {
    f32::from_bits(0x3c88_8889)
}
/// The hue of a grey (999.0, 0x59bb68).
const UNDEFINED: f32 = 999.0;

/// A colour in HLS: hue in degrees (999 for a grey), lightness and
/// saturation 0-1, each kept as the original's f32.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Hls {
    h: f32,
    l: f32,
    s: f32,
}

/// RGB to HLS (0x549ba0). The x87 keeps what it does not store in its
/// registers: those values are f64 here, the stored ones f32.
fn to_hls(r: u8, g: u8, b: u8) -> Hls {
    let r = (f64::from(r) * f64::from(byte())) as f32;
    let g = (f64::from(g) * f64::from(byte())) as f32;
    let b = (f64::from(b) * f64::from(byte())) as f32;
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let sum = f64::from(mx) + f64::from(mn);
    let l_reg = sum * 0.5;
    let l = l_reg as f32;
    if mx == mn {
        return Hls {
            h: UNDEFINED,
            l,
            s: 0.0,
        };
    }
    let d = (f64::from(mx) - f64::from(mn)) as f32;
    let s = if l_reg <= 0.5 {
        (f64::from(d) / f64::from(sum as f32)) as f32
    } else {
        (f64::from(d) / (2.0 - f64::from(mx) - f64::from(mn))) as f32
    };
    let rc = ((f64::from(mx) - f64::from(r)) / f64::from(d)) as f32;
    let gc = ((f64::from(mx) - f64::from(g)) / f64::from(d)) as f32;
    let bc = ((f64::from(mx) - f64::from(b)) / f64::from(d)) as f32;
    let h = if r == mx {
        (f64::from(bc) - f64::from(gc)) as f32
    } else if g == mx {
        (f64::from(rc) + 2.0 - f64::from(bc)) as f32
    } else {
        (f64::from(gc) + 4.0 - f64::from(rc)) as f32
    };
    let h60 = f64::from(h) * 60.0;
    let h = if h60 < 0.0 {
        (h60 + 360.0) as f32
    } else {
        h60 as f32
    };
    Hls { h, l, s }
}

/// One channel of HLS to RGB (0x549d60): the hue `h` brought into 0-360.
fn channel(mut h: f32, m1: f32, m2: f32) -> f64 {
    if h < 0.0 {
        h = (f64::from(h) + 360.0) as f32;
    }
    if h > 360.0 {
        h = (f64::from(h) - 360.0) as f32;
    }
    let (m1, m2, hd) = (f64::from(m1), f64::from(m2), f64::from(h));
    if h < 60.0 {
        (m2 - m1) * hd * f64::from(sixtieth()) + m1
    } else if h < 180.0 {
        m2
    } else if h < 240.0 {
        (m2 - m1) * (240.0 - hd) * f64::from(sixtieth()) + m1
    } else {
        m1
    }
}

/// HLS to RGB (0x549e30), each channel `ftol(c * 255)`; none for a
/// colour of no saturation with a hue (error -5: the entry keeps its
/// colour).
fn to_rgb(c: Hls) -> Option<[u8; 3]> {
    let (l, s) = (f64::from(c.l), f64::from(c.s));
    let m2 = if l >= 0.5 {
        ((s + l) - s * l) as f32
    } else {
        ((s + 1.0) * l) as f32
    };
    let m1 = (l + l - f64::from(m2)) as f32;
    let byte = |v: f64| (v * 255.0) as i64 as u8;
    if c.s == 0.0 {
        if c.h != UNDEFINED {
            return None;
        }
        let v = byte(l);
        return Some([v, v, v]);
    }
    let r = channel((f64::from(c.h) + 120.0) as f32, m1, m2) as f32;
    let g = channel(c.h, m1, m2) as f32;
    let b = channel((f64::from(c.h) - 120.0) as f32, m1, m2);
    Some([byte(f64::from(r)), byte(f64::from(g)), byte(b)])
}

/// Colour `rgb` (0xRRGGBB) with its lightness at `percent` (0x549f20 with
/// hue and saturation at 100): to HLS, each component times its percent x
/// 0.01, hue kept under 360 and the others under 1, and back.
pub fn lightness(rgb: u32, percent: u8) -> u32 {
    let (r, g, b) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    let mut c = to_hls(r, g, b);
    if c.h != UNDEFINED {
        c.h = (100.0 * f64::from(c.h) * 0.01) as f32;
    }
    let l = f64::from(percent) * f64::from(c.l) * 0.01;
    c.l = l as f32;
    c.s = (100.0 * f64::from(c.s) * 0.01) as f32;
    if c.h > 360.0 && c.h != UNDEFINED {
        c.h = 360.0;
    }
    if l > 1.0 {
        c.l = 1.0;
    }
    if c.s > 1.0 {
        c.s = 1.0;
    }
    match to_rgb(c) {
        Some([r, g, b]) => (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
        None => rgb,
    }
}

/// Which way a fade goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Way {
    Out,
    In,
}

/// What the palette is `elapsed` ms into a fade (0x54a100, 0x54a330): as
/// it was before the first step (5 ms in); then step i's lightness,
/// ftol(100 / 50 x i) percent, 100 less that for a fade out; after the
/// fiftieth step the target, exactly (none).
pub fn level(way: Way, elapsed_ms: u64) -> Level {
    let steps = (elapsed_ms / STEP_MS).min(u64::from(STEPS)) as u32;
    if steps == STEPS {
        return Level::Done;
    }
    if steps == 0 {
        return Level::Before;
    }
    let p = (100.0 / f64::from(STEPS) * f64::from(steps - 1)) as u8;
    Level::Percent(match way {
        Way::In => p,
        Way::Out => 100 - p,
    })
}

/// A fade's palette at some moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Before the first step.
    Before,
    /// The lightness of the step shown.
    Percent(u8),
    /// The fade is over.
    Done,
}

/// The colours of a screen's palette a fade changes and how, for one
/// step: each of its entries 10-245 at `percent` (black for none); colours
/// that are only among the other entries stay. A colour in none of them
/// (drawn in another screen's palette) is scaled as the first.
pub struct Shade {
    keep: std::collections::HashSet<u32>,
    percent: Option<u8>,
    cache: std::cell::RefCell<std::collections::HashMap<u32, u32>>,
}

impl Shade {
    /// The step of `percent` (none: black) over `palette` (256 colours).
    pub fn new(palette: &[u32], percent: Option<u8>) -> Self {
        let inner: std::collections::HashSet<u32> = palette
            .get(FIRST..END)
            .unwrap_or(&[])
            .iter()
            .copied()
            .collect();
        let keep = palette
            .iter()
            .enumerate()
            .filter(|&(i, c)| !(FIRST..END).contains(&i) && !inner.contains(c))
            .map(|(_, &c)| c)
            .collect();
        Self {
            keep,
            percent,
            cache: Default::default(),
        }
    }

    /// Colour `c` at this step.
    pub fn apply(&self, c: u32) -> u32 {
        let c = c & 0xff_ffff;
        if self.keep.contains(&c) {
            return c;
        }
        let Some(p) = self.percent else { return 0 };
        *self
            .cache
            .borrow_mut()
            .entry(c)
            .or_insert_with(|| lightness(c, p))
    }

    /// `from` shaded into `to`.
    pub fn frame(&self, from: &[u32], to: &mut [u32]) {
        let mut last = None;
        for (o, &i) in to.iter_mut().zip(from) {
            *o = match last {
                Some((a, b)) if a == i => b,
                _ => {
                    let b = self.apply(i);
                    last = Some((i, b));
                    b
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_come_every_five_milliseconds() {
        assert_eq!(level(Way::Out, 0), Level::Before);
        assert_eq!(level(Way::Out, 4), Level::Before);
        assert_eq!(level(Way::Out, 5), Level::Percent(100));
        assert_eq!(level(Way::Out, 12), Level::Percent(98));
        assert_eq!(level(Way::In, 5), Level::Percent(0));
        assert_eq!(level(Way::In, 249), Level::Percent(96));
        assert_eq!(level(Way::In, 250), Level::Done);
        assert_eq!(level(Way::Out, 245), Level::Percent(4));
    }

    #[test]
    fn lightness_scales_in_hls() {
        // Full lightness gives the colour back, black at none.
        for c in [0x9c4231, 0xffff00, 0x123456, 0xffffff, 0x808080] {
            assert_eq!(lightness(c, 0), 0, "{c:06x}");
        }
        assert_eq!(lightness(0xffffff, 100), 0xffffff);
        assert_eq!(lightness(0x808080, 100), 0x808080);
        // Half the lightness of white is a mid grey: 0.5 x 255, truncated.
        assert_eq!(lightness(0xffffff, 50), 0x7f7f7f);
        // A colour keeps its hue and saturation: pure red at half
        // lightness is a dark red (L 0.5 -> 0.25, S 1).
        assert_eq!(lightness(0xff0000, 50), 0x7f0000);
    }

    #[test]
    fn the_windows_colours_stay() {
        let mut palette = vec![0u32; 256];
        palette[7] = 0xc0c0c0;
        palette[100] = 0x808080;
        palette[255] = 0xffffff;
        palette[175] = 0xffffff;
        let shade = Shade::new(&palette, Some(50));
        assert_eq!(shade.apply(0xc0c0c0), 0xc0c0c0);
        assert_eq!(shade.apply(0x808080), lightness(0x808080, 50));
        // White is also entry 175: it fades.
        assert_eq!(shade.apply(0xffffff), 0x7f7f7f);
        assert_eq!(Shade::new(&palette, None).apply(0x808080), 0);
    }
}
