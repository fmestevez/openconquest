//! The player's statistics for the mission, shown with its outcome as the
//! original does (0x478e30 counts them at the end into game + 0x3309; the
//! screen 0x539b20 lists them with string resources 1047-1058).

use openconquest_formats::ObjectClass;

use super::Scene;
use crate::combat;

/// Points per ship type destroyed, boarded or lost (0x59b358: the ships'
/// amber costs).
const SHIP_POINTS: [i64; 12] = [200, 400, 1100, 1300, 900, 800, 700, 600, 600, 900, 600, 900];
/// Points per kind of monster killed (0x59b358-0x59b374), in the order of
/// the creature victory conditions (taken to be the original's order):
/// dragons, wizards of fire, of wind, sirens, gorgons, harpies, sea
/// creatures, amazons, minotaurs, cyclops.
const MONSTER_POINTS: [i64; 10] = [700, 400, 200, 200, 600, 1100, 1300, 800, 200, 900];

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Stats {
    pub ships_destroyed: [u32; 12],
    pub ships_boarded: [u32; 12],
    pub buildings_destroyed: u32,
    pub monsters_killed: [u32; 10],
    pub lairs_destroyed: u32,
    pub ships_lost: [u32; 12],
    pub buildings_lost: u32,
    pub fish_caught: u64,
    pub amber_earned: u64,
    pub treasures_found: u32,
    /// The campaign's score before this mission.
    pub earlier: u64,
}

impl Stats {
    /// The total score (0x478e30): destroyed and boarded ships and killed
    /// monsters by their points, less the ships lost; plus the amber
    /// earned, a fifth of the fish caught and three boats' worth for each
    /// treasure; a tenth of it all. (Buildings count nothing: the original
    /// looks their price up by index instead of class.)
    pub fn score(&self) -> u64 {
        let ships =
            |n: &[u32; 12]| -> i64 { n.iter().zip(SHIP_POINTS).map(|(&n, p)| n as i64 * p).sum() };
        let monsters: i64 = self
            .monsters_killed
            .iter()
            .zip(MONSTER_POINTS)
            .map(|(&n, p)| n as i64 * p)
            .sum();
        let total = ships(&self.ships_destroyed) + ships(&self.ships_boarded) + monsters
            - ships(&self.ships_lost)
            + self.amber_earned as i64
            + self.fish_caught as i64 * 20 / 100
            + self.treasures_found as i64 * SHIP_POINTS[0] * 3;
        (total.max(0) / 10) as u64
    }

    /// The twelve figures of the end screen, in the original's order
    /// (game + 0x3309 to + 0x3335, read by 0x539a30): enemy ships
    /// destroyed, boarded, enemy terrain objects destroyed, monsters
    /// killed, monsters' terrain objects destroyed, ships lost, terrain
    /// objects lost, fish caught, amber earned, amber left, treasures found
    /// and the total score, the campaign's so far (+ 0x3335 adds each
    /// mission's, cleared when the campaign starts, 0x537f60).
    pub fn values(&self, amber_left: u32) -> [u64; 12] {
        let sum = |n: &[u32]| n.iter().map(|&n| u64::from(n)).sum::<u64>();
        [
            sum(&self.ships_destroyed),
            sum(&self.ships_boarded),
            u64::from(self.buildings_destroyed),
            sum(&self.monsters_killed),
            u64::from(self.lairs_destroyed),
            sum(&self.ships_lost),
            u64::from(self.buildings_lost),
            self.fish_caught,
            self.amber_earned,
            u64::from(amber_left),
            u64::from(self.treasures_found),
            self.earlier + self.score(),
        ]
    }
}

/// The monster kind (index into `MONSTER_POINTS`) of a creature class.
fn monster_kind(class: u32) -> Option<usize> {
    Some(match class {
        0x1f9 => 0,
        0x43 => 1,
        0x42 => 2,
        0x1fd => 3,
        0x1fa => 4,
        0x1f8 => 5,
        0x1fb => 6,
        0x1f6 => 7,
        0x1f5 => 8,
        0x1f4 => 9,
        _ => return None,
    })
}

impl Scene {
    /// Object `i` is destroyed: the player's gain or loss.
    pub(super) fn note_destroyed(&mut self, i: usize) {
        let o = &self.objects[i];
        let own = self.is_own(o);
        let class = o.record.class_id;
        let ship = o.ship.as_ref().map(|s| s.kind.min(11) as usize);
        let monster = o.owner == Some(combat::MONSTERS) || o.owner.is_none();
        let s = &mut self.stats;
        match ship {
            Some(k) if own => s.ships_lost[k] += 1,
            Some(k) if !monster => s.ships_destroyed[k] += 1,
            Some(_) => {}
            None if own => s.buildings_lost += 1,
            None => {
                if let Some(m) = monster_kind(class) {
                    s.monsters_killed[m] += 1;
                } else if matches!(
                    o.class,
                    ObjectClass::Castle(_) | ObjectClass::HarpyNest | ObjectClass::CreatureSpawn
                ) {
                    s.lairs_destroyed += 1;
                } else if matches!(
                    o.class,
                    ObjectClass::Building(_) | ObjectClass::Barbarian(_)
                ) {
                    s.buildings_destroyed += 1;
                }
            }
        }
    }
}
