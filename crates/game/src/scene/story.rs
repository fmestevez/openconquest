//! The campaign's story objectives. The original keeps them in per-mission
//! scripts compiled into the executable (`brain\missNNN.cpp`, not yet
//! ported; their `CONDITIONS` only ever say "kill all enemies"), so they are
//! this port's own reading of each briefing: the heroes to win over come
//! from the mission's `HERO_ARR`, the rest from the briefing's "Your
//! objectives". Goals that need something the map lacks (the scripts create
//! the Argo, Jason and the Golden Fleece as the story goes) are left out.

use openac_formats::map::TILE_SIZE;
use openac_formats::ObjectClass;

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
        let flags = openac_formats::mission::heroes(&archive).unwrap_or_default();
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
}

impl Scene {
    /// The record to carry into the next campaign mission.
    pub fn carry(&self) -> Carry {
        let mut magic = self.magic.clone();
        magic.stop_learning();
        Carry {
            levels: self.levels_of(self.human.map(|h| (h.nation, h.slot))),
            magic,
            permits: self.permits,
            score: self.stats.earlier + self.stats.score(),
        }
    }

    /// Takes up the campaign where `carry` left it: what was learned is
    /// kept, and what this mission reveals is added.
    pub fn resume(&mut self, carry: &Carry) {
        if let Some(h) = self.human {
            self.levels.insert((h.nation, h.slot), carry.levels);
        }
        self.magic = carry.magic.clone();
        self.permits.0 |= carry.permits.0;
        self.stats.earlier = carry.score;
    }
}
