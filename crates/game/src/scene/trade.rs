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

/// Seconds a frame of a building collapsing into ruins. An estimate.
const RUINS_FRAME_SECONDS: f32 = 0.1;

/// Goods a ship takes aboard each time (0x4ad0e0): amber 100 for boats and
/// merchants (20 for other ships), fish by ship type from 0x59b2a8 (boat
/// 33, merchant 50, others 20); the barbarians take a third. Amber comes
/// once per round of the porters, fish once per cast of the net.
fn take_per_round(resource: Resource, kind: u8, barbarian: bool) -> f32 {
    let n = match (resource, kind) {
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

    /// Whether player `owner` can order a ship of type `kind` now.
    pub fn can_build_ship_for(&self, owner: (u32, u32), kind: u8) -> Result<(), String> {
        if Some(owner) == self.human.map(|h| (h.nation, h.slot)) && !self.permits.ship(kind) {
            return Err(format!("{} cannot be built yet", SHIP_NAMES[kind as usize]));
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

    pub fn production_at(&self, site: u32) -> Option<&Production> {
        self.productions.iter().find(|p| p.site == site)
    }

    /// Starts building a ship of type `kind` at shipyard `site`.
    pub fn start_ship(&mut self, site: u32, kind: u8) -> Result<(), String> {
        if self.production_at(site).is_some() {
            return Err("The shipyard is busy".into());
        }
        let owner = self
            .object_index(site)
            .and_then(|i| self.objects[i].owner)
            .ok_or("No such shipyard")?;
        self.can_build_ship_for(owner, kind)?;
        // The player's ships come armed as the Building page says; the
        // computer players' with their standard load.
        let human = Some(owner) == self.human.map(|h| (h.nation, h.slot));
        let (amber, fish, arming) = if human {
            let price = self.ship_price(site, kind);
            let (amber, fish) = (price.amber, price.fish);
            let (have_amber, have_fish) = self.funds(owner);
            if have_amber < amber || have_fish < fish {
                return Err(format!(
                    "Not enough resources: {amber} amber and {fish} fish needed"
                ));
            }
            if let Some(home) = price.home {
                self.garrisons.insert(home, price.stock_left);
            }
            (amber, fish, Some(price.arming))
        } else {
            let (amber, fish) = economy::ship_cost(kind);
            (amber, fish, None)
        };
        self.earn(owner, -(amber as i64), -(fish as i64));
        self.productions.push(Production {
            site,
            ship: kind,
            argo: false,
            elapsed: 0.0,
            total: economy::SHIP_BUILD_SECONDS,
            arming,
        });
        Ok(())
    }

    /// What a ship of type `kind` built at shipyard `site` costs the
    /// player, with its arming if "default arming" is on (0x43e5c0).
    pub fn ship_price(&self, site: u32, kind: u8) -> Price {
        let (hull_amber, hull_fish) = economy::ship_cost(kind);
        let home = self
            .object_index(site)
            .and_then(|i| self.home_settlement(i));
        let stock = home.map(|h| self.garrison(h)).unwrap_or_default();
        let k = kind as usize % 12;
        let mut price = if self.default_arming {
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

    /// Whether the player may have `item` made: it is permitted and they
    /// have a workshop (and a smithy for swordsmen), as the settlement's
    /// training asks.
    pub fn can_make(&self, item: u8) -> bool {
        let Some(h) = self.human else { return false };
        let have = self.buildings_of((h.nation, h.slot));
        let owns = |class: u32| have & (1 << (class - SETTLEMENT)) != 0;
        self.permits.item(item) && owns(WORKSHOP) && (item != 2 || owns(economy::SMITHY))
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
        let fitted = |upgrade: combat::Upgrade, (level, fallback): (u8, u8)| {
            let can = |l: u8| l > 0 && levels.get(upgrade) >= l && self.permits.fit(upgrade, l);
            // A level-2 fit without a fallback falls back to level 1.
            let fallback = if level == 2 && fallback == 0 {
                1
            } else {
                fallback
            };
            [level, fallback].into_iter().find(|&l| can(l))
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
    const ROYAL_SHIP: u8 = 2;

    /// Starts building the Argo at shipyard `site` (0x7d): a royal ship,
    /// once Argus has come to the player's town (permission bit 80, which
    /// building it clears: there is one Argo).
    pub fn start_argo(&mut self, site: u32) -> Result<(), String> {
        if !self.permits.argo() {
            return Err("Argus must come to your town first".into());
        }
        self.start_ship(site, Self::ROYAL_SHIP)?;
        if let Some(p) = self.productions.iter_mut().find(|p| p.site == site) {
            p.argo = true;
        }
        self.permits.set(crate::permits::ARGO, false);
        Ok(())
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
        let argo = self.productions.iter().any(|p| p.argo)
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
        if self.production_at(id).is_some() {
            return Err("The shipyard is busy".into());
        }
        let cost = economy::building_cost(to);
        if self.amber < cost {
            return Err(format!("Not enough amber: {cost} needed"));
        }
        let mut rec = self.objects[i].record.clone();
        rec.class_id = to;
        rec.fields[0] = to;
        let obj = objects::resolve(data, caches, &rec)
            .map_err(|e| e.to_string())?
            .ok_or("Missing upgrade graphics")?;
        self.amber -= cost;
        self.objects[i] = obj;
        Ok(())
    }

    /// Creates a new ship of the human player next to building `site`.
    /// Keeps a hull-under-construction animation beside every shipyard that
    /// is building, its frame following the work done.
    pub(super) fn show_productions(&mut self, data: &GameData, caches: &mut Caches) {
        let building: Vec<u32> = self.productions.iter().map(|p| p.site).collect();
        self.effects
            .retain(|fx| fx.site.is_none_or(|s| building.contains(&s)));
        let t = TILE_SIZE as f32;
        for p in self.productions.clone() {
            let Some(i) = self.object_index(p.site) else {
                continue;
            };
            // Lying along the shore: across for yards facing east or west.
            let facing = self.objects[i].record.fields.get(10).copied().unwrap_or(0);
            let base = if facing % 4 == 0 { 0 } else { 40 };
            let frame = base + ((p.elapsed / p.total).clamp(0.0, 1.0) * 39.0) as usize;
            if let Some(fx) = self.effects.iter_mut().find(|fx| fx.site == Some(p.site)) {
                fx.show_frame(frame);
                continue;
            }
            let Some((cx, cy)) = self.launch_spot(i) else {
                continue;
            };
            let nation = self.objects[i].owner.map_or(1, |o| o.0);
            let Ok(frames) = objects::ship_mode_frames(data, caches, p.ship, nation, 2) else {
                continue;
            };
            if frames.len() < 80 {
                continue;
            }
            let mut fx = Effect {
                x: cx as f32 * t + t / 2.0,
                y: cy as f32 * t + t / 2.0,
                frames,
                age_ms: 0.0,
                follow: None,
                life_ms: None,
                drift: false,
                site: Some(p.site),
            };
            fx.show_frame(frame);
            self.effects.push(fx);
        }
    }

    /// Where a ship built at the yard `i` goes into the water: the nearest
    /// water cell not already holding a ship.
    pub(super) fn launch_spot(&self, i: usize) -> Option<(i32, i32)> {
        let t = TILE_SIZE as f32;
        let (cx, cy) = (
            (self.objects[i].x / t) as i32,
            (self.objects[i].y / t) as i32,
        );
        let occupied: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| o.ship.is_some())
            .map(|o| ((o.x / t) as i32, (o.y / t) as i32))
            .collect();
        for r in 0..8 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let c = (cx + dx, cy + dy);
                    if (dx.abs() == r || dy.abs() == r)
                        && self.nav.is_water(c.0, c.1)
                        && !occupied.contains(&c)
                    {
                        return Some(c);
                    }
                }
            }
        }
        self.nav.nearest_water(cx, cy)
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

    pub(super) fn launch_ship(
        &mut self,
        site: u32,
        kind: u8,
        arming: Option<&Arming>,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<Option<u32>> {
        let Some(i) = self.object_index(site) else {
            return Ok(None);
        };
        let Some((nation, slot)) = self.objects[i].owner else {
            return Ok(None);
        };
        let Some((x, y)) = self.launch_spot(i) else {
            return Ok(None);
        };
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 13];
        fields[0] = 0xc8;
        fields[1] = 1;
        fields[2] = nation;
        fields[3] = slot;
        fields[5] = nation;
        fields[6] = slot;
        fields[7] = kind as u32;
        fields[8] = u32::MAX;
        fields[9] = x as u32;
        fields[10] = y as u32;
        fields[11] = 6; // facing south
        fields[12] = 100;
        let rec = ObjectRecord {
            id,
            class_id: 0xc8,
            fields,
        };
        if let Some(mut obj) = objects::resolve(data, caches, &rec)? {
            // Armed as ordered; boats and merchants keep their one item.
            if let (Some(a), Some(ship), Some(fight)) =
                (arming, obj.ship.as_mut(), obj.combat.as_mut())
            {
                let mut squads = [0; 3];
                for &w in a.items.iter().filter(|&&w| w < 3) {
                    squads[w as usize] += 1;
                }
                ship.crew.squads = squads;
                if combat::carries_weapons(kind) && kind > 1 {
                    fight.weapons = a.items.iter().copied().filter(|&w| w >= 3).collect();
                }
                ship.armour = a.armour;
                ship.ram = a.ram;
            }
            self.objects.push(obj);
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            return Ok(Some(id));
        }
        Ok(None)
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
        let level = self
            .levels_of(self.human.map(|h| (h.nation, h.slot)))
            .get(upgrade);
        let pending = self.research.iter().any(|r| r.1 == upgrade);
        (level < upgrade.max_level() && !pending && self.permits.research(upgrade, level))
            .then_some(level)
    }

    /// Starts studying `upgrade` at sage school `site`.
    pub fn start_research(&mut self, site: u32, upgrade: combat::Upgrade) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such building")?;
        // The workshop improves arms; the sage school everything else.
        let (class, building) = match upgrade {
            combat::Upgrade::Arms(_) | combat::Upgrade::Rate(_) | combat::Upgrade::Reach(_) => {
                (WORKSHOP, "workshop")
            }
            _ => (economy::SCHOOL, "sage school"),
        };
        if self.objects[i].class != ObjectClass::Building(class) || !self.is_own(&self.objects[i]) {
            return Err(format!("Only your {building} can do that"));
        }
        if self.construction_at(site).is_some() {
            return Err("It is still being built".into());
        }
        if self.research.iter().any(|r| r.0 == site) {
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
        let reach = combat::LOAD_REACH * TILE_SIZE as f32;
        self.objects
            .iter()
            .enumerate()
            .filter(|(_, o)| self.is_own(o))
            .filter_map(|(i, o)| {
                let ship = o.ship.as_ref()?;
                let has = if upgrade == combat::Upgrade::Armour {
                    ship.armour
                } else {
                    ship.ram
                };
                let d = ((o.x - sx).powi(2) + (o.y - sy).powi(2)).sqrt();
                (has < level && d <= reach).then_some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Refits the nearest own ship at smithy `smithy` with armour or ram of
    /// `level`, up to what the sage school has studied.
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
        let i = self
            .refit_candidate(smithy, upgrade, level)
            .ok_or("No ship of yours lies next to the smithy needing it")?;
        let kind = self.objects[i].ship.as_ref().unwrap().kind;
        let cost = combat::refit_cost(upgrade, level, kind);
        if self.amber < cost {
            return Err(format!("Not enough amber: {cost} needed"));
        }
        self.amber -= cost;
        let ship = self.objects[i].ship.as_mut().unwrap();
        if upgrade == combat::Upgrade::Armour {
            ship.armour = level;
        } else {
            ship.ram = level;
        }
        self.sounds
            .push(("bild6", self.objects[i].x, self.objects[i].y));
        Ok(())
    }

    /// Research at `site`: the upgrade and its progress 0..1.
    pub fn research_at(&self, site: u32) -> Option<(combat::Upgrade, f32)> {
        self.research
            .iter()
            .find(|r| r.0 == site)
            .map(|r| (r.1, r.3 / r.1.seconds(r.2.saturating_sub(1))))
    }

    pub(super) fn run_research(&mut self, dt: f32) {
        let Some(human) = self.human.map(|h| (h.nation, h.slot)) else {
            return;
        };
        let alive: Vec<u32> = self.objects.iter().map(|o| o.id).collect();
        self.research.retain(|r| alive.contains(&r.0));
        let mut done = Vec::new();
        for r in &mut self.research {
            r.3 += dt;
            if r.3 >= r.1.seconds(r.2.saturating_sub(1)) {
                done.push((r.1, r.2));
            }
        }
        self.research
            .retain(|r| r.3 < r.1.seconds(r.2.saturating_sub(1)));
        for (upgrade, level) in done {
            self.levels.entry(human).or_default().set(upgrade, level);
            self.messages
                .push(format!("Better {} (level {level})", upgrade.name()));
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
            Some(t) if t.kind != item => return Err("The building is busy with other work".into()),
            Some(t) => t.queued += 1,
            None => self.trainings.push(Training {
                site,
                kind: item,
                queued: 1,
                elapsed: 0.0,
            }),
        }
        self.amber -= amber;
        self.fish -= fish;
        Ok(())
    }

    /// Ship `ship` takes on the marines and weapons waiting at own
    /// settlements, towns and workshops nearby, as many as it has room for.
    /// Returns how many items came aboard.
    /// The player's ship nearest to `site` within loading reach.
    pub fn ship_alongside(&self, site: u32) -> Option<u32> {
        let t = TILE_SIZE as f32;
        let s = self.objects.iter().find(|o| o.id == site)?;
        let dist = |o: &MapObject| ((o.x - s.x).powi(2) + (o.y - s.y).powi(2)).sqrt();
        self.objects
            .iter()
            .filter(|o| self.is_own(o) && o.ship.is_some() && dist(o) <= combat::LOAD_REACH * t)
            .min_by(|a, b| dist(a).total_cmp(&dist(b)))
            .map(|o| o.id)
    }

    /// Puts one unit of `item` from `site`'s stock aboard `ship`, which must
    /// lie alongside and have room (the original's load cursor, c_load.cpp).
    pub fn load_item(&mut self, site: u32, ship: u32, item: u8) -> Result<(), String> {
        let t = TILE_SIZE as f32;
        let s = self.object_index(site).ok_or("No such settlement")?;
        let i = self.object_index(ship).ok_or("No such ship")?;
        if !self.is_own(&self.objects[i]) || self.objects[i].ship.is_none() {
            return Err("Choose one of your ships".into());
        }
        let (a, b) = (&self.objects[s], &self.objects[i]);
        if ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt() > combat::LOAD_REACH * t {
            return Err("The ship must lie next to the settlement".into());
        }
        let mut stock = self.garrison(site);
        if stock.get(item as usize).is_none_or(|&n| n == 0) {
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
        if item < 3 {
            ship.crew.squads[item as usize] += 1;
        } else {
            fight.weapons.push(item);
        }
        stock[item as usize] -= 1;
        self.garrisons.insert(site, stock);
        Ok(())
    }

    pub fn load_crew(&mut self, ship: u32) -> Result<u32, String> {
        let t = TILE_SIZE as f32;
        let i = self.object_index(ship).ok_or("No such ship")?;
        let (sx, sy) = (self.objects[i].x, self.objects[i].y);
        let sites: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                self.is_own(o)
                    && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN | WORKSHOP))
                    && ((o.x - sx).powi(2) + (o.y - sy).powi(2)).sqrt() <= combat::LOAD_REACH * t
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
            let mut stock = self.garrison(site);
            let obj = &mut self.objects[i];
            let crew = &mut obj.ship.as_mut().unwrap().crew;
            let weapons = &mut obj.combat.as_mut().unwrap().weapons;
            // The heaviest weapons first, then swordsmen, the strongest
            // boarders, and the other marines.
            let order = (3..9u8)
                .rev()
                .filter(|_| combat::carries_weapons(kind))
                .chain((0..3).rev());
            for item in order {
                let free = room.saturating_sub(combat::load(crew, weapons));
                let take = (free / combat::item_size(item)).min(stock[item as usize]);
                if item < 3 {
                    crew.squads[item as usize] += take;
                } else {
                    weapons.extend(std::iter::repeat_n(item, take as usize));
                }
                stock[item as usize] -= take;
                n += take;
            }
            self.garrisons.insert(site, stock);
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
        for t in &mut self.trainings {
            t.elapsed += dt;
            if t.elapsed >= combat::train_seconds(t.kind) {
                t.elapsed = 0.0;
                t.queued -= 1;
                self.garrisons.entry(t.site).or_default()[t.kind as usize] += 1;
            }
        }
        let alive: Vec<u32> = self.objects.iter().map(|o| o.id).collect();
        self.trainings
            .retain(|t| t.queued > 0 && alive.contains(&t.site));
    }

    /// The construction going on at `site`, if any.
    pub fn construction_at(&self, site: u32) -> Option<&Construction> {
        self.constructions.iter().find(|c| c.site == site)
    }

    /// Whether building class `class` can go up on cell `(cx, cy)` for the
    /// settlement `settlement`; on success, the facing (for ports and
    /// shipyards, towards the water).
    pub fn check_site(&self, origin: u32, class: u32, cx: i32, cy: i32) -> Result<u32, String> {
        let t = TILE_SIZE as f32;
        let i = self.object_index(origin).ok_or("No settlement")?;
        let home = &self.objects[i];
        // Settlements build around their own cells; ships found settlements
        // on the shore they lie at.
        let (x0, y0, x1, y1, reach, too_far) = match (&home.ship, home.cell) {
            (None, Some((hx, hy))) => {
                let size = building_cells(home);
                // Moorages and shipyards a cell further than the rest.
                let reach = if matches!(class, PORT | SHIPYARD | DOCK) {
                    BUILD_RADIUS
                } else {
                    BUILD_RADIUS - 1
                };
                (
                    hx,
                    hy,
                    hx + size - 1,
                    hy + size - 1,
                    reach,
                    "Too far from the settlement",
                )
            }
            _ => {
                let (hx, hy) = ((home.x / t).floor() as i32, (home.y / t).floor() as i32);
                (
                    hx,
                    hy,
                    hx,
                    hy,
                    FOUND_REACH,
                    "The ship must lie next to the site",
                )
            }
        };
        let dx = (x0 - cx).max(cx - x1).max(0);
        let dy = (y0 - cy).max(cy - y1).max(0);
        if dx.max(dy) > reach {
            return Err(too_far.into());
        }
        self.check_ground(class, cx, cy)
    }

    /// Whether building class `class` can stand on cell `(cx, cy)`, wherever
    /// it is ordered from; on success, the facing (see `check_site`). As
    /// probed on the original: a free land cell (trees and rocks do not
    /// matter); moorages and shipyards on the shore, water beside them;
    /// other buildings inland, land on all four sides.
    pub fn check_ground(&self, class: u32, cx: i32, cy: i32) -> Result<u32, String> {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        if cx < 0 || cy < 0 || cx >= w || cy >= h {
            return Err("Off the map".into());
        }
        if self.nav.is_water(cx, cy) {
            return Err("Buildings go on land".into());
        }
        let taken = self.objects.iter().any(|o| {
            let Some((ox, oy)) = o
                .cell
                .filter(|_| matches!(o.class, ObjectClass::Building(_)))
            else {
                return false;
            };
            let size = building_cells(o);
            (ox..ox + size).contains(&cx) && (oy..oy + size).contains(&cy)
        });
        if taken {
            return Err("There is no room there".into());
        }
        let sides = [(0, (1, 0)), (2, (0, -1)), (4, (-1, 0)), (6, (0, 1))];
        let inside = |x: i32, y: i32| (0..w).contains(&x) && (0..h).contains(&y);
        if !matches!(class, PORT | SHIPYARD | DOCK) {
            // Beyond the map's edge counts as land.
            let inland = sides.iter().all(|&(_, (dx, dy))| {
                !inside(cx + dx, cy + dy) || !self.nav.is_water(cx + dx, cy + dy)
            });
            return if inland {
                Ok(0)
            } else {
                Err("This must be built inland, away from the shore".into())
            };
        }
        // Facing 0 = water to the east, 2 north, 4 west, 6 south.
        sides
            .into_iter()
            .find(|&(_, (dx, dy))| self.nav.is_water(cx + dx, cy + dy))
            .map(|(facing, _)| facing)
            .ok_or_else(|| "Moorages and shipyards must be built on the shore".into())
    }

    /// Starts putting up building `class` on cell `(cx, cy)` near settlement
    /// `settlement`, paying for it.
    pub fn place_building(
        &mut self,
        settlement: u32,
        class: u32,
        cx: i32,
        cy: i32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        let human = self.human.ok_or("No player")?;
        // A fort comes armed (default arming), its weapons paid for or
        // taken from the nearest settlement's stock.
        let fort = if (0x34..=0x36).contains(&class) {
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
        if self.amber < cost || self.fish < fish {
            return Err("Not enough resources".into());
        }
        let facing = self.check_site(settlement, class, cx, cy)?;
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
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
        self.amber -= cost;
        self.fish -= fish;
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
        self.constructions.push(Construction {
            site: id,
            elapsed: 0.0,
            total: economy::BUILDING_SECONDS,
        });
        Ok(())
    }

    /// Advances constructions; finished buildings get their standing look.
    pub(super) fn run_constructions(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let mut done = Vec::new();
        for c in &mut self.constructions {
            c.elapsed += dt;
            let Some(i) = self.objects.iter().position(|o| o.id == c.site) else {
                continue;
            };
            self.objects[i].progress = Some((c.elapsed / c.total).min(1.0));
            if let Some(h) = self.objects[i].combat.as_mut() {
                // Hull grows with the work, from a tenth to whole.
                h.hull = (h.hull + h.max_hull * 0.9 * dt / c.total).min(h.max_hull);
            }
            if c.elapsed >= c.total {
                done.push(c.site);
            }
        }
        self.constructions
            .retain(|c| c.elapsed < c.total && self.objects.iter().any(|o| o.id == c.site));
        for id in done {
            self.refresh_building(id, data, caches);
            if let Some(i) = self.object_index(id) {
                self.sounds
                    .push(("bild6", self.objects[i].x, self.objects[i].y));
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
            Ok(frames) if !frames.is_empty() => self.objects[i].frames = frames,
            Ok(_) => {}
            Err(e) => eprintln!("could not load building frames: {e:#}"),
        }
        // A damaged building's frames show it ever more broken: the frame
        // follows its condition within the third (intact ones animate).
        self.objects[i].progress = match state {
            BuildingState::Standing(level @ (1 | 2)) => {
                let top = level as f32 * 100.0 / 3.0;
                Some(((top - percent) * 3.0 / 100.0).clamp(0.0, 1.0))
            }
            _ => None,
        };
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
                *p = (*p + dt / (n * RUINS_FRAME_SECONDS)).min(1.0);
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

    /// Player `owner`'s building nearest to `(x, y)` where cargo can be
    /// unloaded: a port, or failing that a settlement or town.
    pub(super) fn unloading_site(&self, owner: Option<(u32, u32)>, x: f32, y: f32) -> Option<u32> {
        let dist = |o: &MapObject| (o.x - x).powi(2) + (o.y - y).powi(2);
        let nearest = |classes: &[u32]| {
            self.objects
                .iter()
                .filter(|o| {
                    matches!(o.class, ObjectClass::Building(c) if classes.contains(&c))
                        && owner.is_some()
                        && o.owner == owner
                })
                .min_by(|a, b| dist(a).total_cmp(&dist(b)))
                .map(|o| o.id)
        };
        nearest(&[PORT]).or_else(|| nearest(&[SETTLEMENT, TOWN]))
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
            if let Err(e) = self.place_building(id, class, cx, cy, data, caches) {
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
                    if self.shoal_gone(self.objects[d].id) {
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
                    && (self.shoal_gone(self.objects[d].id) || !near(&self.objects[d], 1.6))
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
                    Resource::Fish => Some(super::unload::Activity::Fishing),
                    Resource::Leviathan => None,
                };
                if let Some(a) = activity {
                    self.show_activity(i, deposit, a, data, caches);
                }
                let ship = self.objects[i].ship.as_ref().unwrap();
                // A leviathan is one catch, taken in a hunt.
                let (rate, capacity) = if dep.resource == Resource::Leviathan {
                    (1.0 / crate::objects::HUNT_SECONDS, 1.0)
                } else {
                    // Once the walkway is down or the net out, a load a
                    // round of its animation.
                    // Player 0x10: nation 4 (the barbarians), slot 0.
                    let barbarian = self.objects[i].owner == Some((4, 0));
                    let take = take_per_round(dep.resource, ship.kind, barbarian);
                    let rate = self
                        .planks
                        .iter()
                        .find(|p| p.ship == self.objects[i].id)
                        .and_then(|p| p.round_seconds())
                        .map_or(0.0, |s| take / s);
                    (rate, capacity)
                };
                let mut take = (rate * dt).min(capacity - ship.cargo);
                if let Some(left) = dep.remaining {
                    take = take.min(left);
                }
                let ship = self.objects[i].ship.as_mut().unwrap();
                ship.cargo += take;
                ship.cargo_kind = Some(dep.resource);
                let (full, cargo) = (ship.cargo >= capacity, ship.cargo);
                let mut exhausted = false;
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
                    // The picture shrinks as the amber runs out.
                    if thirds == Some(third(*left)) {
                        thirds = None;
                    }
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
                // At a moorage's front; at a town as close as the water
                // lets it come (it may stand a few cells inland).
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
                if self.object_index(port).is_none() {
                    return self.send_to_port(i, deposit);
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
                // A leviathan comes in whole, in a second; goods a slot a
                // laden porter trip (at the porters' pace where no walkway
                // shows).
                let before = if kind == Some(Resource::Leviathan) {
                    ship.cargo.round()
                } else {
                    ship.cargo
                };
                let after = if kind == Some(Resource::Leviathan) {
                    (before - dt).max(0.0)
                } else if walkway {
                    (before - trips * super::unload::TRIP_LOAD).max(0.0)
                } else {
                    (before - super::unload::UNLOAD_RATE * dt).max(0.0)
                };
                ship.cargo = after;
                let came = (before.floor() - after.floor()) as i64;
                if let (Some(owner), true) = (self.objects[i].owner, came > 0) {
                    match kind {
                        Some(Resource::Amber) => self.earn(owner, came, 0),
                        Some(Resource::Fish) => self.earn(owner, 0, came),
                        Some(Resource::Leviathan) => {
                            let (amber, fish) = crate::objects::LEVIATHAN_WORTH;
                            self.earn(owner, amber * came, fish * came)
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
                let ship = self.objects[i].ship.as_mut().unwrap();
                ship.cargo = 0.0;
                ship.cargo_kind = None;
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
            }
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

    pub(super) fn send_to_port(&mut self, i: usize, deposit: u32) {
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        let owner = self.objects[i].owner;
        let route = self.unloading_site(owner, x, y).and_then(|port| {
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
        let cost = if (0x34..=0x36).contains(&class) {
            self.fort_price(self.home_settlement(i), class)
                .ok_or("Its weapons cannot be had yet")?
                .amber
        } else {
            economy::building_cost(class)
        };
        if self.amber < cost {
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
fn building_cells(o: &MapObject) -> i32 {
    match o.class {
        ObjectClass::Building(SETTLEMENT | TOWN) => 2,
        _ => 1,
    }
}

/// The group (`SHORE_GROUPS`) of the shore tile on cell `(x, y)` (0x4b3770).
pub(super) fn shore_group(grid: &openac_formats::MapGrid, x: i32, y: i32) -> Option<usize> {
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
