//! A ship's route stored (0x4a6330's store) and the step to the next cell
//! asked for (0x4a3a70), in the move tick and at the store alike.

use std::collections::{HashMap, HashSet, VecDeque};

use openconquest_formats::map::TILE_SIZE;

use crate::objects::{self, Ship};

use super::{sail, Scene};

/// What a step asked for (0x4a3a70) is weighed against: the cells ships
/// hold, the ships lying still, waiting or held, the charging rammers and
/// the hazards' cells; with what the asks leave to be done after them.
pub(super) struct Steps {
    /// One ship to a cell: the cells ships lie in, those they are already
    /// sailing into and the extra cells they reserved (`+0x5ab`).
    pub taken: HashMap<(i32, i32), u32>,
    /// Ships lying still (state 0), or waiting 50 ticks and so fallen to
    /// it (0x4a374a).
    pub lying: HashSet<u32>,
    /// Ships waiting at a cell for their next step.
    pub waiting: HashSet<u32>,
    /// Ships grappled, ramming or rammed.
    pub held_still: HashSet<u32>,
    /// Charging rammers and their targets, whose cell they may sail into.
    pub charging: HashMap<u32, u32>,
    /// Reefs' and whirlpools' cells, `true` where Jason sinks it.
    pub hazards: HashMap<(i32, i32), bool>,
    /// Cells whose whirlpool or reef a ship with Jason sinks (0x4a7630).
    pub sinks: Vec<(i32, i32)>,
    /// Ships lying still in a cell a ship asked for, to be pushed aside
    /// (0x4a3a70 -> 0x4b4b50): (lying, mover, the mover's wait before).
    pub asides: Vec<(u32, u32, f32)>,
}

/// The brake 0x4a2f60: move state 4 and `+0x284` (`sail::brake`), its
/// extra cell released when there is no distance to brake on.
pub(super) fn brake(ship: &mut Ship, at: (i32, i32), own: (i32, i32), width: i32) {
    ship.move_state = 4;
    match sail::brake(ship.way, at, own, ship.entering, ship.between.get(), width) {
        Some(b) => ship.brake = b,
        None => ship.ahead = None,
    }
}

fn cell_of(x: f32, y: f32) -> (i32, i32) {
    let t = TILE_SIZE as f32;
    ((x / t).floor() as i32, (y / t).floor() as i32)
}

impl Steps {
    /// Ship `id` (in its cell, not between cells) asks for the step into
    /// the next cell `c` of its route, whose last cell is `last`
    /// (0x4a3a70): free (no other ship holds it, or the target of its
    /// charge; no hazard, or one Jason sinks with no ship there), it is
    /// reserved (`+0x5ab`, `+0x5b6`, 0x499830), the plain acceleration
    /// set (`+0x280`), the wait (`+0x591`) and `+0x594` cleared. Refused,
    /// the wait grows a tick when the ship there lies still (pushing it
    /// aside, 0x4b4b50, the same player's of lower priority), waits or is
    /// held, and a tick more short of the route's last cell (or with
    /// `+0x594`). True if granted.
    pub fn ask(
        &mut self,
        id: u32,
        ship: &mut Ship,
        c: (i32, i32),
        last: Option<(i32, i32)>,
    ) -> bool {
        let free = match self.taken.get(&c) {
            Some(&other) => other == id || self.charging.get(&id) == Some(&other),
            None => true,
        };
        let free = free
            && match self.hazards.get(&c).copied() {
                None => true,
                Some(sinkable) => {
                    let sinks =
                        sinkable && ship.has_hero(objects::JASON) && !self.taken.contains_key(&c);
                    if sinks {
                        self.sinks.push(c);
                    }
                    sinks
                }
            };
        if free {
            self.taken.insert(c, id);
            ship.ahead = Some(c);
            ship.entering = c;
            ship.accel = None;
            ship.waited = 0.0;
            ship.attack_step = false;
        } else {
            if let Some(&other) = self.taken.get(&c).filter(|&&o| o != id) {
                if self.lying.contains(&other) {
                    self.asides.push((other, id, ship.waited));
                }
                if self.lying.contains(&other)
                    || self.waiting.contains(&other)
                    || self.held_still.contains(&other)
                {
                    ship.waited += crate::clock::TICK;
                }
            }
            if last != Some(c) || ship.attack_step {
                ship.waited += crate::clock::TICK;
            }
        }
        free
    }
}

impl Scene {
    /// The cells and ships a step is weighed against, as they stand.
    pub(super) fn steps(&self) -> Steps {
        let t = TILE_SIZE as f32;
        let mut taken: HashMap<(i32, i32), u32> = HashMap::new();
        for o in self.objects.iter().filter(|o| o.ship.is_some()) {
            taken.entry(cell_of(o.x, o.y)).or_insert(o.id);
        }
        for o in &self.objects {
            let Some(&(nx, ny)) = o.ship.as_ref().and_then(|s| s.path.front()) else {
                continue;
            };
            let here = cell_of(o.x, o.y);
            let centre = (here.0 as f32 * t + t / 2.0, here.1 as f32 * t + t / 2.0);
            if (o.x - centre.0).hypot(o.y - centre.1) > 0.5 {
                taken.entry(cell_of(nx, ny)).or_insert(o.id);
            }
        }
        // The cell after the one a ship has entered, reserved as it
        // entered it (`+0x5ab`, 0x4a3f40), while still on its route.
        for o in &self.objects {
            let Some(s) = o.ship.as_ref() else {
                continue;
            };
            let on_route = s
                .path
                .iter()
                .take(2)
                .any(|&(px, py)| Some(cell_of(px, py)) == s.ahead);
            if let Some(a) = s.ahead.filter(|_| on_route) {
                taken.entry(a).or_insert(o.id);
            }
        }
        // The original's move states (+0x58c): 0 lying still, 3 waiting at
        // a cell for the next (0x4a3560), 6 held by a boarding, ram or
        // dock; the rest under way (2 turning, 4 stopping, 5 sailing). A
        // ship waiting 50 ticks falls to lying still, its route kept, and
        // may be pushed aside like any (0x4a374a).
        let rest = crate::clock::ticks(50);
        let lying: HashSet<u32> = self
            .objects
            .iter()
            .filter(|o| {
                o.ship
                    .as_ref()
                    .is_some_and(|s| s.path.is_empty() || s.stalled >= rest)
            })
            .map(|o| o.id)
            .collect();
        let waiting: HashSet<u32> = self
            .objects
            .iter()
            .filter(|o| o.ship.as_ref().is_some_and(|s| s.stalled > 0.0))
            .map(|o| o.id)
            .collect();
        // Ships grappled, ramming or being rammed keep still (they are
        // moved by the boarding and the ram); a charging rammer may sail
        // into its target's cell.
        let held_still = self.held_by_contact();
        let charging: HashMap<u32, u32> = self
            .objects
            .iter()
            .filter_map(|o| {
                let r = o.ship.as_ref()?.ram_run?;
                (r.stage == objects::RamStage::Charge).then_some((o.id, r.target))
            })
            .collect();
        Steps {
            taken,
            lying,
            waiting,
            held_still,
            charging,
            hazards: self.hazard_cells(),
            sinks: Vec::new(),
            asides: Vec::new(),
        }
    }

    /// The pushes aside the asks left (0x4a3a70 -> 0x4b4b50), weighed on
    /// the wait before the ask; pushed, the wait starts again from 0 and
    /// counts this ask (0x4a3b56-0x4a3bab).
    pub(super) fn push_asides(&mut self, asides: Vec<(u32, u32, f32)>) {
        for (still, mover, before) in asides {
            let Some(m) = self.object_index(mover) else {
                continue;
            };
            let after = self.objects[m].ship.as_ref().unwrap().waited;
            self.objects[m].ship.as_mut().unwrap().waited = before;
            let pushed = self.push_aside(still, mover);
            self.objects[m].ship.as_mut().unwrap().waited =
                if pushed { after - before } else { after };
        }
    }

    /// Ship `i` takes the route `path` (0x4a6330's store:
    /// `+0x5c2`, its count `+0x5ba` and its last cell `+0x5b2`, the port's
    /// `goal`), then by its move state (0x4a6667): told to stop (1),
    /// stopping (4) or under way (5) and between cells (0x4e0b50 false),
    /// the route goes on from the cell it is entering (`+0x5be` 1) and it
    /// is under way (5); in its cell, with `+0x5be` 0, the turn 0x4e0100
    /// from its heading (`+0x5af`) to the route's first step (0x59791d,
    /// `+0x5b1`) over 1 asks for the step to route entry 1 at once
    /// (0x4a3a70(this, 1)), and refused it brakes (0x4a2f60) unless
    /// already stopping (4, kept); else (or granted) under way (5). Lying
    /// still or at a cell (0, 3): no turn, at the cell to leave it (3,
    /// `+0x596` 1), else turning (2, `+0x5b0` 0: 0x4a3320 picks the way
    /// round afresh), both through 0x4a3140 (the hull's frame delay
    /// `+0x7d` 2, the port's `SHIP_FRAME_MS`); turning (2): no turn 3
    /// (`+0x596` kept), else 2 and `+0x5b0` 0; all with `+0x5be` 1. Held
    /// (6) it is only stored.
    ///
    /// The port's route holds no entry for the cell it starts from; its
    /// first step is to the first point off its own cell. The turn itself
    /// is the port's mover's (`turn_ship`, 0x4a3320's rotations).
    pub(super) fn store_route(&mut self, i: usize, path: VecDeque<(f32, f32)>) {
        let (x, y, id) = (self.objects[i].x, self.objects[i].y, self.objects[i].id);
        let own = cell_of(x, y);
        let Some(ship) = self.objects[i].ship.as_mut() else {
            return;
        };
        ship.path = path;
        ship.adrift = false;
        // `+0x5b2`: the route's last cell (0x4a664d, 0x4a67c9).
        if let Some(&(lx, ly)) = ship.path.back() {
            ship.goal = Some(cell_of(lx, ly));
        }
        if matches!(ship.move_state, 1 | 4 | 5) && !ship.plans_from_own_cell() {
            ship.move_state = 5;
            return;
        }
        let Some(next) = ship
            .path
            .iter()
            .map(|&(px, py)| cell_of(px, py))
            .find(|&c| c != own)
        else {
            return;
        };
        let dir = super::grapple::direction(next.0 - own.0, next.1 - own.1).unwrap_or(0);
        // `+0x5af`: the hull's frame over 3 while it turns (0x4a3320).
        let heading = ship.rotation.map_or(ship.heading as i32, |r| r as i32 / 3);
        let diff = (heading - dir as i32).abs();
        let turn = if diff > 4 { 8 - diff } else { diff };
        match ship.move_state {
            0 | 3 if turn == 0 => {
                ship.between.set(true);
                ship.move_state = 3;
                return;
            }
            2 if turn == 0 => {
                ship.move_state = 3;
                return;
            }
            0 | 2 | 3 => {
                ship.move_state = 2;
                return;
            }
            1 | 4 | 5 => {}
            _ => return,
        }
        if turn <= 1 {
            ship.move_state = 5;
            return;
        }
        let last = ship.path.back().map(|&(lx, ly)| cell_of(lx, ly));
        let mut steps = self.steps();
        let width = self.grid.width as i32;
        let ship = self.objects[i].ship.as_mut().unwrap();
        // 0x4a3a70 first releases the extra cell it held (`+0x5ab`).
        ship.ahead = None;
        if steps.ask(id, ship, next, last) {
            ship.move_state = 5;
        } else if ship.move_state != 4 {
            brake(ship, (x.round() as i32, y.round() as i32), own, width);
        }
        // 0x4a7630 sends the hazard 0xda9e / 0xda9f inside the ask: it is
        // sinking before the store returns.
        for c in steps.sinks {
            self.sink_hazard_in_ask(c);
        }
        self.push_asides(steps.asides);
    }
}
