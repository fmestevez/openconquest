//! The original in-game interface: the framed game view, the control bar
//! with the minimap and the object panel.
//!
//! The original screen is 800x600: `GAME_FIELD` (592x600, the frame around
//! the view and the message strip) plus `GAME_CTRLBAR` (208x600), on the
//! right, or on the left with "control bar left" ([0x5ca54a]): the bar's x
//! is 592 or 0 (0x512ab0, 0x511170 id 0x66), the field's 0 or 208 (0x504ef0,
//! 0x511170 id 0x67). Panels on
//! the bar, from ANCIENT.EXE: the minimap frame `BKGND_LMAPA` at bar + (4, 4)
//! (0x4fffe0) and the object panel `COBJ_BKMAIN` at bar + (3, 206)
//! (0x4f5800). Names are prefixed with a theme digit, `0` (Greek) or `3`
//! (Persian).
//!
//! With the bar hidden (Space, `[game+0x328d]`) the bar is moved off the
//! screen and the field takes the whole width: `GAME_FIELD_FS` (800x600) at
//! 0 whatever the bar's side, the view 12 x 8 cells at (16, 8) (0x50f910
//! message 0x4e1, Mapa.cpp 0x51cdd0 0xcc3).
//!
//! Widescreen: everything is scaled by the window height, the control bar
//! stays at its side and the frame is widened by tiling its middle part.

use anyhow::{Context, Result};
use openconquest_formats::{Bitmap, ResourceKind, Sequence};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::assets::{rgb, GameData, Image};
use crate::commands::{menu, Menu, Page, SLOTS};
use crate::load_panel;
use crate::panels::{PanelView, Panels};
use crate::render::{
    blit_scaled, draw_fog, draw_minimap, draw_objects, draw_selection, draw_site, draw_world,
    palette_lut, shade_rect, Frame, MinimapRect,
};
use crate::scene::Scene;
use crate::text::TextRenderer;
use crate::view::{ScalePolicy, Screen};

const ORIGINAL_FIELD_W: u32 = 592;
/// `GAME_FIELD_FS`, the field with the bar hidden: the whole screen.
const ORIGINAL_FIELD_FS_W: u32 = 800;
/// The view's place with the bar hidden: 0x511170(0x67, 16), y 8, 12
/// cells across (Mapa.cpp 0x51cdd0 0xcc3: `+0x3c` 12 hidden, 9 shown,
/// 0x51e041..0x51e051; the height, screen `+0x17c2`, 8 always).
const VIEW_HIDDEN: (u32, u32) = (16, 8);
const VIEW_CELLS_HIDDEN: u32 = 12;
/// The chat lines' x (chat.cpp 0x4ff390, 0x4ff4c0: 0x504ef0(8), 16 with
/// the bar hidden), their first y and their step, and their bitmaps'
/// widths in cells (9, 12 hidden) of 64 pixels.
const CHAT_X: [u32; 2] = [8, 16];
const CHAT_Y: (u32, u32) = (8, 20);
const CHAT_CELLS: [u32; 2] = [9, 12];
pub const BAR_W: u32 = 208;
const UI_H: u32 = 600;
const MINIMAP_POS: (u32, u32) = (4, 4);
const OBJECT_PANEL_POS: (u32, u32) = (3, 206);
/// The strip above the command buttons (0x4f06f0: y 0x16e).
const STRIP_Y: u32 = 366;
/// Command buttons (0x4f31b0): 4 columns x 3 rows of 49x49 buttons.
const BUTTON_ORIGIN: (u32, u32) = (2, 403);
const BUTTON_STEP: (u32, u32) = (51, 52);
const BUTTON_SIZE: u32 = 49;
const BUTTON_COLUMNS: usize = 4;
/// Milliseconds per frame of an animated button.
const BUTTON_FRAME_MS: u64 = 60;
/// Money panel `GAME_BKMONEY` (0x504620): at the right end of the strip
/// below the view, 106 pixels from the frame's right edge.
const MONEY_PANEL: (u32, u32) = (106, 527);
/// Coin and fish icons in the money panel (0x505330).
const MONEY_ICON: (u32, u32) = (10, 5);
const FISH_ICON: (u32, u32) = (10, 35);
/// The amber and fish numbers' boxes in the money panel (0x505530).
const MONEY_TEXT: [(u32, u32); 2] = [(45, 10), (45, 41)];
const MONEY_BOX: (u32, u32) = (50, 20);
/// Frame delay of the money and fish icons (0x505330: 40).
const ICON_FRAME_MS: u64 = 40;
/// Wind rose `GAME_ROSEWIND` with the `GAME_WIND_nn` arrow over it
/// (WindTy, 0x504fa0): at the left end of the strip below the view.
const WIND_ROSE: (u32, u32) = (3, 527);
/// Message line `GAME_MEDIT` in the strip below the view.
const MESSAGE_LINE: (u32, u32) = (107, 568);
/// Columns of the field frame kept at each side when widening it.
const FRAME_EDGE: u32 = 100;

/// A rectangle `(x, y, w, h)` in original interface pixels.
type Rect = (u32, u32, u32, u32);

pub struct Interface {
    field: Image,
    /// `GAME_FIELD_FS`, drawn instead with the bar hidden.
    field_fs: Image,
    bar: Image,
    minimap_frame: Image,
    object_panel: Image,
    /// The strip above the command buttons (0x4f06f0): `CCTRL_CLRDEF1`, a
    /// relief, on an object's main page; `CCTRL_CLRDEF2`, plain, on the
    /// others.
    strips: [Option<Image>; 2],
    /// A slot cell's background (`COBJ_BKSMSLOT`, 47x50).
    small_slot: Option<Image>,
    /// Counts on slot cells (`NUMBER_FONT`, letters 2 apart).
    number_font: TextRenderer,
    /// The shading of work not yet done (table 0x5c1658, palette index to
    /// index), by colour.
    shade: HashMap<u32, u32>,
    /// The health bar colours (palette 200 and 199, 203 and 157, 65 and
    /// 68: green, yellow, red).
    bar_colours: [[u32; 2]; 3],
    /// The see-through part of `field` where the world is shown.
    field_view: Rect,
    minimap_area: Rect,
    picture_slot: Rect,
    pictures: RefCell<HashMap<String, Option<Rc<Image>>>>,
    money_panel: Image,
    message_line: Image,
    /// `GAME_MEDIT_FS` (584x26), the message line with the bar hidden.
    message_line_fs: Image,
    /// Number slots of the money panel (amber, fish).
    message_area: Rect,
    money_icon: Vec<Rc<Image>>,
    wind_rose: Image,
    wind_arrows: Vec<Option<Image>>,
    fish_icon: Vec<Rc<Image>>,
    /// The amber and fish counters' fonts.
    digits: [TextRenderer; 2],
    /// Button animations by base name.
    buttons: RefCell<HashMap<String, Rc<Vec<Rc<Image>>>>>,
    button_frame: Option<Rc<Image>>,
    /// Button captions (`BUT_FONT`): atlas 0 on enabled buttons, 1 (grey)
    /// on disabled ones.
    button_font: [TextRenderer; 2],
    /// Button marks (`CBUT_SPEC_00`-`05`): red and grey crosses, green and
    /// grey arrows, green and grey ticks.
    button_marks: Vec<Option<Image>>,
    /// The "default arming" checkbox: its box (`TRIGGER_BKGND`), tick
    /// (`MARKER`) and caption (string 501).
    arming_box: Option<Image>,
    arming_tick: Option<Image>,
    pub panels: Panels,
    /// The animated "pause" sign (`WIN_PAUSE`).
    pause_sign: Vec<Rc<Image>>,
    /// The messages' font (`MESS_FONT`, chat.cpp).
    mess_font: Option<TextRenderer>,
    /// The load panel's pictures.
    pub load_art: load_panel::LoadArt,
    /// The green tick where a ship was sent (`TAKE_CMD`, controlm).
    take_cmd: Vec<Image>,
    /// A site refused (`VETOBUILD`, controlm; gcursor.cpp 0x516010):
    /// frame 0 a cell, 1 a settlement's two by two.
    veto: Vec<Image>,
    /// The object info in the strip below the view.
    info_art: crate::object_info::InfoArt,
    /// The group panel's and the big slot's pictures (`info_panel`).
    panel_art: crate::info_panel::InfoArt,
}

/// The object panel's background: `COBJ_BKMAIN` and the strip for one
/// object, `CGRP_BKG` for a group of `n`, `COBJ_CLEAR` for none
/// (`info_panel`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelBack {
    Object,
    Group(usize),
    Clear,
}

/// Window-space placement of the interface for one window size.
pub struct Layout {
    pub scale: f32,
    pub view: Screen,
    pub minimap: MinimapRect,
    field_w: u32,
    /// The field's left edge, in original pixels: 0, or the bar's width
    /// with the bar on the left (0x504ef0, 0x511170 id 0x67: + 208).
    field_x: u32,
    pub bar_x: f32,
    /// The bar hidden (Space, `[game+0x328d]`): the bar off the screen.
    pub bar_hidden: bool,
}

/// The radar map (Tab): its first cell, and the view the bar's map
/// window shows meanwhile (the view's own place is kept, its frame drawn
/// on the radar map).
#[derive(Debug, Clone, Copy)]
pub struct Radar {
    pub origin: (i32, i32),
    pub view: crate::view::Camera,
}

/// With the radar map (Tab): the view, in the bar's map window, 24 x 24
/// minimap pixels (3 x 3 cells at the view's scale; Mapa.cpp 0x51bd20, mode
/// 4), and the radar map in the view's place (mode 2), from cell `origin`.
pub fn radar_placement(
    layout: &Layout,
    scene: &Scene,
    origin: (i32, i32),
) -> (Screen, MinimapRect) {
    let m = layout.minimap;
    let cell = (openconquest_formats::map::TILE_SIZE / crate::terrain::MINIMAP_DIVISOR) as f32;
    let side = (crate::scene::MINIMAP_CELLS as f32 * cell * m.zoom).round() as u32;
    let bar = Screen {
        x: m.x,
        y: m.y,
        w: side,
        h: side,
        window_h: layout.view.window_h,
    };
    (bar, MinimapRect::radar(scene, &layout.view, m.zoom, origin))
}

/// The radar map's first cell once the view is where `camera` has it,
/// scrolled from `origin` (Mapa.cpp 0x51c7e0): the least scroll keeping
/// the view's frame, the size of the view in its usual place, inside.
pub fn radar_origin(
    layout: &Layout,
    scene: &Scene,
    camera: &crate::view::Camera,
    origin: (i32, i32),
) -> (i32, i32) {
    let t = openconquest_formats::map::TILE_SIZE as f32;
    let (cols, rows) = crate::render::radar_cells(&layout.view, layout.minimap.zoom);
    let (vw, vh) = camera.view_size(&layout.view);
    let map = scene.map_size();
    let axis = |o: i32, pos: f32, view: f32, window: i32, map: u32| {
        crate::render::radar_scroll(
            o,
            (pos / t).floor() as i32,
            (view / t).ceil() as i32,
            window,
            (map as f32 / t) as i32,
        )
    };
    (
        axis(origin.0, camera.x, vw, cols, map.0),
        axis(origin.1, camera.y, vh, rows, map.1),
    )
}

impl Interface {
    pub fn load(data: &GameData, theme: char) -> Result<Self> {
        let load = |name: &str, transparent_black: bool| -> Result<Image> {
            let full = format!("{theme}{name}");
            let b = Bitmap::parse(&data.controls.read(&full, ResourceKind::Bitmap)?)
                .with_context(|| format!("decoding {full}"))?;
            let pixels = b
                .pixels
                .iter()
                .map(|&i| (!(transparent_black && i == 0)).then(|| rgb(b.palette.0[i as usize])))
                .collect();
            Ok(Image::new(b.width, b.height, pixels))
        };
        let field = load("GAME_FIELD", true)?;
        let field_fs = load("GAME_FIELD_FS", true)?;
        let minimap_frame = load("BKGND_LMAPA", false)?;
        let object_panel = load("COBJ_BKMAIN", false)?;
        let field_view = black_rect(&field, field.width / 2, field.height / 2);
        let minimap_area = black_rect(
            &minimap_frame,
            minimap_frame.width / 2,
            minimap_frame.height / 2,
        );
        let picture_slot = black_rect(&object_panel, 20, 20);
        let money_panel = load("GAME_BKMONEY", false)?;
        let message_line = load("GAME_MEDIT", false)?;
        let message_area = black_rect(
            &message_line,
            message_line.width / 2,
            message_line.height / 2,
        );
        let sequence = |name: &str| -> Vec<Rc<Image>> {
            let Some(e) = data.controls.find(name, ResourceKind::Sequence) else {
                return Vec::new();
            };
            let Ok(seq) = data
                .controls
                .read_entry(e)
                .and_then(|d| Sequence::parse(&d, e))
            else {
                return Vec::new();
            };
            seq.frame_names(&data.controls)
                .iter()
                .filter_map(|n| data.load_image(&data.controls, n, false).ok().flatten())
                .map(Rc::new)
                .collect()
        };
        Ok(Self {
            field,
            field_fs,
            message_line_fs: load("GAME_MEDIT_FS", false)?,
            bar: load("GAME_CTRLBAR", false)?,
            minimap_frame,
            object_panel,
            strips: [1, 2].map(|i| load(&format!("CCTRL_CLRDEF{i}"), false).ok()),
            small_slot: load("COBJ_BKSMSLOT", false).ok(),
            number_font: TextRenderer::load_in(
                &data.controls,
                "NUMBER_FONT",
                0,
                Some(&data.palette),
            )?
            .with_spacing(2),
            shade: {
                let mut map = HashMap::new();
                for (i, &to) in SHADE.iter().enumerate() {
                    map.entry(rgb(data.palette.0[i]))
                        .or_insert(rgb(data.palette.0[to as usize]));
                }
                map
            },
            bar_colours: [[200, 199], [203, 157], [65, 68]]
                .map(|pair| pair.map(|i| rgb(data.palette.0[i]))),
            field_view,
            minimap_area,
            picture_slot,
            pictures: RefCell::default(),
            money_panel,
            message_line,
            message_area,
            money_icon: sequence("GAME_MONEY"),
            wind_rose: load("GAME_ROSEWIND", false)?,
            wind_arrows: (0..24)
                .map(|i| {
                    data.load_image(&data.controls, &format!("GAME_WIND_{i:02}"), false)
                        .ok()
                        .flatten()
                })
                .collect(),
            fish_icon: sequence("GAME_FISH"),
            digits: [
                TextRenderer::load_in(&data.controls, "MONEY_FONT", 0, Some(&data.palette))?
                    .with_spacing(0),
                TextRenderer::load_in(&data.controls, "FISH_FONT", 0, Some(&data.palette))?
                    .with_spacing(0),
            ],
            buttons: RefCell::default(),
            panels: Panels::load(data, theme)?,
            pause_sign: sequence(&format!("{theme}WIN_PAUSE")),
            mess_font: TextRenderer::load(&data.controls, "MESS_FONT", 0).ok(),
            load_art: load_panel::LoadArt::load(data, theme),
            take_cmd: (0..4)
                .filter_map(|i| {
                    let menus = data.menus.as_ref()?;
                    data.load_image(menus, &format!("TAKE_CMD{i:02}"), false)
                        .ok()
                        .flatten()
                })
                .collect(),
            veto: (0..2)
                .filter_map(|i| {
                    let menus = data.menus.as_ref()?;
                    data.load_image(menus, &format!("VETOBUILD_{i}"), false)
                        .ok()
                        .flatten()
                })
                .collect(),
            button_frame: data
                .load_image(&data.controls, "CBUT_BK", false)
                .ok()
                .flatten()
                .map(Rc::new),
            button_font: [
                TextRenderer::load_in(&data.controls, "BUT_FONT", 0, Some(&data.palette))?
                    .with_spacing(0),
                TextRenderer::load_in(&data.controls, "BUT_FONT", 1, Some(&data.palette))?
                    .with_spacing(0),
            ],
            info_art: crate::object_info::InfoArt::load(data, theme),
            panel_art: crate::info_panel::InfoArt::load(data, theme),
            arming_box: load("TRIGGER_BKGND", false).ok(),
            arming_tick: data
                .load_image(&data.controls, &format!("{theme}MARKER"), false)
                .ok()
                .flatten(),
            button_marks: (0..6)
                .map(|i| {
                    data.load_image(&data.controls, &format!("CBUT_SPEC_{i:02}"), false)
                        .ok()
                        .flatten()
                })
                .collect(),
        })
    }

    /// The interface's placement, the control bar on the left with
    /// `bar_left` ([0x5ca54a]): the bar at x 0 and the field after it
    /// (0x512ab0, 0x504ef0, 0x511170), else the field at 0 and the bar
    /// after it. With `bar_hidden` the field at 0 the whole width and the
    /// bar past the screen's right edge either side (0x50f910 message 0x24
    /// with `+0x18` 1: 0x512ab0(x) = 800 + x, 0x504ef0(x) = x).
    pub fn layout(
        &self,
        window: (u32, u32),
        policy: &ScalePolicy,
        scene: &Scene,
        bar_left: bool,
        bar_hidden: bool,
    ) -> Layout {
        let scale = policy.base_scale(window.1);
        let ui_w = (window.0 as f32 / scale) as u32;
        let (field_w, field_x, bar_x) = if bar_hidden {
            (ui_w.max(2 * FRAME_EDGE), 0, ui_w as f32 * scale)
        } else {
            let field_w = ui_w.saturating_sub(BAR_W).max(2 * FRAME_EDGE);
            if bar_left {
                (field_w, BAR_W, 0.0)
            } else {
                (field_w, 0, field_w as f32 * scale)
            }
        };
        let to_px = |v: u32| (v as f32 * scale).round() as u32;
        let (vx, vy, vw, vh) = self.field_view;
        // The view at the field's (8, 8) (Mapa.cpp 0x51cdd0, mode 1:
        // 0x511170(0x67, 8)), 9 cells across; with the bar hidden at
        // (16, 8), 12 across.
        let (vx, vy, vw, frame_w) = if bar_hidden {
            let cell = openconquest_formats::map::TILE_SIZE;
            let (x, y) = VIEW_HIDDEN;
            (x, y, VIEW_CELLS_HIDDEN * cell, ORIGINAL_FIELD_FS_W)
        } else {
            (vx, vy, vw, ORIGINAL_FIELD_W)
        };
        let view = Screen {
            x: to_px(field_x + vx),
            y: to_px(vy),
            w: to_px((vw as i32 + field_w as i32 - frame_w as i32).max(16) as u32),
            h: to_px(vh),
            window_h: window.1,
        };
        let (mx, my, mw, mh) = self.minimap_area;
        let minimap = MinimapRect::window(
            scene,
            (bar_x + ((MINIMAP_POS.0 + mx) as f32 * scale)) as u32,
            to_px(MINIMAP_POS.1 + my),
            to_px(mw),
            to_px(mh),
        );
        Layout {
            scale,
            view,
            minimap,
            field_w,
            field_x,
            bar_x,
            bar_hidden,
        }
    }

    /// Draws the interface over the (already rendered) world view, the
    /// object panel's background as `back` says.
    pub fn draw(&self, frame: &mut Frame, layout: &Layout, back: PanelBack) {
        let s = layout.scale;
        // Frame (`GAME_FIELD` at the field's x, `GAME_FIELD_FS` at 0 with
        // the bar hidden, 0x504620 message 5): left and right edges as
        // they are, the middle tiled.
        let (f, fw) = if layout.bar_hidden {
            (&self.field_fs, ORIGINAL_FIELD_FS_W)
        } else {
            (&self.field, ORIGINAL_FIELD_W)
        };
        let fx = layout.field_x;
        blit_scaled(frame, f, (0, 0, FRAME_EDGE, UI_H), fx as f32 * s, 0.0, s);
        let mid = (FRAME_EDGE, 0, fw - 2 * FRAME_EDGE, UI_H);
        let right_at = layout.field_w - FRAME_EDGE;
        let mut x = FRAME_EDGE;
        while x < right_at {
            let w = mid.2.min(right_at - x);
            blit_scaled(frame, f, (mid.0, 0, w, UI_H), (fx + x) as f32 * s, 0.0, s);
            x += w;
        }
        blit_scaled(
            frame,
            f,
            (fw - FRAME_EDGE, 0, FRAME_EDGE, UI_H),
            (fx + right_at) as f32 * s,
            0.0,
            s,
        );
        let full = |img: &Image| (0, 0, img.width, img.height);
        // The strip below the view: the money panel at the field's right
        // end less 106 (694 with the bar hidden), the wind rose at 3, the
        // message line `GAME_MEDIT` (`GAME_MEDIT_FS` hidden) at 107.
        let (mx, my) = self.money_panel_origin(layout);
        blit_scaled(frame, &self.money_panel, full(&self.money_panel), mx, my, s);
        blit_scaled(
            frame,
            &self.wind_rose,
            full(&self.wind_rose),
            (fx + WIND_ROSE.0) as f32 * s,
            WIND_ROSE.1 as f32 * s,
            s,
        );
        let line = if layout.bar_hidden {
            &self.message_line_fs
        } else {
            &self.message_line
        };
        let (lx, ly) = ((fx + MESSAGE_LINE.0) as f32 * s, MESSAGE_LINE.1 as f32 * s);
        blit_scaled(frame, line, full(line), lx, ly, s);
        // The bar (at 800 with it hidden: nothing of it on the screen).
        if layout.bar_hidden {
            return;
        }
        // Control bar and its panels.
        blit_scaled(frame, &self.bar, full(&self.bar), layout.bar_x, 0.0, s);
        let at = |(x, y): (u32, u32)| (layout.bar_x + x as f32 * s, y as f32 * s);
        let (mx, my) = at(MINIMAP_POS);
        blit_scaled(
            frame,
            &self.minimap_frame,
            full(&self.minimap_frame),
            mx,
            my,
            s,
        );
        match back {
            PanelBack::Object => {
                let (ox, oy) = at(OBJECT_PANEL_POS);
                blit_scaled(
                    frame,
                    &self.object_panel,
                    full(&self.object_panel),
                    ox,
                    oy,
                    s,
                );
                self.draw_strip(frame, layout, true);
            }
            // No strip with a group (c_ctrl 0x4f06f0) or nothing selected.
            PanelBack::Group(n) => self.panel_art.draw_group_back(frame, layout.bar_x, s, n),
            PanelBack::Clear => self.panel_art.draw_clear(frame, layout.bar_x, s),
        }
    }

    fn money_panel_origin(&self, layout: &Layout) -> (f32, f32) {
        let s = layout.scale;
        (
            (layout.field_x + layout.field_w.saturating_sub(MONEY_PANEL.0)) as f32 * s,
            MONEY_PANEL.1 as f32 * s,
        )
    }

    /// Draws the wind arrow over the rose.
    fn draw_wind(&self, frame: &mut Frame, layout: &Layout, wind: &crate::weather::Wind) {
        let Some(Some(arrow)) = wind
            .rose_frame()
            .map(|i| self.wind_arrows.get(i as usize).and_then(Option::as_ref))
        else {
            return;
        };
        let s = layout.scale;
        blit_scaled(
            frame,
            arrow,
            (0, 0, arrow.width, arrow.height),
            (layout.field_x + WIND_ROSE.0) as f32 * s,
            WIND_ROSE.1 as f32 * s,
            s,
        );
    }

    /// Draws the player's amber and fish with their animated icons.
    fn draw_resources(
        &self,
        frame: &mut Frame,
        layout: &Layout,
        amber: u32,
        fish: u32,
        time_ms: u64,
    ) {
        let s = layout.scale;
        let (px, py) = self.money_panel_origin(layout);
        let step = (time_ms / ICON_FRAME_MS) as usize;
        for (icon, (ix, iy)) in [(&self.money_icon, MONEY_ICON), (&self.fish_icon, FISH_ICON)] {
            if let Some(img) = icon.get(step % icon.len().max(1)) {
                blit_scaled(
                    frame,
                    img,
                    (0, 0, img.width, img.height),
                    px + ix as f32 * s,
                    py + iy as f32 * s,
                    s,
                );
            }
        }
        // Each number, `%7d`, centred in a 50x20 box (0x505530).
        let text_scale = s.round().max(1.0) as u32;
        for (k, value) in [amber, fish].into_iter().enumerate() {
            let font = &self.digits[k];
            let text = format!("{value:>7}");
            let (bx, by) = MONEY_TEXT[k];
            // The number's bitmap is black throughout, over the box's rim.
            let (x0, y0) = (px + bx as f32 * s, py + by as f32 * s);
            let (x1, y1) = (x0 + MONEY_BOX.0 as f32 * s, y0 + MONEY_BOX.1 as f32 * s);
            for y in (y0.round() as u32)..(y1.round() as u32).min(frame.height) {
                for x in (x0.round() as u32)..(x1.round() as u32).min(frame.width) {
                    frame.put(x as i64, y as i64, 0);
                }
            }
            let left = (MONEY_BOX.0.saturating_sub(font.measure(&text))) / 2;
            let top = (MONEY_BOX.1.saturating_sub(font.line_height())) / 2;
            font.draw(
                frame,
                (px + (bx + left) as f32 * s) as i32,
                (py + (by + top) as f32 * s) as i32,
                text_scale,
                &text,
            );
        }
    }

    /// Writes a message into the message line.
    fn draw_message(&self, frame: &mut Frame, layout: &Layout, text: &TextRenderer, msg: &str) {
        let s = layout.scale;
        let text_scale = s.round().max(1.0) as u32;
        let (x, y, _, h) = self.message_area;
        let left = (layout.field_x + MESSAGE_LINE.0) as f32 * s + (x as f32 + 3.0) * s;
        let top = MESSAGE_LINE.1 as f32 * s
            + y as f32 * s
            + (h as f32 * s - (text.line_height() * text_scale) as f32) / 2.0;
        text.draw(frame, left as i32, top as i32, text_scale, msg);
    }

    /// An object panel picture (a bitmap in CONTROLG), cached.
    pub fn picture(&self, data: &GameData, name: &str) -> Option<Rc<Image>> {
        self.pictures
            .borrow_mut()
            .entry(name.to_string())
            .or_insert_with(|| {
                let b =
                    Bitmap::parse(&data.controls.read(name, ResourceKind::Bitmap).ok()?).ok()?;
                let pixels = b
                    .pixels
                    .iter()
                    .map(|&i| Some(rgb(b.palette.0[i as usize])))
                    .collect();
                Some(Rc::new(Image::new(b.width, b.height, pixels)))
            })
            .clone()
    }

    /// The frames of command button `base` (`<base>_00`, `<base>_01`, ...).
    fn button(&self, data: &GameData, base: &str) -> Rc<Vec<Rc<Image>>> {
        self.buttons
            .borrow_mut()
            .entry(base.to_string())
            .or_insert_with(|| {
                let frames = (0..)
                    .map_while(|i| self.picture(data, &format!("{base}_{i:02}")))
                    .collect();
                Rc::new(frames)
            })
            .clone()
    }

    /// Window rectangle `(x, y, size)` of command slot `i`.
    pub fn button_rect(&self, layout: &Layout, i: usize) -> (f32, f32, f32) {
        let (col, row) = ((i % BUTTON_COLUMNS) as u32, (i / BUTTON_COLUMNS) as u32);
        let s = layout.scale;
        (
            layout.bar_x + (BUTTON_ORIGIN.0 + col * BUTTON_STEP.0) as f32 * s,
            (BUTTON_ORIGIN.1 + row * BUTTON_STEP.1) as f32 * s,
            BUTTON_SIZE as f32 * s,
        )
    }

    /// Where the "default arming" checkbox and its caption are clicked:
    /// the box at (7, 371) of the bar, 20 pixels, and the caption after it
    /// (measured on the original).
    pub fn arming_rect(&self, layout: &Layout) -> (f32, f32, f32, f32) {
        let s = layout.scale;
        (layout.bar_x + 7.0 * s, 371.0 * s, 140.0 * s, 20.0 * s)
    }

    /// Draws the "default arming" checkbox.
    pub fn draw_arming(
        &self,
        frame: &mut Frame,
        layout: &Layout,
        text: &TextRenderer,
        caption: &str,
        on: bool,
    ) {
        let (x, y, _, _) = self.arming_rect(layout);
        let s = layout.scale;
        if let Some(b) = &self.arming_box {
            blit_scaled(frame, b, (0, 0, b.width, b.height), x, y, s);
        }
        if let (true, Some(m)) = (on, &self.arming_tick) {
            blit_scaled(
                frame,
                m,
                (0, 0, m.width, m.height),
                x + 3.0 * s,
                y + 3.0 * s,
                s,
            );
        }
        let ts = (s.round() as u32).max(1);
        text.draw(
            frame,
            (x + 30.0 * s) as i32,
            (y + 2.0 * s) as i32,
            ts,
            caption,
        );
    }

    /// The command slot under window position `(x, y)`.
    pub fn button_at(&self, layout: &Layout, x: f32, y: f32) -> Option<usize> {
        (0..SLOTS).find(|&i| {
            let (bx, by, size) = self.button_rect(layout, i);
            x >= bx && y >= by && x < bx + size && y < by + size
        })
    }

    /// Draws a command menu. The button under the cursor plays its
    /// animation (the original loads every frame, 0x4f31b0 and 0x542f20,
    /// unless a low-detail setting keeps only the first and the greyed
    /// last) and is framed; disabled buttons show the greyed last frame; a
    /// running order (such as a ship being built) shows its progress
    /// through the frames.
    #[allow(clippy::too_many_arguments)]
    fn draw_menu(
        &self,
        frame: &mut Frame,
        data: &GameData,
        layout: &Layout,
        menu: &Menu,
        nation: u32,
        hover: Option<usize>,
        time_ms: u64,
    ) {
        for (i, slot) in menu.iter().enumerate() {
            let Some(slot) = slot else { continue };
            let Some(base) = slot.command.button(nation) else {
                continue;
            };
            let frames = self.button(data, &base);
            let Some(last) = frames.last() else { continue };
            let animated = frames.len() > 2;
            let img = if !slot.enabled {
                last
            } else if hover == Some(i) && animated {
                &frames[(time_ms / BUTTON_FRAME_MS) as usize % (frames.len() - 1)]
            } else {
                &frames[0]
            };
            // The rim `CBUT_BK` two pixels down, then the picture a pixel
            // right of the slot (0x4f2a90; pressed, a pixel down instead).
            let (x, y, _) = self.button_rect(layout, i);
            let s = layout.scale;
            if let Some(bk) = &self.button_frame {
                blit_scaled(frame, bk, (0, 0, bk.width, bk.height), x, y + 2.0 * s, s);
            }
            let x = x + s;
            blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
            // Marks: an arrow on buttons that open a page, a tick on those
            // switched on (grey on disabled buttons); then the caption.
            let marks = [
                (slot.closes, 0),
                (slot.command.opens_page() && !slot.closes, 2),
                (slot.checked, 4),
            ];
            for (_, mark) in marks.iter().filter(|(on, _)| *on) {
                let mark = mark + usize::from(!slot.enabled);
                if let Some(Some(m)) = self.button_marks.get(mark) {
                    blit_scaled(frame, m, (0, 0, m.width, m.height), x, y, s);
                }
            }
            if let Some(caption) = data.strings.get(&slot.command.caption_id()) {
                self.draw_caption(frame, layout, caption, slot.enabled, (x, y));
            }
            // Work in hand: the part still to do shaded (0x54a580), then
            // the count of items ordered on buttons 0x21-0x39 in
            // `NUMBER_FONT` at (3, 3).
            if let Some(p) = slot.progress {
                self.shade_unfinished(frame, (x, y), s, p);
            }
            if slot.count > 0 && (0x21..0x3a).contains(&slot.command.0) {
                self.number_font.draw(
                    frame,
                    (x + 3.0 * s) as i32,
                    (y + 3.0 * s) as i32,
                    s.round().max(1.0) as u32,
                    &slot.count.to_string(),
                );
            }
        }
    }

    /// Shades the part of a button not yet done, as the original's clock
    /// (0x54a580): swept clockwise from 12 o'clock by `done` (0..1) about
    /// the picture's centre, the rest remapped through the shading table at
    /// 0x5c1658. Nothing at all once done.
    fn shade_unfinished(&self, frame: &mut Frame, (x, y): (f32, f32), s: f32, done: f32) {
        let pct = (done * 100.0) as i32;
        if pct > 99 {
            return;
        }
        let size = BUTTON_SIZE as i32;
        let centre = (size / 2) as f32;
        let sweep = pct.max(0) as f32 * 3.6;
        let (x0, y0) = (x.round() as i64, y.round() as i64);
        let n = (size as f32 * s).round() as i64;
        for py in 0..n {
            for px in 0..n {
                let (bx, by) = (px as f32 / s, py as f32 / s);
                let (dx, dy) = (bx - centre, centre - by);
                let angle = dx.atan2(dy).to_degrees().rem_euclid(360.0);
                if angle >= sweep {
                    let (fx, fy) = (x0 + px, y0 + py);
                    if let Some(c) = frame.get(fx, fy) {
                        if let Some(&d) = self.shade.get(&c) {
                            frame.put(fx, fy, d);
                        }
                    }
                }
            }
        }
    }

    /// A button's caption (0x4f2a90): its lines centred one above the
    /// other, `BUT_FONT` lines 2 pixels closer than the font's height, the
    /// block at the bottom of the button.
    fn draw_caption(
        &self,
        frame: &mut Frame,
        layout: &Layout,
        caption: &str,
        enabled: bool,
        (x, y): (f32, f32),
    ) {
        let font = &self.button_font[usize::from(!enabled)];
        let s = layout.scale;
        let ts = s.round().max(1.0) as u32;
        let lines: Vec<&str> = caption.split('\n').collect();
        // The original sets the lines 11 pixels high (2 less than the
        // font's height) and 2 closer still: 9 apart (measured).
        let cell = font.line_height() as i32 - 2;
        let pitch = cell - 2;
        let height = lines.len() as i32 * cell - 2 * (lines.len() as i32 - 1);
        for (k, line) in lines.iter().enumerate() {
            let left = (BUTTON_SIZE as i32 - font.measure(line) as i32) / 2;
            let top = BUTTON_SIZE as i32 - height + k as i32 * pitch;
            font.draw(
                frame,
                (x + left as f32 * s) as i32,
                (y + top as f32 * s) as i32,
                ts,
                line,
            );
        }
    }

    /// The strip above the command buttons: the relief on a main page.
    fn draw_strip(&self, frame: &mut Frame, layout: &Layout, main: bool) {
        if let Some(img) = &self.strips[usize::from(!main)] {
            let (w, h) = (img.width, img.height);
            let s = layout.scale;
            blit_scaled(
                frame,
                img,
                (0, 0, w, h),
                layout.bar_x,
                STRIP_Y as f32 * s,
                s,
            );
        }
    }

    /// The selected object's portrait in the object panel's picture slot.
    fn draw_portrait(
        &self,
        frame: &mut Frame,
        data: &GameData,
        layout: &Layout,
        obj: &crate::objects::MapObject,
    ) {
        if let Some((name, (px, py))) = &obj.picture {
            if let Some(img) = self.picture(data, name) {
                let (sx, sy, _, _) = self.picture_slot(layout);
                let (ox, oy) = self.picture_slot_offset();
                blit_scaled(
                    frame,
                    &img,
                    (0, 0, img.width, img.height),
                    sx + (*px as f32 - ox as f32) * layout.scale,
                    sy + (*py as f32 - oy as f32) * layout.scale,
                    layout.scale,
                );
            }
        }
    }

    /// The object panel's slot cells, or the group's buttons, their slider
    /// and the cell held down shown big (`info_panel`, c_info.cpp).
    #[allow(clippy::too_many_arguments)]
    fn draw_info(
        &self,
        frame: &mut Frame,
        data: &GameData,
        layout: &Layout,
        scene: &Scene,
        shown: &crate::info_panel::Shown,
        view: &crate::info_panel::InfoView,
    ) {
        use crate::info_panel::{self as ip, Hit, Shown};
        let s = layout.scale;
        let bx = layout.bar_x;
        let at = |x: u32, y: u32| (bx + x as f32 * s, y as f32 * s);
        let mut big = None;
        match shown {
            Shown::Clear => {}
            Shown::Group(ids) => {
                for (i, x, y) in ip::group_cells(ids.len(), view.scroll) {
                    let Some(o) = scene.object_index(ids[i]).map(|k| &scene.objects[k]) else {
                        continue;
                    };
                    let portrait = ip::member_portrait(o).and_then(|n| self.picture(data, &n));
                    self.panel_art.draw_member(
                        frame,
                        at(x, y),
                        s,
                        portrait.as_deref(),
                        crate::object_info::life(o),
                        &self.bar_colours,
                        view.pressed == Some(Hit::Member(i)),
                    );
                }
            }
            Shown::Single(id, list) => {
                let Some(obj) = scene.object_index(*id).map(|k| &scene.objects[k]) else {
                    return;
                };
                let nation3 = obj.owner.is_some_and(|o| o.0 == 3);
                if ip::list_scroll_back(shown) {
                    if let Some(img) = &self.load_art.scroll_back[0] {
                        let (x, y) = at(168, 212);
                        blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
                    }
                }
                for (i, x, y) in ip::cells(list, view.scroll) {
                    let e = &list[i];
                    let cell = SlotCell {
                        frame: e.frame(nation3),
                        bar: e.bar,
                        count: e.count,
                    };
                    self.draw_cell(frame, data, layout, at(x, y), &cell, &e.marks(), false);
                }
                if let Some(e) = view.big.and_then(|i| list.get(i)) {
                    let picture = ip::big_picture(e, nation3).and_then(|n| self.picture(data, &n));
                    let levels = scene.levels_of(scene.human.map(|h| (h.nation, h.slot)));
                    let text =
                        ip::describe(&data.strings, e, &levels, ip::treasure_of(scene, obj, e));
                    big = self.panel_art.big_slot(picture.as_deref(), &text);
                }
            }
        }
        if let Some((group, scroll, max)) = ip::slider(shown, view) {
            self.panel_art.draw_slider(
                frame,
                bx,
                s,
                &self.load_art,
                group,
                (scroll, max),
                view.pressed,
            );
        }
        if let Some(img) = big {
            let (x, y) = at(ip::BIG_SLOT.0, ip::BIG_SLOT.1);
            blit_scaled(frame, &img, (0, 0, img.width, img.height), x, y, s);
        }
    }

    /// One cell (0x4f9520): `COBJ_BKSMSLOT`, the `COBJ_SSLOT` picture at
    /// (1, 1), the health bar, the chosen frame (`COBJ_SACT`), the marks
    /// (`COBJ_SSPEC`) and the count in `NUMBER_FONT` at (3, 3).
    #[allow(clippy::too_many_arguments)]
    fn draw_cell(
        &self,
        frame: &mut Frame,
        data: &GameData,
        layout: &Layout,
        (x, y): (f32, f32),
        cell: &SlotCell,
        more_marks: &[u8],
        chosen: bool,
    ) {
        let s = layout.scale;
        let (w, h) = self
            .small_slot
            .as_ref()
            .map_or((47, 50), |b| (b.width, b.height));
        if let Some(bk) = &self.small_slot {
            blit_scaled(frame, bk, (0, 0, w, h), x, y, s);
        }
        if let Some(img) = self.picture(data, &format!("COBJ_SSLOT_{:02}", cell.frame)) {
            blit_scaled(frame, &img, (0, 0, img.width, img.height), x + s, y + s, s);
        }
        if let Some(pct) = cell.bar {
            // A black bar 4 high, then two lines of colour (0x4f9520).
            let len = (w - 2) * pct as u32 / 100;
            let colours = if pct >= 50 {
                self.bar_colours[0]
            } else if pct >= 25 {
                self.bar_colours[1]
            } else {
                self.bar_colours[2]
            };
            let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32, c: u32| {
                let (px0, py0) = ((x + x0 as f32 * s) as i64, (y + y0 as f32 * s) as i64);
                let (px1, py1) = ((x + x1 as f32 * s) as i64, (y + y1 as f32 * s) as i64);
                for py in py0..py1 {
                    for px in px0..px1 {
                        frame.put(px, py, c);
                    }
                }
            };
            fill(1, h - 5, 1 + len, h - 1, 0);
            if len > 2 {
                fill(2, h - 4, len, h - 3, colours[0]);
                fill(2, h - 3, len, h - 2, colours[1]);
            }
        }
        // The chosen cell's frame (`COBJ_SACT`).
        if chosen {
            if let Some(img) = &self.load_art.active {
                blit_scaled(frame, img, (0, 0, img.width, img.height), x + s, y + s, s);
            }
        }
        for mark in more_marks {
            if let Ok(Some(img)) =
                data.load_image(&data.controls, &format!("COBJ_SSPEC_{mark:02}"), false)
            {
                blit_scaled(frame, &img, (0, 0, img.width, img.height), x + s, y + s, s);
            }
        }
        if cell.count > 0 {
            self.number_font.draw(
                frame,
                (x + 3.0 * s) as i32,
                (y + 3.0 * s) as i32,
                s.round().max(1.0) as u32,
                &cell.count.to_string(),
            );
        }
    }

    /// The load panel (c_load.cpp; see `load_panel`): the selected
    /// object's holder above with its portrait, its partner's below, the
    /// two lists, Kill item and Finish reload, and a slider on a list that
    /// scrolls.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_load_panel(
        &self,
        frame: &mut Frame,
        data: &GameData,
        layout: &Layout,
        scene: &Scene,
        panel: &load_panel::Lists,
        view: &load_panel::LoadView,
        pressed: Option<load_panel::Hit>,
    ) {
        use load_panel::{
            can_kill, cells, max_scroll, scrolls, Hit, ARROW_X, ARROW_Y, BUTTON, END, KILL,
            PANEL_X, PANEL_Y, SCROLL_BACK, SIDE_Y, TRACK, TRACK_SIZE,
        };
        let s = layout.scale;
        let bx = layout.bar_x;
        let at = |x: u32, y: u32| (bx + x as f32 * s, y as f32 * s);
        let art = &self.load_art;
        let backs = [Some(&self.object_panel), art.bottom.as_ref()];
        for side in 0..2 {
            let holder = panel.holders[side];
            let obj = scene.object_index(holder).map(|i| &scene.objects[i]);
            if let Some(bk) = backs[side] {
                let (x, y) = at(PANEL_X, PANEL_Y[side]);
                blit_scaled(frame, bk, (0, 0, bk.width, bk.height), x, y, s);
            }
            let list = &panel.entries[side];
            if scrolls(list) {
                if let Some(img) = &art.scroll_back[side] {
                    let (x, y) = at(PANEL_X + SCROLL_BACK.0, PANEL_Y[side] + SCROLL_BACK.1[side]);
                    blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
                }
            }
            // The holder's portrait as on the object panel (0x4f8ad0).
            if let Some((name, (px, py))) = obj.and_then(|o| o.picture.as_ref()) {
                if let Some(img) = self.picture(data, name) {
                    let (x, y) = at(PANEL_X + *px, PANEL_Y[side] + *py);
                    blit_scaled(frame, &img, (0, 0, img.width, img.height), x, y, s);
                }
            }
            let nation3 = obj.and_then(|o| o.owner).is_some_and(|o| o.0 == 3);
            for (i, cx, cy) in cells(list, side, view.scroll[side]) {
                let e = &list[i];
                let marks = e.marks();
                let cell = SlotCell {
                    frame: e.frame(nation3),
                    bar: e.bar,
                    count: e.count,
                };
                self.draw_cell(
                    frame,
                    data,
                    layout,
                    at(cx, cy),
                    &cell,
                    &marks,
                    view.chosen == (side, i),
                );
            }
            if scrolls(list) {
                let y0 = SIDE_Y[side];
                if let Some(img) = &art.track[side] {
                    let (x, y) = at(TRACK.0, y0 + TRACK.1);
                    blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
                }
                let max = max_scroll(list.len() - 1).max(1);
                if let Some(img) = &art.thumb {
                    let room = TRACK_SIZE.1 - img.height;
                    let k = view.scroll[side].min(max) as u32 * room / max as u32;
                    let (x, y) = at(TRACK.0 + 4, y0 + TRACK.1 + k);
                    blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
                }
                for (arrows, dy, hit) in [
                    (&art.up, ARROW_Y[0], Hit::Up(side)),
                    (&art.down, ARROW_Y[1], Hit::Down(side)),
                ] {
                    let f = usize::from(pressed == Some(hit)) * 2;
                    if let Some(img) = arrows.get(f).or(arrows.first()) {
                        let (x, y) = at(ARROW_X, y0 + dy);
                        blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
                    }
                }
            }
        }
        // Kill item and Finish reload (0x4e9ca0): frame 1 pressed, the
        // caption (strings 474 and 473) in `INT_FONT`, centred.
        let kill = can_kill(panel, view);
        for (pos, frames, id, hit, enabled) in [
            (KILL, &art.kill, 474, Hit::Kill, kill),
            (END, &art.end, 473, Hit::End, true),
        ] {
            let down = pressed == Some(hit) && enabled;
            let (x, y) = at(pos.0, pos.1);
            if let Some(img) = frames.get(usize::from(down)).or(frames.first()) {
                blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, s);
            }
            let caption = data.strings.get(&id).map_or(
                if id == 474 {
                    "Kill item"
                } else {
                    "Finish reload"
                },
                |c| c.as_str(),
            );
            let font = if enabled {
                art.font.as_ref()
            } else {
                Some(&self.button_font[1])
            };
            if let Some(font) = font {
                let ts = s.round().max(1.0) as u32;
                let w = font.measure(caption) as f32;
                let h = font.line_height() as f32;
                let shift = if down { 2.0 } else { 0.0 };
                let tx = x + ((BUTTON.0 as f32 - w) / 2.0 - shift) * s;
                let ty = y + ((BUTTON.1 as f32 - h) / 2.0 + shift) * s;
                font.draw(frame, tx as i32, ty as i32, ts, caption);
            }
        }
    }

    /// Offset of the picture slot within the object panel.
    pub fn picture_slot_offset(&self) -> (u32, u32) {
        (self.picture_slot.0, self.picture_slot.1)
    }

    /// Window rectangle of the object panel's picture slot.
    pub fn picture_slot(&self, layout: &Layout) -> (f32, f32, f32, f32) {
        let s = layout.scale;
        let (x, y, w, h) = self.picture_slot;
        (
            layout.bar_x + (OBJECT_PANEL_POS.0 + x) as f32 * s,
            (OBJECT_PANEL_POS.1 + y) as f32 * s,
            w as f32 * s,
            h as f32 * s,
        )
    }
}

/// The largest rectangle of black pixels grown outwards from `(x, y)`.
fn black_rect(img: &Image, x: u32, y: u32) -> Rect {
    let black = |x: u32, y: u32| img.pixels[(y * img.width + x) as usize].is_none_or(|p| p == 0);
    let (mut x0, mut y0, mut x1, mut y1) = (x, y, x + 1, y + 1);
    loop {
        let mut grew = false;
        if x0 > 0 && (y0..y1).all(|y| black(x0 - 1, y)) {
            x0 -= 1;
            grew = true;
        }
        if x1 < img.width && (y0..y1).all(|y| black(x1, y)) {
            x1 += 1;
            grew = true;
        }
        if y0 > 0 && (x0..x1).all(|x| black(x, y0 - 1)) {
            y0 -= 1;
            grew = true;
        }
        if y1 < img.height && (x0..x1).all(|x| black(x, y1)) {
            y1 += 1;
            grew = true;
        }
        if !grew {
            return (x0, y0, x1 - x0, y1 - y0);
        }
    }
}

/// Everything needed to draw one frame of the game screen.
pub struct GameView<'a> {
    pub data: &'a GameData,
    pub scene: &'a Scene,
    pub camera: &'a crate::view::Camera,
    pub interface: &'a Interface,
    /// The bar hidden (Space, `[game+0x328d]`).
    pub bar_hidden: bool,
    pub text: &'a TextRenderer,
    pub selected: Option<u32>,
    /// Other ships selected with it.
    pub group: &'a [u32],
    /// When each selected object was selected (`time_ms`), for the frame's
    /// zoom.
    pub selected_since: &'a HashMap<u32, u64>,
    /// When objects were deselected (`time_ms`), for their frames' zoom out.
    pub deselected: &'a HashMap<u32, u64>,
    /// A selection box being dragged (window pixels).
    pub select_box: Option<(f32, f32, f32, f32)>,
    pub status: Option<&'a str>,
    /// The object under the pointer, named in the message line.
    pub hover_object: Option<u32>,
    pub time_ms: u64,
    /// Command slot under the cursor.
    pub hover_button: Option<usize>,
    /// The configuration's "button animation": a hovered button's picture
    /// plays (off, 0x4f2a90 keeps its first frame).
    pub button_animation: bool,
    /// The object panel's state (`info_panel`).
    pub info: crate::info_panel::InfoView,
    /// Where the player last sent ships (world point) and how many
    /// milliseconds ago.
    pub sent: Option<(f32, f32, u64)>,
    /// Which menu of the selected object is shown.
    pub page: Page,
    /// A building site being chosen: the cell and whether it is allowed.
    pub site: Option<((i32, i32), bool)>,
    /// The building standing where it would go, shown in place of the
    /// site's frame when it may go there.
    pub site_preview: Option<&'a crate::objects::MapObject>,
    /// How many cells across the building to put up is.
    pub site_cells: i32,
    /// The radar map in place of the view (Tab).
    pub radar: Option<Radar>,
    /// The command keys, told on the buttons' help line.
    pub hotkeys: &'a crate::hotkeys::Hotkeys,
    /// Show the mission briefing over the view.
    pub briefing: bool,
    /// The bar's tabs and open panel (with the interface shown).
    pub panel: Option<PanelView<'a>>,
    /// The bar-relative point under the cursor, in original pixels.
    pub panel_hover: Option<(u32, u32)>,
    /// The game is paused.
    pub paused: bool,
    /// A question asked, and the answer held down.
    pub ask: Option<(String, bool, Option<bool>)>,
    /// The game's messages shown over the view, oldest first (empty with
    /// "message pop-up" off).
    pub chat: &'a [String],
    /// The load panel's state, and the panel button held down.
    pub load: Option<(load_panel::LoadView, Option<load_panel::Hit>)>,
    /// What the strip below the view tells of the object under the
    /// pointer (see `object_info`).
    pub object_info: Option<crate::object_info::ObjectInfo>,
    /// The control bar on the left ([0x5ca54a]).
    pub bar_left: bool,
}

impl GameView<'_> {
    /// The mission briefing in a box over the view; the first paragraph
    /// is the title.
    fn draw_briefing(&self, frame: &mut Frame, screen: &Screen, scale: u32) {
        let line = self.text.line_height() * scale;
        let pad = 12 * scale;
        let width = (screen.w * 3 / 5).max(200 * scale);
        let mut lines: Vec<String> = Vec::new();
        let paragraphs = self.scene.briefing.iter().filter(|p| !p.is_empty());
        for (i, p) in paragraphs.enumerate() {
            if i > 0 {
                lines.push(String::new());
            }
            lines.extend(wrap(self.text, p, width / scale));
        }
        lines.push(String::new());
        lines.push("Click or press Enter to begin (B shows this again)".to_string());
        let height = (lines.len() as u32 * line).min(screen.h.saturating_sub(2 * pad));
        let x = screen.x + screen.w.saturating_sub(width) / 2;
        let y = screen.y + screen.h.saturating_sub(height) / 2;
        shade_rect(
            frame,
            x.saturating_sub(pad),
            y.saturating_sub(pad),
            width + 2 * pad,
            height + 2 * pad,
        );
        for (i, l) in lines.iter().enumerate() {
            let ly = y + i as u32 * line;
            if ly + line > y + height {
                break;
            }
            self.text.draw(frame, x as i32, ly as i32, scale, l);
        }
    }

    /// The pause sign in the middle of the view (text without the
    /// interface's art).
    fn draw_pause(&self, frame: &mut Frame, screen: &Screen, scale: f32) {
        let sign = Some(&self.interface.pause_sign).filter(|s| !s.is_empty());
        match sign {
            Some(frames) => {
                let img = &frames[(self.time_ms / 60) as usize % frames.len()];
                let x = screen.x as f32 + (screen.w as f32 - img.width as f32 * scale) / 2.0;
                let y = screen.y as f32 + (screen.h as f32 - img.height as f32 * scale) / 2.0;
                blit_scaled(frame, img, (0, 0, img.width, img.height), x, y, scale);
            }
            None => {
                let ts = scale.round().max(1.0) as u32 * 2;
                let w = self.text.measure("Paused") * ts;
                let x = screen.x + screen.w.saturating_sub(w) / 2;
                let y = screen.y + screen.h / 2;
                self.text.draw(frame, x as i32, y as i32, ts, "Paused");
            }
        }
    }

    /// A banner across the view announcing the end of the mission.
    fn draw_outcome(
        &self,
        frame: &mut Frame,
        screen: &Screen,
        outcome: crate::scene::Outcome,
        scale: u32,
    ) {
        let text = match outcome {
            crate::scene::Outcome::Victory => "Victory! The mission is accomplished.",
            crate::scene::Outcome::Defeat => "Defeat. Your fleet and towns are lost.",
        };
        let scale = scale * 2;
        let (w, h) = (
            self.text.measure(text) * scale,
            self.text.line_height() * scale,
        );
        let pad = 8 * scale;
        let x = screen.x + screen.w.saturating_sub(w) / 2;
        let y = screen.y + screen.h.saturating_sub(h) / 3;
        shade_rect(
            frame,
            x.saturating_sub(pad),
            y.saturating_sub(pad),
            w + 2 * pad,
            h + 2 * pad,
        );
        self.text.draw(frame, x as i32, y as i32, scale, text);
    }

    /// The world viewport and minimap placement for a window size.
    pub fn placement(&self, window: (u32, u32)) -> (Screen, MinimapRect, Layout) {
        let layout = self.interface.layout(
            window,
            &self.camera.policy,
            self.scene,
            self.bar_left,
            self.bar_hidden,
        );
        let (view, minimap) = match self.radar {
            Some(r) => radar_placement(&layout, self.scene, r.origin),
            None => (layout.view, layout.minimap),
        };
        (view, minimap, layout)
    }

    pub fn draw(&self, frame: &mut Frame) {
        let window = (frame.width, frame.height);
        let (screen, minimap, layout) = self.placement(window);
        let lut = palette_lut(self.data, self.time_ms);
        let radar = self.radar.map(|_| layout.view);
        match radar {
            // Tab: the radar map in the view's place (Mapa.cpp mode 2), its
            // frame the view's size; the view goes to the bar's window.
            Some(field) => {
                crate::render::fill_rect(frame, field.x, field.y, field.w, field.h, 0);
                draw_minimap(frame, &minimap, self.scene, self.camera, &field, &lut);
            }
            None => self.draw_view(frame, &screen, self.camera, &lut),
        }
        let selected = self
            .selected
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i]);
        let ui_scale = self.camera.policy.base_scale(window.1);
        let text_scale = ui_scale.round().max(1.0) as u32;
        let panel_open = self
            .panel
            .as_ref()
            .is_some_and(|p| Panels::covers_bar(p.tab));
        self.draw_interface(
            frame,
            &screen,
            &minimap,
            &layout,
            &lut,
            radar.is_some(),
            selected,
            panel_open,
            ui_scale,
            text_scale,
        );
    }

    /// The world in `screen`: terrain, objects, fog and the marks on them.
    fn draw_view(
        &self,
        frame: &mut Frame,
        screen: &Screen,
        camera: &crate::view::Camera,
        lut: &[u32; 256],
    ) {
        let screen = *screen;
        let lut = *lut;
        draw_world(frame, &screen, self.scene, camera, &lut);
        draw_objects(frame, &screen, self.scene, camera, &lut, self.time_ms);
        draw_fog(frame, &screen, self.scene, camera, &lut);
        for (&id, &t) in self.deselected {
            let ticks = (self.time_ms.saturating_sub(t) / crate::clock::TICK_MS) as usize;
            if let Some(zoom) = (crate::render::SELECTION_FRAMES - 2).checked_sub(ticks) {
                crate::render::draw_deselection(
                    frame,
                    &screen,
                    self.data,
                    self.scene,
                    camera,
                    id,
                    self.time_ms,
                    zoom,
                    &lut,
                );
            }
        }
        for &id in self.selected.iter().chain(self.group) {
            // Ticks since it was selected pick the frame's zoom; unknown
            // (a screenshot) shows it whole.
            let zoom = self.selected_since.get(&id).map_or(usize::MAX, |&t| {
                (self.time_ms.saturating_sub(t) / crate::clock::TICK_MS) as usize
            });
            draw_selection(
                frame,
                &screen,
                self.data,
                self.scene,
                camera,
                id,
                self.time_ms,
                zoom,
                &lut,
            );
        }
        // The green tick where ships were sent (gcursor.cpp 0x515e60): its
        // top-left 11 and 24 pixels up-left of the click, a frame each 250
        // ms, gone as it reaches its last (0x5166f8).
        let ui = self.interface;
        if let Some((x, y, age)) = self.sent {
            const TICK_MS: u64 = 250;
            let n = ui.take_cmd.len() as u64;
            if n > 1 && age < (n - 1) * TICK_MS {
                let img = &ui.take_cmd[(age / TICK_MS) as usize];
                let (cx, cy) = (
                    x - 11.0 + img.width as f32 / 2.0,
                    y - 24.0 + img.height as f32 / 2.0,
                );
                crate::render::draw_centred(frame, &screen, camera, &lut, img, cx, cy);
            }
        }
        if let Some(b) = self.select_box {
            crate::render::draw_box(frame, &screen, b);
        }
        // Where it may go, the building as it will stand; where not, the
        // hatched `VETOBUILD` at the site's top-left cell (0x516010:
        // frame 1 for a settlement, else 0).
        let veto = ui
            .veto
            .get(usize::from(self.site_cells >= 2))
            .or(ui.veto.first());
        match (self.site, self.site_preview) {
            (Some((_, true)), Some(b)) => {
                crate::render::draw_object(frame, &screen, camera, &lut, b, self.time_ms)
            }
            (Some((cell, false)), _) if veto.is_some() => {
                let Some(img) = veto else { return };
                let t = openconquest_formats::map::TILE_SIZE as f32;
                let (cx, cy) = (
                    cell.0 as f32 * t + img.width as f32 / 2.0,
                    cell.1 as f32 * t + img.height as f32 / 2.0,
                );
                crate::render::draw_centred(frame, &screen, camera, &lut, img, cx, cy);
            }
            (Some((cell, ok)), _) => draw_site(frame, &screen, camera, cell, self.site_cells, ok),
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_interface(
        &self,
        frame: &mut Frame,
        screen: &Screen,
        minimap: &MinimapRect,
        layout: &Layout,
        lut: &[u32; 256],
        radar: bool,
        selected: Option<&crate::objects::MapObject>,
        panel_open: bool,
        ui_scale: f32,
        text_scale: u32,
    ) {
        let (screen, minimap, lut) = (*screen, *minimap, *lut);
        let ui = self.interface;
        let selection: Vec<u32> = self.selected.iter().chain(self.group).copied().collect();
        let shown = crate::info_panel::shown(self.scene, &selection, &self.info);
        let reloading_now = selected.is_some_and(|o| reloading_shown(self.load, self.scene, o.id));
        let back = match &shown {
            crate::info_panel::Shown::Group(ids) if !reloading_now => PanelBack::Group(ids.len()),
            crate::info_panel::Shown::Clear => PanelBack::Clear,
            _ => PanelBack::Object,
        };
        ui.draw(frame, layout, back);
        // With the bar hidden nothing of it, its panels or its minimap is
        // on the screen (0x512ab0 = 800 + x; the minimap's mapa at
        // 0x511170(0x66, 8) = 808 and its 0xcc3 skipped, 0x51cdd0).
        if let (Some(obj), false, false) = (selected, panel_open, layout.bar_hidden) {
            let group = matches!(back, PanelBack::Group(_));
            if !group {
                ui.draw_portrait(frame, self.data, layout, obj);
            }
            // Reloading: the load panel takes the slots' and the
            // buttons' place (0x4fffe0).
            let reloading = self.load.and_then(|(view, pressed)| {
                let panel = load_panel::lists(self.scene, obj.id, &view)?;
                Some((panel, view, pressed))
            });
            if let Some((panel, view, pressed)) = reloading {
                ui.draw_load_panel(frame, self.data, layout, self.scene, &panel, &view, pressed);
            }
            if reloading_now {
                // Nothing else of the object panel.
            } else {
                // Also over a shipyard's or dock's ships (c_ctrl 0x4f4cc0:
                // panel kind 0x13), in place of the strip's relief.
                let yard = self.page == Page::Main
                    && matches!(
                        obj.class,
                        openconquest_formats::ObjectClass::Building(
                            crate::economy::SHIPYARD | crate::economy::DOCK
                        )
                    );
                if !group && (self.page != Page::Main || yard && self.scene.is_own(obj)) {
                    ui.draw_strip(frame, layout, false);
                }
                if (matches!(self.page, Page::Structures | Page::Ground) || yard)
                    && self.scene.is_own(obj)
                {
                    let caption = self
                        .data
                        .strings
                        .get(&501)
                        .map_or("default arming", |s| s.as_str());
                    ui.draw_arming(frame, layout, self.text, caption, self.scene.default_arming);
                }
                let nation = self.scene.human.map_or(1, |h| h.nation);
                ui.draw_menu(
                    frame,
                    self.data,
                    layout,
                    &menu(self.scene, obj, self.page),
                    nation,
                    self.hover_button.filter(|_| self.button_animation),
                    self.time_ms,
                );
                // Only the portrait and the slots, or the group's
                // buttons: the original writes nothing more about the
                // selection (c_info.cpp: a slot held down tells what
                // it holds).
                ui.draw_info(frame, self.data, layout, self.scene, &shown, &self.info);
            }
        }
        if !layout.bar_hidden && !panel_open {
            if let Some(r) = self.radar.filter(|_| radar) {
                self.draw_view(frame, &screen, &r.view, &lut);
            } else {
                draw_minimap(frame, &minimap, self.scene, self.camera, &screen, &lut);
            }
        }
        if let (false, Some(view)) = (layout.bar_hidden, &self.panel) {
            let origin = (layout.bar_x, 0.0);
            ui.panels.draw(
                frame,
                origin,
                layout.scale,
                view,
                self.text,
                self.panel_hover,
            );
        }
        // The messages (chat.cpp 0x4ff390, 0x4ff4c0): a line every 20
        // pixels from the field's (8, 8) ((16, 8) with the bar hidden), in
        // `MESS_FONT`, each on a bitmap of 9 (12) cells' width.
        if let Some(font) = ui.mess_font.as_ref() {
            let hidden = usize::from(layout.bar_hidden);
            let s = layout.scale;
            let x = (layout.field_x + CHAT_X[hidden]) as f32 * s;
            let cell = openconquest_formats::map::TILE_SIZE;
            let right = x + (CHAT_CELLS[hidden] * cell) as f32 * s;
            for (k, line) in self.chat.iter().enumerate() {
                let y = (CHAT_Y.0 + CHAT_Y.1 * k as u32) as f32 * s;
                font.draw_clipped(frame, x as i32, y as i32, text_scale, line, right as i32);
            }
        }
        if let Some(outcome) = self.scene.outcome {
            self.draw_outcome(frame, &screen, outcome, text_scale);
        }
        if let Some((text, yes_no, pressed)) = &self.ask {
            let ask = crate::panels::AskView {
                text,
                yes_no: *yes_no,
                pressed: *pressed,
            };
            // Centred on the view, as the original on its screen.
            let (w, h) = ui.panels.ask_size();
            let origin = (
                screen.x as f32 + (screen.w as f32 - w as f32 * layout.scale) / 2.0,
                screen.y as f32 + (screen.h as f32 - h as f32 * layout.scale) / 2.0,
            );
            ui.panels.draw_ask(frame, origin, layout.scale, &ask);
        }
        if self.briefing && !self.scene.briefing.is_empty() {
            self.draw_briefing(frame, &screen, text_scale);
        }
        if self.paused {
            self.draw_pause(frame, &screen, ui_scale);
        }
        {
            ui.draw_resources(
                frame,
                layout,
                self.scene.shown[0],
                self.scene.shown[1],
                self.time_ms,
            );
            ui.draw_wind(frame, layout, &self.scene.wind);
            if let Some(info) = &self.object_info {
                let (x, y) = crate::object_info::ORIGIN;
                let s = layout.scale;
                let x = (layout.field_x + x) as f32 * s;
                ui.info_art
                    .draw(frame, (x, y as f32 * s), s, info, layout.bar_hidden);
            }
            // The button under the cursor says what it does and costs, and
            // its key (0x505e70).
            let hovered = self.hover_button.and_then(|slot| {
                let obj = self
                    .selected
                    .and_then(|id| self.scene.object_index(id))
                    .map(|i| &self.scene.objects[i])?;
                menu(self.scene, obj, self.page)[slot]
                    .map(|e| e.command.describe(Some(self.scene)) + &self.hotkeys.suffix(&e))
            });
            // Blank with nothing to say, as on the original (the objectives
            // are in the help panel).
            let named = || {
                let i = self.scene.object_index(self.hover_object?)?;
                let o = &self.scene.objects[i];
                self.scene
                    .is_visible(o)
                    .then(|| crate::objects::info_name(self.data, o))
            };
            let msg = match (self.status, hovered) {
                (Some(s), _) => s.to_string(),
                (None, Some(h)) => h,
                (None, None) => named().unwrap_or_default(),
            };
            ui.draw_message(frame, layout, self.text, &msg);
        }
    }
}

/// Whether the load panel shows for `selected` (see `load_panel`).
fn reloading_shown(
    load: Option<(load_panel::LoadView, Option<load_panel::Hit>)>,
    scene: &Scene,
    selected: u32,
) -> bool {
    load.is_some_and(|(view, _)| load_panel::lists(scene, selected, &view).is_some())
}

/// Splits `text` into lines no wider than `width` font pixels.
pub fn wrap(font: &TextRenderer, text: &str, width: u32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(line) if font.measure(&format!("{line} {word}")) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// A cell of the object panel's slot grid (c_load.cpp 0x4f9520): a
/// `COBJ_SSLOT` picture and, for crew, weapons and cargo, a health or fill
/// bar in percent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotCell {
    pub frame: u8,
    pub bar: Option<u8>,
    /// A count shown in the corner (`NUMBER_FONT`), if not 0.
    pub count: u32,
}

/// The shading of work not yet done (0x5c1658): palette index to index.
#[rustfmt::skip]
const SHADE: [u8; 256] = [
    0, 29, 144, 27, 27, 27, 27, 13, 14, 142, 142, 141, 141, 141, 140, 140,
    205, 142, 142, 141, 141, 141, 141, 141, 140, 140, 140, 141, 142, 14, 13, 27,
    27, 28, 144, 144, 29, 30, 30, 141, 142, 141, 140, 140, 28, 27, 27, 13,
    142, 142, 141, 141, 141, 140, 140, 140, 205, 205, 14, 142, 142, 142, 141, 141,
    141, 142, 141, 141, 141, 141, 141, 140, 13, 13, 14, 140, 140, 205, 141, 141,
    141, 140, 14, 142, 141, 141, 140, 140, 205, 205, 141, 141, 140, 140, 140, 140,
    140, 140, 140, 140, 205, 140, 141, 142, 143, 13, 13, 13, 27, 27, 27, 28,
    144, 144, 144, 29, 144, 28, 27, 28, 27, 28, 27, 27, 27, 27, 27, 27,
    14, 14, 14, 142, 141, 141, 140, 140, 205, 205, 205, 205, 205, 205, 140, 142,
    142, 142, 14, 14, 14, 142, 142, 142, 142, 141, 142, 14, 14, 13, 13, 14,
    142, 141, 140, 205, 205, 141, 142, 13, 13, 27, 27, 27, 144, 144, 144, 29,
    30, 13, 142, 14, 14, 13, 14, 142, 141, 141, 141, 141, 141, 142, 142, 14,
    142, 27, 142, 141, 140, 205, 140, 140, 141, 140, 14, 28, 144, 205, 205, 205,
    140, 141, 140, 140, 141, 140, 141, 141, 142, 142, 142, 142, 142, 142, 142, 13,
    13, 13, 27, 27, 27, 27, 27, 27, 27, 28, 144, 235, 236, 237, 238, 239,
    240, 241, 242, 243, 244, 245, 27, 247, 248, 27, 13, 27, 27, 144, 27, 255,
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::Caches;

    /// The control bar at either side ([0x5ca54a]): on the right the field
    /// at 0 and the bar at 592 (0x512ab0: 0x250); on the left the bar at 0
    /// and the field, the view (0x511170(0x67, 8)), the wind rose, the
    /// money panel and the message line 208 further right (0x504ef0,
    /// 0x504620 message 5). Needs `OPENCONQUEST_DATA`.
    #[test]
    fn the_bar_goes_to_either_side() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let ui = Interface::load(&data, '0').unwrap();
        let policy = ScalePolicy {
            fixed: None,
            integer: false,
        };
        let right = ui.layout((800, 600), &policy, &scene, false, false);
        let left = ui.layout((800, 600), &policy, &scene, true, false);
        let view = |l: &Layout| (l.view.x, l.view.y, l.view.w, l.view.h);
        assert_eq!(view(&right), (8, 8, 576, 512));
        assert_eq!(view(&left), (216, 8, 576, 512));
        assert_eq!((right.bar_x, left.bar_x), (592.0, 0.0));
        assert_eq!((right.field_x, left.field_x), (0, 208));
        assert_eq!(right.minimap.x, left.minimap.x + 592);
        assert_eq!(right.minimap.y, left.minimap.y);
        // Drawn: the bar's pixels and the field's the same, moved.
        let draw = |l: &Layout| {
            let mut pixels = vec![0u32; 800 * 600];
            let mut frame = Frame {
                pixels: &mut pixels,
                width: 800,
                height: 600,
            };
            ui.draw(&mut frame, l, PanelBack::Clear);
            ui.draw_wind(&mut frame, l, &scene.wind);
            ui.draw_resources(&mut frame, l, 1234, 56, 0);
            pixels
        };
        let (r, l) = (draw(&right), draw(&left));
        for y in 0..600 {
            let row = y * 800;
            assert_eq!(r[row + 592..row + 800], l[row..row + 208], "bar row {y}");
            assert_eq!(r[row..row + 592], l[row + 208..row + 800], "field row {y}");
        }
    }

    /// The bar hidden (Space, 0x50f910 message 0x4e1): the view 12 x 8
    /// cells at (16, 8) and the field at 0 whatever the bar's side, the bar
    /// off the screen (0x512ab0: 800 + x), the field drawn as
    /// `GAME_FIELD_FS`, the money panel at (694, 527). Needs
    /// `OPENCONQUEST_DATA`.
    #[test]
    fn the_bar_hidden() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let ui = Interface::load(&data, '0').unwrap();
        let policy = ScalePolicy {
            fixed: None,
            integer: false,
        };
        let right = ui.layout((800, 600), &policy, &scene, false, true);
        let left = ui.layout((800, 600), &policy, &scene, true, true);
        for l in [&right, &left] {
            assert_eq!((l.view.x, l.view.y, l.view.w, l.view.h), (16, 8, 768, 512));
            assert_eq!((l.field_x, l.field_w, l.bar_x), (0, 800, 800.0));
            assert_eq!(ui.money_panel_origin(l), (694.0, 527.0));
        }
        // `GAME_FIELD_FS`'s see-through part holds the view.
        let (x, y, w, h) = black_rect(&ui.field_fs, 400, 300);
        println!("GAME_FIELD_FS hole {:?}", (x, y, w, h));
        assert!(x <= 16 && y <= 8 && x + w >= 16 + 768 && y + h >= 8 + 512);
        // The two sides draw alike.
        let draw = |l: &Layout| {
            let mut pixels = vec![0u32; 800 * 600];
            let mut frame = Frame {
                pixels: &mut pixels,
                width: 800,
                height: 600,
            };
            ui.draw(&mut frame, l, PanelBack::Clear);
            pixels
        };
        assert!(draw(&right) == draw(&left));
    }
}
