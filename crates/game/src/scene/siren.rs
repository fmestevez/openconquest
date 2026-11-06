//! The siren (SirenaClassTy, sirena.cpp, class 0x1fd): she hides under
//! the water (state 0, `r % 751 + 250` ticks), hops to a cell of the 3 x 3
//! about her (0x403520), surfaces (1, `SIRENA` 0-10), sings once (2,
//! 11-100, at frame 45) and dives (3, 101-113); struck to nothing she dies
//! (4, 114-162, laying what she carried) and goes at the end of her next
//! rest (5). Her tick (message 0, 0x403ba0) runs the state's code, which
//! steps her picture (0x403360); a state is entered by 0x403650; her own
//! generator (`+0x18`) is seeded `[game +0x2ee5] x id` as the Gorgon's.

use openconquest_formats::map::TILE_SIZE;

use super::gorgon::{lcg, pick};
use super::{Caches, GameData, Owner, Scene};

/// Her record class.
pub const SIREN: u32 = 0x1fd;
/// Her life (`+0xb8`, the constructor 0x403050).
pub const SIREN_HP: i32 = 100_000;
/// Treasures she carries at most (`[0x5c113a]`).
const CARRIES: usize = 2;
/// The frame of `SIRENA` whose drawing sings (0x403ba0 state 2).
const SONG_FRAME: i32 = 45;
/// The song's reach about her cell, either way (0x403cc3).
const SONG_REACH: i32 = 7;
/// The song's length (message 0x15ca7 `+0x14`).
const SONG_TICKS: u32 = 750;

/// Her `SIRENA` frames by state, first and last (0x403110).
fn frames_of(state: u8) -> (i32, i32) {
    match state {
        1 => (0, 10),
        2 => (11, 100),
        3 => (101, 113),
        4 => (114, 162),
        _ => (0, 0),
    }
}

/// A siren: the original's fields, by offset.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Siren {
    /// Her generator (`+0x18`).
    pub rng: u32,
    /// State `+0x110`: 0 hidden, 1 surface, 2 sing, 3 dive, 4 death, 5
    /// gone.
    pub state: u8,
    /// Life `+0xb8`, the original's points.
    pub hp: i32,
    /// The song's latch (`+0x114`).
    pub latch: bool,
    /// Her area, cells inclusive: x0, x1, y0, y1 (`+0xfc`-`+0x108`).
    pub area: (i32, i32, i32, i32),
    /// The last frame's tick (`+0x118`), ticks a frame (`+0x11c`).
    pub last: u32,
    pub delay: u32,
    /// The state's start (`+0x120`) and the rest's length (`+0x124`).
    pub since: u32,
    pub rest: u32,
    /// The picture's run done (`+0x128`).
    pub done: bool,
    /// Cut short by a spell (`+0x12c`).
    pub cut: bool,
    /// Her killer (`+0x138`), set by lightning only.
    pub killer: Option<Owner>,
    /// Her cell (`+0xbc`, `+0xc0`), whose slot `+0x18` holds her.
    pub cell: (i32, i32),
    /// Her picture shown (`+0x164`): frame about to be drawn, the state's
    /// first and last, and the one drawn last.
    pub picture: bool,
    pub frame: i32,
    pub start: i32,
    pub end: i32,
    pub drawn: i32,
}

impl Siren {
    fn inside(&self, (x, y): (i32, i32)) -> bool {
        (self.area.0..=self.area.1).contains(&x) && (self.area.2..=self.area.3).contains(&y)
    }
}

impl Scene {
    /// Message 2 (0x403f60): her generator seeded `[game +0x2ee5] x id`,
    /// placed (`creature_start`; her area the whole map only with field 7
    /// or 8 below 0), hidden (state 0).
    pub(super) fn start_siren(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let Some((cell, area)) = self.creature_start(id, CARRIES, 0) else {
            return;
        };
        let mut s = Siren {
            rng: self.game_seed().wrapping_mul(id),
            hp: SIREN_HP,
            cell,
            area,
            ..Default::default()
        };
        let i = self.object_index(id).unwrap();
        self.sync_siren_hull(i, s.hp);
        self.set_siren_state(i, &mut s, 0, data, caches);
        self.sirens.insert(id, s);
    }

    /// The port keeps her life as her object's hull, a fifteenth of it.
    fn sync_siren_hull(&mut self, i: usize, hp: i32) {
        if let Some(c) = self.objects[i].combat.as_mut() {
            c.hull = hp as f32 / 15.0;
        }
    }

    /// Her tick (message 0, 0x403ba0).
    pub(super) fn tick_siren(&mut self, id: u32, data: &GameData, caches: &mut Caches) {
        let (Some(mut s), Some(i)) = (self.sirens.remove(&id), self.object_index(id)) else {
            return;
        };
        let now = self.ticks;
        let next = match s.state {
            0 => {
                if now < s.since + s.rest {
                    None
                } else if s.hp < 1 {
                    // Her kill reported to her killer, if any (0x50e750),
                    // her slot cleared and she goes (0x542670, code 10).
                    if let Some(k) = s.killer {
                        self.note_destroyed(i, Some(k));
                    }
                    self.held.retain(|h| h.0 != id);
                    self.objects.remove(i);
                    return;
                } else if self.siren_move(i, &mut s) {
                    Some(1)
                } else {
                    Some(0)
                }
            }
            1 if !s.done => {
                self.siren_step(&mut s);
                None
            }
            1 => Some(2),
            2 if !s.done => {
                // One song a surfacing: as frame 45 is about to be drawn.
                if s.frame == SONG_FRAME {
                    if !s.latch {
                        s.latch = true;
                        self.siren_song(&s);
                    }
                } else {
                    s.latch = false;
                }
                self.siren_step(&mut s);
                None
            }
            2 => Some(3),
            3 | 4 if !s.done => {
                self.siren_step(&mut s);
                None
            }
            3 if s.hp > 0 => Some(0),
            3 => Some(4),
            4 => Some(0),
            _ => None,
        };
        if let Some(n) = next {
            self.set_siren_state(i, &mut s, n, data, caches);
        }
        self.sirens.insert(id, s);
    }

    /// 0x403650: to state `s` (its run not done, its start now).
    fn set_siren_state(
        &mut self,
        i: usize,
        s: &mut Siren,
        state: u8,
        data: &GameData,
        caches: &mut Caches,
    ) {
        s.done = false;
        s.since = self.ticks;
        let id = self.objects[i].id;
        // A queued target's mark: states 0 and 4 on free it and forget it
        // (0x403650: 0x50c320, `+0x7c` 0); 1 makes it again, 2 and 3 keep
        // it.
        if state == 0 || state >= 4 {
            self.wp_marks.remove(&id);
        }
        match state {
            0 => {
                s.state = 0;
                s.picture = false;
                s.rest = lcg(&mut s.rng) % 751 + 250;
                s.cut = false;
            }
            1 => {
                s.state = 1;
                s.picture = true;
                self.siren_picture(i, s);
                self.siren_step(s);
                // Sound event 95 (`app3`), mode 6 at her cell.
                let (px, py) = (self.objects[i].x, self.objects[i].y);
                self.sounds.push(("app3", px, py));
            }
            2 => {
                s.state = 2;
                if s.hp <= 0 || s.cut {
                    self.set_siren_state(i, s, 3, data, caches);
                    return;
                }
                self.siren_picture(i, s);
                self.siren_step(s);
                self.pick_up_treasure(id, s.cell, CARRIES);
            }
            3 => {
                s.state = 3;
                self.siren_picture(i, s);
                self.siren_step(s);
            }
            4 => {
                s.state = 4;
                self.siren_picture(i, s);
                s.cut = false;
                self.siren_step(s);
                // Sound event 106 (`deamons`), mode 6 at her cell.
                let (px, py) = (self.objects[i].x, self.objects[i].y);
                self.sounds.push(("deamons", px, py));
                self.drop_carried(id, px, py, data, caches);
            }
            _ => {
                s.state = state;
                s.cut = false;
            }
        }
    }

    /// 0x403110: the state's `SIRENA` frames (clipped to the sequence's
    /// last) from the first, a frame every 2 ticks (3 dying), drawn at the
    /// next step.
    fn siren_picture(&self, i: usize, s: &mut Siren) {
        let (start, end) = frames_of(s.state);
        let last = self.objects[i].frames.len() as i32 - 1;
        (s.start, s.end) = (start.min(last), end.min(last));
        s.frame = s.start;
        s.delay = if s.state == 4 { 3 } else { 2 };
        s.last = 0;
    }

    /// 0x403360: when its time has come (at once when cut short), the
    /// frame is drawn and the next made ready; past the last, the first
    /// again and the run done.
    fn siren_step(&self, s: &mut Siren) {
        let d = if s.cut { 0 } else { s.delay };
        if s.last + d > self.ticks {
            return;
        }
        s.last = self.ticks;
        s.drawn = s.frame;
        s.frame += 1;
        if s.frame > s.end {
            s.frame = s.start;
            s.done = true;
        }
    }

    /// The song (0x403ba0 state 2): each map cell's occupant (`+0xc`) of
    /// the 15 x 15 about her (columns then rows, 50 at most), of any side:
    /// each gets message 0x15ca7 with 750 ticks, a share of 88 / their
    /// number (22 from five), her cell and her id (not kept). A ship takes it as
    /// `Scene::song_heard` (0x4a7390). Nothing else reads 0x15ca7: the
    /// fort 0x419f80, the barbarians 0x40ef70, the dock 0x416f90, the
    /// yards 0x4e8660, the port 0x4e7640, the hero's house 0x421150, the
    /// castles 0x410ca0, the nests 0x41d4a0, the lairs 0x414f00, the
    /// schools, smithies, studios and temples through 0x427f50, towns
    /// 0x441ba0 and LandObj 0x425990 have no case for it, the land
    /// creatures' jam 0x415ee0 hands only 0xda93, 0xda96 and 0x187a2 to
    /// its members (the rest to 0x50c9e0, none): such an occupant counts
    /// in the share and takes nothing.
    fn siren_song(&mut self, s: &Siren) {
        let (x, y) = s.cell;
        // Sound event 128 (`sir1`), mode 6 at her cell.
        let t = TILE_SIZE as f32;
        self.sounds
            .push(("sir1", x as f32 * t + t / 2.0, y as f32 * t + t / 2.0));
        let r = SONG_REACH;
        let mut list = self.cell_occupants((x - r, y - r), (x + r + 1, y + r + 1));
        list.sort_by_key(|&(c, _)| c);
        list.truncate(50);
        let n = list.len() as u32;
        if n == 0 {
            return;
        }
        let share = if n < 5 { 88 / n } else { 22 };
        let ids: Vec<u32> = list.iter().map(|&(_, k)| self.objects[k].id).collect();
        for ship in ids {
            self.song_heard(ship, SONG_TICKS, share, s.cell);
        }
    }

    /// 0x403520, at the end of a rest: the cells of the 3 x 3 about her
    /// (her own too; columns then rows) within her area that 0x402ee0
    /// lets her into (strict, never onto a treasure), each weighing 1.
    /// One picked (0x404730), she moves there. False with none.
    fn siren_move(&mut self, i: usize, s: &mut Siren) -> bool {
        let id = self.objects[i].id;
        let mut cells = Vec::new();
        for di in 0..3 {
            for dj in 0..3 {
                let c = (s.cell.0 - 1 + di, s.cell.1 - 1 + dj);
                if s.inside(c) && self.may_hold(id, c, true, false) {
                    cells.push(c);
                }
            }
        }
        if cells.is_empty() {
            return false;
        }
        s.cell = cells[pick(&vec![1; cells.len()], &mut s.rng)];
        let t = TILE_SIZE as f32;
        self.objects[i].x = s.cell.0 as f32 * t + t / 2.0;
        self.objects[i].y = s.cell.1 as f32 * t + t / 2.0;
        true
    }

    /// vt+4 (0x403480): a blow of `damage` (ours, a fifteenth of hers)
    /// from player `by`. Only surfacing, singing or diving does she take
    /// it: off her life (down to 0), a blow leaving none reporting her
    /// kill at once (0x50e750, every such blow). Sound event 117 has no
    /// sample: silent.
    pub(super) fn siren_struck(&mut self, id: u32, damage: f32, by: Option<Owner>) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        let Some(s) = self.sirens.get_mut(&id) else {
            return;
        };
        if !matches!(s.state, 1..=3) {
            return;
        }
        let blow = (damage.max(0.0) * 15.0).round() as i32;
        s.hp = (s.hp - blow).max(0);
        let hp = s.hp;
        self.sync_siren_hull(i, hp);
        if hp == 0 {
            self.note_destroyed(i, by);
        }
    }

    /// Lightning on her (0xda93, her picture shown): her life to 0, cut
    /// short, the caster her killer (reported when she goes); event 117,
    /// silent.
    pub(super) fn siren_lightning(&mut self, id: u32, caster: Option<Owner>) {
        let Some(i) = self.object_index(id) else {
            return;
        };
        if let Some(s) = self.sirens.get_mut(&id).filter(|s| s.picture) {
            s.hp = 0;
            s.cut = true;
            s.killer = caster;
            self.sync_siren_hull(i, 0);
        }
    }

    /// Whether she can be shot at (vt+0x2c, 0x404690): surfacing, singing
    /// or diving.
    pub(super) fn siren_up(&self, id: u32) -> bool {
        self.sirens
            .get(&id)
            .is_some_and(|s| matches!(s.state, 1..=3))
    }

    /// The `SIRENA` frame she last drew.
    pub(super) fn siren_frame(&self, id: u32) -> Option<u32> {
        self.sirens
            .get(&id)
            .filter(|s| s.picture)
            .map(|s| s.drawn.max(0) as u32)
    }

    /// The point a shot at her flies to (vt+0xc, 0x4048f0, by the hit
    /// byte): with no picture her cell's centre; else from her picture's
    /// corner (centred on her cell), a hit `30 + (r & 7)` each way, a miss
    /// `r % 61`, `40 + r % 24` (x drawn first).
    pub(super) fn siren_aim(&mut self, id: u32, hit: bool) -> Option<(i32, i32)> {
        let i = self.object_index(id)?;
        let t = TILE_SIZE as i32;
        let cell = self.sirens.get(&id)?.cell;
        let (px, py) = self.picture_corner(i, cell);
        let s = self.sirens.get_mut(&id)?;
        if !s.picture {
            return Some((cell.0 * t + t / 2, cell.1 * t + t / 2));
        }
        Some(if hit {
            let x = (lcg(&mut s.rng) & 7) as i32 + 30;
            (px + x, py + (lcg(&mut s.rng) & 7) as i32 + 30)
        } else {
            let x = (lcg(&mut s.rng) % 61) as i32;
            (px + x, py + (lcg(&mut s.rng) % 24) as i32 + 40)
        })
    }
}
