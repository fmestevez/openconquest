//! The control bar's bookmark tabs and the panels they open.
//!
//! The bar ends in five tabs (`BMARK_*`, ids 1000-1004 in 0x511c00, left to
//! right): the object panel (ship's wheel), game file operations (disk,
//! `Filesys.cpp`), configuration (wrench, `Cfgssys.cpp`), sound
//! configuration (clef, `Cfgsys.cpp`) and mission objectives and quick help
//! (scroll, `Viewsys.cpp`). The other panels cover the bar above the tabs.
//! Positions are in bar pixels, as the original lays its controls out.

use std::rc::Rc;

use anyhow::Result;
use openconquest_formats::{parse_help_page, Archive, Bitmap, HelpItem, ResourceKind, Sequence};

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

/// The file panel's bookmarks (ids 0x3ed-0x3ef): what its wide button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileMode {
    #[default]
    Save,
    Load,
    Delete,
}

impl FileMode {
    const ALL: [FileMode; 3] = [FileMode::Save, FileMode::Load, FileMode::Delete];
}

/// The music the sound panel chooses (ids 0x3f3-0x3f6; `[0x5ca55e]` 1, 2,
/// 4, 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Music {
    None,
    #[default]
    Cd,
    Midi,
    Fx,
}

impl Music {
    const ALL: [Music; 4] = [Music::None, Music::Cd, Music::Midi, Music::Fx];
}

/// The panels' check boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    MessagePopup,
    ButtonAnimation,
    HelpString,
    GraphicTrails,
    Lighting,
    PackSaves,
    /// This port's (kept in `config.json`): left click selects, a double
    /// click every own ship of its kind in view, a click on nothing lets
    /// go, dragging draws a selection box, right click gives orders.
    /// Without it the mouse is the original's.
    ModernMouse,
    UnitReport,
    BattleSounds,
    AmbientSounds,
}

/// The panels' sliders. Each takes a whole position below its count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slider {
    GameSpeed,
    MouseScroll,
    KeyScroll,
    /// Message scroll speed.
    ChatSpeed,
    /// Number of message lines.
    ChatLines,
    MusicVolume,
    EffectsVolume,
    FxVolume,
    MainVolume,
}

/// Rows of a game speed in the table at 0x5bed7c: milliseconds per game
/// update (its first column), by speed 1-9.
const UPDATE_MS: [u32; 10] = [40, 13, 17, 20, 33, 40, 50, 60, 70, 80];

/// The settings the configuration and sound panels show, in the
/// original's units (the globals at 0x5ca542-0x5ca791), kept with this
/// port's `config.json`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// `SPEED_GAME` (0x5ca556), 1 fastest to 9; a new game has 5.
    pub game_speed: u8,
    /// Milliseconds per mouse scroll step (0x5ca54e: 200 - 19 x position).
    pub mouse_scroll: u32,
    /// Milliseconds per key scroll step (0x5ca552: 95 - 9 x position).
    pub key_scroll: u32,
    pub message_popup: bool,
    /// Message scroll speed (0x5ca784, 3-15).
    pub chat_speed: u8,
    /// Message lines (0x5ca783, 3-25).
    pub chat_lines: u8,
    /// The control bar on the left (0x5ca54a).
    pub bar_left: bool,
    pub button_animation: bool,
    pub help_string: bool,
    pub graphic_trails: bool,
    pub lighting: bool,
    pub pack_saves: bool,
    pub music: Music,
    pub unit_report: bool,
    pub battle_sounds: bool,
    pub ambient_sounds: bool,
    /// Volumes as slider positions 0-4000: the music's and the effects'
    /// are DirectSound attenuations (position - 4000, hundredths of a dB;
    /// 0x5ca568, 0x5ca56c); the wave (FX) and main ones the mixer's.
    pub music_volume: u16,
    pub effects_volume: u16,
    pub fx_volume: u16,
    pub main_volume: u16,
}

impl Default for Config {
    /// As the original starts (read from its memory).
    fn default() -> Self {
        Self {
            game_speed: 5,
            mouse_scroll: 59,
            key_scroll: 14,
            message_popup: true,
            chat_speed: 5,
            chat_lines: 10,
            bar_left: false,
            button_animation: true,
            help_string: true,
            graphic_trails: true,
            lighting: true,
            pack_saves: true,
            music: Music::Cd,
            unit_report: true,
            battle_sounds: true,
            ambient_sounds: true,
            // Half loud, as this port played music before.
            music_volume: 3398,
            effects_volume: 4000,
            fx_volume: 0,
            main_volume: 2000,
        }
    }
}

impl Config {
    /// Slider `s`'s position and count (the inverse formulas of 0x4faec0
    /// and of the panels' messages in 0x4fb540, 0x4fe130).
    pub fn slider(&self, s: Slider) -> (u32, u32) {
        match s {
            Slider::GameSpeed => (9u32.saturating_sub(self.game_speed as u32), 9),
            Slider::MouseScroll => (10u32.saturating_sub((self.mouse_scroll - 10) / 19), 10),
            Slider::KeyScroll => (10u32.saturating_sub((self.key_scroll - 5) / 9), 10),
            Slider::ChatSpeed => (15u32.saturating_sub(self.chat_speed as u32), 13),
            Slider::ChatLines => ((self.chat_lines as u32).saturating_sub(3), 23),
            Slider::MusicVolume => (self.music_volume as u32, 4001),
            Slider::EffectsVolume => (self.effects_volume as u32, 4001),
            Slider::FxVolume => (self.fx_volume as u32, 4001),
            Slider::MainVolume => (self.main_volume as u32, 4001),
        }
    }

    pub fn set_slider(&mut self, s: Slider, pos: u32) {
        let pos = pos.min(self.slider(s).1 - 1);
        match s {
            Slider::GameSpeed => self.game_speed = 9 - pos as u8,
            Slider::MouseScroll => self.mouse_scroll = 200 - 19 * pos,
            Slider::KeyScroll => self.key_scroll = 95 - 9 * pos,
            Slider::ChatSpeed => self.chat_speed = 15 - pos as u8,
            Slider::ChatLines => self.chat_lines = pos as u8 + 3,
            Slider::MusicVolume => self.music_volume = pos as u16,
            Slider::EffectsVolume => self.effects_volume = pos as u16,
            Slider::FxVolume => self.fx_volume = pos as u16,
            Slider::MainVolume => self.main_volume = pos as u16,
        }
    }

    pub fn check(&self, c: Check) -> bool {
        match c {
            Check::MessagePopup => self.message_popup,
            Check::ButtonAnimation => self.button_animation,
            Check::HelpString => self.help_string,
            Check::GraphicTrails => self.graphic_trails,
            Check::Lighting => self.lighting,
            Check::PackSaves => self.pack_saves,
            Check::ModernMouse => false,
            Check::UnitReport => self.unit_report,
            Check::BattleSounds => self.battle_sounds,
            Check::AmbientSounds => self.ambient_sounds,
        }
    }

    pub fn toggle(&mut self, c: Check) {
        let v = match c {
            Check::MessagePopup => &mut self.message_popup,
            Check::ButtonAnimation => &mut self.button_animation,
            Check::HelpString => &mut self.help_string,
            Check::GraphicTrails => &mut self.graphic_trails,
            Check::Lighting => &mut self.lighting,
            Check::PackSaves => &mut self.pack_saves,
            Check::ModernMouse => return,
            Check::UnitReport => &mut self.unit_report,
            Check::BattleSounds => &mut self.battle_sounds,
            Check::AmbientSounds => &mut self.ambient_sounds,
        };
        *v = !*v;
    }

    /// How much faster than at speed 5 the game runs: a game update every
    /// `UPDATE_MS[speed]` milliseconds instead of 40.
    pub fn speed(&self) -> f32 {
        40.0 / UPDATE_MS[self.game_speed.clamp(1, 9) as usize] as f32
    }

    /// Scrolling speed with the mouse at the edge, and with the keys,
    /// relative to the original's defaults.
    pub fn scroll(&self, keys: bool) -> f32 {
        if keys {
            14.0 / self.key_scroll.max(1) as f32
        } else {
            59.0 / self.mouse_scroll.max(1) as f32
        }
    }

    /// The gain of an attenuation slider position (hundredths of a dB
    /// below full).
    pub fn gain(pos: u16) -> f32 {
        10f32.powf((pos as f32 - 4000.0) / 2000.0)
    }
}

/// What a click on the bar hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PanelHit {
    Tab(Tab),
    FileMode(FileMode),
    /// A row of the saved games' list (its index in the list).
    SaveRow(usize),
    /// The list's arrows (-1 up, 1 down) and its scroll bar (the first row
    /// to show).
    ListScroll(i32),
    ListThumb(usize),
    /// The wide button: save, load or delete, by the mode.
    FileAction,
    Restart,
    MainMenu,
    Quit,
    Slider(Slider, u32),
    Check(Check),
    /// The control bar's side: left or right.
    BarLeft(bool),
    Music(Music),
    /// The help panel's up and down buttons.
    HelpUp,
    HelpDown,
    /// The help panel's tabs: mission objectives or quick help.
    QuickHelp(bool),
    /// A quick help hyperlink to a page.
    HelpLink(usize),
}

/// The state the panels show.
pub struct PanelView<'a> {
    pub tab: Tab,
    /// The saved games' names.
    pub saves: &'a [String],
    pub save_first: usize,
    pub selected_save: Option<usize>,
    pub file_mode: FileMode,
    /// The name field's text.
    pub name: &'a str,
    pub config: &'a Config,
    /// Modern mouse controls (right click gives orders).
    pub modern_mouse: bool,
    /// The music's tracks, as the sound panel lists them.
    pub tracks: &'a [String],
    /// Mission objectives text, as paragraphs (see `help_paragraphs`).
    pub help: &'a [String],
    /// The first objectives line shown.
    pub help_first: usize,
    /// The quick help page shown, if the quick help tab is open.
    pub help_page: Option<usize>,
    /// A button held down.
    pub pressed: Option<PanelHit>,
    /// Milliseconds since start, for the quick help's animations.
    pub time_ms: u64,
}

// Placement in bar pixels (bar-relative, as the original passes them
// through 0x512ab0).
const TABS_Y: u32 = 558;
const TAB_X: [u32; 5] = [0, 44, 85, 126, 167];
/// `CFILE_BKGND` (0x50a700) and `CCFG_BKGND` (0x4fa6f0, 0x4fc170): the
/// configuration panels' controls are placed from here.
const PANEL: (i32, i32) = (3, 5);

// Game file operations (0x5099f0).
const NAME_FIELD: (i32, i32, i32, i32) = (11, 14, 179, 17);
const SAVE_LIST: (i32, i32, i32, i32) = (14, 44, 140, 280);
const SAVE_ROW_H: i32 = 17;
const SAVE_ROWS: usize = (SAVE_LIST.3 / SAVE_ROW_H) as usize;
const LIST_UP: (i32, i32) = (172, 44);
const LIST_DOWN: (i32, i32) = (172, 310);
/// `CFILE_SLSL` and its `SLIDER_BB` thumb, 4 pixels in.
const LIST_BAR: (i32, i32, i32, i32) = (168, 66, 30, 244);
const BOOKMARK: (i32, i32, i32) = (5, 388, 66);
const FILE_ACTION: (i32, i32) = (9, 340);
const RESTART: (i32, i32) = (4, 461);
const MAIN_MENU: (i32, i32) = (4, 510);
const QUIT: (i32, i32) = (106, 510);

// Configuration (0x4faec0, labels 0x4fa6f0), from `PANEL`.
const CFG_SLIDERS: [(Slider, i32); 5] = [
    (Slider::GameSpeed, 63),
    (Slider::MouseScroll, 106),
    (Slider::KeyScroll, 149),
    (Slider::ChatSpeed, 255),
    (Slider::ChatLines, 298),
];
const SLIDER_X: i32 = 30;
const CFG_CHECK_X: i32 = 14;
/// Check boxes and their captions (string ids). The original's last slot
/// (y 510, "guaranteed sending", a network option) holds this port's
/// modern mouse switch.
const CFG_CHECKS: [(Check, i32, u32); 7] = [
    (Check::MessagePopup, 200, 0x7a),
    (Check::ButtonAnimation, 385, 0x73),
    (Check::HelpString, 410, 0x78),
    (Check::GraphicTrails, 435, 0x7e),
    (Check::Lighting, 460, 0x7f),
    (Check::PackSaves, 485, 0x80),
    (Check::ModernMouse, 510, 0),
];
/// Control bar left and right (radio, ids 0x3f1, 0x3f2).
const CFG_SIDES: [(bool, i32, u32); 2] = [(true, 335, 0x74), (false, 360, 0x75)];
/// The message speed and lines numbers (`CFGSCR_CHATIND`).
const CHAT_BOXES: [(Slider, i32); 2] = [(Slider::ChatSpeed, 247), (Slider::ChatLines, 290)];

// Sound configuration (0x4fc750, labels 0x4fc170), from `PANEL`.
const SND_RADIO: (i32, i32, i32) = (7, 20, 23);
const SND_CHECKS: [(Check, i32, u32); 3] = [
    (Check::UnitReport, 313, 0x83),
    (Check::BattleSounds, 336, 0x85),
    (Check::AmbientSounds, 359, 0x84),
];
const SND_SLIDERS: [(Slider, i32, i32, u32); 4] = [
    (Slider::MusicVolume, 405, 385, 0x87),
    (Slider::EffectsVolume, 440, 420, 0x89),
    (Slider::FxVolume, 475, 455, 0x86),
    (Slider::MainVolume, 510, 490, 0x88),
];
/// The play list (bar pixels), its arrows and scroll bar.
const TRACK_LIST: (i32, i32, i32, i32) = (13, 134, 150, 136);
const TRACK_ROWS: usize = (TRACK_LIST.3 / SAVE_ROW_H) as usize;
const TRACK_UP: (i32, i32) = (172, 137);
const TRACK_DOWN: (i32, i32) = (172, 246);
const TRACK_BAR: (i32, i32) = (168, 159);
/// Add, up, down and play under the list (from `PANEL`).
const TRACK_BUTTONS: [i32; 4] = [6, 54, 102, 150];
const TRACK_BUTTONS_Y: i32 = 268;

// The help panel (Viewsys 0x526930): its background, objectives text
// (`HELP_FONT`, 2 pixels between letters and lines, 188 wide, 445 high),
// up and down buttons and its two page tabs; the quick help pages are
// placed 6 pixels in from the bar's edge.
const HELP_BG: (u32, u32) = (4, 5);
const HELP_TEXT: (u32, u32, u32, u32) = (12, 11, 188, 445);
const HELP_LINE_GAP: u32 = 2;
const HELP_BUTTONS: [(u32, u32); 2] = [(10, 460), (108, 460)];
const HELP_TABS: [(u32, u32); 2] = [(3, 513), (103, 513)];
const HELP_PAGE_X: i32 = 6;

/// A font with its letter and line gaps (0x5470f0) in its colour variants.
struct Face {
    atlases: Vec<TextRenderer>,
    line_gap: i32,
}

impl Face {
    fn load(archive: &Archive, names: &[String], letters: u32, lines: i32) -> Option<Self> {
        let name = names
            .iter()
            .find(|n| archive.find(n, ResourceKind::Font).is_some())?;
        let atlases: Vec<_> = (0..2)
            .filter_map(|a| TextRenderer::load(archive, name, a).ok())
            .map(|t| t.with_spacing(letters))
            .collect();
        (!atlases.is_empty()).then_some(Self {
            atlases,
            line_gap: lines,
        })
    }

    /// Colour variant `atlas` (the first if the font lacks it, as 0x547710).
    fn atlas(&self, atlas: usize) -> &TextRenderer {
        self.atlases.get(atlas).unwrap_or(&self.atlases[0])
    }
}

/// Where text goes in its rectangle (0x547860): at an offset, or centred.
#[derive(Clone, Copy)]
enum Align {
    At(i32),
    Centre,
}

/// A quick help page's pictures (with their frames) and links.
struct HelpPage {
    pictures: Vec<(i32, i32, u64, Vec<Image>)>,
    links: Vec<(i32, i32, i32, i32, usize)>,
}

/// An image in its states, by a bitmap name plus `00`, `01`, `02`.
type States = Vec<Option<Image>>;

pub struct Panels {
    /// Per tab: normal, hover, active.
    tabs: Vec<[Option<Image>; 3]>,
    int_font: Option<Face>,
    /// `INT_FONT` as the file panel sets it (no letter gap, lines 2
    /// closer).
    button_font: Option<Face>,
    little_font: Option<Face>,
    system_font: Option<Face>,
    /// `SYSTEM_FONT` as the game keeps it (`[game+0xd64]`).
    number_font: Option<Face>,
    help_font: Option<TextRenderer>,
    strings: std::collections::HashMap<u32, String>,
    // Files.
    file_bg: Option<Image>,
    bookmarks: [States; 3],
    file_action: States,
    restart: States,
    main_menu: States,
    quit: States,
    list_bar: Option<Image>,
    list_up: States,
    list_down: States,
    list_thumb: Option<Image>,
    // Configuration and sound.
    settings_bg: Option<Image>,
    tracks_bg: Option<Image>,
    tracks_bar_bg: Option<Image>,
    tracks_bar: Option<Image>,
    track_buttons: [States; 4],
    sliders: Vec<(Slider, Option<Image>)>,
    chat_box: Option<Image>,
    check_box: Option<Image>,
    check_mark: Option<Image>,
    thumb: Option<Rc<Image>>,
    // Help.
    help_bg: Option<Image>,
    /// Up and down: normal, pressed, disabled.
    help_buttons: [States; 2],
    /// The objectives and quick help page tabs: normal, selected.
    help_tabs: [States; 2],
    pages: Vec<HelpPage>,
    // The message box.
    ask_bg: Option<Image>,
    /// `MESS_BUT01` OK, `MESS_BUT11` yes, `MESS_BUT12` no.
    ask_buttons: [States; 3],
    ask_font: Option<Face>,
    /// The buttons' captions' (gaps 1, 1; 0x512cd0).
    ask_button_font: Option<Face>,
}

/// A question the game asks (gamesys.cpp, 0x512af0 and 0x512f00): the
/// string, and whether it is answered yes or no (else OK).
pub struct AskView<'a> {
    pub text: &'a str,
    pub yes_no: bool,
    /// The button held down: yes or OK (true), no (false).
    pub pressed: Option<bool>,
}

/// The message box's buttons, from its top left: OK at (53, 152), yes at
/// (7, 152), no at (99, 152), 91 x 46 (0x512f00).
const ASK_OK: (i32, i32) = (53, 152);
const ASK_YES: (i32, i32) = (7, 152);
const ASK_NO: (i32, i32) = (99, 152);
const ASK_BUTTON: (u32, u32) = (91, 46);

impl Panels {
    pub fn load(data: &GameData, theme: char) -> Result<Self> {
        let names = |name: &str| [format!("{theme}{name}"), name.to_string()];
        let bitmap_in = |lib: &Archive, name: &str| -> Option<Image> {
            let raw = names(name)
                .iter()
                .find_map(|n| lib.read(n, ResourceKind::Bitmap).ok())?;
            let b = Bitmap::parse(&raw).ok()?;
            let pixels = b
                .pixels
                .iter()
                .map(|&i| Some(rgb(b.palette.0[i as usize])))
                .collect();
            Some(Image::new(b.width, b.height, pixels))
        };
        let bitmap = |name: &str| bitmap_in(&data.controls, name);
        let states =
            |name: &str| -> States { (0..3).map(|s| bitmap(&format!("{name}{s:02}"))).collect() };
        let sprite = |name: &str| -> Option<Image> {
            names(name)
                .iter()
                .find_map(|n| data.load_image(&data.controls, n, false).ok().flatten())
        };
        let tabs = ["COBJ", "FILE", "CFG", "VHELP", "VTASK"]
            .iter()
            .map(|t| [0, 1, 2].map(|s| bitmap(&format!("BMARK_{t}_{s:02}"))))
            .collect();
        let font = |name: &str, l: u32, g: i32| Face::load(&data.controls, &names(name), l, g);
        let sliders = [
            (Slider::GameSpeed, "CFGSCR_GSPD"),
            (Slider::MouseScroll, "CFGSCR_MSPD"),
            (Slider::KeyScroll, "CFGSCR_KSPD"),
            (Slider::ChatSpeed, "CFGSCR_CHATSPD"),
            (Slider::ChatLines, "CFGSCR_CHATSTR"),
            (Slider::MusicVolume, "CFGSND_MUSVOL"),
            (Slider::EffectsVolume, "CFGSND_EFFVOL"),
            (Slider::FxVolume, "CFGSND_WAVOL"),
            (Slider::MainVolume, "CFGSND_MAINVOL"),
        ]
        .into_iter()
        .map(|(s, n)| (s, bitmap(n)))
        .collect();
        let pages = (0..)
            .map_while(|i| {
                let raw = data
                    .controls
                    .read(&format!("SPHELP_{i:03}"), ResourceKind::Other(24))
                    .ok()?;
                Some(parse_help_page(&raw).unwrap_or_default())
            })
            .collect::<Vec<_>>();
        let pages = pages
            .iter()
            .map(|items| load_page(data, items, &bitmap_in))
            .collect();
        Ok(Self {
            tabs,
            int_font: font("INT_FONT", 1, 1),
            button_font: font("INT_FONT", 0, -2),
            little_font: font("LITTLE_FONT", 2, 1),
            system_font: font("SYSTEM_FONT", 2, 1),
            number_font: font("SYSTEM_FONT", 1, 0),
            help_font: TextRenderer::load(&data.controls, &format!("{theme}HELP_FONT"), 0)
                .ok()
                .map(|f| f.with_spacing(2)),
            strings: data.strings.clone(),
            file_bg: bitmap("CFILE_BKGND"),
            bookmarks: ["CFILE_BMRKSAVE", "CFILE_BMRKLOAD", "CFILE_BMRKDEL"].map(states),
            file_action: states("CFILE_SLDGAME"),
            restart: states("CFILE_RSTART"),
            main_menu: states("CFILE_MMENU"),
            quit: states("CFILE_QUIT"),
            list_bar: bitmap("CFILE_SLSL"),
            list_up: states("SLIDER_UP_"),
            list_down: states("SLIDER_DN_"),
            list_thumb: sprite("SLIDER_BB"),
            settings_bg: bitmap("CCFG_BKGND"),
            tracks_bg: bitmap("CFGSND_BKLIST"),
            tracks_bar_bg: bitmap("CFGSND_BKSLLIST"),
            tracks_bar: bitmap("CFGSND_SLLIST"),
            track_buttons: ["CFGSND_MARK_", "CFGSND_UP_", "CFGSND_DN_", "CFGSND_PLAY_"].map(states),
            sliders,
            chat_box: bitmap("CFGSCR_CHATIND"),
            check_box: bitmap("TRIGGER_BKGND"),
            check_mark: sprite("TRIGGER_BB"),
            thumb: sprite("TRACKBAR_BB").map(Rc::new),
            help_bg: bitmap("HVIEW_BKGND"),
            help_buttons: ["VIEW_BUTUP_", "VIEW_BUTDN_"].map(states),
            help_tabs: ["HVIEW_TASK", "HVIEW_HELP"].map(states),
            pages,
            ask_bg: bitmap("MESS_BKGND"),
            ask_buttons: ["MESS_BUT01", "MESS_BUT11", "MESS_BUT12"].map(states),
            ask_font: font("SYSTEM_FONT", 2, 5),
            ask_button_font: font("SYSTEM_FONT", 1, 1),
        })
    }

    /// The message box's size: `MESS_BKGND`'s, which the original centres
    /// on its screen (0x512af0).
    pub fn ask_size(&self) -> (u32, u32) {
        self.ask_bg
            .as_ref()
            .map_or((197, 205), |b| (b.width, b.height))
    }

    /// Draws a question with the box's top left at `origin` (window
    /// pixels): its text in `SYSTEM_FONT` (letter gap 2, line gap 5)
    /// centred in the box 10 pixels in, 130 high, and its buttons.
    pub fn draw_ask(&self, frame: &mut Frame, origin: (f32, f32), scale: f32, ask: &AskView) {
        let p = Painter {
            origin,
            scale,
            ts: scale.round().max(1.0) as u32,
        };
        let (x, y) = (0, 0);
        let Some(bg) = &self.ask_bg else {
            return;
        };
        p.blit(frame, bg, x, y);
        if let Some(f) = &self.ask_font {
            let rect = (x + 10, y + 10, bg.width as i32 - 20, 130);
            p.lines(frame, f, 0, rect, ask.text);
        }
        // Captioned with strings 10 OK, 11 Yes, 12 No (0x512cd0), centred;
        // pressed, the text a pixel left and down.
        type AskButton = (usize, (i32, i32), bool, (u32, &'static str));
        let buttons: &[AskButton] = if ask.yes_no {
            &[
                (1, ASK_YES, true, (11, "Yes")),
                (2, ASK_NO, false, (12, "No")),
            ]
        } else {
            &[(0, ASK_OK, true, (10, "OK"))]
        };
        for &(k, at, answer, (id, fallback)) in buttons {
            let pressed = ask.pressed == Some(answer);
            let Some(img) = state_image(&self.ask_buttons[k], usize::from(pressed)) else {
                continue;
            };
            let (bx, by) = (x + at.0, y + at.1);
            p.blit(frame, img, bx, by);
            let (w, h) = (img.width as i32, img.height as i32);
            let rect = if pressed {
                (bx, by, w - 2, h + 2)
            } else {
                (bx, by, w, h)
            };
            if let Some(f) = &self.ask_button_font {
                p.lines(frame, f, 0, rect, &self.string(id, fallback));
            }
        }
    }

    /// The answer under point `p` (original pixels from the box's top
    /// left): yes or OK true, no false.
    pub fn ask_hit(&self, yes_no: bool, p: (u32, u32)) -> Option<bool> {
        let (x, y) = (0, 0);
        let at =
            |(bx, by): (i32, i32)| ((x + bx) as u32, (y + by) as u32, ASK_BUTTON.0, ASK_BUTTON.1);
        if yes_no {
            if inside(p, at(ASK_YES)) {
                return Some(true);
            }
            if inside(p, at(ASK_NO)) {
                return Some(false);
            }
            None
        } else {
            inside(p, at(ASK_OK)).then_some(true)
        }
    }

    /// The game's string `id` (`fallback` without the executable).
    fn string(&self, id: u32, fallback: &'static str) -> String {
        self.strings
            .get(&id)
            .cloned()
            .unwrap_or_else(|| fallback.to_string())
    }

    /// The tab art in the bar picture is in this order: wheel, disk,
    /// wrench, clef, scroll. The resource names do not follow it (the disk
    /// is `BMARK_VTASK`), so they are matched by position.
    fn tab_image(&self, i: usize, state: usize) -> Option<&Image> {
        const ORDER: [usize; 5] = [0, 4, 1, 2, 3];
        self.tabs.get(ORDER[i]).and_then(|t| t[state].as_ref())
    }

    /// The number of quick help pages.
    pub fn help_pages(&self) -> usize {
        self.pages.len()
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
        let p = Painter {
            origin,
            scale,
            ts: scale.round().max(1.0) as u32,
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
                p.blit(frame, img, TAB_X[i] as i32, TABS_Y as i32);
            }
        }
        match view.tab {
            Tab::Object => {}
            Tab::File => self.draw_file(frame, &p, view),
            Tab::Settings => self.draw_settings(frame, &p, view),
            Tab::Sound => self.draw_sound(frame, &p, view),
            Tab::Help => self.draw_help(frame, &p, view, text),
        }
    }

    fn draw_file(&self, frame: &mut Frame, p: &Painter, view: &PanelView) {
        if let Some(bg) = &self.file_bg {
            p.blit(frame, bg, PANEL.0, PANEL.1);
        }
        // The name field and the list in `SYSTEM_FONT` (0x50a700 messages
        // 0x551, 0x530): the field's text in colour 0 from its left edge;
        // a row's 2 pixels in, colour 1, the chosen one colour 0.
        if let Some(f) = &self.system_font {
            let (x, y, w, h) = NAME_FIELD;
            p.text(
                frame,
                f,
                0,
                (x + 4, y, w - 4, h),
                view.name,
                Align::At(0),
                Align::Centre,
            );
            let rows = view.saves.iter().enumerate().skip(view.save_first);
            for (row, (i, name)) in rows.take(SAVE_ROWS).enumerate() {
                let atlas = usize::from(view.selected_save != Some(i));
                let y = SAVE_LIST.1 + row as i32 * SAVE_ROW_H;
                let rect = (SAVE_LIST.0, y, SAVE_LIST.2, SAVE_ROW_H);
                p.text(frame, f, atlas, rect, name, Align::At(2), Align::Centre);
            }
        }
        let held = |h: PanelHit| view.pressed == Some(h);
        // The list's arrows and scroll bar (0x531-0x533; 0x4e9b60: picture
        // 02 disabled, while the list fits, 01 pressed).
        let scrolls = view.saves.len() > SAVE_ROWS;
        for (imgs, at, hit) in [
            (&self.list_up, LIST_UP, PanelHit::ListScroll(-1)),
            (&self.list_down, LIST_DOWN, PanelHit::ListScroll(1)),
        ] {
            let state = if !scrolls { 2 } else { usize::from(held(hit)) };
            if let Some(img) = state_image(imgs, state) {
                p.blit(frame, img, at.0, at.1);
            }
        }
        if let Some(bar) = &self.list_bar {
            p.blit(frame, bar, LIST_BAR.0, LIST_BAR.1);
            if let Some(thumb) = &self.list_thumb {
                let max = view.saves.len().saturating_sub(SAVE_ROWS);
                let travel = LIST_BAR.3 - thumb.height as i32;
                let y = thumb_offset(view.save_first as u32, max as u32 + 1, travel);
                p.blit(frame, thumb, LIST_BAR.0 + 4, LIST_BAR.1 + y);
            }
        }
        // The bookmarks, captioned SAVE, LOAD, DELETE (message 0x54e:
        // `INT_FONT`, centred, 10 pixels down).
        for (i, mode) in FileMode::ALL.iter().enumerate() {
            let x = BOOKMARK.0 + i as i32 * BOOKMARK.2;
            let state = usize::from(*mode == view.file_mode);
            if let Some(img) = state_image(&self.bookmarks[i], state) {
                p.blit(frame, img, x, BOOKMARK.1);
                if let Some(f) = &self.button_font {
                    let caption = self.string(0x25 + i as u32, ["SAVE", "LOAD", "DELETE"][i]);
                    let rect = (x, BOOKMARK.1, img.width as i32, img.height as i32);
                    p.text(frame, f, 0, rect, &caption, Align::Centre, Align::At(10));
                }
            }
        }
        // The wide buttons (0x4e9ca0): the caption centred line by line,
        // colour 1 when disabled; pressed, picture 01 and the text a pixel
        // left and down.
        let action = match view.file_mode {
            FileMode::Save => (0x23, "Save game"),
            FileMode::Load => (0x24, "Load game"),
            FileMode::Delete => (0x22, "Delete game"),
        };
        let action_enabled = view.file_mode == FileMode::Save || view.selected_save.is_some();
        for (imgs, at, (id, fallback), hit, enabled) in [
            (
                &self.file_action,
                FILE_ACTION,
                action,
                PanelHit::FileAction,
                action_enabled,
            ),
            (
                &self.restart,
                RESTART,
                (0x20, "Restart mission"),
                PanelHit::Restart,
                true,
            ),
            (
                &self.main_menu,
                MAIN_MENU,
                (0x1f, "Main\nmenu"),
                PanelHit::MainMenu,
                true,
            ),
            (
                &self.quit,
                QUIT,
                (0x21, "Quit\nprogram"),
                PanelHit::Quit,
                true,
            ),
        ] {
            let pressed = enabled && held(hit);
            let Some(img) = state_image(imgs, usize::from(pressed)) else {
                continue;
            };
            p.blit(frame, img, at.0, at.1);
            let (w, h) = (img.width as i32, img.height as i32);
            let rect = if pressed {
                (at.0, at.1, w - 2, h + 2)
            } else {
                (at.0, at.1, w, h)
            };
            if let Some(f) = &self.button_font {
                let caption = self.string(id, fallback);
                p.lines(frame, f, usize::from(!enabled), rect, &caption);
            }
        }
    }

    fn draw_settings(&self, frame: &mut Frame, p: &Painter, view: &PanelView) {
        let o = PANEL;
        if let Some(bg) = &self.settings_bg {
            p.blit(frame, bg, o.0, o.1);
        }
        let w = self.settings_bg.as_ref().map_or(202, |b| b.width as i32);
        // Captions drawn on the background (0x4fa6f0): `INT_FONT` (gaps 1),
        // the sliders' centred, the check boxes' 10 pixels into a box from
        // x 34; MIN and MAX in `LITTLE_FONT` colour 1.
        if let Some(f) = &self.int_font {
            for (id, fallback, y, width) in [
                (0x70, "Game speed", 43, w),
                (0x71, "Mouse scroll speed", 86, w),
                (0x72, "Key scroll speed", 129, w),
                (0x7b, "Scroll speed", 235, w - 50),
                (0x7c, "Message lines", 278, w - 50),
            ] {
                let s = self.string(id, fallback);
                let rect = (o.0, o.1 + y, width, 22);
                p.text(frame, f, 0, rect, &s, Align::Centre, Align::Centre);
            }
            let checks = CFG_CHECKS.iter().map(|&(_, y, id)| (y, id));
            let sides = CFG_SIDES.iter().map(|&(_, y, id)| (y, id));
            for (y, id) in checks.chain(sides) {
                let s = if id == 0 {
                    "modern mouse".to_string()
                } else {
                    self.string(id, "")
                };
                let rect = (o.0 + 34, o.1 + y, w - 34, 20);
                p.text(frame, f, 0, rect, &s, Align::At(10), Align::Centre);
            }
        }
        if let Some(f) = &self.little_font {
            let (min, max) = (self.string(0x76, "MIN"), self.string(0x77, "MAX"));
            for (y, max_x) in [
                (58, w - 25),
                (101, w - 25),
                (144, w - 25),
                (250, w - 72),
                (293, w - 72),
            ] {
                p.text(
                    frame,
                    f,
                    1,
                    (o.0, o.1 + y, 25, 15),
                    &min,
                    Align::Centre,
                    Align::Centre,
                );
                let rect = (o.0 + max_x, o.1 + y, 25, 15);
                p.text(frame, f, 1, rect, &max, Align::Centre, Align::Centre);
            }
        }
        // The message speed and lines (0x4fa6f0; 0x4fb540 messages 0x526,
        // 0x528), in the game's `SYSTEM_FONT`.
        if let (Some(b), Some(f)) = (&self.chat_box, &self.number_font) {
            for (s, y) in CHAT_BOXES {
                p.blit(frame, b, o.0 + 157, o.1 + y);
                let n = match s {
                    Slider::ChatSpeed => view.config.chat_speed,
                    _ => view.config.chat_lines,
                };
                let rect = (o.0 + 157 + 5, o.1 + y, b.width as i32 - 10, b.height as i32);
                p.text(
                    frame,
                    f,
                    0,
                    rect,
                    &n.to_string(),
                    Align::Centre,
                    Align::At(4),
                );
            }
        }
        for (s, y) in CFG_SLIDERS {
            self.draw_slider(frame, p, view.config, s, (o.0 + SLIDER_X, o.1 + y));
        }
        for (c, y, _) in CFG_CHECKS {
            let on = match c {
                Check::ModernMouse => view.modern_mouse,
                c => view.config.check(c),
            };
            self.draw_check(frame, p, (o.0 + CFG_CHECK_X, o.1 + y), on);
        }
        for (left, y, _) in CFG_SIDES {
            let on = view.config.bar_left == left;
            self.draw_check(frame, p, (o.0 + CFG_CHECK_X, o.1 + y), on);
        }
    }

    fn draw_sound(&self, frame: &mut Frame, p: &Painter, view: &PanelView) {
        let o = PANEL;
        if let Some(bg) = &self.settings_bg {
            p.blit(frame, bg, o.0, o.1);
        }
        let w = self.settings_bg.as_ref().map_or(202, |b| b.width as i32);
        let music = view.config.music != Music::None;
        // Captions (0x4fc170): the choices' and check boxes' 10 pixels into
        // a box from x 27, the volumes' centred; no music, no music volume.
        if let Some(f) = &self.int_font {
            let radio = [
                (0x8a, "None"),
                (0x8d, "CD music"),
                (0x8c, "MIDI music"),
                (0x8b, "FX music"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (id, s))| (SND_RADIO.1 + i as i32 * SND_RADIO.2, id, s));
            let checks = SND_CHECKS.iter().map(|&(_, y, id)| (y, id, ""));
            for (y, id, fallback) in radio.chain(checks) {
                let s = self.string(id, fallback);
                let rect = (o.0 + 27, o.1 + y, w - 27, 20);
                p.text(frame, f, 0, rect, &s, Align::At(10), Align::Centre);
            }
            for (s, _, y, id) in SND_SLIDERS {
                if s == Slider::MusicVolume && !music {
                    continue;
                }
                let rect = (o.0, o.1 + y, w, 22);
                p.text(
                    frame,
                    f,
                    0,
                    rect,
                    &self.string(id, ""),
                    Align::Centre,
                    Align::Centre,
                );
            }
        }
        if let Some(f) = &self.little_font {
            let (min, max) = (self.string(0x76, "MIN"), self.string(0x77, "MAX"));
            for i in usize::from(!music)..4 {
                let y = o.1 + 400 + 35 * i as i32;
                p.text(
                    frame,
                    f,
                    1,
                    (o.0, y, 25, 15),
                    &min,
                    Align::Centre,
                    Align::Centre,
                );
                let rect = (o.0 + w - 25, y, 25, 15);
                p.text(frame, f, 1, rect, &max, Align::Centre, Align::Centre);
            }
        }
        if let Some(b) = &self.tracks_bg {
            p.blit(frame, b, o.0 + 7, o.1 + 127);
        }
        if let Some(b) = &self.tracks_bar_bg {
            p.blit(frame, b, o.0 + 165, o.1 + 128);
        }
        // The play list (message 0x534: 17 pixels in, colour 1).
        if let Some(f) = &self.system_font {
            for (row, name) in view.tracks.iter().take(TRACK_ROWS).enumerate() {
                let y = TRACK_LIST.1 + row as i32 * SAVE_ROW_H;
                let rect = (TRACK_LIST.0 + 17, y, TRACK_LIST.2 - 17, SAVE_ROW_H);
                p.text(frame, f, 1, rect, name, Align::At(0), Align::Centre);
            }
        }
        let scrolls = view.tracks.len() > TRACK_ROWS;
        for (imgs, at) in [(&self.list_up, TRACK_UP), (&self.list_down, TRACK_DOWN)] {
            if let Some(img) = state_image(imgs, if scrolls { 0 } else { 2 }) {
                p.blit(frame, img, at.0, at.1);
            }
        }
        if let Some(bar) = &self.tracks_bar {
            p.blit(frame, bar, TRACK_BAR.0, TRACK_BAR.1);
            if let Some(thumb) = &self.list_thumb {
                p.blit(frame, thumb, TRACK_BAR.0 + 4, TRACK_BAR.1);
            }
        }
        // Add, up, down need a chosen track; play needs tracks (state 0
        // disabled shows picture 02, state 1 picture 00; 0x4e9b60).
        for (i, x) in TRACK_BUTTONS.into_iter().enumerate() {
            let enabled = i == 3 && music && !view.tracks.is_empty();
            if let Some(img) = state_image(&self.track_buttons[i], if enabled { 0 } else { 2 }) {
                p.blit(frame, img, o.0 + x, o.1 + TRACK_BUTTONS_Y);
            }
        }
        for (i, m) in Music::ALL.iter().enumerate() {
            let y = o.1 + SND_RADIO.1 + i as i32 * SND_RADIO.2;
            self.draw_check(frame, p, (o.0 + SND_RADIO.0, y), view.config.music == *m);
        }
        for (c, y, _) in SND_CHECKS {
            self.draw_check(frame, p, (o.0 + SND_RADIO.0, o.1 + y), view.config.check(c));
        }
        for (s, y, _, _) in SND_SLIDERS {
            if s == Slider::MusicVolume && !music {
                continue;
            }
            self.draw_slider(frame, p, view.config, s, (o.0 + SLIDER_X, o.1 + y));
        }
    }

    fn draw_help(&self, frame: &mut Frame, p: &Painter, view: &PanelView, text: &TextRenderer) {
        if let Some(bg) = &self.help_bg {
            p.blit(frame, bg, HELP_BG.0 as i32, HELP_BG.1 as i32);
        }
        let can = match view.help_page {
            Some(page) => {
                if let Some(pg) = self.pages.get(page) {
                    for (x, y, delay, frames) in &pg.pictures {
                        let i = if frames.len() > 1 && *delay > 0 {
                            (view.time_ms / delay) as usize % frames.len()
                        } else {
                            0
                        };
                        p.blit(frame, &frames[i], HELP_PAGE_X + x, *y);
                    }
                }
                [page > 0, page + 1 < self.pages.len()]
            }
            None => {
                let font = self.help_font.as_ref().unwrap_or(text);
                let lines = help_lines(font, view.help);
                let rows = self.help_rows();
                let pitch = font.line_height() + HELP_LINE_GAP;
                for (i, l) in lines.iter().skip(view.help_first).take(rows).enumerate() {
                    let (px, py) =
                        p.at(HELP_TEXT.0 as i32, (HELP_TEXT.1 + i as u32 * pitch) as i32);
                    font.draw(frame, px as i32, py as i32, p.ts, l);
                }
                [view.help_first > 0, view.help_first + rows < lines.len()]
            }
        };
        for (k, &(x, y)) in HELP_BUTTONS.iter().enumerate() {
            let hit = [PanelHit::HelpUp, PanelHit::HelpDown][k];
            let state = if !can[k] {
                2
            } else {
                usize::from(view.pressed == Some(hit))
            };
            if let Some(img) = state_image(&self.help_buttons[k], state) {
                p.blit(frame, img, x as i32, y as i32);
            }
        }
        // The open page's tab is drawn in state 01 (0x527ddf).
        let quick = view.help_page.is_some();
        for (k, &(x, y)) in HELP_TABS.iter().enumerate() {
            let open = (k == 1) == quick;
            if let Some(img) = state_image(&self.help_tabs[k], usize::from(open)) {
                p.blit(frame, img, x as i32, y as i32);
            }
        }
    }

    /// A slider (the trackbar, strack.cpp): its track, and the
    /// `TRACKBAR_BB` thumb at its position (0x552400).
    fn draw_slider(&self, frame: &mut Frame, p: &Painter, c: &Config, s: Slider, at: (i32, i32)) {
        let Some(track) = self.track(s) else { return };
        p.blit(frame, track, at.0, at.1);
        if let Some(thumb) = &self.thumb {
            let (pos, count) = c.slider(s);
            let travel = track.width as i32 - thumb.width as i32;
            p.blit(frame, thumb, at.0 + thumb_offset(pos, count, travel), at.1);
        }
    }

    /// A check box (0x4e9e30): `TRIGGER_BKGND`, `TRIGGER_BB` over it when
    /// set.
    fn draw_check(&self, frame: &mut Frame, p: &Painter, at: (i32, i32), on: bool) {
        if let Some(b) = &self.check_box {
            p.blit(frame, b, at.0, at.1);
        }
        if let (true, Some(m)) = (on, &self.check_mark) {
            p.blit(frame, m, at.0, at.1);
        }
    }

    fn track(&self, s: Slider) -> Option<&Image> {
        self.sliders
            .iter()
            .find(|(t, _)| *t == s)
            .and_then(|(_, i)| i.as_ref())
    }

    /// How many objectives lines the help panel shows at once.
    pub fn help_rows(&self) -> usize {
        let pitch = self
            .help_font
            .as_ref()
            .map_or(20, |f| f.line_height() + HELP_LINE_GAP);
        ((HELP_TEXT.3 + HELP_LINE_GAP) / pitch.max(1)) as usize
    }

    /// The objectives text's lines, wrapped as the panel shows them.
    pub fn help_line_count(&self, help: &[String], text: &TextRenderer) -> usize {
        help_lines(self.help_font.as_ref().unwrap_or(text), help).len()
    }

    /// How many saved games the file panel's list shows at once.
    pub fn save_rows() -> usize {
        SAVE_ROWS
    }

    /// The bar-relative rectangles of a slider's tracks, by panel.
    fn sliders_of(&self, tab: Tab, c: &Config) -> Vec<(Slider, (i32, i32, i32, i32))> {
        let place = |s: Slider, y: i32| {
            let (w, h) = self
                .track(s)
                .map_or((141, 14), |t| (t.width as i32, t.height as i32));
            (s, (PANEL.0 + SLIDER_X, PANEL.1 + y, w, h))
        };
        match tab {
            Tab::Settings => CFG_SLIDERS.iter().map(|&(s, y)| place(s, y)).collect(),
            Tab::Sound => SND_SLIDERS
                .iter()
                .filter(|&&(s, ..)| s != Slider::MusicVolume || c.music != Music::None)
                .map(|&(s, y, ..)| place(s, y))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// What lies at bar-relative point `p` with the panels showing `view`.
    pub fn hit(&self, view: &PanelView, p: (u32, u32)) -> Option<PanelHit> {
        for (i, t) in Tab::ALL.iter().enumerate() {
            if inside(p, (TAB_X[i], TABS_Y, tab_width(i), 42)) {
                return Some(PanelHit::Tab(*t));
            }
        }
        let q = (p.0 as i32, p.1 as i32);
        let within =
            |(x, y, w, h): (i32, i32, i32, i32)| q.0 >= x && q.1 >= y && q.0 < x + w && q.1 < y + h;
        let size = |imgs: &States, d: (i32, i32)| {
            state_image(imgs, 0).map_or(d, |i| (i.width as i32, i.height as i32))
        };
        let check_at = |(x, y): (i32, i32)| within((x, y, 20, 20));
        match view.tab {
            Tab::Object => None,
            Tab::Help => {
                for (k, &(x, y)) in HELP_TABS.iter().enumerate() {
                    if inside(p, (x, y, 100, 44)) {
                        return Some(PanelHit::QuickHelp(k == 1));
                    }
                }
                let hits = [PanelHit::HelpUp, PanelHit::HelpDown];
                if let Some(h) = HELP_BUTTONS
                    .iter()
                    .zip(hits)
                    .find(|(&(x, y), _)| inside(p, (x, y, 90, 46)))
                    .map(|(_, h)| h)
                {
                    return Some(h);
                }
                // A link counts with its edges (0x526930 message 0x61).
                let page = self.pages.get(view.help_page?)?;
                page.links
                    .iter()
                    .find(|&&(x, y, w, h, target)| {
                        let x = x + HELP_PAGE_X;
                        target < self.pages.len()
                            && (x..=x + w).contains(&q.0)
                            && (y..=y + h).contains(&q.1)
                    })
                    .map(|l| PanelHit::HelpLink(l.4))
            }
            Tab::File => {
                for (i, mode) in FileMode::ALL.iter().enumerate() {
                    let x = BOOKMARK.0 + i as i32 * BOOKMARK.2;
                    let (w, h) = size(&self.bookmarks[i], (66, 70));
                    if within((x, BOOKMARK.1, w, h)) {
                        return Some(PanelHit::FileMode(*mode));
                    }
                }
                for (imgs, at, hit) in [
                    (&self.file_action, FILE_ACTION, PanelHit::FileAction),
                    (&self.restart, RESTART, PanelHit::Restart),
                    (&self.main_menu, MAIN_MENU, PanelHit::MainMenu),
                    (&self.quit, QUIT, PanelHit::Quit),
                    (&self.list_up, LIST_UP, PanelHit::ListScroll(-1)),
                    (&self.list_down, LIST_DOWN, PanelHit::ListScroll(1)),
                ] {
                    let (w, h) = size(imgs, (22, 22));
                    if within((at.0, at.1, w, h)) {
                        return Some(hit);
                    }
                }
                if within(LIST_BAR) {
                    let max = view.saves.len().saturating_sub(SAVE_ROWS);
                    let thumb = self.list_thumb.as_ref().map_or(22, |t| t.height as i32);
                    let pos = thumb_position(q.1 - LIST_BAR.1, max as u32 + 1, LIST_BAR.3, thumb);
                    return Some(PanelHit::ListThumb(pos as usize));
                }
                if within(SAVE_LIST) {
                    let row = ((q.1 - SAVE_LIST.1) / SAVE_ROW_H) as usize + view.save_first;
                    return (row < view.saves.len()).then_some(PanelHit::SaveRow(row));
                }
                None
            }
            Tab::Settings | Tab::Sound => {
                for (s, r) in self.sliders_of(view.tab, view.config) {
                    // The thumb reaches 2 pixels above and below the track.
                    if within((r.0, r.1 - 2, r.2, r.3 + 4)) {
                        return Some(PanelHit::Slider(
                            s,
                            self.slider_at(s, view.config, q.0 - r.0),
                        ));
                    }
                }
                if view.tab == Tab::Settings {
                    let x = PANEL.0 + CFG_CHECK_X;
                    if let Some(&(c, ..)) = CFG_CHECKS.iter().find(|c| check_at((x, PANEL.1 + c.1)))
                    {
                        return Some(PanelHit::Check(c));
                    }
                    CFG_SIDES
                        .iter()
                        .find(|c| check_at((x, PANEL.1 + c.1)))
                        .map(|&(left, ..)| PanelHit::BarLeft(left))
                } else {
                    let x = PANEL.0 + SND_RADIO.0;
                    for (i, m) in Music::ALL.iter().enumerate() {
                        if check_at((x, PANEL.1 + SND_RADIO.1 + i as i32 * SND_RADIO.2)) {
                            return Some(PanelHit::Music(*m));
                        }
                    }
                    SND_CHECKS
                        .iter()
                        .find(|c| check_at((x, PANEL.1 + c.1)))
                        .map(|&(c, ..)| PanelHit::Check(c))
                }
            }
        }
    }

    /// The position of slider `s` for a cursor `x` pixels from its track's
    /// left edge (0x5524a0; for dragging, so not limited to the track).
    fn slider_at(&self, s: Slider, c: &Config, x: i32) -> u32 {
        let track = self.track(s).map_or(141, |t| t.width as i32);
        let thumb = self.thumb.as_ref().map_or(20, |t| t.width as i32);
        thumb_position(x, c.slider(s).1, track, thumb)
    }

    /// The position of slider `s` for a cursor at bar x `x` (dragging).
    pub fn slider_value(&self, s: Slider, c: &Config, x: f32) -> u32 {
        let left = PANEL.0 + SLIDER_X;
        self.slider_at(s, c, x as i32 - left)
    }

    /// Whether `tab` covers the minimap and object panel.
    pub fn covers_bar(tab: Tab) -> bool {
        tab != Tab::Object
    }
}

/// Loads a quick help page's pictures (sequences frame by frame) and links.
fn load_page(
    data: &GameData,
    items: &[HelpItem],
    bitmap: &dyn Fn(&Archive, &str) -> Option<Image>,
) -> HelpPage {
    let mut page = HelpPage {
        pictures: Vec::new(),
        links: Vec::new(),
    };
    for item in items {
        match item {
            HelpItem::Picture {
                archive,
                kind,
                name,
                x,
                y,
                delay,
                ..
            } => {
                let Some(lib) = (match archive {
                    0 => Some(&data.controls),
                    _ => data.menus.as_ref(),
                }) else {
                    continue;
                };
                let sprite = |n: &str| data.load_image(lib, n, false).ok().flatten();
                let frames: Vec<Image> = match kind {
                    1 => bitmap(lib, name).into_iter().collect(),
                    7 => lib
                        .find(name, ResourceKind::Sequence)
                        .and_then(|e| Sequence::parse(&lib.read_entry(e).ok()?, e).ok())
                        .map(|s| s.frame_names(lib))
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|n| sprite(n))
                        .collect(),
                    _ => sprite(name).into_iter().collect(),
                };
                if !frames.is_empty() {
                    page.pictures
                        .push((*x as i32, *y as i32, *delay as u64, frames));
                }
            }
            HelpItem::Link { x, y, w, h, target } => {
                page.links.push((*x, *y, *w, *h, *target as usize))
            }
        }
    }
    page
}

/// Draws in bar pixels.
struct Painter {
    origin: (f32, f32),
    scale: f32,
    /// Text magnification (whole pixels).
    ts: u32,
}

impl Painter {
    fn at(&self, x: i32, y: i32) -> (f32, f32) {
        (
            self.origin.0 + x as f32 * self.scale,
            self.origin.1 + y as f32 * self.scale,
        )
    }

    fn blit(&self, frame: &mut Frame, img: &Image, x: i32, y: i32) {
        let (px, py) = self.at(x, y);
        blit_scaled(
            frame,
            img,
            (0, 0, img.width, img.height),
            px,
            py,
            self.scale,
        );
    }

    /// Text in a rectangle (mfcfnt.cpp 0x547860): at an offset or centred
    /// each way, by its width and its tallest glyph.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        frame: &mut Frame,
        face: &Face,
        atlas: usize,
        (x, y, w, h): (i32, i32, i32, i32),
        s: &str,
        ax: Align,
        ay: Align,
    ) {
        let font = face.atlas(atlas);
        let dx = match ax {
            Align::At(d) => d,
            Align::Centre => (w - font.measure(s) as i32) / 2,
        };
        let dy = match ay {
            Align::At(d) => d,
            Align::Centre => (h - font.height(s) as i32) / 2,
        };
        let (px, py) = self.at(x + dx, y + dy);
        font.draw(frame, px as i32, py as i32, self.ts, s);
    }

    /// Lines centred one by one, the block centred (0x5479f0): each line
    /// as tall as its tallest glyph, plus the face's line gap.
    fn lines(
        &self,
        frame: &mut Frame,
        face: &Face,
        atlas: usize,
        rect: (i32, i32, i32, i32),
        s: &str,
    ) {
        let font = face.atlas(atlas);
        let lines: Vec<&str> = s.split('\n').collect();
        let total: i32 = lines.iter().map(|l| font.height(l) as i32).sum::<i32>()
            + face.line_gap * (lines.len() as i32 - 1);
        let mut y = rect.1 + (rect.3 - total) / 2;
        for l in lines {
            self.text(
                frame,
                face,
                atlas,
                (rect.0, y, rect.2, 0),
                l,
                Align::Centre,
                Align::At(0),
            );
            y += font.height(l) as i32 + face.line_gap;
        }
    }
}

fn state_image(imgs: &States, state: usize) -> Option<&Image> {
    imgs.get(state)
        .and_then(Option::as_ref)
        .or_else(|| imgs.first().and_then(Option::as_ref))
}

/// The thumb's offset along a track `travel` pixels long for position
/// `pos` of `count` (0x552400): the ends at the ends, otherwise the middle
/// of the position's share.
pub(crate) fn thumb_offset(pos: u32, count: u32, travel: i32) -> i32 {
    if pos == 0 || count == 0 {
        0
    } else if pos + 1 >= count {
        travel
    } else {
        (travel as f32 / count as f32 * (pos as f32 + 0.5)) as i32
    }
}

/// The position under a cursor `x` pixels along a track (0x5524a0): the
/// ends within half a thumb of them.
pub(crate) fn thumb_position(x: i32, count: u32, track: i32, thumb: i32) -> u32 {
    let travel = (track - thumb).max(1);
    if x <= thumb / 2 || count <= 1 {
        0
    } else if x >= travel + thumb / 2 {
        count - 1
    } else {
        (((x - thumb / 2) as f32 * count as f32 / travel as f32) as u32).min(count - 1)
    }
}

fn tab_width(i: usize) -> u32 {
    if i == 0 {
        44
    } else {
        41
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
    use super::*;

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

    #[test]
    fn sliders_round_trip_the_originals_values() {
        let mut c = Config::default();
        // As the original shows its defaults.
        assert_eq!(c.slider(Slider::GameSpeed), (4, 9));
        assert_eq!(c.slider(Slider::MouseScroll), (8, 10));
        assert_eq!(c.slider(Slider::KeyScroll), (9, 10));
        assert_eq!(c.slider(Slider::ChatSpeed), (10, 13));
        assert_eq!(c.slider(Slider::ChatLines), (7, 23));
        for s in [
            Slider::GameSpeed,
            Slider::MouseScroll,
            Slider::KeyScroll,
            Slider::ChatSpeed,
            Slider::ChatLines,
        ] {
            for pos in 0..c.slider(s).1 {
                c.set_slider(s, pos);
                assert_eq!(c.slider(s).0, pos, "{s:?}");
            }
        }
        c.game_speed = 5;
        assert_eq!(c.speed(), 1.0);
        c.game_speed = 9;
        assert_eq!(c.speed(), 0.5);
    }

    #[test]
    fn thumbs_sit_in_the_middle_of_their_share() {
        assert_eq!(thumb_offset(0, 9, 121), 0);
        assert_eq!(thumb_offset(8, 9, 121), 121);
        assert_eq!(thumb_offset(4, 9, 121), 60);
        for pos in 0..9 {
            let x = thumb_offset(pos, 9, 121) + 10;
            assert_eq!(thumb_position(x, 9, 141, 20), pos);
        }
    }

    fn view<'a>(tab: Tab, config: &'a Config, saves: &'a [String]) -> PanelView<'a> {
        PanelView {
            tab,
            saves,
            save_first: 0,
            selected_save: None,
            file_mode: FileMode::Save,
            name: "",
            config,
            modern_mouse: false,
            tracks: &[],
            help: &[],
            help_first: 0,
            help_page: Some(0),
            pressed: None,
            time_ms: 0,
        }
    }

    /// Needs `OPENCONQUEST_DATA`.
    #[test]
    fn clicks_find_tabs_and_controls() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let panels = Panels::load(&data, '0').unwrap();
        let config = Config::default();
        let saves = ["one".to_string(), "two".to_string()];
        let v = |tab| view(tab, &config, &saves);
        assert_eq!(
            panels.hit(&v(Tab::Object), (10, 570)),
            Some(PanelHit::Tab(Tab::Object))
        );
        assert_eq!(
            panels.hit(&v(Tab::Object), (60, 570)),
            Some(PanelHit::Tab(Tab::File))
        );
        assert_eq!(
            panels.hit(&v(Tab::Object), (200, 570)),
            Some(PanelHit::Tab(Tab::Help))
        );
        assert_eq!(panels.hit(&v(Tab::Object), (100, 300)), None);
        // Files: the list's second row, the LOAD bookmark, the wide button.
        assert_eq!(
            panels.hit(&v(Tab::File), (40, 44 + 17 + 5)),
            Some(PanelHit::SaveRow(1))
        );
        assert_eq!(panels.hit(&v(Tab::File), (40, 44 + 3 * 17)), None);
        assert_eq!(
            panels.hit(&v(Tab::File), (80, 400)),
            Some(PanelHit::FileMode(FileMode::Load))
        );
        assert_eq!(
            panels.hit(&v(Tab::File), (100, 360)),
            Some(PanelHit::FileAction)
        );
        assert_eq!(panels.hit(&v(Tab::File), (150, 530)), Some(PanelHit::Quit));
        // Configuration: the key scroll slider's right end, check boxes.
        match panels.hit(&v(Tab::Settings), (33 + 140, 154 + 5)) {
            Some(PanelHit::Slider(Slider::KeyScroll, 9)) => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(
            panels.hit(&v(Tab::Settings), (20, 5 + 200 + 5)),
            Some(PanelHit::Check(Check::MessagePopup))
        );
        assert_eq!(
            panels.hit(&v(Tab::Settings), (20, 5 + 335 + 5)),
            Some(PanelHit::BarLeft(true))
        );
        assert_eq!(
            panels.hit(&v(Tab::Settings), (20, 5 + 510 + 5)),
            Some(PanelHit::Check(Check::ModernMouse))
        );
        // Sound: CD music, the effects volume's left end.
        assert_eq!(
            panels.hit(&v(Tab::Sound), (12, 5 + 43 + 5)),
            Some(PanelHit::Music(Music::Cd))
        );
        assert!(matches!(
            panels.hit(&v(Tab::Sound), (35, 5 + 440 + 5)),
            Some(PanelHit::Slider(Slider::EffectsVolume, 0))
        ));
        // Quick help: nine pages; the contents page links to page 1.
        assert_eq!(panels.help_pages(), 9);
        assert_eq!(
            panels.hit(&v(Tab::Help), (6 + 60, 57 + 10)),
            Some(PanelHit::HelpLink(1))
        );
        assert_eq!(
            panels.hit(&v(Tab::Help), (120, 520)),
            Some(PanelHit::QuickHelp(true))
        );
    }
}
