//! The mission's victory conditions (`Ai\Boss_kib.cpp`, 0x448610: judged
//! every 250 ticks). Computer players are run by the campaign's mission
//! brains (`brain`) in the campaign and by `AIPlayerClassTy`
//! (`computer`) in battles and single missions.

use super::*;

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

    /// A Custom game with no human player (`[game+0x3212]` 0xff) is over
    /// (0x448610, game type 2, no side-1 player): each player whose side
    /// is not 0 (every row of the Custom screen but the disabled ones; the
    /// barbarians have none) counted by 0x4c46c0 with ship mask 0xfff and
    /// building mask 0x1fff (its ships and its buildings 0x32-0x3e still
    /// standing); over when at most one of them has any.
    pub fn spectators_done(&self) -> bool {
        let players: std::collections::BTreeSet<(u32, u32)> = self
            .purses
            .keys()
            .copied()
            .chain(self.seats.iter().map(|s| (s.player.nation, s.player.slot)))
            .filter(|&(nation, slot)| nation != 4 && !self.computers.off.contains(&(nation, slot)))
            .collect();
        let owning = players
            .iter()
            .filter(|&&p| {
                self.objects.iter().any(|o| {
                    o.owner == Some(p)
                        && o.combat.is_some()
                        && matches!(o.class, ObjectClass::Ship | ObjectClass::Building(_))
                })
            })
            .count();
        owning <= 1
    }

    /// A battle's or single mission's outcome as things stand (0x448610):
    /// defeat when the player has no ship or building left, victory when
    /// every condition of the mission is met. (Campaign missions have
    /// their own judges, `judge_campaign`.)
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
}
