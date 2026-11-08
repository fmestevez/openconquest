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
    /// Command 0x14 with priority byte 0xff (0x4cc650): a move that
    /// leaves the ship's priority as it was.
    MoveKeep {
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
    /// The selection's plain order (a move without Shift, a stop) as the
    /// navy takes it (0x49e230), sent once before the ships' own: the
    /// selection's group kept, its block ended (`keep_selection_group`).
    KeepGroup {
        ships: Vec<u32>,
    },
    /// A picked list's first order (0x49e230 with the navy's flag
    /// `+0x20` set): 0x49ca60 makes a new group of its ships, each leaving
    /// its old one, even when they are another group's (`new_group`).
    NewGroup {
        ships: Vec<u32>,
    },
    /// Shift-click on an own ship not in the selected group (0xc455,
    /// 0x49e850): it joins the group of `member` (0x49bd60, notice 300 by
    /// 0x4a1340; `join_group`).
    JoinGroup {
        member: u32,
        ship: u32,
    },
    /// Shift-click on a member of the selected group (0xc455, 0x49e850):
    /// it is taken out of the group (0x49bef0; `leave_zones`).
    LeaveGroup {
        ship: u32,
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
    /// The behaviour command `#` a fleet sends (0x4c42f0: sub, players
    /// `+1`, classes `+5`, third `+9`; `objects::Behaviour::command`).
    BehaviourCommand {
        ship: u32,
        sub: u8,
        players: u32,
        classes: u32,
        third: u32,
    },
    Scout {
        ship: u32,
        x: f32,
        y: f32,
    },
    /// The patrol button's click at cell `(x, y)` with `ships` selected
    /// (0x49d790 case 8; `Scene::order_patrol_route`).
    Patrol {
        ships: Vec<u32>,
        x: i32,
        y: i32,
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
    /// Command 0x1f sub 1 `{port, 1, prio}` (0x4e2ba0): `ship` unloads
    /// its goods and treasures at own port `port`, priority `prio` (the
    /// human's 0x4d74a0 75, the gather zone's 0x4c89b0 and the treasure
    /// block's 0x4d9fa0 0x5a).
    Unload {
        ship: u32,
        port: u32,
        #[serde(default = "human_unload_prio")]
        prio: u8,
    },
    /// The gather command (0x19 fish, 0x1a amber, 0x1d leviathan) from a
    /// gather zone: `{deposit, port, 0x5a}`.
    Gather {
        ship: u32,
        deposit: u32,
        #[serde(default)]
        port: u32,
    },
    /// Event 0x40 code 10 `{class, 10, deposit, 0, player 0xff, 0x5a}` a
    /// ship's own machine sends itself (vt+4(0x40, ..)): the amber's step
    /// 1 to the nearest standing amber (0x4b0590 step 1), the
    /// leviathan hunter's next prey (0x4ad510). Not a command: no 0x4e2ba0
    /// test, only 0x4aab00's of the class (fish bit 5, leviathan 6, amber
    /// 7, 0x4aacb6-0x4aacde); run by the ship's next 0x4ab720 like any
    /// event of the 0x40 code (`Scene::post_event`).
    GatherEvent {
        ship: u32,
        deposit: u32,
    },
    /// Event 0x40 code 0xc `{400, 0xc, port, 0, player, 0x5a}` a ship's
    /// own machine sends itself to unload at `port`: the unload's step 1
    /// whose port's cell holds it no more, to the one 0x4c5910 finds
    /// (0x4adef0 step 1), and the leviathan hunter's (0x4ad510). No
    /// test but 0x4aab00's look-up of the port.
    UnloadEvent {
        ship: u32,
        port: u32,
        prio: u8,
    },
    /// The human's Shift move click with `ships` selected, the ships of a
    /// group running a gather zone (0x49e230 case 0x14 -> 0x49de70 with
    /// key flag 4): cell `(x, y)` added to the zone's waypoints `+0x61`.
    ZoneWaypoint {
        ships: Vec<u32>,
        x: i32,
        y: i32,
    },
    /// The human's Shift move click at cell `(x, y)` with `ships`
    /// selected (0x49de70 with key flag 4; `Scene::order_shift_move`).
    ShiftMove {
        ships: Vec<u32>,
        x: i32,
        y: i32,
    },
    /// The human's move click at cell `(x, y)` without Shift with `ships`
    /// selected (0x49de70 -> 0x4ccc30; `Scene::order_plain_move`).
    FleetMove {
        ships: Vec<u32>,
        x: i32,
        y: i32,
    },
    /// The human's attack zone with `ships` selected (0x49d790 case 10):
    /// the selection's group given the attack block over the rectangle
    /// dragged between `a` and `b` (`human_attack`).
    AttackZone {
        ships: Vec<u32>,
        a: (f32, f32),
        b: (f32, f32),
    },
    /// The human's guard zone with `ships` selected (0x49d790 case 9):
    /// the selection's group given the guard block over the rectangle
    /// dragged between `a` and `b` (`human_guard`).
    GuardZone {
        ships: Vec<u32>,
        a: (f32, f32),
        b: (f32, f32),
    },
    /// The human's fleet command `command` (0x15 / 0x16 attack, 0x17
    /// board, 0x18 ram) at `target` with `ships` selected (0x49dda0): the
    /// selection's group given the fleet attack task (`fleet_attack`).
    /// `keys`: the click's key flags (4 Shift: the target queued).
    FleetAttack {
        ships: Vec<u32>,
        target: u32,
        command: u8,
        keys: u8,
    },
    /// The human's fleet command 0x20 (the found pointer) at cell `(x,
    /// y)` with `ships` selected (0x49dc90): the selection's group given
    /// the founding block (`fleet_found`). `code` the pointer's `+0xe1`
    /// (the building command less 0x15); `keys` the click's key flags
    /// (4 Shift: the site listed).
    FleetFound {
        ships: Vec<u32>,
        x: i32,
        y: i32,
        code: u8,
        keys: u8,
    },
    /// The human's gather click on a deposit with `ships` selected
    /// (0x49d3c0): a gather zone of the 3 x 3 cells about it. `keys`: the
    /// click's key flags (Sinput, the mouse message's `+0x14`: 4 Shift, 8
    /// Ctrl, 2 the right button held).
    GatherAt {
        ships: Vec<u32>,
        deposit: u32,
        #[serde(default)]
        keys: u8,
    },
    /// The merchants' zone buttons over a rectangle (0x49d790).
    GatherArea {
        ships: Vec<u32>,
        a: (f32, f32),
        b: (f32, f32),
        resource: Resource,
    },
    /// Message `*` (0x4d8b30): `ship` keeps within `range` cells of
    /// `ward`, priority 0.
    Follow {
        ship: u32,
        ward: u32,
        range: u8,
    },
    /// Command 0x28 sub 4 (0x4cf760): the ship forgets its last blow
    /// (`+0x691`, `+0x692`, `+0x634`).
    ClearBlow {
        ship: u32,
    },
    /// Command 0x29 sub 2 (0x4a7230): the ship keeps `cell` in its path
    /// memory (`+0x5cd`), once.
    Remember {
        ship: u32,
        cell: (i32, i32),
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
    /// Ship command 0x1e with byte 0 3 (0x4dbb40 for an order 200 block
    /// of mode 1): to the dock to be sold there (`+0x749 == 3`).
    Sell {
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
    /// Settlement or town `site` sells own building or docked ship
    /// `target` (order 0x34 byte 1).
    SellObject {
        site: u32,
        target: u32,
    },
    /// Sell settlement or town `site`, with its stock.
    SellTown {
        site: u32,
    },
    /// Workshop `workshop`'s settlement mends its stock (command 0x15).
    RepairStock {
        workshop: u32,
    },
    /// Command 0x1e a land fleet's repair part sends a fort or barbarian
    /// structure of its list (0x429f60): it mends itself.
    SelfMend {
        id: u32,
    },
    /// Command 0x20 a brain's ship part sends a barbarian structure of its
    /// list (0x428f20): it builds the ship, armed by default.
    StructureShip {
        site: u32,
        kind: u8,
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
    /// Fort `site` mends itself (its Repair, command 0x14,
    /// `Scene::fort_repair`).
    FortRepair {
        site: u32,
    },
    /// Fort `site` casts the spell it stores at cell `(cx, cy)` over
    /// `target` (order 0x22, `Scene::fort_cast`).
    FortCast {
        site: u32,
        spell: Spell,
        cx: i32,
        cy: i32,
        target: Option<u32>,
    },
    /// Ship `ship` casts spell index `spell` from its own magic at cell
    /// `(cx, cy)` over `target` (order 0x22, `Scene::ship_cast`).
    ShipCast {
        ship: u32,
        spell: u32,
        cx: i32,
        cy: i32,
        target: Option<u32>,
    },
    BuildShip {
        site: u32,
        kind: u8,
    },
    /// The town command 0x20 a land fleet's ship part sends (0x428f20),
    /// handed to yard `site`: paid unless `pay` is off (field 8), with
    /// the default-arming byte `arm`.
    YardShip {
        site: u32,
        kind: u8,
        pay: bool,
        arm: bool,
    },
    /// The town command 0x1e a land fleet's repair part sends (0x429f60):
    /// settlement or town `site` mends `target`, for any player.
    TownMend {
        site: u32,
        target: u32,
    },
    /// The town command 0x22 a land fleet's cast entry sends (0x42a500):
    /// town `site`'s idle temple casts kind `kind` (5-16) at `cell`, on
    /// `ship`, or (no cell) over `rect`, at `discount` hundredths off.
    TownSpell {
        site: u32,
        kind: u32,
        cell: (i32, i32),
        rect: Option<(i32, i32, i32, i32)>,
        ship: Option<u32>,
        discount: u32,
    },
    /// The town command 0x21 a computer's study part sends (0x42a860):
    /// town `site` of `owner` has its first idle school, workshop or
    /// smithy (0x441ba0 case 0x21) study `upgrade`.
    TownStudy {
        site: u32,
        owner: (u32, u32),
        upgrade: Upgrade,
    },
    /// The same for a temple's next spell (code 0xe, sub 7 or 8: the
    /// temple of the sea or of war).
    TownLearn {
        site: u32,
        owner: (u32, u32),
        temple: Temple,
    },
    /// The town command 0x28 with payload 3 a land fleet's held ship part
    /// sends (0x428f20 at 0x4295cb): town `site` of `owner` clears its
    /// berths (0x4415a0).
    TownBerths {
        site: u32,
        owner: (u32, u32),
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
        /// The facing a town's own placing chose (the land fleet's
        /// building part, 0x428a70): the cell is not looked over again.
        #[serde(default)]
        facing: Option<u32>,
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

impl Order {
    /// The ship a ship command goes to.
    pub fn ship(&self) -> Option<u32> {
        use Order::*;
        match *self {
            Move { ship, .. }
            | MoveKeep { ship, .. }
            | Waypoint { ship, .. }
            | Attack { ship, .. }
            | Close { ship, .. }
            | Scout { ship, .. }
            | Zone { ship, .. }
            | GatherZone { ship, .. }
            | Convoy { ship, .. }
            | Gather { ship, .. }
            | GatherEvent { ship, .. }
            | UnloadEvent { ship, .. }
            | Follow { ship, .. }
            | Take { ship, .. }
            | Unload { ship, .. }
            | Found { ship, .. }
            | Repair { ship, .. }
            | Sell { ship, .. }
            | LoadCrew { ship }
            | LoadObject { ship, .. } => Some(ship),
            _ => None,
        }
    }
}

/// Ticks at the start of a game whose commands are held (0x521570: while
/// the Players object's `+0x3e` is below the speed's `+0x14`, 2 at the
/// default speed 5, it counts up and delivers nothing; single player never
/// zeroes it again).
pub const HOLD: u32 = 2;

impl Scene {
    /// Carries out `order` for the active player. `Ok` holds a count where
    /// the order has one (marines loaded). The interface's commands reach
    /// their objects at the start of the next tick (0x520cc0's outbox,
    /// delivered by 0x5228c0 before the tick runs), after the commands
    /// the brains and computer players gave in the last one: the port
    /// carries them out at once, the last tick's having been delivered at
    /// its end (`Scene::deliver`). In the first ticks of a game they are
    /// held with the rest (`HOLD`). (The Building page's switch is the
    /// interface's own: read into the yard command's byte 0xd at the
    /// click, never held.)
    pub fn apply(
        &mut self,
        order: &Order,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<u32, String> {
        // A human's order to ships makes them a group of their own
        // (0x49ca60), out of their gather zone.
        match order {
            Order::GatherAt { .. }
            | Order::GatherArea { .. }
            | Order::GatherZone { .. }
            | Order::ZoneWaypoint { .. }
            | Order::ShiftMove { .. }
            | Order::FleetMove { .. }
            | Order::Patrol { .. }
            | Order::AttackZone { .. }
            | Order::GuardZone { .. }
            | Order::FleetAttack { .. }
            | Order::FleetFound { .. } => {}
            Order::Behaviour { .. } | Order::LoadCrew { .. } | Order::LoadObject { .. } => {}
            // The selection's plain order: its group kept (0x49e230),
            // made at once at the click, before the ships' own orders.
            Order::KeepGroup { ships } => {
                self.keep_selection_group(ships);
                return Ok(0);
            }
            // The selection's group, made, joined or left at once.
            Order::NewGroup { ships } => {
                self.new_group(ships);
                return Ok(0);
            }
            Order::JoinGroup { member, ship } => {
                self.join_group(*member, *ship);
                return Ok(0);
            }
            Order::LeaveGroup { ship } => {
                self.leave_zones(&[*ship]);
                return Ok(0);
            }
            // After `KeepGroup` a plain order (0x49e230's default case: the
            // move, the stop 10, the treasure 0x22) finds its ship in the
            // selection's bare group, which it keeps.
            Order::Move { ship: id, .. } | Order::Stop { id } | Order::Take { ship: id, .. }
                if self.human_groups.iter().any(|g| g.ships.contains(id)) => {}
            Order::Stop { id } => self.leave_zones(&[*id]),
            o => {
                if let Some(id) = o.ship() {
                    self.leave_zones(&[id]);
                }
            }
        }
        if self.ticks < HOLD && !matches!(order, Order::DefaultArming { .. }) {
            self.outbox.push(order.clone());
            return Ok(0);
        }
        self.carry_out(order, data, caches)
    }

    /// Gives `order` as a brain or computer player does (0x520cc0): it
    /// goes into the outbox and reaches the ship or building at the next
    /// tick, in the order given, after nobody in this tick has seen it.
    pub(super) fn post(&mut self, order: Order) {
        self.outbox.push(order);
    }

    /// A ship's machine sends itself an event (vt+4: its record written,
    /// `+0x7d6` set) for its next tick's 0x4ab720, where it is the
    /// group's like any order (a busy task drops it, `task_busy`). Written
    /// at the machine's tick, it is under the commands delivered at the
    /// next tick's start (0x5228c0 -> 0x4e2ba0 -> vt+4 overwrite a
    /// record of their code), so it goes before every command in the
    /// outbox, after the events already there.
    pub(super) fn post_event(&mut self, order: Order) {
        let at = self
            .outbox
            .iter()
            .take_while(|o| matches!(o, Order::GatherEvent { .. } | Order::UnloadEvent { .. }))
            .count();
        self.outbox.insert(at, order);
    }

    /// Delivers the outbox at a tick's turn (0x521570, 0x5228c0): each
    /// command in order to its object's handler, refusals dropped
    /// silently; a ship's 0x1f0 group resolves from here
    /// (`one_event_a_tick`). Nothing in the first `HOLD` ticks.
    pub(super) fn deliver(&mut self, data: &GameData, caches: &mut Caches) {
        if self.ticks < HOLD {
            return;
        }
        self.tick_orders.clear();
        for order in std::mem::take(&mut self.outbox) {
            let _ = self.carry_out(&order, data, caches);
        }
    }

    /// Hands `order` to its object, as the command's handler does.
    fn carry_out(
        &mut self,
        order: &Order,
        data: &GameData,
        caches: &mut Caches,
    ) -> Result<u32, String> {
        // The command handler 0x4e2ba0 tests a ship's command on its
        // capability word (0x4e6700 on `+0x261`, `Scene::capabilities`)
        // and drops a refused one silently, nothing posted (`word_test`).
        // A ship being built (0x2000 alone), being sold (0), netting a
        // leviathan, landing, docking, taking a treasure, loading,
        // ramming, boarding or boarded, stone or charmed so refuses them.
        if let Some(passed) = self.word_test(order) {
            if !passed {
                return Err("The ship cannot do that now".into());
            }
            // Passed, its event is still never run by a ship being built
            // or sold: 0x4ab720 runs the group 0x1f0 only for `+0x251`
            // neither 1 nor 2 (0x4ab7e1) with `+0x261 & 0x1f0` (0x4ab7ef),
            // else the song's alone (kind 0x17 with `+0x25c`).
            if order.ship().is_some_and(|id| self.being_built(id)) {
                return Err("The ship is still being built".into());
            }
        } else {
            // The port's own orders, not commands of 0x4e2ba0 (a waypoint,
            // the scout, zones, the convoy, the follow, the crew load):
            // held as before through the holds. (Not decoded: what of the
            // original stands for them.)
            let ship = order.ship();
            if ship.is_some_and(|id| self.being_sold(id)) {
                return Err("The ship is being sold".into());
            }
            if ship.is_some_and(|id| self.being_built(id)) {
                return Err("The ship is still being built".into());
            }
            if ship.is_some_and(|id| self.hunt_holds(id)) {
                return Err("The ship is hunting".into());
            }
            if ship.is_some_and(|id| self.landing_holds(id)) {
                return Err("The ship is landing".into());
            }
            if ship.is_some_and(|id| self.load_holds(id))
                && !matches!(order, Order::LoadCrew { .. })
            {
                return Err("The ship is reloading".into());
            }
        }
        if let Some(r) = self.one_event_a_tick(order, data, caches) {
            return r;
        }
        self.apply_now(order, data, caches)
    }

    /// The test 0x4e2ba0 makes of ship command `order` (0x4e6700 on the
    /// ship's word, `Scene::capabilities`): move 0x14 bit 10; fire 0x15 /
    /// 0x16 bit 11 (then a weapon at the position asked, 0x4bc270, and
    /// not limed, `+0x258`: `order_attack`); board 0x17 bit 4 (then
    /// marines, 0x4bc270(1), or a hero who fights, 0x4b4dd0:
    /// `order_close`); ram 0x18 bit 3; the fish gather 0x19 bit 5, the
    /// amber 0x1a and leviathan 0x1c / 0x1d bit 6; repair and sale 0x1e
    /// bit 1; unload 0x1f kind 1 (then cargo aboard, `+0x47f..+0x484`)
    /// and reload alongside kind 2 bit 2; found 0x20 bit 0; the treasure
    /// take 0x22 kind 0x10 bit 15 with room (0x4be800 modes 6 and 7), the
    /// spells (0x22 other kinds) none. The stop (case 10, event 8) and
    /// the behaviour switches (0x23) take no test here; the stop's is
    /// its event's (0x4aa540, `Scene::stop`). The events test again: the
    /// dock / found 0x4aa990 kind 1 bit 1, kind 2 bit 0 (0x4aa99e,
    /// 0x4aa9af: 0x4e2ba0's own), the gathers 0x4aab00 by class (the
    /// amber's bit 7 besides). `None` for an order that is not such a
    /// command, or not to a ship.
    pub(super) fn word_test(&self, order: &Order) -> Option<bool> {
        use Order::*;
        let i = self.object_index(order.ship()?)?;
        let s = self.objects[i].ship.as_ref()?;
        let caps = self.capabilities(i);
        let bit = match *order {
            Move { .. } | MoveKeep { .. } => 0x400,
            Attack { .. } => 0x800,
            Close { board: true, .. } => 0x10,
            Close { board: false, .. } => 8,
            Gather { deposit, .. } => {
                let d = self.object_index(deposit);
                match d.and_then(|d| super::gather::deposit_class(&self.objects[d])) {
                    Some(Resource::Fish) => 0x20,
                    // The event tests the class again (0x4aab00 at
                    // 0x4aacb6-0x4aacde): fish 0x191 bit 5, leviathan
                    // 0x192 bit 6, amber 400 bit 7, a refused one dropped
                    // (`+0x7d6 &= ~0x40` first); the amber so wants bits
                    // 6 (0x4e6700) and 7. The port runs the event at
                    // delivery, so the two tests read one word.
                    Some(Resource::Amber) => 0x40 | 0x80,
                    _ => 0x40,
                }
            }
            // An event, past 0x4e2ba0: 0x4aab00's own tests at its run.
            GatherEvent { .. } | UnloadEvent { .. } => return Some(true),
            Repair { .. } | Sell { .. } => 2,
            Unload { .. } | LoadObject { .. } => 4,
            Found { .. } => 1,
            Take { .. } => {
                let room = s.treasures.len() < crate::objects::treasure_room(s.kind)
                    || s.heroes.len() < crate::objects::hero_room(s.kind);
                return Some(caps & 0x8000 != 0 && room);
            }
            _ => return None,
        };
        Some(caps & bit == bit)
    }

    /// A ship takes one order of the 0x1f0 group a tick (0x4ab720): its
    /// events are kept one a code (0x20 dock / found, 0x40 gather /
    /// treasure / unload, 0x10 board / ram, 0x80 move; a later one of the
    /// same code overwrites the record) and at its tick the first pending
    /// of 0x20, 0x40, 0x10, 0x80 runs, the others dropped with it (`+0x7d6
    /// &= ~m`), even when it refuses; a stop (event 8) runs before the
    /// group and drops what is pending. The fire records (0x100: commands
    /// 0x15 / 0x16) run after the group, and are dropped with it (`m =
    /// 0x1f0`) when 0x4ab350 finds the last record of an armed position
    /// (`+0x289 + 3k`) of kind 3: always so for an attack on a ship
    /// (0x16 sub 7, every position kind 3), so an attack goes with any
    /// order of the group in its tick, whichever came first. The port
    /// applies orders as they are delivered, so the ship's state before
    /// its first such order of the batch (the last tick's outbox, then
    /// the player's) is kept and an order is run over it only if the
    /// original's would win. Before the group, 0x4abe50: a ship whose
    /// running task is busy (`Scene::task_busy`) has it wound down
    /// (0x4abc60, `Scene::wind_down`) and loses the batch's orders of the
    /// group, the fire records with them. `None` for an order outside
    /// the group. (Not ported: the events wait in the original until the
    /// ship's own turn in the tick, and 0x4ab720's gate (0x4ab7ef) keeps
    /// them pending there while `+0x261 & 0x1f0` is clear, till the word
    /// comes back; the port runs them at delivery, so a hold started
    /// between the two by an object earlier in the Sapp's walk (0x542270,
    /// its list `+0x10` in making order by 0x541b40: the navy, the
    /// brain, then the map's objects by record and the computers) does not
    /// keep them. Every hold the port has refuses the group's commands at
    /// 0x4e2ba0 already, so nothing passes delivery to wait at the gate.
    /// With the group's event refused at its run, the old task, which
    /// 0x4abe50 has taken off `+0x25d` (`&= 0xffffff0f`), ends, and the
    /// 0x80 move's machine with it (`Ship::adrift`).)
    fn one_event_a_tick(
        &mut self,
        order: &Order,
        data: &GameData,
        caches: &mut Caches,
    ) -> Option<Result<u32, String>> {
        use Order::*;
        let (ship, rank) = match *order {
            Found { ship, .. } | Repair { ship, .. } | Sell { ship, .. } => (ship, 0),
            Gather { ship, .. }
            | GatherEvent { ship, .. }
            | Take { ship, .. }
            | Unload { ship, .. }
            | UnloadEvent { ship, .. } => (ship, 1),
            Close { ship, .. } => (ship, 2),
            Move { ship, .. } | MoveKeep { ship, .. } => (ship, 3),
            Attack { ship, .. } => (ship, 4),
            Stop { id } => {
                let i = self.object_index(id)?;
                self.objects[i].ship.as_ref()?;
                // Refused without bit 13 (0x4aa540), the group pending
                // stays (`+0x7d6` cleared only on taking it).
                if self.capabilities(i) & 0x2000 == 0 {
                    return None;
                }
                // The group pending this tick goes; then the stop.
                if let Some(TickOrder::Ran { before, .. } | TickOrder::Dropped { before }) =
                    self.tick_orders.remove(&id)
                {
                    self.objects[i].ship = Some(*before);
                }
                self.tick_orders.insert(id, TickOrder::Stopped);
                return None;
            }
            _ => return None,
        };
        let i = self.object_index(ship)?;
        self.objects[i].ship.as_ref()?;
        let before = match self.tick_orders.get(&ship) {
            Some(TickOrder::Stopped) => return Some(Ok(0)),
            Some(TickOrder::Ran { rank: won, .. }) if *won < rank => return Some(Ok(0)),
            Some(TickOrder::Ran { before, .. }) => {
                let before = before.clone();
                self.objects[i].ship = Some((*before).clone());
                before
            }
            Some(TickOrder::Dropped { .. }) => return Some(Ok(0)),
            None => {
                // The group's turn: 0x4abe50 first; a busy task is wound
                // down (0x4abc60) and the group dropped (`Scene::task_busy`).
                let before = Box::new(self.objects[i].ship.clone().unwrap());
                let busy = self.task_busy(i);
                if busy {
                    self.wind_down(i);
                }
                // Then 0x4abe00 (with the fire task on and afloat),
                // whatever the order does or whether it was dropped.
                let s = self.objects[i].ship.as_mut().unwrap();
                s.fire_reset();
                if busy {
                    self.tick_orders.insert(ship, TickOrder::Dropped { before });
                    return Some(Ok(0));
                }
                Box::new(s.clone())
            }
        };
        let r = self.apply_now(order, data, caches);
        if r.is_err() {
            // Refused (0x4aab00, 0x4ab250 returning early): what was
            // pending went with the clear, and 0x4abe50 had already taken
            // the tasks 0x10-0x80 off `+0x25d` (`&= 0xffffff0f`, 0x4abf41),
            // so the old one's machine is no longer run (the ship tick
            // 0x4ab930 runs the 0x20, 0x10, 0x40 and 0x80 machines,
            // 0x4b17b0, 0x4c1370 / 0x4b4da0 / 0x4b7370, 0x4ac2d0, 0x4a5f30,
            // by those bits): it is over where it stood, nothing of it
            // freed or wound down (no 0x4abc60, no stop of the move under
            // way, `+0x400`'s 0x4a3f40). The take's step 5 has its bit off
            // already (0x4b0e40) and stays as it was.
            if let Some(i) = self.object_index(ship) {
                let mut s = (*before).clone();
                use crate::objects::Task::*;
                if matches!(
                    s.task,
                    Some(
                        Gather { .. }
                            | Board { .. }
                            | Ram { .. }
                            | LoadAt { .. }
                            | LoadRuins { .. }
                            | Found { .. }
                            | Repair { .. }
                    )
                ) || matches!(s.task, Some(Take { step, .. }) if step != 5)
                {
                    s.task = None;
                }
                // The 0x80 move's 0x4a5f30 is off too: the route it has
                // sails on (0x4a3f40), planned again by nothing.
                s.pending = None;
                s.adrift = true;
                self.objects[i].ship = Some(s);
            }
        }
        self.tick_orders
            .insert(ship, TickOrder::Ran { rank, before });
        Some(r)
    }

    /// Whether ship `i`'s running task is busy at its order group's turn
    /// (0x4abe50 at 0x4abe50-0x4abea1): the task of 0x20 with `+0x739`,
    /// 0x40 with `+0x77b`, 0x10 with `+0x6a0`, 0x80 with `+0x58b` or
    /// 0x100 with `+0x5e2` set. Busy, 0x4abe50 keeps the first pending of
    /// 0x20, 0x40, 0x10, 0x100, 0x80 in `+0x265`, winds the task down
    /// (0x4abc60) and zeroes `+0x265` again whenever `+0x7d6` is not 0
    /// (0x4abf00-0x4abf0a), which it is with that event pending (0x4abc60
    /// and its callees 0x4c1950, 0x4a3800, 0x4b93f0, 0x4a6330 leave
    /// `+0x7d6`), then drops the group (`&= 0xfffffe0f`): the orders are
    /// lost, not deferred (0x4abf21's restore from `+0x265` is never
    /// reached with a code; only 0x4abe50 writes it nonzero). No
    /// instruction writes `+0x739`, `+0x58b` or `+0x5e2` (the listing's
    /// only references are 0x4abe50's reads), so those tasks are never
    /// busy. The 0x40 task's byte is the fish gather's (`Ship::net_busy`),
    /// which no other 0x40 machine writes, so one a full haul left set
    /// makes any 0x40 task busy: the gathers (fish, leviathan, amber, and
    /// the unload they go on to), the unload order and the treasure take
    /// (`+0x25d |= 0x40` at 0x4aab00's end, 0x4ab1ef; the take's step 5
    /// takes it off again, 0x4b0e40). The 0x10 task's is
    /// `Ship::hook_busy`, read for a board, ram or load order.
    pub(super) fn task_busy(&self, i: usize) -> bool {
        let Some(s) = self.objects[i].ship.as_ref() else {
            return false;
        };
        // The 0x10 task (`+0x25d & 0x10`, set by 0x4aa840): a board, ram
        // or load order.
        let ten = matches!(
            s.task,
            Some(
                crate::objects::Task::Board { .. }
                    | crate::objects::Task::Ram { .. }
                    | crate::objects::Task::LoadAt { .. }
                    | crate::objects::Task::LoadRuins { .. }
            )
        );
        // The 0x40 task (`+0x25d & 0x40`).
        let forty = match s.task {
            Some(crate::objects::Task::Gather { .. }) => true,
            Some(crate::objects::Task::Take { step, .. }) => step != 5,
            _ => false,
        };
        (s.net_busy && forty) || (s.hook_busy && ten)
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
            MoveKeep { ship, x, y } => {
                let kept = self
                    .object_index(ship)
                    .and_then(|i| self.objects[i].ship.as_ref())
                    .map(|s| s.priority);
                if self.order_move(ship, x, y) {
                    if let (Some(p), Some(i)) = (kept, self.object_index(ship)) {
                        self.objects[i].ship.as_mut().unwrap().priority = p;
                    }
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
            KeepGroup { ref ships } => {
                self.keep_selection_group(ships);
                Ok(0)
            }
            NewGroup { ref ships } => {
                self.new_group(ships);
                Ok(0)
            }
            JoinGroup { member, ship } => {
                self.join_group(member, ship);
                Ok(0)
            }
            LeaveGroup { ship } => {
                self.leave_zones(&[ship]);
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
            // 0x4e2ba0 case 0x23: no capability test (0x4e6700 is not
            // called), so a ship being built takes it too.
            BehaviourCommand {
                ship,
                sub,
                players,
                classes,
                third,
            } => {
                // A fort or barbarian structure: its own case 0x23
                // (0x419f80, 0x40ef70).
                if self.object_index(ship).is_some_and(|i| {
                    self.objects[i].ship.is_none() && self.objects[i].holds_units()
                }) {
                    let masks = crate::combat::FortMasks {
                        players,
                        classes,
                        kinds: third,
                    };
                    return done(self.fort_command(ship, sub, masks));
                }
                let s = self
                    .object_index(ship)
                    .and_then(|i| self.objects[i].ship.as_mut())
                    .ok_or("Not a ship")?;
                let firing = s.fire_task();
                s.behaviour.command(sub, players, classes, third, firing);
                Ok(0)
            }
            Scout { ship, x, y } => done(self.order_scout(ship, x, y)),
            Patrol { ref ships, x, y } => done(self.order_patrol_route(ships, x, y)),
            // An attack zone given to one ship (an outbox of an older
            // save) is its group's.
            Zone {
                ship,
                a,
                b,
                attack: true,
            } => done(self.order_attack_zone(&[ship], a, b)),
            // A guard zone given to one ship (an outbox of an older save)
            // is its group's.
            Zone { ship, a, b, .. } => done(self.order_guard_zone(&[ship], a, b)),
            AttackZone { ref ships, a, b } => done(self.order_attack_zone(ships, a, b)),
            GuardZone { ref ships, a, b } => done(self.order_guard_zone(ships, a, b)),
            FleetAttack {
                ref ships,
                target,
                command,
                keys,
            } => done(self.order_fleet_attack(ships, target, command, keys)),
            FleetFound {
                ref ships,
                x,
                y,
                code,
                keys,
            } => done(self.order_human_found(ships, x, y, code, keys)),
            GatherZone {
                ship,
                a,
                b,
                resource,
            } => done(self.order_gather_area(&[ship], a, b, resource)),
            GatherArea {
                ref ships,
                a,
                b,
                resource,
            } => done(self.order_gather_area(ships, a, b, resource)),
            ZoneWaypoint { ref ships, x, y } => done(self.order_zone_waypoint(ships, x, y)),
            ShiftMove { ref ships, x, y } => done(self.order_shift_move(ships, x, y)),
            FleetMove { ref ships, x, y } => done(self.order_plain_move(ships, x, y)),
            GatherAt {
                ref ships,
                deposit,
                keys,
            } => {
                let r = self.order_gather_at(ships, deposit, keys);
                if r.is_ok() {
                    if let Some(&first) = ships.first() {
                        self.gather_voice(first, deposit);
                    }
                }
                done(r)
            }
            Follow { ship, ward, range } => done(self.order_follow(ship, ward, range)),
            Remember { ship, cell } => {
                if let Some(s) = self
                    .object_index(ship)
                    .and_then(|i| self.objects[i].ship.as_mut())
                {
                    if !s.remembered.contains(&cell) {
                        s.remembered.push(cell);
                    }
                }
                Ok(0)
            }
            ClearBlow { ship } => {
                if let Some(s) = self
                    .object_index(ship)
                    .and_then(|i| self.objects[i].ship.as_mut())
                {
                    s.blow = None;
                }
                Ok(0)
            }
            Convoy { ship, a, b } => done(self.order_convoy(ship, a, b)),
            Gather {
                ship,
                deposit,
                port,
            } => done(self.order_gather_with(ship, deposit, port)),
            Take { ship, target } => done(self.order_take(ship, target)),
            Unload { ship, port, prio } => done(self.order_unload_with(ship, port, prio)),
            GatherEvent { ship, deposit } => {
                // 0x4aab00's class test (0x4aacb6-0x4aacde) on `+0x261`.
                let i = self.object_index(ship).ok_or("No such ship")?;
                let d = self.object_index(deposit).ok_or("No such deposit")?;
                let bit = match super::gather::deposit_class(&self.objects[d]) {
                    Some(Resource::Fish) => 0x20,
                    Some(Resource::Leviathan) => 0x40,
                    Some(Resource::Amber) => 0x80,
                    _ => 0,
                };
                if self.capabilities(i) & bit != bit {
                    return Err("The ship cannot gather that now".into());
                }
                done(self.order_gather_with(ship, deposit, 0))
            }
            UnloadEvent { ship, port, prio } => {
                let i = self.object_index(ship).ok_or("No such ship")?;
                let p = self.object_index(port).ok_or("No such port")?;
                done(self.unload_event(i, p, prio))
            }
            Found {
                ship,
                cx,
                cy,
                class,
            } => done(self.order_found(ship, cx, cy, class)),
            Repair { ship, dock } => done(self.order_repair(ship, dock)),
            Sell { ship, dock } => done(self.order_sell(ship, dock)),
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
            SellObject { site, target } => done(self.sell_object(site, target, data, caches)),
            RepairStock { workshop } => done(self.order_repair_stock(workshop)),
            Halt { id } => done(self.halt(id, data, caches)),
            SelfMend { id } => done(self.self_mend(id)),
            StructureShip { site, kind } => {
                self.barbarian_order(site, kind, true);
                self.make_new_ships(data, caches);
                Ok(0)
            }
            FortBehaviour { site, sub } => done(self.fort_order(site, sub)),
            FortRepair { site } => done(self.fort_repair(site)),
            FortCast {
                site,
                spell,
                cx,
                cy,
                target,
            } => done(self.fort_cast(site, spell, (cx, cy), target, data, caches)),
            ShipCast {
                ship,
                spell,
                cx,
                cy,
                target,
            } => done(self.ship_cast(ship, spell, (cx, cy), target, data, caches)),
            BuildShip { site, kind } => {
                let r = self.start_ship(site, kind);
                self.make_new_ships(data, caches);
                done(r)
            }
            YardShip {
                site,
                kind,
                pay,
                arm,
            } => {
                let r = self.start_ship_armed(site, kind, pay, arm);
                self.make_new_ships(data, caches);
                done(r)
            }
            TownMend { site, target } => done(self.mend_by(site, target)),
            TownSpell {
                site,
                kind,
                cell,
                rect,
                ship,
                discount,
            } => {
                self.town_spell(site, kind, cell, rect, ship, discount, data, caches);
                Ok(0)
            }
            TownStudy {
                site,
                owner,
                upgrade,
            } => done(self.town_study(site, owner, upgrade)),
            TownLearn {
                site,
                owner,
                temple,
            } => done(self.town_learn(site, owner, temple)),
            TownBerths { site, owner } => {
                let yards = self.town_yards_of(site, owner);
                self.clear_berths(Some(owner), &yards);
                Ok(0)
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
                facing,
            } => done(self.place_building_facing(site, class, cx, cy, facing, data, caches)),
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
/// one ran, of its rank (0 for 0x20 .. 3 for 0x80, 4 for an attack's fire
/// records 0x100), over the ship as it was
/// before; or a stop came, which drops the rest of the tick's; or the
/// ship was busy and dropped them all.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TickOrder {
    Ran {
        rank: u8,
        before: Box<crate::objects::Ship>,
    },
    Stopped,
    /// The ship was busy at the group's turn (0x4abe50): its task wound
    /// down over the ship as it was `before`, the group dropped.
    Dropped {
        before: Box<crate::objects::Ship>,
    },
}

/// The human's unload goes out with priority 75 (0x4d74a0's 0x1f).
fn human_unload_prio() -> u8 {
    75
}
