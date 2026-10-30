//! Player orders as data: what the player does through the interface,
//! applied to the scene directly in a single-player game and through the
//! network's turns in a multiplayer one (each player's machine applies
//! every player's orders at the same turn).

use crate::assets::GameData;
use crate::combat::Upgrade;
use crate::commands::Command;
use crate::magic::{Spell, Temple};
use crate::objects::{Caches, Resource};

use super::Scene;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Order {
    Move {
        ship: u32,
        x: f32,
        y: f32,
    },
    Waypoint {
        ship: u32,
        x: f32,
        y: f32,
    },
    Stop {
        id: u32,
    },
    /// The Building page's "default arming" checkbox.
    DefaultArming {
        on: bool,
    },
    Attack {
        ship: u32,
        target: u32,
    },
    Close {
        ship: u32,
        target: u32,
        board: bool,
    },
    Behaviour {
        ship: u32,
        command: Command,
    },
    Scout {
        ship: u32,
        x: f32,
        y: f32,
    },
    Patrol {
        ship: u32,
        x: f32,
        y: f32,
    },
    Zone {
        ship: u32,
        a: (f32, f32),
        b: (f32, f32),
        attack: bool,
    },
    GatherZone {
        ship: u32,
        a: (f32, f32),
        b: (f32, f32),
        resource: Resource,
    },
    Convoy {
        ship: u32,
        a: (f32, f32),
        b: (f32, f32),
    },
    /// Merchant `ship` unloads its goods at own port `port`.
    Unload {
        ship: u32,
        port: u32,
    },
    Gather {
        ship: u32,
        deposit: u32,
    },
    Take {
        ship: u32,
        target: u32,
    },
    Found {
        ship: u32,
        cx: i32,
        cy: i32,
        class: u32,
    },
    Repair {
        ship: u32,
        dock: u32,
    },
    Mend {
        site: u32,
        target: u32,
    },
    LoadCrew {
        ship: u32,
    },
    /// One unit of `item` from a settlement's stock onto a ship alongside.
    LoadItem {
        site: u32,
        ship: u32,
        item: u8,
    },
    /// Reload Object: own `ship` sails alongside own port, shipyard, dock
    /// or ship `target` and the two reload (the load panel).
    LoadObject {
        ship: u32,
        target: u32,
    },
    /// The load panel: `cargo` from holder `from` to holder `to`, two
    /// sides of one reloading (message 0x10ac).
    Transfer {
        from: u32,
        to: u32,
        cargo: super::Cargo,
    },
    /// The load panel's Kill item: holder `holder` throws `cargo` away.
    Discard {
        holder: u32,
        cargo: super::Cargo,
    },
    /// The load panel's Finish reload, for the ship or building `id`.
    EndLoad {
        id: u32,
    },
    /// Sell one `item` of settlement `site`'s stock.
    SellItem {
        site: u32,
        item: u8,
    },
    /// Sell an armour piece (or with `ram` a ram) of `level` from
    /// settlement `site`'s stock.
    SellFitting {
        site: u32,
        ram: bool,
        level: u8,
    },
    /// Sell settlement or town `site`, with its stock.
    SellTown {
        site: u32,
    },
    /// Workshop `workshop`'s settlement mends its stock (command 0x15).
    RepairStock {
        workshop: u32,
    },
    /// Order 10 to `id` (`Scene::halt`).
    Halt {
        id: u32,
    },
    /// Order 0x23 sub `sub` to fort `site` (its Guard and Fire on
    /// aggressor buttons, `Scene::fort_order`).
    FortBehaviour {
        site: u32,
        sub: u8,
    },
    BuildShip {
        site: u32,
        kind: u8,
    },
    BuildArgo {
        site: u32,
    },
    CancelShip {
        site: u32,
        kind: u8,
    },
    Train {
        site: u32,
        item: u8,
    },
    CancelTrain {
        site: u32,
        item: u8,
    },
    CancelRefit {
        smithy: u32,
        upgrade: Upgrade,
        level: u8,
    },
    Refit {
        smithy: u32,
        upgrade: Upgrade,
        level: u8,
    },
    Research {
        site: u32,
        upgrade: Upgrade,
    },
    Upgrade {
        id: u32,
    },
    PlaceBuilding {
        site: u32,
        class: u32,
        cx: i32,
        cy: i32,
    },
    /// At the temple `site` (its work, standing still while it is limed).
    LearnSpell {
        temple: Temple,
        #[serde(default)]
        site: Option<u32>,
    },
    CastFlame {
        #[serde(default)]
        site: Option<u32>,
    },
    CastWind {
        direction: u32,
    },
    CastOn {
        spell: Spell,
        target: u32,
    },
    CastArea {
        spell: Spell,
        cx: i32,
        cy: i32,
    },
    CastHazard {
        spell: Spell,
        cx: i32,
        cy: i32,
    },
}

impl Scene {
    /// Carries out `order` for the active player. `Ok` holds a count where
    /// the order has one (marines loaded).
    pub fn apply(
        &mut self,
        order: &Order,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<u32, String> {
        use Order::*;
        // A ship being built refuses its commands (0x4e2ba0 tests each with
        // 0x4e6700, and its capability word `+0x261` is 0x2000 alone:
        // moves 0x14, fire 0x15 / 0x16, board, ram, gather, dock,
        // transfer, found and treasure are dropped, nothing kept), but the
        // stop, which calls the building off (event 8, 0x4aa540), the
        // behaviour switches (0x23: applied at once by 0x4a9e30) and the
        // spells cast on it.
        let ship = match *order {
            Move { ship, .. }
            | Waypoint { ship, .. }
            | Attack { ship, .. }
            | Close { ship, .. }
            | Scout { ship, .. }
            | Patrol { ship, .. }
            | Zone { ship, .. }
            | GatherZone { ship, .. }
            | Convoy { ship, .. }
            | Gather { ship, .. }
            | Take { ship, .. }
            | Unload { ship, .. }
            | Found { ship, .. }
            | Repair { ship, .. }
            | LoadCrew { ship }
            | LoadObject { ship, .. } => Some(ship),
            _ => None,
        };
        if ship.is_some_and(|id| self.being_built(id)) {
            return Err("The ship is still being built".into());
        }
        if let Some(r) = self.one_event_a_tick(order, data, caches) {
            return r;
        }
        self.apply_now(order, data, caches)
    }

    /// A ship takes one order of the 0x1f0 group a tick (0x4ab720): its
    /// events are kept one a code (0x20 dock / found, 0x40 gather /
    /// treasure / unload, 0x10 board / ram, 0x80 move; a later one of the
    /// same code overwrites the record) and at its tick the first pending
    /// of 0x20, 0x40, 0x10, 0x80 runs, the others dropped with it (`+0x7d6
    /// &= ~m`), even when it refuses; a stop (event 8) runs before the
    /// group and drops what is pending. The port applies orders at once,
    /// so the ship's state before its first such order of the tick is
    /// kept and an order is run over it only if the original's would win.
    /// `None` for an order outside the group. (Not ported: the original
    /// runs them at the ship's next tick, a tick late; the fire records
    /// 0x100, which 0x4ab350 may or may not drop with them; the computer
    /// players' and brains' own calls, which do not come through here.)
    fn one_event_a_tick(
        &mut self,
        order: &Order,
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<Result<u32, String>> {
        use Order::*;
        let (ship, rank) = match *order {
            Found { ship, .. } | Repair { ship, .. } => (ship, 0),
            Gather { ship, .. } | Take { ship, .. } | Unload { ship, .. } => (ship, 1),
            Close { ship, .. } => (ship, 2),
            Move { ship, .. } => (ship, 3),
            Stop { id } => {
                let i = self.object_index(id)?;
                self.objects[i].ship.as_ref()?;
                // The group pending this tick goes; then the stop.
                if let Some(TickOrder::Ran { before, .. }) = self.tick_orders.remove(&id) {
                    self.objects[i].ship = Some(*before);
                }
                self.tick_orders.insert(id, TickOrder::Stopped);
                return None;
            }
            _ => return None,
        };
        let i = self.object_index(ship)?;
        let now = self.objects[i].ship.clone()?;
        let before = match self.tick_orders.get(&ship) {
            Some(TickOrder::Stopped) => return Some(Ok(0)),
            Some(TickOrder::Ran { rank: won, .. }) if *won < rank => return Some(Ok(0)),
            Some(TickOrder::Ran { before, .. }) => {
                let before = before.clone();
                self.objects[i].ship = Some((*before).clone());
                before
            }
            None => Box::new(now),
        };
        let r = self.apply_now(order, data, caches);
        if r.is_err() {
            // Refused (0x4aab00, 0x4ab250 returning early): the old task
            // stays, and what was pending went with the clear.
            if let Some(i) = self.object_index(ship) {
                self.objects[i].ship = Some((*before).clone());
            }
        }
        self.tick_orders
            .insert(ship, TickOrder::Ran { rank, before });
        Some(r)
    }

    /// Carries out `order` at once.
    fn apply_now(
        &mut self,
        order: &Order,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<u32, String> {
        use Order::*;
        let done = |r: Result<(), String>| r.map(|()| 0);
        match *order {
            Move { ship, x, y } => {
                if self.order_move(ship, x, y) {
                    Ok(0)
                } else {
                    Err("The ship cannot go there".into())
                }
            }
            Waypoint { ship, x, y } => {
                if self.order_waypoint(ship, x, y) {
                    Ok(0)
                } else {
                    Err("The ship cannot go there".into())
                }
            }
            Stop { id } => {
                self.stop(id);
                Ok(0)
            }
            DefaultArming { on } => {
                self.default_arming = on;
                Ok(0)
            }
            Attack { ship, target } => done(self.order_attack(ship, target)),
            Close {
                ship,
                target,
                board,
            } => done(self.order_close(ship, target, board)),
            Behaviour { ship, command } => done(self.toggle_behaviour(ship, command)),
            Scout { ship, x, y } => done(self.order_scout(ship, x, y)),
            Patrol { ship, x, y } => done(self.order_patrol(ship, x, y)),
            Zone { ship, a, b, attack } => done(self.order_zone(ship, a, b, attack)),
            GatherZone {
                ship,
                a,
                b,
                resource,
            } => done(self.order_gather_zone(ship, a, b, resource)),
            Convoy { ship, a, b } => done(self.order_convoy(ship, a, b)),
            Gather { ship, deposit } => {
                let r = self.order_gather(ship, deposit);
                if r.is_ok() {
                    self.gather_voice(ship, deposit);
                }
                done(r)
            }
            Take { ship, target } => done(self.order_take(ship, target)),
            Unload { ship, port } => done(self.order_unload(ship, port)),
            Found {
                ship,
                cx,
                cy,
                class,
            } => done(self.order_found(ship, cx, cy, class)),
            Repair { ship, dock } => done(self.order_repair(ship, dock)),
            Mend { site, target } => done(self.order_mend(site, target)),
            LoadCrew { ship } => self.load_crew(ship),
            LoadItem { site, ship, item } => done(self.load_item(site, ship, item)),
            LoadObject { ship, target } => done(self.order_load(ship, target)),
            Transfer { from, to, cargo } => done(self.transfer(from, to, cargo, data, caches)),
            Discard { holder, cargo } => done(self.discard(holder, cargo, data, caches)),
            EndLoad { id } => done(self.end_load(id)),
            SellItem { site, item } => done(self.sell_item(site, item)),
            SellFitting { site, ram, level } => done(self.sell_fitting(site, ram, level)),
            SellTown { site } => done(self.sell_town(site, data, caches)),
            RepairStock { workshop } => done(self.order_repair_stock(workshop)),
            Halt { id } => done(self.halt(id, data, caches)),
            FortBehaviour { site, sub } => done(self.fort_order(site, sub)),
            BuildShip { site, kind } => {
                let r = self.start_ship(site, kind);
                self.make_new_ships(data, caches);
                done(r)
            }
            BuildArgo { site } => {
                let r = self.start_argo(site);
                self.make_new_ships(data, caches);
                done(r)
            }
            CancelShip { site, kind } => done(self.cancel_ship(site, kind)),
            Train { site, item } => done(self.train(site, item)),
            CancelTrain { site, item } => done(self.cancel_training(site, item)),
            Refit {
                smithy,
                upgrade,
                level,
            } => done(self.refit(smithy, upgrade, level)),
            CancelRefit {
                smithy,
                upgrade,
                level,
            } => done(self.cancel_refit(smithy, upgrade, level)),
            Research { site, upgrade } => done(self.start_research(site, upgrade)),
            Upgrade { id } => done(self.upgrade(id, data, caches)),
            PlaceBuilding {
                site,
                class,
                cx,
                cy,
            } => done(self.place_building(site, class, cx, cy, data, caches)),
            LearnSpell { temple, site } => done(self.learn_spell(temple, site)),
            CastFlame { site } => done(self.cast_flame(site)),
            CastWind { direction } => done(self.cast_wind(direction)),
            CastOn { spell, target } => done(self.cast_on(spell, target, data, caches)),
            CastArea { spell, cx, cy } => done(self.cast_area(spell, cx, cy, data, caches)),
            CastHazard { spell, cx, cy } => done(self.cast_hazard(spell, cx, cy, data, caches)),
        }
    }
}

/// What a merchant answers when sent to gather (0x49d3c0, played on the
/// interface's voice channel): events 156-159 for amber, 160-163 for fish,
/// 164-167 for a leviathan, + 0, 1, 3, 2 by nation; only the Greeks' and
/// the Persians' are recorded, alike.
pub(super) const AMBER_VOICES: [&str; 5] = ["am4", "am1", "am3", "come4", "come5a"];
pub(super) const FISH_VOICES: [&str; 6] = ["fi3", "fi1", "fi4", "fish1", "fish2", "fish3"];
pub(super) const LEVIATHAN_VOICES: [&str; 8] = [
    "lev3", "lev1", "lev4", "levi1", "levi2", "levi3", "levi4", "levi5",
];

impl Scene {
    /// The viewing player's merchant `ship` answers an order to gather
    /// from `deposit`.
    fn gather_voice(&mut self, ship: u32, deposit: u32) {
        let (Some(i), Some(d)) = (self.object_index(ship), self.object_index(deposit)) else {
            return;
        };
        if !self.is_own(&self.objects[i]) {
            return;
        }
        // Nations 1 and 3 have voices.
        if !matches!(self.objects[i].owner, Some((1 | 3, _))) {
            return;
        }
        let voices: &[&'static str] = match self.objects[d].deposit.map(|d| d.resource) {
            Some(Resource::Amber) => &AMBER_VOICES,
            Some(Resource::Fish) => &FISH_VOICES,
            Some(Resource::Leviathan) => &LEVIATHAN_VOICES,
            None => return,
        };
        let pick = voices[self.gull_rng.next() as usize % voices.len()];
        self.sounds.push((pick, f32::NAN, f32::NAN));
    }
}

/// A ship's order of the 0x1f0 group this tick (`Scene::one_event_a_tick`):
/// one ran, of its rank (0 for 0x20 .. 3 for 0x80), over the ship as it was
/// before; or a stop came, which drops the rest of the tick's.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TickOrder {
    Ran {
        rank: u8,
        before: Box<crate::objects::Ship>,
    },
    Stopped,
}
