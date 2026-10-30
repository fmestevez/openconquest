//! The human players of a multiplayer game. The scene keeps one active
//! player in its single-player fields (`human`, `amber`, `fish`, `magic`,
//! `permits`, `free_casts`, `flame`); each other human player's share of
//! them waits in a seat, their amber and fish in `purses` like a computer
//! player's. Every machine simulates with the first seat active and seats
//! each order's player to apply it, so all compute the same game; between
//! turns the local player's seat is active for the interface.

use std::collections::BTreeMap;

use openconquest_formats::PlayerInfo;

use super::{Order, Scene, Seen};
use crate::assets::GameData;
use crate::magic::{Magic, Spell};
use crate::objects::Caches;
use crate::permits::Permits;

/// One human player's own state while another's is active.
#[derive(Debug, Clone)]
pub struct Seat {
    pub player: PlayerInfo,
    magic: Magic,
    permits: Permits,
    free_casts: BTreeMap<Spell, u32>,
    flame: f32,
    sight: Vec<Seen>,
    stats: super::Stats,
}

impl Scene {
    /// Makes `players` the human players, the first of them the one active
    /// now (the scene's human). The others stop being computer players.
    pub fn seat(&mut self, players: &[PlayerInfo]) {
        self.seats = players
            .iter()
            .map(|&p| Seat {
                player: PlayerInfo {
                    computer: false,
                    ..p
                },
                magic: Magic::default(),
                permits: Permits::ALL,
                free_casts: BTreeMap::new(),
                flame: 0.0,
                sight: vec![Seen::Unexplored; self.sight.len()],
                stats: Default::default(),
            })
            .collect();
        self.active = 0;
        if let Some(first) = self.seats.first() {
            let p = first.player;
            let funds = self.purses.remove(&(p.nation, p.slot));
            if let Some(h) = self.human {
                if (h.nation, h.slot) != (p.nation, p.slot) {
                    self.purses
                        .insert((h.nation, h.slot), (self.amber, self.fish));
                    (self.amber, self.fish) = funds.unwrap_or((p.amber, p.fish));
                }
            }
            self.human = Some(p);
        }
    }

    /// Makes `player` the one human player of a single-player game (Custom
    /// missions' players, 0x53b940 on Start: side + 0x1a1d = 1 for the
    /// Human row): the mission's human takes a purse like a computer
    /// player's, `player` its own.
    pub fn play_as(&mut self, player: PlayerInfo) {
        let owner = (player.nation, player.slot);
        if let Some(h) = self.human {
            if (h.nation, h.slot) == owner {
                return;
            }
            self.purses
                .insert((h.nation, h.slot), (self.amber, self.fish));
        }
        (self.amber, self.fish) = self
            .purses
            .remove(&owner)
            .unwrap_or((player.amber, player.fish));
        self.human = Some(PlayerInfo {
            computer: false,
            ..player
        });
        self.refresh_sight();
    }

    /// The seat of player `owner`.
    pub fn seat_of(&self, owner: (u32, u32)) -> Option<usize> {
        self.seats
            .iter()
            .position(|s| (s.player.nation, s.player.slot) == owner)
    }

    /// Makes seat `k` the active human player.
    pub fn sit(&mut self, k: usize) {
        if k == self.active || k >= self.seats.len() {
            return;
        }
        let cur = self.active;
        let h = self.seats[cur].player;
        self.purses
            .insert((h.nation, h.slot), (self.amber, self.fish));
        self.swap_seat(cur);
        self.swap_seat(k);
        let p = self.seats[k].player;
        (self.amber, self.fish) = self.purses.remove(&(p.nation, p.slot)).unwrap_or_default();
        self.human = Some(p);
        self.active = k;
    }

    /// Exchanges the active player's own fields with seat `k`'s.
    fn swap_seat(&mut self, k: usize) {
        let s = &mut self.seats[k];
        std::mem::swap(&mut self.magic, &mut s.magic);
        std::mem::swap(&mut self.permits, &mut s.permits);
        std::mem::swap(&mut self.free_casts, &mut s.free_casts);
        std::mem::swap(&mut self.flame, &mut s.flame);
        std::mem::swap(&mut self.sight, &mut s.sight);
        std::mem::swap(&mut self.stats, &mut s.stats);
    }

    /// The lime on building `site` wore off, `since` after it struck: the
    /// inactive seats' temple work there goes on from then.
    pub(super) fn seat_lime_cleared(&mut self, site: u32, since: f32) {
        for (k, s) in self.seats.iter_mut().enumerate() {
            if k != self.active {
                s.magic.lime_cleared(site, since, s.permits);
            }
        }
    }

    /// The inactive seats' temples learn and their flame burns down.
    pub(super) fn seat_upkeep(&mut self, dt: f32, limed: &[u32]) {
        for (k, s) in self.seats.iter_mut().enumerate() {
            if k != self.active {
                s.magic.update(dt, limed, s.permits);
                s.flame = (s.flame - dt).max(0.0);
                s.flame += s.magic.take_flames() as f32 * crate::magic::FLAME_SECONDS;
            }
        }
    }

    /// Plays one multiplayer turn: each player's `orders` (seat, order),
    /// then `steps` steps of `step` seconds, the first seat active so every
    /// machine computes the same; then seat `local` again, with what it
    /// sees and how its game stands. Returns the turn's checksum and the
    /// local player's refused orders.
    pub fn play_turn(
        &mut self,
        orders: &[(usize, Order)],
        local: usize,
        steps: usize,
        step: f32,
        data: &GameData,
        caches: &mut Caches,
    ) -> (u64, Vec<String>) {
        let mine = self.outcome.take();
        let mut refused = Vec::new();
        for (seat, order) in orders {
            self.sit(*seat);
            if let (true, Err(e)) = (*seat == local, self.apply(order, data, caches)) {
                refused.push(e);
            }
        }
        self.sit(0);
        let said = self.messages.len();
        for _ in 0..steps {
            self.update(step, data, caches);
        }
        // What the game says while it plays is said to the first seat.
        if local != 0 {
            self.messages.truncate(said);
        }
        let check = self.checksum();
        // Messages and the outcome are the local player's.
        self.outcome = None;
        self.sit(local);
        self.refresh_sight();
        self.outcome = mine.or_else(|| self.judge());
        (check, refused)
    }

    /// A digest of the game's state, to tell whether two machines still
    /// play the same game (FNV-1a over the objects, funds and dice).
    pub fn checksum(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |v: u64| {
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
        };
        for o in &self.objects {
            eat(o.id as u64);
            eat(o.x.to_bits() as u64);
            eat(o.y.to_bits() as u64);
            eat(o.combat.as_ref().map_or(0, |c| c.hull.to_bits() as u64));
        }
        eat(self.amber as u64);
        eat(self.fish as u64);
        for (&(n, s), &(a, f)) in &self.purses {
            eat((n as u64) << 32 | s as u64);
            eat((a as u64) << 32 | f as u64);
        }
        eat(self.rng.0 as u64);
        h
    }

    /// Seconds of Flame left for player `owner`'s weapons.
    pub fn flame_of(&self, owner: Option<(u32, u32)>) -> f32 {
        let Some(owner) = owner else { return 0.0 };
        if self.human.is_some_and(|h| (h.nation, h.slot) == owner) {
            return self.flame;
        }
        if let Some(c) = self.computers.players.iter().find(|c| c.owner == owner) {
            return c.flame;
        }
        if let Some(&(_, flame)) = self.brain_flames.get(&owner) {
            if flame > 0.0 {
                return flame;
            }
        }
        self.seat_of(owner).map_or(0.0, |k| self.seats[k].flame)
    }
}
