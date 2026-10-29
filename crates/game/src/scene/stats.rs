//! The player's statistics for the mission, shown with its outcome as the
//! original does (0x478e30 counts them at the end into game + 0x3309; the
//! screen 0x539b20 lists them with string resources 1047-1058).

use super::{Owner, Scene};

/// Points per ship type destroyed, boarded or lost (0x59b358: the ships'
/// amber costs).
const SHIP_POINTS: [i64; 12] = [200, 400, 1100, 1300, 900, 800, 700, 600, 600, 900, 600, 900];
/// Points per monster killed, by class - 0x1f4 (the jump table 0x479140
/// into 0x59b358-0x59b374): cyclops 700, minotaur 400, amazon 200, 0x1f7
/// 200, harpy 600, dragon 1100, gorgon 1300, sea creature 800, 0x1fc 200,
/// siren 900. (Class 0x1fe is counted, `+0x7c8`, but neither shown nor
/// scored.)
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
    /// looks their price up (0x42d8b0) by index instead of class.)
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

impl Scene {
    /// Object `i` is destroyed, the blow that did it given by `killer`
    /// (0x4a9250 through 0x50e750, message 0x83): what it does to the human
    /// player's statistics. A thing destroyed by its own player counts
    /// nothing; else its player's loss is counted (ships, and buildings of
    /// classes 0x32-0x43), and the killer's gain (ships by type, buildings
    /// 0x32-0x41 as enemy terrain objects, 0x42-0x49, the wizards and the
    /// lairs, as monsters' terrain objects, monsters 0x1f4-0x1fd by
    /// class); only the human player's own are kept here. (Not decoded:
    /// the killer each of the 25 callers of 0x50e750 passes; the port
    /// gives the shooter's, rammer's or boarder's player, and where it has
    /// none (`None`: spells, fire, creatures, volcanoes) counts the thing
    /// as the human player's kill unless it was theirs.)
    pub(super) fn note_destroyed(&mut self, i: usize, killer: Option<Owner>) {
        let o = &self.objects[i];
        let human = self.human.map(|h| (h.nation, h.slot));
        let own = self.is_own(o);
        if killer.is_some() && killer == o.owner {
            return;
        }
        let by_human = match killer {
            Some(k) => Some(k) == human,
            None => !own,
        };
        let class = o.record.class_id;
        let ship = o.ship.as_ref().map(|s| s.kind.min(11) as usize);
        let s = &mut self.stats;
        match (ship, class) {
            (Some(k), _) => {
                if own {
                    s.ships_lost[k] += 1;
                }
                if by_human {
                    s.ships_destroyed[k] += 1;
                }
            }
            (None, 0x32..=0x4a) => {
                // Classes 0x44-0x49 are no player's loss; 0x4a is counted
                // in neither sum.
                if own && class <= 0x43 {
                    s.buildings_lost += 1;
                }
                if by_human {
                    match class {
                        0x32..=0x41 => s.buildings_destroyed += 1,
                        0x42..=0x49 => s.lairs_destroyed += 1,
                        _ => {}
                    }
                }
            }
            (None, 0x1f4..=0x1fd) if by_human => {
                s.monsters_killed[(class - 0x1f4) as usize] += 1;
            }
            _ => {}
        }
    }
}
