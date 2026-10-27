//! The mission's victory conditions (`Ai\Boss_kib.cpp`, 0x448610: judged
//! every 250 ticks). Computer players have no AI of their own beyond the
//! campaign's mission brains (`brain`): in battles and single missions,
//! as in the original (0x51f680 makes a brain only for the campaign),
//! their ships and forts only guard and answer fire.

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
}
