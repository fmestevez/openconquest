//! Fighting: gunfire, ramming, boarding, and the effects drawn for them.

use super::*;

impl Scene {
    /// Whether two objects belong to different players. All players are
    /// treated as enemies of each other until diplomacy is implemented.
    /// A building asleep is no one's enemy (0x40dfc0): nothing aims at it
    /// and it aims at nothing.
    pub fn are_enemies(a: &MapObject, b: &MapObject) -> bool {
        a.asleep.is_none()
            && b.asleep.is_none()
            && matches!((a.owner, b.owner), (Some(x), Some(y)) if x != y)
    }

    /// Orders armed ship `ship` to attack the enemy ship or building `target`.
    pub fn order_attack(&mut self, ship: u32, target: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let t = self.object_index(target).ok_or("No such target")?;
        // A fort's attack: its Fire button's pointer (0x4f34c0 command 3:
        // state 0x15, kind 7) or a plain click with the attack pointer
        // (0x5180e0 pointer 9) sends order 0x15 / 0x16 {target, 7}, every
        // group at it, once the fort's answer (0x419f80 message 0xc452) let
        // the pointer through: standing, not limed, an enemy some group
        // reaches.
        if self.objects[i].holds_units() {
            if self.construction_at(ship).is_some()
                || self.objects[i]
                    .combat
                    .as_ref()
                    .is_some_and(|c| c.lime > 0.0)
            {
                return Err(String::new());
            }
            if self.objects[t].combat.is_none()
                || !Self::are_enemies(&self.objects[i], &self.objects[t])
            {
                return Err("Choose an enemy ship or building".into());
            }
            if !self.fort_reaches(i, t) {
                return Err("Out of the fort's reach".into());
            }
            self.fort_attack(ship, target, 7, combat::FortMasks::PLAYER_ATTACK);
            return Ok(());
        }
        if self.objects[i].ship.is_none() || !self.objects[i].is_armed() {
            return Err("This ship has no weapons".into());
        }
        // Limed, its fire command is not to be had (0x4e6700, 0x15).
        if self.objects[i]
            .combat
            .as_ref()
            .is_some_and(|c| c.lime > 0.0)
        {
            return Err(String::new());
        }
        if self.objects[t].combat.is_none()
            || !Self::are_enemies(&self.objects[i], &self.objects[t])
        {
            return Err("Choose an enemy ship or building".into());
        }
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path.clear();
        ship.task = Some(Task::Attack { target });
        Ok(())
    }

    /// Whether some unit of fort `i` reaches object `t` (the answer to the
    /// pointer, 0x419f80 message 0xc452: 0x446800 over the groups with
    /// units).
    pub fn fort_reaches(&self, i: usize, t: usize) -> bool {
        let (me, it) = (&self.objects[i], &self.objects[t]);
        let d = ((me.x - it.x).powi(2) + (me.y - it.y).powi(2)).sqrt();
        let levels = self.levels_of(me.owner);
        me.shooters().iter().any(|&w| {
            let (near, far) = combat::reach(w, levels.reach_for(w));
            d >= near && d <= far
        })
    }

    /// Order 0x15 / 0x16 {target, groups, kinds, players, classes} to fort
    /// or barbarian structure `site` (0x419f80 / 0x40ef70): the groups
    /// named (`order_groups`) fire at `target` (0x446960: only if it is
    /// found, 0x51f1f0; mode bit 2 added, the at-will bit kept); a fort
    /// stores the masks (`+0x494` the byte `+5`, `+0x48c` the dword `+6`
    /// less its own player's bit, `+0x490` the dword `+10`), whatever is
    /// found. (Not ported: a cyclops' jam, class 0x1f7, which the fort
    /// turns into the jam's first member, 0x415df0, the earliest of the
    /// creatures on that cell still there: the port has no jams, a click
    /// gives the creature itself.)
    pub fn fort_attack(&mut self, site: u32, target: u32, groups: u8, masks: combat::FortMasks) {
        let Some(i) = self
            .object_index(site)
            .filter(|&i| self.objects[i].holds_units())
        else {
            return;
        };
        let fort = matches!(self.objects[i].class, ObjectClass::Building(0x34..=0x36));
        let owner = self.objects[i].owner;
        let found = self.object_index(target).is_some();
        let Some(c) = self.objects[i].combat.as_mut() else {
            return;
        };
        if fort {
            if let Some(owner) = owner {
                c.fire.masks = masks.stored(owner);
            }
        }
        if found {
            for k in combat::order_groups(groups) {
                c.fire.targets[k] = target;
            }
        }
    }

    /// Order 0x23 sub `sub` to fort or barbarian structure `site`: the
    /// fort's Guard button (0x4f34c0 command 0x70: sub 1 when it is not
    /// ticked, 2 when it is) and Fire on aggressor (command 0x73: sub 3,
    /// or 4 ticked), with the player's masks (`FortMasks::
    /// PLAYER_BEHAVIOUR`). A fort (0x419f80 case 0x23): 1 the masks
    /// stored, every group at will (0x4469a0), `+0x498 = 0`; 2 and 4 every
    /// group ceases fire (0x4469b0), `+0x498 = 0`; 3 the masks stored,
    /// `+0x498 = 1`, every group ceases fire (it answers blows,
    /// `fort_aggressed`). A barbarian structure (0x40ef70) takes sub 1
    /// only, and keeps no masks.
    pub fn fort_order(&mut self, site: u32, sub: u8) -> Result<(), String> {
        let i = self
            .object_index(site)
            .filter(|&i| self.objects[i].holds_units())
            .ok_or("Not a fort")?;
        let fort = matches!(self.objects[i].class, ObjectClass::Building(0x34..=0x36));
        let owner = self.objects[i].owner;
        let c = self.objects[i].combat.as_mut().unwrap();
        let masks = owner.map(|o| combat::FortMasks::PLAYER_BEHAVIOUR.stored(o));
        match sub {
            1 => {
                if let (true, Some(m)) = (fort, masks) {
                    c.fire.masks = m;
                }
                c.fire.fire_at_will();
                c.fire.aggressor = false;
            }
            2 | 4 if fort => {
                c.fire.cease();
                c.fire.aggressor = false;
            }
            3 if fort => {
                if let Some(m) = masks {
                    c.fire.masks = m;
                }
                c.fire.cease();
                c.fire.aggressor = true;
            }
            _ => {}
        }
        Ok(())
    }

    /// Whether a group of fort `i` carrying `item` may shoot object `t`
    /// by the fort's masks (0x41c4e0 before its score): `t`'s side
    /// (`+0x57`: 2 for barbarian structures, 0x410150, lairs, 0x414e20,
    /// and the port's other monsters, whose constructors were not all
    /// read: with every mask the player's orders store, sides 1 and 2
    /// both pass; 1 another player's object; none else), its
    /// player's bit, its class bit and the group's kind. Barbarian
    /// structures' groups have another shot test (not decoded) and shoot
    /// as before.
    pub(super) fn fort_may_shoot(&self, i: usize, item: u8, t: usize) -> bool {
        let (me, it) = (&self.objects[i], &self.objects[t]);
        if !matches!(me.class, ObjectClass::Building(0x34..=0x36)) {
            return true;
        }
        let Some(c) = me.combat.as_ref() else {
            return false;
        };
        let side = if matches!(it.class, ObjectClass::Barbarian(_))
            || it.owner == Some(combat::MONSTERS)
        {
            2
        } else if it.owner.is_some() {
            1
        } else {
            0
        };
        let class = combat::class_bit(it.record.class_id, it.ship.as_ref().map(|s| s.kind));
        c.fire.masks.pass(item, side, it.owner, class)
    }

    /// A blow of bomb kind `kind` from `shooter` reaches fort `t` firing on
    /// aggressors (0x4184f0 at 0x418bdd, after the units' share: message
    /// 0x138e `+4` the kind, `+0x16` the shooter, the bomb's `+0xbe`): the
    /// groups `aggressor_groups(kind)` names fire at it (0x446960).
    pub(super) fn fort_aggressed(&mut self, t: usize, kind: u8, shooter: Option<u32>) {
        let Some(shooter) = shooter.filter(|&s| self.object_index(s).is_some()) else {
            return;
        };
        if !matches!(self.objects[t].class, ObjectClass::Building(0x34..=0x36)) {
            return;
        }
        let Some(c) = self.objects[t].combat.as_mut() else {
            return;
        };
        if !c.fire.aggressor {
            return;
        }
        let mask = combat::aggressor_groups(kind as u32);
        for k in 0..12 {
            if mask >> k & 1 != 0 {
                c.fire.targets[k] = shooter;
            }
        }
    }

    /// Armed ships and forts pick targets, ships close in, and everything in
    /// range fires; sunk ships and destroyed buildings are removed.
    pub(super) fn run_combat(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let dist =
            |a: &MapObject, b: &MapObject| ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt();
        let armed: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.is_armed())
            .map(|o| o.id)
            .collect();
        let mut volleys = Vec::new();
        for id in armed {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            // A ship being built does no more than rise (0x4ab930 runs
            // only its task 4 while `+0x251 == 2`).
            if self.objects[i]
                .ship
                .as_ref()
                .is_some_and(|s| s.build.is_some())
            {
                continue;
            }
            let levels = self.levels_of(self.objects[i].owner);
            let c = self.objects[i].combat.as_mut().unwrap();
            if c.stone > 0.0 {
                continue;
            }
            // Limed, a fort or stronghold does not fire (0x419e00,
            // 0x40ef70), nor a ship (0x4e6700 shuts 0x4ab930's gate) unless
            // its ram or board switch is on and it is a royal ship, rammer
            // or ship killer (0x597a30) or carries a hero who fights
            // (0x4e66c0: 0x4b4dd0 over 0x5991e0, every hero but Orpheus).
            if c.lime > 0.0 {
                let fights_on = self.objects[i].ship.as_ref().is_some_and(|s| {
                    (s.behaviour.ram || s.behaviour.board)
                        && (matches!(s.kind, 2 | 5 | 6)
                            || s.heroes.iter().any(|&h| h != objects::ORPHEUS))
                });
                if !fights_on {
                    continue;
                }
            }
            let shooters = self.objects[i].shooters();
            let c = self.objects[i].combat.as_mut().unwrap();
            c.reloads.resize(shooters.len(), 0.0);
            for r in &mut c.reloads {
                *r = (*r - dt).max(0.0);
            }
            let ship = self.objects[i].ship.as_ref();
            let theseus = ship.is_some_and(|s| s.has_hero(objects::THESEUS));
            let atalanta = ship.is_some_and(|s| s.has_hero(objects::ATALANTA));
            let reach_level = |w: u8| {
                if theseus {
                    objects::HERO_LEVEL
                } else {
                    levels.reach_for(w)
                }
            };
            let rate_level = |w: u8| {
                if atalanta {
                    objects::HERO_LEVEL
                } else {
                    levels.rate_for(w)
                }
            };
            // The longest of its weapons' ranges.
            let range = shooters
                .iter()
                .map(|&w| combat::reach(w, reach_level(w)).1)
                .fold(0.0, f32::max);
            let (ordered, idle) = match &self.objects[i].ship {
                Some(ship) => (
                    match ship.task {
                        Some(Task::Attack { target }) => Some(target),
                        Some(Task::Zone { rect, attack }) => self.zone_target(i, rect, attack),
                        _ => None,
                    },
                    ship.behaviour.guard
                        && ((ship.task.is_none() && ship.path.is_empty())
                            || matches!(
                                ship.task,
                                Some(Task::Patrol { .. } | Task::Convoy { .. })
                            )),
                ),
                // Forts and barbarian structures scan only while their
                // groups fire at will, and with no mode set on any group
                // (order 10, 0x4469b0) do not fire at all (0x4469c0).
                None => match self.objects[i].combat.as_ref().map(|c| c.fire) {
                    Some(fire) if self.objects[i].holds_units() => {
                        if !fire.fires() {
                            continue;
                        }
                        (None, fire.at_will)
                    }
                    _ => (None, true),
                },
            };
            let me = &self.objects[i];
            let target = match ordered {
                // A target gone, or no longer to be fought (a building
                // fallen to ruins, a ship captured), ends the attack.
                Some(t) => match self.object_index(t).filter(|&ti| {
                    self.objects[ti].combat.is_some()
                        && Self::are_enemies(&self.objects[i], &self.objects[ti])
                }) {
                    Some(ti) => Some(ti),
                    None => {
                        let ship = self.objects[i].ship.as_mut().unwrap();
                        if matches!(ship.task, Some(Task::Attack { .. })) {
                            ship.task = None;
                            ship.path.clear();
                        }
                        continue;
                    }
                },
                // Idle ships and forts fire at enemy ships in range.
                None if idle => self
                    .objects
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| {
                        o.ship.is_some()
                            && Self::are_enemies(me, o)
                            && !self.holds_fire(me, o)
                            && dist(me, o) <= range
                            && o.combat.as_ref().is_none_or(|c| c.phantom <= 0.0)
                            && !(me.owner == Some(combat::MONSTERS)
                                && o.ship
                                    .as_ref()
                                    .is_some_and(|s| s.has_hero(objects::ORPHEUS)))
                    })
                    .min_by(|a, b| dist(me, a.1).total_cmp(&dist(me, b.1)))
                    .map(|(ti, _)| ti),
                None => None,
            };
            // A fort's or barbarian structure's groups (item k's units are
            // group k) each fire at their target while it is in their
            // reach (0x447400; out of reach it is kept), else, at will, at
            // what the scan found (0x4469c0).
            if self.objects[i].holds_units() {
                let fire = self.objects[i].combat.as_ref().unwrap().fire;
                let tracked = fire.targets.map(|t| {
                    (t != 0)
                        .then(|| self.object_index(t))
                        .flatten()
                        .filter(|&ti| {
                            self.objects[ti].combat.is_some()
                                && Self::are_enemies(&self.objects[i], &self.objects[ti])
                        })
                });
                let picks: Vec<Option<usize>> = shooters
                    .iter()
                    .map(|&w| {
                        let (near, far) = combat::reach(w, reach_level(w));
                        let reaches = |ti: usize| {
                            let d = dist(&self.objects[i], &self.objects[ti]);
                            d >= near && d <= far
                        };
                        // A fort's group shoots only what its masks let
                        // through (0x41c4e0, called by 0x447400 and the
                        // scan 0x446d60).
                        let may = |ti: usize| self.fort_may_shoot(i, w, ti);
                        match tracked.get(w as usize).copied().flatten() {
                            Some(ti) if reaches(ti) && may(ti) => Some(ti),
                            _ => target.filter(|&ti| reaches(ti) && may(ti)),
                        }
                    })
                    .collect();
                let c = self.objects[i].combat.as_mut().unwrap();
                let mut fired: Vec<(usize, Vec<u8>)> = Vec::new();
                for (k, (&w, pick)) in shooters.iter().zip(&picks).enumerate() {
                    let Some(ti) = *pick else { continue };
                    if c.reloads[k] > 0.0 {
                        continue;
                    }
                    let jitter = crate::clock::ticks(self.rng.next() % combat::RELOAD_JITTER);
                    c.reloads[k] = combat::reload_seconds(w, rate_level(w)) + jitter;
                    match fired.iter_mut().find(|f| f.0 == ti) {
                        Some(f) => f.1.push(w),
                        None => fired.push((ti, vec![w])),
                    }
                }
                for (ti, weapons) in fired {
                    let owner = self.objects[i].owner;
                    volleys.push((self.objects[ti].id, weapons, owner, id, theseus));
                }
                continue;
            }
            let Some(ti) = target else { continue };
            // A guard set to ram or board goes in rather than firing.
            if ordered.is_none() {
                let target = self.objects[ti].id;
                if let Some(ship) = self.objects[i].ship.as_mut() {
                    let b = ship.behaviour;
                    if b.ram || (b.board && ship.crew.count() > 0) {
                        ship.task = Some(if b.ram {
                            Task::Ram { target }
                        } else {
                            Task::Board {
                                target,
                                thrown: None,
                                hooking: false,
                            }
                        });
                        continue;
                    }
                }
            }
            let d = dist(&self.objects[i], &self.objects[ti]);
            let (tx, ty) = (self.objects[ti].x, self.objects[ti].y);
            if d > range {
                // Chase: re-plan when the target has moved away from the route's end.
                let Some(ship) = self.objects[i].ship.as_ref() else {
                    continue;
                };
                // A ship may sail off: follow it. A building or creature on
                // land stays put, and the route ends where the water does,
                // maybe far from it: plan once, not every tick.
                let moving = self.objects[ti].ship.is_some();
                let stale = match ship.path.back() {
                    None => true,
                    Some(&(px, py)) => {
                        moving && ((px - tx).powi(2) + (py - ty).powi(2)).sqrt() > range / 2.0
                    }
                };
                if ordered.is_some() && stale {
                    if let Some(path) = self.plan_route(i, tx, ty) {
                        self.objects[i].ship.as_mut().unwrap().path = path;
                    }
                }
                continue;
            }
            let target_id = self.objects[ti].id;
            // In range: it stops - at the end of the step it is on, ships
            // keeping to the grid's cell centres.
            if ordered.is_some() {
                let path = &mut self.objects[i].ship.as_mut().unwrap().path;
                path.truncate(1);
            }
            // Each weapon fires when it is loaded and the target lies
            // between its shortest and longest range.
            let c = self.objects[i].combat.as_mut().unwrap();
            let mut fired = Vec::new();
            for (k, &w) in shooters.iter().enumerate() {
                let (near, far) = combat::reach(w, reach_level(w));
                if c.reloads[k] <= 0.0 && d >= near && d <= far {
                    let jitter = crate::clock::ticks(self.rng.next() % combat::RELOAD_JITTER);
                    c.reloads[k] = combat::reload_seconds(w, rate_level(w)) + jitter;
                    fired.push(w);
                }
            }
            if !fired.is_empty() {
                volleys.push((target_id, fired, self.objects[i].owner, id, theseus));
            }
        }
        for (target, weapons, owner, shooter, theseus) in volleys {
            self.scout_attacked(target);
            self.wizard_attacked(target, shooter);
            self.computer_attacked(target, shooter);
            self.brain_hit(target);
            // A brain's fleet notes who fired on it, whoever it was.
            self.fleet_attacked(target, shooter);
            // A guard told to fire on aggressors turns on whoever shoots it.
            if let Some(ship) = self
                .object_index(target)
                .and_then(|t| self.objects[t].ship.as_mut())
            {
                let b = ship.behaviour;
                if b.guard && b.aggressor && ship.task.is_none() {
                    ship.path.clear();
                    ship.task = Some(Task::Attack { target: shooter });
                }
            }
            let levels = self.levels_of(owner);
            let warded = self
                .object_index(target)
                .and_then(|t| self.objects[t].combat.as_ref())
                .is_some_and(|c| c.protect_weapons > 0.0);
            // What the target's state takes off every blow: the weapon
            // shield 80%, the Flame and Theseus add; on a ship the Armour of
            // Ares and the armour cut it, and it is shared out on arrival
            // (0x4bf170, `combat::Shares`).
            let mut factor = 1.0;
            // A fort shares its blow out first and cuts each part under the
            // shield on its own (`fort_blow`).
            let holder = self
                .object_index(target)
                .is_some_and(|t| self.objects[t].holds_units());
            if warded && !holder {
                factor *= magic::SHIELD_FACTOR;
            }
            if theseus {
                factor *= objects::THESEUS_DAMAGE;
            }
            let flaming = self.flame_of(owner) > 0.0;
            if flaming {
                factor *= magic::FLAME_FACTOR;
            }
            let mut shares = None;
            if let Some(t) = self.object_index(target) {
                if let Some(ship) = self.objects[t].ship.as_ref() {
                    if let Some(a) = ship
                        .treasures
                        .iter()
                        .find(|t| t.kind == objects::ARMOUR_OF_ARES)
                    {
                        let share = if a.amount > 0 {
                            a.amount
                        } else {
                            objects::ARMOUR_OF_ARES_PERCENT
                        };
                        factor *= 1.0 - share.min(100) as f32 / 100.0;
                    }
                }
                if let (Some(ship), Some(c)) = (&self.objects[t].ship, &self.objects[t].combat) {
                    let levels = self.levels_of(self.objects[t].owner);
                    factor *= combat::armour_factor(ship.armour, &levels);
                    shares = Some(combat::Shares::of(ship, &c.weapons));
                }
            }
            // Each weapon sends a shot off, carrying its blow (bomb.cpp).
            let Some(s) = self.object_index(shooter) else {
                continue;
            };
            let from = (self.objects[s].x, self.objects[s].y);
            let Some(t) = self.object_index(target) else {
                continue;
            };
            // Land creatures throw as the original's (CyclopClassTy
            // 0x412740, 0x4126f0): their own picture, a level-1 weapon's
            // blow less as they weaken (0x594ab8), sure to hit but for a
            // 1 in (8 - cells) miss from 2 cells off.
            let thrower = match self.objects[s].class {
                ObjectClass::Monster(c @ 0x1f4..=0x1f6) => Some(c),
                _ => None,
            };
            // The player each shot is put down to (message 0x14ba's `+4`):
            // a ship's `+0x24f` (0x4c026b), a fort's player (0x4469c0); a
            // land creature's throw none (0x412900 gives 0xff, 0x4129f3).
            let by = match thrower {
                Some(_) => None,
                None => self.objects[s].owner,
            };
            let health = self.objects[s]
                .combat
                .as_ref()
                .map_or(1.0, |c| (c.hull / c.max_hull.max(1.0)).clamp(0.0, 1.0));
            let dist = (self.objects[t].x - from.0).hypot(self.objects[t].y - from.1);
            let shooters = self.objects[s].shooters();
            let same = |w: u8| shooters.iter().filter(|&&x| x == w).count();
            let target_is_ship = self.objects[t].ship.is_some();
            for w in weapons {
                let aim = match (&self.objects[t].ship, &self.objects[t].combat) {
                    (Some(ship), c) => combat::Aim::Ship {
                        kind: ship.kind,
                        zetes: ship.has_hero(objects::ZETES),
                        phantom: c.as_ref().is_some_and(|c| c.phantom > 0.0),
                    },
                    _ => combat::Aim::Other,
                };
                let (strikes, level, letter, weaken) = match thrower {
                    Some(class) => {
                        let cells = (dist / TILE_SIZE as f32) as u32;
                        let missed = (2..8).contains(&cells) && self.rng.next() % (8 - cells) == 0;
                        (
                            !missed,
                            1,
                            Some(combat::throw_letter(class)),
                            combat::throw_weakness(health),
                        )
                    }
                    None => {
                        // A sure hit strikes whole; a miss strikes the hull
                        // anyway for its chance in percent of the blow,
                        // unless it is the ship's only one of the item, when
                        // it flies wide (0x4c0230, 0x4bf7f0, 0x4bfc20).
                        let chance = combat::hit_chance(w, dist, aim);
                        let accuracy = if self.rng.chance(chance) {
                            100
                        } else if same(w) > 1 || !target_is_ship {
                            chance
                        } else {
                            0
                        };
                        (
                            accuracy > 0,
                            levels.arms_for(w),
                            None,
                            accuracy as f32 / 100.0,
                        )
                    }
                };
                // A ship's weapons of a kind fire by their mean condition;
                // quick lime by the ship's hull instead (0x4bc360, 0x4bfc20).
                let blow = if w == combat::QUICKLIME {
                    combat::lime_power(level) * health * factor * weaken
                } else {
                    let worn = self.objects[s].mean_condition(w) as f32 / 100.0;
                    combat::hull_damage_at(w, level) * factor * weaken * worn
                };
                // The hull's share, the crew's (by the item's B over its
                // A, 0x4ac1b0) and the weapons', these in the units'
                // points (blows here are a fifteenth of the original's).
                let (damage, wounds, wear) = match shares {
                    Some(sh) => (
                        sh.hull_part(blow) * combat::hull_percent(w) as f32 / 100.0,
                        sh.crew_part(blow) * 15.0 * combat::crew_scale(w),
                        sh.weapons_part(blow) * 15.0,
                    ),
                    None => (blow, 0.0, 0.0),
                };
                self.fire_shot(
                    super::shot::Firing {
                        item: w,
                        letter,
                        flaming,
                        from,
                        target,
                        strikes,
                        damage,
                        wounds,
                        wear,
                        rope: None,
                        shooter: Some(shooter),
                        by,
                    },
                    data,
                    caches,
                );
            }
        }
        for fx in &mut self.effects {
            fx.age_ms += dt * 1000.0;
        }
        self.effects.retain(|fx| fx.frame().is_some());
    }

    /// Takes `points` off the marines of ship `target` (0x4be410, see
    /// `combat::unit_blow`); those broken are gone. Boats and merchants
    /// carry their marines as cargo, out of reach, and the ship of the
    /// dead's crew takes nothing.
    pub(super) fn wound(&mut self, target: u32, points: f32) {
        let Some(t) = self.object_index(target) else {
            return;
        };
        let o = &mut self.objects[t];
        if o.combat.is_none() || o.ship.as_ref().is_none_or(|s| s.kind <= 1 || s.kind == 11) {
            return;
        }
        let units = o.units().unwrap();
        if combat::unit_blow(units, 0..3, points.max(0.0) as u32, false, &mut self.rng) {
            o.drop_broken_units();
        }
    }

    /// A blow on a fort or barbarian structure (message 0x138e:
    /// FortressClassTy 0x4184f0, BarbarianClassTy 0x40e0f0): its units take
    /// their share (`combat::fort_share`), the weapons 3-8 all of it and the
    /// marines it by the shot's B over its A (0x4ac1b0), each spread by
    /// `combat::fort_unit_blow`; the structure takes the rest. `damage` is
    /// the shot's blow (ours, a fifteenth of the original's), `lime` quick
    /// lime's, which spares structures, in the units' points. Under a magic
    /// shield (`+0x15c`) the structure takes a fifth and a fort's units a
    /// fifth of a fireball's share and two thirds of a flaming shot's.
    /// `by`: the player who dealt it (see `strike`); `shooter`: who fired
    /// it (message 0x138e `+0x16`), for a fort firing on aggressors.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fort_blow(
        &mut self,
        target: u32,
        damage: f32,
        lime: f32,
        item: u8,
        flaming: bool,
        by: Option<Owner>,
        shooter: Option<u32>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(t) = self.object_index(target) else {
            return;
        };
        let o = &self.objects[t];
        let class = o.record.class_id;
        let Some(c) = o.combat.as_ref() else {
            return;
        };
        let shielded = c.protect_weapons > 0.0;
        let mut blow = if damage > 0.0 {
            (damage * 15.0) as u32
        } else {
            lime.max(0.0) as u32
        };
        // A fort's Torch of Ares takes a third off a flaming blow and puts
        // its flame out (0x418f60, flag `+0x4ac` from its store, 0x419e00).
        // Its Armour of Ares (`+0x4b0`, the treasure's amount in percent)
        // only stops a blow it would cut to nothing: the original reckons
        // the cut, then posts the blow uncut. (Both at once would post the
        // blow twice; a fort stores one treasure.)
        let mut damage = damage;
        let mut flaming = flaming;
        if (0x34..=0x36).contains(&class) {
            let held: Vec<objects::Treasure> = self.held_by(target).into_iter().copied().collect();
            if flaming && held.iter().any(|tr| tr.kind == objects::FIRE_TORCH) {
                let before = blow;
                blow = blow * 100 / 150;
                flaming = false;
                if before > 0 && damage > 0.0 {
                    damage *= blow as f32 / before as f32;
                }
            }
            if let Some(ares) = held.iter().find(|tr| tr.kind == objects::ARMOUR_OF_ARES) {
                let pct = ares.amount;
                if pct > 0 && blow as u64 * pct as u64 / 100 == blow as u64 {
                    return;
                }
            }
        }
        let c = self.objects[t].combat.as_ref().unwrap();
        let mut share = combat::fort_share(class, &c.weapons, blow);
        let mut hull = if blow > 0 && damage > 0.0 {
            damage * (blow - share) as f32 / blow as f32
        } else {
            damage
        };
        if shielded {
            hull *= magic::SHIELD_FACTOR;
            if (0x34..=0x36).contains(&class) {
                if item == super::shot::FIREBALL {
                    share = share * 20 / 100;
                } else if flaming {
                    share = share * 100 / 150;
                }
            }
        }
        if share > 0 {
            let crew = (share as f32 * combat::crew_scale(item)) as u32;
            let mut places = self.objects[t].fort_places();
            combat::fort_unit_blow(&mut places, 3..9, share, &mut self.rng);
            combat::fort_unit_blow(&mut places, 0..3, crew, &mut self.rng);
            self.objects[t].set_fort_places(places);
            // A unit gone from a fort being mended leaves its mending
            // (0x418430).
            self.forget_gone_units(target);
        }
        // Firing on aggressors, it turns on the shooter.
        if let Some(t) = self.object_index(target) {
            self.fort_aggressed(t, item, shooter);
        }
        self.strike(target, hull, false, by, true, data, caches);
    }

    /// Takes `points` off the weapons of ship `target` (0x4be110); those
    /// broken are gone, but a boat's or merchant's own is kept at 10%.
    pub(super) fn wear(&mut self, target: u32, points: f32) {
        let Some(t) = self.object_index(target) else {
            return;
        };
        let o = &mut self.objects[t];
        let Some(small) = o.ship.as_ref().map(|s| s.kind <= 1) else {
            return;
        };
        if o.combat.is_none() {
            return;
        }
        let units = o.units().unwrap();
        if combat::unit_blow(units, 3..9, points.max(0.0) as u32, small, &mut self.rng) {
            o.drop_broken_units();
        }
    }

    /// A blow `blow` to ship `target` that is shared out (0x4befc0): the
    /// hull, then the crew and the weapons each take their share (crew's and
    /// weapons' in the units' points, the original's blows being fifteen
    /// times ours). Other objects take it whole.
    pub(super) fn share_blow(
        &mut self,
        target: u32,
        blow: f32,
        by: Option<Owner>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let shares = self.object_index(target).and_then(|t| {
            let o = &self.objects[t];
            Some(combat::Shares::of(
                o.ship.as_ref()?,
                &o.combat.as_ref()?.weapons,
            ))
        });
        let Some(sh) = shares else {
            self.hit(target, blow, by, data, caches);
            return;
        };
        // A ship being built (0x4befc0 with `+0x251 == 2`) takes the blow
        // whole on its hull, in the original's points, never below 0; under
        // 1 it sinks (`+0x251 = 4`), the blow's giver (`+0x80b`) its killer
        // whether it sinks or not: the ordinary sinking, nothing given back.
        let building = self
            .object_index(target)
            .and_then(|t| self.objects[t].ship.as_ref())
            .is_some_and(|s| s.build.is_some());
        if building {
            let t = self.object_index(target).unwrap();
            let c = self.objects[t].combat.as_mut().unwrap();
            let hull = ((c.hull * 15.0).round() as i64 - (blow * 15.0).round() as i64).max(0);
            c.hull = hull as f32 / 15.0;
            if hull < 1 {
                c.hull = 0.0;
                if let Some(s) = self.objects[t].ship.as_mut() {
                    s.build = None;
                }
                self.strike(target, 0.0, false, by, true, data, caches);
            }
            return;
        }
        // The hull first; a wreck takes no more (0x4be0c0).
        self.hit(target, sh.hull_part(blow), by, data, caches);
        self.wound(target, sh.crew_part(blow) * 15.0);
        self.wear(target, sh.weapons_part(blow) * 15.0);
    }

    /// Deals `damage` to the hull of `target`, with a hit (or splash) effect;
    /// a sunk ship is removed and a destroyed building left in ruins.
    pub(super) fn hit(
        &mut self,
        target: u32,
        damage: f32,
        by: Option<Owner>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        // Asleep, it ignores blows (0xda96, 0x158bf).
        if self
            .object_index(target)
            .is_some_and(|t| self.objects[t].asleep.is_some())
        {
            return;
        }
        // A fort's or barbarian structure's every blow comes through
        // message 0x138e, its units taking their share: spells, tornadoes
        // and the like too, not only shots. (No shooter for a fort firing
        // on aggressors: the stare's 0x158bf has `+0x16` 0, 0x41a041; the
        // other senders' `+0x16` not decoded.)
        if self
            .object_index(target)
            .is_some_and(|t| self.objects[t].holds_units())
        {
            self.fort_blow(
                target,
                damage,
                0.0,
                NOT_A_WEAPON,
                false,
                by,
                None,
                data,
                caches,
            );
            return;
        }
        self.strike(target, damage, true, by, true, data, caches);
    }

    /// The same, showing the hit or splash only if `show` (a shot shows
    /// its own). `by` is the player who dealt the blow: the one the
    /// statistics take for the killer (a ship's sinking blow keeps its
    /// giver, `+0x80b` to `+0x7f`, 0x4befc0, 0x4be0c0, and the death
    /// reports it to 0x50e750); `None` for none (0xff). Unless `report`,
    /// a death is not reported at all (a harpy or dragon shot down).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn strike(
        &mut self,
        target: u32,
        damage: f32,
        show: bool,
        by: Option<Owner>,
        report: bool,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(ti) = self.object_index(target) else {
            return;
        };
        // Where the player was last struck (Backspace goes there).
        if damage > 0.0 && self.is_own(&self.objects[ti]) {
            self.last_attack = Some((self.objects[ti].x, self.objects[ti].y));
        }
        let (tx, ty) = (self.objects[ti].x, self.objects[ti].y);
        let jitter = |rng: &mut Rng| (rng.next() % 33) as f32 - 16.0;
        let (fx, fy) = (tx + jitter(&mut self.rng), ty + jitter(&mut self.rng));
        let Some(c) = self.objects[ti].combat.as_mut() else {
            return;
        };
        c.hull -= damage;
        // A sea monster struck to nothing dies at its next turn under water
        // (0x491fa0), its kill reported now (0x50e750): on every blow that
        // leaves it at nothing, not only the one that takes it there
        // (0x490c70 code 0 tests the life after the blow, not before), each
        // a full count.
        let hull = c.hull;
        let monster = self.objects[ti].class == ObjectClass::Monster(0x1fb)
            && self.sea_monster_states.contains_key(&target);
        if monster && hull <= 0.0 {
            self.objects[ti].combat.as_mut().unwrap().hull = 0.0;
            if report {
                self.note_destroyed(ti, by);
            }
        }
        let c = self.objects[ti].combat.as_mut().unwrap();
        let destroyed = c.hull <= 0.0 && !monster;
        if show {
            let name = if damage > 0.0 {
                combat::FX_HIT
            } else {
                combat::FX_SPLASH
            };
            self.spawn_effect(data, caches, name, fx, fy);
            self.sounds
                .push((if damage > 0.0 { "hitmed" } else { "splash1" }, fx, fy));
        }
        let is_building = self.objects[ti].ship.is_none();
        if destroyed {
            if report {
                self.note_destroyed(ti, by);
            }
            self.drop_held(target, data, caches);
            // A building gone leaves its town's list (the drain 0x427bb0,
            // 0x2874); a settlement or town gone tells its buildings.
            self.leave_town(target, false);
            if matches!(
                self.objects[ti].class,
                ObjectClass::Building(SETTLEMENT | TOWN)
            ) {
                self.town_gone(target, data, caches);
            }
            let Some(ti) = self.object_index(target) else {
                return;
            };
            // Creatures die with their own cry (land creatures 0x413ad0:
            // events 100-102 by class; the harpy 0x41fb02 event 103; the
            // sea's creatures events 105-107), buildings fall, ships wreck.
            let sound = match self.objects[ti].class {
                ObjectClass::Monster(0x1f4 | 0x1f5) => "deac1d",
                ObjectClass::Monster(0x1f6) => "destr1",
                ObjectClass::Monster(0x1f8) => "deadga1",
                ObjectClass::Monster(0x1f9) => "dedr2",
                ObjectClass::Monster(_) => "deamons",
                _ if is_building => "razru2",
                _ => "wreck1",
            };
            self.sounds.push((sound, tx, ty));
            // A ship gone: a lament if it was the player's, a fanfare
            // ("deny", event 319) if not (0x4dec40).
            if !is_building {
                let own = self.is_own(&self.objects[ti]);
                self.sounds
                    .push((if own { "deadsh" } else { "deny" }, tx, ty));
            }
            self.ship_orders.retain(|p| p.site != target);
            self.constructions.retain(|c| c.site != target);
            match self.objects[ti].ship.as_ref().map(|sh| sh.kind) {
                // Ships go down with the sinking animation of their hull.
                Some(kind) => {
                    let nation = self.objects[ti].record.fields.get(5).copied().unwrap_or(1);
                    match objects::ship_mode_frames(data, caches, kind, nation, 1) {
                        Ok(frames) if !frames.is_empty() => self.effects.push(Effect {
                            x: tx,
                            y: ty,
                            frames,
                            age_ms: 0.0,
                            follow: None,
                            life_ms: None,
                            frame_ms: crate::combat::FX_FRAME_MS,
                            tornado: None,
                            site: None,
                        }),
                        _ => self.spawn_effect(data, caches, combat::FX_SINK, tx, ty),
                    }
                }
                None => self.spawn_effect(data, caches, combat::FX_SINK, tx, ty),
            }
            let ruins = is_building
                .then(|| {
                    objects::building_frames(
                        data,
                        caches,
                        &self.objects[ti].record,
                        BuildingState::Ruins,
                    )
                    .ok()
                })
                .flatten()
                .filter(|f| !f.is_empty());
            match ruins {
                // Ruins stay as scenery, owned by no one.
                Some(frames) => {
                    let obj = &mut self.objects[ti];
                    obj.frames = frames;
                    obj.combat = None;
                    obj.owner = None;
                    // The collapse plays once.
                    obj.progress = Some(0.0);
                    obj.picture = None;
                    obj.label = format!("Ruins ({})", obj.label);
                }
                None => {
                    self.objects.remove(ti);
                }
            }
        } else if is_building && self.construction_at(target).is_none() {
            self.refresh_building(target, data, caches);
        }
    }

    /// Redraws ships whose look changed (0x4be030 after each blow or
    /// mending: the hull band; refits: ram and armour).
    pub(super) fn refresh_ship_looks(&mut self, data: &GameData, caches: &mut Caches) {
        for o in &mut self.objects {
            let Some(ship) = o.ship.as_ref() else {
                continue;
            };
            // A ship being built shows its building sequence.
            if o.frames.is_empty() || ship.build.is_some() {
                continue;
            }
            let look = objects::ShipLook::of(ship, o.combat.as_ref());
            if ship.look == Some(look) {
                continue;
            }
            let f = |i: usize| o.record.fields.get(i).copied().unwrap_or(0);
            match objects::ship_frames(data, caches, ship.kind, f(5), f(6), look) {
                Ok(frames) if !frames.is_empty() => o.frames = frames,
                Ok(_) => {}
                Err(e) => eprintln!("could not redraw ship {}: {e:#}", o.id),
            }
            o.ship.as_mut().unwrap().look = Some(look);
        }
    }

    /// Buildings fallen to ruins play their collapse once, over the third
    /// entry of their class's cycle table (+0xc0: 75 ticks for every
    /// player building and barbarian structure), then keep its last frame
    /// (for a barbarian structure, the dust it leaves).
    pub(super) fn run_collapses(&mut self, dt: f32) {
        let step = dt / crate::clock::ticks(COLLAPSE_TICKS);
        for o in &mut self.objects {
            if o.combat.is_none() && o.owner.is_none() && o.ship.is_none() {
                if let Some(p) = o.progress.as_mut() {
                    if *p < 1.0 {
                        *p = (*p + step).min(1.0);
                    }
                }
            }
        }
    }

    /// Orders ship `ship` to ram or board the enemy ship `target`.
    pub fn order_close(&mut self, ship: u32, target: u32, board: bool) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let t = self.object_index(target).ok_or("No such target")?;
        if self.objects[t].ship.is_none() || !Self::are_enemies(&self.objects[i], &self.objects[t])
        {
            return Err("Choose an enemy ship".into());
        }
        let ship = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        if board && ship.crew.count() == 0 {
            return Err("There is no crew to board with".into());
        }
        ship.path.clear();
        ship.task = Some(if board {
            Task::Board {
                target,
                thrown: None,
                hooking: false,
            }
        } else {
            Task::Ram { target }
        });
        Ok(())
    }

    /// Switches one of own ship `ship`'s behaviours (`commands::GUARD`,
    /// `RAM_ATTACK`, `BOARD_ATTACK`, `FIRE_ON_AGGRESSOR`).
    pub fn toggle_behaviour(
        &mut self,
        ship: u32,
        command: crate::commands::Command,
    ) -> Result<(), String> {
        use crate::commands as c;
        let i = self.object_index(ship).ok_or("No such ship")?;
        if !self.is_own(&self.objects[i]) {
            return Err("Not your ship".into());
        }
        let b = &mut self.objects[i].ship.as_mut().ok_or("Not a ship")?.behaviour;
        match command {
            c::GUARD => b.guard = !b.guard,
            c::RAM_ATTACK => b.ram = !b.ram,
            c::BOARD_ATTACK => b.board = !b.board,
            c::FIRE_ON_AGGRESSOR if b.guard => b.aggressor = !b.aggressor,
            c::FIRE_ON_AGGRESSOR => return Err("The ship must be on guard".into()),
            _ => return Err("Not a behaviour".into()),
        }
        Ok(())
    }

    /// Ships ordered to ram or board close in on their target to a
    /// neighbouring cell (0x4b7f10, 0x4c1f60); there a rammer charges it
    /// and a boarder throws hooks at it (see `grapple`).
    pub(super) fn run_contact(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.run_grapples(dt, data, caches);
        self.run_ram_runs(dt, data, caches);
        let closing: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship.as_ref().is_some_and(|s| {
                    s.grapple.is_none()
                        && s.ram_run.is_none()
                        && matches!(s.task, Some(Task::Ram { .. } | Task::Board { .. }))
                })
            })
            .map(|o| o.id)
            .collect();
        for id in closing {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let task = self.objects[i].ship.as_ref().unwrap().task.unwrap();
            let (Task::Ram { target } | Task::Board { target, .. }) = task else {
                continue;
            };
            let Some(ti) = self.object_index(target).filter(|&t| {
                self.objects[t].ship.is_some()
                    && Self::are_enemies(&self.objects[i], &self.objects[t])
            }) else {
                self.objects[i].ship.as_mut().unwrap().task = None;
                continue;
            };
            // A boarder waits for its hook to land (0x4b5df0 state 7); one
            // lost (a save does not keep it) frees it.
            if let Task::Board {
                hooking: true,
                thrown,
                ..
            } = task
            {
                if self.shots.iter().any(|s| s.rope == Some(id)) {
                    self.objects[i].ship.as_mut().unwrap().path.clear();
                    continue;
                }
                self.objects[i].ship.as_mut().unwrap().task = Some(Task::Board {
                    target,
                    thrown,
                    hooking: false,
                });
            }
            let (tx, ty) = (self.objects[ti].x, self.objects[ti].y);
            // Ships touch from neighbouring cells: one ship to a cell.
            let t = TILE_SIZE as f32;
            let cell = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
            let (a, b) = (cell(self.objects[i].x, self.objects[i].y), cell(tx, ty));
            if (a.0 - b.0).abs() > 1 || (a.1 - b.1).abs() > 1 {
                let ship = self.objects[i].ship.as_ref().unwrap();
                let stale = ship
                    .path
                    .back()
                    .is_none_or(|&(px, py)| ((px - tx).powi(2) + (py - ty).powi(2)).sqrt() > t);
                if stale {
                    if let Some(path) = self.plan_route(i, tx, ty) {
                        self.objects[i].ship.as_mut().unwrap().path = path;
                    }
                }
                continue;
            }
            self.objects[i].ship.as_mut().unwrap().path.clear();
            match task {
                Task::Ram { .. } => self.start_ram(i, ti),
                Task::Board { .. } => self.throw_hook(i, ti, data, caches),
                _ => {}
            }
        }
    }

    /// Ship `i` rams ship `ti` (0x4c1c60): both take blows by the closing
    /// speed, their ram quality and power, the angle and their hulls (see
    /// `combat::ram_blows`); the rammed ship's armour softens its blow
    /// (0x4c1ad0), and each hull takes its share.
    pub(super) fn ram(&mut self, i: usize, ti: usize, data: &GameData, caches: &mut Caches) {
        let (a, t) = (&self.objects[i], &self.objects[ti]);
        let (sa, st) = (a.ship.as_ref().unwrap(), t.ship.as_ref().unwrap());
        let angle = combat::ram_angle(sa.heading, st.heading);
        // Each feather of Boreas aboard adds its amount in percent to the
        // rammer's blow in turn (0x4c1c60: Ba += amount x Ba / 100).
        let feather: f32 = sa
            .treasures
            .iter()
            .filter(|t| t.kind == objects::FEATHER)
            .map(|t| 1.0 + t.amount as f32 / 100.0)
            .product();
        // Not decoded: the two ships' way (`+0x59b`) when the blow falls,
        // after the impact picture (0x4a3f40 over those frames); the
        // rammer charges at no less than its cruising speed here.
        let (to_target, to_self) = combat::ram_blows(
            combat::Rammer {
                kind: sa.kind,
                ram: sa.ram,
                level: self.levels_of(a.owner).ram_kinds[sa.ram.min(2) as usize],
                heading: sa.heading,
                speed: sa.velocity.max(sa.speed),
                rigging: self.levels_of(a.owner).speed,
            },
            combat::Rammer {
                kind: st.kind,
                ram: st.ram,
                level: self.levels_of(t.owner).ram_kinds[st.ram.min(2) as usize],
                heading: st.heading,
                speed: st.velocity,
                rigging: self.levels_of(t.owner).speed,
            },
            angle,
        );
        // Argus takes three quarters off a ram's blow.
        let argus = |o: &MapObject| {
            if o.ship.as_ref().is_some_and(|s| s.has_hero(objects::ARGUS)) {
                0.25
            } else {
                1.0
            }
        };
        let to_target = to_target
            * feather
            * combat::armour_factor(st.armour, &self.levels_of(t.owner))
            * argus(t);
        let to_self = to_self * argus(a);
        let (ida, idt) = (a.id, t.id);
        let (by, recoil) = (a.owner, t.owner);
        self.objects[i].ship.as_mut().unwrap().task = None;
        // A fire rammer's barrel of powder goes off in the ram. (Not
        // decoded: 0x4c1c60 and 0x4c1ad0 have no such term; where the
        // barrel goes off is still to be read.)
        let c = self.objects[i].combat.as_mut().unwrap();
        let barrel = c.weapons.iter().position(|&w| w == combat::POWDER);
        let (blast, burn) = match barrel {
            Some(k) => {
                c.weapons.remove(k);
                (
                    combat::hull_damage(combat::POWDER),
                    combat::crew_damage_at(combat::POWDER, 0) * 15.0,
                )
            }
            None => (0.0, 0.0),
        };
        self.wound(idt, burn);
        // Each ship's blow is put down to the other's player (0x4c1c60:
        // the prey's 0x4c1ad0 gets the rammer's `+0x24f`, 0x4c1d63, the
        // rammer's its prey's, 0x4c1f2a).
        self.ram_blow(idt, to_target, blast, by, data, caches);
        self.ram_blow(ida, to_self, 0.0, recoil, data, caches);
    }

    /// A ram's blow `blow` to ship `id` (0x4c1ad0): the hull takes its
    /// share x 1.5 (and `blast` whole), then the crew its share x 0.4 and
    /// the weapons theirs, in the units' points.
    fn ram_blow(
        &mut self,
        id: u32,
        blow: f32,
        blast: f32,
        by: Option<Owner>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(sh) = self.object_index(id).and_then(|k| {
            let o = &self.objects[k];
            Some(combat::Shares::of(
                o.ship.as_ref()?,
                &o.combat.as_ref()?.weapons,
            ))
        }) else {
            return;
        };
        let hull = sh.hull_part(blow) * combat::RAM_HULL_FACTOR + blast;
        self.hit(id, hull, by, data, caches);
        self.wound(id, sh.crew_part(blow) * 15.0 * combat::RAM_CREW_FACTOR);
        self.wear(id, sh.weapons_part(blow) * 15.0);
    }

    /// A side's boarding strength in the original's units (0x4b4e80,
    /// 0x4bee60): each marine's condition x C x 15 / 1000 (0x4ac1e0 with
    /// the A x 15 cache; C from 0x597d2c, `melee`), ten times that on the
    /// ship of the dead.
    pub(crate) fn boarding_strength(&self, i: usize) -> u32 {
        let o = &self.objects[i];
        let Some(ship) = o.ship.as_ref() else {
            return 0;
        };
        let units = o.unit_conditions();
        let sum: u32 = (0..3)
            .flat_map(|k| {
                units[k]
                    .iter()
                    .map(move |&c| c as u32 * combat::melee(k) * 15 / 1000)
            })
            .sum();
        if ship.kind == 11 {
            sum * 10
        } else {
            sum
        }
    }

    /// A side's strength in a boarding (0x4b5b00): halved while limed.
    pub(super) fn fighting_strength(&self, i: usize) -> u32 {
        let s = self.boarding_strength(i);
        if self.objects[i]
            .combat
            .as_ref()
            .is_some_and(|c| c.lime > 0.0)
        {
            s / 2
        } else {
            s
        }
    }

    /// Decides the fight for ship `ti` boarded from `i` (0x4b5b00): with
    /// strengths a and d (each + 1), the boarders win with chance
    /// (2a - d) / (a + d); Hercules and the ship of the dead always win
    /// unless the defenders have the same. (The magic wood keeps boarders
    /// off sooner: their hooks fly wide, 0x4b84b0.)
    pub(super) fn board_roll(&mut self, i: usize, ti: usize) -> bool {
        let sure = |o: &MapObject| {
            o.ship
                .as_ref()
                .is_some_and(|s| s.has_hero(objects::HERCULES) || s.kind == 11)
        };
        let (attacker, defender) = (&self.objects[i], &self.objects[ti]);
        if sure(attacker) {
            return !sure(defender);
        }
        let a = self.fighting_strength(i) + 1;
        let d = self.fighting_strength(ti) + 1;
        let roll = self.rng.next() % (a + d);
        let line = 2 * a as i64 - d as i64;
        line >= (a + d) as i64 || (roll as i64) < line
    }

    /// The end of the fight for ship `t` boarded from `a` (0x4b5df0 state
    /// 3 for the boarders, 0x4c3410 for the defenders), with the blows of
    /// 0x4b5b00: the boarders' is one more than the defenders' strength
    /// and a hundredth of the prize's hull, the defenders' one more than
    /// the boarders' strength. Won, the boarders take a tenth of theirs ten times on their
    /// crew and weapons and a tenth on the hull, the defenders lose their
    /// marines, take theirs nine times on the weapons and seven times on
    /// the hull, and half the boarders man the prize. Lost, the boarders
    /// lose their marines, take theirs nine times on the weapons and seven
    /// tenths of it on the hull, and the defenders a tenth ten times on
    /// crew and weapons and all of it on the hull. Hercules or the ship of
    /// the dead only take theirs on the hull.
    pub(super) fn board_end(
        &mut self,
        a: u32,
        t: u32,
        won: bool,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let (Some(i), Some(ti)) = (self.object_index(a), self.object_index(t)) else {
            return;
        };
        let hull_t = self.objects[ti].combat.as_ref().map_or(0.0, |c| c.hull);
        // In the original's units: hulls here are a fifteenth.
        let blow = self.fighting_strength(ti) + 1 + (hull_t * 15.0 / 100.0) as u32;
        let foe = self.fighting_strength(i) + 1;
        let sure = |o: &MapObject| {
            o.ship
                .as_ref()
                .is_some_and(|s| s.has_hero(objects::HERCULES) || s.kind == 11)
        };
        let (sure_a, sure_t) = (sure(&self.objects[i]), sure(&self.objects[ti]));
        let (by_a, by_t) = (self.objects[i].owner, self.objects[ti].owner);
        self.objects[i].ship.as_mut().unwrap().task = None;
        if !won {
            if sure_a {
                self.hit(a, blow as f32 / 15.0, by_t, data, caches);
            } else {
                self.rout(a, blow, blow / 10 * 7, by_t, data, caches);
            }
            for _ in 0..10 {
                self.wound(t, (foe / 10) as f32);
                self.wear(t, (foe / 10) as f32);
            }
            self.hit(t, foe as f32 / 15.0, by_a, data, caches);
            return;
        }
        for _ in 0..10 {
            self.wound(a, (blow / 10) as f32);
            self.wear(a, (blow / 10) as f32);
        }
        self.hit(a, (blow / 10) as f32 / 15.0, by_t, data, caches);
        if sure_t {
            self.hit(t, foe as f32 / 15.0, by_a, data, caches);
        } else {
            self.rout(t, foe, foe * 7, by_a, data, caches);
        }
        let (Some(i), Some(ti)) = (self.object_index(a), self.object_index(t)) else {
            return;
        };
        // Half the boarding squads (at least one) stay on the prize,
        // strongest first, the last of each kind going.
        let class = self.objects[ti].ship.as_ref().unwrap().crew.class;
        let mut stay = self.objects[i]
            .ship
            .as_ref()
            .unwrap()
            .crew
            .count()
            .div_ceil(2);
        let mut prize: [Vec<u8>; 3] = Default::default();
        for kind in (0..3u8).rev() {
            while stay > 0 {
                let n = self.objects[i].unit_conditions()[kind as usize].len();
                let Some(c) = n
                    .checked_sub(1)
                    .and_then(|last| self.objects[i].take_unit(kind, last))
                else {
                    break;
                };
                prize[kind as usize].insert(0, c);
                stay -= 1;
            }
        }
        let owner = self.objects[i].owner;
        self.capture(t, owner, class, prize, data, caches);
    }

    /// A side routed in a boarding: its marines fall (but a boat's or
    /// merchant's, carried as cargo), its weapons take `blow` nine times
    /// and its hull `hull` (original units).
    fn rout(
        &mut self,
        id: u32,
        blow: u32,
        hull: u32,
        by: Option<Owner>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        if let Some(k) = self.object_index(id) {
            let s = self.objects[k].ship.as_mut().unwrap();
            if s.kind > 1 {
                s.crew.squads = [0; 3];
                s.conditions[..3].iter_mut().for_each(Vec::clear);
            }
        }
        for _ in 0..9 {
            self.wear(id, blow as f32);
            self.wound(id, blow as f32);
        }
        self.hit(id, hull as f32 / 15.0, by, data, caches);
    }

    /// Gives ship `id` to `owner`, redrawing it in the new owner's colours.
    /// Its crew becomes `marines`: the conditions of the units of each
    /// marine kind, of crew class `class`.
    pub(super) fn capture(
        &mut self,
        id: u32,
        owner: Option<(u32, u32)>,
        class: u8,
        marines: [Vec<u8>; 3],
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let Some((nation, player)) = owner else {
            return;
        };
        // Boarded is counted as the fight is won (0x4a91a0 at 0x4b6559),
        // taken or not.
        if Some((nation, player)) == self.human.map(|h| (h.nation, h.slot)) {
            if let Some(s) = self.objects[i].ship.as_ref() {
                self.stats.ships_boarded[s.kind.min(11) as usize] += 1;
            }
        }
        // Player 16's ships are not taken, nor do they take: the prize
        // sinks, put down to its taker (0x4b6f20 at 0x4b7169: `+0x251 =
        // 4`, `+0x7f` the taker's `+0x24f`; its sinking, 0x4dec40, reports
        // it), no 0x5d97.
        const PLAYER_16: (u32, u32) = (4, 0);
        if owner == Some(PLAYER_16) || self.objects[i].owner == Some(PLAYER_16) {
            let hull = self.objects[i].combat.as_ref().map_or(0.0, |c| c.hull);
            self.strike(id, hull.max(0.0) + 1.0, false, owner, true, data, caches);
            return;
        }
        // The prize is reported destroyed by its taker before it changes
        // hands (0x4b6f20: 0x50e750 with the boarder's `+0x24f`, then
        // 0x4b9700): the taker's ship destroyed, the loser's ship lost.
        self.note_destroyed(i, owner);
        let mut rec = self.objects[i].record.clone();
        if rec.fields.len() < 7 {
            rec.fields.resize(7, 0);
        }
        rec.fields[5] = nation;
        rec.fields[6] = player;
        let obj = &mut self.objects[i];
        obj.owner = owner;
        if let Some(ship) = obj.ship.as_mut() {
            ship.crew = combat::Crew {
                class,
                squads: std::array::from_fn(|k| marines[k].len() as u32),
            };
            for (k, m) in marines.into_iter().enumerate() {
                ship.conditions[k] = m;
            }
            ship.task = None;
            ship.path.clear();
            // Its new colours drawn with its damage.
            ship.look = None;
        }
        match objects::resolve(data, caches, &rec) {
            Ok(Some(new)) => {
                obj.frames = new.frames;
                obj.label = new.label;
                obj.picture = new.picture;
            }
            Ok(None) => {}
            Err(e) => eprintln!("could not redraw captured ship: {e:#}"),
        }
        obj.record = rec;
        self.brain_prize(id, (nation, player));
    }

    /// A prize taken by a campaign brain's player (event 0x5d97, sent by
    /// the capture 0x4b6f20). In miss002-miss006 it joins the player's
    /// repair member (0x479300 op 2: the handlers' tables), whose repair
    /// order sends it to its yard's dock (message 300, `run_repair_orders`):
    /// player 15's in miss002 (1, 8), miss003 (0, 4), miss004 (0, 6),
    /// miss005 and miss006 (1, 5); player 14's in miss004 (2, 4), miss006
    /// (2, 5). Elsewhere (not ported: miss007-miss013's tables) it sails to
    /// one of the player's docks, picked at random.
    fn brain_prize(&mut self, id: u32, owner: (u32, u32)) {
        if self.brain.is_none() || self.human.map(|h| (h.nation, h.slot)) == Some(owner) {
            return;
        }
        let mission = self.brain.as_ref().unwrap().mission.clone();
        let repair = match (mission.as_str(), owner.0 * 4 + owner.1) {
            ("miss002", 15) => Some((1, 8)),
            ("miss003", 15) => Some((0, 4)),
            ("miss004", 15) => Some((0, 6)),
            ("miss004", 14) => Some((2, 4)),
            ("miss005" | "miss006", 15) => Some((1, 5)),
            ("miss006", 14) => Some((2, 5)),
            ("miss002" | "miss003" | "miss004" | "miss005" | "miss006", _) => return,
            _ => None,
        };
        if let Some((group, member)) = repair {
            self.brain_join(id, group, member);
            return;
        }
        let docks: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.owner == Some(owner) && o.class == ObjectClass::Building(economy::DOCK))
            .map(|o| o.id)
            .collect();
        if docks.is_empty() {
            return;
        }
        let dock = docks[self.rng.next() as usize % docks.len()];
        let _ = self.order_repair(id, dock);
    }

    pub(super) fn spawn_effect(
        &mut self,
        data: &GameData,
        caches: &mut Caches,
        name: &str,
        x: f32,
        y: f32,
    ) {
        match objects::sequence_frames(data, caches, name) {
            Ok(frames) if !frames.is_empty() => self.effects.push(Effect {
                x,
                y,
                frames,
                age_ms: 0.0,
                follow: None,
                life_ms: None,
                // Shots' and lightning's pictures step a frame a tick
                // (bomb.cpp, lightn.cpp).
                frame_ms: if name.starts_with("BOMB") || name.starts_with("LTSPR") {
                    crate::clock::TICK_MS as f32
                } else {
                    crate::combat::FX_FRAME_MS
                },
                tornado: None,
                site: None,
            }),
            Ok(_) => {}
            Err(e) => eprintln!("could not load effect {name}: {e:#}"),
        }
    }

    /// Loops the animation `name` over object `id` for `seconds`.
    pub(super) fn spawn_aura(
        &mut self,
        data: &GameData,
        caches: &mut Caches,
        name: &str,
        id: u32,
        seconds: f32,
    ) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        // A new aura of the same kind replaces the old one.
        self.effects
            .retain(|fx| fx.follow != Some(id) || fx.life_ms.is_none());
        match objects::sequence_frames(data, caches, name) {
            Ok(frames) if !frames.is_empty() => self.effects.push(Effect {
                x,
                y,
                frames,
                age_ms: 0.0,
                follow: Some(id),
                life_ms: Some(seconds * 1000.0),
                frame_ms: crate::combat::FX_FRAME_MS,
                tornado: None,
                site: None,
            }),
            Ok(_) => {}
            Err(e) => eprintln!("could not load effect {name}: {e:#}"),
        }
    }

    /// Protections running out, and effects following their objects.
    pub(super) fn run_statuses(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let wrecked: Vec<u32> = Vec::new();
        for o in &mut self.objects {
            let Some(c) = o.combat.as_mut() else { continue };
            c.protect_weapons = (c.protect_weapons - dt).max(0.0);
            c.protect_magic = (c.protect_magic - dt).max(0.0);
            c.phantom = (c.phantom - dt).max(0.0);
        }
        for id in wrecked {
            self.hit(id, 0.0, None, data, caches);
        }
        let positions: std::collections::HashMap<u32, (f32, f32)> =
            self.objects.iter().map(|o| (o.id, (o.x, o.y))).collect();
        self.effects.retain_mut(|fx| match fx.follow {
            Some(id) => match positions.get(&id) {
                Some(&(x, y)) => {
                    (fx.x, fx.y) = (x, y);
                    true
                }
                None => false,
            },
            None => true,
        });
    }
}

/// The item of a blow that is not a weapon's (a spell's, a tornado's):
/// its marines' share is the weapons' (B over A as 1).
const NOT_A_WEAPON: u8 = 254;

/// Ticks a building's collapse lasts (cycle table entry 2).
const COLLAPSE_TICKS: u32 = 75;
