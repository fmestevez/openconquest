//! The currently loaded mission.

use anyhow::Result;
use openac_formats::map::TILE_SIZE;
use openac_formats::{players, MapGrid, ObjectClass, ObjectRecord, PlayerInfo};

use crate::assets::GameData;
use crate::combat::{self, Effect, Rng};
use crate::economy::{self, DOCK, PORT, SETTLEMENT, SHIPYARD, TOWN};
use crate::magic::{self, Magic, Spell, Temple};
use crate::nav::NavGrid;
use crate::objects::{
    self, cargo_capacity, BuildingState, Caches, GatherPhase, MapObject, Resource, Task, SHIP_NAMES,
};
use crate::weather::Wind;
use std::collections::VecDeque;

/// Units of goods a merchant gathers per second. Placeholder: the original
/// rate has not been recovered yet.
const GATHER_RATE: f32 = 50.0;
use crate::render::palette_lut;
use crate::terrain::Terrain;

/// Marines being trained at a settlement, one at a time.
#[derive(Debug, Clone)]
pub struct Training {
    pub site: u32,
    /// Crew type (see `combat::CREW_NAMES`).
    pub kind: u8,
    /// Marines ordered and not yet trained.
    pub queued: u32,
    /// Seconds spent on the current one.
    pub elapsed: f32,
}

impl Training {
    /// Progress 0..1 of the marine in training.
    pub fn progress(&self) -> f32 {
        (self.elapsed / combat::CREW_TRAIN_SECONDS).min(1.0)
    }
}

/// A building going up.
#[derive(Debug, Clone)]
pub struct Construction {
    pub site: u32,
    pub elapsed: f32,
    pub total: f32,
}

/// How far from the settlement that orders it a building may go, in cells.
/// Placeholder.
pub const BUILD_RADIUS: i32 = 6;
/// How close a ship must lie to the site of the settlement it founds.
pub const FOUND_REACH: i32 = 2;

/// A ship under construction at a shipyard.
#[derive(Debug, Clone)]
pub struct Production {
    pub site: u32,
    pub ship: u8,
    pub elapsed: f32,
    pub total: f32,
}

pub struct Scene {
    pub mission: String,
    pub grid: MapGrid,
    pub terrain: Terrain,
    pub objects: Vec<MapObject>,
    pub nav: NavGrid,
    /// The human player, if the mission defines one.
    pub human: Option<PlayerInfo>,
    /// The human player's current amber and fish.
    pub amber: u32,
    pub fish: u32,
    /// Ships being built.
    pub productions: Vec<Production>,
    /// Buildings going up.
    pub constructions: Vec<Construction>,
    /// Marines in training.
    pub trainings: Vec<Training>,
    /// Trained marines waiting at each settlement, by crew type.
    pub garrisons: std::collections::HashMap<u32, [u32; 3]>,
    /// News for the player (e.g. an order that could not be carried out).
    pub messages: Vec<String>,
    pub minimap: Vec<u32>,
    pub minimap_size: (u32, u32),
    /// Running one-shot animations (explosions, splashes).
    pub effects: Vec<Effect>,
    rng: Rng,
    /// Seconds until the computer players next decide.
    ai_timer: f32,
    pub wind: Wind,
    /// The human player's temple magic.
    pub magic: Magic,
}

impl Scene {
    pub fn load(data: &GameData, caches: &mut Caches, mission: &str) -> Result<Self> {
        let archive = data.open_mission(mission)?;
        let grid = data.load_map(&archive, mission)?;
        let terrain = Terrain::compose(data, &grid, &mut caches.tiles)?;
        if !terrain.missing_tiles.is_empty() {
            eprintln!(
                "warning: {mission}: missing tiles: {}",
                terrain.missing_tiles.join(", ")
            );
        }
        let objs = objects::load(data, caches, &archive)?;
        if !objs.unresolved.is_empty() {
            let mut classes: Vec<_> = objs.unresolved.iter().collect();
            classes.sort();
            let list: Vec<String> = classes.iter().map(|(c, n)| format!("{c:#x}x{n}")).collect();
            eprintln!(
                "{mission}: {} objects drawn; not drawn yet (class x count): {}",
                objs.drawn.len(),
                list.join(" ")
            );
        }
        let (mw, mh, minimap) = terrain.minimap(&palette_lut(data, 0));
        let nav = NavGrid::from_map(&grid);
        let players = players(&archive).unwrap_or_default();
        let human = players.iter().copied().find(|p| !p.computer);
        Ok(Self {
            mission: mission.to_string(),
            grid,
            terrain,
            objects: objs.drawn,
            nav,
            productions: Vec::new(),
            constructions: Vec::new(),
            messages: Vec::new(),
            trainings: Vec::new(),
            garrisons: Default::default(),
            amber: human.map_or(0, |p| p.amber),
            fish: human.map_or(0, |p| p.fish),
            human,
            minimap,
            minimap_size: (mw, mh),
            effects: Vec::new(),
            rng: Rng(0x1234_5678),
            ai_timer: 0.0,
            wind: Wind::new(0x5eed_0001),
            magic: Magic::default(),
        })
    }

    /// Whether the human player owns `obj`.
    pub fn is_own(&self, obj: &MapObject) -> bool {
        match (self.human, obj.owner) {
            (Some(h), Some(owner)) => (h.nation, h.slot) == owner,
            _ => false,
        }
    }

    /// Standing buildings the human player owns, as a bit mask over building
    /// classes (bit n = class 0x32 + n). A town also counts as a settlement,
    /// a dock as a shipyard.
    pub fn owned_buildings(&self) -> u32 {
        let mut mask = 0;
        for obj in &self.objects {
            if let ObjectClass::Building(cls) = obj.class {
                let standing = self.construction_at(obj.id).is_none();
                if self.is_own(obj) && standing && (SETTLEMENT..=0x3e).contains(&cls) {
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
        let need = economy::ship_requirements(kind) | 1;
        if self.owned_buildings() & need != need {
            return Err(format!(
                "{} needs more buildings",
                SHIP_NAMES[kind as usize]
            ));
        }
        let (amber, fish) = economy::ship_cost(kind);
        if self.amber < amber || self.fish < fish {
            return Err(format!(
                "Not enough resources: {amber} amber and {fish} fish needed"
            ));
        }
        Ok(())
    }

    pub fn production_at(&self, site: u32) -> Option<&Production> {
        self.productions.iter().find(|p| p.site == site)
    }

    /// Starts building a ship of type `kind` at shipyard `site`.
    pub fn start_ship(&mut self, site: u32, kind: u8) -> Result<(), String> {
        if self.production_at(site).is_some() {
            return Err("The shipyard is busy".into());
        }
        self.can_build_ship(kind)?;
        let (amber, fish) = economy::ship_cost(kind);
        self.amber -= amber;
        self.fish -= fish;
        self.productions.push(Production {
            site,
            ship: kind,
            elapsed: 0.0,
            total: economy::SHIP_BUILD_SECONDS,
        });
        Ok(())
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
    fn launch_ship(
        &mut self,
        site: u32,
        kind: u8,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<()> {
        let (Some(i), Some(human)) = (self.object_index(site), self.human) else {
            return Ok(());
        };
        let t = TILE_SIZE as f32;
        let (cx, cy) = (
            (self.objects[i].x / t) as i32,
            (self.objects[i].y / t) as i32,
        );
        // The nearest water cell not already holding a ship.
        let occupied: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| o.ship.is_some())
            .map(|o| ((o.x / t) as i32, (o.y / t) as i32))
            .collect();
        let mut spot = None;
        'search: for r in 0..8 {
            for dy in -r..=r {
                for dx in -r..=r {
                    let c = (cx + dx, cy + dy);
                    if (dx.abs() == r || dy.abs() == r)
                        && self.nav.is_water(c.0, c.1)
                        && !occupied.contains(&c)
                    {
                        spot = Some(c);
                        break 'search;
                    }
                }
            }
        }
        let Some((x, y)) = spot.or_else(|| self.nav.nearest_water(cx, cy)) else {
            return Ok(());
        };
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 13];
        fields[0] = 0xc8;
        fields[1] = 1;
        fields[2] = human.nation;
        fields[3] = human.slot;
        fields[5] = human.nation;
        fields[6] = human.slot;
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
        }
        Ok(())
    }

    /// Whether the human player has a standing temple of `temple`.
    pub fn has_temple(&self, temple: Temple) -> bool {
        self.owned_buildings() & 1 << (temple.class() - SETTLEMENT) != 0
    }

    /// Starts learning the next spell of `temple`.
    pub fn learn_spell(&mut self, temple: Temple) -> Result<(), String> {
        if !self.has_temple(temple) {
            return Err("You need a temple for that".into());
        }
        if self.magic.learning(temple).is_some() {
            return Err("The temple is already studying".into());
        }
        if self.magic.all_known(temple) {
            return Err("All the temple's spells are known".into());
        }
        if self.amber < magic::LEARN_COST {
            return Err("Not enough amber".into());
        }
        self.amber -= magic::LEARN_COST;
        self.magic.start_learning(temple);
        Ok(())
    }

    /// Pays for `spell` if it is known and affordable.
    fn pay_spell(&mut self, spell: Spell) -> Result<(), String> {
        if !self.has_temple(spell.temple()) || !self.magic.knows(spell) {
            return Err("The temple does not know that spell".into());
        }
        let (amber, fish) = spell.cost();
        if self.amber < amber || self.fish < fish {
            return Err("Not enough amber or fish".into());
        }
        self.amber -= amber;
        self.fish -= fish;
        Ok(())
    }

    /// Casts `spell` on the ship or building `target`: lightning and flame
    /// on enemies, protections on own ships.
    pub fn cast_on(
        &mut self,
        spell: Spell,
        target: u32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        let t = self.object_index(target).ok_or("Nothing there")?;
        let obj = &self.objects[t];
        let c = obj.combat.as_ref().ok_or("Nothing there to cast on")?;
        match spell.target() {
            magic::Target::Enemy if self.is_own(obj) || obj.owner.is_none() => {
                return Err("Choose an enemy ship or building".into())
            }
            magic::Target::OwnShip if !self.is_own(obj) || obj.ship.is_none() => {
                return Err("Choose one of your ships".into())
            }
            magic::Target::Enemy | magic::Target::OwnShip => {}
            _ => return Err("That spell is not cast on a ship".into()),
        }
        let warded = c.protect_magic > 0.0 && spell.target() == magic::Target::Enemy;
        let large = obj
            .ship
            .as_ref()
            .is_some_and(|s| combat::hull_strength(s.kind) >= 1600.0);
        self.pay_spell(spell)?;
        let (x, y) = (self.objects[t].x, self.objects[t].y);
        if warded {
            self.messages.push("The spell was turned aside".into());
            return Ok(());
        }
        match spell {
            Spell::Lightning | Spell::SmallLightning => {
                self.spawn_effect(data, caches, magic::FX_LIGHTNING, x, y);
                let damage = if spell == Spell::Lightning {
                    magic::LIGHTNING_DAMAGE
                } else {
                    magic::SMALL_LIGHTNING_DAMAGE
                };
                self.hit(target, damage, data, caches);
            }
            Spell::Flame => {
                self.objects[t].combat.as_mut().unwrap().burning = magic::FLAME_SECONDS;
                self.spawn_aura(data, caches, magic::FX_FLAME, target, magic::FLAME_SECONDS);
            }
            Spell::ProtectWeapons | Spell::ProtectMagic => {
                let c = self.objects[t].combat.as_mut().unwrap();
                if spell == Spell::ProtectWeapons {
                    c.protect_weapons = magic::PROTECTION_SECONDS;
                } else {
                    c.protect_magic = magic::PROTECTION_SECONDS;
                }
                let name = magic::fx_protection(spell, large);
                self.spawn_aura(data, caches, &name, target, magic::PROTECTION_SECONDS);
            }
            _ => return Err("That spell is not cast on a ship".into()),
        }
        Ok(())
    }

    /// Calls up a strong wind towards `direction`.
    pub fn cast_wind(&mut self, direction: u32) -> Result<(), String> {
        self.pay_spell(Spell::Wind)?;
        self.wind.force(direction, magic::WIND_SECONDS);
        Ok(())
    }

    /// Raises a reef or summons a whirlpool at map cell `(cx, cy)`.
    pub fn cast_hazard(
        &mut self,
        spell: Spell,
        cx: i32,
        cy: i32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        let class = match spell {
            Spell::Whirlpool => 0x258,
            Spell::Reef => 0x25a,
            _ => return Err("Not a spell for the sea".into()),
        };
        if !self.nav.is_water(cx, cy) {
            return Err("Choose open water".into());
        }
        if !self.magic.knows(spell) {
            return Err("The temple does not know that spell".into());
        }
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 12];
        fields[0] = class;
        fields[1] = 1;
        fields[5] = cx as u32;
        fields[6] = cy as u32;
        fields[7] = 1;
        let rec = ObjectRecord {
            id,
            class_id: class,
            fields,
        };
        let obj = objects::resolve(data, caches, &rec)
            .map_err(|e| format!("The spell failed: {e:#}"))?
            .ok_or("The spell failed")?;
        self.pay_spell(spell)?;
        self.objects.push(obj);
        self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        Ok(())
    }

    /// Ships caught in whirlpools or on reefs take damage.
    fn run_hazards(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let hazards: Vec<(f32, f32, f32)> = self
            .objects
            .iter()
            .filter_map(|o| match o.class {
                ObjectClass::Whirlpool => Some((o.x, o.y, magic::WHIRLPOOL_DPS)),
                ObjectClass::Reef(_) => Some((o.x, o.y, magic::REEF_DPS)),
                _ => None,
            })
            .collect();
        if hazards.is_empty() {
            return;
        }
        let mut wrecked = Vec::new();
        for o in self.objects.iter_mut().filter(|o| o.ship.is_some()) {
            let dps: f32 = hazards
                .iter()
                .filter(|(x, y, _)| {
                    ((o.x - x).powi(2) + (o.y - y).powi(2)).sqrt() <= magic::HAZARD_RADIUS
                })
                .map(|h| h.2)
                .sum();
            if let (true, Some(c)) = (dps > 0.0, o.combat.as_mut()) {
                c.hull -= dps * dt;
                if c.hull <= 0.0 {
                    wrecked.push(o.id);
                }
            }
        }
        for id in wrecked {
            self.hit(id, 0.0, data, caches);
        }
    }

    /// The training going on at `site`, if any.
    pub fn training_at(&self, site: u32) -> Option<&Training> {
        self.trainings.iter().find(|t| t.site == site)
    }

    /// Marines waiting at `site`, by crew type.
    pub fn garrison(&self, site: u32) -> [u32; 3] {
        self.garrisons.get(&site).copied().unwrap_or_default()
    }

    /// Orders one more marine of type `kind` at settlement `site`, paying
    /// for it. A settlement trains one crew type at a time.
    pub fn train(&mut self, site: u32, kind: u8) -> Result<(), String> {
        let (amber, fish) = combat::CREW_COST[kind as usize];
        if self.amber < amber || self.fish < fish {
            return Err("Not enough amber or fish".into());
        }
        match self.trainings.iter_mut().find(|t| t.site == site) {
            Some(t) if t.kind != kind => {
                return Err("The settlement is training other marines".into())
            }
            Some(t) => t.queued += 1,
            None => self.trainings.push(Training {
                site,
                kind,
                queued: 1,
                elapsed: 0.0,
            }),
        }
        self.amber -= amber;
        self.fish -= fish;
        Ok(())
    }

    /// Ship `ship` takes on marines from an own settlement or town nearby:
    /// of its crew's type, or if it has none, of the most numerous type
    /// waiting. Returns how many came aboard.
    pub fn load_crew(&mut self, ship: u32) -> Result<u32, String> {
        let t = TILE_SIZE as f32;
        let i = self.object_index(ship).ok_or("No such ship")?;
        let (sx, sy) = (self.objects[i].x, self.objects[i].y);
        let site = self
            .objects
            .iter()
            .filter(|o| {
                self.is_own(o)
                    && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
                    && ((o.x - sx).powi(2) + (o.y - sy).powi(2)).sqrt() <= combat::LOAD_REACH * t
            })
            .map(|o| o.id)
            .max_by_key(|&id| self.garrison(id).iter().sum::<u32>())
            .ok_or("The ship must lie next to your settlement")?;
        let mut garrison = self.garrison(site);
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        let kind = if s.crew.count > 0 {
            s.crew.kind
        } else {
            (0..3u8).max_by_key(|&k| garrison[k as usize]).unwrap()
        };
        let room = combat::CREW_CAPACITY[s.kind as usize % 12].saturating_sub(s.crew.count);
        let n = room.min(garrison[kind as usize]);
        if n == 0 {
            return Err(if room == 0 {
                "The ship is full"
            } else {
                "No marines of that kind are waiting"
            }
            .into());
        }
        s.crew.kind = kind;
        s.crew.count += n;
        garrison[kind as usize] -= n;
        self.garrisons.insert(site, garrison);
        Ok(n)
    }

    /// Advances marine training.
    fn run_training(&mut self, dt: f32) {
        for t in &mut self.trainings {
            t.elapsed += dt;
            if t.elapsed >= combat::CREW_TRAIN_SECONDS {
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
    fn run_constructions(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let mut done = Vec::new();
        for c in &mut self.constructions {
            c.elapsed += dt;
            let Some(i) = self.objects.iter().position(|o| o.id == c.site) else {
                continue;
            };
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
        }
    }

    /// Re-selects a building's frames for its current condition.
    fn refresh_building(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
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
    }

    /// Index of the object with mission id `id`.
    pub fn object_index(&self, id: u32) -> Option<usize> {
        self.objects.iter().position(|o| o.id == id)
    }

    /// Orders ship `id` to sail to the world position `(x, y)`. Returns
    /// false if the ship cannot get there.
    pub fn order_move(&mut self, id: u32, x: f32, y: f32) -> bool {
        let Some(i) = self.object_index(id) else {
            return false;
        };
        let Some(path) = self.plan_route(i, x, y) else {
            return false;
        };
        let Some(ship) = self.objects[i].ship.as_mut() else {
            return false;
        };
        ship.path = path;
        ship.task = None;
        true
    }

    /// A route for object `i` to the world point `(x, y)`, or to the nearest
    /// water if the point is not navigable.
    fn plan_route(&self, i: usize, x: f32, y: f32) -> Option<VecDeque<(f32, f32)>> {
        let t = TILE_SIZE as f32;
        let obj = &self.objects[i];
        let from = ((obj.x / t).floor() as i32, (obj.y / t).floor() as i32);
        let target = ((x / t).floor() as i32, (y / t).floor() as i32);
        let to = self.nav.nearest_water(target.0, target.1)?;
        // A ship left on a non-water cell (e.g. placed at a dock) starts from
        // the nearest water.
        let start = if self.nav.is_water(from.0, from.1) {
            from
        } else {
            self.nav.nearest_water(from.0, from.1)?
        };
        let cells = self.nav.find_path(start, to)?;
        let mut path: VecDeque<(f32, f32)> = cells
            .into_iter()
            .map(|(cx, cy)| (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0))
            .collect();
        // Finish on the exact point if it is in open water.
        if to == target {
            if let Some(last) = path.back_mut() {
                *last = (x, y);
            }
        }
        Some(path)
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

    /// The human player's building nearest to `(x, y)` where cargo can be
    /// unloaded: a port, or failing that a settlement or town.
    fn unloading_site(&self, x: f32, y: f32) -> Option<u32> {
        let dist = |o: &MapObject| (o.x - x).powi(2) + (o.y - y).powi(2);
        let nearest = |classes: &[u32]| {
            self.objects
                .iter()
                .filter(|o| {
                    matches!(o.class, ObjectClass::Building(c) if classes.contains(&c))
                        && self.is_own(o)
                })
                .min_by(|a, b| dist(a).total_cmp(&dist(b)))
                .map(|o| o.id)
        };
        nearest(&[PORT]).or_else(|| nearest(&[SETTLEMENT, TOWN]))
    }

    /// Advances the standing order of ship `i`.
    fn run_task(&mut self, i: usize, dt: f32, data: &GameData, caches: &mut Caches) {
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
        let deposit_at = self.object_index(deposit);
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
                let ship = self.objects[i].ship.as_ref().unwrap();
                let mut take = (GATHER_RATE * dt).min(capacity - ship.cargo);
                if let Some(left) = dep.remaining {
                    take = take.min(left);
                }
                let ship = self.objects[i].ship.as_mut().unwrap();
                ship.cargo += take;
                ship.cargo_kind = Some(dep.resource);
                let (full, cargo) = (ship.cargo >= capacity, ship.cargo);
                let mut exhausted = false;
                if let Some(left) = self.objects[d]
                    .deposit
                    .as_mut()
                    .and_then(|d| d.remaining.as_mut())
                {
                    *left -= take;
                    exhausted = *left <= 0.0;
                }
                if exhausted {
                    // Removing the deposit may shift the ship's index.
                    let ship_id = self.objects[i].id;
                    self.objects.remove(d);
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
                if !near(&self.objects[p], 2.6) {
                    return self.send_to_port(i, deposit);
                }
                let ship = self.objects[i].ship.as_mut().unwrap();
                let (cargo, kind) = (ship.cargo.floor(), ship.cargo_kind.take());
                ship.cargo = 0.0;
                if self.is_own(&self.objects[i]) {
                    match kind {
                        Some(Resource::Amber) => self.amber += cargo as u32,
                        Some(Resource::Fish) => self.fish += cargo as u32,
                        None => {}
                    }
                }
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
    fn send_to_port(&mut self, i: usize, deposit: u32) {
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        let route = self.unloading_site(x, y).and_then(|port| {
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

    /// Whether two objects belong to different players. All players are
    /// treated as enemies of each other until diplomacy is implemented.
    pub fn are_enemies(a: &MapObject, b: &MapObject) -> bool {
        matches!((a.owner, b.owner), (Some(x), Some(y)) if x != y)
    }

    /// Orders armed ship `ship` to attack the enemy ship or building `target`.
    pub fn order_attack(&mut self, ship: u32, target: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let t = self.object_index(target).ok_or("No such target")?;
        if self.objects[i].ship.is_none() || !self.objects[i].is_armed() {
            return Err("This ship has no weapons".into());
        }
        if self.objects[t].combat.is_none()
            || !Self::are_enemies(&self.objects[i], &self.objects[t])
        {
            return Err("Choose an enemy ship or building".into());
        }
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path.clear();
        ship.task = Some(Task::Attack { target });
        Ok(())
    }

    /// Computer players: idle warships hunt the human player's ships that
    /// come within sight. (A first, simple opponent; the original AI has not
    /// been studied yet.)
    fn run_ai(&mut self) {
        const SIGHT: f32 = 10.0 * TILE_SIZE as f32;
        let human: Vec<(u32, f32, f32)> = self
            .objects
            .iter()
            .filter(|o| o.ship.is_some() && self.is_own(o))
            .map(|o| (o.id, o.x, o.y))
            .collect();
        if human.is_empty() {
            return;
        }
        let mut orders = Vec::new();
        for o in &self.objects {
            let Some(ship) = &o.ship else { continue };
            if o.owner.is_none() || self.is_own(o) || !o.is_armed() || ship.task.is_some() {
                continue;
            }
            let near = human
                .iter()
                .map(|&(id, x, y)| (id, ((x - o.x).powi(2) + (y - o.y).powi(2)).sqrt()))
                .filter(|&(_, d)| d <= SIGHT)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((target, _)) = near {
                orders.push((o.id, target));
            }
        }
        for (ship, target) in orders {
            let _ = self.order_attack(ship, target);
        }
    }

    /// Armed ships and forts pick targets, ships close in, and everything in
    /// range fires; sunk ships and destroyed buildings are removed.
    fn run_combat(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let dist =
            |a: &MapObject, b: &MapObject| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
        let armed: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.is_armed())
            .map(|o| o.id)
            .collect();
        let mut volleys = Vec::new();
        for id in armed {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let c = self.objects[i].combat.as_mut().unwrap();
            c.reload = (c.reload - dt).max(0.0);
            let (ordered, idle) = match &self.objects[i].ship {
                Some(ship) => (
                    match ship.task {
                        Some(Task::Attack { target }) => Some(target),
                        _ => None,
                    },
                    ship.task.is_none() && ship.path.is_empty(),
                ),
                // Forts are always on guard.
                None => (None, true),
            };
            let me = &self.objects[i];
            let target = match ordered {
                Some(t) => match self.object_index(t) {
                    Some(ti) => Some(ti),
                    None => {
                        self.objects[i].ship.as_mut().unwrap().task = None;
                        continue;
                    }
                },
                // Idle ships and forts fire at enemy ships in range.
                None if idle => self
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| {
                        o.ship.is_some() && Self::are_enemies(me, o) && dist(me, o) <= combat::RANGE
                    })
                    .min_by(|a, b| dist(me, a.1).total_cmp(&dist(me, b.1)))
                    .map(|(ti, _)| ti),
                None => None,
            };
            let Some(ti) = target else { continue };
            let d = dist(&self.objects[i], &self.objects[ti]);
            let (tx, ty) = (self.objects[ti].x, self.objects[ti].y);
            if d > combat::RANGE {
                // Chase: re-plan when the target has moved away from the route's end.
                let Some(ship) = self.objects[i].ship.as_ref() else {
                    continue;
                };
                let stale = ship.path.back().is_none_or(|&(px, py)| {
                    ((px - tx).powi(2) + (py - ty).powi(2)).sqrt() > combat::RANGE / 2.0
                });
                if ordered.is_some() && stale {
                    if let Some(path) = self.plan_route(i, tx, ty) {
                        self.objects[i].ship.as_mut().unwrap().path = path;
                    }
                }
                continue;
            }
            let target_id = self.objects[ti].id;
            if ordered.is_some() {
                self.objects[i].ship.as_mut().unwrap().path.clear();
            }
            let c = self.objects[i].combat.as_mut().unwrap();
            if c.reload <= 0.0 {
                c.reload = combat::RELOAD_SECONDS;
                volleys.push((target_id, c.weapons.clone()));
            }
        }
        for (target, weapons) in volleys {
            let warded = self
                .object_index(target)
                .and_then(|t| self.objects[t].combat.as_ref())
                .is_some_and(|c| c.protect_weapons > 0.0);
            if warded {
                self.hit(target, 0.0, data, caches);
                continue;
            }
            let mut damage = 0.0;
            for w in weapons {
                if combat::hull_damage(w) > 0.0 && self.rng.chance(combat::HIT_CHANCE) {
                    damage += combat::hull_damage(w);
                }
            }
            self.hit(target, damage, data, caches);
        }
        for fx in &mut self.effects {
            fx.age_ms += dt * 1000.0;
        }
        self.effects.retain(|fx| fx.frame().is_some());
    }

    /// Deals `damage` to the hull of `target`, with a hit (or splash) effect;
    /// a sunk ship is removed and a destroyed building left in ruins.
    fn hit(&mut self, target: u32, damage: f32, data: &GameData, caches: &mut Caches) {
        let Some(ti) = self.object_index(target) else {
            return;
        };
        let (tx, ty) = (self.objects[ti].x, self.objects[ti].y);
        let jitter = |rng: &mut Rng| (rng.next() % 33) as f32 - 16.0;
        let (fx, fy) = (tx + jitter(&mut self.rng), ty + jitter(&mut self.rng));
        let Some(c) = self.objects[ti].combat.as_mut() else {
            return;
        };
        c.hull -= damage;
        let destroyed = c.hull <= 0.0;
        let name = if damage > 0.0 {
            combat::FX_HIT
        } else {
            combat::FX_SPLASH
        };
        self.spawn_effect(data, caches, name, fx, fy);
        let is_building = self.objects[ti].ship.is_none();
        if destroyed {
            self.productions.retain(|p| p.site != target);
            self.constructions.retain(|c| c.site != target);
            self.spawn_effect(data, caches, combat::FX_SINK, tx, ty);
            let ruins = is_building
                .then(|| {
                    objects::building_frames(
                        data,
                        caches,
                        &self.objects[ti].record,
                        BuildingState::Ruins,
                    )
                    .ok()
                })
                .flatten()
                .filter(|f| !f.is_empty());
            match ruins {
                // Ruins stay as scenery, owned by no one.
                Some(frames) => {
                    let obj = &mut self.objects[ti];
                    obj.frames = frames;
                    obj.combat = None;
                    obj.owner = None;
                    obj.picture = None;
                    obj.label = format!("Ruins ({})", obj.label);
                }
                None => {
                    self.objects.remove(ti);
                }
            }
        } else if is_building && self.construction_at(target).is_none() {
            self.refresh_building(target, data, caches);
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

    /// Orders ship `ship` to ram or board the enemy ship `target`.
    pub fn order_close(&mut self, ship: u32, target: u32, board: bool) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let t = self.object_index(target).ok_or("No such target")?;
        if self.objects[t].ship.is_none() || !Self::are_enemies(&self.objects[i], &self.objects[t])
        {
            return Err("Choose an enemy ship".into());
        }
        let ship = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        if board && ship.crew.count == 0 {
            return Err("There is no crew to board with".into());
        }
        ship.path.clear();
        ship.task = Some(if board {
            Task::Board { target, round: 0.0 }
        } else {
            Task::Ram { target }
        });
        Ok(())
    }

    /// Ships ordered to ram or board close in on their target and, on
    /// contact, ram it or fight its crew.
    fn run_contact(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let closing: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                matches!(
                    o.ship.as_ref().and_then(|s| s.task),
                    Some(Task::Ram { .. } | Task::Board { .. })
                )
            })
            .map(|o| o.id)
            .collect();
        for id in closing {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let task = self.objects[i].ship.as_ref().unwrap().task.unwrap();
            let (Task::Ram { target } | Task::Board { target, .. }) = task else {
                continue;
            };
            let Some(ti) = self.object_index(target).filter(|&t| {
                self.objects[t].ship.is_some()
                    && Self::are_enemies(&self.objects[i], &self.objects[t])
            }) else {
                self.objects[i].ship.as_mut().unwrap().task = None;
                continue;
            };
            let (tx, ty) = (self.objects[ti].x, self.objects[ti].y);
            let d = ((self.objects[i].x - tx).powi(2) + (self.objects[i].y - ty).powi(2)).sqrt();
            if d > combat::CONTACT {
                let ship = self.objects[i].ship.as_ref().unwrap();
                let stale = ship.path.back().is_none_or(|&(px, py)| {
                    ((px - tx).powi(2) + (py - ty).powi(2)).sqrt() > combat::CONTACT
                });
                if stale {
                    if let Some(path) = self.plan_route(i, tx, ty) {
                        self.objects[i].ship.as_mut().unwrap().path = path;
                    }
                }
                continue;
            }
            self.objects[i].ship.as_mut().unwrap().path.clear();
            match task {
                Task::Ram { .. } => {
                    self.ram(i, ti, data, caches);
                }
                Task::Board { round, .. } => {
                    // Grappled: the target stops too.
                    self.objects[ti].ship.as_mut().unwrap().path.clear();
                    let round = round - dt;
                    if round > 0.0 {
                        self.objects[i].ship.as_mut().unwrap().task =
                            Some(Task::Board { target, round });
                    } else {
                        self.objects[i].ship.as_mut().unwrap().task = Some(Task::Board {
                            target,
                            round: combat::BOARD_ROUND,
                        });
                        self.board_round(id, target, data, caches);
                    }
                }
                _ => {}
            }
        }
    }

    /// Ship `i` rams ship `ti`; both take damage by the odds of the ram.
    fn ram(&mut self, i: usize, ti: usize, data: &GameData, caches: &mut Caches) {
        let (a, t) = (&self.objects[i], &self.objects[ti]);
        let (sa, st) = (a.ship.as_ref().unwrap(), t.ship.as_ref().unwrap());
        let angle = combat::ram_angle(sa.heading, st.heading);
        let odds = combat::ram_odds((sa.kind, sa.crew), (st.kind, st.crew), angle) as f32;
        let max = |o: &MapObject| o.combat.as_ref().map_or(0.0, |c| c.max_hull);
        // Placeholder scale: a ram at the best odds takes half the target's hull.
        let to_target = max(t) * odds / 400.0;
        let to_self = max(a) * 0.1 * (100.0 / odds.max(1.0)).min(1.0);
        let (ida, idt) = (a.id, t.id);
        self.objects[i].ship.as_mut().unwrap().task = None;
        self.hit(idt, to_target, data, caches);
        self.hit(ida, to_self, data, caches);
    }

    /// One exchange of blows between the boarders of ship `a` and the crew
    /// of ship `t`; when the defenders fall, the ship changes hands.
    fn board_round(&mut self, a: u32, t: u32, data: &GameData, caches: &mut Caches) {
        let (Some(i), Some(ti)) = (self.object_index(a), self.object_index(t)) else {
            return;
        };
        let ca = self.objects[i].ship.as_ref().unwrap().crew;
        let ct = self.objects[ti].ship.as_ref().unwrap().crew;
        let pa = 100 * ca.melee() / (ca.melee() + ct.melee()).max(1);
        // Each fighter strikes once a round, killing with his side's odds.
        let kills_a = (0..ca.count).filter(|_| self.rng.chance(pa / 2)).count() as u32;
        let kills_t = (0..ct.count)
            .filter(|_| self.rng.chance((100 - pa) / 2))
            .count() as u32;
        let crew_a = ca.count.saturating_sub(kills_t);
        let crew_t = ct.count.saturating_sub(kills_a);
        self.objects[i].ship.as_mut().unwrap().crew.count = crew_a;
        self.objects[ti].ship.as_mut().unwrap().crew.count = crew_t;
        if crew_a == 0 {
            self.objects[i].ship.as_mut().unwrap().task = None;
            return;
        }
        if crew_t > 0 {
            return;
        }
        // Captured: half the boarders (at least one) stay on the prize.
        let prize = crew_a.div_ceil(2);
        let owner = self.objects[i].owner;
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.crew.count -= prize;
        ship.task = None;
        self.capture(
            t,
            owner,
            combat::Crew {
                kind: ca.kind,
                count: prize,
            },
            data,
            caches,
        );
    }

    /// Gives ship `id` to `owner`, redrawing it in the new owner's colours.
    fn capture(
        &mut self,
        id: u32,
        owner: Option<(u32, u32)>,
        crew: combat::Crew,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let Some((nation, player)) = owner else {
            return;
        };
        let mut rec = self.objects[i].record.clone();
        if rec.fields.len() < 7 {
            rec.fields.resize(7, 0);
        }
        rec.fields[5] = nation;
        rec.fields[6] = player;
        let obj = &mut self.objects[i];
        obj.owner = owner;
        if let Some(ship) = obj.ship.as_mut() {
            ship.crew = crew;
            ship.task = None;
            ship.path.clear();
        }
        match objects::resolve(data, caches, &rec) {
            Ok(Some(new)) => {
                obj.frames = new.frames;
                obj.label = new.label;
                obj.picture = new.picture;
            }
            Ok(None) => {}
            Err(e) => eprintln!("could not redraw captured ship: {e:#}"),
        }
        obj.record = rec;
    }

    fn spawn_effect(&mut self, data: &GameData, caches: &mut Caches, name: &str, x: f32, y: f32) {
        match objects::sequence_frames(data, caches, name) {
            Ok(frames) if !frames.is_empty() => self.effects.push(Effect {
                x,
                y,
                frames,
                age_ms: 0.0,
                follow: None,
                life_ms: None,
            }),
            Ok(_) => {}
            Err(e) => eprintln!("could not load effect {name}: {e:#}"),
        }
    }

    /// Loops the animation `name` over object `id` for `seconds`.
    fn spawn_aura(
        &mut self,
        data: &GameData,
        caches: &mut Caches,
        name: &str,
        id: u32,
        seconds: f32,
    ) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        // A new aura of the same kind replaces the old one.
        self.effects
            .retain(|fx| fx.follow != Some(id) || fx.life_ms.is_none());
        match objects::sequence_frames(data, caches, name) {
            Ok(frames) if !frames.is_empty() => self.effects.push(Effect {
                x,
                y,
                frames,
                age_ms: 0.0,
                follow: Some(id),
                life_ms: Some(seconds * 1000.0),
            }),
            Ok(_) => {}
            Err(e) => eprintln!("could not load effect {name}: {e:#}"),
        }
    }

    /// Burning, protections running out, and effects following their objects.
    fn run_statuses(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let mut wrecked = Vec::new();
        for o in &mut self.objects {
            let Some(c) = o.combat.as_mut() else { continue };
            if c.burning > 0.0 {
                c.burning -= dt;
                c.hull -= magic::FLAME_DPS * dt;
                if c.hull <= 0.0 {
                    wrecked.push(o.id);
                }
            }
            c.protect_weapons = (c.protect_weapons - dt).max(0.0);
            c.protect_magic = (c.protect_magic - dt).max(0.0);
        }
        for id in wrecked {
            self.hit(id, 0.0, data, caches);
        }
        let positions: std::collections::HashMap<u32, (f32, f32)> =
            self.objects.iter().map(|o| (o.id, (o.x, o.y))).collect();
        self.effects.retain_mut(|fx| match fx.follow {
            Some(id) => match positions.get(&id) {
                Some(&(x, y)) => {
                    (fx.x, fx.y) = (x, y);
                    true
                }
                None => false,
            },
            None => true,
        });
    }

    /// Cancels a ship's route.
    pub fn stop(&mut self, id: u32) {
        if let Some(i) = self.object_index(id) {
            if let Some(ship) = self.objects[i].ship.as_mut() {
                ship.path.clear();
                ship.task = None;
            }
        }
    }

    /// Advances the simulation by `dt` seconds.
    pub fn update(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        for p in &mut self.productions {
            p.elapsed += dt;
        }
        let (done, running): (Vec<_>, Vec<_>) = self
            .productions
            .drain(..)
            .partition(|p| p.elapsed >= p.total);
        self.productions = running;
        for p in done {
            if let Err(e) = self.launch_ship(p.site, p.ship, data, caches) {
                eprintln!("could not launch ship: {e:#}");
            }
        }
        let ships: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.task.is_some()))
            .map(|o| o.id)
            .collect();
        for id in ships {
            if let Some(i) = self.object_index(id) {
                self.run_task(i, dt, data, caches);
            }
        }
        self.ai_timer -= dt;
        if self.ai_timer <= 0.0 {
            self.ai_timer = 1.0;
            self.run_ai();
        }
        self.wind.update(dt);
        self.run_training(dt);
        self.magic.update(dt);
        self.run_statuses(dt, data, caches);
        self.run_hazards(dt, data, caches);
        self.run_constructions(dt, data, caches);
        self.run_combat(dt, data, caches);
        self.run_contact(dt, data, caches);
        let mut moved = false;
        for obj in &mut self.objects {
            let Some(ship) = obj.ship.as_mut() else {
                continue;
            };
            // The wind speeds or slows the ship by its heading.
            let merchant = cargo_capacity(ship.kind) > 0.0;
            let mut budget = ship.speed * self.wind.speed_factor(merchant, ship.heading) * dt;
            while budget > 0.0 {
                let Some(&(tx, ty)) = ship.path.front() else {
                    break;
                };
                let (dx, dy) = (tx - obj.x, ty - obj.y);
                let dist = (dx * dx + dy * dy).sqrt();
                if dist > 0.5 {
                    ship.heading = heading_towards(dx, dy);
                }
                if dist <= budget {
                    obj.x = tx;
                    obj.y = ty;
                    budget -= dist;
                    ship.path.pop_front();
                } else {
                    obj.x += dx / dist * budget;
                    obj.y += dy / dist * budget;
                    budget = 0.0;
                }
                moved = true;
            }
        }
        if moved {
            // Keep painter's order as ships move.
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        }
    }

    pub fn map_size(&self) -> (u32, u32) {
        (self.terrain.width, self.terrain.height)
    }
}

/// The ship heading (0 = east, counter-clockwise in 45 degree steps) closest
/// to the screen-space direction `(dx, dy)`.
pub fn heading_towards(dx: f32, dy: f32) -> usize {
    // Screen y grows downwards; headings count counter-clockwise from east.
    let angle = (-dy).atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    ((angle / step).round() as i32).rem_euclid(8) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::Caches;

    /// Builds a boat and upgrades the settlement in the first campaign
    /// mission (needs `OPENAC_DATA`).
    #[test]
    fn shipyard_builds_and_settlement_upgrades() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let own = |scene: &Scene, cls| {
            scene
                .objects
                .iter()
                .find(|o| o.class == ObjectClass::Building(cls) && scene.is_own(o))
                .map(|o| o.id)
                .unwrap()
        };
        let yard = own(&scene, SHIPYARD);
        let ships_before = scene
            .objects
            .iter()
            .filter(|o| o.ship.is_some() && scene.is_own(o))
            .count();
        assert_eq!(scene.amber, 300);
        // A merchant needs a town; a boat only a shipyard and a settlement.
        assert!(scene.can_build_ship(1).is_err());
        scene.start_ship(yard, 0).unwrap();
        assert_eq!(scene.amber, 100);
        assert!(
            scene.start_ship(yard, 0).is_err(),
            "shipyard should be busy"
        );
        for _ in 0..(60.0 * (economy::SHIP_BUILD_SECONDS + 1.0)) as usize {
            scene.update(1.0 / 60.0, &data, &mut caches);
        }
        let own_ships: Vec<_> = scene
            .objects
            .iter()
            .filter(|o| o.ship.is_some() && scene.is_own(o))
            .collect();
        assert_eq!(own_ships.len(), ships_before + 1);
        assert!(scene.productions.is_empty());

        let settlement = own(&scene, SETTLEMENT);
        assert!(
            scene.upgrade(settlement, &data, &mut caches).is_err(),
            "100 amber is not enough"
        );
        scene.amber = 1000;
        scene.upgrade(settlement, &data, &mut caches).unwrap();
        assert_eq!(scene.amber, 500);
        let town = &scene.objects[scene.object_index(settlement).unwrap()];
        assert_eq!(town.class, ObjectClass::Building(TOWN));
        // With a town and 500 amber, a merchant (400 amber, 200 fish) is possible.
        assert_eq!(scene.can_build_ship(1), Ok(()));
    }

    /// A warship attacks and sinks an enemy ship (needs `OPENAC_DATA`).
    #[test]
    fn warship_sinks_enemy() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let human = scene.human.unwrap();
        // Put a royal ship of ours next to a merchant of another player.
        let mine = scene
            .objects
            .iter()
            .position(|o| scene.is_own(o) && o.ship.is_some())
            .unwrap();
        let (x, y) = (scene.objects[mine].x, scene.objects[mine].y);
        let ship = scene.objects[mine].ship.as_mut().unwrap();
        ship.kind = 2;
        let c = scene.objects[mine].combat.as_mut().unwrap();
        c.weapons = combat::DEFAULT_WEAPONS[2].to_vec();
        c.max_hull = combat::hull_strength(2);
        c.hull = c.max_hull;
        let enemy = scene
            .objects
            .iter()
            .position(|o| o.ship.is_some() && !scene.is_own(o) && o.owner.is_some());
        let enemy = match enemy {
            Some(e) => e,
            None => {
                let e = scene
                    .objects
                    .iter()
                    .position(|o| o.ship.is_some() && o.id != scene.objects[mine].id)
                    .unwrap();
                scene.objects[e].owner = Some((human.nation, human.slot + 1));
                e
            }
        };
        scene.objects[enemy].x = x + 128.0;
        scene.objects[enemy].y = y;
        let target = scene.objects[enemy].id;
        let mine = scene.objects[mine].id;
        scene.order_attack(mine, target).unwrap();
        for _ in 0..60 * 120 {
            scene.update(1.0 / 60.0, &data, &mut caches);
            if scene.object_index(target).is_none() {
                break;
            }
        }
        assert!(scene.object_index(target).is_none(), "the enemy was sunk");
        assert!(scene.object_index(mine).is_some());
    }

    /// A rammer damages its target, and boarders capture a ship (needs
    /// `OPENAC_DATA`).
    #[test]
    fn ramming_and_boarding() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
        let human = scene.human.unwrap();
        let mine = scene
            .objects
            .iter()
            .position(|o| scene.is_own(o) && o.ship.is_some())
            .unwrap();
        let mine_id = scene.objects[mine].id;
        let other = scene
            .objects
            .iter()
            .position(|o| o.ship.is_some() && o.id != mine_id)
            .unwrap();
        let other_id = scene.objects[other].id;
        scene
            .objects
            .retain(|o| o.ship.is_none() || o.id == mine_id || o.id == other_id);
        // Two unarmed ships side by side in open water.
        let (x, y) = (
            scene.objects[scene.object_index(mine_id).unwrap()].x,
            scene.objects[scene.object_index(mine_id).unwrap()].y,
        );
        for (id, dx, kind, crew) in [(mine_id, 0.0, 5u8, 20u32), (other_id, 40.0, 1, 3)] {
            let i = scene.object_index(id).unwrap();
            let o = &mut scene.objects[i];
            o.x = x + dx;
            o.y = y;
            o.combat.as_mut().unwrap().weapons.clear();
            let s = o.ship.as_mut().unwrap();
            s.kind = kind;
            s.crew = combat::Crew {
                kind: 2,
                count: crew,
            };
        }
        let i = scene.object_index(other_id).unwrap();
        scene.objects[i].owner = Some((1, (human.slot + 1) % 4));
        let hull = |s: &Scene, id| {
            s.objects[s.object_index(id).unwrap()]
                .combat
                .as_ref()
                .unwrap()
                .hull
        };
        let before = hull(&scene, other_id);
        scene.order_close(mine_id, other_id, false).unwrap();
        scene.update(0.02, &data, &mut caches);
        assert!(hull(&scene, other_id) < before, "the ram did damage");
        scene.order_close(mine_id, other_id, true).unwrap();
        for _ in 0..50 * 60 {
            scene.update(0.02, &data, &mut caches);
            let i = scene.object_index(other_id).unwrap();
            if scene.is_own(&scene.objects[i]) {
                break;
            }
        }
        let prize = &scene.objects[scene.object_index(other_id).unwrap()];
        assert!(scene.is_own(prize), "the ship was captured");
        assert!(prize.ship.as_ref().unwrap().crew.count > 0);
    }

    /// A boat founds a settlement on a distant shore (needs `OPENAC_DATA`).
    #[test]
    fn boat_founds_settlement() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let boat = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
            .unwrap()
            .id;
        scene.objects.retain(|o| o.ship.is_none() || o.id == boat);
        scene.amber = 1000;
        let settlements = |s: &Scene| {
            s.objects
                .iter()
                .filter(|o| s.is_own(o) && o.class == ObjectClass::Building(SETTLEMENT))
                .count()
        };
        let before = settlements(&scene);
        // A shore cell well away from the boat's home.
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let shore = (0..h)
            .rev()
            .flat_map(|y| (0..w).rev().map(move |x| (x, y)))
            .find(|&(x, y)| {
                scene.check_ground(SETTLEMENT, x, y).is_ok()
                    && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .any(|(dx, dy)| scene.nav.is_water(x + dx, y + dy))
            })
            .unwrap();
        scene.order_found(boat, shore.0, shore.1).unwrap();
        for _ in 0..50 * 600 {
            scene.update(0.02, &data, &mut caches);
            if !scene.constructions.is_empty() {
                break;
            }
        }
        assert!(scene.messages.is_empty(), "{:?}", scene.messages);
        assert_eq!(scene.amber, 1000 - economy::building_cost(SETTLEMENT));
        for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
            scene.update(0.02, &data, &mut caches);
        }
        assert_eq!(settlements(&scene), before + 1);
    }

    /// A settlement trains swordsmen and a ship alongside takes them on
    /// (needs `OPENAC_DATA`).
    #[test]
    fn marines_train_and_embark() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let home = scene
            .objects
            .iter()
            .find(|o| {
                scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
            })
            .unwrap();
        let (hx, hy, home) = (home.x, home.y, home.id);
        let ship = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.is_some())
            .unwrap()
            .id;
        scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
        let (amber, fish) = (scene.amber, scene.fish);
        scene.train(home, 2).unwrap();
        scene.train(home, 2).unwrap();
        assert!(scene.train(home, 0).is_err(), "one crew type at a time");
        assert_eq!((scene.amber, scene.fish), (amber - 140, fish - 600));
        for _ in 0..50 * (2 * combat::CREW_TRAIN_SECONDS as usize + 1) {
            scene.update(0.02, &data, &mut caches);
        }
        assert_eq!(scene.garrison(home), [0, 0, 2]);
        assert!(scene.training_at(home).is_none());
        let i = scene.object_index(ship).unwrap();
        scene.objects[i].ship.as_mut().unwrap().crew = combat::Crew { kind: 2, count: 0 };
        let i = scene.object_index(ship).unwrap();
        scene.objects[i].x = hx + 64.0;
        scene.objects[i].y = hy;
        assert_eq!(scene.load_crew(ship), Ok(2));
        assert_eq!(scene.garrison(home), [0, 0, 0]);
    }

    /// A Temple of the Sea learns spells; the wind obeys and a whirlpool
    /// sinks a ship (needs `OPENAC_DATA`).
    #[test]
    fn temple_magic() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap().id;
        scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
        scene.amber = 50_000;
        scene.fish = 50_000;
        assert!(scene.learn_spell(Temple::Sea).is_err(), "no temple yet");
        let home = scene
            .objects
            .iter()
            .find(|o| {
                scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
            })
            .unwrap();
        let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
        let site = (-BUILD_RADIUS..=BUILD_RADIUS)
            .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
            .find(|&(x, y)| {
                scene
                    .check_site(home, economy::TEMPLE_OF_THE_SEA, x, y)
                    .is_ok()
            })
            .expect("room for a temple");
        scene
            .place_building(
                home,
                economy::TEMPLE_OF_THE_SEA,
                site.0,
                site.1,
                &data,
                &mut caches,
            )
            .unwrap();
        assert!(scene.learn_spell(Temple::Sea).is_err(), "still being built");
        for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
            scene.update(0.02, &data, &mut caches);
        }
        // Learn the first four: wind, view map, tornado, whirlpool.
        for _ in 0..4 {
            scene.learn_spell(Temple::Sea).unwrap();
            for _ in 0..50 * (magic::LEARN_SECONDS as usize + 1) {
                scene.update(0.02, &data, &mut caches);
            }
        }
        assert!(scene.magic.knows(Spell::Whirlpool) && !scene.magic.knows(Spell::Reef));
        scene.cast_wind(6).unwrap();
        assert_eq!((scene.wind.strength, scene.wind.direction), (7, 6));
        let i = scene.object_index(ship).unwrap();
        let (cx, cy) = (
            (scene.objects[i].x / 64.0) as i32,
            (scene.objects[i].y / 64.0) as i32,
        );
        assert!(
            scene
                .cast_hazard(Spell::Reef, cx, cy, &data, &mut caches)
                .is_err(),
            "unknown"
        );
        scene
            .cast_hazard(Spell::Whirlpool, cx, cy, &data, &mut caches)
            .unwrap();
        // Put the ship in the whirlpool's eye.
        let pool = scene
            .objects
            .iter()
            .find(|o| {
                o.class == ObjectClass::Whirlpool
                    && o.record.fields[5] == cx as u32
                    && o.record.fields[6] == cy as u32
            })
            .unwrap();
        let (px, py) = (pool.x, pool.y);
        let i = scene.object_index(ship).unwrap();
        scene.objects[i].x = px;
        scene.objects[i].y = py;
        for _ in 0..50 * 30 {
            scene.update(0.02, &data, &mut caches);
            if scene.object_index(ship).is_none() {
                break;
            }
        }
        assert!(
            scene.object_index(ship).is_none(),
            "the whirlpool sank the ship"
        );
    }

    /// A Temple of War learns all its spells; protection wards off weapons
    /// and magic, lightning and flame hurt (needs `OPENAC_DATA`).
    #[test]
    fn war_magic() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.amber = 200_000;
        scene.fish = 200_000;
        let home = scene
            .objects
            .iter()
            .find(|o| {
                scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
            })
            .unwrap();
        let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
        let war = economy::TEMPLE_OF_WAR;
        let site = (-BUILD_RADIUS..=BUILD_RADIUS)
            .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
            .find(|&(x, y)| scene.check_site(home, war, x, y).is_ok())
            .expect("room for a temple");
        scene
            .place_building(home, war, site.0, site.1, &data, &mut caches)
            .unwrap();
        for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
            scene.update(0.02, &data, &mut caches);
        }
        for _ in 0..6 {
            scene.learn_spell(Temple::War).unwrap();
            for _ in 0..50 * (magic::LEARN_SECONDS as usize + 1) {
                scene.magic.update(0.02);
            }
        }
        assert!(scene.magic.all_known(Temple::War));
        // Two ships far from everyone: ours and an enemy.
        let mine = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.is_some())
            .unwrap()
            .id;
        let enemy = scene
            .objects
            .iter()
            .find(|o| o.ship.is_some() && o.id != mine)
            .unwrap()
            .id;
        let human = scene.human.unwrap();
        let e = scene.object_index(enemy).unwrap();
        scene.objects[e].owner = Some((1, (human.slot + 1) % 4));
        scene
            .objects
            .retain(|o| o.ship.is_none() || o.id == mine || o.id == enemy);
        for id in [mine, enemy] {
            let i = scene.object_index(id).unwrap();
            let c = scene.objects[i].combat.as_mut().unwrap();
            c.weapons.clear();
            c.max_hull = 10_000.0;
            c.hull = 10_000.0;
        }
        let hull = |s: &Scene, id| {
            s.objects[s.object_index(id).unwrap()]
                .combat
                .as_ref()
                .unwrap()
                .hull
        };
        assert!(
            scene
                .cast_on(Spell::Lightning, mine, &data, &mut caches)
                .is_err(),
            "not on own ships"
        );
        assert!(
            scene
                .cast_on(Spell::ProtectMagic, enemy, &data, &mut caches)
                .is_err(),
            "not on enemies"
        );
        scene
            .cast_on(Spell::Lightning, enemy, &data, &mut caches)
            .unwrap();
        assert_eq!(hull(&scene, enemy), 10_000.0 - magic::LIGHTNING_DAMAGE);
        scene
            .cast_on(Spell::Flame, enemy, &data, &mut caches)
            .unwrap();
        for _ in 0..50 * 20 {
            scene.update(0.02, &data, &mut caches);
        }
        let burnt = 10_000.0 - magic::LIGHTNING_DAMAGE - magic::FLAME_DPS * magic::FLAME_SECONDS;
        assert!(
            (hull(&scene, enemy) - burnt).abs() < 2.0,
            "{}",
            hull(&scene, enemy)
        );
        // A warded enemy shrugs off lightning.
        let e = scene.object_index(enemy).unwrap();
        scene.objects[e].combat.as_mut().unwrap().protect_magic = 10.0;
        let before = hull(&scene, enemy);
        scene
            .cast_on(Spell::SmallLightning, enemy, &data, &mut caches)
            .unwrap();
        assert_eq!(hull(&scene, enemy), before);
        // Protection from weapons on our ship turns aside a volley.
        scene
            .cast_on(Spell::ProtectWeapons, mine, &data, &mut caches)
            .unwrap();
        let e = scene.object_index(enemy).unwrap();
        scene.objects[e].combat.as_mut().unwrap().weapons = vec![4; 8];
        let m = scene.object_index(mine).unwrap();
        let (mx, my) = (scene.objects[m].x, scene.objects[m].y);
        scene.objects[e].x = mx + 100.0;
        scene.objects[e].y = my;
        for _ in 0..50 * 10 {
            scene.update(0.02, &data, &mut caches);
        }
        assert_eq!(hull(&scene, mine), 10_000.0);
    }

    /// A settlement puts up a smithy next to it (needs `OPENAC_DATA`).
    #[test]
    fn settlement_builds_structures() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.objects.retain(|o| o.ship.is_none());
        scene.amber = 5000;
        let home = scene
            .objects
            .iter()
            .find(|o| {
                scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
            })
            .unwrap();
        let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
        let smithy = 0x38;
        let mut spot = None;
        'find: for r in 2..=BUILD_RADIUS {
            for dy in -r..=r {
                for dx in -r..=r {
                    if scene.check_site(home, smithy, hx + dx, hy + dy).is_ok() {
                        spot = Some((hx + dx, hy + dy));
                        break 'find;
                    }
                }
            }
        }
        let (cx, cy) = spot.expect("room for a smithy");
        assert!(
            scene.check_site(home, smithy, hx + 50, hy).is_err(),
            "too far"
        );
        scene
            .place_building(home, smithy, cx, cy, &data, &mut caches)
            .unwrap();
        assert_eq!(scene.amber, 5000 - economy::building_cost(smithy));
        assert!(scene.check_site(home, smithy, cx, cy).is_err(), "occupied");
        let bit = 1 << (smithy - SETTLEMENT);
        assert_eq!(scene.owned_buildings() & bit, 0, "not standing yet");
        for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
            scene.update(0.02, &data, &mut caches);
        }
        assert_ne!(scene.owned_buildings() & bit, 0, "the smithy stands");
        assert!(scene.constructions.is_empty());
    }

    /// A warship destroys an enemy building, and forts fire at enemy ships.
    #[test]
    fn buildings_fight() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
        let fort = scene
            .objects
            .iter()
            .position(|o| matches!(o.class, ObjectClass::Building(0x34..=0x36)) && o.is_armed())
            .expect("a fort");
        let (fx, fy, fort_id) = (
            scene.objects[fort].x,
            scene.objects[fort].y,
            scene.objects[fort].id,
        );
        let owner = scene.objects[fort].owner.unwrap();
        // An enemy ship next to the fort, alone at sea.
        let ship = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
        let ship_id = scene.objects[ship].id;
        scene
            .objects
            .retain(|o| o.ship.is_none() || o.id == ship_id);
        let i = scene.object_index(ship_id).unwrap();
        scene.objects[i].owner = Some((owner.0, owner.1 + 1));
        let water = scene
            .nav
            .nearest_water((fx / 64.0) as i32, (fy / 64.0) as i32)
            .unwrap();
        scene.objects[i].x = water.0 as f32 * 64.0 + 32.0;
        scene.objects[i].y = water.1 as f32 * 64.0 + 32.0;
        let hull = scene.objects[i].combat.as_ref().unwrap().hull;
        for _ in 0..50 * 5 {
            scene.update(0.02, &data, &mut caches);
        }
        let hit = scene
            .object_index(ship_id)
            .is_none_or(|i| scene.objects[i].combat.as_ref().unwrap().hull < hull);
        assert!(hit, "the fort fired at the ship");
        // Make the ship a strong warship and have it destroy the fort.
        let Some(i) = scene.object_index(ship_id) else {
            return;
        };
        let c = scene.objects[i].combat.as_mut().unwrap();
        c.weapons = vec![4; 8];
        c.hull = 1.0e6;
        scene.order_attack(ship_id, fort_id).unwrap();
        for _ in 0..50 * 120 {
            scene.update(0.02, &data, &mut caches);
            if scene
                .object_index(fort_id)
                .is_none_or(|i| scene.objects[i].combat.is_none())
            {
                break;
            }
        }
        let ruins = &scene.objects[scene.object_index(fort_id).expect("ruins remain")];
        assert!(
            ruins.combat.is_none() && ruins.owner.is_none(),
            "the fort was destroyed"
        );
    }

    /// Computer warships attack a human ship that sails close.
    #[test]
    fn computer_ships_attack_intruders() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
        let enemy = scene
            .objects
            .iter()
            .position(|o| !scene.is_own(o) && o.owner.is_some() && o.ship.is_some() && o.is_armed())
            .unwrap();
        let (x, y, enemy) = (
            scene.objects[enemy].x,
            scene.objects[enemy].y,
            scene.objects[enemy].id,
        );
        let mine = scene
            .objects
            .iter()
            .position(|o| scene.is_own(o) && o.ship.is_some())
            .unwrap();
        scene.objects[mine].x = x + 3.0 * 64.0;
        scene.objects[mine].y = y;
        let mine = scene.objects[mine].id;
        scene.update(0.02, &data, &mut caches);
        let i = scene.object_index(enemy).unwrap();
        assert_eq!(
            scene.objects[i].ship.as_ref().unwrap().task,
            Some(Task::Attack { target: mine })
        );
    }

    /// A boat gathers amber and delivers it (needs `OPENAC_DATA`).
    #[test]
    fn merchant_gathers_amber() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let boat = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
            .unwrap();
        let (bx, by, boat) = (boat.x, boat.y, boat.id);
        let deposit = scene
            .objects
            .iter()
            .filter(|o| o.deposit.is_some_and(|d| d.resource == Resource::Amber))
            .min_by(|a, b| {
                let d = |o: &&MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
                d(a).total_cmp(&d(b))
            })
            .unwrap();
        let (deposit, before) = (deposit.id, deposit.deposit.unwrap().remaining.unwrap());
        scene.order_gather(boat, deposit).unwrap();
        let amber = scene.amber;
        for _ in 0..60 * 600 {
            scene.update(1.0 / 60.0, &data, &mut caches);
            if scene.amber > amber {
                break;
            }
        }
        assert_eq!(scene.amber, amber + 400, "one full boat load delivered");
        let left = scene.objects[scene.object_index(deposit).unwrap()]
            .deposit
            .unwrap()
            .remaining
            .unwrap();
        assert!((before - left - 400.0).abs() < 1.0 || left < before - 400.0);
    }

    /// Sails a ship across a real mission map (needs `OPENAC_DATA`).
    #[test]
    fn ship_sails_around_islands() {
        let Some(root) = std::env::var_os("OPENAC_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut scene = Scene::load(&data, &mut Caches::default(), "miss000").unwrap();
        let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap();
        let id = ship.id;
        // Sail alone, out of reach of other players' warships.
        scene.objects.retain(|o| o.ship.is_none() || o.id == id);
        // A far-away water cell: the bottom-right-most one.
        let t = TILE_SIZE as f32;
        let target = (0..scene.nav.height as i32)
            .rev()
            .flat_map(|y| (0..scene.nav.width as i32).rev().map(move |x| (x, y)))
            .find(|&(x, y)| scene.nav.is_water(x, y))
            .unwrap();
        let (tx, ty) = (target.0 as f32 * t + t / 2.0, target.1 as f32 * t + t / 2.0);
        assert!(scene.order_move(id, tx, ty));
        for _ in 0..(60 * 600) {
            scene.update(1.0 / 60.0, &data, &mut Caches::default());
            let o = &scene.objects[scene.object_index(id).unwrap()];
            assert!(
                scene
                    .nav
                    .is_water((o.x / t).floor() as i32, (o.y / t).floor() as i32),
                "ship ran aground at {}, {}",
                o.x,
                o.y
            );
            if o.ship.as_ref().unwrap().path.is_empty() {
                break;
            }
        }
        let o = &scene.objects[scene.object_index(id).unwrap()];
        assert!(
            (o.x - tx).abs() < 1.0 && (o.y - ty).abs() < 1.0,
            "did not arrive"
        );
    }

    #[test]
    fn headings() {
        assert_eq!(heading_towards(1.0, 0.0), 0);
        assert_eq!(heading_towards(1.0, -1.0), 1);
        assert_eq!(heading_towards(0.0, -1.0), 2);
        assert_eq!(heading_towards(-1.0, 0.0), 4);
        assert_eq!(heading_towards(0.0, 1.0), 6);
        assert_eq!(heading_towards(1.0, 1.0), 7);
    }
}
