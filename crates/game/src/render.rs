//! Software rendering into a `0x00RRGGBB` framebuffer.

use openconquest_formats::ObjectClass;

use crate::assets::{rgb, GameData, Image, BLEND, SHADOW};
use crate::objects::{Layer, MapObject};
use crate::scene::{Scene, Seen};
use crate::terrain::{resolve, MINIMAP_DIVISOR};
use crate::view::{Camera, Screen};
use openconquest_formats::map::TILE_SIZE;

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

    #[inline]
    pub fn get(&self, x: i64, y: i64) -> Option<u32> {
        (x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height)
            .then(|| self.pixels[(y as u32 * self.width + x as u32) as usize])
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
        matches!(
            o.class,
            openconquest_formats::ObjectClass::Monster(0x1f8 | 0x1f9)
        )
    };
    let visible = scene.objects.iter().filter(|o| scene.is_visible(o));
    let (above, mut below): (Vec<_>, Vec<_>) = visible.partition(flying);
    // Things on the water's surface (fish shoals, leviathans, whirlpools,
    // reefs) under everything; the rest by how far down the map they reach,
    // nearer ones over farther.
    let surface = |o: &crate::objects::MapObject| {
        matches!(
            o.class,
            openconquest_formats::ObjectClass::FishZone
                | openconquest_formats::ObjectClass::WhaleZone
                | openconquest_formats::ObjectClass::Whirlpool
                | openconquest_formats::ObjectClass::Reef(_)
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
    // Ships' wakes, under the hulls.
    for d in &scene.droplets {
        let frames = match d.trail {
            Some(k) => match scene.trail_art.get(&k) {
                Some(f) => f,
                None => continue,
            },
            None if d.small => &scene.wake_art.0,
            None => &scene.wake_art.1,
        };
        if let Some(layers) = d.frame(frames.len()).and_then(|f| frames.get(f)) {
            let (x, y) = d.at();
            draw(frame, layers, x, y);
        }
    }
    // Boarders' walkways, and hooks' lines (palette colour 0x6d, 0x4964c0):
    // just behind the hulls (their draw orders the hull's less 0x46 and
    // 0x47), so over the water and under the ships.
    for (layers, x, y) in scene.planks_shown() {
        draw(frame, layers, x, y);
    }
    // A founding ship's walkway and its party going down it, likewise.
    for (layers, x, y) in scene.landings_shown() {
        draw(frame, layers, x, y);
    }
    let rope = Image::new(1, 1, vec![Some(lut[0x6d])]);
    for ((x0, y0), (x1, y1)) in scene.ropes() {
        let (x0, y0, x1, y1) = (
            x0.round() as i32,
            y0.round() as i32,
            x1.round() as i32,
            y1.round() as i32,
        );
        let n = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for k in 0..=n {
            let x = x0 + (x1 - x0) * k / n;
            let y = y0 + (y1 - y0) * k / n;
            draw_image(frame, screen, camera, lut, &rope, x, y);
        }
    }
    for obj in below {
        // A flock: its sheep, each with its top-left 7 and 14 pixels up-left
        // of its place (0x4331c0).
        if let Some(flock) = scene.flocks.get(&obj.id) {
            for s in &flock.sheep {
                let frames = &scene.sheep_art[s.dir % 8][usize::from(s.walking)];
                if let Some(layers) = frames.get(s.frame).or(frames.first()) {
                    if let Some(l) = layers.first() {
                        let (w, h) = (l.image.width as f32, l.image.height as f32);
                        draw(frame, layers, s.x - 7.0 + w / 2.0, s.y - 14.0 + h / 2.0);
                    }
                }
            }
            continue;
        }
        // A ship with the phantom (`+0x253`): 0x4df420, at the casting
        // (0x4de3a2), its end (0x4df23c) and the ship's set-up (0x4e2b44),
        // and 0x4e1bb0 at each layer's loading, pass its layers 0-3 and
        // 5-8 to 0x4df380, which, for the player at this seat (0x5c7508
        // `+0x18` the ship's `+0x24f`), draws them through the ghost table
        // (0x50e460 with 0x5cdd2c), and for every other player hides them
        // (0x50e350: `+0x8d`, the sprite's flag 0x40000). The flames'
        // frames are of kind 1, which 0x57b7f0 draws without the mask.
        // A building with the phantom (`+0x134`): its tick (LandObj
        // 0x425990 at 0x4260a0-0x4262e3) does the same to its picture
        // (layer 0), banner (1) and shields (0xd, 0xc), the mask 0x80
        // wide whatever the picture, for the player whose nation and slot
        // are its `+0x54 / +0x55`; its fire and smoke (layers 3-10) are
        // left as they are.
        let phantom = Scene::is_phantom(obj);
        let hidden = scene.phantom_hidden(obj);
        let ship = obj.ship.is_some();
        if hidden && ship {
            for (l, left, top) in scene.hunt_gear(obj) {
                draw_image(frame, screen, camera, lut, &l.image, left, top);
            }
            continue;
        }
        // The mask's width: a ship's picture's (`+0x61`), a building's
        // 0x80.
        let mask = |img: &Image| phantom.then_some(if ship { img.width } else { 0x80 });
        // Limed, its pictures and flames are whitened (0x4df510).
        let limed = obj
            .combat
            .as_ref()
            .is_some_and(|c| c.lime > 0.0)
            .then_some(&scene.lime_remap);
        match limed {
            _ if hidden => {}
            None if !phantom => draw(frame, scene.frame_of(obj, time_ms), obj.x, obj.y),
            _ => {
                // Every sprite of a ship's or building's picture has the
                // picture's size (`+0x61`) and stands at its top left, so
                // the ghost table's place is the sprite's own pixel.
                for layer in scene.frame_of(obj, time_ms) {
                    let (left, top) = layer_origin(obj, layer);
                    draw_image_ghosted(
                        frame,
                        screen,
                        camera,
                        lut,
                        &layer.image,
                        left,
                        top,
                        false,
                        limed,
                        mask(&layer.image),
                    );
                }
            }
        }
        // A leviathan hunt's haul and line, from their top left.
        for (l, left, top) in scene.hunt_gear(obj) {
            draw_image(frame, screen, camera, lut, &l.image, left, top);
        }
        // Flames on a burning ship.
        if let Some((layers, left, top)) = scene.fire_on(obj, time_ms) {
            if let Some(l) = layers.first() {
                draw_image_remapped(
                    frame,
                    screen,
                    camera,
                    lut,
                    &l.image,
                    left.round() as i32,
                    top.round() as i32,
                    false,
                    limed,
                );
            }
        }
        // A building's smoke and fire (layers 10 and 6), from their top left.
        for (layers, left, top) in scene.building_fire_on(obj.id) {
            if let Some(l) = layers.first() {
                draw_image_remapped(
                    frame, screen, camera, lut, &l.image, left, top, false, limed,
                );
            }
        }
        // An amber deposit's glint, part of it.
        if let Some((f, x, y)) = scene
            .sparkles
            .get(&obj.id)
            .and_then(|s| s.frame(scene.sparkle_art.len()))
        {
            if let Some(layers) = scene.sparkle_art.get(f) {
                draw(frame, layers, x, y);
            }
        }
        // Its banner, from the picture's top left; limed, remapped as the
        // picture (0x425990 at 0x4263c6: 0x50df60 on layer 1 too).
        if let (Some((flags, dx, dy)), Some(first), true) = (
            &obj.banner,
            obj.frame_at(time_ms).first(),
            obj.owner.is_some() && obj.asleep.is_none(),
        ) {
            const FLAG_MS: u64 = 75;
            let flag = &flags[(time_ms / FLAG_MS) as usize % flags.len()];
            let left = (obj.x - first.image.width as f32 / 2.0).round() as i32 + dx;
            let top = (obj.y - first.image.height as f32 / 2.0).round() as i32 + dy;
            if !hidden {
                draw_image_ghosted(
                    frame,
                    screen,
                    camera,
                    lut,
                    flag,
                    left,
                    top,
                    false,
                    limed,
                    mask(flag),
                );
            }
        }
        // A building's shields (layers 0xc and 0xd, depth the picture's
        // less 4 and 5: over its fire and banner), from their top left;
        // kind-0 sprites, ghosted or hidden with the phantom as the rest.
        // A ship's (layers 8 and 7, depth both the hull's less 0x4a: over
        // the rest of the ship), likewise (0x4e1bb0 -> 0x4df380); neither
        // is remapped by the lime or the stone.
        for (layers, left, top) in scene.building_shields_on(obj.id) {
            for l in layers.iter().filter(|_| !hidden) {
                draw_image_ghosted(
                    frame,
                    screen,
                    camera,
                    lut,
                    &l.image,
                    left + l.dx,
                    top + l.dy,
                    false,
                    None,
                    mask(&l.image),
                );
            }
        }
    }
    // Walkways of unloading ships, and their porters.
    for p in &scene.planks {
        for (layers, x, y) in p.frame() {
            draw(frame, layers, x, y);
        }
    }
    for p in &scene.porters {
        if let Some(layers) = p.frame(time_ms) {
            draw(frame, layers, p.x, p.y);
        }
    }
    // Landing parties ashore.
    for (layers, x, y) in scene.parties_shown() {
        draw(frame, layers, x, y);
    }
    for obj in above {
        draw(frame, scene.frame_of(obj, time_ms), obj.x, obj.y);
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
    // Fireballs over their cells.
    for f in scene
        .fireballs
        .iter()
        .filter(|f| scene.seen(f.cell.0, f.cell.1) == Seen::InSight)
    {
        if let Some((layers, left, top)) = f.picture() {
            for l in layers {
                draw_image(
                    frame,
                    screen,
                    camera,
                    lut,
                    &l.image,
                    (left + l.dx as f32) as i32,
                    (top + l.dy as f32) as i32,
                );
            }
        }
    }
    // Shots in flight, over everything below them.
    for s in scene
        .shots
        .iter()
        .filter(|s| scene.seen((s.x / t).floor() as i32, (s.y / t).floor() as i32) == Seen::InSight)
    {
        if let Some(layer) = s.layer() {
            draw_centred(frame, screen, camera, lut, &layer.image, s.x, s.y);
        }
    }
}

/// Draws a sprite whose top-left corner is at world position `(left, top)`.
/// Draws `img` centred on the world point `(x, y)`.
pub fn draw_centred(
    frame: &mut Frame,
    screen: &Screen,
    camera: &Camera,
    lut: &[u32; 256],
    img: &Image,
    x: f32,
    y: f32,
) {
    let left = (x - img.width as f32 / 2.0).round() as i32;
    let top = (y - img.height as f32 / 2.0).round() as i32;
    draw_image(frame, screen, camera, lut, img, left, top);
}

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

/// The configuration's "lighting effects": with them off, the colours a
/// frame darkens through the shadow table (shadows, the flames' 0x26) are
/// left out. The original's switch (0x4ed8d0) sets the sprite system's
/// flag `+0x638`, which the drawing (0x57252b, 0x572825) reads to send
/// such frames to 0x57b470 instead of the remapping blitter: it skips a
/// copied run starting in the range (repeated runs are still painted;
/// not reproduced here).
pub static LIGHTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Draws `img` at world `(left, top)`; `as_shadow` puts what lies under
/// its pixels through the shadow table instead ([`SHADOW_TABLE`]).
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
    draw_image_remapped(frame, screen, camera, lut, img, left, top, as_shadow, None);
}

/// The shadow table (0x5c1358, static data): a layer's shadow remap set
/// by 0x50e050(layer, 0x5c1358) (0x55dd10: the sprite's `+0x63c`) with
/// 0x50e130(layer, 1, colour, 0) (0x55dd40: table `+0x5c` 0, value the
/// colour, count 1). The drawing (0x572585-0x57259e) takes the sprite's
/// table for a frame with count 1 and no table of its own, and the
/// blitter 0x57b7f0 draws the colour as the entry at the palette index
/// beneath: objects' shadows 0xfe, buildings' smoke 0x26, others 0xd,
/// 0x27, 0x2a.
#[rustfmt::skip]
pub const SHADOW_TABLE: [u8; 256] = [
    0x00, 0x78, 0x06, 0xb6, 0xb1, 0x08, 0x08, 0x0a, 0x0b, 0xb8, 0x0c, 0x0d, 0x0e, 0x8e, 0x8e, 0x8d,
    0x8a, 0x15, 0x16, 0x17, 0x18, 0x18, 0x19, 0x19, 0x1a, 0x88, 0x10, 0x0e, 0x1b, 0x1b, 0x1c, 0x1d,
    0x1e, 0x1e, 0x1f, 0x20, 0x78, 0x21, 0x22, 0x29, 0x2a, 0x2b, 0x2b, 0x2b, 0x2e, 0x2f, 0x30, 0x31,
    0x31, 0x32, 0x32, 0x35, 0x36, 0x37, 0x37, 0x38, 0x39, 0x39, 0x3c, 0x4e, 0x50, 0x3f, 0x85, 0xd5,
    0x87, 0x43, 0x5f, 0x5f, 0x60, 0x62, 0x5d, 0xa2, 0xd9, 0x83, 0x3e, 0x4c, 0x4d, 0x10, 0x17, 0x18,
    0x18, 0x10, 0x54, 0x55, 0x56, 0x57, 0x58, 0x58, 0x39, 0x59, 0x5c, 0x5d, 0x5e, 0x5e, 0x88, 0x62,
    0x63, 0x64, 0x64, 0x64, 0x89, 0xa2, 0x47, 0x47, 0xd4, 0x97, 0x83, 0x3d, 0x82, 0x80, 0x49, 0x6b,
    0x6c, 0x6d, 0x6d, 0x6e, 0x6c, 0x6c, 0x49, 0x7a, 0x1e, 0xa7, 0x90, 0x92, 0x92, 0x81, 0x82, 0xd9,
    0x3e, 0xd8, 0xd8, 0x85, 0x86, 0x51, 0xd2, 0x51, 0x89, 0x8a, 0x8b, 0x8b, 0x8c, 0x8c, 0x8d, 0x0d,
    0x1b, 0x1b, 0xd7, 0x96, 0xd7, 0xd7, 0xd3, 0x85, 0xd1, 0x17, 0x50, 0x13, 0x99, 0x9a, 0x98, 0x66,
    0x46, 0x45, 0x88, 0xa4, 0xa4, 0x87, 0x40, 0x1c, 0x90, 0xb3, 0xb3, 0xb3, 0xa9, 0x05, 0x7f, 0x6e,
    0x6f, 0x8f, 0x0c, 0xb2, 0xb2, 0x09, 0xb8, 0xb8, 0x0d, 0xb9, 0xb9, 0xb9, 0xb9, 0xb9, 0xb9, 0xb9,
    0xb7, 0xb6, 0xc3, 0xc6, 0x38, 0xc5, 0xc5, 0xc4, 0xc7, 0xc7, 0x2b, 0x3a, 0xc1, 0xcd, 0xce, 0xcf,
    0xcf, 0xa2, 0x0f, 0x0f, 0x86, 0x0f, 0x85, 0xd3, 0xd3, 0x3f, 0xd4, 0xa5, 0x84, 0xd4, 0xd4, 0x97,
    0x95, 0x95, 0xde, 0x82, 0xdf, 0x81, 0x6a, 0xe1, 0xe0, 0x7f, 0x7f, 0xb9, 0xb9, 0xb9, 0xb9, 0xb9,
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0x4e, 0xde, 0xde, 0x4f, 0x17, 0x9f, 0x9f, 0xc1, 0xbe, 0x00,
];

/// The quick lime's whitening (0x5c1758, put on a limed ship's or
/// building's pictures by 0x4df510 / 0x425990): palette index to index.
#[rustfmt::skip]
pub const LIME: [u8; 256] = [
    0, 38, 38, 38, 38, 38, 38, 38, 37, 36, 35, 34, 120, 31, 30, 144,
    27, 36, 35, 34, 33, 33, 32, 31, 30, 29, 144, 33, 35, 38, 38, 38,
    38, 38, 38, 38, 38, 38, 38, 34, 33, 32, 31, 29, 38, 38, 38, 38,
    37, 34, 32, 33, 32, 31, 30, 144, 27, 14, 38, 37, 35, 36, 34, 33,
    33, 34, 34, 33, 33, 120, 31, 31, 38, 38, 38, 30, 144, 27, 34, 120,
    120, 29, 38, 36, 34, 120, 31, 30, 28, 141, 120, 31, 30, 30, 144, 30,
    30, 30, 30, 29, 28, 31, 33, 34, 36, 38, 38, 38, 38, 38, 38, 38,
    38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38,
    38, 38, 37, 35, 33, 32, 31, 30, 28, 13, 14, 141, 13, 28, 29, 35,
    36, 37, 37, 38, 37, 36, 35, 35, 35, 34, 35, 38, 38, 38, 38, 37,
    35, 34, 30, 28, 142, 33, 37, 38, 38, 38, 38, 38, 38, 38, 38, 38,
    38, 38, 36, 38, 38, 38, 38, 35, 33, 34, 34, 34, 34, 34, 35, 38,
    37, 38, 35, 33, 144, 28, 29, 30, 31, 31, 38, 38, 38, 141, 14, 13,
    29, 32, 30, 31, 33, 30, 34, 33, 35, 36, 37, 37, 36, 35, 37, 38,
    38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 38, 236, 237, 238, 239,
    240, 241, 242, 243, 244, 245, 38, 247, 248, 38, 38, 38, 38, 38, 38, 38,
];

/// The flames' blend (0x5c1858, put on a building's or a ship's fire by
/// 0x50e240(layer, 1, 0x5c1858, 1, 7), 0x4241f0 / 0x4e1ff8): the
/// blitter 0x57b7f0 draws a colour c of 1-7 as row c - 1 at the palette
/// index beneath (0x57b80f-0x57b82b, 0x57b9c1-0x57b9dd).
#[rustfmt::skip]
pub const FLAME_BLEND: [[u8; 256]; 7] = [
    [
        0x00, 0x26, 0x26, 0x25, 0x26, 0x26, 0x26, 0x01, 0xad, 0xfd, 0xfd, 0x04, 0x06, 0x06, 0x1d, 0xd8,
        0x40, 0xf6, 0xf6, 0xf6, 0xf9, 0x9d, 0x9e, 0x9d, 0x9b, 0xdb, 0x13, 0x20, 0x22, 0x24, 0x26, 0x26,
        0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x27, 0x27, 0x27, 0x27, 0x27, 0x26, 0x25, 0x24, 0x2c,
        0x2c, 0x2c, 0x2e, 0xb5, 0xb5, 0x33, 0x33, 0x33, 0x34, 0x57, 0xcb, 0xcb, 0xf6, 0x74, 0x6f, 0x6e,
        0xe6, 0x41, 0x41, 0x42, 0xca, 0xca, 0xfc, 0xfc, 0xb0, 0xaf, 0x73, 0x3a, 0x3c, 0x50, 0xf6, 0xf9,
        0x9d, 0xa6, 0x72, 0x74, 0x76, 0x76, 0x52, 0x53, 0x54, 0xc4, 0xfc, 0xfc, 0xfc, 0x9f, 0xa0, 0x41,
        0xca, 0xca, 0xa1, 0xa0, 0x99, 0xfc, 0xfb, 0xfb, 0xe9, 0x71, 0xb0, 0xb0, 0xb0, 0x25, 0x26, 0x26,
        0x26, 0x26, 0x26, 0x26, 0x26, 0xb0, 0xb0, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x25,
        0xaf, 0xaf, 0xaf, 0xae, 0xe6, 0xe2, 0x69, 0xa6, 0x66, 0x47, 0x19, 0x10, 0x0e, 0x1b, 0x1c, 0x21,
        0x23, 0x23, 0x24, 0x24, 0xea, 0xea, 0xea, 0xe9, 0xe6, 0xf9, 0xf6, 0x75, 0xcb, 0xcb, 0x74, 0xe9,
        0xfb, 0xfc, 0x9f, 0x44, 0x62, 0xe6, 0x74, 0x25, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26,
        0x26, 0x01, 0x02, 0x01, 0xfd, 0xfd, 0xad, 0xfd, 0x03, 0xbf, 0xbe, 0xbe, 0xbe, 0xbe, 0xbe, 0xcc,
        0xcc, 0x01, 0xfe, 0xb5, 0x33, 0xc3, 0xc2, 0xc8, 0xc8, 0xc8, 0xf9, 0xcb, 0x01, 0x8c, 0x37, 0xd3,
        0x96, 0x9e, 0x94, 0xe0, 0xfb, 0x48, 0xe6, 0xe7, 0xea, 0x71, 0xae, 0x75, 0xae, 0xe9, 0xae, 0x23,
        0xb0, 0x25, 0xaf, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0xec, 0xed, 0xee, 0xef,
        0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xcb, 0xf7, 0xf8, 0xcb, 0xcb, 0x24, 0xea, 0x26, 0xfe, 0x26,
    ],
    [
        0x00, 0x26, 0x26, 0x01, 0x26, 0x25, 0x25, 0x01, 0xad, 0xfd, 0x03, 0x05, 0x06, 0xb1, 0x1d, 0xd8,
        0x17, 0xf6, 0xf6, 0x9d, 0x9d, 0x9d, 0x9e, 0x9c, 0x11, 0x98, 0x15, 0x1f, 0x21, 0x23, 0x25, 0x26,
        0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x27, 0x27, 0x27, 0x27, 0x27, 0x25, 0x25, 0x24, 0x2c,
        0x2c, 0x2d, 0x2f, 0xb5, 0xb5, 0x33, 0x33, 0x33, 0x34, 0x57, 0xcb, 0xcb, 0xf6, 0x75, 0x6e, 0x6c,
        0x6c, 0x41, 0x41, 0x42, 0xca, 0xca, 0xfc, 0x9f, 0xb0, 0x73, 0x72, 0x3b, 0x4e, 0x50, 0xf6, 0x9d,
        0x9d, 0xdc, 0x74, 0x75, 0x76, 0x52, 0x52, 0x33, 0x55, 0xc4, 0xfc, 0xfc, 0x9f, 0xa0, 0x5a, 0x41,
        0xca, 0xca, 0xa1, 0xa0, 0x45, 0x9f, 0xfb, 0xfb, 0xe9, 0xae, 0xaf, 0xb0, 0xb0, 0xb0, 0x25, 0x26,
        0x26, 0x26, 0x26, 0x26, 0x26, 0xb0, 0xb0, 0x26, 0x26, 0x26, 0x26, 0x25, 0x26, 0x26, 0x25, 0xb0,
        0x73, 0xaf, 0x71, 0xae, 0xe5, 0x6a, 0x69, 0xa6, 0x66, 0xa2, 0x88, 0x10, 0x0e, 0x0d, 0x1c, 0x78,
        0x22, 0x22, 0x23, 0x23, 0xea, 0xea, 0xe7, 0xe9, 0x9e, 0xf9, 0xf6, 0x76, 0xcb, 0xcb, 0x75, 0xe9,
        0xfb, 0xfc, 0x68, 0x45, 0xa3, 0xe2, 0x75, 0xb0, 0x25, 0x25, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26,
        0x26, 0x01, 0xab, 0x02, 0xac, 0xfd, 0xfd, 0xfd, 0x05, 0xbf, 0xbe, 0xbe, 0xbe, 0xbe, 0xbe, 0xcc,
        0xcc, 0x01, 0xac, 0xb5, 0x33, 0xc3, 0xc2, 0xc8, 0xc8, 0xc8, 0xf9, 0xcb, 0x01, 0x8c, 0x37, 0xd0,
        0x96, 0x9e, 0x97, 0x81, 0xe4, 0x91, 0xe6, 0x7b, 0xe8, 0xae, 0xe9, 0x75, 0xae, 0xfb, 0xe9, 0xea,
        0xaf, 0x24, 0xaf, 0x25, 0x25, 0x25, 0x25, 0x26, 0x26, 0x26, 0x26, 0x26, 0xec, 0xed, 0xee, 0xef,
        0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xcb, 0xf7, 0xf8, 0xcb, 0xf6, 0x23, 0xea, 0x25, 0xfe, 0x26,
    ],
    [
        0x00, 0x26, 0x26, 0x01, 0x25, 0x01, 0x01, 0x01, 0x02, 0x03, 0x03, 0x06, 0x07, 0xb1, 0x90, 0xd7,
        0x87, 0xf9, 0x9d, 0x9d, 0x9d, 0x9e, 0x9b, 0x9c, 0x11, 0x98, 0x15, 0x7a, 0x20, 0x22, 0x24, 0x25,
        0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x27, 0x27, 0x27, 0x27, 0x27, 0x25, 0x24, 0x2c, 0x2c,
        0x2c, 0x2d, 0x2f, 0xb5, 0x33, 0x33, 0x33, 0x33, 0x35, 0x57, 0xcb, 0xf6, 0xf6, 0x6f, 0x6c, 0x6b,
        0xe2, 0x41, 0x41, 0x41, 0x42, 0xca, 0xfc, 0x9f, 0xaf, 0x72, 0x74, 0x3c, 0x50, 0x4b, 0x9d, 0x9d,
        0x3a, 0xd6, 0x74, 0x76, 0x76, 0x52, 0x52, 0x33, 0x55, 0x38, 0xfc, 0xfc, 0x9f, 0xa0, 0x5a, 0x41,
        0x43, 0xa1, 0xa1, 0xa0, 0x60, 0xa0, 0xfc, 0xfb, 0xe9, 0xea, 0x71, 0xaf, 0xb0, 0xb0, 0xb0, 0x25,
        0x26, 0x26, 0x26, 0x26, 0xb0, 0xb0, 0xaf, 0x26, 0x26, 0x26, 0x25, 0x25, 0x25, 0x25, 0xb0, 0xaf,
        0x71, 0xea, 0xae, 0xe6, 0xe2, 0x6a, 0xde, 0xa6, 0xd1, 0xa2, 0x88, 0x89, 0x8e, 0x0d, 0x1b, 0xeb,
        0x21, 0x77, 0xea, 0xea, 0xea, 0xe8, 0xe7, 0xe4, 0x9e, 0x9d, 0xf6, 0xf6, 0xf6, 0xcb, 0x75, 0xfb,
        0xfb, 0xfc, 0x67, 0x45, 0xa3, 0xe2, 0x75, 0x23, 0x01, 0x01, 0x25, 0x26, 0x25, 0x26, 0x26, 0x26,
        0x26, 0x23, 0xab, 0xac, 0xac, 0xac, 0xfd, 0x03, 0x05, 0xbf, 0xbe, 0xbe, 0xbe, 0xbe, 0xbe, 0xad,
        0xcc, 0x01, 0xac, 0xb5, 0x33, 0xc3, 0xc2, 0xc8, 0xc8, 0xc8, 0xf9, 0xcb, 0x01, 0x8c, 0x38, 0xd0,
        0x96, 0x9e, 0x97, 0x92, 0xe2, 0x91, 0xe2, 0x7b, 0xe5, 0x6f, 0xe9, 0xe6, 0xe6, 0xfb, 0xe9, 0xea,
        0x71, 0x24, 0x71, 0xb0, 0x25, 0xb0, 0xb0, 0x26, 0x26, 0x26, 0x26, 0x25, 0xec, 0xed, 0xee, 0xef,
        0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xcb, 0xf7, 0xf8, 0xcb, 0xf6, 0xea, 0xea, 0x01, 0xfe, 0x26,
    ],
    [
        0x00, 0x26, 0x26, 0x01, 0x01, 0x01, 0x01, 0xad, 0x04, 0x03, 0x05, 0x07, 0xb1, 0x90, 0x1c, 0xd7,
        0x87, 0x9d, 0x9d, 0x9d, 0x9c, 0x9b, 0x9b, 0x11, 0x12, 0x15, 0x16, 0x1e, 0x1f, 0x21, 0x23, 0x24,
        0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x27, 0x27, 0x27, 0x27, 0x28, 0x25, 0x24, 0x2c, 0x2c,
        0x2d, 0x2e, 0x30, 0x33, 0x33, 0x33, 0x33, 0x34, 0x36, 0x37, 0xf6, 0xf6, 0x9d, 0x6e, 0x6b, 0x49,
        0xa6, 0x41, 0x41, 0x41, 0x42, 0x42, 0xa0, 0x9f, 0x71, 0x74, 0x6f, 0x3c, 0x50, 0x4b, 0x9d, 0x9c,
        0x3a, 0x40, 0x75, 0x76, 0x52, 0x52, 0x53, 0x33, 0x56, 0x38, 0xfc, 0x9f, 0xa0, 0x5a, 0x5a, 0x41,
        0xa1, 0xa1, 0xa1, 0x45, 0x60, 0xa0, 0xfc, 0xfc, 0xfb, 0xe9, 0xae, 0x71, 0x73, 0xb0, 0xb0, 0xb0,
        0x25, 0x26, 0x26, 0x26, 0xb0, 0xaf, 0x73, 0x26, 0x26, 0x26, 0x24, 0xaf, 0x24, 0x25, 0xaf, 0x71,
        0xae, 0xea, 0x6f, 0x6d, 0x6a, 0x80, 0x97, 0xdc, 0x47, 0xa2, 0x88, 0x89, 0x8e, 0x0e, 0x1b, 0xeb,
        0x20, 0x79, 0xe7, 0xe7, 0xe8, 0x7d, 0x7c, 0xe4, 0x9e, 0x9d, 0xf9, 0x9d, 0xf6, 0xcb, 0x76, 0xfb,
        0xfc, 0xca, 0x66, 0x45, 0xa3, 0x69, 0x76, 0x22, 0x23, 0x01, 0x01, 0x25, 0x01, 0x26, 0x25, 0x26,
        0x26, 0x02, 0xaa, 0xab, 0xac, 0xac, 0xfd, 0xc1, 0x07, 0xc0, 0xbe, 0xbe, 0xbe, 0xbe, 0xbe, 0xfd,
        0xcc, 0xad, 0xb5, 0xb5, 0x33, 0xc3, 0xc3, 0xc8, 0xc8, 0xc8, 0xfa, 0xcb, 0x01, 0x8c, 0x38, 0xd0,
        0xd7, 0xda, 0xd4, 0x95, 0x69, 0xd8, 0xe2, 0xe1, 0x7b, 0xe5, 0xe9, 0xe6, 0xe6, 0xfb, 0xe9, 0xea,
        0xea, 0xea, 0xae, 0xaf, 0xaf, 0xb0, 0xb0, 0x25, 0x25, 0xb0, 0x26, 0x01, 0xec, 0xed, 0xee, 0xef,
        0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xcb, 0xf7, 0xf8, 0xcb, 0xf6, 0xea, 0xe9, 0x01, 0xfe, 0x26,
    ],
    [
        0x00, 0x26, 0x26, 0x01, 0x01, 0x01, 0x01, 0xad, 0x04, 0x03, 0x05, 0x07, 0xb1, 0x90, 0x1c, 0xd7,
        0x87, 0x9d, 0x9d, 0x9d, 0x9c, 0x9b, 0x9b, 0x11, 0x12, 0x15, 0x16, 0x1e, 0x1f, 0x21, 0x23, 0x24,
        0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x26, 0x27, 0x27, 0x27, 0x27, 0x28, 0x25, 0x24, 0x2c, 0x2c,
        0x2d, 0x2e, 0x30, 0x33, 0x33, 0x33, 0x33, 0x34, 0x36, 0x37, 0xf6, 0xf6, 0x9d, 0x6e, 0x6b, 0x49,
        0xa6, 0x41, 0x41, 0x41, 0x42, 0x42, 0xa0, 0x9f, 0x71, 0x74, 0x6f, 0x3c, 0x50, 0x4b, 0x9d, 0x9c,
        0x3a, 0x40, 0x75, 0x76, 0x52, 0x52, 0x53, 0x33, 0x56, 0x38, 0xfc, 0x9f, 0xa0, 0x5a, 0x5a, 0x41,
        0xa1, 0xa1, 0xa1, 0x45, 0x60, 0xa0, 0xfc, 0xfc, 0xfb, 0xe9, 0xae, 0x71, 0x73, 0xb0, 0xb0, 0xb0,
        0x25, 0x26, 0x26, 0x26, 0xb0, 0xaf, 0x73, 0x26, 0x26, 0x26, 0x24, 0xaf, 0x24, 0x25, 0xaf, 0x71,
        0xae, 0xea, 0x6f, 0x6d, 0x6a, 0x80, 0x97, 0xdc, 0x47, 0xa2, 0x88, 0x89, 0x8e, 0x0e, 0x1b, 0xeb,
        0x20, 0x79, 0xe7, 0xe7, 0xe8, 0x7d, 0x7c, 0xe4, 0x9e, 0x9d, 0xf9, 0x9d, 0xf6, 0xcb, 0x76, 0xfb,
        0xfc, 0xca, 0x66, 0x45, 0xa3, 0x69, 0x76, 0x22, 0x23, 0x01, 0x01, 0x25, 0x01, 0x26, 0x25, 0x26,
        0x26, 0x02, 0xaa, 0xab, 0xac, 0xac, 0xfd, 0xc1, 0x07, 0xc0, 0xbd, 0xbe, 0xbe, 0xbe, 0xbe, 0xfe,
        0xcc, 0xad, 0xb5, 0xb5, 0x33, 0xc3, 0xc3, 0xc8, 0xc8, 0xc8, 0xfa, 0xcb, 0x01, 0x8c, 0x38, 0xd0,
        0xd7, 0xda, 0xd4, 0x95, 0x69, 0xd8, 0xe2, 0xe1, 0x7b, 0xe5, 0xe9, 0xe6, 0xe6, 0xfb, 0xe9, 0xea,
        0xea, 0xea, 0xae, 0xaf, 0xaf, 0xb0, 0xb0, 0x25, 0x25, 0xb0, 0x26, 0x01, 0xec, 0xed, 0xee, 0xef,
        0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xcb, 0xf7, 0xf8, 0xcb, 0xf6, 0xea, 0xe9, 0x01, 0xfe, 0x26,
    ],
    [
        0x00, 0x26, 0x01, 0xfd, 0xad, 0xad, 0x02, 0x04, 0x06, 0x07, 0xb6, 0x08, 0x0a, 0x0b, 0x1b, 0xd5,
        0x51, 0x9c, 0x9b, 0x9b, 0x11, 0x11, 0x12, 0x99, 0x15, 0x16, 0x18, 0x90, 0x1e, 0x1f, 0x78, 0x22,
        0x23, 0x24, 0x26, 0x26, 0x26, 0x26, 0x26, 0x27, 0x27, 0x27, 0x28, 0x2a, 0x2c, 0x2c, 0x2c, 0x2d,
        0x2e, 0x2f, 0x31, 0x33, 0x33, 0x33, 0x34, 0x35, 0x37, 0x38, 0xf6, 0x9d, 0x3a, 0x6b, 0x4a, 0x3d,
        0xa6, 0x41, 0x41, 0x41, 0x42, 0xa1, 0xa0, 0x66, 0x6e, 0x6e, 0x6c, 0x50, 0x4b, 0x4c, 0x3b, 0x9a,
        0x3c, 0x85, 0x6c, 0x52, 0x52, 0x53, 0x54, 0x56, 0x57, 0x59, 0xa0, 0x5a, 0x5a, 0x5b, 0x5c, 0x43,
        0x44, 0x60, 0x45, 0x60, 0x63, 0x66, 0x9f, 0x9f, 0xe2, 0xe4, 0xe6, 0x6f, 0x70, 0x71, 0x71, 0xaf,
        0xb0, 0xb0, 0xb0, 0xb0, 0xaf, 0x73, 0x72, 0x25, 0x24, 0x23, 0x77, 0xea, 0xea, 0x71, 0x70, 0xea,
        0x6d, 0x7d, 0x7e, 0x6a, 0xdc, 0x83, 0xd4, 0x40, 0xa2, 0x88, 0x89, 0x8a, 0x8d, 0x8e, 0x0e, 0xb1,
        0x1e, 0x7a, 0x7b, 0x7c, 0x7f, 0xe0, 0x93, 0x69, 0x9b, 0x9b, 0x9c, 0x9d, 0xf9, 0xf6, 0x9e, 0xfc,
        0xfc, 0x42, 0xd1, 0x62, 0xa4, 0xdb, 0xe2, 0x79, 0x21, 0xac, 0x02, 0x01, 0x01, 0x01, 0xaf, 0x25,
        0x26, 0x04, 0xb3, 0xa9, 0xc1, 0xac, 0xc1, 0xb6, 0xb6, 0xc0, 0xbc, 0xbd, 0xbe, 0xbe, 0xbe, 0xbf,
        0xc1, 0xfd, 0xb5, 0xc2, 0x35, 0xc4, 0xc6, 0xc8, 0xc8, 0xc8, 0xfa, 0xcb, 0xcc, 0xcd, 0xce, 0xd0,
        0xd3, 0x98, 0x86, 0xd7, 0x68, 0x3f, 0xdb, 0x95, 0x81, 0x6a, 0xe2, 0x9e, 0xe2, 0x69, 0xe2, 0xe4,
        0xe8, 0xe7, 0xe9, 0xea, 0xea, 0xae, 0x71, 0x23, 0x71, 0x71, 0xaf, 0x22, 0xec, 0xed, 0xee, 0xef,
        0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xcb, 0xf7, 0xf8, 0xf6, 0xf9, 0xe9, 0xfb, 0xad, 0xfe, 0x26,
    ],
    [
        0x00, 0x26, 0x01, 0xac, 0x02, 0x02, 0x04, 0x05, 0x07, 0xb6, 0x08, 0x0a, 0x0b, 0x0c, 0x0d, 0x0f,
        0x1a, 0x9b, 0x9b, 0x11, 0x9a, 0x12, 0x15, 0x14, 0x17, 0x18, 0x19, 0x1c, 0x1d, 0x1e, 0x1f, 0x78,
        0x22, 0x23, 0x24, 0x25, 0x26, 0x26, 0x26, 0x27, 0x27, 0x28, 0x29, 0x2a, 0x2c, 0x2c, 0x2d, 0x2e,
        0x2f, 0x30, 0x31, 0x33, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x9d, 0x3a, 0x3b, 0x4a, 0x3d, 0x83,
        0xd6, 0x41, 0x41, 0x42, 0x43, 0xa1, 0x45, 0xd1, 0x7e, 0x6c, 0x6b, 0x4b, 0x4b, 0x4c, 0x3b, 0x3c,
        0x4e, 0x87, 0x49, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x59, 0x5a, 0x5a, 0x5b, 0x5c, 0x5d, 0x5f,
        0x60, 0x60, 0x46, 0x61, 0x63, 0x46, 0x98, 0x68, 0x69, 0xe2, 0xe5, 0x6d, 0x6e, 0x6f, 0x70, 0x71,
        0xaf, 0xb0, 0xaf, 0xb0, 0x73, 0x72, 0x74, 0x23, 0x22, 0x77, 0x79, 0xe7, 0xe7, 0x77, 0x6f, 0xe5,
        0x6b, 0x7b, 0xe0, 0x80, 0x83, 0x84, 0x85, 0x85, 0xa2, 0x88, 0x89, 0x8b, 0x8d, 0x8e, 0x0e, 0x90,
        0x1d, 0xa7, 0xe1, 0xe1, 0xdf, 0x81, 0x92, 0xde, 0xda, 0x11, 0x9c, 0x9e, 0x9d, 0xf9, 0x9e, 0xfc,
        0x9f, 0xa1, 0x47, 0x62, 0xa4, 0xd6, 0xa6, 0x7b, 0xeb, 0xab, 0xab, 0xac, 0xac, 0x01, 0x71, 0xb0,
        0xb0, 0x06, 0xb3, 0xa9, 0xb5, 0xc1, 0x07, 0xb6, 0x0a, 0xb9, 0xbb, 0xbc, 0xbd, 0xbe, 0xbe, 0xbf,
        0xc0, 0x03, 0xc2, 0xc2, 0xc4, 0xc5, 0xc6, 0xc9, 0xc8, 0xc8, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf,
        0xd0, 0x66, 0x86, 0xd7, 0x97, 0xd5, 0xdc, 0x96, 0xd9, 0x80, 0x69, 0x9b, 0x80, 0x9f, 0x69, 0x7f,
        0xe3, 0x7c, 0xe4, 0xe8, 0xe9, 0xae, 0xae, 0xea, 0xea, 0xea, 0x71, 0x78, 0xec, 0xed, 0xee, 0xef,
        0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xcb, 0xf7, 0xf8, 0xf6, 0xfa, 0xfb, 0xfb, 0xfd, 0xfe, 0x26,
    ],
];

/// `draw_image_as`, its colours put through `remap` (colour to colour).
#[allow(clippy::too_many_arguments)]
fn draw_image_remapped(
    frame: &mut Frame,
    screen: &Screen,
    camera: &Camera,
    lut: &[u32; 256],
    img: &Image,
    left: i32,
    top: i32,
    as_shadow: bool,
    remap: Option<&std::collections::HashMap<u32, u32>>,
) {
    draw_image_ghosted(
        frame, screen, camera, lut, img, left, top, as_shadow, remap, None,
    );
}

/// The phantom's ghost table (0x5cdd2c, 0x800 bytes of bss): filled at
/// start (0x519280 at 0x5193e5-0x51942b) as 128 rows of 16 bytes, 0xaa
/// on an even row and 0x55 on an odd one. 0x4df380 puts it on a phantom
/// ship's layers for its owner (0x50e460 -> 0x55e010: the sprite's
/// `+0xb8`, and the picture's width `+0x61` as the mask's width
/// `+0xbc`).
pub const GHOST_TABLE: [u8; 0x800] = {
    let mut t = [0u8; 0x800];
    let mut i = 0;
    while i < 0x800 {
        t[i] = if (i / 16) % 2 == 0 { 0xaa } else { 0x55 };
        i += 1;
    }
    t
};

/// Whether pixel `(x, y)` of a picture under a mask `width` wide shows through the
/// ghost table: the drawing (0x572391-0x5723a4, 0x5725cc-0x5725df)
/// passes the table with a row of `(width + 7) / 8` bytes and the
/// pixel's place in the picture, and the blitter 0x57b7f0 (0x57b835-
/// 0x57b84b) tests bit `0x80 >> (x & 7)` of byte `row x y + x / 8`.
/// A ship's pictures (56 to 80 square, their own width) and a
/// building's (at most 128 square, width 0x80) stay within the table.
pub fn ghost_shows(width: u32, x: u32, y: u32) -> bool {
    let i = (width as usize).div_ceil(8) * y as usize + x as usize / 8;
    GHOST_TABLE[i] & (0x80 >> (x & 7)) != 0
}

/// `draw_image_remapped`, with `ghost` (the mask's width) its pixels drawn
/// only where the ghost table shows them ([`ghost_shows`]):
/// 0x57b7f0 on a frame of kind 0 (header byte 0xd & 3; 0x57c5df-
/// 0x57c616) draws a pixel of the shadow colour through the shadow table
/// whatever the mask, and the rest only on a set bit.
#[allow(clippy::too_many_arguments)]
fn draw_image_ghosted(
    frame: &mut Frame,
    screen: &Screen,
    camera: &Camera,
    lut: &[u32; 256],
    img: &Image,
    left: i32,
    top: i32,
    as_shadow: bool,
    remap: Option<&std::collections::HashMap<u32, u32>>,
    ghost: Option<u32>,
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
    let lighting = LIGHTING.load(std::sync::atomic::Ordering::Relaxed);
    if as_shadow && !lighting {
        return;
    }
    let (sx0, sx1) = span(camera.x, left, right, w);
    let (sy0, sy1) = span(camera.y, top, bottom, h);
    let mut beneath = Beneath::default();
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
            match src[ix as usize] {
                // A shadow's colour as the shadow table's entry at the
                // colour beneath (0x57b7f0 with 0x5c1358).
                Some(px) if px == SHADOW || as_shadow => {
                    if lighting {
                        let under = beneath.index(*d, lut);
                        *d = lut[SHADOW_TABLE[under as usize] as usize];
                    }
                }
                // A flame's colour c (1-7) through row c - 1 of the blend
                // table at the colour beneath (0x57b7f0); with the lighting
                // off left out, as the shadows.
                Some(px) if px & BLEND != 0 => {
                    if lighting {
                        let c = (px & 0xff) as usize;
                        let under = beneath.index(*d, lut);
                        *d = lut[FLAME_BLEND[c - 1][under as usize] as usize];
                    }
                }
                Some(_) if ghost.is_some_and(|w| !ghost_shows(w, ix, iy)) => {}
                Some(px) => {
                    let px = remap.and_then(|m| m.get(&px).copied()).unwrap_or(px);
                    *d = resolve(px, lut)
                }
                None => {}
            }
        }
    }
}

/// The palette index of a colour on the screen, for the flames' blend and
/// the shadow table. The original reads the screen's byte beneath; the
/// port's frame holds colours, so the index is the first whose colour it
/// is, or, for a colour none has, the nearest.
#[derive(Default)]
struct Beneath(Option<std::collections::HashMap<u32, u8>>);

impl Beneath {
    fn index(&mut self, px: u32, lut: &[u32; 256]) -> u8 {
        let map = self.0.get_or_insert_with(|| {
            let mut m = std::collections::HashMap::new();
            for (i, &c) in lut.iter().enumerate() {
                m.entry(c).or_insert(i as u8);
            }
            m
        });
        if let Some(&i) = map.get(&px) {
            return i;
        }
        let ch = |c: u32, s: u32| (c >> s & 0xff) as i32;
        let i = (0..256)
            .min_by_key(|&i| {
                let c = lut[i];
                [16, 8, 0]
                    .iter()
                    .map(|&s| (ch(c, s) - ch(px, s)).pow(2))
                    .sum::<i32>()
            })
            .unwrap_or(0) as u8;
        map.insert(px, i);
        i
    }
}

/// World position of the top-left corner of a layer's image (a half
/// pixel taken up, so a picture of odd size stands alike on either side
/// of the map's edge).
pub fn layer_origin(obj: &MapObject, layer: &Layer) -> (i32, i32) {
    let img = &layer.image;
    (
        (obj.x + layer.dx as f32 - img.width as f32 / 2.0 + 0.5).floor() as i32,
        (obj.y + layer.dy as f32 - img.height as f32 / 2.0 + 0.5).floor() as i32,
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
    // A building asleep cannot be clicked (0x423460).
    let visible = || {
        scene
            .objects
            .iter()
            .rev()
            .filter(|o| scene.is_visible(o) && o.asleep.is_none())
    };
    // Harpies and dragons first (0x49a580 tests the cells' flyers before
    // their occupants), then the land creatures' jams (`Scene::jam_at`);
    // land creatures are picked only so.
    let walker = |o: &&crate::objects::MapObject| {
        matches!(
            o.class,
            openconquest_formats::ObjectClass::Monster(0x1f4..=0x1f6)
        )
    };
    let flyer = |o: &&crate::objects::MapObject| {
        matches!(
            o.class,
            openconquest_formats::ObjectClass::Monster(0x1f8 | 0x1f9)
        )
    };
    // A flyer by its creature's mask (vt+8 0x41deb0, `Scene::flyer_hit`).
    if let Some(o) =
        visible().find(|o| flyer(o) && o.combat.is_some() && scene.flyer_hit(o.id, px, py))
    {
        return Some(o.id);
    }
    // The occupants of the pointer's cell and of those about it, in
    // 0x49a580's order: a jam by its members' pictures, anything else by
    // its own pixels.
    for cell in scene.pick_cells(px, py) {
        if let Some(id) = scene.jam_at(cell, px, py) {
            return Some(id);
        }
        if let Some(k) = scene.slot_object(cell) {
            let o = &scene.objects[k];
            if scene.is_visible(o) && o.asleep.is_none() && solid(&o) {
                return Some(o.id);
            }
        }
    }
    // Then the pointer's cell's own slot: the Gorgon or a siren by her
    // picture's rectangle and state (`Scene::lurker_at`); neither is picked
    // otherwise.
    let t = openconquest_formats::map::TILE_SIZE as i32;
    if let Some(id) = scene.lurker_at((px.div_euclid(t), py.div_euclid(t)), px, py) {
        return Some(id);
    }
    let walker = |o: &&crate::objects::MapObject| {
        walker(o)
            || matches!(
                o.class,
                openconquest_formats::ObjectClass::Monster(0x1fa | 0x1fd)
            )
    };
    // Ships, buildings and creatures: what can be selected or fought.
    if let Some(o) = visible().find(|o| !walker(o) && o.combat.is_some() && solid(o)) {
        return Some(o.id);
    }
    // Deposits are areas: anywhere in a fishing ground (its cells, record
    // fields 6-9), near an amber deposit; a leviathan where its picture is
    // (0x495a60: the picture's rect, then its `WHALEM_` mask).
    let t = openconquest_formats::map::TILE_SIZE as f32;
    let (cx, cy) = ((wx / t).floor() as i32, (wy / t).floor() as i32);
    if let Some(o) = visible().find(|o| {
        o.deposit.is_some() && {
            let f = |i| o.record.field(i).unwrap_or(0) as i32;
            match o.class {
                openconquest_formats::ObjectClass::FishZone => {
                    let (x, y, w, h) = (f(6), f(7), f(8), f(9));
                    cx >= x && cy >= y && cx < x + w.max(1) && cy < y + h.max(1)
                }
                openconquest_formats::ObjectClass::WhaleZone => scene.leviathan_hit(o.id, (px, py)),
                _ => (o.x - wx).powi(2) + (o.y - wy).powi(2) <= (t * 0.6).powi(2) || solid(o),
            }
        }
    }) {
        return Some(o.id);
    }
    visible().find(|o| !walker(o) && solid(o)).map(|o| o.id)
}

/// Draws the selection box around object `id` (with its hull bar).
#[allow(clippy::too_many_arguments)]
pub fn draw_selection(
    frame: &mut Frame,
    screen: &Screen,
    data: &GameData,
    scene: &Scene,
    camera: &Camera,
    id: u32,
    time_ms: u64,
    zoom: usize,
    lut: &[u32; 256],
) {
    let Some(obj) = scene.objects.iter().find(|o| o.id == id) else {
        return;
    };
    if draw_frame_sprite(frame, screen, data, obj, camera, time_ms, zoom, true, lut) {
        return;
    }
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

thread_local! {
    /// The selection frames (`RAM_`, objects.dkx).
    static FRAMES: std::cell::RefCell<crate::assets::ImageCache> =
        std::cell::RefCell::new(crate::assets::ImageCache::new(crate::assets::Source::Objects));
}

/// Frames of the selection frame's zoom: picked one a tick from the
/// smallest as an object is selected (LandObj 0x427268; the frame's delay
/// +0x7d is 0), held at the last.
pub const SELECTION_FRAMES: usize = 10;

/// The selection frame's picture for `obj` (its name in the constructors:
/// `RAM_0<w>X0<h>` from a ship's picture, 0x4e0c80; `RAM_128X128` for
/// settlements and towns, 0x43b000; `RAM_064X078` for ports, shipyards,
/// docks and the barbarians' yard, 0x4e7f50; `RAM_072X072` for other land
/// buildings, 0x427750), frame `zoom`.
fn selection_frame(
    data: &GameData,
    obj: &MapObject,
    w: u32,
    h: u32,
    zoom: usize,
) -> Option<std::rc::Rc<Image>> {
    let base = match obj.class {
        ObjectClass::Ship => format!("RAM_0{w}X0{h}"),
        ObjectClass::Building(0x32 | 0x33) => "RAM_128X128".into(),
        ObjectClass::Building(0x39..=0x3b) | ObjectClass::Barbarian(0x40) => "RAM_064X078".into(),
        ObjectClass::Building(_) if obj.cell.is_some() => "RAM_072X072".into(),
        _ => return None,
    };
    let name = format!("{base}{:02}", zoom.min(SELECTION_FRAMES - 1));
    FRAMES.with(|c| c.borrow_mut().get(data, &name).ok().flatten())
}

/// The selection frame of an object just deselected, shrinking (0x427648:
/// from the last frame but one, a frame a tick, gone at 0), without its
/// hull bar.
#[allow(clippy::too_many_arguments)]
pub fn draw_deselection(
    frame: &mut Frame,
    screen: &Screen,
    data: &GameData,
    scene: &Scene,
    camera: &Camera,
    id: u32,
    time_ms: u64,
    zoom: usize,
    lut: &[u32; 256],
) {
    if let Some(obj) = scene.objects.iter().find(|o| o.id == id) {
        draw_frame_sprite(frame, screen, data, obj, camera, time_ms, zoom, false, lut);
    }
}

/// Draws the selection frame sprite and the hull bar on its top edge;
/// false if the object has none (the drawn corners serve instead). A
/// ship's frame is centred on its picture (the frame is 4 pixels larger),
/// a building's laid at its picture's corner (measured on a settlement).
#[allow(clippy::too_many_arguments)]
fn draw_frame_sprite(
    frame: &mut Frame,
    screen: &Screen,
    data: &GameData,
    obj: &MapObject,
    camera: &Camera,
    time_ms: u64,
    zoom: usize,
    bar: bool,
    lut: &[u32; 256],
) -> bool {
    let Some(first) = obj.frame_at(time_ms).first() else {
        return false;
    };
    let (w, h) = (first.image.width, first.image.height);
    let Some(img) = selection_frame(data, obj, w, h, zoom) else {
        return false;
    };
    let (pl, pt) = (
        obj.x + first.dx as f32 - w as f32 / 2.0,
        obj.y + first.dy as f32 - h as f32 / 2.0,
    );
    let (fw, fh) = (img.width as f32, img.height as f32);
    let (left, top) = match obj.class {
        ObjectClass::Ship => (pl + (w as f32 - fw) / 2.0, pt + (h as f32 - fh) / 2.0),
        _ => (pl, pt),
    };
    draw_image(
        frame,
        screen,
        camera,
        lut,
        &img,
        left.round() as i32,
        top.round() as i32,
    );
    // The hull bar on the top edge: a black frame 4 high, two rows of
    // colour inside (as the panel's slot bars), from 14 in, width - 28.
    let Some(c) = obj.combat.as_ref().filter(|_| bar) else {
        return true;
    };
    let scale = camera.scale(screen);
    let thick = (scale.round() as i64).max(1);
    let ox = screen.x as i64 + ((left.round() - camera.x) * scale) as i64;
    let oy = screen.y as i64 + ((top.round() - camera.y) * scale) as i64;
    let (cx0, cy0) = (screen.x as i64, screen.y as i64);
    let (cx1, cy1) = (cx0 + screen.w as i64, cy0 + screen.h as i64);
    let len = img.width as i64 - 28;
    let fill = ((len - 2) as f32 * (c.hull / c.max_hull).clamp(0.0, 1.0)) as i64;
    let colours = match c.hull / c.max_hull {
        f if f >= 0.5 => [lut[200], lut[199]],
        f if f >= 0.25 => [lut[203], lut[157]],
        _ => [lut[65], lut[68]],
    };
    for y in 0..4 {
        for x in 0..len {
            let inside = (1..3).contains(&y) && (1..=fill).contains(&x);
            let col = if inside { colours[y as usize - 1] } else { 0 };
            let (px, py) = (
                ox + ((14 + x) as f32 * scale) as i64,
                oy + (y as f32 * scale) as i64,
            );
            for dy in 0..thick {
                for dx in 0..thick {
                    let (qx, qy) = (px + dx, py + dy);
                    if qx >= cx0 && qy >= cy0 && qx < cx1 && qy < cy1 {
                        frame.put(qx, qy, col);
                    }
                }
            }
        }
    }
    true
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

pub fn draw_site(
    frame: &mut Frame,
    screen: &Screen,
    camera: &Camera,
    cell: (i32, i32),
    cells: i32,
    ok: bool,
) {
    let scale = camera.scale(screen);
    let t = TILE_SIZE as f32;
    let to_screen = |wx: f32, wy: f32| {
        (
            screen.x as i64 + ((wx - camera.x) * scale) as i64,
            screen.y as i64 + ((wy - camera.y) * scale) as i64,
        )
    };
    let (x0, y0) = to_screen(cell.0 as f32 * t, cell.1 as f32 * t);
    let (x1, y1) = to_screen(
        (cell.0 + cells.max(1)) as f32 * t,
        (cell.1 + cells.max(1)) as f32 * t,
    );
    let colour = if ok { HULL_OK } else { HULL_LOST };
    let thick = (scale.round() as i64).max(1);
    let (cx0, cy0) = (screen.x as i64, screen.y as i64);
    let (cx1, cy1) = (cx0 + screen.w as i64, cy0 + screen.h as i64);
    let mut put = |x: i64, y: i64| {
        if x >= cx0 && x < cx1 && y >= cy0 && y < cy1 {
            frame.put(x, y, colour);
        }
    };
    for k in 0..thick {
        for x in x0..=x1 {
            put(x, y0 + k);
            put(x, y1 - k);
        }
        for y in y0..=y1 {
            put(x0 + k, y);
            put(x1 - k, y);
        }
    }
    // Where it may not go, stripes across: one diagonal every 8 pixels.
    if !ok {
        let step = (8.0 * scale).round().max(2.0) as i64;
        for y in y0..=y1 {
            for x in x0..=x1 {
                if (x - x0 + y - y0).rem_euclid(step) < thick {
                    put(x, y);
                }
            }
        }
    }
}

/// Draws object `obj` alone (a building's preview).
pub fn draw_object(
    frame: &mut Frame,
    screen: &Screen,
    camera: &Camera,
    lut: &[u32; 256],
    obj: &MapObject,
    time_ms: u64,
) {
    for layer in obj.frame_at(time_ms) {
        let img = &layer.image;
        let left = (obj.x + layer.dx as f32 - img.width as f32 / 2.0).round() as i32;
        let top = (obj.y + layer.dy as f32 - img.height as f32 / 2.0).round() as i32;
        draw_image(frame, screen, camera, lut, img, left, top);
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
    if !scene.fogged() {
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

/// Fills a screen rectangle with colour `c`.
pub fn fill_rect(frame: &mut Frame, x: u32, y: u32, w: u32, h: u32, c: u32) {
    for py in y..(y + h).min(frame.height) {
        for px in x..(x + w).min(frame.width) {
            frame.pixels[(py * frame.width + px) as usize] = c;
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

    /// The radar map (Tab) in the view's place `screen`, `zoom` window
    /// pixels a minimap pixel (Mapa.cpp 0x51b8c0, mode 2): 8 pixels a cell,
    /// as many cells as fit (72 x 64 at 800x600, from the view's corner); a
    /// smaller map centred by whole cells, a larger one from cell `origin`.
    pub fn radar(scene: &Scene, screen: &Screen, zoom: f32, origin: (i32, i32)) -> Self {
        let cell = TILE_SIZE / MINIMAP_DIVISOR;
        let (cols, rows) = radar_cells(screen, zoom);
        let map = (
            (scene.minimap_size.0 / cell) as i32,
            (scene.minimap_size.1 / cell) as i32,
        );
        let px = |cells: i32| (cells as f32 * cell as f32 * zoom) as u32;
        let shown = (map.0.min(cols), map.1.min(rows));
        Self {
            x: screen.x + px((cols - shown.0).max(0) / 2),
            y: screen.y + px((rows - shown.1).max(0) / 2),
            w: px(shown.0),
            h: px(shown.1),
            zoom,
            origin: (origin.0.max(0) as u32 * cell, origin.1.max(0) as u32 * cell),
        }
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

/// How many cells the radar map shows in `screen` (`zoom` window pixels a
/// minimap pixel).
pub fn radar_cells(screen: &Screen, zoom: f32) -> (i32, i32) {
    let cell = (TILE_SIZE / MINIMAP_DIVISOR) as f32 * zoom.max(0.01);
    (
        (screen.w as f32 / cell) as i32,
        (screen.h as f32 / cell) as i32,
    )
}

/// The radar map's first cell on an axis after the view moved to cell
/// `view` (Mapa.cpp 0x51c7e0, 0x51b8c0): the least scroll that keeps the
/// view's frame (`frame` cells) inside the `window` cells shown, within a
/// map of `map` cells.
pub fn radar_scroll(origin: i32, view: i32, frame: i32, window: i32, map: i32) -> i32 {
    let mut o = origin.min(view);
    if o + window < view + frame {
        o = view + frame - window;
    }
    o.clamp(0, (map - window).max(0))
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
        // Another player's ship with the phantom has no icon: 0x4df380
        // takes it off for layer 0 (0x50de40, message 0xd21) and puts it
        // back at the end (0x50da60, 0xd20).
        for obj in scene
            .objects
            .iter()
            .filter(|o| (!moves(o) || scene.is_visible(o)) && !scene.phantom_hidden(o))
        {
            // A fallen building keeps its icon, in its owner's colours,
            // until it is removed (message 3 sends itself 0xd21 at
            // 0x42686a; nothing at the fall).
            let Some((icon, large)) = minimap_icon(obj, scene.kept_owner(obj)) else {
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
            if let (true, Some(art)) = (scene.fogged(), art) {
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
/// large) as the original picks it (0x50da60, 0x51cdd0), `owner` the
/// object's (a fallen building's kept one).
fn minimap_icon(obj: &MapObject, owner: Option<(u32, u32)>) -> Option<(u32, bool)> {
    if obj.asleep.is_some() {
        return None;
    }
    if let Some((nation, slot)) =
        owner.filter(|_| matches!(obj.class, ObjectClass::Ship | ObjectClass::Building(_)))
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

/// Darkens a pixel to 3/4 brightness (the interface's shaded text
/// backgrounds and scaled pictures; the map's shadows go through
/// [`SHADOW_TABLE`]).
#[inline]
pub fn darken(px: u32) -> u32 {
    let r = (px >> 16 & 0xff) * 3 / 4;
    let g = (px >> 8 & 0xff) * 3 / 4;
    let b = (px & 0xff) * 3 / 4;
    r << 16 | g << 8 | b
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ghost table as 0x519280 fills it (the loop's code as the exe
    /// has it: rows 0-0x7f, 0x10 bytes a row, `(-(row & 1) & 0xab) +
    /// 0xaa` at 0x5cdd2c + row x 16 + byte), and where it shows a
    /// picture's pixels: a 72-wide picture's rows of 9 bytes run across
    /// the table's rows of 16.
    #[test]
    fn ghost_table_is_the_exes() {
        if let Some(root) = std::env::var_os("OPENCONQUEST_DATA") {
            let path = crate::assets::find_ci(std::path::Path::new(&root), "ancient.exe")
                .unwrap()
                .unwrap();
            let exe = std::fs::read(path).unwrap();
            let code = |va: usize, n: usize| exe[va - 0x40_0000..][..n].to_vec();
            assert_eq!(code(0x5193ed, 6), [0x81, 0xf9, 0x80, 0, 0, 0]);
            assert_eq!(code(0x5193fd, 3), [0x83, 0xf8, 0x10]);
            assert_eq!(
                code(0x519411, 13),
                [0xf7, 0xda, 0x1b, 0xd2, 0x83, 0xe2, 0xab, 0x81, 0xc2, 0xaa, 0, 0, 0]
            );
            assert_eq!(
                code(0x519420, 10),
                [0xc1, 0xe6, 0x04, 0x88, 0x94, 0x06, 0x2c, 0xdd, 0x5c, 0x00]
            );
        }
        for row in 0..0x80u32 {
            let v = ((row & 1).wrapping_neg() & 0xffff_ffab).wrapping_add(0xaa) as u8;
            assert!(GHOST_TABLE[row as usize * 16..][..16]
                .iter()
                .all(|&b| b == v));
        }
        // 64 wide: two picture rows to a table row, even pixels on rows 0
        // and 1, odd ones on rows 2 and 3.
        assert!(ghost_shows(64, 0, 0) && !ghost_shows(64, 1, 0));
        assert!(ghost_shows(64, 62, 1) && !ghost_shows(64, 63, 1));
        assert!(!ghost_shows(64, 0, 2) && ghost_shows(64, 1, 3));
        // 72 wide, row 1: bytes 9-17, the table's row 1 from byte 16.
        assert!(ghost_shows(72, 0, 1) && ghost_shows(72, 54, 1));
        assert!(!ghost_shows(72, 56, 1) && ghost_shows(72, 57, 1));
        // The largest ship picture, 80 square, ends at byte 799.
        assert!(ghost_shows(80, 79, 79) == (GHOST_TABLE[799] & 1 != 0));
    }

    /// A ship with the phantom on miss000: drawn through the ghost table
    /// for its owner (its shadow whole, the rest only where the table
    /// shows), nothing of it for another player, and no radar icon.
    #[test]
    fn a_phantom_ship_is_ghosted_for_its_owner_and_hidden_from_others() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = crate::objects::Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let i = scene
            .objects
            .iter()
            .position(|o| o.ship.is_some() && scene.is_own(o))
            .expect("a ship of the player's");
        let ship = scene.objects.swap_remove(i);
        scene.objects.clear();
        scene.objects.push(ship);
        scene.gulls.clear();
        scene.droplets.clear();
        let o = &scene.objects[0];
        let layers: Vec<_> = scene.frame_of(o, 0).to_vec();
        let (left, top) = layer_origin(o, &layers[0]);
        let (w, h) = (layers[0].image.width, layers[0].image.height);
        let screen = Screen::full(w + 20, h + 20);
        let mut camera = Camera::new(crate::view::ScalePolicy {
            fixed: Some(1.0),
            integer: true,
        });
        camera.x = (left - 10) as f32;
        camera.y = (top - 10) as f32;
        let lut = palette_lut(&data, 0);
        let draw = |scene: &Scene| {
            const BACK: u32 = 0x0012_3456;
            let mut px = vec![BACK; ((w + 20) * (h + 20)) as usize];
            let mut frame = Frame {
                pixels: &mut px,
                width: w + 20,
                height: h + 20,
            };
            draw_objects(&mut frame, &screen, scene, &camera, &lut, 0);
            (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .filter(|&(x, y)| px[((y + 10) * (w + 20) + x + 10) as usize] != BACK)
                .collect::<Vec<_>>()
        };
        let whole = draw(&scene);
        assert!(!whole.is_empty());
        scene.objects[0].combat.as_mut().unwrap().phantom = 30.0;
        let ghost = draw(&scene);
        assert!(!ghost.is_empty() && ghost.len() < whole.len());
        let shadow = |x: u32, y: u32| {
            layers
                .iter()
                .any(|l| l.image.pixels[(y * w + x) as usize] == Some(SHADOW))
        };
        for &(x, y) in &whole {
            assert_eq!(
                ghost.contains(&(x, y)),
                shadow(x, y) || ghost_shows(w, x, y),
                "({x}, {y})"
            );
        }
        // Another player's: nothing drawn, and off the radar.
        let mut other = scene.human.unwrap();
        other.slot = (other.slot + 1) % 4;
        scene.human = Some(other);
        assert!(scene.phantom_hidden(&scene.objects[0]));
        assert!(draw(&scene).is_empty());
        scene.objects[0].combat.as_mut().unwrap().phantom = 0.0;
        assert!(!scene.phantom_hidden(&scene.objects[0]));
    }

    #[test]
    fn radar_fits_the_view_in_cells() {
        // 800x600: the view is 576 x 512, 72 x 64 cells at 8 pixels.
        let view = Screen {
            x: 8,
            y: 8,
            w: 576,
            h: 512,
            window_h: 600,
        };
        assert_eq!(radar_cells(&view, 1.0), (72, 64));
        assert_eq!(radar_cells(&view, 2.0), (36, 32));
    }

    #[test]
    fn radar_scrolls_to_keep_the_frame() {
        // A map that fits does not scroll.
        assert_eq!(radar_scroll(0, 15, 9, 72, 24), 0);
        // A larger one: the least scroll keeping the 9-cell frame inside.
        assert_eq!(radar_scroll(0, 60, 9, 72, 128), 0);
        assert_eq!(radar_scroll(0, 70, 9, 72, 128), 7);
        assert_eq!(radar_scroll(7, 3, 9, 72, 128), 3);
        assert_eq!(radar_scroll(40, 119, 9, 72, 128), 56);
    }

    /// A building with the phantom on miss000: its picture drawn through
    /// the ghost table under a mask 0x80 wide for its owner (LandObj
    /// 0x425990 at 0x4260a0), its banner ghosted too; nothing of either
    /// for another player (0x50e350), off the radar, yet still picked by
    /// the pointer (0x49a580: the building's vt+8 0x423460 and the ship's
    /// 0x4e06c0 test no hidden flag). Every building picture is a single
    /// kind-0 sprite of at most 128 x 128 at its top left.
    #[test]
    fn a_phantom_building_is_ghosted_for_its_owner_and_hidden_from_others() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = crate::objects::Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        for o in &scene.objects {
            if let openconquest_formats::ObjectClass::Building(_) = o.class {
                for f in &o.frames {
                    assert_eq!(f.len(), 1);
                    assert!(f[0].image.width <= 128 && f[0].image.height <= 128);
                    assert_eq!((f[0].dx, f[0].dy), (0, 0));
                }
            }
        }
        let i = scene
            .objects
            .iter()
            .position(|o| {
                matches!(o.class, openconquest_formats::ObjectClass::Building(_))
                    && scene.is_own(o)
                    && o.banner.is_some()
            })
            .expect("a building of the player's with a banner");
        let building = scene.objects.swap_remove(i);
        let ship = scene
            .objects
            .iter()
            .position(|o| o.ship.is_some() && scene.is_own(o))
            .map(|k| scene.objects.swap_remove(k))
            .expect("a ship of the player's");
        scene.objects.clear();
        scene.objects.push(building);
        scene.gulls.clear();
        scene.droplets.clear();
        scene.fog = false;
        let banner = scene.objects[0].banner.take();
        let o = &scene.objects[0];
        let layers: Vec<_> = scene.frame_of(o, 0).to_vec();
        let (left, top) = layer_origin(o, &layers[0]);
        let (w, h) = (layers[0].image.width, layers[0].image.height);
        let screen = Screen::full(w + 20, h + 20);
        let mut camera = Camera::new(crate::view::ScalePolicy {
            fixed: Some(1.0),
            integer: true,
        });
        camera.x = (left - 10) as f32;
        camera.y = (top - 10) as f32;
        let lut = palette_lut(&data, 0);
        let draw = |scene: &Scene| {
            const BACK: u32 = 0x0012_3456;
            let mut px = vec![BACK; ((w + 20) * (h + 20)) as usize];
            let mut frame = Frame {
                pixels: &mut px,
                width: w + 20,
                height: h + 20,
            };
            draw_objects(&mut frame, &screen, scene, &camera, &lut, 0);
            (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .filter(|&(x, y)| px[((y + 10) * (w + 20) + x + 10) as usize] != BACK)
                .collect::<Vec<_>>()
        };
        let whole = draw(&scene);
        assert!(!whole.is_empty());
        scene.objects[0].combat.as_mut().unwrap().phantom = 30.0;
        let ghost = draw(&scene);
        assert!(!ghost.is_empty() && ghost.len() < whole.len());
        let shadow = |x: u32, y: u32| layers[0].image.pixels[(y * w + x) as usize] == Some(SHADOW);
        for &(x, y) in &whole {
            assert_eq!(
                ghost.contains(&(x, y)),
                shadow(x, y) || ghost_shows(0x80, x, y),
                "({x}, {y})"
            );
        }
        // Its banner through the mask from the banner's own top left.
        let (flags, dx, dy) = banner.clone().unwrap();
        let flag = &flags[0];
        scene.objects[0].banner = banner;
        let flagged = draw(&scene);
        let mut shown = 0;
        for fy in 0..flag.height {
            for fx in 0..flag.width {
                let (x, y) = (fx as i32 + dx, fy as i32 + dy);
                let solid = flag.pixels[(fy * flag.width + fx) as usize].is_some();
                if !solid || x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                    continue;
                }
                let (x, y) = (x as u32, y as u32);
                let drawn = flagged.contains(&(x, y));
                shown += usize::from(drawn);
                if !ghost.contains(&(x, y)) {
                    assert_eq!(drawn, ghost_shows(0x80, fx, fy), "flag ({fx}, {fy})");
                }
            }
        }
        assert!(shown > 0);
        // Another player's: nothing drawn, off the radar, still picked.
        let (bx, by) = whole[whole.len() / 2];
        let (bx, by) = ((left + bx as i32) as f32, (top + by as i32) as f32);
        let id = scene.objects[0].id;
        let mut other = scene.human.unwrap();
        other.slot = (other.slot + 1) % 4;
        scene.human = Some(other);
        assert!(scene.phantom_hidden(&scene.objects[0]));
        assert!(draw(&scene).is_empty());
        assert_eq!(hit_test(&scene, bx, by, 0), Some(id));
        // A hidden phantom ship likewise.
        scene.objects.clear();
        scene.objects.push(ship);
        scene.objects[0].combat.as_mut().unwrap().phantom = 30.0;
        let o = &scene.objects[0];
        assert!(scene.phantom_hidden(o));
        let l = &scene.frame_of(o, 0)[0];
        let (sl, st) = layer_origin(o, l);
        let k = l
            .image
            .pixels
            .iter()
            .position(|p| p.is_some_and(|p| p != SHADOW))
            .unwrap() as u32;
        let (sx, sy) = (
            sl + (k % l.image.width) as i32,
            st + (k / l.image.width) as i32,
        );
        assert_eq!(hit_test(&scene, sx as f32, sy as f32, 0), Some(o.id));
    }
}
