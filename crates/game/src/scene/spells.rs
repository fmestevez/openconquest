//! Temple magic and the sea's hazards.

use super::*;

impl Scene {
    /// Whether the human player has a standing temple of `temple`.
    pub fn has_temple(&self, temple: Temple) -> bool {
        self.owned_buildings() & 1 << (temple.class() - SETTLEMENT) != 0
    }

    /// Starts learning the next spell of `temple`.
    pub fn learn_spell(&mut self, temple: Temple) -> Result<(), String> {
        if !self.has_temple(temple) {
            return Err("You need a temple for that".into());
        }
        if self.magic.learning(temple).is_some() {
            return Err("The temple is already studying".into());
        }
        if self.magic.all_known(temple) {
            return Err("All the temple's spells are known".into());
        }
        if self.amber < magic::LEARN_COST {
            return Err("Not enough amber".into());
        }
        self.amber -= magic::LEARN_COST;
        self.magic.start_learning(temple);
        Ok(())
    }

    /// Pays for `spell` if it is known and affordable.
    pub(super) fn pay_spell(&mut self, spell: Spell) -> Result<(), String> {
        // A spell from a treasure is cast once for nothing.
        if self.free_casts.get(&spell).is_some_and(|&n| n > 0) {
            if !self.has_temple(spell.temple()) {
                return Err("You need a temple for that".into());
            }
            *self.free_casts.get_mut(&spell).unwrap() -= 1;
            return Ok(());
        }
        if !self.has_temple(spell.temple()) || !self.magic.knows(spell) {
            return Err("The temple does not know that spell".into());
        }
        let (amber, fish) = spell.cost();
        if self.amber < amber || self.fish < fish {
            return Err("Not enough amber or fish".into());
        }
        self.amber -= amber;
        self.fish -= fish;
        Ok(())
    }

    /// Whether the world point `(x, y)` is within reach of `spell`: within
    /// its range of one of the player's standing temples of its kind.
    fn in_reach(&self, spell: Spell, x: f32, y: f32) -> Result<(), String> {
        let Some(range) = spell.range() else {
            return Ok(());
        };
        let t = TILE_SIZE as f32;
        let class = spell.temple().class();
        let near = self.objects.iter().any(|o| {
            o.class == ObjectClass::Building(class)
                && self.is_own(o)
                && self.construction_at(o.id).is_none()
                && ((o.x - x).powi(2) + (o.y - y).powi(2)).sqrt() <= range as f32 * t
        });
        if near {
            Ok(())
        } else {
            Err(format!("Too far from your temple (reach: {range} cells)"))
        }
    }

    /// Casts a shield or the phantom on the player's ship or building
    /// `target`.
    pub fn cast_on(
        &mut self,
        spell: Spell,
        target: u32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        let t = self.object_index(target).ok_or("Nothing there")?;
        let obj = &self.objects[t];
        if spell.target() != magic::Target::OwnShip {
            return Err("That spell is not cast on a ship".into());
        }
        if !self.is_own(obj) || obj.combat.is_none() {
            return Err("Choose one of your ships or buildings".into());
        }
        self.in_reach(spell, obj.x, obj.y)?;
        let large = obj
            .ship
            .as_ref()
            .is_none_or(|s| combat::hull_strength(s.kind) >= 1600.0);
        self.pay_spell(spell)?;
        let (x, y) = (self.objects[t].x, self.objects[t].y);
        let c = self.objects[t].combat.as_mut().unwrap();
        match spell {
            Spell::ProtectWeapons => c.protect_weapons = magic::PROTECTION_SECONDS,
            Spell::ProtectMagic => c.protect_magic = magic::PROTECTION_SECONDS,
            _ => c.phantom = magic::PROTECTION_SECONDS,
        }
        self.sounds.push(("prote1", x, y));
        if spell != Spell::Phantom {
            let name = magic::fx_protection(spell, large);
            self.spawn_aura(data, caches, &name, target, magic::PROTECTION_SECONDS);
        }
        Ok(())
    }

    /// Casts lightning, a fireball or the view on the map cell `(cx, cy)`.
    pub fn cast_area(
        &mut self,
        spell: Spell,
        cx: i32,
        cy: i32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        let t = TILE_SIZE as f32;
        let (x, y) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
        self.in_reach(spell, x, y)?;
        match spell {
            Spell::ViewMap => {
                self.pay_spell(spell)?;
                self.reveal(cx, cy, magic::VIEW_RADIUS);
                self.sounds.push(("flash", x, y));
            }
            Spell::Lightning => {
                self.pay_spell(spell)?;
                self.spawn_effect(data, caches, magic::FX_LIGHTNING, x, y);
                self.sounds.push(("molniya2", x, y));
                // Everything of the enemy's in the strike goes, unless
                // shielded from magic.
                let struck: Vec<u32> = self
                    .objects
                    .iter()
                    .filter(|o| {
                        o.owner.is_some()
                            && !self.is_own(o)
                            && o.combat.as_ref().is_some_and(|c| c.protect_magic <= 0.0)
                            && ((o.x - x).powi(2) + (o.y - y).powi(2)).sqrt()
                                <= magic::LIGHTNING_RADIUS
                    })
                    .map(|o| o.id)
                    .collect();
                for id in struck {
                    self.hit(id, f32::MAX, data, caches);
                }
            }
            Spell::Fireball => {
                self.pay_spell(spell)?;
                self.fires.push((x, y, magic::FIREBALL_SECONDS));
                self.sounds.push(("fireball", x, y));
                if let Ok(frames) = objects::sequence_frames(data, caches, magic::FX_FLAME) {
                    if !frames.is_empty() {
                        self.effects.push(Effect {
                            x,
                            y,
                            frames,
                            age_ms: 0.0,
                            follow: None,
                            life_ms: Some(magic::FIREBALL_SECONDS * 1000.0),
                            drift: false,
                            site: None,
                        });
                    }
                }
            }
            _ => return Err("That spell is not cast on an area".into()),
        }
        Ok(())
    }

    /// Makes the player's weapons stronger for a while.
    pub fn cast_flame(&mut self) -> Result<(), String> {
        self.pay_spell(Spell::Flame)?;
        self.flame = magic::FLAME_SECONDS;
        self.sounds.push(("fireball", f32::NAN, f32::NAN));
        Ok(())
    }

    /// Fireballs burn enemy objects around them.
    pub(super) fn run_fires(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.flame = (self.flame - dt).max(0.0);
        for f in &mut self.fires {
            f.2 -= dt;
        }
        self.fires.retain(|f| f.2 > 0.0);
        if self.fires.is_empty() {
            return;
        }
        let human = self.human.map(|h| (h.nation, h.slot));
        let mut wrecked = Vec::new();
        for o in self.objects.iter_mut() {
            if o.owner.is_none() || o.owner == human {
                continue;
            }
            // The torch of Ares protects from all kinds of fire.
            if o.ship
                .as_ref()
                .is_some_and(|s| s.has_treasure(objects::FIRE_TORCH))
            {
                continue;
            }
            let Some(c) = o.combat.as_mut() else { continue };
            let inside = self
                .fires
                .iter()
                .filter(|f| {
                    ((o.x - f.0).powi(2) + (o.y - f.1).powi(2)).sqrt() <= magic::FIREBALL_RADIUS
                })
                .count();
            if inside > 0 {
                let shield = if c.protect_magic > 0.0 {
                    magic::SHIELD_FACTOR
                } else {
                    1.0
                };
                c.hull -= magic::FIREBALL_DPS * inside as f32 * shield * dt;
                if c.hull <= 0.0 {
                    wrecked.push(o.id);
                }
            }
        }
        for id in wrecked {
            self.hit(id, 0.0, data, caches);
        }
    }

    /// Calls up a strong wind towards `direction`.
    pub fn cast_wind(&mut self, direction: u32) -> Result<(), String> {
        self.pay_spell(Spell::Wind)?;
        self.wind.force(direction, magic::WIND_SECONDS);
        self.sounds.push(("wind8", f32::NAN, f32::NAN));
        Ok(())
    }

    /// Raises a reef or summons a whirlpool at map cell `(cx, cy)`.
    pub fn cast_hazard(
        &mut self,
        spell: Spell,
        cx: i32,
        cy: i32,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        if !matches!(
            spell,
            Spell::Whirlpool | Spell::Reef | Spell::Monster | Spell::Waterspout
        ) {
            return Err("Not a spell for the sea".into());
        }
        if !self.nav.is_water(cx, cy) {
            return Err("Choose open water".into());
        }
        let t = TILE_SIZE as f32;
        self.in_reach(spell, cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0)?;
        if !self.magic.knows(spell) && self.free_casts.get(&spell).is_none_or(|&n| n == 0) {
            return Err("The temple does not know that spell".into());
        }
        self.pay_spell(spell)?;
        let master = self.human.map(|h| (h.nation, h.slot));
        self.spawn_hazard(spell, cx, cy, master, data, caches)
    }

    /// Raises reef, whirlpool, waterspout or sea monster `spell` at map
    /// cell `(cx, cy)`, the monster fighting for `master`.
    pub(super) fn spawn_hazard(
        &mut self,
        spell: Spell,
        cx: i32,
        cy: i32,
        master: Option<(u32, u32)>,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<(), String> {
        let class = match spell {
            Spell::Whirlpool => 0x258,
            Spell::Reef => 0x25a,
            Spell::Monster => 0x1fb,
            Spell::Waterspout => 0,
            _ => return Err("Not a spell for the sea".into()),
        };
        let t = TILE_SIZE as f32;
        if spell == Spell::Waterspout {
            let frames = objects::sequence_frames(data, caches, magic::FX_TORNADO)
                .map_err(|e| format!("The spell failed: {e:#}"))?;
            let (x, y) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
            self.effects.push(Effect {
                x,
                y,
                frames,
                age_ms: 0.0,
                follow: None,
                life_ms: Some(magic::WATERSPOUT_SECONDS * 1000.0),
                drift: true,
                site: None,
            });
            self.sounds.push(("wind8", x, y));
            return Ok(());
        }
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 12];
        fields[0] = class;
        fields[1] = 1;
        fields[5] = cx as u32;
        fields[6] = cy as u32;
        fields[7] = 1;
        if spell == Spell::Monster {
            // UMonster records: type (the nation's monster) in field 4, the
            // cell in fields 7 and 8.
            fields[4] = master.map_or(0, |m| m.0.min(3));
            fields[7] = cx as u32;
            fields[8] = cy as u32;
        }
        let rec = ObjectRecord {
            id,
            class_id: class,
            fields,
        };
        let mut obj = objects::resolve(data, caches, &rec)
            .map_err(|e| format!("The spell failed: {e:#}"))?
            .ok_or("The spell failed")?;
        if spell == Spell::Monster {
            // A summoned monster fights for its master for a while.
            obj.owner = master;
            if let Some(c) = obj.combat.as_mut() {
                c.weapons = vec![magic::MONSTER_WEAPON; magic::MONSTER_WEAPONS];
            }
            self.summons.push((id, magic::MONSTER_SECONDS));
        }
        self.sounds.push((
            if spell == Spell::Whirlpool {
                "whirl3"
            } else {
                "splash1"
            },
            obj.x,
            obj.y,
        ));
        self.objects.push(obj);
        self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        Ok(())
    }

    /// Ships caught in whirlpools or on reefs take damage.
    pub(super) fn run_hazards(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        // Tornadoes drift with the wind, wandering a little.
        let (dir, strength) = (self.wind.direction as f32, self.wind.strength.max(3) as f32);
        for fx in self.effects.iter_mut().filter(|fx| fx.drift) {
            let wobble = ((fx.age_ms / 700.0).sin()) * 0.8;
            let angle = dir * std::f32::consts::FRAC_PI_4 + wobble;
            let speed = magic::WATERSPOUT_SPEED * strength / 6.0;
            fx.x += angle.cos() * speed * dt;
            fx.y -= angle.sin() * speed * dt;
        }
        // Summoned monsters go back to the deep.
        for s in &mut self.summons {
            s.1 -= dt;
        }
        let gone: Vec<u32> = self
            .summons
            .iter()
            .filter(|s| s.1 <= 0.0)
            .map(|s| s.0)
            .collect();
        self.summons.retain(|s| s.1 > 0.0);
        self.objects.retain(|o| !gone.contains(&o.id));
        // (x, y, damage per second, whether it is magic the shield blunts)
        let hazards: Vec<(f32, f32, f32, bool)> = self
            .objects
            .iter()
            .filter_map(|o| match o.class {
                ObjectClass::Whirlpool => Some((o.x, o.y, magic::WHIRLPOOL_DPS, false)),
                ObjectClass::Reef(_) => Some((o.x, o.y, magic::REEF_DPS, false)),
                _ => None,
            })
            .chain(
                self.effects
                    .iter()
                    .filter(|fx| fx.drift && fx.frame().is_some())
                    .map(|fx| (fx.x, fx.y, magic::WATERSPOUT_DPS, true)),
            )
            .collect();
        if hazards.is_empty() {
            return;
        }
        let mut wrecked = Vec::new();
        for o in self.objects.iter_mut().filter(|o| o.ship.is_some()) {
            let shielded = o.combat.as_ref().is_some_and(|c| c.protect_magic > 0.0);
            // Jason's ship is not harmed by whirlpools and reefs, nor
            // Zetes's by waterspouts.
            let s = o.ship.as_ref().unwrap();
            let (jason, zetes) = (s.has_hero(objects::JASON), s.has_hero(objects::ZETES));
            let dps: f32 = hazards
                .iter()
                .filter(|(x, y, _, _)| {
                    ((o.x - x).powi(2) + (o.y - y).powi(2)).sqrt() <= magic::HAZARD_RADIUS
                })
                .filter(|h| if h.3 { !zetes } else { !jason })
                .map(|h| {
                    if h.3 && shielded {
                        h.2 * magic::SHIELD_FACTOR
                    } else {
                        h.2
                    }
                })
                .sum();
            if let (true, Some(c)) = (dps > 0.0, o.combat.as_mut()) {
                c.hull -= dps * dt;
                if c.hull <= 0.0 {
                    wrecked.push(o.id);
                }
            }
        }
        for id in wrecked {
            self.hit(id, 0.0, data, caches);
        }
    }
}
