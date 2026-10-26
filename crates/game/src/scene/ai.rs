//! Computer players and the mission's victory conditions.

use super::*;

/// Seconds between the computer players' economic decisions.
pub(super) const ECONOMY_PERIOD: u32 = 5;
/// Seconds before computer players start attacking. Placeholder.
pub(super) const GRACE_SECONDS: f32 = 300.0;
/// Idle warships a computer player gathers before attacking. Placeholder.
const WAVE: usize = 3;

impl Scene {
    /// Whether objective `i` (see `OBJECTIVES`) is achieved.
    pub fn objective_met(&self, i: usize) -> bool {
        let standing = |o: &&MapObject| o.combat.is_some();
        if i == 0 {
            // No other player has a ship or building left.
            return !self.objects.iter().filter(standing).any(|o| {
                matches!(
                    o.class,
                    ObjectClass::Ship | ObjectClass::Building(_) | ObjectClass::Barbarian(_)
                ) && o.owner.is_some_and(|w| w != combat::MONSTERS)
                    && !self.is_own(o)
            });
        }
        let classes = OBJECTIVES[i].1;
        // Only wild creatures count, not ones summoned by a player.
        !self.objects.iter().filter(standing).any(|o| {
            classes.contains(&o.record.class_id) && o.owner.is_none_or(|w| w == combat::MONSTERS)
        })
    }

    /// The mission's outcome as things stand (0x448610): defeat when the
    /// player has no ship or building left, victory when every condition
    /// of the mission is met.
    pub fn judge(&self) -> Option<Outcome> {
        self.human?;
        let alive = self.objects.iter().any(|o| {
            self.is_own(o)
                && o.combat.is_some()
                && matches!(o.class, ObjectClass::Ship | ObjectClass::Building(_))
        });
        if !alive {
            return Some(Outcome::Defeat);
        }
        if !self.story.is_empty() {
            return self
                .story
                .iter()
                .all(|&g| self.goal_met(g))
                .then_some(Outcome::Victory);
        }
        let wanted: Vec<usize> = (0..OBJECTIVES.len())
            .filter(|&i| self.conditions[i])
            .collect();
        (!wanted.is_empty() && wanted.iter().all(|&i| self.objective_met(i)))
            .then_some(Outcome::Victory)
    }

    /// Computer players: idle warships hunt the human player's ships that
    /// come within sight. (A first, simple opponent; the original AI has not
    /// been studied yet.)
    pub(super) fn run_ai(&mut self) {
        const SIGHT: f32 = 10.0 * TILE_SIZE as f32;
        let human: Vec<(u32, f32, f32)> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship.is_some()
                    && self.is_seated(o.owner)
                    && o.combat.as_ref().is_none_or(|c| c.phantom <= 0.0)
            })
            .map(|o| (o.id, o.x, o.y))
            .collect();
        if human.is_empty() {
            return;
        }
        let mut orders = Vec::new();
        for o in &self.objects {
            let Some(ship) = &o.ship else { continue };
            // The mission's brain commands its own fleets.
            if self.in_fleet(o.id) {
                continue;
            }
            if o.owner.is_none() || self.is_seated(o.owner) || !o.is_armed() || ship.task.is_some()
            {
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

    /// Computer players: idle merchants gather, shipyards build merchants
    /// until there are two and then the strongest warship they can afford,
    /// and once the grace period is over, idle warships set out in waves
    /// against the human player. A stand-in for the original AI.
    pub(super) fn run_computer_economy(&mut self) {
        let Some(human) = self.human else { return };
        let human = (human.nation, human.slot);
        // Human players' funds wait in purses too, in multiplayer games.
        let owners: Vec<(u32, u32)> = self
            .purses
            .keys()
            .copied()
            .filter(|&o| !self.is_seated(Some(o)))
            .collect();
        for owner in owners {
            let mine = |o: &MapObject| o.owner == Some(owner);
            // Merchants without work gather from the nearest deposit.
            let idle_merchants: Vec<(u32, f32, f32)> = self
                .objects
                .iter()
                .filter(|o| mine(o))
                .filter_map(|o| {
                    let s = o.ship.as_ref()?;
                    (cargo_capacity(s.kind) > 0.0
                        && s.task.is_none()
                        && s.path.is_empty()
                        && s.zone.is_none()
                        && !self.in_fleet(o.id))
                    .then_some((o.id, o.x, o.y))
                })
                .collect();
            for (id, x, y) in idle_merchants {
                let dist = |o: &MapObject| (o.x - x).powi(2) + (o.y - y).powi(2);
                let deposit = self
                    .objects
                    .iter()
                    .filter(|o| {
                        o.deposit
                            .is_some_and(|d| d.remaining.is_none_or(|r| r > 0.0))
                    })
                    .min_by(|a, b| dist(a).total_cmp(&dist(b)))
                    .map(|o| o.id);
                if let Some(d) = deposit {
                    let _ = self.order_gather(id, d);
                }
            }
            // Shipyards build.
            let merchants = self
                .objects
                .iter()
                .filter(|o| {
                    mine(o)
                        && o.ship
                            .as_ref()
                            .is_some_and(|s| cargo_capacity(s.kind) > 0.0)
                })
                .count();
            let yards: Vec<u32> = self
                .objects
                .iter()
                .filter(|o| {
                    mine(o)
                        && matches!(o.class, ObjectClass::Building(SHIPYARD | DOCK))
                        && self.construction_at(o.id).is_none()
                        && self.production_at(o.id).is_none()
                })
                .map(|o| o.id)
                .collect();
            for (n, yard) in yards.into_iter().enumerate() {
                let wanted: Vec<u8> = if merchants + n < 2 {
                    vec![1, 0]
                } else {
                    let mut warships: Vec<u8> = economy::buildable_ships(owner.0)
                        .filter(|&k| k >= 2)
                        .collect();
                    warships.sort_by_key(|&k| std::cmp::Reverse(economy::ship_cost(k).0));
                    warships
                };
                if let Some(kind) = wanted
                    .into_iter()
                    .find(|&k| self.can_build_ship_for(owner, k).is_ok())
                {
                    let _ = self.start_ship(yard, kind);
                }
            }
            // Attack waves.
            if self.clock < GRACE_SECONDS {
                continue;
            }
            let fleet: Vec<u32> = self
                .objects
                .iter()
                .filter(|o| {
                    mine(o)
                        && o.is_armed()
                        && o.ship
                            .as_ref()
                            .is_some_and(|s| s.task.is_none() && s.path.is_empty())
                })
                .map(|o| o.id)
                .collect();
            if fleet.len() < WAVE {
                continue;
            }
            let Some(lead) = self.object_index(fleet[0]) else {
                continue;
            };
            let (lx, ly) = (self.objects[lead].x, self.objects[lead].y);
            let dist = |o: &MapObject| (o.x - lx).powi(2) + (o.y - ly).powi(2);
            let target = self
                .objects
                .iter()
                .filter(|o| o.owner == Some(human) && o.combat.is_some())
                .min_by(|a, b| {
                    // Buildings first, then ships.
                    (a.ship.is_some(), dist(a))
                        .partial_cmp(&(b.ship.is_some(), dist(b)))
                        .unwrap()
                })
                .map(|o| o.id);
            if let Some(target) = target {
                for ship in fleet {
                    let _ = self.order_attack(ship, target);
                }
            }
        }
    }
}
