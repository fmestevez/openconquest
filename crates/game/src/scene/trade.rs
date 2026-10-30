//! Ships, buildings, marines, research and trade.

use super::*;

/// What a new ship or fort is armed with: items (see
/// `combat::ITEM_NAMES`), armour and ram levels.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Arming {
    pub items: Vec<u8>,
    pub armour: u8,
    pub ram: u8,
}

/// The price of a ship or fort with its arming, and the stock of its
/// settlement (`home`) once the arms it holds are taken.
#[derive(Debug, Clone, Default)]
pub struct Price {
    pub amber: u32,
    pub fish: u32,
    pub arming: Arming,
    pub home: Option<u32>,
    pub stock_left: [u32; 9],
}

/// Seconds a frame of a building collapsing into ruins: the ruins' cycle
/// (`objects::BUILDING_CYCLE`, 75 ticks) over its frames, in whole ticks.
fn ruins_frame_seconds(frames: usize) -> f32 {
    crate::clock::ticks((objects::BUILDING_CYCLE[2] / frames.max(1) as u32).max(1))
}

/// Goods a ship takes aboard each time (0x4ad0e0): amber 100 for boats and
/// merchants (20 for other ships), fish by ship type from 0x59b2a8 (boat
/// 33, merchant 50, others 20); the barbarians take a third. Amber comes
/// once per round of the porters, fish once per cast of the net.
fn take_per_round(resource: Resource, kind: u8, barbarian: bool) -> f32 {
    let n = match (resource, kind) {
        // A leviathan 50 a cast, whoever hunts it (0x4ad0e0).
        (Resource::Leviathan, _) => return 50.0,
        (Resource::Amber, 0 | 1) => 100.0,
        (Resource::Fish, 0) => 33.0,
        (Resource::Fish, 1) => 50.0,
        _ => 20.0,
    };
    if barbarian {
        (n / 3.0_f32).floor()
    } else {
        n
    }
}

impl Scene {
    /// Standing buildings the human player owns, as a bit mask over building
    /// classes (bit n = class 0x32 + n). A town also counts as a settlement,
    /// a dock as a shipyard.
    pub fn owned_buildings(&self) -> u32 {
        self.human
            .map_or(0, |h| self.buildings_of((h.nation, h.slot)))
    }

    /// Standing buildings of player `owner`, as for `owned_buildings`.
    pub fn buildings_of(&self, owner: (u32, u32)) -> u32 {
        let mut mask = 0;
        for obj in &self.objects {
            if let ObjectClass::Building(cls) = obj.class {
                let standing = self.construction_at(obj.id).is_none();
                if obj.owner == Some(owner) && standing && (SETTLEMENT..=0x3e).contains(&cls) {
                    mask |= 1 << (cls - SETTLEMENT);
                    if cls == TOWN {
                        mask |= 1;
                    }
                    if cls == DOCK {
                        mask |= 1 << (SHIPYARD - SETTLEMENT);
                    }
                }
            }
        }
        mask
    }

    /// Whether the human player can order a ship of type `kind` now.
    pub fn can_build_ship(&self, kind: u8) -> Result<(), String> {
        let human = self.human.ok_or("No player")?;
        self.can_build_ship_for((human.nation, human.slot), kind)
    }

    /// Whether player `owner` may have a ship of type `kind`, its price
    /// aside (0x43da20 case 4 with param_7 set: the permission, the
    /// studies and the buildings it needs, in that order). (The buildings
    /// are the town's own, `+0x238`; the port counts the player's.)
    pub fn may_build_ship_for(&self, owner: (u32, u32), kind: u8) -> Result<(), String> {
        if Some(owner) == self.human.map(|h| (h.nation, h.slot)) && !self.permits.ship(kind) {
            return Err(format!("{} cannot be built yet", SHIP_NAMES[kind as usize]));
        }
        // -7 (0x43dfb0): a bomber without the armour studied (player
        // `+0x5f`), a rammer without the ram (`+0x5e`).
        let study = match kind {
            3 => Some(combat::Upgrade::Armour),
            5 => Some(combat::Upgrade::Ram),
            _ => None,
        };
        if let Some(u) = study.filter(|&u| self.levels_of(Some(owner)).get(u) == 0) {
            return Err(format!(
                "{} needs {} studied at the sage school",
                SHIP_NAMES[kind as usize],
                u.name()
            ));
        }
        // The unique ships must first be studied at the sage school.
        if (8..12).contains(&kind)
            && self
                .levels_of(Some(owner))
                .get(combat::Upgrade::Special(kind - 8))
                == 0
        {
            return Err(format!(
                "{} must be studied at the sage school first",
                SHIP_NAMES[kind as usize]
            ));
        }
        let need = economy::ship_requirements(kind) | 1;
        if self.buildings_of(owner) & need != need {
            return Err(format!(
                "{} needs more buildings",
                SHIP_NAMES[kind as usize]
            ));
        }
        Ok(())
    }

    /// Whether player `owner` can order a ship of type `kind` now.
    pub fn can_build_ship_for(&self, owner: (u32, u32), kind: u8) -> Result<(), String> {
        self.may_build_ship_for(owner, kind)?;
        let (amber, fish) = economy::ship_cost(kind);
        let (have_amber, have_fish) = self.funds(owner);
        if have_amber < amber || have_fish < fish {
            return Err(format!(
                "Not enough resources: {amber} amber and {fish} fish needed"
            ));
        }
        Ok(())
    }

    /// Amber and fish of player `owner`.
    pub fn funds(&self, owner: (u32, u32)) -> (u32, u32) {
        match self.human {
            Some(h) if (h.nation, h.slot) == owner => (self.amber, self.fish),
            _ => self.purses.get(&owner).copied().unwrap_or_default(),
        }
    }

    /// Adds (or with negative amounts, takes) amber and fish to player
    /// `owner`'s funds; the caller checks they suffice.
    pub(super) fn earn(&mut self, owner: (u32, u32), amber: i64, fish: i64) {
        let add = |v: &mut u32, d: i64| *v = (*v as i64 + d).max(0) as u32;
        match self.human {
            Some(h) if (h.nation, h.slot) == owner => {
                self.stats.amber_earned += amber.max(0) as u64;
                self.stats.fish_caught += fish.max(0) as u64;
                add(&mut self.amber, amber);
                add(&mut self.fish, fish);
            }
            _ => {
                let purse = self.purses.entry(owner).or_default();
                add(&mut purse.0, amber);
                add(&mut purse.1, fish);
            }
        }
    }

    /// Whether yard `site` has a ship under way: being built on its berth
    /// or waiting for it.
    pub fn yard_busy(&self, site: u32) -> bool {
        self.ship_orders.iter().any(|o| o.site == site) || self.ship_on_berth(site).is_some()
    }

    /// Orders a ship of type `kind` at shipyard `site`.
    pub fn start_ship(&mut self, site: u32, kind: u8) -> Result<(), String> {
        self.start_ship_with(site, kind, true)
    }

    /// Orders a ship of type `kind` at shipyard `site` (0x416a40, see
    /// `yard_order`); without `pay` one built now is free (byte 0xf 0xff:
    /// order 0x5c's part; no price test, nothing taken). It must be
    /// allowed (0x43da20, the price aside). The player's yard buttons send
    /// order 0x20 straight to the yard clicked, pay 1, the default-arming
    /// byte the switch at the click (0x4f34c0, 0x49e230 -> 0x520cc0).
    pub fn start_ship_with(&mut self, site: u32, kind: u8, pay: bool) -> Result<(), String> {
        let owner = self
            .object_index(site)
            .and_then(|i| self.objects[i].owner)
            .ok_or("No such shipyard")?;
        if pay {
            self.can_build_ship_for(owner, kind)?;
        } else {
            self.may_build_ship_for(owner, kind)?;
        }
        self.yard_order(site, kind, false, pay, None)
    }

    /// Ships of type `kind` ordered at yard `site` and waiting their turn
    /// (the yard's queue, counted on its buttons by 0x4164e0).
    pub fn ships_queued(&self, site: u32, kind: u8, argo: bool) -> u32 {
        self.ships_waiting(site, kind, argo)
    }

    /// Takes the last ship of type `kind` waiting at yard `site` off its
    /// queue and pays its price back (a right click on its button, message
    /// 0x280f '<', 0x416f90; a queued order took no stock). One
    /// being built on the berth is called off by a stop order instead.
    pub fn cancel_ship(&mut self, site: u32, kind: u8) -> Result<(), String> {
        let owner = self
            .object_index(site)
            .and_then(|i| self.objects[i].owner)
            .ok_or("No such shipyard")?;
        let k = self
            .ship_orders
            .iter()
            .rposition(|p| p.site == site && p.ship == kind && !p.argo)
            .ok_or("None of those are waiting")?;
        let paid = self.ship_orders.remove(k).paid;
        self.earn(owner, paid.0 as i64, paid.1 as i64);
        Ok(())
    }

    /// What a ship of type `kind` built at shipyard `site` costs the
    /// player, with its arming if "default arming" is on (0x43e5c0).
    pub fn ship_price(&self, site: u32, kind: u8) -> Price {
        self.ship_price_with(site, kind, self.default_arming)
    }

    /// The same with the order's default-arming byte `arm` (byte 0xd):
    /// 0x43e5c0 for an order queued, 0x43ead0 for one built now (the same
    /// price; 0x43ead0 also takes what it counts from the stock, the
    /// caller's `stock_left`).
    pub(super) fn ship_price_with(&self, site: u32, kind: u8, arm: bool) -> Price {
        let (hull_amber, hull_fish) = economy::ship_cost(kind);
        let home = self
            .object_index(site)
            .and_then(|i| self.home_settlement(i));
        let stock = home.map(|h| self.garrison(h)).unwrap_or_default();
        let k = kind as usize % 12;
        let mut price = if arm {
            self.plan_arming(
                stock,
                combat::DEFAULT_WEAPONS[k],
                combat::FALLBACK_WEAPONS[k],
                combat::MIN_POWER[k],
                combat::DEFAULT_ARMOUR[k],
                combat::DEFAULT_RAM[k],
            )
        } else {
            Price {
                stock_left: stock,
                ..Price::default()
            }
        };
        price.amber += hull_amber;
        price.fish += hull_fish;
        price.home = home;
        price
    }

    /// What a fort of `class` costs put up near `settlement`, armed if
    /// "default arming" is on (0x43e990); `None` if its weapons cannot be
    /// had (0x43d430).
    pub fn fort_price(&self, settlement: Option<u32>, class: u32) -> Option<Price> {
        let stock = settlement.map(|h| self.garrison(h)).unwrap_or_default();
        let (items, fallbacks) = combat::fort_arming(class);
        let mut price = Price {
            stock_left: stock,
            ..Price::default()
        };
        if self.default_arming {
            if fallbacks
                .iter()
                .any(|&w| stock[w as usize] == 0 && !self.can_make(w))
            {
                return None;
            }
            price = self.plan_arming(stock, items, fallbacks, 0, (0, 0), (0, 0));
        }
        price.amber += economy::building_cost(class);
        price.home = settlement;
        Some(price)
    }

    /// Whether the player may have `item` made (0x43da20 case 1, without
    /// the price): it is permitted, the sage school's arming has reached
    /// its group (player +0xa3, from 1, >= item / 3), and they have a workshop, and
    /// for swordsmen or item 5 a smithy.
    pub fn can_make(&self, item: u8) -> bool {
        self.human
            .is_some_and(|h| self.can_make_by((h.nation, h.slot), item))
    }

    /// Whether player `owner` may have `item` made (see `can_make`).
    pub(super) fn can_make_by(&self, owner: (u32, u32), item: u8) -> bool {
        let have = self.buildings_of(owner);
        let owns = |class: u32| have & (1 << (class - SETTLEMENT)) != 0;
        self.permits.item(item)
            // The original's arming (+0xa3) starts at 1: ours one less.
            && self.levels_of(Some(owner)).weapons + 1 >= item / 3
            && owns(WORKSHOP)
            && (!matches!(item, 2 | 5) || owns(economy::SMITHY))
    }

    /// Whether settlement `site` may put up a fort of `class` (0x43d430
    /// with its items checked, no price): permitted, the stone fort only
    /// at a town, and each fallback weapon in its stock or to be made by
    /// its owner.
    pub(super) fn fort_allowed(&self, site: u32, class: u32) -> bool {
        let Some(i) = self.object_index(site) else {
            return false;
        };
        let Some(owner) = self.objects[i].owner else {
            return false;
        };
        let stock = self.garrison(site);
        let (_, fallbacks) = combat::fort_arming(class);
        self.permits.building(class)
            && (class != 0x36 || self.objects[i].class == ObjectClass::Building(TOWN))
            && fallbacks
                .iter()
                .all(|&w| stock[w as usize] > 0 || self.can_make_by(owner, w))
    }

    /// Whether the player has ship type `kind` unlocked (0x43da20 case 4,
    /// without the price): permitted, studied if a unique ship, and the
    /// buildings it needs standing. Locked ones are left off the
    /// shipyard's menu; unlocked ones it cannot pay for are greyed.
    pub fn ship_unlocked(&self, kind: u8) -> bool {
        match self.can_build_ship(kind) {
            Ok(()) => true,
            Err(_) => {
                let Some(h) = self.human else { return false };
                let owner = (h.nation, h.slot);
                let need = economy::ship_requirements(kind) | 1;
                self.permits.ship(kind)
                    && (!(8..12).contains(&kind)
                        || self
                            .levels_of(Some(owner))
                            .get(combat::Upgrade::Special(kind - 8))
                            > 0)
                    && self.buildings_of(owner) & need == need
            }
        }
    }

    /// The player's settlement or town nearest building `i`: whose stock
    /// arms what it builds.
    pub fn home_settlement(&self, i: usize) -> Option<u32> {
        let (x, y, owner) = (self.objects[i].x, self.objects[i].y, self.objects[i].owner?);
        self.objects
            .iter()
            .filter(|o| {
                o.owner == Some(owner)
                    && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
                    && self.construction_at(o.id).is_none()
            })
            .min_by(|a, b| {
                let d = |o: &&MapObject| (o.x - x).hypot(o.y - y);
                d(a).total_cmp(&d(b))
            })
            .map(|o| o.id)
    }

    /// Default arming (0x43e5c0): each of `items` taken from `stock` if it
    /// holds one, else bought if it may be made, else its fallback (from
    /// stock or bought); then more fallbacks, from the last, until the
    /// arms are as strong as `min_power`; and armour and a ram of their
    /// levels, or of the fallback levels, if they may be fitted.
    fn plan_arming(
        &self,
        mut stock: [u32; 9],
        items: &[u8],
        fallbacks: &[u8],
        min_power: u32,
        armour: (u8, u8),
        ram: (u8, u8),
    ) -> Price {
        let mut price = Price::default();
        let mut power = 0;
        let take = |item: u8, price: &mut Price, stock: &mut [u32; 9]| {
            if stock[item as usize] > 0 {
                stock[item as usize] -= 1;
            } else {
                let (amber, fish) = combat::item_cost(item);
                price.amber += amber;
                price.fish += fish;
            }
            price.arming.items.push(item);
        };
        for (j, &w) in items.iter().enumerate() {
            let item = if stock[w as usize] > 0 || self.can_make(w) {
                w
            } else {
                fallbacks.get(j).copied().unwrap_or(w)
            };
            take(item, &mut price, &mut stock);
            power += combat::item_power(item);
        }
        let mut k = fallbacks.len();
        while k > 1 && power < min_power {
            k -= 1;
            take(fallbacks[k], &mut price, &mut stock);
            power += combat::item_power(fallbacks[k]);
        }
        let levels = self.levels_of(self.human.map(|h| (h.nation, h.slot)));
        // Armour and ram (0x43ead0 steps 4, 5): the level if it may be
        // fitted (0x43da20 case 2 / 3); else a level 2 with no fallback
        // tries level 1 by the same test; else the fallback is bought
        // with no test at all; none without one.
        let fitted = |upgrade: combat::Upgrade, (level, fallback): (u8, u8)| {
            let can = |l: u8| l > 0 && levels.get(upgrade) >= l && self.permits.fit(upgrade, l);
            if level == 0 || can(level) {
                return (level > 0).then_some(level);
            }
            if level == 2 && fallback == 0 {
                return can(1).then_some(1);
            }
            (fallback > 0).then_some(fallback)
        };
        if let Some(l) = fitted(combat::Upgrade::Armour, armour) {
            price.arming.armour = l;
            price.amber += [0, 30, 60][l as usize];
        }
        if let Some(l) = fitted(combat::Upgrade::Ram, ram) {
            price.arming.ram = l;
            price.amber += [0, 25, 50][l as usize];
        }
        price.stock_left = stock;
        price
    }

    /// Ship type of the Argo.
    pub const ROYAL_SHIP: u8 = 2;

    /// Orders the Argo at shipyard `site` (0x7d, code 0x25): a royal ship,
    /// once Argus has come to the player's town (permission bit 80, which
    /// building it clears: there is one Argo).
    pub fn start_argo(&mut self, site: u32) -> Result<(), String> {
        if !self.permits.argo() {
            return Err("Argus must come to your town first".into());
        }
        let owner = self
            .object_index(site)
            .and_then(|i| self.objects[i].owner)
            .ok_or("No such shipyard")?;
        self.can_build_ship_for(owner, Self::ROYAL_SHIP)?;
        self.yard_order(site, Self::ROYAL_SHIP, true, true, None)
    }

    /// In miss002, Argus come to one of the player's settlements lets
    /// them build the Argo (the brain's message 0x5d99), unless it is
    /// afloat or being built.
    pub(super) fn offer_argo(&mut self) {
        if self.mission != "miss002" || self.permits.argo() {
            return;
        }
        const ARGUS: u8 = 5;
        let argus_home = self
            .site_heroes
            .iter()
            .any(|(site, heroes)| heroes.contains(&ARGUS) && self.object_index(*site).is_some());
        let argo = self.ship_orders.iter().any(|p| p.argo)
            || self
                .objects
                .iter()
                .any(|o| self.is_own(o) && o.ship.as_ref().is_some_and(|s| s.argo));
        if argus_home && !argo {
            self.permits.set(crate::permits::ARGO, true);
            self.messages
                .push("Argus can build the Argo at your shipyard".into());
        }
    }

    /// Ship `id` is the Argo.
    pub(super) fn christen_argo(&mut self, id: u32) {
        if let Some(i) = self.object_index(id) {
            self.objects[i].label = "Argo".into();
            if let Some(s) = self.objects[i].ship.as_mut() {
                s.argo = true;
            }
            self.messages.push("The Argo is afloat".into());
        }
    }

    /// Upgrades a settlement to a town or a shipyard to a dock.
    pub fn upgrade(&mut self, id: u32, data: &GameData, caches: &mut Caches) -> Result<(), String> {
        let i = self.object_index(id).ok_or("No such object")?;
        let to = match self.objects[i].class {
            ObjectClass::Building(SETTLEMENT) => TOWN,
            ObjectClass::Building(SHIPYARD) => DOCK,
            _ => return Err("This cannot be upgraded".into()),
        };
        if self.yard_busy(id) {
            return Err("The shipyard is busy".into());
        }
        // The upgrade's price, and the mending of what the building lacks
        // (TownObj 0x43b290 / Dock 0x416340, message 0x138a: 0x42d8b0(0x33)
        // + 0x42d8b0(0x32) x (100 - condition) / 100 when under 100;
        // 0x42d8b0(0x3b) + 0x42d8b0(0x3a) x (100 - condition) / 100).
        let condition = self.objects[i].combat.as_ref().map_or(100, |c| {
            ((c.hull / c.max_hull.max(1.0)) * 100.0).clamp(0.0, 100.0) as u32
        });
        let from = if to == TOWN { SETTLEMENT } else { SHIPYARD };
        let cost =
            economy::building_cost(to) + economy::building_cost(from) * (100 - condition) / 100;
        let owner = self.objects[i].owner.ok_or("No owner")?;
        if self.funds(owner).0 < cost {
            return Err(format!("Not enough amber: {cost} needed"));
        }
        if self.construction_at(id).is_some() {
            return Err("It is still being built".into());
        }
        let mut rec = self.objects[i].record.clone();
        rec.class_id = to;
        rec.fields[0] = to;
        let mut obj = objects::resolve(data, caches, &rec)
            .map_err(|e| e.to_string())?
            .ok_or("Missing upgrade graphics")?;
        // It goes up as a new building does (the town or dock in its
        // building state, 0x43c740 / 0x4164e0 showing the work on the
        // upgrade's button), whole: the price mends it first.
        if let Ok(frames) =
            objects::building_frames(data, caches, &rec, BuildingState::Construction)
        {
            if !frames.is_empty() {
                obj.frames = frames;
            }
        }
        obj.progress = Some(0.0);
        self.earn(owner, -(cost as i64), 0);
        self.objects[i] = obj;
        self.constructions.push(Construction::new(id, true));
        Ok(())
    }

    /// Puts a new ship of type `kind` for `owner` at cell `(cx, cy)`.
    pub(super) fn spawn_ship(
        &mut self,
        owner: (u32, u32),
        kind: u8,
        cx: i32,
        cy: i32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<u32> {
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 13];
        fields[0] = 0xc8;
        fields[1] = 1;
        fields[2] = owner.0;
        fields[3] = owner.1;
        fields[5] = owner.0;
        fields[6] = owner.1;
        fields[7] = kind as u32;
        fields[8] = u32::MAX;
        fields[9] = cx as u32;
        fields[10] = cy as u32;
        fields[11] = 6; // facing south
        fields[12] = 100;
        let rec = ObjectRecord {
            id,
            class_id: 0xc8,
            fields,
        };
        let obj = objects::resolve(data, caches, &rec).ok()??;
        self.objects.push(obj);
        self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        Some(id)
    }
    /// Player `owner`'s research levels.
    pub fn levels_of(&self, owner: Option<(u32, u32)>) -> combat::Levels {
        owner
            .and_then(|o| self.levels.get(&o).copied())
            .unwrap_or_default()
    }

    /// The human player's current level of `upgrade`, if it can still be
    /// raised and no school is already studying it.
    pub fn next_level(&self, upgrade: combat::Upgrade) -> Option<u8> {
        let owner = self.human.map(|h| (h.nation, h.slot));
        let levels = self.levels_of(owner);
        let level = levels.get(upgrade);
        let pending = self.research.iter().any(|r| {
            r.1.excludes(upgrade)
                && self
                    .object_index(r.0)
                    .is_some_and(|i| self.objects[i].owner == owner)
        });
        // The smithy offers silver and golden kinds once the sage school
        // has studied armour or rams that far (0x433e60).
        let known = match upgrade {
            combat::Upgrade::ArmourKind(k) => levels.armour >= k,
            combat::Upgrade::RamKind(k) => levels.ram >= k,
            _ => true,
        };
        (level < upgrade.max_level() && !pending && known && self.permits.research(upgrade, level))
            .then_some(level)
    }

    /// Starts studying `upgrade` at sage school `site`.
    pub fn start_research(&mut self, site: u32, upgrade: combat::Upgrade) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such building")?;
        let class = upgrade.building();
        let building = match class {
            WORKSHOP => "workshop",
            economy::SMITHY => "smithy",
            _ => "sage school",
        };
        if self.objects[i].class != ObjectClass::Building(class) || !self.is_own(&self.objects[i]) {
            return Err(format!("Only your {building} can do that"));
        }
        if self.construction_at(site).is_some() {
            return Err("It is still being built".into());
        }
        if self.research.iter().any(|r| r.0 == site) || self.refits.iter().any(|r| r.0 == site) {
            return Err("The building is busy".into());
        }
        let level = self.next_level(upgrade).ok_or_else(|| {
            let level = self
                .levels_of(self.human.map(|h| (h.nation, h.slot)))
                .get(upgrade);
            if self.permits.research(upgrade, level) {
                "Already as good as it gets"
            } else {
                "Not known yet"
            }
        })?;
        if let combat::Upgrade::Special(k) = upgrade {
            let levels = self.levels_of(self.human.map(|h| (h.nation, h.slot)));
            if levels.weapons < 1 {
                return Err("Study arming first".into());
            }
            let greek = self.human.is_some_and(|h| h.nation == 1);
            if greek != (k < 2) {
                return Err("Not a ship of your nation".into());
            }
        }
        let cost = upgrade.cost(level);
        if self.amber < cost {
            return Err("Not enough amber".into());
        }
        self.amber -= cost;
        self.research.push((site, upgrade, level + 1, 0.0));
        Ok(())
    }

    /// The own ship near smithy `smithy` that a refit of `upgrade` to
    /// `level` would go to: one with less, the nearest first.
    pub fn refit_candidate(
        &self,
        smithy: u32,
        upgrade: combat::Upgrade,
        level: u8,
    ) -> Option<usize> {
        let s = self.object_index(smithy)?;
        let (sx, sy) = (self.objects[s].x, self.objects[s].y);
        self.objects
            .iter()
            .enumerate()
            .filter(|(i, o)| o.owner == self.objects[s].owner && self.alongside(*i, s))
            .filter_map(|(i, o)| {
                let ship = o.ship.as_ref()?;
                let has = if upgrade == combat::Upgrade::Armour {
                    ship.armour
                } else {
                    ship.ram
                };
                let d = ((o.x - sx).powi(2) + (o.y - sy).powi(2)).sqrt();
                (has < level).then_some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Orders a refit of armour or ram of `level` at smithy `smithy`, up
    /// to what the sage school has studied, for a ship of ours alongside
    /// (0x433e60 offers it only then). It is paid now and queued (message
    /// 0x138c, 0x434a00); when its time is up the ship then alongside gets
    /// it (0x43542a, messages 0x248d and 0x248e).
    pub fn refit(
        &mut self,
        smithy: u32,
        upgrade: combat::Upgrade,
        level: u8,
    ) -> Result<(), String> {
        let s = self.object_index(smithy).ok_or("No such building")?;
        if self.objects[s].class != ObjectClass::Building(economy::SMITHY)
            || !self.is_own(&self.objects[s])
        {
            return Err("Only your smithy can do that".into());
        }
        let studied = self
            .levels_of(self.human.map(|h| (h.nation, h.slot)))
            .get(upgrade);
        if studied < level {
            return Err("Your sage school has not studied that yet".into());
        }
        if self.research_at(smithy).is_some() {
            return Err("The smithy is busy".into());
        }
        self.refit_candidate(smithy, upgrade, level)
            .ok_or("No ship of yours lies next to the smithy needing it")?;
        let cost = combat::refit_cost(upgrade, level);
        if self.amber < cost {
            return Err(format!("Not enough amber: {cost} needed"));
        }
        self.amber -= cost;
        // One batch while it is busy (0x434a00): idle, it starts one; each
        // refit adds its price and time (150 or 200 ticks).
        if !self.refits.iter().any(|r| r.0 == smithy) {
            self.refit_batches.remove(&smithy);
        }
        let batch = self.refit_batches.entry(smithy).or_default();
        batch.paid += cost;
        batch.ticks += (combat::refit_seconds(level) / crate::clock::TICK).round() as u32;
        self.refits.push((smithy, upgrade, level, 0.0));
        Ok(())
    }

    /// Takes the last refit of `upgrade` to `level` off `smithy`'s queue,
    /// even the one under way, and pays its price back (message 0x3c,
    /// sent by a right click on its button).
    pub fn cancel_refit(
        &mut self,
        smithy: u32,
        upgrade: combat::Upgrade,
        level: u8,
    ) -> Result<(), String> {
        let k = self
            .refits
            .iter()
            .rposition(|r| r.0 == smithy && (r.1, r.2) == (upgrade, level))
            .ok_or("None of those are queued")?;
        self.refits.remove(k);
        self.amber += combat::refit_cost(upgrade, level);
        Ok(())
    }

    /// The refits queued at `smithy` of `upgrade` to `level`: how many,
    /// and the progress 0..1 of the one under way if it is one of them
    /// (0x433d10).
    pub fn refits_at(
        &self,
        smithy: u32,
        upgrade: combat::Upgrade,
        level: u8,
    ) -> (u32, Option<f32>) {
        let mut queue = self.refits.iter().filter(|r| r.0 == smithy);
        let head = queue
            .next()
            .filter(|r| (r.1, r.2) == (upgrade, level))
            .map(|r| (r.3 / combat::refit_seconds(r.2)).min(1.0));
        let count = self
            .refits
            .iter()
            .filter(|r| r.0 == smithy && (r.1, r.2) == (upgrade, level))
            .count() as u32;
        (count, head)
    }

    /// Each smithy works on its first refit; when done, the ship of its
    /// owner alongside that needs it most nearly gets it.
    pub(super) fn run_refits(&mut self, dt: f32) {
        let alive: Vec<u32> = self.objects.iter().map(|o| o.id).collect();
        self.refits.retain(|r| alive.contains(&r.0));
        let refits = &self.refits;
        self.refit_batches
            .retain(|s, _| refits.iter().any(|r| r.0 == *s));
        let mut seen = Vec::new();
        let mut done = Vec::new();
        let limed = self.limed();
        for (k, r) in self.refits.iter_mut().enumerate() {
            if seen.contains(&r.0) {
                continue;
            }
            seen.push(r.0);
            if limed.contains(&r.0) {
                continue;
            }
            r.3 += dt;
            if let Some(b) = self.refit_batches.get_mut(&r.0) {
                b.elapsed += dt;
            }
            if r.3 >= combat::refit_seconds(r.2) {
                done.push(k);
            }
        }
        for k in done.into_iter().rev() {
            let (smithy, upgrade, level, _) = self.refits.remove(k);
            let Some(i) = self.refit_candidate(smithy, upgrade, level) else {
                continue;
            };
            let ship = self.objects[i].ship.as_mut().unwrap();
            if upgrade == combat::Upgrade::Armour {
                ship.armour = level;
            } else {
                ship.ram = level;
            }
            self.sounds
                .push(("bild6", self.objects[i].x, self.objects[i].y));
        }
    }

    /// Research at `site`: the upgrade and its progress 0..1.
    pub fn research_at(&self, site: u32) -> Option<(combat::Upgrade, f32)> {
        self.research
            .iter()
            .find(|r| r.0 == site)
            .map(|r| (r.1, r.3 / r.1.seconds(r.2.saturating_sub(1))))
    }

    pub(super) fn run_research(&mut self, dt: f32) {
        let human = self.human.map(|h| (h.nation, h.slot));
        let alive: Vec<u32> = self.objects.iter().map(|o| o.id).collect();
        self.research.retain(|r| alive.contains(&r.0));
        let mut done = Vec::new();
        let limed = self.limed();
        for r in &mut self.research {
            if limed.contains(&r.0) {
                continue;
            }
            r.3 += dt;
            if r.3 >= r.1.seconds(r.2.saturating_sub(1)) {
                done.push((r.0, r.1, r.2));
            }
        }
        self.research
            .retain(|r| r.3 < r.1.seconds(r.2.saturating_sub(1)));
        // The school's player learns it (a computer player's too).
        for (site, upgrade, level) in done {
            let Some(owner) = self
                .object_index(site)
                .and_then(|i| self.objects[i].owner)
                .or(human)
            else {
                continue;
            };
            self.levels.entry(owner).or_default().set(upgrade, level);
            if Some(owner) == human {
                self.messages
                    .push(format!("Better {} (level {level})", upgrade.name()));
            }
        }
    }

    /// The training going on at `site`, if any.
    pub fn training_at(&self, site: u32) -> Option<&Training> {
        self.trainings.iter().find(|t| t.site == site)
    }

    /// Items (marines or weapons) waiting at `site`, by item.
    pub fn garrison(&self, site: u32) -> [u32; 9] {
        self.garrisons.get(&site).copied().unwrap_or_default()
    }

    /// The conditions of the units in `site`'s stock by item, in their
    /// order: stock places of two bytes, kind and condition (`[+0x244] +
    /// 0x23`). As many as `garrison` counts: units added since are whole,
    /// those gone the last.
    pub fn stock_units(&self, site: u32) -> [Vec<u8>; 9] {
        let counts = self.garrison(site);
        let mut units = self
            .stock_conditions
            .get(&site)
            .cloned()
            .unwrap_or_default();
        for (u, &n) in units.iter_mut().zip(&counts) {
            u.resize(n as usize, 100);
        }
        units
    }

    /// Keeps the units' conditions, aboard ships and in stock, as many as
    /// the units counted, those gone being the last: a unit counted in
    /// later is whole.
    pub(super) fn sync_units(&mut self) {
        for o in &mut self.objects {
            o.units();
        }
        let sites: Vec<u32> = self.stock_conditions.keys().copied().collect();
        for site in sites {
            let units = self.stock_units(site);
            self.set_stock_units(site, units);
        }
    }

    /// Sets `site`'s stock to `units` (their conditions by item).
    pub(super) fn set_stock_units(&mut self, site: u32, mut units: [Vec<u8>; 9]) {
        // Each kind's units are kept least worn first (0x440e70 sorts them
        // after each comes in, 0x4478e0; taking from either end keeps it).
        for u in &mut units {
            u.sort_by(|a, b| b.cmp(a));
        }
        let counts: [u32; 9] = std::array::from_fn(|k| units[k].len() as u32);
        if counts.iter().all(|&n| n == 0) {
            self.garrisons.remove(&site);
        } else {
            self.garrisons.insert(site, counts);
        }
        if units.iter().flatten().all(|&c| c >= 100) {
            self.stock_conditions.remove(&site);
        } else {
            self.stock_conditions.insert(site, units);
        }
    }

    /// Orders one more `item` (see `combat::ITEM_NAMES`) at `site`,
    /// paying for it: marines at a settlement or town, weapons at a
    /// workshop. A site makes one kind of item at a time.
    pub fn train(&mut self, site: u32, item: u8) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such building")?;
        let fits = match self.objects[i].class {
            ObjectClass::Building(SETTLEMENT | TOWN) => item < 3,
            ObjectClass::Building(WORKSHOP) => (3..9).contains(&item),
            _ => false,
        };
        if !fits {
            return Err("This building cannot make that".into());
        }
        let (amber, fish) = combat::item_cost(item);
        if self.amber < amber || self.fish < fish {
            return Err("Not enough amber or fish".into());
        }
        match self.trainings.iter_mut().find(|t| t.site == site) {
            Some(t) => t.queue.push(item),
            None => {
                let mut t = Training::new(site);
                t.queue.push(item);
                self.trainings.push(t);
            }
        }
        self.amber -= amber;
        self.fish -= fish;
        Ok(())
    }

    /// Takes the last item of kind `item` still to be made off `site`'s
    /// queue and pays its price back (message 0x3c, sub-order 2, sent by a
    /// right click on its button, 0x4f46c0). The time already spent stays
    /// with the run.
    pub fn cancel_training(&mut self, site: u32, item: u8) -> Result<(), String> {
        let t = self
            .trainings
            .iter_mut()
            .find(|t| t.site == site)
            .ok_or("Nothing is being made here")?;
        let k = t
            .queue
            .iter()
            .rposition(|&k| k == item)
            .filter(|&k| k >= t.head)
            .ok_or("None of those are being made")?;
        t.queue.remove(k);
        // Its amber 0x597164[k / 3] back (0x4a8dc0) and its fish
        // 0x597170[k] raw (0x4a8c30): a hundredth of the 0x597170[k] x 100
        // taken (0x4a8be0 at 0x43baac).
        let (amber, fish) = combat::item_cost(item);
        self.amber += amber;
        self.fish += fish / 100;
        Ok(())
    }

    /// Ships sent to load at a settlement take their unit aboard once
    /// alongside it, or give up if they cannot get there.
    pub(super) fn run_fetches(&mut self) {
        let waiting: Vec<(u32, u32, u8)> = self
            .objects
            .iter()
            .filter_map(|o| match o.ship.as_ref() {
                Some(s) if s.path.is_empty() => match s.task {
                    Some(Task::Fetch { site, item }) => Some((o.id, site, item)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        for (ship, site, item) in waiting {
            let (Some(i), Some(s)) = (self.object_index(ship), self.object_index(site)) else {
                continue;
            };
            self.objects[i].ship.as_mut().unwrap().task = None;
            if self.alongside(i, s) {
                if let Err(e) = self.load_item(site, ship, item) {
                    if self.is_own(&self.objects[i]) {
                        self.messages.push(e);
                    }
                }
            }
        }
    }

    /// Puts one unit of `item` from `site`'s stock aboard `ship`, which must
    /// lie alongside and have room (the original's load cursor, c_load.cpp).
    /// Sells one `item` of settlement `site`'s stock, the first of its
    /// place, for three quarters of its amber price times its condition
    /// (0x441ba0, message 0x280f); no fish comes back.
    pub fn sell_item(&mut self, site: u32, item: u8) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such settlement")?;
        if !self.is_own(&self.objects[i]) {
            return Err("Not your settlement".into());
        }
        let mut stock = self.stock_units(site);
        let units = stock.get_mut(item as usize).ok_or("No such item")?;
        if units.is_empty() {
            return Err("None left".into());
        }
        let condition = units.remove(0) as u32;
        self.set_stock_units(site, stock);
        let amber = combat::item_cost(item).0 * 75 / 100 * condition / 100;
        if let Some(owner) = self.objects[i].owner {
            self.earn(owner, amber as i64, 0);
        }
        Ok(())
    }

    /// Sells an armour piece, or with `ram` a ram, of `level` from
    /// settlement `site`'s stock (message 0x280f): three quarters of its
    /// fitting cost (0x441ba0).
    pub fn sell_fitting(&mut self, site: u32, ram: bool, level: u8) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such settlement")?;
        if !self.is_own(&self.objects[i]) {
            return Err("Not your settlement".into());
        }
        let (armour, rams) = self.fittings.entry(site).or_default();
        let pieces = if ram { rams } else { armour };
        let k = pieces.iter().position(|&l| l == level).ok_or("None left")?;
        pieces.remove(k);
        let upgrade = if ram {
            combat::Upgrade::Ram
        } else {
            combat::Upgrade::Armour
        };
        let amber = if level == 0 {
            0
        } else {
            combat::refit_cost(upgrade, level) * 75 / 100
        };
        if let Some(owner) = self.objects[i].owner {
            self.earn(owner, amber as i64, 0);
        }
        Ok(())
    }

    /// Sells settlement or town `site` (command 0x80: order 0x34 byte 3,
    /// which it turns into message 0x248f; 0x4f34c0, 0x441ba0): three
    /// quarters of each stock unit's amber price, and of a crew's fish, by
    /// its condition, of each armour and ram stored (0x42d960, 0x42d940),
    /// and of the building's cost by its condition (0x425990), all in
    /// integers. It is not destroyed: it stands sold (state 3), drawn as its
    /// building picture's last frame, keeping its owner and stock, with no
    /// menu and taking no orders.
    pub fn sell_town(
        &mut self,
        site: u32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such settlement")?;
        let obj = &self.objects[i];
        if !self.is_own(obj) || !matches!(obj.class, ObjectClass::Building(SETTLEMENT | TOWN)) {
            return Err("Not your settlement".into());
        }
        if self.sold.contains(&site) {
            return Err(String::new());
        }
        // Standing and idle (`+0x10c`): not building, training or mending.
        if self.construction_at(site).is_some()
            || self.training_at(site).is_some()
            || self.stock_repair_at(site).is_some()
        {
            return Err("The settlement is busy".into());
        }
        let ObjectClass::Building(class) = obj.class else {
            unreachable!()
        };
        let condition = obj.combat.as_ref().map_or(100, |c| {
            (c.hull / c.max_hull * 100.0).clamp(0.0, 100.0) as i64
        });
        let owner = obj.owner;
        let stock = self.stock_units(site);
        let (mut amber, mut fish) = (0i64, 0i64);
        for (item, units) in stock.iter().enumerate() {
            for &c in units {
                let c = c as i64;
                amber += combat::item_cost(item as u8).0 as i64 * c * 75 / 10000;
                if item < 3 {
                    // 0x597170 by the hundred: 2, 2, 3 fish.
                    fish += combat::CREW_COST[item].1 as i64 / 100 * c * 7500 / 10000;
                }
            }
        }
        if let Some((armour, rams)) = self.fittings.get(&site) {
            for &level in armour.iter().filter(|&&l| l > 0) {
                amber +=
                    combat::refit_cost(combat::Upgrade::Armour, level) as i64 * 100 * 75 / 10000;
            }
            for &level in rams.iter().filter(|&&l| l > 0) {
                amber += combat::refit_cost(combat::Upgrade::Ram, level) as i64 * 100 * 75 / 10000;
            }
        }
        amber += economy::building_cost(class) as i64 * condition * 75 / 10000;
        if let Some(owner) = owner {
            self.earn(owner, amber, fish);
        }
        self.sold.push(site);
        self.show_sold(site, data, caches);
        Ok(())
    }

    /// A sold town's look: its building picture held on its last frame
    /// (0x422e30, state 3).
    pub(super) fn show_sold(&mut self, site: u32, data: &GameData, caches: &mut Caches) {
        let Some(i) = self.object_index(site) else {
            return;
        };
        let rec = self.objects[i].record.clone();
        if let Ok(frames) =
            objects::building_frames(data, caches, &rec, BuildingState::Construction)
        {
            if let Some(last) = frames.last() {
                self.objects[i].frames = vec![last.clone()];
            }
        }
    }

    pub fn load_item(&mut self, site: u32, ship: u32, item: u8) -> Result<(), String> {
        let s = self.object_index(site).ok_or("No such settlement")?;
        let i = self.object_index(ship).ok_or("No such ship")?;
        if !self.is_own(&self.objects[i]) || self.objects[i].ship.is_none() {
            return Err("Choose one of your ships".into());
        }
        let mut stock = self.stock_units(site);
        if stock.get(item as usize).is_none_or(|u| u.is_empty()) {
            return Err("None left".into());
        }
        let kind = self.objects[i].ship.as_ref().unwrap().kind;
        if item >= 3 && !combat::carries_weapons(kind) {
            return Err("This ship carries no weapons".into());
        }
        let obj = &mut self.objects[i];
        let (Some(ship), Some(fight)) = (obj.ship.as_mut(), obj.combat.as_mut()) else {
            return Err("Not a ship".into());
        };
        let free = combat::capacity(kind).saturating_sub(combat::load(&ship.crew, &fight.weapons));
        if free < combat::item_size(item) {
            return Err("No room aboard".into());
        }
        // A ship away from the settlement sails alongside first, and takes
        // the unit aboard there (the original's load pointer sends it).
        if !self.alongside(i, s) {
            let path = self
                .route_beside(i, s)
                .ok_or("The ship cannot reach the settlement")?;
            let ship = self.objects[i].ship.as_mut().unwrap();
            ship.path = path;
            ship.task = Some(Task::Fetch { site, item });
            return Ok(());
        }
        if self.objects[i].combat.is_none() {
            return Err("Not a ship".into());
        }
        let condition = stock[item as usize].remove(0);
        self.objects[i].add_unit(item, condition);
        self.set_stock_units(site, stock);
        Ok(())
    }

    /// Ship `ship` takes on the marines and weapons waiting at own
    /// settlements, towns and workshops nearby, as many as it has room for.
    /// Returns how many items came aboard.
    pub fn load_crew(&mut self, ship: u32) -> Result<u32, String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let sites: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                self.is_own(o)
                    && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN | WORKSHOP))
            })
            .filter(|o| {
                self.object_index(o.id)
                    .is_some_and(|k| self.alongside(i, k))
            })
            .map(|o| o.id)
            .collect();
        if sites.is_empty() {
            return Err("The ship must lie next to your settlement or workshop".into());
        }
        if self.objects[i].ship.is_none() || self.objects[i].combat.is_none() {
            return Err("Not a ship".into());
        }
        let kind = self.objects[i].ship.as_ref().unwrap().kind;
        let room = combat::capacity(kind);
        let mut n = 0;
        for site in sites {
            let mut stock = self.stock_units(site);
            // The heaviest weapons first, then swordsmen, the strongest
            // boarders, and the other marines.
            let order = (3..9u8)
                .rev()
                .filter(|_| combat::carries_weapons(kind))
                .chain((0..3).rev());
            for item in order {
                let obj = &self.objects[i];
                let load = combat::load(
                    &obj.ship.as_ref().unwrap().crew,
                    &obj.combat.as_ref().unwrap().weapons,
                );
                let free = room.saturating_sub(load);
                let units = &mut stock[item as usize];
                let take = ((free / combat::item_size(item)) as usize).min(units.len());
                for c in units.drain(..take) {
                    self.objects[i].add_unit(item, c);
                }
                n += take as u32;
            }
            self.set_stock_units(site, stock);
            // Heroes waiting there come aboard, as many as there is room
            // for.
            let obj = &mut self.objects[i];
            let ship = obj.ship.as_mut().unwrap();
            if let Some(waiting) = self.site_heroes.get_mut(&site) {
                while ship.heroes.len() < crate::objects::hero_room(kind) && !waiting.is_empty() {
                    ship.heroes.push(waiting.remove(0));
                    n += 1;
                }
                if waiting.is_empty() {
                    self.site_heroes.remove(&site);
                }
            }
        }
        if n == 0 {
            let full = {
                let o = &self.objects[i];
                combat::load(
                    &o.ship.as_ref().unwrap().crew,
                    &o.combat.as_ref().unwrap().weapons,
                ) >= room
            };
            return Err(if full {
                "The ship is full"
            } else {
                "Nothing is waiting to be loaded"
            }
            .into());
        }
        Ok(n)
    }

    /// Advances marine training and weapon building.
    pub(super) fn run_training(&mut self, dt: f32) {
        let limed = self.limed();
        for t in &mut self.trainings {
            if limed.contains(&t.site) {
                continue;
            }
            t.elapsed += dt;
            // Each item is ready once the run has lasted to its end
            // (0x441ba0: 0x248c for the one before the item now in hand).
            loop {
                let end: f32 = t.queue[..(t.head + 1).min(t.queue.len())]
                    .iter()
                    .map(|&k| combat::train_seconds(k))
                    .sum();
                if t.head >= t.queue.len() || t.elapsed < end {
                    break;
                }
                let kind = t.queue[t.head];
                self.garrisons.entry(t.site).or_default()[kind as usize] += 1;
                t.head += 1;
            }
        }
        let alive: Vec<u32> = self.objects.iter().map(|o| o.id).collect();
        self.trainings
            .retain(|t| t.head < t.queue.len() && alive.contains(&t.site));
    }

    /// The construction going on at `site`, if any.
    pub fn construction_at(&self, site: u32) -> Option<&Construction> {
        self.constructions.iter().find(|c| c.site == site)
    }

    /// Whether building class `class` can go up on cell `(cx, cy)` for the
    /// settlement `origin` (the town's check, message 0x4eb, 0x42dae0); on
    /// success, the facing (ports, shipyards and docks towards the water).
    /// A town builds next to one of its own cells: the settlement's, or
    /// one of its buildings' or forts' (any of the 8 about); its forts
    /// with no rule for the tile (flags 0x20), the rest as `check_ground`
    /// says (0x15 inland, 0x1e on the shore). A ship founding (`origin` a
    /// ship) must lie near the site.
    pub fn check_site(&self, origin: u32, class: u32, cx: i32, cy: i32) -> Result<u32, String> {
        let t = TILE_SIZE as f32;
        let i = self.object_index(origin).ok_or("No settlement")?;
        let home = &self.objects[i];
        if home.ship.is_some() {
            let (hx, hy) = ((home.x / t).floor() as i32, (home.y / t).floor() as i32);
            if (hx - cx).abs().max((hy - cy).abs()) > FOUND_REACH {
                return Err("The ship must lie next to the site".into());
            }
            return self.check_ground(class, cx, cy);
        }
        let owner = home.owner;
        let own_cell = |x: i32, y: i32| {
            self.objects.iter().enumerate().any(|(k, o)| {
                let Some((ox, oy)) = o.cell else { return false };
                let n = building_cells(o);
                let on = (ox..ox + n).contains(&x) && (oy..oy + n).contains(&y);
                on && o.owner == owner
                    && match o.class {
                        ObjectClass::Building(SETTLEMENT | TOWN) => o.id == origin,
                        ObjectClass::Building(0x34..=0x3e) => {
                            self.home_settlement(k) == Some(origin)
                        }
                        _ => false,
                    }
            })
        };
        let beside = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
            .any(|(dx, dy)| (dx, dy) != (0, 0) && own_cell(cx + dx, cy + dy));
        if !beside {
            return Err("Too far from the settlement".into());
        }
        if (0x34..=0x36).contains(&class) {
            self.free_ground(cx, cy)?;
            return Ok(0);
        }
        self.check_ground(class, cx, cy)
    }

    /// Whether building `i` (a fort, port, workshop and the like of a
    /// town's) still stands on a site its town could build it on: the
    /// town's check run on its own cell, what stands there aside (message
    /// 0x4eb with its flags and 0x40, the town's id; WorkLandObj 0x427f50
    /// asks it when lightning strikes about it).
    pub(crate) fn site_holds(&self, i: usize) -> bool {
        let o = &self.objects[i];
        let (ObjectClass::Building(class @ 0x34..=0x3e), Some((cx, cy))) = (o.class, o.cell) else {
            return true;
        };
        let Some(town) = self.home_settlement(i) else {
            return false;
        };
        let owner = o.owner;
        let own_cell = |x: i32, y: i32| {
            self.objects.iter().enumerate().any(|(k, b)| {
                let Some((bx, by)) = b.cell else { return false };
                let n = building_cells(b);
                (bx..bx + n).contains(&x)
                    && (by..by + n).contains(&y)
                    && b.owner == owner
                    && match b.class {
                        ObjectClass::Building(SETTLEMENT | TOWN) => b.id == town,
                        ObjectClass::Building(0x34..=0x3e) => {
                            k != i && self.home_settlement(k) == Some(town)
                        }
                        _ => false,
                    }
            })
        };
        let beside = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
            .any(|(dx, dy)| (dx, dy) != (0, 0) && own_cell(cx + dx, cy + dy));
        if !beside {
            return false;
        }
        if (0x34..=0x36).contains(&class) {
            return self.free_ground_but(cx, cy, false).is_ok();
        }
        self.check_ground_but(class, cx, cy, false).is_ok()
    }

    /// The edges of the land tile on cell `(x, y)` that meet the water, a
    /// bit each: 1 north, 2 east, 4 south, 8 west (0x5acf13 by the tile's
    /// variant); none for inland tiles.
    pub(super) fn tile_edges(&self, x: i32, y: i32) -> u8 {
        const EDGES: [u8; 45] = [
            0, 0, 9, 9, 9, 9, 9, 3, 3, 3, 3, 3, 12, 12, 12, 12, 12, 6, 6, 6, 6, 6, 0, 0, 0, 0, 0,
            0, 0, 0, 8, 8, 2, 2, 1, 1, 1, 4, 4, 4, 4, 4, 0, 0, 0,
        ];
        if x < 0 || y < 0 || x as u32 >= self.grid.width || y as u32 >= self.grid.height {
            return 0;
        }
        let v = self.grid.get(0, x as u32, y as u32).variant as usize;
        EDGES.get(v).copied().unwrap_or(0)
    }

    /// Whether a building may stand on cell `(cx, cy)` at all (0x42dae0's
    /// first tests): land, nothing standing there, and none of tile sets
    /// 0x1a and 0x30 on layers 5-7 (0x503150; trees and rocks do not
    /// matter).
    pub(super) fn free_ground(&self, cx: i32, cy: i32) -> Result<(), String> {
        self.free_ground_but(cx, cy, true)
    }

    /// The same, minding what stands there only if `occupied` (0x42dae0
    /// flag 0x40 leaves it out).
    pub(super) fn free_ground_but(&self, cx: i32, cy: i32, occupied: bool) -> Result<(), String> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        if cx < 0 || cy < 0 || cx >= w || cy >= h {
            return Err("Off the map".into());
        }
        if self.nav.is_water(cx, cy) {
            return Err("Buildings go on land".into());
        }
        let g = &self.grid;
        let barred = (5..g.layers.min(8)).any(|l| {
            let t = g.get(l, cx as u32, cy as u32);
            !t.is_empty() && matches!(t.tileset, 0x1a | 0x30)
        });
        let taken = self.objects.iter().any(|o| {
            o.cell
                .filter(|_| {
                    matches!(
                        o.class,
                        ObjectClass::Building(_)
                            | ObjectClass::Barbarian(_)
                            | ObjectClass::Castle(_)
                            | ObjectClass::HarpyNest
                            | ObjectClass::CreatureSpawn
                            | ObjectClass::HeroHouse
                    )
                })
                .is_some_and(|(ox, oy)| {
                    let n = building_cells(o);
                    (ox..ox + n).contains(&cx) && (oy..oy + n).contains(&cy)
                })
        });
        // Nothing on its third place, and not a volcano's ground
        // (0x42dae0; these whatever stands there).
        let (third, volcano) = self.blocked_cells();
        let blocked = third.contains(&(cx, cy)) || volcano.contains(&(cx, cy));
        if barred || blocked || (taken && occupied) {
            return Err("There is no room there".into());
        }
        Ok(())
    }

    /// Whether building class `class` can stand on cell `(cx, cy)`, wherever
    /// it is ordered from; on success, the facing. Inland buildings on an
    /// inland tile; ports, shipyards and docks on a shore tile facing
    /// water south, north, west or (whatever the tile) east (0x42dae0
    /// flags 2 and 8); forts founded by ship on a shore tile (0x4e4420,
    /// flags 0x20); a settlement on 2 x 2 free inland cells with a shore
    /// tile in the ring about them (0x42e1e0).
    pub fn check_ground(&self, class: u32, cx: i32, cy: i32) -> Result<u32, String> {
        self.check_ground_but(class, cx, cy, true)
    }

    pub(super) fn check_ground_but(
        &self,
        class: u32,
        cx: i32,
        cy: i32,
        occupied: bool,
    ) -> Result<u32, String> {
        if class == SETTLEMENT {
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                self.free_ground_but(cx + dx, cy + dy, occupied)?;
                if self.tile_edges(cx + dx, cy + dy) != 0 {
                    return Err("This must be built inland, away from the shore".into());
                }
            }
            let shore = (cy - 1..=cy + 2).any(|y| {
                (cx - 1..=cx + 2).any(|x| {
                    let ring = x == cx - 1 || x == cx + 2 || y == cy - 1 || y == cy + 2;
                    ring && !self.nav.is_water(x, y)
                        && x >= 0
                        && y >= 0
                        && (x as u32) < self.grid.width
                        && (y as u32) < self.grid.height
                        && self.tile_edges(x, y) != 0
                })
            });
            return if shore {
                Ok(0)
            } else {
                Err("A settlement goes near the shore".into())
            };
        }
        self.free_ground_but(cx, cy, occupied)?;
        let edges = self.tile_edges(cx, cy);
        if (0x34..=0x36).contains(&class) {
            return if edges != 0 {
                Ok(0)
            } else {
                Err("Forts are founded on the shore".into())
            };
        }
        if !matches!(class, PORT | SHIPYARD | DOCK) {
            return if edges == 0 {
                Ok(0)
            } else {
                Err("This must be built inland, away from the shore".into())
            };
        }
        if edges == 0 {
            return Err("Moorages and shipyards must be built on the shore".into());
        }
        // Facing 6 south, 2 north, 4 west on the tile's water edges, then
        // 0 east whatever the tile.
        let water = |x: i32, y: i32| self.nav.is_water(x, y);
        [
            (6, 4, (0, 1)),
            (2, 1, (0, -1)),
            (4, 8, (-1, 0)),
            (0, 0, (1, 0)),
        ]
        .into_iter()
        .find(|&(_, bit, (dx, dy))| (bit == 0 || edges & bit != 0) && water(cx + dx, cy + dy))
        .map(|(facing, ..)| facing)
        .ok_or_else(|| "Moorages and shipyards must face the water".into())
    }

    /// Starts putting up building `class` on cell `(cx, cy)` near settlement
    /// `settlement`, paying for it.
    /// What building class `class` would look like standing at cell
    /// `(cx, cy)` facing `facing`: the preview shown where it may go.
    pub fn site_preview(
        &self,
        class: u32,
        cx: i32,
        cy: i32,
        facing: u32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<MapObject> {
        let human = self.human?;
        let mut fields = vec![0u32; 66];
        fields[0] = class;
        fields[1] = 1;
        fields[2] = human.nation;
        fields[3] = human.slot;
        fields[5] = cx as u32;
        fields[6] = cy as u32;
        fields[7] = 100;
        fields[10] = facing;
        let rec = ObjectRecord {
            id: 0,
            class_id: class,
            fields,
        };
        objects::resolve(data, caches, &rec).ok().flatten()
    }

    pub fn place_building(
        &mut self,
        settlement: u32,
        class: u32,
        cx: i32,
        cy: i32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        self.place_building_facing(settlement, class, cx, cy, None, data, caches)
    }

    /// As `place_building`; with `facing` given, on a cell already chosen
    /// (a computer town's, `m_build.cpp`) without the player's checks.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn place_building_facing(
        &mut self,
        settlement: u32,
        class: u32,
        cx: i32,
        cy: i32,
        facing: Option<u32>,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        // The settlement's (or founding ship's) player builds it.
        let owner = self
            .object_index(settlement)
            .and_then(|i| self.objects[i].owner)
            .ok_or("No settlement")?;
        let human = self.human.is_some_and(|h| (h.nation, h.slot) == owner);
        // A fort comes armed (default arming), its weapons paid for or
        // taken from the nearest settlement's stock; a computer player's
        // with its standard weapons.
        let fort = if human && (0x34..=0x36).contains(&class) {
            let home = self
                .object_index(settlement)
                .and_then(|i| self.home_settlement(i));
            Some(
                self.fort_price(home, class)
                    .ok_or("Its weapons cannot be had yet")?,
            )
        } else {
            None
        };
        let (cost, fish) = fort
            .as_ref()
            .map_or((economy::building_cost(class), 0), |p| (p.amber, p.fish));
        let (have_amber, have_fish) = self.funds(owner);
        if have_amber < cost || have_fish < fish {
            return Err("Not enough resources".into());
        }
        let facing = match facing {
            Some(f) => f,
            None => self.check_site(settlement, class, cx, cy)?,
        };
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 66];
        fields[0] = class;
        fields[1] = 1;
        fields[2] = owner.0;
        fields[3] = owner.1;
        fields[5] = cx as u32;
        fields[6] = cy as u32;
        fields[7] = 100;
        fields[10] = facing;
        let rec = ObjectRecord {
            id,
            class_id: class,
            fields,
        };
        let failed = |e: anyhow::Error| format!("Cannot build here: {e:#}");
        let mut obj = objects::resolve(data, caches, &rec)
            .map_err(failed)?
            .ok_or("Your nation cannot build that")?;
        let frames = objects::building_frames(data, caches, &rec, BuildingState::Construction)
            .map_err(failed)?;
        if !frames.is_empty() {
            obj.frames = frames;
        }
        // The building goes up as the work does.
        obj.progress = Some(0.0);
        if let Some(c) = obj.combat.as_mut() {
            c.hull = c.max_hull * 0.1;
        }
        self.earn(owner, -(cost as i64), -(fish as i64));
        if let Some(price) = fort {
            if let Some(home) = price.home {
                self.garrisons.insert(home, price.stock_left);
            }
            if let Some(c) = obj.combat.as_mut() {
                c.weapons = price
                    .arming
                    .items
                    .iter()
                    .copied()
                    .filter(|&w| w >= 3)
                    .collect();
            }
        }
        self.objects.push(obj);
        self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        self.constructions.push(Construction::new(id, false));
        Ok(())
    }

    /// Advances constructions a tick (0x4230f0, state 0): a frame every
    /// `BUILDING_CYCLE[0] / n` ticks (`now >= last + delay`, the first on
    /// the first tick); while the frame is under `n` the condition is
    /// `frame x 100 / n` and the hit points `max x condition / 100`
    /// (0x423410, integers on the original's points); the step reaching
    /// `n` stands it (condition 100, state 1), and the owner's finished
    /// call plays (events 188-191 by nation, for the viewing player).
    /// (The original leaves `+0xac` at the last step's share while `+0xa8`
    /// is 100; the port keeps one figure and makes it whole.)
    pub(super) fn step_constructions(&mut self, data: &GameData, caches: &mut Caches) {
        let mut done = Vec::new();
        for k in 0..self.constructions.len() {
            let Some(i) = self.object_index(self.constructions[k].site) else {
                continue;
            };
            let n = (self.objects[i].frames.len() as u32).max(1);
            let c = &mut self.constructions[k];
            c.frames = n;
            let delay = objects::BUILDING_CYCLE[0] / n;
            if c.started {
                c.wait += 1;
                if c.wait < delay {
                    continue;
                }
            }
            c.started = true;
            c.wait = 0;
            c.frame += 1;
            let f = c.frame;
            if f >= n {
                done.push(c.site);
                continue;
            }
            self.objects[i].progress = Some((f as f32 + 0.5) / n as f32);
            if let Some(h) = self.objects[i].combat.as_mut() {
                let condition = f as u64 * 100 / n as u64;
                let max = (h.max_hull * 15.0).round() as u64;
                h.hull = (max * condition / 100) as f32 / 15.0;
            }
        }
        let alive: Vec<u32> = self.objects.iter().map(|o| o.id).collect();
        self.constructions
            .retain(|c| c.frame < c.frames.max(1) && alive.contains(&c.site));
        for id in done {
            if let Some(h) = self
                .object_index(id)
                .and_then(|i| self.objects[i].combat.as_mut())
            {
                h.hull = h.max_hull;
            }
            self.refresh_building(id, data, caches);
            if let Some(i) = self.object_index(id) {
                let o = &self.objects[i];
                if self.is_own(o) {
                    let (x, y, owner) = (o.x, o.y, o.owner);
                    self.building_voice(owner, &super::halt::FINISHED_CALLS, x, y);
                }
            }
        }
    }

    /// Re-selects a building's frames for its current condition.
    pub(super) fn refresh_building(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let obj = &self.objects[i];
        let Some(c) = &obj.combat else { return };
        let percent = c.hull / c.max_hull * 100.0;
        let state = BuildingState::for_condition(percent);
        match objects::building_frames(data, caches, &obj.record, state) {
            Ok(frames) if !frames.is_empty() => {
                self.objects[i].frame_ms = objects::building_frame_ms(frames.len());
                self.objects[i].frames = frames;
            }
            Ok(_) => {}
            Err(e) => eprintln!("could not load building frames: {e:#}"),
        }
        // Its condition picks the picture (thirds); the picture's frames
        // then play round on the building's cycle, damaged or not
        // (LandObjClassTy 0x422e30).
        self.objects[i].progress = None;
    }

    /// Ruins collapse once, then lie still on their last frame.
    pub(super) fn run_ruins(&mut self, dt: f32) {
        for obj in &mut self.objects {
            if !matches!(obj.class, ObjectClass::Building(_))
                || obj.owner.is_some()
                || obj.combat.is_some()
            {
                continue;
            }
            if let Some(p) = obj.progress.as_mut().filter(|p| **p < 1.0) {
                let n = obj.frames.len().max(1) as f32;
                *p = (*p + dt / (n * ruins_frame_seconds(obj.frames.len()))).min(1.0);
            }
        }
    }

    /// Orders merchant `ship` to gather from `deposit` and deliver to port.
    pub fn order_gather(&mut self, ship: u32, deposit: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let d = self.object_index(deposit).ok_or("No such deposit")?;
        let kind = self.objects[i].ship.as_ref().ok_or("Not a ship")?.kind;
        if cargo_capacity(kind) <= 0.0 {
            return Err("Only merchant ships can gather".into());
        }
        let resource = self.objects[d]
            .deposit
            .ok_or("Nothing to gather there")?
            .resource;
        let (x, y) = self.deposit_goal(i, d);
        let path = self
            .plan_route(i, x, y)
            .ok_or("The ship cannot sail there")?;
        let ship = self.objects[i].ship.as_mut().unwrap();
        if ship.cargo_kind.is_some_and(|k| k != resource) {
            ship.cargo = 0.0;
        }
        ship.path = path;
        ship.task = Some(Task::Gather {
            deposit,
            phase: GatherPhase::ToDeposit,
        });
        Ok(())
    }

    /// The building of ship `i`'s player nearest to it where cargo can be
    /// unloaded (0x4c5910 mode 0 over the player's building list): a port
    /// (class 0x39) not asleep (vt+0x2c, 0x40dfc0: `+0xa4 != 0x10`) by its
    /// berth (vt+0xb8, 0x40e050); for player 16 a barbarian structure of
    /// class 0x40 by its anchor (vt+0x28, 0x40dfa0). Settlements and towns
    /// take no cargo (0x4aab00 drops an unloading order at one). Nearest
    /// by the wavefront 0x4a1e90 from the ship's cell over its draught's
    /// grid with limit 5000: the least `W[ship] - W[cell]` (a cell the
    /// flood did not reach counts as `W[ship]`, a closed one one more),
    /// the first in the list on a tie (strict `<`). (The list in object
    /// order; a facing below 0, which gives 0x40e050's (-1, -1), counts
    /// no berth here.)
    pub(super) fn unloading_site(&self, i: usize) -> Option<u32> {
        let owner = self.objects[i].owner?;
        let t = TILE_SIZE as f32;
        let at = (
            (self.objects[i].x / t).floor() as i32,
            (self.objects[i].y / t).floor() as i32,
        );
        let deep = crate::objects::deep_draught(self.objects[i].ship.as_ref()?.kind);
        let w = self.nav.wavefront(at, deep, 5000)?;
        let width = self.nav.width as i32;
        let start = w[(at.1 * width + at.0) as usize] as i32;
        let mut best: Option<(i32, u32)> = None;
        for (k, o) in self.objects.iter().enumerate() {
            if o.owner != Some(owner) || !self.takes_cargo(o) {
                continue;
            }
            let cell = match o.class {
                ObjectClass::Building(PORT) => self.berth_cell(k),
                _ => o.cell,
            };
            let Some((x, y)) =
                cell.filter(|&(x, y)| x >= 0 && y >= 0 && x < width && y < self.nav.height as i32)
            else {
                continue;
            };
            let d = start - w[(y * width + x) as usize] as i32;
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, o.id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Whether building `o` takes an unloading order from a ship of its own
    /// player (0x4aab00 with code 0xc, 0x4ac050): a port not asleep; for
    /// player 16 a barbarian structure of class 0x40.
    pub(super) fn takes_cargo(&self, o: &MapObject) -> bool {
        match o.class {
            ObjectClass::Building(PORT) => o.owner != Some((4, 0)) && o.asleep.is_none(),
            ObjectClass::Barbarian(0x40) => o.owner == Some((4, 0)),
            _ => false,
        }
    }

    /// How far from building `p` (in cells) a ship can come: from its
    /// nearest water, and a cell more.
    pub(super) fn landing_reach(&self, p: usize) -> f32 {
        let t = TILE_SIZE as f32;
        let (x, y) = (self.objects[p].x, self.objects[p].y);
        self.nav
            .nearest_water((x / t) as i32, (y / t) as i32)
            .map_or(2.6, |(cx, cy)| {
                let (wx, wy) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
                (wx - x).hypot(wy - y) / t + 1.0
            })
    }

    /// Advances the standing order of ship `i`.
    pub(super) fn run_task(&mut self, i: usize, dt: f32, data: &GameData, caches: &mut Caches) {
        if let Some(Task::Found { cx, cy, class }) =
            self.objects[i].ship.as_ref().and_then(|s| s.task)
        {
            if !self.objects[i].ship.as_ref().unwrap().path.is_empty() {
                return;
            }
            self.objects[i].ship.as_mut().unwrap().task = None;
            let id = self.objects[i].id;
            let own = self.is_own(&self.objects[i]);
            if let Err(e) = self.place_building(id, class, cx, cy, data, caches) {
                if !own {
                    return;
                }
                self.messages.push(format!(
                    "Cannot build the {}: {e}",
                    crate::objects::building_label(class)
                ));
            }
            return;
        }
        let t = TILE_SIZE as f32;
        let Some(ship) = &self.objects[i].ship else {
            return;
        };
        let Some(Task::Gather { deposit, phase }) = ship.task else {
            return;
        };
        if !ship.path.is_empty() {
            return;
        }
        let (sx, sy) = (self.objects[i].x, self.objects[i].y);
        let near = |o: &MapObject, cells: f32| {
            ((o.x - sx).powi(2) + (o.y - sy).powi(2)).sqrt() <= cells * t
        };
        let capacity = cargo_capacity(ship.kind);
        let set = |scene: &mut Scene, task: Option<Task>, path: Option<VecDeque<(f32, f32)>>| {
            let ship = scene.objects[i].ship.as_mut().unwrap();
            ship.task = task;
            if let Some(p) = path {
                ship.path = p;
            }
        };
        // An exhausted deposit growing back counts as gone.
        let deposit_at = self
            .object_index(deposit)
            .filter(|&d| self.objects[d].deposit.is_some());
        match phase {
            GatherPhase::ToDeposit => {
                let arrived = deposit_at.is_some_and(|d| near(&self.objects[d], 1.6));
                // A shoal: wait while it is below; sail on to where it
                // comes up.
                if let Some(d) =
                    deposit_at.filter(|&d| self.objects[d].class == ObjectClass::FishZone)
                {
                    if self.shoal_down(self.objects[d].id) {
                        return;
                    }
                    if !arrived {
                        let (x, y) = (self.objects[d].x, self.objects[d].y);
                        if let Some(path) = self.plan_route(i, x, y) {
                            let ship = self.objects[i].ship.as_mut().unwrap();
                            ship.path = path;
                        }
                        return;
                    }
                }
                // At amber the ship first closes in on the shore, until its
                // bow is about to touch land (the original moves it there
                // along a path, Run_Ship.cpp 0x4b0590).
                if let (true, Some(d)) = (arrived, deposit_at) {
                    let amber = self.objects[d]
                        .deposit
                        .is_some_and(|dep| dep.resource == Resource::Amber);
                    let cell = self.objects[d]
                        .record
                        .fields
                        .get(6..8)
                        .map(|f| (f[0] as i32, f[1] as i32));
                    if let (true, Some(cell)) = (amber, cell) {
                        if let Some(((ax, ay), face)) = self.approach(i, cell, false) {
                            let ship = self.objects[i].ship.as_mut().unwrap();
                            ship.face = Some(face);
                            if (ax - sx).hypot(ay - sy) > 1.0 {
                                ship.path = [(ax, ay)].into();
                                return;
                            }
                        }
                    }
                }
                // Stopped short (it gave a jammed route up, 0x4a618d): its
                // gathering sends it on again while that gets it nearer.
                if let (false, Some(d)) = (arrived, deposit_at) {
                    let (dx, dy) = (self.objects[d].x, self.objects[d].y);
                    let gap = |(x, y): (f32, f32)| (x - dx).hypot(y - dy);
                    if let Some(path) = self.plan_route(i, dx, dy) {
                        if path
                            .back()
                            .is_some_and(|&e| gap(e) + t / 2.0 < gap((sx, sy)))
                        {
                            return set(self, Some(Task::Gather { deposit, phase }), Some(path));
                        }
                    }
                }
                let next = arrived.then_some(Task::Gather {
                    deposit,
                    phase: GatherPhase::Collecting,
                });
                set(self, next, None);
            }
            GatherPhase::Collecting => {
                let Some(d) = deposit_at else {
                    return self.send_to_port(i, deposit);
                };
                // A shoal that has gone below or moved on: follow it.
                if self.objects[d].class == ObjectClass::FishZone
                    && (self.shoal_down(self.objects[d].id) || !near(&self.objects[d], 1.6))
                {
                    let ship = self.objects[i].ship.as_mut().unwrap();
                    ship.task = Some(Task::Gather {
                        deposit,
                        phase: GatherPhase::ToDeposit,
                    });
                    return;
                }
                let dep = self.objects[d].deposit.unwrap();
                // A walkway to the amber, or the net out.
                let activity = match dep.resource {
                    Resource::Amber => Some(super::unload::Activity::Amber),
                    Resource::Fish | Resource::Leviathan => Some(super::unload::Activity::Fishing),
                };
                if let Some(a) = activity {
                    self.show_activity(i, deposit, a, data, caches);
                }
                let ship = self.objects[i].ship.as_ref().unwrap();
                // Once the walkway is down or the net out, a load a round of
                // its animation.
                // Player 0x10: nation 4 (the barbarians), slot 0.
                let barbarian = self.objects[i].owner == Some((4, 0));
                let take = take_per_round(dep.resource, ship.kind, barbarian);
                let rate = self
                    .planks
                    .iter()
                    .find(|p| p.ship == self.objects[i].id)
                    .and_then(|p| p.round_seconds())
                    .map_or(0.0, |s| take / s);
                let mut take = (rate * dt).min(capacity - ship.cargo);
                if let Some(left) = dep.remaining {
                    take = take.min(left);
                }
                let ship = self.objects[i].ship.as_mut().unwrap();
                ship.cargo += take;
                ship.cargo_kind = Some(dep.resource);
                let (full, cargo) = (ship.cargo >= capacity, ship.cargo);
                let mut exhausted = false;
                let mut rest = 0.0;
                let mut thirds = None;
                if let Some(left) = self.objects[d]
                    .deposit
                    .as_mut()
                    .and_then(|d| d.remaining.as_mut())
                {
                    let third = |v: f32| (v / 100.0).ceil() as i64;
                    thirds = Some(third(*left));
                    *left -= take;
                    exhausted = *left <= 1e-3;
                    if exhausted {
                        // What rounding left of it goes aboard too.
                        rest = left.max(0.0);
                        *left = 0.0;
                    }
                    // The picture shrinks as the amber runs out.
                    if thirds == Some(third(*left)) {
                        thirds = None;
                    }
                }
                if exhausted {
                    // What rounding left of it goes aboard too, the load
                    // made whole.
                    let s = self.objects[i].ship.as_mut().unwrap();
                    let c = s.cargo + rest;
                    s.cargo = if (c - c.round()).abs() < 0.01 {
                        c.round()
                    } else {
                        c
                    }
                    .min(capacity);
                }
                if thirds.is_some() && !exhausted {
                    self.refresh_amber(d, data, caches);
                }
                if exhausted {
                    let ship_id = self.objects[i].id;
                    let dep = self.objects[d].deposit.unwrap();
                    if dep.regrows > 0 {
                        // Amber grows back after a while.
                        self.objects[d].deposit = None;
                        self.objects[d].dormant = Some((dep, 0.0));
                        self.refresh_amber(d, data, caches);
                    } else {
                        // Removing the deposit may shift the ship's index.
                        self.objects.remove(d);
                    }
                    let Some(i) = self.object_index(ship_id) else {
                        return;
                    };
                    if cargo > 0.0 {
                        self.send_to_port(i, deposit);
                    } else if let Some(ship) = self.objects[i].ship.as_mut() {
                        ship.task = None;
                    }
                } else if full {
                    self.send_to_port(i, deposit);
                }
            }
            GatherPhase::ToPort(port) => {
                let Some(p) = self.object_index(port) else {
                    return self.send_to_port(i, deposit);
                };
                // At a moorage's front; at player 16's barbarian structure
                // as close as the water lets it come (not decoded: its own
                // cell vt+0x28, which 0x4c5910 reads for it).
                let arrived = match self.front_cell(p) {
                    Some(_) => {
                        let (bx, by) = self.berth(p);
                        if (bx - sx).hypot(by - sy) > 0.75 * t {
                            false
                        } else {
                            // In its cell, it closes in on the pier.
                            if let Some(cell) = self.objects[p].cell {
                                if let Some(((ax, ay), face)) = self.approach(i, cell, true) {
                                    let ship = self.objects[i].ship.as_mut().unwrap();
                                    ship.face = Some(face);
                                    if (ax - sx).hypot(ay - sy) > 1.0 {
                                        ship.path = [(ax, ay)].into();
                                        return;
                                    }
                                }
                            }
                            true
                        }
                    }
                    None => near(&self.objects[p], self.landing_reach(p).max(2.6)),
                };
                if !arrived {
                    return self.send_to_port(i, deposit);
                }
                // Arrived: the porters carry the goods in.
                set(
                    self,
                    Some(Task::Gather {
                        deposit,
                        phase: GatherPhase::Unloading(port),
                    }),
                    None,
                );
                self.show_activity(i, port, super::unload::Activity::Unloading, data, caches);
            }
            GatherPhase::Unloading(port) => {
                let Some(p) = self.object_index(port) else {
                    return self.send_to_port(i, deposit);
                };
                // Lying at a port with its anchor down, the porters at
                // work, it is held: move state 6, written each tick
                // (0x4adef0 step 0x11 at 0x4ae486). Only ports take that
                // path (the unloading order 0xc, 0x4aab00: class 0x39, for
                // player 16 class 0x40); 0x4b0590 is the amber collecting
                // (sub-codes 10 / 11), and towns take no cargo.
                let at_port = self.unloads_at_port(i, p);
                if at_port {
                    self.objects[i].ship.as_mut().unwrap().move_state = 6;
                }
                let id = self.objects[i].id;
                let walkway = self
                    .planks
                    .iter()
                    .any(|p| p.ship == id && p.activity == super::unload::Activity::Unloading);
                let trips = self.unloaded.iter().filter(|&&s| s == id).count() as f32;
                self.unloaded.retain(|&s| s != id);
                let ship = self.objects[i].ship.as_mut().unwrap();
                let kind = ship.cargo_kind;
                // Goods come ashore a slot a laden porter trip (at the
                // porters' pace where no walkway shows).
                let before = ship.cargo;
                let after = if walkway {
                    // The original counts whole units: a fraction left
                    // (rounding at large steps) is not worth a trip, and
                    // would keep the ship waiting for one.
                    Some((before - trips * super::unload::TRIP_LOAD).max(0.0))
                        .filter(|&a| a >= 1.0)
                        .unwrap_or(0.0)
                } else {
                    (before - super::unload::UNLOAD_RATE * dt).max(0.0)
                };
                ship.cargo = after;
                let came = (before.floor() - after.floor()) as i64;
                // Each unloading draws the ship a priority of 91-100
                // (0x4ad9f0), above those coming in to gather (90), so
                // leaving it may push them aside.
                if came > 0 && (1..235).contains(&ship.priority) {
                    ship.priority = 91 + (self.rng.next() % 10) as u8;
                }
                if let (Some(owner), true) = (self.objects[i].owner, came > 0) {
                    // What it is worth (0x4ad9f0).
                    let (num, den) = crate::objects::AMBER_WORTH;
                    let whole = |v: f32| v.floor() as i64;
                    match kind {
                        Some(Resource::Amber) => {
                            let paid = whole(before.floor() * num as f32 / den as f32)
                                - whole(after.floor() * num as f32 / den as f32);
                            self.earn(owner, paid, 0)
                        }
                        Some(Resource::Fish) => self.earn(owner, 0, came),
                        Some(Resource::Leviathan) => {
                            // Its 2500 fish as the unloading ends.
                            let fish = if after <= 0.0 {
                                crate::objects::LEVIATHAN_FISH
                            } else {
                                0
                            };
                            self.earn(owner, crate::objects::LEVIATHAN_AMBER * came, fish)
                        }
                        None => {}
                    }
                }
                if after > 0.0 {
                    return;
                }
                // It casts off once its porters are back aboard.
                let id = self.objects[i].id;
                if self.porters.iter().any(|p| p.ship == id)
                    || self.planks.iter().any(|p| p.ship == id && p.trips % 2 == 1)
                {
                    return;
                }
                let roll = 91 + (self.rng.next() % 10) as u8;
                let ship = self.objects[i].ship.as_mut().unwrap();
                ship.cargo = 0.0;
                ship.cargo_kind = None;
                // Unloaded, an order's priority between 1 and 234 is
                // rolled again, 91-100 (0x4ad9f0).
                if (1..=234).contains(&ship.priority) {
                    ship.priority = roll;
                }
                // Leaving a port its route is cleared and it lies still
                // (step 0x13, 0x4a5b70).
                if at_port {
                    ship.move_state = 0;
                }
                match deposit_at {
                    Some(d) => {
                        let (x, y) = self.deposit_goal(i, d);
                        let path = self.plan_route(i, x, y);
                        let task = path.as_ref().map(|_| Task::Gather {
                            deposit,
                            phase: GatherPhase::ToDeposit,
                        });
                        set(self, task, path);
                    }
                    None => set(self, None, None),
                }
                // It moves a cell off the port's berth (0x4b46f0 with '[',
                // priority 91), going on from there. (Not decoded: how the
                // gathering's next leg, 0x4ac2d0, and this move, run by the
                // ship's events one a tick, combine; here the move aside
                // comes first.)
                if at_port {
                    self.off_the_berth(i, 91);
                }
            }
        }
    }

    /// Whether ship `i` unloads at `p` by the unloading order 0xc (0x4aab00
    /// with 0x4ac050): a port (class 0x39), or, for player 16, a
    /// barbarian structure of class 0x40.
    pub(super) fn unloads_at_port(&self, i: usize, p: usize) -> bool {
        match self.objects[p].class {
            ObjectClass::Building(PORT) => true,
            ObjectClass::Barbarian(0x40) => self.objects[i].owner == Some((4, 0)),
            _ => false,
        }
    }

    /// Sends ship `i` (gathering from `deposit`) to unload.
    /// The water cell in front of moorage or shipyard `p` (record field
    /// 10: 0 east, 2 north, 4 west, 6 south), where ships berth.
    pub(super) fn front_cell(&self, p: usize) -> Option<(i32, i32)> {
        let o = &self.objects[p];
        if !matches!(o.class, ObjectClass::Building(PORT | SHIPYARD | DOCK)) {
            return None;
        }
        let (cx, cy) = o.cell?;
        let (dx, dy) = match o.record.fields.get(10)? {
            0 => (1, 0),
            2 => (0, -1),
            4 => (-1, 0),
            6 => (0, 1),
            _ => return None,
        };
        self.nav
            .is_water(cx + dx, cy + dy)
            .then_some((cx + dx, cy + dy))
    }

    /// Object `b`'s footprint (corner cell and size) and how far out its
    /// nearest water lies: ships lie alongside it in that ring (1 for a
    /// building on the shore; up to 4 for one a little inland).
    fn berth_ring(&self, b: usize) -> (i32, i32, i32, i32) {
        let t = TILE_SIZE as f32;
        let other = &self.objects[b];
        let (x0, y0, n) = match (other.ship.is_none(), other.cell) {
            (true, Some((bx, by))) => (bx, by, building_cells(other)),
            _ => (
                (other.x / t).floor() as i32,
                (other.y / t).floor() as i32,
                1,
            ),
        };
        let ring = (1..=4)
            .find(|&r| {
                (y0 - r..=y0 + n - 1 + r)
                    .any(|y| (x0 - r..=x0 + n - 1 + r).any(|x| self.nav.is_water(x, y)))
            })
            .unwrap_or(1);
        (x0, y0, n, ring)
    }

    /// Whether ship `i` lies alongside object `b` (see `berth_ring`).
    pub(super) fn alongside(&self, i: usize, b: usize) -> bool {
        let t = TILE_SIZE as f32;
        let o = &self.objects[i];
        let (cx, cy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let (x0, y0, n, ring) = self.berth_ring(b);
        let dx = (x0 - cx).max(cx - (x0 + n - 1)).max(0);
        let dy = (y0 - cy).max(cy - (y0 + n - 1)).max(0);
        dx.max(dy) <= ring
    }

    /// A route for ship `i` to lie alongside object `b`: to the nearest
    /// water cell of its berth ring that the ship can reach.
    pub(super) fn route_beside(&self, i: usize, b: usize) -> Option<VecDeque<(f32, f32)>> {
        let from = (self.objects[i].x, self.objects[i].y);
        self.beside_cells(b, from)
            .into_iter()
            .find_map(|(x, y)| self.plan_route(i, x, y))
    }

    /// The water cells of object `b`'s berth ring, nearest `from` first,
    /// as world points.
    pub(super) fn beside_cells(&self, b: usize, from: (f32, f32)) -> Vec<(f32, f32)> {
        let t = TILE_SIZE as f32;
        let (x0, y0, n, r) = self.berth_ring(b);
        let mut cells = (y0 - r..=y0 + n - 1 + r)
            .flat_map(|y| (x0 - r..=x0 + n - 1 + r).map(move |x| (x, y)))
            .filter(|&(x, y)| self.nav.is_water(x, y))
            .map(|(x, y)| (x as f32 * t + t / 2.0, y as f32 * t + t / 2.0))
            .collect::<Vec<_>>();
        cells.sort_by(|a, b| {
            (a.0 - from.0)
                .hypot(a.1 - from.1)
                .total_cmp(&(b.0 - from.0).hypot(b.1 - from.1))
        });
        cells
    }

    /// Where a ship lies to unload at `p`: before a moorage's front, else at
    /// the building.
    pub(super) fn berth(&self, p: usize) -> (f32, f32) {
        let t = TILE_SIZE as f32;
        match self.front_cell(p) {
            Some((cx, cy)) => (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0),
            None => (self.objects[p].x, self.objects[p].y),
        }
    }

    /// Where ship `i` sails to work deposit `d`: for amber, the water cell
    /// straight beside it nearest the ship (so it can close in, 0x4afe50);
    /// else the deposit.
    pub(super) fn deposit_goal(&self, i: usize, d: usize) -> (f32, f32) {
        let t = TILE_SIZE as f32;
        let o = &self.objects[d];
        let amber = o.deposit.is_some_and(|dep| dep.resource == Resource::Amber);
        let cell = o
            .record
            .fields
            .get(6..8)
            .map(|f| (f[0] as i32, f[1] as i32));
        let (sx, sy) = (self.objects[i].x, self.objects[i].y);
        if let (true, Some((cx, cy))) = (amber, cell) {
            let best = [(1, 0), (0, -1), (-1, 0), (0, 1)]
                .iter()
                .map(|&(dx, dy)| (cx + dx, cy + dy))
                .filter(|&(x, y)| self.nav.is_water(x, y))
                .map(|(x, y)| (x as f32 * t + t / 2.0, y as f32 * t + t / 2.0))
                .min_by(|a, b| {
                    (a.0 - sx)
                        .hypot(a.1 - sy)
                        .total_cmp(&(b.0 - sx).hypot(b.1 - sy))
                });
            if let Some(p) = best {
                return p;
            }
        }
        (o.x, o.y)
    }

    /// How ship `i` closes in on the shore cell `target` to load amber or
    /// (`port`) to unload (0x4afe50): only from straight beside it, a set
    /// distance from its cell's middle towards it, turning to lie side-on
    /// (whichever way is nearer its heading). The point and that heading.
    pub(super) fn approach(
        &self,
        i: usize,
        target: (i32, i32),
        port: bool,
    ) -> Option<((f32, f32), usize)> {
        let t = TILE_SIZE as f32;
        let o = &self.objects[i];
        let ship = o.ship.as_ref()?;
        let (cx, cy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let dir = match (target.0 - cx, target.1 - cy) {
            (1, 0) => 0,
            (0, -1) => 2,
            (-1, 0) => 4,
            (0, 1) => 6,
            _ => return None,
        };
        let k = ship.kind as usize % 12;
        let dist = if port {
            // 0x59a7c8 across, 0x59a82c up and down.
            if dir % 4 == 0 {
                10
            } else {
                13
            }
        } else {
            // The shore's shape: its tile among the groups at 0x5bb870
            // (0x4b3770), then the distance by direction, type and group.
            let group = shore_group(&self.grid, target.0, target.1)?;
            APPROACH[dir / 2][k][group]
        };
        if dist == 0 {
            return None;
        }
        let (ux, uy) = [(1.0, 0.0), (0.0, -1.0), (-1.0, 0.0), (0.0, 1.0)][dir / 2];
        let (mx, my) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
        // Side-on: north or south to a shore east or west, east or west to
        // one north or south.
        let (a, b) = if dir % 4 == 0 { (2, 6) } else { (0, 4) };
        let turn = |h: usize| {
            let d = (h as i32 - ship.heading as i32).rem_euclid(8);
            d.min(8 - d)
        };
        let face = if turn(a) < turn(b) { a } else { b };
        Some(((mx + ux * dist as f32, my + uy * dist as f32), face))
    }

    /// Sends merchant `ship` to unload its goods at own port `port` (the
    /// original's unload pointer, mode 13: message 0x1f kind 1, event 0x40
    /// code 0xc), then back to what it was gathering. 0x4aab00 takes it
    /// only for a port of the ship's player (0x4ac050) not asleep; any
    /// other target drops it, the old task kept.
    pub fn order_unload(&mut self, ship: u32, port: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let p = self.object_index(port).ok_or("Choose one of your ports")?;
        let o = &self.objects[i];
        if !self.is_own(o) || o.ship.as_ref().is_none_or(|s| s.cargo < 1.0) {
            return Err("The ship has nothing to unload".into());
        }
        let b = &self.objects[p];
        if b.owner != o.owner || !self.takes_cargo(b) {
            return Err("Choose one of your ports".into());
        }
        let deposit = match o.ship.as_ref().and_then(|s| s.task) {
            Some(Task::Gather { deposit, .. }) => deposit,
            _ => 0,
        };
        let (bx, by) = self.berth(p);
        let path = self
            .plan_route(i, bx, by)
            .ok_or("The ship cannot reach it")?;
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path = path;
        ship.task = Some(Task::Gather {
            deposit,
            phase: GatherPhase::ToPort(port),
        });
        // Unloading at a port goes out with 75 and is raised by one
        // (0x4d74a0's 0x1f, 0x4aac43).
        ship.order_tag = Task::Gather {
            deposit,
            phase: GatherPhase::ToPort(port),
        }
        .tag();
        ship.priority = 76;
        Ok(())
    }

    /// (In the original a full or done amber or fish ship ends its task
    /// and posts notice 0x138 to its group, whose gather zone (0x4c9e30
    /// -> 0x4c9ae0) sends it to another deposit or, by command 0x1f, to
    /// its own port or the zone's pick 0x4c6640. Not ported: that pick
    /// and the zone's deposit choice; the port sends the ship to the
    /// nearest port here, as only the leviathan's 0x4ad510 does.)
    pub(super) fn send_to_port(&mut self, i: usize, deposit: u32) {
        let route = self.unloading_site(i).and_then(|port| {
            let p = self.object_index(port)?;
            let (bx, by) = self.berth(p);
            Some((port, self.plan_route(i, bx, by)?))
        });
        let ship = self.objects[i].ship.as_mut().unwrap();
        match route {
            Some((port, path)) => {
                ship.path = path;
                ship.task = Some(Task::Gather {
                    deposit,
                    phase: GatherPhase::ToPort(port),
                });
            }
            None => ship.task = None,
        }
    }

    /// Orders merchant `ship` to sail to the land cell `(cx, cy)` and found a
    /// settlement there.
    /// Sends `ship` to put up a settlement or fort (`class`) at a land
    /// cell: any ship may (the ships' building page, 0x4e6980 page 2).
    pub fn order_found(&mut self, ship: u32, cx: i32, cy: i32, class: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        if self.objects[i].ship.is_none() {
            return Err("Only ships can found settlements".into());
        }
        if !(class == SETTLEMENT || (0x34..=0x36).contains(&class)) {
            return Err("Ships found settlements and forts".into());
        }
        let owner = self.objects[i].owner.ok_or("No owner")?;
        let human = self.human.is_some_and(|h| (h.nation, h.slot) == owner);
        let cost = if human && (0x34..=0x36).contains(&class) {
            self.fort_price(self.home_settlement(i), class)
                .ok_or("Its weapons cannot be had yet")?
                .amber
        } else {
            economy::building_cost(class)
        };
        if self.funds(owner).0 < cost {
            return Err("Not enough amber".into());
        }
        self.check_ground(class, cx, cy)?;
        let t = TILE_SIZE as f32;
        let path = self
            .plan_route(i, cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0)
            .ok_or("The ship cannot sail there")?;
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path = path;
        ship.task = Some(Task::Found { cx, cy, class });
        Ok(())
    }
}

/// How many cells across a building stands on: settlements and towns two.
pub(super) fn building_cells(o: &MapObject) -> i32 {
    match o.class {
        ObjectClass::Building(SETTLEMENT | TOWN) => 2,
        _ => 1,
    }
}

/// The group (`SHORE_GROUPS`) of the shore tile on cell `(x, y)` (0x4b3770).
pub(super) fn shore_group(grid: &openconquest_formats::MapGrid, x: i32, y: i32) -> Option<usize> {
    if x < 0 || y < 0 || x as u32 >= grid.width || y as u32 >= grid.height {
        return None;
    }
    let cell = grid.get(0, x as u32, y as u32);
    SHORE_GROUPS.iter().position(|g| g.contains(&cell.variant))
}

/// Shore tiles (layer 0 variants of island ground) in the groups the
/// approach distances go by (0x5bb870).
const SHORE_GROUPS: [&[u8]; 12] = [
    &[2, 3, 4, 5],
    &[6],
    &[7, 8, 9, 10],
    &[11],
    &[12, 13, 14, 15],
    &[16],
    &[17, 18, 19, 20],
    &[21],
    &[30, 31],
    &[32, 33],
    &[34, 35, 36],
    &[37, 38, 39, 40, 41],
];

/// How far a ship closes in on amber from its cell's middle, by direction
/// (east 0x599c44, north 0x59a1c8, west 0x599ee8, south 0x59a4c8), ship
/// type and shore group; 0 where it does not.
const APPROACH: [[[u8; 12]; 12]; 4] = [
    [
        [16, 16, 0, 0, 33, 33, 0, 0, 27, 0, 0, 0],
        [20, 20, 0, 0, 38, 38, 0, 0, 30, 0, 0, 0],
        [16, 16, 0, 0, 33, 33, 0, 0, 25, 0, 0, 0],
        [18, 18, 0, 0, 35, 35, 0, 0, 25, 0, 0, 0],
        [15, 15, 0, 0, 35, 35, 0, 0, 25, 0, 0, 0],
        [19, 19, 0, 0, 33, 33, 0, 0, 25, 0, 0, 0],
        [18, 18, 0, 0, 35, 35, 0, 0, 25, 0, 0, 0],
        [18, 18, 0, 0, 35, 35, 0, 0, 25, 0, 0, 0],
        [18, 18, 0, 0, 35, 35, 0, 0, 25, 0, 0, 0],
        [18, 18, 0, 0, 35, 35, 0, 0, 25, 0, 0, 0],
        [18, 18, 0, 0, 35, 35, 0, 0, 25, 0, 0, 0],
        [18, 18, 0, 0, 35, 35, 0, 0, 25, 0, 0, 0],
    ],
    [
        [0, 0, 0, 0, 19, 19, 19, 19, 0, 0, 0, 19],
        [0, 0, 0, 0, 18, 18, 18, 18, 0, 0, 0, 18],
        [0, 0, 0, 0, 20, 20, 20, 20, 0, 0, 0, 20],
        [0, 0, 0, 0, 20, 20, 20, 20, 0, 0, 0, 20],
        [0, 0, 0, 0, 14, 14, 14, 14, 0, 0, 0, 14],
        [0, 0, 0, 0, 17, 17, 17, 17, 0, 0, 0, 17],
        [0, 0, 0, 0, 17, 17, 17, 17, 0, 0, 0, 17],
        [0, 0, 0, 0, 17, 17, 17, 17, 0, 0, 0, 17],
        [0, 0, 0, 0, 17, 17, 17, 17, 0, 0, 0, 17],
        [0, 0, 0, 0, 17, 17, 17, 17, 0, 0, 0, 17],
        [0, 0, 0, 0, 17, 17, 17, 17, 0, 0, 0, 17],
        [0, 0, 0, 0, 17, 17, 17, 17, 0, 0, 0, 17],
    ],
    [
        [0, 0, 16, 16, 0, 0, 33, 33, 0, 16, 0, 0],
        [0, 0, 20, 20, 0, 0, 38, 38, 0, 20, 0, 0],
        [0, 0, 16, 16, 0, 0, 33, 33, 0, 16, 0, 0],
        [0, 0, 18, 18, 0, 0, 35, 35, 0, 18, 0, 0],
        [0, 0, 15, 15, 0, 0, 35, 35, 0, 15, 0, 0],
        [0, 0, 19, 19, 0, 0, 33, 33, 0, 19, 0, 0],
        [0, 0, 18, 18, 0, 0, 35, 35, 0, 18, 0, 0],
        [0, 0, 18, 18, 0, 0, 35, 35, 0, 18, 0, 0],
        [0, 0, 18, 18, 0, 0, 35, 35, 0, 18, 0, 0],
        [0, 0, 18, 18, 0, 0, 35, 35, 0, 18, 0, 0],
        [0, 0, 18, 18, 0, 0, 35, 35, 0, 18, 0, 0],
        [0, 0, 18, 18, 0, 0, 35, 35, 0, 18, 0, 0],
    ],
    [
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
        [23, 23, 23, 23, 0, 0, 0, 0, 0, 0, 23, 0],
        [21, 20, 20, 20, 0, 0, 0, 0, 0, 0, 20, 0],
        [21, 21, 21, 21, 0, 0, 0, 0, 0, 0, 21, 0],
    ],
];
