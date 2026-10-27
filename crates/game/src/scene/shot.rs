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
const LAUNCH: [&str; 14] = [
    "bas031", "cross1", "thrcat1", "bas034", "gastsn1", "thrcat1", "thrbal1", "thrcat1", "thrbal1",
    "cross1", "", "", "fir1", "thrcat1",
];
const LAND: [[&[&str]; 4]; 14] = [
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
    pub variant: u8,
    from: (f32, f32),
    to: (f32, f32),
    /// What it was fired at, and whether it strikes it.
    target: u32,
    strikes: bool,
    /// The blow to the hull and to the crew it carries.
    damage: f32,
    wounds: f32,
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
}

impl Shot {
    /// Its picture now.
    pub fn layer(&self) -> Option<&Layer> {
        if self.frames.is_empty() {
            return None;
        }
        let tick = (self.age / crate::clock::TICK) as usize;
        Some(&self.frames[tick % self.frames.len()])
    }
}

/// The letter and variant for item `item` (0x4c0230: Greek fire flies as
/// M, flaming; quick lime as N).
fn letter_of(item: u8, flaming: bool) -> (u8, u8) {
    match item {
        combat::GREEK_FIRE => (12, 1),
        combat::QUICKLIME => (13, 0),
        _ => (item.min(8), u8::from(flaming)),
    }
}

fn picture(letter: u8, state: u8, variant: u8) -> String {
    format!("BOMB{}{}{}", (b'A' + letter) as char, state.min(6), variant)
}

/// The 16 directions (0x497e50): 0 east, counter-clockwise.
fn direction(dx: f32, dy: f32) -> usize {
    let a = (-dy).atan2(dx).rem_euclid(std::f32::consts::TAU);
    ((a / (std::f32::consts::TAU / 16.0)).round() as usize) % 16
}

/// A shot about to be fired.
pub(super) struct Firing {
    pub item: u8,
    pub flaming: bool,
    pub from: (f32, f32),
    pub target: u32,
    pub strikes: bool,
    pub damage: f32,
    pub wounds: f32,
}

impl Scene {
    /// Sends shot `f` off at its target, landing on it if it strikes and
    /// beside it if not.
    pub(super) fn fire_shot(&mut self, f: Firing, data: &GameData, caches: &mut Caches) {
        let Some(t) = self.object_index(f.target) else {
            return;
        };
        let (mut tx, mut ty) = (self.objects[t].x, self.objects[t].y);
        if !f.strikes {
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
        let (letter, variant) = letter_of(f.item, f.flaming);
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
        let name = picture(letter, 0, variant);
        let cells = |name: &str, caches: &mut Caches| -> Vec<Layer> {
            objects::composite_layer_frames(data, caches, name, dir)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|f| f.into_iter().next())
                .collect()
        };
        let mut frames = cells(&name, caches);
        if frames.is_empty() {
            frames = cells(&picture(letter, 0, 1 - variant), caches);
        }
        if !LAUNCH[l.min(13)].is_empty() {
            self.sounds.push((LAUNCH[l.min(13)], f.from.0, f.from.1));
        }
        self.shots.push(Shot {
            letter,
            variant,
            from: f.from,
            to: (tx, ty),
            target: f.target,
            strikes: f.strikes,
            damage: f.damage,
            wounds: f.wounds,
            age: 0.0,
            flight,
            apex,
            frames,
            x: f.from.0,
            y: f.from.1,
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
        for k in landed.into_iter().rev() {
            let s = self.shots.swap_remove(k);
            self.land_shot(s, data, caches);
        }
    }

    fn land_shot(&mut self, s: Shot, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let target = self.object_index(s.target).filter(|_| s.strikes);
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
        let choices = LAND[(s.letter as usize).min(13)][(state as usize - 1).min(3)];
        if !choices.is_empty() {
            let pick = choices[self.gull_rng.next() as usize % choices.len()];
            self.sounds.push((pick, s.to.0, s.to.1));
        }
        if target.is_some() {
            self.wound(s.target, s.wounds);
            self.strike(s.target, s.damage, false, data, caches);
        }
    }
}
