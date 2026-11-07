//! The navy's group walk (message 0, 0x49ab30 at 0x49acb6): every side's
//! groups, by their player's list.

use super::gather::ZoneGroup;
use super::Scene;

/// A group of the navy's lists, as the port keeps it.
#[derive(Debug, Clone, Copy)]
enum Group {
    /// A human's group, by its serial (0x49ca60 -> 0x49b990).
    Human(u32),
    /// Computer player `k`'s flotilla `f` (0x483e10 -> 0x4a0480 op 1).
    Flotilla(usize, usize),
    /// Brain member `k` (0x479200 -> 0x4a0480 op 1).
    Brain(usize),
}

impl Scene {
    /// The navy's tick: the 20 players' group lists (navy `+0x22 + 4p`)
    /// from p = 0 up, each by index (its count read before the walk), a
    /// land fleet (kind 0xb) by its pass 0x42b910, any other by 0x4c4f40
    /// (its block's tick alone). A group is appended to its player's list
    /// at its making (0x49b990 -> 0x5577d0, which never reuses an entry),
    /// so a list is in making order: the human's groups (0x49ca60, the
    /// navy's `+0x21` player) by serial, a computer's flotillas by their
    /// making (0x483e10: the manager's a land group), a brain's members
    /// by member within each group (0x479200 at message 0x8d1). A group
    /// made in the walk waits for the next tick (the count read before).
    pub(super) fn run_navy(&mut self) {
        self.run_berth_waits();
        self.drop_gone_attacks();
        self.drop_gone_fleet_attacks();
        self.drop_gone_waypoint_moves();
        self.drop_gone_guards();
        self.drop_gone_founds();
        for k in 0..self.computers.players.len() * self.computers.on as usize {
            let mut ai = std::mem::take(&mut self.computers.players[k]);
            self.computer_drop_gone(&mut ai);
            self.computers.players[k] = ai;
        }
        self.brain_lands_load();
        for (_, _, g) in self.navy_walk() {
            match g {
                Group::Human(s) => self.human_group_tick(s),
                Group::Flotilla(k, f) => {
                    let id = self.flotilla_groups(k)[f].0;
                    let z = self
                        .gather_zones
                        .iter()
                        .position(|z| z.group == ZoneGroup::Computer(id, f));
                    if let Some(z) = z {
                        self.gather_zone_tick(z);
                    }
                    self.flotilla_group_tick(k, f);
                }
                Group::Brain(k) => self.brain_group_tick(k),
            }
        }
        self.gather_zones
            .retain(|z| !(matches!(z.group, ZoneGroup::Human(_)) && z.ships.is_empty()));
    }

    /// The walk's groups, `(player, place in its list, group)`, in order.
    fn navy_walk(&mut self) -> Vec<(u32, u32, Group)> {
        let mut walk: Vec<(u32, u32, Group)> = Vec::new();
        let human = self.human.map_or(0, |h| h.nation * 4 + h.slot);
        let mut serials: Vec<u32> = self
            .gather_zones
            .iter()
            .filter_map(|z| match z.group {
                ZoneGroup::Human(s) => Some(s),
                _ => None,
            })
            .chain(self.human_attacks.iter().map(|a| a.serial))
            .chain(self.fleet_attacks.iter().map(|a| a.serial))
            .chain(self.waypoint_moves.iter().map(|m| m.serial))
            .chain(self.human_guards.iter().map(|g| g.serial))
            .chain(self.human_founds.iter().map(|g| g.serial))
            .chain(self.human_groups.iter().map(|g| g.serial))
            .collect();
        serials.sort_unstable();
        serials.dedup();
        walk.extend(serials.into_iter().map(|s| (human, s, Group::Human(s))));
        // The computer players' flotillas, while they run (`computers.on`).
        let players = if self.computers.on {
            self.computers.players.len()
        } else {
            0
        };
        for k in 0..players {
            for (p, f) in self.flotilla_groups(k) {
                walk.push((p, f as u32, Group::Flotilla(k, f)));
            }
        }
        for (p, member, k) in self.brain_groups() {
            // A group with no player in `group_players` (none known) last.
            let p = p.map_or(u32::MAX, |(n, s)| n * 4 + s);
            walk.push((p, member as u32, Group::Brain(k)));
        }
        walk.sort_by_key(|&(p, i, _)| (p, i));
        walk
    }

    /// The human groups' serials in the walk.
    #[cfg(test)]
    pub(super) fn navy_walk_serials(&mut self) -> Vec<u32> {
        self.navy_walk()
            .into_iter()
            .filter_map(|(_, _, g)| match g {
                Group::Human(s) => Some(s),
                _ => None,
            })
            .collect()
    }

    /// `human_group_tick`, for the tests.
    #[cfg(test)]
    pub(super) fn human_group_tick_for_test(&mut self, s: u32) {
        self.human_group_tick(s);
    }

    /// A human group's block tick (0x4c4f40), whichever block it holds.
    fn human_group_tick(&mut self, s: u32) {
        if let Some(z) = self
            .gather_zones
            .iter()
            .position(|z| z.group == ZoneGroup::Human(s))
        {
            self.gather_zone_tick(z);
        }
        if let Some(z) = self.human_attacks.iter().position(|a| a.serial == s) {
            let mut a = self.human_attacks.remove(z);
            self.ha_tick(&mut a);
            self.human_attacks.insert(z, a);
        }
        if let Some(z) = self.fleet_attacks.iter().position(|a| a.serial == s) {
            let mut a = self.fleet_attacks.remove(z);
            if self.fa_tick(&mut a) {
                self.fleet_attacks.insert(z, a);
            }
        }
        if let Some(z) = self.waypoint_moves.iter().position(|m| m.serial == s) {
            self.wm_tick(z);
        }
        if let Some(z) = self.human_guards.iter().position(|g| g.serial == s) {
            let mut g = self.human_guards.remove(z);
            self.guard_tick(&mut g);
            self.human_guards.insert(z, g);
        }
        if let Some(z) = self.human_founds.iter().position(|g| g.serial == s) {
            self.human_found_tick(z);
        }
        // A bare group (block null) runs nothing; the plain move `{100,
        // 1}` its pass 0x4ccd80.
        if let Some(z) = self.human_groups.iter().position(|g| g.serial == s) {
            self.plain_move_tick(z);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::GameData;
    use crate::objects::Caches;

    fn data() -> Option<GameData> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        Some(GameData::open(std::path::Path::new(&root)).unwrap())
    }

    /// The navy's walk: on battle01 each computer player's flotillas in
    /// their making order (the manager's land group first), the players
    /// from the lowest id up; on miss004 the brain's members by their
    /// group's player (group 2's 14 first, then group 0's 15, group 1's
    /// 16), a group's by member.
    #[test]
    fn the_navy_walks_every_sides_groups_by_player_lists() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        scene.update(0.1, &data, &mut caches);
        assert!(!scene.computers.players.is_empty());
        let mut ids: Vec<(u32, usize)> = scene
            .computers
            .players
            .iter()
            .enumerate()
            .map(|(k, c)| (c.id(), k))
            .collect();
        ids.sort_unstable();
        let mut want = Vec::new();
        for (id, k) in ids {
            let ai = &scene.computers.players[k];
            assert_eq!(ai.flotillas[0].role, 0);
            want.extend((0..ai.flotillas.len()).map(|f| (id, f)));
        }
        let got: Vec<(u32, usize)> = scene
            .navy_walk()
            .iter()
            .filter_map(|&(p, _, g)| match g {
                Group::Flotilla(_, f) => Some((p, f)),
                _ => None,
            })
            .collect();
        assert_eq!(got, want);

        let mut scene = Scene::load(&data, &mut caches, "miss004").unwrap();
        let walk = scene.navy_walk();
        let players: Vec<u32> = walk.iter().map(|w| w.0).collect();
        assert!(players.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(players.first(), Some(&14));
        assert_eq!(players.last(), Some(&16));
        let defs = super::super::brain::mission_fleets("miss004").unwrap();
        for w in walk.windows(2) {
            if let ((p, a, Group::Brain(j)), (q, b, Group::Brain(k))) = (w[0], w[1]) {
                if p == q {
                    assert!(a <= b);
                    assert_eq!(defs[j].group, defs[k].group);
                }
            }
        }
    }
}
