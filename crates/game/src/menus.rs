//! The front end: main menu, mission lists, briefing and end screens.
//!
//! The art is the original's (`controlm.dkx`: `M_BKGND_00`-`05`, the
//! `M_BUT*` button shapes and the `S_FNT_*` fonts); captions and the
//! placement of buttons are this port's own. Screens are 800x600, scaled to
//! the window's height and centred.

use anyhow::{Context, Result};
use openac_formats::{Bitmap, ResourceKind};

use crate::assets::{rgb, GameData, Image};
use crate::render::{blit_scaled, shade_rect, Frame};
use crate::text::TextRenderer;

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
    pub wide: bool,
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
}

/// What a click hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuHit {
    Button(usize),
    Item(usize),
}

// The list and text boxes.
const LIST: (f32, f32, f32, f32) = (50.0, 110.0, 470.0, 410.0);
const ROW: f32 = 26.0;
/// List rows that fit.
pub const VISIBLE_ROWS: usize = ((LIST.3 - 8.0) / ROW) as usize;
const TEXT_BOX: (f32, f32, f32, f32) = (40.0, 90.0, 520.0, 420.0);

pub struct Menus {
    backdrops: Vec<Option<Image>>,
    /// Wide and narrow button shapes, normal and lit.
    wide: [Option<Image>; 2],
    narrow: [Option<Image>; 2],
    pub big: TextRenderer,
    pub text: TextRenderer,
}

impl Menus {
    pub fn load(data: &GameData, theme: char) -> Result<Self> {
        let archive = data
            .menus
            .as_ref()
            .context("controlm.dkx is not installed")?;
        let bitmap = |name: &str| -> Option<Image> {
            let data = archive
                .read(&format!("{theme}{name}"), ResourceKind::Bitmap)
                .ok()?;
            let b = Bitmap::parse(&data).ok()?;
            let pixels = b
                .pixels
                .iter()
                .map(|&i| Some(rgb(b.palette.0[i as usize])))
                .collect();
            Some(Image::new(b.width, b.height, pixels))
        };
        let sprite = |name: &str| {
            data.load_image(archive, &format!("{theme}{name}"), false)
                .ok()
                .flatten()
        };
        let font = |name: &str| TextRenderer::load(archive, &format!("{theme}{name}"), 0);
        Ok(Self {
            backdrops: (0..6).map(|i| bitmap(&format!("M_BKGND_{i:02}"))).collect(),
            wide: [sprite("M_BUT0_00"), sprite("M_BUT0_01")],
            narrow: [sprite("M_BUT2_00"), sprite("M_BUT2_01")],
            big: font("S_FNT_BH")?,
            text: font("S_FNT_MH")?,
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
        let img = if b.wide {
            &self.wide[0]
        } else {
            &self.narrow[0]
        };
        img.as_ref()
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
        if let Some(title) = view.title {
            write(frame, &self.big, LIST.0 + 12.0, LIST.1 - 30.0, title);
        }
        if let Some((items, selected)) = view.list {
            shade(frame, LIST);
            for (i, item) in items.iter().enumerate().skip(view.first) {
                let y = LIST.1 + 8.0 + (i - view.first) as f32 * ROW;
                if y + ROW > LIST.1 + LIST.3 {
                    break;
                }
                // The chosen entry is marked; the one under the cursor too.
                let mark = if selected == Some(i) {
                    "> "
                } else if view
                    .hover
                    .is_some_and(|p| row_at(p).map(|r| r + view.first) == Some(i))
                {
                    "- "
                } else {
                    "  "
                };
                write(
                    frame,
                    &self.text,
                    LIST.0 + 12.0,
                    y,
                    &format!("{mark}{item}"),
                );
            }
            // More above or below: the wheel scrolls.
            if view.first > 0 {
                write(
                    frame,
                    &self.text,
                    LIST.0 + LIST.2 - 70.0,
                    LIST.1 + 8.0,
                    "^ more",
                );
            }
            if items.len() > view.first + VISIBLE_ROWS {
                let y = LIST.1 + LIST.3 - ROW;
                write(frame, &self.text, LIST.0 + LIST.2 - 70.0, y, "v more");
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
        for b in view.buttons {
            let (w, h) = self.button_size(b);
            let lit = view.hover.is_some_and(|p| inside(p, (b.x, b.y, w, h)));
            let shapes = if b.wide { &self.wide } else { &self.narrow };
            if let Some(img) = &shapes[usize::from(lit)] {
                let (x, y) = at(b.x, b.y);
                blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
            }
            let font = &self.big;
            let tw = font.measure(&b.caption) as f32 * ts as f32 / s;
            let th = font.line_height() as f32 * ts as f32 / s;
            write(
                frame,
                font,
                b.x + (w - tw) / 2.0,
                b.y + (h - th) / 2.0,
                &b.caption,
            );
        }
    }

    /// What lies at 800x600 point `p`.
    pub fn hit(&self, view: &MenuView, p: (f32, f32)) -> Option<MenuHit> {
        for (i, b) in view.buttons.iter().enumerate() {
            let (w, h) = self.button_size(b);
            if inside(p, (b.x, b.y, w, h)) {
                return Some(MenuHit::Button(i));
            }
        }
        let (items, _) = view.list?;
        row_at(p)
            .map(|r| r + view.first)
            .filter(|&i| i < items.len())
            .map(MenuHit::Item)
    }
}

/// The list row at point `p`, if any.
fn row_at(p: (f32, f32)) -> Option<usize> {
    if !inside(p, LIST) {
        return None;
    }
    let i = ((p.1 - LIST.1 - 8.0) / ROW).floor();
    (i >= 0.0 && LIST.1 + 8.0 + (i + 1.0) * ROW <= LIST.1 + LIST.3).then_some(i as usize)
}

fn inside(p: (f32, f32), (x, y, w, h): (f32, f32, f32, f32)) -> bool {
    p.0 >= x && p.1 >= y && p.0 < x + w && p.1 < y + h
}
