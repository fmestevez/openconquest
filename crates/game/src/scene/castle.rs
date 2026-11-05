//! The wizards' castles (CastleClassTy, `Castle.cpp`, vtable 0x5949fc,
//! handler 0x410ca0): the sea castle (class 0x42, the wind wizard) casts
//! the waterspout (spell 3), the war castle (0x43, the fire wizard) the
//! fireball (spell 7), at a cell around him picked by what lies there.
//! His tick (message 0) is LandObj's (0x427f50 -> 0x425990, which steps
//! the picture, vt+0x3c 0x4230f0), then his own 0x410790. His generator
//! (`+0x18`) is the building one (`Scene::building_draw`), seeded `[game
//! +0x2ee5] x id` by 0x427f50 at message 2.

use openconquest_formats::ObjectClass;

use super::Scene;
use crate::assets::GameData;
use crate::magic::Spell;
use crate::objects::{self, Caches};

/// The picture's cycles in ticks by state (`+0xc0` = 0x5949e8; state 4
/// reads 0x5949f8).
const CYCLE: [u32; 5] = [300, 20, 100, 75, 64];
/// The cast picture's frame at which he casts (0x410790, `+0x45` 7).
const CAST_FRAME: i32 = 7;
/// Ticks he hurries after a blow (0x410790: `+0x1f8 + 3000`).
const ANGER: u32 = 3000;

/// A castle's wizard: the original's fields, by offset.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Castle {
    /// State `+0xa4`: 1 standing, 4 casting.
    pub state: u8,
    /// The next cast's tick (`+0x1ec`).
    pub next: u32,
    /// His last attacker's id (`+0x1f4`, 0 none) and the blow's tick
    /// (`+0x1f8`).
    pub attacker: u32,
    pub attacked: u32,
    /// The next ambient sound's tick (`+0x1fc`).
    pub sound: u32,
    /// The picture: the frame about to be drawn (`+0x7c` `+0x45`), the
    /// one drawn last, the last step's tick (`+0x81`), and whether it
    /// has stepped since it was set (`+0xbc`).
    pub frame: i32,
    pub drawn: i32,
    pub last: u32,
    pub stepped: bool,
    /// The damage level its picture was loaded at (3-1).
    pub level: u8,
    /// The picture the object holds: state and level (not saved).
    #[serde(skip)]
    art: Option<(u8, u8)>,
}

impl Scene {
    /// Message 2 to every castle with none yet (0x410ca0): fields 22-24
    /// kept (`+0x1e0` level, `+0x1e4` / `+0x1e8` reach); a new one
    /// (record field 4 below 2) draws its first cast's tick, `now +
    /// (1250 - 125 x level) x (r % 21 + 80) / 100` (halved from now if
    /// attacked, never at the start), then every one its ambient sound's,
    /// `now + (r % 41 + 60) x 375 / 100`. Standing (state 1), its picture
    /// set (vt+0x38: frame 0, a step on the next tick).
    pub(super) fn start_castles(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Castle(_)))
            .map(|o| o.id)
            .filter(|id| !self.castles.contains_key(id))
            .collect();
        for id in ids {
            let i = self.object_index(id).unwrap();
            let now = self.ticks;
            let level = self.objects[i].record.field(22).unwrap_or(0);
            let mut c = Castle {
                state: 1,
                ..Castle::default()
            };
            c.next = now.wrapping_add(cast_wait(level, self.building_draw(i)));
            c.sound = now.wrapping_add(sound_wait(self.building_draw(i)));
            self.castle_picture(i, &mut c, data, caches);
            self.castles.insert(id, c);
        }
    }

    /// A blow from `shooter` on castle `target` (its drain 0x4105c0, a
    /// shooter in `+0x16`): he is the wizard's attacker (`+0x1f4`) from
    /// now (`+0x1f8`).
    pub(super) fn wizard_attacked(&mut self, target: u32, shooter: u32) {
        let now = self.ticks;
        if let Some(c) = self.castles.get_mut(&target) {
            c.attacker = shooter;
            c.attacked = now;
        }
    }

    /// The frame castle `id` shows.
    pub(super) fn castle_frame(&self, id: u32) -> Option<u32> {
        self.castles.get(&id).map(|c| c.drawn.max(0) as u32)
    }

    /// Whether castle `id` stands (`+0xa4` 1): the cast (4) bars the fire
    /// (0x4240e0).
    pub(super) fn castle_standing(&self, id: u32) -> bool {
        self.castles.get(&id).is_some_and(|c| c.state == 1)
    }

    /// The castles a game tick.
    pub(super) fn tick_castles(&mut self, data: &GameData, caches: &mut Caches) {
        self.start_castles(data, caches);
        let ids: Vec<u32> = self.castles.keys().copied().collect();
        for id in ids {
            let Some(i) = self.object_index(id) else {
                self.castles.remove(&id);
                continue;
            };
            if self.objects[i]
                .combat
                .as_ref()
                .is_none_or(|c| c.hull <= 0.0)
            {
                continue;
            }
            let mut c = self.castles.remove(&id).unwrap();
            // A blow that changed the damage level set the picture again
            // (0x423d60 -> vt+0x38); the port at the tick after it.
            if self.castle_level(i) != c.level {
                self.castle_picture(i, &mut c, data, caches);
            }
            self.castle_art(i, &mut c, data, caches);
            self.castle_step(i, &mut c, data, caches);
            self.castle_tick(i, &mut c, data, caches);
            self.castles.insert(id, c);
        }
    }

    /// The damage level of castle `i`'s picture: `ceil(+0xa8 x 2 / 67)`,
    /// at least 1 (0x4101d0).
    fn castle_level(&self, i: usize) -> u8 {
        let o = &self.objects[i];
        let percent = o
            .combat
            .as_ref()
            .map_or(100.0, |c| c.hull / c.max_hull * 100.0);
        match objects::BuildingState::for_condition(percent) {
            objects::BuildingState::Standing(l) => l as u8,
            _ => 3,
        }
    }

    /// vt+0x38 (0x4101d0): the state's picture `<prefix>SPR_<state><level>`
    /// (`CSTLSEA` / `CSTLWAR`, `+0xb8`), its frame kept unless past its
    /// last (then 0), the last step's time 0 and a step due (`+0xbc` 0).
    /// (In state 1 it loads the mask `<prefix>MSK_1<level>` too, which the
    /// fire reads, `building_mask`.)
    fn castle_picture(&mut self, i: usize, c: &mut Castle, data: &GameData, caches: &mut Caches) {
        c.level = self.castle_level(i);
        self.castle_art(i, c, data, caches);
        if self.objects[i].frames.len() as i32 <= c.frame {
            c.frame = 0;
        }
        c.last = 0;
        c.stepped = false;
    }

    /// The picture loaded into object `i` when it is not the castle's
    /// state's and level's (a new one, or after a restore).
    fn castle_art(&mut self, i: usize, c: &mut Castle, data: &GameData, caches: &mut Caches) {
        if c.art == Some((c.state, c.level)) {
            return;
        }
        let ObjectClass::Castle(class) = self.objects[i].class else {
            return;
        };
        let name = format!("{}SPR_{}{}", prefix(class), c.state, c.level);
        if let Ok(frames) = objects::sequence_frames(data, caches, &name) {
            if !frames.is_empty() {
                self.objects[i].frames = frames;
            }
        }
        c.art = Some((c.state, c.level));
    }

    /// LandObj's picture step (vt+0x3c, 0x4230f0): when a step is due
    /// (none since the picture was set, or `cycle[state] / n` ticks since
    /// the last), the frame is drawn and one added (vt+0x40 0x427b30 ->
    /// 0x4233f0); past the last, standing (1) goes back to frame 0, and
    /// the cast (4) ends (vt+0x48 0x4106f0: with the cast still due, which
    /// it is when nothing was there to cast at, its next tick drawn as at
    /// a cast) standing at frame 0 with the picture set again.
    fn castle_step(&mut self, i: usize, c: &mut Castle, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let n = self.objects[i].frames.len() as u32;
        let delay = CYCLE[c.state as usize % CYCLE.len()] / n.max(1);
        if now < c.last.wrapping_add(delay) && c.stepped {
            return;
        }
        c.stepped = true;
        c.last = now;
        c.drawn = c.frame;
        c.frame += 1;
        if c.frame < n as i32 && c.frame >= 0 {
            return;
        }
        match c.state {
            1 => c.frame = 0,
            4 => {
                if c.next <= now {
                    let level = self.objects[i].record.field(22).unwrap_or(0);
                    let r = self.building_draw(i);
                    c.next = self.hurried(c, now.wrapping_add(cast_wait(level, r)));
                }
                c.state = 1;
                c.frame = 0;
                self.castle_picture(i, c, data, caches);
            }
            _ => {}
        }
    }

    /// The next cast's tick halved from now while he is attacked
    /// (`+0x1f4` set).
    fn hurried(&self, c: &Castle, next: u32) -> u32 {
        let now = self.ticks;
        if c.attacker != 0 {
            (next.wrapping_sub(now) >> 1).wrapping_add(now)
        } else {
            next
        }
    }

    /// 0x410790: the attacker forgotten 3000 ticks after his blow; at its
    /// tick the ambient sound (mode 5, event 0x8d sea / 0x8e war, neither
    /// with a `SND_n` list: silent) and the next drawn; at the cast's tick
    /// standing turns to casting (state 4, its picture set); casting, at
    /// frame 7 the cast: each cell within `+0x1e4` across and `+0x1e8`
    /// down of his cell, on the map, weighs the 3 x 3 about it, row by
    /// row, by what lies on each (`cell_occupants`) of a player's
    /// (`+0x57` 1) and not nation 4: a ship (class 200-211) with `+0x5ab` not 0
    /// (the cell it is entering, -1 when none) or a fort (0x34-0x36) adds
    /// `10 - d` (`d` its cell's distance from his, 0x5576d8; it may go
    /// below 0), another building 0x32-0x3e 1; the sum so far times 4 on
    /// his attacker's cell (`+0x4c` = `+0x1f4`). The cells whose sum is not
    /// 0 are listed with it; their total not 0, a draw picks the first
    /// whose running sum (signed) is not below `r % total`, the spell's
    /// count is raised by one and it is cast there (0x42f580: free), the
    /// cast sound plays (mode 1, 0x82 / 0x83: no lists, silent) and the
    /// next cast's tick is drawn.
    fn castle_tick(&mut self, i: usize, c: &mut Castle, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        if c.attacker != 0 && c.attacked.wrapping_add(ANGER) <= now {
            c.attacker = 0;
        }
        if c.sound <= now {
            c.sound = now.wrapping_add(sound_wait(self.building_draw(i)));
        }
        if now < c.next {
            return;
        }
        if c.state == 1 {
            c.state = 4;
            self.castle_picture(i, c, data, caches);
            return;
        }
        if c.state != 4 || c.frame != CAST_FRAME {
            return;
        }
        let o = &self.objects[i];
        let ObjectClass::Castle(class) = o.class else {
            return;
        };
        let f = |k| o.record.field(k).unwrap_or(0) as i32;
        let (level, rx, ry) = (f(22) as u32, f(23), f(24));
        let (cx, cy) = o.cell.unwrap_or((f(5), f(6)));
        let (w, h) = self.map_size();
        let (w, h) = (w as i32, h as i32);
        let occupants = self.cell_occupants((cx - rx - 1, cy - ry - 1), (cx + rx + 2, cy + ry + 2));
        // What an occupant adds and its id; nothing for none that counts.
        let weigh = |cell: (i32, i32)| -> Option<(i32, u32)> {
            let &(_, k) = occupants.iter().find(|&&(c, _)| c == cell)?;
            let t = &self.objects[k];
            if Scene::side_of(t) != 1 || t.owner.is_some_and(|(n, _)| n == 4) {
                return None;
            }
            let d = super::fort::cell_distance((cx, cy), cell) as i32;
            let class = t.record.class_id;
            // `+0x5ab` is a cell's index, so 0 only entering cell (0, 0).
            let ship = (200..=211).contains(&class)
                && t.ship
                    .as_ref()
                    .is_none_or(|s| s.under_way_into() != Some((0, 0)));
            let add = if ship || (0x34..=0x36).contains(&class) {
                10 - d
            } else if (0x32..=0x3e).contains(&class) {
                1
            } else {
                0
            };
            Some((add, t.id))
        };
        let mut list: Vec<((i32, i32), i32)> = Vec::new();
        let mut total = 0u32;
        for y in cy - ry..=cy + ry {
            for x in cx - rx..=cx + rx {
                if x < 0 || x >= w || y < 0 || y >= h {
                    continue;
                }
                let mut sum = 0i32;
                for ny in y - 1..=y + 1 {
                    for nx in x - 1..=x + 1 {
                        if let Some((add, id)) = weigh((nx, ny)) {
                            sum = sum.wrapping_add(add);
                            if id == c.attacker {
                                sum = sum.wrapping_mul(4);
                            }
                        }
                    }
                }
                if sum != 0 {
                    total = total.wrapping_add(sum as u32);
                    list.push(((x, y), sum));
                }
            }
        }
        if total == 0 {
            return;
        }
        let pick = (self.building_draw(i) % total) as i32;
        let mut run = 0i32;
        let mut at = list[list.len() - 1].0;
        for &(cell, sum) in &list {
            run = run.wrapping_add(sum);
            if run >= pick {
                at = cell;
                break;
            }
        }
        let monsters = Some(crate::combat::MONSTERS);
        if class == 0x42 {
            let _ = self.spawn_hazard(Spell::Waterspout, at.0, at.1, monsters, data, caches);
        } else {
            self.spawn_fireball(at.0, at.1, monsters, data, caches);
        }
        let r = self.building_draw(i);
        c.next = self.hurried(c, now.wrapping_add(cast_wait(level, r)));
    }
}

/// `(1250 - 125 x level) x (r % 21 + 80) / 100` ticks (unsigned).
fn cast_wait(level: u32, r: u32) -> u32 {
    1250u32
        .wrapping_sub(125u32.wrapping_mul(level))
        .wrapping_mul(r % 21 + 80)
        / 100
}

/// `(r % 41 + 60) x 375 / 100` ticks.
fn sound_wait(r: u32) -> u32 {
    (r % 41 + 60) * 375 / 100
}

/// The castle's picture prefix (`+0xb8`, 0x410ca0 at message 2).
pub(super) fn prefix(class: u32) -> &'static str {
    if class == 0x42 {
        "CSTLSEA"
    } else {
        "CSTLWAR"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TICK;
    use crate::scene::gorgon::lcg;

    /// miss009's fire wizard (level 7, reach 15 either way, at (30, 1)),
    /// a game tick at a time: his first cast and sound ticks drawn at the
    /// start from his generator `[game +0x2ee5] x id`; the cast picture
    /// (32 frames, 2 ticks each) at the cast's tick, its frame kept from
    /// the standing one (here 7, stepped past at once, so no cast) and,
    /// ended with the cast still due, the next cast's tick drawn; at frame
    /// 7 of a later one a fireball on the cell the weighted draw picks
    /// about a player's ship 3 cells off (each of the 9 cells about it
    /// `10 - 4`), then the next cast's tick (needs `OPENCONQUEST_DATA`).
    #[test]
    fn the_fire_wizard_casts() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
        scene.brain = None;
        let k = scene
            .objects
            .iter()
            .position(|o| o.class == ObjectClass::Castle(0x43))
            .unwrap();
        let id = scene.objects[k].id;
        let f = |n| scene.objects[k].record.field(n).unwrap();
        assert_eq!((f(22), f(23), f(24)), (7, 15, 15));
        assert_eq!(scene.objects[k].cell, Some((30, 1)));
        let mut r = scene.game_seed().wrapping_mul(id);
        let next = 375 * (lcg(&mut r) % 21 + 80) / 100;
        let sound = (lcg(&mut r) % 41 + 60) * 375 / 100;
        let c = &scene.castles[&id];
        assert_eq!((c.state, c.next, c.sound, scene.ticks), (1, next, sound, 0));
        assert_eq!(scene.building_lcgs[&id], r);
        let t = openconquest_formats::map::TILE_SIZE as f32;
        let s = scene
            .objects
            .iter()
            .position(|o| scene.is_own(o) && o.ship.is_some())
            .unwrap();
        scene.objects[s].x = 33.0 * t + t / 2.0;
        scene.objects[s].y = 4.0 * t + t / 2.0;
        let (mut states, mut casts) = (Vec::new(), Vec::new());
        while scene.ticks < 1200 {
            let before = scene.castles[&id].clone();
            let r0 = scene.building_lcgs[&id];
            scene.update(TICK, &data, &mut caches);
            let c = &scene.castles[&id];
            if c.state != before.state {
                states.push((scene.ticks, c.state, c.frame));
            }
            if before.state == 4 && c.state == 4 && c.next != before.next {
                let cell = scene.fireballs.last().unwrap().cell;
                casts.push((scene.ticks, cell, r0, c.next, c.frame));
            }
        }

        assert_eq!(&states[..2], &[(333, 4, 7), (382, 1, 0)]);
        let (tick, cell, r0, next, frame) = casts[0];
        assert_eq!((tick, frame), (1118, 7));
        // Its draws this tick: the cell's, then the next cast's.
        let mut r = r0;
        let pick = (lcg(&mut r) % 54) as i32;
        let n = (pick.max(1) - 1) / 6;
        assert_eq!(cell, (32 + n % 3, 3 + n / 3));
        assert_eq!(next, tick + 375 * (lcg(&mut r) % 21 + 80) / 100);
    }
}
