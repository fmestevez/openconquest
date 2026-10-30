//! The front end: main menu, mission lists, briefing and end screens.
//!
//! The art is the original's (`controlm.dkx`: `M_BKGND_00`-`05`, the
//! `M_BUT*` button shapes, the widget pictures and the `S_FNT_*` fonts), and
//! so are the captions (the exe's string resources) and where the buttons
//! and widgets stand on the title screen, Custom missions, Load game, the
//! briefing, the end screen, Best Conquerors and the multiplayer connect
//! screen (startsys.cpp's screens, 0x528000-0x540400). The port's own:
//! the Multiplayer and Waiting screens; the differences still open are
//! listed in docs/FORMATS.md. Screens are
//! 800x600, scaled to the window's height and centred.

use anyhow::{Context, Result};
use openconquest_formats::{Bitmap, ResourceKind, Sprite};

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
    /// The button held down with the pointer inside it (_sbutton.cpp
    /// state 2).
    pub pressed: Option<usize>,
    /// The title screen's version and publisher lines.
    pub credits: bool,
    /// The briefing as the original shows it (Starttsk.cpp): paragraphs
    /// in this box, wrapped to its width, each first line indented, empty
    /// paragraphs blank lines.
    pub story: Option<(Rect, &'a [String])>,
    /// The story's font and line pitch, if not the briefing's.
    pub story_style: Option<(Style, f32)>,
    /// The first line of the story shown (it scrolls).
    pub story_first: usize,
    /// Labels in the big font, at their top left.
    pub labels: &'a [(String, f32, f32)],
    /// Checkboxes (`M_CHECK0`/`1`) with their captions.
    pub checks: &'a [Check],
    /// Where the list goes, if not in the plain box.
    pub list_rect: Option<Rect>,
    /// A picture fitted into a box (the map of the chosen mission).
    pub picture: Option<(&'a Image, Rect)>,
    /// Text lines in a box, in the small font (a panel's contents).
    pub boxes: &'a [(Rect, Vec<String>)],
    /// The original's own widget pictures (panels, scroll tracks, list
    /// rows), by name, at their top left; drawn over the backdrop first.
    pub sprites: &'a [Placed],
    /// Text in a given font, variant and spacing, at its top left.
    pub texts: &'a [Text],
    /// A picture shown 1:1 in a box, from the given offset into it (the
    /// map of the chosen mission).
    pub map: Option<(&'a Image, Rect, (u32, u32))>,
    /// Row height of the list, when not the plain box's.
    pub row_height: Option<f32>,
    /// Clickable spots beyond buttons and the list (scroll arrows).
    pub spots: &'a [(Rect, MenuHit)],
    /// Scroll bar tracks: a click or a drag on one picks a position.
    pub tracks: &'a [Track],
    /// Filled rectangles (a text field's cursor), drawn last.
    pub fills: &'a [(Rect, u32)],
    /// Tabs (`M_LOADZ`, wide ones `M_LOADZ_BIG`; the second picture is
    /// the open tab).
    pub tabs: &'a [MenuTab],
    /// A sprite over the whole screen (Custom missions' kind pop-up, layer
    /// 10): its pictures, then its texts, drawn last.
    pub top: (&'a [Placed], &'a [Text]),
}

/// One of the original's widget pictures placed on a screen.
#[derive(Debug, Clone)]
pub struct Placed {
    pub name: String,
    pub x: f32,
    pub y: f32,
}

/// The fonts of the front end, by name (`S_FNT_*` without the prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style {
    pub font: &'static str,
    pub variant: usize,
    pub spacing: u32,
}

/// Text placed on a screen.
#[derive(Debug, Clone)]
pub struct Text {
    pub style: Style,
    pub x: f32,
    pub y: f32,
    pub text: String,
    /// The width of the box it is drawn into, cut off at its right
    /// (0x4eb430 draws into a bitmap of the box's size); none, all of it.
    pub clip: Option<f32>,
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
/// 38), the second centred. The version's number is the third byte of the
/// registry's `Type` (game + 0xa7d, `(Type >> 16) & 0xff` into "Version
/// 2.%02d"), which the CD's installer writes as the word 0xa21e (setup.exe
/// 0x416040) over the install type (0x404530; 1, 2 or 4): 30.
const INSTALLED_TYPE: u32 = 0xa21e_0000;

fn version() -> String {
    format!("Version 2.{:02}", (INSTALLED_TYPE >> 16) & 0xff)
}

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
    /// A scroll arrow: what it scrolls, back (-1) or on (1).
    Step(Scroller, i32),
    /// A scroll bar's track at a position of what it scrolls.
    Track(Scroller, usize),
    Tab(usize),
    /// Custom missions' kind field of a players row (_sbutton 0x197 + k),
    /// or its arrow (M_CBUT, 0x1af + k) when the flag is set.
    Kind(usize, bool),
}

/// What a scroll bar scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroller {
    /// The screen's list.
    List,
    /// The mission description.
    Story,
    /// The map preview, across and down.
    MapX,
    MapY,
}

/// The thumb pictures of the front end's scroll bars (`*_TR1`) are 15
/// pixels square.
pub const THUMB: f32 = 15.0;

/// A scroll bar's track (Ourlib's _strack.cpp): `count` positions along
/// `rect` (the track picture 4 pixels in on each side), the thumb at `pos`.
#[derive(Debug, Clone, Copy)]
pub struct Track {
    pub rect: Rect,
    pub vertical: bool,
    pub count: usize,
    pub pos: usize,
    pub target: Scroller,
}

impl Track {
    /// The track whose picture's top left is at (`x`, `y`), `length`
    /// pixels long along its axis.
    pub fn at(
        x: f32,
        y: f32,
        length: f32,
        vertical: bool,
        count: usize,
        pos: usize,
        target: Scroller,
    ) -> Self {
        let rect = if vertical {
            (x + 4.0, y + 4.0, THUMB, length - 8.0)
        } else {
            (x + 4.0, y + 4.0, length - 8.0, THUMB)
        };
        Track {
            rect,
            vertical,
            count,
            pos,
            target,
        }
    }

    fn length(&self) -> f32 {
        if self.vertical {
            self.rect.3
        } else {
            self.rect.2
        }
    }

    /// Where its thumb's top left goes (0x552400).
    pub fn thumb(&self) -> (f32, f32) {
        let travel = (self.length() - THUMB) as i32;
        let off = crate::panels::thumb_offset(self.pos as u32, self.count as u32, travel) as f32;
        if self.vertical {
            (self.rect.0, self.rect.1 + off)
        } else {
            (self.rect.0 + off, self.rect.1)
        }
    }

    /// The position under 800x600 point `p` (0x5524a0): the ends within
    /// half a thumb of them.
    pub fn pos_at(&self, p: (f32, f32)) -> usize {
        let along = if self.vertical {
            p.1 - self.rect.1
        } else {
            p.0 - self.rect.0
        };
        crate::panels::thumb_position(
            along.floor() as i32,
            self.count as u32,
            self.length() as i32,
            THUMB as i32,
        ) as usize
    }
}

/// The positions of a scroll bar over `total` lines (or cells) shown
/// `visible` at a time: one for each first line that leaves none of the
/// window empty (the original's viewer and edit, 0x54c320, 0x54ee70);
/// none when everything fits.
pub fn scroll_positions(total: usize, visible: usize) -> usize {
    if total > visible {
        total - visible + 1
    } else {
        0
    }
}

/// A scroll bar's position a step back (`d` < 0) or on from `pos`, of
/// `count`, staying put at the ends (_sslider.cpp, messages 0x2f, 0x30).
pub fn step_position(pos: usize, count: usize, d: i32) -> usize {
    if d < 0 {
        pos.saturating_sub(1)
    } else if pos + 1 < count {
        pos + 1
    } else {
        pos
    }
}

/// The first row shown of a list of `n` rows shown `rows` at a time,
/// asked to start at `first`: never past the last full window, so that
/// what is drawn and what is clicked agree.
pub fn list_window(first: usize, n: usize, rows: usize) -> usize {
    first.min(n.saturating_sub(rows))
}

/// The first row shown once row `cursor` is brought into view, the window
/// moved no further than it must (Ourlib's _slist.cpp, 0x54cd00).
pub fn follow(first: usize, cursor: usize, rows: usize) -> usize {
    if cursor < first {
        cursor
    } else if rows > 0 && cursor >= first + rows {
        cursor + 1 - rows
    } else {
        first
    }
}

/// How many rows of the plain list fit in box `rect`.
pub fn list_rows(rect: Rect) -> usize {
    ((rect.3 - 8.0) / ROW).floor().max(0.0) as usize
}

/// How many lines of a story fit in its box `rect`, `pitch` apart, the
/// first a line down (see `MenuView::story`).
pub fn story_rows(rect: Rect, pitch: f32) -> usize {
    ((rect.3 - pitch) / pitch).floor().max(0.0) as usize
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
    /// The loading screen (`<theme>LOADING`, 800 x 600, its own palette),
    /// shown while a game starts (0x50f910).
    pub loading: Option<Image>,
    /// Each backdrop's palette, as colours (the screen's 256 colours).
    palettes: Vec<Option<Vec<u32>>>,
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
    /// Tabs, narrow and wide, closed and open, by backdrop.
    tabs: Vec<[[Option<Image>; 2]; 2]>,
    /// Widget pictures by name, in the front end's palette.
    pictures: std::collections::HashMap<String, Image>,
    /// Fonts by style.
    styles: std::collections::HashMap<Style, TextRenderer>,
}

/// The widget pictures the screens use (theme-prefixed in controlm.dkx).
const PICTURES: &[&str] = &[
    "RAM_BEST",
    "MP_MAP",
    "MP_DESC",
    "MP_LFILE0",
    "MP_LFILE1",
    "SP_LPLAY0",
    "SP_LPLAY1",
    "MP_LPLAY0",
    "MP_LPLAY1",
    "MAP_TR_V0",
    "MAP_TR_V1",
    "MAP_TR_H0",
    "MAP_TR_H1",
    "DESC_TR0",
    "DESC_TR1",
    "LFILE_TR0",
    "LFILE_TR1",
    "SLPLAY_TR0",
    "SLPLAY_TR1",
    "LPLAY_TR0",
    "LPLAY_TR1",
    "LPLAY_FL",
    "LPLAY_TP",
    "LPLAY_NP",
    "LPLAY_NPM",
    "M_CBUT0",
    "M_CBUT1",
    "M_CBUT2",
    "T_PLAYER3",
    "T_PLAYER5",
    "M_CHECK0",
    "M_CHECK1",
    "M_LBUT_UP0",
    "M_LBUT_UP1",
    "M_LBUT_UP2",
    "M_LBUT_DN0",
    "M_LBUT_DN1",
    "M_LBUT_DN2",
    "M_LBUT_LF0",
    "M_LBUT_LF1",
    "M_LBUT_LF2",
    "M_LBUT_RT0",
    "M_LBUT_RT1",
    "M_LBUT_RT2",
    "LOAD_RM0",
    "LOAD_RM1",
    "LOAD_TR0",
    "LOAD_TR1",
    "PROV_NAME",
    "PROV_RAM0",
    "PROV_RAM1",
    "PROV_TR0",
    "PROV_TR1",
    "NEW_BUT0",
    "NEW_BUT1",
    "MESS_MS",
    "MP_MESS",
    "MESS_TR0",
    "MESS_TR1",
    "M_LOADZ0",
    "M_LOADZ1",
    "M_LOADZ_BIG0",
    "M_LOADZ_BIG1",
    "M_SBUT_UP0",
    "M_SBUT_UP2",
    "M_SBUT_DN0",
    "M_SBUT_DN2",
];

/// The fonts the screens use.
const FONTS: [&str; 7] = ["LH", "BH", "BS", "MH", "MS", "SH", "SS"];

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
        let mut pictures = std::collections::HashMap::new();
        for name in PICTURES {
            if let Some(img) = sprite(name, &None) {
                pictures.insert(name.to_string(), img);
            }
        }
        // The players' flags have no theme prefix.
        for i in 0..160 {
            let name = format!("M_FLAGS{i:03}");
            if let Some(palette) = main_palette.as_ref() {
                if let Some(e) = archive.find(&name, ResourceKind::Sprite) {
                    if let Ok(s) = Sprite::parse(&archive.read_entry(e)?) {
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
                        pictures.insert(name, Image::new(s.width, s.height, pixels));
                    }
                }
            }
        }
        let mut styles = std::collections::HashMap::new();
        for f in FONTS {
            for variant in 0..4 {
                if let Ok(r) = font(&format!("S_FNT_{f}"), variant) {
                    for spacing in 0..4 {
                        styles.insert(
                            Style {
                                font: f,
                                variant,
                                spacing,
                            },
                            r.clone().with_spacing(spacing),
                        );
                    }
                }
            }
        }
        let loading = archive
            .read(&format!("{theme}LOADING"), ResourceKind::Bitmap)
            .ok()
            .and_then(|d| Bitmap::parse(&d).ok())
            .map(|b| {
                let pixels = b
                    .pixels
                    .iter()
                    .map(|&i| Some(rgb(b.palette.0[i as usize])))
                    .collect();
                Image::new(b.width, b.height, pixels)
            });
        Ok(Self {
            loading,
            palettes: backdrops
                .iter()
                .map(|b| {
                    b.as_ref()
                        .map(|b| b.palette.0.iter().map(|&c| rgb(c)).collect())
                })
                .collect(),
            pictures,
            styles,
            shapes,
            checks: backdrops
                .iter()
                .map(|bg| [sprite("M_CHECK0", bg), sprite("M_CHECK1", bg)])
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
        for p in view.sprites {
            if let Some(img) = self.pictures.get(&p.name) {
                let (x, y) = at(p.x, p.y);
                blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
            }
        }
        if let Some((img, (bx, by, bw, bh), (sx, sy))) = view.map {
            let w = (bw as u32).min(img.width.saturating_sub(sx));
            let h = (bh as u32).min(img.height.saturating_sub(sy));
            let (x, y) = at(bx, by);
            blit_scaled(frame, img, (sx, sy, w, h), x, y, s);
        }
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
            write(frame, vf, 10.0, vy, &version());
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
                // Rows the screen draws itself (the original's widgets).
                if view.row_height.is_some() {
                    continue;
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
                } else if view.hover.is_some_and(|p| {
                    row_at(list, view.row_height, p).map(|r| r + view.first) == Some(i)
                }) {
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
            if view.first > 0 && view.row_height.is_none() {
                write(
                    frame,
                    &self.text,
                    list.0 + list.2 - 70.0,
                    list.1 + 8.0,
                    "^ more",
                );
            }
            if items.len() > view.first + VISIBLE_ROWS && view.row_height.is_none() {
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
        if let Some((rect, paragraphs)) = view.story {
            // Wrapped to the box, a line apart, from the first line
            // scrolled to. The original draws the wrapped lines (0x5482b0)
            // from the box's top (0x547d20); the first shown here stands a
            // line down, where it was measured on the original's screens
            // (whether its wrapped text starts with an empty line is not
            // settled).
            let (font, pitch) = self.story_font_of(view.story_style);
            let lines = self.story_lines(view.story_style, rect.2, paragraphs);
            let rows = story_rows(rect, pitch);
            for (k, l) in lines.iter().skip(view.story_first).take(rows).enumerate() {
                write(frame, font, rect.0, rect.1 + pitch + k as f32 * pitch, l);
            }
        }
        self.write_texts(frame, view.texts, s, (ox, oy));
        for &((x, y, w, h), colour) in view.fills {
            let (x0, y0) = at(x, y);
            let (x1, y1) = at(x + w, y + h);
            for py in (y0.round() as i64).max(0)..(y1.round() as i64).min(frame.height as i64) {
                for px in (x0.round() as i64).max(0)..(x1.round() as i64).min(frame.width as i64) {
                    frame.pixels[(py as u32 * frame.width + px as u32) as usize] = colour;
                }
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
        // A button's looks by its state (_sbutton.cpp, captions by
        // 0x4ea470): normal, picture 0 and the caption in variant 2; under
        // the pointer, picture 1, the caption in variant 3 two up and left
        // of its shadow in variant 0; held down inside, picture 1 and the
        // caption in variant 3 where it stands, without a shadow.
        for (i, b) in view.buttons.iter().enumerate() {
            let (w, h) = self.button_size(b);
            let pressed = view.pressed == Some(i);
            let under = view.hover.is_some_and(|p| inside(p, (b.x, b.y, w, h)));
            let lit = pressed || under;
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
            if lit && !pressed {
                write(
                    frame,
                    &self.lit_shadow,
                    b.x + ((w - tw) / 2.0).floor(),
                    b.y + ((h - th) / 2.0).floor(),
                    &b.caption,
                );
            }
            // A lit caption rises out of its shadow.
            let lift = if lit && !pressed { LIT_LIFT } else { 0.0 };
            write(
                frame,
                font,
                b.x + ((w - tw) / 2.0).floor() - lift,
                b.y + ((h - th) / 2.0).floor() - lift,
                &b.caption,
            );
        }
        let (sprites, texts) = view.top;
        for p in sprites {
            if let Some(img) = self.pictures.get(&p.name) {
                let (x, y) = at(p.x, p.y);
                blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
            }
        }
        self.write_texts(frame, texts, s, (ox, oy));
    }

    /// Texts in their styles, each cut at its box's right when it has one.
    fn write_texts(&self, frame: &mut Frame, texts: &[Text], s: f32, (ox, oy): (f32, f32)) {
        let ts = s.round().max(1.0) as u32;
        for t in texts {
            if let Some(f) = self.styles.get(&t.style) {
                let (px, py) = (ox + t.x * s, oy + t.y * s);
                match t.clip {
                    Some(w) => {
                        let right = (ox + (t.x + w) * s).round() as i32;
                        f.draw_clipped(frame, px as i32, py as i32, ts, &t.text, right);
                    }
                    None => f.draw(frame, px as i32, py as i32, ts, &t.text),
                }
            }
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

    /// The font and line pitch of a story in style `style` (the
    /// briefing's when none).
    fn story_font_of(&self, style: Option<(Style, f32)>) -> (&TextRenderer, f32) {
        style
            .and_then(|(st, l)| Some((self.styles.get(&st)?, l)))
            .unwrap_or((&self.story_font, STORY_LINE))
    }

    /// A story's paragraphs wrapped at spaces to `width` (empty words keep
    /// double spaces), each paragraph from a new line, empty ones blank
    /// lines; blank lines at the end left out.
    pub fn story_lines(
        &self,
        style: Option<(Style, f32)>,
        width: f32,
        paragraphs: &[String],
    ) -> Vec<String> {
        let (font, _) = self.story_font_of(style);
        let width = width as u32;
        let mut lines = Vec::new();
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
                        lines.push(l);
                        line = Some(word.to_string());
                    }
                    _ => line = Some(candidate),
                }
            }
            lines.push(line.unwrap_or_default());
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        lines
    }

    /// Colour `index` of the palette of screen `backdrop`.
    pub fn colour(&self, backdrop: Backdrop, index: u8) -> Option<u32> {
        let palette = self.palettes.get(backdrop as usize)?.as_ref()?;
        palette.get(usize::from(index)).copied()
    }

    /// Height of `text` in style `style` (its tallest glyph's, 0x547350).
    pub fn height(&self, style: Style, text: &str) -> f32 {
        self.styles
            .get(&style)
            .map_or(0.0, |f| f.height(text) as f32)
    }

    /// Height of style `style`'s tallest glyph (0x5471f0).
    pub fn tallest(&self, style: Style) -> f32 {
        self.styles.get(&style).map_or(0.0, |f| f.tallest() as f32)
    }

    /// Width of `text` in style `style`, in screen pixels.
    pub fn measure(&self, style: Style, text: &str) -> f32 {
        self.styles
            .get(&style)
            .map_or(0.0, |f| f.measure(text) as f32)
    }

    /// What lies at 800x600 point `p`.
    pub fn hit(&self, view: &MenuView, p: (f32, f32)) -> Option<MenuHit> {
        for &(r, hit) in view.spots {
            if inside(p, r) {
                return Some(hit);
            }
        }
        for t in view.tracks {
            if t.count > 0 && inside(p, t.rect) {
                return Some(MenuHit::Track(t.target, t.pos_at(p)));
            }
        }
        // A choice's item is its M_CHECK picture alone, 23 x 23 (the
        // briefing's group, 0x53f360); its caption is not.
        for (i, c) in view.checks.iter().enumerate() {
            if inside(p, (c.x, c.y, 23.0, 23.0)) {
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
        let (items, _) = view.list?;
        row_at(view.list_rect.unwrap_or(LIST), view.row_height, p)
            .map(|r| r + view.first)
            .filter(|&i| i < items.len())
            .map(MenuHit::Item)
    }
}

/// The row of list `list` at point `p`, if any.
fn row_at(list: Rect, row: Option<f32>, p: (f32, f32)) -> Option<usize> {
    if !inside(p, list) {
        return None;
    }
    // Rows of the given height from the box's top, or the plain box's.
    let (top, row) = match row {
        Some(r) => (list.1, r),
        None => (list.1 + 8.0, ROW),
    };
    let i = ((p.1 - top) / row).floor();
    (i >= 0.0 && top + (i + 1.0) * row <= list.1 + list.3).then_some(i as usize)
}

/// The frame of a chosen row on the port's own lists (palette entry 22 of
/// the front end's backdrops).
const PANEL_RED: u32 = 0x9c4231;

fn inside(p: (f32, f32), (x, y, w, h): (f32, f32, f32, f32)) -> bool {
    p.0 >= x && p.1 >= y && p.0 < x + w && p.1 < y + h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The connect screen's name caret takes palette colour 0xfb
    /// (0x538440), and its fonts' tallest glyphs are as the end screen's
    /// rows assume (needs `OPENCONQUEST_DATA`).
    #[test]
    fn the_front_end_palette_and_fonts() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let menus = Menus::load(&data, '0').unwrap();
        assert_eq!(menus.colour(Backdrop::TitleDim, 0xfb), Some(0xffff00));
        let bh = Style {
            font: "BH",
            variant: 2,
            spacing: 2,
        };
        assert_eq!(menus.tallest(bh), 23.0);
    }

    #[test]
    fn a_bar_has_a_position_for_each_full_window() {
        // A 24-cell map in the 19-cell viewer: first cells 0 to 5.
        assert_eq!(scroll_positions(24, 19), 6);
        assert_eq!(scroll_positions(19, 19), 0);
        assert_eq!(scroll_positions(3, 7), 0);
    }

    #[test]
    fn arrows_step_one_position_and_stop_at_the_ends() {
        assert_eq!(step_position(0, 6, -1), 0);
        assert_eq!(step_position(3, 6, -1), 2);
        assert_eq!(step_position(3, 6, 1), 4);
        assert_eq!(step_position(5, 6, 1), 5);
        assert_eq!(step_position(0, 0, 1), 0);
    }

    #[test]
    fn the_window_never_starts_past_the_last_full_one() {
        assert_eq!(list_window(12, 14, 9), 5);
        assert_eq!(list_window(3, 14, 9), 3);
        assert_eq!(list_window(4, 5, 9), 0);
    }

    #[test]
    fn the_window_follows_the_cursor_no_further_than_it_must() {
        // Down past the bottom row: the cursor becomes the bottom row.
        assert_eq!(follow(0, 9, 9), 1);
        assert_eq!(follow(0, 13, 9), 5);
        // Up past the top: the cursor becomes the top row.
        assert_eq!(follow(5, 2, 9), 2);
        // Already in view: the window stays.
        assert_eq!(follow(5, 9, 9), 5);
    }

    #[test]
    fn a_click_picks_the_row_drawn_there_after_scrolling() {
        // The Load screen's list: rows 27 high from y 248, 10 shown, the
        // window asked to start past its last full one (as the arrows and
        // the wheel once did): drawn and clicked from the same row.
        let rect = (12.0, 248.0, 750.0, 27.0 * 10.0);
        let first = list_window(13, 14, 10);
        let row = row_at(rect, Some(27.0), (300.0, 248.0 + 27.0 * 3.5)).unwrap();
        assert_eq!(row + first, 7);
        // The plain list's rows.
        assert_eq!(list_rows(LIST), VISIBLE_ROWS);
    }

    #[test]
    fn a_track_maps_points_and_positions_as_the_original() {
        // The description's bar: DESC_TR0 (107 high) at (766, 76), its
        // track 99 long, the thumb 15, so 84 pixels of travel.
        let t = Track::at(766.0, 76.0, 107.0, true, 6, 0, Scroller::Story);
        assert_eq!(t.rect, (770.0, 80.0, 15.0, 99.0));
        assert_eq!(t.thumb(), (770.0, 80.0));
        let last = Track { pos: 5, ..t };
        assert_eq!(last.thumb(), (770.0, 164.0));
        // Middle positions sit in the middle of their share (0x552400).
        let mid = Track { pos: 2, ..t };
        assert_eq!(mid.thumb(), (770.0, 80.0 + (84.0f32 / 6.0 * 2.5).floor()));
        // Within half a thumb of an end: that end (0x5524a0).
        assert_eq!(t.pos_at((775.0, 87.0)), 0);
        assert_eq!(t.pos_at((775.0, 80.0 + 84.0 + 7.0)), 5);
        assert_eq!(t.pos_at((775.0, 80.0 + 7.0 + 42.0)), 3);
        // Across: the map's bar.
        let h = Track::at(32.0, 185.0, 107.0, false, 6, 0, Scroller::MapX);
        assert_eq!(h.rect, (36.0, 189.0, 99.0, 15.0));
        assert_eq!(h.pos_at((36.0 + 7.0 + 42.0, 190.0)), 3);
    }

    /// The installed copy's version line; a fade changes every entry of the
    /// front end's palettes, the Windows colours at 0-9 and 246-255 too
    /// (needs `OPENCONQUEST_DATA`).
    #[test]
    fn the_version_and_the_fades_palette() {
        assert_eq!(version(), "Version 2.30");
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        assert_eq!(data.strings.get(&1145).unwrap(), "Version 2.%02d");
        let menus = Menus::load(&data, '0').unwrap();
        let colour = |i| menus.colour(Backdrop::Best, i).unwrap();
        assert_eq!(colour(7), 0xc0c0c0);
        assert_eq!(colour(251), 0xffff00);
        let shade = crate::fade::Shade::new(Some(50));
        assert_eq!(shade.apply(0xc0c0c0), crate::fade::lightness(0xc0c0c0, 50));
        assert!(menus.loading.is_some());
        assert_eq!(
            shade.apply(colour(100)),
            crate::fade::lightness(colour(100), 50)
        );
    }
}
