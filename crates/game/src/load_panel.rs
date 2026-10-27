//! The load panel (`c_load.cpp`, 0x4f7f40-0x4fa140): while the selected
//! ship or building reloads (`scene::load`), it takes the object panel's
//! and the command buttons' place (0x4fffe0 creates panel 0x22e instead of
//! 0x22c and 0x22d when the selection answers message 0xc45a). Its own
//! holder's list fills the top (`COBJ_BKMAIN` at bar (3, 206)), its
//! partner's the bottom (`CLOAD_BKMAIN` at bar (3, 400)), with Kill item
//! and Finish reload between them at y 365.
//!
//! Positions are the bar's original pixels (0x4f8670, 0x4f8ee0, 0x4f9af0,
//! 0x4f7f40).

use std::rc::Rc;

use openconquest_formats::{Bitmap, ResourceKind};

use crate::assets::{rgb, GameData, Image};
use crate::scene::{Cargo, Entry, Scene};
use crate::text::TextRenderer;

/// Where each side's cells start: y 210 on top, 194 lower (0xc2) below.
pub const SIDE_Y: [u32; 2] = [210, 404];
/// The two backgrounds' tops.
pub const PANEL_Y: [u32; 2] = [206, 400];
pub const PANEL_X: u32 = 3;
/// Kill item and Finish reload (100x34), at bar x 4 and 105, y 365.
pub const KILL: (u32, u32) = (4, 365);
pub const END: (u32, u32) = (105, 365);
pub const BUTTON: (u32, u32) = (100, 34);
/// A cell (`COBJ_BKSMSLOT`).
pub const CELL: (u32, u32) = (47, 50);
/// The scrolled list's background (`COBJ_BKGSCROLL`/`CLOAD_BKGSCROLL`)
/// at panel x 165, y 6 on top and 4 below; the slider: arrows (22x22) at
/// bar x 172, side y + 6 and + 124, its track (30x96) at x 168, y + 28.
pub const SCROLL_BACK: (u32, [u32; 2]) = (165, [6, 4]);
pub const ARROW_X: u32 = 172;
pub const ARROW_Y: [u32; 2] = [6, 124];
pub const TRACK: (u32, u32) = (168, 28);
pub const TRACK_SIZE: (u32, u32) = (30, 96);
pub const ARROW: u32 = 22;
/// Entries shown at once in a scrolled list: three rows of two.
pub const WINDOW: usize = 6;

/// The panel's own state (panel +0x68/+0x69 the chosen side and entry,
/// +0x3c an opened group per side, +0xec the scroll).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LoadView {
    /// The selected object it belongs to.
    pub owner: u32,
    pub open: [Option<(u8, u8)>; 2],
    pub scroll: [usize; 2],
    pub chosen: (usize, usize),
}

/// What a click on the panel hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Cell(usize, usize),
    Kill,
    End,
    Up(usize),
    Down(usize),
    /// Elsewhere on the panel.
    Panel,
}

/// The panel's pictures.
pub struct LoadArt {
    pub bottom: Option<Image>,
    pub end: Vec<Image>,
    pub kill: Vec<Image>,
    /// The scrolled list's background, top and bottom.
    pub scroll_back: [Option<Image>; 2],
    /// The slider's track, top and bottom.
    pub track: [Option<Image>; 2],
    pub up: Vec<Image>,
    pub down: Vec<Image>,
    pub thumb: Option<Image>,
    /// The buttons' captions (`INT_FONT`).
    pub font: Option<TextRenderer>,
    pub active: Option<Rc<Image>>,
}

impl LoadArt {
    pub fn load(data: &GameData, theme: char) -> Self {
        let bitmap = |name: &str| -> Option<Image> {
            let b = Bitmap::parse(&data.controls.read(name, ResourceKind::Bitmap).ok()?).ok()?;
            let pixels = b
                .pixels
                .iter()
                .map(|&i| Some(rgb(b.palette.0[i as usize])))
                .collect();
            Some(Image::new(b.width, b.height, pixels))
        };
        let themed = |name: &str| bitmap(&format!("{theme}{name}"));
        let frames = |base: &str| -> Vec<Image> {
            (0..)
                .map_while(|i| themed(&format!("{base}{i:02}")))
                .collect()
        };
        Self {
            bottom: themed("CLOAD_BKMAIN"),
            end: frames("CLOAD_END"),
            kill: frames("CLOAD_KILL"),
            scroll_back: [themed("COBJ_BKGSCROLL"), themed("CLOAD_BKGSCROLL")],
            track: [themed("COBJ_BKSCROLL"), themed("CLOAD_BKSCROLL")],
            up: frames("SLIDER_UP_"),
            down: frames("SLIDER_DN_"),
            thumb: data
                .load_image(&data.controls, &format!("{theme}SLIDER_BB"), false)
                .ok()
                .flatten(),
            font: TextRenderer::load_in(
                &data.controls,
                &format!("{theme}INT_FONT"),
                0,
                Some(&data.palette),
            )
            .ok()
            .map(|f| f.with_spacing(0)),
            active: data
                .load_image(&data.controls, "COBJ_SACT", false)
                .ok()
                .flatten()
                .map(Rc::new),
        }
    }
}

/// The load panel of the selected object `selected`, if it is reloading:
/// the two holders and their lists, with `view`'s groups opened.
pub struct Lists {
    pub holders: [u32; 2],
    pub entries: [Vec<Entry>; 2],
}

pub fn lists(scene: &Scene, selected: u32, view: &LoadView) -> Option<Lists> {
    let i = scene.object_index(selected)?;
    if !scene.is_own(&scene.objects[i]) {
        return None;
    }
    let holders = scene.load_sides(selected)?;
    let entries = [0, 1].map(|s| scene.load_list(holders[s], view.open[s]));
    Some(Lists { holders, entries })
}

/// Whether a side's list is an opened group (its header first).
fn opened(list: &[Entry]) -> bool {
    list.first()
        .is_some_and(|e| e.flags & crate::scene::load::HEADER != 0)
}

/// The cells of `side`'s list: entry index and bar position (0x4f8ee0).
/// Closed: entries in the slot grid's places, four to a row 48 apart from
/// x 9, rows 51 apart, the first place left for the portrait. Opened: the
/// header at the bottom-left place, the rest three to a row from x 57; or
/// with ten and more, a window of six, two to a row 53 apart from x 62,
/// that the slider scrolls.
pub fn cells(list: &[Entry], side: usize, scroll: usize) -> Vec<(usize, u32, u32)> {
    let y0 = SIDE_Y[side];
    let mut out = Vec::new();
    if !opened(list) {
        for (i, e) in list.iter().enumerate().take(11) {
            if e.flags == 0 {
                continue;
            }
            let n = i as u32 + 1;
            out.push((i, 9 + n % 4 * 48, y0 + n / 4 * 51));
        }
        return out;
    }
    out.push((0, 9, y0 + 102));
    let n = list.len() - 1;
    if n < 10 {
        for j in 0..n {
            out.push((j + 1, 57 + j as u32 % 3 * 48, y0 + j as u32 / 3 * 51));
        }
    } else {
        let first = scroll.min(max_scroll(n));
        for j in first..(first + WINDOW).min(n) {
            let k = (j - first) as u32;
            out.push((j + 1, 62 + k % 2 * 53, y0 + k / 2 * 51));
        }
    }
    out
}

/// Whether `side`'s list scrolls (an opened group of ten and more).
pub fn scrolls(list: &[Entry]) -> bool {
    opened(list) && list.len() > 10
}

/// The furthest a list of `n` entries scrolls, in entries (two a row,
/// three rows shown; 0x4f9af0).
pub fn max_scroll(n: usize) -> usize {
    n.div_ceil(2).saturating_sub(3) * 2
}

/// What lies at bar point `(x, y)` (original pixels).
pub fn hit(lists: &Lists, view: &LoadView, x: f32, y: f32) -> Option<Hit> {
    let inside = |(rx, ry): (u32, u32), (w, h): (u32, u32)| {
        x >= rx as f32 && y >= ry as f32 && x < (rx + w) as f32 && y < (ry + h) as f32
    };
    if inside(KILL, BUTTON) {
        return Some(Hit::Kill);
    }
    if inside(END, BUTTON) {
        return Some(Hit::End);
    }
    for (side, (list, y0)) in lists.entries.iter().zip(SIDE_Y).enumerate() {
        for (i, cx, cy) in cells(list, side, view.scroll[side]) {
            if inside((cx, cy), CELL) {
                return Some(Hit::Cell(side, i));
            }
        }
        if scrolls(list) {
            if inside((ARROW_X, y0 + ARROW_Y[0]), (ARROW, ARROW)) {
                return Some(Hit::Up(side));
            }
            if inside((ARROW_X, y0 + ARROW_Y[1]), (ARROW, ARROW)) {
                return Some(Hit::Down(side));
            }
        }
    }
    let panel = (0, PANEL_Y[0]);
    inside(panel, (208, 600 - PANEL_Y[0] - 42)).then_some(Hit::Panel)
}

/// Whether Kill item may be pressed: something on the chosen side can be
/// thrown away (0x4fa0b0).
pub fn can_kill(lists: &Lists, view: &LoadView) -> bool {
    lists.entries[view.chosen.0]
        .iter()
        .any(|e| e.cargo.is_some() && e.is_choosable())
}

/// What the chosen entry carries.
pub fn chosen_cargo(lists: &Lists, view: &LoadView) -> Option<(u32, Cargo)> {
    let (side, i) = view.chosen;
    let e = lists.entries[side].get(i)?;
    Some((lists.holders[side], e.cargo?))
}
