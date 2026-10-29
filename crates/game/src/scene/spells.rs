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
        self.pay_spell(spell)?;
        self.protect(spell, t, data, caches);
        Ok(())
    }

    /// Puts a shield or the phantom `spell` on the ship or building at
    /// index `t` (which has combat), however it was cast.
    pub(super) fn protect(&mut self, spell: Spell, t: usize, data: &GameData, caches: &mut Caches) {
        let target = self.objects[t].id;
        let large = self.objects[t]
            .ship
            .as_ref()
            .is_none_or(|s| matches!(s.kind, 2..=5 | 9 | 11));
        let (x, y) = (self.objects[t].x, self.objects[t].y);
        let c = self.objects[t].combat.as_mut().unwrap();
        // Casting again adds to the time left.
        let left = match spell {
            Spell::ProtectWeapons => &mut c.protect_weapons,
            Spell::ProtectMagic => &mut c.protect_magic,
            _ => &mut c.phantom,
        };
        *left = left.max(0.0) + magic::PROTECTION_SECONDS;
        let left = *left;
        self.sounds.push(("prote1", x, y));
        if spell != Spell::Phantom {
            let name = magic::fx_protection(spell, large);
            self.spawn_aura(data, caches, &name, target, left);
        }
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
                self.add_sight_point(cx, cy);
                self.sounds.push(("flash", x, y));
            }
            Spell::Lightning => {
                self.pay_spell(spell)?;
                self.add_sight_point(cx, cy);
                let t = TILE_SIZE as f32;
                let water = self
                    .nav
                    .is_water((x / t).floor() as i32, (y / t).floor() as i32);
                self.spawn_effect(data, caches, magic::fx_lightning(water), x, y);
                self.sounds.push(("molniya2", x, y));
                // It strikes at frame 11 of its picture over water, 40 over
                // land, a frame a tick (0x42cfa0).
                let at = if water { 11 } else { 40 };
                let caster = self.human.map(|h| (h.nation, h.slot));
                self.lightning.push((x, y, crate::clock::ticks(at), caster));
            }
            Spell::Fireball => {
                self.pay_spell(spell)?;
                self.add_sight_point(cx, cy);
                let owner = self.human.map(|h| (h.nation, h.slot));
                let t = TILE_SIZE as f32;
                self.spawn_fireball(
                    (x / t).floor() as i32,
                    (y / t).floor() as i32,
                    owner,
                    data,
                    caches,
                );
            }
            _ => return Err("That spell is not cast on an area".into()),
        }
        Ok(())
    }

    /// Makes the player's weapons stronger for a while.
    pub fn cast_flame(&mut self) -> Result<(), String> {
        self.pay_spell(Spell::Flame)?;
        self.magic.brew_flame();
        self.sounds.push(("fireball", f32::NAN, f32::NAN));
        Ok(())
    }

    /// Lightning cast and waiting for its picture to strike.
    pub(super) fn run_lightning(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        for l in &mut self.lightning {
            l.2 -= dt;
        }
        let due: Vec<(f32, f32, Option<super::Owner>)> = self
            .lightning
            .iter()
            .filter(|l| l.2 <= 0.0)
            .map(|l| (l.0, l.1, l.3))
            .collect();
        self.lightning.retain(|l| l.2 > 0.0);
        for (x, y, caster) in due {
            self.lightning_strike(x, y, caster, data, caches);
        }
    }

    /// Lightning strikes cell `(x, y)`'s 3 x 3 (LightningObjClassTy
    /// 0x42cfa0): each cell's occupant, row by row, is asked whether it is
    /// shielded (0xda94), a ship going down at the asking, shielded or
    /// not, till a building shielded from magic answers; unless one did,
    /// the cell's ground is dug into sea (0x5030c0);
    /// then the blow (0xda93): unshielded ships go down; a fort, port,
    /// dock, workshop, temple, school, castle or nest (WorkLandObj
    /// 0x427f50, asleep or not) struck on its own cell unshielded, or whose
    /// ground is gone, falls, else loses a quarter of its condition (a
    /// twentieth shielded); settlements and towns take nothing; a lair, a
    /// barbarian structure, a harpy or dragon fall struck on their own
    /// cell; sea monsters lose half their strength, land creatures and the
    /// gorgon a quarter. The caster's brain hears of it (0x5d9d).
    fn lightning_strike(
        &mut self,
        x: f32,
        y: f32,
        caster: Option<(u32, u32)>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let t = TILE_SIZE as f32;
        let (cx, cy) = ((x / t).floor() as i32, (y / t).floor() as i32);
        let near = |p: (i32, i32)| (p.0 - cx).abs() <= 1 && (p.1 - cy).abs() <= 1;
        // The query, cell by cell, row by row: a ship there is wrecked; a
        // building shielded from magic stops it, and the dig.
        let mut wrecked = Vec::new();
        let mut absorbed = false;
        'query: for dy in -1..=1 {
            for dx in -1..=1 {
                let c = (cx + dx, cy + dy);
                for o in &self.objects {
                    let Some(cb) = o.combat.as_ref() else {
                        continue;
                    };
                    if o.ship.is_some() {
                        if ((o.x / t).floor() as i32, (o.y / t).floor() as i32) == c {
                            wrecked.push(o.id);
                        }
                    } else if cb.protect_magic > 0.0
                        && o.cell.is_some_and(|(bx, by)| {
                            let n = super::trade::building_cells(o);
                            (bx..bx + n).contains(&c.0) && (by..by + n).contains(&c.1)
                        })
                    {
                        absorbed = true;
                        break 'query;
                    }
                }
            }
        }
        if !absorbed {
            self.dig(cx, cy, data, caches);
        }
        let mut blows: Vec<(u32, f32)> = Vec::new();
        for o in &self.objects {
            let Some(c) = o.combat.as_ref() else { continue };
            let (ox, oy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
            let f = |i| o.record.field(i).unwrap_or(0) as i32;
            let home = match o.class {
                ObjectClass::Building(_) | ObjectClass::Barbarian(_) | ObjectClass::Castle(_) => {
                    (f(5), f(6))
                }
                ObjectClass::HarpyNest | ObjectClass::CreatureSpawn => (f(4), f(5)),
                _ => (ox, oy),
            };
            let centre = home == (cx, cy);
            let shielded = c.protect_magic > 0.0;
            let work = matches!(
                o.class,
                ObjectClass::Building(0x34..=0x3e)
                    | ObjectClass::Castle(_)
                    | ObjectClass::HarpyNest
            );
            let k = self.object_index(o.id).unwrap_or(0);
            let blow = match o.class {
                // The query wrecks the ship on the cell (0x4dc630, 0xda94);
                // the blow, an unshielded one.
                ObjectClass::Ship if wrecked.contains(&o.id) => c.hull,
                ObjectClass::Ship if near(home) && !shielded => c.hull,
                _ if work && near(home) => {
                    let gone = (centre && !shielded) || !self.site_holds(k);
                    if gone {
                        c.hull
                    } else {
                        c.max_hull * if shielded { 0.05 } else { 0.25 }
                    }
                }
                ObjectClass::Barbarian(_)
                | ObjectClass::CreatureSpawn
                | ObjectClass::Monster(0x1f8 | 0x1f9)
                    if centre =>
                {
                    c.hull
                }
                ObjectClass::Monster(0x1fb) if near(home) => c.max_hull / 2.0,
                ObjectClass::Monster(0x1f4..=0x1f6 | 0x1fa) if near(home) => c.max_hull / 4.0,
                _ => 0.0,
            };
            if blow > 0.0 {
                blows.push((o.id, blow));
            }
        }
        for (id, blow) in blows {
            // 0xda93 heeds no sleep: struck straight (not through the
            // fort's units either).
            self.strike(id, blow, true, data, caches);
        }
        // miss009's brain (0x4663d0): the player's lightning in the strait.
        if caster.is_some()
            && caster == self.human.map(|h| (h.nation, h.slot))
            && (55..=62).contains(&cx)
            && (3..=8).contains(&cy)
        {
            self.strait_struck = true;
        }
    }

    /// The Flame wears off; a brewed one lights.
    pub(super) fn run_fires(&mut self, dt: f32, _data: &GameData, _caches: &mut Caches) {
        self.flame = (self.flame - dt).max(0.0);
        self.flame += self.magic.take_flames() as f32 * magic::FLAME_SECONDS;
    }

    /// Calls up a strong wind towards `direction`.
    pub fn cast_wind(&mut self, direction: u32) -> Result<(), String> {
        self.pay_spell(Spell::Wind)?;
        self.wind.force(direction);
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
                life_ms: None,
                frame_ms: crate::combat::FX_FRAME_MS,
                tornado: Some(crate::combat::Tornado {
                    clock: 0.0,
                    frame: 0,
                    phase: 1,
                    moving: 0.0,
                    hop: 0,
                    from: (x, y),
                    to: (x, y),
                }),
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
            // UMonster records: the cell in fields 7 and 8, the type in
            // field 10: the caster's nation's, a barbarian's at random
            // (0x42f580 case 4, 0x493780).
            fields[7] = cx as u32;
            fields[8] = cy as u32;
            fields[10] = match master {
                Some((4, _)) | None => self.rng.next() & 3,
                Some((n, _)) => n.min(3),
            };
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
            // A summoned monster bites its master's enemies (`creatures`).
            obj.owner = master;
            // It stays while its strength lasts (below).
            self.summons.push((id, f32::INFINITY));
        } else {
            // Cast whirlpools and reefs go after 7500-15000 ticks (Whirl.cpp
            // 0x447ee0, Reef.cpp 0x4306d0); the map's own stay.
            let ticks = 7500 + self.rng.next() % 7501;
            self.summons.push((id, crate::clock::ticks(ticks)));
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
        let struck = self.run_tornadoes(dt);
        // Summoned monsters go back to the deep, cast whirlpools and reefs
        // fade.
        for s in &mut self.summons {
            s.1 -= dt;
        }
        for k in 0..self.summons.len() {
            let id = self.summons[k].0;
            let Some(i) = self.object_index(id) else {
                continue;
            };
            if self.objects[i].class != ObjectClass::Monster(0x1fb) {
                continue;
            }
            if let Some(c) = self.objects[i].combat.as_mut() {
                c.hull -= c.max_hull * 0.01 * dt / magic::MONSTER_ROUND_SECONDS;
                if c.hull <= 0.0 {
                    self.summons[k].1 = 0.0;
                }
            }
        }
        let gone: Vec<u32> = self
            .summons
            .iter()
            .filter(|s| s.1 <= 0.0)
            .map(|s| s.0)
            .collect();
        self.summons.retain(|s| s.1 > 0.0);
        self.objects.retain(|o| !gone.contains(&o.id));
        // Whirlpools and reefs wreck the ships on their cells (messages
        // 0xda95, 0xda94: Whirl.cpp 0x447ee0, Reef.cpp 0x4306d0), but for
        // Jason's; a tornado strikes the ships on each cell it comes to,
        // but for Zetes's.
        let traps: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Whirlpool | ObjectClass::Reef(_)))
            .map(|o| {
                let f = |i| o.record.field(i).unwrap_or(0) as i32;
                (f(5), f(6))
            })
            .collect();
        if traps.is_empty() && struck.is_empty() {
            return;
        }
        let t = TILE_SIZE as f32;
        let mut wrecked = Vec::new();
        let mut spouts = Vec::new();
        for o in self.objects.iter_mut().filter(|o| o.ship.is_some()) {
            let cell = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
            let s = o.ship.as_ref().unwrap();
            let (jason, zetes) = (s.has_hero(objects::JASON), s.has_hero(objects::ZETES));
            if !jason && traps.contains(&cell) {
                if let Some(c) = o.combat.as_mut() {
                    c.hull = 0.0;
                }
                wrecked.push(o.id);
                continue;
            }
            let blows = struck.iter().filter(|&&c| c == cell).count();
            if let (true, false, Some(c)) = (blows > 0, zetes, o.combat.as_ref()) {
                let blow = if c.protect_magic > 0.0 {
                    magic::WATERSPOUT_STRIKE * magic::SHIELD_FACTOR
                } else {
                    magic::WATERSPOUT_STRIKE
                };
                spouts.push((o.id, blow * blows as f32));
            }
        }
        for id in wrecked {
            self.hit(id, 0.0, data, caches);
        }
        // Shared out as any blow (hull, crew, weapons).
        for (id, blow) in spouts {
            self.share_blow(id, blow, data, caches);
        }
    }

    /// Moves the tornadoes on (0x43a380); the cells they strike.
    fn run_tornadoes(&mut self, dt: f32) -> Vec<(i32, i32)> {
        const FRAME: f32 = 2.0 * crate::clock::TICK;
        let mut struck = Vec::new();
        for k in 0..self.effects.len() {
            let Some(mut tw) = self.effects[k].tornado.clone() else {
                continue;
            };
            let last = self.effects[k].frames.len().saturating_sub(1);
            tw.clock += dt;
            while tw.clock >= FRAME && tw.phase != 0 {
                tw.clock -= FRAME;
                if tw.phase == 2 {
                    tw.moving += FRAME;
                }
                tw.frame += 1;
                let end = [0, 41, 83, 124][tw.phase as usize].min(last);
                if tw.frame > end {
                    match tw.phase {
                        1 => {
                            tw.phase = 2;
                            tw.frame = 42;
                            tw.moving = 0.0;
                            tw.hop = 0;
                            struck.push(self.tornado_hop(&mut tw));
                        }
                        2 => tw.frame = 42,
                        _ => tw.phase = 0,
                    }
                }
                if tw.phase == 2 {
                    let f = tw.hop as f32 / 16.0;
                    let fx = &mut self.effects[k];
                    fx.x = tw.from.0 + (tw.to.0 - tw.from.0) * f;
                    fx.y = tw.from.1 + (tw.to.1 - tw.from.1) * f;
                    tw.hop += 1;
                    if tw.hop > 16 {
                        if tw.moving > crate::clock::ticks(250) {
                            tw.phase = 3;
                            tw.frame = 84.min(last);
                        } else {
                            struck.push(self.tornado_hop(&mut tw));
                            tw.hop = 0;
                        }
                    }
                }
            }
            self.effects[k].tornado = Some(tw);
        }
        struck
    }

    /// A tornado at the end of a hop strikes its cell and picks the next
    /// (0x43a6c0): any water cell around, those within a heading of the
    /// wind three times in eight, the others two. (The original tests the
    /// cell it is on, not each one around.) Returns the cell struck.
    fn tornado_hop(&mut self, tw: &mut crate::combat::Tornado) -> (i32, i32) {
        const DIRS: [(i32, i32); 8] = [
            (1, 0),
            (1, -1),
            (0, -1),
            (-1, -1),
            (-1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];
        let t = TILE_SIZE as f32;
        let cell = ((tw.to.0 / t).floor() as i32, (tw.to.1 / t).floor() as i32);
        tw.from = tw.to;
        let wind = self.wind.direction as i32;
        let around: Vec<((i32, i32), u32)> = DIRS
            .iter()
            .enumerate()
            .map(|(d, &(dx, dy))| ((cell.0 + dx, cell.1 + dy), d as i32))
            .filter(|&(c, _)| self.nav.is_water(c.0, c.1))
            .map(|(c, d)| (c, if (wind - d).abs() < 2 { 3 } else { 2 }))
            .collect();
        let total: u32 = around.iter().map(|a| a.1).sum();
        if total > 0 {
            let roll = self.rng.next() % total + 1;
            let mut sum = 0;
            for (c, w) in around {
                sum += w;
                if roll <= sum {
                    tw.to = (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
                    break;
                }
            }
        }
        cell
    }
}
