//! OpenConquest: an open source reimplementation of *Ancient Conquest*.

mod app;
mod assets;
mod audio;
mod clock;
mod combat;
mod commands;
mod cursor;
mod economy;
mod hotkeys;
mod info_panel;
mod launch;
mod load_panel;
mod magic;
mod menus;
mod music;
mod nav;
mod net;
mod object_info;
mod objects;
mod panels;
mod permits;
mod records;
mod render;
mod save;
mod scene;
mod terrain;
mod text;
mod ui;
mod video;
mod view;
mod weather;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Parser;

use crate::assets::GameData;
use crate::objects::Caches;
use crate::render::Frame;
use crate::scene::Scene;
use crate::text::TextRenderer;
use crate::ui::{GameView, Interface};
use crate::view::{Camera, ScalePolicy};

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Game data directory (an installed Ancient Conquest, or one made by
    /// `openconquest-extract install`). Without it OpenConquest uses the one chosen
    /// last time, or asks for the CD or install folder.
    #[arg(long, env = "OPENCONQUEST_DATA")]
    data: Option<PathBuf>,
    /// Mission to open (e.g. miss000, single03, battle12).
    #[arg(long)]
    mission: Option<String>,
    /// Host a multiplayer game of `--mission` (battle01 by default) for
    /// `--players` players, waiting for them to join on `--port`.
    #[arg(long)]
    host: bool,
    /// Join the multiplayer game hosted at this address (host or
    /// host:port).
    #[arg(long, value_name = "ADDRESS")]
    join: Option<String>,
    #[arg(long, default_value_t = net::PORT)]
    port: u16,
    #[arg(long, default_value_t = 2)]
    players: usize,
    /// Continue a saved game (Ctrl+S saves to <data>/saves/quick.json).
    #[arg(long, value_name = "FILE")]
    load: Option<PathBuf>,
    /// List the available missions and exit.
    #[arg(long)]
    list_missions: bool,
    /// Screen pixels per game pixel. By default derived from the window
    /// height, so the view is as tall as the original 600-line screen.
    #[arg(long)]
    scale: Option<f32>,
    /// Restrict scaling to whole multiples for crisp pixels.
    #[arg(long)]
    integer_scale: bool,
    /// Start in borderless fullscreen.
    #[arg(long)]
    fullscreen: bool,
    /// Scroll when the cursor touches the window edge (always on in fullscreen).
    #[arg(long)]
    edge_scroll: bool,
    /// General MIDI SoundFont (.sf2) for the music; common install places
    /// are tried when it is not given.
    #[arg(long, env = "OPENCONQUEST_SOUNDFONT", value_name = "SF2")]
    soundfont: Option<PathBuf>,
    /// Folder of the game CD's audio tracks ripped by you (Track02 to
    /// Track09 as WAV, FLAC or Ogg); played instead of the MIDI music.
    /// Defaults to `<data>/cdaudio`.
    #[arg(long, env = "OPENCONQUEST_CD_AUDIO", value_name = "DIR")]
    cd_audio: Option<PathBuf>,
    /// Skip the company logos and title sequence at start-up.
    #[arg(long)]
    no_intro: bool,
    /// Play without fog of war (the whole map visible from the start).
    #[arg(long)]
    no_fog: bool,
    /// How hard the computer players of the campaign play: how many ships
    /// they gather before raiding, and how soon.
    #[arg(long, value_enum, default_value_t = Difficulty::Normal)]
    difficulty: Difficulty,
    /// Start with sound off (M toggles it).
    #[arg(long)]
    mute: bool,
    /// Initial window size, e.g. 1600x900 (logical pixels).
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    window: (u32, u32),
    /// Render one frame of the given size (e.g. 1920x1080) to a PNG and exit.
    #[arg(long, value_name = "PNG")]
    screenshot: Option<PathBuf>,
    /// Hide the original interface in the screenshot.
    #[arg(long)]
    no_interface: bool,
    /// Map cell to centre the screenshot on, e.g. 12,30 (default: map centre).
    #[arg(long, value_parser = parse_size, value_name = "X,Y")]
    focus: Option<(u32, u32)>,
    /// Render a front-end screen instead of the game (main, missions, load,
    /// briefing, victory, defeat).
    #[arg(long, hide = true)]
    menu_shot: Option<String>,
    /// Menu page of the selected object to show (behaviour, research,
    /// structures, wind).
    #[arg(long, hide = true)]
    page: Option<String>,
    /// Show the pause sign in the screenshot.
    #[arg(long, hide = true)]
    paused: bool,
    /// Bar panel to show in the screenshot.
    #[arg(long, value_enum, default_value = "object")]
    panel: panels::Tab,
    /// Quick help page to show on the help panel (instead of the mission
    /// objectives).
    #[arg(long, hide = true)]
    help_page: Option<usize>,
    /// Run the simulation for this many seconds before taking the screenshot.
    #[arg(long, default_value_t = 0.0, value_name = "SECONDS")]
    simulate: f32,
    /// Command slot shown hovered in the screenshot (with the player's
    /// first ship selected).
    #[arg(long, hide = true)]
    hover_button: Option<usize>,
    /// Debugging: before simulating, order the focused ship to attack the
    /// nearest ship of another player.
    #[arg(long)]
    skirmish: bool,
    /// Debugging: before simulating, order the player's merchant nearest
    /// the focus to gather the nearest amber (or fish).
    #[arg(long, hide = true, value_name = "amber|fish")]
    gather: Option<String>,
    /// Debugging: before simulating, send the player's ship nearest the
    /// focus to this cell.
    #[arg(long, hide = true, value_parser = parse_size, value_name = "X,Y")]
    sail: Option<(u32, u32)>,
    /// Debugging: before simulating, bring the nearest enemy ship two cells
    /// from the ship nearest the focus and order that one to board or ram
    /// it (to see the hooks, walkway and fight, or the ram).
    #[arg(long, hide = true, value_name = "board|ram")]
    close: Option<String>,
    /// Centre the screenshot where the mission's view starts.
    #[arg(long, hide = true)]
    at_start: bool,
    /// Debugging: before simulating, send the player's ship nearest the
    /// focus to reload at the own port, shipyard or dock nearest it, and
    /// show the building's load panel (with --simulate long enough for the
    /// ship to get there).
    #[arg(long, hide = true)]
    reload: bool,
    /// Debugging: before simulating, order these items (comma-separated,
    /// see `combat::ITEM_NAMES`) at the own settlement nearest the focus,
    /// weapons at a workshop put up beside it, and show the settlement.
    #[arg(long, hide = true, value_delimiter = ',', value_name = "ITEMS")]
    train: Vec<u8>,
    /// Debugging: set every ship's hull to this percent before simulating
    /// (to see the damaged pictures).
    #[arg(long, hide = true, value_name = "PCT")]
    hull: Option<f32>,
    /// Debugging: show the radar map (Tab) in the screenshot.
    #[arg(long, hide = true)]
    radar: bool,
    /// Debugging: show the site of a workshop at this cell, as when
    /// placing one from the settlement nearest it.
    #[arg(long, hide = true, value_parser = parse_size, value_name = "X,Y")]
    build_site: Option<(u32, u32)>,
    /// Debugging: set every ship on fire (Greek fire) before simulating.
    #[arg(long, hide = true)]
    burn: bool,
    /// Debugging: show the object info of the object under this map cell's
    /// centre (or the nearest one) in the strip below the view.
    #[arg(long, hide = true, value_parser = parse_size, value_name = "X,Y")]
    hover: Option<(u32, u32)>,
    /// Debugging: select this many of the player's ships nearest the
    /// focus (the object panel's group buttons).
    #[arg(long, hide = true, value_name = "N")]
    select_group: Option<usize>,
    /// Debugging: press these object panel slot cells in turn (a group
    /// opens or closes; anything else is shown big, as held down).
    #[arg(long, hide = true, value_delimiter = ',', value_name = "CELLS")]
    slot_press: Vec<usize>,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum Difficulty {
    Easy,
    Normal,
    Hard,
}

impl Difficulty {
    fn level(self) -> usize {
        match self {
            Difficulty::Easy => scene::EASY,
            Difficulty::Normal => scene::NORMAL,
            Difficulty::Hard => scene::HARD,
        }
    }
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once(['x', 'X', ','])
        .ok_or("expected WIDTHxHEIGHT or X,Y")?;
    let w: u32 = w.parse().map_err(|_| "bad width")?;
    let h: u32 = h.parse().map_err(|_| "bad height")?;
    Ok((w, h))
}

fn main() -> Result<()> {
    let result = run();
    // Started from a file browser there is no terminal to show errors in.
    if let Err(e) = &result {
        if !std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            launch::show_error(e);
        }
    }
    result
}

fn run() -> Result<()> {
    let args = Args::parse();
    if args.window.0 == 0 || args.window.1 == 0 {
        bail!("window size must be non-zero");
    }
    let Some(root) = launch::data_dir(args.data.clone())? else {
        return Ok(());
    };
    let data = GameData::open(&root)?;
    let missions = data.missions()?;
    if missions.is_empty() {
        bail!("no missions found in {}", root.display());
    }
    if args.list_missions {
        for m in &missions {
            println!("{m}");
        }
        return Ok(());
    }
    // A multiplayer game is set up before the window opens.
    let net = if let Some(addr) = &args.join {
        Some(net::Net::join(addr)?)
    } else if args.host {
        let mission = args.mission.clone().unwrap_or_else(|| "battle01".into());
        let archive = data.open_mission(&mission)?;
        let players = openconquest_formats::players(&archive)?;
        let Some(seats) = net::seats_for(&players, args.players) else {
            bail!("{mission} has no room for {} players", args.players);
        };
        Some(net::Net::host(
            args.port,
            net::Setup {
                mission,
                seats,
                difficulty: args.difficulty.level(),
                fog: !args.no_fog,
            },
        )?)
    } else {
        None
    };
    let save = args.load.as_deref().map(save::SaveGame::read).transpose()?;
    let wanted = save.as_ref().map(|s| &s.mission).or(args.mission.as_ref());
    let mission_index = match wanted {
        Some(m) => missions
            .iter()
            .position(|x| x.eq_ignore_ascii_case(m))
            .with_context(|| format!("unknown mission {m}; use --list-missions"))?,
        None => 0,
    };
    let camera = Camera::new(ScalePolicy {
        fixed: args.scale,
        integer: args.integer_scale,
    });

    if let (Some(path), Some(which)) = (&args.screenshot, &args.menu_shot) {
        let options = app::Options {
            fullscreen: false,
            edge_scroll: false,
            window_size: args.window,
            mute: true,
            soundfont: None,
            cd_audio: None,
            intro: false,
            fog: true,
            difficulty: args.difficulty.level(),
        };
        let size = args.window;
        let mut app = app::App::new(data, missions, mission_index, camera, options, None, true)?;
        let mut pixels = vec![0u32; (size.0 * size.1) as usize];
        let mut frame = Frame {
            pixels: &mut pixels,
            width: size.0,
            height: size.1,
        };
        app.draw_menu_screen(which, &mut frame)?;
        return write_png(path, size, &pixels);
    }
    if let Some(path) = &args.screenshot {
        return screenshot(&data, &args, &missions[mission_index], camera, path);
    }

    let options = app::Options {
        fullscreen: args.fullscreen,
        edge_scroll: args.edge_scroll,
        window_size: args.window,
        mute: args.mute,
        soundfont: args.soundfont.clone(),
        cd_audio: args.cd_audio.clone(),
        intro: !args.no_intro,
        fog: !args.no_fog,
        difficulty: args.difficulty.level(),
    };
    let start_menu = args.mission.is_none() && save.is_none() && net.is_none();
    let mut app = app::App::new(
        data,
        missions,
        mission_index,
        camera,
        options,
        save,
        start_menu,
    )?;
    if let Some(net) = net {
        app.start_multiplayer(net)?;
    }
    app.run()
}

/// Orders the armed ship nearest to `focus` to attack the nearest ship of
/// another player.
fn skirmish(scene: &mut Scene, focus: Option<(u32, u32)>) -> Option<u32> {
    let (fx, fy) = focus.map_or((0.0, 0.0), |(x, y)| {
        ((x * 64 + 32) as f32, (y * 64 + 32) as f32)
    });
    let d = |o: &crate::objects::MapObject, x: f32, y: f32| (o.x - x).powi(2) + (o.y - y).powi(2);
    let attacker = scene
        .objects
        .iter()
        .filter(|o| o.ship.is_some() && o.is_armed() && o.owner.is_some())
        .min_by(|a, b| d(a, fx, fy).total_cmp(&d(b, fx, fy)))?;
    let target = scene
        .objects
        .iter()
        .filter(|o| o.ship.is_some() && Scene::are_enemies(attacker, o))
        .min_by(|a, b| d(a, attacker.x, attacker.y).total_cmp(&d(b, attacker.x, attacker.y)));
    if let Some(target) = target {
        let (a, t) = (attacker.id, target.id);
        eprintln!(
            "skirmish: {} at ({:.0},{:.0}) attacks {} at ({:.0},{:.0})",
            attacker.label,
            attacker.x / 64.0,
            attacker.y / 64.0,
            target.label,
            target.x / 64.0,
            target.y / 64.0
        );
        if let Err(e) = scene.order_attack(a, t) {
            eprintln!("skirmish: {e}");
        }
        return Some(a);
    }
    None
}

fn close(scene: &mut Scene, focus: Option<(u32, u32)>, board: bool) -> Option<u32> {
    let (fx, fy) = focus.map_or((0.0, 0.0), |(x, y)| {
        ((x * 64 + 32) as f32, (y * 64 + 32) as f32)
    });
    let d = |o: &crate::objects::MapObject, x: f32, y: f32| (o.x - x).powi(2) + (o.y - y).powi(2);
    let a = scene
        .objects
        .iter()
        .filter(|o| {
            o.owner.is_some()
                && o.ship
                    .as_ref()
                    .is_some_and(|s| !board || s.crew.count() > 0)
        })
        .min_by(|p, q| d(p, fx, fy).total_cmp(&d(q, fx, fy)))?;
    let (aid, ax, ay) = (a.id, a.x, a.y);
    let t = scene
        .objects
        .iter()
        .filter(|o| o.ship.is_some() && Scene::are_enemies(a, o))
        .min_by(|p, q| d(p, ax, ay).total_cmp(&d(q, ax, ay)))?
        .id;
    let (cx, cy) = ((ax / 64.0).floor() as i32, (ay / 64.0).floor() as i32);
    let spot = [(2, 0), (-2, 0), (0, 2), (0, -2), (2, 2), (-2, -2)]
        .into_iter()
        .map(|(dx, dy)| (cx + dx, cy + dy))
        .find(|&(x, y)| {
            scene.nav.is_water(x, y)
                && !scene.objects.iter().any(|o| {
                    o.ship.is_some()
                        && (o.x / 64.0).floor() as i32 == x
                        && (o.y / 64.0).floor() as i32 == y
                })
        })?;
    if let Some(o) = scene.objects.iter_mut().find(|o| o.id == t) {
        o.x = (spot.0 * 64 + 32) as f32;
        o.y = (spot.1 * 64 + 32) as f32;
        if let Some(s) = o.ship.as_mut() {
            s.path.clear();
            s.task = None;
        }
    }
    if let Err(e) = scene.order_close(aid, t, board) {
        eprintln!("close: {e}");
    }
    eprintln!("close: {aid} at ({cx},{cy}) goes for {t} at {spot:?}");
    Some(aid)
}

fn gather(scene: &mut Scene, focus: Option<(u32, u32)>, what: &str) -> Option<u32> {
    use crate::objects::{cargo_capacity, Resource};
    let (fx, fy) = focus.map_or((0.0, 0.0), |(x, y)| {
        ((x * 64 + 32) as f32, (y * 64 + 32) as f32)
    });
    let d = |o: &crate::objects::MapObject, x: f32, y: f32| (o.x - x).powi(2) + (o.y - y).powi(2);
    let resource = if what == "fish" {
        Resource::Fish
    } else {
        Resource::Amber
    };
    let boat = scene
        .objects
        .iter()
        .filter(|o| {
            scene.is_own(o)
                && o.ship
                    .as_ref()
                    .is_some_and(|s| cargo_capacity(s.kind) > 0.0)
        })
        .min_by(|a, b| d(a, fx, fy).total_cmp(&d(b, fx, fy)))?;
    let deposit = scene
        .objects
        .iter()
        .filter(|o| o.deposit.is_some_and(|dep| dep.resource == resource))
        .min_by(|a, b| d(a, boat.x, boat.y).total_cmp(&d(b, boat.x, boat.y)))?;
    let (b, t) = (boat.id, deposit.id);
    if let Err(e) = scene.order_gather(b, t) {
        eprintln!("gather: {e}");
    }
    Some(b)
}

fn screenshot(
    data: &GameData,
    args: &Args,
    mission: &str,
    mut camera: Camera,
    path: &std::path::Path,
) -> Result<()> {
    let size = args.window;
    let mut caches = Caches::default();
    let mut scene = Scene::load_at(data, &mut caches, mission, args.difficulty.level())?;
    scene.fog = !args.no_fog;
    // The counters as they settle once the game has run a moment.
    scene.shown = [scene.amber, scene.fish];
    let reloading = if args.reload {
        reload(&mut scene, args.focus)
    } else {
        None
    };
    if let Some(pct) = args.hull {
        for o in scene.objects.iter_mut().filter(|o| o.ship.is_some()) {
            if let Some(c) = o.combat.as_mut() {
                c.hull = c.max_hull * pct / 100.0;
            }
        }
    }
    if args.burn {
        scene.burn_all(data, &mut caches);
    }
    let training = (!args.train.is_empty())
        .then(|| train(&mut scene, data, &mut caches, args.focus, &args.train))
        .flatten();
    let attacker = if args.skirmish {
        skirmish(&mut scene, args.focus)
    } else if let Some(how) = &args.close {
        close(&mut scene, args.focus, how == "board")
    } else if let Some(what) = &args.gather {
        gather(&mut scene, args.focus, what)
    } else if let Some((x, y)) = args.sail {
        let (fx, fy) = args.focus.map_or((0.0, 0.0), |(x, y)| {
            ((x * 64 + 32) as f32, (y * 64 + 32) as f32)
        });
        let ship = scene
            .objects
            .iter()
            .filter(|o| o.ship.is_some() && scene.is_own(o))
            .min_by(|a, b| {
                (a.x - fx)
                    .hypot(a.y - fy)
                    .total_cmp(&(b.x - fx).hypot(b.y - fy))
            })
            .map(|o| o.id);
        if let Some(id) = ship {
            let ok = scene.order_move(id, (x * 64 + 32) as f32, (y * 64 + 32) as f32);
            eprintln!("sail: ship {id} to ({x},{y}): {ok}");
        }
        ship
    } else {
        None
    };
    let steps = (args.simulate * 50.0) as u32;
    for _ in 0..steps {
        scene.update(0.02, data, &mut caches);
    }
    let time_ms = (args.simulate * 1000.0) as u64;
    let text = TextRenderer::load(&data.controls, "0SYSTEM_FONT", 0)?;
    let interface = Interface::load(data, '0')?;
    let initial = camera;
    let config = panels::Config::default();
    let help = panels::help_paragraphs(
        &scene.briefing_lines,
        scene::is_custom(&scene.mission),
        &scene.conditions,
    );
    let site_shown = args.build_site.and_then(|(x, y)| {
        let (x, y) = (x as i32, y as i32);
        let home = scene
            .objects
            .iter()
            .filter(|o| {
                scene.is_own(o)
                    && matches!(
                        o.class,
                        openconquest_formats::ObjectClass::Building(
                            crate::economy::SETTLEMENT | crate::economy::TOWN
                        )
                    )
            })
            .min_by_key(|o| {
                let (cx, cy) = o.cell.unwrap_or((0, 0));
                (cx - x).pow(2) + (cy - y).pow(2)
            })?
            .id;
        let class = crate::economy::WORKSHOP;
        let facing = scene.check_site(home, class, x, y).ok();
        let preview = facing.and_then(|f| scene.site_preview(class, x, y, f, data, &mut caches));
        Some(((x, y), facing.is_some(), preview))
    });
    let group_sel: Vec<u32> = match (args.select_group, args.focus) {
        (Some(n), Some((cx, cy))) => {
            let (fx, fy) = ((cx * 64 + 32) as f32, (cy * 64 + 32) as f32);
            let mut ships: Vec<&crate::objects::MapObject> = scene
                .objects
                .iter()
                .filter(|o| o.ship.is_some() && scene.is_own(o))
                .collect();
            let d = |o: &crate::objects::MapObject| (o.x - fx).powi(2) + (o.y - fy).powi(2);
            ships.sort_by(|a, b| d(a).total_cmp(&d(b)));
            ships.iter().take(n).map(|o| o.id).collect()
        }
        _ => Vec::new(),
    };
    let since = std::collections::HashMap::new();
    let hotkeys = hotkeys::Hotkeys::default();
    let radar = args.radar.then(|| {
        let layout = interface.layout(size, &initial.policy, &scene);
        crate::ui::Radar {
            origin: crate::ui::radar_origin(&layout, &scene, &initial, (0, 0)),
            view: initial,
        }
    });
    let mut view = GameView {
        hotkeys: &hotkeys,
        selected_since: &since,
        site_preview: site_shown.as_ref().and_then(|s| s.2.as_ref()),
        site_cells: 1,
        radar,
        deselected: &since,
        data,
        scene: &scene,
        camera: &initial,
        interface: (!args.no_interface).then_some(&interface),
        text: &text,
        selected: None,
        group: group_sel.get(1..).unwrap_or(&[]),
        select_box: None,
        status: None,
        time_ms,
        hover_button: args.hover_button,
        info: Default::default(),
        sent: None,
        page: match args.page.as_deref() {
            Some("behaviour") => commands::Page::Behaviour,
            Some("research") => commands::Page::Research,
            Some("structures") => commands::Page::Structures,
            Some("wind") => commands::Page::Wind,
            _ => Default::default(),
        },
        site: site_shown.as_ref().map(|s| (s.0, s.1)),
        briefing: false,
        panel: Some(panels::PanelView {
            tab: args.panel,
            saves: &[],
            save_first: 0,
            selected_save: None,
            file_mode: panels::FileMode::Save,
            name: "",
            config: &config,
            modern_mouse: false,
            tracks: &[],
            help: &help,
            help_first: 0,
            help_page: args.help_page,
            pressed: None,
            time_ms,
        }),
        panel_hover: None,
        paused: args.paused,
        load: Some((load_panel::LoadView::default(), None)),
        object_info: None,
        hover_object: None,
    };
    let (screen, _, _) = view.placement(size);
    let map = scene.map_size();
    let followed = attacker.and_then(|a| scene.objects.iter().find(|o| o.id == a));
    let (fx, fy) = match (followed, args.focus) {
        _ if args.at_start => scene.start_view(),
        (Some(o), _) => (o.x, o.y),
        (None, focus) => match focus {
            Some((cx, cy)) => ((cx * 64 + 32) as f32, (cy * 64 + 32) as f32),
            None => (map.0 as f32 / 2.0, map.1 as f32 / 2.0),
        },
    };
    camera.center_on(fx, fy, &screen, map);
    if args.at_start {
        if let Some((x, y)) = scene.start_corner() {
            camera.x = x;
            camera.y = y;
            camera.clamp(&screen, map);
        }
    }
    let t = openconquest_formats::map::TILE_SIZE as f32;
    let (vw, vh) = camera.view_size(&screen);
    scene.follow_view(
        ((camera.x / t).floor() as i32, (camera.y / t).floor() as i32),
        ((vw / t).ceil() as i32, (vh / t).ceil() as i32),
    );
    view.camera = &camera;
    // Select the object nearest to the focus point, to show the object panel.
    view.selected = args.focus.and_then(|(cx, cy)| {
        let (fx, fy) = ((cx * 64 + 32) as f32, (cy * 64 + 32) as f32);
        scene
            .objects
            .iter()
            .filter(|o| o.picture.is_some())
            .min_by(|a, b| {
                let d = |o: &&crate::objects::MapObject| (o.x - fx).powi(2) + (o.y - fy).powi(2);
                d(a).total_cmp(&d(b))
            })
            .map(|o| o.id)
    });
    if training.is_some() {
        view.selected = training;
    }
    if let Some(&first) = group_sel.first() {
        view.selected = Some(first);
    }
    if let Some(sel) = view.selected {
        use info_panel::{Hit, InfoView, Shown};
        let selection: Vec<u32> = std::iter::once(sel)
            .chain(view.group.iter().copied())
            .collect();
        let mut info = InfoView {
            owner: (sel, view.group.len()),
            ..Default::default()
        };
        for &i in &args.slot_press {
            if let Shown::Single(_, list) = info_panel::shown(&scene, &selection, &info) {
                match list.get(i) {
                    Some(e) if e.is_group() => {
                        info.open =
                            (e.flags & crate::scene::load::HEADER == 0).then_some((e.cat, e.sub));
                        info.scroll = 0;
                    }
                    Some(e) if info_panel::describable(e) => {
                        info.big = Some(i);
                        info.pressed = Some(Hit::Cell(i));
                    }
                    e => eprintln!("slot {i}: nothing to press ({e:?})"),
                }
            }
        }
        view.info = info;
    }
    if let Some(target) = reloading {
        view.selected = Some(target);
        view.load = Some((
            load_panel::LoadView {
                owner: target,
                ..Default::default()
            },
            None,
        ));
        if scene.load_link(target).is_none() {
            eprintln!("reload: the ship has not got alongside yet");
        }
    }
    let hovered = args.hover.and_then(|(cx, cy)| {
        let (fx, fy) = ((cx * 64 + 32) as f32, (cy * 64 + 32) as f32);
        let near = |o: &&crate::objects::MapObject| (o.x - fx).powi(2) + (o.y - fy).powi(2);
        render::hit_test(&scene, fx, fy, time_ms)
            .and_then(|id| scene.objects.iter().find(|o| o.id == id))
            .or_else(|| {
                scene
                    .objects
                    .iter()
                    .min_by(|a, b| near(a).total_cmp(&near(b)))
            })
    });
    if let Some(o) = hovered {
        view.object_info = object_info::of(&scene, o);
        view.hover_object = Some(o.id);
        eprintln!("hover: {} {:?}: {:?}", o.id, o.class, view.object_info);
    }
    let mut pixels = vec![0u32; (size.0 * size.1) as usize];
    let mut frame = Frame {
        pixels: &mut pixels,
        width: size.0,
        height: size.1,
    };
    view.draw(&mut frame);
    write_png(path, size, &pixels)
}

/// Orders `items` at the player's settlement nearest `focus` (see
/// `--train`), putting up a workshop beside it if there is none; returns
/// the settlement.
fn train(
    scene: &mut Scene,
    data: &GameData,
    caches: &mut Caches,
    focus: Option<(u32, u32)>,
    items: &[u8],
) -> Option<u32> {
    use crate::economy::{SETTLEMENT, TOWN, WORKSHOP};
    use openconquest_formats::ObjectClass;
    let t = openconquest_formats::map::TILE_SIZE as f32;
    let (fx, fy) = focus.map_or((0.0, 0.0), |(x, y)| (x as f32 * t, y as f32 * t));
    let near = |o: &crate::objects::MapObject| (o.x - fx).powi(2) + (o.y - fy).powi(2);
    let home = scene
        .objects
        .iter()
        .filter(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .min_by(|a, b| near(a).total_cmp(&near(b)))?;
    let (home, cell) = (home.id, home.cell?);
    scene.amber += 10_000;
    scene.fish += 10_000;
    let owned = |scene: &Scene| {
        scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.class == ObjectClass::Building(WORKSHOP))
            .map(|o| o.id)
    };
    if owned(scene).is_none() {
        let r = crate::scene::BUILD_RADIUS;
        let site = (-r..=r)
            .flat_map(|dy| (-r..=r).map(move |dx| (cell.0 + dx, cell.1 + dy)))
            .find(|&(x, y)| scene.check_site(home, WORKSHOP, x, y).is_ok())?;
        scene
            .place_building(home, WORKSHOP, site.0, site.1, data, caches)
            .ok()?;
    }
    let workshop = owned(scene)?;
    for &item in items {
        let site = if item < 3 { home } else { workshop };
        if let Err(e) = scene.train(site, item) {
            eprintln!("train: {e}");
        }
    }
    Some(home)
}

/// Sends the player's ship nearest `focus` to reload at the own port,
/// shipyard or dock nearest it (see `--reload`); returns the building.
fn reload(scene: &mut Scene, focus: Option<(u32, u32)>) -> Option<u32> {
    use openconquest_formats::ObjectClass;
    let (fx, fy) = focus.map_or((0.0, 0.0), |(x, y)| {
        ((x * 64 + 32) as f32, (y * 64 + 32) as f32)
    });
    let near = |o: &&crate::objects::MapObject| (o.x - fx).hypot(o.y - fy);
    let ship = scene
        .objects
        .iter()
        .filter(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind < 10))
        .min_by(|a, b| near(a).total_cmp(&near(b)))?;
    let (sx, sy, ship) = (ship.x, ship.y, ship.id);
    let target = scene
        .objects
        .iter()
        .filter(|o| {
            scene.is_own(o)
                && matches!(
                    o.class,
                    ObjectClass::Building(economy::PORT | economy::SHIPYARD | economy::DOCK)
                )
        })
        .min_by(|a, b| {
            (a.x - sx)
                .hypot(a.y - sy)
                .total_cmp(&(b.x - sx).hypot(b.y - sy))
        })?
        .id;
    match scene.order_load(ship, target) {
        Ok(()) => eprintln!("reload: ship {ship} at ({sx}, {sy}) to {target}"),
        Err(e) => eprintln!("reload: {e}"),
    }
    Some(target)
}

fn write_png(path: &std::path::Path, size: (u32, u32), pixels: &[u32]) -> Result<()> {
    let rgb: Vec<u8> = pixels
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
        .collect();
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgb)?;
    println!("wrote {}", path.display());
    Ok(())
}
