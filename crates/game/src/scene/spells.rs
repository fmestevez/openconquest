//! Temple magic and the sea's hazards.

use super::*;

impl Scene {
    /// Whether the human player has a standing temple of `temple`.
    pub fn has_temple(&self, temple: Temple) -> bool {
        self.owned_buildings() & 1 << (temple.class() - SETTLEMENT) != 0
    }

    /// The temple building a temple's work goes on at: `site`, the one
    /// whose command it was, else the player's first standing temple of
    /// the kind.
    fn temple_site(&self, temple: Temple, site: Option<u32>) -> Option<u32> {
        site.or_else(|| {
            self.objects
                .iter()
                .find(|o| {
                    o.class == ObjectClass::Building(temple.class())
                        && self.is_own(o)
                        && self.construction_at(o.id).is_none()
                })
                .map(|o| o.id)
        })
    }

    /// Starts learning the next spell of `temple`, at the temple `site`.
    pub fn learn_spell(&mut self, temple: Temple, site: Option<u32>) -> Result<(), String> {
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
        let site = self.temple_site(temple, site);
        self.magic.start_learning(temple, site);
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
    /// It brews at the Temple of War `site`.
    pub fn cast_flame(&mut self, site: Option<u32>) -> Result<(), String> {
        self.pay_spell(Spell::Flame)?;
        let site = self.temple_site(Temple::War, site);
        self.magic.brew_flame(site);
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
    /// 0x427f50, asleep or not; barbarian structures too) struck on its own
    /// cell unshielded, or whose ground is gone, falls, else loses a
    /// quarter of its condition (a twentieth shielded); settlements and
    /// towns take nothing; each creature takes its own rule (below; sea
    /// monsters `sea_monster_lightning`). The caster's brain hears of it
    /// (0x5d9d).
    pub(super) fn lightning_strike(
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
                    if let Some(ship) = o.ship.as_ref() {
                        // The occupant slot holds a ship on its cell and on
                        // the next cell it has reserved (`+0x5ab`, taken
                        // once it leaves its cell's middle); the query
                        // asks both (only the blow skips the reserved one).
                        let here = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
                        let centre = (here.0 as f32 * t + t / 2.0, here.1 as f32 * t + t / 2.0);
                        let reserved = ship
                            .path
                            .front()
                            .filter(|_| (o.x - centre.0).hypot(o.y - centre.1) > 0.5)
                            .map(|&(nx, ny)| ((nx / t).floor() as i32, (ny / t).floor() as i32));
                        if (here == c || reserved == Some(c)) && !wrecked.contains(&o.id) {
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
        // The blow (0xda93) to every object in a slot of the 9 cells; each
        // handler decides by its own cell against the struck one. Life in
        // the original's points is the port's hull x 15.
        let points = |hull: f32| (hull * 15.0).round() as i64;
        let w = self.grid.width as i32;
        let land = |x: i32, y: i32| {
            x >= 0
                && y >= 0
                && x < w
                && (y as u32) < self.grid.height
                && self.cell_types[(y * w + x) as usize] & 0xf == 8
        };
        let mut blows: Vec<(u32, f32)> = Vec::new();
        let mut gorgons = Vec::new();
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
            if !near(home) && !wrecked.contains(&o.id) {
                continue;
            }
            let centre = home == (cx, cy);
            let shielded = c.protect_magic > 0.0;
            // WorkLandObj 0x427f50 (0x428600), the barbarian structures
            // through 0x40ef70 -> 0x4e8660 -> 0x4e7640 too. (Their site
            // check's flags `+0x1d8 | 0x40` and `+0x1c8` are not decoded:
            // their site is taken to hold.)
            let work = matches!(
                o.class,
                ObjectClass::Building(0x34..=0x3e)
                    | ObjectClass::Castle(_)
                    | ObjectClass::HarpyNest
                    | ObjectClass::Barbarian(_)
            );
            let k = self.object_index(o.id).unwrap_or(0);
            let blow = match o.class {
                // The query wrecks the ship on the cell (0x4dc630, 0xda94);
                // the blow, an unshielded one (0x4de2ac, `+0x255 == 0`).
                ObjectClass::Ship if wrecked.contains(&o.id) => c.hull,
                ObjectClass::Ship if !shielded => c.hull,
                _ if work => {
                    let holds = matches!(o.class, ObjectClass::Barbarian(_)) || self.site_holds(k);
                    if (centre && !shielded) || !holds {
                        c.hull
                    } else {
                        c.max_hull * if shielded { 0.05 } else { 0.25 }
                    }
                }
                // A settlement or town (TownObj 0x441ba0 -> 0x4449d4): on
                // its anchor unshielded it falls; else the map's 0x4e9 on
                // its anchor (0x42e500 -> 0x42e1e0, the settlement's site
                // check with flag 0x40: what stands there aside) failing,
                // it falls; else 25 points of condition, 5 shielded.
                ObjectClass::Building(crate::economy::SETTLEMENT | crate::economy::TOWN) => {
                    let (hx, hy) = home;
                    let holds = self
                        .check_ground_but(crate::economy::SETTLEMENT, hx, hy, false)
                        .is_ok();
                    if (centre && !shielded) || !holds {
                        c.hull
                    } else {
                        c.max_hull * if shielded { 0.05 } else { 0.25 }
                    }
                }
                // A lair (CreaturesFlatClassTy 0x4158b7): on its anchor, or
                // its site no longer holding (0x4eb with flags 0x41: free
                // ground, what stands there aside, an inland tile), it
                // falls; else 25 points of condition, no shield variant.
                ObjectClass::CreatureSpawn => {
                    let (hx, hy) = home;
                    let holds = self.free_ground_but(hx, hy, false).is_ok()
                        && land(hx, hy)
                        && self.tile_edges(hx, hy) == 0;
                    let condition = points(c.hull) * 100 / points(c.max_hull).max(1);
                    if centre || !holds || condition <= 25 {
                        c.hull
                    } else {
                        let max = points(c.max_hull);
                        c.hull - (max * (condition - 25) / 100) as f32 / 15.0
                    }
                }
                // Harpy and dragon (0x420cd8): on their own cell removed;
                // else a quarter of the life they have (signed / 100).
                ObjectClass::Monster(0x1f8 | 0x1f9) => {
                    if centre {
                        c.hull
                    } else {
                        (points(c.hull) * 25 / 100) as f32 / 15.0
                    }
                }
                // Land creatures (0x414b0f, through their jam): killed on
                // the struck cell or when the half cell they stand on is
                // shut in the walk grid rebuilt after the dig (0x411ef0,
                // `walker_half_shut`); else 25 percentage points off, the
                // life the percentage of the full one, killed at 0.
                ObjectClass::Monster(0x1f4..=0x1f6) => {
                    let max = points(c.max_hull);
                    let pct = points(c.hull) * 100 / max.max(1) - 25;
                    if centre || self.walker_half_shut(o.id) || pct <= 0 {
                        c.hull
                    } else {
                        c.hull - (max * pct / 100) as f32 / 15.0
                    }
                }
                // The gorgon (0x402ab8) and the siren (0x404505): anywhere
                // in the 3 x 3, whatever their shield, life to 0 with the
                // caster as killer, unless resting under water (no
                // picture). The gorgon's own sound, event 116 (`hitmed`);
                // the siren's 117 has no entry.
                ObjectClass::Monster(0x1fa | 0x1fd) => {
                    if self.creature_hidden(o.id) {
                        0.0
                    } else {
                        if o.class == ObjectClass::Monster(0x1fa) {
                            gorgons.push((o.x, o.y));
                        }
                        c.hull
                    }
                }
                // A landing party (0x4227c5): removed, reported to the
                // caster.
                ObjectClass::Monster(0x1fc) => c.hull,
                _ => 0.0,
            };
            if blow > 0.0 {
                blows.push((o.id, blow));
            }
        }
        for (x, y) in gorgons {
            self.sounds.push(("hitmed", x, y));
        }
        for (id, blow) in blows {
            // 0xda93 heeds no sleep: struck straight (not through the
            // fort's units either). Put down to the caster (message
            // `+0x18`, the bolt's `+0x75`: buildings' `+0x1ac` 0x428633,
            // creatures 0x414ce0, harpies 0x420cce), but a ship's wreck to
            // no one (0x4de2cd stores -1, and 0x4aa280's wreck event 0xff).
            // No handler shows a hit or plays `hitmed` for it.
            let k = self.object_index(id);
            let ship = k.is_some_and(|k| self.objects[k].ship.is_some());
            let walker = k.is_some_and(|k| {
                matches!(self.objects[k].class, ObjectClass::Monster(0x1f4..=0x1f6))
            });
            let by = if ship { None } else { caster };
            let heard = self.sounds.len();
            self.strike(id, blow, false, by, true, data, caches);
            // A land creature cries out at its death (0x413ad0, 0x138b)
            // only when a shot has struck it since its last turn (`+0x144`,
            // set by 0x138e at 0x413b69, cleared at 0x411698 / 0x411879);
            // lightning's kill brings no 0x138e: no cry.
            if walker {
                let tail = self.sounds.split_off(heard);
                self.sounds.extend(
                    tail.into_iter()
                        .filter(|s| !matches!(s.0, "deac1d" | "destr1")),
                );
            }
        }
        // A sheep flock in the 3 x 3 is removed (0x433a77), not reported.
        let sheep: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Sheep)
            .filter(|o| {
                let f = |i| o.record.field(i).unwrap_or(0) as i32;
                near((f(4), f(5)))
            })
            .map(|o| o.id)
            .collect();
        for id in sheep {
            self.objects.retain(|o| o.id != id);
            self.flocks.remove(&id);
        }
        // Reefs (0x430d38) and whirlpools (0x448479) are removed (message
        // 10), not reported, unless under water (state `+0x74` 3,
        // 0x430320 / 0x447b80 case 3); a cast medium reef not yet up
        // (0xff) goes too.
        let hazards: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Whirlpool | ObjectClass::Reef(_)))
            .filter(|o| {
                let f = |i| o.record.field(i).unwrap_or(0) as i32;
                near((f(5), f(6)))
            })
            .filter(|o| self.hazard_wrecks(o.id))
            .map(|o| o.id)
            .collect();
        for id in hazards {
            self.remove_hazard(id);
        }
        // Amber standing (its picture `+0xd5`, 0x48ae45): its amount to 0.
        // 0x48a430 finds it empty (0x48d2f0 gives 0): state 1, then it lies
        // fallow, or goes once it has grown back as often as it may
        // (`+0x88 >= +0x84`), as one gathered out. (The original takes the
        // two ticks of its states 0 and 1 over it; here at once.)
        let amber: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Amber && o.deposit.is_some())
            .filter(|o| {
                let f = |i| o.record.field(i).unwrap_or(0) as i32;
                near((f(6), f(7)))
            })
            .map(|o| o.id)
            .collect();
        for id in amber {
            self.exhaust_deposit(id, data, caches);
        }
        // A fish shoal up (its picture `+0x111`: states 1-3, 0x48e7ef):
        // its amount to 0 and down at once (0x48d910(0): 25-75 ticks under
        // water); 0x48dd80 then finds it empty (`move_shoals`).
        let shoals: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::FishZone && !self.shoals_gone.contains_key(&o.id))
            .filter(|o| near(((o.x / t).floor() as i32, (o.y / t).floor() as i32)))
            .map(|o| o.id)
            .collect();
        for id in shoals {
            if let Some(d) = self
                .object_index(id)
                .and_then(|k| self.objects[k].deposit.as_mut())
            {
                d.remaining = Some(0.0);
            }
            self.dive_shoal(id);
        }
        let monsters: Vec<(u32, bool)> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::Monster(0x1fb))
            .map(|o| ((o.x / t).floor() as i32, (o.y / t).floor() as i32, o.id))
            .filter(|&(x, y, _)| near((x, y)))
            .map(|(x, y, id)| (id, (x, y) == (cx, cy)))
            .collect();
        for (id, centre) in monsters {
            self.sea_monster_lightning(id, centre, caster, data, caches);
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
                    caster: master,
                }),
                site: None,
            });
            self.sounds.push(("wind8", x, y));
            return Ok(());
        }
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 17];
        fields[0] = class;
        fields[1] = 1;
        fields[5] = cx as u32;
        fields[6] = cy as u32;
        // Cast (0x42f580 cases 1, 2): mode 1 (field 4); a reef of kind
        // `draw % 3 + 1` (field 7, the caster's draw; field 8 left
        // unwritten), a whirlpool its caster's player (field 7, nation x 4
        // + slot).
        fields[4] = 1;
        fields[7] = match spell {
            Spell::Reef => self.rng.next() % 3 + 1,
            _ => master.map_or(0xff, |(n, s)| n * 4 + s),
        };
        if spell == Spell::Monster {
            // UMonster records (0x42f580 case 4, 0x493780): full strength
            // and whole (fields 5, 6, percent), the cell in fields 7 and 8,
            // the area 4 about it (11, 12), a scan of 2 (13, 14), mode 2
            // (15); the type in field 10: the caster's nation's, a
            // barbarian's at random.
            fields[5] = 100;
            fields[6] = 100;
            fields[7] = cx as u32;
            fields[8] = cy as u32;
            fields[11] = 4;
            fields[12] = 4;
            fields[13] = 2;
            fields[14] = 2;
            fields[15] = 2;
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
            // A summoned monster bites its master's enemies; it stays
            // till it dies (`sea_monster`).
            obj.owner = master;
            self.summons.push((id, f32::INFINITY));
        }
        // Cast whirlpools and reefs go after 7500-15000 ticks, start under
        // the water (a whirlpool) or 75 ticks unseen (a reef), and are
        // heard as they rise (`hazards`: Whirl.cpp 0x447ee0, Reef.cpp
        // 0x4306d0).
        self.objects.push(obj);
        self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
        if spell != Spell::Monster {
            self.create_hazard(id, data, caches);
        }
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
        // Not while under the water (state 3); a ship stepping into the
        // cell (its reserved next cell, `+0x5ab`) is spared till it lies
        // there.
        let traps: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Whirlpool | ObjectClass::Reef(_)))
            .filter(|o| self.hazard_wrecks(o.id))
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
            let reserved = s
                .path
                .front()
                .is_some_and(|&(x, y)| ((x / t).floor() as i32, (y / t).floor() as i32) == cell);
            if !jason && !reserved && traps.contains(&cell) {
                if let Some(c) = o.combat.as_mut() {
                    c.hull = 0.0;
                }
                wrecked.push(o.id);
                continue;
            }
            if zetes {
                continue;
            }
            let Some(c) = o.combat.as_ref() else {
                continue;
            };
            let blow = if c.protect_magic > 0.0 {
                magic::WATERSPOUT_STRIKE * magic::SHIELD_FACTOR
            } else {
                magic::WATERSPOUT_STRIKE
            };
            for &(_, caster) in struck.iter().filter(|s| s.0 == cell) {
                spouts.push((o.id, blow, caster));
            }
        }
        // A reef's or whirlpool's wreck is put down to no one (0x4aa280,
        // the wreck event, gives `+0x7f` 0xff).
        for id in wrecked {
            self.hit(id, 0.0, None, data, caches);
        }
        // Shared out as any blow (hull, crew, weapons), put down to the
        // caster (0xda96: the code-4 blow of class 0x25c, player = the
        // message's `+0x18`, the tornado's `+0x75`, 0x4de3ce).
        for (id, blow, caster) in spouts {
            self.share_blow(id, blow, caster, data, caches);
        }
    }

    /// Moves the tornadoes on (0x43a380); the cells they strike, with
    /// their casters.
    fn run_tornadoes(&mut self, dt: f32) -> Vec<((i32, i32), Option<super::Owner>)> {
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
                            struck.push((self.tornado_hop(&mut tw), tw.caster));
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
                            struck.push((self.tornado_hop(&mut tw), tw.caster));
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
