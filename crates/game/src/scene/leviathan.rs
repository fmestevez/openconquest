//! The leviathan (WhaleClassTy, Grig\whale.cpp): under water it waits,
//! swims a cell or two within its bounds and shows itself; struck down to
//! no life it comes up to be hunted, and once caught it waits below a long
//! while and comes up whole elsewhere (0x494b90 a tick, states entered by
//! 0x494400, messages 0x494f90).

use openconquest_formats::map::TILE_SIZE;
use openconquest_formats::ObjectClass;

use super::{Caches, GameData, Scene};
use crate::combat::Rng;
use crate::objects;

/// `+0x74`: under water, waiting (0).
pub const UNDER: u8 = 0;
/// Showing itself, a loop of its picture (1).
pub const SHOWING: u8 = 1;
/// Coming up to be hunted (2), up (3, the only state a net takes), going
/// down again (4).
pub const RISING: u8 = 2;
pub const UP: u8 = 3;
pub const SINKING: u8 = 4;
/// Caught: waiting below (5), then looking for a cell to come up on (6).
pub const CAUGHT: u8 = 5;
pub const RESURFACING: u8 = 6;
pub const GONE: u8 = 0xff;

/// Its move weights over the 5 x 5 cells about it (`[dy * 5 + dx]`), by
/// record field 13 below 9 (0x5ba958, 9 x 25 dwords).
const WEIGHTS: [[u32; 25]; 9] = [
    [
        1, 1, 2, 5, 10, 1, 1, 2, 10, 15, 1, 1, 2, 20, 20, 1, 1, 2, 10, 15, 1, 1, 2, 5, 10,
    ],
    [
        2, 5, 10, 15, 20, 1, 2, 10, 20, 15, 1, 1, 2, 10, 10, 1, 1, 1, 2, 5, 1, 1, 1, 1, 2,
    ],
    [
        10, 15, 20, 15, 10, 5, 10, 20, 10, 5, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    ],
    [
        20, 15, 10, 5, 2, 15, 20, 10, 2, 1, 10, 10, 2, 1, 1, 5, 2, 1, 1, 1, 2, 1, 1, 1, 1,
    ],
    [
        10, 5, 2, 1, 1, 15, 10, 2, 1, 1, 20, 20, 2, 1, 1, 15, 10, 2, 1, 1, 10, 5, 2, 1, 1,
    ],
    [
        2, 1, 1, 1, 1, 5, 2, 1, 1, 1, 10, 10, 2, 1, 1, 15, 20, 10, 2, 1, 20, 15, 10, 5, 2,
    ],
    [
        1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 5, 10, 20, 10, 5, 10, 15, 20, 15, 10,
    ],
    [
        1, 1, 1, 1, 2, 1, 1, 1, 2, 5, 1, 1, 2, 10, 10, 1, 2, 10, 20, 15, 2, 5, 10, 15, 20,
    ],
    [1; 25],
];

/// Treasures it may carry: [0x5c112c] for the move's room, [0x5c1140]
/// for the taking (both 2).
const CARRIES: usize = 2;

/// A tornado's blow (0x43a6c0: message 0xda96 with 30000).
pub const TORNADO_BLOW: i32 = 30_000;

/// A leviathan's state (the object's fields of 0x494f90).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Leviathan {
    /// `+0x74`.
    pub state: u8,
    /// Life and full life (`+0x80`, `+0x7c`), the life it comes up with
    /// again (`+0x90`).
    pub life: i32,
    pub max_life: i32,
    pub refill_life: i32,
    /// Times it has come up again after a catch (`+0x94`) and may
    /// (`+0x84`, record field 39); field 40 (`+0x88`).
    pub shown: i32,
    pub shows: i32,
    pub f40: i32,
    /// Its cell (`+0xa9`, `+0xad`) and whether the cell's leviathan slot
    /// (`+0x10`) holds it.
    pub cell: (i32, i32),
    pub in_slot: bool,
    /// The cells it keeps to: x0, x1, y0, y1 (`+0xb1`..`+0xbd`).
    pub bounds: (i32, i32, i32, i32),
    /// Move weights (`+0x121`).
    pub weights: [u32; 25],
    /// Heading 0-7 (`+0x98`).
    pub heading: u8,
    /// The state's start and wait (`+0xf9`, `+0xfd`).
    pub since: u32,
    pub wait: u32,
    /// Its picture: shown (`+0x189`), the tick of its last frame (`+0xf1`),
    /// ticks a frame (`+0xf5`), the frame (`+0x45`), the loop's first and
    /// last (`+0x4d`, `+0x51`), forwards (`+0x185`), the frame on show.
    pub picture: bool,
    pub last: u32,
    pub delay: u32,
    pub frame: i32,
    pub first: i32,
    pub end: i32,
    pub forward: bool,
    pub drawn: i32,
    /// Loops done and wanted (`+0x111`, `+0x115`).
    pub loops: i32,
    pub wanted: i32,
    /// Struck down, it comes up to be hunted (`+0x119`).
    pub comes_up: bool,
    /// Struck while up: a frame every tick, no more loops (`+0x11d`).
    pub hurried: bool,
    /// Its own generator (`+0x18`).
    pub lcg: u32,
    /// The picture its object holds (not saved: loaded again).
    #[serde(skip)]
    pub art: Option<u8>,
    /// Its picture's masks (`+0x18d`, `+0x191`: `WHALEM_<row><heading>`,
    /// 0x493a20 mode 1), loaded with it.
    #[serde(skip)]
    pub masks: Masks,
}

/// A leviathan picture's hit masks (kind 18), a frame each.
#[derive(Clone, Default)]
pub struct Masks(pub(super) Option<std::rc::Rc<Vec<openconquest_formats::Mask>>>);

impl PartialEq for Masks {
    /// Loaded again with the picture: not part of its state.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Masks {}

impl std::fmt::Debug for Masks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Masks({})", self.0.as_ref().map_or(0, |m| m.len()))
    }
}

/// The masks of `WHALEM_<row><heading>` (kind 11: the masks' name prefix,
/// `WHALE_MSK_<heading>`, `WHALE_M1_MSK` or `WHALE_M2_MSK`, and their
/// count, the entry's width; each kind 18 named by the prefix and two
/// digits).
fn load_masks(data: &GameData, name: &str) -> Masks {
    use openconquest_formats::ResourceKind;
    let lib = &data.objects;
    let Some(e) = lib.find(name, ResourceKind::Other(11)) else {
        return Masks(None);
    };
    let Ok(raw) = lib.read_entry(e) else {
        return Masks(None);
    };
    let prefix: String = raw
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect();
    let mut masks = Vec::new();
    for k in 0..e.width {
        let Some(m) = lib.find(&format!("{prefix}{k:02}"), ResourceKind::Mask) else {
            break;
        };
        let Ok(bits) = lib.read_entry(m) else {
            break;
        };
        let Ok(mask) = openconquest_formats::Mask::parse(&bits, m.width.into(), m.height.into())
        else {
            break;
        };
        masks.push(mask);
    }
    Masks(Some(std::rc::Rc::new(masks)))
}

/// Where a shot that hits it is aimed while it shows itself (0x495b40
/// with an accuracy, state 1: 0x5bace0 by heading, from its cell's
/// corner).
const AIM: [(i32, i32); 8] = [
    (42, 52),
    (40, 23),
    (31, 15),
    (22, 20),
    (23, 52),
    (25, 30),
    (33, 43),
    (35, 29),
];

/// A shot's blow on it in percent of the shot's power, by its letter
/// (0x598100, read by 0x4967b0 and 0x498240).
const SHOT_SHARE: [i32; 12] = [60, 0, 60, 60, 60, 40, 40, 44, 45, 0, 0, 0];

impl Leviathan {
    fn draw(&mut self) -> u32 {
        let mut r = Rng(self.lcg);
        let v = r.next();
        self.lcg = r.0;
        v
    }

    fn inside(&self, (x, y): (i32, i32)) -> bool {
        (self.bounds.0..=self.bounds.1).contains(&x) && (self.bounds.2..=self.bounds.3).contains(&y)
    }
}

/// The heading from its cell to `to` (0x494140): east 0, counting
/// counter-clockwise; the same cell keeps the old one.
fn heading_to(from: (i32, i32), to: (i32, i32), old: u8) -> u8 {
    match ((to.0 - from.0).signum(), (to.1 - from.1).signum()) {
        (1, 0) => 0,
        (1, -1) => 1,
        (0, -1) => 2,
        (-1, -1) => 3,
        (-1, 0) => 4,
        (-1, 1) => 5,
        (0, 1) => 6,
        (1, 1) => 7,
        _ => old,
    }
}

/// The weighted pick (0x495e80): one choice needs no draw; else `r % (sum
/// + 1)`, the first whose running sum reaches it (0 if none).
fn pick(weights: &[u32], l: &mut Leviathan) -> usize {
    if weights.len() == 1 {
        return 0;
    }
    let total: u32 = weights.iter().sum();
    let r = l.draw() % (total + 1);
    let mut sum = 0;
    for (k, &w) in weights.iter().enumerate() {
        sum += w;
        if r <= sum {
            return k;
        }
    }
    0
}

/// The layers its showing picture keeps by its life (0x493ac0): the body
/// (layer 0) always, the wounds (1, 2, 3) below 76, 51 and 26 percent.
fn wound_mask(life: i32, max: i32) -> u32 {
    // (A full life of 0 would divide by 0 in the original; it never shows
    // itself without life, so the guard is never reached.)
    let pct = life * 100 / max.max(1);
    let mut mask = 1;
    if pct < 76 {
        mask |= 2;
    }
    if pct < 51 {
        mask |= 4;
    }
    if pct < 26 {
        mask |= 8;
    }
    mask
}

impl Scene {
    fn cell_centre((x, y): (i32, i32)) -> (f32, f32) {
        let t = TILE_SIZE as f32;
        (x as f32 * t + t / 2.0, y as f32 * t + t / 2.0)
    }

    /// The leviathan in the slot of cell `c`, if any.
    pub(super) fn leviathan_in(&self, c: (i32, i32)) -> Option<u32> {
        self.leviathans
            .iter()
            .find(|(_, l)| l.in_slot && l.cell == c)
            .map(|(&id, _)| id)
    }

    /// Leviathan `id`'s cell while the cell's slot holds it (states 0-4).
    pub fn leviathan_cell(&self, id: u32) -> Option<(i32, i32)> {
        self.leviathans
            .get(&id)
            .filter(|l| l.in_slot)
            .map(|l| l.cell)
    }

    /// Whether leviathan `who` (or none) may be on cell `c` (0x48ed40):
    /// landscape low nibble up to 7, nibble 1 up to 3; with `free` no ship
    /// in the cell's occupant slot (0x499900), nothing on its `+0x1c` but,
    /// with `room`, a treasure (0x50e600's family 700), and no sea monster
    /// with a picture on its `+0x18` (0x403010, vt +0x30 0x493710; not
    /// ported: the gorgon's and siren's vt +0x30 there); no other leviathan
    /// in its slot.
    fn leviathan_may_be(&self, who: Option<u32>, c: (i32, i32), free: bool, room: bool) -> bool {
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        if c.0 < 0 || c.1 < 0 || c.0 >= w || c.1 >= h {
            return false;
        }
        let types = self.cell_types[(c.1 * w + c.0) as usize];
        if types & 0xf > 7 || types & 0xf0 > 0x30 {
            return false;
        }
        if free && self.hazard_occupant(c).is_some() {
            return false;
        }
        if free {
            if let Some(k) = self.third_place_object(c) {
                if !room || self.objects[k].class != ObjectClass::Treasure {
                    return false;
                }
            }
        }
        if free {
            let t = TILE_SIZE as f32;
            let shown = self.objects.iter().any(|o| {
                o.class == ObjectClass::Monster(0x1fb)
                    && ((o.x / t).floor() as i32, (o.y / t).floor() as i32) == c
                    && !self.sea_monster_hidden(o.id)
            });
            if shown {
                return false;
            }
        }
        self.leviathan_in(c).is_none_or(|id| Some(id) == who)
    }

    /// Message 2 (mode 0, a new one): amount field 4 x 100 (the object's
    /// deposit), full life field 5, life field 6; its cell fields 7, 8 and
    /// its bounds that cell -/+ fields 10, 11 clamped to the map (either
    /// below 1: the whole map); field 39 the times it comes up again, 40
    /// how soon; its weights from table 0x5ba958 by field 13 (below 9) or
    /// fields 14-38 (each at least 1). Its generator is seeded `game
    /// +0x2ee5 x id`. A cell 0x48ed40 refuses: gone (message 10).
    fn new_leviathan(&mut self, i: usize, data: &GameData, caches: &mut Caches) -> Leviathan {
        let fields = self.objects[i].record.fields.clone();
        let f = |k: usize| fields.get(k).copied().unwrap_or(0) as i32;
        let (w, h) = (self.nav.width as i32, self.nav.height as i32);
        let cell = (f(7), f(8));
        let bounds = if f(10) < 1 || f(11) < 1 {
            (0, w - 1, 0, h - 1)
        } else {
            (
                (cell.0 - f(10)).max(0),
                (cell.0 + f(10)).min(w - 1),
                (cell.1 - f(11)).max(0),
                (cell.1 + f(11)).min(h - 1),
            )
        };
        let weights = if (0..9).contains(&f(13)) {
            WEIGHTS[f(13) as usize]
        } else {
            let mut ws = [1u32; 25];
            for (k, wt) in ws.iter_mut().enumerate() {
                *wt = f(14 + k).max(1) as u32;
            }
            ws
        };
        let id = self.objects[i].id;
        let lcg = self.game_seed().wrapping_mul(id);
        let life = ((f(6) as u32).wrapping_mul(100) / 100) as i32;
        let mut l = Leviathan {
            state: UNDER,
            life,
            max_life: f(5),
            refill_life: life,
            shown: 0,
            shows: f(39),
            f40: f(40),
            cell,
            in_slot: false,
            bounds,
            weights,
            heading: 0,
            since: self.ticks,
            wait: 0,
            picture: false,
            last: 0,
            delay: 0,
            frame: 0,
            first: 0,
            end: 0,
            forward: true,
            drawn: 0,
            loops: 0,
            wanted: 0,
            comes_up: false,
            hurried: false,
            lcg,
            art: None,
            masks: Masks::default(),
        };
        let (x, y) = Self::cell_centre(cell);
        self.objects[i].x = x;
        self.objects[i].y = y;
        if self.leviathan_may_be(Some(id), cell, false, false) {
            l.in_slot = true;
            self.set_leviathan_state(i, &mut l, UNDER, data, caches);
        } else {
            l.state = GONE;
        }
        l
    }

    /// 0x494400: to state `s`.
    fn set_leviathan_state(
        &mut self,
        i: usize,
        l: &mut Leviathan,
        s: u8,
        data: &GameData,
        caches: &mut Caches,
    ) {
        l.loops = 0;
        l.since = self.ticks;
        let (px, py) = Self::cell_centre(l.cell);
        match s {
            UNDER => {
                l.state = UNDER;
                l.picture = false;
                l.hurried = false;
                l.wait = l.draw() % 51 + 25;
            }
            SHOWING => {
                l.state = SHOWING;
                self.leviathan_picture(i, l, data, caches);
                l.wanted = 1;
                // Sound event 99 (`kit4`) with an odd heading.
                if l.heading % 2 == 1 {
                    self.sounds.push(("kit4", px, py));
                }
                // With room it takes the treasure on its cell's +0x1c
                // (0x158c1 to it, its five dwords kept). (Not ported, as
                // for the sea monster: the treasure's state 5, refused.)
                let id = self.objects[i].id;
                if self.held.iter().filter(|h| h.0 == id).count() < CARRIES {
                    if let Some(g) = self
                        .third_place_object(l.cell)
                        .filter(|&g| self.objects[g].class == ObjectClass::Treasure)
                    {
                        if let Some(tr) = self.objects[g].treasure {
                            self.objects.remove(g);
                            self.held.push((id, tr));
                        }
                    }
                }
            }
            RISING => {
                l.state = RISING;
                l.heading = 0;
                self.leviathan_picture(i, l, data, caches);
                l.wanted = 1;
                // Sound event 110 (`prote1`).
                self.sounds.push(("prote1", px, py));
            }
            UP if !l.hurried => {
                l.state = UP;
                l.heading = 0;
                l.wanted = (l.draw() % 41 + 40) as i32;
                self.leviathan_picture(i, l, data, caches);
                self.leviathan_step(l);
                // What it carried falls at its cell (0x520c70 each).
                // (Not decoded: whether 0x16e3 shows a message; none here.)
                let id = self.objects[i].id;
                let (kept, dropped): (Vec<_>, Vec<_>) = std::mem::take(&mut self.held)
                    .into_iter()
                    .partition(|h| h.0 != id);
                self.held = kept;
                for (_, tr) in dropped {
                    self.place_treasure(tr, px, py, data, caches);
                }
            }
            // Struck while coming up: no loops up, down at once (the
            // switch taken again as 4, 0x4944ba).
            UP | SINKING => {
                l.state = SINKING;
                l.heading = 0;
                self.leviathan_picture(i, l, data, caches);
                self.leviathan_step(l);
                l.wanted = 1;
            }
            CAUGHT => {
                l.state = CAUGHT;
                l.hurried = false;
                l.f40 = l.f40.clamp(0, 9);
                l.wait = (27_750 - 3000 * l.f40) as u32;
            }
            _ => {
                l.state = s;
                l.hurried = false;
            }
        }
    }

    /// 0x493b00(-1): the state's picture from its first frame (forwards
    /// but going down), drawn at the next step: a frame every 2 ticks (1
    /// showing with an odd heading, 3 up).
    fn leviathan_picture(
        &mut self,
        i: usize,
        l: &mut Leviathan,
        data: &GameData,
        caches: &mut Caches,
    ) {
        l.picture = true;
        l.forward = l.state != SINKING;
        l.delay = match l.state {
            SHOWING => 2 - (l.heading % 2) as u32,
            UP => 3,
            _ => 2,
        };
        l.last = 0;
        let frames = Self::leviathan_frames(l, data, caches);
        let n = frames.len() as i32;
        self.objects[i].frames = frames;
        l.art = Some(l.state);
        l.masks = load_masks(data, &Self::mask_name(l));
        let last = (n - 1).max(0);
        let zero = if n < 1 { n - 1 } else { 0 };
        (l.first, l.end) = if l.forward {
            (zero, last)
        } else {
            (last, zero)
        };
        l.frame = l.first;
        l.drawn = l.frame;
    }

    /// The name and layers of its picture (0x493a20, 0x493ac0):
    /// `WHALE_0<heading>` showing, a composite with its wounds;
    /// `WHALE_10` coming up and going down, `WHALE_20` up.
    fn leviathan_frames(l: &Leviathan, data: &GameData, caches: &mut Caches) -> super::Frames {
        let row = match l.state {
            SHOWING => 0,
            RISING | SINKING => 1,
            UP => 2,
            _ => 9,
        };
        let name = format!("WHALE_{row}{}", l.heading);
        let mut frames = if l.state == SHOWING {
            objects::composite_masked_frames(data, caches, &name, wound_mask(l.life, l.max_life))
        } else {
            objects::sequence_frames(data, caches, &name)
        }
        .unwrap_or_default();
        // At its cell's corner (`+0x59`, `+0x5d`), 5 lower but showing.
        let down = if l.state == SHOWING { 0 } else { 5 };
        let t = TILE_SIZE as i32;
        let size = frames
            .first()
            .and_then(|f| f.first())
            .map_or((t, t), |l| (l.image.width as i32, l.image.height as i32));
        for f in &mut frames {
            for layer in f {
                layer.dx += size.0 / 2 - t / 2;
                layer.dy += size.1 / 2 - t / 2 + down;
            }
        }
        frames
    }

    /// Its picture loaded again into object `i` after a restore.
    fn leviathan_art(&mut self, i: usize, l: &mut Leviathan, data: &GameData, caches: &mut Caches) {
        if l.picture && l.art != Some(l.state) {
            self.objects[i].frames = Self::leviathan_frames(l, data, caches);
            l.art = Some(l.state);
            l.masks = load_masks(data, &Self::mask_name(l));
        }
    }

    /// The name of its masks (0x493a20 mode 1): `WHALEM_<row><heading>`,
    /// the row as its picture's.
    fn mask_name(l: &Leviathan) -> String {
        let row = match l.state {
            SHOWING => 0,
            RISING | SINKING => 1,
            UP => 2,
            _ => 9,
        };
        format!("WHALEM_{row}{}", l.heading)
    }

    /// 0x493f10: a frame when its time has come (every tick while
    /// hurried); past the loop's end back to its first, a loop more.
    fn leviathan_step(&mut self, l: &mut Leviathan) {
        let delay = if l.hurried { 0 } else { l.delay };
        if self.ticks < l.last + delay {
            return;
        }
        l.last = self.ticks;
        l.drawn = l.frame;
        if l.forward {
            l.frame += 1;
            if l.frame <= l.end {
                return;
            }
        } else {
            l.frame -= 1;
            if l.frame >= l.end {
                return;
            }
        }
        l.frame = l.first;
        l.loops += 1;
    }

    /// 0x4941f0: a cell of the 5 x 5 about it, within its bounds, that
    /// 0x48ed40 lets it in (with room for a treasure while it carries
    /// fewer than two), weighted, ten times over onto a treasure. False if
    /// none.
    fn leviathan_move(&mut self, i: usize, l: &mut Leviathan) -> bool {
        let id = self.objects[i].id;
        let room = self.held.iter().filter(|h| h.0 == id).count() < CARRIES;
        let mut cells = Vec::new();
        let mut weights = Vec::new();
        for dx in 0..5 {
            for dy in 0..5 {
                let c = (l.cell.0 - 2 + dx, l.cell.1 - 2 + dy);
                if !l.inside(c) || !self.leviathan_may_be(Some(id), c, true, room) {
                    continue;
                }
                let w = l.weights[(dy * 5 + dx) as usize];
                let treasure = self
                    .third_place_object(c)
                    .is_some_and(|k| self.objects[k].class == ObjectClass::Treasure);
                cells.push(c);
                weights.push(if treasure { w * 10 } else { w });
            }
        }
        if cells.is_empty() {
            return false;
        }
        let to = cells[pick(&weights, l)];
        l.heading = heading_to(l.cell, to, l.heading);
        l.cell = to;
        let (x, y) = Self::cell_centre(to);
        self.objects[i].x = x;
        self.objects[i].y = y;
        true
    }

    fn leviathan_amount(&self, i: usize) -> f32 {
        self.objects[i]
            .deposit
            .and_then(|d| d.remaining)
            .unwrap_or(0.0)
    }

    fn set_leviathan_amount(&mut self, i: usize, v: f32) {
        if let Some(d) = self.objects[i].deposit.as_mut() {
            d.remaining = Some(v);
        }
    }

    /// One tick (0x494b90).
    fn leviathan_tick(
        &mut self,
        i: usize,
        l: &mut Leviathan,
        data: &GameData,
        caches: &mut Caches,
    ) {
        let now = self.ticks;
        match l.state {
            UNDER => {
                if now < l.since + l.wait {
                    return;
                }
                if l.life > 0 {
                    if self.leviathan_move(i, l) {
                        self.set_leviathan_state(i, l, SHOWING, data, caches);
                    } else {
                        self.set_leviathan_state(i, l, UNDER, data, caches);
                    }
                } else if l.comes_up {
                    if self.leviathan_move(i, l) {
                        self.set_leviathan_state(i, l, RISING, data, caches);
                    } else {
                        self.set_leviathan_state(i, l, UNDER, data, caches);
                    }
                } else {
                    // Off its cell's slot (not decoded: 0x523d80(200,
                    // +0xa1), `+0xa1` never written by whale.cpp); gone
                    // once it has come up again field 39 times.
                    l.in_slot = false;
                    if l.shown >= l.shows {
                        self.set_leviathan_state(i, l, GONE, data, caches);
                    } else {
                        self.set_leviathan_state(i, l, CAUGHT, data, caches);
                    }
                }
            }
            SHOWING | SINKING => {
                if l.loops < l.wanted {
                    self.leviathan_step(l);
                } else {
                    self.set_leviathan_state(i, l, UNDER, data, caches);
                }
            }
            RISING => {
                if l.loops < l.wanted {
                    self.leviathan_step(l);
                } else {
                    self.set_leviathan_state(i, l, UP, data, caches);
                }
            }
            UP => {
                if l.wanted <= l.loops || !l.comes_up {
                    l.comes_up = false;
                    self.set_leviathan_state(i, l, SINKING, data, caches);
                    return;
                }
                self.leviathan_step(l);
                // Hunted out: down at the end of this loop.
                if self.leviathan_amount(i) < 1.0 {
                    l.comes_up = false;
                    l.wanted = l.loops + 1;
                }
            }
            CAUGHT => {
                if l.since + l.wait <= now {
                    self.set_leviathan_state(i, l, RESURFACING, data, caches);
                }
            }
            RESURFACING => {
                if l.loops >= 100 {
                    self.set_leviathan_state(i, l, CAUGHT, data, caches);
                    return;
                }
                // One draw of x and one of y in its bounds a tick; its cell
                // taken before the test.
                l.loops += 1;
                let x = (l.draw() % (l.bounds.1 - l.bounds.0 + 1) as u32) as i32 + l.bounds.0;
                let y = (l.draw() % (l.bounds.3 - l.bounds.2 + 1) as u32) as i32 + l.bounds.2;
                l.cell = (x, y);
                let (px, py) = Self::cell_centre(l.cell);
                self.objects[i].x = px;
                self.objects[i].y = py;
                if !self.leviathan_may_be(None, l.cell, false, false) {
                    return;
                }
                let refill = self.objects[i].deposit.map_or(0.0, |d| d.refill);
                self.set_leviathan_amount(i, refill);
                l.life = l.refill_life;
                l.comes_up = true;
                l.shown += 1;
                l.in_slot = true;
                self.set_leviathan_state(i, l, UNDER, data, caches);
            }
            _ => {}
        }
    }

    /// The leviathans' tick (message 0 of 0x494f90), with their creation
    /// (message 2) for those not yet seen; before it, the reefs and
    /// whirlpools not under water strike the one on their cell (0xda94 /
    /// 0xda95, sent from their own ticks; not settled: the order of the
    /// two within a tick).
    pub(super) fn run_leviathans(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self
            .objects
            .iter()
            .filter(|o| o.class == ObjectClass::WhaleZone)
            .map(|o| o.id)
            .collect();
        self.leviathans.retain(|id, _| ids.contains(id));
        let traps: Vec<(i32, i32)> = self
            .objects
            .iter()
            .filter(|o| matches!(o.class, ObjectClass::Whirlpool | ObjectClass::Reef(_)))
            .filter(|o| self.hazard_wrecks(o.id))
            .map(|o| {
                let f = |i| o.record.field(i).unwrap_or(0) as i32;
                (f(5), f(6))
            })
            .collect();
        for c in traps {
            if let Some(id) = self.leviathan_in(c) {
                self.leviathan_wrecked(id);
            }
        }
        for id in ids {
            let Some(i) = self.object_index(id) else {
                continue;
            };
            let mut l = match self.leviathans.remove(&id) {
                Some(l) => l,
                None => self.new_leviathan(i, data, caches),
            };
            if l.state != GONE {
                self.leviathan_tick(i, &mut l, data, caches);
            }
            let Some(i) = self.object_index(id) else {
                continue;
            };
            if l.state == GONE {
                // Message 10: it goes (what it carries with it).
                self.held.retain(|h| h.0 != id);
                self.objects.remove(i);
                continue;
            }
            self.leviathan_art(i, &mut l, data, caches);
            self.leviathans.insert(id, l);
        }
    }

    /// Lightning on the 3 x 3 about `centre` (0xda93), unless caught (5,
    /// 6): its life to 0, or with none its amount; on its own cell it goes
    /// down (0x494400(0)), elsewhere with a picture it hurries.
    pub(super) fn leviathan_lightning(
        &mut self,
        id: u32,
        centre: (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let Some(mut l) = self.leviathans.remove(&id) else {
            return;
        };
        if l.state != CAUGHT && l.state != RESURFACING {
            if l.life < 1 {
                self.set_leviathan_amount(i, 0.0);
            } else {
                l.life = 0;
            }
            if centre == l.cell {
                self.set_leviathan_state(i, &mut l, UNDER, data, caches);
            } else if l.picture {
                l.hurried = true;
            }
        }
        self.leviathans.insert(id, l);
    }

    /// A reef's or whirlpool's wreck on its cell (0xda94 / 0xda95): with a
    /// picture it hurries; with no life its amount goes.
    pub(super) fn leviathan_wrecked(&mut self, id: u32) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let Some(l) = self.leviathans.get(&id) else {
            return;
        };
        if !l.picture {
            return;
        }
        if l.life < 1 {
            self.set_leviathan_amount(i, 0.0);
        }
        self.leviathans.get_mut(&id).unwrap().hurried = true;
    }

    /// A tornado's blow on cell `c` (0xda96) to the leviathan in its slot:
    /// with a picture its life less the blow, or with none its amount
    /// gone; it hurries.
    pub(super) fn leviathan_blow(&mut self, c: (i32, i32), blow: i32) {
        let Some(id) = self.leviathan_in(c) else {
            return;
        };
        let Some(i) = self.object_index(id) else {
            return;
        };
        let l = self.leviathans.get_mut(&id).unwrap();
        if !l.picture {
            return;
        }
        l.hurried = true;
        if l.life > 0 {
            l.life -= blow;
        } else {
            self.set_leviathan_amount(i, 0.0);
        }
    }

    /// Whether leviathan `id` shows no picture: under water, caught, not
    /// yet created.
    pub fn leviathan_hidden(&self, id: u32) -> bool {
        self.leviathans.get(&id).is_none_or(|l| !l.picture)
    }

    /// The frame leviathan `id` shows.
    pub(super) fn leviathan_frame(&self, id: u32) -> Option<u32> {
        self.leviathans
            .get(&id)
            .filter(|l| l.picture)
            .map(|l| l.drawn.max(0) as u32)
    }

    /// Leviathan `id`'s state (`+0x74`), once created.
    pub fn leviathan_state(&self, id: u32) -> Option<u8> {
        self.leviathans.get(&id).map(|l| l.state)
    }

    /// Leviathan `id`'s cell as its vt+0x28 gives it (0x495ff0): in states
    /// 0 to 4; none caught, coming up again elsewhere or gone.
    pub(super) fn leviathan_where(&self, id: u32) -> Option<(i32, i32)> {
        self.leviathans
            .get(&id)
            .filter(|l| !matches!(l.state, CAUGHT | RESURFACING | GONE))
            .map(|l| l.cell)
    }

    /// The leviathan in the slot of cell `c` with its picture shown, if
    /// any: what a hunter's look over the cells about it sees (0x4ac8d0:
    /// the slot `+0x10`, `+0x189`).
    pub(super) fn leviathan_seen(&self, c: (i32, i32)) -> Option<u32> {
        self.leviathan_in(c)
            .filter(|id| self.leviathans.get(id).is_some_and(|l| l.picture))
    }

    /// The leviathan in the slot of cell `c` up (state 3), if any.
    pub(super) fn leviathan_up_in(&self, c: (i32, i32)) -> Option<u32> {
        self.leviathan_in(c)
            .filter(|id| self.leviathans.get(id).is_some_and(|l| l.state == UP))
    }

    /// Whether a leviathan lies in the slot of cell `c` (`+0x10`).
    pub(super) fn leviathan_slot(&self, c: (i32, i32)) -> bool {
        self.leviathan_in(c).is_some()
    }

    /// Its amount (`+0x78`).
    pub(super) fn leviathan_left(&self, id: u32) -> i32 {
        self.object_index(id)
            .map_or(0, |i| self.leviathan_amount(i) as i32)
    }

    /// vt+4 mode 1 (0x494090): a net takes `n` of its amount, only while
    /// it is up (state 3).
    pub(super) fn leviathan_netted(&mut self, id: u32, n: i32) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        if self.leviathans.get(&id).is_some_and(|l| l.state == UP) {
            let left = self.leviathan_amount(i) - n as f32;
            self.set_leviathan_amount(i, left);
        }
    }

    /// The point a shot that hits it is aimed at (0x495b40 with an
    /// accuracy): showing itself, its heading's point of 0x5bace0 from
    /// its cell's corner. (Ships shoot at it only then, 0x4af6a0.)
    pub(super) fn leviathan_aim(&self, id: u32) -> Option<(i32, i32)> {
        let l = self.leviathans.get(&id).filter(|l| l.state == SHOWING)?;
        let t = TILE_SIZE as i32;
        let (ax, ay) = AIM[l.heading as usize % 8];
        Some((l.cell.0 * t + ax, l.cell.1 * t + ay))
    }

    /// Its hit test at the world point `(x, y)` (vt+8, 0x495a60): its
    /// picture shown and the point in its rect (the cell's corner, 5
    /// lower but showing itself; the picture's size), then the bit of its
    /// mask for the frame (`+0x45`, the last mask past their count) at
    /// that point, rows `(width + 7) & !7` bits long, the first bit the
    /// highest (0x557640).
    pub fn leviathan_hit(&self, id: u32, (x, y): (i32, i32)) -> bool {
        let Some(l) = self.leviathans.get(&id).filter(|l| l.picture) else {
            return false;
        };
        let Some(i) = self.object_index(id) else {
            return false;
        };
        let t = TILE_SIZE as i32;
        let (w, h) = self.objects[i]
            .frames
            .first()
            .and_then(|f| f.first())
            .map_or((t, t), |l| (l.image.width as i32, l.image.height as i32));
        let left = l.cell.0 * t;
        let top = l.cell.1 * t + if l.state == SHOWING { 0 } else { 5 };
        if x < left || x > left + w - 1 || y < top || y > top + h - 1 {
            return false;
        }
        let Some(masks) = l.masks.0.as_ref().filter(|m| !m.is_empty()) else {
            return false;
        };
        let k = (l.frame.max(0) as usize).min(masks.len() - 1);
        let bit = ((w + 7) & !7) as usize * (y - top) as usize + (x - left) as usize;
        masks[k].bit(bit)
    }

    /// A shot's blow landing on it (0x4967b0 / 0x498240, then vt+4 mode 0,
    /// 0x494090): `(0x598100[letter] x power / 100) x 100 / 10000` off its
    /// life while it shows itself (with sound event 121, which lists no
    /// sound); with no life left, its death put down to the shot's player
    /// (0x50e750), on every such blow.
    pub(super) fn leviathan_shot(
        &mut self,
        id: u32,
        letter: u8,
        power: i32,
        by: Option<(u32, u32)>,
    ) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let Some(l) = self.leviathans.get_mut(&id) else {
            return;
        };
        let share = SHOT_SHARE.get(letter as usize).copied().unwrap_or(0);
        let blow = share * power / 100 * 100 / 10000;
        if l.state == SHOWING {
            l.life -= blow;
        }
        if l.life < 1 {
            self.note_destroyed(i, by);
        }
    }

    /// Leviathan `o`'s life (`+0x80`): the record's field 6 until created.
    pub fn leviathan_life(&self, o: &objects::MapObject) -> i32 {
        self.leviathans
            .get(&o.id)
            .map_or(o.record.field(6).unwrap_or(0) as i32, |l| l.life)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// miss005's leviathan (id 13): amount 1, life 100 of 100, its cell
    /// (33, 1), bounds -/+ 5, table 8, field 39 2, field 40 2.
    fn miss005() -> Option<(GameData, Caches, Scene, u32)> {
        let root = std::env::var_os("OPENCONQUEST_DATA")?;
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss005").unwrap();
        scene.brain = None;
        Some((data, caches, scene, 13))
    }

    fn tick(scene: &mut Scene, data: &GameData, caches: &mut Caches) {
        scene.ticks += 1;
        scene.run_leviathans(data, caches);
    }

    /// Message 2: its start cell is fields 7, 8 and its bounds that cell
    /// -/+ fields 10, 11, clamped to the map; it starts below, waiting
    /// 25-75 ticks, its weights table 8's (all 1).
    #[test]
    fn leviathan_starts_at_its_cell() {
        let Some((data, mut caches, mut scene, id)) = miss005() else {
            return;
        };
        tick(&mut scene, &data, &mut caches);
        let l = scene.leviathans[&id].clone();
        assert_eq!(l.cell, (33, 1));
        assert!(l.in_slot);
        // (The map is 36 cells wide.)
        assert_eq!(l.bounds, (28, 35, 0, 6));
        assert_eq!(l.weights, [1; 25]);
        assert_eq!((l.life, l.max_life, l.shows, l.f40), (100, 100, 2, 2));
        assert_eq!(l.state, UNDER);
        assert!((25..=75).contains(&l.wait));
        assert!(scene.leviathan_hidden(id));
        let i = scene.object_index(id).unwrap();
        assert_eq!(scene.leviathan_amount(i), 100.0);
        assert_eq!(scene.leviathan_cell(id), Some((33, 1)));
    }

    /// With life it swims to a cell of the 5 x 5 about it within its
    /// bounds and shows itself (`WHALE_0<heading>`, a loop: a frame every 2
    /// ticks, 1 with an odd heading; `kit4` with an odd heading), then goes
    /// below again. It is never up to be netted.
    #[test]
    fn leviathan_with_life_only_shows_itself() {
        let Some((data, mut caches, mut scene, id)) = miss005() else {
            return;
        };
        let mut shown = 0;
        let mut cell = (33, 1);
        for _ in 0..3000 {
            scene.sounds.clear();
            let before = scene.leviathans.get(&id).map(|l| l.state);
            tick(&mut scene, &data, &mut caches);
            let l = scene.leviathans[&id].clone();
            assert!(l.state == UNDER || l.state == SHOWING, "{}", l.state);
            if before == Some(UNDER) && l.state == SHOWING {
                shown += 1;
                assert!((l.cell.0 - cell.0).abs() <= 2 && (l.cell.1 - cell.1).abs() <= 2);
                assert!(l.inside(l.cell));
                cell = l.cell;
                assert_eq!(l.delay, 2 - (l.heading % 2) as u32);
                assert_eq!(
                    scene.sounds.iter().any(|s| s.0 == "kit4"),
                    l.heading % 2 == 1
                );
                assert!(!scene.leviathan_hidden(id));
                let i = scene.object_index(id).unwrap();
                // The whole body, no wounds: one layer a frame.
                assert!(scene.objects[i].frames.iter().all(|f| f.len() == 1));
                assert_eq!(l.end + 1, scene.objects[i].frames.len() as i32);
            }
        }
        assert!(shown > 5);
    }

    /// Lightning on its cell takes its life and sends it down; with no
    /// life and not struck down before (no `+0x119`) it leaves its slot and
    /// waits 27750 - 3000 x field 40 ticks (state 5), then comes up whole
    /// on a cell of its bounds (state 6), shown once.
    #[test]
    fn leviathan_struck_waits_and_comes_up_whole() {
        let Some((data, mut caches, mut scene, id)) = miss005() else {
            return;
        };
        tick(&mut scene, &data, &mut caches);
        scene.leviathan_lightning(id, (33, 1), &data, &mut caches);
        let l = scene.leviathans[&id].clone();
        assert_eq!((l.life, l.state), (0, UNDER));
        let start = l.since + l.wait;
        while scene.ticks < start {
            tick(&mut scene, &data, &mut caches);
        }
        let l = scene.leviathans[&id].clone();
        assert_eq!(l.state, CAUGHT);
        assert!(!l.in_slot);
        assert_eq!(l.wait, 27_750 - 3000 * 2);
        assert_eq!(scene.leviathan_cell(id), None);
        let i = scene.object_index(id).unwrap();
        scene.set_leviathan_amount(i, 0.0);
        while scene.leviathans[&id].state == CAUGHT {
            tick(&mut scene, &data, &mut caches);
        }
        while scene.leviathans[&id].state == RESURFACING {
            tick(&mut scene, &data, &mut caches);
        }
        let l = scene.leviathans[&id].clone();
        assert_eq!(l.state, UNDER);
        assert!(l.in_slot && l.comes_up && l.inside(l.cell));
        assert_eq!((l.life, l.shown), (100, 1));
        let i = scene.object_index(id).unwrap();
        assert_eq!(scene.leviathan_amount(i), 100.0);
    }

    /// Struck down to no life after coming up again (`+0x119`), it comes
    /// up to be hunted: 2 (`prote1`), 3 for 40-80 loops of `WHALE_20` (a
    /// frame every 3 ticks), only then netted, by a ship of type 0 or 1 on
    /// its cell; hunted out it goes down at the end of the loop.
    #[test]
    fn leviathan_is_netted_only_up() {
        let Some((data, mut caches, mut scene, id)) = miss005() else {
            return;
        };
        tick(&mut scene, &data, &mut caches);
        {
            let l = scene.leviathans.get_mut(&id).unwrap();
            l.life = 0;
            l.comes_up = true;
            l.wait = 0;
        }
        let ship = scene.objects.iter().position(|o| o.ship.is_some()).unwrap();
        let id_ship = scene.objects[ship].id;
        let mut rose = false;
        let mut up = false;
        for _ in 0..200 {
            scene.sounds.clear();
            tick(&mut scene, &data, &mut caches);
            let l = scene.leviathans[&id].clone();
            if l.state == RISING && !rose {
                rose = true;
                assert!(scene.sounds.iter().any(|s| s.0 == "prote1"));
                assert_eq!(l.heading, 0);
            }
            if l.state == UP {
                up = true;
                assert!((40..=80).contains(&l.wanted));
                assert_eq!(l.delay, 3);
                break;
            }
        }
        assert!(rose && up);
        // Up in its cell's slot: what a net takes (0x4aee40).
        let cell = scene.leviathans[&id].cell;
        assert_eq!(scene.leviathan_up_in(cell), Some(id));
        let _ = id_ship;
        // Hunted out: down at the end of this loop, then below.
        let i = scene.object_index(id).unwrap();
        scene.set_leviathan_amount(i, 0.0);
        tick(&mut scene, &data, &mut caches);
        let l = scene.leviathans[&id].clone();
        assert!(!l.comes_up);
        assert_eq!(l.wanted, l.loops + 1);
        for _ in 0..400 {
            tick(&mut scene, &data, &mut caches);
            if scene.leviathans[&id].state != UP {
                break;
            }
        }
        assert_eq!(scene.leviathans[&id].state, SINKING);
        assert_eq!(scene.leviathan_up_in(cell), None);
    }

    /// A tornado's blow on its cell while it shows a picture takes its
    /// life (30000) and hurries it; below it is not struck.
    #[test]
    fn leviathan_tornado_blow() {
        let Some((data, mut caches, mut scene, id)) = miss005() else {
            return;
        };
        tick(&mut scene, &data, &mut caches);
        scene.leviathan_blow((33, 1), TORNADO_BLOW);
        assert_eq!(scene.leviathans[&id].life, 100);
        while scene.leviathans[&id].state != SHOWING {
            tick(&mut scene, &data, &mut caches);
        }
        let cell = scene.leviathans[&id].cell;
        scene.leviathan_blow(cell, TORNADO_BLOW);
        let l = &scene.leviathans[&id];
        assert_eq!(l.life, 100 - 30_000);
        assert!(l.hurried);
    }

    /// 0x495e80, 0x493ac0, 0x494140.
    #[test]
    fn leviathan_pick_mask_heading() {
        let mut l = blank();
        l.lcg = 1;
        assert_eq!(pick(&[5], &mut l), 0);
        assert_eq!(l.lcg, 1);
        // r = draw % 4 for weights 1, 2: 0, 1 -> 0; 2, 3 -> 1.
        let r = Rng(1).next() % 4;
        assert_eq!(pick(&[1, 2], &mut l), if r <= 1 { 0 } else { 1 });
        assert_eq!(wound_mask(100, 100), 1);
        assert_eq!(wound_mask(75, 100), 3);
        assert_eq!(wound_mask(50, 100), 7);
        assert_eq!(wound_mask(25, 100), 15);
        assert_eq!(heading_to((5, 5), (5, 5), 3), 3);
        assert_eq!(heading_to((5, 5), (6, 4), 0), 1);
        assert_eq!(heading_to((5, 5), (4, 6), 0), 5);
    }

    fn blank() -> Leviathan {
        Leviathan {
            state: UNDER,
            life: 0,
            max_life: 0,
            refill_life: 0,
            shown: 0,
            shows: 0,
            f40: 0,
            cell: (0, 0),
            in_slot: false,
            bounds: (0, 0, 0, 0),
            weights: [1; 25],
            heading: 0,
            since: 0,
            wait: 0,
            picture: false,
            last: 0,
            delay: 0,
            frame: 0,
            first: 0,
            end: 0,
            forward: true,
            drawn: 0,
            loops: 0,
            wanted: 0,
            comes_up: false,
            hurried: false,
            lcg: 0,
            art: None,
            masks: Masks::default(),
        }
    }
}
