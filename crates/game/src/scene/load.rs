//! Reloading objects (the original's load panel, `c_load.cpp`): a ship
//! sent with Reload Object to an own port, shipyard, dock or ship lies
//! alongside it, and marines, weapons, armour, rams, treasures, heroes and
//! goods move between the two, or are thrown away.
//!
//! In the original the ship's contact (`+0x8e6`, type 0x32 for a building,
//! 200 for a ship) sends message 0x2875 to the target once alongside, which
//! takes it up (`+0x104`, `+0x108`); either side then shows the panel. A
//! port, shipyard or dock (`ShipServiceObjClassTy`, 0x4e7640) lists and
//! moves its settlement's stock (`+0x1c8`, `home_settlement`).
//!
//! A fort (`FortressClassTy`, 0x419f80) lists and moves its own units and
//! treasure, with a ship alongside or with its settlement, which takes it up
//! with its own Reload Object (0x441ba0 case 0x1f; no sailing: the two
//! stay linked until Finish reload).

use openconquest_formats::ObjectClass;

use super::Scene;
use crate::assets::GameData;
use crate::combat;
use crate::economy::{DOCK, PORT, SETTLEMENT, SHIPYARD, TOWN};

/// Forts' classes (small, medium, big).
const FORTS: std::ops::RangeInclusive<u32> = 0x34..=0x36;
/// Treasures a fort can store (0x5c1094 by class, through 0x424af0).
const FORT_STORE: usize = 1;
use crate::objects::{self, Caches, Resource, Task, Treasure};

/// Entry flags (the list entries' word at +6, 0x43bfe0, 0x4e5e40,
/// 0x4f9520): nothing there (dimmed), something there, a group to open,
/// locked in place, an open group's header.
pub const EMPTY: u8 = 1;
pub const PRESENT: u8 = 2;
pub const GROUP: u8 = 4;
pub const LOCKED: u8 = 8;
pub const HEADER: u8 = 0x10;

/// Entry categories (the entries' first byte): a unit (marines or a
/// weapon), armour, a ram, a settlement's armour-and-rams place, goods, a
/// hero, a treasure, a settlement's treasures-and-heroes place.
pub const UNIT: u8 = 1;
pub const ARMOUR: u8 = 2;
pub const RAM: u8 = 4;
pub const FITTINGS: u8 = 8;
pub const GOODS: u8 = 0x10;
pub const HERO: u8 = 0x20;
pub const TREASURE: u8 = 0x40;
pub const STORE: u8 = 0x80;

/// Highest armour and ram levels by ship type (0x597b14, 0x597b20).
const MAX_ARMOUR: [u8; 12] = [0, 0, 2, 2, 1, 1, 0, 1, 0, 1, 0, 0];
const MAX_RAM: [u8; 12] = [0, 0, 2, 0, 0, 2, 1, 0, 0, 0, 0, 0];
/// Cargo places of the merchant types (0x597af0).
const CARGO_SLOTS: [u32; 2] = [4, 10];
/// Treasures a building can store, by class (0x5c1094): settlements and
/// towns 50, forts 1, the rest 2.
const SETTLEMENT_STORE: usize = 50;

/// Armour and ram levels stored at a settlement.
pub type Fittings = (Vec<u8>, Vec<u8>);

/// What moves or is thrown away: one marine squad or weapon (`item` as in
/// `combat::ITEM_NAMES`, and its index among the holder's of its item:
/// entry bytes 2-5), armour or a ram of a level, the treasure at
/// `index` among its holder's (its kind to check), a hero, or a merchant's
/// last slot of goods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Cargo {
    Unit(u8, u8),
    Armour(u8),
    Ram(u8),
    Treasure { index: u8, kind: u8 },
    Hero(u8),
    Goods,
}

/// A ship lying alongside an own port, shipyard, dock, fort or ship to
/// reload, or a settlement reloading with one of its forts (`ship` is then
/// the settlement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LoadLink {
    pub ship: u32,
    pub other: u32,
}

/// One entry of a side's list (the 0x14-byte entries of 0x43bfe0 and
/// 0x4e5e40): category, its byte (item, level, hero or treasure kind),
/// flags, the count in its corner, the health bar in percent, and what a
/// move of it carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub cat: u8,
    pub sub: u8,
    pub flags: u8,
    pub count: u32,
    pub bar: Option<u8>,
    pub cargo: Option<Cargo>,
}

impl Entry {
    fn new(cat: u8, sub: u8, flags: u8) -> Self {
        Self {
            cat,
            sub,
            flags,
            count: 0,
            bar: None,
            cargo: None,
        }
    }

    /// Its `COBJ_SSLOT` picture (0x4f9520); `nation3` for the Persians'
    /// marines.
    pub fn frame(&self, nation3: bool) -> u8 {
        let dim = self.flags & EMPTY != 0;
        let s = self.sub;
        match self.cat {
            UNIT => match s {
                0..=2 => {
                    let f = s + if nation3 { 13 } else { 10 };
                    if dim {
                        f + 15
                    } else {
                        f
                    }
                }
                3..=11 if dim => s + 13,
                3..=11 => s - 2,
                12..=14 => s + 0x43,
                _ => 0,
            },
            ARMOUR if dim => 0x21,
            ARMOUR => 0x22 + s,
            RAM if dim => 0x25,
            RAM => 0x23 + s,
            FITTINGS => 0x20 - u8::from(dim),
            GOODS if dim => 0x29,
            GOODS => 0x2a + s,
            HERO if dim => 0x2f,
            HERO => 0x30 + s,
            TREASURE if dim => 0x39,
            TREASURE => 0x3a + treasure_picture(s),
            STORE => 0x2e - u8::from(dim),
            _ => 0,
        }
    }

    /// Its `COBJ_SSPEC` marks (0x4f9520): an arrow to open a group (0, or 1
    /// without anything in it), the arrow to close one (2), a lock (4).
    pub fn marks(&self) -> Vec<u8> {
        let grey = u8::from(self.flags & PRESENT == 0);
        let mut marks = Vec::new();
        if self.flags & GROUP != 0 {
            marks.push(grey);
        }
        if self.flags & HEADER != 0 {
            marks.push(2 + grey);
        }
        if self.flags & LOCKED != 0 {
            marks.push(4 + grey);
        }
        marks
    }

    /// Whether a click opens or closes it as a group (0x4f9da0).
    pub fn is_group(&self) -> bool {
        self.flags & PRESENT != 0 && self.flags & (GROUP | HEADER) != 0
    }

    /// Whether it can be chosen at all.
    pub fn is_choosable(&self) -> bool {
        self.flags & (EMPTY | LOCKED) == 0
    }
}

/// A treasure's place among the pictures from 0x3a (0x5005a0): kinds 6-27
/// in order, but for kind 17 last.
pub fn treasure_picture(kind: u8) -> u8 {
    match kind {
        6 | 7 => kind - 6,
        9..=16 => kind - 7,
        17 => 20,
        18..=27 => kind - 8,
        _ => 0,
    }
}

/// What came out of a holder, on its way to the other: a unit with its
/// condition (the entry's bar, message 0x10ac +0xc).
enum Payload {
    Unit(u8, u8),
    Armour(u8),
    Ram(u8),
    Treasure(Treasure),
    Hero(u8),
    Goods(Resource, f32),
}

impl Scene {
    /// Sends own ship `ship` (Reload Object, command 0x1f with a target) to
    /// lie alongside own port, shipyard, dock, fort or ship `target`; once
    /// there the two can reload (0x4dc630: the targets 0x4dd7af accepts).
    /// A settlement's Reload Object takes up one of its forts at once
    /// (`load_fort`).
    pub fn order_load(&mut self, ship: u32, target: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        if matches!(
            self.objects[i].class,
            ObjectClass::Building(SETTLEMENT | TOWN)
        ) {
            return self.load_fort(ship, target);
        }
        let t = self
            .object_index(target)
            .ok_or("Choose your port, dock or ship")?;
        let kind = self.objects[i].ship.as_ref().ok_or("Not a ship")?.kind;
        if !self.is_own(&self.objects[i]) {
            return Err("Not your ship".into());
        }
        if kind >= 10 {
            return Err("This ship cannot reload".into());
        }
        let other = &self.objects[t];
        let fits = other.ship.is_some()
            || matches!(other.class, ObjectClass::Building(PORT | SHIPYARD | DOCK))
            || matches!(other.class, ObjectClass::Building(c) if FORTS.contains(&c));
        if ship == target || !fits || other.owner != self.objects[i].owner {
            return Err("Choose your port, shipyard, dock, fort or another ship of yours".into());
        }
        if self.load_holder(target).is_none() {
            return Err("The port belongs to no settlement".into());
        }
        if self.construction_at(target).is_some() {
            return Err("It is still being built".into());
        }
        self.loads.retain(|l| l.ship != ship);
        if self.alongside(i, t) {
            let s = self.objects[i].ship.as_mut().unwrap();
            s.path.clear();
            s.task = None;
            self.link(ship, target);
            return Ok(());
        }
        let path = self
            .route_beside(i, t)
            .ok_or("The ship cannot get alongside")?;
        let s = self.objects[i].ship.as_mut().unwrap();
        s.path = path;
        s.task = Some(Task::LoadAt { target });
        Ok(())
    }

    /// Settlement or town `site`'s Reload Object (menu slot 1, command 5;
    /// 0x441ba0 case 0x1f) on its fort `fort`: 0xc452 takes one of its own
    /// buildings (`+0x1d0`) of classes 0x34-0x36 while the settlement stands
    /// idle (`+0xa4` 1, `+0x10c` 0); the two are linked at once (`+0x104`,
    /// `+0x108`, message 0x2875 to the fort) and the panel opens.
    pub fn load_fort(&mut self, site: u32, fort: u32) -> Result<(), String> {
        let s = self.object_index(site).ok_or("No such settlement")?;
        let f = self.object_index(fort).ok_or("Choose one of its forts")?;
        if !self.is_own(&self.objects[s]) {
            return Err("Not your settlement".into());
        }
        if !matches!(
            self.objects[s].class,
            ObjectClass::Building(SETTLEMENT | TOWN)
        ) {
            return Err("Only settlements reload forts".into());
        }
        if self.construction_at(site).is_some()
            || self.training_at(site).is_some()
            || self.stock_repair_at(site).is_some()
            || self.mending.contains(&site)
        {
            return Err("The settlement is busy".into());
        }
        if !self.is_fort(fort)
            || self.objects[f].owner != self.objects[s].owner
            || self.home_settlement(f) != Some(site)
        {
            return Err("Choose one of this settlement's forts".into());
        }
        if self.construction_at(fort).is_some() {
            return Err("It is still being built".into());
        }
        self.link(site, fort);
        Ok(())
    }

    /// Takes `ship` and `other` up for reloading (message 0x2875): a
    /// newcomer replaces any ship already there (`+0x108`).
    fn link(&mut self, ship: u32, other: u32) {
        self.loads
            .retain(|l| l.ship != ship && l.other != other && l.other != ship && l.ship != other);
        self.loads.push(LoadLink { ship, other });
    }

    /// Ships sent to reload take up their target on arriving; links end as
    /// either side sails off, falls or drifts apart (0x4b4401: the contact
    /// ends with message 0x2876).
    pub(super) fn run_loads(&mut self) {
        let arrived: Vec<(u32, u32)> = self
            .objects
            .iter()
            .filter_map(|o| match o.ship.as_ref() {
                Some(s) if s.path.is_empty() => match s.task {
                    Some(Task::LoadAt { target }) => Some((o.id, target)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        for (ship, target) in arrived {
            let Some(i) = self.object_index(ship) else {
                continue;
            };
            self.objects[i].ship.as_mut().unwrap().task = None;
            match self.object_index(target) {
                Some(t) if self.alongside(i, t) => self.link(ship, target),
                _ => {
                    if self.is_own(&self.objects[i]) {
                        self.messages
                            .push("The ship could not get alongside".into());
                    }
                }
            }
        }
        let keep: Vec<bool> = self
            .loads
            .iter()
            .map(|l| {
                let (Some(i), Some(t)) = (self.object_index(l.ship), self.object_index(l.other))
                else {
                    return false;
                };
                let still = |k: usize| {
                    self.objects[k]
                        .ship
                        .as_ref()
                        .is_none_or(|s| s.path.is_empty() && s.task.is_none())
                };
                let standing = |k: usize| {
                    self.objects[k].owner.is_some()
                        && self.objects[k].combat.as_ref().is_none_or(|c| c.hull > 0.0)
                };
                // A settlement and its fort have no contact to lose: they
                // part only at Finish reload (or when one falls).
                if self.objects[i].ship.is_none() {
                    return standing(i) && standing(t);
                }
                still(i) && still(t) && standing(t) && self.alongside(i, t)
            })
            .collect();
        let mut k = 0;
        self.loads.retain(|_| {
            k += 1;
            keep[k - 1]
        });
    }

    /// The link object `id` (a ship or the building it lies at) takes part
    /// in, if any.
    pub fn load_link(&self, id: u32) -> Option<LoadLink> {
        self.loads
            .iter()
            .find(|l| l.ship == id || l.other == id)
            .copied()
    }

    /// Who holds what object `id` lists and moves: a ship, settlement or
    /// fort itself; a port, shipyard or dock its settlement (0x4e7640,
    /// `+0x1c8`).
    pub fn load_holder(&self, id: u32) -> Option<u32> {
        let i = self.object_index(id)?;
        match self.objects[i].class {
            ObjectClass::Ship => Some(id),
            ObjectClass::Building(PORT | SHIPYARD | DOCK) => self.home_settlement(i),
            ObjectClass::Building(SETTLEMENT | TOWN) => Some(id),
            ObjectClass::Building(c) if FORTS.contains(&c) => Some(id),
            _ => None,
        }
    }

    /// Settlement `site`'s forts: the forts of its list (`+0x1d0`).
    pub fn forts_of(&self, site: u32) -> Vec<u32> {
        self.town_buildings(site)
            .into_iter()
            .map(|k| self.objects[k].id)
            .filter(|&id| self.is_fort(id) && self.construction_at(id).is_none())
            .collect()
    }

    /// Whether `id` is a fort.
    fn is_fort(&self, id: u32) -> bool {
        self.object_index(id).is_some_and(
            |i| matches!(self.objects[i].class, ObjectClass::Building(c) if FORTS.contains(&c)),
        )
    }

    /// A fort's list (0x4179b0): each unit with its condition as its bar
    /// (all dimmed while the fort is being mended, `+0x10c` with `+0x110`
    /// 0xff), an empty place for each room left (0x5966d8 less the load),
    /// then its treasures (0x4237b0), or one empty treasure place.
    fn fort_list(&self, i: usize) -> Vec<Entry> {
        let o = &self.objects[i];
        let mending = self.mending.contains(&o.id);
        let mut list = Vec::new();
        for (k, place) in o.fort_places().iter().enumerate() {
            for (j, &c) in place.iter().enumerate() {
                let mut e = Entry::new(UNIT, k as u8, if mending { EMPTY } else { PRESENT });
                e.bar = Some(c);
                e.cargo = Some(Cargo::Unit(k as u8, j as u8));
                list.push(e);
            }
        }
        let weapons = o.combat.as_ref().map_or(&[][..], |c| c.weapons.as_slice());
        let room = combat::fort_room(o.record.class_id).saturating_sub(combat::fort_load(weapons));
        for _ in 0..room {
            list.push(Entry::new(UNIT, 0xff, EMPTY));
        }
        let held = self.held_by(o.id);
        if held.is_empty() {
            list.push(Entry::new(TREASURE, 0, EMPTY));
        }
        for (k, tr) in held.iter().enumerate() {
            let mut e = Entry::new(TREASURE, tr.kind, PRESENT);
            e.cargo = Some(Cargo::Treasure {
                index: k as u8,
                kind: tr.kind,
            });
            list.push(e);
        }
        list
    }

    /// The two holders the load panel shows with `selected` chosen: its
    /// own (top) and its partner's (bottom), as 0x4e4250 and 0x4e7640 fill
    /// them; `None` when it is not reloading.
    pub fn load_sides(&self, selected: u32) -> Option<[u32; 2]> {
        let l = self.load_link(selected)?;
        let partner = if l.ship == selected { l.other } else { l.ship };
        Some([self.load_holder(selected)?, self.load_holder(partner)?])
    }

    /// Whether `a` and `b` are the two holders of one link.
    fn linked(&self, a: u32, b: u32) -> bool {
        self.loads.iter().any(|l| {
            let (x, y) = (self.load_holder(l.ship), self.load_holder(l.other));
            (x == Some(a) && y == Some(b)) || (x == Some(b) && y == Some(a))
        })
    }

    /// Holder `id`'s list for the load panel: a ship's places (0x4e5e40,
    /// 0x4e4120), or a settlement's stock (0x43bfe0 through 0x43c570):
    /// its nine units' places with counts (not groups here), its armour
    /// and rams and its treasures and heroes as groups; with `open` one
    /// group's header and contents instead.
    pub fn load_list(&self, id: u32, open: Option<(u8, u8)>) -> Vec<Entry> {
        let Some(i) = self.object_index(id) else {
            return Vec::new();
        };
        if self.objects[i].ship.is_some() {
            return self.ship_list(i);
        }
        if self.is_fort(id) {
            return self.fort_list(i);
        }
        let held: Vec<&Treasure> = self.held_by(id);
        let heroes = self.site_heroes.get(&id).cloned().unwrap_or_default();
        let (armour, rams) = self.fittings.get(&id).cloned().unwrap_or_default();
        if let Some((cat, sub)) = open {
            let mut list = vec![Entry::new(cat, sub, HEADER | PRESENT)];
            if cat == FITTINGS {
                for &l in &armour {
                    let mut e = Entry::new(ARMOUR, l, PRESENT);
                    e.cargo = Some(Cargo::Armour(l));
                    list.push(e);
                }
                for &l in &rams {
                    let mut e = Entry::new(RAM, l + 3, PRESENT);
                    e.cargo = Some(Cargo::Ram(l));
                    list.push(e);
                }
            } else {
                for (k, tr) in held.iter().enumerate() {
                    let mut e = Entry::new(TREASURE, tr.kind, PRESENT);
                    e.cargo = Some(Cargo::Treasure {
                        index: k as u8,
                        kind: tr.kind,
                    });
                    list.push(e);
                }
                for &h in &heroes {
                    let mut e = Entry::new(HERO, h, PRESENT);
                    e.cargo = Some(Cargo::Hero(h));
                    list.push(e);
                }
            }
            return list;
        }
        let stock = self.stock_units(id);
        let have = |n: usize| if n > 0 { PRESENT } else { EMPTY };
        let mut list = vec![Entry::new(0, 0, 0); 11];
        for item in 0..9u8 {
            let units = &stock[item as usize];
            let n = units.len() as u32;
            let mut e = Entry::new(UNIT, item, have(n as usize));
            e.count = n;
            // The place's bar is its first unit's condition (0x43c570).
            if let Some(&c) = units.first() {
                e.bar = Some(c);
                e.cargo = Some(Cargo::Unit(item, 0));
            }
            list[item as usize + item as usize / 3] = e;
        }
        let mut e = Entry::new(FITTINGS, 0, have(armour.len() + rams.len()) | GROUP);
        e.count = (armour.len() + rams.len()) as u32;
        list[3] = e;
        let mut e = Entry::new(STORE, 0, have(held.len() + heroes.len()) | GROUP);
        e.count = (held.len() + heroes.len()) as u32;
        list[7] = e;
        list
    }

    /// A ship's places (0x4e5e40): its marines and weapons (merchants: the
    /// marines in their cargo places, then a slot a hundred units of goods,
    /// then the empty places), the unique ships' own weapon, empty places,
    /// armour and ram; from the eighth place its treasures and heroes and
    /// their empty places. The unique ships' weapons are locked, and all of
    /// the Argo's places (0x4e4120).
    fn ship_list(&self, i: usize) -> Vec<Entry> {
        let o = &self.objects[i];
        let ship = o.ship.as_ref().unwrap();
        let kind = ship.kind as usize % 12;
        let units = o.unit_conditions();
        let mut list = Vec::new();
        // Each unit with its condition as its bar (0x4e5e40).
        let unit = |item: u8, index: usize| {
            let mut e = Entry::new(UNIT, item, PRESENT);
            e.bar = Some(units[item as usize][index]);
            e.cargo = Some(Cargo::Unit(item, index as u8));
            e
        };
        if kind <= 1 {
            for item in 0..3u8 {
                for k in 0..units[item as usize].len() {
                    list.push(unit(item, k));
                }
            }
            if let Some(goods) = ship.cargo_kind.filter(|_| ship.cargo > 0.0) {
                let sub = match goods {
                    Resource::Amber => 0,
                    Resource::Fish => 1,
                    Resource::Leviathan => 2,
                };
                let slots = (ship.cargo / 100.0).ceil() as usize;
                for k in 0..slots {
                    let left = ship.cargo - k as f32 * 100.0;
                    let mut e = Entry::new(GOODS, sub, PRESENT);
                    e.bar = Some(left.clamp(0.0, 100.0).round() as u8);
                    e.cargo = (k + 1 == slots).then_some(Cargo::Goods);
                    list.push(e);
                }
            }
            while list.len() < CARGO_SLOTS[kind] as usize {
                list.push(Entry::new(GOODS, 0, EMPTY));
            }
        } else {
            let mut room = 0;
            for item in 0..9u8 {
                for k in 0..units[item as usize].len() {
                    list.push(unit(item, k));
                    room += combat::item_size(item) as usize;
                }
            }
            if let Some(own) = combat::special_weapon(kind as u8) {
                let mut e = Entry::new(UNIT, own, PRESENT | LOCKED);
                e.bar = Some(100);
                list.push(e);
                room += 1;
            }
            let places = [0, 0, 5, 7, 4, 3, 2, 2, 0, 1, 1, 3][kind];
            while room < places {
                list.push(Entry::new(UNIT, 0xff, EMPTY));
                room += 1;
            }
            if MAX_ARMOUR[kind] > 0 {
                let mut e = Entry::new(ARMOUR, ship.armour, PRESENT);
                e.cargo = (ship.armour > 0).then_some(Cargo::Armour(ship.armour));
                list.push(e);
            }
            if MAX_RAM[kind] > 0 {
                let mut e = Entry::new(RAM, ship.ram + 3, PRESENT);
                e.cargo = (ship.ram > 0).then_some(Cargo::Ram(ship.ram));
                list.push(e);
            }
        }
        let treasures = objects::treasure_room(kind as u8);
        let heroes = objects::hero_room(kind as u8);
        if treasures + heroes > 0 {
            while list.len() < 7 {
                list.push(Entry::new(0, 0, 0));
            }
            for (k, tr) in ship.treasures.iter().enumerate() {
                let mut e = Entry::new(TREASURE, tr.kind, PRESENT);
                e.bar = Some(100);
                e.cargo = Some(Cargo::Treasure {
                    index: k as u8,
                    kind: tr.kind,
                });
                list.push(e);
            }
            for _ in ship.treasures.len()..treasures {
                list.push(Entry::new(TREASURE, 0, EMPTY));
            }
            for &h in &ship.heroes {
                let mut e = Entry::new(HERO, h, PRESENT);
                e.cargo = Some(Cargo::Hero(h));
                list.push(e);
            }
            for _ in ship.heroes.len()..heroes {
                list.push(Entry::new(HERO, 0, EMPTY));
            }
        }
        if kind == 11 {
            for e in &mut list {
                e.flags |= LOCKED;
            }
        }
        list
    }

    /// The treasures building `id` stores, in order.
    pub(super) fn held_by(&self, id: u32) -> Vec<&Treasure> {
        self.held
            .iter()
            .filter(|h| h.0 == id)
            .map(|h| &h.1)
            .collect()
    }

    /// Moves `cargo` from holder `from` to holder `to`, two sides of one
    /// link (message 0x10ac, directions 0 and 1: 0x4e7640 for a port's
    /// settlement and its ship, 0x4e3b70 between ships). What the receiver
    /// cannot take stays (0x4bc8c0: room aboard, armour and rams the type
    /// takes; a building's room for treasures, 0x424af0).
    pub fn transfer(
        &mut self,
        from: u32,
        to: u32,
        cargo: Cargo,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        self.check_holder(from)?;
        if !self.linked(from, to) {
            return Err("They are not reloading together".into());
        }
        self.can_give(from, cargo)?;
        self.can_take(to, from, cargo)?;
        let payload = self.take_out(from, cargo, false, None, data, caches)?;
        self.put_in(to, from, payload);
        Ok(())
    }

    /// Throws away `cargo` of holder `id` (Kill item, 0x1d20: message
    /// 0x10ac with direction 2 or 3): a unit, armour or ram is lost; a
    /// treasure thrown from a ship falls into the sea there (0x4bd500),
    /// one in a building is gone (0x424cf0).
    pub fn discard(
        &mut self,
        id: u32,
        cargo: Cargo,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        self.check_holder(id)?;
        if !self
            .loads
            .iter()
            .any(|l| self.load_holder(l.ship) == Some(id) || self.load_holder(l.other) == Some(id))
        {
            return Err("Not reloading".into());
        }
        self.can_give(id, cargo)?;
        let i = self.object_index(id).unwrap();
        let at = (self.objects[i].x, self.objects[i].y);
        let overboard = self.objects[i].ship.is_some().then_some(at);
        self.take_out(id, cargo, true, overboard, data, caches)?;
        Ok(())
    }

    /// Ends the reloading object `id` takes part in (Finish reload,
    /// 0x1d1f: message 0xc45b; both sides let go, 0x2876).
    pub fn end_load(&mut self, id: u32) -> Result<(), String> {
        let i = self.object_index(id).ok_or("No such object")?;
        if !self.is_own(&self.objects[i]) {
            return Err("Not yours".into());
        }
        let before = self.loads.len();
        self.loads.retain(|l| l.ship != id && l.other != id);
        if self.loads.len() == before {
            return Err("Not reloading".into());
        }
        Ok(())
    }

    fn check_holder(&self, id: u32) -> Result<(), String> {
        let i = self.object_index(id).ok_or("No such object")?;
        if !self.is_own(&self.objects[i]) {
            return Err("Not yours".into());
        }
        Ok(())
    }

    /// Whether holder `id` has `cargo` to give, and may let it go.
    fn can_give(&self, id: u32, cargo: Cargo) -> Result<(), String> {
        let i = self.object_index(id).ok_or("No such object")?;
        let o = &self.objects[i];
        let missing = Err("It is not there".to_string());
        if let Some(s) = &o.ship {
            if s.kind == 11 {
                return Err("The Argo's places are locked".into());
            }
            let units = o.unit_conditions();
            let ok = match cargo {
                Cargo::Unit(k, index) => units
                    .get(k as usize)
                    .is_some_and(|u| (index as usize) < u.len()),
                Cargo::Armour(l) => l > 0 && s.armour == l,
                Cargo::Ram(l) => l > 0 && s.ram == l,
                Cargo::Treasure { index, kind } => s
                    .treasures
                    .get(index as usize)
                    .is_some_and(|t| t.kind == kind),
                Cargo::Hero(h) => s.heroes.contains(&h),
                Cargo::Goods => s.cargo > 0.0 && s.cargo_kind.is_some(),
            };
            return if ok { Ok(()) } else { missing };
        }
        if self.is_fort(id) {
            // Its units are dimmed while it is being mended (0x4179b0).
            let ok = match cargo {
                Cargo::Unit(..) if self.mending.contains(&id) => {
                    return Err("The fort is being repaired".into())
                }
                Cargo::Unit(k, index) => o
                    .fort_places()
                    .get(k as usize)
                    .is_some_and(|p| (index as usize) < p.len()),
                Cargo::Treasure { index, kind } => self
                    .held_by(id)
                    .get(index as usize)
                    .is_some_and(|t| t.kind == kind),
                _ => false,
            };
            return if ok { Ok(()) } else { missing };
        }
        let (armour, rams) = self.fittings.get(&id).cloned().unwrap_or_default();
        let ok = match cargo {
            Cargo::Unit(k, _) => self.garrison(id).get(k as usize).is_some_and(|&n| n > 0),
            Cargo::Armour(l) => armour.contains(&l),
            Cargo::Ram(l) => rams.contains(&l),
            Cargo::Treasure { index, kind } => self
                .held_by(id)
                .get(index as usize)
                .is_some_and(|t| t.kind == kind),
            Cargo::Hero(h) => self.site_heroes.get(&id).is_some_and(|v| v.contains(&h)),
            Cargo::Goods => false,
        };
        if ok {
            Ok(())
        } else {
            missing
        }
    }

    /// Whether holder `to` can take `cargo` from `from` (0x4bc8c0 for a
    /// ship, 0x4e7640's cases for a settlement).
    fn can_take(&self, to: u32, from: u32, cargo: Cargo) -> Result<(), String> {
        let i = self.object_index(to).ok_or("No such object")?;
        let o = &self.objects[i];
        let full = Err("No room aboard".to_string());
        if self.is_fort(to) {
            // A unit if the fort has room for it and is not being mended
            // (0x419390, 0x417ef0); a treasure into its store of one
            // (0x424b30, 0x424af0: not while it is mended), barrels and
            // goods being credited; no heroes (its vtable +0x80 says no),
            // armour, rams or goods.
            let mending = self.mending.contains(&to);
            return match cargo {
                Cargo::Unit(item, _) => {
                    if mending {
                        return Err("The fort is being repaired".into());
                    }
                    let weapons = o.combat.as_ref().map_or(&[][..], |c| c.weapons.as_slice());
                    let load = combat::fort_load(weapons) + combat::item_power(item);
                    if item >= 9 || load > combat::fort_room(o.record.class_id) {
                        return Err("There is no room in the fort".into());
                    }
                    Ok(())
                }
                Cargo::Treasure { kind, .. } if kind >= 7 => {
                    if mending || self.held_by(to).len() >= FORT_STORE {
                        return Err("There is no room for the treasure".into());
                    }
                    Ok(())
                }
                Cargo::Treasure { .. } => Ok(()),
                Cargo::Hero(_) => Err("Heroes do not stay in forts".into()),
                _ => Err("A fort takes only units and treasures".into()),
            };
        }
        let Some(s) = &o.ship else {
            return match cargo {
                Cargo::Goods => Err("Goods are unloaded with Unload".into()),
                // Not while it mends its stock (0x440d40).
                Cargo::Unit(..) if self.stock_repair_at(to).is_some() => {
                    Err("The settlement is repairing its stock".into())
                }
                Cargo::Treasure { kind, .. } if kind >= 8 => {
                    if self.held_by(to).len() < SETTLEMENT_STORE {
                        Ok(())
                    } else {
                        Err("There is no room for the treasure".into())
                    }
                }
                _ => Ok(()),
            };
        };
        let kind = s.kind as usize % 12;
        // Ships 10 and 11 take nothing (0x4be800).
        if kind >= 10 {
            return Err("This ship cannot take anything aboard".into());
        }
        let weapons = o.combat.as_ref().map_or(&[][..], |c| c.weapons.as_slice());
        let cargo_slots = (s.cargo / 100.0).ceil() as u32;
        match cargo {
            Cargo::Unit(item, _) if kind <= 1 => {
                if item >= 3 {
                    return Err("This ship carries no weapons".into());
                }
                if cargo_slots + s.crew.count() >= CARGO_SLOTS[kind] {
                    return full;
                }
                Ok(())
            }
            Cargo::Unit(item, _) => {
                let free =
                    combat::capacity(kind as u8).saturating_sub(combat::load(&s.crew, weapons));
                if free < combat::item_size(item) {
                    return full;
                }
                Ok(())
            }
            Cargo::Armour(l) | Cargo::Ram(l) => {
                let armour = matches!(cargo, Cargo::Armour(_));
                let (max, now) = if armour {
                    (MAX_ARMOUR[kind], s.armour)
                } else {
                    (MAX_RAM[kind], s.ram)
                };
                if l > max {
                    return Err(format!(
                        "This ship cannot take that {}",
                        if armour { "armour" } else { "ram" }
                    ));
                }
                if l == now {
                    return Err("The ship has it already".into());
                }
                // Between ships the two swap (0x4bcff0): the giver must
                // take the other's.
                if let Some(g) = self.object_index(from) {
                    if let Some(gs) = &self.objects[g].ship {
                        let gmax = if armour {
                            MAX_ARMOUR[gs.kind as usize % 12]
                        } else {
                            MAX_RAM[gs.kind as usize % 12]
                        };
                        if now > gmax {
                            return Err("The other ship cannot take this one's".into());
                        }
                    }
                }
                Ok(())
            }
            Cargo::Treasure { .. } => {
                if s.treasures.len() >= objects::treasure_room(s.kind) {
                    return Err("There is no room for the treasure".into());
                }
                Ok(())
            }
            Cargo::Hero(h) => {
                if s.heroes.len() >= objects::hero_room(s.kind) || s.heroes.contains(&h) {
                    return Err("There is no room for the hero".into());
                }
                Ok(())
            }
            Cargo::Goods => {
                let g = self.object_index(from).ok_or("No such object")?;
                let gs = self.objects[g].ship.as_ref().ok_or("No goods there")?;
                if kind > 1 {
                    return Err("Only merchant ships carry goods".into());
                }
                if s.cargo > 0.0 && s.cargo_kind != gs.cargo_kind {
                    return Err("The ship carries other goods".into());
                }
                if cargo_slots + s.crew.count() >= CARGO_SLOTS[kind] {
                    return full;
                }
                Ok(())
            }
        }
    }

    /// Takes `cargo` out of holder `id`: the first unit of a settlement's
    /// place goes (0x4e7640 direction 0), one thrown away (`kill`) its last
    /// (0x446620). With `overboard` a treasure falls into the sea there.
    fn take_out(
        &mut self,
        id: u32,
        cargo: Cargo,
        kill: bool,
        overboard: Option<(f32, f32)>,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<Payload, String> {
        let i = self.object_index(id).ok_or("No such object")?;
        if self.objects[i].ship.is_some() {
            let o = &mut self.objects[i];
            if let Cargo::Unit(k, index) = cargo {
                let c = o.take_unit(k, index as usize).ok_or("It is not there")?;
                return Ok(Payload::Unit(k, c));
            }
            let s = o.ship.as_mut().unwrap();
            let payload = match cargo {
                Cargo::Unit(..) => unreachable!(),
                Cargo::Armour(l) => {
                    s.armour = 0;
                    Payload::Armour(l)
                }
                Cargo::Ram(l) => {
                    s.ram = 0;
                    Payload::Ram(l)
                }
                Cargo::Treasure { index, .. } => {
                    Payload::Treasure(s.treasures.remove(index as usize))
                }
                Cargo::Hero(h) => {
                    s.heroes.retain(|&x| x != h);
                    Payload::Hero(h)
                }
                Cargo::Goods => {
                    let slots = (s.cargo / 100.0).ceil();
                    let amount = s.cargo - (slots - 1.0) * 100.0;
                    let goods = s.cargo_kind.ok_or("No goods there")?;
                    s.cargo -= amount;
                    if s.cargo <= 0.0 {
                        s.cargo = 0.0;
                        s.cargo_kind = None;
                    }
                    Payload::Goods(goods, amount)
                }
            };
            if let (Payload::Treasure(tr), Some((x, y))) = (&payload, overboard) {
                let tr = *tr;
                self.place_treasure(tr, x, y, data, caches);
            }
            return Ok(payload);
        }
        if self.is_fort(id) {
            // The chosen unit itself goes, either way (0x419340 by its
            // place in the list).
            return Ok(match cargo {
                Cargo::Unit(k, index) => {
                    let mut places = self.objects[i].fort_places();
                    let place = places.get_mut(k as usize).ok_or("It is not there")?;
                    if index as usize >= place.len() {
                        return Err("It is not there".into());
                    }
                    let c = place.remove(index as usize);
                    self.objects[i].set_fort_places(places);
                    Payload::Unit(k, c)
                }
                Cargo::Treasure { index, .. } => {
                    let p = self
                        .held
                        .iter()
                        .enumerate()
                        .filter(|(_, h)| h.0 == id)
                        .nth(index as usize)
                        .map(|(p, _)| p)
                        .ok_or("It is not there")?;
                    Payload::Treasure(self.held.remove(p).1)
                }
                _ => return Err("It is not there".into()),
            });
        }
        Ok(match cargo {
            Cargo::Unit(k, _) => {
                let mut stock = self.stock_units(id);
                let units = &mut stock[k as usize];
                let c = if kill {
                    units.pop()
                } else {
                    (!units.is_empty()).then(|| units.remove(0))
                }
                .ok_or("It is not there")?;
                self.set_stock_units(id, stock);
                Payload::Unit(k, c)
            }
            Cargo::Armour(l) | Cargo::Ram(l) => {
                let f = self.fittings.entry(id).or_default();
                let list = if matches!(cargo, Cargo::Armour(_)) {
                    &mut f.0
                } else {
                    &mut f.1
                };
                let p = list.iter().position(|&x| x == l).ok_or("It is not there")?;
                list.remove(p);
                if f.0.is_empty() && f.1.is_empty() {
                    self.fittings.remove(&id);
                }
                if matches!(cargo, Cargo::Armour(_)) {
                    Payload::Armour(l)
                } else {
                    Payload::Ram(l)
                }
            }
            Cargo::Treasure { index, .. } => {
                let p = self
                    .held
                    .iter()
                    .enumerate()
                    .filter(|(_, h)| h.0 == id)
                    .nth(index as usize)
                    .map(|(p, _)| p)
                    .ok_or("It is not there")?;
                Payload::Treasure(self.held.remove(p).1)
            }
            Cargo::Hero(h) => {
                if let Some(v) = self.site_heroes.get_mut(&id) {
                    v.retain(|&x| x != h);
                    if v.is_empty() {
                        self.site_heroes.remove(&id);
                    }
                }
                Payload::Hero(h)
            }
            Cargo::Goods => return Err("No goods there".into()),
        })
    }

    /// Puts `payload` into holder `to`; armour or a ram it replaces goes
    /// back to `from` (0x4bde80, 0x4bcff0).
    fn put_in(&mut self, to: u32, from: u32, payload: Payload) {
        let Some(i) = self.object_index(to) else {
            return;
        };
        if self.objects[i].ship.is_some() {
            let o = &mut self.objects[i];
            if let Payload::Unit(k, c) = payload {
                o.add_unit(k, c);
                return;
            }
            let s = o.ship.as_mut().unwrap();
            let mut back = None;
            match payload {
                Payload::Unit(..) => unreachable!(),
                Payload::Armour(l) => {
                    if s.armour > 0 {
                        back = Some(Payload::Armour(s.armour));
                    }
                    s.armour = l;
                }
                Payload::Ram(l) => {
                    if s.ram > 0 {
                        back = Some(Payload::Ram(s.ram));
                    }
                    s.ram = l;
                }
                Payload::Treasure(tr) => s.treasures.push(tr),
                Payload::Hero(h) => s.heroes.push(h),
                Payload::Goods(goods, amount) => {
                    s.cargo_kind = Some(goods);
                    s.cargo += amount;
                }
            }
            if let Some(b) = back {
                self.put_in(from, to, b);
            }
            return;
        }
        if self.is_fort(to) {
            match payload {
                // Last of its place (0x446500).
                Payload::Unit(k, c) => {
                    let mut places = self.objects[i].fort_places();
                    if let Some(p) = places.get_mut(k as usize) {
                        p.push(c);
                    }
                    self.objects[i].set_fort_places(places);
                }
                Payload::Treasure(tr) => self.store_at(to, tr),
                _ => {}
            }
            return;
        }
        match payload {
            Payload::Unit(k, c) => {
                let mut stock = self.stock_units(to);
                stock[k as usize].push(c);
                self.set_stock_units(to, stock);
            }
            Payload::Armour(l) => self.fittings.entry(to).or_default().0.push(l),
            Payload::Ram(l) => self.fittings.entry(to).or_default().1.push(l),
            Payload::Hero(h) => self.site_heroes.entry(to).or_default().push(h),
            Payload::Treasure(tr) => self.store_treasure(to, tr),
            Payload::Goods(..) => {}
        }
    }

    /// A building other than a settlement takes treasure `tr` (0x424b30):
    /// barrels of amber or fish and goods are credited, the rest stored
    /// (`can_take` checked the room) and announced (message 0x5d94 with 1).
    fn store_at(&mut self, site: u32, tr: Treasure) {
        let Some(owner) = self.object_index(site).and_then(|i| self.objects[i].owner) else {
            return;
        };
        match tr.kind {
            0..=2 => self.earn(owner, tr.amount as i64, 0),
            3..=5 => self.earn(owner, 0, tr.amount as i64),
            6 => self.hand_over(owner, site, tr),
            _ => {
                self.held.push((site, tr));
                if Some(owner) == self.human.map(|h| (h.nation, h.slot)) {
                    self.delivered.push(tr.kind);
                    self.messages
                        .push(format!("Handed over: {}", objects::treasure_name(tr.kind)));
                }
            }
        }
    }

    /// Building `site` takes treasure `tr` (0x441480, 0x424b30): barrels
    /// of amber or fish and goods are credited, a pack of weapons joins
    /// its stock, the rest is stored and announced to the brains (message
    /// 0x5d94 with 1; here `delivered`).
    fn store_treasure(&mut self, site: u32, tr: Treasure) {
        let Some(owner) = self.object_index(site).and_then(|i| self.objects[i].owner) else {
            return;
        };
        match tr.kind {
            0..=2 => self.earn(owner, tr.amount as i64, 0),
            3..=5 => self.earn(owner, 0, tr.amount as i64),
            6 | 7 => self.hand_over(owner, site, tr),
            _ => {
                self.held.push((site, tr));
                if Some(owner) == self.human.map(|h| (h.nation, h.slot)) {
                    self.delivered.push(tr.kind);
                    self.messages
                        .push(format!("Handed over: {}", objects::treasure_name(tr.kind)));
                    if tr.kind == 27 {
                        let says = self.permits.read_scroll(tr.param);
                        self.messages.push(says.to_string());
                    }
                }
            }
        }
    }
}
