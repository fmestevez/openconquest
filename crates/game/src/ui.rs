//! The original in-game interface: the framed game view, the control bar
//! on the right with the minimap and the object panel.
//!
//! The original screen is 800x600: `GAME_FIELD` (592x600, the frame around
//! the view and the message strip) plus `GAME_CTRLBAR` (208x600). Panels on
//! the bar, from ANCIENT.EXE: the minimap frame `BKGND_LMAPA` at bar + (4, 4)
//! (0x4fffe0) and the object panel `COBJ_BKMAIN` at bar + (3, 206)
//! (0x4f5800). Names are prefixed with a theme digit, `0` (Greek) or `3`
//! (Persian).
//!
//! Widescreen: everything is scaled by the window height, the control bar
//! stays on the right and the frame is widened by tiling its middle part.

use anyhow::{Context, Result};
use openac_formats::{Bitmap, ResourceKind, Sequence};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::assets::{rgb, GameData, Image};
use crate::commands::{menu, Menu, Page, SLOTS};
use crate::render::{
    blit_scaled, draw_minimap, draw_objects, draw_selection, draw_site, draw_world, palette_lut,
    shade_rect, Frame, MinimapRect,
};
use crate::scene::Scene;
use crate::text::TextRenderer;
use crate::view::{ScalePolicy, Screen};

const ORIGINAL_FIELD_W: u32 = 592;
const BAR_W: u32 = 208;
const UI_H: u32 = 600;
const MINIMAP_POS: (u32, u32) = (4, 4);
const OBJECT_PANEL_POS: (u32, u32) = (3, 206);
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
    bar: Image,
    minimap_frame: Image,
    object_panel: Image,
    /// The see-through part of `field` where the world is shown.
    field_view: Rect,
    minimap_area: Rect,
    picture_slot: Rect,
    pictures: RefCell<HashMap<String, Option<Rc<Image>>>>,
    money_panel: Image,
    message_line: Image,
    /// Number slots of the money panel (amber, fish).
    money_slots: [Rect; 2],
    message_area: Rect,
    money_icon: Vec<Rc<Image>>,
    wind_rose: Image,
    wind_arrows: Vec<Option<Image>>,
    fish_icon: Vec<Rc<Image>>,
    digits: TextRenderer,
    /// Button animations by base name.
    buttons: RefCell<HashMap<String, Rc<Vec<Rc<Image>>>>>,
    button_frame: Option<Rc<Image>>,
}

/// Window-space placement of the interface for one window size.
pub struct Layout {
    pub scale: f32,
    pub view: Screen,
    pub minimap: MinimapRect,
    field_w: u32,
    bar_x: f32,
    /// Top-left of the object panel's text area, in window pixels.
    pub info_text: (f32, f32),
    pub info_width: f32,
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
        let money_slots = [
            black_rect(&money_panel, 75, 18),
            black_rect(&money_panel, 75, 48),
        ];
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
            bar: load("GAME_CTRLBAR", false)?,
            minimap_frame,
            object_panel,
            field_view,
            minimap_area,
            picture_slot,
            pictures: RefCell::default(),
            money_panel,
            message_line,
            money_slots,
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
            digits: TextRenderer::load(&data.controls, "MONEY_FONT", 0)?,
            buttons: RefCell::default(),
            button_frame: data
                .load_image(&data.controls, "CBUT_BK", false)
                .ok()
                .flatten()
                .map(Rc::new),
        })
    }

    pub fn layout(&self, window: (u32, u32), policy: &ScalePolicy, scene: &Scene) -> Layout {
        let scale = policy.base_scale(window.1);
        let ui_w = (window.0 as f32 / scale) as u32;
        let field_w = ui_w.saturating_sub(BAR_W).max(2 * FRAME_EDGE);
        let bar_x = field_w as f32 * scale;
        let to_px = |v: u32| (v as f32 * scale).round() as u32;
        let (vx, vy, vw, vh) = self.field_view;
        let view = Screen {
            x: to_px(vx),
            y: to_px(vy),
            w: to_px((vw as i32 + field_w as i32 - ORIGINAL_FIELD_W as i32).max(16) as u32),
            h: to_px(vh),
            window_h: window.1,
        };
        let (mx, my, mw, mh) = self.minimap_area;
        let minimap = MinimapRect::fit(
            scene,
            (bar_x + ((MINIMAP_POS.0 + mx) as f32 * scale)) as u32,
            to_px(MINIMAP_POS.1 + my),
            to_px(mw),
            to_px(mh),
        );
        let (_, py, _, ph) = self.picture_slot;
        Layout {
            scale,
            view,
            minimap,
            field_w,
            bar_x,
            info_text: (
                bar_x + (OBJECT_PANEL_POS.0 + 8) as f32 * scale,
                (OBJECT_PANEL_POS.1 + py + ph + 6) as f32 * scale,
            ),
            info_width: (self.object_panel.width - 16) as f32 * scale,
        }
    }

    /// Draws the interface over the (already rendered) world view.
    pub fn draw(&self, frame: &mut Frame, layout: &Layout) {
        let s = layout.scale;
        let f = &self.field;
        // Frame: left and right edges as they are, the middle tiled.
        blit_scaled(frame, f, (0, 0, FRAME_EDGE, UI_H), 0.0, 0.0, s);
        let mid = (FRAME_EDGE, 0, ORIGINAL_FIELD_W - 2 * FRAME_EDGE, UI_H);
        let right_at = layout.field_w - FRAME_EDGE;
        let mut x = FRAME_EDGE;
        while x < right_at {
            let w = mid.2.min(right_at - x);
            blit_scaled(frame, f, (mid.0, 0, w, UI_H), x as f32 * s, 0.0, s);
            x += w;
        }
        blit_scaled(
            frame,
            f,
            (ORIGINAL_FIELD_W - FRAME_EDGE, 0, FRAME_EDGE, UI_H),
            right_at as f32 * s,
            0.0,
            s,
        );
        // Control bar and its panels.
        let full = |img: &Image| (0, 0, img.width, img.height);
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
        let (ox, oy) = at(OBJECT_PANEL_POS);
        blit_scaled(
            frame,
            &self.object_panel,
            full(&self.object_panel),
            ox,
            oy,
            s,
        );
        let (mx, my) = self.money_panel_origin(layout);
        blit_scaled(frame, &self.money_panel, full(&self.money_panel), mx, my, s);
        blit_scaled(
            frame,
            &self.wind_rose,
            full(&self.wind_rose),
            WIND_ROSE.0 as f32 * s,
            WIND_ROSE.1 as f32 * s,
            s,
        );
        let (lx, ly) = (MESSAGE_LINE.0 as f32 * s, MESSAGE_LINE.1 as f32 * s);
        blit_scaled(
            frame,
            &self.message_line,
            full(&self.message_line),
            lx,
            ly,
            s,
        );
    }

    fn money_panel_origin(&self, layout: &Layout) -> (f32, f32) {
        let s = layout.scale;
        (
            (layout.field_w.saturating_sub(MONEY_PANEL.0)) as f32 * s,
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
            WIND_ROSE.0 as f32 * s,
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
        let text_scale = s.round().max(1.0) as u32;
        for (value, (x, y, w, h)) in [(amber, self.money_slots[0]), (fish, self.money_slots[1])] {
            let text = value.to_string();
            let tw = self.digits.measure(&text) * text_scale;
            let th = self.digits.line_height() * text_scale;
            let right = px + (x + w) as f32 * s - 3.0 * s;
            let top = py + y as f32 * s + ((h as f32 * s) - th as f32) / 2.0;
            self.digits.draw(
                frame,
                (right - tw as f32) as i32,
                top as i32,
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
        let left = MESSAGE_LINE.0 as f32 * s + (x as f32 + 3.0) * s;
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

    /// The command slot under window position `(x, y)`.
    pub fn button_at(&self, layout: &Layout, x: f32, y: f32) -> Option<usize> {
        (0..SLOTS).find(|&i| {
            let (bx, by, size) = self.button_rect(layout, i);
            x >= bx && y >= by && x < bx + size && y < by + size
        })
    }

    /// Draws a command menu. The hovered button plays its animation;
    /// disabled buttons show their greyed last frame.
    /// Draws a command menu. The hovered button plays its animation;
    /// disabled buttons show their greyed last frame; a running order (such
    /// as a ship being built) shows its progress through the frames.
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
            let img = if let (Some(p), true) = (slot.progress, animated) {
                &frames[((p * (frames.len() - 1) as f32) as usize).min(frames.len() - 2)]
            } else if !slot.enabled {
                last
            } else if hover == Some(i) && animated {
                &frames[(time_ms / BUTTON_FRAME_MS) as usize % (frames.len() - 1)]
            } else {
                &frames[0]
            };
            let (x, y, _) = self.button_rect(layout, i);
            blit_scaled(
                frame,
                img,
                (0, 0, img.width, img.height),
                x,
                y,
                layout.scale,
            );
            if hover == Some(i) && slot.enabled {
                if let Some(bk) = &self.button_frame {
                    blit_scaled(frame, bk, (0, 0, bk.width, bk.height), x, y, layout.scale);
                }
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
    /// `None` hides the interface (full-window view with an overlay minimap).
    pub interface: Option<&'a Interface>,
    pub text: &'a TextRenderer,
    pub selected: Option<u32>,
    pub status: Option<&'a str>,
    pub time_ms: u64,
    /// Command slot under the cursor.
    pub hover_button: Option<usize>,
    /// Which menu of the selected object is shown.
    pub page: Page,
    /// A building site being chosen: the cell and whether it is allowed.
    pub site: Option<((i32, i32), bool)>,
    /// Show the mission briefing over the view.
    pub briefing: bool,
}

impl GameView<'_> {
    /// The mission name and its objectives, ticked off as they are met.
    fn headline(&self) -> String {
        let goals: Vec<String> = (0..crate::scene::OBJECTIVES.len())
            .filter(|&i| self.scene.conditions[i])
            .map(|i| {
                let name = crate::scene::OBJECTIVES[i].0;
                if self.scene.objective_met(i) {
                    format!("{name} (done)")
                } else {
                    name.to_string()
                }
            })
            .collect();
        let name = self.scene.mission.to_uppercase();
        if goals.is_empty() {
            name
        } else {
            format!("{name}: {}", goals.join(", "))
        }
    }

    /// The mission briefing in a box over the view; the first paragraph
    /// is the title.
    fn draw_briefing(&self, frame: &mut Frame, screen: &Screen, scale: u32) {
        let line = self.text.line_height() * scale;
        let pad = 12 * scale;
        let width = (screen.w * 3 / 5).max(200 * scale);
        let mut lines: Vec<String> = Vec::new();
        for (i, p) in self.scene.briefing.iter().enumerate() {
            if i > 0 {
                lines.push(String::new());
            }
            lines.extend(wrap(self.text, p, width / scale));
        }
        lines.push(String::new());
        lines.push("Click or press Enter to begin (F1 shows this again)".to_string());
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
    pub fn placement(&self, window: (u32, u32)) -> (Screen, MinimapRect, Option<Layout>) {
        match self.interface {
            Some(ui) => {
                let layout = ui.layout(window, &self.camera.policy, self.scene);
                (layout.view, layout.minimap, Some(layout))
            }
            None => (
                Screen::full(window.0, window.1),
                MinimapRect::overlay(self.scene, window),
                None,
            ),
        }
    }

    pub fn draw(&self, frame: &mut Frame) {
        let window = (frame.width, frame.height);
        let (screen, minimap, layout) = self.placement(window);
        let lut = palette_lut(self.data, self.time_ms);
        draw_world(frame, &screen, self.scene, self.camera, &lut);
        draw_objects(frame, &screen, self.scene, self.camera, &lut, self.time_ms);
        if let Some(id) = self.selected {
            draw_selection(frame, &screen, self.scene, self.camera, id, self.time_ms);
        }
        if let Some((cell, ok)) = self.site {
            draw_site(frame, &screen, self.camera, cell, ok);
        }
        let selected = self
            .selected
            .and_then(|id| self.scene.object_index(id))
            .map(|i| &self.scene.objects[i]);
        let ui_scale = self.camera.policy.base_scale(window.1);
        let text_scale = ui_scale.round().max(1.0) as u32;
        if let (Some(ui), Some(layout)) = (self.interface, &layout) {
            ui.draw(frame, layout);
            if let Some(obj) = selected {
                if let Some((name, (px, py))) = &obj.picture {
                    if let Some(img) = ui.picture(self.data, name) {
                        let (sx, sy, _, _) = ui.picture_slot(layout);
                        let (ox, oy) = ui.picture_slot_offset();
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
                let nation = self.scene.human.map_or(1, |h| h.nation);
                ui.draw_menu(
                    frame,
                    self.data,
                    layout,
                    &menu(self.scene, obj, self.page),
                    nation,
                    self.hover_button,
                    self.time_ms,
                );
                let (tx, ty) = layout.info_text;
                let lines = wrap(
                    self.text,
                    &details(self.scene, obj),
                    (layout.info_width / text_scale as f32) as u32,
                );
                for (i, line) in lines.iter().enumerate() {
                    let y = ty + (i as u32 * (self.text.line_height() + 1) * text_scale) as f32;
                    self.text.draw(frame, tx as i32, y as i32, text_scale, line);
                }
            }
        }
        draw_minimap(frame, &minimap, self.scene, self.camera, &screen);
        if let Some(outcome) = self.scene.outcome {
            self.draw_outcome(frame, &screen, outcome, text_scale);
        }
        if self.briefing && !self.scene.briefing.is_empty() {
            self.draw_briefing(frame, &screen, text_scale);
        }
        if let (Some(ui), Some(layout)) = (self.interface, &layout) {
            ui.draw_resources(
                frame,
                layout,
                self.scene.amber,
                self.scene.fish,
                self.time_ms,
            );
            ui.draw_wind(frame, layout, &self.scene.wind);
            let msg = self.status.map_or_else(|| self.headline(), str::to_string);
            ui.draw_message(frame, layout, self.text, &msg);
            return;
        }
        // Without the interface: mission name, selection and status messages
        // in the top-left corner of the view.
        let mut lines = vec![self.headline()];
        if let Some(obj) = selected {
            lines.push(obj.label.clone());
        }
        if let Some(msg) = self.status {
            lines.push(msg.to_string());
        }
        let line = (self.text.line_height() + 2) * text_scale;
        let pad = 4 * text_scale;
        let width = lines
            .iter()
            .map(|l| self.text.measure(l))
            .max()
            .unwrap_or(0)
            * text_scale;
        let (x0, y0) = (screen.x + pad, screen.y + pad);
        shade_rect(
            frame,
            x0,
            y0,
            width + 2 * pad,
            lines.len() as u32 * line + 2 * pad,
        );
        for (i, l) in lines.iter().enumerate() {
            self.text.draw(
                frame,
                (x0 + pad) as i32,
                (y0 + pad + i as u32 * line) as i32,
                text_scale,
                l,
            );
        }
    }
}

/// Splits `text` into lines no wider than `width` font pixels.
fn wrap(font: &TextRenderer, text: &str, width: u32) -> Vec<String> {
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

/// The object panel text: name, owner and what the object is doing.
fn details(scene: &Scene, obj: &crate::objects::MapObject) -> String {
    use crate::objects::{GatherPhase, Resource, Task};
    let mut text = obj.label.clone();
    let goods = |r: Resource| {
        if r == Resource::Amber {
            "amber"
        } else {
            "fish"
        }
    };
    if let Some(d) = obj.deposit {
        match d.remaining {
            Some(n) => text += &format!(". {} {} left", n as u32, goods(d.resource)),
            None => text += ". Inexhaustible",
        }
    }
    if let Some(c) = &obj.combat {
        text += &format!(
            ". Hull {}/{}",
            c.hull.max(0.0).ceil() as u32,
            c.max_hull as u32
        );
    }
    if let Some(ship) = &obj.ship {
        if ship.crew.count() > 0 {
            text += &format!(". Marines: {}", ship.crew.describe());
        }
        if let Some(c) = obj.combat.as_ref().filter(|c| !c.weapons.is_empty()) {
            let names: Vec<&str> = c
                .weapons
                .iter()
                .map(|&w| crate::combat::ITEM_NAMES[w as usize % 9])
                .collect();
            text += &format!(". Weapons: {}", names.join(", "));
        }
        if let (Some(kind), true) = (ship.cargo_kind, ship.cargo >= 1.0) {
            text += &format!(". Carrying {} {}", ship.cargo as u32, goods(kind));
        }
        match ship.task {
            Some(Task::Gather {
                phase: GatherPhase::Collecting,
                ..
            }) => text += ". Gathering",
            Some(Task::Gather {
                phase: GatherPhase::ToPort(_),
                ..
            }) => text += ". Returning to port",
            Some(Task::Gather {
                phase: GatherPhase::ToDeposit,
                ..
            }) => text += ". Sailing to gather",
            Some(Task::Attack { .. }) => text += ". Attacking",
            Some(Task::Ram { .. }) => text += ". Ramming",
            Some(Task::Found { .. }) => text += ". Sailing to found a settlement",
            Some(Task::Board { .. }) => text += ". Boarding",
            None => {}
        }
    }
    let garrison = scene.garrison(obj.id);
    if garrison.iter().any(|&n| n > 0) {
        let parts: Vec<String> = garrison
            .iter()
            .zip(crate::combat::ITEM_NAMES)
            .filter(|(n, _)| **n > 0)
            .map(|(n, name)| format!("{n} {name}"))
            .collect();
        text += &format!(". Waiting: {}", parts.join(", "));
    }
    if let Some(t) = scene.training_at(obj.id) {
        text += &format!(
            ". Training {} {}",
            t.queued,
            crate::combat::ITEM_NAMES[t.kind as usize % 9]
        );
    }
    if let Some(c) = scene.construction_at(obj.id) {
        text += &format!(
            ". Under construction: {}%",
            (c.elapsed / c.total * 100.0) as u32
        );
    }
    if let Some(p) = scene.production_at(obj.id) {
        text += &format!(
            ". Building a {}: {}%",
            crate::objects::SHIP_NAMES[p.ship as usize],
            (p.elapsed / p.total * 100.0) as u32
        );
    }
    text
}
