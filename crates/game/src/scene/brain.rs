//! The campaign missions' "brains", the original's per-mission scripts
//! (`brain\missNNN.cpp`), ported. A brain commands fleets of a computer
//! player's ships (those standing in given cells at the start, and ships
//! its builder fleet makes when a fleet wants more), each fleet running a
//! small program of states: wait, sail somewhere, gather in a zone, patrol
//! a round of points, attack a zone, build a ship, or a step of the
//! mission's own. Timings are game ticks (20 ms) as in the original's
//! `missCOM` primitives (0x47a3b0 and the others; see docs/FORMATS.md).

use openac_formats::map::TILE_SIZE;

use super::Scene;
use crate::assets::GameData;
use crate::objects::{Caches, Resource};

/// A rectangle of map cells: x, y, width, height.
pub type Cells = (i32, i32, i32, i32);

/// Go back to the state before (the original's 0xffffff0).
pub const PREVIOUS: u32 = u32::MAX;

/// A patrol's length taken from the fleet's variable 0.
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
    /// Gather a resource in a zone (0x47a570) until `ticks` have passed.
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
}

fn mission(name: &str) -> Option<Mission> {
    match name {
        "miss000" => Some(Mission {
            fleets: miss000::FLEETS,
            start: miss000::start,
        }),
        "miss001" => Some(Mission {
            fleets: miss001::FLEETS,
            start: miss001::start,
        }),
        "miss002" => Some(Mission {
            fleets: miss002::FLEETS,
            start: miss002::start,
        }),
        "miss003" => Some(Mission {
            fleets: miss003::FLEETS,
            start: miss003::start,
        }),
        "miss004" => Some(Mission {
            fleets: miss004::FLEETS,
            start: miss004::start,
        }),
        "miss005" => Some(Mission {
            fleets: miss005::FLEETS,
            start: miss005::start,
        }),
        "miss006" => Some(Mission {
            fleets: miss006::FLEETS,
            start: miss006::start,
        }),
        "miss007" => Some(Mission {
            fleets: miss007::FLEETS,
            start: miss007::start,
        }),
        "miss008" => Some(Mission {
            fleets: miss008::FLEETS,
            start: miss008::start,
        }),
        "miss009" => Some(Mission {
            fleets: miss009::FLEETS,
            start: miss009::start,
        }),
        "miss010" => Some(Mission {
            fleets: miss010::FLEETS,
            start: miss010::start,
        }),
        "miss011" => Some(Mission {
            fleets: miss011::FLEETS,
            start: miss011::start,
        }),
        "miss012" => Some(Mission {
            fleets: miss012::FLEETS,
            start: miss012::start,
        }),
        "miss013" => Some(Mission {
            fleets: miss013::FLEETS,
            start: miss013::start,
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

    /// Whether ship `id` belongs to a brain's fleet (the simple AI leaves
    /// it alone).
    pub fn in_fleet(&self, id: u32) -> bool {
        self.brain
            .as_ref()
            .is_some_and(|b| b.fleets.iter().any(|f| f.ships.contains(&id)))
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
        let (have_a, have_f) = self.funds(owner);
        let Some(r) = brain.requests.iter().position(|&(f, kind)| {
            let (amber, fish) = crate::economy::ship_cost(kind);
            groups.get(f) == groups.get(k) && have_a >= amber && have_f >= fish
        }) else {
            return false;
        };
        let (fleet, kind) = brain.requests[r];
        let (amber, fish) = crate::economy::ship_cost(kind);
        self.earn(owner, -(amber as i64), -(fish as i64));
        self.productions.push(super::Production {
            argo: false,
            site,
            ship: kind,
            elapsed: 0.0,
            total: crate::economy::SHIP_BUILD_SECONDS,
        });
        let brain = self.brain.as_mut().unwrap();
        brain.requests.remove(r);
        brain.building.push((site, kind, fleet));
        true
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
        let rec = openac_formats::ObjectRecord {
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

/// miss007's brain (0x45f770), ported coarsely: the barbarians' many yards
/// feed fleets that patrol their waters around the wind wizard; the
/// Persians in the north gather, guard and raid. (The original's yards
/// each build for one fleet, its wizard and several fleets use primitives
/// not yet decoded, 0x47aed0 and others.)
mod miss007 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16; 0x45ca00-0x45e120).
        FleetDef {
            group: 0,
            cells: &[
                (36, 26, 1, 1),
                (34, 31, 1, 1),
                (27, 37, 1, 1),
                (22, 51, 1, 1),
            ],
            program: yard,
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
            program: east_patrol,
            wants: &[(ROYAL, [1, 2, 2]), (RAMMER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: west_patrol,
            wants: &[(ROYAL, [1, 2, 2]), (RAMMER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: south_patrol,
            wants: &[(ROYAL, [1, 2, 2]), (RAMMER, [1, 1, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: raiders,
            wants: &[(ROYAL, [2, 2, 3]), (RAMMER, [1, 2, 2])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: raiders,
            wants: &[(ROYAL, [2, 2, 3]), (RAMMER, [1, 2, 2])],
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
            program: persian_guard,
            wants: &[(ROYAL, [2; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_south_guard,
            wants: &[(ROYAL, [1; 3]), (BOMBER, [1; 3]), (BOARDER, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_raiders,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1; 3]), (BOARDER, [1, 1, 2])],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        for (x, y) in [(44, 10), (23, 22), (21, 21), (8, 56), (19, 45), (15, 52)] {
            scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
        }
        for (x, y) in [(49, 9), (7, 60), (15, 46)] {
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

    fn patrol_with(state: u32, round: &'static [(i32, i32, u32)], watch: &'static [Event]) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            15 => Op::Attack {
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

    fn east_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(41, 23, 750), (33, 28, 1000), (42, 32, 750)];
        const W: &[Event] = &[Event::Zone {
            state: 15,
            area: (33, 21, 15, 14),
        }];
        patrol_with(state, ROUND, W)
    }

    fn west_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(1, 31, 1000), (8, 36, 1000)];
        const W: &[Event] = &[Event::Zone {
            state: 15,
            area: (0, 28, 9, 13),
        }];
        patrol_with(state, ROUND, W)
    }

    fn south_patrol(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (11, 46, 1000),
            (17, 50, 1000),
            (20, 39, 750),
            (21, 32, 1250),
        ];
        const W: &[Event] = &[Event::Zone {
            state: 15,
            area: (9, 34, 15, 15),
        }];
        patrol_with(state, ROUND, W)
    }

    /// Gathers, then raids the player now and then (between 18 and 34
    /// seconds' worth of ticks x 25, as the original's waits).
    fn raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(21, 32, 1000), (33, 28, 1000)];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 10,
                points: ROUND,
                ticks: 9000,
                events: &[],
            },
            10 => Op::Attack {
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

    /// The ships about the wind wizard's island guard it.
    fn wizard_guard(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Wait {
                next: 6,
                ticks: 250,
            },
            6 => Op::Custom(|scene, _| {
                if scene.human_in((14, 21, 10, 11)) {
                    20
                } else {
                    5
                }
            }),
            _ => Op::Attack {
                next: 0,
                target: Target::Area((14, 21, 10, 11)),
                ticks: 3000,
                events: &[],
            },
        }
    }

    fn persian_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (0, 0, 12, 15),
                resource: Resource::Amber,
                ticks: 4500,
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

    const PERSIAN_ROUND: &[(i32, i32, u32)] = &[(21, 5, 500), (12, 10, 750)];

    fn persian_guard(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (16, 3, 10, 12),
            },
            Event::Zone {
                state: 20,
                area: (0, 0, 15, 10),
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
                events: W,
            },
        }
    }

    fn persian_south_guard(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (20, 40, 20, 33),
            },
            Event::Zone {
                state: 20,
                area: (0, 0, 15, 10),
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
                events: W,
            },
        }
    }

    fn persian_raiders(state: u32) -> Op {
        const W: &[Event] = &[
            Event::Zone {
                state: 20,
                area: (16, 3, 10, 12),
            },
            Event::Zone {
                state: 20,
                area: (0, 0, 15, 10),
            },
        ];
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 10,
                points: PERSIAN_ROUND,
                ticks: 9000,
                events: W,
            },
            10 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 3000,
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
}

/// miss008's brain (0x463240), ported coarsely: the barbarians in the
/// north-west and the Persians in the east (who threaten Iolcus) each
/// build, gather, patrol and raid.
mod miss008 {
    use super::*;

    const MERCHANT: u8 = 1;
    const ROYAL: u8 = 2;
    const BOMBER: u8 = 3;
    const BOARDER: u8 = 4;
    const RAMMER: u8 = 5;
    const SHIP_KILLER: u8 = 6;

    pub const FLEETS: &[FleetDef] = &[
        // The barbarians (player 16; 0x461050-0x461a40).
        FleetDef {
            group: 0,
            cells: &[(12, 17, 1, 1), (18, 14, 1, 1), (20, 9, 1, 1)],
            program: yard,
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
            program: barbarian_sea,
            wants: &[(SHIP_KILLER, [1; 3]), (ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_home,
            wants: &[(ROYAL, [1, 2, 2]), (BOMBER, [1; 3])],
        },
        FleetDef {
            group: 0,
            cells: &[],
            program: barbarian_fish,
            wants: &[(MERCHANT, [1; 3])],
        },
        // The Persians (player 15; 0x461b30-0x462fb0).
        FleetDef {
            group: 1,
            cells: &[(47, 10, 1, 1), (53, 8, 1, 1), (53, 14, 1, 1)],
            program: yard,
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
            program: persian_raiders,
            wants: &[(ROYAL, [2, 2, 3]), (RAMMER, [1, 2, 2])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_north,
            wants: &[(RAMMER, [1; 3]), (ROYAL, [1, 2, 2]), (BOARDER, [1; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_middle,
            wants: &[(BOARDER, [2; 3])],
        },
        FleetDef {
            group: 1,
            cells: &[],
            program: persian_east,
            wants: &[(BOMBER, [1, 1, 2])],
        },
    ];

    pub fn start(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        for (x, y) in [(12, 41), (17, 35), (12, 34), (13, 36), (54, 31), (49, 31)] {
            scene.add_deposit(0x190, (x, y, 1, 1), data, caches);
        }
        for (x, y) in [(4, 34), (2, 42), (55, 33)] {
            scene.add_deposit(0x191, (x, y, 5, 5), data, caches);
        }
        if let Some(br) = scene.brain.as_mut() {
            br.fleets[12].vars = [7500, 4, 0, 0];
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

    fn barbarian_sea(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[
            (11, 21, 750),
            (16, 26, 1000),
            (26, 24, 750),
            (32, 19, 1000),
            (30, 13, 750),
            (23, 13, 1000),
        ];
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (8, 8, 26, 22),
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

    fn barbarian_home(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(2, 6, 750), (5, 11, 1000), (1, 10, 750)];
        const W: &[Event] = &[Event::Zone {
            state: 20,
            area: (0, 0, 22, 18),
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

    fn barbarian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 7,
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

    fn persian_amber(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (42, 2, 20, 30),
                resource: Resource::Amber,
                ticks: 4500,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to: (50, 23),
                ticks: 750,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 500,
            },
        }
    }

    fn persian_fish(state: u32) -> Op {
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Gather {
                next: 15,
                area: (40, 0, 22, 25),
                resource: Resource::Fish,
                ticks: 15000,
                events: &[],
            },
            15 => Op::Move {
                ok: 20,
                failed: 20,
                to: (50, 23),
                ticks: 500,
                events: &[],
            },
            _ => Op::Wait {
                next: 5,
                ticks: 250,
            },
        }
    }

    /// Patrols the south-east; once four strong and it has been a while,
    /// raids the player; answers fire (0x462630).
    fn persian_raiders(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(47, 33, 500), (36, 26, 750), (32, 32, 500), (37, 42, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 30,
                area: (40, 30, 22, 14),
            },
            Event::Size {
                state: 10,
                after: Some(0),
                cmp: Cmp::AtLeast,
                n: 1,
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
            10 => Op::Attack {
                next: 0,
                target: Target::HumanBuilding,
                ticks: 4500,
                events: &[],
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

    fn persian_north(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] =
            &[(39, 11, 500), (41, 16, 750), (46, 20, 500), (51, 11, 750)];
        const W: &[Event] = &[
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

    fn persian_middle(state: u32) -> Op {
        const ROUND: &[(i32, i32, u32)] = &[(44, 23, 750), (51, 25, 750), (48, 20, 750)];
        const W: &[Event] = &[
            Event::Zone {
                state: 30,
                area: (43, 20, 19, 8),
            },
            Event::Zone {
                state: 20,
                area: (30, 10, 30, 10),
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
            _ => Op::Attack {
                next: 0,
                target: Target::Seen,
                ticks: 1500,
                events: &[],
            },
        }
    }

    fn persian_east(state: u32) -> Op {
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
        match state {
            0 => Op::Behave { next: 5 },
            5 => Op::Patrol {
                timeout: 0,
                points: ROUND,
                ticks: 4500,
                events: W,
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
