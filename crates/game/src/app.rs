//! Window, input handling and the frame loop.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::assets::GameData;
use crate::audio::Audio;
use crate::commands::{menu, Command, Page};
use crate::economy;
use crate::magic::{self, Spell, Target, Temple};
use crate::menus::{
    scroll_positions, step_position, Backdrop, Button, MenuHit, MenuView, Menus, Scroller,
};
use crate::music::Music;
use crate::objects::Caches;
use crate::objects::{Resource, SHIP_NAMES};
use crate::panels::{self, Check, FileMode, PanelHit, PanelView, Panels, Slider, Tab};
use crate::render::{hit_test, Frame, MinimapRect};
use crate::save::SaveGame;
use crate::scene::{Order, Scene};
use crate::text::TextRenderer;
use crate::ui::{GameView, Interface};
use crate::view::{Camera, Screen};

/// Keyboard scroll speed in original pixels per second.
const SCROLL_SPEED: f32 = 1400.0;
/// Longest name the file panel's field takes.
const SAVE_NAME_MAX: usize = 40;
/// Width of the screen border that triggers edge scrolling, in screen pixels.
const EDGE_MARGIN: f64 = 8.0;
const ZOOM_STEP: f32 = 1.15;
/// Cursor travel (screen pixels) after which a right-button press is a pan
/// rather than a click.
const CLICK_SLOP: f64 = 5.0;
/// -1, 0 or 1 by the sign of `v` (`f32::signum` gives 1 for 0, which
/// would scroll the map with no key held).
fn direction(v: f32) -> f32 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// How long status messages stay on screen.
const STATUS_TIME: Duration = Duration::from_secs(3);

/// Window pixels the mouse must move before a click becomes a box.
const BOX_THRESHOLD: f64 = 6.0;
/// Two clicks on a ship within this make a double click (modern mouse).
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Where the `k`-th ship of a group goes relative to the point it is sent
/// to: rings of places 48 pixels apart.
fn group_offset(k: usize) -> (f32, f32) {
    if k == 0 {
        return (0.0, 0.0);
    }
    let ring = ((k as f32).sqrt().ceil()) as i32;
    let around = (8 * ring) as usize;
    let a = (k % around) as f32 / around as f32 * std::f32::consts::TAU;
    let r = 48.0 * ring as f32;
    (r * a.cos(), r * a.sin())
}

/// The first of the ten flag frames of each player (nation x 4 + slot;
/// table 0x5c2560), in `M_FLAGS`.
const FLAG_STARTS: [u32; 16] = [
    10, 50, 90, 130, 20, 60, 100, 140, 30, 70, 110, 150, 0, 40, 80, 120,
];

/// The title screen's buttons, in the original's order.
const TITLE_BUTTONS: [&str; 6] = [
    "Best Conquerors",
    "Start scenario",
    "Load game",
    "Custom missions",
    "MultiPlayer game",
    "Quit game",
];

/// "Best Conquerors" (string 1042), the end screen's table's title.
const STRING_BEST: &str = "Best Conquerors";

/// The end screen's statistics' labels (strings 1047-1058), their dots
/// cut off by the box they are drawn in.
const STAT_LABELS: [&str; 12] = [
    "Enemy ships destroyed . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .",
    "Enemy ships boarded . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .",
    "Enemy terrain objects destroyed . . . . . . . . . . . . . . . . . . . . . . . . . . . . .",
    "Monsters killed . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .",
    "Monsters terrain objects destroyed . . . . . . . . . . . . . . . . . . . . . . . . . . . .",
    "Ships lost . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .",
    "Terrain objects lost . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . .",
    "Fish caught . . . . . . . . . . . . . . . . . .",
    "Money earned . . . . . . . . . . . . . . . . .",
    "Money left . . . . . . . . . . . . . . . . . .",
    "Treasures found . . . . . . . . . . . . . . . .",
    "Total score . . . . . . . . . . . . . . . . . .",
];

/// What a front-end key binding sends (Sinput.cpp's records, `sinput`).
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyMsg {
    /// A button's accelerator down and up (its record's key bytes,
    /// 0x4ea2d0; _sbutton.cpp messages 0x2c and 0x2e): held down, then
    /// acting.
    Down(&'static str),
    Up(&'static str),
    /// A key the screen binds itself (0x4e9840): the command of the button
    /// it acts for, on the key's going down.
    Command(&'static str),
    /// A name edit's keys (_sedit.cpp 0x54f040 message 8), by the message
    /// each sends it: Esc 0x35, Backspace 0x36, Delete 0x37, Insert 0x38,
    /// Enter 0x39, Up 0x3a, Down 0x3b, Right 0x3c, Left 0x3d, Home 0x3e,
    /// End 0x3f.
    Edit(u8),
    /// Custom missions' kind pop-up (0x52ebe0): its Esc (0x1d5), its
    /// list's Enter, and its Up and Down buttons down and up.
    PopupClose,
    PopupChoose,
    PopupStep(i32),
    PopupStepEnd,
    /// The map tick's Alt+A down and up (0x52c5c0: its accelerator, as a
    /// button's).
    TickDown,
    TickUp,
}

/// A binding a screen makes: flags, keys, priority, and its messages down
/// and up (`sinput::Keys::add`).
type Binding = (u8, (u8, u8), u32, KeyMsg, Option<KeyMsg>);

/// The end screen's name edit, over the row a won campaign mission went
/// into (Startrpt.cpp 0x539350: type 6, 39 characters, text list + 0x2a8).
#[derive(Debug, Clone)]
struct ScoreEntry {
    /// The campaign type and the row (+ 0x2b0, + 0x2ac).
    kind: usize,
    row: usize,
    text: String,
    edit: NameEdit,
}

/// A single-line name edit (_sedit.cpp 0x54f040, flags 1: the connect
/// screen's and the end screen's), in the fields the original keeps. It is
/// made active with the caret before the first character (message 2
/// zeroes the caret and scroll fields).
#[derive(Debug, Clone, Copy, Default)]
struct NameEdit {
    /// State 1: its keys bound and the caret drawn.
    active: bool,
    /// The first column shown (+ 0x146), the caret's column from it
    /// (+ 0x1c6) and its x in the box (+ 0x1ba).
    first: usize,
    col: usize,
    x: f32,
}

impl NameEdit {
    fn new() -> Self {
        Self {
            active: true,
            ..Default::default()
        }
    }

    /// The caret to column `c` of `text` (0x54e5f0, single line): up to
    /// the text's end; the first column shown back to it, then on while the
    /// characters from it to the caret, each its width and the gap
    /// (`band`), reach the box's width.
    fn move_to(&mut self, text: &str, c: usize, band: &dyn Fn(&str) -> f32, width: f32) {
        let c = c.min(text.chars().count());
        self.first = self.first.min(c);
        while band(columns(text, self.first, c)) >= width {
            self.first += 1;
        }
        self.x = band(columns(text, self.first, c));
        self.col = c - self.first;
    }

    /// The caret's column in the text.
    fn at(&self) -> usize {
        self.first + self.col
    }

    /// A left click `px` across the box (0x54e430): the caret before the
    /// first character from the first shown whose band holds it, past the
    /// end at the end. The column found is stored as it is, from the
    /// text's start, in the field counted from the first shown (equal
    /// while nothing is scrolled).
    fn click(&mut self, text: &str, px: f32, band: &dyn Fn(&str) -> f32) {
        let n = text.chars().count();
        let hit = (self.first..n).find(|&i| {
            band(columns(text, self.first, i)) <= px && px < band(columns(text, self.first, i + 1))
        });
        let c = hit.unwrap_or(n);
        self.col = c;
        self.x = band(columns(text, self.first, c));
    }
}

/// Characters `a` to `b` (not included) of `text`.
fn columns(text: &str, a: usize, b: usize) -> &str {
    let at = |k: usize| text.char_indices().nth(k).map_or(text.len(), |(i, _)| i);
    if b <= a {
        return "";
    }
    &text[at(a)..at(b)]
}

/// The name edits' keys (scan code, message), in the order 0x54f040 binds
/// them (priority 0xff, repeating and exclusive).
const EDIT_KEYS: [(u8, u8); 11] = [
    (0x01, 0x35),
    (0xc8, 0x3a),
    (0xd0, 0x3b),
    (0xcd, 0x3c),
    (0xcb, 0x3d),
    (0xc7, 0x3e),
    (0xcf, 0x3f),
    (0x1c, 0x39),
    (0xd2, 0x38),
    (0x0e, 0x36),
    (0xd3, 0x37),
];

/// The name edits' font: MH variant 2, characters a pixel apart.
const EDIT_STYLE: crate::menus::Style = crate::menus::Style {
    font: "MH",
    variant: 2,
    spacing: 1,
};

/// The connect screen's name edit box (0x538440: (30 + 13, 225 + 31),
/// 0x1a4 x 0x18) and the end screen's width (0x539350).
const CONNECT_EDIT: crate::menus::Rect = (43.0, 256.0, 420.0, 24.0);
const SCORE_EDIT_WIDTH: f32 = 286.0;

/// A front-end fade under way (`fade`): the picture it shades, its size,
/// and what comes after it.
struct Fading {
    way: crate::fade::Way,
    since: Instant,
    pixels: Vec<u32>,
    size: (u32, u32),
    then: AfterFade,
}

/// What follows a fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterFade {
    Nothing,
    /// The new front-end screen fades in (message 0x102).
    ScreenIn,
    /// A game starting (0x50f910): `LOADING` fades in, then out, then the
    /// game's palette fades in from its idle.
    LoadingIn,
    LoadingOut,
    GameIn,
    /// A saved game loading (command 0x102: 0x4ef350 after the systems
    /// are torn down, then gamesys 0x101's own): a second fade out, black
    /// to black, the full 50 x 5 ms, then `LoadingIn`.
    Black,
    /// Quit (command 0x131: 0x4ef350, then the loop ends): no fade in.
    Exit,
}

/// The screen last shown: which (none, the game) and its picture (without
/// the pointer).
struct Shown {
    screen: Option<std::mem::Discriminant<MenuState>>,
    pixels: Vec<u32>,
    size: (u32, u32),
}

/// The longest names the edits take: the end screen's (0x539350, 0x27)
/// and the connect screen's (0x538440, 0x104).
const SCORE_NAME_MAX: usize = 0x27;
const PLAYER_NAME_MAX: usize = 0x104;

/// A question the game asks before acting (gamesys.cpp 0x511c00,
/// 0x512f00; the file panel's, 0x50a700), or a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Quit,
    MainMenu,
    Restart,
    Overwrite,
    Load,
    Delete,
    /// Notices, answered OK.
    NoName,
    SaveFailed,
}

impl Ask {
    fn string(self) -> (u32, &'static str) {
        match self {
            Ask::Quit => (13, "Exit to Windows.\nAre you sure?"),
            Ask::Delete => (14, "Delete game.\nAre you sure?"),
            Ask::MainMenu => (15, "Exit to main menu.\nAre you sure?"),
            Ask::Restart => (16, "Restart mission.\nAre you sure?"),
            Ask::Load => (17, "Load game.\nAre you sure?"),
            Ask::Overwrite => (18, "Slot already exists.\nOverwrite saved game?"),
            Ask::NoName => (19, "Slot name is\nnot entered!"),
            Ask::SaveFailed => (20, "Error while saving\ngame!"),
        }
    }

    /// Answered yes or no (else OK).
    fn yes_no(self) -> bool {
        !matches!(self, Ask::NoName | Ask::SaveFailed)
    }
}

/// Mission groups on the front end, by archive name prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Category {
    Campaign,
    Single,
    Battle,
}

impl Category {
    fn prefix(self) -> &'static str {
        match self {
            Category::Campaign => "miss",
            Category::Single => "single",
            Category::Battle => "battle",
        }
    }

    fn of(mission: &str) -> Option<Self> {
        [Category::Campaign, Category::Single, Category::Battle]
            .into_iter()
            .find(|c| mission.starts_with(c.prefix()))
    }
}

/// The front-end screen shown instead of the game.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuState {
    Main,
    Missions(Category, Option<usize>),
    Load(LoadTab, Option<usize>),
    Briefing,
    Outcome(crate::scene::Outcome),
    /// The best results.
    Best,
    /// The original's first multiplayer screen: the player's name and the
    /// way to connect.
    Connect,
    /// The multiplayer lobby: a battle map to host (with the number of
    /// players), or an address to join.
    Multiplayer(Option<usize>),
    /// Hosting or joining, until everyone is there.
    Waiting,
}

/// The load screen's tabs (as the original's: string resources 1034,
/// 1035 and 1142).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoadTab {
    /// Games saved in the campaign.
    Scenario,
    /// Games saved in other missions.
    Custom,
    /// A campaign mission to start from.
    StartFrom,
}

/// An entry of the load screen's list.
#[derive(Debug, Clone, Copy)]
enum LoadEntry {
    /// A saved game (its index in `App::saves`).
    Save(usize),
    /// A start report (its index in `App::reports`).
    Report(usize),
}

/// A start report (`MISSIONS\SAVE_MISSION_<NN>.RPM`, written by the
/// briefing's Start, 0x53a3a0): the difficulty chosen and the campaign's
/// record at the mission's start.
struct StartReport {
    path: std::path::PathBuf,
    label: String,
    mission: u32,
}

/// What the port keeps of a start report after its "RP" and difficulty
/// word, in place of the original's 0x151 bytes of game + 0x32fd and its
/// carried objects (0x53a3a0), whose layout is not decoded: the campaign
/// mission's number (game + 0x3305) and the port's record carried into it.
#[derive(serde::Serialize, serde::Deserialize)]
struct ReportBody {
    mission: u32,
    carry: Option<crate::scene::Carry>,
}

/// The first word of a start report (0x53a3a0, 0x53a5a0): "RP".
const REPORT_MAGIC: u16 = 0x5052;

/// What the mission screens show of the chosen mission: its map, size,
/// players and briefing.
struct Preview {
    index: usize,
    size: (u32, u32),
    /// The map picture as shown (`marks`): none with the map tick off.
    map: Option<Rc<crate::assets::Image>>,
    /// The terrain, 8 pixels a cell, as palette colours (`screen`).
    terrain: (u32, u32, Vec<u32>),
    /// Palette index to the menu screen's colour.
    screen: [u32; 256],
    /// The mission's `LIST_OBJ` (+0x7f4).
    list: Vec<openconquest_formats::ListObj>,
    /// The cell at the top left of the human player's starting view.
    start: (u32, u32),
    /// The briefing as the original lays it out.
    text: Vec<String>,
}

impl Preview {
    fn load(data: &GameData, caches: &mut Caches, mission: &str, index: usize) -> Result<Self> {
        let archive = data.open_mission(mission)?;
        let grid = data.load_map(&archive, mission)?;
        // The menu screens' palette has none of the game's colours: each
        // is shown as the nearest of the backdrop's.
        let mut screen = crate::render::palette_lut(data, 0);
        if let Some(menus) = &data.menus {
            if let Ok(raw) = menus.read("0M_BKGND_01", openconquest_formats::ResourceKind::Bitmap) {
                if let Ok(b) = openconquest_formats::Bitmap::parse(&raw) {
                    let near = |c: u32| {
                        let (r, g, bl) = (
                            (c >> 16 & 255) as i32,
                            (c >> 8 & 255) as i32,
                            (c & 255) as i32,
                        );
                        b.palette
                            .0
                            .iter()
                            .min_by_key(|p| {
                                let d = |a: i32, b: u8| (a - b as i32) * (a - b as i32);
                                d(r, p.r) + d(g, p.g) + d(bl, p.b)
                            })
                            .map_or(c, |p| crate::assets::rgb(*p))
                    };
                    for slot in screen.iter_mut() {
                        *slot = near(*slot);
                    }
                }
            }
        }
        let terrain = crate::terrain::Terrain::small(data, &grid, &mut caches.tiles, &screen)?;
        let infos = openconquest_formats::players(&archive).unwrap_or_default();
        let start = infos
            .iter()
            .find(|p| !p.computer)
            .and_then(|p| openconquest_formats::mission::start_view(&archive, p.nation, p.slot))
            .unwrap_or((0, 0));
        Ok(Self {
            index,
            size: (grid.width, grid.height),
            map: None,
            terrain,
            screen,
            list: openconquest_formats::list_obj(&archive),
            start,
            text: openconquest_formats::briefing_text(&archive).unwrap_or_default(),
        })
    }

    /// Makes the map picture again (Startcom.cpp 0x52f470): none with the
    /// map tick off (`[0x5ca538]`); else the terrain, then the marks of
    /// `LIST_OBJ` kept by 0x528de0 for the players list `players` (Custom
    /// missions'; `None` on the Load screen, which has none): a player's
    /// object only while its owner's row is not Disable, anything else
    /// always. Two passes, the other objects then the players'. A
    /// player's object's frame is nation x 4 + slot (0x10 for the
    /// barbarians), anything else's by its kind (`MARK_FRAMES`); with bit
    /// 0x80 (a player's object of size 4, or kind 0x18) a 16 x 16 frame of
    /// `LMAP16X16`, else an 8 x 8 one of `LMAP8X8`, its top left at the
    /// entry's map pixel / 8 (toward zero, at least 0), skipping the
    /// entry's key colour (0x56dd90) or copied whole (key -1, 0x562770).
    /// On Custom missions (type 6) only players' objects are drawn, and
    /// with `HEAD_CUST` a != 0 and b == 0 and a Human row, only that
    /// player's; on Load game (type 5) players' objects only. Shown only
    /// with byte +22 set. (The fog of the mission's visibility data over
    /// it is not ported.)
    fn make_map(
        &mut self,
        data: &GameData,
        tick: bool,
        players: Option<(&[CustomPlayer], [u32; 3])>,
    ) {
        self.map = None;
        if !tick {
            return;
        }
        let (w, h, terrain) = &self.terrain;
        let mut pixels = terrain.clone();
        let sheet = |name: &str| {
            let raw = data
                .menus
                .as_ref()?
                .read(name, openconquest_formats::ResourceKind::Bitmap)
                .ok()?;
            openconquest_formats::Bitmap::parse(&raw).ok()
        };
        let sheets = [sheet("LMAP8X8"), sheet("LMAP16X16")];
        // 0x528de0: the sides of the rows not disabled.
        let mut sides = [false; 20];
        if let Some((rows, _)) = players {
            for r in rows.iter().filter(|r| r.kind != 0) {
                if let Some(s) = sides.get_mut((r.info.nation * 4 + r.info.slot) as usize) {
                    *s = true;
                }
            }
        }
        let kept = |e: &&openconquest_formats::ListObj| {
            e.kind == 2
                || players.is_none()
                || sides
                    .get(usize::from(e.owner.0) * 4 + usize::from(e.owner.1))
                    .copied()
                    .unwrap_or(false)
        };
        // The Human row (0x528a70) when `HEAD_CUST` a != 0 and b == 0.
        let only = players.and_then(|(rows, head)| {
            (head[0] != 0 && head[1] == 0).then(|| {
                rows.iter()
                    .find(|r| r.kind == 2)
                    .map(|r| (r.info.nation, r.info.slot))
            })
        });
        for pass in [2u8, 1] {
            for e in self.list.iter().filter(kept).filter(|e| e.kind == pass) {
                let frame = if e.kind == 1 {
                    let f = if e.owner.0 == 4 {
                        0x10
                    } else {
                        u32::from(e.owner.0) * 4 + u32::from(e.owner.1)
                    };
                    if e.size == 4 {
                        f | 0x80
                    } else {
                        f
                    }
                } else {
                    MARK_FRAMES
                        .iter()
                        .find(|m| m.0.contains(&e.class))
                        .map_or(0, |m| m.1)
                };
                let drawn = match only {
                    Some(Some((nation, slot))) => {
                        e.kind == 1
                            && (u32::from(e.owner.0), u32::from(e.owner.1)) == (nation, slot)
                    }
                    _ => e.kind == 1,
                };
                if !e.shown || !drawn {
                    continue;
                }
                let (big, side) = if frame & 0x80 != 0 { (1, 16) } else { (0, 8) };
                let Some(src) = &sheets[big] else {
                    continue;
                };
                let (x0, y0) = ((e.x / 8).max(0) as u32, (e.y / 8).max(0) as u32);
                let top = (frame & 0x7f) * side;
                for sy in 0..side {
                    for sx in 0..side {
                        let (dx, dy) = (x0 + sx, y0 + sy);
                        if dx >= *w || dy >= *h || sx >= src.width || top + sy >= src.height {
                            continue;
                        }
                        let i = src.pixels[((top + sy) * src.width + sx) as usize];
                        if e.key != -1 && i == e.key as u8 {
                            continue;
                        }
                        pixels[(dy * w + dx) as usize] = self.screen[usize::from(i)];
                    }
                }
            }
        }
        self.map = Some(Rc::new(crate::assets::Image::new(
            *w,
            *h,
            pixels.into_iter().map(Some).collect(),
        )));
    }
}

/// The map preview's frame for anything but a player's object, by its
/// kind (0x52f470's switch): `LMAP8X8` frames, 0x91 a `LMAP16X16` one;
/// other kinds 0.
const MARK_FRAMES: [(&[u32], u32); 20] = [
    (&[1], 0x12),
    (&[2], 0x13),
    (&[3], 0x11),
    (&[4], 0x14),
    (&[5], 0x15),
    (&[6], 0x16),
    (&[7], 0x17),
    (&[8], 0x18),
    (&[9, 0x13, 0x15], 0x19),
    (&[10], 0x1a),
    (&[0xb], 0x1b),
    (&[0xc], 0x1c),
    (&[0xd], 0x1d),
    (&[0x10], 0x1e),
    (&[0x11], 0x1f),
    (&[0x12], 0x20),
    (&[0x14], 0x21),
    (&[0x16], 0x22),
    (&[0x17], 0x23),
    (&[0x18], 0x91),
];

/// A player of Custom missions' players list (Startcom.cpp: the screen's
/// list + 0x7e8, an entry of 0x174 bytes, made by 0x52a990): every player
/// of the mission's `DESCRIPTOR` but the barbarians (nation 4).
#[derive(Debug, Clone)]
struct CustomPlayer {
    info: openconquest_formats::PlayerInfo,
    /// Its kind (+ 0x184): 0 Disable, 1 Available, 2 Human, 3-5 AI Weak,
    /// Medium and Strong (strings 1146 + kind).
    kind: u8,
    /// Its name (+ 0x188): the session's for the human, else empty.
    name: String,
}

/// The players kinds' captions, strings 1146-1151.
const KIND_NAMES: [&str; 6] = [
    "Disable",
    "Available",
    "Human",
    "AI Weak",
    "AI Medium",
    "AI Strong",
];

/// The font variant a players row draws its kind and name in (0x53b940
/// message 0x195): 0 for Disable and Available, 2 for Human, 1 for the
/// computer.
fn kind_variant(kind: u8) -> usize {
    match kind {
        0 | 1 => 0,
        2 => 2,
        _ => 1,
    }
}

/// Custom missions' players rows: 11 (screen + 0x650), 27 apart (the
/// SP_LPLAY row frame's 26 and a pixel, + 0x658), the first's kind box at
/// (230, 249), its arrow at (342, 249) and its name at (375, 249)
/// (0x52c5c0, 0x529de0).
const PLAYER_ROWS: usize = 11;

/// Custom missions' kind pop-up (0x52ebe0), while it is open.
#[derive(Debug, Clone)]
struct KindPopup {
    /// The player it is for (+ 0x590) and where it stands (+ 0x584,
    /// + 0x588).
    player: usize,
    x: f32,
    y: f32,
    /// The kinds it offers (+ 0x598).
    entries: Vec<u8>,
    /// The list's current entry (_slist + 0x136).
    current: usize,
    /// The entry the left button went down on, while it is held (+ 0x1f2,
    /// + 0x1ee), and whether a drag moved the current one since (+ 0x1f6).
    pressed: Option<usize>,
    dragged: bool,
    /// Up or Down held (the list's two buttons, 0xc8 and 0xd0) and when
    /// it next repeats.
    step: Option<(i32, u64)>,
}

impl KindPopup {
    /// Its picture, `T_PLAYER5` (146 x 131) or `T_PLAYER3` (146 x 87),
    /// by how many entries it offers.
    fn picture(&self) -> (&'static str, f32, f32) {
        if self.entries.len() == 3 {
            ("T_PLAYER3", 146.0, 87.0)
        } else {
            ("T_PLAYER5", 146.0, 131.0)
        }
    }

    /// The list (_slist, made by 0x52ebe0): at (x + 16, y + 12), LPLAY_TP's
    /// 110 wide and 21 an entry.
    fn list(&self) -> crate::menus::Rect {
        (
            self.x + 16.0,
            self.y + 12.0,
            110.0,
            21.0 * self.entries.len() as f32,
        )
    }

    /// The entry at `p` (0x54cd00 messages 0x60-0x62): inside the list,
    /// the first whose band from its top to its top + 21 (both included)
    /// holds it.
    fn entry_at(&self, p: (f32, f32)) -> Option<usize> {
        let (x, y, w, h) = self.list();
        if p.0 < x || p.0 >= x + w || p.1 < y || p.1 >= y + h {
            return None;
        }
        (0..self.entries.len()).find(|&i| {
            let top = y + 21.0 * i as f32;
            top <= p.1 && p.1 <= top + 21.0
        })
    }

    /// Whether `p` is outside the picture (StartMouseClassTy 0x52fbb0: on
    /// its left or top edge counts as outside).
    fn outside(&self, p: (f32, f32)) -> bool {
        let (_, w, h) = self.picture();
        p.0 <= self.x || p.1 <= self.y || p.0 >= self.x + w || p.1 >= self.y + h
    }
}

/// What a front-end screen shows, owned (see `MenuView`).
struct MenuPage {
    backdrop: Backdrop,
    title: Option<String>,
    items: Vec<String>,
    selected: Option<usize>,
    text: Vec<String>,
    notes: Vec<String>,
    buttons: Vec<Button>,
    first: usize,
    /// How many rows of the list show at once (0: as many as fit).
    rows: usize,
    credits: bool,
    story: Option<(crate::menus::Rect, Vec<String>)>,
    story_style: Option<(crate::menus::Style, f32)>,
    story_first: usize,
    labels: Vec<(String, f32, f32)>,
    checks: Vec<crate::menus::Check>,
    list_rect: Option<crate::menus::Rect>,
    picture: Option<(Rc<crate::assets::Image>, crate::menus::Rect)>,
    boxes: Vec<(crate::menus::Rect, Vec<String>)>,
    tabs: Vec<crate::menus::MenuTab>,
    sprites: Vec<crate::menus::Placed>,
    texts: Vec<crate::menus::Text>,
    map: Option<(Rc<crate::assets::Image>, crate::menus::Rect, (u32, u32))>,
    row_height: Option<f32>,
    spots: Vec<(crate::menus::Rect, MenuHit)>,
    tracks: Vec<crate::menus::Track>,
    fills: Vec<(crate::menus::Rect, u32)>,
    /// Over everything (see `MenuView::top`).
    top_sprites: Vec<crate::menus::Placed>,
    top_texts: Vec<crate::menus::Text>,
}

/// The parts the mission screens share (see `App::mission_panels`).
struct MissionPanels {
    sprites: Vec<crate::menus::Placed>,
    texts: Vec<crate::menus::Text>,
    map: Option<(Rc<crate::assets::Image>, crate::menus::Rect, (u32, u32))>,
    story: Vec<String>,
    story_first: usize,
    tracks: Vec<crate::menus::Track>,
    spots: Vec<(crate::menus::Rect, MenuHit)>,
}

/// A mission screen's list bar (see `App::list_bar`).
struct ListBar {
    first: usize,
    sprites: Vec<crate::menus::Placed>,
    spots: Vec<(crate::menus::Rect, MenuHit)>,
    track: crate::menus::Track,
}

/// The mission description's box and font on the mission screens: the
/// read-only edit at (196 + 8, 27 + 0x20), 0x22b x 0x90 (Startcom.cpp,
/// 0x52c5c0).
const STORY_BOX: crate::menus::Rect = (204.0, 59.0, 555.0, 144.0);
const STORY_STYLE: (crate::menus::Style, f32) = (
    crate::menus::Style {
        font: "MH",
        variant: 1,
        spacing: 1,
    },
    18.0,
);
/// The map preview's viewer shows 19 x 19 cells (Startcom.cpp, 0x52c5c0).
const MAP_CELLS: usize = 19;

impl MenuPage {
    fn view(&self, hover: Option<(f32, f32)>, pressed: Option<usize>) -> MenuView<'_> {
        MenuView {
            backdrop: self.backdrop,
            title: self.title.as_deref(),
            list: (!self.items.is_empty()).then_some((self.items.as_slice(), self.selected)),
            first: self.first,
            text: &self.text,
            notes: &self.notes,
            buttons: &self.buttons,
            hover,
            pressed,
            credits: self.credits,
            story: self.story.as_ref().map(|(r, p)| (*r, p.as_slice())),
            story_style: self.story_style,
            story_first: self.story_first,
            labels: &self.labels,
            checks: &self.checks,
            list_rect: self.list_rect,
            picture: self.picture.as_ref().map(|(i, r)| (&**i, *r)),
            boxes: &self.boxes,
            tabs: &self.tabs,
            sprites: &self.sprites,
            texts: &self.texts,
            map: self.map.as_ref().map(|(i, r, o)| (&**i, *r, *o)),
            row_height: self.row_height,
            spots: &self.spots,
            tracks: &self.tracks,
            fills: &self.fills,
            top: (&self.top_sprites, &self.top_texts),
        }
    }
}

/// What the pointer is over (see `App::hover_key`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoverKey {
    Object(u32),
    Cell((i32, i32)),
}

/// Player settings from the configuration and sound panels (saved with
/// this port's settings).
#[derive(Debug, Clone, Copy)]
struct Settings {
    config: panels::Config,
    /// Right click gives orders, instead of the original's stepping through
    /// the pointers.
    modern_mouse: bool,
    /// The command keys (config.exe's Hotkey editor; `config.json`).
    hotkeys: crate::hotkeys::Hotkeys,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            config: crate::launch::panel_config(),
            modern_mouse: crate::launch::modern_mouse(),
            hotkeys: crate::launch::hotkeys(),
        }
    }
}

/// A saved game in the save folder.
#[derive(Debug, Clone)]
struct SaveEntry {
    path: std::path::PathBuf,
    /// Its mission, and how the front end's load screen labels it.
    mission: String,
    label: String,
}

pub struct Options {
    pub fullscreen: bool,
    pub edge_scroll: bool,
    pub window_size: (u32, u32),
    pub mute: bool,
    pub soundfont: Option<std::path::PathBuf>,
    /// The ripped CD audio tracks' folder.
    pub cd_audio: Option<std::path::PathBuf>,
    /// Show the company logos and title sequence at start-up.
    pub intro: bool,
    /// Fog of war.
    pub fog: bool,
    /// The computer players' difficulty (`scene::EASY`, `NORMAL`, `HARD`).
    pub difficulty: usize,
}

struct Gfx {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    _context: softbuffer::Context<Rc<Window>>,
}

pub struct App {
    data: GameData,
    caches: Caches,
    missions: Vec<String>,
    mission_index: usize,
    scene: Scene,
    camera: Camera,
    options: Options,
    gfx: Option<Gfx>,
    error: Option<anyhow::Error>,
    interface: Interface,
    /// The bar hidden (Space, `[game+0x328d]`): false at every game build
    /// (0x4ebf50) and on leaving the game (0x4ec1a0 0x100/0x10b/0x10c), not
    /// saved.
    bar_hidden: bool,
    text: TextRenderer,

    keys: HashSet<KeyCode>,
    modifiers: ModifiersState,
    cursor: Option<PhysicalPosition<f64>>,
    focused: bool,
    drag_from: Option<PhysicalPosition<f64>>,
    /// Where the right button went down, and whether it has since been dragged.
    right_press: Option<(PhysicalPosition<f64>, bool)>,
    /// The original mouse's right button stepping through the pointers:
    /// what the pointer was over, and how many steps on.
    cycle: Cell<(Option<HoverKey>, usize)>,
    /// The object info last shown in the strip below the view (kept over
    /// what shows none, as 0x516c10 does).
    shown_info: Cell<Option<crate::object_info::ObjectInfo>>,
    minimap_drag: bool,
    selected: Option<u32>,
    /// Other own ships selected with it.
    group: Vec<u32>,
    /// The groups assigned with Ctrl+1 ... Ctrl+0 (Cmd on macOS), recalled
    /// with 1 ... 0 (the manual's "Game Hot Keys").
    groups: [Vec<u32>; 10],
    /// When each selected object was selected, for its frame's zoom.
    selected_since: HashMap<u32, u64>,
    /// The building shown standing where it would go: its site, and it.
    site_shown: Option<(SiteKey, crate::objects::MapObject)>,
    /// When objects were deselected, for their frames' zoom out.
    deselected: HashMap<u32, u64>,
    /// The map places assigned with Shift+F1 ... Shift+F10, centred on with
    /// F1 ... F10.
    places: [Option<(f32, f32)>; 10],
    /// Where a left-button selection box started.
    box_from: Option<PhysicalPosition<f64>>,
    /// A left press on the map with something selected, held back: a drag
    /// from it draws a selection box, a release without one clicks there.
    left_down: Option<PhysicalPosition<f64>>,
    /// The radar map shown in place of the view, the view in the bar's map
    /// window (Tab).
    radar: bool,
    /// The radar map's first cell (it scrolls to keep the view's frame).
    radar_origin: (i32, i32),
    /// What the bar's map window shows with the radar map.
    radar_view: Camera,
    /// A command waiting for a target on the map (e.g. Move).
    pending: Option<Command>,
    /// The window size the frame being drawn has (see `window_size`).
    drawn_size: (u32, u32),
    /// The original's mouse pointers, drawn by the game.
    pointers: crate::cursor::Pointers,
    /// Which menu of the selected object is shown.
    page: Page,
    /// The mission briefing is shown (and the game waits).
    briefing: bool,
    /// Sound output, if a device is available.
    audio: Option<Audio>,
    /// Music, if a SoundFont is available.
    music: Option<Music>,
    /// The menus' sample channel (`mus_menu`, sound event 325, which the
    /// original plays looped when no other is asked for: 0x525370; the
    /// briefing's FON), while the front-end menus are up.
    menu_music: Option<rodio::Sink>,
    /// The sea's surf under a mission (`prib3`, event 273).
    ambience: Option<rodio::Sink>,
    /// The surf could not be loaded: not tried again.
    no_ambience: bool,
    /// `mus_menu` could not be played: do not try again.
    no_menu_music: bool,
    music_volume: f32,
    /// Where the ripped CD audio tracks are looked for.
    cd_audio: Option<std::path::PathBuf>,
    /// The video playing, and those waiting their turn.
    video: Option<crate::video::Player>,
    videos: std::collections::VecDeque<std::path::PathBuf>,
    /// The control bar's open tab.
    tab: Tab,
    /// The saved games, and their names as the file panel lists them.
    saves: Vec<SaveEntry>,
    /// The start reports Load game's "Start from..." lists.
    reports: Vec<StartReport>,
    save_names: Vec<String>,
    /// The file panel: the first row shown, the row chosen, the bookmark
    /// and the name field.
    save_first: usize,
    selected_save: Option<usize>,
    file_mode: FileMode,
    save_name: String,
    help_text: Vec<String>,
    help_first: usize,
    /// The quick help page shown (none: the mission objectives).
    help_page: Option<usize>,
    /// A panel button held down.
    panel_pressed: Option<PanelHit>,
    /// A question asked before the game is left (the game holds still
    /// meanwhile), and the answer held down.
    ask: Option<Ask>,
    ask_pressed: Option<bool>,
    /// The own ship last clicked and when (the modern mouse's double
    /// click).
    last_click: Option<(u32, Instant)>,
    /// The game's messages over the view, oldest first, and when the
    /// oldest last went (chat.cpp).
    chat: Vec<String>,
    chat_since: Instant,
    /// The SoundFont given for MIDI music.
    soundfont: Option<std::path::PathBuf>,
    /// The music's tracks as the sound panel lists them.
    track_names: Vec<String>,
    /// The object panel's state (`info_panel`).
    info_view: crate::info_panel::InfoView,
    /// The load panel's state (`load_panel`), and its button held down.
    load_view: crate::load_panel::LoadView,
    load_pressed: Option<crate::load_panel::Hit>,
    settings: Settings,
    /// Quit at the next opportunity.
    quit: bool,
    /// Fog of war in the missions played.
    fog: bool,
    difficulty: usize,
    /// The campaign record the current mission started from (none when it
    /// was picked from the menu).
    carry: Option<crate::scene::Carry>,
    /// A multiplayer game's connection, and the game time not yet played
    /// in turns.
    net: Option<crate::net::Net>,
    turn_time: f32,
    /// The lobby: players wanted, the address being typed, the battle maps
    /// and how many players each takes, and a game being hosted or joined.
    lobby_players: usize,
    join_address: String,
    /// Custom missions' players (see `CustomPlayer`), for the mission
    /// shown (its index), and its `HEAD_CUST`.
    custom_players: Vec<CustomPlayer>,
    custom_for: Option<usize>,
    custom_head: [u32; 3],
    /// A players row's kind field or arrow held down with the pointer
    /// inside it (its row; the arrow's when set).
    kind_held: Option<(usize, bool)>,
    /// The kind pop-up, while it is open.
    kind_popup: Option<KindPopup>,
    /// The scan codes the system repeated since the keyboard's last poll.
    key_repeats: Vec<u8>,
    /// The session's name (the front end's name list, resources + 0x1b9,
    /// one string): made once from the stored name (`stored_name`) by
    /// Start scenario, MultiPlayer, Custom missions or Load game
    /// (0x53d220), edited by the connect screen's edit.
    player_name: String,
    session_made: bool,
    /// The stored player's name (Config + 0x50, 0x5ca578).
    stored_name: String,
    /// A chat line being typed in a multiplayer game.
    chat_line: Option<String>,
    /// The first entry shown of a front-end list, and wheel movement not
    /// yet a whole row (trackpads scroll in small steps).
    list_first: usize,
    wheel_rest: f32,
    /// The first line of the mission description shown, and the map
    /// preview's top left cell once scrolled (the player's starting view
    /// till then).
    desc_first: usize,
    map_at: Option<(usize, usize)>,
    /// The scroll bar being dragged on a front-end screen, and the scroll
    /// arrow held with when it next repeats.
    menu_drag: Option<crate::menus::Scroller>,
    menu_repeat: Option<(MenuHit, u64)>,
    /// The front-end button pressed, while the left button is held with
    /// the pointer inside it (_sbutton.cpp 0x54ab20: its action comes on
    /// the release inside it; the pointer leaving lets it go).
    menu_held: Option<usize>,
    /// The map tick held down (it acts when let go on it, _sbutton).
    tick_held: bool,
    battle_maps: Vec<(usize, usize)>,
    hosting: Option<crate::net::Lobby>,
    joining: Option<crate::net::Joining>,
    /// A slider being dragged with the left button.
    slider_drag: Option<Slider>,
    /// The game is paused (P).
    paused: bool,
    /// The front end, if it is shown instead of the game.
    menu: Option<MenuState>,
    menus: Option<Menus>,
    /// The mission chosen on a mission screen, shown.
    preview: Option<Preview>,
    /// A mission to show and when: the list loads it 200 ms after the
    /// choice last changed (0x52dc40).
    preview_due: Option<(usize, Instant)>,
    /// Mission titles from their briefings, by mission index.
    mission_titles: Vec<String>,
    /// Whether Custom missions lists each mission: `HEAD_CUST`'s a set
    /// (0x52b7c0).
    custom_listed: Vec<bool>,
    /// A game is under way (the main menu offers to resume it).
    in_game: bool,
    /// The mission clock when it was won or lost.
    outcome_at: Option<f32>,
    /// The Best Conquerors table, and the campaigns the CD installed.
    best: crate::records::Table,
    campaigns: crate::records::Campaigns,
    /// The play-through's id (game + 0x1a0d), and the campaigns started
    /// (game + 0x3214, a bit each, 0x537f60).
    game_id: u32,
    campaign_mask: u32,
    /// The end screen's name edit, when a row was entered.
    score_entry: Option<ScoreEntry>,
    /// The connect screen's name edit.
    connect_edit: NameEdit,
    /// The front end's key bindings, and the screen they are for.
    menu_keys: crate::sinput::Keys<KeyMsg>,
    keys_screen: Option<std::mem::Discriminant<MenuState>>,
    /// A fade between screens, and the screen last shown.
    fade: Option<Fading>,
    shown: Option<Shown>,
    /// The game being started by the briefing's or Custom missions' Start
    /// (command 0x101): the fade into it shows `LOADING`.
    entering: bool,
    /// The game entered is a saved one being loaded (command 0x102): the
    /// fade into it goes through black first (`AfterFade::Black`).
    entering_load: bool,
    /// The game's fade in (0x50f740): steps taken and when the last was.
    game_fade: Option<(u32, Instant)>,
    /// What the menus' sample channel plays (`mus_menu` or the briefing's
    /// FON), the briefing's archive (`<video>TASKS.DKD`, none: not there)
    /// and its voice's clock (Starttsk.cpp + 0xf8, + 0xfc).
    menu_sample: Option<String>,
    tasks: Option<Option<openconquest_formats::Archive>>,
    briefing_voice: Option<(Instant, u8)>,
    status: Option<(String, Instant)>,
    started: Instant,
    last_frame: Instant,
    time_ms: u64,
    /// Where ships were last sent and when (`time_ms`): the green tick.
    sent: Option<(f32, f32, u64)>,
    center_pending: bool,
}

impl App {
    pub fn new(
        data: GameData,
        missions: Vec<String>,
        mission_index: usize,
        camera: Camera,
        options: Options,
        save: Option<SaveGame>,
        start_menu: bool,
    ) -> Result<Self> {
        let mute = options.mute;
        let fog = options.fog;
        let difficulty = options.difficulty;
        let soundfont = options.soundfont.clone();
        let cd_audio = options.cd_audio.clone();
        let intro = options.intro;
        let data_root = data.root().to_path_buf();
        let best = crate::launch::best_table();
        let campaigns = crate::records::Campaigns::read(data.root());
        let menus = Menus::load(&data, '0')
            .map_err(|e| eprintln!("no menus: {e:#}"))
            .ok();
        // Mission titles: the first line of each briefing.
        let mission_titles = missions
            .iter()
            .map(|m| {
                data.open_mission(m)
                    .ok()
                    .and_then(|a| openconquest_formats::briefing(&a).ok())
                    .and_then(|b| b.first().map(|t| t.trim_matches('"').to_string()))
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| m.to_uppercase())
            })
            .collect();
        let custom_listed = missions
            .iter()
            .map(|m| {
                data.open_mission(m)
                    .is_ok_and(|a| openconquest_formats::head_cust(&a)[0] != 0)
            })
            .collect();
        let mut caches = Caches::default();
        let restored = save.is_some();
        // The play-through's id: the save's (ID_GAME), else a new one
        // (0x4ed3f0).
        let game_id = save
            .as_ref()
            .and_then(|s| s.game_id)
            .unwrap_or_else(crate::records::game_id);
        let scene = match &save {
            Some(save) => Scene::restore(&data, &mut caches, save)?,
            None => Scene::load_at(&data, &mut caches, &missions[mission_index], difficulty)?,
        };
        let text = TextRenderer::load(&data.controls, "0SYSTEM_FONT", 0)?;
        let interface = Interface::load(&data, '0')?;
        let now = Instant::now();
        let mut app = Self {
            data,
            caches,
            missions,
            mission_index,
            scene,
            camera,
            options,
            gfx: None,
            error: None,
            interface,
            bar_hidden: false,
            text,
            keys: HashSet::new(),
            modifiers: ModifiersState::empty(),
            cursor: None,
            focused: true,
            drag_from: None,
            right_press: None,
            cycle: Cell::new((None, 0)),
            shown_info: Cell::new(None),
            minimap_drag: false,
            selected: None,
            group: Vec::new(),
            groups: Default::default(),
            selected_since: HashMap::new(),
            site_shown: None,
            deselected: HashMap::new(),
            places: [None; 10],
            box_from: None,
            left_down: None,
            radar: false,
            radar_origin: (0, 0),
            radar_view: camera,
            pending: None,
            drawn_size: (1, 1),
            pointers: Default::default(),
            page: Page::Main,
            briefing: !restored,
            music: None,
            menu_music: None,
            ambience: None,
            no_ambience: false,
            no_menu_music: false,
            music_volume: 0.5,
            cd_audio,
            video: None,
            videos: if start_menu && intro {
                crate::video::INTRO
                    .iter()
                    .filter_map(|n| crate::video::find(data_root.as_path(), n))
                    .collect()
            } else {
                Default::default()
            },
            tab: Tab::Object,
            saves: Vec::new(),
            reports: Vec::new(),
            save_names: Vec::new(),
            save_first: 0,
            selected_save: None,
            file_mode: FileMode::Save,
            save_name: String::new(),
            help_text: Vec::new(),
            help_first: 0,
            help_page: None,
            panel_pressed: None,
            ask: None,
            ask_pressed: None,
            last_click: None,
            chat: Vec::new(),
            chat_since: Instant::now(),
            soundfont: soundfont.clone(),
            track_names: Vec::new(),
            info_view: Default::default(),
            load_view: Default::default(),
            load_pressed: None,
            settings: Settings::default(),
            quit: false,
            fog,
            difficulty,
            carry: None,
            net: None,
            turn_time: 0.0,
            lobby_players: 2,
            join_address: String::new(),
            player_name: String::new(),
            session_made: false,
            stored_name: crate::launch::player_name(),
            custom_players: Vec::new(),
            custom_for: None,
            custom_head: [1, 1, 0],
            kind_held: None,
            kind_popup: None,
            key_repeats: Vec::new(),
            chat_line: None,
            list_first: 0,
            wheel_rest: 0.0,
            desc_first: 0,
            map_at: None,
            menu_drag: None,
            menu_held: None,
            tick_held: false,
            menu_repeat: None,
            battle_maps: Vec::new(),
            hosting: None,
            joining: None,
            slider_drag: None,
            paused: false,
            menu: start_menu.then_some(MenuState::Main),
            menus,
            preview: None,
            preview_due: None,
            mission_titles,
            custom_listed,
            in_game: !start_menu,
            outcome_at: None,
            best,
            campaigns,
            game_id,
            campaign_mask: 0,
            score_entry: None,
            connect_edit: NameEdit::default(),
            menu_keys: Default::default(),
            keys_screen: None,
            fade: None,
            shown: None,
            entering: false,
            entering_load: false,
            game_fade: None,
            menu_sample: None,
            tasks: None,
            briefing_voice: None,
            audio: Audio::open().map(|mut a| {
                a.muted = mute;
                a
            }),
            status: None,
            started: now,
            last_frame: now,
            time_ms: 0,
            sent: None,
            center_pending: true,
        };
        app.scene.fog = app.fog;
        // The front end starts with no campaign (0x4ebd10: + 0x3214 = 0).
        if !start_menu {
            app.note_campaign();
        }
        app.apply_volumes();
        app.start_music();
        Ok(app)
    }

    pub fn run(mut self) -> Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.run_app(&mut self)?;
        // The table and the name are written when the program ends, as the
        // original writes its registry (0x4ee6c0 -> 0x53d2f0, 0x501090):
        // the session's name, if it was made, into the stored one.
        crate::launch::save_at_exit(
            &self.best,
            self.session_made.then_some(self.player_name.as_str()),
        );
        match self.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// The window's size in pixels. While a frame is drawn (the window
    /// taken out of `gfx`), the size it had when the frame began: anything
    /// laid out then (the hovered button, the pointer) needs the real size.
    fn window_size(&self) -> (u32, u32) {
        self.gfx
            .as_ref()
            .map(|g| {
                let s = g.window.inner_size();
                (s.width.max(1), s.height.max(1))
            })
            .unwrap_or(self.drawn_size)
    }

    fn view(&self) -> GameView<'_> {
        let status = self
            .status
            .as_ref()
            .filter(|(_, at)| at.elapsed() < STATUS_TIME)
            .map(|(m, _)| m.as_str())
            .or_else(|| {
                // The load panel's buttons say what they do (strings 400,
                // 401).
                let id = match self.cursor.and_then(|c| self.load_hit(c))? {
                    crate::load_panel::Hit::End => 400,
                    crate::load_panel::Hit::Kill => 401,
                    _ => return None,
                };
                self.data.strings.get(&id).map(|s| s.as_str())
            });
        // With "help string" on, the object under the pointer names itself
        // in the message line (gcursor 0x516c10 sends its info to the line,
        // InfoStringTy, as to the strip).
        let hover_object = (self.settings.config.help_string && self.pending.is_none())
            .then(|| self.cursor.and_then(|c| self.object_at(c)))
            .flatten();
        GameView {
            bar_left: self.settings.config.bar_left,
            hover_object,
            radar: self.radar_shown().then_some(crate::ui::Radar {
                origin: self.radar_origin,
                view: self.radar_view,
            }),
            hotkeys: &self.settings.hotkeys,
            site_preview: None,
            site_cells: match self.pending.and_then(economy::building_of_command) {
                Some(economy::SETTLEMENT | economy::TOWN) => 2,
                _ => 1,
            },
            selected_since: &self.selected_since,
            deselected: &self.deselected,
            data: &self.data,
            scene: &self.scene,
            camera: &self.camera,
            interface: &self.interface,
            bar_hidden: self.bar_hidden,
            text: &self.text,
            selected: self.selected,
            group: &self.group,
            // A selection box with the modern mouse; a zone being drawn
            // with either.
            select_box: self
                .box_from
                .zip(self.cursor)
                .filter(|_| self.settings.modern_mouse || self.pending.is_some())
                .and_then(|(a, b)| {
                    ((a.x - b.x).abs() + (a.y - b.y).abs() > BOX_THRESHOLD)
                        .then_some((a.x as f32, a.y as f32, b.x as f32, b.y as f32))
                }),
            status,
            time_ms: self.time_ms,
            sent: self
                .sent
                .map(|(x, y, at)| (x, y, self.time_ms.saturating_sub(at))),
            hover_button: self.cursor.and_then(|c| self.button_at(c)),
            button_animation: self.settings.config.button_animation,
            info: self.info_state(),
            page: self.page,
            site: None,
            briefing: self.briefing,
            panel: Some(self.panel_view()),
            panel_hover: self.cursor.and_then(|c| self.bar_point(c)),
            paused: self.paused,
            ask: self
                .ask
                .map(|a| (self.ask_text(a), a.yes_no(), self.ask_pressed)),
            chat: if self.settings.config.message_popup {
                &self.chat
            } else {
                &[]
            },
            load: Some((self.load_state(), self.load_pressed)),
            object_info: self.hovered_info(),
        }
    }

    /// The object info for the strip below the view (gcursor.cpp
    /// 0x516c10): the object under the pointer fills it; off objects it is
    /// blank; one that shows none (type 0x1f7) leaves the last.
    fn hovered_info(&self) -> Option<crate::object_info::ObjectInfo> {
        // Only with "help string" on (0x5175b0 asks [0x5ca542]), and with
        // a viewing player (with none 0x5175b0 only moves the pointer's
        // sprite, 0x517635).
        if !self.settings.config.help_string || self.scene.human.is_none() {
            return None;
        }
        let obj = self
            .cursor
            .and_then(|c| self.object_at(c))
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i]);
        let info = match obj {
            None => None,
            Some(o) => crate::object_info::of(&self.scene, o).or(self.shown_info.get()),
        };
        self.shown_info.set(info);
        info
    }

    /// The load panel's state for the selected object (fresh when the
    /// selection changed).
    fn load_state(&self) -> crate::load_panel::LoadView {
        match self.selected {
            Some(id) if self.load_view.owner == id => self.load_view,
            Some(id) => crate::load_panel::LoadView {
                owner: id,
                ..Default::default()
            },
            None => Default::default(),
        }
    }

    /// The load panel's lists while the selected object reloads and the
    /// object panel shows.
    fn reloading(&self) -> Option<crate::load_panel::Lists> {
        if self.bar_hidden || Panels::covers_bar(self.tab) {
            return None;
        }
        crate::load_panel::lists(&self.scene, self.selected?, &self.load_state())
    }

    /// What of the load panel lies under window position `pos`.
    fn load_hit(&self, pos: PhysicalPosition<f64>) -> Option<crate::load_panel::Hit> {
        let panel = self.reloading()?;
        let layout = self.layout();
        let x = (pos.x as f32 - layout.bar_x) / layout.scale;
        let y = pos.y as f32 / layout.scale;
        crate::load_panel::hit(&panel, &self.load_state(), x, y)
    }

    /// A click on the load panel (0x4f9da0, 0x4fa140): an entry is chosen,
    /// and chosen again it goes to the other side; a group opens or closes;
    /// Kill item throws the chosen entry away; Finish reload ends.
    fn load_click(&mut self, pos: PhysicalPosition<f64>) -> bool {
        use crate::load_panel::Hit;
        let (Some(hit), Some(panel), Some(selected)) =
            (self.load_hit(pos), self.reloading(), self.selected)
        else {
            return false;
        };
        let mut view = self.load_state();
        self.load_pressed = Some(hit);
        match hit {
            Hit::Cell(side, i) => {
                let e = panel.entries[side][i];
                if !e.is_choosable() {
                    return true;
                }
                if e.is_group() {
                    view.open[side] = if e.flags & crate::scene::load::HEADER != 0 {
                        None
                    } else {
                        Some((e.cat, e.sub))
                    };
                    view.scroll[side] = 0;
                    view.chosen = (side, 0);
                } else if view.chosen == (side, i) {
                    if let Some(cargo) = e.cargo {
                        let order = Order::Transfer {
                            from: panel.holders[side],
                            to: panel.holders[1 - side],
                            cargo,
                        };
                        if let Err(e) = self.issue(order) {
                            self.say(e);
                        }
                    }
                } else {
                    view.chosen = (side, i);
                }
            }
            Hit::Kill => {
                if let Some((holder, cargo)) = crate::load_panel::chosen_cargo(&panel, &view) {
                    if let Err(e) = self.issue(Order::Discard { holder, cargo }) {
                        self.say(e);
                    }
                }
            }
            Hit::End => {
                if let Err(e) = self.issue(Order::EndLoad { id: selected }) {
                    self.say(e);
                }
            }
            Hit::Up(side) => view.scroll[side] = view.scroll[side].saturating_sub(2),
            Hit::Down(side) => {
                let n = panel.entries[side].len().saturating_sub(1);
                view.scroll[side] = (view.scroll[side] + 2).min(crate::load_panel::max_scroll(n));
            }
            Hit::Panel => {}
        }
        self.load_view = view;
        true
    }

    /// A window position as a point on the control bar, in original pixels.
    fn bar_point(&self, pos: PhysicalPosition<f64>) -> Option<(u32, u32)> {
        if self.bar_hidden {
            return None;
        }
        let layout = self.layout();
        let x = (pos.x as f32 - layout.bar_x) / layout.scale;
        let y = pos.y as f32 / layout.scale;
        // The bar is 208 wide, at either side.
        (x >= 0.0 && x < crate::ui::BAR_W as f32 && y >= 0.0).then_some((x as u32, y as u32))
    }

    /// The panels' state, for hit testing.
    fn panel_view(&self) -> PanelView<'_> {
        PanelView {
            tab: self.tab,
            saves: &self.save_names,
            save_first: self.save_first,
            selected_save: self.selected_save,
            file_mode: self.file_mode,
            name: &self.save_name,
            config: &self.settings.config,
            modern_mouse: self.settings.modern_mouse,
            tracks: &self.track_names,
            help: &self.help_text,
            help_first: self.help_first,
            help_page: self.help_page,
            pressed: self.panel_pressed,
            time_ms: self.time_ms,
        }
    }

    /// Handles a click on the bar's tabs and panels; true if it hit one.
    fn panel_click(&mut self, pos: PhysicalPosition<f64>) -> bool {
        let Some(p) = self.bar_point(pos) else {
            return false;
        };
        let Some(hit) = self.interface.panels.hit(&self.panel_view(), p) else {
            // Clicks elsewhere on an open panel do nothing.
            return Panels::covers_bar(self.tab) && p.1 < 558;
        };
        self.panel_pressed = Some(hit);
        match hit {
            PanelHit::Tab(tab) => {
                self.tab = tab;
                self.refresh_panels();
            }
            PanelHit::HelpUp => match self.help_page {
                Some(page) => self.help_page = Some(page.saturating_sub(1)),
                None => self.help_first = self.help_first.saturating_sub(1),
            },
            PanelHit::HelpDown => match self.help_page {
                Some(page) => {
                    let last = self.interface.panels.help_pages().saturating_sub(1);
                    self.help_page = Some((page + 1).min(last));
                }
                None => {
                    let panels = &self.interface.panels;
                    let last = panels
                        .help_line_count(&self.help_text, &self.text)
                        .saturating_sub(panels.help_rows());
                    self.help_first = (self.help_first + 1).min(last);
                }
            },
            // The objectives and quick help tabs (0x406, 0x407); quick help
            // opens on its contents.
            PanelHit::QuickHelp(quick) => {
                if quick != self.help_page.is_some() {
                    self.help_page = quick.then_some(0);
                }
            }
            PanelHit::HelpLink(page) => self.help_page = Some(page),
            PanelHit::FileMode(mode) => self.file_mode = mode,
            // Choosing a saved game puts its name in the field (0x534).
            PanelHit::SaveRow(i) => {
                self.selected_save = Some(i);
                self.save_name = self.save_names[i].clone();
            }
            PanelHit::ListScroll(d) => {
                let max = self.saves.len().saturating_sub(Panels::save_rows());
                self.save_first = (self.save_first as i64 + d as i64).clamp(0, max as i64) as usize;
            }
            PanelHit::ListThumb(first) => self.save_first = first,
            PanelHit::FileAction => self.file_action(),
            // Each asks first (the file panel sends 0x4fd's question,
            // 0x4f5 and 0x4ef, 0x50a700).
            PanelHit::Restart => self.ask = Some(Ask::Restart),
            PanelHit::MainMenu => self.ask = Some(Ask::MainMenu),
            PanelHit::Quit => self.ask = Some(Ask::Quit),
            PanelHit::Slider(slider, v) => {
                self.slider_drag = Some(slider);
                self.set_slider(slider, v);
            }
            PanelHit::Check(Check::ModernMouse) => {
                let on = !self.settings.modern_mouse;
                self.settings.modern_mouse = on;
                crate::launch::set_modern_mouse(on);
                self.say(if on {
                    "Modern mouse: right click gives orders"
                } else {
                    "Original mouse: right click changes the pointer"
                });
            }
            PanelHit::Check(c) => {
                self.settings.config.toggle(c);
                self.config_changed();
            }
            PanelHit::BarLeft(left) => {
                self.settings.config.bar_left = left;
                self.config_changed();
            }
            PanelHit::Music(m) => {
                if self.settings.config.music != m {
                    self.settings.config.music = m;
                    self.config_changed();
                    self.music = None;
                    self.start_music();
                }
            }
        }
        true
    }

    /// The file panel's wide button (0x50a700 message 0x4fa): saves under
    /// the name typed, or loads or deletes the chosen game.
    /// The file panel's action button (0x4fb, 0x50a700): saving asks
    /// before overwriting a slot (string 18) and wants a name (19);
    /// loading and deleting ask first (17, 14).
    fn file_action(&mut self) {
        match self.file_mode {
            FileMode::Save => {
                if self.net.is_some() {
                    self.say("Multiplayer games cannot be saved");
                    return;
                }
                let name = self.save_slot_name();
                if name.is_empty() {
                    self.ask = Some(Ask::NoName);
                } else if self.save_names.contains(&name) {
                    self.ask = Some(Ask::Overwrite);
                } else {
                    self.save_now();
                }
            }
            FileMode::Load if self.selected_save.is_some() => self.ask = Some(Ask::Load),
            FileMode::Delete if self.selected_save.is_some() => self.ask = Some(Ask::Delete),
            _ => {}
        }
    }

    /// The name typed for a saved game, without characters files cannot
    /// have.
    fn save_slot_name(&self) -> String {
        self.save_name
            .trim()
            .chars()
            .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
            .collect()
    }

    /// The game to save, with the play-through's id (saves write it,
    /// 0x511926).
    fn saved_scene(&self) -> SaveGame {
        let mut save = self.scene.save();
        save.game_id = Some(self.game_id);
        save
    }

    /// A campaign mission is being played: its campaign's bit (game +
    /// 0x3214, set by 0x537f60 from `miss<campaign><mission>`).
    fn note_campaign(&mut self) {
        let mission = &self.missions[self.mission_index];
        if let Some(c) = crate::scene::is_campaign(mission)
            .then(|| mission[4..5].parse::<u32>().ok())
            .flatten()
        {
            self.campaign_mask = 1 << c;
        }
    }

    fn save_now(&mut self) {
        let name = self.save_slot_name();
        let path = self.data.save_dir().join(format!("{name}.json"));
        match self
            .saved_scene()
            .write(&path, self.settings.config.pack_saves)
        {
            Ok(()) => {
                self.say("Game saved");
                self.refresh_panels();
                self.selected_save = self.save_names.iter().position(|n| *n == name);
            }
            Err(e) => {
                self.say(format!("Could not save: {e:#}"));
                self.ask = Some(Ask::SaveFailed);
            }
        }
    }

    fn set_slider(&mut self, slider: Slider, v: u32) {
        self.settings.config.set_slider(slider, v);
        self.config_changed();
    }

    /// Keeps the settings and applies the volumes.
    fn config_changed(&mut self) {
        crate::launch::set_panel_config(&self.settings.config);
        self.apply_volumes();
    }

    /// Sets the effects' and the music's volumes from the sound panel's.
    fn apply_volumes(&mut self) {
        let c = &self.settings.config;
        // The wave and master controls scale linearly over the
        // DirectSound attenuations (as the system mixer would).
        let (wave, master) = (c.fx_volume as f32 / 4000.0, c.main_volume as f32 / 4000.0);
        let effects = panels::Config::gain(c.effects_volume) * wave * master;
        self.music_volume = panels::Config::gain(c.music_volume) * master;
        let music = self.music_volume;
        let muted = self.audio.as_ref().is_some_and(|a| a.muted);
        if let Some(a) = self.audio.as_mut() {
            a.volume = effects;
        }
        if let Some(m) = &self.music {
            m.set_volume(if muted { 0.0 } else { music });
        }
        if let Some(s) = &self.menu_music {
            s.set_volume(if muted { 0.0 } else { effects });
        }
    }

    /// What front-end screen `state` shows.
    fn menu_page(&self, state: MenuState) -> MenuPage {
        let empty = MenuPage {
            backdrop: Backdrop::TitleDim,
            title: None,
            items: Vec::new(),
            selected: None,
            text: Vec::new(),
            notes: Vec::new(),
            buttons: Vec::new(),
            first: self.list_first,
            rows: 0,
            credits: false,
            story: None,
            story_style: None,
            story_first: 0,
            labels: Vec::new(),
            checks: Vec::new(),
            list_rect: None,
            picture: None,
            boxes: Vec::new(),
            tabs: Vec::new(),
            sprites: Vec::new(),
            texts: Vec::new(),
            map: None,
            row_height: None,
            spots: Vec::new(),
            tracks: Vec::new(),
            fills: Vec::new(),
            top_sprites: Vec::new(),
            top_texts: Vec::new(),
        };
        let mut page = self.menu_page_of(state, empty);
        // What is drawn and what is clicked start at the same row.
        if page.rows == 0 {
            page.rows = match (page.list_rect, page.row_height) {
                (Some(r), Some(h)) => (r.3 / h).floor() as usize,
                (Some(r), None) => crate::menus::list_rows(r),
                (None, _) => crate::menus::VISIBLE_ROWS,
            };
        }
        page.first = crate::menus::list_window(page.first, page.items.len(), page.rows);
        page
    }

    fn menu_page_of(&self, state: MenuState, empty: MenuPage) -> MenuPage {
        let button = |caption: &str, x: f32, y: f32| Button {
            caption: caption.into(),
            x,
            y,
            shape: 0,
        };
        let column = |captions: &[&str]| -> Vec<Button> {
            captions
                .iter()
                .enumerate()
                // Closer together when there are many, above the frieze.
                .map(|(i, c)| {
                    let step = (300.0 / (captions.len().max(2) - 1) as f32).min(52.0);
                    button(c, 590.0, 220.0 + i as f32 * step)
                })
                .collect()
        };
        match state {
            // The original's title screen (Startmm.cpp, 0x530cb0): six
            // buttons down a diagonal from (70, 232), 40 right and 52 down
            // apart, the two wide shapes taking turns; captions are string
            // resources 1042, 101, 1027, 1026, 1000 and 103.
            MenuState::Main => MenuPage {
                backdrop: Backdrop::Title,
                buttons: TITLE_BUTTONS
                    .iter()
                    .enumerate()
                    .map(|(n, c)| Button {
                        caption: (*c).into(),
                        x: 30.0 + 40.0 * (n + 1) as f32,
                        y: 180.0 + 52.0 * (n + 1) as f32,
                        shape: (n % 2) as u8,
                    })
                    .collect(),
                credits: true,
                ..empty
            },
            // "Select custom mission" as the original's (Startspl.cpp
            // 0x53b940, its widgets by Startcom.cpp 0x52c5c0): the map
            // panel with its scroll bars and the tick, the description, the
            // missions and the players, each where the original creates it.
            MenuState::Missions(category, selected) => {
                let list = self.screen_missions(category);
                let chosen = selected.and_then(|i| list.get(i)).copied();
                let preview = self.preview.as_ref().filter(|p| Some(p.index) == chosen);
                self.custom_page(&list, selected, preview, empty)
            }
            MenuState::Load(tab, selected) => {
                let entries = self.load_entries(tab);
                let chosen = selected.and_then(|i| entries.get(i)).map(|e| e.0);
                let preview = self
                    .preview
                    .as_ref()
                    .filter(|p| chosen.and_then(|e| self.entry_mission(e)) == Some(p.index));
                self.load_page(tab, &entries, selected, preview, empty)
            }
            // "Your mission" (Starttsk.cpp, 0x53ed40, 0x53f360 and
            // 0x53f550): the briefing in a box at (20, 80, 630, 440),
            // "Difficulty level:" (string 105) at (660, 405) over three
            // boxes from (660, 440), 30 apart (strings 106-108); Start
            // (1010) at (244, 543) and Cancel (1002) at (440, 543), or Start
            // alone at (335, 543) in a campaign past its first mission (game
            // + 0x3215 not 0).
            MenuState::Briefing => {
                let mut story = self.scene.briefing_text.clone();
                if story.is_empty() {
                    story.push(self.scene.mission.to_uppercase());
                }
                let on = self.campaign_number().is_some_and(|n| n != 0);
                let mut buttons = vec![Button {
                    caption: "Start".into(),
                    x: if on { 335.0 } else { 244.0 },
                    y: 543.0,
                    shape: 2,
                }];
                if !on {
                    buttons.push(Button {
                        caption: "Cancel".into(),
                        x: 440.0,
                        y: 543.0,
                        shape: 3,
                    });
                }
                MenuPage {
                    backdrop: Backdrop::Mission,
                    story: Some(((20.0, 80.0, 630.0, 440.0), story)),
                    labels: vec![("Difficulty level:".into(), 660.0, 405.0)],
                    checks: ["Easy", "Normal", "Hard"]
                        .iter()
                        .enumerate()
                        .map(|(i, c)| crate::menus::Check {
                            caption: (*c).into(),
                            x: 660.0,
                            y: 440.0 + 30.0 * i as f32,
                            checked: self.difficulty == i,
                        })
                        .collect(),
                    buttons,
                    ..empty
                }
            }
            // The original's screen (Startba.cpp, 0x528140, 0x528210,
            // 0x528390 and 0x528520): the RAM_BEST picture at ((800 - 2 x
            // 380 - 10) / 2, (600 - 2 x 212 - 10) / 2 + 212 + 10), its title
            // at (9, 1) in it in BH, and the campaign type's seven rows,
            // names at (25, 339 + 24 k) 286 wide and scores centred at
            // (311, ...) 75 wide, in MH, a row shown when it has a name and
            // a score; variant 2 when the CD installed the campaign, else
            // 0. OK (string 1001) at (345, 540).
            MenuState::Best => {
                use crate::menus::{Placed, Style, Text};
                let kind = self.score_kind(true).unwrap_or(0);
                let variant = if self.campaigns.has(kind) { 2 } else { 0 };
                let style = |font| Style {
                    font,
                    variant,
                    spacing: 1,
                };
                let mut texts = vec![Text {
                    style: style("BH"),
                    x: 15.0 + 9.0,
                    y: 305.0 + 1.0,
                    text: self.campaigns.title(kind, &self.data.strings),
                    clip: None,
                }];
                texts.extend(self.score_rows(kind, None, (25.0, 339.0), style("MH"), |_| variant));
                MenuPage {
                    backdrop: Backdrop::Best,
                    sprites: vec![Placed {
                        name: "RAM_BEST".into(),
                        x: 15.0,
                        y: 305.0,
                    }],
                    texts,
                    buttons: vec![Button {
                        caption: "OK".into(),
                        x: 345.0,
                        y: 540.0,
                        shape: 2,
                    }],
                    ..empty
                }
            }
            // The original's multiplayer screen (Startmp, fitted to it):
            // the player's name with its cursor, the ways to connect (OpenConquest
            // offers its own network game, over TCP/IP), New, Join, Cancel.
            MenuState::Connect => {
                use crate::menus::{Placed, Style, Text};
                let at = |name: &str, x: f32, y: f32| Placed {
                    name: name.into(),
                    x,
                    y,
                };
                let style = |font, variant| Style {
                    font,
                    variant,
                    spacing: 1,
                };
                let text = |style, x, y, t: &str| Text {
                    style,
                    x,
                    y,
                    text: t.into(),
                    clip: None,
                };
                let name_style = EDIT_STYLE;
                // The name's edit (type 6) at (43, 256), its text from its
                // first column shown, its caret in palette colour 0xfb
                // (0x538440, 0x563140) at the caret's x.
                let caret_colour = self
                    .menus
                    .as_ref()
                    .and_then(|m| m.colour(Backdrop::TitleDim, 0xfb));
                let edit = self.connect_edit;
                let shown = columns(&self.player_name, edit.first, usize::MAX);
                let at_button = |caption: &str, x, shape| Button {
                    caption: caption.into(),
                    x,
                    y: 510.0,
                    shape,
                };
                MenuPage {
                    backdrop: Backdrop::TitleDim,
                    sprites: vec![
                        at("PROV_NAME", 30.0, 225.0),
                        at("PROV_RAM1", 30.0, 290.0),
                        at("PROV_RAM0", 38.0, 320.0),
                        at("M_SBUT_UP2", 472.0, 317.0),
                        at("M_SBUT_DN2", 472.0, 448.0),
                        at("PROV_TR0", 472.0, 349.0),
                        at("PROV_TR1", 476.0, 353.0),
                    ],
                    texts: vec![
                        text(style("BH", 2), 43.0, 228.0, "Player's name"),
                        text(style("BH", 2), 43.0, 293.0, "Select communication channel"),
                        Text {
                            clip: Some(CONNECT_EDIT.2),
                            ..text(name_style, 43.0, 256.0, shown)
                        },
                        text(style("MH", 2), 43.0, 321.0, "Internet TCP/IP Connection"),
                    ],
                    // While the edit is not left (Esc).
                    fills: caret_colour
                        .filter(|_| edit.active)
                        .map(|c| vec![((43.0 + edit.x, 256.0, 2.0, 18.0), c)])
                        .unwrap_or_default(),
                    buttons: vec![
                        at_button("New", 60.0, 2),
                        at_button("Join", 215.0, 3),
                        at_button("Cancel", 365.0, 2),
                    ],
                    ..empty
                }
            }
            MenuState::Multiplayer(selected) => {
                let here =
                    crate::net::local_address().map_or("unknown".to_string(), |a| a.to_string());
                MenuPage {
                    title: Some("Multiplayer".into()),
                    items: self
                        .battle_maps
                        .iter()
                        .map(|&(i, n)| format!("{}  (up to {n})", self.mission_titles[i]))
                        .collect(),
                    selected,
                    notes: vec![
                        format!(
                            "Players: {}   (host a game of the map chosen)",
                            self.lobby_players
                        ),
                        format!("Join address: {}_   (type it)", self.join_address),
                        format!("Your address: {here}"),
                    ],
                    buttons: column(&["Host", "Join", "More players", "Fewer players", "Back"]),
                    ..empty
                }
            }
            MenuState::Waiting => {
                let mut text = Vec::new();
                if let Some(l) = &self.hosting {
                    text.push(format!("Hosting {}", l.setup.mission.to_uppercase()));
                    text.push(format!(
                        "Waiting for {} more player(s). Others join with the address {}:{}.",
                        l.missing(),
                        crate::net::local_address().map_or("?".into(), |a| a.to_string()),
                        l.port
                    ));
                }
                if let Some(j) = &self.joining {
                    text.push(format!("Joined {}", j.address));
                    text.push(match j.seat() {
                        Some(k) => format!(
                            "You are player {}. Waiting for the others and the host.",
                            k + 1
                        ),
                        None => "Waiting for the host to answer.".into(),
                    });
                }
                MenuPage {
                    title: Some("Multiplayer".into()),
                    text,
                    buttons: column(&["Cancel"]),
                    ..empty
                }
            }
            // The end screen (Startrpt.cpp, 0x5390a0, 0x539350 and
            // 0x539b20): its backdrop by the outcome and OK (string 1001)
            // at (345, 540). After a won campaign mission it also shows
            // the RAM_BEST picture at (400, 310) titled "Best Conquerors"
            // (1042) at (9, 1) in it, and the twelve statistics: labels
            // (1047-1058) at (15, 85 + 30 i, 15 more from the 6th, the 8th,
            // 30 from the 12th) in BH, cut at 420 wide (300 from the 8th),
            // the values 5 right of them, 80 wide; and the campaign's rows of
            // the Best Conquerors table with the name edit over the row
            // just entered (`open_outcome`).
            MenuState::Outcome(outcome) => {
                use crate::menus::{Placed, Style, Text};
                let won = outcome == crate::scene::Outcome::Victory;
                let mut sprites = Vec::new();
                let mut texts = Vec::new();
                let mut fills = Vec::new();
                if won && self.campaign_number().is_some() {
                    let style = |spacing| Style {
                        font: "BH",
                        variant: 2,
                        spacing,
                    };
                    sprites.push(Placed {
                        name: "RAM_BEST".into(),
                        x: 400.0,
                        y: 310.0,
                    });
                    texts.push(Text {
                        style: style(1),
                        x: 409.0,
                        y: 311.0,
                        text: STRING_BEST.into(),
                        clip: None,
                    });
                    let values = self.scene.stats.values(self.scene.amber);
                    for (i, (label, value)) in STAT_LABELS.iter().zip(values).enumerate() {
                        // 7 more than BH's tallest glyph (23) apart.
                        let pitch = self
                            .menus
                            .as_ref()
                            .map_or(0.0, |m| m.tallest(style(2)) + 7.0);
                        let y = 85.0
                            + pitch * i as f32
                            + if i >= 5 { 15.0 } else { 0.0 }
                            + if i >= 7 { 15.0 } else { 0.0 }
                            + if i >= 11 { 30.0 } else { 0.0 };
                        let w = if i < 7 { 420.0 } else { 300.0 };
                        // Each centred down in a box of the font's tallest
                        // glyph's height.
                        let mut put = |style: Style, x: f32, w: f32, t: String| {
                            let dy = self.menus.as_ref().map_or(0.0, |m| {
                                ((m.tallest(style) - m.height(style, &t)) / 2.0).floor()
                            });
                            texts.push(Text {
                                style,
                                x,
                                y: y + dy,
                                text: t,
                                clip: Some(w),
                            });
                        };
                        put(style(2), 15.0, w, (*label).into());
                        put(style(1), 15.0 + w + 5.0, 80.0, value.to_string());
                    }
                    // The table's rows: names in variant 1, scores in 1, the
                    // row just entered's in 2 (0x539b20).
                    let entry = self.score_entry.as_ref();
                    let kind = entry.map(|e| e.kind).or(self.score_kind(false));
                    if let Some(kind) = kind {
                        let row = entry.map(|e| e.row);
                        let name = Style {
                            font: "MH",
                            variant: 1,
                            spacing: 1,
                        };
                        texts.extend(self.score_rows(kind, row, (410.0, 344.0), name, |k| {
                            1 + usize::from(Some(k) == row)
                        }));
                    }
                    // The edit over the entered row (410, 344 + 24 i, 286,
                    // h + 1): its text from the top left in MH variant 2, one
                    // apart, and while it is active a caret 2 wide and h high
                    // in palette colour 0xfb (0x4eb140, as the connect
                    // screen's).
                    if let (Some(e), Some(menus)) = (entry, &self.menus) {
                        let style = Style {
                            font: "MH",
                            variant: 2,
                            spacing: 1,
                        };
                        let h = menus.tallest(style);
                        let y = 310.0 + 34.0 + (h + 6.0) * e.row as f32;
                        texts.push(Text {
                            style,
                            x: 410.0,
                            y,
                            text: columns(&e.text, e.edit.first, usize::MAX).into(),
                            clip: Some(SCORE_EDIT_WIDTH),
                        });
                        if e.edit.active {
                            if let Some(c) = menus.colour(Backdrop::Accomplished, 0xfb) {
                                fills.push(((410.0 + e.edit.x, y, 2.0, h), c));
                            }
                        }
                    }
                }
                MenuPage {
                    backdrop: if won {
                        Backdrop::Accomplished
                    } else {
                        Backdrop::Failed
                    },
                    sprites,
                    texts,
                    fills,
                    buttons: vec![Button {
                        caption: "OK".into(),
                        x: 345.0,
                        y: 540.0,
                        shape: 2,
                    }],
                    ..empty
                }
            }
        }
    }

    /// Campaign type `kind`'s rows of the Best Conquerors table as both
    /// screens draw them (0x528520, 0x539b20 through 0x4eb430): row k's
    /// name from `origin` + (0, (h + 6) k), h MH's tallest glyph, in a box
    /// 286 wide and h high, centred down; its score in the box 75 wide
    /// after it, centred, in `score_variant(k)`; a row shown when it has a
    /// name and a score. Row `skip`'s name is under the edit.
    fn score_rows(
        &self,
        kind: usize,
        skip: Option<usize>,
        origin: (f32, f32),
        name_style: crate::menus::Style,
        score_variant: impl Fn(usize) -> usize,
    ) -> Vec<crate::menus::Text> {
        let Some(menus) = &self.menus else {
            return Vec::new();
        };
        let h = menus.tallest(name_style);
        let mut texts = Vec::new();
        for (k, row) in self.best.rows(kind).iter().enumerate() {
            if row.name.is_empty() || row.score == 0 {
                continue;
            }
            let y = origin.1 + (h + 6.0) * k as f32;
            // Halves as the C division takes them (toward 0).
            let centred = |style, text: &str, w: f32| {
                let dx = (w as i32 - menus.measure(style, text) as i32) / 2;
                let dy = (h as i32 - menus.height(style, text) as i32) / 2;
                (dx as f32, dy as f32)
            };
            if skip != Some(k) {
                let (_, dy) = centred(name_style, &row.name, 286.0);
                texts.push(crate::menus::Text {
                    style: name_style,
                    x: origin.0,
                    y: y + dy,
                    text: row.name.clone(),
                    clip: Some(286.0),
                });
            }
            let style = crate::menus::Style {
                variant: score_variant(k),
                ..name_style
            };
            let score = row.score.to_string();
            let (dx, dy) = centred(style, &score, 75.0);
            texts.push(crate::menus::Text {
                style,
                x: origin.0 + 286.0 + dx,
                y: y + dy,
                text: score,
                clip: None,
            });
        }
        texts
    }

    /// The campaign mission number of the mission played (game + 0x3215,
    /// set by 0x537f60 from `miss%d%02d`): none outside the campaign.
    fn campaign_number(&self) -> Option<u32> {
        let mission = &self.missions[self.mission_index];
        if !crate::scene::is_campaign(mission) {
            return None;
        }
        mission[5..].parse().ok()
    }

    /// The missions a mission screen lists: for Custom missions, as the
    /// original (0x52b7c0: `<MainPath>CUSTOM\*.DKD`, a file kept when its
    /// `HEAD_CUST` a is set), the battles and single missions (the
    /// installation's custom missions, every one with a = 1) by file name.
    fn screen_missions(&self, category: Category) -> Vec<usize> {
        if category != Category::Single {
            return self.category_missions(category);
        }
        let mut list: Vec<usize> = (0..self.missions.len())
            .filter(|&i| {
                matches!(
                    Category::of(&self.missions[i]),
                    Some(Category::Single | Category::Battle)
                ) && self.custom_listed.get(i) == Some(&true)
            })
            .collect();
        list.sort_by(|&a, &b| self.missions[a].cmp(&self.missions[b]));
        list
    }

    /// A mission screen's list bar (Ourlib's _slist.cpp, 0x54cd00): its
    /// arrows and track move the chosen entry (the cursor, the first when
    /// none is chosen), the window following it; the up arrow at
    /// (x, 241) and the down one at `down`, the track picture `track` (its
    /// name without the frame, its height) at (x, 265).
    fn list_bar(
        &self,
        n: usize,
        selected: Option<usize>,
        rows: usize,
        down: (f32, f32),
        track: (&str, f32),
    ) -> ListBar {
        use crate::menus::{Placed, Scroller, Track};
        let x = down.0;
        let cursor = selected.unwrap_or(0).min(n.saturating_sub(1));
        let bar = Track::at(x, 265.0, track.1, true, n, cursor, Scroller::List);
        let arrow = |dir: &str, can: bool| format!("M_LBUT_{dir}{}", if can { 0 } else { 2 });
        let at = |name: String, x: f32, y: f32| Placed { name, x, y };
        let (tx, ty) = bar.thumb();
        ListBar {
            first: crate::menus::list_window(self.list_first, n, rows),
            sprites: vec![
                at(arrow("UP", cursor > 0), x, 241.0),
                at(arrow("DN", cursor + 1 < n), down.0, down.1),
                at(format!("{}0", track.0), x, 265.0),
                at(format!("{}1", track.0), tx, ty),
            ],
            spots: vec![
                ((x, 241.0, 23.0, 23.0), MenuHit::Step(Scroller::List, -1)),
                (
                    (down.0, down.1, 23.0, 23.0),
                    MenuHit::Step(Scroller::List, 1),
                ),
            ],
            track: bar,
        }
    }

    /// The map and description panels of the mission screens (Custom
    /// missions, Load game), as the original's (Startcom.cpp, 0x52c5c0):
    /// the map in a viewer (Ourlib's _sviewer.cpp) of 19 x 19 cells, 8
    /// pixels a cell, scrolled to the player's starting view; the
    /// description in a read-only edit (_sedit.cpp); each with arrows and a
    /// scroll bar (_sslider.cpp). The titles bright once a mission is chosen.
    fn mission_panels(&self, preview: Option<&Preview>) -> MissionPanels {
        use crate::menus::{Placed, Scroller, Style, Text, Track};
        let at = |name: &str, x: f32, y: f32| Placed {
            name: name.into(),
            x,
            y,
        };
        let title = Style {
            font: "BH",
            variant: if preview.is_some() { 2 } else { 1 },
            spacing: 1,
        };
        let size = preview.map_or((0, 0), |p| p.size);
        let texts = vec![
            Text {
                style: title,
                x: 16.0,
                y: 4.0,
                text: format!("Map ({}x{})", size.0, size.1),
                clip: None,
            },
            // Bright whether or not a mission is chosen (the original's).
            Text {
                style: Style {
                    variant: 2,
                    ..title
                },
                x: 209.0,
                y: 28.0,
                text: "Mission description".into(),
                clip: None,
            },
        ];
        let mut sprites = vec![at("MP_MAP", 3.0, 3.0), at("MP_DESC", 196.0, 27.0)];
        // The map: a position a cell, as many as the cells beyond the
        // viewer's plus one (0x54c320); the viewer at (3 + 7, 3 + 27),
        // 19 cells of 8 (0x52c5c0).
        const VIEW: u32 = MAP_CELLS as u32 * 8;
        let mut map = None;
        let (mut along, mut count) = ((0, 0), (0, 0));
        if let Some(p) = preview {
            if let Some(m) = &p.map {
                count = (
                    scroll_positions(p.size.0 as usize, MAP_CELLS),
                    scroll_positions(p.size.1 as usize, MAP_CELLS),
                );
                let (x, y) = self
                    .map_at
                    .unwrap_or((p.start.0 as usize, p.start.1 as usize));
                along = (
                    x.min(count.0.saturating_sub(1)),
                    y.min(count.1.saturating_sub(1)),
                );
                let off = (
                    (along.0 as u32 * 8).min(m.width.saturating_sub(VIEW)),
                    (along.1 as u32 * 8).min(m.height.saturating_sub(VIEW)),
                );
                map = Some((m.clone(), (10.0, 30.0, VIEW as f32, VIEW as f32), off));
            }
        }
        // The track pictures' lengths: MAP_TR_V0, MAP_TR_H0 and DESC_TR0
        // are 107 pixels long.
        let down = Track::at(165.0, 52.0, 107.0, true, count.1, along.1, Scroller::MapY);
        let across = Track::at(32.0, 185.0, 107.0, false, count.0, along.0, Scroller::MapX);
        // The description, a position a line, as many as the lines past
        // the box plus one (0x54ee70); none without a mission.
        let story = preview.map(|p| p.text.clone()).unwrap_or_default();
        let lines = self.menus.as_ref().map_or(0, |m| {
            m.story_lines(Some(STORY_STYLE), STORY_BOX.2, &story).len()
        });
        let rows = crate::menus::story_rows(STORY_BOX, STORY_STYLE.1);
        let lines_count = if story.is_empty() {
            0
        } else {
            scroll_positions(lines, rows).max(1)
        };
        let first = self.desc_first.min(lines_count.saturating_sub(1));
        let about = Track::at(
            766.0,
            76.0,
            107.0,
            true,
            lines_count,
            first,
            Scroller::Story,
        );
        // An arrow is live while it can move its bar (picture 0, else 2).
        let arrow = |dir: &str, t: &Track, back: bool| {
            let can = if back { t.pos > 0 } else { t.pos + 1 < t.count };
            format!("M_LBUT_{dir}{}", if can { 0 } else { 2 })
        };
        let thumb = |name: &str, t: &Track| {
            let (x, y) = t.thumb();
            at(name, x, y)
        };
        sprites.extend([
            at(&arrow("UP", &down, true), 165.0, 28.0),
            at(&arrow("DN", &down, false), 165.0, 160.0),
            at("MAP_TR_V0", 165.0, 52.0),
            thumb("MAP_TR_V1", &down),
            at(&arrow("LF", &across, true), 8.0, 185.0),
            at(&arrow("RT", &across, false), 140.0, 185.0),
            at("MAP_TR_H0", 32.0, 185.0),
            thumb("MAP_TR_H1", &across),
            // The map tick (0x52c5c0: `M_CHECK`, ticked with
            // [0x5ca538] set).
            at(
                if self.settings.config.map_tick {
                    "M_CHECK1"
                } else {
                    "M_CHECK0"
                },
                165.0,
                185.0,
            ),
            at(&arrow("UP", &about, true), 766.0, 52.0),
            at(&arrow("DN", &about, false), 766.0, 184.0),
            at("DESC_TR0", 766.0, 76.0),
            thumb("DESC_TR1", &about),
        ]);
        let arrow_spot =
            |x: f32, y: f32, s: Scroller, d: i32| ((x, y, 23.0, 23.0), MenuHit::Step(s, d));
        MissionPanels {
            sprites,
            texts,
            map,
            story,
            story_first: first,
            tracks: vec![down, across, about],
            spots: vec![
                arrow_spot(165.0, 28.0, Scroller::MapY, -1),
                arrow_spot(165.0, 160.0, Scroller::MapY, 1),
                arrow_spot(8.0, 185.0, Scroller::MapX, -1),
                arrow_spot(140.0, 185.0, Scroller::MapX, 1),
                arrow_spot(766.0, 52.0, Scroller::Story, -1),
                arrow_spot(766.0, 184.0, Scroller::Story, 1),
                ((165.0, 185.0, 23.0, 23.0), MenuHit::MapTick),
            ],
        }
    }

    /// The original's "Select mission to load" screen: the map and
    /// description panels, the list of saved games (or campaign missions)
    /// across the screen, the tabs Scenario, Custom and Start from...,
    /// Load and Cancel (positions fitted to the original).
    fn load_page(
        &self,
        tab: LoadTab,
        entries: &[(LoadEntry, String)],
        selected: Option<usize>,
        preview: Option<&Preview>,
        empty: MenuPage,
    ) -> MenuPage {
        use crate::menus::{Placed, Style, Text};
        let at = |name: &str, x: f32, y: f32| Placed {
            name: name.into(),
            x,
            y,
        };
        let style = |variant| Style {
            font: "BH",
            variant,
            spacing: 1,
        };
        let text = |style, x, y, t: &str| Text {
            style,
            x,
            y,
            text: t.into(),
            clip: None,
        };
        let panels = self.mission_panels(preview);
        let (mut sprites, mut texts) = (panels.sprites, panels.texts);
        sprites.push(at("LOAD_RM1", 3.0, 216.0));
        texts.extend([
            text(style(2), 396.0, 2.0, "Select mission to load"),
            text(style(2), 13.0, 217.0, "Missions"),
        ]);
        // The list (0x52fe90): from (3 + 9, 216 + 0x20), ten rows of the
        // LOAD_RM0 picture's height and a pixel; the chosen row on that
        // picture. Each text in MH at 4 across, centred down in the row's
        // 26 (the chosen one's in 24, pale).
        const ROWS: usize = 10;
        let bar = self.list_bar(
            entries.len(),
            selected,
            ROWS,
            (767.0, 500.0),
            ("LOAD_TR", 234.0),
        );
        let first = bar.first;
        for (k, (_, label)) in entries.iter().enumerate().skip(first).take(ROWS) {
            let y = 248.0 + 27.0 * (k - first) as f32;
            if selected == Some(k) {
                sprites.push(at("LOAD_RM0", 12.0, y));
            }
            let (variant, high) = if selected == Some(k) {
                (2, 24.0)
            } else {
                (1, 26.0)
            };
            let style = Style {
                font: "MH",
                variant,
                spacing: 1,
            };
            let th = self.menus.as_ref().map_or(0.0, |m| m.height(style, label));
            texts.push(text(style, 16.0, y + ((high - th) / 2.0).floor(), label));
        }
        sprites.extend(bar.sprites);
        // The tabs (0x4eaa70): the open one's picture and its caption pale,
        // centred in (0, 0, w, 28); a closed one's in variant 1, centred in
        // (1, 1, w, 28), a pixel right and down.
        let tabs = [
            ("Scenario", 322.0, LoadTab::Scenario, false),
            ("Custom", 452.0, LoadTab::Custom, false),
            ("Start from...", 582.0, LoadTab::StartFrom, true),
        ];
        for (caption, x, t, wide) in tabs {
            let open = t == tab;
            let picture = match (wide, open) {
                (false, false) => "M_LOADZ0",
                (false, true) => "M_LOADZ1",
                (true, false) => "M_LOADZ_BIG0",
                (true, true) => "M_LOADZ_BIG1",
            };
            sprites.push(at(picture, x, 528.0));
            let w = if wide { 214.0 } else { 130.0 };
            let tw = self
                .menus
                .as_ref()
                .map_or(0.0, |m| m.measure(style(if open { 2 } else { 1 }), caption));
            let shift = if open { 0.0 } else { 1.0 };
            texts.push(text(
                style(if open { 2 } else { 1 }),
                x + shift + ((w - tw) / 2.0).floor(),
                530.0 + shift,
                caption,
            ));
        }
        let tab_spot = |x: f32, w: f32, k: usize| ((x, 528.0, w, 28.0), MenuHit::Tab(k));
        let mut spots = panels.spots;
        spots.extend(bar.spots);
        spots.extend([
            tab_spot(322.0, 130.0, 0),
            tab_spot(452.0, 130.0, 1),
            tab_spot(582.0, 214.0, 2),
        ]);
        let mut tracks = panels.tracks;
        tracks.push(bar.track);
        MenuPage {
            backdrop: Backdrop::TitleDim,
            items: entries.iter().map(|e| e.1.clone()).collect(),
            selected,
            first,
            rows: ROWS,
            list_rect: Some((12.0, 248.0, 750.0, 27.0 * ROWS as f32)),
            row_height: Some(27.0),
            story: Some((STORY_BOX, panels.story)),
            story_style: Some(STORY_STYLE),
            story_first: panels.story_first,
            sprites,
            texts,
            map: panels.map,
            spots,
            tracks,
            buttons: vec![
                Button {
                    caption: "Load".into(),
                    x: 224.0,
                    y: 557.0,
                    shape: 2,
                },
                Button {
                    caption: "Cancel".into(),
                    x: 460.0,
                    y: 557.0,
                    shape: 3,
                },
            ],
            ..empty
        }
    }

    /// The original's "Select custom mission" screen.
    fn custom_page(
        &self,
        list: &[usize],
        selected: Option<usize>,
        preview: Option<&Preview>,
        empty: MenuPage,
    ) -> MenuPage {
        use crate::menus::{Placed, Style, Text};
        let at = |name: &str, x: f32, y: f32| Placed {
            name: name.into(),
            x,
            y,
        };
        let style = |font, variant| Style {
            font,
            variant,
            spacing: 1,
        };
        let title = style("BH", 2);
        let text = |style, x, y, t: &str| Text {
            style,
            x,
            y,
            text: t.into(),
            clip: None,
        };
        // Arrows: picture 0 when they can scroll, 2 when they cannot.
        let arrow = |dir: &str, can: bool| format!("M_LBUT_{dir}{}", if can { 0 } else { 2 });
        let panels = self.mission_panels(preview);
        let (mut sprites, mut texts) = (panels.sprites, panels.texts);
        sprites.extend([at("MP_LFILE1", 3.0, 216.0), at("SP_LPLAY1", 196.0, 216.0)]);
        texts.extend([
            text(title, 398.0, 2.0, "Select custom mission"),
            text(title, 13.0, 217.0, "Missions"),
            text(title, 209.0, 217.0, "Players"),
        ]);
        // The missions, 27 apart; the chosen one framed and pale.
        const ROWS: usize = 11;
        let bar = self.list_bar(
            list.len(),
            selected,
            ROWS,
            (165.0, 525.0),
            ("LFILE_TR", 259.0),
        );
        let first = bar.first;
        for (k, &i) in list.iter().enumerate().skip(first).take(ROWS) {
            let y = 249.0 + 27.0 * (k - first) as f32;
            if selected == Some(k) {
                sprites.push(at("MP_LFILE0", 11.0, y - 4.0));
            }
            // The chosen one pale and a pixel higher (fitted).
            let (variant, dy) = if selected == Some(k) {
                (2, -1.0)
            } else {
                (1, 0.0)
            };
            texts.push(text(style("MH", variant), 13.0, y + dy, &self.missions[i]));
        }
        sprites.extend(bar.sprites);
        // The players (0x53b940 message 0x195, row k from (204, 247 + 27k)):
        // the flag, LPLAY_TP at (26, 2) with the kind's caption and
        // LPLAY_NP at (171, 2) with the name, each in MH 4 across and
        // centred down, in the kind's variant; over them the kind field,
        // the arrow M_CBUT (pressed M_CBUT1, else M_CBUT0: 0x4ea6e0) and
        // the name field (0x529de0), the fields drawing nothing.
        let players = if preview.is_some() {
            self.custom_rows()
        } else {
            &[]
        };
        let frame = (self.time_ms / 100 % 10) as u32;
        let centred = |style, t: &str| {
            let h = self.menus.as_ref().map_or(0.0, |m| m.height(style, t));
            ((21 - h as i32) / 2) as f32
        };
        for (k, cp) in players.iter().enumerate() {
            let y = 249.0 + 27.0 * k as f32;
            let start = FLAG_STARTS[((cp.info.nation * 4 + cp.info.slot) as usize).min(15)];
            let arrow = if self.kind_held == Some((k, true)) {
                "M_CBUT1"
            } else {
                "M_CBUT0"
            };
            sprites.extend([
                at("LPLAY_FL", 206.0, y),
                at(&format!("M_FLAGS{:03}", start + frame), 209.0, y),
                at("LPLAY_TP", 230.0, y),
                at(arrow, 342.0, y),
                at("LPLAY_NP", 375.0, y),
            ]);
            let st = style("MH", kind_variant(cp.kind));
            let who = KIND_NAMES[usize::from(cp.kind.min(5))];
            texts.push(Text {
                clip: Some(106.0),
                ..text(st, 234.0, y + centred(st, who), who)
            });
            if !cp.name.is_empty() {
                texts.push(Text {
                    clip: Some(382.0),
                    ..text(st, 379.0, y + centred(st, &cp.name), &cp.name)
                });
            }
        }
        // The kind pop-up (0x52f270): its picture, the current entry's row
        // on LPLAY_TP, each entry's caption in MH variant 0, 4 across and
        // centred down in its row at (16, 12 + 21i), 110 x 21.
        let (mut top_sprites, mut top_texts) = (Vec::new(), Vec::new());
        if let Some(pop) = &self.kind_popup {
            top_sprites.push(at(pop.picture().0, pop.x, pop.y));
            let st = style("MH", 0);
            for (i, &v) in pop.entries.iter().enumerate() {
                let (rx, ry) = (pop.x + 16.0, pop.y + 12.0 + 21.0 * i as f32);
                if i == pop.current {
                    top_sprites.push(at("LPLAY_TP", rx, ry));
                }
                let t = KIND_NAMES[usize::from(v)];
                top_texts.push(Text {
                    clip: Some(106.0),
                    ..text(st, rx + 4.0, ry + centred(st, t), t)
                });
            }
        }
        sprites.extend([
            at(&arrow("UP", false), 766.0, 241.0),
            at(&arrow("DN", false), 766.0, 525.0),
            at("SLPLAY_TR0", 766.0, 265.0),
            at("SLPLAY_TR1", 770.0, 269.0),
        ]);
        MenuPage {
            backdrop: Backdrop::TitleDim,
            items: list.iter().map(|&i| self.missions[i].clone()).collect(),
            selected,
            list_rect: Some((8.0, 245.0, 155.0, 27.0 * ROWS as f32)),
            row_height: Some(27.0),
            first,
            rows: ROWS,
            story: Some((STORY_BOX, panels.story)),
            story_style: Some(STORY_STYLE),
            story_first: panels.story_first,
            sprites,
            texts,
            map: panels.map,
            tracks: {
                let mut tracks = panels.tracks;
                tracks.push(bar.track);
                tracks
            },
            spots: {
                let mut spots = panels.spots;
                spots.extend(bar.spots);
                for k in 0..players.len() {
                    let y = 249.0 + 27.0 * k as f32;
                    spots.push(((230.0, y, 110.0, 21.0), MenuHit::Kind(k, false)));
                    spots.push(((342.0, y, 29.0, 21.0), MenuHit::Kind(k, true)));
                }
                spots
            },
            top_sprites,
            top_texts,
            buttons: vec![
                Button {
                    caption: "Start".into(),
                    x: 224.0,
                    y: 558.0,
                    shape: 2,
                },
                Button {
                    caption: "Cancel".into(),
                    x: 460.0,
                    y: 558.0,
                    shape: 3,
                },
            ],
            ..empty
        }
    }

    /// Missions of `category`, as indices into `missions`.
    fn category_missions(&self, category: Category) -> Vec<usize> {
        (0..self.missions.len())
            .filter(|&i| Category::of(&self.missions[i]) == Some(category))
            .collect()
    }

    /// The campaign mission a won one goes on to: the number its brain
    /// hands the judge's end (0x478e30, game + 0x3305), NN + 1 for
    /// `miss<c>NN` up to miss012 and -1 (the campaign's end) for miss013
    /// (miss000 gives -1 only with the edition word + 0xa81 = 0xd0dd,
    /// which the retail install never has); found by name, `miss<c><NN>`
    /// (0x539b20).
    fn next_mission(&self) -> Option<usize> {
        let n = self.campaign_number()?;
        if n >= 13 {
            return None;
        }
        let name = format!("{}{:02}", &self.missions[self.mission_index][..5], n + 1);
        self.missions.iter().position(|m| *m == name)
    }

    /// Opens mission `index` on its briefing screen.
    /// Starts the multiplayer game `net` was set up for: its mission with
    /// its human players seated, this machine's player active.
    pub fn start_multiplayer(&mut self, net: crate::net::Net) -> Result<()> {
        let setup = &net.setup;
        let mut scene = Scene::load_at(
            &self.data,
            &mut self.caches,
            &setup.mission,
            setup.difficulty,
        )?;
        scene.fog = setup.fog;
        let archive = self.data.open_mission(&setup.mission)?;
        let players = openconquest_formats::players(&archive)?;
        let seated: Vec<_> = setup
            .seats
            .iter()
            .filter_map(|&(n, s)| {
                players
                    .iter()
                    .find(|p| (p.nation, p.slot) == (n, s))
                    .copied()
            })
            .collect();
        if seated.len() != setup.seats.len() {
            anyhow::bail!("{} has no such players", setup.mission);
        }
        scene.seat(&seated);
        scene.sit(net.seat);
        scene.refresh_sight();
        if let Some(i) = self.missions.iter().position(|m| *m == setup.mission) {
            self.mission_index = i;
        }
        self.game_id = crate::records::game_id();
        self.scene = scene;
        // Every game build shows the bar (0x4ebf50: [game+0x328d] = 0).
        self.bar_hidden = false;
        self.fog = setup.fog;
        self.selected = None;
        self.group.clear();
        self.pending = None;
        self.page = Page::Main;
        self.center_pending = true;
        self.briefing = false;
        // As Custom missions' Start: command 0x101 (0x5373ab), gamesys
        // 0x101's fades (from the session screen).
        self.entering = self.menu.is_some();
        self.menu = None;
        self.in_game = true;
        self.videos.clear();
        self.say(format!(
            "Multiplayer: you are player {} of {}",
            net.seat + 1,
            setup.seats.len()
        ));
        self.net = Some(net);
        self.turn_time = 0.0;
        self.refresh_panels();
        self.update_title();
        Ok(())
    }

    /// Plays the multiplayer turns due: each turn every player's orders,
    /// then `net::TURN` seconds of game in fixed steps, the host's player
    /// active so every machine computes the same.
    fn run_turns(&mut self, dt: f32) {
        let Some(net) = self.net.as_mut() else { return };
        net.poll();
        let chat = std::mem::take(&mut net.chat);
        for notice in std::mem::take(&mut net.notices) {
            self.say(notice);
        }
        for (seat, text) in chat {
            self.say(format!("Player {}: {text}", seat + 1));
        }
        // Behind by more than a second: catch up, but not all at once.
        self.turn_time = (self.turn_time + dt).min(1.0);
        while self.turn_time >= crate::net::TURN {
            let net = self.net.as_mut().unwrap();
            let (turn, local) = (net.turn, net.seat);
            let Some(orders) = net.next_turn() else { break };
            self.turn_time -= crate::net::TURN;
            let steps = (crate::net::TURN / crate::net::STEP).round() as usize;
            let (check, refused) = self.scene.play_turn(
                &orders,
                local,
                steps,
                crate::net::STEP,
                &self.data,
                &mut self.caches,
            );
            self.net.as_mut().unwrap().played(turn, check);
            for e in refused {
                self.say(e);
            }
        }
    }

    fn open_mission(&mut self, index: usize) {
        self.open_mission_for(index, None);
    }

    /// Opens mission `index`; a Custom start passes its players' sides
    /// (`Scene::load_custom`).
    fn open_mission_for(&mut self, index: usize, sides: Option<&[(u32, u32)]>) {
        if self.net.is_some() {
            self.say("Not in a multiplayer game");
            return;
        }
        let loaded = match sides {
            Some(sides) => Scene::load_custom(
                &self.data,
                &mut self.caches,
                &self.missions[index],
                self.difficulty,
                sides,
            ),
            None => Scene::load_carried(
                &self.data,
                &mut self.caches,
                &self.missions[index],
                self.difficulty,
                self.carry.as_ref(),
            ),
        };
        match loaded {
            Ok(scene) => {
                self.scene = scene;
                // Every game build shows the bar (0x4ebf50: [game+0x328d] = 0).
                self.bar_hidden = false;
                self.scene.fog = self.fog;
                if let Some(carry) = &self.carry {
                    self.scene.resume(carry);
                }
                self.mission_index = index;
                // A mission file has no ID_GAME: a new play-through id
                // (0x4ed770 -> 0x4ed3f0).
                self.game_id = crate::records::game_id();
                self.note_campaign();
                self.selected = None;
                self.group.clear();
                self.pending = None;
                self.page = Page::Main;
                self.tab = Tab::Object;
                self.center_pending = true;
                self.briefing = false;
                self.outcome_at = None;
                self.refresh_panels();
                self.update_title();
                self.menu = Some(MenuState::Briefing);
            }
            Err(e) => self.say(format!("Could not open the mission: {e:#}")),
        }
    }

    /// The campaign type the end screen and Best Conquerors show (game +
    /// 0x3214 through 0x5291b0; Best Conquerors takes 1 for none).
    fn score_kind(&self, best: bool) -> Option<usize> {
        let mask = if best && self.campaign_mask == 0 {
            1
        } else {
            self.campaign_mask
        };
        crate::records::lowest_bit(mask)
    }

    /// Opens the end screen (Startrpt.cpp 0x539350): after a won campaign
    /// mission (game + 0xd80 = 1, + 0x32fd set) its total score goes into
    /// the campaign's table (0x5397e0) under the player's name, and an
    /// edit to type the name over the row it took.
    fn open_outcome(&mut self, outcome: crate::scene::Outcome) {
        self.menu = Some(MenuState::Outcome(outcome));
        self.score_entry = None;
        if outcome != crate::scene::Outcome::Victory || self.campaign_number().is_none() {
            return;
        }
        let Some(kind) = self.score_kind(false) else {
            return;
        };
        // Game + 0x3335, the twelfth figure, a dword.
        let score = self.scene.stats.values(self.scene.amber)[11] as u32 as i32;
        // The name: the session's (resources + 0x1b9) when made and not
        // empty, else the stored one (Config's 0x5ca578).
        let name = if self.session_made && !self.player_name.is_empty() {
            self.player_name.clone()
        } else {
            self.stored_name.clone()
        };
        if let Some(row) = self.best.enter(kind, score, self.game_id, &name) {
            self.score_entry = Some(ScoreEntry {
                kind,
                row,
                text: name,
                edit: NameEdit::new(),
            });
        }
    }

    /// The end screen's edit's text into its row (0x539b20: messages
    /// 0x175 and 0x16c).
    fn store_score_name(&mut self) {
        if let Some(e) = &self.score_entry {
            self.best.rename(e.kind, e.row, &e.text);
        }
    }

    /// Button `caption` of screen `state` acts.
    fn menu_fire(&mut self, state: MenuState, caption: &str) {
        let was = self.menu;
        self.menu_action(state, caption);
        if self.menu != was {
            self.list_first = 0;
            self.desc_first = 0;
            self.map_at = None;
        }
    }

    /// The bindings screen `state` makes, in the order it makes them: its
    /// buttons' accelerators (their create records' key bytes, 0x4ea2d0;
    /// Alt + a letter, Esc for Cancel), the keys it binds itself (0x4e9840)
    /// and its name edit's (priority 0xff, repeating and exclusive).
    fn screen_keys(&self, state: MenuState) -> Vec<Binding> {
        use crate::sinput::{ENTER, ESCAPE, EXCLUSIVE, LALT, SPACE};
        let button = |caption: &'static str, keys: (u8, u8)| {
            (0, keys, 0, KeyMsg::Down(caption), Some(KeyMsg::Up(caption)))
        };
        let command =
            |caption: &'static str, key: u8| (0, (key, 0), 0, KeyMsg::Command(caption), None);
        // The edit's eleven, in the order 0x54f040 makes them.
        let edit = || {
            EDIT_KEYS
                .iter()
                .map(|&(key, msg)| (EXCLUSIVE | 1, (key, 0), 0xff, KeyMsg::Edit(msg), None))
                .collect::<Vec<Binding>>()
        };
        match state {
            // Startmm.cpp 0x530cb0: no Esc, Enter or Space.
            MenuState::Main => vec![
                button("Best Conquerors", (LALT, 0x30)),
                button("Start scenario", (LALT, 0x1f)),
                button("Load game", (LALT, 0x26)),
                button("Custom missions", (LALT, 0x2e)),
                button("MultiPlayer game", (LALT, 0x32)),
                button("Quit game", (LALT, 0x10)),
            ],
            // Startspl.cpp 0x53b940, then the map tick's Alt+A (made by
            // Startcom.cpp 0x52c5c0 after the screen's own part).
            MenuState::Missions(..) => vec![
                button("Start", (LALT, 0x1f)),
                button("Cancel", (ESCAPE, 0)),
                (0, (LALT, 0x1e), 0, KeyMsg::TickDown, Some(KeyMsg::TickUp)),
            ],
            // Startld.cpp 0x52fe90, then the map tick's.
            MenuState::Load(..) => vec![
                button("Load", (LALT, 0x26)),
                button("Cancel", (ESCAPE, 0)),
                (0, (LALT, 0x1e), 0, KeyMsg::TickDown, Some(KeyMsg::TickUp)),
            ],
            // Starttsk.cpp: Enter and Space start (0x53f360), then Start
            // and, for a campaign's first mission, Cancel (0x53f550).
            MenuState::Briefing => {
                let mut keys = vec![command("Start", ENTER), command("Start", SPACE)];
                keys.push(button("Start", (LALT, 0x1f)));
                if !self.campaign_number().is_some_and(|n| n != 0) {
                    keys.push(button("Cancel", (ESCAPE, 0)));
                }
                keys
            }
            // Startrpt.cpp 0x539350: the edit (made first), OK, Enter.
            MenuState::Outcome(_) => {
                let mut keys = Vec::new();
                if self.score_entry.as_ref().is_some_and(|e| e.edit.active) {
                    keys.extend(edit());
                }
                keys.push(button("OK", (LALT, 0x18)));
                keys.push(command("OK", ENTER));
                keys
            }
            // Startba.cpp 0x528390.
            MenuState::Best => vec![
                button("OK", (LALT, 0x18)),
                command("OK", ESCAPE),
                command("OK", ENTER),
                command("OK", SPACE),
            ],
            // Startprv.cpp 0x538440: the buttons, then the name's edit.
            MenuState::Connect => {
                let mut keys = vec![
                    button("New", (LALT, 0x31)),
                    button("Join", (LALT, 0x24)),
                    button("Cancel", (ESCAPE, 0)),
                ];
                if self.connect_edit.active {
                    keys.extend(edit());
                }
                keys
            }
            // The port's own screens.
            MenuState::Multiplayer(_) | MenuState::Waiting => Vec::new(),
        }
    }

    /// Which scan codes are down (the poll's keyboard state).
    fn scan_state(&self) -> [bool; 256] {
        let mut down = [false; 256];
        for &k in &self.keys {
            if let Some(c) = crate::sinput::scan_code(k) {
                down[usize::from(c)] = true;
            }
        }
        down
    }

    /// The front end's keyboard poll (Sinput.cpp 0x553090), once a frame:
    /// a screen opened binds its keys first (with the keys down then
    /// waiting to come up), then each binding's message as its keys go.
    fn poll_menu_keys(&mut self) {
        let Some(state) = self
            .menu
            .filter(|_| self.menus.is_some() && self.video.is_none())
        else {
            self.keys_screen = None;
            return;
        };
        let down = self.scan_state();
        let held = |k: u8| down[usize::from(k)];
        let screen = std::mem::discriminant(&state);
        if self.keys_screen != Some(screen) {
            self.keys_screen = Some(screen);
            self.menu_keys.clear();
            for (flags, keys, priority, press, release) in self.screen_keys(state) {
                self.menu_keys
                    .add(flags, keys, priority, Some(press), release, &held);
            }
        }
        let repeats = std::mem::take(&mut self.key_repeats);
        let repeated = |k: u8| repeats.contains(&k);
        self.menu_keys.begin();
        let mut at = 0;
        while let Some(msg) = self.menu_keys.next(&held, &repeated, &mut at) {
            self.key_message(state, msg);
            // A message that changed the bindings ends the poll (+ 0x60).
            if self.menu_keys.changed() || self.menu != Some(state) {
                break;
            }
        }
    }

    /// What a front-end binding's message does on screen `state`.
    fn key_message(&mut self, state: MenuState, msg: KeyMsg) {
        let index = |caption: &str| {
            self.menu_page(state)
                .buttons
                .iter()
                .position(|b| b.caption == caption)
        };
        match msg {
            // Pressed look (state 2) unless already held.
            KeyMsg::Down(caption) => {
                if self.menu_held.is_none() {
                    self.menu_held = index(caption);
                }
            }
            // Acts if still held (the pointer leaving lets it go).
            KeyMsg::Up(caption) => {
                if self.menu_held.is_some() && self.menu_held == index(caption) {
                    self.menu_held = None;
                    self.menu_fire(state, caption);
                }
            }
            KeyMsg::Command(caption) => self.menu_fire(state, caption),
            KeyMsg::TickDown => self.tick_held = true,
            KeyMsg::TickUp => {
                if std::mem::take(&mut self.tick_held) {
                    self.toggle_map_tick();
                }
            }
            KeyMsg::Edit(msg) => self.edit_key(state, msg),
            KeyMsg::PopupClose => self.close_kind_popup(),
            KeyMsg::PopupChoose => {
                if let Some(i) = self.kind_popup.as_ref().map(|p| p.current) {
                    self.choose_kind(i);
                }
            }
            // The list's buttons step its current entry at once, then
            // after 500 ms and every 50 while held (0x52ebe0: + 0x1a2,
            // + 0x1a6 of their records).
            KeyMsg::PopupStep(d) => {
                self.step_kind_popup(d);
                if let Some(p) = self.kind_popup.as_mut() {
                    p.step = Some((d, self.time_ms + 500));
                }
            }
            KeyMsg::PopupStepEnd => {
                if let Some(p) = self.kind_popup.as_mut() {
                    p.step = None;
                }
            }
        }
    }

    /// Custom missions' players rows shown (screen + 0x57c: the players, at
    /// most 11), for the mission shown.
    fn custom_rows(&self) -> &[CustomPlayer] {
        let shown = self.preview.as_ref().map(|p| p.index);
        if shown.is_none() || self.custom_for != shown {
            return &[];
        }
        &self.custom_players[..self.custom_players.len().min(PLAYER_ROWS)]
    }

    /// Custom missions' players list for mission `index` (0x52a990, type
    /// 6): every player but the barbarians; the human (kind 2, the
    /// session's name) is the one whose id `nation * 4 + slot` is
    /// `HEAD_CUST`'s c when its a is set, else the first; each other one
    /// at its `DESCRIPTOR` level (0x528a40: 1 AI Medium, 2 AI Strong, else
    /// AI Weak).
    fn make_custom_players(&mut self, index: usize) {
        let archive = self.data.open_mission(&self.missions[index]).ok();
        let head = archive
            .as_ref()
            .map_or([1, 1, 0], openconquest_formats::head_cust);
        let players = archive
            .as_ref()
            .and_then(|a| openconquest_formats::players(a).ok())
            .unwrap_or_default();
        let mut list: Vec<CustomPlayer> = Vec::new();
        for p in players.into_iter().filter(|p| p.nation != 4) {
            let human = if head[0] != 0 {
                head[2] == p.nation * 4 + p.slot
            } else {
                list.is_empty()
            };
            let (kind, name) = if human {
                (2, self.player_name.clone())
            } else {
                (
                    match p.level {
                        1 => 4,
                        2 => 5,
                        _ => 3,
                    },
                    String::new(),
                )
            };
            list.push(CustomPlayer {
                info: p,
                kind,
                name,
            });
        }
        self.custom_players = list;
        self.custom_head = head;
        self.custom_for = Some(index);
    }

    /// The kind field or arrow of players row `k` acts (0x53b940, ids
    /// 0x197 + k and 0x1af + k): unless a pop-up is open, the row's
    /// player's pop-up opens at (196 + 19, 216 + 42 + 27k), on the last
    /// row its picture's height higher (0x52ebe0). With `HEAD_CUST` a set
    /// and b clear it offers the computer's levels only (`T_PLAYER3`) and
    /// does not open for the human; else Disable, Human and the levels
    /// (`T_PLAYER5`). Its list starts at its first entry. The screen's
    /// key bindings are suspended (Sinput 0x18) and its pointer receivers
    /// turned off (0x1a); its own Esc (exclusive, 0x1d5), the list's
    /// Enter and its buttons' Up and Down are bound at the screen's
    /// priority, next to the suspended ones, and so start suspended
    /// themselves (see `sinput`). (Not decoded: how the list and its
    /// buttons get their activation, message 8; they are taken as active
    /// from here.)
    fn open_kind_popup(&mut self, k: usize) {
        if self.kind_popup.is_some() || k >= self.custom_rows().len() {
            return;
        }
        // The list's top is row 0: its 11 rows hold every player.
        let player = k;
        let [a, b, _] = self.custom_head;
        if a != 0 && b == 0 && self.custom_players[player].kind == 2 {
            return;
        }
        let entries = if a == 0 || b != 0 {
            vec![0, 2, 3, 4, 5]
        } else {
            vec![3, 4, 5]
        };
        let mut pop = KindPopup {
            player,
            x: 196.0 + 19.0,
            y: 216.0 + 42.0 + 27.0 * k as f32,
            entries,
            current: 0,
            pressed: None,
            dragged: false,
            step: None,
        };
        if k == PLAYER_ROWS - 1 {
            pop.y -= pop.picture().2;
        }
        let down = self.scan_state();
        let held = |k: u8| down[usize::from(k)];
        use crate::sinput::{ENTER, ESCAPE, EXCLUSIVE};
        self.menu_keys.suspend();
        let keys = &mut self.menu_keys;
        keys.add(
            EXCLUSIVE,
            (ESCAPE, 0),
            0,
            Some(KeyMsg::PopupClose),
            None,
            &held,
        );
        keys.add(0, (ENTER, 0), 0, Some(KeyMsg::PopupChoose), None, &held);
        for (key, d) in [(0xc8, -1), (0xd0, 1)] {
            keys.add(
                0,
                (key, 0),
                0,
                Some(KeyMsg::PopupStep(d)),
                Some(KeyMsg::PopupStepEnd),
                &held,
            );
        }
        self.kind_popup = Some(pop);
    }

    /// The pop-up closes (0x52f100): its list and bindings go, the Esc
    /// last, and the screen's bindings and receivers are back (0x1b,
    /// 0x19). Nothing else changes.
    fn close_kind_popup(&mut self) {
        if self.kind_popup.take().is_none() {
            return;
        }
        let down = self.scan_state();
        let held = |k: u8| down[usize::from(k)];
        use crate::sinput::{ENTER, ESCAPE};
        let keys = &mut self.menu_keys;
        keys.remove((ENTER, 0), &KeyMsg::PopupChoose, &held);
        keys.remove((0xc8, 0), &KeyMsg::PopupStep(-1), &held);
        keys.remove((0xd0, 0), &KeyMsg::PopupStep(1), &held);
        keys.remove((ESCAPE, 0), &KeyMsg::PopupClose, &held);
        keys.resume();
    }

    /// The pop-up's list moves its current entry a step, up to its ends
    /// (_slist messages 0x2f and 0x30).
    fn step_kind_popup(&mut self, d: i32) {
        if let Some(p) = self.kind_popup.as_mut() {
            p.current = (p.current as i32 + d).clamp(0, p.entries.len() as i32 - 1) as usize;
        }
    }

    /// Up or Down held in the pop-up repeats every 50 ms.
    fn kind_popup_repeat(&mut self) {
        let Some((d, due)) = self.kind_popup.as_ref().and_then(|p| p.step) else {
            return;
        };
        if self.time_ms < due {
            return;
        }
        self.step_kind_popup(d);
        if let Some(p) = self.kind_popup.as_mut() {
            p.step = Some((d, self.time_ms + 50));
        }
    }

    /// Entry `j` of the pop-up chosen (0x53b940 message 0x1cc): when it is
    /// not the player's kind, Disable leaves the player empty; Human makes
    /// the human player so far AI Weak and this one the human with the
    /// session's name; a computer level sets it. When the player was
    /// disabled, or becomes disabled or human, the map's player marks are
    /// made again (0x528de0, 0x52f470) and the viewer goes to the human's
    /// starting view (0x52a250; none without a human). Then it closes.
    fn choose_kind(&mut self, j: usize) {
        let Some(pop) = self.kind_popup.clone() else {
            return;
        };
        let (Some(&v), Some(old)) = (
            pop.entries.get(j),
            self.custom_players.get(pop.player).map(|c| c.kind),
        ) else {
            self.close_kind_popup();
            return;
        };
        if v != old {
            let name = if v == 2 {
                if let Some(h) = self.custom_players.iter_mut().find(|c| c.kind == 2) {
                    h.kind = 3;
                    h.name.clear();
                }
                self.player_name.clone()
            } else {
                String::new()
            };
            let cp = &mut self.custom_players[pop.player];
            cp.kind = v;
            cp.name = name;
            if old == 0 || v == 0 || v == 2 {
                self.make_preview_map();
                self.recentre_custom_map();
            }
        }
        self.close_kind_popup();
    }

    /// The map viewer to the human player's starting view (0x52a250), when
    /// a player is human; the preview clamps it to the map.
    fn recentre_custom_map(&mut self) {
        let Some(human) = self.custom_players.iter().find(|c| c.kind == 2) else {
            return;
        };
        let (nation, slot) = (human.info.nation, human.info.slot);
        let Some(index) = self.custom_for else {
            return;
        };
        let view = self
            .data
            .open_mission(&self.missions[index])
            .ok()
            .and_then(|a| openconquest_formats::mission::start_view(&a, nation, slot));
        if let Some((x, y)) = view {
            self.map_at = Some((x as usize, y as usize));
        }
    }

    /// Custom missions' Start (0x53b940, 0x178): the players set up and
    /// the game started at once (0x101), with no briefing: the Human row
    /// the human player (side + 0x1a1d = 1), each computer row a computer
    /// player at its level (side 3, + 0x1a31 through 0x528a20), each
    /// disabled row none (side 0): its objects are not made (0x51e810).
    /// With no Human row (0x528a70 < 0) there is no viewing player
    /// (`[game+0x3212]` 0xff, 0x53bfe6..0x53bff7): a game of computers
    /// only, nothing fogged, the view at the map's top-left.
    fn start_custom(&mut self, index: usize) {
        if self.custom_for != Some(index) {
            self.make_custom_players(index);
        }
        let players = self.custom_players.clone();
        self.carry = None;
        let sides: Vec<(u32, u32)> = players
            .iter()
            .filter(|p| p.kind != 0)
            .map(|p| (p.info.nation, p.info.slot))
            .collect();
        self.open_mission_for(index, Some(&sides));
        if self.menu != Some(MenuState::Briefing) {
            return;
        }
        match players.iter().find(|c| c.kind == 2) {
            Some(h) => self.scene.play_as(h.info),
            None => self.scene.spectate(),
        }
        let c = &mut self.scene.computers;
        c.levels = players
            .iter()
            .filter(|p| (3..=5).contains(&p.kind))
            .map(|p| ((p.info.nation, p.info.slot), usize::from(p.kind - 3)))
            .collect();
        c.off = players
            .iter()
            .filter(|p| p.kind == 0)
            .map(|p| (p.info.nation, p.info.slot))
            .collect();
        self.center_pending = true;
        self.refresh_panels();
        self.menu = None;
        self.in_game = true;
        self.entering = true;
    }

    /// Runs `op` on screen `state`'s name edit with its text, the band of
    /// a run of characters (their widths and a pixel's gap each, 0x547280
    /// and 0x547110), its length limit and its box's width. False when the
    /// screen has none.
    fn with_edit(
        &mut self,
        state: MenuState,
        op: impl FnOnce(&mut String, &mut NameEdit, &dyn Fn(&str) -> f32, usize, f32),
    ) -> bool {
        let Some(menus) = self.menus.as_ref() else {
            return false;
        };
        let band = |t: &str| {
            if t.is_empty() {
                0.0
            } else {
                menus.measure(EDIT_STYLE, t) + 1.0
            }
        };
        let (text, edit, max, width) = match state {
            MenuState::Outcome(_) => match self.score_entry.as_mut() {
                Some(e) => (&mut e.text, &mut e.edit, SCORE_NAME_MAX, SCORE_EDIT_WIDTH),
                None => return false,
            },
            MenuState::Connect => (
                &mut self.player_name,
                &mut self.connect_edit,
                PLAYER_NAME_MAX,
                CONNECT_EDIT.2,
            ),
            _ => return false,
        };
        op(text, edit, &band, max, width);
        true
    }

    /// Whether screen `state`'s name edit is active.
    fn edit_active(&self, state: MenuState) -> bool {
        match state {
            MenuState::Outcome(_) => self.score_entry.as_ref().is_some_and(|e| e.edit.active),
            MenuState::Connect => self.connect_edit.active,
            _ => false,
        }
    }

    /// A key of the active name edit (0x54f040, a single-line edit that is
    /// not read-only): the caret moves left, right, home and end, up to
    /// the text's ends (Up has no line above; Down stays on the line);
    /// Backspace takes the character before the caret, Delete the one at
    /// it (0x54ea60); Insert toggles a flag nothing here reads (flag 2,
    /// typing always inserts); Enter posts 0x175; Esc leaves the edit.
    fn edit_key(&mut self, state: MenuState, msg: u8) {
        match msg {
            0x35 => self.leave_edit(state),
            // Single line (flag 1): the Enter message, 0x175. The end
            // screen puts the text into its row (0x539b20); the connect
            // screen posts 0x170, New's action (0x538af0; the port's one
            // channel is always the one chosen).
            0x39 => match state {
                MenuState::Outcome(_) => self.store_score_name(),
                MenuState::Connect => self.menu_fire(state, "New"),
                _ => {}
            },
            0x38 => {}
            _ => {
                self.with_edit(state, |text, e, band, _, width| {
                    let n = text.chars().count();
                    let c = e.at();
                    match msg {
                        0x36 | 0x37 => {
                            let Some(c) = (if msg == 0x36 {
                                c.checked_sub(1)
                            } else {
                                Some(c)
                            }) else {
                                return;
                            };
                            if c < n {
                                let from = columns(text, 0, c).len();
                                let len = columns(text, c, c + 1).len();
                                text.replace_range(from..from + len, "");
                            }
                            e.move_to(text, c, band, width);
                        }
                        0x3a => {}
                        0x3b => e.move_to(text, c, band, width),
                        0x3c => e.move_to(text, c + 1, band, width),
                        0x3d => {
                            if let Some(c) = c.checked_sub(1) {
                                e.move_to(text, c, band, width);
                            }
                        }
                        0x3e => e.move_to(text, 0, band, width),
                        0x3f => e.move_to(text, n, band, width),
                        _ => {}
                    }
                });
            }
        }
    }

    /// Esc in a name edit (0x35): it is left (state 0, the caret kept)
    /// and its bindings go, giving back those they shadowed.
    fn leave_edit(&mut self, state: MenuState) {
        if !self.with_edit(state, |_, e, _, _, _| e.active = false) {
            return;
        }
        let down = self.scan_state();
        let held = |k: u8| down[usize::from(k)];
        for &(key, msg) in &EDIT_KEYS {
            self.menu_keys.remove((key, 0), &KeyMsg::Edit(msg), &held);
        }
    }

    /// A left press at `p` on screen `state` reaches its name edit
    /// (0x54f040 message 0x61): strictly inside its box it becomes active
    /// (its keys bound again) and the caret goes where the click is
    /// (0x54e430); outside, an active edit is left.
    fn edit_press(&mut self, state: MenuState, p: (f32, f32)) {
        let rect = match state {
            MenuState::Connect => CONNECT_EDIT,
            MenuState::Outcome(_) => match (&self.score_entry, &self.menus) {
                (Some(e), Some(menus)) => {
                    let h = menus.tallest(EDIT_STYLE);
                    (
                        410.0,
                        344.0 + (h + 6.0) * e.row as f32,
                        SCORE_EDIT_WIDTH,
                        h + 1.0,
                    )
                }
                _ => return,
            },
            _ => return,
        };
        let (x, y, w, h) = rect;
        let inside = p.0 > x && p.0 < x + w && p.1 > y && p.1 < y + h;
        let was = self.edit_active(state);
        if !inside {
            if was {
                self.leave_edit(state);
            }
            return;
        }
        if !was {
            self.with_edit(state, |_, e, _, _, _| e.active = true);
            let down = self.scan_state();
            let held = |k: u8| down[usize::from(k)];
            use crate::sinput::EXCLUSIVE;
            for &(key, msg) in &EDIT_KEYS {
                self.menu_keys.add(
                    EXCLUSIVE | 1,
                    (key, 0),
                    0xff,
                    Some(KeyMsg::Edit(msg)),
                    None,
                    &held,
                );
            }
        }
        self.with_edit(state, |text, e, band, _, _| e.click(text, p.0 - x, band));
    }

    /// Characters typed (WM_CHAR, _sedit.cpp case 0x34): into the active
    /// name edit at the caret, any but Backspace, Enter and Esc, while the
    /// text is shorter than its limit (0x54ecd0; the original's bytes,
    /// here characters); not with Alt held (Windows sends those as
    /// WM_SYSCHAR). The system repeats them as it repeats the key.
    fn edit_typed(&mut self, code: KeyCode, typed: &str) -> bool {
        let Some(state) = self.menu else { return false };
        if !self.edit_active(state) {
            return false;
        }
        if self.modifiers.alt_key()
            || matches!(
                code,
                KeyCode::Backspace | KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape
            )
        {
            return true;
        }
        for ch in typed.chars() {
            self.with_edit(state, |text, e, band, max, width| {
                let n = text.chars().count();
                if n >= max {
                    return;
                }
                let c = e.at().min(n);
                let from = columns(text, 0, c).len();
                text.insert(from, ch);
                e.move_to(text, e.at() + 1, band, width);
            });
        }
        true
    }

    fn menu_click(&mut self, pos: PhysicalPosition<f64>) {
        let p = Menus::to_menu(self.window_size(), pos.x as f32, pos.y as f32);
        self.menu_press(p);
    }

    /// A press of the left button at 800x600 point `p` of a front-end
    /// screen.
    fn menu_press(&mut self, p: (f32, f32)) {
        // The kind pop-up open: a press outside its picture closes it
        // (StartMouseClassTy 0x52fbb0, 0x1d5); on its list an entry is
        // noted (_slist message 0x61); nothing else takes the pointer.
        if let Some(pop) = self.kind_popup.as_mut() {
            if pop.outside(p) {
                self.close_kind_popup();
            } else if let Some(i) = pop.entry_at(p) {
                pop.pressed = Some(i);
                pop.dragged = false;
            }
            return;
        }
        // A screen's name edit takes the press too.
        if let Some(state) = self.menu {
            self.edit_press(state, p);
        }
        let (Some(state), Some(menus)) = (self.menu, &self.menus) else {
            return;
        };
        let page = self.menu_page(state);
        let Some(hit) = menus.hit(&page.view(None, None), p) else {
            return;
        };
        match hit {
            // A button is held down; it acts when let go (`menu_release`).
            MenuHit::Button(i) => {
                self.menu_held = Some(i);
            }
            MenuHit::Tab(t) => {
                if let MenuState::Load(..) = state {
                    let tab = [LoadTab::Scenario, LoadTab::Custom, LoadTab::StartFrom][t.min(2)];
                    self.open_list(MenuState::Load(tab, None));
                    self.list_first = 0;
                    self.desc_first = 0;
                    self.map_at = None;
                }
            }
            // A scroll arrow moves its bar a position, and again while
            // held: after 500 ms, then every 50 (75 for a list's), as the
            // original's arrow buttons (Startcom.cpp, 0x52c5c0).
            MenuHit::Step(target, d) => {
                self.menu_step(state, &page, target, d);
                self.menu_repeat = Some((hit, self.time_ms + 500));
            }
            // A scroll bar's track takes the position under the pointer,
            // and follows it while the button is held (_strack.cpp).
            MenuHit::Track(target, pos) => {
                self.menu_scroll_to(state, &page, target, pos);
                self.menu_drag = Some(target);
            }
            // A difficulty: the mission is set up again for it.
            MenuHit::Check(i) => {
                if state == MenuState::Briefing && self.difficulty != i {
                    self.difficulty = i;
                    self.open_mission(self.mission_index);
                }
            }
            // A players row's kind field or arrow is held down; it acts
            // when let go inside (_sbutton).
            MenuHit::Kind(k, arrow) => self.kind_held = Some((k, arrow)),
            MenuHit::MapTick => self.tick_held = true,
            MenuHit::Item(i) => {
                self.menu_select(state, i);
            }
        }
    }

    /// The left button let go at 800x600 point `p` of a front-end screen:
    /// the button held down acts if the pointer is still inside it
    /// (_sbutton.cpp 0x54ab20, message 0x62).
    fn menu_release(&mut self, p: (f32, f32)) {
        // On the pop-up's list (_slist message 0x62): let go on the entry
        // it went down on, unless dragged, chooses it.
        if let Some(pop) = self.kind_popup.as_mut() {
            let pressed = pop.pressed.take();
            if let (Some(i), false) = (pressed, pop.dragged) {
                if pop.entry_at(p) == Some(i) {
                    pop.current = i;
                    self.choose_kind(i);
                }
            }
            return;
        }
        if let (Some(held), Some(state), Some(menus)) =
            (self.kind_held.take(), self.menu, &self.menus)
        {
            let page = self.menu_page(state);
            if menus.hit(&page.view(None, None), p) == Some(MenuHit::Kind(held.0, held.1)) {
                self.open_kind_popup(held.0);
            }
        }
        if std::mem::take(&mut self.tick_held) {
            if let (Some(state), Some(menus)) = (self.menu, &self.menus) {
                let page = self.menu_page(state);
                if menus.hit(&page.view(None, None), p) == Some(MenuHit::MapTick) {
                    self.toggle_map_tick();
                }
            }
        }
        let (Some(held), Some(state), Some(menus)) =
            (self.menu_held.take(), self.menu, &self.menus)
        else {
            return;
        };
        let page = self.menu_page(state);
        if menus.hit(&page.view(None, None), p) != Some(MenuHit::Button(held)) {
            return;
        }
        let Some(caption) = page.buttons.get(held).map(|b| b.caption.clone()) else {
            return;
        };
        self.menu_fire(state, &caption);
    }

    /// The pointer at 800x600 point `p` of a front-end screen: a button
    /// held down is let go once the pointer leaves it (_sbutton.cpp
    /// message 0x60), without acting.
    fn menu_pointer_at(&mut self, p: (f32, f32)) {
        // A drag on the pop-up's list (_slist message 0x60): from the
        // current entry onto the next or the one before, that one becomes
        // current, and the release chooses nothing.
        if let Some(pop) = self.kind_popup.as_mut() {
            if let (Some(i), Some(down)) = (pop.entry_at(p), pop.pressed) {
                if down == pop.current && i.abs_diff(pop.current) == 1 {
                    pop.dragged = true;
                    pop.pressed = Some(i);
                    pop.current = i;
                }
            }
            return;
        }
        if let (Some(held), Some(state), Some(menus)) = (self.kind_held, self.menu, &self.menus) {
            let page = self.menu_page(state);
            if menus.hit(&page.view(None, None), p) != Some(MenuHit::Kind(held.0, held.1)) {
                self.kind_held = None;
            }
        }
        if let (true, Some(state), Some(menus)) = (self.tick_held, self.menu, &self.menus) {
            let page = self.menu_page(state);
            if menus.hit(&page.view(None, None), p) != Some(MenuHit::MapTick) {
                self.tick_held = false;
            }
        }
        let (Some(held), Some(state), Some(menus)) = (self.menu_held, self.menu, &self.menus)
        else {
            return;
        };
        let page = self.menu_page(state);
        if menus.hit(&page.view(None, None), p) != Some(MenuHit::Button(held)) {
            self.menu_held = None;
        }
    }

    /// Entry `i` of a front-end list chosen: on the mission screens its
    /// mission's map and description shown from their start.
    fn menu_select(&mut self, state: MenuState, i: usize) {
        self.menu = Some(match state {
            MenuState::Missions(c, _) => {
                if let Some(&index) = self.screen_missions(c).get(i) {
                    self.show_preview(index, true);
                }
                MenuState::Missions(c, Some(i))
            }
            MenuState::Load(tab, _) => {
                let entry = self.load_entries(tab).get(i).map(|e| e.0);
                if let Some(index) = entry.and_then(|e| self.entry_mission(e)) {
                    let fresh = self.preview.as_ref().is_none_or(|p| p.index != index);
                    self.show_preview(index, fresh);
                }
                MenuState::Load(tab, Some(i))
            }
            MenuState::Multiplayer(_) => {
                // A map takes at most so many players.
                if let Some(&(_, n)) = self.battle_maps.get(i) {
                    self.lobby_players = self.lobby_players.min(n);
                }
                MenuState::Multiplayer(Some(i))
            }
            other => other,
        });
    }

    /// Opens list screen `state` with its first entry chosen, as the
    /// original's list does (`_slist.cpp` sets its cursor to 0 and sends
    /// 0x189, which shows that mission).
    fn open_list(&mut self, state: MenuState) {
        self.menu = Some(state);
        let any = match state {
            MenuState::Missions(c, _) => !self.screen_missions(c).is_empty(),
            MenuState::Load(tab, _) => !self.load_entries(tab).is_empty(),
            _ => false,
        };
        if any {
            self.menu_select(state, 0);
        }
    }

    /// Shows mission `index`'s map and description (loaded again if
    /// `load`), each scrolled back to its start.
    fn show_preview(&mut self, index: usize, load: bool) {
        self.desc_first = 0;
        self.map_at = None;
        if load {
            self.preview_due = Some((index, Instant::now() + Duration::from_millis(200)));
        }
    }

    /// Loads the mission the list chose once its 200 ms are up.
    fn load_due_preview(&mut self) {
        let Some((index, at)) = self.preview_due else {
            return;
        };
        if Instant::now() < at {
            return;
        }
        self.preview_due = None;
        match Preview::load(&self.data, &mut self.caches, &self.missions[index], index) {
            Ok(p) => self.preview = Some(p),
            Err(e) => self.say(format!("{e:#}")),
        }
        if let Some(MenuState::Missions(Category::Single, _)) = self.menu {
            self.make_custom_players(index);
        }
        self.make_preview_map();
    }

    /// The map picture made again for the screen shown (0x52f470): on
    /// Custom missions with its players list (0x528de0).
    fn make_preview_map(&mut self) {
        let players = match self.menu {
            Some(MenuState::Missions(..)) => Some((&self.custom_players[..], self.custom_head)),
            _ => None,
        };
        let tick = self.settings.config.map_tick;
        if let Some(p) = self.preview.as_mut() {
            p.make_map(&self.data, tick, players);
        }
    }

    /// The map tick (0x52c5c0: message 0x1dd, click or Alt+A; 0x52e450):
    /// `[0x5ca538]` flipped, the map picture made again (none when off,
    /// 0x52f470) and the viewer recentred (0x52a250; on the Load screen,
    /// which has no players list, not decoded: the viewer is left).
    fn toggle_map_tick(&mut self) {
        let c = &mut self.settings.config;
        c.map_tick = !c.map_tick;
        self.make_preview_map();
        if let Some(MenuState::Missions(..)) = self.menu {
            self.recentre_custom_map();
        }
    }

    /// A scroll arrow: its bar a position back (`d` < 0) or on, if it can
    /// go (_sslider.cpp 0x551ca0, messages 0x2f and 0x30). A list without
    /// a bar (screens of our own) moves its window a row.
    fn menu_step(&mut self, state: MenuState, page: &MenuPage, target: Scroller, d: i32) {
        match page.tracks.iter().find(|t| t.target == target) {
            Some(t) => {
                let pos = step_position(t.pos, t.count, d);
                if pos != t.pos {
                    self.menu_scroll_to(state, page, target, pos);
                }
            }
            None => {
                let last = page.items.len().saturating_sub(page.rows);
                self.list_first = (page.first as i64 + d as i64).clamp(0, last as i64) as usize;
            }
        }
    }

    /// A scroll bar set to position `pos`: a list's chosen entry, with its
    /// window following (_slist.cpp), the description's first line, the
    /// map's top left cell.
    fn menu_scroll_to(&mut self, state: MenuState, page: &MenuPage, target: Scroller, pos: usize) {
        let along = |s: Scroller| {
            page.tracks
                .iter()
                .find(|t| t.target == s)
                .map_or(0, |t| t.pos)
        };
        match target {
            Scroller::List => {
                if pos >= page.items.len() {
                    return;
                }
                if page.selected != Some(pos) {
                    self.menu_select(state, pos);
                }
                self.list_first = crate::menus::follow(page.first, pos, page.rows);
            }
            Scroller::Story => self.desc_first = pos,
            Scroller::MapX => self.map_at = Some((pos, along(Scroller::MapY))),
            Scroller::MapY => self.map_at = Some((along(Scroller::MapX), pos)),
        }
    }

    /// The scroll bar being dragged follows the pointer, at 800x600 point
    /// `p`.
    fn menu_drag_to(&mut self, p: (f32, f32)) {
        let (Some(state), Some(target)) = (self.menu, self.menu_drag) else {
            return;
        };
        let page = self.menu_page(state);
        if let Some(t) = page.tracks.iter().find(|t| t.target == target) {
            let pos = t.pos_at(p);
            if pos != t.pos && t.count > 0 {
                self.menu_scroll_to(state, &page, target, pos);
            }
        }
    }

    /// A held scroll arrow repeats while the pointer stays on it.
    fn menu_repeat_step(&mut self) {
        let (Some(state), Some((hit, due)), Some(c)) = (self.menu, self.menu_repeat, self.cursor)
        else {
            return;
        };
        if self.time_ms < due {
            return;
        }
        let MenuHit::Step(target, d) = hit else {
            return;
        };
        let every = if target == Scroller::List { 75 } else { 50 };
        self.menu_repeat = Some((hit, self.time_ms + every));
        let page = self.menu_page(state);
        let p = Menus::to_menu(self.window_size(), c.x as f32, c.y as f32);
        let here = self
            .menus
            .as_ref()
            .and_then(|m| m.hit(&page.view(None, None), p));
        if here == Some(hit) {
            self.menu_step(state, &page, target, d);
        }
    }

    fn menu_action(&mut self, state: MenuState, caption: &str) {
        // The main menu's Start scenario (0x168), MultiPlayer (0x16a),
        // Custom missions (0x1cd) and Load game (0x1cf) make the session's
        // name from the stored one, once (0x53d220).
        if state == MenuState::Main
            && matches!(
                caption,
                "Start scenario" | "MultiPlayer game" | "Custom missions" | "Load game"
            )
            && !self.session_made
        {
            self.session_made = true;
            self.player_name = self.stored_name.clone();
        }
        match (state, caption) {
            (_, "Load game") => {
                self.refresh_panels();
                self.open_list(MenuState::Load(LoadTab::Scenario, None));
            }
            (_, "Quit game") => self.quit_game(),
            // The campaign from its first mission, on its briefing.
            (_, "Start scenario") => {
                if let Some(&first) = self.category_missions(Category::Campaign).first() {
                    // The story opens before the first mission.
                    self.queue_video(crate::video::OPENING);
                    self.carry = None;
                    self.open_mission(first);
                }
            }
            (_, "Custom missions") => self.open_list(MenuState::Missions(Category::Single, None)),
            (_, "Best Conquerors") => self.menu = Some(MenuState::Best),
            (MenuState::Best, "OK") => self.menu = Some(MenuState::Main),
            // The name's edit starts active (0x538440, state 1).
            (_, "MultiPlayer game") => {
                self.connect_edit = NameEdit::new();
                self.menu = Some(MenuState::Connect);
            }
            (MenuState::Connect, "New") | (MenuState::Connect, "Join") => {
                self.find_battle_maps();
                self.menu = Some(MenuState::Multiplayer(None));
            }
            (MenuState::Connect, "Cancel") => self.menu = Some(MenuState::Main),
            (MenuState::Multiplayer(sel), "More players") => {
                let most = sel
                    .and_then(|i| self.battle_maps.get(i))
                    .map_or(8, |&(_, n)| n);
                self.lobby_players = (self.lobby_players + 1).min(most);
            }
            (MenuState::Multiplayer(_), "Fewer players") => {
                self.lobby_players = (self.lobby_players - 1).max(2);
            }
            (MenuState::Multiplayer(sel), "Host") => self.host_game(sel),
            (MenuState::Multiplayer(_), "Join") => self.join_game(),
            (MenuState::Waiting, "Cancel") => {
                self.hosting = None;
                self.joining = None;
                self.menu = Some(MenuState::Multiplayer(None));
            }
            (MenuState::Missions(c, Some(i)), "Start") => {
                if let Some(&index) = self.screen_missions(c).get(i) {
                    if c == Category::Single {
                        self.start_custom(index);
                    } else {
                        self.carry = None;
                        self.open_mission(index);
                    }
                }
            }
            (MenuState::Load(tab, Some(i)), "Load") => {
                let Some((entry, _)) = self.load_entries(tab).get(i).cloned() else {
                    return;
                };
                let i = match entry {
                    LoadEntry::Save(i) => i,
                    // A start report (0x530480, 0x53a670): its difficulty
                    // and record restored, its mission's briefing opened
                    // (0x537f60).
                    LoadEntry::Report(k) => {
                        let Some(index) = self.entry_mission(entry) else {
                            return;
                        };
                        let Some((difficulty, body)) =
                            self.reports.get(k).and_then(|r| read_report(&r.path))
                        else {
                            self.say("Not a start report");
                            return;
                        };
                        if self.category_missions(Category::Campaign).first() == Some(&index) {
                            self.queue_video(crate::video::OPENING);
                        }
                        self.difficulty = usize::from(difficulty);
                        self.carry = body.carry;
                        self.open_mission(index);
                        return;
                    }
                };
                let Some(path) = self.saves.get(i).map(|e| e.path.clone()) else {
                    return;
                };
                self.load_game(&path);
                if self
                    .status
                    .as_ref()
                    .is_some_and(|(m, _)| m == "Game loaded")
                {
                    self.menu = None;
                    self.in_game = true;
                }
            }
            (MenuState::Briefing, "Start") => {
                self.write_start_report();
                self.leave_briefing_sounds();
                self.menu = None;
                self.in_game = true;
                self.entering = true;
            }
            (MenuState::Briefing, "Cancel") => {
                self.leave_briefing_sounds();
                self.menu = Some(MenuState::Main);
            }
            (MenuState::Missions(..), "Cancel") | (MenuState::Load(..), "Cancel") => {
                self.menu = Some(MenuState::Main)
            }
            // The end screen's OK (0x539b20, 0x16c): after a won campaign
            // mission the next one's briefing, the campaign going on with
            // what was learned; after the last, the main menu; else the
            // main menu. With the options word's bit 2 ([0x5ca53c], 3 as
            // the installer writes it, setup.exe 0x404530) a video comes
            // first: command 0x128 case 5 `MIS%03d.AVI` (next + campaign x
            // 100) then the briefing, case 6 `OUTRO%d.AVI` (campaign) then
            // the main menu; a missing file goes straight on (0x4ecc05,
            // FindFirstFileA).
            (MenuState::Outcome(outcome), "OK") => {
                // The name typed goes into the row first.
                self.store_score_name();
                let won = outcome == crate::scene::Outcome::Victory;
                let campaign = won
                    .then(|| self.campaign_number())
                    .flatten()
                    .and_then(|_| self.missions[self.mission_index][4..5].parse::<u32>().ok());
                match campaign {
                    Some(c) => match self.next_mission() {
                        Some(next) => {
                            let n = self.missions[next][5..].parse::<u32>().unwrap_or(0);
                            self.queue_video(&crate::video::mission(n + c * 100));
                            self.carry = Some(self.scene.carry());
                            self.open_mission(next);
                        }
                        None => {
                            self.queue_video(&crate::video::outro(c));
                            self.menu = Some(MenuState::Main);
                        }
                    },
                    None => self.menu = Some(MenuState::Main),
                }
            }
            (_, "Main menu") | (_, "Back") => self.menu = Some(MenuState::Main),
            _ => {}
        }
    }

    /// Renders a front-end screen (`main`, `missions`, `load`, `briefing`,
    /// `victory`, `defeat`) into `frame`, for screenshots.
    pub fn draw_menu_screen(&mut self, which: &str, frame: &mut Frame) -> Result<()> {
        // `screen+x,y+...`: after presses of the left button at those
        // 800x600 points.
        let mut parts = which.split('+');
        let which = parts.next().unwrap_or_default();
        let presses: Vec<(f32, f32)> = parts
            .filter_map(|at| {
                let (x, y) = at.split_once(',')?;
                Some((x.parse().ok()?, y.parse().ok()?))
            })
            .collect();
        // `screen@x,y`: with the mouse at 800x600 point (x, y).
        let (which, hover) = match which.split_once('@') {
            Some((w, at)) => (
                w,
                at.split_once(',')
                    .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))),
            ),
            None => (which, None),
        };
        let state = match which {
            "main" => MenuState::Main,
            "custom" => MenuState::Missions(Category::Single, Some(0)),
            "load" => {
                self.refresh_panels();
                MenuState::Load(LoadTab::Scenario, Some(0))
            }
            "briefing" => MenuState::Briefing,
            "victory" => MenuState::Outcome(crate::scene::Outcome::Victory),
            "defeat" => MenuState::Outcome(crate::scene::Outcome::Defeat),
            "best" => MenuState::Best,
            "connect" => {
                self.player_name.clear();
                self.connect_edit = NameEdit::new();
                MenuState::Connect
            }
            "multiplayer" => {
                self.find_battle_maps();
                self.join_address = "192.168.1.20".into();
                MenuState::Multiplayer(Some(4))
            }
            other => anyhow::bail!("unknown menu screen {other}"),
        };
        self.menu = Some(state);
        if let MenuState::Missions(_, Some(i)) | MenuState::Load(_, Some(i)) = state {
            self.menu_select(state, i);
            if let Some((index, _)) = self.preview_due {
                self.preview_due = Some((index, Instant::now()));
                self.load_due_preview();
            }
        }
        for p in presses {
            self.menu_press(p);
            self.menu_release(p);
            self.menu_drag = None;
            self.menu_repeat = None;
        }
        let state = self.menu.unwrap_or(state);
        let menus = self.menus.as_ref().context("menus are not installed")?;
        menus.draw(frame, &self.menu_page(state).view(hover, self.menu_held));
        Ok(())
    }

    /// The battle maps and how many players each takes.
    fn find_battle_maps(&mut self) {
        if !self.battle_maps.is_empty() {
            return;
        }
        self.battle_maps = self
            .category_missions(Category::Battle)
            .into_iter()
            .filter_map(|i| {
                let archive = self.data.open_mission(&self.missions[i]).ok()?;
                let players = openconquest_formats::players(&archive).ok()?;
                let n = players
                    .iter()
                    .filter(|p| !p.computer || p.nation != 4)
                    .count();
                (n >= 2).then_some((i, n))
            })
            .collect();
    }

    /// Hosts a game of the chosen battle map.
    fn host_game(&mut self, selected: Option<usize>) {
        let Some(&(i, _)) = selected.and_then(|k| self.battle_maps.get(k)) else {
            self.say("Choose a map to host");
            return;
        };
        let mission = self.missions[i].clone();
        let seats = self
            .data
            .open_mission(&mission)
            .ok()
            .and_then(|a| openconquest_formats::players(&a).ok())
            .and_then(|p| crate::net::seats_for(&p, self.lobby_players));
        let Some(seats) = seats else {
            self.say("The map has no room for so many players");
            return;
        };
        let setup = crate::net::Setup {
            mission,
            seats,
            difficulty: self.difficulty,
            fog: self.fog,
        };
        match crate::net::Lobby::open(crate::net::PORT, setup) {
            Ok(lobby) => {
                self.hosting = Some(lobby);
                self.menu = Some(MenuState::Waiting);
            }
            Err(e) => self.say(format!("{e:#}")),
        }
    }

    /// Joins the game at the address typed.
    fn join_game(&mut self) {
        let address = self.join_address.trim().to_string();
        if address.is_empty() {
            self.say("Type the host's address first");
            return;
        }
        match crate::net::Joining::connect(&address) {
            Ok(j) => {
                self.joining = Some(j);
                self.menu = Some(MenuState::Waiting);
            }
            Err(e) => self.say(format!("{e:#}")),
        }
    }

    /// While waiting in the lobby: starts the game once everyone is there.
    fn run_lobby(&mut self) {
        let polled = if let Some(l) = self.hosting.as_mut() {
            l.poll()
        } else if let Some(j) = self.joining.as_mut() {
            j.poll()
        } else {
            return;
        };
        match polled {
            Ok(None) => {}
            Ok(Some(net)) => {
                self.hosting = None;
                self.joining = None;
                if let Err(e) = self.start_multiplayer(net) {
                    self.say(format!("Could not start the game: {e:#}"));
                    self.menu = Some(MenuState::Multiplayer(None));
                }
            }
            Err(e) => {
                self.hosting = None;
                self.joining = None;
                self.say(format!("{e:#}"));
                self.menu = Some(MenuState::Multiplayer(None));
            }
        }
    }

    /// Ends the game under way and shows the title screen (Main menu,
    /// message 0x4fc).
    fn leave_game(&mut self) {
        self.in_game = false;
        // As every way out of the game (0x4ec1a0 0x100, 0x10b, 0x10c:
        // [game+0x328d] = 0).
        self.bar_hidden = false;
        self.net = None;
        self.hosting = None;
        self.joining = None;
        self.paused = false;
        self.selected = None;
        self.group.clear();
        self.pending = None;
        self.menu = Some(MenuState::Main);
    }

    /// Esc on the port's own multiplayer screens: back to the main menu.
    fn menu_back(&mut self) {
        self.list_first = 0;
        self.menu = Some(MenuState::Main);
    }

    /// Re-reads the save slots and the help text shown in the panels.
    fn refresh_panels(&mut self) {
        // The saved games, by name (as the original lists its SAVEGAME
        // folder).
        let mut saves: Vec<SaveEntry> = std::fs::read_dir(self.data.save_dir())
            .map(|dir| {
                dir.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "json"))
                    .filter_map(|path| {
                        let save = SaveGame::read(&path).ok()?;
                        let label = path.file_stem()?.to_string_lossy().into_owned();
                        Some(SaveEntry {
                            path,
                            mission: save.mission,
                            label,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        saves.sort_by_key(|e| e.label.to_lowercase());
        self.reports = self.read_reports();
        let chosen = self
            .selected_save
            .and_then(|i| self.saves.get(i))
            .map(|e| e.path.clone());
        self.selected_save = chosen.and_then(|p| saves.iter().position(|e| e.path == p));
        self.save_names = saves.iter().map(|e| e.label.clone()).collect();
        self.saves = saves;
        self.save_first = self
            .save_first
            .min(self.saves.len().saturating_sub(Panels::save_rows()));
        let help = panels::help_paragraphs(
            &self.scene.briefing_lines,
            crate::scene::is_custom(&self.scene.mission),
            &self.scene.conditions,
        );
        if help != self.help_text {
            self.help_first = 0;
        }
        self.help_text = help;
    }

    /// The selected object, if it is a ship.
    fn selected_ship(&self) -> Option<&crate::objects::MapObject> {
        let id = self.selected?;
        self.scene
            .objects
            .iter()
            .find(|o| o.id == id)
            .filter(|o| o.ship.is_some())
    }

    /// The building site under a window position while a building is being
    /// placed, and whether it is allowed.
    fn site_at(&self, pos: PhysicalPosition<f64>) -> Option<((i32, i32), bool)> {
        self.site_facing(pos)
            .map(|(cell, _, f)| (cell, f.is_some()))
    }

    /// The building site under `pos`: its cell, the class to put up, and
    /// its facing if it may go there.
    fn site_facing(&self, pos: PhysicalPosition<f64>) -> Option<((i32, i32), u32, Option<u32>)> {
        let cell = self.cell_at(pos)?;
        let class = self.pending.and_then(economy::building_of_command)?;
        let facing = if self.selected_ship().is_some() {
            self.scene.check_ground(class, cell.0, cell.1).ok()
        } else {
            let settlement = self.selected?;
            self.scene
                .check_site(settlement, class, cell.0, cell.1)
                .ok()
        };
        Some((cell, class, facing))
    }

    /// The map cell under a window position in the world view.
    fn cell_at(&self, pos: PhysicalPosition<f64>) -> Option<(i32, i32)> {
        let (wx, wy) = self.view_world(pos)?;
        let t = openconquest_formats::map::TILE_SIZE as f32;
        Some(((wx / t).floor() as i32, (wy / t).floor() as i32))
    }

    /// The object panel's state for the selection (fresh when it
    /// changed: the panel is made anew, ProcOnCreate 0x4f5230).
    fn info_state(&self) -> crate::info_panel::InfoView {
        let owner = (self.selected.unwrap_or(u32::MAX), self.group.len());
        if self.info_view.owner == owner {
            self.info_view
        } else {
            crate::info_panel::InfoView {
                owner,
                ..Default::default()
            }
        }
    }

    /// A press on the object panel (c_info.cpp DoInfoBut 0x4f72d0 and
    /// GetMessage 0x4f79e0): a settlement's place opens its group and the
    /// header closes it; anything else there shows big while the button
    /// is held (or, while selling, is sold); a group member's button or
    /// the portrait centres the view on its object; the arrows scroll.
    fn info_press(&mut self, pos: PhysicalPosition<f64>) -> bool {
        use crate::info_panel::{self as ip, Hit, Shown};
        use crate::scene::load::{ARMOUR, PRESENT, RAM, UNIT};
        if self.bar_hidden || Panels::covers_bar(self.tab) || self.reloading().is_some() {
            return false;
        }
        let layout = self.layout();
        let (x, y) = (
            (pos.x as f32 - layout.bar_x) / layout.scale,
            pos.y as f32 / layout.scale,
        );
        let mut view = self.info_state();
        let shown = ip::shown(&self.scene, &self.selection(), &view);
        let Some(hit) = ip::hit(&shown, &view, x, y) else {
            return false;
        };
        view.pressed = Some(hit);
        let centre = |app: &mut Self, id: u32| {
            if let Some(i) = app.scene.object_index(id) {
                let (ox, oy) = (app.scene.objects[i].x, app.scene.objects[i].y);
                let (screen, _) = app.placement();
                app.camera.center_on(ox, oy, &screen, app.scene.map_size());
            }
        };
        match (hit, &shown) {
            (Hit::Member(i), Shown::Group(ids)) => centre(self, ids[i]),
            (Hit::Portrait, Shown::Single(id, _)) => centre(self, *id),
            (Hit::Up | Hit::Down, _) => {
                let mut v = view;
                ip::step(&shown, &mut v, hit == Hit::Down);
                view.scroll = v.scroll;
            }
            (Hit::Cell(i), Shown::Single(site, list)) => {
                let e = list[i];
                let selling = self.pending == Some(Command::SELL_ITEM);
                if selling && e.flags == PRESENT {
                    // The pointer's sell mode (0x34) takes a stock unit,
                    // armour or ram (message 0x280f to the settlement).
                    if matches!(e.cat, UNIT | ARMOUR | RAM) {
                        let order = match e.cat {
                            UNIT => Order::SellItem {
                                site: *site,
                                item: e.sub,
                            },
                            // Armour kinds 0-2, rams 3-5.
                            ARMOUR => Order::SellFitting {
                                site: *site,
                                ram: false,
                                level: e.sub.min(2),
                            },
                            _ => Order::SellFitting {
                                site: *site,
                                ram: true,
                                level: e.sub.saturating_sub(3).min(2),
                            },
                        };
                        match self.issue(order) {
                            Ok(_) => {
                                self.pending = None;
                                self.status = None;
                            }
                            Err(err) => self.say(err),
                        }
                    }
                } else if e.is_group() {
                    view.open = if e.flags & crate::scene::load::HEADER != 0 {
                        None
                    } else {
                        Some((e.cat, e.sub))
                    };
                    view.scroll = 0;
                } else if ip::describable(&e) {
                    view.big = Some(i);
                }
            }
            _ => {}
        }
        self.info_view = view;
        true
    }

    /// Whether a window position is on the Building page's "default
    /// arming" checkbox.
    /// The main page of one of the player's shipyards or docks (which shows
    /// the "default arming" checkbox too).
    fn yard_selected(&self) -> bool {
        self.page == Page::Main
            && self
                .selected
                .and_then(|id| self.scene.object_index(id))
                .is_some_and(|i| {
                    let o = &self.scene.objects[i];
                    self.scene.is_own(o)
                        && matches!(
                            o.class,
                            openconquest_formats::ObjectClass::Building(
                                economy::SHIPYARD | economy::DOCK
                            )
                        )
                })
    }

    fn on_arming_box(&self, pos: PhysicalPosition<f64>) -> bool {
        if self.bar_hidden
            || Panels::covers_bar(self.tab)
            || !(matches!(self.page, Page::Structures | Page::Ground) || self.yard_selected())
            || self.selection().is_empty()
        {
            return false;
        }
        let layout = self.layout();
        let (x, y, w, h) = self.interface.arming_rect(&layout);
        let (px, py) = (pos.x as f32, pos.y as f32);
        px >= x && py >= y && px < x + w && py < y + h
    }

    /// The command slots whose key `code` is (0x4f2140): the buttons'
    /// accelerators live while the command panel shows them.
    fn hotkey_slots(&self, code: KeyCode) -> Vec<(usize, Command)> {
        if self.bar_hidden || Panels::covers_bar(self.tab) || self.reloading().is_some() {
            return Vec::new();
        }
        let Some(obj) = self
            .selected
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i])
        else {
            return Vec::new();
        };
        let shown = menu(&self.scene, obj, self.page);
        let m = self.modifiers;
        let held = |scan: u8| match scan {
            0x2a | 0x36 => m.shift_key(),
            0x1d | 0x9d => m.control_key() || m.super_key(),
            0x38 | 0xb8 => m.alt_key(),
            s => crate::hotkeys::key_code(s).is_some_and(|k| self.keys.contains(&k)),
        };
        self.settings
            .hotkeys
            .slots_for(&shown, code, &held)
            .into_iter()
            .filter_map(|i| shown[i].map(|s| (i, s.command)))
            .collect()
    }

    /// The command in slot `slot` of the command panel now.
    fn command_in(&self, slot: usize) -> Option<Command> {
        let obj = self
            .selected
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i])?;
        menu(&self.scene, obj, self.page)[slot].map(|s| s.command)
    }

    /// The command slot under a window position, if the interface is shown.
    fn button_at(&self, pos: PhysicalPosition<f64>) -> Option<usize> {
        if self.bar_hidden || Panels::covers_bar(self.tab) || self.reloading().is_some() {
            return None;
        }
        let layout = self.layout();
        self.interface
            .button_at(&layout, pos.x as f32, pos.y as f32)
    }

    /// Starts the music the sound panel chooses, if there is sound: none;
    /// the CD's tracks, if they have been ripped (else the MIDI tunes); or
    /// the MIDI tunes through a SoundFont. This port has no FX music; it
    /// plays as CD music.
    fn start_music(&mut self) {
        self.track_names.clear();
        let Some(audio) = &self.audio else { return };
        let mode = self.settings.config.music;
        if mode == panels::Music::None {
            return;
        }
        if mode != panels::Music::Midi {
            let cd_dir = self
                .cd_audio
                .clone()
                .unwrap_or_else(|| self.data.root().join("cdaudio"));
            let tracks = crate::music::cd_tracks(&cd_dir);
            if !tracks.is_empty() {
                // As the sound panel lists them (string 0x8e, "Track ").
                let word = self
                    .data
                    .strings
                    .get(&0x8e)
                    .map_or("Track ", |s| s.as_str());
                let names = (2..2 + tracks.len())
                    .map(|n| format!("{word}{n}"))
                    .collect();
                match Music::from_files(&audio.handle, tracks) {
                    Ok(music) => {
                        music.set_volume(if audio.muted { 0.0 } else { self.music_volume });
                        self.music = Some(music);
                        self.track_names = names;
                        return;
                    }
                    Err(e) => eprintln!("no CD music: {e:#}"),
                }
            }
        }
        let Some(font) = crate::music::find_soundfont(self.soundfont.as_deref()) else {
            eprintln!("no music: give a General MIDI SoundFont with --soundfont");
            return;
        };
        let tunes = crate::music::playlist(&self.data.sound_dir());
        let names = tunes
            .iter()
            .filter_map(|t| Some(t.file_name()?.to_string_lossy().into_owned()))
            .collect();
        match Music::new(&audio.handle, &font, &tunes) {
            Ok(music) => {
                music.set_volume(if audio.muted { 0.0 } else { self.music_volume });
                self.music = Some(music);
                self.track_names = names;
            }
            Err(e) => eprintln!("no music: {e:#}"),
        }
    }

    /// The surf under a mission (event 273, `prib3`): the original loops
    /// it on its ambient channel 9 (0x525370 kind 3) from the mission's
    /// start (0x50f910) while "ambient sound effects" is on (0x524f40).
    fn choose_ambience(&mut self) {
        let want = self.in_game
            && self.menu.is_none()
            && self.video.is_none()
            && self.settings.config.ambient_sounds
            && !self.no_ambience;
        if want != self.ambience.is_some() {
            self.ambience = None;
            if want {
                if let (Some(audio), Some(sounds)) =
                    (self.audio.as_mut(), self.data.sounds.as_ref())
                {
                    self.ambience = audio.loop_sound(sounds, "prib3");
                    self.no_ambience = self.ambience.is_none();
                }
            }
        }
        if let (Some(s), Some(audio)) = (&self.ambience, &self.audio) {
            s.set_volume(if audio.muted { 0.0 } else { audio.volume });
        }
    }

    /// The briefing's archive, `<VideoPath>TASKS.DKD` (Starttsk.cpp
    /// 0x53ed40, string 0x5c31c4), opened once.
    fn tasks(&mut self) -> Option<&openconquest_formats::Archive> {
        if self.tasks.is_none() {
            let path = crate::assets::find_ci(&self.data.root().join("video"), "tasks.dkx")
                .ok()
                .flatten();
            self.tasks = Some(path.and_then(|p| openconquest_formats::Archive::open(p).ok()));
        }
        self.tasks.as_ref().and_then(Option::as_ref)
    }

    /// The briefing's sample names, `<what><type>_<mission:02>` (the
    /// screen's + 0x100 and game + 0x3215).
    fn briefing_sample(&self, what: &str) -> String {
        let kind = self.score_kind(false).map_or(-1, |k| k as i32);
        let mission = self.campaign_number().unwrap_or(0);
        format!("{what}{kind}_{mission:02}")
    }

    /// The menus' sample channel (channel 0 of 0x568b30) in the front-end
    /// menus (not over a game, nor while a video plays), the game's music
    /// otherwise: `mus_menu` looped (0x525370 case 8: sound 325, flags 5),
    /// but on the briefing its FON wave looped instead (Starttsk.cpp
    /// 0x53ed40: `FON<type>_<mission>`, else `FON000`, flags 1), both at
    /// the effects' volume (DAT_005ca56c). On leaving the briefing for
    /// another screen `mus_menu` comes back (what the original's briefing
    /// does with the channel when left is not decoded).
    fn choose_music(&mut self) {
        let front = self.menu.is_some() && !self.in_game && self.video.is_none();
        let briefing = front && self.menu == Some(MenuState::Briefing);
        self.briefing_voice_step(briefing);
        let fon = if briefing && self.audio.is_some() {
            let own = self.briefing_sample("FON");
            self.tasks().and_then(|t| {
                [own.as_str(), "FON000"]
                    .into_iter()
                    .find(|n| {
                        t.find(n, openconquest_formats::ResourceKind::Sound)
                            .is_some()
                    })
                    .map(str::to_string)
            })
        } else {
            None
        };
        let want = match fon {
            Some(n) => Some(n),
            // `mus_menu` (0x525370 case 8) unless the music device is none
            // (DAT_005ca55e = 1, the sound panel's None).
            None if front
                && !self.no_menu_music
                && self.settings.config.music != crate::panels::Music::None =>
            {
                Some("mus_menu".to_string())
            }
            None => None,
        };
        if want == self.menu_sample {
            return;
        }
        self.menu_music = None;
        self.menu_sample = None;
        if let Some(name) = want {
            let tasks = self.tasks.as_ref().and_then(Option::as_ref);
            let archive = if name == "mus_menu" {
                self.data.sounds.as_ref()
            } else {
                tasks
            };
            if let (Some(audio), Some(archive)) = (self.audio.as_mut(), archive) {
                self.menu_music = audio.loop_sound(archive, &name);
                let volume = if audio.muted { 0.0 } else { audio.volume };
                if let Some(s) = &self.menu_music {
                    s.set_volume(volume);
                }
            }
            if self.menu_music.is_some() {
                self.menu_sample = Some(name);
            } else if name == "mus_menu" {
                self.no_menu_music = true;
            }
        }
        if let Some(m) = &self.music {
            m.set_paused(self.menu_music.is_some());
        }
    }

    /// The briefing's Start and Cancel (0x53ee30): when its FON or its
    /// voice was started and sound is on, every sound stops (0x569070) and
    /// the channels are freed. Start then goes into the game with no menu
    /// music; Cancel to the main menu (0x100), which plays `mus_menu`
    /// again (`choose_music`).
    fn leave_briefing_sounds(&mut self) {
        let fon = self.menu_sample.as_deref().is_some_and(|n| n != "mus_menu");
        let voice = self.briefing_voice.is_some_and(|(_, k)| k >= 3);
        if !(fon || voice) {
            return;
        }
        if let Some(audio) = self.audio.as_mut() {
            audio.stop_all();
            // The channel's sample stops too; `choose_music` notes it.
            if let Some(m) = self.menu_music.take() {
                m.stop();
            }
        }
    }

    /// The briefing's voice (Starttsk.cpp idle 0x53fba0): 3000 ms after
    /// the screen was made its clock moves on, and at the next turn its
    /// `VOICE<type>_<mission>` wave, if `TASKS.DKD` has one, plays once.
    fn briefing_voice_step(&mut self, briefing: bool) {
        if !briefing {
            self.briefing_voice = None;
            return;
        }
        match self.briefing_voice {
            None => self.briefing_voice = Some((Instant::now(), 0)),
            Some((since, 0)) => {
                if since.elapsed() >= Duration::from_millis(3000) {
                    self.briefing_voice = Some((Instant::now(), 1));
                }
            }
            Some((_, 1)) => {
                let name = self.briefing_sample("VOICE");
                if self.tasks().is_some() {
                    let tasks = self.tasks.as_ref().and_then(Option::as_ref);
                    if let (Some(audio), Some(tasks)) = (self.audio.as_mut(), tasks) {
                        if tasks
                            .find(&name, openconquest_formats::ResourceKind::Sound)
                            .is_some()
                        {
                            audio.play(tasks, &name, 1.0);
                        }
                    }
                }
                self.briefing_voice = Some((Instant::now(), 3));
            }
            Some(_) => {}
        }
    }

    /// Plays the sounds the scene reported, by their distance from the
    /// centre of the view.
    fn play_scene_sounds(&mut self) {
        let sounds = std::mem::take(&mut self.scene.sounds);
        if sounds.is_empty() {
            return;
        }
        let (screen, _) = self.placement();
        let (Some(audio), Some(archive)) = (self.audio.as_mut(), self.data.sounds.as_ref()) else {
            return;
        };
        let (vw, vh) = self.camera.view_size(&screen);
        let (cx, cy) = (self.camera.x + vw / 2.0, self.camera.y + vh / 2.0);
        let reach = (vw * vw + vh * vh).sqrt() / 2.0;
        let mut played: Vec<&str> = Vec::new();
        let c = &self.settings.config;
        for (name, x, y) in sounds {
            // One of each sound per frame, and not too many at once.
            if played.contains(&name) || played.len() >= 8 {
                continue;
            }
            // The sound panel's switches (see `sound_kind`).
            let on = match crate::scene::sound_kind(name) {
                crate::scene::SoundKind::Voice => c.unit_report,
                crate::scene::SoundKind::Battle => c.battle_sounds,
                crate::scene::SoundKind::Ambient => c.ambient_sounds,
                crate::scene::SoundKind::Always => true,
            };
            if !on {
                continue;
            }
            let gain = if x.is_nan() {
                1.0
            } else {
                crate::audio::falloff(((x - cx).powi(2) + (y - cy).powi(2)).sqrt(), reach)
            };
            if gain > 0.05 {
                audio.play(archive, name, gain);
                played.push(name);
            }
        }
    }

    /// The load screen's entries on tab `tab`: saved games of the campaign
    /// or of other missions, or the campaign's missions to start from.
    fn load_entries(&self, tab: LoadTab) -> Vec<(LoadEntry, String)> {
        if tab == LoadTab::StartFrom {
            return self
                .reports
                .iter()
                .enumerate()
                .map(|(i, r)| (LoadEntry::Report(i), r.label.clone()))
                .collect();
        }
        self.saves
            .iter()
            .enumerate()
            .filter(|(_, e)| crate::scene::is_campaign(&e.mission) == (tab == LoadTab::Scenario))
            .map(|(i, e)| (LoadEntry::Save(i), e.label.clone()))
            .collect()
    }

    /// Where start reports go: the original's `<MainPath>MISSIONS\`, here
    /// in the port's saves folder.
    fn reports_dir(&self) -> std::path::PathBuf {
        self.data.save_dir().join("MISSIONS")
    }

    /// The briefing's Start writes a start report (0x53f550 case 0x178,
    /// 0x53a3a0) for a campaign mission when it is the first or it was
    /// continued (game + 0x32fd set): `SAVE_` + string 100 `MISSION_` +
    /// the mission's number + 1 in two digits + `.RPM`; the word "RP" and
    /// the difficulty word (DAT_005ca78d), then the campaign's record.
    fn write_start_report(&mut self) {
        let Some(n) = self.campaign_number() else {
            return;
        };
        if n != 0 && self.carry.is_none() {
            return;
        }
        let body = ReportBody {
            mission: n,
            carry: self.carry.clone(),
        };
        let dir = self.reports_dir();
        let path = dir.join(format!("SAVE_MISSION_{:02}.RPM", n + 1));
        let written = report_bytes(self.difficulty as u16, &body).and_then(|bytes| {
            std::fs::create_dir_all(&dir)?;
            std::fs::write(&path, &bytes)?;
            Ok(())
        });
        if let Err(e) = written {
            self.say(format!("Could not write {}: {e:#}", path.display()));
        }
    }

    /// The start reports (0x52b7c0, Load's tab 4): the files of the
    /// reports' folder whose first word is "RP" (0x53a5a0), by name, each
    /// shown as `"%s  [%02u/%02u/%04u, %02u:%02u, %s]"` (0x5c26c4): the
    /// name without its extension and `SAVE_`, the local date of its last
    /// write (month first when string 10000 starts with '0', as this
    /// CD's does, else the day) and time, and string 106 + difficulty.
    fn read_reports(&self) -> Vec<StartReport> {
        let Ok(dir) = std::fs::read_dir(self.reports_dir()) else {
            return Vec::new();
        };
        let mut paths: Vec<std::path::PathBuf> = dir
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        let month_first = self
            .data
            .strings
            .get(&10000)
            .and_then(|s| s.chars().next())
            .is_none_or(|c| c == '0');
        paths
            .into_iter()
            .filter_map(|path| {
                let (difficulty, body) = read_report(&path)?;
                let stem = path.file_stem()?.to_string_lossy().into_owned();
                let name = stem.strip_prefix("SAVE_").unwrap_or(&stem).to_string();
                let label = match std::fs::metadata(&path).and_then(|m| m.modified()) {
                    Ok(t) => {
                        let secs = t
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs() as i64);
                        let (y, mo, d, h, mi) = crate::records::local_date(secs);
                        let (a, b) = if month_first { (mo, d) } else { (d, mo) };
                        let level = self
                            .data
                            .strings
                            .get(&(106 + u32::from(difficulty)))
                            .cloned()
                            .unwrap_or_default();
                        format!("{name}  [{a:02}/{b:02}/{y:04}, {h:02}:{mi:02}, {level}]")
                    }
                    Err(_) => name,
                };
                Some(StartReport {
                    path,
                    label,
                    mission: body.mission,
                })
            })
            .collect()
    }

    /// The mission of a load screen entry.
    fn entry_mission(&self, entry: LoadEntry) -> Option<usize> {
        match entry {
            LoadEntry::Report(i) => {
                let name = format!("miss0{:02}", self.reports.get(i)?.mission);
                self.missions.iter().position(|m| *m == name)
            }
            LoadEntry::Save(i) => {
                let name = self.saves.get(i)?.mission.to_lowercase();
                self.missions.iter().position(|m| *m == name)
            }
        }
    }

    /// The answer under the pointer, if a question is asked.
    fn ask_answer_at(&self) -> Option<bool> {
        let c = self.cursor?;
        let scale = self.layout().scale;
        let (screen, _) = self.placement();
        let (w, h) = self.interface.panels.ask_size();
        let x0 = screen.x as f32 + (screen.w as f32 - w as f32 * scale) / 2.0;
        let y0 = screen.y as f32 + (screen.h as f32 - h as f32 * scale) / 2.0;
        let (x, y) = ((c.x as f32 - x0) / scale, (c.y as f32 - y0) / scale);
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let yes_no = self.ask.is_some_and(Ask::yes_no);
        self.interface.panels.ask_hit(yes_no, (x as u32, y as u32))
    }

    /// A question's text, from the game's strings.
    fn ask_text(&self, ask: Ask) -> String {
        let (id, fallback) = ask.string();
        self.data
            .strings
            .get(&id)
            .cloned()
            .unwrap_or_else(|| fallback.into())
    }

    /// The question answered: yes ends the game as asked, no goes on.
    fn answer(&mut self, yes: bool) {
        let Some(ask) = self.ask.take() else {
            return;
        };
        self.ask_pressed = None;
        if !yes {
            return;
        }
        match ask {
            Ask::Overwrite => self.save_now(),
            Ask::Load => {
                if let Some(e) = self.selected_save.and_then(|i| self.saves.get(i)) {
                    let path = e.path.clone();
                    self.load_game(&path);
                }
            }
            Ask::Delete => {
                if let Some(e) = self.selected_save.and_then(|i| self.saves.get(i)) {
                    let _ = std::fs::remove_file(&e.path);
                    self.selected_save = None;
                    self.save_name.clear();
                    self.refresh_panels();
                }
            }
            Ask::NoName | Ask::SaveFailed => {}
            Ask::Restart => {
                if self.switch_mission(0) {
                    self.fade_into_load();
                }
            }
            // The game ends and the title screen comes back (0x4fc).
            Ask::MainMenu => {
                if self.menus.is_some() {
                    self.leave_game();
                }
            }
            Ask::Quit => self.quit_game(),
        }
    }

    fn say(&mut self, msg: impl Into<String>) {
        // A refusal with nothing to say (a command not to be had) says
        // nothing.
        let msg = msg.into();
        if msg.is_empty() {
            return;
        }
        self.status = Some((msg, Instant::now()));
    }

    /// A right click on command button `slot` (0x4f46c0): on a ship,
    /// marine, weapon or refit button, takes the last one ordered off the
    /// queue (message 0x3c), its price paid back; on 0x14, 0x15, 0x3a-0x3c,
    /// 0x3e-0x54 and 0x67, order 10 to the selection. Nothing while the
    /// game is paused (screen + 0x17e6; the "Bad connection to" box, + 0x17ea,
    /// is not ported).
    fn cancel_button(&mut self, slot: usize) {
        if self.paused {
            return;
        }
        let Some(obj) = self
            .selected
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i])
        else {
            return;
        };
        let Some(entry) = menu(&self.scene, obj, self.page)[slot] else {
            return;
        };
        // The button's second message (0x1d83 + i, 0x4f46c0): for the
        // codes that send order 10 or 0x3c the pointer is reset first.
        let code = entry.command.0;
        if matches!(code, 0x14 | 0x15 | 0x21..=0x3c | 0x3e..=0x54 | 0x67 | 0x7d) {
            self.pending = None;
        }
        // Order 10 (no data) to the selection for 0x14, 0x15, 0x3a-0x3c
        // (UPGTOWN, UPGDOCK, CBUT_UPARMING: the inner switch's 0x3a/0x3b
        // labels are dead), 0x3e-0x54 and 0x67 (`Scene::halt`).
        if matches!(code, 0x14 | 0x15 | 0x3a..=0x3c | 0x3e..=0x54 | 0x67) {
            for id in self.selection() {
                if let Err(e) = self.issue(Order::Halt { id }) {
                    self.say(e);
                }
            }
            return;
        }
        if let Some(kind) = economy::ship_of_command(entry.command) {
            if entry.count > 0 {
                let order = Order::CancelShip { site: obj.id, kind };
                if let Err(e) = self.issue(order) {
                    self.say(e);
                }
            }
            return;
        }
        if let Some((upgrade, level)) = crate::combat::refit_of_command(entry.command.0) {
            if entry.count > 0 {
                let order = Order::CancelRefit {
                    smithy: obj.id,
                    upgrade,
                    level,
                };
                if let Err(e) = self.issue(order) {
                    self.say(e);
                }
            }
            return;
        }
        let train = Command::TRAIN_BOWMEN.0;
        if (train..train + 9).contains(&entry.command.0) && entry.count > 0 {
            let order = Order::CancelTrain {
                site: obj.id,
                item: entry.command.0 - train,
            };
            if let Err(e) = self.issue(order) {
                self.say(e);
            }
        }
    }

    /// A left click (or accelerator) on command button `slot` (0x4f34c0):
    /// nothing while the game is paused (0x4f34fc/0x4f3508: screen +
    /// 0x17e6).
    fn press_button(&mut self, slot: usize) {
        if self.paused {
            return;
        }
        let Some(obj) = self
            .selected
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i])
        else {
            return;
        };
        let Some(entry) = menu(&self.scene, obj, self.page)[slot] else {
            return;
        };
        let id = obj.id;
        let obj_is_ship = obj.ship.is_some();
        let ship_kind = economy::ship_of_command(entry.command);
        if !entry.enabled {
            let reason = match ship_kind {
                Some(_) if self.scene.production_at(id).is_some() => {
                    "The shipyard is busy".to_string()
                }
                Some(kind) => self.scene.can_build_ship(kind).err().unwrap_or_default(),
                None if (Command::TRAIN_BOWMEN.0..=Command::TRAIN_BOWMEN.0 + 8)
                    .contains(&entry.command.0) =>
                {
                    "Not enough amber or fish".to_string()
                }
                None if matches!(entry.command, Command::UPGRADE_TOWN | Command::UPGRADE_DOCK)
                    || economy::building_of_command(entry.command).is_some() =>
                {
                    "Not enough amber".to_string()
                }
                None if crate::combat::refit_of_command(entry.command.0).is_some() => {
                    "No ship of yours lies next to the smithy needing it, or not enough amber"
                        .to_string()
                }
                None if entry.command == magic::GET_MAGIC
                    || crate::combat::Upgrade::of_command(entry.command.0).is_some() =>
                {
                    "Not enough amber, or the building is busy".to_string()
                }
                None if Spell::of_command(entry.command).is_some_and(|s| s.implemented())
                    || (magic::WIND_EAST.0..magic::WIND_EAST.0 + 8).contains(&entry.command.0) =>
                {
                    "Not enough amber or fish".to_string()
                }
                None => match entry.command {
                    Command::UNLOAD => "Nothing aboard to unload".to_string(),
                    Command::FIRE => "This ship has no weapons".to_string(),
                    Command::BOARD => "No marines aboard to board with".to_string(),
                    Command::REPAIR_SHIP => "The ship needs no repair".to_string(),
                    Command::LOAD => "The settlement is busy, or has no fort".to_string(),
                    Command::SELL_ITEM | Command::SELL_TOWN => "The settlement is busy".to_string(),
                    Command::REPAIR_STOCK => {
                        "Nothing worn to mend, the settlement is busy, or not enough amber"
                            .to_string()
                    }
                    Command::BUILD_ARGO => {
                        "The shipyard is busy, or not enough amber or fish".to_string()
                    }
                    crate::commands::FIRE_ON_AGGRESSOR => "Only while guarding".to_string(),
                    crate::commands::GUARD_ZONE | crate::commands::ATTACK_ZONE => {
                        "This ship has no weapons".to_string()
                    }
                    _ => "Not possible now".to_string(),
                },
            };
            // The original only greys the button: no sound (its "deny"
            // sound, event 319, is the sting for an enemy ship sunk).
            self.say(reason);
            return;
        }
        if let Some(kind) = ship_kind {
            match self.issue(Order::BuildShip { site: id, kind }) {
                Ok(_) => self.say(format!("Building a {}", SHIP_NAMES[kind as usize])),
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(class) = economy::building_of_command(entry.command) {
            self.pending = Some(entry.command);
            self.say(format!(
                "Choose where to build the {}",
                crate::objects::building_label(class)
            ));
            return;
        }
        match entry.command {
            crate::commands::RESEARCH => self.page = Page::Research,
            Command::BEHAVIOUR => self.page = Page::Behaviour,
            Command::ZONES => self.page = Page::Zones,
            c if crate::commands::GATHER_ZONES.contains(&c) => {
                self.pending = Some(c);
                self.say("Drag a rectangle around the zone");
            }
            c @ (crate::commands::GUARD_ZONE | crate::commands::ATTACK_ZONE) => {
                self.pending = Some(c);
                self.say("Drag a rectangle around the zone");
            }
            crate::commands::CONVOY => {
                self.pending = Some(crate::commands::CONVOY);
                self.say("Drag a rectangle around the ship to escort");
            }
            c @ (crate::commands::SCOUT | crate::commands::PATROL) => {
                self.pending = Some(c);
                self.say(if c == crate::commands::SCOUT {
                    "Choose where to scout"
                } else {
                    "Choose the far end of the patrol"
                });
            }
            c @ (crate::commands::GUARD
            | crate::commands::RAM_ATTACK
            | crate::commands::BOARD_ATTACK
            | crate::commands::FIRE_ON_AGGRESSOR) => {
                if let Err(e) = self.issue(Order::Behaviour {
                    ship: id,
                    command: c,
                }) {
                    self.say(e);
                }
            }
            Command::BUILD_STRUCTURES => {
                self.pending = None;
                self.page = if self.page == Page::Structures {
                    Page::Main
                } else {
                    Page::Structures
                };
            }
            Command::MOVE => {
                self.pending = Some(Command::MOVE);
                self.say("Choose a destination");
            }
            Command::STOP => {
                self.pending = None;
                for id in self.selection() {
                    let _ = self.issue(Order::Stop { id });
                }
            }
            c if (Command::TRAIN_BOWMEN.0..=Command::TRAIN_BOWMEN.0 + 8).contains(&c.0) => {
                let kind = entry.command.0 - Command::TRAIN_BOWMEN.0;
                match self.issue(Order::Train {
                    site: id,
                    item: kind,
                }) {
                    Ok(_) => self.say(entry.command.name()),
                    Err(e) => self.say(e),
                }
            }
            // The pointer's sell mode: the stock unit pressed next is
            // sold (c_info.cpp 0x4f72d0 with the cursor in mode 0x34).
            Command::SELL_ITEM => {
                self.pending = Some(Command::SELL_ITEM);
                self.say("Choose an item in the stock to sell");
            }
            Command::SELL_TOWN => {
                if let Err(e) = self.issue(Order::SellTown { site: id }) {
                    self.say(e);
                }
            }
            Command::REPAIR_STOCK => {
                if let Err(e) = self.issue(Order::RepairStock { workshop: id }) {
                    self.say(e);
                }
            }
            // A ship's Reload Object picks the port, dock or ship to reload
            // at (pointer mode 12, command 0x1f).
            Command::UNLOAD => {
                self.pending = Some(Command::UNLOAD);
                self.say("Choose your port or settlement to unload at");
            }
            Command::LOAD if obj_is_ship => {
                self.pending = Some(Command::LOAD);
                self.say("Choose your port, shipyard, dock, fort or ship to reload at");
            }
            // A settlement's Reload Object picks one of its forts (0x441ba0
            // case 0x1f).
            Command::LOAD => {
                self.pending = Some(Command::LOAD);
                self.say("Choose one of the settlement's forts");
            }
            c if crate::combat::refit_of_command(c.0).is_some() => {
                let (upgrade, level) = crate::combat::refit_of_command(c.0).unwrap();
                match self.issue(Order::Refit {
                    smithy: id,
                    upgrade,
                    level,
                }) {
                    Ok(_) => self.say(entry.command.name()),
                    Err(e) => self.say(e),
                }
            }
            c if crate::combat::Upgrade::of_command(c.0).is_some() => {
                if entry.progress.is_none() {
                    let upgrade = crate::combat::Upgrade::of_command(c.0).unwrap();
                    match self.issue(Order::Research { site: id, upgrade }) {
                        Ok(_) => self.say(entry.command.name()),
                        Err(e) => self.say(e),
                    }
                }
            }
            magic::GET_MAGIC => {
                if entry.progress.is_none() {
                    let temple = self
                        .scene
                        .object_index(id)
                        .and_then(|i| match self.scene.objects[i].class {
                            openconquest_formats::ObjectClass::Building(c) => Temple::of_class(c),
                            _ => None,
                        })
                        .unwrap_or(Temple::Sea);
                    match self.issue(Order::LearnSpell {
                        temple,
                        site: Some(id),
                    }) {
                        Ok(_) => self.say("The priests study a new spell"),
                        Err(e) => self.say(e),
                    }
                }
            }
            c if Spell::of_command(c) == Some(Spell::Wind) => self.page = Page::Wind,
            c if Spell::of_command(c) == Some(Spell::Flame) => {
                match self.issue(Order::CastFlame { site: Some(id) }) {
                    Ok(_) => self.say("Your weapons burn hotter"),
                    Err(e) => self.say(e),
                }
            }
            Command(0x57) => self.page = Page::Main,
            c if (magic::WIND_EAST.0..magic::WIND_EAST.0 + 8).contains(&c.0) => {
                match self.issue(Order::CastWind {
                    direction: (c.0 - magic::WIND_EAST.0) as u32,
                }) {
                    Ok(_) => {
                        self.page = Page::Main;
                        self.say("The wind rises");
                    }
                    Err(e) => self.say(e),
                }
            }
            c if Spell::of_command(c).is_some_and(|s| {
                matches!(s.target(), Target::Water | Target::Area | Target::OwnShip)
            }) =>
            {
                self.pending = Some(c);
                self.say(Spell::of_command(c).unwrap().prompt());
            }
            Command::BUILD_GROUND => self.page = Page::Ground,
            Command::RAM | Command::BOARD => {
                self.pending = Some(entry.command);
                self.say("Choose an enemy ship");
            }
            Command::TREASURE => {
                self.pending = Some(Command::TREASURE);
                self.say("Choose a treasure or a hero's house");
            }
            Command::REPAIR_SHIP => {
                self.pending = Some(Command::REPAIR_SHIP);
                self.say("Choose one of your docks");
            }
            Command::REPAIR_BUILDING => {
                self.pending = Some(Command::REPAIR_BUILDING);
                self.say("Choose a damaged building of yours");
            }
            Command::FIRE => {
                self.pending = Some(Command::FIRE);
                self.say("Choose an enemy ship or building");
            }
            Command::AMBER => {
                self.pending = Some(Command::AMBER);
                self.say("Choose an amber deposit");
            }
            Command::FISH => {
                self.pending = Some(Command::FISH);
                self.say("Choose a fishing ground");
            }
            Command::WHALE => {
                self.pending = Some(Command::WHALE);
                self.say("Choose a leviathan");
            }
            Command::BUILD_ARGO => {
                if entry.progress.is_none() {
                    match self.issue(Order::BuildArgo { site: id }) {
                        Ok(_) => self.say("Building the Argo"),
                        Err(e) => self.say(e),
                    }
                }
            }
            Command::UPGRADE_TOWN | Command::UPGRADE_DOCK => {
                match self.issue(Order::Upgrade { id }) {
                    Ok(_) => self.say("Upgrade complete"),
                    Err(e) => self.say(e),
                }
            }
            _ => {}
        }
    }

    /// The world viewport and minimap rectangle for the current window.
    /// The view and minimap rectangles (as `GameView::placement`, without
    /// building the whole view, which asks for the hovered object).
    fn placement(&self) -> (Screen, MinimapRect) {
        let layout = self.layout();
        (layout.view, layout.minimap)
    }

    /// The interface's placement for the current window, bar side and
    /// whether the bar is hidden.
    fn layout(&self) -> crate::ui::Layout {
        self.interface.layout(
            self.window_size(),
            &self.camera.policy,
            &self.scene,
            self.settings.config.bar_left,
            self.bar_hidden,
        )
    }

    fn update_title(&self) {
        if let Some(g) = &self.gfx {
            let s = &self.scene;
            g.window.set_title(&format!(
                "OpenConquest — {} ({}x{}) — PgUp/PgDn: mission · Tab: radar map · F11: fullscreen",
                s.mission, s.grid.width, s.grid.height
            ));
        }
    }

    fn switch_mission(&mut self, delta: isize) -> bool {
        if self.net.is_some() {
            self.say("Not in a multiplayer game");
            return false;
        }
        let n = self.missions.len() as isize;
        let next = (self.mission_index as isize + delta).rem_euclid(n) as usize;
        match Scene::load_at(
            &self.data,
            &mut self.caches,
            &self.missions[next],
            self.difficulty,
        ) {
            Ok(scene) => {
                self.scene = scene;
                // Every game build shows the bar (0x4ebf50: [game+0x328d] = 0).
                self.bar_hidden = false;
                self.scene.fog = self.fog;
                self.selected = None;
                self.group.clear();
                self.pending = None;
                self.mission_index = next;
                self.center_pending = true;
                self.briefing = true;
                self.refresh_panels();
                self.update_title();
                true
            }
            Err(e) => {
                eprintln!("failed to load {}: {e:#}", self.missions[next]);
                false
            }
        }
    }

    fn quick_save_path(&self) -> std::path::PathBuf {
        self.data.save_dir().join("quick.json")
    }

    /// Saves the game in progress (Ctrl+S).
    fn quick_save(&mut self) {
        if self.net.is_some() {
            self.say("Multiplayer games cannot be saved");
            return;
        }
        let path = self.quick_save_path();
        match self
            .saved_scene()
            .write(&path, self.settings.config.pack_saves)
        {
            Ok(()) => self.say("Game saved"),
            Err(e) => self.say(format!("Could not save: {e:#}")),
        }
    }

    /// Loads the quick save (Ctrl+O).
    fn quick_load(&mut self) {
        let path = self.quick_save_path();
        self.load_game(&path);
    }

    fn load_game(&mut self, path: &std::path::Path) {
        if self.net.is_some() {
            self.say("Not in a multiplayer game");
            return;
        }
        let path = path.to_path_buf();
        let loaded = SaveGame::read(&path).and_then(|save| {
            let scene = Scene::restore(&self.data, &mut self.caches, &save)?;
            Ok((save.mission, scene, save.game_id))
        });
        match loaded {
            Ok((mission, scene, id)) => {
                self.scene = scene;
                // Every game build shows the bar (0x4ebf50: [game+0x328d] = 0).
                self.bar_hidden = false;
                self.scene.fog = self.fog;
                if let Some(i) = self.missions.iter().position(|m| *m == mission) {
                    self.mission_index = i;
                }
                // The save's ID_GAME, else a new id (0x4ed3f0).
                self.game_id = id.unwrap_or_else(crate::records::game_id);
                self.note_campaign();
                self.selected = None;
                self.group.clear();
                self.pending = None;
                self.page = Page::Main;
                self.briefing = false;
                self.refresh_panels();
                self.update_title();
                self.fade_into_load();
                self.say("Game loaded");
            }
            Err(e) => self.say(format!("Could not load: {e:#}")),
        }
    }

    fn toggle_fullscreen(&mut self) {
        if let Some(g) = &self.gfx {
            let fs = g.window.fullscreen().is_some();
            g.window
                .set_fullscreen((!fs).then_some(Fullscreen::Borderless(None)));
        }
    }

    fn is_fullscreen(&self) -> bool {
        self.gfx
            .as_ref()
            .is_some_and(|g| g.window.fullscreen().is_some())
    }

    /// Moves the camera to the world point under a minimap position.
    /// Returns false if the position is outside the minimap.
    fn minimap_jump(&mut self, pos: PhysicalPosition<f64>) -> bool {
        if self.bar_hidden || Panels::covers_bar(self.tab) || self.radar_shown() {
            return false;
        }
        let (screen, minimap) = self.placement();
        let (sx, sy) = (pos.x as f32, pos.y as f32);
        if !minimap.contains(sx, sy) {
            return false;
        }
        let (wx, wy) = minimap.to_world(sx, sy);
        self.camera
            .center_on(wx, wy, &screen, self.scene.map_size());
        true
    }

    /// Whether the radar map is shown (Tab, with the control bar).
    fn radar_shown(&self) -> bool {
        self.radar && !self.bar_hidden
    }

    /// The world point under a window position on the map: in the view,
    /// or on the radar map in its place (where a click is one on the map,
    /// Mapa.cpp 0x51cdd0 handing it to the pointer as the view does; the
    /// view in the bar's window takes none).
    fn view_world(&self, pos: PhysicalPosition<f64>) -> Option<(f32, f32)> {
        let (sx, sy) = (pos.x as f32, pos.y as f32);
        if let Some((_, radar)) = self.radar_rects() {
            return radar.contains(sx, sy).then(|| radar.to_world(sx, sy));
        }
        let (screen, _) = self.placement();
        screen
            .contains(sx, sy)
            .then(|| self.camera.screen_to_world(&screen, sx, sy))
    }

    /// As `view_world`, past the map's edges too.
    fn view_world_any(&self, pos: PhysicalPosition<f64>) -> (f32, f32) {
        let (sx, sy) = (pos.x as f32, pos.y as f32);
        match self.radar_rects() {
            Some((_, radar)) => radar.to_world(sx, sy),
            None => self.camera.screen_to_world(&self.placement().0, sx, sy),
        }
    }

    /// With the radar map: the bar's map window showing the view, and the
    /// radar map in the view's place.
    fn radar_rects(&self) -> Option<(Screen, MinimapRect)> {
        if !self.radar_shown() {
            return None;
        }
        let layout = self.layout();
        Some(crate::ui::radar_placement(
            &layout,
            &self.scene,
            self.radar_origin,
        ))
    }

    /// Space (0x50f910 message 0x4e1, no guard of its own): any drag on
    /// the maps ended (0x4ee970: broadcast 0x14a), a radar map put back
    /// (0xcbc), the bar hidden or shown ([game+0x328d] flipped), the bar's
    /// tab the object panel either way (message 0x22 with 1000), and the
    /// windows moved (message 0x24): with the bar hidden the view is 12
    /// cells wide at (16, 8), its scroll clamped to the map (0xcc3: the
    /// top-left cell kept).
    fn toggle_bar(&mut self) {
        self.drag_from = None;
        self.box_from = None;
        self.minimap_drag = false;
        if self.radar {
            self.toggle_radar();
        }
        self.bar_hidden = !self.bar_hidden;
        self.tab = Tab::Object;
        let (screen, _) = self.placement();
        self.camera.clamp(&screen, self.scene.map_size());
    }

    /// Tab: the radar map in the view's place and the view in the bar's
    /// map window, or back (0x504620 message 0xcbc: the view's mapa
    /// between modes 1 and 2, the bar's between 3 and 4). Not with the bar
    /// hidden.
    fn toggle_radar(&mut self) {
        if self.bar_hidden {
            return;
        }
        self.radar = !self.radar;
        if self.radar {
            self.radar_origin = (0, 0);
            self.scroll_radar();
            // The bar's window from the view's top-left corner (0x51bd20);
            // the view itself stays put, and is back where it was after
            // (seen on the original).
            self.radar_view = self.camera;
            if let Some((bar, _)) = self.radar_rects() {
                self.radar_view.clamp(&bar, self.scene.map_size());
            }
        }
    }

    /// Keeps the view's frame on the radar map.
    fn scroll_radar(&mut self) {
        if !self.radar_shown() {
            return;
        }
        let layout = self.layout();
        self.radar_origin =
            crate::ui::radar_origin(&layout, &self.scene, &self.camera, self.radar_origin);
    }

    /// The pointer over the radar map takes the bar's window there: the 3
    /// x 3 cells around the cell under it (0x51c6b0, message 0xcb9); the
    /// view's frame stays (seen on the original).
    fn radar_hover(&mut self, pos: PhysicalPosition<f64>) {
        if self.menu.is_some() {
            return;
        }
        let Some((bar, radar)) = self.radar_rects() else {
            return;
        };
        let (sx, sy) = (pos.x as f32, pos.y as f32);
        if !radar.contains(sx, sy) {
            return;
        }
        let t = openconquest_formats::map::TILE_SIZE as f32;
        let (wx, wy) = radar.to_world(sx, sy);
        let (cx, cy) = ((wx / t).floor() + 0.5, (wy / t).floor() + 0.5);
        self.radar_view
            .center_on(cx * t, cy * t, &bar, self.scene.map_size());
    }

    fn left_click(&mut self, pos: PhysicalPosition<f64>) {
        if self.panel_click(pos) {
            return;
        }
        if self.load_click(pos) {
            return;
        }
        if self.info_press(pos) {
            return;
        }
        if let Some(slot) = self.button_at(pos) {
            self.press_button(slot);
            return;
        }
        if self.on_arming_box(pos) {
            let on = !self.scene.default_arming;
            if let Err(e) = self.issue(Order::DefaultArming { on }) {
                self.say(e);
            }
            return;
        }
        if self.pending == Some(Command::MOVE) {
            self.pending = None;
            self.status = None;
            self.right_click(pos);
            return;
        }
        if let Some(class) = self.pending.and_then(economy::building_of_command) {
            let (Some(settlement), Some(cell)) = (self.selected, self.cell_at(pos)) else {
                return;
            };
            // A ship sails there to put it up.
            if self.selected_ship().is_some() {
                match self.issue(Order::Found {
                    ship: settlement,
                    cx: cell.0,
                    cy: cell.1,
                    class,
                }) {
                    Ok(_) => {
                        self.pending = None;
                        self.page = Page::Main;
                        self.status = None;
                    }
                    Err(e) => self.say(e),
                }
                return;
            }
            match self.issue(Order::PlaceBuilding {
                site: settlement,
                class,
                cx: cell.0,
                cy: cell.1,
            }) {
                Ok(_) => {
                    self.pending = None;
                    self.page = Page::Main;
                    self.say(format!(
                        "Building a {}",
                        crate::objects::building_label(class)
                    ));
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(spell) = self
            .pending
            .and_then(Spell::of_command)
            .filter(|s| matches!(s.target(), Target::Area | Target::OwnShip))
        {
            let order = match (spell.target(), self.object_at(pos), self.cell_at(pos)) {
                (Target::OwnShip, Some(target), _) => self.issue(Order::CastOn { spell, target }),
                (Target::Area, _, Some((cx, cy))) => self.issue(Order::CastArea { spell, cx, cy }),
                _ => Err(spell.prompt().to_string()),
            };
            match order {
                Ok(_) => {
                    self.pending = None;
                    self.status = None;
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(spell) = self
            .pending
            .and_then(Spell::of_command)
            .filter(|s| s.target() == Target::Water)
        {
            let Some(cell) = self.cell_at(pos) else {
                return;
            };
            match self.issue(Order::CastHazard {
                spell,
                cx: cell.0,
                cy: cell.1,
            }) {
                Ok(_) => {
                    self.pending = None;
                    self.status = None;
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(cmd @ (Command::RAM | Command::BOARD)) = self.pending {
            let order = match self.object_at(pos) {
                Some(target) => self.order_each(|ship| Order::Close {
                    ship,
                    target,
                    board: cmd == Command::BOARD,
                }),
                None => Err("Choose an enemy ship".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(cmd @ (Command::REPAIR_SHIP | Command::REPAIR_BUILDING)) = self.pending {
            let order = match self.object_at(pos) {
                Some(target) if cmd == Command::REPAIR_SHIP => {
                    self.order_each(|ship| Order::Repair { ship, dock: target })
                }
                Some(target) => self.order_each(|site| Order::Mend { site, target }),
                None => Err("Choose a building".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                    self.confirm(pos);
                }
                Err(e) => self.say(e),
            }
            return;
        }
        let zone = matches!(
            self.pending,
            Some(
                crate::commands::GUARD_ZONE
                    | crate::commands::ATTACK_ZONE
                    | crate::commands::CONVOY
            )
        ) || self
            .pending
            .is_some_and(|c| crate::commands::GATHER_ZONES.contains(&c));
        if zone && self.world_at(pos).is_some() {
            // The zone is the box dragged from here (see `finish_select`).
            self.box_from = Some(pos);
            return;
        }
        if let Some(cmd @ (crate::commands::SCOUT | crate::commands::PATROL)) = self.pending {
            let order = match self.world_at(pos) {
                Some((x, y)) if cmd == crate::commands::SCOUT => {
                    self.order_each(|ship| Order::Scout { ship, x, y })
                }
                Some((x, y)) => self.order_each(|ship| Order::Patrol { ship, x, y }),
                None => Err("Choose a place at sea".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                    self.confirm(pos);
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if self.pending == Some(Command::LOAD) {
            let order = match self.object_at(pos) {
                Some(target) => self.order_each(|ship| Order::LoadObject { ship, target }),
                None => Err("Choose your port, shipyard, dock, fort or ship".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                    self.confirm(pos);
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if self.pending == Some(Command::UNLOAD) {
            let order = match self.object_at(pos) {
                Some(port) => self.order_each(|ship| Order::Unload { ship, port }),
                None => Err("Choose one of your ports or settlements".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                    self.confirm(pos);
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if self.pending == Some(Command::TREASURE) {
            let order = match self.object_at(pos) {
                Some(target) => self.order_each(|ship| Order::Take { ship, target }),
                None => Err("Choose a treasure or a hero's house".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                    self.confirm(pos);
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if self.pending == Some(Command::FIRE) {
            let order = match self.object_at(pos) {
                Some(target) => self.order_each(|ship| Order::Attack { ship, target }),
                None => Err("Choose an enemy ship or building".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(cmd @ (Command::AMBER | Command::FISH | Command::WHALE)) = self.pending {
            let wanted = match cmd {
                Command::AMBER => Resource::Amber,
                Command::FISH => Resource::Fish,
                _ => Resource::Leviathan,
            };
            let target = self.object_at(pos).filter(|&id| {
                self.scene
                    .object_index(id)
                    .and_then(|i| self.scene.objects[i].deposit)
                    .is_some_and(|d| d.resource == wanted)
            });
            match (target, self.selected) {
                (Some(deposit), Some(_)) => {
                    self.pending = None;
                    self.status = None;
                    if let Err(e) = self.order_each(|ship| Order::Gather { ship, deposit }) {
                        self.say(e);
                    }
                }
                _ => self.say(match cmd {
                    Command::AMBER => "Choose an amber deposit",
                    Command::FISH => "Choose a fishing ground",
                    _ => "Choose a leviathan",
                }),
            }
            return;
        }
        // As in the original, the left button gives orders too: with own
        // ships selected, a click where the pointer shows an order (the sea,
        // an enemy, a treasure, a deposit, a dock) carries it out; anywhere
        // else it selects.
        // With the modern mouse the left button only selects (orders go
        // with the right).
        if self.pending.is_none() && !self.settings.modern_mouse {
            use crate::cursor::Pointer as P;
            let (pointer, greyed) = self.pointer(pos);
            let order = matches!(
                pointer,
                P::Sea
                    | P::Attack
                    | P::Board
                    | P::Ram
                    | P::Repair
                    | P::Unload
                    | P::Load
                    | P::Treasure
                    | P::Amber
                    | P::Fish
                    | P::Whale
            );
            if order && !greyed {
                self.act(pointer, pos);
                return;
            }
        }
        if self.minimap_jump(pos) {
            self.minimap_drag = true;
            return;
        }
        self.minimap_drag = false;
        if self.view_world(pos).is_none() {
            return;
        }
        self.box_from = Some(pos);
        self.pending = None;
    }

    /// The pointer for the mouse at `pos` and whether it is greyed (the
    /// action cannot be done there): as the original (0x5175b0), by what
    /// the selected units would do to what is under it.
    fn default_pointer(&self, pos: PhysicalPosition<f64>) -> (crate::cursor::Pointer, bool) {
        use crate::cursor::Pointer as P;
        if self.video.is_some() || self.menu.is_some() || self.button_at(pos).is_some() {
            return (P::Standard, false);
        }
        // No viewing player: the pointer keeps mode 0 (0x5175b0 skips all
        // but the sprite's move, 0x517635 -> 0x517a16).
        if self.in_game && self.scene.human.is_none() {
            return (P::Standard, false);
        }
        // Selling, the sell pointer (0x441ba0 answers the sell mode 0x34
        // with gcursor mode 0x28, CUR_SELL_).
        if self.pending == Some(Command::SELL_ITEM) {
            return (P::Sell, false);
        }
        // Over a quick help link, the hypertext pointer (0x51728a).
        if self.tab == Tab::Help {
            let link = self
                .bar_point(pos)
                .and_then(|p| self.interface.panels.hit(&self.panel_view(), p));
            if let Some(PanelHit::HelpLink(_)) = link {
                return (P::Hypertext, false);
            }
        }
        if self.view_world(pos).is_none() {
            return (P::Standard, false);
        }
        let target = self
            .object_at(pos)
            .and_then(|id| self.scene.object_index(id).map(|i| &self.scene.objects[i]));
        let selected = self
            .selected
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i]);
        let own = |o: &crate::objects::MapObject| self.scene.is_own(o);
        let enemy = |o: &crate::objects::MapObject| {
            o.combat.is_some() && selected.is_some_and(|s| Scene::are_enemies(s, o)) && !own(o)
        };
        let resource = |o: &crate::objects::MapObject| o.deposit.map(|d| d.resource);
        let water = self
            .cell_at(pos)
            .is_some_and(|(x, y)| self.scene.nav.is_water(x, y));
        // A command waiting for its target.
        if let Some(cmd) = self.pending {
            let deposit = |r: Resource| target.and_then(resource) == Some(r);
            return match cmd {
                Command::MOVE => (P::Sea, !water),
                Command::FIRE => (P::Attack, !target.is_some_and(enemy)),
                Command::RAM => (
                    P::Ram,
                    !target.is_some_and(|o| enemy(o) && o.ship.is_some()),
                ),
                Command::BOARD => (
                    P::Board,
                    !target.is_some_and(|o| enemy(o) && o.ship.is_some()),
                ),
                Command::AMBER => (P::Amber, !deposit(Resource::Amber)),
                Command::FISH => (P::Fish, !deposit(Resource::Fish)),
                // The hunt command's own pointer (0x4dc630 case 0x1b: mode
                // 3, `CUR_GETW`); `CUR_WW` is for pointing at a leviathan.
                Command::WHALE => (P::GetWhale, !deposit(Resource::Leviathan)),
                Command::TREASURE => (
                    P::Treasure,
                    !target.is_some_and(|o| {
                        o.treasure.is_some()
                            || o.class == openconquest_formats::ObjectClass::HeroHouse
                    }),
                ),
                Command::REPAIR_SHIP | Command::REPAIR_BUILDING => {
                    (P::Repair, !target.is_some_and(own))
                }
                Command::UNLOAD => (
                    P::Unload,
                    !target.is_some_and(|o| {
                        own(o)
                            && matches!(
                                o.class,
                                openconquest_formats::ObjectClass::Building(
                                    economy::PORT | economy::SETTLEMENT | economy::TOWN
                                )
                            )
                    }),
                ),
                Command::LOAD => (
                    P::Load,
                    !target.is_some_and(|o| own(o) && is_load_target(&self.scene, o, selected)),
                ),
                // Over a site it may not go, the ban pointer (gcursor
                // 0x516ea0 mode 100, answered by 0x441ba0 to 0xc451).
                c if economy::building_of_command(c).is_some() => {
                    if self.site_at(pos).is_some_and(|s| s.1) {
                        (P::Build, false)
                    } else {
                        (P::Ban, false)
                    }
                }
                c if Spell::of_command(c).is_some() => (P::Magic, false),
                _ => (P::Behaviour, false),
            };
        }
        // What a right click would do with own ships selected.
        let ship = selected.filter(|s| own(s) && s.ship.is_some());
        if let Some(s) = ship {
            let merchant = s
                .ship
                .as_ref()
                .is_some_and(|sh| crate::objects::cargo_capacity(sh.kind) > 0.0);
            return match target {
                Some(o) if own(o) => (P::Own, false),
                Some(o) if enemy(o) => (P::Attack, !s.is_armed()),
                Some(o) if o.treasure.is_some() => (P::Treasure, false),
                Some(o) if o.class == openconquest_formats::ObjectClass::HeroHouse => {
                    (P::Treasure, false)
                }
                Some(o) if resource(o).is_some() && merchant => match resource(o).unwrap() {
                    Resource::Amber => (P::Amber, false),
                    Resource::Fish => (P::Fish, false),
                    Resource::Leviathan => (P::Whale, false),
                },
                _ if water => (P::Sea, false),
                _ => (P::Ban, false),
            };
        }
        match target {
            Some(o) if own(o) => (P::Own, false),
            Some(o) if o.combat.is_some() || o.deposit.is_some() => (P::Select, false),
            _ => (P::Standard, false),
        }
    }

    /// What the pointer is over, for the right button's stepping: an
    /// object, or a map cell.
    fn hover_key(&self, pos: PhysicalPosition<f64>) -> Option<HoverKey> {
        match self.object_at(pos) {
            Some(id) => Some(HoverKey::Object(id)),
            None => self.cell_at(pos).map(HoverKey::Cell),
        }
    }

    /// The pointers a click may take at `pos`: the one the original picks
    /// (0x5175b0) first, then the others possible there with own ships
    /// selected, in the order its right button steps through them (0x5180e0:
    /// own, sea, attack, board, ram, repair, ..., fish, amber, leviathan,
    /// treasure).
    fn pointer_modes(&self, pos: PhysicalPosition<f64>) -> Vec<(crate::cursor::Pointer, bool)> {
        use crate::cursor::Pointer as P;
        let first = self.default_pointer(pos);
        let mut modes = vec![first];
        if self.pending.is_some() {
            return modes;
        }
        let ship = self
            .selected
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i])
            .filter(|o| o.ship.is_some() && self.scene.is_own(o));
        let target = self
            .object_at(pos)
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i]);
        let (Some(s), Some(t)) = (ship, target) else {
            return modes;
        };
        let sh = s.ship.as_ref().unwrap();
        let enemy = t.combat.is_some() && Scene::are_enemies(s, t) && !self.scene.is_own(t);
        let mut more = Vec::new();
        if enemy {
            // Only what the ship can do to it (0x4e45c0): fire if armed;
            // board with marines aboard and ram, a warship at a ship
            // (0x4e3580, 0x4e3800). The first of them is the default.
            let warship = sh.kind >= 2;
            let options = [
                (P::Attack, s.is_armed()),
                (P::Board, warship && t.ship.is_some() && sh.crew.count() > 0),
                (P::Ram, warship && t.ship.is_some()),
            ];
            let open: Vec<_> = options
                .iter()
                .filter(|o| o.1)
                .map(|o| (o.0, false))
                .collect();
            if let Some(&first) = open.first() {
                if modes[0].0 == P::Attack {
                    modes[0] = first;
                }
            }
            more.extend(open);
        }
        let own_dock = self.scene.is_own(t)
            && t.class == openconquest_formats::ObjectClass::Building(economy::DOCK);
        if own_dock {
            more.push((P::Repair, false));
        }
        if self.scene.is_own(t) && is_load_target(&self.scene, t, Some(s)) && sh.kind < 10 {
            more.push((P::Load, false));
        }
        let laden = sh.cargo >= 1.0;
        let landing = self.scene.is_own(t)
            && matches!(
                t.class,
                openconquest_formats::ObjectClass::Building(
                    economy::PORT | economy::SETTLEMENT | economy::TOWN
                )
            );
        if laden && landing {
            more.push((P::Unload, false));
        }
        for m in more {
            if !modes.iter().any(|x| x.0 == m.0) {
                modes.push(m);
            }
        }
        modes
    }

    /// The pointer for the mouse at `pos` and whether it is greyed: the
    /// original's pick, or with its mouse controls the one the right button
    /// has stepped to.
    fn pointer(&self, pos: PhysicalPosition<f64>) -> (crate::cursor::Pointer, bool) {
        let modes = self.pointer_modes(pos);
        if self.settings.modern_mouse || modes.len() == 1 {
            return modes[0];
        }
        let (key, steps) = self.cycle.get();
        let steps = if key == self.hover_key(pos) { steps } else { 0 };
        modes[steps % modes.len()]
    }

    /// The original's right button: puts back what is carried, drops a
    /// command waiting for its target, or steps the pointer on to the next
    /// thing the selected ships could do there (0x5180e0).
    /// Whether a left press at `pos` waits to see if it becomes a drag: on
    /// the map, with something selected and no command waiting for its
    /// target.
    fn holds_press(&self, pos: PhysicalPosition<f64>) -> bool {
        let (_, minimap) = self.placement();
        let (x, y) = (pos.x as f32, pos.y as f32);
        self.selected.is_some()
            && self.pending.is_none()
            && self.menu.is_none()
            && self.view_world(pos).is_some()
            && (self.radar_shown() || !minimap.contains(x, y))
            && self.button_at(pos).is_none()
    }

    fn step_pointer(&mut self, pos: PhysicalPosition<f64>) {
        if self.pending.take().is_some() {
            self.status = None;
            return;
        }
        let key = self.hover_key(pos);
        let (was, steps) = self.cycle.get();
        let steps = if was == key { steps + 1 } else { 1 };
        self.cycle.set((key, steps));
    }

    /// Carries out pointer `pointer` at `pos` for the selected ships (the
    /// original's left button).
    fn act(&mut self, pointer: crate::cursor::Pointer, pos: PhysicalPosition<f64>) {
        use crate::cursor::Pointer as P;
        let target = self.object_at(pos);
        let result = match (pointer, target) {
            (P::Attack, Some(target)) => self.order_each(|ship| Order::Attack { ship, target }),
            (P::Board | P::Ram, Some(target)) => self.order_each(|ship| Order::Close {
                ship,
                target,
                board: pointer == P::Board,
            }),
            (P::Repair, Some(dock)) => self.order_each(|ship| Order::Repair { ship, dock }),
            (P::Unload, Some(port)) => self.order_each(|ship| Order::Unload { ship, port }),
            (P::Load, Some(target)) => self.order_each(|ship| Order::LoadObject { ship, target }),
            _ => {
                self.right_click(pos);
                Ok(())
            }
        };
        if let Err(e) = result {
            self.say(e);
        }
        self.cycle.set((None, 0));
    }

    /// Gives `order` for the player: at once in a single-player game, in a
    /// coming turn in a multiplayer one (where refusals show then).
    fn issue(&mut self, order: Order) -> Result<u32, String> {
        if let Some(net) = self.net.as_mut() {
            net.send(order);
            return Ok(0);
        }
        self.scene.apply(&order, &self.data, &mut self.caches)
    }

    /// Gives an order to each selected object: done if any of them took
    /// it, else the first refusal.
    fn order_each(&mut self, order: impl Fn(u32) -> Order) -> Result<(), String> {
        let mut result = Err("Nothing is selected".to_string());
        for (k, id) in self.selection().into_iter().enumerate() {
            match self.issue(order(id)) {
                Ok(_) => result = Ok(()),
                Err(e) if k == 0 => result = Err(e),
                Err(_) => {}
            }
        }
        result
    }

    /// The selected object and the ships selected with it.
    fn selection(&self) -> Vec<u32> {
        self.selected
            .iter()
            .chain(&self.group)
            .copied()
            .filter(|&id| self.scene.object_index(id).is_some())
            .collect()
    }

    /// Selects group `n` again (key 1 ... 0): those of its objects still
    /// there and still the player's.
    fn recall_group(&mut self, n: usize) {
        let ids: Vec<u32> = self.groups[n]
            .iter()
            .copied()
            .filter(|&id| {
                self.scene
                    .object_index(id)
                    .is_some_and(|i| self.scene.is_own(&self.scene.objects[i]))
            })
            .collect();
        if let Some((&first, rest)) = ids.split_first() {
            self.selected = Some(first);
            self.group = rest.to_vec();
            self.page = Page::Main;
        }
    }

    /// Selects every own ship of `id`'s kind within the view, `id` first.
    fn select_kind_in_view(&mut self, id: u32) {
        let Some(kind) = self
            .scene
            .object_index(id)
            .and_then(|i| self.scene.objects[i].ship.as_ref())
            .map(|s| s.kind)
        else {
            return;
        };
        let (screen, _) = self.placement();
        let (vw, vh) = self.camera.view_size(&screen);
        let (x0, y0) = (self.camera.x, self.camera.y);
        let mut all: Vec<u32> = vec![id];
        all.extend(
            self.scene
                .objects
                .iter()
                .filter(|o| {
                    o.id != id
                        && self.scene.is_own(o)
                        && o.ship.as_ref().is_some_and(|s| s.kind == kind)
                        && (x0..x0 + vw).contains(&o.x)
                        && (y0..y0 + vh).contains(&o.y)
                })
                .map(|o| o.id),
        );
        if self.selected != Some(id) {
            self.page = Page::Main;
        }
        self.selected = Some(id);
        self.group = all.into_iter().skip(1).collect();
    }

    /// Whether `id` is one of the player's ships.
    fn own_ship(&self, id: u32) -> bool {
        self.scene.object_index(id).is_some_and(|i| {
            let o = &self.scene.objects[i];
            o.ship.is_some() && self.scene.is_own(o)
        })
    }

    /// Ends a left click or drag in the world view: a click selects what
    /// is under it (with Shift, adds or removes an own ship, 0xc455; with
    /// Ctrl, its group), as the original does. With the modern mouse only,
    /// a drag selects every own ship inside the box (with Shift, added to
    /// the selection): the original selects by clicks and groups only.
    fn finish_select(&mut self, from: PhysicalPosition<f64>, to: PhysicalPosition<f64>) {
        if self.pending == Some(crate::commands::CONVOY) {
            let order = match (self.world_at(from), self.world_at(to)) {
                (Some(a), Some(b)) => self.order_each(|ship| Order::Convoy { ship, a, b }),
                _ => Err("Drag a rectangle around the ship to escort".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(k) = self
            .pending
            .and_then(|c| crate::commands::GATHER_ZONES.iter().position(|&z| z == c))
        {
            let resource = [Resource::Amber, Resource::Fish, Resource::Leviathan][k];
            let order = match (self.world_at(from), self.world_at(to)) {
                (Some(a), Some(b)) => self.order_each(|ship| Order::GatherZone {
                    ship,
                    a,
                    b,
                    resource,
                }),
                _ => Err("Drag a rectangle around the zone".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(cmd @ (crate::commands::GUARD_ZONE | crate::commands::ATTACK_ZONE)) =
            self.pending
        {
            let attack = cmd == crate::commands::ATTACK_ZONE;
            let order = match (self.world_at(from), self.world_at(to)) {
                (Some(a), Some(b)) => self.order_each(|ship| Order::Zone { ship, a, b, attack }),
                _ => Err("Drag a rectangle around the zone".into()),
            };
            match order {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                }
                Err(e) => self.say(e),
            }
            return;
        }
        let shift = self.modifiers.shift_key();
        let dragged = self.settings.modern_mouse
            && (from.x - to.x).abs() + (from.y - to.y).abs() > BOX_THRESHOLD;
        // With the modern mouse a double click on an own ship selects
        // every own ship of its kind in view.
        if self.settings.modern_mouse && !shift && !dragged {
            let now = Instant::now();
            let hit = self.object_at(to).filter(|&id| self.own_ship(id));
            let double = hit.is_some()
                && self.last_click.is_some_and(|(id, at)| {
                    Some(id) == hit && now.duration_since(at) < DOUBLE_CLICK
                });
            self.last_click = hit.map(|id| (id, now));
            if double {
                if let Some(id) = hit {
                    self.select_kind_in_view(id);
                    self.last_click = None;
                    return;
                }
            }
        }
        // Ctrl-click on an own ship selects its group (0x5180e0 with
        // MK_CONTROL: message 0xc456, 0x49ecb0, the ship's fleet +0x5c).
        let ctrl = self.modifiers.control_key() || self.modifiers.super_key();
        if ctrl && !shift && !dragged {
            if let Some(id) = self.object_at(to).filter(|&id| self.own_ship(id)) {
                if let Some(n) = self.groups.iter().position(|g| g.contains(&id)) {
                    self.recall_group(n);
                    return;
                }
            }
        }
        let mut chosen: Vec<u32> = if dragged {
            let (a, b) = (self.view_world_any(from), self.view_world_any(to));
            let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
            let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
            self.scene
                .objects
                .iter()
                .filter(|o| o.ship.is_some() && self.scene.is_own(o))
                .filter(|o| (x0..=x1).contains(&o.x) && (y0..=y1).contains(&o.y))
                .map(|o| o.id)
                .collect()
        } else {
            self.object_at(to).into_iter().collect()
        };
        if shift && !dragged {
            // Shift-click adds or removes one own ship.
            if let Some(&id) = chosen.first().filter(|&&id| self.own_ship(id)) {
                let mut all = self.selection();
                if let Some(k) = all.iter().position(|&s| s == id) {
                    all.remove(k);
                } else {
                    all.retain(|&s| self.own_ship(s));
                    all.push(id);
                }
                chosen = all;
            } else {
                return;
            }
        } else if shift {
            let mut all: Vec<u32> = self
                .selection()
                .into_iter()
                .filter(|&s| self.own_ship(s))
                .collect();
            for id in chosen {
                if !all.contains(&id) {
                    all.push(id);
                }
            }
            chosen = all;
        } else if dragged && chosen.is_empty() {
            return;
        }
        let first = chosen.first().copied();
        if first != self.selected {
            self.page = Page::Main;
        }
        self.selected = first;
        self.group = chosen.into_iter().skip(1).collect();
    }

    /// The world point under a window position, in the view or on the
    /// minimap.
    fn world_at(&self, pos: PhysicalPosition<f64>) -> Option<(f32, f32)> {
        let (_, minimap) = self.placement();
        let (sx, sy) = (pos.x as f32, pos.y as f32);
        if let Some(w) = self.view_world(pos) {
            Some(w)
        } else if minimap.contains(sx, sy) && !self.radar_shown() {
            Some(minimap.to_world(sx, sy))
        } else {
            None
        }
    }

    /// The object under a window position in the world view.
    fn object_at(&self, pos: PhysicalPosition<f64>) -> Option<u32> {
        let (wx, wy) = self.view_world(pos)?;
        hit_test(&self.scene, wx, wy, self.time_ms)
    }

    fn right_click(&mut self, pos: PhysicalPosition<f64>) {
        // Ships of a group spread out around the point they are sent to.
        for (k, id) in self.selection().into_iter().enumerate() {
            self.right_click_for(id, k, pos);
        }
    }

    /// Carries out a right click for selected object `id`, the `k`-th of
    /// the selection.
    fn right_click_for(&mut self, id: u32, k: usize, pos: PhysicalPosition<f64>) {
        // Right-clicking a deposit with a merchant selected starts gathering.
        if let Some(target) = self.object_at(pos) {
            // Treasures and heroes' houses: go and take them.
            let takeable = self.scene.object_index(target).is_some_and(|t| {
                matches!(
                    self.scene.objects[t].class,
                    openconquest_formats::ObjectClass::Treasure
                        | openconquest_formats::ObjectClass::HeroHouse
                )
            });
            if takeable && self.own_ship(id) {
                match self.issue(Order::Take { ship: id, target }) {
                    Ok(_) if k == 0 => self.confirm(pos),
                    Ok(_) => {}
                    Err(e) => self.say(e),
                }
                return;
            }
            let is_deposit = self
                .scene
                .object_index(target)
                .is_some_and(|i| self.scene.objects[i].deposit.is_some());
            let own_merchant = self.scene.object_index(id).is_some_and(|i| {
                let o = &self.scene.objects[i];
                self.scene.is_own(o)
                    && o.ship
                        .as_ref()
                        .is_some_and(|s| crate::objects::cargo_capacity(s.kind) > 0.0)
            });
            if is_deposit && own_merchant {
                if let Err(e) = self.issue(Order::Gather {
                    ship: id,
                    deposit: target,
                }) {
                    self.say(e);
                }
                return;
            }
            // Right-clicking an enemy ship or building with an own warship attacks it.
            let own_warship = self.scene.object_index(id).is_some_and(|i| {
                let o = &self.scene.objects[i];
                self.scene.is_own(o) && o.ship.is_some() && o.is_armed()
            });
            let enemy_ship = match (self.scene.object_index(id), self.scene.object_index(target)) {
                (Some(a), Some(b)) => {
                    self.scene.objects[b].combat.is_some()
                        && Scene::are_enemies(&self.scene.objects[a], &self.scene.objects[b])
                }
                _ => false,
            };
            if own_warship && enemy_ship {
                if let Err(e) = self.issue(Order::Attack { ship: id, target }) {
                    self.say(e);
                }
                return;
            }
            // Unarmed warships (fire ships, fire rammers) ram instead.
            let own_rammer = self.scene.object_index(id).is_some_and(|i| {
                let o = &self.scene.objects[i];
                self.scene.is_own(o)
                    && o.ship.as_ref().is_some_and(|s| s.kind >= 2)
                    && !o.is_armed()
            });
            let target_ship = self
                .scene
                .object_index(target)
                .is_some_and(|t| self.scene.objects[t].ship.is_some());
            if own_rammer && enemy_ship && target_ship {
                if let Err(e) = self.issue(Order::Close {
                    ship: id,
                    target,
                    board: false,
                }) {
                    self.say(e);
                }
                return;
            }
        }
        let is_ship = self.scene.object_index(id).is_some_and(|i| {
            let obj = &self.scene.objects[i];
            obj.ship.is_some() && self.scene.is_own(obj)
        });
        let Some(target) = self.world_at(pos) else {
            return;
        };
        let (dx, dy) = group_offset(k);
        let target = (target.0 + dx, target.1 + dy);
        if !is_ship {
            return;
        }
        let (x, y) = target;
        let order = if self.modifiers.shift_key() {
            Order::Waypoint { ship: id, x, y }
        } else {
            Order::Move { ship: id, x, y }
        };
        if self.issue(order).is_err() {
            self.say("The ship cannot sail there.");
        } else if k == 0 {
            self.sent = Some((x - dx, y - dy, self.time_ms));
        }
    }

    /// The green tick confirming an order at the clicked point (gcursor.cpp
    /// 0x5180e0: after a move, taking a treasure, a behaviour's point, and
    /// loading or unloading; never after gathering or attacking).
    fn confirm(&mut self, pos: PhysicalPosition<f64>) {
        if let Some((x, y)) = self.world_at(pos) {
            self.sent = Some((x, y, self.time_ms));
        }
    }

    /// Puts video `name` in line to play, if it is installed.
    fn queue_video(&mut self, name: &str) {
        if let Some(path) = crate::video::find(self.data.root(), name) {
            self.videos.push_back(path);
        }
    }

    /// Plays the videos in line; true while one is showing.
    fn run_videos(&mut self, dt: f32) -> bool {
        if let Some(v) = self.video.as_mut() {
            if v.advance(dt) {
                return true;
            }
            self.video = None;
        }
        while let Some(path) = self.videos.pop_front() {
            let volume = self
                .audio
                .as_ref()
                .map_or(0.0, |a| if a.muted { 0.0 } else { a.volume });
            match crate::video::Player::open(&path, self.audio.as_ref().map(|a| &a.handle), volume)
            {
                Ok(v) => {
                    self.video = Some(v);
                    // The music waits, and the menus' music stops.
                    if let Some(m) = &self.music {
                        m.set_volume(0.0);
                    }
                    self.menu_music = None;
                    self.menu_sample = None;
                    self.ambience = None;
                    return true;
                }
                Err(e) => eprintln!("video: {e:#}"),
            }
        }
        false
    }

    /// Skips the video playing, or with `all` every one in line.
    fn skip_video(&mut self, all: bool) {
        self.video = None;
        if all {
            self.videos.clear();
        }
        if self.videos.is_empty() {
            self.restore_music();
        }
    }

    /// The music's volume as set (after a video).
    fn restore_music(&self) {
        if let (Some(m), Some(a)) = (&self.music, &self.audio) {
            m.set_volume(if a.muted { 0.0 } else { self.music_volume });
        }
    }

    fn update(&mut self, dt: f32) {
        // The world waits while a video plays.
        let was_playing = self.video.is_some();
        if self.run_videos(dt) {
            return;
        }
        if was_playing {
            self.restore_music();
        }
        // The game waits while the briefing is read.
        if let (Some(_), None) = (self.scene.outcome, self.outcome_at) {
            self.outcome_at = Some(self.scene.clock);
        }
        // The end screen follows a few seconds after the mission is decided.
        if let (Some(outcome), Some(at), None) = (self.scene.outcome, self.outcome_at, self.menu) {
            if self.scene.clock > at + 3.0 && self.menus.is_some() {
                self.open_outcome(outcome);
                self.in_game = false;
                self.bar_hidden = false;
            }
        }
        // A Custom game with no human player over: command 0x100, the
        // main menu with no end screen (0x448610).
        if self.scene.over && self.in_game {
            self.leave_game();
        }
        if self.menu == Some(MenuState::Waiting) {
            self.run_lobby();
        }
        self.scene.trails = self.settings.config.graphic_trails;
        crate::render::LIGHTING.store(
            self.settings.config.lighting,
            std::sync::atomic::Ordering::Relaxed,
        );
        if self.net.is_some() {
            // Multiplayer games run at one speed and do not pause, menus
            // open or not.
            self.run_turns(dt);
        } else if !self.briefing && !self.paused && self.menu.is_none() && self.ask.is_none() {
            self.scene.update(
                dt * self.settings.config.speed(),
                &self.data,
                &mut self.caches,
            );
        }
        self.play_scene_sounds();
        self.choose_music();
        self.choose_ambience();
        if let Some(m) = self.music.as_mut() {
            m.keep_playing();
        }
        // The game's messages join the lines over the view (chat.cpp
        // 0x4ff390); the oldest goes every "message speed" seconds, or at
        // once while there are more than "message lines" (0x4ff4c0).
        self.chat.append(&mut self.scene.messages);
        let lines = self.settings.config.chat_lines.max(1) as usize;
        let every = Duration::from_secs(self.settings.config.chat_speed.max(1) as u64);
        if self.chat.len() > lines || self.chat_since.elapsed() >= every {
            self.chat_since = Instant::now();
            if !self.chat.is_empty() {
                self.chat.remove(0);
            }
        }
        while self.chat.len() > lines {
            self.chat.remove(0);
        }
        let window = self.window_size();
        let (screen, _) = self.placement();
        let map = self.scene.map_size();
        if self.center_pending {
            self.center_pending = false;
            match self.scene.start_corner() {
                Some((x, y)) => {
                    self.camera.x = x;
                    self.camera.y = y;
                    self.camera.clamp(&screen, map);
                }
                // No viewing player: the view stays at scroll (0, 0)
                // (0x50f910 message 0x101 centres it, 0xcba, only when
                // [game+0x3212] != 0xff, 0x510148).
                None if self.scene.human.is_none() => {
                    self.camera.x = 0.0;
                    self.camera.y = 0.0;
                    self.camera.clamp(&screen, map);
                }
                None => {
                    let (x, y) = self.scene.start_view();
                    self.camera.center_on(x, y, &screen, map);
                }
            }
        }
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        let held = |k: &[KeyCode]| k.iter().any(|k| self.keys.contains(k));
        let mut keyed = false;
        if held(&[KeyCode::ArrowLeft]) {
            dx -= 1.0;
        }
        if held(&[KeyCode::ArrowRight]) {
            dx += 1.0;
        }
        if held(&[KeyCode::ArrowUp]) {
            dy -= 1.0;
        }
        if held(&[KeyCode::ArrowDown]) {
            dy += 1.0;
        }
        keyed |= dx != 0.0 || dy != 0.0;
        let edge = self.options.edge_scroll || self.is_fullscreen();
        if let (true, true, Some(c), None) = (edge, self.focused, self.cursor, self.drag_from) {
            let (w, h) = (window.0 as f64, window.1 as f64);
            if c.x < EDGE_MARGIN {
                dx -= 1.0;
            } else if c.x > w - EDGE_MARGIN {
                dx += 1.0;
            }
            if c.y < EDGE_MARGIN {
                dy -= 1.0;
            } else if c.y > h - EDGE_MARGIN {
                dy += 1.0;
            }
        }
        // Speed is defined in original pixels so scrolling feels the same at
        // any resolution.
        let speed = SCROLL_SPEED
            * self.settings.config.scroll(keyed)
            * self.camera.policy.base_scale(window.1)
            * dt;
        if dx != 0.0 || dy != 0.0 {
            self.camera
                .scroll(direction(dx) * speed, direction(dy) * speed, &screen, map);
        }
        self.camera.clamp(&screen, map);
        self.scroll_radar();
    }

    /// Draws the front end into `frame`, fading from screen to screen
    /// (`fade`); false when the game is to be drawn instead. The screen or
    /// game left fades out to black (0x4ef350(50, 5), over the game's last
    /// frame too) and a new screen, drawn under the black, fades in to its
    /// backdrop's palette (message 0x102, 0x4ef530(game, backdrop, 50,
    /// 5)). Into the game started by Start (0x101, 0x50f910) `LOADING`
    /// fades in (0x4ef420) and out, then the game fades in from its idle
    /// (`keep_game_frame`); into it otherwise only the fade out.
    fn draw_front_end(&mut self, frame: &mut Frame) -> bool {
        use crate::fade::{level, Level, Shade, Way};
        let size = (frame.width, frame.height);
        let screen = self
            .menu
            .filter(|_| self.menus.is_some())
            .map(|s| std::mem::discriminant(&s));
        if self.fade.is_none() {
            match self.shown.take() {
                Some(last) if last.screen != screen => {
                    if last.size == size && last.pixels.len() == (size.0 * size.1) as usize {
                        let then = if screen.is_some() {
                            AfterFade::ScreenIn
                        } else if std::mem::take(&mut self.entering) {
                            if std::mem::take(&mut self.entering_load) {
                                AfterFade::Black
                            } else {
                                AfterFade::LoadingIn
                            }
                        } else {
                            AfterFade::Nothing
                        };
                        self.fade = Some(Fading {
                            way: Way::Out,
                            since: Instant::now(),
                            pixels: last.pixels,
                            size,
                            then,
                        });
                    } else {
                        self.fade_in(size);
                    }
                }
                None => self.fade_in(size),
                last => self.shown = last,
            }
        }
        // A fade of another size is dropped (Quit's ends at once).
        if self.fade.as_ref().is_some_and(|f| f.size != size)
            && self.fade.take().is_some_and(|f| f.then == AfterFade::Exit)
        {
            self.quit = true;
        }
        if let Some(f) = &self.fade {
            if level(f.way, f.since.elapsed().as_millis() as u64) == Level::Done {
                match f.then {
                    AfterFade::ScreenIn => self.fade_in(size),
                    AfterFade::LoadingIn => self.loading_fade(Way::In, size, AfterFade::LoadingOut),
                    AfterFade::LoadingOut => self.loading_fade(Way::Out, size, AfterFade::GameIn),
                    AfterFade::GameIn => {
                        self.fade = None;
                        self.game_fade = Some((0, Instant::now()));
                    }
                    AfterFade::Black => {
                        self.fade = Some(Fading {
                            way: Way::Out,
                            since: Instant::now(),
                            pixels: vec![0; (size.0 * size.1) as usize],
                            size,
                            then: AfterFade::LoadingIn,
                        });
                    }
                    // Black until the program ends.
                    AfterFade::Exit => self.quit = true,
                    AfterFade::Nothing => self.fade = None,
                }
            }
        }
        if let Some(f) = &self.fade {
            let percent = match (f.way, level(f.way, f.since.elapsed().as_millis() as u64)) {
                (Way::Out, Level::Before) => {
                    frame.pixels.copy_from_slice(&f.pixels);
                    return true;
                }
                (_, Level::Percent(p)) => Some(p),
                _ => None,
            };
            Shade::new(percent).frame(&f.pixels, frame.pixels);
            return true;
        }
        let (Some(state), Some(menus)) = (self.menu, &self.menus) else {
            // The game: its frame is kept once drawn (`keep_game_frame`).
            let pixels = self.shown.take().map(|s| s.pixels).unwrap_or_default();
            self.shown = Some(Shown {
                screen: None,
                pixels,
                size,
            });
            return false;
        };
        let page = self.menu_page(state);
        // While the kind pop-up is open the screen's widgets take no
        // pointer (Sinput message 0x1a).
        let hover = self
            .cursor
            .filter(|_| self.kind_popup.is_none())
            .map(|c| Menus::to_menu(size, c.x as f32, c.y as f32));
        menus.draw(frame, &page.view(hover, self.menu_held));
        let mut pixels = self.shown.take().map(|s| s.pixels).unwrap_or_default();
        pixels.clear();
        pixels.extend_from_slice(frame.pixels);
        self.shown = Some(Shown {
            screen,
            pixels,
            size,
        });
        true
    }

    /// The picture shown now fades out (0x4ef350(50, 5)), then `then`: a
    /// saved game loaded in the game, or Quit. With no picture shown yet
    /// (nothing drawn), `then` at once.
    fn fade_out_then(&mut self, then: AfterFade) {
        let shown = self
            .shown
            .take()
            .filter(|s| !s.pixels.is_empty() && s.pixels.len() == (s.size.0 * s.size.1) as usize);
        match shown {
            Some(s) => {
                self.fade = Some(Fading {
                    way: crate::fade::Way::Out,
                    since: Instant::now(),
                    pixels: s.pixels,
                    size: s.size,
                    then,
                });
                // The game's frame is kept again once it is drawn.
                self.shown = Some(Shown {
                    screen: s.screen,
                    pixels: Vec::new(),
                    size: s.size,
                });
            }
            None => self.quit |= then == AfterFade::Exit,
        }
    }

    /// Quit game (the main menu's, 0x5310a0 message 0x164, and Alt+Q's yes,
    /// 0x4f0: command 0x131): the picture fades out (0x4ef350(50, 5)),
    /// then the program ends; no pointer hidden, no fade in.
    fn quit_game(&mut self) {
        self.fade_out_then(AfterFade::Exit);
    }

    /// A saved game built in place of the one under way (command 0x102:
    /// the file panel's load, Restart, Ctrl+O): the game's picture fades
    /// out, then black, `LOADING` in and out and the game in (gamesys
    /// 0x101). From a front-end screen the screen change does it
    /// (`entering_load`).
    fn fade_into_load(&mut self) {
        if self.menu.is_none() {
            self.fade_out_then(AfterFade::Black);
        } else {
            self.entering = true;
            self.entering_load = true;
        }
    }

    /// `LOADING` fading `way`, drawn into a picture of `size`, then `then`.
    fn loading_fade(&mut self, way: crate::fade::Way, size: (u32, u32), then: AfterFade) {
        let mut pixels = vec![0u32; (size.0 * size.1) as usize];
        if let Some(img) = self.menus.as_ref().and_then(|m| m.loading.as_ref()) {
            let (s, ox, oy) = Menus::placement(size);
            let mut frame = Frame {
                pixels: &mut pixels,
                width: size.0,
                height: size.1,
            };
            crate::render::blit_scaled(&mut frame, img, (0, 0, img.width, img.height), ox, oy, s);
        }
        self.fade = Some(Fading {
            way,
            since: Instant::now(),
            pixels,
            size,
            then,
        });
    }

    /// The game drawn into `frame`: while it fades in (0x50f740), a step
    /// each frame at least 1 ms after the last, step i at 4i percent, then
    /// as it is; and the frame kept, for the fade out of it into the front
    /// end (the dispatcher's 0x4ef350 over the game's last frame).
    fn keep_game_frame(&mut self, frame: &mut Frame) {
        use crate::fade::{game_percent, Shade, GAME_STEPS, GAME_STEP_MS};
        if let Some((steps, last)) = self.game_fade {
            let now = Instant::now();
            let due = steps == 0 || now.duration_since(last).as_millis() as u64 >= GAME_STEP_MS;
            let steps = if due { steps + 1 } else { steps };
            if steps > GAME_STEPS {
                self.game_fade = None;
            } else {
                self.game_fade = Some((steps, if due { now } else { last }));
                let from = frame.pixels.to_vec();
                Shade::new(Some(game_percent(steps - 1))).frame(&from, frame.pixels);
            }
        }
        if let Some(s) = self.shown.as_mut().filter(|s| s.screen.is_none()) {
            s.pixels.clear();
            s.pixels.extend_from_slice(frame.pixels);
            s.size = (frame.width, frame.height);
        }
    }

    /// The front-end screen shown now starts fading in, drawn once (it
    /// paints under the black palette before message 0x102).
    fn fade_in(&mut self, size: (u32, u32)) {
        self.fade = None;
        let (Some(state), Some(menus)) = (self.menu, &self.menus) else {
            return;
        };
        let page = self.menu_page(state);
        let mut pixels = vec![0u32; (size.0 * size.1) as usize];
        let hover = self
            .cursor
            .map(|c| Menus::to_menu(size, c.x as f32, c.y as f32));
        menus.draw(
            &mut Frame {
                pixels: &mut pixels,
                width: size.0,
                height: size.1,
            },
            &page.view(hover, self.menu_held),
        );
        self.fade = Some(Fading {
            way: crate::fade::Way::In,
            since: Instant::now(),
            pixels,
            size,
            then: AfterFade::Nothing,
        });
    }

    fn redraw(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.time_ms = (now - self.started).as_millis() as u64;
        // A fade holds everything up but the sound (it busy-waits).
        if self.fade.is_none() {
            self.update(dt);
            self.menu_repeat_step();
            self.kind_popup_repeat();
            self.load_due_preview();
            self.poll_menu_keys();
        } else {
            self.choose_music();
        }
        let (screen, _) = self.placement();
        let (vw, vh) = self.camera.view_size(&screen);
        let t = openconquest_formats::map::TILE_SIZE as f32;
        self.scene.follow_view(
            (
                (self.camera.x / t).floor() as i32,
                (self.camera.y / t).floor() as i32,
            ),
            ((vw / t).ceil() as i32, (vh / t).ceil() as i32),
        );

        let window = self.window_size();
        self.drawn_size = window;
        let Some(mut gfx) = self.gfx.take() else {
            return Ok(());
        };
        let result = (|| -> Result<()> {
            let (w, h) = (
                NonZeroU32::new(window.0).unwrap(),
                NonZeroU32::new(window.1).unwrap(),
            );
            gfx.surface
                .resize(w, h)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let mut buffer = gfx
                .surface
                .buffer_mut()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let mut frame = Frame {
                pixels: &mut buffer,
                width: window.0,
                height: window.1,
            };
            if let Some(v) = &self.video {
                v.draw(&mut frame);
                self.shown = None;
                self.fade = None;
            } else if self.draw_front_end(&mut frame) {
            } else {
                let site = self.cursor.and_then(|c| self.site_at(c));
                // The building as it would stand there.
                let at = self
                    .cursor
                    .and_then(|c| self.site_facing(c))
                    .and_then(|(cell, class, facing)| facing.map(|f| (cell, class, f)));
                if self.site_shown.as_ref().map(|p| p.0) != at {
                    self.site_shown = at.and_then(|key @ (cell, class, f)| {
                        self.scene
                            .site_preview(class, cell.0, cell.1, f, &self.data, &mut self.caches)
                            .map(|o| (key, o))
                    });
                }
                // Newly selected objects start their frame's zoom.
                let now = self.time_ms;
                let chosen: Vec<u32> = self.selected.iter().chain(&self.group).copied().collect();
                let gone: Vec<u32> = self
                    .selected_since
                    .keys()
                    .filter(|id| !chosen.contains(id))
                    .copied()
                    .collect();
                for id in gone {
                    self.selected_since.remove(&id);
                    self.deselected.insert(id, now);
                }
                let span = crate::render::SELECTION_FRAMES as u64 * crate::clock::TICK_MS;
                self.deselected
                    .retain(|id, t| !chosen.contains(id) && now.saturating_sub(*t) < span);
                for id in chosen {
                    self.selected_since.entry(id).or_insert(now);
                }
                GameView {
                    site,
                    site_preview: self.site_shown.as_ref().map(|p| &p.1),
                    ..self.view()
                }
                .draw(&mut frame);
                self.keep_game_frame(&mut frame);
            }
            // The pointer is hidden while the palette fades (0x55ed40
            // before a fade out; 0x516980 shows it again after the fade
            // in).
            let fading = self.fade.is_some() || self.game_fade.is_some();
            if let Some(c) = self.cursor.filter(|_| !fading) {
                let (pointer, greyed) = self.pointer(c);
                let scale = self.camera.policy.base_scale(window.1);
                self.pointers.draw(
                    &mut frame,
                    &self.data,
                    pointer,
                    greyed,
                    c.x as f32,
                    c.y as f32,
                    scale,
                    self.time_ms,
                );
            }
            buffer.present().map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(())
        })();
        self.gfx = Some(gfx);
        result
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, e: anyhow::Error) {
        self.error = Some(e);
        event_loop.exit();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        let (w, h) = self.options.window_size;
        let mut attrs = Window::default_attributes()
            .with_title("OpenConquest")
            .with_inner_size(LogicalSize::new(w, h))
            .with_min_inner_size(LogicalSize::new(400, 300));
        if self.options.fullscreen {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let result = (|| -> Result<Gfx> {
            let window = Rc::new(event_loop.create_window(attrs)?);
            // The game draws its own pointers over the window.
            if self.pointers.available(&self.data) {
                window.set_cursor_visible(false);
            }
            let context =
                softbuffer::Context::new(window.clone()).map_err(|e| anyhow::anyhow!("{e}"))?;
            let surface = softbuffer::Surface::new(&context, window.clone())
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(Gfx {
                window,
                surface,
                _context: context,
            })
        })();
        match result {
            Ok(gfx) => {
                self.gfx = Some(gfx);
                self.update_title();
            }
            Err(e) => self.fail(event_loop, e),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Focused(f) => {
                self.focused = f;
                if !f {
                    self.keys.clear();
                }
            }
            WindowEvent::Resized(_) => {
                let (screen, _) = self.placement();
                self.camera.clamp(&screen, self.scene.map_size());
            }
            WindowEvent::ModifiersChanged(m) => {
                self.modifiers = m.state();
                // macOS sends no key release for keys let go while Command
                // is held: forget held keys rather than scroll for ever.
                if self.modifiers.super_key() {
                    self.keys.clear();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                if event.state == ElementState::Released {
                    self.keys.remove(&code);
                    return;
                }
                // (See ModifiersChanged: with Command held, key releases
                // never come.)
                if !self.modifiers.super_key() {
                    self.keys.insert(code);
                }
                if event.repeat {
                    if let Some(c) = crate::sinput::scan_code(code) {
                        self.key_repeats.push(c);
                    }
                }
                // A fade waits for nothing (0x54a230 and 0x54a460 run
                // without the message loop).
                if self.fade.is_some() {
                    return;
                }
                // A front-end name edit takes what is typed, repeats too.
                if self.video.is_none()
                    && self.menus.is_some()
                    && self.edit_typed(code, event.text.as_deref().unwrap_or(""))
                {
                    return;
                }
                // A question takes Y or N, a notice Enter (its buttons'
                // keys, scan codes 0x15, 0x31, 0x1c), and nothing else.
                if let Some(ask) = self.ask {
                    match code {
                        KeyCode::KeyY if ask.yes_no() => self.answer(true),
                        KeyCode::KeyN if ask.yes_no() => self.answer(false),
                        KeyCode::Enter | KeyCode::NumpadEnter if !ask.yes_no() => self.answer(true),
                        _ => {}
                    }
                    return;
                }
                // Alt+Q leaves the game, Alt+X goes to the main menu, each
                // asking first (accelerators 0x4ef, 0x4f5; 0x511c00).
                if self.menu.is_none() && self.modifiers.alt_key() {
                    match code {
                        KeyCode::KeyQ => {
                            self.ask = Some(Ask::Quit);
                            return;
                        }
                        KeyCode::KeyX => {
                            self.ask = Some(Ask::MainMenu);
                            return;
                        }
                        _ => {}
                    }
                }
                if event.repeat {
                    return;
                }
                // Any key skips a video; Esc skips them all.
                if self.video.is_some() {
                    self.skip_video(code == KeyCode::Escape);
                    return;
                }
                // Chat in a multiplayer game: Enter opens a line, Enter
                // again sends it, Esc drops it.
                if self.net.is_some() && self.menu.is_none() {
                    if let Some(line) = self.chat_line.as_mut() {
                        match code {
                            KeyCode::Enter | KeyCode::NumpadEnter => {
                                let text = self.chat_line.take().unwrap_or_default();
                                if !text.trim().is_empty() {
                                    if let Some(net) = self.net.as_mut() {
                                        net.chat(text.trim());
                                    }
                                    self.say(format!("You: {}", text.trim()));
                                } else {
                                    self.status = None;
                                }
                            }
                            KeyCode::Escape => {
                                self.chat_line = None;
                                self.status = None;
                            }
                            KeyCode::Backspace => {
                                line.pop();
                                let shown = format!("Say: {line}_");
                                self.say(shown);
                            }
                            _ => {
                                if let Some(t) = event.text.as_deref() {
                                    if line.len() < 200 {
                                        line.extend(t.chars().filter(|c| !c.is_control()));
                                    }
                                }
                                let shown = format!("Say: {line}_");
                                self.say(shown);
                            }
                        }
                        // Typing is not a command.
                        self.keys.remove(&code);
                        return;
                    }
                    if matches!(code, KeyCode::Enter | KeyCode::NumpadEnter)
                        && !self.modifiers.alt_key()
                    {
                        self.chat_line = Some(String::new());
                        self.say("Say: _");
                        return;
                    }
                }
                // The file panel's name field takes the typing while its
                // SAVE bookmark is open; Enter saves.
                let command = self.modifiers.control_key() || self.modifiers.super_key();
                if self.menu.is_none()
                    && !self.bar_hidden
                    && self.tab == Tab::File
                    && self.file_mode == FileMode::Save
                    && !command
                {
                    let typed: String = event
                        .text
                        .as_deref()
                        .unwrap_or("")
                        .chars()
                        .filter(|c| !c.is_control())
                        .collect();
                    match code {
                        KeyCode::Backspace => {
                            self.save_name.pop();
                        }
                        KeyCode::Enter | KeyCode::NumpadEnter => self.file_action(),
                        _ if self.save_name.chars().count() < SAVE_NAME_MAX => {
                            self.save_name.push_str(&typed);
                        }
                        _ => {}
                    }
                    if !typed.is_empty()
                        || matches!(
                            code,
                            KeyCode::Backspace | KeyCode::Enter | KeyCode::NumpadEnter
                        )
                    {
                        self.keys.remove(&code);
                        return;
                    }
                }
                if let (Some(MenuState::Multiplayer(_)), Some(_)) = (self.menu, &self.menus) {
                    if code == KeyCode::Backspace {
                        self.join_address.pop();
                        return;
                    }
                    let typed = event.text.as_deref().unwrap_or("");
                    if !typed.is_empty()
                        && typed
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-'))
                        && self.join_address.len() < 60
                    {
                        self.join_address.push_str(typed);
                        return;
                    }
                }
                // The front end's keys are its screens' bindings, polled
                // each frame (`poll_menu_keys`); Esc goes back only on the
                // port's own multiplayer screens.
                if let (Some(state), Some(_)) = (self.menu, &self.menus) {
                    if code == KeyCode::Escape
                        && matches!(state, MenuState::Multiplayer(_) | MenuState::Waiting)
                    {
                        self.menu_back();
                    } else if code == KeyCode::F11 {
                        self.toggle_fullscreen();
                    }
                    return;
                }
                // Alt+B: the briefing again (this port's; B is a command
                // key).
                if code == KeyCode::KeyB && self.modifiers.alt_key() {
                    self.briefing = !self.briefing;
                    return;
                }
                if self.briefing
                    && matches!(
                        code,
                        KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space | KeyCode::Escape
                    )
                {
                    self.briefing = false;
                    return;
                }
                let (screen, _) = self.placement();
                let map = self.scene.map_size();
                let center = (
                    screen.x as f32 + screen.w as f32 / 2.0,
                    screen.y as f32 + screen.h as f32 / 2.0,
                );
                let command = self.modifiers.control_key() || self.modifiers.super_key();
                if let Some(n) = digit_index(code) {
                    if command {
                        self.groups[n] = self.selection();
                        if !self.groups[n].is_empty() {
                            self.say(format!("Group {} set", (n + 1) % 10));
                        }
                    } else {
                        self.recall_group(n);
                    }
                    return;
                }
                if let Some(n) = function_index(code) {
                    if self.modifiers.shift_key() {
                        let (vw, vh) = self.camera.view_size(&screen);
                        self.places[n] = Some((self.camera.x + vw / 2.0, self.camera.y + vh / 2.0));
                    } else if let Some((x, y)) = self.places[n] {
                        self.camera.center_on(x, y, &screen, map);
                    }
                    return;
                }
                // A command's key presses its button (as config.exe's
                // Hotkey editor sets them), after Esc's cancelling.
                let cancels = code == KeyCode::Escape
                    && (self.pending.is_some() || self.reloading().is_some());
                let slots = if cancels {
                    Vec::new()
                } else {
                    self.hotkey_slots(code)
                };
                if !slots.is_empty() {
                    // Each button of the key in turn, while the panel still
                    // shows it there.
                    for (slot, command) in slots {
                        if self.command_in(slot) == Some(command) {
                            self.press_button(slot);
                        }
                    }
                    return;
                }
                match code {
                    KeyCode::KeyS if command => self.quick_save(),
                    KeyCode::KeyO if command => self.quick_load(),
                    KeyCode::Escape if self.pending.is_some() => {
                        self.pending = None;
                        self.status = None;
                    }
                    // Finish reloading objects - Esc (string 400).
                    KeyCode::Escape if self.reloading().is_some() => {
                        if let Some(id) = self.selected {
                            if let Err(e) = self.issue(Order::EndLoad { id }) {
                                self.say(e);
                            }
                        }
                    }
                    KeyCode::Escape if self.page != Page::Main => self.page = Page::Main,
                    KeyCode::Escape if self.tab != Tab::Object => self.tab = Tab::Object,
                    KeyCode::Escape if self.selected.is_some() => {
                        self.selected = None;
                        self.group.clear();
                    }
                    // With nothing to cancel Esc does no more (the main
                    // menu is the file panel's or Alt+X's).
                    KeyCode::Escape if self.menus.is_some() => {}
                    KeyCode::Escape => event_loop.exit(),
                    KeyCode::F11 => self.toggle_fullscreen(),
                    // Alt+P (the manual's Game Hot Keys; P is Repair).
                    KeyCode::KeyP if self.modifiers.alt_key() => self.paused = !self.paused,
                    KeyCode::Pause => self.paused = !self.paused,
                    // Backspace shows where the player was last attacked.
                    KeyCode::Backspace => {
                        if let Some((x, y)) = self.scene.last_attack {
                            self.camera.center_on(x, y, &screen, map);
                        }
                    }
                    // Alt+M (this port's; M is Move).
                    KeyCode::KeyM if self.modifiers.alt_key() => {
                        if let Some(a) = self.audio.as_mut() {
                            a.muted = !a.muted;
                            let muted = a.muted;
                            if let Some(m) = &self.music {
                                m.set_volume(if muted { 0.0 } else { self.music_volume });
                            }
                            if let Some(s) = &self.menu_music {
                                s.set_volume(if muted { 0.0 } else { a.volume });
                            }
                            self.say(if muted { "Sound off" } else { "Sound on" });
                        }
                    }
                    KeyCode::Enter if self.modifiers.alt_key() => self.toggle_fullscreen(),
                    KeyCode::PageDown => {
                        self.switch_mission(1);
                    }
                    KeyCode::PageUp => {
                        self.switch_mission(-1);
                    }
                    // Tab puts the radar map in place of the view.
                    KeyCode::Tab => self.toggle_radar(),
                    // Space hides or shows the control bar (the manual's
                    // Game Hot Keys).
                    KeyCode::Space => self.toggle_bar(),
                    KeyCode::Equal | KeyCode::NumpadAdd => self
                        .camera
                        .zoom_at(ZOOM_STEP, center.0, center.1, &screen, map),
                    KeyCode::Minus | KeyCode::NumpadSubtract => {
                        self.camera
                            .zoom_at(1.0 / ZOOM_STEP, center.0, center.1, &screen, map)
                    }
                    KeyCode::Home => {
                        let z = 1.0 / self.camera.zoom;
                        self.camera.zoom_at(z, center.0, center.1, &screen, map);
                    }
                    _ => {}
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.menu_drag.is_some() {
                    let p =
                        Menus::to_menu(self.window_size(), position.x as f32, position.y as f32);
                    self.menu_drag_to(p);
                }
                if self.menu_held.is_some()
                    || self.kind_held.is_some()
                    || self.tick_held
                    || self.kind_popup.is_some()
                {
                    let p =
                        Menus::to_menu(self.window_size(), position.x as f32, position.y as f32);
                    self.menu_pointer_at(p);
                }
                if let Some(slider) = self.slider_drag {
                    if let Some(layout) = (!self.bar_hidden).then(|| self.layout()) {
                        let x = (position.x as f32 - layout.bar_x) / layout.scale;
                        let v =
                            self.interface
                                .panels
                                .slider_value(slider, &self.settings.config, x);
                        self.set_slider(slider, v);
                    }
                }
                // Dragging draws a selection box with the modern mouse only
                // (the original selects by clicks and groups).
                if let Some(start) = self.left_down.filter(|_| self.settings.modern_mouse) {
                    let (dx, dy) = (position.x - start.x, position.y - start.y);
                    if dx * dx + dy * dy > CLICK_SLOP * CLICK_SLOP {
                        self.left_down = None;
                        self.box_from = Some(start);
                    }
                }
                if let Some((start, moved)) = self.right_press.as_mut() {
                    let (dx, dy) = (position.x - start.x, position.y - start.y);
                    *moved |= dx * dx + dy * dy > CLICK_SLOP * CLICK_SLOP;
                }
                if let Some(from) = self.drag_from {
                    let (screen, _) = self.placement();
                    self.camera.scroll(
                        (from.x - position.x) as f32,
                        (from.y - position.y) as f32,
                        &screen,
                        self.scene.map_size(),
                    );
                    self.drag_from = Some(position);
                } else if self.minimap_drag {
                    self.minimap_jump(position);
                }
                self.radar_hover(position);
                self.cursor = Some(position);
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                // Nothing is read during a fade.
                _ if self.fade.is_some() => {}
                // A right press outside the kind pop-up closes it too.
                (MouseButton::Right, ElementState::Pressed) if self.kind_popup.is_some() => {
                    if let Some(c) = self.cursor {
                        let p = Menus::to_menu(self.window_size(), c.x as f32, c.y as f32);
                        if self.kind_popup.as_ref().is_some_and(|k| k.outside(p)) {
                            self.close_kind_popup();
                        }
                    }
                }
                (MouseButton::Right | MouseButton::Middle, ElementState::Pressed) => {
                    self.drag_from = self.cursor;
                    if button == MouseButton::Right {
                        self.right_press = self.cursor.map(|c| (c, false));
                    }
                }
                (MouseButton::Right | MouseButton::Middle, ElementState::Released) => {
                    self.drag_from = None;
                    if button == MouseButton::Right {
                        if let (Some((_, false)), Some(c)) = (self.right_press.take(), self.cursor)
                        {
                            if let Some(slot) = self.button_at(c) {
                                self.cancel_button(slot);
                            } else if self.settings.modern_mouse {
                                self.right_click(c);
                            } else {
                                // The original's right button: drops a
                                // waiting command or steps the pointer on;
                                // it never lets the selection go.
                                self.step_pointer(c);
                            }
                        }
                    }
                }
                (MouseButton::Left, ElementState::Pressed) if self.video.is_some() => {
                    self.skip_video(false);
                }
                (MouseButton::Left, ElementState::Pressed) if self.ask.is_some() => {
                    self.ask_pressed = self.ask_answer_at();
                }
                (MouseButton::Left, ElementState::Released) if self.ask.is_some() => {
                    let held = self.ask_pressed.take();
                    if let Some(yes) = held.filter(|&h| Some(h) == self.ask_answer_at()) {
                        self.answer(yes);
                    }
                }
                (MouseButton::Left, ElementState::Pressed)
                    if self.menu.is_some() && self.menus.is_some() =>
                {
                    if let Some(c) = self.cursor {
                        self.menu_click(c);
                    }
                }
                (MouseButton::Left, ElementState::Pressed) if self.briefing => {
                    self.briefing = false;
                }
                (MouseButton::Left, ElementState::Pressed) => {
                    if let Some(c) = self.cursor {
                        if self.holds_press(c) {
                            self.left_down = Some(c);
                        } else {
                            self.left_click(c);
                        }
                    }
                }
                (MouseButton::Left, ElementState::Released) => {
                    let menu_hold = self.menu_held.is_some()
                        || self.kind_held.is_some()
                        || self.tick_held
                        || self.kind_popup.is_some();
                    if let Some(c) = self.cursor.filter(|_| menu_hold) {
                        let p = Menus::to_menu(self.window_size(), c.x as f32, c.y as f32);
                        self.menu_release(p);
                    }
                    self.menu_held = None;
                    self.kind_held = None;
                    self.tick_held = false;
                    if let Some(c) = self.left_down.take() {
                        self.left_click(c);
                    }
                    self.load_pressed = None;
                    self.panel_pressed = None;
                    // The cell shown big shrinks back (DoInfoBut).
                    self.info_view.big = None;
                    self.info_view.pressed = None;
                    self.minimap_drag = false;
                    self.slider_drag = None;
                    self.menu_drag = None;
                    self.menu_repeat = None;
                    if let (Some(from), Some(to)) = (self.box_from.take(), self.cursor) {
                        self.finish_select(from, to);
                    }
                }
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                // On the front end the wheel scrolls the description under
                // the pointer, else the list's window (the original has no
                // wheel).
                if let (Some(state), Some(_)) = (self.menu, &self.menus) {
                    let page = self.menu_page(state);
                    self.wheel_rest += steps;
                    let whole = self.wheel_rest.trunc();
                    self.wheel_rest -= whole;
                    let step = -whole as i64;
                    let p = self
                        .cursor
                        .map(|c| Menus::to_menu(self.window_size(), c.x as f32, c.y as f32));
                    let story = page.tracks.iter().find(|t| t.target == Scroller::Story);
                    let over_story = page.story.as_ref().is_some_and(|(r, _)| {
                        p.is_some_and(|p| {
                            p.0 >= r.0 && p.1 >= r.1 && p.0 < r.0 + r.2 + 30.0 && p.1 < r.1 + r.3
                        })
                    });
                    match story {
                        Some(t) if over_story => {
                            let last = t.count.saturating_sub(1) as i64;
                            self.desc_first = (t.pos as i64 + step).clamp(0, last) as usize;
                        }
                        _ => {
                            let last = page.items.len().saturating_sub(page.rows) as i64;
                            self.list_first = (page.first as i64 + step).clamp(0, last) as usize;
                        }
                    }
                    return;
                }
                let (screen, _) = self.placement();
                let c = self.cursor.unwrap_or(PhysicalPosition::new(
                    (screen.x + screen.w / 2) as f64,
                    (screen.y + screen.h / 2) as f64,
                ));
                if !screen.contains(c.x as f32, c.y as f32) {
                    return;
                }
                match delta {
                    // A mouse wheel zooms.
                    MouseScrollDelta::LineDelta(..) => self.camera.zoom_at(
                        ZOOM_STEP.powf(steps),
                        c.x as f32,
                        c.y as f32,
                        &screen,
                        self.scene.map_size(),
                    ),
                    // A trackpad's two-finger swipe moves the map (pinch
                    // zooms, below).
                    MouseScrollDelta::PixelDelta(p) => {
                        self.camera
                            .scroll(-p.x as f32, -p.y as f32, &screen, self.scene.map_size())
                    }
                }
            }
            WindowEvent::PinchGesture { delta, .. } => {
                let (screen, _) = self.placement();
                let c = self.cursor.unwrap_or(PhysicalPosition::new(
                    (screen.x + screen.w / 2) as f64,
                    (screen.y + screen.h / 2) as f64,
                ));
                if self.menu.is_none() && screen.contains(c.x as f32, c.y as f32) {
                    self.camera.zoom_at(
                        (1.0 + delta as f32).max(0.1),
                        c.x as f32,
                        c.y as f32,
                        &screen,
                        self.scene.map_size(),
                    );
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    self.fail(event_loop, e);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.quit {
            event_loop.exit();
            return;
        }
        // Redraw at ~60 Hz; palette cycling needs a steady clock even when idle.
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::group_offset;

    #[test]
    fn group_places_are_apart() {
        let places: Vec<(f32, f32)> = (0..12).map(group_offset).collect();
        assert_eq!(places[0], (0.0, 0.0));
        for (i, a) in places.iter().enumerate() {
            for b in &places[i + 1..] {
                let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
                assert!(d > 16.0, "{a:?} {b:?}");
            }
        }
    }
}

#[cfg(test)]
mod scroll_tests {
    /// No key held, no scrolling: the map once drifted right and down
    /// because `f32::signum(0.0)` is 1.
    #[test]
    fn no_input_no_scroll() {
        assert_eq!(super::direction(0.0), 0.0);
        assert_eq!(super::direction(2.0), 1.0);
        assert_eq!(super::direction(-1.0), -1.0);
    }
}

/// Whether the Reload Object of `selected` may take up `o` (command 0x1f
/// with a target): for a ship (0x4dc630) an own port, shipyard, dock or
/// fort, or another ship; for a settlement (0x441ba0, 0xc452) one of its
/// forts.
fn is_load_target(
    scene: &Scene,
    o: &crate::objects::MapObject,
    selected: Option<&crate::objects::MapObject>,
) -> bool {
    use openconquest_formats::ObjectClass;
    let fort = matches!(o.class, ObjectClass::Building(0x34..=0x36));
    if let Some(site) = selected.filter(|s| {
        matches!(
            s.class,
            ObjectClass::Building(economy::SETTLEMENT | economy::TOWN)
        )
    }) {
        return fort
            && scene
                .object_index(o.id)
                .is_some_and(|i| scene.home_settlement(i) == Some(site.id));
    }
    match o.class {
        ObjectClass::Ship => selected.is_some_and(|s| s.id != o.id),
        ObjectClass::Building(c) => {
            fort || matches!(c, economy::PORT | economy::SHIPYARD | economy::DOCK)
        }
        _ => false,
    }
}

/// Group key `code` (1 ... 9, 0) as an index 0-9.
fn digit_index(code: KeyCode) -> Option<usize> {
    use KeyCode::*;
    [
        Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
    ]
    .iter()
    .position(|&k| k == code)
}

/// Place key `code` (F1 ... F10) as an index 0-9.
fn function_index(code: KeyCode) -> Option<usize> {
    use KeyCode::*;
    [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10]
        .iter()
        .position(|&k| k == code)
}

/// A start report's bytes: the word "RP", the difficulty word, the body.
fn report_bytes(difficulty: u16, body: &ReportBody) -> Result<Vec<u8>> {
    let mut bytes = (u32::from(REPORT_MAGIC) | u32::from(difficulty) << 16)
        .to_le_bytes()
        .to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(body)?);
    Ok(bytes)
}

/// A start report's difficulty and body, when `path` begins with "RP"
/// (0x53a670 checks the magic).
fn read_report(path: &std::path::Path) -> Option<(u16, ReportBody)> {
    let bytes = std::fs::read(path).ok()?;
    let head = bytes.get(..4)?;
    if u16::from_le_bytes([head[0], head[1]]) != REPORT_MAGIC {
        return None;
    }
    let difficulty = u16::from_le_bytes([head[2], head[3]]);
    let body = serde_json::from_slice(&bytes[4..]).ok()?;
    Some((difficulty, body))
}

/// A building site shown: its cell, the class and its facing.
type SiteKey = ((i32, i32), u32, u32);

#[cfg(test)]
mod custom_tests {
    use super::*;

    /// An app on the title screen over the game data (needs
    /// `OPENCONQUEST_DATA`).
    pub(super) fn app() -> Option<App> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let missions = data.missions().unwrap();
        let options = Options {
            fullscreen: false,
            edge_scroll: false,
            window_size: (800, 600),
            mute: true,
            soundfont: None,
            cd_audio: None,
            intro: false,
            fog: true,
            difficulty: 1,
        };
        let camera = Camera::new(crate::view::ScalePolicy {
            fixed: None,
            integer: false,
        });
        Some(App::new(data, missions, 0, camera, options, None, true).unwrap())
    }

    /// Custom missions with `mission` chosen and shown.
    fn show(app: &mut App, mission: &str) {
        let list = app.screen_missions(Category::Single);
        let i = list
            .iter()
            .position(|&k| app.missions[k] == mission)
            .unwrap();
        let state = MenuState::Missions(Category::Single, None);
        app.menu = Some(state);
        app.menu_select(state, i);
        let (index, _) = app.preview_due.unwrap();
        app.preview_due = Some((index, Instant::now()));
        app.load_due_preview();
        app.poll_menu_keys();
    }

    fn click(app: &mut App, p: (f32, f32)) {
        app.menu_press(p);
        app.menu_release(p);
    }

    fn kinds(app: &App) -> Vec<u8> {
        app.custom_players.iter().map(|c| c.kind).collect()
    }

    /// The players' kinds as 0x52a990 starts them, the pop-up's entries
    /// and place (0x53b940, 0x52ebe0), and what a choice sets (0x1cc).
    #[test]
    fn the_kind_popup_chooses_as_the_original() {
        let Some(mut app) = app() else {
            return;
        };
        for (k, name) in KIND_NAMES.iter().enumerate() {
            assert_eq!(app.data.strings.get(&(1146 + k as u32)).unwrap(), name);
        }
        app.player_name = "Jason".into();
        // battle01: HEAD_CUST (1, 1, 5): player 5 (nation 1, slot 1) human.
        show(&mut app, "battle01");
        assert_eq!(kinds(&app), vec![2, 3]);
        assert_eq!(app.custom_players[0].name, "Jason");
        // The second row's arrow: the five entries at (215, 258 + 27).
        click(&mut app, (350.0, 280.0));
        let pop = app.kind_popup.clone().unwrap();
        assert_eq!(pop.entries, vec![0, 2, 3, 4, 5]);
        assert_eq!((pop.x, pop.y), (215.0, 285.0));
        assert_eq!(pop.current, 0);
        // A press outside closes it, and Start under it is not pressed.
        app.menu_press((280.0, 575.0));
        assert!(app.kind_popup.is_none());
        assert!(app.menu_held.is_none());
        click(&mut app, (300.0, 280.0));
        assert!(app.kind_popup.is_some());
        // Esc: bound next to the suspended bindings, it does nothing.
        app.keys.insert(KeyCode::Escape);
        app.poll_menu_keys();
        assert!(app.kind_popup.is_some());
        app.keys.clear();
        app.poll_menu_keys();
        // Human (entry 1, at 285 + 12 + 21): the old human goes AI Weak.
        click(&mut app, (260.0, 285.0 + 12.0 + 21.0 + 5.0));
        assert!(app.kind_popup.is_none());
        assert_eq!(kinds(&app), vec![3, 2]);
        assert_eq!(app.custom_players[1].name, "Jason");
        assert!(app.custom_players[0].name.is_empty());
        // Press on one entry, let go on another: nothing.
        click(&mut app, (350.0, 253.0));
        let pop = app.kind_popup.clone().unwrap();
        assert_eq!(pop.y, 258.0);
        app.menu_press((260.0, 258.0 + 12.0 + 21.0 * 4.0 + 5.0));
        app.menu_release((260.0, 258.0 + 12.0 + 21.0 * 3.0 + 5.0));
        assert!(app.kind_popup.is_some());
        // A drag from the current entry moves it, and chooses nothing.
        app.menu_press((260.0, 270.0 + 5.0));
        app.menu_pointer_at((260.0, 270.0 + 21.0 + 5.0));
        assert_eq!(app.kind_popup.as_ref().unwrap().current, 1);
        app.menu_release((260.0, 270.0 + 21.0 + 5.0));
        assert!(app.kind_popup.is_some());
        // Disable chosen.
        click(&mut app, (260.0, 275.0));
        assert_eq!(kinds(&app), vec![0, 2]);
        // Start: straight into the game, the second player human, the
        // first without a computer player.
        let state = app.menu.unwrap();
        app.menu_action(state, "Start");
        assert_eq!(app.menu, None);
        let h = app.scene.human.unwrap();
        assert_eq!((h.nation, h.slot), (3, 3));
        assert_eq!(app.scene.computers.off, vec![(1, 1)]);
        // The disabled player's objects were never made (0x51e810).
        assert!(!app.scene.objects.iter().any(|o| o.owner == Some((1, 1))));
    }

    /// The map preview's marks (0x52f470): `LIST_OBJ`'s entries, a
    /// player's object in its player's frame, kept while its row is not
    /// disabled (0x528de0); with `HEAD_CUST` a set and b clear only the
    /// Human's; none with the map tick off.
    #[test]
    fn the_map_preview_marks_as_the_original() {
        let Some(mut app) = app() else {
            return;
        };
        show(&mut app, "battle01");
        let p = app.preview.as_ref().unwrap();
        assert_eq!(p.list.len(), 50);
        let town = openconquest_formats::ListObj {
            kind: 1,
            size: 4,
            owner: (1, 1),
            class: 0x0101,
            key: -1,
            x: 64,
            y: 1152,
            shown: true,
        };
        assert!(p.list.contains(&town));
        // Whether any pixel of entry `e`'s mark differs from the terrain.
        let marked = |app: &App, e: &openconquest_formats::ListObj| {
            let p = app.preview.as_ref().unwrap();
            let m = p.map.as_ref().unwrap();
            let side = if e.kind == 1 && e.size == 4 { 16 } else { 8 };
            let (x0, y0) = ((e.x / 8).max(0) as u32, (e.y / 8).max(0) as u32);
            (0..side).any(|dy| {
                (0..side).any(|dx| {
                    let k = ((y0 + dy) * m.width + x0 + dx) as usize;
                    m.pixels[k] != Some(p.terrain.2[k])
                })
            })
        };
        let theirs: Vec<_> = p
            .list
            .iter()
            .filter(|e| e.kind == 1 && e.owner == (3, 3) && e.shown)
            .copied()
            .collect();
        let others: Vec<_> = p.list.iter().filter(|e| e.kind == 2).copied().collect();
        assert!(!theirs.is_empty() && !others.is_empty());
        assert!(marked(&app, &town));
        assert!(theirs.iter().all(|e| marked(&app, e)));
        // Custom missions draw players' objects only.
        assert!(!others.iter().any(|e| marked(&app, e)));
        // The second player disabled: its marks go.
        app.custom_players[1].kind = 0;
        app.make_preview_map();
        assert!(!theirs.iter().any(|e| marked(&app, e)));
        assert!(marked(&app, &town));
        // The map tick off: no picture; on again, the marks back.
        app.toggle_map_tick();
        assert!(app.preview.as_ref().unwrap().map.is_none());
        app.toggle_map_tick();
        assert!(marked(&app, &town));
        // single01: HEAD_CUST a = 1, b = 0: the Human's marks only.
        show(&mut app, "single01");
        assert_eq!(app.custom_head[..2], [1, 0]);
        let p = app.preview.as_ref().unwrap();
        let human = app
            .custom_players
            .iter()
            .find(|c| c.kind == 2)
            .unwrap()
            .info;
        let (mine, rest): (Vec<_>, Vec<_>) = p
            .list
            .iter()
            .filter(|e| e.kind == 1 && e.shown)
            .partition(|e| {
                (u32::from(e.owner.0), u32::from(e.owner.1)) == (human.nation, human.slot)
            });
        assert!(!mine.is_empty() && !rest.is_empty());
        assert!(mine.iter().all(|e| marked(&app, e)));
        assert!(!rest.iter().any(|e| marked(&app, e)));
    }

    /// The fades of Quit (command 0x131: 0x4ef350(50, 5), then the end)
    /// and of a saved game loaded (command 0x102: the picture out, a
    /// black 50 x 5 ms, `LOADING` in and out, the game's 25 steps in).
    #[test]
    fn quit_and_load_fade_as_the_original() {
        let Some(mut app) = app() else {
            return;
        };
        let size = (800, 600);
        let mut pixels = vec![0u32; 800 * 600];
        let mut draw = |app: &mut App| {
            let mut frame = Frame {
                pixels: &mut pixels,
                width: size.0,
                height: size.1,
            };
            app.draw_front_end(&mut frame)
        };
        // Ages the fade under way past its 50 steps of 5 ms.
        let age = |app: &mut App| {
            let f = app.fade.as_mut().unwrap();
            f.since -= Duration::from_millis(crate::fade::STEPS as u64 * crate::fade::STEP_MS);
            f.then
        };
        // The title screen shown, Quit game: it fades out, then the end.
        app.menu = Some(MenuState::Main);
        app.shown = None;
        draw(&mut app);
        age(&mut app);
        draw(&mut app);
        assert!(app.fade.is_none());
        app.menu_action(MenuState::Main, "Quit game");
        let f = app.fade.as_ref().unwrap();
        assert_eq!((f.way, f.then), (crate::fade::Way::Out, AfterFade::Exit));
        assert!(!app.quit);
        draw(&mut app);
        assert!(!app.quit);
        age(&mut app);
        draw(&mut app);
        assert!(app.quit);
        // A game shown, a save loaded in it: out, black, LOADING in and
        // out, then the game's own fade in.
        app.quit = false;
        app.fade = None;
        app.menu = None;
        app.in_game = true;
        app.shown = Some(Shown {
            screen: None,
            pixels: vec![7; 800 * 600],
            size,
        });
        app.fade_into_load();
        assert_eq!(app.fade.as_ref().unwrap().pixels[0], 7);
        let mut chain = Vec::new();
        while app.fade.is_some() {
            chain.push(age(&mut app));
            draw(&mut app);
        }
        assert_eq!(
            chain,
            [
                AfterFade::Black,
                AfterFade::LoadingIn,
                AfterFade::LoadingOut,
                AfterFade::GameIn
            ]
        );
        assert!(app.game_fade.is_some());
        // From the Load game screen the screen change does it, through
        // black too.
        app.menu = Some(MenuState::Load(LoadTab::Scenario, None));
        app.fade_into_load();
        assert!(app.entering && app.entering_load);
    }

    /// Custom missions' Start with no Human row (0x53b940: no viewing
    /// player): every row a computer player, nothing fogged, the view at
    /// (0, 0), no pointer mode, and the game over into the main menu once
    /// at most one player owns anything (0x448610).
    #[test]
    fn a_custom_game_without_a_human() {
        let Some(mut app) = app() else {
            return;
        };
        show(&mut app, "battle01");
        assert_eq!(kinds(&app), vec![2, 3]);
        app.custom_players[0].kind = 4;
        let state = app.menu.unwrap();
        app.menu_action(state, "Start");
        assert_eq!(app.menu, None);
        assert!(app.scene.human.is_none());
        assert!(!app.scene.fogged());
        assert!(app.scene.purses.contains_key(&(1, 1)));
        assert!(app.scene.computers.levels.contains(&((1, 1), 1)));
        assert!(app.scene.objects.iter().any(|o| o.owner == Some((1, 1))));
        app.cursor = Some(PhysicalPosition::new(300.0, 300.0));
        assert!(app.hovered_info().is_none());
        // Both still own something: not over.
        assert!(!app.scene.spectators_done());
        for o in app.scene.objects.iter_mut() {
            if o.owner == Some((3, 3)) {
                o.combat = None;
            }
        }
        assert!(app.scene.spectators_done());
        app.camera.x = 500.0;
        app.update(0.0);
        assert_eq!((app.camera.x, app.camera.y), (0.0, 0.0));
        assert!(app.scene.over);
        app.update(0.0);
        assert_eq!(app.menu, Some(MenuState::Main));
        assert!(app.scene.outcome.is_none());
    }

    /// The connect screen's name edit (_sedit.cpp): the caret starts at
    /// column 0, typing inserts at it, the keys move and delete as
    /// 0x54f040 does, Esc leaves and a click inside comes back with the
    /// caret at the character clicked.
    #[test]
    fn the_name_edit_keys_as_the_original() {
        let Some(mut app) = app() else {
            return;
        };
        let state = MenuState::Connect;
        app.menu_action(MenuState::Main, "MultiPlayer game");
        assert_eq!(app.menu, Some(state));
        app.poll_menu_keys();
        app.player_name = "Jason".into();
        assert_eq!((app.connect_edit.at(), app.connect_edit.x), (0, 0.0));
        let type_in = |app: &mut App, t: &str| assert!(app.edit_typed(KeyCode::KeyA, t));
        let key = |app: &mut App, m: u8| app.key_message(state, KeyMsg::Edit(m));
        type_in(&mut app, "A");
        assert_eq!(app.player_name, "AJason");
        assert_eq!(app.connect_edit.at(), 1);
        key(&mut app, 0x3f);
        key(&mut app, 0x3d);
        key(&mut app, 0x3d);
        type_in(&mut app, "X");
        assert_eq!(app.player_name, "AJasXon");
        key(&mut app, 0x3e);
        key(&mut app, 0x36);
        assert_eq!(app.player_name, "AJasXon");
        key(&mut app, 0x37);
        assert_eq!(app.player_name, "JasXon");
        key(&mut app, 0x3f);
        key(&mut app, 0x37);
        key(&mut app, 0x36);
        assert_eq!(app.player_name, "JasXo");
        key(&mut app, 0x3c);
        assert_eq!(app.connect_edit.at(), 5);
        // The caret's x: each character's width and a pixel.
        let band = app.menus.as_ref().unwrap().measure(EDIT_STYLE, "JasXo") + 1.0;
        assert_eq!(app.connect_edit.x, band);
        // Esc leaves it: nothing typed goes in.
        key(&mut app, 0x35);
        assert!(!app.connect_edit.active);
        assert!(!app.edit_typed(KeyCode::KeyA, "Q"));
        // A click inside, on the second character.
        let one = app.menus.as_ref().unwrap().measure(EDIT_STYLE, "J") + 1.0;
        app.menu_press((CONNECT_EDIT.0 + one + 1.0, 260.0));
        assert!(app.connect_edit.active);
        assert_eq!(app.connect_edit.at(), 1);
        // A click outside leaves it again.
        app.menu_press((600.0, 100.0));
        assert!(!app.connect_edit.active);
    }

    /// The session's name is made once from the stored one (0x53d220) by
    /// the main menu's mission buttons, and the human takes it.
    #[test]
    fn the_session_name_comes_from_the_stored_one() {
        let Some(mut app) = app() else {
            return;
        };
        app.stored_name = "Medea".into();
        app.menu_action(MenuState::Main, "Custom missions");
        assert!(app.session_made);
        assert_eq!(app.player_name, "Medea");
        show(&mut app, "battle01");
        assert_eq!(app.custom_players[0].name, "Medea");
        // Made once: a changed session name stays.
        app.player_name = "Jason".into();
        app.menu_action(MenuState::Main, "Load game");
        assert_eq!(app.player_name, "Jason");
    }

    /// A start report (0x53a3a0): "RP", the difficulty word, then the
    /// port's body; read back, the magic checked (0x53a670). Its name and
    /// row use strings 100, 106-108 and 10000 (month first).
    #[test]
    fn start_reports_go_and_come_back() {
        let Some(app) = app() else {
            return;
        };
        let strings = &app.data.strings;
        assert_eq!(strings.get(&100).unwrap(), "MISSION_");
        assert_eq!(strings.get(&107).unwrap(), "Normal");
        assert!(strings.get(&10000).unwrap().starts_with('0'));
        let body = ReportBody {
            mission: 1,
            carry: Some(app.scene.carry()),
        };
        let bytes = report_bytes(2, &body).unwrap();
        assert_eq!(&bytes[..4], &[b'R', b'P', 2, 0]);
        let path = std::env::temp_dir().join(format!("oc-report-{}.RPM", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let (difficulty, back) = read_report(&path).unwrap();
        assert_eq!((difficulty, back.mission), (2, 1));
        assert!(back.carry.is_some());
        std::fs::write(&path, b"SAVE").unwrap();
        assert!(read_report(&path).is_none());
        std::fs::remove_file(&path).unwrap();
    }

    /// single02's HEAD_CUST (1, 0, 15): the human's row does not open, a
    /// computer's offers the three levels (T_PLAYER3); the last row opens
    /// upward by the picture's height.
    #[test]
    fn a_custom_mission_without_b_offers_the_levels_only() {
        let Some(mut app) = app() else {
            return;
        };
        show(&mut app, "single02");
        // (1, 1) computer, (3, 3) human.
        assert_eq!(kinds(&app), vec![3, 2]);
        click(&mut app, (350.0, 280.0));
        assert!(app.kind_popup.is_none());
        click(&mut app, (350.0, 253.0));
        let pop = app.kind_popup.clone().unwrap();
        assert_eq!(pop.entries, vec![3, 4, 5]);
        assert_eq!(pop.picture(), ("T_PLAYER3", 146.0, 87.0));
        // AI Strong.
        click(&mut app, (260.0, 258.0 + 12.0 + 42.0 + 5.0));
        assert_eq!(kinds(&app), vec![5, 2]);
        let state = app.menu.unwrap();
        app.menu_action(state, "Start");
        assert_eq!(app.scene.computers.levels, vec![((1, 1), 2)]);
        // single03's computer is at level 1: AI Medium.
        app.menu = None;
        show(&mut app, "single03");
        assert_eq!(kinds(&app), vec![2, 4]);
    }
}
