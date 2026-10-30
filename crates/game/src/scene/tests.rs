//! Scene tests; most need the game data (`OPENCONQUEST_DATA`).

use super::*;
use crate::objects::Caches;

/// Plays as if past the first ticks of the game, whose commands are held
/// (`order::HOLD`), so the player's orders are carried out at once.
fn past_hold(scene: &mut Scene) {
    scene.ticks = order::HOLD;
}

/// Builds a boat and upgrades the settlement in the first campaign
/// mission (needs `OPENCONQUEST_DATA`).
#[test]
fn shipyard_builds_and_settlement_upgrades() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    let own = |scene: &Scene, cls| {
        scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::Building(cls) && scene.is_own(o))
            .map(|o| o.id)
            .unwrap()
    };
    let yard = own(&scene, SHIPYARD);
    let ships_before = scene
        .objects
        .iter()
        .filter(|o| o.ship.is_some() && scene.is_own(o))
        .count();
    // 4000 amber, and miss000's brain adds 2000 at the middle difficulty;
    // spend down to 300.
    assert_eq!(scene.amber, 6000);
    scene.amber = 300;
    // A merchant needs a town; a boat only a shipyard and a settlement.
    assert!(scene.can_build_ship(1).is_err());
    let order = Order::BuildShip {
        site: yard,
        kind: 0,
    };
    scene.apply(&order, &data, &mut caches).unwrap();
    assert_eq!(scene.amber, 100);
    // The first is built on the berth; more wait their turn, paid; the
    // last can be taken back, paid back.
    scene.amber = 500;
    scene.apply(&order, &data, &mut caches).unwrap();
    scene.apply(&order, &data, &mut caches).unwrap();
    assert_eq!(scene.ships_queued(yard, 0, false), 2);
    assert_eq!(scene.amber, 100);
    scene.cancel_ship(yard, 0).unwrap();
    assert_eq!((scene.ships_queued(yard, 0, false), scene.amber), (1, 300));
    scene.amber = 100;
    for _ in 0..1000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let own_ships: Vec<_> = scene
        .objects
        .iter()
        .filter(|o| o.ship.is_some() && scene.is_own(o))
        .collect();
    assert_eq!(own_ships.len(), ships_before + 2);
    assert!(scene.ship_orders.is_empty() && !scene.yard_busy(yard));

    let settlement = own(&scene, SETTLEMENT);
    assert!(
        scene.upgrade(settlement, &data, &mut caches).is_err(),
        "100 amber is not enough"
    );
    scene.amber = 1000;
    scene.upgrade(settlement, &data, &mut caches).unwrap();
    assert_eq!(scene.amber, 500);
    let town = &scene.objects[scene.object_index(settlement).unwrap()];
    assert_eq!(town.class, ObjectClass::Building(TOWN));
    // It goes up as a new building does: no town until it stands.
    assert!(scene.construction_at(settlement).is_some());
    assert!(scene.can_build_ship(1).is_err());
    for _ in 0..(60.0 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) + 1.0)) as usize {
        scene.update(1.0 / 60.0, &data, &mut caches);
    }
    assert!(scene.construction_at(settlement).is_none());
    // With a town and 500 amber, a merchant (400 amber, 200 fish) is possible.
    scene.amber = 500;
    assert_eq!(scene.can_build_ship(1), Ok(()));
}

/// A yard's berth is the cell a step from its anchor toward its facing
/// (0x40e050 with 0x5c4f4c); an order with the berth free builds the ship
/// there at once, hull at nothing, heading 6 for a yard facing north or
/// south (0x416a40, 0x4dfe00); it rises a fortieth every 8 ticks and is
/// done at the 39th step for a boat, the 40th for a merchant (0x4b3ab0),
/// then moves a cell off the berth with priority 101 (0x4b46f0); an order
/// while the berth is taken is queued, charged the full price even when
/// free, and built free 25 ticks after the berth is clear (0x416f90);
/// one that cannot be paid is dropped (needs `OPENCONQUEST_DATA`).
#[test]
fn shipyard_berth_and_queue_follow_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    let yard = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(SHIPYARD) && scene.is_own(o))
        .map(|o| o.id)
        .unwrap();
    let y = scene.object_index(yard).unwrap();
    let (ax, ay) = scene.objects[y].cell.unwrap();
    let facing = scene.objects[y].record.fields[10];
    let steps = [
        (1, 0),
        (1, -1),
        (0, -1),
        (-1, -1),
        (-1, 0),
        (-1, 1),
        (0, 1),
        (1, 1),
    ];
    for (f, (dx, dy)) in steps.into_iter().enumerate() {
        scene.objects[y].record.fields[10] = f as u32;
        assert_eq!(scene.berth_cell(y), Some((ax + dx, ay + dy)), "facing {f}");
    }
    scene.objects[y].record.fields[10] = facing;
    let berth = scene.berth_cell(y).unwrap();
    assert_eq!(scene.berth_answer(yard), 0);
    scene.amber = 1000;
    scene
        .apply(
            &Order::BuildShip {
                site: yard,
                kind: 0,
            },
            &data,
            &mut caches,
        )
        .unwrap();
    let k = scene.ship_on_berth(yard).expect("built on the berth");
    let o = &scene.objects[k];
    let t = TILE_SIZE as f32;
    assert_eq!(((o.x / t) as i32, (o.y / t) as i32), berth);
    let heading = if matches!(facing, 2 | 6) { 6 } else { 0 };
    assert_eq!(o.ship.as_ref().unwrap().heading, heading);
    assert_eq!(o.combat.as_ref().unwrap().hull, 0.0);
    assert_eq!(scene.berth_answer(yard), -1);
    let id = o.id;
    // Taken: a free order is queued and charged all the same.
    let amber = scene.amber;
    scene.start_ship_with(yard, 0, false).unwrap();
    assert_eq!(scene.ship_orders.len(), 1);
    assert_eq!(scene.amber, amber - 200);
    // One that cannot be paid is dropped.
    let amber = scene.amber;
    scene.amber = 50;
    scene
        .yard_order(yard, 0, false, true, None, scene.default_arming)
        .unwrap_err();
    assert_eq!(scene.ship_orders.len(), 1);
    scene.amber = amber;
    // A ship being built takes no moves.
    let (ox, oy) = (scene.objects[k].x, scene.objects[k].y);
    assert!(!scene.order_move(id, ox + 3.0 * t, oy));
    // The boat: 39 steps of 8 ticks, the first at its first tick.
    let mut ticks = 0;
    while scene.being_built(id) {
        scene.update(crate::clock::TICK, &data, &mut caches);
        ticks += 1;
        assert!(ticks < 400);
    }
    assert_eq!(ticks, 1 + 38 * 8);
    let k = scene.object_index(id).unwrap();
    let o = &scene.objects[k];
    let s = o.ship.as_ref().unwrap();
    assert_eq!(
        o.combat.as_ref().unwrap().hull,
        o.combat.as_ref().unwrap().max_hull
    );
    // Off the berth: one cell, priority 101 (or one more a row, for a
    // cell of an own ship).
    assert!(s.priority >= 101);
    let goal = s.path.back().copied().expect("moving off the berth");
    let goal = ((goal.0 / t) as i32, (goal.1 / t) as i32);
    let here = ((o.x / t) as i32, (o.y / t) as i32);
    assert!(goal != here && (goal.0 - here.0).abs() <= 2 && (goal.1 - here.1).abs() <= 2);
    // The queued boat goes on the berth once it is clear, at a 25-tick
    // pass of the queue, built free.
    let amber = scene.amber;
    let mut ticks = 0;
    while scene.ship_on_berth(yard).is_none() {
        scene.update(crate::clock::TICK, &data, &mut caches);
        ticks += 1;
        assert!(ticks < 400, "the queue builds");
    }
    assert!(scene.ship_orders.is_empty());
    assert_eq!(scene.amber, amber);
    // A merchant's hull takes 40 steps.
    let k = scene.ship_on_berth(yard).unwrap();
    let id = scene.objects[k].id;
    {
        let s = scene.objects[k].ship.as_mut().unwrap();
        s.kind = 1;
        let b = s.build.as_mut().unwrap();
        b.frame = if heading == 6 { 40 } else { 0 };
        b.since = 8;
        scene.objects[k].combat.as_mut().unwrap().hull = 0.0;
    }
    let mut ticks = 0;
    while scene.being_built(id) {
        scene.update(crate::clock::TICK, &data, &mut caches);
        ticks += 1;
        assert!(ticks < 400);
    }
    assert_eq!(ticks, 1 + 39 * 8);
}

/// A stop order calls off a ship being built: its picture runs back down
/// a frame every 8 ticks, then its price is given back, the owner's ship
/// counted lost and the ship removed (0x4aa540, 0x4b3ab0; needs
/// `OPENCONQUEST_DATA`).
#[test]
fn stopping_a_ship_being_built_calls_it_off() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    let yard = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(SHIPYARD) && scene.is_own(o))
        .map(|o| o.id)
        .unwrap();
    scene.amber = 1000;
    scene
        .apply(
            &Order::BuildShip {
                site: yard,
                kind: 0,
            },
            &data,
            &mut caches,
        )
        .unwrap();
    assert_eq!(scene.amber, 800);
    let id = scene.objects[scene.ship_on_berth(yard).unwrap()].id;
    for _ in 0..(1 + 8 * 4) {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let k = scene.object_index(id).unwrap();
    let (frame, heading) = {
        let s = scene.objects[k].ship.as_ref().unwrap();
        (s.build.unwrap().frame, s.heading)
    };
    let base = if heading == 6 { 40 } else { 0 };
    assert_eq!(frame - base, 5);
    scene
        .apply(&Order::Stop { id }, &data, &mut caches)
        .unwrap();
    // Back down to the base (heading 0: below 1; heading 6: below 40).
    let steps = if heading == 6 { 6 } else { 5 };
    for _ in 0..(steps * 8 - 1) {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    assert!(scene.being_built(id));
    let lost = scene.stats.ships_lost[0];
    scene.update(crate::clock::TICK, &data, &mut caches);
    assert!(scene.object_index(id).is_none());
    assert_eq!(scene.amber, 1000);
    assert_eq!(scene.stats.ships_lost[0], lost + 1);
}

/// A warship attacks and sinks an enemy ship (needs `OPENCONQUEST_DATA`).
#[test]
fn warship_sinks_enemy() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let human = scene.human.unwrap();
    // Put a royal ship of ours next to a merchant of another player.
    let mine = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let (x, y) = (scene.objects[mine].x, scene.objects[mine].y);
    let ship = scene.objects[mine].ship.as_mut().unwrap();
    ship.kind = 2;
    let c = scene.objects[mine].combat.as_mut().unwrap();
    c.weapons = combat::DEFAULT_WEAPONS[2].to_vec();
    c.max_hull = combat::hull_strength(2);
    c.hull = c.max_hull;
    let enemy = scene
        .objects
        .iter()
        .position(|o| o.ship.is_some() && !scene.is_own(o) && o.owner.is_some());
    let enemy = match enemy {
        Some(e) => e,
        None => {
            let e = scene
                .objects
                .iter()
                .position(|o| o.ship.is_some() && o.id != scene.objects[mine].id)
                .unwrap();
            scene.objects[e].owner = Some((human.nation, human.slot + 1));
            e
        }
    };
    scene.objects[enemy].x = x + 128.0;
    scene.objects[enemy].y = y;
    let target = scene.objects[enemy].id;
    let mine = scene.objects[mine].id;
    scene.order_attack(mine, target).unwrap();
    let (mut flew, mut landed) = (false, false);
    for _ in 0..60 * 120 {
        scene.update(1.0 / 60.0, &data, &mut caches);
        // Shots fly with their pictures and land with an impact a frame a
        // tick.
        flew |= scene.shots.iter().any(|s| s.layer().is_some());
        landed |= scene
            .effects
            .iter()
            .any(|fx| fx.frame_ms == crate::clock::TICK_MS as f32);
        if scene.object_index(target).is_none() {
            break;
        }
    }
    assert!(flew && landed, "shots flew ({flew}) and landed ({landed})");
    assert!(scene.object_index(target).is_none(), "the enemy was sunk");
    assert!(
        scene.effects.iter().any(|fx| fx.frames.len() == 20),
        "the hull goes down with its sinking animation"
    );
    assert!(scene.object_index(mine).is_some());
}

/// A rammer damages its target, and boarders capture a ship (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn ramming_and_boarding() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    let human = scene.human.unwrap();
    let mine = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let mine_id = scene.objects[mine].id;
    let other = scene
        .objects
        .iter()
        .position(|o| o.ship.is_some() && o.id != mine_id)
        .unwrap();
    let other_id = scene.objects[other].id;
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == mine_id || o.id == other_id);
    // Two unarmed ships side by side in open water.
    let (x, y) = (
        scene.objects[scene.object_index(mine_id).unwrap()].x,
        scene.objects[scene.object_index(mine_id).unwrap()].y,
    );
    for (id, dx, kind, crew) in [(mine_id, 0.0, 5u8, 6u32), (other_id, 40.0, 1, 1)] {
        let i = scene.object_index(id).unwrap();
        let o = &mut scene.objects[i];
        o.x = x + dx;
        o.y = y;
        o.combat.as_mut().unwrap().weapons.clear();
        let s = o.ship.as_mut().unwrap();
        s.kind = kind;
        s.crew = combat::Crew {
            class: 2,
            squads: [0, 0, crew],
        };
    }
    let i = scene.object_index(other_id).unwrap();
    scene.objects[i].owner = Some((1, (human.slot + 1) % 4));
    // Sturdy enough to outlast a rammer's blow, to be boarded after.
    let c = scene.objects[i].combat.as_mut().unwrap();
    let ordinary = c.max_hull;
    (c.max_hull, c.hull) = (1e6, 1e6);
    let hull = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull
    };
    let before = hull(&scene, other_id);
    scene.order_close(mine_id, other_id, false).unwrap();
    // The rammer charges, shows the splinters at its bow (BOMBH30) and
    // strikes as they end.
    let mut splinters = false;
    for _ in 0..50 * 10 {
        scene.update(0.02, &data, &mut caches);
        splinters |= !scene.effects.is_empty();
        if hull(&scene, other_id) < before {
            break;
        }
    }
    assert!(splinters, "the impact was shown");
    assert!(hull(&scene, other_id) < before, "the ram did damage");
    // Ordinary again for the boarding: the boarders' toll counts a
    // hundredth of the prize's hull.
    let i = scene.object_index(other_id).unwrap();
    let c = scene.objects[i].combat.as_mut().unwrap();
    (c.max_hull, c.hull) = (ordinary, ordinary);
    scene.order_close(mine_id, other_id, true).unwrap();
    // The fight aboard shows (ABORD_ pictures run 40-67 frames).
    let mut fought = false;
    for _ in 0..50 * 60 {
        scene.update(0.02, &data, &mut caches);
        fought |= scene
            .effects
            .iter()
            .any(|fx| (40..=67).contains(&fx.frames.len()));
        let i = scene.object_index(other_id).unwrap();
        if scene.is_own(&scene.objects[i]) {
            break;
        }
    }
    assert!(fought, "the boarding was shown");
    let prize = &scene.objects[scene.object_index(other_id).unwrap()];
    assert!(scene.is_own(prize), "the ship was captured");
    assert!(prize.ship.as_ref().unwrap().crew.count() > 0);
}

/// A boat founds a settlement on a distant shore (needs `OPENCONQUEST_DATA`).
#[test]
fn boat_founds_settlement() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let boat = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .unwrap()
        .id;
    scene.objects.retain(|o| o.ship.is_none() || o.id == boat);
    scene.amber = 1000;
    let settlements = |s: &Scene| {
        s.objects
            .iter()
            .filter(|o| s.is_own(o) && o.class == ObjectClass::Building(SETTLEMENT))
            .count()
    };
    let before = settlements(&scene);
    // An inland cell near the sea, well away from the boat's home.
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let shore = (0..h)
        .rev()
        .flat_map(|y| (0..w).rev().map(move |x| (x, y)))
        .find(|&(x, y)| {
            scene.check_ground(SETTLEMENT, x, y).is_ok()
                && (-2..=2)
                    .flat_map(|dy| (-2..=2).map(move |dx| (dx, dy)))
                    .any(|(dx, dy)| scene.nav.is_water(x + dx, y + dy))
        })
        .unwrap();
    scene
        .order_found(boat, shore.0, shore.1, SETTLEMENT)
        .unwrap();
    for _ in 0..50 * 600 {
        scene.update(0.02, &data, &mut caches);
        if !scene.constructions.is_empty() {
            break;
        }
    }
    assert!(scene.messages.is_empty(), "{:?}", scene.messages);
    assert_eq!(scene.amber, 1000 - economy::building_cost(SETTLEMENT));
    for _ in 0..50 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(settlements(&scene), before + 1);
}

/// A settlement trains swordsmen and a ship alongside takes them on
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn marines_train_and_embark() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = (home.x, home.y, home.id);
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
    // Start from an empty stock (miss000's settlement holds two marines).
    assert_eq!(scene.garrison(home)[..3], [0, 1, 1]);
    scene.garrisons.remove(&home);
    let (amber, fish) = (scene.amber, scene.fish);
    scene.train(home, 2).unwrap();
    scene.train(home, 0).unwrap();
    scene.train(home, 2).unwrap();
    scene.train(home, 0).unwrap();
    // Kinds mix in one queue; a right click takes the last bowman back:
    // its 30 amber and 2 raw fish (0x597170[0], a hundredth of the 200
    // taken, 0x4a8c30).
    let t = scene.training_at(home).unwrap();
    assert_eq!((t.count(0), t.count(2), t.count(1)), (2, 2, 0));
    let (a0, f0) = combat::item_cost(0);
    scene.cancel_training(home, 0).unwrap();
    assert_eq!(scene.training_at(home).unwrap().queue, [2, 0, 2]);
    assert!(scene.cancel_training(home, 1).is_err());
    assert_eq!(
        (scene.amber, scene.fish),
        (amber - 60 - a0, fish - 600 - 2 * f0 + 2)
    );
    // Halfway through the first, the swordsmen's button shows a sixth
    // (the time to the last of them, the third), the bowmen's a quarter.
    for _ in 0..(combat::train_seconds(0) / 2.0 / 0.02).round() as usize {
        scene.update(0.02, &data, &mut caches);
    }
    let t = scene.training_at(home).unwrap();
    assert!((t.progress(2, false).unwrap() - 1.0 / 6.0).abs() < 0.02);
    assert!((t.progress(0, false).unwrap() - 0.25).abs() < 0.02);
    for _ in 0..50 * (3 * combat::train_seconds(0).ceil() as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.garrison(home)[..3], [1, 0, 2]);
    assert!(scene.training_at(home).is_none());
    scene.garrisons.get_mut(&home).unwrap()[0] = 0;
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].ship.as_mut().unwrap().crew = combat::Crew {
        class: 0,
        squads: [0; 3],
    };
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].x = hx + 64.0;
    scene.objects[i].y = hy;
    assert_eq!(scene.load_crew(ship), Ok(2));
    assert_eq!(scene.garrison(home)[..3], [0, 0, 0]);
}

/// A Temple of the Sea learns spells; the wind obeys and a whirlpool
/// sinks a ship (needs `OPENCONQUEST_DATA`).
#[test]
fn temple_magic() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap().id;
    scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
    scene.amber = 50_000;
    // Every spell allowed (the campaign withholds them: they would be passed
    // over).
    scene.permits = crate::permits::Permits::ALL;
    scene.fish = 50_000;
    assert!(
        scene.learn_spell(Temple::Sea, None).is_err(),
        "no temple yet"
    );
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| {
            scene
                .check_site(home, economy::TEMPLE_OF_THE_SEA, x, y)
                .is_ok()
        })
        .expect("room for a temple");
    scene
        .place_building(
            home,
            economy::TEMPLE_OF_THE_SEA,
            site.0,
            site.1,
            &data,
            &mut caches,
        )
        .unwrap();
    assert!(
        scene.learn_spell(Temple::Sea, None).is_err(),
        "still being built"
    );
    for _ in 0..50 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    // Learn the first four: wind, view map, tornado, whirlpool.
    for _ in 0..4 {
        scene.learn_spell(Temple::Sea, None).unwrap();
        for _ in 0..50 * (magic::LEARN_SECONDS as usize + 1) {
            scene.update(0.02, &data, &mut caches);
        }
    }
    assert!(scene.magic.knows(Spell::Whirlpool) && !scene.magic.knows(Spell::Reef));
    // View lets us see a far corner for 3000 ticks.
    let far = (scene.nav.width as i32 - 2, scene.nav.height as i32 - 2);
    scene
        .cast_area(Spell::ViewMap, far.0, far.1, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.seen(far.0, far.1), Seen::InSight);
    for _ in 0..50 * 121 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_ne!(scene.seen(far.0, far.1), Seen::InSight);
    scene.cast_wind(6).unwrap();
    assert_eq!((scene.wind.strength, scene.wind.direction), (7, 6));
    let i = scene.object_index(ship).unwrap();
    let (cx, cy) = (
        (scene.objects[i].x / 64.0) as i32,
        (scene.objects[i].y / 64.0) as i32,
    );
    assert!(
        scene
            .cast_hazard(Spell::Reef, cx, cy, &data, &mut caches)
            .is_err(),
        "unknown"
    );
    // Not under one of the player's own ships (0x42ef90: the occupant
    // at war with the caster only); another player's, yes.
    assert!(scene
        .cast_hazard(Spell::Whirlpool, cx, cy, &data, &mut caches)
        .is_err());
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].owner = scene.objects[i].owner.map(|(n, s)| (n, s + 1));
    scene
        .cast_hazard(Spell::Whirlpool, cx, cy, &data, &mut caches)
        .unwrap();
    // The ship on the whirlpool's cell goes down.
    assert!(scene.objects.iter().any(|o| {
        o.class == ObjectClass::Whirlpool
            && o.record.fields[5] == cx as u32
            && o.record.fields[6] == cy as u32
    }));
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].x = cx as f32 * 64.0 + 32.0;
    scene.objects[i].y = cy as f32 * 64.0 + 32.0;
    for _ in 0..50 * 30 {
        scene.update(0.02, &data, &mut caches);
        if scene.object_index(ship).is_none() {
            break;
        }
    }
    assert!(
        scene.object_index(ship).is_none(),
        "the whirlpool sank the ship"
    );
}

/// A Temple of War learns all its spells; protection wards off weapons
/// and magic, lightning and flame hurt (needs `OPENCONQUEST_DATA`).
#[test]
fn war_magic() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.amber = 200_000;
    scene.permits = crate::permits::Permits::ALL;
    scene.fish = 200_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let war = economy::TEMPLE_OF_WAR;
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, war, x, y).is_ok())
        .expect("room for a temple");
    scene
        .place_building(home, war, site.0, site.1, &data, &mut caches)
        .unwrap();
    for _ in 0..50 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    for _ in 0..6 {
        scene.learn_spell(Temple::War, None).unwrap();
        for _ in 0..50 * (magic::LEARN_SECONDS as usize + 1) {
            scene.magic.update(0.02, &[], scene.permits);
        }
    }
    assert!(scene.magic.all_known(Temple::War));
    // Two ships far from everyone: ours and an enemy.
    let mine = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let enemy = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && o.id != mine)
        .unwrap()
        .id;
    let human = scene.human.unwrap();
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].owner = Some((1, (human.slot + 1) % 4));
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == mine || o.id == enemy);
    for id in [mine, enemy] {
        let i = scene.object_index(id).unwrap();
        let c = scene.objects[i].combat.as_mut().unwrap();
        c.weapons.clear();
        c.max_hull = 10_000.0;
        c.hull = 10_000.0;
    }
    let hull = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull
    };
    // Both ships next to the temple, within its reach.
    let temple = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(war))
        .unwrap();
    let (tx, ty) = (temple.x, temple.y);
    for (id, dx) in [(mine, 64.0), (enemy, 164.0)] {
        let i = scene.object_index(id).unwrap();
        scene.objects[i].x = tx + dx;
        scene.objects[i].y = ty;
    }
    let t = TILE_SIZE as f32;
    let cell = |s: &Scene, id| {
        let o = &s.objects[s.object_index(id).unwrap()];
        ((o.x / t) as i32, (o.y / t) as i32)
    };
    assert!(
        scene
            .cast_on(Spell::ProtectMagic, enemy, &data, &mut caches)
            .is_err(),
        "not on enemies"
    );
    let far = scene.nav.width as i32 * 2;
    assert!(
        scene
            .cast_area(Spell::Lightning, far, 0, &data, &mut caches)
            .is_err(),
        "out of reach"
    );
    // The weapon shield takes most of an enemy broadside.
    scene
        .cast_on(Spell::ProtectWeapons, mine, &data, &mut caches)
        .unwrap();
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].combat.as_mut().unwrap().weapons = vec![4; 8];
    for _ in 0..50 * 10 {
        scene.update(0.02, &data, &mut caches);
    }
    let lost = 10_000.0 - hull(&scene, mine);
    assert!(lost > 0.0 && lost < 600.0, "shielded loss {lost}");
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].combat.as_mut().unwrap().weapons.clear();
    // The magic shield blunts a fireball.
    scene.objects[e].combat.as_mut().unwrap().protect_magic = 60.0;
    let (ex, ey) = cell(&scene, enemy);
    let before = hull(&scene, enemy);
    scene
        .cast_area(Spell::Fireball, ex, ey, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.fireballs.len(), 1);
    for _ in 0..50 * 20 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.fireballs.is_empty(), "the fireball died away");
    // 2-4 strikes, each sharing 88000 / 15 out, a fifth through the shield.
    let burnt = before - hull(&scene, enemy);
    let most = 88_000.0 / 15.0 * 2.0 * magic::SHIELD_FACTOR;
    assert!(burnt > 0.0 && burnt <= most + 1.0, "burnt {burnt}");
    assert!(
        hull(&scene, mine) > 10_000.0 - 600.0,
        "the fireball spares our own"
    );
    // Lightning sinks it, shielded or not: asked whether it is shielded,
    // a ship goes down (0xda94).
    let (ex, ey) = cell(&scene, enemy);
    scene
        .cast_area(Spell::Lightning, ex, ey, &data, &mut caches)
        .unwrap();
    // It strikes partway through its picture.
    assert!(scene.object_index(enemy).is_some(), "not yet");
    for _ in 0..50 * 2 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.object_index(enemy).is_none(), "struck down");
    // Flame makes our weapons stronger for a while.
    scene.cast_flame(None).unwrap();
    // It brews at the temple first, 250 ticks.
    assert_eq!(scene.flame, 0.0);
    for _ in 0..50 * 11 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.flame > 0.0);
}

/// Missions are won when every condition is met and lost when the
/// player has nothing left (needs `OPENCONQUEST_DATA`).
#[test]
fn victory_and_defeat() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    // single04: kill all enemies and the dragons.
    let mut scene = Scene::load(&data, &mut caches, "single04").unwrap();
    assert_eq!(&scene.conditions[..2], &[true, true]);
    let enemy =
        |s: &Scene, o: &MapObject| o.owner.is_some_and(|w| w != combat::MONSTERS) && !s.is_own(o);
    let keep: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| !enemy(&scene, o))
        .map(|o| o.id)
        .collect();
    scene.objects.retain(|o| keep.contains(&o.id));
    // The mission is judged every 250 ticks.
    for _ in 0..11 {
        scene.update(1.0, &data, &mut caches);
    }
    assert!(scene.objective_met(0) && !scene.objective_met(1));
    assert_eq!(scene.outcome, None, "the dragons still live");
    let nest = scene
        .objects
        .iter()
        .find(|o| o.record.class_id == 0x44)
        .unwrap()
        .id;
    scene.hit(nest, 1.0e6, None, &data, &mut caches);
    for _ in 0..11 {
        scene.update(1.0, &data, &mut caches);
    }
    assert_eq!(scene.outcome, Some(Outcome::Victory));

    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let own: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| scene.is_own(o))
        .map(|o| o.id)
        .collect();
    scene.objects.retain(|o| !own.contains(&o.id));
    for _ in 0..11 {
        scene.update(1.0, &data, &mut caches);
    }
    assert_eq!(scene.outcome, Some(Outcome::Defeat));
}

/// A workshop builds a ballista and a ship alongside takes it on
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn workshop_builds_weapons() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.amber = 10_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, WORKSHOP, x, y).is_ok())
        .unwrap();
    scene
        .place_building(home, WORKSHOP, site.0, site.1, &data, &mut caches)
        .unwrap();
    let workshop = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(WORKSHOP))
        .unwrap()
        .id;
    assert!(
        scene.train(workshop, 0).is_err(),
        "workshops make weapons, not marines"
    );
    assert!(
        scene.train(home, 6).is_err(),
        "settlements train marines, not weapons"
    );
    let amber = scene.amber;
    scene.train(workshop, 6).unwrap();
    assert_eq!(scene.amber, amber - 70);
    for _ in 0..50 * (combat::train_seconds(8).ceil() as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.garrison(workshop)[6], 1);
    // The workshop's button: the time spent on that kind over its time.
    scene.train(workshop, 3).unwrap();
    scene.train(workshop, 6).unwrap();
    for _ in 0..(combat::train_seconds(3) / 0.02).round() as usize + 5 {
        scene.update(0.02, &data, &mut caches);
    }
    let t = scene.training_at(workshop).unwrap();
    assert_eq!((t.count(3), t.count(6)), (0, 1));
    assert!(t.progress(6, true).unwrap() < 0.05);
    for _ in 0..50 * (combat::train_seconds(6).ceil() as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.garrison(workshop)[6], 2);
    scene.garrisons.get_mut(&workshop).unwrap()[3] = 0;
    scene.garrisons.get_mut(&workshop).unwrap()[6] = 1;
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    // Only the workshop's stock, not the settlement's.
    scene.garrisons.remove(&home);
    let w = scene.object_index(workshop).unwrap();
    let (wx, wy) = (scene.objects[w].x, scene.objects[w].y);
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].x = wx + 64.0;
    scene.objects[i].y = wy;
    scene.objects[i].combat.as_mut().unwrap().weapons.clear();
    let s = scene.objects[i].ship.as_mut().unwrap();
    s.kind = 5; // a rammer: room for 3
    s.crew.squads = [0; 3];
    assert_eq!(scene.load_crew(ship), Ok(1));
    let i = scene.object_index(ship).unwrap();
    assert_eq!(scene.objects[i].combat.as_ref().unwrap().weapons, vec![6]);
    assert!(scene.objects[i].is_armed());
}

/// A tornado hops about and goes; a summoned sea monster fights for
/// its master, then leaves (needs `OPENCONQUEST_DATA`).
#[test]
fn tornado_and_sea_monster() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let human = scene.human.unwrap();
    scene.amber = 100_000;
    // Every spell allowed (the campaign withholds them: they would be passed
    // over).
    scene.permits = crate::permits::Permits::ALL;
    scene.fish = 100_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let sea = economy::TEMPLE_OF_THE_SEA;
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, sea, x, y).is_ok())
        .unwrap();
    scene
        .place_building(home, sea, site.0, site.1, &data, &mut caches)
        .unwrap();
    for _ in 0..50 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    for _ in 0..6 {
        scene.learn_spell(Temple::Sea, None).unwrap();
        for _ in 0..50 * (magic::LEARN_SECONDS as usize + 1) {
            scene.magic.update(0.02, &[], scene.permits);
        }
    }
    // An enemy ship alone at sea.
    let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap().id;
    scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].owner = Some((1, (human.slot + 1) % 4));
    scene.objects[i].combat.as_mut().unwrap().weapons.clear();
    let (sx, sy) = (scene.objects[i].x, scene.objects[i].y);
    let cell = ((sx / 64.0) as i32, (sy / 64.0) as i32);
    // A sea monster next to it sinks it, and then goes away.
    scene
        .cast_hazard(Spell::Monster, cell.0, cell.1, &data, &mut caches)
        .unwrap();
    let monster = scene.summons[0].0;
    let m = scene.object_index(monster).unwrap();
    assert!(scene.is_own(&scene.objects[m]), "it fights for us");
    for _ in 0..50 * 60 {
        scene.update(0.02, &data, &mut caches);
        if scene.object_index(ship).is_none() {
            break;
        }
    }
    assert!(
        scene.object_index(ship).is_none(),
        "the monster sank the ship"
    );
    // Below a tenth of its strength (mode 0) it spends a hundredth of it
    // each round of 50-125 ticks under water; with none left it dies, its
    // dying picture 65 ticks (0x491fa0).
    let m = scene.object_index(monster).unwrap();
    let c = scene.objects[m].combat.as_mut().unwrap();
    c.hull = c.max_hull * 0.02;
    for _ in 0..50 * 30 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.object_index(monster).is_none(), "the monster left");
    // A tornado rises, hops from cell to cell and sinks.
    scene
        .cast_hazard(Spell::Waterspout, cell.0, cell.1, &data, &mut caches)
        .unwrap();
    let tornado = |s: &Scene| s.effects.iter().position(|fx| fx.tornado.is_some());
    let fx = tornado(&scene).unwrap();
    let start = (scene.effects[fx].x, scene.effects[fx].y);
    for _ in 0..50 * 5 {
        scene.update(0.02, &data, &mut caches);
    }
    let fx = tornado(&scene).unwrap();
    assert_ne!((scene.effects[fx].x, scene.effects[fx].y), start);
    for _ in 0..50 * 20 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(tornado(&scene).is_none(), "the tornado is gone");
}

/// A saved game, restored, carries on exactly as the original would
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn save_and_restore() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    for _ in 0..50 * 40 {
        scene.update(0.02, &data, &mut caches);
    }
    // Knowledge gained since the start is kept.
    scene.permits.read_scroll(1);
    let path = std::env::temp_dir().join(format!(
        "openconquest-save-test-{}.json",
        std::process::id()
    ));
    scene.save().write(&path, true).unwrap();
    let save = SaveGame::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let mut copy = Scene::restore(&data, &mut caches, &save).unwrap();
    assert_eq!(copy.permits, scene.permits);
    let state = |s: &Scene| {
        let mut v: Vec<_> = s
            .objects
            .iter()
            .map(|o| {
                let hull = o.combat.as_ref().map_or(-1, |c| c.hull as i32);
                (
                    o.id,
                    (o.x * 10.0) as i32,
                    (o.y * 10.0) as i32,
                    o.owner,
                    hull,
                )
            })
            .collect();
        v.sort();
        (
            v,
            s.amber,
            s.fish,
            s.purses.clone(),
            s.wind.strength,
            s.wind.direction,
        )
    };
    assert_eq!(state(&scene), state(&copy));
    for _ in 0..50 * 30 {
        scene.update(0.02, &data, &mut caches);
        copy.update(0.02, &data, &mut caches);
    }
    assert_eq!(state(&scene), state(&copy), "the restored game diverged");
}

/// A sage school studies weapons, and better weapons hit harder
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn school_research() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    // The campaign has not revealed it all yet.
    scene.permits = crate::permits::Permits::ALL;
    scene.amber = 10_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let school = economy::SCHOOL;
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, school, x, y).is_ok())
        .unwrap();
    scene
        .place_building(home, school, site.0, site.1, &data, &mut caches)
        .unwrap();
    let id = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(school))
        .unwrap()
        .id;
    assert!(
        scene.start_research(id, combat::Upgrade::Weapons).is_err(),
        "still being built"
    );
    for _ in 0..50 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    let amber = scene.amber;
    scene.start_research(id, combat::Upgrade::Weapons).unwrap();
    assert_eq!(scene.amber, amber - 1300);
    assert!(
        scene.start_research(id, combat::Upgrade::Ram).is_err(),
        "one at a time"
    );
    for _ in 0..50 * 31 {
        scene.update(0.02, &data, &mut caches);
    }
    let human = scene.human.map(|h| (h.nation, h.slot));
    assert_eq!(scene.levels_of(human).weapons, 1);
    assert_eq!(
        scene.next_level(combat::Upgrade::Weapons),
        None,
        "arming is studied once"
    );
    // A ballista strikes harder at damage level 1 (240 against 200).
    assert_eq!(combat::hull_damage_at(6, 1), 240.0);
}

/// A smithy makes armour for its town's store once the school has
/// studied it (0x434ed0 job 1: message 0x248d {level, 100} to `+0x1c8`);
/// no ship alongside is fitted (needs `OPENCONQUEST_DATA`).
#[test]
fn smithy_refits() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.amber = 10_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let smithy = economy::SMITHY;
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, smithy, x, y).is_ok())
        .unwrap();
    scene
        .place_building(home, smithy, site.0, site.1, &data, &mut caches)
        .unwrap();
    let id = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(smithy))
        .unwrap()
        .id;
    // A ship of ours alongside, which keeps what it has.
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let s = scene.object_index(id).unwrap();
    let (sx, sy) = (scene.objects[s].x, scene.objects[s].y);
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].x = sx + 64.0;
    scene.objects[i].y = sy;
    let armour = combat::Upgrade::Armour;
    assert!(scene.refit(id, armour, 1).is_err(), "not studied yet");
    let human = scene.human.map(|h| (h.nation, h.slot)).unwrap();
    scene.levels.entry(human).or_default().set(armour, 1);
    let amber = scene.amber;
    scene.refit(id, armour, 1).unwrap();
    assert_eq!(scene.amber, amber - combat::refit_cost(armour, 1));
    let armour_of = |scene: &Scene| {
        scene.objects[scene.object_index(ship).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .armour
    };
    let stored = |scene: &Scene| scene.fittings.get(&home).cloned().unwrap_or_default();
    let before = stored(&scene);
    // Made once its 150 ticks are up, the button showing the work.
    assert_eq!(scene.refits_at(id, armour, 1), (1, Some(0.0)));
    scene.run_refits(combat::refit_seconds(1) / 2.0);
    assert_eq!(stored(&scene), before);
    scene.run_refits(combat::refit_seconds(1) / 2.0 + 0.01);
    assert_eq!(stored(&scene).0, [before.0.clone(), vec![1]].concat());
    assert_eq!(armour_of(&scene), 0);
    assert_eq!(scene.refits_at(id, armour, 1), (0, None));
    // A right click takes a queued piece back, its price returned.
    scene.levels.entry(human).or_default().set(armour, 2);
    let amber = scene.amber;
    scene.refit(id, armour, 2).unwrap();
    scene.cancel_refit(id, armour, 2).unwrap();
    assert_eq!(scene.amber, amber);
    assert!(scene.cancel_refit(id, armour, 2).is_err());
    scene.levels.entry(human).or_default().armour = 1;
    assert!(scene.refit(id, armour, 2).is_err(), "level 2 not studied");
    // Without a town the smithy takes no order.
    scene.towns.of.remove(&id);
    assert!(scene.refit(id, armour, 1).is_err());
}

/// A settlement puts up a smithy next to it (needs `OPENCONQUEST_DATA`).
#[test]
fn settlement_builds_structures() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.objects.retain(|o| o.ship.is_none());
    scene.amber = 5000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let smithy = 0x38;
    let mut spot = None;
    'find: for r in 2..=BUILD_RADIUS {
        for dy in -r..=r {
            for dx in -r..=r {
                if scene.check_site(home, smithy, hx + dx, hy + dy).is_ok() {
                    spot = Some((hx + dx, hy + dy));
                    break 'find;
                }
            }
        }
    }
    let (cx, cy) = spot.expect("room for a smithy");
    assert!(
        scene.check_site(home, smithy, hx + 50, hy).is_err(),
        "too far"
    );
    scene
        .place_building(home, smithy, cx, cy, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.amber, 5000 - economy::building_cost(smithy));
    assert!(scene.check_site(home, smithy, cx, cy).is_err(), "occupied");
    let bit = 1 << (smithy - SETTLEMENT);
    assert_eq!(scene.owned_buildings() & bit, 0, "not standing yet");
    for _ in 0..50 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_ne!(scene.owned_buildings() & bit, 0, "the smithy stands");
    assert!(scene.constructions.is_empty());
}

/// A warship destroys an enemy building, and forts fire at enemy ships.
#[test]
fn buildings_fight() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    let fort = scene
        .objects
        .iter()
        .position(|o| matches!(o.class, ObjectClass::Building(0x34..=0x36)) && o.is_armed())
        .expect("a fort");
    let (fx, fy, fort_id) = (
        scene.objects[fort].x,
        scene.objects[fort].y,
        scene.objects[fort].id,
    );
    let owner = scene.objects[fort].owner.unwrap();
    // An enemy ship next to the fort, alone at sea.
    let ship = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
    let ship_id = scene.objects[ship].id;
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == ship_id);
    let i = scene.object_index(ship_id).unwrap();
    scene.objects[i].owner = Some((owner.0, owner.1 + 1));
    // Out of the brain's fleets: their move orders would send it back to
    // its place in the formation (0x4ccd80).
    for f in &mut scene.brain.as_mut().unwrap().fleets {
        f.ships.retain(|&s| s != ship_id);
    }
    let water = scene
        .nav
        .nearest_water((fx / 64.0) as i32, (fy / 64.0) as i32)
        .unwrap();
    scene.objects[i].x = water.0 as f32 * 64.0 + 32.0;
    scene.objects[i].y = water.1 as f32 * 64.0 + 32.0;
    let hull = scene.objects[i].combat.as_ref().unwrap().hull;
    for _ in 0..50 * 5 {
        scene.update(0.02, &data, &mut caches);
    }
    let hit = scene
        .object_index(ship_id)
        .is_none_or(|i| scene.objects[i].combat.as_ref().unwrap().hull < hull);
    assert!(hit, "the fort fired at the ship");
    // Make the ship a strong warship and have it destroy the fort.
    let Some(i) = scene.object_index(ship_id) else {
        return;
    };
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.weapons = vec![4; 8];
    c.hull = 1.0e6;
    // The fort's blows would wear the ship's weapons away: it is
    // disarmed.
    let f = scene.object_index(fort_id).unwrap();
    scene.objects[f].combat.as_mut().unwrap().weapons.clear();
    scene.order_attack(ship_id, fort_id).unwrap();
    for _ in 0..50 * 120 {
        scene.update(0.02, &data, &mut caches);
        if scene
            .object_index(fort_id)
            .is_none_or(|i| scene.objects[i].combat.is_none())
        {
            break;
        }
    }
    let ruins = &scene.objects[scene.object_index(fort_id).expect("ruins remain")];
    assert!(
        ruins.combat.is_none() && ruins.owner.is_none(),
        "the fort was destroyed"
    );
}

/// A boat gathers amber and delivers it (needs `OPENCONQUEST_DATA`).
#[test]
fn merchant_gathers_amber() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let boat = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .unwrap();
    let (bx, by, boat) = (boat.x, boat.y, boat.id);
    let deposit = scene
        .objects
        .iter()
        .filter(|o| o.deposit.is_some_and(|d| d.resource == Resource::Amber))
        .min_by(|a, b| {
            let d = |o: &&MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let (deposit, before) = (deposit.id, deposit.deposit.unwrap().remaining.unwrap());
    scene
        .apply(
            &Order::GatherAt {
                ships: vec![boat],
                deposit,
                keys: 0,
            },
            &data,
            &mut caches,
        )
        .unwrap();
    let amber = scene.amber;
    let (mut partly, mut walkway, mut porters) = (false, false, false);
    let mut amber_walkway = false;
    for _ in 0..60 * 600 {
        scene.update(1.0 / 60.0, &data, &mut caches);
        // Unloading takes a while: porters carry the goods in.
        partly |= scene.amber > amber && scene.amber < amber + 400;
        walkway |= scene
            .planks
            .iter()
            .any(|p| p.activity == super::unload::Activity::Unloading);
        amber_walkway |= scene
            .planks
            .iter()
            .any(|p| p.activity == super::unload::Activity::Amber);
        porters |= !scene.porters.is_empty();
        if scene.amber >= amber + 400 {
            break;
        }
    }
    // 400 amber unloaded is worth 480 (0x4a8fa0: x 3 x 40%).
    assert_eq!(scene.amber, amber + 480, "one full boat load delivered");
    assert!(partly && walkway && porters, "{partly} {walkway} {porters}");
    assert!(amber_walkway, "a walkway to the amber while gathering");
    let left = scene.objects[scene.object_index(deposit).unwrap()]
        .deposit
        .unwrap()
        .remaining
        .unwrap();
    assert!((before - left - 400.0).abs() < 1.0 || left < before - 400.0);
}

/// Sails a ship across a real mission map (needs `OPENCONQUEST_DATA`).
#[test]
fn ship_sails_around_islands() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut scene = Scene::load(&data, &mut Caches::default(), "miss000").unwrap();
    let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap();
    let id = ship.id;
    // Sail alone, out of reach of other players' warships.
    scene.objects.retain(|o| o.ship.is_none() || o.id == id);
    // A far-away water cell: the bottom-right-most one.
    let t = TILE_SIZE as f32;
    let target = (0..scene.nav.height as i32)
        .rev()
        .flat_map(|y| (0..scene.nav.width as i32).rev().map(move |x| (x, y)))
        .find(|&(x, y)| scene.nav.is_water(x, y))
        .unwrap();
    let (tx, ty) = (target.0 as f32 * t + t / 2.0, target.1 as f32 * t + t / 2.0);
    assert!(scene.order_move(id, tx, ty));
    for _ in 0..(60 * 600) {
        scene.update(1.0 / 60.0, &data, &mut Caches::default());
        let o = &scene.objects[scene.object_index(id).unwrap()];
        assert!(
            scene
                .nav
                .is_water((o.x / t).floor() as i32, (o.y / t).floor() as i32),
            "ship ran aground at {}, {}",
            o.x,
            o.y
        );
        if o.ship.as_ref().unwrap().path.is_empty() {
            break;
        }
    }
    let o = &scene.objects[scene.object_index(id).unwrap()];
    assert!(
        (o.x - tx).abs() < 1.0 && (o.y - ty).abs() < 1.0,
        "did not arrive"
    );
}

#[test]
fn headings() {
    assert_eq!(heading_towards(1.0, 0.0), 0);
    assert_eq!(heading_towards(1.0, -1.0), 1);
    assert_eq!(heading_towards(0.0, -1.0), 2);
    assert_eq!(heading_towards(-1.0, 0.0), 4);
    assert_eq!(heading_towards(0.0, 1.0), 6);
    assert_eq!(heading_towards(1.0, 1.0), 7);
}

/// Fog of war: the player starts seeing around their own things, what
/// they leave behind stays explored, and other players' ships hide out of
/// sight (needs `OPENCONQUEST_DATA`).
#[test]
fn fog_of_war() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let t = TILE_SIZE as f32;
    let cell = |o: &MapObject| ((o.x / t) as i32, (o.y / t) as i32);
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let (start, id) = (cell(ship), ship.id);
    assert_eq!(scene.seen(start.0, start.1), Seen::InSight);
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let unexplored = (0..w * h)
        .filter(|&i| scene.seen(i % w, i / w) == Seen::Unexplored)
        .count();
    assert!(unexplored > 0, "some of the map starts dark");
    // Keep only this ship of ours (and no other ships), and sail it away.
    let gone: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| (o.ship.is_some() || scene.is_own(o)) && o.id != id)
        .map(|o| o.id)
        .collect();
    scene.objects.retain(|o| !gone.contains(&o.id));
    let i = scene.object_index(id).unwrap();
    let far = if start.0 < w / 2 { w - 2 } else { 1 };
    scene.objects[i].x = far as f32 * t + t / 2.0;
    scene.update(1.0, &data, &mut caches);
    assert_eq!(
        scene.seen(start.0, start.1),
        Seen::Explored,
        "left behind, still known"
    );
    // An enemy ship out of sight is hidden; in sight it shows.
    let i = scene.object_index(id).unwrap();
    let mut enemy = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some())
        .map(|o| o.record.clone())
        .unwrap();
    enemy.id = 9999;
    let mut other = objects::resolve(&data, &mut caches, &enemy)
        .unwrap()
        .unwrap();
    other.owner = Some((1, 3));
    other.x = start.0 as f32 * t;
    other.y = start.1 as f32 * t;
    scene.objects.push(other);
    assert!(!scene.is_visible(scene.objects.last().unwrap()));
    let (sx, sy) = (scene.objects[i].x, scene.objects[i].y);
    let last = scene.objects.len() - 1;
    scene.objects[last].x = sx + t;
    scene.objects[last].y = sy;
    assert!(scene.is_visible(&scene.objects[last]));
    scene.explore_all();
    assert!((0..w * h).all(|i| scene.seen(i % w, i / w) != Seen::Unexplored));
}

/// A workshop improves heavy weapons, and they strike harder (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn workshop_research() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    // The campaign has not revealed it all yet.
    scene.permits = crate::permits::Permits::ALL;
    scene.amber = 10_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, WORKSHOP, x, y).is_ok())
        .unwrap();
    scene
        .place_building(home, WORKSHOP, site.0, site.1, &data, &mut caches)
        .unwrap();
    let id = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(WORKSHOP))
        .unwrap()
        .id;
    for _ in 0..50 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    let heavy = combat::Upgrade::Arms(2);
    assert!(
        scene.start_research(id, combat::Upgrade::Armour).is_err(),
        "the school's, not ours"
    );
    let amber = scene.amber;
    scene.start_research(id, heavy).unwrap();
    assert_eq!(scene.amber, amber - 1800);
    for _ in 0..50 * (heavy.seconds(0) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    let levels = scene.levels_of(scene.human.map(|h| (h.nation, h.slot)));
    assert_eq!(levels.arms, [0, 0, 1]);
    // Ballistae (item 6) use the heavy weapons' level; gastraphetes (4) not.
    assert_eq!(combat::hull_damage_at(6, levels.arms_for(6)), 240.0);
    assert_eq!(combat::hull_damage_at(4, levels.arms_for(4)), 60.0);
    // Marines' rate of fire cannot be raised; the strong weapons' range can.
    assert!(scene.next_level(combat::Upgrade::Rate(0)).is_none());
    let reach = combat::Upgrade::Reach(2);
    scene.start_research(id, reach).unwrap();
    for _ in 0..50 * (reach.seconds(0) as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    let levels = scene.levels_of(scene.human.map(|h| (h.nation, h.slot)));
    assert_eq!(levels.reach, [0, 0, 1]);
    assert!(combat::reach(7, levels.reach_for(7)).1 > combat::reach(7, 0).1);
}

/// Ships are repaired at a dock and buildings by a settlement, for amber
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn repairs() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    // The mission's computer fleets stay home: this is about the mending.
    scene.brain = None;
    scene.amber = 10_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap()
        .id;
    // The settlement mends itself.
    let h = scene.object_index(home).unwrap();
    let c = scene.objects[h].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    scene.order_mend(home, home).unwrap();
    // A ship sails to a dock (here the settlement is made one) and is mended.
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    assert!(scene.order_repair(ship, home).is_err(), "not a dock");
    let i = scene.object_index(ship).unwrap();
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    let amber = scene.amber;
    for _ in 0..50 * 60 {
        scene.update(0.02, &data, &mut caches);
    }
    let h = scene.object_index(home).unwrap();
    let c = scene.objects[h].combat.as_ref().unwrap();
    assert_eq!(c.hull, c.max_hull, "the settlement is whole again");
    assert!(scene.amber < amber, "repairs cost amber");
    let dock = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(_)) && o.id != home)
        .map(|o| o.id)
        .unwrap_or(home);
    let d = scene.object_index(dock).unwrap();
    scene.objects[d].class = ObjectClass::Building(economy::DOCK);
    scene.order_repair(ship, dock).unwrap();
    for _ in 0..50 * 120 {
        scene.update(0.02, &data, &mut caches);
    }
    let i = scene.object_index(ship).unwrap();
    let c = scene.objects[i].combat.as_ref().unwrap();
    assert_eq!(c.hull, c.max_hull, "the ship is whole again");
    assert!(scene.objects[i].ship.as_ref().unwrap().task.is_none());
}

/// Behaviour switches: a ship off guard holds its fire, one set to ram
/// goes in, and one firing on aggressors turns on its attacker (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn behaviours() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let human = scene.human.unwrap();
    let mine = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let enemy = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && o.id != mine)
        .unwrap()
        .id;
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == mine || o.id == enemy);
    scene
        .objects
        .retain(|o| o.ship.is_some() || o.combat.as_ref().is_none_or(|c| c.weapons.is_empty()));
    let m = scene.object_index(mine).unwrap();
    let (mx, my) = (scene.objects[m].x, scene.objects[m].y);
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].owner = Some((1, (human.slot + 1) % 4));
    scene.objects[e].x = mx + 128.0;
    scene.objects[e].y = my;
    for id in [mine, enemy] {
        let i = scene.object_index(id).unwrap();
        let c = scene.objects[i].combat.as_mut().unwrap();
        c.weapons = vec![4; 2];
        c.max_hull = 100_000.0;
        c.hull = 100_000.0;
        let s = scene.objects[i].ship.as_mut().unwrap();
        s.kind = 5;
        s.path.clear();
        s.task = None;
    }
    let hull = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull
    };
    let task = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .task
    };
    // Neither guards: nobody fires.
    scene
        .toggle_behaviour(mine, crate::commands::GUARD)
        .unwrap();
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].ship.as_mut().unwrap().behaviour.guard = false;
    assert!(scene
        .toggle_behaviour(mine, crate::commands::FIRE_ON_AGGRESSOR)
        .is_err());
    for _ in 0..50 * 5 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(hull(&scene, enemy), 100_000.0, "held fire");
    // On guard and firing on aggressors: when the enemy opens fire, we turn
    // on it.
    scene
        .toggle_behaviour(mine, crate::commands::GUARD)
        .unwrap();
    scene
        .toggle_behaviour(mine, crate::commands::FIRE_ON_AGGRESSOR)
        .unwrap();
    let m = scene.object_index(mine).unwrap();
    scene.objects[m].x -= 64.0 * 6.0; // out of our own range
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].ship.as_mut().unwrap().task = Some(Task::Attack { target: mine });
    for _ in 0..50 * 10 {
        scene.update(0.02, &data, &mut caches);
        if task(&scene, mine).is_some() {
            break;
        }
    }
    assert_eq!(task(&scene, mine), Some(Task::Attack { target: enemy }));
    // Set to ram, an idle guard rams instead of firing.
    let m = scene.object_index(mine).unwrap();
    scene.objects[m].ship.as_mut().unwrap().task = None;
    scene
        .toggle_behaviour(mine, crate::commands::RAM_ATTACK)
        .unwrap();
    for _ in 0..50 * 3 {
        scene.update(0.02, &data, &mut caches);
        if matches!(task(&scene, mine), Some(Task::Ram { .. })) {
            break;
        }
    }
    assert!(matches!(task(&scene, mine), Some(Task::Ram { target }) if target == enemy));
}

/// A boat hunts a leviathan and brings it home for 5000 amber and 2500
/// fish; the zone is then empty (needs `OPENCONQUEST_DATA`).
#[test]
fn leviathan_hunt() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let boat = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .unwrap();
    let (bx, by, boat) = (boat.x, boat.y, boat.id);
    // The nearest fishing ground stands in for a leviathan zone.
    let zone = scene
        .objects
        .iter()
        .filter(|o| o.deposit.is_some_and(|d| d.resource == Resource::Fish))
        .min_by(|a, b| {
            let d = |o: &&MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
            d(a).total_cmp(&d(b))
        })
        .unwrap()
        .id;
    let z = scene.object_index(zone).unwrap();
    // A leviathan of 100, taken 50 a cast of the net.
    scene.objects[z].deposit = Some(crate::objects::Deposit {
        resource: Resource::Leviathan,
        remaining: Some(100.0),
        regrows: 0,
        refill: 0.0,
    });
    scene.order_gather(boat, zone).unwrap();
    let (amber, fish) = (scene.amber, scene.fish);
    for _ in 0..60 * 600 {
        scene.update(1.0 / 60.0, &data, &mut caches);
        if scene.fish > fish {
            break;
        }
    }
    assert_eq!((scene.amber, scene.fish), (amber + 5000, fish + 2500));
    assert!(scene.object_index(zone).is_none(), "the leviathan is gone");
}

/// Waypoints add legs to a ship's route (needs `OPENCONQUEST_DATA`).
#[test]
fn waypoints() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut scene = Scene::load(&data, &mut Caches::default(), "miss000").unwrap();
    let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap();
    let (id, x, y) = (ship.id, ship.x, ship.y);
    let t = TILE_SIZE as f32;
    let water = |s: &Scene, dx: i32| {
        let c = ((x / t) as i32 + dx, (y / t) as i32);
        let w = s.nav.nearest_water(c.0, c.1).unwrap();
        (w.0 as f32 * t + t / 2.0, w.1 as f32 * t + t / 2.0)
    };
    let (a, b) = (water(&scene, 3), water(&scene, -3));
    assert!(scene.order_move(id, a.0, a.1));
    let first = scene.objects[scene.object_index(id).unwrap()]
        .ship
        .as_ref()
        .unwrap()
        .path
        .len();
    assert!(scene.order_waypoint(id, b.0, b.1));
    let path = &scene.objects[scene.object_index(id).unwrap()]
        .ship
        .as_ref()
        .unwrap()
        .path;
    assert!(path.len() > first);
    assert_eq!(path[first - 1], a, "through the first point");
    assert_eq!(*path.back().unwrap(), b, "on to the second");
}

/// A scout sails out and back; a patrol turns at each end (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn scout_and_patrol() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap();
    let (id, x, y) = (ship.id, ship.x, ship.y);
    scene.objects.retain(|o| o.ship.is_none() || o.id == id);
    let t = TILE_SIZE as f32;
    let c = scene
        .nav
        .nearest_water((x / t) as i32 + 4, (y / t) as i32)
        .unwrap();
    let far = (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
    let at = |s: &Scene| {
        let o = &s.objects[s.object_index(id).unwrap()];
        (o.x, o.y, o.ship.as_ref().unwrap().task)
    };
    let near =
        |a: (f32, f32), b: (f32, f32)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() < t;
    scene.order_scout(id, far.0, far.1).unwrap();
    let mut reached = false;
    for _ in 0..50 * 120 {
        scene.update(0.02, &data, &mut caches);
        let (px, py, _) = at(&scene);
        reached |= near((px, py), far);
    }
    let (px, py, task) = at(&scene);
    assert!(reached, "went out");
    assert!(near((px, py), (x, y)) && task.is_none(), "came home");
    scene.order_patrol(id, far.0, far.1).unwrap();
    let mut turns = 0;
    let mut last = at(&scene).2;
    for _ in 0..50 * 240 {
        scene.update(0.02, &data, &mut caches);
        let task = at(&scene).2;
        if task != last {
            turns += 1;
            last = task;
        }
    }
    assert!(turns >= 2, "turned {turns} times");
    assert!(matches!(last, Some(Task::Patrol { .. })));
}

/// A ship guarding a zone fights an enemy that comes into it; an attack
/// zone order ends once the zone is clear (needs `OPENCONQUEST_DATA`).
#[test]
fn zones() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let human = scene.human.unwrap();
    let mine = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let enemy = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && o.id != mine)
        .unwrap()
        .id;
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == mine || o.id == enemy);
    scene
        .objects
        .retain(|o| o.ship.is_some() || o.combat.as_ref().is_none_or(|c| c.weapons.is_empty()));
    let m = scene.object_index(mine).unwrap();
    let (mx, my) = (scene.objects[m].x, scene.objects[m].y);
    scene.objects[m].combat.as_mut().unwrap().weapons = vec![6; 2];
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].owner = Some((1, (human.slot + 1) % 4));
    scene.objects[e].combat.as_mut().unwrap().weapons.clear();
    scene.objects[e].ship.as_mut().unwrap().crew.squads = [0; 3];
    // The enemy lies outside the zone at first.
    scene.objects[e].x = mx + 64.0 * 8.0;
    scene.objects[e].y = my;
    let zone = (
        (mx - 64.0 * 3.0, my - 64.0 * 3.0),
        (mx + 64.0 * 3.0, my + 64.0 * 3.0),
    );
    assert!(
        scene.order_zone(enemy, zone.0, zone.1, false).is_err(),
        "unarmed"
    );
    scene.order_zone(mine, zone.0, zone.1, false).unwrap();
    let hull = |s: &Scene| {
        s.objects[s.object_index(enemy).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull
    };
    let before = hull(&scene);
    for _ in 0..50 * 5 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(hull(&scene), before, "outside the zone, left alone");
    let task = |s: &Scene| {
        s.objects[s.object_index(mine).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .task
    };
    assert!(matches!(
        task(&scene),
        Some(Task::Zone { attack: false, .. })
    ));
    // It sails in and is fought.
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].x = mx + 64.0 * 2.0;
    for _ in 0..50 * 10 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.object_index(enemy).is_none_or(|e| scene.objects[e]
        .combat
        .as_ref()
        .unwrap()
        .hull
        < before));
    // An attack zone with nothing in it is done at once.
    scene
        .order_zone(mine, (mx - 64.0, my - 64.0), (mx - 32.0, my - 32.0), true)
        .unwrap();
    scene.update(0.02, &data, &mut caches);
    assert_eq!(task(&scene), None);
}

/// A click on amber with a boat selected makes the boat a group with a
/// gather zone of the 3 x 3 cells about the amber (0x49d3c0, flags
/// 0x100); the zone sends it there, port 0, through the outbox (0x4c8a80
/// -> 0x4c6ee0 -> 0x4c8f20 -> 0x4c9270); moving it by hand takes it out
/// of the group (needs `OPENCONQUEST_DATA`).
#[test]
fn gathering_zone() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    let boat = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .unwrap()
        .id;
    let deposit = scene
        .objects
        .iter()
        .find(|o| o.deposit.is_some_and(|d| d.resource == Resource::Amber))
        .unwrap();
    let d = deposit.id;
    let (cx, cy) = (
        deposit.record.fields[6] as i32,
        deposit.record.fields[7] as i32,
    );
    scene
        .apply(
            &Order::GatherAt {
                ships: vec![boat],
                deposit: d,
                keys: 0,
            },
            &data,
            &mut caches,
        )
        .unwrap();
    assert_eq!(scene.gather_zones.len(), 1);
    let z = &scene.gather_zones[0];
    assert_eq!(z.rect, (cx - 1, cy - 1, 3, 3));
    assert_eq!(z.flags, 0x100);
    assert_eq!(z.ships, [boat]);
    assert_eq!(z.deposits, [d]);
    // The command waits in the outbox till the tick's end.
    assert_eq!(
        scene.outbox,
        [Order::Gather {
            ship: boat,
            deposit: d,
            port: 0,
        }]
    );
    scene.update(crate::clock::TICK, &data, &mut caches);
    let ship = |s: &Scene| {
        s.objects[s.object_index(boat).unwrap()]
            .ship
            .clone()
            .unwrap()
    };
    assert!(matches!(ship(&scene).task, Some(Task::Gather { deposit, .. }) if deposit == d));
    let g = ship(&scene).gathered.unwrap();
    assert_eq!(g.cell, (cx, cy));
    assert_eq!(g.rect, (cx - 2, cy - 2, cx + 2, cy + 2));
    assert_eq!(ship(&scene).home_port, 0);
    let (bx, by) = {
        let o = &scene.objects[scene.object_index(boat).unwrap()];
        (o.x, o.y)
    };
    scene
        .apply(
            &Order::Move {
                ship: boat,
                x: bx,
                y: by,
            },
            &data,
            &mut caches,
        )
        .unwrap();
    assert_eq!(scene.ship_zone(boat), None);
    assert!(scene.gather_zones.is_empty());
}

/// An escort follows the ship it convoys (needs `OPENCONQUEST_DATA`).
#[test]
fn convoy() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let own: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| scene.is_own(o) && o.ship.is_some())
        .map(|o| o.id)
        .take(2)
        .collect();
    let (escort, ward) = (own[0], own[1]);
    scene
        .objects
        .retain(|o| o.ship.is_none() || own.contains(&o.id));
    let pos = |s: &Scene, id| {
        let o = &s.objects[s.object_index(id).unwrap()];
        (o.x, o.y)
    };
    let w = pos(&scene, ward);
    assert!(scene
        .order_convoy(escort, (w.0 + 500.0, w.1), (w.0 + 600.0, w.1 + 10.0))
        .is_err());
    scene
        .order_convoy(escort, (w.0 - 8.0, w.1 - 8.0), (w.0 + 8.0, w.1 + 8.0))
        .unwrap();
    // The ward sails off; the escort keeps up.
    let t = TILE_SIZE as f32;
    let c = scene
        .nav
        .nearest_water((w.0 / t) as i32 + 5, (w.1 / t) as i32)
        .unwrap();
    assert!(scene.order_move(ward, c.0 as f32 * t + 32.0, c.1 as f32 * t + 32.0));
    for _ in 0..50 * 90 {
        scene.update(0.02, &data, &mut caches);
    }
    let (a, b) = (pos(&scene, escort), pos(&scene, ward));
    let d = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    assert!(d <= 3.0 * t, "escort {d} away");
}

/// Treasures: a barrel pays at once, a lasting gift goes aboard, and a
/// hero's item brings the hero from his house (needs `OPENCONQUEST_DATA`).
#[test]
fn treasures_and_heroes() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    // miss003 has treasures lying about and Theseus's house.
    let mut scene = Scene::load(&data, &mut caches, "miss003").unwrap();
    // Not a test of the mission's own computer player.
    scene.brain = None;
    let treasures = scene
        .objects
        .iter()
        .filter(|o| o.treasure.is_some())
        .count();
    assert!(treasures >= 5, "{treasures} treasures on the map");
    let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap().id;
    let human = scene.human.unwrap();
    scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].owner = Some((human.nation, human.slot));
    scene.objects[i].ship.as_mut().unwrap().kind = 2; // a royal ship
    let (sx, sy) = (scene.objects[i].x, scene.objects[i].y);
    // Put a treasure right by the ship and take it.
    let place = |scene: &mut Scene, kind: u8, amount: u32| {
        let t = scene
            .objects
            .iter()
            .position(|o| o.treasure.is_some())
            .unwrap();
        scene.objects[t].treasure = Some(crate::objects::Treasure {
            kind,
            param: 0,
            amount,
        });
        scene.objects[t].x = sx + 20.0;
        scene.objects[t].y = sy;
        scene.objects[t].id
    };
    let amber = scene.amber;
    let barrel = place(&mut scene, 1, 600);
    scene.order_take(ship, barrel).unwrap();
    for _ in 0..50 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.amber, amber + 600);
    assert!(scene.object_index(barrel).is_none(), "taken");
    // Ariadne's clew (Theseus's item) goes aboard.
    let clew = place(&mut scene, 19, 0);
    scene.order_take(ship, clew).unwrap();
    for _ in 0..50 {
        scene.update(0.02, &data, &mut caches);
    }
    let s = |scene: &Scene| {
        scene.objects[scene.object_index(ship).unwrap()]
            .ship
            .clone()
            .unwrap()
    };
    assert!(s(&scene).has_treasure(19));
    // Theseus comes for it.
    let house = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::HeroHouse && o.record.field(22) == Some(1))
        .unwrap()
        .id;
    scene.order_take(ship, house).unwrap();
    for _ in 0..50 * 300 {
        scene.update(0.02, &data, &mut caches);
        if !s(&scene).heroes.is_empty() {
            break;
        }
    }
    assert_eq!(s(&scene).heroes, vec![crate::objects::THESEUS]);
    assert!(!s(&scene).has_treasure(19), "the clew stays with Theseus");
}

/// The Gorgon turns ships in her area to stone (not Perseus's), and a siren
/// draws ships towards her (needs `OPENCONQUEST_DATA`).
#[test]
fn gorgon_and_siren() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    let setup = |mission: &str, class: u32, caches: &mut Caches| {
        let mut scene = Scene::load(&data, caches, mission).unwrap();
        let c = scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::Monster(class))
            .unwrap();
        let (cx, cy) = (c.x, c.y);
        // Two Greek rammers of player 1, built from ship records.
        scene.objects.retain(|o| o.ship.is_none());
        let mut ships = Vec::new();
        for k in 0..2u32 {
            let id = 900_000 + k;
            let mut fields = vec![0u32; 13];
            fields[0] = 0xc8;
            fields[1] = 1;
            fields[5] = 1;
            fields[7] = 5;
            fields[12] = 100;
            let rec = ObjectRecord {
                id,
                class_id: 0xc8,
                fields,
            };
            let obj = crate::objects::resolve(&data, caches, &rec)
                .unwrap()
                .unwrap();
            scene.objects.push(obj);
            ships.push(id);
        }
        // Keep the creature's neighbours from fighting.
        for o in scene.objects.iter_mut() {
            if let Some(c) = o.combat.as_mut() {
                c.weapons.clear();
            }
        }
        // Put the ships on water within a cell or two of the creature.
        let w = scene
            .nav
            .nearest_water((cx / t) as i32 + 2, (cy / t) as i32)
            .unwrap();
        for &id in &ships {
            let i = scene.object_index(id).unwrap();
            scene.objects[i].x = w.0 as f32 * t + t / 2.0;
            scene.objects[i].y = w.1 as f32 * t + t / 2.0;
            scene.objects[i].ship.as_mut().unwrap().path.clear();
            scene.objects[i].ship.as_mut().unwrap().task = None;
        }
        (scene, ships, (cx, cy))
    };
    let (mut scene, ships, _) = setup("battle17", 0x1fa, &mut caches);
    let i = scene.object_index(ships[1]).unwrap();
    scene.objects[i].ship.as_mut().unwrap().heroes.push(8); // Perseus
                                                            // Her blow would sink them; she rests up to 500 ticks before rising.
    for &id in &ships {
        let i = scene.object_index(id).unwrap();
        let c = scene.objects[i].combat.as_mut().unwrap();
        (c.max_hull, c.hull) = (1e7, 1e7);
    }
    let stone = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .stone
    };
    let (mut stoned, mut perseus) = (false, false);
    // Held where they were put (sharing a cell, they would make room for
    // each other and sail off).
    let held: Vec<(f32, f32)> = ships
        .iter()
        .map(|&id| {
            let o = &scene.objects[scene.object_index(id).unwrap()];
            (o.x, o.y)
        })
        .collect();
    for _ in 0..50 * 60 {
        scene.update(0.02, &data, &mut caches);
        for (&id, &(x, y)) in ships.iter().zip(&held) {
            let i = scene.object_index(id).unwrap();
            (scene.objects[i].x, scene.objects[i].y) = (x, y);
            scene.objects[i].ship.as_mut().unwrap().path.clear();
        }
        stoned |= stone(&scene, ships[0]) > 0.0;
        perseus |= stone(&scene, ships[1]) > 0.0;
        if stoned {
            break;
        }
    }
    assert!(stoned, "turned to stone");
    assert!(!perseus, "Perseus is safe");

    let (mut scene, ships, (sx, sy)) = setup("battle08", 0x1fd, &mut caches);
    let dist = |s: &Scene| {
        let o = &s.objects[s.object_index(ships[0]).unwrap()];
        ((o.x - sx).powi(2) + (o.y - sy).powi(2)).sqrt()
    };
    // She sings within 250-1000 ticks of surfacing and the ship heads
    // for her.
    let before = dist(&scene);
    let mut charmed = false;
    let siren = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Monster(0x1fd))
        .unwrap()
        .id;
    scene.update(0.02, &data, &mut caches);
    assert!(scene.creature_hidden(siren), "she starts under the water");
    for _ in 0..50 * 45 {
        scene.update(0.02, &data, &mut caches);
        if !charmed && scene.charmed.contains_key(&ships[0]) {
            assert!(!scene.creature_hidden(siren), "she is up when she sings");
        }
        charmed |= scene.charmed.contains_key(&ships[0]);
        if charmed && dist(&scene) < before {
            break;
        }
    }
    assert!(charmed, "the siren sang");
    assert!(
        dist(&scene) < before,
        "drawn towards the siren: {} -> {}",
        before,
        dist(&scene)
    );
}

/// Quicklime kills crews without touching the hull; a fire rammer's barrel
/// of powder goes off in the ram (needs `OPENCONQUEST_DATA`).
#[test]
fn crews_under_fire() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let human = scene.human.unwrap();
    let mine = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let enemy = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && o.id != mine)
        .unwrap()
        .id;
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == mine || o.id == enemy);
    scene
        .objects
        .retain(|o| o.ship.is_some() || o.combat.as_ref().is_none_or(|c| c.weapons.is_empty()));
    let m = scene.object_index(mine).unwrap();
    let (mx, my) = (scene.objects[m].x, scene.objects[m].y);
    scene.objects[m].combat.as_mut().unwrap().weapons = vec![combat::QUICKLIME];
    scene.objects[m].ship.as_mut().unwrap().crew.squads = [0; 3];
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].owner = Some((1, (human.slot + 1) % 4));
    scene.objects[e].x = mx + 100.0;
    scene.objects[e].y = my;
    scene.objects[e].combat.as_mut().unwrap().weapons = vec![4, 4];
    scene.objects[e].ship.as_mut().unwrap().crew.squads = [0, 0, 4];
    // Its own guns would not fire limed; keep it from firing at all.
    scene.objects[e].ship.as_mut().unwrap().behaviour.guard = false;
    let hull = scene.objects[e].combat.as_ref().unwrap().hull;
    let mut limed = 0.0f32;
    // Long enough for the hits the draws give.
    for _ in 0..50 * 80 {
        scene.update(0.02, &data, &mut caches);
        let e = scene.object_index(enemy).unwrap();
        limed = limed.max(scene.objects[e].combat.as_ref().unwrap().lime);
    }
    let e = scene.object_index(enemy).unwrap();
    let units = scene.objects[e].unit_conditions();
    assert!(
        units[2].iter().map(|&c| c as u32).sum::<u32>() < 400,
        "crews burnt"
    );
    assert!(
        units[4].iter().map(|&c| c as u32).sum::<u32>() < 200,
        "weapons worn"
    );
    assert_eq!(
        scene.objects[e].combat.as_ref().unwrap().hull,
        hull,
        "hull untouched"
    );
    assert!(limed > 0.0, "limed (0x4c4120)");
    // A barrel of powder adds its blast to a ram, once.
    let m = scene.object_index(mine).unwrap();
    scene.objects[m].combat.as_mut().unwrap().weapons = vec![combat::POWDER];
    // Both sturdy enough to outlast the ram.
    for k in [m, e] {
        let c = scene.objects[k].combat.as_mut().unwrap();
        (c.max_hull, c.hull) = (1e6, 1e6);
    }
    let before = scene.objects[e].combat.as_ref().unwrap().hull;
    scene.ram(m, e, &data, &mut caches);
    let e = scene.object_index(enemy).unwrap();
    let lost = before - scene.objects[e].combat.as_ref().unwrap().hull;
    assert!(lost >= combat::hull_damage(combat::POWDER), "lost {lost}");
    let m = scene.object_index(mine).unwrap();
    assert!(
        scene.objects[m].combat.as_ref().unwrap().weapons.is_empty(),
        "spent"
    );
}

/// Unique ships need their study at the sage school, which needs the
/// arming at level 2 (needs `OPENCONQUEST_DATA`).
#[test]
fn unique_ships_need_study() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    // The campaign has not revealed it all yet.
    scene.permits = crate::permits::Permits::ALL;
    let human = scene.human.unwrap();
    let owner = (human.nation, human.slot);
    let first = if human.nation == 1 { 0 } else { 2 };
    let kind = 8 + first;
    scene.amber = 100_000;
    scene.fish = 100_000;
    let err = scene.can_build_ship(kind).unwrap_err();
    assert!(err.contains("studied"), "{err}");
    let mut levels = scene.levels_of(Some(owner));
    levels.set(combat::Upgrade::Special(first), 1);
    scene.levels.insert(owner, levels);
    assert!(!scene
        .can_build_ship(kind)
        .err()
        .unwrap_or_default()
        .contains("studied"));
    assert_eq!(
        combat::Upgrade::of_command(0x52),
        Some(combat::Upgrade::Special(2))
    );
    assert_eq!(combat::Upgrade::Special(0).max_level(), 1);
}

/// From the start a workshop makes medium weapons (the original's arming
/// starts at 1), strong ones after the school's arming; a settlement may
/// put up the forts whose fallback weapons can be had, the stone fort only
/// at a town (needs `OPENCONQUEST_DATA`).
#[test]
fn arming_opens_weapons_and_forts() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.permits = crate::permits::Permits::ALL;
    let human = scene.human.unwrap();
    let owner = (human.nation, human.slot);
    let own: Vec<usize> = (0..scene.objects.len())
        .filter(|&i| {
            scene.is_own(&scene.objects[i])
                && matches!(scene.objects[i].class, ObjectClass::Building(_))
        })
        .take(2)
        .collect();
    assert_eq!(own.len(), 2, "two buildings to work with");
    scene.objects[own[0]].class = ObjectClass::Building(economy::SETTLEMENT);
    scene.objects[own[1]].class = ObjectClass::Building(WORKSHOP);
    let site = scene.objects[own[0]].id;
    // The workshop is the settlement's (its list, `+0x1d0`).
    let shop = scene.objects[own[1]].id;
    scene.towns.lists.insert(site, vec![shop]);
    scene.towns.of.insert(shop, site);
    assert_eq!(scene.levels_of(Some(owner)).weapons, 0);
    assert!(scene.can_make_at(Some(site), 4) && !scene.can_make_at(Some(site), 6));
    assert!(scene.fort_allowed(site, 0x34) && scene.fort_allowed(site, 0x35));
    assert!(!scene.fort_allowed(site, 0x36), "stone forts at towns only");
    scene.objects[own[0]].class = ObjectClass::Building(TOWN);
    // The stone fort falls back on ballistae (6), out of reach yet.
    assert!(!scene.fort_allowed(site, 0x36));
    let mut levels = scene.levels_of(Some(owner));
    levels.weapons = 1;
    scene.levels.insert(owner, levels);
    assert!(scene.can_make_at(Some(site), 6) && scene.fort_allowed(site, 0x36));
}

/// The smithy studies each armour and ram kind to level 2, the silver and
/// golden kinds once the school has studied that far, one armour and one
/// ram study at a time; armour cuts more by its level (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn smithy_studies_armour_and_ram_kinds() {
    use combat::Upgrade::{ArmourKind, RamKind};
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.permits = crate::permits::Permits::ALL;
    let human = scene.human.unwrap();
    let owner = (human.nation, human.slot);
    scene.amber = 100_000;
    let i = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(_)))
        .unwrap();
    scene.objects[i].class = ObjectClass::Building(economy::SMITHY);
    let smithy = scene.objects[i].id;
    // Silver rams wait for the school's rams.
    assert_eq!(scene.next_level(RamKind(1)), None);
    let mut levels = scene.levels_of(Some(owner));
    levels.ram = 1;
    scene.levels.insert(owner, levels);
    assert_eq!(scene.next_level(RamKind(1)), Some(0));
    let amber = scene.amber;
    scene.start_research(smithy, RamKind(1)).unwrap();
    assert_eq!(scene.amber, amber - 600);
    // One ram study at a time; armour may go on elsewhere.
    assert_eq!(scene.next_level(RamKind(0)), None);
    assert_eq!(scene.next_level(ArmourKind(0)), Some(0));
    scene.run_research(RamKind(1).seconds(0) + 0.1);
    assert_eq!(scene.levels_of(Some(owner)).ram_kinds, [0, 1, 0]);
    assert_eq!(scene.next_level(RamKind(1)), Some(1));
    // Silver armour at level 1 cuts 27 percent, not 20.
    let mut levels = scene.levels_of(Some(owner));
    assert!((combat::armour_factor(1, &levels) - 0.8).abs() < 1e-6);
    levels.set(ArmourKind(1), 1);
    assert!((combat::armour_factor(1, &levels) - 0.73).abs() < 1e-6);
    assert_eq!(combat::Upgrade::of_command(0x4b), Some(ArmourKind(2)));
    assert_eq!(ArmourKind(2).cost(1), 900);
}

/// Campaign missions are judged by their story: miss000 is won with the
/// barbarians gone and the parchment aboard (needs `OPENCONQUEST_DATA`).
#[test]
fn campaign_story() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    for m in 0..14 {
        let scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).unwrap();
        let names: Vec<String> = scene.story.iter().map(|&g| goal_name(g)).collect();
        println!("miss{m:03}: {}", names.join("; "));
        assert!(!scene.story.is_empty());
    }
    // Jason's oar is not on miss001's map: a barbarian structure (object 6)
    // holds it, and drops it when destroyed.
    let mut scene = Scene::load(&data, &mut caches, "miss001").unwrap();
    assert!(!scene
        .objects
        .iter()
        .any(|o| o.treasure.is_some_and(|t| t.kind == 18)));
    let holder = scene.held.iter().find(|h| h.1.kind == 18).unwrap().0;
    assert_eq!(holder, 6);
    scene.hit(holder, f32::MAX, None, &data, &mut caches);
    assert!(scene
        .objects
        .iter()
        .any(|o| o.treasure.is_some_and(|t| t.kind == 18)));
    assert!(!scene.held.iter().any(|h| h.1.kind == 18));
    let scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    assert_eq!(
        scene.story,
        vec![
            Goal::Joined(7),
            Goal::Argo,
            Goal::Assemble((0, 0, 2, 2)),
            Goal::Hero(0)
        ]
    );
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    assert_eq!(scene.story, vec![Goal::Kill(0), Goal::Aboard(27)]);
    // No barbarian left: not yet won without the parchment.
    let human = scene.human.map(|h| (h.nation, h.slot));
    scene
        .objects
        .retain(|o| o.owner.is_none() || o.owner == human || o.owner == Some(combat::MONSTERS));
    for _ in 0..50 * 6 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.outcome, None);
    let ship = scene
        .objects
        .iter()
        .position(|o| o.ship.is_some() && o.owner == human)
        .unwrap();
    scene.objects[ship]
        .ship
        .as_mut()
        .unwrap()
        .treasures
        .push(crate::objects::Treasure {
            kind: 27,
            param: 0,
            amount: 0,
        });
    // Judged every 125 ticks, won once that has held for 250.
    for _ in 0..50 * 6 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.outcome, None, "not held long enough");
    for _ in 0..50 * 10 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.outcome, Some(Outcome::Victory));
}

/// miss000's brain: its fleets are the barbarian ships, the merchant goes
/// fishing, and the guards attack when the player comes into a bay (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss000_brain() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let sizes: Vec<usize> = scene
        .brain
        .as_ref()
        .unwrap()
        .fleets
        .iter()
        .map(|f| f.ships.len())
        .collect();
    assert_eq!(sizes, vec![2, 1, 2]);
    // Keep the player's ships out of the way at first.
    for _ in 0..50 * 10 {
        scene.update(0.02, &data, &mut caches);
    }
    let brain = scene.brain.clone().unwrap();
    let merchant = brain.fleets[1].ships[0];
    assert!(scene.ship_zone(merchant).is_some(), "fishing");
    assert_eq!(brain.fleets[0].state, 5, "the guards patrol");
    // A ship of the player's sails into the western bay.
    let t = TILE_SIZE as f32;
    let human = scene.human.map(|h| (h.nation, h.slot));
    let mine = scene
        .objects
        .iter()
        .position(|o| o.ship.is_some() && o.owner == human)
        .unwrap();
    let w = scene.nav.nearest_water(9, 19).unwrap();
    scene.objects[mine].x = w.0 as f32 * t + t / 2.0;
    scene.objects[mine].y = w.1 as f32 * t + t / 2.0;
    for _ in 0..50 * 2 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(
        scene.brain.as_ref().unwrap().fleets[0].state,
        20,
        "the guards attack"
    );
}

/// Later campaign missions bring the heroes won so far, waiting at the
/// starting settlement for a ship to take them aboard (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn campaign_heroes_carry_over() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    // miss005 holds no town of the player's: picked from the list, they
    // wait in the bare one where the carried town would stand.
    let scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    assert_eq!(scene.carried, vec![0, 5, 1, 8]);
    let town = scene.own_sites()[0];
    assert_eq!(scene.site_heroes.get(&town), Some(&vec![0, 5, 1, 8]));
    // miss003 starts without a ship: the Argo comes, with Jason and Argus,
    // where the view starts.
    let scene = Scene::load(&data, &mut caches, "miss003").unwrap();
    let argo = scene
        .objects
        .iter()
        .find(|o| o.ship.as_ref().is_some_and(|s| s.argo))
        .unwrap();
    assert_eq!(argo.ship.as_ref().unwrap().heroes, vec![0, 5]);
    let (vx, vy) = scene.start_view();
    assert!((argo.x - vx).abs() < 600.0 && (argo.y - vy).abs() < 600.0);
    // Heroes waiting at a settlement go aboard a ship that comes for them.
    let mut scene = Scene::load(&data, &mut caches, "miss004").unwrap();
    let site = scene.start_sites[0];
    for o in scene.objects.iter_mut() {
        if let Some(s) = o.ship.as_mut() {
            s.heroes.clear();
        }
    }
    scene.site_heroes.insert(site, scene.carried.clone());
    let waiting = scene.site_heroes[&site].clone();
    assert_eq!(waiting, scene.carried);
    let s = scene.object_index(site).unwrap();
    let (sx, sy) = (scene.objects[s].x, scene.objects[s].y);
    // A royal ship of the player's (room for two heroes).
    let h = scene.human.unwrap();
    let mut fields = vec![0u32; 13];
    fields[0] = 0xc8;
    fields[1] = 1;
    fields[5] = h.nation;
    fields[6] = h.slot;
    fields[7] = 2;
    fields[12] = 100;
    let rec = ObjectRecord {
        id: 900_000,
        class_id: 0xc8,
        fields,
    };
    let obj = crate::objects::resolve(&data, &mut caches, &rec)
        .unwrap()
        .unwrap();
    scene.objects.push(obj);
    let i = scene.objects.len() - 1;
    let w = scene
        .nav
        .nearest_water((sx / 64.0) as i32, (sy / 64.0) as i32)
        .unwrap();
    scene.objects[i].x = w.0 as f32 * 64.0 + 32.0;
    scene.objects[i].y = w.1 as f32 * 64.0 + 32.0;
    let id = scene.objects[i].id;
    scene.load_crew(id).unwrap();
    let i = scene.object_index(id).unwrap();
    let aboard = scene.objects[i].ship.as_ref().unwrap().heroes.clone();
    assert_eq!(aboard, waiting[..aboard.len()].to_vec());
    assert!(!aboard.is_empty());
}

/// miss001's brain: the barbarians' yard builds the ships its fleets ask
/// for, paid from their purse (needs `OPENCONQUEST_DATA`).
#[test]
fn miss001_brain_builds() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss001").unwrap();
    let sizes = |s: &Scene| -> Vec<usize> {
        s.brain
            .as_ref()
            .unwrap()
            .fleets
            .iter()
            .map(|f| f.ships.len())
            .collect()
    };
    assert_eq!(sizes(&scene), vec![2, 2, 0, 0, 0]);
    for _ in 0..50 * 240 {
        scene.update(0.02, &data, &mut caches);
    }
    let after = sizes(&scene);
    let built: usize = after[2..].iter().sum();
    assert!(built >= 2, "ships built for the fleets: {after:?}");
    // The deposits the brain laid.
    assert!(scene.objects.iter().filter(|o| o.deposit.is_some()).count() >= 4);
}

/// miss002's brain: two computer players, each building for its own fleets
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn miss002_brain() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    let sizes = |s: &Scene| -> Vec<usize> {
        s.brain
            .as_ref()
            .unwrap()
            .fleets
            .iter()
            .map(|f| f.ships.len())
            .collect()
    };
    let start = sizes(&scene);
    println!("start {start:?}");
    assert!(
        start[0] >= 1 && start[8] >= 1,
        "both yards found: {start:?}"
    );
    assert!(start[1] >= 1, "the barbarian guard: {start:?}");
    for _ in 0..50 * 240 {
        scene.update(0.02, &data, &mut caches);
    }
    let after = sizes(&scene);
    println!("after {after:?}");
    assert!(
        after[2..8].iter().sum::<usize>() >= 2,
        "the Persians built ships: {after:?}"
    );
}

/// miss003's brain finds its fleets and both yards build (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss003_brain() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss003").unwrap();
    let sizes = |s: &Scene| -> Vec<usize> {
        s.brain
            .as_ref()
            .unwrap()
            .fleets
            .iter()
            .map(|f| f.ships.len())
            .collect()
    };
    let start = sizes(&scene);
    println!("start {start:?}");
    assert!(start[3] >= 1 && start[5] >= 1, "both yards: {start:?}");
    for _ in 0..50 * 240 {
        scene.update(0.02, &data, &mut caches);
    }
    let after = sizes(&scene);
    println!("after {after:?}");
    assert!(
        after.iter().sum::<usize>() > start.iter().sum::<usize>(),
        "ships built"
    );
}

/// Every ported brain finds some of its fleets and runs a few minutes
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn ported_brains_run() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    for m in 0..14 {
        let name = format!("miss{m:03}");
        let mut scene = Scene::load(&data, &mut caches, &name).unwrap();
        let Some(brain) = scene.brain.as_ref() else {
            continue;
        };
        let start: Vec<usize> = brain.fleets.iter().map(|f| f.ships.len()).collect();
        assert!(
            start.iter().any(|&n| n > 0),
            "{name}: no fleet found its ships: {start:?}"
        );
        for _ in 0..50 * 240 {
            scene.update(0.02, &data, &mut caches);
        }
        let after: Vec<usize> = scene
            .brain
            .as_ref()
            .unwrap()
            .fleets
            .iter()
            .map(|f| f.ships.len())
            .collect();
        println!("{name}: {start:?} -> {after:?}");
    }
}

/// The difficulty reaches the brain: miss000's bonus is 4000 amber on easy,
/// 2000 on normal and none on hard (needs `OPENCONQUEST_DATA`).
#[test]
fn difficulty_sets_bonus() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let amber: Vec<u32> = [super::EASY, super::NORMAL, super::HARD]
        .into_iter()
        .map(|d| {
            let scene = Scene::load_at(&data, &mut caches, "miss000", d).unwrap();
            assert_eq!(scene.brain.as_ref().unwrap().difficulty, d);
            scene.amber
        })
        .collect();
    // The original shows 8000 amber on Easy (4000 + 4000).
    assert_eq!(amber, [8000, 6000, 4000]);
}

/// The campaign reveals technology as it goes: miss002's player cannot
/// build bombers or study armour until the amazons' scroll of knowledge
/// reaches a settlement (needs `OPENCONQUEST_DATA`).
#[test]
fn scroll_reveals_armour() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    assert!(!scene.permits.ship(3));
    assert!(scene.next_level(crate::combat::Upgrade::Armour).is_none());
    // The amazon lair (object 0x18) holds scroll 1.
    assert!(scene
        .held
        .iter()
        .any(|(id, t)| *id == 0x18 && t.kind == 27 && t.param == 1));
    let owner = scene.human.map(|h| (h.nation, h.slot)).unwrap();
    // (No settlement yet: where it is handed over does not matter.)
    scene.hand_over(
        owner,
        0,
        crate::objects::Treasure {
            kind: 27,
            param: 1,
            amount: 1,
        },
    );
    assert_eq!(scene.next_level(crate::combat::Upgrade::Armour), Some(0));
    assert!(scene
        .messages
        .iter()
        .any(|m| m == "You can now upgrade to silver armor."));
}

/// Research, spells and revealed technology carry into the next campaign
/// mission (needs `OPENCONQUEST_DATA`).
#[test]
fn campaign_carries_research() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    let owner = scene.human.map(|h| (h.nation, h.slot)).unwrap();
    let mut levels = scene.levels_of(Some(owner));
    levels.set(crate::combat::Upgrade::Reach(1), 2);
    scene.levels.insert(owner, levels);
    scene.permits.read_scroll(11);
    let carry = scene.carry();
    let mut next = Scene::load(&data, &mut caches, "miss003").unwrap();
    assert!(!next.permits.ship(3));
    next.resume(&carry);
    let owner = next.human.map(|h| (h.nation, h.slot)).unwrap();
    assert_eq!(
        next.levels_of(Some(owner))
            .get(crate::combat::Upgrade::Reach(1)),
        2
    );
    assert!(next.permits.ship(3), "bombers stay revealed");
    assert!(next.permits.building(0x3d), "and miss003 adds temples");
}

/// miss009's script puts the fire wizard's castle at (30, 1): it strikes
/// the player's ships in reach and, destroyed, gives up the scroll of Greek
/// fire (needs `OPENCONQUEST_DATA`).
#[test]
fn fire_wizard() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    scene.brain = None;
    let t = TILE_SIZE as f32;
    let castle = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Castle(0x43))
        .unwrap();
    let (cx, cy, castle) = (castle.x, castle.y, castle.id);
    assert_eq!((cx / t) as i32, 30);
    assert!(cy < 3.0 * t);
    let ship = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    scene.objects[ship].x = cx + 3.0 * t;
    scene.objects[ship].y = cy + 3.0 * t;
    let ship = scene.objects[ship].id;
    let before = scene.objects[scene.object_index(ship).unwrap()]
        .combat
        .as_ref()
        .unwrap()
        .hull;
    for _ in 0..50 * 25 {
        scene.update(0.02, &data, &mut caches);
    }
    let after = scene
        .object_index(ship)
        .map_or(0.0, |i| scene.objects[i].combat.as_ref().unwrap().hull);
    assert!(after < before, "the wizard struck ({before} -> {after})");
    scene.hit(castle, f32::MAX, None, &data, &mut caches);
    assert!(scene
        .objects
        .iter()
        .any(|o| o.treasure.is_some_and(|t| t.kind == 27 && t.param == 13)));
}

/// miss010's dragon leaves its nest, strikes a ship nearby and, killed, is
/// followed by another while the nest has some left (needs `OPENCONQUEST_DATA`).
#[test]
fn dragon_flies_and_returns() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss010").unwrap();
    scene.brain = None;
    let t = TILE_SIZE as f32;
    let dragon = scene
        .flyers
        .iter()
        .find(|f| {
            scene.objects[scene.object_index(f.id).unwrap()]
                .record
                .class_id
                == 0x1f9
        })
        .unwrap()
        .clone();
    let nest = &scene.objects[scene.object_index(dragon.nest).unwrap()];
    let (nx, ny) = (nest.x, nest.y);
    let ship = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    scene.objects[ship].x = nx - 4.0 * t;
    scene.objects[ship].y = ny + 2.0 * t;
    // (The ship draws the dragon out.)
    // Its teeth rise as a sea monster or a land creature.
    let sown = |s: &Scene| {
        s.objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Monster(0x1f4..=0x1f6 | 0x1fb)))
            .count()
    };
    let before = sown(&scene);
    for _ in 0..50 * 30 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(sown(&scene) > before, "the dragon sowed its teeth");
    // A nest waits 55500 ticks less 6000 per field 10: shortened here.
    for n in scene.nests.iter_mut().filter(|n| n.id == dragon.nest) {
        n.delay = 5.0;
    }
    scene.hit(dragon.id, f32::MAX, None, &data, &mut caches);
    for _ in 0..50 * 60 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(
        scene
            .flyers
            .iter()
            .any(|f| f.nest == dragon.nest && f.id != dragon.id),
        "another dragon came"
    );
}

/// Lairs send out their cyclops, minotaurs and amazons, who keep to the
/// land and throw at ships in reach (needs `OPENCONQUEST_DATA`).
#[test]
fn creatures_leave_their_lairs() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss006").unwrap();
    scene.brain = None;
    let cyclops_out = |scene: &Scene| {
        scene.walkers.iter().any(|w| {
            scene
                .object_index(w.id)
                .is_some_and(|i| scene.objects[i].record.class_id == 0x1f4)
        })
    };
    // Which come out first is down to chance: wait for a cyclops.
    for _ in 0..50 * 180 {
        scene.update(0.02, &data, &mut caches);
        if scene.walkers.len() >= 3 && cyclops_out(&scene) {
            break;
        }
    }
    assert!(scene.walkers.len() >= 3, "{} out", scene.walkers.len());
    let t = TILE_SIZE as f32;
    for w in &scene.walkers {
        let o = &scene.objects[scene.object_index(w.id).unwrap()];
        assert!(!scene.nav.is_water((o.x / t) as i32, (o.y / t) as i32));
    }
    // They walk off the half they came out on, on land.
    let halves: Vec<(u32, (i32, i32))> = scene.walkers.iter().map(|w| (w.id, w.half)).collect();
    for _ in 0..50 * 30 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene
        .walkers
        .iter()
        .any(|w| halves.iter().any(|&(id, h)| id == w.id && h != w.half)));
    for w in &scene.walkers {
        let o = &scene.objects[scene.object_index(w.id).unwrap()];
        assert!(!scene.nav.is_water((o.x / t) as i32, (o.y / t) as i32));
    }
    // A ship beside a cyclops is stoned.
    // A cyclops with water within three cells.
    let (wx, wy) = scene
        .walkers
        .iter()
        .map(|w| &scene.objects[scene.object_index(w.id).unwrap()])
        .filter(|o| o.record.class_id == 0x1f4)
        .find_map(|o| {
            let (cx, cy) = ((o.x / t) as i32, (o.y / t) as i32);
            (-5..=5)
                .flat_map(|dy| (-5..=5).map(move |dx| (cx + dx, cy + dy)))
                .find(|&(x, y)| scene.nav.is_water(x, y))
        })
        .expect("no cyclops near the sea");
    let ship = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    scene.objects[ship].x = wx as f32 * t + t / 2.0;
    scene.objects[ship].y = wy as f32 * t + t / 2.0;
    scene.objects[ship].ship.as_mut().unwrap().path.clear();
    let ship = scene.objects[ship].id;
    let hull = |s: &Scene| {
        s.object_index(ship)
            .map_or(0.0, |i| s.objects[i].combat.as_ref().unwrap().hull)
    };
    let before = hull(&scene);
    for _ in 0..50 * 20 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(hull(&scene) < before, "the creatures threw at the ship");
}

/// miss002: Argus at the player's town lets them build the Argo, and the
/// mission wants it afloat with Jason and Argus aboard (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn argus_builds_the_argo() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    scene.brain = None;
    assert!(scene.story.contains(&Goal::ArgoWith));
    assert!(!scene.permits.argo());
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    // Argus waits at one of the player's buildings.
    scene.site_heroes.insert(ship, vec![5]);
    scene.update(0.02, &data, &mut caches);
    assert!(scene.permits.argo());
    assert!(!scene.goal_met(Goal::ArgoWith));
    scene.christen_argo(ship);
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].ship.as_mut().unwrap().heroes = vec![0, 5];
    assert!(scene.goal_met(Goal::ArgoWith));
    assert_eq!(scene.objects[i].label, "Argo");
}

/// Two machines, a host and a client over a local connection, play
/// battle01 together: each gives its ship an order, both play the same
/// turns and their games stay the same (needs `OPENCONQUEST_DATA`).
#[test]
fn multiplayer_lockstep() {
    use crate::net::{Net, Setup, STEP, TURN};
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let setup = Setup {
        mission: "battle01".into(),
        seats: vec![(1, 1), (3, 3)],
        difficulty: 1,
        fog: true,
    };
    let port = 20_000 + (std::process::id() % 20_000) as u16;
    let hosting = std::thread::spawn(move || Net::host(port, setup).unwrap());
    let mut client = (0..50)
        .find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            Net::join(&format!("127.0.0.1:{port}")).ok()
        })
        .unwrap();
    let mut host = hosting.join().unwrap();
    assert_eq!(client.seat, 1);
    let archive = data.open_mission("battle01").unwrap();
    let players = openconquest_formats::players(&archive).unwrap();
    let seated: Vec<_> = [(1, 1), (3, 3)]
        .iter()
        .map(|&(n, s)| {
            *players
                .iter()
                .find(|p| (p.nation, p.slot) == (n, s))
                .unwrap()
        })
        .collect();
    let mut scene_for = |seat: usize| {
        let mut s = Scene::load(&data, &mut caches, "battle01").unwrap();
        s.seat(&seated);
        s.sit(seat);
        s
    };
    let (mut a, mut b) = (scene_for(0), scene_for(1));
    // Each trains marines at its settlement.
    let own_site = |s: &Scene| {
        s.objects
            .iter()
            .find(|o| s.is_own(o) && o.ship.is_none() && o.combat.is_some())
            .map(|o| o.id)
            .unwrap()
    };
    let (ha, cb) = (own_site(&a), own_site(&b));
    assert_ne!(ha, cb);
    let funds = |s: &Scene, p: (u32, u32)| {
        if s.human.is_some_and(|h| (h.nation, h.slot) == p) {
            (s.amber, s.fish)
        } else {
            s.purses[&p]
        }
    };
    let before = (funds(&a, (1, 1)), funds(&a, (3, 3)));
    let steps = (TURN / STEP).round() as usize;
    let (mut ca, mut cc) = (Vec::new(), Vec::new());
    for t in 0..60u64 {
        if t == 1 {
            host.send(Order::Train { site: ha, item: 0 });
            client.send(Order::Train { site: cb, item: 1 });
        }
        host.poll();
        let orders = host.next_turn().expect("the host plays");
        let (check, _) = a.play_turn(&orders, 0, steps, STEP, &data, &mut caches);
        host.played(t, check);
        ca.push(check);
        let orders = (0..200)
            .find_map(|_| {
                client.poll();
                let o = client.next_turn();
                if o.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                o
            })
            .expect("the client gets the turn");
        let (check, _) = b.play_turn(&orders, 1, steps, STEP, &data, &mut caches);
        client.played(t, check);
        client.poll();
        cc.push(check);
    }
    assert_eq!(ca, cc, "both machines play the same game");
    // Chat crosses too.
    client.chat("well met");
    let heard = (0..100).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(5));
        host.poll();
        host.chat
            .iter()
            .any(|(seat, t)| *seat == 1 && t == "well met")
    });
    assert!(heard, "the host heard the client");
    host.poll();
    assert!(host.notices.is_empty(), "{:?}", host.notices);
    // Both paid, and both machines agree on it.
    for p in [(1, 1), (3, 3)] {
        assert_eq!(funds(&a, p), funds(&b, p));
    }
    assert_ne!(funds(&a, (1, 1)), before.0, "the host's player paid");
    assert_ne!(funds(&a, (3, 3)), before.1, "the client's player paid");
}

/// The outcome screen's statistics: an enemy ship destroyed and one of the
/// player's lost count, and score by the ships' points (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn statistics_and_score() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    scene.brain = None;
    let ship_of = |s: &Scene, own: bool| {
        s.objects
            .iter()
            .find(|o| {
                o.ship.is_some()
                    && s.is_own(o) == own
                    && o.owner.is_some_and(|w| w != crate::combat::MONSTERS)
            })
            .map(|o| (o.id, o.ship.as_ref().unwrap().kind))
            .unwrap()
    };
    let (enemy, ek) = ship_of(&scene, false);
    let (mine, mk) = ship_of(&scene, true);
    let human = scene.human.map(|h| (h.nation, h.slot));
    scene.hit(enemy, f32::MAX, human, &data, &mut caches);
    scene.hit(mine, f32::MAX, None, &data, &mut caches);
    assert_eq!(scene.stats.ships_destroyed[ek as usize], 1);
    assert_eq!(scene.stats.ships_lost[mk as usize], 1);
    let points = [
        200i64, 400, 1100, 1300, 900, 800, 700, 600, 600, 900, 600, 900,
    ];
    let expected = (points[ek as usize] - points[mk as usize]).max(0) / 10;
    assert_eq!(scene.stats.score() as i64, expected);
    assert_eq!(scene.stats.values(scene.amber)[11] as i64, expected);
}

/// A ship between cells (`+0x596`, set as it leaves a cell's middle and
/// cleared as it crosses into the cell it is entering, `+0x5b6`) plans
/// from the cell it is entering and sails on into it (0x4a6330 with
/// 0x4e0b50, 0x4a3560, 0x4a3f40; needs `OPENCONQUEST_DATA`).
#[test]
fn a_ship_between_cells_plans_from_the_cell_it_enters() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let t = TILE_SIZE as f32;
    let cell = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
    let centre = |(cx, cy): (i32, i32)| (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0);
    let id = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let ship = |s: &Scene| {
        let o = &s.objects[s.object_index(id).unwrap()];
        let sh = o.ship.as_ref().unwrap();
        (
            cell(o.x, o.y),
            sh.move_state,
            sh.between.get(),
            sh.entering,
            sh.path.front().copied(),
        )
    };
    let (home, ..) = ship(&scene);
    let goals: Vec<(i32, i32)> = [(8, 0), (-8, 0), (0, 8), (0, -8), (6, 6), (-6, -6)]
        .into_iter()
        .map(|(dx, dy)| (home.0 + dx, home.1 + dy))
        .collect();
    let first = goals
        .iter()
        .position(|&g| scene.order_move(id, centre(g).0, centre(g).1))
        .expect("a move");
    // Under way and not yet in the cell it is entering.
    let mut seen = None;
    for _ in 0..2000 {
        scene.update(0.04, &data, &mut caches);
        let (here, state, between, entering, _) = ship(&scene);
        if state == 5 && between {
            assert_ne!(here, entering);
            seen = Some(entering);
            break;
        }
    }
    let entering = seen.expect("between cells");
    // A new move plans from there: its first point is that cell's middle.
    assert!(goals
        .iter()
        .enumerate()
        .any(|(k, &g)| k != first && scene.order_move(id, centre(g).0, centre(g).1)));
    let (_, _, between, _, front) = ship(&scene);
    assert!(between);
    assert_eq!(front.map(|(x, y)| cell(x, y)), Some(entering));
    // Crossing into it the flag goes.
    let mut crossed = false;
    for _ in 0..500 {
        scene.update(0.04, &data, &mut caches);
        let (here, _, between, _, _) = ship(&scene);
        if here == entering {
            crossed = !between;
            break;
        }
    }
    assert!(crossed);
}

/// A ship told to stop under way is told to stop (move state 1) and goes
/// on into the cell it is going into, braking (4) to lie still (0) in its
/// middle (0x4a6330 with the stop flag, 0x4a38f0; needs
/// `OPENCONQUEST_DATA`).
#[test]
fn a_stop_under_way_ends_in_the_next_cell() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let t = TILE_SIZE as f32;
    let cell = |x: f32, y: f32| ((x / t).floor() as i32, (y / t).floor() as i32);
    let id = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    // Where it is, its move state and its next point.
    let ship = |s: &Scene| {
        let o = &s.objects[s.object_index(id).unwrap()];
        let sh = o.ship.as_ref().unwrap();
        (o.x, o.y, sh.move_state, sh.path.front().copied())
    };
    // Far off, then under way and still in the cell it last reached.
    let (ox, oy, ..) = ship(&scene);
    let mut ordered = false;
    for (dx, dy) in [(8, 0), (-8, 0), (0, 8), (0, -8), (6, 6), (-6, -6)] {
        let (cx, cy) = cell(ox, oy);
        let (x, y) = (
            (cx + dx) as f32 * t + t / 2.0,
            (cy + dy) as f32 * t + t / 2.0,
        );
        if scene.order_move(id, x, y) {
            ordered = true;
            break;
        }
    }
    assert!(ordered);
    let mut going = None;
    for _ in 0..2000 {
        scene.update(0.04, &data, &mut caches);
        if let (x, y, 5, Some((nx, ny))) = ship(&scene) {
            if cell(nx, ny) != cell(x, y) {
                going = Some(cell(nx, ny));
                break;
            }
        }
    }
    let going = going.expect("under way between cells");
    scene.stop(id);
    assert_eq!(ship(&scene).2, 1);
    let mut states = vec![1];
    for _ in 0..500 {
        scene.update(0.04, &data, &mut caches);
        let m = ship(&scene).2;
        if states.last() != Some(&m) {
            states.push(m);
        }
    }
    let (x, y, ..) = ship(&scene);
    assert_eq!(states, [1, 4, 0]);
    assert_eq!(
        (x, y),
        (going.0 as f32 * t + t / 2.0, going.1 as f32 * t + t / 2.0)
    );
}

/// miss006's brain draws the barbarian raiders' wait anew every 5 ticks
/// while the southern Persians' yard (21, 68) is found standing ((8, 6,
/// 4) + rand % 6, x 1500), and the northern raiders' once more when the
/// second of the yards is found gone; then nothing (0x45b863-0x45b993;
/// needs `OPENCONQUEST_DATA`).
#[test]
fn miss006_rerolls_the_raid_waits_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss006").unwrap();
    const NORTH: usize = 16;
    let wait = |s: &Scene, k: usize| s.brain.as_ref().unwrap().fleets[k].vars[0];
    assert_eq!(scene.brain.as_ref().unwrap().yards, [true, true]);
    assert!(scene.cell_slot_taken((21, 68)) && scene.cell_slot_taken((1, 50)));
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..500 {
        scene.run_brain();
        if scene.brain.as_ref().unwrap().tick % 5 == 0 {
            seen.insert(wait(&scene, 3));
        }
    }
    // Normal: (6 + 0..5) x 1500, every one of them drawn.
    let all: std::collections::BTreeSet<u32> = (6..12).map(|b| b * 1500).collect();
    assert_eq!(seen, all);
    // The yards gone: the latches clear, the northern wait is drawn once,
    // (6 + 0..3) x 1500, and nothing changes after.
    let gone = |s: &mut Scene, cell: (i32, i32)| {
        s.objects.retain(|o| {
            o.cell.is_none_or(|(x, y)| {
                let n = super::trade::building_cells(o);
                !((x..x + n).contains(&cell.0) && (y..y + n).contains(&cell.1))
            })
        });
    };
    // Between the members' own turns (tick % 50: 15 for (0, 3), 30 for
    // (2, 4)), two passes of the brain: the first finds both gone.
    while scene.brain.as_ref().unwrap().tick % 50 != 0 {
        scene.run_brain();
    }
    gone(&mut scene, (21, 68));
    gone(&mut scene, (1, 50));
    scene.brain.as_mut().unwrap().fleets[NORTH].vars[0] = 0;
    for _ in 0..5 {
        scene.run_brain();
    }
    assert_eq!(scene.brain.as_ref().unwrap().yards, [false, false]);
    let north = wait(&scene, NORTH);
    assert!((6..10).map(|b| b * 1500).any(|w| w == north), "{north}");
    let barbarians = wait(&scene, 3);
    for _ in 0..5 {
        scene.run_brain();
    }
    assert_eq!((wait(&scene, NORTH), wait(&scene, 3)), (north, barbarians));
}

/// miss003's brain puts creatures down after its members (0x44fd90): the
/// player's ship in (49, 26, 4, 3) brings 4 minotaurs (6 on normal and
/// hard) to (53, 24) and 3 amazons (4) to each of (48, 26) and (55, 30),
/// once; in (48, 12, 3, 2), 3 amazons (5) to each of (46, 14) and (52,
/// 14), where the cell is free landing ground (0x42ebf0; needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss003_puts_creatures_down_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss003").unwrap();
    let t = TILE_SIZE as f32;
    let at = |s: &Scene, cell: (i32, i32)| {
        s.walkers
            .iter()
            .filter(|w| {
                let o = &s.objects[s.object_index(w.id).unwrap()];
                ((o.x / t) as i32, (o.y / t) as i32) == cell
            })
            .count()
    };
    let cells = [(53, 24), (48, 26), (55, 30), (46, 14), (52, 14)];
    let ground: Vec<bool> = cells.iter().map(|&c| scene.landing_ground(c)).collect();
    assert_eq!(ground, [true; 5], "all five are landing ground");
    let mine = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let put = |s: &mut Scene, (x, y): (i32, i32)| {
        let i = s.object_index(mine).unwrap();
        s.objects[i].x = x as f32 * t + t / 2.0;
        s.objects[i].y = y as f32 * t + t / 2.0;
    };
    // Normal: 4 + 2 minotaurs, 3 + 1 amazons on each cell.
    put(&mut scene, (50, 27));
    for _ in 0..10 {
        scene.update(0.04, &data, &mut caches);
    }
    let first: Vec<usize> = cells.iter().map(|&c| at(&scene, c)).collect();
    assert_eq!(first, [6, 4, 4, 0, 0]);
    assert_eq!(scene.brain.as_ref().unwrap().landed, [true, false]);
    // Once only.
    put(&mut scene, (50, 27));
    for _ in 0..10 {
        scene.update(0.04, &data, &mut caches);
    }
    assert_eq!(
        scene
            .walkers
            .iter()
            .filter(|w| w.lair == walkers::NO_LAIR)
            .count(),
        14
    );
    put(&mut scene, (49, 13));
    for _ in 0..10 {
        scene.update(0.04, &data, &mut caches);
    }
    assert_eq!(at(&scene, (46, 14)), 5);
    assert_eq!(at(&scene, (52, 14)), 5);
    assert_eq!(scene.brain.as_ref().unwrap().landed, [true, true]);
}

/// A member with no ships is given its orders as any other: at its next
/// turn the order reports done, result 1, and the step goes to its p4
/// (0x4a0f60, 0x4d63d0): miss010's north-western raiders (member 4,
/// 0x468240) fighting in their wider waters (state 0x14, p3 0x28, p4 0)
/// go back to 0, not on to their strike (needs `OPENCONQUEST_DATA`).
#[test]
fn an_empty_fleet_takes_its_orders_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss010").unwrap();
    let k = 4;
    {
        let f = &mut scene.brain.as_mut().unwrap().fleets[k];
        f.ships.clear();
        f.state = 20;
        f.started = false;
        f.area = Some((0, 10, 10, 20));
    }
    let mut states = Vec::new();
    for _ in 0..100 {
        scene.run_brain();
        let f = &scene.brain.as_ref().unwrap().fleets[k];
        if states.last() != Some(&(f.state, f.started)) {
            states.push((f.state, f.started));
        }
    }
    assert_eq!(
        states[..3],
        [(20, false), (20, true), (0, false)],
        "{states:?}"
    );
}

/// The killer each death is put down to (0x50e750's callers): none (0xff)
/// counts the owner's loss and nobody's gain; each rammed ship goes to
/// the other's player (0x4c1c60); a prize taken counts as the taker's ship
/// destroyed (0x4b6f20); a harpy shot down is reported to no one
/// (0x420670); lightning wrecks a ship for no one (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn kills_are_put_down_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let fresh = |caches: &mut Caches| {
        let mut s = Scene::load(&data, caches, "miss010").unwrap();
        s.brain = None;
        s
    };
    let ships = |s: &Scene, own: bool| -> Vec<u32> {
        s.objects
            .iter()
            .filter(|o| {
                o.ship.is_some()
                    && s.is_own(o) == own
                    && o.owner.is_some_and(|w| w != crate::combat::MONSTERS)
            })
            .map(|o| o.id)
            .collect()
    };
    let sum = |n: &[u32; 12]| n.iter().sum::<u32>();
    // No killer: an enemy ship sunk counts nothing, the human's is lost.
    let mut scene = fresh(&mut caches);
    let human = scene.human.map(|h| (h.nation, h.slot));
    scene.hit(ships(&scene, false)[0], f32::MAX, None, &data, &mut caches);
    assert_eq!(sum(&scene.stats.ships_destroyed), 0);
    scene.hit(ships(&scene, true)[0], f32::MAX, None, &data, &mut caches);
    assert_eq!(sum(&scene.stats.ships_lost), 1);
    // A rammer that sinks itself on the human's ship is the human's kill.
    let mut scene = fresh(&mut caches);
    let (e, m) = (ships(&scene, false)[0], ships(&scene, true)[0]);
    let (ei, mi) = (
        scene.object_index(e).unwrap(),
        scene.object_index(m).unwrap(),
    );
    scene.objects[ei].combat.as_mut().unwrap().hull = 0.01;
    scene.objects[mi].combat.as_mut().unwrap().hull = 1.0e9;
    scene.objects[mi].combat.as_mut().unwrap().max_hull = 1.0e9;
    scene.ram(ei, mi, &data, &mut caches);
    assert!(scene.object_index(e).is_none(), "the rammer sank");
    assert_eq!(sum(&scene.stats.ships_destroyed), 1);
    assert_eq!(sum(&scene.stats.ships_lost), 0);
    // A prize: destroyed and boarded for the taker, lost for the loser.
    let mut scene = fresh(&mut caches);
    let (e, m) = (ships(&scene, false)[0], ships(&scene, true)[0]);
    let enemy = scene.objects[scene.object_index(e).unwrap()].owner;
    scene.capture(e, human, 0, Default::default(), &data, &mut caches);
    assert_eq!(sum(&scene.stats.ships_destroyed), 1);
    assert_eq!(sum(&scene.stats.ships_boarded), 1);
    scene.capture(m, enemy, 0, Default::default(), &data, &mut caches);
    assert_eq!(sum(&scene.stats.ships_lost), 1);
    // Player 16's prize is not taken: it sinks, put down to its taker
    // (0x4b7169), boarded counted all the same.
    let mut scene = fresh(&mut caches);
    let e = ships(&scene, false)[0];
    let ei = scene.object_index(e).unwrap();
    scene.objects[ei].owner = Some((4, 0));
    scene.capture(e, human, 0, Default::default(), &data, &mut caches);
    assert!(scene
        .object_index(e)
        .is_none_or(|i| scene.objects[i].owner == Some((4, 0))));
    assert!(scene.object_index(e).is_none_or(|i| scene.objects[i]
        .combat
        .as_ref()
        .is_none_or(|c| c.hull <= 0.0)));
    assert_eq!(sum(&scene.stats.ships_destroyed), 1);
    assert_eq!(sum(&scene.stats.ships_boarded), 1);
    // A harpy (or dragon) shot down by the human counts nothing.
    let mut scene = fresh(&mut caches);
    let dragon = scene
        .objects
        .iter()
        .find(|o| matches!(o.class, ObjectClass::Monster(0x1f8 | 0x1f9)) && o.combat.is_some())
        .map(|o| o.id)
        .unwrap();
    let from = {
        let k = scene.object_index(ships(&scene, true)[0]).unwrap();
        (scene.objects[k].x, scene.objects[k].y)
    };
    scene.fire_shot(
        super::shot::Firing {
            item: combat::GREEK_FIRE,
            letter: None,
            flaming: false,
            from,
            target: dragon,
            strikes: true,
            damage: f32::MAX,
            wounds: 0.0,
            wear: 0.0,
            rope: None,
            shooter: None,
            by: human,
            to: None,
        },
        &data,
        &mut caches,
    );
    for _ in 0..200 {
        scene.run_shots(0.04, &data, &mut caches);
    }
    assert!(scene.object_index(dragon).is_none(), "the dragon fell");
    assert_eq!(scene.stats.monsters_killed.iter().sum::<u32>(), 0);
    // The human's lightning: an enemy ship wrecked counts nothing.
    let mut scene = fresh(&mut caches);
    let e = ships(&scene, false)[0];
    let k = scene.object_index(e).unwrap();
    let (x, y) = (scene.objects[k].x, scene.objects[k].y);
    scene.lightning_strike(x, y, human, &data, &mut caches);
    assert!(scene.object_index(e).is_none(), "the ship was wrecked");
    assert_eq!(sum(&scene.stats.ships_destroyed), 0);
}

/// Missions start with the view where the original puts it (`WIN_VIEW`):
/// miss005's at cell (27, 28), the player's ship in sight (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn missions_start_where_the_view_is_set() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    let (x, y) = scene.start_view();
    assert_eq!((x, y), (27.0 * 64.0 + 296.0, 28.0 * 64.0 + 263.0));
    let ship = scene.objects.iter().find(|o| scene.is_own(o)).unwrap();
    assert!((ship.x - x).abs() < 300.0 && (ship.y - y).abs() < 270.0);
}

/// A fishing ground is picked anywhere in it, not only on the fish drawn
/// in its middle (needs `OPENCONQUEST_DATA`).
#[test]
fn deposits_are_picked_by_area() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.fog = false;
    let t = TILE_SIZE as f32;
    let zone = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::FishZone && o.record.field(8).unwrap_or(0) >= 2)
        .unwrap();
    let (x, y) = (
        zone.record.field(6).unwrap() as f32,
        zone.record.field(7).unwrap() as f32,
    );
    // Its top-left cell, away from the fish in the middle.
    let picked = crate::render::hit_test(&scene, x * t + t / 2.0, y * t + t / 2.0, 0);
    assert_eq!(picked, Some(zone.id));
}

/// Every mouse pointer, normal and greyed, loads from the game data (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn pointers_load() {
    use crate::cursor::{Pointer as P, Pointers};
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let pointers = Pointers::default();
    assert!(pointers.available(&data));
    for p in [
        P::Standard,
        P::Fish,
        P::Whale,
        P::Amber,
        P::Own,
        P::Select,
        P::Sea,
        P::Attack,
        P::Board,
        P::Ram,
        P::Build,
        P::Repair,
        P::Magic,
        P::Treasure,
        P::Behaviour,
        P::Ban,
    ] {
        for greyed in [false, true] {
            assert!(pointers.count(&data, p, greyed) > 0, "{p:?} {greyed}");
        }
    }
}

/// A boat fishing casts its net (needs `OPENCONQUEST_DATA`).
#[test]
fn fishing_casts_the_net() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let (boat, bx, by) = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .map(|o| (o.id, o.x, o.y))
        .unwrap();
    let ground = scene
        .objects
        .iter()
        .filter(|o| o.deposit.is_some_and(|d| d.resource == Resource::Fish))
        .min_by(|a, b| {
            let d = |o: &&MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
            d(a).total_cmp(&d(b))
        })
        .unwrap()
        .id;
    scene.order_gather(boat, ground).unwrap();
    let net = (0..60 * 120).any(|_| {
        scene.update(1.0 / 60.0, &data, &mut caches);
        scene
            .planks
            .iter()
            .any(|p| p.activity == super::unload::Activity::Fishing && !p.frame().is_empty())
    });
    assert!(net);
}

/// Exhausted amber lies empty for a while and grows back to what it held
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn amber_grows_back() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss004").unwrap();
    scene.brain = None;
    let (boat, bx, by) = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .map(|o| (o.id, o.x, o.y))
        .unwrap();
    let amber = scene
        .objects
        .iter()
        .filter(|o| {
            o.deposit
                .is_some_and(|d| d.resource == Resource::Amber && d.regrows > 0)
        })
        .min_by(|a, b| {
            let d = |o: &&MapObject| (o.x - bx).powi(2) + (o.y - by).powi(2);
            d(a).total_cmp(&d(b))
        })
        .unwrap()
        .id;
    let a = scene.object_index(amber).unwrap();
    let full = scene.objects[a].deposit.unwrap().refill;
    let regrows = scene.objects[a].deposit.unwrap().regrows;
    scene.objects[a].deposit.as_mut().unwrap().remaining = Some(5.0);
    scene.order_gather(boat, amber).unwrap();
    let emptied = (0..60 * 120).any(|_| {
        scene.update(1.0 / 60.0, &data, &mut caches);
        let a = scene.object_index(amber).unwrap();
        scene.objects[a].dormant.is_some()
    });
    assert!(emptied, "the amber runs out");
    let a = scene.object_index(amber).unwrap();
    assert!(scene.objects[a].deposit.is_none() && scene.objects[a].frames.is_empty());
    for _ in 0..(60.0 * (crate::objects::REGROW_SECONDS + 0.5)) as usize {
        scene.update(1.0 / 60.0, &data, &mut caches);
    }
    let a = scene.object_index(amber).unwrap();
    let dep = scene.objects[a].deposit.expect("grown back");
    assert_eq!(dep.remaining, Some(full));
    assert_eq!(dep.regrows, regrows - 1);
    assert!(!scene.objects[a].frames.is_empty());
}

/// Missions give some players research levels from the start: in miss008
/// the computer players have armour, ram and the workshop's levels at 2,
/// and the human, the mission started on its own, its standalone levels
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn missions_give_starting_levels() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let scene = Scene::load(&data, &mut caches, "miss008").unwrap();
    let human = scene.human.map(|h| (h.nation, h.slot));
    // Started on its own, the human gets the mission's standalone levels.
    let mine = scene.levels_of(human);
    assert_eq!((mine.weapons, mine.ram, mine.armour), (1, 2, 1));
    assert_eq!(
        (mine.rate, mine.reach, mine.arms),
        ([2, 2, 0], [2, 2, 0], [2; 3])
    );
    let strong: Vec<_> = scene
        .levels
        .iter()
        .filter(|(owner, _)| Some(**owner) != human)
        .collect();
    assert_eq!(strong.len(), 2);
    for (_, l) in strong {
        assert_eq!((l.armour, l.ram, l.weapons), (2, 2, 1));
        assert_eq!((l.arms, l.rate, l.reach), ([2; 3], [2; 3], [2; 3]));
    }
}

/// Finds the menu font, colour variant and spacing that draw `OPENCONQUEST_FIT_TEXT`
/// at `OPENCONQUEST_FIT_AT` (x,y,w,h) as in the 800x600 raw RGB screenshot
/// `OPENCONQUEST_FIT_RGB` (a development aid; does nothing without them).
#[test]
fn fit_font() {
    let (Some(root), Ok(raw), Ok(bg), Ok(text), Ok(at)) = (
        std::env::var_os("OPENCONQUEST_DATA"),
        std::env::var("OPENCONQUEST_FIT_RGB"),
        std::env::var("OPENCONQUEST_FIT_BG"),
        std::env::var("OPENCONQUEST_FIT_TEXT"),
        std::env::var("OPENCONQUEST_FIT_AT"),
    ) else {
        return;
    };
    let bg: Vec<u32> = std::fs::read(bg)
        .unwrap()
        .chunks(3)
        .map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
        .collect();
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let arc = data.menus.as_ref().unwrap();
    let bytes = std::fs::read(raw).unwrap();
    let shot: Vec<u32> = bytes
        .chunks(3)
        .map(|c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
        .collect();
    let r: Vec<i32> = at.split(',').map(|v| v.parse().unwrap()).collect();
    let (x0, y0, w, h) = (r[0], r[1], r[2], r[3]);
    let palette = openconquest_formats::Bitmap::parse(
        &arc.read("0M_BKGND_00", openconquest_formats::ResourceKind::Bitmap)
            .unwrap(),
    )
    .unwrap()
    .palette;
    let mut results = Vec::new();
    for name in [
        "S_FNT_LH", "S_FNT_BH", "S_FNT_BS", "S_FNT_MH", "S_FNT_MS", "S_FNT_SH", "S_FNT_SS",
    ] {
        for variant in 0..4 {
            if crate::text::TextRenderer::load_in(arc, &format!("0{name}"), variant, Some(&palette))
                .is_err()
            {
                continue;
            }
            for spacing in 0..4 {
                let f = f_clone(arc, name, variant, &palette).with_spacing(spacing);
                let reach: i32 = std::env::var("OPENCONQUEST_FIT_REACH")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(3);
                for dy in -reach..=reach {
                    for dx in -reach..=reach {
                        let mut px = bg.clone();
                        let mut frame = crate::render::Frame {
                            pixels: &mut px,
                            width: 800,
                            height: 600,
                        };
                        f.draw(&mut frame, x0 + dx, y0 + dy, 1, &text);
                        // Only where the candidate draws (the background may
                        // hold more than the backdrop): mismatches per
                        // thousand drawn pixels.
                        let (mut n, mut drawn) = (0, 0);
                        for y in y0..y0 + h {
                            for x in x0..x0 + w {
                                let k = (y * 800 + x) as usize;
                                if px[k] == bg[k] {
                                    continue;
                                }
                                drawn += 1;
                                let (a, b) = (px[k], shot[k]);
                                let d =
                                    |s: u32| ((a >> s & 255) as i32 - (b >> s & 255) as i32).abs();
                                if d(16) + d(8) + d(0) > 24 {
                                    n += 1;
                                }
                            }
                        }
                        if drawn > 40 {
                            results.push((n * 1000 / drawn, name, variant, spacing, dx, dy));
                        }
                    }
                }
            }
        }
    }
    results.sort();
    for r in results.iter().take(6) {
        eprintln!("== fit {r:?}");
    }
}

fn f_clone(
    arc: &openconquest_formats::Archive,
    name: &str,
    variant: usize,
    palette: &openconquest_formats::Palette,
) -> crate::text::TextRenderer {
    crate::text::TextRenderer::load_in(arc, &format!("0{name}"), variant, Some(palette)).unwrap()
}

/// Finds where the menu sprites `OPENCONQUEST_FIT_SPRITES` (comma separated) sit
/// in the 800x600 raw RGB screenshot `OPENCONQUEST_FIT_RGB`, searching the area
/// `OPENCONQUEST_FIT_AREA` (x0,y0,x1,y1 for the sprites' top-left corner); a
/// development aid that does nothing without them.
#[test]
fn fit_sprite() {
    let (Some(root), Ok(raw), Ok(names), Ok(area)) = (
        std::env::var_os("OPENCONQUEST_DATA"),
        std::env::var("OPENCONQUEST_FIT_RGB"),
        std::env::var("OPENCONQUEST_FIT_SPRITES"),
        std::env::var("OPENCONQUEST_FIT_AREA"),
    ) else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let arc = data.menus.as_ref().unwrap();
    let shot: Vec<[u8; 3]> = std::fs::read(raw)
        .unwrap()
        .chunks(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let palette = openconquest_formats::Bitmap::parse(
        &arc.read("0M_BKGND_00", openconquest_formats::ResourceKind::Bitmap)
            .unwrap(),
    )
    .unwrap()
    .palette;
    let a: Vec<i32> = area.split(',').map(|v| v.parse().unwrap()).collect();
    for name in names.split(',') {
        let e = arc
            .find(
                &format!("0{name}"),
                openconquest_formats::ResourceKind::Sprite,
            )
            .or_else(|| arc.find(name, openconquest_formats::ResourceKind::Sprite))
            .unwrap();
        let s = openconquest_formats::Sprite::parse(&arc.read_entry(e).unwrap()).unwrap();
        let mut ink = Vec::new();
        for y in 0..s.box_height {
            for x in 0..s.box_width {
                let i = (y * s.box_width + x) as usize;
                if s.opaque[i] != 0 {
                    let c = palette.0[s.pixels[i] as usize];
                    ink.push(((s.x + x) as i32, (s.y + y) as i32, [c.r, c.g, c.b]));
                }
            }
        }
        let mut best = (usize::MAX, 0, 0);
        for oy in a[1]..=a[3] {
            for ox in a[0]..=a[2] {
                let mut bad = 0;
                for &(x, y, c) in &ink {
                    let (px, py) = (ox + x, oy + y);
                    if !(0..800).contains(&px) || !(0..600).contains(&py) {
                        bad += 1;
                        continue;
                    }
                    let p = shot[(py * 800 + px) as usize];
                    if (0..3)
                        .map(|k| (p[k] as i32 - c[k] as i32).abs())
                        .sum::<i32>()
                        > 24
                    {
                        bad += 1;
                    }
                    if bad >= best.0 {
                        break;
                    }
                }
                if bad < best.0 {
                    best = (bad, ox, oy);
                }
            }
        }
        eprintln!(
            "== {name} {}x{} at ({}, {}): {} of {} differ",
            s.width,
            s.height,
            best.1,
            best.2,
            best.0,
            ink.len()
        );
    }
}

#[test]
fn start_fog_matches_the_original() {
    // The explored layer (DarkTy layer 0, edge codes) of the original's
    // human player at the start of miss000, read from its memory; rows 10
    // and below are all 0.
    const ORIGINAL: [&str; 10] = [
        "0000000000007fffffffffff",
        "0000000000007fffffffffff",
        "0000000000007fffffffffff",
        "0000000000007fffffffffff",
        "000000000000c4ffffffffff",
        "0000000000000c4fffffffff",
        "00000000000000c84fffffff",
        "0000000000000000c4ffffff",
        "00000000000000000c84ffff",
        "0000000000000000000c8888",
    ];
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    for y in 0..24 {
        let ours: String = (0..24)
            .map(|x| format!("{:x}", scene.fog_codes(x, y).0))
            .collect();
        let expected = ORIGINAL
            .get(y as usize)
            .copied()
            .unwrap_or("000000000000000000000000");
        assert_eq!(ours, expected, "row {y}");
    }
}

#[test]
fn warships_open_the_building_page() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let killer = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 7))
        .unwrap();
    let main = crate::commands::menu(&scene, killer, crate::commands::Page::Main);
    let build = main
        .iter()
        .flatten()
        .find(|s| s.command == crate::commands::Command::BUILD_GROUND)
        .unwrap();
    assert!(build.enabled);
    let ground = crate::commands::menu(&scene, killer, crate::commands::Page::Ground);
    assert_eq!(ground[0].unwrap().command, crate::commands::Command(0x16));
}

/// A settlement's stocked marine goes aboard a ship alongside, one at a time
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn stock_loads_onto_a_ship_alongside() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.class == ObjectClass::Building(SETTLEMENT))
        .unwrap();
    let (hx, hy, home) = (home.x, home.y, home.id);
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 7))
        .unwrap()
        .id;
    assert!(scene.load_item(home, ship, 1).is_err(), "too far away");
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].x = hx + 64.0;
    scene.objects[i].y = hy;
    // Its two places are taken (a bowman and a scorpion): make room.
    let full = scene.load_item(home, ship, 1);
    assert_eq!(full, Err("No room aboard".to_string()));
    scene.objects[i].ship.as_mut().unwrap().crew.squads = [0; 3];
    let before = scene.objects[i].ship.as_ref().unwrap().crew.squads[1];
    scene.load_item(home, ship, 1).unwrap();
    let i = scene.object_index(ship).unwrap();
    assert_eq!(
        scene.objects[i].ship.as_ref().unwrap().crew.squads[1],
        before + 1
    );
    assert_eq!(scene.garrison(home)[1], 0);
    assert!(scene.load_item(home, ship, 1).is_err(), "none left");
}

/// Ships turn on the spot before they set off, and sail cell to cell along
/// the eight directions only (needs `OPENCONQUEST_DATA`).
#[test]
fn ships_turn_then_sail_the_grid() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
    let i = scene.object_index(ship).unwrap();
    let (x0, y0) = (scene.objects[i].x, scene.objects[i].y);
    let heading = scene.objects[i].ship.as_ref().unwrap().heading;
    // Somewhere behind it and off to one side, so it must turn.
    let t = TILE_SIZE as f32;
    let back = [
        (1.0, 0.0),
        (1.0, -1.0),
        (0.0, -1.0),
        (-1.0, -1.0),
        (-1.0, 0.0),
        (-1.0, 1.0),
        (0.0, 1.0),
        (1.0, 1.0),
    ][(heading + 3) % 8];
    let (tx, ty) = (x0 + back.0 * 3.0 * t, y0 + back.1 * 3.0 * t);
    assert!(scene.order_move(ship, tx, ty), "no route");
    let mut turned = false;
    let mut last = (x0, y0);
    for _ in 0..400 {
        scene.update(0.02, &data, &mut caches);
        let i = scene.object_index(ship).unwrap();
        let o = &scene.objects[i];
        let s = o.ship.as_ref().unwrap();
        if s.rotation.is_some() {
            turned = true;
            assert_eq!((o.x, o.y), last, "moved while turning");
        }
        // On a straight or diagonal line through its nearest cell centre.
        let c = |v: f32| (v / t).floor() * t + t / 2.0;
        let (dx, dy) = ((o.x - c(o.x)).abs(), (o.y - c(o.y)).abs());
        assert!(
            dx < 0.05 || dy < 0.05 || (dx - dy).abs() < 0.05,
            "off the grid at ({}, {})",
            o.x,
            o.y
        );
        last = (o.x, o.y);
    }
    assert!(turned, "never turned");
    assert!(last != (x0, y0), "never moved");
}

/// Shoals keep moving while fished, and the fishing ship follows its shoal
/// and goes on fishing (needs `OPENCONQUEST_DATA`).
#[test]
fn fishing_ships_follow_their_shoal() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let ship = scene
        .objects
        .iter()
        .find(|o| {
            scene.is_own(o)
                && o.ship
                    .as_ref()
                    .is_some_and(|s| crate::objects::cargo_capacity(s.kind) > 0.0)
        })
        .unwrap()
        .id;
    let shoal = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::FishZone)
        .unwrap()
        .id;
    scene.order_gather(ship, shoal).unwrap();
    let mut spots = std::collections::HashSet::new();
    let mut caught = 0.0f32;
    let (mut nets, mut off) = (0, false);
    for _ in 0..50 * 90 {
        scene.update(0.02, &data, &mut caches);
        let o = &scene.objects[scene.object_index(shoal).unwrap()];
        spots.insert((o.x as i32, o.y as i32));
        let s = scene.objects[scene.object_index(ship).unwrap()]
            .ship
            .as_ref()
            .unwrap();
        caught = caught.max(s.cargo);
        // The net lies off the ship by its table, not on its middle.
        let o = &scene.objects[scene.object_index(ship).unwrap()];
        for p in scene.planks.iter().filter(|p| p.ship == ship) {
            for (_, x, y) in p.frame() {
                nets += 1;
                off |= (x - o.x).hypot(y - o.y) > 4.0;
            }
        }
    }
    assert!(nets > 0, "no net was cast");
    assert!(off, "the net lies off the ship's middle");
    assert!(spots.len() > 1, "the shoal never moved");
    assert!(caught > 0.0, "nothing caught");
}

/// Selling an item from a settlement's stock brings back three quarters of
/// its amber price (needs `OPENCONQUEST_DATA`).
#[test]
fn settlement_sells_stock() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let site = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.class == ObjectClass::Building(SETTLEMENT))
        .unwrap()
        .id;
    let mut stock = [0u32; 9];
    stock[4] = 2;
    scene.garrisons.insert(site, stock);
    let amber = scene.amber;
    scene.sell_item(site, 4).unwrap();
    assert_eq!(scene.amber, amber + 37);
    assert_eq!(scene.garrison(site)[4], 1);
    scene.sell_item(site, 4).unwrap();
    assert!(scene.sell_item(site, 4).is_err());
    // Selling the settlement: a bowman's squad (30 x 100 x 75 / 10000 = 22
    // amber, 150 fish) and three quarters of the building; it stands sold
    // (state 3), still the player's, with no menu.
    let mut stock = [0u32; 9];
    stock[0] = 1;
    scene.garrisons.insert(site, stock);
    let (amber, fish) = (scene.amber, scene.fish);
    scene.sell_town(site, &data, &mut caches).unwrap();
    assert_eq!(
        scene.amber,
        amber + 22 + economy::building_cost(SETTLEMENT) * 3 / 4
    );
    assert_eq!(scene.fish, fish + 150);
    let i = scene.object_index(site).expect("not destroyed");
    assert!(scene.is_own(&scene.objects[i]));
    assert!(scene.sold.contains(&site));
    let menu = crate::commands::main_menu(&scene, &scene.objects[i]);
    assert!(menu.iter().all(|b| b.is_none()));
    assert!(scene.sell_town(site, &data, &mut caches).is_err(), "once");
}

/// A ram ordered from afar goes in and strikes from the neighbouring cell
/// (one ship to a cell; needs `OPENCONQUEST_DATA`).
#[test]
fn ram_from_afar_connects() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let human = scene.human.unwrap();
    let mine = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let other = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && o.id != mine)
        .unwrap()
        .id;
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == mine || o.id == other);
    scene
        .objects
        .retain(|o| o.ship.is_some() || o.combat.as_ref().is_none_or(|c| c.weapons.is_empty()));
    let t = TILE_SIZE as f32;
    // A stretch of open water, five cells along and three deep.
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let (cx, cy) = (1..h - 1)
        .flat_map(|y| (1..w - 5).map(move |x| (x, y)))
        .find(|&(x, y)| (-1..=1).all(|dy| (-1..=4).all(|dx| scene.nav.is_water(x + dx, y + dy))))
        .expect("open water");
    let target_cell = (cx + 3, cy);
    let m = scene.object_index(mine).unwrap();
    scene.objects[m].x = cx as f32 * t + t / 2.0;
    scene.objects[m].y = cy as f32 * t + t / 2.0;
    scene.objects[m].ship.as_mut().unwrap().path.clear();
    let e = scene.object_index(other).unwrap();
    let enemy = &mut scene.objects[e];
    enemy.owner = Some((1, (human.slot + 1) % 4));
    enemy.x = target_cell.0 as f32 * t + t / 2.0;
    enemy.y = target_cell.1 as f32 * t + t / 2.0;
    enemy.ship.as_mut().unwrap().path.clear();
    enemy.combat.as_mut().unwrap().weapons.clear();
    let target = other;
    let i = scene.object_index(mine).unwrap();
    scene.objects[i].combat.as_mut().unwrap().weapons.clear();
    scene.order_close(mine, target, false).unwrap();
    let hull = |s: &Scene| {
        s.object_index(target)
            .map_or(0.0, |k| s.objects[k].combat.as_ref().unwrap().hull)
    };
    let before = hull(&scene);
    for _ in 0..50 * 30 {
        scene.update(0.02, &data, &mut caches);
        if hull(&scene) < before {
            break;
        }
    }
    assert!(hull(&scene) < before, "the ram struck");
}

/// Two ships in neighbouring cells of open water, the first the human
/// player's boarder (Hercules aboard, so it wins) and the second an
/// unarmed enemy merchant, both heading east; their ids and the first's
/// cell.
fn boarding_pair(scene: &mut Scene, dx: i32, dy: i32) -> (u32, u32, (i32, i32)) {
    let human = scene.human.unwrap();
    let mine = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let other = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && o.id != mine)
        .unwrap()
        .id;
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == mine || o.id == other);
    scene
        .objects
        .retain(|o| o.ship.is_some() || o.combat.as_ref().is_none_or(|c| c.weapons.is_empty()));
    let t = TILE_SIZE as f32;
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let cell = (2..h - 2)
        .flat_map(|y| (2..w - 2).map(move |x| (x, y)))
        .find(|&(x, y)| (-2..=2).all(|j| (-2..=2).all(|i| scene.nav.is_water(x + i, y + j))))
        .expect("open water");
    for (id, (cx, cy), kind, crew) in [
        (mine, cell, 4u8, 6u32),
        (other, (cell.0 + dx, cell.1 + dy), 1, 1),
    ] {
        let k = scene.object_index(id).unwrap();
        let o = &mut scene.objects[k];
        o.x = cx as f32 * t + t / 2.0;
        o.y = cy as f32 * t + t / 2.0;
        o.combat.as_mut().unwrap().weapons.clear();
        let s = o.ship.as_mut().unwrap();
        s.path.clear();
        s.task = None;
        s.heading = 0;
        s.kind = kind;
        s.crew = combat::Crew {
            class: 2,
            squads: [0, 0, crew],
        };
        if id == mine {
            s.heroes = vec![objects::HERCULES];
        } else {
            o.owner = Some((1, (human.slot + 1) % 4));
            let c = o.combat.as_mut().unwrap();
            (c.max_hull, c.hull) = (1e6, 1e6);
        }
    }
    (mine, other, cell)
}

/// Boarders throw a grappling hook with its line, pull both ships
/// alongside and parallel, furl, lower the walkway, fight, raise it and
/// both go back to their cells, at the original's pace (0x4b84b0,
/// 0x4b54a0, 0x4b5df0, 0x4c3410; needs `OPENCONQUEST_DATA`).
#[test]
fn boarders_hook_and_lie_alongside() {
    use crate::objects::GrappleStage;
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let (mine, other, cell) = boarding_pair(&mut scene, 1, 0);
    scene.order_close(mine, other, true).unwrap();
    let t = TILE_SIZE as f32;
    let grapple = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .grapple
    };
    let mut hook = None;
    let mut roped = false;
    let mut stages: Vec<(GrappleStage, u32)> = Vec::new();
    let mut lying = None;
    let mut walkway = false;
    let mut end = None;
    for tick in 0..3000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        if hook.is_none() && scene.shots.iter().any(|s| s.rope == Some(mine)) {
            hook = Some(tick);
            roped = !scene.ropes().is_empty();
        }
        walkway |= !scene.planks_shown().is_empty();
        match grapple(&scene, mine) {
            Some(g) => {
                if stages.last().is_none_or(|&(s, _)| s != g.stage) {
                    stages.push((g.stage, tick));
                }
                // A save keeps the boarding under way.
                if g.stage == GrappleStage::Fight && g.ticks == 1 {
                    let path = std::env::temp_dir()
                        .join(format!("openconquest-board-{}.json", std::process::id()));
                    scene.save().write(&path, false).unwrap();
                    let save = SaveGame::read(&path).unwrap();
                    let _ = std::fs::remove_file(&path);
                    let restored = Scene::restore(&data, &mut caches, &save).unwrap();
                    assert_eq!(grapple(&restored, mine), Some(g));
                    assert!(grapple(&restored, other).is_some());
                }
                if g.stage == GrappleStage::Plank && lying.is_none() {
                    let pos = |id| {
                        let o = &scene.objects[scene.object_index(id).unwrap()];
                        (o.x, o.y, o.ship.as_ref().unwrap().heading)
                    };
                    lying = Some((pos(mine), pos(other), grapple(&scene, other)));
                }
            }
            None if !stages.is_empty() => {
                end = Some(tick);
                break;
            }
            None => {}
        }
    }
    assert!(hook.is_some() && roped, "a hook flew on its line");
    let order: Vec<GrappleStage> = stages.iter().map(|s| s.0).collect();
    use GrappleStage::*;
    assert_eq!(order, [Pull, Furl, Plank, Fight, Retract, Leave]);
    let at = |s: GrappleStage| stages.iter().find(|x| x.0 == s).unwrap().1;
    // Four frames of furling, five ticks each; the walkway's 18 frames
    // down and up, a tick each; the fight as long as its picture.
    assert_eq!(at(Plank) - at(Furl), 20);
    assert_eq!(at(Fight) - at(Plank), 18);
    assert!((40..=67).contains(&(at(Retract) - at(Fight))));
    assert_eq!(at(Leave) - at(Retract), 18);
    assert!(walkway, "the walkway was shown");
    // Alongside: each slid 10 pixels towards the other and lies parallel,
    // north or south; the boarded ship has dropped the line by then.
    let ((ax, ay, ah), (bx, by, bh), bg) = lying.unwrap();
    let mid = (cell.0 as f32 * t + t / 2.0, cell.1 as f32 * t + t / 2.0);
    assert_eq!((ax, ay), (mid.0 + 10.0, mid.1));
    assert_eq!((bx, by), (mid.0 + t - 10.0, mid.1));
    assert!(matches!(ah, 2 | 6) && matches!(bh, 2 | 6), "{ah} {bh}");
    let bg = bg.unwrap();
    assert!(!bg.rope && bg.stage == Lie);
    // Won (Hercules): the prize is ours; both ships end back in their cells
    // with their sails set.
    assert!(end.is_some());
    for _ in 0..100 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let prize = &scene.objects[scene.object_index(other).unwrap()];
    assert!(scene.is_own(prize), "the ship was captured");
    for (id, c) in [(mine, cell), (other, (cell.0 + 1, cell.1))] {
        let o = &scene.objects[scene.object_index(id).unwrap()];
        let s = o.ship.as_ref().unwrap();
        assert!(s.grapple.is_none() && s.furl == 0.0);
        assert_eq!(
            (o.x, o.y),
            (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0)
        );
    }
}

/// The magic wood aboard keeps boarders off: their hooks fly wide, one
/// every reload of the strong weapons (85 ticks at level 0; 0x4b84b0,
/// 0x4ba190; needs `OPENCONQUEST_DATA`).
#[test]
fn magic_wood_keeps_hooks_off() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let (mine, other, _) = boarding_pair(&mut scene, 1, 1);
    let k = scene.object_index(other).unwrap();
    scene.objects[k]
        .ship
        .as_mut()
        .unwrap()
        .treasures
        .push(objects::Treasure {
            kind: objects::MAGIC_WOOD,
            param: 0,
            amount: 0,
        });
    scene.order_close(mine, other, true).unwrap();
    let mut throws = Vec::new();
    let mut flying = false;
    for tick in 0..400 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        let now = scene.shots.iter().any(|s| s.rope == Some(mine));
        if now && !flying {
            throws.push(tick);
        }
        flying = now;
        let s = scene.objects[scene.object_index(mine).unwrap()]
            .ship
            .as_ref()
            .unwrap();
        assert!(s.grapple.is_none(), "never grappled");
    }
    assert!(throws.len() >= 3, "{throws:?}");
    assert!(throws.windows(2).all(|w| w[1] - w[0] == 85), "{throws:?}");
}

/// The pull's slide and turn as the original steps them (0x4b5220,
/// 0x4b57e0, 0x4b54a0).
#[test]
fn grapple_slide_and_parallel() {
    use super::grapple::{parallel, slide};
    // Ten pixels: two points a tick, over at the sixth (12 points, padded).
    assert_eq!(slide((0.0, 0.0), (10.0, 0.0), 5), ((10.0, 0.0), false));
    assert_eq!(slide((0.0, 0.0), (10.0, 0.0), 6), ((10.0, 0.0), true));
    assert!(slide((0.0, 0.0), (0.0, -13.0), 7).1);
    assert!(!slide((0.0, 0.0), (19.0, 19.0), 9).1);
    assert!(slide((0.0, 0.0), (19.0, 19.0), 10).1);
    // The other ship east: north or south, the nearer; a tie goes south.
    assert_eq!(parallel(0, 0), 6);
    assert_eq!(parallel(0, 3), 2);
    assert_eq!(parallel(4, 21), 6);
    // North-east: north-west or south-east.
    assert_eq!(parallel(1, 0), 7);
    assert_eq!(parallel(1, 6), 3);
    // North: east or west.
    assert_eq!(parallel(2, 9), 4);
    assert_eq!(parallel(2, 3), 0);
}

/// A rammer sails into its prey until the bow touches the hull, shows the
/// splinters there a frame a tick, recoils, strikes as they end and backs
/// off to the middle of its cell (0x4c2230; needs `OPENCONQUEST_DATA`).
#[test]
fn rammer_charges_strikes_and_backs_off() {
    use crate::objects::RamStage;
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let (mine, other, cell) = boarding_pair(&mut scene, 1, 0);
    let hull = |s: &Scene| {
        s.objects[s.object_index(other).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull
    };
    let before = hull(&scene);
    scene.order_close(mine, other, false).unwrap();
    let run = |s: &Scene| {
        s.objects[s.object_index(mine).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .ram_run
    };
    let mut stages = Vec::new();
    let mut impact_x = None;
    for tick in 0..1000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        let r = run(&scene);
        let stage = r.map(|r| r.stage);
        if stages.last().map(|s: &(Option<RamStage>, u32)| s.0) != Some(stage) {
            stages.push((stage, tick));
        }
        let o = &scene.objects[scene.object_index(mine).unwrap()];
        if stage == Some(RamStage::Impact) && impact_x.is_none() {
            impact_x = Some(o.x);
            // The bow lies on the target's hull, the rammer short of
            // the target's middle.
            assert!(o.x < cell.0 as f32 * 64.0 + 96.0);
        }
        if stages.len() > 1 && stage.is_none() {
            break;
        }
    }
    let order: Vec<_> = stages.iter().map(|s| s.0).collect();
    assert_eq!(
        order,
        [
            Some(RamStage::Charge),
            Some(RamStage::Impact),
            Some(RamStage::Back),
            None
        ]
    );
    assert!(hull(&scene) < before, "the ram struck");
    let impact = stages[2].1 - stages[1].1;
    let splinters = objects::sequence_frames(&data, &mut caches, "BOMBH30")
        .unwrap()
        .len() as u32;
    assert_eq!(impact, splinters);
    let o = &scene.objects[scene.object_index(mine).unwrap()];
    assert_eq!(
        (o.x, o.y),
        (cell.0 as f32 * 64.0 + 32.0, cell.1 as f32 * 64.0 + 32.0)
    );
    assert!(o.ship.as_ref().unwrap().task.is_none());
}

/// A game saved mid-battle keeps the shots in flight and the spells
/// under way (needs `OPENCONQUEST_DATA`).
#[test]
fn battles_survive_a_save() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let human = scene.human.unwrap();
    let mine = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let (x, y) = (scene.objects[mine].x, scene.objects[mine].y);
    scene.objects[mine].ship.as_mut().unwrap().kind = 2;
    scene.objects[mine].combat.as_mut().unwrap().weapons = combat::DEFAULT_WEAPONS[2].to_vec();
    let mine = scene.objects[mine].id;
    let e = scene
        .objects
        .iter()
        .position(|o| o.ship.is_some() && o.id != mine)
        .unwrap();
    scene.objects[e].owner = Some((human.nation, human.slot + 1));
    scene.objects[e].x = x + 192.0;
    scene.objects[e].y = y;
    let target = scene.objects[e].id;
    scene.order_attack(mine, target).unwrap();
    for _ in 0..50 * 10 {
        scene.update(0.02, &data, &mut caches);
        if !scene.shots.is_empty() {
            break;
        }
    }
    assert!(!scene.shots.is_empty(), "a volley flies");
    scene.lightning.push((x, y, 1.0, None));
    let path =
        std::env::temp_dir().join(format!("openconquest-battle-{}.json", std::process::id()));
    scene.save().write(&path, false).unwrap();
    let save = SaveGame::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let restored = Scene::restore(&data, &mut caches, &save).unwrap();
    assert_eq!(restored.shots.len(), scene.shots.len());
    assert!(
        restored.shots.iter().all(|s| s.layer().is_some()),
        "pictures found again"
    );
    assert_eq!(restored.lightning.len(), 1);
}

/// The Golden Fleece mends 1% of the hull every 100 ticks, Argus every
/// 50 (needs `OPENCONQUEST_DATA`).
#[test]
fn fleece_and_argus_mend() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    // By id: the objects are sorted again as creatures come out.
    let id = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let i = scene.object_index(id).unwrap();
    let max = scene.objects[i].combat.as_ref().unwrap().max_hull;
    scene.objects[i].combat.as_mut().unwrap().hull = max / 2.0;
    scene.objects[i]
        .ship
        .as_mut()
        .unwrap()
        .treasures
        .push(objects::Treasure {
            kind: 16,
            param: 0,
            amount: 10,
        });
    let seconds = 100.0 * crate::clock::TICK;
    let mut t = 0.0;
    while t < seconds {
        scene.update(0.02, &data, &mut caches);
        t += 0.02;
    }
    let i = scene.object_index(id).unwrap();
    let hull = scene.objects[i].combat.as_ref().unwrap().hull;
    assert!(
        ((hull - max / 2.0) / max - 0.01).abs() < 0.002,
        "{}",
        (hull - max / 2.0) / max
    );
    scene.objects[i].ship.as_mut().unwrap().heroes.push(5);
    let before = hull;
    let mut t = 0.0;
    while t < seconds {
        scene.update(0.02, &data, &mut caches);
        t += 0.02;
    }
    let i = scene.object_index(id).unwrap();
    let hull = scene.objects[i].combat.as_ref().unwrap().hull;
    assert!(
        ((hull - before) / max - 0.03).abs() < 0.003,
        "{}",
        (hull - before) / max
    );
}

/// A settlement mends a building's hull over 200 ticks for its price:
/// half of a settlement's in 100 ticks for 200 amber (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn mending_takes_the_original_time_and_price() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.class == ObjectClass::Building(SETTLEMENT))
        .unwrap()
        .id;
    let h = scene.object_index(home).unwrap();
    let c = scene.objects[h].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    let max = c.max_hull;
    scene.order_mend(home, home).unwrap();
    let amber = scene.amber;
    let mut t = 0.0;
    while t < 95.0 * crate::clock::TICK {
        scene.update(0.02, &data, &mut caches);
        t += 0.02;
    }
    let c = scene.objects[h].combat.as_ref().unwrap();
    assert!(c.hull < max, "not whole before 100 ticks");
    while t < 105.0 * crate::clock::TICK {
        scene.update(0.02, &data, &mut caches);
        t += 0.02;
    }
    let c = scene.objects[h].combat.as_ref().unwrap();
    assert_eq!(c.hull, max);
    let paid = amber as i64 - scene.amber as i64;
    assert!((199..=201).contains(&paid), "paid {paid}");
}

/// A harpy wanders its nest's range, strikes a crewed ship beside it,
/// then flies home and rests (needs `OPENCONQUEST_DATA`).
#[test]
fn harpy_strikes_and_goes_home() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    scene.brain = None;
    let t = TILE_SIZE as f32;
    let harpy = scene.flyers[0].clone();
    let nest = &scene.objects[scene.object_index(harpy.nest).unwrap()];
    let (nx, ny) = (nest.x, nest.y);
    // A crewed ship two cells off, the only one in reach.
    let ship = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    scene.objects[ship].x = (nx / t).floor() * t + t / 2.0 + 2.0 * t;
    scene.objects[ship].y = ((ny - t / 2.0) / t).floor() * t + t / 2.0;
    scene.objects[ship].ship.as_mut().unwrap().crew.squads = [3, 0, 0];
    scene.objects[ship].combat.as_mut().unwrap().max_hull = 1e9;
    scene.objects[ship].combat.as_mut().unwrap().hull = 1e9;
    let id = scene.objects[ship].id;
    let mut struck = false;
    let mut home_again = false;
    for _ in 0..50 * 240 {
        scene.update(0.02, &data, &mut caches);
        let i = scene.object_index(id).unwrap();
        // Hold the ship where it is.
        scene.objects[i].ship.as_mut().unwrap().path.clear();
        let f = scene.flyers.iter().find(|f| f.id == harpy.id).unwrap();
        if matches!(f.flight, flyers::Flight::Striking { .. }) {
            struck = true;
        }
        if struck && matches!(f.flight, flyers::Flight::Rest(_)) {
            home_again = true;
            break;
        }
    }
    assert!(struck, "the harpy struck");
    assert!(home_again, "and went home to rest");
}

/// A wrecked hull sails at half speed, and armour slows a ship (0x4bece0).
#[test]
fn damage_and_armour_slow_ships() {
    assert_eq!(combat::sailing_factors(100.0, 100.0, 0), (1.0, 1.0));
    let (speed, accel) = combat::sailing_factors(0.0, 100.0, 0);
    assert!((speed - 0.5).abs() < 1e-6 && (accel - 0.5).abs() < 1e-6);
    let (speed, accel) = combat::sailing_factors(100.0, 100.0, 2);
    assert!((speed - 0.9).abs() < 1e-6 && (accel - 0.65).abs() < 1e-6);
}

/// Merchants never fish a shoal that is diving, gone below or out of
/// reach: the net comes in as it dives (needs `OPENCONQUEST_DATA`).
#[test]
fn no_fishing_over_a_dived_shoal() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let merchants: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| {
            scene.is_own(o)
                && o.ship
                    .as_ref()
                    .is_some_and(|s| objects::cargo_capacity(s.kind) > 0.0)
        })
        .map(|o| o.id)
        .collect();
    let shoals: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| o.class == ObjectClass::FishZone)
        .map(|o| o.id)
        .collect();
    for (k, &m) in merchants.iter().enumerate() {
        let _ = scene.order_gather(m, shoals[k % shoals.len()]);
    }
    let mut bad = 0;
    for step in 0..50 * 600 {
        scene.update(0.02, &data, &mut caches);
        for &m in &merchants {
            let Some(i) = scene.object_index(m) else {
                continue;
            };
            let o = &scene.objects[i];
            if let Some(Task::Gather {
                deposit,
                phase: objects::GatherPhase::Collecting,
            }) = o.ship.as_ref().unwrap().task
            {
                let Some(d) = scene.object_index(deposit) else {
                    continue;
                };
                let dd = &scene.objects[d];
                if dd.class != ObjectClass::FishZone {
                    continue;
                }
                let dist = (dd.x - o.x).hypot(dd.y - o.y) / 64.0;
                let gone = scene.shoal_gone(deposit);
                let left = scene.shoals_shown.get(&deposit).copied();
                let netted = scene.planks.iter().any(|p| p.ship == m);
                if gone || dist > 1.6 || left.is_some_and(|l| l < SHOAL_DIVING) {
                    bad += 1;
                    eprintln!("t={:.1} ship {m} shoal {deposit} gone {gone} dist {dist:.2} left {left:?} net {netted}", step as f32 * 0.02);
                }
            }
        }
    }
    assert_eq!(bad, 0, "fished where the shoal was not");
}

/// A merchant sent to fish answers (0x49d3c0, needs `OPENCONQUEST_DATA`).
#[test]
fn merchants_answer_gather_orders() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    let ship = scene
        .objects
        .iter()
        .find(|o| {
            scene.is_own(o)
                && o.ship
                    .as_ref()
                    .is_some_and(|s| objects::cargo_capacity(s.kind) > 0.0)
        })
        .unwrap()
        .id;
    let shoal = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::FishZone)
        .unwrap()
        .id;
    scene.sounds.clear();
    scene
        .apply(
            &order::Order::GatherAt {
                ships: vec![ship],
                deposit: shoal,
                keys: 0,
            },
            &data,
            &mut caches,
        )
        .unwrap();
    assert!(
        scene
            .sounds
            .iter()
            .any(|s| s.0.starts_with("fi") && s.1.is_nan()),
        "{:?}",
        scene.sounds
    );
}

/// A ship told to load at a settlement it is away from sails alongside
/// and takes the unit aboard there (needs `OPENCONQUEST_DATA`).
#[test]
fn ships_sail_to_load() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let site = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap()
        .id;
    let mut stock = scene.garrison(site);
    stock[0] = 3;
    scene.garrisons.insert(site, stock);
    let s = scene.object_index(site).unwrap();
    // The warship farthest from the settlement that is still near enough.
    let ship = scene
        .objects
        .iter()
        .enumerate()
        .filter(|(i, o)| {
            scene.is_own(o)
                && o.ship.as_ref().is_some_and(|sh| sh.kind >= 2)
                && !scene.alongside(*i, s)
        })
        .map(|(_, o)| o.id)
        .next()
        .expect("a warship away from the settlement");
    // Room aboard.
    let k = scene.object_index(ship).unwrap();
    scene.objects[k].ship.as_mut().unwrap().crew.squads = [0, 0, 0];
    scene.objects[k].combat.as_mut().unwrap().weapons.clear();
    let before = scene.objects[scene.object_index(ship).unwrap()]
        .ship
        .as_ref()
        .unwrap()
        .crew
        .squads[0];
    scene.load_item(site, ship, 0).unwrap();
    for _ in 0..50 * 120 {
        scene.update(0.02, &data, &mut caches);
        let i = scene.object_index(ship).unwrap();
        if scene.objects[i].ship.as_ref().unwrap().crew.squads[0] > before {
            assert!(scene.alongside(i, scene.object_index(site).unwrap()));
            return;
        }
    }
    panic!("the ship never loaded");
}

/// A laden merchant sent to unload at a settlement delivers its goods
/// there (needs `OPENCONQUEST_DATA`).
#[test]
fn merchants_unload_where_sent() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let ship = scene
        .objects
        .iter()
        .find(|o| {
            scene.is_own(o)
                && o.ship
                    .as_ref()
                    .is_some_and(|s| objects::cargo_capacity(s.kind) > 0.0)
        })
        .unwrap()
        .id;
    let port = scene
        .unloading_site(scene.object_index(ship).unwrap())
        .unwrap();
    let k = scene.object_index(ship).unwrap();
    let s = scene.objects[k].ship.as_mut().unwrap();
    s.cargo = 100.0;
    s.cargo_kind = Some(Resource::Fish);
    let fish = scene.fish;
    scene
        .apply(&order::Order::Unload { ship, port }, &data, &mut caches)
        .unwrap();
    for _ in 0..50 * 180 {
        scene.update(0.02, &data, &mut caches);
        if scene.fish >= fish + 100 {
            return;
        }
    }
    panic!("never unloaded ({} fish)", scene.fish - fish);
}

/// A ship a campaign brain's player captures goes to be mended at one of
/// that player's docks (miss007-miss013, whose prize tables are not
/// ported; needs `OPENCONQUEST_DATA`).
#[test]
fn brains_mend_their_prizes() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss007").unwrap();
    assert!(scene.brain.is_some());
    let human = scene.human.map(|h| (h.nation, h.slot));
    // One of the computer's buildings stands in for a dock.
    let b = scene
        .objects
        .iter()
        .position(|o| {
            matches!(o.class, ObjectClass::Building(_))
                && o.owner
                    .is_some_and(|w| Some(w) != human && w != combat::MONSTERS)
        })
        .expect("a computer building");
    let owner = scene.objects[b].owner.unwrap();
    scene.objects[b].class = ObjectClass::Building(economy::DOCK);
    let ship = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let c = scene.objects[ship].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    let id = scene.objects[ship].id;
    let crew = scene.objects[ship].ship.as_ref().unwrap().crew;
    let units = scene.objects[ship].unit_conditions();
    let marines = [units[0].clone(), units[1].clone(), units[2].clone()];
    scene.capture(id, Some(owner), crew.class, marines, &data, &mut caches);
    let i = scene.object_index(id).unwrap();
    assert!(
        matches!(
            scene.objects[i].ship.as_ref().unwrap().task,
            Some(Task::Repair { .. })
        ),
        "{:?}",
        scene.objects[i].ship.as_ref().unwrap().task
    );
}

/// miss007's brain: the barbarian yards build for their own fleets, and
/// the Persians' yard for its merchants and guards (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss007_brain_builds_its_fleets() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss007").unwrap();
    assert!(scene.brain.is_some());
    // Twenty minutes, in ticks.
    for _ in 0..30_000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let brain = scene.brain.as_ref().unwrap();
    let ships = |k: usize| brain.fleets[k].ships.len();
    let barbarians: usize = (6..=10).map(ships).sum();
    let persians: usize = (13..=17).map(ships).sum();
    eprintln!(
        "barbarian fleets {:?}, persian fleets {:?}",
        (6..=10).map(ships).collect::<Vec<_>>(),
        (13..=17).map(ships).collect::<Vec<_>>()
    );
    assert!(barbarians > 0, "the barbarian yards built for their fleets");
    assert!(persians > 0, "the Persians built ships");
}

/// miss007's builder repairs (order 96): a barbarian structure worn in
/// its cells is mended and, once whole, sent order 10 (0x42a2f0 at
/// 0x42a3dd), its groups then ceasing fire (0x40ef70). Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn miss007_mended_structure_ceases_fire() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss007").unwrap();
    let i = scene
        .objects
        .iter()
        .position(|o| {
            matches!(o.class, ObjectClass::Barbarian(_))
                && o.cell == Some((36, 26))
                && o.combat.is_some()
        })
        .expect("the structure at (36, 26)");
    let id = scene.objects[i].id;
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.hull = c.max_hull * 0.9;
    let mut mended = false;
    for _ in 0..3000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        let i = scene.object_index(id).unwrap();
        let c = scene.objects[i].combat.as_ref().unwrap();
        let jobs = &scene.brain.as_ref().unwrap().jobs;
        if jobs.iter().any(
            |j| matches!(j, super::brain::Job::Mended { target, tries: 10, .. } if *target == id),
        ) {
            mended = true;
        }
        if mended
            && !jobs
                .iter()
                .any(|j| matches!(j, super::brain::Job::Mended { .. }))
        {
            assert!(c.hull >= c.max_hull);
            assert!(!c.fire.fires(), "the mended structure still fires at will");
            return;
        }
    }
    panic!("not mended and sent order 10 (mended: {mended})");
}

/// miss008's brain: the barbarian yards build for the raiders, home guard
/// and fishers, and the Persians' two yards for their members (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss008_brain_builds_its_fleets() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss008").unwrap();
    let brain = scene.brain.as_ref().unwrap();
    let sites = |k: usize| brain.fleets[k].ships.len();
    // The builders and yards hold their buildings.
    assert!([1, 2, 3, 8, 9].iter().all(|&k| sites(k) > 0));
    // The leviathan laid on normal swims in its zone (25, 33, 5, 5).
    let t = TILE_SIZE as f32;
    assert!(scene.objects.iter().any(|o| {
        o.class == ObjectClass::WhaleZone
            && (25..30).contains(&((o.x / t) as i32))
            && (33..38).contains(&((o.y / t) as i32))
    }));
    // Twenty minutes, in ticks.
    for _ in 0..30_000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let brain = scene.brain.as_ref().unwrap();
    let ships = |k: usize| brain.fleets[k].ships.len();
    let barbarians: usize = (4..=6).map(ships).sum();
    let persians: usize = (10..=20).map(ships).sum();
    eprintln!(
        "barbarian fleets {:?}, persian fleets {:?}",
        (4..=6).map(ships).collect::<Vec<_>>(),
        (10..=20).map(ships).collect::<Vec<_>>()
    );
    assert!(barbarians > 0, "the barbarian yards built for their fleets");
    assert!(persians > 0, "the Persians built ships");
}

/// miss009's yards build for the barbarians' and the Persians' fleets
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn miss009_brain_builds_its_fleets() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    let brain = scene.brain.as_ref().unwrap();
    let sites = |k: usize| brain.fleets[k].ships.len();
    // The builder and yards hold their buildings.
    assert!([0, 1, 2, 3, 8].iter().all(|&k| sites(k) > 0));
    // The leviathan laid on normal swims in its zone (17, 30, 5, 5).
    let t = TILE_SIZE as f32;
    assert!(scene.objects.iter().any(|o| {
        o.class == ObjectClass::WhaleZone
            && (17..22).contains(&((o.x / t) as i32))
            && (30..35).contains(&((o.y / t) as i32))
    }));
    // Twenty minutes, in ticks.
    for _ in 0..30_000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let brain = scene.brain.as_ref().unwrap();
    let ships = |k: usize| brain.fleets[k].ships.len();
    eprintln!(
        "barbarian fleets {:?}, persian fleets {:?}",
        (4..=7).map(ships).collect::<Vec<_>>(),
        (9..=14).map(ships).collect::<Vec<_>>()
    );
    // The south-western raiders and the fishers start with nothing.
    assert!(
        ships(5) + ships(7) > 0,
        "the barbarian yards built for their fleets"
    );
    assert!(
        (9..=12).map(ships).sum::<usize>() > 0,
        "the Persians built ships"
    );
}

/// miss010's brain: the builders and yards hold their buildings, the
/// strait patrol its ships, and after twenty minutes the barbarian yards
/// have built for their fleets and the Persians for theirs.
#[test]
fn miss010_brain_builds_its_fleets() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss010").unwrap();
    let brain = scene.brain.as_ref().unwrap();
    let sites = |k: usize| brain.fleets[k].ships.len();
    eprintln!(
        "start: {:?}",
        (0..brain.fleets.len()).map(sites).collect::<Vec<_>>()
    );
    // The builders and yards hold their buildings; the raiders and the
    // strait patrol start with ships.
    assert!([0, 1, 2, 3, 8, 9, 10].iter().all(|&k| sites(k) > 0));
    assert!(sites(4) > 0 && sites(16) > 0);
    // The leviathan laid on normal swims in its zone (10, 20, 5, 5).
    let t = TILE_SIZE as f32;
    assert!(scene.objects.iter().any(|o| {
        o.class == ObjectClass::WhaleZone
            && (10..15).contains(&((o.x / t) as i32))
            && (20..25).contains(&((o.y / t) as i32))
    }));
    // A barbarian building holds a disposable magic, the Persian yard at
    // (26, 30) 2500 amber.
    assert!(scene.held.iter().any(|h| h.1.kind == 9 && h.1.param == 6));
    assert!(scene
        .held
        .iter()
        .any(|h| h.1.kind == 2 && h.1.amount == 2500));
    // Twenty minutes, in ticks. The eastern patrol and the nest's guard
    // sail near the dragon and lose ships: note the most each fleet held.
    let mut most = [0usize; 19];
    for _ in 0..30_000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        let brain = scene.brain.as_ref().unwrap();
        for (m, f) in most.iter_mut().zip(&brain.fleets) {
            *m = (*m).max(f.ships.len());
        }
    }
    eprintln!(
        "most: barbarian fleets {:?}, persian fleets {:?}",
        &most[4..=7],
        &most[11..=18]
    );
    // The eastern patrol, the nest's guard and the fishers start with
    // nothing.
    assert!(
        most[5..=7].iter().all(|&n| n > 0),
        "the barbarian yards built for their fleets"
    );
    assert!(
        most[11..=15].iter().all(|&n| n > 0),
        "the Persians built for their gatherers and patrol"
    );
}

/// miss011's brain takes its fleets, lays the deposits and fills the
/// Persians' purse at the start; twenty minutes on, the barbarian yards
/// have built for their patrols and raiders and the Persians for their
/// gatherers, guards and strikers.
#[test]
fn miss011_brain_builds_its_fleets() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss011").unwrap();
    let brain = scene.brain.as_ref().unwrap();
    let sites = |k: usize| brain.fleets[k].ships.len();
    eprintln!(
        "start: {:?}",
        (0..brain.fleets.len()).map(sites).collect::<Vec<_>>()
    );
    // The yards hold their buildings; the gatherers of the west coast,
    // the raiders of the north-east, the sentries and the Persians' guards
    // and strikers start with ships.
    assert_eq!(brain.fleets.len(), 19);
    assert!([0, 4, 11].iter().all(|&k| sites(k) > 0));
    assert!([2, 3, 7, 8, 9, 10, 14, 15, 16]
        .iter()
        .all(|&k| sites(k) > 0));
    // Amber at (57, 26), (57, 35) and (51, 34), on every difficulty.
    let deposits = scene
        .objects
        .iter()
        .filter(|o| o.class == ObjectClass::Amber)
        .count();
    assert!(deposits >= 3);
    // The Persians' temple (class 0x3e) member 7 casts about stands at
    // (33, 15).
    assert!(scene
        .objects
        .iter()
        .any(|o| o.class == ObjectClass::Building(0x3e) && o.cell == Some((33, 15))));
    // The Persians' purse: 17000 amber and 10000 fish on normal, added.
    let persian = scene.objects[scene.object_index(brain.fleets[11].ships[0]).unwrap()]
        .owner
        .unwrap();
    assert!(scene.funds(persian).0 >= 17_000);
    // Twenty minutes, in ticks: note the most each fleet held.
    let mut most = [0usize; 19];
    for _ in 0..30_000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        let brain = scene.brain.as_ref().unwrap();
        for (m, f) in most.iter_mut().zip(&brain.fleets) {
            *m = (*m).max(f.ships.len());
        }
    }
    eprintln!(
        "most: barbarian fleets {:?}, persian fleets {:?}",
        &most[1..=10],
        &most[12..=16]
    );
    // The patrols of the north-east and the west coast, the raiders of
    // the west and the Persians' gatherers start with nothing.
    assert!(
        [1, 5, 6].iter().all(|&k| most[k] > 0),
        "the barbarian yards built for their fleets"
    );
    assert!(
        most[12..=13].iter().all(|&n| n > 0),
        "the Persians built for their gatherers"
    );
}

/// miss012's brain takes its fleets, lays the deposits and fills the two
/// Persian players' purses at the start; twenty minutes on, the Persian
/// yards have built for their gatherers and the south-west's raiders.
#[test]
fn miss012_brain_builds_its_fleets() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss012").unwrap();
    let brain = scene.brain.as_ref().unwrap();
    let sites = |k: usize| brain.fleets[k].ships.len();
    eprintln!(
        "start: {:?}",
        (0..brain.fleets.len()).map(sites).collect::<Vec<_>>()
    );
    // The three yards hold their buildings; the barbarians' guard,
    // gatherers and raiders and the Persians' patrols start with ships.
    assert_eq!(brain.fleets.len(), 20);
    assert!([0, 4, 12].iter().all(|&k| sites(k) > 0));
    assert!([1, 2, 3, 7, 8, 11, 15, 16].iter().all(|&k| sites(k) > 0));
    // Amber at (40, 0), (45, 0) and (53, 0), on every difficulty.
    let deposits = scene
        .objects
        .iter()
        .filter(|o| o.class == ObjectClass::Amber)
        .count();
    assert!(deposits >= 3);
    // Two Persian players, each with a war temple (class 0x3e) for its
    // caster to look about; their purses: 15000 amber each on normal,
    // added.
    let owner = |k: usize| {
        scene.objects[scene.object_index(brain.fleets[k].ships[0]).unwrap()]
            .owner
            .unwrap()
    };
    let (north, south) = (owner(4), owner(12));
    assert_ne!(north, south);
    for p in [north, south] {
        assert!(scene.funds(p).0 >= 15_000);
        assert!(scene
            .objects
            .iter()
            .any(|o| o.owner == Some(p) && o.class == ObjectClass::Building(0x3e)));
    }
    // Twenty minutes, in ticks: note the most each fleet held.
    let mut most = [0usize; 20];
    for _ in 0..30_000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        let brain = scene.brain.as_ref().unwrap();
        for (m, f) in most.iter_mut().zip(&brain.fleets) {
            *m = (*m).max(f.ships.len());
        }
    }
    eprintln!(
        "most: barbarian fleets {:?}, north-west {:?}, south-west {:?}",
        &most[1..=3],
        &most[5..=11],
        &most[13..=19]
    );
    // The Persians' gatherers and the south-west's raiders start with
    // nothing.
    assert!(
        [5, 6, 13, 14, 17].iter().all(|&k| most[k] > 0),
        "the Persian yards built for their fleets"
    );
}

/// miss013's brain takes its fleets, lays the deposits and fills the two
/// Persian players' purses at the start; twenty minutes on, the yards have
/// built for the barbarians' guard and the Persians' gatherers.
#[test]
fn miss013_brain_builds_its_fleets() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss013").unwrap();
    let brain = scene.brain.as_ref().unwrap();
    let sites = |k: usize| brain.fleets[k].ships.len();
    eprintln!(
        "start: {:?}",
        (0..brain.fleets.len()).map(sites).collect::<Vec<_>>()
    );
    // The three yards hold their buildings; the barbarians' gatherers and
    // raiders and the Persians' guards, strikers and patrols start with
    // ships.
    assert_eq!(brain.fleets.len(), 19);
    assert!([0, 4, 11].iter().all(|&k| sites(k) > 0));
    assert!([2, 3, 7, 8, 14, 15, 16].iter().all(|&k| sites(k) > 0));
    // Amber at (47, 0), (49, 1) and (38, 0), on every difficulty.
    let deposits = scene
        .objects
        .iter()
        .filter(|o| o.class == ObjectClass::Amber)
        .count();
    assert!(deposits >= 3);
    // Two Persian players, each with a war temple (class 0x3e) for its
    // caster to look about; their purses: 15000 amber each on normal,
    // added.
    let owner = |k: usize| {
        scene.objects[scene.object_index(brain.fleets[k].ships[0]).unwrap()]
            .owner
            .unwrap()
    };
    let (middle, south) = (owner(4), owner(11));
    assert_ne!(middle, south);
    for p in [middle, south] {
        assert!(scene.funds(p).0 >= 15_000);
        assert!(scene
            .objects
            .iter()
            .any(|o| o.owner == Some(p) && o.class == ObjectClass::Building(0x3e)));
    }
    // Twenty minutes, in ticks: note the most each fleet held.
    let mut most = [0usize; 19];
    for _ in 0..30_000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        let brain = scene.brain.as_ref().unwrap();
        for (m, f) in most.iter_mut().zip(&brain.fleets) {
            *m = (*m).max(f.ships.len());
        }
    }
    eprintln!(
        "most: barbarian fleets {:?}, middle {:?}, south {:?}",
        &most[1..=3],
        &most[5..=10],
        &most[12..=18]
    );
    // The barbarians' guard and the Persians' gatherers start with
    // nothing.
    assert!(most[1] > 0, "the barbarian yard built for its guard");
    assert!(
        [5, 6, 12, 13].iter().all(|&k| most[k] > 0),
        "the Persian yards built for their gatherers"
    );
}

/// miss007's wizard guard turns on the player's ship holding Hercules's
/// sword (needs `OPENCONQUEST_DATA`).
#[test]
fn miss007_guard_hunts_the_sword() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss007").unwrap();
    let i = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    scene.objects[i]
        .ship
        .as_mut()
        .unwrap()
        .treasures
        .push(objects::Treasure {
            kind: 22,
            param: 0,
            amount: 0,
        });
    for _ in 0..100 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    assert_eq!(scene.brain.as_ref().unwrap().fleets[11].state, 25);
}

/// Puts own ship `ship` beside object `target` and takes the two up for
/// reloading (Reload Object, already alongside).
fn reload_beside(scene: &mut Scene, ship: u32, target: u32) {
    let (i, t) = (
        scene.object_index(ship).unwrap(),
        scene.object_index(target).unwrap(),
    );
    let from = (scene.objects[i].x, scene.objects[i].y);
    let (x, y) = scene
        .beside_cells(t, from)
        .into_iter()
        .find(|&(x, y)| {
            !scene
                .objects
                .iter()
                .any(|o| o.id != ship && o.ship.is_some() && (o.x - x).hypot(o.y - y) < 32.0)
        })
        .unwrap();
    scene.objects[i].x = x;
    scene.objects[i].y = y;
    scene.objects[i].ship.as_mut().unwrap().path.clear();
    scene.objects[i].ship.as_mut().unwrap().task = None;
    assert!(scene.alongside(i, t));
    scene.order_load(ship, target).unwrap();
}

/// The load panel at a shipyard (c_load.cpp, 0x4e7640): the settlement's
/// stock and the ship trade marines and weapons both ways within the
/// room aboard; a treasure handed to the settlement is stored and counts
/// as delivered (message 0x5d94), and goes back aboard; Kill item throws
/// a treasure into the sea; Finish reload ends it (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn load_panel_moves_items_both_ways() {
    use crate::scene::load::{Cargo, GROUP, PRESENT, STORE, TREASURE};
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    scene.brain = None;
    let own = |scene: &Scene, cls| {
        scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::Building(cls) && scene.is_own(o))
            .map(|o| o.id)
            .unwrap()
    };
    let yard = own(&scene, SHIPYARD);
    let site = own(&scene, SETTLEMENT);
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 7))
        .unwrap()
        .id;
    // Not a reload target: a settlement (only ports, yards, docks, ships).
    assert!(scene.order_load(ship, site).is_err());
    reload_beside(&mut scene, ship, yard);
    assert_eq!(scene.load_sides(yard), Some([site, ship]));
    assert_eq!(scene.load_sides(ship), Some([ship, site]));
    let k = scene.object_index(ship).unwrap();
    scene.objects[k].ship.as_mut().unwrap().crew.squads = [0; 3];
    scene.objects[k].combat.as_mut().unwrap().weapons.clear();
    scene.garrisons.insert(site, [3, 0, 0, 2, 0, 0, 1, 0, 0]);
    let crew = |scene: &Scene| {
        scene.objects[scene.object_index(ship).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .crew
            .squads
    };
    // The settlement's list: its nine places with counts, its armour and
    // rams, its treasures and heroes (groups).
    let list = scene.load_list(site, None);
    assert_eq!(list.len(), 11);
    assert_eq!((list[0].count, list[0].cargo), (3, Some(Cargo::Unit(0, 0))));
    assert_eq!(list[4].cargo, Some(Cargo::Unit(3, 0)));
    assert_eq!(list[7].cat, STORE);
    assert_ne!(list[7].flags & GROUP, 0);
    // A bowman aboard, then back ashore; through the orders.
    let order = |from, to, cargo| Order::Transfer { from, to, cargo };
    scene
        .apply(&order(site, ship, Cargo::Unit(0, 0)), &data, &mut caches)
        .unwrap();
    assert_eq!(crew(&scene)[0], 1);
    assert_eq!(scene.garrison(site)[0], 2);
    scene
        .apply(&order(ship, site, Cargo::Unit(0, 0)), &data, &mut caches)
        .unwrap();
    assert_eq!(crew(&scene)[0], 0);
    assert_eq!(scene.garrison(site)[0], 3);
    // A ballista (two places) and a scorpion fill the bireme's two places.
    scene
        .transfer(site, ship, Cargo::Unit(6, 0), &data, &mut caches)
        .unwrap();
    assert_eq!(
        scene.transfer(site, ship, Cargo::Unit(3, 0), &data, &mut caches),
        Err("No room aboard".to_string())
    );
    assert_eq!(scene.garrison(site)[3], 2, "the refused one stays");
    scene
        .transfer(ship, site, Cargo::Unit(6, 0), &data, &mut caches)
        .unwrap();
    assert_eq!(scene.garrison(site)[6], 1);
    // Nothing that is not there moves.
    assert!(scene
        .transfer(ship, site, Cargo::Unit(6, 0), &data, &mut caches)
        .is_err());
    // The Golden Fleece handed to the settlement: stored, delivered.
    let fleece = crate::objects::Treasure {
        kind: 16,
        param: 0,
        amount: 10,
    };
    let k = scene.object_index(ship).unwrap();
    scene.objects[k].ship.as_mut().unwrap().treasures = vec![fleece];
    let tr = Cargo::Treasure { index: 0, kind: 16 };
    scene.transfer(ship, site, tr, &data, &mut caches).unwrap();
    assert!(scene.held.contains(&(site, fleece)));
    assert!(scene.delivered.contains(&16), "the brains hear of it");
    let store = scene.load_list(site, Some((STORE, 0)));
    assert_eq!(store.len(), 2, "the header and the Fleece");
    assert_eq!((store[1].cat, store[1].sub), (TREASURE, 16));
    assert_eq!(store[1].flags, PRESENT);
    // And back aboard.
    scene.transfer(site, ship, tr, &data, &mut caches).unwrap();
    assert!(!scene.held.iter().any(|h| h.0 == site));
    let k = scene.object_index(ship).unwrap();
    assert!(scene.objects[k].ship.as_ref().unwrap().has_treasure(16));
    // A second treasure finds no room aboard (a bireme holds one).
    scene
        .held
        .push((site, crate::objects::Treasure { kind: 13, ..fleece }));
    assert!(scene
        .transfer(
            site,
            ship,
            Cargo::Treasure { index: 0, kind: 13 },
            &data,
            &mut caches
        )
        .is_err());
    // Kill item: thrown overboard, it lies in the sea.
    let lying = |scene: &Scene| {
        scene
            .objects
            .iter()
            .filter(|o| o.treasure.is_some_and(|t| t.kind == 16))
            .count()
    };
    let before = lying(&scene);
    scene
        .apply(
            &Order::Discard {
                holder: ship,
                cargo: tr,
            },
            &data,
            &mut caches,
        )
        .unwrap();
    assert_eq!(lying(&scene), before + 1);
    // Kill a unit of the stock.
    scene
        .discard(site, Cargo::Unit(0, 0), &data, &mut caches)
        .unwrap();
    assert_eq!(scene.garrison(site)[0], 2);
    // Finish reload; then nothing moves.
    scene
        .apply(&Order::EndLoad { id: yard }, &data, &mut caches)
        .unwrap();
    assert!(scene.load_link(ship).is_none());
    assert!(scene
        .transfer(site, ship, Cargo::Unit(0, 0), &data, &mut caches)
        .is_err());
}

/// Two ships reload alongside each other: marines, a treasure and a
/// hero move from one to the other, as there is room; the link ends as
/// one sails off (needs `OPENCONQUEST_DATA`).
#[test]
fn ships_reload_from_each_other() {
    use crate::scene::load::Cargo;
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let ships: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| scene.is_own(o) && o.ship.is_some() && o.combat.is_some())
        .map(|o| o.id)
        .collect();
    assert!(ships.len() >= 2, "two ships");
    let (a, b) = (ships[0], ships[1]);
    for id in [a, b] {
        let k = scene.object_index(id).unwrap();
        let o = &mut scene.objects[k];
        o.ship.as_mut().unwrap().kind = 2; // royal ships: room for heroes
        o.ship.as_mut().unwrap().crew.squads = [0; 3];
        o.combat.as_mut().unwrap().weapons.clear();
    }
    reload_beside(&mut scene, a, b);
    assert_eq!(scene.load_sides(b), Some([b, a]));
    let ka = scene.object_index(a).unwrap();
    {
        let s = scene.objects[ka].ship.as_mut().unwrap();
        s.crew.squads = [2, 0, 0];
        s.heroes = vec![crate::objects::THESEUS];
        s.treasures = vec![crate::objects::Treasure {
            kind: 19,
            param: 0,
            amount: 0,
        }];
    }
    scene
        .transfer(a, b, Cargo::Unit(0, 0), &data, &mut caches)
        .unwrap();
    scene
        .transfer(
            a,
            b,
            Cargo::Hero(crate::objects::THESEUS),
            &data,
            &mut caches,
        )
        .unwrap();
    scene
        .transfer(
            a,
            b,
            Cargo::Treasure { index: 0, kind: 19 },
            &data,
            &mut caches,
        )
        .unwrap();
    let ship = |scene: &Scene, id| {
        scene.objects[scene.object_index(id).unwrap()]
            .ship
            .clone()
            .unwrap()
    };
    assert_eq!(ship(&scene, a).crew.squads[0], 1);
    assert_eq!(ship(&scene, b).crew.squads[0], 1);
    assert_eq!(ship(&scene, b).heroes, vec![crate::objects::THESEUS]);
    assert!(ship(&scene, b).has_treasure(19) && !ship(&scene, a).has_treasure(19));
    // A royal ship holds five units: the sixth finds no room.
    let kb = scene.object_index(b).unwrap();
    scene.objects[kb].ship.as_mut().unwrap().crew.squads = [5, 0, 0];
    assert_eq!(
        scene.transfer(a, b, Cargo::Unit(0, 0), &data, &mut caches),
        Err("No room aboard".to_string())
    );
    // Sailing off ends the reloading.
    let (x, y) = (scene.objects[ka].x + 64.0 * 4.0, scene.objects[ka].y);
    assert!(scene.order_move(a, x, y));
    scene.update(0.02, &data, &mut caches);
    assert!(scene.load_link(b).is_none());
}

/// A ship sent to reload sails alongside its port first and is taken up
/// on arriving (needs `OPENCONQUEST_DATA`).
#[test]
fn ships_sail_to_reload() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let yard = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(SHIPYARD) && scene.is_own(o))
        .unwrap()
        .id;
    let t = scene.object_index(yard).unwrap();
    let ship = scene
        .objects
        .iter()
        .enumerate()
        .filter(|(i, o)| scene.is_own(o) && o.ship.is_some() && !scene.alongside(*i, t))
        .map(|(_, o)| o.id)
        .next()
        .unwrap();
    scene
        .apply(
            &Order::LoadObject { ship, target: yard },
            &data,
            &mut caches,
        )
        .unwrap();
    assert!(scene.load_link(ship).is_none());
    for _ in 0..50 * 120 {
        scene.update(0.02, &data, &mut caches);
        if scene.load_link(yard).is_some() {
            assert_eq!(scene.load_link(yard).unwrap().ship, ship);
            return;
        }
    }
    panic!("the ship never got alongside");
}

/// Ships attacking a building stop once it falls to ruins (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn attacks_end_at_ruins() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let target = scene
        .objects
        .iter()
        .find(|o| matches!(o.class, ObjectClass::Barbarian(_)))
        .unwrap()
        .id;
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some() && o.is_armed())
        .unwrap()
        .id;
    scene.order_attack(ship, target).unwrap();
    scene.hit(target, f32::MAX, None, &data, &mut caches);
    let t = scene.object_index(target).unwrap();
    assert!(scene.objects[t].combat.is_none(), "ruins");
    for _ in 0..50 {
        scene.update(0.02, &data, &mut caches);
    }
    let i = scene.object_index(ship).unwrap();
    assert_eq!(scene.objects[i].ship.as_ref().unwrap().task, None);
    // The collapse plays out in 75 ticks.
    for _ in 0..(3.1 / 0.02) as usize {
        scene.update(0.02, &data, &mut caches);
    }
    let t = scene.object_index(target).unwrap();
    assert_eq!(scene.objects[t].progress, Some(1.0), "collapsed");
}

/// A ship's picture follows its hull (0x4be030) and fittings (0x4e5da0)
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn ship_looks_follow_damage() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let i = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
    let look_at = |scene: &mut Scene, pct: f32| {
        let c = scene.objects[i].combat.as_mut().unwrap();
        c.hull = c.max_hull * pct / 100.0;
        let o = &scene.objects[i];
        crate::objects::ShipLook::of(o.ship.as_ref().unwrap(), o.combat.as_ref())
    };
    assert_eq!(look_at(&mut scene, 71.0).band, 1);
    assert_eq!(look_at(&mut scene, 70.0).band, 2);
    assert_eq!(look_at(&mut scene, 36.0).band, 2);
    let low = look_at(&mut scene, 35.0);
    assert_eq!(low.band, 3);
    let mask = crate::objects::ShipLook {
        ram: 2,
        armour: 1,
        argo: false,
        ..low
    }
    .layers();
    assert_eq!(mask, 0x105 | 0x10 | 0x20);
    scene.update(0.02, &data, &mut caches);
    assert_eq!(scene.objects[i].ship.as_ref().unwrap().look, Some(low));
}

/// A blow shared out on a ship (0x4befc0) wears its marines and weapons
/// down, one at a time, and breaks them, but for the last of each; worn
/// weapons fire weaker; the units' conditions show as bars and go with
/// them through the load panel into a settlement's stock, whose first
/// unit's shows on its place (needs `OPENCONQUEST_DATA`).
#[test]
fn units_wear_and_move_with_their_condition() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let own = |scene: &Scene, cls| {
        scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::Building(cls) && scene.is_own(o))
            .map(|o| o.id)
            .unwrap()
    };
    let (yard, site) = (own(&scene, SHIPYARD), own(&scene, SETTLEMENT));
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let k = scene.object_index(ship).unwrap();
    {
        let o = &mut scene.objects[k];
        let s = o.ship.as_mut().unwrap();
        s.kind = 3;
        (s.armour, s.ram) = (0, 0);
        s.crew.squads = [0, 0, 2];
        s.conditions = Default::default();
        let c = o.combat.as_mut().unwrap();
        c.weapons = vec![4, 6];
        (c.max_hull, c.hull) = (1e6, 1e6);
    }
    // Shares: hull 18, weapons 1 + 2, crew 2 (not in the sum): a blow of
    // 210 (ours) takes 180 off the hull, and 300 and 450 points off the
    // crew and the weapons.
    let sh = combat::Shares::of(
        scene.objects[k].ship.as_ref().unwrap(),
        &scene.objects[k].combat.as_ref().unwrap().weapons,
    );
    assert_eq!((sh.hull, sh.crew, sh.weapons, sh.sum), (18, 2, 3, 21));
    scene.share_blow(ship, 210.0, None, &data, &mut caches);
    let o = &scene.objects[scene.object_index(ship).unwrap()];
    assert_eq!(o.combat.as_ref().unwrap().hull, 1e6 - 180.0);
    let units = o.unit_conditions();
    let worn = |u: &Vec<u8>| u.iter().filter(|&&c| c < 100).count();
    assert_eq!(worn(&units[2]), 1, "{units:?}");
    assert_eq!(worn(&units[4]) + worn(&units[6]), 1, "{units:?}");
    // The worn unit's bar, on the panel and in the load list.
    let cells = crate::info_panel::entries(&scene, o, None);
    assert!(cells.iter().any(|c| c.bar.is_some_and(|b| b < 100)));
    // Crushing blows break units but leave the last of the crew and of
    // the weapons standing.
    for _ in 0..20 {
        scene.share_blow(ship, 20_000.0, None, &data, &mut caches);
    }
    let k = scene.object_index(ship).unwrap();
    let o = &scene.objects[k];
    assert_eq!(o.ship.as_ref().unwrap().crew.count(), 1);
    assert_eq!(o.combat.as_ref().unwrap().weapons.len(), 1);
    assert!(o.unit_conditions().iter().flatten().all(|&c| c > 0));

    // Worn weapons fire by their mean condition.
    let o = &mut scene.objects[k];
    o.combat.as_mut().unwrap().weapons = vec![4, 4, 6];
    o.ship.as_mut().unwrap().crew.squads = [0, 0, 1];
    o.ship.as_mut().unwrap().conditions = Default::default();
    o.units().unwrap()[4] = vec![40, 81];
    o.units().unwrap()[2] = vec![30];
    assert_eq!(o.mean_condition(4), 60);
    assert_eq!(o.mean_condition(6), 100);

    // Through the load panel: the worn scorpion ashore keeps its 40%, the
    // settlement's place shows its first unit's.
    reload_beside(&mut scene, ship, yard);
    scene.set_stock_units(site, Default::default());
    let list = scene.load_list(ship, None);
    let first = list
        .iter()
        .position(|e| e.cargo == Some(Cargo::Unit(4, 0)))
        .unwrap();
    assert_eq!(list[first].bar, Some(40));
    assert_eq!(list[first + 1].bar, Some(81));
    scene
        .transfer(ship, site, Cargo::Unit(4, 0), &data, &mut caches)
        .unwrap();
    scene
        .transfer(ship, site, Cargo::Unit(2, 0), &data, &mut caches)
        .unwrap();
    assert_eq!(scene.stock_units(site)[4], [40]);
    assert_eq!(scene.stock_units(site)[2], [30]);
    let k = scene.object_index(ship).unwrap();
    assert_eq!(scene.objects[k].unit_conditions()[4], [81]);
    let list = scene.load_list(site, None);
    assert_eq!((list[5].count, list[5].bar), (1, Some(40)));
    // Back aboard with it.
    scene
        .transfer(site, ship, Cargo::Unit(4, 0), &data, &mut caches)
        .unwrap();
    let k = scene.object_index(ship).unwrap();
    assert_eq!(scene.objects[k].unit_conditions()[4], [81, 40]);
    // Kill item throws away a settlement place's last unit.
    scene.set_stock_units(site, {
        let mut u: [Vec<u8>; 9] = Default::default();
        u[0] = vec![20, 70];
        u
    });
    // (Kept least worn first, 0x440e70: [70, 20].)
    scene
        .discard(site, Cargo::Unit(0, 0), &data, &mut caches)
        .unwrap();
    assert_eq!(scene.stock_units(site)[0], [70]);
    // Selling it brings back 75% of its price by its condition.
    let amber = scene.amber;
    scene.sell_item(site, 0).unwrap();
    assert_eq!(scene.amber, amber + 30 * 75 / 100 * 70 / 100);
}

/// Worn units are mended: at a dock for a share of their price (0x4b1fc0),
/// by a settlement for amber and time (0x440fd0, 0x441ba0 state 2), and by
/// the Golden Fleece aboard (0x4b98b0) (needs `OPENCONQUEST_DATA`).
#[test]
fn worn_units_are_mended() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    scene.amber = 10_000;
    let own = |scene: &Scene, cls| {
        scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::Building(cls) && scene.is_own(o))
            .map(|o| o.id)
            .unwrap()
    };
    let site = own(&scene, SETTLEMENT);
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let k = scene.object_index(ship).unwrap();
    {
        let o = &mut scene.objects[k];
        o.ship.as_mut().unwrap().kind = 3;
        o.ship.as_mut().unwrap().crew.squads = [1, 0, 0];
        o.combat.as_mut().unwrap().weapons = vec![6];
        o.ship.as_mut().unwrap().conditions = Default::default();
        o.units().unwrap()[6] = vec![50];
        o.units().unwrap()[0] = vec![90];
        let c = o.combat.as_mut().unwrap();
        c.hull = c.max_hull;
    }
    // A full hull: the ship is still taken in for its worn units. (The
    // shipyard stands in for a dock.)
    let dock = own(&scene, SHIPYARD);
    let d = scene.object_index(dock).unwrap();
    scene.objects[d].class = ObjectClass::Building(economy::DOCK);
    scene.order_repair(ship, dock).unwrap();
    let amber = scene.amber;
    for _ in 0..50 * 120 {
        scene.update(0.02, &data, &mut caches);
        let k = scene.object_index(ship).unwrap();
        if scene.objects[k].ship.as_ref().unwrap().task.is_none() {
            break;
        }
    }
    let k = scene.object_index(ship).unwrap();
    let units = scene.objects[k].unit_conditions();
    assert_eq!((units[0].clone(), units[6].clone()), (vec![100], vec![100]));
    // 70 x 50 x 9 / 1000 and 30 x 10 x 9 / 1000.
    assert_eq!(amber - scene.amber, 31 + 2);

    // The settlement mends its stock for 50 x 70 / 100 amber and
    // 50 x 95 / 100 ticks, from its workshop's command.
    let workshop = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.class == ObjectClass::Building(economy::WORKSHOP))
        .map(|o| o.id);
    let workshop = workshop.unwrap_or_else(|| {
        let w = scene
            .objects
            .iter()
            .position(|o| {
                scene.is_own(o)
                    && matches!(o.class, ObjectClass::Building(_))
                    && o.id != site
                    && o.id != dock
            })
            .unwrap();
        scene.objects[w].class = ObjectClass::Building(economy::WORKSHOP);
        scene.objects[w].id
    });
    assert_eq!(
        scene.home_settlement(scene.object_index(workshop).unwrap()),
        Some(site)
    );
    scene.set_stock_units(site, {
        let mut u: [Vec<u8>; 9] = Default::default();
        u[6] = vec![50, 100];
        u
    });
    let amber = scene.amber;
    scene
        .apply(&Order::RepairStock { workshop }, &data, &mut caches)
        .unwrap();
    assert_eq!(amber - scene.amber, 35);
    // Ordered again it pays again and starts over (0x4438a2: no busy
    // test).
    scene.order_repair_stock(workshop).unwrap();
    assert_eq!(amber - scene.amber, 70);
    // No unit goes ashore meanwhile.
    reload_beside(&mut scene, ship, dock);
    assert!(scene
        .transfer(ship, site, Cargo::Unit(0, 0), &data, &mut caches)
        .is_err());
    for _ in 0..46 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    assert_eq!(scene.stock_units(site)[6], [100, 50]);
    for _ in 0..2 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    assert_eq!(scene.stock_units(site)[6], [100, 100]);
    assert!(scene.stock_repair_at(site).is_none());

    // The Golden Fleece (10 a tick) heals every unit a point every 100
    // ticks, the Cup (40) the marines every 25.
    let k = scene.object_index(ship).unwrap();
    {
        let o = &mut scene.objects[k];
        o.units().unwrap()[6] = vec![50];
        o.units().unwrap()[0] = vec![50];
        o.ship.as_mut().unwrap().treasures = vec![crate::objects::Treasure {
            kind: 16,
            param: 0,
            amount: 10,
        }];
    }
    scene.end_load(ship).unwrap();
    for _ in 0..200 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let k = scene.object_index(ship).unwrap();
    let units = scene.objects[k].unit_conditions();
    assert_eq!((units[0][0], units[6][0]), (52, 52));
}

/// Greek fire sets a ship ablaze from the flames' middle; they burn out on
/// their own; the Torch of Ares keeps them off (needs `OPENCONQUEST_DATA`).
#[test]
fn ships_burn_and_go_out() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let i = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
    let id = scene.objects[i].id;
    scene.ignite(i, 9, false, 100.0, &data, &mut caches);
    let f = scene.fires.iter().find(|f| f.ship == id).unwrap();
    assert_eq!((f.size, f.frame), (2, f.frames.len() / 2));
    let left = f.frames.len() - f.frame;
    assert!(scene.fire_on(&scene.objects[i], 0).is_some());
    for _ in 0..left * 2 + 2 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    assert!(scene.fires.is_empty(), "burnt out");
    let ship = scene.objects[i].ship.as_mut().unwrap();
    ship.treasures.push(crate::objects::Treasure {
        kind: 10,
        param: 0,
        amount: 0,
    });
    scene.ignite(i, 9, false, 100.0, &data, &mut caches);
    assert!(scene.fires.is_empty(), "the Torch of Ares");
}

/// At a faster game speed (larger steps) the goods gathered are not whole
/// trips; the fraction left after the last porter must not keep the ship
/// moored for ever (needs `OPENCONQUEST_DATA`).
#[test]
fn unloading_ends_at_any_speed() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let amber = scene
        .objects
        .iter()
        .filter(|o| {
            o.deposit
                .is_some_and(|d| d.resource == crate::objects::Resource::Amber)
        })
        .min_by(|a, b| {
            (a.x - 1100.0)
                .hypot(a.y)
                .total_cmp(&(b.x - 1100.0).hypot(b.y))
        })
        .unwrap()
        .id;
    let boats: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| {
            scene.is_own(o)
                && o.ship
                    .as_ref()
                    .is_some_and(|s| crate::objects::cargo_capacity(s.kind) > 0.0)
        })
        .map(|o| o.id)
        .collect();
    for &b in &boats {
        scene.order_gather(b, amber).unwrap();
    }
    let mut since: HashMap<u32, usize> = HashMap::new();
    for t in 0..15000 {
        scene.update(crate::clock::TICK * 2.0, &data, &mut caches);
        for &b in &boats {
            let Some(k) = scene.object_index(b) else {
                continue;
            };
            let unloading = matches!(
                scene.objects[k].ship.as_ref().unwrap().task,
                Some(crate::objects::Task::Gather {
                    phase: crate::objects::GatherPhase::Unloading(_),
                    ..
                })
            );
            if unloading {
                let s = *since.entry(b).or_insert(t);
                assert!(t - s < 1500, "ship {b} stuck unloading");
            } else {
                since.remove(&b);
            }
        }
    }
}

/// Three ships sent to a real dock are all mended: one mended and idle at
/// the dock makes way for the next (needs `OPENCONQUEST_DATA`).
#[test]
fn idle_ships_make_way_at_the_dock() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    scene.amber = 10_000;
    let yard = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.class == ObjectClass::Building(SHIPYARD))
        .unwrap()
        .id;
    scene.upgrade(yard, &data, &mut caches).unwrap();
    // The dock goes up first.
    for _ in 0..(50.0 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) + 1.0)) as usize {
        scene.update(0.02, &data, &mut caches);
    }
    let ids: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| scene.is_own(o) && o.ship.is_some())
        .map(|o| o.id)
        .collect();
    for &id in &ids {
        let i = scene.object_index(id).unwrap();
        let c = scene.objects[i].combat.as_mut().unwrap();
        c.hull = c.max_hull / 2.0;
        scene.order_repair(id, yard).unwrap();
    }
    for _ in 0..3000 {
        scene.update(0.02, &data, &mut caches);
    }
    for id in ids {
        let i = scene.object_index(id).unwrap();
        let c = scene.objects[i].combat.as_ref().unwrap();
        assert_eq!(c.hull, c.max_hull, "ship {id} mended");
    }
}

/// A ship under way about to leave its cell cannot be hooked (0x4b84b0,
/// 0x4b5140); one lying still, or just entering its cell, can (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn ships_slipping_away_cannot_be_hooked() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let i = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
    let t = 64.0;
    let (cx, cy) = (
        (scene.objects[i].x / t).floor() * t,
        (scene.objects[i].y / t).floor() * t,
    );
    let set = |scene: &mut Scene, x: f32, heading: usize, velocity: f32| {
        let o = &mut scene.objects[i];
        o.x = cx + x;
        o.y = cy + t / 2.0;
        let s = o.ship.as_mut().unwrap();
        s.heading = heading;
        s.velocity = velocity;
    };
    // 60 pixels a second is 2.4 a tick: 17 ticks cover 41 pixels.
    set(&mut scene, 40.0, 0, 60.0);
    assert!(scene.slipping(i), "24 pixels from the east edge");
    set(&mut scene, 10.0, 0, 60.0);
    assert!(!scene.slipping(i), "54 pixels from it");
    set(&mut scene, 10.0, 4, 60.0);
    assert!(
        scene.slipping(i),
        "10 pixels from the west edge, going west"
    );
    set(&mut scene, 60.0, 0, 0.0);
    assert!(!scene.slipping(i), "lying still");
}

/// A boarder ordered elsewhere with its walkway going down rolls it back
/// up and both ships leave, no fight (needs `OPENCONQUEST_DATA`).
#[test]
fn boarding_called_off_before_the_fight() {
    use crate::objects::GrappleStage;
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let (mine, other, _) = boarding_pair(&mut scene, 1, 0);
    scene.order_close(mine, other, true).unwrap();
    let grapple = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .grapple
    };
    let mut called = false;
    let mut fought = false;
    for _ in 0..3000 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        match grapple(&scene, mine) {
            Some(g) if g.stage == GrappleStage::Plank && g.ticks == 5 && !called => {
                let i = scene.object_index(mine).unwrap();
                scene.objects[i].ship.as_mut().unwrap().task = None;
                called = true;
            }
            Some(g) => fought |= g.stage == GrappleStage::Fight,
            None if called => break,
            None => {}
        }
    }
    assert!(called, "the walkway went down");
    assert!(!fought, "no fight");
    assert!(grapple(&scene, mine).is_none());
    for _ in 0..200 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    assert!(
        grapple(&scene, other).is_none(),
        "the boarded ship let go too"
    );
}

/// A big fort of miss012 with water beside it, handed to the human with
/// the settlement whose ground it stands in and a warship: the scene, the
/// fort and its settlement.
pub(super) fn own_fort(data: &GameData, caches: &mut Caches) -> (Scene, u32, u32) {
    let mut scene = Scene::load(data, caches, "miss012").unwrap();
    past_hold(&mut scene);
    scene.brain = None;
    let human = scene.human.unwrap();
    let me = Some((human.nation, human.slot));
    let near = |scene: &Scene, f: &MapObject, s: &MapObject| {
        let (Some((hx, hy)), Some((cx, cy))) = (s.cell, f.cell) else {
            return false;
        };
        let size = super::trade::building_cells(s);
        let dx = (hx - cx).max(cx - (hx + size - 1)).max(0);
        let dy = (hy - cy).max(cy - (hy + size - 1)).max(0);
        let _ = scene;
        dx.max(dy) <= BUILD_RADIUS && s.owner == f.owner
    };
    let (f, site) = scene
        .objects
        .iter()
        .enumerate()
        .filter(|(i, o)| {
            o.class == ObjectClass::Building(0x36) && !scene.beside_cells(*i, (o.x, o.y)).is_empty()
        })
        .find_map(|(i, o)| {
            scene
                .objects
                .iter()
                .position(|s| {
                    matches!(s.class, ObjectClass::Building(SETTLEMENT | TOWN))
                        && near(&scene, o, s)
                })
                .map(|s| (i, s))
        })
        .unwrap();
    let owner = scene.objects[f].owner;
    let ship = scene
        .objects
        .iter()
        .position(|o| o.ship.as_ref().is_some_and(|s| (2..10).contains(&s.kind)))
        .unwrap();
    // All of the fort's player's becomes the human's.
    for o in scene.objects.iter_mut() {
        if o.owner == owner {
            o.owner = me;
        }
    }
    scene.objects[ship].owner = me;
    let (fort, site) = (scene.objects[f].id, scene.objects[site].id);
    assert_eq!(scene.home_settlement(f), Some(site));
    (scene, fort, site)
}

/// Forts as load partners (FortressClassTy 0x419f80, 0x4179b0; the
/// settlement's Reload Object 0x441ba0 case 0x1f): a settlement takes up
/// one of its forts at once; the fort lists its units with their
/// conditions, its room left and a treasure place; units move within the
/// fort's room (0x5966d8: 10 for a big one), each with its condition; it
/// stores one treasure and no hero; a ship alongside reloads with it too
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn forts_reload_with_settlements_and_ships() {
    use crate::scene::load::{Cargo, EMPTY, PRESENT, TREASURE, UNIT};
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, fort, site) = own_fort(&data, &mut caches);
    let f = scene.object_index(fort).unwrap();
    // Its record: bowmen, javelin men, items 4-8 (load 1+1+1+1+2+2+2).
    assert_eq!(
        scene.objects[f].fort_places().map(|p| p.len()),
        [1, 1, 0, 0, 1, 1, 1, 1, 1]
    );
    scene.objects[f].combat.as_mut().unwrap().conditions = vec![100, 90, 80, 70, 60, 50, 40];
    // The settlement's Reload Object takes up its fort, not another's.
    let other = scene
        .objects
        .iter()
        .enumerate()
        .find(|(i, o)| {
            matches!(o.class, ObjectClass::Building(0x34..=0x36))
                && scene.home_settlement(*i) != Some(site)
        })
        .unwrap()
        .1
        .id;
    assert!(scene.order_load(site, other).is_err());
    assert!(scene.forts_of(site).contains(&fort));
    scene
        .apply(
            &Order::LoadObject {
                ship: site,
                target: fort,
            },
            &data,
            &mut caches,
        )
        .unwrap();
    assert_eq!(scene.load_sides(site), Some([site, fort]));
    assert_eq!(scene.load_sides(fort), Some([fort, site]));
    scene.run_loads();
    assert_eq!(
        scene.load_sides(fort),
        Some([fort, site]),
        "no contact needed"
    );
    // The fort's list: seven units, full (no empty place), a treasure place.
    let list = scene.load_list(fort, None);
    assert_eq!(list.len(), 8);
    assert_eq!(
        (list[0].cat, list[0].sub, list[0].flags),
        (UNIT, 0, PRESENT)
    );
    assert_eq!(list[1].bar, Some(90));
    assert_eq!(list[6].cargo, Some(Cargo::Unit(8, 0)));
    assert_eq!((list[7].cat, list[7].flags), (TREASURE, EMPTY));
    // Full: a settlement's swordsman has no room.
    scene.garrisons.insert(site, [0, 0, 1, 0, 0, 0, 0, 0, 0]);
    assert!(scene
        .transfer(site, fort, Cargo::Unit(2, 0), &data, &mut caches)
        .is_err());
    // The javelin men go ashore with their condition, the swordsman in.
    scene
        .apply(
            &Order::Transfer {
                from: fort,
                to: site,
                cargo: Cargo::Unit(1, 0),
            },
            &data,
            &mut caches,
        )
        .unwrap();
    assert_eq!(scene.stock_units(site)[1], vec![90]);
    scene
        .transfer(site, fort, Cargo::Unit(2, 0), &data, &mut caches)
        .unwrap();
    let f = scene.object_index(fort).unwrap();
    assert_eq!(scene.objects[f].fort_places()[2], vec![100]);
    assert!(scene.objects[f].fort_places()[1].is_empty());
    // Kill item throws the chosen unit away: the onager.
    scene
        .discard(fort, Cargo::Unit(8, 0), &data, &mut caches)
        .unwrap();
    let list = scene.load_list(fort, None);
    let empty = list
        .iter()
        .filter(|e| e.cat == UNIT && e.flags == EMPTY)
        .count();
    assert_eq!(empty, 2, "an onager's room");
    // One treasure fits, a second does not, and no hero.
    let tr = |kind| crate::objects::Treasure {
        kind,
        param: 0,
        amount: 0,
    };
    scene.held.push((site, tr(11)));
    scene.held.push((site, tr(15)));
    let store = Cargo::Treasure { index: 0, kind: 11 };
    scene
        .transfer(site, fort, store, &data, &mut caches)
        .unwrap();
    let second = Cargo::Treasure { index: 0, kind: 15 };
    assert!(scene
        .transfer(site, fort, second, &data, &mut caches)
        .is_err());
    assert_eq!(scene.load_list(fort, None).last().unwrap().sub, 11);
    scene.site_heroes.insert(site, vec![1]);
    assert!(scene
        .transfer(site, fort, Cargo::Hero(1), &data, &mut caches)
        .is_err());
    // Finish reload parts them; then a ship alongside takes the fort up.
    scene.end_load(site).unwrap();
    assert!(scene.load_sides(fort).is_none());
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| (2..10).contains(&s.kind)))
        .unwrap()
        .id;
    reload_beside(&mut scene, ship, fort);
    assert_eq!(scene.load_sides(fort), Some([fort, ship]));
    scene.run_loads();
    assert_eq!(scene.load_sides(ship), Some([ship, fort]));
    let s = scene.object_index(ship).unwrap();
    scene.objects[s].ship.as_mut().unwrap().crew.squads = [0; 3];
    scene.objects[s].combat.as_mut().unwrap().weapons.clear();
    scene
        .transfer(fort, ship, Cargo::Unit(5, 0), &data, &mut caches)
        .unwrap();
    let s = scene.object_index(ship).unwrap();
    assert_eq!(scene.objects[s].unit_conditions()[5], vec![70]);
    scene
        .transfer(ship, fort, Cargo::Unit(5, 0), &data, &mut caches)
        .unwrap();
    let f = scene.object_index(fort).unwrap();
    assert_eq!(scene.objects[f].fort_places()[5], vec![70]);
}

/// A shot on a fort (0x4184f0): the units take `blow x load / (35 + load)`
/// of a big fort's blow, the structure the rest; broken units are gone
/// and each place fires by its units' mean condition (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn forts_units_take_their_share_of_blows() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, fort, _) = own_fort(&data, &mut caches);
    let points = |scene: &Scene| -> u32 {
        let o = &scene.objects[scene.object_index(fort).unwrap()];
        o.fort_places()
            .iter()
            .enumerate()
            .flat_map(|(k, p)| {
                p.iter()
                    .map(move |&c| c as u32 * combat::UNIT_POINTS[k] / 100)
            })
            .sum()
    };
    let hull = |scene: &Scene| {
        let o = &scene.objects[scene.object_index(fort).unwrap()];
        o.combat.as_ref().unwrap().hull
    };
    let (p0, h0) = (points(&scene), hull(&scene));
    // A ballista's shot of 3500 (the original's); load 10 of 45.
    scene.fort_blow(
        fort,
        3500.0 / 15.0,
        0.0,
        6,
        false,
        None,
        None,
        &data,
        &mut caches,
    );
    let share = 3500 * 10 / 45;
    assert!((h0 - hull(&scene) - (3500 - share) as f32 / 15.0).abs() < 0.05);
    // The weapons take the share, the marines it by the ballista's B over
    // its A.
    let crew = (share as f32 * combat::crew_scale(6)) as u32;
    let lost = p0 - points(&scene);
    assert!(lost >= share && lost <= share + crew + 200, "{lost}");
    // Blow on blow, units break and go; each place fires by its mean.
    for _ in 0..40 {
        scene.fort_blow(
            fort,
            3500.0 / 15.0,
            0.0,
            6,
            false,
            None,
            None,
            &data,
            &mut caches,
        );
    }
    let o = &scene.objects[scene.object_index(fort).unwrap()];
    assert!(o.combat.as_ref().unwrap().weapons.len() < 7);
    for item in 0..9u8 {
        let p = &o.fort_places()[item as usize];
        if !p.is_empty() {
            let mean = p.iter().map(|&c| c as u32).sum::<u32>() / p.len() as u32;
            assert_eq!(o.mean_condition(item), mean);
        }
    }
}

/// A fort being mended mends its units as far as its structure's repair
/// has gone (0x418300, 0x419ba0), paying for them; its units cannot move
/// meanwhile (needs `OPENCONQUEST_DATA`).
#[test]
fn forts_mend_units_with_their_structure() {
    use crate::scene::load::Cargo;
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, fort, site) = own_fort(&data, &mut caches);
    let f = scene.object_index(fort).unwrap();
    {
        let c = scene.objects[f].combat.as_mut().unwrap();
        c.hull = c.max_hull / 2.0;
        c.conditions = vec![50, 100, 20, 100, 100, 100, 100];
    }
    scene.amber = 100_000;
    scene.fish = 100_000;
    scene.order_mend(site, fort).unwrap();
    scene.load_fort(site, fort).unwrap();
    assert!(scene
        .transfer(fort, site, Cargo::Unit(0, 0), &data, &mut caches)
        .is_err());
    let mut half = None;
    for _ in 0..20000 {
        scene.update(1.0 / 25.0, &data, &mut caches);
        let o = &scene.objects[scene.object_index(fort).unwrap()];
        let c = o.combat.as_ref().unwrap();
        if half.is_none() && c.hull >= c.max_hull * 0.75 {
            half = Some(o.fort_places());
        }
        if !scene.mending.contains(&fort) {
            break;
        }
    }
    // Halfway there, the bowmen are about halfway from 50 to 100.
    let half = half.unwrap();
    assert!((70..=80).contains(&half[0][0]), "{half:?}");
    let o = &scene.objects[scene.object_index(fort).unwrap()];
    assert!(o.fort_places().iter().flatten().all(|&c| c == 100));
    assert!(scene.amber < 100_000 && scene.fish < 100_000);
    assert!(scene.unit_mends.is_empty());
}

/// A Cup of Asclepius stored in a fort heals its marines a point every
/// 1000 of its rate, a Golden Fleece every unit and its structure (0x419e00,
/// 0x419f80; needs `OPENCONQUEST_DATA`).
#[test]
fn fort_gifts_heal_units() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, fort, _) = own_fort(&data, &mut caches);
    let f = scene.object_index(fort).unwrap();
    scene.objects[f].combat.as_mut().unwrap().conditions = vec![50; 7];
    let gift = |kind| crate::objects::Treasure {
        kind,
        param: 0,
        amount: 40,
    };
    scene.held.push((fort, gift(15)));
    // 250 ticks at 40: ten points, to the marines only.
    scene.heal_forts(250.0 * crate::clock::TICK);
    let places = scene.objects[f].fort_places();
    assert_eq!((places[0][0], places[1][0], places[4][0]), (60, 60, 50));
    scene.held.retain(|h| h.0 != fort);
    scene.held.push((fort, gift(16)));
    let c = scene.objects[f].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    scene.heal_forts(250.0 * crate::clock::TICK);
    let o = &scene.objects[f];
    assert_eq!(o.fort_places()[4][0], 60);
    let c = o.combat.as_ref().unwrap();
    assert!((c.hull / c.max_hull - 0.6).abs() < 0.001);
}

/// A ship sent against something on land plans its way to the water
/// nearest it once, rather than every tick (the route's end lies far from
/// a target inland), and stops in range at a cell's middle (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn attacks_on_land_plan_once() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let village = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::CreatureSpawn)
        .unwrap()
        .id;
    let s = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 7))
        .unwrap()
        .id;
    let i = scene.object_index(s).unwrap();
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.max_hull = 1e9;
    c.hull = 1e9;
    let v = scene.object_index(village).unwrap();
    if scene.objects[v].owner.is_none() {
        scene.objects[v].owner = Some((9, 9));
    }
    scene.order_attack(s, village).unwrap();
    let mut last = None;
    let mut plans = 0;
    for _ in 0..1500 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        let i = scene.object_index(s).unwrap();
        let end = scene.objects[i].ship.as_ref().unwrap().path.back().copied();
        if end.is_some() && end != last {
            plans += 1;
        }
        last = end;
    }
    assert!(plans <= 3, "re-planned {plans} times");
    // In range it stopped at a cell's middle, keeping to the grid.
    let i = scene.object_index(s).unwrap();
    let (x, y) = (scene.objects[i].x, scene.objects[i].y);
    assert_eq!((x.rem_euclid(64.0), y.rem_euclid(64.0)), (32.0, 32.0));
}

/// A blow that is not a shot (a spell's, a tornado's) wears a fort's units
/// too: every blow comes through message 0x138e (needs `OPENCONQUEST_DATA`).
#[test]
fn spells_wear_fort_units() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, fort, _) = own_fort(&data, &mut caches);
    let f = scene.object_index(fort).unwrap();
    let whole: u32 = scene.objects[f]
        .fort_places()
        .iter()
        .flatten()
        .map(|&c| c as u32)
        .sum();
    scene.hit(fort, 3500.0 / 15.0, None, &data, &mut caches);
    let f = scene.object_index(fort).unwrap();
    let after: u32 = scene.objects[f]
        .fort_places()
        .iter()
        .flatten()
        .map(|&c| c as u32)
        .sum();
    assert!(after < whole, "{after} of {whole}");
}

/// A computer player's fort mended refills its room down its default
/// arming and pays for it (0x419910); a human's does not (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn computer_forts_rearm_after_repair() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss012").unwrap();
    scene.brain = None;
    let human = scene.human.map(|h| (h.nation, h.slot));
    let f = scene
        .objects
        .iter()
        .position(|o| {
            o.class == ObjectClass::Building(0x36) && o.owner.is_some() && o.owner != human
        })
        .unwrap();
    let (id, owner) = (scene.objects[f].id, scene.objects[f].owner.unwrap());
    scene.objects[f].set_fort_places(Default::default());
    scene.earn(owner, 10_000, 10_000);
    let before = scene.funds(owner);
    scene.rearm_fort(id);
    let f = scene.object_index(id).unwrap();
    let items: Vec<u8> = scene.objects[f].combat.as_ref().unwrap().weapons.clone();
    assert_eq!(
        combat::fort_load(&items),
        combat::fort_room(0x36),
        "{items:?}"
    );
    assert!(scene.funds(owner).0 < before.0, "paid");
    // A human's fort is left as it is.
    scene.objects[f].owner = human;
    scene.objects[f].set_fort_places(Default::default());
    scene.rearm_fort(id);
    assert!(scene.objects[f].combat.as_ref().unwrap().weapons.is_empty());
}

/// The computer players weigh objects as the original (0x50ea70): armed
/// ships by fire, boarding and armour; forts by their units (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn computer_strength_follows_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss012").unwrap();
    let ship = scene
        .objects
        .iter()
        .position(|o| o.ship.as_ref().is_some_and(|s| s.kind >= 2) && o.is_armed())
        .unwrap();
    scene.objects[ship].ship.as_mut().unwrap().armour = 0;
    let base = scene.strength(ship);
    assert!(base > 0);
    scene.objects[ship].ship.as_mut().unwrap().armour = 2;
    assert_eq!(scene.strength(ship), base + 70);
    let fort = scene
        .objects
        .iter()
        .position(|o| o.class == ObjectClass::Building(0x36))
        .unwrap();
    let full = scene.strength(fort);
    scene.objects[fort].set_fort_places(Default::default());
    assert!(full > 0 && scene.strength(fort) == 0);
}

/// A ship held up by one of its player's lying still pushes it aside once
/// its patience is out, if the other's priority is lower (0x4a3a70,
/// 0x4b4b50); orders give priorities (needs `OPENCONQUEST_DATA`).
#[test]
fn ships_push_lower_priority_ones_aside() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let own: Vec<usize> = (0..scene.objects.len())
        .filter(|&i| scene.is_own(&scene.objects[i]) && scene.objects[i].ship.is_some())
        .take(2)
        .collect();
    assert_eq!(own.len(), 2);
    let (still, mover) = (scene.objects[own[0]].id, scene.objects[own[1]].id);
    let set = |scene: &mut Scene, k: usize, priority: u8, waited: f32| {
        let s = scene.objects[own[k]].ship.as_mut().unwrap();
        s.priority = priority;
        s.waited = waited;
        s.path.clear();
    };
    let wait = crate::clock::ticks(crate::objects::PATIENCE as u32) + 0.01;
    // Equal priorities: it waits.
    set(&mut scene, 0, 0, 0.0);
    set(&mut scene, 1, 0, wait);
    assert!(!scene.push_aside(still, mover));
    // Not yet out of patience.
    set(&mut scene, 1, 90, 0.0);
    assert!(!scene.push_aside(still, mover));
    // A gatherer pushes an idle ship, which takes its priority.
    set(&mut scene, 1, 90, wait);
    assert!(scene.push_aside(still, mover));
    let s = scene.objects[own[0]].ship.as_ref().unwrap();
    assert_eq!((s.path.len(), s.priority), (1, 90));
    let m = scene.objects[own[1]].ship.as_ref().unwrap();
    assert_eq!(m.patience, crate::objects::PATIENCE * 3);
    assert_eq!(
        crate::objects::Task::Gather {
            deposit: 0,
            phase: crate::objects::GatherPhase::ToDeposit
        }
        .priority(),
        Some(90)
    );
    // A gatherer waiting on its way is pushed with its task kept, going
    // on from the cell aside.
    let task = crate::objects::Task::Gather {
        deposit: 0,
        phase: crate::objects::GatherPhase::ToDeposit,
    };
    let (sx, sy) = (scene.objects[own[0]].x, scene.objects[own[0]].y);
    let goal = scene
        .plan_route(own[0], sx + 3.0 * 64.0, sy)
        .or_else(|| scene.plan_route(own[0], sx - 3.0 * 64.0, sy));
    if let Some(goal) = goal.filter(|g| !g.is_empty()) {
        set(&mut scene, 0, 50, 0.0);
        let s = scene.objects[own[0]].ship.as_mut().unwrap();
        s.task = Some(task);
        s.order_tag = task.tag();
        s.path = goal.clone();
        let m = scene.objects[own[1]].ship.as_mut().unwrap();
        m.priority = 90;
        m.patience = crate::objects::PATIENCE;
        m.waited = wait;
        assert!(scene.push_aside(still, mover));
        let s = scene.objects[own[0]].ship.as_ref().unwrap();
        assert_eq!(s.task, Some(task));
        assert_eq!(s.path.back(), goal.back());
    }
}

/// Held up 150 ticks on a plain move, a ship gives the move up and lies
/// still (0x4a618d); one with a task keeps waiting (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn ships_give_up_a_move_held_up_too_long() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let own: Vec<usize> = (0..scene.objects.len())
        .filter(|&i| scene.is_own(&scene.objects[i]) && scene.objects[i].ship.is_some())
        .take(2)
        .collect();
    let (id, other) = (scene.objects[own[0]].id, scene.objects[own[1]].id);
    let hold = |scene: &mut Scene, waited: f32| {
        let s = scene.objects[own[0]].ship.as_mut().unwrap();
        s.path = std::iter::once((0.0, 0.0)).collect();
        s.waited = waited;
        scene.go_round(vec![(id, (0, 0), false, false, other)]);
        scene.objects[own[0]].ship.as_ref().unwrap().path.is_empty()
    };
    assert!(!hold(&mut scene, crate::clock::ticks(100)));
    assert!(hold(&mut scene, crate::clock::ticks(150)));
}

/// A settlement sells an armour piece or a ram from its stock for three
/// quarters of its fitting cost (needs `OPENCONQUEST_DATA`).
#[test]
fn settlements_sell_armour_and_rams() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let site = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(_)))
        .unwrap()
        .id;
    scene.fittings.insert(site, (vec![2], vec![1]));
    let amber = scene.amber;
    scene.sell_fitting(site, false, 2).unwrap();
    assert_eq!(scene.amber, amber + 45);
    scene.sell_fitting(site, true, 1).unwrap();
    assert_eq!(scene.amber, amber + 45 + 18);
    assert!(scene.sell_fitting(site, true, 1).is_err(), "none left");
}

/// The sound panel's switches answer for the original's channels.
#[test]
fn sounds_follow_the_panel_switches() {
    use crate::scene::{sound_kind, SoundKind};
    assert_eq!(sound_kind("am4"), SoundKind::Voice);
    assert_eq!(sound_kind("jb_d1"), SoundKind::Voice);
    assert_eq!(sound_kind("abor6a"), SoundKind::Battle);
    assert_eq!(sound_kind("htk1"), SoundKind::Battle);
    assert_eq!(sound_kind("pit3"), SoundKind::Ambient);
    assert_eq!(sound_kind("deny"), SoundKind::Always);
}

/// Greek fire in flight leaves a trail of `TRAKS08` particles a tick, with
/// graphic trails on (0x4974d0); none with them off (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn greek_fire_leaves_a_trail() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let ships: Vec<usize> = (0..scene.objects.len())
        .filter(|&i| scene.objects[i].ship.is_some())
        .take(2)
        .collect();
    let (from, target) = (
        (scene.objects[ships[0]].x, scene.objects[ships[0]].y),
        scene.objects[ships[1]].id,
    );
    let fire = |scene: &mut Scene, caches: &mut Caches| {
        scene.fire_shot(
            super::shot::Firing {
                item: combat::GREEK_FIRE,
                letter: None,
                flaming: true,
                from,
                target,
                strikes: true,
                damage: 0.0,
                wounds: 0.0,
                wear: 0.0,
                rope: None,
                shooter: None,
                by: None,
                to: None,
            },
            &data,
            caches,
        );
    };
    scene.droplets.clear();
    scene.trails = false;
    fire(&mut scene, &mut caches);
    scene.run_shots(0.2, &data, &mut caches);
    assert!(scene.droplets.iter().all(|d| d.trail.is_none()));
    scene.trails = true;
    fire(&mut scene, &mut caches);
    scene.run_shots(0.2, &data, &mut caches);
    assert!(scene.droplets.iter().any(|d| d.trail == Some(8)));
    assert!(!scene.trail_art[&8].is_empty(), "TRAKS08 loaded");
}

/// A volcano with a strength (field 8) erupts in its time and throws
/// burning stones, Q, R and S among them, about its crater (0x445ad0;
/// needs `OPENCONQUEST_DATA`).
#[test]
fn volcanoes_erupt_and_throw_stones() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    let mut letters = std::collections::BTreeSet::new();
    for _ in 0..10 * 60 * 25 {
        scene.update(0.04, &data, &mut caches);
        letters.extend(scene.shots.iter().filter(|s| s.volcanic).map(|s| s.letter));
    }
    assert!(
        [16, 17, 18].iter().all(|l| letters.contains(l)),
        "{letters:?}"
    );
}

/// A campaign victory carries the Argo and the town on: miss004's Pylos
/// arrives at (32, 34) of miss005's map and the Argo at (31, 31), as the
/// original's victory handler moves them (needs `OPENCONQUEST_DATA`).
#[test]
fn campaign_carries_the_town_and_the_argo() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut from = Scene::load(&data, &mut caches, "miss004").unwrap();
    let pylos = from
        .objects
        .iter()
        .find(|o| {
            from.is_own(o)
                && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN))
                && o.cell
                    .is_some_and(|(x, y)| (25..=27).contains(&x) && (5..=7).contains(&y))
        })
        .map(|o| o.id)
        .expect("Pylos is the player's");
    from.site_heroes.insert(pylos, vec![0, 3]);
    // Picked from the list it has no royal ship to be the Argo: make one.
    let ship = from
        .objects
        .iter()
        .position(|o| from.is_own(o) && o.ship.is_some())
        .unwrap();
    from.objects[ship].ship.as_mut().unwrap().argo = true;
    let carry = from.carry();
    let scene =
        Scene::load_carried(&data, &mut caches, "miss005", super::NORMAL, Some(&carry)).unwrap();
    let town = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.cell == Some((32, 34)))
        .expect("the town arrives");
    assert!(matches!(
        town.class,
        ObjectClass::Building(SETTLEMENT | TOWN)
    ));
    assert_eq!(scene.site_heroes.get(&town.id), Some(&vec![0, 3]));
    assert_eq!(scene.carried, vec![0, 3]);
    let argo = scene
        .objects
        .iter()
        .find(|o| o.ship.as_ref().is_some_and(|s| s.argo))
        .expect("the Argo arrives");
    let t = TILE_SIZE as f32;
    assert_eq!(((argo.x / t) as i32, (argo.y / t) as i32), (31, 31));
}

/// Campaign missions whose maps hold no town of the player's, picked from
/// the list, start with a bare one where the carried town would stand,
/// the heroes so far waiting there (needs `OPENCONQUEST_DATA`).
#[test]
fn list_picked_missions_get_a_bare_town() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    for m in [
        "miss005", "miss006", "miss007", "miss009", "miss010", "miss011", "miss012",
    ] {
        let scene = Scene::load(&data, &mut caches, m).unwrap();
        let sites = scene.own_sites();
        assert_eq!(sites.len(), 1, "{m} has one town");
        let heroes = scene
            .site_heroes
            .get(&sites[0])
            .cloned()
            .unwrap_or_default();
        assert_eq!(heroes, scene.carried, "{m}'s heroes wait in it");
        assert!(!heroes.is_empty(), "{m} has heroes");
    }
}

/// Each campaign mission picked from the list starts with what its judge
/// wants kept (the town, the Argo, Jason, the parchment) and is not lost
/// as it starts (needs `OPENCONQUEST_DATA`).
#[test]
fn campaign_missions_start_unlost() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    for m in 0..14 {
        let mut scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).unwrap();
        let judge = story::judge_of(&scene.mission).unwrap();
        let failing: Vec<_> = judge.keep.iter().filter(|&&g| !scene.goal_met(g)).collect();
        assert!(failing.is_empty(), "miss{m:03} fails {failing:?}");
        for _ in 0..600 {
            scene.update(0.04, &data, &mut caches);
        }
        assert_eq!(scene.outcome, None, "miss{m:03}");
    }
}

/// miss001's settlement, port and dock sleep (record field 4 = 3): unseen
/// on the radar, unclickable, unharmed, till a ship of the player's comes
/// beside them, when all three wake together (needs `OPENCONQUEST_DATA`).
#[test]
fn sleeping_buildings_wake_together() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss001").unwrap();
    let asleep: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| o.asleep.is_some())
        .map(|o| o.id)
        .collect();
    assert_eq!(asleep.len(), 3);
    let town = scene.own_sites()[0];
    let i = scene.object_index(town).unwrap();
    let hull = scene.objects[i].combat.as_ref().unwrap().hull;
    scene.hit(town, 10.0, None, &data, &mut caches);
    let i = scene.object_index(town).unwrap();
    assert_eq!(scene.objects[i].combat.as_ref().unwrap().hull, hull);
    // A while with the boat away: still asleep, not lost.
    for _ in 0..200 {
        scene.update(0.04, &data, &mut caches);
    }
    assert!(!scene.woken);
    // The boat beside the dock (2, 3).
    let boat = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let t = TILE_SIZE as f32;
    (scene.objects[boat].x, scene.objects[boat].y) = (3.5 * t, 4.5 * t);
    for _ in 0..20 {
        scene.update(0.04, &data, &mut caches);
    }
    assert!(scene.woken);
    assert!(asleep.iter().all(|&id| {
        let i = scene.object_index(id).unwrap();
        scene.objects[i].asleep.is_none() && scene.objects[i].record.fields[4] == 0
    }));
}

/// miss009's north-east sea is shut off by land: the player's lightning
/// digs a way through (0x5030c0), and a strike in the strait that opens
/// none loses the mission (0x464340) (needs `OPENCONQUEST_DATA`).
#[test]
fn lightning_digs_miss009_strait() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    let at = |c: i32| c as f32 * t + t / 2.0;
    let strike = |scene: &mut Scene, c: (i32, i32), human: bool, caches: &mut Caches| {
        let caster = human
            .then(|| scene.human.map(|h| (h.nation, h.slot)))
            .flatten();
        scene.lightning.push((at(c.0), at(c.1), 0.0, caster));
        scene.update(0.02, &data, caches);
    };
    // The way through, dug.
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    assert!(!scene.strait_open(), "shut at first");
    assert!(!scene.nav.is_water(59, 4));
    strike(&mut scene, (59, 4), true, &mut caches);
    assert!(scene.nav.is_water(59, 4), "dug into sea");
    assert!(scene.strait_struck && scene.strait_open());
    assert!(scene
        .dug
        .iter()
        .any(|&(l, x, y, _)| (l, x, y) == (0, 59, 4)));
    for _ in 0..50 * 5 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.strait_checked);
    assert_eq!(scene.losing, None);
    // A saved game keeps the dig.
    let saved = scene.save();
    let back = Scene::restore(&data, &mut caches, &saved).unwrap();
    assert!(back.nav.is_water(59, 4) && back.strait_checked);
    // A strike that opens nothing loses it.
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    strike(&mut scene, (56, 6), true, &mut caches);
    assert!(scene.strait_struck && !scene.strait_open());
    for _ in 0..50 * 5 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.losing.is_some(), "the strait is shut");
    // Outside the strait, or the game's own: not heeded.
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    strike(&mut scene, (63, 4), true, &mut caches);
    strike(&mut scene, (57, 5), false, &mut caches);
    assert!(!scene.strait_struck);
}

/// Lightning on the cells one in from the map's side digs as anywhere
/// else (0x502da0 gives the border's edges), and never shuts water (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn lightning_digs_at_the_border() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut dug = 0;
    for m in ["miss001", "miss009", "miss010"] {
        let base = Scene::load(&data, &mut caches, m).unwrap();
        let (w, h) = (base.nav.width as i32, base.nav.height as i32);
        let ring: Vec<(i32, i32)> = (1..w - 1)
            .flat_map(|x| [(x, 1), (x, h - 2)])
            .chain((1..h - 1).flat_map(|y| [(1, y), (w - 2, y)]))
            .filter(|&(x, y)| !base.nav.is_water(x, y))
            .take(12)
            .collect();
        for c in ring {
            let mut s = Scene::load(&data, &mut caches, m).unwrap();
            s.dig(c.0, c.1, &data, &mut caches);
            dug += usize::from(s.nav.is_water(c.0, c.1));
            for y in 0..h {
                for x in 0..w {
                    assert!(
                        !base.nav.is_water(x, y) || s.nav.is_water(x, y),
                        "{m} {c:?} shut {x},{y}"
                    );
                }
            }
        }
    }
    assert!(dug > 0, "border cells dug");
}

/// A town's building beside a strike loses a quarter of its condition;
/// with its ground dug away it falls (WorkLandObj 0x427f50, the town's
/// check) (needs `OPENCONQUEST_DATA`).
#[test]
fn lightning_on_a_towns_buildings() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    scene.brain = None;
    let i = (0..scene.objects.len())
        .find(|&i| {
            matches!(scene.objects[i].class, ObjectClass::Building(0x37..=0x3e))
                && scene.site_holds(i)
        })
        .expect("a town's building");
    let id = scene.objects[i].id;
    let (cx, cy) = scene.objects[i].cell.unwrap();
    let max = scene.objects[i].combat.as_ref().unwrap().max_hull;
    // Struck beside it, on water or ground that leaves its site whole.
    let beside = [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
    ]
    .into_iter()
    .map(|(dx, dy)| (cx + dx, cy + dy))
    .find(|&(x, y)| scene.nav.is_water(x, y))
    .unwrap();
    scene.lightning.push((
        beside.0 as f32 * t + t / 2.0,
        beside.1 as f32 * t + t / 2.0,
        0.0,
        None,
    ));
    scene.update(0.02, &data, &mut caches);
    let i = scene.object_index(id).expect("still standing");
    let hull = scene.objects[i].combat.as_ref().unwrap().hull;
    assert!(
        (hull - max * 0.75).abs() < 1.0,
        "a quarter off: {hull} of {max}"
    );
    // Its own cell dug into sea: the site no longer holds.
    let at = scene.objects[i].cell.unwrap();
    scene.dig(at.0, at.1, &data, &mut caches);
    let i = scene.object_index(id).unwrap();
    assert!(!scene.site_holds(i));
}

/// Lightning on a settlement (TownObj 0x441ba0 -> 0x4449d4): a quarter of
/// its condition beside it with its site whole, felled on its anchor; on
/// reefs and whirlpools (removed, 0x430d38, 0x448479), amber standing (its
/// amount to 0: it lies fallow, 0x48ae45) and a fish shoal up (emptied
/// and down, 0x48e7ef; gone at the end of its dive). Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn lightning_on_towns_and_nature() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    let at = |c: (i32, i32)| (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
    // A settlement whose 3 x 3 about its anchor has water outside it.
    let (mut scene, id, anchor, water) = (0..14)
        .find_map(|m| {
            let mut scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).ok()?;
            scene.brain = None;
            let (id, anchor, water) = scene
                .objects
                .iter()
                .filter(|o| {
                    matches!(
                        o.class,
                        ObjectClass::Building(crate::economy::SETTLEMENT | crate::economy::TOWN)
                    )
                })
                .find_map(|o| {
                    let (x, y) = o.cell?;
                    let w = [(-1, -1), (0, -1), (1, -1), (-1, 0), (-1, 1)]
                        .into_iter()
                        .map(|(dx, dy)| (x + dx, y + dy))
                        .find(|&(cx, cy)| scene.nav.is_water(cx, cy))?;
                    Some((o.id, (x, y), w))
                })?;
            Some((scene, id, anchor, water))
        })
        .expect("a settlement by the sea");
    let i = scene.object_index(id).unwrap();
    assert!(scene
        .check_ground_but(crate::economy::SETTLEMENT, anchor.0, anchor.1, false)
        .is_ok());
    let max = scene.objects[i].combat.as_ref().unwrap().max_hull;
    let (x, y) = at(water);
    scene.lightning_strike(x, y, None, &data, &mut caches);
    let i = scene.object_index(id).expect("still standing");
    let hull = scene.objects[i].combat.as_ref().unwrap().hull;
    assert!((hull - max * 0.75).abs() < 1e-3, "{hull} of {max}");
    let (x, y) = at(anchor);
    scene.lightning_strike(x, y, None, &data, &mut caches);
    // Fallen: gone, or its ruins (no combat left).
    assert!(scene.object_index(id).is_none_or(|i| scene.objects[i]
        .combat
        .as_ref()
        .is_none_or(|c| c.hull <= 0.0)));
    // Nature about a strike on the water.
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let sea = scene.nav.nearest_water(20, 20).unwrap();
    scene
        .spawn_hazard(
            magic::Spell::Whirlpool,
            sea.0,
            sea.1,
            None,
            &data,
            &mut caches,
        )
        .unwrap();
    let whirl = scene.objects.iter().map(|o| o.id).max().unwrap();
    let (x, y) = at(sea);
    // Cast, it starts under the water (state 3): spared.
    assert_eq!(scene.hazards[&whirl].state, 3);
    scene.lightning_strike(x, y, None, &data, &mut caches);
    assert!(scene.object_index(whirl).is_some());
    while scene.hazards[&whirl].state == 3 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    scene.lightning_strike(x, y, None, &data, &mut caches);
    assert!(scene.object_index(whirl).is_none());
    assert!(!scene.hazards.contains_key(&whirl));
    assert!(!scene.summons.iter().any(|s| s.0 == whirl));
    let (amber, cell, regrows) = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Amber && o.deposit.is_some())
        .map(|o| {
            let f = |i: usize| o.record.fields[i] as i32;
            (o.id, (f(6), f(7)), o.deposit.unwrap().regrows)
        })
        .expect("amber");
    let (x, y) = at(cell);
    scene.lightning_strike(x, y, None, &data, &mut caches);
    match scene.object_index(amber) {
        Some(k) => {
            assert!(regrows > 0);
            assert!(scene.objects[k].deposit.is_none());
            assert!(scene.objects[k].dormant.is_some());
        }
        None => assert_eq!(regrows, 0),
    }
    let (shoal, cell) = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::FishZone && o.deposit.is_some())
        .map(|o| (o.id, ((o.x / t).floor() as i32, (o.y / t).floor() as i32)))
        .expect("a fish shoal");
    let (x, y) = at(cell);
    scene.lightning_strike(x, y, None, &data, &mut caches);
    let k = scene.object_index(shoal).unwrap();
    assert_eq!(
        scene.objects[k].deposit.and_then(|d| d.remaining),
        Some(0.0)
    );
    let wait = scene.shoals_gone[&shoal];
    assert!((crate::clock::ticks(25) - 1e-4..=crate::clock::ticks(75) + 1e-4).contains(&wait));
    scene.move_shoals(wait + 1e-3);
    assert!(scene
        .object_index(shoal)
        .is_none_or(|k| scene.objects[k].deposit.is_none()));
}

/// Quick lime's figures from the exe: its power by the heavy weapons'
/// damage level (row 13's B), no hull share, the crew's taken as is, and
/// the unique ships' reload at the third rate column (0x4c06e0).
#[test]
fn quick_lime_figures() {
    assert_eq!([0, 1, 2].map(combat::lime_power), [200.0, 240.0, 280.0]);
    assert_eq!(combat::hull_percent(combat::QUICKLIME), 0);
    assert_eq!(combat::crew_scale(combat::QUICKLIME), 1.0);
    for level in 0..3 {
        assert_eq!(
            combat::reload_seconds(combat::QUICKLIME, level),
            crate::clock::ticks(6000 / 55)
        );
        assert_eq!(
            combat::reload_seconds(combat::GREEK_FIRE, level),
            crate::clock::ticks(6000 / 80)
        );
    }
}

/// A limed ship: a hit limes it 120 ticks (24 under its weapon shield),
/// added up; it crawls, boards at half strength, will not take a fire
/// order, shows no fire power, and comes clear when the time is out; a
/// ship mended in a dock is untouched (needs `OPENCONQUEST_DATA`).
#[test]
fn quick_lime_blinds() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let i = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind >= 2))
        .unwrap();
    scene.objects[i].combat.as_mut().unwrap().weapons = vec![4];
    scene.objects[i].ship.as_mut().unwrap().crew.squads = [0, 0, 4];
    let id = scene.objects[i].id;
    let strength = scene.boarding_strength(i);
    scene.lime(i, combat::LIME_TICKS);
    scene.lime(i, combat::LIME_TICKS);
    let c = scene.objects[i].combat.as_ref().unwrap();
    assert!((c.lime - crate::clock::ticks(240)).abs() < 1e-3);
    let enemy = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && !scene.is_own(o) && o.owner.is_some())
        .unwrap()
        .id;
    assert!(scene.order_attack(id, enemy).is_err(), "no fire order");
    assert_eq!(scene.fighting_strength(i), strength / 2);
    let info = crate::object_info::of(&scene, &scene.objects[i]).unwrap();
    assert_eq!(info.fire, Some((0, 0)));
    assert_eq!(info.experience, None);
    // It crawls: its way settles at a tick's gain.
    let accel = objects::ship_accel(scene.objects[i].ship.as_ref().unwrap().kind);
    let to = scene.nav.nearest_water(
        (scene.objects[i].x / 64.0) as i32 + 6,
        (scene.objects[i].y / 64.0) as i32,
    );
    let (x, y) = to.unwrap();
    let ok = scene.order_move(id, x as f32 * 64.0 + 32.0, y as f32 * 64.0 + 32.0);
    assert!(ok);
    let mut v = 0.0f32;
    for _ in 0..50 * 4 {
        scene.update(0.02, &data, &mut caches);
        let i = scene.object_index(id).unwrap();
        v = v.max(scene.objects[i].ship.as_ref().unwrap().velocity);
    }
    assert!(
        v > 0.0 && v <= accel * crate::clock::TICK + 1e-3,
        "crawls: {v}"
    );
    for _ in 0..50 * 10 {
        scene.update(0.02, &data, &mut caches);
    }
    let i = scene.object_index(id).unwrap();
    assert_eq!(scene.objects[i].combat.as_ref().unwrap().lime, 0.0);
    // Under its weapon shield, a fifth as long.
    scene.objects[i].combat.as_mut().unwrap().protect_weapons = 10.0;
    scene.lime(i, combat::LIME_TICKS);
    let c = scene.objects[i].combat.as_ref().unwrap();
    assert!((c.lime - crate::clock::ticks(24)).abs() < 1e-3);
}

/// A limed town's training stands still; when the lime wears off the job
/// runs again from when the lime struck (0x425990: `+0x124` set to the
/// lime's start), the limed time counted (needs `OPENCONQUEST_DATA`).
#[test]
fn quick_lime_stops_a_towns_work() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let site = scene.own_sites()[0];
    scene.amber += 100_000;
    scene.fish += 100_000;
    for _ in 0..6 {
        scene.train(site, 2).unwrap();
    }
    for _ in 0..25 {
        scene.update(0.04, &data, &mut caches);
    }
    let before = scene.trainings[0].elapsed;
    assert!(before > 0.0);
    let i = scene.object_index(site).unwrap();
    scene.lime(i, combat::LIME_BUILDING_TICKS);
    for _ in 0..100 {
        scene.update(0.04, &data, &mut caches);
    }
    assert_eq!(scene.trainings[0].elapsed, before, "stands still");
    // Run till the lime is out.
    while scene.objects[scene.object_index(site).unwrap()]
        .combat
        .as_ref()
        .unwrap()
        .lime
        > 0.0
    {
        scene.update(0.04, &data, &mut caches);
    }
    let t = &scene.trainings[0];
    assert!(
        (t.elapsed - crate::clock::ticks(200)).abs() <= 0.04 + 1e-3,
        "from the lime's start: {}",
        t.elapsed
    );
}

/// A limed building's mending stands still, nothing paid (0x425990 calls
/// the step 0x425100 only while not limed) (needs `OPENCONQUEST_DATA`).
#[test]
fn quick_lime_stops_mending() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let site = scene.own_sites()[0];
    scene.amber += 100_000;
    let i = scene.object_index(site).unwrap();
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    scene.mend_by(site, site).unwrap();
    let i = scene.object_index(site).unwrap();
    scene.lime(i, combat::LIME_BUILDING_TICKS);
    let hull = |s: &Scene| {
        s.objects[s.object_index(site).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull
    };
    let (before, amber) = (hull(&scene), scene.amber);
    for _ in 0..100 {
        scene.update(0.04, &data, &mut caches);
    }
    assert_eq!((hull(&scene), scene.amber), (before, amber), "stands still");
    for _ in 0..150 {
        scene.update(0.04, &data, &mut caches);
    }
    assert!(hull(&scene) > before, "mended again once the lime is out");
}

/// A temple's learning and the Flame stand still while the temple is
/// limed; when the lime wears off they count from when it struck (the
/// temples' message 0, after 0x425990 set `+0x124` to the lime's start),
/// and may end at once (0x438300, 0x439260).
#[test]
fn quick_lime_stops_a_temples_work() {
    let mut m = Magic::default();
    let (sea, war) = (7, 8);
    m.start_learning(Temple::Sea, Some(sea));
    m.brew_flame(Some(war));
    let all = crate::permits::Permits::ALL;
    m.update(crate::clock::ticks(50), &[], all);
    m.update(crate::clock::ticks(500), &[sea, war], all);
    assert_eq!(m.learning(Temple::Sea), Some(crate::clock::ticks(50)));
    assert!(m.flame_brewing().unwrap() < 1.0);
    // Limed 199 ticks: the learning (200) is not done, and goes on from 199.
    m.lime_cleared(sea, crate::clock::ticks(199), all);
    assert_eq!(m.learning(Temple::Sea), Some(crate::clock::ticks(199)));
    assert!(!m.knows(Spell::Wind));
    // Another temple's lime does not touch it; 200 ticks end it at once.
    m.lime_cleared(war, crate::clock::ticks(100), all);
    assert_eq!(m.flame_brewing(), Some(0.4));
    m.lime_cleared(sea, crate::clock::ticks(200), all);
    assert!(m.knows(Spell::Wind) && m.learning(Temple::Sea).is_none());
}

/// After a spell is learnt, while fewer than six are known, the spell at
/// 0x596da4 + known x 4 (one before the order's own entry at 0x596da8,
/// so the one just learnt) is tested and passed over with the next one
/// while its permission bit is clear (0x438300, 0x42fd70).
#[test]
fn learning_passes_over_withheld_spells() {
    let mut m = Magic::default();
    let mut p = crate::permits::Permits::ALL;
    // Wind (bit 59) withheld: learning it passes over View as well, and
    // stops at Waterspout's test (View, bit 60, allowed).
    p.set(59, false);
    m.start_learning(Temple::Sea, None);
    m.update(magic::LEARN_SECONDS, &[], p);
    assert_eq!(m.known(Temple::Sea).len(), 2);
    // All allowed: one at a time.
    let mut m = Magic::default();
    m.start_learning(Temple::Sea, None);
    m.update(magic::LEARN_SECONDS, &[], crate::permits::Permits::ALL);
    assert_eq!(m.known(Temple::Sea), [Spell::Wind]);
}

/// A ship's move state (+0x58c): under way 5 between cells, braking 4 in
/// its route's last cell, lying still 0 there; a sea monster weighs it
/// 50, 60 and 100 (0x4911f0) (needs `OPENCONQUEST_DATA`).
#[test]
fn ship_move_states() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    let i = scene.object_index(ship).unwrap();
    let (x, y) = (scene.objects[i].x, scene.objects[i].y);
    let t = 64.0;
    // Some water cell a few cells off, reachable.
    let to = [(4.0, 0.0), (-4.0, 0.0), (0.0, 4.0), (0.0, -4.0)]
        .into_iter()
        .find(|&(dx, dy)| scene.order_move(ship, x + dx * t, y + dy * t))
        .expect("a move");
    let _ = to;
    let mut seen = Vec::new();
    for _ in 0..2000 {
        scene.update(0.04, &data, &mut caches);
        let i = scene.object_index(ship).unwrap();
        let s = scene.objects[i].ship.as_ref().unwrap();
        if seen.last() != Some(&s.move_state) {
            seen.push(s.move_state);
        }
        if s.path.is_empty() && s.move_state == 0 {
            break;
        }
    }
    assert!(seen.contains(&5), "under way: {seen:?}");
    let at = seen.iter().position(|&s| s == 4).expect("braking");
    assert_eq!(&seen[at..], [4, 0], "brakes then lies still: {seen:?}");
}

/// A sea monster (umonster.cpp) starts under water, unseen; it comes up
/// to strike a ship beside it, biting once a strike, the strikes 43 ticks
/// and a wait of 50-125 under water apart, each bite costing it a
/// twentieth of its strength (needs `OPENCONQUEST_DATA`).
#[test]
fn sea_monster_strikes_in_turns() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let human = scene.human.unwrap();
    let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap().id;
    scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].owner = Some((1, (human.slot + 1) % 4));
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.weapons.clear();
    (c.max_hull, c.hull) = (1e7, 1e7);
    let cell = (
        (scene.objects[i].x / 64.0) as i32,
        (scene.objects[i].y / 64.0) as i32,
    );
    let me = Some((human.nation, human.slot));
    scene
        .spawn_hazard(Spell::Monster, cell.0, cell.1, me, &data, &mut caches)
        .unwrap();
    let monster = scene.summons[0].0;
    scene.update(0.04, &data, &mut caches);
    assert!(scene.creature_hidden(monster), "under water at first");
    let mut bites = Vec::new();
    let mut last = scene.objects[scene.object_index(ship).unwrap()]
        .combat
        .as_ref()
        .unwrap()
        .hull;
    let strength = |s: &Scene| {
        s.objects[s.object_index(monster).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull
    };
    let full = strength(&scene);
    for tick in 0..1500 {
        scene.update(0.04, &data, &mut caches);
        let hull = scene.objects[scene.object_index(ship).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull;
        if hull < last {
            bites.push(tick);
        }
        last = hull;
    }
    assert!(bites.len() >= 3, "it bites: {bites:?}");
    for w in bites.windows(2) {
        let gap = w[1] - w[0];
        assert!(
            (93..=168).contains(&gap),
            "strikes apart: {gap} ({bites:?})"
        );
    }
    let spent = full - strength(&scene);
    let each = full * 0.05;
    assert!(
        (spent - each * bites.len() as f32).abs() < 1.0,
        "a twentieth a bite: {spent} for {}",
        bites.len()
    );
}

/// miss006's northern squadrons (members (2, 6) and (2, 7), 0x45ae70 and
/// 0x45af50) start with their ships; the first time one of (2, 6)'s is
/// struck, its ships join (2, 7) (0x45b280, message 0x5d9c) (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss006_squadrons_join_when_struck() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss006").unwrap();
    let six = scene.member_fleet(2, 6).unwrap();
    let seven = scene.member_fleet(2, 7).unwrap();
    let ships = |s: &Scene, k: usize| s.brain.as_ref().unwrap().fleets[k].ships.clone();
    let (a, b) = (ships(&scene, six), ships(&scene, seven));
    assert_eq!((a.len(), b.len()), (3, 4), "their ships");
    scene.brain_hit(a[0]);
    assert!(ships(&scene, six).is_empty());
    assert_eq!(ships(&scene, seven).len(), 7);
    // Once only.
    scene.brain_hit(b[0]);
    assert_eq!(ships(&scene, seven).len(), 7);
}

/// A settlement's stock repair (0x441ba0): nothing is mended till the
/// end; called off, the part not done comes back and the units whose time
/// fits are mended, least worn first, the first that does not fit ending
/// it (needs `OPENCONQUEST_DATA`).
#[test]
fn stock_repair_called_off() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let site = scene.own_sites()[0];
    // Bowmen at 90 and 50: 7 and 35 ticks to mend (70 x worn / 100).
    let mut units: [Vec<u8>; 9] = Default::default();
    units[0] = vec![50, 90];
    scene.set_stock_units(site, units);
    assert_eq!(scene.stock_units(site)[0], vec![90, 50], "least worn first");
    scene.stock_repairs.push(crate::scene::repair::StockRepair {
        site,
        elapsed: 0.0,
        seconds: crate::clock::ticks(42),
        paid: 18,
        progress: 0,
    });
    for _ in 0..20 {
        scene.update(0.04, &data, &mut caches);
    }
    let r = *scene.stock_repair_at(site).unwrap();
    assert_eq!(r.progress, 20 * 100 / 42);
    assert_eq!(
        scene.stock_units(site)[0],
        vec![90, 50],
        "nothing mended yet"
    );
    let amber = scene.amber;
    scene.cancel_stock_repair(site).unwrap();
    assert_eq!(scene.amber, amber + (100 - r.progress) * 18 / 100);
    // 20 ticks spent: the bowman at 90 (7 ticks) is mended, the one at 50
    // (35 more) is not.
    assert_eq!(scene.stock_units(site)[0], vec![100, 50]);
    assert!(scene.stock_repair_at(site).is_none());
}

/// A cell with something on its third place (a flock here) or a volcano's
/// ground takes no building (0x42dae0) (needs `OPENCONQUEST_DATA`).
#[test]
fn flocks_and_volcanoes_take_no_building() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut flocks, mut volcanoes) = (0, 0);
    for m in 0..14 {
        let scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).unwrap();
        for o in &scene.objects {
            let f = |k| o.record.field(k).unwrap_or(0) as i32;
            let cells: Vec<(i32, i32)> = match o.class {
                ObjectClass::Sheep => vec![(f(4), f(5))],
                ObjectClass::Lava => (0..4).map(|k| (f(6) + k % 2, f(7) + k / 2)).collect(),
                _ => continue,
            };
            for (x, y) in cells {
                assert!(
                    scene.check_ground(economy::SCHOOL, x, y).is_err(),
                    "miss{m:03} {:?} at {x},{y}",
                    o.class
                );
            }
            if o.class == ObjectClass::Sheep {
                flocks += 1;
            } else {
                volcanoes += 1;
            }
        }
    }
    assert!(flocks > 0 && volcanoes > 0);
}

/// The brains' members run as each mission's main runs them (message 0):
/// every 5 ticks of the game clock, member (g, m) when the tick is 5 x
/// (g + m) modulo 50, by the original's member index: miss002's Persian
/// yard is (1, 7) (0x44cbd0, at 40), miss003's southern guard (0, 5)
/// (0x44f050, at 25), miss005's raiders (1, 6) and (1, 7) (0x456580,
/// 0x456e80, at 35 and 40); members the switch has no case for never run
/// (miss008 (0, 0) and (1, 0)) (needs `OPENCONQUEST_DATA`).
#[test]
fn brain_members_run_at_their_phase() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    /// A member (group, member) and the tick modulo 50 it runs at.
    type Phase = (u8, usize, u32);
    let cases: [(&str, &[Phase]); 4] = [
        ("miss002", &[(1, 7, 40)]),
        ("miss003", &[(0, 5, 25)]),
        ("miss005", &[(1, 6, 35), (1, 7, 40)]),
        ("miss008", &[]),
    ];
    for (mission, members) in cases {
        let mut scene = Scene::load(&data, &mut caches, mission).unwrap();
        for _ in 0..2 * 120 {
            scene.update(0.02, &data, &mut caches);
        }
        let brain = scene.brain.as_ref().unwrap();
        assert_eq!(brain.tick, 120, "{mission}: the brain's tick is the game's");
        for &(g, m, phase) in members {
            let k = scene.member_fleet(g, m).unwrap();
            let since = scene.brain.as_ref().unwrap().fleets[k].since;
            assert!(since > 0, "{mission} ({g}, {m}) ran");
            assert_eq!(since % 50, phase, "{mission} ({g}, {m})");
        }
        if mission == "miss008" {
            for (g, m) in [(0, 0), (1, 0)] {
                let f = &scene.brain.as_ref().unwrap().fleets[scene.member_fleet(g, m).unwrap()];
                assert!(!f.started && f.since == 0, "miss008 ({g}, {m}) never runs");
            }
        }
    }
}

/// miss009's and miss010's brain tick (0x4665c9, 0x46a32a): two numbers
/// drawn every 5 ticks, the second, x, read twice, and the tick run when
/// the tick is ((x % 10 + x % 10) % 10) x 5 modulo 50 (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn brain_random_tick_draws_twice() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    let lcg = |s: u32| s.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
    let mut runs = [0u32; 50];
    for seed in 0..2000u32 {
        for tick in (0..50).step_by(5) {
            scene.brain.as_mut().unwrap().rng = crate::combat::Rng(seed);
            scene.brain.as_mut().unwrap().tick = tick;
            let s = lcg(lcg(seed));
            let x = s >> 16;
            let expect = tick % 50 == ((x % 10 + x % 10) % 10) * 5;
            assert_eq!(brain::random_tick(&mut scene), expect);
            assert_eq!(scene.brain.as_ref().unwrap().rng.0, s, "two draws");
            runs[tick as usize] += expect as u32;
        }
    }
    // Only on ticks a multiple of 10.
    for (tick, &n) in runs.iter().enumerate() {
        assert_eq!(n > 0, tick % 10 == 0, "tick {tick}");
    }
}

/// The attacked event (0x47a060) as miss013's member (2, 4) has it: the
/// first report of a blow waits, later ones are dropped; one from another
/// player is kept but never fires; one younger than the step's `after` is
/// dropped; the human's fires and puts `last_area`, member `+0x40`, about
/// the shooter where it is now: 2 cells about it, the corner not past 0,
/// the size 5 cut at the map's edge (0x4796c0) (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn brain_attacked_event_sets_last_area() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss013").unwrap();
    let k = scene.member_fleet(2, 4).unwrap();
    let fleet = |s: &Scene| s.brain.as_ref().unwrap().fleets[k].clone();
    assert_eq!(fleet(&scene).last_area, (0, 0, 0, 0));
    let target = fleet(&scene).ships[0];
    let mine = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let theirs = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && !scene.is_own(o) && o.id != target)
        .map(|o| o.id)
        .unwrap();
    let me = scene.objects[mine].id;
    let events = [brain::Event::Attacked {
        state: 30,
        after: 375,
    }];
    // Another player's report first: it stays, and keeps the human's out.
    scene.fleet_attacked(target, theirs);
    scene.fleet_attacked(target, me);
    assert_eq!(fleet(&scene).attacked_by, Some(theirs));
    assert_eq!(scene.event_fired(k, &events, 375), None);
    assert_eq!(fleet(&scene).attacked_by, Some(theirs));
    // Too early: dropped.
    scene.brain.as_mut().unwrap().fleets[k].attacked_by = None;
    scene.fleet_attacked(target, me);
    assert_eq!(scene.event_fired(k, &events, 374), None);
    assert_eq!(fleet(&scene).attacked_by, None);
    // The shooter at cell (1, 1): (0, 0, 5, 5).
    let t = TILE_SIZE as f32;
    let at = |s: &mut Scene, x: i32, y: i32| {
        s.objects[mine].x = x as f32 * t + t / 2.0;
        s.objects[mine].y = y as f32 * t + t / 2.0;
    };
    at(&mut scene, 1, 1);
    scene.fleet_attacked(target, me);
    assert_eq!(scene.event_fired(k, &events, 375), Some(30));
    assert_eq!(fleet(&scene).last_area, (0, 0, 5, 5));
    assert_eq!(fleet(&scene).attacked_by, None);
    // By the far corner: cut to the map.
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    at(&mut scene, w - 2, h - 1);
    scene.fleet_attacked(target, me);
    assert_eq!(scene.event_fired(k, &events, 375), Some(30));
    assert_eq!(fleet(&scene).last_area, (w - 4, h - 3, 4, 3));
}

/// The score counts what the human player destroys (0x4a9250): monsters by
/// class (a dragon, 0x1f9, 1100 points); a dragon's nest (0x44) is a
/// monsters' terrain object worth nothing and no one's loss; another
/// player's kill and the human's own thing destroyed by the human count
/// nothing (needs `OPENCONQUEST_DATA`).
#[test]
fn score_counts_the_human_players_kills() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss010").unwrap();
    scene.brain = None;
    let human = scene.human.map(|h| (h.nation, h.slot));
    let find = |s: &Scene, class: u32| {
        s.objects
            .iter()
            .find(|o| o.record.class_id == class && o.combat.is_some())
            .map(|o| o.id)
            .unwrap()
    };
    let dragon = find(&scene, 0x1f9);
    scene.hit(dragon, f32::MAX, human, &data, &mut caches);
    assert_eq!(scene.stats.monsters_killed[0x1f9 - 0x1f4], 1);
    assert_eq!(scene.stats.score(), 1100 / 10);
    let nest = find(&scene, 0x44);
    scene.hit(nest, f32::MAX, human, &data, &mut caches);
    assert_eq!(scene.stats.lairs_destroyed, 1);
    assert_eq!(scene.stats.buildings_lost, 0);
    assert_eq!(scene.stats.score(), 1100 / 10);
    let v = scene.stats.values(scene.amber);
    assert_eq!((v[2], v[3], v[4]), (0, 1, 1));
    // Another player's kill.
    let enemy = scene
        .objects
        .iter()
        .find(|o| {
            o.ship.is_some()
                && !scene.is_own(o)
                && o.owner.is_some_and(|w| w != crate::combat::MONSTERS)
        })
        .map(|o| o.id)
        .unwrap();
    let owner = scene.objects[scene.object_index(enemy).unwrap()].owner;
    let other = scene
        .objects
        .iter()
        .find(|o| o.owner.is_some() && o.owner != owner && o.owner != human)
        .and_then(|o| o.owner);
    scene.hit(enemy, f32::MAX, other, &data, &mut caches);
    assert_eq!(scene.stats.ships_destroyed.iter().sum::<u32>(), 0);
    // The human's own ship sunk by the human: neither lost nor destroyed.
    let mine = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .map(|o| o.id)
        .unwrap();
    scene.hit(mine, f32::MAX, human, &data, &mut caches);
    assert_eq!(scene.stats.ships_lost.iter().sum::<u32>(), 0);
    assert_eq!(scene.stats.score(), 1100 / 10);
}

/// Order 10 (0x280f, order byte 10): a building's own mending stops with
/// nothing given back (LandObj 0x425990, job 0xff); a settlement's stock
/// repair is called off (TownObj 0x441ba0 -> 0x4430f0), also through its
/// workshop (Studio 0x436ee0, job 5); a fort's mending is left (not
/// ported). Needs `OPENCONQUEST_DATA`.
#[test]
fn order_10_calls_off_what_the_object_does() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.amber = 10_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap()
        .id;
    let h = scene.object_index(home).unwrap();
    let c = scene.objects[h].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    scene.order_mend(home, home).unwrap();
    scene.update(crate::clock::TICK * 10.0, &data, &mut caches);
    assert!(scene.mending.contains(&home));
    let amber = scene.amber;
    scene
        .apply(&Order::Halt { id: home }, &data, &mut caches)
        .unwrap();
    assert!(!scene.mending.contains(&home));
    assert_eq!(scene.amber, amber);
    // Nothing to call off: nothing happens.
    scene
        .apply(&Order::Halt { id: home }, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.amber, amber);
    // The stock repair, through the workshop: (100 - progress) x paid /
    // 100 back.
    let w = scene
        .objects
        .iter()
        .position(|o| {
            scene.is_own(o) && matches!(o.class, ObjectClass::Building(_)) && o.id != home
        })
        .unwrap();
    scene.objects[w].class = ObjectClass::Building(economy::WORKSHOP);
    let workshop = scene.objects[w].id;
    assert_eq!(scene.home_settlement(w), Some(home));
    scene.set_stock_units(home, {
        let mut u: [Vec<u8>; 9] = Default::default();
        u[6] = vec![50, 100];
        u
    });
    let amber = scene.amber;
    scene.order_repair_stock(workshop).unwrap();
    assert_eq!(amber - scene.amber, 35);
    scene
        .apply(&Order::Halt { id: workshop }, &data, &mut caches)
        .unwrap();
    assert!(scene.stock_repair_at(home).is_none());
    assert_eq!(scene.amber, amber);
    // On the settlement itself too.
    scene.order_repair_stock(workshop).unwrap();
    scene
        .apply(&Order::Halt { id: home }, &data, &mut caches)
        .unwrap();
    assert!(scene.stock_repair_at(home).is_none());
    assert_eq!(scene.amber, amber);
}

/// A merchant unloading at a port lies held, move state 6, each tick
/// (0x4adef0 step 0x11); leaving, it lies still (0x4a5b70) and moves a
/// cell off the berth with priority 91 (0x4b46f0 with '['; needs
/// `OPENCONQUEST_DATA`).
#[test]
fn merchant_unloading_at_a_port_is_held() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut found = None;
    for m in [
        "miss000", "miss001", "miss002", "miss003", "battle01", "battle02",
    ] {
        let scene = Scene::load(&data, &mut caches, m).unwrap();
        let port = scene
            .objects
            .iter()
            .position(|o| o.class == ObjectClass::Building(PORT) && o.owner.is_some());
        if let Some(p) = port {
            found = Some((m, scene, p));
            break;
        }
    }
    let (_, mut scene, p) = found.expect("a mission with a port");
    let owner = scene.objects[p].owner;
    let port = scene.objects[p].id;
    let k = scene
        .objects
        .iter()
        .position(|o| o.owner == owner && o.ship.as_ref().is_some_and(|s| s.kind <= 1))
        .or_else(|| scene.objects.iter().position(|o| o.ship.is_some()))
        .unwrap();
    let id = scene.objects[k].id;
    scene.objects[k].owner = owner;
    let berth = scene.front_cell(scene.object_index(port).unwrap()).unwrap();
    let t = TILE_SIZE as f32;
    scene.objects[k].x = berth.0 as f32 * t + t / 2.0;
    scene.objects[k].y = berth.1 as f32 * t + t / 2.0;
    {
        let s = scene.objects[k].ship.as_mut().unwrap();
        s.path.clear();
        s.cargo = 50.0;
        s.cargo_kind = Some(crate::objects::Resource::Fish);
        s.move_state = 0;
        s.task = Some(Task::Gather {
            deposit: u32::MAX,
            phase: crate::objects::GatherPhase::Unloading(port),
        });
    }
    scene.update(crate::clock::TICK, &data, &mut caches);
    let k = scene.object_index(id).unwrap();
    let s = scene.objects[k].ship.as_ref().unwrap();
    assert_eq!(s.move_state, 6);
    let mut ticks = 0;
    while scene.objects[scene.object_index(id).unwrap()]
        .ship
        .as_ref()
        .unwrap()
        .task
        .is_some()
    {
        scene.update(crate::clock::TICK, &data, &mut caches);
        ticks += 1;
        assert!(ticks < 2000);
    }
    let k = scene.object_index(id).unwrap();
    let s = scene.objects[k].ship.as_ref().unwrap();
    assert_ne!(s.move_state, 6);
    assert!(s.priority >= 91);
    let goal = s.path.back().copied().expect("a cell off the berth");
    let goal = ((goal.0 / t) as i32, (goal.1 / t) as i32);
    assert!(goal != berth && (goal.0 - berth.0).abs() <= 2 && (goal.1 - berth.1).abs() <= 2);
}

/// A mission's ship made on a cell an earlier one holds, or one not fit
/// for its draught, is removed and counted lost to its owner (0x4dc787;
/// needs `OPENCONQUEST_DATA`).
#[test]
fn misplaced_mission_ships_are_removed() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let own: Vec<usize> = (0..scene.objects.len())
        .filter(|&i| scene.is_own(&scene.objects[i]) && scene.objects[i].ship.is_some())
        .take(2)
        .collect();
    let (first, second) = (scene.objects[own[0]].id, scene.objects[own[1]].id);
    let kind = scene.objects[own[1]].ship.as_ref().unwrap().kind as usize;
    (scene.objects[own[1]].x, scene.objects[own[1]].y) =
        (scene.objects[own[0]].x, scene.objects[own[0]].y);
    let lost = scene.stats.ships_lost[kind];
    assert_eq!(scene.drop_misplaced_ships(), vec![second]);
    assert!(scene.object_index(first).is_some());
    assert_eq!(scene.stats.ships_lost[kind], lost + 1);
}

/// A new game's brain starts its own generator from 0 (0x4788a0), but in
/// miss007-010 from the clock (needs `OPENCONQUEST_DATA`).
#[test]
fn brain_seed_follows_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss006").unwrap();
    let mut fresh = Scene::load(&data, &mut caches, "miss006").unwrap();
    // The same draws from 0, game after game; the first is 0x3039 >> 16.
    fresh.brain.as_mut().unwrap().rng = crate::combat::Rng(0);
    assert_eq!(fresh.brain_draw(), 0);
    for _ in 0..200 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let a = scene.brain.as_ref().unwrap().rng.0;
    let mut again = Scene::load(&data, &mut caches, "miss006").unwrap();
    for _ in 0..200 {
        again.update(crate::clock::TICK, &data, &mut caches);
    }
    assert_eq!(again.brain.as_ref().unwrap().rng.0, a);
    // Saved with the brain.
    let save = again.save();
    assert_eq!(save.brain.as_ref().unwrap().rng.0, a);
}

/// The repair members of miss002-miss005 sit where the original makes
/// them (0x479200 at 0x8d1, member order), and the members after them
/// move up (needs `OPENCONQUEST_DATA`).
#[test]
fn repair_members_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    for (mission, members) in [
        ("miss002", &[(1, 8, 9)][..]),
        ("miss003", &[(0, 4, 4), (0, 5, 5), (1, 0, 6), (1, 4, 10)]),
        (
            "miss004",
            &[(0, 6, 6), (1, 0, 7), (1, 3, 10), (2, 2, 13), (2, 4, 15)],
        ),
        ("miss005", &[(1, 5, 9), (1, 6, 10), (1, 7, 11)]),
    ] {
        let scene = Scene::load(&data, &mut caches, mission).unwrap();
        for &(g, m, k) in members {
            assert_eq!(scene.member_fleet(g, m), Some(k), "{mission} ({g}, {m})");
        }
        let brain = scene.brain.as_ref().unwrap();
        match mission {
            // The barbarian yard's rest and the raids' waits land on their
            // members: miss003's yard (1, 0) rests 750 on normal.
            "miss003" => {
                assert_eq!(brain.fleets[6].vars[0], 750);
                assert!(brain.fleets[10].vars[0] >= 9 * 1500);
            }
            "miss004" => {
                assert_eq!(brain.fleets[10].vars, [7500, 3, 0, 0]);
                assert_eq!(brain.fleets[13].vars[1], 2);
            }
            "miss005" => {
                assert_eq!(brain.fleets[10].vars[1], 5);
                assert_eq!(brain.fleets[11].vars[1], 3);
            }
            _ => {}
        }
    }
}

/// A brain saved before the repair members were added lines up again on
/// restore: the members go in empty at their places and the fleet
/// indices it holds move up (needs `OPENCONQUEST_DATA`).
#[test]
fn old_brains_get_their_repair_members() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let scene = Scene::load(&data, &mut caches, "miss004").unwrap();
    let mut old = scene.brain.clone().unwrap();
    let new = old.clone();
    old.fleets.remove(15);
    old.fleets.remove(6);
    old.layout = 0;
    old.requests = vec![(5, 2), (6, 2), (13, 4)];
    old.upgrade();
    assert_eq!(old.layout, super::brain::LAYOUT);
    assert_eq!(old.fleets.len(), new.fleets.len());
    assert_eq!(old.requests, vec![(5, 2), (7, 2), (14, 4)]);
    for k in 0..new.fleets.len() {
        assert_eq!(old.fleets[k].ships, new.fleets[k].ships, "fleet {k}");
    }
}

/// The repair members' order 0x82 (0x47a7e0, 0x4cdf80): miss003's (0, 4)
/// picks its start dock from the player's fleet 0, a ship fleet, so has
/// none; a prize of player 15 joins it and is sent to the first dock of
/// the yard's town at (58, 1) (fleet 3; 0x4cdea0 at the join, 0x4cdb00
/// to `+0x10`). miss005's (1, 5) has fleet 0 for its yard: its start dock
/// is the first of the town at (8, 11) (needs `OPENCONQUEST_DATA`).
#[test]
fn repair_orders_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let town_dock = |scene: &Scene, (x, y): (i32, i32)| {
        let town = scene
            .objects
            .iter()
            .find(|o| {
                matches!(
                    o.class,
                    ObjectClass::Building(economy::SETTLEMENT | economy::TOWN)
                ) && o
                    .cell
                    .is_some_and(|(cx, cy)| (x..x + 2).contains(&cx) && (y..y + 2).contains(&cy))
            })
            .map(|o| o.id)
            .expect("the yard's town");
        let docks: Vec<u32> = (0..scene.objects.len())
            .filter(|&d| {
                scene.objects[d].class == ObjectClass::Building(economy::DOCK)
                    && scene.home_settlement(d) == Some(town)
            })
            .map(|d| scene.objects[d].id)
            .collect();
        assert_eq!(docks.len(), 1, "one dock to the town");
        docks[0]
    };
    let mut scene = Scene::load(&data, &mut caches, "miss003").unwrap();
    // (0, 4) behaves at tick 20 and gives the order at its next turn, 70.
    for _ in 0..70 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let f = &scene.brain.as_ref().unwrap().fleets[4];
    assert_eq!(f.state, 10);
    let order = f.repair.clone().expect("the repair order");
    assert_eq!((order.dock, order.yard), (0, 3));
    let dock = town_dock(&scene, (58, 1));
    let ship = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let c = scene.objects[ship].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    let id = scene.objects[ship].id;
    let crew = scene.objects[ship].ship.as_ref().unwrap().crew;
    let units = scene.objects[ship].unit_conditions();
    let marines = [units[0].clone(), units[1].clone(), units[2].clone()];
    scene.capture(id, Some((3, 3)), crew.class, marines, &data, &mut caches);
    assert!(scene.brain.as_ref().unwrap().fleets[4].ships.contains(&id));
    let i = scene.object_index(id).unwrap();
    assert!(scene.objects[i].ship.as_ref().unwrap().task.is_none());
    // Heard at the order's next tick.
    scene.update(crate::clock::TICK, &data, &mut caches);
    let i = scene.object_index(id).unwrap();
    assert_eq!(
        scene.objects[i].ship.as_ref().unwrap().task,
        Some(Task::Repair { dock })
    );
    assert_eq!(
        scene.brain.as_ref().unwrap().fleets[4]
            .repair
            .as_ref()
            .unwrap()
            .dock,
        dock
    );

    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    // (1, 5) runs at 30, then 80.
    for _ in 0..80 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let order = scene.brain.as_ref().unwrap().fleets[9]
        .repair
        .clone()
        .expect("the repair order");
    assert_eq!((order.dock, order.yard), (town_dock(&scene, (8, 11)), 0));
}

/// miss008's Persian patrols cast on the first call of their fights
/// (0x47acf0 by the land fleet (1, 2), paid): with no war temple nothing is
/// cast, but each call with ships in the target credits player 15 a
/// cast's price before the temple is looked for: member 5 the weapon
/// shield at 30 (1000 amber, 100 fish), member 6 the Flame at 20 (6000,
/// 1500), member 10 the Flame on the strike's ships at 40, member 11 the
/// phantom at 20 (1000, 100); a later call of the same state, or a target
/// without ships, credits nothing (needs `OPENCONQUEST_DATA`).
#[test]
fn miss008_patrol_casts_credit_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss008").unwrap();
    let persians = (3, 3);
    assert!(
        !scene
            .objects
            .iter()
            .any(|o| o.class == ObjectClass::Building(economy::TEMPLE_OF_WAR)),
        "no war temple"
    );
    // The caster, the land fleet (1, 2) at (58, 23), is player 15's.
    let caster = scene.brain.as_ref().unwrap().fleets[9].ships[0];
    let i = scene.object_index(caster).unwrap();
    assert_eq!(scene.objects[i].owner, Some(persians));
    // The Persians start with no ships: one of the player's, made theirs,
    // stands in each target fleet.
    let i = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    scene.objects[i].owner = Some(persians);
    let ship = scene.objects[i].id;
    // (fleet, state, target, its turn's tick modulo 50, credit).
    for (k, state, target, phase, credit) in [
        (12, 30, 12, 30, (1000, 100)),
        (13, 20, 13, 35, (6000, 1500)),
        (17, 40, 14, 5, (6000, 1500)),
        (18, 20, 18, 10, (1000, 100)),
    ] {
        let tick = scene.brain.as_ref().unwrap().tick;
        let next = tick + (phase + 50 - tick % 50) % 50;
        while scene.brain.as_ref().unwrap().tick + 1 < next {
            scene.update(crate::clock::TICK, &data, &mut caches);
        }
        let brain = scene.brain.as_mut().unwrap();
        for f in &mut brain.fleets {
            f.ships.retain(|&s| s != ship);
        }
        brain.fleets[target].ships.push(ship);
        brain.fleets[k].state = state;
        brain.fleets[k].started = false;
        let (a, f) = scene.funds(persians);
        scene.update(crate::clock::TICK, &data, &mut caches);
        assert_eq!(scene.brain.as_ref().unwrap().tick % 50, phase);
        let (a2, f2) = scene.funds(persians);
        assert_eq!(
            (a2 as i64 - a as i64, f2 as i64 - f as i64),
            (credit.0, credit.1),
            "fleet {k} at {state}"
        );
    }
    // Member 5 again at 30, its step begun: no cast.
    let tick = scene.brain.as_ref().unwrap().tick;
    let next = tick + 1 + (30 + 50 - (tick + 1) % 50) % 50;
    let brain = scene.brain.as_mut().unwrap();
    for f in &mut brain.fleets {
        f.ships.retain(|&s| s != ship);
    }
    brain.fleets[12].ships.push(ship);
    brain.fleets[12].state = 30;
    brain.fleets[12].started = true;
    brain.fleets[12].since = brain.tick;
    while scene.brain.as_ref().unwrap().tick + 1 < next {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let (a, f) = scene.funds(persians);
    scene.update(crate::clock::TICK, &data, &mut caches);
    assert_eq!(scene.brain.as_ref().unwrap().tick % 50, 30);
    assert_eq!(scene.funds(persians), (a, f));
}

/// miss009's Persian patrol (member 3) casts the Flame on its ships by the
/// land fleet (1, 0) on the first call of its fight in the north, paid,
/// difficulty + 3 times (`+0x12e`, 0x465b20); then no more (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss009_patrol_flames_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    let persians = (3, 2);
    // Member (1, 3), fleet 11, runs at 20 modulo 50; normal: 4 Flames.
    assert_eq!(scene.brain.as_ref().unwrap().fleets[11].vars[0], 4);
    let ship = scene
        .objects
        .iter()
        .find(|o| o.owner == Some(persians) && o.ship.is_some())
        .map(|o| o.id)
        .expect("a Persian ship");
    // The war temple at (57, 44) brews each Flame, paid by the credit.
    let mut brewed = Vec::new();
    for _ in 0..5 {
        let tick = scene.brain.as_ref().unwrap().tick;
        let next = tick + 1 + (20 + 50 - (tick + 1) % 50) % 50;
        while scene.brain.as_ref().unwrap().tick + 1 < next {
            scene.update(crate::clock::TICK, &data, &mut caches);
        }
        let brain = scene.brain.as_mut().unwrap();
        for f in &mut brain.fleets {
            f.ships.retain(|&s| s != ship);
        }
        brain.fleets[11].ships.push(ship);
        brain.fleets[11].state = 20;
        brain.fleets[11].started = false;
        scene.brain_flames.clear();
        let funds = scene.funds(persians);
        scene.update(crate::clock::TICK, &data, &mut caches);
        assert_eq!(scene.brain.as_ref().unwrap().tick % 50, 20);
        let brewing = scene.brain_flames.get(&persians).is_some_and(|f| f.0 > 0.0);
        brewed.push((brewing, scene.funds(persians) == funds));
    }
    assert_eq!(scene.brain.as_ref().unwrap().fleets[11].vars[0], 0);
    assert_eq!(
        brewed,
        [
            (true, true),
            (true, true),
            (true, true),
            (true, true),
            (false, true)
        ]
    );
}

/// The brains' pick of one of the player's settlements or towns
/// (0x45599a): settlements then towns, each row by row in the rect, one
/// brain draw; the area 0x421740's rect (the anchor, 2 x 2, grown to take
/// in its buildings' anchors) grown by 2. From seed 0 the draw is
/// 0x3039 >> 16 = 0: the first settlement. Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn brain_picks_a_site_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    scene.brain.as_mut().unwrap().rng.0 = 0;
    let rect = (0, 0, 35, 35);
    let inside = |(x, y): (i32, i32)| x < 35 && y < 35;
    let first = scene
        .objects
        .iter()
        .filter(|o| {
            scene.is_own(o) && o.combat.is_some() && o.class == ObjectClass::Building(SETTLEMENT)
        })
        .filter_map(|o| o.cell.filter(|&c| inside(c)).map(|c| (o.id, c)))
        .min_by_key(|&(_, (x, y))| (y, x))
        .or_else(|| {
            scene
                .objects
                .iter()
                .filter(|o| {
                    scene.is_own(o) && o.combat.is_some() && o.class == ObjectClass::Building(TOWN)
                })
                .filter_map(|o| o.cell.filter(|&c| inside(c)).map(|c| (o.id, c)))
                .min_by_key(|&(_, (x, y))| (y, x))
        })
        .expect("a site of the player's in the west");
    let (sx, sy, sw, sh) = scene.site_rect(first.0).unwrap();
    assert!((sx, sy) <= first.1 && sw >= 2 && sh >= 2);
    let area = scene.human_site_area(Some(rect)).unwrap();
    assert_eq!(area, scene.area_about((sx, sy, sw, sh), 2));
    assert_eq!(
        scene.brain.as_ref().unwrap().rng.0,
        0x3039,
        "one brain draw"
    );
}

/// The buildings' voice lists are the game's `SND_n` entries by nation
/// (events base, base + 1, base + 3, base + 2). Needs `OPENCONQUEST_DATA`.
#[test]
fn building_calls_are_the_sound_entries() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let archive = data.sounds.as_ref().unwrap();
    use super::halt::*;
    for (calls, base) in [
        (&TOWN_CALLS, 220),
        (&SMITHY_CALLS, 232),
        (&STUDIO_CALLS, 236),
        (&SCHOOL_CALLS, 240),
        (&WAR_TEMPLE_CALLS, 248),
        (&FINISHED_CALLS, 188),
    ] {
        for (nation, add) in [0, 1, 3, 2].into_iter().enumerate() {
            let names = openconquest_formats::sound::sound_choices(archive, base + add).unwrap();
            assert_eq!(calls[nation], names.as_slice(), "event {}", base + add);
        }
    }
}

/// Land creatures look about, keep jams and throw as the original's: two
/// cyclopes put on one cell share its jam (0x1498, 0x1879f), in the order
/// they came; the first's look about (0x412290) takes the nearest cell of
/// a building in its mask (priority `10 - d`) and tells the jam, whose
/// other member pursues that cell (0x187a2, the absolute cell as a half);
/// standing, it goes to throw (0x412740), turns by the signs to the
/// target, starts at once (`+0x138` 0) and throws at its picture's frame
/// 0xb, once, hitting as its own draws say. Needs `OPENCONQUEST_DATA`.
#[test]
fn land_creatures_look_alert_and_throw() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    let foot = |o: &crate::objects::MapObject| {
        let (bx, by) = o.cell.unwrap();
        let n = super::trade::building_cells(o);
        (by..by + n).flat_map(move |y| (bx..bx + n).map(move |x| (x, y)))
    };
    // A building with a free land cell 3 off it and nothing else that
    // stands within 7 cells of that one.
    let pick = |scene: &Scene| {
        let w = scene.grid.width as i32;
        let h = scene.grid.height as i32;
        let land = |x: i32, y: i32| {
            x >= 0 && y >= 0 && x < w && y < h && scene.cell_types[(y * w + x) as usize] & 0xf == 8
        };
        scene
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Building(0x32..=0x3e)) && o.cell.is_some())
            .filter(|o| {
                o.combat
                    .as_ref()
                    .is_some_and(|c| c.hull > 0.0 && c.phantom <= 0.0)
            })
            .find_map(|b| {
                let (bx, by) = b.cell.unwrap();
                let n = super::trade::building_cells(b);
                [
                    (bx - 3, by),
                    (bx + n + 2, by),
                    (bx, by - 3),
                    (bx, by + n + 2),
                ]
                .into_iter()
                .find(|&cell| {
                    let near =
                        |c: (i32, i32)| (c.0 - cell.0).abs() <= 7 && (c.1 - cell.1).abs() <= 7;
                    land(cell.0, cell.1)
                        && scene.objects.iter().all(|o| {
                            o.id == b.id
                                || match (o.cell, &o.ship) {
                                    (_, Some(_)) => !near(((o.x / t) as i32, (o.y / t) as i32)),
                                    (Some(_), None) => !foot(o).any(near),
                                    _ => true,
                                }
                        })
                })
                .map(|cell| (b.id, cell))
            })
    };
    let (mut scene, (bid, cell)) = (0..14)
        .find_map(|m| {
            let mut scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).ok()?;
            scene.brain = None;
            let found = pick(&scene)?;
            Some((scene, found))
        })
        .expect("a building with room beside it");
    scene.put_walker(super::walkers::NO_LAIR, 0x1f4, cell, &data, &mut caches);
    scene.put_walker(super::walkers::NO_LAIR, 0x1f4, cell, &data, &mut caches);
    let (a, b) = (scene.walkers[0].id, scene.walkers[1].id);
    assert_eq!(scene.jams.len(), 1);
    assert_eq!(scene.jams[0].cell, cell);
    assert_eq!(scene.jams[0].members, vec![a, b]);
    assert_eq!((scene.jams[0].id, b), (a + 1, a + 2));
    // The expected target: the building's cell of the highest priority,
    // the first in row order among equals.
    let bi = scene.object_index(bid).unwrap();
    let mut best: Option<((i32, i32), u32)> = None;
    for c in foot(&scene.objects[bi]) {
        let (dx, dy) = ((c.0 - cell.0) as i64, (c.1 - cell.1) as i64);
        let p = 10 - super::fort::root((dx * dx + dy * dy) as u64);
        if best.is_none_or(|(_, q)| p > q) {
            best = Some((c, p));
        }
    }
    let (target_cell, prio) = best.unwrap();
    scene.ticks = 100;
    for w in &mut scene.walkers {
        (w.state, w.scan, w.wake) = (super::walkers::STAND, 0, 0);
        w.frame = w.first;
    }
    // The second stands by, to be seen as told.
    (scene.walkers[1].scan, scene.walkers[1].wake) = (u32::MAX, u32::MAX);
    scene.ticks += 1;
    scene.tick_walkers(&data, &mut caches);
    let (wa, wb) = (&scene.walkers[0], &scene.walkers[1]);
    assert!(wa.target);
    assert_eq!(
        (wa.target_cell, wa.prio, wa.target_id),
        (target_cell, prio, bid)
    );
    assert!(!wb.target);
    assert!(wb.pursue && wb.crowded);
    assert_eq!((wb.pursue_to, wb.prio), (target_cell, prio));
    // It went to throw on its step this tick; it starts on the next and
    // throws at frame 0xb, twelve ticks on, or thirteen when it turned
    // (its picture set again is first drawn without a step, `+0xa8`).
    assert_eq!(scene.walkers[0].state, super::walkers::THROW);
    let entered = scene.ticks;
    let before = scene.walkers[0].heading;
    let mut thrown_at = None;
    let mut expect = None;
    for _ in 0..30 {
        let w = &scene.walkers[0];
        if w.throwing && w.frame == 0xb && !w.thrown {
            // The step's draw, the cry's, then the miss draw beyond 1 cell.
            let mut r = crate::combat::Rng(w.rng);
            r.next();
            r.next();
            let (dx, dy) = (
                (target_cell.0 - cell.0) as i64,
                (target_cell.1 - cell.1) as i64,
            );
            let d = super::fort::root((dx * dx + dy * dy) as u64);
            expect = Some(!(d > 1 && r.next() % (8 - d) == 0));
        }
        scene.walkers[1].scan = u32::MAX;
        scene.ticks += 1;
        scene.tick_walkers(&data, &mut caches);
        if !scene.throws.is_empty() && thrown_at.is_none() {
            thrown_at = Some(scene.ticks);
        }
    }
    let turned = u32::from(scene.walkers[0].heading != before);
    assert_eq!(thrown_at, Some(entered + 12 + turned));
    assert_eq!(scene.throws.len(), 1, "once a throw");
    let throw = scene.throws[0];
    assert_eq!((throw.target, throw.shooter, throw.weapon), (bid, a, 6));
    assert_eq!(Some(throw.strikes), expect);
    let dx = (target_cell.0 - cell.0).signum();
    let dy = (target_cell.1 - cell.1).signum();
    let heading = super::walk_path::DIRS
        .iter()
        .rposition(|&(x, y)| x.signum() == dx && y.signum() == dy)
        .unwrap();
    assert_eq!(scene.walkers[0].heading as usize, heading);
    // It leaves from its picture's top left as 0x412a8d takes it (`+0xb4
    // - w / 2`, `+0xb8 - h`, the sequence's first frame's size) and the
    // cyclops' spot for its heading (0x594ae0); at a building it aims as
    // the port's other shots (0x423520 not ported).
    let names = caches
        .sequence(&data, &format!("CLPSPR_3{heading}"))
        .unwrap();
    let raw = data
        .objects
        .read(
            &names[0],
            openconquest_formats::archive::ResourceKind::Sprite,
        )
        .unwrap();
    let dword = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap()) as i32;
    let (w, h) = (dword(4), dword(8));
    let spot = [
        (30, 14),
        (44, 11),
        (51, 6),
        (44, 0),
        (31, 0),
        (18, 0),
        (11, 4),
        (16, 11),
    ][heading];
    let pos = scene.walkers[0].pos;
    assert_eq!(throw.from, (pos.0 - w / 2 + spot.0, spot.1 - h + pos.1));
    assert_eq!(throw.to, None);
}

/// A shot at a ship aims where its vt+0xc (0x4e07c0) says, drawing from
/// the ship's own generator: a hit at its hull's spot for its type and
/// heading (0x598ad8) moved `r % 20 - 9` along its heading, led by its
/// way under way; a miss by its picture's edge (`r % 6` fifths, the side
/// by a second draw). Needs `OPENCONQUEST_DATA`.
#[test]
fn shots_aim_at_ships_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let k = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
    let (left, top, w) = {
        let o = &scene.objects[k];
        let l = &o.frames[0][0];
        let (x, y) = crate::render::layer_origin(o, l);
        (x, y, l.image.width as i32)
    };
    let seed = 0x1234_5678;
    let draws = |n: usize| {
        let mut r = crate::combat::Rng(seed);
        (0..n).map(|_| r.next()).collect::<Vec<u32>>()
    };
    let set = |scene: &mut Scene, heading: usize, state: u8, velocity: f32| {
        let s = scene.objects[k].ship.as_mut().unwrap();
        (s.heading, s.move_state, s.velocity, s.lcg) = (heading, state, velocity, Some(seed));
    };
    // A hit lying still, heading 2 (north) on a type-0 hull.
    scene.objects[k].ship.as_mut().unwrap().kind = 0;
    set(&mut scene, 2, 0, 0.0);
    let r = draws(1)[0];
    let k9 = (r % 20) as i32 - 9;
    assert_eq!(scene.ship_aim(k, true), (26 + left, -k9 + 21 + top));
    // Under way (state 5) the hit leads by its way (`+0x59b`, here 20000
    // in 1/6000 pixel a tick) x the heading x 15 / 100000: 3 pixels east.
    let velocity = 20000.0 / (crate::clock::TICK * 6000.0);
    set(&mut scene, 0, 5, velocity);
    let way = (velocity * crate::clock::TICK * 6000.0) as i32;
    assert_eq!(
        scene.ship_aim(k, true),
        (k9 + 40 + left + way * 15 / 100000, 26 + top)
    );
    assert_eq!(way * 15 / 100000, 3);
    // A miss heading 2: three quarters of the width or an eighth across,
    // `u` fifths of the width down.
    set(&mut scene, 2, 0, 0.0);
    let d = draws(2);
    let u = (d[0] % 6) as i32;
    let x = if d[1] & 1 == 0 { w - w / 8 } else { w / 8 };
    assert_eq!(scene.ship_aim(k, false), (left + x, top + w * u / 5));
}

/// A land creature's picture stands up and left of its place by its
/// class's offset (0x4111b0: the cyclops' 0x1e, 0x30); the pointer picks
/// its jam, given as the first member, when it is on any member's picture
/// rectangle (0x49a580, 0x415c70, 0x411b50), not on a dying one, and the
/// cells about the pointer's are tried too (needs `OPENCONQUEST_DATA`).
#[test]
fn the_pointer_picks_a_land_creatures_jam() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    // A land cell with land all about and nothing standing on or near it.
    let (mut scene, cell) = (0..14)
        .find_map(|m| {
            let mut scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).ok()?;
            scene.brain = None;
            let (w, h) = (scene.grid.width as i32, scene.grid.height as i32);
            let land = |x: i32, y: i32| scene.cell_types[(y * w + x) as usize] & 0xf == 8;
            let cell = (2..h - 2)
                .flat_map(|y| (2..w - 2).map(move |x| (x, y)))
                .find(|&(x, y)| {
                    (-2..=2).all(|dy| (-2..=2).all(|dx| land(x + dx, y + dy)))
                        && scene
                            .objects
                            .iter()
                            .filter(|o| o.combat.is_some() || o.cell.is_some())
                            .all(|o| {
                                let (ox, oy) = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
                                (ox - x).abs() > 3 || (oy - y).abs() > 3
                            })
                })?;
            Some((scene, cell))
        })
        .expect("a free land cell");
    scene.put_walker(super::walkers::NO_LAIR, 0x1f4, cell, &data, &mut caches);
    scene.put_walker(super::walkers::NO_LAIR, 0x1f4, cell, &data, &mut caches);
    let (a, b) = (scene.walkers[0].id, scene.walkers[1].id);
    // Put the second a half off, so its picture reaches other pixels.
    let pos = scene.walkers[0].pos;
    scene.walkers[1].pos = (pos.0 + 32, pos.1);
    let i = scene.object_index(b).unwrap();
    (scene.objects[i].x, scene.objects[i].y) = (pos.0 as f32 + 32.0, pos.1 as f32);
    let k = scene.object_index(a).unwrap();
    let l = &scene.objects[k].frames[0][0];
    let (fw, fh) = (l.image.width as i32, l.image.height as i32);
    assert_eq!(
        crate::render::layer_origin(&scene.objects[k], l),
        (pos.0 - 0x1e, pos.1 - 0x30)
    );
    let pick = |scene: &Scene, x: i32, y: i32| {
        crate::render::hit_test(scene, x as f32 + 0.5, y as f32 + 0.5, 0)
    };
    // The first's picture's corners pick the jam; just outside, nothing
    // of it (the second's lies a half to the east).
    let (left, top) = (pos.0 - 0x1e, pos.1 - 0x30);
    assert_eq!(pick(&scene, left, top), Some(a));
    assert_eq!(pick(&scene, left, top + fh - 1), Some(a));
    assert_ne!(pick(&scene, left - 1, top), Some(a));
    assert_ne!(pick(&scene, left, top - 1), Some(a));
    // The second's far side: still the jam, as its first member.
    assert_eq!(pick(&scene, left + 32 + fw - 1, top), Some(a));
    assert_ne!(pick(&scene, left + 32 + fw, top), Some(a));
    // A dying member is not on it.
    scene.walkers[1].state = super::walkers::DYING;
    assert_ne!(pick(&scene, left + 32 + fw - 1, top), Some(a));
    assert!(scene.pick_cells(left, top).len() == 9);
}

/// A blow to a land creature waits for its tick (0x4139e0, 0x413ad0): one
/// that leaves it life takes it off (30000 - 10000, 66 percent) and shows
/// the hit when its picture next ends (state 4, its 10 frames, then back);
/// one that does not leaves it dying at life 1 (state 5) with its cry,
/// removed at the dying picture's end (a cyclops' 16 frames at 2 ticks, a
/// minotaur's 20 at 2 then 4 past frame 0xb), leaving its jam. Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn land_creatures_hit_and_die() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    use super::walkers::{DYING, HIT, STAND};
    let run = |scene: &mut Scene, n: u32, data: &GameData, caches: &mut Caches| {
        let mut states = Vec::new();
        for _ in 0..n {
            scene.ticks += 1;
            scene.tick_walkers(data, caches);
            states.push(scene.walkers[0].state);
        }
        states
    };
    for (class, last) in [(0x1f4, 32), (0x1f5, 56)] {
        let mut scene = Scene::load(&data, &mut caches, "miss006").unwrap();
        scene.brain = None;
        let w = scene.grid.width as i32;
        let land = |s: &Scene, x: i32, y: i32| s.cell_types[(y * w + x) as usize] & 0xf == 8;
        let start = (2..scene.grid.height as i32 - 2)
            .flat_map(|y| (2..w - 4).map(move |x| (x, y)))
            .find(|&(x, y)| land(&scene, x, y))
            .unwrap();
        scene.put_walker(super::walkers::NO_LAIR, class, start, &data, &mut caches);
        let id = scene.walkers[0].id;
        (scene.walkers[0].scan, scene.walkers[0].wake) = (u32::MAX, u32::MAX);
        if class == 0x1f4 {
            scene.walker_struck(id, 10_000.0 / 15.0, None);
            let states = run(&mut scene, 30, &data, &mut caches);
            assert_eq!((scene.walkers[0].life, scene.walkers[0].pct), (20_000, 66));
            // It appears for 16 ticks, then shows the hit for 10.
            let hit: Vec<usize> = (0..30).filter(|&i| states[i] == HIT).collect();
            assert_eq!(hit, (15..25).collect::<Vec<_>>());
            assert_eq!(states[25], STAND);
        }
        scene.sounds.clear();
        scene.walker_struck(id, 40_000.0 / 15.0, Some((1, 0)));
        let states = run(&mut scene, 1, &data, &mut caches);
        let dying_from = scene.ticks;
        assert_eq!(states, vec![DYING]);
        assert_eq!((scene.walkers[0].life, scene.walkers[0].pct), (1, 1));
        assert!(scene.sounds.iter().any(|s| s.0 == "deac1d"));
        let mut gone_at = None;
        for _ in 0..80 {
            scene.ticks += 1;
            scene.tick_walkers(&data, &mut caches);
            if scene.walkers[0].gone {
                gone_at = Some(scene.ticks - dying_from);
                break;
            }
        }
        assert_eq!(gone_at, Some(last), "class {class:#x}");
        scene.run_walkers(0.0, &data, &mut caches);
        assert!(scene.walkers.is_empty() && scene.jams.is_empty());
        assert!(scene.object_index(id).is_none());
    }
}

/// A settlement being made a town, called off at frame f of n: `500 x (n
/// - f) / n` back, a settlement again (TownObj 0x441ba0 case 10); its
/// training called off: `(100 - p) x amber / 100` back, the fish share
/// into the town's `+0x252` / `+0x253` and the queue dropped. Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn order_10_on_a_town() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    scene.amber = 10_000;
    scene.fish = 10_000;
    let home = scene.own_sites()[0];
    let h = scene.object_index(home).unwrap();
    let c = scene.objects[h].combat.as_mut().unwrap();
    c.hull = c.max_hull;
    let amber = scene.amber;
    scene.upgrade(home, &data, &mut caches).unwrap();
    assert_eq!(scene.amber, amber - 500);
    for _ in 0..30 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let c = scene.construction_at(home).unwrap().clone();
    assert!(c.frame > 0 && c.frame < c.frames);
    let amber = scene.amber;
    scene
        .apply(&Order::Halt { id: home }, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.amber, amber + 500 * (c.frames - c.frame) / c.frames);
    let h = scene.object_index(home).unwrap();
    assert_eq!(scene.objects[h].class, ObjectClass::Building(SETTLEMENT));
    assert!(scene.construction_at(home).is_none());
    // Two swordsmen (30 amber, 3 raw fish and 70 ticks each), 19 ticks
    // in: p = 19 x 100 / 140 = 13.
    let (amber, fish) = (scene.amber, scene.fish);
    scene.train(home, 2).unwrap();
    scene.train(home, 2).unwrap();
    for _ in 0..19 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    scene
        .apply(&Order::Halt { id: home }, &data, &mut caches)
        .unwrap();
    assert!(scene.training_at(home).is_none());
    assert_eq!(scene.amber, amber - 60 + 87 * 60 / 100);
    assert_eq!(scene.fish, fish - 600);
    // v = 87 x 6 / 100 = 5: count 1, last part 5.
    assert_eq!(scene.town_fish.get(&home), Some(&(1, 5)));
}

/// A building goes up a frame of its construction picture every 200 / n
/// ticks, the first on the first tick, its hit points `max x (f x 100 /
/// n) / 100` (0x4230f0, 0x423410); order 10 while it is built gives the
/// whole price back and removes it, no ruins, counted as the owner's
/// building lost (LandObj 0x425990, 0x424740). A school's study called
/// off gives `(100 - p) x price / 100` back (0x432210). Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn order_10_on_buildings_and_studies() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    scene.brain = None;
    scene.permits = crate::permits::Permits::ALL;
    scene.amber = 10_000;
    let home = scene.own_sites()[0];
    let h = scene.object_index(home).unwrap();
    let (hx, hy) = (
        (scene.objects[h].x / 64.0) as i32,
        (scene.objects[h].y / 64.0) as i32,
    );
    let school = economy::SCHOOL;
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, school, x, y).is_ok())
        .unwrap();
    let place = |scene: &mut Scene, caches: &mut Caches| {
        scene
            .place_building(home, school, site.0, site.1, &data, caches)
            .unwrap();
        scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::Building(school))
            .unwrap()
            .id
    };
    let id = place(&mut scene, &mut caches);
    let i = scene.object_index(id).unwrap();
    let n = scene.objects[i].frames.len() as u32;
    let max = (scene.objects[i].combat.as_ref().unwrap().max_hull * 15.0).round() as u32;
    scene.update(crate::clock::TICK, &data, &mut caches);
    let c = scene.construction_at(id).unwrap().clone();
    assert_eq!((c.frame, c.frames), (1, n));
    let i = scene.object_index(id).unwrap();
    let hull = scene.objects[i].combat.as_ref().unwrap().hull;
    assert_eq!((hull * 15.0).round() as u32, max * (100 / n) / 100);
    // Called off while built: 1000 (0x42d8b0(0x3c)) back, gone at once.
    let amber = scene.amber;
    let lost = scene.stats.buildings_lost;
    scene
        .apply(&Order::Halt { id }, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.amber, amber + economy::building_cost(school));
    assert!(scene.object_index(id).is_none());
    assert!(scene.construction_at(id).is_none());
    assert_eq!(scene.stats.buildings_lost, lost + 1);
    assert!(!scene
        .objects
        .iter()
        .any(|o| o.class == ObjectClass::Building(school)));
    // Built again, it stands at the step reaching n: 1 + (n - 1) x d ticks.
    let id = place(&mut scene, &mut caches);
    let d = crate::objects::BUILDING_CYCLE[0] / n;
    let mut ticks = 0;
    while scene.construction_at(id).is_some() {
        scene.update(crate::clock::TICK, &data, &mut caches);
        ticks += 1;
    }
    assert_eq!(ticks, 1 + (n - 1) * d);
    // The ram studied from level 0 (1000, 600 ticks): called off at once,
    // all of it back; at 240 ticks (p = 40), 60%.
    let amber = scene.amber;
    scene.start_research(id, combat::Upgrade::Ram).unwrap();
    assert_eq!(scene.amber, amber - 1000);
    scene
        .apply(&Order::Halt { id }, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.amber, amber);
    assert!(scene.research_at(id).is_none());
    scene.start_research(id, combat::Upgrade::Ram).unwrap();
    for _ in 0..240 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    scene
        .apply(&Order::Halt { id }, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.amber, amber - 1000 + 60 * 1000 / 100);
    // Nothing on: nothing back.
    let amber = scene.amber;
    scene
        .apply(&Order::Halt { id }, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.amber, amber);
}

/// The temples called off (0x438300, 0x439260 case 10): learning gives
/// back the elapsed share, `p x 1000 / 100`; the Flame `p x 6000 / 100`
/// amber and `p x 15` fish. Needs `OPENCONQUEST_DATA`.
#[test]
fn order_10_on_temples() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    scene.permits = crate::permits::Permits::ALL;
    scene.amber = 20_000;
    scene.fish = 20_000;
    let home = scene.own_sites()[0];
    let h = scene.object_index(home).unwrap();
    let (hx, hy) = (
        (scene.objects[h].x / 64.0) as i32,
        (scene.objects[h].y / 64.0) as i32,
    );
    let war = crate::magic::Temple::War.class();
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, war, x, y).is_ok())
        .unwrap();
    scene
        .place_building(home, war, site.0, site.1, &data, &mut caches)
        .unwrap();
    let id = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(war))
        .unwrap()
        .id;
    while scene.construction_at(id).is_some() {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    // 50 ticks of the 200: p = 25, 250 back.
    scene
        .learn_spell(crate::magic::Temple::War, Some(id))
        .unwrap();
    let amber = scene.amber;
    for _ in 0..50 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    scene
        .apply(&Order::Halt { id }, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.amber, amber + 25 * 1000 / 100);
    assert!(scene.magic.learning(crate::magic::Temple::War).is_none());
    // The Flame, 100 ticks of 250: p = 40.
    scene.magic.brew_flame(Some(id));
    let (amber, fish) = (scene.amber, scene.fish);
    for _ in 0..100 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    scene
        .apply(&Order::Halt { id }, &data, &mut caches)
        .unwrap();
    assert_eq!(
        (scene.amber, scene.fish),
        (amber + 40 * 6000 / 100, fish + 40 * 15)
    );
    assert!(scene.magic.flame_brewing().is_none());
}

/// A fort's group scan (0x446d60): two enemy ships at the same cell
/// distance score alike, `(reach + 1) x 10 - d x 10` (0x4472d0); the draw
/// modulo their count picks among them in row order; the one held stays
/// on a tie. The unit list's generator starts at 0 and steps `0xa70427df
/// - s x 0x7f82434b` (0x41a97c), a draw its high half. Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn fort_scan_scores_and_ties() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let mut fire = crate::combat::FortFire::default();
    fire.step_draws();
    assert_eq!(fire.draws[..3], [0xa704, 0xc21f, 0xaf5a]);
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    scene.brain = None;
    let fort = scene
        .objects
        .iter()
        .position(|o| matches!(o.class, ObjectClass::Building(0x34..=0x36)) && o.is_armed())
        .expect("a fort");
    let fort_id = scene.objects[fort].id;
    let owner = scene.objects[fort].owner.unwrap();
    let (fx, fy) = scene.objects[fort].cell.unwrap();
    let item = scene.objects[fort].shooters()[0];
    let level = scene.levels_of(Some(owner)).reach_for(item);
    let reach = crate::combat::range_cells(item, level).1 as i32;
    let mask = super::placement::reach_mask(item, level).unwrap();
    let half = mask.len() as i32 / 2;
    // Water cells the group sees, by distance, in row order.
    let d = |x: i32, y: i32| {
        super::fort::root(((x - fx) * (x - fx) + (y - fy) * (y - fy)) as u64) as i32
    };
    let mut cells = Vec::new();
    for y in fy - reach..=fy + reach {
        for x in fx - reach..=fx + reach {
            let (mx, my) = (x - fx + half, y - fy + half);
            if scene.nav.is_water(x, y)
                && (0..mask.len() as i32).contains(&mx)
                && (0..mask.len() as i32).contains(&my)
                && mask[my as usize].as_bytes()[mx as usize] == b'#'
            {
                cells.push((x, y));
            }
        }
    }
    let far = (1..=reach)
        .rev()
        .find(|&k| cells.iter().filter(|c| d(c.0, c.1) == k).count() >= 2)
        .expect("two water cells alike");
    let pair: Vec<(i32, i32)> = cells
        .iter()
        .copied()
        .filter(|c| d(c.0, c.1) == far)
        .take(2)
        .collect();
    let ships: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| o.ship.is_some())
        .take(2)
        .map(|o| o.id)
        .collect();
    scene
        .objects
        .retain(|o| o.id == fort_id || ships.contains(&o.id));
    for (&id, &(x, y)) in ships.iter().zip(&pair) {
        let k = scene.object_index(id).unwrap();
        scene.objects[k].owner = Some((owner.0, owner.1 + 1));
        (scene.objects[k].x, scene.objects[k].y) = (x as f32 * 64.0 + 32.0, y as f32 * 64.0 + 32.0);
    }
    let f = scene.object_index(fort_id).unwrap();
    let k = scene.object_index(ships[0]).unwrap();
    assert_eq!(
        scene.group_score(f, item, pair[0], k),
        ((reach + 1) * 10 - far * 10) as u32
    );
    assert_eq!(
        scene.group_scan(f, item, 0, None),
        Some((ships[0], pair[0]))
    );
    assert_eq!(
        scene.group_scan(f, item, 1, None),
        Some((ships[1], pair[1]))
    );
    assert_eq!(
        scene.group_scan(f, item, 0xa704, None),
        Some((ships[0], pair[0]))
    );
    // Held, the second stays on the tie.
    let held = Some((ships[1], pair[1]));
    assert_eq!(scene.group_scan(f, item, 0, held), held);
}

/// Order 10 to a fort: its twelve unit groups cease fire (0x4469b0) and
/// it shoots no more (0x4469c0 with mode 0), until order 0x23 sub 1 sets
/// them at will (0x4469a0), an attack order 0x15 / 0x16 at a target
/// (0x446960), or, firing on aggressors (sub 3), a blow turns the groups
/// its bomb kind names on the shooter (0x4184f0). Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn order_10_silences_a_fort() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    let fort = scene
        .objects
        .iter()
        .position(|o| matches!(o.class, ObjectClass::Building(0x34..=0x36)) && o.is_armed())
        .expect("a fort");
    let (fx, fy) = (scene.objects[fort].x, scene.objects[fort].y);
    let fort_id = scene.objects[fort].id;
    let owner = scene.objects[fort].owner.unwrap();
    let forts: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| matches!(o.class, ObjectClass::Building(0x34..=0x36)))
        .map(|o| o.id)
        .collect();
    for &id in &forts {
        scene.halt(id, &data, &mut caches).unwrap();
        let i = scene.object_index(id).unwrap();
        let fire = scene.objects[i].combat.as_ref().unwrap().fire;
        assert!(!fire.fires() && !fire.aggressor);
    }
    let ship = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
    let ship_id = scene.objects[ship].id;
    scene
        .objects
        .retain(|o| o.ship.is_none() || o.id == ship_id);
    let i = scene.object_index(ship_id).unwrap();
    scene.objects[i].owner = Some((owner.0, owner.1 + 1));
    let water = scene
        .nav
        .nearest_water((fx / 64.0) as i32, (fy / 64.0) as i32)
        .unwrap();
    scene.objects[i].x = water.0 as f32 * 64.0 + 32.0;
    scene.objects[i].y = water.1 as f32 * 64.0 + 32.0;
    // The ship's own weapons are taken, so every shot is a fort's.
    scene.objects[i].combat.as_mut().unwrap().weapons.clear();
    // The ship is kept where it is, by the fort.
    let at = (water.0 as f32 * 64.0 + 32.0, water.1 as f32 * 64.0 + 32.0);
    let quiet = |scene: &mut Scene, caches: &mut Caches| {
        for _ in 0..50 * 5 {
            if let Some(i) = scene.object_index(ship_id) {
                let o = &mut scene.objects[i];
                (o.x, o.y) = at;
                let s = o.ship.as_mut().unwrap();
                s.task = None;
                s.path.clear();
            }
            scene.update(0.02, &data, caches);
            // The forts' shots (a stronghold of miss002's fires at the
            // player's buildings in its reach, 0x40fd30).
            if scene
                .shots
                .iter()
                .any(|s| s.shooter.is_some_and(|id| forts.contains(&id)))
            {
                return false;
            }
        }
        true
    };
    assert!(quiet(&mut scene, &mut caches), "a fort fired");
    let fire_of = |scene: &Scene| {
        let i = scene.object_index(fort_id).unwrap();
        scene.objects[i].combat.as_ref().unwrap().fire
    };
    // Fire on aggressor (0x23 sub 3): still silent, until struck.
    scene.fort_order(fort_id, 3).unwrap();
    let fire = fire_of(&scene);
    assert!(!fire.fires() && fire.aggressor);
    assert!(quiet(&mut scene, &mut caches), "a fort fired");
    // A blow of bomb kind 4 from the ship: group 1 turns on it
    // (0x4184f0), the others stay silent.
    scene.fort_blow(
        fort_id,
        1.0,
        0.0,
        4,
        false,
        None,
        Some(ship_id),
        &data,
        &mut caches,
    );
    let fire = fire_of(&scene);
    let mut want = [0; 12];
    want[1] = ship_id;
    assert_eq!(fire.targets, want);
    assert!(!fire.at_will);
    // Guard off (sub 2) ceases fire and clears the aggressor; guard on
    // (sub 1) fires at will, and the fort fires again.
    scene.fort_order(fort_id, 2).unwrap();
    let fire = fire_of(&scene);
    assert!(!fire.fires() && !fire.aggressor);
    scene.fort_order(fort_id, 1).unwrap();
    assert!(fire_of(&scene).at_will);
    assert!(!quiet(&mut scene, &mut caches), "the fort did not fire");
    // Silenced again, an attack order (0x15 / 0x16 {target, 7}) sets
    // every group at the ship, and it fires.
    for &id in &forts {
        scene.halt(id, &data, &mut caches).unwrap();
    }
    scene.shots.clear();
    assert!(quiet(&mut scene, &mut caches), "a fort fired");
    scene.order_attack(fort_id, ship_id).unwrap();
    let fire = fire_of(&scene);
    assert_eq!(fire.targets, [ship_id; 12]);
    assert!(!fire.at_will);
    assert!(!quiet(&mut scene, &mut caches), "the fort did not fire");
}

/// Lightning on the Gorgon (0x402ab8) and a siren (0x404505): resting under
/// water (no picture) they are left whole; up, struck one cell off they
/// are killed, put down to the caster once; the Gorgon's sound event 116
/// (`hitmed`), the siren's 117 has none. Needs `OPENCONQUEST_DATA`.
#[test]
fn lightning_on_gorgon_and_siren() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    let at = |c: (i32, i32)| (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
    for (mission, class, sound) in [("battle17", 0x1fa, true), ("battle08", 0x1fd, false)] {
        let mut scene = Scene::load(&data, &mut caches, mission).unwrap();
        scene.brain = None;
        let human = scene.human.map(|h| (h.nation, h.slot));
        assert!(human.is_some());
        // Nothing near her fights or is fought.
        scene.objects.retain(|o| o.ship.is_none());
        for o in scene.objects.iter_mut() {
            if let Some(c) = o.combat.as_mut() {
                c.weapons.clear();
            }
        }
        let id = scene
            .objects
            .iter()
            .find(|o| o.class == ObjectClass::Monster(class))
            .map(|o| o.id)
            .unwrap();
        let cell = |s: &Scene| {
            let o = &s.objects[s.object_index(id).unwrap()];
            ((o.x / t).floor() as i32, (o.y / t).floor() as i32)
        };
        let life = |s: &Scene| {
            s.object_index(id)
                .and_then(|k| s.objects[k].combat.as_ref())
                .map(|c| c.hull)
        };
        let killed = |s: &Scene| s.stats.monsters_killed[(class - 0x1f4) as usize];
        // Her first turn: resting under water.
        scene.update(crate::clock::TICK, &data, &mut caches);
        assert!(scene.creature_hidden(id));
        let full = life(&scene).unwrap();
        let (cx, cy) = cell(&scene);
        let (x, y) = at((cx + 1, cy));
        scene.lightning_strike(x, y, human, &data, &mut caches);
        assert_eq!(life(&scene), Some(full));
        assert_eq!(killed(&scene), 0);
        // Up: one cell off kills her, put down to the caster.
        let mut n = 0;
        while scene.creature_hidden(id) {
            scene.update(crate::clock::TICK, &data, &mut caches);
            n += 1;
            assert!(n < 2000, "she never came up");
        }
        scene.sounds.clear();
        scene.lightning_strike(x, y, human, &data, &mut caches);
        assert!(life(&scene).is_none_or(|h| h <= 0.0));
        assert_eq!(killed(&scene), 1);
        assert_eq!(scene.sounds.iter().any(|s| s.0 == "hitmed"), sound);
        scene.lightning_strike(x, y, human, &data, &mut caches);
        assert_eq!(killed(&scene), 1);
    }
}

/// A land creature walks from half cell to half cell as the original's
/// picture runs (0x412740, 0x411520, 0x4111b0): the first step takes 39
/// ticks for a cyclops (frames 0-0x26, a frame a tick, and the tick that
/// takes the next half), the later ones 33 (0x16-0x36); a minotaur heading
/// east 37 and 17 (its 36 frames, from 0x14 after); its place moves 32
/// pixels a half, held over the first frames (cyclops below 7). Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn land_creatures_step_half_cells() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    for (class, steps) in [(0x1f4, [39, 33, 33]), (0x1f5, [37, 17, 17])] {
        let mut scene = Scene::load(&data, &mut caches, "miss006").unwrap();
        scene.brain = None;
        let w = scene.grid.width as i32;
        let land = |s: &Scene, x: i32, y: i32| s.cell_types[(y * w + x) as usize] & 0xf == 8;
        let start = (2..scene.grid.height as i32 - 2)
            .flat_map(|y| (2..w - 4).map(move |x| (x, y)))
            .find(|&(x, y)| (0..3).all(|d| land(&scene, x + d, y)))
            .unwrap();
        scene.put_walker(super::walkers::NO_LAIR, class, start, &data, &mut caches);
        let k = scene.walkers.len() - 1;
        let id = scene.walkers[k].id;
        let w = &mut scene.walkers[k];
        let (hx, hy) = w.half;
        let gw = w.grid.unwrap().2 * 2;
        w.path = (0..4).map(|d| (hx + d, hy, 0)).collect();
        for &(x, y, _) in &w.path {
            w.halves[(y * gw + x) as usize] = 0;
        }
        (w.state, w.step, w.moving, w.heading) = (super::walkers::WALK, 0, false, 0);
        w.scan = u32::MAX;
        let x0 = w.pos.0;
        let mut changes = Vec::<u32>::new();
        let mut last = (hx, hy);
        let mut held = None;
        for t in 1..200u32 {
            scene.ticks += 1;
            scene.tick_walkers(&data, &mut caches);
            let w = &scene.walkers[k];
            if t == 5 {
                held = Some(w.pos.0);
            }
            if w.half != last {
                changes.push(t);
                last = w.half;
            }
            if changes.len() == 3 {
                break;
            }
        }
        // The first tick takes the start half itself; then a half a step.
        changes.insert(0, 1);
        let gaps: Vec<u32> = changes.windows(2).map(|p| p[1] - p[0]).collect();
        assert_eq!(gaps, steps, "class {class:#x}: {changes:?}");
        assert_eq!(held, Some(x0), "held over the first frames");
        let i = scene.object_index(id).unwrap();
        assert_eq!(scene.objects[i].x as i32, x0 + 3 * 32);
    }
}

/// Lightning on creatures (0xda93): a harpy or dragon one cell off loses a
/// quarter of the life it has (0x420cd8), on its cell it is removed; a land
/// creature one cell off loses 25 percentage points (0x414b0f); a gorgon up
/// anywhere in the 3 x 3 is killed with the caster as killer (0x402ab8);
/// none shows a hit. Needs `OPENCONQUEST_DATA`.
#[test]
fn lightning_on_creatures() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    let points = |s: &Scene, id: u32| {
        let i = s.object_index(id).unwrap();
        (s.objects[i].combat.as_ref().unwrap().hull * 15.0).round() as i64
    };
    let at = |c: (i32, i32)| (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
    let human = Some((1, 0));
    // A harpy (200000), put beside a water cell: 150000 struck one cell
    // off, then removed on its cell.
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let beside = scene.nav.nearest_water(10, 10).unwrap();
    let cell = (beside.0 + 1, beside.1);
    let harpy = scene.objects.iter().map(|o| o.id).max().unwrap() + 1;
    let mut fields = vec![0u32; 11];
    fields[0] = 0x1f8;
    fields[4] = cell.0 as u32;
    fields[5] = cell.1 as u32;
    fields[7] = 1;
    let rec = openconquest_formats::ObjectRecord {
        id: harpy,
        class_id: 0x1f8,
        fields,
    };
    let mut obj = crate::objects::resolve(&data, &mut caches, &rec)
        .unwrap()
        .unwrap();
    (obj.x, obj.y) = at(cell);
    scene.objects.push(obj);
    let full = points(&scene, harpy);
    assert_eq!(full, 200_000);
    let (x, y) = at(beside);
    scene.lightning_strike(x, y, human, &data, &mut caches);
    assert_eq!(points(&scene, harpy), full - full * 25 / 100);
    assert!(!scene.sounds.iter().any(|s| s.0 == "hitmed"));
    let (x, y) = at(cell);
    scene.lightning_strike(x, y, human, &data, &mut caches);
    assert!(scene.object_index(harpy).is_none());
    // A cyclops (30000) on land beside the sea to its east, on the north-
    // west half of its cell (0x411e7f, 0x414359), struck on the water:
    // 22500, then 15000, then 7500, then killed.
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let w = scene.grid.width as i32;
    let clear = |s: &Scene, x: i32, y: i32| {
        s.objects.iter().all(|o| {
            ((o.x / t).floor() as i32 - x).abs() > 2 || ((o.y / t).floor() as i32 - y).abs() > 2
        })
    };
    let ground = |s: &Scene, x: i32, y: i32| {
        let l = s.cell_types[(y * w + x) as usize];
        l & 0xf == 8 && (l >> 20) & 0xf != 1 && l >> 24 == 0
    };
    let (land, beside) = (1..scene.grid.height as i32 - 1)
        .flat_map(|y| (1..w - 1).map(move |x| (x, y)))
        .find(|&(x, y)| {
            ground(&scene, x, y)
                && scene.tile_edges(x, y) & 9 == 0
                && scene.nav.is_water(x + 1, y)
                && clear(&scene, x, y)
        })
        .map(|(x, y)| ((x, y), (x + 1, y)))
        .unwrap();
    scene.put_walker(super::walkers::NO_LAIR, 0x1f4, land, &data, &mut caches);
    let cyclops = scene.objects.iter().map(|o| o.id).max().unwrap();
    let k = scene.object_index(cyclops).unwrap();
    assert_eq!(
        (scene.objects[k].x, scene.objects[k].y),
        (land.0 as f32 * t + 16.0, land.1 as f32 * t + 16.0)
    );
    let full = points(&scene, cyclops);
    assert_eq!(full, 30_000);
    let (x, y) = at(beside);
    for pct in [75, 50, 25] {
        scene.lightning_strike(x, y, human, &data, &mut caches);
        assert_eq!(points(&scene, cyclops), full * pct / 100);
    }
    scene.lightning_strike(x, y, human, &data, &mut caches);
    // Killed: removed at once (message 10), not left dying.
    assert!(scene
        .walkers
        .iter()
        .find(|w| w.id == cyclops)
        .is_none_or(|w| w.gone));
    // One on a tile whose west side is shore stands on a shut half (its
    // north-west, 0x5acf13 bit 8): the first strike about it kills it.
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let (land, beside) = (1..scene.grid.height as i32 - 1)
        .flat_map(|y| (1..w - 1).map(move |x| (x, y)))
        .find(|&(x, y)| {
            ground(&scene, x, y)
                && scene.tile_edges(x, y) & 8 != 0
                && scene.nav.is_water(x - 1, y)
                && clear(&scene, x, y)
        })
        .map(|(x, y)| ((x, y), (x - 1, y)))
        .unwrap();
    scene.put_walker(super::walkers::NO_LAIR, 0x1f4, land, &data, &mut caches);
    let cyclops = scene.objects.iter().map(|o| o.id).max().unwrap();
    let (x, y) = at(beside);
    scene.sounds.clear();
    scene.lightning_strike(x, y, human, &data, &mut caches);
    // Killed: removed at once (message 10), not left dying.
    assert!(scene
        .walkers
        .iter()
        .find(|w| w.id == cyclops)
        .is_none_or(|w| w.gone));
    // No cry: no shot struck it (0x413ad0's `+0x144`).
    assert!(!scene.sounds.iter().any(|s| s.0 == "deac1d"));
}

/// A ship being built takes shots by 0x4b4000 alone: `trunc(0x597fd8[letter]
/// x blow / 100)` off the hull, letters 0-2 nothing; at 0 it sinks, its
/// owner's ship lost, nothing given back. Needs `OPENCONQUEST_DATA`.
#[test]
fn shots_on_a_ship_being_built() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let yard = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(SHIPYARD) && scene.is_own(o))
        .map(|o| o.id)
        .unwrap();
    scene.amber = 1000;
    scene.start_ship(yard, 0).unwrap();
    scene.make_new_ships(&data, &mut caches);
    let id = scene.objects[scene.ship_on_berth(yard).unwrap()].id;
    // Its first step: 18200 / 40 = 455.
    scene.update(crate::clock::TICK, &data, &mut caches);
    let hull = |s: &Scene| {
        let k = s.object_index(id).unwrap();
        (s.objects[k].combat.as_ref().unwrap().hull * 15.0).round() as i64
    };
    assert_eq!(hull(&scene), 455);
    let hit = |s: &mut Scene, letter: u8, blow: u32| {
        let k = s.object_index(id).unwrap();
        let b = s.objects[k].ship.as_mut().unwrap().build.as_mut().unwrap();
        b.hits[0] = (letter, blow);
        b.hit_count = 1;
    };
    hit(&mut scene, 3, 300);
    scene.update(crate::clock::TICK, &data, &mut caches);
    assert_eq!(hull(&scene), 155);
    hit(&mut scene, 0, 300);
    scene.update(crate::clock::TICK, &data, &mut caches);
    assert_eq!(hull(&scene), 155);
    let (amber, fish) = (scene.amber, scene.fish);
    let lost = scene.stats.ships_lost[0];
    hit(&mut scene, 3, 500);
    scene.update(crate::clock::TICK, &data, &mut caches);
    assert!(scene.object_index(id).is_none(), "sunk");
    assert_eq!(scene.stats.ships_lost[0], lost + 1);
    assert_eq!((scene.amber, scene.fish), (amber, fish));
    assert!(scene.ship_on_berth(yard).is_none());
}

/// A settlement's store starts with the rams and armour its record counts
/// (0x441ba0 message 2: fields 8-10 rams 3-5, 11-13 armour 0-2): battle08's
/// settlement 0x12 a level-2 armour and a ram 5. Needs `OPENCONQUEST_DATA`.
#[test]
fn settlements_start_with_their_fittings() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let scene = Scene::load(&data, &mut caches, "battle08").unwrap();
    assert_eq!(scene.fittings.get(&0x12), Some(&(vec![2], vec![2])));
    for m in 0..14 {
        let scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).unwrap();
        for (armour, rams) in scene.fittings.values() {
            assert!(armour.len() + rams.len() < 100, "miss{m:03}");
        }
    }
}

/// A code-4 blow on a ship being built (0x4befc0, `+0x251 == 2`): the hull
/// takes it whole, crew and weapons nothing; under 1 it sinks, the blow's
/// giver its killer (`+0x80b`), nothing given back. Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn blows_on_a_ship_being_built() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let yard = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(SHIPYARD) && scene.is_own(o))
        .map(|o| o.id)
        .unwrap();
    scene.amber = 1000;
    scene.start_ship(yard, 0).unwrap();
    scene.make_new_ships(&data, &mut caches);
    let id = scene.objects[scene.ship_on_berth(yard).unwrap()].id;
    scene.update(crate::clock::TICK, &data, &mut caches);
    let hull = |s: &Scene| {
        let k = s.object_index(id).unwrap();
        (s.objects[k].combat.as_ref().unwrap().hull * 15.0).round() as i64
    };
    assert_eq!(hull(&scene), 455);
    let crew = |s: &Scene| {
        s.objects[s.object_index(id).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .crew
            .squads
    };
    let squads = crew(&scene);
    scene.share_blow(id, 300.0 / 15.0, None, &data, &mut caches);
    assert_eq!(hull(&scene), 155);
    assert_eq!(crew(&scene), squads);
    let (amber, fish) = (scene.amber, scene.fish);
    let lost = scene.stats.ships_lost[0];
    scene.share_blow(id, 155.0 / 15.0, Some((4, 0)), &data, &mut caches);
    assert!(scene.object_index(id).is_none(), "sunk");
    assert_eq!(scene.stats.ships_lost[0], lost + 1);
    assert_eq!((scene.amber, scene.fish), (amber, fish));
}

/// 0x49a4b0 searches the berth's neighbours from the north-west clockwise,
/// 3 at a corner and 5 on a side, from the first clockwise from outside
/// (0x49a1d0, 0x499ff0).
#[test]
fn berth_neighbours_in_the_original_order() {
    use super::yard::neighbours;
    let n = neighbours((11, 10), (64, 64));
    assert_eq!(n.len(), 8);
    assert_eq!(n[0], (10, 9));
    assert_eq!(n[7], (10, 10), "a yard at (10, 10) facing east: the 8th");
    assert_eq!(neighbours((0, 0), (64, 64)), vec![(1, 0), (1, 1), (0, 1)]);
    assert_eq!(
        neighbours((63, 0), (64, 64)),
        vec![(63, 1), (62, 1), (62, 0)]
    );
    assert_eq!(
        neighbours((5, 0), (64, 64)),
        vec![(6, 0), (6, 1), (5, 1), (4, 1), (4, 0)]
    );
    assert_eq!(
        neighbours((0, 5), (64, 64)),
        vec![(0, 4), (1, 4), (1, 5), (1, 6), (0, 6)]
    );
}

/// A bomber needs armour studied, a rammer the ram (0x43da20 case 4: -7).
/// Needs `OPENCONQUEST_DATA`.
#[test]
fn bombers_and_rammers_need_their_study() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.permits = crate::permits::Permits::ALL;
    let owner = scene.human.map(|h| (h.nation, h.slot)).unwrap();
    scene.levels.insert(owner, Default::default());
    let e = scene.may_build_ship_for(owner, 3).unwrap_err();
    assert!(e.contains("armour"), "{e}");
    let e = scene.may_build_ship_for(owner, 5).unwrap_err();
    assert!(e.contains("rams"), "{e}");
    let levels = scene.levels.get_mut(&owner).unwrap();
    levels.armour = 1;
    levels.ram = 1;
    for kind in [3, 5] {
        if let Err(e) = scene.may_build_ship_for(owner, kind) {
            assert!(!e.contains("studied"), "{e}");
        }
    }
}

/// A plan's armour falls back as 0x43ead0 does: a level 2 with no
/// fallback tries level 1 by the fit test; a fallback is bought untested.
/// Needs `OPENCONQUEST_DATA`.
#[test]
fn default_arming_takes_its_fallbacks_untested() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.permits = crate::permits::Permits::ALL;
    scene.default_arming = true;
    let owner = scene.human.map(|h| (h.nation, h.slot)).unwrap();
    let yard = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(SHIPYARD) && scene.is_own(o))
        .map(|o| o.id)
        .unwrap();
    // A bomber (armour 2, fallback 1) with no armour studied: the
    // fallback 1, untested (+30).
    scene.levels.insert(owner, Default::default());
    let p = scene.ship_price(yard, 3);
    assert_eq!(p.arming.armour, 1);
    // A royal ship (armour 2, no fallback) with armour 1 studied: level 1
    // by the test (+30) only with a smithy of the yard's town idle and the
    // amber in hand (0x43da20 case 2 with `param_3` 1); none studied: none.
    scene.levels.get_mut(&owner).unwrap().armour = 1;
    assert_eq!(scene.ship_price(yard, 2).arming.armour, 0);
    let town = scene
        .home_settlement(scene.object_index(yard).unwrap())
        .unwrap();
    let k = scene
        .objects
        .iter()
        .position(|o| {
            scene.is_own(o) && o.id != yard && o.id != town && o.class != ObjectClass::Ship
        })
        .unwrap();
    scene.objects[k].class = ObjectClass::Building(economy::SMITHY);
    let smithy = scene.objects[k].id;
    scene.towns.lists.entry(town).or_default().push(smithy);
    scene.amber = 29;
    assert_eq!(scene.ship_price(yard, 2).arming.armour, 0);
    scene.amber = 30;
    assert_eq!(scene.ship_price(yard, 2).arming.armour, 1);
    scene
        .research
        .push((smithy, combat::Upgrade::Armour, 2, 0.0));
    assert_eq!(scene.ship_price(yard, 2).arming.armour, 0);
    scene.research.clear();
    assert_eq!(scene.ship_price(yard, 2).arming.armour, 1);
    scene.levels.get_mut(&owner).unwrap().armour = 0;
    assert_eq!(scene.ship_price(yard, 2).arming.armour, 0);
    // Armour 2 and a ram 2 stored at the yard's settlement (`+0x248`,
    // `+0x24c`): fitted free, though not studied; built now they leave the
    // store (0x43ead0).
    let home = scene
        .home_settlement(scene.object_index(yard).unwrap())
        .unwrap();
    let bare = scene.ship_price(yard, 2);
    scene.fittings.insert(home, (vec![1, 2], vec![2]));
    let p = scene.ship_price(yard, 2);
    assert_eq!((p.arming.armour, p.arming.ram), (2, 2));
    assert_eq!(p.amber, bare.amber - [0, 25, 50][bare.arming.ram as usize]);
    assert_eq!(p.fittings_left, (vec![1], vec![]));
    scene.amber = 100_000;
    scene.fish = 100_000;
    scene.yard_order(yard, 2, false, true, None, true).unwrap();
    assert_eq!(scene.new_ships.len(), 1);
    assert_eq!(scene.fittings.get(&home), Some(&(vec![1], vec![])));
}

/// A barbarian structure's order builds on its berth at once, free, as a
/// yard's: a merchant in 1 + 39 x 8 ticks (0x40ef70 case 0x20, 0x4b3ab0).
/// Needs `OPENCONQUEST_DATA`.
#[test]
fn barbarian_structures_build_on_their_berth() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut found = false;
    for m in 0..14 {
        let mut scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).unwrap();
        scene.brain = None;
        let sites: Vec<u32> = scene
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Barbarian(_)) && o.owner.is_some())
            .map(|o| o.id)
            .collect();

        for site in sites {
            let funds = scene.funds(
                scene.objects[scene.object_index(site).unwrap()]
                    .owner
                    .unwrap(),
            );
            scene.barbarian_order(site, 1, false);
            scene.make_new_ships(&data, &mut caches);
            let Some(k) = scene.ship_on_berth(site) else {
                continue;
            };
            let owner = scene.objects[k].owner.unwrap();
            assert_eq!(scene.funds(owner), funds, "nothing paid");
            // Without the default-arming byte: bare, no marines.
            let sh = scene.objects[k].ship.as_ref().unwrap();
            assert_eq!(sh.crew.squads, [0; 3]);
            assert_eq!((sh.armour, sh.ram), (0, 0));
            let id = scene.objects[k].id;
            let mut ticks = 0;
            while scene.being_built(id) {
                scene.update(crate::clock::TICK, &data, &mut caches);
                ticks += 1;
                assert!(ticks < 400);
            }
            assert_eq!(ticks, 1 + 39 * 8);
            found = true;
            break;
        }
        if found {
            break;
        }
    }
    assert!(found, "a barbarian structure with a free berth");
}

/// Cargo comes ashore only at a port of the ship's own player not asleep
/// (0x4aab00 code 0xc, 0x4c5910): an order to unload at a settlement is
/// refused, the old task kept, and a laden ship with no port has none to
/// go to. Needs `OPENCONQUEST_DATA`.
#[test]
fn only_ports_take_cargo() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let home = scene.own_sites()[0];
    let i = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    let ship = scene.objects[i].id;
    {
        let s = scene.objects[i].ship.as_mut().unwrap();
        s.cargo = 50.0;
        s.cargo_kind = Some(Resource::Amber);
        s.task = None;
    }
    assert!(scene.order_unload(ship, home).is_err());
    assert!(scene.objects[i].ship.as_ref().unwrap().task.is_none());
    scene
        .objects
        .retain(|o| o.class != ObjectClass::Building(economy::PORT));
    let i = scene.object_index(ship).unwrap();
    assert_eq!(scene.unloading_site(i), None);
    scene.send_to_port(i, 0);
    assert!(scene.objects[i].ship.as_ref().unwrap().task.is_none());
}

/// One order of the 0x1f0 group a tick (0x4ab720): of a move (0x80) and
/// a gather (0x40) given in one tick the gather runs whatever their order;
/// a second move overwrites the first; a stop in the tick leaves the ship
/// stopped. Needs `OPENCONQUEST_DATA`.
#[test]
fn one_ship_order_a_tick() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    scene.brain = None;
    let t = TILE_SIZE as f32;
    let i = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind <= 1))
        .unwrap();
    let ship = scene.objects[i].id;
    let deposit = scene
        .objects
        .iter()
        .find(|o| o.deposit.is_some_and(|d| d.resource == Resource::Amber))
        .unwrap()
        .id;
    let (sx, sy) = (scene.objects[i].x, scene.objects[i].y);
    let water = (0..20)
        .flat_map(|d| [(d, 0), (-d, 0), (0, d), (0, -d)])
        .map(|(dx, dy)| ((sx / t) as i32 + dx, (sy / t) as i32 + dy))
        .find(|&(x, y)| (x, y) != ((sx / t) as i32, (sy / t) as i32) && scene.nav.is_water(x, y))
        .unwrap();
    let (mx, my) = (water.0 as f32 * t + t / 2.0, water.1 as f32 * t + t / 2.0);
    let task = |s: &Scene| {
        s.objects[s.object_index(ship).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .task
    };
    let gather = Order::Gather {
        ship,
        deposit,
        port: 0,
    };
    let go = Order::Move { ship, x: mx, y: my };
    // Move, then gather: the gather.
    scene.apply(&go, &data, &mut caches).unwrap();
    scene.apply(&gather, &data, &mut caches).unwrap();
    assert!(matches!(task(&scene), Some(Task::Gather { .. })));
    // Next tick, gather then move: the move is dropped.
    scene.update(crate::clock::TICK, &data, &mut caches);
    scene
        .apply(&Order::Stop { id: ship }, &data, &mut caches)
        .unwrap();
    scene.update(crate::clock::TICK, &data, &mut caches);
    scene.apply(&gather, &data, &mut caches).unwrap();
    scene.apply(&go, &data, &mut caches).unwrap();
    assert!(matches!(task(&scene), Some(Task::Gather { .. })));
    // A stop then a gather in one tick: stopped.
    scene.update(crate::clock::TICK, &data, &mut caches);
    scene
        .apply(&Order::Stop { id: ship }, &data, &mut caches)
        .unwrap();
    scene.apply(&gather, &data, &mut caches).unwrap();
    assert!(task(&scene).is_none());
}

/// A brain's barbarian builder (miss001 fleet 0: a class 0x3f and a class
/// 0x40 structure) orders through the land fleet's ship part: only the
/// class 0x40 takes it (vtable +0x9c, 0x40e7b0), with its berth free; the
/// ship goes on the berth at once, free, with the full default arming
/// (byte 0xd 1, 0x428f20), and is built in 1 + 38 x 8 ticks as a yard's
/// (0x4b3ab0); with the berth taken no order is given. Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn barbarian_builder_builds_on_its_berth() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss001").unwrap();
    let sites = scene.brain.as_ref().unwrap().fleets[0].ships.clone();
    let class = |scene: &Scene, id: u32| scene.objects[scene.object_index(id).unwrap()].class;
    assert!(sites
        .iter()
        .any(|&s| class(&scene, s) == ObjectClass::Barbarian(0x3f)));
    let site = *sites
        .iter()
        .find(|&&s| class(&scene, s) == ObjectClass::Barbarian(0x40))
        .unwrap();
    let owner = scene.objects[scene.object_index(site).unwrap()]
        .owner
        .unwrap();
    let funds = scene.funds(owner);
    // A bomber (type 3): weapons, armour and ram by 0x5964d8, 0x596268,
    // 0x5962c8.
    assert!(scene.fleet_build_for(0, 1, 3));
    scene.make_new_ships(&data, &mut caches);
    let k = scene
        .ship_on_berth(site)
        .expect("built on the barbarians' berth");
    let berth = scene.berth_cell(scene.object_index(site).unwrap()).unwrap();
    let t = TILE_SIZE as f32;
    let o = &scene.objects[k];
    assert_eq!(((o.x / t) as i32, (o.y / t) as i32), berth);
    assert_eq!(o.owner, Some(owner));
    assert_eq!(scene.funds(owner), funds);
    let id = o.id;
    // The berth taken: no order.
    assert!(!scene.fleet_build_for(0, 1, 3));
    let mut ticks = 0;
    while scene.being_built(id) {
        scene.update(crate::clock::TICK, &data, &mut caches);
        ticks += 1;
        assert!(ticks < 400);
    }
    assert_eq!(ticks, 1 + 38 * 8);
    let o = &scene.objects[scene.object_index(id).unwrap()];
    let mut weapons = o.combat.as_ref().unwrap().weapons.clone();
    weapons.sort();
    // Its marines (items 0-2) go aboard as crew.
    let list = combat::DEFAULT_WEAPONS[3];
    let mut want: Vec<u8> = list.iter().copied().filter(|&w| w >= 3).collect();
    want.sort();
    assert_eq!(weapons, want);
    let s = o.ship.as_ref().unwrap();
    assert_eq!(
        s.crew.count(),
        list.iter().filter(|&&w| w < 3).count() as u32
    );
    assert_eq!(s.armour, combat::DEFAULT_ARMOUR[3].0);
    assert_eq!(s.ram, combat::DEFAULT_RAM[3].0);
}

/// The land fleet's ship part passes a yard in a builder's list over
/// (0x428f20 orders types 0x1a6b and 0x1491 only): a computer player's
/// yard put in miss008's builder list, with the price in its purse, is
/// ordered nothing. Needs `OPENCONQUEST_DATA`.
#[test]
fn builder_passes_yards_over() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss008").unwrap();
    let yard = scene
        .objects
        .iter()
        .position(|o| {
            !scene.is_own(o)
                && o.owner.is_some()
                && matches!(
                    o.class,
                    ObjectClass::Building(economy::SHIPYARD | economy::DOCK)
                )
        })
        .expect("a computer player's yard");
    let (id, owner) = (scene.objects[yard].id, scene.objects[yard].owner.unwrap());
    scene.earn(owner, 100_000, 100_000);
    assert!(!scene.yard_busy(id));
    scene.brain.as_mut().unwrap().fleets[0].ships = vec![id];
    assert!(!scene.fleet_build_for(0, 1, 1));
    assert!(!scene.yard_busy(id));
}

/// Order 96's wait for a mended building counts only the land fleet's
/// passes after one that started nothing (`+0x18`): the pass after the
/// one that started the mending does not count. Needs
/// `OPENCONQUEST_DATA`.
#[test]
fn mended_wait_skips_the_pass_after_the_start() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss007").unwrap();
    let i = scene
        .objects
        .iter()
        .position(|o| matches!(o.class, ObjectClass::Barbarian(_)) && o.combat.is_some())
        .unwrap();
    let id = scene.objects[i].id;
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.hull = c.max_hull * 0.5;
    // Not being mended: it stays worn while the order waits.
    scene.brain.as_mut().unwrap().jobs = vec![super::brain::Job::Mended {
        target: id,
        tries: 2,
        fresh: true,
    }];
    let mut passes = |scene: &mut Scene, n: u32| {
        for _ in 0..50 * n {
            scene.update(crate::clock::TICK, &data, &mut caches);
            scene.mending.clear();
        }
    };
    let waiting = |scene: &Scene| {
        scene
            .brain
            .as_ref()
            .unwrap()
            .jobs
            .iter()
            .find_map(|j| match *j {
                super::brain::Job::Mended { target, tries, .. } if target == id => Some(tries),
                _ => None,
            })
    };
    passes(&mut scene, 1);
    assert_eq!(waiting(&scene), Some(2));
    passes(&mut scene, 1);
    assert_eq!(waiting(&scene), Some(1));
    passes(&mut scene, 1);
    assert_eq!(waiting(&scene), None);
}

/// An attack's fire records (0x100) run after the 0x1f0 group and go with
/// it (0x4ab350: the last armed position's record is of kind 3): an
/// attack and a move in one tick leave the move, whichever came first;
/// alone the attack is taken. Needs `OPENCONQUEST_DATA`.
#[test]
fn an_attack_goes_with_the_group() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss001").unwrap();
    past_hold(&mut scene);
    scene.brain = None;
    let t = TILE_SIZE as f32;
    let i = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some() && o.is_armed())
        .unwrap();
    let ship = scene.objects[i].id;
    let target = scene
        .objects
        .iter()
        .find(|o| o.combat.is_some() && Scene::are_enemies(&scene.objects[i], o))
        .unwrap()
        .id;
    let (sx, sy) = (scene.objects[i].x, scene.objects[i].y);
    let water = (1..20)
        .flat_map(|d| [(d, 0), (-d, 0), (0, d), (0, -d)])
        .map(|(dx, dy)| ((sx / t) as i32 + dx, (sy / t) as i32 + dy))
        .find(|&(x, y)| scene.nav.is_water(x, y))
        .unwrap();
    let (mx, my) = (water.0 as f32 * t + t / 2.0, water.1 as f32 * t + t / 2.0);
    let task = |s: &Scene| {
        s.objects[s.object_index(ship).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .task
    };
    let attack = Order::Attack { ship, target };
    let go = Order::Move { ship, x: mx, y: my };
    let tick = |s: &mut Scene, caches: &mut Caches| s.update(crate::clock::TICK, &data, caches);
    // Attack, then move: the move.
    scene.apply(&attack, &data, &mut caches).unwrap();
    scene.apply(&go, &data, &mut caches).unwrap();
    assert!(task(&scene).is_none());
    // Move, then attack: the move.
    tick(&mut scene, &mut caches);
    scene.apply(&go, &data, &mut caches).unwrap();
    scene.apply(&attack, &data, &mut caches).unwrap();
    assert!(task(&scene).is_none());
    // The attack alone.
    tick(&mut scene, &mut caches);
    scene.apply(&attack, &data, &mut caches).unwrap();
    assert_eq!(task(&scene), Some(Task::Attack { target }));
}

/// The commands of 0x520cc0 reach their objects at the next tick, in
/// order (0x521570, 0x5228c0 before the tick 0x542390): a command given
/// in the first two ticks of a game waits for the third (the hold of
/// `+0x3e`, 2 at speed 5); later ones are delivered at the turn of the
/// tick, nobody in the tick seeing them; a move and a dock order to one
/// ship in a tick leave it docking whichever came first (0x20 before
/// 0x80, 0x4ab720). Needs `OPENCONQUEST_DATA`.
#[test]
fn commands_reach_ships_at_the_next_tick() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    // A mission with a dock and a ship of its player that can reach it.
    let (mut scene, ship, dock) = (0..14)
        .find_map(|n| {
            let mut scene = Scene::load(&data, &mut caches, &format!("miss{n:03}")).ok()?;
            scene.brain = None;
            scene.computers.players.clear();
            let d = scene.objects.iter().position(|o| {
                o.class == ObjectClass::Building(economy::DOCK) && o.owner.is_some()
            })?;
            let i = scene.objects.iter().position(|o| {
                o.ship.is_some() && o.owner == scene.objects[d].owner && o.combat.is_some()
            })?;
            scene.route_beside(i, d)?;
            let (ship, dock) = (scene.objects[i].id, scene.objects[d].id);
            Some((scene, ship, dock))
        })
        .expect("a dock and a ship of its player");
    let i = scene.object_index(ship).unwrap();
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.hull = c.max_hull / 2.0;
    let (sx, sy) = (scene.objects[i].x, scene.objects[i].y);
    let water = (1..20)
        .flat_map(|d| [(d, 0), (-d, 0), (0, d), (0, -d)])
        .map(|(dx, dy)| ((sx / t) as i32 + dx, (sy / t) as i32 + dy))
        .find(|&(x, y)| scene.nav.is_water(x, y))
        .unwrap();
    let go = Order::Move {
        ship,
        x: water.0 as f32 * t + t / 2.0,
        y: water.1 as f32 * t + t / 2.0,
    };
    let state = |s: &Scene| {
        let o = &s.objects[s.object_index(ship).unwrap()];
        let sh = o.ship.as_ref().unwrap();
        (sh.task, sh.path.is_empty())
    };
    let tick = |s: &mut Scene, caches: &mut Caches| s.update(crate::clock::TICK, &data, caches);
    // The first two ticks hold even the player's.
    assert_eq!(scene.ticks, 0);
    scene.apply(&go, &data, &mut caches).unwrap();
    assert_eq!(state(&scene), (None, true));
    tick(&mut scene, &mut caches);
    assert_eq!(state(&scene), (None, true));
    tick(&mut scene, &mut caches);
    assert_eq!(state(&scene), (None, false));
    // A command given now is not seen until the tick's turn.
    scene.post(Order::Repair { ship, dock });
    assert_eq!(state(&scene).0, None);
    tick(&mut scene, &mut caches);
    assert_eq!(state(&scene).0, Some(Task::Repair { dock }));
    // A move and a dock order in one tick: the dock, either way.
    for orders in [
        [go.clone(), Order::Repair { ship, dock }],
        [Order::Repair { ship, dock }, go.clone()],
    ] {
        scene.post(Order::Stop { id: ship });
        tick(&mut scene, &mut caches);
        assert_eq!(state(&scene).0, None);
        for o in orders {
            scene.post(o);
        }
        tick(&mut scene, &mut caches);
        assert_eq!(state(&scene).0, Some(Task::Repair { dock }));
    }
}

/// An upgrade keeps the settlement's condition (TownObj 0x43b290 does not
/// write `+0xa8`; vt+0x54, 0x424910, rescales the hit points to the
/// town's maximum), until the first step writes the frame's share
/// (0x423410). Needs `OPENCONQUEST_DATA`.
#[test]
fn upgrade_keeps_the_condition_till_its_first_step() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let settlement = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(SETTLEMENT) && scene.is_own(o))
        .unwrap()
        .id;
    let i = scene.object_index(settlement).unwrap();
    let c = scene.objects[i].combat.as_mut().unwrap();
    c.hull = c.max_hull * 0.6;
    scene.amber = 10_000;
    scene.upgrade(settlement, &data, &mut caches).unwrap();
    // 500 and 400 x 40 / 100 for the mending.
    assert_eq!(scene.amber, 10_000 - 500 - 160);
    let i = scene.object_index(settlement).unwrap();
    let c = scene.objects[i].combat.as_ref().unwrap();
    assert!((c.hull / c.max_hull - 0.6).abs() < 1e-3);
    // The first tick steps frame 1 of n: `100 / n` percent.
    scene.update(crate::clock::TICK, &data, &mut caches);
    let i = scene.object_index(settlement).unwrap();
    let n = scene.objects[i].frames.len() as f32;
    let c = scene.objects[i].combat.as_ref().unwrap();
    let want = (100.0 / n).floor() / 100.0;
    assert!((c.hull / c.max_hull - want).abs() < 0.01);
}

/// A cast whirlpool's cycle (Whirl.cpp 0x447ee0, 0x447b80): under the
/// water 150-250 ticks, it rises (`whirl3`, `WHIRLSPR_0`, nibble 1 4),
/// is up (`spider`) 125-250 ticks till its picture's last frame, sinks
/// (`WHIRLSPR_2`) and is under again, its cell its own. A ship with Jason
/// on the cell keeps it under. A cast medium reef shows nothing for 75
/// ticks, then rises (`riff1wav`) (needs `OPENCONQUEST_DATA`).
#[test]
fn hazards_rise_and_sink() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.brain = None;
    let tick = |scene: &mut Scene, caches: &mut Caches| {
        scene.sounds.clear();
        scene.update(crate::clock::TICK, &data, caches);
    };
    let heard = |scene: &Scene, name: &str| scene.sounds.iter().any(|s| s.0 == name);
    let sea = scene.nav.nearest_water(20, 20).unwrap();
    let w = scene.grid.width as i32;
    let nibble = |scene: &Scene| (scene.cell_types[(sea.1 * w + sea.0) as usize] >> 4) & 0xf;
    let own = nibble(&scene);
    scene
        .spawn_hazard(
            magic::Spell::Whirlpool,
            sea.0,
            sea.1,
            None,
            &data,
            &mut caches,
        )
        .unwrap();
    let id = scene.objects.iter().map(|o| o.id).max().unwrap();
    let h = scene.hazards[&id];
    assert_eq!(h.state, 3);
    assert!((150..=250).contains(&h.wait));
    assert!((7500..=15000).contains(&(h.ends - scene.ticks)));
    assert!(scene.hazard_hidden(id) && !scene.hazard_cells().contains_key(&sea));
    for _ in 1..h.wait {
        tick(&mut scene, &mut caches);
        assert_eq!(scene.hazards[&id].state, 3);
    }
    tick(&mut scene, &mut caches);
    assert_eq!(scene.hazards[&id].state, 0);
    assert!(heard(&scene, "whirl3"));
    assert_eq!(nibble(&scene), 4);
    assert_eq!(scene.hazard_cells().get(&sea), Some(&true));
    let n0 = scene.objects[scene.object_index(id).unwrap()].frames.len() as u32;
    assert_eq!(scene.hazards[&id].step, 60 / n0);
    while scene.hazards[&id].state == 0 {
        tick(&mut scene, &mut caches);
    }
    assert!(heard(&scene, "spider"));
    let h = scene.hazards[&id];
    assert_eq!(h.state, 1);
    assert!((125..=250).contains(&h.wait));
    let up = scene.ticks;
    while scene.hazards[&id].state == 1 {
        tick(&mut scene, &mut caches);
    }
    assert!(scene.ticks - up >= h.wait);
    assert_eq!(scene.hazards[&id].state, 2);
    while scene.hazards[&id].state == 2 {
        tick(&mut scene, &mut caches);
    }
    assert_eq!(scene.hazards[&id].state, 3);
    assert_eq!(nibble(&scene), own);
    // Jason lying on the cell: its wait out, it stays under.
    let k = scene
        .objects
        .iter()
        .position(|o| o.ship.is_some())
        .expect("a ship");
    let ship = scene.objects[k].id;
    scene.objects[k].x = sea.0 as f32 * 64.0 + 32.0;
    scene.objects[k].y = sea.1 as f32 * 64.0 + 32.0;
    let s = scene.objects[k].ship.as_mut().unwrap();
    s.path.clear();
    s.task = None;
    s.heroes.push(crate::objects::JASON);
    for _ in 0..300 {
        tick(&mut scene, &mut caches);
        let k = scene.object_index(ship).unwrap();
        let o = &mut scene.objects[k];
        (o.x, o.y) = (sea.0 as f32 * 64.0 + 32.0, sea.1 as f32 * 64.0 + 32.0);
        o.ship.as_mut().unwrap().path.clear();
    }
    assert_eq!(scene.hazards[&id].state, 3);
    // A cast medium reef: 75 ticks unseen (0xff), then up with its sound.
    let reef_at = scene.nav.nearest_water(10, 10).unwrap();
    scene
        .spawn_hazard(
            magic::Spell::Reef,
            reef_at.0,
            reef_at.1,
            None,
            &data,
            &mut caches,
        )
        .unwrap();
    let reef = scene.objects.iter().map(|o| o.id).max().unwrap();
    assert_eq!(scene.hazards[&reef].state, 0xff);
    assert!((1..=3).contains(
        &scene.objects[scene.object_index(reef).unwrap()]
            .record
            .fields[7]
    ));
    for _ in 1..75 {
        tick(&mut scene, &mut caches);
        assert_eq!(scene.hazards[&reef].state, 0xff);
    }
    tick(&mut scene, &mut caches);
    assert_eq!(scene.hazards[&reef].state, 0);
    assert!(heard(&scene, "riff1wav"));
    // Saved and restored, it holds its cell again.
    let save = scene.save();
    let back = Scene::restore(&data, &mut caches, &save).unwrap();
    assert_eq!(back.hazards[&reef].state, 0);
    assert_eq!(back.hazard_cells().get(&reef_at), Some(&true));
}

/// Lightning on a leviathan (0x494f90 0xda93): its life (field 6, 100)
/// first goes to 0, the info then showing what is left; struck again,
/// what is left goes; struck on its cell it goes down (0x494400(0)) (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn lightning_on_a_leviathan() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    scene.brain = None;
    let k = scene
        .objects
        .iter()
        .position(|o| o.class == ObjectClass::WhaleZone)
        .unwrap();
    let id = scene.objects[k].id;
    scene.run_leviathans(&data, &mut caches);
    let k = scene.object_index(id).unwrap();
    assert_eq!(scene.leviathan_life(&scene.objects[k]), 100);
    let info = crate::object_info::of(&scene, &scene.objects[k]).unwrap();
    assert_eq!((info.life, info.volume), (Some(100), None));
    let (x, y) = (scene.objects[k].x, scene.objects[k].y);
    scene.lightning_strike(x, y, None, &data, &mut caches);
    let k = scene.object_index(id).unwrap();
    assert_eq!(scene.leviathan_life(&scene.objects[k]), 0);
    assert_eq!(
        scene.objects[k].deposit.and_then(|d| d.remaining),
        Some(100.0)
    );
    let info = crate::object_info::of(&scene, &scene.objects[k]).unwrap();
    assert_eq!((info.life, info.volume), (None, Some((100, 100))));
    scene.lightning_strike(x, y, None, &data, &mut caches);
    let k = scene.object_index(id).unwrap();
    assert_eq!(
        scene.objects[k].deposit.and_then(|d| d.remaining),
        Some(0.0)
    );
    assert_eq!(scene.leviathans[&id].state, super::leviathan::UNDER);
}

/// A town's buildings (0x427de0, 0x441190 / 0x427e40 at the start,
/// 0x43b290's record, 0x2873 on the first tick standing, 0x2874): in
/// miss005 the town 2's ten buildings name its anchor (8, 11) as their
/// town cell and join it in the objects' order; the port 41, free-standing
/// (0xff), has none; 35 is its workshop. A workshop it orders is its own at once and joins
/// its list on the first tick it stands; a save without the links has
/// them made again the same way.
#[test]
fn a_town_lists_its_buildings() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    let list = [34, 35, 36, 37, 38, 39, 40, 42, 60, 61];
    assert_eq!(scene.towns.lists.get(&2), Some(&list.to_vec()));
    let i = scene.object_index(41).unwrap();
    assert_eq!(scene.home_settlement(i), None);
    for id in list {
        let k = scene.object_index(id).unwrap();
        assert_eq!(scene.home_settlement(k), Some(2));
    }
    // An old save: made again as at the start.
    let mut save = scene.save();
    save.towns = None;
    let copy = Scene::restore(&data, &mut caches, &save).unwrap();
    assert_eq!(copy.towns, scene.towns);
    // A workshop ordered by the town: its own now, listed once standing.
    let id = scene.objects.iter().map(|o| o.id).max().unwrap() + 1;
    scene
        .place_building_facing(2, WORKSHOP, 12, 12, Some(0xff), &data, &mut caches)
        .unwrap();
    let w = scene.object_index(id).unwrap();
    assert_eq!(scene.objects[w].record.fields[8..10], [8, 11]);
    assert_eq!(scene.home_settlement(w), Some(2));
    let workshops = |scene: &Scene| -> Vec<usize> {
        scene
            .town_buildings(2)
            .into_iter()
            .filter(|&k| scene.objects[k].class == ObjectClass::Building(WORKSHOP))
            .collect()
    };
    let shops = workshops(&scene);
    assert_eq!(shops, vec![scene.object_index(35).unwrap()]);
    scene.run_town_links();
    assert!(!scene.towns.lists[&2].contains(&id));
    scene.constructions.retain(|c| c.site != id);
    scene.run_town_links();
    assert_eq!(scene.towns.lists[&2].last(), Some(&id));
    assert_eq!(workshops(&scene), [shops, vec![w]].concat());
    // Gone, it leaves the list.
    scene.leave_town(id, false);
    assert!(!scene.towns.lists[&2].contains(&id));
}

/// 0x43ead0 / 0x43e5c0 buy a piece only when the yard's town may have it
/// now (0x43da20 with `param_3` 1, `Scene::may_buy`): miss005's town 2
/// (workshop 35, smithy 34, dock 36). A royal ship with everything
/// allowed and idle: items 0, 2, 4, 8, armour 2, ram 2, 1390 amber and 9
/// fish-hundreds. With its only workshop busy on a study, the weapons
/// fall back untested: 4 -> 4, 8 -> 5, then strength 4 < 5 adds the
/// fallback 5: items [0, 2, 4, 5, 5], 1420 amber, 9 fish-hundreds. Its
/// town being made a town buys nothing (-1): the items' fallbacks come
/// untested, armour and ram (no untested fallback for this kind) none.
#[test]
fn default_arming_buys_what_the_town_may_have_now() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    scene.permits = crate::permits::Permits::ALL;
    let owner = scene.objects[scene.object_index(36).unwrap()]
        .owner
        .unwrap();
    let mut levels = scene.levels_of(Some(owner));
    levels.weapons = 1;
    levels.armour = 2;
    levels.ram = 2;
    scene.levels.insert(owner, levels);
    scene.garrisons.remove(&2);
    scene.fittings.remove(&2);
    scene.earn(owner, 100_000, 100_000);
    assert_eq!(
        scene.town_workshops(2, WORKSHOP),
        vec![scene.object_index(35).unwrap()]
    );
    let p = scene.ship_price_with(36, 2, true);
    assert_eq!(p.arming.items, vec![0, 2, 4, 8]);
    assert_eq!((p.arming.armour, p.arming.ram), (2, 2));
    assert_eq!((p.amber, p.fish), (1390, 900));
    scene.research.push((35, combat::Upgrade::Weapons, 2, 0.0));
    let p = scene.ship_price_with(36, 2, true);
    assert_eq!(p.arming.items, vec![0, 2, 4, 5, 5]);
    assert_eq!((p.amber, p.fish), (1420, 900));
    scene.research.clear();
    scene.constructions.push(super::Construction::new(2, true));
    let p = scene.ship_price_with(36, 2, true);
    assert_eq!(p.arming.items, vec![0, 2, 4, 5, 5]);
    assert_eq!((p.arming.armour, p.arming.ram), (0, 0));
}

/// 0x43ead0 takes a stock item best first (qsort by condition) and the
/// ship built gets its condition (0x4dfe00 -> 0x4bd920, `+0x299`); one
/// bought is whole: miss005's town 2 with item 0 at 90 and 40, a royal
/// ship built now at its dock 36 carries item 0 at 90, the stock keeps
/// the one at 40.
#[test]
fn a_ship_built_takes_the_best_stock_unit() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    scene.brain = None;
    scene.permits = crate::permits::Permits::ALL;
    scene.default_arming = true;
    let owner = scene.objects[scene.object_index(36).unwrap()]
        .owner
        .unwrap();
    // The town's player plays.
    let h = scene.human.as_mut().unwrap();
    (h.nation, h.slot) = owner;
    scene.earn(owner, 100_000, 100_000);
    let mut units: [Vec<u8>; 9] = Default::default();
    units[0] = vec![40, 90];
    scene.set_stock_units(2, units);
    scene.fittings.remove(&2);
    let p = scene.ship_price_with(36, 2, true);
    assert_eq!(p.arming.items[0], 0);
    assert_eq!(p.arming.conditions[0], 90);
    assert_eq!(p.units_left[0], vec![40]);
    assert!(p.arming.conditions[1..].iter().all(|&c| c == 100));
    while let Some(k) = scene.ship_orders.iter().position(|o| o.site == 36) {
        scene.ship_orders.remove(k);
    }
    assert_eq!(scene.yard_answer(scene.object_index(36).unwrap(), None), 0);
    scene.yard_order(36, 2, false, true, None, true).unwrap();
    assert_eq!(scene.stock_units(2)[0], vec![40]);
    scene.make_new_ships(&data, &mut caches);
    let ship = scene
        .objects
        .iter()
        .find(|o| {
            o.ship
                .as_ref()
                .is_some_and(|s| s.build.as_ref().is_some_and(|b| b.yard == 36))
        })
        .unwrap();
    assert_eq!(ship.ship.as_ref().unwrap().conditions[0], vec![90]);
}

/// A computer player's ship is priced and armed by 0x43ead0 as the
/// player's, its orders carrying the default-arming byte (0x428f20,
/// 0x42957a): miss005's town 2 (a computer's) builds a royal ship at its
/// dock 36 now, taking its stock's item 0 and paying the rest.
#[test]
fn a_computers_ship_is_armed_from_its_town() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    scene.brain = None;
    scene.permits = crate::permits::Permits::ALL;
    let owner = scene.objects[scene.object_index(36).unwrap()]
        .owner
        .unwrap();
    assert_ne!(scene.human.map(|h| (h.nation, h.slot)), Some(owner));
    scene.earn(owner, 100_000, 100_000);
    let mut units: [Vec<u8>; 9] = Default::default();
    units[0] = vec![90];
    scene.set_stock_units(2, units);
    scene.fittings.remove(&2);
    scene.ship_orders.retain(|o| o.site != 36);
    let price = scene.ship_price_with(36, 2, true);
    let funds = scene.funds(owner);
    scene.start_ship_armed(36, 2, true, true).unwrap();
    assert_eq!(scene.garrison(2)[0], 0);
    let n = scene.new_ships.last().unwrap();
    let arming = n.arming.as_ref().unwrap();
    assert_eq!(arming, &price.arming);
    assert_eq!((arming.items[0], arming.conditions[0]), (0, 90));
    assert_eq!(n.price, (price.amber, price.fish));
    let left = scene.funds(owner);
    assert_eq!(
        (funds.0 - left.0, funds.1 - left.1),
        (price.amber, price.fish)
    );
}

/// A brain's fleet sails in formation (fleet order 100, 0x4ccc30 ->
/// 0x4cc970 -> the planner 0x560100): miss009's fleet 6, four shallow
/// ships and a deep one ordered to (45, 41), each get the cell the
/// original's planner gives for this layout (worked out by running
/// 0x5614e0 itself on the same grids), and are sent there by command
/// 0x14; miss010's fleet 4, six ships whose slots 0x55f870 deals out.
#[test]
fn brain_fleet_moves_in_formation() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    for _ in 0..30 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let order = scene.brain.as_ref().unwrap().fleets[6]
        .move_order
        .clone()
        .unwrap();
    assert_eq!(order.to, (45, 41));
    let want = [
        (62, (47, 41)),
        (63, (44, 39)),
        (64, (45, 41)),
        (65, (43, 41)),
        (66, (45, 42)),
    ];
    assert_eq!(order.slots, want);
    // Delivered at the tick's end: each ship is on its way to its cell.
    for (id, (x, y)) in want {
        let i = scene.object_index(id).unwrap();
        let end = *scene.objects[i].ship.as_ref().unwrap().path.back().unwrap();
        assert_eq!(((end.0 / 64.0) as i32, (end.1 / 64.0) as i32), (x, y));
    }
    let mut scene = Scene::load(&data, &mut caches, "miss010").unwrap();
    for _ in 0..70 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let order = scene.brain.as_ref().unwrap().fleets[4]
        .move_order
        .clone()
        .unwrap();
    assert_eq!(order.to, (6, 4));
    assert_eq!(
        order.slots,
        [
            (7, (6, 4)),
            (6, (5, 3)),
            (8, (6, 3)),
            (9, (7, 3)),
            (10, (5, 6)),
            (11, (5, 4)),
        ]
    );
}

/// A boat of miss000's gathering amber in a zone, with its first cargo.
fn amber_boat_in_zone(data: &GameData, caches: &mut Caches) -> (Scene, u32, u32, Vec<u32>) {
    let mut scene = Scene::load(data, caches, "miss000").unwrap();
    past_hold(&mut scene);
    let boat = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .unwrap()
        .id;
    let deposit = scene
        .objects
        .iter()
        .find(|o| o.deposit.is_some_and(|d| d.resource == Resource::Amber))
        .unwrap()
        .id;
    let ports: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| scene.is_own(o) && o.class == ObjectClass::Building(PORT))
        .map(|o| o.id)
        .collect();
    scene
        .apply(
            &Order::GatherAt {
                ships: vec![boat],
                deposit,
                keys: 0,
            },
            data,
            caches,
        )
        .unwrap();
    scene.update(crate::clock::TICK, data, caches);
    (scene, boat, deposit, ports)
}

/// A full amber boat ends its gathering and posts notice 0x138; its zone
/// hears it at the navy's tick (0x4c9e30 -> 0x4c9ae0): full, it has no
/// deposit to go to and is sent to its port, picked by 0x4c6640, by
/// command 0x1f, delivered at the end of the tick: the boat does not
/// choose a port itself (needs `OPENCONQUEST_DATA`).
#[test]
fn zone_sends_a_full_boat_to_unload() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, deposit, ports) = amber_boat_in_zone(&data, &mut caches);
    assert_eq!(ports.len(), 1);
    let i = scene.object_index(boat).unwrap();
    {
        let s = scene.objects[i].ship.as_mut().unwrap();
        s.cargo = crate::objects::cargo_capacity(s.kind);
        s.cargo_kind = Some(Resource::Amber);
        s.task = Some(Task::Gather {
            deposit,
            phase: GatherPhase::Collecting,
        });
    }
    scene.gathering_done(i, deposit);
    let s = scene.objects[i].ship.as_ref().unwrap();
    assert!(s.task.is_none());
    let notes: Vec<(u32, u32, u32)> = scene.gather_zones[0]
        .notices
        .iter()
        .map(|n| (n.code, n.ship, n.class))
        .collect();
    assert_eq!(notes, [(0x138, boat, 400)]);
    // The zone's list is the player's one port; the boat goes there.
    scene.update(crate::clock::TICK, &data, &mut caches);
    assert!(scene.gather_zones[0].notices.is_empty());
    assert_eq!(scene.gather_zones[0].ports.as_deref(), Some(&ports[..]));
    let i = scene.object_index(boat).unwrap();
    let s = scene.objects[i].ship.as_ref().unwrap();
    assert_eq!(s.home_port, ports[0]);
    assert!(matches!(
        s.task,
        Some(Task::Gather {
            deposit: d,
            phase: GatherPhase::ToPort(p),
        }) if d == deposit && p == ports[0]
    ));
    assert_eq!(s.priority, 76);
}

/// The ships' routes on miss005's shallow grid as the original's machine
/// code gives them (0x55ca10 and 0x55ba50 with the heading 0, run under
/// an emulator): the exact route floods from the goal, the near one from
/// the start, so round the same land they part (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn ship_routes_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    let nav = &scene.nav;
    let exact = |s, g| nav.find_path_avoiding(s, g, false, |_| false).unwrap();
    let near = |s, g, tol| nav.find_path_near(s, g, false, |_| false, tol).unwrap();
    assert_eq!(
        exact((33, 26), (27, 6)),
        [
            (32, 25),
            (31, 24),
            (30, 24),
            (29, 24),
            (28, 24),
            (27, 24),
            (26, 24),
            (25, 24),
            (24, 24),
            (23, 24),
            (22, 24),
            (21, 23),
            (20, 22),
            (20, 21),
            (20, 20),
            (20, 19),
            (20, 18),
            (20, 17),
            (20, 16),
            (20, 15),
            (21, 14),
            (22, 13),
            (23, 12),
            (24, 11),
            (25, 10),
            (26, 9),
            (27, 8),
            (27, 7),
            (27, 6)
        ]
    );
    assert_eq!(
        near((33, 26), (27, 6), 0),
        [
            (32, 27),
            (31, 27),
            (30, 27),
            (29, 27),
            (28, 27),
            (27, 27),
            (26, 27),
            (25, 27),
            (24, 26),
            (23, 25),
            (22, 24),
            (21, 23),
            (20, 22),
            (20, 21),
            (20, 20),
            (20, 19),
            (21, 18),
            (22, 17),
            (23, 16),
            (24, 15),
            (24, 14),
            (24, 13),
            (24, 12),
            (24, 11),
            (24, 10),
            (24, 9),
            (25, 8),
            (26, 7),
            (27, 6)
        ]
    );
    assert_eq!(
        exact((10, 1), (18, 13)),
        [
            (11, 1),
            (12, 1),
            (13, 1),
            (14, 1),
            (15, 2),
            (16, 3),
            (16, 4),
            (16, 5),
            (16, 6),
            (16, 7),
            (16, 8),
            (15, 9),
            (14, 10),
            (14, 11),
            (15, 12),
            (16, 13),
            (17, 13),
            (18, 13)
        ]
    );
    assert_eq!(
        near((10, 1), (18, 13), 0),
        [
            (11, 1),
            (12, 1),
            (13, 1),
            (14, 1),
            (15, 2),
            (16, 3),
            (16, 4),
            (16, 5),
            (15, 6),
            (14, 7),
            (14, 8),
            (14, 9),
            (14, 10),
            (14, 11),
            (15, 12),
            (16, 13),
            (17, 13),
            (18, 13)
        ]
    );
    // A goal on land, with a tolerance of 1 (a plain move there, 0x4a5f30):
    // it cannot be reached, so the nearest reached cell (0x55ba50's rings).
    assert!(!nav.is_sea(1, 29));
    assert_eq!(
        near((18, 14), (1, 29), 1),
        [
            (17, 13),
            (16, 13),
            (15, 13),
            (14, 14),
            (13, 15),
            (12, 16),
            (11, 17),
            (10, 18),
            (10, 19),
            (10, 20),
            (9, 21),
            (8, 22),
            (7, 23),
            (6, 23),
            (5, 23),
            (4, 24),
            (3, 25),
            (3, 26),
            (3, 27)
        ]
    );
}

/// A fort a ship founds (its landing party, 0x4b2d60 -> 0x421f00) costs
/// the building alone, holds no units and has no town (record town cell
/// 0xff), default arming or not (needs `OPENCONQUEST_DATA`).
#[test]
fn ship_founded_fort_bare() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    assert!(scene.default_arming);
    let boat = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .unwrap()
        .id;
    scene.objects.retain(|o| o.ship.is_none() || o.id == boat);
    scene.amber = 5000;
    scene.fish = 5000;
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let site = (0..h)
        .rev()
        .flat_map(|y| (0..w).rev().map(move |x| (x, y)))
        .find(|&(x, y)| scene.check_ground(0x34, x, y).is_ok())
        .unwrap();
    scene.order_found(boat, site.0, site.1, 0x34).unwrap();
    for _ in 0..50 * 600 {
        scene.update(0.02, &data, &mut caches);
        if !scene.constructions.is_empty() {
            break;
        }
    }
    assert!(scene.messages.is_empty(), "{:?}", scene.messages);
    assert_eq!(scene.amber, 5000 - economy::building_cost(0x34));
    assert_eq!(scene.fish, 5000);
    let fort = scene.constructions[0].site;
    let i = scene.object_index(fort).unwrap();
    assert_eq!(scene.objects[i].class, ObjectClass::Building(0x34));
    assert!(scene.objects[i].fort_places().iter().all(|p| p.is_empty()));
    assert_eq!(scene.objects[i].record.fields[8], 0xff);
    assert_eq!(scene.home_settlement(i), None);
}

/// A ship's building page (0x4e6980 page 2) keeps each button in its own
/// slot (settlement 0, forts 0x34-0x36 in 1-3, by the technology) and
/// tests no price; the founding order takes no amber either (0x4aa990):
/// the landing party pays on the site (needs `OPENCONQUEST_DATA`).
#[test]
fn ship_building_page_tests_no_price() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.amber = 0;
    let boat = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.as_ref().is_some_and(|s| s.kind == 0))
        .unwrap()
        .id;
    // Only the fort 0x35 allowed: the others' slots stay empty.
    scene.permits.set(1, true);
    scene.permits.set(73, false);
    scene.permits.set(74, false);
    let i = scene.object_index(boat).unwrap();
    let ground = crate::commands::menu(&scene, &scene.objects[i], crate::commands::Page::Ground);
    let at = |k: usize| ground[k].map(|s| (s.command, s.enabled));
    use crate::commands::Command;
    assert_eq!(at(0), Some((Command(0x16), true)));
    assert_eq!(at(1), None);
    assert_eq!(at(2), Some((Command(0x18), true)));
    assert_eq!(at(3), None);
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let site = (0..h)
        .rev()
        .flat_map(|y| (0..w).rev().map(move |x| (x, y)))
        .find(|&(x, y)| scene.check_ground(0x35, x, y).is_ok())
        .unwrap();
    assert!(scene.order_found(boat, site.0, site.1, 0x35).is_ok());
}

/// A plain move to water the exact route (0x55ca10) cannot reach makes no
/// route at first: the ship waits one (`+0x591`), and 25 ticks on plans
/// again, waited (0x4a6150 -> 0x4a6330 -> 0x4a51b0), as near as it gets
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn unreachable_move_waits_then_plans_near() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let t = TILE_SIZE as f32;
    let found = (0..14).find_map(|m| {
        let mut scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).ok()?;
        scene.brain = None;
        let k = scene
            .objects
            .iter()
            .position(|o| scene.is_own(o) && o.ship.is_some())?;
        let o = &scene.objects[k];
        let from = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let deep = crate::objects::deep_draught(o.ship.as_ref().unwrap().kind);
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let goal = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .find(|&(x, y)| {
                scene.nav.is_sea(x, y)
                    && scene.nav.passable(x, y, deep)
                    && scene
                        .nav
                        .find_path_avoiding(from, (x, y), deep, |_| false)
                        .is_none()
            })?;
        let id = scene.objects[k].id;
        Some((scene, id, goal))
    });
    let (mut scene, id, goal) = found.expect("a water cell a ship cannot reach");
    let (gx, gy) = (goal.0 as f32 * t + t / 2.0, goal.1 as f32 * t + t / 2.0);
    assert!(scene.order_move(id, gx, gy));
    let i = scene.object_index(id).unwrap();
    let s = scene.objects[i].ship.as_ref().unwrap();
    assert!(s.path.is_empty());
    assert_eq!(s.pending, Some((gx, gy)));
    assert_eq!(s.waited, crate::clock::TICK);
    for _ in 0..24 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let i = scene.object_index(id).unwrap();
    assert!(scene.objects[i].ship.as_ref().unwrap().path.is_empty());
    scene.update(crate::clock::TICK, &data, &mut caches);
    let i = scene.object_index(id).unwrap();
    let s = scene.objects[i].ship.as_ref().unwrap();
    assert_eq!(s.pending, None);
    assert!(!s.path.is_empty());
}

/// A computer's landing party founds a settlement without the amber
/// (0x421f00 kind 1, side 3 or 4), its amber left at 0 (0x4a8d60); the
/// player's builds nothing without it (needs `OPENCONQUEST_DATA`).
#[test]
fn computer_founds_a_settlement_without_amber() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let found = (0..14).find_map(|m| {
        let scene = Scene::load(&data, &mut caches, &format!("miss{m:03}")).ok()?;
        let ship = scene.objects.iter().find(|o| {
            o.ship.is_some()
                && o.owner.is_some_and(|w| {
                    scene.human.is_none_or(|h| (h.nation, h.slot) != w)
                        && scene.seat_of(w).is_none()
                        && scene.purses.contains_key(&w)
                })
        })?;
        let (id, owner) = (ship.id, ship.owner.unwrap());
        let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
        let site = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .find(|&(x, y)| scene.check_site(id, SETTLEMENT, x, y).is_ok())?;
        Some((scene, id, owner, site))
    });
    let (mut scene, ship, owner, site) = found.expect("a computer's ship and a site");
    let cost = economy::building_cost(SETTLEMENT);
    scene.purses.insert(owner, (cost - 1, 0));
    scene
        .place_building(ship, SETTLEMENT, site.0, site.1, &data, &mut caches)
        .unwrap();
    assert_eq!(scene.purses[&owner].0, 0);
    // The player's own ship: refused.
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let boat = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
    scene.amber = cost - 1;
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let site = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .find(|&(x, y)| scene.check_ground(SETTLEMENT, x, y).is_ok())
        .unwrap();
    let t = TILE_SIZE as f32;
    let i = scene.object_index(boat).unwrap();
    (scene.objects[i].x, scene.objects[i].y) = (site.0 as f32 * t, site.1 as f32 * t);
    assert!(scene.check_site(boat, SETTLEMENT, site.0, site.1).is_ok());
    assert!(scene
        .place_building(boat, SETTLEMENT, site.0, site.1, &data, &mut caches)
        .is_err());
    assert_eq!(scene.amber, cost - 1);
}

/// A town counts the forts it orders going up (0x43b290 `+0x22c..`), one
/// fewer as one is called off (0x2491) or stands (0x2873), and orders no
/// fort past a weighted count of 15 (needs `OPENCONQUEST_DATA`).
#[test]
fn town_counts_forts_going_up() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.default_arming = false;
    scene.amber = 50_000;
    let home = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && matches!(o.class, ObjectClass::Building(SETTLEMENT | TOWN)))
        .unwrap();
    let (hx, hy, home) = ((home.x / 64.0) as i32, (home.y / 64.0) as i32, home.id);
    let mut forts = Vec::new();
    for _ in 0..2 {
        let site = (-BUILD_RADIUS..=BUILD_RADIUS)
            .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
            .find(|&(x, y)| scene.check_site(home, 0x34, x, y).is_ok())
            .expect("room for a fort");
        scene
            .place_building(home, 0x34, site.0, site.1, &data, &mut caches)
            .unwrap();
        forts.push(scene.constructions.last().unwrap().site);
    }
    let up = |s: &Scene| s.towns.going_up.get(&home).copied().unwrap_or_default();
    assert_eq!(up(&scene), [2, 0, 0]);
    // Called off while going up: one fewer.
    scene.halt(forts[0], &data, &mut caches).unwrap();
    assert_eq!(up(&scene), [1, 0, 0]);
    // Standing, it tells its town: none going up, one standing.
    for _ in 0..50 * (crate::clock::ticks(crate::objects::BUILDING_CYCLE[0]) as usize + 2) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(up(&scene), [0, 0, 0]);
    let standing = scene
        .town_buildings(home)
        .into_iter()
        .filter(|&i| matches!(scene.objects[i].class, ObjectClass::Building(0x34..=0x36)))
        .map(|i| scene.objects[i].record.class_id - 0x33)
        .sum::<u32>();
    assert!(standing >= 1);
    // The weighted count: wooden 1, middle 2, stone 3, the new one too.
    scene.towns.going_up.insert(home, [15 - standing - 1, 0, 0]);
    assert!(!scene.forts_full(home, 0x34));
    assert!(scene.forts_full(home, 0x35));
    scene.towns.going_up.insert(home, [15 - standing, 0, 0]);
    assert!(scene.forts_full(home, 0x34));
    let site = (-BUILD_RADIUS..=BUILD_RADIUS)
        .flat_map(|dy| (-BUILD_RADIUS..=BUILD_RADIUS).map(move |dx| (hx + dx, hy + dy)))
        .find(|&(x, y)| scene.check_site(home, 0x34, x, y).is_ok())
        .unwrap();
    assert!(scene
        .place_building(home, 0x34, site.0, site.1, &data, &mut caches)
        .is_err());
}

/// The sounds of channels 5 and 6 (0x50e570 modes 5 and 6): exactly the
/// names of those events' `SND_n` lists, all answering the ambient switch
/// (needs `OPENCONQUEST_DATA`).
#[test]
fn ambient_channel_sounds() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let a = data.sounds.as_ref().unwrap();
    // Mode 5 (event 4: a building's fall by default; 100-102 the land
    // creatures' death, 0x413c66) and mode 6.
    let events = [
        85, 86, 87, 89, 92, 93, 94, 96, 99, 103, 105, 107, 110, 111, 114, 115, 116, 122, 125, 126,
        127, 129, 133, 136, 149, 150, 151, 152, 153, 154, 155, 169, 170, 174, 274, 1, 4, 100, 101,
        102, 88, 95, 106, 128, 146, 147, 148, 271, 272,
    ];
    let mut names: Vec<String> = events
        .iter()
        .flat_map(|&e| openconquest_formats::sound::sound_choices(a, e).unwrap())
        .collect();
    names.sort();
    names.dedup();
    let mut list: Vec<String> = AMBIENT_PLACED.iter().map(|s| s.to_string()).collect();
    list.sort();
    assert_eq!(names, list);
    for n in AMBIENT_PLACED {
        assert_eq!(sound_kind(n), SoundKind::Ambient, "{n}");
    }
}

/// A standing settlement's picture steps its own frame (0x4230f0 state
/// 1: at once on the first tick, then every 75 / n ticks, back to 0 at
/// n), and its upgrade starts the construction from that frame (0x422e30
/// keeps it unless `>= n`; needs `OPENCONQUEST_DATA`).
#[test]
fn upgrade_starts_from_the_standing_frame() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let id = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.class == ObjectClass::Building(SHIPYARD))
        .unwrap()
        .id;
    let i = scene.object_index(id).unwrap();
    // The game's standing shipyard is one frame (always 0); the loop is
    // shown over the picture made three frames long.
    assert_eq!(scene.objects[i].frames.len(), 1);
    let one = scene.objects[i].frames[0].clone();
    scene.objects[i].frames = vec![one.clone(), one.clone(), one];
    let n = 3;
    let delay = crate::objects::BUILDING_CYCLE[1] / n;
    assert_eq!(delay, 25);
    for ticks in 1..=(2 * delay + 2) {
        scene.step_standing_frames();
        // Steps on ticks 1, 1 + delay, 1 + 2 delay, ...
        let steps = 1 + (ticks - 1) / delay;
        assert_eq!(scene.standing_frames[&id].frame, steps % n);
    }
    let frame = scene.standing_frames[&id].frame;
    assert_eq!(frame, 0);
    scene.step_standing_frames();
    let frame = scene.standing_frames[&id].frame;
    assert_eq!(frame, 0);
    for _ in 0..delay {
        scene.step_standing_frames();
    }
    let frame = scene.standing_frames[&id].frame;
    assert_eq!(frame, 1);
    scene.amber = 5000;
    scene.upgrade(id, &data, &mut caches).unwrap();
    let c = scene.construction_at(id).unwrap();
    let nc = c.frames;
    assert_eq!(c.frame, if frame >= nc { 0 } else { frame });
    assert!(!c.started);
    assert!(!scene.standing_frames.contains_key(&id));
}

/// After unloading the boat's task ends and it posts notice 0x139; it
/// does not go back by itself: its zone sends it to the amber still in
/// its cell `+0x797` (0x4c9930 -> 0x4c96e0), port 0 (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn zone_sends_an_unloaded_boat_back() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, deposit, ports) = amber_boat_in_zone(&data, &mut caches);
    let i = scene.object_index(boat).unwrap();
    {
        let s = scene.objects[i].ship.as_mut().unwrap();
        s.cargo = 0.0;
        s.cargo_kind = None;
        s.path.clear();
        s.task = Some(Task::Gather {
            deposit,
            phase: GatherPhase::Unloading(ports[0]),
        });
    }
    scene.run_task(i, 0.0, &data, &mut caches);
    let s = scene.objects[i].ship.as_ref().unwrap();
    assert!(s.task.is_none());
    let notes: Vec<(u32, u32)> = scene.gather_zones[0]
        .notices
        .iter()
        .map(|n| (n.code, n.ship))
        .collect();
    assert_eq!(notes, [(0x139, boat)]);
    scene.update(crate::clock::TICK, &data, &mut caches);
    let i = scene.object_index(boat).unwrap();
    let s = scene.objects[i].ship.as_ref().unwrap();
    assert!(matches!(
        s.task,
        Some(Task::Gather {
            deposit: d,
            phase: GatherPhase::ToDeposit,
        }) if d == deposit
    ));
    assert_eq!(s.home_port, 0);
}

/// 0x49a330 counts a berth's neighbours on the map (0x49a1d0 / 0x597900:
/// 3 at a corner, 5 on an edge, 8 inside) that are water, and those
/// holding a merchant of the player sailing in to unload; a merchant
/// waiting by a crowded berth steps aside 300 ticks after coming within
/// a cell of it and gives its port up after 350, posting notice 0x138
/// (0x4adef0 step 1; needs `OPENCONQUEST_DATA`).
#[test]
fn crowded_berth_wait() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, deposit, ports) = amber_boat_in_zone(&data, &mut caches);
    let p = scene.object_index(ports[0]).unwrap();
    let b = scene.berth_cell(p).unwrap();
    let owner = scene.objects[p].owner;
    let water: Vec<(i32, i32)> = (-1..=1)
        .flat_map(|dy| (-1..=1).map(move |dx| (b.0 + dx, b.1 + dy)))
        .filter(|&c| c != b && scene.sea_cell(c, false))
        .collect();
    assert_eq!(scene.berth_crowd(b, owner), (water.len() as i32, 0));
    // Fill the free water about the berth but one cell with merchants
    // coming in to unload: crowded.
    let t = TILE_SIZE as f32;
    let free: Vec<(i32, i32)> = water
        .iter()
        .copied()
        .filter(|&c| scene.slot_ship(c).is_none())
        .collect();
    for &(x, y) in &free[1..] {
        let id = scene
            .spawn_ship(owner.unwrap(), 1, x, y, &data, &mut caches)
            .unwrap();
        let k = scene.object_index(id).unwrap();
        let s = scene.objects[k].ship.as_mut().unwrap();
        s.task = Some(Task::Gather {
            deposit,
            phase: GatherPhase::ToPort(ports[0]),
        });
    }
    let (a, n) = scene.berth_crowd(b, owner);
    assert_eq!((a, n), (water.len() as i32, free.len() as i32 - 1));
    // The boat waits by the berth with cargo, its timer started 349 ticks
    // ago: it gives its port up.
    let w = free[0];
    let i = scene.object_index(boat).unwrap();
    scene.ticks = 1000;
    let now = scene.ticks;
    {
        let o = &mut scene.objects[i];
        o.x = w.0 as f32 * t + t / 2.0;
        o.y = w.1 as f32 * t + t / 2.0;
        let s = o.ship.as_mut().unwrap();
        s.path.clear();
        s.cargo = 100.0;
        s.cargo_kind = Some(Resource::Amber);
        s.home_port = ports[0];
        s.task = Some(Task::Gather {
            deposit,
            phase: GatherPhase::ToPort(ports[0]),
        });
        s.berth_wait = now - gather::BERTH_GIVE_UP;
    }
    scene.run_berth_waits();
    let s = scene.objects[i].ship.as_ref().unwrap();
    assert_eq!(s.home_port, 0);
    assert_eq!(s.berth_wait, 0);
    let notes: Vec<(u32, u32, u32)> = scene.gather_zones[0]
        .notices
        .iter()
        .map(|n| (n.code, n.ship, n.class))
        .collect();
    assert_eq!(notes, [(0x138, boat, 0)]);
}

/// miss000's merchant (fleet 1) fishes in its fleet's gather zone (order
/// 0x78, flags 0, 0x47a570); the zone's state answers its query 0x79
/// (0x4c8da0) with deposits left but no port list (player 16's class-0x40
/// structures are pruned from it, 0x4c65c0): result 0xb, which leaves
/// the member's state (needs `OPENCONQUEST_DATA`).
#[test]
fn brain_gather_zone() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    for _ in 0..25 * 10 {
        scene.update(crate::clock::TICK, &data, &mut caches);
    }
    let merchant = scene.brain.as_ref().unwrap().fleets[1].ships[0];
    let z = scene.ship_zone(merchant).unwrap();
    let zone = &scene.gather_zones[z];
    assert_eq!(zone.group, gather::ZoneGroup::Brain(1));
    assert_eq!(zone.class, Resource::Fish);
    assert_eq!(zone.flags, 0);
    assert!(!zone.deposits.is_empty());
    assert_eq!(zone.owner, Some((4, 0)));
    let status = scene.zone_status(gather::ZoneGroup::Brain(1));
    assert_eq!(status, Some((0x1f5, 0xb)));
}

/// A zone ship lying idle without a notice is sent again only by the
/// re-plan (0x4ca290), when the group's count passes 85 (0x4ca8d0): to
/// the same amber (0x4c96e0) (needs `OPENCONQUEST_DATA`).
#[test]
fn zone_replans_its_idle_ships() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, deposit, _) = amber_boat_in_zone(&data, &mut caches);
    let i = scene.object_index(boat).unwrap();
    {
        let s = scene.objects[i].ship.as_mut().unwrap();
        s.task = None;
        s.path.clear();
    }
    scene.gather_zones[0].elapsed = 0;
    let idle = |s: &Scene| {
        s.objects[s.object_index(boat).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .task
            .is_none()
    };
    // Calls 0..=85 leave it; the 87th plans it (`+8 > 0x55`), and its
    // command is delivered at that tick's end.
    for _ in 0..86 {
        scene.update(crate::clock::TICK, &data, &mut caches);
        assert!(idle(&scene));
    }
    scene.update(crate::clock::TICK, &data, &mut caches);
    let s = scene.objects[scene.object_index(boat).unwrap()]
        .ship
        .as_ref()
        .unwrap();
    assert!(matches!(s.task, Some(Task::Gather { deposit: d, .. }) if d == deposit));
}

/// A brain fleet's move is judged by 0x4ccf60: each ship within the
/// order's tolerance (0x557440) of its cell in the formation is in; all
/// in, done (0x1f5 / 0xb); none in and none under way, failed (0x1f5 /
/// 1); else under way (500). A ship that joins is planned in anew
/// (0x4cc6a0 -> 0x4cc970) (needs `OPENCONQUEST_DATA`).
#[test]
fn fleet_move_status() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    past_hold(&mut scene);
    // miss000's guards (fleet 0), two ships.
    let ships = scene.brain.as_ref().unwrap().fleets[0].ships.clone();
    assert_eq!(ships.len(), 2);
    let t = TILE_SIZE as f32;
    let at = |s: &Scene, id: u32| {
        let o = &s.objects[s.object_index(id).unwrap()];
        ((o.x / t).floor() as i32, (o.y / t).floor() as i32)
    };
    let here = at(&scene, ships[0]);
    scene.fleet_move(0, here, 1);
    let slots = scene.brain.as_ref().unwrap().fleets[0]
        .move_order
        .clone()
        .unwrap()
        .slots;
    let slot = |id: u32| slots.iter().find(|s| s.0 == id).unwrap().1;
    let put = |s: &mut Scene, id: u32, c: (i32, i32)| {
        let i = s.object_index(id).unwrap();
        s.objects[i].x = c.0 as f32 * t + t / 2.0;
        s.objects[i].y = c.1 as f32 * t + t / 2.0;
        s.objects[i].ship.as_mut().unwrap().path.clear();
    };
    // Both on their cells: done.
    for &id in &ships {
        put(&mut scene, id, slot(id));
    }
    assert_eq!(scene.move_status(0), Some((0x1f5, 0xb)));
    // One two cells off and lying still: some in, none under way: done
    // too.
    let c = slot(ships[1]);
    put(&mut scene, ships[1], (c.0 + 2, c.1));
    assert_eq!(scene.move_status(0), Some((0x1f5, 0xb)));
    // Both off and still: failed.
    let c0 = slot(ships[0]);
    put(&mut scene, ships[0], (c0.0 + 2, c0.1 + 2));
    assert_eq!(scene.move_status(0), Some((0x1f5, 1)));
    // One under way: going.
    let i = scene.object_index(ships[0]).unwrap();
    scene.objects[i]
        .ship
        .as_mut()
        .unwrap()
        .path
        .push_back((0.0, 0.0));
    assert_eq!(scene.move_status(0), Some((500, 0)));
    // The tolerance: 2 counts the ship two cells off in.
    scene.brain.as_mut().unwrap().fleets[0]
        .move_order
        .as_mut()
        .unwrap()
        .tol = 2;
    scene.objects[i].ship.as_mut().unwrap().path.clear();
    put(&mut scene, ships[1], (c.0 + 2, c.1));
    put(&mut scene, ships[0], (c0.0 + 2, c0.1));
    assert_eq!(scene.move_status(0), Some((0x1f5, 0xb)));
    // A ship that joins has the formation planned anew: every ship sent.
    scene.outbox.clear();
    scene.brain.as_mut().unwrap().fleets[0]
        .joined
        .push(ships[1]);
    scene.run_move_orders();
    let sent: Vec<u32> = scene.outbox.iter().filter_map(|o| o.ship()).collect();
    assert_eq!(sent.len(), 2);
}

/// 0x4c7fc0's arrays come off the stack (0x5878f0): a zone's only port
/// whose berth is not water leaves its object slot holding the stack
/// probe's return address 0x4c8068, read as an object: the deposit is
/// rated for the id at 0x4c80b4, 0xffff6c85, score 1 (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn zone_rating_reads_the_stack_probe() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, _boat, deposit, ports) = amber_boat_in_zone(&data, &mut caches);
    let d = scene.object_index(deposit).unwrap();
    let f = &scene.objects[d].record.fields;
    let cell = (f[6] as i32, f[7] as i32);
    let rec = |port| gather::Rec {
        id: deposit,
        cell,
        port,
        pscore: 0,
        amount: 100,
    };
    // The port as it stands: its own id, score 1.
    let mut recs = [rec(7)];
    assert_eq!(scene.zone_rate(0, &mut recs), 1);
    assert_eq!((recs[0].port, recs[0].pscore), (ports[0], 1));
    // Face it to land.
    let p = scene.object_index(ports[0]).unwrap();
    let land = (0..8u32)
        .find(|&k| {
            scene.objects[p].record.fields[10] = k;
            !scene.sea_cell(scene.berth_cell(p).unwrap(), false)
        })
        .unwrap();
    scene.objects[p].record.fields[10] = land;
    let mut recs = [rec(7)];
    assert_eq!(scene.zone_rate(0, &mut recs), 1);
    assert_eq!((recs[0].port, recs[0].pscore), (gather::PROBE_PORT_ID, 1));
}

/// 0x4c5ec0 with an amber that has no approach (0x4c76f0 -1) reads the
/// word before the wavefront, a heap header: with one candidate it is
/// never compared and the amber is sent; with two the pick is not known
/// and 0x4c9270 sends nothing (needs `OPENCONQUEST_DATA`).
#[test]
fn zone_pick_without_an_approach() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, deposit, _ports) = amber_boat_in_zone(&data, &mut caches);
    let i = scene.object_index(boat).unwrap();
    let d = scene.object_index(deposit).unwrap();
    let f = &scene.objects[d].record.fields;
    let cell = (f[6] as i32, f[7] as i32);
    // A cell of land with land all about it.
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let inland = (1..h - 1)
        .flat_map(|y| (1..w - 1).map(move |x| (x, y)))
        .find(|&(x, y)| {
            (-1..=1).all(|dy| (-1..=1).all(|dx| !scene.sea_cell((x + dx, y + dy), false)))
        })
        .unwrap();
    let amber = gather::Cand {
        cell,
        amount: 100,
        id: deposit,
    };
    let dry = gather::Cand {
        cell: inland,
        amount: 100,
        id: deposit,
    };
    scene.outbox.clear();
    assert_eq!(scene.zone_one(0, i, &[amber], false), 0x19);
    assert_eq!(scene.outbox.len(), 1);
    scene.outbox.clear();
    assert_eq!(scene.zone_one(0, i, &[amber, dry], false), 0);
    assert_eq!(scene.zone_one(0, i, &[dry, amber], false), 0);
    assert!(scene.outbox.is_empty());
}

/// 0x4c6640 starts its least at the list's first berth (0x4c6520): a
/// first entry that is not the player's port gives -1 and the word
/// before the wavefront is read (not known): with a port reached no port
/// is picked; with the player's own first, the other is passed over
/// (miss003's own port and the Persians'; needs `OPENCONQUEST_DATA`).
#[test]
fn zone_port_pick_with_a_foreign_first_port() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss008").unwrap();
    past_hold(&mut scene);
    let (own, foreign) = (18, 29);
    let p = scene.object_index(own).unwrap();
    let (bx, by) = scene.berth_cell(p).unwrap();
    let owner = scene.objects[p].owner.unwrap();
    // (The player's port lies asleep at the start: woken.)
    scene.objects[p].asleep = None;
    let boat = scene
        .spawn_ship(owner, 0, bx, by, &data, &mut caches)
        .unwrap();
    let deposit = scene
        .objects
        .iter()
        .find(|o| gather::deposit_class(o).is_some())
        .unwrap()
        .id;
    scene.order_gather_at(&[boat], deposit, 0).unwrap();
    let i = scene.object_index(boat).unwrap();
    scene.gather_zones[0].ports = Some(vec![own, foreign]);
    assert_eq!(scene.zone_port_pick(0, Some(i)), Some(own));
    scene.gather_zones[0].ports = Some(vec![foreign, own]);
    assert_eq!(scene.zone_port_pick(0, Some(i)), None);
}

/// 0x4c7fc0 over two ports (miss008's own two). The last port's berth
/// not water: its score holds the probe's 0x4c8090, not reached by the
/// walk: the first port is taken. The same port first: the least stays
/// 0 (set only by the first deposit's first port), so one deposit has
/// no draws (`T` 0); with a second deposit nearer than the first the
/// walk starts at the failed port's score, what 0x4c65c0 left on the
/// stack, and the rating gives up (0) (needs `OPENCONQUEST_DATA`).
#[test]
fn zone_rating_over_two_ports_with_one_failed() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss008").unwrap();
    past_hold(&mut scene);
    let ports = [18, 2];
    for id in ports {
        let p = scene.object_index(id).unwrap();
        scene.objects[p].asleep = None;
    }
    let p = scene.object_index(ports[0]).unwrap();
    let (bx, by) = scene.berth_cell(p).unwrap();
    let owner = scene.objects[p].owner.unwrap();
    let boat = scene
        .spawn_ship(owner, 0, bx, by, &data, &mut caches)
        .unwrap();
    let t = TILE_SIZE as f32;
    let mut shoals: Vec<(u32, (i32, i32))> = scene
        .objects
        .iter()
        .filter(|o| gather::deposit_class(o) == Some(Resource::Fish))
        .map(|o| (o.id, ((o.x / t).floor() as i32, (o.y / t).floor() as i32)))
        .filter(|&(_, (x, y))| {
            x >= 0 && y >= 0 && x < scene.nav.width as i32 && y < scene.nav.height as i32
        })
        .collect();
    shoals.sort_by_key(|&(_, (x, y))| (x - bx).abs().max((y - by).abs()));
    scene.order_gather_at(&[boat], shoals[0].0, 0).unwrap();
    let rec = |(id, cell): (u32, (i32, i32))| gather::Rec {
        id,
        cell,
        port: 7,
        pscore: 0,
        amount: 100,
    };
    let p = scene.object_index(ports[1]).unwrap();
    let land = (0..8u32)
        .find(|&k| {
            scene.objects[p].record.fields[10] = k;
            !scene.sea_cell(scene.berth_cell(p).unwrap(), false)
        })
        .unwrap();
    scene.objects[p].record.fields[10] = land;
    scene.gather_zones[0].ports = Some(ports.to_vec());
    // Alone with the one port rating it, a deposit's value is its `s`.
    let mut rated: Vec<(_, i32)> = Vec::new();
    for &sh in &shoals {
        let mut recs = [rec(sh)];
        assert_eq!(scene.zone_rate(0, &mut recs), 1);
        if recs[0].port == ports[0] && rated.iter().all(|r| r.1 != recs[0].pscore) {
            rated.push((sh, recs[0].pscore));
        }
    }
    assert!(rated.len() >= 2);
    rated.sort_by_key(|r| -r.1);
    let (far, near) = (rated[0].0, rated[1].0);
    // The failed port first.
    scene.gather_zones[0].ports = Some(vec![ports[1], ports[0]]);
    let mut recs = [rec(shoals[0])];
    assert_eq!(scene.zone_rate(0, &mut recs), 1);
    assert_eq!((recs[0].port, recs[0].pscore), (7, 0));
    let mut recs = [rec(far), rec(near)];
    assert_eq!(scene.zone_rate(0, &mut recs), 0);
    assert_eq!((recs[1].port, recs[1].pscore), (7, 0));
}

/// A zone's escort at the re-plan (0x4ca290 -> 0x4c6270): its ward marked
/// with a blow (0x4bf3aa, `+0x691` / `+0x692`) sends it after the giver
/// (0x4d0140, the attack on a ship; state 0xb) and the ward forgets the
/// blow at the next tick (0x4cf760, command 0x28 sub 4); after the
/// attacker and more than 2 cells away it follows its ward again (state
/// 10); its entry goes when it leaves the group, and stays when the ward
/// does (needs `OPENCONQUEST_DATA`).
#[test]
fn zone_escort_turns_on_its_wards_attacker() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, _deposit, _ports) = amber_boat_in_zone(&data, &mut caches);
    let i = scene.object_index(boat).unwrap();
    let owner = scene.objects[i].owner.unwrap();
    let t = TILE_SIZE as f32;
    let (bx, by) = (
        (scene.objects[i].x / t).floor() as i32,
        (scene.objects[i].y / t).floor() as i32,
    );
    let near = (-2..=2)
        .flat_map(|dy| (-2..=2).map(move |dx| (bx + dx, by + dy)))
        .find(|&c| c != (bx, by) && scene.sea_cell(c, false) && scene.slot_ship(c).is_none())
        .unwrap();
    let escort = scene
        .spawn_ship(owner, 2, near.0, near.1, &data, &mut caches)
        .unwrap();
    let entry = gather::Escort {
        ship: escort,
        ward: boat,
        state: 10,
        target: 0,
    };
    scene.gather_zones[0].ships.push(escort);
    scene.gather_zones[0].escorts = vec![entry];
    let i = scene.object_index(boat).unwrap();
    scene.objects[i].ship.as_mut().unwrap().blow = Some(0x777);
    scene.outbox.clear();
    scene.gather_zones[0].elapsed = 0x56;
    scene.run_gather_zones();
    let e = scene.gather_zones[0].escorts[0];
    assert_eq!((e.state, e.target), (0xb, 0x777));
    assert!(scene.outbox.contains(&Order::ClearBlow { ship: boat }));
    assert!(scene.outbox.contains(&Order::Attack {
        ship: escort,
        target: 0x777
    }));
    // The clear lands at the tick's turn.
    scene.update(crate::clock::TICK, &data, &mut caches);
    let i = scene.object_index(boat).unwrap();
    assert_eq!(scene.objects[i].ship.as_ref().unwrap().blow, None);
    // After the attacker, far from its ward: following again.
    let k = scene.object_index(escort).unwrap();
    let far = (0..scene.nav.height as i32)
        .flat_map(|y| (0..scene.nav.width as i32).map(move |x| (x, y)))
        .find(|&(x, y)| {
            (x - bx).abs().max((y - by).abs()) > 3
                && scene.sea_cell((x, y), false)
                && scene.slot_ship((x, y)).is_none()
        })
        .unwrap();
    scene.objects[k].x = far.0 as f32 * t + t / 2.0;
    scene.objects[k].y = far.1 as f32 * t + t / 2.0;
    scene.gather_zones[0].escorts[0].state = 0xb;
    scene.outbox.clear();
    scene.gather_zones[0].elapsed = 0x56;
    scene.run_gather_zones();
    let e = scene.gather_zones[0].escorts[0];
    assert_eq!((e.state, e.target), (10, 0));
    assert!(scene.outbox.contains(&Order::Follow {
        ship: escort,
        ward: boat,
        range: 2
    }));
    // The ward out of the group: the entry is kept, nothing sent.
    scene.gather_zones[0].ships.retain(|&s| s != boat);
    scene.outbox.clear();
    scene.gather_zones[0].elapsed = 0x56;
    scene.run_gather_zones();
    assert_eq!(scene.gather_zones[0].escorts.len(), 1);
    assert!(scene.outbox.iter().all(|o| o.ship() != Some(escort)));
    // An old save's entry, `[escort, ward, state]`, reads with no target.
    let old: gather::Escort = serde_json::from_str("[5, 6, 10]").unwrap();
    assert_eq!(
        old,
        gather::Escort {
            ship: 5,
            ward: 6,
            state: 10,
            target: 0
        }
    );
}

/// The gather click's key flags (0x49d3c0): Shift on the same selection's
/// zone of the code's class only makes the deposit known (0x4c6e30), no
/// ship sent; of another class nothing; Shift on a new selection makes a
/// 1 x 1 zone, and any key flag the zone's first deposit only (group
/// `+0x10`); Ctrl keeps the 3 x 3 zone (needs `OPENCONQUEST_DATA`).
#[test]
fn gather_click_key_flags() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, deposit, _ports) = amber_boat_in_zone(&data, &mut caches);
    let other = scene
        .objects
        .iter()
        .find(|o| o.id != deposit && gather::deposit_class(o) == Some(Resource::Amber))
        .unwrap()
        .id;
    let fish = scene
        .objects
        .iter()
        .find(|o| gather::deposit_class(o) == Some(Resource::Fish))
        .unwrap()
        .id;
    let before = scene.gather_zones[0].clone();
    scene.outbox.clear();
    scene.order_gather_at(&[boat], other, 4).unwrap();
    assert_eq!(scene.gather_zones.len(), 1);
    assert_eq!(scene.gather_zones[0].group, before.group);
    assert_eq!(scene.gather_zones[0].rect, before.rect);
    let mut known = before.deposits.clone();
    known.push(other);
    assert_eq!(scene.gather_zones[0].deposits, known);
    assert!(scene.outbox.is_empty());
    // A shoal on an amber zone: nothing.
    scene.order_gather_at(&[boat], fish, 4).unwrap();
    assert_eq!(scene.gather_zones[0].deposits, known);
    assert!(scene.outbox.is_empty());
    // Shift with another selection: a zone of the amber's cell alone.
    let d = scene.object_index(deposit).unwrap();
    let f = &scene.objects[d].record.fields;
    let cell = (f[6] as i32, f[7] as i32);
    let i = scene.object_index(boat).unwrap();
    let owner = scene.objects[i].owner.unwrap();
    let t = TILE_SIZE as f32;
    let at = (
        (scene.objects[i].x / t).floor() as i32,
        (scene.objects[i].y / t).floor() as i32,
    );
    let near = (-2..=2)
        .flat_map(|dy| (-2..=2).map(move |dx| (at.0 + dx, at.1 + dy)))
        .find(|&c| c != at && scene.sea_cell(c, false) && scene.slot_ship(c).is_none())
        .unwrap();
    let mate = scene
        .spawn_ship(owner, 0, near.0, near.1, &data, &mut caches)
        .unwrap();
    scene.order_gather_at(&[boat, mate], deposit, 4).unwrap();
    let z = scene.gather_zones.last().unwrap();
    assert_eq!(z.rect, (cell.0, cell.1, 1, 1));
    assert!(z.first_only);
    // Ctrl: 3 x 3, the first deposit only.
    scene.order_gather_at(&[boat], deposit, 8).unwrap();
    let z = scene.gather_zones.last().unwrap();
    assert_eq!(z.rect, (cell.0 - 1, cell.1 - 1, 3, 3));
    assert!(z.first_only);
}

/// The zone buttons' drag sends its top-left cell and its size in cells
/// (0x51cdd0 -> 0x517c00 -> 0x49d790), the cells toward zero and not
/// clipped to the map (needs `OPENCONQUEST_DATA`).
#[test]
fn gather_area_is_the_dragged_cells() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, _deposit, _ports) = amber_boat_in_zone(&data, &mut caches);
    let w = scene.nav.width as f32 * 64.0;
    scene
        .order_gather_area(
            &[boat],
            (w + 100.0, 200.0),
            (w - 70.0, 130.0),
            Resource::Amber,
        )
        .unwrap();
    let z = scene.gather_zones.last().unwrap();
    let mw = scene.nav.width as i32;
    assert_eq!(z.rect, (mw - 2, 2, 4, 2));
    assert_eq!(z.flags, 0x10000);
}

/// An order's port list is kept as given, each port once (0x4c6400),
/// none leaving it null for 0x4c6c30 (needs `OPENCONQUEST_DATA`).
#[test]
fn zone_takes_the_orders_ports() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, _deposit, ports) = amber_boat_in_zone(&data, &mut caches);
    let group = scene.gather_zones[0].group;
    let rect = scene.gather_zones[0].rect;
    scene.start_zone_ports(
        group,
        vec![boat],
        rect,
        Resource::Amber,
        0,
        false,
        vec![ports[0], ports[0]],
    );
    assert_eq!(scene.gather_zones[0].ports, Some(vec![ports[0]]));
    scene.start_zone(group, vec![boat], rect, Resource::Amber, 0, false);
    // (Null, then filled by the start's dispatch, 0x4c6c30.)
    assert_eq!(scene.gather_zones[0].ports, Some(ports.clone()));
}

/// The zone's other notices (0x4c9e30): 0x133, a ship wrecked, keeps its
/// cell (`+0x35`), sent to every ship at each re-plan as command 0x29 sub
/// 2 (its path memory, 0x4a7230); 0x137, a ship leaving the group, drops
/// its escort entry (0x4c61d0); 0x145, a new port of the player's, lists
/// it (0x4c6400) and sends a human's unloading merchants to the zone's
/// pick at once (0x4c7b80) (needs `OPENCONQUEST_DATA`).
#[test]
fn zone_hears_wrecks_leavers_and_new_ports() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let (mut scene, boat, deposit, ports) = amber_boat_in_zone(&data, &mut caches);
    let i = scene.object_index(boat).unwrap();
    let owner = scene.objects[i].owner.unwrap();
    let t = TILE_SIZE as f32;
    let at = (
        (scene.objects[i].x / t).floor() as i32,
        (scene.objects[i].y / t).floor() as i32,
    );
    let near = (-2..=2)
        .flat_map(|dy| (-2..=2).map(move |dx| (at.0 + dx, at.1 + dy)))
        .find(|&c| c != at && scene.sea_cell(c, false) && scene.slot_ship(c).is_none())
        .unwrap();
    let escort = scene
        .spawn_ship(owner, 2, near.0, near.1, &data, &mut caches)
        .unwrap();
    scene.gather_zones[0].ships.push(escort);
    scene.gather_zones[0].escorts = vec![gather::Escort {
        ship: escort,
        ward: boat,
        state: 10,
        target: 0,
    }];
    // The boat wrecked on its cell.
    scene.post_wreck(boat);
    scene.run_gather_zones();
    let w = scene.nav.width as i32;
    assert_eq!(scene.gather_zones[0].wrecks, [(at.1 * w + at.0) as u32]);
    scene.outbox.clear();
    scene.gather_zones[0].elapsed = 0x56;
    scene.run_gather_zones();
    for ship in [boat, escort] {
        assert!(scene.outbox.contains(&Order::Remember { ship, cell: at }));
    }
    scene.update(crate::clock::TICK, &data, &mut caches);
    let i = scene.object_index(escort).unwrap();
    assert!(scene.objects[i]
        .ship
        .as_ref()
        .unwrap()
        .remembered
        .contains(&at));
    // A new port, the boat sailing in to unload: re-sent.
    {
        let i = scene.object_index(boat).unwrap();
        let s = scene.objects[i].ship.as_mut().unwrap();
        s.cargo = 100.0;
        s.cargo_kind = Some(Resource::Amber);
        s.task = Some(Task::Gather {
            deposit,
            phase: GatherPhase::ToPort(ports[0]),
        });
    }
    scene.gather_zones[0].ports = None;
    scene.outbox.clear();
    scene.post_new_port(ports[0]);
    scene.run_gather_zones();
    assert_eq!(scene.gather_zones[0].ports, Some(vec![ports[0]]));
    assert!(scene.outbox.contains(&Order::Unload {
        ship: boat,
        port: ports[0]
    }));
    // The escort leaves for a group of its own: its entry goes.
    scene.leave_zones(&[escort]);
    assert_eq!(scene.gather_zones[0].escorts.len(), 1);
    scene.run_gather_zones();
    assert!(scene.gather_zones[0].escorts.is_empty());
}

/// miss004's western post (0x451890) waits at (5, 3) only with exactly
/// two royal ships: with two it goes 5, 7 (the watch), with three 5, 6
/// (the move again, back to 0 on arriving) and never waits (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss004_posts_wait_on_their_ship_count() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    for n in [2usize, 3] {
        let mut scene = Scene::load(&data, &mut caches, "miss004").unwrap();
        let near: Vec<(i32, i32)> = (-2..=2)
            .flat_map(|dy| (-2..=2).map(move |dx| (5 + dx, 3 + dy)))
            .filter(|&c| scene.sea_cell(c, false) && scene.slot_ship(c).is_none())
            .take(n)
            .collect();
        for &(x, y) in &near {
            let id = scene
                .spawn_ship((3, 3), 2, x, y, &data, &mut caches)
                .unwrap();
            scene.brain.as_mut().unwrap().fleets[2].ships.push(id);
        }
        let mut states = Vec::new();
        for _ in 0..1500 {
            scene.update(0.04, &data, &mut caches);
            let f = &scene.brain.as_ref().unwrap().fleets[2];
            if states.last() != Some(&f.state) {
                states.push(f.state);
            }
        }
        let want: &[u32] = if n == 2 {
            &[0, 5, 7]
        } else {
            &[0, 5, 6, 0, 5, 6]
        };
        assert_eq!(&states[..want.len()], want, "{n}: {states:?}");
        assert!(!states.contains(&if n == 2 { 6 } else { 7 }), "{states:?}");
    }
}

/// miss004's raiders as the original's 0x451c60 and 0x4528a0: the brain
/// draws nothing at the start; the northern raiders' first wait is 2250
/// (`+0x114`) and their first turn draws only the sweep (rand % 3); the
/// eastern post seeing the player calls the southern raiders to 0x5a once,
/// which attack the area of the post's zone, and they at home call the
/// northern raiders to 0x5a, which attack the area of their zone (index
/// 1: past the end of 0x5af1a8, zeros); the northern raiders' attacks
/// draw rand % 6 + 1 once (needs `OPENCONQUEST_DATA`).
#[test]
fn miss004_raiders_call_each_other_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss004").unwrap();
    const EAST: usize = 3;
    const NORTH: usize = 4;
    const SOUTH: usize = 13;
    let mut rng = scene.brain.as_ref().unwrap().rng.clone();
    assert_eq!(scene.brain.as_ref().unwrap().fields[0], 2250);
    // The northern raiders run at tick 20, the first member to draw.
    for _ in 0..20 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!(b.fleets[NORTH].state, 5);
    assert_eq!((b.fields[0], b.fields[1]), (2250, rng.next() % 3));
    // Then the southern raiders, at the same tick, their own wait.
    assert_eq!(b.fields[2], (rng.next() % 3 + 5) * 1500);
    assert_eq!(b.rng.0, rng.0, "no other draw");
    // The eastern post at war in its second zone; the southern raiders
    // last saw their second zone.
    while scene.brain.as_ref().unwrap().tick % 50 != 0 {
        scene.run_brain();
    }
    {
        let b = scene.brain.as_mut().unwrap();
        b.fleets[EAST].state = 20;
        b.fleets[EAST].started = false;
        b.fleets[EAST].area = Some((0, 0, 12, 7));
        b.fleets[SOUTH].area = Some((13, 23, 13, 12));
    }
    // Tick 15: the post attacks (5, 7, 7, 8), its zones at the southern
    // raiders' index 1, and calls them in.
    for _ in 0..15 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!(b.fleets[EAST].last_area, (0, 0, 12, 7));
    assert_eq!((b.fleets[SOUTH].state, b.fields[3]), (90, 1));
    // Tick 20: the southern raiders attack the post's zone (index 1).
    for _ in 0..5 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!(b.fleets[SOUTH].last_area, (0, 0, 12, 7));
    // At home they call the northern raiders, which attack at the
    // southern raiders' index (1, zeros); in their sweep's attack they
    // draw once.
    {
        let b = scene.brain.as_mut().unwrap();
        b.fleets[SOUTH].state = 20;
        b.fleets[SOUTH].started = false;
    }
    for _ in 0..50 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!((b.fleets[NORTH].state, b.fields[4]), (90, 1));
    for _ in 0..50 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!(b.fleets[NORTH].last_area, (0, 0, 0, 0));
    let mut rng = b.rng.clone();
    {
        let b = scene.brain.as_mut().unwrap();
        b.fleets[NORTH].state = 50;
        b.fleets[NORTH].started = false;
        b.fleets[NORTH].area = Some((14, 0, 5, 15));
    }
    for _ in 0..50 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!(b.fields[5], rng.next() % 6 + 1);
    assert_eq!(b.fleets[NORTH].last_area, (14, 0, 5, 15));
}

/// miss005's barbarian raiders as the original's 0x455620: their wait
/// drawn in state 0 ((7, 6, 4) + rand & 3, x 1500); home at (1, 23) with
/// two ships or fewer; a fort of the player's in (13, 25, 9, 7) found
/// before their turn; ready to raid with it found when they set out and
/// the draw 0 or 1, they attack its area and send the guard there (whose
/// attack takes it too), else they draw their way and sail (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn miss005_barbarian_raiders_as_the_original() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    const GUARD: usize = 1;
    const RAIDERS: usize = 3;
    let mut rng = scene.brain.as_ref().unwrap().rng.clone();
    // (0, 3) runs at tick 15, the first member to draw.
    for _ in 0..15 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!(b.fleets[RAIDERS].state, 5);
    assert_eq!(b.fleets[RAIDERS].vars[0], (6 + (rng.next() & 3)) * 1500);
    assert_eq!(b.rng.0, rng.0);
    // No ships: home.
    for _ in 0..50 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!(b.fleets[RAIDERS].state, 5);
    let home = b.fleets[RAIDERS].move_order.as_ref().map(|m| m.to);
    assert_eq!(home, Some((1, 23)));
    // A fort of the player's at (14, 27).
    let mut fort = scene
        .site_preview(0x35, 14, 27, 0, &data, &mut caches)
        .unwrap();
    fort.id = scene.objects.iter().map(|o| o.id).max().unwrap() + 1;
    assert!(scene.is_own(&fort) && fort.combat.is_some());
    assert_eq!(fort.cell, Some((14, 27)));
    scene.objects.push(fort);
    for _ in 0..50 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!((b.fields[2], b.fields[4]), (1, 1));
    // Ready to raid, the fort found as they set out.
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..40 {
        {
            let b = scene.brain.as_mut().unwrap();
            b.fleets[RAIDERS].state = 40;
            b.fleets[RAIDERS].started = false;
            b.fleets[GUARD].state = 0;
            b.fields[0] = 0;
            b.fields[3] = 1;
        }
        for _ in 0..49 {
            scene.run_brain();
        }
        let mut rng = scene.brain.as_ref().unwrap().rng.clone();
        scene.run_brain();
        let b = scene.brain.as_ref().unwrap();
        let draw = rng.next() & 3;
        assert_eq!(b.fields[0], draw);
        if draw <= 1 {
            assert_eq!(b.fleets[RAIDERS].state, 40);
            assert_eq!(b.fleets[RAIDERS].last_area, (13, 25, 9, 7));
            assert_eq!(b.fleets[GUARD].state, 50);
            // The guard's turn, at 5.
            for _ in 0..40 {
                scene.run_brain();
            }
            let b = scene.brain.as_ref().unwrap();
            assert_eq!(b.fleets[GUARD].last_area, (13, 25, 9, 7));
            while scene.brain.as_ref().unwrap().tick % 50 != 15 {
                scene.run_brain();
            }
        } else {
            let way = rng.next() & 1;
            assert_eq!((b.fleets[RAIDERS].state, b.fields[1]), (50, way));
        }
        seen.insert(draw <= 1);
    }
    assert_eq!(seen.len(), 2, "both ways taken");
}

/// A fort a computer's town orders comes armed (0x428a70 sets the town
/// command 0x20's byte `+0xd`, 0x441ba0 copies it to the record, 0x43b290
/// runs 0x43f440): miss005's town 2 orders a small fort holding two of
/// item 4 in stock (60, 80): it takes the better for item 4, item 5 and
/// item 6 if it may have them now, else their fallback 4 (the stock's
/// other, then bought), and the fallbacks from the last but one while the
/// units weigh under 4; the bought ones are whole and paid (needs
/// `OPENCONQUEST_DATA`).
#[test]
fn a_computers_town_arms_its_forts() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    scene.brain = None;
    let owner = scene.objects[scene.object_index(2).unwrap()].owner.unwrap();
    assert!(!scene.is_own(&scene.objects[scene.object_index(2).unwrap()]));
    scene.earn(owner, 100_000, 100_000);
    let mut units: [Vec<u8>; 9] = Default::default();
    units[4] = vec![60, 80];
    scene.set_stock_units(2, units);
    let may = |s: &Scene, item: u8| s.may_buy(2, owner, super::trade::Piece::Item(item)) == 0;
    // The plan, by 0x43f440's steps.
    let mut stock = vec![80u8, 60];
    let mut want: Vec<(u8, u8)> = vec![(4, stock.remove(0))];
    let (mut amber, mut weight) = (0, 1);
    for item in [5u8, 6] {
        let (got, cond) = if may(&scene, item) {
            (item, 100)
        } else if !stock.is_empty() {
            (4, stock.remove(0))
        } else {
            (4, 100)
        };
        if cond == 100 {
            amber += combat::item_cost(got).0;
        }
        weight += combat::item_power(got);
        want.push((got, cond));
    }
    for _ in [2, 1] {
        if weight >= 4 {
            break;
        }
        let cond = if stock.is_empty() {
            amber += combat::item_cost(4).0;
            100
        } else {
            stock.remove(0)
        };
        weight += 1;
        want.push((4, cond));
    }
    let before = scene.funds(owner).0;
    let site = (12, 14);
    scene
        .place_building_facing(2, 0x34, site.0, site.1, Some(0), &data, &mut caches)
        .unwrap();
    let fort = scene
        .objects
        .iter()
        .find(|o| o.class == ObjectClass::Building(0x34) && o.cell == Some(site))
        .unwrap();
    let c = fort.combat.as_ref().unwrap();
    let mut weapons: Vec<u8> = want.iter().map(|u| u.0).collect();
    weapons.sort_unstable();
    assert_eq!(c.weapons, weapons);
    let conditions: Vec<u8> = want.iter().map(|u| u.1).collect();
    assert_eq!(c.conditions, conditions);
    assert_eq!(scene.stock_units(2)[4], stock);
    assert_eq!(
        before - scene.funds(owner).0,
        economy::building_cost(0x34) + amber
    );
}

/// A patrol's legs are moves with its own tolerance (0x47a900's p7):
/// miss004's barbarian raiders (0x4536c0) sail to (36, 30) with 3, the
/// northern raiders' sweep (0x451c60) to (8, 9) with 2; an event firing
/// on the step's first turn sends no leg (needs `OPENCONQUEST_DATA`).
#[test]
fn a_patrol_moves_with_its_own_tolerance() {
    let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss004").unwrap();
    // (1, 3) runs at 20, (0, 4) at 20 too.
    {
        let b = scene.brain.as_mut().unwrap();
        b.fleets[10].state = 5;
        b.fleets[4].state = 40;
        b.fields[1] = 0;
    }
    for _ in 0..20 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    let order = |k: usize| b.fleets[k].move_order.as_ref().map(|m| (m.to, m.tol));
    assert_eq!(order(10), Some(((36, 30), 3)));
    assert_eq!(order(4), Some(((8, 9), 2)));
    // The player in the home zone: the event, and no leg.
    let mut scene = Scene::load(&data, &mut caches, "miss004").unwrap();
    let t = TILE_SIZE as f32;
    let mine = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    scene.objects[mine].x = 3.0 * t + t / 2.0;
    scene.objects[mine].y = 5.0 * t + t / 2.0;
    {
        let b = scene.brain.as_mut().unwrap();
        b.fleets[4].state = 40;
        b.fields[1] = 0;
    }
    for _ in 0..20 {
        scene.run_brain();
    }
    let b = scene.brain.as_ref().unwrap();
    assert_eq!(b.fleets[4].state, 20);
    assert!(b.fleets[4].move_order.is_none());
}
