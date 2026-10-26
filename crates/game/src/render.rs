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
/// The minimap's view frame (measured on the original).
const MINIMAP_VIEW: u32 = 0x00_e6_07_06;
const SELECTION: u32 = 0x00_f0_e8_a0;
/// The selection corners' highlight (measured; not palette colours).
const SELECTION_GLINT: u32 = 0x00_d0_d0_5c;
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
    let (above, mut below): (Vec<_>, Vec<_>) = visible.partition(flying);
    // Things on the water's surface (fish shoals, leviathans, whirlpools,
    // reefs) under everything; the rest by how far down the map they reach,
    // nearer ones over farther.
    let surface = |o: &crate::objects::MapObject| {
        matches!(
            o.class,
            openac_formats::ObjectClass::FishZone
                | openac_formats::ObjectClass::WhaleZone
                | openac_formats::ObjectClass::Whirlpool
                | openac_formats::ObjectClass::Reef(_)
        )
    };
    let bottom = |o: &crate::objects::MapObject| {
        o.frame_at(time_ms)
            .first()
            .map_or(o.y, |l| o.y + l.dy as f32 + l.image.height as f32 / 2.0)
    };
    below.sort_by(|a, b| {
        (!surface(a), bottom(a))
            .partial_cmp(&(!surface(b), bottom(b)))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let draw = |frame: &mut Frame, layers: &[crate::objects::Layer], x: f32, y: f32| {
        for layer in layers {
            let img = &layer.image;
            let left = (x + layer.dx as f32 - img.width as f32 / 2.0).round() as i32;
            let top = (y + layer.dy as f32 - img.height as f32 / 2.0).round() as i32;
            draw_image(frame, screen, camera, lut, img, left, top);
        }
    };
    // Gulls' shadows on the water.
    let (gulls, shadows) = (&scene.gull_art.0, &scene.gull_art.1);
    for g in &scene.gulls {
        if let Some(l) = g
            .frame()
            .and_then(|f| shadows.get(f))
            .and_then(|f| f.first())
        {
            let (x, y) = Scene::gull_shadow(g);
            draw_image_as(
                frame, screen, camera, lut, &l.image, x as i32, y as i32, true,
            );
        }
    }
    for obj in below {
        draw(frame, obj.frame_at(time_ms), obj.x, obj.y);
        // Its banner, from the picture's top left.
        if let (Some((flag, dx, dy)), Some(first), true) = (
            &obj.banner,
            obj.frame_at(time_ms).first(),
            obj.owner.is_some(),
        ) {
            let left = (obj.x - first.image.width as f32 / 2.0).round() as i32 + dx;
            let top = (obj.y - first.image.height as f32 / 2.0).round() as i32 + dy;
            draw_image(frame, screen, camera, lut, flag, left, top);
        }
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
    // Gulls over everything.
    for g in &scene.gulls {
        if let Some(l) = g.frame().and_then(|f| gulls.get(f)).and_then(|f| f.first()) {
            draw_image(frame, screen, camera, lut, &l.image, g.x as i32, g.y as i32);
        }
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
    draw_image_as(frame, screen, camera, lut, img, left, top, false);
}

/// Draws `img` at world `(left, top)`; `as_shadow` darkens what lies under
/// its pixels instead (the original's shadow table, 0x5c1358).
#[allow(clippy::too_many_arguments)]
fn draw_image_as(
    frame: &mut Frame,
    screen: &Screen,
    camera: &Camera,
    lut: &[u32; 256],
    img: &Image,
    left: i32,
    top: i32,
    as_shadow: bool,
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
                *d = if px == SHADOW || as_shadow {
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

/// Draws the selection box around object `id` (with its hull bar), and its
/// route if it is a ship.
pub fn draw_selection(
    frame: &mut Frame,
    screen: &Screen,
    scene: &Scene,
    camera: &Camera,
    id: u32,
    time_ms: u64,
    lut: &[u32; 256],
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
    // Where the player sent it: the helm marker (`WPMOVE`, a 29x36 frame
    // centred on the point), numbered when there are waypoints.
    if let Some(ship) = obj.ship.as_ref().filter(|s| s.task.is_none()) {
        let (marker, digits) = (&scene.waypoint_art.0, &scene.waypoint_art.1);
        let many = ship.marks.len() > 1;
        for (n, &(mx, my)) in ship.marks.iter().enumerate() {
            let (left, top) = (mx as i32 - 14, my as i32 - 18);
            if let Some(img) = marker {
                draw_image(frame, screen, camera, lut, img, left, top);
            }
            if many {
                let label = (n + 1).to_string();
                for (k, d) in label.bytes().enumerate() {
                    if let Some(Some(img)) = digits.get((d - b'0') as usize) {
                        draw_image(
                            frame,
                            screen,
                            camera,
                            lut,
                            img,
                            left + 19 + 10 * k as i32,
                            top + 18,
                        );
                    }
                }
            }
        }
    }
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
    // The box: the picture's frame and 2 pixels around it (measured on the
    // original), its corners 13-pixel yellow angles lined inside in a darker
    // colour, with a small highlight curving between.
    let Some(first) = obj.frame_at(time_ms).first() else {
        return;
    };
    let (w, h) = (first.image.width as f32, first.image.height as f32);
    // A building's box is its footprint from its cell, 4 pixels more
    // (measured on the original's settlement).
    let t = TILE_SIZE as f32;
    let (left, top, bw, bh) = match (obj.cell, obj.class) {
        (Some((cx, cy)), ObjectClass::Building(class)) => {
            let cells = if matches!(class, 0x32 | 0x33) { 2 } else { 1 };
            let size = cells * TILE_SIZE as i64 + 4;
            (cx as f32 * t + 2.0, cy as f32 * t + 2.0, size, size)
        }
        _ => (
            obj.x + first.dx as f32 - w / 2.0,
            obj.y + first.dy as f32 - h / 2.0,
            w as i64 + 4,
            h as i64 + 4,
        ),
    };
    let origin = to_screen(left.round() - 2.0, top.round() - 2.0);
    let s = scale;
    // A pixel of the box in box pixels, mirrored into each corner.
    let mut corner_px = |x: i64, y: i64, c: u32| {
        for (mx, my) in [
            (x, y),
            (bw - 1 - x, y),
            (x, bh - 1 - y),
            (bw - 1 - x, bh - 1 - y),
        ] {
            put(
                origin.0 + (mx as f32 * s) as i64,
                origin.1 + (my as f32 * s) as i64,
                c,
            );
        }
    };
    // Palette 248 (a shimmering yellow), 247 inside it, 117 the
    // highlight's brightest.
    for k in 0..13 {
        corner_px(k, 0, lut[248]);
        corner_px(0, k, lut[248]);
    }
    for k in 1..12 {
        corner_px(k, 1, lut[247]);
        corner_px(1, k, lut[247]);
    }
    for &(x, y, c) in &[
        (3, 3, 0x00_f3_f3_9e),
        (4, 3, 0x00_e4_e4_7b),
        (3, 4, 0x00_e4_e4_7b),
        (5, 3, SELECTION_GLINT),
        (6, 3, SELECTION_GLINT),
        (4, 4, lut[117]),
        (3, 5, SELECTION_GLINT),
        (3, 6, SELECTION_GLINT),
    ] {
        corner_px(x, y, c);
    }
    // The hull bar on the box's top edge: a black frame 4 high, two rows of
    // colour inside (as the panel's slot bars).
    if let Some(c) = &obj.combat {
        let len = bw - 28;
        let fill = ((len - 2) as f32 * (c.hull / c.max_hull).clamp(0.0, 1.0)) as i64;
        let colours = match c.hull / c.max_hull {
            f if f >= 0.5 => [lut[200], lut[199]],
            f if f >= 0.25 => [lut[203], lut[157]],
            _ => [lut[65], lut[68]],
        };
        for y in 0..4 {
            for x in 0..len {
                let inside = (1..3).contains(&y) && (1..=fill).contains(&x);
                let c = if inside { colours[y as usize - 1] } else { 0 };
                put(
                    origin.0 + ((14 + x) as f32 * s) as i64,
                    origin.1 + (y as f32 * s) as i64,
                    c,
                );
            }
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

/// Fog of war over the view, as the original draws it (0x5762d0): over a
/// covered cell (explored code 0) the cloud picture, tiled 9 x 8 cells
/// across the map; over the others, black stippled through the in-sight
/// edge mask, then the clouds through the explored edge mask.
pub fn draw_fog(
    frame: &mut Frame,
    screen: &Screen,
    scene: &Scene,
    camera: &Camera,
    lut: &[u32; 256],
) {
    if !scene.fog {
        return;
    }
    let Some(art) = scene.fog_art.as_deref() else {
        return;
    };
    let scale = camera.scale(screen);
    let t = TILE_SIZE as i32;
    let w = screen.w.min(frame.width.saturating_sub(screen.x));
    let h = screen.h.min(frame.height.saturating_sub(screen.y));
    let cloud = &art.cloud;
    let (tiles_x, tiles_y) = (
        (cloud.width as i32 / t).max(1),
        (cloud.height as i32 / t).max(1),
    );
    let cols: Vec<i32> = (0..w)
        .map(|sx| sample(camera.x, sx, scale).floor() as i32)
        .collect();
    for sy in 0..h {
        let wy = sample(camera.y, sy, scale).floor() as i32;
        let (cy, iy) = (wy.div_euclid(t), wy.rem_euclid(t));
        let ty = cy.rem_euclid(tiles_y) * t + iy;
        let row =
            &mut frame.pixels[((screen.y + sy) * frame.width + screen.x) as usize..][..w as usize];
        for (px, &wx) in row.iter_mut().zip(&cols) {
            let (cx, ix) = (wx.div_euclid(t), wx.rem_euclid(t));
            let (dark, fog) = scene.fog_codes(cx, cy);
            let cloud_px = || {
                let tx = cx.rem_euclid(tiles_x) * t + ix;
                cloud.pixels[(ty as u32 * cloud.width + tx as u32) as usize]
                    .map_or(0, |p| crate::terrain::resolve(p, lut))
            };
            if dark == 0 {
                *px = cloud_px();
                continue;
            }
            let (ix, iy) = (ix as u32, iy as u32);
            if let Some(m) = art.fog.get(fog as usize) {
                if m.get(ix, iy) {
                    *px = 0;
                }
            }
            if let Some(m) = art.dark.get(dark as usize) {
                if m.get(ix, iy) {
                    *px = cloud_px();
                }
            }
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
    /// The first minimap pixel shown.
    pub origin: (u32, u32),
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
            origin: (0, 0),
        }
    }

    /// The bar's minimap, as the original's: 24 x 24 cells at 8 pixels a
    /// cell in the box `(x, y, w, h)` (192 x 192 at 800x600), from the top
    /// left, a smaller map not stretched, from the scene's minimap origin.
    pub fn window(scene: &Scene, x: u32, y: u32, w: u32, h: u32) -> Self {
        let cell = TILE_SIZE / MINIMAP_DIVISOR;
        let zoom = w as f32 / (crate::scene::MINIMAP_CELLS as u32 * cell) as f32;
        let (mw, mh) = scene.minimap_size;
        let shown = |map: u32, box_px: u32| ((map as f32 * zoom) as u32).min(box_px);
        Self {
            x,
            y,
            w: shown(mw, w),
            h: shown(mh, h),
            zoom,
            origin: (
                scene.minimap_origin.get().0.max(0) as u32 * cell,
                scene.minimap_origin.get().1.max(0) as u32 * cell,
            ),
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
        let d = MINIMAP_DIVISOR as f32;
        (
            ((sx - self.x as f32) / self.zoom + self.origin.0 as f32) * d,
            ((sy - self.y as f32) / self.zoom + self.origin.1 as f32) * d,
        )
    }
}

pub fn draw_minimap(
    frame: &mut Frame,
    rect: &MinimapRect,
    scene: &Scene,
    camera: &Camera,
    screen: &Screen,
    lut: &[u32; 256],
) {
    let (mw, mh) = scene.minimap_size;
    let MinimapRect {
        x: x0,
        y: y0,
        w,
        h,
        zoom,
        origin,
    } = *rect;
    let art = scene.fog_art.as_deref();
    // Minimap pixels per cell.
    let cell = (TILE_SIZE / MINIMAP_DIVISOR) as i32;
    for y in 0..h {
        let my = ((y as f32 / zoom) as u32 + origin.1).min(mh - 1);
        for x in 0..w {
            let mx = ((x as f32 / zoom) as u32 + origin.0).min(mw - 1);
            let c = resolve(scene.minimap[(my * mw + mx) as usize], lut);
            frame.put((x0 + x) as i64, (y0 + y) as i64, c);
        }
    }
    let k = zoom / MINIMAP_DIVISOR as f32;
    let (ox, oy) = (origin.0 as f32 * zoom, origin.1 as f32 * zoom);
    let inside = |px: i64, py: i64| {
        px >= x0 as i64 && py >= y0 as i64 && px < (x0 + w) as i64 && py < (y0 + h) as i64
    };
    // Object icons at their cells (Mapa.cpp 0x51cdd0, message 0xd20): a
    // player's by nation (the shape) and player (the colour), 16x16 for
    // settlements and towns; the rest by class.
    if let Some((small, big)) = art.and_then(|a| a.icons.as_ref()) {
        // Things that stay put are drawn under the clouds, which hide them
        // until explored; ships and creatures only in sight.
        let moves = |o: &MapObject| o.ship.is_some() || matches!(o.class, ObjectClass::Monster(_));
        for obj in scene
            .objects
            .iter()
            .filter(|o| !moves(o) || scene.is_visible(o))
        {
            let Some((icon, large)) = minimap_icon(obj) else {
                continue;
            };
            let atlas = if large { big } else { small };
            let size = atlas.width;
            if (icon + 1) * size > atlas.height {
                continue;
            }
            // At its cell: a building's or deposit's top-left one.
            let t = TILE_SIZE as f32;
            let fields = &obj.record.fields;
            let (cx, cy) = match (obj.cell, obj.record.class_id) {
                (Some(c), _) => c,
                (None, 0x190) if fields.len() > 7 => (fields[6] as i32, fields[7] as i32),
                _ => ((obj.x / t).floor() as i32, (obj.y / t).floor() as i32),
            };
            let left = x0 as f32 + (cx * cell) as f32 * zoom - ox;
            let top = y0 as f32 + (cy * cell) as f32 * zoom - oy;
            for iy in 0..(size as f32 * zoom) as i64 {
                for ix in 0..(size as f32 * zoom) as i64 {
                    let (px, py) = (left as i64 + ix, top as i64 + iy);
                    if !inside(px, py) {
                        continue;
                    }
                    let (sx, sy) = ((ix as f32 / zoom) as u32, (iy as f32 / zoom) as u32);
                    // Black is see-through.
                    match atlas.pixels[((icon * size + sy) * size + sx) as usize] {
                        Some(p) if p & 0xff_ffff != 0 => frame.put(px, py, resolve(p, lut)),
                        _ => {}
                    }
                }
            }
        }
    }
    // Fog as in the view, over the icons, in the small masks and clouds
    // (Mapa.cpp: s_DARK, s_FOG, OBL_SM).
    for y in 0..h {
        let my = ((y as f32 / zoom) as u32 + origin.1).min(mh - 1);
        for x in 0..w {
            let mx = ((x as f32 / zoom) as u32 + origin.0).min(mw - 1);
            let (px, py) = ((x0 + x) as i64, (y0 + y) as i64);
            let mut c = frame.pixels[(py as u32 * frame.width + px as u32) as usize];
            let before = c;
            if let (true, Some(art)) = (scene.fog, art) {
                let (cx, cy) = (mx as i32 / cell, my as i32 / cell);
                let (ix, iy) = ((mx as i32 % cell) as u32, (my as i32 % cell) as u32);
                let (dark, fog) = scene.fog_codes(cx, cy);
                let cloud = || {
                    art.small_cloud.as_ref().map_or(0, |img| {
                        let px = mx % img.width;
                        let py = my % img.height;
                        img.pixels[(py * img.width + px) as usize].map_or(0, |p| resolve(p, lut))
                    })
                };
                if dark == 0 {
                    c = cloud();
                } else {
                    if art
                        .small_fog
                        .get(fog as usize)
                        .is_some_and(|m| m.get(ix, iy))
                    {
                        c = 0;
                    }
                    if art
                        .small_dark
                        .get(dark as usize)
                        .is_some_and(|m| m.get(ix, iy))
                    {
                        c = cloud();
                    }
                }
            }
            if c != before {
                frame.put(px, py, c);
            }
        }
    }
    // The view: a dashed red frame, two on and one off from its corner
    // (the right side a step later; measured on the original).
    let (vw, vh) = camera.view_size(screen);
    let vx0 = x0 as i64 + (camera.x * k - ox).round() as i64;
    let vy0 = y0 as i64 + (camera.y * k - oy).round() as i64;
    let vx1 = vx0 + (vw * k).round() as i64 - 1;
    let vy1 = vy0 + (vh * k).round() as i64 - 1;
    let mut put = |px: i64, py: i64| {
        if inside(px, py) {
            frame.put(px, py, MINIMAP_VIEW);
        }
    };
    for x in vx0..=vx1 {
        if (x - vx0) % 3 != 0 {
            put(x, vy0);
            put(x, vy1);
        }
    }
    for y in vy0..=vy1 {
        if (y - vy0) % 3 != 0 {
            put(vx0, y);
        }
        if (y - vy0) % 3 != 2 {
            put(vx1, y);
        }
    }
}

/// An object's minimap icon (an index into `LMAP8X8`, or `LMAP16X16` when
/// large) as the original picks it (0x50da60, 0x51cdd0).
fn minimap_icon(obj: &MapObject) -> Option<(u32, bool)> {
    if let Some((nation, slot)) = obj
        .owner
        .filter(|_| matches!(obj.class, ObjectClass::Ship | ObjectClass::Building(_)))
    {
        let icon = if nation == 4 { 0x10 } else { nation * 4 + slot };
        let large = matches!(obj.class, ObjectClass::Building(0x32 | 0x33));
        return Some((icon, large));
    }
    let icon = match obj.record.class_id {
        0x190 => 0x11,
        0x191 => 0x12,
        0x192 => 0x13,
        0x258 => 0x14,
        0x1f4 => 0x15,
        0x25d => 0x16,
        0x25c => 0x17,
        0x259 | 0x25a => 0x18,
        0x46..=0x48 => 0x19,
        0x1fa => 0x1a,
        0x1f8 => 0x1b,
        0x1fb => 0x1c,
        0x45 => 0x1d,
        0x49 => 0x1e,
        0x42..=0x44 => 0x1f,
        0x1f5 => 0x20,
        0x1f6 => 0x21,
        0x1fd => 0x22,
        0x1f9 => 0x23,
        0x25e => return Some((0x11, true)),
        _ => return None,
    };
    Some((icon, false))
}

/// Darkens a pixel to 3/4 brightness (see [`SHADOW`]).
#[inline]
pub fn darken(px: u32) -> u32 {
    let r = (px >> 16 & 0xff) * 3 / 4;
    let g = (px >> 8 & 0xff) * 3 / 4;
    let b = (px & 0xff) * 3 / 4;
    r << 16 | g << 8 | b
}
