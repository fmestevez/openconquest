//! A building's shield sprites (LandObj 0x425990): the weapon shield's
//! `PROTWSPR` on layer 0xd and the magic shield's `PROTMSPR` on layer 0xc;
//! a ship's (its tick by 0x4e0f70 / 0x4e11a0) on layers 7 and 8.

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

/// A ship's shield's loop while on: the frame stepped to the first goes
/// back to the second (0x4e104a weapon, 0x4e1284 magic).
const SHIP_WEAPON_LOOP: (i32, i32) = (45, 14);
const SHIP_MAGIC_LOOP: (i32, i32) = (42, 13);

/// One shield's sprite state: its frame (`+0x18c` / `+0x16c`), the tick of
/// its last step (`+0x190` / `+0x170`) and its sprite (`+0x194` /
/// `+0x174`, layer `+0x198` / `+0x178`) with its top left; a ship's are
/// the sprite's own frame `+0x45` and time `+0x81` (sprites `+0x8da` /
/// `+0x8de`). `placed` is the frame 0x50d660 last showed.
#[derive(Default)]
pub struct ShieldSprite {
    pub frame: i32,
    time: u32,
    placed: i32,
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
        self.placed = self.frame;
    }

    /// A ship's shield sprite's place over its hull (`+0x8be`, top left
    /// and size `at`), as 0x4e0e70 makes it and 0x4e0f70 steps it: the
    /// hull's left less half the widths' difference, its top less half the
    /// heights' difference and a quarter of its height (C division).
    fn ship_place(&mut self, at: (i32, i32, i32, i32)) {
        let (left, top, w, h) = at;
        let Some((frames, x, y)) = self.sprite.as_mut() else {
            return;
        };
        let (sw, sh) = frames
            .first()
            .and_then(|f| f.first())
            .map_or((0, 0), |l| (l.image.width as i32, l.image.height as i32));
        (*x, *y) = (left - (sw - w) / 2, top - h / 4 - (sh - h) / 2);
    }

    /// A ship's shield's step (0x4e0f70 weapon, 0x4e11a0 magic), from its
    /// tick while its state (`+0x254` / `+0x255`) is not 0: with the
    /// sprite made and the game tick past its time `+0x81` by more than
    /// the delay `+0x7d` (0), time = the tick and 0x50d660 shows its frame
    /// at its place (depth the hull's `+0x19` less 0x4a); then (with the
    /// update's ticks `[clock+0xe2]` odd, or not a multiple of 3 for the
    /// magic one: 1 in single player) the frame goes on by one: on (state
    /// 2 or 3), from `lo_hi.0` back to `lo_hi.1` (state 3); off (state 1),
    /// at the sequence's count `+0x49` the layer is removed (0x4e0f50 /
    /// 0x4e1180, 0x50cdb0) and the state is 0.
    fn ship_step(&mut self, on: bool, now: u32, at: (i32, i32, i32, i32), lo_hi: (i32, i32)) {
        if self.sprite.is_none() || now.wrapping_sub(self.time) == 0 {
            return;
        }
        self.time = now;
        self.ship_place(at);
        self.placed = self.frame;
        self.frame += 1;
        if on {
            if self.frame >= lo_hi.0 {
                self.frame = lo_hi.1;
            }
        } else if self
            .sprite
            .as_ref()
            .is_some_and(|s| self.frame >= s.0.len() as i32)
        {
            self.sprite = None;
        }
    }

    /// The frame shown and its top left: the frame modulo the count
    /// (0x55ec70).
    pub fn shown(&self) -> Option<(&[objects::Layer], i32, i32)> {
        let (frames, x, y) = self.sprite.as_ref()?;
        let n = frames.len() as i32;
        let f = frames.get(self.placed.rem_euclid(n.max(1)) as usize)?;
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

    /// The top left and size of object `i`'s picture (a ship's hull,
    /// `+0x8be`: its `+0x59`, `+0x5d`, `+0x61`, `+0x65`).
    fn picture_at(&self, i: usize) -> Option<(i32, i32, i32, i32)> {
        let o = &self.objects[i];
        o.frames.first().and_then(|f| f.first()).map(|l| {
            let (left, top) = crate::render::layer_origin(o, l);
            (left, top, l.image.width as i32, l.image.height as i32)
        })
    }

    /// A shield cast on the ship at index `i` while its state (`+0x254`
    /// weapon / `+0x255` magic) is below 2 (0xda9a at 0x4de53c /
    /// 0x4de4c8, before its time `+0x91` / `+0x99` is the tick and its
    /// length `+0x95` / `+0x9d` 0): without a sprite one is made (0x4e0e70
    /// / 0x4e10a0: layer 7 / 8 by 0x50cbf0 depth 10, `PROTWSPR_64` /
    /// `PROTMSPR_64` whatever the ship (0x4e0f30 / 0x4e1160), frame,
    /// delay and time 0, placed over the hull); a sprite still playing
    /// out goes on from its frame. The state is then 2.
    pub(super) fn ship_shield_cast(
        &mut self,
        i: usize,
        weapon: bool,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let id = self.objects[i].id;
        let at = self.picture_at(i).unwrap_or_default();
        let s = self.shields_of(id);
        let sprite = if weapon { &mut s.weapon } else { &mut s.magic };
        if sprite.sprite.is_some() {
            return;
        }
        let name = if weapon { "PROTWSPR_64" } else { "PROTMSPR_64" };
        let frames = objects::sequence_frames(data, caches, name).unwrap_or_else(|e| {
            eprintln!("could not load effect {name}: {e:#}");
            Vec::new()
        });
        if frames.is_empty() {
            return;
        }
        *sprite = ShieldSprite {
            sprite: Some((frames, 0, 0)),
            ..ShieldSprite::default()
        };
        sprite.ship_place(at);
    }

    /// The ships' phantom and shields in their tick (0x4dc630): 0x4dec40
    /// ends them while the ship's mode `+0x251` is below 3 (0 afloat, 1,
    /// 2 being built or sold; 4, sunk, is the only other value written,
    /// and the port takes a sunk ship out at its blow, so every ship here
    /// passes). First the phantom (`+0x253` set, 0x4df210): the game tick
    /// less its start `+0x81` at least its length `+0x85` (unsigned), its
    /// start and length 0, 0x4df420(0) and `+0x253` 0; then (after the
    /// statuses `+0x257` and `+0x258`) the
    /// weapon shield then the magic one, each on (state 2 or 3) when the
    /// game tick less its time is at least its length (unsigned), its
    /// state then 1; the magic one's end clears the weapon shield's
    /// length `+0x95` (0x4defa9) instead of its own `+0x9d`, so a weapon
    /// shield still on ends at the next tick. Then the magic shield's
    /// sprite is stepped (0x4dca36), then the weapon one's (0x4dca47).
    pub(super) fn tick_ship_shields(&mut self, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        for i in 0..self.objects.len() {
            let o = &mut self.objects[i];
            let id = o.id;
            let Some(c) = o.combat.as_mut().filter(|_| o.ship.is_some()) else {
                continue;
            };
            // Saves from before the ticks: what is left, from now.
            for (left, span) in [
                (c.phantom, &mut c.phantom_span),
                (c.protect_weapons, &mut c.weapon_span),
                (c.protect_magic, &mut c.magic_span),
            ] {
                if left > 0.0 && span.is_none() {
                    *span = Some((now, (left / crate::clock::TICK).round() as u32));
                }
            }
            let ended =
                |s: Option<(u32, u32)>| s.is_some_and(|(at, len)| now.wrapping_sub(at) >= len);
            if ended(c.phantom_span) {
                (c.phantom, c.phantom_span) = (0.0, None);
            }
            if ended(c.weapon_span) {
                (c.protect_weapons, c.weapon_span) = (0.0, None);
            }
            if ended(c.magic_span) {
                (c.protect_magic, c.magic_span) = (0.0, None);
                if let Some((_, len)) = c.weapon_span.as_mut() {
                    *len = 0;
                }
            }
            let (weapon, magic) = (c.weapon_span.is_some(), c.magic_span.is_some());
            // A loaded game's shields (their sprites not saved) start anew.
            for (on, w) in [(weapon, true), (magic, false)] {
                let made = self.building_shields.iter().any(|s| {
                    s.building == id && if w { &s.weapon } else { &s.magic }.sprite.is_some()
                });
                if on && !made {
                    self.ship_shield_cast(i, w, data, caches);
                }
            }
            let Some(k) = self.building_shields.iter().position(|s| s.building == id) else {
                continue;
            };
            let at = self.picture_at(i).unwrap_or_default();
            let s = &mut self.building_shields[k];
            s.magic.ship_step(magic, now, at, SHIP_MAGIC_LOOP);
            s.weapon.ship_step(weapon, now, at, SHIP_WEAPON_LOOP);
        }
    }

    /// Building `id`'s shield sprites now, the magic shield's (depth the
    /// picture's less 4) then the weapon shield's (less 5, nearer), each
    /// with whether it is the weapon shield.
    pub fn building_shields_on(&self, id: u32) -> Vec<(&[objects::Layer], i32, i32, bool)> {
        self.building_shields
            .iter()
            .filter(|s| s.building == id)
            .flat_map(|s| {
                [
                    s.magic.shown().map(|(l, x, y)| (l, x, y, false)),
                    s.weapon.shown().map(|(l, x, y)| (l, x, y, true)),
                ]
            })
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::magic::Spell;
    use crate::scene::spells::SHIELD_SOUND;

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
        scene.sounds.clear();
        scene.protect(Spell::ProtectWeapons, i, &data, &mut caches);
        let start = scene.ticks;
        // Event 0x56 at the building (0x42719f), `prote3`, at every
        // casting, on or off.
        let (sx, sy) = (scene.objects[i].x, scene.objects[i].y);
        assert_eq!(scene.sounds, [(SHIELD_SOUND, sx, sy)]);
        scene.protect(Spell::ProtectWeapons, i, &data, &mut caches);
        assert_eq!(scene.sounds.len(), 2);
        scene.objects[i].combat.as_mut().unwrap().weapon_span =
            Some((start, crate::magic::PROTECTION_TICKS));
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

    #[test]
    fn a_ships_shields_loop_play_out_and_end_as_its_tick_steps_them() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let i = scene
            .objects
            .iter()
            .position(|o| o.ship.is_some() && o.combat.is_some())
            .expect("a ship");
        let id = scene.objects[i].id;
        let o = &scene.objects[i];
        let hull = &o.frames[0][0];
        let (left, top) = crate::render::layer_origin(o, hull);
        let (w, h) = (hull.image.width as i32, hull.image.height as i32);
        let tick = |scene: &mut Scene, caches: &mut Caches| {
            scene.ticks += 1;
            scene.tick_ship_shields(&data, caches);
        };
        // The magic shield first: made at the casting, `_64` whatever the
        // ship, at frame 0 over the hull.
        let start = scene.ticks;
        scene.sounds.clear();
        scene.protect(Spell::ProtectMagic, i, &data, &mut caches);
        // Cast while off, event 0x57 at the ship (0x4de510): `prote3`.
        let (sx, sy) = (scene.objects[i].x, scene.objects[i].y);
        assert_eq!(scene.sounds, [(SHIELD_SOUND, sx, sy)]);
        let frames = objects::sequence_frames(&data, &mut caches, "PROTMSPR_64").unwrap();
        let (sw, sh) = (
            frames[0][0].image.width as i32,
            frames[0][0].image.height as i32,
        );
        let s = &scene.building_shields[0];
        assert_eq!(s.building, id);
        let (_, x, y) = s.magic.shown().unwrap();
        assert_eq!((x, y), (left - (sw - w) / 2, top - h / 4 - (sh - h) / 2));
        for _ in 0..10 {
            tick(&mut scene, &mut caches);
        }
        // Shown, then stepped, each tick: the magic one 0..41, then 13..41
        // round; the weapon one 0..44, then 14..44.
        scene.protect(Spell::ProtectWeapons, i, &data, &mut caches);
        let weapon_cast = scene.ticks;
        // Cast again while on: the length grows, no sound (0x4de4cf).
        scene.sounds.clear();
        scene.protect(Spell::ProtectWeapons, i, &data, &mut caches);
        assert!(scene.sounds.is_empty());
        let (mut magic, mut weapon) = (Vec::new(), Vec::new());
        for _ in 0..100 {
            tick(&mut scene, &mut caches);
            magic.push(scene.building_shields[0].magic.placed);
            weapon.push(scene.building_shields[0].weapon.placed);
        }
        assert_eq!(&magic[..3], &[10, 11, 12]);
        assert_eq!((magic[31], magic[32]), (41, 13));
        assert_eq!(magic.iter().max(), Some(&41));
        assert_eq!((weapon[0], weapon[44], weapon[45]), (0, 44, 14));
        assert_eq!(weapon.iter().max(), Some(&44));
        // The magic shield ends at its length and clears the weapon's
        // length (0x4defa9): the weapon shield ends at the next tick.
        while scene.ticks < start + crate::magic::PROTECTION_TICKS - 1 {
            tick(&mut scene, &mut caches);
        }
        let c = scene.objects[i].combat.as_ref().unwrap();
        assert!(c.magic_span.is_some() && c.weapon_span.is_some());
        tick(&mut scene, &mut caches);
        let c = scene.objects[i].combat.as_ref().unwrap();
        assert!(c.magic_span.is_none() && c.protect_magic == 0.0);
        assert_eq!(c.weapon_span, Some((weapon_cast, 0)));
        tick(&mut scene, &mut caches);
        let c = scene.objects[i].combat.as_ref().unwrap();
        assert!(c.weapon_span.is_none() && c.protect_weapons == 0.0);
        // Off, each plays on to its count, then is removed.
        let count = frames.len() as i32;
        let mut last = 0;
        while scene.building_shields[0].magic.sprite.is_some() {
            last = scene.building_shields[0].magic.placed;
            tick(&mut scene, &mut caches);
            assert!(scene.ticks < start + crate::magic::PROTECTION_TICKS + 100);
        }
        // The step that shows the count's last frame removes the layer,
        // so the last drawn is the one before.
        assert_eq!(last, count - 2);
        while scene.building_shields[0].weapon.sprite.is_some() {
            tick(&mut scene, &mut caches);
            assert!(scene.ticks < start + crate::magic::PROTECTION_TICKS + 100);
        }
        assert!(scene.building_shields_on(id).is_empty());
        // The phantom (0x4de378): its start the tick it was cast off, each
        // casting adding 3000; it ends in the tick whose distance from the
        // start reaches the length (0x4df210), before the shields.
        let phantom = scene.ticks;
        scene.protect(Spell::Phantom, i, &data, &mut caches);
        tick(&mut scene, &mut caches);
        scene.protect(Spell::Phantom, i, &data, &mut caches);
        let c = scene.objects[i].combat.as_ref().unwrap();
        assert_eq!(c.phantom_span, Some((phantom, 6000)));
        while scene.ticks < phantom + 5999 {
            tick(&mut scene, &mut caches);
        }
        assert!(scene.objects[i].combat.as_ref().unwrap().phantom > 0.0);
        tick(&mut scene, &mut caches);
        let c = scene.objects[i].combat.as_ref().unwrap();
        assert!(c.phantom_span.is_none() && c.phantom == 0.0);
    }
}
