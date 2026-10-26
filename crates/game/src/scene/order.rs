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
    BuildShip {
        site: u32,
        kind: u8,
    },
    BuildArgo {
        site: u32,
    },
    Train {
        site: u32,
        item: u8,
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
    LearnSpell {
        temple: Temple,
    },
    CastFlame,
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
            Gather { ship, deposit } => done(self.order_gather(ship, deposit)),
            Take { ship, target } => done(self.order_take(ship, target)),
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
            BuildShip { site, kind } => done(self.start_ship(site, kind)),
            BuildArgo { site } => done(self.start_argo(site)),
            Train { site, item } => done(self.train(site, item)),
            Refit {
                smithy,
                upgrade,
                level,
            } => done(self.refit(smithy, upgrade, level)),
            Research { site, upgrade } => done(self.start_research(site, upgrade)),
            Upgrade { id } => done(self.upgrade(id, data, caches)),
            PlaceBuilding {
                site,
                class,
                cx,
                cy,
            } => done(self.place_building(site, class, cx, cy, data, caches)),
            LearnSpell { temple } => done(self.learn_spell(temple)),
            CastFlame => done(self.cast_flame()),
            CastWind { direction } => done(self.cast_wind(direction)),
            CastOn { spell, target } => done(self.cast_on(spell, target, data, caches)),
            CastArea { spell, cx, cy } => done(self.cast_area(spell, cx, cy, data, caches)),
            CastHazard { spell, cx, cy } => done(self.cast_hazard(spell, cx, cy, data, caches)),
        }
    }
}
