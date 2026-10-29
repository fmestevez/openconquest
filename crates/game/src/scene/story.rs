//! The campaign's story objectives. The original keeps them in per-mission
//! scripts compiled into the executable (`brain\missNNN.cpp`, not yet
//! ported; their `CONDITIONS` only ever say "kill all enemies"), so they are
//! this port's own reading of each briefing: the heroes to win over come
//! from the mission's `HERO_ARR`, the rest from the briefing's "Your
//! objectives". Goals that need something the map lacks (the scripts create
//! the Argo, Jason and the Golden Fleece as the story goes) are left out.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;

use super::{Scene, OBJECTIVES};
use crate::economy::{SETTLEMENT, TOWN};
use crate::objects::{self, MapObject};

/// One objective of a campaign mission.
const JASON: u8 = 0;
const ARGUS: u8 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Goal {
    /// One of `OBJECTIVES` (kill all enemies, the Gorgon, a wizard, ...).
    Kill(usize),
    /// Hero `n` aboard one of the player's ships.
    Recruit(u8),
    /// A treasure of this kind aboard one of the player's ships.
    Hold(u8),
    /// A settlement founded during the mission.
    Settle,
    /// The mission's heroes at one of the player's settlements or towns
    /// (one founded during the mission, if `Settle` is also asked).
    Assemble,
    /// The Argo built, with Jason and Argus aboard (miss002).
    Argo,
}

/// What each campaign mission asks besides its heroes, from its briefing.
fn briefing_goals(mission: &str) -> &'static [Goal] {
    use Goal::*;
    match mission {
        // "Kill all Barbarians. Find and get the parchment on board."
        "miss000" => &[Kill(0), Hold(27)],
        // "Defeat barbarians. Find the magic oar. Bring Jason to your town."
        "miss001" => &[Kill(0), Assemble],
        // "Defeat the barbarians. Find Argus's hammer and bring Argus to
        // your town. Build the Argo ship and put Argus and Jason on board."
        "miss002" => &[Kill(0), Argo],
        // "... Build a settlement in the south-west and bring ... there."
        "miss003" | "miss005" | "miss006" => &[Settle, Assemble],
        // "Kill the Gorgon. Defeat the Persians. Bring ... to Pylos."
        "miss004" => &[Kill(5), Kill(0), Assemble],
        // "Overcome the wizard of wind."
        "miss007" => &[Kill(3)],
        // "Find the town of Propontis ..." (the town is the script's).
        "miss008" => &[Kill(0)],
        // "Overcome the wizard of fire. ... build a settlement ..."
        "miss009" => &[Kill(2), Settle],
        // "Get the Golden Fleece (guarded by a dragon). Build a settlement."
        "miss010" => &[Kill(1), Hold(16), Settle],
        "miss011" => &[Settle],
        // "Destroy the Persian town on the south-west. Build a settlement."
        "miss012" => &[Kill(0), Settle],
        "miss013" => &[Kill(0)],
        _ => &[],
    }
}

/// The heroes the campaign has won over before `mission`: those of every
/// earlier mission's `HERO_ARR`. (The original carries them over in its
/// campaign state and puts them in the player's starting settlement, the
/// brains' message 2; a mission started from the menu gets the heroes of
/// the story so far.)
pub fn carried_heroes(data: &crate::assets::GameData, mission: &str) -> Vec<u8> {
    let Ok(n) = mission[4..].parse::<u32>() else {
        return Vec::new();
    };
    let mut heroes = Vec::new();
    for m in 0..n {
        let Ok(archive) = data.open_mission(&format!("miss{m:03}")) else {
            continue;
        };
        let flags = openconquest_formats::mission::heroes(&archive).unwrap_or_default();
        for h in 0..9u8 {
            if flags[h as usize] && !heroes.contains(&h) {
                heroes.push(h);
            }
        }
    }
    heroes
}

/// How near (cells) heroes must be to a settlement to count as there.
const ASSEMBLE_REACH: f32 = 3.0;

impl Scene {
    /// The goals of campaign mission `mission`: its heroes to win over and
    /// its briefing's objectives, keeping only those the map allows.
    pub(super) fn story_goals(&self, heroes: [bool; 9]) -> Vec<Goal> {
        let mut goals: Vec<Goal> = (0..9u8)
            .filter(|&h| heroes[h as usize])
            .map(Goal::Recruit)
            .collect();
        for &g in briefing_goals(&self.mission) {
            let possible = match g {
                // A creature goal needs the creature on the map.
                Goal::Kill(i) if i > 0 => self
                    .objects
                    .iter()
                    .any(|o| OBJECTIVES[i].1.contains(&o.record.class_id)),
                Goal::Hold(kind) => {
                    self.objects
                        .iter()
                        .any(|o| o.treasure.is_some_and(|t| t.kind == kind))
                        || self.held.iter().any(|h| h.1.kind == kind)
                }
                Goal::Assemble => !goals.is_empty() || !self.carried.is_empty(),
                _ => true,
            };
            if possible {
                goals.push(g);
            }
        }
        goals
    }

    /// The player's settlements and towns standing now.
    pub(super) fn own_sites(&self) -> Vec<u32> {
        self.objects
            .iter()
            .filter(|o| {
                self.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
            })
            .map(|o| o.id)
            .collect()
    }

    fn own_ships(&self) -> impl Iterator<Item = &MapObject> {
        self.objects
            .iter()
            .filter(|o| o.ship.is_some() && self.is_own(o))
    }

    pub fn goal_met(&self, goal: Goal) -> bool {
        match goal {
            Goal::Kill(i) => self.objective_met(i),
            Goal::Recruit(h) => self
                .own_ships()
                .any(|o| o.ship.as_ref().unwrap().has_hero(h)),
            // Aboard, or already handed over at a settlement.
            Goal::Hold(kind) => {
                self.delivered.contains(&kind)
                    || self
                        .own_ships()
                        .any(|o| o.ship.as_ref().unwrap().has_treasure(kind))
            }
            Goal::Settle => self.new_sites().next().is_some(),
            Goal::Argo => self.own_ships().any(|o| {
                let s = o.ship.as_ref().unwrap();
                s.argo && s.has_hero(JASON) && s.has_hero(ARGUS)
            }),
            Goal::Assemble => {
                // This mission's heroes and those the campaign brought.
                let mut heroes: Vec<u8> = self
                    .story
                    .iter()
                    .filter_map(|g| match g {
                        Goal::Recruit(h) => Some(*h),
                        _ => None,
                    })
                    .collect();
                heroes.extend(self.carried.iter().copied());
                let reach = ASSEMBLE_REACH * TILE_SIZE as f32;
                let sites: Vec<&MapObject> = if self.story.contains(&Goal::Settle) {
                    self.new_sites().collect()
                } else {
                    self.own_sites()
                        .into_iter()
                        .filter_map(|id| self.object_index(id).map(|i| &self.objects[i]))
                        .collect()
                };
                sites.iter().any(|site| {
                    let waiting = self.site_heroes.get(&site.id);
                    heroes.iter().all(|&h| {
                        waiting.is_some_and(|w| w.contains(&h))
                            || self.own_ships().any(|o| {
                                o.ship.as_ref().unwrap().has_hero(h)
                                    && ((o.x - site.x).powi(2) + (o.y - site.y).powi(2)).sqrt()
                                        <= reach
                            })
                    })
                })
            }
        }
    }

    /// Settlements and towns founded during the mission.
    fn new_sites(&self) -> impl Iterator<Item = &MapObject> {
        self.objects.iter().filter(|o| {
            self.is_own(o)
                && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
                && !self.start_sites.contains(&o.id)
        })
    }
}

/// What a goal asks, for the objectives line.
pub fn goal_name(goal: Goal) -> String {
    match goal {
        Goal::Kill(i) => OBJECTIVES[i].0.to_string(),
        Goal::Recruit(h) => format!("Win over {}", objects::HERO_NAMES[h as usize % 9]),
        // In the campaign the scroll is Lemnius's parchment.
        Goal::Hold(27) => "Take the parchment aboard".into(),
        Goal::Hold(kind) => format!(
            "Take aboard the {}",
            objects::treasure_name(kind).to_lowercase()
        ),
        Goal::Settle => "Found a new settlement".into(),
        Goal::Assemble => "Bring the heroes to your settlement".into(),
        Goal::Argo => "Build the Argo; Jason and Argus aboard".into(),
    }
}

impl Scene {
    /// The Argo, from miss003 on: the player's royal ship nearest where
    /// the mission's view starts, or, with no ship at all (miss003, where
    /// the original's script brings it), a royal ship launched on the water
    /// nearest that view. Its index.
    pub(super) fn ready_argo(
        &mut self,
        data: &crate::assets::GameData,
        caches: &mut crate::objects::Caches,
    ) -> Option<usize> {
        const ROYAL_SHIP: u8 = 2;
        let human = self.human?;
        let owner = Some((human.nation, human.slot));
        let (vx, vy) = self.start_view();
        let d = |o: &MapObject| (o.x - vx).powi(2) + (o.y - vy).powi(2);
        let mine = |o: &&MapObject| o.owner == owner && o.ship.is_some();
        let any_ship = self.objects.iter().any(|o| mine(&o));
        let found = self
            .objects
            .iter()
            .enumerate()
            .filter(|(_, o)| mine(o) && o.ship.as_ref().unwrap().kind == ROYAL_SHIP)
            .min_by(|a, b| d(a.1).total_cmp(&d(b.1)))
            .map(|(i, _)| i);
        let i = match found {
            Some(i) => i,
            None if !any_ship => {
                let t = TILE_SIZE as f32;
                let (cx, cy) = self.nav.nearest_water((vx / t) as i32, (vy / t) as i32)?;
                let id = self.spawn_ship(owner?, ROYAL_SHIP, cx, cy, data, caches)?;
                self.object_index(id)?
            }
            None => return None,
        };
        let o = &mut self.objects[i];
        o.ship.as_mut().unwrap().argo = true;
        o.label = "Argo".into();
        Some(i)
    }
}

/// What the campaign carries from one mission to the next (the original's
/// record at game + 0x3361, restored by 0x4a9740): the player's research,
/// the spells learned and the technology revealed.
#[derive(Debug, Clone)]
pub struct Carry {
    levels: crate::combat::Levels,
    magic: crate::magic::Magic,
    permits: crate::permits::Permits,
    score: u64,
    /// The ships and buildings carried into the next mission's map
    /// (message 0x8d2 to each, at its cell there), and where each hero
    /// arrives (+0x33be).
    objects: Vec<Carried>,
    heroes: Vec<(u8, HeroSpot)>,
}

/// A ship or building carried on (the ship's blob 0x4e5ab0: type, the
/// Argo, its hull, levels, units and treasures; a town's 0x4416c0 cut by
/// 0x4451f0: its class, condition and treasures, its stock emptied).
#[derive(Debug, Clone)]
struct Carried {
    class: u32,
    cell: (i32, i32),
    facing: u32,
    ship: Option<crate::objects::Ship>,
    combat: Option<crate::combat::Combatant>,
    treasures: Vec<crate::objects::Treasure>,
}

/// Where a carried hero arrives: the building on a cell, or the Argo.
#[derive(Debug, Clone, Copy, PartialEq)]
enum HeroSpot {
    Cell((i32, i32)),
    Argo,
}

impl Scene {
    /// The record to carry into the next campaign mission.
    pub fn carry(&self) -> Carry {
        let mut magic = self.magic.clone();
        magic.stop_learning();
        let (objects, heroes) = self.carried_on();
        Carry {
            levels: self.levels_of(self.human.map(|h| (h.nation, h.slot))),
            magic,
            permits: self.permits,
            score: self.stats.earlier + self.stats.score(),
            objects,
            heroes,
        }
    }

    /// What this mission's victory carries on (each brain's vtable +0xc):
    /// the Argo, the town and its buildings by the mission, each to a cell
    /// of the next map; and the heroes there.
    fn carried_on(&self) -> (Vec<Carried>, Vec<(u8, HeroSpot)>) {
        let n: u32 = self
            .mission
            .get(4..)
            .and_then(|s| s.parse().ok())
            .unwrap_or(99);
        let mut objects = Vec::new();
        let mut heroes = Vec::new();
        if !super::is_campaign(&self.mission) {
            return (objects, heroes);
        }
        let own: Vec<usize> = (0..self.objects.len())
            .filter(|&i| self.is_own(&self.objects[i]))
            .collect();
        let argo = own
            .iter()
            .copied()
            .find(|&i| self.objects[i].ship.as_ref().is_some_and(|s| s.argo));
        let cell = |i: usize| -> (i32, i32) {
            let o = &self.objects[i];
            o.cell.unwrap_or((
                (o.x / TILE_SIZE as f32) as i32,
                (o.y / TILE_SIZE as f32) as i32,
            ))
        };
        let inside = |c: (i32, i32), x: (i32, i32), y: (i32, i32)| {
            (x.0..=x.1).contains(&c.0) && (y.0..=y.1).contains(&c.1)
        };
        let is_town = |i: usize| {
            matches!(
                self.objects[i].class,
                ObjectClass::Building(SETTLEMENT | TOWN)
            )
        };
        let towns_in = |x: (i32, i32), y: (i32, i32)| -> Vec<usize> {
            own.iter()
                .copied()
                .filter(|&i| is_town(i) && inside(cell(i), x, y))
                .collect()
        };
        let carry = |i: usize, to: (i32, i32), objects: &mut Vec<Carried>| {
            let o = &self.objects[i];
            let ship = o.ship.clone().map(|mut s| {
                s.path.clear();
                s.task = None;
                s.zone = None;
                s.heroes.clear();
                s.cargo = 0.0;
                s.cargo_kind = None;
                s.heading = 0;
                s
            });
            let treasures = self
                .held
                .iter()
                .filter(|(id, _)| *id == o.id)
                .map(|(_, t)| *t)
                .collect();
            objects.push(Carried {
                class: o.record.class_id,
                cell: to,
                facing: o.record.fields.get(10).copied().unwrap_or(0),
                ship,
                combat: o.combat.clone(),
                treasures,
            });
        };
        let town_heroes = |i: usize| {
            self.site_heroes
                .get(&self.objects[i].id)
                .cloned()
                .unwrap_or_default()
        };
        let argo_to = |to: (i32, i32), objects: &mut Vec<Carried>| {
            if let Some(a) = argo {
                carry(a, to, objects);
            }
        };
        // A town carried with its heroes.
        let town = |x: (i32, i32),
                    y: (i32, i32),
                    to: (i32, i32),
                    objects: &mut Vec<Carried>,
                    heroes: &mut Vec<(u8, HeroSpot)>| {
            if let Some(&t) = towns_in(x, y).first() {
                carry(t, to, objects);
                for h in town_heroes(t) {
                    heroes.push((h, HeroSpot::Cell(to)));
                }
            }
        };
        match n {
            // The ship with the parchment (treasure 27).
            0 => {
                if let Some(&i) = own.iter().find(|&&i| {
                    self.objects[i]
                        .ship
                        .as_ref()
                        .is_some_and(|s| s.treasures.iter().any(|t| t.kind == 27))
                }) {
                    carry(i, (1, 13), &mut objects);
                }
            }
            // Every building in (0..13, 0..12), where it stands.
            1 => {
                for &i in &own {
                    let c = cell(i);
                    if self.objects[i].ship.is_none()
                        && self.objects[i].cell.is_some()
                        && inside(c, (0, 13), (0, 12))
                    {
                        carry(i, c, &mut objects);
                        if is_town(i) {
                            for h in town_heroes(i) {
                                heroes.push((h, HeroSpot::Cell(c)));
                            }
                        }
                    }
                }
            }
            2 => {
                argo_to((9, 8), &mut objects);
                if let Some(a) = argo {
                    for &h in &self.objects[a].ship.as_ref().unwrap().heroes {
                        heroes.push((h, HeroSpot::Argo));
                    }
                }
            }
            3 => {
                argo_to((45, 10), &mut objects);
                town((0, 4), (40, 47), (46, 6), &mut objects, &mut heroes);
            }
            4 => {
                argo_to((31, 31), &mut objects);
                town((25, 27), (5, 7), (32, 34), &mut objects, &mut heroes);
            }
            5 => {
                argo_to((50, 64), &mut objects);
                town((0, 2), (0, 2), (53, 67), &mut objects, &mut heroes);
            }
            6 => {
                argo_to((49, 3), &mut objects);
                town((0, 12), (0, 8), (52, 1), &mut objects, &mut heroes);
            }
            // Everyone aboard or ashore to the town at (0, 30).
            7 => {
                argo_to((4, 33), &mut objects);
                for &i in &own {
                    let on: Vec<u8> = match &self.objects[i].ship {
                        Some(s) => s.heroes.clone(),
                        None if is_town(i) => town_heroes(i),
                        None => Vec::new(),
                    };
                    heroes.extend(on.into_iter().map(|h| (h, HeroSpot::Cell((0, 30)))));
                }
            }
            8 => {
                argo_to((5, 39), &mut objects);
                for &i in &own {
                    let c = cell(i);
                    if self.objects[i].ship.is_none()
                        && self.objects[i].cell.is_some()
                        && c.0 >= 56
                        && c.1 <= 8
                    {
                        carry(i, (c.0 - 56, c.1 + 37), &mut objects);
                    }
                    let on: Vec<u8> = match &self.objects[i].ship {
                        Some(s) => s.heroes.clone(),
                        None if is_town(i) => town_heroes(i),
                        None => Vec::new(),
                    };
                    heroes.extend(on.into_iter().map(|h| (h, HeroSpot::Cell((2, 37)))));
                }
            }
            9 => {
                if let Some(a) = argo.filter(|&a| cell(a).0 >= 60 && cell(a).1 <= 6) {
                    carry(a, (3, 37), &mut objects);
                }
                town(
                    (60, i32::MAX),
                    (i32::MIN, 6),
                    (2, 40),
                    &mut objects,
                    &mut heroes,
                );
            }
            10 => {
                for &i in &own {
                    let c = cell(i);
                    let fixed = self.objects[i].ship.is_none() && self.objects[i].cell.is_some();
                    if (fixed || Some(i) == argo) && c.0 >= 50 && c.1 <= 8 {
                        let to = (c.0 - 2, c.1 + 26);
                        carry(i, to, &mut objects);
                        if is_town(i) {
                            for h in town_heroes(i) {
                                heroes.push((h, HeroSpot::Cell(to)));
                            }
                        }
                    }
                }
            }
            11 => {
                argo_to((47, 2), &mut objects);
                town((0, 5), (0, 5), (43, 1), &mut objects, &mut heroes);
            }
            12 => {
                argo_to((42, 7), &mut objects);
                town((2, 8), (48, 52), (41, 2), &mut objects, &mut heroes);
            }
            _ => {}
        }
        (objects, heroes)
    }

    /// Puts what the campaign carried into this mission's map (0x522d50
    /// recreating the blobs, 0x4dfa90 a ship's), before its brain starts:
    /// the ships and buildings at their cells, and the heroes in the
    /// building on their cell or aboard the Argo. True if it did.
    pub(super) fn arrive(
        &mut self,
        carry: &Carry,
        data: &crate::assets::GameData,
        caches: &mut crate::objects::Caches,
    ) -> bool {
        let Some(human) = self.human else {
            return false;
        };
        if carry.objects.is_empty() && carry.heroes.is_empty() {
            return false;
        }
        let owner = (human.nation, human.slot);
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let mut argo = None;
        for c in &carry.objects {
            if c.cell.0 < 0 || c.cell.1 < 0 || c.cell.0 >= w || c.cell.1 >= h {
                continue;
            }
            let id = if let Some(ship) = &c.ship {
                let Some(id) = self.spawn_ship(owner, ship.kind, c.cell.0, c.cell.1, data, caches)
                else {
                    continue;
                };
                let i = self.object_index(id).unwrap();
                let o = &mut self.objects[i];
                let mut s = ship.clone();
                s.heading = 0;
                o.ship = Some(s);
                if let Some(cb) = &c.combat {
                    o.combat = Some(cb.clone());
                }
                if ship.argo {
                    o.label = "Argo".into();
                    argo = Some(id);
                }
                id
            } else {
                let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
                let mut fields = vec![0u32; 66];
                fields[0] = c.class;
                fields[1] = 1;
                fields[2] = owner.0;
                fields[3] = owner.1;
                fields[5] = c.cell.0 as u32;
                fields[6] = c.cell.1 as u32;
                fields[7] = 100;
                fields[10] = c.facing;
                let rec = openconquest_formats::ObjectRecord {
                    id,
                    class_id: c.class,
                    fields,
                };
                let Ok(Some(mut obj)) = crate::objects::resolve(data, caches, &rec) else {
                    continue;
                };
                // Its condition carried; a town's stock and marines not.
                if let (Some(cb), Some(old)) = (obj.combat.as_mut(), c.combat.as_ref()) {
                    let share = (old.hull / old.max_hull.max(1.0)).clamp(0.0, 1.0);
                    cb.hull = cb.max_hull * share;
                    if !matches!(c.class, SETTLEMENT | TOWN) {
                        cb.weapons = old.weapons.clone();
                        cb.conditions = old.conditions.clone();
                    }
                }
                self.objects.push(obj);
                self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
                id
            };
            for t in &c.treasures {
                self.held.push((id, *t));
            }
        }
        let mut placed = Vec::new();
        for &(hero, spot) in &carry.heroes {
            match spot {
                HeroSpot::Argo => {
                    if let Some(i) = argo.and_then(|a| self.object_index(a)) {
                        self.objects[i].ship.as_mut().unwrap().heroes.push(hero);
                        placed.push(hero);
                    }
                }
                HeroSpot::Cell(c) => {
                    let site = self
                        .objects
                        .iter()
                        .find(|o| {
                            self.is_own(o)
                                && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
                                && o.cell.is_some_and(|(x, y)| {
                                    (x..x + 2).contains(&c.0) && (y..y + 2).contains(&c.1)
                                })
                        })
                        .map(|o| o.id);
                    if let Some(site) = site {
                        self.site_heroes.entry(site).or_default().push(hero);
                        placed.push(hero);
                    }
                }
            }
        }
        self.carried = placed;
        true
    }

    /// Takes up the campaign where `carry` left it: what was learned is
    /// kept, and what this mission reveals is added.
    pub fn resume(&mut self, carry: &Carry) {
        if let Some(h) = self.human {
            self.levels.insert((h.nation, h.slot), carry.levels);
        }
        self.magic = carry.magic.clone();
        // The carried permissions replace the mission's, and only its own
        // grants are added: scrolls not delivered stay unread.
        let n = self
            .mission
            .get(4..)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        self.permits = crate::permits::Permits::continued(carry.permits, n);
        self.stats.earlier = carry.score;
    }
}
