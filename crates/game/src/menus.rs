//! The front end: main menu, mission lists, briefing and end screens.
//!
//! The art is the original's (`controlm.dkx`: `M_BKGND_00`-`05`, the
//! `M_BUT*` button shapes and the `S_FNT_*` fonts); captions and the
//! placement of buttons are this port's own. Screens are 800x600, scaled to
//! the window's height and centred.

use anyhow::{Context, Result};
use openac_formats::{Bitmap, ResourceKind, Sprite};

use crate::assets::{rgb, GameData, Image};
use crate::render::{blit_scaled, shade_rect, Frame};
use crate::text::TextRenderer;

/// A rectangle on a menu screen: x, y, width, height.
pub type Rect = (f32, f32, f32, f32);

pub const W: f32 = 800.0;
pub const H: f32 = 600.0;

/// Background art by screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backdrop {
    Title = 0,
    TitleDim = 1,
    Best = 2,
    Accomplished = 3,
    Failed = 4,
    Mission = 5,
}

/// A clickable button on a menu screen, in 800x600 coordinates.
#[derive(Debug, Clone)]
pub struct Button {
    pub caption: String,
    pub x: f32,
    pub y: f32,
    /// Its picture, `M_BUT<shape>_`: 0 and 1 wide, 2 and 3 narrow.
    pub shape: u8,
}

/// What a menu screen shows.
pub struct MenuView<'a> {
    pub backdrop: Backdrop,
    pub title: Option<&'a str>,
    /// A list to pick from (missions, saves), with the selected entry,
    /// and the first entry shown (longer lists scroll).
    pub list: Option<(&'a [String], Option<usize>)>,
    pub first: usize,
    /// Text in a box (the briefing).
    pub text: &'a [String],
    /// A few lines under the list (the multiplayer lobby's settings).
    pub notes: &'a [String],
    pub buttons: &'a [Button],
    /// The cursor, in 800x600 coordinates.
    pub hover: Option<(f32, f32)>,
    /// The title screen's version and publisher lines.
    pub credits: bool,
    /// The briefing as the original shows it (Starttsk.cpp): paragraphs
    /// in this box, wrapped to its width, each first line indented, empty
    /// paragraphs blank lines.
    pub story: Option<(Rect, &'a [String])>,
    /// Labels in the big font, at their top left.
    pub labels: &'a [(String, f32, f32)],
    /// Checkboxes (`M_CHECK0`/`1`) with their captions.
    pub checks: &'a [Check],
    /// Framed boxes.
    pub panels: &'a [Panel],
    /// Where the list goes, if not in the plain box.
    pub list_rect: Option<Rect>,
    /// A picture fitted into a box (the map of the chosen mission).
    pub picture: Option<(&'a Image, Rect)>,
    /// Text lines in a box, in the small font (a panel's contents).
    pub boxes: &'a [(Rect, Vec<String>)],
    /// Tabs (`M_LOADZ`, wide ones `M_LOADZ_BIG`; the second picture is
    /// the open tab).
    pub tabs: &'a [MenuTab],
}

/// A tab on a menu screen, at its top left.
#[derive(Debug, Clone)]
pub struct MenuTab {
    pub caption: String,
    pub x: f32,
    pub y: f32,
    pub wide: bool,
    pub open: bool,
}

/// A checkbox on a menu screen; its caption stands 30 to the right
/// (Starttsk.cpp's difficulty boxes).
#[derive(Debug, Clone)]
pub struct Check {
    pub caption: String,
    pub x: f32,
    pub y: f32,
    pub checked: bool,
}

/// The title screen's lines (string resources 1145 and 1143), drawn by
/// Startmm.cpp (0x5310a0) in boxes at (10, 0, 200, 27) and (0, 561, 799,
/// 38), the second centred.
const VERSION: &str = "Version 2.00";
const PUBLISHER: [&str; 2] = [
    "Published under license by K.E.Media for Denmark, Sweden, Finland and Norway",
    "(C) 1999, 1998 Megamedia Australia, Reaction Entertainment",
];

/// What a click hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    Button(usize),
    Item(usize),
    Check(usize),
    /// A panel's up (-1) or down (1) arrow.
    Scroll(i32),
    Tab(usize),
}

// The list and text boxes.
const LIST: (f32, f32, f32, f32) = (50.0, 110.0, 470.0, 410.0);
const ROW: f32 = 26.0;
/// List rows that fit.
pub const VISIBLE_ROWS: usize = ((LIST.3 - 8.0) / ROW) as usize;
const TEXT_BOX: (f32, f32, f32, f32) = (40.0, 90.0, 520.0, 420.0);
/// How far a lit caption rises up and left of its shadow, which stays
/// where the caption was (measured on the original).
const LIT_LIFT: f32 = 2.0;
/// Line spacing of the briefing text.
const STORY_LINE: f32 = 19.0;

pub struct Menus {
    backdrops: Vec<Option<Image>>,
    /// The button shapes `M_BUT0_` to `M_BUT3_`, normal and lit, coloured
    /// with each backdrop's palette (the original's 256-colour screen takes
    /// the backdrop's; the global palette turns the lit buttons purple).
    shapes: Vec<[[Option<Image>; 2]; 4]>,
    pub big: TextRenderer,
    /// The big font for a lit button's caption (`S_FNT_BS`; Startmm.cpp
    /// loads both).
    lit_font: TextRenderer,
    /// Its shadow (variant 0, black), drawn first, offset down and right.
    lit_shadow: TextRenderer,
    pub text: TextRenderer,
    /// Labels and checkbox captions (`S_FNT_BH` variant 2), and the
    /// briefing's text (`S_FNT_MH` variant 2), both one pixel apart (fitted
    /// to the original's "Your mission" screen).
    label_font: TextRenderer,
    story_font: TextRenderer,
    /// An unchecked box's caption (`S_FNT_BH` variant 1).
    unchecked_font: TextRenderer,
    /// The title screen's version line and publisher lines (`S_FNT_SH`).
    version_font: TextRenderer,
    publisher_font: TextRenderer,
    /// Checkbox pictures, clear and ticked, by backdrop.
    checks: Vec<[Option<Image>; 2]>,
    /// Up and down arrows, by backdrop.
    arrows: Vec<[Option<Image>; 2]>,
    /// Tabs, narrow and wide, closed and open, by backdrop.
    tabs: Vec<[[Option<Image>; 2]; 2]>,
}

impl Menus {
    pub fn load(data: &GameData, theme: char) -> Result<Self> {
        let archive = data
            .menus
            .as_ref()
            .context("controlm.dkx is not installed")?;
        let backdrop = |i: usize| -> Option<Bitmap> {
            let data = archive
                .read(&format!("{theme}M_BKGND_{i:02}"), ResourceKind::Bitmap)
                .ok()?;
            Bitmap::parse(&data).ok()
        };
        let backdrops: Vec<Option<Bitmap>> = (0..6).map(backdrop).collect();
        let main_palette = backdrops.iter().flatten().next().map(|b| b.palette.clone());
        // A sprite in the palette of backdrop `b` (the global palette if
        // it has none).
        let sprite = |name: &str, b: &Option<Bitmap>| -> Option<Image> {
            let palette = b.as_ref().map(|b| &b.palette).or(main_palette.as_ref());
            let Some(palette) = palette else {
                return data
                    .load_image(archive, &format!("{theme}{name}"), false)
                    .ok()
                    .flatten();
            };
            let e = archive.find(&format!("{theme}{name}"), ResourceKind::Sprite)?;
            let s = Sprite::parse(&archive.read_entry(e).ok()?).ok()?;
            let mut pixels = vec![None; (s.width * s.height) as usize];
            for y in 0..s.box_height {
                for x in 0..s.box_width {
                    let i = (y * s.box_width + x) as usize;
                    if s.opaque[i] != 0 {
                        pixels[((s.y + y) * s.width + s.x + x) as usize] =
                            Some(rgb(palette.0[s.pixels[i] as usize]));
                    }
                }
            }
            Some(Image::new(s.width, s.height, pixels))
        };
        let shapes = backdrops
            .iter()
            .map(|bg| {
                [0, 1, 2, 3].map(|k| {
                    [
                        sprite(&format!("M_BUT{k}_00"), bg),
                        sprite(&format!("M_BUT{k}_01"), bg),
                    ]
                })
            })
            .collect();
        // In the title screen's palette, as the original's display shows
        // them (all the front end's backdrops share its text colours).
        // Fonts have colour variants (atlases): 0 dim, 1 orange, 2 pale.
        let font = |name: &str, variant: usize| {
            TextRenderer::load_in(
                archive,
                &format!("{theme}{name}"),
                variant,
                main_palette.as_ref(),
            )
        };
        Ok(Self {
            shapes,
            checks: backdrops
                .iter()
                .map(|bg| [sprite("M_CHECK0", bg), sprite("M_CHECK1", bg)])
                .collect(),
            arrows: backdrops
                .iter()
                .map(|bg| [sprite("M_LBUT_UP0", bg), sprite("M_LBUT_DN0", bg)])
                .collect(),
            tabs: backdrops
                .iter()
                .map(|bg| {
                    [
                        [sprite("M_LOADZ0", bg), sprite("M_LOADZ1", bg)],
                        [sprite("M_LOADZ_BIG0", bg), sprite("M_LOADZ_BIG1", bg)],
                    ]
                })
                .collect(),
            backdrops: backdrops
                .iter()
                .map(|b| {
                    b.as_ref().map(|b| {
                        let pixels = b
                            .pixels
                            .iter()
                            .map(|&i| Some(rgb(b.palette.0[i as usize])))
                            .collect();
                        Image::new(b.width, b.height, pixels)
                    })
                })
                .collect(),
            // Button captions: S_FNT_BS's orange, one pixel apart
            // (measured on the original's title screen).
            big: font("S_FNT_BS", 2)?,
            lit_font: font("S_FNT_BS", 3)?,
            lit_shadow: font("S_FNT_BS", 0)?,
            text: font("S_FNT_MH", 0)?,
            // Startmm.cpp 0x5310a0: the version in variant 0, one pixel
            // apart; the publisher's lines in variant 2, two apart.
            version_font: font("S_FNT_SH", 0)?.with_spacing(1),
            label_font: font("S_FNT_BH", 2)?.with_spacing(1),
            unchecked_font: font("S_FNT_BH", 1)?.with_spacing(1),
            story_font: font("S_FNT_MH", 2)?.with_spacing(1),
            publisher_font: font("S_FNT_SH", 2)?.with_spacing(2),
        })
    }

    /// Scale and top-left of the 800x600 screen in a window.
    pub fn placement(window: (u32, u32)) -> (f32, f32, f32) {
        let s = (window.0 as f32 / W).min(window.1 as f32 / H);
        (
            s,
            (window.0 as f32 - W * s) / 2.0,
            (window.1 as f32 - H * s) / 2.0,
        )
    }

    /// A window position in 800x600 coordinates.
    pub fn to_menu(window: (u32, u32), x: f32, y: f32) -> (f32, f32) {
        let (s, ox, oy) = Self::placement(window);
        ((x - ox) / s, (y - oy) / s)
    }

    fn button_size(&self, b: &Button) -> (f32, f32) {
        self.shapes
            .first()
            .and_then(|s| s[usize::from(b.shape.min(3))][0].as_ref())
            .map_or((174.0, 40.0), |i| (i.width as f32, i.height as f32))
    }

    pub fn draw(&self, frame: &mut Frame, view: &MenuView) {
        frame.pixels.fill(0);
        let window = (frame.width, frame.height);
        let (s, ox, oy) = Self::placement(window);
        let at = |x: f32, y: f32| (ox + x * s, oy + y * s);
        if let Some(Some(bg)) = self.backdrops.get(view.backdrop as usize) {
            let (x, y) = at(0.0, 0.0);
            blit_scaled(frame, bg, (0, 0, bg.width, bg.height), x, y, s);
        }
        let ts = s.round().max(1.0) as u32;
        let write = |frame: &mut Frame, font: &TextRenderer, x: f32, y: f32, text: &str| {
            let (px, py) = at(x, y);
            font.draw(frame, px as i32, py as i32, ts, text);
        };
        let shade = |frame: &mut Frame, (x, y, w, h): (f32, f32, f32, f32)| {
            let (px, py) = at(x, y);
            shade_rect(frame, px as u32, py as u32, (w * s) as u32, (h * s) as u32);
        };
        let line = self.text.line_height() as f32 * ts as f32 / s;
        if view.credits {
            // Centred vertically in their boxes, (10, 0, 200, 27) and
            // (0, 561, 799, 38), the publisher's lines across the second.
            let vf = &self.version_font;
            let vy = ((27 - vf.line_height() as i32) / 2) as f32;
            write(frame, vf, 10.0, vy, VERSION);
            let pf = &self.publisher_font;
            let pl = pf.line_height() as f32;
            let top = 561.0 + ((38.0 - pl * PUBLISHER.len() as f32) / 2.0).floor();
            for (i, l) in PUBLISHER.iter().enumerate() {
                let w = pf.measure(l) as f32;
                write(
                    frame,
                    pf,
                    ((799.0 - w) / 2.0).floor(),
                    top + i as f32 * pl,
                    l,
                );
            }
        }
        let arrows = self
            .arrows
            .get(view.backdrop as usize)
            .or(self.arrows.first());
        for p in view.panels {
            self.draw_panel(frame, p, s, (ox, oy));
            write(frame, &self.big, p.rect.0 + 8.0, p.rect.1 + 2.0, &p.title);
            if p.arrows {
                for (k, (ax, ay)) in p.arrow_spots().into_iter().enumerate() {
                    if let Some(img) = arrows.and_then(|a| a[k].as_ref()) {
                        let (x, y) = at(ax, ay);
                        blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
                    }
                }
            }
        }
        let tabs = self.tabs.get(view.backdrop as usize).or(self.tabs.first());
        for t in view.tabs {
            let img = tabs.and_then(|k| k[usize::from(t.wide)][usize::from(t.open)].as_ref());
            if let Some(img) = img {
                let (x, y) = at(t.x, t.y);
                blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
                let tw = self.big.measure(&t.caption) as f32 * ts as f32 / s;
                let th = self.big.line_height() as f32 * ts as f32 / s;
                write(
                    frame,
                    &self.big,
                    t.x + (img.width as f32 - tw) / 2.0,
                    t.y + (img.height as f32 - th) / 2.0,
                    &t.caption,
                );
            }
        }
        if let Some((img, (bx, by, bw, bh))) = view.picture {
            let k = (bw / img.width as f32).min(bh / img.height as f32);
            let (w, h) = (img.width as f32 * k, img.height as f32 * k);
            let (x, y) = at(bx + (bw - w) / 2.0, by + (bh - h) / 2.0);
            blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s * k);
        }
        for ((bx, by, bw, bh), lines) in view.boxes {
            let width = ((bw - 12.0) * s / ts as f32) as u32;
            let mut y = by + 4.0;
            'lines: for l in lines {
                for w in crate::ui::wrap(&self.text, l, width) {
                    if y + line > by + bh {
                        break 'lines;
                    }
                    write(frame, &self.text, bx + 6.0, y, &w);
                    y += line;
                }
            }
        }
        if let Some(title) = view.title {
            write(frame, &self.big, LIST.0 + 12.0, LIST.1 - 30.0, title);
        }
        if let Some((items, selected)) = view.list {
            let list = view.list_rect.unwrap_or(LIST);
            if view.list_rect.is_none() {
                shade(frame, list);
            }
            for (i, item) in items.iter().enumerate().skip(view.first) {
                let y = list.1 + 8.0 + (i - view.first) as f32 * ROW;
                if y + ROW > list.1 + list.3 {
                    break;
                }
                // In a panel, as in the original: the chosen row framed.
                if view.list_rect.is_some() {
                    if selected == Some(i) {
                        self.outline(
                            frame,
                            (list.0 + 2.0, y - 2.0, list.2 - 4.0, ROW - 2.0),
                            s,
                            (ox, oy),
                        );
                    }
                    write(frame, &self.text, list.0 + 6.0, y, item);
                    continue;
                }
                // The chosen entry is marked; the one under the cursor too.
                let mark = if selected == Some(i) {
                    "> "
                } else if view
                    .hover
                    .is_some_and(|p| row_at(list, p).map(|r| r + view.first) == Some(i))
                {
                    "- "
                } else {
                    "  "
                };
                write(
                    frame,
                    &self.text,
                    list.0 + 12.0,
                    y,
                    &format!("{mark}{item}"),
                );
            }
            // More above or below: the wheel scrolls.
            if view.first > 0 {
                write(
                    frame,
                    &self.text,
                    list.0 + list.2 - 70.0,
                    list.1 + 8.0,
                    "^ more",
                );
            }
            if items.len() > view.first + VISIBLE_ROWS {
                let y = list.1 + list.3 - ROW;
                write(frame, &self.text, list.0 + list.2 - 70.0, y, "v more");
            }
        }
        if !view.text.is_empty() {
            shade(frame, TEXT_BOX);
            let width = ((TEXT_BOX.2 - 24.0) * s / ts as f32) as u32;
            let mut y = TEXT_BOX.1 + 12.0;
            'text: for (i, p) in view.text.iter().enumerate() {
                let font = if i == 0 { &self.big } else { &self.text };
                for l in crate::ui::wrap(font, p, width) {
                    if y + line > TEXT_BOX.1 + TEXT_BOX.3 - 8.0 {
                        break 'text;
                    }
                    write(frame, font, TEXT_BOX.0 + 12.0, y, &l);
                    y += line;
                }
                y += line / 2.0;
            }
        }
        if !view.notes.is_empty() {
            let top = LIST.1 + LIST.3 + 6.0;
            shade(frame, (LIST.0, top, LIST.2, 600.0 - top - 8.0));
            for (i, n) in view.notes.iter().enumerate() {
                write(
                    frame,
                    &self.text,
                    LIST.0 + 12.0,
                    top + 6.0 + i as f32 * line,
                    n,
                );
            }
        }
        if let Some(((bx, by, bw, _bh), paragraphs)) = view.story {
            // Wrapped at spaces to the box (empty words keep double
            // spaces), 19 a line from 19 down (fitted to the original).
            let font = &self.story_font;
            let width = bw as u32;
            let mut y = by + 19.0;
            for p in paragraphs {
                let mut line: Option<String> = None;
                for word in p.split(' ') {
                    let candidate = match &line {
                        None => word.to_string(),
                        Some(l) => format!("{l} {word}"),
                    };
                    match line {
                        // A word fits with the space after it (matches every
                        // break on the original's screen).
                        Some(l)
                            if !l.trim().is_empty()
                                && font.measure(&format!("{candidate} ")) > width =>
                        {
                            write(frame, font, bx, y, &l);
                            y += STORY_LINE;
                            line = Some(word.to_string());
                        }
                        _ => line = Some(candidate),
                    }
                }
                let line = line.unwrap_or_default();
                write(frame, font, bx, y, &line);
                y += STORY_LINE;
            }
        }
        for (label, x, y) in view.labels {
            write(frame, &self.label_font, *x, *y, label);
        }
        let checks = self
            .checks
            .get(view.backdrop as usize)
            .or(self.checks.first());
        for c in view.checks {
            if let Some(img) = checks.and_then(|k| k[usize::from(c.checked)].as_ref()) {
                let (x, y) = at(c.x, c.y);
                blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
            }
            let font = if c.checked {
                &self.label_font
            } else {
                &self.unchecked_font
            };
            write(frame, font, c.x + 30.0, c.y, &c.caption);
        }
        for b in view.buttons {
            let (w, h) = self.button_size(b);
            let lit = view.hover.is_some_and(|p| inside(p, (b.x, b.y, w, h)));
            let shapes = self
                .shapes
                .get(view.backdrop as usize)
                .or(self.shapes.first());
            let img =
                shapes.and_then(|s| s[usize::from(b.shape.min(3))][usize::from(lit)].as_ref());
            if let Some(img) = img {
                let (x, y) = at(b.x, b.y);
                blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
            }
            let font = if lit { &self.lit_font } else { &self.big };
            let tw = font.measure(&b.caption) as f32 * ts as f32 / s;
            let th = font.line_height() as f32 * ts as f32 / s;
            // Whole pixels, as the original centres (mfcfnt.cpp 0x547860).
            let (tw, th) = (tw.round(), th.round());
            if lit {
                write(
                    frame,
                    &self.lit_shadow,
                    b.x + ((w - tw) / 2.0).floor(),
                    b.y + ((h - th) / 2.0).floor(),
                    &b.caption,
                );
            }
            // A lit caption rises out of its shadow.
            let lift = if lit { LIT_LIFT } else { 0.0 };
            write(
                frame,
                font,
                b.x + ((w - tw) / 2.0).floor() - lift,
                b.y + ((h - th) / 2.0).floor() - lift,
                &b.caption,
            );
        }
    }

    /// A 1-pixel red frame around `rect` (a chosen list row).
    fn outline(&self, frame: &mut Frame, (x, y, w, h): Rect, s: f32, (ox, oy): (f32, f32)) {
        let t = s.max(1.0).round() as i64;
        let (x0, y0) = ((ox + x * s).round() as i64, (oy + y * s).round() as i64);
        let (x1, y1) = (
            (ox + (x + w) * s).round() as i64,
            (oy + (y + h) * s).round() as i64,
        );
        let mut put = |a: i64, b: i64, c: i64, d: i64| {
            for yy in b.max(0)..d.min(frame.height as i64) {
                for xx in a.max(0)..c.min(frame.width as i64) {
                    frame.pixels[(yy as u32 * frame.width + xx as u32) as usize] = PANEL_RED;
                }
            }
        };
        put(x0, y0, x1, y0 + t);
        put(x0, y1 - t, x1, y1);
        put(x0, y0, x0 + t, y1);
        put(x1 - t, y0, x1, y1);
    }

    fn draw_panel(&self, frame: &mut Frame, panel: &Panel, s: f32, (ox, oy): (f32, f32)) {
        let (x, y, w, h) = panel.rect;
        // One game pixel, in window pixels.
        let put = |frame: &mut Frame, gx: f32, gy: f32, colour: u32| {
            let (x0, y0) = ((ox + gx * s).round() as i64, (oy + gy * s).round() as i64);
            let (x1, y1) = (
                (ox + (gx + 1.0) * s).round() as i64,
                (oy + (gy + 1.0) * s).round() as i64,
            );
            for yy in y0.max(0)..y1.min(frame.height as i64) {
                for xx in x0.max(0)..x1.min(frame.width as i64) {
                    frame.pixels[(yy as u32 * frame.width + xx as u32) as usize] = colour;
                }
            }
        };
        let bevel = |frame: &mut Frame, (x, y, w, h): Rect, light: u32, dark: u32| {
            for i in 0..w as i32 {
                put(frame, x + i as f32, y, light);
                put(frame, x + i as f32, y + h - 1.0, dark);
            }
            for i in 0..h as i32 {
                put(frame, x, y + i as f32, light);
                put(frame, x + w - 1.0, y + i as f32, dark);
            }
        };
        for gy in 3..23 {
            for gx in 1..(w as i32 - 1) {
                let (px, py) = (x as i32 + gx, y as i32 + gy);
                if (px + py) % 2 == 0 {
                    put(frame, px as f32, py as f32, PANEL_STIPPLE);
                }
            }
        }
        bevel(frame, panel.rect, PANEL_GOLD, PANEL_RED);
        bevel(
            frame,
            (x + 5.0, y + PANEL_BAND, w - 10.0, h - PANEL_BAND - 5.0),
            PANEL_RED,
            PANEL_GOLD,
        );
    }

    /// What lies at 800x600 point `p`.
    pub fn hit(&self, view: &MenuView, p: (f32, f32)) -> Option<MenuHit> {
        for (i, c) in view.checks.iter().enumerate() {
            let w = 30.0 + self.big.measure(&c.caption) as f32;
            if inside(p, (c.x, c.y, w, 24.0)) {
                return Some(MenuHit::Check(i));
            }
        }
        for (i, b) in view.buttons.iter().enumerate() {
            let (w, h) = self.button_size(b);
            if inside(p, (b.x, b.y, w, h)) {
                return Some(MenuHit::Button(i));
            }
        }
        for (i, t) in view.tabs.iter().enumerate() {
            let w = if t.wide { 214.0 } else { 130.0 };
            if inside(p, (t.x, t.y, w, 28.0)) {
                return Some(MenuHit::Tab(i));
            }
        }
        for panel in view.panels.iter().filter(|q| q.arrows) {
            for (k, (ax, ay)) in panel.arrow_spots().into_iter().enumerate() {
                if inside(p, (ax, ay, 23.0, 23.0)) {
                    return Some(MenuHit::Scroll(if k == 0 { -1 } else { 1 }));
                }
            }
        }
        let (items, _) = view.list?;
        row_at(view.list_rect.unwrap_or(LIST), p)
            .map(|r| r + view.first)
            .filter(|&i| i < items.len())
            .map(MenuHit::Item)
    }
}

/// The row of list `list` at point `p`, if any.
fn row_at(list: Rect, p: (f32, f32)) -> Option<usize> {
    if !inside(p, list) {
        return None;
    }
    let i = ((p.1 - list.1 - 8.0) / ROW).floor();
    (i >= 0.0 && list.1 + 8.0 + (i + 1.0) * ROW <= list.1 + list.3).then_some(i as usize)
}

/// The original's framed boxes, measured on its screens: a raised outer
/// edge (gold top and left, red bottom and right), a sunken inner one 5 in
/// and 25 down (red top and left, gold bottom and right), and between them
/// at the top a band stippled on every other pixel; the inside is left as
/// it is. The colours are palette entries 24, 22 and 45 of the front end's
/// backdrops.
const PANEL_GOLD: u32 = 0xc69439;
const PANEL_RED: u32 = 0x9c4231;
const PANEL_STIPPLE: u32 = 0x945a41;
const PANEL_BAND: f32 = 25.0;

/// A framed box on a menu screen, its title in the band.
#[derive(Debug, Clone)]
pub struct Panel {
    pub rect: Rect,
    pub title: String,
    /// Up and down arrows at its right (`M_LBUT_UP`, `M_LBUT_DN`), which
    /// scroll its list.
    pub arrows: bool,
}

impl Panel {
    /// Where a list inside it goes (clear of its arrows).
    pub fn inner(&self) -> Rect {
        let (x, y, w, h) = self.rect;
        let arrows = if self.arrows { 26.0 } else { 0.0 };
        (
            x + 5.0,
            y + PANEL_BAND,
            w - 11.0 - arrows,
            h - PANEL_BAND - 4.0,
        )
    }

    /// Top-left corners of its up and down arrows.
    fn arrow_spots(&self) -> [(f32, f32); 2] {
        let (x, y, w, h) = self.rect;
        [
            (x + w - 30.0, y + PANEL_BAND + 2.0),
            (x + w - 30.0, y + h - 29.0),
        ]
    }
}

fn inside(p: (f32, f32), (x, y, w, h): (f32, f32, f32, f32)) -> bool {
    p.0 >= x && p.1 >= y && p.0 < x + w && p.1 < y + h
}
