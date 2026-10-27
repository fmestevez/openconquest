//! Fighting: gunfire, ramming, boarding, and the effects drawn for them.

use super::*;

impl Scene {
    /// Whether two objects belong to different players. All players are
    /// treated as enemies of each other until diplomacy is implemented.
    pub fn are_enemies(a: &MapObject, b: &MapObject) -> bool {
        matches!((a.owner, b.owner), (Some(x), Some(y)) if x != y)
    }

    /// Orders armed ship `ship` to attack the enemy ship or building `target`.
    pub fn order_attack(&mut self, ship: u32, target: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        let t = self.object_index(target).ok_or("No such target")?;
        if self.objects[i].ship.is_none() || !self.objects[i].is_armed() {
            return Err("This ship has no weapons".into());
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
            let levels = self.levels_of(self.objects[i].owner);
            let c = self.objects[i].combat.as_mut().unwrap();
            if c.stone > 0.0 {
                continue;
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
                // Forts are always on guard.
                None => (None, true),
            };
            let me = &self.objects[i];
            let target = match ordered {
                Some(t) => match self.object_index(t) {
                    Some(ti) => Some(ti),
                    None => {
                        self.objects[i].ship.as_mut().unwrap().task = None;
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
                                round: 0.0,
                                won: None,
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
                let stale = ship.path.back().is_none_or(|&(px, py)| {
                    ((px - tx).powi(2) + (py - ty).powi(2)).sqrt() > range / 2.0
                });
                if ordered.is_some() && stale {
                    if let Some(path) = self.plan_route(i, tx, ty) {
                        self.objects[i].ship.as_mut().unwrap().path = path;
                    }
                }
                continue;
            }
            let target_id = self.objects[ti].id;
            if ordered.is_some() {
                self.objects[i].ship.as_mut().unwrap().path.clear();
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
            // A brain's fleet notes who fired on it.
            if owner == self.human.map(|h| (h.nation, h.slot)) {
                if let Some(s) = self.object_index(shooter) {
                    let (x, y) = (self.objects[s].x, self.objects[s].y);
                    self.fleet_attacked(target, x, y);
                }
            }
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
            // shield 80%, the Flame adds, Theseus, the Armour of Ares, the
            // armour and the hull's share.
            let mut factor = 1.0;
            let mut wound_factor = 1.0;
            let mut lime_factor = 1.0;
            if warded {
                factor *= magic::SHIELD_FACTOR;
                wound_factor *= magic::SHIELD_FACTOR;
                lime_factor *= magic::SHIELD_FACTOR;
            }
            if theseus {
                wound_factor *= objects::THESEUS_DAMAGE;
                lime_factor *= objects::THESEUS_DAMAGE;
                factor *= objects::THESEUS_DAMAGE;
            }
            let flaming = self.flame_of(owner) > 0.0;
            if flaming {
                factor *= magic::FLAME_FACTOR;
            }
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
                    factor *= combat::armour_factor(ship.armour) * ship_hull_share(ship, c);
                    // The crew's share of the blow, in the original's units
                    // (units of 100 condition points).
                    let loads = (ship.cargo / 100.0).ceil().max(0.0) as u32;
                    wound_factor *= combat::armour_factor(ship.armour)
                        * combat::crew_share(
                            ship.kind,
                            ship.armour,
                            ship.ram,
                            c.weapons.len(),
                            loads,
                            ship.crew.count(),
                        )
                        * 15.0;
                } else {
                    wound_factor = 0.0;
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
                let (factor, wound_factor) = (factor * weaken, wound_factor * weaken);
                self.fire_shot(
                    super::shot::Firing {
                        item: w,
                        letter,
                        flaming,
                        from,
                        target,
                        strikes,
                        damage: combat::hull_damage_at(w, level) * factor,
                        // A blow's crew share; quick lime, which spares
                        // hulls, burns crews by its own figure.
                        wounds: if combat::hull_damage_at(w, level) > 0.0 {
                            combat::hull_damage_at(w, level) * wound_factor
                        } else {
                            combat::crew_damage_at(w, level) * lime_factor
                        },
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

    /// Wounds the crew of ship `target`: every `SQUAD_LIFE` of wounds costs
    /// it a squad, taken at random.
    pub(super) fn wound(&mut self, target: u32, wounds: f32) {
        let Some(t) = self.object_index(target) else {
            return;
        };
        let o = &mut self.objects[t];
        let (Some(ship), Some(c)) = (o.ship.as_mut(), o.combat.as_mut()) else {
            return;
        };
        if ship.crew.count() == 0 {
            c.wounds = 0.0;
            return;
        }
        c.wounds += wounds;
        while c.wounds >= combat::SQUAD_LIFE && ship.crew.count() > 0 {
            c.wounds -= combat::SQUAD_LIFE;
            let n = self.rng.next() % ship.crew.count();
            ship.crew.lose(n);
        }
    }

    /// Deals `damage` to the hull of `target`, with a hit (or splash) effect;
    /// a sunk ship is removed and a destroyed building left in ruins.
    pub(super) fn hit(&mut self, target: u32, damage: f32, data: &GameData, caches: &mut Caches) {
        self.strike(target, damage, true, data, caches);
    }

    /// The same, showing the hit or splash only if `show` (a shot shows
    /// its own).
    pub(super) fn strike(
        &mut self,
        target: u32,
        damage: f32,
        show: bool,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(ti) = self.object_index(target) else {
            return;
        };
        let (tx, ty) = (self.objects[ti].x, self.objects[ti].y);
        let jitter = |rng: &mut Rng| (rng.next() % 33) as f32 - 16.0;
        let (fx, fy) = (tx + jitter(&mut self.rng), ty + jitter(&mut self.rng));
        let Some(c) = self.objects[ti].combat.as_mut() else {
            return;
        };
        c.hull -= damage;
        let destroyed = c.hull <= 0.0;
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
            self.note_destroyed(ti);
            self.drop_held(target, data, caches);
            self.sounds
                .push((if is_building { "razru2" } else { "wreck1" }, tx, ty));
            // A ship gone: a lament if it was the player's, a fanfare
            // ("deny", event 319) if not (0x4dec40).
            if !is_building {
                let own = self.is_own(&self.objects[ti]);
                self.sounds
                    .push((if own { "deadsh" } else { "deny" }, tx, ty));
            }
            self.productions.retain(|p| p.site != target);
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
                // A sea monster dies in a pool of its blood.
                None if self.objects[ti].record.class_id == 0x1fb => {
                    let kind = self.objects[ti]
                        .record
                        .fields
                        .get(4)
                        .copied()
                        .unwrap_or(0)
                        .min(3);
                    self.spawn_effect(data, caches, &format!("UMON{kind}40"), tx, ty);
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
                round: 0.0,
                won: None,
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

    /// Ships ordered to ram or board close in on their target and, on
    /// contact, ram it or fight its crew.
    pub(super) fn run_contact(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        let closing: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| {
                matches!(
                    o.ship.as_ref().and_then(|s| s.task),
                    Some(Task::Ram { .. } | Task::Board { .. })
                )
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
                Task::Ram { .. } => {
                    self.ram(i, ti, data, caches);
                }
                Task::Board { round, won, .. } => {
                    // Grappled: the target stops too.
                    self.objects[ti].ship.as_mut().unwrap().path.clear();
                    match won {
                        // The fight is decided at once and shown; its
                        // outcome stands when the picture ends.
                        None => {
                            let won = self.board_roll(i, ti);
                            let round = self.show_boarding(i, ti, won, data, caches);
                            self.objects[i].ship.as_mut().unwrap().task = Some(Task::Board {
                                target,
                                round,
                                won: Some(won),
                            });
                        }
                        Some(won) if round > dt => {
                            self.objects[i].ship.as_mut().unwrap().task = Some(Task::Board {
                                target,
                                round: round - dt,
                                won: Some(won),
                            });
                        }
                        Some(won) => self.board_end(id, target, won, data, caches),
                    }
                }
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
        // The feathers of Boreas add their amount in percent to the
        // rammer's blow (0x4c1c60).
        let feather = 1.0 + sa.feather_percent() as f32 / 100.0;
        // The rammer charges at no less than its cruising speed here (ours
        // may slow as it closes).
        let (to_target, to_self) = combat::ram_blows(
            combat::Rammer {
                kind: sa.kind,
                ram: sa.ram,
                heading: sa.heading,
                speed: sa.velocity.max(sa.speed),
            },
            combat::Rammer {
                kind: st.kind,
                ram: st.ram,
                heading: st.heading,
                speed: st.velocity,
            },
            angle,
        );
        let share = |o: &MapObject| match (&o.ship, &o.combat) {
            (Some(s), Some(c)) => ship_hull_share(s, c),
            _ => 1.0,
        };
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
            * combat::armour_factor(st.armour)
            * share(t)
            * combat::RAM_HULL_FACTOR
            * argus(t);
        let to_self = to_self * share(a) * combat::RAM_HULL_FACTOR * argus(a);
        let (ida, idt) = (a.id, t.id);
        self.objects[i].ship.as_mut().unwrap().task = None;
        // A fire rammer's barrel of powder goes off in the ram.
        let c = self.objects[i].combat.as_mut().unwrap();
        let barrel = c.weapons.iter().position(|&w| w == combat::POWDER);
        let (blast, burn) = match barrel {
            Some(k) => {
                c.weapons.remove(k);
                (
                    combat::hull_damage(combat::POWDER),
                    combat::crew_damage_at(combat::POWDER, 0),
                )
            }
            None => (0.0, 0.0),
        };
        self.wound(idt, burn);
        self.hit(idt, to_target + blast, data, caches);
        self.hit(ida, to_self, data, caches);
    }

    /// A side's boarding strength in the original's units (0x4b4e80,
    /// 0x4bee60): each squad's C x 15 (0x597d2c, `melee`), the ship of the
    /// dead's crews doubled.
    fn boarding_strength(&self, i: usize) -> u32 {
        let o = &self.objects[i];
        let Some(ship) = o.ship.as_ref() else {
            return 0;
        };
        let undead = if ship.kind == 11 {
            combat::UNDEAD_MELEE
        } else {
            1
        };
        (0..ship.crew.count())
            .map(|n| combat::melee(ship.crew.kind_of(n)) * 15 * undead)
            .sum()
    }

    /// Decides the fight for ship `ti` boarded from `i` (0x4b5b00): with
    /// strengths a and d (each + 1), the boarders win with chance
    /// (2a - d) / (a + d); Hercules and the ship of the dead always win
    /// unless the defenders have the same; magic wood keeps boarders off.
    fn board_roll(&mut self, i: usize, ti: usize) -> bool {
        let sure = |o: &MapObject| {
            o.ship
                .as_ref()
                .is_some_and(|s| s.has_hero(objects::HERCULES) || s.kind == 11)
        };
        let (attacker, defender) = (&self.objects[i], &self.objects[ti]);
        if defender
            .ship
            .as_ref()
            .is_some_and(|s| s.has_treasure(objects::MAGIC_WOOD))
        {
            return false;
        }
        if sure(attacker) {
            return !sure(defender);
        }
        let a = self.boarding_strength(i) + 1;
        let d = self.boarding_strength(ti) + 1;
        let roll = self.rng.next() % (a + d);
        let line = 2 * a as i64 - d as i64;
        line >= (a + d) as i64 || (roll as i64) < line
    }

    /// The end of the fight for ship `t` boarded from `a` (0x4b5df0 state
    /// 3): won, the boarders lose as much as the defenders' strength and a
    /// hundredth of the prize's hull, and half of them man the prize; lost,
    /// they all fall and their ship takes seven tenths of that.
    fn board_end(&mut self, a: u32, t: u32, won: bool, data: &GameData, caches: &mut Caches) {
        let (Some(i), Some(ti)) = (self.object_index(a), self.object_index(t)) else {
            return;
        };
        let hull_t = self.objects[ti].combat.as_ref().map_or(0.0, |c| c.hull);
        // In the original's units: hulls here are a fifteenth.
        let blow = self.boarding_strength(ti) as f32 + 1.0 + hull_t * 15.0 / 100.0;
        self.objects[i].ship.as_mut().unwrap().task = None;
        if !won {
            self.objects[i].ship.as_mut().unwrap().crew.squads = [0; 3];
            self.hit(a, blow * 0.7 / 15.0, data, caches);
            return;
        }
        // The losses fall on squads at random, up to the blow's worth.
        let mut ca = self.objects[i].ship.as_ref().unwrap().crew;
        let mut lost = 0.0;
        while ca.count() > 1 {
            let n = self.rng.next() % ca.count();
            let worth = (combat::melee(ca.kind_of(n)) * 15) as f32;
            if lost + worth > blow {
                break;
            }
            lost += worth;
            ca.lose(n);
        }
        let ct = self.objects[ti].ship.as_ref().unwrap().crew;
        // Half the boarding squads (at least one) stay on the prize,
        // strongest first.
        let mut prize = combat::Crew {
            class: ct.class,
            squads: [0; 3],
        };
        let mut stay = ca.count().div_ceil(2);
        for kind in (0..3).rev() {
            let take = stay.min(ca.squads[kind]);
            prize.squads[kind] += take;
            ca.squads[kind] -= take;
            stay -= take;
        }
        let owner = self.objects[i].owner;
        self.objects[i].ship.as_mut().unwrap().crew = ca;
        self.capture(t, owner, prize, data, caches);
    }

    /// Plays the boarding picture between ships `i` and `ti`; its length.
    fn show_boarding(
        &mut self,
        i: usize,
        ti: usize,
        gained: bool,
        data: &GameData,
        caches: &mut Caches,
    ) -> f32 {
        let (a, t) = (&self.objects[i], &self.objects[ti]);
        let heading = heading_towards(t.x - a.x, t.y - a.y);
        let (x, y) = ((a.x + t.x) / 2.0, (a.y + t.y) / 2.0);
        let name = format!("ABORD_{}{heading}", u8::from(!gained));
        // The grappling's sound (event 83, 0x4b5df0).
        self.sounds.push(("abor6a", x, y));
        match objects::sequence_frames(data, caches, &name) {
            Ok(frames) if !frames.is_empty() => {
                let round = frames.len() as f32 * crate::clock::TICK;
                self.effects.push(Effect {
                    x,
                    y,
                    frames,
                    age_ms: 0.0,
                    follow: None,
                    life_ms: None,
                    frame_ms: crate::clock::TICK_MS as f32,
                    tornado: None,
                    site: None,
                });
                round
            }
            _ => combat::BOARD_ROUND,
        }
    }

    /// Gives ship `id` to `owner`, redrawing it in the new owner's colours.
    pub(super) fn capture(
        &mut self,
        id: u32,
        owner: Option<(u32, u32)>,
        crew: combat::Crew,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let Some((nation, player)) = owner else {
            return;
        };
        if Some((nation, player)) == self.human.map(|h| (h.nation, h.slot)) {
            if let Some(s) = self.objects[i].ship.as_ref() {
                self.stats.ships_boarded[s.kind.min(11) as usize] += 1;
            }
        }
        let mut rec = self.objects[i].record.clone();
        if rec.fields.len() < 7 {
            rec.fields.resize(7, 0);
        }
        rec.fields[5] = nation;
        rec.fields[6] = player;
        let obj = &mut self.objects[i];
        obj.owner = owner;
        if let Some(ship) = obj.ship.as_mut() {
            ship.crew = crew;
            ship.task = None;
            ship.path.clear();
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
            self.hit(id, 0.0, data, caches);
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

/// The share of a blow ship `ship` takes on its hull.
pub(super) fn ship_hull_share(ship: &objects::Ship, c: &combat::Combatant) -> f32 {
    let loads = (ship.cargo / 100.0).ceil().max(0.0) as u32;
    combat::hull_share(ship.kind, ship.armour, ship.ram, c.weapons.len(), loads)
}
