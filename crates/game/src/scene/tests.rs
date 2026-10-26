//! Scene tests; most need the game data (`OPENAC_DATA`).

use super::*;
use crate::objects::Caches;

/// Builds a boat and upgrades the settlement in the first campaign
/// mission (needs `OPENAC_DATA`).
#[test]
fn shipyard_builds_and_settlement_upgrades() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
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
    scene.start_ship(yard, 0).unwrap();
    assert_eq!(scene.amber, 100);
    assert!(
        scene.start_ship(yard, 0).is_err(),
        "shipyard should be busy"
    );
    for _ in 0..(60.0 * (economy::SHIP_BUILD_SECONDS + 1.0)) as usize {
        scene.update(1.0 / 60.0, &data, &mut caches);
    }
    let own_ships: Vec<_> = scene
        .objects
        .iter()
        .filter(|o| o.ship.is_some() && scene.is_own(o))
        .collect();
    assert_eq!(own_ships.len(), ships_before + 1);
    assert!(scene.productions.is_empty());

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
    // With a town and 500 amber, a merchant (400 amber, 200 fish) is possible.
    assert_eq!(scene.can_build_ship(1), Ok(()));
}

/// A warship attacks and sinks an enemy ship (needs `OPENAC_DATA`).
#[test]
fn warship_sinks_enemy() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    for _ in 0..60 * 120 {
        scene.update(1.0 / 60.0, &data, &mut caches);
        if scene.object_index(target).is_none() {
            break;
        }
    }
    assert!(scene.object_index(target).is_none(), "the enemy was sunk");
    assert!(
        scene.effects.iter().any(|fx| fx.frames.len() == 20),
        "the hull goes down with its sinking animation"
    );
    assert!(scene.object_index(mine).is_some());
}

/// A rammer damages its target, and boarders capture a ship (needs
/// `OPENAC_DATA`).
#[test]
fn ramming_and_boarding() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    let hull = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .hull
    };
    let before = hull(&scene, other_id);
    scene.order_close(mine_id, other_id, false).unwrap();
    scene.update(0.02, &data, &mut caches);
    assert!(hull(&scene, other_id) < before, "the ram did damage");
    scene.order_close(mine_id, other_id, true).unwrap();
    for _ in 0..50 * 60 {
        scene.update(0.02, &data, &mut caches);
        let i = scene.object_index(other_id).unwrap();
        if scene.is_own(&scene.objects[i]) {
            break;
        }
    }
    let prize = &scene.objects[scene.object_index(other_id).unwrap()];
    assert!(scene.is_own(prize), "the ship was captured");
    assert!(prize.ship.as_ref().unwrap().crew.count() > 0);
}

/// A boat founds a settlement on a distant shore (needs `OPENAC_DATA`).
#[test]
fn boat_founds_settlement() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    // A shore cell well away from the boat's home.
    let (w, h) = (scene.nav.width as i32, scene.nav.height as i32);
    let shore = (0..h)
        .rev()
        .flat_map(|y| (0..w).rev().map(move |x| (x, y)))
        .find(|&(x, y)| {
            scene.check_ground(SETTLEMENT, x, y).is_ok()
                && [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|(dx, dy)| scene.nav.is_water(x + dx, y + dy))
        })
        .unwrap();
    scene.order_found(boat, shore.0, shore.1).unwrap();
    for _ in 0..50 * 600 {
        scene.update(0.02, &data, &mut caches);
        if !scene.constructions.is_empty() {
            break;
        }
    }
    assert!(scene.messages.is_empty(), "{:?}", scene.messages);
    assert_eq!(scene.amber, 1000 - economy::building_cost(SETTLEMENT));
    for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(settlements(&scene), before + 1);
}

/// A settlement trains swordsmen and a ship alongside takes them on
/// (needs `OPENAC_DATA`).
#[test]
fn marines_train_and_embark() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    let (amber, fish) = (scene.amber, scene.fish);
    scene.train(home, 2).unwrap();
    scene.train(home, 2).unwrap();
    assert!(scene.train(home, 0).is_err(), "one crew type at a time");
    assert_eq!((scene.amber, scene.fish), (amber - 60, fish - 600));
    for _ in 0..50 * (2 * combat::train_seconds(0).ceil() as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.garrison(home)[..3], [0, 0, 2]);
    assert!(scene.training_at(home).is_none());
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
/// sinks a ship (needs `OPENAC_DATA`).
#[test]
fn temple_magic() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let ship = scene.objects.iter().find(|o| o.ship.is_some()).unwrap().id;
    scene.objects.retain(|o| o.ship.is_none() || o.id == ship);
    scene.amber = 50_000;
    scene.fish = 50_000;
    assert!(scene.learn_spell(Temple::Sea).is_err(), "no temple yet");
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
    assert!(scene.learn_spell(Temple::Sea).is_err(), "still being built");
    for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    // Learn the first four: wind, view map, tornado, whirlpool.
    for _ in 0..4 {
        scene.learn_spell(Temple::Sea).unwrap();
        for _ in 0..50 * (magic::LEARN_SECONDS as usize + 1) {
            scene.update(0.02, &data, &mut caches);
        }
    }
    assert!(scene.magic.knows(Spell::Whirlpool) && !scene.magic.knows(Spell::Reef));
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
    scene
        .cast_hazard(Spell::Whirlpool, cx, cy, &data, &mut caches)
        .unwrap();
    // Put the ship in the whirlpool's eye.
    let pool = scene
        .objects
        .iter()
        .find(|o| {
            o.class == ObjectClass::Whirlpool
                && o.record.fields[5] == cx as u32
                && o.record.fields[6] == cy as u32
        })
        .unwrap();
    let (px, py) = (pool.x, pool.y);
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].x = px;
    scene.objects[i].y = py;
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
/// and magic, lightning and flame hurt (needs `OPENAC_DATA`).
#[test]
fn war_magic() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    scene.amber = 200_000;
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
    for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    for _ in 0..6 {
        scene.learn_spell(Temple::War).unwrap();
        for _ in 0..50 * (magic::LEARN_SECONDS as usize + 1) {
            scene.magic.update(0.02);
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
    // The magic shield stops lightning and blunts a fireball.
    scene.objects[e].combat.as_mut().unwrap().protect_magic = 60.0;
    let (ex, ey) = cell(&scene, enemy);
    scene
        .cast_area(Spell::Lightning, ex, ey, &data, &mut caches)
        .unwrap();
    assert!(
        scene.object_index(enemy).is_some(),
        "shielded from lightning"
    );
    let before = hull(&scene, enemy);
    scene
        .cast_area(Spell::Fireball, ex, ey, &data, &mut caches)
        .unwrap();
    for _ in 0..50 * 5 {
        scene.update(0.02, &data, &mut caches);
    }
    let burnt = before - hull(&scene, enemy);
    let expected = magic::FIREBALL_DPS * magic::SHIELD_FACTOR * 5.0;
    assert!(
        (burnt - expected).abs() < expected * 0.1,
        "burnt {burnt}, expected {expected}"
    );
    assert!(
        hull(&scene, mine) > 10_000.0 - 600.0,
        "the fireball spares our own"
    );
    // Unshielded, lightning destroys it.
    let e = scene.object_index(enemy).unwrap();
    scene.objects[e].combat.as_mut().unwrap().protect_magic = 0.0;
    let (ex, ey) = cell(&scene, enemy);
    scene
        .cast_area(Spell::Lightning, ex, ey, &data, &mut caches)
        .unwrap();
    assert!(scene.object_index(enemy).is_none(), "struck down");
    // Flame makes our weapons stronger for a while.
    scene.cast_flame().unwrap();
    assert!(scene.flame > 0.0);
}

/// Missions are won when every condition is met and lost when the
/// player has nothing left (needs `OPENAC_DATA`).
#[test]
fn victory_and_defeat() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    scene.update(1.0, &data, &mut caches);
    assert!(scene.objective_met(0) && !scene.objective_met(1));
    assert_eq!(scene.outcome, None, "the dragons still live");
    let nest = scene
        .objects
        .iter()
        .find(|o| o.record.class_id == 0x44)
        .unwrap()
        .id;
    scene.hit(nest, 1.0e6, &data, &mut caches);
    scene.update(1.0, &data, &mut caches);
    assert_eq!(scene.outcome, Some(Outcome::Victory));

    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let own: Vec<u32> = scene
        .objects
        .iter()
        .filter(|o| scene.is_own(o))
        .map(|o| o.id)
        .collect();
    scene.objects.retain(|o| !own.contains(&o.id));
    scene.update(1.0, &data, &mut caches);
    assert_eq!(scene.outcome, Some(Outcome::Defeat));
}

/// Computer players gather and build ships, then attack after the grace
/// period (needs `OPENAC_DATA`).
#[test]
fn computer_players_build_and_attack() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    let persians = (3, 3);
    let ships = |s: &Scene| {
        s.objects
            .iter()
            .filter(|o| o.owner == Some(persians) && o.ship.is_some())
            .count()
    };
    let funds = scene.funds(persians);
    for _ in 0..50 * 60 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(ships(&scene) > 0, "the Persians launched ships");
    assert!(scene.funds(persians).0 < funds.0, "and paid for them");
    let attacking = |s: &Scene| {
        s.objects.iter().any(|o| {
            o.owner == Some(persians)
                && matches!(
                    o.ship.as_ref().and_then(|s| s.task),
                    Some(Task::Attack { .. })
                )
        })
    };
    assert!(!attacking(&scene), "no attack during the grace period");
    for _ in 0..50 * (ai::GRACE_SECONDS as usize) {
        scene.update(0.02, &data, &mut caches);
        if attacking(&scene) {
            break;
        }
    }
    assert!(attacking(&scene), "a wave set out");
}

/// A workshop builds a ballista and a ship alongside takes it on
/// (needs `OPENAC_DATA`).
#[test]
fn workshop_builds_weapons() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    let ship = scene
        .objects
        .iter()
        .find(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap()
        .id;
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

/// A tornado drifts and wrecks ships; a summoned sea monster fights for
/// its master, then leaves (needs `OPENAC_DATA`).
#[test]
fn tornado_and_sea_monster() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    let human = scene.human.unwrap();
    scene.amber = 100_000;
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
    for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    for _ in 0..6 {
        scene.learn_spell(Temple::Sea).unwrap();
        for _ in 0..50 * (magic::LEARN_SECONDS as usize + 1) {
            scene.magic.update(0.02);
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
    assert!(scene.is_own(&scene.objects[m]) && scene.objects[m].is_armed());
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
    for _ in 0..50 * (magic::MONSTER_SECONDS as usize) {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.object_index(monster).is_none(), "the monster left");
    // A tornado drifts with the wind.
    scene
        .cast_hazard(Spell::Waterspout, cell.0, cell.1, &data, &mut caches)
        .unwrap();
    let fx = scene.effects.iter().position(|fx| fx.drift).unwrap();
    let start = (scene.effects[fx].x, scene.effects[fx].y);
    scene.update(1.0, &data, &mut caches);
    let fx = scene.effects.iter().position(|fx| fx.drift).unwrap();
    assert_ne!((scene.effects[fx].x, scene.effects[fx].y), start);
}

/// A saved game, restored, carries on exactly as the original would
/// (needs `OPENAC_DATA`).
#[test]
fn save_and_restore() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    let path = std::env::temp_dir().join(format!("openac-save-test-{}.json", std::process::id()));
    scene.save().write(&path).unwrap();
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
/// (needs `OPENAC_DATA`).
#[test]
fn school_research() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
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

/// A smithy fits armour to a ship alongside, once the school has
/// studied it (needs `OPENAC_DATA`).
#[test]
fn smithy_refits() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    // A ship of ours alongside.
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
    let kind = scene.objects[scene.object_index(ship).unwrap()]
        .ship
        .as_ref()
        .unwrap()
        .kind;
    let amber = scene.amber;
    scene.refit(id, armour, 1).unwrap();
    assert_eq!(scene.amber, amber - combat::refit_cost(armour, 1, kind));
    assert_eq!(
        scene.objects[scene.object_index(ship).unwrap()]
            .ship
            .as_ref()
            .unwrap()
            .armour,
        1
    );
    assert!(scene.refit(id, armour, 1).is_err(), "already fitted");
    assert!(scene.refit(id, armour, 2).is_err(), "level 2 not studied");
}

/// A settlement puts up a smithy next to it (needs `OPENAC_DATA`).
#[test]
fn settlement_builds_structures() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
        scene.update(0.02, &data, &mut caches);
    }
    assert_ne!(scene.owned_buildings() & bit, 0, "the smithy stands");
    assert!(scene.constructions.is_empty());
}

/// A warship destroys an enemy building, and forts fire at enemy ships.
#[test]
fn buildings_fight() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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

/// Computer warships attack a human ship that sails close.
#[test]
fn computer_ships_attack_intruders() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss009").unwrap();
    // The stand-in AI, not miss009's brain.
    scene.brain = None;
    let enemy = scene
        .objects
        .iter()
        .position(|o| !scene.is_own(o) && o.owner.is_some() && o.ship.is_some() && o.is_armed())
        .unwrap();
    let (x, y, enemy) = (
        scene.objects[enemy].x,
        scene.objects[enemy].y,
        scene.objects[enemy].id,
    );
    let mine = scene
        .objects
        .iter()
        .position(|o| scene.is_own(o) && o.ship.is_some())
        .unwrap();
    scene.objects[mine].x = x + 3.0 * 64.0;
    scene.objects[mine].y = y;
    let mine = scene.objects[mine].id;
    scene.update(0.02, &data, &mut caches);
    let i = scene.object_index(enemy).unwrap();
    assert_eq!(
        scene.objects[i].ship.as_ref().unwrap().task,
        Some(Task::Attack { target: mine })
    );
}

/// A boat gathers amber and delivers it (needs `OPENAC_DATA`).
#[test]
fn merchant_gathers_amber() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    scene.order_gather(boat, deposit).unwrap();
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
    assert_eq!(scene.amber, amber + 400, "one full boat load delivered");
    assert!(partly && walkway && porters, "{partly} {walkway} {porters}");
    assert!(amber_walkway, "a walkway to the amber while gathering");
    let left = scene.objects[scene.object_index(deposit).unwrap()]
        .deposit
        .unwrap()
        .remaining
        .unwrap();
    assert!((before - left - 400.0).abs() < 1.0 || left < before - 400.0);
}

/// Sails a ship across a real mission map (needs `OPENAC_DATA`).
#[test]
fn ship_sails_around_islands() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// sight (needs `OPENAC_DATA`).
#[test]
fn fog_of_war() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// `OPENAC_DATA`).
#[test]
fn workshop_research() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    for _ in 0..50 * (economy::BUILDING_SECONDS as usize + 1) {
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
/// (needs `OPENAC_DATA`).
#[test]
fn repairs() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// `OPENAC_DATA`).
#[test]
fn behaviours() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// fish; the zone is then empty (needs `OPENAC_DATA`).
#[test]
fn leviathan_hunt() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    scene.objects[z].deposit = Some(crate::objects::Deposit {
        resource: Resource::Leviathan,
        remaining: Some(1.0),
        regrows: 0,
        refill: 0.0,
    });
    scene.order_gather(boat, zone).unwrap();
    let (amber, fish) = (scene.amber, scene.fish);
    for _ in 0..60 * 600 {
        scene.update(1.0 / 60.0, &data, &mut caches);
        if scene.amber > amber {
            break;
        }
    }
    assert_eq!((scene.amber, scene.fish), (amber + 5000, fish + 2500));
    assert!(scene.object_index(zone).is_none(), "the leviathan is gone");
}

/// Waypoints add legs to a ship's route (needs `OPENAC_DATA`).
#[test]
fn waypoints() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// `OPENAC_DATA`).
#[test]
fn scout_and_patrol() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// zone order ends once the zone is clear (needs `OPENAC_DATA`).
#[test]
fn zones() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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

/// A boat given an amber zone goes to gather from the deposit in it on
/// its own; moving it by hand ends the zone (needs `OPENAC_DATA`).
#[test]
fn gathering_zone() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    let deposit = scene
        .objects
        .iter()
        .find(|o| o.deposit.is_some_and(|d| d.resource == Resource::Amber))
        .unwrap();
    let (d, dx, dy) = (deposit.id, deposit.x, deposit.y);
    scene
        .order_gather_zone(
            boat,
            (dx - 32.0, dy - 32.0),
            (dx + 32.0, dy + 32.0),
            Resource::Amber,
        )
        .unwrap();
    scene.update(0.02, &data, &mut caches);
    let ship = |s: &Scene| {
        s.objects[s.object_index(boat).unwrap()]
            .ship
            .clone()
            .unwrap()
    };
    assert!(matches!(ship(&scene).task, Some(Task::Gather { deposit, .. }) if deposit == d));
    let (bx, by) = {
        let o = &scene.objects[scene.object_index(boat).unwrap()];
        (o.x, o.y)
    };
    assert!(scene.order_move(boat, bx, by));
    assert!(ship(&scene).zone.is_none());
}

/// An escort follows the ship it convoys (needs `OPENAC_DATA`).
#[test]
fn convoy() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// hero's item brings the hero from his house (needs `OPENAC_DATA`).
#[test]
fn treasures_and_heroes() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// draws ships towards her (needs `OPENAC_DATA`).
#[test]
fn gorgon_and_siren() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    for _ in 0..50 * (GORGON_TEST_SECONDS) {
        scene.update(0.02, &data, &mut caches);
    }
    let stone = |s: &Scene, id| {
        s.objects[s.object_index(id).unwrap()]
            .combat
            .as_ref()
            .unwrap()
            .stone
    };
    assert!(stone(&scene, ships[0]) > 0.0, "turned to stone");
    assert_eq!(stone(&scene, ships[1]), 0.0, "Perseus is safe");

    let (mut scene, ships, (sx, sy)) = setup("battle08", 0x1fd, &mut caches);
    let dist = |s: &Scene| {
        let o = &s.objects[s.object_index(ships[0]).unwrap()];
        ((o.x - sx).powi(2) + (o.y - sy).powi(2)).sqrt()
    };
    let before = dist(&scene);
    for _ in 0..50 * 3 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(
        dist(&scene) < before,
        "drawn towards the siren: {} -> {}",
        before,
        dist(&scene)
    );
}

const GORGON_TEST_SECONDS: usize = 9;

/// Quicklime kills crews without touching the hull; a fire rammer's barrel
/// of powder goes off in the ram (needs `OPENAC_DATA`).
#[test]
fn crews_under_fire() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    scene.objects[e].combat.as_mut().unwrap().weapons.clear();
    scene.objects[e].ship.as_mut().unwrap().crew.squads = [0, 0, 4];
    let hull = scene.objects[e].combat.as_ref().unwrap().hull;
    for _ in 0..50 * 20 {
        scene.update(0.02, &data, &mut caches);
    }
    let e = scene.object_index(enemy).unwrap();
    assert!(
        scene.objects[e].ship.as_ref().unwrap().crew.count() < 4,
        "crews burnt"
    );
    assert_eq!(
        scene.objects[e].combat.as_ref().unwrap().hull,
        hull,
        "hull untouched"
    );
    // A barrel of powder adds its blast to a ram, once.
    let m = scene.object_index(mine).unwrap();
    scene.objects[m].combat.as_mut().unwrap().weapons = vec![combat::POWDER];
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
/// arming at level 2 (needs `OPENAC_DATA`).
#[test]
fn unique_ships_need_study() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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

/// Campaign missions are judged by their story: miss000 is won with the
/// barbarians gone and the parchment aboard (needs `OPENAC_DATA`).
#[test]
fn campaign_story() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    scene.hit(holder, f32::MAX, &data, &mut caches);
    assert!(scene
        .objects
        .iter()
        .any(|o| o.treasure.is_some_and(|t| t.kind == 18)));
    assert!(!scene.held.iter().any(|h| h.1.kind == 18));
    let scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    assert_eq!(
        scene.story,
        vec![Goal::Recruit(7), Goal::Settle, Goal::Assemble]
    );
    let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
    assert_eq!(scene.story, vec![Goal::Kill(0), Goal::Hold(27)]);
    // No barbarian left: not yet won without the parchment.
    let human = scene.human.map(|h| (h.nation, h.slot));
    scene
        .objects
        .retain(|o| o.owner.is_none() || o.owner == human || o.owner == Some(combat::MONSTERS));
    for _ in 0..50 * 4 {
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
    for _ in 0..50 * 4 {
        scene.update(0.02, &data, &mut caches);
    }
    assert_eq!(scene.outcome, Some(Outcome::Victory));
}

/// miss000's brain: its fleets are the barbarian ships, the merchant goes
/// fishing, and the guards attack when the player comes into a bay (needs
/// `OPENAC_DATA`).
#[test]
fn miss000_brain() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    let m = scene.object_index(merchant).unwrap();
    assert!(
        scene.objects[m].ship.as_ref().unwrap().zone.is_some(),
        "fishing"
    );
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
/// `OPENAC_DATA`).
#[test]
fn campaign_heroes_carry_over() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    // miss005 starts with only a boat: they ride on it.
    let scene = Scene::load(&data, &mut caches, "miss005").unwrap();
    assert_eq!(scene.carried, vec![0, 5, 1, 8]);
    let human = scene.human.map(|h| (h.nation, h.slot));
    let boat = scene
        .objects
        .iter()
        .find(|o| o.ship.is_some() && o.owner == human)
        .unwrap();
    assert_eq!(boat.ship.as_ref().unwrap().heroes, vec![0, 5, 1, 8]);
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
/// for, paid from their purse (needs `OPENAC_DATA`).
#[test]
fn miss001_brain_builds() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// (needs `OPENAC_DATA`).
#[test]
fn miss002_brain() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// `OPENAC_DATA`).
#[test]
fn miss003_brain() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// (needs `OPENAC_DATA`).
#[test]
fn ported_brains_run() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// 2000 on normal and none on hard (needs `OPENAC_DATA`).
#[test]
fn difficulty_sets_bonus() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// reaches a settlement (needs `OPENAC_DATA`).
#[test]
fn scroll_reveals_armour() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// mission (needs `OPENAC_DATA`).
#[test]
fn campaign_carries_research() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// fire (needs `OPENAC_DATA`).
#[test]
fn fire_wizard() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    scene.hit(castle, f32::MAX, &data, &mut caches);
    assert!(scene
        .objects
        .iter()
        .any(|o| o.treasure.is_some_and(|t| t.kind == 27 && t.param == 13)));
}

/// miss010's dragon leaves its nest, strikes a ship nearby and, killed, is
/// followed by another while the nest has some left (needs `OPENAC_DATA`).
#[test]
fn dragon_flies_and_returns() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    let ship = scene.objects[ship].id;
    let hull = |s: &Scene| {
        s.object_index(ship)
            .map_or(0.0, |i| s.objects[i].combat.as_ref().unwrap().hull)
    };
    let before = hull(&scene);
    for _ in 0..50 * 30 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(hull(&scene) < before, "the dragon struck");
    scene.hit(dragon.id, f32::MAX, &data, &mut caches);
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
/// land and throw at ships in reach (needs `OPENAC_DATA`).
#[test]
fn creatures_leave_their_lairs() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss006").unwrap();
    scene.brain = None;
    for _ in 0..50 * 40 {
        scene.update(0.02, &data, &mut caches);
    }
    assert!(scene.walkers.len() >= 3, "{} out", scene.walkers.len());
    let t = TILE_SIZE as f32;
    for w in &scene.walkers {
        let o = &scene.objects[scene.object_index(w.id).unwrap()];
        assert!(!scene.nav.is_water((o.x / t) as i32, (o.y / t) as i32));
    }
    // A ship beside a cyclops is stoned.
    let cyclops = scene
        .walkers
        .iter()
        .map(|w| &scene.objects[scene.object_index(w.id).unwrap()])
        .find(|o| o.record.class_id == 0x1f4)
        .map(|o| (o.x, o.y))
        .unwrap();
    let (cx, cy) = ((cyclops.0 / t) as i32, (cyclops.1 / t) as i32);
    let (wx, wy) = (-3..=3)
        .flat_map(|dy| (-3..=3).map(move |dx| (cx + dx, cy + dy)))
        .find(|&(x, y)| scene.nav.is_water(x, y))
        .unwrap();
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
/// `OPENAC_DATA`).
#[test]
fn argus_builds_the_argo() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let mut scene = Scene::load(&data, &mut caches, "miss002").unwrap();
    scene.brain = None;
    assert!(scene.story.contains(&Goal::Argo));
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
    assert!(!scene.goal_met(Goal::Argo));
    scene.christen_argo(ship);
    let i = scene.object_index(ship).unwrap();
    scene.objects[i].ship.as_mut().unwrap().heroes = vec![0, 5];
    assert!(scene.goal_met(Goal::Argo));
    assert_eq!(scene.objects[i].label, "Argo");
}

/// Two machines, a host and a client over a local connection, play
/// battle01 together: each gives its ship an order, both play the same
/// turns and their games stay the same (needs `OPENAC_DATA`).
#[test]
fn multiplayer_lockstep() {
    use crate::net::{Net, Setup, STEP, TURN};
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    let players = openac_formats::players(&archive).unwrap();
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
/// `OPENAC_DATA`).
#[test]
fn statistics_and_score() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
    scene.hit(enemy, f32::MAX, &data, &mut caches);
    scene.hit(mine, f32::MAX, &data, &mut caches);
    assert_eq!(scene.stats.ships_destroyed[ek as usize], 1);
    assert_eq!(scene.stats.ships_lost[mk as usize], 1);
    let points = [
        200i64, 400, 1100, 1300, 900, 800, 700, 600, 600, 900, 600, 900,
    ];
    let expected = (points[ek as usize] - points[mk as usize]).max(0) / 10;
    assert_eq!(scene.stats.score() as i64, expected);
    assert!(scene
        .stats
        .lines(scene.amber)
        .last()
        .unwrap()
        .starts_with("Total score"));
}

/// Missions start with the view where the original puts it (`WIN_VIEW`):
/// miss005's at cell (27, 28), the player's ship in sight (needs
/// `OPENAC_DATA`).
#[test]
fn missions_start_where_the_view_is_set() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// in its middle (needs `OPENAC_DATA`).
#[test]
fn deposits_are_picked_by_area() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// `OPENAC_DATA`).
#[test]
fn pointers_load() {
    use crate::cursor::{Pointer as P, Pointers};
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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

/// A boat fishing casts its net (needs `OPENAC_DATA`).
#[test]
fn fishing_casts_the_net() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// (needs `OPENAC_DATA`).
#[test]
fn amber_grows_back() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
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
/// the computer players have armour, ram and the workshop's levels at 2
/// (needs `OPENAC_DATA`).
#[test]
fn missions_give_starting_levels() {
    let Some(root) = std::env::var_os("OPENAC_DATA") else {
        return;
    };
    let data = GameData::open(std::path::Path::new(&root)).unwrap();
    let mut caches = Caches::default();
    let scene = Scene::load(&data, &mut caches, "miss008").unwrap();
    let human = scene.human.map(|h| (h.nation, h.slot));
    assert_eq!(scene.levels_of(human), combat::Levels::default());
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

/// Finds the menu font, colour variant and spacing that draw `OPENAC_FIT_TEXT`
/// at `OPENAC_FIT_AT` (x,y,w,h) as in the 800x600 raw RGB screenshot
/// `OPENAC_FIT_RGB` (a development aid; does nothing without them).
#[test]
fn fit_font() {
    let (Some(root), Ok(raw), Ok(bg), Ok(text), Ok(at)) = (
        std::env::var_os("OPENAC_DATA"),
        std::env::var("OPENAC_FIT_RGB"),
        std::env::var("OPENAC_FIT_BG"),
        std::env::var("OPENAC_FIT_TEXT"),
        std::env::var("OPENAC_FIT_AT"),
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
    let palette = openac_formats::Bitmap::parse(
        &arc.read("0M_BKGND_00", openac_formats::ResourceKind::Bitmap)
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
                let reach: i32 = std::env::var("OPENAC_FIT_REACH")
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
    arc: &openac_formats::Archive,
    name: &str,
    variant: usize,
    palette: &openac_formats::Palette,
) -> crate::text::TextRenderer {
    crate::text::TextRenderer::load_in(arc, &format!("0{name}"), variant, Some(palette)).unwrap()
}

/// Finds where the menu sprites `OPENAC_FIT_SPRITES` (comma separated) sit
/// in the 800x600 raw RGB screenshot `OPENAC_FIT_RGB`, searching the area
/// `OPENAC_FIT_AREA` (x0,y0,x1,y1 for the sprites' top-left corner); a
/// development aid that does nothing without them.
#[test]
fn fit_sprite() {
    let (Some(root), Ok(raw), Ok(names), Ok(area)) = (
        std::env::var_os("OPENAC_DATA"),
        std::env::var("OPENAC_FIT_RGB"),
        std::env::var("OPENAC_FIT_SPRITES"),
        std::env::var("OPENAC_FIT_AREA"),
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
    let palette = openac_formats::Bitmap::parse(
        &arc.read("0M_BKGND_00", openac_formats::ResourceKind::Bitmap)
            .unwrap(),
    )
    .unwrap()
    .palette;
    let a: Vec<i32> = area.split(',').map(|v| v.parse().unwrap()).collect();
    for name in names.split(',') {
        let e = arc
            .find(&format!("0{name}"), openac_formats::ResourceKind::Sprite)
            .or_else(|| arc.find(name, openac_formats::ResourceKind::Sprite))
            .unwrap();
        let s = openac_formats::Sprite::parse(&arc.read_entry(e).unwrap()).unwrap();
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
