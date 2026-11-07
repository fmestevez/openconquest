//! The human's guard zone: the interface's fleet command (0x49d790 case 9,
//! command 0x74) gives the selection's group the guard block `{0x6e,
//! 0x29}` (0x4d1ce0) over the rectangle dragged, mode 2: the group's
//! ships posted in the rectangle in formation (0x4d1080), and at each pass
//! of the block (0x4d3b30, P = 0x4b) the watch (0x4d33c0). The block is
//! the computers' (`computer/fleet_guard.rs`), driven here through
//! `GuardGroup`. See docs/FORMATS.md ("The guard task", "Who makes it").

use super::computer::fleet_guard::{GuardBlock, GuardGroup};
use super::gather::Notice;
use super::Scene;
use openconquest_formats::map::TILE_SIZE;
use serde::{Deserialize, Serialize};

/// The period P of 0x4d3b30 and 0x4d3080 for a side not of kind 3 or 4.
pub const PERIOD: u32 = 0x4b;

/// A human group running the guard block: the group's own fields the
/// block reads and the block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HumanGuard {
    /// The group's serial number (shared with the other human groups).
    pub serial: u32,
    pub owner: (u32, u32),
    /// Group `+0x31`: its ships, by id.
    pub ships: Vec<u32>,
    /// Group `+0x2d`: the notices not yet read, oldest first.
    pub notices: Vec<Notice>,
    /// Group `+8`: ticks since the last pass.
    pub elapsed: u32,
    /// Group `+0x14..+0x24` (`HumanBehaviour`): `None` as made
    /// (0x49b990), else kept from the group's earlier blocks.
    #[serde(default)]
    pub behaviour: super::gather::HumanBehaviour,
    /// Group `+0x25`: the block.
    pub block: Option<GuardBlock>,
    /// The block's route `+0x61`: the Shift move's waypoints (0x49de70;
    /// the guard's code never reads it: the cells are only marked).
    #[serde(default)]
    pub route: Option<super::gather::Route>,
}

impl GuardGroup for HumanGuard {
    fn ships(&self) -> &[u32] {
        &self.ships
    }
    fn block(&self) -> &Option<GuardBlock> {
        &self.block
    }
    fn block_mut(&mut self) -> &mut Option<GuardBlock> {
        &mut self.block
    }
    fn owner(&self) -> (u32, u32) {
        self.owner
    }
    fn behaviour(&self) -> (u32, u32, u32) {
        self.behaviour.unwrap_or((0, 0, 0))
    }
    fn period(&self) -> u32 {
        PERIOD
    }
    fn elapsed(&mut self) -> &mut u32 {
        &mut self.elapsed
    }
    fn take_notices(&mut self) -> Vec<(u32, u32)> {
        std::mem::take(&mut self.notices)
            .into_iter()
            .map(|n| (n.ship, n.code))
            .collect()
    }
}

impl Scene {
    /// The human's guard zone (0x49d790 case 9) for `ships` over the
    /// rectangle the drag gave (0x51cdd0: its start and end cells,
    /// ordered, `{x0, y0, x1 - x0 + 1, y1 - y0 + 1}`, as the attack
    /// zone's). The selection is the group (0x49e230 keeps it while the
    /// selection is unchanged; else 0x49ca60 makes a new one, its ships
    /// leaving their old groups). Fourteen dwords zeroed, `[0..3]` the
    /// rect, `[10]` mode 2, straight to 0x4d1ce0 (no 0x4c4350 first): a
    /// block of another kind ended (0x4a1c60, its notices freed; the
    /// group's serial, `+8` and `+0x19..+0x24` kept), a guard block
    /// rewritten, none made; no second rect, no mask, flags 0; the
    /// formation into the rect (0x4d1080).
    pub fn order_guard_zone(
        &mut self,
        ships: &[u32],
        a: (f32, f32),
        b: (f32, f32),
    ) -> Result<(), String> {
        let t = TILE_SIZE as f32;
        let cell = |v: f32| (v / t) as i32;
        let (ax, ay, bx, by) = (cell(a.0), cell(a.1), cell(b.0), cell(b.1));
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let (y0, y1) = (ay.min(by), ay.max(by));
        let rect = (x0, y0, x1 - x0 + 1, y1 - y0 + 1);
        let ships: Vec<u32> = ships
            .iter()
            .copied()
            .filter(|&id| {
                self.object_index(id)
                    .is_some_and(|i| self.objects[i].ship.is_some())
            })
            .collect();
        let owner = ships
            .iter()
            .find_map(|&id| self.object_index(id).and_then(|i| self.objects[i].owner));
        let Some(owner) = owner else {
            return Err("No such ship".into());
        };
        let z = match self.selection_guard(&ships) {
            Some(z) => z,
            None => {
                // The group keeps its behaviour `+0x14..+0x24` from its
                // earlier blocks (0x4a1c60 leaves it); a new one has none.
                let (serial, elapsed, behaviour) = match self.end_selection_block(&ships) {
                    Some((serial, elapsed, _, behaviour)) => (serial, elapsed, behaviour),
                    None => {
                        self.leave_zones(&ships);
                        self.zone_serial += 1;
                        (self.zone_serial, 0, None)
                    }
                };
                self.human_guards.push(HumanGuard {
                    serial,
                    owner,
                    ships,
                    notices: Vec::new(),
                    elapsed,
                    behaviour,
                    block: None,
                    route: None,
                });
                self.human_guards.len() - 1
            }
        };
        let mut g = self.human_guards.remove(z);
        self.guard_make(&mut g, rect, (0, 0, 0, 0), 0, 2);
        self.human_guards.insert(z, g);
        Ok(())
    }

    /// The human guard group whose ships are exactly `ships`.
    pub fn selection_guard(&self, ships: &[u32]) -> Option<usize> {
        let mut sel: Vec<u32> = ships.to_vec();
        sel.sort_unstable();
        sel.dedup();
        self.human_guards.iter().position(|g| {
            let mut s = g.ships.clone();
            s.sort_unstable();
            !s.is_empty() && s == sel
        })
    }

    /// The human guard group holding ship `id`.
    pub fn ship_guard(&self, id: u32) -> Option<usize> {
        self.human_guards.iter().position(|g| g.ships.contains(&id))
    }

    /// Ships leaving their guard groups (0x49ca60, or sunk: 0x4a8410 ->
    /// 0x49bef0) post notice 0x137 to the group (0x4a1770) before they
    /// are taken out; a group left with none goes.
    pub(super) fn leave_guards(&mut self, ids: &[u32]) {
        let tick = self.ticks;
        for g in &mut self.human_guards {
            for &s in &g.ships {
                if ids.contains(&s) {
                    g.notices.push(Notice {
                        code: 0x137,
                        tick,
                        ship: s,
                        class: 0,
                    });
                }
            }
            g.ships.retain(|s| !ids.contains(s));
        }
        self.human_guards.retain(|g| !g.ships.is_empty());
    }

    /// The navy's tick for the guard groups (0x4d3b30 each, from
    /// 0x4c4f40).
    pub(super) fn run_human_guards(&mut self) {
        let gone: Vec<u32> = self
            .human_guards
            .iter()
            .flat_map(|g| g.ships.iter().copied())
            .filter(|&id| {
                self.object_index(id)
                    .is_none_or(|i| self.objects[i].ship.is_none())
            })
            .collect();
        if !gone.is_empty() {
            self.leave_guards(&gone);
        }
        // A sunk ship leaves its bare group too (0x4a8410 -> 0x49bef0).
        let objects = &self.objects;
        for g in &mut self.human_groups {
            g.ships
                .retain(|&id| objects.iter().any(|o| o.id == id && o.ship.is_some()));
        }
        self.human_groups.retain(|g| !g.ships.is_empty());
        for z in 0..self.human_guards.len() {
            let mut g = self.human_guards.remove(z);
            self.guard_tick(&mut g);
            self.human_guards.insert(z, g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::GameData;
    use crate::objects::Caches;
    use crate::scene::computer::fleet_guard::Post;
    use crate::scene::Order;

    fn data() -> Option<GameData> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        Some(GameData::open(std::path::Path::new(&root)).unwrap())
    }

    /// Two warships given a guard zone (0x49d790 case 9) on miss000: the
    /// group's block is mode 2 over the rect, flags 0, no second rect, no
    /// mask; each ship posted at its formation cell in the rect (0x4d1080)
    /// and sent there (0x4cf070). At the pass past P = 0x4b (0x4d3b30) a
    /// post hit by an enemy (its `+0x691`) has the blow forgotten
    /// (0x4cf760) and, its strength at least three times the enemy's
    /// (0x4d2980 giving the enemy's own strength, at most twice ours),
    /// leaves its post for the engaged list on the attacker (0x4d2b00 (2),
    /// 0x4d18d0: command 0x16). The group's `+0x19` is 0 (0x49b990), so
    /// the zone's own targets (0x4cd300) are none. A ship given another
    /// order leaves with notice 0x137 and its post goes.
    #[test]
    fn human_guard_zone_on_miss000() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let foe_owner = (human.0 ^ 1, human.1);
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(3)
            .collect();
        let (i0, i1, f) = (ships[0], ships[1], ships[2]);
        let t = TILE_SIZE as f32;
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let around = |c0: (i32, i32)| -> Vec<(i32, i32)> {
            (-3..=3)
                .flat_map(|dy| (-3..=3).map(move |dx| (c0.0 + dx, c0.1 + dy)))
                .filter(|&c| c != c0 && water(&scene, c))
                .collect()
        };
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let c0 = (3..h - 3)
            .flat_map(|y| (3..w - 3).map(move |x| (x, y)))
            .find(|&c| water(&scene, c) && around(c).len() == 48)
            .unwrap();
        let near = around(c0);
        let (c1, cf) = (near[0], near[near.len() - 1]);
        for (i, c, owner) in [(i0, c0, human), (i1, c1, human), (f, cf, foe_owner)] {
            let o = &mut scene.objects[i];
            o.owner = Some(owner);
            o.x = c.0 as f32 * t + t / 2.0;
            o.y = c.1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.argo = false;
            s.move_state = 0;
        }
        let e = &mut scene.objects[f];
        e.combat.as_mut().unwrap().weapons.clear();
        e.ship.as_mut().unwrap().crew.squads = [0; 3];
        let (id0, id1, idf) = (
            scene.objects[i0].id,
            scene.objects[i1].id,
            scene.objects[f].id,
        );
        scene.outbox.clear();
        let corner = |d: i32| ((c0.0 + d) as f32 * t + 1.0, (c0.1 + d) as f32 * t + 1.0);
        scene
            .order_guard_zone(&[id0, id1], corner(2), corner(-2))
            .unwrap();
        assert_eq!(scene.human_guards.len(), 1);
        let g = scene.human_guards[0].clone();
        assert_eq!(g.ships, [id0, id1]);
        assert_eq!(g.behaviour, None);
        let b = g.block.unwrap();
        let rect = (c0.0 - 2, c0.1 - 2, 5, 5);
        assert_eq!(
            (b.rect, b.rect2, b.mode, b.flags),
            (rect, (0, 0, 0, 0), 2, 0)
        );
        assert!(b.mask.is_none() && b.engaged.is_none() && b.patrol.is_none());
        let posts = b.posts.unwrap();
        assert_eq!(posts.iter().map(|p| p.ship).collect::<Vec<_>>(), [id0, id1]);
        assert_ne!((posts[0].x, posts[0].y), (posts[1].x, posts[1].y));
        let inside =
            |p: &Post| p.x >= rect.0 && p.x < rect.0 + 5 && p.y >= rect.1 && p.y < rect.1 + 5;
        assert!(posts.iter().all(inside));
        let go = |p: &Post| Order::Move {
            ship: p.ship,
            x: p.x as f32 * t + t / 2.0,
            y: p.y as f32 * t + t / 2.0,
        };
        assert_eq!(scene.outbox, [go(&posts[0]), go(&posts[1])]);
        // Within the period nothing; past it the watch.
        scene.outbox.clear();
        scene.objects[i0].ship.as_mut().unwrap().blow = Some(idf);
        scene.run_human_guards();
        assert!(scene.outbox.is_empty());
        assert_eq!(scene.human_guards[0].elapsed, 1);
        scene.human_guards[0].elapsed = PERIOD + 1;
        let (own, foe) = (scene.strength(i0), scene.strength(f));
        assert!(foe <= own * 2 && own >= 3 * foe);
        scene.run_human_guards();
        assert!(scene.outbox.contains(&Order::ClearBlow { ship: id0 }));
        assert!(scene.outbox.contains(&Order::Attack {
            ship: id0,
            target: idf
        }));
        let g = &scene.human_guards[0];
        assert_eq!(g.elapsed, 1);
        let b = g.block.as_ref().unwrap();
        let engaged = b.engaged.as_ref().unwrap();
        assert_eq!(engaged.len(), 1);
        assert_eq!((engaged[0].ship, engaged[0].target), (id0, idf));
        assert_eq!((engaged[0].x, engaged[0].y), c0);
        let posts = b.posts.as_ref().unwrap();
        assert_eq!(posts.iter().map(|p| p.ship).collect::<Vec<_>>(), [id1]);
        // Another order: notice 0x137, the post gone at the next pass.
        scene.leave_zones(&[id1]);
        assert_eq!(scene.human_guards[0].ships, [id0]);
        scene.run_human_guards();
        let b = scene.human_guards[0].block.as_ref().unwrap();
        assert!(b.posts.as_ref().unwrap().is_empty());
        assert!(scene.human_guards[0].notices.is_empty());
    }

    /// A group's behaviour `+0x14..+0x24` goes with the group through its
    /// blocks: the attack zone gives it (0x49da43: 0x4c4350 `{0,
    /// 0x4a2f10(player), -1, -1}`), the patrol's waypoint move and the
    /// guard zone keep it (0x4a1c60 leaves the group's fields), and the
    /// guard watch's 0x4cd300 then lists the enemy in its zone; a fresh
    /// group's is zero (0x49b990).
    #[test]
    fn human_group_keeps_its_behaviour() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let foe_owner = (human.0 ^ 1, human.1);
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(3)
            .collect();
        let (i0, i1, f) = (ships[0], ships[1], ships[2]);
        let t = TILE_SIZE as f32;
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let c0 = (3..h - 3)
            .flat_map(|y| (3..w - 3).map(move |x| (x, y)))
            .find(|&c| {
                (-3..=3)
                    .flat_map(|dy| (-3..=3).map(move |dx| (c.0 + dx, c.1 + dy)))
                    .all(|c| water(&scene, c))
            })
            .unwrap();
        let (c1, cf) = ((c0.0 + 1, c0.1), (c0.0 - 1, c0.1 - 1));
        for (i, c, owner) in [(i0, c0, human), (i1, c1, human), (f, cf, foe_owner)] {
            let o = &mut scene.objects[i];
            o.owner = Some(owner);
            o.x = c.0 as f32 * t + t / 2.0;
            o.y = c.1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.argo = false;
            s.move_state = 0;
        }
        let (id0, id1, idf) = (
            scene.objects[i0].id,
            scene.objects[i1].id,
            scene.objects[f].id,
        );
        let corner = |d: i32| ((c0.0 + d) as f32 * t + 1.0, (c0.1 + d) as f32 * t + 1.0);
        let players = !(1u32 << ((human.1 + human.0 * 4) & 0x1f));
        let given = Some((players, u32::MAX, u32::MAX));
        // A fresh group: none.
        scene
            .order_guard_zone(&[id0, id1], corner(2), corner(-2))
            .unwrap();
        assert_eq!(scene.human_guards[0].behaviour, None);
        scene.leave_zones(&[id0, id1]);
        assert!(scene.human_guards.is_empty());
        // Attack zone, patrol, guard zone: the one group, its behaviour
        // kept.
        scene
            .order_attack_zone(&[id0, id1], corner(2), corner(-2))
            .unwrap();
        let serial = scene.human_attacks[0].serial;
        scene
            .order_patrol_route(&[id0, id1], c0.0 + 2, c0.1)
            .unwrap();
        assert!(scene.human_attacks.is_empty());
        assert_eq!(scene.waypoint_moves[0].serial, serial);
        assert_eq!(scene.waypoint_moves[0].behaviour, given);
        scene
            .order_guard_zone(&[id0, id1], corner(2), corner(-2))
            .unwrap();
        assert!(scene.waypoint_moves.is_empty());
        let g = &scene.human_guards[0];
        assert_eq!((g.serial, g.behaviour), (serial, given));
        let rect = g.block.as_ref().unwrap().rect;
        let listed = |p: u32| -> Vec<u32> {
            scene
                .area_targets(rect, Some(human), p)
                .iter()
                .map(|t| t.id)
                .collect()
        };
        assert!(listed(GuardGroup::behaviour(g).0).contains(&idf));
        assert!(listed(0).is_empty());
    }

    /// The selection's group through plain orders on miss000: a move
    /// without Shift (0x49de70 -> 0x4ccc30) and a stop (command 10,
    /// 0x49e230's default) end its block (0x4a1c60) and keep the group,
    /// its serial and behaviour (0x49ca60 makes one only for a changed
    /// selection); the guard zone then given reads the attack zone's
    /// players; a Shift move on the guard group appends the cell to the
    /// block's route `+0x61` (0x49de70: 0x4ce5e0, 0x4ce6c0) and keeps it;
    /// a changed selection makes a new group, its ship leaving the old.
    #[test]
    fn plain_orders_keep_the_selection_group() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(2)
            .collect();
        let (i0, i1) = (ships[0], ships[1]);
        let t = TILE_SIZE as f32;
        let water = |scene: &Scene, c: (i32, i32)| {
            scene.nav.is_sea(c.0, c.1) && scene.nav.passable(c.0, c.1, true)
        };
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let c0 = (3..h - 3)
            .flat_map(|y| (3..w - 3).map(move |x| (x, y)))
            .find(|&c| {
                (-3..=3)
                    .flat_map(|dy| (-3..=3).map(move |dx| (c.0 + dx, c.1 + dy)))
                    .all(|c| water(&scene, c))
            })
            .unwrap();
        for (i, c) in [(i0, c0), (i1, (c0.0 + 1, c0.1))] {
            let o = &mut scene.objects[i];
            o.owner = Some(human);
            o.x = c.0 as f32 * t + t / 2.0;
            o.y = c.1 as f32 * t + t / 2.0;
            let s = o.ship.as_mut().unwrap();
            s.kind = 2;
            s.argo = false;
            s.move_state = 0;
        }
        let (id0, id1) = (scene.objects[i0].id, scene.objects[i1].id);
        let corner = |d: i32| ((c0.0 + d) as f32 * t + 1.0, (c0.1 + d) as f32 * t + 1.0);
        let players = !(1u32 << ((human.1 + human.0 * 4) & 0x1f));
        let given = Some((players, u32::MAX, u32::MAX));
        scene
            .order_attack_zone(&[id0, id1], corner(2), corner(-2))
            .unwrap();
        let serial = scene.human_attacks[0].serial;
        let sel = vec![id0, id1];
        let mut apply = |scene: &mut Scene, o: Order| {
            scene.apply(&o, &data, &mut caches).unwrap();
        };
        // The move: the attack block ended, the group kept.
        apply(&mut scene, Order::KeepGroup { ships: sel.clone() });
        for (id, k) in [(id0, 0.0), (id1, 1.0)] {
            let (x, y) = corner(1);
            apply(
                &mut scene,
                Order::Move {
                    ship: id,
                    x: x + k * t,
                    y,
                },
            );
        }
        assert!(scene.human_attacks.is_empty());
        assert_eq!(scene.human_groups.len(), 1);
        let g = &scene.human_groups[0];
        assert_eq!((g.serial, g.behaviour), (serial, given));
        // The stop keeps it again.
        apply(&mut scene, Order::KeepGroup { ships: sel.clone() });
        for id in [id0, id1] {
            apply(&mut scene, Order::Stop { id });
        }
        assert_eq!(scene.human_groups.len(), 1);
        assert_eq!(scene.human_groups[0].serial, serial);
        // The guard zone on it reads the attack zone's players.
        scene.order_guard_zone(&sel, corner(2), corner(-2)).unwrap();
        assert!(scene.human_groups.is_empty());
        let g = &scene.human_guards[0];
        assert_eq!((g.serial, g.behaviour), (serial, given));
        // Shift moves: the cells appended to the guard block's route.
        scene.order_shift_move(&sel, c0.0 + 2, c0.1).unwrap();
        scene.order_shift_move(&sel, c0.0 + 2, c0.1).unwrap();
        scene.order_shift_move(&sel, c0.0 - 2, c0.1).unwrap();
        assert!(scene.waypoint_moves.is_empty());
        let g = &scene.human_guards[0];
        assert_eq!(g.serial, serial);
        assert!(g.block.is_some());
        let route = g.route.as_ref().unwrap();
        assert_eq!(route.waypoints, vec![(c0.0 + 2, c0.1), (c0.0 - 2, c0.1)]);
        // A changed selection: a new group of its own, id0 leaving the
        // guard group with notice 0x137.
        apply(&mut scene, Order::KeepGroup { ships: vec![id0] });
        apply(&mut scene, Order::Stop { id: id0 });
        assert_eq!(scene.human_guards[0].ships, vec![id1]);
        assert!(scene.human_guards[0]
            .notices
            .iter()
            .any(|n| n.code == 0x137 && n.ship == id0));
        let g = &scene.human_groups[0];
        assert_eq!((g.ships.clone(), g.behaviour), (vec![id0], None));
        assert_eq!(g.serial, scene.zone_serial);
        assert!(g.serial > serial);
    }
}
