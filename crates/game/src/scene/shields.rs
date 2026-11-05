//! A building's shield sprites (LandObj 0x425990): the weapon shield's
//! `PROTWSPR` on layer 0xd and the magic shield's `PROTMSPR` on layer 0xc.

use super::Scene;
use crate::objects;
use crate::{Caches, GameData};
use openconquest_formats::ObjectClass;

/// The frames a shield's sprite loops through while the shield is on
/// (lowest, highest), for a building of size 4 and for the others: the
/// weapon shield's 0x595f90 / 0x595f98, the magic shield's 0x595fa0 /
/// 0x595fa8.
const WEAPON_LOOP: [(i32, i32); 2] = [(14, 43), (14, 44)];
const MAGIC_LOOP: [(i32, i32); 2] = [(14, 43), (13, 40)];

/// One shield's sprite state: its frame (`+0x18c` / `+0x16c`), the tick of
/// its last step (`+0x190` / `+0x170`) and its sprite (`+0x194` /
/// `+0x174`, layer `+0x198` / `+0x178`) with its top left.
#[derive(Default)]
pub struct ShieldSprite {
    pub frame: i32,
    time: u32,
    pub sprite: Option<(Vec<Vec<objects::Layer>>, i32, i32)>,
}

/// A building's two shield sprites, kept once it has had a shield (the
/// frames and times stay in the LandObj between shields).
pub struct BuildingShields {
    pub building: u32,
    pub weapon: ShieldSprite,
    pub magic: ShieldSprite,
}

impl ShieldSprite {
    /// The tick's work on one shield (0x4263ef-0x4265a4 for the weapon
    /// shield, 0x4265ed-0x4267ed for the magic one), `on` its flag
    /// (`+0x17c` / `+0x15c`), `at` the picture's top left and size, `big`
    /// the building of size 4 (`+0x56`). Off, a made sprite plays on until
    /// its frame reaches the sequence's count (`+0x49`), then its layer is
    /// removed (0x50cdb0). On and not made, it is made (0x50cbf0, 0x50cf60
    /// with `name`) and placed at its frame, which the casting set to 0.
    /// Otherwise, made and more than 1 tick past its time, its frame goes
    /// on by one, while on back to the loop's lowest once past its highest,
    /// and it is placed (0x50d660).
    fn tick(
        &mut self,
        on: bool,
        now: u32,
        at: (i32, i32, i32, i32),
        big: bool,
        lo_hi: [(i32, i32); 2],
        load: impl FnOnce() -> Vec<Vec<objects::Layer>>,
    ) {
        if !on
            && self
                .sprite
                .as_ref()
                .is_none_or(|s| self.frame >= s.0.len() as i32)
        {
            self.sprite = None;
            return;
        }
        if on && self.sprite.is_none() {
            let frames = load();
            if frames.is_empty() {
                return;
            }
            self.sprite = Some((frames, 0, 0));
        } else {
            if self.sprite.is_none() || now <= self.time + 1 {
                return;
            }
            self.time = now;
            self.frame += 1;
            let (lo, hi) = lo_hi[usize::from(!big)];
            if on && hi < self.frame {
                self.frame = lo;
            }
        }
        // 0x50d660 at the picture's left + w / 2 less the sprite's w / 2,
        // and its top + h / 4 less the sprite's h / 2.
        let (left, top, w, h) = at;
        let (frames, x, y) = self.sprite.as_mut().unwrap();
        let (sw, sh) = frames
            .first()
            .and_then(|f| f.first())
            .map_or((0, 0), |l| (l.image.width as i32, l.image.height as i32));
        (*x, *y) = (w / 2 - sw / 2 + left, h / 4 - sh / 2 + top);
    }

    /// The frame shown and its top left: the frame modulo the count
    /// (0x55ec70).
    pub fn shown(&self) -> Option<(&[objects::Layer], i32, i32)> {
        let (frames, x, y) = self.sprite.as_ref()?;
        let n = frames.len() as i32;
        let f = frames.get(self.frame.rem_euclid(n.max(1)) as usize)?;
        Some((f.as_slice(), *x, *y))
    }
}

impl Scene {
    /// Whether building `class` gets the shields' sprites: LandObj and the
    /// classes whose tick runs 0x425990 (castles through 0x427f50, lairs,
    /// harpy nests, towns, workshops...). The hero's house (0x49, 0x420fd0)
    /// clears `+0x180` / `+0x160`, and forts and barbarian structures tick
    /// by their own 0x419ba0 / 0x40eba0, which make none.
    fn shield_sprites(class: u32) -> bool {
        class != 0x49 && !crate::combat::holds_units(class)
    }

    /// A shield cast on building `id` while off (0xda9a): its frame
    /// `+0x18c` / `+0x16c` to 0, and the weapon shield's time `+0x190` to
    /// the tick for either shield (the magic one's `+0x170` is left).
    pub(super) fn shield_cast(&mut self, id: u32, weapon: bool) {
        let now = self.ticks;
        let s = self.shields_of(id);
        s.weapon.time = now;
        if weapon {
            s.weapon.frame = 0;
        } else {
            s.magic.frame = 0;
        }
    }

    fn shields_of(&mut self, id: u32) -> &mut BuildingShields {
        let k = match self.building_shields.iter().position(|s| s.building == id) {
            Some(k) => k,
            None => {
                self.building_shields.push(BuildingShields {
                    building: id,
                    weapon: ShieldSprite::default(),
                    magic: ShieldSprite::default(),
                });
                self.building_shields.len() - 1
            }
        };
        &mut self.building_shields[k]
    }

    /// Building `i`'s shield sprites in its tick, the weapon shield's
    /// (`weapon`) or the magic one's, before the shield's end is tested.
    pub(super) fn tick_shield_sprite(
        &mut self,
        i: usize,
        weapon: bool,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let o = &self.objects[i];
        let ObjectClass::Building(class) = o.class else {
            return;
        };
        let Some(c) = o.combat.as_ref() else {
            return;
        };
        let on = if weapon { c.weapon_span } else { c.magic_span }.is_some();
        let id = o.id;
        let known = self.building_shields.iter().any(|s| s.building == id);
        if !Self::shield_sprites(class) || (!on && !known) {
            return;
        }
        let Some(at) = o.frames.first().and_then(|f| f.first()).map(|l| {
            let (left, top) = crate::render::layer_origin(o, l);
            (left, top, l.image.width as i32, l.image.height as i32)
        }) else {
            return;
        };
        // Size 4 (`+0x56`): settlements and towns; `_64` for size 1.
        let big = matches!(class, 0x32 | 0x33);
        let name = format!(
            "{}_{}",
            if weapon { "PROTWSPR" } else { "PROTMSPR" },
            if big { 128 } else { 64 }
        );
        let now = self.ticks;
        let s = self.shields_of(id);
        let (sprite, lo_hi) = if weapon {
            (&mut s.weapon, WEAPON_LOOP)
        } else {
            (&mut s.magic, MAGIC_LOOP)
        };
        sprite.tick(on, now, at, big, lo_hi, || {
            objects::sequence_frames(data, caches, &name).unwrap_or_else(|e| {
                eprintln!("could not load effect {name}: {e:#}");
                Vec::new()
            })
        });
    }

    /// Building `id`'s shield sprites now, the magic shield's (depth the
    /// picture's less 4) then the weapon shield's (less 5, nearer).
    pub fn building_shields_on(&self, id: u32) -> Vec<(&[objects::Layer], i32, i32)> {
        self.building_shields
            .iter()
            .filter(|s| s.building == id)
            .flat_map(|s| [s.magic.shown(), s.weapon.shown()])
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::magic::Spell;

    #[test]
    fn a_buildings_shields_loop_and_play_out_as_its_tick_steps_them() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        // Every frame of each sequence one size, one layer.
        for name in ["PROTWSPR_64", "PROTWSPR_128", "PROTMSPR_64", "PROTMSPR_128"] {
            let frames = objects::sequence_frames(&data, &mut caches, name).unwrap();
            let first = frames[0][0].image.clone();
            eprintln!(
                "{name}: {} frames {}x{}",
                frames.len(),
                first.width,
                first.height
            );
            for f in &frames {
                assert_eq!(f.len(), 1);
                assert_eq!(
                    (f[0].image.width, f[0].image.height),
                    (first.width, first.height)
                );
            }
        }
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let i = scene
            .objects
            .iter()
            .position(|o| {
                matches!(o.class, ObjectClass::Building(c)
                    if Scene::shield_sprites(c) && !matches!(c, 0x32 | 0x33))
                    && o.combat.is_some()
            })
            .expect("a building with shields' sprites");
        let id = scene.objects[i].id;
        let o = &scene.objects[i];
        let pic = &o.frames[0][0];
        let (left, top) = crate::render::layer_origin(o, pic);
        let (w, h) = (pic.image.width as i32, pic.image.height as i32);
        scene.protect(Spell::ProtectWeapons, i, &data, &mut caches);
        let start = scene.ticks;
        let frames = objects::sequence_frames(&data, &mut caches, "PROTWSPR_64").unwrap();
        let (sw, sh) = (
            frames[0][0].image.width as i32,
            frames[0][0].image.height as i32,
        );
        let count = frames.len() as i32;
        let tick = |scene: &mut Scene, caches: &mut Caches| {
            scene.ticks += 1;
            scene.tick_building_statuses(&data, caches);
        };
        // Made in the next tick at frame 0, at the picture's left + w / 2
        // - sw / 2 and top + h / 4 - sh / 2.
        tick(&mut scene, &mut caches);
        let s = &scene.building_shields[0];
        assert_eq!(s.building, id);
        let (_, x, y) = s.weapon.shown().unwrap();
        assert_eq!((x, y), (w / 2 - sw / 2 + left, h / 4 - sh / 2 + top));
        assert_eq!(s.weapon.frame, 0);
        assert!(s.magic.shown().is_none());
        // A frame each 2 ticks; while on, 14..44 round.
        let mut seen = Vec::new();
        for _ in 0..200 {
            tick(&mut scene, &mut caches);
            seen.push(scene.building_shields[0].weapon.frame);
        }
        assert_eq!(&seen[..4], &[1, 1, 2, 2]);
        assert_eq!(seen.iter().max(), Some(&44));
        let k = seen.iter().position(|&f| f == 44).unwrap();
        assert_eq!(seen[k + 2], 14);
        // Off at its length; it plays on to the count, then is removed.
        while scene.ticks < start + crate::magic::PROTECTION_TICKS {
            tick(&mut scene, &mut caches);
            assert!(scene.building_shields[0].weapon.sprite.is_some());
        }
        assert!(scene.objects[i]
            .combat
            .as_ref()
            .unwrap()
            .weapon_span
            .is_none());
        let mut last = 0;
        while scene.building_shields[0].weapon.sprite.is_some() {
            last = scene.building_shields[0].weapon.frame;
            tick(&mut scene, &mut caches);
            assert!(scene.ticks < start + crate::magic::PROTECTION_TICKS + 200);
        }
        assert_eq!(last, count);
        assert!(scene.building_shields_on(id).is_empty());
    }
}
