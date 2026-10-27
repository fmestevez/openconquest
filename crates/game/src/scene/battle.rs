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
            let reach_bonus = if theseus { objects::THESEUS_RANGE } else { 0.0 };
            // The longest of its weapons' ranges.
            let range = reach_bonus
                + shooters
                    .iter()
                    .map(|&w| combat::reach(w, levels.reach_for(w)).1)
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
                            Task::Board { target, round: 0.0 }
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
                let (near, far) = combat::reach(w, levels.reach_for(w));
                let far = far + reach_bonus;
                if c.reloads[k] <= 0.0 && d >= near && d <= far {
                    let rate = if atalanta {
                        objects::ATALANTA_RATE
                    } else {
                        1.0
                    };
                    c.reloads[k] = combat::reload_seconds(w, levels.rate_for(w)) / rate;
                    fired.push(w);
                }
            }
            if !fired.is_empty() {
                volleys.push((
                    target_id,
                    fired,
                    self.objects[i].owner,
                    id,
                    theseus,
                    atalanta,
                ));
            }
        }
        for (target, weapons, owner, shooter, theseus, atalanta) in volleys {
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
            // shield 80%, the Flame adds, Theseus, the Armour of Ares and
            // Argus, the armour and the hull's share.
            let mut factor = 1.0;
            let mut wound_factor = 1.0;
            if warded {
                factor *= magic::SHIELD_FACTOR;
                wound_factor *= magic::SHIELD_FACTOR;
            }
            if theseus {
                wound_factor *= objects::THESEUS_DAMAGE;
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
                    if ship.has_hero(objects::ARGUS) && owner == Some(combat::MONSTERS) {
                        factor *= objects::ARGUS_MONSTER_DAMAGE;
                    }
                }
                if let (Some(ship), Some(c)) = (&self.objects[t].ship, &self.objects[t].combat) {
                    factor *= combat::armour_factor(ship.armour) * ship_hull_share(ship, c);
                }
            }
            // Each weapon sends a shot off, carrying its blow (bomb.cpp).
            let Some(s) = self.object_index(shooter) else {
                continue;
            };
            let from = (self.objects[s].x, self.objects[s].y);
            for w in weapons {
                let aim = combat::HIT_CHANCE + if atalanta { objects::ATALANTA_HIT } else { 0 };
                let strikes = self.rng.chance(aim);
                let level = levels.arms_for(w);
                self.fire_shot(
                    super::shot::Firing {
                        item: w,
                        flaming,
                        from,
                        target,
                        strikes,
                        damage: combat::hull_damage_at(w, level) * factor,
                        wounds: combat::crew_damage_at(w, level) * wound_factor,
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
            Task::Board { target, round: 0.0 }
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
            let d = ((self.objects[i].x - tx).powi(2) + (self.objects[i].y - ty).powi(2)).sqrt();
            if d > combat::CONTACT {
                let ship = self.objects[i].ship.as_ref().unwrap();
                let stale = ship.path.back().is_none_or(|&(px, py)| {
                    ((px - tx).powi(2) + (py - ty).powi(2)).sqrt() > combat::CONTACT
                });
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
                Task::Board { round, .. } => {
                    // Grappled: the target stops too.
                    self.objects[ti].ship.as_mut().unwrap().path.clear();
                    let round = round - dt;
                    if round > 0.0 {
                        self.objects[i].ship.as_mut().unwrap().task =
                            Some(Task::Board { target, round });
                    } else {
                        let round = self.board_round(id, target, data, caches);
                        if let Some(ship) = self.objects.get_mut(i).and_then(|o| o.ship.as_mut()) {
                            if matches!(ship.task, Some(Task::Board { .. })) {
                                ship.task = Some(Task::Board { target, round });
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Ship `i` rams ship `ti`; both take damage by the odds of the ram.
    pub(super) fn ram(&mut self, i: usize, ti: usize, data: &GameData, caches: &mut Caches) {
        let (a, t) = (&self.objects[i], &self.objects[ti]);
        let (sa, st) = (a.ship.as_ref().unwrap(), t.ship.as_ref().unwrap());
        let angle = combat::ram_angle(sa.heading, st.heading);
        // The feather of Boreas strengthens the ram.
        let ram = (sa.ram + sa.has_treasure(objects::FEATHER) as u8).min(2);
        // The ram quality table (0x598054) is read by the ships' ram levels.
        let with_ram = |crew: combat::Crew, class: u8| combat::Crew { class, ..crew };
        let odds = combat::ram_odds(
            (sa.kind, with_ram(sa.crew, ram)),
            (st.kind, with_ram(st.crew, st.ram)),
            angle,
        ) as f32
            * combat::armour_factor(st.armour);
        let max = |o: &MapObject| o.combat.as_ref().map_or(0.0, |c| c.max_hull);
        // Placeholder scale: a ram at the best odds takes half the target's hull.
        let to_target = max(t) * odds / 400.0;
        let to_self = max(a) * 0.1 * (100.0 / odds.max(1.0)).min(1.0);
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
        let to_target = to_target + blast;
        self.hit(idt, to_target, data, caches);
        self.hit(ida, to_self, data, caches);
    }

    /// One exchange of blows between the boarders of ship `a` and the crew
    /// of ship `t`; when the defenders fall, the ship changes hands.
    /// Returns the seconds until the next: the boarding picture's length.
    pub(super) fn board_round(
        &mut self,
        a: u32,
        t: u32,
        data: &GameData,
        caches: &mut Caches,
    ) -> f32 {
        let (Some(i), Some(ti)) = (self.object_index(a), self.object_index(t)) else {
            return combat::BOARD_ROUND;
        };
        let mut ca = self.objects[i].ship.as_ref().unwrap().crew;
        let mut ct = self.objects[ti].ship.as_ref().unwrap().crew;
        // Each squad strikes once a round at a random enemy squad, winning
        // by the odds of their boarding strengths.
        let mut strikes = Vec::new();
        for (side, (from, to)) in [(&ca, &ct), (&ct, &ca)].into_iter().enumerate() {
            for n in 0..from.count() {
                if to.count() == 0 {
                    break;
                }
                let target = self.rng.next() % to.count();
                // The ship of the dead's crews are undead and stronger.
                let undead = |side: usize| {
                    let o = &self.objects[if side == 0 { i } else { ti }];
                    if o.ship.as_ref().is_some_and(|s| s.kind == 11) {
                        combat::UNDEAD_MELEE
                    } else {
                        1
                    }
                };
                let (mine, theirs) = (
                    combat::melee(from.kind_of(n)) * undead(side),
                    combat::melee(to.kind_of(target)) * undead(1 - side),
                );
                if self.rng.chance(50 * mine / (mine + theirs)) {
                    strikes.push((side, target));
                }
            }
        }
        // Hercules cannot be beaten aboard, and magic wood keeps boarders
        // off: such a side loses no one.
        let unbeatable = |o: &MapObject, defending: bool| {
            o.ship.as_ref().is_some_and(|s| {
                s.has_hero(objects::HERCULES) || (defending && s.has_treasure(objects::MAGIC_WOOD))
            })
        };
        let (safe_attackers, safe_defenders) = (
            unbeatable(&self.objects[i], false),
            unbeatable(&self.objects[ti], true),
        );
        strikes.retain(|&(side, _)| {
            if side == 0 {
                !safe_defenders
            } else {
                !safe_attackers
            }
        });
        // Losses fall after the exchange, highest squad numbers first.
        strikes.sort_by_key(|&(side, n)| std::cmp::Reverse((side, n)));
        strikes.dedup();
        for (side, n) in strikes {
            if side == 0 {
                ct.lose(n)
            } else {
                ca.lose(n)
            }
        }
        let gained = ct.count() < self.objects[ti].ship.as_ref().unwrap().crew.count();
        self.objects[i].ship.as_mut().unwrap().crew = ca;
        self.objects[ti].ship.as_mut().unwrap().crew = ct;
        // The fight aboard (0x4b5df0, 0x4e1bb0): the boarders gaining,
        // `ABORD_0<heading>`, or thrown back, `ABORD_1<heading>`, a frame
        // a tick between the ships; the next exchange waits for it.
        let round = self.show_boarding(i, ti, gained, data, caches);
        if ca.count() == 0 {
            self.objects[i].ship.as_mut().unwrap().task = None;
            return round;
        }
        if ct.count() > 0 {
            return round;
        }
        // Captured: half the boarding squads (at least one) stay on the
        // prize, strongest first.
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
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.crew = ca;
        ship.task = None;
        self.capture(t, owner, prize, data, caches);
        round
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
                // Shots' pictures step a frame a tick (bomb.cpp).
                frame_ms: if name.starts_with("BOMB") {
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
