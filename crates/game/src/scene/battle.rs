//! Fighting: gunfire, ramming, boarding, and the effects drawn for them.

use super::*;

/// A cell a fort's group looks at, with its slots' objects (slot: 0 the
/// occupant, 1 `+0x10`, 2 `+0xc`; object index).
type GroupCell = ((i32, i32), Vec<(u8, usize)>);

/// Player 16 (`+0x24f` 0x10): its ships are not taken, nor do they take.
const PLAYER_16: (u32, u32) = (4, 0);

/// The game's alliance table (game `+0x2f45`, 20 x 20 bytes, row the
/// first player id `nation * 4 + slot`): 1 where a player holds the other
/// its ally. A new game zeroes it (0x4ec1a0, commands 0x100 / 0x10b /
/// 0x10c) and a saved game reloads its `ALIANCES` chunk (0x4ed620, 400
/// bytes); only a network game writes it (its set-up 0x532b20, which sets
/// game `+0xd80` = 3; the diplomacy message 0x50f910 0x4e5 case 6, taken
/// only with `+0xd80` = 3; its sync 0x536c3a). The port has no network
/// game, so it stays all 0: every player at war with every other.
const ALLIANCES: [[u8; 20]; 20] = [[0; 20]; 20];

impl Scene {
    /// 0x4efe20: player `p`'s standing with player `q` (ids `nation * 4 +
    /// slot`), from the alliance table: 0 for the same player, -2 when
    /// neither holds the other an ally, -1 when only `q` holds `p` one, 1
    /// when only `p` holds `q` one, 2 when both do (any other byte 0).
    /// Below 0 is war.
    pub fn diplomacy(p: u8, q: u8) -> i32 {
        if p == q {
            return 0;
        }
        let at = |a: u8, b: u8| {
            ALLIANCES
                .get(a as usize)
                .and_then(|r| r.get(b as usize))
                .copied()
                .unwrap_or(0)
        };
        match (at(p, q), at(q, p)) {
            (0, 0) => -2,
            (1, 0) => -1,
            (0, 1) => 1,
            (1, 1) => 2,
            _ => 0,
        }
    }

    /// The player id 0x4efe20 takes for an owner (`nation * 4 + slot`, a
    /// byte: the object's `+0x54` * 4 + `+0x55`, a ship's `+0x24f`).
    pub fn player_id((nation, slot): (u32, u32)) -> u8 {
        (nation * 4 + slot) as u8
    }

    /// Whether `a` is at war with `b`: both owned, and 0x4efe20(`b`'s
    /// player, `a`'s) < 0, as the fire pass's look 0x4bfc60 asks it (the
    /// target's player first, the looker's `+0x24f` second). A building
    /// asleep is no one's enemy (0x40dfc0): nothing aims at it and it aims
    /// at nothing.
    pub fn are_enemies(a: &MapObject, b: &MapObject) -> bool {
        a.asleep.is_none()
            && b.asleep.is_none()
            && matches!((a.owner, b.owner), (Some(x), Some(y))
                if Self::diplomacy(Self::player_id(y), Self::player_id(x)) < 0)
    }

    /// Whether `o` is a fallen building's ruins whose kept bytes (`+0x54
    /// / +0x55`, `kept_owner`; 0x423d60 clears neither) are not `me`'s
    /// and at war with it (0x4efe20 < 0): what a ship's answer to the
    /// pointer (0x4dc630 state 0, 0x4dd424) and its taking of an attack
    /// (0x4ab3f0: 0x4ac050 with the record's player -1 finds it by id,
    /// 0x51f1f0; `+0x57` 1, so the bytes are tested) see. (The ruins are
    /// out of their player's list: the port's `owner` is none.)
    pub fn ruins_enemy(&self, me: &MapObject, o: &MapObject) -> bool {
        o.owner.is_none()
            && match (me.owner, self.ruins.get(&o.id).and_then(|r| r.owner)) {
                (Some(x), Some(y)) => {
                    x != y && Self::diplomacy(Self::player_id(y), Self::player_id(x)) < 0
                }
                _ => false,
            }
    }

    /// Whether `me`'s at-will search may pick `o`: a ship by its masks
    /// (`objects::Behaviour::may_pick`: players `+0x689`, classes
    /// `+0x68d`, positions `+0x685`), anything else always.
    pub(super) fn picks_at_will(&self, me: &MapObject, o: &MapObject) -> bool {
        let Some(ship) = me.ship.as_ref() else {
            return true;
        };
        let class = combat::class_bit(o.record.class_id, o.ship.as_ref().map(|s| s.kind));
        o.owner.is_some_and(|ow| ship.behaviour.may_pick(ow, class))
    }

    /// Orders armed ship `ship` to attack the enemy ship or building `target`.
    pub fn order_attack(&mut self, ship: u32, target: u32) -> Result<(), String> {
        let i = self.object_index(ship).ok_or("No such ship")?;
        if let Some(jam) = self.jams.iter().find(|j| j.id == target) {
            return self.order_attack_jam(i, jam.id);
        }
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
            // An enemy's ruins pass the answer's test of the kept bytes
            // (0x41bec0), and the order takes them: 0x446960 finds them by
            // id (0x51f1f0). Its groups then never shoot: their reach test
            // 0x447400 passes over a building of classes 0x32-0x3e in
            // state `+0xa4` 2, so the target is kept and nothing is shot
            // at it (0x4469c0; the port's ruins are in no group's cells).
            let ruins = self.ruins_enemy(&self.objects[i], &self.objects[t]);
            if !ruins
                && (self.objects[t].combat.is_none()
                    || !Self::are_enemies(&self.objects[i], &self.objects[t]))
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
        // An enemy's ruins take the attack as a standing building does
        // (0x4ab3f0: found by id, the kept bytes an enemy's, 0x4abf60 a
        // distance only), the position's player code `+0x644` the kept
        // owner's; the fire pass's first look for it (0x4c09c0 ->
        // 0x4b9140 -> 0x4ac050 with that code, the player's list) finds
        // nothing and stops the attack with notice 0x13a (`fire_attack`).
        let ruins = self.ruins_enemy(&self.objects[i], &self.objects[t]);
        if !ruins
            && (self.objects[t].combat.is_none()
                || !Self::are_enemies(&self.objects[i], &self.objects[t]))
        {
            return Err("Choose an enemy ship or building".into());
        }
        let armed = self.objects[i].armed_positions();
        let code = match self.kept_owner(&self.objects[t]) {
            Some((n, s)) if ruins => (n * 4 + s) as i8,
            _ => self.player_code(target),
        };
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path.clear();
        ship.task = Some(Task::Attack { target, jam: None });
        // The ship's tick takes the attack's fire records (0x4ab3f0 kind
        // 3 at every position, order 0x16 sub 7).
        ship.take_attack(target, code, armed);
        Ok(())
    }

    /// Armed ship `i`'s attack on land creatures' jam `jam` (order 0x16
    /// {jam id, 7}: the navy's fleet attack task 0x4d04a0 keeps the jam's
    /// id and sends it again, `fleet_attack`): the jam is side 2, so no
    /// diplomacy test (0x4e46c0 asks only for weapons); the ship's tick
    /// (0x4ab3f0, 0x4ab5a4) takes the jam's first member (0x415df0) as
    /// its target. A fort's attack at a jam gets the member from the pick
    /// (`Scene::jam_at`).
    fn order_attack_jam(&mut self, i: usize, jam: u32) -> Result<(), String> {
        if self.objects[i].holds_units() {
            let member = self.jam_first(jam).ok_or("No such target")?;
            let ship = self.objects[i].id;
            return self.order_attack(ship, member);
        }
        if self.objects[i].ship.is_none() || !self.objects[i].is_armed() {
            return Err("This ship has no weapons".into());
        }
        if self.objects[i]
            .combat
            .as_ref()
            .is_some_and(|c| c.lime > 0.0)
        {
            return Err(String::new());
        }
        let target = self.jam_first(jam).ok_or("No such target")?;
        let armed = self.objects[i].armed_positions();
        let code = self.player_code(target);
        let ship = self.objects[i].ship.as_mut().unwrap();
        ship.path.clear();
        ship.task = Some(Task::Attack {
            target,
            jam: Some(jam),
        });
        ship.take_attack(target, code, armed);
        Ok(())
    }

    /// The first member of jam `jam` (0x415df0), if the jam is there.
    pub(super) fn jam_first(&self, jam: u32) -> Option<u32> {
        self.jams
            .iter()
            .find(|j| j.id == jam)?
            .members
            .first()
            .copied()
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
    /// found; a barbarian structure the same as they come (`+0x200`,
    /// `+0x1f8`, `+0x1fc`, 0x40f75b). The pointer picks a land
    /// creatures' jam (0x49a580), class 0x1f7, which the fort turns into
    /// the jam's first member (0x415df0): the port's pick gives that
    /// member (`Scene::jam_at`).
    pub fn fort_attack(&mut self, site: u32, target: u32, groups: u8, masks: combat::FortMasks) {
        // A fort's call after the order (0x41bd2f; nothing below pushes a
        // sound), whatever is found.
        self.fort_voice(site);
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
        match owner {
            Some(owner) if fort => c.fire.masks = masks.stored(owner),
            _ => c.fire.masks = masks,
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
    /// only, storing the masks as they come (`+0x1f8`, `+0x1fc`, `+0x200`
    /// from `+1`, `+5`, `+9`, 0x40f8e9).
    pub fn fort_order(&mut self, site: u32, sub: u8) -> Result<(), String> {
        self.fort_command(site, sub, combat::FortMasks::PLAYER_BEHAVIOUR)
    }

    /// Command `#` (0x23) sub `sub` with the payload's masks (players
    /// `+1`, classes `+5`, kinds `+9`) to fort or barbarian structure
    /// `site` (0x419f80 case 0x23, 0x40ef70; see `fort_order`). Also what
    /// a computer's forts flotilla sends its forts (0x4c42f0, 0x4c4230:
    /// no class test, every member of the group).
    pub(super) fn fort_command(
        &mut self,
        site: u32,
        sub: u8,
        payload: combat::FortMasks,
    ) -> Result<(), String> {
        let i = self
            .object_index(site)
            .filter(|&i| self.objects[i].holds_units())
            .ok_or("Not a fort")?;
        let fort = matches!(self.objects[i].class, ObjectClass::Building(0x34..=0x36));
        let owner = self.objects[i].owner;
        let c = self.objects[i].combat.as_mut().unwrap();
        let masks = owner.map(|o| payload.stored(o));
        match sub {
            1 => {
                c.fire.masks = match masks {
                    Some(m) if fort => m,
                    _ => payload,
                };
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

    /// An object's side (`+0x57`): 2 the monsters' (the castles, 0x410150,
    /// which the load 0x427f50 makes nation 4 slot 1; lairs, 0x414e20; the
    /// jam, 0x415c10; the port's other monsters, whose constructors were
    /// not all read), 1 a player's (barbarian structures too: 0x40de20
    /// makes them side 1, nation 4, and the load 0x427f50 keeps the
    /// record's nation and slot, 4 and 0 in every mission: player 16), 0
    /// none.
    /// The player code an attack's record keeps of object `id` (0x4ab3f0,
    /// `+0x644`): -1 for side 2 (or none), else nation x 4 + slot.
    pub(super) fn player_code(&self, id: u32) -> i8 {
        match self.object_index(id).map(|t| &self.objects[t]) {
            Some(o) if Self::side_of(o) != 2 => o.owner.map_or(-1, |(n, s)| (n * 4 + s) as i8),
            _ => -1,
        }
    }

    /// 0x4ac050(code, id): object `id` found again by an attack's record,
    /// `code` its player code: with -1 anything of that id (0x51f1f0),
    /// else only among that player's objects (0x4a8710).
    pub(super) fn find_by_code(&self, code: i8, id: u32) -> Option<usize> {
        let t = self.object_index(id)?;
        if code < 0 {
            return Some(t);
        }
        let owner = self.objects[t].owner?;
        (Self::side_of(&self.objects[t]) != 2 && (owner.0 * 4 + owner.1) as i8 == code).then_some(t)
    }

    pub(super) fn side_of(o: &MapObject) -> u8 {
        if o.owner == Some(combat::MONSTERS) {
            2
        } else if o.owner.is_some() {
            1
        } else {
            0
        }
    }

    /// The shot test of fort or barbarian structure `i`'s group `item` at
    /// object `t` on cell `cell` (group vtable slot 0: a fort's 0x41c4e0,
    /// a barbarian structure's 0x40fd30): the masks (`FortMasks::pass`;
    /// a land creature by its jam's class, 0x1f7, and side 2), then the
    /// score 0x4472d0, `(reach + 1) x 10 - d x 10`, the reach the group's
    /// range in cells (0x4466c0: 0x598154 by its item and the player's
    /// reach level for item / 3, `+0x39`) and `d` the cell distance from
    /// the structure's cell (the group's `+0x14` / `+0x18`, the list's
    /// `+4` / `+8`: its `+0x74` / `+0x78`) by 0x5576d8. 0 is no.
    pub(super) fn group_score(&self, i: usize, item: u8, cell: (i32, i32), t: usize) -> u32 {
        let (me, it) = (&self.objects[i], &self.objects[t]);
        let Some(c) = me.combat.as_ref() else {
            return 0;
        };
        let fort = matches!(me.class, ObjectClass::Building(0x34..=0x36));
        let walker = self.walkers.iter().any(|w| w.id == it.id);
        let class = if walker {
            combat::class_bit(0x1f7, None)
        } else {
            combat::class_bit(it.record.class_id, it.ship.as_ref().map(|s| s.kind))
        };
        let side = if walker { 2 } else { Self::side_of(it) };
        if !c.fire.masks.pass(fort, item, side, it.owner, class) {
            return 0;
        }
        let reach = combat::range_cells(item, self.levels_of(me.owner).reach_for(item)).1 as i32;
        let (fx, fy) = self.cell_of(i);
        let (dx, dy) = ((cell.0 - fx) as i64, (cell.1 - fy) as i64);
        let d = super::fort::root((dx * dx + dy * dy) as u64) as i32;
        ((reach + 1) * 10 - d * 10) as u32
    }

    /// The cells group `item` of structure `i` looks at, row by row, each
    /// with what it may aim at there (0x446d60 / 0x447400): the box of
    /// its range (0x4466c0) about the structure's cell, on the map, where
    /// its reach mask (0x447840, `reach_mask`) is set; with the fog on,
    /// only cells the structure's player has explored (DarkTy `+0x24`
    /// layer 0 not 0, while `[0x5ca546]` and not `+0x18c`; the port keeps
    /// the human's layer only). In a cell, the slots in order: the
    /// occupant (`+4`: a ship, a building on any cell of its footprint,
    /// the land creatures' jam), the sea creatures (`+0x10`), the flyers
    /// (`+0xc`: harpy, dragon); each object that is there (vt+0x2c; the
    /// port: life left, not hidden).
    fn group_cells(&self, i: usize, item: u8) -> Vec<GroupCell> {
        let me = &self.objects[i];
        let level = self.levels_of(me.owner).reach_for(item);
        let Some(mask) = super::placement::reach_mask(item, level) else {
            return Vec::new();
        };
        let r = combat::range_cells(item, level).1 as i32;
        let (fx, fy) = self.cell_of(i);
        let (w, h) = self.map_size();
        let half = mask.len() as i32 / 2;
        let fog = self.fogged() && self.is_own(me);
        let t = TILE_SIZE as f32;
        let at = |o: &MapObject| ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let mut cells: Vec<GroupCell> = Vec::new();
        for y in fy - r..=fy + r {
            for x in fx - r..=fx + r {
                if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                    continue;
                }
                if fog && self.seen(x, y) == sight::Seen::Unexplored {
                    continue;
                }
                let (mx, my) = ((x - fx + half) as usize, (y - fy + half) as usize);
                if mask[my].as_bytes()[mx] != b'#' {
                    continue;
                }
                cells.push(((x, y), Vec::new()));
            }
        }
        if cells.is_empty() {
            return cells;
        }
        for (k, o) in self.objects.iter().enumerate() {
            if !o.combat.as_ref().is_some_and(|c| c.hull > 0.0)
                || self.creature_hidden(o.id)
                || self.shoal_gone(o.id)
            {
                continue;
            }
            let mut put = |cell: (i32, i32), slot: u8| {
                if let Some(c) = cells.iter_mut().find(|c| c.0 == cell) {
                    c.1.push((slot, k));
                }
            };
            match o.class {
                ObjectClass::Ship => {
                    if !self.in_dock(k) {
                        put(at(o), 0);
                    }
                }
                ObjectClass::Building(_)
                | ObjectClass::Barbarian(_)
                | ObjectClass::Castle(_)
                | ObjectClass::HarpyNest
                | ObjectClass::CreatureSpawn
                | ObjectClass::HeroHouse => {
                    if let Some((bx, by)) = o.cell {
                        let n = super::trade::building_cells(o);
                        for y in by..by + n {
                            for x in bx..bx + n {
                                put((x, y), 0);
                            }
                        }
                    }
                }
                ObjectClass::Monster(0x1fa | 0x1fb | 0x1fd) => put(at(o), 1),
                ObjectClass::Monster(0x1f8 | 0x1f9) => put(at(o), 2),
                _ => {}
            }
        }
        // The land creatures' jams hold their cells' occupant slots, there
        // whatever (vt+0x2c 0x411160 gives 1), each answering as its first
        // member (0x415df0).
        for jam in &self.jams {
            let first = jam.members.first().and_then(|&m| self.object_index(m));
            if let (Some(k), Some(c)) = (first, cells.iter_mut().find(|c| c.0 == jam.cell)) {
                c.1.push((0, k));
            }
        }
        for c in &mut cells {
            c.1.sort_by_key(|&(slot, _)| slot);
        }
        cells
    }

    /// The scan of structure `i`'s group `item` (0x446d60) with the tie
    /// draw `draw` and what it holds (`current`: `+0x20` and the cell it
    /// was found on, `+0x28` / `+0x2c`). Each cell gives at most one
    /// candidate, the first of its slots that is: the occupant, of
    /// another side (`+0x57`) or an enemy (0x4efe20 < 0), but a building
    /// of classes 0x32-0x3e in state `+0xa4` 2 (fallen: the port's ruins,
    /// owned by no one; a settlement mending its stock is in state 1, its
    /// job `+0x110` 2); a sea creature, the medusa (0x1fa) or the siren (0x1fd)
    /// whoever's, else an enemy; a flyer (0x1f8, 0x1f9). It is kept with
    /// a non-zero shot test (`group_score`), but the object held, kept
    /// with the score it had where it was found, whatever (up to 150 in
    /// all, 0x5c6a48). The best score wins (unsigned); when several share
    /// it, those below it are dropped, the order kept, and the one at
    /// `draw % count` wins; one alone, the first. The object held stays
    /// when it is among them and nothing scores above it. Nothing found
    /// clears it.
    pub(super) fn group_scan(
        &self,
        i: usize,
        item: u8,
        draw: u32,
        current: Option<(u32, (i32, i32))>,
    ) -> Option<(u32, (i32, i32))> {
        let me = &self.objects[i];
        let current = current
            .and_then(|(id, cell)| self.object_index(id).map(|t| (id, cell, t)))
            .map(|(id, cell, t)| (id, cell, self.group_score(i, item, cell, t)));
        let mut held = false;
        let mut list: Vec<(u32, (i32, i32), u32)> = Vec::new();
        for (cell, slots) in self.group_cells(i, item) {
            let pick = slots.iter().find(|&&(slot, k)| {
                let o = &self.objects[k];
                match slot {
                    // (Not decoded: the side of the ships a barbarian
                    // structure builds, which 0x4e2117 copies from their
                    // creation record's `+0x1f`; the port keeps the
                    // structure's own player's objects out, as before.)
                    0 => {
                        o.owner != me.owner
                            && (Self::side_of(o) != Self::side_of(me) || Self::are_enemies(me, o))
                            && !(matches!(o.class, ObjectClass::Building(0x32..=0x3e))
                                && self.fallen(o.id))
                    }
                    1 => {
                        matches!(o.class, ObjectClass::Monster(0x1fa | 0x1fd))
                            || Self::are_enemies(me, o)
                    }
                    _ => true,
                }
            });
            let Some(&(_, k)) = pick else { continue };
            // The land creatures on a cell answer as their jam, whose
            // first member is shot (0x4469c0 -> 0x415df0).
            let id = self.objects[k].id;
            let score = match current {
                Some((cur, _, s)) if cur == id => {
                    held = true;
                    s
                }
                _ => match self.group_score(i, item, cell, k) {
                    0 => continue,
                    s => s,
                },
            };
            list.push((id, cell, score));
            if list.len() == 150 {
                break;
            }
        }
        let best = list.iter().map(|c| c.2).max()?;
        if let Some((cur, cell, s)) = current {
            if held && best <= s {
                return Some((cur, cell));
            }
        }
        let tied: Vec<_> = list.iter().filter(|c| c.2 == best).collect();
        let c = if tied.len() > 1 {
            tied[draw as usize % tied.len()]
        } else {
            tied[0]
        };
        Some((c.0, c.1))
    }

    /// Whether structure `i`'s group `item` reaches its target `target`
    /// (0x447400): the first of `group_cells` holding it, in any slot,
    /// then the shot test there.
    pub(super) fn group_track(&self, i: usize, item: u8, target: u32) -> Option<usize> {
        let t = self.object_index(target)?;
        let jam = self
            .jams
            .iter()
            .find(|j| j.members.contains(&target))
            .map(|j| j.cell);
        let (cell, _) = self
            .group_cells(i, item)
            .into_iter()
            .find(|(c, slots)| Some(*c) == jam || slots.iter().any(|&(_, k)| k == t))?;
        (self.group_score(i, item, cell, t) != 0).then_some(t)
    }

    /// Each standing, unlimed fort and barbarian structure steps its unit
    /// list's generator once for each of its twelve groups, every tick
    /// (0x41a96f, 0x40f61d: state `+0xa4` 1, `+0x144` 0; the fort also
    /// `+0x1e4` 0, which only a saved record sets), from the draw of its
    /// own generator its message 2 put there (`Scene::building_rng`).
    pub(super) fn step_fort_draws(&mut self) {
        for k in 0..self.objects.len() {
            if !self.objects[k].holds_units() || self.objects[k].progress.is_some() {
                continue;
            }
            self.building_rng(k);
            let o = &mut self.objects[k];
            if let Some(c) = o.combat.as_mut().filter(|c| c.lime <= 0.0) {
                c.fire.step_draws();
            }
        }
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
            // A ship fires from its fire pass (`scene::fire_pass`).
            .filter(|o| o.ship.is_none())
            // Land creatures throw from their own state step (0x412740
            // state 3, `Scene::tick_walkers`), handed over in `throws`.
            .filter(|o| !self.walkers.iter().any(|w| w.id == o.id))
            .map(|o| o.id)
            .collect();
        let mut volleys: Vec<Volley> = Vec::new();
        for id in armed {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let levels = self.levels_of(self.objects[i].owner);
            let c = self.objects[i].combat.as_mut().unwrap();
            // Limed, a fort or stronghold does not fire (0x419e00,
            // 0x40ef70).
            if c.stone_len > 0 || c.lime > 0.0 {
                continue;
            }
            let shooters = self.objects[i].shooters();
            let c = self.objects[i].combat.as_mut().unwrap();
            c.reloads.resize(shooters.len(), 0.0);
            for r in &mut c.reloads {
                *r = (*r - dt).max(0.0);
            }
            let reach_level = |w: u8| levels.reach_for(w);
            let rate_level = |w: u8| levels.rate_for(w);
            // The longest of its weapons' ranges.
            let range = shooters
                .iter()
                .map(|&w| combat::reach(w, reach_level(w)).1)
                .fold(0.0, f32::max);
            // Forts and barbarian structures with no mode set on any group
            // (order 10, 0x4469b0) do not fire at all (0x4469c0); the others
            // pick by group below.
            let idle = match self.objects[i].combat.as_ref().map(|c| c.fire) {
                Some(fire) if self.objects[i].holds_units() => {
                    if !fire.fires() {
                        continue;
                    }
                    false
                }
                _ => true,
            };
            let theseus = false;
            let me = &self.objects[i];
            // The others fire at enemy ships in range.
            let target = if idle {
                self.objects
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| {
                        o.ship.is_some()
                            && Self::are_enemies(me, o)
                            && self.picks_at_will(me, o)
                            && dist(me, o) <= range
                            && o.combat.as_ref().is_none_or(|c| c.phantom <= 0.0)
                            && !(me.owner == Some(combat::MONSTERS)
                                && o.ship
                                    .as_ref()
                                    .is_some_and(|s| s.has_hero(objects::ORPHEUS)))
                    })
                    .min_by(|a, b| dist(me, a.1).total_cmp(&dist(me, b.1)))
                    .map(|(ti, _)| ti)
            } else {
                None
            };
            // A fort's or barbarian structure's groups (item k's units are
            // group k) shoot when loaded (0x4469c0): with mode bit 2 at
            // their target while it is in their reach (0x447400; out of
            // reach it is kept), else, bit 1 also set, at what a scan
            // finds, not kept; with bit 1 alone at what the scan finds,
            // kept (`+0x20`) for the next. A group reloads as one: a group
            // with units and a mode passes its gate (`combat::group_loaded`,
            // its own draw as `param_3`), stamps the time `+0x1c` whether
            // it finds a target or not, and all its units fire together
            // (one blow of 0x446710's strength).
            if self.objects[i].holds_units() {
                let now = self.ticks;
                let fire = self.objects[i].combat.as_ref().unwrap().fire;
                let mut last = fire.last;
                let mut picks = [None; 12];
                let mut kept = (fire.picks, fire.pick_cells);
                for g in 0..12u8 {
                    let k = g as usize;
                    let draw = fire.draws[k];
                    if !shooters.contains(&g)
                        || !(fire.at_will || fire.targets[k] != 0)
                        || !combat::group_loaded(g, rate_level(g), draw, last[k], now)
                    {
                        continue;
                    }
                    last[k] = now;
                    picks[k] = if fire.targets[k] != 0 {
                        match self.group_track(i, g, fire.targets[k]) {
                            Some(t) => Some(t),
                            None if fire.at_will => self
                                .group_scan(i, g, draw, None)
                                .and_then(|(id, _)| self.object_index(id)),
                            None => None,
                        }
                    } else if fire.at_will {
                        let held = (kept.0[k] != 0).then_some((kept.0[k], kept.1[k]));
                        let found = self.group_scan(i, g, draw, held);
                        kept.0[k] = found.map_or(0, |f| f.0);
                        kept.1[k] = found.map_or(kept.1[k], |f| f.1);
                        found.and_then(|(id, _)| self.object_index(id))
                    } else {
                        None
                    };
                }
                let c = self.objects[i].combat.as_mut().unwrap();
                (c.fire.picks, c.fire.pick_cells) = kept;
                c.fire.last = last;
                let mut fired: Vec<(usize, Vec<u8>)> = Vec::new();
                for &w in &shooters {
                    let Some(ti) = picks.get(w as usize).copied().flatten() else {
                        continue;
                    };
                    match fired.iter_mut().find(|f| f.0 == ti) {
                        Some(f) => f.1.push(w),
                        None => fired.push((ti, vec![w])),
                    }
                }
                for (ti, weapons) in fired {
                    let owner = self.objects[i].owner;
                    volleys.push((self.objects[ti].id, weapons, owner, id, theseus, None, None));
                }
                continue;
            }
            let Some(ti) = target else { continue };
            let d = dist(&self.objects[i], &self.objects[ti]);
            if d > range {
                continue;
            }
            let target_id = self.objects[ti].id;
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
                volleys.push((
                    target_id,
                    fired,
                    self.objects[i].owner,
                    id,
                    theseus,
                    None,
                    None,
                ));
            }
        }
        for t in std::mem::take(&mut self.throws) {
            let owner = self
                .object_index(t.shooter)
                .and_then(|s| self.objects[s].owner);
            volleys.push((
                t.target,
                vec![t.weapon],
                owner,
                t.shooter,
                false,
                Some(t),
                None,
            ));
        }
        for v in volleys {
            self.send_volley(v, data, caches);
        }
        for fx in &mut self.effects {
            fx.age_ms += dt * 1000.0;
        }
        self.effects.retain(|fx| fx.frame().is_some());
    }

    /// Sends a volley off: the target's answers (the scouts, computers,
    /// brains and fleets told, a guard on aggressors turning
    /// on the shooter), then a shot for each item with its blow.
    pub(super) fn send_volley(
        &mut self,
        (target, weapons, owner, shooter, theseus, throw, pass): Volley,
        data: &GameData,
        caches: &mut Caches,
    ) {
        self.scout_attacked(target);
        // A ship's blow reaches its player only by its group's pass
        // (0x4a21c0, `report_blow`, once the shot lands and marks it);
        // the port's buildings still report as the volley goes.
        let ship = self
            .object_index(target)
            .is_some_and(|i| self.objects[i].ship.is_some());
        if !ship {
            self.computer_attacked(target, shooter);
            self.brain_hit(target);
            self.fleet_attacked(target, shooter);
        }
        // A guard told to fire on aggressors turns on whoever shoots it
        // (the port's own; its attack fires from the fire pass, by the
        // attack's records, 0x4ab3f0 kind 3).
        let mut armed_turn = false;
        if let Some(ship) = self
            .object_index(target)
            .and_then(|t| self.objects[t].ship.as_mut())
        {
            let b = ship.behaviour;
            if b.guard && b.aggressor && ship.task.is_none() {
                ship.path.clear();
                ship.task = Some(Task::Attack {
                    target: shooter,
                    jam: None,
                });
                armed_turn = true;
            }
        }
        if armed_turn {
            if let Some(t) = self.object_index(target) {
                let armed = self.objects[t].armed_positions();
                let code = self.player_code(shooter);
                let ship = self.objects[t].ship.as_mut().unwrap();
                ship.take_attack(shooter, code, armed);
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
        // A harpy nest takes its blow whole (vt+4 0x41d300), shielded or
        // not.
        let nest = self
            .object_index(target)
            .is_some_and(|t| self.objects[t].class == ObjectClass::HarpyNest);
        if warded && !holder && !nest {
            factor *= magic::SHIELD_FACTOR;
        }
        // A ship's pass shot carries them in its blow already (0x4c0230).
        let mut own_factor = 1.0;
        if theseus {
            own_factor *= objects::THESEUS_DAMAGE;
        }
        let flaming = match pass {
            Some(p) => p.flaming,
            None => self.flame_of(owner) > 0.0,
        };
        if flaming && pass.is_none() {
            own_factor *= magic::FLAME_FACTOR;
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
            return;
        };
        // A land creature's throw leaves from its picture (0x412a8d).
        let from = match throw {
            Some(t) => (t.from.0 as f32, t.from.1 as f32),
            None => (self.objects[s].x, self.objects[s].y),
        };
        let Some(t) = self.object_index(target) else {
            return;
        };
        // Land creatures throw as the original's (CyclopClassTy
        // 0x412740 state 3, 0x4126f0): their own picture, a level-1
        // weapon's blow less as they weaken (0x594ab8 by their
        // percentage), hitting or not as their own draw said.
        let thrower = match (self.objects[s].class, throw) {
            (ObjectClass::Monster(c @ 0x1f4..=0x1f6), Some(t)) => Some((c, t)),
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
            let (strikes, level, letter, weaken) = match (thrower, pass) {
                (Some((class, t)), _) => (t.strikes, 1, Some(combat::throw_letter(class)), 1.0),
                (None, Some(p)) => (p.accuracy != 0, 0, None, 1.0),
                (None, None) => {
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
            let blow = if let Some((_, t)) = thrower {
                combat::throw_blow(w, t.pct) * factor
            } else if let Some(p) = pass {
                p.blow * factor
            } else if w == combat::QUICKLIME {
                combat::lime_power(level) * health * factor * own_factor * weaken
            } else {
                let worn = self.objects[s].mean_condition(w) as f32 / 100.0;
                combat::hull_damage_at(w, level) * factor * own_factor * weaken * worn
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
                    to: throw
                        .and_then(|t| t.to)
                        .or(pass.and_then(|p| p.to))
                        .map(|(x, y)| (x as f32, y as f32)),
                },
                data,
                caches,
            );
        }
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
    /// lime's, which spares structures, in the units' points. A fort's
    /// units' share (0x4184f0) and then the structure's rest (LandObj's
    /// drain 0x423d60, reached through 0x4e7550 and 0x427bb0) are each cut
    /// the same way: under the magic shield (`+0x15c`) x 20 / 100 for a
    /// fireball (kind 10), else x 100 / 150 if flaming; under the weapon
    /// shield (`+0x17c`) x 20 / 100 more. A barbarian structure's units
    /// take theirs uncut (0x40e0f0), its rest, written back to the
    /// message's `+9`, cut by the same 0x423d60 (0x4e7550 -> 0x427bb0).
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
        let (magic_shield, weapon_shield) = (c.protect_magic > 0.0, c.protect_weapons > 0.0);
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
        let cut = |v: u32| {
            let mut v = v;
            if magic_shield {
                if item == super::shot::FIREBALL {
                    v = v * 20 / 100;
                } else if flaming {
                    v = v * 100 / 150;
                }
            }
            if weapon_shield {
                v = v * 20 / 100;
            }
            v
        };
        let rest = blow - share;
        let hull = if blow > 0 && damage > 0.0 {
            damage * cut(rest) as f32 / blow as f32
        } else {
            damage
        };
        if (0x34..=0x36).contains(&class) {
            share = cut(share);
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
        self.strike(target, hull, by, true, data, caches);
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
                self.strike(target, 0.0, by, true, data, caches);
            }
            return;
        }
        // The hull first; a wreck takes no more (0x4be0c0).
        self.hit(target, sh.hull_part(blow), by, data, caches);
        self.wound(target, sh.crew_part(blow) * 15.0);
        self.wear(target, sh.weapons_part(blow) * 15.0);
        // Afloat, its stats refreshed (0x4befc0 -> 0x4bee60).
        self.refresh_stats(target);
    }

    /// Deals `damage` to the hull of `target`; a sunk ship is removed and a
    /// destroyed building left in ruins.
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
        if self.blow_dropped(target) {
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
        self.strike(target, damage, by, true, data, caches);
    }

    /// Whether a blow (message 0x138e) on `target` is dropped: the town's
    /// handler 0x43b290 takes 0x138e off its queue (0x55b500) before the
    /// building's own 0x423d60 reads it when the town (class 0x33) is in
    /// state `+0xa4` 0, going up (an upgrade), whoever dealt it; any other
    /// town, and every other message, is left for 0x423d60. (Its report
    /// 0x5d9c, sent first in any state, is not changed here.)
    pub(super) fn blow_dropped(&self, target: u32) -> bool {
        self.object_index(target)
            .is_some_and(|t| self.objects[t].class == ObjectClass::Building(TOWN))
            && self.construction_at(target).is_some()
    }

    /// The same, with no picture or sound at the blow: a blow is a message
    /// to the struck object (a ship's code 4, 0x4a9e30, taken in its tick
    /// 0x4ab930 by 0x4befc0, which changes only the hull's own damage
    /// picture, 0x4be030 -> 0x4e1bb0; a building's or land creature's
    /// 0x138e, 0x423d60 / 0x413ad0, a building's fires its only
    /// pictures), and the senders without a shot make none at the struck
    /// object (ram 0x4c1c60 / 0x4c1ad0, boarding 0x4b5b00 / 0x4b5df0 /
    /// 0x4c3410, a sea monster's bite 0x490830 and 0xf166, the Gorgon's
    /// stare 0x401f70 and 0x158bf, the charm 0x4a74c0, the waterspout
    /// 0x43a6c0 and 0xda96, reefs and whirlpools 0xda94 / 0xda95): their
    /// own pictures are the ram's splinters, the boarding fight, the
    /// monster's strike. `by` is the player who dealt the blow: the one the
    /// statistics take for the killer (a ship's sinking blow keeps its
    /// giver, `+0x80b` to `+0x7f`, 0x4befc0, 0x4be0c0, and the death
    /// reports it to 0x50e750); `None` for none (0xff). Unless `report`,
    /// a death is not reported at all (a harpy or dragon shot down).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn strike(
        &mut self,
        target: u32,
        damage: f32,
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
        // The Gorgon takes any blow herself (vt+4 0x4014d0, no hit shown).
        if self.gorgons.contains_key(&target) {
            self.gorgon_struck(target, damage, by, None);
            return;
        }
        // The siren too (vt+4 0x403480).
        if self.sirens.contains_key(&target) {
            self.siren_struck(target, damage, by);
            return;
        }
        // A land creature takes its blows at its next tick (0x4139e0
        // queues message 0x138e; 0x413ad0).
        if self.walkers.iter().any(|w| w.id == target) {
            self.walker_struck(target, damage, by);
            return;
        }
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
        let is_building = self.objects[ti].ship.is_none();
        if destroyed {
            if report {
                self.note_destroyed(ti, by);
            }
            // A ship's treasures stay aboard till its sinking's
            // next-to-last frame (0x4e4f20, `sink_drop`).
            let aboard: Vec<objects::Treasure> = if is_building {
                self.drop_held(target, data, caches);
                Vec::new()
            } else {
                let (aboard, kept): (Vec<_>, Vec<_>) =
                    self.held.drain(..).partition(|h| h.0 == target);
                self.held = kept;
                aboard.into_iter().map(|h| h.1).collect()
            };
            // A fort going up tells its town (0x4184f0, 0x2491); a
            // building gone leaves its town's list (the drain 0x427bb0,
            // 0x2874); a settlement or town gone tells its buildings.
            self.fort_down(target);
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
            // A building falls: its fires and smokes go out, its rubble
            // is laid (0x138b, 0x4243a2, 0x4244a4).
            if matches!(self.objects[ti].class, ObjectClass::Building(_)) {
                self.building_fires.retain(|f| f.building != target);
                self.lay_rubble(ti, data, caches);
            }
            // Creatures die with their own cry (land creatures 0x413ad0:
            // events 100-102 by class; the harpy 0x41fb02 event 103; the
            // sea's creatures events 105-107), buildings fall with their
            // `+0x1a8` (0x424492): event 4 `razru2` (0x422dec), but the
            // sea and war castles' 0x6c / 0x6d (0x410ca0 at 0x410d78 /
            // 0x410d3a), whose `SND_` lists are empty.
            let sound = match self.objects[ti].class {
                ObjectClass::Monster(0x1f8) => Some("deadga1"),
                ObjectClass::Monster(0x1f9) => Some("dedr2"),
                ObjectClass::Monster(_) => Some("deamons"),
                ObjectClass::Castle(_) => None,
                _ if is_building => Some("razru2"),
                _ => None,
            };
            if let Some(sound) = sound {
                self.sounds.push((sound, tx, ty));
            }
            // A ship gone plays only a lament if it was the player's
            // (event 3) or a fanfare ("deny", event 319) if not (0x4dec40
            // at its end, 0x50e570 mode 5); no other sound or picture.
            if !is_building {
                let own = self.is_own(&self.objects[ti]);
                self.sounds
                    .push((if own { "deadsh" } else { "deny" }, tx, ty));
            }
            self.ship_orders.retain(|p| p.site != target);
            self.constructions.retain(|c| c.site != target);
            // Ships go down with the sinking picture of their hull at
            // their heading (0x4dec40: `+0x74` 5, 0x4e1bb0(0), layer 0's
            // frame 0 and delay `+0x7d` 2), a frame each time more than 2
            // ticks have passed (0x4c12b0), gone past the last (message
            // 10). The port counts frame 0 from the sinking, the original
            // from the hull picture's last step (`+0x81`). At the
            // next-to-last frame the sunken chest (0x4e4e30) and the
            // treasures aboard (0x4e4f20) are left (`sink_drop`).
            // A hull with no sinking picture shows nothing; a fallen
            // building only its ruins (0x423d60 on 0x138b: no blast).
            if let Some(ship) = self.objects[ti].ship.as_ref() {
                let nation = self.objects[ti].record.fields.get(5).copied().unwrap_or(1);
                let frames =
                    objects::ship_sink_frames(data, caches, ship.kind, nation, ship.heading)
                        .unwrap_or_default();
                self.sink_drop(ti, frames.len(), aboard);
                if !frames.is_empty() {
                    self.effects.push(Effect {
                        x: tx,
                        y: ty,
                        frames,
                        age_ms: 0.0,
                        follow: None,
                        life_ms: None,
                        frame_ms: (3 * crate::clock::TICK_MS) as f32,
                        tornado: None,
                        site: None,
                        depth: crate::combat::FxDepth::Sinking,
                    });
                }
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
                // Ruins, owned by no one, until their collapse ends. The
                // original keeps `+0x54 / +0x55` but takes the building out
                // of its player's list at the fall (0x423d60 at 0x42439d,
                // forts' 0x4184f0 at 0x418cfe / 0x418d48: 0x50e750 -> the
                // navy's 0x83 -> 0x4a8410), which the judge's counts
                // (0x4c46c0), the ports' search (0x4c5910) and the groups
                // (0x4a8460) walk; the owner is kept with the ruins for
                // the radar icon, taken off only at the removal.
                Some(frames) => {
                    let obj = &mut self.objects[ti];
                    obj.frames = frames;
                    obj.combat = None;
                    let owner = obj.owner.take();
                    // The collapse plays once, then the object goes
                    // (0x4230f0 state 2, `step_ruins`).
                    obj.progress = Some(0.0);
                    obj.picture = None;
                    obj.label = format!("Ruins ({})", obj.label);
                    let frames = obj.frames.len() as u32;
                    self.ruins.insert(
                        target,
                        Construction {
                            site: target,
                            frames,
                            owner,
                            ..Default::default()
                        },
                    );
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

    /// Whether ship `ship` offers the attack pointer at an enemy (0x4e45c0
    /// mode 4): with bit 11 of its word (`Scene::capabilities`) and a
    /// weapon at any position (0x4e35e0 with 0).
    pub fn may_fire(&self, ship: u32) -> bool {
        self.object_index(ship)
            .is_some_and(|i| self.capabilities(i) & 0x800 != 0 && self.objects[i].is_armed())
    }

    /// Whether ship `ship` may board (`board`) or ram object `target`, as
    /// the pointer's choice asks (0x4e45c0): with bit 4 (board) or 3 (ram)
    /// of its word (`Scene::capabilities`), which a ship being built or
    /// with its commands held (0x4b37d0) lacks; the board also
    /// with marines aboard (0x4e6700 0x17: 0x4bc270(1), items 0-2) or a
    /// hero who fights (0x4b4dd0); then the target a ship (0x50e600 200),
    /// the picker of type neither 0 nor 1, and the target taking a contact
    /// (`+0x261 & 0x1000`) (0x4e3580, 0x4e3800).
    pub fn may_close(&self, ship: u32, target: u32, board: bool) -> bool {
        let (Some(i), Some(t)) = (self.object_index(ship), self.object_index(target)) else {
            return false;
        };
        let Some(s) = self.objects[i].ship.as_ref() else {
            return false;
        };
        if self.capabilities(i) & if board { 0x10 } else { 8 } == 0 {
            return false;
        }
        if board && s.crew.count() == 0 && !s.heroes.iter().any(|&h| h != objects::ORPHEUS) {
            return false;
        }
        self.objects[t].ship.is_some() && s.kind > 1 && self.takes_contact(t)
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
        // 0x4e6700 0x17: marines aboard or a hero who fights.
        if board && ship.crew.count() == 0 && !ship.heroes.iter().any(|&h| h != objects::ORPHEUS) {
            return Err("There is no crew to board with".into());
        }
        ship.path.clear();
        ship.closing = None;
        // `+0x6a0 = 0` (0x4aa840 at 0x4aa8ee).
        ship.hook_busy = false;
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
    /// `RAM_ATTACK`, `BOARD_ATTACK`, `FIRE_ON_AGGRESSOR`): the left press
    /// 0x4f34c0 sends command `#` with the button's state as the sub.
    /// Guard 0x6f / 0x70 sub 1 (2 when set), fire on aggressor 0x72 /
    /// 0x73 sub 3 (4 when set), payload players `+1` -1, classes `+5` -1,
    /// `+9` 0xffff (positions 0xff, no ram or board bit); board 0x78 sub
    /// 5 (0xe when set), ram 0x79 sub 6 (0xf when set), payload all -1.
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
        let s = self.objects[i].ship.as_mut().ok_or("Not a ship")?;
        let firing = s.fire_task();
        let b = &mut s.behaviour;
        let (sub, third) = match command {
            c::GUARD => (if b.guard { 2 } else { 1 }, 0xffff),
            c::RAM_ATTACK => (if b.ram { 0xf } else { 6 }, u32::MAX),
            c::BOARD_ATTACK => (if b.board { 0xe } else { 5 }, u32::MAX),
            c::FIRE_ON_AGGRESSOR if b.guard => (if b.aggressor { 4 } else { 3 }, 0xffff),
            c::FIRE_ON_AGGRESSOR => return Err("The ship must be on guard".into()),
            _ => return Err("Not a behaviour".into()),
        };
        b.command(sub, u32::MAX, u32::MAX, third, firing);
        Ok(())
    }

    /// Rammers and boarders in contact: grappled ships and ram runs (see
    /// `grapple`); their closing in is `Scene::run_closing`.
    pub(super) fn run_contact(&mut self, dt: f32, data: &GameData, caches: &mut Caches) {
        self.run_grapples(dt, data, caches);
        // The prey ticks before its rammer here: a hit the blow starts
        // steps from the next tick (0x4c3ec0 state 6 is entered on the
        // tick of state 3, its step 0 a tick later).
        self.run_ram_hits(dt);
        self.run_ram_runs(dt, data, caches);
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
                speed: sa.velocity().max(sa.speed),
                rigging: self.levels_of(a.owner).speed,
            },
            combat::Rammer {
                kind: st.kind,
                ram: st.ram,
                level: self.levels_of(t.owner).ram_kinds[st.ram.min(2) as usize],
                heading: st.heading,
                speed: st.velocity(),
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
        // No barrel of powder goes off here: 0x4c1c60 and 0x4c1ad0 read
        // no item 11 and the ram (0x4c2230) spends none. A fire rammer's
        // blast is the burning S it throws as its bow strikes (0x4c2566,
        // 0x4c21c0; see `run_ram_runs`).
        // Each ship's blow is put down to the other's player (0x4c1c60:
        // the prey's 0x4c1ad0 gets the rammer's `+0x24f`, 0x4c1d63, the
        // rammer's its prey's, 0x4c1f2a).
        self.ram_blow(idt, to_target, by, data, caches);
        self.ram_blow(ida, to_self, recoil, data, caches);
        // The rammer's crew gains 330 experience (0x4b4680, 0x4c1f42).
        if let Some(i) = self.object_index(ida) {
            self.gain_experience(i, 330);
        }
    }

    /// A ram's blow `blow` to ship `id` (0x4c1ad0): the hull takes its
    /// share x 1.5, then the crew its share x 0.4 and
    /// the weapons theirs, in the units' points.
    fn ram_blow(
        &mut self,
        id: u32,
        blow: f32,
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
        let hull = sh.hull_part(blow) * combat::RAM_HULL_FACTOR;
        self.hit(id, hull, by, data, caches);
        self.wound(id, sh.crew_part(blow) * 15.0 * combat::RAM_CREW_FACTOR);
        self.wear(id, sh.weapons_part(blow) * 15.0);
        // Afloat, its stats refreshed (0x4c1ad0 -> 0x4bee60).
        self.refresh_stats(id);
    }

    /// A side's boarding strength in the original's units (`+0x53c`,
    /// 0x4bee60): 0x4b4e80's, each marine's condition x C x 15 / 1000
    /// (0x4ac1e0 with the A x 15 cache; C from 0x597d2c, `melee`), ten
    /// times that on the ship of the dead; plus the crew's experience
    /// `+0x544` / 100 percent of it. (The original keeps it from its last
    /// stats refresh; the port works it out when asked.)
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
        let base = if ship.kind == 11 { sum * 10 } else { sum };
        (ship.experience / 100 * base as i32 / 100 + base as i32) as u32
    }

    /// 0x4b4680: ship `i`'s crew gains `n` experience (`+0x544`), at most
    /// 10000. (Its notice to the panel, 0x4bebe0 every hundred, is not
    /// needed: the port's panel reads it when drawn.)
    pub(super) fn gain_experience(&mut self, i: usize, n: i32) {
        if let Some(s) = self.objects[i].ship.as_mut() {
            s.experience = (s.experience + n).min(10000);
        }
    }

    /// A side's strength against the other in a boarding (0x4b5b00): its
    /// fighting strength + 1, plus `+0x544` / 10000 of that.
    fn contest_strength(&self, i: usize) -> u32 {
        let s = self.fighting_strength(i) as i64 + 1;
        let exp = self.objects[i].ship.as_ref().map_or(0, |s| s.experience) as i64;
        (s + exp * s / 10000) as u32
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
    /// strengths a and d (each + 1, and + `+0x544` / 10000 of that), a draw
    /// of the boarders' generator % (a + d); the boarders win with chance
    /// (2a - d) / (a + d). Hercules and the ship of the dead always win
    /// unless the defenders have the same. Won, the boarders' crew gains
    /// 450 experience (0x4b4680). (The magic wood keeps boarders off
    /// sooner: their hooks fly wide, 0x4b84b0.)
    pub(super) fn board_roll(&mut self, i: usize, ti: usize) -> bool {
        let sure = |o: &MapObject| {
            o.ship
                .as_ref()
                .is_some_and(|s| s.has_hero(objects::HERCULES) || s.kind == 11)
        };
        let a = self.contest_strength(i);
        let d = self.contest_strength(ti);
        let roll = self.ship_draw(i) % (a + d);
        let line = 2 * a as i64 - d as i64;
        let won = line >= (a + d) as i64 || (roll as i64) < line;
        let (attacker, defender) = (&self.objects[i], &self.objects[ti]);
        let won = if sure(attacker) { !sure(defender) } else { won };
        if won {
            self.gain_experience(i, 450);
        }
        won
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
        // The defenders' blow is the boarders' strength with its experience
        // (0x4b5b00 `+0x721`), the boarders' without (`+0x6a4`).
        let foe = self.contest_strength(i);
        let sure = |o: &MapObject| {
            o.ship
                .as_ref()
                .is_some_and(|s| s.has_hero(objects::HERCULES) || s.kind == 11)
        };
        let (sure_a, sure_t) = (sure(&self.objects[i]), sure(&self.objects[ti]));
        let (by_a, by_t) = (self.objects[i].owner, self.objects[ti].owner);
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
        let taken = owner != Some(PLAYER_16) && self.objects[ti].owner != Some(PLAYER_16);
        self.capture(t, Some(a), owner, class, prize, data, caches);
        // The taker posts notice 0x13a with its prize to its group
        // (0x4b6f20 at 0x4b7105: 0x4a1800, `{prize, 0x17}`), but where
        // player 16 is a side.
        if taken {
            self.post_notice(a, 0x13a, t);
        }
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
    #[allow(clippy::too_many_arguments)]
    pub(super) fn capture(
        &mut self,
        id: u32,
        by: Option<u32>,
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
        if owner == Some(PLAYER_16) || self.objects[i].owner == Some(PLAYER_16) {
            let hull = self.objects[i].combat.as_ref().map_or(0.0, |c| c.hull);
            self.strike(id, hull.max(0.0) + 1.0, owner, true, data, caches);
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
        // Its phantom ends (0x4b7016-0x4b7034: 0x4df420(0), `+0x253` 0).
        if let Some(c) = obj.combat.as_mut() {
            (c.phantom, c.phantom_span) = (0.0, None);
        }
        if let Some(ship) = obj.ship.as_mut() {
            ship.crew = combat::Crew {
                class,
                squads: std::array::from_fn(|k| marines[k].len() as u32),
            };
            for (k, m) in marines.into_iter().enumerate() {
                ship.conditions[k] = m;
            }
            // The capture (0x4b6f20 at 0x4b7085) clears the ram and board
            // switches (`+0x684 &= ~6`); the guard and the masks stay.
            ship.behaviour.ram = false;
            ship.behaviour.board = false;
            // And gives it bit 13 (stop) of its capability word, the rest
            // kept (`+0x261 |= 0x2000` at 0x4b707f).
            ship.caps |= 0x2000;
            // Its weapon positions' records go (0x4b6f20 at 0x4b7058: the
            // 0x12 dwords from `+0x63c`, all four records): no target, id,
            // player or state is left for its fire pass (0x4bf9b0,
            // 0x4c0230) or the attack zone's sends; the fire state and the
            // guard stay. The pending order group (`+0x7d6 &= ~0x1f0`, 0x4b7077) and
            // the busy ship's kept code (`+0x265 = 0`, 0x4b708c) have nothing to
            // clear here: the port runs a ship's events at delivery
            // (`one_event_a_tick`) and keeps no `+0x265` (only 0x4abe50
            // writes it nonzero, and zeroes it again).
            ship.behaviour.aims = Default::default();
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
        // Then the stop event vt+4(8, 0) (0x4b7098), pending in `+0x7d6`
        // and taken at the prize's next message 0 (0x4dec40: 0x4ab720 ->
        // 0x4aa540, then 0x4ab930's machines). The capture runs in the
        // taker's tick (its boarding machine 0x4b5df0 at 0x4b6a16), so a
        // prize after it in the Sapp's walk (0x542270, the list `+0x10`)
        // stops in this tick; one before it, at the next, after that
        // tick's delivery (0x5228c0), which so loses the batch's group
        // orders (`Scene::run_capture_stops`). Either way the brain's
        // prize order comes later: 0x4b6cd0's 0x5d97 is a direct send
        // (`[1] = 2`) to the brain, whose member hears the join as notice
        // 300 at the next group walk and posts its dock order from there.
        let taker = by.and_then(|b| self.object_index(b));
        if taker.is_some_and(|b| i < b) {
            self.capture_stops.push(id);
        } else {
            self.capture_stop(i);
        }
        self.brain_prize(id, (nation, player));
        // Then message 0x5d94 with 0 for its treasures (0x4b6f20 at
        // 0x4b7142: 0x4b6e80 as the taker, while the prize holds any, for
        // each slot below its type's room 0x597afc; the slots past the
        // ones held, whose leftovers the port does not keep, are not sent).
        let held: Vec<objects::Treasure> = self.objects[i]
            .ship
            .as_ref()
            .map_or(Vec::new(), |s| s.treasures.clone());
        for tr in held {
            self.treasure_news(owner, None, tr, false);
        }
    }

    /// The capture's stop (event 8, 0x4aa540) on prize `i`: taken on bit
    /// 13 of its word (`+0x261 & 0x2000`), which the capture has just set
    /// (0x4b707f) over the boarded ship's cleared one (0x4c3410 state 9,
    /// 0x4b37d0 with 0), and 0x4ab720 runs before the machines of 0x4ab930
    /// could clear it again: `Ship::caps`, not the port's boarded hold in
    /// `Scene::capabilities`. Its task wound down (0x4abc60, `wind_down`).
    fn capture_stop(&mut self, i: usize) {
        if self.objects[i]
            .ship
            .as_ref()
            .is_some_and(|s| s.caps & 0x2000 != 0)
        {
            self.wind_down(i);
        }
    }

    /// The stops of prizes taken this tick by a taker later in the walk
    /// (`capture`), at their next tick after the delivery: the batch's
    /// group orders to them go (0x4aa540 clears the pending group, `&=
    /// 0xfffffe07`), the ship put back as it was before them, and the
    /// rest of the batch dropped (`TickOrder::Stopped`).
    pub(super) fn run_capture_stops(&mut self) {
        use super::order::TickOrder;
        for id in std::mem::take(&mut self.capture_stops) {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            if self.objects[i]
                .ship
                .as_ref()
                .is_none_or(|s| s.caps & 0x2000 == 0)
            {
                continue;
            }
            if let Some(TickOrder::Ran { before, .. } | TickOrder::Dropped { before }) =
                self.tick_orders.remove(&id)
            {
                self.objects[i].ship = Some(*before);
            }
            self.tick_orders.insert(id, TickOrder::Stopped);
            self.capture_stop(i);
        }
    }

    /// A prize taken by a campaign brain's player (event 0x5d97, sent by
    /// the capture 0x4b6f20). The handlers (miss002 0x44d0e0 ...
    /// miss013 0x478692) test the prize's player and call 0x479300(g, m,
    /// ship) for one member: player 15's in miss002 (1, 8), miss003 (0,
    /// 4), miss004 (0, 6), miss005 and miss006 (1, 5), miss007 (1, 6)
    /// (0x46088b), miss008 (1, 13) (0x464153), miss010 (1, 10)
    /// (0x46b323), miss012 and miss013 (1, 5) (0x473dee, 0x478692);
    /// player 14's in miss004 (2, 4), miss006 (2, 5), miss009 (1, 6)
    /// (0x46792a), miss011 (1, 6) (0x46f5b3), miss012 (1, 5) and miss013
    /// (1, 6). miss000 (0x449b10) and miss001 (0x44af70) have no case:
    /// the prize joins nothing. 0x479300 hands the member's record
    /// `{fleet, player}` to 0x4a0480 op 2, which (the record's fleet being
    /// kind 10) adds the ship to that fleet number of **the ship's own
    /// player** (0x49bd60) and posts message 300 to the record's player's
    /// fleet (0x4a1340). So player 14's prizes in miss012 sail with its
    /// member (2, 5) while the 300 goes to (1, 5), whose repair order
    /// hears only ships in its own list (0x4cc590): nobody sends them; in
    /// miss013 they join (2, 6), the repair, the 300 going to the idle
    /// (1, 6), which has no queue (no order): the repair sends them at
    /// its resend (`+8 > 0x5f`, `repair_order_tick`).
    fn brain_prize(&mut self, id: u32, owner: (u32, u32)) {
        if self.brain.is_none() || self.human.map(|h| (h.nation, h.slot)) == Some(owner) {
            return;
        }
        let mission = self.brain.as_ref().unwrap().mission.clone();
        // (the member called, the member of the ship's own player).
        let (heard, into) = match (mission.as_str(), owner.0 * 4 + owner.1) {
            ("miss002", 15) => ((1, 8), (1, 8)),
            ("miss003", 15) => ((0, 4), (0, 4)),
            ("miss004", 15) => ((0, 6), (0, 6)),
            ("miss004", 14) => ((2, 4), (2, 4)),
            ("miss005" | "miss006" | "miss012" | "miss013", 15) => ((1, 5), (1, 5)),
            ("miss006", 14) => ((2, 5), (2, 5)),
            ("miss007", 15) => ((1, 6), (1, 6)),
            ("miss008", 15) => ((1, 13), (1, 13)),
            ("miss009" | "miss011", 14) => ((1, 6), (1, 6)),
            ("miss010", 15) => ((1, 10), (1, 10)),
            ("miss012", 14) => ((1, 5), (2, 5)),
            ("miss013", 14) => ((1, 6), (2, 6)),
            _ => return,
        };
        if heard == into {
            self.brain_join(id, into.0, into.1);
        } else {
            self.brain_join_heard(id, into, heard);
        }
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
                depth: crate::combat::FxDepth::Over,
            }),
            Ok(_) => {}
            Err(e) => eprintln!("could not load effect {name}: {e:#}"),
        }
    }

    /// A building's timed statuses run out in its tick (LandObj 0x425990
    /// message 0), in this order: the phantom when its start `+0x13c` +
    /// length `+0x140` is below the game tick `+0xde` (0x426071-0x426096);
    /// the lime when `+0x14c + +0x150` is at most the tick (0x4262e3,
    /// `+0x124` then set to `+0x14c`, the work's restart); the weapon
    /// shield when `+0x184 + +0x188` is at most the tick and its sprite
    /// (`+0x198`) is made (0x4265a4), and the magic shield likewise with
    /// `+0x164 + +0x168` and `+0x174` (0x4267ed). The sprites are made in
    /// the same tick before the test (0x4263ef, 0x4265ed) whenever
    /// `+0x180` / `+0x160` are set, as the LandObj constructor 0x422cc0
    /// sets them for every player's building, so a shield ends at its
    /// length's tick; each sprite's step comes before its shield's test
    /// (`tick_shield_sprite`).
    pub(super) fn tick_building_statuses(&mut self, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        let mut cleared = Vec::new();
        for i in 0..self.objects.len() {
            let o = &mut self.objects[i];
            let Some(c) = o.combat.as_mut().filter(|_| o.ship.is_none()) else {
                continue;
            };
            // Saves from before the ticks: what is left, from now.
            for (left, span) in [
                (c.phantom, &mut c.phantom_span),
                (c.lime, &mut c.lime_span),
                (c.protect_weapons, &mut c.weapon_span),
                (c.protect_magic, &mut c.magic_span),
            ] {
                if left > 0.0 && span.is_none() {
                    *span = Some((now, (left / crate::clock::TICK).round() as u32));
                }
            }
            if c.phantom_span.is_some_and(|(at, len)| at + len < now) {
                (c.phantom, c.phantom_span) = (0.0, None);
            }
            if let Some((_, len)) = c.lime_span.filter(|&(at, len)| at + len <= now) {
                (c.lime, c.lime_total, c.lime_span) = (0.0, 0.0, None);
                cleared.push((o.id, crate::clock::ticks(len)));
            }
            self.tick_shield_sprite(i, true, data, caches);
            let c = self.objects[i].combat.as_mut().unwrap();
            if c.weapon_span.is_some_and(|(at, len)| at + len <= now) {
                (c.protect_weapons, c.weapon_span) = (0.0, None);
            }
            self.tick_shield_sprite(i, false, data, caches);
            let c = self.objects[i].combat.as_mut().unwrap();
            if c.magic_span.is_some_and(|(at, len)| at + len <= now) {
                (c.protect_magic, c.magic_span) = (0.0, None);
            }
        }
        self.lime_worn_off(cleared);
    }

    /// Protections running out, and effects following their objects.
    pub(super) fn run_statuses(&mut self, data: &GameData, caches: &mut Caches) {
        // A building's run out in its tick (`tick_building_statuses`), a
        // ship's in its (`tick_ship_shields`).
        let wrecked: Vec<u32> = Vec::new();
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
/// A volley: target, items, the shooter's player, the shooter, Theseus
/// aboard, a land creature's throw, a ship's pass shot (`ItemShot`).
pub(super) type Volley = (
    u32,
    Vec<u8>,
    Option<(u32, u32)>,
    u32,
    bool,
    Option<super::walkers::Throw>,
    Option<ItemShot>,
);

/// A ship's shot from its fire pass (0x4c0230): one an item, its accuracy
/// (0 flies wide) and its blow (a fifteenth of the original's, with the
/// Flame, Theseus and Greek fire's part in it) worked out by the pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ItemShot {
    pub accuracy: u8,
    pub blow: f32,
    pub flaming: bool,
    /// The target's aim point, where the port has it (a sea monster's,
    /// 0x493350).
    pub to: Option<(i32, i32)>,
}

const NOT_A_WEAPON: u8 = 254;
