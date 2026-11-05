//! Ships set on fire (0x4e4fb0): the flames burn out on their own; the blow
//! that lit them did the harm.

use super::*;

/// Where the flames stand on a ship, by type and heading (0x598ad8), from
/// its picture's top left: the flames' bottom middle, 23 pixels lower
/// (0x4e04d0).
pub(super) const SPOT: [[(i32, i32); 8]; 12] = {
    const A: [(i32, i32); 8] = [
        (40, 26),
        (30, 20),
        (26, 21),
        (24, 24),
        (29, 25),
        (20, 35),
        (29, 38),
        (32, 30),
    ];
    const B: [(i32, i32); 8] = [
        (43, 33),
        (37, 20),
        (29, 21),
        (24, 24),
        (29, 29),
        (25, 40),
        (29, 39),
        (39, 39),
    ];
    const C: [(i32, i32); 8] = [
        (52, 40),
        (38, 35),
        (40, 36),
        (36, 36),
        (40, 38),
        (30, 45),
        (39, 51),
        (50, 46),
    ];
    const D: [(i32, i32); 8] = [
        (44, 33),
        (42, 25),
        (33, 28),
        (31, 30),
        (32, 32),
        (28, 38),
        (34, 41),
        (42, 39),
    ];
    [A, B, C, C, D, D, D, D, D, D, D, D]
};

/// The Torch of Ares keeps fire off its ship (shot blow 0x4bf1a0: a
/// treasure of kind 10 aboard).
const TORCH_OF_ARES: u8 = 10;

/// Flames on a ship: `FIRE<size>` played once, a frame every other tick
/// (the layer's delay +0x7d is 1, 0x4e04d0).
pub struct Fire {
    pub ship: u32,
    pub size: u8,
    /// The layer's frame `+0x45`: each step adds one and shows it modulo
    /// the count (0x55ec70), so the last step's `count` shows frame 0.
    pub frame: usize,
    /// The game tick of the lighting or of the last step (`+0x81`).
    time: u32,
    pub frames: Vec<Vec<objects::Layer>>,
}

impl Scene {
    /// A shot of `item` (`flaming` with the Flame) lands on ship `i` with
    /// `blow` (0x4bf1a0): Greek fire sets it ablaze at once, the biggest
    /// flames from their middle; a flaming shot does with a chance of
    /// blow / 27 + its lost hull percent / 2 in 100 (0x4e5120), a fire
    /// already burning growing a size.
    pub(super) fn ignite(
        &mut self,
        i: usize,
        item: u8,
        flaming: bool,
        blow: f32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let o = &self.objects[i];
        let Some(ship) = o.ship.as_ref() else { return };
        if ship.treasures.iter().any(|t| t.kind == TORCH_OF_ARES) {
            return;
        }
        let id = o.id;
        let size = if item == 9 {
            3
        } else if flaming {
            let pct = o.combat.as_ref().map_or(100, |c| {
                (c.hull.max(0.0) * 100.0 / c.max_hull.max(1.0)) as i32
            });
            let roll = (self.gull_rng.next() % 100) as i32;
            if roll > blow as i32 / 27 + (100 - pct) / 2 {
                return;
            }
            match self.fires.iter().find(|f| f.ship == id) {
                Some(f) => (f.size + 1).min(2),
                None => 0,
            }
        } else {
            return;
        };
        self.set_fire(id, size, data, caches);
        // 0x4e4fb0 ends with 0x50e570(1, cell, 5) (0x4e510f), lit anew or
        // not: sound event 5, whose `SND_5` lists `firee1` alone, mode 1
        // at the ship's cell.
        let o = &self.objects[i];
        self.sounds.push(("firee1", o.x, o.y));
    }

    /// Lights flames of `size` on ship `id` (0x4e4fb0: 3 is the biggest,
    /// from their middle); on a ship already burning they only go back to
    /// their middle if past it.
    fn set_fire(&mut self, id: u32, size: u8, data: &GameData, caches: &mut Caches) {
        if let Some(f) = self.fires.iter_mut().find(|f| f.ship == id) {
            let half = f.frames.len() / 2;
            f.frame = f.frame.max(half);
            return;
        }
        let shown = size.min(2);
        // 0x4e4fb0 sets colour 0x26 as shadow (0x50e050 with 0x5c1358,
        // 0x50e130(5, 1, 0x26, 0)) before the picture is loaded, then
        // 0x4e1bb0(5) loads `FIRE<+0x7a>` and puts 0x50e240(5, 1, 0x5c1858,
        // 1, 7) on it (0x4e1ff4-0x4e2003): the same frame fields (0x55dd40:
        // table +0x5c, value +0x60, count +0x64), so the flames' colours
        // 1-7 blend with what is beneath through 0x5c1858, as a building's
        // fire, and 0x26 is painted.
        let Ok(frames) = caches.marked_frames(data, &format!("FIRE{shown}"), None, true) else {
            return;
        };
        if frames.is_empty() {
            return;
        }
        let frame = if size == 3 { frames.len() / 2 } else { 0 };
        self.fires.push(Fire {
            ship: id,
            size: shown,
            frame,
            time: self.ticks,
            frames,
        });
    }

    /// Sets every ship ablaze (`--burn`).
    pub fn burn_all(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.is_some())
            .map(|o| o.id)
            .collect();
        for id in ids {
            self.set_fire(id, 3, data, caches);
        }
    }

    /// The flames burn on and go out, or with their ship, each game tick
    /// as the ship's tick (0x4dc630 message 0, at 0x4dca16 while `+0x8ca`
    /// is set) steps them (0x4e04d0): with the frame below the count and
    /// the tick more than the delay 1 past their time, frame + 1 and time
    /// = the tick; with the frame at the count the layer is removed
    /// (0x4e0260).
    pub(super) fn step_ship_fires(&mut self) {
        let alive: HashSet<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.is_some())
            .map(|o| o.id)
            .collect();
        self.fires
            .retain(|f| f.frame < f.frames.len() && alive.contains(&f.ship));
        let now = self.ticks;
        for f in &mut self.fires {
            if now.wrapping_sub(f.time) > 1 {
                f.frame += 1;
                f.time = now;
            }
        }
    }

    /// The flames on ship `obj` now and their top left, from its picture's.
    pub fn fire_on(
        &self,
        obj: &objects::MapObject,
        time_ms: u64,
    ) -> Option<(&[objects::Layer], f32, f32)> {
        let f = self.fires.iter().find(|f| f.ship == obj.id)?;
        let ship = obj.ship.as_ref()?;
        let layers = f.frames.get(f.frame % f.frames.len())?;
        let flame = layers.first()?;
        let first = obj.frame_at(time_ms).first()?;
        let (pl, pt) = (
            obj.x + first.dx as f32 - first.image.width as f32 / 2.0,
            obj.y + first.dy as f32 - first.image.height as f32 / 2.0,
        );
        let (sx, sy) = SPOT[(ship.kind as usize).min(11)][ship.heading % 8];
        Some((
            layers,
            pl + sx as f32 - (flame.image.width / 2) as f32,
            pt + sy as f32 - flame.image.height as f32 + 23.0,
        ))
    }
}

/// Flames as a save keeps them.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SavedFire {
    ship: u32,
    size: u8,
    frame: usize,
    /// The tick of the last step (`+0x81`).
    #[serde(default)]
    time: u32,
}

impl Scene {
    pub(super) fn saved_fires(&self) -> Vec<SavedFire> {
        self.fires
            .iter()
            .map(|f| SavedFire {
                ship: f.ship,
                size: f.size,
                frame: f.frame,
                time: f.time,
            })
            .collect()
    }

    pub(super) fn restore_fires(
        &mut self,
        fires: &[SavedFire],
        data: &GameData,
        caches: &mut Caches,
    ) {
        for f in fires {
            self.set_fire(f.ship, f.size, data, caches);
            if let Some(g) = self.fires.iter_mut().find(|g| g.ship == f.ship) {
                g.frame = f.frame;
                g.time = f.time;
            }
        }
    }
}
