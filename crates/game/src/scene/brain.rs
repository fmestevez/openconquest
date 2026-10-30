//! The campaign missions' "brains", the original's per-mission scripts
//! (`brain\missNNN.cpp`), ported. A brain commands fleets of a computer
//! player's ships (those standing in given cells at the start, and ships
//! its builder fleet makes when a fleet wants more), each fleet running a
//! small program of states: wait, sail somewhere, gather in a zone, patrol
//! a round of points, attack a zone, build a ship, or a step of the
//! mission's own. Timings are game ticks (`clock::TICK`) as in the original's
//! `missCOM` primitives (0x47a3b0 and the others; see docs/FORMATS.md).

use openconquest_formats::map::TILE_SIZE;

use super::Scene;
use crate::assets::GameData;
use crate::objects::{Caches, Resource};

/// A rectangle of map cells: x, y, width, height.
pub type Cells = (i32, i32, i32, i32);

/// Go back to the state before (the original's 0xffffff0).
pub const PREVIOUS: u32 = u32::MAX;

/// A patrol's or gathering's length taken from the fleet's variable 0
/// (for a length the original rolls when the step starts).
pub const VAR0: u32 = u32::MAX;

/// A comparison of a fleet's size (0x47a1e0, 0x47a2a0: 0 equal, 1 at
/// least, 2 more than, -2 fewer than, -1 at most).
#[derive(Debug, Clone, Copy)]
pub enum Cmp {
    AtLeast,
    AtMost,
}

/// Something that interrupts a fleet's step and sends it to `state`.
#[derive(Debug, Clone, Copy)]
pub enum Event {
    /// The human player has something in `area`: attack it (0x47a010,
    /// 0x479f40; the fleet remembers the area to attack, not as its
    /// `last_area`: the event writes only `+0x50`, the zone's index).
    Zone { state: u32, area: Cells },
    /// The fleet's pending report of a blow (message 0x5d9c) is the human
    /// player's, at least `after` ticks into the step: `last_area` becomes
    /// the 5 x 5 cells about the shooter, where it is now (0x47a060).
    Attacked { state: u32, after: u32 },
    /// The fleet's size compares with the fleet's variable `n` once the
    /// step has lasted the fleet's variable `after` ticks (0x47a2a0), or
    /// at once when `after` is `None` (0x47a1e0).
    Size {
        state: u32,
        after: Option<usize>,
        cmp: Cmp,
        n: usize,
    },
    /// Cell `cell` holds (`holds`) or no longer holds a standing object of
    /// `class` (0x456540).
    Cell {
        state: u32,
        cell: (i32, i32),
        class: u32,
        holds: bool,
    },
}

/// Where an attack goes.
#[derive(Debug, Clone, Copy)]
pub enum Target {
    /// The area the event that started it saw.
    Seen,
    /// A fixed area.
    Area(Cells),
    /// Around the ship that fired on the fleet: `last_area`, which the
    /// attacked event has just written.
    Attacker,
    /// The area of the fleet's last area order or attacked event
    /// (`last_area`, member `+0x40`), whatever wrote it.
    LastArea,
    /// Around one of the human player's buildings, picked at random.
    HumanBuilding,
}

/// One step of a fleet's program.
#[derive(Debug, Clone, Copy)]
pub enum Op {
    /// Set the fleet's behaviour, then go on (0x4797e0).
    Behave { next: u32 },
    /// Wait (0x47a3b0).
    Wait { next: u32, ticks: u32 },
    /// Wait as many ticks as the fleet's variable `var` holds.
    WaitVar { next: u32, var: usize },
    /// Wait (0x47a3b0 given events): `next` after `ticks`, unless an
    /// event fires first.
    Watch {
        next: u32,
        ticks: u32,
        events: &'static [Event],
    },
    /// Sail to a cell (0x47a450): `ok` on arriving, `failed` if stopped
    /// short, `ok` after `ticks`.
    Move {
        ok: u32,
        failed: u32,
        to: (i32, i32),
        ticks: u32,
        events: &'static [Event],
    },
    /// Gather a resource in a zone (0x47a570) until `ticks` have passed
    /// (`VAR0`: the fleet's variable 0).
    Gather {
        next: u32,
        area: Cells,
        resource: Resource,
        ticks: u32,
        events: &'static [Event],
    },
    /// Sail a round of points, pausing at each (0x47a900), until `ticks`
    /// (0: forever: `timeout`) or an event; `failed` when a leg ends short
    /// (the move query 0x65 done but not "arrived").
    Patrol {
        timeout: u32,
        failed: u32,
        points: &'static [(i32, i32, u32)],
        ticks: u32,
        events: &'static [Event],
    },
    /// Attack (0x47a6a0): `next` once the area is clear or after `ticks`;
    /// `failed` (its p4) when the order reports done otherwise (no ship
    /// able, or none at all).
    Attack {
        next: u32,
        failed: u32,
        target: Target,
        ticks: u32,
        events: &'static [Event],
    },
    /// The builder fleet builds the ship asked for longest ago, for the
    /// fleet that asked (0x47b130): `next` once it is launched.
    Build { next: u32 },
    /// A step of the mission's own.
    Custom(fn(&mut Scene, usize) -> u32),
}

/// A program: the step for each state.
pub type Program = fn(u32) -> Op;

/// A fleet as a mission defines it.
pub struct FleetDef {
    /// The computer player's group it belongs to (each group's builder
    /// serves its own fleets).
    pub group: u8,
    /// Its member index in the group (0x479200 at message 0x8d1): it runs
    /// when the tick is `5 x (group + member)` modulo 50.
    pub member: u8,
    /// Areas whose ships (or, for a builder, buildings) it starts with.
    pub cells: &'static [Cells],
    pub program: Program,
    /// Ship types it wants at least so many of (by difficulty), asking the
    /// builder for more (0x479dc0 and the requests at brain + 0x114).
    pub wants: &'static [(u8, [usize; 3])],
}

/// A fleet of the brain and where it is in its program.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Fleet {
    pub ships: Vec<u32>,
    pub state: u32,
    pub previous: u32,
    pub(super) started: bool,
    pub(super) since: u32,
    /// The area an event chose to attack.
    pub(super) area: Option<Cells>,
    /// Patrol: the point sailed to, and when the pause there began.
    point: usize,
    paused_at: Option<u32>,
    /// The pending report of a blow on one of its ships (message 0x5d9c,
    /// the brain's list `+0x100`): the shooter's id. The first is kept,
    /// later ones dropped until it is consumed.
    #[serde(default)]
    pub attacked_by: Option<u32>,
    /// Member `+0x40`: the area of its last attack, gathering, founding or
    /// order 0xbe, or of the last attacked event that fired; (0, 0, 0, 0)
    /// until the first.
    #[serde(default)]
    pub last_area: Cells,
    /// The mission's own numbers (e.g. a raid's timing).
    pub vars: [u32; 4],
}

/// A mission's brain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Brain {
    pub mission: String,
    pub tick: u32,
    pub fleets: Vec<Fleet>,
    /// Ships asked for: (fleet, type), oldest first; and being built:
    /// (shipyard, type, fleet).
    pub requests: Vec<(usize, u8)>,
    pub building: Vec<(u32, u8, usize)>,
    /// The original's difficulty (+0x32a1): 0 easy, 1 normal, 2 hard.
    #[serde(default = "normal")]
    pub difficulty: usize,
    /// Town orders given to members' land fleets (0x47b130): the yards'
    /// full-town order (code 1) and the builders' repairs (code 0x60).
    #[serde(default)]
    pub jobs: Vec<Job>,
    /// miss006's watch for waterspouts: armed, checks made, since when.
    #[serde(default)]
    pub watch: [u32; 3],
    /// miss006: squadron (2, 6) has joined (2, 7) (`+0x1b4`).
    #[serde(default)]
    pub joined: bool,
    /// miss005: the harpy nests and the barbarians' stronghold found gone
    /// (`+0x184`, `+0x18c` cleared by the brain's pass).
    #[serde(default)]
    pub gone: [bool; 2],
}

/// A town order a brain member gave (0x47b130 through the land fleet,
/// 0x42b910, a pass every 50 ticks).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Job {
    /// Code 1 (table 0x5ac7c8's order 2 and the two temples): the town in
    /// the member's cells gets the port, sage school, shipyard, smithy,
    /// is made a town, its shipyard a dock, then the school and both
    /// temples, a part at a time, each when it can be paid for; failed
    /// parts tried again (flag 0x10).
    FullTown {
        fleet: usize,
        /// Order 240 (0xf0, the yards of miss002-miss004): a small and a
        /// medium fort of the town's instead of the temples.
        #[serde(default)]
        forts: bool,
    },
    /// Code 0x60 (order 96): the first worn building of the member's is
    /// mended, once; after 10 passes with none, the order fails.
    Repair { fleet: usize, idle: u32 },
}

/// The difficulty levels (+0x32a1).
pub const EASY: usize = 0;
pub const NORMAL: usize = 1;
pub const HARD: usize = 2;

fn normal() -> usize {
    NORMAL
}

thread_local! {
    /// The difficulty of the brain running on this thread: the programs
    /// are plain functions of their state.
    static LEVEL: std::cell::Cell<usize> = const { std::cell::Cell::new(NORMAL) };
}

/// The difficulty of the brain running (indexes the per-difficulty tables).
fn difficulty() -> usize {
    LEVEL.with(|l| l.get())
}

struct Mission {
    fleets: &'static [FleetDef],
    /// Run once at the start (deposits, bonuses).
    start: fn(&mut Scene, &GameData, &mut Caches),
    /// Run for each fleet on its turn before its step, even while it has
    /// no ships (the original's members' own bookkeeping: what to ask the
    /// yards for, and the like).
    turn: Option<fn(&mut Scene, usize)>,
    /// Members made at the start (0x8d1) that the main's switch has no
    /// case for: they never run.
    silent: &'static [(u8, u8)],
    /// Whether member `k` runs its step this turn, for members whose own
    /// code tests their ships before their switch.
    gate: Option<fn(&Scene, usize) -> bool>,
    /// The brain's own work on its 5-tick pass, before the members and
    /// after them.
    before: Option<fn(&mut Scene)>,
    after: Option<fn(&mut Scene)>,
}

/// A mission's fleets as it defines them.
fn mission_fleets(name: &str) -> Option<&'static [FleetDef]> {
    mission(name).map(|m| m.fleets)
}

fn mission(name: &str) -> Option<Mission> {
    match name {
        "miss000" => Some(Mission {
            fleets: miss000::FLEETS,
            start: miss000::start,
            turn: None,
            silent: &[],
            gate: None,
            before: None,
            after: None,
        }),
        "miss001" => Some(Mission {
            fleets: miss001::FLEETS,
            start: miss001::start,
            turn: None,
            silent: &[],
            gate: None,
            before: None,
            after: None,
        }),
        "miss002" => Some(Mission {
            fleets: miss002::FLEETS,
            start: miss002::start,
            turn: None,
            silent: &[],
            gate: None,
            before: None,
            after: None,
        }),
        "miss003" => Some(Mission {
            fleets: miss003::FLEETS,
            start: miss003::start,
            turn: None,
            silent: &[],
            gate: None,
            before: None,
            after: None,
        }),
        "miss004" => Some(Mission {
            fleets: miss004::FLEETS,
            start: miss004::start,
            turn: None,
            silent: &[(0, 7), (2, 5)],
            gate: None,
            before: None,
            after: None,
        }),
        "miss005" => Some(Mission {
            fleets: miss005::FLEETS,
            start: miss005::start,
            turn: Some(miss005::turn),
            silent: &[],
            gate: None,
            before: None,
            after: Some(miss005::after),
        }),
        "miss006" => Some(Mission {
            fleets: miss006::FLEETS,
            start: miss006::start,
            turn: Some(miss006::turn),
            silent: &[(1, 6)],
            gate: None,
            before: None,
            after: None,
        }),
        "miss007" => Some(Mission {
            fleets: miss007::FLEETS,
            start: miss007::start,
            turn: Some(miss007::turn),
            silent: &[],
            gate: None,
            before: Some(miss007::before),
            after: None,
        }),
        "miss008" => Some(Mission {
            fleets: miss008::FLEETS,
            start: miss008::start,
            turn: Some(miss008::turn),
            silent: &[(0, 0), (1, 0)],
            gate: Some(miss008::gate),
            before: None,
            after: None,
        }),
        "miss009" => Some(Mission {
            fleets: miss009::FLEETS,
            start: miss009::start,
            turn: Some(miss009::turn),
            silent: &[(0, 0)],
            gate: None,
            before: Some(miss009::before),
            after: None,
        }),
        "miss010" => Some(Mission {
            fleets: miss010::FLEETS,
            start: miss010::start,
            turn: Some(miss010::turn),
            silent: &[(0, 0), (1, 0)],
            gate: None,
            before: Some(miss010::before),
            after: None,
        }),
        "miss011" => Some(Mission {
            fleets: miss011::FLEETS,
            start: miss011::start,
            turn: Some(miss011::turn),
            silent: &[],
            gate: None,
            before: None,
            after: Some(miss011::after),
        }),
        "miss012" => Some(Mission {
            fleets: miss012::FLEETS,
            start: miss012::start,
            turn: Some(miss012::turn),
            silent: &[],
            gate: None,
            before: None,
            after: None,
        }),
        "miss013" => Some(Mission {
            fleets: miss013::FLEETS,
            start: miss013::start,
            turn: Some(miss013::turn),
            silent: &[],
            gate: None,
            before: None,
            after: None,
        }),
        _ => None,
    }
}

/// miss009's and miss010's every-5-ticks gate on their tick (0x4665c9,
/// 0x46a32a): a number is drawn and dropped, another drawn, `x`, and the
/// tick runs when the tick is `((x % 10 + x % 10) % 10) x 5` modulo 50
/// (both terms read the seed again after the second draw: the same `x`).
/// So only on a tick a multiple of 10, one time in five.
pub(super) fn random_tick(scene: &mut Scene) -> bool {
    scene.rng.next();
    let x = scene.rng.next();
    let phase = ((x % 10 + x % 10) % 10) * 5;
    scene.brain.as_ref().unwrap().tick % 50 == phase
}

impl Scene {
    /// Starts the brain of campaign mission `mission`, if ported.
    pub(super) fn start_brain(&mut self, data: &GameData, caches: &mut Caches, difficulty: usize) {
        let Some(m) = mission(&self.mission) else {
            return;
        };
        let difficulty = difficulty.min(HARD);
        LEVEL.with(|l| l.set(difficulty));
        let t = TILE_SIZE as f32;
        let mut brain = Brain {
            mission: self.mission.clone(),
            tick: 0,
            fleets: Vec::new(),
            requests: Vec::new(),
            building: Vec::new(),
            difficulty,
            jobs: Vec::new(),
            watch: [1, 0, 0],
            joined: false,
            gone: [false; 2],
        };
        for def in m.fleets {
            // The ships (or buildings) standing in the given cells (0x479200).
            let ships: Vec<u32> = def
                .cells
                .iter()
                .flat_map(|&(x, y, w, h)| {
                    self.objects
                        .iter()
                        .filter(move |o| {
                            let (cx, cy) = ((o.x / t) as i32, (o.y / t) as i32);
                            o.combat.is_some()
                                && o.owner.is_some()
                                && cx >= x
                                && cx < x + w
                                && cy >= y
                                && cy < y + h
                        })
                        .filter(|o| !self.is_own(o))
                        .map(|o| o.id)
                })
                .collect();
            brain.fleets.push(Fleet {
                ships,
                state: 0,
                previous: 0,
                started: false,
                since: 0,
                area: None,
                point: 0,
                paused_at: None,
                attacked_by: None,
                last_area: (0, 0, 0, 0),
                vars: [0; 4],
            });
        }
        self.brain = Some(brain);
        (m.start)(self, data, caches);
    }

    /// A ship launched at `site`: if the brain ordered it, it joins the
    /// fleet that asked.
    pub(super) fn ship_launched(&mut self, site: u32, kind: u8, id: u32) {
        let Some(brain) = self.brain.as_mut() else {
            return;
        };
        if let Some(k) = brain
            .building
            .iter()
            .position(|b| b.0 == site && b.1 == kind)
        {
            let (_, _, fleet) = brain.building.remove(k);
            brain.fleets[fleet].ships.push(id);
        }
    }

    /// A brain hears its player's object `target` was struck (message
    /// 0x5d9c): miss006's brain, the first time one of squadron (2, 6)'s
    /// ships is, sends its ships to squadron (2, 7) (0x45b280, 0x479370).
    /// (Not decoded: the ship's `+0x691` gate on the report.)
    pub(super) fn brain_hit(&mut self, target: u32) {
        let Some(b) = self.brain.as_ref() else {
            return;
        };
        if b.mission != "miss006" || b.joined {
            return;
        }
        let (Some(from), Some(into)) = (self.member_fleet(2, 6), self.member_fleet(2, 7)) else {
            return;
        };
        let theirs = self
            .object_index(target)
            .is_some_and(|i| self.objects[i].owner == Some((3, 2)));
        if theirs
            && self.brain.as_ref().unwrap().fleets[from]
                .ships
                .contains(&target)
        {
            self.fleet_merge(into, from);
            self.brain.as_mut().unwrap().joined = true;
        }
    }

    /// A blow from `shooter` (anyone's ship or building) on ship `target`
    /// (0x4bf170 marks the ship `+0x691`, `+0x692` the shooter's id; the
    /// fleet's order handlers report it with message 0x5d9c, 0x4a21c0):
    /// the brain keeps the report unless one for the same fleet is pending
    /// (0x47b2e0). (Not decoded: which fleet orders report, and how often,
    /// 0x4ca290 and the others; the port reports at once.)
    pub(super) fn fleet_attacked(&mut self, target: u32, shooter: u32) {
        if let Some(b) = self.brain.as_mut() {
            for f in &mut b.fleets {
                if f.ships.contains(&target) && f.attacked_by.is_none() {
                    f.attacked_by = Some(shooter);
                }
            }
        }
    }

    /// Runs the brain one tick (message 0 of each mission's main). Its
    /// tick is the game clock (0x47b2e0 copies `[game +0xde]` into
    /// `+0xfc`). Every 5 ticks the brain does its own work (`before`, for
    /// miss007, miss009 and miss010), runs its members, group by group in
    /// order, member (g, m) when the tick is `5 x (g + m)` modulo 50, and
    /// does the rest of its own (`after`, for miss005 and miss011). A
    /// member runs whether or not it has ships (the original checks only
    /// its fleet id).
    pub(super) fn run_brain(&mut self) {
        let Some(brain) = self.brain.as_mut() else {
            return;
        };
        brain.tick += 1;
        let tick = brain.tick;
        LEVEL.with(|l| l.set(brain.difficulty));
        let Some(m) = mission(&brain.mission) else {
            return;
        };
        if tick % 5 != 0 {
            return;
        }
        if let Some(before) = m.before {
            before(self);
        }
        for k in 0..m
            .fleets
            .len()
            .min(self.brain.as_ref().unwrap().fleets.len())
        {
            let (g, member) = (m.fleets[k].group, m.fleets[k].member);
            if m.silent.contains(&(g, member)) || tick % 50 != (5 * (g as u32 + member as u32)) % 50
            {
                continue;
            }
            let ids = self.brain.as_ref().unwrap().fleets[k].ships.clone();
            // Ships sunk, or captured by the player, leave the fleet.
            let alive: Vec<u32> = ids
                .into_iter()
                .filter(|&id| {
                    self.object_index(id)
                        .is_some_and(|i| !self.is_own(&self.objects[i]))
                })
                .collect();
            self.brain.as_mut().unwrap().fleets[k].ships = alive;
            if let Some(turn) = m.turn {
                turn(self, k);
            }
            self.ask_for_ships(k, &m.fleets[k]);
            // A member with no ships runs its steps as any other (see
            // `fleet_step`), but for one whose own code holds it.
            if m.gate.is_some_and(|gate| !gate(self, k)) {
                continue;
            }
            let groups: Vec<u8> = m.fleets.iter().map(|f| f.group).collect();
            self.fleet_step(k, m.fleets[k].program, &groups, tick);
        }
        if let Some(after) = m.after {
            after(self);
        }
    }

    /// A fleet short of a ship type it wants asks the builder for one.
    fn ask_for_ships(&mut self, k: usize, def: &FleetDef) {
        for &(kind, min) in def.wants {
            let brain = self.brain.as_ref().unwrap();
            let have = brain.fleets[k]
                .ships
                .iter()
                .filter(|&&id| {
                    self.object_index(id).is_some_and(|i| {
                        self.objects[i]
                            .ship
                            .as_ref()
                            .is_some_and(|s| s.kind == kind)
                    })
                })
                .count();
            let asked = brain.requests.contains(&(k, kind))
                || brain.building.iter().any(|b| b.2 == k && b.1 == kind);
            if have < min[difficulty()] && !asked {
                self.brain.as_mut().unwrap().requests.push((k, kind));
            }
        }
    }

    fn set_state(&mut self, k: usize, state: u32) {
        let f = &mut self.brain.as_mut().unwrap().fleets[k];
        let state = if state == PREVIOUS { f.previous } else { state };
        f.previous = f.state;
        f.state = state;
        f.started = false;
        f.paused_at = None;
    }

    /// The state the first firing event sends the fleet to, if any.
    pub(super) fn event_fired(&mut self, k: usize, events: &[Event], elapsed: u32) -> Option<u32> {
        let t = TILE_SIZE as f32;
        for e in events {
            match *e {
                Event::Zone { state, area } => {
                    if self.human_in(area) {
                        self.brain.as_mut().unwrap().fleets[k].area = Some(area);
                        return Some(state);
                    }
                }
                Event::Attacked { state, after } => {
                    // 0x47a060: a report younger than the step's `after`
                    // is dropped, as is one whose shooter is gone; one not
                    // the human player's (mask 0x20) stays.
                    let Some(shooter) = self.brain.as_ref().unwrap().fleets[k].attacked_by else {
                        continue;
                    };
                    let found = self.object_index(shooter);
                    if elapsed < after || found.is_none() {
                        self.brain.as_mut().unwrap().fleets[k].attacked_by = None;
                        continue;
                    }
                    let o = &self.objects[found.unwrap()];
                    if !self.is_own(o) {
                        continue;
                    }
                    let cell = ((o.x / t) as i32, (o.y / t) as i32);
                    let area = self.cells_about(cell, 2);
                    let f = &mut self.brain.as_mut().unwrap().fleets[k];
                    f.attacked_by = None;
                    f.last_area = area;
                    return Some(state);
                }
                Event::Size {
                    state,
                    after,
                    cmp,
                    n,
                } => {
                    let f = &self.brain.as_ref().unwrap().fleets[k];
                    let ready = after.is_none_or(|v| elapsed >= f.vars[v]);
                    let size = f.ships.len() as u32;
                    let met = match cmp {
                        Cmp::AtLeast => size >= f.vars[n],
                        Cmp::AtMost => size <= f.vars[n],
                    };
                    if ready && met {
                        return Some(state);
                    }
                }
                Event::Cell {
                    state,
                    cell,
                    class,
                    holds,
                } => {
                    let there = self.objects.iter().any(|o| {
                        o.record.class_id == class
                            && o.cell == Some(cell)
                            && o.combat.as_ref().is_none_or(|c| c.hull > 0.0)
                    });
                    if there == holds {
                        return Some(state);
                    }
                }
            }
        }
        None
    }

    fn fleet_step(&mut self, k: usize, program: Program, groups: &[u8], tick: u32) {
        let f = &self.brain.as_ref().unwrap().fleets[k];
        let (state, started, since) = (f.state, f.started, f.since);
        let op = program(state);
        if !started {
            let f = &mut self.brain.as_mut().unwrap().fleets[k];
            f.started = true;
            f.since = tick;
        }
        let elapsed = if started {
            tick.saturating_sub(since)
        } else {
            0
        };
        // An order given to a fleet with no ships is taken like any other
        // (0x4a09d0 allocates its block, 0x4cc500 / 0x4d6540) and left as
        // it was by the fleet tick; every status routine (0x4ccf60,
        // 0x4d63d0, 0x4d70d0, 0x4c8da0) then reports it done, result 1, at
        // the member's next turn.
        let empty = started && self.brain.as_ref().unwrap().fleets[k].ships.is_empty();
        match op {
            Op::Behave { next } => self.set_state(k, next),
            Op::Wait { next, ticks } => {
                if started && elapsed >= ticks {
                    self.set_state(k, next);
                }
            }
            Op::WaitVar { next, var } => {
                let ticks = self.brain.as_ref().unwrap().fleets[k].vars[var];
                if started && elapsed >= ticks {
                    self.set_state(k, next);
                }
            }
            Op::Watch {
                next,
                ticks,
                events,
            } => {
                if started {
                    if let Some(s) = self.event_fired(k, events, elapsed) {
                        self.set_state(k, s);
                    } else if elapsed >= ticks {
                        self.set_state(k, next);
                    }
                }
            }
            Op::Move {
                ok,
                failed,
                to,
                ticks,
                events,
            } => {
                if started {
                    if let Some(s) = self.event_fired(k, events, elapsed) {
                        self.fleet_stop(k);
                        self.set_state(k, s);
                        return;
                    }
                }
                if !started {
                    self.fleet_move(k, to);
                } else if empty {
                    // Done but not arrived: `failed`, unless the time is up
                    // too (0x47a450 tests it after the query).
                    let up = ticks > 0 && elapsed >= ticks;
                    self.set_state(k, if up { ok } else { failed });
                } else if self.fleet_idle(k) {
                    let arrived = self.fleet_near(k, to, 2.0);
                    self.set_state(k, if arrived { ok } else { failed });
                } else if ticks > 0 && elapsed >= ticks {
                    self.set_state(k, ok);
                }
            }
            Op::Gather {
                next,
                area,
                resource,
                ticks,
                events,
            } => {
                if started {
                    if let Some(s) = self.event_fired(k, events, elapsed) {
                        self.fleet_stop(k);
                        self.set_state(k, s);
                        return;
                    }
                }
                let ticks = if ticks == VAR0 {
                    self.brain.as_ref().unwrap().fleets[k].vars[0]
                } else {
                    ticks
                };
                if !started {
                    // Order 0x78 (0x479970) writes the area into `+0x40`.
                    self.brain.as_mut().unwrap().fleets[k].last_area = area;
                    self.fleet_gather(k, area, resource);
                } else if empty || (ticks > 0 && elapsed >= ticks) {
                    // With no ships, result 1 sends 0x47a570 to its p4,
                    // which every mission passes equal to its p3.
                    self.fleet_stop(k);
                    self.set_state(k, next);
                }
            }
            Op::Patrol {
                timeout,
                failed,
                points,
                ticks,
                events,
            } => {
                if let Some(s) = self.event_fired(k, events, elapsed) {
                    self.set_state(k, s);
                    return;
                }
                let ticks = if ticks == VAR0 {
                    self.brain.as_ref().unwrap().fleets[k].vars[0]
                } else {
                    ticks
                };
                if started && ticks > 0 && elapsed >= ticks {
                    self.set_state(k, timeout);
                    return;
                }
                if !started {
                    self.brain.as_mut().unwrap().fleets[k].point = 0;
                    let (x, y, _) = points[0];
                    self.fleet_move(k, (x, y));
                    return;
                }
                let f = &self.brain.as_ref().unwrap().fleets[k];
                let (point, paused) = (f.point.min(points.len() - 1), f.paused_at);
                // On its way to a point (`+0x60 == 1`) a fleet with no ships
                // is done but not arrived: `failed` (0x47a900).
                if empty && paused.is_none() {
                    self.set_state(k, failed);
                    return;
                }
                match paused {
                    None if self.fleet_idle(k) => {
                        self.brain.as_mut().unwrap().fleets[k].paused_at = Some(tick);
                    }
                    Some(at) if tick >= at + points[point].2 => {
                        let next = (point + 1) % points.len();
                        let f = &mut self.brain.as_mut().unwrap().fleets[k];
                        f.point = next;
                        f.paused_at = None;
                        let (x, y, _) = points[next];
                        self.fleet_move(k, (x, y));
                    }
                    _ => {}
                }
            }
            Op::Attack {
                next,
                failed,
                target,
                ticks,
                events,
            } => {
                if started {
                    if let Some(s) = self.event_fired(k, events, elapsed) {
                        self.fleet_stop(k);
                        self.set_state(k, s);
                        return;
                    }
                }
                if !started {
                    let f = &self.brain.as_ref().unwrap().fleets[k];
                    let (seen, last) = (f.area, f.last_area);
                    let area = match target {
                        Target::Area(a) => Some(a),
                        Target::Seen => seen,
                        Target::Attacker | Target::LastArea => Some(last),
                        Target::HumanBuilding => self.random_human_building(),
                    };
                    match area {
                        // Order 0x70 (0x479a40) writes the area into `+0x40`.
                        Some(a) => {
                            self.brain.as_mut().unwrap().fleets[k].last_area = a;
                            self.fleet_attack(k, a);
                        }
                        None => self.set_state(k, next),
                    }
                } else if empty {
                    // Done, result 1: p4, unless the time is up too.
                    let up = ticks > 0 && elapsed >= ticks;
                    self.set_state(k, if up { next } else { failed });
                } else if self.fleet_idle(k) || (ticks > 0 && elapsed >= ticks) {
                    self.fleet_stop(k);
                    self.set_state(k, next);
                }
            }
            Op::Build { next } => {
                if !started {
                    if !self.fleet_build(k, groups) {
                        // Nothing asked for, or not affordable yet: try
                        // again next turn.
                        self.brain.as_mut().unwrap().fleets[k].started = false;
                    }
                } else if !self
                    .brain
                    .as_ref()
                    .unwrap()
                    .building
                    .iter()
                    .any(|b| self.brain.as_ref().unwrap().fleets[k].ships.contains(&b.0))
                {
                    self.set_state(k, next);
                }
            }
            Op::Custom(step) => {
                let next = step(self, k);
                if next != self.brain.as_ref().unwrap().fleets[k].state {
                    self.set_state(k, next);
                } else {
                    // Stay, and run the step again next turn.
                    self.brain.as_mut().unwrap().fleets[k].started = false;
                }
            }
        }
    }

    /// The builder fleet `k` orders the oldest request at one of its
    /// buildings, paying from its player's purse. False if it could not.
    fn fleet_build(&mut self, k: usize, groups: &[u8]) -> bool {
        let brain = self.brain.as_ref().unwrap();
        let Some(&site) = brain.fleets[k]
            .ships
            .iter()
            .find(|&&id| self.production_at(id).is_none())
        else {
            return false;
        };
        let Some(owner) = self.object_index(site).and_then(|i| self.objects[i].owner) else {
            return false;
        };
        // The oldest request of the builder's group it can pay for (one it
        // cannot afford waits, without holding up cheaper ones).
        let free = self.barbarian_site(site);
        let (have_a, have_f) = self.funds(owner);
        let Some(r) = brain.requests.iter().position(|&(f, kind)| {
            let (amber, fish) = crate::economy::ship_cost(kind);
            groups.get(f) == groups.get(k) && (free || (have_a >= amber && have_f >= fish))
        }) else {
            return false;
        };
        let (fleet, kind) = brain.requests[r];
        if !free {
            let (amber, fish) = crate::economy::ship_cost(kind);
            self.earn(owner, -(amber as i64), -(fish as i64));
        }
        self.productions.push(super::Production {
            argo: false,
            site,
            ship: kind,
            elapsed: 0.0,
            total: crate::economy::SHIP_BUILD_SECONDS,
            arming: None,
            paid: (0, 0),
        });
        let brain = self.brain.as_mut().unwrap();
        brain.requests.remove(r);
        brain.building.push((site, kind, fleet));
        true
    }

    /// The builder fleet `k` orders a ship of type `kind` for fleet
    /// `fleet` at one of its buildings, paying from its player's purse
    /// (0x47b130 with the member to hand the ship to). False if it could
    /// not (no free yard, or not affordable yet).
    pub(super) fn fleet_build_for(&mut self, k: usize, fleet: usize, kind: u8) -> bool {
        let brain = self.brain.as_ref().unwrap();
        let Some(&site) = brain.fleets[k]
            .ships
            .iter()
            .find(|&&id| self.production_at(id).is_none())
        else {
            return false;
        };
        let Some(owner) = self.object_index(site).and_then(|i| self.objects[i].owner) else {
            return false;
        };
        // Barbarian structures build for nothing (BarbarianClassTy pays
        // only to mend its units, 0x40eba0).
        if !self.barbarian_site(site) {
            let (amber, fish) = crate::economy::ship_cost(kind);
            let (have_a, have_f) = self.funds(owner);
            if have_a < amber || have_f < fish {
                return false;
            }
            self.earn(owner, -(amber as i64), -(fish as i64));
        }
        self.productions.push(super::Production {
            argo: false,
            site,
            ship: kind,
            elapsed: 0.0,
            total: crate::economy::SHIP_BUILD_SECONDS,
            arming: None,
            paid: (0, 0),
        });
        self.brain
            .as_mut()
            .unwrap()
            .building
            .push((site, kind, fleet));
        true
    }

    /// Whether `site` is a barbarian structure (class 0x3f-0x41).
    fn barbarian_site(&self, site: u32) -> bool {
        self.object_index(site).is_some_and(|i| {
            matches!(
                self.objects[i].class,
                openconquest_formats::ObjectClass::Barbarian(_)
            )
        })
    }

    /// Whether a ship builder fleet `k` started is still being built.
    pub(super) fn fleet_building(&self, k: usize) -> bool {
        let brain = self.brain.as_ref().unwrap();
        brain
            .building
            .iter()
            .any(|b| brain.fleets[k].ships.contains(&b.0))
    }

    /// How many ships of each type fleet `k` has (0x479dc0).
    pub(super) fn fleet_types(&self, k: usize) -> [usize; 12] {
        let mut n = [0; 12];
        for &id in &self.brain.as_ref().unwrap().fleets[k].ships {
            if let Some(s) = self
                .object_index(id)
                .and_then(|i| self.objects[i].ship.as_ref())
            {
                n[s.kind as usize % 12] += 1;
            }
        }
        n
    }

    /// Whether the human player has a building of class `class` on cell
    /// `cell` (0x421490 over one cell).
    pub(super) fn human_class_at(&self, class: u32, cell: (i32, i32)) -> bool {
        self.objects.iter().any(|o| {
            self.is_own(o)
                && o.combat.is_some()
                && o.class == openconquest_formats::ObjectClass::Building(class)
                && o.cell == Some(cell)
        })
    }

    /// An area around one of the human player's settlements or towns
    /// (classes 0x32, 0x33) standing in `rect`, or anywhere when `None`,
    /// picked at random (0x421490, 0x421740).
    pub(super) fn human_site_area(&mut self, rect: Option<Cells>) -> Option<Cells> {
        let spots: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| {
                self.is_own(o)
                    && o.combat.is_some()
                    && matches!(
                        o.class,
                        openconquest_formats::ObjectClass::Building(0x32 | 0x33)
                    )
            })
            .filter_map(|o| o.cell)
            .filter(|&(x, y)| {
                rect.is_none_or(|(rx, ry, w, h)| x >= rx && x < rx + w && y >= ry && y < ry + h)
            })
            .collect();
        if spots.is_empty() {
            return None;
        }
        let (x, y) = spots[self.rng.next() as usize % spots.len()];
        Some((x - 3, y - 3, 7, 7))
    }

    /// The first of `zones` where the human player has a ship or building
    /// (0x479f40).
    pub(super) fn first_human_zone(&self, zones: &[Cells]) -> Option<usize> {
        zones.iter().position(|&z| self.human_in(z))
    }

    /// Whether the human player has a ship in `area` (0x479ed0's test).
    pub(super) fn human_ships_in(&self, (x, y, w, h): Cells) -> bool {
        let t = TILE_SIZE as f32;
        self.objects.iter().any(|o| {
            self.is_own(o) && o.ship.is_some() && {
                let (cx, cy) = ((o.x / t) as i32, (o.y / t) as i32);
                cx >= x && cx < x + w && cy >= y && cy < y + h
            }
        })
    }

    /// Puts up a big reef (class 0x259, 0x430ec0) of kind `kind` (1-3) on
    /// a cell.
    pub(super) fn big_reef(
        &mut self,
        x: i32,
        y: i32,
        kind: u32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 12];
        fields[0] = 0x259;
        fields[1] = 1;
        fields[5] = x as u32;
        fields[6] = y as u32;
        fields[7] = kind;
        let rec = openconquest_formats::ObjectRecord {
            id,
            class_id: 0x259,
            fields,
        };
        match crate::objects::resolve(data, caches, &rec) {
            Ok(Some(obj)) => {
                self.objects.push(obj);
                self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            }
            Ok(None) => {}
            Err(e) => eprintln!("could not raise a reef: {e:#}"),
        }
    }

    /// A treasure of kind `kind` lying on the map, if any.
    pub(super) fn loose_treasure(&self, kind: u8) -> Option<u32> {
        self.objects
            .iter()
            .find(|o| o.treasure.is_some_and(|t| t.kind == kind))
            .map(|o| o.id)
    }

    /// The cell of the human player's ship or building holding a treasure
    /// of kind `kind`.
    pub(super) fn human_holder_of(&self, kind: u8) -> Option<(i32, i32)> {
        let t = TILE_SIZE as f32;
        self.objects
            .iter()
            .filter(|o| self.is_own(o))
            .find(|o| {
                o.ship.as_ref().is_some_and(|s| s.has_treasure(kind))
                    || self.held.iter().any(|h| h.0 == o.id && h.1.kind == kind)
            })
            .map(|o| ((o.x / t) as i32, (o.y / t) as i32))
    }

    /// `n` cells about the `w` x `h` cells at `(x, y)` (0x4796c0, here
    /// with a 1 x 1 area): the corner moved `n` up and left but not past
    /// 0, the size grown by `2 n`, then cut to end at the map's edge.
    pub(super) fn cells_about(&self, (x, y): (i32, i32), n: i32) -> Cells {
        let (mw, mh) = (self.nav.width as i32, self.nav.height as i32);
        let x0 = if x - n < 1 { 0 } else { x - n };
        let y0 = if y - n < 1 { 0 } else { y - n };
        let (w, h) = (1 + 2 * n, 1 + 2 * n);
        let w = if x0 + w > mw { mw - x0 } else { w };
        let h = if y0 + h > mh { mh - y0 } else { h };
        (x0, y0, w, h)
    }

    /// Sets the area fleet `k` attacks next (`Target::Seen`).
    pub(super) fn set_fleet_area(&mut self, k: usize, area: Cells) {
        self.brain.as_mut().unwrap().fleets[k].area = Some(area);
    }

    /// Fleet `from`'s ships join fleet `into` (0x479370, op 0xe), keeping
    /// what they were doing until `into` gives its next order.
    pub(super) fn fleet_merge(&mut self, into: usize, from: usize) {
        if into == from {
            return;
        }
        let brain = self.brain.as_mut().unwrap();
        let ships = std::mem::take(&mut brain.fleets[from].ships);
        brain.fleets[into].ships.extend(ships);
    }

    /// A brain player's ship mended at a dock sails out (0x4b6d60, message
    /// 0x5d98) and joins the member its mission names for its type
    /// (0x479300, op 2), leaving whatever fleet it was in.
    pub(super) fn brain_mended(&mut self, id: u32) {
        let Some(brain) = self.brain.as_ref() else {
            return;
        };
        let Some(i) = self.object_index(id) else {
            return;
        };
        let (Some((n, s)), Some(kind)) = (
            self.objects[i].owner,
            self.objects[i].ship.as_ref().map(|s| s.kind),
        ) else {
            return;
        };
        let player = n * 4 + s;
        let mission = brain.mission.clone();
        let bit = |scene: &mut Scene| scene.rng.next() & 1 != 0;
        // Ship classes: 200 + type; 0xca-0xcf types 2-7, 0xd0-0xd1 8-9,
        // 0xd2-0xd3 10-11.
        let (group, member) = match (mission.as_str(), player, kind) {
            ("miss002", 15, 0 | 1) => (1, if bit(self) { 1 } else { 0 }),
            ("miss002", 15, 2..=7) => (1, 2),
            ("miss003", 15, 0 | 1) => (0, 0),
            ("miss003", 15, 2..=7) => (0, 2),
            ("miss004", 15, 0 | 1) => (0, 0),
            ("miss004", 15, 2..=7) => (0, 3),
            ("miss004", 14, 0 | 1) => (2, 0),
            ("miss004", 14, 2..=7) => (2, 2),
            ("miss005" | "miss006", 15, 0 | 1) => (1, 2),
            ("miss005" | "miss006", 15, 2..=7) => (1, 4),
            ("miss006", 14, 0 | 1) => (2, 2),
            ("miss006", 14, 2..=7) => (2, 4),
            ("miss007", 15, 0 | 1) => (1, if bit(self) { 1 } else { 2 }),
            ("miss007", 15, 2..=7) => (1, if bit(self) { 4 } else { 5 }),
            ("miss008", 15, 0) => (1, if bit(self) { 3 } else { 8 }),
            ("miss008", 15, 1) => (1, if bit(self) { 4 } else { 9 }),
            ("miss008", 15, 2..=7) => (1, 10),
            ("miss009", 14, 0) => (1, 2),
            ("miss009", 14, 1) => (1, 1),
            ("miss009", 14, 2..=9) => (1, 4),
            ("miss010", 15, 0) => (1, 7),
            ("miss010", 15, 1) => (1, 6),
            ("miss010", 15, 2..=9) => (1, 8),
            ("miss011", 14, 0 | 1) => (1, 2),
            ("miss011", 14, 2..=7) => (1, 5),
            ("miss012" | "miss013", 15, 0 | 1) => (1, 2),
            ("miss012" | "miss013", 15, _) => (1, 4),
            ("miss012" | "miss013", 14, 0 | 1) => (2, 2),
            ("miss012" | "miss013", 14, _) => (2, 4),
            _ => return,
        };
        let Some(defs) = mission_fleets(&mission) else {
            return;
        };
        let Some(into) = defs
            .iter()
            .position(|d| d.group == group && d.member == member)
        else {
            return;
        };
        let brain = self.brain.as_mut().unwrap();
        for f in &mut brain.fleets {
            f.ships.retain(|&s| s != id);
        }
        if let Some(f) = brain.fleets.get_mut(into) {
            f.ships.push(id);
        }
    }

    /// Fleet `k`'s damaged ships with nothing to do sail to one of their
    /// player's docks, picked at random, to be mended (0x47a7e0 with order
    /// 0x82: the repair member sends each ship that joins it to a dock,
    /// message 300; as `brain_prize` does for prizes).
    pub(super) fn fleet_to_docks(&mut self, k: usize) {
        for id in self.fleet_ships(k) {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let o = &self.objects[i];
            let idle = o
                .ship
                .as_ref()
                .is_some_and(|s| s.path.is_empty() && s.task.is_none());
            let hurt = o.combat.as_ref().is_some_and(|c| c.hull < c.max_hull);
            if !idle || !hurt {
                continue;
            }
            let owner = o.owner;
            let docks: Vec<u32> = self
                .objects
                .iter()
                .filter(|d| {
                    d.owner == owner
                        && d.class
                            == openconquest_formats::ObjectClass::Building(crate::economy::DOCK)
                })
                .map(|d| d.id)
                .collect();
            if docks.is_empty() {
                continue;
            }
            let dock = docks[self.rng.next() as usize % docks.len()];
            let _ = self.order_repair(id, dock);
        }
    }

    /// Puts a leviathan zone (class 0x192, 0x4960b0) of `area` on the map:
    /// one leviathan (100 to take) that comes up `shows` times; the record
    /// as the original's message 0x167d fills it.
    fn add_leviathan(
        &mut self,
        (x, y, w, h): Cells,
        shows: u32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 41];
        fields[0] = 0x192;
        fields[1] = 2;
        fields[4] = 1;
        fields[5] = 100;
        fields[6] = 100;
        fields[7] = x as u32;
        fields[8] = y as u32;
        fields[10] = w as u32;
        fields[11] = h as u32;
        fields[13] = 8;
        fields[39] = shows;
        let rec = openconquest_formats::ObjectRecord {
            id,
            class_id: 0x192,
            fields,
        };
        match crate::objects::resolve(data, caches, &rec) {
            Ok(Some(obj)) => {
                self.objects.push(obj);
                self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            }
            Ok(None) => {}
            Err(e) => eprintln!("could not put a leviathan: {e:#}"),
        }
    }

    /// A few cells around one of the human player's buildings.
    fn random_human_building(&mut self) -> Option<Cells> {
        let t = TILE_SIZE as f32;
        let spots: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| self.is_own(o) && o.ship.is_none() && o.combat.is_some())
            .map(|o| ((o.x / t) as i32, (o.y / t) as i32))
            .collect();
        if spots.is_empty() {
            return None;
        }
        let (x, y) = spots[self.rng.next() as usize % spots.len()];
        Some((x - 3, y - 3, 7, 7))
    }

    /// Whether the human player has a ship or building in `area`.
    fn human_in(&self, (x, y, w, h): Cells) -> bool {
        let t = TILE_SIZE as f32;
        self.objects.iter().any(|o| {
            self.is_own(o) && o.combat.is_some() && {
                let (cx, cy) = ((o.x / t) as i32, (o.y / t) as i32);
                cx >= x && cx < x + w && cy >= y && cy < y + h
            }
        })
    }

    fn fleet_ships(&self, k: usize) -> Vec<u32> {
        self.brain.as_ref().unwrap().fleets[k].ships.clone()
    }

    fn fleet_move(&mut self, k: usize, (cx, cy): (i32, i32)) {
        let t = TILE_SIZE as f32;
        for (n, id) in self.fleet_ships(k).into_iter().enumerate() {
            // Side by side, not on top of each other.
            let dx = (n as f32 - 0.5) * 24.0;
            self.order_move(id, cx as f32 * t + t / 2.0 + dx, cy as f32 * t + t / 2.0);
        }
    }

    fn fleet_stop(&mut self, k: usize) {
        for id in self.fleet_ships(k) {
            if let Some(i) = self.object_index(id) {
                if let Some(s) = self.objects[i].ship.as_mut() {
                    s.task = None;
                    s.zone = None;
                    s.path.clear();
                }
            }
        }
    }

    fn fleet_gather(&mut self, k: usize, (x, y, w, h): Cells, resource: Resource) {
        let t = TILE_SIZE as f32;
        let (a, b) = (
            (x as f32 * t, y as f32 * t),
            ((x + w) as f32 * t, (y + h) as f32 * t),
        );
        for id in self.fleet_ships(k) {
            if let Some(i) = self.object_index(id) {
                if let Some(s) = self.objects[i].ship.as_mut() {
                    if crate::objects::cargo_capacity(s.kind) > 0.0 {
                        s.zone = Some(((a.0, a.1, b.0, b.1), resource));
                    }
                }
            }
        }
    }

    fn fleet_attack(&mut self, k: usize, (x, y, w, h): Cells) {
        let t = TILE_SIZE as f32;
        let (a, b) = (
            (x as f32 * t, y as f32 * t),
            ((x + w) as f32 * t, (y + h) as f32 * t),
        );
        for id in self.fleet_ships(k) {
            let _ = self.order_zone(id, a, b, true);
        }
    }

    /// No ship of fleet `k` has anything left to do.
    fn fleet_idle(&self, k: usize) -> bool {
        self.fleet_ships(k).into_iter().all(|id| {
            self.object_index(id).is_none_or(|i| {
                self.objects[i]
                    .ship
                    .as_ref()
                    .is_none_or(|s| s.path.is_empty() && s.task.is_none())
            })
        })
    }

    /// Some ship of fleet `k` lies within `cells` of cell `to`.
    fn fleet_near(&self, k: usize, (cx, cy): (i32, i32), cells: f32) -> bool {
        let t = TILE_SIZE as f32;
        let (x, y) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
        self.fleet_ships(k).into_iter().any(|id| {
            self.object_index(id).is_some_and(|i| {
                let o = &self.objects[i];
                ((o.x - x).powi(2) + (o.y - y).powi(2)).sqrt() <= cells * t
            })
        })
    }

    /// The human player's warships of types 4-7 (boarders, rammers, ship
    /// and men killers), as miss000's brain counts them.
    fn human_warships(&self) -> usize {
        self.objects
            .iter()
            .filter(|o| {
                self.is_own(o) && o.ship.as_ref().is_some_and(|s| (4..=7).contains(&s.kind))
            })
            .count()
    }

    /// Casts `spell` on cell `(cx, cy)` for nothing, as the brains do
    /// through `Magic.cpp` (0x42f580, a free cast; 0x47ae10 sends it as
    /// message 0x3ee): lightning (its picture, then its strike, as the
    /// player's); a fireball sparing `owner`'s; a waterspout, whirlpool,
    /// reef or sea monster (fighting for `owner`) on open water only, as
    /// the player may cast them. Other spells are not cast on a cell.
    pub(super) fn brain_cast(
        &mut self,
        spell: crate::magic::Spell,
        cx: i32,
        cy: i32,
        owner: Option<(u32, u32)>,
        data: &GameData,
        caches: &mut Caches,
    ) {
        use crate::magic::Spell;
        let t = TILE_SIZE as f32;
        let (x, y) = (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
        let water = self.nav.is_water(cx, cy);
        match spell {
            Spell::Lightning => {
                self.spawn_effect(data, caches, crate::magic::fx_lightning(water), x, y);
                self.sounds.push(("molniya2", x, y));
                let at = if water { 11 } else { 40 };
                self.lightning.push((x, y, crate::clock::ticks(at), None));
            }
            Spell::Fireball => self.spawn_fireball(cx, cy, owner, data, caches),
            Spell::Waterspout | Spell::Whirlpool | Spell::Reef | Spell::Monster if water => {
                if let Err(e) = self.spawn_hazard(spell, cx, cy, owner, data, caches) {
                    eprintln!("a brain's spell failed: {e}");
                }
            }
            _ => {}
        }
    }

    /// The Persians' levels at the start (each campaign main's message
    /// 0x8d1, whatever the difficulty): arming 2, ram 5 and armour 2
    /// (`+0xa3`, `+0x5e`, `+0x5f`); with `workshop`, the workshop's rate of
    /// fire 1 and 2 for groups 2 and 3 and range and damage 2 for all
    /// (`+0x8e`, `+0x95`, `+0x9c`); and the unique ships of `special`
    /// studied. Players by id (nation x 4 + slot).
    /// The human's levels when the mission is not continued from the one
    /// before (miss007-miss010's start, `+0x3305 <= 0`): arming 2, rate of
    /// fire and range for groups 1-2, damage for all, armour, ram. (In the
    /// shipped game a campaign mission always starts continued; a mission
    /// picked from the list here is the like.)
    pub(super) fn standalone_levels(&mut self, rate: [u8; 2], armour: u8) {
        let Some(h) = self.human else {
            return;
        };
        let l = self.levels.entry((h.nation, h.slot)).or_default();
        l.weapons = l.weapons.max(1);
        l.rate[0] = l.rate[0].max(rate[0]);
        l.rate[1] = l.rate[1].max(rate[1]);
        l.reach[0] = l.reach[0].max(2);
        l.reach[1] = l.reach[1].max(2);
        l.arms = [2; 3];
        l.armour = l.armour.max(armour);
        l.ram = 2;
    }

    pub(super) fn persian_bonus(&mut self, players: &[u32], workshop: bool, special: &[usize]) {
        for &p in players {
            let l = self.levels.entry((p / 4, p % 4)).or_default();
            l.weapons = l.weapons.max(1);
            l.ram = 2;
            l.armour = 2;
            if workshop {
                l.rate = [0, 1, 2];
                l.reach = [2; 3];
                l.arms = [2; 3];
            }
            for &k in special {
                l.special[k] = 1;
            }
        }
    }

    /// A brain's fleet-wide cast on `ships` (0x47acf0 through 0x479c20:
    /// an order to the casting group's war temple for each ship, 0x42a500,
    /// 0x43fee0): only with a war temple standing; each cast paid in full
    /// (0x42f470), with one cast's price credited first when `pay`
    /// (0x4a8eb0, 0x4a8f40). The Flame is the player's: brewed 250 ticks,
    /// then 1500 more of it (the temple's code 5).
    pub(super) fn brain_cast_fleet(
        &mut self,
        ships: Vec<u32>,
        spell: crate::magic::Spell,
        pay: bool,
    ) {
        use crate::magic::Spell;
        let Some(owner) = ships
            .iter()
            .find_map(|&id| self.object_index(id).and_then(|i| self.objects[i].owner))
        else {
            return;
        };
        let temple = self.objects.iter().any(|o| {
            o.owner == Some(owner)
                && o.class == openconquest_formats::ObjectClass::Building(0x3e)
                && self.construction_at(o.id).is_none()
        });
        if !temple {
            return;
        }
        let (amber, fish) = spell.cost();
        if pay {
            self.earn(owner, amber as i64, fish as i64);
        }
        let afford = |s: &Scene| {
            let (a, f) = s.funds(owner);
            a >= amber && f >= fish
        };
        if spell == Spell::Flame {
            let brewing = self.brain_flames.get(&owner).is_some_and(|f| f.0 > 0.0);
            if !brewing && afford(self) {
                self.earn(owner, -(amber as i64), -(fish as i64));
                self.brain_flames.entry(owner).or_default().0 = crate::clock::ticks(250);
            }
            return;
        }
        for id in ships {
            if !afford(self) {
                break;
            }
            self.earn(owner, -(amber as i64), -(fish as i64));
            self.pending_ship_casts.push((spell, id));
        }
    }

    /// The fleet of the brain's member `member` of group `group`.
    pub(super) fn member_fleet(&self, group: u8, member: usize) -> Option<usize> {
        mission_fleets(&self.brain.as_ref()?.mission)?
            .iter()
            .position(|d| d.group == group && d.member as usize == member)
    }

    /// The orders a mission's members give their land fleets at the start
    /// (0x47b130): the yards' full-town orders (`full`, by group and
    /// member) and the builders' repairs (`mend`).
    pub(super) fn start_jobs(&mut self, full: &[(u8, usize)], mend: &[(u8, usize)]) {
        let mut jobs = Vec::new();
        for &(g, m) in full {
            if let Some(fleet) = self.member_fleet(g, m) {
                jobs.push(Job::FullTown {
                    fleet,
                    forts: false,
                });
            }
        }
        for &(g, m) in mend {
            if let Some(fleet) = self.member_fleet(g, m) {
                jobs.push(Job::Repair { fleet, idle: 10 });
            }
        }
        self.brain.as_mut().unwrap().jobs.extend(jobs);
    }

    /// The town standing in fleet `k`'s cells (a yard's), by index.
    fn fleet_town(&self, k: usize) -> Option<usize> {
        let defs = mission_fleets(&self.brain.as_ref()?.mission)?;
        let cells = defs.get(k)?.cells;
        (0..self.objects.len()).find(|&i| {
            matches!(
                self.objects[i].class,
                openconquest_formats::ObjectClass::Building(
                    crate::economy::SETTLEMENT | crate::economy::TOWN
                )
            ) && self.objects[i].cell.is_some_and(|(x, y)| {
                cells
                    .iter()
                    .any(|&(cx, cy, w, h)| (cx..cx + w).contains(&x) && (cy..cy + h).contains(&y))
            })
        })
    }

    /// The first part of the full-town order the town at `t` lacks: a
    /// building class to put up, or a building to upgrade (`Err(id)`);
    /// `None` when it is complete. A part being built waits (`Some(Ok(0))`).
    fn town_part(&self, t: usize, forts: bool) -> Option<Result<u32, u32>> {
        use crate::economy::{
            DOCK, PORT, SCHOOL, SETTLEMENT, SHIPYARD, SMITHY, TEMPLE_OF_THE_SEA, TEMPLE_OF_WAR,
            TOWN,
        };
        use openconquest_formats::ObjectClass;
        let site = self.objects[t].id;
        let owner = self.objects[t].owner;
        let of = |classes: &[u32]| -> Vec<usize> {
            (0..self.objects.len())
                .filter(|&i| {
                    self.objects[i].owner == owner
                        && matches!(self.objects[i].class, ObjectClass::Building(c) if classes.contains(&c))
                        && self.home_settlement(i) == Some(site)
                })
                .collect()
        };
        let building = |v: &[usize]| {
            v.iter()
                .any(|&i| self.construction_at(self.objects[i].id).is_some())
        };
        for classes in [&[PORT][..], &[0x37], &[SHIPYARD, DOCK], &[SMITHY]] {
            let have = of(classes);
            if have.is_empty() {
                return Some(Ok(classes[0]));
            }
            if building(&have) {
                return Some(Ok(0));
            }
        }
        if self.construction_at(site).is_some() {
            return Some(Ok(0));
        }
        if self.objects[t].class == ObjectClass::Building(SETTLEMENT) {
            return Some(Err(site));
        }
        let _ = TOWN;
        if of(&[DOCK]).is_empty() {
            let yard = of(&[SHIPYARD]);
            return Some(match yard.first() {
                Some(&y) if self.production_at(self.objects[y].id).is_none() => {
                    Err(self.objects[y].id)
                }
                _ => Ok(0),
            });
        }
        let rest: &[u32] = if forts {
            &[SCHOOL, 0x34, 0x35]
        } else {
            &[SCHOOL, TEMPLE_OF_THE_SEA, TEMPLE_OF_WAR]
        };
        // A town's forts are those about it (the town's counters +0x214,
        // +0x218; free-standing forts further off do not count).
        let (tx, ty) = self.objects[t].cell.unwrap_or((0, 0));
        let r = super::BUILD_RADIUS + 2;
        let forts_of = |class: u32| -> Vec<usize> {
            (0..self.objects.len())
                .filter(|&i| {
                    self.objects[i].owner == owner
                        && self.objects[i].class == ObjectClass::Building(class)
                        && self.objects[i]
                            .cell
                            .is_some_and(|(x, y)| (x - tx).abs() <= r && (y - ty).abs() <= r)
                })
                .collect()
        };
        for &class in rest {
            let have = if (0x34..=0x36).contains(&class) {
                forts_of(class)
            } else {
                of(&[class])
            };
            if have.is_empty() {
                return Some(Ok(class));
            }
            if building(&have) {
                return Some(Ok(0));
            }
        }
        None
    }

    /// Whether yard `k`'s town has all the full-town order asks for.
    pub(super) fn yard_town_complete(&self, k: usize) -> bool {
        self.fleet_town(k)
            .is_some_and(|t| self.town_part(t, false).is_none())
    }

    /// Whether yard `k`'s town has all order 240 asks for.
    pub(super) fn yard_town_fortified(&self, k: usize) -> bool {
        self.fleet_town(k)
            .is_some_and(|t| self.town_part(t, true).is_none())
    }

    /// Yard `k` gives its full-town order, if it has not already.
    pub(super) fn want_full_town(&mut self, k: usize) {
        self.want_town_order(k, false);
    }

    /// The same, of order 240 with `forts` (the yards of miss002-004).
    pub(super) fn want_town_order(&mut self, k: usize, forts: bool) {
        let brain = self.brain.as_mut().unwrap();
        let job = Job::FullTown { fleet: k, forts };
        if !brain.jobs.contains(&job) {
            brain.jobs.push(job);
        }
    }

    /// Carries the brain's town orders on, a pass every 50 ticks.
    pub(super) fn run_brain_jobs(&mut self, data: &GameData, caches: &mut Caches) {
        let Some(brain) = self.brain.as_ref() else {
            return;
        };
        if brain.tick % 50 != 0 || brain.jobs.is_empty() {
            return;
        }
        let jobs = brain.jobs.clone();
        let mut keep = Vec::new();
        for job in jobs {
            match job {
                Job::FullTown { fleet, forts } => {
                    let Some(t) = self.fleet_town(fleet) else {
                        continue;
                    };
                    let site = self.objects[t].id;
                    match self.town_part(t, forts) {
                        None => continue,
                        Some(Ok(0)) => {}
                        Some(Ok(class)) => {
                            if let Some((x, y, facing)) = self.town_build_place(site, class) {
                                let _ = self.place_building_facing(
                                    site,
                                    class,
                                    x,
                                    y,
                                    Some(facing),
                                    data,
                                    caches,
                                );
                            }
                        }
                        Some(Err(id)) => {
                            let _ = self.upgrade(id, data, caches);
                        }
                    }
                    keep.push(job);
                }
                Job::Repair { fleet, idle } => {
                    let defs = mission_fleets(&self.brain.as_ref().unwrap().mission).unwrap_or(&[]);
                    let cells = defs.get(fleet).map_or(&[][..], |d| d.cells);
                    let worn = self.objects.iter().find(|o| {
                        o.ship.is_none()
                            && o.combat
                                .as_ref()
                                .is_some_and(|c| c.hull > 0.0 && c.hull < c.max_hull)
                            && o.cell.is_some_and(|(x, y)| {
                                cells.iter().any(|&(cx, cy, w, h)| {
                                    (cx..cx + w).contains(&x) && (cy..cy + h).contains(&y)
                                })
                            })
                    });
                    match worn.map(|o| o.id) {
                        Some(id) => {
                            if !self.mending.contains(&id) {
                                self.mending.push(id);
                            }
                        }
                        None if idle > 1 => keep.push(Job::Repair {
                            fleet,
                            idle: idle - 1,
                        }),
                        None => {}
                    }
                }
            }
        }
        self.brain.as_mut().unwrap().jobs = keep;
    }

    /// The brains' players' Flame: brewing, then burning.
    pub(super) fn run_brain_flames(&mut self, dt: f32) {
        for (brew, flame) in self.brain_flames.values_mut() {
            if *brew > 0.0 {
                *brew -= dt;
                if *brew <= 0.0 {
                    *brew = 0.0;
                    *flame += crate::magic::FLAME_SECONDS;
                }
            } else {
                *flame = (*flame - dt).max(0.0);
            }
        }
    }

    /// Casts a shield or the phantom on ship `id` for nothing (0x47acf0,
    /// each ship of a fleet through 0x479c20), as the player's `cast_on`
    /// does. The Flame is the player's in the port, not a ship's: not
    /// cast.
    pub(super) fn brain_cast_on(
        &mut self,
        spell: crate::magic::Spell,
        id: u32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        use crate::magic::Spell;
        if matches!(
            spell,
            Spell::Phantom | Spell::ProtectWeapons | Spell::ProtectMagic
        ) {
            if let Some(i) = self.object_index(id) {
                if self.objects[i].combat.is_some() {
                    self.protect(spell, i, data, caches);
                }
            }
        }
    }

    /// Puts an object made from a record of `class` with `fields` on the
    /// map (the creatures the brains lay at the start: 0x404a20 a siren).
    fn add_record(&mut self, class: u32, fields: Vec<u32>, data: &GameData, caches: &mut Caches) {
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let rec = openconquest_formats::ObjectRecord {
            id,
            class_id: class,
            fields,
        };
        match crate::objects::resolve(data, caches, &rec) {
            Ok(Some(obj)) => {
                self.objects.push(obj);
                self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            }
            Ok(None) => {}
            Err(e) => eprintln!("could not put a {class:#x}: {e:#}"),
        }
    }

    /// Puts an amber deposit (0x48b180) or a fishing ground (0x48ebc0) on
    /// the map, its first cell at `(x, y)`; deposits laid by the brains are
    /// unlimited.
    fn add_deposit(
        &mut self,
        class: u32,
        (x, y, w, h): Cells,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 12];
        fields[0] = class;
        fields[4] = u32::MAX; // unlimited
        fields[6] = x as u32;
        fields[7] = y as u32;
        fields[8] = w as u32;
        fields[9] = h as u32;
        let rec = openconquest_formats::ObjectRecord {
            id,
            class_id: class,
            fields,
        };
        if let Ok(Some(obj)) = crate::objects::resolve(data, caches, &rec) {
            self.objects.push(obj);
            self.objects.sort_by(|a, b| a.y.total_cmp(&b.y));
            if let Some(i) = self.object_index(id) {
                self.refresh_amber(i, data, caches);
            }
        }
    }
}

/// miss000's brain (0x449b10): the barbarians' three fleets.
mod miss000 {
    use super::*;

    pub const FLEETS: &[FleetDef] = &[
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(9, 21, 1, 1), (10, 22, 1, 1)],
            program: guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(11, 18, 1, 1)],
            program: fisher,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(6, 13, 1, 1), (6, 14, 1, 1)],
            program: raider,
            wants: &[],
        },
    ];

    /// The player starts with more amber and fish the easier the game
    /// (4000/2000, 2000/1000, none).
    pub fn start(scene: &mut Scene, _: &GameData, _: &mut Caches) {
        const BONUS: [(i64, i64); 3] = [(4000, 2000), (2000, 1000), (0, 0)];
        if let Some(h) = scene.human {
            let (amber, fish) = BONUS[difficulty()];
            scene.earn((h.nation, h.slot), amber, fish);
        }
    }

    /// The zones the fleets watch (0x5ad930, 0x5ad9c8).
    const BAYS: [Event; 2] = [
        Event::Zone {
            state: 20,
            area: (2, 16, 15, 8),
        },
        Event::Zone {
            state: 20,
            area: (17, 16, 7, 4),
        },
    ];

    /// Two ship killers patrol the south and attack whatever comes into
    /// the bays (0x4495a0).
    fn guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(10, 22, 750), (12, 13, 1500)];
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: &BAYS,
            },
        }
    }

    /// A merchant fishes two grounds by turns (0x449680).
    fn fisher(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (7, 12, 17, 6),
                resource: Resource::Fish,
                ticks: 7500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (13, 23),
                ticks: 750,
                events: &[],
            },
            17 => Op::Wait {
                next: 18,
                ticks: 1500,
            },
            18 => Op::Gather {
                next: 19,
                area: (10, 17, 6, 6),
                resource: Resource::Fish,
                ticks: 6000,
                events: &[],
            },
            19 => Op::Move {
                ok: 20,
                failed: 20,
                to: (23, 14),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 3000,
            },
        }
    }

    /// Two ship killers patrol the middle, attack what they see, and when
    /// the player has built more warships than the difficulty allows, raid
    /// the player's harbour (0x449810).
    fn raider(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(6, 13, 500), (10, 12, 750), (15, 13, 500), (20, 16, 250)];
        const WATCH: &[Event] = &[
            BAYS[0],
            BAYS[1],
            Event::Zone {
                state: 20,
                area: (3, 10, 21, 7),
            },
        ];
        /// How long a round lasts by difficulty (0x5ad924), and how many
        /// warships the player may have before a raid (0x5ad918).
        const ROUND_TICKS: [u32; 3] = [6000, 4500, 4500];
        const TOLERATED: [usize; 3] = [3, 2, 1];
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            40 => Op::Custom(|scene, _| {
                if scene.human_warships() <= TOLERATED[difficulty()] {
                    0
                } else {
                    50
                }
            }),
            // Two ways to the player's harbour, picked at random.
            50 => Op::Custom(|scene, _| if scene.rng.next() % 2 == 0 { 51 } else { 52 }),
            51 => Op::Move {
                ok: 55,
                failed: 55,
                to: (6, 13),
                ticks: 750,
                events: &[],
            },
            52 => Op::Move {
                ok: 55,
                failed: 55,
                to: (20, 16),
                ticks: 750,
                events: &[],
            },
            55 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((15, 0, 9, 6)),
                ticks: 4500,
                events: &[],
            },
            // A leg that ends short starts the round again (0x47a900 at
            // 0x449810 state 5: p3 0x28, p4 5).
            _ => Op::Patrol {
                timeout: 40,
                failed: 5,
                points: ROUND,
                ticks: ROUND_TICKS[difficulty()],
                events: WATCH,
            },
        }
    }
}

/// miss001's brain (0x44af70): the barbarians build ships at their yard
/// for four fleets that guard their coast, gather, and raid the player.
mod miss001 {
    use super::*;

    /// Ship types: royal ship, rammer, ship killer, merchant.
    const ROYAL: u8 = 2;
    const RAMMER: u8 = 5;
    const SHIP_KILLER: u8 = 6;
    const MERCHANT: u8 = 1;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians' yard and stronghold build what the others ask for.
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(25, 1, 1, 1), (27, 3, 1, 1)],
            program: builder,
            wants: &[],
        },
        // A royal ship and a rammer guard the coast (they want more only on
        // the hardest difficulty).
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(23, 4, 1, 1), (23, 7, 1, 1)],
            program: coastguard,
            wants: &[(ROYAL, [0, 0, 1]), (RAMMER, [0, 0, 1])],
        },
        // A merchant with a ship killer for escort gathers amber and fish.
        FleetDef {
            group: 0,
            member: 2,
            cells: &[],
            program: gatherers,
            wants: &[(MERCHANT, [1; 3]), (SHIP_KILLER, [1; 3])],
        },
        // Ship killers patrol the middle of the sea (0x5adafc).
        FleetDef {
            group: 0,
            member: 3,
            cells: &[],
            program: patrol,
            wants: &[(SHIP_KILLER, [2, 3, 4])],
        },
        // Ship killers and royal ships raid the player when strong enough
        // (0x5adb2c).
        FleetDef {
            group: 0,
            member: 4,
            cells: &[],
            program: raiders,
            wants: &[(SHIP_KILLER, [2; 3]), (ROYAL, [1, 2, 3])],
        },
    ];

    /// Amber deposits and a fishing ground near the barbarians (0x48b180,
    /// 0x48ebc0; the third deposit only below the hardest difficulty).
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.add_deposit(0x190, (2, 11, 1, 1), data, caches);
        scene.add_deposit(0x190, (3, 16, 1, 1), data, caches);
        if difficulty() != 2 {
            scene.add_deposit(0x190, (7, 6, 1, 1), data, caches);
        }
        scene.add_deposit(0x191, (4, 4, 2, 2), data, caches);
        // The raiders' timing (0x44ac80, 0x5adb08/14/20).
        const RAID_AFTER: [u32; 3] = [8, 6, 4];
        const RAID_SIZE: [u32; 3] = [2, 3, 4];
        const RETREAT_AT: [u32; 3] = [1, 1, 3];
        let wait = (RAID_AFTER[difficulty()] + scene.rng.next() % 4) * 1500;
        if let Some(b) = scene.brain.as_mut() {
            b.fleets[4].vars = [wait, RAID_SIZE[difficulty()], RETREAT_AT[difficulty()], 0];
        }
    }

    /// The zones the fleets watch, each to be attacked when the player
    /// shows up in it (0x5adb90, 0x5adcf8, 0x5adea0).
    const HOME: [Event; 2] = [
        Event::Zone {
            state: 20,
            area: (18, 0, 8, 11),
        },
        Event::Zone {
            state: 20,
            area: (26, 4, 4, 6),
        },
    ];
    const SEA: [Event; 4] = [
        Event::Zone {
            state: 20,
            area: (12, 0, 9, 8),
        },
        Event::Zone {
            state: 20,
            area: (11, 8, 9, 8),
        },
        Event::Zone {
            state: 20,
            area: (15, 15, 15, 5),
        },
        Event::Zone {
            state: 20,
            area: (17, 11, 10, 4),
        },
    ];
    const RAIDER_WATCH: [Event; 4] = [HOME[0], HOME[1], SEA[1], SEA[2]];

    /// Builds the ships asked for, then rests (0x44a4e0, 0x5adaf0).
    fn builder(state: u32) -> Op {
        const REST: [u32; 3] = [2000, 1750, 1000];
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: REST[difficulty()],
            },
        }
    }

    /// Patrols the barbarians' coast; attacks the player in it, or whoever
    /// fires on it (0x44a6f0).
    fn coastguard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(22, 0, 750), (25, 5, 1000), (21, 8, 750)];
        const EVENTS: &[Event] = &[
            HOME[0],
            HOME[1],
            Event::Attacked {
                state: 30,
                after: 250,
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: PREVIOUS,
                failed: PREVIOUS,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: EVENTS,
            },
        }
    }

    /// Gathers amber, brings it home, then fish (0x44a8b0).
    fn gatherers(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
                area: (17, 7, 4, 4),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            7 => Op::Move {
                ok: 9,
                failed: 9,
                to: (29, 5),
                ticks: 750,
                events: &[],
            },
            9 => Op::Wait {
                next: 11,
                ticks: 1500,
            },
            11 => Op::Gather {
                next: 13,
                area: (22, 0, 8, 8),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[],
            },
            13 => Op::Move {
                ok: 15,
                failed: 15,
                to: (29, 5),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 1500,
            },
        }
    }

    /// Patrols the middle of the sea and attacks the player there (0x44ab40).
    fn patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(21, 16, 500), (12, 8, 750), (15, 2, 500), (21, 12, 875)];
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: PREVIOUS,
                failed: PREVIOUS,
                target: Target::Seen,
                ticks: 4500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: &SEA,
            },
        }
    }

    /// Waits off the barbarians' coast; once long enough has passed and the
    /// fleet is strong enough, raids the player's harbour, falling back when
    /// down to a few ships (0x44ac80).
    fn raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(25, 15, 1500), (19, 13, 750)];
        const WAITING: &[Event] = &[
            RAIDER_WATCH[0],
            RAIDER_WATCH[1],
            RAIDER_WATCH[2],
            RAIDER_WATCH[3],
            Event::Size {
                state: 10,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
        ];
        const RAIDING: &[Event] = &[
            RAIDER_WATCH[0],
            RAIDER_WATCH[1],
            RAIDER_WATCH[2],
            RAIDER_WATCH[3],
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: 2,
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            10 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((1, 1, 9, 6)),
                ticks: 6000,
                events: RAIDING,
            },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 0,
                events: WAITING,
            },
        }
    }
}

/// miss002's brain (0x44d0e0): the barbarians guard their coast in the
/// north-east; the Persians in the south-east gather, guard and raid, and
/// both build at their yards.
mod miss002 {
    use super::*;

    const ROYAL: u8 = 2;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;
    const SHIP_KILLER: u8 = 6;
    const MERCHANT: u8 = 1;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (0x44bba0, 0x44bd80).
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(33, 3, 1, 1)],
            program: barbarian_yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(30, 4, 3, 3)],
            program: barbarian_guard,
            wants: &[(RAMMER, [2, 3, 3]), (ROYAL, [1, 1, 2])],
        },
        // The Persians (0x44bee0-0x44cbd0).
        FleetDef {
            group: 1,
            member: 0,
            cells: &[],
            program: amber_gatherers,
            wants: &[(MERCHANT, [1; 3]), (SHIP_KILLER, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[],
            program: fishers,
            wants: &[(MERCHANT, [1; 3]), (SHIP_KILLER, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[],
            program: home_guard,
            wants: &[(ROYAL, [3; 3]), (BOARDER, [1; 3]), (RAMMER, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[],
            program: raiders,
            wants: &[(ROYAL, [1, 1, 2]), (BOARDER, [1, 2, 1]), (RAMMER, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[],
            program: west_guard,
            wants: &[(ROYAL, [2; 3]), (BOARDER, [2; 3])],
        },
        FleetDef {
            group: 1,
            member: 5,
            cells: &[],
            program: south_guard,
            wants: &[(ROYAL, [2; 3]), (RAMMER, [3; 3])],
        },
        FleetDef {
            group: 1,
            // (1, 6) does nothing (0x406330), (1, 8) is the builders'
            // repair (0x44cf20): the yard is (1, 7) (0x44cbd0).
            member: 7,
            cells: &[(34, 29, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
    ];

    /// Amber deposits and a fishing ground (0x5ae094...).
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.add_deposit(0x190, (31, 1, 1, 1), data, caches);
        scene.add_deposit(0x190, (11, 3, 1, 1), data, caches);
        scene.add_deposit(0x190, (9, 18, 1, 1), data, caches);
        scene.add_deposit(0x191, (8, 10, 2, 2), data, caches);
        // How many ships the raiders fall back at (fewer than three).
        if let Some(b) = scene.brain.as_mut() {
            b.fleets[5].vars[1] = 2;
        }
    }

    fn barbarian_yard(state: u32) -> Op {
        const REST: [u32; 3] = [2000, 1750, 1000];
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: REST[difficulty()],
            },
        }
    }

    fn persian_yard(state: u32) -> Op {
        match state {
            // Order 240 before every ship (0x47b130 code 0xf0).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_fortified(k) {
                    1
                } else {
                    scene.want_town_order(k, true);
                    0
                }
            }),
            1 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 500,
            },
        }
    }

    /// The barbarians' coast (0x5ae1e0).
    const BARBARIAN_COAST: &[Event] = &[
        Event::Zone {
            state: 20,
            area: (26, 4, 10, 9),
        },
        Event::Zone {
            state: 20,
            area: (21, 4, 5, 7),
        },
        Event::Zone {
            state: 20,
            area: (21, 0, 11, 4),
        },
    ];

    fn barbarian_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(24, 2, 750), (24, 7, 1000), (33, 9, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: PREVIOUS,
                failed: PREVIOUS,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: BARBARIAN_COAST,
            },
        }
    }

    /// The Persians' harbour, and its approaches.
    const HARBOUR: Event = Event::Zone {
        state: 20,
        area: (27, 25, 9, 10),
    };
    const APPROACH: &[Event] = &[
        HARBOUR,
        Event::Zone {
            state: 20,
            area: (26, 21, 5, 4),
        },
    ];

    fn amber_gatherers(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (23, 32, 13, 8),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[HARBOUR],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (24, 39),
                ticks: 500,
                events: &[HARBOUR],
            },
            17 => Op::Wait {
                next: 5,
                ticks: 3000,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn fishers(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 0,
                area: (23, 32, 8, 13),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[HARBOUR],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    /// Holds the harbour mouth (0x44c280).
    fn home_guard(state: u32) -> Op {
        const WATCH: &[Event] = &[
            HARBOUR,
            APPROACH[1],
            Event::Zone {
                state: 20,
                area: (27, 21, 9, 3),
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 0,
                failed: 0,
                to: (33, 24),
                ticks: 3000,
                events: WATCH,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: WATCH,
            },
        }
    }

    /// Gathers off the harbour; once three strong, waits a while, then
    /// sweeps the middle of the sea and attacks the player there, falling
    /// back when down to two ships (0x44c460).
    fn raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(21, 14, 500), (27, 20, 750)];
        const SWEEP: &[Event] = &[
            HARBOUR,
            Event::Zone {
                state: 50,
                area: (10, 8, 8, 4),
            },
            Event::Zone {
                state: 50,
                area: (13, 12, 2, 2),
            },
            Event::Zone {
                state: 50,
                area: (15, 11, 12, 10),
            },
            Event::Zone {
                state: 50,
                area: (27, 14, 9, 11),
            },
        ];
        const FIGHTING: &[Event] = &[
            APPROACH[0],
            APPROACH[1],
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: 1,
            },
        ];
        /// The wait before a sweep, in 1500-tick units (0x5ae058).
        const WAIT: [u32; 3] = [8, 6, 4];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Custom(|scene, k| {
                if scene.brain.as_ref().unwrap().fleets[k].ships.len() < 3 {
                    6
                } else {
                    8
                }
            }),
            6 => Op::Move {
                ok: 0,
                failed: 0,
                to: (34, 19),
                ticks: 0,
                events: APPROACH,
            },
            8 => Op::Move {
                ok: 7,
                failed: 7,
                to: (34, 19),
                ticks: 0,
                events: APPROACH,
            },
            7 => Op::WaitVar { next: 10, var: 0 },
            10 => Op::Custom(|scene, k| {
                let r = scene.rng.next() % 4;
                let f = &mut scene.brain.as_mut().unwrap().fleets[k];
                if f.vars[0] == 0 {
                    f.vars[0] = (r + WAIT[difficulty()]) * 1500;
                }
                40
            }),
            40 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 3000,
                events: SWEEP,
            },
            50 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: FIGHTING,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: APPROACH,
            },
        }
    }

    /// Guards the west and south-west (0x44c790, 0x44c9b0): holds a post,
    /// and when the player shows up further off, patrols there.
    const WEST: &[Event] = &[
        HARBOUR,
        Event::Zone {
            state: 40,
            area: (8, 25, 4, 13),
        },
        Event::Zone {
            state: 40,
            area: (11, 21, 12, 7),
        },
        Event::Zone {
            state: 40,
            area: (11, 28, 9, 12),
        },
    ];

    fn west_guard(state: u32) -> Op {
        const POST: &[Event] = &[
            HARBOUR,
            Event::Zone {
                state: 20,
                area: (22, 26, 5, 5),
            },
            Event::Zone {
                state: 30,
                area: (2, 32, 11, 4),
            },
        ];
        const ROUND: &[(i32, i32, u32)] = &[(19, 23, 1500), (17, 31, 1000)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 0,
                failed: 0,
                to: (25, 28),
                ticks: 3000,
                events: POST,
            },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: POST,
            },
            30 => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: WEST,
            },
            _ => Op::Attack {
                next: 30,
                failed: 30,
                target: Target::Seen,
                ticks: 1500,
                events: WEST,
            },
        }
    }

    fn south_guard(state: u32) -> Op {
        const POST: &[Event] = &[
            HARBOUR,
            Event::Zone {
                state: 20,
                area: (20, 32, 16, 8),
            },
            Event::Zone {
                state: 30,
                area: (2, 32, 11, 4),
            },
        ];
        const ROUND: &[(i32, i32, u32)] = &[(9, 27, 1000), (13, 34, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 0,
                failed: 0,
                to: (23, 35),
                ticks: 3000,
                events: POST,
            },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: POST,
            },
            30 => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: WEST,
            },
            _ => Op::Attack {
                next: 30,
                failed: 30,
                target: Target::Seen,
                ticks: 1500,
                events: WEST,
            },
        }
    }
}

/// miss003's brain (0x44fd90): the Persians hold the north-east and the
/// south-east and raid towards Chios; the barbarians in the south-west
/// gather and guard, and raid the player.
/// (Not ported: the brain's pass after the members, every 5 ticks: the
/// player's things in (49, 26, 4, 3) or (48, 12, 3, 2) (0x4c46c0, once
/// each, `+0x114` and `+0x120`) set off calls of 0x414db0 with classes
/// 0x1f5 and 0x1f6 at given cells (each gated by 0x42ebf0 and an empty
/// cell, more on normal and hard); not decoded.)
mod miss003 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;
    const SHIP_KILLER: u8 = 6;

    pub const FLEETS: &[FleetDef] = &[
        // The Persians (0x44e290-0x44f050).
        FleetDef {
            group: 0,
            member: 0,
            cells: &[],
            program: persian_gatherers,
            wants: &[(MERCHANT, [1; 3]), (SHIP_KILLER, [1; 3])],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(54, 3, 3, 3)],
            program: persian_guard,
            wants: &[(ROYAL, [2, 3, 3]), (RAMMER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(51, 0, 2, 2)],
            program: persian_raiders,
            wants: &[(BOARDER, [2, 3, 2]), (ROYAL, [2, 2, 3])],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[(58, 1, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        // (0, 4) mends (0x452fb0, not ported): the southern guard is (0, 5).
        FleetDef {
            group: 0,
            member: 5,
            cells: &[(52, 40, 5, 5)],
            program: southern_guard,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [2, 3, 3])],
        },
        // The barbarians (0x44f1e0-0x44f880).
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(16, 31, 1, 1)],
            program: barbarian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[(11, 21, 1, 1)],
            program: barbarian_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[(11, 20, 1, 1)],
            program: barbarian_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[(12, 23, 3, 1)],
            program: barbarian_guard,
            wants: &[(ROYAL, [2; 3]), (RAMMER, [2; 3])],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[],
            program: barbarian_raiders,
            wants: &[(RAMMER, [1, 2, 2]), (ROYAL, [2; 3])],
        },
    ];

    /// Amber deposits and a fishing ground (0x5ae8cc...).
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.add_deposit(0x190, (24, 14, 1, 1), data, caches);
        scene.add_deposit(0x190, (23, 21, 1, 1), data, caches);
        scene.add_deposit(0x190, (34, 11, 1, 1), data, caches);
        scene.add_deposit(0x191, (25, 15, 2, 2), data, caches);
        // The raids' waits, in 1500-tick units plus up to three more
        // (0x5ae860, 0x5ae8c0), and the barbarian yard's rest (0x5ae884).
        const PERSIAN_WAIT: [u32; 3] = [9, 7, 5];
        const BARBARIAN_WAIT: [u32; 3] = [11, 9, 7];
        const YARD_REST: [u32; 3] = [1000, 750, 500];
        let (a, b) = (scene.rng.next() % 4, scene.rng.next() % 4);
        if let Some(br) = scene.brain.as_mut() {
            br.fleets[2].vars = [(a + PERSIAN_WAIT[difficulty()]) * 1500, 2, 0, 0];
            br.fleets[5].vars[0] = YARD_REST[difficulty()];
            br.fleets[9].vars = [(b + BARBARIAN_WAIT[difficulty()]) * 1500, 0, 0, 0];
        }
    }

    const NORTH_EAST: Event = Event::Zone {
        state: 20,
        area: (43, 0, 15, 7),
    };

    fn persian_gatherers(state: u32) -> Op {
        const W: &[Event] = &[NORTH_EAST];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (39, 0, 21, 11),
                resource: Resource::Amber,
                ticks: 4500,
                events: W,
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (54, 0),
                ticks: 1000,
                events: W,
            },
            17 => Op::Wait {
                next: 25,
                ticks: 3000,
            },
            25 => Op::Gather {
                next: 30,
                area: (35, 0, 16, 9),
                resource: Resource::Fish,
                ticks: 4500,
                events: W,
            },
            30 => Op::Move {
                ok: 35,
                failed: 35,
                to: (54, 0),
                ticks: 1000,
                events: W,
            },
            35 => Op::Wait {
                next: 0,
                ticks: 3000,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn persian_guard(state: u32) -> Op {
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (43, 0, 15, 10),
        }];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 0,
                failed: 0,
                to: (54, 4),
                ticks: 3000,
                events: W,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
        }
    }

    fn persian_yard(state: u32) -> Op {
        match state {
            // Order 240 before every ship (0x47b130 code 0xf0).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_fortified(k) {
                    1
                } else {
                    scene.want_town_order(k, true);
                    0
                }
            }),
            1 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 1000,
            },
        }
    }

    /// Waits in the north-east until strong; patrols for a while, then
    /// raids the west (Chios) if the player is there, or the middle, and
    /// comes back when down to two ships (0x44e790).
    fn persian_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(38, 2, 500), (47, 6, 500), (52, 10, 500), (47, 6, 500)];
        const W: &[Event] = &[NORTH_EAST];
        const FIGHT: &[Event] = &[
            NORTH_EAST,
            Event::Zone {
                state: 20,
                area: (34, 0, 11, 8),
            },
            Event::Zone {
                state: 20,
                area: (38, 5, 22, 7),
            },
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: 1,
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Custom(|scene, k| {
                if scene.brain.as_ref().unwrap().fleets[k].ships.len() >= 5 {
                    6
                } else {
                    4
                }
            }),
            4 => Op::Move {
                ok: 0,
                failed: 0,
                to: (51, 0),
                ticks: 0,
                events: W,
            },
            6 => Op::Patrol {
                timeout: 40,
                failed: 40,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            40 => Op::Custom(|scene, _| {
                if scene.human_in((3, 3, 21, 12)) {
                    45
                } else {
                    46
                }
            }),
            45 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((3, 3, 21, 12)),
                ticks: 3000,
                events: FIGHT,
            },
            46 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((32, 16, 7, 7)),
                ticks: 3000,
                events: FIGHT,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
        }
    }

    fn southern_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(52, 46, 500), (50, 39, 500), (58, 40, 500)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (51, 36, 9, 12),
            },
            Event::Zone {
                state: 20,
                area: (44, 39, 7, 9),
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
            _ => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 4500,
                events: W,
            },
        }
    }

    fn barbarian_yard(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::WaitVar { next: 0, var: 0 },
        }
    }

    fn barbarian_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
                area: (2, 18, 20, 10),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            7 => Op::Move {
                ok: 9,
                failed: 9,
                to: (7, 21),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 3000,
            },
        }
    }

    fn barbarian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
                area: (2, 18, 20, 10),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[],
            },
            7 => Op::Move {
                ok: 9,
                failed: 9,
                to: (7, 21),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 3000,
            },
        }
    }

    const HOME: Event = Event::Zone {
        state: 20,
        area: (15, 24, 11, 14),
    };

    fn barbarian_guard(state: u32) -> Op {
        const W: &[Event] = &[
            HOME,
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        match state {
            0 => Op::Behave { next: 3 },
            3 => Op::Move {
                ok: 5,
                failed: 5,
                to: (21, 29),
                ticks: 1000,
                events: W,
            },
            5 => Op::Wait {
                next: 0,
                ticks: 500,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 250,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    /// Gathers until three strong, patrols the south-west for a while, then
    /// raids one of the player's buildings (0x44f880).
    fn barbarian_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (16, 21, 750),
            (17, 24, 1000),
            (25, 27, 750),
            (20, 34, 750),
            (25, 27, 750),
            (17, 24, 1000),
        ];
        const W: &[Event] = &[
            HOME,
            Event::Zone {
                state: 20,
                area: (10, 16, 8, 8),
            },
            Event::Zone {
                state: 20,
                area: (18, 18, 7, 6),
            },
            Event::Zone {
                state: 20,
                area: (25, 24, 3, 6),
            },
            Event::Zone {
                state: 20,
                area: (24, 34, 3, 4),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Custom(|scene, k| {
                if scene.brain.as_ref().unwrap().fleets[k].ships.len() >= 3 {
                    6
                } else {
                    4
                }
            }),
            4 => Op::Move {
                ok: 0,
                failed: 0,
                to: (18, 22),
                ticks: 1000,
                events: &[HOME],
            },
            6 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 250,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }
}

/// miss004's brain (0x453b00): two Persian players hold the west (Pylos is
/// besieged) and raid the middle; the barbarians in the south-east gather,
/// guard and raid.
mod miss004 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;

    pub const FLEETS: &[FleetDef] = &[
        // The northern Persians (player 15; 0x4515f0-0x452240).
        FleetDef {
            group: 0,
            member: 0,
            cells: &[],
            program: north_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[],
            program: north_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[],
            program: north_post_a,
            wants: &[(ROYAL, [2, 3, 3])],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[],
            program: north_post_b,
            wants: &[(ROYAL, [2, 2, 3])],
        },
        FleetDef {
            group: 0,
            member: 4,
            cells: &[],
            program: north_raiders,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [1, 2, 3])],
        },
        FleetDef {
            group: 0,
            member: 5,
            cells: &[(1, 8, 2, 2)],
            program: yard_750,
            wants: &[],
        },
        // The barbarians (player 16; 0x453030-0x4536c0).
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(30, 34, 1, 1)],
            program: yard_var,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[(26, 29, 1, 1)],
            program: barbarian_gatherers,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[(31, 32, 2, 2)],
            program: barbarian_guard,
            wants: &[(ROYAL, [1, 2, 2])],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[(36, 30, 2, 2)],
            program: barbarian_raiders,
            wants: &[(RAMMER, [2, 2, 3]), (ROYAL, [1, 2, 2])],
        },
        // The southern Persians (player 14; 0x452600-0x452c60).
        FleetDef {
            group: 2,
            member: 0,
            cells: &[],
            program: south_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            member: 1,
            cells: &[],
            program: south_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            member: 2,
            cells: &[],
            program: south_raiders,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [1, 2, 2])],
        },
        FleetDef {
            group: 2,
            member: 3,
            cells: &[(0, 29, 2, 2)],
            program: yard_2000,
            wants: &[],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.add_deposit(0x190, (27, 22, 1, 1), data, caches);
        scene.add_deposit(0x190, (30, 24, 1, 1), data, caches);
        scene.add_deposit(0x190, (33, 26, 1, 1), data, caches);
        scene.add_deposit(0x191, (42, 16, 2, 2), data, caches);
        // Raid waits (0x5aef68, 0x5aef8c), the barbarian yard's rest
        // (0x5aef98), and when raiders fall back (fewer than three).
        const RAID_WAIT: [u32; 3] = [7, 5, 3];
        const YARD_REST: [u32; 3] = [1500, 1000, 750];
        let (a, b) = (scene.rng.next() % 3, scene.rng.next() % 3);
        if let Some(br) = scene.brain.as_mut() {
            br.fleets[4].vars = [(a + RAID_WAIT[difficulty()]) * 1500, 2, 0, 0];
            br.fleets[6].vars[0] = YARD_REST[difficulty()];
            br.fleets[9].vars = [7500, 3, 0, 0];
            br.fleets[12].vars = [(b + RAID_WAIT[difficulty()]) * 1500, 2, 0, 0];
        }
    }

    fn yard_750(state: u32) -> Op {
        match state {
            // The full-town order before every ship (0x47b130 code 1).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_complete(k) {
                    1
                } else {
                    scene.want_full_town(k);
                    0
                }
            }),
            1 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 750,
            },
        }
    }

    fn yard_2000(state: u32) -> Op {
        match state {
            // Order 240 before every ship (0x47b130 code 0xf0).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_fortified(k) {
                    1
                } else {
                    scene.want_town_order(k, true);
                    0
                }
            }),
            1 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 2000,
            },
        }
    }

    fn yard_var(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::WaitVar { next: 0, var: 0 },
        }
    }

    fn north_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (0, 0, 14, 16),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (1, 18),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 3000,
            },
        }
    }

    fn north_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (0, 0, 14, 30),
                resource: Resource::Fish,
                ticks: 6000,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (1, 18),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 1500,
            },
        }
    }

    /// Two posts by Pylos (0x451890, 0x451a60).
    fn north_post_a(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (0, 0, 12, 7),
            },
            Event::Zone {
                state: 20,
                area: (5, 7, 7, 8),
            },
            Event::Zone {
                state: 20,
                area: (0, 11, 5, 8),
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 7,
                failed: 7,
                to: (5, 3),
                ticks: 0,
                events: W,
            },
            7 => Op::Wait {
                next: 0,
                ticks: 1500,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
        }
    }

    fn north_post_b(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (5, 7, 7, 8),
            },
            Event::Zone {
                state: 20,
                area: (0, 0, 12, 7),
            },
            Event::Zone {
                state: 20,
                area: (0, 11, 5, 8),
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 7,
                failed: 7,
                to: (7, 10),
                ticks: 0,
                events: W,
            },
            7 => Op::Wait {
                next: 0,
                ticks: 1500,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
        }
    }

    /// Waits off Pylos, then sweeps east and attacks what it finds there,
    /// coming back when down to two ships (0x451c60).
    fn north_raiders(state: u32) -> Op {
        const HOME: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (0, 2, 9, 12),
            },
            Event::Zone {
                state: 20,
                area: (8, 7, 6, 8),
            },
        ];
        const SWEEP: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (0, 2, 9, 12),
            },
            Event::Zone {
                state: 50,
                area: (14, 0, 5, 15),
            },
            Event::Zone {
                state: 50,
                area: (19, 0, 7, 13),
            },
            Event::Zone {
                state: 50,
                area: (19, 13, 9, 9),
            },
        ];
        const FIGHT: &[Event] = &[
            HOME[0],
            HOME[1],
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: 1,
            },
        ];
        const ROUND: &[(i32, i32, u32)] = &[(8, 9, 500), (24, 18, 750), (8, 9, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 7,
                failed: 7,
                to: (8, 9),
                ticks: 0,
                events: HOME,
            },
            7 => Op::WaitVar { next: 40, var: 0 },
            40 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 4500,
                events: SWEEP,
            },
            50 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: FIGHT,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: HOME,
            },
        }
    }

    const BARBARIAN_HOME: Event = Event::Zone {
        state: 20,
        area: (28, 28, 9, 8),
    };
    const BARBARIAN_COAST: &[Event] = &[
        BARBARIAN_HOME,
        Event::Zone {
            state: 20,
            area: (34, 28, 10, 8),
        },
    ];

    fn barbarian_gatherers(state: u32) -> Op {
        const W: &[Event] = &[BARBARIAN_HOME];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
                area: (20, 26, 9, 7),
                resource: Resource::Fish,
                ticks: 3000,
                events: W,
            },
            7 => Op::Move {
                ok: 9,
                failed: 9,
                to: (23, 26),
                ticks: 750,
                events: W,
            },
            9 => Op::Wait {
                next: 11,
                ticks: 4500,
            },
            11 => Op::Gather {
                next: 13,
                area: (28, 27, 5, 6),
                resource: Resource::Amber,
                ticks: 4500,
                events: W,
            },
            13 => Op::Move {
                ok: 15,
                failed: 15,
                to: (23, 26),
                ticks: 750,
                events: W,
            },
            15 => Op::Wait {
                next: 0,
                ticks: 1500,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn barbarian_guard(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 3 },
            3 => Op::Move {
                ok: 5,
                failed: 5,
                to: (31, 30),
                ticks: 1000,
                events: BARBARIAN_COAST,
            },
            5 => Op::Wait {
                next: 0,
                ticks: 500,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    /// Patrols the barbarians' coast; answers fire; when three strong and
    /// it has been a while, sweeps north (0x4536c0).
    fn barbarian_raiders(state: u32) -> Op {
        const HOME_ROUND: &[(i32, i32, u32)] = &[(36, 30, 750), (40, 30, 1000)];
        const NORTH_ROUND: &[(i32, i32, u32)] = &[(32, 22, 750), (38, 22, 1000), (36, 30, 750)];
        const W: &[Event] = &[
            BARBARIAN_COAST[0],
            BARBARIAN_COAST[1],
            Event::Size {
                state: 40,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        const NORTH: &[Event] = &[
            BARBARIAN_COAST[0],
            BARBARIAN_COAST[1],
            Event::Zone {
                state: 20,
                area: (31, 18, 17, 10),
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: HOME_ROUND,
                ticks: 0,
                events: W,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 250,
                events: &[],
            },
            40 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: NORTH_ROUND,
                ticks: 4500,
                events: NORTH,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 3000,
                events: BARBARIAN_COAST,
            },
        }
    }

    fn south_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (0, 30, 11, 6),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (1, 34),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 3000,
            },
        }
    }

    fn south_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (0, 20, 14, 16),
                resource: Resource::Fish,
                ticks: 6000,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (1, 34),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 1500,
            },
        }
    }

    /// Waits off the south-west, then sweeps south-east (0x4528a0).
    fn south_raiders(state: u32) -> Op {
        const HOME: &[Event] = &[Event::Zone {
            state: 20,
            area: (0, 22, 11, 14),
        }];
        const SWEEP: &[Event] = &[
            HOME[0],
            Event::Zone {
                state: 30,
                area: (13, 23, 13, 12),
            },
        ];
        const FIGHT: &[Event] = &[
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: 1,
            },
            HOME[0],
            Event::Zone {
                state: 30,
                area: (13, 23, 13, 12),
            },
        ];
        const ROUND: &[(i32, i32, u32)] = &[(17, 28, 500), (21, 24, 750), (7, 24, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 7,
                failed: 7,
                to: (7, 24),
                ticks: 0,
                events: HOME,
            },
            7 => Op::WaitVar { next: 40, var: 0 },
            40 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 4500,
                events: SWEEP,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: FIGHT,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: HOME,
            },
        }
    }
}

/// miss005's brain (0x457950): the barbarians in the south-west and the
/// Persians in the north-west, each gathering, guarding and raiding.
mod miss005 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16; 0x454f60-0x455620).
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(3, 29, 1, 1)],
            program: yard_var,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[],
            program: barbarian_guard,
            wants: &[(ROYAL, [1, 2, 1]), (BOMBER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[],
            program: barbarian_gatherers,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[],
            program: barbarian_raiders,
            wants: &[(ROYAL, [2, 2, 3]), (RAMMER, [1, 2, 2])],
        },
        // The Persians (player 15; 0x455b80-0x456e80).
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(8, 11, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[],
            program: persian_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[],
            program: persian_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[],
            program: persian_west,
            wants: &[(ROYAL, [2, 2, 3])],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[],
            program: persian_east,
            wants: &[(ROYAL, [1, 3, 3])],
        },
        // (1, 5) mends (0x4564c0, not ported): the raiders are (1, 6) and
        // (1, 7), run at 35 and 40 (0x457950).
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: persian_raiders_north,
            wants: &[(BOARDER, [2; 3]), (RAMMER, [1, 2, 2]), (ROYAL, [1, 2, 3])],
        },
        FleetDef {
            group: 1,
            member: 7,
            cells: &[],
            program: persian_raiders_west,
            wants: &[(RAMMER, [2; 3]), (ROYAL, [1, 1, 2])],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.add_deposit(0x190, (18, 27, 1, 1), data, caches);
        scene.add_deposit(0x190, (13, 34, 1, 1), data, caches);
        scene.add_deposit(0x190, (35, 26, 1, 1), data, caches);
        scene.add_deposit(0x191, (18, 33, 2, 2), data, caches);
        // The barbarian yard's rest (0x5af8e8) and the raids' waits
        // (0x5af924, 0x5af96c, 0x5af990), in 1500-tick units plus 0-3.
        const YARD_REST: [u32; 3] = [1500, 1000, 750];
        const BARBARIAN_WAIT: [u32; 3] = [7, 6, 4];
        const NORTH_WAIT: [u32; 3] = [8, 7, 5];
        const WEST_WAIT: [u32; 3] = [7, 6, 4];
        let r: Vec<u32> = (0..3).map(|_| scene.rng.next() % 4).collect();
        if let Some(br) = scene.brain.as_mut() {
            br.fleets[0].vars[0] = YARD_REST[difficulty()];
            br.fleets[3].vars = [(r[0] + BARBARIAN_WAIT[difficulty()]) * 1500, 3, 0, 0];
            br.fleets[9].vars = [(r[1] + NORTH_WAIT[difficulty()]) * 1500, 5, 0, 2];
            br.fleets[10].vars = [(r[2] + WEST_WAIT[difficulty()]) * 1500, 3, 0, 1];
        }
    }

    fn yard_var(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::WaitVar { next: 0, var: 0 },
        }
    }

    fn persian_yard(state: u32) -> Op {
        match state {
            // The full-town order before every ship (0x47b130 code 1).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_complete(k) {
                    1
                } else {
                    scene.want_full_town(k);
                    0
                }
            }),
            1 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 1250,
            },
        }
    }

    const BARBARIAN_HOME: Event = Event::Zone {
        state: 20,
        area: (3, 24, 7, 10),
    };

    fn barbarian_guard(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 7,
                failed: 7,
                to: (7, 29),
                ticks: 0,
                events: &[BARBARIAN_HOME],
            },
            7 => Op::Wait {
                next: 0,
                ticks: 7500,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn barbarian_gatherers(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
                area: (7, 21, 8, 8),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[],
            },
            7 => Op::Move {
                ok: 9,
                failed: 9,
                to: (5, 23),
                ticks: 750,
                events: &[],
            },
            9 => Op::Wait {
                next: 11,
                ticks: 3000,
            },
            11 => Op::Gather {
                next: 13,
                area: (0, 21, 9, 8),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            13 => Op::Move {
                ok: 15,
                failed: 15,
                to: (5, 23),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 3000,
            },
        }
    }

    /// Patrols the barbarians' waters; answers fire; when three strong and
    /// it has waited long enough, raids the player (0x455620).
    fn barbarian_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(7, 29, 500), (10, 23, 750), (13, 25, 500), (10, 29, 875)];
        const W: &[Event] = &[
            BARBARIAN_HOME,
            Event::Zone {
                state: 20,
                area: (7, 20, 8, 9),
            },
            Event::Zone {
                state: 20,
                area: (7, 29, 6, 7),
            },
            Event::Size {
                state: 40,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn persian_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (2, 6, 8, 12),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (1, 9),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 3000,
            },
        }
    }

    fn persian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (1, 7, 14, 7),
                resource: Resource::Fish,
                ticks: 6000,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (1, 8),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 1500,
            },
        }
    }

    const PERSIAN_HOME: &[Event] = &[
        Event::Zone {
            state: 20,
            area: (2, 7, 14, 12),
        },
        Event::Zone {
            state: 30,
            area: (3, 1, 6, 3),
        },
        Event::Attacked {
            state: 40,
            after: 375,
        },
    ];

    /// Two guards patrol the Persians' coast (0x456160, 0x456310).
    fn persian_west(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(3, 14, 500), (13, 14, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 4500,
                events: PERSIAN_HOME,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((0, 0, 8, 9)),
                ticks: 1500,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: PERSIAN_HOME,
            },
        }
    }

    fn persian_east(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(7, 7, 500), (16, 13, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 4500,
                events: PERSIAN_HOME,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((0, 0, 8, 9)),
                ticks: 1500,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: PERSIAN_HOME,
            },
        }
    }

    /// Raider fleets: north 9, west 10. Their variables: 0 the wait before
    /// a raid, 1 the size it raids at, 2 the spell it casts on its ships
    /// as it attacks (0 none), 3 the size below which a raid is called off
    /// ("fewer than" + 1). The barbarian yard's (fleet 0) 2 and 3: north
    /// and west have landed.
    const NORTH: usize = 9;
    const WEST: usize = 10;
    const SPELL: usize = 2;
    const LANDED: [usize; 2] = [2, 3];

    /// Casts raider `k`'s spell on its ships through the Persians' war
    /// temple (0x47acf0 by member (1, 0), unpaid) and, unless `keep`,
    /// clears it.
    fn cast_spell(scene: &mut Scene, k: usize, clear: usize) {
        use crate::magic::Spell;
        let kind = scene.brain.as_ref().unwrap().fleets[k].vars[SPELL];
        let spell = match kind {
            9 => Some(Spell::Flame),
            12 => Some(Spell::Phantom),
            15 => Some(Spell::ProtectWeapons),
            _ => None,
        };
        if let Some(spell) = spell {
            let ships = scene.fleet_ships(k);
            scene.brain_cast_fleet(ships, spell, false);
        }
        scene.brain.as_mut().unwrap().fleets[clear].vars[SPELL] = 0;
    }

    fn landed(scene: &Scene, west: bool) -> bool {
        scene.brain.as_ref().unwrap().fleets[0].vars[LANDED[west as usize]] != 0
    }

    /// Rolls the raid's wait and spell (states 0 and 50).
    fn roll(scene: &mut Scene, k: usize, second: bool) {
        let d = difficulty();
        let base = if k == NORTH {
            [8, 7, 5][d]
        } else {
            [7, 6, 4][d]
        };
        let wait = (base + (scene.rng.next() & 3)) * 1500;
        let r = scene.rng.next();
        let spell = match (k == NORTH, second) {
            (true, false) => [9, 15, 12, 9][(r & 3) as usize],
            (false, false) => [9, 12, 9, 15][(r & 3) as usize],
            (true, true) => [9, 0, 12, 9, 15][(r % 5) as usize],
            (false, true) => [9, 12, 9, 0, 15][(r % 5) as usize],
        };
        let f = &mut scene.brain.as_mut().unwrap().fleets[k];
        f.vars[0] = wait;
        f.vars[SPELL] = spell;
    }

    /// A raid on one of the player's settlements or towns in the west
    /// (0x421490, its area grown by 2), else on (27, 28, 9, 8).
    fn raid_target(scene: &mut Scene, k: usize, found: u32, none: u32) -> u32 {
        match scene.human_site_area(Some((0, 0, 35, 35))) {
            Some(a) => {
                scene.set_fleet_area(k, a);
                found
            }
            None => none,
        }
    }

    const FORT: u32 = 0x35;
    const FAR: Cells = (27, 28, 9, 8);

    /// `Brain::gone`: the harpy nests (`+0x184`), the stronghold (`+0x18c`).
    const NESTS: usize = 0;
    const STRONGHOLD: usize = 1;

    /// The brain's pass, after the members (0x457950, every 5 ticks):
    /// `+0x184` is cleared once the cells of both harpy nests, (23, 20) and
    /// (26, 19), are empty, `+0x18c` once the stronghold's (3, 29) is.
    pub fn after(scene: &mut Scene) {
        let mut gone = scene.brain.as_ref().unwrap().gone;
        if !gone[NESTS] && !scene.standing_at((23, 20)) && !scene.standing_at((26, 19)) {
            gone[NESTS] = true;
        }
        if !gone[STRONGHOLD] && !scene.standing_at((3, 29)) {
            gone[STRONGHOLD] = true;
        }
        scene.brain.as_mut().unwrap().gone = gone;
    }

    /// Each turn (`turn`): west, in its raids on settlements (85, 90 and
    /// their attacks), casts its spell again and clears north's instead
    /// (the original's slip at 0x456e80).
    pub fn turn(scene: &mut Scene, k: usize) {
        let state = scene.brain.as_ref().unwrap().fleets[k].state;
        if k == WEST && matches!(state, 86 | 91) {
            cast_spell(scene, WEST, NORTH);
        }
        // Landing: the order waits for its price, its ship sailing there
        // (0x4cbcf0, as `found_by`).
        if matches!(k, NORTH | WEST) && state == 46 {
            let at = if k == WEST { (8, 20) } else { (19, 16) };
            let founding = scene.fleet_ships(k).iter().any(|&id| {
                scene.object_index(id).is_some_and(|i| {
                    matches!(
                        scene.objects[i].ship.as_ref().and_then(|s| s.task),
                        Some(crate::objects::Task::Found { .. })
                    )
                })
            });
            if !founding {
                if let Some(&ship) = scene.fleet_ships(k).first() {
                    scene.found_by(ship, at, FORT);
                }
            }
        }
    }

    /// The northern raiders (0x456580): wait their time on patrol, then
    /// once both harpy nests (23, 20) and (26, 19) are gone land (a medium
    /// fort at (19, 16)) unless the player holds (6, 18, 7, 7); landed,
    /// and the barbarians' stronghold (3, 29) gone, raid the player's
    /// settlements; from the fort, patrol and raid, back to the start if
    /// it falls.
    fn persian_raiders_north(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(15, 7, 500), (13, 0, 750), (19, 0, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (2, 7, 14, 12),
            },
            Event::Zone {
                state: 20,
                area: (16, 0, 12, 8),
            },
            Event::Zone {
                state: 20,
                area: (22, 4, 7, 8),
            },
            Event::Size {
                state: 25,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
        ];
        const Z: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (2, 7, 14, 12),
            },
            Event::Zone {
                state: 20,
                area: (16, 0, 12, 8),
            },
            Event::Zone {
                state: 20,
                area: (22, 4, 7, 8),
            },
        ];
        const FALLEN: Event = Event::Cell {
            state: 0,
            cell: (19, 16),
            class: FORT,
            holds: false,
        };
        const ZONES2: [Event; 4] = [
            Event::Zone {
                state: 70,
                area: (18, 14, 5, 8),
            },
            Event::Zone {
                state: 70,
                area: (2, 7, 14, 12),
            },
            Event::Zone {
                state: 70,
                area: (22, 4, 7, 8),
            },
            Event::Zone {
                state: 70,
                area: (2, 7, 14, 12),
            },
        ];
        const W2: &[Event] = &[
            FALLEN,
            ZONES2[0],
            ZONES2[1],
            ZONES2[2],
            ZONES2[3],
            Event::Size {
                state: 80,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
        ];
        const RAID: &[Event] = &[
            FALLEN,
            Event::Size {
                state: 50,
                after: None,
                cmp: Cmp::AtMost,
                n: 3,
            },
            ZONES2[0],
            ZONES2[1],
            ZONES2[2],
            ZONES2[3],
        ];
        raider(state, NORTH, ROUND, W, Z, W2, RAID, (19, 16), (6, 18, 7, 7))
    }

    /// The western raiders (0x456e80): as the northern, landing at (8, 20)
    /// unless the player holds (17, 12, 6, 8), and only once the
    /// barbarians' stronghold is gone.
    fn persian_raiders_west(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(1, 10, 500), (1, 14, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (2, 7, 14, 12),
            },
            Event::Zone {
                state: 20,
                area: (16, 0, 12, 8),
            },
            Event::Size {
                state: 25,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
        ];
        const Z: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (2, 7, 14, 12),
            },
            Event::Zone {
                state: 20,
                area: (16, 0, 12, 8),
            },
        ];
        const FALLEN: Event = Event::Cell {
            state: 0,
            cell: (8, 20),
            class: FORT,
            holds: false,
        };
        const W2: &[Event] = &[
            FALLEN,
            Event::Zone {
                state: 70,
                area: (6, 18, 7, 8),
            },
            Event::Zone {
                state: 70,
                area: (2, 7, 14, 12),
            },
            Event::Size {
                state: 80,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
        ];
        const RAID: &[Event] = &[
            FALLEN,
            Event::Size {
                state: 50,
                after: None,
                cmp: Cmp::AtMost,
                n: 3,
            },
            Event::Zone {
                state: 70,
                area: (6, 18, 7, 8),
            },
            Event::Zone {
                state: 70,
                area: (2, 7, 14, 12),
            },
        ];
        raider(state, WEST, ROUND, W, Z, W2, RAID, (8, 20), (17, 12, 6, 8))
    }

    /// The raiders' common program.
    #[allow(clippy::too_many_arguments)]
    fn raider(
        state: u32,
        k: usize,
        round: &'static [(i32, i32, u32)],
        watch: &'static [Event],
        zones: &'static [Event],
        watch2: &'static [Event],
        raid: &'static [Event],
        fort: (i32, i32),
        held: Cells,
    ) -> Op {
        let round2: &'static [(i32, i32, u32)] = if k == NORTH {
            &[(19, 14, 500), (23, 14, 750)]
        } else {
            &[(9, 18, 500), (11, 21, 750)]
        };
        match state {
            0 => Op::Custom(|scene, k| {
                roll(scene, k, false);
                1
            }),
            1 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: round,
                ticks: 0,
                events: watch,
            },
            // A fight where the player was seen: north casts first.
            20 if k == NORTH => Op::Custom(|scene, k| {
                cast_spell(scene, k, k);
                21
            }),
            20 | 21 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            25 => Op::Custom(|scene, k| {
                let west = k == WEST;
                let gone = scene.brain.as_ref().unwrap().gone;
                let stronghold = !gone[STRONGHOLD];
                let landed = landed(scene, west);
                let (fort_held, may_land) = if west {
                    ((17, 12, 6, 8), !stronghold)
                } else {
                    ((6, 18, 7, 7), gone[NESTS])
                };
                if west && stronghold {
                    return 0;
                }
                if may_land && !landed {
                    return if scene.human_in(fort_held) { 30 } else { 45 };
                }
                if stronghold || !landed {
                    return 0;
                }
                raid_target(scene, k, 37, 39)
            }),
            30 => Op::Custom(|scene, k| {
                cast_spell(scene, k, k);
                31
            }),
            31 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area(held),
                ticks: 3000,
                events: zones,
            },
            37 => Op::Custom(|scene, k| {
                cast_spell(scene, k, k);
                38
            }),
            38 => Op::Attack {
                next: 50,
                failed: 50,
                target: Target::Seen,
                ticks: 6000,
                events: zones,
            },
            39 => Op::Custom(|scene, k| {
                cast_spell(scene, k, k);
                40
            }),
            40 => Op::Attack {
                next: 50,
                failed: 50,
                target: Target::Area(FAR),
                ticks: 6000,
                events: zones,
            },
            // The landing: a medium fort (0x47b000, order 0x8c, its 1 x 1
            // area written into `+0x40` by 0x479af0).
            45 => Op::Custom(|scene, k| {
                let west = k == WEST;
                let at = if west { (8, 20) } else { (19, 16) };
                scene.brain.as_mut().unwrap().fleets[k].last_area = (at.0, at.1, 1, 1);
                if let Some(&ship) = scene.fleet_ships(k).first() {
                    scene.found_by(ship, at, FORT);
                }
                scene.brain.as_mut().unwrap().fleets[0].vars[LANDED[west as usize]] = 1;
                46
            }),
            46 => Op::Watch {
                next: 50,
                ticks: 3000,
                events: if k == NORTH {
                    &[Event::Cell {
                        state: 50,
                        cell: (19, 16),
                        class: FORT,
                        holds: true,
                    }]
                } else {
                    &[Event::Cell {
                        state: 50,
                        cell: (8, 20),
                        class: FORT,
                        holds: true,
                    }]
                },
            },
            50 => Op::Custom(|scene, k| {
                roll(scene, k, true);
                51
            }),
            51 => Op::Behave { next: 60 },
            60 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: round2,
                ticks: 0,
                events: watch2,
            },
            // North casts as it fights about its fort (0x456b96).
            70 if k == NORTH => Op::Custom(|scene, k| {
                cast_spell(scene, k, k);
                71
            }),
            70 | 71 => Op::Attack {
                next: 50,
                failed: 50,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            80 => Op::Custom(|scene, k| raid_target(scene, k, 85, 90)),
            85 => Op::Custom(|scene, k| {
                // West casts its own but clears north's (the slip).
                let clear = if k == WEST { NORTH } else { k };
                cast_spell(scene, k, clear);
                86
            }),
            86 => Op::Attack {
                next: 50,
                failed: 50,
                target: Target::Seen,
                ticks: 6000,
                events: raid,
            },
            90 => Op::Custom(|scene, k| {
                let clear = if k == WEST { NORTH } else { k };
                cast_spell(scene, k, clear);
                91
            }),
            91 => Op::Attack {
                next: 50,
                failed: 50,
                target: Target::Area(FAR),
                ticks: 6000,
                events: raid,
            },
            _ => {
                let _ = fort;
                Op::Behave { next: 0 }
            }
        }
    }
}

/// miss006's brain (0x45b280): barbarians in the south-west, and two Persian
/// players, one in the south and one in the north, each gathering,
/// guarding and raiding.
/// (Not ported: the brain's pass after the members, every 5 ticks
/// (0x45b863-0x45b993), rerolls the times of the event records 0x5b06cc
/// and 0x5b0c5c while the yards at (21, 68) and (1, 50) stand; which
/// members read those records was not traced: the port rolls the raids'
/// waits only at their state 0.)
mod miss006 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;

    /// Member (1, 7), the waterspouts' watch.
    const WATCHER: usize = 11;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16; 0x458990-0x458f20).
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(1, 50, 1, 1)],
            program: yard_1000,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[],
            program: barbarian_guard,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[],
            program: barbarian_gatherers,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[],
            program: barbarian_raiders,
            wants: &[(BOMBER, [1; 3]), (ROYAL, [3, 3, 4]), (RAMMER, [2, 3, 3])],
        },
        // The southern Persians (player 15; 0x459400-0x459b90).
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(21, 68, 2, 2)],
            program: yard_500,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[],
            program: south_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[],
            program: south_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[],
            program: south_guard,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [2, 2, 1])],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[],
            program: south_raiders,
            wants: &[(BOARDER, [2, 2, 3]), (ROYAL, [2; 3])],
        },
        // Mends what it is handed (0x459f60; the prizes: `brain_prize`).
        FleetDef {
            group: 1,
            member: 5,
            cells: &[],
            program: repair,
            wants: &[],
        },
        // Members (1, 6), which never runs (no case in 0x45b280), and
        // (1, 7), the waterspouts' watch (0x459fe0, run as `turn`): no
        // ships.
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 7,
            cells: &[],
            program: idle,
            wants: &[],
        },
        // The northern Persians (player 14; 0x45a270-0x45af50).
        FleetDef {
            group: 2,
            member: 0,
            cells: &[(14, 17, 2, 2)],
            program: yard_750,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 1,
            cells: &[],
            program: north_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            member: 2,
            cells: &[],
            program: north_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            member: 3,
            cells: &[],
            program: north_guard,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [2, 3, 2])],
        },
        FleetDef {
            group: 2,
            member: 4,
            cells: &[],
            program: north_raiders,
            wants: &[(BOARDER, [2, 2, 3]), (ROYAL, [2, 3, 2]), (RAMMER, [2; 3])],
        },
        // Mends what it is handed (0x477190; the prizes: `brain_prize`).
        FleetDef {
            group: 2,
            member: 5,
            cells: &[],
            program: repair,
            wants: &[],
        },
        // Member (2, 6) (0x45ae70).
        FleetDef {
            group: 2,
            member: 6,
            cells: &[(49, 15, 1, 1), (50, 14, 1, 1), (51, 16, 1, 1)],
            program: squadron_patrol,
            wants: &[],
        },
        // Member (2, 7) (0x45af50).
        FleetDef {
            group: 2,
            member: 7,
            cells: &[
                (51, 17, 1, 1),
                (49, 18, 1, 1),
                (52, 18, 1, 1),
                (50, 19, 1, 1),
            ],
            program: squadron_watch,
            wants: &[],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.add_deposit(0x190, (48, 68, 1, 1), data, caches);
        scene.add_deposit(0x190, (47, 67, 1, 1), data, caches);
        scene.add_deposit(0x190, (48, 57, 1, 1), data, caches);
        scene.add_deposit(0x191, (45, 65, 2, 2), data, caches);
        // The Persians' levels and spells (the main's 0x8d1).
        scene.persian_bonus(&[15, 14], true, &[]);
        // The barbarians' raid: after 10500 ticks, once 6 strong; the
        // Persians' raids roll their waits at their state 0, striking 4
        // (south) and 6 (north) strong, giving up below 3.
        if let Some(br) = scene.brain.as_mut() {
            br.fleets[3].vars = [10500, 6, 0, 0];
            br.fleets[SOUTH_RAIDERS].vars = [0, 4, 0, 2];
            br.fleets[NORTH_RAIDERS].vars = [0, 6, 0, 0];
        }
    }

    fn yard_500(state: u32) -> Op {
        match state {
            // The full-town order before every ship (0x47b130 code 1).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_complete(k) {
                    1
                } else {
                    scene.want_full_town(k);
                    0
                }
            }),
            1 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 500,
            },
        }
    }

    fn yard_750(state: u32) -> Op {
        match state {
            // The full-town order before every ship (0x47b130 code 1).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_complete(k) {
                    1
                } else {
                    scene.want_full_town(k);
                    0
                }
            }),
            1 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 750,
            },
        }
    }

    fn yard_1000(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 1000,
            },
        }
    }

    const BARBARIAN_HOME: Event = Event::Zone {
        state: 20,
        area: (0, 45, 11, 10),
    };

    fn barbarian_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(6, 48, 750), (4, 55, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: PREVIOUS,
                failed: PREVIOUS,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: &[BARBARIAN_HOME],
            },
        }
    }

    fn barbarian_gatherers(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
                area: (0, 44, 6, 14),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[],
            },
            7 => Op::Move {
                ok: 9,
                failed: 9,
                to: (0, 55),
                ticks: 750,
                events: &[],
            },
            9 => Op::Wait {
                next: 11,
                ticks: 3000,
            },
            11 => Op::Gather {
                next: 13,
                area: (0, 44, 6, 14),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            13 => Op::Move {
                ok: 15,
                failed: 15,
                to: (0, 55),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 3000,
            },
        }
    }

    /// Patrols the southern sea; when six strong, raids the player (0x458f20).
    fn barbarian_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (22, 44, 500),
            (25, 50, 750),
            (36, 47, 500),
            (53, 46, 875),
            (36, 47, 500),
            (25, 50, 750),
        ];
        const W: &[Event] = &[
            BARBARIAN_HOME,
            Event::Zone {
                state: 30,
                area: (10, 41, 16, 12),
            },
            Event::Zone {
                state: 30,
                area: (27, 44, 17, 8),
            },
            Event::Zone {
                state: 30,
                area: (44, 42, 12, 12),
            },
            Event::Size {
                state: 40,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn south_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (19, 60, 14, 10),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (21, 60),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 3000,
            },
        }
    }

    fn south_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (19, 60, 14, 10),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (21, 60),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 3000,
            },
        }
    }

    fn south_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(18, 66, 500), (24, 64, 750), (28, 66, 750), (24, 64, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (16, 61, 15, 9),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 4500,
                events: W,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
        }
    }

    /// Fleet indices of the casting members.
    const SOUTH_RAIDERS: usize = 8;
    const NORTH_RAIDERS: usize = 16;
    const SPELL: usize = 2;
    const FAR: Cells = (36, 60, 17, 10);

    /// Casts fleet `k`'s spell on its ships through its group's war temple
    /// (0x47acf0 by member 0, unpaid) and clears it.
    fn cast(scene: &mut Scene, k: usize) {
        use crate::magic::Spell;
        let spell = match scene.brain.as_ref().unwrap().fleets[k].vars[SPELL] {
            9 => Some(Spell::Flame),
            12 => Some(Spell::Phantom),
            _ => None,
        };
        if let Some(spell) = spell {
            let ships = scene.fleet_ships(k);
            scene.brain_cast_fleet(ships, spell, false);
        }
        scene.brain.as_mut().unwrap().fleets[k].vars[SPELL] = 0;
    }

    fn set(scene: &mut Scene, k: usize, v: usize, value: u32) {
        scene.brain.as_mut().unwrap().fleets[k].vars[v] = value;
    }

    /// Member (1, 4) (0x459b90): rolls its wait ((8, 6, 4) + rand & 3,
    /// x 1500) and takes the Flame (0); short of its boarders and royal
    /// ships it waits at (17, 63) (5), else patrols (6); a raid on one of
    /// the player's settlements or towns (37), else on (36, 60, 17, 10)
    /// (39), casting first, 6000 ticks, called off below 3.
    fn south_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (22, 56, 500),
            (28, 55, 750),
            (36, 56, 750),
            (36, 62, 750),
            (26, 62, 750),
        ];
        const HOME: Event = Event::Zone {
            state: 20,
            area: (16, 62, 15, 8),
        };
        const W: &[Event] = &[
            HOME,
            Event::Zone {
                state: 20,
                area: (19, 52, 18, 11),
            },
            Event::Zone {
                state: 20,
                area: (33, 55, 8, 15),
            },
            Event::Size {
                state: 25,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
        ];
        const RAID: &[Event] = &[
            HOME,
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: 3,
            },
        ];
        match state {
            0 => Op::Custom(|scene, k| {
                let wait = ([8, 6, 4][difficulty()] + (scene.rng.next() & 3)) * 1500;
                set(scene, k, 0, wait);
                set(scene, k, SPELL, 9);
                1
            }),
            1 => Op::Behave { next: 5 },
            5 => Op::Custom(|scene, k| {
                let n = scene.fleet_types(k);
                let have = n[ROYAL as usize] + n[BOARDER as usize];
                if have < [4, 4, 5][difficulty()] {
                    2
                } else {
                    6
                }
            }),
            2 => Op::Move {
                ok: 0,
                failed: 0,
                to: (17, 63),
                ticks: 1500,
                events: &[HOME],
            },
            6 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            25 => Op::Custom(
                |scene, k| match scene.human_site_area(Some((0, 0, 56, 70))) {
                    Some(a) => {
                        scene.set_fleet_area(k, a);
                        37
                    }
                    None => 39,
                },
            ),
            37 => Op::Custom(|scene, k| {
                cast(scene, k);
                38
            }),
            38 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 6000,
                events: RAID,
            },
            39 => Op::Custom(|scene, k| {
                cast(scene, k);
                40
            }),
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area(FAR),
                ticks: 6000,
                events: RAID,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn north_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (19, 10, 12, 4),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (14, 24),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 3000,
            },
        }
    }

    fn north_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (10, 23, 11, 8),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (14, 24),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 3000,
            },
        }
    }

    const NORTH_HOME: Event = Event::Zone {
        state: 20,
        area: (8, 13, 14, 15),
    };

    /// Member (2, 3) (0x45a850): takes the Flame at each start, patrols,
    /// and fights the player in its home waters, casting first.
    fn north_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(20, 17, 500), (20, 23, 750), (15, 26, 750)];
        match state {
            0 => Op::Custom(|scene, k| {
                set(scene, k, SPELL, 9);
                1
            }),
            1 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 4500,
                events: &[NORTH_HOME],
            },
            20 => Op::Custom(|scene, k| {
                cast(scene, k);
                21
            }),
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[NORTH_HOME],
            },
        }
    }

    /// Member (2, 4) (0x45a9f0): rolls its wait (base (20, 15, 15) while
    /// the southern Persians' town (21, 68) or the barbarians' stronghold
    /// (1, 50) stands, else (8, 6, 4), + rand & 3, x 1500); short of
    /// ships it waits at (11, 24), else patrols; a raid rolls the phantom
    /// or the Flame and strikes one of the player's settlements or towns,
    /// else (36, 60, 17, 10), casting first, 4500 ticks.
    fn north_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (27, 17, 500),
            (33, 20, 750),
            (41, 25, 750),
            (34, 32, 750),
            (26, 26, 750),
            (34, 32, 750),
            (41, 25, 750),
            (33, 20, 750),
        ];
        const W: &[Event] = &[
            Event::Zone {
                state: 30,
                area: (8, 13, 14, 15),
            },
            Event::Zone {
                state: 30,
                area: (21, 9, 29, 20),
            },
            Event::Size {
                state: 25,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
            },
        ];
        match state {
            0 => Op::Custom(|scene, k| {
                let held = scene.standing_at((21, 68)) || scene.standing_at((1, 50));
                let base = if held { [20, 15, 15] } else { [8, 6, 4] }[difficulty()];
                let wait = (base + (scene.rng.next() & 3)) * 1500;
                set(scene, k, 0, wait);
                1
            }),
            1 => Op::Behave { next: 5 },
            5 => Op::Custom(|scene, k| {
                let n = scene.fleet_types(k);
                let have = n[ROYAL as usize] + n[BOARDER as usize] + n[RAMMER as usize];
                if have < 4 {
                    2
                } else {
                    6
                }
            }),
            2 => Op::Move {
                ok: 0,
                failed: 0,
                to: (11, 24),
                ticks: 1500,
                events: &[NORTH_HOME],
            },
            6 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            25 => Op::Custom(|scene, k| {
                let spell = if scene.rng.next() & 3 < 2 { 12 } else { 9 };
                set(scene, k, SPELL, spell);
                match scene.human_site_area(Some((0, 0, 56, 70))) {
                    Some(a) => {
                        scene.set_fleet_area(k, a);
                        37
                    }
                    None => 39,
                }
            }),
            37 => Op::Custom(|scene, k| {
                cast(scene, k);
                38
            }),
            38 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: &[NORTH_HOME],
            },
            39 => Op::Custom(|scene, k| {
                cast(scene, k);
                40
            }),
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area(FAR),
                ticks: 4500,
                events: &[NORTH_HOME],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    /// Member (1, 7)'s watch (0x459fe0, an empty fleet run as its `turn`,
    /// at 40): every 1500 ticks, the first time the player comes into
    /// one of four small zones, if more than so many of their ships are
    /// about, waterspouts on its cells through the sea temple (0x47ab40,
    /// kind 6, paid in full); two checks in all.
    pub fn turn(scene: &mut Scene, k: usize) {
        use crate::magic::Spell;
        if k != WATCHER {
            return;
        }
        /// The zone, the cells counted about it, the ships needed, the
        /// waterspouts' cells.
        type Watch = (Cells, Cells, usize, &'static [(i32, i32)]);
        const WATCH: [Watch; 4] = [
            ((21, 59, 3, 2), (19, 56, 6, 8), 3, &[(21, 61), (22, 62)]),
            ((26, 59, 3, 2), (24, 55, 7, 8), 3, &[(27, 60), (28, 61)]),
            ((32, 62, 2, 2), (30, 60, 8, 6), 2, &[(31, 62)]),
            ((31, 67, 2, 3), (30, 65, 7, 5), 3, &[(31, 67), (32, 68)]),
        ];
        let tick = scene.brain.as_ref().unwrap().tick;
        let w = scene.brain.as_ref().unwrap().watch;
        let (mut armed, mut checks, mut since) = (w[0] != 0, w[1], w[2]);
        if tick >= since + 1500 {
            since = tick;
            armed = true;
        }
        if armed && checks <= 1 {
            for (zone, about, needed, cells) in WATCH {
                if !scene.human_in(zone) {
                    continue;
                }
                armed = false;
                let ships = scene
                    .objects
                    .iter()
                    .filter(|o| {
                        scene.is_own(o) && o.ship.is_some() && {
                            let t = TILE_SIZE as f32;
                            let (x, y) = ((o.x / t) as i32, (o.y / t) as i32);
                            (about.0..about.0 + about.2).contains(&x)
                                && (about.1..about.1 + about.3).contains(&y)
                        }
                    })
                    .count();
                if ships > needed {
                    let owner = Some((3, 3));
                    let temple = scene.objects.iter().any(|o| {
                        o.owner == owner
                            && o.class == openconquest_formats::ObjectClass::Building(0x3d)
                    });
                    let (amber, fish) = Spell::Waterspout.cost();
                    for &(x, y) in cells {
                        let (a, f) = scene.funds((3, 3));
                        if temple && a >= amber && f >= fish {
                            scene.earn((3, 3), -(amber as i64), -(fish as i64));
                            scene.pending_casts.push((Spell::Waterspout, x, y, owner));
                        }
                    }
                }
                checks += 1;
                break;
            }
        }
        scene.brain.as_mut().unwrap().watch = [armed as u32, checks, since];
    }

    /// Member (2, 6) (0x45ae70): patrols (25, 32)-(46, 31), 4500 ticks a
    /// round, fighting the player seen in its three zones 1500 ticks.
    fn squadron_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(25, 32, 500), (37, 34, 750), (46, 31, 750), (37, 34, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (8, 13, 14, 15),
            },
            Event::Zone {
                state: 20,
                area: (21, 9, 29, 20),
            },
            Event::Zone {
                state: 20,
                area: (23, 30, 27, 8),
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 4500,
                events: W,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    /// Member (2, 7) (0x45af50): lies at its cells 3000 ticks at a time,
    /// fighting the player seen in (8, 13, 14, 15) or (48, 11, 7, 14).
    fn squadron_watch(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (8, 13, 14, 15),
            },
            Event::Zone {
                state: 20,
                area: (48, 11, 7, 14),
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Watch {
                next: 5,
                ticks: 3000,
                events: W,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    /// The prize members (1, 5) and (2, 5) (0x459f60, 0x477190): their
    /// ships to the docks to be mended (0x47a7e0).
    fn repair(_: u32) -> Op {
        Op::Custom(|scene, k| {
            scene.fleet_to_docks(k);
            0
        })
    }

    /// A member with no ships of its own.
    fn idle(_: u32) -> Op {
        Op::Custom(|_, _| 0)
    }
}

/// miss007's brain (0x45f770). The barbarians (player 16, group 0) have
/// six yards; yards 1-5 each build for one fleet (members 6-10), which asks
/// for a ship type with a wait to follow (0x45ca00); the fleets patrol
/// their waters and raid the player's forts and towns (0x45cbf0-0x45dbd0),
/// and member 11 guards the wind wizard's waters (0x45e120). The Persians
/// (player 15, group 1) have one yard serving their merchants, fishers,
/// two guards and raiders (0x45e510-0x45eff0), and a fleet mending prizes.
/// Member 11 also hunts Hercules's sword: it takes it where it lies
/// (0x47aed0, order 0xbe) and attacks about whoever of the player's holds
/// it.
mod miss007 {
    use super::*;

    const BOAT: u8 = 0;
    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const KILLER: u8 = 6;

    /// Fleet variables: 0 the size it wants before raiding, 1 the ticks of
    /// patrolling before that (7500), 2 the ship asked of its yard (type +
    /// 1, 0 none), 3 the size at which a raid is called off (2).
    const TARGET: usize = 0;
    const PATIENCE: usize = 1;
    const ASKED: usize = 2;
    const FEW: usize = 3;

    /// The yards, by fleet index; yard k builds for fleet k + 5.
    const FIRST_BARBARIAN_FLEET: usize = 6;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16): member 0's yard builds nothing
        // (0x406330 is empty); yards 1-5 (0x45ca00).
        FleetDef {
            group: 0,
            member: 0,
            cells: &[
                (36, 26, 1, 1),
                (34, 31, 1, 1),
                (27, 37, 1, 1),
                (22, 51, 1, 1),
                (38, 50, 1, 1),
                (7, 53, 1, 1),
                (29, 71, 1, 1),
            ],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(45, 30, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(6, 40, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[(11, 40, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 4,
            cells: &[(15, 68, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 5,
            cells: &[(51, 74, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 6,
            cells: &[],
            program: east,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 7,
            cells: &[],
            program: west,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 8,
            cells: &[],
            program: south,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 9,
            cells: &[],
            program: south_west,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 10,
            cells: &[],
            program: south_east,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 11,
            cells: &[(14, 21, 10, 11)],
            program: wizard_guard,
            wants: &[],
        },
        // The Persians (player 15; 0x45e510-0x45f370).
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(9, 1, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[],
            program: persian_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[],
            program: persian_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[],
            program: persian_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[],
            program: persian_south_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 5,
            cells: &[],
            program: persian_raiders,
            wants: &[],
        },
        // Mends the Persians' prizes (0x45f370; see `brain_prize`).
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: idle,
            wants: &[],
        },
    ];

    /// The start (message 0x8d1): a big reef at one of two places, the
    /// deposits by difficulty (0x48b180 amber, 0x48ebc0 fish: easy all,
    /// normal the last three, hard none), and the fleets' numbers.
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        // Started on its own: the human's levels (a carried game's replace them).
        scene.standalone_levels([0, 1], 1);
        // The yards' full-town order and the builders' repairs (0x47b130).
        scene.start_jobs(&[(1, 0)], &[(0, 0)]);
        // The Persians' studies at the start (the main's 0x8d1).
        scene.persian_bonus(&[15], false, &[]);
        let (x, y) = if scene.rng.next() & 1 == 0 {
            (41, 69)
        } else {
            (39, 70)
        };
        let kind = scene.rng.next() % 3 + 1;
        scene.big_reef(x, y, kind, data, caches);
        let level = difficulty();
        if level == EASY {
            for (x, y) in [(44, 10), (23, 22), (21, 21), (8, 56)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            for (x, y) in [(49, 9), (7, 60)] {
                scene.add_deposit(0x191, (x, y, 5, 5), data, caches);
            }
        }
        if level <= NORMAL {
            for (x, y) in [(19, 45), (15, 52)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            scene.add_deposit(0x191, (15, 46, 5, 5), data, caches);
        }
        // The barbarian fleets' sizes before raiding (brain + 0x108-0x10c).
        let d = level as u32;
        let rolls = [(2, 3), (1, 4), (2, 3), (2, 3), (3, 4)];
        for (n, (span, base)) in rolls.into_iter().enumerate() {
            let target = scene.rng.next() % (d + span) + base;
            let f = &mut scene.brain.as_mut().unwrap().fleets[FIRST_BARBARIAN_FLEET + n];
            f.vars = [target, 7500, 0, 2];
        }
        let f = &mut scene.brain.as_mut().unwrap().fleets[17];
        f.vars = [d + 3, 7500, 0, 2];
    }

    /// The brain's pass, before the members (0x45f770, every 5 ticks):
    /// once (`+0x107`), the big reef put up at (22, 17) when the player's
    /// ships sail into (14, 17, 8, 14) (0x479ed0, 0x430ec0).
    pub fn before(scene: &mut Scene) {
        let f = &scene.brain.as_ref().unwrap().fleets[0];
        if f.vars[0] == 0 && scene.human_ships_in((14, 17, 8, 14)) {
            scene.brain.as_mut().unwrap().fleets[0].vars[0] = 1;
            let kind = scene.rng.next() % 3 + 1;
            scene.pending_reefs.push((22, 17, kind));
        }
    }

    /// Each fleet's bookkeeping on its turn (the start of 0x45cbf0 and the
    /// others): what to ask the yard for.
    pub fn turn(scene: &mut Scene, k: usize) {
        // Member 11 (0x45e120): while the player holds Hercules's sword,
        // it attacks about the ship or building holding it.
        if k == 11 {
            let state = scene.brain.as_ref().unwrap().fleets[k].state;
            if state != 25 {
                if let Some(cell) = scene.human_holder_of(SWORD) {
                    scene.set_fleet_area(k, (cell.0 - 2, cell.1 - 2, 5, 5));
                    scene.brain.as_mut().unwrap().fleets[k].state = 25;
                    scene.brain.as_mut().unwrap().fleets[k].started = false;
                }
            }
            return;
        }
        // Member 0 does nothing (0x406330).
        if k == 0 {
            return;
        }
        let d = difficulty() as u32;
        let n = scene.fleet_types(k);
        // Barbarian fleets ask their yard for one ship at a time.
        let ask = |r: u32| -> Option<(usize, u8, (u32, u32))> {
            match k {
                6 => Some((
                    n[ROYAL as usize] + n[KILLER as usize],
                    if r % (d + 1) != 0 { ROYAL } else { KILLER },
                    (2, 3),
                )),
                7 => Some((
                    n[ROYAL as usize] + n[KILLER as usize],
                    if r % (d + 1) < 2 { KILLER } else { ROYAL },
                    (1, 4),
                )),
                8 => Some((
                    n[ROYAL as usize] + n[BOMBER as usize] + n[KILLER as usize],
                    if r % (d + 1) == 2 { BOMBER } else { ROYAL },
                    (2, 3),
                )),
                9 => Some((
                    n[ROYAL as usize] + n[BOMBER as usize],
                    if r % (d + 1) == 2 { BOMBER } else { ROYAL },
                    (2, 3),
                )),
                10 => Some((n[KILLER as usize], KILLER, (3, 4))),
                _ => None,
            }
        };
        let r = scene.rng.next();
        if let Some((have, kind, (span, base))) = ask(r) {
            let f = &mut scene.brain.as_mut().unwrap().fleets[k];
            if (have as u32) < f.vars[TARGET] && f.vars[ASKED] == 0 {
                f.vars[ASKED] = kind as u32 + 1;
            } else {
                f.vars[TARGET] = scene.rng.next() % (d + span) + base;
            }
            return;
        }
        // The Persians ask their one yard.
        let want = match k {
            13 if n[BOAT as usize] + n[MERCHANT as usize] < 2 => {
                Some(if d < 2 { BOAT } else { MERCHANT })
            }
            14 if n[MERCHANT as usize] < 1 => Some(MERCHANT),
            15 if (n[ROYAL as usize] as u32) < d + 2 => Some(ROYAL),
            16 | 17
                if ((n[ROYAL as usize] + n[BOMBER as usize] + n[BOARDER as usize]) as u32)
                    < d + 3 =>
            {
                Some(match scene.rng.next() % (d + 3) {
                    1 | 2 => ROYAL,
                    3 | 4 => BOMBER,
                    _ => BOARDER,
                })
            }
            _ => None,
        };
        if let Some(kind) = want {
            let brain = scene.brain.as_ref().unwrap();
            let asked =
                brain.requests.iter().any(|r| r.0 == k) || brain.building.iter().any(|b| b.2 == k);
            if !asked {
                scene.brain.as_mut().unwrap().requests.push((k, kind));
            }
        }
    }

    fn idle(_: u32) -> Op {
        Op::Wait {
            next: 0,
            ticks: 1500,
        }
    }

    /// A barbarian yard (0x45ca00): builds what its fleet (k + 5) asked
    /// for, waits (60 or 70 + rand 61) x 25 ticks, then takes the next.
    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, k| {
                if scene.brain.as_ref().unwrap().fleets[k + 5].vars[ASKED] != 0 {
                    10
                } else {
                    0
                }
            }),
            10 => Op::Custom(|scene, k| {
                let asked = scene.brain.as_ref().unwrap().fleets[k + 5].vars[ASKED];
                if asked == 0 {
                    return 0;
                }
                if !scene.fleet_build_for(k, k + 5, asked as u8 - 1) {
                    return 10;
                }
                let base = if k == 1 { 60 } else { 70 };
                let wait = (scene.rng.next() % 61 + base) * 25;
                scene.brain.as_mut().unwrap().fleets[k].vars[0] = wait;
                11
            }),
            11 => Op::Custom(|scene, k| if scene.fleet_building(k) { 11 } else { 12 }),
            12 => Op::WaitVar { next: 13, var: 0 },
            _ => Op::Custom(|scene, k| {
                scene.brain.as_mut().unwrap().fleets[k + 5].vars[ASKED] = 0;
                0
            }),
        }
    }

    /// The Persians' yard (0x45e510): builds the members' requests one at a
    /// time, resting (40 + rand 31) x 25 ticks after each.
    fn persian_yard(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            11 => Op::Custom(|scene, k| {
                let wait = (scene.rng.next() % 31 + 40) * 25;
                scene.brain.as_mut().unwrap().fleets[k].vars[0] = wait;
                12
            }),
            _ => Op::WaitVar { next: 0, var: 0 },
        }
    }

    const EAST_ROUND: &[(i32, i32, u32)] = &[(41, 23, 750), (33, 28, 1000), (42, 32, 750)];
    const WEST_ROUND: &[(i32, i32, u32)] = &[(1, 31, 1000), (8, 36, 1000)];
    const SOUTH_ROUND: &[(i32, i32, u32)] = &[
        (11, 46, 1000),
        (17, 50, 1000),
        (20, 39, 750),
        (21, 32, 1250),
    ];
    const SOUTH_WEST_ROUND: &[(i32, i32, u32)] =
        &[(11, 58, 1000), (2, 52, 1000), (4, 62, 750), (10, 69, 1250)];
    const SOUTH_EAST_ROUND: &[(i32, i32, u32)] = &[(52, 65, 1000), (49, 69, 1000)];

    /// The patrol's events (0x5b0fb0 and the like): the player in its
    /// waters, enough ships after 7500 ticks, being fired on.
    macro_rules! watch {
        ($zone:expr) => {
            &[
                Event::Zone {
                    state: 15,
                    area: $zone,
                },
                Event::Size {
                    state: 10,
                    after: Some(PATIENCE),
                    cmp: Cmp::AtLeast,
                    n: TARGET,
                },
                Event::Attacked {
                    state: 50,
                    after: 375,
                },
            ]
        };
    }
    macro_rules! raiding {
        ($zone:expr) => {
            &[
                Event::Zone {
                    state: 15,
                    area: $zone,
                },
                Event::Size {
                    state: 0,
                    after: None,
                    cmp: Cmp::AtMost,
                    n: FEW,
                },
                Event::Attacked {
                    state: 50,
                    after: 375,
                },
            ]
        };
    }

    /// A barbarian fleet's program (0x45cbf0, 0x45d0b0, 0x45d640,
    /// 0x45dbd0): patrol until it has its ships, then raid (state 10 picks
    /// where, `pick`), the raid called off at two ships left.
    fn barbarian(
        state: u32,
        round: &'static [(i32, i32, u32)],
        watch: &'static [Event],
        raiding: &'static [Event],
        raid_ticks: u32,
        pick: fn(&mut Scene, usize) -> u32,
    ) -> Op {
        match state {
            0 => Op::Custom(|scene, k| {
                scene.brain.as_mut().unwrap().fleets[k].vars[PATIENCE] = 7500;
                1
            }),
            1 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: round,
                ticks: 0,
                events: watch,
            },
            10 => Op::Custom(pick),
            15 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            20 | 25 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: raid_ticks,
                events: raiding,
            },
            // Nothing to raid: look again in (30 + rand 41) x 25 ticks.
            30 => Op::Custom(|scene, k| {
                let wait = (scene.rng.next() % 41 + 30) * 25;
                scene.brain.as_mut().unwrap().fleets[k].vars[PATIENCE] = wait;
                31
            }),
            31 => Op::WaitVar {
                next: 32,
                var: PATIENCE,
            },
            32 => Op::Custom(|scene, k| {
                scene.brain.as_mut().unwrap().fleets[k].vars[PATIENCE] = 7500;
                10
            }),
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    /// Raid the player's strong fort at (15, 26) (but one time in
    /// difficulty + 2), else one of their towns in `rect`.
    fn raid_target(scene: &mut Scene, k: usize, fort: bool, rect: Option<Cells>) -> bool {
        let d = difficulty() as u32;
        if fort && scene.human_class_at(0x36, (15, 26)) && scene.rng.next() % (d + 2) != 0 {
            scene.set_fleet_area(k, (13, 25, 5, 7));
            return true;
        }
        match scene.human_site_area(rect) {
            Some(a) => {
                scene.set_fleet_area(k, a);
                true
            }
            None => false,
        }
    }

    /// Else the first of `zones` the player is in, or wait and look again.
    fn raid_zone(scene: &mut Scene, k: usize, zones: &[Cells]) -> u32 {
        match scene.first_human_zone(zones) {
            Some(i) => {
                scene.set_fleet_area(k, zones[i]);
                25
            }
            None => 30,
        }
    }

    const CENTRE: Cells = (10, 10, 30, 30);

    fn east(state: u32) -> Op {
        barbarian(
            state,
            EAST_ROUND,
            watch!((33, 21, 15, 14)),
            raiding!((33, 21, 15, 14)),
            10500,
            |scene, k| {
                if raid_target(scene, k, true, None) {
                    20
                } else {
                    0
                }
            },
        )
    }

    fn west(state: u32) -> Op {
        barbarian(
            state,
            WEST_ROUND,
            watch!((0, 28, 9, 13)),
            raiding!((0, 28, 9, 13)),
            6000,
            |scene, k| {
                if raid_target(scene, k, true, Some(CENTRE)) {
                    20
                } else {
                    raid_zone(scene, k, &[(15, 15, 20, 20)])
                }
            },
        )
    }

    fn south(state: u32) -> Op {
        barbarian(
            state,
            SOUTH_ROUND,
            watch!((9, 34, 15, 15)),
            raiding!((9, 34, 15, 15)),
            6000,
            |scene, k| {
                if raid_target(scene, k, false, Some(CENTRE)) {
                    20
                } else {
                    raid_zone(scene, k, &[(46, 22, 9, 7), (26, 31, 20, 15)])
                }
            },
        )
    }

    fn south_west(state: u32) -> Op {
        barbarian(
            state,
            SOUTH_WEST_ROUND,
            watch!((0, 50, 15, 20)),
            raiding!((0, 50, 15, 20)),
            6000,
            |scene, k| {
                raid_zone(
                    scene,
                    k,
                    &[(0, 50, 15, 20), (0, 32, 8, 10), (20, 40, 20, 30)],
                )
            },
        )
    }

    fn south_east(state: u32) -> Op {
        barbarian(
            state,
            SOUTH_EAST_ROUND,
            watch!((41, 57, 12, 15)),
            raiding!((41, 57, 12, 15)),
            6000,
            |scene, k| raid_zone(scene, k, &[(41, 57, 12, 15), (20, 58, 18, 15)]),
        )
    }

    /// The ships about the wind wizard's island (0x45e120): they hold the
    /// waters before the fortress (13, 23, 5, 5), 1500 ticks at a time.
    fn wizard_guard(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Attack {
                next: 10,
                failed: 10,
                target: Target::Area((13, 23, 5, 5)),
                ticks: 1500,
                events: &[],
            },
            // While the player's fort stands at (15, 26) it holds the
            // waters; else it looks for the sword.
            10 => Op::Custom(|scene, _| {
                if scene.human_class_at(0x36, (15, 26)) {
                    5
                } else {
                    15
                }
            }),
            // 0x47aed0 (order 0xbe): a ship takes the sword (kind 22) where
            // it lies anywhere on the map (0x4d9920; the whole map written
            // into `+0x40` by 0x479c90); 5 if there is none.
            15 => Op::Custom(|scene, k| {
                let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
                scene.brain.as_mut().unwrap().fleets[k].last_area = (0, 0, w, h);
                match scene.loose_treasure(SWORD) {
                    Some(target) => {
                        for id in scene.fleet_ships(k) {
                            if scene.order_take(id, target).is_ok() {
                                return 16;
                            }
                        }
                        5
                    }
                    None => 5,
                }
            }),
            16 => Op::Custom(|scene, k| {
                let held = scene.fleet_ships(k).iter().any(|&id| {
                    scene
                        .object_index(id)
                        .and_then(|i| scene.objects[i].ship.as_ref())
                        .is_some_and(|s| s.has_treasure(SWORD))
                });
                if held {
                    20
                } else if scene.loose_treasure(SWORD).is_some() {
                    16
                } else {
                    5
                }
            }),
            // Taken: fleet 8 joins it (0x479370) and it holds the waters.
            20 => Op::Custom(|scene, k| {
                scene.fleet_merge(k, 8);
                5
            }),
            // The player holds the sword: around its holder, 6000 ticks.
            25 => Op::Attack {
                next: 15,
                failed: 15,
                target: Target::Seen,
                ticks: 6000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((13, 23, 5, 5)),
                ticks: 1500,
                events: &[],
            },
        }
    }

    /// Hercules's sword (treasure kind 22, the brain's +0x104 watch).
    const SWORD: u8 = 22;

    fn persian_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            // (3 + rand(difficulty + 1)) x 1500 ticks: its middle here.
            5 => Op::Gather {
                next: 15,
                area: (0, 0, 12, 15),
                resource: Resource::Amber,
                ticks: (6 + difficulty() as u32) * 750,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to: (5, 0),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 750,
            },
        }
    }

    fn persian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (0, 0, 12, 15),
                resource: Resource::Fish,
                ticks: 15000,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to: (3, 8),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 750,
            },
        }
    }

    const PERSIAN_ROUND: &[(i32, i32, u32)] = &[(21, 5, 500), (12, 10, 750)];
    const HOME: Cells = (0, 0, 15, 10);

    /// A Persian guard (0x45ec10, 0x45edd0): patrols 4500 ticks at a time;
    /// the player in its waters is fought 6000 ticks, at home 1500, and
    /// whoever fires on it 375.
    fn guard(state: u32, watch: &'static [Event]) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: PERSIAN_ROUND,
                ticks: 4500,
                events: watch,
            },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 6000,
                events: watch,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    fn persian_guard(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (16, 3, 10, 12),
            },
            Event::Zone {
                state: 30,
                area: HOME,
            },
            Event::Attacked {
                state: 50,
                after: 375,
            },
        ];
        guard(state, W)
    }

    fn persian_south_guard(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (20, 40, 20, 33),
            },
            Event::Zone {
                state: 30,
                area: HOME,
            },
            Event::Attacked {
                state: 50,
                after: 375,
            },
        ];
        guard(state, W)
    }

    /// The Persian raiders (0x45eff0): patrol until they are difficulty +
    /// 3 strong after 7500 ticks, then raid one of the player's towns in
    /// (40, 0, 13, 20), called off at two ships left.
    fn persian_raiders(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (16, 3, 10, 12),
            },
            Event::Zone {
                state: 30,
                area: HOME,
            },
            Event::Size {
                state: 10,
                after: Some(PATIENCE),
                cmp: Cmp::AtLeast,
                n: TARGET,
            },
            Event::Attacked {
                state: 50,
                after: 375,
            },
        ];
        const RAIDING: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (16, 3, 10, 12),
            },
            Event::Zone {
                state: 30,
                area: HOME,
            },
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: FEW,
            },
            Event::Attacked {
                state: 50,
                after: 375,
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: PERSIAN_ROUND,
                ticks: 0,
                events: W,
            },
            10 => Op::Custom(
                |scene, k| match scene.human_site_area(Some((40, 0, 13, 20))) {
                    Some(a) => {
                        scene.set_fleet_area(k, a);
                        40
                    }
                    None => 5,
                },
            ),
            20 | 30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 6000,
                events: RAIDING,
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }
}

/// miss008's brain (0x463240). The barbarians (player 16, group 0) have a
/// builder (member 0) and three yards: yards 1 and 2 (0x461050) each build
/// what raiders (member 4, 0x461480) ask of it, yard 3 (0x461220) what the
/// home guard (member 5, 0x461720) and the fishers (member 6, 0x461a40)
/// ask. The Persians (player 15, group 1) have a builder (member 0) and two
/// yards (0x461b30), yard 1 serving members 3-7, yard 2 members 8-12; their
/// merchants and fishers (0x462020, 0x462270), four patrols that answer the
/// player in their waters (0x462440, 0x462630, 0x4629b0, 0x462c40,
/// 0x462e30), member 7 (0x4628d0), the strike on (0, 30, 10, 10) that
/// members 6 and 10 hand their ships to, which then hands them to member
/// 13 to be mended (0x462fb0).
/// (Not ported: the spells the Persians cast on their patrols when they
/// attack (0x47acf0, kinds 9, 12 and 15): with no war temple none takes
/// effect, each call only crediting player 15 one cast's price (see
/// `campaign_spells_spec`). An attack's "failed" exit is taken as its "ok" one,
/// the runtime telling them apart only by time. 0x479ed0's test with
/// buildings (mask 0xffffff) is taken as `human_in`, without as
/// `human_ships_in`.)
mod miss008 {
    use super::*;

    const BOAT: u8 = 0;
    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;
    const KILLER: u8 = 6;
    /// The Persians' second own ship (code 0xd3).
    const PERSIAN_B: u8 = 11;

    /// Fleet variables: 0 the size a size event compares with (a yard: its
    /// rest; a merchant: its gathering's length), 1 the ticks before a
    /// size event looks, 2 the ship asked for (type + 1, 0 none; brain
    /// `+0x105` and on; a barbarian yard 1 or 2 holds the raiders' request
    /// it serves), 3 the size at which a raid is called off (a yard: the
    /// fleet it serves; Persian member 6: how often it has handed its
    /// ships on).
    const N: usize = 0;
    const AFTER: usize = 1;
    const ASKED: usize = 2;
    const FEW: usize = 3;
    const WAIT: usize = 0;
    const SERVING: usize = 3;
    const SENT: usize = 3;

    /// Fleet indices: the barbarians' members 0-6 are fleets 0-6, the
    /// Persians' members 0-13 fleets 7-20.
    const RAIDERS: usize = 4;
    const HOME: usize = 5;
    const FISHERS: usize = 6;
    const PERSIANS: usize = 7;
    const STRIKE: usize = PERSIANS + 7;
    const REPAIR: usize = PERSIANS + 13;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians.
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(12, 17, 1, 1), (18, 14, 1, 1), (20, 9, 1, 1)],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(10, 9, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(14, 8, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[(3, 2, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 4,
            cells: &[],
            program: raiders,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 5,
            cells: &[],
            program: home_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 6,
            cells: &[],
            program: barbarian_fish,
            wants: &[],
        },
        // The Persians.
        FleetDef {
            group: 1,
            member: 0,
            cells: &[
                (47, 10, 1, 1),
                (53, 8, 1, 1),
                (53, 14, 1, 1),
                (51, 18, 1, 1),
                (43, 17, 1, 1),
                (46, 27, 1, 1),
                (53, 27, 1, 1),
                (53, 31, 1, 1),
                (46, 37, 1, 1),
            ],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[(55, 38, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[(58, 23, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[],
            program: south_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[],
            program: south_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 5,
            cells: &[],
            program: south_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: south_raiders,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 7,
            cells: &[],
            program: strike,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 8,
            cells: &[],
            program: north_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 9,
            cells: &[],
            program: north_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 10,
            cells: &[],
            program: north_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 11,
            cells: &[],
            program: middle_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 12,
            cells: &[],
            program: east_guard,
            wants: &[],
        },
        // Mends what is handed to it (0x462fb0; prizes: `brain_prize`).
        FleetDef {
            group: 1,
            member: 13,
            cells: &[],
            program: repair,
            wants: &[],
        },
    ];

    /// The start (message 0x8d1): deposits by difficulty (0x48b180 amber,
    /// 0x48ebc0 fish, 0x4960b0 a leviathan: easy and normal in the west,
    /// normal a few more, hard some in the Persians' waters), the Persians'
    /// purse (normal 5000 amber and fish, hard 10000: 0x4a8de0, 0x4a8c30),
    /// and the fleets' numbers.
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        // Started on its own: the human's levels (a carried game's replace them).
        scene.standalone_levels([2, 2], 1);
        // The yards' full-town order and the builders' repairs (0x47b130).
        scene.start_jobs(&[(1, 1), (1, 2)], &[(0, 0), (1, 0)]);
        // The Persians' studies at the start (the main's 0x8d1).
        scene.persian_bonus(&[15], false, &[3]);
        let level = difficulty();
        if level == NORMAL {
            for (x, y) in [(12, 34), (13, 36)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            scene.add_deposit(0x191, (2, 42, 5, 5), data, caches);
        }
        if level <= NORMAL {
            for (x, y) in [(12, 41), (17, 35)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            scene.add_deposit(0x191, (4, 34, 5, 5), data, caches);
            scene.add_leviathan((25, 33, 5, 5), 0, data, caches);
        }
        if level == HARD {
            for (x, y) in [(54, 31), (49, 31)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            scene.add_deposit(0x191, (55, 33, 5, 5), data, caches);
        }
        let purse = [0, 5000, 10000][level];
        let persian = (PERSIANS..PERSIANS + 3)
            .flat_map(|k| scene.brain.as_ref().unwrap().fleets[k].ships.clone())
            .find_map(|id| scene.object_index(id).and_then(|i| scene.objects[i].owner));
        if let (Some(owner), true) = (persian, purse > 0) {
            scene.earn(owner, purse, purse);
        }
        let brain = scene.brain.as_mut().unwrap();
        // The raiders' and home guard's size events (0x5b1a08, 0x5b1a40
        // and the like): at least 0 after 4500 ticks, at most 2.
        brain.fleets[RAIDERS].vars = [0, 4500, 0, 2];
        brain.fleets[HOME].vars = [0, 4500, 0, 2];
        // Persian member 6's: at least 4 after 7500 ticks (0x5b1e70).
        brain.fleets[PERSIANS + 6].vars = [4, 7500, 0, 0];
    }

    /// Asks for ship `kind` in `holder`'s request if it is free.
    fn ask(scene: &mut Scene, holder: usize, kind: u8) {
        let f = &mut scene.brain.as_mut().unwrap().fleets[holder];
        if f.vars[ASKED] == 0 {
            f.vars[ASKED] = kind as u32 + 1;
        }
    }

    fn free(scene: &Scene, holder: usize) -> bool {
        scene.brain.as_ref().unwrap().fleets[holder].vars[ASKED] == 0
    }

    /// Each member's bookkeeping on its turn (the start of each member's
    /// routine): what to ask its yard for, handing ships on, and the
    /// barbarian home guard's strike while the player is in the Persians'
    /// waters.
    pub fn turn(scene: &mut Scene, k: usize) {
        let d = difficulty() as u32;
        let n = scene.fleet_types(k);
        let count = |types: &[u8]| types.iter().map(|&t| n[t as usize]).sum::<usize>() as u32;
        match k {
            // Two requests at a time, one in each of yards 1 and 2.
            RAIDERS => {
                if count(&[ROYAL, BOMBER, KILLER]) < 2 * d + 3 && (free(scene, 1) || free(scene, 2))
                {
                    let holder = if free(scene, 1) { 1 } else { 2 };
                    let kind = match scene.rng.next() % (d + 2) {
                        0 => KILLER,
                        3 => BOMBER,
                        _ => ROYAL,
                    };
                    ask(scene, holder, kind);
                }
            }
            HOME => {
                let have = count(&[ROYAL, BOMBER]);
                // The raiders all but gone and the player near the
                // barbarians' home: the guard joins them.
                let r = scene.fleet_types(RAIDERS);
                if r[ROYAL as usize] + r[BOMBER as usize] + r[KILLER as usize] < 2
                    && scene.human_in((9, 7, 14, 13))
                    && have != 0
                {
                    scene.fleet_merge(RAIDERS, HOME);
                }
                if have < d + 4 && free(scene, HOME) {
                    match scene.rng.next() % (d + 1) {
                        0 | 1 => ask(scene, HOME, ROYAL),
                        2 => ask(scene, HOME, BOMBER),
                        _ => {}
                    }
                }
                // The player's ships in the Persians' waters: strike at
                // (0, 30, 10, 10), anew every turn.
                if scene.human_ships_in((40, 0, 20, 20)) {
                    scene.set_state(k, 25);
                }
            }
            FISHERS if count(&[MERCHANT]) < 2 => ask(scene, FISHERS, MERCHANT),
            // Merchants (members 3, 8): a boat on easy, else a merchant.
            10 | 15 if count(&[BOAT, MERCHANT]) == 0 => {
                ask(scene, k, if d != 0 { MERCHANT } else { BOAT });
            }
            11 | 16 if count(&[MERCHANT]) < 1 => ask(scene, k, MERCHANT),
            12 => {
                if count(&[ROYAL, BOMBER]) < d + 3 && free(scene, k) {
                    let kind = if scene.rng.next() & 1 == 0 {
                        ROYAL
                    } else {
                        BOMBER
                    };
                    ask(scene, k, kind);
                }
            }
            13 => {
                let mut have = count(&[ROYAL, BOMBER, BOARDER, RAMMER]);
                // The player near the barbarians (table 0x5b1db8) and the
                // fleet strong enough: it joins the strike, up to 3 times.
                let sent = scene.brain.as_ref().unwrap().fleets[k].vars[SENT];
                if scene
                    .first_human_zone(&[(10, 7, 10, 10), (16, 0, 14, 10)])
                    .is_some()
                    && have >= d + 2
                    && sent < 3
                {
                    scene.fleet_merge(STRIKE, k);
                    have = 0;
                    scene.brain.as_mut().unwrap().fleets[k].vars[SENT] = sent + 1;
                }
                if have < d + 3 && free(scene, k) {
                    let kind = [ROYAL, BOMBER, BOARDER, RAMMER][scene.rng.next() as usize & 3];
                    ask(scene, k, kind);
                }
            }
            17 => {
                if count(&[ROYAL, BOARDER, RAMMER]) < d + 3 && free(scene, k) {
                    let kind = [RAMMER, ROYAL, BOARDER][scene.rng.next() as usize % 3];
                    ask(scene, k, kind);
                }
            }
            18 => {
                if count(&[BOARDER, PERSIAN_B]) < d + 3 && free(scene, k) {
                    match scene.rng.next() % (d + 1) {
                        0 => ask(scene, k, BOARDER),
                        _ => ask(scene, k, PERSIAN_B),
                    }
                }
            }
            19 if count(&[BOMBER]) < d + 3 => ask(scene, k, BOMBER),
            _ => {}
        }
    }

    fn idle(_: u32) -> Op {
        Op::Wait {
            next: 0,
            ticks: 1500,
        }
    }

    /// The fleet whose request yard `k` takes next: yards 1 and 2 hold the
    /// raiders' requests themselves; yard 3 serves the fishers before the
    /// home guard; the Persian yards their members in order.
    fn next_request(scene: &Scene, k: usize) -> Option<usize> {
        let asked = |f: usize| !free(scene, f);
        match k {
            1 | 2 => asked(k).then_some(k),
            3 => [FISHERS, HOME].into_iter().find(|&f| asked(f)),
            8 => (PERSIANS + 3..PERSIANS + 8).find(|&f| asked(f)),
            9 => (PERSIANS + 8..PERSIANS + 13).find(|&f| asked(f)),
            _ => None,
        }
    }

    /// A yard (0x461050, 0x461220, 0x461b30): takes the next request,
    /// builds it for the fleet that asked, rests `(30 + rand 41) x 25`
    /// ticks once it is launched, then clears the request. Codes a yard
    /// does not build are dropped (barbarians: merchants, royal ships,
    /// bombers, rammers, ship killers; Persians: types 0-6, 10, 11).
    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, k| match next_request(scene, k) {
                Some(f) => {
                    scene.brain.as_mut().unwrap().fleets[k].vars[SERVING] = f as u32;
                    10
                }
                None => 0,
            }),
            10 => Op::Custom(|scene, k| {
                let holder = scene.brain.as_ref().unwrap().fleets[k].vars[SERVING] as usize;
                let asked = scene.brain.as_ref().unwrap().fleets[holder].vars[ASKED];
                if asked == 0 {
                    return 0;
                }
                let kind = asked as u8 - 1;
                let builds = if k < PERSIANS {
                    matches!(kind, 1 | 2 | 3 | 5 | 6)
                } else {
                    matches!(kind, 0..=6 | 10 | 11)
                };
                if !builds {
                    return 13;
                }
                let fleet = if k < 3 { RAIDERS } else { holder };
                if scene.fleet_build_for(k, fleet, kind) {
                    11
                } else {
                    10
                }
            }),
            11 => Op::Custom(|scene, k| {
                if scene.fleet_building(k) {
                    return 11;
                }
                let wait = (scene.rng.next() % 41 + 30) * 25;
                scene.brain.as_mut().unwrap().fleets[k].vars[WAIT] = wait;
                12
            }),
            12 => Op::WaitVar {
                next: 13,
                var: WAIT,
            },
            _ => Op::Custom(|scene, k| {
                let holder = scene.brain.as_ref().unwrap().fleets[k].vars[SERVING] as usize;
                scene.brain.as_mut().unwrap().fleets[holder].vars[ASKED] = 0;
                0
            }),
        }
    }

    /// A barbarian fleet (0x461480, 0x461720): behaves, then patrols (5)
    /// or, already strong (`start`), patrols watching more of the sea
    /// (10); after 4500 ticks of the first it goes on to the second, which
    /// it leaves at two ships left. The player in its home waters is
    /// fought 1500 ticks (15), in the wider waters 6000 (20).
    fn barbarian(
        state: u32,
        start: fn(&mut Scene, usize) -> u32,
        round: &'static [(i32, i32, u32)],
        watch: &'static [Event],
        wide: &'static [Event],
    ) -> Op {
        match state {
            // Behave (a no-op here), then choose.
            0 => Op::Custom(start),
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: round,
                ticks: 0,
                events: watch,
            },
            10 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: round,
                ticks: 0,
                events: wide,
            },
            15 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 6000,
                events: wide,
            },
        }
    }

    const RAIDER_ROUND: &[(i32, i32, u32)] = &[
        (11, 21, 750),
        (16, 26, 1000),
        (26, 24, 750),
        (32, 19, 1000),
        (30, 13, 750),
        (23, 13, 1000),
    ];

    /// The raiders (member 4, 0x461480; round 0x5b18c0, events 0x5b1a70,
    /// 0x5b1a88).
    fn raiders(state: u32) -> Op {
        const ZONE: Event = Event::Zone {
            state: 15,
            area: (9, 8, 15, 14),
        };
        const WATCH: &[Event] = &[
            ZONE,
            Event::Size {
                state: 10,
                after: Some(AFTER),
                cmp: Cmp::AtLeast,
                n: N,
            },
        ];
        const WIDE: &[Event] = &[
            ZONE,
            Event::Zone {
                state: 20,
                area: (0, 0, 8, 20),
            },
            Event::Zone {
                state: 20,
                area: (8, 20, 25, 12),
            },
            Event::Zone {
                state: 20,
                area: (30, 10, 11, 11),
            },
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: FEW,
            },
        ];
        barbarian(
            state,
            |scene, k| {
                let n = scene.fleet_types(k);
                let have = n[ROYAL as usize] + n[BOMBER as usize] + n[KILLER as usize];
                if (have as u32) < 2 * difficulty() as u32 + 2 {
                    5
                } else {
                    10
                }
            },
            RAIDER_ROUND,
            WATCH,
            WIDE,
        )
    }

    /// The home guard (member 5, 0x461720; round 0x5b1aa0, events
    /// 0x5b1be8, 0x5b1c00), and its strike at (0, 30, 10, 10) while the
    /// player's ships are in the Persians' waters (25; see `turn`).
    fn home_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(2, 6, 750), (5, 11, 1000), (1, 10, 750)];
        const ZONE: Event = Event::Zone {
            state: 15,
            area: (0, 0, 6, 12),
        };
        const WATCH: &[Event] = &[
            ZONE,
            Event::Size {
                state: 10,
                after: Some(AFTER),
                cmp: Cmp::AtLeast,
                n: N,
            },
        ];
        const WIDE: &[Event] = &[
            ZONE,
            Event::Zone {
                state: 20,
                area: (9, 7, 14, 13),
            },
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: FEW,
            },
        ];
        match state {
            25 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((0, 30, 10, 10)),
                ticks: 6000,
                events: WIDE,
            },
            _ => barbarian(
                state,
                |scene, k| {
                    let n = scene.fleet_types(k);
                    let have = n[ROYAL as usize] + n[BOMBER as usize];
                    if (have as u32) < difficulty() as u32 + 3 {
                        5
                    } else {
                        10
                    }
                },
                ROUND,
                WATCH,
                WIDE,
            ),
        }
    }

    /// The barbarians' fishers (member 6, 0x461a40): fish (0, 7, 23, 15)
    /// 7500 ticks, rest 1500.
    fn barbarian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (0, 7, 23, 15),
                resource: Resource::Fish,
                ticks: 7500,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 1500,
            },
        }
    }

    /// A Persian merchant (members 3, 8; 0x462020): gathers amber
    /// `(3 + rand(difficulty + 1)) x 1500` ticks, sails to `to` (750 ticks
    /// at most), waits 500.
    fn merchant(state: u32, area: Cells, to: (i32, i32)) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Custom(|scene, k| {
                let ticks = (scene.rng.next() % (difficulty() as u32 + 1) + 3) * 1500;
                scene.brain.as_mut().unwrap().fleets[k].vars[0] = ticks;
                6
            }),
            6 => Op::Gather {
                next: 15,
                area,
                resource: Resource::Amber,
                ticks: VAR0,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to,
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 500,
            },
        }
    }

    fn south_amber(state: u32) -> Op {
        merchant(state, (27, 30, 35, 14), (52, 34))
    }

    fn north_amber(state: u32) -> Op {
        merchant(state, (42, 2, 20, 30), (50, 23))
    }

    /// A Persian fisher (members 4, 9; 0x462270): fishes `ticks`, sails to
    /// `to` (500 ticks at most), waits 250.
    fn fisher(state: u32, area: Cells, ticks: u32, to: (i32, i32)) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area,
                resource: Resource::Fish,
                ticks,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to,
                ticks: 500,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 250,
            },
        }
    }

    fn south_fish(state: u32) -> Op {
        fisher(state, (40, 30, 22, 14), 7500, (52, 34))
    }

    fn north_fish(state: u32) -> Op {
        fisher(state, (40, 0, 22, 25), 15000, (50, 23))
    }

    /// A Persian patrol (members 5, 6, 10, 11, 12): patrols its round 4500
    /// ticks at a time (then behaves anew); the player in a zone of its
    /// list is fought 4500 ticks with the same watch (20), in its first
    /// zone 1500 (30); whoever fires on it 375 (50). `done` follows a
    /// fight that ends.
    fn patrol(
        state: u32,
        round: &'static [(i32, i32, u32)],
        watch: &'static [Event],
        fight: u32,
        done: u32,
    ) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: round,
                ticks: 4500,
                events: watch,
            },
            // The fight's p4 is 0 for every member (0x4629b0 passes 0x28,
            // 0).
            20 => Op::Attack {
                next: done,
                failed: 0,
                target: Target::Seen,
                ticks: fight,
                events: watch,
            },
            30 => Op::Attack {
                next: done,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    /// Member 5 (0x462440; round 0x5b1ca8, events 0x5b1cd0): the south
    /// coast, its fights 1500 ticks.
    fn south_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(36, 34, 500), (43, 40, 750), (41, 38, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 30,
                area: (50, 30, 12, 14),
            },
            Event::Zone {
                state: 20,
                area: (30, 26, 10, 24),
            },
            Event::Zone {
                state: 20,
                area: (40, 30, 10, 14),
            },
        ];
        patrol(state, ROUND, W, 1500, 0)
    }

    const SOUTH_ROUND: &[(i32, i32, u32)] =
        &[(47, 33, 500), (36, 26, 750), (32, 32, 500), (37, 42, 750)];

    /// Member 6 (0x462630; round 0x5b1ea8, events 0x5b1f28, 0x5b1f10): the
    /// south-west; at least 4 strong after 7500 ticks (never, as its
    /// patrol restarts every 4500) it would watch wider (10). It hands its
    /// ships to the strike (see `turn`).
    fn south_raiders(state: u32) -> Op {
        const HOME: Event = Event::Zone {
            state: 30,
            area: (40, 30, 22, 14),
        };
        const FIRED_ON: Event = Event::Attacked {
            state: 50,
            after: 375,
        };
        const W: &[Event] = &[
            HOME,
            Event::Size {
                state: 10,
                after: Some(AFTER),
                cmp: Cmp::AtLeast,
                n: N,
            },
            FIRED_ON,
        ];
        const WIDE: &[Event] = &[
            HOME,
            Event::Zone {
                state: 20,
                area: (27, 24, 13, 19),
            },
            Event::Zone {
                state: 20,
                area: (40, 27, 5, 3),
            },
            Event::Zone {
                state: 20,
                area: (35, 4, 25, 15),
            },
            Event::Zone {
                state: 20,
                area: (45, 30, 17, 10),
            },
            FIRED_ON,
        ];
        match state {
            10 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: SOUTH_ROUND,
                ticks: 4500,
                events: WIDE,
            },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: WIDE,
            },
            _ => patrol(state, SOUTH_ROUND, W, 4500, 0),
        }
    }

    /// Member 7 (0x4628d0) runs only while it has royal ships, bombers,
    /// boarders or rammers (the byte sum of their counts, 0x479dc0, is
    /// not 0); the other members always.
    pub fn gate(scene: &Scene, k: usize) -> bool {
        if k != STRIKE {
            return true;
        }
        let n = scene.fleet_types(k);
        [ROYAL, BOMBER, BOARDER, RAMMER]
            .iter()
            .map(|&t| n[t as usize])
            .sum::<usize>() as u8
            != 0
    }

    /// Member 7 (0x4628d0): with the ships handed to it, attacks
    /// (0, 30, 10, 10) until done, then hands them to the repair member
    /// (and so whatever it gets until member 10 starts it again).
    fn strike(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Attack {
                next: 10,
                failed: 0,
                target: Target::Area((0, 30, 10, 10)),
                ticks: 0,
                events: &[],
            },
            _ => Op::Custom(|scene, k| {
                scene.fleet_merge(REPAIR, k);
                10
            }),
        }
    }

    const NORTH_WATCH: &[Event] = &[
        Event::Zone {
            state: 30,
            area: (43, 20, 19, 8),
        },
        Event::Zone {
            state: 20,
            area: (30, 10, 30, 10),
        },
        Event::Zone {
            state: 20,
            area: (50, 30, 12, 14),
        },
        Event::Attacked {
            state: 50,
            after: 375,
        },
    ];

    /// Member 10 (0x4629b0; round 0x5b1fd8, events 0x5b2040): the north;
    /// a fight won (40) sends it, members 5, 6 and 12 to the strike,
    /// which starts again.
    fn north_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(39, 11, 500), (41, 16, 750), (46, 20, 500), (51, 11, 750)];
        match state {
            40 => Op::Custom(|scene, k| {
                for from in [PERSIANS + 5, PERSIANS + 6, k, PERSIANS + 12] {
                    scene.fleet_merge(STRIKE, from);
                }
                scene.set_state(STRIKE, 0);
                5
            }),
            _ => patrol(state, ROUND, NORTH_WATCH, 4500, 40),
        }
    }

    /// Member 11 (0x462c40; round 0x5b20c8, events 0x5b20f0).
    fn middle_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(44, 23, 750), (51, 25, 750), (48, 20, 750)];
        patrol(state, ROUND, &NORTH_WATCH[..2], 4500, 0)
    }

    /// Member 12 (0x462e30; round 0x5b2170, events 0x5b2188).
    fn east_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(56, 16, 750), (60, 15, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 30,
                area: (43, 20, 19, 8),
            },
            Event::Zone {
                state: 20,
                area: (45, 10, 10, 10),
            },
        ];
        patrol(state, ROUND, W, 4500, 0)
    }

    /// Member 13 (0x462fb0): sends what it is handed to the docks.
    fn repair(_: u32) -> Op {
        Op::Custom(|scene, k| {
            scene.fleet_to_docks(k);
            0
        })
    }
}

/// miss009's brain (0x4663d0). The barbarians (player 16, group 0) have a
/// builder (member 0) and three yards (0x464930), each holding one request
/// (brain `+0x116`, `+0x118`, `+0x11a`): yard 1 builds for the raiders in
/// the north-west (member 4, 0x464b20), yard 2 for the raiders in the
/// south-west (member 5, 0x464da0), yard 3 for the guard of the south-east
/// (member 6, 0x465010); a merchant asked by the fishers (member 7,
/// 0x465200) in yard 2's or 3's request goes to them. The Persians (player
/// 14, group 1) have one yard (0x465390) serving members 1-4 in order:
/// merchants and boats gathering in the south-east (0x465830, 0x4659d0),
/// a patrol (0x465b20) and a guard (0x465da0) that, once the fire wizard
/// at (30, 1) is gone, takes the patrol's ships against (20, 0, 20, 15);
/// member 5 (0x466060) strikes at the player's ships carrying heroes, and
/// member 6 (0x4661e0) mends prizes. The brain's own tick casts lightning:
/// once at (10 or 12, 9) when the player first comes into (0, 9, 18, 5),
/// which also wakes the north-western raiders; and, on normal and hard,
/// on each of the player's temples in (10, 0, 40, 20) while the fire
/// wizard lives.
/// (Not ported: the spells the Persians cast on their own ships (0x47acf0:
/// member 3 kind 9, the Flame, up to difficulty + 3 times when it attacks;
/// members 3 and 4 kind 15, the weapon shield, each turn they fight in
/// their home waters; member 4 kinds 12 and 9 when it strikes, never in
/// fact as `+0x12f` is never counted down). A temple is "in" (10, 0,
/// 40, 20) by its cell.)
mod miss009 {
    use super::*;
    use crate::magic::Spell;
    use crate::objects::Treasure;
    use openconquest_formats::ObjectClass;

    const BOAT: u8 = 0;
    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;
    const KILLER: u8 = 6;
    /// The Persians' first own ship (code 0xd2).
    const PERSIAN_A: u8 = 10;

    /// Fleet indices: the barbarians' members 0-7 are fleets 0-7, the
    /// Persians' members 0-6 fleets 8-14.
    const YARD1: usize = 1;
    const YARD2: usize = 2;
    const YARD3: usize = 3;
    const RAIDERS: usize = 4;
    const SOUTH: usize = 5;
    const EAST: usize = 6;
    const FISHERS: usize = 7;
    const PERSIANS: usize = 8;
    const MERCHANTS: usize = PERSIANS + 1;
    const BOATS: usize = PERSIANS + 2;
    const PATROL: usize = PERSIANS + 3;
    const GUARD: usize = PERSIANS + 4;
    const HUNTERS: usize = PERSIANS + 5;

    /// Fleet variables: 2 the ship asked for (type + 1, 0 none; a
    /// barbarian yard's own request, a Persian member's `+0x11c` and on).
    /// The north-western raiders: 0 the size they raid at (difficulty + 3,
    /// set by the constructor 0x466260), 1 the ticks before they look
    /// (4500), 3 the size at which a raid is called off (2). A yard: 0 its
    /// rest, 3 the member it serves (Persians). Member 0: 0 the player has
    /// come into (0, 9, 18, 5) (`+0x104`). Member 5: 0 it has struck
    /// (`+0x105`). The Persian guard: 0 the fire wizard is gone
    /// (`+0x131`). The Persian merchants: 0 their gathering's length.
    const N: usize = 0;
    const AFTER: usize = 1;
    const ASKED: usize = 2;
    const FEW: usize = 3;
    const WAIT: usize = 0;
    const SERVING: usize = 3;
    const WOKEN: usize = 0;
    const STRUCK: usize = 0;
    const GONE: usize = 0;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians.
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(0, 9, 1, 1), (8, 7, 1, 1), (13, 5, 1, 1)],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(8, 3, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(17, 20, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[(37, 40, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 4,
            cells: &[(9, 2, 4, 5)],
            program: raiders,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 5,
            cells: &[],
            program: south_raiders,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 6,
            cells: &[(42, 39, 5, 4)],
            program: east_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 7,
            cells: &[],
            program: fishers,
            wants: &[],
        },
        // The Persians.
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(58, 43, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[],
            program: persian_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[],
            program: persian_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[(55, 35, 8, 6)],
            program: persian_patrol,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[],
            program: persian_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 5,
            cells: &[(55, 0, 15, 5)],
            program: hunters,
            wants: &[],
        },
        // Mends the Persians' prizes (0x4661e0; see `brain_prize`).
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: repair,
            wants: &[],
        },
    ];

    /// The start (message 0x8d1): on easy and normal amber at (7, 43) and
    /// (18, 25), fish at (21, 34), a leviathan zone (17, 30, 5, 5) and a
    /// barrel of 2500 amber at (15, 16) (0x520c70); normal adds amber at
    /// (13, 1) and (28, 20), fish at (32, 14) and two barrels of 2000 at
    /// (27, 11) and (36, 8). The Persians get 8000 amber and 6000 fish on
    /// normal and 7000 and 4000 more on hard (0x4a8de0 and 0x4a8c30 add);
    /// a wild sea monster comes up at (33, 14) on normal and hard
    /// (0x493780), a siren sings at (31, 14) on hard (0x404a20).
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        // Started on its own: the human's levels (a carried game's replace them).
        scene.standalone_levels([2, 2], 2);
        // The yards' full-town order and the builders' repairs (0x47b130).
        scene.start_jobs(&[(1, 0)], &[(0, 0)]);
        // The Persians' studies at the start (the main's 0x8d1).
        scene.persian_bonus(&[14], false, &[2, 3]);
        let level = difficulty();
        let t = TILE_SIZE as f32;
        let barrel = |scene: &mut Scene, (x, y), amount, caches: &mut Caches| {
            let tr = Treasure {
                kind: 2,
                param: 0,
                amount,
            };
            let (px, py) = (x as f32 * t + t / 2.0, y as f32 * t + t / 2.0);
            scene.place_treasure(tr, px, py, data, caches);
        };
        if level == NORMAL {
            for (x, y) in [(13, 1), (28, 20)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            scene.add_deposit(0x191, (32, 14, 5, 5), data, caches);
            barrel(scene, (27, 11), 2000, caches);
            barrel(scene, (36, 8), 2000, caches);
        }
        if level <= NORMAL {
            for (x, y) in [(7, 43), (18, 25)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            scene.add_deposit(0x191, (21, 34, 5, 5), data, caches);
            scene.add_leviathan((17, 30, 5, 5), 0, data, caches);
            barrel(scene, (15, 16), 2500, caches);
        }
        let persian = (PERSIANS..PERSIANS + 7)
            .flat_map(|k| scene.brain.as_ref().unwrap().fleets[k].ships.clone())
            .find_map(|id| scene.object_index(id).and_then(|i| scene.objects[i].owner));
        if level == HARD {
            if let Some(owner) = persian {
                scene.earn(owner, 7000, 4000);
            }
            let mut fields = vec![0u32; 12];
            fields[..2].copy_from_slice(&[0x1fd, 2]);
            fields[4..9].copy_from_slice(&[100_000, 31, 14, 5, 5]);
            scene.add_record(0x1fd, fields, data, caches);
        }
        if level >= NORMAL {
            if let Some(owner) = persian {
                scene.earn(owner, 8000, 6000);
            }
            let wild = Some(crate::combat::MONSTERS);
            let _ = scene.spawn_hazard(Spell::Monster, 33, 14, wild, data, caches);
        }
        let d = level as u32;
        scene.brain.as_mut().unwrap().fleets[RAIDERS].vars = [d + 3, 4500, 0, 2];
    }

    /// Asks for ship `kind` in `holder`'s request if it is free.
    fn ask(scene: &mut Scene, holder: usize, kind: u8) {
        let f = &mut scene.brain.as_mut().unwrap().fleets[holder];
        if f.vars[ASKED] == 0 {
            f.vars[ASKED] = kind as u32 + 1;
        }
    }

    fn free(scene: &Scene, holder: usize) -> bool {
        scene.brain.as_ref().unwrap().fleets[holder].vars[ASKED] == 0
    }

    fn var(scene: &Scene, k: usize, v: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].vars[v]
    }

    fn set_var(scene: &mut Scene, k: usize, v: usize, value: u32) {
        scene.brain.as_mut().unwrap().fleets[k].vars[v] = value;
    }

    /// The fire wizard's castle still stands at (30, 1) (the brain looks
    /// at that cell's object).
    fn wizard_lives(scene: &Scene) -> bool {
        scene.objects.iter().any(|o| {
            matches!(o.class, ObjectClass::Castle(_))
                && o.record.field(5) == Some(30)
                && o.record.field(6) == Some(1)
                && o.combat.as_ref().is_some_and(|c| c.hull > 0.0)
        })
    }

    /// The zones of the Persians' waters (0x5b25f8).
    const PERSIAN_ZONES: [Cells; 2] = [(50, 30, 20, 16), (39, 0, 23, 30)];

    /// The zones the south-eastern guard answers (0x5b24b0).
    const EAST_ZONES: [Cells; 6] = [
        (36, 30, 15, 16),
        (40, 21, 10, 10),
        (47, 15, 15, 10),
        (43, 2, 15, 10),
        (50, 30, 20, 16),
        (30, 25, 10, 10),
    ];

    /// Each member's bookkeeping on its turn (the start of each member's
    /// routine): what to ask its yard for, and the moves a member makes
    /// whatever its state.
    pub fn turn(scene: &mut Scene, k: usize) {
        let d = difficulty() as u32;
        let n = scene.fleet_types(k);
        let count = |types: &[u8]| types.iter().map(|&t| n[t as usize]).sum::<usize>() as u32;
        match k {
            RAIDERS => {
                if count(&[ROYAL, BOMBER, RAMMER, KILLER]) < d + 3 && free(scene, YARD1) {
                    let kind = match scene.rng.next() % (d + 3) {
                        0 | 1 => KILLER,
                        2 | 3 => ROYAL,
                        _ => BOMBER,
                    };
                    ask(scene, YARD1, kind);
                }
            }
            SOUTH => {
                let have = count(&[ROYAL, BOMBER, RAMMER, KILLER]);
                if have < d + 4 && free(scene, YARD2) {
                    let kind = match scene.rng.next() % (d + 3) {
                        0 => KILLER,
                        1 | 2 => RAMMER,
                        3 => ROYAL,
                        _ => BOMBER,
                    };
                    ask(scene, YARD2, kind);
                }
                // Full strength the first time: a strike at (0, 30, 10, 15).
                if var(scene, k, STRUCK) == 0 && have == d + 4 {
                    scene.set_state(k, 25);
                    set_var(scene, k, STRUCK, 1);
                }
            }
            EAST => {
                if count(&[ROYAL, BOMBER, RAMMER]) < 2 * (d + 2) && free(scene, YARD3) {
                    let kind = match scene.rng.next() % (d + 3) {
                        0 => RAMMER,
                        1 | 2 => ROYAL,
                        _ => BOMBER,
                    };
                    ask(scene, YARD3, kind);
                }
                // The player in one of its zones is attacked, but a turn at
                // a time: the next turn sends the guard home to (45, 41).
                let state = scene.brain.as_ref().unwrap().fleets[k].state;
                match scene.first_human_zone(&EAST_ZONES) {
                    Some(i) if state != 10 => {
                        scene.set_fleet_area(k, EAST_ZONES[i]);
                        scene.set_state(k, 10);
                    }
                    _ => scene.set_state(k, 20),
                }
            }
            FISHERS if n[MERCHANT as usize] < 2 => {
                if free(scene, YARD2) {
                    ask(scene, YARD2, MERCHANT);
                } else if free(scene, YARD3) {
                    ask(scene, YARD3, MERCHANT);
                }
            }
            // A boat on easy, else a merchant.
            MERCHANTS if count(&[BOAT, MERCHANT]) < 2 => {
                ask(scene, k, if d != 0 { MERCHANT } else { BOAT });
            }
            BOATS if n[BOAT as usize] < 1 => ask(scene, k, BOAT),
            PATROL => {
                if count(&[ROYAL, BOMBER, BOARDER, RAMMER, KILLER]) < 2 * (d + 2) && free(scene, k)
                {
                    let kind = [ROYAL, BOMBER, BOARDER][scene.rng.next() as usize % 3];
                    ask(scene, k, kind);
                }
            }
            GUARD => {
                if (n[PERSIAN_A as usize] as u32) < 2 * d + 5 {
                    ask(scene, k, PERSIAN_A);
                }
                // The fire wizard gone: the guard takes the patrol's ships
                // and strikes at (20, 0, 20, 15), once.
                if var(scene, k, GONE) == 0 && !wizard_lives(scene) {
                    set_var(scene, k, GONE, 1);
                    scene.set_state(k, 40);
                    scene.fleet_merge(k, PATROL);
                }
            }
            HUNTERS => {
                // The player in the Persians' waters: strike about one of
                // their ships carrying heroes (the last found).
                let state = scene.brain.as_ref().unwrap().fleets[k].state;
                if state == 0 && scene.first_human_zone(&PERSIAN_ZONES).is_some() {
                    let t = TILE_SIZE as f32;
                    let carriers: Vec<(i32, i32)> = scene
                        .objects
                        .iter()
                        .filter(|o| {
                            scene.is_own(o) && o.ship.as_ref().is_some_and(|s| !s.heroes.is_empty())
                        })
                        .map(|o| ((o.x / t) as i32, (o.y / t) as i32))
                        .collect();
                    if let Some(&(x, y)) = carriers.last() {
                        // 0x4796c0: two cells about the ship, within the map.
                        scene.set_fleet_area(k, ((x - 2).max(0), (y - 2).max(0), 5, 5));
                        scene.set_state(k, 10);
                    }
                }
            }
            _ => {}
        }
    }

    /// The brain's pass, before the members (0x4663d0, every 5 ticks):
    /// two numbers drawn, its tick run on the 50-tick phase they pick
    /// (`random_tick`).
    pub fn before(scene: &mut Scene) {
        if random_tick(scene) {
            tick(scene);
        }
    }

    /// The brain's tick (0x4665b2): lightning at (10 or 12, 9) the first
    /// time the player comes into (0, 9, 18, 5), which wakes the
    /// north-western raiders; on normal and hard, while the fire wizard
    /// lives, lightning on each of the player's temples (0x3d, 0x3e) in
    /// (10, 0, 40, 20).
    fn tick(scene: &mut Scene) {
        if var(scene, 0, WOKEN) == 0 && scene.human_in((0, 9, 18, 5)) {
            let x = if scene.rng.next() & 1 != 0 { 10 } else { 12 };
            scene.pending_casts.push((Spell::Lightning, x, 9, None));
            set_var(scene, 0, WOKEN, 1);
        }
        if difficulty() != EASY && var(scene, GUARD, GONE) == 0 {
            let temples: Vec<(i32, i32)> = scene
                .objects
                .iter()
                .filter(|o| {
                    scene.is_own(o)
                        && o.combat.is_some()
                        && matches!(o.class, ObjectClass::Building(0x3d | 0x3e))
                })
                .filter_map(|o| o.cell)
                .filter(|&(x, y)| (10..50).contains(&x) && (0..20).contains(&y))
                .collect();
            for (x, y) in temples {
                scene.pending_casts.push((Spell::Lightning, x, y, None));
            }
        }
        // While the fire wizard lives, the player's ships or forts in one
        // of six zones wake the lava there with strength 8 (0x445e00), on
        // every tick they stay.
        if var(scene, GUARD, GONE) == 0 {
            const LAVA: [(Cells, &[(i32, i32)]); 6] = [
                ((14, 4, 12, 15), &[(16, 7), (21, 8), (18, 14)]),
                ((15, 10, 16, 12), &[(18, 14), (22, 16), (27, 13)]),
                ((20, 22, 13, 11), &[(24, 24), (29, 27)]),
                ((33, 11, 11, 12), &[(36, 18), (39, 14)]),
                ((36, 6, 12, 11), &[(39, 14), (43, 11)]),
                ((30, 2, 12, 11), &[(33, 9), (37, 5)]),
            ];
            for (zone, flows) in LAVA {
                if scene.human_in(zone) {
                    for &cell in flows {
                        if let Some(id) = scene.volcano_at(cell) {
                            scene.wake_volcano(id, 8);
                        }
                    }
                }
            }
        }
    }

    fn idle(_: u32) -> Op {
        Op::Wait {
            next: 0,
            ticks: 1500,
        }
    }

    /// A barbarian yard (0x464930): builds the ship in its request, a
    /// merchant for the fishers, a warship for member yard + 3; rests
    /// `(50 + rand(41 - 10 x difficulty)) x 25` ticks once it is launched,
    /// then clears the request. Other codes (boarders) are dropped.
    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, k| if free(scene, k) { 0 } else { 10 }),
            10 => Op::Custom(|scene, k| {
                let asked = var(scene, k, ASKED);
                if asked == 0 {
                    return 0;
                }
                let kind = asked as u8 - 1;
                let fleet = match kind {
                    MERCHANT => FISHERS,
                    ROYAL | BOMBER | RAMMER | KILLER => k + 3,
                    _ => return 13,
                };
                if scene.fleet_build_for(k, fleet, kind) {
                    11
                } else {
                    10
                }
            }),
            11 => Op::Custom(|scene, k| {
                if scene.fleet_building(k) {
                    return 11;
                }
                let span = 41 - 10 * difficulty() as u32;
                let wait = (scene.rng.next() % span + 50) * 25;
                set_var(scene, k, WAIT, wait);
                12
            }),
            12 => Op::WaitVar {
                next: 13,
                var: WAIT,
            },
            _ => Op::Custom(|scene, k| {
                set_var(scene, k, ASKED, 0);
                0
            }),
        }
    }

    /// The Persians' yard (0x465390): takes the first of members 1-4's
    /// requests, builds it for them, rests `(30 + rand(41 - 10 x
    /// difficulty)) x 25` ticks once it is launched, then clears it. Codes
    /// it does not build (types 7-9) are dropped.
    fn persian_yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(
                |scene, k| match (MERCHANTS..=GUARD).find(|&f| !free(scene, f)) {
                    Some(f) => {
                        set_var(scene, k, SERVING, f as u32);
                        10
                    }
                    None => 0,
                },
            ),
            10 => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING) as usize;
                let asked = var(scene, holder, ASKED);
                if asked == 0 {
                    return 0;
                }
                let kind = asked as u8 - 1;
                if !matches!(kind, 0..=6 | 10 | 11) {
                    return 13;
                }
                if scene.fleet_build_for(k, holder, kind) {
                    11
                } else {
                    10
                }
            }),
            11 => Op::Custom(|scene, k| {
                if scene.fleet_building(k) {
                    return 11;
                }
                let span = 41 - 10 * difficulty() as u32;
                let wait = (scene.rng.next() % span + 30) * 25;
                set_var(scene, k, WAIT, wait);
                12
            }),
            12 => Op::WaitVar {
                next: 13,
                var: WAIT,
            },
            _ => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING) as usize;
                set_var(scene, holder, ASKED, 0);
                0
            }),
        }
    }

    const RAIDER_ROUND: &[(i32, i32, u32)] = &[(2, 11, 750), (8, 15, 1000), (13, 12, 750)];

    /// The north-western raiders (member 4, 0x464b20; round 0x5b21e8,
    /// events 0x5b23b0, 0x5b23c8): asleep until the player first comes
    /// into (0, 9, 18, 5) (see `tick`); then they patrol (5), or, already
    /// difficulty + 2 strong, patrol watching wider waters (10), going on
    /// to that once difficulty + 3 strong after 4500 ticks and back at two
    /// ships left. The player before their home (8, 1, 7, 10) is fought
    /// 1500 ticks (15), in the wider waters 6000 (20); whoever fires on
    /// them 375 (50).
    fn raiders(state: u32) -> Op {
        const HOME: Event = Event::Zone {
            state: 15,
            area: (8, 1, 7, 10),
        };
        const WATCH: &[Event] = &[
            HOME,
            Event::Size {
                state: 10,
                after: Some(AFTER),
                cmp: Cmp::AtLeast,
                n: N,
            },
        ];
        const WIDE: &[Event] = &[
            HOME,
            Event::Zone {
                state: 20,
                area: (0, 9, 18, 5),
            },
            Event::Zone {
                state: 20,
                area: (0, 30, 10, 15),
            },
            Event::Zone {
                state: 20,
                area: (0, 15, 20, 20),
            },
            Event::Size {
                state: 0,
                after: None,
                cmp: Cmp::AtMost,
                n: FEW,
            },
            Event::Attacked {
                state: 50,
                after: 375,
            },
        ];
        match state {
            // Behave (a no-op here), then choose.
            0 => Op::Custom(|scene, k| {
                if var(scene, 0, WOKEN) == 0 {
                    return 0;
                }
                let n = scene.fleet_types(k);
                let have = [ROYAL, BOMBER, RAMMER, KILLER]
                    .iter()
                    .map(|&t| n[t as usize])
                    .sum::<usize>();
                if (have as u32) < difficulty() as u32 + 2 {
                    5
                } else {
                    10
                }
            }),
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: RAIDER_ROUND,
                ticks: 0,
                events: WATCH,
            },
            10 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: RAIDER_ROUND,
                ticks: 0,
                events: WIDE,
            },
            15 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 6000,
                events: &WIDE[..5],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    /// The south-western raiders (member 5, 0x464da0; round 0x5b23e8,
    /// events 0x5b2498): patrol, fighting the player in (0, 15, 30, 20)
    /// 1500 ticks and whoever fires on them 375; at full strength
    /// (difficulty + 4) the first time they strike at (0, 30, 10, 15) for
    /// 2250 ticks (25; see `turn`).
    fn south_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(12, 27, 750), (18, 35, 1000), (23, 33, 750), (21, 28, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 15,
                area: (0, 15, 30, 20),
            },
            Event::Attacked {
                state: 50,
                after: 375,
            },
        ];
        match state {
            0 => Op::Behave { next: 10 },
            10 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            15 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            25 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((0, 30, 10, 15)),
                ticks: 2250,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    /// The south-eastern guard (member 6, 0x465010; zones 0x5b24b0, events
    /// 0x5b25e8): `turn` sends it against the first of its zones the
    /// player is in, `(difficulty + 1) x 1500` ticks at most (10), and the
    /// turn after (or with nobody there) home to (45, 41) (20); whoever
    /// fires on it while it attacks is fought 375 ticks (50).
    fn east_guard(state: u32) -> Op {
        match state {
            10 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: (difficulty() as u32 + 1) * 1500,
                events: &[Event::Attacked {
                    state: 50,
                    after: 375,
                }],
            },
            50 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Move {
                ok: 20,
                failed: 20,
                to: (45, 41),
                ticks: 750,
                events: &[],
            },
        }
    }

    /// The barbarians' fishers (member 7, 0x465200): fish (10, 20, 20, 20)
    /// or (35, 30, 15, 16), at random each time (brain `+0x106`), 7500
    /// ticks, rest 1500.
    fn fishers(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, _| if scene.rng.next() & 1 != 0 { 5 } else { 6 }),
            5 => Op::Gather {
                next: 15,
                area: (10, 20, 20, 20),
                resource: Resource::Fish,
                ticks: 7500,
                events: &[],
            },
            6 => Op::Gather {
                next: 15,
                area: (35, 30, 15, 16),
                resource: Resource::Fish,
                ticks: 7500,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 1500,
            },
        }
    }

    /// The Persian merchants (member 1, 0x465830): gather amber in (50, 28,
    /// 20, 18) `(3 + rand(difficulty + 1)) x 1500` ticks, sail to (62, 36)
    /// (1250 ticks at most), wait 2000.
    fn persian_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Custom(|scene, k| {
                let ticks = (scene.rng.next() % (difficulty() as u32 + 1) + 3) * 1500;
                set_var(scene, k, 0, ticks);
                6
            }),
            6 => Op::Gather {
                next: 15,
                area: (50, 28, 20, 18),
                resource: Resource::Amber,
                ticks: VAR0,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to: (62, 36),
                ticks: 1250,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 2000,
            },
        }
    }

    /// The Persian boats (member 2, 0x4659d0): fish (50, 28, 20, 18) 3000
    /// ticks, sail to (62, 36) (1250 ticks at most), wait 1375.
    fn persian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (50, 28, 20, 18),
                resource: Resource::Fish,
                ticks: 3000,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to: (62, 36),
                ticks: 1250,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 1375,
            },
        }
    }

    const PERSIAN_ROUND: &[(i32, i32, u32)] = &[(59, 32, 500), (60, 16, 750), (54, 22, 750)];

    /// The Persians' waters (0x5b26c8, 0x5b2718): their home (50, 30, 20,
    /// 16) is fought 1500 ticks (30), the waters north of it 1500 with the
    /// fight watched (20); whoever fires on them 375 (50).
    const PERSIAN_WATCH: &[Event] = &[
        Event::Zone {
            state: 30,
            area: PERSIAN_ZONES[0],
        },
        Event::Zone {
            state: 20,
            area: PERSIAN_ZONES[1],
        },
        Event::Attacked {
            state: 50,
            after: 375,
        },
    ];

    /// A Persian patrol (members 3 and 4): patrols its round 4500 ticks at
    /// a time (then behaves anew), fighting the player in its waters.
    fn patrol(state: u32, fight: &'static [Event]) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: PERSIAN_ROUND,
                ticks: 4500,
                events: PERSIAN_WATCH,
            },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: fight,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    /// Member 3 (0x465b20; round 0x5b2668): its fights in the north watch
    /// its zones only.
    fn persian_patrol(state: u32) -> Op {
        patrol(state, &PERSIAN_WATCH[..2])
    }

    /// Member 4 (0x465da0): the same with the attacked event, and its
    /// strike at (20, 0, 20, 15) for `(difficulty + 5) x 1500` ticks once
    /// the fire wizard is gone (40; see `turn`).
    fn persian_guard(state: u32) -> Op {
        match state {
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((20, 0, 20, 15)),
                ticks: (difficulty() as u32 + 5) * 1500,
                events: &[],
            },
            _ => patrol(state, PERSIAN_WATCH),
        }
    }

    /// Member 5 (0x466060): holds its waters; `turn` sends it against one
    /// of the player's ships carrying heroes, 500 ticks (10).
    fn hunters(state: u32) -> Op {
        match state {
            10 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 500,
                events: &[],
            },
            _ => Op::Custom(|_, _| 0),
        }
    }

    /// Member 6 (0x4661e0): sends what it is handed to the docks.
    fn repair(_: u32) -> Op {
        Op::Custom(|scene, k| {
            scene.fleet_to_docks(k);
            0
        })
    }
}

/// miss010's brain (0x46a0f0; constructor 0x469d90). The barbarians
/// (group 0) have a builder (member 0, the buildings at (5, 9), (11, 4),
/// (51, 19), (58, 29) and (56, 33)) and three yards (0x468050), each
/// holding one request (brain `+0x104`, `+0x106`, `+0x108`): yard 1 at
/// (4, 6) builds for the raiders of the north-west (member 4, 0x468240,
/// the ships in (0, 0, 8, 4)), yard 2 at (53, 30) for the patrol of the
/// east (member 5, 0x4684e0), yard 3 at (60, 32) for the guard of the
/// dragon's waters (member 6, 0x4687a0); a merchant the fishers (member 7,
/// 0x468ad0) ask of yard 1 goes to them. The Persians (player 15, group 1)
/// have a builder (member 0 at (23, 24)) and two yards (0x468bc0): yard 1
/// at (26, 30) serves members 3-5, yard 2 at (24, 14) members 6-9, in
/// order. Members 3 and 6 (0x4690b0) gather amber, 4 and 7 (0x469320)
/// fish, south and north of the strait; member 5 (0x469580) patrols the
/// south, member 8 (0x4697d0; the ships in (19, 6, 6, 6)) and member 9
/// (0x469a80) the strait, and member 10 (0x469d10) mends prizes. The
/// brain's own tick casts lightning at (58, 5), then at (59, 4), once the
/// player has delivered the Golden Fleece (message 0x5d94, kind 16).
/// Member 9's request is checked in its own slot (`+0x11c`, which nothing
/// sets) but written into member 8's (`+0x11a`), so member 9 never gets a
/// ship and member 8 gets the Persians' own ships too; ported as is.
/// (The Golden Fleece is delivered by
/// storing it in one of the player's buildings through the load panel
/// (0x5d94; automatic unloading never hands it over). An
/// attack's "failed" exit is taken as its "ok" one, as in miss008. A
/// Persian yard that cannot pay tries again on its next turn.)
mod miss010 {
    use super::*;
    use crate::magic::Spell;
    use crate::objects::Treasure;

    const BOAT: u8 = 0;
    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;
    const KILLER: u8 = 6;
    /// The Persians' own ships (codes 0xd2, 0xd3).
    const PERSIAN_A: u8 = 10;
    const PERSIAN_B: u8 = 11;

    /// Fleet indices: the barbarians' members 0-7 are fleets 0-7, the
    /// Persians' members 0-10 fleets 8-18.
    const YARD1: usize = 1;
    const YARD2: usize = 2;
    const YARD3: usize = 3;
    const RAIDERS: usize = 4;
    const EAST: usize = 5;
    const NEST: usize = 6;
    const FISHERS: usize = 7;
    const PERSIANS: usize = 8;
    const PYARD1: usize = PERSIANS + 1;
    const PYARD2: usize = PERSIANS + 2;
    const SOUTH_AMBER: usize = PERSIANS + 3;
    const SOUTH_FISH: usize = PERSIANS + 4;
    const SOUTH: usize = PERSIANS + 5;
    const NORTH_AMBER: usize = PERSIANS + 6;
    const NORTH_FISH: usize = PERSIANS + 7;
    const STRAIT: usize = PERSIANS + 8;
    const GUARD: usize = PERSIANS + 9;

    /// The members each Persian yard serves, in the order of its four
    /// request slots (yard 1's last slot, `+0x114`, is never set).
    const PYARD1_SERVES: [usize; 3] = [SOUTH_AMBER, SOUTH_FISH, SOUTH];
    const PYARD2_SERVES: [usize; 4] = [NORTH_AMBER, NORTH_FISH, STRAIT, GUARD];

    /// Fleet variables: 2 the ship asked for (type + 1, 0 none; a
    /// barbarian yard's own request, a Persian member's `+0x10e` and on).
    /// The patrols: 0 the size at which they watch wider waters (set by
    /// the constructor: 2 x difficulty + 4, the Persians' southern patrol
    /// difficulty + 4, member 9 3 x difficulty + 5), which is also the
    /// strength they ask their yard up to; 1 the ticks before they look
    /// (4500); 3 the size at which they fall back (2). A yard: 0 its rest,
    /// 3 the member it serves (Persians). A gatherer: 0 its gathering's
    /// length. Member 0: 0 the lightnings cast for the fleece (`+0x12f`
    /// counted down from 2).
    const N: usize = 0;
    const AFTER: usize = 1;
    const ASKED: usize = 2;
    const FEW: usize = 3;
    const WAIT: usize = 0;
    const SERVING: usize = 3;
    const CAST: usize = 0;
    /// Fleet 0's: the lava has been woken.
    const LAVA: usize = 1;

    /// The waters about the dragon's nest (40, 0, 25, 10) (0x479fd0).
    const NEST_WATERS: Cells = (40, 0, 25, 10);

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians.
        FleetDef {
            group: 0,
            member: 0,
            cells: &[
                (5, 9, 1, 1),
                (11, 4, 1, 1),
                (51, 19, 1, 1),
                (58, 29, 1, 1),
                (56, 33, 1, 1),
            ],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(4, 6, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(53, 30, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[(60, 32, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 4,
            cells: &[(0, 0, 8, 4)],
            program: raiders,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 5,
            cells: &[],
            program: east_patrol,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 6,
            cells: &[],
            program: nest_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 7,
            cells: &[],
            program: fishers,
            wants: &[],
        },
        // The Persians.
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(23, 24, 1, 1)],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[(26, 30, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[(24, 14, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[],
            program: south_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[],
            program: south_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 5,
            cells: &[],
            program: south_patrol,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: north_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 7,
            cells: &[],
            program: north_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 8,
            cells: &[(19, 6, 6, 6)],
            program: strait_patrol,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 9,
            cells: &[],
            program: guard_patrol,
            wants: &[],
        },
        // Mends the Persians' prizes (0x469d10; see `brain_prize`).
        FleetDef {
            group: 1,
            member: 10,
            cells: &[],
            program: repair,
            wants: &[],
        },
    ];

    /// The start (message 0x8d1): one of the barbarians' buildings at (58,
    /// 29) or (56, 33), at random, holds a disposable magic (treasure kind
    /// 9, parameter 6; message 0x158c1). On easy and normal amber at (30,
    /// 40) and (30, 28), fish at (3, 25), a leviathan zone (10, 20, 5, 5),
    /// a barrel of 2500 amber at (39, 40) (0x520c70) and 2500 amber held
    /// by the Persian yard at (26, 30); normal adds amber at (29, 12) and
    /// (39, 22), fish at (56, 23) and a barrel of 2000 at (27, 19). The
    /// Persians get 10000 amber and 8000 fish on normal, twice that on
    /// hard (0x4a8de0 and 0x4a8c30 add). The dragon's nest is laid by
    /// `place_scripted`.
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        // Started on its own: the human's levels (a carried game's replace them).
        scene.standalone_levels([2, 2], 2);
        // The yards' full-town order and the builders' repairs (0x47b130).
        scene.start_jobs(&[(1, 2)], &[(0, 0), (1, 0)]);
        // The Persians' studies at the start (the main's 0x8d1).
        scene.persian_bonus(&[15], false, &[2, 3]);
        let level = difficulty();
        let t = TILE_SIZE as f32;
        let at = |scene: &Scene, cell: (i32, i32)| {
            scene
                .objects
                .iter()
                .find(|o| {
                    // Where the brain finds its fleets' buildings (0x479200).
                    let here = ((o.x / t) as i32, (o.y / t) as i32);
                    o.combat.is_some() && (o.cell == Some(cell) || here == cell)
                })
                .map(|o| o.id)
        };
        let magic = if scene.rng.next() & 1 != 0 {
            (58, 29)
        } else {
            (56, 33)
        };
        if let Some(id) = at(scene, magic) {
            let tr = Treasure {
                kind: 9,
                param: 6,
                amount: 0,
            };
            scene.held.push((id, tr));
        }
        let barrel = |scene: &mut Scene, (x, y), amount, caches: &mut Caches| {
            let tr = Treasure {
                kind: 2,
                param: 0,
                amount,
            };
            let (px, py) = (x as f32 * t + t / 2.0, y as f32 * t + t / 2.0);
            scene.place_treasure(tr, px, py, data, caches);
        };
        if level == NORMAL {
            for (x, y) in [(29, 12), (39, 22)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            scene.add_deposit(0x191, (56, 23, 5, 5), data, caches);
            barrel(scene, (27, 19), 2000, caches);
        }
        if level <= NORMAL {
            for (x, y) in [(30, 40), (30, 28)] {
                scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
            }
            scene.add_deposit(0x191, (3, 25, 5, 5), data, caches);
            scene.add_leviathan((10, 20, 5, 5), 0, data, caches);
            barrel(scene, (39, 40), 2500, caches);
            if let Some(id) = at(scene, (26, 30)) {
                let tr = Treasure {
                    kind: 2,
                    param: 0,
                    amount: 2500,
                };
                scene.held.push((id, tr));
            }
        }
        let persian = (PERSIANS..PERSIANS + 3)
            .flat_map(|k| scene.brain.as_ref().unwrap().fleets[k].ships.clone())
            .find_map(|id| scene.object_index(id).and_then(|i| scene.objects[i].owner));
        if let Some(owner) = persian {
            let times = level as i64;
            scene.earn(owner, 10_000 * times, 8000 * times);
        }
        // The constructor's sizes (0x469d90).
        let d = level as u32;
        let fleets = &mut scene.brain.as_mut().unwrap().fleets;
        fleets[RAIDERS].vars = [2 * d + 4, 4500, 0, 2];
        fleets[EAST].vars = [2 * d + 4, 4500, 0, 2];
        fleets[SOUTH].vars = [d + 4, 4500, 0, 2];
        fleets[STRAIT].vars = [2 * d + 4, 4500, 0, 2];
        fleets[GUARD].vars = [3 * d + 5, 4500, 0, 2];
    }

    /// Asks for ship `kind` in `holder`'s request if it is free.
    fn ask(scene: &mut Scene, holder: usize, kind: u8) {
        let f = &mut scene.brain.as_mut().unwrap().fleets[holder];
        if f.vars[ASKED] == 0 {
            f.vars[ASKED] = kind as u32 + 1;
        }
    }

    fn free(scene: &Scene, holder: usize) -> bool {
        scene.brain.as_ref().unwrap().fleets[holder].vars[ASKED] == 0
    }

    fn var(scene: &Scene, k: usize, v: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].vars[v]
    }

    fn set_var(scene: &mut Scene, k: usize, v: usize, value: u32) {
        scene.brain.as_mut().unwrap().fleets[k].vars[v] = value;
    }

    fn state(scene: &Scene, k: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].state
    }

    /// How many ships of `types` fleet `k` has (0x479dc0).
    fn count(scene: &Scene, k: usize, types: &[u8]) -> u32 {
        let n = scene.fleet_types(k);
        types.iter().map(|&t| n[t as usize]).sum::<usize>() as u32
    }

    /// The warships each patrol counts.
    fn strength(k: usize) -> &'static [u8] {
        match k {
            RAIDERS => &[ROYAL, BOMBER, RAMMER, KILLER],
            STRAIT => &[ROYAL, BOMBER, BOARDER, RAMMER],
            GUARD => &[PERSIAN_A, PERSIAN_B],
            _ => &[ROYAL, BOMBER],
        }
    }

    /// Each member's bookkeeping on its turn (the start of each member's
    /// routine): what to ask its yard for, and the moves a member makes
    /// whatever its state.
    pub fn turn(scene: &mut Scene, k: usize) {
        let d = difficulty() as u32;
        let n = scene.fleet_types(k);
        let have = count(scene, k, strength(k));
        let roll = |scene: &mut Scene| scene.rng.next() % (d + 3);
        match k {
            RAIDERS => {
                if have < var(scene, k, N) && free(scene, YARD1) {
                    let kind = match roll(scene) {
                        0 => RAMMER,
                        1 | 2 => ROYAL,
                        _ => BOMBER,
                    };
                    ask(scene, YARD1, kind);
                }
            }
            EAST => {
                if have < var(scene, k, N) && free(scene, YARD2) {
                    let kind = if roll(scene) < 3 { ROYAL } else { BOMBER };
                    ask(scene, YARD2, kind);
                }
                // The player about the nest: strike there (40), unless
                // patrolling.
                if scene.human_in(NEST_WATERS) && !matches!(state(scene, k), 5 | 10 | 40) {
                    scene.set_state(k, 40);
                }
            }
            NEST => {
                if have < d + 5 && free(scene, YARD3) {
                    ask(scene, YARD3, if d != 0 { BOMBER } else { ROYAL });
                }
                // The player about the nest, and the guard more than two
                // strong: strike about one of their ships carrying heroes
                // (the last found; 30), else at the nest's waters (40).
                if scene.human_in(NEST_WATERS) && state(scene, k) != 40 && have > 2 {
                    let t = TILE_SIZE as f32;
                    let carriers: Vec<(i32, i32)> = scene
                        .objects
                        .iter()
                        .filter(|o| {
                            scene.is_own(o) && o.ship.as_ref().is_some_and(|s| !s.heroes.is_empty())
                        })
                        .map(|o| ((o.x / t) as i32, (o.y / t) as i32))
                        .collect();
                    for (x, y) in carriers {
                        // 0x4796c0: two cells about the ship, within the map.
                        scene.set_fleet_area(k, ((x - 2).max(0), (y - 2).max(0), 5, 5));
                        scene.set_state(k, 30);
                    }
                    if state(scene, k) != 30 {
                        scene.set_state(k, 40);
                    }
                }
            }
            FISHERS => {
                if n[MERCHANT as usize] < 2 && free(scene, YARD1) {
                    ask(scene, YARD1, MERCHANT);
                }
            }
            // A boat on easy, else a merchant, when it has neither.
            SOUTH_AMBER | NORTH_AMBER if n[BOAT as usize] + n[MERCHANT as usize] == 0 => {
                ask(scene, k, if d != 0 { MERCHANT } else { BOAT });
            }
            SOUTH_FISH | NORTH_FISH if n[MERCHANT as usize] < 1 => ask(scene, k, MERCHANT),
            SOUTH => {
                if have < var(scene, k, N) && free(scene, k) {
                    let kind = if roll(scene) < 3 { RAMMER } else { ROYAL };
                    ask(scene, k, kind);
                }
            }
            STRAIT => {
                if have < var(scene, k, N) && free(scene, k) {
                    let kind = match roll(scene) {
                        0 => RAMMER,
                        1 => BOARDER,
                        2 | 3 => ROYAL,
                        _ => BOMBER,
                    };
                    ask(scene, k, kind);
                }
            }
            // Checked in its own slot, written into member 8's, whatever
            // that holds.
            GUARD if have < var(scene, k, N) && free(scene, k) => {
                let kind = if roll(scene) < 2 {
                    PERSIAN_A
                } else {
                    PERSIAN_B
                };
                set_var(scene, STRAIT, ASKED, kind as u32 + 1);
            }
            _ => {}
        }
    }

    /// The brain's pass, before the members (0x46a0f0, every 5 ticks):
    /// two numbers drawn, its tick run on the 50-tick phase they pick
    /// (`random_tick`).
    pub fn before(scene: &mut Scene) {
        if random_tick(scene) {
            tick(scene);
        }
    }

    /// The brain's tick (0x46a313): once the player has delivered the
    /// Golden Fleece to one of their buildings (message 0x5d94 with kind
    /// 16), lightning at (58, 5), and on the next tick at (59, 4).
    fn tick(scene: &mut Scene) {
        let cast = var(scene, 0, CAST);
        if cast < 2 && scene.delivered.contains(&16) {
            let (x, y) = if cast == 0 { (58, 5) } else { (59, 4) };
            scene.pending_casts.push((Spell::Lightning, x, y, None));
            set_var(scene, 0, CAST, cast + 1);
        }
        // Once the dragon's nest at (59, 4) is gone, the lava about it
        // wakes with strength 9 (0x445e00): the flows recorded at (57, 4)
        // and (58, 5); the third call names (63, 4), where no flow is (it
        // lies at (63, 5)), and does nothing.
        if var(scene, 0, LAVA) == 0 && !scene.standing_at((59, 4)) {
            for cell in [(57, 4), (58, 5), (63, 4)] {
                if let Some(id) = scene.volcano_at(cell) {
                    scene.wake_volcano(id, 9);
                }
            }
            set_var(scene, 0, LAVA, 1);
        }
    }

    fn idle(_: u32) -> Op {
        Op::Wait {
            next: 0,
            ticks: 1500,
        }
    }

    /// A barbarian yard (0x468050): builds the ship in its request (10), a
    /// merchant for the fishers, a warship for member yard + 3; rests
    /// `(50 + rand(41 - 10 x difficulty)) x 25` ticks once it is launched
    /// (11), then clears the request (12). Other codes are dropped. (State
    /// 20, waiting for the launch, is the port's.)
    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, k| if free(scene, k) { 0 } else { 10 }),
            10 => Op::Custom(|scene, k| {
                let asked = var(scene, k, ASKED);
                if asked == 0 {
                    return 0;
                }
                let kind = asked as u8 - 1;
                let fleet = match kind {
                    MERCHANT => FISHERS,
                    ROYAL | BOMBER | RAMMER | KILLER => k + 3,
                    _ => return 12,
                };
                if scene.fleet_build_for(k, fleet, kind) {
                    20
                } else {
                    10
                }
            }),
            20 => Op::Custom(|scene, k| {
                if scene.fleet_building(k) {
                    return 20;
                }
                let span = 41 - 10 * difficulty() as u32;
                let wait = (scene.rng.next() % span + 50) * 25;
                set_var(scene, k, WAIT, wait);
                11
            }),
            11 => Op::WaitVar {
                next: 12,
                var: WAIT,
            },
            _ => Op::Custom(|scene, k| {
                set_var(scene, k, ASKED, 0);
                0
            }),
        }
    }

    /// A Persian yard (0x468bc0): takes the first request of the members
    /// it serves (0), builds it for that member (10), rests `(30 +
    /// rand(41)) x 25` ticks once it is launched (11), then clears it
    /// (12). Codes it does not build (types 7-9) are dropped.
    fn persian_yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, k| {
                let serves: &[usize] = match k {
                    PYARD1 => &PYARD1_SERVES,
                    PYARD2 => &PYARD2_SERVES,
                    _ => &[],
                };
                match serves.iter().find(|&&f| !free(scene, f)) {
                    Some(&f) => {
                        set_var(scene, k, SERVING, f as u32);
                        10
                    }
                    None => 0,
                }
            }),
            10 => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING) as usize;
                let asked = var(scene, holder, ASKED);
                if asked == 0 {
                    return 0;
                }
                let kind = asked as u8 - 1;
                if !matches!(kind, 0..=6 | 10 | 11) {
                    return 12;
                }
                if scene.fleet_build_for(k, holder, kind) {
                    20
                } else {
                    10
                }
            }),
            20 => Op::Custom(|scene, k| {
                if scene.fleet_building(k) {
                    return 20;
                }
                let wait = (scene.rng.next() % 41 + 30) * 25;
                set_var(scene, k, WAIT, wait);
                11
            }),
            11 => Op::WaitVar {
                next: 12,
                var: WAIT,
            },
            _ => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING) as usize;
                set_var(scene, holder, ASKED, 0);
                0
            }),
        }
    }

    /// Whoever fires on a patrol is fought 375 ticks (0x47a060).
    const ATTACKED: Event = Event::Attacked {
        state: 50,
        after: 375,
    };
    /// Strong enough after 4500 ticks: watch wider waters (0x47a2a0).
    const GROWN: Event = Event::Size {
        state: 10,
        after: Some(AFTER),
        cmp: Cmp::AtLeast,
        n: N,
    };
    /// Two ships or fewer: back to choosing (0x47a1e0).
    const SHRUNK: Event = Event::Size {
        state: 0,
        after: None,
        cmp: Cmp::AtMost,
        n: FEW,
    };

    /// A patrol's choice (state 0): short of its size less one, it patrols
    /// watching its home (5), else watching wider waters (10).
    fn choose() -> Op {
        Op::Custom(|scene, k| {
            if count(scene, k, strength(k)) + 1 < var(scene, k, N) {
                5
            } else {
                10
            }
        })
    }

    /// A patrol's states: patrol its `round` watching `home` (5) or `wide`
    /// (10); the player seen at home is fought 1500 ticks (15), in the
    /// wider waters 6000 watching `wide[..n]` (20), going on to `strike`
    /// if given (40); whoever fires on it 375 ticks (50).
    fn patrol(
        state: u32,
        round: &'static [(i32, i32, u32)],
        home: &'static [Event],
        wide: &'static [Event],
        n: usize,
        strike: Option<Cells>,
    ) -> Op {
        match state {
            0 => choose(),
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: round,
                ticks: 0,
                events: home,
            },
            10 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: round,
                ticks: 0,
                events: wide,
            },
            15 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            // p4 is 0 (0x468240, 0x469a80 pass 0x28, 0).
            20 => Op::Attack {
                next: if strike.is_some() { 40 } else { 0 },
                failed: 0,
                target: Target::Seen,
                ticks: 6000,
                events: &wide[..n],
            },
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area(strike.unwrap_or((0, 0, 0, 0))),
                ticks: 6000,
                events: &wide[..n],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    /// The strike the north-western raiders and the Persians' strait
    /// patrols go on to after a fight in their wider waters.
    const WEST_STRIKE: Cells = (0, 35, 20, 8);

    /// The north-western raiders (member 4, 0x468240; round 0x5b2780,
    /// events 0x5b2920, 0x5b2940): watch (0, 0, 10, 10), or, at least
    /// `2 x difficulty + 4` strong after 4500 ticks, (0, 10, 10, 20) and
    /// (10, 0, 20, 20) too; a fight there goes on to (0, 35, 20, 8). They
    /// ask yard 1 for rammers, royal ships and bombers.
    fn raiders(state: u32) -> Op {
        const HOME: Event = Event::Zone {
            state: 15,
            area: (0, 0, 10, 10),
        };
        const W: &[Event] = &[
            HOME,
            Event::Zone {
                state: 20,
                area: (0, 10, 10, 20),
            },
            Event::Zone {
                state: 20,
                area: (10, 0, 20, 20),
            },
            SHRUNK,
        ];
        const ROUND: &[(i32, i32, u32)] = &[(6, 4, 750), (1, 19, 1000), (9, 15, 750)];
        patrol(
            state,
            ROUND,
            &[HOME, GROWN, ATTACKED],
            W,
            W.len(),
            Some(WEST_STRIKE),
        )
    }

    /// The eastern patrol (member 5, 0x4684e0; round 0x5b2958, events
    /// 0x5b2ad8, 0x5b2af8): watches (50, 20, 15, 20), at full strength
    /// (40, 14, 25, 29) too; the player about the dragon's nest (40, 0, 25,
    /// 10) is fought 1500 ticks (40; see `turn`). It asks yard 2 for royal
    /// ships and bombers.
    fn east_patrol(state: u32) -> Op {
        const HOME: Event = Event::Zone {
            state: 15,
            area: (50, 20, 15, 20),
        };
        const W: &[Event] = &[
            HOME,
            Event::Zone {
                state: 20,
                area: (40, 14, 25, 29),
            },
            SHRUNK,
            ATTACKED,
        ];
        const ROUND: &[(i32, i32, u32)] = &[(49, 15, 750), (51, 27, 1000), (58, 27, 750)];
        match state {
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area(NEST_WATERS),
                ticks: 1500,
                events: &W[..3],
            },
            _ => patrol(state, ROUND, &[HOME, GROWN, ATTACKED], W, 3, None),
        }
    }

    /// The guard of the dragon's waters (member 6, 0x4687a0; round
    /// 0x5b2b18, events 0x5b2c30): patrols watching (50, 20, 15, 20) and
    /// (40, 14, 25, 29); `turn` sends it about a ship carrying heroes 500
    /// ticks (30) or to the nest's waters 1500 (40). It asks yard 3 for
    /// royal ships (easy) or bombers.
    fn nest_guard(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 15,
                area: (50, 20, 15, 20),
            },
            Event::Zone {
                state: 20,
                area: (40, 14, 25, 29),
            },
        ];
        const ROUND: &[(i32, i32, u32)] = &[(51, 23, 750), (56, 28, 1000), (54, 16, 750)];
        match state {
            0 => Op::Behave { next: 10 },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 500,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area(NEST_WATERS),
                ticks: 1500,
                events: &[],
            },
            _ => patrol(state, ROUND, W, W, W.len(), None),
        }
    }

    /// The barbarians' fishers (member 7, 0x468ad0): fish (0, 0, 20, 30)
    /// 7500 ticks, rest 1500.
    fn fishers(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (0, 0, 20, 30),
                resource: Resource::Fish,
                ticks: 7500,
                events: &[],
            },
            _ => Op::Wait {
                next: 0,
                ticks: 1500,
            },
        }
    }

    /// A Persian gatherer (0x4690b0, 0x469320): rolls its gathering's
    /// length `(rand(difficulty + 1) + base) x 1500` (5), gathers (6),
    /// sails home (1500 - 250 x difficulty ticks at most; 15), rests (20).
    fn gatherer(
        state: u32,
        roll: fn(&mut Scene, usize) -> u32,
        area: Cells,
        resource: Resource,
        home: (i32, i32),
        rest: u32,
    ) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Custom(roll),
            6 => Op::Gather {
                next: 15,
                area,
                resource,
                ticks: VAR0,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to: home,
                ticks: 1500 - 250 * difficulty() as u32,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: rest,
            },
        }
    }

    fn roll_length(scene: &mut Scene, k: usize, base: u32) -> u32 {
        let ticks = (scene.rng.next() % (difficulty() as u32 + 1) + base) * 1500;
        set_var(scene, k, 0, ticks);
        6
    }

    /// The southern merchants (member 3): amber in (20, 30, 20, 13), home
    /// to (26, 27), rest 750.
    fn south_amber(state: u32) -> Op {
        let roll = |scene: &mut Scene, k: usize| roll_length(scene, k, 1);
        gatherer(
            state,
            roll,
            (20, 30, 20, 13),
            Resource::Amber,
            (26, 27),
            750,
        )
    }

    /// The northern merchants (member 6): amber in (20, 0, 10, 25), home to
    /// (27, 9), rest 750.
    fn north_amber(state: u32) -> Op {
        let roll = |scene: &mut Scene, k: usize| roll_length(scene, k, 1);
        gatherer(state, roll, (20, 0, 10, 25), Resource::Amber, (27, 9), 750)
    }

    /// The southern fishers (member 4): fish in (20, 20, 10, 10), home to
    /// (26, 27), rest 1500 - 500 x difficulty.
    fn south_fish(state: u32) -> Op {
        let roll = |scene: &mut Scene, k: usize| roll_length(scene, k, 1);
        let rest = 1500 - 500 * difficulty() as u32;
        gatherer(
            state,
            roll,
            (20, 20, 10, 10),
            Resource::Fish,
            (26, 27),
            rest,
        )
    }

    /// The northern fishers (member 7): fish in (20, 5, 15, 10) a turn of
    /// 1500 longer, home to (27, 9), rest 1500 - 500 x difficulty.
    fn north_fish(state: u32) -> Op {
        let roll = |scene: &mut Scene, k: usize| roll_length(scene, k, 2);
        let rest = 1500 - 500 * difficulty() as u32;
        gatherer(state, roll, (20, 5, 15, 10), Resource::Fish, (27, 9), rest)
    }

    /// The Persians' southern patrol (member 5, 0x469580; round 0x5b2c40,
    /// events 0x5b2dc8, 0x5b2de8): watches (20, 25, 10, 15), at
    /// `difficulty + 4` strong (20, 20, 10, 20) and (0, 20, 20, 10). It
    /// asks yard 1 for rammers and royal ships.
    fn south_patrol(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 15,
                area: (20, 20, 10, 20),
            },
            Event::Zone {
                state: 20,
                area: (0, 20, 20, 10),
            },
            SHRUNK,
            ATTACKED,
        ];
        const HOME: &[Event] = &[
            Event::Zone {
                state: 15,
                area: (20, 25, 10, 15),
            },
            GROWN,
            ATTACKED,
        ];
        const ROUND: &[(i32, i32, u32)] =
            &[(12, 29, 750), (23, 36, 1000), (24, 28, 750), (17, 24, 750)];
        patrol(state, ROUND, HOME, W, 3, None)
    }

    /// The Persians' strait patrols (member 8, 0x4697d0, round 0x5b2e08,
    /// and member 9, 0x469a80, round 0x5b2fe8; events 0x5b2fa8, 0x5b2fc8,
    /// 0x5b3188, 0x5b31a8): watch (20, 10, 10, 10), at full strength (0,
    /// 10, 20, 10) and (20, 20, 10, 20) too, a fight there going on to (0,
    /// 35, 20, 8). Member 8 asks yard 2 for rammers, boarders, royal ships
    /// and bombers, member 9 for the Persians' own ships (into member 8's
    /// request).
    fn strait_patrol(state: u32) -> Op {
        const HOME: Event = Event::Zone {
            state: 15,
            area: (20, 10, 10, 10),
        };
        const W: &[Event] = &[
            HOME,
            Event::Zone {
                state: 20,
                area: (0, 10, 20, 10),
            },
            Event::Zone {
                state: 20,
                area: (20, 20, 10, 20),
            },
            SHRUNK,
            ATTACKED,
        ];
        const ROUND8: &[(i32, i32, u32)] = &[(18, 11, 750), (13, 23, 1000), (20, 22, 750)];
        patrol(
            state,
            ROUND8,
            &[HOME, GROWN, ATTACKED],
            W,
            4,
            Some(WEST_STRIKE),
        )
    }

    /// Member 9 (0x469a80): the same on its own round.
    fn guard_patrol(state: u32) -> Op {
        const ROUND9: &[(i32, i32, u32)] = &[(22, 5, 750), (22, 21, 1000), (13, 23, 750)];
        match strait_patrol(state) {
            Op::Patrol {
                timeout,
                failed,
                ticks,
                events,
                ..
            } => Op::Patrol {
                timeout,
                failed,
                points: ROUND9,
                ticks,
                events,
            },
            op => op,
        }
    }

    /// Member 10 (0x469d10): sends what it is handed to the docks.
    fn repair(_: u32) -> Op {
        Op::Custom(|scene, k| {
            scene.fleet_to_docks(k);
            0
        })
    }
}

/// miss011's brain (0x46e8e0; its members' handlers precede it,
/// 0x46b8d0-0x46e110). The barbarians (group 0) have two yards
/// (0x46b8d0 at (54, 11), 0x46c0b0 at (8, 31)), each serving its
/// members' request slots (brain `+0x118`, `+0x144`, by member) in order
/// and building for free: yard 1 for the patrol of the north-east (member
/// 1, 0x46bb30) and the raiders there (member 3, 0x46be60, the ships in
/// (55, 15, 3, 3)), yard 2 for the patrol of the west coast (member 5,
/// 0x46c310) and the raiders there (member 6, 0x46c4c0). Member 2
/// (0x46bce0, the ships in (6, 29, 2, 2)) fishes and gathers amber off the
/// west coast; members 7-10 (0x46c880-0x46cdc0, the ships in (0, 4),
/// (0, 10), (1, 18) and (1, 23)) keep the north-west, all striking where
/// one is fired on. The Persians (player 14, group 1) have a yard (member
/// 0, 0x46cf80, at (32, 17)) serving members 1-7 (`+0x170`) and resting
/// 500 ticks; members 1 and 2 (0x46d320, 0x46d470) gather amber and fish
/// in the middle sea, 3 and 4 (0x46d5c0, 0x46d830; the ships in (32, 21,
/// 2, 2) and (36, 17, 3, 2)) guard it, 5 (0x46da00, the ships in (38, 20,
/// 3, 3)) patrols it and, grown, strikes at the player's settlements once
/// the barbarians' yard at (54, 11) is gone (the brain's tick, `+0x19c`),
/// casting a spell on its ships first; 6 (0x46e090) mends prizes; 7
/// (0x46e110), with no ships, casts a spell now and then at the player's
/// things within 15 cells of the Persians' temple. Member 3 casts
/// lightning at (38, 14) and (30, 23) when the player comes into the
/// middle sea's western zones.
/// (Not ported: the yard's code 0x34 (class 0x30, never asked for, no
/// effect); member 5's states 30 and 40 (0x1e, 0x28), which no event
/// reaches. Member 7 looks through the player's things in the scene's
/// order, not the player's list's; a building is "in" an area by its
/// cell. A waterspout or sea monster asked for on land is not cast. An
/// attack's "failed" exit is taken as its "ok" one, as in miss008; a
/// fleet's move that becomes needless (members 3 and 6 growing to
/// strength) is called off on its next turn.)
mod miss011 {
    use super::*;
    use crate::magic::Spell;
    use crate::objects::MapObject;
    use openconquest_formats::ObjectClass;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const RAMMER: u8 = 5;
    const KILLER: u8 = 6;
    /// The Persians' own ships (codes 0xd2, 0xd3).
    const PERSIAN_A: u8 = 10;
    const PERSIAN_B: u8 = 11;

    /// Fleet indices: the barbarians' members 0-10 are fleets 0-10, the
    /// Persians' members 0-7 fleets 11-18.
    const YARD1: usize = 0;
    const EAST: usize = 1;
    const EAST_RAIDERS: usize = 3;
    const YARD2: usize = 4;
    const WEST: usize = 5;
    const WEST_RAIDERS: usize = 6;
    const SENTRIES: [usize; 4] = [7, 8, 9, 10];
    const PERSIANS: usize = 11;
    const P_AMBER: usize = PERSIANS + 1;
    const P_FISH: usize = PERSIANS + 2;
    const GUARD1: usize = PERSIANS + 3;
    const GUARD2: usize = PERSIANS + 4;
    const STRIKERS: usize = PERSIANS + 5;
    const CASTER: usize = PERSIANS + 7;

    /// The members each yard serves, in the order of its slots.
    const YARD1_SERVES: [usize; 2] = [EAST, EAST_RAIDERS];
    const YARD2_SERVES: [usize; 2] = [WEST, WEST_RAIDERS];
    const PERSIAN_SERVES: [usize; 5] = [P_AMBER, P_FISH, GUARD1, GUARD2, STRIKERS];

    /// No member served, no spell chosen (the original's -1).
    const NONE: u32 = u32::MAX;

    /// Fleet variables: 2 the ship asked for (type + 1, 0 none; a member's
    /// slot in its yard's requests). A yard: 0 its rest (barbarians), 3
    /// the member it serves. Yard 1: 1 the barbarians' yard at (54, 11)
    /// still stands (`+0x19c`). The Persian yard: 0 and 1 the Persians'
    /// player (nation, slot + 1; 0 unknown). The western raiders: 0 their
    /// patrol's length (`+0x190`). Member 5 (strikers): 0 the size at which
    /// it may strike (5), 1 the ticks before it looks (rolled, event data
    /// 0x5b3ec8), 3 the size below which it gives up (2, as "fewer than
    /// 3"). Member 7 (caster): 0 the tick its wait ends, 1 the spells it
    /// has cast (`+0x1e8`), 2 the spell member 5 casts on its ships when it
    /// strikes (`+0x194`: 0 at first, none after).
    const ASKED: usize = 2;
    const WAIT: usize = 0;
    const SERVING: usize = 3;
    const STANDS: usize = 1;
    const NATION: usize = 0;
    const SLOT: usize = 1;
    const N: usize = 0;
    const AFTER: usize = 1;
    const FEW: usize = 3;
    const UNTIL: usize = 0;
    const CASTS: usize = 1;
    const SPELL: usize = 2;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians.
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(54, 11, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[],
            program: east_patrol,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(6, 29, 2, 2)],
            program: coast_gatherers,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[(55, 15, 3, 3)],
            program: east_raiders,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 4,
            cells: &[(8, 31, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 5,
            cells: &[],
            program: west_patrol,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 6,
            cells: &[],
            program: west_raiders,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 7,
            cells: &[(0, 4, 2, 2)],
            program: sentry1,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 8,
            cells: &[(0, 10, 2, 2)],
            program: sentry2,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 9,
            cells: &[(1, 18, 2, 2)],
            program: sentry3,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 10,
            cells: &[(1, 23, 2, 2)],
            program: sentry4,
            wants: &[],
        },
        // The Persians.
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(32, 17, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[],
            program: persian_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[],
            program: persian_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[(32, 21, 2, 2)],
            program: guard1,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[(36, 17, 3, 2)],
            program: guard2,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 5,
            cells: &[(38, 20, 3, 3)],
            program: strikers,
            wants: &[],
        },
        // Mends the Persians' prizes (0x46e090; see `brain_prize`).
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: repair,
            wants: &[],
        },
        // Casts at the player near the Persians' temple (0x46e110; see
        // `turn`: it has no ships).
        FleetDef {
            group: 1,
            member: 7,
            cells: &[],
            program: idle,
            wants: &[],
        },
    ];

    /// The start (message 0x8d1): amber at (57, 26), (57, 35) and (51, 34)
    /// and fish at (53, 31) (0x48b180, 0x48ebc0, on every difficulty); the
    /// Persians get 12000, 17000 or 25000 amber and 8000, 10000 or 15000
    /// fish by difficulty (0x4a8de0 and 0x4a8c30 add).
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        // The Persians' studies at the start (the main's 0x8d1).
        scene.persian_bonus(&[14], true, &[2, 3]);
        let level = difficulty();
        for (x, y) in [(57, 26), (57, 35), (51, 34)] {
            scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
        }
        scene.add_deposit(0x191, (53, 31, 2, 2), data, caches);
        let persian = scene.brain.as_ref().unwrap().fleets[PERSIANS..=STRIKERS]
            .iter()
            .flat_map(|f| f.ships.clone())
            .find_map(|id| scene.object_index(id).and_then(|i| scene.objects[i].owner));
        if let Some(owner) = persian {
            let amber = [12_000, 17_000, 25_000][level];
            let fish = [8000, 10_000, 15_000][level];
            scene.earn(owner, amber, fish);
            set_var(scene, PERSIANS, NATION, owner.0);
            set_var(scene, PERSIANS, SLOT, owner.1 + 1);
        }
        for k in [YARD1, YARD2, PERSIANS] {
            set_var(scene, k, SERVING, NONE);
        }
        set_var(scene, YARD1, STANDS, 1);
        set_var(scene, STRIKERS, N, 5);
        set_var(scene, STRIKERS, FEW, 2);
    }

    /// Asks for ship `kind` in `holder`'s request slot if it is free.
    fn ask(scene: &mut Scene, holder: usize, kind: u8) {
        let f = &mut scene.brain.as_mut().unwrap().fleets[holder];
        if f.vars[ASKED] == 0 {
            f.vars[ASKED] = kind as u32 + 1;
        }
    }

    fn var(scene: &Scene, k: usize, v: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].vars[v]
    }

    fn set_var(scene: &mut Scene, k: usize, v: usize, value: u32) {
        scene.brain.as_mut().unwrap().fleets[k].vars[v] = value;
    }

    fn state(scene: &Scene, k: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].state
    }

    /// The Persians' player, as the start found it.
    fn persians(scene: &Scene) -> Option<(u32, u32)> {
        let slot = var(scene, PERSIANS, SLOT);
        (slot != 0).then(|| (var(scene, PERSIANS, NATION), slot - 1))
    }

    /// An object's cell: a building's own, else where it lies.
    fn cell_of(o: &MapObject) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        o.cell.unwrap_or(((o.x / t) as i32, (o.y / t) as i32))
    }

    /// Something other than a ship stands on map cell `cell` (the map
    /// cell's object).
    fn stands_at(scene: &Scene, cell: (i32, i32)) -> bool {
        scene.objects.iter().any(|o| {
            o.cell == Some(cell) || (o.ship.is_none() && o.combat.is_some() && cell_of(o) == cell)
        })
    }

    fn inside((x, y): (i32, i32), (ax, ay, w, h): Cells) -> bool {
        x >= ax && x < ax + w && y >= ay && y < ay + h
    }

    /// Each member's bookkeeping on its turn (the start of each member's
    /// handler): what to ask its yard for, and the moves a member makes
    /// whatever its state.
    pub fn turn(scene: &mut Scene, k: usize) {
        let d = difficulty();
        let n = scene.fleet_types(k);
        let have = |kind: u8| n[kind as usize];
        match k {
            EAST => {
                if have(ROYAL) < [1, 1, 2][d] {
                    ask(scene, k, ROYAL);
                }
                if have(BOMBER) < 1 {
                    ask(scene, k, BOMBER);
                }
            }
            EAST_RAIDERS | WEST_RAIDERS => {
                let (royal, rammer) = ([2, 2, 3][d], [2, 3, 2][d]);
                if have(ROYAL) < royal {
                    ask(scene, k, ROYAL);
                }
                if have(RAMMER) < rammer {
                    ask(scene, k, RAMMER);
                }
                // Grown while sailing to its post: patrol instead.
                if state(scene, k) == MARCH && strong(scene, k) {
                    scene.set_state(k, if k == EAST_RAIDERS { 10 } else { 6 });
                }
            }
            WEST => {
                if have(ROYAL) < [2, 2, 3][d] {
                    ask(scene, k, ROYAL);
                }
                if have(BOMBER) < [2, 3, 2][d] {
                    ask(scene, k, BOMBER);
                }
            }
            // A sentry fired on fights back, and the others join it at the
            // attacker (brain `+0x1c8`, states 0x1e and 0x28).
            7..=10 if state(scene, k) == 30 => {
                let area = scene.brain.as_ref().unwrap().fleets[k].last_area;
                for other in SENTRIES.into_iter().filter(|&o| o != k) {
                    scene.set_fleet_area(other, area);
                    scene.set_state(other, 40);
                }
            }
            P_AMBER | P_FISH if have(MERCHANT) < 1 => ask(scene, k, MERCHANT),
            GUARD1 | GUARD2 => {
                if have(ROYAL) < 2 {
                    ask(scene, k, ROYAL);
                }
                let own = if k == GUARD1 { PERSIAN_A } else { PERSIAN_B };
                if have(own) < [1, 1, 2][d] {
                    ask(scene, k, own);
                }
            }
            STRIKERS => {
                if have(BOMBER) < 2 {
                    ask(scene, k, BOMBER);
                }
                if have(ROYAL) < [1, 2, 2][d] {
                    ask(scene, k, ROYAL);
                }
                if have(PERSIAN_B) < [1, 1, 2][d] {
                    ask(scene, k, PERSIAN_B);
                }
                if have(PERSIAN_A) < [1, 1, 2][d] {
                    ask(scene, k, PERSIAN_A);
                }
                // The strike at a settlement begins with the spell.
                if state(scene, k) == 85 {
                    cast_on_strikers(scene);
                }
            }
            CASTER => caster(scene),
            _ => {}
        }
    }

    /// The brain's pass, after the members (0x46e8e0, every 5 ticks): once
    /// the barbarians' yard at (54, 11) is gone, member 5 starts over (and,
    /// grown, strikes at the player instead of patrolling on).
    pub fn after(scene: &mut Scene) {
        if var(scene, YARD1, STANDS) != 0 && !stands_at(scene, (54, 11)) {
            set_var(scene, YARD1, STANDS, 0);
            scene.set_state(STRIKERS, 0);
        }
    }

    fn idle(_: u32) -> Op {
        Op::Custom(|scene, k| state(scene, k))
    }

    /// A barbarian yard (0x46b8d0, 0x46c0b0): rolls its rest `((50, 30,
    /// 10) by difficulty + rand(31)) x 25` ticks if it has none, takes the
    /// first request of the members it serves (0), builds it for that
    /// member for free (10), rests once it is launched (11), then clears
    /// the request (12). Codes other than merchants, royal ships, bombers,
    /// rammers and ship killers are dropped. (State 20, waiting for the
    /// launch, is the port's.)
    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, k| {
                if var(scene, k, WAIT) == 0 {
                    let rest = ([50, 30, 10][difficulty()] + scene.rng.next() % 31) * 25;
                    set_var(scene, k, WAIT, rest);
                }
                let serves: &[usize] = if k == YARD1 {
                    &YARD1_SERVES
                } else {
                    &YARD2_SERVES
                };
                match serves.iter().find(|&&f| var(scene, f, ASKED) != 0) {
                    Some(&f) => {
                        set_var(scene, k, SERVING, f as u32);
                        10
                    }
                    None => 0,
                }
            }),
            10 => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder == NONE {
                    return 0;
                }
                let kind = var(scene, holder as usize, ASKED) as u8 - 1;
                if !matches!(kind, MERCHANT | ROYAL | BOMBER | RAMMER | KILLER) {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    set_var(scene, k, WAIT, 0);
                    return 10;
                }
                if scene.fleet_build_for(k, holder as usize, kind) {
                    20
                } else {
                    10
                }
            }),
            20 => Op::Custom(|scene, k| if scene.fleet_building(k) { 20 } else { 11 }),
            11 => Op::WaitVar {
                next: 12,
                var: WAIT,
            },
            _ => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder != NONE {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    set_var(scene, k, WAIT, 0);
                }
                0
            }),
        }
    }

    /// The Persian yard (0x46cf80): (its order of class 1 at 0: the full town, see `Job`) takes the first request of members 1-5 (5), builds it
    /// for that member (10), rests 500 ticks once it is launched (11),
    /// then clears it (12). (State 20, waiting for the launch, is the
    /// port's.)
    fn persian_yard(state: u32) -> Op {
        match state {
            // The full-town order before every ship (0x47b130 code 1).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_complete(k) {
                    5
                } else {
                    scene.want_full_town(k);
                    0
                }
            }),
            5 => Op::Custom(|scene, k| {
                match PERSIAN_SERVES.iter().find(|&&f| var(scene, f, ASKED) != 0) {
                    Some(&f) => {
                        set_var(scene, k, SERVING, f as u32);
                        10
                    }
                    None => 5,
                }
            }),
            10 => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder == NONE {
                    return 0;
                }
                let kind = var(scene, holder as usize, ASKED) as u8 - 1;
                if !matches!(kind, 0..=6 | PERSIAN_A | PERSIAN_B) {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    return 10;
                }
                if scene.fleet_build_for(k, holder as usize, kind) {
                    20
                } else {
                    10
                }
            }),
            20 => Op::Custom(|scene, k| if scene.fleet_building(k) { 20 } else { 11 }),
            11 => Op::Wait {
                next: 12,
                ticks: 500,
            },
            _ => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder != NONE {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                }
                0
            }),
        }
    }

    /// A fleet's fights: the zone seen 1500 ticks (20), whoever fired on
    /// it `answer` ticks (30).
    fn fight(state: u32, answer: u32) -> Op {
        match state {
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: answer,
                events: &[],
            },
        }
    }

    const fn attacked(after: u32) -> Event {
        Event::Attacked { state: 30, after }
    }

    const fn zone(area: Cells) -> Event {
        Event::Zone { state: 20, area }
    }

    /// The patrol of the north-east (member 1, 0x46bb30; round 0x5b3438,
    /// events 0x5b3498): watches (49, 12, 15, 6). It asks yard 1 for
    /// royal ships (1, 1, 2 by difficulty) and a bomber.
    fn east_patrol(state: u32) -> Op {
        const W: &[Event] = &[zone((49, 12, 15, 6)), attacked(250)];
        const ROUND: &[(i32, i32, u32)] = &[(50, 14, 750), (54, 14, 750), (59, 14, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            _ => fight(state, 375),
        }
    }

    /// The barbarians' gatherers (member 2, 0x46bce0): fish (0, 21, 17,
    /// 15) 4500 ticks, home to (2, 33) (750 at most), rest 3000; then amber
    /// the same.
    fn coast_gatherers(state: u32) -> Op {
        const AREA: Cells = (0, 21, 17, 15);
        let home = |next| Op::Move {
            ok: next,
            failed: next,
            to: (2, 33),
            ticks: 750,
            events: &[],
        };
        match state {
            0 => Op::Behave { next: 5 },
            5 | 11 => Op::Gather {
                next: state + 2,
                area: AREA,
                resource: if state == 5 {
                    Resource::Fish
                } else {
                    Resource::Amber
                },
                ticks: 4500,
                events: &[],
            },
            7 | 13 => home(state + 2),
            9 => Op::Wait {
                next: 11,
                ticks: 3000,
            },
            _ => Op::Wait {
                next: 0,
                ticks: 3000,
            },
        }
    }

    /// The port's state for a raiding party's sail to its post (members 3
    /// and 6 call the move at state 5 while short of strength).
    const MARCH: u32 = 105;

    /// Members 3 and 6 count their royal ships, rammers (and member 3 its
    /// bombers) against the royal ships and rammers they ask for.
    fn strong(scene: &Scene, k: usize) -> bool {
        let d = difficulty();
        let n = scene.fleet_types(k);
        let mut have = n[ROYAL as usize] + n[RAMMER as usize];
        if k == EAST_RAIDERS {
            have += n[BOMBER as usize];
        }
        have >= [2, 2, 3][d] + [2, 3, 2][d]
    }

    /// The raiders of the north-east (member 3, 0x46be60; the ships in
    /// (55, 15, 3, 3)): short of strength they sail to (50, 15) (45000
    /// ticks at most) and wait there 1000, watching (49, 12, 15, 5) (events
    /// 0x5b35d0); at strength they patrol (0x5b35a0) watching (49, 12, 15,
    /// 5) and (46, 16, 17, 8) (0x5b35e8). They ask yard 1 for royal ships
    /// and rammers.
    fn east_raiders(state: u32) -> Op {
        const MARCHING: &[Event] = &[zone((49, 12, 15, 5)), attacked(250)];
        const W: &[Event] = &[zone((49, 12, 15, 5)), zone((46, 16, 17, 8)), attacked(250)];
        const ROUND: &[(i32, i32, u32)] =
            &[(49, 17, 500), (56, 18, 750), (62, 16, 500), (56, 18, 875)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Custom(|scene, k| if strong(scene, k) { 10 } else { MARCH }),
            MARCH => Op::Move {
                ok: 7,
                failed: 7,
                to: (50, 15),
                ticks: 45_000,
                events: MARCHING,
            },
            7 => Op::Watch {
                next: 0,
                ticks: 1000,
                events: MARCHING,
            },
            10 => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            _ => fight(state, 375),
        }
    }

    const WEST_COAST: [Event; 2] = [zone((1, 27, 6, 8)), zone((7, 25, 8, 11))];

    /// The patrol of the west coast (member 5, 0x46c310; round 0x5b36a8,
    /// events 0x5b36c0): watches (1, 27, 6, 8) and (7, 25, 8, 11). It asks
    /// yard 2 for royal ships and bombers.
    fn west_patrol(state: u32) -> Op {
        const W: &[Event] = &[WEST_COAST[0], WEST_COAST[1], attacked(250)];
        const ROUND: &[(i32, i32, u32)] = &[(6, 28, 750), (11, 29, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 5,
                failed: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            _ => fight(state, 375),
        }
    }

    /// The raiders of the west (member 6, 0x46c4c0): roll their patrol's
    /// length `(rand(4) + (10, 8, 6) by difficulty) x 1500` (0); short of
    /// strength they sail to (3, 32) watching the coast (events 0x5b3840),
    /// at strength patrol (0x5b3810) watching (11, 19, 9, 17) too
    /// (0x5b3858), then attack about one of the player's settlements or
    /// towns in (0, 0, 35, 36) 6000 ticks (10, 15). They answer fire 250
    /// ticks and ask yard 2 for royal ships and rammers.
    fn west_raiders(state: u32) -> Op {
        const MARCHING: &[Event] = &[WEST_COAST[0], WEST_COAST[1], attacked(375)];
        const W: &[Event] = &[
            WEST_COAST[0],
            WEST_COAST[1],
            zone((11, 19, 9, 17)),
            attacked(375),
        ];
        const ROUND: &[(i32, i32, u32)] =
            &[(14, 21, 500), (15, 26, 750), (17, 31, 500), (15, 26, 875)];
        match state {
            0 => Op::Custom(|scene, k| {
                let ticks = (scene.rng.next() % 4 + [10, 8, 6][difficulty()]) * 1500;
                set_var(scene, k, 0, ticks);
                5
            }),
            5 => Op::Custom(|scene, k| if strong(scene, k) { 6 } else { MARCH }),
            MARCH => Op::Move {
                ok: 0,
                failed: 0,
                to: (3, 32),
                ticks: 0,
                events: MARCHING,
            },
            6 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Custom(
                |scene, k| match scene.human_site_area(Some((0, 0, 35, 36))) {
                    Some(a) => {
                        scene.set_fleet_area(k, a);
                        15
                    }
                    None => 0,
                },
            ),
            15 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 6000,
                events: &[],
            },
            _ => fight(state, 250),
        }
    }

    /// A sentry of the north-west (members 7-10, 0x46c880-0x46cdc0): holds
    /// its post (sailing there again every 750 ticks), watching (5, 7, 9,
    /// 7) (events 0x5b38f0 and on): the player there is attacked in (0, 0,
    /// 17, 20) 1500 ticks (20); fired on, it fights the attacker 1500
    /// ticks (30, `+0x40` copied to the brain's `+0x1c8`) and the other
    /// sentries join it (40; see `turn`).
    fn sentry(state: u32, post: (i32, i32)) -> Op {
        const W: &[Event] = &[zone((5, 7, 9, 7)), attacked(0)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 0,
                failed: 0,
                to: post,
                ticks: 750,
                events: W,
            },
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((0, 0, 17, 20)),
                ticks: 1500,
                events: &[],
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::LastArea,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn sentry1(state: u32) -> Op {
        sentry(state, (0, 4))
    }

    fn sentry2(state: u32) -> Op {
        sentry(state, (0, 10))
    }

    fn sentry3(state: u32) -> Op {
        sentry(state, (1, 18))
    }

    fn sentry4(state: u32) -> Op {
        sentry(state, (1, 23))
    }

    /// A Persian gatherer (members 1 and 2, 0x46d320, 0x46d470): asks for
    /// a merchant while it has none, gathers 4500 ticks, sails home to
    /// (37, 24) (750 at most) and rests 1500.
    fn gatherer(state: u32, area: Cells, resource: Resource) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area,
                resource,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (37, 24),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 1500,
            },
        }
    }

    /// Member 1: amber in (31, 23, 11, 7).
    fn persian_amber(state: u32) -> Op {
        gatherer(state, (31, 23, 11, 7), Resource::Amber)
    }

    /// Member 2: fish in (32, 18, 11, 12).
    fn persian_fish(state: u32) -> Op {
        gatherer(state, (32, 18, 11, 12), Resource::Fish)
    }

    /// The zones of the middle sea the guards watch (0x5b3ad0, 0x5b3bd8).
    const MIDDLE: [Cells; 4] = [
        (30, 19, 8, 5),
        (34, 15, 6, 5),
        (28, 10, 8, 6),
        (22, 13, 8, 15),
    ];
    const GUARDING: &[Event] = &[
        zone(MIDDLE[0]),
        zone(MIDDLE[1]),
        zone(MIDDLE[2]),
        zone(MIDDLE[3]),
        attacked(375),
    ];

    /// A guard of the middle sea (members 3 and 4): sails to its post and
    /// waits there 1000 ticks, watching the middle sea's zones (events
    /// 0x5b3bc0, 0x5b3cc8); the player there is fought 1500 ticks, whoever
    /// fires on it 375. Each asks for two royal ships and its own Persian
    /// ships (1, 1, 2).
    fn guard(state: u32, post: (i32, i32)) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 7,
                failed: 7,
                to: post,
                ticks: 0,
                events: GUARDING,
            },
            7 => Op::Watch {
                next: 0,
                ticks: 1000,
                events: GUARDING,
            },
            _ => fight(state, 375),
        }
    }

    /// Member 3 (0x46d5c0; post (38, 18)): the player in the western zones
    /// (28, 10, 8, 6) or (22, 13, 8, 15) brings lightning (0x47ae10 kind
    /// 10, cast by member 0 for nothing) at (38, 14) and at (30, 23), each
    /// if something stands there (20); two turns later it attacks (22,
    /// 25).
    fn guard1(state: u32) -> Op {
        match state {
            20 => Op::Custom(|scene, k| {
                let seen = scene.brain.as_ref().unwrap().fleets[k].area;
                if seen == Some(MIDDLE[2]) || seen == Some(MIDDLE[3]) {
                    let owner = persians(scene);
                    for cell in [(38, 14), (30, 23)] {
                        if stands_at(scene, cell) {
                            scene
                                .pending_casts
                                .push((Spell::Lightning, cell.0, cell.1, owner));
                        }
                    }
                }
                22
            }),
            22 => Op::Custom(|_, _| 25),
            25 => fight(20, 375),
            _ => guard(state, (38, 18)),
        }
    }

    /// Member 4 (0x46d830; post (34, 23)).
    fn guard2(state: u32) -> Op {
        guard(state, (34, 23))
    }

    /// Member 5's strikes end on the player in the middle sea (20) or with
    /// fewer than three ships (0x5b3f58).
    const STRIKING: &[Event] = &[
        zone(MIDDLE[0]),
        zone(MIDDLE[1]),
        zone((38, 16, 7, 13)),
        Event::Size {
            state: 0,
            after: None,
            cmp: Cmp::AtMost,
            n: FEW,
        },
    ];

    /// Member 5 casts the spell it chose on its ships (0x47acf0, by
    /// member 0, for nothing), once.
    fn cast_on_strikers(scene: &mut Scene) {
        let kind = var(scene, CASTER, SPELL);
        if kind == NONE {
            return;
        }
        let spell = match kind {
            9 => Some(Spell::Flame),
            12 => Some(Spell::Phantom),
            15 => Some(Spell::ProtectWeapons),
            16 => Some(Spell::ProtectMagic),
            _ => None,
        };
        if let Some(spell) = spell {
            let ships = scene.fleet_ships(STRIKERS);
            scene.brain_cast_fleet(ships, spell, false);
        }
        set_var(scene, CASTER, SPELL, NONE);
    }

    /// Member 5 (0x46da00; the ships in (38, 20, 3, 3)): rolls when it
    /// looks, `((8, 6, 4) by difficulty + rand(4)) x 1500` ticks, and, if
    /// it has none, a spell: flame, the phantom or the weapon shield, or
    /// none, from `rand((10, 6, 3) by difficulty + 1)` (0). It patrols
    /// (0x5b3ce0) watching the middle sea (events 0x5b3ef0) (the player
    /// there fought 1500 ticks, 20); five strong when it looks (25), it
    /// starts over while the barbarians' yard at (54, 11) stands, else
    /// strikes about one of the player's settlements or towns (60, 85), or
    /// if there are none about the first of the player's things (90, 100),
    /// 6000 ticks. It asks for bombers, royal ships and both Persian ships.
    fn strikers(state: u32) -> Op {
        const W: &[Event] = &[
            zone(MIDDLE[0]),
            zone(MIDDLE[1]),
            zone((38, 16, 7, 13)),
            Event::Size {
                state: 25,
                after: Some(AFTER),
                cmp: Cmp::AtLeast,
                n: N,
            },
        ];
        const ROUND: &[(i32, i32, u32)] = &[(38, 24, 500), (40, 19, 750)];
        let strike = Op::Attack {
            next: 0,
            failed: 0,
            target: Target::Seen,
            ticks: 6000,
            events: STRIKING,
        };
        match state {
            0 => Op::Custom(|scene, k| {
                let d = difficulty();
                let after = ([8, 6, 4][d] + (scene.rng.next() & 3)) * 1500;
                set_var(scene, k, AFTER, after);
                let roll = scene.rng.next() % ([10, 6, 3][d] + 1);
                if var(scene, CASTER, SPELL) == NONE {
                    let kind = match roll {
                        0 | 1 => 9,
                        2 => 12,
                        3 => 15,
                        _ => NONE,
                    };
                    set_var(scene, CASTER, SPELL, kind);
                }
                // `+0x198`, rolled and never read.
                scene.rng.next();
                5
            }),
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            20 => fight(20, 375),
            25 => Op::Custom(|scene, _| {
                if var(scene, YARD1, STANDS) != 0 {
                    0
                } else {
                    60
                }
            }),
            60 => Op::Custom(
                |scene, k| match scene.human_site_area(Some((0, 0, 64, 36))) {
                    Some(a) => {
                        scene.set_fleet_area(k, a);
                        85
                    }
                    None => 90,
                },
            ),
            90 => Op::Custom(|scene, k| {
                let first = scene
                    .objects
                    .iter()
                    .find(|o| scene.is_own(o) && o.combat.is_some())
                    .map(cell_of);
                match first {
                    Some(cell) => {
                        cast_on_strikers(scene);
                        scene.set_fleet_area(k, scene.cells_about(cell, 2));
                        100
                    }
                    None => 90,
                }
            }),
            _ => strike,
        }
    }

    /// Member 6 (0x46e090): sends what it is handed to the docks.
    fn repair(_: u32) -> Op {
        Op::Custom(|scene, k| {
            scene.fleet_to_docks(k);
            0
        })
    }

    /// The things of the player in `area`, ships and buildings (0x479ed0
    /// with masks 0xfff and 0xffffff).
    fn human_count(scene: &Scene, area: Cells) -> usize {
        scene
            .objects
            .iter()
            .filter(|o| scene.is_own(o) && o.combat.is_some() && inside(cell_of(o), area))
            .count()
    }

    /// Member 7 (0x46e110), run from `turn` as it has no ships: waits
    /// `(3, 2, 1) by difficulty x 1500` ticks (10), then, if it has cast
    /// fewer than (1, 2, 3) spells, looks through the player's things
    /// within 15 cells of the Persians' first temple (class 0x3e) (15) and
    /// casts one spell (0x47ae10 by member 0, for nothing) at the first it
    /// picks: a fireball at a settlement one time in two; lightning at a
    /// town or temple (0x3e); a waterspout or fireball, one time in four
    /// each, at a temple (0x3d), fort (0x36) or building of class 0x34,
    /// 0x35; at anything else with at least four of the player's things
    /// within 3 cells, a waterspout, fireball or sea monster one time in
    /// (20, 15, 10) + 1 each. A roll that picks nothing ends the look.
    fn caster(scene: &mut Scene) {
        let k = CASTER;
        let d = difficulty();
        match state(scene, k) {
            0 => scene.set_state(k, 10),
            10 => {
                let now = scene.brain.as_ref().unwrap().tick;
                let until = var(scene, k, UNTIL);
                if until == 0 {
                    set_var(scene, k, UNTIL, now + [3, 2, 1][d] * 1500);
                } else if now >= until {
                    set_var(scene, k, UNTIL, 0);
                    scene.set_state(k, 15);
                }
            }
            15 if var(scene, k, CASTS) < [1, 2, 3][d] as u32 => {
                look_and_cast(scene);
                scene.set_state(k, 0);
            }
            _ => {}
        }
    }

    fn look_and_cast(scene: &mut Scene) {
        let d = difficulty();
        let Some(persian) = persians(scene) else {
            return;
        };
        let Some(temple) = scene
            .objects
            .iter()
            .find(|o| o.owner == Some(persian) && o.class == ObjectClass::Building(0x3e))
            .map(cell_of)
        else {
            return;
        };
        let area = scene.cells_about(temple, 15);
        let things: Vec<(ObjectClass, (i32, i32))> = scene
            .objects
            .iter()
            .filter(|o| scene.is_own(o) && o.combat.is_some())
            .map(|o| (o.class, cell_of(o)))
            .collect();
        for (class, cell) in things {
            if !inside(cell, area) {
                continue;
            }
            let (spell, at) = match class {
                ObjectClass::Building(0x32) => match scene.rng.next() & 3 {
                    0 | 2 => (Spell::Fireball, cell),
                    _ => break,
                },
                ObjectClass::Building(0x33 | 0x3e) => (Spell::Lightning, cell),
                ObjectClass::Building(0x34 | 0x35 | 0x36 | 0x3d) => match scene.rng.next() & 3 {
                    0 => (Spell::Waterspout, cell),
                    2 => (Spell::Fireball, cell),
                    _ => break,
                },
                _ => {
                    let roll = scene.rng.next() % ([20, 15, 10][d] + 1);
                    let near = scene.cells_about(cell, 3);
                    let at = (near.0 + near.2 / 2, near.1 + near.3 / 2);
                    if human_count(scene, near) < 4 {
                        break;
                    }
                    match roll {
                        0 => (Spell::Waterspout, at),
                        4 => (Spell::Fireball, at),
                        8 => (Spell::Monster, at),
                        _ => break,
                    }
                }
            };
            scene.pending_casts.push((spell, at.0, at.1, Some(persian)));
            let casts = var(scene, CASTER, CASTS);
            set_var(scene, CASTER, CASTS, casts + 1);
            break;
        }
    }
}

/// miss012's brain (0x472fa0; its members' handlers precede it,
/// 0x46fb80-0x472890, with the repair handlers 0x475a70 and 0x477190;
/// 0x46f760 is its judge). Three computer players: the barbarians (group
/// 0, player 16) in the east, and two Persian players, player 15 (group 1)
/// in the north-west and player 14 (group 2) in the south-west, each group
/// with a yard (member 0) serving its members' request slots (brain
/// `+0x110`, `+0x138`, `+0x15c`, by member) in member order: the
/// barbarians' (0x46fb80 at (50, 32)) builds for free and rests `((50, 30,
/// 10) + rand(31)) x 25` ticks, the Persians' (0x470620 at (15, 12),
/// 0x471b50 at (4, 50)) pay and rest 500. The barbarians' member 1
/// (0x46fde0, the ships in (50, 27, 2, 2)) holds (50, 28), member 2
/// (0x46ff90, (48, 31)) fishes and gathers amber in (45, 18, 11, 19),
/// member 3 (0x470110, (53, 26, 3, 3)) patrols and raids. In the
/// north-west members 1 and 2 (0x4709c0, 0x470b10) gather amber and fish,
/// 3 (0x470c60, (10, 10, 3, 3)) patrols the home waters, 4 (0x470eb0, (22,
/// 12, 2, 2)) patrols the middle sea and then picks a target (for itself
/// or for member 3), 5 (0x475a70) mends prizes, 6 (0x471440) casts at the
/// player near player 15's temple and 7 (0x4718c0, (19, 15, 2, 2)) guards
/// the coast. In the south-west members 1 and 2 (0x471ef0, 0x472040)
/// gather, 3 and 4 (0x472190, 0x472370; (1, 46, 2, 2), (10, 49, 3, 2))
/// patrol, 5 (0x472550) gathers strength, patrols wider waters and casts a
/// spell on its ships when it fights, 6 (0x477190) mends and 7 (0x472890)
/// casts at the player near player 14's temple (as player 15).
/// (Not ported: the yards' code 0x34 (class 0x30, never asked for, no
/// effect); prizes of either Persian player handed to
/// group 1's repair member (0x5d97): here each player's prizes sail to its
/// own docks (`brain_prize`); the unreachable
/// states of members (1, 4) and (1, 7) (0x19, 0x28: nothing sends them
/// there). The casters look
/// through the player's things in the scene's order; a building is "in"
/// an area by its cell. A waterspout or sea monster asked for on land is
/// not cast.)
mod miss012 {
    use super::*;
    use crate::magic::Spell;
    use crate::objects::MapObject;
    use openconquest_formats::ObjectClass;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const RAMMER: u8 = 5;
    const KILLER: u8 = 6;
    /// The Persians' own ships (codes 0xd2, 0xd3).
    const PERSIAN_A: u8 = 10;
    const PERSIAN_B: u8 = 11;

    /// Fleet indices: the barbarians' members 0-3 are fleets 0-3, player
    /// 15's members 0-7 fleets 4-11, player 14's fleets 12-19.
    const B_YARD: usize = 0;
    const B_GUARD: usize = 1;
    const B_RAIDERS: usize = 3;
    const NORTH: usize = 4;
    const N_AMBER: usize = NORTH + 1;
    const N_FISH: usize = NORTH + 2;
    const N_PATROL: usize = NORTH + 3;
    const N_STRIKERS: usize = NORTH + 4;
    const N_REPAIR: usize = NORTH + 5;
    const N_CASTER: usize = NORTH + 6;
    const N_GUARD: usize = NORTH + 7;
    const SOUTH: usize = 12;
    const S_AMBER: usize = SOUTH + 1;
    const S_FISH: usize = SOUTH + 2;
    const S_PATROL: usize = SOUTH + 3;
    const S_PATROL2: usize = SOUTH + 4;
    const S_RAIDERS: usize = SOUTH + 5;
    const S_REPAIR: usize = SOUTH + 6;
    const S_CASTER: usize = SOUTH + 7;

    /// No member served (the original's -1).
    const NONE: u32 = u32::MAX;

    /// Fleet variables: 2 the ship asked for (type + 1, 0 none; a member's
    /// slot in its yard's requests). A yard: 0 its rest (barbarians), 3
    /// the member it serves. The barbarians' raiders (`+0x130`), member
    /// (1, 4) (`+0x184`) and member (2, 5) (`+0x198`): 0 their patrol's
    /// length; (2, 5) also 1 the spell it casts on its ships when it
    /// fights (`+0x1b0`). The raiders and members (1, 3) and (1, 4): 3 the
    /// size at which their strike gives up (as "fewer than" one more). The
    /// casters: 0 the tick their wait ends, 1 the spells they have cast
    /// (`+0x17c`, `+0x19c`). The repair members: 0 and 1 their group's
    /// player (nation, slot + 1; 0 unknown).
    const ASKED: usize = 2;
    const WAIT: usize = 0;
    const SERVING: usize = 3;
    const LENGTH: usize = 0;
    const SPELL: usize = 1;
    const FEW: usize = 3;
    const UNTIL: usize = 0;
    const CASTS: usize = 1;
    const NATION: usize = 0;
    const SLOT: usize = 1;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16).
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(50, 32, 1, 1)],
            program: barbarian_yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[(50, 27, 2, 2)],
            program: east_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(48, 31, 1, 1)],
            program: east_gatherers,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[(53, 26, 3, 3)],
            program: east_raiders,
            wants: &[],
        },
        // Player 15, the north-west.
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(15, 12, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[],
            program: north_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[],
            program: north_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[(10, 10, 3, 3)],
            program: north_patrol,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[(22, 12, 2, 2)],
            program: north_strikers,
            wants: &[],
        },
        // Mends prizes (0x475a70; see `brain_prize`).
        FleetDef {
            group: 1,
            member: 5,
            cells: &[],
            program: repair,
            wants: &[],
        },
        // Casts at the player near player 15's temple (0x471440; see
        // `turn`: it has no ships).
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 7,
            cells: &[(19, 15, 2, 2)],
            program: north_guard,
            wants: &[],
        },
        // Player 14, the south-west.
        FleetDef {
            group: 2,
            member: 0,
            cells: &[(4, 50, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 1,
            cells: &[],
            program: south_amber,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 2,
            cells: &[],
            program: south_fish,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 3,
            cells: &[(1, 46, 2, 2)],
            program: south_patrol,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 4,
            cells: &[(10, 49, 3, 2)],
            program: south_patrol2,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 5,
            cells: &[],
            program: south_raiders,
            wants: &[],
        },
        // Mends prizes (0x477190; see `brain_prize`).
        FleetDef {
            group: 2,
            member: 6,
            cells: &[],
            program: repair,
            wants: &[],
        },
        // Casts at the player near player 14's temple (0x472890; see
        // `turn`: it has no ships).
        FleetDef {
            group: 2,
            member: 7,
            cells: &[],
            program: idle,
            wants: &[],
        },
    ];

    /// The start (message 0x8d1): amber at (40, 0), (45, 0) and (53, 0)
    /// and fish at (52, 5) (0x48b180, 0x48ebc0, on every difficulty);
    /// player 15 gets 10000, 15000 or 25000 amber and 8000, 10000 or 15000
    /// fish by difficulty, player 14 15000, 15000 or 25000 and 10000,
    /// 10000 or 15000 (0x4a8de0 and 0x4a8c30 add).
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        // The Persians' studies at the start (the main's 0x8d1).
        scene.persian_bonus(&[15, 14], true, &[2, 3]);
        let level = difficulty();
        for x in [40, 45, 53] {
            scene.add_deposit(0x190, (x, 0, 1, 1), data, caches);
        }
        scene.add_deposit(0x191, (52, 5, 2, 2), data, caches);
        let purses = [
            (
                NORTH,
                N_REPAIR,
                [10_000, 15_000, 25_000],
                [8000, 10_000, 15_000],
            ),
            (
                SOUTH,
                S_REPAIR,
                [15_000, 15_000, 25_000],
                [10_000, 10_000, 15_000],
            ),
        ];
        for (first, holder, amber, fish) in purses {
            let owner = scene.brain.as_ref().unwrap().fleets[first..first + 8]
                .iter()
                .flat_map(|f| f.ships.clone())
                .find_map(|id| scene.object_index(id).and_then(|i| scene.objects[i].owner));
            if let Some(owner) = owner {
                scene.earn(owner, amber[level], fish[level]);
                set_var(scene, holder, NATION, owner.0);
                set_var(scene, holder, SLOT, owner.1 + 1);
            }
        }
        for k in [B_YARD, NORTH, SOUTH] {
            set_var(scene, k, SERVING, NONE);
        }
        set_var(scene, B_RAIDERS, FEW, 2);
        set_var(scene, N_PATROL, FEW, 1);
        set_var(scene, N_STRIKERS, FEW, 1);
    }

    /// Asks for ship `kind` in `holder`'s request slot if it is free.
    fn ask(scene: &mut Scene, holder: usize, kind: u8) {
        let f = &mut scene.brain.as_mut().unwrap().fleets[holder];
        if f.vars[ASKED] == 0 {
            f.vars[ASKED] = kind as u32 + 1;
        }
    }

    fn var(scene: &Scene, k: usize, v: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].vars[v]
    }

    fn set_var(scene: &mut Scene, k: usize, v: usize, value: u32) {
        scene.brain.as_mut().unwrap().fleets[k].vars[v] = value;
    }

    fn state(scene: &Scene, k: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].state
    }

    /// A Persian group's player, as the start found it (kept by the
    /// group's repair member).
    fn player(scene: &Scene, holder: usize) -> Option<(u32, u32)> {
        let slot = var(scene, holder, SLOT);
        (slot != 0).then(|| (var(scene, holder, NATION), slot - 1))
    }

    /// An object's cell: a building's own, else where it lies.
    fn cell_of(o: &MapObject) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        o.cell.unwrap_or(((o.x / t) as i32, (o.y / t) as i32))
    }

    fn inside((x, y): (i32, i32), (ax, ay, w, h): Cells) -> bool {
        x >= ax && x < ax + w && y >= ay && y < ay + h
    }

    /// The cells of the player's things, ships and buildings, in order
    /// (0x4a8620 on the human player).
    fn human_things(scene: &Scene) -> Vec<(i32, i32)> {
        scene
            .objects
            .iter()
            .filter(|o| scene.is_own(o) && o.combat.is_some())
            .map(cell_of)
            .collect()
    }

    /// The things of the player in `area` (0x479ed0 with masks 0xfff and
    /// 0xffffff).
    fn human_count(scene: &Scene, area: Cells) -> usize {
        human_things(scene)
            .into_iter()
            .filter(|&c| inside(c, area))
            .count()
    }

    /// The port's state for a fleet's sail to its post while short of
    /// strength (the barbarians' raiders and member (2, 5) call the move
    /// at state 5, checking their strength each turn).
    const MARCH: u32 = 105;

    /// The barbarians' raiders count their royal ships, bombers and
    /// rammers against the royal ships and rammers they ask for; member
    /// (2, 5) its royal ships and Persian ships against what it asks for.
    fn strong(scene: &Scene, k: usize) -> bool {
        let d = difficulty();
        let n = scene.fleet_types(k);
        let at = |kind: u8| n[kind as usize];
        if k == B_RAIDERS {
            at(ROYAL) + at(BOMBER) + at(RAMMER) >= 2 + [1, 2, 2][d]
        } else {
            at(ROYAL) + at(PERSIAN_A) + at(PERSIAN_B) >= 3 + [1, 2, 2][d] + [1, 1, 2][d]
        }
    }

    /// Player 15 holds at least 5000 amber and 4000 fish (0x4a8ce0, player
    /// `+0x2c`).
    fn rich(scene: &Scene) -> bool {
        player(scene, N_REPAIR).is_some_and(|p| {
            let (amber, fish) = scene.funds(p);
            amber >= 5000 && fish >= 4000
        })
    }

    /// Each member's bookkeeping on its turn (the start of each member's
    /// handler): what to ask its yard for, and the moves a member makes
    /// whatever its state.
    pub fn turn(scene: &mut Scene, k: usize) {
        let d = difficulty();
        let n = scene.fleet_types(k);
        let have = |kind: u8| n[kind as usize];
        let want = |scene: &mut Scene, kind: u8, min: [usize; 3]| {
            if have(kind) < min[d] {
                ask(scene, k, kind);
            }
        };
        match k {
            B_GUARD => {
                want(scene, ROYAL, [2, 2, 2]);
                want(scene, BOMBER, [1, 1, 2]);
            }
            B_RAIDERS | S_RAIDERS => {
                if k == B_RAIDERS {
                    want(scene, ROYAL, [2, 2, 2]);
                    want(scene, RAMMER, [1, 2, 2]);
                } else {
                    want(scene, ROYAL, [3, 3, 3]);
                    want(scene, PERSIAN_B, [1, 2, 2]);
                    want(scene, PERSIAN_A, [1, 1, 2]);
                }
                // Grown while sailing to its post: patrol instead.
                if state(scene, k) == MARCH && strong(scene, k) {
                    scene.set_state(k, 6);
                }
            }
            N_AMBER | N_FISH | S_AMBER | S_FISH => want(scene, MERCHANT, [2, 2, 2]),
            N_PATROL => {
                want(scene, ROYAL, [2, 2, 2]);
                want(scene, PERSIAN_A, [1, 1, 2]);
                want(scene, PERSIAN_B, [1, 2, 2]);
                // Its strike for member 4 lasts while player 15 is rich.
                if state(scene, k) == 26 && !rich(scene) {
                    scene.set_state(k, 0);
                }
            }
            N_STRIKERS => {
                want(scene, ROYAL, [0, 1, 2]);
                want(scene, PERSIAN_B, [2, 2, 2]);
                want(scene, PERSIAN_A, [2, 2, 2]);
            }
            N_GUARD => {
                want(scene, BOMBER, [2, 2, 2]);
                want(scene, PERSIAN_A, [1, 2, 2]);
                want(scene, PERSIAN_B, [1, 1, 2]);
            }
            S_PATROL => {
                want(scene, ROYAL, [2, 3, 3]);
                want(scene, PERSIAN_A, [1, 2, 2]);
                want(scene, PERSIAN_B, [1, 2, 3]);
            }
            S_PATROL2 => {
                want(scene, BOMBER, [2, 2, 2]);
                want(scene, PERSIAN_B, [0, 1, 2]);
                want(scene, PERSIAN_A, [2, 2, 2]);
            }
            N_CASTER => caster(scene, k, N_REPAIR),
            S_CASTER => caster(scene, k, S_REPAIR),
            _ => {}
        }
    }

    fn idle(_: u32) -> Op {
        Op::Custom(|scene, k| state(scene, k))
    }

    /// The barbarians' yard (0x46fb80): rolls its rest `((50, 30, 10) by
    /// difficulty + rand(31)) x 25` ticks if it has none, takes the first
    /// request of members 0-3 (0), builds it for that member for free
    /// (10), rests once it is launched (11), then clears the request (12).
    /// Codes other than merchants, royal ships, bombers, rammers and ship
    /// killers are dropped. (State 20, waiting for the launch, is the
    /// port's.)
    fn barbarian_yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, k| {
                if var(scene, k, WAIT) == 0 {
                    let rest = ([50, 30, 10][difficulty()] + scene.rng.next() % 31) * 25;
                    set_var(scene, k, WAIT, rest);
                }
                match (B_YARD..=B_RAIDERS).find(|&f| var(scene, f, ASKED) != 0) {
                    Some(f) => {
                        set_var(scene, k, SERVING, f as u32);
                        10
                    }
                    None => 0,
                }
            }),
            10 => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder == NONE {
                    return 0;
                }
                let kind = var(scene, holder as usize, ASKED) as u8 - 1;
                if !matches!(kind, MERCHANT | ROYAL | BOMBER | RAMMER | KILLER) {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    set_var(scene, k, WAIT, 0);
                    return 10;
                }
                if scene.fleet_build_for(k, holder as usize, kind) {
                    20
                } else {
                    10
                }
            }),
            20 => Op::Custom(|scene, k| if scene.fleet_building(k) { 20 } else { 11 }),
            11 => Op::WaitVar {
                next: 12,
                var: WAIT,
            },
            _ => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder != NONE {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    set_var(scene, k, WAIT, 0);
                }
                0
            }),
        }
    }

    /// A Persian yard (0x470620, 0x471b50): (its order of class 1 at 0: the full town, see `Job`) takes the first request of its group's members 0-7 (5),
    /// builds it for that member (10), rests 500 ticks once it is launched
    /// (11), then clears it (12). (State 20, waiting for the launch, is
    /// the port's.)
    fn persian_yard(state: u32) -> Op {
        match state {
            // The full-town order before every ship (0x47b130 code 1).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_complete(k) {
                    5
                } else {
                    scene.want_full_town(k);
                    0
                }
            }),
            5 => Op::Custom(
                |scene, k| match (k..k + 8).find(|&f| var(scene, f, ASKED) != 0) {
                    Some(f) => {
                        set_var(scene, k, SERVING, f as u32);
                        10
                    }
                    None => 5,
                },
            ),
            10 => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder == NONE {
                    return 0;
                }
                let kind = var(scene, holder as usize, ASKED) as u8 - 1;
                if !matches!(kind, 0..=6 | PERSIAN_A | PERSIAN_B) {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    return 10;
                }
                if scene.fleet_build_for(k, holder as usize, kind) {
                    20
                } else {
                    10
                }
            }),
            20 => Op::Custom(|scene, k| if scene.fleet_building(k) { 20 } else { 11 }),
            11 => Op::Wait {
                next: 12,
                ticks: 500,
            },
            _ => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder != NONE {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                }
                0
            }),
        }
    }

    const fn zone(area: Cells) -> Event {
        Event::Zone { state: 20, area }
    }

    const fn attacked(after: u32) -> Event {
        Event::Attacked { state: 30, after }
    }

    /// Fewer ships than one more than the fleet's variable 3 (0x47a1e0,
    /// "fewer than"): back to 0.
    const FEWER: Event = Event::Size {
        state: 0,
        after: None,
        cmp: Cmp::AtMost,
        n: FEW,
    };

    /// A fleet's fights: the zone seen 1500 ticks (20), whoever fired on
    /// it `answer` ticks (30).
    fn fight(state: u32, answer: u32) -> Op {
        match state {
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: answer,
                events: &[],
            },
        }
    }

    /// The barbarians' waters (event data 0x5b4228 and on).
    const EAST: Cells = (43, 21, 13, 19);

    /// The barbarians' guard (member 1, 0x46fde0; the ships in (50, 27,
    /// 2, 2)): sails to (50, 28) every 750 ticks watching their waters
    /// (events 0x5b42a8); the player there is fought 1500 ticks, whoever
    /// fires on it 250. It asks for royal ships (2) and bombers (1, 1, 2).
    fn east_guard(state: u32) -> Op {
        const W: &[Event] = &[zone(EAST), attacked(375)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 0,
                failed: 0,
                to: (50, 28),
                ticks: 750,
                events: W,
            },
            _ => fight(state, 250),
        }
    }

    /// The barbarians' gatherers (member 2, 0x46ff90; the ships at (48,
    /// 31)): fish (45, 18, 11, 19) 4500 ticks, home to (47, 33) (750 at
    /// most), rest 3000; then amber the same.
    fn east_gatherers(state: u32) -> Op {
        const AREA: Cells = (45, 18, 11, 19);
        let home = |next| Op::Move {
            ok: next,
            failed: next,
            to: (47, 33),
            ticks: 750,
            events: &[],
        };
        match state {
            0 => Op::Behave { next: 5 },
            5 | 11 => Op::Gather {
                next: state + 2,
                area: AREA,
                resource: if state == 5 {
                    Resource::Fish
                } else {
                    Resource::Amber
                },
                ticks: 4500,
                events: &[],
            },
            7 | 13 => home(state + 2),
            9 => Op::Wait {
                next: 11,
                ticks: 3000,
            },
            _ => Op::Wait {
                next: 0,
                ticks: 3000,
            },
        }
    }

    /// Rolls a patrol's length `(rand(4) + base by difficulty) x 1500`.
    fn roll_length(scene: &mut Scene, k: usize, base: [u32; 3]) -> u32 {
        let ticks = ((scene.rng.next() & 3) + base[difficulty()]) * 1500;
        set_var(scene, k, LENGTH, ticks);
        5
    }

    /// The barbarians' raiders (member 3, 0x470110; the ships in (53, 26,
    /// 3, 3)): roll their patrol's length `(rand(4) + (10, 8, 6)) x 1500`
    /// (`+0x130`; 0); short of strength they sail to (54, 26) watching
    /// their waters (events 0x5b43f8), at strength patrol (0x5b43e0)
    /// watching (45, 11, 11, 10) too (0x5b4410), then strike 3000 ticks
    /// (10, 15; events 0x5b4428, until fewer than three are left): at (37,
    /// 0, 19, 10) if the player has something there, else about one of
    /// the player's settlements or towns in (0, 0, 56, 52), else about the
    /// first of the player's things. They answer fire 250 ticks and ask
    /// for royal ships (2) and rammers (1, 2, 2).
    fn east_raiders(state: u32) -> Op {
        const MARCHING: &[Event] = &[zone(EAST), attacked(375)];
        const W: &[Event] = &[zone(EAST), zone((45, 11, 11, 10)), attacked(375)];
        const STRIKING: &[Event] = &[zone(EAST), FEWER];
        const ROUND: &[(i32, i32, u32)] = &[(51, 14, 500), (53, 20, 750)];
        match state {
            0 => Op::Custom(|scene, k| roll_length(scene, k, [10, 8, 6])),
            5 => Op::Custom(|scene, k| if strong(scene, k) { 6 } else { MARCH }),
            MARCH => Op::Move {
                ok: 0,
                failed: 0,
                to: (54, 26),
                ticks: 0,
                events: MARCHING,
            },
            6 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Custom(|scene, k| {
                const NORTH_EAST: Cells = (37, 0, 19, 10);
                if human_count(scene, NORTH_EAST) > 0 {
                    scene.set_fleet_area(k, NORTH_EAST);
                    return 15;
                }
                if let Some(a) = scene.human_site_area(Some((0, 0, 56, 52))) {
                    scene.set_fleet_area(k, a);
                    return 15;
                }
                match human_things(scene).first() {
                    Some(&cell) => {
                        scene.set_fleet_area(k, scene.cells_about(cell, 2));
                        15
                    }
                    None => 0,
                }
            }),
            15 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 3000,
                events: STRIKING,
            },
            _ => fight(state, 250),
        }
    }

    /// A Persian gatherer: asks for merchants while it has fewer than two,
    /// gathers `ticks`, sails home (750 at most) and rests.
    fn gatherer(
        state: u32,
        area: Cells,
        resource: Resource,
        home: (i32, i32),
        ticks: u32,
        rest: u32,
    ) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area,
                resource,
                ticks,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: home,
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: rest,
            },
        }
    }

    /// The north-west's waters (event data 0x5b4440 and on).
    const HOME: Cells = (8, 5, 16, 15);

    /// Member (1, 1) (0x4709c0): amber in (8, 5, 16, 15) 4500 ticks, home
    /// to (10, 16), rest 1500.
    fn north_amber(state: u32) -> Op {
        gatherer(state, HOME, Resource::Amber, (10, 16), 4500, 1500)
    }

    /// Member (1, 2) (0x470b10): fish there the same.
    fn north_fish(state: u32) -> Op {
        gatherer(state, HOME, Resource::Fish, (10, 16), 4500, 1500)
    }

    /// The north-west's patrol (member (1, 3), 0x470c60; the ships in (10,
    /// 10, 3, 3)): patrols (0x5b44b0) watching (8, 5, 16, 15) and (10, 0,
    /// 14, 6) (events 0x5b4530; the player there fought 1500 ticks, 20 and
    /// 40), answering fire 375. Member 4 may send it to strike (25):
    /// while player 15 holds 5000 amber and 4000 fish, at the area member
    /// 4 saw 4500 ticks (26; events 0x5b4548, until fewer than two are
    /// left), else back to 0; or (35, 0x23, a state it has no step for)
    /// to stay as it is for good. It asks for royal ships (2) and Persian
    /// ships (1, 1, 2 and 1, 2, 2).
    fn north_patrol(state: u32) -> Op {
        const FAR: Event = Event::Zone {
            state: 40,
            area: (10, 0, 14, 6),
        };
        const W: &[Event] = &[zone(HOME), FAR, attacked(375)];
        const STRIKING: &[Event] = &[zone(HOME), FAR, FEWER];
        const ROUND: &[(i32, i32, u32)] = &[(8, 13, 1500), (15, 7, 1500)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            20 | 40 => fight(20, 375),
            25 => Op::Custom(|scene, _| if rich(scene) { 26 } else { 0 }),
            26 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: STRIKING,
            },
            35 => Op::Custom(|_, _| 35),
            _ => fight(state, 375),
        }
    }

    /// The north-west's strikers (member (1, 4), 0x470eb0; the ships in
    /// (22, 12, 2, 2)): roll their patrol's length `(rand(4) + (10, 8, 6))
    /// x 1500` (`+0x184`; 0), patrol the middle sea (0x5b4560) watching (8,
    /// 5, 16, 15), (24, 8, 9, 4), (24, 12, 14, 15) and (38, 15, 6, 10)
    /// (events 0x5b4708; the player there fought 1500 ticks, whoever fires
    /// on them 375, then back to what they did), then look (10): about the
    /// last of the player's royal ships carrying heroes (ship `+0x259`) or
    /// a lasting gift (treasure kinds 10-14, 0x4b9740 group 0x10) they
    /// strike 4500 ticks (35; events 0x5b4720, until fewer than two are
    /// left); else, the player having something in (35, 0, 15, 9), they
    /// send member 3 to strike there (state 0x19), or, the player having a
    /// settlement or town in (0, 0, 50, 72), they send member 3 to state
    /// 0x23 (which it has no step for) about one picked at random; and
    /// start over. They ask for royal ships (0, 1, 2) and two of each
    /// Persian ship.
    fn north_strikers(state: u32) -> Op {
        const W: &[Event] = &[
            zone(HOME),
            zone((24, 8, 9, 4)),
            zone((24, 12, 14, 15)),
            zone((38, 15, 6, 10)),
            attacked(375),
        ];
        const HUNTING: &[Event] = &[zone(HOME), FEWER];
        const ROUND: &[(i32, i32, u32)] = &[
            (26, 11, 1500),
            (34, 19, 1500),
            (25, 20, 1500),
            (34, 19, 1500),
        ];
        match state {
            0 => Op::Custom(|scene, k| roll_length(scene, k, [10, 8, 6])),
            5 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Custom(|scene, k| {
                let carrier = scene
                    .objects
                    .iter()
                    .filter(|o| {
                        scene.is_own(o)
                            && o.ship.as_ref().is_some_and(|s| {
                                s.kind == ROYAL
                                    && (!s.heroes.is_empty()
                                        || s.treasures.iter().any(|t| (10..=14).contains(&t.kind)))
                            })
                    })
                    .map(cell_of)
                    .last();
                if let Some(cell) = carrier {
                    scene.set_fleet_area(k, scene.cells_about(cell, 2));
                    return 35;
                }
                const NORTH_EAST: Cells = (35, 0, 15, 9);
                if human_count(scene, NORTH_EAST) > 0 {
                    scene.set_fleet_area(N_PATROL, NORTH_EAST);
                    scene.set_state(N_PATROL, 25);
                } else if let Some(a) = scene.human_site_area(Some((0, 0, 50, 72))) {
                    scene.set_fleet_area(N_PATROL, a);
                    scene.set_state(N_PATROL, 35);
                }
                0
            }),
            20 => fight(20, 375),
            35 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: HUNTING,
            },
            _ => Op::Attack {
                next: PREVIOUS,
                failed: PREVIOUS,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    /// The north-west's coast guard (member (1, 7), 0x4718c0; the ships in
    /// (19, 15, 2, 2)): patrols (0x5b47d0) watching (8, 5, 16, 15) (events
    /// 0x5b47b8), the player there fought 1500 ticks, whoever fires on it
    /// 375. It asks for bombers (2) and Persian ships (1, 2, 2 and 1, 1,
    /// 2).
    fn north_guard(state: u32) -> Op {
        const W: &[Event] = &[zone(HOME), attacked(375)];
        const ROUND: &[(i32, i32, u32)] = &[(16, 19, 1500), (20, 15, 1500)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            _ => fight(state, 375),
        }
    }

    /// The south-west's waters (event data 0x5b47e8 and on).
    const SOUTH_WATERS: Cells = (0, 40, 19, 12);

    /// Member (2, 1) (0x471ef0): amber in (0, 41, 15, 11) 6000 ticks, home
    /// to (0, 50), rest 3000.
    fn south_amber(state: u32) -> Op {
        gatherer(state, (0, 41, 15, 11), Resource::Amber, (0, 50), 6000, 3000)
    }

    /// Member (2, 2) (0x472040): fish there the same.
    fn south_fish(state: u32) -> Op {
        gatherer(state, (0, 41, 15, 11), Resource::Fish, (0, 50), 6000, 3000)
    }

    /// A patrol of the south-west (members (2, 3) and (2, 4)): patrols its
    /// round watching (0, 40, 19, 12), the player there fought 1500 ticks,
    /// whoever fires on it 375.
    fn south_round(state: u32, round: &'static [(i32, i32, u32)]) -> Op {
        const W: &[Event] = &[zone(SOUTH_WATERS), attacked(375)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: round,
                ticks: 0,
                events: W,
            },
            _ => fight(state, 375),
        }
    }

    /// Member (2, 3) (0x472190; the ships in (1, 46, 2, 2); round 0x5b4830,
    /// events 0x5b4880). It asks for royal ships (2, 3, 3) and Persian
    /// ships (1, 2, 2 and 1, 2, 3).
    fn south_patrol(state: u32) -> Op {
        south_round(state, &[(1, 42, 1500), (6, 45, 1500)])
    }

    /// Member (2, 4) (0x472370; the ships in (10, 49, 3, 2); round
    /// 0x5b4898, events 0x5b4940). It asks for bombers (2) and Persian
    /// ships (0, 1, 2 and 2).
    fn south_patrol2(state: u32) -> Op {
        south_round(state, &[(9, 43, 1500), (14, 46, 1500), (11, 50, 1500)])
    }

    /// The south-west's raiders (member (2, 5), 0x472550): roll `r =
    /// rand(4)`, their spell (`+0x1b0`: 0-1 the Flame, paid; 2 the
    /// phantom; 3 the weapon shield) and their patrol's length `((20, 15,
    /// 10) + r) x 1500` (`+0x198`, the table `+0x1b8` picks, set at the
    /// start; 0); short of strength they sail to (8, 46), at strength
    /// patrol wider waters (0x5b4958); both watching (0, 40, 19, 12) (the
    /// player there: the spell on their ships (0x47acf0 by member 0), then
    /// a fight of 1500 ticks; 20, 25), (1, 34, 21, 7) and (19, 39, 6, 13)
    /// (state 0x1e, 30: 375 ticks on `last_area`, the area of their last
    /// attack or of the last attacker that fired the attacked event, not
    /// the zone) (events 0x5b4ac8), and answer fire 375 (30 too). They
    /// ask for royal ships (3) and Persian ships (1, 2, 2 and 1, 1, 2).
    fn south_raiders(state: u32) -> Op {
        const W: &[Event] = &[
            zone(SOUTH_WATERS),
            Event::Zone {
                state: 30,
                area: (1, 34, 21, 7),
            },
            Event::Zone {
                state: 30,
                area: (19, 39, 6, 13),
            },
            attacked(375),
        ];
        const ROUND: &[(i32, i32, u32)] = &[
            (3, 37, 1500),
            (12, 37, 1500),
            (20, 39, 1500),
            (20, 48, 1500),
        ];
        match state {
            0 => Op::Custom(|scene, k| {
                let r = scene.rng.next() & 3;
                set_var(scene, k, SPELL, [9, 9, 12, 15][r as usize]);
                let ticks = ([20, 15, 10][difficulty()] + r) * 1500;
                set_var(scene, k, LENGTH, ticks);
                5
            }),
            5 => Op::Custom(|scene, k| if strong(scene, k) { 6 } else { MARCH }),
            MARCH => Op::Move {
                ok: 0,
                failed: 0,
                to: (8, 46),
                ticks: 0,
                events: W,
            },
            6 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            20 => Op::Custom(|scene, k| {
                // Through the war temple (0x47acf0, 0x42f470): the Flame
                // credited one cast (pay 1), the shields not.
                let spell = match var(scene, k, SPELL) {
                    9 => Some((Spell::Flame, true)),
                    12 => Some((Spell::Phantom, false)),
                    15 => Some((Spell::ProtectWeapons, false)),
                    _ => None,
                };
                if let Some((spell, pay)) = spell {
                    let ships = scene.fleet_ships(k);
                    scene.brain_cast_fleet(ships, spell, pay);
                }
                25
            }),
            25 => fight(20, 375),
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::LastArea,
                ticks: 375,
                events: &[],
            },
            _ => fight(state, 375),
        }
    }

    /// Members (1, 5) and (2, 6) (0x475a70, 0x477190): send what they are
    /// handed to the docks.
    fn repair(_: u32) -> Op {
        Op::Custom(|scene, k| {
            scene.fleet_to_docks(k);
            0
        })
    }

    /// A caster (members (1, 6) and (2, 7), 0x471440 and 0x472890), run
    /// from `turn` as it has no ships: waits `(3, 2, 1) by difficulty x
    /// 1500` ticks (10), then, if it has cast fewer than (1, 2, 3) spells,
    /// looks through the player's things within 15 cells of the first
    /// temple (class 0x3e) of the player `holder` keeps (15) and casts one
    /// spell (0x47ae10 by member (1, 0), as player 15, for nothing) at the
    /// first it picks (see `look_and_cast`), and starts over; past its
    /// spells it stays.
    fn caster(scene: &mut Scene, k: usize, holder: usize) {
        let d = difficulty();
        match state(scene, k) {
            0 => scene.set_state(k, 10),
            10 => {
                let now = scene.brain.as_ref().unwrap().tick;
                let until = var(scene, k, UNTIL);
                if until == 0 {
                    set_var(scene, k, UNTIL, now + [3, 2, 1][d] * 1500);
                } else if now >= until {
                    set_var(scene, k, UNTIL, 0);
                    scene.set_state(k, 15);
                }
            }
            15 if var(scene, k, CASTS) < [1, 2, 3][d] => {
                look_and_cast(scene, k, holder);
                scene.set_state(k, 0);
            }
            _ => {}
        }
    }

    /// The caster's pick: a fireball at a settlement one time in two;
    /// lightning at a town or temple (0x3e); a waterspout or fireball, one
    /// time in four each, at a temple (0x3d), fort (0x36) or building of
    /// class 0x34, 0x35; at anything else with at least four of the
    /// player's things within 3 cells, a waterspout, fireball or sea
    /// monster one time in (20, 15, 10) + 1 each. A roll that picks
    /// nothing ends the look.
    fn look_and_cast(scene: &mut Scene, k: usize, holder: usize) {
        let d = difficulty();
        let Some(owner) = player(scene, holder) else {
            return;
        };
        let caster = player(scene, N_REPAIR);
        let Some(temple) = scene
            .objects
            .iter()
            .find(|o| o.owner == Some(owner) && o.class == ObjectClass::Building(0x3e))
            .map(cell_of)
        else {
            return;
        };
        let area = scene.cells_about(temple, 15);
        let things: Vec<(ObjectClass, (i32, i32))> = scene
            .objects
            .iter()
            .filter(|o| scene.is_own(o) && o.combat.is_some())
            .map(|o| (o.class, cell_of(o)))
            .collect();
        for (class, cell) in things {
            if !inside(cell, area) {
                continue;
            }
            let (spell, at) = match class {
                ObjectClass::Building(0x32) => match scene.rng.next() & 3 {
                    0 | 2 => (Spell::Fireball, cell),
                    _ => break,
                },
                ObjectClass::Building(0x33 | 0x3e) => (Spell::Lightning, cell),
                ObjectClass::Building(0x34 | 0x35 | 0x36 | 0x3d) => match scene.rng.next() & 3 {
                    0 => (Spell::Waterspout, cell),
                    2 => (Spell::Fireball, cell),
                    _ => break,
                },
                _ => {
                    let roll = scene.rng.next() % ([20, 15, 10][d] + 1);
                    let near = scene.cells_about(cell, 3);
                    let at = (near.0 + near.2 / 2, near.1 + near.3 / 2);
                    if human_count(scene, near) < 4 {
                        break;
                    }
                    match roll {
                        0 => (Spell::Waterspout, at),
                        4 => (Spell::Fireball, at),
                        8 => (Spell::Monster, at),
                        _ => break,
                    }
                }
            };
            scene.pending_casts.push((spell, at.0, at.1, caster));
            let casts = var(scene, k, CASTS);
            set_var(scene, k, CASTS, casts + 1);
            break;
        }
    }
}

/// miss013's brain (0x477930; its members' handlers precede it,
/// 0x4741b0-0x477210, with the repair handlers 0x475a70 and 0x477190 it
/// shares with miss012; 0x473f40 is its judge). Three computer players:
/// the barbarians (group 0, player 16) in the north-west corner, player 15
/// (group 1) in the middle channel and player 14 (group 2) in the south,
/// each group with a yard (member 0) serving its members' request slots
/// (brain `+0x10c`, `+0x134`, `+0x154`, by member) in member order: the
/// barbarians' (0x4741b0 at (2, 4)) builds for free and rests `((50, 30,
/// 10) + rand(31)) x 25` ticks, the Persians' (0x474be0 at (14, 28),
/// 0x475f70 at (44, 59)) pay and rest 500. The barbarians' member 1
/// (0x474410) holds (8, 5), member 2 (0x4745c0, the ships at (6, 11))
/// fishes and gathers amber in the corner, member 3 (0x474740, (8, 7, 3,
/// 3)) patrols and raids. In the middle members 1 and 2 (0x474f80,
/// 0x4750d0) gather amber and fish, 3 (0x475220, (20, 26, 5, 3)) guards
/// the channel and strikes when member 4 sends it, 4 (0x4754c0, (17, 20,
/// 3, 4)) patrols the channel and then picks a target (for itself or for
/// member 3), 5 (0x475a70) mends prizes and 6 (0x475af0) casts at the
/// player near player 15's temple. In the south members 1 and 2 (0x476310,
/// 0x476460) gather, 3 (0x4765b0, (39, 56, 4, 3)) patrols the home waters
/// and spots the player's ships coming south, 4 (0x4768e0, (38, 61, 2, 3))
/// gathers strength, patrols the southern sea and strikes royal ships
/// carrying heroes, 5 (0x476dc0, (38, 65, 3, 2)) gathers strength, patrols
/// the south-west and strikes there under a spell, 6 (0x477190) mends and
/// 7 (0x477210) casts at the player near player 14's temple (as player
/// 15).
/// (Not ported: the yards' code 0x34 (class 0x30, never asked for, no
/// effect); prizes handed to member (1, 5) (player
/// 15's) or (1, 6) (player 14's: the caster, which never moves them)
/// (0x5d97): here each player's prizes sail to its own docks
/// (`brain_prize`); member (1, 4)'s state 0x19
/// (a sail to (42, 21)), which nothing sends it to. Member (1, 4)'s fights
/// look their zone up in a shorter table than the one its patrol watches
/// (0x5b5138 against 0x5b51f0): the player seen in (0, 18, 8, 8) sends it
/// back to 0 (an attack on an empty area), in (4, 17, 22, 7) to fight at
/// (20, 7, 18, 6), as the misread record says. The areas about one of
/// the player's settlements or towns are `human_site_area`'s. The brain
/// looks through the player's
/// things in the scene's order; a building is "in" an area by its cell. A
/// waterspout or sea monster asked for on land is not cast. A strike
/// started from a step of the port's own (member (1, 3)'s at 0x23) starts
/// a turn later.)
mod miss013 {
    use super::*;
    use crate::magic::Spell;
    use crate::objects::MapObject;
    use openconquest_formats::ObjectClass;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;
    const KILLER: u8 = 6;
    /// The Persians' own ships (codes 0xd2, 0xd3).
    const PERSIAN_A: u8 = 10;
    const PERSIAN_B: u8 = 11;

    /// Fleet indices: the barbarians' members 0-3 are fleets 0-3, player
    /// 15's members 0-6 fleets 4-10, player 14's members 0-7 fleets 11-18.
    const B_YARD: usize = 0;
    const B_GUARD: usize = 1;
    const B_RAIDERS: usize = 3;
    const MIDDLE: usize = 4;
    const M_AMBER: usize = MIDDLE + 1;
    const M_FISH: usize = MIDDLE + 2;
    const M_GUARD: usize = MIDDLE + 3;
    const M_STRIKERS: usize = MIDDLE + 4;
    const M_REPAIR: usize = MIDDLE + 5;
    const M_CASTER: usize = MIDDLE + 6;
    const SOUTH: usize = 11;
    const S_AMBER: usize = SOUTH + 1;
    const S_FISH: usize = SOUTH + 2;
    const S_PATROL: usize = SOUTH + 3;
    const S_RAIDERS: usize = SOUTH + 4;
    const S_STRIKERS: usize = SOUTH + 5;
    const S_REPAIR: usize = SOUTH + 6;
    const S_CASTER: usize = SOUTH + 7;

    /// No member served (the original's -1).
    const NONE: u32 = u32::MAX;

    /// Fleet variables: 2 the ship asked for (type + 1, 0 none; a member's
    /// slot in its yard's requests). A yard: 0 its rest (barbarians), 3
    /// the member it serves; player 15's yard also keeps 1 the area its
    /// members 3 and 4 strike (`+0x180`, packed by `pack`), player 14's 0
    /// whether member 3 has sent members 4 and 5 after a ship it spotted
    /// (`+0x1cc`). The patrols that roll their length (members (0, 3),
    /// (1, 4), (2, 3), (2, 4), (2, 5); `+0x12c`, `+0x17c`, `+0x190`,
    /// `+0x194`, `+0x198`): 0 that length; (2, 5) also 1 the spell it
    /// casts on its ships when it strikes (`+0x1b0`). The members whose
    /// strike gives up when the fleet shrinks: 3 that size (as "fewer
    /// than" one more). The casters: 0 the tick their wait ends, 1 the
    /// spells they have cast (`+0x174`, `+0x19c`). The repair members: 0
    /// and 1 their group's player (nation, slot + 1; 0 unknown).
    const ASKED: usize = 2;
    const WAIT: usize = 0;
    const SERVING: usize = 3;
    const STRIKE_AREA: usize = 1;
    const SPOTTED: usize = 0;
    const LENGTH: usize = 0;
    const SPELL: usize = 1;
    const FEW: usize = 3;
    const UNTIL: usize = 0;
    const CASTS: usize = 1;
    const NATION: usize = 0;
    const SLOT: usize = 1;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16), the north-west corner.
        FleetDef {
            group: 0,
            member: 0,
            cells: &[(2, 4, 1, 1)],
            program: barbarian_yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 1,
            cells: &[],
            program: corner_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 2,
            cells: &[(6, 11, 1, 1)],
            program: corner_gatherers,
            wants: &[],
        },
        FleetDef {
            group: 0,
            member: 3,
            cells: &[(8, 7, 3, 3)],
            program: corner_raiders,
            wants: &[],
        },
        // Player 15, the middle channel.
        FleetDef {
            group: 1,
            member: 0,
            cells: &[(14, 28, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 1,
            cells: &[],
            program: middle_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 2,
            cells: &[],
            program: middle_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 3,
            cells: &[(20, 26, 5, 3)],
            program: middle_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            member: 4,
            cells: &[(17, 20, 3, 4)],
            program: middle_strikers,
            wants: &[],
        },
        // Mends prizes (0x475a70; see `brain_prize`).
        FleetDef {
            group: 1,
            member: 5,
            cells: &[],
            program: repair,
            wants: &[],
        },
        // Casts at the player near player 15's temple (0x475af0; see
        // `turn`: it has no ships).
        FleetDef {
            group: 1,
            member: 6,
            cells: &[],
            program: idle,
            wants: &[],
        },
        // Player 14, the south.
        FleetDef {
            group: 2,
            member: 0,
            cells: &[(44, 59, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 1,
            cells: &[],
            program: south_amber,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 2,
            cells: &[],
            program: south_fish,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 3,
            cells: &[(39, 56, 4, 3)],
            program: south_patrol,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 4,
            cells: &[(38, 61, 2, 3)],
            program: south_raiders,
            wants: &[],
        },
        FleetDef {
            group: 2,
            member: 5,
            cells: &[(38, 65, 3, 2)],
            program: south_strikers,
            wants: &[],
        },
        // Mends prizes (0x477190; see `brain_prize`).
        FleetDef {
            group: 2,
            member: 6,
            cells: &[],
            program: repair,
            wants: &[],
        },
        // Casts at the player near player 14's temple (0x477210; see
        // `turn`: it has no ships).
        FleetDef {
            group: 2,
            member: 7,
            cells: &[],
            program: idle,
            wants: &[],
        },
    ];

    /// The start (message 0x8d1): amber at (47, 0), (49, 1) and (38, 0)
    /// and fish at (36, 1) (0x48b180, 0x48ebc0, on every difficulty);
    /// player 15 gets 10000, 15000 or 25000 amber and 8000, 10000 or 15000
    /// fish by difficulty, player 14 15000, 15000 or 25000 and 10000,
    /// 10000 or 15000 (0x4a8de0 and 0x4a8c30 add).
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        // The Persians' studies at the start (the main's 0x8d1).
        scene.persian_bonus(&[15, 14], true, &[2, 3]);
        let level = difficulty();
        for (x, y) in [(47, 0), (49, 1), (38, 0)] {
            scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
        }
        scene.add_deposit(0x191, (36, 1, 2, 2), data, caches);
        let purses = [
            (
                MIDDLE..SOUTH,
                M_REPAIR,
                [10_000, 15_000, 25_000],
                [8000, 10_000, 15_000],
            ),
            (
                SOUTH..S_CASTER + 1,
                S_REPAIR,
                [15_000, 15_000, 25_000],
                [10_000, 10_000, 15_000],
            ),
        ];
        for (members, holder, amber, fish) in purses {
            let owner = scene.brain.as_ref().unwrap().fleets[members]
                .iter()
                .flat_map(|f| f.ships.clone())
                .find_map(|id| scene.object_index(id).and_then(|i| scene.objects[i].owner));
            if let Some(owner) = owner {
                scene.earn(owner, amber[level], fish[level]);
                set_var(scene, holder, NATION, owner.0);
                set_var(scene, holder, SLOT, owner.1 + 1);
            }
        }
        for k in [B_YARD, MIDDLE, SOUTH] {
            set_var(scene, k, SERVING, NONE);
        }
        // "Fewer than 2" (events 0x5b4fa0, 0x5b50f0) and "fewer than 3"
        // (0x5b5380, 0x5b5708, 0x5b5918).
        set_var(scene, B_RAIDERS, FEW, 1);
        set_var(scene, M_GUARD, FEW, 1);
        for k in [M_STRIKERS, S_RAIDERS, S_STRIKERS] {
            set_var(scene, k, FEW, 2);
        }
    }

    /// Asks for ship `kind` in `holder`'s request slot if it is free.
    fn ask(scene: &mut Scene, holder: usize, kind: u8) {
        let f = &mut scene.brain.as_mut().unwrap().fleets[holder];
        if f.vars[ASKED] == 0 {
            f.vars[ASKED] = kind as u32 + 1;
        }
    }

    fn var(scene: &Scene, k: usize, v: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].vars[v]
    }

    fn set_var(scene: &mut Scene, k: usize, v: usize, value: u32) {
        scene.brain.as_mut().unwrap().fleets[k].vars[v] = value;
    }

    fn state(scene: &Scene, k: usize) -> u32 {
        scene.brain.as_ref().unwrap().fleets[k].state
    }

    /// An area in one fleet variable (cells within -128..128).
    fn pack((x, y, w, h): Cells) -> u32 {
        u32::from_le_bytes([x as u8, y as u8, w as u8, h as u8])
    }

    fn unpack(v: u32) -> Cells {
        let [x, y, w, h] = v.to_le_bytes().map(|b| b as i8 as i32);
        (x, y, w, h)
    }

    /// A Persian group's player, as the start found it (kept by the
    /// group's repair member).
    fn player(scene: &Scene, holder: usize) -> Option<(u32, u32)> {
        let slot = var(scene, holder, SLOT);
        (slot != 0).then(|| (var(scene, holder, NATION), slot - 1))
    }

    /// An object's cell: a building's own, else where it lies.
    fn cell_of(o: &MapObject) -> (i32, i32) {
        let t = TILE_SIZE as f32;
        o.cell.unwrap_or(((o.x / t) as i32, (o.y / t) as i32))
    }

    fn inside((x, y): (i32, i32), (ax, ay, w, h): Cells) -> bool {
        x >= ax && x < ax + w && y >= ay && y < ay + h
    }

    /// The cells of the player's things, ships and buildings, in order
    /// (0x4a8620 on the human player).
    fn human_things(scene: &Scene) -> Vec<(i32, i32)> {
        scene
            .objects
            .iter()
            .filter(|o| scene.is_own(o) && o.combat.is_some())
            .map(cell_of)
            .collect()
    }

    /// The things of the player in `area` (0x479ed0 with masks 0xfff and
    /// 0xffffff).
    fn human_count(scene: &Scene, area: Cells) -> usize {
        human_things(scene)
            .into_iter()
            .filter(|&c| inside(c, area))
            .count()
    }

    /// The port's state for a fleet's sail to its post while short of
    /// strength (members (0, 3), (2, 4) and (2, 5) call the move at state
    /// 5, checking their strength each turn).
    const MARCH: u32 = 105;

    /// A fleet counts the ships it asks for against the numbers it asks:
    /// member (0, 3) royal ships, bombers and rammers against 2 + 1;
    /// (2, 4) boarders, royal ships and both Persian ships against (0, 1,
    /// 2) + 0 + 2 + 3; (2, 5) royal ships and both Persian ships against 3
    /// + (1, 1, 2) + 1.
    fn strong(scene: &Scene, k: usize) -> bool {
        let d = difficulty();
        let n = scene.fleet_types(k);
        let at = |kind: u8| n[kind as usize];
        match k {
            B_RAIDERS => at(ROYAL) + at(BOMBER) + at(RAMMER) >= 3,
            S_RAIDERS => {
                at(BOARDER) + at(ROYAL) + at(PERSIAN_B) + at(PERSIAN_A) >= [0, 1, 2][d] + 2 + 3
            }
            _ => at(ROYAL) + at(PERSIAN_B) + at(PERSIAN_A) >= [5, 5, 6][d],
        }
    }

    /// Player 15 holds at least 5000 amber and 4000 fish (0x4a8ce0, player
    /// `+0x2c`).
    fn rich(scene: &Scene) -> bool {
        player(scene, M_REPAIR).is_some_and(|p| {
            let (amber, fish) = scene.funds(p);
            amber >= 5000 && fish >= 4000
        })
    }

    /// Each member's bookkeeping on its turn (the start of each member's
    /// handler): what to ask its yard for, and the moves a member makes
    /// whatever its state.
    pub fn turn(scene: &mut Scene, k: usize) {
        let d = difficulty();
        let n = scene.fleet_types(k);
        let have = |kind: u8| n[kind as usize];
        let want = |scene: &mut Scene, kind: u8, min: [usize; 3]| {
            if have(kind) < min[d] {
                ask(scene, k, kind);
            }
        };
        match k {
            B_GUARD => {
                want(scene, ROYAL, [1, 1, 2]);
                want(scene, BOMBER, [1, 1, 1]);
            }
            B_RAIDERS => {
                want(scene, ROYAL, [2, 2, 2]);
                want(scene, RAMMER, [1, 1, 1]);
                // Grown while sailing to its post: patrol instead.
                if state(scene, k) == MARCH && strong(scene, k) {
                    scene.set_state(k, 6);
                }
            }
            M_AMBER | M_FISH | S_AMBER | S_FISH => want(scene, MERCHANT, [2, 2, 2]),
            M_GUARD => {
                want(scene, ROYAL, [1, 2, 2]);
                want(scene, PERSIAN_A, [1, 1, 2]);
                want(scene, BOMBER, [2, 1, 2]);
                want(scene, PERSIAN_B, [2, 1, 2]);
                // Its strike lasts while player 15 is rich.
                if state(scene, k) == 36 && !rich(scene) {
                    scene.set_state(k, 0);
                }
            }
            M_STRIKERS | S_RAIDERS => {
                // (Royal ships from a table nothing fills, 0x5c7474 and
                // 0x5c7480: never asked for.)
                want(scene, BOARDER, [0, 1, 2]);
                want(scene, PERSIAN_B, [2, 2, 2]);
                want(scene, PERSIAN_A, [3, 3, 3]);
                if k == S_RAIDERS && state(scene, k) == MARCH && strong(scene, k) {
                    scene.set_state(k, 6);
                }
            }
            S_PATROL => {
                want(scene, ROYAL, [2, 2, 2]);
                want(scene, BOMBER, [3, 3, 3]);
                want(scene, PERSIAN_A, [0, 1, 1]);
                want(scene, PERSIAN_B, [0, 0, 1]);
                spot(scene);
            }
            S_STRIKERS => {
                want(scene, ROYAL, [3, 3, 3]);
                want(scene, PERSIAN_B, [1, 1, 2]);
                want(scene, PERSIAN_A, [1, 1, 1]);
                if state(scene, k) == MARCH && strong(scene, k) {
                    scene.set_state(k, 8);
                }
            }
            M_CASTER => caster(scene, k, M_REPAIR),
            S_CASTER => caster(scene, k, S_REPAIR),
            _ => {}
        }
    }

    /// Member (2, 3)'s look out (0x4765b0, on each turn): the first of
    /// the player's ships, if it lies in rows 38-54 and nothing was sent
    /// yet, sends members 4 and 5 to strike 3 cells about it (0x37; brain
    /// `+0x1bc`, 1500 ticks), once until member 3 or 4 starts over.
    fn spot(scene: &mut Scene) {
        let Some(cell) = scene
            .objects
            .iter()
            .find(|o| scene.is_own(o) && o.ship.is_some())
            .map(cell_of)
        else {
            return;
        };
        if (38..=54).contains(&cell.1) && var(scene, SOUTH, SPOTTED) == 0 {
            let area = scene.cells_about(cell, 3);
            for f in [S_RAIDERS, S_STRIKERS] {
                scene.set_fleet_area(f, area);
                scene.set_state(f, 55);
            }
            set_var(scene, SOUTH, SPOTTED, 1);
        }
    }

    fn idle(_: u32) -> Op {
        Op::Custom(|scene, k| state(scene, k))
    }

    /// The barbarians' yard (0x4741b0): rolls its rest `((50, 30, 10) by
    /// difficulty + rand(31)) x 25` ticks if it has none, takes the first
    /// request of members 0-3 (0), builds it for that member for free
    /// (10), rests once it is launched (11), then clears the request (12).
    /// Codes other than merchants, royal ships, bombers, rammers and ship
    /// killers are dropped. (State 20, waiting for the launch, is the
    /// port's.)
    fn barbarian_yard(state: u32) -> Op {
        match state {
            0 => Op::Custom(|scene, k| {
                if var(scene, k, WAIT) == 0 {
                    let rest = ([50, 30, 10][difficulty()] + scene.rng.next() % 31) * 25;
                    set_var(scene, k, WAIT, rest);
                }
                match (B_YARD..=B_RAIDERS).find(|&f| var(scene, f, ASKED) != 0) {
                    Some(f) => {
                        set_var(scene, k, SERVING, f as u32);
                        10
                    }
                    None => 0,
                }
            }),
            10 => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder == NONE {
                    return 0;
                }
                let kind = var(scene, holder as usize, ASKED) as u8 - 1;
                if !matches!(kind, MERCHANT | ROYAL | BOMBER | RAMMER | KILLER) {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    set_var(scene, k, WAIT, 0);
                    return 10;
                }
                if scene.fleet_build_for(k, holder as usize, kind) {
                    20
                } else {
                    10
                }
            }),
            20 => Op::Custom(|scene, k| if scene.fleet_building(k) { 20 } else { 11 }),
            11 => Op::WaitVar {
                next: 12,
                var: WAIT,
            },
            _ => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder != NONE {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    set_var(scene, k, WAIT, 0);
                }
                0
            }),
        }
    }

    /// The members a Persian yard serves: player 15's 0-6, player 14's
    /// 0-7.
    fn members(yard: usize) -> std::ops::Range<usize> {
        if yard == MIDDLE {
            MIDDLE..M_CASTER + 1
        } else {
            SOUTH..S_CASTER + 1
        }
    }

    /// A Persian yard (0x474be0, 0x475f70): (its order of class 1 at 0: the full town, see `Job`) takes the first request of its group's members (5),
    /// builds it for that member (10), rests 500 ticks once it is launched
    /// (11), then clears it (12). (State 20, waiting for the launch, is
    /// the port's.)
    fn persian_yard(state: u32) -> Op {
        match state {
            // The full-town order before every ship (0x47b130 code 1).
            0 => Op::Custom(|scene, k| {
                if scene.yard_town_complete(k) {
                    5
                } else {
                    scene.want_full_town(k);
                    0
                }
            }),
            5 => Op::Custom(
                |scene, k| match members(k).find(|&f| var(scene, f, ASKED) != 0) {
                    Some(f) => {
                        set_var(scene, k, SERVING, f as u32);
                        10
                    }
                    None => 5,
                },
            ),
            10 => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder == NONE {
                    return 0;
                }
                let kind = var(scene, holder as usize, ASKED) as u8 - 1;
                if !matches!(kind, 0..=6 | PERSIAN_A | PERSIAN_B) {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                    return 10;
                }
                if scene.fleet_build_for(k, holder as usize, kind) {
                    20
                } else {
                    10
                }
            }),
            20 => Op::Custom(|scene, k| if scene.fleet_building(k) { 20 } else { 11 }),
            11 => Op::Wait {
                next: 12,
                ticks: 500,
            },
            _ => Op::Custom(|scene, k| {
                let holder = var(scene, k, SERVING);
                if holder != NONE {
                    set_var(scene, holder as usize, ASKED, 0);
                    set_var(scene, k, SERVING, NONE);
                }
                0
            }),
        }
    }

    const fn zone(area: Cells) -> Event {
        Event::Zone { state: 20, area }
    }

    const ATTACKED: Event = Event::Attacked {
        state: 30,
        after: 375,
    };

    /// Fewer ships than one more than the fleet's variable 3 (0x47a1e0,
    /// "fewer than"): back to 0.
    const FEWER: Event = Event::Size {
        state: 0,
        after: None,
        cmp: Cmp::AtMost,
        n: FEW,
    };

    /// A fleet's fights: the zone seen 1500 ticks (20), whoever fired on
    /// it `answer` ticks (30).
    fn fight(state: u32, answer: u32) -> Op {
        match state {
            20 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Attacker,
                ticks: answer,
                events: &[],
            },
        }
    }

    /// Rolls a patrol's length `(rand(4) + base by difficulty) x 1500`.
    fn roll_length(scene: &mut Scene, k: usize, base: [u32; 3]) -> u32 {
        let ticks = ((scene.rng.next() & 3) + base[difficulty()]) * 1500;
        set_var(scene, k, LENGTH, ticks);
        5
    }

    /// The barbarians' corner (event data 0x5b4da0 and on).
    const CORNER: Cells = (1, 1, 9, 7);

    /// The barbarians' guard (member 1, 0x474410; no ships at first):
    /// sails to (8, 5) every 750 ticks watching their corner (events
    /// 0x5b4e20); the player there is fought 1500 ticks, whoever fires on
    /// it 250. It asks for royal ships (1, 1, 2) and bombers (1).
    fn corner_guard(state: u32) -> Op {
        const W: &[Event] = &[zone(CORNER), ATTACKED];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 0,
                failed: 0,
                to: (8, 5),
                ticks: 750,
                events: W,
            },
            _ => fight(state, 250),
        }
    }

    /// The barbarians' gatherers (member 2, 0x4745c0; the ships at (6,
    /// 11)): fish (2, 1, 8, 7) 4500 ticks, home to (5, 1) (750 at most),
    /// rest 3000; then amber in (9, 11, 9, 8) the same.
    fn corner_gatherers(state: u32) -> Op {
        let home = |next| Op::Move {
            ok: next,
            failed: next,
            to: (5, 1),
            ticks: 750,
            events: &[],
        };
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
                area: (2, 1, 8, 7),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[],
            },
            11 => Op::Gather {
                next: 13,
                area: (9, 11, 9, 8),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            7 | 13 => home(state + 2),
            9 => Op::Wait {
                next: 11,
                ticks: 3000,
            },
            _ => Op::Wait {
                next: 0,
                ticks: 3000,
            },
        }
    }

    /// The barbarians' raiders (member 3, 0x474740; the ships in (8, 7, 3,
    /// 3)): roll their patrol's length `(rand(4) + (9, 7, 5)) x 1500`
    /// (`+0x12c`; 0); short of strength they sail to (6, 1) watching their
    /// corner (events 0x5b4f70), at strength patrol (0x5b4f58) watching
    /// (3, 5, 19, 10) too (0x5b4f88), then strike 6000 ticks (10, 15;
    /// events 0x5b4fa0, until fewer than two are left): at (35, 0, 15, 9)
    /// if the player has something there, else about one of the player's
    /// settlements or towns in (0, 0, 50, 72), else not at all. They
    /// answer fire 250 ticks and ask for royal ships (2) and rammers (1).
    fn corner_raiders(state: u32) -> Op {
        const MARCHING: &[Event] = &[zone(CORNER), ATTACKED];
        const W: &[Event] = &[zone(CORNER), zone((3, 5, 19, 10)), ATTACKED];
        const STRIKING: &[Event] = &[zone(CORNER), FEWER];
        const ROUND: &[(i32, i32, u32)] = &[(5, 11, 500), (14, 9, 750)];
        match state {
            0 => Op::Custom(|scene, k| roll_length(scene, k, [9, 7, 5])),
            5 => Op::Custom(|scene, k| if strong(scene, k) { 6 } else { MARCH }),
            MARCH => Op::Move {
                ok: 0,
                failed: 0,
                to: (6, 1),
                ticks: 0,
                events: MARCHING,
            },
            6 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Custom(|scene, k| {
                const NORTH_EAST: Cells = (35, 0, 15, 9);
                if human_count(scene, NORTH_EAST) > 0 {
                    scene.set_fleet_area(k, NORTH_EAST);
                    return 15;
                }
                match scene.human_site_area(Some((0, 0, 50, 72))) {
                    Some(a) => {
                        scene.set_fleet_area(k, a);
                        15
                    }
                    None => 0,
                }
            }),
            15 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 6000,
                events: STRIKING,
            },
            _ => fight(state, 250),
        }
    }

    /// A Persian gatherer: gathers `ticks`, sails home (750 at most) and
    /// rests (it asks for merchants while it has fewer than two).
    fn gatherer(
        state: u32,
        area: Cells,
        resource: Resource,
        home: (i32, i32),
        ticks: u32,
        rest: u32,
    ) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area,
                resource,
                ticks,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: home,
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: rest,
            },
        }
    }

    /// Member (1, 1) (0x474f80): amber in (16, 19, 14, 17) 4500 ticks,
    /// home to (13, 23), rest 1500.
    fn middle_amber(state: u32) -> Op {
        gatherer(
            state,
            (16, 19, 14, 17),
            Resource::Amber,
            (13, 23),
            4500,
            1500,
        )
    }

    /// Member (1, 2) (0x4750d0): fish there the same.
    fn middle_fish(state: u32) -> Op {
        gatherer(
            state,
            (16, 19, 14, 17),
            Resource::Fish,
            (13, 23),
            4500,
            1500,
        )
    }

    /// The channel's zones (event data 0x5b4fb8 and on).
    const CHANNEL: [Event; 4] = [
        zone((7, 18, 6, 6)),
        zone((13, 18, 6, 6)),
        zone((18, 20, 6, 6)),
        zone((18, 26, 7, 5)),
    ];

    /// The channel's guard (member (1, 3), 0x475220; the ships in (20,
    /// 26, 5, 3)): sails to (22, 25) every 750 ticks watching the channel
    /// (events 0x5b50d8; the player there fought 1500 ticks, whoever fires
    /// on it 375). Member 4 may send it to (42, 21) (0x19, 750 ticks at
    /// most, the same events) and on to strike, or to strike at once
    /// (0x23): while player 15 holds 5000 amber and 4000 fish, at the area
    /// member 4 last chose (brain `+0x180`) 4500 ticks (36; events
    /// 0x5b50f0, until fewer than two are left), else back to 0. It asks
    /// for royal ships (1, 2, 2), Persian ships 0xd2 (1, 1, 2), bombers
    /// (2, 1, 2) and Persian ships 0xd3 (2, 1, 2).
    fn middle_guard(state: u32) -> Op {
        const W: &[Event] = &[CHANNEL[0], CHANNEL[1], CHANNEL[2], CHANNEL[3], ATTACKED];
        const STRIKING: &[Event] = &[CHANNEL[0], CHANNEL[1], CHANNEL[2], CHANNEL[3], FEWER];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Move {
                ok: 0,
                failed: 0,
                to: (22, 25),
                ticks: 750,
                events: W,
            },
            25 => Op::Move {
                ok: 35,
                failed: 35,
                to: (42, 21),
                ticks: 750,
                events: W,
            },
            35 => Op::Custom(|scene, k| {
                if !rich(scene) {
                    return 0;
                }
                let area = unpack(var(scene, MIDDLE, STRIKE_AREA));
                scene.set_fleet_area(k, area);
                36
            }),
            36 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: STRIKING,
            },
            _ => fight(state, 375),
        }
    }

    /// The channel's strikers (member (1, 4), 0x4754c0; the ships in (17,
    /// 20, 3, 4)): roll their patrol's length `(rand(4) + (11, 8, 7)) x
    /// 1500` (`+0x17c`; 0), patrol the channel (0x5b5108) watching it and
    /// (0, 18, 8, 8) and (4, 17, 22, 7) (events 0x5b5368; the player there
    /// fought 1500 ticks, but see the module's notes; whoever fires on
    /// them 375, then back to what they did), then look (10): about the
    /// last of the player's royal ships carrying heroes (ship `+0x259`) or
    /// a lasting gift (treasure kinds 10-14, 0x4b9740 group 0x10), one
    /// time in two, they strike 4500 ticks (0x23; events 0x5b5380, until
    /// fewer than three are left); else, the player having something in
    /// (35, 0, 15, 9), they send member 3 there by (42, 21) (0x19), or, the
    /// player having a settlement or town in (0, 0, 50, 72), they send
    /// member 3 to strike about one picked at random (0x23); and start
    /// over. The area chosen is kept for member 3 (`+0x180`). They ask for
    /// boarders (0, 1, 2) and Persian ships 0xd3 (2) and 0xd2 (3).
    fn middle_strikers(state: u32) -> Op {
        const W: &[Event] = &[
            CHANNEL[0],
            CHANNEL[1],
            CHANNEL[2],
            CHANNEL[3],
            Event::Zone {
                state: 21,
                area: (0, 18, 8, 8),
            },
            Event::Zone {
                state: 22,
                area: (4, 17, 22, 7),
            },
            ATTACKED,
        ];
        const HUNTING: &[Event] = &[CHANNEL[0], CHANNEL[1], CHANNEL[2], CHANNEL[3], FEWER];
        const ROUND: &[(i32, i32, u32)] = &[
            (2, 21, 1500),
            (13, 20, 1500),
            (20, 21, 1500),
            (13, 20, 1500),
        ];
        match state {
            0 => Op::Custom(|scene, k| roll_length(scene, k, [11, 8, 7])),
            5 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Custom(|scene, k| {
                let carrier = scene
                    .objects
                    .iter()
                    .filter(|o| {
                        scene.is_own(o)
                            && o.ship.as_ref().is_some_and(|s| {
                                s.kind == ROYAL
                                    && (!s.heroes.is_empty()
                                        || s.treasures.iter().any(|t| (10..=14).contains(&t.kind)))
                            })
                    })
                    .map(cell_of)
                    .last();
                if let Some(cell) = carrier {
                    let area = scene.cells_about(cell, 2);
                    set_var(scene, MIDDLE, STRIKE_AREA, pack(area));
                    if scene.rng.next() & 1 == 0 {
                        scene.set_fleet_area(k, area);
                        return 35;
                    }
                }
                const NORTH_EAST: Cells = (35, 0, 15, 9);
                set_var(scene, MIDDLE, STRIKE_AREA, pack(NORTH_EAST));
                if human_count(scene, NORTH_EAST) > 0 {
                    scene.set_state(M_GUARD, 25);
                } else if let Some(a) = scene.human_site_area(Some((0, 0, 50, 72))) {
                    set_var(scene, MIDDLE, STRIKE_AREA, pack(a));
                    scene.set_state(M_GUARD, 35);
                }
                0
            }),
            20 => fight(20, 375),
            // The zones its fights misread (see the module's notes).
            21 => Op::Custom(|_, _| 0),
            22 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((20, 7, 18, 6)),
                ticks: 1500,
                events: &[],
            },
            35 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: HUNTING,
            },
            _ => Op::Attack {
                next: PREVIOUS,
                failed: PREVIOUS,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
        }
    }

    /// Member (2, 1) (0x476310): amber in (45, 56, 5, 8) 6000 ticks, home
    /// to (48, 66), rest 3000.
    fn south_amber(state: u32) -> Op {
        gatherer(state, (45, 56, 5, 8), Resource::Amber, (48, 66), 6000, 3000)
    }

    /// Member (2, 2) (0x476460): fish in (28, 52, 22, 20) the same.
    fn south_fish(state: u32) -> Op {
        gatherer(
            state,
            (28, 52, 22, 20),
            Resource::Fish,
            (48, 66),
            6000,
            3000,
        )
    }

    /// The south's home waters (event data 0x5b5398 and on).
    const HOME: [Event; 3] = [
        zone((35, 52, 15, 5)),
        zone((35, 57, 8, 7)),
        zone((35, 64, 15, 6)),
    ];

    /// The south's patrol (member (2, 3), 0x4765b0; the ships in (39, 56,
    /// 4, 3)): rolls its patrol's length `(rand(4) + (12, 10, 10)) x 1500`
    /// (`+0x190`) and clears its look out (0), patrols the home waters
    /// (0x5b5428; events 0x5b54c0), the player there fought 1500 ticks,
    /// whoever fires on it 375, and starts over (see `spot`). It asks for
    /// royal ships (2), bombers (3) and Persian ships 0xd2 (0, 1, 1) and
    /// 0xd3 (0, 0, 1).
    fn south_patrol(state: u32) -> Op {
        const W: &[Event] = &[HOME[0], HOME[1], HOME[2], ATTACKED];
        const ROUND: &[(i32, i32, u32)] = &[
            (39, 55, 1500),
            (38, 59, 1500),
            (39, 65, 1500),
            (38, 59, 1500),
        ];
        match state {
            0 => Op::Custom(|scene, k| {
                set_var(scene, SOUTH, SPOTTED, 0);
                roll_length(scene, k, [12, 10, 10])
            }),
            5 => Op::Patrol {
                timeout: 0,
                failed: 0,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            _ => fight(state, 375),
        }
    }

    /// The south's raiders (member (2, 4), 0x4768e0; the ships in (38,
    /// 61, 2, 3)): clear the look out and roll their patrol's length
    /// `(rand(4) + (10, 8, 6)) x 1500` (`+0x194`; 0); short of strength
    /// they sail to (46, 53), at strength patrol the southern sea
    /// (0x5b54f0); both watching the home waters (fought 1500 ticks),
    /// (14, 49, 24, 14) and (37, 41, 9, 8) (state 0x1e, 30: 375 ticks on
    /// `last_area`, not the zone) (events 0x5b56f0) and answering fire
    /// 375 (30 too); then look (10): the first of the
    /// player's royal ships carrying heroes, if it lies below row 44, one
    /// time in two, they strike 2 cells about 4500 ticks (0x19; events
    /// 0x5b5708, until fewer than three are left); else in (0, 35, 50, 18)
    /// if the player has something there; else start over. Member 3 may
    /// send them to strike (0x37). They ask for boarders (0, 1, 2) and
    /// Persian ships 0xd3 (2) and 0xd2 (3).
    fn south_raiders(state: u32) -> Op {
        const W: &[Event] = &[
            HOME[0],
            HOME[1],
            HOME[2],
            Event::Zone {
                state: 30,
                area: (14, 49, 24, 14),
            },
            Event::Zone {
                state: 30,
                area: (37, 41, 9, 8),
            },
            ATTACKED,
        ];
        const STRIKING: &[Event] = &[HOME[0], HOME[1], HOME[2], FEWER];
        const ROUND: &[(i32, i32, u32)] = &[
            (16, 50, 1500),
            (27, 47, 1500),
            (44, 47, 1500),
            (27, 47, 1500),
        ];
        match state {
            0 => Op::Custom(|scene, k| {
                set_var(scene, SOUTH, SPOTTED, 0);
                roll_length(scene, k, [10, 8, 6])
            }),
            5 => Op::Custom(|scene, k| if strong(scene, k) { 6 } else { MARCH }),
            MARCH => Op::Move {
                ok: 0,
                failed: 0,
                to: (46, 53),
                ticks: 0,
                events: W,
            },
            6 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Custom(|scene, k| {
                let heroes = scene
                    .objects
                    .iter()
                    .find(|o| {
                        scene.is_own(o)
                            && o.ship
                                .as_ref()
                                .is_some_and(|s| s.kind == ROYAL && !s.heroes.is_empty())
                    })
                    .map(cell_of);
                if let Some(cell) = heroes {
                    scene.set_fleet_area(k, scene.cells_about(cell, 2));
                    if cell.1 > 44 && scene.rng.next() & 1 == 0 {
                        return 25;
                    }
                }
                const SEA: Cells = (0, 35, 50, 18);
                scene.set_fleet_area(k, SEA);
                if human_count(scene, SEA) > 0 {
                    25
                } else {
                    0
                }
            }),
            25 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 4500,
                events: STRIKING,
            },
            30 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::LastArea,
                ticks: 375,
                events: &[],
            },
            55 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => fight(state, 375),
        }
    }

    /// The south-west's strikers (member (2, 5), 0x476dc0; the ships in
    /// (38, 65, 3, 2)): roll `r = rand(4)`, their spell (`+0x1b0`: 0-1 the
    /// Flame, paid; 2 the phantom; 3 the weapon shield) and their patrol's
    /// length `((20, 15, 10) + r) x 1500` (`+0x198`, the table `+0x1b8`
    /// picks, set at the start; 0); short of strength they sail to (47,
    /// 67) and wait there 750 ticks, at strength patrol the south-west
    /// (0x5b5720); all watching the home waters and (24, 58, 13, 14)
    /// (events 0x5b5900; fought 1500 ticks) and answering fire 375. Their
    /// patrol done, they cast the spell on their ships (0x47acf0 by member
    /// 0; 10) and strike (0, 55, 8, 8) 4500 ticks (0x19; events 0x5b5918,
    /// until fewer than three are left). Member 3 may send them to strike
    /// (0x37). They ask for royal ships (3) and Persian ships 0xd3 (1, 1,
    /// 2) and 0xd2 (1).
    fn south_strikers(state: u32) -> Op {
        const W: &[Event] = &[HOME[0], HOME[1], HOME[2], zone((24, 58, 13, 14)), ATTACKED];
        const STRIKING: &[Event] = &[HOME[0], HOME[1], HOME[2], FEWER];
        const ROUND: &[(i32, i32, u32)] = &[
            (26, 68, 1500),
            (30, 60, 1500),
            (31, 69, 1500),
            (30, 60, 1500),
        ];
        match state {
            0 => Op::Custom(|scene, k| {
                let r = scene.rng.next() & 3;
                set_var(scene, k, SPELL, [9, 9, 12, 15][r as usize]);
                let ticks = ([20, 15, 10][difficulty()] + r) * 1500;
                set_var(scene, k, LENGTH, ticks);
                5
            }),
            5 => Op::Custom(|scene, k| if strong(scene, k) { 8 } else { MARCH }),
            MARCH => Op::Move {
                ok: 6,
                failed: 6,
                to: (47, 67),
                ticks: 0,
                events: W,
            },
            6 => Op::Watch {
                next: 0,
                ticks: 750,
                events: W,
            },
            8 => Op::Patrol {
                timeout: 10,
                failed: 10,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Custom(|scene, k| {
                // Through the war temple (0x47acf0, 0x42f470): the Flame
                // credited one cast (pay 1), the shields not.
                let spell = match var(scene, k, SPELL) {
                    9 => Some((Spell::Flame, true)),
                    12 => Some((Spell::Phantom, false)),
                    15 => Some((Spell::ProtectWeapons, false)),
                    _ => None,
                };
                if let Some((spell, pay)) = spell {
                    let ships = scene.fleet_ships(k);
                    scene.brain_cast_fleet(ships, spell, pay);
                }
                25
            }),
            25 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Area((0, 55, 8, 8)),
                ticks: 4500,
                events: STRIKING,
            },
            55 => Op::Attack {
                next: 0,
                failed: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => fight(state, 375),
        }
    }

    /// Members (1, 5) and (2, 6) (0x475a70, 0x477190): send what they are
    /// handed to the docks.
    fn repair(_: u32) -> Op {
        Op::Custom(|scene, k| {
            scene.fleet_to_docks(k);
            0
        })
    }

    /// A caster (members (1, 6) and (2, 7), 0x475af0 and 0x477210), run
    /// from `turn` as it has no ships: waits `(3, 2, 1) by difficulty x
    /// 1500` ticks (10), then, if it has cast fewer than (1, 2, 3) spells,
    /// looks through the player's things within 15 cells of the first
    /// temple (class 0x3e) of the player `holder` keeps (15) and casts one
    /// spell (0x47ae10 by member (1, 0), as player 15, for nothing) at the
    /// first it picks (see `look_and_cast`), and starts over; past its
    /// spells it stays.
    fn caster(scene: &mut Scene, k: usize, holder: usize) {
        let d = difficulty();
        match state(scene, k) {
            0 => scene.set_state(k, 10),
            10 => {
                let now = scene.brain.as_ref().unwrap().tick;
                let until = var(scene, k, UNTIL);
                if until == 0 {
                    set_var(scene, k, UNTIL, now + [3, 2, 1][d] * 1500);
                } else if now >= until {
                    set_var(scene, k, UNTIL, 0);
                    scene.set_state(k, 15);
                }
            }
            15 if var(scene, k, CASTS) < [1, 2, 3][d] => {
                look_and_cast(scene, k, holder);
                scene.set_state(k, 0);
            }
            _ => {}
        }
    }

    /// The caster's pick: a fireball at a settlement one time in two;
    /// lightning at a town or temple (0x3e); a waterspout or fireball, one
    /// time in four each, at a temple (0x3d), fort (0x36) or building of
    /// class 0x34, 0x35; at anything else with at least four of the
    /// player's things within 3 cells, a waterspout, fireball or sea
    /// monster one time in (20, 15, 10) + 1 each. A roll that picks
    /// nothing ends the look.
    fn look_and_cast(scene: &mut Scene, k: usize, holder: usize) {
        let d = difficulty();
        let Some(owner) = player(scene, holder) else {
            return;
        };
        let caster = player(scene, M_REPAIR);
        let Some(temple) = scene
            .objects
            .iter()
            .find(|o| o.owner == Some(owner) && o.class == ObjectClass::Building(0x3e))
            .map(cell_of)
        else {
            return;
        };
        let area = scene.cells_about(temple, 15);
        let things: Vec<(ObjectClass, (i32, i32))> = scene
            .objects
            .iter()
            .filter(|o| scene.is_own(o) && o.combat.is_some())
            .map(|o| (o.class, cell_of(o)))
            .collect();
        for (class, cell) in things {
            if !inside(cell, area) {
                continue;
            }
            let (spell, at) = match class {
                ObjectClass::Building(0x32) => match scene.rng.next() & 3 {
                    0 | 2 => (Spell::Fireball, cell),
                    _ => break,
                },
                ObjectClass::Building(0x33 | 0x3e) => (Spell::Lightning, cell),
                ObjectClass::Building(0x34 | 0x35 | 0x36 | 0x3d) => match scene.rng.next() & 3 {
                    0 => (Spell::Waterspout, cell),
                    2 => (Spell::Fireball, cell),
                    _ => break,
                },
                _ => {
                    let roll = scene.rng.next() % ([20, 15, 10][d] + 1);
                    let near = scene.cells_about(cell, 3);
                    let at = (near.0 + near.2 / 2, near.1 + near.3 / 2);
                    if human_count(scene, near) < 4 {
                        break;
                    }
                    match roll {
                        0 => (Spell::Waterspout, at),
                        4 => (Spell::Fireball, at),
                        8 => (Spell::Monster, at),
                        _ => break,
                    }
                }
            };
            scene.pending_casts.push((spell, at.0, at.1, caster));
            let casts = var(scene, k, CASTS);
            set_var(scene, k, CASTS, casts + 1);
            break;
        }
    }
}
