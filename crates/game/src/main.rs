//! OpenAC: an open source reimplementation of *Ancient Conquest*.

mod app;
mod assets;
mod combat;
mod commands;
mod economy;
mod magic;
mod nav;
mod objects;
mod render;
mod scene;
mod terrain;
mod text;
mod ui;
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
    /// Game data directory created by `openac-extract install`.
    #[arg(long, env = "OPENAC_DATA", default_value = "gamedata")]
    data: PathBuf,
    /// Mission to open (e.g. miss000, single03, battle12).
    #[arg(long)]
    mission: Option<String>,
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
    /// Run the simulation for this many seconds before taking the screenshot.
    #[arg(long, default_value_t = 0.0, value_name = "SECONDS")]
    simulate: f32,
    /// Debugging: before simulating, order the focused ship to attack the
    /// nearest ship of another player.
    #[arg(long)]
    skirmish: bool,
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
    let args = Args::parse();
    if args.window.0 == 0 || args.window.1 == 0 {
        bail!("window size must be non-zero");
    }
    let data = GameData::open(&args.data)?;
    let missions = data.missions()?;
    if missions.is_empty() {
        bail!("no missions found in {}", args.data.display());
    }
    if args.list_missions {
        for m in &missions {
            println!("{m}");
        }
        return Ok(());
    }
    let mission_index = match &args.mission {
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

    if let Some(path) = &args.screenshot {
        return screenshot(&data, &args, &missions[mission_index], camera, path);
    }

    let options = app::Options {
        fullscreen: args.fullscreen,
        edge_scroll: args.edge_scroll,
        window_size: args.window,
    };
    app::App::new(data, missions, mission_index, camera, options)?.run()
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

fn screenshot(
    data: &GameData,
    args: &Args,
    mission: &str,
    mut camera: Camera,
    path: &PathBuf,
) -> Result<()> {
    let size = args.window;
    let mut caches = Caches::default();
    let mut scene = Scene::load(data, &mut caches, mission)?;
    let attacker = if args.skirmish {
        skirmish(&mut scene, args.focus)
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
    let mut view = GameView {
        data,
        scene: &scene,
        camera: &initial,
        interface: (!args.no_interface).then_some(&interface),
        text: &text,
        selected: None,
        status: None,
        time_ms,
        hover_button: None,
        page: Default::default(),
        site: None,
    };
    let (screen, _, _) = view.placement(size);
    let map = scene.map_size();
    let followed = attacker.and_then(|a| scene.objects.iter().find(|o| o.id == a));
    let (fx, fy) = match (followed, args.focus) {
        (Some(o), _) => (o.x, o.y),
        (None, focus) => match focus {
            Some((cx, cy)) => ((cx * 64 + 32) as f32, (cy * 64 + 32) as f32),
            None => (map.0 as f32 / 2.0, map.1 as f32 / 2.0),
        },
    };
    camera.center_on(fx, fy, &screen, map);
    view.camera = &camera;
    // Select the object nearest to the focus point, to show the object panel.
    view.selected = args.focus.and_then(|_| {
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
    let mut pixels = vec![0u32; (size.0 * size.1) as usize];
    let mut frame = Frame {
        pixels: &mut pixels,
        width: size.0,
        height: size.1,
    };
    view.draw(&mut frame);
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
