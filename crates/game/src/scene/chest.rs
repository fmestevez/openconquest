//! The sunken chest (`SkarbClassTy` 0x2c1, `andrey\ska`, handler
//! 0x405230): dropped by a sinking ship at its sinking's next-to-last frame
//! (0x4dec40 when 0x4c12b0 returns 1: 0x4e4e30, world message 0x16e2), it
//! rises, lies a while and sinks on the ship's cell; a ship stepping onto
//! that cell takes its amber (0x4a3f40 at 0x4a47c0). It lies in its cell's
//! `+0x10` slot, the leviathans' (0x5c74fc grid, `+8` in words).

use openconquest_formats::ObjectRecord;

use super::{Scene, TILE_SIZE};
use crate::assets::GameData;
use crate::objects::{self, Caches, Treasure};

/// 0x597afc: the treasures aboard a sinking hull lays on its cell (by the
/// hull's type `+0x25a`; 0x4e4f20).
pub const SINK_TREASURES: [u8; 12] = [1, 1, 2, 1, 1, 1, 1, 1, 0, 0, 0, 0];

/// A chest: the original's fields, by offset.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Chest {
    /// Its cell (`+0xb8`, `+0xbc`).
    pub cell: (i32, i32),
    /// The amber it holds (`+0xc0`).
    pub value: u32,
    /// Its kind (`+0x74`): the picture `SKARB<kind><0|1>`.
    pub kind: u32,
    /// Its generator (`+0x18`).
    pub rng: u32,
    /// State `+0xb4`: 1 rising, 2 lying, 3 sinking, 4 gone.
    pub state: u8,
    /// The frame to draw next (`+0x45`) and the one drawn.
    pub frame: i32,
    pub drawn: i32,
    /// The picture's first and last frames (`+0x4d`, `+0x51`).
    pub first: i32,
    pub last: i32,
    /// Passes made (`+0x88`) and to make (`+0x8c`).
    pub passes: u32,
    pub target: u32,
    /// Hurried (`+0x90`): no delay between frames.
    pub hurried: bool,
    /// The tick of the last step (`+0x7c`).
    pub stepped: u32,
}

/// A sinking ship still in the world's object table (0x51f1f0 finds it):
/// the original keeps the hull (`+0x74` 5, `+0x251` 4) until 0x4dec40,
/// on 0x4c12b0's 2 (the frame at the count; with no picture the first
/// step), posts message 10 to it (0x542670) and it is removed. Its
/// player's list lost it at the sinking (0x4a8410), so 0x4ac050 no
/// longer finds it for a player (0x4a8710) and the attackers' 0x13a
/// comes while it is still found here. The port takes the ship out at
/// once and keeps this.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Sinking {
    /// The ship's id (`+0x4c`).
    pub id: u32,
    /// Its cell (`+0x5a3`, `+0x5a7`; the hull no longer moves).
    pub cell: (i32, i32),
    /// The tick of message 10: a step each 3 ticks (`+0x7d` 2), so
    /// `3 x max(frames, 1)` from the sinking, counted from the sinking as
    /// the port's frames are.
    pub until: u32,
}

/// What a sinking ship leaves at its next-to-last frame (0x4e4e30,
/// 0x4e4f20), kept from the sinking (the port takes the ship out then).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SinkDrop {
    /// The tick of the sinking's next-to-last frame.
    pub at: u32,
    /// The ship's cell (`+0x5a3`, `+0x5a7`).
    pub cell: (i32, i32),
    /// Half the hull's sale value (0x4e4ce0 / 2).
    pub value: u32,
    /// The hull's type (`+0x25a`).
    pub hull: u8,
    /// The treasures aboard (`+0x482`, `+0x485`), in their order.
    pub treasures: Vec<Treasure>,
}

/// The chest's picture (0x404af0): `SKARB<kind>1` lying, else
/// `SKARB<kind>0`; each frame's top left at `cell x 64 - (w - 0x40) / 2`
/// on both axes, `w` the picture's width (0x404c90: `+0x61` on x and on
/// y), the object at the cell's middle.
pub fn chest_frames(
    data: &GameData,
    caches: &mut Caches,
    kind: u32,
    lying: bool,
    cell: (i32, i32),
) -> Vec<Vec<objects::Layer>> {
    let name = format!("SKARB{kind}{}", u32::from(lying));
    let mut frames = objects::sequence_frames(data, caches, &name).unwrap_or_default();
    let t = TILE_SIZE as i32;
    let Some(w) = frames
        .first()
        .and_then(|f| f.first())
        .map(|l| l.image.width as i32)
    else {
        return frames;
    };
    let (cx, cy) = (cell.0 * t + t / 2, cell.1 * t + t / 2);
    let (left, top) = (cell.0 * t - (w - 0x40) / 2, cell.1 * t - (w - 0x40) / 2);
    for f in &mut frames {
        for l in f {
            l.dx = left - cx + l.image.width as i32 / 2;
            l.dy = top - cy + l.image.height as i32 / 2;
        }
    }
    frames
}

impl Scene {
    /// The chest lying in cell `c`'s slot.
    pub(super) fn chest_at(&self, c: (i32, i32)) -> Option<u32> {
        self.chests
            .iter()
            .find(|(_, k)| k.cell == c && k.state != 4)
            .map(|(&id, _)| id)
    }

    /// The frame chest `id` shows.
    pub(super) fn chest_frame(&self, id: u32) -> Option<u32> {
        self.chests.get(&id).map(|k| k.drawn.max(0) as u32)
    }

    /// A ship sinks: what its next-to-last frame will leave (0x4e4e30,
    /// 0x4e4f20), `frames` its sinking picture's count. 0x4c12b0 steps a
    /// frame when more than 2 ticks have passed (delay `+0x7d` 2) and
    /// returns 1 when the step leaves the frame at count - 2: never with
    /// fewer than 3 frames (the first step already leaves frame 1, and
    /// with none it is past the last), so nothing is left then.
    pub(super) fn sink_drop(&mut self, i: usize, frames: usize, treasures: Vec<Treasure>) {
        {
            let t = TILE_SIZE as f32;
            let o = &self.objects[i];
            let cell = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
            self.sinkings.push(Sinking {
                id: o.id,
                cell,
                until: self.ticks + 3 * (frames.max(1) as u32),
            });
        }
        if frames < 3 {
            return;
        }
        let t = TILE_SIZE as f32;
        let o = &self.objects[i];
        let Some(ship) = o.ship.as_ref() else {
            return;
        };
        let cell = ((o.x / t).floor() as i32, (o.y / t).floor() as i32);
        let hull = ship.kind;
        let value = (self.sale_value(i).0 / 2).max(0) as u32;
        self.sink_drops.push(SinkDrop {
            at: self.ticks + 3 * (frames as u32 - 2),
            cell,
            value,
            hull,
            treasures,
        });
    }

    /// The sinkings' next-to-last frames come (0x4dec40 on 0x4c12b0's 1):
    /// the chest (0x4e4e30), then the treasures aboard laid on the cell,
    /// the first of them each time, at most 0x597afc[type] (0x4e4f20 ->
    /// 0x4bd500 -> 0x520c70; the rest go down with the ship). Not ported:
    /// each one's announcement to the brains (0x4b6e80, message 0x5d94
    /// with 0).
    pub(super) fn run_sink_drops(&mut self, data: &GameData, caches: &mut Caches) {
        let now = self.ticks;
        self.sinkings.retain(|k| k.until > now);
        let (due, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.sink_drops)
            .into_iter()
            .partition(|d| d.at <= now);
        self.sink_drops = kept;
        let t = TILE_SIZE as f32;
        for d in due {
            // The kind by the hull's type (0x4e4e30's switch: 4 and 8 give
            // 2; 0x10, 0x20, 0x40 and 0x80 1, beyond the types; else 0).
            let kind = match d.hull {
                4 | 8 => 2,
                0x10 | 0x20 | 0x40 | 0x80 => 1,
                _ => 0,
            };
            self.new_chest(d.cell, d.value, kind, data, caches);
            let cap = SINK_TREASURES.get(d.hull as usize).copied().unwrap_or(0) as usize;
            let (x, y) = (d.cell.0 as f32 * t + t / 2.0, d.cell.1 as f32 * t + t / 2.0);
            for tr in d.treasures.into_iter().take(cap) {
                // Message 0x5d94 with 0 for each (0x4e4f20 at 0x4e4f52).
                self.treasure_news(None, None, tr, false);
                self.place_treasure(tr, x, y, data, caches);
            }
        }
    }

    /// Message 0x16e2 (0x51ee0f), a new chest's message 2 (0x405230): its
    /// generator seeded `[game +0x2ee5] x id`; a cell 0x48ec60 refuses
    /// (landscape low nibble above 7 or nibble 1 above 3, a ship in the
    /// occupant slot, anything on `+0x1c`, a picture on `+0x18`, the
    /// `+0x10` slot taken) and it goes at once (message 10); else it takes
    /// the slot, rises (state 1) and plays event 88 (`wreck1`, mode 6) at
    /// its cell (0x4053f8).
    fn new_chest(
        &mut self,
        cell: (i32, i32),
        value: u32,
        kind: u32,
        data: &GameData,
        caches: &mut Caches,
    ) {
        if !self.leviathan_may_be(None, cell, true, false) || self.chest_at(cell).is_some() {
            return;
        }
        let id = self.objects.iter().map(|o| o.id).max().unwrap_or(0) + 1;
        let mut fields = vec![0u32; 8];
        fields[0] = 0x2c1;
        fields[4] = cell.0 as u32;
        fields[5] = cell.1 as u32;
        fields[6] = kind;
        let rec = ObjectRecord {
            id,
            class_id: 0x2c1,
            fields,
        };
        let Ok(Some(obj)) = objects::resolve(data, caches, &rec) else {
            return;
        };
        self.objects.push(obj);
        let rng = self.objects_seed().wrapping_mul(id);
        let mut k = Chest {
            cell,
            value,
            kind,
            rng,
            ..Chest::default()
        };
        self.chest_state(id, &mut k, 1, data, caches);
        self.chests.insert(id, k);
        let t = TILE_SIZE as f32;
        self.sounds.push((
            "wreck1",
            cell.0 as f32 * t + t / 2.0,
            cell.1 as f32 * t + t / 2.0,
        ));
    }

    /// 0x404e10: chest `id` to state `s`, the passes from 0. Rising (1)
    /// one pass of `SKARB<k>0` backward; lying (2) `r % 16 + 5` passes of
    /// `SKARB<k>1` (`r` its generator's `>> 16 & 0xf`), none if hurried,
    /// which sinks it at once; sinking (3) one pass of `SKARB<k>0`. Each
    /// loads its picture (0x404c90: from frame count - 2 rising, else the
    /// first; delay 2, last step 0) and steps at once (0x404b50). Any other
    /// state: no longer hurried.
    fn chest_state(&mut self, id: u32, k: &mut Chest, s: u8, data: &GameData, caches: &mut Caches) {
        let mut s = s;
        loop {
            k.passes = 0;
            match s {
                1 | 3 => {
                    k.state = s;
                    k.target = 1;
                }
                2 => {
                    k.state = 2;
                    k.target = if k.hurried {
                        0
                    } else {
                        k.rng = k.rng.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
                        (k.rng >> 16 & 0xf) + 5
                    };
                    if k.target == 0 {
                        s = 3;
                        continue;
                    }
                }
                _ => {
                    k.state = s;
                    k.hurried = false;
                    return;
                }
            }
            break;
        }
        let lying = k.state == 2;
        let frames = chest_frames(data, caches, k.kind, lying, k.cell);
        let count = frames.len() as i32;
        k.first = if count < 1 { count - 1 } else { 0 };
        k.last = (count - 1).max(0);
        k.frame = if k.state == 1 { k.last - 1 } else { k.first };
        k.stepped = 0;
        if let Some(i) = self.object_index(id) {
            self.objects[i].frames = frames;
            if let Some(f) = self.objects[i].record.fields.get_mut(7) {
                *f = u32::from(lying);
            }
        }
        self.chest_step(k);
    }

    /// 0x404b50: once the tick reaches the last step plus the delay (2, 0
    /// hurried), the frame is drawn and the next readied: backward rising,
    /// from the first past it to the last but one; forward else, from the
    /// last past it to the first; each wrap a pass.
    fn chest_step(&self, k: &mut Chest) {
        let delay = if k.hurried { 0 } else { 2 };
        if self.ticks < k.stepped + delay {
            return;
        }
        k.stepped = self.ticks;
        k.drawn = k.frame;
        match k.state {
            1 => {
                k.frame -= 1;
                if k.frame >= 0 {
                    return;
                }
                k.frame = k.last - 1;
            }
            2 | 3 => {
                k.frame += 1;
                if k.frame <= k.last {
                    return;
                }
                k.frame = k.first;
            }
            _ => return,
        }
        k.passes += 1;
    }

    /// The chests' tick (message 0, 0x404f80): a state's passes made, the
    /// next (rising to lying to sinking); lying hurried, the pass under way
    /// is the last; sunk, the slot is cleared and the chest goes (message
    /// 10, state 4).
    pub(super) fn tick_chests(&mut self, data: &GameData, caches: &mut Caches) {
        let ids: Vec<u32> = self.chests.keys().copied().collect();
        for id in ids {
            let Some(mut k) = self.chests.remove(&id) else {
                continue;
            };
            match k.state {
                1 if k.passes < k.target => self.chest_step(&mut k),
                1 => self.chest_state(id, &mut k, 2, data, caches),
                2 if k.passes < k.target => {
                    if k.hurried {
                        k.passes = k.target - 1;
                    }
                    self.chest_step(&mut k);
                }
                2 => self.chest_state(id, &mut k, 3, data, caches),
                3 if k.passes < k.target => self.chest_step(&mut k),
                3 => {
                    self.chest_state(id, &mut k, 4, data, caches);
                    self.objects.retain(|o| o.id != id);
                    continue;
                }
                _ => {}
            }
            self.chests.insert(id, k);
        }
    }

    /// A ship has stepped into cell `c` (0x4a3f40 at 0x4a47c0): a chest in
    /// its `+0x10` slot plays, for the viewing player's ship, the event of
    /// its nation (`+0x54`: 0 0xac, 1 0xad, 3 0xae, 2 0xaf, mode 5; only
    /// 0xad and 0xae list sounds, `piw1a`, `piw3a`, `piw4a`); not gone, it
    /// gives the ship's player its amber (0x4a8de0) and is sent 0x158c0,
    /// which hurries it and sinks it (0x405230).
    pub(super) fn ship_steps_on_chest(
        &mut self,
        ship: u32,
        c: (i32, i32),
        data: &GameData,
        caches: &mut Caches,
    ) {
        let Some(id) = self.chest_at(c) else {
            return;
        };
        let Some(i) = self.object_index(ship) else {
            return;
        };
        let owner = self.objects[i].owner;
        if self.is_own(&self.objects[i]) && matches!(owner, Some((1 | 3, _))) {
            let t = TILE_SIZE as f32;
            let (x, y) = (c.0 as f32 * t + t / 2.0, c.1 as f32 * t + t / 2.0);
            let name = ["piw1a", "piw3a", "piw4a"][self.gull_rng.next() as usize % 3];
            self.sounds.push((name, x, y));
        }
        let Some(mut k) = self.chests.remove(&id) else {
            return;
        };
        if k.state != 4 {
            if let Some(owner) = owner {
                self.earn(owner, k.value as i64, 0);
            }
            k.hurried = true;
            self.chest_state(id, &mut k, 3, data, caches);
        }
        self.chests.insert(id, k);
    }
}
