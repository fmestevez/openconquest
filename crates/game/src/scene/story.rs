//! The campaign's story: each mission's judge (the brain's vtable +8,
//! `brain\missNNN.cpp`), what the player must do to win and must keep not
//! to lose, and what a victory carries into the next mission.

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;

use super::{Scene, OBJECTIVES};
use crate::economy::{SETTLEMENT, TOWN};
use crate::objects::{self, MapObject};

/// One objective of a campaign mission.
const JASON: u8 = 0;
const ARGUS: u8 = 5;

fn inside(c: (i32, i32), b: Cells) -> bool {
    (b.0..=b.2).contains(&c.0) && (b.1..=b.3).contains(&c.1)
}

/// Cells, inclusive: x0, y0, x1, y1.
pub type Cells = (i32, i32, i32, i32);

/// Anywhere east or south of a cell.
const FAR: i32 = i32::MAX;

/// A term of a campaign mission's judge (0x4c46c0 counting the player's
/// ships and buildings in a box, the heroes and treasures they hold).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Goal {
    /// One of `OBJECTIVES` (the barbarians cleared).
    Kill(usize),
    /// Hero `n` has joined (0x5d99, once; Argus by boarding a ship).
    Joined(u8),
    /// Hero `n` held, aboard a ship or in a settlement.
    Hero(u8),
    /// Hero `n` held, if they ever joined.
    KeptAfter(u8),
    /// A treasure of this kind aboard one of the player's ships.
    Aboard(u8),
    /// A treasure of this kind stored in a settlement or town of the
    /// player's (in the box, if any).
    Stored(u8, Option<Cells>),
    /// A treasure of this kind stored or aboard.
    Kept(u8),
    /// A settlement or town of the player's in the box.
    TownIn(Cells),
    /// That, and every hero the player holds in the box's towns.
    Assemble(Cells),
    /// The Argo afloat.
    Argo,
    /// The Argo afloat, if it ever was (miss002).
    ArgoKept,
    /// The Argo in the box.
    ArgoIn(Cells),
    /// The Argo with Jason and Argus aboard (miss002).
    ArgoWith,
    /// The Persians (3, 2) and (3, 3) without ships and settlements, or
    /// (true) without ships and any building 0x32-0x3e.
    PersiansGone(bool),
    /// No castle of the wind wizard (0x42) in the box.
    NoCastle(Cells),
    /// A ship of the player's, or their settlement woken (miss001).
    ShipOrWoken,
    /// A settlement or town of the player's in the box, once one of
    /// theirs has woken (miss004's Pylos).
    TownOnceWoken(Cells),
}

/// A campaign mission's judge: checked every `period` ticks; won when
/// every `win` term has held for 250 ticks, lost 250 ticks after a `keep`
/// term first fails (0x4c4920, the brains' judges).
pub struct Judge {
    pub period: u32,
    pub win: &'static [Goal],
    pub keep: &'static [Goal],
}

/// Each campaign mission's judge (with `Goal::Joined` added for the
/// mission's own heroes).
pub fn judge_of(mission: &str) -> Option<Judge> {
    use Goal::*;
    let j = |period, win, keep| Some(Judge { period, win, keep });
    match mission {
        "miss000" => j(125, &[Kill(0), Aboard(27)], &[TownIn((19, 0, 20, 1))]),
        "miss001" => j(
            75,
            &[
                Kill(0),
                Assemble((0, 2, 1, 3)),
                Stored(27, None),
                Hero(JASON),
            ],
            &[
                Kept(27),
                TownIn((0, 2, 1, 3)),
                KeptAfter(JASON),
                ShipOrWoken,
            ],
        ),
        "miss002" => j(
            75,
            &[TownIn((0, 2, 1, 3)), ArgoWith],
            &[
                TownIn((0, 2, 1, 3)),
                ArgoKept,
                Hero(JASON),
                KeptAfter(ARGUS),
            ],
        ),
        "miss003" => j(
            125,
            &[
                Argo,
                Assemble((0, 40, 4, 47)),
                TownIn((35, 20, 36, 21)),
                Hero(JASON),
            ],
            &[Argo, TownIn((35, 20, 36, 21)), Hero(JASON)],
        ),
        "miss004" => j(
            125,
            &[
                PersiansGone(false),
                Argo,
                Assemble((25, 5, 27, 7)),
                Hero(JASON),
            ],
            &[Argo, Hero(JASON), TownOnceWoken((25, 5, 27, 7))],
        ),
        "miss005" => j(
            125,
            &[Argo, Assemble((0, 0, 2, 2)), Hero(JASON)],
            &[Argo, Hero(JASON)],
        ),
        "miss006" => j(
            125,
            &[Argo, Assemble((0, 0, 12, 8)), Hero(JASON)],
            &[Argo, Hero(JASON)],
        ),
        "miss007" => j(
            75,
            &[TownIn((52, 1, 53, 2)), Argo, NoCastle((10, 65, 49, 75))],
            &[TownIn((52, 1, 53, 2)), Argo, Hero(JASON)],
        ),
        "miss008" => j(
            75,
            &[
                TownIn((0, 30, 1, 31)),
                ArgoIn((57, 0, FAR, 7)),
                Assemble((55, 0, 59, 2)),
            ],
            &[TownIn((0, 30, 1, 31)), Argo, Hero(JASON)],
        ),
        "miss009" => j(
            75,
            &[ArgoIn((60, 0, FAR, 4)), Assemble((60, 0, 69, 6))],
            &[Argo, Hero(JASON)],
        ),
        "miss010" => j(
            75,
            &[
                ArgoIn((50, 0, FAR, 8)),
                Assemble((50, 0, 64, 8)),
                Stored(16, Some((50, 0, 64, 8))),
            ],
            &[Argo, Hero(JASON)],
        ),
        "miss011" => j(
            125,
            &[
                Argo,
                Assemble((0, 0, 5, 5)),
                Hero(JASON),
                Stored(16, Some((0, 0, 5, 5))),
            ],
            &[Argo, Hero(JASON)],
        ),
        "miss012" => j(
            125,
            &[
                Argo,
                Assemble((2, 48, 8, 52)),
                Hero(JASON),
                Stored(16, Some((2, 48, 8, 52))),
            ],
            &[Argo, Hero(JASON)],
        ),
        "miss013" => j(
            125,
            &[
                Argo,
                Assemble((0, 58, 2, 60)),
                Stored(16, Some((0, 58, 2, 60))),
                Hero(JASON),
                PersiansGone(true),
            ],
            &[Argo, Hero(JASON), TownIn((0, 58, 2, 60))],
        ),
        _ => None,
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

impl Scene {
    /// The terms campaign mission `mission` is won by: its judge's, and
    /// its own heroes joined.
    pub(super) fn story_goals(&self, heroes: [bool; 9]) -> Vec<Goal> {
        let mut goals: Vec<Goal> = (0..9u8)
            .filter(|&h| heroes[h as usize])
            .map(Goal::Joined)
            .collect();
        if let Some(j) = judge_of(&self.mission) {
            goals.extend_from_slice(j.win);
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

    /// The player's settlements and towns anchored in `b`.
    fn towns_in(&self, b: Cells) -> impl Iterator<Item = &MapObject> {
        self.objects.iter().filter(move |o| {
            self.is_own(o)
                && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
                && o.cell.is_some_and(|c| inside(c, b))
        })
    }

    fn argo(&self) -> Option<&MapObject> {
        self.own_ships().find(|o| o.ship.as_ref().unwrap().argo)
    }

    /// The heroes the player holds, aboard or in a settlement.
    fn heroes_held(&self) -> Vec<u8> {
        let mut out: Vec<u8> = self
            .own_ships()
            .flat_map(|o| o.ship.as_ref().unwrap().heroes.iter().copied())
            .collect();
        for site in self.own_sites() {
            out.extend(self.site_heroes.get(&site).into_iter().flatten());
        }
        out
    }

    fn stored_in(&self, kind: u8, b: Option<Cells>) -> bool {
        self.held.iter().any(|(id, t)| {
            t.kind == kind
                && self.object_index(*id).is_some_and(|i| {
                    let o = &self.objects[i];
                    self.is_own(o)
                        && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
                        && b.is_none_or(|b| o.cell.is_some_and(|c| inside(c, b)))
                })
        })
    }

    pub fn goal_met(&self, goal: Goal) -> bool {
        let cell = |o: &MapObject| {
            let t = TILE_SIZE as f32;
            ((o.x / t) as i32, (o.y / t) as i32)
        };
        match goal {
            Goal::Kill(i) => self.objective_met(i),
            Goal::Joined(h) => self.joined.contains(&h),
            Goal::Hero(h) => self.heroes_held().contains(&h),
            Goal::KeptAfter(h) => !self.joined.contains(&h) || self.heroes_held().contains(&h),
            Goal::Aboard(kind) => self.own_ships().any(|o| {
                let s = o.ship.as_ref().unwrap();
                s.kind <= 9 && s.has_treasure(kind)
            }),
            Goal::Stored(kind, b) => self.stored_in(kind, b),
            Goal::Kept(kind) => self.stored_in(kind, None) || self.goal_met(Goal::Aboard(kind)),
            Goal::TownIn(b) => self.towns_in(b).next().is_some(),
            Goal::Assemble(b) => {
                let towns: Vec<u32> = self.towns_in(b).map(|o| o.id).collect();
                !towns.is_empty()
                    && self.heroes_held().iter().all(|h| {
                        towns
                            .iter()
                            .any(|t| self.site_heroes.get(t).is_some_and(|w| w.contains(h)))
                    })
            }
            Goal::Argo => self.argo().is_some(),
            Goal::ArgoKept => !self.had_argo || self.argo().is_some(),
            Goal::ArgoIn(b) => self.argo().is_some_and(|o| inside(cell(o), b)),
            Goal::ArgoWith => self.own_ships().any(|o| {
                let s = o.ship.as_ref().unwrap();
                s.argo && s.has_hero(JASON) && s.has_hero(ARGUS)
            }),
            Goal::PersiansGone(all) => !self.objects.iter().any(|o| {
                matches!(o.owner, Some((3, 2 | 3)))
                    && o.combat.as_ref().is_some_and(|c| c.hull > 0.0)
                    && match o.class {
                        ObjectClass::Ship => true,
                        ObjectClass::Building(c) if all => (0x32..=0x3e).contains(&c),
                        ObjectClass::Building(c) => c == SETTLEMENT || c == TOWN,
                        _ => false,
                    }
            }),
            Goal::ShipOrWoken => self.woken || self.own_ships().next().is_some(),
            Goal::TownOnceWoken(b) => !self.woken || self.goal_met(Goal::TownIn(b)),
            Goal::NoCastle(b) => !self.objects.iter().any(|o| {
                o.record.class_id == 0x42
                    && o.combat.as_ref().is_none_or(|c| c.hull > 0.0)
                    && inside(cell(o), b)
            }),
        }
    }

    /// The campaign mission's judge, every period (0x4c4920): the terms to
    /// keep first lose it 250 ticks after one fails, whatever follows; the
    /// terms to win must hold at every check for 250 ticks.
    pub(super) fn judge_campaign(&mut self, period: u32) -> Option<super::Outcome> {
        let judge = judge_of(&self.mission)?;
        // Heroes join once, aboard a ship.
        let aboard: Vec<u8> = self
            .own_ships()
            .flat_map(|o| o.ship.as_ref().unwrap().heroes.iter().copied())
            .collect();
        for h in aboard {
            if !self.joined.contains(&h) {
                self.joined.push(h);
            }
        }
        if self.argo().is_some() {
            self.had_argo = true;
        }
        if let Some(left) = self.losing.as_mut() {
            *left = left.saturating_sub(period);
            if *left == 0 {
                return Some(super::Outcome::Defeat);
            }
        }
        if self.story.iter().all(|&g| self.goal_met(g)) {
            self.winning += period;
            if self.winning > 250 {
                return Some(super::Outcome::Victory);
            }
        } else {
            self.winning = 0;
        }
        if self.losing.is_none() && judge.keep.iter().any(|&g| !self.goal_met(g)) {
            self.losing = Some(250);
        }
        None
    }
}

/// What a goal asks, for the objectives line.
pub fn goal_name(goal: Goal) -> String {
    let hero = |h: u8| objects::HERO_NAMES[h as usize % 9];
    match goal {
        Goal::Kill(i) => OBJECTIVES[i].0.to_string(),
        Goal::Joined(h) => format!("Win over {}", hero(h)),
        Goal::Hero(h) | Goal::KeptAfter(h) => format!("Keep {}", hero(h)),
        // In the campaign the scroll is Lemnius's parchment.
        Goal::Aboard(27) => "Take the parchment aboard".into(),
        Goal::Aboard(kind) => format!(
            "Take aboard the {}",
            objects::treasure_name(kind).to_lowercase()
        ),
        Goal::Stored(27, _) | Goal::Kept(27) => "Bring the parchment to your town".into(),
        Goal::Stored(kind, _) | Goal::Kept(kind) => format!(
            "Bring the {} to your town",
            objects::treasure_name(kind).to_lowercase()
        ),
        Goal::TownIn(_) => "Hold your town".into(),
        Goal::Assemble(_) => "Bring every hero to your settlement".into(),
        Goal::Argo | Goal::ArgoKept => "Keep the Argo".into(),
        Goal::ArgoIn(_) => "Bring the Argo there".into(),
        Goal::ArgoWith => "Build the Argo; Jason and Argus aboard".into(),
        Goal::PersiansGone(_) => "Defeat the Persians".into(),
        Goal::NoCastle(_) => "Overcome the wizard of wind".into(),
        Goal::ShipOrWoken => "Keep a ship till your settlement wakes".into(),
        Goal::TownOnceWoken(_) => "Hold Pylos".into(),
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
            // No royal ship: one launched for the Argo (the campaign would
            // have brought it).
            None if !any_ship || self.mission.as_str() > "miss003" => {
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

    /// A mission picked from the list whose map holds no town of the
    /// player's (the original only reaches it carrying one): a bare town
    /// where the last victory would have put it. Its id, if one was made.
    pub(super) fn bare_town(
        &mut self,
        data: &crate::assets::GameData,
        caches: &mut crate::objects::Caches,
    ) -> Option<u32> {
        if !self.own_sites().is_empty() {
            return None;
        }
        let cell = match self.mission.as_str() {
            // miss001's buildings stay where they stood.
            "miss002" => (0, 2),
            "miss005" => (32, 34),
            "miss006" => (53, 67),
            "miss007" => (52, 1),
            "miss009" => (2, 37),
            "miss010" => (2, 40),
            // miss010's town lands at (x - 2, y + 26) of where it stood in
            // x >= 50, y <= 8: the first place a town can stand there.
            "miss011" => {
                let w = self.nav.width as i32;
                (26..=34)
                    .flat_map(|y| (48..w).map(move |x| (x, y)))
                    .find(|&(x, y)| self.check_ground(SETTLEMENT, x, y).is_ok())?
            }
            "miss012" => (43, 1),
            _ => return None,
        };
        let town = Carried {
            class: TOWN,
            cell,
            facing: 0,
            ship: None,
            combat: None,
            treasures: Vec::new(),
        };
        let carry = Carry {
            levels: Default::default(),
            magic: self.magic.clone(),
            permits: self.permits,
            score: 0,
            objects: vec![town],
            heroes: Vec::new(),
        };
        let heroes = std::mem::take(&mut self.carried);
        self.arrive(&carry, data, caches);
        self.carried = heroes;
        self.own_sites().first().copied()
    }

    /// A mission picked from the list gets the treasures the campaign
    /// would have brought: miss001 the boat with Lemnius's parchment (what
    /// miss000's victory carries to (1, 13)), miss011-miss013 the Golden
    /// Fleece aboard the Argo.
    pub(super) fn list_treasures(
        &mut self,
        argo: Option<usize>,
        data: &crate::assets::GameData,
        caches: &mut crate::objects::Caches,
    ) {
        let owner = self.human.map(|h| (h.nation, h.slot));
        match self.mission.as_str() {
            "miss001" => {
                let Some(owner) = owner else {
                    return;
                };
                let Some(id) = self.spawn_ship(owner, 0, 1, 13, data, caches) else {
                    return;
                };
                let i = self.object_index(id).unwrap();
                self.objects[i]
                    .ship
                    .as_mut()
                    .unwrap()
                    .treasures
                    .push(objects::Treasure {
                        kind: 27,
                        param: 0,
                        amount: 0,
                    });
            }
            "miss011" | "miss012" | "miss013" => {
                if let Some(i) = argo {
                    self.objects[i]
                        .ship
                        .as_mut()
                        .unwrap()
                        .treasures
                        .push(objects::Treasure {
                            kind: 16,
                            param: 0,
                            amount: 0,
                        });
                }
            }
            _ => {}
        }
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
