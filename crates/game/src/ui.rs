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
use openconquest_formats::{Bitmap, ResourceKind, Sequence};

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::assets::{rgb, GameData, Image};
use crate::commands::{menu, Menu, Page, SLOTS};
use crate::panels::{PanelView, Panels};
use crate::render::{
    blit_scaled, draw_fog, draw_minimap, draw_objects, draw_selection, draw_site, draw_world,
    palette_lut, shade_rect, Frame, MinimapRect,
};
use crate::scene::Scene;
use crate::text::TextRenderer;
use crate::view::{ScalePolicy, Screen};

const ORIGINAL_FIELD_W: u32 = 592;
const BAR_W: u32 = 208;
const UI_H: u32 = 600;
const MINIMAP_POS: (u32, u32) = (4, 4);
const OBJECT_PANEL_POS: (u32, u32) = (3, 206);
/// The strip above the command buttons (0x4f06f0: y 0x16e).
const STRIP_Y: u32 = 366;
/// The object panel's slot grid (measured on the original).
const SLOTS_ORIGIN: (u32, u32) = (9, 210);
const SLOT_STEP: (u32, u32) = (48, 51);
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
    /// The green tick where a ship was sent (`TAKE_CMD`, controlm).
    take_cmd: Vec<Image>,
}

/// Window-space placement of the interface for one window size.
pub struct Layout {
    pub scale: f32,
    pub view: Screen,
    pub minimap: MinimapRect,
    field_w: u32,
    pub bar_x: f32,
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
            strips: [1, 2].map(|i| load(&format!("CCTRL_CLRDEF{i}"), false).ok()),
            small_slot: load("COBJ_BKSMSLOT", false).ok(),
            number_font: TextRenderer::load_in(
                &data.controls,
                "NUMBER_FONT",
                0,
                Some(&data.palette),
            )?
            .with_spacing(2),
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
            take_cmd: (0..4)
                .filter_map(|i| {
                    let menus = data.menus.as_ref()?;
                    data.load_image(menus, &format!("TAKE_CMD{i:02}"), false)
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
        let minimap = MinimapRect::window(
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
        self.draw_strip(frame, layout, true);
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
    /// The slot cell (its index in the panel's list) under window position
    /// `(x, y)`.
    pub fn slot_cell_at(&self, layout: &Layout, x: f32, y: f32) -> Option<usize> {
        let s = layout.scale;
        let (fx, fy) = ((x - layout.bar_x) / s, y / s);
        let (ox, oy) = (fx - SLOTS_ORIGIN.0 as f32, fy - SLOTS_ORIGIN.1 as f32);
        if ox < 0.0 || oy < 0.0 {
            return None;
        }
        let (col, row) = (
            (ox / SLOT_STEP.0 as f32) as u32,
            (oy / SLOT_STEP.1 as f32) as u32,
        );
        if col >= 4 || row >= 3 || ox % SLOT_STEP.0 as f32 >= 47.0 {
            return None;
        }
        (row * 4 + col).checked_sub(1).map(|i| i as usize)
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
            let img = if let (Some(p), true) = (slot.progress, animated) {
                &frames[((p * (frames.len() - 1) as f32) as usize).min(frames.len() - 2)]
            } else if !slot.enabled {
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

    /// A ship's slot grid (c_load.cpp): four cells to a row, 48 apart from
    /// bar (9, 210), rows 51 apart; the first cell is the portrait's.
    fn draw_slots(
        &self,
        frame: &mut Frame,
        data: &GameData,
        layout: &Layout,
        cells: &[Option<SlotCell>],
        chosen: Option<usize>,
    ) {
        let s = layout.scale;
        for (i, cell) in cells.iter().enumerate() {
            let Some(cell) = cell else { continue };
            let n = i as u32 + 1;
            let (x, y) = (
                layout.bar_x + (SLOTS_ORIGIN.0 + n % 4 * SLOT_STEP.0) as f32 * s,
                (SLOTS_ORIGIN.1 + n / 4 * SLOT_STEP.1) as f32 * s,
            );
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
            // The chosen cell's frame (`COBJ_SACT`).
            if chosen == Some(i) {
                if let Ok(Some(img)) = data.load_image(&data.controls, "COBJ_SACT", false) {
                    blit_scaled(frame, &img, (0, 0, img.width, img.height), x + s, y + s, s);
                }
            }
            if let Some(mark) = cell.mark {
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
    /// Other ships selected with it.
    pub group: &'a [u32],
    /// A selection box being dragged (window pixels).
    pub select_box: Option<(f32, f32, f32, f32)>,
    pub status: Option<&'a str>,
    pub time_ms: u64,
    /// Command slot under the cursor.
    pub hover_button: Option<usize>,
    /// The panel's slot cell chosen (its index), shown framed.
    pub chosen_cell: Option<usize>,
    /// Where the player last sent ships (world point) and how many
    /// milliseconds ago.
    pub sent: Option<(f32, f32, u64)>,
    /// Which menu of the selected object is shown.
    pub page: Page,
    /// A building site being chosen: the cell and whether it is allowed.
    pub site: Option<((i32, i32), bool)>,
    /// Show the mission briefing over the view.
    pub briefing: bool,
    /// The bar's tabs and open panel (with the interface shown).
    pub panel: Option<PanelView<'a>>,
    /// The bar-relative point under the cursor, in original pixels.
    pub panel_hover: Option<(u32, u32)>,
    /// The game is paused.
    pub paused: bool,
}

impl GameView<'_> {
    /// The mission name and its objectives, ticked off as they are met.
    fn headline(&self) -> String {
        let done = |name: String, met: bool| if met { format!("{name} (done)") } else { name };
        if !self.scene.story.is_empty() {
            let goals: Vec<String> = self
                .scene
                .story
                .iter()
                .map(|&g| done(crate::scene::goal_name(g), self.scene.goal_met(g)))
                .collect();
            return format!(
                "{}: {}",
                self.scene.mission.to_uppercase(),
                goals.join(", ")
            );
        }
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
        let paragraphs = self.scene.briefing.iter().filter(|p| !p.is_empty());
        for (i, p) in paragraphs.enumerate() {
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

    /// The pause sign in the middle of the view (text without the
    /// interface's art).
    fn draw_pause(&self, frame: &mut Frame, screen: &Screen, scale: f32) {
        let sign = self
            .interface
            .map(|ui| &ui.pause_sign)
            .filter(|s| !s.is_empty());
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
        draw_fog(frame, &screen, self.scene, self.camera, &lut);
        for &id in self.selected.iter().chain(self.group) {
            draw_selection(
                frame,
                &screen,
                self.scene,
                self.camera,
                id,
                self.time_ms,
                &lut,
            );
        }
        // The green tick where ships were sent (gcursor.cpp 0x515e60): its
        // top-left 11 and 24 pixels up-left of the click, a frame each 250
        // ms, gone as it reaches its last (0x5166f8).
        if let (Some((x, y, age)), Some(ui)) = (self.sent, self.interface) {
            const TICK_MS: u64 = 250;
            let n = ui.take_cmd.len() as u64;
            if n > 1 && age < (n - 1) * TICK_MS {
                let img = &ui.take_cmd[(age / TICK_MS) as usize];
                let (cx, cy) = (
                    x - 11.0 + img.width as f32 / 2.0,
                    y - 24.0 + img.height as f32 / 2.0,
                );
                crate::render::draw_centred(frame, &screen, self.camera, &lut, img, cx, cy);
            }
        }
        if let Some(b) = self.select_box {
            crate::render::draw_box(frame, &screen, b);
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
        let panel_open = self
            .panel
            .as_ref()
            .is_some_and(|p| Panels::covers_bar(p.tab));
        if let (Some(ui), Some(layout)) = (self.interface, &layout) {
            ui.draw(frame, layout);
            if let (Some(obj), false) = (selected, panel_open) {
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
                if self.page != Page::Main {
                    ui.draw_strip(frame, layout, false);
                }
                if matches!(self.page, Page::Structures | Page::Ground) && self.scene.is_own(obj) {
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
                    self.hover_button,
                    self.time_ms,
                );
                let mut slots = ship_slots(obj);
                if slots.is_empty() && self.scene.is_own(obj) {
                    slots = settlement_slots(self.scene, obj);
                }
                if !slots.is_empty() {
                    ui.draw_slots(frame, self.data, layout, &slots, self.chosen_cell);
                }
                // Ships show their slots instead (no text, as on the
                // original).
                let (tx, ty) = layout.info_text;
                let lines = if !slots.is_empty() {
                    Vec::new()
                } else {
                    wrap(
                        self.text,
                        &match self.group.len() {
                            0 => details(self.scene, obj),
                            n => format!(
                                "{}. With {n} more ship{}",
                                details(self.scene, obj),
                                if n == 1 { "" } else { "s" }
                            ),
                        },
                        (layout.info_width / text_scale as f32) as u32,
                    )
                };
                for (i, line) in lines.iter().enumerate() {
                    let y = ty + (i as u32 * (self.text.line_height() + 1) * text_scale) as f32;
                    self.text.draw(frame, tx as i32, y as i32, text_scale, line);
                }
            }
        }
        if self.interface.is_none() || !panel_open {
            draw_minimap(frame, &minimap, self.scene, self.camera, &screen, &lut);
        }
        if let (Some(ui), Some(layout), Some(view)) = (self.interface, &layout, &self.panel) {
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
        if let Some(outcome) = self.scene.outcome {
            self.draw_outcome(frame, &screen, outcome, text_scale);
        }
        if self.briefing && !self.scene.briefing.is_empty() {
            self.draw_briefing(frame, &screen, text_scale);
        }
        if self.paused {
            self.draw_pause(frame, &screen, ui_scale);
        }
        if let (Some(ui), Some(layout)) = (self.interface, &layout) {
            ui.draw_resources(
                frame,
                layout,
                self.scene.shown[0],
                self.scene.shown[1],
                self.time_ms,
            );
            ui.draw_wind(frame, layout, &self.scene.wind);
            // The button under the cursor says what it does and costs.
            let hovered = self.hover_button.and_then(|slot| {
                let obj = self
                    .selected
                    .and_then(|id| self.scene.object_index(id))
                    .map(|i| &self.scene.objects[i])?;
                menu(self.scene, obj, self.page)[slot].map(|e| e.command.describe(Some(self.scene)))
            });
            // Blank with nothing to say, as on the original (the objectives
            // are in the help panel).
            let msg = match (self.status, hovered) {
                (Some(s), _) => s.to_string(),
                (None, Some(h)) => h,
                (None, None) => String::new(),
            };
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

/// The object panel text: name, owner and what the object is doing.
fn details(scene: &Scene, obj: &crate::objects::MapObject) -> String {
    use crate::objects::{GatherPhase, Resource, Task};
    let mut text = obj.label.clone();
    let goods = |r: Resource| match r {
        Resource::Amber => "amber",
        Resource::Fish => "fish",
        Resource::Leviathan => "leviathan",
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
        if ship.armour > 0 || ship.ram > 0 {
            text += &format!(". Armour {}, ram {}", ship.armour, ship.ram);
        }
        if ship.crew.count() > 0 {
            text += &format!(". Marines: {}", ship.crew.describe());
        }
        if let Some(c) = obj.combat.as_ref().filter(|c| !c.weapons.is_empty()) {
            let names: Vec<&str> = c
                .weapons
                .iter()
                .map(|&w| crate::combat::item_name(w))
                .collect();
            text += &format!(". Weapons: {}", names.join(", "));
        }
        if obj.combat.as_ref().is_some_and(|c| c.stone > 0.0) {
            text += ". Turned to stone";
        }
        if !ship.heroes.is_empty() {
            let names: Vec<&str> = ship
                .heroes
                .iter()
                .map(|&h| crate::objects::HERO_NAMES[h as usize % 9])
                .collect();
            text += &format!(". Heroes: {}", names.join(", "));
        }
        if !ship.treasures.is_empty() {
            let names: Vec<String> = ship
                .treasures
                .iter()
                .map(|t| crate::objects::treasure_name(t.kind))
                .collect();
            text += &format!(". Treasures: {}", names.join(", "));
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
                phase: GatherPhase::Unloading(_),
                ..
            }) => text += ". Unloading",
            Some(Task::Gather {
                phase: GatherPhase::ToDeposit,
                ..
            }) => text += ". Sailing to gather",
            Some(Task::Attack { .. }) => text += ". Attacking",
            Some(Task::Ram { .. }) => text += ". Ramming",
            Some(Task::Found { class, .. }) => {
                text += &format!(
                    ". Sailing to build a {}",
                    crate::objects::building_label(class).to_lowercase()
                )
            }
            Some(Task::Board { .. }) => text += ". Boarding",
            Some(Task::Repair { .. }) => text += ". Repairing",
            Some(Task::Scout { .. }) => text += ". Scouting",
            Some(Task::Patrol { .. }) => text += ". Patrolling",
            Some(Task::Zone { attack: false, .. }) => text += ". Guarding a zone",
            Some(Task::Zone { attack: true, .. }) => text += ". Attacking a zone",
            Some(Task::Convoy { .. }) => text += ". Escorting",
            Some(Task::Take { .. }) => text += ". Sailing to take a treasure",
            None => {}
        }
    }
    if let Some(heroes) = scene.site_heroes.get(&obj.id).filter(|h| !h.is_empty()) {
        let names: Vec<&str> = heroes
            .iter()
            .map(|&h| crate::objects::HERO_NAMES[h as usize % 9])
            .collect();
        text += &format!(". Heroes waiting: {}", names.join(", "));
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

/// A cell of the object panel's slot grid (c_load.cpp 0x4f9520): a
/// `COBJ_SSLOT` picture and, for crew, weapons and cargo, a health or fill
/// bar in percent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotCell {
    pub frame: u8,
    pub bar: Option<u8>,
    /// A `COBJ_SSPEC` mark (0 green arrow, 1 dark arrow: units to open).
    pub mark: Option<u8>,
    /// A count shown in the corner (`NUMBER_FONT`), if not 0.
    pub count: u32,
}

/// Slot counts by ship type (0x597ae4 weapon and crew places, 0x597af0
/// cargo, 0x597afc treasures, 0x597b08 heroes; 0x597b14 and 0x597b20
/// whether it shows its armour and ram).
const SLOT_ITEMS: [usize; 12] = [0, 0, 5, 7, 4, 3, 2, 2, 0, 1, 1, 3];
const SLOT_CARGO: [usize; 12] = [4, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const SLOT_TREASURES: [usize; 12] = [1, 1, 2, 1, 1, 1, 1, 1, 0, 0, 0, 0];
const SLOT_HEROES: [usize; 12] = [0, 0, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0];
const SLOT_ARMOUR: [bool; 12] = [
    false, false, true, true, true, true, false, true, false, true, false, false,
];
const SLOT_RAM: [bool; 12] = [
    false, false, true, false, false, true, true, false, false, false, false, false,
];

/// A treasure's picture among the slots (0x5005a0): kinds 6-27 in order,
/// but for kind 17 last.
fn treasure_frame(kind: u8) -> u8 {
    let n = match kind {
        6 | 7 => kind - 6,
        9..=16 => kind - 7,
        17 => 20,
        18..=27 => kind - 8,
        _ => return 0x3a,
    };
    0x3a + n
}

/// A ship's slot cells by grid position (0x4e5e40): its crew and weapons
/// (merchants: their cargo, a hundred units a slot), empty places, armour
/// and ram; then, from the eighth place on, treasures and heroes, with their
/// empty places dimmed.
pub fn ship_slots(obj: &crate::objects::MapObject) -> Vec<Option<SlotCell>> {
    let Some(ship) = &obj.ship else {
        return Vec::new();
    };
    let kind = ship.kind as usize % 12;
    let mut cells: Vec<Option<SlotCell>> = Vec::new();
    let full = |frame: u8| {
        Some(SlotCell {
            frame,
            bar: Some(100),
            mark: None,
            count: 0,
        })
    };
    let bare = |frame: u8| {
        Some(SlotCell {
            frame,
            bar: None,
            mark: None,
            count: 0,
        })
    };
    if kind <= 1 {
        let capacity = SLOT_CARGO[kind];
        if let Some(goods) = ship.cargo_kind.filter(|_| ship.cargo > 0.0) {
            let frame = match goods {
                crate::objects::Resource::Fish => 0x2b,
                crate::objects::Resource::Leviathan => 0x2c,
                crate::objects::Resource::Amber => 0x2a,
            };
            let units = (ship.cargo / 100.0).ceil() as usize;
            for k in 0..units.min(capacity) {
                let left = ship.cargo - k as f32 * 100.0;
                cells.push(Some(SlotCell {
                    frame,
                    bar: Some(left.clamp(0.0, 100.0).round() as u8),
                    mark: None,
                    count: 0,
                }));
            }
        }
        while cells.len() < capacity {
            cells.push(bare(0x29));
        }
    } else {
        let weapons = obj
            .combat
            .as_ref()
            .map(|c| c.weapons.as_slice())
            .unwrap_or(&[]);
        // Soldiers look different for nation 3.
        let nation3 = obj.owner.is_some_and(|o| o.0 == 3);
        let mut big = 0;
        for item in 0..9u8 {
            let count = if item < 3 {
                ship.crew.squads[item as usize] as usize
            } else {
                weapons.iter().filter(|&&w| w == item).count()
            };
            for _ in 0..count {
                let frame = match item {
                    0..=2 if kind == 11 => item + 12 + 0x43,
                    0..=2 => item + if nation3 { 13 } else { 10 },
                    _ => item - 2,
                };
                cells.push(full(frame));
                if (6..9).contains(&item) {
                    big += 1;
                }
            }
        }
        // The unique ships' own weapon.
        match kind {
            8 => cells.push(full(7)),
            9 => cells.push(full(8)),
            10 => cells.push(full(9)),
            _ => {}
        }
        while cells.len() + big < SLOT_ITEMS[kind] {
            cells.push(bare(0));
        }
        if SLOT_ARMOUR[kind] {
            cells.push(bare(0x22 + ship.armour.min(2)));
        }
        if SLOT_RAM[kind] {
            cells.push(bare(0x26 + ship.ram.min(2)));
        }
    }
    if SLOT_TREASURES[kind] + SLOT_HEROES[kind] > 0 {
        while cells.len() < 7 {
            cells.push(None);
        }
        for t in ship.treasures.iter().take(SLOT_TREASURES[kind]) {
            cells.push(full(treasure_frame(t.kind)));
        }
        for _ in ship.treasures.len()..SLOT_TREASURES[kind] {
            cells.push(bare(0x39));
        }
        for &h in ship.heroes.iter().take(SLOT_HEROES[kind]) {
            cells.push(bare(0x30 + h));
        }
        for _ in ship.heroes.len()..SLOT_HEROES[kind] {
            cells.push(bare(0x2f));
        }
    }
    cells
}

/// A settlement's or town's slot cells (0x43bfe0, page 0): its stock of each
/// item, marines in the first row, medium and strong weapons in the next
/// two, dimmed where there is none, each with an arrow to open its units;
/// at the start of the second row its armour and rams, of the third its
/// goods.
pub fn settlement_slots(scene: &Scene, obj: &crate::objects::MapObject) -> Vec<Option<SlotCell>> {
    if !matches!(
        obj.class,
        openconquest_formats::ObjectClass::Building(
            crate::economy::SETTLEMENT | crate::economy::TOWN
        )
    ) {
        return Vec::new();
    }
    let stock = scene.garrison(obj.id);
    let nation3 = obj.owner.is_some_and(|o| o.0 == 3);
    let mut cells = vec![None; 11];
    let cell = |frame: u8, dim: u8, count: u32| {
        Some(SlotCell {
            frame: if count > 0 { frame } else { dim },
            bar: None,
            mark: Some(u8::from(count == 0)),
            count,
        })
    };
    for item in 0..9u8 {
        let (frame, dim) = match item {
            0..=2 => {
                let f = item + if nation3 { 13 } else { 10 };
                (f, f + 15)
            }
            _ => (item - 2, item + 13),
        };
        cells[item as usize * 4 / 3] = cell(frame, dim, stock[item as usize]);
    }
    cells[3] = cell(0x20, 0x1f, 0);
    cells[7] = cell(0x2e, 0x2d, 0);
    cells
}
