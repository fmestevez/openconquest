//! Buildings set on fire and their rubble (LandObjClassTy): a flaming blow
//! lights `FIRE<size>` at its hit point (the drain 0x423d60, blow 0x138e,
//! 0x4240e0-0x4242e6), the building's tick plays it once with a smoke at
//! its end (0x425990, message 0), and the fall leaves rubble tiles on map
//! layer 5 (0x138b, 0x4244a4-0x42456d, 0x527fe0).

use super::*;
use openconquest_formats::map::TileRef;

/// The ground's groups (0x5c2480, read by 0x527eb0): a layer-0 variant's
/// group is the 1-based index of the five bytes holding it, a group's
/// scan stopping at a 0 byte.
#[rustfmt::skip]
pub(super) const GROUPS: [[u8; 5]; 19] = [
    [0x01, 0x00, 0x00, 0x00, 0x00],
    [0x02, 0x03, 0x04, 0x05, 0x00],
    [0x06, 0x00, 0x00, 0x00, 0x00],
    [0x07, 0x08, 0x09, 0x0a, 0x00],
    [0x0b, 0x00, 0x00, 0x00, 0x00],
    [0x0c, 0x0d, 0x0e, 0x0f, 0x00],
    [0x10, 0x00, 0x00, 0x00, 0x00],
    [0x11, 0x12, 0x13, 0x14, 0x00],
    [0x15, 0x00, 0x00, 0x00, 0x00],
    [0x16, 0x17, 0x00, 0x00, 0x00],
    [0x18, 0x19, 0x00, 0x00, 0x00],
    [0x1a, 0x1b, 0x00, 0x00, 0x00],
    [0x1c, 0x1d, 0x00, 0x00, 0x00],
    [0x1e, 0x1f, 0x00, 0x00, 0x00],
    [0x20, 0x21, 0x00, 0x00, 0x00],
    [0x22, 0x23, 0x24, 0x00, 0x00],
    [0x25, 0x26, 0x27, 0x28, 0x29],
    [0x2a, 0x00, 0x00, 0x00, 0x00],
    [0x2b, 0x00, 0x00, 0x00, 0x00],
];

/// The group of layer-0 variant `v` (0x527eb0), 0 for none.
pub(super) fn ground_group(v: u8) -> u8 {
    GROUPS
        .iter()
        .position(|g| g.iter().take_while(|&&b| b != 0).any(|&b| b == v))
        .map_or(0, |i| i as u8 + 1)
}

/// The smoke starts this many frames before the fire's end (0x425990).
const SMOKE_LEAD: usize = 14;

/// A picture a building's tick plays once (a fire or smoke layer: delay
/// `+0x7d` 1, time `+0x81`, frame `+0x45`), at its top left.
pub struct Burn {
    pub x: i32,
    pub y: i32,
    pub frame: usize,
    time: u32,
    pub frames: Vec<Vec<objects::Layer>>,
}

impl Burn {
    fn size(&self) -> (i32, i32) {
        self.frames
            .first()
            .and_then(|f| f.first())
            .map_or((0, 0), |l| (l.image.width as i32, l.image.height as i32))
    }

    /// Its step (`time + delay <= now`: the next frame, the last one
    /// ending it); whether it still plays.
    fn step(&mut self, now: u32) -> Option<bool> {
        if self.time + 1 > now {
            return None;
        }
        self.time = now;
        self.frame += 1;
        Some(self.frame < self.frames.len())
    }
}

/// A building's fire (slot 3, layer 6: the lighting takes the last slot
/// it passes, and passes all four only when none burns) and its smoke
/// (layer 10), with the fire's size byte (`+0xa3`).
pub struct BuildingFire {
    pub building: u32,
    pub size: u8,
    pub fire: Option<Burn>,
    pub smoke: Option<Burn>,
}

/// A building's fire as a save keeps it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedBuildingFire {
    building: u32,
    size: u8,
    fire: Option<(i32, i32, usize, u32)>,
    smoke: Option<(i32, i32, usize, u32)>,
}

fn fire_frames(data: &GameData, caches: &mut Caches, size: u8) -> Vec<Vec<objects::Layer>> {
    // Its colours 1-7 through the blend table 0x5c1858 (0x50e240(L, 1,
    // 0x5c1858, 1, 7)).
    caches
        .marked_frames(data, &format!("FIRE{size}"), None, true)
        .unwrap_or_default()
}

fn smoke_frames(data: &GameData, caches: &mut Caches, size: u8) -> Vec<Vec<objects::Layer>> {
    // Colour 0x26 as shadow (0x50e130(L, 1, 0x26, 0x5c1358)).
    caches
        .shaded_frames(data, &format!("SMOKE{size}"), 0x26)
        .unwrap_or_default()
}

/// The cells of a building's site (`+0x56`: 4 for settlements and towns,
/// two to a row from its cell, else 1).
fn site_cells(class: u32) -> &'static [(i32, i32)] {
    match class {
        0x32 | 0x33 => &[(0, 0), (1, 0), (0, 1), (1, 1)],
        _ => &[(0, 0)],
    }
}

impl Scene {
    /// A blow of bomb kind `kind`, `flaming` or not, from player `by` lands
    /// on building `i` at `(hx, hy)` (0x423d60 at 0x4240e0, after the
    /// damage): it lights a fire when the blow is flaming, the building
    /// has no magic shield (`+0x15c`), kind / 3 is 1 or more, the building
    /// stands (`+0xa4` 1), the player is someone's, the hit point less the
    /// main picture's top left is a set bit of its mask (`<prefix>MSK_<
    /// nation>11`, 0x422e30; 0x557640: bit `(row x ((w + 7) & ~7) + x) ^
    /// 7`) and none of its fires burns (0x424153); then one draw of its
    /// generator, `(r >> 16) % m == 0` with `m` 5 for kind / 3 == 1, else
    /// 3. The fire `FIRE<kind / 3 for kinds 0-8, else 2>` stands with its
    /// top left at the hit point less half its width and four fifths of
    /// its height. (Not decoded: the classes with no mask resource, towns,
    /// forts, ports and docks, would read a null mask in 0x557640; the
    /// port lights none on them. A hit point above or left of the picture
    /// reads before the mask: none here.)
    #[allow(clippy::too_many_arguments)]
    pub(super) fn light_building(
        &mut self,
        i: usize,
        kind: u8,
        flaming: bool,
        (hx, hy): (i32, i32),
        by: Option<Owner>,
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
        if !flaming
            || c.protect_magic > 0.0
            || kind / 3 == 0
            || by.is_none()
            || self.construction_at(o.id).is_some()
        {
            return;
        }
        let Some((left, top)) = o
            .frames
            .first()
            .and_then(|f| f.first())
            .map(|l| crate::render::layer_origin(o, l))
        else {
            return;
        };
        let nation = o
            .record
            .field(openconquest_formats::mission::field::NATION)
            .unwrap_or(0);
        let Some(mask) = building_mask(data, class, nation) else {
            return;
        };
        let (mx, my) = (hx - left, hy - top);
        let stride = (mask.width as i32 + 7) & !7;
        if mx < 0 || my < 0 || !mask.bit((my * stride + mx) as usize) {
            return;
        }
        let id = o.id;
        if self
            .building_fires
            .iter()
            .any(|f| f.building == id && f.fire.is_some())
        {
            return;
        }
        let m = if kind / 3 == 1 { 5 } else { 3 };
        if self.building_draw(i) % m != 0 {
            return;
        }
        let size = if kind <= 8 { kind / 3 } else { 2 };
        let frames = fire_frames(data, caches, size);
        let Some((w, h)) = frames
            .first()
            .and_then(|f| f.first())
            .map(|l| (l.image.width as i32, l.image.height as i32))
        else {
            return;
        };
        let fire = Burn {
            x: hx - w / 2,
            y: hy - (4 * h) / 5,
            frame: 0,
            time: 0,
            frames,
        };
        match self.building_fires.iter_mut().find(|f| f.building == id) {
            Some(f) => {
                f.size = size;
                f.fire = Some(fire);
            }
            None => self.building_fires.push(BuildingFire {
                building: id,
                size,
                fire: Some(fire),
                smoke: None,
            }),
        }
    }

    /// The buildings' fires a tick (0x425990, message 0): each steps a
    /// frame a tick and goes out after its last; from frame count - 14
    /// on, its smoke slot free, a smoke `SMOKE<size>` starts below its
    /// middle (x `fire.x + fire.w / 2 - smoke.w / 2`, y `fire.y + (3
    /// fire.h) / 4 - smoke.h`) and steps the same tick; the smoke plays
    /// once. Gone with the building or its fall (0x4243a2, message 3).
    pub(super) fn step_building_fires(&mut self, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let standing: HashSet<u32> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Building(_)) && o.combat.is_some())
            .map(|o| o.id)
            .collect();
        self.building_fires
            .retain(|f| standing.contains(&f.building));
        for k in 0..self.building_fires.len() {
            let f = &mut self.building_fires[k];
            let mut start_smoke = None;
            if let Some(fire) = f.fire.as_mut() {
                match fire.step(now) {
                    Some(true) => {
                        if fire.frame + SMOKE_LEAD >= fire.frames.len() && f.smoke.is_none() {
                            let (w, h) = fire.size();
                            start_smoke = Some((fire.x + w / 2, fire.y + (h * 3) / 4));
                        }
                    }
                    Some(false) => f.fire = None,
                    None => {}
                }
            }
            if let Some((cx, by)) = start_smoke {
                let size = f.size;
                let frames = smoke_frames(data, caches, size);
                let f = &mut self.building_fires[k];
                let mut smoke = Burn {
                    x: cx,
                    y: by,
                    frame: 0,
                    time: 0,
                    frames,
                };
                let (w, h) = smoke.size();
                smoke.x -= w / 2;
                smoke.y -= h;
                f.smoke = Some(smoke);
            }
            let f = &mut self.building_fires[k];
            if let Some(smoke) = f.smoke.as_mut() {
                // The step shows frame `+0x45`, then counts it: the smoke
                // just started shows frame 0 now.
                if smoke.step(now) == Some(false) {
                    f.smoke = None;
                }
            }
        }
        self.building_fires
            .retain(|f| f.fire.is_some() || f.smoke.is_some());
    }

    /// The fire and smoke on building `id` now: (frame, left, top), the
    /// smoke first, the fire over it.
    pub fn building_fire_on(&self, id: u32) -> Vec<(&[objects::Layer], i32, i32)> {
        let Some(f) = self.building_fires.iter().find(|f| f.building == id) else {
            return Vec::new();
        };
        [f.smoke.as_ref(), f.fire.as_ref()]
            .into_iter()
            .flatten()
            .filter_map(|b| {
                // A step shows frame `+0x45` and then counts it on.
                let shown = b.frame.checked_sub(1)?;
                Some((b.frames.get(shown)?.as_slice(), b.x, b.y))
            })
            .collect()
    }

    /// Building `i` falls (0x138b at 0x4244a4, `+0x158` set): for each
    /// cell of its site two draws of its generator give k = r % 3 + 1 and
    /// s = r % 3 + 1, and 0x527fe0 lays the rubble: on a cell whose layer
    /// 0 is land (nibble 8), its layer 5 empty and its layer-0 variant in
    /// a ground group, the layer-5 tile {the group, tile set 0x22 + 3k +
    /// s} (`b_vor<k>_<s|m|b>`), the cell redrawn (message 0xcc2 to the
    /// mapas: the terrain and the minimap).
    pub(super) fn lay_rubble(&mut self, i: usize, data: &GameData, caches: &mut Caches) {
        let o = &self.objects[i];
        let (ObjectClass::Building(class), Some((cx, cy))) = (o.class, o.cell) else {
            return;
        };
        let mut changed = Vec::new();
        for &(dx, dy) in site_cells(class) {
            let k = self.building_draw(i) % 3 + 1;
            let s = self.building_draw(i) % 3 + 1;
            let (x, y) = (cx + dx, cy + dy);
            if x < 0 || y < 0 || x as u32 >= self.grid.width || y as u32 >= self.grid.height {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            let types = self.cell_types[(y * self.grid.width + x) as usize];
            if types & 0xf != 8 || types & 0xf0_0000 != 0 {
                continue;
            }
            let g = ground_group(self.grid.get(0, x, y).variant);
            if g == 0 {
                continue;
            }
            let t = TileRef {
                variant: g,
                tileset: (0x22 + 3 * k + s) as u8,
            };
            self.grid.set(5, x, y, t);
            changed.push((5, x, y, t));
        }
        self.dug.extend(changed.iter().copied());
        self.after_dig(&changed, data, caches);
    }

    pub(super) fn saved_building_fires(&self) -> Vec<SavedBuildingFire> {
        let save = |b: &Burn| (b.x, b.y, b.frame, b.time);
        self.building_fires
            .iter()
            .map(|f| SavedBuildingFire {
                building: f.building,
                size: f.size,
                fire: f.fire.as_ref().map(save),
                smoke: f.smoke.as_ref().map(save),
            })
            .collect()
    }

    pub(super) fn restore_building_fires(
        &mut self,
        fires: &[SavedBuildingFire],
        data: &GameData,
        caches: &mut Caches,
    ) {
        for f in fires {
            let fire = f.fire.map(|(x, y, frame, time)| Burn {
                x,
                y,
                frame,
                time,
                frames: fire_frames(data, caches, f.size),
            });
            let smoke = f.smoke.map(|(x, y, frame, time)| Burn {
                x,
                y,
                frame,
                time,
                frames: smoke_frames(data, caches, f.size),
            });
            self.building_fires.push(BuildingFire {
                building: f.building,
                size: f.size,
                fire,
                smoke,
            });
        }
    }
}

/// A building's mask, `<prefix>MSK_<nation>11` (`%sMSK_%1u%1u%1u`, the
/// prefix `+0xb8`, 0x422e30 in state 1); none for classes without one.
pub(super) fn building_mask(
    data: &GameData,
    class: u32,
    nation: u32,
) -> Option<openconquest_formats::Mask> {
    use openconquest_formats::ResourceKind;
    let name = objects::building_name(class, nation, 0, objects::BuildingState::Standing(3))?;
    let prefix = &name[..name.find("SPR_")?];
    let lib = &data.objects;
    let e = lib.find(&format!("{prefix}MSK_{nation}11"), ResourceKind::Mask)?;
    let bits = lib.read_entry(e).ok()?;
    openconquest_formats::Mask::parse(&bits, e.width.into(), e.height.into()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::Caches;

    fn exe() -> Option<Vec<u8>> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        let path = crate::assets::find_ci(std::path::Path::new(&root), "ancient.exe")
            .unwrap()
            .unwrap();
        Some(std::fs::read(path).unwrap())
    }

    /// 0x5c1858 (the flames' blend), 0x5c1358 (the shadow table) and
    /// 0x5c2480 (the ground's groups) as the exe has them.
    #[test]
    fn flame_blend_and_ground_groups_are_the_exes() {
        let Some(bytes) = exe() else {
            return;
        };
        let at = 0x5c1858 - 0x40_0000;
        assert_eq!(crate::render::FLAME_BLEND.concat(), bytes[at..at + 7 * 256]);
        let at = 0x5c1358 - 0x40_0000;
        assert_eq!(crate::render::SHADOW_TABLE, bytes[at..at + 256]);
        let at = 0x5c2480 - 0x40_0000;
        assert_eq!(GROUPS.concat(), bytes[at..at + 19 * 5]);
        assert_eq!(ground_group(0x01), 1);
        assert_eq!(ground_group(0x05), 2);
        assert_eq!(ground_group(0x29), 17);
        assert_eq!(ground_group(0x2b), 19);
        assert_eq!(ground_group(0x2c), 0);
        assert_eq!(ground_group(0x00), 0);
    }

    /// A building with a mask, standing on land with an empty layer 5,
    /// in some campaign mission.
    fn masked_building(data: &GameData, caches: &mut Caches) -> (Scene, usize) {
        for n in 0..30 {
            let Ok(scene) = Scene::load(data, caches, &format!("miss{n:03}")) else {
                continue;
            };
            let found = scene.objects.iter().position(|o| {
                let ObjectClass::Building(class) = o.class else {
                    return false;
                };
                let nation = o.record.field(openconquest_formats::mission::field::NATION);
                let land = o.cell.is_some_and(|(x, y)| {
                    let t = scene.cell_types[(y as u32 * scene.grid.width + x as u32) as usize];
                    t & 0xf == 8
                        && t & 0xf0_0000 == 0
                        && ground_group(scene.grid.get(0, x as u32, y as u32).variant) != 0
                });
                o.combat.is_some()
                    && o.asleep.is_none()
                    && land
                    && building_mask(data, class, nation.unwrap_or(0)).is_some()
            });
            if let Some(i) = found {
                return (scene, i);
            }
        }
        panic!("no masked building on land");
    }

    /// A flaming blow of kind 3-5 on a set bit of the mask lights FIRE1
    /// when the building's draw is 0 mod 5, at the hit point less w / 2
    /// and 4h / 5; it burns a frame a tick, its smoke from count - 14 on,
    /// and leaves the slot free; a blow on a burning building or a draw
    /// not 0 mod 5 lights none (0x4240e0, 0x425990).
    #[test]
    fn a_flaming_blow_lights_a_building_fire() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let (mut scene, i) = masked_building(&data, &mut caches);
        let o = &scene.objects[i];
        let ObjectClass::Building(class) = o.class else {
            unreachable!()
        };
        let nation = o
            .record
            .field(openconquest_formats::mission::field::NATION)
            .unwrap();
        let mask = building_mask(&data, class, nation).unwrap();
        let l = &o.frames[0][0];
        let (left, top) = crate::render::layer_origin(o, l);
        let stride = (mask.width + 7) & !7;
        let (mx, my) = (0..mask.height)
            .flat_map(|y| (0..mask.width).map(move |x| (x, y)))
            .find(|&(x, y)| mask.bit((y * stride + x) as usize))
            .unwrap();
        let hit = (left + mx as i32, top + my as i32);
        let id = o.id;
        let by = Some(o.owner.unwrap_or((1, 0)));
        // Seeds whose next draw is (not) 0 mod 5.
        let seed = |zero: bool| {
            (1u32..)
                .find(|&v| (combat::Rng(v).next() % 5 == 0) == zero)
                .unwrap()
        };
        // Not flaming, or of kind 0-2: no draw.
        scene.building_lcgs.insert(id, seed(true));
        scene.light_building(i, 4, false, hit, by, &data, &mut caches);
        scene.light_building(i, 2, true, hit, by, &data, &mut caches);
        assert_eq!(scene.building_lcgs[&id], seed(true));
        // A draw not 0 mod 5: drawn, no fire.
        scene.building_lcgs.insert(id, seed(false));
        scene.light_building(i, 4, true, hit, by, &data, &mut caches);
        assert_ne!(scene.building_lcgs[&id], seed(false));
        assert!(scene.building_fires.is_empty());
        // Under a magic shield (`+0x15c`): no draw, no fire.
        scene.building_lcgs.insert(id, seed(true));
        scene.objects[i].combat.as_mut().unwrap().protect_magic = 10.0;
        scene.light_building(i, 4, true, hit, by, &data, &mut caches);
        assert_eq!(scene.building_lcgs[&id], seed(true));
        assert!(scene.building_fires.is_empty());
        scene.objects[i].combat.as_mut().unwrap().protect_magic = 0.0;
        // 0 mod 5: FIRE1, a weapon shield (`+0x17c`) no matter.
        scene.objects[i].combat.as_mut().unwrap().protect_weapons = 10.0;
        scene.light_building(i, 4, true, hit, by, &data, &mut caches);
        let f = &scene.building_fires[0];
        assert_eq!(f.size, 1);
        let fire = f.fire.as_ref().unwrap();
        let (w, h) = fire.size();
        assert_eq!((fire.x, fire.y), (hit.0 - w / 2, hit.1 - 4 * h / 5));
        let count = fire.frames.len();
        assert!(count > 14);
        assert!(fire.frames[0][0]
            .image
            .pixels
            .iter()
            .flatten()
            .any(|&p| p & crate::assets::BLEND != 0));
        // Burning: no further draw.
        let lcg = scene.building_lcgs[&id];
        scene.light_building(i, 4, true, hit, by, &data, &mut caches);
        assert_eq!(scene.building_lcgs[&id], lcg);
        // A frame a tick; the smoke from count - 14.
        let mut smoke_at = None;
        for t in 1..=count as u32 {
            scene.ticks += 1;
            scene.step_building_fires(&data, &mut caches);
            let f = &scene.building_fires[0];
            if let Some(fire) = &f.fire {
                assert_eq!(fire.frame, t as usize);
            }
            if let (None, Some(s)) = (smoke_at, f.smoke.as_ref()) {
                smoke_at = Some(t as usize);
                let fire = f.fire.as_ref().unwrap();
                let ((fw, fh), (sw, sh)) = (fire.size(), s.size());
                assert_eq!(
                    (s.x, s.y),
                    (fire.x + fw / 2 - sw / 2, fire.y + (3 * fh) / 4 - sh)
                );
                assert_eq!(s.frame, 1, "steps the tick it starts");
            }
        }
        assert_eq!(smoke_at, Some(count - 14));
        let f = &scene.building_fires[0];
        assert!(f.fire.is_none(), "out after its last frame");
        let smoke_count = f.smoke.as_ref().unwrap().frames.len();
        // The smoke plays on to its end.
        for _ in 0..smoke_count {
            scene.ticks += 1;
            scene.step_building_fires(&data, &mut caches);
        }
        assert!(scene.building_fires.is_empty());
    }

    /// The fall lays `b_vor<k>_<s|m|b>` on layer 5 with the ground's
    /// group, k and s two draws of the building's generator each r % 3 +
    /// 1 (0x4244a4, 0x527fe0).
    #[test]
    fn a_fallen_building_leaves_rubble() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let (mut scene, i) = masked_building(&data, &mut caches);
        let id = scene.objects[i].id;
        let (x, y) = scene.objects[i].cell.unwrap();
        let (x, y) = (x as u32, y as u32);
        scene.building_lcgs.insert(id, 12345);
        let mut r = combat::Rng(12345);
        let k = r.next() % 3 + 1;
        let s = r.next() % 3 + 1;
        let g = ground_group(scene.grid.get(0, x, y).variant);
        scene.lay_rubble(i, &data, &mut caches);
        let t = scene.grid.get(5, x, y);
        assert_eq!((t.variant, t.tileset as u32), (g, 0x22 + 3 * k + s));
        let name = data.tilesets.by_id(t.tileset).unwrap().tile_name(t.variant);
        assert!(name.starts_with("b_vor"), "{name}");
        let w = scene.grid.width;
        assert_eq!(scene.cell_types[(y * w + x) as usize] >> 20 & 0xf, 2);
        assert_eq!(scene.dug.last().map(|d| (d.0, d.1, d.2)), Some((5, x, y)));
        // A second fall on the cell lays nothing (layer 5 taken).
        scene.lay_rubble(i, &data, &mut caches);
        assert_eq!(scene.grid.get(5, x, y), t);
    }
}
