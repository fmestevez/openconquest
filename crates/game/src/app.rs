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
use crate::menus::{Backdrop, Button, MenuHit, MenuView, Menus};
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
    /// A mission to start.
    Mission(usize),
}

/// What the mission screens show of the chosen mission: its map, size,
/// briefing and players.
struct Preview {
    index: usize,
    size: (u32, u32),
    map: Option<Rc<crate::assets::Image>>,
    briefing: Vec<String>,
    players: Vec<String>,
    /// Each player's nation, slot and whether the computer plays it.
    seats: Vec<(u32, u32, bool)>,
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
        let (w, h, pixels) =
            crate::terrain::Terrain::small(data, &grid, &mut caches.tiles, &screen)?;
        let nation = |n: u32| match n {
            1 => "Greeks",
            3 => "Persians",
            _ => "Barbarians",
        };
        let infos = openconquest_formats::players(&archive).unwrap_or_default();
        let seats = infos
            .iter()
            .map(|p| (p.nation, p.slot, p.computer))
            .collect();
        let start = infos
            .iter()
            .find(|p| !p.computer)
            .and_then(|p| openconquest_formats::mission::start_view(&archive, p.nation, p.slot))
            .unwrap_or((0, 0));
        let players = infos
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let who = if p.computer { "computer" } else { "you" };
                format!("{}. {} ({who})", i + 1, nation(p.nation))
            })
            .collect();
        Ok(Self {
            index,
            size: (grid.width, grid.height),
            map: Some(Rc::new(crate::assets::Image::new(
                w,
                h,
                pixels.into_iter().map(Some).collect(),
            ))),
            briefing: openconquest_formats::briefing(&archive).unwrap_or_default(),
            players,
            seats,
            start,
            text: openconquest_formats::briefing_text(&archive).unwrap_or_default(),
        })
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
    credits: bool,
    story: Option<(crate::menus::Rect, Vec<String>)>,
    story_style: Option<(crate::menus::Style, f32)>,
    story_arrow: Option<(f32, f32)>,
    labels: Vec<(String, f32, f32)>,
    checks: Vec<crate::menus::Check>,
    panels: Vec<crate::menus::Panel>,
    list_rect: Option<crate::menus::Rect>,
    picture: Option<(Rc<crate::assets::Image>, crate::menus::Rect)>,
    boxes: Vec<(crate::menus::Rect, Vec<String>)>,
    tabs: Vec<crate::menus::MenuTab>,
    sprites: Vec<crate::menus::Placed>,
    texts: Vec<crate::menus::Text>,
    map: Option<(Rc<crate::assets::Image>, crate::menus::Rect, (u32, u32))>,
    row_height: Option<f32>,
    spots: Vec<(crate::menus::Rect, MenuHit)>,
    fills: Vec<(crate::menus::Rect, u32)>,
}

impl MenuPage {
    fn view(&self, hover: Option<(f32, f32)>) -> MenuView<'_> {
        MenuView {
            backdrop: self.backdrop,
            title: self.title.as_deref(),
            list: (!self.items.is_empty()).then_some((self.items.as_slice(), self.selected)),
            first: self.first,
            text: &self.text,
            notes: &self.notes,
            buttons: &self.buttons,
            hover,
            credits: self.credits,
            story: self.story.as_ref().map(|(r, p)| (*r, p.as_slice())),
            story_style: self.story_style,
            story_arrow: self.story_arrow,
            labels: &self.labels,
            checks: &self.checks,
            panels: &self.panels,
            list_rect: self.list_rect,
            picture: self.picture.as_ref().map(|(i, r)| (&**i, *r)),
            boxes: &self.boxes,
            tabs: &self.tabs,
            sprites: &self.sprites,
            texts: &self.texts,
            map: self.map.as_ref().map(|(i, r, o)| (&**i, *r, *o)),
            row_height: self.row_height,
            spots: &self.spots,
            fills: &self.fills,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            config: crate::launch::panel_config(),
            modern_mouse: crate::launch::modern_mouse(),
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
    show_interface: bool,
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
    /// When objects were deselected, for their frames' zoom out.
    deselected: HashMap<u32, u64>,
    /// The map places assigned with Shift+F1 ... Shift+F10, centred on with
    /// F1 ... F10.
    places: [Option<(f32, f32)>; 10],
    /// Where a left-button selection box started.
    box_from: Option<PhysicalPosition<f64>>,
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
    /// The menus' own music (`mus_menu`, sound event 325, which the
    /// original plays looped when no other is asked for: 0x525370), while
    /// the front-end menus are up.
    menu_music: Option<rodio::Sink>,
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
    /// The SoundFont given for MIDI music.
    soundfont: Option<std::path::PathBuf>,
    /// The music's tracks as the sound panel lists them.
    track_names: Vec<String>,
    /// The object panel's slot cell chosen: the object and the cell.
    chosen_cell: Option<(u32, usize)>,
    /// A unit picked up from a settlement's stock: the settlement and item.
    carrying: Option<(u32, u8)>,
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
    /// The computer players' level on the custom missions screen (0 weak,
    /// as the original starts it).
    custom_ai: usize,
    /// The name typed on the multiplayer screen.
    player_name: String,
    /// A chat line being typed in a multiplayer game.
    chat_line: Option<String>,
    /// The first entry shown of a front-end list, and wheel movement not
    /// yet a whole row (trackpads scroll in small steps).
    list_first: usize,
    wheel_rest: f32,
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
    /// Mission titles from their briefings, by mission index.
    mission_titles: Vec<String>,
    /// A game is under way (the main menu offers to resume it).
    in_game: bool,
    /// The mission clock when it was won or lost.
    outcome_at: Option<f32>,
    /// Best winning times, and what the last win came to: its time and
    /// whether it was a new best.
    records: crate::records::Records,
    last_win: Option<(f32, bool)>,
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
        let records = crate::records::Records::load(&data.save_dir().join("best.json"));
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
        let mut caches = Caches::default();
        let restored = save.is_some();
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
            show_interface: true,
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
            deselected: HashMap::new(),
            places: [None; 10],
            box_from: None,
            pending: None,
            drawn_size: (1, 1),
            pointers: Default::default(),
            page: Page::Main,
            briefing: !restored,
            music: None,
            menu_music: None,
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
            save_names: Vec::new(),
            save_first: 0,
            selected_save: None,
            file_mode: FileMode::Save,
            save_name: String::new(),
            help_text: Vec::new(),
            help_first: 0,
            help_page: None,
            panel_pressed: None,
            soundfont: soundfont.clone(),
            track_names: Vec::new(),
            chosen_cell: None,
            carrying: None,
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
            custom_ai: 0,
            chat_line: None,
            list_first: 0,
            wheel_rest: 0.0,
            battle_maps: Vec::new(),
            hosting: None,
            joining: None,
            slider_drag: None,
            paused: false,
            menu: start_menu.then_some(MenuState::Main),
            menus,
            preview: None,
            mission_titles,
            in_game: !start_menu,
            outcome_at: None,
            records,
            last_win: None,
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
        app.apply_volumes();
        app.start_music();
        Ok(app)
    }

    pub fn run(mut self) -> Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.run_app(&mut self)?;
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
        GameView {
            selected_since: &self.selected_since,
            deselected: &self.deselected,
            data: &self.data,
            scene: &self.scene,
            camera: &self.camera,
            interface: self.show_interface.then_some(&self.interface),
            text: &self.text,
            selected: self.selected,
            group: &self.group,
            select_box: self.box_from.zip(self.cursor).and_then(|(a, b)| {
                ((a.x - b.x).abs() + (a.y - b.y).abs() > BOX_THRESHOLD)
                    .then_some((a.x as f32, a.y as f32, b.x as f32, b.y as f32))
            }),
            status,
            time_ms: self.time_ms,
            sent: self
                .sent
                .map(|(x, y, at)| (x, y, self.time_ms.saturating_sub(at))),
            hover_button: self.cursor.and_then(|c| self.button_at(c)),
            chosen_cell: self
                .chosen_cell
                .filter(|&(site, _)| Some(site) == self.selected)
                .map(|(_, i)| i),
            page: self.page,
            site: None,
            briefing: self.briefing,
            panel: Some(self.panel_view()),
            panel_hover: self.cursor.and_then(|c| self.bar_point(c)),
            paused: self.paused,
            load: Some((self.load_state(), self.load_pressed)),
            object_info: self.hovered_info(),
        }
    }

    /// The object info for the strip below the view (gcursor.cpp
    /// 0x516c10): the object under the pointer fills it; off objects it is
    /// blank; one that shows none (type 0x1f7) leaves the last.
    fn hovered_info(&self) -> Option<crate::object_info::ObjectInfo> {
        // Only with "help string" on (0x5175b0 asks [0x5ca542]).
        if !self.settings.config.help_string {
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
        if !self.show_interface || Panels::covers_bar(self.tab) {
            return None;
        }
        crate::load_panel::lists(&self.scene, self.selected?, &self.load_state())
    }

    /// What of the load panel lies under window position `pos`.
    fn load_hit(&self, pos: PhysicalPosition<f64>) -> Option<crate::load_panel::Hit> {
        let panel = self.reloading()?;
        let layout = self
            .interface
            .layout(self.window_size(), &self.camera.policy, &self.scene);
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
        if !self.show_interface {
            return None;
        }
        let layout = self
            .interface
            .layout(self.window_size(), &self.camera.policy, &self.scene);
        let x = (pos.x as f32 - layout.bar_x) / layout.scale;
        let y = pos.y as f32 / layout.scale;
        (x >= 0.0 && y >= 0.0).then_some((x as u32, y as u32))
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
            PanelHit::Restart => self.switch_mission(0),
            PanelHit::MainMenu => {
                if self.menus.is_some() {
                    self.menu = Some(MenuState::Main);
                }
            }
            PanelHit::Quit => self.quit = true,
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
    fn file_action(&mut self) {
        match self.file_mode {
            FileMode::Save => {
                if self.net.is_some() {
                    self.say("Multiplayer games cannot be saved");
                    return;
                }
                let name: String = self
                    .save_name
                    .trim()
                    .chars()
                    .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
                    .collect();
                if name.is_empty() {
                    self.say("Type a name for the saved game");
                    return;
                }
                let path = self.data.save_dir().join(format!("{name}.json"));
                match self.scene.save().write(&path) {
                    Ok(()) => {
                        self.say("Game saved");
                        self.refresh_panels();
                        self.selected_save = self.save_names.iter().position(|n| *n == name);
                    }
                    Err(e) => self.say(format!("Could not save: {e:#}")),
                }
            }
            FileMode::Load => {
                if let Some(e) = self.selected_save.and_then(|i| self.saves.get(i)) {
                    let path = e.path.clone();
                    self.load_game(&path);
                }
            }
            FileMode::Delete => {
                if let Some(e) = self.selected_save.and_then(|i| self.saves.get(i)) {
                    let _ = std::fs::remove_file(&e.path);
                    self.selected_save = None;
                    self.save_name.clear();
                    self.refresh_panels();
                }
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
        let effects = panels::Config::gain(c.effects_volume);
        self.music_volume = panels::Config::gain(c.music_volume);
        let music = self.music_volume;
        let muted = self.audio.as_ref().is_some_and(|a| a.muted);
        if let Some(a) = self.audio.as_mut() {
            a.volume = effects;
        }
        if let Some(m) = &self.music {
            m.set_volume(if muted { 0.0 } else { music });
        }
        if let Some(s) = &self.menu_music {
            s.set_volume(if muted { 0.0 } else { music });
        }
    }

    /// What front-end screen `state` shows.
    fn menu_page(&self, state: MenuState) -> MenuPage {
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
        let empty = MenuPage {
            backdrop: Backdrop::TitleDim,
            title: None,
            items: Vec::new(),
            selected: None,
            text: Vec::new(),
            notes: Vec::new(),
            buttons: Vec::new(),
            first: self.list_first,
            credits: false,
            story: None,
            story_style: None,
            story_arrow: None,
            labels: Vec::new(),
            checks: Vec::new(),
            panels: Vec::new(),
            list_rect: None,
            picture: None,
            boxes: Vec::new(),
            tabs: Vec::new(),
            sprites: Vec::new(),
            texts: Vec::new(),
            map: None,
            row_height: None,
            spots: Vec::new(),
            fills: Vec::new(),
        };
        match state {
            MenuState::Main if self.in_game => MenuPage {
                backdrop: Backdrop::Title,
                buttons: column(&[
                    "Resume",
                    "Campaign",
                    "Single missions",
                    "Battles",
                    "Multiplayer",
                    "Load game",
                    "Best results",
                    "Quit",
                ]),
                ..empty
            },
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
            // "Select custom mission" as the original's (Startcst.cpp),
            // every widget placed where the original draws it (fitted to
            // its screen): the map panel with its scroll bars and the tick,
            // the description, the missions and the players.
            MenuState::Missions(Category::Single, selected) => {
                let list = self.screen_missions(Category::Single);
                let chosen = selected.and_then(|i| list.get(i)).copied();
                let preview = self.preview.as_ref().filter(|p| Some(p.index) == chosen);
                self.custom_page(&list, selected, preview, empty)
            }
            // "Select custom mission" as the original lays it out (panel
            // frames measured on its screen): the map of the chosen
            // mission, its description, the missions and its players;
            // Start and Cancel below.
            MenuState::Missions(category, selected) => {
                let list = self.screen_missions(category);
                let chosen = selected.and_then(|i| list.get(i)).copied();
                let preview = self.preview.as_ref().filter(|p| Some(p.index) == chosen);
                let panel = |rect, title: String, arrows| crate::menus::Panel {
                    rect,
                    title,
                    arrows,
                };
                let size = preview.map_or((0, 0), |p| p.size);
                let missions = panel((2.0, 216.0, 190.0, 338.0), "Missions".into(), true);
                let players = panel((195.0, 216.0, 598.0, 338.0), "Players".into(), true);
                let about = panel(
                    (195.0, 27.0, 599.0, 186.0),
                    "Mission description".into(),
                    true,
                );
                let mut boxes = Vec::new();
                if let Some(p) = preview {
                    boxes.push((about.inner(), p.briefing.clone()));
                    boxes.push((players.inner(), p.players.clone()));
                }
                let heading = match category {
                    Category::Campaign => "Select scenario mission",
                    Category::Single => "Select custom mission",
                    Category::Battle => "Select battle",
                };
                MenuPage {
                    backdrop: Backdrop::TitleDim,
                    labels: vec![(heading.into(), self.centred(heading, 494.0), 6.0)],
                    items: list
                        .iter()
                        .map(|&i| self.mission_titles[i].clone())
                        .collect(),
                    selected,
                    list_rect: Some(missions.inner()),
                    picture: preview
                        .and_then(|p| p.map.clone())
                        .map(|m| (m, (8.0, 29.0, 153.0, 153.0))),
                    boxes,
                    panels: vec![
                        panel(
                            (2.0, 3.0, 190.0, 210.0),
                            format!("Map ({}x{})", size.0, size.1),
                            false,
                        ),
                        about,
                        missions,
                        players,
                    ],
                    buttons: vec![
                        Button {
                            caption: "Start".into(),
                            x: 225.0,
                            y: 557.0,
                            shape: 2,
                        },
                        Button {
                            caption: "Cancel".into(),
                            x: 459.0,
                            y: 557.0,
                            shape: 3,
                        },
                    ],
                    ..empty
                }
            }
            // "Select mission to load" as the original's.
            MenuState::Load(tab, selected) => {
                let entries = self.load_entries(tab);
                let chosen = selected.and_then(|i| entries.get(i)).map(|e| e.0);
                let preview = self
                    .preview
                    .as_ref()
                    .filter(|p| chosen.and_then(|e| self.entry_mission(e)) == Some(p.index));
                self.load_page(tab, &entries, selected, preview, empty)
            }
            // "Your mission" (Starttsk.cpp, 0x53ed40 and 0x53f550): the
            // briefing in a box at (20, 80, 630, 440), "Difficulty level:"
            // (string 105) at (660, 405) over three boxes from (660, 440),
            // 30 apart (strings 106-108), Start (1010) at (244, 543) and
            // Cancel (164) at (440, 543).
            MenuState::Briefing => {
                let mut story = self.scene.briefing_text.clone();
                if story.is_empty() {
                    story.push(self.scene.mission.to_uppercase());
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
                    buttons: vec![
                        Button {
                            caption: "Start".into(),
                            x: 244.0,
                            y: 543.0,
                            shape: 2,
                        },
                        Button {
                            caption: "Cancel".into(),
                            x: 440.0,
                            y: 543.0,
                            shape: 3,
                        },
                    ],
                    ..empty
                }
            }
            MenuState::Best => {
                let items: Vec<String> = (0..self.missions.len())
                    .filter_map(|i| {
                        let best = self.records.best(&self.missions[i])?;
                        Some(format!(
                            "{}  {}",
                            crate::records::clock(best),
                            self.mission_titles[i]
                        ))
                    })
                    .collect();
                // The original's screen: its framed box at (15, 305, 378,
                // 210), empty until someone has won, and OK (string 10) at
                // (345, 540); both checked pixel for pixel against it.
                let panel = crate::menus::Panel {
                    rect: (15.0, 305.0, 378.0, 210.0),
                    title: String::new(),
                    arrows: false,
                };
                MenuPage {
                    backdrop: Backdrop::Best,
                    list_rect: Some(panel.inner()),
                    panels: vec![panel],
                    items,
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
                };
                let name_style = style("MH", 2);
                let typed = self
                    .menus
                    .as_ref()
                    .map_or(0.0, |m| m.measure(name_style, &self.player_name));
                let caret = if self.player_name.is_empty() {
                    43.0
                } else {
                    44.0 + typed
                };
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
                        text(name_style, 43.0, 255.0, &self.player_name),
                        text(style("MH", 2), 43.0, 321.0, "Internet TCP/IP Connection"),
                    ],
                    fills: vec![((caret, 256.0, 2.0, 18.0), 0xffff00)],
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
            MenuState::Outcome(outcome) => {
                let won = outcome == crate::scene::Outcome::Victory;
                let mut buttons = Vec::new();
                if won && self.next_mission().is_some() {
                    buttons.push(button("Next mission", 313.0, 470.0));
                }
                if !won {
                    buttons.push(button("Try again", 313.0, 470.0));
                }
                buttons.push(button("Main menu", 313.0, 522.0));
                let mut text = match (won, self.last_win) {
                    (true, Some((time, new))) => vec![format!(
                        "Won in {}{}",
                        crate::records::clock(time),
                        if new { ", a new best" } else { "" }
                    )],
                    _ => vec![if won {
                        "Mission accomplished"
                    } else {
                        "Mission failed"
                    }
                    .into()],
                };
                text.extend(self.scene.stats.lines(self.scene.amber));
                MenuPage {
                    backdrop: if won {
                        Backdrop::Accomplished
                    } else {
                        Backdrop::Failed
                    },
                    text,
                    buttons,
                    ..empty
                }
            }
        }
    }

    /// Missions of `category`, as indices into `missions`.
    /// The missions a mission screen lists: for Custom missions, as the
    /// original, the battles and single missions by file name.
    fn screen_missions(&self, category: Category) -> Vec<usize> {
        if category != Category::Single {
            return self.category_missions(category);
        }
        let mut list: Vec<usize> = (0..self.missions.len())
            .filter(|&i| {
                matches!(
                    Category::of(&self.missions[i]),
                    Some(Category::Single | Category::Battle)
                )
            })
            .collect();
        list.sort_by(|&a, &b| self.missions[a].cmp(&self.missions[b]));
        list
    }

    /// The map and description panels of the mission screens (Custom
    /// missions, Load game), as the original's: the map 8 pixels a cell in
    /// a 153-pixel window scrolled to the player's starting view, with its
    /// scroll bars and the tick; the titles bright once a mission is chosen.
    #[allow(clippy::type_complexity)]
    fn map_panels(
        preview: Option<&Preview>,
    ) -> (
        Vec<crate::menus::Placed>,
        Vec<crate::menus::Text>,
        Option<(Rc<crate::assets::Image>, crate::menus::Rect, (u32, u32))>,
    ) {
        use crate::menus::{Placed, Style, Text};
        let at = |name: &str, x: f32, y: f32| Placed {
            name: name.into(),
            x,
            y,
        };
        let arrow = |dir: &str, can: bool| format!("M_LBUT_{dir}{}", if can { 0 } else { 2 });
        let thumb = |at: u32, most: u32, travel: f32| {
            if most == 0 {
                0.0
            } else {
                (at.min(most) as f32 / most as f32 * travel).round()
            }
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
            },
        ];
        let mut sprites = vec![at("MP_MAP", 3.0, 3.0), at("MP_DESC", 196.0, 27.0)];
        // The map, 8 pixels a cell in a 153-pixel window, scrolled to the
        // player's starting view.
        const VIEW: u32 = 153;
        let mut map = None;
        let (mut mx, mut my, mut most_x, mut most_y) = (0, 0, 0, 0);
        if let Some(p) = preview {
            if let Some(m) = &p.map {
                most_x = m.width.saturating_sub(VIEW);
                most_y = m.height.saturating_sub(VIEW);
                mx = (p.start.0 * 8).min(most_x);
                my = (p.start.1 * 8).min(most_y);
                map = Some((m.clone(), (9.0, 29.0, VIEW as f32, VIEW as f32), (mx, my)));
            }
        }
        sprites.extend([
            at(&arrow("UP", my > 0), 165.0, 28.0),
            at(&arrow("DN", my < most_y), 165.0, 160.0),
            at("MAP_TR_V0", 165.0, 52.0),
            at("MAP_TR_V1", 169.0, 56.0 + thumb(my, most_y, 84.0)),
            at(&arrow("LF", mx > 0), 8.0, 185.0),
            at(&arrow("RT", mx < most_x), 140.0, 185.0),
            at("MAP_TR_H0", 32.0, 185.0),
            at("MAP_TR_H1", 36.0 + thumb(mx, most_x, 84.0), 189.0),
            at("M_CHECK1", 165.0, 185.0),
            at(&arrow("UP", false), 766.0, 52.0),
            at("DESC_TR0", 766.0, 76.0),
            at("DESC_TR1", 770.0, 80.0),
        ]);
        (sprites, texts, map)
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
        };
        let (mut sprites, mut texts, map) = Self::map_panels(preview);
        sprites.push(at("LOAD_RM1", 3.0, 216.0));
        texts.extend([
            text(style(2), 396.0, 2.0, "Select mission to load"),
            text(style(2), 13.0, 217.0, "Missions"),
        ]);
        const ROWS: usize = 9;
        let first = self.list_first.min(entries.len().saturating_sub(ROWS));
        for (k, (_, label)) in entries.iter().enumerate().skip(first).take(ROWS) {
            let y = 249.0 + 27.0 * (k - first) as f32;
            if selected == Some(k) {
                sprites.push(at("LOAD_RM0", 11.0, y - 4.0));
            }
            let (variant, dy) = if selected == Some(k) {
                (2, -1.0)
            } else {
                (1, 0.0)
            };
            texts.push(text(
                Style {
                    font: "MH",
                    variant,
                    spacing: 1,
                },
                13.0,
                y + dy,
                label,
            ));
        }
        let most = entries.len().saturating_sub(ROWS);
        let arrow = |dir: &str, can: bool| format!("M_LBUT_{dir}{}", if can { 0 } else { 2 });
        sprites.extend([
            at(&arrow("UP", first > 0), 767.0, 241.0),
            at(&arrow("DN", first < most), 767.0, 500.0),
            at("LOAD_TR0", 767.0, 265.0),
            at(
                "LOAD_TR1",
                771.0,
                269.0
                    + if most == 0 {
                        0.0
                    } else {
                        (first as f32 / most as f32 * 211.0).round()
                    },
            ),
        ]);
        // The tabs: the open one's picture and a bright caption a pixel
        // higher.
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
            texts.push(text(
                style(if open { 2 } else { 1 }),
                x + ((w - tw) / 2.0).ceil(),
                if open { 530.0 } else { 531.0 },
                caption,
            ));
        }
        let story = preview.map(|p| p.text.clone()).unwrap_or_default();
        let tab_spot = |x: f32, w: f32, k: usize| ((x, 528.0, w, 28.0), MenuHit::Tab(k));
        MenuPage {
            backdrop: Backdrop::TitleDim,
            items: entries.iter().map(|e| e.1.clone()).collect(),
            selected,
            first: self.list_first,
            list_rect: Some((8.0, 245.0, 755.0, 27.0 * ROWS as f32)),
            row_height: Some(27.0),
            story: Some(((204.0, 58.0, 559.0, 145.0), story)),
            story_style: Some((
                Style {
                    font: "MH",
                    variant: 1,
                    spacing: 1,
                },
                18.0,
            )),
            story_arrow: Some((766.0, 184.0)),
            sprites,
            texts,
            map,
            spots: vec![
                ((767.0, 241.0, 23.0, 23.0), MenuHit::Scroll(-1)),
                ((767.0, 500.0, 23.0, 23.0), MenuHit::Scroll(1)),
                tab_spot(322.0, 130.0, 0),
                tab_spot(452.0, 130.0, 1),
                tab_spot(582.0, 214.0, 2),
            ],
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
        };
        // Arrows: picture 0 when they can scroll, 2 when they cannot.
        let arrow = |dir: &str, can: bool| format!("M_LBUT_{dir}{}", if can { 0 } else { 2 });
        // A thumb's offset along a track of travel `travel`.
        let thumb = |at: u32, most: u32, travel: f32| {
            if most == 0 {
                0.0
            } else {
                (at.min(most) as f32 / most as f32 * travel).round()
            }
        };
        let (mut sprites, mut texts, map) = Self::map_panels(preview);
        sprites.extend([at("MP_LFILE1", 3.0, 216.0), at("SP_LPLAY1", 196.0, 216.0)]);
        texts.extend([
            text(title, 398.0, 2.0, "Select custom mission"),
            text(title, 13.0, 217.0, "Missions"),
            text(title, 209.0, 217.0, "Players"),
        ]);
        // The description, from its first line.
        let story = preview.map(|p| p.text.clone()).unwrap_or_default();
        // The missions, 27 apart; the chosen one framed and pale.
        const ROWS: usize = 11;
        let first = self.list_first.min(list.len().saturating_sub(ROWS));
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
        let most = list.len().saturating_sub(ROWS) as u32;
        sprites.extend([
            at(&arrow("UP", first > 0), 165.0, 241.0),
            at(&arrow("DN", (first as u32) < most), 165.0, 525.0),
            at("LFILE_TR0", 165.0, 265.0),
            at("LFILE_TR1", 169.0, 269.0 + thumb(first as u32, most, 236.0)),
        ]);
        // The players: flag, who plays, the choice arrow, the name.
        if let Some(p) = preview {
            let frame = (self.time_ms / 100 % 10) as u32;
            for (k, &(nation, slot, computer)) in p.seats.iter().enumerate().take(10) {
                let y = 249.0 + 27.0 * k as f32;
                let start = FLAG_STARTS[((nation * 4 + slot) as usize).min(15)];
                sprites.extend([
                    at("LPLAY_FL", 206.0, y),
                    at(&format!("M_FLAGS{:03}", start + frame), 209.0, y),
                    at("LPLAY_TP", 230.0, y),
                    at("M_CBUT0", 342.0, y),
                    at("LPLAY_NP", 375.0, y),
                ]);
                let (who, variant) = if computer {
                    (
                        ["AI Weak", "AI Medium", "AI Strong"][self.custom_ai.min(2)],
                        1,
                    )
                } else {
                    ("Human", 2)
                };
                texts.push(text(style("MH", variant), 234.0, y + 1.0, who));
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
            story: Some(((204.0, 58.0, 559.0, 145.0), story)),
            story_style: Some((style("MH", 1), 18.0)),
            story_arrow: Some((766.0, 184.0)),
            sprites,
            texts,
            map,
            spots: {
                let mut spots = vec![
                    ((165.0, 241.0, 23.0, 23.0), MenuHit::Scroll(-1)),
                    ((165.0, 525.0, 23.0, 23.0), MenuHit::Scroll(1)),
                ];
                if let Some(p) = preview {
                    for (k, s) in p.seats.iter().enumerate().take(10) {
                        if s.2 {
                            let y = 249.0 + 27.0 * k as f32;
                            spots.push(((342.0, y, 29.0, 21.0), MenuHit::Check(k)));
                        }
                    }
                }
                spots
            },
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

    fn category_missions(&self, category: Category) -> Vec<usize> {
        (0..self.missions.len())
            .filter(|&i| Category::of(&self.missions[i]) == Some(category))
            .collect()
    }

    /// The mission after the current one in its group.
    fn next_mission(&self) -> Option<usize> {
        let list = self.category_missions(Category::of(&self.missions[self.mission_index])?);
        let at = list.iter().position(|&i| i == self.mission_index)?;
        list.get(at + 1).copied()
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
        self.scene = scene;
        self.fog = setup.fog;
        self.selected = None;
        self.group.clear();
        self.pending = None;
        self.page = Page::Main;
        self.center_pending = true;
        self.briefing = false;
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
        if self.net.is_some() {
            self.say("Not in a multiplayer game");
            return;
        }
        match Scene::load_at(
            &self.data,
            &mut self.caches,
            &self.missions[index],
            self.difficulty,
        ) {
            Ok(scene) => {
                self.scene = scene;
                self.scene.fog = self.fog;
                if let Some(carry) = &self.carry {
                    self.scene.resume(carry);
                }
                self.mission_index = index;
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

    fn menu_click(&mut self, pos: PhysicalPosition<f64>) {
        let (Some(state), Some(menus)) = (self.menu, &self.menus) else {
            return;
        };
        let page = self.menu_page(state);
        let p = Menus::to_menu(self.window_size(), pos.x as f32, pos.y as f32);
        let Some(hit) = menus.hit(&page.view(None), p) else {
            return;
        };
        let caption = match hit {
            MenuHit::Button(i) => page.buttons[i].caption.clone(),
            MenuHit::Tab(t) => {
                if let MenuState::Load(..) = state {
                    let tab = [LoadTab::Scenario, LoadTab::Custom, LoadTab::StartFrom][t.min(2)];
                    self.menu = Some(MenuState::Load(tab, None));
                    self.list_first = 0;
                }
                return;
            }
            MenuHit::Scroll(d) => {
                let most = page.items.len().saturating_sub(1);
                self.list_first =
                    (self.list_first as i64 + d as i64).clamp(0, most as i64) as usize;
                return;
            }
            // A difficulty: the mission is set up again for it.
            MenuHit::Check(i) => {
                // A player's choice arrow on Custom missions: the computer
                // players' level goes round Weak, Medium, Strong.
                if let MenuState::Missions(Category::Single, _) = state {
                    let _ = i;
                    self.custom_ai = (self.custom_ai + 1) % 3;
                    return;
                }
                if state == MenuState::Briefing && self.difficulty != i {
                    self.difficulty = i;
                    self.open_mission(self.mission_index);
                }
                return;
            }
            MenuHit::Item(i) => {
                self.menu = Some(match state {
                    MenuState::Missions(c, _) => {
                        if let Some(&index) = self.screen_missions(c).get(i) {
                            match Preview::load(
                                &self.data,
                                &mut self.caches,
                                &self.missions[index],
                                index,
                            ) {
                                Ok(p) => self.preview = Some(p),
                                Err(e) => self.say(format!("{e:#}")),
                            }
                        }
                        MenuState::Missions(c, Some(i))
                    }
                    MenuState::Load(tab, _) => {
                        let entry = self.load_entries(tab).get(i).map(|e| e.0);
                        if let Some(index) = entry.and_then(|e| self.entry_mission(e)) {
                            if self.preview.as_ref().is_none_or(|p| p.index != index) {
                                match Preview::load(
                                    &self.data,
                                    &mut self.caches,
                                    &self.missions[index],
                                    index,
                                ) {
                                    Ok(p) => self.preview = Some(p),
                                    Err(e) => self.say(format!("{e:#}")),
                                }
                            }
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
                return;
            }
        };
        let was = self.menu;
        self.menu_action(state, &caption);
        if self.menu != was {
            self.list_first = 0;
        }
    }

    fn menu_action(&mut self, state: MenuState, caption: &str) {
        match (state, caption) {
            (_, "Resume") => self.menu = None,
            (_, "Campaign") => self.menu = Some(MenuState::Missions(Category::Campaign, None)),
            (_, "Single missions") => self.menu = Some(MenuState::Missions(Category::Single, None)),
            (_, "Battles") => self.menu = Some(MenuState::Missions(Category::Battle, None)),
            (_, "Load game") => {
                self.refresh_panels();
                self.menu = Some(MenuState::Load(LoadTab::Scenario, None));
            }
            (_, "Quit") | (_, "Quit game") => self.quit = true,
            // The campaign from its first mission, on its briefing.
            (_, "Start scenario") => {
                if let Some(&first) = self.category_missions(Category::Campaign).first() {
                    // The story opens before the first mission.
                    self.queue_video(crate::video::OPENING);
                    self.carry = None;
                    self.open_mission(first);
                }
            }
            (_, "Custom missions") => self.menu = Some(MenuState::Missions(Category::Single, None)),
            (_, "Best Conquerors") => self.menu = Some(MenuState::Best),
            (MenuState::Best, "OK") => self.menu = Some(MenuState::Main),
            (_, "MultiPlayer game") => self.menu = Some(MenuState::Connect),
            (MenuState::Connect, "New") | (MenuState::Connect, "Join") => {
                self.find_battle_maps();
                self.menu = Some(MenuState::Multiplayer(None));
            }
            (MenuState::Connect, "Cancel") => self.menu = Some(MenuState::Main),
            (_, "Best results") => self.menu = Some(MenuState::Best),
            (_, "Multiplayer") => {
                self.find_battle_maps();
                self.menu = Some(MenuState::Multiplayer(None));
            }
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
                // The story opens before the first campaign mission.
                if c == Category::Campaign && i == 0 {
                    self.queue_video(crate::video::OPENING);
                }
                if let Some(&index) = self.screen_missions(c).get(i) {
                    self.carry = None;
                    if c == Category::Single {
                        self.difficulty = self.custom_ai;
                    }
                    self.open_mission(index);
                }
            }
            (MenuState::Load(tab, Some(i)), "Load") => {
                let Some((entry, _)) = self.load_entries(tab).get(i).cloned() else {
                    return;
                };
                let i = match entry {
                    LoadEntry::Save(i) => i,
                    LoadEntry::Mission(index) => {
                        if self.category_missions(Category::Campaign).first() == Some(&index) {
                            self.queue_video(crate::video::OPENING);
                        }
                        self.carry = None;
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
                self.menu = None;
                self.in_game = true;
            }
            (MenuState::Briefing, "Cancel")
            | (MenuState::Missions(..), "Cancel")
            | (MenuState::Load(..), "Cancel") => self.menu = Some(MenuState::Main),
            (MenuState::Outcome(_), "Next mission") => {
                if let Some(next) = self.next_mission() {
                    // The campaign goes on with what was learned.
                    self.carry =
                        crate::scene::is_campaign(&self.missions[next]).then(|| self.scene.carry());
                    self.open_mission(next);
                }
            }
            (MenuState::Outcome(_), "Try again") => self.open_mission(self.mission_index),
            (_, "Main menu") | (_, "Back") => self.menu = Some(MenuState::Main),
            _ => {}
        }
    }

    /// Renders a front-end screen (`main`, `missions`, `load`, `briefing`,
    /// `victory`, `defeat`) into `frame`, for screenshots.
    pub fn draw_menu_screen(&mut self, which: &str, frame: &mut Frame) -> Result<()> {
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
            "missions" => MenuState::Missions(Category::Campaign, Some(1)),
            "custom" => MenuState::Missions(Category::Single, Some(0)),
            "load" => {
                self.refresh_panels();
                MenuState::Load(LoadTab::Scenario, Some(0))
            }
            "briefing" => MenuState::Briefing,
            "victory" => MenuState::Outcome(crate::scene::Outcome::Victory),
            "defeat" => MenuState::Outcome(crate::scene::Outcome::Defeat),
            "best" => MenuState::Best,
            "battles" => {
                self.list_first = 10;
                MenuState::Missions(Category::Battle, Some(12))
            }
            "connect" => {
                self.player_name.clear();
                MenuState::Connect
            }
            "multiplayer" => {
                self.find_battle_maps();
                self.join_address = "192.168.1.20".into();
                MenuState::Multiplayer(Some(4))
            }
            other => anyhow::bail!("unknown menu screen {other}"),
        };
        if let MenuState::Missions(c, Some(i)) = state {
            if let Some(&index) = self.screen_missions(c).get(i) {
                self.preview = Some(Preview::load(
                    &self.data,
                    &mut self.caches,
                    &self.missions[index],
                    index,
                )?);
            }
        }
        let menus = self.menus.as_ref().context("menus are not installed")?;
        menus.draw(frame, &self.menu_page(state).view(hover));
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

    /// Esc on the front end: back a screen, or out of the main menu.
    fn menu_back(&mut self, event_loop: &ActiveEventLoop) {
        self.list_first = 0;
        self.menu = match self.menu {
            Some(MenuState::Main) if self.in_game => None,
            Some(MenuState::Main) => {
                event_loop.exit();
                return;
            }
            _ => Some(MenuState::Main),
        };
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
        let cell = self.cell_at(pos)?;
        let class = self.pending.and_then(economy::building_of_command)?;
        if self.selected_ship().is_some() {
            let ok = self.scene.check_ground(class, cell.0, cell.1).is_ok();
            return Some((cell, ok));
        }
        let settlement = self.selected?;
        let ok = self
            .scene
            .check_site(settlement, class, cell.0, cell.1)
            .is_ok();
        Some((cell, ok))
    }

    /// The map cell under a window position in the world view.
    fn cell_at(&self, pos: PhysicalPosition<f64>) -> Option<(i32, i32)> {
        let (screen, _) = self.placement();
        if !screen.contains(pos.x as f32, pos.y as f32) {
            return None;
        }
        let (wx, wy) = self
            .camera
            .screen_to_world(&screen, pos.x as f32, pos.y as f32);
        let t = openconquest_formats::map::TILE_SIZE as f32;
        Some(((wx / t).floor() as i32, (wy / t).floor() as i32))
    }

    /// A click on the object panel's slot cells: a settlement's stocked
    /// item is chosen, and chosen again picked up to load onto a ship
    /// (c_load.cpp 0x4f9da0).
    fn slot_click(&mut self, pos: PhysicalPosition<f64>) -> bool {
        if !self.show_interface || Panels::covers_bar(self.tab) || self.reloading().is_some() {
            return false;
        }
        let layout = self
            .interface
            .layout(self.window_size(), &self.camera.policy, &self.scene);
        let Some(i) = self
            .interface
            .slot_cell_at(&layout, pos.x as f32, pos.y as f32)
        else {
            return false;
        };
        let Some(site) = self.selected else {
            return false;
        };
        let Some(obj) = self.scene.objects.iter().find(|o| o.id == site) else {
            return false;
        };
        let cells = crate::ui::settlement_slots(&self.scene, obj);
        let Some(Some(cell)) = cells.get(i) else {
            return true;
        };
        // Places 3 and 7 are the armour and goods stores.
        let item = [0, 1, 2, 99, 3, 4, 5, 99, 6, 7, 8]
            .get(i)
            .copied()
            .unwrap_or(99);
        if cell.count == 0 || item == 99 || !self.scene.is_own(obj) {
            self.chosen_cell = None;
            return true;
        }
        if self.chosen_cell == Some((site, i)) {
            self.carrying = Some((site, item));
            self.say(format!(
                "Choose a ship alongside to take the {}",
                crate::combat::ITEM_NAMES[item as usize]
            ));
        } else {
            self.chosen_cell = Some((site, i));
        }
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
        if !self.show_interface
            || Panels::covers_bar(self.tab)
            || !(matches!(self.page, Page::Structures | Page::Ground) || self.yard_selected())
            || self.selection().is_empty()
        {
            return false;
        }
        let layout = self
            .interface
            .layout(self.window_size(), &self.camera.policy, &self.scene);
        let (x, y, w, h) = self.interface.arming_rect(&layout);
        let (px, py) = (pos.x as f32, pos.y as f32);
        px >= x && py >= y && px < x + w && py < y + h
    }

    /// The command slot under a window position, if the interface is shown.
    fn button_at(&self, pos: PhysicalPosition<f64>) -> Option<usize> {
        if !self.show_interface || Panels::covers_bar(self.tab) || self.reloading().is_some() {
            return None;
        }
        let layout = self
            .interface
            .layout(self.window_size(), &self.camera.policy, &self.scene);
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

    /// The menus' music in the front-end menus (not over a game, nor while
    /// a video plays), the game's music otherwise.
    fn choose_music(&mut self) {
        let want =
            self.menu.is_some() && !self.in_game && self.video.is_none() && !self.no_menu_music;
        if want == self.menu_music.is_some() {
            return;
        }
        if want {
            if let (Some(audio), Some(sounds)) = (self.audio.as_mut(), self.data.sounds.as_ref()) {
                self.menu_music = audio.loop_sound(sounds, "mus_menu");
                let muted = audio.muted;
                if let Some(s) = &self.menu_music {
                    s.set_volume(if muted { 0.0 } else { self.music_volume });
                }
            }
            if self.menu_music.is_none() {
                self.no_menu_music = true;
                return;
            }
        } else {
            self.menu_music = None;
        }
        if let Some(m) = &self.music {
            m.set_paused(want);
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
        for (name, x, y) in sounds {
            // One of each sound per frame, and not too many at once.
            if played.contains(&name) || played.len() >= 8 {
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
                .category_missions(Category::Campaign)
                .into_iter()
                .map(|i| (LoadEntry::Mission(i), self.mission_titles[i].clone()))
                .collect();
        }
        self.saves
            .iter()
            .enumerate()
            .filter(|(_, e)| crate::scene::is_campaign(&e.mission) == (tab == LoadTab::Scenario))
            .map(|(i, e)| (LoadEntry::Save(i), e.label.clone()))
            .collect()
    }

    /// The mission of a load screen entry.
    fn entry_mission(&self, entry: LoadEntry) -> Option<usize> {
        match entry {
            LoadEntry::Mission(i) => Some(i),
            LoadEntry::Save(i) => {
                let name = self.saves.get(i)?.mission.to_lowercase();
                self.missions.iter().position(|m| *m == name)
            }
        }
    }

    /// Where text in the big menu font starts to be centred on `x`.
    fn centred(&self, text: &str, x: f32) -> f32 {
        self.menus
            .as_ref()
            .map_or(x, |m| x - m.big.measure(text) as f32 / 2.0)
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    /// A right click on command button `slot` (0x4f46c0): on a marine or
    /// weapon button, takes the last one ordered off the queue (message
    /// 0x3c), its price paid back.
    fn cancel_button(&mut self, slot: usize) {
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

    fn press_button(&mut self, slot: usize) {
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
                None => format!("{}: not available yet", entry.command.name()),
            };
            // The original only greys the button: no sound (its "deny"
            // sound, event 319, is the sting for an enemy ship sunk).
            self.say(reason);
            return;
        }
        if let Some(kind) = ship_kind {
            if entry.progress.is_some() {
                return;
            }
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
            // From a settlement: onto the ship alongside it.
            Command::SELL_ITEM => {
                let chosen = self.chosen_cell.filter(|&(site, _)| site == id);
                let item = chosen.and_then(|(_, i)| {
                    [0u8, 1, 2, 99, 3, 4, 5, 99, 6, 7, 8]
                        .get(i)
                        .copied()
                        .filter(|&k| k != 99)
                });
                match item {
                    Some(item) => {
                        if let Err(e) = self.issue(Order::SellItem { site: id, item }) {
                            self.say(e);
                        }
                        self.chosen_cell = None;
                    }
                    None => self.say("Choose an item in the stock to sell"),
                }
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
            Command::LOAD if obj_is_ship => {
                self.pending = Some(Command::LOAD);
                self.say("Choose your port, shipyard, dock or ship to reload at");
            }
            Command::LOAD => match self.issue(Order::LoadCrew {
                ship: self.scene.ship_alongside(id).unwrap_or(id),
            }) {
                Ok(n) if self.net.is_none() => {
                    self.say(format!("{n} marines and weapons came aboard"))
                }
                Ok(_) => {}
                Err(e) => self.say(e),
            },
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
                    match self.issue(Order::LearnSpell { temple }) {
                        Ok(_) => self.say("The priests study a new spell"),
                        Err(e) => self.say(e),
                    }
                }
            }
            c if Spell::of_command(c) == Some(Spell::Wind) => self.page = Page::Wind,
            c if Spell::of_command(c) == Some(Spell::Flame) => match self.issue(Order::CastFlame) {
                Ok(_) => self.say("Your weapons burn hotter"),
                Err(e) => self.say(e),
            },
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
        let window = self.window_size();
        if self.show_interface {
            let layout = self.interface.layout(window, &self.camera.policy, &self.scene);
            (layout.view, layout.minimap)
        } else {
            (
                Screen::full(window.0, window.1),
                MinimapRect::overlay(&self.scene, window),
            )
        }
    }

    fn update_title(&self) {
        if let Some(g) = &self.gfx {
            let s = &self.scene;
            g.window.set_title(&format!(
                "OpenConquest — {} ({}x{}) — PgUp/PgDn: mission · Tab: interface · F11: fullscreen",
                s.mission, s.grid.width, s.grid.height
            ));
        }
    }

    fn switch_mission(&mut self, delta: isize) {
        if self.net.is_some() {
            self.say("Not in a multiplayer game");
            return;
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
                self.scene.fog = self.fog;
                self.selected = None;
                self.group.clear();
                self.pending = None;
                self.mission_index = next;
                self.center_pending = true;
                self.briefing = true;
                self.refresh_panels();
                self.update_title();
            }
            Err(e) => eprintln!("failed to load {}: {e:#}", self.missions[next]),
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
        match self.scene.save().write(&path) {
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
            Ok((save.mission, scene))
        });
        match loaded {
            Ok((mission, scene)) => {
                self.scene = scene;
                self.scene.fog = self.fog;
                if let Some(i) = self.missions.iter().position(|m| *m == mission) {
                    self.mission_index = i;
                }
                self.selected = None;
                self.group.clear();
                self.pending = None;
                self.page = Page::Main;
                self.briefing = false;
                self.refresh_panels();
                self.update_title();
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
        if self.show_interface && Panels::covers_bar(self.tab) {
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

    fn left_click(&mut self, pos: PhysicalPosition<f64>) {
        if self.panel_click(pos) {
            return;
        }
        if self.load_click(pos) {
            return;
        }
        if self.slot_click(pos) {
            return;
        }
        // A unit picked up from a settlement's stock goes aboard the ship
        // clicked.
        if let Some((site, item)) = self.carrying.take() {
            self.status = None;
            match self.object_at(pos) {
                Some(ship) => match self.issue(Order::LoadItem { site, ship, item }) {
                    Ok(_) => self.confirm(pos),
                    Err(e) => self.say(e),
                },
                None => self.say("Choose a ship alongside the settlement"),
            }
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
                None => Err("Choose your port, shipyard, dock or ship".into()),
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
        if self.pending.is_none() {
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
        let (screen, _) = self.placement();
        if !screen.contains(pos.x as f32, pos.y as f32) {
            return;
        }
        let (wx, wy) = self
            .camera
            .screen_to_world(&screen, pos.x as f32, pos.y as f32);
        let _ = (wx, wy);
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
        // Over a quick help link, the hypertext pointer (0x51728a).
        if self.tab == Tab::Help {
            let link = self
                .bar_point(pos)
                .and_then(|p| self.interface.panels.hit(&self.panel_view(), p));
            if let Some(PanelHit::HelpLink(_)) = link {
                return (P::Hypertext, false);
            }
        }
        let (screen, _) = self.placement();
        if !screen.contains(pos.x as f32, pos.y as f32) {
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
        // A unit picked up from a settlement's stock: onto one of own ships.
        if self.carrying.is_some() {
            return (P::Load, !target.is_some_and(|o| own(o) && o.ship.is_some()));
        }
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
                Command::LOAD => (
                    P::Load,
                    !target.is_some_and(|o| own(o) && is_load_target(o, selected)),
                ),
                c if economy::building_of_command(c).is_some() => {
                    (P::Build, !self.site_at(pos).is_some_and(|s| s.1))
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
        if self.scene.is_own(t) && is_load_target(t, Some(s)) && sh.kind < 10 {
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
    fn step_pointer(&mut self, pos: PhysicalPosition<f64>) {
        if self.carrying.take().is_some() || self.pending.take().is_some() {
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
            self.chosen_cell = None;
        }
    }

    /// Whether `id` is one of the player's ships.
    fn own_ship(&self, id: u32) -> bool {
        self.scene.object_index(id).is_some_and(|i| {
            let o = &self.scene.objects[i];
            o.ship.is_some() && self.scene.is_own(o)
        })
    }

    /// Ends a left click or drag in the world view: a click selects what
    /// is under it (with Shift, adds or removes an own ship), a drag every
    /// own ship inside the box (with Shift, added to the selection).
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
        let dragged = (from.x - to.x).abs() + (from.y - to.y).abs() > BOX_THRESHOLD;
        let mut chosen: Vec<u32> = if dragged {
            let (screen, _) = self.placement();
            let (x0, x1) = (from.x.min(to.x) as f32, from.x.max(to.x) as f32);
            let (y0, y1) = (from.y.min(to.y) as f32, from.y.max(to.y) as f32);
            let scale = self.camera.scale(&screen);
            self.scene
                .objects
                .iter()
                .filter(|o| o.ship.is_some() && self.scene.is_own(o))
                .filter(|o| {
                    let sx = screen.x as f32 + (o.x - self.camera.x) * scale;
                    let sy = screen.y as f32 + (o.y - self.camera.y) * scale;
                    (x0..=x1).contains(&sx) && (y0..=y1).contains(&sy)
                })
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
        let (screen, minimap) = self.placement();
        let (sx, sy) = (pos.x as f32, pos.y as f32);
        if screen.contains(sx, sy) {
            Some(self.camera.screen_to_world(&screen, sx, sy))
        } else if minimap.contains(sx, sy) {
            Some(minimap.to_world(sx, sy))
        } else {
            None
        }
    }

    /// The object under a window position in the world view.
    fn object_at(&self, pos: PhysicalPosition<f64>) -> Option<u32> {
        let (screen, _) = self.placement();
        if !screen.contains(pos.x as f32, pos.y as f32) {
            return None;
        }
        let (wx, wy) = self
            .camera
            .screen_to_world(&screen, pos.x as f32, pos.y as f32);
        hit_test(&self.scene, wx, wy, self.time_ms)
    }

    fn right_click(&mut self, pos: PhysicalPosition<f64>) {
        // A unit picked up is put back.
        if self.carrying.take().is_some() {
            self.status = None;
            return;
        }
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
        let (screen, minimap) = self.placement();
        let (sx, sy) = (pos.x as f32, pos.y as f32);
        let target = if screen.contains(sx, sy) {
            self.camera.screen_to_world(&screen, sx, sy)
        } else if minimap.contains(sx, sy) {
            minimap.to_world(sx, sy)
        } else {
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
                    // The music waits.
                    if let Some(m) = &self.music {
                        m.set_volume(0.0);
                    }
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
        if let (Some(outcome), None) = (self.scene.outcome, self.outcome_at) {
            self.outcome_at = Some(self.scene.clock);
            // A win goes on the best results.
            self.last_win = (outcome == crate::scene::Outcome::Victory).then(|| {
                let mission = self.missions[self.mission_index].clone();
                let new = self.records.record(&mission, self.scene.clock);
                if new {
                    if let Err(e) = self.records.save(&self.data.save_dir().join("best.json")) {
                        eprintln!("could not save the best results: {e}");
                    }
                }
                (self.scene.clock, new)
            });
        }
        // The end screen follows a few seconds after the mission is decided.
        if let (Some(outcome), Some(at), None) = (self.scene.outcome, self.outcome_at, self.menu) {
            if self.scene.clock > at + 3.0 && self.menus.is_some() {
                self.menu = Some(MenuState::Outcome(outcome));
                self.in_game = false;
                // The story's end after the last campaign mission is won.
                let campaign =
                    Category::of(&self.missions[self.mission_index]) == Some(Category::Campaign);
                if outcome == crate::scene::Outcome::Victory
                    && campaign
                    && self.next_mission().is_none()
                {
                    self.queue_video(crate::video::ENDING);
                }
            }
        }
        if self.menu == Some(MenuState::Waiting) {
            self.run_lobby();
        }
        if self.net.is_some() {
            // Multiplayer games run at one speed and do not pause, menus
            // open or not.
            self.run_turns(dt);
        } else if !self.briefing && !self.paused && self.menu.is_none() {
            self.scene.update(
                dt * self.settings.config.speed(),
                &self.data,
                &mut self.caches,
            );
        }
        self.play_scene_sounds();
        self.choose_music();
        if let Some(m) = self.music.as_mut() {
            m.keep_playing();
        }
        let latest = std::mem::take(&mut self.scene.messages).pop();
        if let Some(msg) = latest {
            self.say(msg);
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
                None => {
                    let (x, y) = self.scene.start_view();
                    self.camera.center_on(x, y, &screen, map);
                }
            }
        }
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        let held = |k: &[KeyCode]| k.iter().any(|k| self.keys.contains(k));
        let mut keyed = false;
        if held(&[KeyCode::ArrowLeft, KeyCode::KeyA]) {
            dx -= 1.0;
        }
        if held(&[KeyCode::ArrowRight, KeyCode::KeyD]) {
            dx += 1.0;
        }
        if held(&[KeyCode::ArrowUp, KeyCode::KeyW]) {
            dy -= 1.0;
        }
        if held(&[KeyCode::ArrowDown, KeyCode::KeyS]) {
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
    }

    fn redraw(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.time_ms = (now - self.started).as_millis() as u64;
        self.update(dt);
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
            } else if let (Some(state), Some(menus)) = (self.menu, &self.menus) {
                let page = self.menu_page(state);
                let hover = self
                    .cursor
                    .map(|c| Menus::to_menu(window, c.x as f32, c.y as f32));
                menus.draw(&mut frame, &page.view(hover));
            } else {
                let site = self.cursor.and_then(|c| self.site_at(c));
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
                    ..self.view()
                }
                .draw(&mut frame);
            }
            if let Some(c) = self.cursor {
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
                    && self.show_interface
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
                if let (Some(MenuState::Connect), Some(_)) = (self.menu, &self.menus) {
                    if code == KeyCode::Backspace {
                        self.player_name.pop();
                        return;
                    }
                    let typed = event.text.as_deref().unwrap_or("");
                    if !typed.is_empty()
                        && !typed.chars().any(char::is_control)
                        && self.player_name.len() < 30
                    {
                        self.player_name.push_str(typed);
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
                if self.menu.is_some() && self.menus.is_some() {
                    if code == KeyCode::Escape {
                        self.menu_back(event_loop);
                    } else if code == KeyCode::F11 {
                        self.toggle_fullscreen();
                    }
                    return;
                }
                if code == KeyCode::KeyB {
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
                match code {
                    KeyCode::KeyS if command => self.quick_save(),
                    KeyCode::KeyO if command => self.quick_load(),
                    KeyCode::Escape if self.carrying.is_some() => {
                        self.carrying = None;
                        self.status = None;
                    }
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
                    KeyCode::Escape if self.menus.is_some() => self.menu = Some(MenuState::Main),
                    KeyCode::Escape => event_loop.exit(),
                    KeyCode::F11 => self.toggle_fullscreen(),
                    KeyCode::KeyP | KeyCode::Pause => self.paused = !self.paused,
                    KeyCode::KeyM => {
                        if let Some(a) = self.audio.as_mut() {
                            a.muted = !a.muted;
                            let muted = a.muted;
                            if let Some(m) = &self.music {
                                m.set_volume(if muted { 0.0 } else { self.music_volume });
                            }
                            if let Some(s) = &self.menu_music {
                                s.set_volume(if muted { 0.0 } else { self.music_volume });
                            }
                            self.say(if muted { "Sound off" } else { "Sound on" });
                        }
                    }
                    KeyCode::Enter if self.modifiers.alt_key() => self.toggle_fullscreen(),
                    KeyCode::PageDown => self.switch_mission(1),
                    KeyCode::PageUp => self.switch_mission(-1),
                    KeyCode::Tab => {
                        self.show_interface = !self.show_interface;
                        let (screen, _) = self.placement();
                        self.camera.clamp(&screen, map);
                    }
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
                if let Some(slider) = self.slider_drag {
                    if let Some(layout) = self.show_interface.then(|| {
                        self.interface
                            .layout(self.window_size(), &self.camera.policy, &self.scene)
                    }) {
                        let x = (position.x as f32 - layout.bar_x) / layout.scale;
                        let v =
                            self.interface
                                .panels
                                .slider_value(slider, &self.settings.config, x);
                        self.set_slider(slider, v);
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
                self.cursor = Some(position);
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
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
                                self.step_pointer(c);
                            }
                        }
                    }
                }
                (MouseButton::Left, ElementState::Pressed) if self.video.is_some() => {
                    self.skip_video(false);
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
                        self.left_click(c);
                    }
                }
                (MouseButton::Left, ElementState::Released) => {
                    self.load_pressed = None;
                    self.panel_pressed = None;
                    self.minimap_drag = false;
                    self.slider_drag = None;
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
                // On the front end the wheel scrolls lists.
                if let (Some(state), Some(_)) = (self.menu, &self.menus) {
                    let rows = self.menu_page(state).items.len();
                    let last = rows.saturating_sub(crate::menus::VISIBLE_ROWS);
                    self.wheel_rest += steps;
                    let whole = self.wheel_rest.trunc();
                    self.wheel_rest -= whole;
                    let step = -whole as isize;
                    self.list_first =
                        (self.list_first as isize + step).clamp(0, last as isize) as usize;
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

/// Whether a ship's Reload Object may take up `o` (0x4dc630, command 0x1f
/// with a target): an own port, shipyard or dock, or another ship.
fn is_load_target(o: &crate::objects::MapObject, ship: Option<&crate::objects::MapObject>) -> bool {
    use openconquest_formats::ObjectClass;
    match o.class {
        ObjectClass::Ship => ship.is_some_and(|s| s.id != o.id),
        ObjectClass::Building(c) => matches!(c, economy::PORT | economy::SHIPYARD | economy::DOCK),
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
