//! Ships, buildings, marines, research and trade.

use super::*;

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
        let (amber, fish) = economy::ship_cost(kind);
        self.earn(owner, -(amber as i64), -(fish as i64));
        self.productions.push(Production {
            site,
            ship: kind,
            argo: false,
            elapsed: 0.0,
            total: economy::SHIP_BUILD_SECONDS,
        });
        Ok(())
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
        if let Some(obj) = objects::resolve(data, caches, &rec)? {
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
        let (hx, hy) = ((home.x / t).floor() as i32, (home.y / t).floor() as i32);
        // Settlements build around them; ships found settlements on the shore
        // they lie at.
        let (reach, too_far) = if home.ship.is_some() {
            (FOUND_REACH, "The ship must lie next to the site")
        } else {
            (BUILD_RADIUS, "Too far from the settlement")
        };
        if (cx - hx).abs().max((cy - hy).abs()) > reach {
            return Err(too_far.into());
        }
        self.check_ground(class, cx, cy)
    }

    /// Whether building class `class` can stand on cell `(cx, cy)`, wherever
    /// it is ordered from; on success, the facing (see `check_site`).
    pub fn check_ground(&self, class: u32, cx: i32, cy: i32) -> Result<u32, String> {
        let t = TILE_SIZE as f32;
        // Sprites are taller than a cell: keep off the map's border.
        if cx < 1 || cy < 1 || cx >= self.nav.width as i32 - 1 || cy >= self.nav.height as i32 - 1 {
            return Err("Too close to the edge of the map".into());
        }
        if self.nav.is_water(cx, cy) {
            return Err("Buildings go on land".into());
        }
        let crowded = self.objects.iter().any(|o| {
            let structure = !matches!(
                o.class,
                ObjectClass::Ship
                    | ObjectClass::Amber
                    | ObjectClass::FishZone
                    | ObjectClass::WhaleZone
                    | ObjectClass::Other(_)
            );
            let (ox, oy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
            structure && (ox - cx).abs() <= 1 && (oy - cy).abs() <= 1
        });
        if crowded {
            return Err("There is no room there".into());
        }
        if !matches!(class, PORT | SHIPYARD) {
            return Ok(0);
        }
        // Facing 0 = water to the east, 2 north, 4 west, 6 south.
        [(0, (1, 0)), (2, (0, -1)), (4, (-1, 0)), (6, (0, 1))]
            .into_iter()
            .find(|&(_, (dx, dy))| self.nav.is_water(cx + dx, cy + dy))
            .map(|(facing, _)| facing)
            .ok_or_else(|| "Ports and shipyards must be built on the shore".into())
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
        let cost = economy::building_cost(class);
        if self.amber < cost {
            return Err("Not enough amber".into());
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
        let state = BuildingState::for_condition(c.hull / c.max_hull * 100.0);
        match objects::building_frames(data, caches, &obj.record, state) {
            Ok(frames) if !frames.is_empty() => self.objects[i].frames = frames,
            Ok(_) => {}
            Err(e) => eprintln!("could not load building frames: {e:#}"),
        }
        self.objects[i].progress = None;
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
        let (x, y) = (self.objects[d].x, self.objects[d].y);
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
        if let Some(Task::Found { cx, cy }) = self.objects[i].ship.as_ref().and_then(|s| s.task) {
            if !self.objects[i].ship.as_ref().unwrap().path.is_empty() {
                return;
            }
            self.objects[i].ship.as_mut().unwrap().task = None;
            let id = self.objects[i].id;
            if let Err(e) = self.place_building(id, SETTLEMENT, cx, cy, data, caches) {
                self.messages
                    .push(format!("Cannot found the settlement: {e}"));
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
                // As close as the water lets it come: a town may stand a
                // few cells inland.
                let reach = self.landing_reach(p).max(2.6);
                if !near(&self.objects[p], reach) {
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
                let ship = self.objects[i].ship.as_mut().unwrap();
                let kind = ship.cargo_kind;
                // A leviathan comes in whole, in a second; goods at the
                // porters' pace.
                let (before, rate) = if kind == Some(Resource::Leviathan) {
                    (ship.cargo.round(), 1.0)
                } else {
                    (ship.cargo, super::unload::UNLOAD_RATE)
                };
                let after = (before - rate * dt).max(0.0);
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
                let ship = self.objects[i].ship.as_mut().unwrap();
                ship.cargo = 0.0;
                ship.cargo_kind = None;
                match deposit_at {
                    Some(d) => {
                        let (x, y) = (self.objects[d].x, self.objects[d].y);
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
    pub(super) fn send_to_port(&mut self, i: usize, deposit: u32) {
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        let owner = self.objects[i].owner;
        let route = self.unloading_site(owner, x, y).and_then(|port| {
            let p = self.object_index(port)?;
            Some((
                port,
                self.plan_route(i, self.objects[p].x, self.objects[p].y)?,
            ))
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
    pub fn order_found(&mut self, ship: u32, cx: i32, cy: i32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        if self.objects[i]
            .ship
            .as_ref()
            .is_none_or(|s| cargo_capacity(s.kind) <= 0.0)
        {
            return Err("Only merchant ships can found settlements".into());
        }
        if self.amber < economy::building_cost(SETTLEMENT) {
            return Err("Not enough amber".into());
        }
        self.check_ground(SETTLEMENT, cx, cy)?;
        let t = TILE_SIZE as f32;
        let path = self
            .plan_route(i, cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0)
            .ok_or("The ship cannot sail there")?;
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path = path;
        ship.task = Some(Task::Found { cx, cy });
        Ok(())
    }
}
