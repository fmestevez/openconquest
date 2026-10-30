//! Reefs and whirlpools rising and sinking (Reef.cpp 0x4306d0, Whirl.cpp
//! 0x447ee0): states 0 rising, 1 up, 2 sinking, 3 under water, 0xff not
//! yet up (a cast medium reef's first 75 ticks). While not under they hold
//! their cell (landscape nibble 1) and wreck the ships on it.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;

use super::{Caches, GameData, Scene};
use crate::combat::Rng;
use crate::objects;

/// A reef's or whirlpool's cycle (the object's fields of 0x4306d0 /
/// 0x447ee0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Hazard {
    /// `+0x74`: 0 rising, 1 up, 2 sinking, 3 under water, 0xff not yet.
    pub state: u8,
    /// The picture's frame (`+0x45`), its step in ticks (`+0x7d`) and the
    /// tick of its last step (`+0x81`).
    pub frame: u32,
    pub step: u32,
    pub stepped: u32,
    /// The state's start (`+0x8e` / `+0x89`) and wait (`+0x92` / `+0x8d`).
    pub since: u32,
    pub wait: u32,
    /// Cast (`+0x86` / `+0x85`), the tick it was cast (`+0x8a`, reefs) and
    /// the tick it goes (`+0x96` / `+0x91`).
    pub cast: bool,
    pub cast_at: u32,
    pub ends: u32,
    /// Its own generator (`+0x18`).
    pub lcg: u32,
    /// The state whose picture the object holds (not saved: loaded again).
    #[serde(skip)]
    pub art: Option<u8>,
}

impl Hazard {
    /// The next draw of its generator (the usual LCG).
    fn draw(&mut self) -> u32 {
        let mut r = Rng(self.lcg);
        let v = r.next();
        self.lcg = r.0;
        v
    }
}

/// Which hazard an object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HazardKind {
    Whirlpool,
    BigReef,
    Reef,
}

impl HazardKind {
    pub fn of(class: ObjectClass) -> Option<Self> {
        match class {
            ObjectClass::Whirlpool => Some(Self::Whirlpool),
            ObjectClass::Reef(0x259) => Some(Self::BigReef),
            ObjectClass::Reef(_) => Some(Self::Reef),
            _ => None,
        }
    }

    /// The waits by state, min and max ticks (reefs 0x596a60, whirlpools
    /// 0x5972c0).
    fn waits(self) -> [(u32, u32); 4] {
        match self {
            Self::Whirlpool => [(0, 0), (125, 250), (0, 0), (150, 250)],
            _ => [(0, 0), (1, 2), (0, 0), (150, 250)],
        }
    }

    /// Nibble 1 of the landscape while it is up (0x430320, 0x447b80): 5 a
    /// big reef, 6 a medium one, 4 a whirlpool.
    fn mark(self) -> u32 {
        match self {
            Self::Whirlpool => 4,
            Self::BigReef => 5,
            Self::Reef => 6,
        }
    }
}

/// Picture lengths by state (0 rising, 1 up, 2 sinking), in ticks.
const REEF_CYCLE: [u32; 3] = [30, 60, 30]; // 0x596a50
const WHIRL_CYCLE: [u32; 3] = [60, 60, 60]; // 0x5972b0

/// Sound events (0x50e570 mode 5 at its cell): rising from under the
/// water, and up.
const WHIRL_RISES: &str = "whirl3"; // 149
const REEF_RISES: &str = "riff1wav"; // 151
const UP: &str = "spider"; // 150, 152

impl Scene {
    /// The hazards' tick (message 0 of 0x4306d0 / 0x447ee0), with their
    /// creation (message 2) for those not yet seen.
    pub(super) fn run_hazard_cycles(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| HazardKind::of(o.class).is_some())
            .map(|o| o.id)
            .collect();
        self.hazards.retain(|id, _| ids.contains(id));
        for id in ids {
            if !self.hazards.contains_key(&id) {
                self.create_hazard(id, data, caches);
            }
            self.hazard_tick(id, data, caches);
        }
        self.jason_lying_still(data, caches);
    }

    /// Message 2: the record's field 4 is its mode (0 the map's or a
    /// brain's, 1 cast). Its generator is seeded `game +0x2ee5 x id`.
    pub(super) fn create_hazard(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some(k) = self.object_index(id) else {
            return;
        };
        let kind = HazardKind::of(self.objects[k].class).unwrap();
        let cast = self.objects[k].record.field(4) == Some(1);
        let seed = self.game_seed().wrapping_mul(id);
        let now = self.ticks;
        let mut h = Hazard {
            state: 0xff,
            frame: 0,
            step: 0,
            stepped: now,
            since: now,
            wait: 0,
            cast,
            cast_at: 0,
            ends: 0,
            lcg: seed,
            art: None,
        };
        if cast {
            // 7500-15000 ticks (0x4306d0, 0x447ee0: `r % 7501 + 7500`).
            h.ends = h.draw() % 7501 + 7500 + now;
            h.cast_at = now;
        }
        self.hazards.insert(id, h);
        match (kind, cast) {
            (_, false) => self.set_hazard(id, 1, false, data, caches),
            // A cast medium reef waits 75 ticks unseen; a big one rises at
            // once; a whirlpool starts under the water (150-250 ticks).
            (HazardKind::Reef, true) => {}
            (HazardKind::BigReef, true) => self.set_hazard(id, 0, true, data, caches),
            (HazardKind::Whirlpool, true) => self.set_hazard(id, 3, true, data, caches),
        }
    }

    /// The game's seed of the objects' generators (`+0x2ee5`; see
    /// `Scene::ship_draw`).
    fn game_seed(&mut self) -> u32 {
        match self.rnd_init {
            Some(v) => v,
            None => {
                let v = self.rng.next() << 16 | self.rng.next();
                self.rnd_init = Some(v);
                v
            }
        }
    }

    /// Hazard `id`'s kind and cell.
    fn hazard_at(&self, id: u32) -> Option<(HazardKind, (i32, i32))> {
        let o = &self.objects[self.object_index(id)?];
        let f = |i| o.record.field(i).unwrap_or(0) as i32;
        Some((HazardKind::of(o.class)?, (f(5), f(6))))
    }

    /// The ship in cell `c`'s occupant slot (`grid + 0xc`): one lying there,
    /// or one stepping into it, whose reserved next cell (`+0x5ab`) it is;
    /// and whether it is reserved, and whether Jason is aboard (0x4ba790).
    pub(super) fn hazard_occupant(&self, c: (i32, i32)) -> Option<(u32, bool, bool)> {
        let t = TILE_SIZE as f32;
        let cell = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
        self.objects.iter().find_map(|o| {
            let s = o.ship.as_ref()?;
            let next = s.path.front().map(|&(x, y)| cell(x, y));
            let reserved = next == Some(c);
            (cell(o.x, o.y) == c || reserved).then(|| (o.id, reserved, s.has_hero(objects::JASON)))
        })
    }

    /// 0x430320 / 0x447b80: hazard `id` to state `s`; `loud` the second
    /// argument (the rise is heard though it was not under water).
    pub(super) fn set_hazard(
        &mut self,
        id: u32,
        s: u8,
        loud: bool,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some((kind, cell)) = self.hazard_at(id) else {
            return;
        };
        let now = self.ticks;
        let waits = kind.waits();
        let draw = |h: &mut Hazard, s: u8| {
            let (lo, hi) = waits[s as usize];
            h.draw() % (hi - lo + 1) + lo
        };
        let t = TILE_SIZE as f32;
        let (px, py) = (cell.0 as f32 * t + t / 2.0, cell.1 as f32 * t + t / 2.0);
        let Some(mut h) = self.hazards.get(&id).copied() else {
            return;
        };
        match s {
            3 => {
                // Under: the landscape back to its own (sandbank 3 under
                // a shoal tile set of layer 1, 2-6; else 0), the picture
                // hidden (0xd21), a wait of 150-250 ticks.
                self.unmark_hazard(cell);
                h.since = now;
                h.state = 3;
                h.wait = draw(&mut h, 3);
                self.hazards.insert(id, h);
                return;
            }
            2 => {
                h.state = 2;
                self.hazards.insert(id, h);
                self.show_hazard(id, data, caches);
                return;
            }
            1 => {
                h.since = now;
                h.wait = draw(&mut h, 1);
                if h.state == 0 {
                    self.sounds.push((UP, px, py));
                    h.state = 1;
                    self.hazards.insert(id, h);
                    self.show_hazard(id, data, caches);
                    return;
                }
            }
            _ => {}
        }
        // Rising: a ship with Jason in the cell keeps it under (state 3,
        // the wait of the state it was to take).
        if self.hazard_occupant(cell).is_some_and(|o| o.2) {
            h.state = 3;
            h.since = now;
            h.wait = draw(&mut h, s);
            self.hazards.insert(id, h);
            return;
        }
        if h.state == 3 || loud {
            let rises = if kind == HazardKind::Whirlpool {
                WHIRL_RISES
            } else {
                REEF_RISES
            };
            self.sounds.push((rises, px, py));
        }
        self.mark_hazard(cell, kind.mark());
        h.state = s;
        self.hazards.insert(id, h);
        self.show_hazard(id, data, caches);
    }

    /// Nibble 1 of cell `c`'s landscape set to `mark`.
    fn mark_hazard(&mut self, (x, y): (i32, i32), mark: u32) {
        let w = self.grid.width as i32;
        if let Some(l) = self.cell_types.get_mut((y * w + x) as usize) {
            *l = *l & !0xf0 | mark << 4;
        }
    }

    /// Nibble 1 of cell `c`'s landscape back to its tile's: 3 when layer
    /// 1's tile set is 2-6 (a shoal), else 0 (0x430320 / 0x447b80 case 3,
    /// message 3).
    pub(super) fn unmark_hazard(&mut self, (x, y): (i32, i32)) {
        let (w, h) = (self.grid.width as i32, self.grid.height as i32);
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let set = self.grid.get(1, x as u32, y as u32).tileset;
        let own = if (2..=6).contains(&set) { 3 } else { 0 };
        self.mark_hazard((x, y), own);
    }

    /// The landscape marks of the hazards up, after a load (the save
    /// keeps their states, the landscape is made again from the map).
    pub(super) fn mark_hazards(&mut self) {
        let up: Vec<(u32, (i32, i32), HazardKind)> = self
            .hazards
            .iter()
            .filter(|(_, h)| h.state <= 2)
            .filter_map(|(&id, _)| {
                let (kind, cell) = self.hazard_at(id)?;
                Some((id, cell, kind))
            })
            .collect();
        for (_, cell, kind) in up {
            self.mark_hazard(cell, kind.mark());
        }
    }

    /// 0x4300a0 / 0x447930: the state's picture (`BREEF<kind>SPR_<state>`,
    /// `MREEF..`, `WHIRLSPR_<state>`), from its first frame, a step of the
    /// state's length over its frames.
    fn show_hazard(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some(mut h) = self.hazards.get(&id).copied() else {
            return;
        };
        if h.state > 2 {
            return;
        }
        let n = self.hazard_art(id, h.state, data, caches);
        let (kind, _) = self.hazard_at(id).unwrap();
        let cycle = if kind == HazardKind::Whirlpool {
            WHIRL_CYCLE
        } else {
            REEF_CYCLE
        }[h.state as usize];
        h.frame = 0;
        h.step = cycle / n.max(1);
        h.stepped = self.ticks;
        h.art = Some(h.state);
        self.hazards.insert(id, h);
    }

    /// Loads hazard `id`'s picture of state `state` into its object; the
    /// frames it has.
    fn hazard_art(&mut self, id: u32, state: u8, data: &GameData, caches: &mut Caches) -> u32 {
        let Some(k) = self.object_index(id) else {
            return 0;
        };
        let o = &self.objects[k];
        let name = match o.class {
            ObjectClass::Whirlpool => format!("WHIRLSPR_{state}"),
            ObjectClass::Reef(cls) => {
                let size = if cls == 0x259 { 'B' } else { 'M' };
                let kind = o.record.field(7).unwrap_or(1).clamp(1, 3);
                format!("{size}REEF{kind}SPR_{state}")
            }
            _ => return 0,
        };
        match objects::sequence_frames(data, caches, &name) {
            Ok(frames) if !frames.is_empty() => {
                let n = frames.len() as u32;
                self.objects[k].frames = frames;
                n
            }
            _ => self.objects[k].frames.len() as u32,
        }
    }

    /// 0x4301f0 / 0x447a50: the picture steps a frame when its step is
    /// due; at its end the rising goes up and the sinking under.
    fn step_hazard(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some(h) = self.hazards.get(&id).copied() else {
            return;
        };
        if h.state == 3 {
            return;
        }
        let now = self.ticks;
        if now < h.stepped + h.step {
            return;
        }
        let n = self
            .object_index(id)
            .map_or(0, |k| self.objects[k].frames.len() as u32);
        if n >= 2 {
            let frame = h.frame + 1;
            self.hazards.get_mut(&id).unwrap().frame = frame;
            if frame >= n {
                match h.state {
                    0 => self.set_hazard(id, 1, false, data, caches),
                    2 => self.set_hazard(id, 3, false, data, caches),
                    _ => self.hazards.get_mut(&id).unwrap().frame = 0,
                }
            }
        }
        if let Some(h) = self.hazards.get_mut(&id) {
            h.stepped = now;
        }
    }

    /// Message 0 of 0x4306d0 / 0x447ee0 (the wrecks are `run_hazards`').
    fn hazard_tick(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some((kind, cell)) = self.hazard_at(id) else {
            return;
        };
        let Some(h) = self.hazards.get(&id).copied() else {
            return;
        };
        // A picture lost to a load is loaded again where it was.
        if h.state <= 2 && h.art != Some(h.state) {
            self.hazard_art(id, h.state, data, caches);
            if let Some(h) = self.hazards.get_mut(&id) {
                h.art = Some(h.state);
            }
        }
        let now = self.ticks;
        // Jason aboard a ship lying in the cell (not stepping into it).
        let jason_holds = self
            .hazard_occupant(cell)
            .is_some_and(|(_, reserved, jason)| !reserved && jason);
        if kind == HazardKind::Reef
            && h.cast
            && h.state == 0xff
            && now >= h.cast_at + 75
            && !jason_holds
        {
            self.set_hazard(id, 0, true, data, caches);
        }
        let h = self.hazards[&id];
        let due = now >= h.since + h.wait;
        let gone = h.cast && now >= h.ends;
        match (kind, h.state) {
            // A big reef never sinks; cast, it goes at its end (message
            // 10), whatever its state.
            (HazardKind::BigReef, _) if gone => {
                self.remove_hazard(id);
                return;
            }
            (HazardKind::Reef | HazardKind::Whirlpool, 1) => {
                let last = self
                    .object_index(id)
                    .map_or(0, |k| self.objects[k].frames.len() as u32)
                    .saturating_sub(1);
                if due && h.frame == last {
                    self.set_hazard(id, 2, false, data, caches);
                }
            }
            // Under, a cast one goes at its end (message 10); else, its
            // wait out, it rises: a whirlpool waits on while a ship with
            // Jason lies there; a reef's 0x430320 finds him and draws
            // another wait.
            (HazardKind::Reef | HazardKind::Whirlpool, 3) => {
                if gone {
                    self.remove_hazard(id);
                    return;
                }
                if due && !(kind == HazardKind::Whirlpool && jason_holds) {
                    self.set_hazard(id, 0, false, data, caches);
                }
            }
            _ => {}
        }
        self.step_hazard(id, data, caches);
    }

    /// Message 10 then 3: the hazard goes, its cell's landscape back to
    /// its own.
    pub(super) fn remove_hazard(&mut self, id: u32) {
        if let Some((_, cell)) = self.hazard_at(id) {
            if self.hazards.get(&id).is_none_or(|h| h.state <= 2) {
                self.unmark_hazard(cell);
            }
        }
        self.hazards.remove(&id);
        self.objects.retain(|o| o.id != id);
        self.summons.retain(|s| s.0 != id);
    }

    /// 0xda9e / 0xda9f (0x4a7630): a medium reef or whirlpool on cell `c`
    /// sinks, unless sinking or under already.
    pub(super) fn sink_hazard_at(&mut self, c: (i32, i32), data: &GameData, caches: &mut Caches) {
        let found = self.objects.iter().find_map(|o| {
            let f = |i| o.record.field(i).unwrap_or(0) as i32;
            matches!(o.class, ObjectClass::Whirlpool | ObjectClass::Reef(0x25a))
                .then_some((o.id, (f(5), f(6))))
                .filter(|&(_, at)| at == c)
                .map(|(id, _)| id)
        });
        let Some(id) = found else {
            return;
        };
        if self
            .hazards
            .get(&id)
            .is_some_and(|h| h.state != 2 && h.state != 3)
        {
            self.set_hazard(id, 2, false, data, caches);
        }
    }

    /// A ship with Jason lying still (0x4a3150, move state 0) on a cell
    /// holding a whirlpool or medium reef calls 0x4a7630 with the cell's x
    /// for both arguments (`push ecx; push ecx`, 0x4a330c): the one at (x,
    /// x) sinks, if there is one.
    fn jason_lying_still(&mut self, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let sinkable: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Whirlpool | ObjectClass::Reef(0x25a)))
            .map(|o| {
                let f = |i| o.record.field(i).unwrap_or(0) as i32;
                (f(5), f(6))
            })
            .collect();
        if sinkable.is_empty() {
            return;
        }
        let calls: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship
                    .as_ref()
                    .is_some_and(|s| s.move_state == 0 && s.has_hero(objects::JASON))
            })
            .map(|o| ((o.x / t).floor() as i32, (o.y / t).floor() as i32))
            .filter(|c| sinkable.contains(c))
            .map(|(x, _)| (x, x))
            .collect();
        for c in calls {
            self.sink_hazard_at(c, data, caches);
        }
    }

    /// Whether hazard `id` is under the water or not yet up: no picture.
    pub fn hazard_hidden(&self, id: u32) -> bool {
        self.hazards.get(&id).is_some_and(|h| h.state > 2)
    }

    /// Whether hazard `id` wrecks the ships on its cell (any state but 3).
    pub(super) fn hazard_wrecks(&self, id: u32) -> bool {
        self.hazards.get(&id).is_none_or(|h| h.state != 3)
    }

    /// The cells hazards hold (nibble 1 4-6: up, rising or sinking), each
    /// with whether a ship with Jason sinks it (a whirlpool or medium reef).
    pub(super) fn hazard_cells(&self) -> std::collections::HashMap<(i32, i32), bool> {
        self.objects
            .iter()
            .filter_map(|o| {
                let kind = HazardKind::of(o.class)?;
                let up = self.hazards.get(&o.id).is_none_or(|h| h.state <= 2);
                up.then(|| {
                    let f = |i| o.record.field(i).unwrap_or(0) as i32;
                    ((f(5), f(6)), kind != HazardKind::BigReef)
                })
            })
            .collect()
    }

    /// The frame hazard `id` shows.
    pub(super) fn hazard_frame(&self, id: u32) -> Option<u32> {
        self.hazards.get(&id).map(|h| h.frame)
    }
}
