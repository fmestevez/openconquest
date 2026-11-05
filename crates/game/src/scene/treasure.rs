//! Treasures and heroes: taking treasures from the map, what they do, and
//! heroes joining ships that bring their magic items (the manual's
//! "Treasures" and "Heroes"; kinds as in `objects::treasure_name`).

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;

use super::Scene;
use crate::assets::GameData;
use crate::magic::Spell;
use crate::objects::{self, Caches, Task, Treasure};

/// A hero's house's picture cycles by state (`+0xc0`, 0x595e80): the
/// ticks a picture's frames take in states 0-5.
const HOUSE_CYCLE: [u32; 6] = [50, 75, 75, 57, 100, 500];

/// A taken hero's house playing out its pictures (0x4230f0 on states 4,
/// 5 and 2): the state `+0xa4`, the frame `+0x7c->+0x45` and count
/// `+0x49`, the frame last drawn, the first step made (`+0xbc`) and the
/// ticks since the last.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HouseFade {
    pub state: u8,
    pub frame: u32,
    pub frames: u32,
    pub shown: u32,
    pub started: bool,
    pub wait: u32,
}

/// The heroes' voices at their house (0x4ba2a0, mode 2), by hero: yes
/// (0xfc, 0x102, 0x100, 0xfe, 0x104, 0x106, 0x108, 0x10a, 0x10c) and no
/// (each one more), their `SND_<n>` samples.
const HERO_VOICES: [(&str, &str); 9] = [
    ("ja1", "ja4"),
    ("th4", "th1"),
    ("or3", "or1"),
    ("ze3", "ze1"),
    ("he2", "he4"),
    ("ar2", "ar4"),
    ("at1", "at4"),
    ("od5", "od1"),
    ("pe6", "pe1"),
];
/// What heals aboard (0x4b98b0, a tick at a time): each Cup of Asclepius
/// and Golden Fleece adds its treasure amount (field 8: 40 and 10 in the
/// campaign) and Argus 20 (0x4bd36a) to a count; every 1000 heals each
/// worn unit a point of its 100 (the Cup the marines, the Fleece every
/// unit) and mends 1% of the hull (the Fleece and Argus).
const HEAL_STEP: f32 = 1000.0;
pub const ARGUS_RATE: u32 = 20;

impl Scene {
    /// Sends own ship `ship` to take the treasure or the hero `target`.
    pub fn order_take(&mut self, ship: u32, target: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let t = self
            .object_index(target)
            .ok_or("Choose a treasure or a hero's house")?;
        let kind = self.objects[i].ship.as_ref().ok_or("Not a ship")?.kind;
        match self.objects[t].class {
            ObjectClass::Treasure => {}
            // The hero's item is asked for at the house (0x4ba3f0 at the
            // walkway's last frame, the hero's "no" voice without it).
            ObjectClass::HeroHouse => {
                let ship = self.objects[i].ship.as_ref().unwrap();
                if objects::hero_room(kind) <= ship.heroes.len() {
                    return Err("Only warships with room for heroes can take them".into());
                }
            }
            _ => return Err("Choose a treasure or a hero's house".into()),
        }
        // 0x4aab00 / 0x4af940: a treasure on a cell 0x499990(cell, 0)
        // takes is sailed to (`+0x792` = `+0x797` its cell, `+0x7c2` the
        // box); one on land to the sea cell 0x55a890 picks (`land_take`),
        // the wait `+0x591` set 0 first; the hero's house to its berth
        // (vt+0xb8, 0x40e050), its own cell the land cell.
        let w = TILE_SIZE as f32;
        let mid = |(cx, cy): (i32, i32)| ((cx as f32 + 0.5) * w, (cy as f32 + 0.5) * w);
        let treasure = self.objects[t].class == ObjectClass::Treasure;
        let at = treasure
            .then(|| self.cell_of(t))
            .filter(|&c| self.sea_cell(c, false));
        let mut land = None;
        let site = self.cell_of(t);
        let path = match at {
            Some(c) => self.plan_route(i, mid(c).0, mid(c).1),
            None if treasure => {
                self.objects[i].ship.as_mut().unwrap().waited = 0.0;
                let cell = self.cell_of(t);
                let (e, l) = self
                    .land_take(i, cell)
                    .ok_or("The ship cannot sail there")?;
                land = Some((e, l));
                if e == self.cell_of(i) {
                    Some(Default::default())
                } else {
                    self.plan_route(i, mid(e).0, mid(e).1)
                }
            }
            None => {
                let e = self.vt_berth(t);
                land = Some((e, site));
                if e == self.cell_of(i) {
                    Some(Default::default())
                } else {
                    self.plan_route(i, mid(e).0, mid(e).1)
                }
            }
        }
        .ok_or("The ship cannot sail there")?;
        let s = self.objects[i].ship.as_mut().unwrap();
        s.path = path;
        s.task = Some(Task::Take {
            target,
            at,
            land,
            site,
            step: 0,
            // `+0x78a` the house itself till 0x4ba6b0 answers: no voice,
            // as 0xff.
            hero: (!treasure).then_some(0xff),
        });
        Ok(())
    }

    /// 0x4af940 for a treasure on land at `g`: the ship's grid (0x5c93bc,
    /// 0x5c9398 by draught), every cell 0x499990(cell, 0) refuses that
    /// holds an object (`+0xc`) less one, the ships about it laid on by
    /// 0x4a4e20 (-2, no list), the treasure's cell -1, and 0x55a890 from
    /// the ship's cell with 0x422a60 (`party_can_walk`): the sea cell
    /// (`+0x792`) and the land cell (`+0x797`).
    pub(super) fn land_take(
        &mut self,
        i: usize,
        g: (i32, i32),
    ) -> Option<((i32, i32), (i32, i32))> {
        use std::collections::HashSet;
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let here = self.cell_of(i);
        let s = self.objects[i].ship.as_ref()?;
        let deep = objects::deep_draught(s.kind);
        let traps: HashSet<(i32, i32)> = self.hazard_cells().into_keys().collect();
        let mut m: Vec<i16> = (0..w * h)
            .map(|n| (n % w, n / w))
            .map(|c| {
                if self.nav.passable(c.0, c.1, deep) && !traps.contains(&c) {
                    0
                } else {
                    -1
                }
            })
            .collect();
        // The cells' standing objects (`volcano.rs::standing_at`'s).
        let mut standing: HashSet<(i32, i32)> = HashSet::new();
        for o in &self.objects {
            if let Some((x, y)) = o.cell {
                if o.combat.as_ref().is_some_and(|c| c.hull <= 0.0) {
                    continue;
                }
                let n = super::trade::building_cells(o);
                for dy in 0..n {
                    for dx in 0..n {
                        standing.insert((x + dx, y + dy));
                    }
                }
            }
        }
        for &(x, y) in &standing {
            if (0..w).contains(&x) && (0..h).contains(&y) && !self.nav.is_sea(x, y) {
                m[(y * w + x) as usize] -= 1;
            }
        }
        // 0x4a4e20(ship, m, `+0x591`, -2, no list): the draw only when the
        // wait is over 50 (on the scene's generator, as `replan`); hero 0
        // aboard opens the whirlpools' and medium reefs' cells; within 3
        // cells other ships closed by the plain rule (the group `+0x5e`
        // not kept by the port, as in `replan`) and remembered, any other
        // object closed; then the remembered cells whose ship still lies
        // there.
        let id = self.objects[i].id;
        let wait = (s.waited / crate::clock::TICK) as u32;
        let hero0 = s.heroes.contains(&0);
        let impatient = wait > 50 && self.rng.next() % 30 + 50 < wait;
        let under_way = |s: &objects::Ship| !s.path.is_empty() && s.stalled <= 0.0;
        let mut remember = Vec::new();
        for y in (here.1 - 3).max(0)..(here.1 + 4).min(h) {
            for x in (here.0 - 3).max(0)..(here.0 + 4).min(w) {
                let c = (x, y);
                let n = (y * w + x) as usize;
                if hero0
                    && self.objects.iter().enumerate().any(|(k, o)| {
                        matches!(o.class, ObjectClass::Whirlpool | ObjectClass::Reef(0x25a))
                            && self.cell_of(k) == c
                    })
                {
                    m[n] = 0;
                    continue;
                }
                if let Some(k) = self.slot_ship(c).filter(|&k| self.objects[k].id != id) {
                    let o = self.objects[k].ship.as_ref().unwrap();
                    let still = o.path.is_empty()
                        || o.stalled >= crate::clock::ticks(50)
                        || o.grapple.is_some();
                    if (impatient && !under_way(o)) || (!impatient && still) {
                        m[n] = -2;
                        remember.push(c);
                    }
                } else if c != here && standing.contains(&c) {
                    m[n] = -2;
                }
            }
        }
        let lying: HashSet<(i32, i32)> = (0..self.objects.len())
            .filter(|&k| self.objects[k].id != id)
            .filter(|&k| self.objects[k].ship.as_ref().is_some_and(|s| !under_way(s)))
            .map(|k| self.cell_of(k))
            .collect();
        let s = self.objects[i].ship.as_mut()?;
        s.remembered.retain(|c| lying.contains(c));
        for c in remember {
            if !s.remembered.contains(&c) {
                s.remembered.push(c);
            }
        }
        for &(x, y) in &s.remembered {
            if (0..w).contains(&x) && (0..h).contains(&y) && (x, y) != here {
                m[(y * w + x) as usize] = -2;
            }
        }
        if (0..w).contains(&g.0) && (0..h).contains(&g.1) {
            m[(g.1 * w + g.0) as usize] = -1;
        }
        super::walk_path::sea_land_pick(&mut m, w, h, here, g, |l, g| self.party_can_walk(l, g))
    }

    /// The take party on the site `site` (0x421f00 kind 3): a treasure
    /// there (class 700, 0x407950) asks ship `ship` (message 0xda9d,
    /// 0x4de5ea): one used at once (0x4b9740 class 2) is taken through
    /// 0x4b97d0, any other through 0x4bd0c0 while there is room aboard
    /// (0x4be800 mode 6: 0x597afc[type] less the treasures aboard);
    /// answered yes, 0x158c1 to the treasure. Else it stays.
    pub(super) fn party_takes(
        &mut self,
        ship: u32,
        site: (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(i) = self.object_index(ship) else {
            return;
        };
        let Some(g) = (0..self.objects.len()).find(|&g| {
            self.objects[g].class == ObjectClass::Treasure
                && self.objects[g].treasure.is_some()
                && self.cell_of(g) == site
        }) else {
            return;
        };
        let kind = self.objects[g].treasure.unwrap().kind;
        let Some(s) = self.objects[i].ship.as_ref() else {
            return;
        };
        let room = super::chest::SINK_TREASURES
            .get(s.kind as usize)
            .map_or(0, |&n| n as i32 - s.treasures.len() as i32);
        if Self::treasure_use(kind) & 2 != 0 || room >= 1 {
            self.take_treasure(i, g, data, caches);
        }
    }

    /// What kind of treasure `kind` is to a ship (0x4b9740): 2 used at
    /// once (amber, fish, the bursting spells: kinds 0-5, 8), 4 goods for
    /// the settlement (6, 7, 27), 8 spells and heroes' items (9, 17-26),
    /// 0x10 the barbarians' (10-14), 1 the healing ones (15, 16); 0 any
    /// other.
    pub(super) fn treasure_use(kind: u8) -> u8 {
        match kind {
            0..=5 | 8 => 2,
            6 | 7 | 27 => 4,
            9 | 17..=26 => 8,
            10..=14 => 0x10,
            15 | 16 => 1,
            _ => 0,
        }
    }

    /// Whether ship `i` takes a treasure of kind `kind` (0x4a5bc0): one
    /// used at once always; any other with room aboard (0x4be800 mode 6:
    /// 0x597afc[type] less the treasures aboard `+0x482`) and of a known
    /// use.
    pub(super) fn takes_treasure(&self, i: usize, kind: u8) -> bool {
        let class = Self::treasure_use(kind);
        if class & 2 != 0 {
            return true;
        }
        let Some(s) = self.objects[i].ship.as_ref() else {
            return false;
        };
        let room = super::chest::SINK_TREASURES
            .get(s.kind as usize)
            .map_or(0, |&n| n as i32 - s.treasures.len() as i32);
        room >= 1 && class != 0
    }

    /// A ship has stepped into cell `c` (0x4a3f40 at 0x4a48d2, after the
    /// chest, on every cell it comes into, whatever its order): a
    /// treasure on the cell's slot (class 700, `+0xa4` not 5) is taken
    /// when 0x4a5bc0 allows (`takes_treasure`): message 0x158c1 to it and
    /// the ship's notice (0x4a1af0 used at once, through 0x4b97d0; else
    /// 0x4a1a60, put aboard through 0x4bd0c0). (The treasure's state 5
    /// not decoded: never set by the port.)
    pub(super) fn ship_steps_on_treasure(
        &mut self,
        ship: u32,
        c: (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(i) = self.object_index(ship) else {
            return;
        };
        if self.objects[i].ship.is_none() {
            return;
        }
        let Some(g) = (0..self.objects.len()).find(|&g| {
            self.objects[g].class == ObjectClass::Treasure
                && self.objects[g].treasure.is_some()
                && self.cell_of(g) == c
        }) else {
            return;
        };
        let kind = self.objects[g].treasure.unwrap().kind;
        if self.takes_treasure(i, kind) {
            self.take_treasure(i, g, data, caches);
        }
    }

    /// The take orders (0x4ac2d0, class 700 / 0x49, sub-code 9: 0x4ad680
    /// at step 1). A treasure on the sea is taken as the ship comes into
    /// its cell (`ship_steps_on_treasure`); the order ends with the ship
    /// in that cell (0x4ac490: 0x4b8d40 clears the task, taken or not).
    /// The treasure gone first (0x51f1f0 failing), the ship posts notice
    /// 0x138 to its group every tick (0x4ad9b4, 0x4a1430) and the order
    /// stays. A treasure on land or a hero's house: the landing of
    /// sub-code 10 (`land_take_tick`). The Golden Fleece and Argus mend
    /// their ships.
    pub(super) fn run_treasures(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let takers: Vec<_> = self
            .objects
            .iter()
            .filter_map(|o| match o.ship.as_ref()?.task? {
                Task::Take {
                    target,
                    at,
                    land,
                    site,
                    step,
                    ..
                } => Some((o.id, target, at, land, site, step)),
                _ => None,
            })
            .collect();
        for (ship, target, at, land, site, step) in takers {
            let Some(i) = self.object_index(ship) else {
                continue;
            };
            if let Some((e, l)) = land {
                self.land_take_tick(i, target, (e, l), site, step, data, caches);
                continue;
            }
            if let Some(c) = at {
                if self.cell_of(i) == c {
                    self.objects[i].ship.as_mut().unwrap().task = None;
                } else if self.object_index(target).is_none() {
                    self.post_notice(ship, 0x138, 0);
                }
            }
        }
        self.heal_forts(dt);
    }

    /// A take of a treasure on land or a hero's house, a tick (0x4ac2d0,
    /// class 700 / 0x49). Sub-code 9 (`step` 0): sailing to the sea cell
    /// `e`; there, lying still (0x4ad680 giving 2, `+0x58c` 0), with
    /// nothing standing on the land cell `l` (`+0xc`; the house: the house
    /// itself there) sub-code 10 step 1, else 0x4af940 picks the cells
    /// again. Step 1 (0x4b0e40): the treasure gone, the order ends
    /// (0x4b8d40); else the close-in (`take_close_in`) asked each tick
    /// till it holds, the commands held (0x4b37d0) and the landing's
    /// steps (`take_landing_tick`). Backed out, it lies in step 5 with
    /// nothing more to do; called off on its slide, the order ends. (The
    /// port's route to `e` its plain plan, once more when it lies short of
    /// it; 0x4ad680's re-plans and make-way not ported.)
    #[allow(clippy::too_many_arguments)]
    fn land_take_tick(
        &mut self,
        i: usize,
        target: u32,
        (e, l): ((i32, i32), (i32, i32)),
        site: (i32, i32),
        step: u8,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let set = |s: &mut Self, land: ((i32, i32), (i32, i32)), step: u8| {
            if let Some(Task::Take {
                land: lc, step: st, ..
            }) = s.objects[i].ship.as_mut().unwrap().task.as_mut()
            {
                *lc = Some(land);
                *st = step;
            }
        };
        let Some(g) = self.object_index(target) else {
            if step == 0 || (step == 1 && self.objects[i].ship.as_ref().unwrap().landing.is_none())
            {
                self.objects[i].ship.as_mut().unwrap().task = None;
                return;
            }
            if step == 5 {
                return;
            }
            return self.take_landing_step(i, site, data, caches);
        };
        match step {
            0 => {
                let s = self.objects[i].ship.as_ref().unwrap();
                if !s.path.is_empty() || s.move_state != 0 {
                    return;
                }
                if self.cell_of(i) != e {
                    let w = TILE_SIZE as f32;
                    let to = ((e.0 as f32 + 0.5) * w, (e.1 as f32 + 0.5) * w);
                    match self.plan_route(i, to.0, to.1) {
                        Some(path) => self.objects[i].ship.as_mut().unwrap().path = path,
                        None => self.objects[i].ship.as_mut().unwrap().task = None,
                    }
                    return;
                }
                // The house: its cell's slot holding it (the port's house
                // always stands on its cell); a treasure: nothing there.
                let house = self.objects[g].class == ObjectClass::HeroHouse;
                if house || !self.standing_at(l) {
                    set(self, (e, l), 1);
                } else if let Some(cells) = self.land_take(i, self.cell_of(g)) {
                    set(self, cells, 0);
                }
            }
            1 if self.objects[i].ship.as_ref().unwrap().landing.is_none() => {
                if let Some(lnd) = self.take_close_in(i, l) {
                    self.objects[i].ship.as_mut().unwrap().landing = Some(lnd);
                }
            }
            1 => self.take_landing_step(i, site, data, caches),
            _ => {}
        }
    }

    /// A tick of the take's landing past its close-in, the party's site
    /// `site`.
    fn take_landing_step(
        &mut self,
        i: usize,
        site: (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        self.take_landing_tick(i, site, data, caches);
    }

    /// A game tick of 0x4b98b0 on every ship (the ship tick 0x4ab930 at
    /// 0x4abbdb, while `+0x25d & 0x1000`, which 0x4ab9c6 sets when slot 0
    /// or 1 mends or Argus is aboard; not on a ship sunk, being built,
    /// `+0x251` 2, or turned to stone, `+0x257`, whose tick returns
    /// first). Each treasure slot of a Cup (15) or a Fleece (16) adds its
    /// amount to its own count, Argus 20 to his; `n` = count / 1000 heals
    /// (the count kept modulo 1000): the Cup each worn unit of items 0-2 by
    /// `n` (to 100), the Fleece every item's; the Fleece and Argus, while
    /// `hull x 100 / full` is under 100, add `full x n / 100` to the hull
    /// (to full). Anything healed runs the stats refresh 0x4bee60.
    pub(super) fn heal_aboard(&mut self) {
        let mut refresh = Vec::new();
        for o in &mut self.objects {
            let barbarian = o.owner == Some((4, 0));
            let (Some(s), Some(c)) = (o.ship.as_mut(), o.combat.as_ref()) else {
                continue;
            };
            if c.hull <= 0.0 || s.build.is_some() || c.stone_len > 0 {
                continue;
            }
            s.sync_heal_slots(barbarian);
            let argus = s.heroes.contains(&5);
            if !argus && !s.heal_slots.iter().take(2).any(|h| h.mends) {
                continue;
            }
            let full = crate::combat::hull_points(s.kind) as i64;
            let hull0 = (c.hull * 15.0).round() as i64;
            let mut hull = hull0;
            // 0x4b9a2a / 0x4b9b2c: 1% of the full hull a heal, from below
            // full only.
            let mend = |hull: &mut i64, n: u32| {
                if *hull * 100 / full < 100 {
                    *hull = (*hull + full * n as i64 / 100).min(full);
                    true
                } else {
                    false
                }
            };
            let mut healed = false;
            let mut points = [0u32; 9];
            for (tr, slot) in s.treasures.iter().zip(s.heal_slots.iter_mut()) {
                let items = match tr.kind {
                    15 => 3,
                    16 => 9,
                    _ => continue,
                };
                slot.count += tr.amount;
                let n = slot.count / 1000;
                if n == 0 {
                    continue;
                }
                slot.count %= 1000;
                points[..items].iter_mut().for_each(|p| *p += n);
                if tr.kind == 16 {
                    healed |= mend(&mut hull, n);
                }
            }
            if argus {
                s.argus_count += ARGUS_RATE;
                let n = s.argus_count / 1000;
                if n != 0 {
                    s.argus_count %= 1000;
                    healed |= mend(&mut hull, n);
                }
            }
            if points != [0; 9] {
                let units = o.units().unwrap();
                for (u, &p) in units.iter_mut().zip(&points) {
                    for c in u.iter_mut().filter(|c| **c < 100 && p > 0) {
                        *c = (*c as u32 + p).min(100) as u8;
                        healed = true;
                    }
                }
            }
            if hull != hull0 {
                o.combat.as_mut().unwrap().hull = hull as f32 / 15.0;
            }
            if healed {
                refresh.push(o.id);
            }
        }
        for id in refresh {
            self.refresh_stats(id);
        }
    }

    /// A fort storing a Cup of Asclepius or a Golden Fleece (0x419e00 sets
    /// `+0x4b8`/`+0x4c4` and the rate `+0x4bc`, the treasure's amount; the
    /// tick of 0x419f80 heals): every 1000 of the rate a point, the Cup to
    /// its marines, else the Fleece to every unit and a percent to the
    /// structure.
    pub(super) fn heal_forts(&mut self, dt: f32) {
        let ticks = dt / crate::clock::TICK;
        let gifts: Vec<(u32, bool, f32)> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Building(0x34..=0x36)) && o.holds_units())
            .filter_map(|o| {
                let held = self.held_by(o.id);
                let cup = held.iter().any(|tr| tr.kind == 15);
                let fleece = held.iter().any(|tr| tr.kind == 16);
                let rate = held
                    .iter()
                    .rev()
                    .find(|tr| tr.kind == 15 || tr.kind == 16)?
                    .amount as f32;
                (cup || fleece).then_some((o.id, cup, rate))
            })
            .collect();
        for (id, cup, rate) in gifts {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let o = &mut self.objects[i];
            let c = o.combat.as_mut().unwrap();
            c.healing += rate * ticks;
            let heal = (c.healing / HEAL_STEP) as u32;
            c.healing -= heal as f32 * HEAL_STEP;
            if heal == 0 {
                continue;
            }
            let mut places = o.fort_places();
            let items = if cup { 0..3 } else { 0..9 };
            for c in places[items].iter_mut().flatten() {
                *c = (*c as u32 + heal).min(100) as u8;
            }
            o.set_fort_places(places);
            if !cup {
                // The structure's percentage (`+0xa8`) and its points.
                let c = o.combat.as_mut().unwrap();
                let pct = ((c.hull / c.max_hull.max(1.0)) * 100.0) as u32 + heal;
                c.hull = c.max_hull * pct.min(100) as f32 / 100.0;
            }
        }
    }

    /// Ship `i` takes the treasure object `g`.
    fn take_treasure(&mut self, i: usize, g: usize, data: &GameData, caches: &mut Caches) {
        let tr = self.objects[g].treasure.unwrap();
        let id = self.objects[g].id;
        let owner = self.objects[i].owner;
        let (x, y) = (self.objects[g].x, self.objects[g].y);
        let t = TILE_SIZE as f32;
        let cell = ((x / t) as i32, (y / t) as i32);
        let ship_id = self.objects[i].id;
        let name = objects::treasure_name(tr.kind);
        if self.is_own(&self.objects[i]) {
            self.stats.treasures_found += 1;
            // Its crew cheers (events 168-171 by nation, 0x4a5bc0).
            if matches!(owner, Some((1 | 3, _))) {
                let (sx, sy) = (self.objects[i].x, self.objects[i].y);
                self.sounds.push(("pit3", sx, sy));
            }
        }
        match tr.kind {
            0..=5 => {
                if let Some(owner) = owner {
                    let amount = tr.amount as i64;
                    if tr.kind <= 2 {
                        self.earn(owner, amount, 0);
                        self.messages.push(format!("Found {amount} amber"));
                    } else {
                        self.earn(owner, 0, amount);
                        self.messages.push(format!("Found {amount} fish"));
                    }
                }
            }
            8 => {
                // A spell bursts out where the treasure lay: protections on
                // the ship, or reefs, whirlpools, waterspouts and monsters
                // around it.
                match Spell::of_index(tr.param) {
                    Some(
                        s @ (Spell::Reef | Spell::Whirlpool | Spell::Waterspout | Spell::Monster),
                    ) => {
                        let _ = self.spawn_hazard(s, cell.0, cell.1, None, data, caches);
                        self.messages.push(format!("{name}: {}", s.name()));
                    }
                    Some(s @ (Spell::Phantom | Spell::ProtectWeapons | Spell::ProtectMagic)) => {
                        let weapon = s == Spell::ProtectWeapons;
                        let c = self.objects[i].combat.as_ref().unwrap();
                        let span = if weapon { c.weapon_span } else { c.magic_span };
                        if s != Spell::Phantom && span.is_none() {
                            self.ship_shield_cast(i, weapon, data, caches);
                        }
                        let now = self.ticks;
                        let c = self.objects[i].combat.as_mut().unwrap();
                        let (left, span) = match s {
                            Spell::Phantom => (&mut c.phantom, &mut c.phantom_span),
                            Spell::ProtectWeapons => (&mut c.protect_weapons, &mut c.weapon_span),
                            _ => (&mut c.protect_magic, &mut c.magic_span),
                        };
                        // A ship's phantom and shields in game ticks
                        // (`tick_ship_shields`). (The treasure's route to
                        // the casting not read: no casting sound.)
                        *left = crate::combat::Combatant::extend_span(
                            span,
                            now,
                            crate::magic::PROTECTION_TICKS,
                        );
                        self.messages.push(format!("{name}: {}", s.name()));
                    }
                    _ => {}
                }
            }
            9 => {
                // A spell to cast once, at no cost.
                if let Some(s) = Spell::of_index(tr.param) {
                    *self.free_casts.entry(s).or_default() += 1;
                    self.messages.push(format!("{name}: one free {}", s.name()));
                }
            }
            _ => {
                let s = self.objects[i].ship.as_mut().unwrap();
                s.add_treasure(tr, owner == Some((4, 0)));
                self.messages.push(format!("Taken aboard: {name}"));
            }
        }
        let _ = ship_id;
        // The treasure answers message 0x158c1 (0x4074d0) by going to
        // state 3, 0x406c80: off its cell, sound event 272 (`trea2`).
        self.sounds.push(("trea2", x, y));
        if let Some(g) = self.object_index(id) {
            self.objects.remove(g);
        }
    }

    /// Puts treasure `tr` on the map at the water cell nearest the world
    /// point `(x, y)`, where ships can reach it.
    pub(super) fn place_treasure(
        &mut self,
        tr: Treasure,
        x: f32,
        y: f32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let t = TILE_SIZE as f32;
        let Some((cx, cy)) = self.nav.nearest_water((x / t) as i32, (y / t) as i32) else {
            return;
        };
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 12];
        fields[0] = 0x2bc;
        fields[4] = tr.kind as u32;
        fields[5] = u32::MAX;
        fields[7] = tr.param;
        fields[8] = tr.amount;
        fields[9] = cx as u32;
        fields[10] = cy as u32;
        let rec = openconquest_formats::ObjectRecord {
            id,
            class_id: 0x2bc,
            fields,
        };
        if let Ok(Some(obj)) = objects::resolve(data, caches, &rec) {
            self.objects.push(obj);
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        }
    }

    /// The treasures object `id` held fall where it stood.
    pub(super) fn drop_held(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        let (dropped, kept): (Vec<_>, Vec<_>) = self.held.drain(..).partition(|h| h.0 == id);
        self.held = kept;
        for (_, tr) in dropped {
            self.place_treasure(tr, x, y, data, caches);
            self.messages.push(format!(
                "{} lies in the wreckage",
                objects::treasure_name(tr.kind)
            ));
        }
    }

    /// Heroes' items neither on the map nor in an inventory (the
    /// campaign's scripts would hand them out) go to the enemy building or
    /// nest nearest each hero's house, and drop when it is destroyed; with
    /// none about, they lie by the house.
    pub(super) fn hide_hero_items(&mut self, heroes: &[u8], data: &GameData, caches: &mut Caches) {
        for &h in heroes {
            let kind = 18 + h;
            if self
                .objects
                .iter()
                .any(|o| o.treasure.is_some_and(|t| t.kind == kind))
                || self.held.iter().any(|h| h.1.kind == kind)
            {
                continue;
            }
            let Some(house) = self.objects.iter().find(|o| {
                o.class == ObjectClass::HeroHouse
                    && o.record.field(22).unwrap_or(0).min(8) as u8 == h
            }) else {
                continue;
            };
            let (hx, hy) = (house.x, house.y);
            let holder = self
                .objects
                .iter()
                .filter(|o| {
                    o.combat.is_some() && o.ship.is_none() && o.owner.is_some() && !self.is_own(o)
                })
                .min_by(|a, b| {
                    let d =
                        |o: &&crate::objects::MapObject| (o.x - hx).powi(2) + (o.y - hy).powi(2);
                    d(a).total_cmp(&d(b))
                })
                .map(|o| o.id);
            let tr = Treasure {
                kind,
                param: 0,
                amount: 0,
            };
            match holder {
                Some(id) => self.held.push((id, tr)),
                None => self.place_treasure(tr, hx, hy, data, caches),
            }
        }
    }

    /// Ship `k`'s take is of a hero's house (`+0x76e` 0x49).
    pub(super) fn house_take(&self, k: usize) -> bool {
        matches!(
            self.objects[k].ship.as_ref().and_then(|s| s.task),
            Some(Task::Take { hero: Some(_), .. })
        )
    }

    /// 0x4ba6b0 at the walkway's last frame: the object on the land cell
    /// (`+0x797`'s slot `+0xc`) must be the house (class 0x49, id
    /// `+0x77e`); its answer to 0x24af0 (0x421150: the hero `+0x1e4`, or
    /// 0xff once taken) into `+0x78a`; then true when the hero is not
    /// 0xff and his item is aboard (0x4ba3f0).
    pub(super) fn house_answer(&mut self, k: usize) -> bool {
        let Some(Task::Take {
            target,
            land: Some((_, l)),
            ..
        }) = self.objects[k].ship.as_ref().and_then(|s| s.task)
        else {
            return false;
        };
        let Some(g) = self
            .object_index(target)
            .filter(|&g| self.objects[g].class == ObjectClass::HeroHouse)
            .filter(|&g| self.objects[g].cell == Some(l))
        else {
            return false;
        };
        let answer = if self.taken_houses.iter().any(|&(id, _)| id == target) {
            0xff
        } else {
            self.objects[g].record.field(22).unwrap_or(0) as u8
        };
        let s = self.objects[k].ship.as_mut().unwrap();
        if let Some(Task::Take { hero, .. }) = s.task.as_mut() {
            *hero = Some(answer);
        }
        Self::hero_item(s, answer).is_some() && answer != 0xff
    }

    /// 0x4ba3f0(hero, 0): the first treasure slot of hero `hero`'s item
    /// (kind 0x12 + hero; none for another hero or with nothing aboard).
    fn hero_item(s: &objects::Ship, hero: u8) -> Option<usize> {
        s.treasures
            .iter()
            .position(|tr| objects::treasure_hero(tr.kind) == Some(hero))
    }

    /// 0x4ba2a0(`+0x78a`, ok): for the viewer's ship only (`+0x24f`
    /// against the game's `+0x3213 + 4 x +0x3212`), the hero's yes or no
    /// (`HERO_VOICES`), mode 2 at the ship's cell; any other answer (0xff,
    /// or the house itself before 0x4ba6b0's) none.
    pub(super) fn hero_voice(&mut self, k: usize, ok: bool) {
        let o = &self.objects[k];
        let Some(Task::Take {
            hero: Some(hero), ..
        }) = o.ship.as_ref().and_then(|s| s.task)
        else {
            return;
        };
        let Some(&(yes, no)) = HERO_VOICES.get(hero as usize) else {
            return;
        };
        if o.owner.is_none() || o.owner != self.human.map(|h| (h.nation, h.slot)) {
            return;
        }
        let (x, y) = (o.x, o.y);
        self.sounds.push((if ok { yes } else { no }, x, y));
    }

    /// Step 6 at half the party's frames for the hero's house: 0x4bd350
    /// takes hero `+0x78a` aboard with a hero place free (0x4be800 mode 7:
    /// 0x597b08[type] less the heroes aboard; Argus's count from 0);
    /// aboard, 0x4ba7d0 sends the house 0x24af1 (`house_taken`), 0x4b6df0
    /// message 0x5d99 to the owner's player (not ported: the judge counts
    /// the heroes aboard), and the hero's item 0x4ba3f0 finds is taken
    /// out (0x4bd500, not dropped).
    pub(super) fn hero_boards(&mut self, k: usize, data: &GameData, caches: &mut Caches) {
        let Some(Task::Take {
            target,
            hero: Some(hero),
            ..
        }) = self.objects[k].ship.as_ref().and_then(|s| s.task)
        else {
            return;
        };
        let s = self.objects[k].ship.as_mut().unwrap();
        if objects::hero_room(s.kind) as i32 - (s.heroes.len() as i32) < 1 {
            return;
        }
        s.add_hero(hero);
        self.house_taken(target, data, caches);
        let s = self.objects[k].ship.as_mut().unwrap();
        if let Some(idx) = Self::hero_item(s, hero) {
            s.remove_treasure(idx);
        }
        if let Some(name) = objects::HERO_NAMES.get(hero as usize) {
            self.messages.push(format!("{name} comes aboard"));
        }
    }

    /// The house `id` taken (0x421150 message 0x24af1): taken `+0x1e8` 1,
    /// state `+0xa4` 4, the tick `+0x1ec` the map's `+0xde`, its pictures
    /// reloaded (vt+0x38, 0x422e30: `HHOUSE<hero>SPR_043`, the frame the
    /// standing picture was at kept unless past the new count, `+0xbc`
    /// cleared). Its own tick (message 0) runs only taken and in state 1,
    /// which a taken house never is again: the 500-tick fade there and its
    /// 0x138b are not reached. The house plays out instead
    /// (`step_house_fades`).
    pub(super) fn house_taken(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some(g) = self.object_index(id) else {
            return;
        };
        self.taken_houses.retain(|&(h, _)| h != id);
        self.taken_houses.push((id, self.ticks));
        let standing = self.standing_frames.remove(&id).map_or(0, |s| s.frame);
        self.show_house(g, 4, data, caches);
        let frames = self.objects[g].frames.len() as u32;
        let frame = if standing >= frames { 0 } else { standing };
        self.house_fades.insert(
            id,
            HouseFade {
                state: 4,
                frame,
                frames,
                shown: frame,
                started: false,
                wait: 0,
            },
        );
    }

    /// A house's picture in state `state` (`HHOUSE<hero>SPR_0<state>3`).
    pub(super) fn show_house(&mut self, g: usize, state: u8, data: &GameData, caches: &mut Caches) {
        let hero = self.objects[g].record.field(22).unwrap_or(0).min(8) as u8;
        if let Ok(frames) = objects::hero_house_frames(data, caches, hero, state) {
            if !frames.is_empty() {
                self.objects[g].frames = frames;
            }
        }
    }

    /// The taken houses' picture steps, LandObj's vt+0x3c 0x4230f0 with
    /// the house's cycles (`HOUSE_CYCLE`): with `+0xbc` 0 or
    /// `now >= last + cycle[state] / n`, the frame drawn and one added; the step
    /// reaching `n` goes on by vt+0x48 (0x421050: 4 to 5, 5 to 2), the
    /// frame 0 and the state's picture loaded (vt+0x38, `+0xbc` cleared:
    /// `HHOUSE<hero>SPR_053`, then `_023`), and in state 2 posts message
    /// 10 to itself (0x4232ed): the house is removed after the tick (its
    /// message 3: 0x421150 frees its name, 0x499860 clears its cell).
    /// Returns the houses removed so.
    pub(super) fn step_house_fades(&mut self, data: &GameData, caches: &mut Caches) -> Vec<u32> {
        let mut done = Vec::new();
        let ids: Vec<u32> = self.house_fades.keys().copied().collect();
        for id in ids {
            let Some(g) = self.object_index(id) else {
                self.house_fades.remove(&id);
                continue;
            };
            let f = self.house_fades.get_mut(&id).unwrap();
            let n = f.frames.max(1);
            let delay = HOUSE_CYCLE[f.state as usize] / n;
            if f.started {
                f.wait += 1;
                if f.wait < delay {
                    continue;
                }
            }
            f.started = true;
            f.wait = 0;
            f.shown = f.frame;
            f.frame += 1;
            if f.frame < n {
                continue;
            }
            let next = match f.state {
                4 => 5,
                5 => 2,
                _ => {
                    done.push(id);
                    continue;
                }
            };
            self.show_house(g, next, data, caches);
            let frames = self.objects[g].frames.len() as u32;
            let f = self.house_fades.get_mut(&id).unwrap();
            *f = HouseFade {
                state: next,
                frames,
                ..HouseFade::default()
            };
        }
        done
    }

    /// The frame a taken house shows.
    pub(super) fn house_frame(&self, id: u32) -> Option<u32> {
        self.house_fades.get(&id).map(|f| f.shown)
    }

    /// 0x4a8fa0: goods of deposit class `class` credited to `owner`:
    /// amber (400) x 3 x 40 / 100, fish (0x191) one for one, a
    /// leviathan's (0x192) x 125 x 40 / 100 in amber; any other class
    /// nothing.
    pub(super) fn credit_class(&mut self, owner: (u32, u32), class: u32, amount: i64) {
        match class {
            400 => self.earn(owner, amount * 3 * 40 / 100, 0),
            0x191 => self.earn(owner, 0, amount),
            0x192 => self.earn(owner, amount * 125 * 40 / 100, 0),
            _ => {}
        }
    }

    /// A pack (kind 7) handed to settlement `site` (0x441480): field 7's
    /// low word its type, byte 2 the item, field 8 how many. Units (1)
    /// join the stock whole, one by one (0x440d40 with 100, refused while
    /// the stock is mended); armour (2) into its list (`+0x248`) but level
    /// 0; rams (4) into theirs (`+0x24c`) but level 3 (none); any other
    /// type nothing. The pack is used up either way, unannounced.
    pub(super) fn unpack(&mut self, site: u32, tr: Treasure) {
        let item = (tr.param >> 16) as u8;
        match tr.param as u16 {
            1 => {
                for _ in 0..tr.amount {
                    if self.stock_repair_at(site).is_some() || item >= 9 {
                        continue;
                    }
                    let mut stock = self.stock_units(site);
                    stock[item as usize].push(100);
                    self.set_stock_units(site, stock);
                }
            }
            2 if item != 0 => {
                let f = self.fittings.entry(site).or_default();
                f.0.extend(std::iter::repeat_n(item, tr.amount as usize));
            }
            4 if item != 3 => {
                let f = self.fittings.entry(site).or_default();
                f.1.extend(std::iter::repeat_n(item - 3, tr.amount as usize));
            }
            _ => {}
        }
    }
}
