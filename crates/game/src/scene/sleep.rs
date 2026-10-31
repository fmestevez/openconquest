//! Buildings asleep (record field 4 = 3, state 0x10, 0x441ba0 and
//! 0x427f50): first `r % 15` ticks on (`r` its own generator's first
//! draw, 0x4281de / 0x4422ad), then every 15 ticks each looks about its footprint, one cell
//! round (0x425590), and wakes when a ship of its owner's stands there or
//! an awake building of its owner's (0x32-0x3e) does; waking (0x425720)
//! wakes the sleepers of its owner's about it the same way, and tells the
//! owner (0x5d9a).

use super::*;

impl Scene {
    pub(super) fn run_wakeups(&mut self, dt: f32) {
        // A sleeper's first look is its generator's first draw.
        for k in 0..self.objects.len() {
            let o = &self.objects[k];
            if o.asleep.is_some() && !self.building_lcgs.contains_key(&o.id) {
                self.building_wake_drawn(k);
            }
        }
        let mut woken = Vec::new();
        for o in self.objects.iter_mut() {
            if let Some(left) = o.asleep.as_mut() {
                *left -= dt;
                if *left <= 0.0 {
                    *left += crate::clock::ticks(15);
                    woken.push(o.id);
                }
            }
        }
        for id in woken {
            if self.called(id) {
                self.wake(id);
            }
        }
    }

    /// The cells one round building `i`'s footprint.
    fn ring(&self, i: usize) -> Option<(i32, i32, i32, i32)> {
        let o = &self.objects[i];
        let (x, y) = o.cell?;
        let n = super::trade::building_cells(o);
        Some((x - 1, y - 1, x + n, y + n))
    }

    /// Whether sleeper `id` has its owner's ship or awake building about.
    fn called(&self, id: u32) -> bool {
        let Some(i) = self.object_index(id) else {
            return false;
        };
        let Some((x0, y0, x1, y1)) = self.ring(i) else {
            return false;
        };
        let owner = self.objects[i].owner;
        let t = TILE_SIZE as f32;
        let inside = |x: i32, y: i32| (x0..=x1).contains(&x) && (y0..=y1).contains(&y);
        self.objects.iter().any(|o| {
            o.owner == owner
                && o.asleep.is_none()
                && match (&o.ship, o.class, o.cell) {
                    (Some(_), _, _) => inside((o.x / t).floor() as i32, (o.y / t).floor() as i32),
                    (None, ObjectClass::Building(0x32..=0x3e), Some((cx, cy))) => {
                        let n = super::trade::building_cells(o);
                        (cx..cx + n).any(|x| (cy..cy + n).any(|y| inside(x, y)))
                    }
                    _ => false,
                }
        })
    }

    /// Wakes sleeper `id` and its owner's sleepers about it (0x425720).
    pub(super) fn wake(&mut self, id: u32) {
        let mut queue = vec![id];
        while let Some(id) = queue.pop() {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            if self.objects[i].asleep.take().is_none() {
                continue;
            }
            // Rebuilt from its record (an upgrade, a carry), it stays awake.
            if let Some(f) = self.objects[i].record.fields.get_mut(4) {
                *f = 0;
            }
            let owner = self.objects[i].owner;
            if owner.is_some() && owner == self.human.map(|h| (h.nation, h.slot)) {
                self.on_woken();
            }
            let Some((x0, y0, x1, y1)) = self.ring(i) else {
                continue;
            };
            for o in &self.objects {
                if o.asleep.is_some() && o.owner == owner {
                    if let Some((cx, cy)) = o.cell {
                        let n = super::trade::building_cells(o);
                        if (cx..cx + n).any(|x| (x0..=x1).contains(&x))
                            && (cy..cy + n).any(|y| (y0..=y1).contains(&y))
                        {
                            queue.push(o.id);
                        }
                    }
                }
            }
        }
        self.update_sight();
    }

    /// The player's brain told of a waking (0x5d9a): the judges of miss001
    /// and miss004 note it; miss003's first gives the player 2000 amber and
    /// 500 fish and starts its raiders over (members 2 and 9 to state 0).
    fn on_woken(&mut self) {
        let first = !self.woken;
        self.woken = true;
        if first && self.mission == "miss003" && self.brain.is_some() {
            if let Some(h) = self.human {
                self.earn((h.nation, h.slot), 2000, 500);
            }
            let brain = self.brain.as_mut().unwrap();
            for k in [2, 9] {
                if let Some(f) = brain.fleets.get_mut(k) {
                    f.state = 0;
                }
            }
        }
    }
}
