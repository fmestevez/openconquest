//! Window, input handling and the frame loop.

use std::collections::HashSet;
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
use crate::panels::{self, PanelHit, PanelView, Panels, Slider, Tab};
use crate::render::{hit_test, Frame, MinimapRect};
use crate::save::SaveGame;
use crate::scene::{Order, Scene};
use crate::text::TextRenderer;
use crate::ui::{GameView, Interface};
use crate::view::{Camera, Screen};

/// Keyboard scroll speed in original pixels per second.
const SCROLL_SPEED: f32 = 1400.0;
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
    /// Save slot (`panels::SLOTS` is the quick save).
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
}

impl Preview {
    fn load(data: &GameData, caches: &mut Caches, mission: &str, index: usize) -> Result<Self> {
        let archive = data.open_mission(mission)?;
        let grid = data.load_map(&archive, mission)?;
        let terrain = crate::terrain::Terrain::compose(data, &grid, &mut caches.tiles)?;
        let (w, h, pixels) = terrain.minimap(&crate::render::palette_lut(data, 0));
        let nation = |n: u32| match n {
            1 => "Greeks",
            3 => "Persians",
            _ => "Barbarians",
        };
        let players = openac_formats::players(&archive)
            .unwrap_or_default()
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
            briefing: openac_formats::briefing(&archive).unwrap_or_default(),
            players,
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
    labels: Vec<(String, f32, f32)>,
    checks: Vec<crate::menus::Check>,
    panels: Vec<crate::menus::Panel>,
    list_rect: Option<crate::menus::Rect>,
    picture: Option<(Rc<crate::assets::Image>, crate::menus::Rect)>,
    boxes: Vec<(crate::menus::Rect, Vec<String>)>,
    tabs: Vec<crate::menus::MenuTab>,
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
            labels: &self.labels,
            checks: &self.checks,
            panels: &self.panels,
            list_rect: self.list_rect,
            picture: self.picture.as_ref().map(|(i, r)| (&**i, *r)),
            boxes: &self.boxes,
            tabs: &self.tabs,
        }
    }
}

/// Player settings from the wrench panel, as slider positions 0..1.
#[derive(Debug, Clone, Copy)]
struct Settings {
    game_speed: f32,
    scroll_speed: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            game_speed: 1.0 / 3.0,
            scroll_speed: 0.5,
        }
    }
}

impl Settings {
    /// Simulation speed: 0.5x to 2x, normal at a third of the way.
    fn speed(&self) -> f32 {
        if self.game_speed <= 1.0 / 3.0 {
            0.5 + 1.5 * self.game_speed
        } else {
            1.0 + 1.5 * (self.game_speed - 1.0 / 3.0)
        }
    }

    /// Scrolling speed factor: about 0.35x to 2.8x, normal in the middle.
    fn scroll(&self) -> f32 {
        2f32.powf((self.scroll_speed - 0.5) * 3.0)
    }
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
    minimap_drag: bool,
    selected: Option<u32>,
    /// Other own ships selected with it.
    group: Vec<u32>,
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
    /// Labels of the save slots (`None`: empty).
    slot_labels: Vec<Option<String>>,
    quick_label: Option<String>,
    selected_slot: Option<usize>,
    help_text: Vec<String>,
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
                    .and_then(|a| openac_formats::briefing(&a).ok())
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
            minimap_drag: false,
            selected: None,
            group: Vec::new(),
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
            slot_labels: vec![None; panels::SLOTS],
            quick_label: None,
            selected_slot: None,
            help_text: Vec::new(),
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
            center_pending: true,
        };
        app.scene.fog = app.fog;
        app.start_music(soundfont.as_deref());
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
            .map(|(m, _)| m.as_str());
        GameView {
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
            hover_button: self.cursor.and_then(|c| self.button_at(c)),
            page: self.page,
            site: None,
            briefing: self.briefing,
            panel: Some(PanelView {
                tab: self.tab,
                slots: &self.slot_labels,
                selected_slot: self.selected_slot,
                game_speed: self.settings.game_speed,
                scroll_speed: self.settings.scroll_speed,
                volume: self.audio.as_ref().map_or(0.0, |a| a.volume),
                music: self.music.as_ref().map(|_| self.music_volume),
                help: &self.help_text,
            }),
            panel_hover: self.cursor.and_then(|c| self.bar_point(c)),
            paused: self.paused,
        }
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

    /// Handles a click on the bar's tabs and panels; true if it hit one.
    fn panel_click(&mut self, pos: PhysicalPosition<f64>) -> bool {
        let Some(p) = self.bar_point(pos) else {
            return false;
        };
        let Some(hit) = self.interface.panels.hit(self.tab, p) else {
            // Clicks elsewhere on an open panel do nothing.
            return Panels::covers_bar(self.tab) && p.1 < 558;
        };
        match hit {
            PanelHit::Tab(tab) => {
                self.tab = tab;
                self.refresh_panels();
            }
            PanelHit::Slot(i) => self.selected_slot = Some(i),
            PanelHit::Save => match self.selected_slot {
                Some(i) => match self.scene.save().write(&self.slot_path(i)) {
                    Ok(()) => {
                        self.say(format!("Saved in slot {}", i + 1));
                        self.refresh_panels();
                    }
                    Err(e) => self.say(format!("Could not save: {e:#}")),
                },
                None => self.say("Choose a slot"),
            },
            PanelHit::Load => match self.selected_slot {
                Some(i) if self.slot_labels[i].is_some() => self.load_game(&self.slot_path(i)),
                _ => self.say("Choose a saved game"),
            },
            PanelHit::Delete => {
                if let Some(i) = self
                    .selected_slot
                    .filter(|&i| self.slot_labels[i].is_some())
                {
                    let _ = std::fs::remove_file(self.slot_path(i));
                    self.say(format!("Slot {} cleared", i + 1));
                    self.refresh_panels();
                }
            }
            PanelHit::Restart => self.switch_mission(0),
            PanelHit::Quit => self.quit = true,
            PanelHit::Slider(slider, v) => {
                self.slider_drag = Some(slider);
                self.set_slider(slider, v);
            }
        }
        true
    }

    fn set_slider(&mut self, slider: Slider, v: f32) {
        match slider {
            Slider::GameSpeed => self.settings.game_speed = v,
            Slider::ScrollSpeed => self.settings.scroll_speed = v,
            Slider::Volume => {
                if let Some(a) = self.audio.as_mut() {
                    a.volume = v;
                }
            }
            Slider::Music => {
                self.music_volume = v;
                let muted = self.audio.as_ref().is_some_and(|a| a.muted);
                if let Some(m) = &self.music {
                    m.set_volume(if muted { 0.0 } else { v });
                }
                if let Some(s) = &self.menu_music {
                    s.set_volume(if muted { 0.0 } else { v });
                }
            }
        }
    }

    fn slot_path(&self, i: usize) -> std::path::PathBuf {
        self.data.save_dir().join(format!("slot{}.json", i + 1))
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
            labels: Vec::new(),
            checks: Vec::new(),
            panels: Vec::new(),
            list_rect: None,
            picture: None,
            boxes: Vec::new(),
            tabs: Vec::new(),
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
            // "Select custom mission" as the original lays it out (panel
            // frames measured on its screen): the map of the chosen
            // mission, its description, the missions and its players;
            // Start and Cancel below.
            MenuState::Missions(category, selected) => {
                let list = self.category_missions(category);
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
            // "Select mission to load" as the original lays it out: the
            // map and description of the chosen entry, the list, the tabs
            // Scenario, Custom and Start from... at (321, 529), (451, 529)
            // and (581, 529), Load and Cancel below.
            MenuState::Load(tab, selected) => {
                let entries = self.load_entries(tab);
                let chosen = selected.and_then(|i| entries.get(i)).map(|e| e.0);
                let preview = self
                    .preview
                    .as_ref()
                    .filter(|p| chosen.and_then(|e| self.entry_mission(e)) == Some(p.index));
                let panel = |rect, title: &str, arrows| crate::menus::Panel {
                    rect,
                    title: title.into(),
                    arrows,
                };
                let size = preview.map_or((0, 0), |p| p.size);
                let list = panel((2.0, 216.0, 792.0, 313.0), "Missions", true);
                let about = panel((195.0, 27.0, 599.0, 186.0), "Mission description", true);
                let mut boxes = Vec::new();
                if let Some(p) = preview {
                    boxes.push((about.inner(), p.briefing.clone()));
                }
                let heading = "Select mission to load";
                let tab_at = |caption: &str, x, wide, t| crate::menus::MenuTab {
                    caption: caption.into(),
                    x,
                    y: 529.0,
                    wide,
                    open: tab == t,
                };
                MenuPage {
                    backdrop: Backdrop::TitleDim,
                    labels: vec![(heading.into(), self.centred(heading, 494.0), 6.0)],
                    items: entries.into_iter().map(|e| e.1).collect(),
                    selected,
                    list_rect: Some(list.inner()),
                    picture: preview
                        .and_then(|p| p.map.clone())
                        .map(|m| (m, (8.0, 29.0, 153.0, 153.0))),
                    boxes,
                    panels: vec![
                        panel(
                            (2.0, 3.0, 190.0, 210.0),
                            &format!("Map ({}x{})", size.0, size.1),
                            false,
                        ),
                        about,
                        list,
                    ],
                    tabs: vec![
                        tab_at("Scenario", 321.0, false, LoadTab::Scenario),
                        tab_at("Custom", 451.0, false, LoadTab::Custom),
                        tab_at("Start from...", 581.0, true, LoadTab::StartFrom),
                    ],
                    buttons: vec![
                        Button {
                            caption: "Load".into(),
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
            // The original's multiplayer screen (frames measured on it):
            // "Player's name" over a text field, "Select communication
            // channel" over the ways to connect (OpenAC has its own, over
            // the internet, instead of DirectPlay's), New, Join and Cancel.
            MenuState::Connect => {
                let name = crate::menus::Panel {
                    rect: (29.0, 225.0, 478.0, 60.0),
                    title: "Player's name".into(),
                    arrows: false,
                };
                let channel = crate::menus::Panel {
                    rect: (29.0, 290.0, 478.0, 193.0),
                    title: "Select communication channel".into(),
                    arrows: true,
                };
                let (nx, ny, nw, nh) = name.inner();
                let at = |caption: &str, x, shape| Button {
                    caption: caption.into(),
                    x,
                    y: 510.0,
                    shape,
                };
                MenuPage {
                    backdrop: Backdrop::Title,
                    list_rect: Some(channel.inner()),
                    items: vec!["Internet TCP/IP connection (OpenAC)".into()],
                    selected: Some(0),
                    boxes: vec![(
                        (nx, ny + 2.0, nw, nh),
                        vec![format!("{}_", self.player_name)],
                    )],
                    panels: vec![name, channel],
                    buttons: vec![
                        at("New", 60.0, 2),
                        at("Join", 215.0, 3),
                        at("Cancel", 365.0, 2),
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
        let players = openac_formats::players(&archive)?;
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
                if state == MenuState::Briefing && self.difficulty != i {
                    self.difficulty = i;
                    self.open_mission(self.mission_index);
                }
                return;
            }
            MenuHit::Item(i) => {
                self.menu = Some(match state {
                    MenuState::Missions(c, _) => {
                        if let Some(&index) = self.category_missions(c).get(i) {
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
                if let Some(&index) = self.category_missions(c).get(i) {
                    self.carry = None;
                    self.open_mission(index);
                }
            }
            (MenuState::Load(tab, Some(i)), "Load") => {
                let Some((entry, _)) = self.load_entries(tab).get(i).cloned() else {
                    return;
                };
                let i = match entry {
                    LoadEntry::Save(slot) => slot,
                    LoadEntry::Mission(index) => {
                        if self.category_missions(Category::Campaign).first() == Some(&index) {
                            self.queue_video(crate::video::OPENING);
                        }
                        self.carry = None;
                        self.open_mission(index);
                        return;
                    }
                };
                let path = if i < panels::SLOTS {
                    self.slot_path(i)
                } else {
                    self.quick_save_path()
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
            "custom" => MenuState::Missions(Category::Single, Some(2)),
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
                self.player_name = "Jason".into();
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
            if let Some(&index) = self.category_missions(c).get(i) {
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
                let players = openac_formats::players(&archive).ok()?;
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
            .and_then(|a| openac_formats::players(&a).ok())
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
        self.slot_labels = (0..panels::SLOTS)
            .map(|i| {
                SaveGame::read(&self.slot_path(i)).ok().map(|s| {
                    let t = s.clock as u32;
                    format!("{} {}:{:02}", s.mission.to_uppercase(), t / 60, t % 60)
                })
            })
            .collect();
        self.quick_label = SaveGame::read(&self.quick_save_path()).ok().map(|s| {
            let t = s.clock as u32;
            format!(
                "{} {}:{:02} (quick save)",
                s.mission.to_uppercase(),
                t / 60,
                t % 60
            )
        });
        let mut help: Vec<String> = Vec::new();
        let goals: Vec<&str> = (0..crate::scene::OBJECTIVES.len())
            .filter(|&i| self.scene.conditions[i])
            .map(|i| crate::scene::OBJECTIVES[i].0)
            .collect();
        if !goals.is_empty() {
            help.push(format!("Objectives: {}.", goals.join(", ")));
        }
        help.extend(
            self.scene
                .briefing
                .iter()
                .filter(|p| !p.is_empty())
                .cloned(),
        );
        self.help_text = help;
    }

    /// The building site under a window position while a building is being
    /// placed, and whether it is allowed.
    fn site_at(&self, pos: PhysicalPosition<f64>) -> Option<((i32, i32), bool)> {
        let cell = self.cell_at(pos)?;
        if self.pending == Some(Command::BUILD_GROUND) {
            let ok = self
                .scene
                .check_ground(economy::SETTLEMENT, cell.0, cell.1)
                .is_ok();
            return Some((cell, ok));
        }
        let class = self.pending.and_then(economy::building_of_command)?;
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
        let t = openac_formats::map::TILE_SIZE as f32;
        Some(((wx / t).floor() as i32, (wy / t).floor() as i32))
    }

    /// The command slot under a window position, if the interface is shown.
    fn button_at(&self, pos: PhysicalPosition<f64>) -> Option<usize> {
        if !self.show_interface || Panels::covers_bar(self.tab) {
            return None;
        }
        let layout = self
            .interface
            .layout(self.window_size(), &self.camera.policy, &self.scene);
        self.interface
            .button_at(&layout, pos.x as f32, pos.y as f32)
    }

    /// Starts the music if there is sound and a SoundFont.
    fn start_music(&mut self, soundfont: Option<&std::path::Path>) {
        let Some(audio) = &self.audio else { return };
        // The CD's own music, if it has been ripped.
        let cd_dir = self
            .cd_audio
            .clone()
            .unwrap_or_else(|| self.data.root().join("cdaudio"));
        let tracks = crate::music::cd_tracks(&cd_dir);
        if !tracks.is_empty() {
            match Music::from_files(&audio.handle, tracks) {
                Ok(music) => {
                    music.set_volume(if audio.muted { 0.0 } else { self.music_volume });
                    self.music = Some(music);
                    return;
                }
                Err(e) => eprintln!("no CD music: {e:#}"),
            }
        }
        let Some(font) = crate::music::find_soundfont(soundfont) else {
            eprintln!("no music: give a General MIDI SoundFont with --soundfont");
            return;
        };
        let tunes = crate::music::playlist(&self.data.sound_dir());
        match Music::new(&audio.handle, &font, &tunes) {
            Ok(music) => {
                music.set_volume(if audio.muted { 0.0 } else { self.music_volume });
                self.music = Some(music);
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
        let labels = self
            .slot_labels
            .iter()
            .enumerate()
            .filter_map(|(i, l)| Some((i, l.clone()?)))
            .chain(self.quick_save_label().map(|l| (panels::SLOTS, l)));
        labels
            .filter(|(_, l)| {
                let mission = l.split_whitespace().next().unwrap_or("").to_lowercase();
                crate::scene::is_campaign(&mission) == (tab == LoadTab::Scenario)
            })
            .map(|(i, l)| (LoadEntry::Save(i), l))
            .collect()
    }

    /// The mission of a load screen entry.
    fn entry_mission(&self, entry: LoadEntry) -> Option<usize> {
        match entry {
            LoadEntry::Mission(i) => Some(i),
            LoadEntry::Save(slot) => {
                let label = if slot < panels::SLOTS {
                    self.slot_labels.get(slot)?.clone()?
                } else {
                    self.quick_save_label()?
                };
                let name = label.split_whitespace().next()?.to_lowercase();
                self.missions.iter().position(|m| *m == name)
            }
        }
    }

    /// The quick save, as the save slots are labelled (read by
    /// `refresh_panels`).
    fn quick_save_label(&self) -> Option<String> {
        self.quick_label.clone()
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
                self.page = Page::Structures;
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
            Command::LOAD => match self.issue(Order::LoadCrew { ship: id }) {
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
                            openac_formats::ObjectClass::Building(c) => Temple::of_class(c),
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
            Command::BUILD_GROUND => {
                self.pending = Some(Command::BUILD_GROUND);
                self.say("Choose a shore to found a settlement on");
            }
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
    fn placement(&self) -> (Screen, MinimapRect) {
        let (screen, minimap, _) = self.view().placement(self.window_size());
        (screen, minimap)
    }

    fn update_title(&self) {
        if let Some(g) = &self.gfx {
            let s = &self.scene;
            g.window.set_title(&format!(
                "OpenAC — {} ({}x{}) — PgUp/PgDn: mission · Tab: interface · F11: fullscreen",
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

    /// Saves the game in progress (F5).
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

    /// Loads the quick save (F9).
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
        if let Some(slot) = self.button_at(pos) {
            self.press_button(slot);
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
        if self.pending == Some(Command::BUILD_GROUND) {
            let (Some(ship), Some(cell)) = (self.selected, self.cell_at(pos)) else {
                return;
            };
            match self.issue(Order::Found {
                ship,
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
    fn pointer(&self, pos: PhysicalPosition<f64>) -> (crate::cursor::Pointer, bool) {
        use crate::cursor::Pointer as P;
        if self.video.is_some() || self.menu.is_some() || self.button_at(pos).is_some() {
            return (P::Standard, false);
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
                Command::WHALE => (P::Whale, !deposit(Resource::Leviathan)),
                Command::TREASURE => (
                    P::Treasure,
                    !target.is_some_and(|o| {
                        o.treasure.is_some() || o.class == openac_formats::ObjectClass::HeroHouse
                    }),
                ),
                Command::REPAIR_SHIP | Command::REPAIR_BUILDING => {
                    (P::Repair, !target.is_some_and(own))
                }
                Command::BUILD_GROUND => (P::Build, !self.site_at(pos).is_some_and(|s| s.1)),
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
                Some(o) if o.class == openac_formats::ObjectClass::HeroHouse => {
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
                    openac_formats::ObjectClass::Treasure | openac_formats::ObjectClass::HeroHouse
                )
            });
            if takeable && self.own_ship(id) {
                if let Err(e) = self.issue(Order::Take { ship: id, target }) {
                    self.say(e);
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
            self.scene
                .update(dt * self.settings.speed(), &self.data, &mut self.caches);
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
            let (x, y) = self.scene.start_view();
            self.camera.center_on(x, y, &screen, map);
        }
        let (mut dx, mut dy) = (0.0f32, 0.0f32);
        let held = |k: &[KeyCode]| k.iter().any(|k| self.keys.contains(k));
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
        let speed =
            SCROLL_SPEED * self.settings.scroll() * self.camera.policy.base_scale(window.1) * dt;
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
            .with_title("OpenAC")
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
                if code == KeyCode::F1 {
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
                match code {
                    KeyCode::Escape if self.pending.is_some() => {
                        self.pending = None;
                        self.status = None;
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
                    KeyCode::F5 => self.quick_save(),
                    KeyCode::KeyP | KeyCode::Pause => self.paused = !self.paused,
                    KeyCode::F9 => self.quick_load(),
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
                    KeyCode::Digit0 => {
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
                        let v = self.interface.panels.slider_value(slider, x);
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
                            self.right_click(c);
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
