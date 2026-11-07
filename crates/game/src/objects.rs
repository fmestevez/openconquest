//! Mission objects: turning object records into drawable, animated visuals.
//!
//! Sprite names, record fields, placement and animation timing come from
//! ANCIENT.EXE (see `docs/FORMATS.md`, "Mission objects"):
//!
//! * Buildings (0x4288c0): `prefix + nation [+ class] + state [+ facing] + damage`,
//!   where state 0 is under construction, 1 standing and 2 destroyed, and
//!   damage 3 is intact.
//! * Ships (0x4e0120): `SHIP_<type><nation><nation><player><mode>`; the
//!   owner's nation and player slot select the sail colours.
//! * Nature objects (reefs 0x4300a0, whirlpools 0x447930, castles 0x4101d0)
//!   are sequences whose top-left corner sits at the cell's top-left corner
//!   (minus a per-class offset), with a frame period of
//!   `cycle_ticks[state] / frame_count` game ticks.

use std::collections::HashMap;
use std::rc::Rc;

use anyhow::Result;
use openconquest_formats::anim::EMPTY_SPRITE;
use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::mission::field;
use openconquest_formats::{
    object_records, Archive, Composite, ObjectClass, ObjectRecord, ResourceKind, Sequence,
};

use crate::assets::{GameData, Image, ImageCache, Source};

use crate::clock::TICK_MS;
/// Frame period for objects whose timing has not been recovered yet.
const DEFAULT_FRAME_MS: u64 = 100;

/// Decoded-resource caches shared across missions.
pub struct Caches {
    pub tiles: ImageCache,
    pub sprites: ImageCache,
    composites: HashMap<String, Option<Rc<Composite>>>,
    sequences: HashMap<String, Rc<Vec<String>>>,
    /// Sequences' frames with a palette index drawn as shadow, by name
    /// and index (see `shaded_frames`).
    shaded: HashMap<(String, u16), Vec<Vec<Layer>>>,
}

impl Default for Caches {
    fn default() -> Self {
        Self {
            tiles: ImageCache::new(Source::MapLib),
            sprites: ImageCache::new(Source::Objects),
            composites: HashMap::new(),
            sequences: HashMap::new(),
            shaded: HashMap::new(),
        }
    }
}

impl Caches {
    fn composite(&mut self, data: &GameData, name: &str) -> Result<Option<Rc<Composite>>> {
        if let Some(c) = self.composites.get(name) {
            return Ok(c.clone());
        }
        let c = match data.objects.find(name, ResourceKind::Composite) {
            Some(e) => Some(Rc::new(Composite::parse(&data.objects.read_entry(e)?, e)?)),
            None => None,
        };
        self.composites.insert(name.to_string(), c.clone());
        Ok(c)
    }

    /// The frames of sequence `name` with palette index `shade` drawn as
    /// shadow (`GameData::load_image_shading`).
    pub fn shaded_frames(
        &mut self,
        data: &GameData,
        name: &str,
        shade: u8,
    ) -> Result<Vec<Vec<Layer>>> {
        self.marked_frames(data, name, Some(shade), false)
    }

    /// As `shaded_frames`, with `blend` the colours 1-7 drawn through the
    /// flames' blend (`assets::BLEND`).
    pub fn marked_frames(
        &mut self,
        data: &GameData,
        name: &str,
        shade: Option<u8>,
        blend: bool,
    ) -> Result<Vec<Vec<Layer>>> {
        let key = (
            name.to_string(),
            u16::from(shade.unwrap_or(0)) | u16::from(shade.is_some()) << 9 | u16::from(blend) << 8,
        );
        if let Some(f) = self.shaded.get(&key) {
            return Ok(f.clone());
        }
        let mut frames = Vec::new();
        for n in self.sequence(data, name)?.iter() {
            if let Some(image) = data.load_image_marked(&data.objects, n, shade, blend)? {
                frames.push(vec![Layer {
                    image: Rc::new(image),
                    dx: 0,
                    dy: 0,
                }]);
            }
        }
        self.shaded.insert(key, frames.clone());
        Ok(frames)
    }

    pub(crate) fn sequence(&mut self, data: &GameData, name: &str) -> Result<Rc<Vec<String>>> {
        if let Some(s) = self.sequences.get(name) {
            return Ok(s.clone());
        }
        let frames = match data.objects.find(name, ResourceKind::Sequence) {
            Some(e) => Sequence::parse(&data.objects.read_entry(e)?, e)?.frame_names(&data.objects),
            None => Vec::new(),
        };
        let frames = Rc::new(frames);
        self.sequences.insert(name.to_string(), frames.clone());
        Ok(frames)
    }
}

/// One sprite of an animation frame, offset from the object's position.
#[derive(Clone)]
pub struct Layer {
    pub image: Rc<Image>,
    pub dx: i32,
    pub dy: i32,
}

/// A banner's frames.
pub type Banner = Rc<Vec<Rc<Image>>>;

/// A placed, drawable object.
pub struct MapObject {
    /// Object id from the mission (`OBJ_xxxxxxxx`).
    pub id: u32,
    pub class: ObjectClass,
    /// The record the object was created from, kept so it can be rebuilt
    /// after a change (e.g. an upgrade).
    pub record: ObjectRecord,
    /// Human-readable description.
    pub label: String,
    /// World position of the object's centre, in pixels.
    pub x: f32,
    pub y: f32,
    /// Animation frames, each a stack of layers drawn in order. For ships,
    /// 8 headings x 4 frames.
    pub frames: Vec<Vec<Layer>>,
    pub frame_ms: u64,
    pub ship: Option<Ship>,
    pub deposit: Option<Deposit>,
    /// An exhausted amber deposit growing back: its deposit and the
    /// seconds it has lain empty.
    pub dormant: Option<(Deposit, f32)>,
    /// A building asleep (record field 4 = 3, state 0x10): seconds to its
    /// next look about for its owner's ships or buildings (0x425590).
    /// Unseen on the radar, unclickable, bannerless, blind, harmless and
    /// unharmed till it wakes.
    pub asleep: Option<f32>,
    /// A LandObj's depth in the sprites' list, set once at its making
    /// (0x425990 message 2: `max(20000 - bottom, 10000)` of the picture
    /// it has then) and kept (`render::object_depth`).
    pub made_depth: Option<i32>,
    /// Hull and weapons, for objects that can fight or be attacked.
    pub combat: Option<crate::combat::Combatant>,
    /// Owning player as `(nation, colour slot)`, for player-owned objects.
    pub owner: Option<(u32, u32)>,
    /// Picture for the object panel (`INF_SHIPnn` / `INF_OBJnn` in CONTROLG)
    /// and its offset in the panel.
    pub picture: Option<(String, (u32, u32))>,
    /// A treasure lying on the map.
    pub treasure: Option<Treasure>,
    /// Shows the frame this far (0..1) through the animation instead of
    /// playing it: a building going up.
    pub progress: Option<f32>,
    /// A building's banner (`FLAGSPR_<nation><slot>`, its frames: it waves
    /// a frame every 75 ms, LandObj 0x4247b0 and 0x425990) and where its
    /// top left lies from the building's.
    pub banner: Option<(Banner, i32, i32)>,
    /// The map cell a building stands on (its record's).
    pub cell: Option<(i32, i32)>,
}

/// A treasure (`TreasClassTy` record: kind in field 4, parameters in
/// fields 7 and 8; see `treasure_name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Treasure {
    pub kind: u8,
    /// Field 7: a spell index, resource class or packed item and count.
    pub param: u32,
    /// Field 8: an amount.
    pub amount: u32,
}

/// The nine heroes, by house number (`HeroHouseClassTy` field 22) and
/// portrait (`COBJ_HEROYnn`).
pub const HERO_NAMES: [&str; 9] = [
    "Jason", "Theseus", "Orpheus", "Zetes", "Hercules", "Argus", "Atalanta", "Odysseus", "Perseus",
];

/// What a treasure is, from the original's descriptions (string resources)
/// in the order the treasure routine 0x4b9740 groups the kinds: 0-2 amber
/// and 3-5 fish (0x4a8de0, 0x4a8c30), 6-7 goods to deliver, 8 a spell
/// cast where it is taken (0x4b9bf0), 9 a spell kept for later, 10-17
/// lasting gifts, 18-26 the heroes' magic items, 27 a scroll.
pub fn treasure_name(kind: u8) -> String {
    match kind {
        0..=2 => "Barrel of amber".into(),
        3..=5 => "Barrel of fish".into(),
        6 => "Goods for your settlement".into(),
        7 => "Weapons for your settlement".into(),
        8 => "Gift of Poseidon".into(),
        9 => "Disposable magic".into(),
        10 => "Torch of Ares (protection from fire)".into(),
        11 => "Armour of Ares".into(),
        12 => "Feather of Boreas (speed and ram)".into(),
        13 => "Magic wood (no boarding)".into(),
        14 => "Mirror shield of Athena (protection from Gorgon)".into(),
        15 => "Cup of Asclepius (heals crews)".into(),
        16 => "Golden Fleece".into(),
        17 => "Gorgon's head".into(),
        18..=26 => {
            const ITEMS: [&str; 9] = [
                "Magic oar",
                "Ariadne's clew",
                "Magic harp",
                "Magic horn",
                "Magic sword",
                "Hammer",
                "Magic bow",
                "Golden statuette",
                "Mirror shield",
            ];
            let h = (kind - 18) as usize;
            format!("{} for {}", ITEMS[h], HERO_NAMES[h])
        }
        _ => "Scroll of knowledge".into(),
    }
}

/// The treasures an object's record says it holds (its inventory: a count,
/// then entries of five dwords like a treasure record's fields 4-8: kind,
/// holder -1, 0, parameter, amount). Lairs, nests, barbarian structures,
/// fortresses, towns, sea monsters and leviathans keep theirs at different
/// fields.
/// A ship's capabilities by its type (0x4e6930 on `+0x25a`): all but bit
/// 11 (fire) for boats and merchants (0, 1), all but bit 8 (a merchant's
/// unload and gathers) for warships (2-11), all for any other.
pub fn caps_mask(kind: u8) -> u32 {
    match kind {
        0 | 1 => 0xffff_f7ff,
        2..=11 => 0xffff_feff,
        _ => 0xffff_ffff,
    }
}

/// The capability word of a ship saved before `Ship::caps` was kept.
pub const CAPS_UNSAVED: u32 = u32::MAX;

fn caps_unsaved() -> u32 {
    CAPS_UNSAVED
}

pub fn inventory(rec: &ObjectRecord) -> Vec<Treasure> {
    let count_at = match rec.class_id {
        0x46..=0x48 => 10,
        0x34..=0x36 | 0x3f..=0x41 => 11,
        0x44 | 0x45 => 12,
        0x1fb => 17,
        0x192 => 41,
        0x32 | 0x33 => 60,
        _ => return Vec::new(),
    };
    let f = |i: usize| rec.field(i);
    let Some(mut n) = f(count_at).filter(|&n| n <= 8) else {
        return Vec::new();
    };
    // A sea monster has two places (+0xbd, ten dwords from fields 18-27,
    // message 2 of 0x492360); it takes field 17 as its count unchecked. A
    // leviathan likewise (+0xc5 from fields 42-51, count field 41, 0x494f90).
    if rec.class_id == 0x1fb || rec.class_id == 0x192 {
        n = n.min(2);
    }
    (0..n as usize)
        .map_while(|k| {
            let at = count_at + 1 + 5 * k;
            let kind = f(at).filter(|&kind| kind <= 27)?;
            (f(at + 1)? == u32::MAX).then_some(Treasure {
                kind: kind as u8,
                param: f(at + 3)?,
                amount: f(at + 4)?,
            })
        })
        .collect()
}

/// The hero a treasure kind calls, if it is a hero's item.
pub fn treasure_hero(kind: u8) -> Option<u8> {
    (18..=26).contains(&kind).then(|| kind - 18)
}

/// Treasures and heroes a ship type can carry (0x4be800 modes 6 and 7:
/// the bytes 0x597afc and 0x597b08 by type, dumped).
pub fn treasure_room(kind: u8) -> usize {
    [1, 1, 2, 1, 1, 1, 1, 1, 0, 0, 0, 0][kind as usize % 12]
}

pub fn hero_room(kind: u8) -> usize {
    [0, 0, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0][kind as usize % 12]
}

/// Movement state of a ship.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ship {
    /// Ship type 0-11 (see `SHIP_NAMES`).
    pub kind: u8,
    /// 0 = east, counting counter-clockwise in 45 degree steps.
    pub heading: usize,
    /// Cruising speed in world pixels per second.
    pub speed: f32,
    /// Remaining waypoints (cell centres), nearest first.
    pub path: std::collections::VecDeque<(f32, f32)>,
    /// The hull's rotation (of 24, 3 a heading) while it turns; `None`
    /// when it lies on `heading`.
    #[serde(default)]
    pub rotation: Option<u8>,
    #[serde(default)]
    pub turn_clock: f32,
    /// Seconds it has waited for another ship to clear the cell ahead.
    #[serde(default)]
    pub blocked: f32,
    /// The priority of its last order (+0x5c7): a ship held up by one of
    /// its player's lying still with a lower one pushes it aside (see
    /// `Task::priority`).
    #[serde(default)]
    pub priority: u8,
    /// Ticks it waits on such a ship before pushing it (+0x5c8): 5 with
    /// each order, tripled with each push.
    #[serde(default = "default_patience")]
    pub patience: u8,
    /// The kind of order its priority was last taken from (`Task::tag`).
    #[serde(default)]
    pub order_tag: u8,
    /// Seconds it has been held up since it last made a step (+0x591,
    /// counted to 150 ticks): then it gives the move up (0x4a618d).
    #[serde(default)]
    pub waited: f32,
    /// Seconds since it last made a step while held up (+0x58d, not
    /// touched by a re-plan): at 50 ticks it lies still (state 0), its
    /// route kept, and may be pushed aside.
    #[serde(default)]
    pub stalled: f32,
    /// Cells it keeps closed when it plans (+0x5cd, 0x4a7230): ships it
    /// found lying in its way or planned through, until they leave the
    /// cell or get under way (0x4a7290).
    #[serde(default)]
    pub remembered: Vec<(i32, i32)>,
    /// Seconds since its route was last planned (+0x57b; re-planned every
    /// 25 ticks, +0x57f).
    #[serde(default)]
    pub planned: f32,
    /// A plain move's goal it has no route to yet (the order's cell, kept
    /// by 0x4a5f30): its first plan found none, so it plans again, waited,
    /// every 25 ticks (0x4a6150), keeping whatever route it had.
    #[serde(default)]
    pub pending: Option<(f32, f32)>,
    /// Its way (`+0x59b`), in the original's units of 1/6000 pixel a tick
    /// straight on: stepped every tick under way by its acceleration or
    /// brake and the wind (`scene::sail`, 0x4a3f40).
    #[serde(default)]
    pub way: i32,
    /// The way's units not yet moved (`+0x597`).
    #[serde(default)]
    pub way_rest: i32,
    /// Its brake (`+0x284`, negative), set as it starts braking (0x4a2f60).
    #[serde(default)]
    pub brake: i32,
    /// Its acceleration (`+0x280`) as the last stats refresh left it
    /// (0x4bece0: hull and armour), until the next step granted sets the
    /// plain one (0x4a3a70); `None` for the plain one.
    #[serde(default)]
    pub accel: Option<i32>,
    /// The cell after the one it has entered, reserved as it entered it
    /// (`+0x5ab` at a cell, 0x4a3a70 from 0x4a3f40); released as it enters
    /// the next or lies still.
    #[serde(default)]
    pub ahead: Option<(i32, i32)>,
    /// Its move state (+0x58c, 0x4a3f40): 0 lying still, 1 told to stop,
    /// 2 turning, 3 at a route point about to step, 4 braking to a stop in
    /// its cell, 5 under way between cells, 6 held by a boarding. Kept at
    /// the moves' events (the ship moves in `Scene::update`, the boarding
    /// and the ram); a sea monster weighs its prey by it.
    #[serde(default)]
    pub move_state: u8,
    /// Between two cells (+0x596): set as it sets out from a cell's
    /// middle for the next (0x4a3560, the end of a turn 0x4a3320, a plan
    /// lying still 0x4a6330) and on reaching a cell's middle (0x4a3f40);
    /// cleared as its picture's centre crosses into the cell it is
    /// entering (0x4a3f40) and by a plan made lying still, turning or at
    /// a cell (0x4e0b50, from a plan's `&self`, hence the `Cell`).
    #[serde(default)]
    pub between: std::cell::Cell<bool>,
    /// The cell it is going into (+0x5b6, set as the step is taken,
    /// 0x4a3a70): under way between cells (`between`, move state 1, 4,
    /// 5 or 6) its plans start from it (0x4a6330 with 0x4e0b50).
    #[serde(default)]
    pub entering: (i32, i32),
    /// Its move goal (`+0x5b2`): the last cell of the last route stored
    /// (0x4a6330), its own cell once told to stop in it (0x4a38f0). Coming
    /// into it it posts notice 0x136 and brakes; held one short of it,
    /// 0x136 (0x4a3f40).
    #[serde(default)]
    pub goal: Option<(i32, i32)>,
    /// The heading it turns to while closing in on a shore or pier, to lie
    /// side-on (0x4afe50).
    #[serde(default)]
    pub face: Option<usize>,
    /// How far its sail is furled, in frames of the furling (0-4, 4
    /// furled): it furls while it lies loading or unloading and unfurls as
    /// it leaves.
    #[serde(default)]
    pub furl: f32,
    /// Rocking at rest (0x4a3150): the frame of the four (0-3), swinging
    /// back (`rock_back`), and the ticks since it last moved and the
    /// seconds owed to the next tick.
    #[serde(default)]
    pub rock: u8,
    #[serde(default)]
    pub rock_back: bool,
    #[serde(default)]
    pub rock_ticks: u32,
    #[serde(default)]
    pub rock_clock: f32,
    /// Goods on board.
    pub cargo: f32,
    pub cargo_kind: Option<Resource>,
    /// A standing order the ship carries out on its own.
    pub task: Option<Task>,
    /// Marines on board.
    pub crew: crate::combat::Crew,
    /// Armour and ram fitted at a smithy, levels 0-2.
    #[serde(default)]
    pub armour: u8,
    #[serde(default)]
    pub ram: u8,
    /// What the ship does on its own (warships).
    #[serde(default)]
    pub behaviour: Behaviour,
    /// `+0x7ad`: the port it was sent to unload at (0x4aab00: the unload
    /// command's port; a gather command's, 0 from the port's paths).
    #[serde(default)]
    pub home_port: u32,
    /// The deposit it last gathered from (`+0x797`, its cell), kept when
    /// its gathering ends and the zone sends it to unload.
    #[serde(default)]
    pub last_deposit: u32,
    /// What its last gather command set (0x4aab00): the class `+0x76e`,
    /// the deposit's cell `+0x797` and the box about it `+0x7c2..+0x7ce`
    /// (the cell -/+ 2, clipped to the map's last cell; searched as
    /// `[x0, x1) x [y0, y1)`).
    #[serde(default)]
    pub gathered: Option<Gathered>,
    /// `+0x78e`: the tick its wait by the berth began (0x4adef0 step 1),
    /// 0 for none.
    #[serde(default)]
    pub berth_wait: u32,
    /// `+0x691` / `+0x692`: the last blow taken, by whom (0x4bf170 at
    /// each shot landing; command 0x28 sub 4 clears it).
    #[serde(default)]
    pub blow: Option<u32>,
    /// Treasures aboard.
    #[serde(default)]
    pub treasures: Vec<Treasure>,
    /// Its own Magic object (`+0x7d2`), made by the first spell counted
    /// (0x4ba070, 0x4b9bf0).
    #[serde(default)]
    pub magic: Option<ShipMagic>,
    /// Heroes aboard (see `HERO_NAMES`).
    #[serde(default)]
    pub heroes: Vec<u8>,
    /// This is the Argo (a royal ship built with "Build Argo", 0x7d).
    #[serde(default)]
    pub argo: bool,
    /// Set by the attack (`+0x594`, 0x4c09c0 with a mask) and cleared by
    /// a step granted (0x4a3c6e): a step refused then counts its extra
    /// tick of wait even at the route's last cell (0x4a3d76).
    #[serde(default)]
    pub attack_step: bool,
    /// The crew's experience (`+0x544`, at most 10000, raised by
    /// 0x4b4680): the map record's byte 13 x 100 (0x4dfa90), 0 for a ship
    /// built.
    #[serde(default)]
    pub experience: i32,
    /// What its frames show; they are rebuilt when it changes.
    #[serde(skip)]
    pub look: Option<ShipLook>,
    /// Each marine's and weapon's condition in percent, by item 0-8, in
    /// their order (ship `+0x299`, eight u32 per item; record fields
    /// 29-100). Kept as long as the units counted by `crew` and the
    /// weapons: missing ones are whole (`MapObject::units`).
    #[serde(default)]
    pub conditions: [Vec<u8>; 9],
    /// Each treasure slot's healing count (slot `+0xc`) and whether the
    /// slot mends (0x4b9740's class 1, a Cup or a Fleece, in its flags
    /// `+8`; 4 when player 16 took it, 0x4bd262), by slot as `treasures`.
    #[serde(default)]
    pub heal_slots: Vec<HealSlot>,
    /// Argus's healing count (hero slot `+0x10`, 0 as he comes aboard,
    /// 0x4bd350).
    #[serde(default)]
    pub argus_count: u32,
    /// Lying grappled to another ship in a boarding (0x4b5df0 states 8,
    /// 10, 0xb, 3, 0xc and 6 for the boarder, 0x4c3410 for the boarded).
    #[serde(default)]
    pub grapple: Option<Grapple>,
    /// Charging into a ship to ram it, the blow, and backing off
    /// (0x4c2230 states 2, 3 and 6).
    #[serde(default)]
    pub ram_run: Option<RamRun>,
    /// Being rammed: the prey's contact state 6 (0x4c3ec0, `+0x713` 1).
    #[serde(default)]
    pub ram_hit: Option<RamHit>,
    /// A loader's hook out, pull alongside, walkway and link (0x4b73a0
    /// states 7, 8, 10, 11, 4, 12 and 6).
    #[serde(default)]
    pub load_run: Option<LoadRun>,
    /// Landing a founding party at its berth (0x4b3370 states 2-8).
    #[serde(default)]
    pub landing: Option<Landing>,
    /// What its last close-in left (0x4b2ff0's fields, kept between
    /// orders).
    #[serde(default)]
    pub close_in: CloseIn,
    /// Being built on a shipyard's berth (`+0x251 = 2`, task 4: 0x4dfe00,
    /// 0x4b3ab0); `None` once afloat.
    #[serde(default)]
    pub build: Option<Build>,
    /// Mended at a dock: the tick of its last step (`+0x751`, set as it
    /// lies alongside, 0x4b263f; 0x4b1fc0 steps every 9).
    #[serde(default)]
    pub dock_step: Option<u32>,
    /// Closed in on a shore or pier to the line's end: 0x4b02a0 left its
    /// index `+0x78e` at the last point, the picture one before it.
    #[serde(default)]
    pub slide_end: bool,
    /// Its capability word (`+0x261`), each bit a command it may take
    /// (0x4e6700) and a button of its panel (0x4e6980, 0x4e6e90): 0 the
    /// building page and found (0x20), 1 repair (0x1e), 2 reload (0x1f),
    /// 3 ram (0x18), 4 board (0x17), 5 dock (0x19), 6 gather and the hunt
    /// (0x1a, 0x1c, 0x1d), 7 transfer (0x1b), 8 a merchant's unload and
    /// gathers, 10 move (0x14), 11 fire (0x15, 0x16), 12 taking a contact,
    /// 13 stop, 14 the song's move, 15 taking a treasure (0x22 kind 0x10);
    /// any of bits 0-11 the behaviour or zones button. Set by the map
    /// loader (0x4dfa90: record field 8 & 0x4e6930), the yard's (0x4dfe00
    /// at 0x4dfe4d: 0x2000) and the build's end (0x4b3ab0 at 0x4b3ef1:
    /// 0xffffffff & 0x4e6930); 0 from the sale (0x4b4380 at 0x4b43b6).
    /// What holds take off is laid over it by `Scene::capabilities`.
    /// (`CAPS_UNSAVED` in a save from before it, set again on load.)
    #[serde(default = "caps_unsaved")]
    pub caps: u32,
    /// Its own random numbers (`+0x18`, the usual LCG), from its first
    /// draw (`Scene::ship_draw`).
    #[serde(default)]
    pub lcg: Option<u32>,
    /// Hunting a leviathan (the gather task of class 0x192: 0x4ad680,
    /// 0x4aeef0; `scene::hunt`).
    #[serde(default)]
    pub hunt: Option<Hunt>,
    /// Closing in on the prey of a ram or boarding (`Task::Ram`,
    /// `Task::Board`; `scene::closing`).
    #[serde(default)]
    pub closing: Option<Closing>,
    /// The attack's route plans (0x4c09c0 -> 0x4b9140 -> 0x4a6270 mode
    /// 5): `+0x57b`, `+0x573`, `+0x56b` and `+0x593` as the attack's
    /// fire records set them (0x4ab3f0 kind 3, 0x4ab6a5).
    #[serde(default)]
    pub attack_plan: RoutePlan,
    /// `+0x704`: the tick after which (and 50 more) 0x4bf9b0 marks the
    /// ram switch for the at-will look (`scene::at_will`); 0x4c0b30 sets
    /// it to now after a board, the rammer's way back (0x4c2b9d) too.
    #[serde(default)]
    pub ram_wait: u32,
    /// `+0x708`: the board switch's, likewise; 0x4c0b30 sets it to now
    /// after a ram, the boarder's way back (0x4b6bee) too.
    #[serde(default)]
    pub board_wait: u32,
    /// `+0x25d & 0x100`, the fire task (`fire_task`).
    #[serde(default)]
    pub firing: bool,
    /// `+0x5e3 + 4i`: the tick item i's next shot may go at (0 from the
    /// constructor 0x4dc3f0; 0x4c0230 sets it as the item fires).
    #[serde(default)]
    pub fire_times: [u32; 12],
    /// `+0x613 + i`: item i ready, as 0x4bf9b0 last found it (only items
    /// aboard are looked at; the others keep theirs).
    #[serde(default)]
    pub items_ready: [bool; 12],
    /// `+0x62b + k`: position k has an item ready (0x4bf9b0; cleared after
    /// the fire pass, 0x4abba5).
    #[serde(default)]
    pub positions_ready: [bool; 4],
}

/// A route's last plan (0x4a6150, 0x4a6100): its tick (`+0x57b`), the
/// ship's cell and the goal then (`+0x573`, `+0x56b`), and whether the
/// next is made at once (`+0x593`, cleared as 0x4a6330 plans).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RoutePlan {
    pub planned: u32,
    pub plan_from: (i32, i32),
    pub plan_goal: (i32, i32),
    pub first_plan: bool,
}

/// A rammer's or boarder's way to its prey, a game tick at a time (see
/// `Ship::closing`): set as the order is taken (0x4aa840).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Closing {
    /// The prey it was set for.
    pub target: u32,
    /// `+0x6a1`: closing in about it (1), else sailing to it (3).
    pub near: bool,
    /// The cell its last plan sailing to it set out from (`+0x6c7`).
    pub from: Option<(i32, i32)>,
    /// `+0x6aa`, a cell as `W x y + x` (-1 none): the rammer's goal once
    /// within reach and not alongside; the boarder's prey within reach,
    /// the cell one past the end corner of its look's box (0x4b83bf; the
    /// hook's 0x4b84b0 writes the prey's cell over it before reading it).
    pub contact: i32,
    /// The route's last plan (0x4a6150, 0x4a6100): its tick (`+0x57b`),
    /// the ship's cell and the goal then (`+0x573`, `+0x56b`); the first
    /// is made at once (`+0x593`).
    pub planned: u32,
    pub plan_from: (i32, i32),
    pub plan_goal: (i32, i32),
    pub first_plan: bool,
    /// The rammer's contact asked for and refused while not between
    /// cells (0x4c2230 state 2, `+0x6a9` 2): asked again at its next
    /// look.
    #[serde(default)]
    pub contact_wait: bool,
    /// The load's `+0x6c6`: 0xff when it was far at the order (0x4aa840),
    /// so 0x4b9140 finds the target by id alone (0x4ac050 with -1); else
    /// the target's player, whose list (0x4a8710) has lost fallen ruins.
    #[serde(default)]
    pub by_id: bool,
    /// The loader's last hook's tick (`+0x6be`, 0x4b84b0; zeroed with the
    /// order by 0x4b8d10) and the cell it was thrown at (`+0x6d7`, kept
    /// from the last throw when this one misses before it is written).
    #[serde(default)]
    pub thrown: Option<u32>,
    #[serde(default)]
    pub hook_cell: (i32, i32),
}

/// A ship's leviathan hunt, a game tick at a time (see `Ship::hunt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Hunt {
    /// The step of 0x4aeef0 (`+0x77d`): 1 watching, 10 turning, 0xc the
    /// line going down, 2 a cast, 6 hauling, 0x12 the line coming up,
    /// 0x13 and 5 done.
    pub step: u8,
    /// The route's last plan (0x4a6150, 0x4a6100): its tick (`+0x57b`),
    /// the ship's cell and the goal then (`+0x573`, `+0x56b`); the first
    /// is made at once (`+0x593`).
    pub planned: u32,
    pub plan_from: (i32, i32),
    pub plan_goal: (i32, i32),
    pub first_plan: bool,
    /// What the cast took (`+0x782`).
    pub catch: i32,
    /// The heading it turns to and the way, -1, 0 or 1 (`+0x79b`,
    /// `+0x79d`), and the hull picture's frame (`+0x8be +0x45`, 0x20 to
    /// 0x37 while it turns).
    pub turn_to: u8,
    pub turn: i8,
    pub frame: i32,
    /// The line's frame (picture slot 1, `WMOST_`) and the haul's (slot
    /// 3, `WNOSIL_`), while they are out.
    pub line: Option<i32>,
    pub haul: Option<i32>,
    /// Where those pictures' top left stands (slot 1's `+0x59` / `+0x5d`,
    /// set as the line goes down; the haul's is the same).
    #[serde(default)]
    pub gear: (i32, i32),
    /// The hunt called off (`+0x77c < 0`: a stop while netting, 0x4abc60):
    /// the line comes up and the task ends at step 5.
    #[serde(default)]
    pub called_off: bool,
    /// Its commands held (0x4b37d0: `+0x261 &= 0x2000`) until 0x4b3850.
    pub held: bool,
}

impl Default for Hunt {
    /// As 0x4aab00 leaves it: step 1, the first plan at once.
    fn default() -> Self {
        Hunt {
            step: 1,
            planned: 0,
            plan_from: (-1, -1),
            plan_goal: (-1, -1),
            first_plan: true,
            catch: 0,
            turn_to: 0,
            turn: 0,
            frame: 0,
            line: None,
            haul: None,
            gear: (0, 0),
            called_off: false,
            held: false,
        }
    }
}

/// A ship being built on its yard's berth (see `Ship::build`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Build {
    /// The yard (`+0x75a`).
    pub yard: u32,
    /// Game ticks since its last step (`now - +0x57b`; `+0x57b` is 0 from
    /// the constructor 0x4dc3f0, so the first step comes at its first tick).
    pub since: u32,
    /// The picture's frame (`+0x8be +0x45`) in the hull's building
    /// sequence: from 0 for heading 0, from 40 for heading 6.
    pub frame: i32,
    /// Called off (`+0x73a < 0`): the frame runs back down.
    pub cancelled: bool,
    /// What it cost (amber, fish; `+0x544`), paid back if called off.
    pub price: (u32, u32),
    /// Its building frames are on the object (not saved: set again).
    #[serde(skip)]
    pub shown: bool,
    /// Shots and bombs landed on it since its last tick (event 1: at most
    /// 4, `+0x7de`), each its picture's letter and blow in the original's
    /// points; the count.
    #[serde(default)]
    pub hits: [(u8, u32); 4],
    #[serde(default)]
    pub hit_count: u8,
    /// A move it was sent (event 0x80, kept while it is built: the town's
    /// berth clearing, 0x4b4b50): the cell and priority.
    #[serde(default)]
    pub pending_move: Option<((i32, i32), u8)>,
}

/// What a grappled ship is doing, a game tick at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GrappleStage {
    /// Sliding alongside and turning parallel (0x4b54a0, 0x4b57e0).
    Pull,
    /// Furling the sail, a frame every 5 ticks (0x4b8aa0).
    Furl,
    /// The boarded ship waiting for the fight's end (0x4c3410 states 3, 4).
    Lie,
    /// The boarders' walkway `MOST0_` going down, a frame a tick (state 0xb).
    Plank,
    /// The fight `ABORD_`, a frame a tick (state 3).
    Fight,
    /// The walkway coming up again (state 0xc).
    Retract,
    /// Unfurling and sliding back to the cell's middle (state 6).
    Leave,
}

/// A loader's state in 0x4b73a0 (`+0x6a9`) past its throw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LoadStage {
    /// 7: the hook out, waiting for it to hold.
    Hook,
    /// 8: sliding alongside and turning parallel (0x4b54a0, 0x4b57e0).
    Pull,
    /// 10: furling, a frame every 5 ticks (0x4b8aa0).
    Furl,
    /// 11: the walkway `MOST0_` going down (or up when released).
    Plank,
    /// 4: linked for reloading.
    Linked,
    /// 12: the walkway coming up.
    PlankUp,
    /// 6: back to the cell's middle and unfurling.
    Back,
}

/// The loader's contact `+0x8e6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LoadContact {
    /// None (freed).
    Free,
    /// The hook in the air (type 300).
    Flying,
    /// Taken by the building (0x32) or the ship (200).
    Taken,
}

/// A loader's run (see `Ship::load_run`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LoadRun {
    pub target: u32,
    /// The target is a building (contact kind 0x32), else a ship (200).
    pub building: bool,
    pub stage: LoadStage,
    pub contact: LoadContact,
    /// Released (`+0x6a2` < 0: a stop, Finish reload, the target lost).
    pub released: bool,
    /// The throw's tick (`+0x6be`) and the hook's time (`+0x6c2`).
    pub thrown: u32,
    pub flight: u32,
    /// Where it lies alongside, the middle of its cell, where the
    /// current slide began, the heading it turns to, and the direction of
    /// the target (`+0x6e7`, the walkway's facing).
    pub at: (f32, f32),
    pub home: (f32, f32),
    pub from: (f32, f32),
    pub face: u8,
    pub facing: u8,
    /// The slide's point index (`+0x6b6`) and its count (`+0x6db`).
    pub point: u32,
    pub count: u32,
    /// Ticks in the stage; the walkway's frame (-1 none); the hull's
    /// sail frame past 8h + 0x58 (-1 not furled).
    pub ticks: u32,
    pub plank: i32,
    pub sail: i32,
    /// The hook's line still runs to where it landed (till the furl's
    /// end).
    pub rope: bool,
    pub hook_at: (f32, f32),
}

/// A ship lying grappled to `with` (see `Ship::grapple`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Grapple {
    pub with: u32,
    /// It is the boarder (else the boarded ship).
    pub boarder: bool,
    pub stage: GrappleStage,
    /// Where it lies alongside, and the middle of its cell it returns to.
    pub at: (f32, f32),
    pub home: (f32, f32),
    /// Where the current slide began.
    pub from: (f32, f32),
    /// The heading it turns to (parallel to the other) and the direction
    /// of the other ship (`+0x6e7`, the walkway's facing).
    pub face: u8,
    pub facing: u8,
    /// Ticks in the stage, and seconds owed to the next tick.
    pub ticks: u32,
    pub clock: f32,
    /// The fight's length in ticks and whether the boarders win it.
    pub fight: u32,
    pub won: bool,
    /// The hook's line still runs to the other ship (held by the boarded
    /// one until its sail is furled).
    pub rope: bool,
}

/// A founding ship's landing (0x4b3370, `+0x755`): closing in on the
/// shore (2), furling (8), the walkway `AMOST_` going down (4, 5), the
/// party `ANOSIL_` going ashore (6), the walkway up (7), backing out (3).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Landing {
    pub state: u8,
    /// Called off by a stop (`+0x73a` < 0).
    pub abort: bool,
    /// The direction of the land cell (`+0x76b`, 0x59791d) and its shore
    /// tile's group (`+0x748`, 0x4b3770); both kept from the last close-in
    /// when the shore has no group.
    pub dir: u8,
    pub group: Option<u8>,
    /// The direction of the land cell as this close-in read it (`+0x76d`,
    /// written before the group is looked for): the walkway's picture.
    #[serde(default)]
    pub walk_dir: u8,
    /// The close-in line (0x4b5220, `+0x766`; false once freed, or none
    /// made): from, to and its points (`+0x75e`, kept from the last
    /// close-in when none is made); the point it is at (`+0x74d`) and the
    /// one backing out takes (`+0x751`).
    #[serde(default = "landing_line")]
    pub line: bool,
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub count: i32,
    pub at: i32,
    pub back: i32,
    /// The heading it turns to (`+0x76a`) and the way (`+0x76c`).
    pub face: u8,
    pub turn: i8,
    /// The hull's furling frame (0-3 from `h x 8 + 0x58`), and ticks since
    /// its last frame (`+0x81`).
    pub furl: i32,
    pub hull_ticks: u32,
    /// The walkway's frame (`+0x8c2`) and the party's (`+0x8c6`), while
    /// shown.
    pub plank: Option<i32>,
    pub porters: Option<i32>,
    /// The walkway's top-left (picture slot 1's).
    pub plank_at: (f32, f32),
    /// Seconds owed to the next tick.
    pub clock: f32,
    /// At a dock (the repair order's machine, 0x4b2420): the dock it is in
    /// contact with (`+0x8e6`, the dock's occupant `+0x1e0` the ship).
    #[serde(default)]
    pub dock: Option<u32>,
    /// The amber gather's take of this round of the porters (`+0x782`,
    /// 0x4afdd0), stored aboard at the round's end (0x4ad300).
    #[serde(default)]
    pub load: i32,
}

fn landing_line() -> bool {
    true
}

/// The fields of a ship's close-in (0x4b2ff0) that outlast its landing:
/// the direction (`+0x76b`), the shore's group (`+0x748`), the heading
/// turned to (`+0x76a`), the way (`+0x76c`) and the line's points
/// (`+0x75e`; the line itself is freed as the landing ends, 0x4b1c60 /
/// 0x4b1d40). 0 on a new ship (zeroed at 0x4dc3f0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CloseIn {
    pub dir: u8,
    pub group: u8,
    pub face: u8,
    pub turn: i8,
    pub count: i32,
}

/// What a ramming ship is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RamStage {
    /// Sailing at the target until its bow touches the hull (state 2).
    Charge,
    /// The splinters `BOMBH30` at the bow, a frame a tick; the blow at
    /// their end (state 3).
    Impact,
    /// Backing off to the middle of its cell (state 6).
    Back,
}

/// Ship types that back off from a ram with their under-way frames run
/// backwards (0x597a24).
const BACKS_UNDER_WAY: [bool; 12] = [
    false, false, true, true, true, true, true, true, true, true, false, true,
];

/// A ram under way (see `Ship::ram_run`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RamRun {
    pub target: u32,
    pub stage: RamStage,
    /// The middle of the cell it charged from.
    pub home: (f32, f32),
    /// The tick it set off on while charging; then ticks into the stage.
    pub ticks: u32,
    pub clock: f32,
    /// The impact picture's length in ticks.
    pub frames: u32,
}

/// A rammed ship's state 6 (0x4c3ec0), entered from state 3 on the
/// blow's message (0x301) with its contact a ship (type 200): the hull
/// jolts a rotation over for a tick, then slides (0x4c3070) and is done.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RamHit {
    /// The jolt `+0x726` (0x4c3cc0: -1, 0 or 1).
    pub rock: i8,
    /// The hull's frame `+0x45` it set, while `rock` is not 0.
    pub frame: Option<u8>,
    /// The step `+0x72b`: 0 the jolt, 1 the hull back, 2 the slide.
    pub step: u8,
    /// Where the slide (0x4b5220, `+0x6df`) runs from and to; `None` for
    /// none (`+0x6db` 0).
    pub from: (f32, f32),
    pub to: Option<(f32, f32)>,
    /// Ticks into the slide (step 2).
    pub ticks: u32,
    pub clock: f32,
}

/// What a ship's picture shows (0x4e5da0 picks its composite's layers):
/// its hull band (0x4be030: 1 above 70% of its hull, 2 above 35%, else 3),
/// its ram and armour levels, and whether it is the Argo (ship +0x259, set
/// by the loader 0x4dfa90 for a type-2 ship so marked).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShipLook {
    pub band: u8,
    pub ram: u8,
    pub armour: u8,
    pub argo: bool,
}

impl ShipLook {
    pub fn of(ship: &Ship, combat: Option<&crate::combat::Combatant>) -> Self {
        let pct = combat.map_or(100, |c| {
            (c.hull.max(0.0) * 100.0 / c.max_hull.max(1.0)) as i32
        });
        Self {
            band: match pct {
                71.. => 1,
                36.. => 2,
                _ => 3,
            },
            ram: ship.ram,
            armour: ship.armour,
            argo: ship.argo,
        }
    }

    /// The composite layers shown (0x4e5da0): the wake (0); the hull,
    /// whole (1) or damaged (2); the sail, whole (7) or torn (8), the
    /// Argo's 9 or 10; the ram for its level (3, 4) and the armour for its
    /// (5, 6).
    pub fn layers(self) -> u32 {
        let mut mask = match (self.band, self.argo) {
            (1, false) => 0x83,
            (1, true) => 0x203,
            (2, false) => 0x103,
            (2, true) => 0x403,
            (_, false) => 0x105,
            (_, true) => 0x405,
        };
        mask |= match self.ram {
            1 => 0x08,
            2 => 0x10,
            _ => 0,
        };
        mask |= match self.armour {
            1 => 0x20,
            2 => 0x40,
            _ => 0,
        };
        mask
    }
}

/// Ship type `kind`'s frames for nation `nation`, player `player`, showing
/// `look` (composite `SHIP_<type><nation><nation><player>0`).
pub fn ship_frames(
    data: &GameData,
    caches: &mut Caches,
    kind: u8,
    nation: u32,
    player: u32,
    look: ShipLook,
) -> Result<Vec<Vec<Layer>>> {
    let letter = (b'A' + kind.min(25)) as char;
    let name = format!("SHIP_{letter}{nation}{nation}{player}0");
    let mask = look.layers();
    composite_frames_with(data, caches, &name, 0..usize::MAX, |k, _| {
        k < 32 && mask & (1 << k) != 0
    })
}

/// A warship's behaviour switches (ship byte +0x684 bits 0-2 and +0x633;
/// the manual's "Ship AI options") and the masks its own target search
/// reads. The constructor (0x4dc3f0) zeroes `+0x5d1..+0x695`: a new ship
/// is off guard, every mask 0, fire state 0, until a behaviour reaches it
/// (`launch`, `command`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Behaviour {
    /// Fire at enemies in range.
    pub guard: bool,
    /// Ram enemy ships when attacking them.
    pub ram: bool,
    /// Board enemy ships when attacking them.
    pub board: bool,
    /// Go after whoever fires at the ship (needs `guard`).
    pub aggressor: bool,
    /// `+0x685`: the weapon positions its own search fires (bit k the
    /// position group of weapon slots 3k..3k+2: 0x4e8af0 returns only
    /// those). Written from a record's byte `+0x11`.
    #[serde(default)]
    pub positions: u8,
    /// `+0x689`: the players it may target (bit nation x 4 + slot;
    /// 0x4bfc60, 0x4c0cd0).
    #[serde(default)]
    pub players: u32,
    /// `+0x68d`: the classes it may target at will (0x50ebb0 over
    /// `combat::class_bit`; 0x4bfc60 at 0x4bffb9).
    #[serde(default)]
    pub classes: u32,
    /// `+0x5d1`, the fire state (0x4c1240: 1 and 4 the search 0x4bfc60,
    /// 2 0x4c0eb0, 3 the attack 0x4c09c0). Kept as the original writes it
    /// (see `Ship::fire_wind_down`, `Ship::fire_reset`, `Ship::take_attack`),
    /// read by the at-will look (`scene::at_will`) and the attack zone's
    /// sends (0x4d60c0); the port's fire itself is still its own model.
    #[serde(default)]
    pub fire_state: u8,
    /// The weapon positions' records (0x12 bytes from `+0x63c`, position k
    /// the items 3k..3k+2; a fourth, items 9-11, for types 8 and 9).
    #[serde(default)]
    pub aims: [Aim; 4],
}

/// A weapon position's record (`+0x63c + 0x12k`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Aim {
    /// `+0x640`: its target's id, 0 for none (the pointer `+0x63c` is
    /// looked up by it).
    pub target: u32,
    /// `+0x645`: its state (1 a target, 3 the attack).
    pub state: u8,
    /// `+0x644`: the target's player code as the record was taken
    /// (0x4ab3f0: nation x 4 + slot, -1 for side 2), by which 0x4ac050
    /// finds it again (`Scene::player_code`).
    #[serde(default)]
    pub player: i8,
}

/// A behaviour record (the ship's event 0x100, 0x4a9e30 kinds 4-11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BehaviourKind {
    /// 4: guard on.
    Guard,
    /// 5: guard off.
    GuardOff,
    /// 6: ram attack on.
    Ram,
    /// 7: ram attack off.
    RamOff,
    /// 8: board attack on.
    Board,
    /// 9: board attack off.
    BoardOff,
    /// 10: fire on aggressor on (`+0x633 = 1`).
    Aggressor,
    /// 11: fire on aggressor off.
    AggressorOff,
}

impl Behaviour {
    /// Applies one behaviour record (0x4a9e30, disassembly
    /// 0x4aa1b0-0x4aa20a): the switch, then for the four "on" kinds the
    /// masks (`+0x685` = record byte `+0x11`, `+0x689` = record `+8`,
    /// `+0x68d` = record `+4`) and fire state 4, unless a fire task runs
    /// (`+0x5d1 != 0` and `+0x25d & 0x100`: `firing`). The "off" kinds
    /// leave the masks as they were.
    pub fn apply(
        &mut self,
        kind: BehaviourKind,
        players: u32,
        classes: u32,
        positions: u8,
        firing: bool,
    ) {
        use BehaviourKind as K;
        let on = matches!(kind, K::Guard | K::Ram | K::Board | K::Aggressor);
        match kind {
            K::Guard | K::GuardOff => self.guard = on,
            K::Ram | K::RamOff => self.ram = on,
            K::Board | K::BoardOff => self.board = on,
            K::Aggressor | K::AggressorOff => self.aggressor = on,
        }
        if on {
            self.positions = positions;
            self.players = players;
            self.classes = classes;
            if self.fire_state == 0 || !firing {
                self.fire_state = 4;
            }
        }
    }

    /// The ship's behaviour command `#` (0x4e2ba0 case 0x23, disassembly
    /// 0x4e2c34-0x4e2d03) with payload players `+1`, classes `+5` and
    /// `third` `+9` (its low byte the positions, bit 25 ram, bit 26
    /// board). Sub 1 turns on the guard, and ram / board when their bit is
    /// set, each only where off (a switch already on gets no record and
    /// keeps its masks); 2 guard off; 3 / 4 fire on aggressor on / off
    /// (each only when it changes); 5 / 6 board / ram on (where off); 0xe /
    /// 0xf board / ram off. Any other sub does nothing.
    pub fn command(&mut self, sub: u8, players: u32, classes: u32, third: u32, firing: bool) {
        use BehaviourKind as K;
        let positions = third as u8;
        let send = |b: &mut Behaviour, k| b.apply(k, players, classes, positions, firing);
        match sub {
            1 => {
                if !self.guard {
                    send(self, K::Guard);
                }
                if !self.ram && third & 0x200_0000 != 0 {
                    send(self, K::Ram);
                }
                if !self.board && third & 0x400_0000 != 0 {
                    send(self, K::Board);
                }
            }
            2 => send(self, K::GuardOff),
            3 if !self.aggressor => send(self, K::Aggressor),
            4 if self.aggressor => send(self, K::AggressorOff),
            5 if !self.board => send(self, K::Board),
            6 if !self.ram => send(self, K::Ram),
            0xe => send(self, K::BoardOff),
            0xf => send(self, K::RamOff),
            _ => {}
        }
    }

    /// 0x4e7220: guard on, then ram attack for kinds 5 and 10 and board
    /// attack for 4 and 11, each record with players and classes -1 and
    /// positions 0xff (`+0x11`): every mask all ones, fire state 4. Run
    /// by the map loader for every ship (0x4dfa90 at 0x4dfd12, record
    /// `+0x10 == 0`) and at the launch for a human's ship (0x4b3fcf).
    pub fn launch(&mut self, kind: u8, firing: bool) {
        use BehaviourKind as K;
        self.apply(K::Guard, u32::MAX, u32::MAX, 0xff, firing);
        if matches!(kind, 10 | 5) {
            self.apply(K::Ram, u32::MAX, u32::MAX, 0xff, firing);
        }
        if matches!(kind, 11 | 4) {
            self.apply(K::Board, u32::MAX, u32::MAX, 0xff, firing);
        }
    }

    /// Whether the at-will search may pick an object of player `owner`
    /// (nation, slot) whose class bit is `class` (0x4bfc60: `+0x689` at
    /// 0x4bfe7f, `+0x68d` through 0x50ebb0 at 0x4bffb9) with some weapon
    /// position enabled (`+0x685`: 0x4e8af0 mode 10 returns only those).
    pub fn may_pick(&self, (nation, slot): (u32, u32), class: u32) -> bool {
        self.positions != 0
            && self.players & 1 << ((slot + nation * 4) & 0x1f) != 0
            && self.classes & class != 0
    }
}

/// A treasure slot's healing (0x4b98b0): its count and its flag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HealSlot {
    pub count: u32,
    pub mends: bool,
}

/// A ship's Magic object (`+0x7d2`, 0x42ed20): the counts `+0x10 + 4 s`
/// by spell index 0..13 and its player (`+8` nation, `+0xc` slot), set at
/// each count (0x4ba070, 0x4b9bf0). Its cell (`+0`, `+4`) is the ship's,
/// set there and at each cell the ship steps into (0x4a3f40 at 0x4a48d7):
/// the port takes the ship's cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShipMagic {
    pub counts: [u32; 14],
    pub owner: Option<(u32, u32)>,
}

/// The Gorgon's head's spell index in a ship's magic (0x4ba070) and its
/// slot's parameter (0x4bd0c0): the stare, Magic.cpp 0x42f580 case 0xd.
pub const HEAD_SPELL: u32 = 0xd;

/// Whether a treasure kind is of 0x4b9740's class 8 (9, 0x11-0x1a: the
/// spells, the Gorgon's head and the heroes' items), whose slot 0x4bd0c0
/// flags (`+0x485`) and offers to 0x4ba070.
pub fn treasure_kept(kind: u8) -> bool {
    matches!(kind, 9 | 0x11..=0x1a)
}

impl Ship {
    /// Puts a treasure aboard in the next slot (0x4bd0c0): a Cup's or a
    /// Fleece's count from 0 (0x4bd23e), its flags 0x4b9740's class, or
    /// 4 for player 16's ship (`barbarian`).
    ///
    /// A treasure of class 8 is counted in the ship's magic (0x4ba070):
    /// kind 9 its spell, the Gorgon's head (kind 0x11, its slot's
    /// parameter set 0xd) spell 0xd; the magic made if none, its player
    /// the ship's `owner`.
    pub fn add_treasure(&mut self, mut tr: Treasure, owner: Option<(u32, u32)>) {
        let barbarian = owner == Some((4, 0));
        self.sync_heal_slots(barbarian);
        if tr.kind == 0x11 {
            tr.param = HEAD_SPELL;
        }
        let spell = match tr.kind {
            9 => Some(tr.param),
            0x11 => Some(HEAD_SPELL),
            _ => None,
        };
        if let Some(s) = spell {
            let m = self.magic.get_or_insert_with(ShipMagic::default);
            m.owner = owner;
            if let Some(c) = m.counts.get_mut(s as usize) {
                *c += 1;
            }
        }
        self.treasures.push(tr);
        self.heal_slots.push(HealSlot {
            count: 0,
            mends: !barbarian && matches!(tr.kind, 15 | 16),
        });
    }

    /// Takes treasure slot `k` out, the slots after it moving down with
    /// their counts (0x4bd4f0).
    pub fn remove_treasure(&mut self, k: usize) -> Treasure {
        if k < self.heal_slots.len() {
            self.heal_slots.remove(k);
        }
        // A slot of class 8 (`+0x485`) frees the ship's magic when its
        // counts are all 0 (0x4bd500); the counts are not lowered.
        let tr = self.treasures.remove(k);
        if treasure_kept(tr.kind) && self.magic.is_some_and(|m| m.counts.iter().all(|&c| c == 0)) {
            self.magic = None;
        }
        tr
    }

    /// Whether a slot of class 8 is aboard (0x4b9fd0).
    pub fn keeps_treasure(&self) -> bool {
        self.treasures.iter().any(|t| treasure_kept(t.kind))
    }

    /// A hero comes aboard in the next slot (0x4bd350): Argus's count from
    /// 0 (his rate 20).
    pub fn add_hero(&mut self, hero: u8) {
        if hero == 5 {
            self.argus_count = 0;
        }
        self.heroes.push(hero);
    }

    /// Slots without their healing record (treasures set aboard with the
    /// mission) get one as 0x4bd0c0 gives: count 0, the flag by the kind
    /// and the owner.
    pub fn sync_heal_slots(&mut self, barbarian: bool) {
        self.heal_slots.truncate(self.treasures.len());
        let have = self.heal_slots.len();
        for tr in &self.treasures[have..] {
            self.heal_slots.push(HealSlot {
                count: 0,
                mends: !barbarian && matches!(tr.kind, 15 | 16),
            });
        }
    }

    /// `+0x25d & 0x100`, a fire task: set only by 0x4ab3f0 on taking the
    /// fire records (orders 0x15 / 0x16, 0x4ab580) and by the stop's
    /// fire part (0x4aa7b8, 0x4abdf2, `fire_wind_down`); cleared only by
    /// that (0x4aa799, 0x4abdd2).
    pub fn fire_task(&self) -> bool {
        self.firing
    }

    /// Weapon positions in play: 3, 4 for types 8 and 9 (`1 << type &
    /// 0x300`).
    pub fn positions(&self) -> usize {
        if matches!(self.kind, 8 | 9) {
            4
        } else {
            3
        }
    }

    /// The stop's fire part (0x4abc60's tail at 0x4abdc0-0x4abdf8, and
    /// 0x4aa540's for a stop with 0x100 in its mask, 0x4aa781-0x4aa7c4):
    /// the fire task ends (`+0x25d &= ~0x100`), and with any of guard, ram
    /// or board on (`+0x684`) the ship takes the fire state 2 and the fire
    /// task again.
    pub fn fire_wind_down(&mut self) {
        self.firing = false;
        let b = &mut self.behaviour;
        if b.guard || b.ram || b.board {
            b.fire_state = 2;
            self.firing = true;
        }
    }

    /// 0x4abe00, at the turn of the ship's pending order group (0x4ab720
    /// at 0x4ab854, with the fire task on and `+0x251 == 0`): unless the
    /// fire state is 2 it becomes 1, and each position with a target in
    /// state 3 goes back to 1.
    pub fn fire_reset(&mut self) {
        if !self.firing {
            return;
        }
        let n = self.positions();
        let b = &mut self.behaviour;
        if b.fire_state == 2 {
            return;
        }
        b.fire_state = 1;
        for a in &mut b.aims[..n] {
            if a.target != 0 && a.state == 3 {
                a.state = 1;
            }
        }
    }

    /// The attack's fire records taken (0x4ab3f0 kind 3, every position:
    /// order 0x16 sub 7): each position `armed` (a unit of its items
    /// aboard, `+0x289 + 3k..`, or the fourth) takes state 3 and the
    /// target, and the fire state is 3; each other position with a target
    /// not in state 3 at this one goes to 1. The fire task is on
    /// (0x4ab580) whether a position took it or not.
    pub fn take_attack(&mut self, target: u32, player: i8, armed: [bool; 4]) {
        let n = self.positions();
        let b = &mut self.behaviour;
        for (k, &on) in armed.iter().enumerate().take(n) {
            if !on {
                continue;
            }
            b.aims[k] = Aim {
                target,
                state: 3,
                player,
            };
            b.fire_state = 3;
            // 0x4ab6a5: the next plan at once, 25 ticks apart after.
            self.attack_plan = RoutePlan {
                first_plan: true,
                ..Default::default()
            };
            for (j, a) in b.aims[..3].iter_mut().enumerate() {
                if j != k && a.target != 0 && (a.state != 3 || a.target != target) {
                    a.state = 1;
                }
            }
        }
        self.firing = true;
    }

    /// 0x4e0b50, asked by every plan (0x4a6330): lying still, turning or
    /// at a cell (move states 0, 2, 3) it is no longer between cells;
    /// whether it plans from its own cell, not the one it is entering.
    pub fn plans_from_own_cell(&self) -> bool {
        if matches!(self.move_state, 0 | 2 | 3) {
            self.between.set(false);
        }
        !self.between.get()
    }

    /// The cell it is going into while under way between cells, as the
    /// behaviours' look boxes take it (0x4ac8d0 and others: `+0x596` and
    /// move state 4 or more, `+0x5b6`).
    pub fn under_way_into(&self) -> Option<(i32, i32)> {
        (self.between.get() && self.move_state > 3).then_some(self.entering)
    }

    /// Whether a treasure of `kind` is aboard.
    pub fn has_treasure(&self, kind: u8) -> bool {
        self.treasures.iter().any(|t| t.kind == kind)
    }

    /// Its way in world pixels a second.
    pub fn velocity(&self) -> f32 {
        self.way as f32 / 6000.0 / crate::clock::TICK
    }

    /// Whether hero `hero` (see `HERO_NAMES`) is aboard.
    pub fn has_hero(&self, hero: u8) -> bool {
        self.heroes.contains(&hero)
    }
}

/// Heroes by number.
pub const JASON: u8 = 0;
pub const THESEUS: u8 = 1;
pub const ORPHEUS: u8 = 2;
pub const ZETES: u8 = 3;
pub const HERCULES: u8 = 4;
pub const ARGUS: u8 = 5;
pub const ATALANTA: u8 = 6;

/// Treasure kinds with lasting effects aboard.
pub const FIRE_TORCH: u8 = 10;
pub const ARMOUR_OF_ARES: u8 = 11;
pub const FEATHER: u8 = 12;
pub const MAGIC_WOOD: u8 = 13;

/// Theseus aboard: each shot's blow x 1.5 (0x4c0230, as the Flame), and
/// the weapons' range at its best research level (0x4bfc60, 0x4c07c0).
pub const THESEUS_DAMAGE: f32 = 1.5;
/// The best research level, which Theseus gives the range and Atalanta the
/// rate of fire of every weapon aboard (0x4c0230).
pub const HERO_LEVEL: u8 = 2;
/// The Armour of Ares takes this share of hull damage when its record
/// gives none (miss001's gives 40).
pub const ARMOUR_OF_ARES_PERCENT: u32 = 40;

/// Goods that can be gathered and carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Resource {
    Amber,
    Fish,
    /// A leviathan, taken as fish are.
    Leviathan,
}

/// A deposit of a resource on the map.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Deposit {
    pub resource: Resource,
    /// Units left, or `None` for an inexhaustible deposit.
    pub remaining: Option<f32>,
    /// Times an exhausted amber deposit grows back (record field 9), and
    /// to how much (0x48a430).
    #[serde(default)]
    pub regrows: u32,
    #[serde(default)]
    pub refill: f32,
}

/// Seconds an exhausted amber deposit lies empty before it grows back: 100
/// ticks of its state machine (0x48a430, state 3).
pub const REGROW_SECONDS: f32 = 100.0 * crate::clock::TICK;

/// A merchant's last gather command (0x4aab00): the deposit's class, its
/// cell and the box of cells about it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Gathered {
    pub class: Resource,
    pub cell: (i32, i32),
    pub rect: (i32, i32, i32, i32),
}

/// A ship's standing order.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Task {
    /// Shuttle between a deposit and the nearest port.
    Gather { deposit: u32, phase: GatherPhase },
    /// Chase and fire at an enemy ship or building. Ordered at a land
    /// creatures' jam (class 0x1f7), `jam` keeps its id (the ship's
    /// record's; the fleet attack task sends it at the jam again,
    /// `Scene::run_fleet_attacks`); `target` is the member it was given.
    Attack {
        target: u32,
        #[serde(default)]
        jam: Option<u32>,
    },
    /// Sail to a land cell and put up a building there: a settlement or a
    /// fort (the ships' building page, 0x4e6980 page 2).
    Found {
        cx: i32,
        cy: i32,
        #[serde(default = "settlement_class")]
        class: u32,
        /// The berth it sails to (`+0x756`, 0x4b2710's water cell) and the
        /// cell it lands on (`+0x740`: the pair's land cell for a
        /// settlement, the site for a fort).
        #[serde(default)]
        berth: (i32, i32),
        #[serde(default)]
        land: (i32, i32),
    },
    /// Run into an enemy ship.
    Ram { target: u32 },
    /// Close in on an enemy ship and throw grappling hooks onto it
    /// (0x4b5df0 states 1 and 7, 0x4b84b0): `thrown` is the scene clock
    /// when the last hook went (`+0x6be`), `hooking` while one flies. Once
    /// a hook holds, the ships lie grappled (`Ship::grapple`).
    Board {
        target: u32,
        #[serde(default)]
        thrown: Option<f32>,
        #[serde(default)]
        hooking: bool,
    },
    /// Sail to an own dock and be repaired there (task `+0x72f` 1); with
    /// `sell` (`+0x749 == 3`, ship command 0x1e byte 0 3) sold there
    /// instead, as the dock takes it (0x4b2506).
    Repair {
        dock: u32,
        #[serde(default)]
        sell: bool,
    },
    /// Being sold at a dock (task 5, 0x4b4380, `+0x251 == 2`).
    Sell(Sale),
    /// Sail alongside own settlement `site` and take one `item` of its
    /// stock aboard.
    Fetch { site: u32, item: u8 },
    /// Sail out to scout, then back to `home`.
    Scout { home: (f32, f32) },
    /// Sail to `to`, then back to `from`, and so on.
    Patrol { from: (f32, f32), to: (f32, f32) },
    /// Hold the world rectangle `rect` (x0, y0, x1, y1): fight enemy ships
    /// that come into it (a guard zone), or with `attack`, every enemy
    /// ship and building in it until none is left (an attack zone).
    Zone {
        rect: (f32, f32, f32, f32),
        attack: bool,
    },
    /// Keep within `range` cells of own ship `ward` and guard it (a
    /// follow order, message `*`: the player's convoy 2, a computer's idle
    /// zone gatherer 1; 0x4ab250, 0x4a5f30).
    Convoy {
        ward: u32,
        #[serde(default = "convoy_range")]
        range: u8,
    },
    /// Sail to a treasure or a hero's house and take it (event 0x40
    /// class 700 or 0x49, 0x4aab00). `at`: a treasure lying on the sea,
    /// its cell (`+0x792`, 0x4af940), where the order ends.
    Take {
        target: u32,
        #[serde(default)]
        at: Option<(i32, i32)>,
        /// A treasure on land: the sea cell sailed to and the land cell
        /// landed on (`+0x792`, `+0x797`, 0x4af940).
        #[serde(default)]
        land: Option<((i32, i32), (i32, i32))>,
        /// The treasure's cell, the landing party's site (`+0x7c2`,
        /// `+0x7c6`).
        #[serde(default)]
        site: (i32, i32),
        /// The machine (`+0x772`, `+0x77d`): 0 sailing (sub-code 9), 1 the
        /// landing's close-in asked each tick (sub-code 10 step 1; past it
        /// `Ship::landing`), 5 backed out, lying with nothing more to do.
        #[serde(default)]
        step: u8,
        /// A hero's house: its 0x24af0 answer at the walkway's last frame
        /// (`+0x78a`, 0x4ba6b0): the hero, or 0xff once taken.
        #[serde(default)]
        hero: Option<u8>,
    },
    /// Sail alongside own port, shipyard, dock or ship `target` and reload
    /// there (Reload Object; see `scene::load`).
    LoadAt { target: u32 },
    /// Reload Object at own port, shipyard, dock or fort ruins `target`:
    /// the closing in of the load (`+0x696` 3, 0x4b7370), which never
    /// finds them (see `scene::closing`).
    LoadRuins { target: u32 },
}

/// A ship being sold (task 5, 0x4b4380; `+0x755` its state): 1 just
/// taken, 2 sailing to its cell's middle and turning to heading 0 or 6
/// (0x4b4220), 3 its building picture running down (`+0x8be +0x45`).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sale {
    pub state: u8,
    /// The line to the cell's middle (0x4b5220, `+0x766`): its ends in
    /// world points, its count (`+0x75e`, 0 with none) and the point at
    /// (`+0x74d`).
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub count: i32,
    pub at: i32,
    /// The heading it turns to (`+0x76a`) and the way (`+0x76c`).
    pub face: u8,
    pub turn: i8,
    /// The picture's frame in state 3.
    pub frame: i32,
}

/// A ship's patience with each new order (+0x5c8).
pub const PATIENCE: u8 = 5;

fn default_patience() -> u8 {
    PATIENCE
}

impl Task {
    /// The priority its order gives the ship (+0x5c7, a byte of the order
    /// the navy sends, the player's orders going through the navy too):
    /// gathering 90 (0x4c89b0), taking a treasure 90 for a computer
    /// (0x4d9f50; the player's 0, 0x49e0e0, see the move loop), founding
    /// 100 (0x4cb0b0), docking for repairs 80 (0x4dbb40). Attacking,
    /// boarding, ramming and loading reach the ship through intakes that
    /// leave the byte unread, so 0 (0x4a9f2d stores bytes 0-6 only and
    /// 0x4aa840 reads +0x823, never written); patrols, scouting, escorts
    /// and zones move with plain moves and follows, 0 as well.
    pub fn priority(&self) -> Option<u8> {
        match self {
            Task::Gather { .. } | Task::Take { .. } => Some(90),
            Task::Found { .. } => Some(100),
            Task::Repair { .. } => Some(80),
            _ => Some(0),
        }
    }

    /// The kind of order, 1 up.
    pub fn tag(&self) -> u8 {
        match self {
            Task::Gather { .. } => 1,
            Task::Attack { .. } => 2,
            Task::Found { .. } => 3,
            Task::Ram { .. } => 4,
            Task::Board { .. } => 5,
            Task::Repair { .. } => 6,
            Task::Fetch { .. } => 7,
            Task::Scout { .. } => 8,
            Task::Patrol { .. } => 9,
            Task::Zone { .. } => 10,
            Task::Convoy { .. } => 11,
            Task::Take { .. } => 12,
            Task::LoadAt { .. } | Task::LoadRuins { .. } => 13,
            Task::Sell(_) => 14,
        }
    }
}

/// A sea monster's type (record field 10, 1 for 4 or more; 0x492462).
pub fn sea_monster_type(field: u32) -> u32 {
    if field >= 4 {
        0
    } else {
        field
    }
}

fn convoy_range() -> u8 {
    2
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GatherPhase {
    ToDeposit,
    Collecting,
    ToPort(u32),
    /// Porters carrying the goods into the building.
    Unloading(u32),
}

/// Cargo capacity of a merchant ship type, in units of goods: the manual's
/// cargo displacement (boat 4, merchant 10) times 100.
pub fn cargo_capacity(kind: u8) -> f32 {
    match kind {
        0 => 400.0,
        1 => 1000.0,
        _ => 0.0,
    }
}

pub const SHIP_HEADINGS: usize = 8;
const SHIP_FRAMES_PER_HEADING: usize = 4;
/// First of a ship's 24 turning frames and of its 32 lying-still frames.
const SHIP_TURN_FRAMES: usize = 32;
const SHIP_IDLE_FRAMES: usize = 56;
/// Milliseconds a frame of a ship's animation, under way or turning: its
/// picture's frame delay is 2 (0x4e0160 and the ship loader), a frame once
/// more than 2 ticks have passed (0x4a3320).
pub const SHIP_FRAME_MS: u64 = 3 * TICK_MS;
/// Furling the sail goes slower: the delay of 4 a ship gets lying against
/// something (0x4b5df0), a frame every 5 ticks.
pub const SHIP_FURL_SECONDS: f32 = 5.0 * crate::clock::TICK;
pub(crate) const SHIP_FURL_FRAMES: usize = 88;
/// The furling's four frames done: the sail furled.
pub const SHIP_FURLED: f32 = 4.0;

impl MapObject {
    /// How many units of each item 0-8 a ship carries: its marines and
    /// its weapons.
    pub fn unit_counts(&self) -> [usize; 9] {
        let mut n = [0; 9];
        if let Some(s) = &self.ship {
            for (n, &q) in n.iter_mut().zip(&s.crew.squads) {
                *n = q as usize;
            }
        }
        if let Some(c) = &self.combat {
            for &w in &c.weapons {
                if (3..9).contains(&w) {
                    n[w as usize] += 1;
                }
            }
        }
        n
    }

    /// A ship's units' conditions by item, as many as it carries (units
    /// added since are whole; those gone are the last).
    pub fn units(&mut self) -> Option<&mut [Vec<u8>; 9]> {
        let n = self.unit_counts();
        let s = self.ship.as_mut()?;
        for (c, n) in s.conditions.iter_mut().zip(n) {
            c.resize(n, 100);
        }
        Some(&mut s.conditions)
    }

    /// The conditions of a ship's units by item, without changing it.
    pub fn unit_conditions(&self) -> [Vec<u8>; 9] {
        let n = self.unit_counts();
        let mut out: [Vec<u8>; 9] = Default::default();
        if let Some(s) = &self.ship {
            for (k, o) in out.iter_mut().enumerate() {
                *o = s.conditions[k].clone();
                o.resize(n[k], 100);
            }
        }
        out
    }

    /// Whether it holds units in places as a fort does (forts 0x34-0x36,
    /// barbarian structures 0x3f-0x41): its `combat` weapons, with their
    /// conditions in `combat.conditions`.
    pub fn holds_units(&self) -> bool {
        self.ship.is_none()
            && self.combat.is_some()
            && crate::combat::holds_units(self.record.class_id)
    }

    /// A fort's units' conditions by item (its places `+0x4d0 + 0x23 +
    /// item x 4`), each place's in order; missing conditions are whole.
    pub fn fort_places(&self) -> [Vec<u8>; 9] {
        let mut places: [Vec<u8>; 9] = Default::default();
        if let Some(c) = self.combat.as_ref().filter(|_| self.holds_units()) {
            for (k, &w) in c.weapons.iter().enumerate() {
                if let Some(p) = places.get_mut(w as usize) {
                    p.push(c.conditions.get(k).copied().unwrap_or(100));
                }
            }
        }
        places
    }

    /// Sets a fort's units to `places` (conditions by item), its weapons
    /// in item order.
    pub fn set_fort_places(&mut self, places: [Vec<u8>; 9]) {
        if !self.holds_units() {
            return;
        }
        let c = self.combat.as_mut().unwrap();
        c.weapons.clear();
        c.conditions.clear();
        for (k, p) in places.iter().enumerate() {
            for &cond in p {
                c.weapons.push(k as u8);
                c.conditions.push(cond);
            }
        }
    }

    /// Removes a ship's units at condition 0 (0x4bc6c0).
    pub fn drop_broken_units(&mut self) {
        let Some(units) = self.units() else {
            return;
        };
        let mut gone = [0u32; 9];
        for (k, u) in units.iter_mut().enumerate() {
            let before = u.len();
            u.retain(|&c| c > 0);
            gone[k] = (before - u.len()) as u32;
        }
        for (k, &g) in gone.iter().enumerate() {
            for _ in 0..g {
                self.remove_unit(k as u8);
            }
        }
    }

    /// Takes one unit of item `item` off a ship's count (its condition is
    /// the caller's).
    fn remove_unit(&mut self, item: u8) {
        if item < 3 {
            if let Some(s) = self.ship.as_mut() {
                s.crew.squads[item as usize] = s.crew.squads[item as usize].saturating_sub(1);
            }
        } else if let Some(c) = self.combat.as_mut() {
            if let Some(p) = c.weapons.iter().rposition(|&w| w == item) {
                c.weapons.remove(p);
            }
        }
    }

    /// Takes unit `index` of item `item` off a ship; its condition.
    pub fn take_unit(&mut self, item: u8, index: usize) -> Option<u8> {
        let units = self.units()?;
        let list = units.get_mut(item as usize)?;
        if index >= list.len() {
            return None;
        }
        let c = list.remove(index);
        self.remove_unit(item);
        Some(c)
    }

    /// Puts a unit of item `item` at condition `condition` aboard a ship,
    /// last of its item.
    pub fn add_unit(&mut self, item: u8, condition: u8) {
        if self.units().is_none() || item >= 9 {
            return;
        }
        if item < 3 {
            self.ship.as_mut().unwrap().crew.squads[item as usize] += 1;
        } else if let Some(c) = self.combat.as_mut() {
            c.weapons.push(item);
        } else {
            return;
        }
        self.ship.as_mut().unwrap().conditions[item as usize].push(condition);
    }

    /// The mean condition of a ship's units of item `item` (0x4bc360,
    /// `+0x419`), 100 for others.
    pub fn mean_condition(&self, item: u8) -> u32 {
        if item >= 9 {
            return 100;
        }
        let u = if self.ship.is_some() {
            self.unit_conditions()[item as usize].clone()
        } else if self.holds_units() {
            // A fort's place fires with its units' points (0x446710).
            self.fort_places()[item as usize].clone()
        } else {
            return 100;
        };
        if u.is_empty() {
            return 100;
        }
        u.iter().map(|&c| c as u32).sum::<u32>() / u.len() as u32
    }

    pub fn is_armed(&self) -> bool {
        !self.shooters().is_empty()
    }

    /// The positions 0x4ab3f0 takes a fire record for: a unit of the
    /// position's items aboard (`+0x289 + 3k`, `+0x28a + 3k`, `+0x28b +
    /// 3k`: marines 0-2 by squad, weapons 3-8), and the fourth position
    /// (k = 3, types 8 and 9) always.
    pub fn armed_positions(&self) -> [bool; 4] {
        let mut armed = [false, false, false, true];
        let squads = self.ship.as_ref().map_or([0; 3], |s| s.crew.squads);
        let weapons = self.combat.as_ref().map_or(&[][..], |c| &c.weapons[..]);
        for (k, a) in armed.iter_mut().take(3).enumerate() {
            *a = (3 * k as u8..3 * k as u8 + 3).any(|item| match item {
                0..=2 => squads[item as usize] > 0,
                _ => weapons.contains(&item),
            });
        }
        armed
    }

    /// What fires from the object: its weapons, then (on ships) a shot for
    /// each squad of bowmen and javelin men.
    pub fn shooters(&self) -> Vec<u8> {
        let Some(c) = self.combat.as_ref() else {
            return Vec::new();
        };
        let mut items: Vec<u8> = c
            .weapons
            .iter()
            .copied()
            .filter(|&w| crate::combat::fires(w))
            .collect();
        if let Some(s) = &self.ship {
            for kind in 0..2u8 {
                items.extend(std::iter::repeat_n(
                    kind,
                    s.crew.squads[kind as usize] as usize,
                ));
            }
        }
        items
    }

    pub fn frame_at(&self, time_ms: u64) -> &[Layer] {
        if self.frames.is_empty() {
            return &[];
        }
        if let (Some(p), false) = (self.progress, self.frames.is_empty()) {
            let n = self.frames.len();
            return &self.frames[((p * n as f32) as usize).min(n - 1)];
        }
        let step = (time_ms / self.frame_ms.max(1)) as usize;
        let n = self.frames.len();
        let i = match &self.ship {
            // A ship's composite: 8 headings x 4 frames under way (oars and
            // wake), the 24 hull rotations it turns through, then 8
            // headings x 4 frames lying still: at rest, rocking one way, at
            // rest, rocking the other.
            Some(ship) if n >= SHIP_IDLE_FRAMES + 32 => match ship.rotation {
                Some(r) => SHIP_TURN_FRAMES + r as usize % 24,
                // Rammed, its hull's frame as the hit set it (0x4c3ec0
                // state 6: the jolt, then the rotation of its heading).
                None if ship.ram_hit.is_some_and(|h| h.frame.is_some()) => {
                    ship.ram_hit.and_then(|h| h.frame).unwrap_or(0) as usize
                }
                // Lying loading or unloading, the sail furls (0x4b5df0, the
                // frames from 88, 8 a heading: four furling, then, the sail
                // furled (+0x25b = 5), four it rocks over, 0x4a3150).
                None if ship.furl > 0.0 && n >= SHIP_FURL_FRAMES + 64 => {
                    let base = SHIP_FURL_FRAMES + ship.heading * 8;
                    if ship.furl >= SHIP_FURLED {
                        base + 4 + ship.rock.min(3) as usize
                    } else {
                        base + (ship.furl as usize).min(3)
                    }
                }
                // Backing off from a ram, most types step their under-way
                // frames backwards, one every other step of 3 ticks
                // (0x4c29c7, the types flagged in 0x597a24).
                None if ship.ram_run.is_some_and(|r| r.stage == RamStage::Back)
                    && BACKS_UNDER_WAY[ship.kind.min(11) as usize] =>
                {
                    let back = ship.ram_run.map_or(0, |r| r.ticks as usize) / 6;
                    ship.heading * SHIP_FRAMES_PER_HEADING
                        + (SHIP_FRAMES_PER_HEADING - 1 - back % SHIP_FRAMES_PER_HEADING)
                }
                // At rest it rocks over four frames (0x4a3150), from 56, 4 a
                // heading.
                None if ship.path.is_empty() => {
                    SHIP_IDLE_FRAMES + ship.heading * 4 + ship.rock.min(3) as usize
                }
                None => ship.heading * SHIP_FRAMES_PER_HEADING + step % SHIP_FRAMES_PER_HEADING,
            },
            Some(ship) => {
                let per = SHIP_FRAMES_PER_HEADING.min(n.max(1));
                (ship.heading * per + step % per) % n.max(1)
            }
            None => step % n.max(1),
        };
        &self.frames[i]
    }
}

/// A harpy's (0x1f8) or dragon's sprite sequence for a state (0 resting,
/// 1 waiting, 2 taking off, 3 flying, 4 turning, 5 striking, 6 landing, 7
/// dying) and heading; resting, taking off and landing have only the odd
/// headings. Dragons never rest: they circle (flying).
pub fn flyer_sequence(class: u32, state: u8, heading: usize) -> String {
    let (prefix, state) = if class == 0x1f9 {
        ("DRGSPR", if state < 2 { 3 } else { state })
    } else {
        ("HRPSPR", state)
    };
    let heading = if matches!(state, 0 | 2 | 6) {
        heading | 1
    } else {
        heading
    };
    format!("{prefix}_{state}{}", heading.min(7))
}

/// Amber deposits (Grig/amber.cpp) match the shore they lie on: the
/// base-layer tile codes of each of the 20 variants (0x5b97c8, 0x24 bytes
/// each: four codes, then two rectangles in the cell where the sparkle may
/// show, x, y, width, height).
/// A variant's tile codes and sparkle rectangles.
pub type AmberVariant = ([u8; 4], [(i32, i32, i32, i32); 2]);
pub const AMBER_VARIANTS: [AmberVariant; 20] = [
    ([2, 3, 4, 5], [(37, 28, 26, 30), (14, 41, 23, 22)]),
    ([6, 0, 0, 0], [(12, 17, 16, 46), (28, 17, 36, 20)]),
    ([7, 8, 9, 10], [(0, 27, 24, 29), (25, 40, 24, 23)]),
    ([11, 0, 0, 0], [(0, 20, 38, 17), (37, 20, 18, 43)]),
    ([12, 13, 14, 15], [(16, 0, 25, 23), (40, 11, 20, 27)]),
    ([16, 0, 0, 0], [(11, 0, 17, 36), (13, 36, 50, 19)]),
    ([17, 18, 19, 20], [(0, 16, 26, 23), (18, 2, 31, 22)]),
    ([21, 0, 0, 0], [(0, 39, 42, 17), (35, 3, 19, 45)]),
    ([30, 0, 0, 0], [(7, 3, 20, 60), (7, 3, 20, 60)]),
    ([31, 0, 0, 0], [(12, 3, 20, 60), (12, 3, 20, 60)]),
    ([32, 0, 0, 0], [(39, 3, 20, 60), (39, 3, 20, 60)]),
    ([33, 0, 0, 0], [(31, 3, 24, 60), (31, 3, 24, 60)]),
    ([34, 0, 0, 0], [(2, 30, 61, 22), (2, 30, 61, 22)]),
    ([35, 0, 0, 0], [(0, 27, 61, 15), (0, 27, 61, 15)]),
    ([36, 0, 0, 0], [(0, 37, 61, 23), (0, 37, 61, 23)]),
    ([37, 0, 0, 0], [(1, 27, 61, 21), (1, 27, 61, 21)]),
    ([38, 0, 0, 0], [(2, 36, 61, 17), (2, 36, 61, 17)]),
    ([39, 0, 0, 0], [(1, 27, 61, 21), (1, 27, 61, 21)]),
    ([40, 0, 0, 0], [(2, 35, 60, 16), (2, 35, 60, 16)]),
    ([41, 0, 0, 0], [(1, 21, 61, 23), (1, 21, 61, 23)]),
];

/// The amber variant for a cell whose base tile has code `tile` (0x489a10;
/// variant 0 when none matches).
pub fn amber_variant(tile: u8) -> usize {
    AMBER_VARIANTS
        .iter()
        .position(|(codes, _)| tile != 0 && codes.contains(&tile))
        .unwrap_or(0)
}

/// An amber deposit's frames (0x489cd0): its variant's picture for how
/// much is left (`level` 3 full to 1, frame (variant + 1) x 3 - level of
/// `AMBER_`), still.
pub fn amber_frames(
    data: &GameData,
    caches: &mut Caches,
    variant: usize,
    level: usize,
) -> Result<Vec<Vec<Layer>>> {
    let all = sequence_frames(data, caches, "AMBER_")?;
    let v = variant.min(AMBER_VARIANTS.len() - 1);
    let i = ((v + 1) * 3).saturating_sub(level.clamp(1, 3));
    // The sparkle comes in bursts of its own (`scene::sparkle`).
    Ok(all.get(i).or(all.first()).cloned().into_iter().collect())
}

/// A flyer's frames: each its shadow on the water (layer 1, sequence +
/// `S`), then the creature in the air (layer 0). 0x41e4b0 places both
/// (0x50d660, their top left `+0x59`, `+0x5d`) at the flyer's place less
/// (0x28, 0x78) for a harpy; a dragon's creature less (0x43, 0x92), its
/// shadow less (0x43, 0xd5). The shadow layer has only colour 0x26 put
/// through the shadow table (0x4207b0 at 0x42098e: 0x50e130(1, 1, 0x26,
/// 0x5c1358); the shadow sprites are all of that colour).
pub fn flyer_frames(data: &GameData, caches: &mut Caches, name: &str) -> Result<Vec<Vec<Layer>>> {
    let ((ox, oy), shadow_oy) = if name.starts_with("DRG") {
        ((0x43, 0x92), 0xd5)
    } else {
        ((0x28, 0x78), 0x78)
    };
    // The port centres a layer on the object's place plus (dx, dy)
    // (`render::layer_origin`): the offsets that put its top left there.
    let place = |l: &mut Layer, oy: i32| {
        l.dx = l.image.width as i32 / 2 - ox;
        l.dy = l.image.height as i32 / 2 - oy;
    };
    let mut frames = sequence_frames(data, caches, name)?;
    for l in frames.iter_mut().flatten() {
        place(l, oy);
    }
    if let Ok(shadows) = caches.shaded_frames(data, &format!("{name}S"), 0x26) {
        for (frame, shadow) in frames.iter_mut().zip(shadows) {
            let body = std::mem::take(frame);
            *frame = shadow
                .into_iter()
                .map(|mut l| {
                    place(&mut l, shadow_oy);
                    l
                })
                .chain(body)
                .collect();
        }
    }
    Ok(frames)
}

/// Result of loading a mission's objects.
pub struct Objects {
    /// Drawable objects, sorted back to front.
    pub drawn: Vec<MapObject>,
    /// Records that could not be drawn yet, by class id.
    pub unresolved: HashMap<u32, usize>,
}

pub fn load(data: &GameData, caches: &mut Caches, mission: &Archive) -> Result<Objects> {
    let mut drawn = Vec::new();
    let mut unresolved = HashMap::<u32, usize>::new();
    for rec in object_records(mission)? {
        match resolve(data, caches, &rec)? {
            Some(obj) => drawn.push(obj),
            None => *unresolved.entry(rec.class_id).or_default() += 1,
        }
    }
    // Painter's order: objects further down the map are in front.
    drawn.sort_by(|a, b| a.y.total_cmp(&b.y));
    Ok(Objects { drawn, unresolved })
}

/// Where an object's frames are anchored on the map.
enum Anchor {
    /// Centred on the centre of a cell.
    CellCentre(u32, u32),
    /// Frame top-left at the cell's top-left corner minus an offset.
    CellCorner(u32, u32, i32, i32),
}

/// What to draw and how fast it animates.
struct Visual {
    frames: Vec<Vec<Layer>>,
    frame_ms: u64,
}

/// Builds the drawable object for a mission record (also used to create
/// objects during play, from synthetic records).
pub fn resolve(
    data: &GameData,
    caches: &mut Caches,
    rec: &ObjectRecord,
) -> Result<Option<MapObject>> {
    let f = |i| rec.field(i).unwrap_or(0);
    let class = rec.class();
    // Frame period from a per-state animation cycle length in game ticks.
    let cycle =
        |cycle_ticks: u64, frames: usize| (cycle_ticks / frames.max(1) as u64).max(1) * TICK_MS;

    let (anchor, visual) = match class {
        ObjectClass::Building(cls) | ObjectClass::Barbarian(cls) => {
            let condition = rec.fields.get(7).map_or(100, |&c| c.min(100)) as f32;
            let state = BuildingState::for_condition(condition);
            let Some(name) = building_name(cls, f(field::NATION), f(10), state) else {
                return Ok(None);
            };
            let frames = composite_frames(data, caches, &name, 0..usize::MAX)?;
            let frame_ms = building_frame_ms(frames.len());
            // The picture's top left is the cell's corner less a per-class
            // offset (LandObjClassTy 0x422e30; the constructors' +0xc4,
            // +0xc8): ports and docks (0, 7), forts (4, 12), settlements and
            // towns none (measured on the original), the rest (4, 4).
            let anchor = match cls {
                0x39..=0x3b => CellCorner(f(5), f(6), 0, 7),
                0x34..=0x36 => CellCorner(f(5), f(6), 4, 12),
                0x32 | 0x33 => CellCorner(f(5), f(6), 0, 0),
                0x37 | 0x38 | 0x3c | 0x3d | 0x3e => CellCorner(f(5), f(6), 4, 4),
                _ => CellCentre(f(5), f(6)),
            };
            (anchor, Visual { frames, frame_ms })
        }
        ObjectClass::Ship => {
            // 8 headings x 4 frames under sail, then (frames 88-151) 8
            // headings x 8 frames furling the sail; the layers as the
            // ship's look picks them (see `ShipLook`), a new ship whole.
            let look = ShipLook {
                band: 1,
                ram: crate::combat::Crew::from_record(&rec.fields, 0).class,
                armour: rec.fields.get(15).map_or(0, |&v| v.min(2) as u8),
                argo: false,
            };
            let frames = ship_frames(data, caches, f(7) as u8, f(5), f(6), look)?;
            (
                CellCentre(f(9), f(10)),
                Visual {
                    frames,
                    frame_ms: SHIP_FRAME_MS,
                },
            )
        }
        ObjectClass::Castle(cls) => {
            // CastleClassTy init (0x410ca0): prefix, cycle table and offsets.
            let (prefix, dx, dy) = if cls == 0x42 {
                ("CSTLSEA", 6, 12)
            } else {
                ("CSTLWAR", 4, 13)
            };
            const LIVE: u64 = 1;
            let frames = sequence_frames(data, caches, &format!("{prefix}SPR_{LIVE}3"))?;
            let frame_ms = cycle(CASTLE_CYCLE[LIVE as usize], frames.len());
            (CellCorner(f(5), f(6), dx, dy), Visual { frames, frame_ms })
        }
        // Treasures lying on the map (field 5 is -1; otherwise the id of
        // the object holding them) sparkle at the cell in fields 9 and 10.
        ObjectClass::Treasure if f(5) == u32::MAX => {
            // Afloat it bobs (state 2, 0x406a10): frames 8-22 (30-59 for
            // the big 16 and 27), a frame every 3 ticks (2 for those;
            // 0x4068e0). Frames before rise into view, after sink.
            let kind = f(4).min(27);
            let all = sequence_frames(data, caches, &format!("TREAS{kind:02}"))?;
            let big = kind == 16 || kind == 27;
            let (from, to, ticks) = if big { (30, 59, 2) } else { (8, 22, 3) };
            let frames = if all.len() > from {
                all[from..=to.min(all.len() - 1)].to_vec()
            } else {
                all
            };
            (
                CellCentre(f(9), f(10)),
                Visual {
                    frames,
                    frame_ms: ticks * TICK_MS,
                },
            )
        }
        ObjectClass::HeroHouse => {
            let frames = hero_house_frames(data, caches, f(22).min(8) as u8, 1)?;
            // A land building (0x422e30): its cycle 50, 75, 75, 57 ticks
            // (0x595e80, constructor 0x420fd0), standing 75.
            let frame_ms = building_frame_ms(frames.len());
            (CellCentre(f(5), f(6)), Visual { frames, frame_ms })
        }
        ObjectClass::HarpyNest => {
            // 0x41d010: dragon (0x44) or harpy (0x45) nest; the frame shows
            // damage (0 intact, 1 below 66 %, 2 below 33 %).
            let name = if rec.class_id == 0x44 {
                "DNIDSPR"
            } else {
                "HNIDSPR"
            };
            let mut frames = sequence_frames(data, caches, name)?;
            frames.truncate(1);
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::CreatureSpawn => {
            // Lairs of the cyclopes (0x46), minotaurs (0x47) and amazons
            // (0x48), named like buildings: nation 4, standing, intact.
            let prefix = match rec.class_id {
                0x46 => "CLPF",
                0x47 => "MNTF",
                _ => "AMZF",
            };
            let frames =
                composite_frames(data, caches, &format!("{prefix}SPR_413"), 0..usize::MAX)?;
            // Land buildings too (0x422e30): standing, 75 ticks a round.
            let frame_ms = building_frame_ms(frames.len());
            (CellCentre(f(4), f(5)), Visual { frames, frame_ms })
        }
        ObjectClass::Monster(0x1fb) => {
            // Sea monster, `UMON<type><state><heading>` (0x490040): states 0
            // surfacing, 1 swimming, 2 diving, 3 striking, 4 dying; its type
            // record field 10, 0 for 4 or more (0x492462).
            let frames =
                sequence_frames(data, caches, &format!("UMON{}10", sea_monster_type(f(10))))?;
            (
                CellCentre(f(7), f(8)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Monster(cls @ 0x1f4..=0x1f6) => {
            // A cyclops, minotaur or amazon out of its lair (CyclopClassTy),
            // appearing.
            let name = crate::scene::walker_sequence(cls, 0, 0);
            let mut frames = sequence_frames(data, caches, &name)?;
            crate::scene::anchor_walker_frames(cls, &mut frames);
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: crate::scene::walker_frame_ms(0),
                },
            )
        }
        ObjectClass::Monster(cls @ (0x1f8 | 0x1f9)) => {
            // A harpy or dragon (HarpyClassTy, 0x4207b0) out of its nest:
            // `HRPSPR_`/`DRGSPR_` + state + heading; resting at first.
            // A frame a tick (delays 0x595d10, 0x595d30).
            let frames = flyer_frames(data, caches, &flyer_sequence(cls, 0, 1))?;
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: crate::clock::TICK_MS,
                },
            )
        }
        ObjectClass::Monster(cls @ (0x1fa | 0x1fd)) => {
            // Medusa (`GORG%1d`) and siren, at the centre of their area.
            let name = if cls == 0x1fa { "GORG1" } else { "SIRENA" };
            let frames = sequence_frames(data, caches, name)?;
            (
                CellCentre(f(5), f(6)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Amber => {
            let frames = sequence_frames(data, caches, "AMBER_")?;
            (
                CellCorner(f(6), f(7), 0, 0),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::FishZone => {
            // Zones are areas the creatures roam; show one in the middle.
            let (x, y) = (f(6) + f(8) / 2, f(7) + f(9) / 2);
            let frames = sequence_frames(data, caches, "FISH_12")?;
            (
                CellCorner(x, y, 0, 0),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::WhaleZone => {
            // A leviathan starts at the cell of fields 7 and 8 (0x494f90
            // message 2; fields 10 and 11 are the reach of its bounds about
            // it); its pictures are its states' (`scene::leviathan`).
            let frames = composite_frames(data, caches, "WHALE_00", 0..usize::MAX)?;
            (
                CellCentre(f(7), f(8)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS,
                },
            )
        }
        ObjectClass::Whirlpool => {
            const LIVE: usize = 1;
            let frames = sequence_frames(data, caches, &format!("WHIRLSPR_{LIVE}"))?;
            let frame_ms = cycle(WHIRL_CYCLE[LIVE], frames.len());
            (CellCorner(f(5), f(6), 0, 0), Visual { frames, frame_ms })
        }
        ObjectClass::Reef(cls) => {
            const LIVE: usize = 1;
            let size = if cls == 0x259 { 'B' } else { 'M' };
            let kind = f(7).clamp(1, 3);
            let frames = sequence_frames(data, caches, &format!("{size}REEF{kind}SPR_{LIVE}"))?;
            let frame_ms = cycle(REEF_CYCLE[LIVE], frames.len());
            (CellCorner(f(5), f(6), 0, 0), Visual { frames, frame_ms })
        }
        ObjectClass::Lava => {
            let frames = sequence_frames(data, caches, &format!("VOLCANOLAVA{:02}", f(4)))?;
            // A frame every 3 ticks (the volcano, 0x445630).
            (
                CellCorner(f(6), f(7), 0, 0),
                Visual {
                    frames,
                    frame_ms: 3 * TICK_MS,
                },
            )
        }
        ObjectClass::Sheep => {
            let frames = sheep_frames(data, caches, f(7))?;
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: DEFAULT_FRAME_MS * 2,
                },
            )
        }
        // A sunken chest (0x2c1): its cell fields 4, 5, its kind 6, lying
        // field 7 (the port's; the original saves its state, 0x405130).
        ObjectClass::Other(0x2c1) => {
            let cell = (f(4) as i32, f(5) as i32);
            let frames = crate::scene::chest_frames(data, caches, f(6), f(7) == 1, cell);
            (
                CellCentre(f(4), f(5)),
                Visual {
                    frames,
                    frame_ms: 2 * TICK_MS,
                },
            )
        }
        _ => return Ok(None),
    };
    let banner = match class {
        ObjectClass::Building(cls) if f(field::NATION) < 4 => {
            let (nation, slot) = (f(field::NATION), f(field::PLAYER));
            let at = banner_offset(cls, nation);
            match (
                at,
                caches.composite(data, &format!("FLAGSPR_{nation}{slot}"))?,
            ) {
                (Some((dx, dy)), Some(comp)) => {
                    let mut frames = Vec::new();
                    for k in 0..comp.frames {
                        if let Some(c) = comp.frame(k).first() {
                            if let Some(img) = caches.sprites.get(data, &c.sprite)? {
                                frames.push(img);
                            }
                        }
                    }
                    (!frames.is_empty()).then(|| (Rc::new(frames), dx, dy))
                }
                _ => None,
            }
        }
        _ => None,
    };
    let Visual { frames, frame_ms } = visual;
    let Some(first) = frames.iter().flatten().next() else {
        return Ok(None);
    };
    let t = TILE_SIZE as f32;
    let cell = match (class, &anchor) {
        (
            ObjectClass::Building(_)
            | ObjectClass::Barbarian(_)
            | ObjectClass::Castle(_)
            | ObjectClass::HarpyNest
            | ObjectClass::CreatureSpawn
            | ObjectClass::HeroHouse,
            CellCorner(cx, cy, ..) | CellCentre(cx, cy),
        ) => Some((*cx as i32, *cy as i32)),
        _ => None,
    };
    let (x, y) = match anchor {
        CellCentre(cx, cy) => (cx as f32 * t + t / 2.0, cy as f32 * t + t / 2.0),
        CellCorner(cx, cy, dx, dy) => (
            cx as f32 * t - dx as f32 + first.image.width as f32 / 2.0,
            cy as f32 * t - dy as f32 + first.image.height as f32 / 2.0,
        ),
    };
    let ship = (class == ObjectClass::Ship).then(|| Ship {
        kind: f(7).min(11) as u8,
        heading: (f(11) as usize) % SHIP_HEADINGS,
        speed: SHIP_SPEED.get(f(7) as usize).copied().unwrap_or(100) as f32 * SPEED_SCALE,
        path: Default::default(),
        rotation: None,
        turn_clock: 0.0,
        blocked: 0.0,
        priority: 0,
        patience: PATIENCE,
        order_tag: 0,
        waited: 0.0,
        stalled: 0.0,
        remembered: Vec::new(),
        planned: 0.0,
        pending: None,
        way: 0,
        way_rest: 0,
        brake: 0,
        accel: None,
        ahead: None,
        move_state: 0,
        between: std::cell::Cell::new(false),
        entering: (0, 0),
        goal: None,
        home_port: 0,
        last_deposit: 0,
        gathered: None,
        berth_wait: 0,
        blow: None,
        face: None,
        furl: 0.0,
        rock: 0,
        rock_back: false,
        rock_ticks: 0,
        rock_clock: 0.0,
        grapple: None,
        ram_run: None,
        ram_hit: None,
        load_run: None,
        landing: None,
        close_in: CloseIn::default(),
        build: None,
        lcg: None,
        hunt: None,
        closing: None,
        attack_plan: RoutePlan::default(),
        ram_wait: 0,
        board_wait: 0,
        firing: false,
        fire_times: [0; 12],
        items_ready: [false; 12],
        positions_ready: [false; 4],
        cargo: 0.0,
        cargo_kind: None,
        task: None,
        crew: crate::combat::Crew::from_record(&rec.fields, f(7).min(11) as u8),
        // Armour level in field 15, ram level in field 14 less 3 (the
        // loader 0x4dfa90: ship +0x27a, +0x27b).
        armour: rec.fields.get(15).map_or(0, |&v| v.min(2) as u8),
        ram: crate::combat::Crew::from_record(&rec.fields, 0).class,
        // The map loader (0x4dfa90) runs 0x4e7220 on every ship it makes;
        // the yard's (0x4dfe00) does not (`Scene::make_ship`).
        behaviour: {
            let mut b = Behaviour::default();
            b.launch(f(7).min(11) as u8, false);
            b
        },
        treasures: Vec::new(),
        magic: None,
        heroes: Vec::new(),
        argo: false,
        experience: (f(13) & 0xff) as i32 * 100,
        attack_step: false,
        look: None,
        // Fields 29-100: each unit's condition, eight per item (the
        // loader 0x4dfe00); as many count as the ship carries.
        conditions: std::array::from_fn(|k| {
            rec.fields
                .get(29 + k * 8..37 + k * 8)
                .map_or(Vec::new(), |c| {
                    c.iter().map(|&v| v.min(100) as u8).collect()
                })
        }),
        heal_slots: Vec::new(),
        argus_count: 0,
        dock_step: None,
        slide_end: false,
        // Record field 8 (`+0x20`) masked by the type's (0x4dfa90).
        caps: f(8) & caps_mask(f(7).min(11) as u8),
    });
    let combat = crate::combat::Combatant::from_record(rec, class);
    // Deposits: amber holds field 4 x 100 units; a negative amount starts
    // empty and grows to that much; it grows back field 9 times
    // (AmberClassTy init, 0x48a800). Fishing zones: field 4 x 100,
    // negative for unlimited.
    let mut dormant = None;
    let deposit = match class {
        ObjectClass::Amber => {
            let amount = f(4) as i32;
            let dep = Deposit {
                resource: Resource::Amber,
                remaining: Some(amount.max(0) as f32 * 100.0),
                regrows: f(9),
                refill: amount.unsigned_abs() as f32 * 100.0,
            };
            if amount > 0 {
                Some(dep)
            } else {
                dormant = Some((dep, 0.0));
                None
            }
        }
        ObjectClass::FishZone => {
            let amount = f(4) as i32;
            Some(Deposit {
                resource: Resource::Fish,
                remaining: (amount >= 0).then_some(amount as f32 * 100.0),
                regrows: 0,
                refill: 0.0,
            })
        }
        // A leviathan (WhaleClassTy 0x494f90): 100 x field 4 to take
        // (`+0x78`), and as much again each time it comes up after a catch
        // (`+0x8c`, 0x494b90 state 6).
        ObjectClass::WhaleZone => Some(Deposit {
            resource: Resource::Leviathan,
            remaining: Some((f(4) as i32 * 100) as f32),
            regrows: 0,
            refill: (f(4) as i32 * 100) as f32,
        }),
        _ => None,
    };
    let mut obj = MapObject {
        id: rec.id,
        class,
        record: rec.clone(),
        picture: picture(rec),
        owner: match class {
            ObjectClass::Ship => Some((f(5), f(6))),
            ObjectClass::Building(_) | ObjectClass::Barbarian(_) => {
                Some((f(field::NATION), f(field::PLAYER)))
            }
            ObjectClass::Castle(_)
            | ObjectClass::HarpyNest
            | ObjectClass::CreatureSpawn
            | ObjectClass::Monster(0x1f4..=0x1f6 | 0x1f8..=0x1fb | 0x1fd) => {
                Some(crate::combat::MONSTERS)
            }
            _ => None,
        },
        label: label(rec),
        x,
        y,
        frames,
        frame_ms,
        ship,
        deposit,
        dormant,
        combat,
        treasure: (class == ObjectClass::Treasure).then(|| Treasure {
            kind: f(4).min(27) as u8,
            param: f(7),
            amount: f(8),
        }),
        progress: None,
        banner,
        cell,
        // The first look at load + `r % 15` ticks, `r` its generator's
        // first draw (0x4281de, 0x4422ad), which the scene takes
        // (`Scene::building_draw`).
        asleep: (matches!(class, ObjectClass::Building(_)) && f(4) == 3).then_some(0.0),
        made_depth: None,
    };
    obj.made_depth = crate::render::made_depth(&obj);
    Ok(Some(obj))
}

/// Where a building's banner stands from the building's top left, by class
/// and nation (0-3): the tables each class's constructor sets (+0xdc, read
/// by 0x4247b0); settlements, towns, ports and yards fitted to the
/// original's first frame of miss000 (Greek).
fn banner_offset(class: u32, nation: u32) -> Option<(i32, i32)> {
    let table: [(i32, i32); 4] = match class {
        0x32 | 0x33 => [(37, 33); 4],
        0x39 => [(20, -15); 4],
        0x3a | 0x3b => [(36, -19); 4],
        0x34 => [(36, -16), (35, -9), (37, -22), (35, 1)],
        0x35 => [(36, -16), (32, -23), (37, -22), (34, 2)],
        0x36 => [(36, -16), (41, -23), (37, -22), (35, -22)],
        0x37 => [(37, -15), (44, 3), (52, -20), (49, -20)],
        0x38 => [(35, -17), (48, -16), (52, -3), (49, -20)],
        0x3c => [(38, -21), (37, -11), (38, -22), (34, -12)],
        0x3d => [(42, -13), (41, -13), (45, -26), (41, -14)],
        0x3e => [(56, -16), (48, -21), (37, -23), (42, -14)],
        _ => return None,
    };
    table.get(nation as usize).copied()
}

/// Ship type names, indexed by the type in mission records (field 7),
/// which is also the sprite letter (`'A' + type`) and the index of the cost
/// tables. The barbarians' five types (1, 2, 3, 5, 6) are the manual's
/// merchant, royal ship, bomber, rammer and ship killer. Build commands map
/// to types in 0x42d750.
pub const SHIP_NAMES: [&str; 12] = [
    "Boat",
    "Merchant",
    "Royal Ship",
    "Bomber",
    "Boarder",
    "Rammer",
    "Ship Killer",
    "Men Killer",
    "Fire Ship",
    "Quick Lime Ship",
    "Fire Rammer",
    "Ship of the Dead",
];

/// The side of a ship type's frame (its `SHIP_` composite), centred on the
/// ship: offsets in the original count from its top-left.
pub const SHIP_FRAME: [f32; 12] = [
    56.0, 64.0, 80.0, 80.0, 72.0, 72.0, 72.0, 72.0, 64.0, 72.0, 72.0, 72.0,
];

/// Game ticks a player building's picture takes to play through in each
/// state (construction, standing, ruins, ...): the constructors' cycle
/// tables (+0xc0, e.g. 0x596ab8), alike for all of them.
pub const BUILDING_CYCLE: [u32; 4] = [200, 75, 75, 75];

/// Milliseconds a frame of a standing building's `frames`-frame picture:
/// the cycle over the frames, in whole ticks (0x422e30).
pub fn building_frame_ms(frames: usize) -> u64 {
    (BUILDING_CYCLE[1] as u64 / frames.max(1) as u64).max(1) * TICK_MS
}

/// Top speeds by ship type (0x5bb4e0; the manual's chart, which swaps the
/// sixth and seventh): x 400 in the original's units of 1/6000 pixel a
/// tick (0x4ac210).
/// Whether ships of type `kind` cannot pass over shoals: the royal ship,
/// bomber and rammer (the manual's comparison chart).
/// The draught flag of a ship type (0x597ce4: royal ships, bombers and
/// rammers), kept in `+0x592`.
pub fn deep_draught(kind: u8) -> bool {
    matches!(kind, 2 | 3 | 5)
}

const SHIP_SPEED: [u32; 12] = [150, 140, 100, 60, 90, 150, 120, 130, 90, 100, 120, 100];
/// World pixels per second per chart unit, before the wind factor (0.6 in
/// calm, up to 1.25 downwind; see `weather`): 400 units of 1/6000 pixel a
/// tick (0x4a3f40 moves a straight step half a pixel per 3000 units).
const SPEED_SCALE: f32 = 400.0 / 6000.0 / crate::clock::TICK;
/// The object panel picture (0x4f5800, 0x5004d0).
fn picture(rec: &ObjectRecord) -> Option<(String, (u32, u32))> {
    let f = |i| rec.field(i).unwrap_or(0);
    match rec.class() {
        ObjectClass::Building(cls) => {
            let n = cls + f(field::NATION) * 13 - 0x32;
            Some((format!("INF_OBJ{n:02}"), (8, 6)))
        }
        ObjectClass::Ship => {
            const BASE: [u32; 12] = [0, 8, 16, 32, 40, 48, 56, 64, 72, 80, 72, 80];
            let (nation, player) = (f(5), f(6));
            let mut n = BASE.get(f(7) as usize).copied()?;
            match nation {
                1 => n += player,
                3 => n += 4 + player,
                _ => {}
            }
            Some((format!("1INF_SHIP{n:02}"), (10, 9)))
        }
        _ => None,
    }
}

fn nation_name(n: u32) -> &'static str {
    match n {
        1 => "Greek",
        3 => "Persian",
        4 => "Barbarian",
        _ => "Neutral",
    }
}

/// Name of a player building class.
/// What the message line calls object `o` under the pointer (InfoStringTy
/// 0x506c70, message 0x627): a player's "<NATION> <CLASS>" (strings
/// 0x2f5-0x2f9 for Egypt, Greek, Rome, Persian, Barbarian; 0x1d between),
/// anything else its class alone (strings 0x2bc-0x2f4; ships by 200 plus
/// their type; "None", string 7, for what has none).
pub fn info_name(data: &GameData, o: &MapObject) -> String {
    let class = match o.class {
        ObjectClass::Ship => 200 + o.ship.as_ref().map_or(0, |s| s.kind as u32),
        _ => o.record.class_id,
    };
    let id = match class {
        0x32..=0x49 => 0x2bc + class - 0x32,
        200..=0xd3 => 0x2d4 + class - 200,
        0x191 => 0x2e0,
        0x192 => 0x2e1,
        400 => 0x2e2,
        600 => 0x2e3,
        0x259 => 0x2e4,
        0x25a => 0x2e5,
        0x25c => 0x2e6,
        0x25d => 0x2e7,
        0x25e => 0x2e8,
        500 => 0x2e9,
        0x1f5 => 0x2ea,
        0x1f6 => 0x2eb,
        0x1f8 => 0x2ec,
        0x1f9 => 0x2ed,
        0x1fa => 0x2ee,
        0x1fb => 0x2ef,
        0x1fd => 0x2f0,
        700 => 0x2f1,
        0x2c1 => 0x2f4,
        _ => 7,
    };
    let s = |id: u32| data.strings.get(&id).cloned().unwrap_or_default();
    match o.owner {
        Some((nation, _)) if nation < 5 => {
            let lead = [0x2f5, 0x2f7, 0x2f6, 0x2f8, 0x2f9][nation as usize];
            format!("{}{}{}", s(lead), s(0x1d), s(id))
        }
        _ => s(id),
    }
}

pub fn building_label(class: u32) -> &'static str {
    match class {
        0x32 => "Settlement",
        0x33 => "Town",
        0x34 => "Small Fortress",
        0x35 => "Medium Fortress",
        0x36 => "Big Fortress",
        0x37 => "Workshop",
        0x38 => "Smithy",
        0x39 => "Port",
        0x3a => "Shipyard",
        0x3b => "Dock",
        0x3c => "Sage School",
        0x3d => "Temple of the Sea",
        _ => "Temple of War",
    }
}

fn label(rec: &ObjectRecord) -> String {
    let f = |i| rec.field(i).unwrap_or(0);
    let owner =
        |nation: u32, player: u32| format!("{}, player {}", nation_name(nation), player + 1);
    match rec.class() {
        ObjectClass::Ship => {
            let name = SHIP_NAMES.get(f(7) as usize).copied().unwrap_or("Ship");
            format!("{name} ({})", owner(f(5), f(6)))
        }
        ObjectClass::Building(cls) => {
            let name = building_label(cls);
            format!("{name} ({})", owner(f(field::NATION), f(field::PLAYER)))
        }
        ObjectClass::Barbarian(_) => "Barbarian stronghold".into(),
        ObjectClass::Castle(c) => if c == 0x42 {
            "Sea castle"
        } else {
            "War castle"
        }
        .into(),
        ObjectClass::HarpyNest => if rec.class_id == 0x44 {
            "Dragon nest"
        } else {
            "Harpy nest"
        }
        .into(),
        ObjectClass::CreatureSpawn => match rec.class_id {
            0x46 => "Cyclops lair",
            0x47 => "Minotaur lair",
            _ => "Amazon village",
        }
        .into(),
        ObjectClass::HeroHouse => format!("House of {}", HERO_NAMES[f(22).min(8) as usize]),
        ObjectClass::Treasure => treasure_name(f(4).min(27) as u8),
        ObjectClass::Amber => "Amber".into(),
        ObjectClass::FishZone => "Fish".into(),
        ObjectClass::WhaleZone => "Leviathan".into(),
        ObjectClass::Monster(0x1fa) => "Medusa".into(),
        ObjectClass::Monster(0x1fb) => "Sea monster".into(),
        ObjectClass::Monster(0x1f4) => "Cyclops".into(),
        ObjectClass::Monster(0x1f5) => "Minotaur".into(),
        ObjectClass::Monster(0x1f6) => "Amazon".into(),
        ObjectClass::Monster(0x1f8) => "Harpy".into(),
        ObjectClass::Monster(0x1f9) => "Dragon".into(),
        ObjectClass::Monster(0x1fd) => "Siren".into(),
        ObjectClass::Whirlpool => "Whirlpool".into(),
        ObjectClass::Reef(_) => "Reef".into(),
        ObjectClass::Lava => "Volcano".into(),
        ObjectClass::Sheep => "Sheep".into(),
        other => format!("{other:?}"),
    }
}

use Anchor::{CellCentre, CellCorner};

/// Animation cycle lengths in game ticks, per state (0 appear, 1 live, ...).
const CASTLE_CYCLE: [u64; 4] = [300, 20, 100, 75]; // 0x5949e8
const WHIRL_CYCLE: [u64; 3] = [60, 60, 60]; // 0x5972b0
const REEF_CYCLE: [u64; 3] = [30, 60, 30]; // 0x596a50

/// How a building looks: being built, standing or in ruins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingState {
    Construction,
    /// Standing, with damage level 3 (intact), 2 or 1.
    Standing(u32),
    Ruins,
}

impl BuildingState {
    /// The standing state for a condition in percent (LandObjClassTy
    /// 0x422e30: above 67 intact, above 33 level 2, else 1).
    pub fn for_condition(percent: f32) -> Self {
        // ceil(health x 2 / 67) of the whole percent (0x594ab0), at least 1.
        let level = (percent.floor() * 2.0 / 67.0).ceil() as u32;
        Self::Standing(level.clamp(1, 3))
    }
}

/// A hero's house's picture in state `state` (0x422e30
/// `%sSPR_%1u%1u%1u`: the prefix `HHOUSE<hero>`, the nation digit and the
/// state, then the damage digit; standing 1, taken 4 by message 0x24af1):
/// `HHOUSE<hero>SPR_0<state>3`.
pub fn hero_house_frames(
    data: &GameData,
    caches: &mut Caches,
    hero: u8,
    state: u8,
) -> Result<Vec<Vec<Layer>>> {
    let name = format!("HHOUSE{hero}SPR_0{state}3");
    composite_frames(data, caches, &name, 0..usize::MAX)
}

/// Frames of a building record drawn in `state`.
pub fn building_frames(
    data: &GameData,
    caches: &mut Caches,
    rec: &ObjectRecord,
    state: BuildingState,
) -> Result<Vec<Vec<Layer>>> {
    let f = |i| rec.field(i).unwrap_or(0);
    let cls = rec.class_id;
    match building_name(cls, f(field::NATION), f(10), state) {
        Some(name) => composite_frames(data, caches, &name, 0..usize::MAX),
        None => Ok(Vec::new()),
    }
}

/// Composite name of a building (0x4288c0): prefix, nation, for some the
/// class, then the state (0 construction, 1 standing, 2 ruins), for ports
/// and docks the facing (0 water to the east, 2 north, 4 west, 6 south),
/// and the damage level (3 intact .. 1; construction 3, ruins 1).
pub(crate) fn building_name(
    cls: u32,
    nation: u32,
    facing: u32,
    state: BuildingState,
) -> Option<String> {
    let (state, damage) = match state {
        BuildingState::Construction => (0, 3),
        BuildingState::Standing(d) => (1, d.clamp(1, 3)),
        BuildingState::Ruins => (2, 1),
    };
    let (prefix, with_class, with_facing) = match cls {
        0x32 | 0x33 => ("TSPR_", true, false),
        0x34 => ("FORTSSPR_", false, false),
        0x35 => ("FORTSPR_", false, false),
        0x36 => ("FORTBSPR_", false, false),
        0x37 => ("STSPR_", false, false),
        0x38 => ("SMSPR_", false, false),
        0x39 => ("PTSPR_", true, true),
        0x3a | 0x3b => ("DKSPR_", true, true),
        0x3c => ("SLSPR_", false, false),
        0x3d => ("TSSPR_", false, false),
        0x3e => ("TWSPR_", false, false),
        // Barbarian structures. 0x40 embeds its class and a facing like
        // ports do; which of 0x3f/0x41 is the small and which the big
        // fort is inferred, not confirmed.
        0x3f => ("BARBSSPR_", false, false),
        0x40 => ("BARBSPR_", true, true),
        0x41 => ("BARBBSPR_", false, false),
        _ => return None,
    };
    let mut name = format!("{prefix}{nation}");
    if with_class {
        name += &format!("{cls:2}");
    }
    name += &state.to_string();
    if with_facing {
        name += &facing.to_string();
    }
    name += &damage.to_string();
    Some(name)
}

/// A small flock's still picture: `count` sheep around the cell centre
/// (what is picked and its bounds; the flock itself is drawn and moved by
/// `scene::sheep`).
fn sheep_frames(data: &GameData, caches: &mut Caches, count: u32) -> Result<Vec<Vec<Layer>>> {
    const SPOTS: [(i32, i32); 8] = [
        (-14, -6),
        (8, -12),
        (16, 6),
        (-4, 10),
        (-20, 14),
        (4, -2),
        (22, -8),
        (-10, -18),
    ];
    let mut frames: Vec<Vec<Layer>> = Vec::new();
    for (i, &(dx, dy)) in SPOTS.iter().take(count.clamp(1, 8) as usize).enumerate() {
        let names = caches.sequence(data, &format!("SHEEP_{}1", i % 4))?;
        for (f, n) in names.iter().enumerate() {
            if let Some(image) = caches.sprites.get(data, n)? {
                if frames.len() <= f {
                    frames.resize_with(f + 1, Vec::new);
                }
                frames[f].push(Layer { image, dx, dy });
            }
        }
    }
    Ok(frames)
}

/// A ship's sinking (composite mode 1, `SHIP_<letter><hull nation>001`,
/// 0x4e0120), its layer picked by the heading: 0x4dec40 sets `+0x74` 5
/// and 0x4e1bb0(0) loads layer 0 with the mask `1 << +0x5af` (`0x101 <<
/// +0x5af` while the ship burns, `+0x8ca`; every mode 1 composite has
/// eight layers, one a heading, so the high byte picks nothing). Empty
/// for a hull with no such composite: the original's picture is then
/// missing and nothing is drawn.
pub fn ship_sink_frames(
    data: &GameData,
    caches: &mut Caches,
    kind: u8,
    nation: u32,
    heading: usize,
) -> Result<Vec<Vec<Layer>>> {
    let letter = (b'A' + kind.min(25)) as char;
    composite_frames_with(
        data,
        caches,
        &format!("SHIP_{letter}{nation}001"),
        0..usize::MAX,
        |k, _| k == heading,
    )
}

/// A ship being built (composite mode 2), its layers picked as a whole
/// ship's (0x4e1bb0 sets state 4 and masks it through 0x4e5da0).
pub fn ship_build_frames(
    data: &GameData,
    caches: &mut Caches,
    kind: u8,
    nation: u32,
    argo: bool,
) -> Result<Vec<Vec<Layer>>> {
    let letter = (b'A' + kind.min(25)) as char;
    let mask = ShipLook {
        band: 1,
        ram: 0,
        armour: 0,
        argo,
    }
    .layers();
    composite_frames_with(
        data,
        caches,
        &format!("SHIP_{letter}{nation}002"),
        0..usize::MAX,
        |k, _| k < 32 && mask & (1 << k) != 0,
    )
}

/// Composite `name`'s frames with the layers of `mask` (bit k layer k;
/// a picture of kind 8, 0x50d080).
pub fn composite_masked_frames(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
    mask: u32,
) -> Result<Vec<Vec<Layer>>> {
    composite_frames_with(data, caches, name, 0..usize::MAX, |k, _| {
        k < 32 && mask & (1 << k) != 0
    })
}

/// The frames of one layer of composite `name`, e.g. one facing of the
/// unloading walkway `MOST0_`, whose eight layers are its eight facings.
pub fn composite_layer_frames(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
    layer: usize,
) -> Result<Vec<Vec<Layer>>> {
    let Some(comp) = caches.composite(data, name)? else {
        return Ok(Vec::new());
    };
    let mut frames = Vec::new();
    for i in 0..comp.frames {
        let mut layers = Vec::new();
        if let Some(cell) = comp.frame(i).get(layer) {
            if cell.sprite != EMPTY_SPRITE {
                if let Some(image) = caches.sprites.get(data, &cell.sprite)? {
                    layers.push(Layer {
                        image,
                        dx: cell.dx,
                        dy: cell.dy,
                    });
                }
            }
        }
        frames.push(layers);
    }
    Ok(frames)
}

fn composite_frames(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
    range: std::ops::Range<usize>,
) -> Result<Vec<Vec<Layer>>> {
    composite_frames_with(data, caches, name, range, |_, _| true)
}

/// A composite's frames, its layers drawn in their order (the wake, order
/// -21000, first; then from the highest order to the lowest, so a ship's
/// sails, 4, lie over its hull, 10 and 11), keeping the sprites `keep`
/// accepts.
fn composite_frames_with(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
    range: std::ops::Range<usize>,
    keep: impl Fn(usize, &str) -> bool,
) -> Result<Vec<Vec<Layer>>> {
    let Some(comp) = caches.composite(data, name)? else {
        return Ok(Vec::new());
    };
    let mut frames = Vec::new();
    for i in range.start..range.end.min(comp.frames) {
        let mut layers = Vec::new();
        let mut cells: Vec<_> = comp.frame(i).iter().enumerate().collect();
        cells.sort_by_key(|(_, c)| {
            if c.order < 0 {
                (0, c.order)
            } else {
                (1, -c.order)
            }
        });
        for (k, cell) in cells {
            if cell.sprite == EMPTY_SPRITE || !keep(k, &cell.sprite) {
                continue;
            }
            if let Some(image) = caches.sprites.get(data, &cell.sprite)? {
                layers.push(Layer {
                    image,
                    dx: cell.dx,
                    dy: cell.dy,
                });
            }
        }
        frames.push(layers);
    }
    Ok(frames)
}

pub fn sequence_frames(
    data: &GameData,
    caches: &mut Caches,
    name: &str,
) -> Result<Vec<Vec<Layer>>> {
    let names = caches.sequence(data, name)?;
    let mut frames = Vec::with_capacity(names.len());
    for n in names.iter() {
        if let Some(image) = caches.sprites.get(data, n)? {
            frames.push(vec![Layer {
                image,
                dx: 0,
                dy: 0,
            }]);
        }
    }
    Ok(frames)
}

fn settlement_class() -> u32 {
    crate::economy::SETTLEMENT
}
