//! The fireball spell (SlightClassTy, slight.cpp): a ball of fire over a
//! map cell, drawn a cell up, a frame a tick. It appears (`SLIGHT1`, 60
//! frames), hovers (`SLIGHT2`, 60) and strikes (`SLIGHT4`, 9) 2 to 4
//! times, hovering between, then dies away (`SLIGHT3`, 60; 0x4059e0,
//! 0x405c20). At the strike's frame 4 it throws a shot (`BOMBK1`) at each
//! enemy ship, creature, castle, nest and lair within 2 cells, each
//! carrying 88000 / (the targets, at most 4) / (strikes - 1).

use super::*;
use crate::objects::Layer;

/// The blow a strike shares out, in the original's units (see
/// `combat::hull_strength` for ours).
const BLOW: f32 = 88_000.0 / 15.0;

pub struct Fireball {
    pub cell: (i32, i32),
    owner: Option<(u32, u32)>,
    /// 1 appearing, 2 hovering, 3 dying away, 4 striking; 0 gone.
    state: u8,
    frame: usize,
    clock: f32,
    strikes: u32,
    struck: u32,
    /// Pictures by state 1-4.
    frames: [Vec<Vec<Layer>>; 4],
}

/// A fireball as a save keeps it (its pictures are found again).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedFireball {
    cell: (i32, i32),
    owner: Option<(u32, u32)>,
    state: u8,
    frame: usize,
    clock: f32,
    strikes: u32,
    struck: u32,
}

fn fireball_frames(data: &GameData, caches: &mut Caches) -> [Vec<Vec<Layer>>; 4] {
    [1, 2, 3, 4]
        .map(|n| objects::sequence_frames(data, caches, &format!("SLIGHT{n}")).unwrap_or_default())
}

impl Fireball {
    pub fn saved(&self) -> SavedFireball {
        SavedFireball {
            cell: self.cell,
            owner: self.owner,
            state: self.state,
            frame: self.frame,
            clock: self.clock,
            strikes: self.strikes,
            struck: self.struck,
        }
    }

    pub fn restored(s: &SavedFireball, data: &GameData, caches: &mut Caches) -> Self {
        Fireball {
            cell: s.cell,
            owner: s.owner,
            state: s.state,
            frame: s.frame,
            clock: s.clock,
            strikes: s.strikes,
            struck: s.struck,
            frames: fireball_frames(data, caches),
        }
    }

    /// Its picture now, and its top-left corner in world pixels.
    pub fn picture(&self) -> Option<(&[Layer], f32, f32)> {
        let frames = self.frames.get(self.state.checked_sub(1)? as usize)?;
        let layers = frames.get(self.frame.min(frames.len().checked_sub(1)?))?;
        let w = layers.first()?.image.width as f32;
        let t = TILE_SIZE as f32;
        // 0x405850: a cell up, centred across.
        Some((
            layers,
            self.cell.0 as f32 * t - (w - t) / 2.0,
            (self.cell.1 - 1) as f32 * t - (w - t) / 2.0,
        ))
    }
}

impl Scene {
    /// Sets a fireball over map cell `(cx, cy)` for `owner`.
    pub(super) fn spawn_fireball(
        &mut self,
        cx: i32,
        cy: i32,
        owner: Option<(u32, u32)>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let frames = fireball_frames(data, caches);
        let t = TILE_SIZE as f32;
        let (x, y) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
        self.sounds.push(("bf1", x, y));
        self.fireballs.push(Fireball {
            cell: (cx, cy),
            owner,
            state: 1,
            frame: 0,
            clock: 0.0,
            strikes: 0,
            struck: 0,
            frames,
        });
    }

    /// Runs the fireballs a tick at a time.
    pub(super) fn run_fireballs(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        for k in 0..self.fireballs.len() {
            self.fireballs[k].clock += dt;
            while self.fireballs[k].clock >= crate::clock::TICK && self.fireballs[k].state != 0 {
                let f = &mut self.fireballs[k];
                f.clock -= crate::clock::TICK;
                f.frame += 1;
                let len = f.frames[f.state as usize - 1].len().max(1);
                let (x, y) = (f.cell.0 as f32 * t + t / 2.0, f.cell.1 as f32 * t + t / 2.0);
                if f.state == 4 && f.frame == 4 {
                    self.fireball_strike(k, data, caches);
                }
                let f = &mut self.fireballs[k];
                if f.frame < len {
                    continue;
                }
                f.frame = 0;
                f.state = match f.state {
                    1 => {
                        f.strikes = 2 + self.rng.next() % 3;
                        2
                    }
                    2 if f.struck < f.strikes => {
                        f.struck += 1;
                        self.sounds.push(("fireball", x, y));
                        4
                    }
                    2 => {
                        self.sounds.push(("deafir1", x, y));
                        3
                    }
                    4 => 2,
                    _ => 0,
                };
            }
        }
        self.fireballs.retain(|f| f.state != 0);
    }

    /// Fireball `k` throws its shots.
    fn fireball_strike(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let f = &self.fireballs[k];
        let ((cx, cy), owner, strikes) = (f.cell, f.owner, f.strikes);
        let Some((_, left, top)) = f.picture() else {
            return;
        };
        let w = f
            .picture()
            .and_then(|(l, ..)| l.first())
            .map_or(t, |l| l.image.width as f32);
        let from = (left + w / 2.0, top + 22.0);
        // Its shots are put down to the caster, but for nation 4 slot 1's,
        // the creatures' (0x405fa0-0x405fbb give 0xff).
        let by = owner.filter(|&o| o != crate::combat::MONSTERS);
        let targets: Vec<(u32, bool)> = self
            .objects
            .iter()
            .filter(|o| {
                let (ox, oy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
                (ox - cx).abs() <= 2
                    && (oy - cy).abs() <= 2
                    && o.combat.is_some()
                    && o.owner != owner
                    && matches!(
                        o.class,
                        ObjectClass::Ship
                            | ObjectClass::Monster(0x1f8..=0x1fd)
                            | ObjectClass::Castle(_)
                            | ObjectClass::HarpyNest
                            | ObjectClass::CreatureSpawn
                    )
                    // The torch of Ares keeps all fire off.
                    && !o
                        .ship
                        .as_ref()
                        .is_some_and(|s| s.has_treasure(objects::FIRE_TORCH))
            })
            .map(|o| (o.id, o.combat.as_ref().unwrap().protect_magic > 0.0))
            .collect();
        if targets.is_empty() {
            return;
        }
        let blow = BLOW / targets.len().min(4) as f32 / strikes.saturating_sub(1).max(1) as f32;
        for (target, shielded) in targets {
            let shield = if shielded { magic::SHIELD_FACTOR } else { 1.0 };
            self.fire_shot(
                super::shot::Firing {
                    item: super::shot::FIREBALL,
                    letter: None,
                    flaming: true,
                    from,
                    target,
                    strikes: true,
                    damage: blow * shield,
                    wounds: 0.0,
                    wear: 0.0,
                    rope: None,
                    shooter: None,
                    by,
                    to: None,
                },
                data,
                caches,
            );
        }
    }
}
