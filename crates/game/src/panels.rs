//! The control bar's bookmark tabs and the panels they open.
//!
//! The bar ends in five tabs (`BMARK_*`, ids 1000-1004 in 0x511c00, left to
//! right): the object panel (ship's wheel), files (disk), game settings
//! (wrench), sound (clef) and help and tasks (scroll). The other panels
//! cover the bar above the tabs. Their art is the original's; the placement
//! of their controls is this port's own, since the original lays them out in
//! code that has not been followed yet.

use std::rc::Rc;

use anyhow::Result;
use openac_formats::{Bitmap, ResourceKind};

use crate::assets::{rgb, GameData, Image};
use crate::render::{blit_scaled, Frame};
use crate::text::TextRenderer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Tab {
    #[default]
    Object,
    File,
    Settings,
    Sound,
    Help,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Object, Tab::File, Tab::Settings, Tab::Sound, Tab::Help];
}

/// A slider's value, 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slider {
    GameSpeed,
    ScrollSpeed,
    Volume,
    Music,
}

/// What a click on the bar hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelHit {
    Tab(Tab),
    Slot(usize),
    Save,
    Load,
    Delete,
    Restart,
    Quit,
    Slider(Slider, f32),
    /// The help panel's up and down buttons.
    HelpUp,
    HelpDown,
}

/// Number of save slots.
pub const SLOTS: usize = 8;

/// The state the panels show.
pub struct PanelView<'a> {
    pub tab: Tab,
    /// One label per save slot (`None` for an empty slot).
    pub slots: &'a [Option<String>],
    pub selected_slot: Option<usize>,
    pub game_speed: f32,
    pub scroll_speed: f32,
    pub volume: f32,
    /// Music volume, if music is playing.
    pub music: Option<f32>,
    /// Help panel text, as paragraphs (see `help_paragraphs`).
    pub help: &'a [String],
    /// The first help line shown.
    pub help_first: usize,
}

// Placement in original bar pixels.
const TABS_Y: u32 = 558;
const TAB_X: [u32; 5] = [0, 44, 85, 126, 167];
const PANEL: (u32, u32) = (3, 3);
const SLOT_LIST: (u32, u32, u32) = (12, 44, 34);
const NAME_BOX: (u32, u32) = (12, 8);
const SAVE_BUTTONS: [(u32, u32); 3] = [(4, 336), (70, 336), (136, 336)];
const RESTART_BUTTON: (u32, u32) = (3, 460);
const QUIT_BUTTON: (u32, u32) = (54, 508);
const TRACK_X: u32 = 33;
// The help panel (Viewsys 0x526930), measured on the original: its
// background, text (`HELP_FONT`, 2 pixels between letters and lines,
// 188 wide, 445 high), up and down buttons and its two page tabs.
const HELP_BG: (u32, u32) = (4, 5);
const HELP_TEXT: (u32, u32, u32, u32) = (12, 11, 188, 445);
const HELP_LINE_GAP: u32 = 2;
const HELP_BUTTONS: [(u32, u32); 2] = [(10, 460), (108, 460)];
const HELP_TABS: [(u32, u32); 2] = [(3, 513), (103, 513)];
const SLIDER_ROWS: [(u32, Slider); 4] = [
    (40, Slider::GameSpeed),
    (110, Slider::ScrollSpeed),
    (40, Slider::Volume),
    (110, Slider::Music),
];

pub struct Panels {
    /// Per tab: normal, hover, active.
    tabs: Vec<[Option<Image>; 3]>,
    file_bg: Option<Image>,
    settings_bg: Option<Image>,
    help_bg: Option<Image>,
    help_font: Option<TextRenderer>,
    /// Up and down: normal, pressed, disabled.
    help_buttons: [[Option<Image>; 3]; 2],
    /// The objectives and quick help page tabs: normal, selected.
    help_tabs: [[Option<Image>; 2]; 2],
    /// Save, load, delete; restart; quit. Each normal and pressed.
    file_buttons: Vec<[Option<Image>; 2]>,
    tracks: [Option<Image>; 4],
    thumb: Option<Rc<Image>>,
}

impl Panels {
    pub fn load(data: &GameData, theme: char) -> Result<Self> {
        let bitmap = |name: &str| -> Option<Image> {
            let full = format!("{theme}{name}");
            let data = data.controls.read(&full, ResourceKind::Bitmap).ok()?;
            let b = Bitmap::parse(&data).ok()?;
            let pixels = b
                .pixels
                .iter()
                .map(|&i| Some(rgb(b.palette.0[i as usize])))
                .collect();
            Some(Image::new(b.width, b.height, pixels))
        };
        let tabs = ["COBJ", "FILE", "CFG", "VHELP", "VTASK"]
            .iter()
            .map(|t| [0, 1, 2].map(|s| bitmap(&format!("BMARK_{t}_{s:02}"))))
            .collect();
        let file_buttons = [
            "CFILE_BMRKSAVE",
            "CFILE_BMRKLOAD",
            "CFILE_BMRKDEL",
            "CFILE_RSTART",
            "CFILE_QUIT",
        ]
        .iter()
        .map(|b| [0, 1].map(|s| bitmap(&format!("{b}{s:02}"))))
        .collect();
        Ok(Self {
            tabs,
            file_bg: bitmap("CFILE_BKGND"),
            settings_bg: bitmap("CCFG_BKGND"),
            help_bg: bitmap("HVIEW_BKGND"),
            help_font: TextRenderer::load(&data.controls, &format!("{theme}HELP_FONT"), 0)
                .ok()
                .map(|f| f.with_spacing(2)),
            help_buttons: ["VIEW_BUTUP_", "VIEW_BUTDN_"]
                .map(|b| [0, 1, 2].map(|s| bitmap(&format!("{b}{s:02}")))),
            help_tabs: ["HVIEW_TASK", "HVIEW_HELP"]
                .map(|t| [0, 1].map(|s| bitmap(&format!("{t}{s:02}")))),
            file_buttons,
            tracks: [
                bitmap("CFGSCR_GSPD"),
                bitmap("CFGSCR_KSPD"),
                bitmap("CFGSND_EFFVOL"),
                bitmap("CFGSND_MUSVOL"),
            ],
            thumb: data
                .load_image(&data.controls, &format!("{theme}TRACKBAR_BB"), false)
                .ok()
                .flatten()
                .map(Rc::new),
        })
    }

    /// The tab art in the bar picture is in this order: wheel, disk,
    /// wrench, clef, scroll. The resource names do not follow it (the disk
    /// is `BMARK_VTASK`), so they are matched by position.
    fn tab_image(&self, i: usize, state: usize) -> Option<&Image> {
        const ORDER: [usize; 5] = [0, 4, 1, 2, 3];
        self.tabs.get(ORDER[i]).and_then(|t| t[state].as_ref())
    }

    /// Draws the tabs and, unless the object panel is shown, the open panel.
    /// `origin` is the bar's top-left corner in window pixels; `hover` the
    /// bar-relative point under the cursor.
    pub fn draw(
        &self,
        frame: &mut Frame,
        origin: (f32, f32),
        scale: f32,
        view: &PanelView,
        text: &TextRenderer,
        hover: Option<(u32, u32)>,
    ) {
        let at = |x: u32, y: u32| (origin.0 + x as f32 * scale, origin.1 + y as f32 * scale);
        let blit = |frame: &mut Frame, img: &Image, x: u32, y: u32| {
            let (px, py) = at(x, y);
            blit_scaled(frame, img, (0, 0, img.width, img.height), px, py, scale);
        };
        let hovered = |r: (u32, u32, u32, u32)| hover.is_some_and(|p| inside(p, r));
        // The open tab and the one under the cursor are lit (state 1, as
        // on the original); state 2 is a darkened one.
        for (i, tab) in Tab::ALL.iter().enumerate() {
            let w = tab_width(i);
            let state = if *tab == view.tab || hovered((TAB_X[i], TABS_Y, w, 42)) {
                1
            } else {
                0
            };
            if let Some(img) = self.tab_image(i, state) {
                blit(frame, img, TAB_X[i], TABS_Y);
            }
        }
        let ts = scale.round().max(1.0) as u32;
        let write = |frame: &mut Frame, x: u32, y: u32, s: &str| {
            let (px, py) = at(x, y);
            text.draw(frame, px as i32, py as i32, ts, s);
        };
        let line = (text.line_height() * ts) as f32 / scale;
        match view.tab {
            Tab::Object => {}
            Tab::File => {
                if let Some(bg) = &self.file_bg {
                    blit(frame, bg, PANEL.0, PANEL.1);
                }
                let name = view
                    .selected_slot
                    .map(|i| match &view.slots[i] {
                        Some(label) => label.clone(),
                        None => format!("Slot {}: empty", i + 1),
                    })
                    .unwrap_or_else(|| "Choose a slot".into());
                write(frame, PANEL.0 + NAME_BOX.0, PANEL.1 + NAME_BOX.1, &name);
                for (i, slot) in view.slots.iter().enumerate() {
                    let y = PANEL.1 + SLOT_LIST.1 + i as u32 * SLOT_LIST.2;
                    let mark = if view.selected_slot == Some(i) {
                        ">"
                    } else {
                        " "
                    };
                    let label = slot.as_deref().unwrap_or("empty");
                    write(frame, PANEL.0 + SLOT_LIST.0, y, &format!("{mark}{}", i + 1));
                    write(frame, PANEL.0 + SLOT_LIST.0 + 22, y, label);
                }
                for (i, (x, y)) in file_button_rects().into_iter().enumerate() {
                    let (w, h) = self.button_size(i);
                    let state = usize::from(hovered((x, y, w, h)));
                    if let Some(img) = self.file_buttons[i][state].as_ref() {
                        blit(frame, img, x, y);
                    }
                    // The restart and quit buttons are blank frames; the
                    // original writes their captions.
                    let caption = match i {
                        3 => "Restart mission",
                        4 => "Quit",
                        _ => continue,
                    };
                    let tw = (text.measure(caption) * ts) as f32 / scale;
                    let cx = x + ((w as f32 - tw) / 2.0).max(0.0) as u32;
                    let cy = y + ((h as f32 - line) / 2.0).max(0.0) as u32;
                    write(frame, cx, cy, caption);
                }
            }
            Tab::Settings | Tab::Sound => {
                if let Some(bg) = &self.settings_bg {
                    blit(frame, bg, PANEL.0, PANEL.1);
                }
                let rows: &[(&str, Slider, f32)] = if view.tab == Tab::Settings {
                    &[
                        ("Game speed", Slider::GameSpeed, view.game_speed),
                        ("Scroll speed", Slider::ScrollSpeed, view.scroll_speed),
                    ]
                } else if let Some(music) = view.music {
                    &[
                        ("Sound volume", Slider::Volume, view.volume),
                        ("Music volume", Slider::Music, music),
                    ]
                } else {
                    &[("Sound volume", Slider::Volume, view.volume)]
                };
                for &(label, slider, value) in rows {
                    let y = slider_y(slider);
                    write(frame, TRACK_X, y - 20, label);
                    if let Some(track) = &self.tracks[slider_track(slider)] {
                        blit(frame, track, TRACK_X, y);
                        if let Some(thumb) = &self.thumb {
                            let travel = track.width.saturating_sub(thumb.width) as f32;
                            blit(
                                frame,
                                thumb,
                                TRACK_X + (value.clamp(0.0, 1.0) * travel) as u32,
                                y,
                            );
                        }
                    }
                }
            }
            Tab::Help => {
                if let Some(bg) = &self.help_bg {
                    blit(frame, bg, HELP_BG.0, HELP_BG.1);
                }
                let font = self.help_font.as_ref().unwrap_or(text);
                let lines = help_lines(font, view.help);
                let rows = self.help_rows();
                let pitch = font.line_height() + HELP_LINE_GAP;
                for (i, l) in lines.iter().skip(view.help_first).take(rows).enumerate() {
                    let (px, py) = at(HELP_TEXT.0, HELP_TEXT.1 + i as u32 * pitch);
                    font.draw(frame, px as i32, py as i32, ts, l);
                }
                let can = [view.help_first > 0, view.help_first + rows < lines.len()];
                for (k, &(x, y)) in HELP_BUTTONS.iter().enumerate() {
                    let imgs = &self.help_buttons[k];
                    let size = imgs[0].as_ref().map_or((90, 46), |b| (b.width, b.height));
                    let state = if !can[k] {
                        2
                    } else if hovered((x, y, size.0, size.1)) {
                        1
                    } else {
                        0
                    };
                    if let Some(img) = &imgs[state] {
                        blit(frame, img, x, y);
                    }
                }
                // The objectives page is the one shown.
                for (k, &(x, y)) in HELP_TABS.iter().enumerate() {
                    if let Some(img) = &self.help_tabs[k][usize::from(k == 0)] {
                        blit(frame, img, x, y);
                    }
                }
            }
        }
    }

    /// How many help lines the panel shows at once.
    pub fn help_rows(&self) -> usize {
        let pitch = self
            .help_font
            .as_ref()
            .map_or(20, |f| f.line_height() + HELP_LINE_GAP);
        ((HELP_TEXT.3 + HELP_LINE_GAP) / pitch.max(1)) as usize
    }

    /// The help text's lines, wrapped as the panel shows them.
    pub fn help_line_count(&self, help: &[String], text: &TextRenderer) -> usize {
        help_lines(self.help_font.as_ref().unwrap_or(text), help).len()
    }

    fn button_size(&self, i: usize) -> (u32, u32) {
        self.file_buttons[i][0]
            .as_ref()
            .map_or((66, 70), |b| (b.width, b.height))
    }

    /// What lies at bar-relative point `p` with `tab` open.
    pub fn hit(&self, tab: Tab, p: (u32, u32)) -> Option<PanelHit> {
        for (i, t) in Tab::ALL.iter().enumerate() {
            if inside(p, (TAB_X[i], TABS_Y, tab_width(i), 42)) {
                return Some(PanelHit::Tab(*t));
            }
        }
        match tab {
            Tab::Object => None,
            Tab::Help => {
                let hits = [PanelHit::HelpUp, PanelHit::HelpDown];
                HELP_BUTTONS
                    .iter()
                    .zip(hits)
                    .find(|(&(x, y), _)| inside(p, (x, y, 90, 46)))
                    .map(|(_, h)| h)
            }
            Tab::File => {
                let actions = [
                    PanelHit::Save,
                    PanelHit::Load,
                    PanelHit::Delete,
                    PanelHit::Restart,
                    PanelHit::Quit,
                ];
                for (i, (x, y)) in file_button_rects().into_iter().enumerate() {
                    let (w, h) = self.button_size(i);
                    if inside(p, (x, y, w, h)) {
                        return Some(actions[i]);
                    }
                }
                (0..SLOTS)
                    .find(|&i| {
                        let y = PANEL.1 + SLOT_LIST.1 + i as u32 * SLOT_LIST.2;
                        inside(p, (PANEL.0 + 6, y.saturating_sub(4), 160, SLOT_LIST.2))
                    })
                    .map(PanelHit::Slot)
            }
            Tab::Settings | Tab::Sound => {
                let sliders: &[Slider] = if tab == Tab::Settings {
                    &[Slider::GameSpeed, Slider::ScrollSpeed]
                } else {
                    &[Slider::Volume, Slider::Music]
                };
                sliders.iter().find_map(|&s| {
                    let track = self.tracks[slider_track(s)].as_ref()?;
                    let y = slider_y(s);
                    inside(
                        p,
                        (TRACK_X, y.saturating_sub(4), track.width, track.height + 8),
                    )
                    .then(|| {
                        let thumb = self.thumb.as_ref().map_or(0, |t| t.width);
                        let travel = track.width.saturating_sub(thumb).max(1) as f32;
                        let v = (p.0 as f32 - TRACK_X as f32 - thumb as f32 / 2.0) / travel;
                        PanelHit::Slider(s, v.clamp(0.0, 1.0))
                    })
                })
            }
        }
    }

    /// The value of `slider` for a cursor at bar-relative x `x` (for
    /// dragging, so not limited to the track).
    pub fn slider_value(&self, slider: Slider, x: f32) -> f32 {
        let Some(track) = self.tracks[slider_track(slider)].as_ref() else {
            return 0.0;
        };
        let thumb = self.thumb.as_ref().map_or(0, |t| t.width);
        let travel = track.width.saturating_sub(thumb).max(1) as f32;
        ((x - TRACK_X as f32 - thumb as f32 / 2.0) / travel).clamp(0.0, 1.0)
    }

    /// Whether `tab` covers the minimap and object panel.
    pub fn covers_bar(tab: Tab) -> bool {
        tab != Tab::Object
    }
}

fn tab_width(i: usize) -> u32 {
    if i == 0 {
        44
    } else {
        41
    }
}

fn file_button_rects() -> [(u32, u32); 5] {
    let p = |(x, y): (u32, u32)| (PANEL.0 + x, PANEL.1 + y);
    [
        p(SAVE_BUTTONS[0]),
        p(SAVE_BUTTONS[1]),
        p(SAVE_BUTTONS[2]),
        RESTART_BUTTON,
        QUIT_BUTTON,
    ]
}

fn slider_y(s: Slider) -> u32 {
    PANEL.1 + SLIDER_ROWS.iter().find(|r| r.1 == s).map_or(40, |r| r.0)
}

fn slider_track(s: Slider) -> usize {
    match s {
        Slider::GameSpeed => 0,
        Slider::ScrollSpeed => 1,
        Slider::Volume => 2,
        Slider::Music => 3,
    }
}

fn inside(p: (u32, u32), (x, y, w, h): (u32, u32, u32, u32)) -> bool {
    p.0 >= x && p.1 >= y && p.0 < x + w && p.1 < y + h
}

/// The help panel's text, as the original builds it (Viewsys 0x526930) from
/// the briefing's stored `lines`: for a scenario, what follows the blank
/// line `@   ` (the objectives); for a custom mission, the whole briefing
/// with its closing `@-` lines replaced by the mission's objectives
/// (0x5265e0). Each `@` line begins a paragraph; the lines after it are
/// joined on with a space.
pub fn help_paragraphs(lines: &[String], custom: bool, conditions: &[bool]) -> Vec<String> {
    let mut lines = lines.to_vec();
    if custom {
        while lines.last().is_some_and(|l| l.starts_with("@-")) {
            lines.pop();
        }
        for (i, _) in conditions.iter().enumerate().filter(|(_, &on)| on) {
            if let Some((goal, _)) = crate::scene::OBJECTIVES.get(i) {
                lines.push(format!("@-{goal}"));
            }
        }
    } else {
        // Up to and including the blank line (none: nothing is left).
        let start = lines
            .iter()
            .position(|l| l == "@   ")
            .map_or(lines.len(), |i| i + 1);
        lines.drain(..start);
    }
    let mut paragraphs: Vec<String> = Vec::new();
    for l in lines {
        match l.strip_prefix('@') {
            Some(rest) => paragraphs.push(rest.to_string()),
            None => match paragraphs.last_mut() {
                Some(p) => {
                    p.push(' ');
                    p.push_str(&l);
                }
                None => paragraphs.push(l),
            },
        }
    }
    paragraphs
}

/// Wraps help paragraphs to the panel's width: word by word, a word going
/// down when it and a space would not fit; a blank paragraph (`@ `) gives
/// an empty line.
fn help_lines(font: &TextRenderer, paragraphs: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    for p in paragraphs {
        let mut line = String::new();
        for word in p.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            // As the briefing screen does, with room kept for a space.
            if !line.is_empty() && font.measure(&format!("{candidate} ")) > HELP_TEXT.2 {
                lines.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = candidate;
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    #[test]
    fn help_shows_what_follows_the_blank_line() {
        let lines: Vec<String> = [
            "@Title",
            "@Story that goes on ",
            "and on.",
            "@   ",
            "@Your objectives:",
            "@ ",
            "@- Kill all Barbarians.",
            "@- Find the parchment ",
            "on board.",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            help_paragraphs(&lines, false, &[true]),
            [
                "Your objectives:",
                " ",
                "- Kill all Barbarians.",
                "- Find the parchment  on board."
            ]
        );
        // Custom missions: the whole text, the objectives from the
        // conditions.
        let mut conditions = [false; 11];
        conditions[0] = true;
        conditions[1] = true;
        let custom = help_paragraphs(&lines[..6], true, &conditions);
        assert_eq!(custom[0], "Title");
        assert_eq!(
            &custom[custom.len() - 2..],
            ["-Kill all enemies", "-Kill all dragons"]
        );
    }

    use super::*;

    /// Needs `OPENAC_DATA`.
    #[test]
    fn clicks_find_tabs_and_controls() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let panels = Panels::load(&data, '0').unwrap();
        assert_eq!(
            panels.hit(Tab::Object, (10, 570)),
            Some(PanelHit::Tab(Tab::Object))
        );
        assert_eq!(
            panels.hit(Tab::Object, (60, 570)),
            Some(PanelHit::Tab(Tab::File))
        );
        assert_eq!(
            panels.hit(Tab::Object, (200, 570)),
            Some(PanelHit::Tab(Tab::Help))
        );
        assert_eq!(
            panels.hit(Tab::Object, (100, 300)),
            None,
            "the object panel is not ours"
        );
        let slot = PANEL.1 + SLOT_LIST.1 + 2 * SLOT_LIST.2 + 5;
        assert_eq!(panels.hit(Tab::File, (40, slot)), Some(PanelHit::Slot(2)));
        let (sx, sy) = file_button_rects()[1];
        assert_eq!(
            panels.hit(Tab::File, (sx + 5, sy + 5)),
            Some(PanelHit::Load)
        );
        let y = slider_y(Slider::ScrollSpeed) + 5;
        match panels.hit(Tab::Settings, (TRACK_X + 140, y)) {
            Some(PanelHit::Slider(Slider::ScrollSpeed, v)) => assert!(v > 0.9),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            panels.hit(Tab::Sound, (TRACK_X + 2, slider_y(Slider::Volume) + 5)),
            Some(PanelHit::Slider(Slider::Volume, v)) if v < 0.1
        ));
    }
}
