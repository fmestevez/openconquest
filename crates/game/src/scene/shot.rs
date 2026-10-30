//! Projectiles (BombClassTy, bomb.cpp): each weapon fired sends one off,
//! drawn as `BOMB<letter>0<variant>` (a composite of 16 cells a frame, one
//! per direction: 0 east, counter-clockwise; 0x498a50, 0x497e50), the
//! letter the item's (A for item 0) and variant 1 for a flaming shot. It
//! flies straight or in an arc (0x498030) and, where it lands, plays
//! `BOMB<letter><state><variant>` a frame a tick: state 1 on a ship, 2 in
//! the water, 3 on a building, 4 on the ground, 5 on a sea creature and 6
//! on a land creature or flyer (0x4967b0, 0x4983d0). The blow lands with
//! it.

use super::*;
use crate::objects::Layer;

/// Letters that fly straight (0x498030); the others arc.
const STRAIGHT: [bool; 19] = [
    true, true, false, true, true, false, false, false, false, false, true, true, false, false,
    true, true, false, false, false,
];
/// A straight shot's speed and step (0x59757c, 0x599b08): it covers
/// `speed x step / 10` pixels a tick.
const SPEED: [u32; 19] = [
    50, 40, 45, 45, 45, 35, 35, 35, 35, 25, 25, 25, 45, 45, 45, 45, 45, 45, 0,
];
const STEP: [u32; 19] = [8, 8, 9, 8, 7, 7, 7, 7, 7, 7, 4, 5, 5, 5, 3, 5, 5, 5, 5];
/// An arcing shot's step (0x599b1c) along its flight's length, by
/// distance in tens of pixels from 30 (0x598dd8, its last column).
const ARC_STEP: [u32; 19] = [
    22, 11, 19, 22, 19, 19, 13, 16, 15, 19, 13, 13, 11, 11, 13, 2, 2, 2, 0,
];
const ARC_LENGTH: [u32; 48] = [
    66, 95, 106, 116, 125, 133, 141, 148, 155, 161, 167, 173, 179, 184, 189, 194, 199, 203, 208,
    212, 216, 220, 224, 227, 231, 234, 238, 241, 244, 247, 250, 253, 258, 258, 261, 264, 266, 268,
    272, 275, 278, 281, 285, 288, 291, 294, 297, 300,
];
/// How high an arc rises on screen (0x5baf74); the ballistics of 0x497250
/// put the top at about 0.41 of the distance for 9000.
const HEIGHT: [u32; 19] = [
    2000, 3000, 2000, 2000, 2000, 4000, 9000, 4700, 5000, 5500, 4000, 5000, 5400, 4500, 5000, 5500,
    4000, 5500, 5500,
];

/// Sounds by letter: launching (events at 0x5bbca4) and landing, by state
/// 1-4 (events at 0x5bbcec), as `SND_n` lists them.
pub(super) const LAUNCH: [&str; 14] = [
    "bas031", "cross1", "thrcat1", "bas034", "gastsn1", "thrcat1", "thrbal1", "thrcat1", "thrbal1",
    "cross1", "", "", "fir1", "thrcat1",
];
pub(super) const LAND: [[&[&str]; 4]; 14] = [
    [
        &["bowla1", "strel1"],
        &["aas005"],
        &["strel1", "strel1", "bowla1"],
        &["bowlan"],
    ],
    [
        &["strel1", "bowla1"],
        &["aas005"],
        &["strel1", "bowla1"],
        &["bowlan"],
    ],
    [&["htk1"], &["splash1"], &["htk1"], &["htk1"]],
    [
        &["strel1", "bowla1"],
        &["aas005"],
        &["bowla1", "strel1", "bas007"],
        &["bowlan"],
    ],
    [
        &["strel1", "bowla1"],
        &["aas005"],
        &["gashilo", "bowla1"],
        &["tlock8"],
    ],
    [&["htk1"], &["splash1"], &["hlc3"], &["htk1"]],
    [
        &["htk1"],
        &["splash1"],
        &["hlc2", "hilac1", "hitc2"],
        &["hitc2"],
    ],
    [
        &["htk1", "htk3"],
        &["splash1"],
        &["hlc1", "hlc3", "hlc3", "hlc3", "hlc3"],
        &["hlc3"],
    ],
    [&["htk1"], &["splash1"], &["hlc3", "htk1"], &["htk1"]],
    [&["strel1"], &["aas005"], &[], &[]],
    [&["fifi1"], &["aas005"], &["fifi1"], &["fifi1"]],
    [&["strel1"], &[], &["strel1"], &[]],
    [&["fifi1"], &["aas005"], &["fifi1"], &["fifi1"]],
    [&["quick1"], &["splash1"], &["quick1"], &["quick1"]],
];

/// A projectile in flight.
pub struct Shot {
    pub letter: u8,
    /// What was fired, and with the Flame (it may set ships on fire).
    item: u8,
    flaming: bool,
    pub variant: u8,
    from: (f32, f32),
    to: (f32, f32),
    /// What it was fired at, and whether it strikes it.
    target: u32,
    strikes: bool,
    /// The blow to the hull and to the crew it carries.
    damage: f32,
    wounds: f32,
    /// The weapons' part, in the units' points.
    wear: f32,
    /// Seconds flown, and how long the flight lasts.
    age: f32,
    flight: f32,
    /// The arc's height at its top (0 flying straight).
    apex: f32,
    /// Its picture by frame: the cell for its direction.
    frames: Vec<Layer>,
    /// Where it is drawn now.
    pub x: f32,
    pub y: f32,
    /// A grappling hook's line back to the ship that threw it (bomb slot
    /// 2, a line in palette colour 0x6d, 0x496f40, 0x4964c0).
    pub rope: Option<u32>,
    /// Who fired it.
    pub shooter: Option<u32>,
    /// The player it is put down to (`+0xb5`, from message 0x14ba's `+4`
    /// when it is fired): the shooter's, or none (0xff).
    pub by: Option<Owner>,
    /// Thrown by a volcano or a fire ship (0x499410): no shooter, it
    /// strikes whatever is where it lands, its pictures played once.
    pub volcanic: bool,
    /// Its burning trail is on (+0xe6; always but for a volcano's stones,
    /// one in five).
    pub trail: bool,
}

impl Shot {
    /// Its picture now.
    pub fn layer(&self) -> Option<&Layer> {
        if self.frames.is_empty() {
            return None;
        }
        let tick = (self.age / crate::clock::TICK) as usize;
        let k = if self.volcanic {
            tick.min(self.frames.len() - 1)
        } else {
            tick % self.frames.len()
        };
        Some(&self.frames[k])
    }
}

/// The arc's height by distance for a volcano's stones (0x599088, by
/// clamp(d, 30, 350) / 10 - 3).
const VOLCANIC_ARC: [f32; 33] = [
    220.0, 200.0, 160.0, 140.0, 120.0, 100.0, 85.0, 70.0, 50.0, 45.0, 40.0, 36.0, 34.0, 33.0, 30.0,
    28.0, 26.0, 24.0, 22.0, 21.0, 20.0, 19.0, 18.0, 17.0, 16.0, 15.0, 14.0, 13.0, 12.0, 11.0, 10.0,
    10.0, 10.0,
];

/// A volcanic shot's letter: its blow at level 0 (A, ours a fifteenth of
/// the original's: 0x5c75cc from 0x597d2c), its crew's share by B over A,
/// and its arc's rise (0x5baf28, in thousandths).
fn volcanic(letter: u8) -> (f32, f32, f32) {
    match letter {
        16 => (1220.0, 600.0 / 1220.0, 0.55),
        17 => (1420.0, 1120.0 / 1420.0, 0.45),
        18 => (1550.0, 1180.0 / 1550.0, 0.35),
        6 => (combat::hull_damage_at(6, 0), combat::crew_scale(6), 0.36),
        _ => (combat::hull_damage_at(5, 0), combat::crew_scale(5), 0.54),
    }
}

/// The fireball's shots fly as K, flaming (slight.cpp 0x405c20).
pub const FIREBALL: u8 = 255;
/// Grappling hooks fly as J (bomb kind 9, 0x4b84b0).
pub const HOOK: u8 = 254;

/// The letter and variant for item `item` (0x4c0230: Greek fire flies as
/// M, flaming; quick lime as N).
fn letter_of(item: u8, flaming: bool) -> (u8, u8) {
    match item {
        FIREBALL => (10, 1),
        HOOK => (9, 0),
        combat::GREEK_FIRE => (12, 1),
        combat::QUICKLIME => (13, 0),
        _ => (item.min(8), u8::from(flaming)),
    }
}

fn picture(letter: u8, state: u8, variant: u8) -> String {
    format!("BOMB{}{}{}", (b'A' + letter) as char, state.min(6), variant)
}

/// A shot's picture in flight: its direction's cell of each frame.
fn flight_frames(
    letter: u8,
    variant: u8,
    dir: usize,
    data: &GameData,
    caches: &mut Caches,
) -> Vec<Layer> {
    let cells = |name: &str, caches: &mut Caches| -> Vec<Layer> {
        objects::composite_layer_frames(data, caches, name, dir)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|f| f.into_iter().next())
            .collect()
    };
    let frames = cells(&picture(letter, 0, variant), caches);
    if frames.is_empty() {
        cells(&picture(letter, 0, 1 - variant), caches)
    } else {
        frames
    }
}

/// A shot in flight as a save keeps it (its picture is found again).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedShot {
    letter: u8,
    variant: u8,
    from: (f32, f32),
    to: (f32, f32),
    target: u32,
    strikes: bool,
    damage: f32,
    wounds: f32,
    #[serde(default)]
    wear: f32,
    #[serde(default)]
    item: u8,
    #[serde(default)]
    flaming: bool,
    age: f32,
    flight: f32,
    apex: f32,
    #[serde(default)]
    rope: Option<u32>,
    #[serde(default)]
    shooter: Option<u32>,
    #[serde(default)]
    by: Option<Owner>,
    #[serde(default)]
    volcanic: bool,
    #[serde(default = "yes")]
    trail: bool,
}

fn yes() -> bool {
    true
}

impl Shot {
    pub fn saved(&self) -> SavedShot {
        SavedShot {
            letter: self.letter,
            variant: self.variant,
            from: self.from,
            to: self.to,
            target: self.target,
            strikes: self.strikes,
            damage: self.damage,
            wounds: self.wounds,
            wear: self.wear,
            item: self.item,
            flaming: self.flaming,
            age: self.age,
            flight: self.flight,
            apex: self.apex,
            rope: self.rope,
            shooter: self.shooter,
            by: self.by,
            volcanic: self.volcanic,
            trail: self.trail,
        }
    }

    pub fn restored(s: &SavedShot, data: &GameData, caches: &mut Caches) -> Self {
        let dir = direction(s.to.0 - s.from.0, s.to.1 - s.from.1);
        Shot {
            letter: s.letter,
            item: s.item,
            flaming: s.flaming,
            variant: s.variant,
            from: s.from,
            to: s.to,
            target: s.target,
            strikes: s.strikes,
            damage: s.damage,
            wounds: s.wounds,
            wear: s.wear,
            age: s.age,
            flight: s.flight,
            apex: s.apex,
            frames: flight_frames(s.letter, s.variant, dir, data, caches),
            x: s.from.0,
            y: s.from.1,
            rope: s.rope,
            shooter: s.shooter,
            by: s.by,
            volcanic: s.volcanic,
            trail: s.trail,
        }
    }
}

impl Scene {
    /// What a volcanic shot strikes where it lands: a ship on the cell,
    /// else a building standing there; its blow shared out as any.
    fn aim_volcanic(&self, s: &mut Shot) {
        let t = TILE_SIZE as f32;
        let cell = ((s.to.0 / t).floor() as i32, (s.to.1 / t).floor() as i32);
        let cell_of = |o: &objects::MapObject| ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let hit = self
            .objects
            .iter()
            .find(|o| o.ship.is_some() && o.combat.is_some() && cell_of(o) == cell)
            .or_else(|| {
                self.objects.iter().find(|o| {
                    o.combat.is_some()
                        && o.cell.is_some_and(|(x, y)| {
                            let n = super::trade::building_cells(o);
                            (x..x + n).contains(&cell.0) && (y..y + n).contains(&cell.1)
                        })
                })
            });
        let Some(o) = hit else {
            s.target = 0;
            s.strikes = false;
            return;
        };
        let (blow, crew, _) = volcanic(s.letter);
        s.target = o.id;
        s.strikes = true;
        match (&o.ship, &o.combat) {
            (Some(ship), Some(c)) => {
                let blow = blow * combat::armour_factor(ship.armour, &self.levels_of(o.owner));
                let sh = combat::Shares::of(ship, &c.weapons);
                s.damage = sh.hull_part(blow);
                s.wounds = sh.crew_part(blow) * 15.0 * crew;
                s.wear = sh.weapons_part(blow) * 15.0;
            }
            _ => s.damage = blow,
        }
    }
}

impl Scene {
    /// Where a shot at ship `t` aims (its vt+0xc, 0x4e07c0), by the hit
    /// byte: drawing from the ship's own generator, from its picture's top
    /// left (`+0x8be` `+0x59` / `+0x5d`, size `+0x61` / `+0x65`; here as the
    /// port places it, `render::layer_origin`). A hit goes to its type's and
    /// heading's spot on the hull (0x598ad8) moved `r % 20 - 9` along its
    /// heading (0x5c4f4c), led by its way (`+0x59b` x the heading x 15 /
    /// 100000) under way, braking or told to stop (`+0x58c` 5, 4, 1); a
    /// miss to a point by the picture's edge: `u = r % 6` and a second
    /// draw's bit 16 picking the side, by heading (0 / 4: `x = w u / 5`, `y
    /// = h / 4` or `h - h / 6`; 1 / 5: `(w / 2) u / 5 + w / 2` and `h - (h /
    /// 2) u / 5`, or `(w / 2) u / 5` and `h / 2 - (h / 2) u / 5`; 2 / 6: `w
    /// - w / 8` or `w / 8`, and `w u / 5`, the width, as written; 3 / 7:
    /// `(w / 2) u / 5 + w / 2` and `(h / 2) u / 5`, or `(w / 2) u / 5` and
    /// `(h / 2) u / 5 + h / 2`).
    pub(super) fn ship_aim(&mut self, t: usize, hit: bool) -> (i32, i32) {
        let o = &self.objects[t];
        let Some(ship) = o.ship.as_ref() else {
            return (o.x as i32, o.y as i32);
        };
        let (left, top, w, h) = match o.frames.first().and_then(|f| f.first()) {
            Some(l) => {
                let (x, y) = crate::render::layer_origin(o, l);
                (x, y, l.image.width as i32, l.image.height as i32)
            }
            None => (o.x as i32, o.y as i32, 0, 0),
        };
        let d = ship.heading % 8;
        let (kind, state) = (ship.kind as usize, ship.move_state);
        // `+0x59b`, in 1/6000 pixel a tick (see `slipping`).
        let way = (ship.velocity * crate::clock::TICK * 6000.0) as i64;
        let r = self.ship_draw(t);
        if hit {
            let (vx, vy) = super::walk_path::DIRS[d];
            let k = (r % 20) as i32 - 9;
            let (sx, sy) = super::fire::SPOT[kind.min(11)][d];
            let (mut x, mut y) = (vx * k + sx + left, vy * k + sy + top);
            if way != 0 && matches!(state, 5 | 4 | 1) {
                x += (way * vx as i64 * 15 / 100000) as i32;
                y += (way * vy as i64 * 15 / 100000) as i32;
            }
            return (x, y);
        }
        let u = (r % 6) as i32;
        let side = self.ship_draw(t) & 1 != 0;
        let (x, y) = match (d % 4, side) {
            (0, false) => (w * u / 5, h / 4),
            (0, true) => (w * u / 5, h - h / 6),
            (1, false) => ((w / 2) * u / 5 + w / 2, h - (h / 2) * u / 5),
            (1, true) => ((w / 2) * u / 5, h / 2 - (h / 2) * u / 5),
            (2, false) => (w - w / 8, w * u / 5),
            (2, true) => (w / 8, w * u / 5),
            (_, true) => ((w / 2) * u / 5 + w / 2, (h / 2) * u / 5),
            (_, false) => ((w / 2) * u / 5, (h / 2) * u / 5 + h / 2),
        };
        (left + x, top + y)
    }
}

/// The 16 directions (0x497e50): 0 east, counter-clockwise.
fn direction(dx: f32, dy: f32) -> usize {
    let a = (-dy).atan2(dx).rem_euclid(std::f32::consts::TAU);
    ((a / (std::f32::consts::TAU / 16.0)).round() as usize) % 16
}

/// A shot about to be fired.
pub(super) struct Firing {
    pub item: u8,
    /// The picture's letter, when not the item's (creatures' throws).
    pub letter: Option<u8>,
    pub flaming: bool,
    pub from: (f32, f32),
    pub target: u32,
    pub strikes: bool,
    pub damage: f32,
    pub wounds: f32,
    pub wear: f32,
    /// The ship a hook's line runs back to.
    pub rope: Option<u32>,
    /// Who fired it (a sea monster remembers the ship that struck it).
    pub shooter: Option<u32>,
    /// The player the shot is put down to (see `Shot::by`).
    pub by: Option<Owner>,
    /// The point it aims at, when the original's is known (the target's
    /// vt+0xc: a land creature's throw at a ship, `Scene::ship_aim`).
    pub to: Option<(f32, f32)>,
}

impl Scene {
    /// Sends shot `f` off at its target, landing on it if it strikes and
    /// beside it if not, or at the point it aims at.
    pub(super) fn fire_shot(&mut self, f: Firing, data: &GameData, caches: &mut Caches) {
        let Some(t) = self.object_index(f.target) else {
            return;
        };
        let (mut tx, mut ty) = f.to.unwrap_or((self.objects[t].x, self.objects[t].y));
        if !f.strikes && f.to.is_none() {
            let r = &mut self.gull_rng;
            let off = |r: &mut Rng| {
                let v = (r.next() % 33) as f32 + 16.0;
                if r.next() & 1 == 0 {
                    v
                } else {
                    -v
                }
            };
            tx += off(r);
            ty += off(r);
        }
        let (letter, variant) = match f.letter {
            Some(l) => (l, 0),
            None => letter_of(f.item, f.flaming),
        };
        let l = letter as usize;
        let (dx, dy) = (tx - f.from.0, ty - f.from.1);
        let dist = dx.hypot(dy);
        let dir = direction(dx, dy);
        let (flight, apex) = if STRAIGHT[l] {
            let per_tick = (SPEED[l] * STEP[l]) as f32 / 10.0;
            (crate::clock::TICK * (dist / per_tick).max(1.0), 0.0)
        } else {
            let bucket = ((dist.clamp(30.0, 500.0) / 10.0) as usize)
                .saturating_sub(3)
                .min(47);
            let ticks = ARC_LENGTH[bucket] as f32 / ARC_STEP[l].max(1) as f32;
            (
                crate::clock::TICK * ticks.max(1.0),
                0.41 * dist.min(500.0) * HEIGHT[l] as f32 / 9000.0,
            )
        };
        let frames = flight_frames(letter, variant, dir, data, caches);
        if !LAUNCH[l.min(13)].is_empty() {
            self.sounds.push((LAUNCH[l.min(13)], f.from.0, f.from.1));
        }
        self.shots.push(Shot {
            letter,
            item: f.item,
            flaming: f.flaming,
            variant,
            from: f.from,
            to: (tx, ty),
            target: f.target,
            strikes: f.strikes,
            damage: f.damage,
            wounds: f.wounds,
            wear: f.wear,
            age: 0.0,
            flight,
            apex,
            frames,
            x: f.from.0,
            y: f.from.1,
            rope: f.rope,
            shooter: f.shooter,
            by: f.by,
            volcanic: false,
            trail: true,
        });
    }

    /// An ownerless shot of `letter` (0x499410: a volcano's stones, a fire
    /// ship's blasts) from `from` to the point `to`: it always lands there,
    /// 25 ticks on whatever the distance, on the volcano's arc (0x497be0,
    /// 0x4970e0), and strikes what it lands on.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fire_volcanic(
        &mut self,
        letter: u8,
        flaming: bool,
        from: (f32, f32),
        to: (f32, f32),
        trail: bool,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let d = dx.hypot(dy);
        let p = VOLCANIC_ARC[((d.clamp(30.0, 350.0) / 10.0) as usize - 3).min(32)];
        let apex = volcanic(letter).2 * p * d * d / 4000.0;
        let variant = u8::from(flaming);
        let mut frames = flight_frames(letter, variant, direction(dx, dy), data, caches);
        if frames.is_empty() {
            frames = objects::sequence_frames(data, caches, &picture(letter, 0, variant))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|f| f.into_iter().next())
                .collect();
        }
        self.shots.push(Shot {
            letter,
            item: letter,
            flaming,
            variant,
            from,
            to,
            target: 0,
            strikes: true,
            damage: 0.0,
            wounds: 0.0,
            wear: 0.0,
            age: 0.0,
            flight: crate::clock::ticks(25),
            apex,
            frames,
            x: from.0,
            y: from.1,
            rope: None,
            shooter: None,
            by: None,
            volcanic: true,
            trail,
        });
    }

    /// Moves the shots on; those that land strike or splash.
    pub(super) fn run_shots(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let mut landed = Vec::new();
        for (k, s) in self.shots.iter_mut().enumerate() {
            s.age += dt;
            let f = (s.age / s.flight).min(1.0);
            s.x = s.from.0 + (s.to.0 - s.from.0) * f;
            s.y = s.from.1 + (s.to.1 - s.from.1) * f - s.apex * 4.0 * f * (1.0 - f);
            if f >= 1.0 {
                landed.push(k);
            }
        }
        self.throw_trails(dt, data, caches);
        for k in landed.into_iter().rev() {
            let s = self.shots.swap_remove(k);
            self.land_shot(s, data, caches);
        }
    }

    /// With graphic trails, projectiles leave a particle a tick (0x4974d0,
    /// in flight): Greek fire (letter 12) kind 8 and quick lime (13) kind
    /// 1 where they are; flaming shots of letters 16, 17 and 18 kinds 7, 8
    /// and 9 drifting a pixel a tick down and right; each from the
    /// picture's top left plus 0x5975c8 by letter (the same in every
    /// direction).
    fn throw_trails(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.trail_clock += dt;
        let tick = crate::clock::TICK;
        while self.trail_clock >= tick {
            self.trail_clock -= tick;
            if !self.trails {
                continue;
            }
            let mut new = Vec::new();
            for s in &self.shots {
                let (kind, off, v) = match (s.letter, s.flaming) {
                    (12, _) => (8, 9.0, 0.0),
                    (13, _) => (1, 14.0, 0.0),
                    (16..=18, true) if !s.trail => continue,
                    (16, true) => (7, 6.0, 10.0),
                    (17, true) => (8, 8.0, 10.0),
                    (18, true) => (9, 10.0, 10.0),
                    _ => continue,
                };
                let Some(l) = s.layer() else { continue };
                let (left, top) = (
                    s.x - l.image.width as f32 / 2.0,
                    s.y - l.image.height as f32 / 2.0,
                );
                new.push(super::wake::Droplet::trail(
                    kind,
                    left + off,
                    top + off,
                    (v, v),
                ));
            }
            for d in &new {
                let kind = d.trail.unwrap_or(0);
                self.trail_art.entry(kind).or_insert_with(|| {
                    objects::sequence_frames(data, caches, &format!("TRAKS{kind:02}"))
                        .unwrap_or_default()
                });
            }
            self.droplets.extend(new);
        }
    }

    fn land_shot(&mut self, mut s: Shot, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        if s.volcanic {
            self.aim_volcanic(&mut s);
        }
        // A sea monster under water or dying has no picture to strike
        // (0x492ec0).
        let target = self
            .object_index(s.target)
            .filter(|_| s.strikes && self.sea_monster_hittable(s.target));
        if target.is_some() && self.sea_monster_states.contains_key(&s.target) {
            self.sea_monster_struck(s.target, s.shooter);
        }
        let state = match target.map(|i| self.objects[i].class) {
            Some(ObjectClass::Ship) => 1,
            Some(ObjectClass::Monster(0x1f4..=0x1f6 | 0x1f8 | 0x1f9)) => 6,
            Some(ObjectClass::Monster(_) | ObjectClass::WhaleZone) => 5,
            Some(_) => 3,
            None if self
                .nav
                .is_water((s.to.0 / t).floor() as i32, (s.to.1 / t).floor() as i32) =>
            {
                2
            }
            None => 4,
        };
        let name = picture(s.letter, state, s.variant);
        let other = picture(s.letter, state, 1 - s.variant);
        let frames = objects::sequence_frames(data, caches, &name)
            .ok()
            .filter(|f| !f.is_empty())
            .or_else(|| objects::sequence_frames(data, caches, &other).ok())
            .unwrap_or_default();
        if !frames.is_empty() {
            self.effects.push(Effect {
                x: s.to.0,
                y: s.to.1,
                frames,
                age_ms: 0.0,
                follow: target.map(|_| s.target).filter(|_| state != 3),
                life_ms: None,
                frame_ms: crate::clock::TICK_MS as f32,
                tornado: None,
                site: None,
            });
        }
        // Q, R and S only splash (events 305-308, only 305 recorded).
        let choices: &[&str] = if s.letter >= 16 {
            if state == 2 {
                &["splash1"]
            } else {
                &[]
            }
        } else {
            LAND[(s.letter as usize).min(13)][(state as usize - 1).min(3)]
        };
        if !choices.is_empty() {
            let pick = choices[self.gull_rng.next() as usize % choices.len()];
            self.sounds.push((pick, s.to.0, s.to.1));
        }
        // A hook carries no blow: it holds or falls in the water
        // (0x4bbfc0, 0x4983d0).
        if s.item == HOOK {
            if let Some(from) = s.rope {
                self.hook_landed(from, s.target, target.is_some());
            }
            return;
        }
        // A ship being built keeps the hit for its tick (event 1, 0x4a9e30:
        // a fifth in a tick is dropped), where 0x4bf170 hands it to 0x4b4000
        // alone: no fire, lime, crew or weapon damage.
        if let Some(b) = target.and_then(|i| {
            self.objects[i]
                .ship
                .as_mut()
                .and_then(|sh| sh.build.as_mut())
        }) {
            if b.hit_count < 4 {
                b.hits[b.hit_count as usize] =
                    (s.letter, (s.damage.max(0.0) * 15.0).round() as u32);
                b.hit_count += 1;
            }
            return;
        }
        // A ship not mended in a dock marks the blow and its giver
        // (0x4bf3aa: `+0x691` 1, `+0x692` the shot's `+0xbe`). (Not
        // decoded: `+0xbe` of a shot with no shooter, a volcano's or a
        // fire ship's: left unmarked.)
        if let (Some(i), Some(by)) = (target, s.shooter) {
            if !self.in_dock(i) {
                if let Some(sh) = self.objects[i].ship.as_mut() {
                    sh.blow = Some(by);
                }
            }
        }
        // The hull's share first, then the crew's and the weapons'
        // (0x4bf170).
        if let Some(i) = target.filter(|_| s.volcanic) {
            let report = !matches!(self.objects[i].class, ObjectClass::Monster(0x1f8 | 0x1f9));
            self.ignite(i, s.item, s.flaming, s.damage, data, caches);
            self.strike(s.target, s.damage, false, None, report, data, caches);
            self.wound(s.target, s.wounds);
            self.wear(s.target, s.wear);
            return;
        }
        if let Some(i) = target {
            // The blow's giver: the player in the shot (`+0xb5`); none on
            // a ship being mended in a dock (0x4bf170 hands it to 0x4b4000,
            // whose wreck gives `+0x7f` 0xff). (Not decoded: a land
            // creature struck by a shot following it gets 0x138e with
            // `+0x15` unwritten, 0x496ca5-0x496cef, a byte left on the
            // stack; the port keeps the shot's player.)
            let by = s.by.filter(|_| !self.in_dock(i));
            // A harpy or dragon shot down is reported to no one (0x420670
            // takes its life down to 0 and calls nothing that reports).
            let report = !matches!(self.objects[i].class, ObjectClass::Monster(0x1f8 | 0x1f9));
            // Fire first, judged on the hull before the blow (0x4bf1a0).
            let holder = self.objects[i].holds_units();
            let ship = self.objects[i].ship.is_some();
            // Quick lime limes a ship before anything else, but one being
            // mended in a dock, which it leaves untouched (0x4bf170: state 2
            // goes to 0x4b4000).
            if s.item == combat::QUICKLIME && ship {
                if self.in_dock(i) {
                    return;
                }
                self.lime(i, combat::LIME_TICKS);
            }
            self.ignite(i, s.item, s.flaming, s.damage, data, caches);
            if holder {
                self.fort_blow(
                    s.target, s.damage, s.wounds, s.item, s.flaming, by, s.shooter, data, caches,
                );
            } else {
                self.strike(s.target, s.damage, false, by, report, data, caches);
                self.wound(s.target, s.wounds);
                self.wear(s.target, s.wear);
            }
            // A building it does not bring down is limed (0x42405d).
            if s.item == combat::QUICKLIME && !ship {
                if let Some(i) = self.object_index(s.target) {
                    if matches!(
                        self.objects[i].class,
                        ObjectClass::Building(_) | ObjectClass::Barbarian(_)
                    ) {
                        self.lime(i, combat::LIME_BUILDING_TICKS);
                    }
                }
            }
        }
    }

    /// Limes object `i` for `ticks` (`[unshielded, under its weapon
    /// shield]`), added to what it has (0x4c4120, 0x42405d).
    pub(super) fn lime(&mut self, i: usize, ticks: [u32; 2]) {
        if let Some(c) = self.objects[i].combat.as_mut() {
            let add = crate::clock::ticks(ticks[usize::from(c.protect_weapons > 0.0)]);
            if c.lime <= 0.0 {
                c.lime_total = 0.0;
            }
            c.lime += add;
            c.lime_total += add;
        }
    }

    /// Whether ship `i` lies in a dock being mended (state 2).
    pub(super) fn in_dock(&self, i: usize) -> bool {
        match self.objects[i].ship.as_ref().and_then(|s| s.task) {
            Some(Task::Repair { dock }) => self
                .object_index(dock)
                .is_some_and(|d| self.alongside(i, d)),
            _ => false,
        }
    }

    /// Buildings limed now: their work stands still (school 0x432210,
    /// smithy 0x434ed0, studio 0x436ee0, the town's training 0x441ba0 skip
    /// their "done?" step).
    pub(super) fn limed(&self) -> Vec<u32> {
        self.objects
            .iter()
            .filter(|o| o.ship.is_none() && o.combat.as_ref().is_some_and(|c| c.lime > 0.0))
            .map(|o| o.id)
            .collect()
    }

    /// Lime wears off (a ship 0x4df2d0, a building 0x425990). A building's
    /// work then starts again from when the lime struck (`+0x124` set to
    /// its start): what it had done before is lost and the limed time
    /// counts.
    pub(super) fn run_lime(&mut self, dt: f32) {
        let mut cleared = Vec::new();
        for o in &mut self.objects {
            let Some(c) = o.combat.as_mut() else { continue };
            if c.lime <= 0.0 {
                continue;
            }
            c.lime -= dt;
            if c.lime <= 0.0 {
                c.lime = 0.0;
                if o.ship.is_none() {
                    cleared.push((o.id, c.lime_total));
                }
                c.lime_total = 0.0;
            }
        }
        for (site, since) in cleared {
            // The temples' learning and the Flame likewise (0x438300,
            // 0x439260 run 0x427f50, ending with 0x425990, first); the
            // computers' clocks run twice as fast.
            self.magic.lime_cleared(site, since, self.permits);
            for c in &mut self.computers.players {
                c.magic
                    .lime_cleared(site, since * 2.0, crate::permits::Permits::ALL);
            }
            self.seat_lime_cleared(site, since);
            if let Some(r) = self.research.iter_mut().find(|r| r.0 == site) {
                r.3 = since;
            }
            if let Some(r) = self.refits.iter_mut().find(|r| r.0 == site) {
                r.3 = since;
            }
            if let Some(t) = self.trainings.iter_mut().find(|t| t.site == site) {
                let before: f32 = t.queue[..t.head.min(t.queue.len())]
                    .iter()
                    .map(|&k| combat::train_seconds(k))
                    .sum();
                t.elapsed = before + since;
            }
        }
    }
}
