//! Casts asked of a land fleet (message 0x3ee): the land fleet queues them
//! (0x4a09d0 -> 0x42c430), tries them at its passes (0x42b910 -> 0x42a500,
//! the town's check 0x43fee0) and sends the town command 0x22, which the
//! town hands to an idle temple (0x441ba0 case 0x22) that casts it
//! (0x438300 / 0x439260, `Magic.cpp` 0x42fa90 / 0x42faf0).

use super::*;
use crate::magic::{Magic, Spell, Temple};

/// A cast asked (the message's dwords; 0x4839e0-0x483c40, 0x479c20).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CastAsk {
    /// Dword 4: the kind, 5-16 (as 0x47ae10's).
    pub kind: u32,
    /// Dword 3: the member (a town) to cast from; `None` for any (-1).
    pub town: Option<u32>,
    /// Dwords 5 and 6: the cell.
    pub cell: (i32, i32),
    /// Flag 0x1000: dwords 10 and 11, the area's size from the cell.
    pub rect: Option<(i32, i32)>,
    /// Dword 7 without flag 0x400: the ship cast on.
    pub ship: Option<u32>,
    /// Dword 9: the discount in hundredths (0x488810).
    pub discount: u32,
}

/// The passes a waiting entry of op 0x3ee is tried (0x595170 + 0x3ee x 4).
pub const CAST_TRIES: u32 = 10;

/// Spells' prices by `Magic.cpp`'s numbering (0x596844 amber, 0x59687c
/// fish), 12 the big reef.
const PRICE_AMBER: [u32; 13] = [
    400, 5000, 5000, 2000, 4000, 250, 10000, 3000, 6000, 1000, 1000, 1000, 4000,
];
const PRICE_FISH: [u32; 13] = [1, 12, 12, 5, 15, 0, 25, 7, 15, 1, 1, 1, 10];

/// A brain's land fleet's queue of casts (`LFleet.cpp`, the member's
/// building group: 0x42c430 queues, 0x42b910 passes). The entries are
/// all of priority 0 (0x479c20), in the order asked, each with its passes
/// left. (Not ported: the brains' town orders, `Job`, are not in this
/// queue but run on their own pass every 50 ticks, so a part started
/// there does not end this queue's walk as in the original.)
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct CastQueue {
    /// +0x1c: the tick of the last pass.
    last: u32,
    /// +0x14: the last pass started something (a cast sent).
    started: bool,
    /// +0x28: woken: a pass may come 3 ticks after the last.
    woken: bool,
    pub(super) entries: Vec<(CastAsk, u32)>,
}

impl CastQueue {
    /// A queue first asked at `tick`, its passes so far at the multiples
    /// of 50 (0x42b370: `+0x1c` 0; the brains' land fleets start nothing
    /// before their first cast, their town orders running apart). One at
    /// `tick` itself is still to come.
    pub fn new(tick: u32) -> Self {
        let last = if tick % 50 == 0 {
            tick.saturating_sub(50)
        } else {
            tick - tick % 50
        };
        CastQueue {
            last,
            ..Default::default()
        }
    }

    /// Queues a cast (0x42c430's generic branch: after its equals).
    pub fn push(&mut self, ask: CastAsk) {
        self.entries.push((ask, CAST_TRIES));
    }
}

/// The town's check's view of a kind (0x43fee0): whether the war temple
/// casts it (kinds 9-12, 15, 16; the sea temple 5-8, 13, 14), and the
/// spell (by `Magic.cpp`'s numbering) whose learning and price it tests,
/// which for kinds 5, 7, 8 and 13 is not the spell cast (0x42a500's codes,
/// 0x42fc80): 5 tests the wind, 7 the big reef (in neither temple's order:
/// never known), 8 the reef, 13 the whirlpool.
fn town_spell(kind: u32) -> Option<(Temple, usize)> {
    Some(match kind {
        5 => (Temple::Sea, 0),
        6 => (Temple::Sea, 3),
        7 => (Temple::Sea, 12),
        8 => (Temple::Sea, 1),
        9 => (Temple::War, 8),
        10 => (Temple::War, 6),
        11 => (Temple::War, 7),
        12 => (Temple::War, 9),
        13 => (Temple::Sea, 2),
        14 => (Temple::Sea, 4),
        15 => (Temple::War, 10),
        16 => (Temple::War, 11),
        _ => return None,
    })
}

/// The spell a kind is cast as at the temple (0x42a500's code, 0x42fc80),
/// by `Magic.cpp`'s numbering; the Flame (kind 9) the temple brews itself,
/// 13 (code 0xc) is the sea temple's wind.
fn cast_spell(kind: u32) -> Option<usize> {
    Some(match kind {
        5 => 2,
        6 => 3,
        7 => 1,
        8 => 12,
        10 => 6,
        11 => 7,
        12 => 9,
        14 => 4,
        15 => 10,
        16 => 11,
        _ => return None,
    })
}

impl Scene {
    /// The standing temples of class `class` of `owner`'s town `site` (its
    /// list `+0x1d0` and count `+0x224` / `+0x228`), in the objects' order.
    fn town_temples(&self, site: u32, owner: (u32, u32), class: u32) -> Vec<u32> {
        let Some(i) = self.object_index(site) else {
            return Vec::new();
        };
        let (tx, ty) = self.cell_of(i);
        let r = BUILD_RADIUS + 2;
        self.objects
            .iter()
            .filter(|o| {
                o.owner == Some(owner)
                    && o.class == ObjectClass::Building(class)
                    && self.construction_at(o.id).is_none()
                    && o.cell
                        .is_some_and(|(x, y)| (x - tx).abs() <= r && (y - ty).abs() <= r)
            })
            .map(|o| o.id)
            .collect()
    }

    /// Whether temple `t` of `owner` is at work (`+0x10c`: learning, or
    /// brewing the Flame), so answering -1 to vt+0x9c (0x4101b0). `magic`
    /// is a computer player's, while it is out of `computers`.
    fn temple_busy(&self, t: u32, owner: (u32, u32), magic: Option<&Magic>) -> bool {
        let magic = magic.or_else(|| {
            self.computers
                .players
                .iter()
                .find(|c| c.owner == owner)
                .map(|c| &c.magic)
        });
        if let Some(m) = magic {
            let learning = [Temple::Sea, Temple::War]
                .into_iter()
                .any(|k| m.learning(k).is_some() && m.learning_site(k) == Some(t));
            return learning || (m.flame_brewing().is_some() && m.flame_site() == Some(t));
        }
        self.brain_brews.iter().any(|b| b.0 == t)
    }

    /// Whether a computer side (3 or 4) affords `n` casts of spell `idx` at
    /// `d` hundredths off: `(100 - d) x price x n / 100` amber and fish,
    /// the fish against its hundredths as they are (0x4a8e80 `+0x24`,
    /// 0x4a8f10 `+0x1c`). (The purse is the port's, as for every computer
    /// player's spending.)
    fn side_affords(&self, owner: (u32, u32), idx: usize, n: u32, d: u32) -> bool {
        let off = 100u32.saturating_sub(d);
        let amber = off * PRICE_AMBER[idx] * n / 100;
        let fish = off * PRICE_FISH[idx] * n / 100;
        let (a, f) = self.funds(owner);
        amber <= a && fish <= f
    }

    /// A computer side pays one cast of spell `idx` at `d` hundredths off
    /// (0x42f470: `price x (100 - d) / 100` amber, `price x (100 - d)`
    /// fish hundredths; nothing at 100 or more).
    fn side_pays(&mut self, owner: (u32, u32), idx: usize, d: u32) {
        if d < 100 {
            let amber = PRICE_AMBER[idx] * (100 - d) / 100;
            let fish = PRICE_FISH[idx] * (100 - d);
            self.earn(owner, -(amber as i64), -(fish as i64));
        }
    }

    /// The town's check for kind `kind` (0x43fee0): 0, or -5, -7 (not
    /// learnt), -2 (not affordable), -1 (its temples at work). Stage 0
    /// (0x487640's, `paying` None) tests the town built (`+0xa4`), a temple
    /// of the kind and the spell learnt; stage 1 also the price of `n`
    /// casts at `d` off, then a temple idle. `magic`: the computer
    /// player's (the brains' players' learning, `+0xb4` / `+0xbc`, is not
    /// kept: their spells all pass). The permission bits (player `+0x45`)
    /// are not kept for computer players: all pass.
    pub(super) fn town_cast_check(
        &self,
        site: u32,
        kind: u32,
        magic: Option<&Magic>,
        paying: Option<(u32, u32)>,
    ) -> i32 {
        let Some(i) = self.object_index(site) else {
            return -5;
        };
        let Some(owner) = self.objects[i].owner else {
            return -5;
        };
        if self.construction_at(site).is_some() {
            return -5;
        }
        let Some((temple, idx)) = town_spell(kind) else {
            return -5;
        };
        let temples = self.town_temples(site, owner, temple.class());
        if temples.is_empty() {
            return -5;
        }
        if let Some(m) = magic {
            let at = Spell::of_index(idx as u32)
                .and_then(|s| temple.order().iter().position(|&o| o == s))
                .unwrap_or(6);
            if m.known(temple).len() <= at {
                return -7;
            }
        }
        let Some((n, d)) = paying else {
            return 0;
        };
        if d != 100 && !self.side_affords(owner, idx, n, d) {
            return -2;
        }
        if temples.iter().any(|&t| !self.temple_busy(t, owner, magic)) {
            0
        } else {
            -1
        }
    }

    /// One pass at a cast entry (0x42a500) of a land fleet whose members
    /// are the towns `members`: with `fleet` (flag 0x400) one cast on each
    /// of those ships, none left ending it at once; else the member named
    /// (or, `None`, each in turn) passing the town's check twice sends it
    /// the command 0x22, which reaches it at the next tick. Whether it was
    /// done (state 3); else it waits. (The holds of flags 1 and 4 are never
    /// asked: the computer's flags are 0x800, 0xc00 and 0x1800, the
    /// brains' 0.)
    pub(super) fn cast_entry(
        &mut self,
        members: &[u32],
        ask: CastAsk,
        fleet: Option<Vec<u32>>,
        magic: Option<&Magic>,
    ) -> bool {
        let n = match &fleet {
            Some(ships) if ships.is_empty() => return true,
            Some(ships) => ships.len() as u32,
            None => 1,
        };
        let tried: Vec<u32> = match ask.town {
            Some(t) => members
                .iter()
                .copied()
                .filter(|&m| m == t)
                .take(1)
                .collect(),
            None => members.to_vec(),
        };
        for m in tried {
            if self.town_cast_check(m, ask.kind, magic, None) != 0
                || self.town_cast_check(m, ask.kind, magic, Some((n, ask.discount))) != 0
            {
                continue;
            }
            let order = |cell, rect, ship| Order::TownSpell {
                site: m,
                kind: ask.kind,
                cell,
                rect,
                ship,
                discount: ask.discount,
            };
            match (&fleet, ask.rect) {
                (Some(ships), _) => {
                    for &ship in ships {
                        self.post(order((0, 0), None, Some(ship)));
                    }
                }
                (None, Some((w, h))) => {
                    let (x, y) = ask.cell;
                    self.post(order((-1, -1), Some((x, y, w, h)), None));
                }
                (None, None) => self.post(order(ask.cell, None, ask.ship)),
            }
            return true;
        }
        false
    }

    /// The town command 0x22 reaching town `site` (0x441ba0 case 0x22): its
    /// first temple of the kind's that is idle (vt+0x9c, 0x4101b0) casts
    /// (0x438300 / 0x439260); with none, nothing. The Flame (code 5) is
    /// brewed there, paid then (`(100 - d) x 6000 / 100` amber, `(100 - d)
    /// x 15` fish hundredths), 250 ticks; the others go to `Magic.cpp`
    /// (0x42fa90 -> 0x42f580: the price at `d` off checked, 0x42ed40, and
    /// paid, 0x42f470), on the ship, at the cell, or (8, 0xe, 0xf with no
    /// cell) on each own ship over the area (0x42faf0: within 15 cells of
    /// the temple at `d` off, 0x42ef90, then cast at full price). (Not
    /// ported: kind 8's big reef, whose kind the temple draws from its own
    /// generator, `+0x18`, and kind 13's wind (message 0x186f); the area
    /// casts of kinds 12 and 16, 0x42ef90's cases 8 and 0xf: no player
    /// asks them. The temple's free casts, `Magic +0x10`, are only the
    /// brains' direct casts'.)
    #[allow(clippy::too_many_arguments)]
    pub(super) fn town_spell(
        &mut self,
        site: u32,
        kind: u32,
        cell: (i32, i32),
        rect: Option<(i32, i32, i32, i32)>,
        ship: Option<u32>,
        d: u32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(owner) = self.object_index(site).and_then(|i| self.objects[i].owner) else {
            return;
        };
        let Some((temple, _)) = town_spell(kind) else {
            return;
        };
        let Some(t) = self
            .town_temples(site, owner, temple.class())
            .into_iter()
            .find(|&t| !self.temple_busy(t, owner, None))
        else {
            return;
        };
        if kind == 9 {
            if d != 100 && !self.side_affords(owner, 8, 1, d) {
                return;
            }
            self.side_pays(owner, 8, d);
            if let Some(c) = self.computers.players.iter_mut().find(|c| c.owner == owner) {
                c.magic.brew_flame(Some(t));
            } else {
                self.brain_brews
                    .push((t, owner, crate::magic::FLAME_BREW_SECONDS));
            }
            return;
        }
        let Some(idx) = cast_spell(kind) else {
            return;
        };
        let Some(spell) = Spell::of_index(idx as u32) else {
            return;
        };
        match (kind, rect) {
            (15, Some((x, y, w, h))) if cell.0 < 0 || cell.1 < 0 => {
                let Some(tc) = self.object_index(t).map(|k| self.cell_of(k)) else {
                    return;
                };
                let mut ships: Vec<(i32, i32, u32)> = (0..self.objects.len())
                    .filter(|&k| {
                        self.objects[k].ship.is_some() && self.objects[k].owner == Some(owner)
                    })
                    .map(|k| {
                        let (cx, cy) = self.cell_of(k);
                        (cy, cx, self.objects[k].id)
                    })
                    .filter(|&(cy, cx, _)| cx >= x && cx < x + w && cy >= y && cy < y + h)
                    .collect();
                ships.sort();
                for (cy, cx, id) in ships {
                    let far = super::sea_monster::distance((cx, cy), tc) > 15;
                    if far || !self.side_affords(owner, idx, 1, d) {
                        continue;
                    }
                    if !self.side_affords(owner, idx, 1, 0) {
                        continue;
                    }
                    self.side_pays(owner, idx, 0);
                    self.brain_cast_on(spell, id, data, caches);
                }
            }
            (12 | 15 | 16, _) if cell.0 < 0 || cell.1 < 0 => {}
            (8, _) => {}
            (12 | 15 | 16, _) => {
                if d != 100 && !self.side_affords(owner, idx, 1, d) {
                    return;
                }
                self.side_pays(owner, idx, d);
                if let Some(ship) = ship {
                    self.brain_cast_on(spell, ship, data, caches);
                }
            }
            _ => {
                if d != 100 && !self.side_affords(owner, idx, 1, d) {
                    return;
                }
                self.side_pays(owner, idx, d);
                self.brain_cast(spell, cell.0, cell.1, Some(owner), data, caches);
            }
        }
    }

    /// The brains' land fleets' passes at their casts (0x42b910, message 0
    /// every tick): a pass 50 ticks after one that started nothing; after
    /// one that sent a cast, 3 ticks after it once woken. In each pass the
    /// entries are tried in order (0x42a500) until one is sent; a waiting
    /// one's passes count down only in a pass after one that started
    /// nothing (`+0x18`), and with none left it fails (state 4). Ended
    /// entries wake the fleet.
    pub(super) fn run_brain_casts(&mut self) {
        let Some(brain) = self.brain.as_ref() else {
            return;
        };
        let now = brain.tick;
        let fleets: Vec<usize> = brain.casts.keys().copied().collect();
        for k in fleets {
            let mut q = self.brain.as_mut().unwrap().casts.remove(&k).unwrap();
            let due = if !q.started {
                now >= q.last + 50
            } else {
                q.woken && now >= q.last + 3
            };
            if due {
                q.last = now;
                let count = !q.started;
                q.started = false;
                q.woken = false;
                let members = self.fleet_towns(k);
                let mut i = 0;
                while i < q.entries.len() {
                    let (ask, tries) = q.entries[i];
                    if self.cast_entry(&members, ask, None, None) {
                        q.entries.remove(i);
                        q.started = true;
                        q.woken = true;
                        break;
                    }
                    if count {
                        if tries <= 1 {
                            q.entries.remove(i);
                            q.woken = true;
                            continue;
                        }
                        q.entries[i].1 = tries - 1;
                    }
                    i += 1;
                }
            }
            self.brain.as_mut().unwrap().casts.insert(k, q);
        }
    }

    /// The brains' Flames brewing at their war temples (0x439260: 250
    /// ticks, nothing while limed, `+0x144`), each then burning the
    /// player's weapons 1500 ticks more (player `+0xb0`).
    pub(super) fn run_brain_brews(&mut self, dt: f32, limed: &[u32]) {
        let mut done = Vec::new();
        self.brain_brews.retain_mut(|(t, owner, left)| {
            if limed.contains(t) {
                return true;
            }
            *left -= dt;
            if *left <= 0.0 {
                done.push(*owner);
                return false;
            }
            true
        });
        for owner in done {
            self.brain_flames.entry(owner).or_default().1 += crate::magic::FLAME_SECONDS;
        }
    }
}
