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
    /// 0x479f40; the fleet remembers the area to attack).
    Zone { state: u32, area: Cells },
    /// A ship of the fleet was fired on by the human player, at least
    /// `after` ticks into the step: attack around the attacker (0x47a060).
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
}

/// Where an attack goes.
#[derive(Debug, Clone, Copy)]
pub enum Target {
    /// The area the event that started it saw.
    Seen,
    /// A fixed area.
    Area(Cells),
    /// Around the ship that fired on the fleet.
    Attacker,
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
    /// (0: forever) or an event.
    Patrol {
        timeout: u32,
        points: &'static [(i32, i32, u32)],
        ticks: u32,
        events: &'static [Event],
    },
    /// Attack (0x47a6a0): `next` once the area is clear or after `ticks`.
    Attack {
        next: u32,
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
    started: bool,
    since: u32,
    /// The area an event chose to attack.
    area: Option<Cells>,
    /// Patrol: the point sailed to, and when the pause there began.
    point: usize,
    paused_at: Option<u32>,
    /// Who last fired on the fleet, and where.
    pub attacked_from: Option<(f32, f32)>,
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
}

fn mission(name: &str) -> Option<Mission> {
    match name {
        "miss000" => Some(Mission {
            fleets: miss000::FLEETS,
            start: miss000::start,
            turn: None,
        }),
        "miss001" => Some(Mission {
            fleets: miss001::FLEETS,
            start: miss001::start,
            turn: None,
        }),
        "miss002" => Some(Mission {
            fleets: miss002::FLEETS,
            start: miss002::start,
            turn: None,
        }),
        "miss003" => Some(Mission {
            fleets: miss003::FLEETS,
            start: miss003::start,
            turn: None,
        }),
        "miss004" => Some(Mission {
            fleets: miss004::FLEETS,
            start: miss004::start,
            turn: None,
        }),
        "miss005" => Some(Mission {
            fleets: miss005::FLEETS,
            start: miss005::start,
            turn: None,
        }),
        "miss006" => Some(Mission {
            fleets: miss006::FLEETS,
            start: miss006::start,
            turn: None,
        }),
        "miss007" => Some(Mission {
            fleets: miss007::FLEETS,
            start: miss007::start,
            turn: Some(miss007::turn),
        }),
        "miss008" => Some(Mission {
            fleets: miss008::FLEETS,
            start: miss008::start,
            turn: Some(miss008::turn),
        }),
        "miss009" => Some(Mission {
            fleets: miss009::FLEETS,
            start: miss009::start,
            turn: None,
        }),
        "miss010" => Some(Mission {
            fleets: miss010::FLEETS,
            start: miss010::start,
            turn: None,
        }),
        "miss011" => Some(Mission {
            fleets: miss011::FLEETS,
            start: miss011::start,
            turn: None,
        }),
        "miss012" => Some(Mission {
            fleets: miss012::FLEETS,
            start: miss012::start,
            turn: None,
        }),
        "miss013" => Some(Mission {
            fleets: miss013::FLEETS,
            start: miss013::start,
            turn: None,
        }),
        _ => None,
    }
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
                attacked_from: None,
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

    /// Notes that the human player fired on ship `target` from `(x, y)`.
    pub(super) fn fleet_attacked(&mut self, target: u32, x: f32, y: f32) {
        if let Some(b) = self.brain.as_mut() {
            for f in &mut b.fleets {
                if f.ships.contains(&target) {
                    f.attacked_from = Some((x, y));
                }
            }
        }
    }

    /// Runs the brain one tick: each fleet takes its turn every 50 ticks, a
    /// fleet 5 ticks after the one before (0x44d0e0).
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
        for k in 0..m.fleets.len().min(brain.fleets.len()) {
            if tick % 50 != (5 * k as u32) % 50 {
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
            if self.brain.as_ref().unwrap().fleets[k].ships.is_empty() {
                continue;
            }
            let groups: Vec<u8> = m.fleets.iter().map(|f| f.group).collect();
            self.fleet_step(k, m.fleets[k].program, &groups, tick);
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
    fn event_fired(&mut self, k: usize, events: &[Event], elapsed: u32) -> Option<u32> {
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
                    let f = &mut self.brain.as_mut().unwrap().fleets[k];
                    if let Some((x, y)) = f.attacked_from.take() {
                        if elapsed >= after {
                            let (cx, cy) = ((x / t) as i32, (y / t) as i32);
                            f.area = Some((cx - 1, cy - 1, 3, 3));
                            return Some(state);
                        }
                    }
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
                    self.fleet_gather(k, area, resource);
                } else if ticks > 0 && elapsed >= ticks {
                    self.fleet_stop(k);
                    self.set_state(k, next);
                }
            }
            Op::Patrol {
                timeout,
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
                    let seen = self.brain.as_ref().unwrap().fleets[k].area;
                    let area = match target {
                        Target::Area(a) => Some(a),
                        Target::Seen | Target::Attacker => seen,
                        Target::HumanBuilding => self.random_human_building(),
                    };
                    match area {
                        Some(a) => self.fleet_attack(k, a),
                        None => self.set_state(k, next),
                    }
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
            cells: &[(9, 21, 1, 1), (10, 22, 1, 1)],
            program: guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(11, 18, 1, 1)],
            program: fisher,
            wants: &[],
        },
        FleetDef {
            group: 0,
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
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
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
                target: Target::Area((15, 0, 9, 6)),
                ticks: 4500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 40,
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
            cells: &[(25, 1, 1, 1), (27, 3, 1, 1)],
            program: builder,
            wants: &[],
        },
        // A royal ship and a rammer guard the coast (they want more only on
        // the hardest difficulty).
        FleetDef {
            group: 0,
            cells: &[(23, 4, 1, 1), (23, 7, 1, 1)],
            program: coastguard,
            wants: &[(ROYAL, [0, 0, 1]), (RAMMER, [0, 0, 1])],
        },
        // A merchant with a ship killer for escort gathers amber and fish.
        FleetDef {
            group: 0,
            cells: &[],
            program: gatherers,
            wants: &[(MERCHANT, [1; 3]), (SHIP_KILLER, [1; 3])],
        },
        // Ship killers patrol the middle of the sea (0x5adafc).
        FleetDef {
            group: 0,
            cells: &[],
            program: patrol,
            wants: &[(SHIP_KILLER, [2, 3, 4])],
        },
        // Ship killers and royal ships raid the player when strong enough
        // (0x5adb2c).
        FleetDef {
            group: 0,
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
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
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
                target: Target::Seen,
                ticks: 4500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
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
                target: Target::Area((1, 1, 9, 6)),
                ticks: 6000,
                events: RAIDING,
            },
            20 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 4500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 0,
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
            cells: &[(33, 3, 1, 1)],
            program: barbarian_yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(30, 4, 3, 3)],
            program: barbarian_guard,
            wants: &[(RAMMER, [2, 3, 3]), (ROYAL, [1, 1, 2])],
        },
        // The Persians (0x44bee0-0x44cbd0).
        FleetDef {
            group: 1,
            cells: &[],
            program: amber_gatherers,
            wants: &[(MERCHANT, [1; 3]), (SHIP_KILLER, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: fishers,
            wants: &[(MERCHANT, [1; 3]), (SHIP_KILLER, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: home_guard,
            wants: &[(ROYAL, [3; 3]), (BOARDER, [1; 3]), (RAMMER, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: raiders,
            wants: &[(ROYAL, [1, 1, 2]), (BOARDER, [1, 2, 1]), (RAMMER, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: west_guard,
            wants: &[(ROYAL, [2; 3]), (BOARDER, [2; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_guard,
            wants: &[(ROYAL, [2; 3]), (RAMMER, [3; 3])],
        },
        FleetDef {
            group: 1,
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
            0 => Op::Build { next: 11 },
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
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
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
                points: ROUND,
                ticks: 3000,
                events: SWEEP,
            },
            50 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 4500,
                events: FIGHTING,
            },
            _ => Op::Attack {
                next: 0,
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
                target: Target::Seen,
                ticks: 1500,
                events: POST,
            },
            30 => Op::Patrol {
                timeout: 5,
                points: ROUND,
                ticks: 0,
                events: WEST,
            },
            _ => Op::Attack {
                next: 30,
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
                target: Target::Seen,
                ticks: 1500,
                events: POST,
            },
            30 => Op::Patrol {
                timeout: 5,
                points: ROUND,
                ticks: 0,
                events: WEST,
            },
            _ => Op::Attack {
                next: 30,
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
            cells: &[],
            program: persian_gatherers,
            wants: &[(MERCHANT, [1; 3]), (SHIP_KILLER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[(54, 3, 3, 3)],
            program: persian_guard,
            wants: &[(ROYAL, [2, 3, 3]), (RAMMER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[(51, 0, 2, 2)],
            program: persian_raiders,
            wants: &[(BOARDER, [2, 3, 2]), (ROYAL, [2, 2, 3])],
        },
        FleetDef {
            group: 0,
            cells: &[(58, 1, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(52, 40, 5, 5)],
            program: southern_guard,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [2, 3, 3])],
        },
        // The barbarians (0x44f1e0-0x44f880).
        FleetDef {
            group: 1,
            cells: &[(16, 31, 1, 1)],
            program: barbarian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(11, 21, 1, 1)],
            program: barbarian_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[(11, 20, 1, 1)],
            program: barbarian_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[(12, 23, 3, 1)],
            program: barbarian_guard,
            wants: &[(ROYAL, [2; 3]), (RAMMER, [2; 3])],
        },
        FleetDef {
            group: 1,
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
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
        }
    }

    fn persian_yard(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
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
                target: Target::Area((3, 3, 21, 12)),
                ticks: 3000,
                events: FIGHT,
            },
            46 => Op::Attack {
                next: 0,
                target: Target::Area((32, 16, 7, 7)),
                ticks: 3000,
                events: FIGHT,
            },
            _ => Op::Attack {
                next: 0,
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
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
            _ => Op::Patrol {
                timeout: 0,
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
                target: Target::Attacker,
                ticks: 250,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            10 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 250,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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
            cells: &[],
            program: north_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: north_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: north_post_a,
            wants: &[(ROYAL, [2, 3, 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: north_post_b,
            wants: &[(ROYAL, [2, 2, 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: north_raiders,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [1, 2, 3])],
        },
        FleetDef {
            group: 0,
            cells: &[(1, 8, 2, 2)],
            program: yard_750,
            wants: &[],
        },
        // The barbarians (player 16; 0x453030-0x4536c0).
        FleetDef {
            group: 1,
            cells: &[(30, 34, 1, 1)],
            program: yard_var,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(26, 29, 1, 1)],
            program: barbarian_gatherers,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[(31, 32, 2, 2)],
            program: barbarian_guard,
            wants: &[(ROYAL, [1, 2, 2])],
        },
        FleetDef {
            group: 1,
            cells: &[(36, 30, 2, 2)],
            program: barbarian_raiders,
            wants: &[(RAMMER, [2, 2, 3]), (ROYAL, [1, 2, 2])],
        },
        // The southern Persians (player 14; 0x452600-0x452c60).
        FleetDef {
            group: 2,
            cells: &[],
            program: south_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: south_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: south_raiders,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [1, 2, 2])],
        },
        FleetDef {
            group: 2,
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
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 750,
            },
        }
    }

    fn yard_2000(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
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
                points: ROUND,
                ticks: 4500,
                events: SWEEP,
            },
            50 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 4500,
                events: FIGHT,
            },
            _ => Op::Attack {
                next: 0,
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
                points: HOME_ROUND,
                ticks: 0,
                events: W,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 250,
                events: &[],
            },
            40 => Op::Patrol {
                timeout: 0,
                points: NORTH_ROUND,
                ticks: 4500,
                events: NORTH,
            },
            _ => Op::Attack {
                next: 0,
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
                points: ROUND,
                ticks: 4500,
                events: SWEEP,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 4500,
                events: FIGHT,
            },
            _ => Op::Attack {
                next: 0,
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
            cells: &[(3, 29, 1, 1)],
            program: yard_var,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_guard,
            wants: &[(ROYAL, [1, 2, 1]), (BOMBER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_gatherers,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_raiders,
            wants: &[(ROYAL, [2, 2, 3]), (RAMMER, [1, 2, 2])],
        },
        // The Persians (player 15; 0x455b80-0x456e80).
        FleetDef {
            group: 1,
            cells: &[(8, 11, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_west,
            wants: &[(ROYAL, [2, 2, 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_east,
            wants: &[(ROYAL, [1, 3, 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_raiders_north,
            wants: &[(BOARDER, [2; 3]), (RAMMER, [1, 2, 2]), (ROYAL, [1, 2, 3])],
        },
        FleetDef {
            group: 1,
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
            br.fleets[9].vars = [(r[1] + NORTH_WAIT[difficulty()]) * 1500, 0, 0, 0];
            br.fleets[10].vars = [(r[2] + WEST_WAIT[difficulty()]) * 1500, 0, 0, 0];
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
            0 => Op::Build { next: 11 },
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
                points: ROUND,
                ticks: 0,
                events: W,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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
                points: ROUND,
                ticks: 4500,
                events: PERSIAN_HOME,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Area((0, 0, 8, 9)),
                ticks: 1500,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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
                points: ROUND,
                ticks: 4500,
                events: PERSIAN_HOME,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Area((0, 0, 8, 9)),
                ticks: 1500,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: PERSIAN_HOME,
            },
        }
    }

    /// Two raiding fleets patrol until they have waited long enough, then
    /// raid the player (0x456580, 0x456e80; the original also lands
    /// marines and coordinates the two, not ported).
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
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 25,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            25 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
        }
    }

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
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 25,
                points: ROUND,
                ticks: VAR0,
                events: W,
            },
            25 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
        }
    }
}

/// miss006's brain (0x45b280): barbarians in the south-west, and two Persian
/// players, one in the south and one in the north, each gathering,
/// guarding and raiding.
mod miss006 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16; 0x458990-0x458f20).
        FleetDef {
            group: 0,
            cells: &[(1, 50, 1, 1)],
            program: yard_1000,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_guard,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_gatherers,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_raiders,
            wants: &[(BOMBER, [1; 3]), (ROYAL, [3, 3, 4]), (RAMMER, [2, 3, 3])],
        },
        // The southern Persians (player 15; 0x459400-0x459b90).
        FleetDef {
            group: 1,
            cells: &[(21, 68, 2, 2)],
            program: yard_500,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_guard,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [2, 2, 1])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_raiders,
            wants: &[(BOARDER, [2, 2, 3]), (ROYAL, [2; 3])],
        },
        // The northern Persians (player 14; 0x45a270-0x45af50).
        FleetDef {
            group: 2,
            cells: &[(14, 17, 2, 2)],
            program: yard_750,
            wants: &[],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: north_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: north_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: north_guard,
            wants: &[(ROYAL, [2, 2, 3]), (BOARDER, [2, 3, 2])],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: north_raiders,
            wants: &[(BOARDER, [2, 2, 3]), (ROYAL, [2, 3, 2]), (RAMMER, [2; 3])],
        },
        // Two squadrons placed in the east (0x45ae70, 0x45af50; not decoded:
        // they guard their waters here).
        FleetDef {
            group: 2,
            cells: &[(49, 15, 1, 1), (50, 14, 1, 1), (51, 16, 1, 1)],
            program: east_guard,
            wants: &[],
        },
        FleetDef {
            group: 2,
            cells: &[
                (51, 17, 1, 1),
                (49, 18, 1, 1),
                (52, 18, 1, 1),
                (50, 19, 1, 1),
            ],
            program: east_guard,
            wants: &[],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.add_deposit(0x190, (48, 68, 1, 1), data, caches);
        scene.add_deposit(0x190, (47, 67, 1, 1), data, caches);
        scene.add_deposit(0x190, (48, 57, 1, 1), data, caches);
        scene.add_deposit(0x191, (45, 65, 2, 2), data, caches);
        // The raids: after 10500 ticks, once 6, 4 and 6 ships strong.
        if let Some(br) = scene.brain.as_mut() {
            br.fleets[3].vars = [10500, 6, 0, 0];
            br.fleets[8].vars = [10500, 4, 0, 0];
            br.fleets[13].vars = [10500, 6, 0, 0];
        }
    }

    fn yard_500(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 500,
            },
        }
    }

    fn yard_750(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
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
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
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
                points: ROUND,
                ticks: 0,
                events: W,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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
                points: ROUND,
                ticks: 4500,
                events: W,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: W,
            },
        }
    }

    fn south_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (22, 56, 500),
            (28, 55, 750),
            (36, 56, 750),
            (36, 62, 750),
            (26, 62, 750),
        ];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (16, 62, 15, 8),
            },
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
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            25 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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

    fn north_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(20, 17, 500), (20, 23, 750), (15, 26, 750)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                points: ROUND,
                ticks: 4500,
                events: &[NORTH_HOME],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[NORTH_HOME],
            },
        }
    }

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
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
            25 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
        }
    }

    fn east_guard(state: u32) -> Op {
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (43, 10, 14, 14),
        }];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Wait {
                next: 6,
                ticks: 250,
            },
            6 => Op::Custom(|scene, _| {
                if scene.human_in((43, 10, 14, 14)) {
                    20
                } else {
                    5
                }
            }),
            _ => Op::Attack {
                next: 0,
                target: Target::Area((43, 10, 14, 14)),
                ticks: 3000,
                events: W,
            },
        }
    }
}

/// miss007's brain (0x45f770). The barbarians (player 16, group 0) have
/// six yards; yards 1-5 each build for one fleet (members 6-10), which asks
/// for a ship type with a wait to follow (0x45ca00); the fleets patrol
/// their waters and raid the player's forts and towns (0x45cbf0-0x45dbd0),
/// and member 11 guards the wind wizard's waters (0x45e120). The Persians
/// (player 15, group 1) have one yard serving their merchants, fishers,
/// two guards and raiders (0x45e510-0x45eff0), and a fleet mending prizes.
/// (Not ported: member 11's hunt for Hercules's sword uses 0x47aed0, not
/// yet decoded; it guards its waters.)
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
            cells: &[(45, 30, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(6, 40, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(11, 40, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(15, 68, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(51, 74, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: east,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: west,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: south,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: south_west,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: south_east,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(14, 21, 10, 11)],
            program: wizard_guard,
            wants: &[],
        },
        // The Persians (player 15; 0x45e510-0x45f370).
        FleetDef {
            group: 1,
            cells: &[(9, 1, 2, 2)],
            program: persian_yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_south_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_raiders,
            wants: &[],
        },
        // Mends the Persians' prizes (0x45f370; see `brain_prize`).
        FleetDef {
            group: 1,
            cells: &[],
            program: idle,
            wants: &[],
        },
    ];

    /// The start (message 0x8d1): a big reef at one of two places, the
    /// deposits by difficulty (0x48b180 amber, 0x48ebc0 fish: easy all,
    /// normal the last three, hard none), and the fleets' numbers.
    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
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

    /// Each fleet's bookkeeping on its turn (the start of 0x45cbf0 and the
    /// others): what to ask the yard for; and once (every 5 ticks in the
    /// original) the big reef put up when the player sails into the strait.
    pub fn turn(scene: &mut Scene, k: usize) {
        if k == 0 {
            let f = &scene.brain.as_ref().unwrap().fleets[0];
            if f.vars[0] == 0 && scene.human_ships_in((14, 17, 8, 14)) {
                scene.brain.as_mut().unwrap().fleets[0].vars[0] = 1;
                let kind = scene.rng.next() % 3 + 1;
                scene.pending_reefs.push((22, 17, kind));
            }
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
                points: round,
                ticks: 0,
                events: watch,
            },
            10 => Op::Custom(pick),
            15 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            20 | 25 => Op::Attack {
                next: 0,
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
            _ => Op::Attack {
                next: 0,
                target: Target::Area((13, 23, 5, 5)),
                ticks: 1500,
                events: &[],
            },
        }
    }

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
                points: PERSIAN_ROUND,
                ticks: 4500,
                events: watch,
            },
            20 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 6000,
                events: watch,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            40 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 6000,
                events: RAIDING,
            },
            _ => Op::Attack {
                next: 0,
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
/// attack (0x47acf0, kinds 9, 12 and 15, paid by member 2); the builders'
/// own orders of class 0x60 and the Persian yards' order of class 1
/// (0x47b130, also skipped in miss007); the Persians' technology bit 79
/// and workshop levels set at the start; a launch the brain did not order
/// joining members 3/8, 4/9 or 10 by type (message 0x5d98): every launch
/// here is ordered. An attack's "failed" exit is taken as its "ok" one,
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
            cells: &[(12, 17, 1, 1), (18, 14, 1, 1), (20, 9, 1, 1)],
            program: idle,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(10, 9, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(14, 8, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(3, 2, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: raiders,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: home_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_fish,
            wants: &[],
        },
        // The Persians.
        FleetDef {
            group: 1,
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
            cells: &[(55, 38, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(58, 23, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: south_raiders,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: strike,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: north_amber,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: north_fish,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: north_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: middle_guard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: east_guard,
            wants: &[],
        },
        // Mends what is handed to it (0x462fb0; prizes: `brain_prize`).
        FleetDef {
            group: 1,
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
                points: round,
                ticks: 0,
                events: watch,
            },
            10 => Op::Patrol {
                timeout: 0,
                points: round,
                ticks: 0,
                events: wide,
            },
            15 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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
                points: round,
                ticks: 4500,
                events: watch,
            },
            20 => Op::Attack {
                next: done,
                target: Target::Seen,
                ticks: fight,
                events: watch,
            },
            30 => Op::Attack {
                next: done,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
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
                points: SOUTH_ROUND,
                ticks: 4500,
                events: WIDE,
            },
            20 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 4500,
                events: WIDE,
            },
            _ => patrol(state, SOUTH_ROUND, W, 4500, 0),
        }
    }

    /// Member 7 (0x4628d0): with the ships handed to it, attacks
    /// (0, 30, 10, 10) until done, then hands them to the repair member
    /// (and so whatever it gets until member 10 starts it again).
    fn strike(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Attack {
                next: 10,
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

/// miss009's brain (0x4663d0), ported coarsely: the barbarians in the west
/// and the Persians guarding the isthmus in the east build, gather, patrol
/// and guard. (The fire wizard is the map's own.)
mod miss009 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const RAMMER: u8 = 5;
    const SHIP_KILLER: u8 = 6;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16; 0x464930-0x465200).
        FleetDef {
            group: 0,
            cells: &[(0, 9, 1, 1), (8, 7, 1, 1), (13, 5, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(8, 3, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(17, 20, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(37, 40, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(9, 2, 4, 5)],
            program: north_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_north,
            wants: &[(SHIP_KILLER, [1; 3]), (ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[(42, 39, 5, 4)],
            program: south_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_south,
            wants: &[
                (SHIP_KILLER, [1; 3]),
                (RAMMER, [1; 3]),
                (ROYAL, [1, 2, 2]),
                (BOMBER, [1; 3]),
            ],
        },
        // The Persians (player 14; 0x465390-0x466060).
        FleetDef {
            group: 1,
            cells: &[(58, 43, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[(55, 35, 8, 6)],
            program: persian_patrol,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_patrol,
            wants: &[(ROYAL, [2; 3]), (RAMMER, [1, 2, 2])],
        },
        FleetDef {
            group: 1,
            cells: &[(55, 0, 15, 5)],
            program: isthmus_guard,
            wants: &[],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        for (x, y) in [(7, 43), (18, 25), (13, 1), (28, 20)] {
            scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
        }
        for (x, y) in [(21, 34), (32, 14)] {
            scene.add_deposit(0x191, (x, y, 5, 5), data, caches);
        }
    }

    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 1500,
            },
        }
    }

    /// Holds an area, attacking the player in it.
    fn hold(state: u32, area: Cells) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Wait {
                next: 6,
                ticks: 250,
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Area(area),
                ticks: 3000,
                events: &[],
            },
        }
    }

    fn north_guard(state: u32) -> Op {
        if state == 6 {
            return Op::Custom(|scene, _| {
                if scene.human_in((4, 0, 14, 12)) {
                    20
                } else {
                    5
                }
            });
        }
        hold(state, (4, 0, 14, 12))
    }

    fn south_guard(state: u32) -> Op {
        if state == 6 {
            return Op::Custom(|scene, _| {
                if scene.human_in((37, 34, 14, 12)) {
                    20
                } else {
                    5
                }
            });
        }
        hold(state, (37, 34, 14, 12))
    }

    fn isthmus_guard(state: u32) -> Op {
        if state == 6 {
            return Op::Custom(|scene, _| {
                if scene.human_in((50, 0, 20, 10)) {
                    20
                } else {
                    5
                }
            });
        }
        hold(state, (50, 0, 20, 10))
    }

    fn barbarian_north(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(2, 11, 750), (8, 15, 1000), (13, 12, 750)];
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (0, 5, 20, 15),
        }];
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
        }
    }

    fn barbarian_south(state: u32) -> Op {
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
            0 => Op::Behave { next: 5 },
            50 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            15 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
                points: ROUND,
                ticks: 0,
                events: W,
            },
        }
    }

    fn persian_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (50, 28, 20, 18),
                resource: Resource::Amber,
                ticks: 4500,
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

    fn persian_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(59, 32, 500), (60, 16, 750), (54, 22, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (50, 30, 20, 16),
            },
            Event::Zone {
                state: 20,
                area: (39, 0, 23, 30),
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
                points: ROUND,
                ticks: 4500,
                events: W,
            },
            50 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }
}

/// miss010's brain (0x46a0f0), ported coarsely: the barbarians in the west
/// and east and the Persians blockading the strait of the Golden Fleece
/// build, gather and patrol, attacking the player in their waters.
mod miss010 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const RAMMER: u8 = 5;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (0x468050-0x468bc0).
        FleetDef {
            group: 0,
            cells: &[(5, 9, 1, 1), (11, 4, 1, 1), (51, 19, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(4, 6, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(53, 30, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(60, 32, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(0, 0, 8, 4)],
            program: corner_guard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: west_patrol,
            wants: &[(RAMMER, [1; 3]), (ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: east_patrol,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: east_patrol_2,
            wants: &[(ROYAL, [2; 3]), (RAMMER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        // The Persians (0x468bc0-0x469a80).
        FleetDef {
            group: 1,
            cells: &[(23, 24, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(26, 30, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(24, 14, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: strait_patrol,
            wants: &[(ROYAL, [2, 2, 3]), (RAMMER, [1, 2, 2])],
        },
        FleetDef {
            group: 1,
            cells: &[(19, 6, 6, 6)],
            program: strait_patrol,
            wants: &[],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        for (x, y) in [(30, 40), (30, 28), (29, 12), (39, 22)] {
            scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
        }
        for (x, y) in [(3, 25), (56, 23)] {
            scene.add_deposit(0x191, (x, y, 5, 5), data, caches);
        }
    }

    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 1500,
            },
        }
    }

    fn corner_guard(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Wait {
                next: 6,
                ticks: 250,
            },
            6 => Op::Custom(|scene, _| {
                if scene.human_in((0, 0, 14, 10)) {
                    20
                } else {
                    5
                }
            }),
            _ => Op::Attack {
                next: 0,
                target: Target::Area((0, 0, 14, 10)),
                ticks: 3000,
                events: &[],
            },
        }
    }

    fn patrol(state: u32, round: &'static [(i32, i32, u32)], watch: &'static [Event]) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            20 => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 3000,
                events: &[],
            },
            _ => Op::Patrol {
                timeout: 5,
                points: round,
                ticks: 0,
                events: watch,
            },
        }
    }

    fn west_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(6, 4, 750), (1, 19, 1000), (9, 15, 750)];
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (0, 0, 14, 24),
        }];
        patrol(state, ROUND, W)
    }

    fn east_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(49, 15, 750), (51, 27, 1000), (58, 27, 750)];
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (45, 10, 19, 22),
        }];
        patrol(state, ROUND, W)
    }

    fn east_patrol_2(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(51, 23, 750), (56, 28, 1000), (54, 16, 750)];
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (45, 10, 19, 22),
        }];
        patrol(state, ROUND, W)
    }

    fn strait_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(22, 5, 750), (22, 21, 1000), (13, 23, 750)];
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (10, 0, 20, 28),
        }];
        patrol(state, ROUND, W)
    }

    fn barbarian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
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

    fn persian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (20, 5, 15, 25),
                resource: Resource::Fish,
                ticks: 7500,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to: (27, 9),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 1000,
            },
        }
    }
}

/// miss011's brain (0x46e8e0; its fleets' handlers precede it,
/// 0x46b8d0-0x46d830), ported coarsely: the first computer player holds the
/// north-east and the west coast, with sentries in the north-west; the
/// second holds the middle sea.
mod miss011 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const RAMMER: u8 = 5;

    pub const FLEETS: &[FleetDef] = &[
        FleetDef {
            group: 0,
            cells: &[(54, 11, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: east_patrol,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[(6, 29, 2, 2)],
            program: west_patrol,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[(55, 15, 3, 3)],
            program: east_raiders,
            wants: &[(ROYAL, [1, 2, 2]), (RAMMER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[(8, 31, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: west_gatherers,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: west_raiders,
            wants: &[(ROYAL, [2, 2, 3]), (RAMMER, [1, 2, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[(0, 4, 2, 2), (0, 10, 2, 2), (1, 18, 2, 2), (1, 23, 2, 2)],
            program: sentries,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(32, 17, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: middle_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: middle_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[(32, 21, 2, 2), (36, 17, 3, 2)],
            program: middle_guard,
            wants: &[(ROYAL, [1, 2, 2])],
        },
        FleetDef {
            group: 1,
            cells: &[(38, 20, 3, 3)],
            program: middle_guard_2,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        for (x, y) in [(57, 26), (57, 35), (51, 34)] {
            scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
        }
        scene.add_deposit(0x191, (53, 31, 2, 2), data, caches);
    }

    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 500,
            },
        }
    }

    fn patrol(state: u32, round: &'static [(i32, i32, u32)], watch: &'static [Event]) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                points: round,
                ticks: 4500,
                events: watch,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn gather(state: u32, area: Cells, resource: Resource, home: (i32, i32)) -> Op {
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
                ok: 20,
                failed: 20,
                to: home,
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 1500,
            },
        }
    }

    fn east_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(50, 14, 750), (54, 14, 750), (59, 14, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (49, 12, 15, 6),
            },
            Event::Attacked {
                state: 30,
                after: 250,
            },
        ];
        patrol(state, ROUND, W)
    }

    fn east_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(49, 17, 500), (56, 18, 750), (62, 16, 500), (56, 18, 875)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (49, 12, 15, 5),
            },
            Event::Zone {
                state: 20,
                area: (46, 16, 17, 8),
            },
            Event::Attacked {
                state: 30,
                after: 250,
            },
        ];
        patrol(state, ROUND, W)
    }

    const WEST: &[Event] = &[
        Event::Zone {
            state: 20,
            area: (1, 27, 6, 8),
        },
        Event::Zone {
            state: 20,
            area: (7, 25, 8, 11),
        },
        Event::Attacked {
            state: 30,
            after: 250,
        },
    ];

    fn west_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(6, 28, 750), (11, 29, 750)];
        patrol(state, ROUND, WEST)
    }

    fn west_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(14, 21, 500), (15, 26, 750), (17, 31, 500), (15, 26, 875)];
        patrol(state, ROUND, WEST)
    }

    /// Fish, then amber, off the west coast (0x46bce0).
    fn west_gatherers(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 7 },
            7 => Op::Gather {
                next: 9,
                area: (0, 21, 17, 15),
                resource: Resource::Fish,
                ticks: 4500,
                events: &[],
            },
            9 => Op::Move {
                ok: 11,
                failed: 11,
                to: (2, 33),
                ticks: 750,
                events: &[],
            },
            11 => Op::Wait {
                next: 13,
                ticks: 3000,
            },
            13 => Op::Gather {
                next: 15,
                area: (0, 21, 17, 15),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 17,
                failed: 17,
                to: (2, 33),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 7,
                ticks: 3000,
            },
        }
    }

    /// The north-west sentries attack the player in their waters
    /// (0x46c880-0x46cdc0).
    fn sentries(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Wait {
                next: 6,
                ticks: 250,
            },
            6 => Op::Custom(|scene, _| {
                if scene.human_in((0, 0, 17, 20)) {
                    20
                } else {
                    5
                }
            }),
            _ => Op::Attack {
                next: 0,
                target: Target::Area((0, 0, 17, 20)),
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn middle_amber(state: u32) -> Op {
        gather(state, (31, 23, 11, 7), Resource::Amber, (37, 24))
    }

    fn middle_fish(state: u32) -> Op {
        gather(state, (32, 18, 11, 12), Resource::Fish, (37, 24))
    }

    const MIDDLE: &[Event] = &[
        Event::Zone {
            state: 20,
            area: (30, 19, 8, 5),
        },
        Event::Zone {
            state: 20,
            area: (34, 15, 6, 5),
        },
        Event::Zone {
            state: 20,
            area: (28, 10, 8, 6),
        },
        Event::Zone {
            state: 20,
            area: (22, 13, 8, 15),
        },
        Event::Attacked {
            state: 30,
            after: 375,
        },
    ];

    fn middle_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(38, 18, 1000)];
        patrol(state, ROUND, MIDDLE)
    }

    fn middle_guard_2(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(34, 23, 1000)];
        patrol(state, ROUND, MIDDLE)
    }
}

/// miss012's brain (0x472900; its fleets' handlers precede it,
/// 0x46f760-0x472370), ported coarsely: three computer players, in the
/// east, the north-west and the south-west, build, gather and patrol their
/// waters, answering fire.
mod miss012 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const RAMMER: u8 = 5;

    pub const FLEETS: &[FleetDef] = &[
        FleetDef {
            group: 0,
            cells: &[(50, 32, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(50, 27, 2, 2), (48, 31, 1, 1)],
            program: east_guard,
            wants: &[(ROYAL, [2; 3]), (BOMBER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[(53, 26, 3, 3)],
            program: east_patrol,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(15, 12, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: north_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: north_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[(10, 10, 3, 3)],
            program: north_patrol,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(22, 12, 2, 2), (19, 15, 2, 2)],
            program: middle_patrol,
            wants: &[(ROYAL, [2, 2, 3]), (RAMMER, [1, 2, 2])],
        },
        FleetDef {
            group: 2,
            cells: &[(4, 50, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: south_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: south_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[(1, 46, 2, 2)],
            program: south_patrol,
            wants: &[(ROYAL, [1, 2, 2])],
        },
        FleetDef {
            group: 2,
            cells: &[(10, 49, 3, 2)],
            program: south_patrol_2,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        for x in [40, 45, 53] {
            scene.add_deposit(0x190, (x, 0, 1, 1), data, caches);
        }
        scene.add_deposit(0x191, (52, 5, 2, 2), data, caches);
    }

    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 500,
            },
        }
    }

    /// Patrols a round; attacks what it sees in its waters and answers fire.
    fn patrol(state: u32, round: &'static [(i32, i32, u32)], watch: &'static [Event]) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                points: round,
                ticks: 4500,
                events: watch,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn east_guard(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(50, 28, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (43, 21, 13, 19),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        patrol(state, ROUND, W)
    }

    fn east_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(51, 14, 500), (53, 20, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (43, 21, 13, 19),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        patrol(state, ROUND, W)
    }

    const NORTH: Event = Event::Zone {
        state: 20,
        area: (8, 5, 16, 15),
    };

    fn north_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(8, 13, 1500), (15, 7, 1500)];
        const W: &[Event] = &[
            NORTH,
            Event::Zone {
                state: 20,
                area: (10, 0, 14, 6),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        patrol(state, ROUND, W)
    }

    fn middle_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (26, 11, 1500),
            (34, 19, 1500),
            (25, 20, 1500),
            (34, 19, 1500),
        ];
        const W: &[Event] = &[
            NORTH,
            Event::Zone {
                state: 20,
                area: (24, 8, 9, 4),
            },
            Event::Zone {
                state: 20,
                area: (24, 12, 14, 15),
            },
            Event::Zone {
                state: 20,
                area: (38, 15, 6, 10),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        patrol(state, ROUND, W)
    }

    fn gather(
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
                ok: 20,
                failed: 20,
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

    fn north_amber(state: u32) -> Op {
        gather(state, (8, 5, 16, 15), Resource::Amber, (10, 16), 4500, 1500)
    }

    fn north_fish(state: u32) -> Op {
        gather(state, (8, 5, 16, 15), Resource::Fish, (10, 16), 4500, 1500)
    }

    fn south_amber(state: u32) -> Op {
        gather(state, (0, 41, 15, 11), Resource::Amber, (0, 50), 6000, 3000)
    }

    fn south_fish(state: u32) -> Op {
        gather(state, (0, 41, 15, 11), Resource::Fish, (0, 50), 6000, 3000)
    }

    const SOUTH: &[Event] = &[
        Event::Zone {
            state: 20,
            area: (0, 40, 19, 12),
        },
        Event::Attacked {
            state: 30,
            after: 375,
        },
    ];

    fn south_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(1, 42, 1500), (6, 45, 1500)];
        patrol(state, ROUND, SOUTH)
    }

    fn south_patrol_2(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(9, 43, 1500), (14, 46, 1500), (11, 50, 1500)];
        patrol(state, ROUND, SOUTH)
    }
}

/// miss013's brain (0x477930; its fleets' handlers precede it,
/// 0x473f40-0x476dc0), ported coarsely: three computer players, in the
/// north-west corner, the middle channel and the south, build, gather and
/// patrol their waters, answering fire.
mod miss013 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;

    pub const FLEETS: &[FleetDef] = &[
        FleetDef {
            group: 0,
            cells: &[(2, 4, 1, 1)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: corner_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[(6, 11, 1, 1)],
            program: corner_patrol,
            wants: &[],
        },
        FleetDef {
            group: 0,
            cells: &[(8, 7, 3, 3)],
            program: corner_patrol,
            wants: &[(ROYAL, [2; 3]), (RAMMER, [1, 1, 2])],
        },
        FleetDef {
            group: 1,
            cells: &[(14, 28, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: middle_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[(20, 26, 5, 3)],
            program: channel_patrol,
            wants: &[],
        },
        FleetDef {
            group: 1,
            cells: &[(17, 20, 3, 4)],
            program: channel_patrol,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[(44, 59, 2, 2)],
            program: yard,
            wants: &[],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: south_amber,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[],
            program: south_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        FleetDef {
            group: 2,
            cells: &[(39, 56, 4, 3)],
            program: south_patrol,
            wants: &[(ROYAL, [1, 2, 2])],
        },
        FleetDef {
            group: 2,
            cells: &[(38, 61, 2, 3), (38, 65, 3, 2)],
            program: south_raiders,
            wants: &[(ROYAL, [2; 3]), (BOARDER, [1, 2, 2]), (RAMMER, [1; 3])],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        for (x, y) in [(47, 0), (49, 1), (38, 0)] {
            scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
        }
        scene.add_deposit(0x191, (36, 1, 2, 2), data, caches);
    }

    fn yard(state: u32) -> Op {
        match state {
            0 => Op::Build { next: 11 },
            _ => Op::Wait {
                next: 0,
                ticks: 500,
            },
        }
    }

    fn patrol(state: u32, round: &'static [(i32, i32, u32)], watch: &'static [Event]) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                points: round,
                ticks: 4500,
                events: watch,
            },
            30 => Op::Attack {
                next: 0,
                target: Target::Attacker,
                ticks: 375,
                events: &[],
            },
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn gather(
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
                ok: 20,
                failed: 20,
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

    fn channel_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(22, 25, 750), (42, 21, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (7, 18, 6, 6),
            },
            Event::Zone {
                state: 20,
                area: (13, 18, 6, 6),
            },
            Event::Zone {
                state: 20,
                area: (18, 20, 6, 6),
            },
            Event::Zone {
                state: 20,
                area: (18, 26, 7, 5),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        patrol(state, ROUND, W)
    }

    fn corner_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(5, 11, 500), (14, 9, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (1, 1, 9, 7),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        patrol(state, ROUND, W)
    }

    fn corner_fish(state: u32) -> Op {
        gather(state, (2, 1, 8, 7), Resource::Fish, (5, 1), 4500, 3000)
    }

    fn middle_amber(state: u32) -> Op {
        gather(
            state,
            (16, 19, 14, 17),
            Resource::Amber,
            (13, 23),
            4500,
            1500,
        )
    }

    fn south_amber(state: u32) -> Op {
        gather(state, (45, 56, 5, 8), Resource::Amber, (48, 66), 6000, 3000)
    }

    fn south_fish(state: u32) -> Op {
        gather(
            state,
            (28, 52, 22, 20),
            Resource::Fish,
            (48, 66),
            6000,
            3000,
        )
    }

    fn south_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (39, 55, 1500),
            (38, 59, 1500),
            (39, 65, 1500),
            (38, 59, 1500),
        ];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (35, 52, 15, 5),
            },
            Event::Zone {
                state: 20,
                area: (35, 57, 8, 7),
            },
            Event::Zone {
                state: 20,
                area: (35, 64, 15, 6),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        patrol(state, ROUND, W)
    }

    fn south_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (16, 50, 1500),
            (27, 47, 1500),
            (44, 47, 1500),
            (27, 47, 1500),
        ];
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (35, 52, 15, 5),
            },
            Event::Zone {
                state: 20,
                area: (35, 57, 8, 7),
            },
            Event::Zone {
                state: 20,
                area: (35, 64, 15, 6),
            },
            Event::Zone {
                state: 20,
                area: (14, 49, 24, 14),
            },
            Event::Zone {
                state: 20,
                area: (37, 41, 9, 8),
            },
            Event::Attacked {
                state: 30,
                after: 375,
            },
        ];
        patrol(state, ROUND, W)
    }
}
