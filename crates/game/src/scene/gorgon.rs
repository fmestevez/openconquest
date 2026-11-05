//! The Gorgon (MedusaClassTy, gorgona.cpp, class 0x1fa): she rests under
//! the water (state 0, `r % 301 + 200` ticks), hops to a cell of the 3 x 3
//! about her (0x401780), rises (1, `GORG1`), stares one to three times (2,
//! `GORG2`, at frame 13 of each loop over the 3, 5 then 7 cells square
//! about her) and sinks (3, `GORG3`); struck to nothing she dies (4,
//! `GORG4`, dropping the Gorgon's head and what she carried) and goes at
//! the end of her next rest (5). Her tick (message 0, 0x401f70) runs the
//! state's code, then steps her picture (0x4013b0); a state is entered by
//! 0x4019d0; her own generator (`+0x18`) is seeded `[game +0x2ee5] x id`.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;

use super::siren::SIREN;
use super::{Caches, GameData, Owner, Scene};
use crate::objects::{self, Treasure};

/// An area of cells, inclusive: x0, x1, y0, y1.
pub(super) type Area = (i32, i32, i32, i32);

/// Her record class.
pub const GORGON: u32 = 0x1fa;
/// Her life (`+0x78`, the constructor 0x401000).
pub const GORGON_HP: i32 = 150_000;
/// Treasures she carries at most (`[0x5c1134]`, `[0x5c1136]`).
const CARRIES: usize = 2;
/// The frame of `GORG2` whose drawing stares (0x401f70 state 2).
const STARE_FRAME: i32 = 13;
/// The Gorgon's head (treasure kind 17), dropped as she dies.
const HEAD: u8 = 17;
/// Treasure kind of the mirror shield of Athena; Perseus (hero 8).
const MIRROR_SHIELD: u8 = 14;
const PERSEUS: u8 = 8;

/// A Gorgon: the original's fields, by offset.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Gorgon {
    /// Her generator (`+0x18`).
    pub rng: u32,
    /// State `+0x110`: 0 rest, 1 rise, 2 stare, 3 sink, 4 death, 5 gone.
    pub state: u8,
    /// Life `+0x78`, the original's points.
    pub hp: i32,
    /// The stare's latch (`+0x7c`).
    pub latch: bool,
    /// Her area, cells inclusive: x0, x1, y0, y1 (`+0x80`-`+0x8c`).
    pub area: (i32, i32, i32, i32),
    /// The last frame's tick (`+0x94`), ticks a frame (`+0x98`).
    pub last: u32,
    pub delay: u32,
    /// The state's start (`+0x9c`) and the rest's length (`+0xa0`).
    pub since: u32,
    pub rest: u32,
    /// Loops of the picture done (`+0xa4`) and wanted (`+0xa8`).
    pub loops: i32,
    pub wanted: i32,
    /// Cut short by a spell (`+0xac`).
    pub cut: bool,
    /// Her killer (`+0xb4`), set by lightning only.
    pub killer: Option<Owner>,
    /// The ship or building that struck her (`+0xb8`).
    pub target: Option<u32>,
    /// Her cell (`+0x114`, `+0x118`), whose slot `+0x18` holds her.
    pub cell: (i32, i32),
    /// Her picture shown (`+0x11c`): frame about to be drawn (`+0x45`),
    /// the last (`+0x51`), and the one drawn last.
    pub picture: bool,
    pub frame: i32,
    pub end: i32,
    pub drawn: i32,
    /// The state whose picture the object holds (not saved: loaded again).
    #[serde(skip)]
    art: Option<u8>,
}

impl Gorgon {
    /// Her generator's draw (`+0x18`).
    fn draw(&mut self) -> u32 {
        lcg(&mut self.rng)
    }

    fn inside(&self, (x, y): (i32, i32)) -> bool {
        (self.area.0..=self.area.1).contains(&x) && (self.area.2..=self.area.3).contains(&y)
    }
}

/// A creature's own generator (`+0x18`): `s = s x 0x41c64e6d + 0x3039`,
/// the draw its high half.
pub(super) fn lcg(rng: &mut u32) -> u32 {
    *rng = rng.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
    *rng >> 16
}

/// 0x48f100: the heading of `a` seen from `b` (east 0, counter-clockwise
/// as 0x5c4f4c); the same cell 0.
fn heading(a: (i32, i32), b: (i32, i32)) -> u8 {
    let (dx, dy) = ((a.0 - b.0).signum(), (a.1 - b.1).signum());
    match (dx, dy) {
        (1, 0) => 0,
        (1, -1) => 1,
        (0, -1) => 2,
        (-1, -1) => 3,
        (-1, 0) => 4,
        (-1, 1) => 5,
        (0, 1) => 6,
        (1, 1) => 7,
        _ => 0,
    }
}

/// 0x557440: `(min + 2 max) / 2` of the two distances.
fn distance(a: (i32, i32), b: (i32, i32)) -> i32 {
    let (dx, dy) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    (dx.min(dy) + 2 * dx.max(dy)) / 2
}

/// 0x404730: a pick by weight; one entry is 0 with no draw, else one draw
/// `t = r % (sum + 1)` and the first entry whose running sum reaches `t`.
pub(super) fn pick(weights: &[u32], rng: &mut u32) -> usize {
    if weights.len() == 1 {
        return 0;
    }
    let sum: u32 = weights.iter().sum();
    let t = lcg(rng) % (sum + 1);
    let mut acc = 0;
    for (k, &w) in weights.iter().enumerate() {
        acc += w;
        if t <= acc {
            return k;
        }
    }
    weights.len() - 1
}

impl Scene {
    /// Message 2 for each Gorgon (0x402360) and siren (0x403f60) of the
    /// map, in the objects' order: her generator seeded `[game +0x2ee5] x
    /// id`, placed (`creature_start`), resting (state 0).
    pub(super) fn start_gorgons(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Monster(GORGON | SIREN)))
            .map(|o| o.id)
            .collect();
        for id in ids {
            if self.objects[self.object_index(id).unwrap()].class == ObjectClass::Monster(SIREN) {
                self.start_siren(id, data, caches);
                continue;
            }
            // Her area: either field below 1, the whole map.
            let Some((cell, area)) = self.creature_start(id, CARRIES, 1) else {
                continue;
            };
            let mut g = Gorgon {
                rng: self.game_seed().wrapping_mul(id),
                hp: GORGON_HP,
                cell,
                area,
                ..Default::default()
            };
            let i = self.object_index(id).unwrap();
            self.sync_gorgon_hull(i, g.hp);
            self.set_gorgon_state(i, &mut g, 0, data, caches);
            self.gorgons.insert(id, g);
        }
    }

    /// Message 2's part shared by the Gorgon (0x402360) and the siren
    /// (0x403f60): her cell fields 5, 6, her treasures (field 10 the
    /// count, 11-20 two entries of the treasure record's fields 4-8), her
    /// area her cell -/+ fields 7, 8 clipped to the map (either below
    /// `least`: the whole map). A cell 0x402ee0 refuses (not strict):
    /// removed (message 10, 0x542670), `None`.
    pub(super) fn creature_start(
        &mut self,
        id: u32,
        cap: usize,
        least: i32,
    ) -> Option<((i32, i32), Area)> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let i = self.object_index(id)?;
        let fields = self.objects[i].record.fields.clone();
        let f = |k: usize| fields.get(k).copied().unwrap_or(0) as i32;
        let cell = (f(5), f(6));
        let carried = (f(10).max(0) as usize).min(cap);
        for k in 0..carried {
            let e = |j: usize| f(11 + 5 * k + j) as u32;
            let tr = Treasure {
                kind: e(0) as u8,
                param: e(3),
                amount: e(4),
            };
            self.held.push((id, tr));
        }
        if !self.may_hold(id, cell, false, false) {
            // Removal (0x542670, code 10).
            self.held.retain(|h| h.0 != id);
            self.objects.remove(i);
            return None;
        }
        let area = if f(7) < least || f(8) < least {
            (0, w - 1, 0, h - 1)
        } else {
            (
                (cell.0 - f(7)).max(0),
                (cell.0 + f(7)).min(w - 1),
                (cell.1 - f(8)).max(0),
                (cell.1 + f(8)).min(h - 1),
            )
        };
        let t = TILE_SIZE as f32;
        self.objects[i].x = cell.0 as f32 * t + t / 2.0;
        self.objects[i].y = cell.1 as f32 * t + t / 2.0;
        Some((cell, area))
    }

    /// The port keeps her life as her object's hull, a fifteenth of it.
    fn sync_gorgon_hull(&mut self, i: usize, hp: i32) {
        if let Some(c) = self.objects[i].combat.as_mut() {
            c.hull = hp as f32 / 15.0;
        }
    }

    /// 0x402ee0(self, x, y, strict, treasure_ok): the landscape's low
    /// nibble up to 7 and second up to 3; strict, no ship in the cell's
    /// occupant slot (0x499900), nothing on its third place (`+0x1c`) but,
    /// with `room`, an object exactly of its base class (a treasure,
    /// 0x50e600), and no leviathan showing a picture in `+0x10` (vt+0x30);
    /// then her slot `+0x18` empty or hers (another Gorgon, a siren, a sea
    /// monster). The siren's the same (her `+0x18` too).
    pub(super) fn may_hold(&self, id: u32, c: (i32, i32), strict: bool, room: bool) -> bool {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        if c.0 < 0 || c.1 < 0 || c.0 >= w || c.1 >= h {
            return false;
        }
        let types = self.cell_types[(c.1 * w + c.0) as usize];
        if types & 0xf > 7 || types & 0xf0 > 0x30 {
            return false;
        }
        if strict {
            if self.hazard_occupant(c).is_some() {
                return false;
            }
            if let Some(k) = self.third_place_object(c) {
                if !room || self.objects[k].class != ObjectClass::Treasure {
                    return false;
                }
            }
            if self
                .leviathan_in(c)
                .is_some_and(|l| !self.leviathan_hidden(l))
            {
                return false;
            }
        }
        let other = self
            .gorgons
            .iter()
            .any(|(&o, g)| o != id && g.state < 5 && g.cell == c && self.object_index(o).is_some());
        let siren = self
            .sirens
            .iter()
            .any(|(&o, s)| o != id && s.state < 5 && s.cell == c && self.object_index(o).is_some());
        !other && !siren && self.sea_monster_at(c, 0).is_none()
    }

    /// Each Gorgon's tick (message 0, 0x401f70) and each siren's
    /// (0x403ba0), in the objects' order.
    pub(super) fn tick_gorgons(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| self.gorgons.contains_key(&o.id) || self.sirens.contains_key(&o.id))
            .map(|o| o.id)
            .collect();
        self.gorgons.retain(|id, _| ids.contains(id));
        self.sirens.retain(|id, _| ids.contains(id));
        for id in ids {
            if self.sirens.contains_key(&id) {
                self.tick_siren(id, data, caches);
                continue;
            }
            let Some(mut g) = self.gorgons.remove(&id) else {
                continue;
            };
            let Some(i) = self.object_index(id) else {
                continue;
            };
            self.gorgon_art(i, &mut g, data, caches);
            self.gorgon_tick(i, &mut g, data, caches);
            if g.state == 5 {
                continue;
            }
            self.gorgons.insert(id, g);
        }
    }

    fn gorgon_tick(&mut self, i: usize, g: &mut Gorgon, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let next = match g.state {
            0 => {
                if now < g.since + g.rest {
                    return;
                }
                if g.hp < 1 {
                    // Her kill reported to her killer, if any (0x50e750),
                    // her slot cleared and she goes (0x542670, code 10).
                    if let Some(k) = g.killer {
                        self.note_destroyed(i, Some(k));
                    }
                    let id = self.objects[i].id;
                    self.held.retain(|h| h.0 != id);
                    self.objects.remove(i);
                    g.state = 5;
                    g.cut = false;
                    return;
                }
                if self.gorgon_move(i, g) {
                    1
                } else {
                    0
                }
            }
            1 => {
                if g.loops < g.wanted {
                    self.gorgon_step(g);
                    return;
                }
                2
            }
            2 => {
                if g.loops < g.wanted {
                    if g.frame == STARE_FRAME {
                        if !g.latch {
                            g.latch = true;
                            self.gorgon_stare(g, data, caches);
                        }
                    } else {
                        g.latch = false;
                    }
                    if g.hp < 1 || g.cut {
                        g.loops = g.wanted - 1;
                    }
                    self.gorgon_step(g);
                    return;
                }
                3
            }
            3 => {
                if g.loops < g.wanted {
                    self.gorgon_step(g);
                    return;
                }
                if g.hp > 0 {
                    0
                } else {
                    4
                }
            }
            4 => {
                if g.loops < g.wanted {
                    self.gorgon_step(g);
                    return;
                }
                0
            }
            _ => return,
        };
        self.set_gorgon_state(i, g, next, data, caches);
    }

    /// 0x4019d0: to state `s` (its loops none, its start now).
    fn set_gorgon_state(
        &mut self,
        i: usize,
        g: &mut Gorgon,
        s: u8,
        data: &GameData,
        caches: &mut Caches,
    ) {
        g.loops = 0;
        g.since = self.ticks;
        let id = self.objects[i].id;
        let (px, py) = (self.objects[i].x, self.objects[i].y);
        match s {
            0 => {
                g.state = 0;
                g.picture = false;
                g.rest = g.draw() % 301 + 200;
                g.cut = false;
            }
            1 => {
                g.state = 1;
                g.picture = true;
                self.gorgon_picture(i, g, data, caches);
                // Sound event 94 (`apmed3`), mode 5 at her cell.
                self.sounds.push(("apmed3", px, py));
                g.wanted = 1;
                self.gorgon_step(g);
            }
            2 => {
                g.state = 2;
                if g.hp >= 1 && !g.cut {
                    g.wanted = (g.draw() % 3 + 1) as i32;
                } else {
                    g.wanted = 0;
                    self.set_gorgon_state(i, g, 3, data, caches);
                    return;
                }
                self.gorgon_picture(i, g, data, caches);
                self.gorgon_step(g);
                self.pick_up_treasure(id, g.cell, CARRIES);
            }
            3 => {
                g.state = 3;
                self.gorgon_picture(i, g, data, caches);
                g.wanted = 1;
                self.gorgon_step(g);
            }
            4 => {
                g.state = 4;
                self.gorgon_picture(i, g, data, caches);
                g.wanted = 1;
                g.cut = false;
                self.gorgon_step(g);
                // Sound event 105 (`deamons`), mode 5 at her cell.
                self.sounds.push(("deamons", px, py));
                // Her cell gets the Gorgon's head, then what she carried
                // (0x520c70 each); none carried after.
                let head = Treasure {
                    kind: HEAD,
                    param: 0,
                    amount: 0,
                };
                self.place_treasure(head, px, py, data, caches);
                self.drop_carried(id, px, py, data, caches);
            }
            _ => {
                g.state = s;
                g.cut = false;
            }
        }
    }

    /// Entering the state that takes treasures (the Gorgon's 2, the
    /// siren's 2): with fewer than `cap` carried she takes the treasure on
    /// her cell's third place (0x158c1 to it, its five dwords kept). (Not
    /// ported, as for the leviathan: the treasure's own test 0x407950, its
    /// `+0xa4` 1 or 2.)
    pub(super) fn pick_up_treasure(&mut self, id: u32, cell: (i32, i32), cap: usize) {
        if self.held.iter().filter(|h| h.0 == id).count() >= cap {
            return;
        }
        if let Some(k) = self
            .third_place_object(cell)
            .filter(|&k| self.objects[k].class == ObjectClass::Treasure)
        {
            if let Some(tr) = self.objects[k].treasure {
                self.objects.remove(k);
                self.held.push((id, tr));
            }
        }
    }

    /// Dying: each treasure she carried laid on her cell (0x520c70), none
    /// carried after.
    pub(super) fn drop_carried(
        &mut self,
        id: u32,
        px: f32,
        py: f32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let (kept, dropped): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
            .into_iter()
            .partition(|h| h.0 != id);
        self.held = kept;
        for (_, tr) in dropped {
            self.place_treasure(tr, px, py, data, caches);
        }
    }

    /// 0x401110: the state's picture `GORG<state>` from its first frame,
    /// a frame a tick (every 3 dying), drawn at the next step.
    fn gorgon_picture(&mut self, i: usize, g: &mut Gorgon, data: &GameData, caches: &mut Caches) {
        g.delay = if g.state == 4 { 3 } else { 1 };
        g.last = 0;
        g.frame = 0;
        g.drawn = 0;
        self.gorgon_art(i, g, data, caches);
        g.end = self.objects[i].frames.len() as i32 - 1;
    }

    /// Her picture loaded into object `i` when it is not the state's (a
    /// new state, or after a restore).
    fn gorgon_art(&mut self, i: usize, g: &mut Gorgon, data: &GameData, caches: &mut Caches) {
        if g.picture && g.art != Some(g.state) {
            let name = format!("GORG{}", g.state);
            if let Ok(frames) = objects::sequence_frames(data, caches, &name) {
                self.objects[i].frames = frames;
            }
            g.art = Some(g.state);
        }
    }

    /// 0x4013b0: when its time has come (at once when cut short), the
    /// frame is drawn and the next made ready; past the last, the first
    /// again and a loop more.
    fn gorgon_step(&mut self, g: &mut Gorgon) {
        let d = if g.cut { 0 } else { g.delay };
        if g.last + d > self.ticks {
            return;
        }
        g.last = self.ticks;
        g.drawn = g.frame;
        g.frame += 1;
        if g.frame > g.end {
            g.frame = 0;
            g.loops += 1;
        }
    }

    /// The stare (0x401f70 state 2, at frame 13 of each loop): the square
    /// of side 3, 5 then 7 by the loop about her cell, columns then rows,
    /// each map cell's occupant (`+0xc`) of side 1 (a ship but on the cell
    /// it steps into): each gets message 0x158bf with a blow of 88000 /
    /// their number (22000 from five), 150 and her id. A ship (0x4dc630)
    /// with Perseus or a magic shield is spared; else it is stone for 150
    /// ticks more and takes the blow (cut by a mirror shield's percent).
    /// (Not decoded: the building's handlers of 0x158bf, the fort's
    /// 0x41a041 among them: a building counts but takes nothing here.)
    fn gorgon_stare(&mut self, g: &Gorgon, data: &GameData, caches: &mut Caches) {
        // Sound event 127 (`atmed1c`), mode 5 at her cell (0x402111).
        let t = TILE_SIZE as f32;
        let (cx, cy) = g.cell;
        self.sounds
            .push(("atmed1c", cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0));
        let half = g.loops + 1;
        let (x, y) = g.cell;
        let mut list: Vec<((i32, i32), usize)> = self
            .cell_occupants((x - half, y - half), (x + half + 1, y + half + 1))
            .into_iter()
            .filter(|&(_, k)| super::Scene::side_of(&self.objects[k]) == 1)
            .collect();
        list.sort_by_key(|&(c, _)| c);
        list.truncate(50);
        let n = list.len() as i32;
        if n == 0 {
            return;
        }
        let blow = if n < 5 { 88_000 / n } else { 22_000 };
        let ids: Vec<u32> = list.iter().map(|&(_, k)| self.objects[k].id).collect();
        for id in ids {
            let Some(k) = self.object_index(id) else {
                continue;
            };
            let o = &self.objects[k];
            let Some(s) = o.ship.as_ref() else {
                continue;
            };
            let warded = o.combat.as_ref().is_some_and(|c| c.protect_magic > 0.0);
            if s.has_hero(PERSEUS) || warded {
                continue;
            }
            let mirror = s
                .treasures
                .iter()
                .find(|t| t.kind == MIRROR_SHIELD)
                .map_or(0, |t| if t.amount > 0 { t.amount.min(100) } else { 100 });
            if let Some(c) = self.objects[k].combat.as_mut() {
                c.stone += crate::clock::ticks(150);
            }
            let hurt = blow as f32 / 15.0 * (100 - mirror) as f32 / 100.0;
            self.share_blow(id, hurt, None, data, caches);
        }
    }

    /// 0x401780, at the end of a rest: the cells of the 3 x 3 about her
    /// (her own too; columns then rows) within her area that 0x402ee0
    /// lets her into (strict, with room while she carries fewer than two),
    /// weighted: with no target 100 on a treasure, else 10; with one
    /// `100 / distance` (1 at none) and 1000 more on the heading towards
    /// it. One picked (0x404730), she moves there. False with none.
    fn gorgon_move(&mut self, i: usize, g: &mut Gorgon) -> bool {
        let id = self.objects[i].id;
        let target = self.gorgon_target(g);
        let want = target.map(|t| heading(t, g.cell));
        let room = self.held.iter().filter(|h| h.0 == id).count() < CARRIES;
        let mut cells = Vec::new();
        let mut weights = Vec::new();
        for di in 0..3 {
            for dj in 0..3 {
                let c = (g.cell.0 - 1 + di, g.cell.1 - 1 + dj);
                if !g.inside(c) || !self.may_hold(id, c, true, room) {
                    continue;
                }
                let w = match target {
                    None => {
                        let treasure = self
                            .third_place_object(c)
                            .is_some_and(|k| self.objects[k].class == ObjectClass::Treasure);
                        if treasure {
                            100
                        } else {
                            10
                        }
                    }
                    Some(t) => {
                        let d = distance(c, t);
                        let w = if d != 0 { 100 / d as u32 } else { 1 };
                        if Some(heading(c, g.cell)) == want {
                            w + 1000
                        } else {
                            w
                        }
                    }
                };
                cells.push(c);
                weights.push(w);
            }
        }
        if cells.is_empty() {
            return false;
        }
        g.cell = cells[pick(&weights, &mut g.rng)];
        let t = TILE_SIZE as f32;
        self.objects[i].x = g.cell.0 as f32 * t + t / 2.0;
        self.objects[i].y = g.cell.1 as f32 * t + t / 2.0;
        true
    }

    /// 0x401600: her target's cell. One gone, not a ship or building (base
    /// classes 200, 50) or outside her area is let go; but the cell of one
    /// still there pulls this pick.
    fn gorgon_target(&self, g: &mut Gorgon) -> Option<(i32, i32)> {
        let id = g.target?;
        let k = self.object_index(id).filter(|&k| self.ship_or_building(k));
        let Some(k) = k else {
            g.target = None;
            return None;
        };
        let c = self.object_cell(k);
        if !g.inside(c) {
            g.target = None;
        }
        Some(c)
    }

    /// Base class 200 (ships) or 50 (buildings), 0x50e600.
    fn ship_or_building(&self, k: usize) -> bool {
        let c = self.objects[k].record.class_id;
        (200..=211).contains(&c) || (50..=74).contains(&c)
    }

    /// Its cell as vt+0x28 gives it: a building's own, a ship's where it is.
    fn object_cell(&self, k: usize) -> (i32, i32) {
        match self.objects[k].cell {
            Some(c) if self.objects[k].ship.is_none() => c,
            _ => self.cell_of_object(k),
        }
    }

    /// vt+4 (0x4014d0): a blow of `damage` (ours, a fifteenth of hers) from
    /// player `by`, fired by `attacker`. Resting or gone she takes none;
    /// else it comes off her life (down to 0), and a blow leaving none
    /// reports her kill at once (0x50e750, every such blow). With no
    /// target and life left, a ship or building that struck her becomes
    /// it; one outside her area widens it to the square about her of
    /// half-side one more than its distance either way (0x4016b0). Sound
    /// event 116 (`hitmed`).
    pub(super) fn gorgon_struck(
        &mut self,
        id: u32,
        damage: f32,
        by: Option<Owner>,
        attacker: Option<u32>,
    ) {
        let (Some(mut g), Some(i)) = (self.gorgons.remove(&id), self.object_index(id)) else {
            return;
        };
        if !matches!(g.state, 0 | 5) {
            let blow = (damage.max(0.0) * 15.0).round() as i32;
            g.hp = (g.hp - blow).max(0);
            self.sync_gorgon_hull(i, g.hp);
            if g.hp == 0 {
                self.note_destroyed(i, by);
            }
            if g.target.is_none() && g.hp > 0 {
                if let Some(k) = attacker
                    .and_then(|a| self.object_index(a))
                    .filter(|&k| self.ship_or_building(k))
                {
                    let c = self.object_cell(k);
                    g.target = Some(self.objects[k].id);
                    if !g.inside(c) {
                        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
                        let r = (c.0 - g.cell.0).abs().max((c.1 - g.cell.1).abs()) + 1;
                        g.area = (
                            (g.cell.0 - r).max(0),
                            (g.cell.0 + r).min(w - 1),
                            (g.cell.1 - r).max(0),
                            (g.cell.1 + r).min(h - 1),
                        );
                    }
                }
            }
            let (x, y) = (self.objects[i].x, self.objects[i].y);
            self.sounds.push(("hitmed", x, y));
        }
        self.gorgons.insert(id, g);
    }

    /// Lightning on her (0xda93, her picture shown): her life to 0, cut
    /// short, the caster her killer (reported when she goes).
    pub(super) fn gorgon_lightning(&mut self, id: u32, caster: Option<Owner>) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        if let Some(g) = self.gorgons.get_mut(&id).filter(|g| g.picture) {
            g.hp = 0;
            g.cut = true;
            g.killer = caster;
            self.sync_gorgon_hull(i, 0);
        }
    }

    /// Whether she can be shot at (vt+0x2c, 0x404690): rising, staring or
    /// sinking.
    pub(super) fn gorgon_up(&self, id: u32) -> bool {
        self.gorgons
            .get(&id)
            .is_some_and(|g| matches!(g.state, 1..=3))
    }

    /// The frame she last drew.
    pub(super) fn gorgon_frame(&self, id: u32) -> Option<u32> {
        self.gorgons
            .get(&id)
            .filter(|g| g.picture)
            .map(|g| g.drawn.max(0) as u32)
    }

    /// The point a shot at her flies to (vt+0xc, 0x402da0, by the hit
    /// byte): with no picture her cell's centre; else from her picture's
    /// corner (centred on her cell), a hit `20 + r % 21`, `20 + (r & 15)`,
    /// a miss `10 + r % 51`, `60 + r % 11` (x drawn first).
    pub(super) fn gorgon_aim(&mut self, id: u32, hit: bool) -> Option<(i32, i32)> {
        let i = self.object_index(id)?;
        let t = TILE_SIZE as i32;
        let cell = self.gorgons.get(&id)?.cell;
        let (px, py) = self.picture_corner(i, cell);
        let g = self.gorgons.get_mut(&id)?;
        let (cx, cy) = g.cell;
        if !g.picture {
            return Some((cx * t + t / 2, cy * t + t / 2));
        }
        Some(if hit {
            let x = g.draw() % 21 + 20;
            (px + x as i32, py + (g.draw() & 15) as i32 + 20)
        } else {
            let x = g.draw() % 51 + 10;
            (px + x as i32, py + (g.draw() % 11) as i32 + 60)
        })
    }

    /// Her picture's top left, centred on her cell (as 0x401110 and
    /// 0x403110 place it).
    pub(super) fn picture_corner(&self, i: usize, (cx, cy): (i32, i32)) -> (i32, i32) {
        let t = TILE_SIZE as i32;
        let (w, h) = self.objects[i]
            .frames
            .first()
            .and_then(|f| f.first())
            .map_or((t, t), |l| (l.image.width as i32, l.image.height as i32));
        (t * cx - (w - t) / 2, t * cy - (h - t) / 2)
    }
}
