//! A fort's own menu work (FortressClassTy 0x419f80, its menu 0x417ff0):
//! its Repair (command 0x14) and the spell its store holds (slot 3).

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;

use super::Scene;
use crate::assets::GameData;
use crate::magic::{self, Spell};
use crate::objects::Caches;

/// The integer root 0x557670 takes of a 64-bit square (0x5576d8, the cell
/// distance of `Magic.cpp`'s checks): a Newton walk from `n >> (bits /
/// 2)`, each step `(x + n / x) / 2` rounded up when the division leaves a
/// rest and the sum is odd, until a step leaves it as it was.
pub fn root(n: u64) -> u32 {
    if n == 0 {
        return 0;
    }
    let bits = 64 - n.leading_zeros();
    if bits > 62 {
        return 0x7fff_ffff;
    }
    let high = (n >> 32) as u32;
    let mut x = (n >> (bits / 2)) as u32;
    loop {
        // Kept above the high dword so the quotient fits (0x5576a9).
        if x < high {
            x = high + high / 2;
        }
        let old = x;
        let (q, r) = (n / x as u64, n % x as u64);
        let sum = x as u64 + q;
        x = (sum >> 1) as u32;
        if r != 0 && sum & 1 != 0 {
            x += 1;
        }
        if x == old {
            return x;
        }
    }
}

/// The cell distance from `a` to `b` (0x5576d8).
fn cell_distance(a: (i32, i32), b: (i32, i32)) -> u32 {
    let (dx, dy) = ((a.0 - b.0) as i64, (a.1 - b.1) as i64);
    root((dx * dx + dy * dy) as u64)
}

/// Magic.cpp's numbering of the big reef, which has no spell of the
/// temples' (its permission bit 0x38).
const BIG_REEF: usize = 12;

impl Scene {
    /// The counts of fort `id`'s Magic object (`+0x1b4`, `+0x10 + 4 s` for
    /// spell index s 0..13), as 0x419e00 (vtable +0x7c) rebuilds them from
    /// its store: each treasure of kind 9 adds one to its spell, but the
    /// Flame (index 8), which sets `+0x4a8` instead.
    pub fn fort_spells(&self, id: u32) -> [u32; 14] {
        let mut counts = [0; 14];
        for t in self.held_by(id) {
            if t.kind == 9 && t.param != 8 {
                if let Some(c) = counts.get_mut(t.param as usize) {
                    *c += 1;
                }
            }
        }
        counts
    }

    /// The spell fort `id`'s menu offers in slot 3 (0x417ff0): the first
    /// with a count, its command from 0x42fd70. (The big reef and index
    /// 13 have no command there: the original's button then shows the low
    /// byte of a pointer; the port shows none.)
    pub fn fort_spell(&self, id: u32) -> Option<Spell> {
        let s = self.fort_spells(id).iter().position(|&n| n != 0)?;
        Spell::of_index(s as u32)
    }

    /// The fort's Repair (command 0x14, 0x4f34c0: order 0x1e {1}; 0x419f80
    /// case 0x1e sends itself message 0x1389): standing and idle
    /// (0x423d60), it mends itself (job 0xff), its units noted (0x4184f0 ->
    /// 0x418300). There is no test that it is worn there; its button is
    /// offered only then (0x423be0).
    pub fn fort_repair(&mut self, site: u32) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such building")?;
        if !matches!(self.objects[i].class, ObjectClass::Building(0x34..=0x36)) {
            return Err("Not a fort".into());
        }
        if self.construction_at(site).is_some() || self.mending.contains(&site) {
            return Ok(());
        }
        self.mending.push(site);
        self.begin_unit_mend(i);
        Ok(())
    }

    /// How far fort `id`'s own mending is, in whole percent (`+0x120`, the
    /// share of what it lacked when it began that is back, 0x425100), while
    /// it mends.
    pub fn fort_mend_progress(&self, id: u32) -> Option<u32> {
        if !self.mending.contains(&id) {
            return None;
        }
        let i = self.object_index(id)?;
        let c = self.objects[i].combat.as_ref()?;
        let start = self
            .unit_mends
            .iter()
            .find(|m| m.site == id)
            .map_or(0, |m| m.start)
            .min(99);
        let now = super::repair::structure_percent(c);
        Some(now.saturating_sub(start) * 100 / (100 - start))
    }

    /// Whether fort `id`'s Magic object may cast spell index `s`
    /// (0x42ed40 with a price to check, at the full price): a count of it,
    /// else its permission bit (0x42fd70's, `Permits::spell`; the big
    /// reef's 0x38) and its price at hand.
    fn fort_may_cast(&self, id: u32, s: usize) -> bool {
        if self.fort_spells(id)[s] != 0 {
            return true;
        }
        let Some(owner) = self.object_index(id).and_then(|i| self.objects[i].owner) else {
            return false;
        };
        let (allowed, (amber, fish)) = match Spell::of_index(s as u32) {
            Some(spell) => (self.permits.spell(spell), spell.cost()),
            None if s == BIG_REEF => (self.permits.allows(0x38), (4000, 1000)),
            None => return false,
        };
        let (have_amber, have_fish) = self.funds(owner);
        allowed && have_amber >= amber && have_fish >= fish
    }

    /// The pointer's check for a spell cast by fort `id` at cell `cell`
    /// over `target` (0x42ef90; Magic.cpp distances from the fort's cell,
    /// 0x5576d8): lightning (6) within 12 cells; the phantom and the
    /// shields (8, 0xe, 0xf) on an object of the fort's own player within
    /// 15; the view (0x11) anywhere. (Not ported: the sea's spells, whose
    /// checks read the cell's tile layers and occupants, and the
    /// fireball's, which reads one of the cell's slots, in ways not
    /// decoded here; whirlpool also asks for the big reef and the reef.)
    fn fort_cast_check(
        &self,
        id: u32,
        spell: Spell,
        cell: (i32, i32),
        target: Option<u32>,
    ) -> Result<(), String> {
        let i = self.object_index(id).ok_or("No such fort")?;
        let here = self.objects[i].cell.ok_or("No such fort")?;
        let s = spell as usize;
        if !self.fort_may_cast(id, s) {
            return Err(String::new());
        }
        let d = cell_distance(cell, here);
        match spell {
            Spell::ViewMap => Ok(()),
            Spell::Lightning if d <= 12 => Ok(()),
            Spell::Lightning => Err("Too far from the fort (reach: 12 cells)".into()),
            Spell::Phantom | Spell::ProtectWeapons | Spell::ProtectMagic => {
                let own = target.and_then(|t| self.object_index(t)).is_some_and(|t| {
                    self.objects[t].owner.is_some()
                        && self.objects[t].owner == self.objects[i].owner
                });
                if !own {
                    return Err("Choose one of your ships or buildings".into());
                }
                if d > 15 {
                    return Err("Too far from the fort (reach: 15 cells)".into());
                }
                Ok(())
            }
            _ => Err("Not yet cast from a fort in this port".into()),
        }
    }

    /// Order 0x22 to fort `site` (0x419f80): the spell it stores, cast at
    /// cell `(cx, cy)` over `target`. Only while it is not busy and some
    /// count of its Magic object is not 0; Magic.cpp 0x42f580 casts it
    /// (0x42ed40 again, then 0x42f470: a count taken if there is one,
    /// else the price), and the first treasure of kind 9 for that spell
    /// leaves the store (0x424cf0), cast or not. (Not ported: the draw of
    /// the fort's own generator `+0x18` that goes with it, which the port
    /// does not keep.)
    pub fn fort_cast(
        &mut self,
        site: u32,
        spell: Spell,
        (cx, cy): (i32, i32),
        target: Option<u32>,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        let i = self.object_index(site).ok_or("No such fort")?;
        if !matches!(self.objects[i].class, ObjectClass::Building(0x34..=0x36)) {
            return Err("Not a fort".into());
        }
        self.fort_cast_check(site, spell, (cx, cy), target)?;
        if self.mending.contains(&site) || self.fort_spells(site).iter().all(|&n| n == 0) {
            return Ok(());
        }
        let s = spell as usize;
        let owner = self.objects[i].owner;
        if self.fort_may_cast(site, s) {
            if self.fort_spells(site)[s] == 0 {
                if let Some(owner) = owner {
                    let (amber, fish) = spell.cost();
                    self.earn(owner, -(amber as i64), -(fish as i64));
                }
            }
            let t = TILE_SIZE as f32;
            let (x, y) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
            match spell {
                Spell::ViewMap => {
                    self.add_sight_point(cx, cy);
                    self.sounds.push(("flash", x, y));
                }
                Spell::Lightning => {
                    self.add_sight_point(cx, cy);
                    let water = self.nav.is_water(cx, cy);
                    self.spawn_effect(data, caches, magic::fx_lightning(water), x, y);
                    self.sounds.push(("molniya2", x, y));
                    // It strikes at frame 11 of its picture over water, 40
                    // over land, a frame a tick (0x42cfa0).
                    let at = if water { 11 } else { 40 };
                    self.lightning.push((x, y, crate::clock::ticks(at), owner));
                }
                _ => {
                    if let Some(t) = target.and_then(|t| self.object_index(t)) {
                        self.protect(spell, t, data, caches);
                    }
                }
            }
        }
        if let Some(k) = self
            .held
            .iter()
            .position(|h| h.0 == site && h.1.kind == 9 && h.1.param == s as u32)
        {
            self.held.remove(k);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::root;

    /// 0x557670's root rounds to the nearest (checked against an
    /// emulation of its instructions for every square below 300000): 12
    /// up to 156 = 12^2 + 12, 13 from 157.
    #[test]
    fn newton_root() {
        assert_eq!(root(0), 0);
        assert_eq!(root(1), 1);
        assert_eq!(root(3), 2);
        assert_eq!(root(144), 12);
        assert_eq!(root(156), 12);
        assert_eq!(root(157), 13);
        assert_eq!(root(180), 13);
        assert_eq!(root(240), 15);
        assert_eq!(root(241), 16);
        assert_eq!(root(1 << 63), 0x7fff_ffff);
    }
}

#[cfg(test)]
mod data_tests {
    use crate::objects::{Caches, Treasure};
    use crate::scene::tests::own_fort;

    fn spell(param: u32) -> Treasure {
        Treasure {
            kind: 9,
            param,
            amount: 0,
        }
    }

    /// A fort's menu (0x417ff0): slot 3 the spell its store holds (not the
    /// Flame, 0x419e00), slot 4 Repair while worn or mending, which mends
    /// it (0x1389), shows how far, stays on limed while it mends, and order
    /// 10 ends (needs `OPENCONQUEST_DATA`).
    #[test]
    fn fort_menu_spell_and_repair() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = crate::assets::GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let (mut scene, fort, _) = own_fort(&data, &mut caches);
        scene.permits = crate::permits::Permits::ALL;
        let f = scene.object_index(fort).unwrap();
        let menu = |scene: &crate::scene::Scene| {
            crate::commands::main_menu(scene, &scene.objects[scene.object_index(fort).unwrap()])
        };
        assert!(menu(&scene)[3].is_none() && menu(&scene)[4].is_none());
        // The Flame is not a count (it sets `+0x4a8`).
        scene.held.push((fort, spell(8)));
        assert!(menu(&scene)[3].is_none());
        scene.held.clear();
        scene.held.push((fort, spell(6)));
        let s = menu(&scene)[3].unwrap();
        assert_eq!((s.command.0, s.enabled), (0x69, true));
        // Worn: Repair, no progress.
        let c = scene.objects[f].combat.as_mut().unwrap();
        c.hull = c.max_hull / 2.0;
        let r = menu(&scene)[4].unwrap();
        assert_eq!((r.command.0, r.enabled, r.progress), (0x14, true, None));
        scene.fort_repair(fort).unwrap();
        assert!(scene.mending.contains(&fort));
        let m = menu(&scene);
        assert_eq!(m[4].unwrap().progress, Some(0.0));
        assert!(m[0..4].iter().flatten().all(|s| !s.enabled));
        // Three quarters whole of the half it lacked: 50 percent.
        let c = scene.objects[f].combat.as_mut().unwrap();
        c.hull = c.max_hull * 0.75;
        assert_eq!(scene.fort_mend_progress(fort), Some(50));
        // Limed while mending: 0x423a00 is skipped, Repair stays on.
        scene.objects[f].combat.as_mut().unwrap().lime = 100.0;
        assert!(menu(&scene)[4].unwrap().enabled);
        // Its right click: order 10, the mending ended.
        scene.halt(fort, &data, &mut caches).unwrap();
        assert!(!scene.mending.contains(&fort));
        // Limed and idle: every button off.
        assert!(menu(&scene).iter().flatten().all(|s| !s.enabled));
    }

    /// A fort's stored lightning (0x419f80 case 0x22, 0x42ef90, 0x42f580):
    /// cast free within 12 cells of the fort (0x5576d8 rounds), the
    /// treasure spent; farther refused, the treasure kept (needs
    /// `OPENCONQUEST_DATA`).
    #[test]
    fn fort_casts_its_stored_spell() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = crate::assets::GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let (mut scene, fort, _) = own_fort(&data, &mut caches);
        // Without its permission: only the count lets it cast.
        scene.permits = crate::permits::Permits(0);
        let f = scene.object_index(fort).unwrap();
        let (fx, fy) = scene.objects[f].cell.unwrap();
        scene.held.push((fort, spell(6)));
        let amber = scene.amber;
        let lightning = super::Spell::Lightning;
        // 12^2 + 4^2 = 160: 13 cells.
        let far = scene.fort_cast(fort, lightning, (fx + 12, fy + 4), None, &data, &mut caches);
        assert!(far.is_err());
        assert_eq!(scene.fort_spells(fort)[6], 1);
        // 12^2 + 3^2 = 153: 12 cells.
        let before = scene.lightning.len();
        scene
            .fort_cast(fort, lightning, (fx + 12, fy + 3), None, &data, &mut caches)
            .unwrap();
        assert_eq!(scene.lightning.len(), before + 1);
        assert_eq!(scene.fort_spells(fort)[6], 0);
        assert!(scene.held.iter().all(|h| h.0 != fort));
        assert_eq!(scene.amber, amber);
        // Nothing left and no permission: the pointer refuses it.
        let again = scene.fort_cast(fort, lightning, (fx, fy), None, &data, &mut caches);
        assert!(again.is_err());
    }

    /// A fort's masks (0x419f80 cases 0x15 / 0x16 / 0x23, 0x41c4e0): the
    /// player's orders store all ones but the fort's own player (kinds
    /// 0xff for an attack, 0xffff for Guard); a group shoots only what
    /// its masks let through: with the ship's class bit cleared the fort
    /// stays silent at will, and fires once it is back (needs
    /// `OPENCONQUEST_DATA`).
    #[test]
    fn fort_masks_choose_what_it_shoots() {
        use openconquest_formats::ObjectClass;
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = crate::assets::GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = crate::scene::Scene::load(&data, &mut caches, "miss002").unwrap();
        let forts: Vec<u32> = scene
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Building(0x34..=0x36)))
            .map(|o| o.id)
            .collect();
        let fort = scene
            .objects
            .iter()
            .position(|o| matches!(o.class, ObjectClass::Building(0x34..=0x36)) && o.is_armed())
            .unwrap();
        let (fx, fy) = (scene.objects[fort].x, scene.objects[fort].y);
        let fort_id = scene.objects[fort].id;
        let owner = scene.objects[fort].owner.unwrap();
        for &id in &forts {
            scene.halt(id, &data, &mut caches).unwrap();
        }
        let ship = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
        let ship_id = scene.objects[ship].id;
        scene
            .objects
            .retain(|o| o.ship.is_none() || o.id == ship_id);
        let i = scene.object_index(ship_id).unwrap();
        scene.objects[i].owner = Some((owner.0, owner.1 + 1));
        let kind = scene.objects[i].ship.as_ref().unwrap().kind;
        let water = scene
            .nav
            .nearest_water((fx / 64.0) as i32, (fy / 64.0) as i32)
            .unwrap();
        let at = (water.0 as f32 * 64.0 + 32.0, water.1 as f32 * 64.0 + 32.0);
        scene.objects[i].combat.as_mut().unwrap().weapons.clear();
        let quiet = |scene: &mut crate::scene::Scene, caches: &mut Caches| {
            for _ in 0..50 * 5 {
                if let Some(i) = scene.object_index(ship_id) {
                    let o = &mut scene.objects[i];
                    (o.x, o.y) = at;
                    let s = o.ship.as_mut().unwrap();
                    s.task = None;
                    s.path.clear();
                }
                scene.update(0.02, &data, caches);
                if !scene.shots.is_empty() {
                    return false;
                }
            }
            true
        };
        let masks = |scene: &crate::scene::Scene| {
            let f = scene.object_index(fort_id).unwrap();
            scene.objects[f].combat.as_ref().unwrap().fire.masks
        };
        let own = 1 << ((owner.1 + owner.0 * 4) & 0x1f);
        // Guard (0x23 sub 1): players all but its own, kinds 0xffff.
        scene.fort_order(fort_id, 1).unwrap();
        let m = masks(&scene);
        assert_eq!((m.players, m.classes, m.kinds), (!own, u32::MAX, 0xffff));
        // Its ship type's class bit cleared: nothing to shoot.
        let f = scene.object_index(fort_id).unwrap();
        scene.objects[f].combat.as_mut().unwrap().fire.masks.classes = !(1u32 << kind);
        assert!(quiet(&mut scene, &mut caches), "the fort fired");
        // An attack (0x15 {target, 7, 0xff, -1, -1}) stores the player's
        // masks again: the ship is shot.
        scene.order_attack(fort_id, ship_id).unwrap();
        let m = masks(&scene);
        assert_eq!((m.players, m.classes, m.kinds), (!own, u32::MAX, 0xff));
        assert!(!quiet(&mut scene, &mut caches), "the fort did not fire");
    }

    /// A settlement falling tells its buildings (0x43bc30 -> 0x2874 ->
    /// 0x427f50): a busy one is sent order 10, so a fort mending itself
    /// stops and ceases fire; an idle one keeps firing (needs
    /// `OPENCONQUEST_DATA`).
    #[test]
    fn a_town_gone_halts_its_busy_buildings() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = crate::assets::GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let (mut scene, fort, site) = own_fort(&data, &mut caches);
        let fire = |scene: &crate::scene::Scene| {
            let f = scene.object_index(fort).unwrap();
            scene.objects[f].combat.as_ref().unwrap().fire
        };
        let raze = |scene: &mut crate::scene::Scene, caches: &mut Caches, site: u32| {
            let s = scene.object_index(site).unwrap();
            let hull = scene.objects[s].combat.as_ref().unwrap().hull;
            scene.strike(site, hull + 1.0, false, None, true, &data, caches);
        };
        // Idle: nothing changes.
        let f = scene.object_index(fort).unwrap();
        let next = scene.objects.iter().enumerate().find(|(i, o)| {
            *i != f
                && o.id != site
                && o.owner == scene.objects[f].owner
                && matches!(
                    o.class,
                    openconquest_formats::ObjectClass::Building(crate::economy::SETTLEMENT)
                        | openconquest_formats::ObjectClass::Building(crate::economy::TOWN)
                )
        });
        assert!(next.is_some(), "another settlement of the fort's player");
        raze(&mut scene, &mut caches, site);
        assert!(fire(&scene).at_will);
        // Mending, its new settlement falls: order 10 ends the mending and
        // silences the fort.
        let f = scene.object_index(fort).unwrap();
        let home = scene.home_settlement(f).unwrap();
        let c = scene.objects[f].combat.as_mut().unwrap();
        c.hull = c.max_hull / 2.0;
        scene.fort_repair(fort).unwrap();
        raze(&mut scene, &mut caches, home);
        assert!(!scene.mending.contains(&fort));
        assert!(!fire(&scene).fires());
    }
}
