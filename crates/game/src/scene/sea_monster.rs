//! Sea monsters (UMonsterClassTy, umonster.cpp): under water they wait,
//! choose a prey or a cell, and come up to strike or to swim about, then
//! dive again (0x491fa0 a tick, states entered by 0x491830).

use super::*;
use crate::combat::Rng;

/// The cells about a target, in the order 0x491030 tries them (0x5ba7d8).
const AROUND: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
];

/// The frame of the strike its bite falls on, by type (0x5ba4d8).
const BITE_FRAME: [u32; 4] = [10, 6, 6, 9];

/// A strike's full blow (0x490830: its strength in percent x 37500 / 100).
const BITE: u32 = 37_500;

const UNDER: u8 = 0;
const SURFACING: u8 = 1;
const SWIMMING: u8 = 2;
const DIVING: u8 = 3;
const STRIKING: u8 = 4;
const DYING: u8 = 6;
const GONE: u8 = 0xff;

/// What it has chosen to go for (+0x109).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Prey {
    /// 1 a monster to eat, 2 a ship.
    kind: u8,
    cell: (i32, i32),
    /// The cell it strikes from.
    from: (i32, i32),
}

/// A sea monster's state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SeaMonster {
    state: u8,
    /// Its ticks, and the seconds owed to the next.
    now: u32,
    clock: f32,
    /// When the state began (+0xf1) and, under water, how long it waits
    /// (+0xf5).
    start: u32,
    wait: u32,
    /// Its picture: the tick its last frame was drawn (+0xe9), ticks a
    /// frame (+0xed), the frame and the loop's first, loops done and
    /// wanted (+0xf9, +0xfd).
    last: Option<u32>,
    delay: u32,
    frame: u32,
    first: u32,
    loops: u32,
    wanted: u32,
    /// Struck while up: a frame every tick, no swim (+0x101).
    hurried: bool,
    /// Must swim off before it strikes a ship again (+0x105).
    must_move: bool,
    /// 2 whole, 1 below half its strength, 0 below a tenth (+0xb1).
    mode: u8,
    heading: u8,
    kind: u8,
    /// The cells it keeps to: x0, x1, y0, y1 (+0x99).
    area: (i32, i32, i32, i32),
    /// Half the width and height of what it looks over (+0xa9, +0xad).
    scan: (i32, i32),
    /// The ship that last struck it (+0x129).
    attacker: Option<u32>,
    prey: Option<Prey>,
    rng: Rng,
}

/// The direction from `from` to `to` (0x48f100): east 0, counting
/// counter-clockwise; none 0.
fn heading(to: (i32, i32), from: (i32, i32)) -> u8 {
    match ((to.0 - from.0).signum(), (to.1 - from.1).signum()) {
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

/// 0x557440: the longer side plus half the shorter.
fn reach(a: (i32, i32), b: (i32, i32)) -> i32 {
    let (dx, dy) = ((a.0 - b.0).abs(), (a.1 - b.1).abs());
    (dx.min(dy) + 2 * dx.max(dy)) / 2
}

/// 0x5576d8: the distance, its integer square root rounded (0x557670).
fn distance(a: (i32, i32), b: (i32, i32)) -> i32 {
    let d2 = ((a.0 - b.0).pow(2) + (a.1 - b.1).pow(2)) as f64;
    (d2.sqrt() + 0.5).floor() as i32
}

/// A weighted pick (0x495e80): one choice needs no draw; else `r % (sum +
/// 1)`, the first whose running sum reaches it.
fn pick(weights: &[u32], rng: &mut Rng) -> usize {
    if weights.len() <= 1 {
        return 0;
    }
    let total: u32 = weights.iter().sum();
    let r = rng.next() % (total + 1);
    let mut sum = 0;
    for (k, &w) in weights.iter().enumerate() {
        sum += w;
        if r <= sum {
            return k;
        }
    }
    weights.len() - 1
}

impl Scene {
    fn cell_at(o: &MapObject) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        ((o.x / t).floor() as i32, (o.y / t).floor() as i32)
    }

    /// A new sea monster (0x492360 message 2): under water, its strength
    /// fields 5 and 6 in percent of 50000, its area fields 11 and 12 about
    /// its cell (the whole map if either is 0), its scan fields 13 and 14,
    /// its mode field 15 and its type field 10.
    fn new_sea_monster(&mut self, i: usize) -> SeaMonster {
        let fields = self.objects[i].record.fields.clone();
        let f = |k: usize| fields.get(k).copied().unwrap_or(0) as i32;
        let (x, y) = Self::cell_at(&self.objects[i]);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let area = if f(11) < 1 || f(12) < 1 {
            (0, w - 1, 0, h - 1)
        } else {
            (
                (x - f(11)).max(0),
                (x + f(11)).min(w - 1),
                (y - f(12)).max(0),
                (y + f(12)).min(h - 1),
            )
        };
        let mode = f(15).clamp(0, 2) as u8;
        if let Some(c) = self.objects[i].combat.as_mut() {
            let full = combat::creature_strength(0x1fb);
            let (max, now) = (f(5), f(6));
            if max > 0 {
                c.max_hull = full * max as f32 / 100.0;
                c.hull = full * now as f32 / 100.0;
            }
        }
        let id = self.objects[i].id;
        let mut m = SeaMonster {
            state: UNDER,
            now: 0,
            clock: 0.0,
            start: 0,
            wait: 0,
            last: None,
            delay: 2,
            frame: 0,
            first: 0,
            loops: 0,
            wanted: 0,
            hurried: false,
            must_move: mode < 1,
            mode,
            heading: 0,
            kind: objects::sea_monster_type(f(10) as u32) as u8,
            area,
            scan: (f(13), f(14)),
            attacker: None,
            prey: None,
            rng: Rng(id.wrapping_mul(0x9e37_79b9)),
        };
        m.wait = m.rng.next() % 76 + 50;
        m
    }

    /// Its strength and full strength in the original's units.
    fn monster_strength(&self, i: usize) -> (u32, u32) {
        self.objects[i].combat.as_ref().map_or((0, 1), |c| {
            (
                (c.hull.max(0.0) * 15.0).round() as u32,
                (c.max_hull * 15.0).round().max(1.0) as u32,
            )
        })
    }

    fn set_strength(&mut self, i: usize, now: u32) {
        if let Some(c) = self.objects[i].combat.as_mut() {
            c.hull = now as f32 / 15.0;
        }
    }

    fn sea_monster_at(&self, cell: (i32, i32), but: u32) -> Option<usize> {
        (0..self.objects.len()).find(|&k| {
            let o = &self.objects[k];
            o.id != but
                && o.class == ObjectClass::Monster(0x1fb)
                && Self::cell_at(o) == cell
                && self
                    .sea_monster_states
                    .get(&o.id)
                    .is_none_or(|m| m.state != GONE)
        })
    }

    fn ship_at(&self, cell: (i32, i32)) -> Option<usize> {
        (0..self.objects.len())
            .find(|&k| self.objects[k].ship.is_some() && Self::cell_at(&self.objects[k]) == cell)
    }

    /// What lies on a cell's third place (+0x1c): a treasure lying there,
    /// a reef, a whirlpool, a flock, a volcano's crater; true if a
    /// treasure.
    fn third_place(&self, cell: (i32, i32)) -> Option<bool> {
        self.third_place_object(cell)
            .map(|k| self.objects[k].class == ObjectClass::Treasure)
    }

    /// The object on a cell's third place (+0x1c), by index.
    pub(super) fn third_place_object(&self, cell: (i32, i32)) -> Option<usize> {
        (0..self.objects.len()).find(|&k| {
            let o = &self.objects[k];
            let f = |k| o.record.field(k).unwrap_or(0) as i32;
            let at = match o.class {
                ObjectClass::Treasure => Self::cell_at(o),
                ObjectClass::Reef(_) | ObjectClass::Whirlpool => (f(5), f(6)),
                ObjectClass::Sheep => (f(4), f(5)),
                ObjectClass::Lava => (f(6), f(7)),
                _ => return false,
            };
            at == cell
        })
    }

    /// Whether monster `id` may be on cell `c` (0x402ee0): water; with
    /// `free` no ship there and nothing on its third place but, with
    /// `room`, a treasure; no other monster there. (Not ported: the test
    /// of the object at the cell's +8, vfunc +0x30, not decoded.)
    fn monster_may_be(&self, id: u32, c: (i32, i32), free: bool, room: bool) -> bool {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        if c.0 < 0 || c.1 < 0 || c.0 >= w || c.1 >= h {
            return false;
        }
        let types = self.cell_types[(c.1 * w + c.0) as usize];
        if types & 0xf > 7 || (types >> 4) & 0xf > 3 {
            return false;
        }
        if free {
            if self.ship_at(c).is_some() {
                return false;
            }
            match self.third_place(c) {
                Some(true) if room => {}
                Some(_) => return false,
                None => {}
            }
        }
        self.sea_monster_at(c, id).is_none()
    }

    fn in_area(m: &SeaMonster, c: (i32, i32)) -> bool {
        (m.area.0..=m.area.1).contains(&c.0) && (m.area.2..=m.area.3).contains(&c.1)
    }

    /// The cell beside `target` it strikes from (0x491030): within two of
    /// it, in its area, free; the last one nearer than the first, else the
    /// first.
    fn strike_cell(
        &self,
        id: u32,
        m: &SeaMonster,
        here: (i32, i32),
        target: (i32, i32),
    ) -> Option<(i32, i32)> {
        let kept: Vec<(i32, i32)> = AROUND
            .iter()
            .map(|&(dx, dy)| (target.0 + dx, target.1 + dy))
            .filter(|&c| {
                Self::in_area(m, c)
                    && (here.0 - c.0).abs() < 3
                    && (here.1 - c.1).abs() < 3
                    && self.monster_may_be(id, c, true, false)
            })
            .collect();
        let first = *kept.first()?;
        let d0 = distance(first, here);
        Some(
            kept.iter()
                .rev()
                .find(|&&c| distance(c, here) < d0)
                .copied()
                .unwrap_or(first),
        )
    }

    /// Whether ship `k` is seen: a phantom one time in ten (0x4911b0).
    fn monster_sees(&self, k: usize, rng: &mut Rng) -> bool {
        let phantom = self.objects[k]
            .combat
            .as_ref()
            .is_some_and(|c| c.phantom > 0.0);
        !phantom || rng.next() % 10 == 1
    }

    /// The ship that last struck it, if still to be gone for (0x490ad0).
    fn monster_attacker(&self, m: &mut SeaMonster) -> Option<(usize, (i32, i32))> {
        let k = m.attacker.and_then(|a| self.object_index(a));
        let found = k.filter(|&k| self.objects[k].ship.is_some()).and_then(|k| {
            let c = Self::cell_at(&self.objects[k]);
            (self.monster_sees(k, &mut m.rng) && Self::in_area(m, c)).then_some((k, c))
        });
        if found.is_none() {
            m.attacker = None;
        }
        found
    }

    /// Its prey (0x4911f0): the ship that struck it; else, over the cells
    /// it looks at, a weaker enemy monster under water to eat at once, or
    /// the enemy ships it can reach, those most still preferred, picked by
    /// how near a bite brings them to sinking. 0 none, 1 a monster, 2 a
    /// ship.
    fn monster_prey(&self, i: usize, m: &mut SeaMonster) -> u8 {
        m.prey = None;
        let me = &self.objects[i];
        let id = me.id;
        let here = Self::cell_at(me);
        if let Some((_, c)) = self.monster_attacker(m) {
            if let Some(from) = self.strike_cell(id, m, here, c) {
                m.prey = Some(Prey {
                    kind: 2,
                    cell: c,
                    from,
                });
                return 2;
            }
        }
        let (now, max) = self.monster_strength(i);
        let blow = now * 100 / max * BITE / 100;
        // (prey, weight, score)
        let mut list: Vec<(Prey, u32, u32)> = Vec::new();
        for x in here.0 - m.scan.0..=here.0 + m.scan.0 {
            for y in here.1 - m.scan.1..=here.1 + m.scan.1 {
                let c = (x, y);
                if !Self::in_area(m, c) {
                    continue;
                }
                if let Some(k) = self.sea_monster_at(c, id) {
                    let other = &self.objects[k];
                    let under = self
                        .sea_monster_states
                        .get(&other.id)
                        .is_none_or(|o| o.state == UNDER);
                    let theirs = self.monster_strength(k).0;
                    if under && 0 < theirs && theirs < now && Self::are_enemies(me, other) {
                        if let Some(from) = self.strike_cell(id, m, here, c) {
                            m.prey = Some(Prey {
                                kind: 1,
                                cell: c,
                                from,
                            });
                            return 1;
                        }
                    }
                }
                let Some(k) = self.ship_at(c) else { continue };
                let o = &self.objects[k];
                let s = o.ship.as_ref().unwrap();
                // Held (move state 6) are passed over. A ship in a dock is
                // not held (no dock code writes `+0x58c`), but it is off
                // its cell's ship slot (0x4a3970 on entering), so the
                // original's look at the cell does not find it either.
                // (Not settled: whether every dock entry takes it off.)
                let held = s.move_state == 6 || self.in_dock(k);
                if !Self::are_enemies(me, o) || held || !self.monster_sees(k, &mut m.rng) {
                    continue;
                }
                let Some(from) = self.strike_cell(id, m, here, c) else {
                    continue;
                };
                // By its move state (+0x58c): lying still 100, told to stop
                // 70, turning 80, at a cell 90, braking 60, under way 50.
                let weight = match s.move_state {
                    0 => 100,
                    1 => 70,
                    2 => 80,
                    3 => 90,
                    4 => 60,
                    5 => 50,
                    _ => 0,
                };
                // What a bite leaves of its hull (0x4e4bc0).
                let cb = o.combat.as_ref().unwrap();
                let sh = combat::Shares::of(s, &cb.weapons);
                let levels = self.levels_of(o.owner);
                let cut = combat::armour_factor(s.armour, &levels);
                let lost = sh.hull_part(blow as f32 * cut) as i64;
                let v = (cb.hull * 15.0) as i64 - lost;
                let score = if v < 1 {
                    (100_000 - v) / 100
                } else {
                    (100_000 - v) / 300
                };
                list.push((
                    Prey {
                        kind: 2,
                        cell: c,
                        from,
                    },
                    weight,
                    score.max(1) as u32,
                ));
            }
        }
        if list.is_empty() {
            return 0;
        }
        let top = list.iter().map(|e| e.1).max().unwrap();
        let scores: Vec<u32> = list.iter().filter(|e| e.1 == top).map(|e| e.2).collect();
        let k = pick(&scores, &mut m.rng);
        // The pick indexes the whole list (0x4916ab), not the best alone.
        let chosen = list[k.min(list.len() - 1)];
        if chosen.1 < top {
            return 0;
        }
        m.prey = Some(chosen.0);
        2
    }

    /// Swims off under water to a cell within two (0x490d60): towards a
    /// treasure, or towards the ship that struck it. False if none.
    fn monster_wander(&mut self, i: usize, m: &mut SeaMonster) -> bool {
        let id = self.objects[i].id;
        let here = Self::cell_at(&self.objects[i]);
        let attacker = self.monster_attacker(m).map(|(_, c)| c);
        let towards = attacker.map(|a| heading(a, here));
        let mut cells = Vec::new();
        let mut weights = Vec::new();
        for x in here.0 - 2..=here.0 + 2 {
            for y in here.1 - 2..=here.1 + 2 {
                let c = (x, y);
                if !Self::in_area(m, c) || !self.monster_may_be(id, c, true, true) {
                    continue;
                }
                let w = match (attacker, towards) {
                    (Some(a), Some(d)) => {
                        100 / reach(c, a).max(1) as u32
                            + if heading(c, here) == d { 1000 } else { 0 }
                    }
                    _ => {
                        if self.third_place(c) == Some(true) {
                            100
                        } else {
                            10
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
        let to = cells[pick(&weights, &mut m.rng)];
        self.monster_move(i, m, to);
        if m.mode != 0 {
            m.must_move = false;
        }
        true
    }

    /// Moves it to cell `to` at once (0x491780), facing the way it went.
    fn monster_move(&mut self, i: usize, m: &mut SeaMonster, to: (i32, i32)) {
        let here = Self::cell_at(&self.objects[i]);
        m.heading = heading(to, here);
        let t = TILE_SIZE as f32;
        self.objects[i].x = to.0 as f32 * t + t / 2.0;
        self.objects[i].y = to.1 as f32 * t + t / 2.0;
    }

    /// Enters state `state` (0x491830).
    fn monster_enter(
        &mut self,
        i: usize,
        m: &mut SeaMonster,
        state: u8,
        data: &GameData,
        caches: &mut Caches,
    ) {
        m.loops = 0;
        m.start = m.now;
        m.last = None;
        m.state = state;
        m.frame = 0;
        m.first = 0;
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        match state {
            UNDER => {
                m.hurried = false;
                m.wait = m.rng.next() % 76 + 50;
            }
            SURFACING => {
                m.delay = 2;
                m.wanted = 1;
                self.sounds.push(("apse", x, y));
            }
            SWIMMING => {
                m.delay = (m.rng.next() & 1) + 2;
                if m.hurried {
                    m.wanted = 0;
                } else {
                    m.rng.next();
                    m.wanted = 1;
                }
                // With room (fewer than [0x5c1136] = 2 carried) it takes the
                // treasure on its cell's third place (0x491a40-0x491ab0):
                // message 0x158c1 hands over the treasure's five dwords
                // and the treasure goes (0x4074d0, 0x406c80(3): off its
                // cell, sound event 272). Nothing else is told. (Not
                // ported: the treasure's state 5, refused, has no
                // counterpart here; its meaning was not traced.)
                let id = self.objects[i].id;
                if self.held.iter().filter(|h| h.0 == id).count() < 2 {
                    let here = Self::cell_at(&self.objects[i]);
                    if let Some(g) = self
                        .third_place_object(here)
                        .filter(|&g| self.objects[g].class == ObjectClass::Treasure)
                    {
                        if let Some(tr) = self.objects[g].treasure {
                            let (gx, gy) = (self.objects[g].x, self.objects[g].y);
                            self.objects.remove(g);
                            self.held.push((id, tr));
                            self.sounds.push(("trea2", gx, gy));
                        }
                    }
                }
            }
            DIVING => {
                m.delay = 2;
                m.wanted = 1;
            }
            STRIKING => {
                if let Some(p) = m.prey {
                    m.heading = heading(p.cell, Self::cell_at(&self.objects[i]));
                }
                m.delay = 3;
                m.wanted = 1;
                self.sounds.push(("atmons3", x, y));
            }
            DYING => {
                m.kind = 0;
                m.heading = 0;
                m.frame = 3;
                m.first = 3;
                m.delay = 2;
                m.wanted = 1;
                self.sounds.push(("deamons", x, y));
            }
            _ => {}
        }
        if matches!(state, SURFACING | DIVING | STRIKING | DYING) {
            self.monster_picture(i, m, data, caches);
        }
    }

    fn monster_frames(m: &SeaMonster, data: &GameData, caches: &mut Caches) -> Frames {
        let name = match m.state {
            DYING => "UMON040".to_string(),
            SURFACING => format!("UMON{}0{}", m.kind, m.heading),
            SWIMMING => format!("UMON{}1{}", m.kind, m.heading),
            DIVING => format!("UMON{}2{}", m.kind, m.heading),
            _ => format!("UMON{}3{}", m.kind, m.heading),
        };
        objects::sequence_frames(data, caches, &name).unwrap_or_default()
    }

    /// Draws the picture's next frame if its time has come (0x490830),
    /// biting at the strike's bite frame.
    fn monster_picture(
        &mut self,
        i: usize,
        m: &mut SeaMonster,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let delay = if m.hurried { 0 } else { m.delay };
        if m.last.is_some_and(|l| l + delay > m.now) {
            return;
        }
        m.last = Some(m.now);
        let frames = Self::monster_frames(m, data, caches);
        if let Some(layers) = frames.get(m.frame as usize) {
            let mut layers = layers.clone();
            // The strike is drawn between it and its prey (0x490160).
            if m.state == STRIKING {
                const STEP: [(i32, i32); 8] = [
                    (1, 0),
                    (1, -1),
                    (0, -1),
                    (-1, -1),
                    (-1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ];
                let (dx, dy) = STEP[m.heading as usize % 8];
                for l in &mut layers {
                    l.dx += dx * TILE_SIZE as i32 / 2;
                    l.dy += dy * TILE_SIZE as i32 / 2;
                }
            }
            self.objects[i].frames = vec![layers];
        }
        if m.state == STRIKING && m.frame == BITE_FRAME[m.kind as usize % 4] {
            self.monster_bite(i, m, data, caches);
        }
        m.frame += 1;
        if m.frame as usize >= frames.len().max(1) {
            m.frame = m.first;
            m.loops += 1;
        }
    }

    /// Its bite (0x490830, message 0xf166 to the ship on the prey's cell):
    /// its strength in percent x 37500 / 100, three quarters off with
    /// Argus, a fifth under a magic shield; it costs it a twentieth of its
    /// full strength.
    fn monster_bite(&mut self, i: usize, m: &mut SeaMonster, data: &GameData, caches: &mut Caches) {
        let Some(p) = m.prey.filter(|p| p.kind == 2) else {
            return;
        };
        let Some(k) = self.ship_at(p.cell) else {
            return;
        };
        if m.mode != 2 {
            m.must_move = true;
        }
        let (now, max) = self.monster_strength(i);
        let mut blow = now * 100 / max * BITE / 100;
        let o = &self.objects[k];
        if o.ship.as_ref().is_some_and(|s| s.has_hero(objects::ARGUS)) {
            blow -= blow * 75 / 100;
        }
        if o.combat.as_ref().is_some_and(|c| c.protect_magic > 0.0) {
            blow = blow * 20 / 100;
        }
        let (target, me) = (o.id, self.objects[i].id);
        let by = self.objects[i].owner;
        self.share_blow(target, blow as f32 / 15.0, by, data, caches);
        if let Some(i) = self.object_index(me) {
            self.set_strength(i, now.saturating_sub(max * 5 / 100));
        }
    }

    /// Dies: its dying picture where it may, else gone at once.
    fn monster_die(&mut self, i: usize, m: &mut SeaMonster, data: &GameData, caches: &mut Caches) {
        let id = self.objects[i].id;
        let here = Self::cell_at(&self.objects[i]);
        if self.monster_may_be(id, here, true, false) {
            self.monster_enter(i, m, DYING, data, caches);
        } else {
            m.state = GONE;
        }
    }

    /// One tick of monster `i` (0x491fa0).
    fn monster_tick(&mut self, i: usize, m: &mut SeaMonster, data: &GameData, caches: &mut Caches) {
        if m.state == UNDER {
            let (now, max) = self.monster_strength(i);
            if m.now < m.start + m.wait {
                if now == 0 {
                    self.monster_die(i, m, data, caches);
                }
                return;
            }
            let mut now = now;
            if m.mode == 0 {
                now = now.saturating_sub(max / 100);
                self.set_strength(i, now);
            }
            if now == 0 {
                self.monster_die(i, m, data, caches);
                return;
            }
            let pct = now * 100 / max;
            if pct < 51 && m.mode > 0 {
                m.mode = 1;
            }
            if pct < 11 {
                m.mode = 0;
            }
            match self.monster_prey(i, m) {
                1 => {
                    let p = m.prey.unwrap();
                    if let Some(v) = self.sea_monster_at(p.cell, self.objects[i].id) {
                        let theirs = self.monster_strength(v).0;
                        self.set_strength(i, now.saturating_sub(theirs * 80 / 100));
                        self.set_strength(v, 0);
                        self.note_destroyed(v, self.objects[i].owner);
                        self.monster_move(i, m, p.from);
                        self.monster_enter(i, m, STRIKING, data, caches);
                    }
                }
                2 if !m.must_move => {
                    let p = m.prey.unwrap();
                    if self.ship_at(p.cell).is_some() {
                        self.monster_move(i, m, p.from);
                        self.monster_enter(i, m, STRIKING, data, caches);
                    }
                }
                _ => {
                    if self.monster_wander(i, m) {
                        self.monster_enter(i, m, SURFACING, data, caches);
                    } else {
                        self.monster_enter(i, m, UNDER, data, caches);
                    }
                }
            }
            return;
        }
        if m.loops < m.wanted {
            self.monster_picture(i, m, data, caches);
            return;
        }
        match m.state {
            SURFACING => self.monster_enter(i, m, SWIMMING, data, caches),
            SWIMMING => self.monster_enter(i, m, DIVING, data, caches),
            DIVING => self.monster_enter(i, m, UNDER, data, caches),
            STRIKING => {
                // Into the prey's cell, if it may.
                if let Some(p) = m.prey.filter(|p| p.kind == 2) {
                    let id = self.objects[i].id;
                    if self.monster_may_be(id, p.cell, false, false) {
                        self.monster_move(i, m, p.cell);
                    }
                }
                self.monster_enter(i, m, UNDER, data, caches);
            }
            _ => m.state = GONE,
        }
    }

    /// Runs the sea monsters, a tick at a time.
    pub(super) fn run_sea_monsters(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Monster(0x1fb))
            .map(|o| o.id)
            .collect();
        let tick = crate::clock::TICK;
        for id in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let mut m = match self.sea_monster_states.remove(&id) {
                Some(m) => m,
                None => self.new_sea_monster(i),
            };
            m.clock += dt;
            while m.clock >= tick && m.state != GONE {
                m.clock -= tick;
                m.now += 1;
                let Some(i) = self.object_index(id) else {
                    break;
                };
                self.monster_tick(i, &mut m, data, caches);
            }
            if m.state == GONE {
                // Gone (0x491ed0): what it held is left at its cell.
                self.drop_held(id, data, caches);
                self.objects.retain(|o| o.id != id);
                self.summons.retain(|s| s.0 != id);
            } else {
                self.sea_monster_states.insert(id, m);
            }
        }
    }

    /// Whether sea monster `id` is under water, unseen and out of reach
    /// (no picture, +0x139), or dying (no mask).
    pub fn sea_monster_hidden(&self, id: u32) -> bool {
        self.sea_monster_states
            .get(&id)
            .is_some_and(|m| m.state == UNDER || m.state == GONE)
    }

    /// Whether a shot can strike sea monster `id` now: only while it shows
    /// a picture with a mask (0x492ec0: none dying).
    pub(super) fn sea_monster_hittable(&self, id: u32) -> bool {
        self.sea_monster_states
            .get(&id)
            .is_none_or(|m| !matches!(m.state, UNDER | DYING | GONE))
    }

    /// Ship `by` has struck sea monster `id` (0x490c70): it remembers the
    /// first, its area grown to take it in (0x490ba0).
    pub(super) fn sea_monster_struck(&mut self, id: u32, by: Option<u32>) {
        let Some(by) = by.filter(|&b| {
            self.object_index(b)
                .is_some_and(|k| self.objects[k].ship.is_some())
        }) else {
            return;
        };
        let (Some(i), Some(k)) = (self.object_index(id), self.object_index(by)) else {
            return;
        };
        let here = Self::cell_at(&self.objects[i]);
        let at = Self::cell_at(&self.objects[k]);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let Some(m) = self.sea_monster_states.get_mut(&id) else {
            return;
        };
        if m.attacker.is_some() {
            return;
        }
        m.attacker = Some(by);
        if !Self::in_area(m, at) {
            let r = (at.0 - here.0).abs().max((at.1 - here.1).abs()) + 1;
            m.area = (
                (here.0 - r).max(0),
                (here.0 + r).min(w - 1),
                (here.1 - r).max(0),
                (here.1 + r).min(h - 1),
            );
        }
    }

    /// Lightning on sea monster `id` (0xda93): struck on its own cell, under
    /// water it loses half its full strength and waits anew, up it dies,
    /// and left at nothing it is reported killed by `caster` (0x492c2a,
    /// each time); on a cell about, only up, it loses half and hurries, no
    /// death reported.
    pub(super) fn sea_monster_lightning(
        &mut self,
        id: u32,
        centre: bool,
        caster: Option<super::Owner>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let Some(mut m) = self.sea_monster_states.remove(&id) else {
            return;
        };
        let (now, max) = self.monster_strength(i);
        if centre {
            if m.state == UNDER {
                self.set_strength(i, now.saturating_sub(max * 50 / 100));
            } else {
                self.set_strength(i, 0);
            }
            self.monster_enter(i, &mut m, UNDER, data, caches);
            if self.monster_strength(i).0 == 0 {
                self.note_destroyed(i, caster);
            }
        } else if m.state != UNDER {
            self.set_strength(i, now.saturating_sub(max * 50 / 100));
            m.hurried = true;
        }
        self.sea_monster_states.insert(id, m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each time it comes up to swim, a monster with room (fewer than two
    /// carried) takes the treasure on its cell (0x491a40): the treasure
    /// leaves the map with sound event 272 (`trea2`); gone, it lays what
    /// it carried back at its cell (0x491ed0) (needs `OPENCONQUEST_DATA`).
    #[test]
    fn sea_monster_takes_treasures_on_its_cell() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.unwrap();
        let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap();
        let cell = Scene::cell_at(ship);
        scene.objects.retain(|o| o.ship.is_none());
        let me = Some((human.nation, human.slot));
        scene
            .spawn_hazard(Spell::Monster, cell.0, cell.1, me, &data, &mut caches)
            .unwrap();
        let id = scene.summons[0].0;
        let i = scene.object_index(id).unwrap();
        let here = Scene::cell_at(&scene.objects[i]);
        let (x, y) = (
            (here.0 as f32 + 0.5) * TILE_SIZE as f32,
            (here.1 as f32 + 0.5) * TILE_SIZE as f32,
        );
        let treasure = |kind: u8| crate::objects::Treasure {
            kind,
            param: 0,
            amount: 700,
        };
        let lying = |s: &Scene| {
            s.objects
                .iter()
                .filter(|o| o.class == ObjectClass::Treasure && Scene::cell_at(o) == here)
                .filter_map(|o| o.treasure.map(|t| t.kind))
                .collect::<Vec<u8>>()
        };
        let mut m = scene.new_sea_monster(i);
        for (k, kind) in [0u8, 3, 1].into_iter().enumerate() {
            scene.place_treasure(treasure(kind), x, y, &data, &mut caches);
            assert_eq!(lying(&scene), [kind]);
            scene.sounds.clear();
            let i = scene.object_index(id).unwrap();
            scene.monster_enter(i, &mut m, SWIMMING, &data, &mut caches);
            let carried: Vec<u8> = scene
                .held
                .iter()
                .filter(|h| h.0 == id)
                .map(|h| h.1.kind)
                .collect();
            if k < 2 {
                assert!(lying(&scene).is_empty(), "taken");
                assert_eq!(carried.len(), k + 1);
                assert_eq!(scene.sounds.iter().filter(|s| s.0 == "trea2").count(), 1);
            } else {
                // Full: the third stays where it lies.
                assert_eq!(lying(&scene), [kind]);
                assert_eq!(carried, [0, 3]);
                assert!(!scene.sounds.iter().any(|s| s.0 == "trea2"));
            }
        }
        // Gone, it lays the two back on its cell.
        scene.objects.retain(|o| o.class != ObjectClass::Treasure);
        m.state = GONE;
        scene.sea_monster_states.insert(id, m);
        scene.run_sea_monsters(0.04, &data, &mut caches);
        let mut back = lying(&scene);
        back.sort();
        assert_eq!(back, [0, 3]);
        assert!(!scene.held.iter().any(|h| h.0 == id));
    }
}
