//! Order 10: what each object calls off (a right click on a command button
//! whose code sends it, 0x4f46c0; Players.cpp 0x5228c0 delivers it as
//! message 0x280f, order byte 10, with no data).

use openconquest_formats::ObjectClass;

use super::Scene;
use crate::assets::GameData;
use crate::combat;
use crate::economy::{self, DOCK, SCHOOL, SETTLEMENT, SHIPYARD, SMITHY, TOWN, WORKSHOP};
use crate::magic::{Temple, LEARN_SECONDS};
use crate::objects::{self, BuildingState, Caches};

/// A smithy's refits as the one batch the original runs (0x434a00,
/// message 0x138c): the amber paid for all of them (`+0x118`), the ticks
/// they take together (`+0x114`) and the seconds since the batch began
/// (`+0x124`).
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Batch {
    pub paid: u32,
    pub ticks: u32,
    pub elapsed: f32,
}

impl Batch {
    /// How far along it is in whole percent (`+0x120 = (now - +0x124) x
    /// 100 / +0x114`).
    pub fn progress(&self) -> u32 {
        percent(self.elapsed, self.ticks)
    }
}

/// `(now - start) x 100 / ticks` in whole percent (unsigned), from the
/// seconds spent, as the buildings' ticks write `+0x120`; at most 100.
fn percent(elapsed: f32, ticks: u32) -> u32 {
    let spent = (elapsed / crate::clock::TICK + 1e-3) as u32;
    (spent * 100).checked_div(ticks).unwrap_or(100).min(100)
}

/// The sounds of a building's voice event by its nation 0-3 (events
/// base, base + 1, base + 3, base + 2; the `SND_n` lists, dumped; an
/// event with no entry plays nothing).
pub(super) type Calls = [&'static [&'static str]; 4];
/// The settlement's or town's: events 220-223.
pub(super) const TOWN_CALLS: Calls = [
    &[],
    &["re_o2a", "re_o4a", "re_o6a", "re_o5a"],
    &[],
    &["re_o2a", "re_o4a", "re_o5a", "re_o6a"],
];
const REPORT: &[&str] = &["re_o2a", "re_o4a", "re_o5a", "re_o6a"];
/// The smithy's (232-235), the workshop's (236-239) and the Temple of
/// War's (248-251).
pub(super) const SMITHY_CALLS: Calls = [&[], REPORT, &[], REPORT];
pub(super) const STUDIO_CALLS: Calls = [&[], REPORT, &[], REPORT];
pub(super) const WAR_TEMPLE_CALLS: Calls = [&[], REPORT, &[], REPORT];
/// The sage school's (240-243).
pub(super) const SCHOOL_CALLS: Calls = [
    &[],
    &["re_o2a", "re_o4a", "re_o6a"],
    &[],
    &["re_o2a", "re_o4a", "re_o6a"],
];
/// A building finished (188-191, 0x4230f0).
pub(super) const FINISHED_CALLS: Calls = [
    &[],
    &["bi_c1a", "bi_c2a", "bi_c4a", "bi_c5a"],
    &[],
    &["bi_c1a", "bi_c2a", "bi_c4a", "bi_c5a"],
];

/// What LandObj 0x425990's order-10 branches did.
#[derive(PartialEq)]
enum Land {
    /// Nothing: the building's own job, if any, is still on.
    Nothing,
    /// Its own mending stopped (job 0xff): the building is idle now.
    Mending,
    /// It was being built: paid back and removed.
    Removed,
}

impl Scene {
    /// Order 10 to object `id`. The handlers run in the original's chain
    /// (each layer reads what the one before changed):
    ///
    /// - a ship: the stop event (0x4e2ba0 case 10: event 8 with no record,
    ///   0x4aa540; a ship being built is called off);
    /// - a settlement or town (TownObj 0x441ba0): its own case first
    ///   (the upgrade, or its busy job), then LandObj (0x444f17);
    /// - a shipyard or dock (Dock 0x416f90): the upgrade's cancel, which
    ///   ends it; else 0x4e8660 -> 0x4e7640 -> 0x427f50 -> LandObj;
    /// - every other building, forts (0x419f80 calls 0x4e7640 at
    ///   0x41a176) and barbarian structures (0x40ef70 -> 0x4e8660)
    ///   included: LandObj first (0x427f50 -> 0x425990), then its own
    ///   case.
    ///
    /// LandObj's two branches (`land_halt`): its own mending stopped, and a
    /// building under construction paid back and removed.
    pub fn halt(&mut self, id: u32, data: &GameData, caches: &mut Caches) -> Result<(), String> {
        let i = self.object_index(id).ok_or("No such object")?;
        let class = self.objects[i].class;
        let (x, y, owner) = (self.objects[i].x, self.objects[i].y, self.objects[i].owner);
        match class {
            ObjectClass::Ship => self.stop(id),
            ObjectClass::Building(SETTLEMENT | TOWN) => {
                self.town_halt(id, data, caches);
                self.land_halt(id);
            }
            ObjectClass::Building(SHIPYARD | DOCK) => {
                if !self.dock_upgrade_halt(id, data, caches) {
                    self.land_halt(id);
                }
            }
            ObjectClass::Building(c) => {
                let land = self.land_halt(id);
                let busy = land == Land::Nothing;
                match c {
                    SCHOOL => {
                        // Standing and busy: the study called off, and
                        // only then its call (0x432210).
                        if busy && self.research_halt(id) {
                            self.building_voice(owner, &SCHOOL_CALLS, x, y);
                        }
                    }
                    SMITHY => {
                        if busy && !self.research_halt(id) {
                            self.refits_halt(id);
                        }
                        // Its call on every order but 0x3c (0x434ed0),
                        // even one that found it under construction.
                        self.building_voice(owner, &SMITHY_CALLS, x, y);
                    }
                    WORKSHOP => {
                        if busy && !self.research_halt(id) && !self.training_halt(id) {
                            // Busy with its settlement's stock repair (job
                            // 5): the order goes on to the settlement
                            // (`+0x1c8`).
                            if let Some(home) = self
                                .home_settlement(i)
                                .filter(|&h| self.stock_repair_at(h).is_some())
                            {
                                self.halt(home, data, caches)?;
                            }
                        }
                        self.building_voice(owner, &STUDIO_CALLS, x, y);
                    }
                    economy::TEMPLE_OF_THE_SEA => {
                        if busy {
                            self.temple_halt(id, Temple::Sea);
                        }
                    }
                    economy::TEMPLE_OF_WAR => {
                        if busy && self.temple_halt(id, Temple::War) {
                            self.building_voice(owner, &WAR_TEMPLE_CALLS, x, y);
                        }
                    }
                    0x34..=0x36 => self.cease_fire(id),
                    _ => {}
                }
            }
            ObjectClass::Barbarian(_) => {
                self.land_halt(id);
                self.cease_fire(id);
            }
            _ => {}
        }
        Ok(())
    }

    /// LandObj 0x425990's order-10 branches: its own mending (job 0xff)
    /// stopped, no refund (it is paid step by step); and, under
    /// construction (`+0xa4` 0; `+0xe8` is the scaffold picture's name,
    /// set for every player building and town), the whole price
    /// 0x42d8b0(class) back in amber only (0x4a8dc0; what a fort's arming
    /// cost is not) and the building removed at once (0x424740: `+0x158`
    /// cleared first, so no fall sound and no rubble; the drain 0x423d60
    /// reports it with killer 0xff, its owner's building lost, nobody's
    /// kill, and removes it, no ruins). (Its settlement's lists, message
    /// 0x2874, and a fort's count of forts going up, 0x2491, have no
    /// counterpart here: the port finds a building's settlement by its
    /// place.)
    fn land_halt(&mut self, id: u32) -> Land {
        if self.mending.contains(&id) {
            self.mending.retain(|&m| m != id);
            return Land::Mending;
        }
        if !self.construction_at(id).is_some_and(|c| !c.upgrade) {
            return Land::Nothing;
        }
        let Some(i) = self.object_index(id) else {
            return Land::Nothing;
        };
        let class = self.objects[i].record.class_id;
        if let Some(owner) = self.objects[i].owner {
            self.earn(owner, economy::building_cost(class) as i64, 0);
        }
        self.note_destroyed(i, None);
        self.constructions.retain(|c| c.site != id);
        self.objects.remove(i);
        Land::Removed
    }

    /// TownObj 0x441ba0 case 10: an upgrade to a town (`+0xa4` 0 with
    /// `+0x23c`) is called off: `0x42d8b0(0x33) x (n - f) / n` back (`n`
    /// the construction picture's frames, `f` the one shown; integers),
    /// a settlement again, standing (the mending share of its price is not
    /// paid back); else, busy, its job: the stock repair's cancel
    /// (0x4430f0) or the training's, and for any busy job the training
    /// list freed. Either way its call (events 220-223) for its owner.
    fn town_halt(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let (x, y, owner) = (self.objects[i].x, self.objects[i].y, self.objects[i].owner);
        if self.upgrade_halt(id, TOWN, SETTLEMENT, data, caches) {
            self.building_voice(owner, &TOWN_CALLS, x, y);
            return;
        }
        let mut busy = false;
        if self.stock_repair_at(id).is_some() {
            busy = true;
            let _ = self.cancel_stock_repair(id);
        } else if self.training_halt(id) {
            busy = true;
        } else if self.mending.contains(&id) {
            // Its own mending (job 0xff): stopped with no refund here,
            // which leaves LandObj nothing to do.
            busy = true;
            self.mending.retain(|&m| m != id);
        }
        if busy {
            // Any busy job frees the training list (`+0x258 = +0x254 = 0`):
            // what was not made is lost beyond the refund.
            self.trainings.retain(|t| t.site != id);
            self.building_voice(owner, &TOWN_CALLS, x, y);
        }
    }

    /// Dock 0x416f90 case 10: an upgrade to a dock (`+0x1e8`) is called
    /// off, `0x42d8b0(0x3b) x (n - f) / n` back, a shipyard again, and the
    /// order ends there (no call, nothing further).
    fn dock_upgrade_halt(&mut self, id: u32, data: &GameData, caches: &mut Caches) -> bool {
        self.upgrade_halt(id, DOCK, SHIPYARD, data, caches)
    }

    /// An upgrade of `id` to class `to` called off: its price `x (n - f) /
    /// n` back, class `from` again (vt+0x54), standing (`+0xa4` 1, its
    /// pictures by vt+0x38), keeping the condition the construction had
    /// reached (`+0xa8` is not written). (Not decoded: whether vt+0x54
    /// resets the hit points; the frame an upgrade starts from, 0x422e30
    /// resetting it only when `>= n`: the port starts it at 0.)
    fn upgrade_halt(
        &mut self,
        id: u32,
        to: u32,
        from: u32,
        data: &GameData,
        caches: &mut Caches,
    ) -> bool {
        let Some(c) = self.construction_at(id).filter(|c| c.upgrade).cloned() else {
            return false;
        };
        let Some(i) = self.object_index(id) else {
            return false;
        };
        if self.objects[i].class != ObjectClass::Building(to) {
            return false;
        }
        let n = c.frames.max(1);
        let back = economy::building_cost(to) * (n - c.frame.min(n)) / n;
        if let Some(owner) = self.objects[i].owner {
            self.earn(owner, back as i64, 0);
        }
        self.constructions.retain(|c| c.site != id);
        let condition = c.frame.min(n) * 100 / n;
        let mut rec = self.objects[i].record.clone();
        rec.class_id = from;
        rec.fields[0] = from;
        if let Ok(Some(mut obj)) = objects::resolve(data, caches, &rec) {
            obj.owner = self.objects[i].owner;
            if let Some(h) = obj.combat.as_mut() {
                let max = (h.max_hull * 15.0).round() as u64;
                h.hull = (max * condition as u64 / 100) as f32 / 15.0;
            }
            self.objects[i] = obj;
        }
        self.objects[i].progress = None;
        if let Ok(frames) = objects::building_frames(
            data,
            caches,
            &self.objects[i].record,
            BuildingState::for_condition(condition as f32),
        ) {
            if !frames.is_empty() {
                self.objects[i].frame_ms = objects::building_frame_ms(frames.len());
                self.objects[i].frames = frames;
            }
        }
        true
    }

    /// A study at `site` called off (School 0x432210, Smithy 0x434ed0,
    /// Studio 0x436ee0 case 10, standing and busy): `(100 - p) x price /
    /// 100` back in amber, `p` the progress the last tick wrote (`+0x120`),
    /// the price recomputed from the same level (what was paid: the level
    /// does not change during the study), and the job's player flag
    /// cleared (the entry gone).
    fn research_halt(&mut self, site: u32) -> bool {
        let Some(k) = self.research.iter().position(|r| r.0 == site) else {
            return false;
        };
        let (_, upgrade, level, elapsed) = self.research.remove(k);
        let from = level.saturating_sub(1);
        let ticks = (upgrade.seconds(from) / crate::clock::TICK).round() as u32;
        let p = percent(elapsed, ticks);
        let back = (100 - p) * upgrade.cost(from) / 100;
        if let Some(owner) = self.object_index(site).and_then(|i| self.objects[i].owner) {
            self.earn(owner, back as i64, 0);
        }
        true
    }

    /// The smithy's refit batch called off (job 1): `(100 - p) x +0x118 /
    /// 100` back, the rest of the batch dropped (`+0x1ec` freed).
    fn refits_halt(&mut self, smithy: u32) -> bool {
        if !self.refits.iter().any(|r| r.0 == smithy) {
            return false;
        }
        self.refits.retain(|r| r.0 != smithy);
        let batch = self.refit_batches.remove(&smithy).unwrap_or_default();
        let back = (100 - batch.progress()) * batch.paid / 100;
        if let Some(owner) = self
            .object_index(smithy)
            .and_then(|i| self.objects[i].owner)
        {
            self.earn(owner, back as i64, 0);
        }
        true
    }

    /// The training at `site` called off (TownObj job 1, Studio job 1): one
    /// batch, its progress `p` over the summed time of everything ordered
    /// (`+0x114`); `(100 - p) x +0x118 / 100` amber back. A settlement's
    /// fish share, `(100 - p) x +0x11c / 100` (raw: 0x597170 per unit),
    /// goes not to the player but into its bytes `+0x252` / `+0x253`
    /// (0x44316a); the workshop gives no fish back. The list is freed: the
    /// units not made are lost.
    fn training_halt(&mut self, site: u32) -> bool {
        let Some(k) = self.trainings.iter().position(|t| t.site == site) else {
            return false;
        };
        let t = self.trainings.remove(k);
        let ticks: u32 = t
            .queue
            .iter()
            .map(|&item| (combat::train_seconds(item) / crate::clock::TICK).round() as u32)
            .sum();
        let amber: u32 = t.queue.iter().map(|&item| combat::item_cost(item).0).sum();
        let fish: u32 = t
            .queue
            .iter()
            .map(|&item| combat::item_cost(item).1 / 100)
            .sum();
        let p = percent(t.elapsed, ticks);
        let Some(i) = self.object_index(site) else {
            return true;
        };
        if let Some(owner) = self.objects[i].owner {
            self.earn(owner, ((100 - p) * amber / 100) as i64, 0);
        }
        if matches!(
            self.objects[i].class,
            ObjectClass::Building(SETTLEMENT | TOWN)
        ) {
            let v = ((100 - p) * fish / 100) as i32;
            let (count, last) = self.town_fish.get(&site).copied().unwrap_or((0, 100));
            // With some there already, `v + last + 100 x count - 100`.
            let total = if count != 0 {
                v + last as i32 + 100 * count as i32 - 100
            } else {
                v
            };
            // `+0x252 = total / 100`, `+0x253 = total % 100`; a last part
            // of 0 is 100, any other counts one more.
            let (q, r) = ((total / 100) as u8, (total % 100) as u8);
            let share = if r == 0 {
                (q, 100)
            } else {
                (q.wrapping_add(1), r)
            };
            self.town_fish.insert(site, share);
        }
        true
    }

    /// A temple's job called off (TempleSea 0x438300 / TempleWar 0x439260
    /// case 10, busy): learning `p x 1000 / 100` back (`p` the elapsed
    /// percent, `+0x120`: what was spent, not what was left, and at the
    /// list price), the Flame `p x 6000 / 100` amber ([0x596864]) and `p x
    /// 15` fish ([0x59689c], no / 100). Whether it was busy.
    fn temple_halt(&mut self, site: u32, temple: Temple) -> bool {
        let Some(i) = self.object_index(site) else {
            return false;
        };
        let Some(owner) = self.objects[i]
            .owner
            .filter(|_| self.is_own(&self.objects[i]))
        else {
            return false;
        };
        let here = |s: Option<u32>| s.is_none_or(|s| s == site);
        if temple == Temple::War && here(self.magic.flame_site()) {
            if let Some(t) = self.magic.cancel_flame() {
                let p = percent(t, 250);
                self.earn(owner, (p * 6000 / 100) as i64, (p * 15) as i64);
                return true;
            }
        }
        if here(self.magic.learning_site(temple)) {
            if let Some(t) = self.magic.cancel_learning(temple) {
                let ticks = (LEARN_SECONDS / crate::clock::TICK).round() as u32;
                let p = percent(t, ticks);
                self.earn(owner, (p * 1000 / 100) as i64, 0);
                return true;
            }
        }
        false
    }

    /// Whether building `id` is busy (`+0x10c`): its own mending (job
    /// 0xff), a study, a refit batch, a training or weapons batch, a
    /// temple's learning or Flame.
    pub(super) fn busy(&self, id: u32) -> bool {
        let here = |s: Option<u32>| s.is_none_or(|s| s == id);
        let temple = self
            .object_index(id)
            .and_then(|i| match self.objects[i].class {
                ObjectClass::Building(c) => Temple::of_class(c),
                _ => None,
            });
        let temple_busy = temple.is_some_and(|t| {
            (self.magic.learning(t).is_some() && here(self.magic.learning_site(t)))
                || (t == Temple::War
                    && self.magic.flame_brewing().is_some()
                    && here(self.magic.flame_site()))
        });
        self.mending.contains(&id)
            || self.research.iter().any(|r| r.0 == id)
            || self.refits.iter().any(|r| r.0 == id)
            || self.trainings.iter().any(|t| t.site == id)
            || temple_busy
    }

    /// Settlement or town `town` falls (its 0x138b, 0x43bc30): message
    /// 0x2874 {town} to each of its buildings (`+0x1d0`), which each
    /// handle in 0x427f50: the settlement link `+0x1c8` cleared; busy
    /// (`+0x10c`), order 10 to itself (`Scene::halt`); a reload contact
    /// (`+0x104`) told 0x2876 and let go. (A port's ShipService first
    /// sends 0xcb9 to `+0x1e0` and clears it: not ported, `+0x1e0` not
    /// decoded.) The port has no list: a building's settlement is the
    /// nearest of its owner's (`home_settlement`), which a barbarian
    /// structure, never joining one (0x427f50 case 0x2490 refuses nation
    /// 4), and the settlements themselves do not have.
    pub(super) fn town_gone(&mut self, town: u32, data: &GameData, caches: &mut Caches) {
        let theirs: Vec<u32> = (0..self.objects.len())
            .filter(|&i| {
                matches!(self.objects[i].class, ObjectClass::Building(c) if c != SETTLEMENT && c != TOWN)
                    && self.home_settlement(i) == Some(town)
            })
            .map(|i| self.objects[i].id)
            .collect();
        for id in theirs {
            if self.busy(id) {
                let _ = self.halt(id, data, caches);
            }
            self.loads.retain(|l| l.ship != id && l.other != id);
        }
    }

    /// A fort's or barbarian structure's twelve unit groups cease fire
    /// (0x4469b0; the fort also `+0x498 = 0`): it stays silent until set
    /// firing again (`Scene::fort_order`, `fort_aggressed`).
    fn cease_fire(&mut self, id: u32) {
        if let Some(c) = self
            .object_index(id)
            .and_then(|i| self.objects[i].combat.as_mut())
        {
            c.fire.cease();
            c.fire.aggressor = false;
        }
    }

    /// A building's voice event for its owner when that is the viewing
    /// player (0x50e570 mode 2): one of the event's sounds, the first when
    /// it has fewer than two, else `rand() % count` (0x554820, the C
    /// library's generator, seeded from the clock: the port's own).
    /// (Not ported here: 0x525370's cut-off at 20 cells from the view;
    /// the unit report switch is the front end's.)
    pub(super) fn building_voice(
        &mut self,
        owner: Option<(u32, u32)>,
        calls: &Calls,
        x: f32,
        y: f32,
    ) {
        let Some(owner) = owner else { return };
        if !self.human.is_some_and(|h| (h.nation, h.slot) == owner) {
            return;
        }
        let list = calls[owner.0.min(3) as usize];
        let pick = match list.len() {
            0 => return,
            1 => list[0],
            n => list[self.gull_rng.next() as usize % n],
        };
        self.sounds.push((pick, x, y));
    }
}
