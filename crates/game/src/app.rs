//! Window, input handling and the frame loop.

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::assets::GameData;
use crate::commands::{menu, Command, Page};
use crate::economy;
use crate::magic::{self, Spell, Target, Temple};
use crate::objects::Caches;
use crate::objects::{Resource, SHIP_NAMES};
use crate::render::{hit_test, Frame, MinimapRect};
use crate::scene::Scene;
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
/// How long status messages stay on screen.
const STATUS_TIME: Duration = Duration::from_secs(3);

pub struct Options {
    pub fullscreen: bool,
    pub edge_scroll: bool,
    pub window_size: (u32, u32),
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
    /// A command waiting for a target on the map (e.g. Move).
    pending: Option<Command>,
    /// Which menu of the selected object is shown.
    page: Page,
    /// The mission briefing is shown (and the game waits).
    briefing: bool,
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
    ) -> Result<Self> {
        let mut caches = Caches::default();
        let scene = Scene::load(&data, &mut caches, &missions[mission_index])?;
        let text = TextRenderer::load(&data.controls, "0SYSTEM_FONT", 0)?;
        let interface = Interface::load(&data, '0')?;
        let now = Instant::now();
        Ok(Self {
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
            pending: None,
            page: Page::Main,
            briefing: true,
            status: None,
            started: now,
            last_frame: now,
            time_ms: 0,
            center_pending: true,
        })
    }

    pub fn run(mut self) -> Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.run_app(&mut self)?;
        match self.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn window_size(&self) -> (u32, u32) {
        self.gfx
            .as_ref()
            .map(|g| {
                let s = g.window.inner_size();
                (s.width.max(1), s.height.max(1))
            })
            .unwrap_or((1, 1))
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
            status,
            time_ms: self.time_ms,
            hover_button: self.cursor.and_then(|c| self.button_at(c)),
            page: self.page,
            site: None,
            briefing: self.briefing,
        }
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
        if !self.show_interface {
            return None;
        }
        let layout = self
            .interface
            .layout(self.window_size(), &self.camera.policy, &self.scene);
        self.interface
            .button_at(&layout, pos.x as f32, pos.y as f32)
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
                None if (Command::TRAIN_BOWMEN.0..=Command::TRAIN_SWORDSMEN.0)
                    .contains(&entry.command.0) =>
                {
                    "Not enough amber or fish".to_string()
                }
                None if matches!(entry.command, Command::UPGRADE_TOWN | Command::UPGRADE_DOCK)
                    || economy::building_of_command(entry.command).is_some() =>
                {
                    "Not enough amber".to_string()
                }
                None if entry.command == magic::GET_MAGIC => "Not enough amber".to_string(),
                None if Spell::of_command(entry.command).is_some_and(|s| s.implemented())
                    || (magic::WIND_EAST.0..magic::WIND_EAST.0 + 8).contains(&entry.command.0) =>
                {
                    "Not enough amber or fish".to_string()
                }
                None => format!("{}: not available yet", entry.command.name()),
            };
            self.say(reason);
            return;
        }
        if let Some(kind) = ship_kind {
            if entry.progress.is_some() {
                return;
            }
            match self.scene.start_ship(id, kind) {
                Ok(()) => self.say(format!("Building a {}", SHIP_NAMES[kind as usize])),
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
                self.scene.stop(id);
            }
            c if (Command::TRAIN_BOWMEN.0..=Command::TRAIN_SWORDSMEN.0).contains(&c.0) => {
                let kind = entry.command.0 - Command::TRAIN_BOWMEN.0;
                match self.scene.train(id, kind) {
                    Ok(()) => self.say(entry.command.name()),
                    Err(e) => self.say(e),
                }
            }
            Command::LOAD => match self.scene.load_crew(id) {
                Ok(n) => self.say(format!("{n} marines came aboard")),
                Err(e) => self.say(e),
            },
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
                    match self.scene.learn_spell(temple) {
                        Ok(()) => self.say("The priests study a new spell"),
                        Err(e) => self.say(e),
                    }
                }
            }
            c if Spell::of_command(c) == Some(Spell::Wind) => self.page = Page::Wind,
            Command(0x57) => self.page = Page::Main,
            c if (magic::WIND_EAST.0..magic::WIND_EAST.0 + 8).contains(&c.0) => {
                match self.scene.cast_wind((c.0 - magic::WIND_EAST.0) as u32) {
                    Ok(()) => {
                        self.page = Page::Main;
                        self.say("The wind rises");
                    }
                    Err(e) => self.say(e),
                }
            }
            c if Spell::of_command(c).is_some_and(|s| {
                matches!(s.target(), Target::Water | Target::Enemy | Target::OwnShip)
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
            Command::UPGRADE_TOWN | Command::UPGRADE_DOCK => {
                match self.scene.upgrade(id, &self.data, &mut self.caches) {
                    Ok(()) => self.say("Upgrade complete"),
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
        let n = self.missions.len() as isize;
        let next = (self.mission_index as isize + delta).rem_euclid(n) as usize;
        match Scene::load(&self.data, &mut self.caches, &self.missions[next]) {
            Ok(scene) => {
                self.scene = scene;
                self.selected = None;
                self.pending = None;
                self.mission_index = next;
                self.center_pending = true;
                self.briefing = true;
                self.update_title();
            }
            Err(e) => eprintln!("failed to load {}: {e:#}", self.missions[next]),
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
            match self.scene.place_building(
                settlement,
                class,
                cell.0,
                cell.1,
                &self.data,
                &mut self.caches,
            ) {
                Ok(()) => {
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
            .filter(|s| matches!(s.target(), Target::Enemy | Target::OwnShip))
        {
            let order = match self.object_at(pos) {
                Some(target) => self
                    .scene
                    .cast_on(spell, target, &self.data, &mut self.caches),
                None => Err(spell.prompt().to_string()),
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
        if let Some(spell) = self
            .pending
            .and_then(Spell::of_command)
            .filter(|s| s.target() == Target::Water)
        {
            let Some(cell) = self.cell_at(pos) else {
                return;
            };
            match self
                .scene
                .cast_hazard(spell, cell.0, cell.1, &self.data, &mut self.caches)
            {
                Ok(()) => {
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
            match self.scene.order_found(ship, cell.0, cell.1) {
                Ok(()) => {
                    self.pending = None;
                    self.status = None;
                }
                Err(e) => self.say(e),
            }
            return;
        }
        if let Some(cmd @ (Command::RAM | Command::BOARD)) = self.pending {
            let order = match (self.object_at(pos), self.selected) {
                (Some(target), Some(ship)) => {
                    self.scene.order_close(ship, target, cmd == Command::BOARD)
                }
                _ => Err("Choose an enemy ship".into()),
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
            let order = match (self.object_at(pos), self.selected) {
                (Some(target), Some(ship)) => self.scene.order_attack(ship, target),
                _ => Err("Choose an enemy ship or building".into()),
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
        if let Some(cmd @ (Command::AMBER | Command::FISH)) = self.pending {
            let wanted = if cmd == Command::AMBER {
                Resource::Amber
            } else {
                Resource::Fish
            };
            let target = self.object_at(pos).filter(|&id| {
                self.scene
                    .object_index(id)
                    .and_then(|i| self.scene.objects[i].deposit)
                    .is_some_and(|d| d.resource == wanted)
            });
            match (target, self.selected) {
                (Some(deposit), Some(ship)) => {
                    self.pending = None;
                    self.status = None;
                    if let Err(e) = self.scene.order_gather(ship, deposit) {
                        self.say(e);
                    }
                }
                _ => self.say(if cmd == Command::AMBER {
                    "Choose an amber deposit"
                } else {
                    "Choose a fishing ground"
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
        let hit = hit_test(&self.scene, wx, wy, self.time_ms);
        if hit != self.selected {
            self.page = Page::Main;
        }
        self.selected = hit;
        self.pending = None;
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
        let Some(id) = self.selected else { return };
        // Right-clicking a deposit with a merchant selected starts gathering.
        if let Some(target) = self.object_at(pos) {
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
                if let Err(e) = self.scene.order_gather(id, target) {
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
                if let Err(e) = self.scene.order_attack(id, target) {
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
                if let Err(e) = self.scene.order_close(id, target, false) {
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
        if is_ship && !self.scene.order_move(id, target.0, target.1) {
            self.say("The ship cannot sail there.");
        }
    }

    fn update(&mut self, dt: f32) {
        // The game waits while the briefing is read.
        if !self.briefing {
            self.scene.update(dt, &self.data, &mut self.caches);
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
            self.camera
                .center_on(map.0 as f32 / 2.0, map.1 as f32 / 2.0, &screen, map);
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
        let speed = SCROLL_SPEED * self.camera.policy.base_scale(window.1) * dt;
        self.camera
            .scroll(dx.signum() * speed, dy.signum() * speed, &screen, map);
        self.camera.clamp(&screen, map);
    }

    fn redraw(&mut self) -> Result<()> {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        self.time_ms = (now - self.started).as_millis() as u64;
        self.update(dt);

        let window = self.window_size();
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
            let site = self.cursor.and_then(|c| self.site_at(c));
            GameView {
                site,
                ..self.view()
            }
            .draw(&mut frame);
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
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                if event.state == ElementState::Released {
                    self.keys.remove(&code);
                    return;
                }
                self.keys.insert(code);
                if event.repeat {
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
                    KeyCode::Escape if self.selected.is_some() => self.selected = None,
                    KeyCode::Escape => event_loop.exit(),
                    KeyCode::F11 => self.toggle_fullscreen(),
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
                (MouseButton::Left, ElementState::Pressed) if self.briefing => {
                    self.briefing = false;
                }
                (MouseButton::Left, ElementState::Pressed) => {
                    if let Some(c) = self.cursor {
                        self.left_click(c);
                    }
                }
                (MouseButton::Left, ElementState::Released) => self.minimap_drag = false,
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                };
                let (screen, _) = self.placement();
                let c = self.cursor.unwrap_or(PhysicalPosition::new(
                    (screen.x + screen.w / 2) as f64,
                    (screen.y + screen.h / 2) as f64,
                ));
                if screen.contains(c.x as f32, c.y as f32) {
                    self.camera.zoom_at(
                        ZOOM_STEP.powf(steps),
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
        // Redraw at ~60 Hz; palette cycling needs a steady clock even when idle.
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}
