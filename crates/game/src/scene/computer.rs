//! The computer players of battles and single missions: the original's
//! `AIPlayerClassTy` (`ai\Plr_fsrv.cpp`, `Plr_kib.cpp`, `Plr_mail.cpp`,
//! 0x47d230-0x489990), ported. `Players.cpp` (0x522d50) makes one for every
//! computer player (side +0x1a1d 3 or 4) of a mission that is not the
//! campaign's (game +0xd80 != 1); the campaign's computer players are run
//! by the mission brains (`brain`) instead.
//!
//! A computer player has bases (its settlements and towns, up to 9) and
//! flotillas (groups of ships, each of a role: gatherers, guards, raiders,
//! ...). Each flotilla takes its turn every 25-75 ticks and runs a task: a
//! small state machine over the original's fleet orders (gather in a zone,
//! guard a zone, attack a zone, sail somewhere, found a settlement or a
//! fort). Flotillas ask for ships from want tables by difficulty; the
//! manager flotilla puts the requests and each base's building script
//! (0x5b6d90 and the others) to the base's town as orders. The commander
//! picks an enemy player, sends the warships on a grand attack when they
//! are strong enough, and founds new settlements. See docs/FORMATS.md
//! ("Computer players").
//!
//! The original rolls its own generator (the usual LCG, +0x18), seeded
//! from the clock; here it is seeded from the scene's, so every machine of
//! a multiplayer game computes the same. Where the original asks its
//! terrain helpers (`artem\_L_Ai_*.cpp`: places for towns, forts and
//! waiting zones) or the land fleet (`LFleet.cpp`: executing the town's
//! orders, placing buildings with `m_build.cpp`), this port uses simpler
//! searches of its own (see `town_place` and the others).

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;
use serde::{Deserialize, Serialize};

use super::{Order, Scene, BUILD_RADIUS, FOUND_REACH};
use crate::assets::GameData;
use crate::combat::{Rng, Upgrade};
use crate::economy::{
    self, DOCK, PORT, SCHOOL, SETTLEMENT, SHIPYARD, SMITHY, TEMPLE_OF_THE_SEA, TEMPLE_OF_WAR, TOWN,
    WORKSHOP,
};
use crate::objects::{Caches, Resource, Task};

/// A rectangle of map cells: x, y, width, height.
type Cells = (i32, i32, i32, i32);

/// Flotilla roles (`AIPlayer + 0xdfc` entries, field 0; 0x483e10).
const MANAGER: u8 = 0;
const FORTS: u8 = 1;
const AMBER: u8 = 2;
const FISH: u8 = 3;
const COMMANDER: u8 = 4;
const BUILDERS: u8 = 5;
const GUARD: u8 = 6;
const DEFENCE: u8 = 7;
const RAIDERS: u8 = 8;
const FLEET: u8 = 9;
const RESERVE: u8 = 10;
const SPARE: u8 = 11;

/// "Go back to the state before" (0x7fffffff, 0x482bc0).
const BACK: u32 = 0x7fff_ffff;

/// A fleet order's report (the navy's 0x1f5 "done").
const DONE: u32 = 0x1f5;

/// Resource classes of the original (0x190 amber, 0x191 fish, 0x192 a
/// leviathan).
const AMBER_CLASS: i32 = 400;
const FISH_CLASS: i32 = 0x191;
const WHALE_CLASS: i32 = 0x192;

/// The game's computer players and their clock.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Computers {
    /// The mission is a custom one: its computer players act.
    pub on: bool,
    /// The difficulty chosen for them (+0x1a31 by player, 0-2).
    pub level: usize,
    /// Each player's own, as Custom missions' players set them (0x53b940
    /// on Start, 0x178: AI Weak, Medium and Strong to +0x1a31 = 0, 1, 2
    /// through 0x528a20); players not listed take `level`.
    #[serde(default)]
    pub levels: Vec<((u32, u32), usize)>,
    /// The players Custom missions disabled (side +0x1a1d = 0): no
    /// computer player is made for them (0x522d50 makes one for side 3
    /// only).
    #[serde(default)]
    pub off: Vec<(u32, u32)>,
    /// Game ticks since the start (game +0xde).
    pub tick: u32,
    pub started: bool,
    pub players: Vec<Computer>,
}

/// One computer player (`AIPlayerClassTy`, 0xe68 bytes, 0x487cc0).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Computer {
    pub owner: (u32, u32),
    /// +0x20: 0 easy, 1 normal, 2 hard.
    pub difficulty: usize,
    /// +0x18: its own random generator.
    rng: Rng,
    /// +0x60 (0x15c bytes each, +0xdf8 how many).
    pub bases: Vec<Base>,
    /// +0xdfc.
    pub flotillas: Vec<Flotilla>,
    /// +0xe00: ships and buildings asked of the towns.
    requests: Vec<Request>,
    /// +0xe04: (flotilla helped, flotilla helping).
    helps: Vec<(usize, usize)>,
    /// +0xe08: the enemy player (id nation x 4 + slot), +0xe0c the hits
    /// taken from +0xe10, the player who hit last.
    pub enemy: Option<u32>,
    hits: u32,
    hitter: Option<u32>,
    /// The players its ships hold their fire on: the bits +0xe18 clears
    /// (see `Scene::update_spared`).
    #[serde(default)]
    spared: u32,
    /// +0xe14: when a fleet last got help.
    helped_at: u32,
    /// Role flotillas: +0x30 manager, +0x34 forts, +0x3c commander, +0x40
    /// the base-0 fleet, +0x38 reserve; and the last base's +0x44-+0x5c.
    manager: usize,
    forts: usize,
    reserve: usize,
    commander: usize,
    by_role: [usize; 12],
    /// Ships being built for a flotilla: (shipyard, ship type, flotilla).
    building: Vec<(u32, u8, usize)>,
    /// The town orders under way (the land fleet's +0x34, `LFleet.cpp`),
    /// by priority, highest first; +0x20 the next order's id.
    orders: Vec<TownOrder>,
    next_order: u32,
    /// The land fleet's pass state and queue (0x42b910).
    #[serde(default)]
    land: LandFleet,
    /// Objects already known, to tell new ones (message 0x5d95).
    known: Vec<u32>,
    /// Shots taken since its last turn: (target, shooter).
    attacks: Vec<(u32, u32)>,
    /// What its temples know (computers learn in 100 ticks, 0x439260).
    #[serde(default)]
    pub magic: crate::magic::Magic,
    /// Seconds of the Flame left (player `+0xa8`).
    #[serde(default)]
    pub flame: f32,
    /// Spells asked of the commander (letters 6 and 7), and when it last
    /// cast each kind (commander `+0x188`, `+0x18c`).
    #[serde(default)]
    asks: Vec<SpellAsk>,
}

/// A letter asking the commander for a spell (0x488960 for message
/// 0x5d9c; read by 0x482160).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct SpellAsk {
    /// 6: on a cell (or, with `rect`, own things in an area); 7: on a
    /// flotilla's ships.
    letter: u8,
    /// Cast now, whatever the time since the last (0x4885a0).
    urgent: bool,
    base: usize,
    cell: (i32, i32),
    rect: Option<Cells>,
    flotilla: Option<usize>,
    /// Enemies about the shooter, less one (0x486d50).
    enemies: i32,
    /// The spell kind asked, if any (5-16, as 0x47ae10's).
    kind: Option<u32>,
}

/// A base (+0x60 + i x 0x15c).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Base {
    /// +0x60: the settlement or town.
    pub site: Option<u32>,
    /// +0xf4 its state, +0xf8 started, +0xfc the step's start.
    state: u32,
    started: bool,
    at: u32,
    /// +0x100: the step of the building script, +0x194 how many times it
    /// ran to its end.
    step: usize,
    rounds: u32,
    /// +0x80 the order sent, +0x90 its progress, +0xa0 when it ended.
    order: u32,
    progress: u32,
    ended: u32,
    /// The order asked, as the land fleet's queue knows it (0x482ff0).
    #[serde(default)]
    ask: Option<Ask>,
    /// +0x104: the town's area.
    pub rect: Cells,
    /// +0x114: its cell.
    pub cell: (i32, i32),
    /// +0x11c: four waiting zones (ships, area).
    zones: [(u32, Cells); 4],
}

/// A flotilla (0x34c bytes, 0x483e10).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Flotilla {
    pub role: u8,
    pub base: usize,
    pub ships: Vec<u32>,
    /// +0x14 its period (25-75 ticks), +0x18 its last turn, +0x1c started.
    period: u32,
    last: u32,
    started: bool,
    /// +0x6c the task, +0x70 its 50 parameters (the first 12 kept).
    pub task: u8,
    p: [i32; 12],
    /// +0x20 the task's state and the ring of states before (+0x24, 0x482bc0).
    pub state: u32,
    stack: [u32; 10],
    sp: usize,
    /// +0x50 the step started, +0x54 its length, +0x58 its start.
    step: bool,
    len: u32,
    since: u32,
    /// +0x60 ask for ships this turn, +0x64 send the worn to the reserve,
    /// +0x68 when it last looked.
    wants: bool,
    worn: bool,
    worn_at: u32,
    /// +0x144: ships counted.
    count: i32,
    /// +0x158 the area ordered, +0x168 its waiting zone, +0x178 home.
    area: Cells,
    zone: Cells,
    home: Cells,
    /// +0x138: mail.
    mail: Vec<Mail>,
    /// The fleet order given and what it reports.
    order: FleetOrder,
    /// Commander's clocks: +0x188 spells at a place, +0x18c spells at a
    /// fleet, +0x190 the last settlement founded, +0x194 the last attack.
    clocks: [u32; 4],
}

/// A letter between flotillas (0xd4 bytes, `Plr_mail.cpp`).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct Mail {
    /// 1 a new task, 2 start over, 3 the zones changed, 4 the home
    /// changed, 0x65 the base is attacked.
    kind: u32,
    /// The newest is read first.
    at: u32,
    task: u8,
    p: [i32; 12],
}

/// A ship or building asked of a town (+0xe00 entries, 0x90 bytes).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Request {
    /// The ship type and how many the flotilla wants of it.
    kind: u8,
    count: i32,
    flag: i32,
    prio: i32,
    fleet: usize,
    base: usize,
    /// +0x28: the priority (prio - flag x 2500 + 22767).
    key: i32,
    /// +0x2c: 0 waiting, 1 sent, 2 done, 3 failed; +0x30 the wait before
    /// it is sent, +0x34 its time, +0x3c when it ended.
    state: u32,
    delay: u32,
    at: u32,
    ended: u32,
    #[serde(default)]
    ask: Option<Ask>,
}

/// A town order as asked (the op-1000 message, 0x482f50 / 0x482ff0): its
/// code (dword 3), priority (dwords 2 and 5), base (whose town is dword
/// 6) and flotilla (dword 7, request +0x14). The land fleet finds the
/// asker's entry by these (0x42c870 compares the message's 20 dwords).
type Ask = (u32, i32, usize, Option<usize>);

/// An order to a town (the land fleet's message 1000 and its parts,
/// 0x42c050 with the table at 0x5ac7c8).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TownOrder {
    id: u32,
    prio: i32,
    /// +8: the order's flags (message dword 4): 0x88 for the base
    /// scripts' orders, 0xd8 for their order 0x60, 0x488 for the ships
    /// asked (0x47d2e0, 0x47fe60).
    #[serde(default)]
    flags: u32,
    base: usize,
    /// For ships: the flotilla to join.
    fleet: Option<usize>,
    parts: Vec<Part>,
    /// +0x18: 0 new, 2 under way, 4 failed.
    #[serde(default)]
    state: u32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct Part {
    op: Op,
    /// Done at once if the town has it already (flag 0x20).
    once: bool,
    /// Field 8: the ship is built free (order 0x5c's part).
    #[serde(default)]
    free: bool,
    after: After,
    /// In an order, +0x58: 0 waiting, 2 queued, 3 done, 4 failed; in the
    /// queue (+0x50): 0 queued, 1 tried, 2 under way, 3 done, 4 failed.
    state: u8,
    /// Passes left (+0x64 from 0x595170 waiting, +0x68 from 0x595198
    /// under way, by operation).
    tries: u32,
    /// Buildings of the class before it was ordered (+0xdc).
    before: usize,
    /// A ship part's passes held by a -1 after a pass that started
    /// nothing (+0xe0), and whether its last result was -1 (+0x70).
    #[serde(default)]
    berths: u32,
    #[serde(default)]
    refused: bool,
}

/// The land fleet (`LFleet.cpp`, the player fleet of kind 0xb, built by
/// 0x42b370): what its passes (0x42b910) keep.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LandFleet {
    /// +0x1c: the tick of the last pass.
    last: u32,
    /// +0x14: the last pass started a part.
    started: bool,
    /// +0x28: woken, a pass may come 3 ticks after the last.
    woken: bool,
    /// +0x30: the queue, by priority (+8) highest first.
    queue: Vec<Entry>,
}

/// An entry of the land fleet's queue (0xe4 bytes, 0x42c430).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Entry {
    /// +8.
    prio: i32,
    job: Job,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum Job {
    /// Op 1000, a town order asked (origin 1, 0x4a09d0): started by
    /// 0x42c050, which puts the order (its id at +0xdc); +0x50 its state.
    Ask {
        ask: Ask,
        flags: u32,
        state: u8,
        order: Option<u32>,
    },
    /// A part of order `order` (origin 2, +0x58), the `index`th (+0x5c):
    /// the part's copy, with its own state and tries.
    Part {
        order: u32,
        index: usize,
        base: usize,
        fleet: Option<usize>,
        part: Part,
    },
}

impl Entry {
    fn state(&self) -> u8 {
        match self.job {
            Job::Ask { state, .. } => state,
            Job::Part { part, .. } => part.state,
        }
    }

    fn set_state(&mut self, s: u8) {
        match &mut self.job {
            Job::Ask { state, .. } => *state = s,
            Job::Part { part, .. } => part.state = s,
        }
    }
}

/// A ship part held this many passes by its town's -1 has the town clear
/// its berths (0x428f20 at 0x4295cb).
const BERTH_PASSES: u32 = 25;

/// The land fleet's priority threshold (+0x24, 0x7fff from 0x42b370): a
/// waiting entry at or above it stops the walk. Only a queued op-0x3f0
/// message changes it, and no code builds one (not settled: a brain's
/// data could).
const LAND_THRESHOLD: u32 = 0x7fff;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Op {
    /// 0x3e9: put up a building of the class.
    Build(u32),
    /// 0x3ea: build a ship of the type.
    Ship(u8),
    /// 0x3eb: upgrade the building of the class (a settlement to a town,
    /// a shipyard to a dock).
    Upgrade(u32),
    /// 0x3ef: a study (0x42a860), by its number.
    Study(u8),
    /// 0x3ec: repairs (0x429f60): the town mends the first of its
    /// buildings that is worn (command 0x1e).
    Repair,
    /// 0x3ed and the like, not ported.
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum After {
    /// Field 20 = 0.
    #[default]
    Nothing,
    /// 1: after part field 21.
    Part(usize),
    /// 2: after all the parts before.
    All,
    /// 3: at once, and queued again while it runs whenever no entry of
    /// its operation waits (0x42b910).
    Again,
}

impl Op {
    /// The operation's number (part dword 0).
    fn code(self) -> u32 {
        match self {
            Op::Build(_) => 0x3e9,
            Op::Ship(_) => 0x3ea,
            Op::Upgrade(_) => 0x3eb,
            Op::Repair => 0x3ec,
            Op::Other => 0x3ed,
            Op::Study(_) => 0x3ef,
        }
    }

    /// Passes a waiting part is tried (0x595170 + op x 4).
    fn tries(self) -> u32 {
        match self {
            Op::Build(_) | Op::Upgrade(_) => 20,
            Op::Ship(_) => 25,
            _ => 10,
        }
    }

    /// Passes a part under way is waited for (0x595198 + op x 4).
    fn tries_under_way(self) -> u32 {
        match self {
            Op::Ship(_) | Op::Study(_) => 25,
            Op::Repair | Op::Other => 10,
            _ => 20,
        }
    }
}

/// A fleet order and its report (the navy's orders 0x6e-0xc9).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
enum FleetOrder {
    #[default]
    None,
    /// 0x78: gather in an area.
    Gather(Cells, Resource),
    /// 0x6e: guard an area.
    Guard(Cells),
    /// 0x70: attack an area.
    Attack(Cells),
    /// 100: sail to a cell.
    Move((i32, i32)),
    /// 0x8c: found a building of the class at a cell, by a ship.
    Found(u32, (i32, i32), u32),
    /// 200: to the docks for repairs.
    Dock,
    /// 0xbe: take the treasures in an area (0x4da550), bringing them home.
    Treasure(Cells),
}

/// What a want table entry asks: (ship type, how many, flag, priority);
/// each group lists alternatives, the first the town can build counting.
type Want = (u8, i32, i32, i32);

impl Computer {
    /// The original's generator: the next value below `n`.
    fn roll(&mut self, n: u32) -> u32 {
        self.rng.next() % n
    }

    /// Whether `enemies` are many enough to ask for a spell (0x4886d0):
    /// at least 7-8 on easy, 6-7 on normal, 5-6 on hard.
    fn many(&mut self, enemies: i32) -> bool {
        let base = [7, 6, 5][self.difficulty.min(2)];
        (self.rng.next() & 1) as i32 + base <= enemies
    }

    /// Whether the spell is wanted at once (0x4885a0): 12-14 enemies or 1
    /// in 9 on easy, 10-12 or 1 in 8 on normal, 8-10 or 1 in 6 on hard.
    fn urgent(&mut self, enemies: i32) -> bool {
        let base = [12, 10, 8][self.difficulty.min(2)];
        if self.roll(3) as i32 + base <= enemies {
            return true;
        }
        match self.difficulty {
            0 => self.roll(9) == 1,
            1 => self.rng.next() & 7 == 1,
            _ => self.roll(6) == 2,
        }
    }

    fn id(&self) -> u32 {
        self.owner.0 * 4 + self.owner.1
    }

    fn nation(&self) -> u32 {
        self.owner.0
    }

    /// Makes a flotilla (0x483e10): its period 25-75 ticks.
    fn add_flotilla(&mut self, role: u8, base: usize) -> usize {
        let period = self.roll(0x33) + 0x19;
        self.flotillas.push(Flotilla {
            role,
            base,
            period,
            ..Default::default()
        });
        self.by_role[role as usize] = self.flotillas.len() - 1;
        self.flotillas.len() - 1
    }

    /// The flotilla of role `role` at base `base` (0x484110).
    fn flotilla_of(&self, base: usize, role: u8) -> Option<usize> {
        self.flotillas
            .iter()
            .position(|f| f.role == role && f.base == base)
    }

    /// Mails flotilla `f` (0x489500).
    fn mail(&mut self, f: usize, m: Mail) {
        if let Some(fl) = self.flotillas.get_mut(f) {
            fl.mail.push(m);
        }
    }

    /// Mails every flotilla of base `b` (0x489730).
    fn mail_base(&mut self, b: usize, m: Mail) {
        for f in &mut self.flotillas {
            if f.base == b {
                f.mail.push(m);
            }
        }
    }

    /// The want table of a role (0x483d20, tables 0x5b5fa0-0x5b9000 chosen
    /// by difficulty at 0x47f980; the Persians, player ids 12-15, have
    /// their own for some).
    fn wants_of(&self, role: u8) -> &'static [&'static [Want]] {
        let persian = self.id() & !3 == 12;
        let d = self.difficulty.min(2);
        match role {
            AMBER | FISH => GATHERERS,
            BUILDERS => [BUILDERS_0, BUILDERS_1, BUILDERS_2][d],
            GUARD => {
                if persian {
                    [GUARD_0P, GUARD_1P, GUARD_2P][d]
                } else {
                    [GUARD_0, GUARD_1, GUARD_2][d]
                }
            }
            DEFENCE => [DEFENCE_0, DEFENCE_1, DEFENCE_2][d],
            RAIDERS => {
                if persian {
                    [RAIDERS_0P, RAIDERS_1P, RAIDERS_2P][d]
                } else {
                    [RAIDERS_0, RAIDERS_1, RAIDERS_2][d]
                }
            }
            FLEET => {
                if persian {
                    [FLEET_0P, FLEET_1P, FLEET_2P][d]
                } else {
                    [FLEET_0, FLEET_1, FLEET_2][d]
                }
            }
            _ => &[],
        }
    }

    /// The building script of base `b` (0x47faf0): the first round from
    /// a table by difficulty and whether it is the first base; later
    /// rounds from 0x5b6fe0.
    fn script(&self, b: usize) -> &'static [(u32, i32)] {
        if self.bases[b].rounds > 0 {
            return SCRIPT_AGAIN;
        }
        match (self.difficulty.min(2), b == 0) {
            (0, true) => SCRIPT_0,
            (0, false) => SCRIPT_0B,
            (1, true) => SCRIPT_1,
            (1, false) => SCRIPT_1B,
            (_, true) => SCRIPT_2,
            (_, false) => SCRIPT_2B,
        }
    }

    /// Pushes state `s` on flotilla `f` (0x482bc0; `BACK` pops).
    fn push(&mut self, f: usize, s: u32) {
        let fl = &mut self.flotillas[f];
        if s == BACK {
            fl.sp = (fl.sp + 9) % 10;
        } else {
            fl.sp = (fl.sp + 1) % 10;
            fl.stack[fl.sp] = s;
        }
        fl.state = fl.stack[fl.sp];
        fl.step = false;
    }

    /// Sets flotilla `f`'s task (0x47d290).
    fn set_task(&mut self, f: usize, task: u8, p: [i32; 12]) {
        let fl = &mut self.flotillas[f];
        fl.task = task;
        fl.p = p;
        fl.sp = 0;
        fl.stack = [0; 10];
        fl.state = 0;
        fl.step = false;
    }

    /// Reads the newest letter of flotilla `f` (0x4897a0).
    fn read(&mut self, f: usize) -> Option<Mail> {
        let mail = &mut self.flotillas[f].mail;
        let mut best: Option<usize> = None;
        for (k, m) in mail.iter().enumerate() {
            if best.is_none_or(|b| mail[b].at < m.at) {
                best = Some(k);
            }
        }
        best.map(|k| mail.remove(k))
    }

    /// Drops flotilla `f`'s letters of kind `kind` (0x489820).
    fn drop_mail(&mut self, f: usize, kind: u32) {
        self.flotillas[f].mail.retain(|m| m.kind != kind);
    }

    /// The step started at `since` with length `len` is over.
    fn over(fl: &Flotilla, now: u32) -> bool {
        fl.len != 0 && fl.since + fl.len <= now
    }

    /// The task's own end: when the time `p[len]` from `p[start]` is up,
    /// go to `p[next]`: 0 to state 0, 1 a letter to start over, another
    /// task, or `BACK` nothing.
    fn task_end(&mut self, f: usize, now: u32, len: usize, start: usize, next: usize) {
        let p = self.flotillas[f].p;
        if p[len] != 0 && (p[start] + p[len]) as u32 <= now {
            self.go(f, p[next]);
        }
    }

    /// Goes to `next` (the tasks' common ending): 0 state 0, 1 a letter to
    /// start over, another task.
    fn go(&mut self, f: usize, next: i32) {
        match next {
            n if n == BACK as i32 => {}
            0 => self.push(f, 0),
            1 => self.mail(
                f,
                Mail {
                    kind: 2,
                    ..Default::default()
                },
            ),
            t => self.set_task(f, t as u8, [0; 12]),
        }
    }
}

impl Scene {
    /// Notes which players the computer runs (0x522d50): those of a
    /// custom mission that are neither the human nor a seat of a
    /// multiplayer game.
    pub(super) fn start_computers(&mut self) {
        let c = &mut self.computers;
        if !c.on || c.started {
            return;
        }
        c.started = true;
        let humans: Vec<(u32, u32)> = self
            .human
            .iter()
            .map(|h| (h.nation, h.slot))
            .chain(self.seats.iter().map(|s| (s.player.nation, s.player.slot)))
            .collect();
        let owners: Vec<(u32, u32)> = self
            .purses
            .keys()
            .copied()
            .filter(|o| !humans.contains(o) && !self.computers.off.contains(o))
            .collect();
        for owner in owners {
            let level = self
                .computers
                .levels
                .iter()
                .find(|l| l.0 == owner)
                .map_or(self.computers.level, |l| l.1)
                .min(2);
            // The original seeds from the clock (timeGetTime, message 2).
            let seed = self.rng.next() << 16 | self.rng.next();
            let mut ai = Computer {
                owner,
                difficulty: level,
                rng: Rng(seed),
                ..Default::default()
            };
            self.computer_setup(&mut ai);
            self.computers.players.push(ai);
        }
    }

    /// Runs the computer players one game tick (message 0 to each).
    pub(super) fn run_computers(&mut self, data: &GameData, caches: &mut Caches) {
        if !self.computers.on {
            return;
        }
        self.start_computers();
        self.computers.tick += 1;
        for k in 0..self.computers.players.len() {
            let mut ai = std::mem::take(&mut self.computers.players[k]);
            self.computer_tick(&mut ai, data, caches);
            self.computers.players[k] = ai;
        }
    }

    /// A ship launched at `site`: a computer player's flotilla that asked
    /// for it takes it.
    pub(super) fn computer_launched(&mut self, site: u32, kind: u8, id: u32) {
        for ai in &mut self.computers.players {
            if let Some(k) = ai.building.iter().position(|b| b.0 == site && b.1 == kind) {
                let (_, _, f) = ai.building.remove(k);
                if let Some(fl) = ai.flotillas.get_mut(f) {
                    fl.ships.push(id);
                }
                ai.known.push(id);
                return;
            }
        }
    }

    /// Notes that `shooter` fired on `target` (message 0x5d9c to the
    /// target's player).
    pub(super) fn computer_attacked(&mut self, target: u32, shooter: u32) {
        let Some(owner) = self
            .object_index(target)
            .and_then(|i| self.objects[i].owner)
        else {
            return;
        };
        if let Some(ai) = self.computers.players.iter_mut().find(|a| a.owner == owner) {
            if ai.attacks.len() < 64 {
                ai.attacks.push((target, shooter));
            }
        }
    }

    fn now(&self) -> u32 {
        self.computers.tick
    }

    /// The computer player's objects, in the objects' order (0x4a8620).
    fn owned(&self, owner: (u32, u32)) -> Vec<usize> {
        (0..self.objects.len())
            .filter(|&i| self.objects[i].owner == Some(owner) && self.objects[i].combat.is_some())
            .collect()
    }

    fn class_of(&self, i: usize) -> Option<u32> {
        match self.objects[i].class {
            ObjectClass::Building(c) | ObjectClass::Barbarian(c) => Some(c),
            _ => None,
        }
    }

    /// The cell of object `i` (its top-left for a building).
    pub(super) fn cell_of(&self, i: usize) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        self.objects[i].cell.unwrap_or((
            (self.objects[i].x / t) as i32,
            (self.objects[i].y / t) as i32,
        ))
    }

    fn map_cells(&self) -> (i32, i32) {
        (self.nav.width as i32, self.nav.height as i32)
    }

    /// Sets up a computer player (message 0x8d1, 0x487230).
    fn computer_setup(&mut self, ai: &mut Computer) {
        ai.manager = ai.add_flotilla(MANAGER, 0);
        let forts = ai.add_flotilla(FORTS, 0);
        ai.forts = forts;
        let mine = self.owned(ai.owner);
        // Towns first, then settlements, become bases; forts join the
        // forts' flotilla.
        let mut sites = Vec::new();
        for &i in &mine {
            match self.class_of(i) {
                Some(TOWN) => sites.push(i),
                Some(0x34..=0x36) => ai.flotillas[forts].ships.push(self.objects[i].id),
                _ => {}
            }
        }
        for &i in &mine {
            if self.class_of(i) == Some(SETTLEMENT) {
                sites.push(i);
            }
        }
        let bases = sites.len().clamp(1, 9);
        ai.commander = ai.add_flotilla(COMMANDER, 0);
        ai.add_flotilla(FLEET, 0);
        ai.reserve = ai.add_flotilla(RESERVE, 0);
        for b in 0..bases {
            let mut base = Base {
                cell: (-1, -1),
                ..Default::default()
            };
            if let Some(&i) = sites.get(b) {
                base.site = Some(self.objects[i].id);
                base.cell = self.cell_of(i);
            }
            ai.bases.push(base);
            if ai.bases[b].site.is_some() {
                self.base_zones(ai, b);
                self.base_rect(ai, b);
            }
            for role in [AMBER, FISH, GUARD, DEFENCE, RAIDERS, BUILDERS, SPARE] {
                ai.add_flotilla(role, b);
            }
        }
        // The ships it starts with go where a flotilla wants their type
        // (0x4803a0, 0x480510), else by type (0x484170).
        let mut asked: Vec<(u8, usize, i32)> = Vec::new();
        for f in 0..ai.flotillas.len() {
            for group in ai.wants_of(ai.flotillas[f].role) {
                let (kind, _, flag, prio) = group[0];
                asked.push((kind, f, prio - flag * 2500 + 22767));
                let _ = ai.roll(0x27);
            }
        }
        // By priority, highest first (0x480480).
        asked.sort_by_key(|a| -a.2);
        for &i in &mine {
            let Some(kind) = self.objects[i].ship.as_ref().map(|s| s.kind) else {
                continue;
            };
            let id = self.objects[i].id;
            let f = match asked.iter().position(|a| a.0 == kind) {
                Some(k) => Some(asked.remove(k).1),
                None => ai.fleet_for_type(kind),
            };
            if let Some(f) = f {
                ai.flotillas[f].ships.push(id);
            }
        }
        ai.known = mine.iter().map(|&i| self.objects[i].id).collect();
        self.choose_enemy(ai);
    }

    /// One tick of a computer player (message 0, 0x488960).
    fn computer_tick(&mut self, ai: &mut Computer, data: &GameData, caches: &mut Caches) {
        let now = self.now();
        // Ships sunk or taken leave their flotillas.
        let owner = ai.owner;
        for f in &mut ai.flotillas {
            f.ships.retain(|&id| {
                self.object_index(id)
                    .is_some_and(|i| self.objects[i].owner == Some(owner))
            });
        }
        self.computer_news(ai);
        for (target, shooter) in std::mem::take(&mut ai.attacks) {
            self.computer_hit(ai, target, shooter);
        }
        // Nothing left: the original stops sending it message 0.
        if self.owned(owner).is_empty() {
            return;
        }
        for f in 0..ai.flotillas.len() {
            self.flotilla_turn(ai, f, now);
        }
        self.land_fleet(ai, now, data, caches);
    }

    /// New and lost objects (messages 0x5d95, 0x5d97 and 0x5d9b).
    fn computer_news(&mut self, ai: &mut Computer) {
        // A settlement lost: its base waits to be founded again (0x5d9b).
        for b in 0..ai.bases.len() {
            if let Some(site) = ai.bases[b].site {
                let ours = self.object_index(site).is_some_and(|i| {
                    self.objects[i].owner == Some(ai.owner)
                        && matches!(self.class_of(i), Some(SETTLEMENT | TOWN))
                });
                if !ours {
                    ai.bases[b].site = None;
                    ai.bases[b].state = 0;
                    ai.bases[b].started = false;
                }
            }
        }
        let mine = self.owned(ai.owner);
        let new: Vec<usize> = mine
            .iter()
            .copied()
            .filter(|&i| !ai.known.contains(&self.objects[i].id))
            .collect();
        ai.known = mine.iter().map(|&i| self.objects[i].id).collect();
        for i in new {
            let id = self.objects[i].id;
            if let Some(kind) = self.objects[i].ship.as_ref().map(|s| s.kind) {
                // A ship nobody ordered (0x5d97): to the reserve if some
                // flotilla wants its type, else to the spare flotilla.
                if ai.flotillas.iter().any(|f| f.ships.contains(&id)) {
                    continue;
                }
                let f = if ai.nobody_wants(kind) {
                    ai.by_role[SPARE as usize]
                } else {
                    ai.reserve
                };
                ai.flotillas[f].ships.push(id);
                continue;
            }
            let Some(class) = self.class_of(i) else {
                continue;
            };
            match class {
                // A new settlement (0x5d95 case 0).
                SETTLEMENT | TOWN => self.new_base(ai, i),
                0x34..=0x36 => ai.flotillas[ai.forts].ships.push(id),
                _ => {
                    // A building of a base: its zones and area again.
                    if let Some(b) = self.base_near(ai, i) {
                        let now = self.now();
                        if matches!(class, PORT | SHIPYARD | DOCK) {
                            self.base_zones(ai, b);
                            ai.mail_base(
                                b,
                                Mail {
                                    kind: 3,
                                    at: now,
                                    ..Default::default()
                                },
                            );
                        }
                        self.base_rect(ai, b);
                        ai.mail_base(
                            b,
                            Mail {
                                kind: 4,
                                at: now,
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }
    }

    /// The base whose town building `i` belongs to.
    fn base_near(&self, ai: &Computer, i: usize) -> Option<usize> {
        let (x, y) = self.cell_of(i);
        ai.bases.iter().position(|b| {
            b.site.is_some()
                && (x - b.cell.0).abs() <= BUILD_RADIUS + 2
                && (y - b.cell.1).abs() <= BUILD_RADIUS + 2
        })
    }

    /// A settlement founded or taken (0x5d95 case 0): the base waiting at
    /// its cell gets it, else it makes a new base with its flotillas.
    fn new_base(&mut self, ai: &mut Computer, i: usize) {
        let id = self.objects[i].id;
        if ai.bases.iter().any(|b| b.site == Some(id)) {
            return;
        }
        let cell = self.cell_of(i);
        let now = self.now();
        let near = |b: &Base| (b.cell.0 - cell.0).abs() <= 1 && (b.cell.1 - cell.1).abs() <= 1;
        let b = match ai.bases.iter().position(|b| b.site.is_none() && near(b)) {
            Some(b) => {
                let base = &mut ai.bases[b];
                base.site = Some(id);
                base.cell = cell;
                base.step = 0;
                base.state = 0;
                base.started = false;
                b
            }
            None => {
                if ai.bases.len() >= 9 {
                    return;
                }
                ai.bases.push(Base {
                    site: Some(id),
                    cell,
                    ..Default::default()
                });
                let b = ai.bases.len() - 1;
                for role in [AMBER, FISH, GUARD, DEFENCE, RAIDERS, BUILDERS, SPARE] {
                    ai.add_flotilla(role, b);
                }
                b
            }
        };
        self.base_zones(ai, b);
        self.base_rect(ai, b);
        for kind in [3, 4] {
            ai.mail_base(
                b,
                Mail {
                    kind,
                    at: now,
                    ..Default::default()
                },
            );
        }
    }

    /// Picks the enemy (0x4844b0): any other player, the humans weighted
    /// 250 (200 a remote one) and computers 10.
    fn choose_enemy(&mut self, ai: &mut Computer) {
        let mut sides: Vec<(i32, u32)> = Vec::new();
        if let Some(h) = self.human {
            sides.push((250, h.nation * 4 + h.slot));
        }
        for s in self.seats.iter().skip(1) {
            sides.push((200, s.player.nation * 4 + s.player.slot));
        }
        for o in self.purses.keys() {
            let id = o.0 * 4 + o.1;
            if !sides.iter().any(|s| s.1 == id) {
                sides.push((10, id));
            }
        }
        sides.retain(|s| s.1 != ai.id());
        sides.sort_by_key(|s| s.1);
        if sides.is_empty() {
            ai.enemy = None;
            return;
        }
        let weights: Vec<i32> = sides.iter().map(|s| s.0).collect();
        let k = ai.pick(&weights);
        ai.hits = 0;
        ai.enemy = Some(sides[k].1);
        ai.hitter = ai.enemy;
        self.update_spared(ai);
    }

    /// Whether player `id` has anything left (0x484770).
    fn alive(&self, id: u32) -> bool {
        let owner = (id / 4, id % 4);
        self.objects
            .iter()
            .any(|o| o.owner == Some(owner) && o.combat.is_some())
    }

    fn is_human_id(&self, id: u32) -> bool {
        self.human.is_some_and(|h| h.nation * 4 + h.slot == id)
            || self
                .seats
                .iter()
                .any(|s| s.player.nation * 4 + s.player.slot == id)
    }

    /// A shot at one of the computer's objects (message 0x5d9c).
    fn computer_hit(&mut self, ai: &mut Computer, target: u32, shooter: u32) {
        let now = self.now();
        // At most one answer every 50-100 ticks.
        let wait = ai.roll(0x33) + 0x32;
        if wait + ai.helped_at > now {
            return;
        }
        ai.helped_at = now;
        let (Some(t), Some(s)) = (self.object_index(target), self.object_index(shooter)) else {
            return;
        };
        self.note_hitter(ai, s);
        if self.objects[t].ship.is_some() {
            // A fleet under fire weaker than the enemy about gets help from
            // another (0x486b70, 0x486e80).
            let Some(f) = ai.flotillas.iter().position(|f| f.ships.contains(&target)) else {
                return;
            };
            if ai.helps.iter().any(|h| h.0 == f || h.1 == f) {
                return;
            }
            let Some((rect, strength)) = self.enemy_group(ai, s) else {
                return;
            };
            let Some(helper) = self.find_helper(ai, f, strength) else {
                return;
            };
            let len = ai.roll(0x2329) as i32 + 9000;
            let mut p = [0; 12];
            p[0] = f as i32;
            p[1] = rect.0;
            p[2] = rect.1;
            p[3] = rect.2;
            p[4] = rect.3;
            p[7] = len;
            p[8] = 1;
            p[9] = 1;
            ai.mail(
                helper,
                Mail {
                    kind: 1,
                    at: now,
                    task: 8,
                    p,
                },
            );
            ai.helps.push((f, helper));
            ai.helps.push((helper, helper));
        } else if matches!(self.class_of(t), Some(SETTLEMENT | TOWN)) {
            // The town is attacked: its flotillas defend it.
            if let Some(b) = ai.bases.iter().position(|b| b.site == Some(target)) {
                ai.mail_base(
                    b,
                    Mail {
                        kind: 0x65,
                        at: now,
                        ..Default::default()
                    },
                );
                self.ask_spells(ai, b, s);
            }
        }
    }

    /// The town of base `b` under fire from `s` asks the commander for
    /// spells (0x488960), each if the enemies about the shooter are many
    /// enough (0x4886d0): one at the shooter, the Flame for the forts, the
    /// weapon shield over the base, and one on the guards.
    fn ask_spells(&mut self, ai: &mut Computer, b: usize, s: usize) {
        let cell = self.cell_of(s);
        let enemies = self.enemies_about(ai.owner, cell);
        let ask = |ai: &mut Computer, letter, rect, flotilla, kind| {
            let urgent = ai.urgent(enemies);
            ai.asks.push(SpellAsk {
                letter,
                urgent,
                base: b,
                cell,
                rect,
                flotilla,
                enemies,
                kind,
            });
        };
        if ai.many(enemies) {
            ask(ai, 6, None, None, None);
        }
        if ai.many(enemies) && !ai.flotillas[ai.forts].ships.is_empty() {
            let forts = ai.forts;
            ask(ai, 7, None, Some(forts), Some(9));
        }
        if ai.many(enemies) {
            let area = ai.bases[b].rect;
            ask(ai, 6, Some(area), None, Some(15));
        }
        if ai.many(enemies) {
            if let Some(g) = ai
                .flotillas
                .iter()
                .position(|f| f.role == GUARD && f.base == b)
            {
                ask(ai, 7, None, Some(g), None);
            }
        }
    }

    /// Ships and buildings of others in the 5 x 5 cells about `cell`, less
    /// one (0x486d50).
    fn enemies_about(&self, owner: (u32, u32), (cx, cy): (i32, i32)) -> i32 {
        let (w, h) = self.map_cells();
        let (x0, y0) = ((cx - 2).max(0), (cy - 2).max(0));
        let (x1, y1) = ((x0 + 5).min(w), (y0 + 5).min(h));
        let n = (0..self.objects.len())
            .filter(|&i| {
                let o = &self.objects[i];
                let (x, y) = self.cell_of(i);
                o.owner.is_some_and(|w| w != owner)
                    && (o.ship.is_some() || matches!(o.class, ObjectClass::Building(_)))
                    && x >= x0
                    && x < x1
                    && y >= y0
                    && y < y1
            })
            .count();
        n as i32 - 1
    }

    /// Who keeps hitting it becomes the enemy (0x4845e0).
    fn note_hitter(&mut self, ai: &mut Computer, s: usize) {
        let Some(owner) = self.objects[s].owner else {
            return;
        };
        if owner.0 > 4 {
            return;
        }
        let who = owner.0 * 4 + owner.1;
        if Some(who) == ai.enemy || who == ai.id() {
            return;
        }
        if ai.hitter != Some(who) {
            ai.hitter = Some(who);
            ai.hits = 1;
            return;
        }
        ai.hits += 1;
        let enemy_human = ai.enemy.is_some_and(|e| self.is_human_id(e));
        let limit = if self.is_human_id(who) {
            if enemy_human {
                ai.roll(8) + 8
            } else {
                ai.roll(3) + 1
            }
        } else if enemy_human {
            ai.roll(0x33) + 100
        } else {
            ai.roll(8) + 8
        };
        if ai.hits <= limit {
            return;
        }
        ai.hits = 0;
        ai.enemy = Some(who);
        ai.hitter = Some(who);
        self.update_spared(ai);
    }

    /// The players a computer's ships spare (0x4847d0, on the commander's
    /// first turn and with each new enemy): while its enemy is on the
    /// humans' side, the other computer players that are after the same
    /// enemy, neither of them on easy. The mask (+0xe18) goes to every
    /// fleet with its behaviour (order 0x72, 0x482eb0: fleet +0x19), and
    /// on to the ships (+0x689), which only pick targets whose owner's bit
    /// is set (0x4bfc60).
    fn update_spared(&self, ai: &mut Computer) {
        ai.spared = 0;
        if ai.difficulty == 0 || !ai.enemy.is_some_and(|e| self.is_human_id(e)) {
            return;
        }
        for other in &self.computers.players {
            if other.owner != ai.owner && other.difficulty > 0 && other.enemy == ai.enemy {
                ai.spared |= 1 << other.id();
            }
        }
    }

    /// Whether ship `me` holds its fire on `o` (see `update_spared`).
    pub(super) fn holds_fire(
        &self,
        me: &crate::objects::MapObject,
        o: &crate::objects::MapObject,
    ) -> bool {
        let (Some(mine), Some(theirs)) = (me.owner, o.owner) else {
            return false;
        };
        me.ship.is_some()
            && theirs.0 <= 4
            && self
                .computers
                .players
                .iter()
                .any(|c| c.owner == mine && c.spared & 1 << (theirs.0 * 4 + theirs.1) != 0)
    }

    /// The enemies about shooter `s` (0x486b70): for a ship, fort or
    /// barbarian structure, the first group `LAiFindEnemiesGroups` lists
    /// (0x408b70 with its id: its own ship group, or for a building the
    /// map's first enemy ship group if any), 2 cells wider all round, and
    /// the enemy strength there; nothing for others (no help is asked).
    fn enemy_group(&self, ai: &Computer, s: usize) -> Option<(Cells, i32)> {
        let (w, h) = self.map_cells();
        if !matches!(
            self.objects[s].class,
            ObjectClass::Ship | ObjectClass::Building(0x34..=0x36) | ObjectClass::Barbarian(_)
        ) {
            return None;
        }
        let r = *self
            .enemy_groups(ai.owner, Some(self.objects[s].id))
            .first()?;
        let (x, y) = ((r.0 - 2).max(0), (r.1 - 2).max(0));
        let mut gw = r.2 + 4;
        if x + gw >= w {
            gw = w - x;
        }
        let mut gh = r.3 + 4;
        if y + gh >= h {
            gh = h - y;
        }
        let rect = (x, y, gw, gh);
        Some((rect, self.strength_in(ai.owner, rect)))
    }

    /// `LAiFindEnemiesGroups` (0x408830, mode 1, the whole map): the
    /// others' ships (at their cell) and forts and barbarian structures
    /// (each cell they cover), grouped apart within 4 (0x559070), ship
    /// groups first; with `only`, in each list just the group holding it
    /// when there. Their extents.
    fn enemy_groups(&self, owner: (u32, u32), only: Option<u32>) -> Vec<Cells> {
        let mut ships = Vec::new();
        let mut forts = Vec::new();
        for (i, o) in self.objects.iter().enumerate() {
            let foe = o.owner.is_some_and(|w| w != owner)
                || (o.owner.is_none() && matches!(o.class, ObjectClass::Barbarian(_)));
            if !foe || o.combat.as_ref().is_some_and(|c| c.hull <= 0.0) {
                continue;
            }
            match o.class {
                ObjectClass::Ship => ships.push((self.cell_of(i), o.id)),
                ObjectClass::Building(0x34..=0x36) | ObjectClass::Barbarian(_) => {
                    let (x, y) = self.cell_of(i);
                    let n = super::trade::building_cells(o);
                    for dy in 0..n {
                        for dx in 0..n {
                            forts.push(((x + dx, y + dy), o.id));
                        }
                    }
                }
                _ => {}
            }
        }
        // The scan goes row by row.
        ships.sort_by_key(|&((x, y), _)| (y, x));
        forts.sort_by_key(|&((x, y), _)| (y, x));
        let mut out = Vec::new();
        for list in [ships, forts] {
            let cells: Vec<(i32, i32)> = list.iter().map(|e| e.0).collect();
            let (labels, n) = cluster(&cells, 4);
            let target = only.and_then(|id| list.iter().position(|e| e.1 == id).map(|k| labels[k]));
            for g in 1..=n {
                if target.is_some_and(|t| t != g) {
                    continue;
                }
                let mut rect: Option<(i32, i32, i32, i32)> = None;
                for (k, &(x, y)) in cells.iter().enumerate() {
                    if labels[k] != g {
                        continue;
                    }
                    rect = Some(match rect {
                        None => (x, y, x, y),
                        Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
                    });
                }
                if let Some((a, b, c, d)) = rect {
                    out.push((a, b, c - a + 1, d - b + 1));
                }
            }
        }
        out
    }

    /// The enemy's buildings to strike when no town will do (0x485fc0,
    /// 0x487690): of classes 0x32-0x3e, 7 x 7 about each, weight 10, ten
    /// at most.
    fn enemy_buildings(&self, enemy: u32) -> Vec<(i32, Cells)> {
        let (w, h) = self.map_cells();
        (0..self.objects.len())
            .filter(|&i| {
                self.objects[i].owner == Some((enemy / 4, enemy % 4))
                    && matches!(self.objects[i].class, ObjectClass::Building(0x32..=0x3e))
            })
            .take(10)
            .map(|i| {
                let (x, y) = self.cell_of(i);
                let (x0, y0) = ((x - 3).max(0), (y - 3).max(0));
                (10, (x0, y0, 7.min(w - x0), 7.min(h - y0)))
            })
            .collect()
    }

    /// Foes and creatures in an area (the count 0x4a2390 can return).
    fn foes_in(&self, owner: (u32, u32), rect: Cells) -> usize {
        self.foes_at(owner, rect).count()
    }

    fn foes_at(&self, owner: (u32, u32), (x, y, w, h): Cells) -> impl Iterator<Item = usize> + '_ {
        (0..self.objects.len()).filter(move |&i| {
            let o = &self.objects[i];
            let (cx, cy) = self.cell_of(i);
            let foe = match o.owner {
                Some(w) => w != owner,
                None => matches!(o.class, ObjectClass::Monster(_)),
            };
            foe && o.combat.is_some() && cx >= x && cx < x + w && cy >= y && cy < y + h
        })
    }

    /// Enemy strength in an area (0x4a2390, every class): the strength
    /// (0x50ea70) of each foe or creature there, less one.
    fn strength_in(&self, owner: (u32, u32), rect: Cells) -> i32 {
        self.foes_at(owner, rect)
            .map(|i| self.strength(i))
            .sum::<i32>()
            - 1
    }

    /// A flotilla's strength (0x482d80, 0x4a2290): its ships' strengths
    /// (0x50ea70); -1 if empty.
    fn fleet_strength(&self, f: &Flotilla) -> i32 {
        if f.ships.is_empty() {
            return -1;
        }
        f.ships
            .iter()
            .filter_map(|&id| self.object_index(id))
            .map(|i| self.strength(i))
            .sum()
    }

    /// An object's strength as the computer players weigh it (0x50ea70).
    /// Ships: boarding strength (`+0x53c`, with the crew's experience,
    /// none here) / 10, plus the armour's (`+0x534`: level 1 40, 2 70; the
    /// ram's `+0x538` tests the armour level for 4 or 5, so it is always
    /// 0, 0x4bee60) plus the fire power (`+0x479`: each item's A x 15 x
    /// count x mean condition / 100, summed / 100, 0x4bc360). Forts: their
    /// units' fire (0x446710, x 1.5 under the Flame) / 10. Nests: 20 x item
    /// 4's level-1 blow (0x5c73dc); sea serpents item 4's level-0 / 10
    /// (0x5c75fc), Medusa 8800, the sea monster 5000; others none.
    pub(super) fn strength(&self, i: usize) -> i32 {
        use crate::object_info::cache;
        let o = &self.objects[i];
        let levels = self.levels_of(o.owner);
        if let Some(s) = o.ship.as_ref() {
            let units = o.unit_conditions();
            let fire: u32 = units
                .iter()
                .enumerate()
                .filter(|(_, u)| !u.is_empty())
                .map(|(k, u)| {
                    let n = u.len() as u32;
                    let mean = u.iter().map(|&c| c as u32).sum::<u32>() / n;
                    cache(k as u8, levels.arms_for(k as u8)) * n * mean / 100
                })
                .sum::<u32>()
                / 100;
            let armour = match s.armour {
                1 => 40,
                2 => 70,
                _ => 0,
            };
            return (self.boarding_strength(i) / 10 + armour + fire) as i32;
        }
        match o.class {
            ObjectClass::Building(0x34..=0x36) => {
                let flame = self.flame_of(o.owner) > 0.0;
                let places = o.fort_places();
                let sum: u32 = (0..9u8)
                    .map(|k| {
                        let a = cache(k, levels.arms_for(k));
                        let p: u32 = places[k as usize].iter().map(|&c| c as u32 * a / 100).sum();
                        if flame {
                            p * 150 / 100
                        } else {
                            p
                        }
                    })
                    .sum();
                (sum / 10) as i32
            }
            ObjectClass::HarpyNest => cache(4, 1) as i32 * 20,
            ObjectClass::Monster(0x1f4) => cache(4, 0) as i32 / 10,
            ObjectClass::Monster(0x1fa) => 0x2260,
            ObjectClass::Monster(0x1fb) => 5000,
            _ => 0,
        }
    }

    /// Asks each other flotilla whether it can help fleet `f` against
    /// `strength` (message 100), and picks one by weight: 110, +90 strong
    /// enough, -90 of another base (0x486e80).
    fn find_helper(&mut self, ai: &mut Computer, f: usize, strength: i32) -> Option<usize> {
        let own = self.fleet_strength(&ai.flotillas[f]);
        if own < 0 || own > strength {
            return None;
        }
        let mut found: Vec<(i32, usize)> = Vec::new();
        for k in 0..ai.flotillas.len() {
            if k == f {
                continue;
            }
            let Some(reply) = self.can_help(ai, k, strength) else {
                continue;
            };
            let mut w = 0x6e;
            if ai.flotillas[k].base != ai.flotillas[f].base {
                w -= 90;
            }
            if reply >= 1 {
                w += 90;
            }
            found.push((w, k));
        }
        if found.is_empty() {
            return None;
        }
        let weights: Vec<i32> = found.iter().map(|x| x.0).collect();
        Some(found[ai.pick(&weights)].1)
    }

    /// Message 100: 1 if flotilla `k` is free and as strong as `strength`,
    /// -1 if free but weaker, `None` if it cannot help.
    fn can_help(&self, ai: &Computer, k: usize, strength: i32) -> Option<i32> {
        let fl = &ai.flotillas[k];
        let free = match fl.role {
            GUARD => fl.task == 7 && fl.ships.len() > 3,
            DEFENCE => fl.task == 8 && fl.p[0] < 0,
            RAIDERS => fl.task == 9 && fl.state != 0xc,
            FLEET => fl.task == 10 && fl.state != 0xc,
            _ => false,
        };
        if !free {
            return None;
        }
        let s = self.fleet_strength(fl);
        (s > 0).then_some(if s < strength { -1 } else { 1 })
    }

    /// One flotilla's turn (0x4829d0 by role).
    fn flotilla_turn(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let role = ai.flotillas[f].role;
        if role == FORTS {
            return;
        }
        if role == COMMANDER {
            self.commander_turn(ai, f, now);
            return;
        }
        if !ai.flotillas[f].started {
            ai.flotillas[f].started = true;
            self.default_task(ai, f);
            return;
        }
        {
            let fl = &mut ai.flotillas[f];
            if fl.last + fl.period > now {
                return;
            }
            fl.last = now;
        }
        if matches!(role, BUILDERS | GUARD | DEFENCE | RAIDERS | FLEET) {
            ai.flotillas[f].wants = true;
            ai.flotillas[f].worn = false;
        }
        // The letters, newest first, until one is acted on.
        while let Some(m) = ai.read(f) {
            let acted = self.letter(ai, f, m);
            ai.drop_mail(f, m.kind);
            if acted {
                break;
            }
        }
        self.ai_run_task(ai, f, now);
        match role {
            AMBER | FISH => self.ai_ask_ships(ai, f, now),
            BUILDERS | GUARD | DEFENCE | RAIDERS | FLEET => {
                if ai.flotillas[f].wants {
                    self.ai_ask_ships(ai, f, now);
                }
                if ai.flotillas[f].worn {
                    self.send_worn(ai, f, now);
                }
            }
            _ => {}
        }
    }

    /// The task a flotilla starts with (on its first turn and after a
    /// letter to start over).
    fn default_task(&mut self, ai: &mut Computer, f: usize) {
        let mut p = [0; 12];
        let task = match ai.flotillas[f].role {
            MANAGER => 2,
            AMBER => {
                p[0] = AMBER_CLASS;
                p[1] = AMBER_CLASS;
                p[2] = 7500;
                4
            }
            FISH => {
                p[0] = FISH_CLASS;
                p[1] = FISH_CLASS;
                p[2] = 15000;
                4
            }
            BUILDERS => {
                p[7] = 1;
                6
            }
            GUARD => 7,
            DEFENCE => {
                p[0] = -1;
                p[7] = ai.roll(0x2329) as i32 + 9000;
                8
            }
            RAIDERS => {
                p[1] = ai.roll(0x2329) as i32 + 6000;
                p[2] = ai.roll(0x2329) as i32 + 6000;
                p[3] = self.target_size(ai, f, false) as i32;
                p[4] = 3;
                9
            }
            FLEET => {
                p[2] = ai.roll(0x2329) as i32 + 6000;
                p[3] = self.target_size(ai, f, true) as i32;
                p[4] = 3;
                10
            }
            RESERVE => 0xb,
            SPARE => 0xd,
            _ => return,
        };
        ai.set_task(f, task, p);
    }

    /// Acts on a letter; false if the flotilla does not know its kind (it
    /// is dropped and the next one read).
    fn letter(&mut self, ai: &mut Computer, f: usize, m: Mail) -> bool {
        let role = ai.flotillas[f].role;
        let known: &[u32] = match role {
            MANAGER => &[1],
            AMBER | FISH => &[1, 2, 3],
            RESERVE | SPARE => &[1, 2],
            _ => &[1, 2, 3, 4, 0x65],
        };
        if !known.contains(&m.kind) {
            return false;
        }
        match m.kind {
            1 => {
                let ok = match role {
                    MANAGER => m.task == 2,
                    AMBER | FISH => m.task == 0xc || m.task == 4,
                    BUILDERS | RESERVE | SPARE => m.task == 0xc,
                    _ => m.task == 0xc || m.task == 8,
                };
                if !ok {
                    return true;
                }
                if m.task == 4 {
                    // A gatherer told to gather again starts over.
                    ai.flotillas[f].started = false;
                } else {
                    ai.set_task(f, m.task, m.p);
                }
                true
            }
            2 => {
                if role == BUILDERS {
                    // The builders take turns founding forts and exploring.
                    let mut p = [0; 12];
                    if ai.flotillas[f].task == 5 {
                        p[3..8].fill(1);
                        ai.set_task(f, 6, p);
                    } else {
                        p[7] = 1;
                        ai.set_task(f, 5, p);
                    }
                } else {
                    ai.flotillas[f].started = false;
                }
                true
            }
            3 => {
                self.read_zone(ai, f);
                if matches!(role, BUILDERS | GUARD | DEFENCE | RAIDERS | FLEET)
                    && ai.flotillas[f].state == 5
                {
                    ai.push(f, 5);
                }
                true
            }
            4 => {
                self.read_home(ai, f);
                true
            }
            0x65 => {
                // The base is attacked: defend its area (task 8).
                let fl = &ai.flotillas[f];
                let mine = match role {
                    GUARD => 7,
                    DEFENCE => 8,
                    RAIDERS => 9,
                    FLEET => 10,
                    _ => return true,
                };
                if fl.task != mine && fl.task != 8 {
                    return true;
                }
                if fl.task == 8 {
                    if fl.p[0] == ai.manager as i32 {
                        return true;
                    }
                    ai.helps.retain(|h| h.1 != f);
                }
                let b = ai.flotillas[f].base;
                let r = ai.bases[b].rect;
                let mut p = [0; 12];
                p[0] = ai.manager as i32;
                p[1] = r.0;
                p[2] = r.1;
                p[3] = r.2;
                p[4] = r.3;
                p[7] = ai.roll(0x2329) as i32 + 9000;
                p[8] = 1;
                p[9] = 1;
                ai.set_task(f, 8, p);
                true
            }
            _ => false,
        }
    }

    /// Runs flotilla `f`'s task (0x47f8c0).
    fn ai_run_task(&mut self, ai: &mut Computer, f: usize, now: u32) {
        match ai.flotillas[f].task {
            2 => self.task_manage(ai, f, now),
            4 => self.task_gather(ai, f, now),
            5 => self.task_explore(ai, f, now),
            6 => self.task_fort(ai, f, now),
            7 => self.task_guard(ai, f, now),
            8 => self.task_defend(ai, f, now),
            9 | 10 => self.task_raid(ai, f, now),
            0xb | 0xd => self.task_rest(ai, f, now),
            0xc => self.task_found(ai, f, now),
            _ => {}
        }
    }

    /// Task 2 (0x47d2e0), the manager's: each base's building script and
    /// founding, then the ships asked for.
    fn task_manage(&mut self, ai: &mut Computer, f: usize, now: u32) {
        match ai.flotillas[f].state {
            0 => {
                ai.flotillas[f].p[4] = now as i32;
                ai.push(f, 5);
            }
            5 => {
                if !ai.flotillas[f].step {
                    ai.flotillas[f].step = true;
                } else {
                    for b in 0..ai.bases.len() {
                        self.manage_base(ai, b, now);
                    }
                }
            }
            _ => {}
        }
        self.serve_requests(ai, now);
    }

    /// One base of the manager's task.
    fn manage_base(&mut self, ai: &mut Computer, b: usize, now: u32) {
        let hard = ai.difficulty == 2;
        if ai.bases[b].site.is_some() {
            let base = &ai.bases[b];
            if base.state == 5 {
                if base.started {
                    let wait = if hard { 0 } else { 1500 };
                    if now > base.ended + wait {
                        ai.bases[b].state = 0;
                        ai.bases[b].started = false;
                    }
                } else {
                    ai.bases[b].started = true;
                    self.base_zones(ai, b);
                    ai.mail_base(
                        b,
                        Mail {
                            kind: 3,
                            at: now,
                            ..Default::default()
                        },
                    );
                    ai.bases[b].step += 1;
                    ai.bases[b].ended = now;
                }
                return;
            }
            if base.state != 0 {
                return;
            }
            if base.started {
                match base.progress {
                    0 => ai.bases[b].progress = if base.order == 0x60 { 2 } else { 1 },
                    1 => {
                        // Its entry's state (0x482ff0); none (5) is not
                        // looked at (0x47d2e0).
                        let result = ai.ask_state(base.ask);
                        if result == 3 || result == 4 {
                            ai.bases[b].progress = result - 1;
                            ai.bases[b].ended = now;
                        }
                        // Without its port, shipyard, sage school and
                        // smithy the script starts again.
                        let order = ai.script(b).get(ai.bases[b].step).map_or(0, |e| e.0);
                        if order != 2 && !self.has_basics(ai, b) {
                            ai.bases[b].step = 0;
                            ai.bases[b].state = 0;
                            ai.bases[b].started = false;
                        }
                    }
                    _ => {
                        let wait = 1500 / if hard { 2 } else { 1 };
                        if ai.bases[b].ended + wait < now {
                            ai.bases[b].state = 5;
                            ai.bases[b].started = false;
                        }
                    }
                }
                return;
            }
            ai.bases[b].started = true;
            let (order, weight) = ai
                .script(b)
                .get(ai.bases[b].step)
                .copied()
                .unwrap_or((0, 0));
            if order == 0 {
                ai.bases[b].step = 0;
                ai.bases[b].rounds += 1;
                ai.bases[b].state = 0;
                ai.bases[b].started = false;
                return;
            }
            let base = &mut ai.bases[b];
            base.order = order;
            base.progress = 0;
            base.at = now;
            let _ = ai.roll(0x27);
            let prio = if order == 2 { 100 } else { 0 } + weight + 1 + 22767;
            // Flags 0x88, 0xd8 for the repairs (0x47d2e0).
            let flags = if order == 0x60 { 0xd8 } else { 0x88 };
            let ask = self.town_order(ai, b, order, None, prio, flags);
            ai.bases[b].ask = ask;
            return;
        }
        // No settlement: found one (again).
        let base = &ai.bases[b];
        let (state, started, at) = (base.state, base.started, base.at);
        let start = |ai: &mut Computer| {
            ai.bases[b].started = true;
            ai.bases[b].at = now;
        };
        let set = |ai: &mut Computer, s: u32| {
            ai.bases[b].state = s;
            ai.bases[b].started = false;
        };
        match state {
            0 => {
                let r = ai.roll(0xbb9);
                if now < (r + 3000) * 5 && b == 0 {
                    set(ai, 0x32);
                } else {
                    set(ai, 5);
                }
            }
            5 | 0x32 => {
                if started {
                    let wait = if state == 5 { 500 } else { 0xfa };
                    if now > at + wait {
                        set(ai, if state == 5 { 0xb } else { 0x33 });
                    }
                    return;
                }
                ai.bases[b].started = true;
                let fail = if state == 5 { 0x1e } else { 0x37 };
                let Some(fleet) = self.founders(ai, Some(b), None) else {
                    set(ai, fail);
                    return;
                };
                // 0x484b90: the base's own site while it serves, else the
                // best; early, the best near the founders.
                let place = if state == 5 {
                    self.town_site(ai.owner, Some(ai.bases[b].cell), None)
                } else {
                    let near = self.fleet_cell(&ai.flotillas[fleet]);
                    near.and_then(|n| self.town_site(ai.owner, None, Some(n)))
                };
                let Some(cell) = place else {
                    set(ai, fail);
                    return;
                };
                ai.bases[b].cell = cell;
                let len = ai.roll(0xbb9) as i32 + 3000;
                let mut p = [0; 12];
                p[0] = 0x32;
                p[1] = len;
                p[3..8].fill(1);
                p[8] = -1;
                p[9] = -1;
                ai.mail(
                    fleet,
                    Mail {
                        kind: 1,
                        at: now,
                        task: 0xc,
                        p,
                    },
                );
                ai.bases[b].at = now;
            }
            0xb | 0x33 => {
                if !started {
                    start(ai);
                    return;
                }
                if !self.founding(ai) {
                    set(ai, if state == 0xb { 0xc } else { 0x34 });
                } else if at + if state == 0xb { 1500 } else { 500 } < now {
                    set(ai, 0);
                }
            }
            0xc | 0x34 => {
                if !started {
                    start(ai);
                    return;
                }
                let cell = ai.bases[b].cell;
                if self.building_at(cell) {
                    set(ai, if state == 0xc { 0xd } else { 0x35 });
                } else if at + 375 < now {
                    set(ai, 0);
                }
            }
            0xd | 0x35 | 0x1e | 0x37 => {
                if !started {
                    start(ai);
                    return;
                }
                let wait = match state {
                    0x1e => 1500,
                    0x37 => 375,
                    _ => 1000,
                };
                if at + wait < now {
                    set(ai, 0);
                }
            }
            _ => set(ai, 0),
        }
    }

    /// Whether a building stands on `cell` (the map's object grid).
    fn building_at(&self, (x, y): (i32, i32)) -> bool {
        self.objects.iter().any(|o| {
            o.cell.is_some_and(|(cx, cy)| {
                let size = super::trade::building_cells(o);
                (cx..cx + size).contains(&x) && (cy..cy + size).contains(&y)
            }) && matches!(o.class, ObjectClass::Building(_))
        })
    }

    /// A flotilla with ships to found a settlement, weighted by its size
    /// and nearness: one of base `base`, near the first base's cell
    /// (0x484df0), or any near `near` (0x484f60).
    fn founders(
        &mut self,
        ai: &mut Computer,
        base: Option<usize>,
        near: Option<(i32, i32)>,
    ) -> Option<usize> {
        let to = near.unwrap_or(ai.bases.first().map_or((0, 0), |b| b.cell));
        let mut found: Vec<(i32, usize)> = Vec::new();
        for (k, fl) in ai.flotillas.iter().enumerate() {
            if fl.role <= 1
                || fl.role == COMMANDER
                || fl.ships.is_empty()
                || base.is_some_and(|b| fl.base != b)
            {
                continue;
            }
            let d = self
                .fleet_cell(fl)
                .map_or(1, |(x, y)| ((to.0 - x).abs() + (to.1 - y).abs()).max(1));
            found.push((10000 / d + fl.ships.len() as i32 * 100, k));
        }
        if found.is_empty() {
            return None;
        }
        let weights: Vec<i32> = found.iter().map(|x| x.0).collect();
        Some(found[ai.pick(&weights)].1)
    }

    /// Whether some flotilla is still founding (message 0x67).
    fn founding(&self, ai: &Computer) -> bool {
        ai.flotillas
            .iter()
            .any(|f| f.task == 0xc && !f.ships.is_empty())
    }

    /// The flotilla's ships' mean cell (0x482dc0).
    fn fleet_cell(&self, f: &Flotilla) -> Option<(i32, i32)> {
        let cells: Vec<(i32, i32)> = f
            .ships
            .iter()
            .filter_map(|&id| self.object_index(id).map(|i| self.cell_of(i)))
            .collect();
        let n = cells.len() as i32;
        (n > 0).then(|| {
            (
                cells.iter().map(|c| c.0).sum::<i32>() / n,
                cells.iter().map(|c| c.1).sum::<i32>() / n,
            )
        })
    }

    /// Whether base `b`'s town has its port, shipyard, sage school and
    /// smithy (0x43d220 with 0x39, 0x3a/0x3b, 0x37, 0x38).
    fn has_basics(&self, ai: &Computer, b: usize) -> bool {
        let n = |c: u32| self.town_count(ai, b, c);
        n(PORT) > 0 && n(SHIPYARD) + n(DOCK) > 0 && n(WORKSHOP) > 0 && n(SMITHY) > 0
    }

    /// Buildings of class `class` of base `b`'s town, standing or going up.
    fn town_count(&self, ai: &Computer, b: usize, class: u32) -> usize {
        let Some(site) = ai.bases[b].site else {
            return 0;
        };
        let Some(t) = self.object_index(site) else {
            return 0;
        };
        let (tx, ty) = self.cell_of(t);
        self.objects
            .iter()
            .filter(|o| {
                o.owner == Some(ai.owner)
                    && o.class == ObjectClass::Building(class)
                    && o.cell.is_some_and(|(x, y)| {
                        (x - tx).abs() <= BUILD_RADIUS + 2 && (y - ty).abs() <= BUILD_RADIUS + 2
                    })
            })
            .count()
    }

    /// Ships asked for (0x4800c0): for each group of the flotilla's want
    /// table, the first type its town can build, if it has fewer than
    /// wanted and the reserve has none.
    fn ai_ask_ships(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let b = ai.flotillas[f].base;
        if ai.bases.get(b).is_none_or(|b| b.site.is_none()) {
            return;
        }
        let have = self.type_counts(&ai.flotillas[f]);
        let spare = self.type_counts(&ai.flotillas[ai.reserve]);
        for group in ai.wants_of(ai.flotillas[f].role) {
            let Some(&(kind, count, flag, prio)) =
                group.iter().find(|w| self.can_produce(ai.owner, w.0))
            else {
                continue;
            };
            if have[kind as usize] < count && spare[kind as usize] == 0 {
                let exists = ai
                    .requests
                    .iter()
                    .any(|r| r.kind == kind && r.count == count && r.fleet == f && r.base == b);
                if !exists {
                    let delay = ai.roll(0x27) + 0xc;
                    ai.requests.push(Request {
                        kind,
                        count,
                        flag,
                        prio,
                        fleet: f,
                        base: b,
                        key: prio - flag * 2500 + 22767,
                        delay,
                        at: now,
                        ..Default::default()
                    });
                }
            }
        }
    }

    /// Ships of the flotilla by type (0x482c70).
    fn type_counts(&self, f: &Flotilla) -> [i32; 12] {
        let mut n = [0; 12];
        for &id in &f.ships {
            if let Some(s) = self
                .object_index(id)
                .and_then(|i| self.objects[i].ship.as_ref())
            {
                n[s.kind as usize % 12] += 1;
            }
        }
        n
    }

    /// Whether the player can have ships of type `kind` built (0x43da20
    /// case 4 without the price): its nation's, studied if unique, the
    /// buildings it needs standing.
    fn can_produce(&self, owner: (u32, u32), kind: u8) -> bool {
        if !economy::buildable_ships(owner.0).any(|k| k == kind) {
            return false;
        }
        if (8..12).contains(&kind)
            && self.levels_of(Some(owner)).get(Upgrade::Special(kind - 8)) == 0
        {
            return false;
        }
        let need = economy::ship_requirements(kind) | 1;
        self.buildings_of(owner) & need == need
    }

    /// Sends the asked ships to the towns and forgets the served (0x480270).
    fn serve_requests(&mut self, ai: &mut Computer, now: u32) {
        let mut k = 0;
        while k < ai.requests.len() {
            let r = ai.requests[k].clone();
            if r.delay + r.at > now {
                k += 1;
                continue;
            }
            match r.state {
                0 => {
                    // Kind 0 is order 0x5c (built free), the others 0x51-0x5b
                    // (0x47fe60, its table 0x47ff64); flags 0x488.
                    let code = if r.kind == 0 {
                        0x5c
                    } else {
                        0x50 + r.kind as u32
                    };
                    let ask = self.town_order(ai, r.base, code, Some(r.fleet), r.key, 0x488);
                    ai.requests[k].ask = ask;
                    ai.requests[k].state = 1;
                }
                1 => {
                    // Its entry's state (0x482ff0): 3 done, 4 failed, 5 none.
                    let result = ai.ask_state(r.ask);
                    if result == 3 {
                        ai.requests[k].state = 2;
                        ai.requests[k].ended = now;
                    } else if result == 4 || result == 5 {
                        ai.requests[k].state = 3;
                        ai.requests[k].ended = now;
                    }
                }
                _ => {
                    if r.ended + 1500 < now {
                        ai.requests.remove(k);
                        continue;
                    }
                }
            }
            k += 1;
        }
    }

    /// Asks base `b`'s town for order `code` (0x482f50: message 1000 to
    /// the land fleet, queued as an op-1000 entry by 0x4a09d0 / 0x42c430;
    /// the walk puts the order, 0x42c050). How the asker finds its entry
    /// again, or `None` if the base has no town.
    fn town_order(
        &mut self,
        ai: &mut Computer,
        b: usize,
        code: u32,
        fleet: Option<usize>,
        prio: i32,
        flags: u32,
    ) -> Option<Ask> {
        ai.bases.get(b)?.site?;
        let ask = (code, prio, b, fleet);
        ai.land.queue_entry(Entry {
            prio,
            job: Job::Ask {
                ask,
                flags,
                state: 0,
                order: None,
            },
        });
        Some(ask)
    }

    /// The land fleet's message 0 (0x42b910), every tick: a pass 50 ticks
    /// after one that started nothing; after one that started a part, 3
    /// ticks after it once something woke the fleet (0x42b94c-0x42b98d).
    fn land_fleet(&mut self, ai: &mut Computer, now: u32, data: &GameData, caches: &mut Caches) {
        let due = if !ai.land.started {
            now >= ai.land.last + 50
        } else {
            ai.land.woken && now >= ai.land.last + 3
        };
        if !due {
            return;
        }
        ai.land.last = now;
        ai.queue_order_parts();
        // B: the queue, by priority. Waiting tries and under-way waits
        // count down only in a pass after one that started nothing (+0x18).
        let count = !ai.land.started;
        ai.land.started = false;
        ai.land.woken = false;
        let mut i = 0;
        while i < ai.land.queue.len() {
            let entry = ai.land.queue[i];
            let s = entry.state();
            if s <= 1 {
                ai.land.queue[i].set_state(1);
                match entry.job {
                    Job::Ask { .. } => ai.accept_order(i),
                    Job::Part {
                        base, fleet, part, ..
                    } => {
                        let step = self.order_part(ai, base, fleet, part, data, caches);
                        let Job::Part { part: p, .. } = &mut ai.land.queue[i].job else {
                            unreachable!()
                        };
                        p.refused = matches!(step, PartStep::Berth);
                        match step {
                            PartStep::Done => p.state = 3,
                            PartStep::Berth => {
                                if count {
                                    p.berths += 1;
                                }
                                if p.berths >= BERTH_PASSES {
                                    p.berths = 0;
                                    self.berths_held(ai, i, base, part.op);
                                }
                            }
                            PartStep::Started(before) => {
                                p.state = 2;
                                p.before = before;
                                p.tries = p.op.tries_under_way();
                                // The town wakes its land fleet when it
                                // has handled the part's command (0x441ba0
                                // at 0x44393d). Not decoded: the tick the
                                // command (0x520cc0) reaches the town; the
                                // port's command takes effect at once.
                                ai.land.woken = true;
                            }
                            PartStep::Wait => {
                                if count {
                                    p.tries = p.tries.saturating_sub(1);
                                    if p.tries == 0 {
                                        p.state = 4;
                                    }
                                }
                            }
                            PartStep::Hold => {}
                            PartStep::Failed => p.state = 4,
                        }
                    }
                }
                ai.land.started = matches!(ai.land.queue[i].state(), 2 | 3);
            } else if s == 2 {
                // The under-way checks (op 1000 waits for its order).
                if let Job::Part {
                    base, fleet, part, ..
                } = entry.job
                {
                    let step = self.order_part(ai, base, fleet, part, data, caches);
                    let Job::Part { part: p, .. } = &mut ai.land.queue[i].job else {
                        unreachable!()
                    };
                    match step {
                        PartStep::Done => {
                            p.state = 3;
                            if let Op::Build(_) = part.op {
                                ai.land.built(i, base, part.op);
                            }
                        }
                        PartStep::Wait => {
                            if count {
                                p.tries = p.tries.saturating_sub(1);
                                if p.tries == 0 {
                                    p.state = 4;
                                }
                            }
                        }
                        PartStep::Failed => p.state = 4,
                        PartStep::Hold | PartStep::Berth | PartStep::Started(_) => {}
                    }
                }
            }
            // An entry's own flag 0x10 (restart when failed) and flag 0x800
            // (an op-1000 entry removed when it ends) are not ported: the
            // parts' flags are 7, 0x27 or 0x207 (0x5ac7c8) and the asks'
            // +0x20 is 0 (0x47d2e0, 0x47fe60).
            let e = ai.land.queue[i];
            let s = e.state();
            if s == 3 || s == 4 {
                ai.land.woken = true;
                if let Job::Part { order, index, .. } = e.job {
                    if let Some(o) = ai.orders.iter_mut().find(|o| o.id == order) {
                        if let Some(p) = o.parts.get_mut(index) {
                            p.state = s;
                        }
                    }
                    ai.land.queue.remove(i);
                    if ai.land.started {
                        break;
                    }
                    continue;
                }
            } else if s == 1 && e.prio as u32 >= LAND_THRESHOLD {
                break;
            }
            if ai.land.started {
                break;
            }
            i += 1;
        }
    }

    /// A ship part (the queue's entry `i`, of base `b`) held 25 passes by
    /// a -1 (0x428f20 at 0x4295cb): its town is sent command 0x28 with
    /// payload 3, clearing its berths (0x441ba0, 0x4415a0), and every other
    /// tried part for a ship of the same type whose last result was -1
    /// starts its count again. (Not decoded: the tick a command sent by
    /// 0x520cc0 reaches the town; the port's is at once.)
    fn berths_held(&mut self, ai: &mut Computer, i: usize, b: usize, op: Op) {
        let yards = self.base_yards(ai, b);
        self.clear_berths(Some(ai.owner), &yards);
        for (k, e) in ai.land.queue.iter_mut().enumerate() {
            let state = e.state();
            if let Job::Part { part, .. } = &mut e.job {
                if k != i && state == 1 && part.op == op && part.refused {
                    part.berths = 0;
                }
            }
        }
    }

    /// One pass at a part of a town's order.
    fn order_part(
        &mut self,
        ai: &mut Computer,
        b: usize,
        fleet: Option<usize>,
        part: Part,
        data: &GameData,
        caches: &mut Caches,
    ) -> PartStep {
        let Some(site) = ai.bases.get(b).and_then(|b| b.site) else {
            return PartStep::Failed;
        };
        if self.object_index(site).is_none() {
            return PartStep::Failed;
        }
        let owner = ai.owner;
        match part.op {
            Op::Build(class) => {
                let have = self.town_count(ai, b, class)
                    + if class == SHIPYARD {
                        self.town_count(ai, b, DOCK)
                    } else {
                        0
                    };
                if part.state == 2 {
                    return if have > part.before {
                        PartStep::Done
                    } else {
                        PartStep::Wait
                    };
                }
                if part.once && have > 0 {
                    return PartStep::Done;
                }
                // Schools, temples and big forts go up only by towns.
                let is_town = self
                    .object_index(site)
                    .is_some_and(|i| self.class_of(i) == Some(TOWN));
                if matches!(class, SCHOOL | TEMPLE_OF_THE_SEA | TEMPLE_OF_WAR | 0x36) && !is_town {
                    return PartStep::Wait;
                }
                if self.funds(owner).0 < economy::building_cost(class) {
                    return PartStep::Hold;
                }
                let Some((x, y, facing)) = self.town_build_place(site, class) else {
                    return PartStep::Wait;
                };
                match self.place_building_facing(site, class, x, y, Some(facing), data, caches) {
                    Ok(()) => PartStep::Started(have),
                    Err(_) => PartStep::Wait,
                }
            }
            Op::Ship(kind) => {
                if part.state == 2 {
                    return PartStep::Done;
                }
                // First whether it is allowed at all, the price aside
                // (0x43da20 with param_3 = 0): refused, it counts down.
                if self.may_build_ship_for(owner, kind).is_err() {
                    return PartStep::Wait;
                }
                // Then (param_3 = 1) the town must be idle, else -1 (not
                // ported: the town's states other than going up or being
                // made a town, `+0xa4`).
                if self.construction_at(site).is_some() {
                    return PartStep::Berth;
                }
                // Field 8 skips the price (0x428f20 passes it to 0x43da20 as
                // param_7) and builds the boat free (0x416a40).
                let (amber, fish) = economy::ship_cost(kind);
                let (have_amber, have_fish) = self.funds(owner);
                if !part.free && (have_amber < amber || have_fish < fish) {
                    return PartStep::Hold;
                }
                // The yard test (param_8 = 1): the first of the town's yards
                // answering 0 to the ship's code (0x4e82f0) builds it (the
                // town hands command 0x20 to that one, 0x441ba0); none: -1
                // if one answered -1 (its berth taken), holding the part
                // (flag 4), else -5, a try spent.
                let mut refused = -5;
                let mut yard = None;
                for y in self.base_yards(ai, b) {
                    let Some(i) = self.object_index(y) else {
                        continue;
                    };
                    match self.yard_answer(i, Some(kind)) {
                        0 => {
                            yard = Some(y);
                            break;
                        }
                        -1 => refused = -1,
                        _ => {}
                    }
                }
                let Some(yard) = yard else {
                    return if refused == -1 {
                        PartStep::Berth
                    } else {
                        PartStep::Wait
                    };
                };
                match self.start_ship_armed(yard, kind, !part.free, true) {
                    Ok(()) => {
                        if let Some(f) = fleet {
                            ai.building.push((yard, kind, f));
                        }
                        PartStep::Done
                    }
                    Err(_) => PartStep::Wait,
                }
            }
            Op::Upgrade(class) => {
                let to = if class == SETTLEMENT { TOWN } else { DOCK };
                let target = if class == SETTLEMENT {
                    Some(site)
                } else {
                    self.base_building(ai, b, SHIPYARD)
                };
                if part.state == 2 {
                    return match target.and_then(|t| self.object_index(t)) {
                        Some(i)
                            if self.class_of(i) == Some(to)
                                && self.construction_at(self.objects[i].id).is_none() =>
                        {
                            PartStep::Done
                        }
                        Some(_) => PartStep::Wait,
                        None => PartStep::Failed,
                    };
                }
                let upgraded = if to == TOWN {
                    self.object_index(site)
                        .is_some_and(|i| self.class_of(i) == Some(TOWN))
                } else {
                    self.town_count(ai, b, DOCK) > 0
                };
                if part.once && upgraded {
                    return PartStep::Done;
                }
                let Some(t) = target else {
                    return PartStep::Wait;
                };
                if self.funds(owner).0 < economy::building_cost(to) {
                    return PartStep::Hold;
                }
                match self.upgrade(t, data, caches) {
                    Ok(()) => PartStep::Started(0),
                    Err(_) => PartStep::Wait,
                }
            }
            Op::Study(n @ (33 | 34)) => {
                // The temples' next spell (Temple of the Sea 33, of War 34;
                // `get magic`, 1000 amber).
                use crate::magic::Temple;
                let temple = if n == 33 { Temple::Sea } else { Temple::War };
                if part.state == 2 {
                    return if ai.magic.learning(temple).is_some() {
                        PartStep::Wait
                    } else {
                        PartStep::Done
                    };
                }
                if ai.magic.all_known(temple) {
                    return PartStep::Done;
                }
                let Some(at) = self.base_building(ai, b, temple.class()) else {
                    return PartStep::Wait;
                };
                let cost = crate::magic::LEARN_COST as i64;
                if (self.funds(owner).0 as i64) < cost {
                    return PartStep::Hold;
                }
                self.earn(owner, -cost, 0);
                ai.magic.start_learning(temple, Some(at));
                PartStep::Started(0)
            }
            Op::Study(n) => {
                let Some(upgrade) = study(n, ai.nation()) else {
                    return PartStep::Failed;
                };
                let level = self.levels_of(Some(owner)).get(upgrade);
                let top = match n {
                    1..=3 | 17 => 2.min(upgrade.max_level()),
                    _ => upgrade.max_level(),
                };
                if part.state == 2 {
                    let busy = self.research.iter().any(|r| {
                        r.1 == upgrade
                            && self
                                .object_index(r.0)
                                .is_some_and(|i| self.objects[i].owner == Some(owner))
                    });
                    return if busy { PartStep::Wait } else { PartStep::Done };
                }
                if level >= top {
                    return PartStep::Done;
                }
                if self.funds(owner).0 < upgrade.cost(level) {
                    return PartStep::Hold;
                }
                if self.computer_study(ai, b, upgrade, level) {
                    PartStep::Started(0)
                } else {
                    PartStep::Wait
                }
            }
            Op::Repair => {
                let Some(site) = ai.bases.get(b).and_then(|b| b.site) else {
                    return PartStep::Failed;
                };
                // Under way (0x42a2f0): the building mended (`+0xdc`) gone
                // fails it; whole (life `+0xa8` 100), it is sent order 10
                // (0x42a384), which, besides ending its mending, calls off
                // its work and leaves a fort's groups ceasing fire. (Not
                // ported: 0x43d300's test that it is still the town's.)
                if part.state == 2 {
                    let target = part.before as u32;
                    let Some(t) = self.object_index(target) else {
                        return PartStep::Failed;
                    };
                    let whole = self.objects[t]
                        .combat
                        .as_ref()
                        .is_none_or(|c| c.hull >= c.max_hull);
                    if !whole {
                        return PartStep::Wait;
                    }
                    // Order 10 (0x520cc0): at the next tick.
                    self.post(Order::Halt { id: target });
                    return PartStep::Done;
                }
                let worn: Vec<u32> = self
                    .objects
                    .iter()
                    .filter(|o| {
                        o.owner == Some(owner)
                            && matches!(o.class, ObjectClass::Building(_))
                            && !self.mending.contains(&o.id)
                            && o.combat.as_ref().is_some_and(|c| c.hull < c.max_hull)
                    })
                    .map(|o| o.id)
                    .collect();
                // Nothing worn: it waits, failing when its tries run out
                // (0x429f60).
                if worn.is_empty() {
                    return PartStep::Wait;
                }
                // Started, it is under way with the building kept
                // (`+0xdc`, here `before`).
                for target in worn {
                    if self.mend_by(site, target).is_ok() {
                        return PartStep::Started(target as usize);
                    }
                }
                PartStep::Wait
            }
            Op::Other => PartStep::Done,
        }
    }

    /// Starts studying `upgrade` from `level` at base `b`'s school or
    /// workshop, paying for it (0x4404e0 for the computer).
    fn computer_study(&mut self, ai: &Computer, b: usize, upgrade: Upgrade, level: u8) -> bool {
        let class = upgrade.building();
        if let Upgrade::Special(k) = upgrade {
            if self.levels_of(Some(ai.owner)).weapons < 1 || (ai.nation() == 1) != (k < 2) {
                return false;
            }
        }
        let Some(site) = self.base_building(ai, b, class) else {
            return false;
        };
        if self.construction_at(site).is_some()
            || self.research.iter().any(|r| r.0 == site)
            || self.research.iter().any(|r| {
                r.1.excludes(upgrade)
                    && self
                        .object_index(r.0)
                        .is_some_and(|i| self.objects[i].owner == Some(ai.owner))
            })
        {
            return false;
        }
        let cost = upgrade.cost(level);
        if self.funds(ai.owner).0 < cost {
            return false;
        }
        self.earn(ai.owner, -(cost as i64), 0);
        self.research.push((site, upgrade, level + 1, 0.0));
        true
    }

    /// A standing building of class `class` of base `b`'s town.
    fn base_building(&self, ai: &Computer, b: usize, class: u32) -> Option<u32> {
        let site = ai.bases.get(b)?.site?;
        let (tx, ty) = self.cell_of(self.object_index(site)?);
        self.objects
            .iter()
            .filter(|o| {
                o.owner == Some(ai.owner)
                    && o.class == ObjectClass::Building(class)
                    && self.construction_at(o.id).is_none()
                    && o.cell.is_some_and(|(x, y)| {
                        (x - tx).abs() <= BUILD_RADIUS + 2 && (y - ty).abs() <= BUILD_RADIUS + 2
                    })
            })
            .map(|o| o.id)
            .next()
    }

    /// The shipyards and docks of base `b`'s town (its buildings `+0x1d0`;
    /// the port's are those `base_building` finds), in the objects' order.
    fn base_yards(&self, ai: &Computer, b: usize) -> Vec<u32> {
        let Some(site) = ai.bases.get(b).and_then(|b| b.site) else {
            return Vec::new();
        };
        let Some((tx, ty)) = self.object_index(site).map(|i| self.cell_of(i)) else {
            return Vec::new();
        };
        self.objects
            .iter()
            .filter(|o| {
                o.owner == Some(ai.owner)
                    && matches!(o.class, ObjectClass::Building(SHIPYARD | DOCK))
                    && self.construction_at(o.id).is_none()
                    && o.cell.is_some_and(|(x, y)| {
                        (x - tx).abs() <= BUILD_RADIUS + 2 && (y - ty).abs() <= BUILD_RADIUS + 2
                    })
            })
            .map(|o| o.id)
            .collect()
    }

    /// Base `b`'s area (0x484aa0 with 0x4218f0: the town and its buildings,
    /// 2 cells around).
    fn base_rect(&self, ai: &mut Computer, b: usize) {
        let Some(site) = ai.bases[b].site else {
            return;
        };
        let Some(t) = self.object_index(site) else {
            return;
        };
        let (tx, ty) = self.cell_of(t);
        let (mut x0, mut y0, mut x1, mut y1) = (tx, ty, tx + 1, ty + 1);
        for o in &self.objects {
            if o.owner != Some(ai.owner) || !matches!(o.class, ObjectClass::Building(_)) {
                continue;
            }
            if let Some((x, y)) = o.cell {
                if (x - tx).abs() <= BUILD_RADIUS + 2 && (y - ty).abs() <= BUILD_RADIUS + 2 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        let (w, h) = self.map_cells();
        let (x0, y0) = ((x0 - 2).max(0), (y0 - 2).max(0));
        let (x1, y1) = ((x1 + 2).min(w - 1), (y1 + 2).min(h - 1));
        ai.bases[b].rect = (x0, y0, x1 - x0 + 1, y1 - y0 + 1);
    }

    /// Base `b`'s waiting zones (0x484900 with `LAiFindDispositionZones`,
    /// 0x4093a0): in the 31 x 31 cells about the town, for the guards,
    /// defence, raiders and (first base only) the fleet in turn, a square
    /// for its ships (5-12: side s, the least with s x s >= n + n / 4,
    /// 0x408bb0) holding water for them all, apart from the zones before;
    /// the fewest cells beside the base's ports and yards first, then the
    /// nearest the town, then the most open water, then the fewest cells
    /// on the lanes from the ports to the amber (0x408bf0), the threats and
    /// deposits about weighed (0x594470) before nearness. The first base
    /// has four zones, the others three, their fourth left as it was
    /// (0x484900). The squares run while their far side is short of the
    /// rect's (0x408e7d), in one pass, apart from the zones before
    /// (0x409210); a zone with no square keeps (0, 0, side, side), and puts
    /// zone 0 at (0, 0) as well. (Not ported: with no town the original
    /// looks over the whole map, measuring from where the town was.)
    fn base_zones(&self, ai: &mut Computer, b: usize) {
        let Some(t) = ai.bases[b].site.and_then(|s| self.object_index(s)) else {
            return;
        };
        let (tx, ty) = self.cell_of(t);
        let (w, h) = self.map_cells();
        let span = |c: i32, size: i32| {
            let mut c0 = (c - 15).max(0);
            while size < c0 + 31 && c0 > 0 {
                c0 -= 1;
            }
            (c0, 31.min(size - c0))
        };
        let ((x0, rw), (y0, rh)) = (span(tx, w), span(ty, h));
        // Cells beside the base's ports and yards (0x80000).
        let harbour: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| {
                o.owner == Some(ai.owner)
                    && matches!(o.class, ObjectClass::Building(PORT | SHIPYARD | DOCK))
            })
            .filter_map(|o| o.cell)
            .collect();
        let beside = |x: i32, y: i32| {
            harbour
                .iter()
                .any(|&(hx, hy)| (x - hx).abs() <= 1 && (y - hy).abs() <= 1)
        };
        // The lanes from the base's ports to the amber about (0x90000).
        let t = TILE_SIZE as f32;
        let inside = |(x, y): (i32, i32)| x >= x0 && x < x0 + rw && y >= y0 && y < y0 + rh;
        let mut lanes = std::collections::BTreeSet::new();
        let ports: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| o.owner == Some(ai.owner) && matches!(o.class, ObjectClass::Building(PORT)))
            .filter_map(|o| o.cell)
            .filter_map(|(x, y)| self.nav.nearest_water(x, y))
            .collect();
        let ambers: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| o.deposit.is_some_and(|d| d.resource == Resource::Amber))
            .map(|o| ((o.x / t) as i32, (o.y / t) as i32))
            .filter(|&c| inside(c))
            .filter_map(|(x, y)| self.nav.nearest_water(x, y))
            .collect();
        for &p in &ports {
            for &a in &ambers {
                if let Some(path) = self.nav.find_path_avoiding(p, a, false, |_| false) {
                    lanes.extend(path);
                }
            }
        }
        // What lies about, by the zones' weights (0x594470).
        let wt = self.influence_map(
            ai.owner,
            &super::placement::ZONE_WEIGHTS,
            (x0, y0, rw, rh),
            &|_, _| false,
        );
        let mw = self.grid.width as i32;
        let slots = if b == 0 { 4 } else { 3 };
        let mut taken: Vec<Cells> = Vec::new();
        let mut placed: Vec<(usize, u32, Option<Cells>, i32)> = Vec::new();
        for k in 0..slots {
            let n = {
                let role = [GUARD, DEFENCE, RAIDERS, FLEET][k];
                ai.flotilla_of(b, role)
                    .map(|f| ai.flotillas[f].ships.len() as u32)
                    .filter(|&n| n > 5)
                    .map_or(5, |n| n.min(12))
            };
            let need = n as i32 + n as i32 / 4;
            let mut side = 1;
            while side * side < need {
                side += 1;
            }
            let mut best: Option<(ZoneKey, Cells)> = None;
            {
                for y in y0..y0 + rh - side {
                    for x in x0..x0 + rw - side {
                        let rect = (x, y, side, side);
                        let over = |r: &Cells| {
                            x < r.0 + r.2 && r.0 < x + side && y < r.1 + r.3 && r.1 < y + side
                        };
                        if taken.iter().any(over) {
                            continue;
                        }
                        // Its cells: closed to deep ships, beside the
                        // ports and yards, on the lanes, or open; their
                        // weights (0x408bf0).
                        let (mut passable, mut near, mut lane, mut open, mut weight) =
                            (0, 0, 0, 0, 0);
                        for cy in y..y + side {
                            for cx in x..x + side {
                                if !self.nav.passable(cx, cy, true) {
                                    continue;
                                }
                                passable += 1;
                                weight += wt[(cy * mw + cx) as usize];
                                if beside(cx, cy) {
                                    near += 1;
                                } else if lanes.contains(&(cx, cy)) {
                                    lane += 1;
                                } else {
                                    open += 1;
                                }
                            }
                        }
                        if passable < n as i32 {
                            continue;
                        }
                        let d = super::placement::distance(x + side / 2 - tx, y + side / 2 - ty);
                        // Fewest beside the harbour, the heaviest, the
                        // nearest the town, the most open, the fewest on
                        // the lanes, the most passable.
                        let key = (near, -weight, d, -open, lane, -passable);
                        if best.is_none_or(|bb| key < bb.0) {
                            best = Some((key, rect));
                        }
                    }
                }
            }
            let found = best.map(|b| b.1);
            match found {
                Some(rect) => taken.push(rect),
                // Its mark lands on the rect's corner cells (only those
                // when the rect starts at the map's; beyond, not decoded).
                None if x0 == 0 && y0 == 0 => taken.push((0, 0, side, side)),
                None => {}
            }
            placed.push((k, n, found, side));
        }
        // The records go back in order; a failed one, zeroed, writes zone
        // 0's place (0x409210).
        for (k, n, found, side) in placed {
            match found {
                Some(rect) => ai.bases[b].zones[k] = (n, rect),
                None => {
                    ai.bases[b].zones[k] = (n, (0, 0, side, side));
                    let z = &mut ai.bases[b].zones[0].1;
                    (z.0, z.1) = (0, 0);
                }
            }
        }
    }

    /// The flotilla's waiting zone (0x485120: guards slot 0, gatherers'
    /// and defence slot 1, raiders slot 2, the fleet slot 3).
    fn read_zone(&self, ai: &mut Computer, f: usize) {
        let slot = match ai.flotillas[f].role {
            AMBER | RAIDERS => 2,
            FISH | DEFENCE | SPARE => 1,
            BUILDERS | GUARD => 0,
            FLEET | RESERVE => 3,
            _ => return,
        };
        let b = ai.flotillas[f].base;
        if let Some(base) = ai.bases.get(b) {
            ai.flotillas[f].zone = base.zones[slot].1;
        }
    }

    /// The flotilla's home: its base's area (0x485210).
    fn read_home(&self, ai: &mut Computer, f: usize) {
        let b = ai.flotillas[f].base;
        if let Some(base) = ai.bases.get(b) {
            ai.flotillas[f].home = base.rect;
        }
    }

    /// Ships of the flotilla, if any (message 6).
    fn size(ai: &Computer, f: usize) -> i32 {
        ai.flotillas[f].ships.len() as i32
    }

    /// Task 4 (0x47dc60): gather near the base, sail to the zone and look
    /// again; amber gatherers turn to fish when fish runs short and amber
    /// is plenty, and back.
    fn task_gather(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let fl = &ai.flotillas[f];
        let state = fl.state;
        match state {
            0 => {
                ai.flotillas[f].p[4] = now as i32;
                ai.push(f, 2);
            }
            2 => {
                if Self::size(ai, f) < 1 {
                    ai.push(f, 0);
                } else {
                    let res = ai.flotillas[f].p[0];
                    self.resource_area(ai, f, 4, res);
                    self.read_zone(ai, f);
                    ai.push(f, 5);
                }
            }
            5 => {
                if !fl.step {
                    let len = fl.p[2] as u32;
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = len;
                    let res = resource(fl.p[0]);
                    fl.order = FleetOrder::Gather(fl.area, res);
                    self.give_order(ai, f);
                } else {
                    self.keep_order(ai, f);
                    if Computer::over(&ai.flotillas[f], now) {
                        ai.push(f, 7);
                    }
                }
            }
            7 => {
                if !fl.step {
                    let len = ai.roll(0x2ef) + 0x2ee;
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = len;
                    self.move_to_zone(ai, f);
                } else if self.report(ai, f) == Some(DONE) || Computer::over(&ai.flotillas[f], now)
                {
                    self.switch_resource(ai, f);
                    ai.push(f, 2);
                }
            }
            _ => {}
        }
    }

    /// Picks the resource to gather next (0x47dc60's ending).
    fn switch_resource(&mut self, ai: &mut Computer, f: usize) {
        let (amber, fish) = self.funds(ai.owner);
        let p = ai.flotillas[f].p;
        let mut res = p[0];
        if res == AMBER_CLASS {
            if p[1] != AMBER_CLASS || (fish < 3000 && amber > 0x1194) {
                res = FISH_CLASS;
            }
        } else if p[1] == AMBER_CLASS || (fish > 3000 && amber < 2000) || ai.roll(0xb) == 0 {
            res = AMBER_CLASS;
        }
        let mut len = if res == p[1] { 7500 } else { 3000 };
        if res == FISH_CLASS {
            len *= 3;
        }
        let fl = &mut ai.flotillas[f];
        fl.p[0] = res;
        fl.p[2] = len;
    }

    /// The area about the base holding more than 3 deposits of the
    /// resource (any for leviathans), growing from `r` cells by 2 (0x485380).
    fn resource_area(&self, ai: &mut Computer, f: usize, r: i32, res: i32) {
        let b = ai.flotillas[f].base;
        let (w, h) = self.map_cells();
        let Some(t) = ai
            .bases
            .get(b)
            .and_then(|b| b.site)
            .and_then(|s| self.object_index(s))
        else {
            ai.flotillas[f].area = (0, 0, w, h);
            return;
        };
        let (tx, ty) = self.cell_of(t);
        let need = if res == WHALE_CLASS { 0 } else { 3 };
        let mut r = r;
        loop {
            let (x0, y0) = ((tx - r).max(0), (ty - r).max(0));
            let side = 2 * r + 1;
            let (rw, rh) = (side.min(w - x0), side.min(h - y0));
            let n = self.deposits_in(resource(res), (x0, y0, rw, rh));
            if n > need || (rw >= w && rh >= h) {
                ai.flotillas[f].area = if n > 0 {
                    (x0, y0, rw, rh)
                } else {
                    (0, 0, w, h)
                };
                return;
            }
            r += 2;
        }
    }

    /// The square about the base's town, from `r` cells and 4 more a time
    /// (the side 8 more), that holds a treasure; the whole map if none
    /// does (0x4854f0, counting with 0x48f020).
    fn treasure_area(&self, ai: &Computer, f: usize, r: i32) -> Cells {
        let (w, h) = self.map_cells();
        let b = ai.flotillas[f].base;
        let Some(t) = ai
            .bases
            .get(b)
            .and_then(|b| b.site)
            .and_then(|s| self.object_index(s))
        else {
            return (0, 0, w, h);
        };
        let (tx, ty) = self.cell_of(t);
        let mut r = r;
        loop {
            let (x0, y0) = ((tx - r).max(0), (ty - r).max(0));
            let side = 2 * r + 1;
            let (rw, rh) = (side.min(w - x0), side.min(h - y0));
            if self.treasures_in((x0, y0, rw, rh)).next().is_some() {
                return (x0, y0, rw, rh);
            }
            if rw >= w && rh >= h {
                return (0, 0, w, h);
            }
            r += 4;
        }
    }

    /// Treasures lying in an area (class 700, 0x4d9920).
    fn treasures_in(&self, (x, y, w, h): Cells) -> impl Iterator<Item = usize> + '_ {
        (0..self.objects.len()).filter(move |&i| {
            self.objects[i].class == ObjectClass::Treasure && {
                let (cx, cy) = self.cell_of(i);
                cx >= x && cx < x + w && cy >= y && cy < y + h
            }
        })
    }

    fn deposits_in(&self, res: Resource, (x, y, w, h): Cells) -> usize {
        let t = TILE_SIZE as f32;
        self.objects
            .iter()
            .filter(|o| {
                o.deposit.is_some_and(|d| d.resource == res) && {
                    let (cx, cy) = ((o.x / t) as i32, (o.y / t) as i32);
                    cx >= x && cx < x + w && cy >= y && cy < y + h
                }
            })
            .count()
    }

    /// Task 5 (0x47dfb0): the builders' rounds: sail the waiting zone,
    /// hunt leviathans, then gather (amber if fish is plenty).
    fn task_explore(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let fl = &ai.flotillas[f];
        match fl.state {
            0 => {
                let a = ai.roll(0x5dd) as i32 + 3000;
                let b = ai.roll(0xfb) as i32 + 0xfa;
                let c = ai.roll(0x1195) as i32 + 6000;
                let d = ai.roll(0x3e9) as i32 + 500;
                let total = (a + b + c + d) as u32;
                let total = match ai.difficulty {
                    1 => total * 2 / 3,
                    2 => total / 2,
                    _ => total,
                };
                let fl = &mut ai.flotillas[f];
                fl.p[2] = now as i32;
                fl.p[3] = a;
                fl.p[4] = b;
                fl.p[5] = c;
                fl.p[6] = d;
                fl.p[1] = total as i32;
                ai.push(f, 2);
            }
            // The treasure hunt: order 0xbe in the area about the base
            // holding a treasure (0x4854f0), until none is left or the time
            // is up.
            2 => {
                if !fl.step {
                    if Self::size(ai, f) < 1 {
                        ai.push(f, 0);
                    } else {
                        let len = fl.p[3] as u32;
                        let area = self.treasure_area(ai, f, 4);
                        let fl = &mut ai.flotillas[f];
                        fl.step = true;
                        fl.since = now;
                        fl.len = len;
                        fl.order = FleetOrder::Treasure(area);
                        self.give_order(ai, f);
                    }
                } else {
                    self.keep_order(ai, f);
                    let done = self.report(ai, f) == Some(DONE);
                    if done || Computer::over(&ai.flotillas[f], now) {
                        ai.push(f, 5);
                    }
                }
            }
            5 => {
                if !fl.step {
                    let len = fl.p[4] as u32;
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = len;
                    self.read_zone(ai, f);
                    self.move_to_zone(ai, f);
                } else {
                    let done = self.report(ai, f) == Some(DONE);
                    if done || Computer::over(&ai.flotillas[f], now) {
                        ai.push(f, 7);
                    }
                }
            }
            7 | 10 => {
                if !fl.step {
                    let (len, r, res) = if fl.state == 7 {
                        (fl.p[5], 10, WHALE_CLASS)
                    } else {
                        let fish = self.funds(ai.owner).1;
                        (
                            fl.p[6],
                            4,
                            if fish < 1000 { FISH_CLASS } else { AMBER_CLASS },
                        )
                    };
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = len as u32;
                    fl.p[0] = res;
                    self.resource_area(ai, f, r, res);
                    let fl = &mut ai.flotillas[f];
                    fl.order = FleetOrder::Gather(fl.area, resource(res));
                    self.give_order(ai, f);
                } else {
                    self.keep_order(ai, f);
                    if Computer::over(&ai.flotillas[f], now) {
                        let next = if ai.flotillas[f].state == 7 { 10 } else { 2 };
                        ai.push(f, next);
                    }
                }
            }
            _ => {}
        }
        ai.task_end(f, now, 1, 2, 7);
    }

    /// Task 6 (0x47f200): found a fort guarding the base, unless there are
    /// forts enough (more than the map's cells / 70 + 10).
    fn task_fort(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let fl = &ai.flotillas[f];
        match fl.state {
            0 => {
                let len = ai.roll(0xbb9) as i32 + 6000;
                ai.flotillas[f].p[2] = now as i32;
                ai.flotillas[f].p[1] = len;
                let (w, h) = self.map_cells();
                let forts = ai.flotillas[ai.forts].ships.len() as i32;
                if forts > w * h / 0x46 + 10 {
                    ai.go(f, 1);
                    return;
                }
                let Some(cell) = self.fort_place(ai, f) else {
                    ai.go(f, 1);
                    return;
                };
                // The biggest fort the town may put up (0x480070): stone
                // three times in four, else middle; or middle; or wood.
                let b = ai.flotillas[f].base;
                let town = ai.bases.get(b).and_then(|b| b.site);
                let allowed = |class| town.is_some_and(|t| self.fort_allowed(t, class));
                let class = if allowed(0x36) {
                    if ai.roll(4) != 0 {
                        0x36
                    } else {
                        0x35
                    }
                } else if allowed(0x35) {
                    0x35
                } else if allowed(0x34) {
                    0x34
                } else {
                    ai.go(f, 1);
                    return;
                };
                let fl = &mut ai.flotillas[f];
                fl.p[0] = class;
                fl.home = (cell.0, cell.1, 1, 1);
                ai.push(f, 5);
            }
            5 => {
                if !fl.step {
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = fl.p[1] as u32;
                    self.found(ai, f);
                } else {
                    self.keep_order(ai, f);
                    if self.report(ai, f) == Some(DONE) {
                        let next = ai.flotillas[f].p[5];
                        ai.go(f, next);
                        return;
                    }
                }
            }
            _ => {}
        }
        let p = ai.flotillas[f].p;
        if p[1] != 0 && (p[2] + p[1]) as u32 <= now {
            ai.go(f, p[3]);
        }
    }

    /// Where task 6 founds its fort (0x4852c0: the base's town's guarding
    /// fort place, `guard_fort_place`).
    fn fort_place(&self, ai: &Computer, f: usize) -> Option<(i32, i32)> {
        let site = ai.bases.get(ai.flotillas[f].base)?.site?;
        self.guard_fort_place(site)
    }

    /// Task 7 (0x47eed0): guard the waiting zone.
    fn task_guard(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let fl = &ai.flotillas[f];
        match fl.state {
            0 => {
                ai.flotillas[f].p[1] = now as i32;
                ai.push(f, 2);
            }
            2 => {
                self.read_zone(ai, f);
                self.read_home(ai, f);
                let z = ai.flotillas[f].zone;
                if z.2 > 0 && z.3 > 0 {
                    ai.push(f, 5);
                }
            }
            5 => {
                if !fl.step {
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = 0;
                    fl.order = FleetOrder::Guard(fl.zone);
                    self.give_order(ai, f);
                } else {
                    ai.flotillas[f].worn = true;
                    self.keep_order(ai, f);
                }
            }
            _ => {}
        }
        ai.task_end(f, now, 0, 1, 2);
    }

    /// Task 8 (0x47eb70): guard the zone; with a target area, attack it
    /// for a while, then go back.
    fn task_defend(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let fl = &ai.flotillas[f];
        match fl.state {
            0 => {
                ai.flotillas[f].p[6] = now as i32;
                let next = if ai.flotillas[f].p[0] < 0 { 2 } else { 0xc };
                ai.push(f, next);
            }
            2 => {
                self.read_zone(ai, f);
                self.read_home(ai, f);
                let z = ai.flotillas[f].zone;
                if z.2 > 0 && z.3 > 0 {
                    ai.push(f, 5);
                }
                if ai.flotillas[f].p[0] >= 0 {
                    ai.push(f, 0xc);
                }
            }
            5 => {
                if !fl.step {
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = 0;
                    fl.order = FleetOrder::Guard(fl.zone);
                    self.give_order(ai, f);
                } else {
                    ai.flotillas[f].worn = true;
                    self.keep_order(ai, f);
                    if ai.flotillas[f].p[0] >= 0 {
                        ai.push(f, 0xc);
                    }
                }
            }
            0xc => {
                if !fl.step {
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = fl.p[7] as u32;
                    fl.area = (fl.p[1], fl.p[2], fl.p[3], fl.p[4]);
                    fl.order = FleetOrder::Attack(fl.area);
                    self.give_order(ai, f);
                } else {
                    ai.flotillas[f].wants = false;
                    self.keep_order(ai, f);
                    let done = self.report(ai, f) == Some(DONE);
                    if done || Computer::over(&ai.flotillas[f], now) {
                        ai.flotillas[f].p[0] = -1;
                        ai.helps.retain(|h| h.1 != f);
                        let next = ai.flotillas[f].p[if done { 9 } else { 8 }];
                        ai.go(f, next);
                        return;
                    }
                }
            }
            _ => {}
        }
        ai.task_end(f, now, 5, 6, 8);
    }

    /// Tasks 9 and 10 (0x47e3c0, 0x47e7c0): wait in the zone until strong
    /// enough, then raid the enemy's towns; back home when down to a few.
    fn task_raid(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let fleet = ai.flotillas[f].task == 10;
        let fl = &ai.flotillas[f];
        match fl.state {
            0 => {
                ai.flotillas[f].p[5] = now as i32;
                ai.push(f, 2);
            }
            2 => {
                self.read_zone(ai, f);
                self.read_home(ai, f);
                let size = self.target_size(ai, f, fleet) as i32;
                ai.flotillas[f].p[3] = size;
                let z = ai.flotillas[f].zone;
                if z.2 > 0 && z.3 > 0 {
                    ai.push(f, 5);
                }
            }
            5 => {
                if !fl.step {
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = fl.p[1] as u32;
                    fl.order = FleetOrder::Guard(fl.zone);
                    self.give_order(ai, f);
                    return;
                }
                ai.flotillas[f].worn = true;
                self.keep_order(ai, f);
                if self.report(ai, f) == Some(DONE) {
                    ai.push(f, 2);
                }
                if Computer::over(&ai.flotillas[f], now) {
                    ai.push(f, 10);
                }
                let n = Self::size(ai, f);
                ai.flotillas[f].count = n;
                if ai.flotillas[f].p[3] <= n {
                    ai.push(f, 7);
                }
                // Raiders of the harder levels strike early at a weak town.
                if !fleet
                    && (ai.difficulty == 1 || ai.difficulty == 2)
                    && n > ai.flotillas[f].p[4] + 1
                    && ai.enemy.is_some()
                    && self.weak_target(ai, f)
                {
                    ai.push(f, 0xc);
                }
            }
            7 => {
                if !fl.step {
                    let len = if fleet { 0x5dc } else { 1000 };
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = len;
                } else if Computer::over(&ai.flotillas[f], now) {
                    ai.push(f, 10);
                }
            }
            10 => {
                if ai.enemy.is_none() {
                    return;
                }
                if self.enemy_town(ai, f) {
                    ai.push(f, 0xc);
                }
            }
            0xc => {
                if !fl.step {
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = fl.p[2] as u32;
                    fl.order = FleetOrder::Attack(fl.area);
                    self.give_order(ai, f);
                    return;
                }
                ai.flotillas[f].wants = false;
                self.keep_order(ai, f);
                if self.report(ai, f) == Some(DONE) || Computer::over(&ai.flotillas[f], now) {
                    ai.push(f, 2);
                }
                let n = Self::size(ai, f);
                ai.flotillas[f].count = n;
                if n <= ai.flotillas[f].p[4] {
                    ai.push(f, 2);
                }
            }
            _ => {}
        }
        let p = ai.flotillas[f].p;
        if p[6] != 0 && (p[5] + p[6]) as u32 <= now && p[7] != 0 {
            ai.go(f, p[7]);
        }
    }

    /// How many ships the raiders wait for (0x485650; the fleet 0x485960):
    /// by difficulty and whether the town builds bombers (3), fire ships or
    /// fire rammers (8, 10), quick lime ships or ships of the dead (9, 11).
    fn target_size(&self, ai: &mut Computer, f: usize, fleet: bool) -> u32 {
        let b = ai.flotillas[f].base;
        if ai.bases.get(b).and_then(|b| b.site).is_none() {
            return ai.roll(3) + if fleet { 6 } else { 5 };
        }
        let o = ai.owner;
        let bomb = self.can_produce(o, 3);
        let fire = self.can_produce(o, 8) || self.can_produce(o, 10);
        let lime = self.can_produce(o, 9) || self.can_produce(o, 11);
        let d = ai.difficulty;
        if !fleet {
            match d {
                0 if bomb => match (fire, lime) {
                    (true, false) => (ai.rng.next() & 3) + 7,
                    (true, true) => ai.roll(5) + 8,
                    (false, false) => ai.roll(3) + 7,
                    (false, true) => (ai.rng.next() & 3) + 7,
                },
                1 if !bomb => ai.roll(3) + 5,
                1 => match (fire, lime) {
                    (true, true) => (ai.rng.next() & 3) + 9,
                    (true, false) => (ai.rng.next() & 3) + 7,
                    (false, true) => (ai.rng.next() & 3) + 7,
                    (false, false) => ai.roll(3) + 6,
                },
                2 if bomb => match (fire, lime) {
                    (false, false) => ai.roll(3) + 8,
                    (false, true) | (true, false) => ai.roll(3) + 10,
                    (true, true) => ai.roll(3) + 12,
                },
                0 | 2 => ai.roll(3) + 6,
                _ => 5,
            }
        } else {
            match d {
                1 => {
                    if !bomb {
                        (ai.rng.next() & 1) + 6
                    } else {
                        match (fire, lime) {
                            (false, false) => ai.roll(3) + 9,
                            (true, true) => ai.roll(5) + 12,
                            _ => ai.roll(3) + 12,
                        }
                    }
                }
                0 | 2 if bomb => match (fire, lime) {
                    (false, false) => ai.roll(3) + 8,
                    (false, true) | (true, false) => ai.roll(3) + 10,
                    (true, true) => ai.roll(3) + 12,
                },
                0 | 2 => ai.roll(3) + 6,
                _ => 5,
            }
        }
    }

    /// A weak enemy town to strike at (0x485fc0): one about which the enemy
    /// is weaker than the flotilla.
    fn weak_target(&mut self, ai: &mut Computer, f: usize) -> bool {
        let ours = self.fleet_strength(&ai.flotillas[f]);
        if ours < 1 {
            return false;
        }
        let Some(enemy) = ai.enemy else {
            return false;
        };
        let (w, h) = self.map_cells();
        let mut found: Vec<(i32, Cells)> = Vec::new();
        for i in 0..self.objects.len() {
            let o = &self.objects[i];
            if o.owner != Some((enemy / 4, enemy % 4)) {
                continue;
            }
            let weight = match self.class_of(i) {
                Some(TOWN) => 100,
                Some(SETTLEMENT) => 0x32,
                _ => continue,
            };
            let (x, y) = self.cell_of(i);
            let (x0, y0) = ((x - 3).max(0), (y - 3).max(0));
            let rect = (x0, y0, 8.min(w - x0), 8.min(h - y0));
            if self.strength_in(ai.owner, rect) < ours {
                found.push((weight, rect));
            }
        }
        if found.is_empty() {
            found = self.enemy_buildings(enemy);
        }
        if found.is_empty() {
            return false;
        }
        let weights: Vec<i32> = found.iter().map(|x| x.0).collect();
        ai.flotillas[f].area = found[ai.pick(&weights)].1;
        true
    }

    /// The enemy's town to raid (0x485be0): towns weigh 100, settlements
    /// 50; without any, any of its ships or buildings.
    fn enemy_town(&mut self, ai: &mut Computer, f: usize) -> bool {
        let Some(enemy) = ai.enemy else {
            return false;
        };
        let (w, h) = self.map_cells();
        let theirs: Vec<usize> = (0..self.objects.len())
            .filter(|&i| {
                self.objects[i].owner == Some((enemy / 4, enemy % 4))
                    && self.objects[i].combat.is_some()
            })
            .collect();
        let towns: Vec<(i32, usize)> = theirs
            .iter()
            .filter_map(|&i| match self.class_of(i) {
                Some(TOWN) => Some((100, i)),
                Some(SETTLEMENT) => Some((0x32, i)),
                _ => None,
            })
            .collect();
        let (pick, size, back) = if !towns.is_empty() {
            let weights: Vec<i32> = towns.iter().map(|x| x.0).collect();
            (towns[ai.pick(&weights)].1, 8, 3)
        } else {
            let all: Vec<(i32, usize)> = theirs
                .iter()
                .map(|&i| {
                    let w = if self.objects[i].ship.is_some()
                        || matches!(self.objects[i].class, ObjectClass::Building(_))
                    {
                        100
                    } else {
                        1
                    };
                    (w, i)
                })
                .collect();
            if all.is_empty() {
                return false;
            }
            let weights: Vec<i32> = all.iter().map(|x| x.0).collect();
            (all[ai.pick(&weights)].1, 7, 3)
        };
        let (x, y) = self.cell_of(pick);
        let (x0, y0) = ((x - back).max(0), (y - back).max(0));
        ai.flotillas[f].area = (x0, y0, size.min(w - x0), size.min(h - y0));
        true
    }

    /// Tasks 0xb and 0xd (0x47f460, 0x47f6b0): the reserve and the spare
    /// ships rest at the base and go for repairs; the reserve then hands
    /// its ships to the flotillas that want them.
    fn task_rest(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let fl = &ai.flotillas[f];
        match fl.state {
            0 => {
                ai.flotillas[f].p[2] = now as i32;
                ai.push(f, 5);
            }
            5 => {
                if !fl.step {
                    let len = ai.roll(0x1f5) + 500;
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = len;
                    self.read_zone(ai, f);
                    self.move_to_zone(ai, f);
                } else if self.report(ai, f) == Some(DONE) || Computer::over(&ai.flotillas[f], now)
                {
                    ai.push(f, 10);
                }
            }
            10 => {
                if !fl.step {
                    let len = ai.roll(0x1771) + 6000;
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = len;
                    fl.order = FleetOrder::Dock;
                    self.give_order(ai, f);
                } else if self.report(ai, f) == Some(DONE) {
                    if ai.flotillas[f].task == 0xb {
                        for id in std::mem::take(&mut ai.flotillas[f].ships) {
                            self.hand_ship(ai, id);
                        }
                    }
                    let next = ai.flotillas[f].p[4];
                    ai.go(f, next);
                    return;
                } else if Computer::over(&ai.flotillas[f], now) {
                    ai.push(f, 5);
                }
            }
            _ => {}
        }
        ai.task_end(f, now, 1, 2, 3);
    }

    /// Gives a reserve ship to the flotilla that wants its type most
    /// (0x486910: 0x4866d0, else by type, 0x47fd50).
    fn hand_ship(&mut self, ai: &mut Computer, id: u32) {
        let Some(kind) = self
            .object_index(id)
            .and_then(|i| self.objects[i].ship.as_ref())
            .map(|s| s.kind)
        else {
            return;
        };
        let mut best: Option<(i32, usize)> = None;
        for f in 0..ai.flotillas.len() {
            if let Some(prio) = self.want_of(ai, f, kind) {
                if prio > 0 && best.is_none_or(|b| prio > b.0) {
                    best = Some((prio, f));
                }
            }
        }
        let f = match best {
            Some((_, f)) => Some(f),
            None => ai.fleet_by_type(kind),
        };
        if let Some(f) = f {
            ai.flotillas[f].ships.push(id);
        }
    }

    /// How much flotilla `f` wants a ship of type `kind` (0x486670): the
    /// highest priority of its table's entries of the type, while it has
    /// fewer than entries.
    fn want_of(&self, ai: &Computer, f: usize, kind: u8) -> Option<i32> {
        let mut n = 0;
        let mut top = 0;
        for group in ai.wants_of(ai.flotillas[f].role) {
            for w in group.iter() {
                if w.0 == kind {
                    n += 1;
                    top = top.max(w.3 - w.2 * 2500 + 22767);
                }
            }
        }
        (n > 0 && self.type_counts(&ai.flotillas[f])[kind as usize] < n).then_some(top)
    }

    /// Sends the flotilla's worn ships to the reserve when its base has a
    /// dock (0x486970, every 375 ticks): boats and merchants under 25-45%
    /// of their hull, warships under 50-75%.
    fn send_worn(&mut self, ai: &mut Computer, f: usize, now: u32) {
        if ai.flotillas[f].worn_at + 0x177 > now {
            return;
        }
        ai.flotillas[f].worn_at = now;
        let b = ai.flotillas[f].base;
        if self.base_building(ai, b, DOCK).is_none() {
            return;
        }
        for id in ai.flotillas[f].ships.clone() {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let (Some(s), Some(c)) = (&self.objects[i].ship, &self.objects[i].combat) else {
                continue;
            };
            // Not while in a dock (state 2).
            if matches!(s.task, Some(Task::Repair { .. })) {
                continue;
            }
            let hull = (c.hull * 100.0 / c.max_hull.max(1.0)) as u32;
            let limit = match s.kind {
                0 | 1 => ai.roll(0x15) + 0x19,
                2..=7 => {
                    let l = ai.roll(0x1a) + 0x32;
                    // Or its crew: the mean of its items' mean conditions
                    // (0x4bc590; none counts 0) under 35-65.
                    let crew = ai.roll(0x1f) + 0x23;
                    if self.crew_condition(i) < crew {
                        101
                    } else {
                        l
                    }
                }
                _ => ai.roll(0x1a) + 0x32,
            };
            if hull < limit {
                ai.flotillas[f].ships.retain(|&x| x != id);
                let r = ai.reserve;
                ai.flotillas[r].ships.push(id);
            }
        }
    }

    /// A ship's crew condition (0x4bc590): the mean of the mean
    /// conditions of the items it carries; 0 with none.
    fn crew_condition(&self, i: usize) -> u32 {
        let units = self.objects[i].unit_conditions();
        let means: Vec<u32> = units
            .iter()
            .filter(|u| !u.is_empty())
            .map(|u| u.iter().map(|&c| c as u32).sum::<u32>() / u.len() as u32)
            .filter(|&m| m != 0)
            .collect();
        if means.is_empty() {
            0
        } else {
            means.iter().sum::<u32>() / means.len() as u32
        }
    }

    /// Task 0xc (0x47f050): found a settlement at the given cell (or the
    /// base's), then start over.
    fn task_found(&mut self, ai: &mut Computer, f: usize, now: u32) {
        let fl = &ai.flotillas[f];
        match fl.state {
            0 => {
                let fl = &mut ai.flotillas[f];
                fl.p[2] = now as i32;
                if fl.p[8] < 0 || fl.p[9] < 0 {
                    let b = fl.base;
                    let c = ai.bases.get(b).map_or((0, 0), |b| b.cell);
                    ai.flotillas[f].home = (c.0, c.1, 1, 1);
                } else {
                    fl.home = (fl.p[8], fl.p[9], 1, 1);
                }
                ai.flotillas[f].p[0] = 0x32;
                ai.push(f, 5);
            }
            5 => {
                if !fl.step {
                    let fl = &mut ai.flotillas[f];
                    fl.step = true;
                    fl.since = now;
                    fl.len = fl.p[1] as u32;
                    self.found(ai, f);
                } else {
                    self.keep_order(ai, f);
                    if self.report(ai, f) == Some(DONE) {
                        let next = ai.flotillas[f].p[5];
                        ai.go(f, next);
                        return;
                    }
                }
            }
            _ => {}
        }
        let p = ai.flotillas[f].p;
        if p[1] != 0 && (p[2] + p[1]) as u32 <= now {
            ai.go(f, p[3]);
        }
    }

    /// Order 0x8c: the flotilla's first ship founds class `p[0]` at the
    /// home cell.
    fn found(&mut self, ai: &mut Computer, f: usize) {
        let fl = &ai.flotillas[f];
        let cell = (fl.home.0, fl.home.1);
        let class = fl.p[0] as u32;
        let Some(&ship) = fl.ships.first() else {
            ai.flotillas[f].order = FleetOrder::None;
            return;
        };
        ai.flotillas[f].order = FleetOrder::Found(ship, cell, class);
        self.found_by(ship, cell, class);
    }

    /// Ship `ship` founds `class` at `cell`; while the price cannot be
    /// paid it sails there and waits (the order stands until the step's
    /// time is up). The move is a command, delivered at the next tick.
    /// (Not ported: the founding command 0x20 is given at once here, its
    /// answer deciding the move; the original's 0x4cbcf0 cannot see it.)
    pub(super) fn found_by(&mut self, ship: u32, cell: (i32, i32), class: u32) {
        if self.order_found(ship, cell.0, cell.1, class).is_ok() {
            return;
        }
        let t = TILE_SIZE as f32;
        let Some(i) = self.object_index(ship) else {
            return;
        };
        let (x, y) = (cell.0 as f32 * t + t / 2.0, cell.1 as f32 * t + t / 2.0);
        let o = &self.objects[i];
        if (o.x - x).hypot(o.y - y) > (FOUND_REACH as f32 + 0.5) * t {
            self.post(Order::Move { ship, x, y });
        }
    }

    /// The commander reads the spells asked (letters 6 and 7, 0x482160):
    /// at a place at most every 1500-3000 ticks, on a fleet every
    /// 3000-4500, sooner when urgent; the spell by the enemies about when
    /// none was named.
    fn read_spell_asks(&mut self, ai: &mut Computer, f: usize, now: u32) {
        for ask in std::mem::take(&mut ai.asks).into_iter().rev() {
            if ask.letter == 6 {
                let r = ai.roll(0x5dd);
                if r + 0x5dc + ai.flotillas[f].clocks[0] > now && !ask.urgent {
                    continue;
                }
                let kind = ask.kind.unwrap_or_else(|| {
                    if ask.enemies < 12 {
                        if ai.rng.next() & 1 == 1 {
                            11
                        } else {
                            14
                        }
                    } else if ask.enemies < 18 {
                        if ai.rng.next() & 1 == 1 {
                            6
                        } else {
                            11
                        }
                    } else if ai.roll(3) == 1 {
                        10
                    } else {
                        6
                    }
                });
                if self.computer_cast(ai, ask.base, kind, Some(ask.cell), ask.rect, None) {
                    ai.flotillas[f].clocks[0] = now;
                }
            } else {
                let r = ai.roll(0x5dd);
                let Some(fl) = ask.flotilla.filter(|&k| k < ai.flotillas.len()) else {
                    continue;
                };
                if (r + 3000 + ai.flotillas[f].clocks[1] > now && !ask.urgent)
                    || ai.flotillas[fl].ships.is_empty()
                {
                    continue;
                }
                let kind = ask.kind.unwrap_or_else(|| {
                    if ai.roll(3) == 1 {
                        9
                    } else if ai.rng.next() & 0xf == 5 {
                        16
                    } else if ai.rng.next() & 1 == 1 {
                        12
                    } else {
                        15
                    }
                });
                let base = ai.flotillas[fl].base;
                if self.computer_cast(ai, base, kind, None, None, Some(fl)) {
                    ai.flotillas[f].clocks[1] = now;
                }
            }
        }
    }

    /// Base `b`'s town casts spell `kind` (5-16, as 0x47ae10's; 0x42a500
    /// sends it as command 0x22) if one of the player's temples knows it
    /// and the player can pay (0x487640, 0x43fee0): at `cell`, over own
    /// things in `rect`, or on flotilla `fl`'s ships. The kinds go out as
    /// the temple's codes 1-8 and 12-15 (0x42a500), which it casts as
    /// `Magic.cpp`'s spells (0x42fc80): 5 a whirlpool, 6 a waterspout, 7 a
    /// reef, 8 a big reef, 9 the Flame (the temple brews it), 10
    /// lightning, 11 a fireball, 12 the phantom, 13 nothing (code 12 has no
    /// spell), 14 a sea monster, 15 and 16 the shields.
    fn computer_cast(
        &mut self,
        ai: &mut Computer,
        b: usize,
        kind: u32,
        cell: Option<(i32, i32)>,
        rect: Option<Cells>,
        fl: Option<usize>,
    ) -> bool {
        use crate::magic::Spell;
        let spell = match kind {
            5 => Spell::Whirlpool,
            6 => Spell::Waterspout,
            7 | 8 => Spell::Reef,
            14 => Spell::Monster,
            9 => Spell::Flame,
            10 => Spell::Lightning,
            11 => Spell::Fireball,
            12 => Spell::Phantom,
            15 => Spell::ProtectWeapons,
            16 => Spell::ProtectMagic,
            _ => return false,
        };
        let owner = ai.owner;
        if ai.bases.get(b).and_then(|b| b.site).is_none() || !ai.magic.knows(spell) {
            return false;
        }
        let temple = spell.temple().class();
        let has_temple = self.objects.iter().any(|o| {
            o.owner == Some(owner)
                && o.class == ObjectClass::Building(temple)
                && self.construction_at(o.id).is_none()
        });
        let (amber, fish) = spell.cost();
        let (have_a, have_f) = self.funds(owner);
        if !has_temple || have_a < amber || have_f < fish {
            return false;
        }
        let targets: Vec<u32> = match (rect, fl) {
            (_, Some(fl)) => ai.flotillas[fl].ships.clone(),
            (Some((x, y, w, h)), None) => (0..self.objects.len())
                .filter(|&i| {
                    let (cx, cy) = self.cell_of(i);
                    self.objects[i].owner == Some(owner)
                        && self.objects[i].combat.is_some()
                        && cx >= x
                        && cx < x + w
                        && cy >= y
                        && cy < y + h
                })
                .map(|i| self.objects[i].id)
                .collect(),
            _ => Vec::new(),
        };
        match spell {
            Spell::Flame => ai.flame += crate::magic::FLAME_SECONDS,
            Spell::Phantom | Spell::ProtectWeapons | Spell::ProtectMagic => {
                if targets.is_empty() {
                    return false;
                }
                self.pending_ship_casts
                    .extend(targets.iter().map(|&id| (spell, id)));
            }
            _ => {
                let Some((cx, cy)) = cell else {
                    return false;
                };
                if kind == 8 {
                    // A big reef of kind 1-3 (0x42f580 case 12).
                    let k = self.rng.next() % 3 + 1;
                    self.pending_reefs.push((cx, cy, k));
                } else {
                    self.pending_casts.push((spell, cx, cy, Some(owner)));
                }
            }
        }
        self.earn(owner, -(amber as i64), -(fish as i64));
        true
    }

    /// The commander's turn (0x482160): pick a new enemy when the last is
    /// gone, send the warships against it when strong enough, and found
    /// settlements.
    fn commander_turn(&mut self, ai: &mut Computer, f: usize, now: u32) {
        if !ai.flotillas[f].started {
            ai.flotillas[f].started = true;
            self.update_spared(ai);
            return;
        }
        {
            let fl = &mut ai.flotillas[f];
            if fl.last + fl.period > now {
                return;
            }
            fl.last = now;
        }
        if !ai.enemy.is_some_and(|e| self.alive(e)) {
            self.choose_enemy(ai);
        }
        // The grand attack.
        let r = ai.roll(0x5dd);
        if r + 0x5dc + ai.flotillas[f].clocks[3] < now && ai.enemy.is_some() {
            let first = match ai.difficulty {
                0 => ai.roll(0x1195) + 0x57e4,
                1 => ai.roll(0x1d4d) + 15000,
                _ => ai.roll(0x1195) + 0x34bc,
            };
            if first < now {
                ai.flotillas[f].clocks[3] = now;
                let enemy = ai.enemy.unwrap_or(0);
                let theirs = self
                    .objects
                    .iter()
                    .filter(|o| {
                        o.owner == Some((enemy / 4, enemy % 4))
                            && o.ship.as_ref().is_some_and(|s| (2..12).contains(&s.kind))
                    })
                    .count() as i32;
                let cap = if ai.difficulty == 0 { 0x1e } else { 0x23 };
                let need = ((theirs / 5) * 5 + 0xc).min(cap);
                let ready = self.ready_warships(ai);
                if need <= ready {
                    if let Some(rect) = self.attack_target(ai) {
                        // The Flame for the attack, now and then.
                        let r = ai.roll(0x5dd);
                        if (r + 3000 + ai.flotillas[f].clocks[1] <= now || ai.roll(6) == 3)
                            && !ai.bases.is_empty()
                            && self.computer_cast(ai, 0, 9, None, None, None)
                        {
                            ai.flotillas[f].clocks[1] = now;
                        }
                        let len = ai.roll(0x1d4d) as i32 + 15000;
                        let mut p = [0; 12];
                        p[0] = 0;
                        p[1] = rect.0;
                        p[2] = rect.1;
                        p[3] = rect.2;
                        p[4] = rect.3;
                        p[7] = len;
                        p[8] = 1;
                        p[9] = 1;
                        // To the guards, defence, raiders and the fleet
                        // (mask 0x3c0).
                        for k in 0..ai.flotillas.len() {
                            if (6..=9).contains(&ai.flotillas[k].role) {
                                ai.mail(
                                    k,
                                    Mail {
                                        kind: 1,
                                        at: now * 2,
                                        task: 8,
                                        p,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        self.read_spell_asks(ai, f, now);
        // A new settlement.
        let r = ai.roll(0x1195);
        if r + 0x1d4c + ai.flotillas[f].clocks[2] >= now {
            return;
        }
        let (w, h) = self.map_cells();
        let cap = (w * h / 500).min(ai.difficulty as i32 + 1);
        if cap <= ai.bases.len() as i32 {
            return;
        }
        // Only once the nation's unique ships are studied (+0x6e and the
        // like): easy 8 or 10, otherwise 9 or 11.
        let levels = self.levels_of(Some(ai.owner));
        let studied = if ai.difficulty == 0 {
            levels.get(Upgrade::Special(0)) > 0 || levels.get(Upgrade::Special(2)) > 0
        } else {
            levels.get(Upgrade::Special(1)) > 0 || levels.get(Upgrade::Special(3)) > 0
        };
        let has_ships = self
            .objects
            .iter()
            .any(|o| o.owner == Some(ai.owner) && o.ship.is_some());
        if !studied || !has_ships {
            return;
        }
        // The best site on the whole map (0x484d60).
        let Some(cell) = self.town_site(ai.owner, None, None) else {
            return;
        };
        let Some(fleet) = self.founders(ai, None, Some(cell)) else {
            return;
        };
        let len = ai.roll(0xbb9) as i32 + 3000;
        let mut p = [0; 12];
        p[0] = 0x32;
        p[1] = len;
        p[3..8].fill(1);
        p[8] = cell.0;
        p[9] = cell.1;
        ai.mail(
            fleet,
            Mail {
                kind: 1,
                at: now,
                task: 0xc,
                p,
            },
        );
        ai.flotillas[f].clocks[2] = now;
    }

    /// Warships free for a grand attack (message 0x66 summed, 0x4871a0).
    fn ready_warships(&self, ai: &Computer) -> i32 {
        ai.flotillas
            .iter()
            .filter(|f| match f.role {
                RAIDERS => f.task == 9 && f.state != 0xc,
                FLEET => f.task == 10 && f.state != 0xc,
                GUARD => f.task == 7,
                DEFENCE => f.task == 8 && f.p[0] < 0,
                _ => false,
            })
            .map(|f| f.ships.len() as i32)
            .sum()
    }

    /// The grand attack's target (0x487690): one of the enemy's towns (100)
    /// or settlements (50), else any of its buildings.
    fn attack_target(&mut self, ai: &mut Computer) -> Option<Cells> {
        let enemy = ai.enemy?;
        let (w, h) = self.map_cells();
        let mut found: Vec<(i32, Cells)> = Vec::new();
        for i in 0..self.objects.len() {
            if self.objects[i].owner != Some((enemy / 4, enemy % 4)) {
                continue;
            }
            let weight = match self.class_of(i) {
                Some(TOWN) => 100,
                Some(SETTLEMENT) => 0x32,
                _ => continue,
            };
            let (x, y) = self.cell_of(i);
            let (x0, y0) = ((x - 3).max(0), (y - 3).max(0));
            found.push((weight, (x0, y0, 8.min(w - x0), 8.min(h - y0))));
        }
        if found.is_empty() {
            found = self.enemy_buildings(enemy);
        }
        if found.is_empty() {
            return None;
        }
        let weights: Vec<i32> = found.iter().map(|x| x.0).collect();
        Some(found[ai.pick(&weights)].1)
    }

    /// Order 100: the flotilla sails to its zone's middle.
    fn move_to_zone(&mut self, ai: &mut Computer, f: usize) {
        let z = ai.flotillas[f].zone;
        let to = (z.0 + z.2 / 2, z.1 + z.3 / 2);
        ai.flotillas[f].order = FleetOrder::Move(to);
        self.give_order(ai, f);
    }

    /// Gives the flotilla's order to its ships. A gather order is the
    /// flotilla's gather zone (fleet order 0x78 from 0x483090: its area,
    /// its class; not decoded: the flags (flotilla `+0x154`), the port
    /// list its base town gives (0x43d220, 0x440c60) and the route
    /// 0x40b010 makes; here flags 0, the list made by 0x4c6c30, no
    /// route); any other ends it.
    fn give_order(&mut self, ai: &mut Computer, f: usize) {
        let order = ai.flotillas[f].order;
        let group = super::gather::ZoneGroup::Computer(ai.id(), f);
        if let FleetOrder::Gather(area, res) = order {
            let ships = ai.flotillas[f].ships.clone();
            self.start_zone(group, ships, area, res, 0, false);
            return;
        }
        self.end_zone(group);
        for id in ai.flotillas[f].ships.clone() {
            self.order_ship(ai, id, order);
        }
    }

    /// A flotilla's ships, for its gather zone.
    pub(super) fn computer_flotilla_ships(&self, player: u32, f: usize) -> Option<Vec<u32>> {
        self.computers
            .players
            .iter()
            .find(|c| c.id() == player)
            .and_then(|c| c.flotillas.get(f))
            .map(|fl| fl.ships.clone())
    }

    /// Ships that joined, or fell idle, take up the order again.
    fn keep_order(&mut self, ai: &mut Computer, f: usize) {
        let order = ai.flotillas[f].order;
        if let FleetOrder::Found(ship, cell, class) = order {
            let idle = self
                .object_index(ship)
                .and_then(|i| self.objects[i].ship.as_ref())
                .is_some_and(|s| s.task.is_none() && s.path.is_empty());
            if idle && !self.building_at(cell) {
                self.found_by(ship, cell, class);
            }
            return;
        }
        if !matches!(
            order,
            FleetOrder::Guard(_) | FleetOrder::Attack(_) | FleetOrder::Treasure(_)
        ) {
            return;
        }
        for id in ai.flotillas[f].ships.clone() {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let idle = self.objects[i]
                .ship
                .as_ref()
                .is_some_and(|s| s.task.is_none() && s.path.is_empty());
            if idle {
                self.order_ship(ai, id, order);
            }
        }
    }

    /// The flotilla's order to ship `id`: moves and the dock are commands
    /// through the outbox (0x520cc0, the navy's helpers 0x4cf070 and
    /// 0x4cd930), delivered at the next tick; the zones are the navy's
    /// own tasks, set at once.
    fn order_ship(&mut self, ai: &Computer, id: u32, order: FleetOrder) {
        let t = TILE_SIZE as f32;
        let world = |(x, y, w, h): Cells| {
            (
                (x as f32 * t, y as f32 * t),
                ((x + w) as f32 * t, (y + h) as f32 * t),
            )
        };
        let Some(i) = self.object_index(id) else {
            return;
        };
        match order {
            // The flotilla's own zone (`give_order`).
            FleetOrder::Gather(..) => {}
            FleetOrder::Guard(area) | FleetOrder::Attack(area) => {
                let (a, b) = world(area);
                let attack = matches!(order, FleetOrder::Attack(_));
                if self.order_zone(id, a, b, attack).is_err() {
                    let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                    self.post(Order::Move {
                        ship: id,
                        x: cx,
                        y: cy,
                    });
                }
            }
            FleetOrder::Move((x, y)) => {
                self.post(Order::Move {
                    ship: id,
                    x: x as f32 * t + t / 2.0,
                    y: y as f32 * t + t / 2.0,
                });
            }
            FleetOrder::Dock => {
                let dock = self
                    .objects
                    .iter()
                    .find(|o| {
                        o.owner == Some(ai.owner)
                            && o.class == ObjectClass::Building(DOCK)
                            && self.construction_at(o.id).is_none()
                    })
                    .map(|o| o.id);
                if let Some(d) = dock {
                    self.post(Order::Repair { ship: id, dock: d });
                }
            }
            FleetOrder::Treasure(area) => {
                let f = ai.flotillas.iter().position(|fl| fl.ships.contains(&id));
                self.hunt_treasure(ai, f, i, area)
            }
            FleetOrder::Found(..) | FleetOrder::None => {}
        }
    }

    /// One ship of a treasure hunt (0x4da3e0): carrying treasures for the
    /// settlement it brings them home (0x4d9f50; here alongside the base's
    /// town), else it sails for the nearest treasure in the area no other
    /// ship of the flotilla is after (0x4cf070). (Not ported: the hunt's
    /// treasure command is given at once here, a refused one trying the
    /// next; the home move is a command, delivered at the next tick.)
    fn hunt_treasure(&mut self, ai: &Computer, f: Option<usize>, i: usize, area: Cells) {
        let id = self.objects[i].id;
        let carrying = self.objects[i]
            .ship
            .as_ref()
            .is_some_and(|s| s.treasures.iter().any(|tr| matches!(tr.kind, 6 | 7 | 27)));
        if carrying {
            let home = f
                .and_then(|f| ai.bases.get(ai.flotillas[f].base))
                .and_then(|b| b.site)
                .and_then(|s| self.object_index(s));
            if let Some(t) = home {
                let (x, y) = (self.objects[t].x, self.objects[t].y);
                self.post(Order::Move { ship: id, x, y });
            }
            return;
        }
        let taken: Vec<u32> = f
            .map(|f| ai.flotillas[f].ships.as_slice())
            .unwrap_or_default()
            .iter()
            .filter_map(|&s| self.object_index(s))
            .filter_map(|k| match self.objects[k].ship.as_ref()?.task {
                Some(Task::Take { target }) => Some(target),
                _ => None,
            })
            .collect();
        let (x, y) = (self.objects[i].x, self.objects[i].y);
        let mut near: Vec<(f32, u32)> = self
            .treasures_in(area)
            .filter(|&k| !taken.contains(&self.objects[k].id))
            .map(|k| {
                let o = &self.objects[k];
                ((o.x - x).hypot(o.y - y), o.id)
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, t) in near {
            if self.order_take(id, t).is_ok() {
                return;
            }
        }
    }

    /// The flotilla's order's report (the navy's query): `DONE` when it
    /// is over (a move arrived, an attacked area cleared, a founding ship
    /// done, the ships mended).
    fn report(&self, ai: &Computer, f: usize) -> Option<u32> {
        let fl = &ai.flotillas[f];
        let ships = || fl.ships.iter().filter_map(|&id| self.object_index(id));
        let done = match fl.order {
            FleetOrder::None => true,
            FleetOrder::Move(_) => ships().all(|i| {
                self.objects[i]
                    .ship
                    .as_ref()
                    .is_none_or(|s| s.path.is_empty())
            }),
            FleetOrder::Attack(area) => fl.ships.is_empty() || self.foes_in(ai.owner, area) == 0,
            FleetOrder::Found(ship, cell, _) => {
                !fl.ships.contains(&ship)
                    || self.building_at(cell) && {
                        let i = self.object_index(ship);
                        i.is_none_or(|i| {
                            !matches!(
                                self.objects[i].ship.as_ref().and_then(|s| s.task),
                                Some(Task::Found { .. })
                            )
                        })
                    }
            }
            FleetOrder::Dock => ships().all(|i| {
                self.objects[i]
                    .combat
                    .as_ref()
                    .is_none_or(|c| c.hull >= c.max_hull)
                    && !matches!(
                        self.objects[i].ship.as_ref().and_then(|s| s.task),
                        Some(Task::Repair { .. })
                    )
            }),
            FleetOrder::Treasure(area) => self.treasures_in(area).next().is_none(),
            FleetOrder::Gather(..) | FleetOrder::Guard(_) => false,
        };
        done.then_some(DONE)
    }
}

impl Computer {
    /// Picks an index by weight (0x488540).
    fn pick(&mut self, weights: &[i32]) -> usize {
        if weights.len() == 1 {
            return 0;
        }
        let total: i32 = weights.iter().sum();
        let r = (self.rng.next() % (total.max(0) as u32 + 1)) as i32;
        let mut acc = 0;
        for (k, &w) in weights.iter().enumerate() {
            acc += w;
            if r <= acc {
                return k;
            }
        }
        0
    }

    /// Whether no flotilla wants ships of type `kind` and the reserve has
    /// none (0x4867e0, for boats and merchants).
    fn nobody_wants(&self, kind: u8) -> bool {
        if kind > 1 {
            return false;
        }
        !self.flotillas.iter().any(|f| {
            self.wants_of(f.role)
                .iter()
                .flat_map(|g| g.iter())
                .any(|w| w.0 == kind)
        })
    }

    /// The flotilla of the last base a reserve ship nobody wants goes to
    /// (0x47fd50): boats and merchants to the gatherers, bombers and
    /// rammers to the guards, royal ships and boarders to the raiders,
    /// killers and fire ships to the defence or the raiders, quick lime
    /// ships and ships of the dead to the guards or the raiders.
    fn fleet_by_type(&mut self, kind: u8) -> Option<usize> {
        let f = match kind {
            0 | 1 => {
                if self.rng.next() & 1 != 0 {
                    self.by_role[FISH as usize]
                } else {
                    self.by_role[AMBER as usize]
                }
            }
            2 | 4 => self.by_role[RAIDERS as usize],
            3 | 5 => self.by_role[GUARD as usize],
            6 | 7 | 8 | 10 => {
                if self.rng.next() & 1 == 0 {
                    self.by_role[DEFENCE as usize]
                } else {
                    self.by_role[RAIDERS as usize]
                }
            }
            9 | 11 => {
                if self.rng.next() & 1 != 0 {
                    self.by_role[GUARD as usize]
                } else {
                    self.by_role[RAIDERS as usize]
                }
            }
            _ => return None,
        };
        (f < self.flotillas.len()).then_some(f)
    }

    /// The flotilla a ship it starts with goes to without a request
    /// (0x484170): boats and merchants to the gatherers, bombers to the
    /// guards or the fleet, royal ships, boarders and rammers to the
    /// defence or the raiders, the rest to a flotilla picked at random.
    fn fleet_for_type(&mut self, kind: u8) -> Option<usize> {
        let of = |roles: &[u8], s: &Self| -> Vec<usize> {
            (0..s.flotillas.len())
                .filter(|&k| roles.contains(&s.flotillas[k].role))
                .collect()
        };
        let pool = match kind {
            0 | 1 => of(&[AMBER, FISH], self),
            2 | 4 | 5 => of(&[DEFENCE, RAIDERS], self),
            3 => of(&[GUARD, FLEET], self),
            _ => {
                let n = self.flotillas.len() as u32;
                if n < 4 {
                    return Some(2.min(self.flotillas.len().saturating_sub(1)));
                }
                return Some((self.roll(n - 2) + 2) as usize);
            }
        };
        if pool.is_empty() {
            return None;
        }
        let k = if pool.len() > 1 {
            self.roll(pool.len() as u32) as usize
        } else {
            0
        };
        Some(pool[k])
    }
}

impl LandFleet {
    /// Queues an entry (0x42c430's generic branch): before the first of
    /// lower priority, after its equals (compared unsigned).
    fn queue_entry(&mut self, entry: Entry) {
        let at = self
            .queue
            .iter()
            .position(|e| (e.prio as u32) < entry.prio as u32)
            .unwrap_or(self.queue.len());
        self.queue.insert(at, entry);
    }

    /// A building of entry `i`'s build went up (0x428e10): every other
    /// build of the class under way at the same town counts one more
    /// before it and waits afresh.
    fn built(&mut self, i: usize, base: usize, op: Op) {
        for (k, e) in self.queue.iter_mut().enumerate() {
            if let Job::Part {
                base: b, part: p, ..
            } = &mut e.job
            {
                if k != i && p.state == 2 && *b == base && p.op == op {
                    p.before += 1;
                    p.tries = op.tries_under_way();
                }
            }
        }
    }
}

impl Computer {
    /// The state of the order asked as `ask` (0x482ff0 -> 0x42c870): the
    /// first queue entry asking the same; one done (3) or failed (4) is
    /// taken off the queue; 5 when there is none.
    fn ask_state(&mut self, ask: Option<Ask>) -> u32 {
        let Some(ask) = ask else {
            return 5;
        };
        let Some(k) = self
            .land
            .queue
            .iter()
            .position(|e| matches!(e.job, Job::Ask { ask: a, .. } if a == ask))
        else {
            return 5;
        };
        let s = self.land.queue[k].state();
        if s == 3 || s == 4 {
            self.land.queue.remove(k);
        }
        s as u32
    }

    /// Starts the op-1000 entry `i` (0x42c050): puts its order before the
    /// first of no higher priority, the entry under way until the order
    /// ends; an order not in the table fails it. Either wakes the fleet.
    fn accept_order(&mut self, i: usize) {
        let Job::Ask { ask, flags, .. } = self.land.queue[i].job else {
            return;
        };
        let (code, prio, base, fleet) = ask;
        let code = if code == 0x86 {
            // The nation's unique ships' study.
            match self.nation() {
                1 => 0x73,
                3 => 0x74,
                _ => 0,
            }
        } else {
            code
        };
        self.land.woken = true;
        // Only code 0 is refused (0x86 for the other nations); every other
        // order the computer asks is in the table.
        let table = order_parts(code);
        if code == 0 || table.is_empty() {
            self.land.queue[i].set_state(4);
            return;
        }
        // Flag 0x80 (all the computer's orders): every part takes the
        // order's priority; flag 8 sends them to the base's town and 0x400
        // the ships to the flotilla (the order's `base` and `fleet`).
        let parts: Vec<Part> = table
            .iter()
            .map(|&(op, once, after)| Part {
                op,
                once,
                // Field 8 is set only in order 0x5c's part.
                free: code == 0x5c,
                after,
                ..Default::default()
            })
            .collect();
        let id = self.next_order;
        self.next_order += 1;
        let at = self
            .orders
            .iter()
            .position(|o| o.prio as u32 <= prio as u32)
            .unwrap_or(self.orders.len());
        self.orders.insert(
            at,
            TownOrder {
                id,
                prio,
                flags,
                base,
                fleet,
                parts,
                state: 0,
            },
        );
        if let Job::Ask { state, order, .. } = &mut self.land.queue[i].job {
            *state = 2;
            *order = Some(id);
        }
    }

    /// Queues a part of order `k` (0x42c430 with origin 2).
    fn queue_part(&mut self, k: usize, index: usize) {
        let o = &self.orders[k];
        let mut part = o.parts[index];
        part.state = 0;
        part.tries = part.op.tries();
        part.before = 0;
        self.land.queue_entry(Entry {
            prio: o.prio,
            job: Job::Part {
                order: o.id,
                index,
                base: o.base,
                fleet: o.fleet,
                part,
            },
        });
    }

    /// The first half of a pass (0x42b910, the orders +0x34): each order
    /// queues the parts it is ready for; an order whose parts are all done,
    /// or one of whose parts failed, ends, its op-1000 entry done or failed
    /// and the order forgotten (its parts still queued run on).
    fn queue_order_parts(&mut self) {
        let mut k = 0;
        while k < self.orders.len() {
            if !matches!(self.orders[k].state, 0 | 2) {
                k += 1;
                continue;
            }
            self.orders[k].state = 2;
            let flags = self.orders[k].flags;
            let mut all_done = true;
            for p in 0..self.orders[k].parts.len() {
                let part = self.orders[k].parts[p];
                match part.state {
                    0 => match part.after {
                        After::Nothing | After::Again => {
                            self.queue_part(k, p);
                            self.orders[k].parts[p].state = 2;
                        }
                        After::Part(d) => match self.orders[k].parts.get(d).map(|x| x.state) {
                            Some(4) => self.orders[k].state = 4,
                            Some(3) => {
                                self.queue_part(k, p);
                                self.orders[k].parts[p].state = 2;
                            }
                            _ => {}
                        },
                        After::All => {
                            if all_done {
                                self.queue_part(k, p);
                                self.orders[k].parts[p].state = 2;
                            }
                        }
                    },
                    2 if part.after == After::Again => {
                        let code = part.op.code();
                        let waiting = self.land.queue.iter().any(|e| {
                            matches!(e.job, Job::Part { part: q, .. }
                                if q.op.code() == code && q.state <= 1)
                        });
                        if !waiting {
                            self.queue_part(k, p);
                        }
                    }
                    4 => {
                        if flags & 0x10 != 0 {
                            self.queue_part(k, p);
                            self.orders[k].parts[p].state = 2;
                        } else {
                            self.orders[k].state = 4;
                        }
                    }
                    _ => {}
                }
                if self.orders[k].parts[p].state != 3 {
                    all_done = false;
                }
            }
            let failed = self.orders[k].state == 4;
            if !all_done && !failed {
                k += 1;
                continue;
            }
            if all_done && flags & 0x40 != 0 {
                // It repeats.
                for p in &mut self.orders[k].parts {
                    p.state = 0;
                }
                k += 1;
                continue;
            }
            let id = self.orders[k].id;
            let Some(e) = self
                .land
                .queue
                .iter_mut()
                .find(|e| matches!(e.job, Job::Ask { order: Some(o), .. } if o == id))
            else {
                k += 1;
                continue;
            };
            e.set_state(if failed { 4 } else { 3 });
            self.orders.remove(k);
        }
    }
}

/// How a part of a town's order went this pass.
enum PartStep {
    Done,
    /// Ordered; the count of buildings before.
    Started(usize),
    /// Not yet: a pass less to try.
    Wait,
    /// Waiting for the price, as long as it takes (the town's refusals
    /// -1 to -3 with the order's flags 1, 2, 4: 0x428a70).
    Hold,
    /// A ship part held by the town's -1 (busy, or no yard with its berth
    /// free; flag 4): no try spent, counted towards the town's berth
    /// clearing (0x428f20 at 0x4295cb).
    Berth,
    Failed,
}

/// The resource of an original resource class.
fn resource(class: i32) -> Resource {
    match class {
        AMBER_CLASS => Resource::Amber,
        WHALE_CLASS => Resource::Leviathan,
        _ => Resource::Fish,
    }
}

/// The upgrade of a study number (0x42a860), if ported.
fn study(n: u8, _nation: u32) -> Option<Upgrade> {
    Some(match n {
        1 => Upgrade::Weapons,
        2 => Upgrade::Ram,
        3 => Upgrade::Armour,
        17 => Upgrade::Speed,
        18 => Upgrade::Reach(0),
        19 => Upgrade::Arms(0),
        20 => Upgrade::Rate(0),
        21 => Upgrade::Reach(1),
        22 => Upgrade::Arms(1),
        23 => Upgrade::Rate(1),
        24 => Upgrade::Reach(2),
        25 => Upgrade::Arms(2),
        26 => Upgrade::Rate(2),
        27..=29 => Upgrade::RamKind(n - 27),
        30..=32 => Upgrade::ArmourKind(n - 30),
        35..=38 => Upgrade::Special(n - 35),
        _ => return None,
    })
}

/// `cluster` (0x559070): one raster pass over the cells' box, each cell
/// taking a label found within `r` to its right or in the `r` rows below
/// (`max + min / 2`), else a new one, and handing it on there. Labels from
/// 1; their count.
fn cluster(cells: &[(i32, i32)], r: i32) -> (Vec<u32>, u32) {
    if cells.is_empty() {
        return (Vec::new(), 0);
    }
    let x0 = cells.iter().map(|c| c.0).min().unwrap();
    let y0 = cells.iter().map(|c| c.1).min().unwrap();
    let w = cells.iter().map(|c| c.0).max().unwrap() - x0 + 1;
    let h = cells.iter().map(|c| c.1).max().unwrap() - y0 + 1;
    let dist = |dx: i32, dy: i32| {
        let (a, b) = (dx.abs(), dy.abs());
        (2 * a.max(b) + a.min(b)) / 2
    };
    let at = |x: i32, y: i32| (y * w + x) as usize;
    let mut grid = vec![0i64; (w * h) as usize];
    for &(x, y) in cells {
        grid[at(x - x0, y - y0)] = -1;
    }
    let mut next = 1i64;
    for y in 0..h {
        for x in 0..w {
            let mut v = grid[at(x, y)];
            if v == 0 {
                continue;
            }
            if v == -1 {
                for i in 1..=r {
                    if x + i >= w {
                        break;
                    }
                    if grid[at(x + i, y)] > 0 {
                        v = grid[at(x + i, y)];
                        break;
                    }
                }
                if v == -1 {
                    'below: for k in 1..=r {
                        let yy = y + k;
                        if yy >= h {
                            break;
                        }
                        for c in (x - r).max(0)..=x + r {
                            if c >= w {
                                break;
                            }
                            if grid[at(c, yy)] > 0 && dist(c - x, k) <= r {
                                v = grid[at(c, yy)];
                                break 'below;
                            }
                        }
                    }
                }
                if v == -1 {
                    v = next;
                    next += 1;
                }
                grid[at(x, y)] = v;
            }
            for i in 1..=r {
                if x + i >= w {
                    break;
                }
                if grid[at(x + i, y)] == -1 {
                    grid[at(x + i, y)] = v;
                }
            }
            for k in 1..=r {
                let yy = y + k;
                if yy >= h {
                    break;
                }
                for c in (x - r).max(0)..=x + r {
                    if c >= w {
                        break;
                    }
                    if grid[at(c, yy)] == -1 && dist(c - x, k) <= r {
                        grid[at(c, yy)] = v;
                    }
                }
            }
        }
    }
    let labels = cells
        .iter()
        .map(|&(x, y)| grid[at(x - x0, y - y0)] as u32)
        .collect();
    (labels, (next - 1) as u32)
}

/// A waiting zone's rank (0x408bf0), least first.
type ZoneKey = (i32, i32, i32, i32, i32, i32);

/// The parts of a town's order (the land fleet's table at 0x5ac7c8, each
/// part 0xcc bytes: operation, flags (0x20 "once"), class or number,
/// field 20 what it waits for).
fn order_parts(code: u32) -> Vec<(Op, bool, After)> {
    use After::{All, Nothing, Part};
    use Op::{Build, Ship, Study, Upgrade as Up};
    match code {
        2 => vec![
            (Build(PORT), true, Nothing),
            (Build(WORKSHOP), true, All),
            (Build(SHIPYARD), true, All),
            (Build(SMITHY), true, All),
            (Up(SETTLEMENT), true, All),
            (Up(SHIPYARD), true, Part(4)),
            (Build(SCHOOL), true, Part(5)),
        ],
        3 => vec![
            (Build(TEMPLE_OF_THE_SEA), true, Nothing),
            (Build(TEMPLE_OF_WAR), true, Nothing),
        ],
        48 => vec![(Build(0x35), false, Nothing)],
        50 => vec![(Build(0x34), false, Nothing)],
        52 => vec![(Build(0x36), false, Nothing)],
        0x50..=0x5b => vec![(Ship((code - 0x50) as u8), false, Nothing)],
        // Field 8 = 1: built free (`Part::free`).
        0x5c => vec![(Ship(0), false, Nothing)],
        96 => vec![(Op::Repair, false, After::Again)],
        97 => vec![(Build(PORT), false, Nothing)],
        98 => vec![
            (Build(SHIPYARD), false, Nothing),
            (Up(SHIPYARD), false, All),
        ],
        0x73 => vec![(Study(35), true, Nothing), (Study(36), true, All)],
        0x74 => vec![(Study(37), true, Nothing), (Study(38), true, All)],
        // Six studies at each temple, each after the one before (0x5ac7c8).
        117 => (0..12)
            .map(|k| {
                let n = if k < 6 { 33 } else { 34 };
                let after = if k % 6 == 0 { Nothing } else { Part(k - 1) };
                (Study(n), true, after)
            })
            .collect(),
        118 => vec![(Study(1), true, Nothing)],
        119 => vec![(Study(2), true, Nothing), (Study(2), true, All)],
        120 => vec![(Study(3), true, Nothing), (Study(3), true, All)],
        128 => vec![(Study(18), true, Nothing), (Study(19), true, All)],
        129 => vec![
            (Study(21), true, Nothing),
            (Study(22), true, All),
            (Study(23), true, All),
        ],
        130 => vec![
            (Study(24), true, Nothing),
            (Study(25), true, All),
            (Study(26), true, All),
        ],
        131 => vec![
            (Study(30), true, Nothing),
            (Study(31), true, All),
            (Study(32), true, All),
        ],
        132 => vec![
            (Study(27), true, Nothing),
            (Study(28), true, All),
            (Study(29), true, All),
        ],
        133 => vec![(Study(17), true, Nothing)],
        _ => Vec::new(),
    }
}

// The base building scripts (0x47faf0): town orders in turn. Easy
// 0x5b6d90 (first base) and 0x5b6f30, normal 0x5b7de0 and 0x5b7f70, hard
// 0x5b9330 and 0x5b94d0; after a full round 0x5b6fe0.
// Each entry 16 bytes: the order, 0, a weight, 1; the order's priority is
// 22767 + weight + 1, and 100 more for order 2 (0x47d2e0).
const SCRIPT_0: &[(u32, i32)] = &[
    (2, 40),
    (96, 40),
    (50, 40),
    (118, 40),
    (48, 40),
    (52, 40),
    (119, 40),
    (52, 40),
    (120, 40),
    (97, 40),
    (98, 40),
    (130, 40),
    (129, 40),
    (131, 40),
    (132, 40),
    (128, 40),
    (134, 40),
    (3, 40),
    (52, 40),
    (117, 40),
    (130, 10),
    (129, 10),
    (131, 10),
    (132, 10),
    (128, 10),
];
const SCRIPT_0B: &[(u32, i32)] = &[
    (2, 40),
    (96, 40),
    (52, 40),
    (52, 40),
    (52, 40),
    (97, 40),
    (52, 40),
    (52, 40),
    (3, 40),
    (98, 40),
];
const SCRIPT_1: &[(u32, i32)] = &[
    (2, 40),
    (96, 40),
    (118, 40),
    (119, 40),
    (120, 40),
    (134, 40),
    (48, 22),
    (52, 22),
    (98, 40),
    (97, 40),
    (130, 18),
    (129, 18),
    (131, 18),
    (132, 18),
    (128, 18),
    (3, 18),
    (117, 18),
    (48, 10),
    (52, 10),
    (130, 10),
    (129, 10),
    (131, 10),
    (132, 10),
    (128, 10),
];
const SCRIPT_1B: &[(u32, i32)] = SCRIPT_0B;
const SCRIPT_2: &[(u32, i32)] = &[
    (2, 40),
    (96, 40),
    (118, 40),
    (50, 40),
    (97, 40),
    (3, 40),
    (117, 40),
    (98, 40),
    (119, 40),
    (48, 40),
    (134, 80),
    (52, 40),
    (120, 40),
    (52, 40),
    (130, 40),
    (131, 40),
    (129, 40),
    (132, 40),
    (128, 40),
    (52, 40),
    (130, 40),
    (131, 40),
    (129, 40),
    (132, 40),
    (128, 40),
];
const SCRIPT_2B: &[(u32, i32)] = SCRIPT_0B;
const SCRIPT_AGAIN: &[(u32, i32)] = &[(2, 40), (96, 40), (52, 40), (48, 40)];

// The want tables (0x5b5fa0 and the others; see `Computer::wants_of`):
// groups of alternatives (ship type, how many, flag, priority).
const GATHERERS: &[&[Want]] = &[&[(1, 1, 0, 45)], &[(0, 1, 0, 45)], &[(1, 2, 1, 45)]];

const GUARD_0P: &[&[Want]] = &[
    &[(2, 1, 0, 34)],
    &[(2, 2, 0, 34)],
    &[(2, 3, 0, 34)],
    &[(4, 1, 0, 34)],
    &[(4, 2, 0, 34)],
    &[(5, 1, 0, 34)],
    &[(5, 2, 0, 34)],
    &[(3, 1, 0, 35)],
    &[(3, 2, 0, 35)],
    &[(3, 3, 0, 35)],
    &[(3, 4, 0, 35)],
    &[(3, 5, 0, 35)],
    &[(10, 1, 1, 34)],
    &[(11, 1, 1, 34)],
];
const GUARD_0: &[&[Want]] = &[
    &[(2, 1, 0, 34)],
    &[(2, 2, 0, 34)],
    &[(2, 3, 0, 34)],
    &[(4, 1, 0, 34)],
    &[(4, 2, 0, 34)],
    &[(5, 1, 0, 34)],
    &[(5, 2, 0, 34)],
    &[(3, 1, 0, 35)],
    &[(3, 2, 0, 35)],
    &[(3, 3, 0, 35)],
    &[(8, 1, 1, 34)],
    &[(9, 1, 1, 34)],
    &[(8, 2, 1, 34)],
    &[(9, 2, 1, 34)],
];
const GUARD_1P: &[&[Want]] = &[
    &[(2, 1, 0, 34)],
    &[(2, 2, 0, 34)],
    &[(4, 1, 0, 34)],
    &[(4, 2, 0, 34)],
    &[(5, 1, 0, 34)],
    &[(3, 1, 0, 35)],
    &[(3, 2, 0, 35)],
    &[(3, 3, 0, 35)],
    &[(3, 4, 0, 35)],
    &[(10, 1, 1, 34)],
    &[(11, 1, 1, 34)],
    &[(3, 5, 1, 34)],
    &[(3, 6, 1, 34)],
];
const GUARD_1: &[&[Want]] = &[
    &[(2, 1, 0, 34)],
    &[(2, 2, 0, 34)],
    &[(4, 1, 0, 34)],
    &[(4, 2, 0, 34)],
    &[(5, 1, 0, 34)],
    &[(3, 1, 0, 35)],
    &[(3, 2, 0, 35)],
    &[(3, 3, 0, 35)],
    &[(3, 4, 0, 35)],
    &[(8, 1, 1, 34)],
    &[(9, 1, 1, 34)],
    &[(8, 2, 1, 34)],
    &[(9, 2, 1, 34)],
];
const GUARD_2P: &[&[Want]] = &[
    &[(10, 1, 0, 34), (3, 1, 0, 34), (6, 1, 0, 34)],
    &[(11, 1, 0, 34), (3, 2, 0, 34), (7, 1, 0, 34)],
    &[(2, 1, 0, 34)],
    &[(2, 2, 0, 34)],
    &[(4, 1, 0, 34)],
    &[(11, 2, 0, 34), (4, 2, 0, 34)],
    &[(10, 2, 0, 34), (5, 1, 0, 34)],
    &[(3, 3, 0, 34), (5, 2, 0, 34)],
    &[(3, 4, 0, 34), (2, 3, 0, 40)],
    &[(3, 5, 0, 34), (2, 4, 0, 40)],
    &[(10, 3, 1, 34)],
    &[(11, 3, 1, 34)],
    &[(3, 6, 0, 34)],
    &[(3, 7, 0, 34)],
];
const GUARD_2: &[&[Want]] = &[
    &[(8, 1, 0, 34), (3, 1, 0, 34), (6, 1, 0, 34)],
    &[(9, 1, 0, 34), (3, 2, 0, 34), (7, 1, 0, 34)],
    &[(2, 1, 0, 34)],
    &[(2, 2, 0, 34)],
    &[(4, 1, 0, 34)],
    &[(9, 2, 0, 34), (4, 2, 0, 34)],
    &[(8, 2, 0, 34), (5, 1, 0, 34)],
    &[(3, 3, 0, 34), (5, 2, 0, 34)],
    &[(3, 4, 0, 34), (2, 3, 0, 40)],
    &[(3, 5, 0, 34), (2, 4, 0, 40)],
    &[(8, 3, 1, 34)],
    &[(9, 3, 1, 34)],
    &[(8, 4, 1, 34)],
    &[(9, 4, 1, 34)],
];

const DEFENCE_0: &[&[Want]] = &[
    &[(6, 1, 0, 36)],
    &[(7, 1, 0, 36)],
    &[(4, 1, 0, 35)],
    &[(4, 2, 0, 35)],
    &[(5, 1, 0, 35)],
    &[(5, 2, 0, 35)],
    &[(2, 1, 0, 35)],
    &[(2, 2, 0, 35)],
    &[(8, 1, 1, 35)],
    &[(10, 1, 1, 35)],
    &[(9, 1, 1, 35)],
    &[(11, 1, 1, 35)],
];
const DEFENCE_1: &[&[Want]] = &[
    &[(6, 1, 0, 35)],
    &[(7, 1, 0, 35)],
    &[(4, 1, 0, 35)],
    &[(4, 2, 0, 35)],
    &[(5, 1, 0, 35)],
    &[(2, 1, 0, 35)],
    &[(2, 2, 0, 35)],
    &[(2, 3, 0, 35)],
    &[(8, 1, 1, 35)],
    &[(10, 1, 1, 35)],
    &[(9, 1, 1, 35)],
    &[(11, 1, 1, 35)],
];
const DEFENCE_2: &[&[Want]] = &[
    &[(10, 1, 0, 35), (8, 1, 0, 35), (6, 1, 0, 35)],
    &[(11, 1, 0, 35), (9, 1, 0, 35), (7, 1, 0, 35)],
    &[(4, 1, 0, 35)],
    &[(11, 2, 0, 35), (9, 2, 0, 35), (4, 2, 0, 35)],
    &[(5, 1, 0, 35)],
    &[(10, 2, 0, 35), (8, 2, 0, 35), (5, 2, 0, 35)],
    &[(2, 1, 0, 35)],
    &[(2, 2, 0, 35)],
    &[(8, 3, 1, 35)],
    &[(10, 3, 1, 35)],
    &[(9, 3, 1, 35)],
    &[(11, 3, 1, 35)],
];

const BUILDERS_0: &[&[Want]] = &[
    &[(1, 1, 0, 25)],
    &[(7, 1, 0, 24)],
    &[(8, 1, 1, 24)],
    &[(10, 1, 1, 24)],
];
const BUILDERS_1: &[&[Want]] = &[
    &[(1, 1, 0, 25)],
    &[(6, 1, 0, 24)],
    &[(8, 1, 1, 24)],
    &[(10, 1, 1, 24)],
];
const BUILDERS_2: &[&[Want]] = &[
    &[(1, 1, 0, 25)],
    &[(10, 1, 0, 24), (8, 1, 0, 24), (7, 1, 0, 24)],
    &[(2, 1, 1, 24)],
    &[(2, 2, 1, 24)],
];

const RAIDERS_0P: &[&[Want]] = &[
    &[(6, 1, 0, 20)],
    &[(7, 1, 0, 20)],
    &[(4, 1, 0, 20)],
    &[(5, 1, 0, 20)],
    &[(2, 1, 0, 20)],
    &[(2, 2, 0, 20)],
    &[(2, 3, 0, 20)],
    &[(2, 4, 0, 20)],
    &[(3, 1, 0, 21)],
    &[(3, 2, 0, 21)],
    &[(10, 1, 1, 20)],
    &[(3, 3, 0, 21)],
    &[(11, 1, 1, 20)],
    &[(3, 4, 0, 21)],
];
const RAIDERS_0: &[&[Want]] = &[
    &[(6, 1, 0, 20)],
    &[(7, 1, 0, 20)],
    &[(4, 1, 0, 20)],
    &[(5, 1, 0, 20)],
    &[(2, 1, 0, 20)],
    &[(2, 2, 0, 20)],
    &[(2, 3, 0, 20)],
    &[(2, 4, 0, 20)],
    &[(3, 1, 0, 21)],
    &[(3, 2, 0, 21)],
    &[(8, 1, 1, 20)],
    &[(8, 2, 1, 20)],
    &[(9, 1, 1, 20)],
    &[(9, 2, 1, 20)],
];
const RAIDERS_1P: &[&[Want]] = &[
    &[(4, 1, 0, 20)],
    &[(5, 1, 0, 20)],
    &[(2, 1, 0, 20)],
    &[(2, 2, 0, 20)],
    &[(2, 3, 0, 20)],
    &[(2, 4, 0, 20)],
    &[(3, 1, 0, 20)],
    &[(3, 2, 0, 20)],
    &[(10, 1, 1, 20)],
    &[(3, 4, 0, 21)],
    &[(11, 1, 1, 20)],
    &[(3, 4, 0, 21)],
];
const RAIDERS_1: &[&[Want]] = &[
    &[(4, 1, 0, 20)],
    &[(5, 1, 0, 20)],
    &[(2, 1, 0, 20)],
    &[(2, 2, 0, 20)],
    &[(2, 3, 0, 20)],
    &[(2, 4, 0, 20)],
    &[(3, 1, 0, 20)],
    &[(3, 2, 0, 20)],
    &[(8, 1, 1, 20)],
    &[(8, 2, 1, 20)],
    &[(9, 1, 1, 20)],
    &[(9, 2, 1, 20)],
];
const RAIDERS_2P: &[&[Want]] = &[
    &[(10, 1, 0, 20), (3, 1, 0, 20), (6, 1, 0, 20)],
    &[(11, 1, 0, 20), (3, 2, 0, 20), (7, 1, 0, 20)],
    &[(4, 1, 0, 20)],
    &[(5, 1, 0, 20), (6, 2, 0, 20)],
    &[(2, 1, 0, 20), (4, 2, 0, 20), (6, 3, 0, 20)],
    &[(2, 2, 0, 20), (6, 4, 0, 20)],
    &[(2, 3, 0, 20), (4, 3, 0, 20), (7, 2, 0, 20)],
    &[(2, 4, 0, 20), (7, 3, 0, 20)],
    &[(3, 3, 0, 20)],
    &[(3, 4, 0, 20)],
    &[(10, 2, 1, 20)],
    &[(3, 5, 0, 20)],
    &[(11, 2, 1, 20)],
    &[(3, 6, 0, 20)],
];
const RAIDERS_2: &[&[Want]] = &[
    &[(8, 1, 0, 20), (3, 1, 0, 20), (6, 1, 0, 20)],
    &[(9, 1, 0, 20), (3, 2, 0, 20), (7, 1, 0, 20)],
    &[(4, 1, 0, 20)],
    &[(5, 1, 0, 20), (6, 2, 0, 20)],
    &[(2, 1, 0, 20), (4, 2, 0, 20), (6, 3, 0, 20)],
    &[(2, 2, 0, 20), (6, 4, 0, 20)],
    &[(2, 3, 0, 20), (4, 3, 0, 20), (7, 2, 0, 20)],
    &[(2, 4, 0, 20), (7, 3, 0, 20)],
    &[(3, 3, 0, 20)],
    &[(3, 4, 0, 20)],
    &[(8, 2, 1, 20)],
    &[(8, 3, 1, 20)],
    &[(9, 2, 1, 20)],
    &[(9, 3, 1, 20)],
];

const FLEET_0P: &[&[Want]] = &[
    &[(2, 1, 0, 15)],
    &[(2, 2, 0, 15)],
    &[(4, 1, 0, 15)],
    &[(4, 2, 0, 15)],
    &[(5, 1, 0, 15)],
    &[(5, 2, 0, 15)],
    &[(3, 1, 0, 15)],
    &[(3, 2, 0, 15)],
    &[(3, 3, 0, 15)],
    &[(3, 4, 0, 15)],
    &[(10, 1, 1, 10)],
    &[(10, 2, 1, 10)],
    &[(3, 5, 0, 11)],
    &[(11, 1, 1, 10)],
    &[(3, 6, 1, 10)],
    &[(3, 7, 0, 11)],
];
const FLEET_0: &[&[Want]] = &[
    &[(2, 1, 0, 15)],
    &[(2, 2, 0, 15)],
    &[(4, 1, 0, 15)],
    &[(4, 2, 0, 15)],
    &[(5, 1, 0, 15)],
    &[(5, 2, 0, 15)],
    &[(3, 1, 0, 15)],
    &[(3, 2, 0, 15)],
    &[(3, 3, 0, 15)],
    &[(3, 4, 0, 15)],
    &[(8, 1, 1, 10)],
    &[(8, 2, 1, 10)],
    &[(3, 5, 0, 11)],
    &[(9, 1, 1, 10)],
    &[(9, 2, 1, 10)],
    &[(3, 6, 0, 11)],
];
const FLEET_1P: &[&[Want]] = &[
    &[(2, 1, 0, 15)],
    &[(2, 2, 0, 15)],
    &[(2, 3, 0, 15)],
    &[(4, 1, 0, 15)],
    &[(4, 2, 0, 15)],
    &[(5, 1, 0, 15)],
    &[(5, 2, 0, 15)],
    &[(3, 1, 0, 15)],
    &[(3, 2, 0, 15)],
    &[(3, 3, 0, 15)],
    &[(3, 4, 0, 15)],
    &[(3, 5, 0, 15)],
    &[(10, 1, 1, 10)],
    &[(10, 2, 1, 10)],
    &[(3, 6, 0, 15)],
    &[(11, 1, 1, 10)],
    &[(3, 7, 0, 15)],
    &[(3, 8, 0, 15)],
];
const FLEET_1: &[&[Want]] = &[
    &[(2, 1, 0, 15)],
    &[(2, 2, 0, 15)],
    &[(2, 3, 0, 15)],
    &[(4, 1, 0, 15)],
    &[(4, 2, 0, 15)],
    &[(5, 1, 0, 15)],
    &[(5, 2, 0, 15)],
    &[(3, 1, 0, 15)],
    &[(3, 2, 0, 15)],
    &[(3, 3, 0, 15)],
    &[(3, 4, 0, 15)],
    &[(3, 5, 0, 15)],
    &[(8, 1, 1, 10)],
    &[(8, 2, 1, 10)],
    &[(3, 6, 0, 15)],
    &[(9, 1, 1, 10)],
    &[(9, 2, 1, 10)],
    &[(3, 7, 0, 15)],
];
const FLEET_2P: &[&[Want]] = &[
    &[(10, 1, 0, 15), (3, 1, 0, 15), (6, 1, 0, 15)],
    &[(10, 2, 0, 15), (3, 2, 0, 15), (6, 2, 0, 15)],
    &[(11, 1, 0, 15), (3, 3, 0, 15), (7, 1, 0, 15)],
    &[(11, 2, 0, 15), (3, 4, 0, 15), (7, 2, 0, 15)],
    &[(2, 1, 0, 15)],
    &[(2, 2, 0, 15)],
    &[(4, 1, 0, 15)],
    &[(3, 5, 0, 15), (4, 2, 0, 15)],
    &[(5, 1, 0, 15)],
    &[(3, 6, 0, 15), (5, 2, 0, 15)],
    &[(3, 7, 0, 15)],
    &[(3, 8, 0, 15)],
    &[(3, 9, 0, 10)],
    &[(10, 3, 1, 10)],
    &[(10, 4, 1, 10)],
    &[(3, 10, 0, 10)],
    &[(3, 11, 0, 10)],
    &[(11, 3, 1, 10)],
    &[(3, 12, 0, 10)],
    &[(3, 13, 0, 10)],
];
const FLEET_2: &[&[Want]] = &[
    &[(8, 1, 0, 15), (3, 1, 0, 15), (6, 1, 0, 15)],
    &[(8, 2, 0, 15), (3, 2, 0, 15), (6, 2, 0, 15)],
    &[(9, 1, 0, 15), (3, 3, 0, 15), (7, 1, 0, 15)],
    &[(9, 2, 0, 15), (3, 4, 0, 15), (7, 2, 0, 15)],
    &[(2, 1, 0, 15)],
    &[(2, 2, 0, 15)],
    &[(4, 1, 0, 15)],
    &[(3, 5, 0, 15), (4, 2, 0, 15)],
    &[(5, 1, 0, 15)],
    &[(3, 6, 0, 15), (5, 2, 0, 15)],
    &[(3, 7, 0, 15)],
    &[(3, 8, 0, 15)],
    &[(8, 3, 1, 10)],
    &[(8, 4, 1, 10)],
    &[(3, 9, 0, 10)],
    &[(9, 3, 1, 10)],
    &[(9, 4, 1, 10)],
    &[(3, 10, 0, 10)],
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::GameData;

    fn data() -> Option<GameData> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        Some(GameData::open(std::path::Path::new(&root)).unwrap())
    }

    /// Runs `mission` for `minutes` of game time; prints what each computer
    /// player has.
    fn play(data: &GameData, mission: &str, minutes: u32) -> Scene {
        let mut caches = Caches::default();
        let mut scene = Scene::load(data, &mut caches, mission).unwrap();
        let steps = minutes * 60 * 10;
        for k in 0..steps {
            scene.update(0.1, data, &mut caches);
            if k % 1500 == 1499 {
                report(&scene, k / 600 + 1);
            }
        }
        scene
    }

    fn report(scene: &Scene, minute: u32) {
        for ai in &scene.computers.players {
            let mut ships = [0; 12];
            let mut b = std::collections::BTreeMap::new();
            for o in scene.objects.iter().filter(|o| o.owner == Some(ai.owner)) {
                if let Some(s) = &o.ship {
                    ships[s.kind as usize] += 1;
                }
                if let ObjectClass::Building(c) = o.class {
                    *b.entry(c).or_insert(0) += 1;
                }
            }
            let tasks: Vec<String> = ai
                .flotillas
                .iter()
                .map(|f| format!("{}:{}/{}x{}", f.role, f.task, f.state, f.ships.len()))
                .collect();
            println!(
                "min {minute} {:?} funds {:?} ships {ships:?} bld {b:x?} enemy {:?} {}",
                ai.owner,
                scene.funds(ai.owner),
                ai.enemy,
                tasks.join(" ")
            );
            if std::env::var_os("AI_DEBUG").is_some() {
                for f in &ai.flotillas {
                    for &id in &f.ships {
                        let Some(i) = scene.object_index(id) else {
                            continue;
                        };
                        let o = &scene.objects[i];
                        let Some(s) = o.ship.as_ref() else {
                            continue;
                        };
                        println!(
                            "   role {} ship {} kind {} at ({:.0},{:.0}) task {:?} zone {:?} cargo {:.1} {:?} path {}",
                            f.role,
                            id,
                            s.kind,
                            o.x / 64.0,
                            o.y / 64.0,
                            s.task,
                            scene.ship_zone(id),
                            s.cargo,
                            s.cargo_kind,
                            s.path.len()
                        );
                    }
                }
                if std::env::var_os("AI_MAP").is_some() {
                    let (w, h) = scene.map_cells();
                    for y in 0..h.min(30) {
                        let mut line = String::from("   ");
                        for x in 0..w.min(60) {
                            let mut c = if scene.nav.is_water(x, y) { '.' } else { '#' };
                            for (i, o) in scene.objects.iter().enumerate() {
                                if o.combat.is_none() {
                                    continue;
                                }
                                if scene.cell_of(i) == (x, y) {
                                    c = match (&o.ship, o.owner == Some(ai.owner)) {
                                        (Some(_), true) => 's',
                                        (Some(_), false) => 'e',
                                        (None, true) => 'B',
                                        (None, false) => 'X',
                                    };
                                }
                            }
                            line.push(c);
                        }
                        println!("{line}");
                    }
                }
                println!(
                    "   commander clocks {:?} levels {:?} outcome {:?} orders {}",
                    ai.flotillas[ai.commander].clocks,
                    scene.levels_of(Some(ai.owner)),
                    scene.outcome,
                    ai.orders.len()
                );
            }
        }
    }

    /// Prints what the computer players do (`AI_MISSION`, `AI_MINUTES`;
    /// `AI_DEBUG` for every ship, `AI_MAP` for a map).
    #[test]
    #[ignore]
    fn probe() {
        let Some(data) = data() else {
            return;
        };
        let mission = std::env::var("AI_MISSION").unwrap_or("battle01".into());
        let minutes = std::env::var("AI_MINUTES").map_or(30, |m| m.parse().unwrap());
        play(&data, &mission, minutes);
    }

    /// What the scene looks like, to compare two runs.
    type Print = Vec<(u32, i64, i64, Option<(u32, u32)>)>;

    fn fingerprint(scene: &Scene) -> Print {
        let mut v: Vec<_> = scene
            .objects
            .iter()
            .map(|o| (o.id, (o.x * 16.0) as i64, (o.y * 16.0) as i64, o.owner))
            .collect();
        v.sort();
        for ai in &scene.computers.players {
            let (a, f) = scene.funds(ai.owner);
            v.push((u32::MAX, a as i64, f as i64, Some(ai.owner)));
        }
        v
    }

    /// Gives base 0 of `ai` a shipyard with its berth free (a ship on it
    /// taken away, or its workshop renamed, facing a free water cell).
    fn give_yard(scene: &mut Scene, ai: &Computer) {
        for y in scene.base_yards(ai, 0) {
            let i = scene.object_index(y).unwrap();
            if let Some(k) = scene.berth_cell(i).and_then(|c| scene.slot_ship(c)) {
                scene.objects.remove(k);
            }
            if scene.berth_answer(y) == 0 {
                return;
            }
        }
        let w = scene
            .objects
            .iter()
            .position(|o| o.owner == Some(ai.owner) && o.class == ObjectClass::Building(WORKSHOP))
            .unwrap();
        scene.objects[w].class = ObjectClass::Building(SHIPYARD);
        for f in 0..8 {
            scene.objects[w].record.fields[10] = f;
            if scene.berth_answer(scene.objects[w].id) == 0 {
                return;
            }
        }
        panic!("no water by the workshop");
    }

    /// A flotilla's boat (order 0x5c, part field 8 = 1) is built free: no
    /// price test, nothing paid (0x428f20, 0x416a40 with byte 0xf = 0xff);
    /// other ship orders wait for their price.
    #[test]
    fn computer_boat_order_is_free() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        for _ in 0..600 {
            scene.update(0.1, &data, &mut caches);
        }
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        give_yard(&mut scene, &ai);
        let owner = ai.owner;
        let yard = scene.base_yards(&ai, 0)[0];
        scene.purses.insert(owner, (0, 0));
        let paid = Part {
            op: Op::Ship(0),
            ..Default::default()
        };
        let step = scene.order_part(&mut ai, 0, None, paid, &data, &mut caches);
        assert!(matches!(step, PartStep::Hold));
        let free = Part { free: true, ..paid };
        let step = scene.order_part(&mut ai, 0, None, free, &data, &mut caches);
        assert!(matches!(step, PartStep::Done));
        assert_eq!(scene.funds(owner), (0, 0));
        scene.make_new_ships(&data, &mut caches);
        let k = scene.ship_on_berth(yard).expect("built on the berth");
        assert_eq!(scene.objects[k].ship.as_ref().unwrap().kind, 0);
        // Order 0x5c's part is the free one.
        assert_eq!(order_parts(0x5c)[0].0, Op::Ship(0));
    }

    /// A ship part whose town's yards have their berths taken is held (-1
    /// with flag 4, no try spent); held after 25 passes that followed one
    /// starting nothing, the town clears its berths (command 0x28,
    /// 0x4415a0): the ship on the berth is told a cell aside with priority
    /// 100 (0x428f20 at 0x4295cb, 0x4b4b50).
    #[test]
    fn held_ship_part_clears_the_berths() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        for _ in 0..600 {
            scene.update(0.1, &data, &mut caches);
        }
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        give_yard(&mut scene, &ai);
        let yard = scene.base_yards(&ai, 0)[0];
        let berth = scene.berth_cell(scene.object_index(yard).unwrap()).unwrap();
        // One of its ships lying idle on the berth.
        let ship = scene
            .objects
            .iter()
            .position(|o| {
                o.owner == Some(ai.owner) && o.ship.as_ref().is_some_and(|s| s.build.is_none())
            })
            .unwrap();
        let t = TILE_SIZE as f32;
        let id = scene.objects[ship].id;
        scene.objects[ship].x = berth.0 as f32 * t + t / 2.0;
        scene.objects[ship].y = berth.1 as f32 * t + t / 2.0;
        {
            let s = scene.objects[ship].ship.as_mut().unwrap();
            s.path.clear();
            s.task = None;
            s.priority = 0;
        }
        assert_eq!(scene.berth_answer(yard), -1);
        let part = Part {
            op: Op::Ship(0),
            free: true,
            ..Default::default()
        };
        let step = scene.order_part(&mut ai, 0, None, part, &data, &mut caches);
        assert!(matches!(step, PartStep::Berth));
        ai.land = LandFleet::default();
        ai.orders.clear();
        ai.requests.clear();
        let counted = |ai: &Computer| {
            ai.land.queue.iter().find_map(|e| match e.job {
                Job::Part { part, .. } => Some((part.state, part.tries, part.berths)),
                _ => None,
            })
        };
        scene.town_order(&mut ai, 0, 0x5c, Some(0), 23_000, 0x488);
        scene.land_fleet(&mut ai, 50, &data, &mut caches);
        scene.land_fleet(&mut ai, 53, &data, &mut caches);
        // Tried after a pass that started the order: held, not counted.
        let (state, tries, berths) = counted(&ai).unwrap();
        assert_eq!((state, berths), (1, 0));
        let mut now = 53;
        for n in 1..25 {
            now += 50;
            scene.land_fleet(&mut ai, now, &data, &mut caches);
            assert_eq!(counted(&ai).unwrap(), (1, tries, n), "held, no try spent");
        }
        let k = scene.object_index(id).unwrap();
        assert!(scene.objects[k].ship.as_ref().unwrap().path.is_empty());
        now += 50;
        scene.land_fleet(&mut ai, now, &data, &mut caches);
        assert_eq!(counted(&ai).unwrap().2, 0);
        let s = scene.objects[k].ship.as_ref().unwrap();
        assert_eq!((s.path.len(), s.priority), (1, 100));
        let (gx, gy) = s.path[0];
        let goal = ((gx / t) as i32, (gy / t) as i32);
        assert!(goal != berth && (goal.0 - berth.0).abs() <= 2 && (goal.1 - berth.1).abs() <= 2);
    }

    /// The land fleet's passes (0x42b910): 50 ticks after one that started
    /// nothing; after a start, 3 ticks after the last once woken. The asked
    /// order is put when the walk reaches its entry (0x42c050, a start that
    /// ends the walk); its part is queued and tried in the next pass; a
    /// waiting part's tries count down only in a pass after one that
    /// started nothing; a done part ends the order in the pass after, and
    /// the asker's entry is taken off when it reads the result (0x42c870).
    #[test]
    fn land_fleet_passes() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        for _ in 0..600 {
            scene.update(0.1, &data, &mut caches);
        }
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        give_yard(&mut scene, &ai);
        ai.land = LandFleet::default();
        ai.orders.clear();
        ai.requests.clear();
        let owner = ai.owner;
        scene.purses.insert(owner, (0, 0));
        let state = |ai: &Computer, k: usize| ai.land.queue[k].state();
        // A unique ship not studied: refused, it waits.
        let ask = scene.town_order(&mut ai, 0, 0x58, Some(0), 23_000, 0x488);
        for now in [0, 49] {
            scene.land_fleet(&mut ai, now, &data, &mut caches);
        }
        assert_eq!((ai.orders.len(), state(&ai, 0)), (0, 0));
        scene.land_fleet(&mut ai, 50, &data, &mut caches);
        assert_eq!((ai.orders.len(), state(&ai, 0)), (1, 2));
        assert!(ai.land.started && ai.land.woken);
        scene.land_fleet(&mut ai, 52, &data, &mut caches);
        assert_eq!(ai.land.queue.len(), 1, "not before 3 ticks");
        scene.land_fleet(&mut ai, 53, &data, &mut caches);
        assert_eq!(ai.orders[0].parts[0].state, 2, "queued");
        let tries = |ai: &Computer| match ai.land.queue[1].job {
            Job::Part { part, .. } => (part.state, part.tries),
            _ => unreachable!(),
        };
        assert_eq!(tries(&ai), (1, 25), "tried, not counted");
        assert!(!ai.land.started);
        scene.land_fleet(&mut ai, 102, &data, &mut caches);
        assert_eq!(tries(&ai), (1, 25));
        scene.land_fleet(&mut ai, 103, &data, &mut caches);
        assert_eq!(tries(&ai), (1, 24), "counted");
        assert_eq!(ai.ask_state(ask), 2);
        // A boat (free): done when tried; the order ends a pass later.
        ai.land = LandFleet::default();
        ai.orders.clear();
        let ask = scene.town_order(&mut ai, 0, 0x5c, Some(0), 23_000, 0x488);
        scene.land_fleet(&mut ai, 50, &data, &mut caches);
        scene.land_fleet(&mut ai, 53, &data, &mut caches);
        assert_eq!(ai.orders[0].parts[0].state, 3, "done");
        assert_eq!(ai.land.queue.len(), 1);
        assert!(ai.land.started && ai.land.woken);
        scene.land_fleet(&mut ai, 56, &data, &mut caches);
        assert!(ai.orders.is_empty());
        assert_eq!(ai.ask_state(ask), 3);
        assert_eq!(ai.ask_state(ask), 5, "taken off when read");
        assert_eq!(scene.funds(owner), (0, 0));
    }

    /// In battle01 the computer player (normal) builds its town up, fills
    /// its flotillas with ships, gathers amber and fish, and in the end
    /// sails against the human player, who (doing nothing) loses.
    /// The builders hunt treasures about their base (task 5 state 2, order
    /// 0xbe): a treasure lying by a computer's town is taken.
    /// The repairs order (96, part 0x3ec): the town mends a worn building;
    /// under way, the part waits until it is whole, then sends it order 10
    /// (0x42a2f0).
    #[test]
    fn computer_repairs_worn_buildings() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        scene.update(0.1, &data, &mut caches);
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        let owner = ai.owner;
        let worn = scene
            .objects
            .iter()
            .position(|o| {
                o.owner == Some(owner)
                    && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
            })
            .unwrap();
        let c = scene.objects[worn].combat.as_mut().unwrap();
        c.hull = c.max_hull / 2.0;
        let id = scene.objects[worn].id;
        let part = Part {
            op: Op::Repair,
            ..Default::default()
        };
        let step = scene.order_part(&mut ai, 0, None, part, &data, &mut caches);
        assert!(matches!(step, PartStep::Started(t) if t == id as usize));
        assert!(scene.mending.contains(&id));
        let under_way = Part {
            state: 2,
            before: id as usize,
            ..part
        };
        let step = scene.order_part(&mut ai, 0, None, under_way, &data, &mut caches);
        assert!(matches!(step, PartStep::Wait));
        // Whole: order 10, its mending ended (LandObj 0x425990).
        let c = scene.objects[worn].combat.as_mut().unwrap();
        c.hull = c.max_hull;
        let step = scene.order_part(&mut ai, 0, None, under_way, &data, &mut caches);
        assert!(matches!(step, PartStep::Done));
        // The order reaches the building at the next tick (0x520cc0).
        assert!(scene.mending.contains(&id));
        scene.ticks = scene.ticks.max(crate::scene::order::HOLD);
        scene.deliver(&data, &mut caches);
        assert!(!scene.mending.contains(&id));
        // A building gone fails the part.
        let gone = Part {
            before: u32::MAX as usize,
            ..under_way
        };
        let step = scene.order_part(&mut ai, 0, None, gone, &data, &mut caches);
        assert!(matches!(step, PartStep::Failed));
    }

    /// A warship whose crew is worn goes to the reserve for repairs, its
    /// hull whole (0x486970 with 0x4bc590).
    #[test]
    fn computer_sends_worn_crews_to_the_reserve() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        for _ in 0..1500 {
            scene.update(0.1, &data, &mut caches);
        }
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        let (f, id) = ai
            .flotillas
            .iter()
            .enumerate()
            .find_map(|(f, fl)| {
                fl.ships.iter().find_map(|&id| {
                    let i = scene.object_index(id)?;
                    let k = scene.objects[i].ship.as_ref()?.kind;
                    (2..=7).contains(&k).then_some((f, id))
                })
            })
            .unwrap();
        let i = scene.object_index(id).unwrap();
        let n = scene.objects[i].unit_counts();
        let s = scene.objects[i].ship.as_mut().unwrap();
        for (c, &n) in s.conditions.iter_mut().zip(&n) {
            *c = vec![20; n];
        }
        scene.send_worn(&mut ai, f, 10_000);
        assert!(ai.flotillas[ai.reserve].ships.contains(&id));
    }

    /// The temples' studies (order 117, studies 33/34) teach the computer
    /// spells, and its commander casts them when its town is under fire
    /// from many (letters 6 and 7, 0x488960, 0x482160).
    #[test]
    fn computer_learns_and_casts_spells() {
        use crate::magic::{Spell, Temple};
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        for _ in 0..600 {
            scene.update(0.1, &data, &mut caches);
        }
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        let owner = ai.owner;
        // A Temple of War for the base (its workshop, renamed).
        let w = scene
            .objects
            .iter()
            .position(|o| o.owner == Some(owner) && o.class == ObjectClass::Building(WORKSHOP))
            .unwrap();
        scene.objects[w].class = ObjectClass::Building(TEMPLE_OF_WAR);
        scene.earn(owner, 100_000, 100_000);
        let part = Part {
            op: Op::Study(34),
            ..Default::default()
        };
        let step = scene.order_part(&mut ai, 0, None, part, &data, &mut caches);
        assert!(matches!(step, PartStep::Started(_)));
        assert!(ai.magic.learning(Temple::War).is_some());
        // Learnt in 100 ticks: run the war temple through its list.
        for _ in 0..6 {
            ai.magic.start_learning(Temple::War, None);
            ai.magic.update(
                crate::magic::LEARN_SECONDS,
                &[],
                crate::permits::Permits::ALL,
            );
        }
        assert!(ai.magic.knows(Spell::Lightning) && ai.magic.knows(Spell::Flame));
        // The Flame on its own.
        let funds = scene.funds(owner);
        assert!(scene.computer_cast(&mut ai, 0, 9, None, None, None));
        assert!(ai.flame > 0.0 && scene.funds(owner).0 < funds.0);
        // Lightning at a cell.
        assert!(scene.computer_cast(&mut ai, 0, 10, Some((5, 5)), None, None));
        assert_eq!(
            scene.pending_casts.last().map(|c| c.0),
            Some(Spell::Lightning)
        );
        // Unknown kinds and spells of a temple it lacks are not cast.
        assert!(!scene.computer_cast(&mut ai, 0, 14, Some((5, 5)), None, None));
        assert!(!scene.computer_cast(&mut ai, 0, 6, Some((5, 5)), None, None));
    }

    #[test]
    fn computer_studies_at_the_smithy() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        for _ in 0..600 {
            scene.update(0.1, &data, &mut caches);
        }
        let mut ai = std::mem::take(&mut scene.computers.players[0]);
        let owner = ai.owner;
        // A smithy for the base (its workshop, renamed).
        let w = scene
            .objects
            .iter()
            .position(|o| o.owner == Some(owner) && o.class == ObjectClass::Building(WORKSHOP))
            .unwrap();
        scene.objects[w].class = ObjectClass::Building(SMITHY);
        scene.earn(owner, 100_000, 100_000);
        // Study 28: silver rams.
        let part = Part {
            op: Op::Study(28),
            ..Default::default()
        };
        let step = scene.order_part(&mut ai, 0, None, part, &data, &mut caches);
        assert!(matches!(step, PartStep::Started(_)));
        assert!(scene.research.iter().any(|r| r.1 == Upgrade::RamKind(1)));
        scene.run_research(Upgrade::RamKind(1).seconds(0) + 0.1);
        assert_eq!(scene.levels_of(Some(owner)).ram_kinds[1], 1);
        assert_eq!(study(32, 0), Some(Upgrade::ArmourKind(2)));
    }

    #[test]
    fn computers_after_the_same_human_spare_each_other() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        // Ships to point at each other.
        for _ in 0..1500 {
            scene.update(0.1, &data, &mut caches);
        }
        let human = scene.human.unwrap();
        let foe = human.nation * 4 + human.slot;
        // Two computers, after the same human.
        let others: Vec<(u32, u32)> = (0..16)
            .map(|id| (id / 4, id % 4))
            .filter(|&o| o != (human.nation, human.slot))
            .take(2)
            .collect();
        let mut a = Computer {
            owner: others[0],
            ..Default::default()
        };
        let mut b = Computer {
            owner: others[1],
            ..Default::default()
        };
        for c in [&mut a, &mut b] {
            c.enemy = Some(foe);
            c.difficulty = 1;
        }
        scene.computers.players = vec![a.clone(), b.clone()];
        let mut a = std::mem::take(&mut scene.computers.players[0]);
        scene.update_spared(&mut a);
        assert_eq!(a.spared, 1 << b.id());
        scene.computers.players[0] = a.clone();
        let ships: Vec<usize> = (0..scene.objects.len())
            .filter(|&i| scene.objects[i].ship.is_some())
            .take(3)
            .collect();
        for (&i, owner) in ships
            .iter()
            .zip([a.owner, b.owner, (human.nation, human.slot)])
        {
            scene.objects[i].owner = Some(owner);
        }
        let o = |k: usize| &scene.objects[ships[k]];
        assert!(scene.holds_fire(o(0), o(1)));
        assert!(!scene.holds_fire(o(0), o(2)));
        // On easy, or against another computer, everyone is fair game.
        a.difficulty = 0;
        scene.update_spared(&mut a);
        assert_eq!(a.spared, 0);
        a.difficulty = 1;
        a.enemy = Some(b.id());
        scene.update_spared(&mut a);
        assert_eq!(a.spared, 0);
    }

    #[test]
    fn computer_hunts_treasures() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        let mut took = false;
        let mut sent = false;
        for k in 0..12 * 60 * 10 {
            scene.update(0.1, &data, &mut caches);
            let ai = &mut scene.computers.players[0];
            let Some(f) = ai.flotillas.iter().position(|f| f.role == BUILDERS) else {
                continue;
            };
            if !sent && !ai.flotillas[f].ships.is_empty() {
                // The builders' round, from its treasure hunt.
                let mut p = [0; 12];
                p[7] = 1;
                ai.set_task(f, 5, p);
                sent = true;
            }
            if sent && k % 10 == 0 {
                let owner = ai.owner;
                took |= scene.objects.iter().any(|o| {
                    o.owner == Some(owner)
                        && matches!(
                            o.ship.as_ref().and_then(|s| s.task),
                            Some(Task::Take { .. })
                        )
                });
            }
        }
        assert!(sent && took, "the computer never went for a treasure");
    }

    #[test]
    fn computer_builds_gathers_and_attacks() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "battle01").unwrap();
        let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
        let mut unloaded = false;
        let mut attacked = false;
        let mut defeat_at = None;
        // 25 minutes of game time.
        for k in 0..25 * 60 * 10 {
            scene.update(0.1, &data, &mut caches);
            let ai = &scene.computers.players[0];
            if k % 5 == 0 {
                unloaded |= scene.objects.iter().any(|o| {
                    o.owner == Some(ai.owner)
                        && matches!(
                            o.ship.as_ref().and_then(|s| s.task),
                            Some(Task::Gather {
                                phase: crate::objects::GatherPhase::Unloading(_),
                                ..
                            })
                        )
                });
                attacked |= ai.flotillas.iter().any(|f| f.task == 8 && f.state == 0xc);
            }
            if k == 5 * 60 * 10 {
                // Five minutes in: a town with its port and yard, and ships.
                let owner = ai.owner;
                let has = |c: u32| {
                    scene
                        .objects
                        .iter()
                        .any(|o| o.owner == Some(owner) && o.class == ObjectClass::Building(c))
                };
                assert!(has(TOWN) && has(PORT) && (has(SHIPYARD) || has(DOCK)));
                let ships = scene
                    .objects
                    .iter()
                    .filter(|o| o.owner == Some(owner) && o.ship.is_some())
                    .count();
                assert!(ships >= 10, "{ships} ships");
                assert!(ai
                    .flotillas
                    .iter()
                    .any(|f| f.role == AMBER && !f.ships.is_empty()));
                assert!(ai
                    .flotillas
                    .iter()
                    .any(|f| f.role == FISH && !f.ships.is_empty()));
            }
            if defeat_at.is_none() && scene.outcome == Some(super::super::Outcome::Defeat) {
                defeat_at = Some(k);
            }
        }
        assert!(unloaded, "the computer's merchants never unloaded");
        // Its guards may sink a passive human before the first grand
        // attack is due (some 10 minutes in on normal).
        assert!(
            attacked || defeat_at.is_some_and(|k| k < 10 * 60 * 10),
            "the computer never attacked"
        );
        assert_eq!(
            scene.computers.players[0].enemy,
            Some(human.0 * 4 + human.1)
        );
        assert!(defeat_at.is_some(), "the human player still stands");
    }

    /// Two runs of a mission with two computer players are the same (so
    /// the machines of a multiplayer game agree), and a saved game keeps
    /// the computer players.
    #[test]
    fn computers_are_deterministic_and_saved() {
        let Some(data) = data() else {
            return;
        };
        let run = || {
            let mut caches = Caches::default();
            let mut scene = Scene::load(&data, &mut caches, "battle05").unwrap();
            for _ in 0..4 * 60 * 10 {
                scene.update(0.1, &data, &mut caches);
            }
            scene
        };
        let (a, b) = (run(), run());
        assert_eq!(a.computers.players.len(), 2);
        assert_eq!(fingerprint(&a), fingerprint(&b));
        let mut caches = Caches::default();
        let restored = Scene::restore(&data, &mut caches, &a.save()).unwrap();
        assert_eq!(restored.computers.players.len(), 2);
        for (x, y) in restored.computers.players.iter().zip(&a.computers.players) {
            assert_eq!(x.flotillas.len(), y.flotillas.len());
            assert_eq!(x.enemy, y.enemy);
        }
    }

    /// The campaign's computer players are the brains', not these.
    #[test]
    fn campaign_has_no_computer_players() {
        let Some(data) = data() else {
            return;
        };
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss001").unwrap();
        scene.update(0.5, &data, &mut caches);
        assert!(scene.computers.players.is_empty());
    }
}

#[cfg(test)]
mod cluster_tests {
    use super::cluster;

    /// Cells within 4 (`max + min / 2`) share a label; a far one gets its
    /// own (0x559070).
    #[test]
    fn near_cells_group_and_far_ones_do_not() {
        let (labels, n) = cluster(&[(0, 0), (3, 0), (3, 3), (20, 20)], 4);
        assert_eq!(n, 2);
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[1], labels[2]);
        assert_ne!(labels[0], labels[3]);
    }
}
