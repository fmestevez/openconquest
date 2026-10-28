//! The object panel's upper part (`c_info.cpp`, `CObjInfoTy`, 0x4f5130-
//! 0x4f7b9e): with one object selected its portrait and slot cells; a
//! slot held down shows what is in it on `COBJ_BKBIGSLOT`; with several
//! selected a button for each (the group panel, `CGRP_*`).
//!
//! The panel asks the selection for its list with message 0x109f
//! (ProcOnCreate 0x4f5230). A group (the flotilla, nik_Flotil 0x49f9e0)
//! answers with more than one member: entries of 0x10 bytes (class, the
//! portrait set, the type, life in percent and its maximum 100, id), and
//! the panel draws `CGRP_BKG` instead of `COBJ_BKMAIN` (PaintInfoObj
//! 0x4f5800) and no strip (c_ctrl 0x4f06f0 checks the panel's group flag).
//!
//! Positions are the bar's original pixels (0x511170 adds the bar's x).

use openconquest_formats::{Bitmap, ObjectClass, ResourceKind};

use crate::assets::{rgb, GameData, Image};
use crate::combat::Levels;
use crate::objects::{MapObject, Treasure};
use crate::render::{blit_scaled, Frame};
use crate::scene::load::{
    treasure_picture, ARMOUR, EMPTY, GOODS, GROUP, HEADER, HERO, PRESENT, RAM, TREASURE, UNIT,
};
use crate::scene::{Cargo, Entry, Scene};
use crate::text::TextRenderer;

/// The portrait's place, clicked to centre the view on the object
/// (GetMessage 0x4f79e0, message 0x61: x 9 to 56, y 210 to 260; 0xc450,
/// 0xc459 and 0xcbb to the field).
const PORTRAIT: (u32, u32, u32, u32) = (9, 210, 48, 51);
/// A slot cell (`COBJ_BKSMSLOT`).
const CELL: (u32, u32) = (47, 50);
/// The slot held down grows to `COBJ_BKBIGSLOT` (191x152) over the grid
/// (DoInfoBut 0x4f72d0: rectangle 9, 210, 191, 152).
pub const BIG_SLOT: (u32, u32) = (9, 210);
/// Its picture at (3, 7) (PaintObjBut 0x4f69e0); the text in `BSLT_FONT`
/// in the rectangle (15, 12, 80, 120), lines centred one by one from its
/// top, wrapped to 80 pixels.
const BIG_PICTURE: (i32, i32) = (3, 7);
const BIG_TEXT: (i32, i32, u32, u32) = (15, 12, 80, 120);
/// `BSLT_FONT`'s letter and line gaps (0x5470f0 with 1, -1).
const BIG_LETTERS: u32 = 1;
const BIG_LINES: i32 = -1;
/// Where wrapping may break (0x5bd568, after the character) and what
/// starts a new paragraph (0x5bd570).
const BREAKS: &[u8] = b" ,.;:!?";
const PARAGRAPH: char = '#';

/// Group buttons (ChangeButs 0x4f5f20): rows 47 apart from y 211, four to
/// a row 48 apart from x 10, three with the slider; a button is
/// `CGRP_BUTFRAME` (44x42) and 1, 2 more (CreateBut 0x4f64c0).
const GROUP_ORIGIN: (u32, u32) = (10, 211);
const GROUP_STEP: (u32, u32) = (48, 47);
const MEMBER: (u32, u32) = (45, 44);
/// With 17 members and more the group scrolls: 12 shown, the slider's
/// background `CGRP_BKGSCROLL` at (168, 213).
const GROUP_SCROLLS: usize = 17;
const GROUP_WINDOW: usize = 12;
const GROUP_SCROLL_BACK: (u32, u32) = (168, 213);
/// An opened list shows its slider from nine entries (CreateInfoButtons
/// 0x4f6477 and PaintInfoObj: `COBJ_BKGSCROLL` at panel (165, 6)).
const LIST_SCROLLS: usize = 9;

/// The sliders (CreateScrolling 0x4f76e0): arrows (22x22) at x 172, the
/// track (30 wide) at x 168; a group's arrows at y 217 and 369, its track
/// at y 239, 130 high (`CGRP_BKSCROLL`); a list's at 216, 334 and 238, 96
/// high (`COBJ_BKSCROLL`).
const ARROW_X: u32 = 172;
const ARROW: u32 = 22;
const TRACK_X: u32 = 168;
const GROUP_SLIDER: (u32, u32, u32, u32) = (217, 369, 239, 130);
const LIST_SLIDER: (u32, u32, u32, u32) = (216, 334, 238, 96);

/// The panel's state for the selected object or group.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct InfoView {
    /// Whose it is: the selected object and how many are selected.
    pub owner: (u32, usize),
    /// The group of a settlement's places opened (its key: category and
    /// byte; the panel's +0x3c).
    pub open: Option<(u8, u8)>,
    /// The first entry or member shown (+0x85).
    pub scroll: usize,
    /// The cell held down, shown big (+0x8a).
    pub big: Option<usize>,
    /// What the left button holds down.
    pub pressed: Option<Hit>,
}

/// What lies under the pointer on the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// A slot cell (its index in the list).
    Cell(usize),
    /// A group member's button (its index in the selection).
    Member(usize),
    Up,
    Down,
    Portrait,
}

/// What the panel shows.
pub enum Shown {
    /// Nothing selected: `COBJ_CLEAR` (ClearInfoObj 0x4f5c90).
    Clear,
    /// One object and its slot list.
    Single(u32, Vec<Entry>),
    /// Several: their ids.
    Group(Vec<u32>),
}

/// The panel for `selection` (the selected object first).
pub fn shown(scene: &Scene, selection: &[u32], view: &InfoView) -> Shown {
    let ids: Vec<u32> = selection
        .iter()
        .copied()
        .filter(|&id| scene.object_index(id).is_some())
        .collect();
    match ids.as_slice() {
        [] => Shown::Clear,
        [id] => {
            let obj = &scene.objects[scene.object_index(*id).unwrap()];
            Shown::Single(*id, entries(scene, obj, view.open))
        }
        _ => Shown::Group(ids),
    }
}

/// An object's slot list (message 0x109f): a ship's places as the load
/// panel lists them (0x4e5e40), the ship of the dead's marines as the
/// "live" ones (items 12-14); an own settlement's or town's places
/// (0x43bfe0 page 0: each item's place a group to open, with its count and
/// no bar) or the group opened, its header first (DoInfoBut: flags 0x12)
/// and then its units with their conditions, its armour and rams, or its
/// treasures and heroes.
pub fn entries(scene: &Scene, obj: &MapObject, open: Option<(u8, u8)>) -> Vec<Entry> {
    use crate::economy::{SETTLEMENT, TOWN};
    if let Some(ship) = &obj.ship {
        let mut list = scene.load_list(obj.id, None);
        if ship.kind == 11 {
            for e in list.iter_mut().filter(|e| e.cat == UNIT && e.sub < 3) {
                e.sub += 12;
            }
        }
        return list;
    }
    if !matches!(obj.class, ObjectClass::Building(SETTLEMENT | TOWN)) || !scene.is_own(obj) {
        return Vec::new();
    }
    match open {
        None => {
            let mut list = scene.load_list(obj.id, None);
            for e in list.iter_mut().filter(|e| e.cat == UNIT) {
                e.flags |= GROUP;
                e.bar = None;
                e.cargo = None;
            }
            list
        }
        Some((UNIT, item)) => {
            let units = scene
                .stock_units(obj.id)
                .get(item as usize)
                .cloned()
                .unwrap_or_default();
            let mut list = vec![Entry {
                cat: UNIT,
                sub: item,
                flags: HEADER | PRESENT,
                count: 0,
                bar: None,
                cargo: None,
            }];
            list.extend(units.iter().enumerate().map(|(k, &c)| Entry {
                cat: UNIT,
                sub: item,
                flags: PRESENT,
                count: 0,
                bar: Some(c),
                cargo: Some(Cargo::Unit(item, k as u8)),
            }));
            list
        }
        Some(key) => scene.load_list(obj.id, Some(key)),
    }
}

/// Whether an opened list (its header first) is long enough for the slider.
fn list_scrolls(list: &[Entry]) -> bool {
    list.first().is_some_and(|e| e.flags & HEADER != 0) && list.len() > LIST_SCROLLS
}

/// The furthest an opened list of `n` entries (besides the header)
/// scrolls: the slider has ceil(n / 2) - 2 positions, a row of two each.
fn list_max_scroll(n: usize) -> usize {
    n.div_ceil(2).saturating_sub(3) * 2
}

/// The furthest a group of `n` scrolls: ceil(n / 3) - 3 positions, a row
/// of three each (0x4f76e0, message 0x537).
pub fn group_max_scroll(n: usize) -> usize {
    n.div_ceil(3).saturating_sub(4) * 3
}

/// The slot cells shown: entry index and position. A closed list fills
/// the slot grid's places after the portrait (four to a row 48 apart from
/// x 9, rows 51 apart from y 210); an opened one as the load panel's
/// (`load_panel::cells`).
pub fn cells(list: &[Entry], scroll: usize) -> Vec<(usize, u32, u32)> {
    if list.first().is_some_and(|e| e.flags & HEADER != 0) {
        return crate::load_panel::cells(list, 0, scroll);
    }
    list.iter()
        .enumerate()
        .take(11)
        .filter(|(_, e)| e.flags != 0)
        .map(|(i, _)| {
            let n = i as u32 + 1;
            (i, 9 + n % 4 * 48, 210 + n / 4 * 51)
        })
        .collect()
}

/// The group buttons shown: member index and position.
pub fn group_cells(n: usize, scroll: usize) -> Vec<(usize, u32, u32)> {
    let scrolls = n >= GROUP_SCROLLS;
    let columns = if scrolls { 3 } else { 4 };
    let first = if scrolls {
        scroll.min(group_max_scroll(n))
    } else {
        0
    };
    let end = if scrolls && n - first >= GROUP_WINDOW {
        first + GROUP_WINDOW
    } else {
        n
    };
    (first..end)
        .map(|i| {
            let k = (i - first) as u32;
            (
                i,
                GROUP_ORIGIN.0 + k % columns * GROUP_STEP.0,
                GROUP_ORIGIN.1 + k / columns * GROUP_STEP.1,
            )
        })
        .collect()
}

/// What lies at bar point `(x, y)`.
pub fn hit(shown: &Shown, view: &InfoView, x: f32, y: f32) -> Option<Hit> {
    let inside = |(rx, ry): (u32, u32), (w, h): (u32, u32)| {
        x >= rx as f32 && y >= ry as f32 && x < (rx + w) as f32 && y < (ry + h) as f32
    };
    let arrows = |(up, down, _, _): (u32, u32, u32, u32)| {
        if inside((ARROW_X, up), (ARROW, ARROW)) {
            Some(Hit::Up)
        } else if inside((ARROW_X, down), (ARROW, ARROW)) {
            Some(Hit::Down)
        } else {
            None
        }
    };
    match shown {
        Shown::Clear => None,
        Shown::Group(ids) => {
            for (i, cx, cy) in group_cells(ids.len(), view.scroll) {
                if inside((cx, cy), MEMBER) {
                    return Some(Hit::Member(i));
                }
            }
            (ids.len() >= GROUP_SCROLLS)
                .then(|| arrows(GROUP_SLIDER))
                .flatten()
        }
        Shown::Single(_, list) => {
            for (i, cx, cy) in cells(list, view.scroll) {
                if inside((cx, cy), CELL) {
                    return Some(Hit::Cell(i));
                }
            }
            if list_scrolls(list) {
                if let Some(h) = arrows(LIST_SLIDER) {
                    return Some(h);
                }
            }
            let (px, py, pw, ph) = PORTRAIT;
            inside((px, py), (pw, ph)).then_some(Hit::Portrait)
        }
    }
}

/// `view` scrolled a step up or down.
pub fn step(shown: &Shown, view: &mut InfoView, down: bool) {
    let (row, max) = match shown {
        Shown::Group(ids) => (3, group_max_scroll(ids.len())),
        Shown::Single(_, list) => (2, list_max_scroll(list.len().saturating_sub(1))),
        Shown::Clear => return,
    };
    view.scroll = if down {
        (view.scroll + row).min(max)
    } else {
        view.scroll.saturating_sub(row)
    };
}

/// Whether a press on `e` shows it big: something there that is no group
/// to open or close (DoInfoBut 0x4f72d0).
pub fn describable(e: &Entry) -> bool {
    e.flags & EMPTY == 0 && !e.is_group() && e.flags != 0
}

/// The picture of `e` shown big (PaintObjBut 0x4f69e0): `COBJ_ARMING09`-
/// `11` the marines (`12`-`14` the Persians'), `COBJ_ARMING00`-`08` the
/// weapons (item - 3), `15`-`17` the live crews, `COBJ_ARMORAM` armour and
/// rams by level, `COBJ_RC` goods, `COBJ_HEROY` heroes, `COBJ_TREAS` by
/// treasure kind (0x5005a0).
pub fn big_picture(e: &Entry, nation3: bool) -> Option<String> {
    let s = e.sub;
    let name = match e.cat {
        UNIT => match s {
            0..=2 => format!("COBJ_ARMING{:02}", s + if nation3 { 12 } else { 9 }),
            3..=11 => format!("COBJ_ARMING{:02}", s - 3),
            12..=14 => format!("COBJ_ARMING{:02}", s + 3),
            _ => return None,
        },
        ARMOUR | RAM => format!("COBJ_ARMORAM{s:02}"),
        GOODS => format!("COBJ_RC{s:02}"),
        HERO => format!("COBJ_HEROY{s:02}"),
        TREASURE => format!("COBJ_TREAS{:02}", treasure_picture(s)),
        _ => return None,
    };
    Some(name)
}

/// 0x597d2c: per item three values (hull damage A, crew damage B, boarding
/// C) at three levels.
#[rustfmt::skip]
const POWER: [[[u32; 3]; 3]; 12] = [
    [[5, 7, 10], [45, 56, 68], [35, 44, 52]],
    [[7, 9, 12], [25, 31, 38], [68, 85, 102]],
    [[3, 5, 7], [5, 8, 11], [100, 125, 150]],
    [[40, 48, 56], [80, 96, 112], [0, 0, 0]],
    [[60, 72, 84], [60, 72, 84], [0, 0, 0]],
    [[80, 96, 112], [40, 48, 56], [0, 0, 0]],
    [[200, 240, 280], [80, 96, 112], [0, 0, 0]],
    [[250, 300, 350], [100, 120, 140], [0, 0, 0]],
    [[220, 264, 330], [80, 96, 112], [0, 0, 0]],
    [[0, 0, 0], [0, 0, 0], [0, 0, 0]],
    [[200, 240, 280], [80, 96, 112], [0, 0, 0]],
    [[200, 240, 280], [80, 96, 112], [0, 0, 0]],
];
/// 0x598154 and 0x598178: longest and shortest range by level and item.
#[rustfmt::skip]
const RANGE_MAX: [[u16; 12]; 3] = [
    [3, 2, 2, 3, 3, 3, 5, 3, 4, 2, 3, 0],
    [3, 2, 2, 4, 3, 3, 5, 4, 4, 2, 3, 0],
    [4, 3, 3, 4, 4, 4, 6, 5, 5, 3, 4, 0],
];
const RANGE_MIN: [u16; 12] = [0, 0, 0, 0, 0, 1, 2, 1, 1, 0, 0, 0];
/// 0x597a3c: rate of fire by item and level.
#[rustfmt::skip]
const RATE: [[u16; 3]; 12] = [
    [120, 150, 150], [120, 150, 150], [120, 150, 150], [100, 130, 130],
    [120, 120, 120], [90, 110, 110], [70, 90, 90], [60, 80, 80],
    [70, 90, 90], [50, 70, 80], [45, 50, 55], [60, 80, 80],
];
/// 0x597164: price by item group, a u16 every four bytes (the fourth is
/// the fish table's first, 2).
const PRICE: [u16; 4] = [30, 50, 70, 2];
/// 0x597a00 (armour) and 0x598054 (rams): by kind and the player's level
/// in it; a ram's high byte is read from 0x598084 (4).
const ARMOUR_CUT: [u16; 9] = [0, 7, 14, 20, 27, 33, 37, 44, 50];
const RAM_QUALITY: [u16; 9] = [100, 110, 117, 125, 135, 145, 160, 175, 200];
const RAM_HIGH_BYTE: u16 = 4;

/// What 0x5234b0 writes about `e` (`#` starts a new paragraph): a unit's
/// name, weight, effectiveness against crews and against hulls (boarding
/// for marines) at its condition, range, rate of fire and price (0x4a9870,
/// by the human player's levels); armour's and rams' effectiveness, level
/// and price (0x4a99e0, 0x4a9a60); goods, heroes and treasures their
/// strings. `treasure` is the treasure of a treasure entry.
pub fn describe(
    strings: &std::collections::HashMap<u32, String>,
    e: &Entry,
    levels: &Levels,
    treasure: Option<&Treasure>,
) -> String {
    let s = |id: u32| strings.get(&id).map_or("", |s| s.as_str());
    match e.cat {
        UNIT => {
            let name = if e.sub <= 14 { 0x3a0 + e.sub as u32 } else { 7 };
            let b = if (12..=14).contains(&e.sub) {
                e.sub - 12
            } else {
                e.sub
            } as usize;
            let Some(power) = POWER.get(b) else {
                return String::new();
            };
            let g = b / 3;
            let level = |l: [u8; 3]| l.get(g).copied().unwrap_or(0).min(2) as usize;
            let cond = e.bar.unwrap_or(0) as u32;
            let weight = if (6..=8).contains(&b) { 2 } else { 1 };
            let arms = level(levels.arms);
            let crews = power[1][arms] * cond / 100;
            let other = power[if b < 3 { 2 } else { 0 }][arms] * cond / 100;
            let reach = level(levels.reach);
            let rate = RATE[b][level(levels.rate)];
            format!(
                "{}#{}#{} #{}#{} / {}#{}#{} - {}#{}#{}#{}#{}",
                s(name),
                s(0x3c3),
                weight,
                s(0x3c0),
                crews,
                other,
                s(0x3c4),
                RANGE_MIN[b],
                RANGE_MAX[reach][b],
                s(0x3c5),
                rate,
                s(0x3c2),
                PRICE[g.min(3)]
            )
        }
        ARMOUR | RAM => {
            // The player's level in each kind (+0x60, +0x67) is not kept:
            // 0, as the missions start the human players.
            let level = 0usize;
            let (name, effect, price) = if e.cat == ARMOUR {
                let k = e.sub.min(2) as usize;
                (0x3b1 + k as u32, ARMOUR_CUT[k * 3 + level], [0, 30, 60][k])
            } else {
                let k = e.sub.saturating_sub(3).min(2) as usize;
                (
                    0x3b4 + k as u32,
                    RAM_QUALITY[k * 3 + level] | RAM_HIGH_BYTE << 8,
                    [0, 25, 50][k],
                )
            };
            format!(
                "{}#{}#{}#{}#{}#{}#{}",
                s(name),
                s(0x3c0),
                effect,
                s(0x3c1),
                level,
                s(0x3c2),
                price
            )
        }
        GOODS => {
            let k = e.sub.min(2) as u32;
            format!("{}#{}", s(0x3b8 + k), s(0x3bd + k))
        }
        HERO => s(900 + e.sub as u32).to_string(),
        TREASURE => {
            let param = treasure.map_or(0, |t| t.param);
            let amount = treasure.map_or(0, |t| t.amount);
            match e.sub {
                6 => {
                    let (goods, unit) = match param {
                        400 => (0x3b8, 0x3bb),
                        401 => (0x3b9, 0x3bc),
                        402 => (0x3ba, 0x3bb),
                        _ => (7, 7),
                    };
                    format!("{} {} {:2} {}", s(goods), s(0x3b7), amount, s(unit))
                }
                7 => {
                    let (cat, item) = (param & 0xffff, param >> 16);
                    let name = match (cat, item) {
                        (1, 0..=14) => 0x3a0 + item,
                        (2, 0..=2) => 0x3b1 + item,
                        (4, 3..=5) => 0x3b4 + item - 3,
                        _ => 7,
                    };
                    format!("{} {}.#{}{:2}", s(name), s(0x3af), s(0x3b0), amount)
                }
                9..=17 => s(0x396 + e.sub as u32 - 9).to_string(),
                18..=26 => s(0x38d + e.sub as u32 - 18).to_string(),
                // A scroll of knowledge says what it teaches (0x47b4a0).
                27 => {
                    let id = match param {
                        0 => 2105,
                        1 => 2106,
                        3 => 2108,
                        4 => 2107,
                        5 => 2112,
                        12 => 2113,
                        _ => 0x39f,
                    };
                    s(id).to_string()
                }
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

/// Breaks `text` into lines no wider than `width` (mfcfnt.cpp 0x548590):
/// each `#` paragraph on its own; words end after a space or punctuation
/// and keep it; a word that would overflow starts a new line, which loses
/// its leading spaces; a paragraph's last line gets a space more when it
/// still fits (0x546a30).
pub fn wrap(font: &TextRenderer, gap: u32, text: &str, width: u32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split(PARAGRAPH) {
        let mut first = true;
        let mut line = String::new();
        let mut flush = |line: &mut String, lines: &mut Vec<String>| {
            if !line.is_empty() {
                let shown = if first {
                    line.as_str()
                } else {
                    line.trim_start_matches([' ', '\t'])
                };
                lines.push(shown.to_string());
                first = false;
            }
            line.clear();
        };
        let mut wide = 0;
        let mut word = String::new();
        let words = paragraph.chars().fold(Vec::new(), |mut words, c| {
            word.push(c);
            if c.is_ascii() && BREAKS.contains(&(c as u8)) {
                words.push(std::mem::take(&mut word));
            }
            words
        });
        for w in words.iter().chain((!word.is_empty()).then_some(&word)) {
            if width < gap + font.measure(w) + wide {
                flush(&mut line, &mut lines);
            }
            line.push_str(w);
            wide = font.measure(&line);
        }
        let kept = line.clone();
        line.push(' ');
        if font.measure(&line) > width {
            line = kept;
            flush(&mut line, &mut lines);
        }
        flush(&mut line, &mut lines);
    }
    lines
}

/// The group panel's and the big slot's pictures.
pub struct InfoArt {
    group_back: Option<Image>,
    group_scroll_back: Option<Image>,
    group_track: Option<Image>,
    member_frame: Option<Image>,
    member_shadow: Option<Image>,
    clear: Option<Image>,
    big_slot: Option<Image>,
    big_font: Option<TextRenderer>,
}

impl InfoArt {
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
        Self {
            group_back: themed("CGRP_BKG"),
            group_scroll_back: themed("CGRP_BKGSCROLL"),
            group_track: themed("CGRP_BKSCROLL"),
            member_frame: themed("CGRP_BUTFRAME"),
            member_shadow: data
                .load_image(&data.controls, &format!("{theme}CGRP_BUTBK"), false)
                .ok()
                .flatten(),
            clear: themed("COBJ_CLEAR"),
            big_slot: themed("COBJ_BKBIGSLOT"),
            big_font: TextRenderer::load_in(&data.controls, "BSLT_FONT", 0, Some(&data.palette))
                .ok()
                .map(|f| f.with_spacing(BIG_LETTERS)),
        }
    }

    /// `COBJ_CLEAR` at bar (0, 206), with nothing selected.
    pub fn draw_clear(&self, frame: &mut Frame, bar_x: f32, s: f32) {
        if let Some(img) = &self.clear {
            blit_scaled(frame, img, full(img), bar_x, 206.0 * s, s);
        }
    }

    /// The group's background (`CGRP_BKG` at bar (3, 206)), and the
    /// slider's (`CGRP_BKGSCROLL`) when it scrolls.
    pub fn draw_group_back(&self, frame: &mut Frame, bar_x: f32, s: f32, n: usize) {
        if let Some(img) = &self.group_back {
            blit_scaled(frame, img, full(img), bar_x + 3.0 * s, 206.0 * s, s);
        }
        if n >= GROUP_SCROLLS {
            if let Some(img) = &self.group_scroll_back {
                let (x, y) = GROUP_SCROLL_BACK;
                blit_scaled(frame, img, full(img), bar_x + x as f32 * s, y as f32 * s, s);
            }
        }
    }

    /// A member's button (PaintGrpBut 0x4f6640): the shadow `CGRP_BUTBK`
    /// at (0, 2), then `CGRP_BUTFRAME` with the portrait at (1, 1) and the
    /// life bar at its foot, at (1, 0), or (0, 1) held down.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_member(
        &self,
        frame: &mut Frame,
        (x, y): (f32, f32),
        s: f32,
        portrait: Option<&Image>,
        life: u8,
        bar_colours: &[[u32; 2]; 3],
        down: bool,
    ) {
        if let Some(img) = &self.member_shadow {
            blit_scaled(frame, img, full(img), x, y + 2.0 * s, s);
        }
        let Some(back) = &self.member_frame else {
            return;
        };
        let mut img = Image::new(back.width, back.height, back.pixels.clone());
        if let Some(p) = portrait {
            paste(&mut img, p, 1, 1);
        }
        bar(&mut img, life, bar_colours);
        let (fx, fy) = if down { (x, y + s) } else { (x + s, y) };
        blit_scaled(frame, &img, full(&img), fx, fy, s);
    }

    /// Entry `e` held down: `COBJ_BKBIGSLOT` with its picture and what
    /// 0x5234b0 says of it, at bar (9, 210).
    pub fn big_slot(&self, picture: Option<&Image>, text: &str) -> Option<Image> {
        let back = self.big_slot.as_ref()?;
        let mut img = Image::new(back.width, back.height, back.pixels.clone());
        if let Some(p) = picture {
            paste(&mut img, p, BIG_PICTURE.0, BIG_PICTURE.1);
        }
        let Some(font) = &self.big_font else {
            return Some(img);
        };
        let (bx, by, bw, bh) = BIG_TEXT;
        const NONE: u32 = 0xff00_0000;
        let mut pixels = vec![NONE; (bw * bh) as usize];
        let mut f = Frame {
            pixels: &mut pixels,
            width: bw,
            height: bh,
        };
        let mut ty = 0i32;
        for line in wrap(font, BIG_LETTERS, text, bw) {
            let dx = (bw as i32 - font.measure(&line) as i32) / 2;
            font.draw(&mut f, dx, ty, 1, &line);
            ty += font.height(&line) as i32 + BIG_LINES;
        }
        for (k, &p) in pixels.iter().enumerate() {
            if p != NONE {
                let (px, py) = (bx + (k as u32 % bw) as i32, by + (k as u32 / bw) as i32);
                if px >= 0 && py >= 0 && (px as u32) < img.width && (py as u32) < img.height {
                    img.pixels[(py as u32 * img.width + px as u32) as usize] = Some(p);
                }
            }
        }
        Some(img)
    }

    /// A slider (0x4f76e0): arrows `SLIDER_UP_`/`SLIDER_DN_` (frame 01
    /// held down), the track and the `SLIDER_BB` thumb at its place.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_slider(
        &self,
        frame: &mut Frame,
        bar_x: f32,
        s: f32,
        load: &crate::load_panel::LoadArt,
        group: bool,
        (scroll, max): (usize, usize),
        pressed: Option<Hit>,
    ) {
        let (up, down, track_y, track_h) = if group { GROUP_SLIDER } else { LIST_SLIDER };
        let at = |x: u32, y: u32| (bar_x + x as f32 * s, y as f32 * s);
        let track = if group {
            self.group_track.as_ref()
        } else {
            load.track[0].as_ref()
        };
        if let Some(img) = track {
            let (x, y) = at(TRACK_X, track_y);
            blit_scaled(frame, img, full(img), x, y, s);
        }
        if let Some(img) = &load.thumb {
            let room = track_h.saturating_sub(img.height);
            let k = scroll.min(max) as u32 * room / max.max(1) as u32;
            let (x, y) = at(TRACK_X + 4, track_y + k);
            blit_scaled(frame, img, full(img), x, y, s);
        }
        for (frames, y, hit) in [(&load.up, up, Hit::Up), (&load.down, down, Hit::Down)] {
            let f = usize::from(pressed == Some(hit));
            if let Some(img) = frames.get(f).or(frames.first()) {
                let (x, y) = at(ARROW_X, y);
                blit_scaled(frame, img, full(img), x, y, s);
            }
        }
    }
}

/// Whether a list's or group's slider shows, and its scroll and furthest.
pub fn slider(shown: &Shown, view: &InfoView) -> Option<(bool, usize, usize)> {
    match shown {
        Shown::Group(ids) if ids.len() >= GROUP_SCROLLS => {
            Some((true, view.scroll, group_max_scroll(ids.len())))
        }
        Shown::Single(_, list) if list_scrolls(list) => {
            Some((false, view.scroll, list_max_scroll(list.len() - 1)))
        }
        _ => None,
    }
}

/// The opened list's slider background (`COBJ_BKGSCROLL` at panel
/// (165, 6)), when it shows.
pub fn list_scroll_back(shown: &Shown) -> bool {
    matches!(shown, Shown::Single(_, list) if list_scrolls(list))
}

/// A group member's portrait (0x4f6640): a fort's `INF_GOBJ` by its size
/// and nation, a ship's `INF_SHIP` as on the object panel.
pub fn member_portrait(obj: &MapObject) -> Option<String> {
    match obj.class {
        ObjectClass::Building(c @ 0x34..=0x36) => {
            let nation = obj.owner.map_or(1, |o| o.0);
            Some(format!("INF_GOBJ{:02}", (c - 0x34) * 4 + nation))
        }
        _ if obj.ship.is_some() => obj.picture.as_ref().map(|(n, _)| n.clone()),
        _ => None,
    }
}

/// The treasure entry `e` of `holder` stands for.
pub fn treasure_of<'a>(scene: &'a Scene, holder: &'a MapObject, e: &Entry) -> Option<&'a Treasure> {
    let Some(Cargo::Treasure { index, .. }) = e.cargo else {
        return None;
    };
    match &holder.ship {
        Some(ship) => ship.treasures.get(index as usize),
        None => scene
            .held
            .iter()
            .filter(|h| h.0 == holder.id)
            .nth(index as usize)
            .map(|h| &h.1),
    }
}

fn full(img: &Image) -> (u32, u32, u32, u32) {
    (0, 0, img.width, img.height)
}

/// Copies `src`'s opaque pixels onto `dst` at `(x, y)`.
fn paste(dst: &mut Image, src: &Image, x: i32, y: i32) {
    for sy in 0..src.height as i32 {
        for sx in 0..src.width as i32 {
            let (dx, dy) = (x + sx, y + sy);
            if dx < 0 || dy < 0 || dx as u32 >= dst.width || dy as u32 >= dst.height {
                continue;
            }
            if let Some(p) = src.pixels[(sy as u32 * src.width + sx as u32) as usize] {
                dst.pixels[(dy as u32 * dst.width + dx as u32) as usize] = Some(p);
            }
        }
    }
}

/// The life bar on a member's frame (0x4f6640): black, 4 high, from
/// (1, h - 5), its length (w - 2) x life / 100; two lines of colour inside
/// once longer than 2 (green, yellow below half, red below a quarter).
fn bar(img: &mut Image, life: u8, colours: &[[u32; 2]; 3]) {
    let (w, h) = (img.width, img.height);
    let len = (w - 2) * life.min(100) as u32 / 100;
    let c = if life >= 50 {
        colours[0]
    } else if life >= 25 {
        colours[1]
    } else {
        colours[2]
    };
    let mut put = |x: u32, y: u32, p: u32| {
        if x < w && y < h {
            img.pixels[(y * w + x) as usize] = Some(p);
        }
    };
    for y in h - 5..h - 1 {
        for x in 1..1 + len {
            put(x, y, 0);
        }
    }
    if len > 2 {
        for x in 2..len {
            put(x, h - 4, c[0]);
            put(x, h - 3, c[1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::Caches;
    use crate::scene::load::FITTINGS;

    fn data() -> Option<GameData> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        Some(GameData::open(std::path::Path::new(&root)).unwrap())
    }

    fn entry(cat: u8, sub: u8, bar: Option<u8>) -> Entry {
        Entry {
            cat,
            sub,
            flags: PRESENT,
            count: 0,
            bar,
            cargo: None,
        }
    }

    #[test]
    fn group_buttons_fill_rows_and_scroll() {
        assert_eq!(
            group_cells(5, 0),
            vec![
                (0, 10, 211),
                (1, 58, 211),
                (2, 106, 211),
                (3, 154, 211),
                (4, 10, 258)
            ]
        );
        assert_eq!(group_cells(16, 0).len(), 16);
        // From 17 on: three to a row, twelve shown, the slider beside.
        let cells = group_cells(20, 0);
        assert_eq!(cells.len(), 12);
        assert_eq!(cells[3], (3, 10, 258));
        assert_eq!(group_max_scroll(20), 9);
        assert_eq!(group_cells(20, 99)[0], (9, 10, 211));
        assert_eq!(group_cells(20, 99).len(), 11);
        let ids: Vec<u32> = (0..20).collect();
        let shown = Shown::Group(ids);
        let mut view = InfoView::default();
        assert_eq!(hit(&shown, &view, 60.0, 260.0), Some(Hit::Member(4)));
        assert_eq!(hit(&shown, &view, 180.0, 370.0), Some(Hit::Down));
        step(&shown, &mut view, true);
        step(&shown, &mut view, true);
        step(&shown, &mut view, true);
        step(&shown, &mut view, true);
        assert_eq!(view.scroll, 9);
    }

    #[test]
    fn units_armour_and_treasures_are_described() {
        let Some(data) = data() else {
            return;
        };
        let levels = Levels::default();
        let s = |e: &Entry, t: Option<&Treasure>| describe(&data.strings, e, &levels, t);
        assert_eq!(
            s(&entry(UNIT, 0, Some(100)), None),
            "BOWMEN#Weight#1 #Effectiveness#45 / 35#Range#0 - 3#Rate of fire#120#Costs#30"
        );
        // Worn units are less effective; strong weapons weigh two.
        assert_eq!(
            s(&entry(UNIT, 7, Some(50)), None),
            "BIG CATAPULT#Weight#2 #Effectiveness#50 / 125#Range#1 - 3#Rate of fire#60#Costs#70"
        );
        let mut better = levels;
        better.arms = [2, 0, 0];
        better.reach = [1, 0, 0];
        assert_eq!(
            describe(&data.strings, &entry(UNIT, 13, Some(100)), &better, None),
            "LIVE JAVELINMEN#Weight#1 #Effectiveness#38 / 102#Range#0 - 2#Rate of fire#120#Costs#30"
        );
        assert_eq!(
            s(&entry(ARMOUR, 1, None), None),
            "SILVER ARMOR#Effectiveness#20#Level#0#Costs#30"
        );
        assert_eq!(
            s(&entry(RAM, 3, None), None),
            "REGULAR RAM#Effectiveness#1124#Level#0#Costs#0"
        );
        assert_eq!(s(&entry(GOODS, 1, None), None), "FISH.# ");
        assert!(s(&entry(HERO, 4, None), None).starts_with("HERCULES."));
        let pack = Treasure {
            kind: 7,
            param: 2 << 16 | 1,
            amount: 5,
        };
        assert_eq!(
            s(&entry(TREASURE, 7, None), Some(&pack)),
            "SWORDSMEN IN A DISASSEMBLED STATE.#QUANTITY 5"
        );
        let amber = Treasure {
            kind: 6,
            param: 400,
            amount: 300,
        };
        assert_eq!(
            s(&entry(TREASURE, 6, None), Some(&amber)),
            "AMBER. After delivering to settlement you will get 300 money"
        );
        let scroll = Treasure {
            kind: 27,
            param: 3,
            amount: 0,
        };
        assert_eq!(
            s(&entry(TREASURE, 27, None), Some(&scroll)),
            "Technology that allows you to produce strong weapons."
        );
        assert!(s(&entry(TREASURE, 10, None), None).starts_with("Protection from fire"));
    }

    #[test]
    fn descriptions_wrap_by_paragraph() {
        let Some(data) = data() else {
            return;
        };
        let font = TextRenderer::load_in(&data.controls, "BSLT_FONT", 0, Some(&data.palette))
            .unwrap()
            .with_spacing(BIG_LETTERS);
        let lines = wrap(&font, 1, "BOWMEN#Weight#1 #Rate of fire#120", 80);
        // A paragraph's last line keeps a space more when it fits.
        assert_eq!(
            lines,
            ["BOWMEN ", "Weight ", "1  ", "Rate of fire ", "120 "]
        );
        let long = "Golden Fleece - All gods protect the owner of the Golden Fleece.";
        let lines = wrap(&font, 1, long, 80);
        assert!(lines.len() > 2, "{lines:?}");
        for l in &lines {
            assert!(font.measure(l) <= 80, "{l:?}");
            assert!(!l.starts_with(' '), "{l:?}");
        }
        assert_eq!(lines.concat().replace(' ', ""), long.replace(' ', ""));
    }

    #[test]
    fn settlement_places_open_to_their_units() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let site = scene
            .objects
            .iter()
            .find(|o| {
                scene.is_own(o)
                    && matches!(o.class, ObjectClass::Building(crate::economy::SETTLEMENT))
            })
            .unwrap()
            .id;
        let mut stock = scene.stock_units(site);
        stock[0] = vec![100, 40];
        scene
            .garrisons
            .insert(site, std::array::from_fn(|k| stock[k].len() as u32));
        scene.stock_conditions.insert(site, stock);
        let obj = &scene.objects[scene.object_index(site).unwrap()];
        let closed = entries(&scene, obj, None);
        assert_eq!(closed[0].flags, PRESENT | GROUP);
        assert_eq!((closed[0].count, closed[0].bar), (2, None));
        assert!(closed[0].is_group() && !describable(&closed[0]));
        assert_eq!(closed[3].cat, FITTINGS);
        let open = entries(&scene, obj, Some((UNIT, 0)));
        assert_eq!(open.len(), 3);
        assert_eq!(open[0].flags, HEADER | PRESENT);
        assert_eq!(open[2].bar, Some(40));
        assert!(describable(&open[1]));
        // The header closes it from the lower left; the units three to a row.
        assert_eq!(
            cells(&open, 0),
            vec![(0, 9, 312), (1, 57, 210), (2, 105, 210)]
        );
        let view = InfoView::default();
        let shown = Shown::Single(site, open);
        assert_eq!(hit(&shown, &view, 60.0, 215.0), Some(Hit::Cell(1)));
        assert_eq!(hit(&shown, &view, 10.0, 215.0), Some(Hit::Portrait));
        assert_eq!(hit(&shown, &view, 190.0, 300.0), None);
    }
}
