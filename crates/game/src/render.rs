//! Software rendering into a `0x00RRGGBB` framebuffer.

use openac_formats::ObjectClass;

use crate::assets::{rgb, GameData, Image, SHADOW};
use crate::objects::{Layer, MapObject};
use crate::scene::{Scene, Seen};
use crate::terrain::{resolve, MINIMAP_DIVISOR};
use crate::view::{Camera, Screen};
use openac_formats::map::TILE_SIZE;

/// Color shown outside the map.
const BACKGROUND: u32 = 0x00_0b_1a_26;
const MINIMAP_VIEW: u32 = 0x00_ff_ff_ff;
const MINIMAP_OWN: u32 = 0x00_60_f0_60;
const MINIMAP_ENEMY: u32 = 0x00_f0_50_40;
const MINIMAP_CREATURE: u32 = 0x00_c0_70_f0;
const SELECTION: u32 = 0x00_f0_e8_a0;
const SELECTION_ROUTE: u32 = 0x00_d0_f0_ff;
const HULL_OK: u32 = 0x00_40_d0_40;
const HULL_LOST: u32 = 0x00_c0_30_20;

pub struct Frame<'a> {
    pub pixels: &'a mut [u32],
    pub width: u32,
    pub height: u32,
}

impl Frame<'_> {
    #[inline]
    pub fn put(&mut self, x: i64, y: i64, c: u32) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.pixels[(y as u32 * self.width + x as u32) as usize] = c;
        }
    }
}

/// Palette lookup table for animated pixels at time `time_ms`.
pub fn palette_lut(data: &GameData, time_ms: u64) -> [u32; 256] {
    let mut lut = [0u32; 256];
    for (slot, c) in lut.iter_mut().zip(data.palette.0.iter()) {
        *slot = rgb(*c);
    }
    for c in &data.cycles {
        lut[c.index as usize] = rgb(c.color_at(time_ms));
    }
    lut
}

/// World coordinate sampled by screen pixel `s` (pixel centres), relative
/// to the viewport origin.
#[inline]
fn sample(origin: f32, s: u32, scale: f32) -> f32 {
    origin + (s as f32 + 0.5) / scale
}

/// Draws the terrain as seen by `camera` into the viewport.
pub fn draw_world(
    frame: &mut Frame,
    screen: &Screen,
    scene: &Scene,
    camera: &Camera,
    lut: &[u32; 256],
) {
    let scale = camera.scale(screen);
    let t = &scene.terrain;
    let world = |origin: f32, s: u32, limit: u32| {
        let w = sample(origin, s, scale).floor();
        (w >= 0.0 && w < limit as f32).then_some(w as u32)
    };
    let w = screen.w.min(frame.width.saturating_sub(screen.x));
    let h = screen.h.min(frame.height.saturating_sub(screen.y));
    let cols: Vec<Option<u32>> = (0..w).map(|sx| world(camera.x, sx, t.width)).collect();
    for sy in 0..h {
        let dst =
            &mut frame.pixels[((screen.y + sy) * frame.width + screen.x) as usize..][..w as usize];
        let Some(wy) = world(camera.y, sy, t.height) else {
            dst.fill(BACKGROUND);
            continue;
        };
        let row = &t.pixels[(wy * t.width) as usize..][..t.width as usize];
        for (d, c) in dst.iter_mut().zip(&cols) {
            *d = match c {
                Some(c) => resolve(row[*c as usize], lut),
                None => BACKGROUND,
            };
        }
    }
}

/// Draws the mission objects over the terrain, back to front.
///
/// Sprites are sampled on the same world-pixel grid as the terrain so they
/// stay aligned with it at any scale.
pub fn draw_objects(
    frame: &mut Frame,
    screen: &Screen,
    scene: &Scene,
    camera: &Camera,
    lut: &[u32; 256],
    time_ms: u64,
) {
    // Harpies and dragons fly over everything else.
    let flying = |o: &&crate::objects::MapObject| {
        matches!(o.class, openac_formats::ObjectClass::Monster(0x1f8 | 0x1f9))
    };
    let visible = scene.objects.iter().filter(|o| scene.is_visible(o));
    let (above, below): (Vec<_>, Vec<_>) = visible.partition(flying);
    let draw = |frame: &mut Frame, layers: &[crate::objects::Layer], x: f32, y: f32| {
        for layer in layers {
            let img = &layer.image;
            let left = (x + layer.dx as f32 - img.width as f32 / 2.0).round() as i32;
            let top = (y + layer.dy as f32 - img.height as f32 / 2.0).round() as i32;
            draw_image(frame, screen, camera, lut, img, left, top);
        }
    };
    for obj in below {
        draw(frame, obj.frame_at(time_ms), obj.x, obj.y);
    }
    // Walkways of unloading ships, and their porters.
    for p in &scene.planks {
        for layers in p.frame() {
            draw(frame, layers, p.x, p.y);
        }
    }
    for p in &scene.porters {
        if let Some(layers) = p.frame(time_ms) {
            draw(frame, layers, p.x, p.y);
        }
    }
    for obj in above {
        draw(frame, obj.frame_at(time_ms), obj.x, obj.y);
    }
    let t = TILE_SIZE as f32;
    for fx in scene.effects.iter().filter(|fx| {
        scene.seen((fx.x / t).floor() as i32, (fx.y / t).floor() as i32) == Seen::InSight
    }) {
        if let Some(layers) = fx.frame() {
            for layer in layers {
                let (left, top) = (
                    (fx.x + layer.dx as f32 - layer.image.width as f32 / 2.0).round() as i32,
                    (fx.y + layer.dy as f32 - layer.image.height as f32 / 2.0).round() as i32,
                );
                draw_image(frame, screen, camera, lut, &layer.image, left, top);
            }
        }
    }
}

/// Draws a sprite whose top-left corner is at world position `(left, top)`.
fn draw_image(
    frame: &mut Frame,
    screen: &Screen,
    camera: &Camera,
    lut: &[u32; 256],
    img: &Image,
    left: i32,
    top: i32,
) {
    let scale = camera.scale(screen);
    let (vw, vh) = camera.view_size(screen);
    let w = screen.w.min(frame.width.saturating_sub(screen.x));
    let h = screen.h.min(frame.height.saturating_sub(screen.y));
    // Viewport pixel range whose sample point falls inside [w0, w1) in world space.
    let span = |origin: f32, w0: i32, w1: i32, limit: u32| {
        let s0 = ((w0 as f32 - origin) * scale - 0.5).ceil().max(0.0);
        let s1 = ((w1 as f32 - origin) * scale - 0.5)
            .ceil()
            .min(limit as f32);
        (s0 as u32, s1.max(s0) as u32)
    };
    let (right, bottom) = (left + img.width as i32, top + img.height as i32);
    if right as f32 <= camera.x
        || bottom as f32 <= camera.y
        || left as f32 >= camera.x + vw
        || top as f32 >= camera.y + vh
    {
        return;
    }
    let (sx0, sx1) = span(camera.x, left, right, w);
    let (sy0, sy1) = span(camera.y, top, bottom, h);
    let cols: Vec<u32> = (sx0..sx1)
        .map(|sx| {
            (sample(camera.x, sx, scale).floor() as i32 - left).clamp(0, img.width as i32 - 1)
                as u32
        })
        .collect();
    for sy in sy0..sy1 {
        let iy = (sample(camera.y, sy, scale).floor() as i32 - top).clamp(0, img.height as i32 - 1)
            as u32;
        let src = &img.pixels[(iy * img.width) as usize..][..img.width as usize];
        let start = ((screen.y + sy) * frame.width + screen.x + sx0) as usize;
        let dst = &mut frame.pixels[start..][..cols.len()];
        for (d, &ix) in dst.iter_mut().zip(&cols) {
            if let Some(px) = src[ix as usize] {
                *d = if px == SHADOW {
                    darken(*d)
                } else {
                    resolve(px, lut)
                };
            }
        }
    }
}

/// World position of the top-left corner of a layer's image.
pub fn layer_origin(obj: &MapObject, layer: &Layer) -> (i32, i32) {
    let img = &layer.image;
    (
        (obj.x + layer.dx as f32 - img.width as f32 / 2.0).round() as i32,
        (obj.y + layer.dy as f32 - img.height as f32 / 2.0).round() as i32,
    )
}

/// The topmost object with a solid pixel at world position `(wx, wy)`.
pub fn hit_test(scene: &Scene, wx: f32, wy: f32, time_ms: u64) -> Option<u32> {
    let (px, py) = (wx.floor() as i32, wy.floor() as i32);
    // Treasures are small sparkles: near enough counts.
    let treasure = scene.objects.iter().find(|o| {
        o.treasure.is_some()
            && scene.is_visible(o)
            && (o.x - wx).powi(2) + (o.y - wy).powi(2) <= 24.0 * 24.0
    });
    if let Some(t) = treasure {
        return Some(t.id);
    }
    let solid = |obj: &&crate::objects::MapObject| {
        obj.frame_at(time_ms).iter().any(|layer| {
            let (left, top) = layer_origin(obj, layer);
            layer.image.is_solid(px - left, py - top)
        })
    };
    let visible = || scene.objects.iter().rev().filter(|o| scene.is_visible(o));
    // Ships, buildings and creatures first: what can be selected or fought.
    if let Some(o) = visible().find(|o| o.combat.is_some() && solid(o)) {
        return Some(o.id);
    }
    // Deposits are areas: anywhere in a fishing or leviathan ground (its
    // cells, record fields 6-9), near an amber deposit.
    let t = openac_formats::map::TILE_SIZE as f32;
    let (cx, cy) = ((wx / t).floor() as i32, (wy / t).floor() as i32);
    if let Some(o) = visible().find(|o| {
        o.deposit.is_some() && {
            let f = |i| o.record.field(i).unwrap_or(0) as i32;
            match o.class {
                openac_formats::ObjectClass::FishZone | openac_formats::ObjectClass::WhaleZone => {
                    let (x, y, w, h) = if o.class == openac_formats::ObjectClass::FishZone {
                        (f(6), f(7), f(8), f(9))
                    } else {
                        (f(7), f(8), f(9), f(10))
                    };
                    cx >= x && cy >= y && cx < x + w.max(1) && cy < y + h.max(1)
                }
                _ => (o.x - wx).powi(2) + (o.y - wy).powi(2) <= (t * 0.6).powi(2) || solid(o),
            }
        }
    }) {
        return Some(o.id);
    }
    visible().find(|o| solid(o)).map(|o| o.id)
}

/// World-space bounding box `(x0, y0, x1, y1)` of an object's current frame.
pub fn object_bounds(obj: &MapObject, time_ms: u64) -> Option<(i32, i32, i32, i32)> {
    obj.frame_at(time_ms).iter().fold(None, |acc, layer| {
        let (left, top) = layer_origin(obj, layer);
        let (x0, y0, x1, y1) = layer.image.solid_bounds?;
        let b = (
            left + x0 as i32,
            top + y0 as i32,
            left + x1 as i32,
            top + y1 as i32,
        );
        Some(match acc {
            None => b,
            Some(a) => (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)),
        })
    })
}

/// Draws selection brackets around object `id`, and its route if it is a ship.
pub fn draw_selection(
    frame: &mut Frame,
    screen: &Screen,
    scene: &Scene,
    camera: &Camera,
    id: u32,
    time_ms: u64,
) {
    let Some(obj) = scene.objects.iter().find(|o| o.id == id) else {
        return;
    };
    let scale = camera.scale(screen);
    let to_screen = |wx: f32, wy: f32| {
        (
            screen.x as i64 + ((wx - camera.x) * scale) as i64,
            screen.y as i64 + ((wy - camera.y) * scale) as i64,
        )
    };
    let thick = (scale.round() as i64).max(1);
    let (clip_x0, clip_y0) = (screen.x as i64, screen.y as i64);
    let (clip_x1, clip_y1) = (clip_x0 + screen.w as i64, clip_y0 + screen.h as i64);
    let mut put = |x: i64, y: i64, c: u32| {
        for dy in 0..thick {
            for dx in 0..thick {
                let (px, py) = (x + dx, y + dy);
                if px >= clip_x0 && py >= clip_y0 && px < clip_x1 && py < clip_y1 {
                    frame.put(px, py, c);
                }
            }
        }
    };
    // Route: dotted line through the remaining waypoints.
    if let Some(ship) = &obj.ship {
        let mut from = (obj.x, obj.y);
        for &to in &ship.path {
            let (a, b) = (to_screen(from.0, from.1), to_screen(to.0, to.1));
            let steps = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / (4 * thick)).max(1);
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let x = a.0 as f32 + (b.0 - a.0) as f32 * t;
                let y = a.1 as f32 + (b.1 - a.1) as f32 * t;
                put(x as i64, y as i64, SELECTION_ROUTE);
            }
            from = to;
        }
    }
    let Some((x0, y0, x1, y1)) = object_bounds(obj, time_ms) else {
        return;
    };
    let (sx0, sy0) = to_screen(x0 as f32 - 2.0, y0 as f32 - 2.0);
    let (sx1, sy1) = to_screen(x1 as f32 + 2.0, y1 as f32 + 2.0);
    let arm = ((sx1 - sx0).min(sy1 - sy0) / 4).max(3);
    for i in 0..arm {
        for (x, y) in [
            (sx0 + i, sy0),
            (sx0, sy0 + i),
            (sx1 - i, sy0),
            (sx1, sy0 + i),
            (sx0 + i, sy1),
            (sx0, sy1 - i),
            (sx1 - i, sy1),
            (sx1, sy1 - i),
        ] {
            put(x, y, SELECTION);
        }
    }
    // Hull bar above the box.
    if let Some(c) = &obj.combat {
        let filled = sx0 + ((sx1 - sx0) as f32 * (c.hull / c.max_hull).clamp(0.0, 1.0)) as i64;
        let y = sy0 - 3 * thick;
        for x in (sx0..=sx1).step_by(thick as usize) {
            put(x, y, if x <= filled { HULL_OK } else { HULL_LOST });
        }
    }
}

/// Outlines map cell `cell` as a building site: green if allowed, red if not.
/// Draws the rubber band of a selection being dragged, in window pixels.
pub fn draw_box(frame: &mut Frame, screen: &Screen, (x0, y0, x1, y1): (f32, f32, f32, f32)) {
    let (x0, x1) = (x0.min(x1) as i64, x0.max(x1) as i64);
    let (y0, y1) = (y0.min(y1) as i64, y0.max(y1) as i64);
    let (cx0, cy0) = (screen.x as i64, screen.y as i64);
    let (cx1, cy1) = (cx0 + screen.w as i64, cy0 + screen.h as i64);
    let mut put = |x: i64, y: i64| {
        if x >= cx0 && x < cx1 && y >= cy0 && y < cy1 {
            frame.put(x, y, SELECTION);
        }
    };
    for x in x0..=x1 {
        put(x, y0);
        put(x, y1);
    }
    for y in y0..=y1 {
        put(x0, y);
        put(x1, y);
    }
}

pub fn draw_site(frame: &mut Frame, screen: &Screen, camera: &Camera, cell: (i32, i32), ok: bool) {
    let scale = camera.scale(screen);
    let t = TILE_SIZE as f32;
    let to_screen = |wx: f32, wy: f32| {
        (
            screen.x as i64 + ((wx - camera.x) * scale) as i64,
            screen.y as i64 + ((wy - camera.y) * scale) as i64,
        )
    };
    let (x0, y0) = to_screen(cell.0 as f32 * t, cell.1 as f32 * t);
    let (x1, y1) = to_screen((cell.0 + 1) as f32 * t, (cell.1 + 1) as f32 * t);
    let colour = if ok { HULL_OK } else { HULL_LOST };
    let thick = (scale.round() as i64).max(1);
    let (cx0, cy0) = (screen.x as i64, screen.y as i64);
    let (cx1, cy1) = (cx0 + screen.w as i64, cy0 + screen.h as i64);
    for k in 0..thick {
        for x in x0..=x1 {
            for y in [y0 + k, y1 - k] {
                if x >= cx0 && x < cx1 && y >= cy0 && y < cy1 {
                    frame.put(x, y, colour);
                }
            }
        }
        for y in y0..=y1 {
            for x in [x0 + k, x1 - k] {
                if x >= cx0 && x < cx1 && y >= cy0 && y < cy1 {
                    frame.put(x, y, colour);
                }
            }
        }
    }
}

/// How bright explored cells out of sight are drawn.
const FOG_BRIGHTNESS: f32 = 0.55;

fn scale_rgb(c: u32, k: f32) -> u32 {
    let ch = |shift: u32| ((((c >> shift) & 0xff) as f32 * k) as u32).min(255) << shift;
    ch(16) | ch(8) | ch(0)
}

/// Fog of war over the view: unexplored cells black, explored ones out of
/// sight dimmed, blended between cell centres so the edges are soft.
pub fn draw_fog(frame: &mut Frame, screen: &Screen, scene: &Scene, camera: &Camera) {
    if !scene.fog {
        return;
    }
    let scale = camera.scale(screen);
    let t = TILE_SIZE as f32;
    let w = screen.w.min(frame.width.saturating_sub(screen.x));
    let h = screen.h.min(frame.height.saturating_sub(screen.y));
    let level = |cx: i32, cy: i32| scene.seen(cx, cy) as u8 as f32;
    // Brightness for blended levels 0..2 in 1/64 steps, in 8.8 fixed point.
    let lut: Vec<u32> = (0..=128)
        .map(|i| {
            let v = i as f32 / 64.0;
            let b = if v < 1.0 {
                FOG_BRIGHTNESS * v
            } else {
                FOG_BRIGHTNESS + (1.0 - FOG_BRIGHTNESS) * (v - 1.0)
            };
            (b * 256.0) as u32
        })
        .collect();
    // Per column: the cell to the left of the sample and the blend weight.
    let cols: Vec<(i32, f32)> = (0..w)
        .map(|sx| {
            let fx = sample(camera.x, sx, scale) / t - 0.5;
            (fx.floor() as i32, fx - fx.floor())
        })
        .collect();
    for sy in 0..h {
        let fy = sample(camera.y, sy, scale) / t - 0.5;
        let (cy, ty) = (fy.floor() as i32, fy - fy.floor());
        let row =
            &mut frame.pixels[((screen.y + sy) * frame.width + screen.x) as usize..][..w as usize];
        // The current cell: its left and right levels blended down the row.
        let mut cell = (i32::MIN, 0.0f32, 0.0f32);
        for (px, &(cx, tx)) in row.iter_mut().zip(&cols) {
            if cell.0 != cx {
                let (a, b) = (level(cx, cy), level(cx + 1, cy));
                let (c, d) = (level(cx, cy + 1), level(cx + 1, cy + 1));
                cell = (cx, a + (c - a) * ty, b + (d - b) * ty);
            }
            let (_, left, right) = cell;
            if left == 2.0 && right == 2.0 {
                continue;
            }
            if left == 0.0 && right == 0.0 {
                *px = 0;
                continue;
            }
            let v = left + (right - left) * tx;
            let k = lut[(v * 64.0) as usize];
            // Scale red and blue together, then green.
            let p = *px;
            *px =
                ((((p & 0xff00ff) * k) >> 8) & 0xff00ff) | ((((p & 0x00ff00) * k) >> 8) & 0x00ff00);
        }
    }
}

/// Darkens a screen rectangle (for text backgrounds).
pub fn shade_rect(frame: &mut Frame, x: u32, y: u32, w: u32, h: u32) {
    for py in y..(y + h).min(frame.height) {
        for px in x..(x + w).min(frame.width) {
            let p = &mut frame.pixels[(py * frame.width + px) as usize];
            *p = darken(darken(*p));
        }
    }
}

/// Draws `img` scaled by `scale` with its top-left corner at window position
/// `(x, y)` (nearest neighbour; transparent pixels skipped). `src` selects a
/// sub-rectangle `(x, y, w, h)` of the image.
pub fn blit_scaled(
    frame: &mut Frame,
    img: &Image,
    src: (u32, u32, u32, u32),
    x: f32,
    y: f32,
    scale: f32,
) {
    let (sx, sy, sw, sh) = src;
    let dx0 = x.round().max(0.0) as u32;
    let dy0 = y.round().max(0.0) as u32;
    let dx1 = ((x + sw as f32 * scale).round().max(0.0) as u32).min(frame.width);
    let dy1 = ((y + sh as f32 * scale).round().max(0.0) as u32).min(frame.height);
    for dy in dy0..dy1 {
        let iy = sy + (((dy as f32 + 0.5 - y) / scale) as u32).min(sh - 1);
        let row = &img.pixels[(iy * img.width) as usize..][..img.width as usize];
        for dx in dx0..dx1 {
            let ix = sx + (((dx as f32 + 0.5 - x) / scale) as u32).min(sw - 1);
            if let Some(p) = row[ix as usize] {
                frame.pixels[(dy * frame.width + dx) as usize] = if p == SHADOW {
                    darken(frame.pixels[(dy * frame.width + dx) as usize])
                } else {
                    p
                };
            }
        }
    }
}

/// Where the minimap is drawn: window rectangle and screen pixels per
/// minimap pixel.
#[derive(Debug, Clone, Copy)]
pub struct MinimapRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub zoom: f32,
}

impl MinimapRect {
    /// The largest minimap that fits in the box `(x, y, w, h)`, centred.
    pub fn fit(scene: &Scene, x: u32, y: u32, w: u32, h: u32) -> Self {
        let (mw, mh) = (scene.minimap_size.0.max(1), scene.minimap_size.1.max(1));
        let zoom = (w as f32 / mw as f32).min(h as f32 / mh as f32);
        let (mw, mh) = ((mw as f32 * zoom) as u32, (mh as f32 * zoom) as u32);
        Self {
            x: x + (w - mw.min(w)) / 2,
            y: y + (h - mh.min(h)) / 2,
            w: mw,
            h: mh,
            zoom,
        }
    }

    /// An overlay in the bottom-right corner (used when the interface is hidden).
    pub fn overlay(scene: &Scene, window: (u32, u32)) -> Self {
        let size = window.1 / 4;
        let margin = (window.1 / 100).max(4);
        Self::fit(
            scene,
            window.0.saturating_sub(size * 3 / 2 + margin),
            window.1.saturating_sub(size + margin),
            size * 3 / 2,
            size,
        )
    }

    pub fn contains(&self, sx: f32, sy: f32) -> bool {
        sx >= self.x as f32
            && sy >= self.y as f32
            && sx < (self.x + self.w) as f32
            && sy < (self.y + self.h) as f32
    }

    /// World position under window position `(sx, sy)`.
    pub fn to_world(self, sx: f32, sy: f32) -> (f32, f32) {
        let k = MINIMAP_DIVISOR as f32 / self.zoom;
        ((sx - self.x as f32) * k, (sy - self.y as f32) * k)
    }
}

pub fn draw_minimap(
    frame: &mut Frame,
    rect: &MinimapRect,
    scene: &Scene,
    camera: &Camera,
    screen: &Screen,
) {
    let (mw, mh) = scene.minimap_size;
    let MinimapRect {
        x: x0,
        y: y0,
        w,
        h,
        zoom,
    } = *rect;
    for y in 0..h {
        let sy = ((y as f32 / zoom) as u32).min(mh - 1);
        for x in 0..w {
            let sx = ((x as f32 / zoom) as u32).min(mw - 1);
            let mut c = scene.minimap[(sy * mw + sx) as usize];
            if scene.fog {
                // Minimap pixels are MINIMAP_DIVISOR world pixels.
                let cell = |v: u32| (v * MINIMAP_DIVISOR / TILE_SIZE) as i32;
                c = match scene.seen(cell(sx), cell(sy)) {
                    Seen::Unexplored => 0,
                    Seen::Explored => scale_rgb(c, FOG_BRIGHTNESS),
                    Seen::InSight => c,
                };
            }
            frame.put((x0 + x) as i64, (y0 + y) as i64, c);
        }
    }
    let k = zoom / MINIMAP_DIVISOR as f32;
    // Object markers.
    let dot = (zoom as i64).clamp(2, 4);
    for obj in scene.objects.iter().filter(|o| scene.is_visible(o)) {
        // Yours in green, other players in red, creatures in purple;
        // buildings are drawn a shade darker than ships.
        let side = if scene.is_own(obj) {
            MINIMAP_OWN
        } else if obj.owner == Some(crate::combat::MONSTERS) {
            MINIMAP_CREATURE
        } else if obj.owner.is_some() {
            MINIMAP_ENEMY
        } else {
            continue;
        };
        let color = match obj.class {
            ObjectClass::Ship => side,
            ObjectClass::Building(_) | ObjectClass::Barbarian(_) => darken(side),
            _ if obj.combat.is_some() => side,
            _ => continue,
        };
        let mx = x0 as i64 + (obj.x * k) as i64 - dot / 2;
        let my = y0 as i64 + (obj.y * k) as i64 - dot / 2;
        for dy in 0..dot {
            for dx in 0..dot {
                frame.put(mx + dx, my + dy, color);
            }
        }
    }
    // Visible area, clipped to the minimap.
    let (vw, vh) = camera.view_size(screen);
    let vx0 = (camera.x * k).max(0.0) as i64;
    let vy0 = (camera.y * k).max(0.0) as i64;
    let vx1 = ((camera.x + vw) * k).min(w as f32) as i64 - 1;
    let vy1 = ((camera.y + vh) * k).min(h as f32) as i64 - 1;
    if vx1 > vx0 && vy1 > vy0 {
        let (ox, oy) = (x0 as i64, y0 as i64);
        for x in vx0..=vx1 {
            frame.put(ox + x, oy + vy0, MINIMAP_VIEW);
            frame.put(ox + x, oy + vy1, MINIMAP_VIEW);
        }
        for y in vy0..=vy1 {
            frame.put(ox + vx0, oy + y, MINIMAP_VIEW);
            frame.put(ox + vx1, oy + y, MINIMAP_VIEW);
        }
    }
}

/// Darkens a pixel to 3/4 brightness (see [`SHADOW`]).
#[inline]
fn darken(px: u32) -> u32 {
    let r = (px >> 16 & 0xff) * 3 / 4;
    let g = (px >> 8 & 0xff) * 3 / 4;
    let b = (px & 0xff) * 3 / 4;
    r << 16 | g << 8 | b
}
