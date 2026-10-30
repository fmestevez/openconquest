//! A town's buildings: the town each building belongs to (`+0x1c8`,
//! WorkLandObj 0x427f50) and the list of those that have stood up for it
//! (TownObj `+0x1d0`, count `+0x1c8`), which its workshop and smithy
//! counts, its yards and its fall go by.
//!
//! A building of classes 0x34-0x3e (`+0x1d8 & 0x30`) keeps a town cell
//! (`+0x1cc`, `+0x1d0`: its record's fields 8 and 9, 0xff for none) and
//! takes as its town the object standing on that cell when it has the
//! same nation and slot (0x427de0). A town orders its buildings with its
//! own anchor as that cell (0x43b290, record type 1), so they are its own
//! from their creation; the map's buildings look theirs up at the start
//! (message 0x8d1), when each town also offers itself (0x2490, 0x441190)
//! to its buildings about it, and each that takes it passes it on to its
//! own neighbours (0x427e40). A building joins its town's list on its
//! first tick standing (message 0x2873, `+0x1d4`) and leaves it when it
//! falls or is called off (0x2874 from the drain 0x427bb0) or is made a
//! dock (0x416340, after which the dock joins again when it stands). A
//! town falling tells every building of its list, which forget it
//! (0x43bc30, 0x427f50 case 0x2874). Buildings never change owner (the
//! nation and slot bytes are written only when they are made), so never
//! change town.

use openconquest_formats::ObjectClass;

use super::Scene;
use crate::economy::{DOCK, SETTLEMENT, SHIPYARD, SMITHY, TOWN, WORKSHOP};

/// The towns' building links, as the original keeps them.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Towns {
    /// Each building's town (`+0x1c8`); none absent.
    pub of: std::collections::BTreeMap<u32, u32>,
    /// Buildings whose town was found and that have not told it yet
    /// (`+0x1d4` 0).
    pub pending: std::collections::BTreeSet<u32>,
    /// Each town's buildings in the order they joined (`+0x1d0`).
    pub lists: std::collections::BTreeMap<u32, Vec<u32>>,
    /// Each town's forts going up, by class 0x34-0x36 (`+0x22c`,
    /// `+0x230`, `+0x234`): one more on each fort its order 0x138c makes
    /// (0x43b290), one fewer (never below 0) as one tells it it stands
    /// (0x2873) or falls while going up (0x2491, 0x4184f0); saved
    /// (0x4416c0 record +0x800).
    #[serde(default)]
    pub going_up: std::collections::BTreeMap<u32, [u32; 3]>,
}

/// The town cell field for none (record fields 8, 9).
const NO_CELL: u32 = 0xff;

impl Scene {
    /// Whether object `i` keeps a town (`+0x1d8 & 0x30`: forts and the
    /// town buildings 0x34-0x3e; a barbarian structure, nation 4, never
    /// does: 0x427f50 refuses it).
    fn keeps_town(&self, i: usize) -> bool {
        matches!(self.objects[i].class, ObjectClass::Building(0x34..=0x3e))
            && self.objects[i].owner.is_some_and(|o| o.0 != 4)
    }

    /// Building `i`'s town cell (`+0x1cc`, `+0x1d0`), `None` for 0xff.
    fn town_cell(&self, i: usize) -> Option<(u32, u32)> {
        let f = &self.objects[i].record.fields;
        let x = f.get(8).copied().unwrap_or(NO_CELL);
        let y = f.get(9).copied().unwrap_or(NO_CELL);
        (x != NO_CELL && y != NO_CELL).then_some((x, y))
    }

    /// The object standing on cell `(x, y)` (the cell's object slot):
    /// a building or settlement covering it.
    fn standing_on(&self, x: i32, y: i32) -> Option<usize> {
        self.objects.iter().position(|o| {
            matches!(o.class, ObjectClass::Building(_)) && o.owner.is_some() && {
                let Some((ox, oy)) = o.cell else { return false };
                let n = super::trade::building_cells(o);
                (ox..ox + n).contains(&x) && (oy..oy + n).contains(&y)
            }
        })
    }

    /// 0x427de0: building `i`'s town is what stands on its town cell when
    /// it has the building's nation and slot (`+0x1d4 = 0`: it will tell
    /// it when standing); else none.
    fn find_town(&mut self, i: usize) {
        let id = self.objects[i].id;
        let town = self.town_cell(i).and_then(|(x, y)| {
            let k = self.standing_on(x as i32, y as i32)?;
            (self.objects[k].owner == self.objects[i].owner).then(|| self.objects[k].id)
        });
        match town {
            Some(t) => {
                self.towns.of.insert(id, t);
                self.towns.pending.insert(id);
            }
            None => {
                self.towns.of.remove(&id);
            }
        }
    }

    /// A building made by town `town`'s order (0x43b290: the record's
    /// town cell the town's anchor, type 1; 0x427f50 message 2 looks its
    /// town up at once).
    pub(super) fn ordered_by(&mut self, id: u32, town: u32) {
        let (Some(i), Some(t)) = (self.object_index(id), self.object_index(town)) else {
            return;
        };
        let (x, y) = self.objects[t]
            .cell
            .unwrap_or((NO_CELL as i32, NO_CELL as i32));
        let fields = &mut self.objects[i].record.fields;
        if fields.len() < 10 {
            fields.resize(10, 0);
        }
        fields[8] = x as u32;
        fields[9] = y as u32;
        if self.keeps_town(i) {
            self.find_town(i);
        }
    }

    /// The start (message 0x8d1 to every object, 0x522d50): in the
    /// objects' order, each building with a town cell looks its town up
    /// (0x427de0) and, standing, tells it (0x2873); each settlement or
    /// town offers itself to the buildings about it (0x441190). Also how
    /// a save without the links has them made again.
    pub(super) fn towns_start(&mut self) {
        self.towns = Towns::default();
        let mut ids: Vec<u32> = self.objects.iter().map(|o| o.id).collect();
        ids.sort_unstable();
        for id in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            match self.objects[i].class {
                ObjectClass::Building(SETTLEMENT | TOWN) if self.objects[i].owner.is_some() => {
                    self.offer_town(i, id, -1..=2);
                }
                _ if self.keeps_town(i) => {
                    if self.town_cell(i).is_some() {
                        self.find_town(i);
                    }
                    if self.construction_at(id).is_none() {
                        self.tell_town(id);
                    }
                }
                _ => {}
            }
        }
    }

    /// 0x441190 (a town, `span` -1..=2 about its anchor) and 0x427e40 (a
    /// building passing it on, -1..=1): message 0x2490 {town, its anchor}
    /// to each object on those cells but `i` with its nation and slot, of
    /// classes 0x34-0x3e but the settlements, at once.
    fn offer_town(&mut self, i: usize, town: u32, span: std::ops::RangeInclusive<i32>) {
        let Some((cx, cy)) = self.objects[i].cell else {
            return;
        };
        let owner = self.objects[i].owner;
        let me = self.objects[i].id;
        for y in span.clone().map(|d| cy + d) {
            for x in span.clone().map(|d| cx + d) {
                let Some(k) = self.standing_on(x, y) else {
                    continue;
                };
                let o = &self.objects[k];
                if o.id != me
                    && o.owner == owner
                    && matches!(o.class, ObjectClass::Building(0x34..=0x3e))
                {
                    self.town_offered(o.id, town);
                }
            }
        }
    }

    /// Message 0x2490 {town} at building `id` (0x427f50): taken only
    /// without a town, with a town cell and not by nation 4; the cell
    /// becomes the town's anchor, the town is looked up again (0x427de0)
    /// and, found, offered on to its neighbours (0x427e40).
    fn town_offered(&mut self, id: u32, town: u32) {
        let (Some(i), Some(t)) = (self.object_index(id), self.object_index(town)) else {
            return;
        };
        if self.towns.of.contains_key(&id) || self.town_cell(i).is_none() || !self.keeps_town(i) {
            return;
        }
        let Some((x, y)) = self.objects[t].cell else {
            return;
        };
        let fields = &mut self.objects[i].record.fields;
        fields[8] = x as u32;
        fields[9] = y as u32;
        self.find_town(i);
        if let Some(&t) = self.towns.of.get(&id) {
            self.offer_town(i, t, -1..=1);
        }
    }

    /// Building `id` tells its town it stands (message 0 or 0x8d1 with
    /// `+0x1d4` 0, 0x2873): the town lists it unless it has fallen or been
    /// sold (TownObj state 2, 3), once.
    fn tell_town(&mut self, id: u32) {
        if !self.towns.pending.remove(&id) {
            return;
        }
        let Some(&town) = self.towns.of.get(&id) else {
            return;
        };
        let up = self
            .object_index(town)
            .is_some_and(|t| self.objects[t].owner.is_some())
            && !self.sold.contains(&town);
        if up {
            let list = self.towns.lists.entry(town).or_default();
            if !list.contains(&id) {
                list.push(id);
            }
            // A fort counts as standing, one fewer going up.
            if let Some(i) = self.object_index(id) {
                if let ObjectClass::Building(c @ 0x34..=0x36) = self.objects[i].class {
                    self.forts_going_up(town, c, -1);
                }
            }
        }
    }

    /// Town `town`'s count of forts of class `class` going up changed by
    /// `d` (0x43b290 `+1`; 0x2873, 0x2491 `-1`, only while not 0).
    pub(super) fn forts_going_up(&mut self, town: u32, class: u32, d: i32) {
        let k = (class - 0x34) as usize;
        let n = &mut self.towns.going_up.entry(town).or_default()[k];
        if d > 0 {
            *n += 1;
        } else if *n != 0 {
            *n -= 1;
        }
    }

    /// Whether town `town` may order no more forts of class `class`
    /// (0x43b290 for order 0x138c, the menu 0x43c740 at 0x43c80e for its
    /// buttons): the weighted count of its forts, standing (`+0x218`
    /// wooden, `+0x214` middle, `+0x21c` stone: those of its list) and
    /// going up (`going_up`), wooden 1, middle 2, stone 3, with the new
    /// one's weight, above 15.
    pub fn forts_full(&self, town: u32, class: u32) -> bool {
        let mut n = [0u32; 3];
        for i in self.town_buildings(town) {
            if let ObjectClass::Building(c @ 0x34..=0x36) = self.objects[i].class {
                n[(c - 0x34) as usize] += 1;
            }
        }
        let up = self.towns.going_up.get(&town).copied().unwrap_or_default();
        let sum: u32 = (0..3).map(|k| (n[k] + up[k]) * (k as u32 + 1)).sum();
        sum + (class - 0x33) > 15
    }

    /// Fort `id` falls or is called off while going up (0x4184f0 on
    /// 0x138b with `+0xa4` 0 and a town): 0x2491 {class - 0x34} to its
    /// town.
    pub(super) fn fort_down(&mut self, id: u32) {
        let (Some(i), Some(&town)) = (self.object_index(id), self.towns.of.get(&id)) else {
            return;
        };
        if self.construction_at(id).is_none_or(|c| c.upgrade) {
            return;
        }
        if let ObjectClass::Building(c @ 0x34..=0x36) = self.objects[i].class {
            self.forts_going_up(town, c, -1);
        }
    }

    /// A game tick of the town buildings (0x427f50 message 0): those
    /// standing with their town not yet told tell it.
    pub(super) fn run_town_links(&mut self) {
        // Buildings gone by a way that did not tell their town (the port
        // removes or ruins them in several places; the original's drain
        // 0x427bb0 sends 0x2874 on every fall) leave its list here.
        let gone: Vec<u32> = self
            .towns
            .of
            .keys()
            .copied()
            .filter(|&id| {
                self.object_index(id)
                    .is_none_or(|i| self.objects[i].owner.is_none())
            })
            .collect();
        for id in gone {
            self.leave_town(id, false);
            self.towns.of.remove(&id);
            self.towns.pending.remove(&id);
        }
        let pending: Vec<u32> = self.towns.pending.iter().copied().collect();
        for id in pending {
            if self.construction_at(id).is_none() {
                self.tell_town(id);
            }
        }
    }

    /// Building `id` leaves its town's list (0x2874 from the drain
    /// 0x427bb0 as it falls or is called off; from 0x416340 as a shipyard
    /// is made a dock, which then tells its town again when it stands:
    /// `again`).
    pub(super) fn leave_town(&mut self, id: u32, again: bool) {
        let Some(&town) = self.towns.of.get(&id) else {
            return;
        };
        if let Some(list) = self.towns.lists.get_mut(&town) {
            list.retain(|&b| b != id);
        }
        if again {
            self.towns.pending.insert(id);
        }
    }

    /// Town `town` falls (0x43bc30): each building of its list forgets
    /// it (`+0x1c8 = 0`); the list goes with it. Returns those buildings.
    pub(super) fn town_falls(&mut self, town: u32) -> Vec<u32> {
        let list = self.towns.lists.remove(&town).unwrap_or_default();
        for id in &list {
            self.towns.of.remove(id);
        }
        list
    }

    /// The town building `i` belongs to (`+0x1c8`, while it stands); a
    /// settlement or town is its own.
    pub fn home_settlement(&self, i: usize) -> Option<u32> {
        let o = &self.objects[i];
        o.owner?;
        let town = match o.class {
            ObjectClass::Building(SETTLEMENT | TOWN) => o.id,
            _ => *self.towns.of.get(&o.id)?,
        };
        let t = self.object_index(town)?;
        self.objects[t].owner.is_some().then_some(town)
    }

    /// Town `town`'s buildings (`+0x1d0`), in the order they joined, as
    /// object indices.
    pub(super) fn town_buildings(&self, town: u32) -> Vec<usize> {
        self.towns
            .lists
            .get(&town)
            .map(|l| l.iter().filter_map(|&id| self.object_index(id)).collect())
            .unwrap_or_default()
    }

    /// Town `town`'s workshops (`+0x1fc`, the first five also kept by
    /// pointer, `+0x1d4[]`) or smithies (`+0x200`, `+0x1e8[]`): those of
    /// its list, in order.
    pub(super) fn town_workshops(&self, town: u32, class: u32) -> Vec<usize> {
        debug_assert!(class == WORKSHOP || class == SMITHY);
        self.town_buildings(town)
            .into_iter()
            .filter(|&k| self.objects[k].class == ObjectClass::Building(class))
            .collect()
    }

    /// The yards of town `town` (its list's shipyards and docks, in
    /// order).
    pub(super) fn town_yards(&self, town: u32) -> Vec<usize> {
        self.town_buildings(town)
            .into_iter()
            .filter(|&k| {
                matches!(
                    self.objects[k].class,
                    ObjectClass::Building(SHIPYARD | DOCK)
                ) && self.is_yard(k)
            })
            .collect()
    }
}
