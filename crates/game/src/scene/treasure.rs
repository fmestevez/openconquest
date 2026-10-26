//! Treasures and heroes: taking treasures from the map, what they do, and
//! heroes joining ships that bring their magic items (the manual's
//! "Treasures" and "Heroes"; kinds as in `objects::treasure_name`).

use openac_formats::map::TILE_SIZE;
use openac_formats::ObjectClass;

use super::Scene;
use crate::assets::GameData;
use crate::combat;
use crate::economy::{SETTLEMENT, TOWN};
use crate::magic::Spell;
use crate::objects::{self, Caches, Task, Treasure};

/// How near (cells) a ship must come to take a treasure, and to a hero's
/// house to take the hero.
const TAKE_REACH: f32 = 1.0;
const HERO_REACH: f32 = 2.6;
/// Hull points a second mended by the Golden Fleece and by Argus.
/// Placeholders.
pub const FLEECE_REPAIR: f32 = 10.0;
pub const ARGUS_REPAIR: f32 = 5.0;

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
            ObjectClass::HeroHouse => {
                let hero = self.objects[t].record.field(22).unwrap_or(0).min(8) as u8;
                let ship = self.objects[i].ship.as_ref().unwrap();
                if objects::hero_room(kind) <= ship.heroes.len() {
                    return Err("Only warships with room for heroes can take them".into());
                }
                if !ship
                    .treasures
                    .iter()
                    .any(|tr| objects::treasure_hero(tr.kind) == Some(hero))
                {
                    return Err(format!(
                        "{} will only come for his magic item",
                        objects::HERO_NAMES[hero as usize]
                    ));
                }
            }
            _ => return Err("Choose a treasure or a hero's house".into()),
        }
        let (x, y) = (self.objects[t].x, self.objects[t].y);
        let path = self
            .plan_route(i, x, y)
            .ok_or("The ship cannot sail there")?;
        let s = self.objects[i].ship.as_mut().unwrap();
        s.path = path;
        s.task = Some(Task::Take { target });
        Ok(())
    }

    /// Ships that reach the treasure or hero they were sent for take it;
    /// treasures for the settlement are handed over at own settlements and
    /// towns; the Golden Fleece and Argus mend their ships.
    pub(super) fn run_treasures(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let t = TILE_SIZE as f32;
        let takers: Vec<(u32, u32)> = self
            .objects
            .iter()
            .filter_map(|o| match o.ship.as_ref()?.task? {
                Task::Take { target } => Some((o.id, target)),
                _ => None,
            })
            .collect();
        for (ship, target) in takers {
            let (Some(i), Some(g)) = (self.object_index(ship), self.object_index(target)) else {
                if let Some(i) = self.object_index(ship) {
                    self.objects[i].ship.as_mut().unwrap().task = None;
                }
                continue;
            };
            let (o, goal) = (&self.objects[i], &self.objects[g]);
            let d = ((o.x - goal.x).powi(2) + (o.y - goal.y).powi(2)).sqrt();
            let hero = goal.class == ObjectClass::HeroHouse;
            let idle = o.ship.as_ref().unwrap().path.is_empty();
            if d <= if hero { HERO_REACH } else { TAKE_REACH } * t {
                if hero {
                    self.take_hero(i, g);
                } else {
                    self.take_treasure(i, g, data, caches);
                }
            } else if idle {
                // Arrived as near as the sea allows.
                if hero || d > 2.0 * t {
                    self.messages
                        .push("The ship could not get near enough".into());
                    self.objects[i].ship.as_mut().unwrap().task = None;
                } else {
                    self.take_treasure(i, g, data, caches);
                }
            }
        }
        self.deliver_treasures();
        // Lasting gifts that mend the ship.
        for o in &mut self.objects {
            let (Some(s), Some(c)) = (o.ship.as_ref(), o.combat.as_mut()) else {
                continue;
            };
            let mut rate = 0.0;
            if s.treasures.iter().any(|tr| tr.kind == 16) {
                rate += FLEECE_REPAIR;
            }
            // The cup of Asclepius and the Fleece heal the crew's wounds.
            if s.treasures.iter().any(|tr| tr.kind == 15 || tr.kind == 16) {
                c.wounds = (c.wounds - combat::CREW_HEALING * dt).max(0.0);
            }
            if s.heroes.contains(&5) {
                rate += ARGUS_REPAIR;
            }
            if rate > 0.0 && c.hull < c.max_hull {
                c.hull = (c.hull + rate * dt).min(c.max_hull);
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
        self.objects[i].ship.as_mut().unwrap().task = None;
        if self.is_own(&self.objects[i]) {
            self.stats.treasures_found += 1;
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
                        let c = self.objects[i].combat.as_mut().unwrap();
                        match s {
                            Spell::Phantom => c.phantom = crate::magic::PROTECTION_SECONDS,
                            Spell::ProtectWeapons => {
                                c.protect_weapons = crate::magic::PROTECTION_SECONDS
                            }
                            _ => c.protect_magic = crate::magic::PROTECTION_SECONDS,
                        }
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
                if s.treasures.len() >= objects::treasure_room(s.kind) {
                    self.messages
                        .push("There is no room for the treasure".into());
                    return;
                }
                s.treasures.push(tr);
                self.messages.push(format!("Taken aboard: {name}"));
            }
        }
        let _ = ship_id;
        self.sounds.push(("prote1", x, y));
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
        let rec = openac_formats::ObjectRecord {
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

    /// Ship `i` takes the hero from house `g`, giving up the hero's item.
    fn take_hero(&mut self, i: usize, g: usize) {
        let hero = self.objects[g].record.field(22).unwrap_or(0).min(8) as u8;
        let s = self.objects[i].ship.as_mut().unwrap();
        s.task = None;
        let Some(k) = s
            .treasures
            .iter()
            .position(|tr| objects::treasure_hero(tr.kind) == Some(hero))
        else {
            return;
        };
        if s.heroes.contains(&hero) || s.heroes.len() >= objects::hero_room(s.kind) {
            return;
        }
        s.treasures.remove(k);
        s.heroes.push(hero);
        self.messages.push(format!(
            "{} comes aboard",
            objects::HERO_NAMES[hero as usize]
        ));
    }

    /// Goods, weapons and scrolls are handed over at own settlements and
    /// towns.
    fn deliver_treasures(&mut self) {
        let reach = combat::LOAD_REACH * TILE_SIZE as f32;
        let carriers: Vec<usize> = self
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                o.ship
                    .as_ref()
                    .is_some_and(|s| s.treasures.iter().any(|tr| matches!(tr.kind, 6 | 7 | 27)))
            })
            .map(|(i, _)| i)
            .collect();
        for i in carriers {
            let o = &self.objects[i];
            let Some(owner) = o.owner else { continue };
            let site = self
                .objects
                .iter()
                .find(|b| {
                    matches!(b.class, ObjectClass::Building(SETTLEMENT | TOWN))
                        && b.owner == Some(owner)
                        && ((b.x - o.x).powi(2) + (b.y - o.y).powi(2)).sqrt() <= reach
                })
                .map(|b| b.id);
            let Some(site) = site else { continue };
            let s = self.objects[i].ship.as_mut().unwrap();
            let (given, kept): (Vec<Treasure>, Vec<Treasure>) = s
                .treasures
                .drain(..)
                .partition(|tr| matches!(tr.kind, 6 | 7 | 27));
            s.treasures = kept;
            for tr in given {
                self.hand_over(owner, site, tr);
            }
        }
    }

    pub(super) fn hand_over(&mut self, owner: (u32, u32), site: u32, tr: Treasure) {
        if Some(owner) == self.human.map(|h| (h.nation, h.slot)) {
            self.delivered.push(tr.kind);
        }
        match tr.kind {
            6 => {
                // Field 7 names the goods by their deposit class.
                let amount = tr.amount as i64;
                if tr.param == 0x191 {
                    self.earn(owner, 0, amount);
                    self.messages.push(format!("Delivered {amount} fish"));
                } else {
                    self.earn(owner, amount, 0);
                    self.messages.push(format!("Delivered {amount} amber"));
                }
            }
            7 => {
                // Field 7: item (high word) and how many (low word).
                let item = (tr.param >> 16) as usize;
                let count = tr.param & 0xffff;
                if item < 9 {
                    self.garrisons.entry(site).or_default()[item] += count;
                    self.messages
                        .push(format!("Delivered {count} {}", combat::ITEM_NAMES[item]));
                }
            }
            _ => {
                // A scroll of knowledge (field 7 its number) reveals
                // technology (the brains' message 0x5d94).
                if Some(owner) == self.human.map(|h| (h.nation, h.slot)) {
                    let says = self.permits.read_scroll(tr.param);
                    self.messages.push(says.to_string());
                }
            }
        }
    }
}
