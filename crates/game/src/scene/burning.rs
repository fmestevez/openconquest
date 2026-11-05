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
    /// nation>1<level>`, its damage level after the blow, 0x422e30;
    /// 0x557640: bit `(row x ((w + 7) & ~7) + x) ^ 7`) and none of its
    /// fires burns (0x424153); then one draw of its
    /// generator, `(r >> 16) % m == 0` with `m` 5 for kind / 3 == 1, else
    /// 3. The fire `FIRE<kind / 3 for kinds 0-8, else 2>` stands with its
    /// top left at the hit point less half its width and four fifths of
    /// its height. Forts (their drain 0x4184f0 leaves 0x138e to 0x4e7550
    /// -> 0x427bb0 -> 0x423d60) and towns (0x43b290 leaves it to 0x423d60)
    /// burn the same, on their own masks (`building_mask`), and so do
    /// ports (drain 0x4e7550 -> 0x427bb0 -> 0x423d60), shipyards and docks
    /// (0x416340 -> 0x4e7550 -> ...), on the masks 0x4e7f90 loads; their
    /// blows come in by 0x423c30 (the dock's vt+4 0x4162f0 calls it), which
    /// queues 0x138e unless the state is 2. Barbarian structures too
    /// (their drain 0x40e0f0 -> 0x4e7550 -> ...; vt+4 0x423c30), drawing
    /// their building generator. (Not decoded: the classes with
    /// no mask resource would read a null mask in 0x557640; the port
    /// lights none on them. A hit point above or left of the picture
    /// reads before the mask: none here.) Castles burn the same (their
    /// drain 0x4105c0 -> 0x427bb0 -> 0x423d60) on `CSTLSEAMSK_1<level>` /
    /// `CSTLWARMSK_1<level>` (0x4101d0 at 0x4102d9, `%sMSK_%1u%1u`), the
    /// draw of their own generator (castle.rs), and only standing (state
    /// 1): casting (4) takes none. Lairs burn the same (their drain
    /// 0x414ef0 jumps to 0x423d60) on `<CLPF|MNTF|AMZF>MSK_41<level>`
    /// (LandObj's 0x422e30, nation 4), the draw of their generator (the
    /// building one, drawn too by their tick 0x414f00, walkers.rs, and the
    /// aim at them 0x423520).
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
        let class = match o.class {
            // The test wants `+0xa4` 1 (0x42410e): not going up (0), fallen
            // (2, 0x423d60 on 0x138b; the port's ruins keep no combat),
            // sold (3) nor asleep (0x10). Being mended is state 1 (0x1389
            // sets `+0x10c` only), so a building being mended burns. (An
            // asleep building is never hit: its vt+8 0x423460 is false in
            // 0x10, so no shot lands.)
            ObjectClass::Building(class) | ObjectClass::Barbarian(class) if self.standing(o.id) => {
                class
            }
            // A castle stands in state 1 only: casting (4) it takes none.
            ObjectClass::Castle(class) if self.castle_standing(o.id) => class,
            ObjectClass::CreatureSpawn => o.record.class_id,
            _ => return,
        };
        let Some(c) = o.combat.as_ref() else {
            return;
        };
        if !flaming || c.protect_magic > 0.0 || kind / 3 == 0 || by.is_none() {
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
        let objects::BuildingState::Standing(level) =
            objects::BuildingState::for_condition(c.hull / c.max_hull * 100.0)
        else {
            unreachable!()
        };
        let mask = if matches!(o.class, ObjectClass::Castle(_)) {
            castle_mask(data, class, level)
        } else if o.class == ObjectClass::CreatureSpawn {
            lair_mask(data, class, level)
        } else {
            let facing = o.record.field(10).unwrap_or(0);
            building_mask(data, class, nation, facing, level)
        };
        let Some(mask) = mask else {
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
    /// smoke first, the fire over it, each with whether it is the fire.
    pub fn building_fire_on(&self, id: u32) -> Vec<(&[objects::Layer], i32, i32, bool)> {
        let Some(f) = self.building_fires.iter().find(|f| f.building == id) else {
            return Vec::new();
        };
        [(f.smoke.as_ref(), false), (f.fire.as_ref(), true)]
            .into_iter()
            .filter_map(|(b, fire)| {
                let b = b?;
                // A step shows frame `+0x45` and then counts it on.
                let shown = b.frame.checked_sub(1)?;
                Some((b.frames.get(shown)?.as_slice(), b.x, b.y, fire))
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

/// A building's mask at damage level `level` (3 intact .. 1),
/// `<prefix>MSK_<nation>1<level>` (`%sMSK_%1u%1u%1u`, the prefix `+0xb8`,
/// the nation `+0x54`, 1, the level its picture's: 0x422e30 in state 1,
/// 0x422f4d pushing the level of 0x422ef7, run again by 0x423d60 when a
/// blow changes the level; the forts' prefixes `FORTS`, `FORT`, `FORTB`,
/// 0x41a0e6-0x41a141); a town's `TMSK_<nation><class>1<level>` (TownObj's
/// own LoadImages 0x43bd60, the same level; its drain 0x43b290 leaves
/// 0x138e to 0x423d60); a port's, shipyard's or dock's `PTMSK_` /
/// `DKMSK_<nation><class>1<facing><level>` (0x4e7f90, its level
/// likewise, the facing the record's field 10); none for classes
/// without one.
/// A castle's mask (0x4101d0 at 0x4102d9, `%sMSK_%1u%1u`, loaded in
/// state 1): its prefix, 1, the damage level, so `CSTLWARMSK_13`.
fn castle_mask(data: &GameData, class: u32, level: u32) -> Option<openconquest_formats::Mask> {
    use openconquest_formats::ResourceKind;
    let name = format!("{}MSK_1{level}", super::castle::prefix(class));
    let lib = &data.objects;
    let e = lib.find(&name, ResourceKind::Mask)?;
    let bits = lib.read_entry(e).ok()?;
    openconquest_formats::Mask::parse(&bits, e.width.into(), e.height.into()).ok()
}

/// A lair's mask (0x422e30: its prefix `+0xb8`, nation 4, 1, the damage
/// level), so `CLPFMSK_413`.
fn lair_mask(data: &GameData, class: u32, level: u32) -> Option<openconquest_formats::Mask> {
    use openconquest_formats::ResourceKind;
    let prefix = match class {
        0x46 => "CLPF",
        0x47 => "MNTF",
        _ => "AMZF",
    };
    let name = format!("{prefix}MSK_41{level}");
    let lib = &data.objects;
    let e = lib.find(&name, ResourceKind::Mask)?;
    let bits = lib.read_entry(e).ok()?;
    openconquest_formats::Mask::parse(&bits, e.width.into(), e.height.into()).ok()
}

pub(super) fn building_mask(
    data: &GameData,
    class: u32,
    nation: u32,
    facing: u32,
    level: u32,
) -> Option<openconquest_formats::Mask> {
    use openconquest_formats::ResourceKind;
    let name = match class {
        // TownObj's own (0x43bd60, 0x43be6c): `TMSK_%1u%2u%1u%1u`, the
        // nation, the class `+0x50`, 1, the level.
        0x32 | 0x33 => format!("TMSK_{nation}{class:2}1{level}"),
        // Ports, shipyards and docks load theirs in 0x4e7f90 in state 1
        // (0x4e80b5, `%sMSK_%1u%2u%1u%1u%1u`): the prefix `+0xb8` (`PT`
        // 0x5ad098, `DK` 0x5a2cf0), the nation, the class `+0x50`, 1, the
        // facing `+0x1dc`, the level.
        0x39 => format!("PTMSK_{nation}{class:2}1{facing}{level}"),
        0x3a | 0x3b => format!("DKMSK_{nation}{class:2}1{facing}{level}"),
        // The barbarian structure 0x40 likewise (its vt+0x38 0x40e0d0
        // jumps to 0x4e7f90 for class 0x40; prefix `BARB` 0x5a2910 set at
        // 0x40f014, the facing record field 10 by 0x427f50 at 0x4280f2
        // with `+0x1d8` 0xa), so `BARBMSK_464<1><facing><level>`; 0x3f
        // and 0x41 go to 0x422e30, as lairs (`BARBSMSK_41<level>`,
        // `BARBBMSK_41<level>`). Their blows reach the fire test: the
        // drain 0x40e0f0 writes the structure's rest back (0x55b440) and
        // ends in 0x4e7550 -> 0x427bb0 -> 0x423d60, the draw from their
        // generator `+0x18`, the building one (0x427f50 seeds it through
        // 0x40ef70 -> 0x4e8660 -> 0x4e7640).
        0x40 => format!("BARBMSK_{nation}{class:2}1{facing}{level}"),
        _ => {
            let name =
                objects::building_name(class, nation, 0, objects::BuildingState::Standing(3))?;
            let prefix = &name[..name.find("SPR_")?];
            format!("{prefix}MSK_{nation}1{level}")
        }
    };
    let lib = &data.objects;
    let e = lib.find(&name, ResourceKind::Mask)?;
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
    fn masked_building(
        data: &GameData,
        caches: &mut Caches,
        pick: impl Fn(u32) -> bool,
    ) -> (Scene, usize) {
        for n in 0..30 {
            let Ok(scene) = Scene::load(data, caches, &format!("miss{n:03}")) else {
                continue;
            };
            let found = scene.objects.iter().position(|o| {
                let ObjectClass::Building(class) = o.class else {
                    return false;
                };
                let nation = o.record.field(openconquest_formats::mission::field::NATION);
                let facing = o.record.field(10).unwrap_or(0);
                let land = o.cell.is_some_and(|(x, y)| {
                    let t = scene.cell_types[(y as u32 * scene.grid.width + x as u32) as usize];
                    t & 0xf == 8
                        && t & 0xf0_0000 == 0
                        && ground_group(scene.grid.get(0, x as u32, y as u32).variant) != 0
                });
                pick(class)
                    && o.combat.is_some()
                    && o.asleep.is_none()
                    && land
                    && building_mask(data, class, nation.unwrap_or(0), facing, 3).is_some()
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
        let (mut scene, i) = masked_building(&data, &mut caches, |_| true);
        let o = &scene.objects[i];
        let ObjectClass::Building(class) = o.class else {
            unreachable!()
        };
        let nation = o
            .record
            .field(openconquest_formats::mission::field::NATION)
            .unwrap();
        let mask = building_mask(&data, class, nation, o.record.field(10).unwrap_or(0), 3).unwrap();
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

    /// Towns read `TMSK_<nation><class>1<level>` (0x43bd60), forts
    /// `<FORTS|FORT|FORTB>MSK_<nation>1<level>` (0x422e30) and ports and
    /// docks `<PT|DK>MSK_<nation><class>1<facing><level>` (0x4e7f90); a
    /// flaming blow of kind 6 on a set bit with a draw 0 mod 3 lights FIRE2
    /// on them, none going up, fallen, sold or asleep (`+0xa4` not 1,
    /// 0x42410e); being mended (state 1, `+0x10c`) they burn.
    #[test]
    fn towns_and_forts_take_fire() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let named = |class, nation, level| {
            building_mask(&data, class, nation, 0, level).map(|m| (m.width, m.height))
        };
        assert!(named(0x32, 1, 1).is_some() && named(0x33, 3, 3).is_some());
        assert!(named(0x34, 1, 2).is_some() && named(0x36, 3, 1).is_some());
        // Ports, shipyards and docks by facing (0x4e7f90).
        for facing in [0, 2, 4, 6] {
            for class in 0x39..=0x3b {
                assert!(building_mask(&data, class, 1, facing, 1).is_some());
            }
        }
        // Barbarian structures: 0x40 by facing (0x4e7f90), 0x3f and 0x41
        // as lairs (0x422e30).
        for level in 1..=3 {
            for facing in [0, 2, 4, 6] {
                assert!(building_mask(&data, 0x40, 4, facing, level).is_some());
            }
            assert!(building_mask(&data, 0x3f, 4, 0, level).is_some());
            assert!(building_mask(&data, 0x41, 4, 0, level).is_some());
        }
        let picks: [fn(u32) -> bool; 3] = [
            |c| matches!(c, 0x32 | 0x33),
            |c| matches!(c, 0x34..=0x36),
            |c| matches!(c, 0x39..=0x3b),
        ];
        for pick in picks {
            let (mut scene, i) = masked_building(&data, &mut caches, pick);
            let o = &scene.objects[i];
            let ObjectClass::Building(class) = o.class else {
                unreachable!()
            };
            let nation = o
                .record
                .field(openconquest_formats::mission::field::NATION)
                .unwrap();
            let mask =
                building_mask(&data, class, nation, o.record.field(10).unwrap_or(0), 3).unwrap();
            let (left, top) = crate::render::layer_origin(o, &o.frames[0][0]);
            let stride = (mask.width + 7) & !7;
            let (mx, my) = (0..mask.height)
                .flat_map(|y| (0..mask.width).map(move |x| (x, y)))
                .find(|&(x, y)| mask.bit((y * stride + x) as usize))
                .unwrap();
            let hit = (left + mx as i32, top + my as i32);
            let (id, by) = (o.id, Some(o.owner.unwrap_or((1, 0))));
            let seed = (1u32..).find(|&v| combat::Rng(v).next() % 3 == 0).unwrap();
            scene.building_lcgs.insert(id, seed);
            // Asleep (0x10) it takes none, with no draw.
            scene.objects[i].asleep = Some(0.0);
            scene.light_building(i, 6, true, hit, by, &data, &mut caches);
            scene.objects[i].asleep = None;
            assert_eq!(scene.building_lcgs[&id], seed);
            assert!(scene.building_fires.is_empty());
            // Being mended it stands (`+0xa4` 1; the mending is `+0x10c`,
            // 0x423d60 on 0x1389) and burns.
            scene.mending.push(id);
            scene.light_building(i, 6, true, hit, by, &data, &mut caches);
            scene.mending.clear();
            assert_ne!(scene.building_lcgs[&id], seed);
            let f = &scene.building_fires[0];
            assert_eq!((f.building, f.size), (id, 2));
            assert!(f.fire.is_some());
        }
    }

    /// miss009's war castle takes fire on its mask `CSTLWARMSK_13` by a
    /// draw of its own generator while it stands (state 1), none while it
    /// casts (4), with no draw (needs `OPENCONQUEST_DATA`).
    #[test]
    fn a_standing_castle_takes_fire() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
        let i = scene
            .objects
            .iter()
            .position(|o| o.class == ObjectClass::Castle(0x43))
            .unwrap();
        let mask = castle_mask(&data, 0x43, 3).unwrap();
        let o = &scene.objects[i];
        let (left, top) = crate::render::layer_origin(o, &o.frames[0][0]);
        let stride = (mask.width + 7) & !7;
        let (mx, my) = (0..mask.height)
            .flat_map(|y| (0..mask.width).map(move |x| (x, y)))
            .find(|&(x, y)| mask.bit((y * stride + x) as usize))
            .unwrap();
        let hit = (left + mx as i32, top + my as i32);
        let id = o.id;
        let seed = (1u32..).find(|&v| combat::Rng(v).next() % 3 == 0).unwrap();
        scene.building_lcgs.insert(id, seed);
        scene.castles.get_mut(&id).unwrap().state = 4;
        scene.light_building(i, 6, true, hit, Some((1, 0)), &data, &mut caches);
        assert_eq!(scene.building_lcgs[&id], seed);
        assert!(scene.building_fires.is_empty());
        scene.castles.get_mut(&id).unwrap().state = 1;
        scene.light_building(i, 6, true, hit, Some((1, 0)), &data, &mut caches);
        assert_ne!(scene.building_lcgs[&id], seed);
        let f = &scene.building_fires[0];
        assert_eq!((f.building, f.size), (id, 2));
        assert!(f.fire.is_some());
    }

    /// miss001's barbarian structures: their generator is the building
    /// one, seeded `[game +0x2ee5] x id` by 0x427f50 (through 0x40ef70 ->
    /// 0x4e8660 -> 0x4e7640), then drawn once for the unit list's
    /// generator (0x40f5a5), which the first tick's steps start from;
    /// each takes fire on its mask by a draw of it, none off the mask,
    /// with no draw (needs `OPENCONQUEST_DATA`).
    #[test]
    fn barbarian_structures_draw_their_building_generator_and_burn() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss001").unwrap();
        let ks: Vec<usize> = (0..scene.objects.len())
            .filter(|&k| matches!(scene.objects[k].class, ObjectClass::Barbarian(_)))
            .collect();
        assert!(ks.len() >= 2);
        let game = scene.game_seed();
        for &k in &ks {
            assert!(!scene.building_lcgs.contains_key(&scene.objects[k].id));
        }
        scene.step_fort_draws();
        for &k in &ks {
            let id = scene.objects[k].id;
            let mut r = combat::Rng(game.wrapping_mul(id));
            let list = r.next();
            assert_eq!(scene.building_lcgs[&id], r.0);
            let c = scene.objects[k].combat.as_ref().unwrap();
            let s = 0xa704_27df_u32.wrapping_sub(list.wrapping_mul(0x7f82_434b));
            assert_eq!(c.fire.draws[0], s >> 16);
        }
        for &k in &ks {
            let o = &scene.objects[k];
            let ObjectClass::Barbarian(class) = o.class else {
                unreachable!()
            };
            let facing = o.record.field(10).unwrap_or(0);
            let mask = building_mask(&data, class, 4, facing, 3).unwrap();
            let (left, top) = crate::render::layer_origin(o, &o.frames[0][0]);
            let stride = (mask.width + 7) & !7;
            let (mx, my) = (0..mask.height)
                .flat_map(|y| (0..mask.width).map(move |x| (x, y)))
                .find(|&(x, y)| mask.bit((y * stride + x) as usize))
                .unwrap();
            let id = o.id;
            let seed = (1u32..).find(|&v| combat::Rng(v).next() % 3 == 0).unwrap();
            scene.building_lcgs.insert(id, seed);
            let off = (left - 1, top);
            scene.light_building(k, 6, true, off, Some((1, 0)), &data, &mut caches);
            assert_eq!(scene.building_lcgs[&id], seed);
            assert!(!scene.building_fires.iter().any(|f| f.building == id));
            let hit = (left + mx as i32, top + my as i32);
            scene.light_building(k, 6, true, hit, Some((1, 0)), &data, &mut caches);
            assert_ne!(scene.building_lcgs[&id], seed);
            let f = scene.building_fires.iter().find(|f| f.building == id);
            assert!(f.is_some_and(|f| f.size == 2 && f.fire.is_some()));
        }
    }

    /// miss000's lair takes fire on its mask (`<prefix>MSK_413`, nation
    /// 4) by a draw of its generator, none off it, with no draw (needs
    /// `OPENCONQUEST_DATA`).
    #[test]
    fn a_lair_takes_fire() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let i = scene
            .objects
            .iter()
            .position(|o| o.class == ObjectClass::CreatureSpawn)
            .unwrap();
        let o = &scene.objects[i];
        let mask = lair_mask(&data, o.record.class_id, 3).unwrap();
        let (left, top) = crate::render::layer_origin(o, &o.frames[0][0]);
        let stride = (mask.width + 7) & !7;
        let (mx, my) = (0..mask.height)
            .flat_map(|y| (0..mask.width).map(move |x| (x, y)))
            .find(|&(x, y)| mask.bit((y * stride + x) as usize))
            .unwrap();
        let hit = (left + mx as i32, top + my as i32);
        let id = o.id;
        let seed = (1u32..).find(|&v| combat::Rng(v).next() % 3 == 0).unwrap();
        scene.building_lcgs.insert(id, seed);
        scene.light_building(
            i,
            6,
            true,
            (left - 1, top),
            Some((1, 0)),
            &data,
            &mut caches,
        );
        assert_eq!(scene.building_lcgs[&id], seed);
        assert!(scene.building_fires.is_empty());
        scene.light_building(i, 6, true, hit, Some((1, 0)), &data, &mut caches);
        let mut r = combat::Rng(seed);
        r.next();
        assert_eq!(scene.building_lcgs[&id], r.0);
        let f = &scene.building_fires[0];
        assert_eq!(f.building, id);
        assert!(f.fire.is_some());
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
        // One of a single cell (not a town).
        let (mut scene, i) = masked_building(&data, &mut caches, |c| !matches!(c, 0x32 | 0x33));
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
