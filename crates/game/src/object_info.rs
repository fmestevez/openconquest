//! The object info in the strip below the view: up to six labelled bars of
//! the object under the pointer (life, fire power, board power,
//! experience, cargo, volume) and its hero and treasure icons.
//!
//! The pointer (gcursor.cpp 0x516c10, from 0x5175b0 while "Help string"
//! is on, `[0x5ca542]`) has the object under it fill a 13-dword struct
//! through its vtable +0x18 and, when that changed, sends it to the field
//! with message 0x627 (null off objects); Fieldsys.cpp 0x503d60 draws it
//! on a copy of `CHT_BKINFO` at (107, 527).

use std::rc::Rc;

use openconquest_formats::ObjectClass;

use crate::assets::{rgb, GameData, Image};
use crate::combat;
use crate::objects::MapObject;
use crate::render::{blit_scaled, Frame};
use crate::scene::Scene;
use crate::text::TextRenderer;

/// Where the strip lies on the 800x600 screen (0x503d60: x 0x504ef0(0x6b),
/// y 0x20f; 0x504ef0 adds 208 with the control bar on the left).
pub const ORIGIN: (u32, u32) = (107, 527);
/// Cells' pitch (0x503d60).
const CELL_STEP: u32 = 78;
/// A bar's full width and the label's box in a cell (0x546d80 at (2, 2)).
const BAR_W: u32 = 67;
const LABEL: (u32, u32, u32, u32) = (2, 2, 67, 9);
/// The labels: strings 0x3ca-0x3cf.
const LABELS: [u32; 6] = [0x3ca, 0x3cb, 0x3cc, 0x3cd, 0x3ce, 0x3cf];

/// What the strip shows of an object: the struct its vtable +0x18 fills
/// (base 0x50e7b0). `None` leaves a cell out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ObjectInfo {
    /// `+0xa`, `+0xe`: life in percent.
    pub life: Option<u8>,
    /// `+0xf`, `+0x13`, `+0x15`: fire power against hulls and crews.
    pub fire: Option<(u16, u16)>,
    /// `+0x17`, `+0x1b`.
    pub board: Option<u16>,
    /// `+0x1d`, `+0x21`, `+0x22`: have and most.
    pub experience: Option<(u8, u8)>,
    /// `+0x23`, `+0x27`, `+0x28`.
    pub cargo: Option<(u8, u8)>,
    /// `+0x29`, `+0x2d`, `+0x2e`.
    pub volume: Option<(u8, u8)>,
    /// `+0x2f` (0xff none): the hero aboard, 9 for several.
    pub hero: Option<u8>,
    /// `+0x30`: treasures aboard (vtable +0x20).
    pub treasure: bool,
}

/// The A x 15 cache (0x4ac140 at 0x5c75cc, its copy at 0x5c73a8): row
/// `item` of 0x597d2c, its A (or B without one) at `level`, x 1500 / 100.
fn cache(item: u8, level: u8) -> u32 {
    let a = combat::hull_damage_at(item, level) as u32;
    let a = if a == 0 {
        combat::crew_damage_at(item, level) as u32
    } else {
        a
    };
    a * 1500 / 100
}

/// 0x4ac1b0: `value` x B / A of row `item` at `level` (as is without A).
fn against_crews(item: u8, level: u8, value: u32) -> u32 {
    let b = combat::crew_damage_at(item, level) as u32;
    (b * value)
        .checked_div(combat::hull_damage_at(item, level) as u32)
        .unwrap_or(value)
}

/// Fire power in the strip's units: x 100 / 1500.
fn scaled(v: u32) -> u16 {
    (v * 100 / 1500) as u16
}

/// Life in percent as the objects keep it (ships 0x4e53e0: hull x 100 /
/// the type's; buildings `+0xa8`; creatures their own).
fn life(obj: &MapObject) -> u8 {
    obj.combat.as_ref().map_or(0, |c| {
        if c.max_hull > 0.0 {
            (c.hull.max(0.0) * 100.0 / c.max_hull).min(100.0) as u8
        } else {
            0
        }
    })
}

/// The hero icon (0x40df20 / ship `+0x4b5`): the only hero, 9 for more.
fn hero_icon(heroes: &[u8]) -> Option<u8> {
    match heroes {
        [] => None,
        [h] => Some(*h),
        _ => Some(9),
    }
}

/// A deposit's volume: `+0x78` / `div` (AmberClassTy and FishClassTy
/// 0x48b100 by 50, the leviathan 0x495f40 by 1), above 100 as a u16 shown
/// full.
fn volume(amount: i32, div: i32) -> (u8, u8) {
    let v = amount / div;
    (if v as u16 > 100 { 100 } else { v as u8 }, 100)
}

/// The struct the object under the pointer fills (vtable +0x18); `None`
/// for the cyclops' jam (type 0x1f7, which 0x516c10 does not show,
/// leaving the last info up).
pub fn of(scene: &Scene, obj: &MapObject) -> Option<ObjectInfo> {
    let mut info = ObjectInfo::default();
    let f = |i: usize| obj.record.field(i).unwrap_or(0) as i32;
    match obj.class {
        ObjectClass::Ship => ship(scene, obj, &mut info),
        // CastleClassTy, LandObjClassTy, TownObjClassTy, ... (0x423990):
        // life, the hero in it and whether it holds treasures; forts
        // (Fortress.cpp 0x417dc0) and barbarian strongholds (Barb.cpp
        // 0x40e7f0) add their fire power and garrison.
        ObjectClass::Building(_)
        | ObjectClass::Barbarian(_)
        | ObjectClass::Castle(_)
        | ObjectClass::HarpyNest
        | ObjectClass::CreatureSpawn
        | ObjectClass::HeroHouse => {
            info.life = Some(life(obj));
            info.hero = scene.site_heroes.get(&obj.id).and_then(|h| hero_icon(h));
            info.treasure = scene.held.iter().any(|h| h.0 == obj.id);
            if let ObjectClass::Building(c @ 0x34..=0x36)
            | ObjectClass::Barbarian(c @ 0x3f..=0x41) = obj.class
            {
                garrison(scene, obj, c, &mut info);
            }
        }
        // CyclopClassTy 0x413cf0: life (`+0x78`) and its throw's blow
        // (0x4126f0: a ballista's, a small catapult's and item 3's at
        // level 1) against hulls.
        ObjectClass::Monster(c @ 0x1f4..=0x1f6) => {
            info.life = Some(life(obj));
            let item = [6, 5, 3][(c - 0x1f4) as usize];
            info.fire = Some((scaled(cache(item, 1)), 0));
        }
        ObjectClass::Monster(0x1f7) => return None,
        // Harpies and dragons (0x420760), the gorgon (0x402ec0), sea
        // monsters and sirens (0x4049f0): life.
        ObjectClass::Monster(0x1f8..=0x1fb | 0x1fd) => info.life = Some(life(obj)),
        // AmberClassTy, FishClassTy (0x48b100): what is left, `+0x78`
        // (field 4 x 100; an unlimited fishing ground's schools come up
        // with -field 4 x 100), by 50.
        ObjectClass::Amber | ObjectClass::FishZone => {
            let left = match obj.deposit {
                Some(d) => d.remaining.map_or(f(4).abs() * 100, |r| r as i32),
                None => 0,
            };
            info.volume = Some(volume(left, 50));
        }
        // WhaleClassTy 0x495f40: a leviathan with life (`+0x80`, field 6,
        // of `+0x7c`, field 5; 0x4935d0) shows that, else what is left.
        ObjectClass::WhaleZone => {
            if f(6) > 0 {
                info.life = Some((f(6) * 100 / f(5).max(1)).min(100) as u8);
            } else {
                let left = obj
                    .deposit
                    .and_then(|d| d.remaining)
                    .map_or(0, |r| r as i32);
                info.volume = Some(volume(left, 1));
            }
        }
        // The rest keep the base (0x50e7b0): nothing shown.
        _ => {}
    }
    Some(info)
}

/// ShipClassTy 0x4e51c0.
fn ship(scene: &Scene, obj: &MapObject, info: &mut ObjectInfo) {
    let Some(s) = &obj.ship else {
        return;
    };
    info.life = Some(life(obj));
    let kind = s.kind as usize;
    if kind <= 1 {
        // Boats and merchants: the cargo places taken (0x597af0 less
        // 0x4be800 mode 3), goods a hundred units a place, marines one.
        let most = [4u32, 10][kind];
        let used = (s.cargo / 100.0).ceil() as u32 + s.crew.count();
        let free = most.saturating_sub(used);
        info.cargo = Some(((most - free) as u8, most as u8));
    } else {
        // Each item's fire power (0x4bc360: A x 15 x count x mean
        // condition / 100), against hulls (0x4bc610: x 0x597fd8 / 100, x
        // count) and crews (0x4bc660: count times its x B / A), by the
        // owner's damage level of its group.
        let levels = scene.levels_of(obj.owner);
        let units = obj.unit_conditions();
        let (mut hulls, mut crews) = (0u32, 0u32);
        for (i, u) in units.iter().enumerate() {
            let n = u.len() as u32;
            if n == 0 {
                continue;
            }
            let item = i as u8;
            let level = levels.arms_for(item);
            let mean = u.iter().map(|&c| c as u32).sum::<u32>() / n;
            let power = cache(item, level) * n * mean / 100;
            hulls += power * combat::hull_percent(item) / 100 * n;
            crews += n * against_crews(item, level, power);
        }
        info.fire = Some((scaled(hulls), scaled(crews)));
        // Boarding strength (`+0x53c`, 0x4bee60) x 1000 / 1500, and the
        // crew's experience (`+0x544` / 100 percent; none in a dock,
        // state 2). Quick lime (`+600`) would zero the fire, halve the
        // boarding and hide the experience: not modelled.
        let i = scene.object_index(obj.id).unwrap_or(0);
        info.board = Some((scene.boarding_strength(i) * 1000 / 1500) as u16);
        info.experience = Some((0, 100));
    }
    info.treasure = !s.treasures.is_empty();
    info.hero = hero_icon(&s.heroes);
}

/// A fort's or barbarian stronghold's units (0x417dc0, 0x40e7f0): each
/// group's fire (0x446710: its units' condition x A x 15 / 100), against
/// crews by the running total (0x4ac1b0 is passed the sum so far), and
/// its room taken (0x59729c: 1, 1, 2 by group) of 4, 7 or 10 (0x5967a8).
fn garrison(scene: &Scene, obj: &MapObject, class: u32, info: &mut ObjectInfo) {
    let levels = scene.levels_of(obj.owner);
    let places = obj.fort_places();
    let (mut hulls, mut crews, mut room) = (0u16, 0u16, 0u32);
    for item in 0..9u8 {
        let units = &places[item as usize];
        let n = units.len() as u32;
        if n == 0 {
            continue;
        }
        let level = levels.arms_for(item);
        let fire: u32 = units
            .iter()
            .map(|&c| c as u32 * cache(item, level) / 100)
            .sum();
        hulls = hulls.wrapping_add(fire as u16);
        crews = crews.wrapping_add(against_crews(item, level, hulls as u32) as u16);
        room += [1, 1, 2][(item / 3) as usize] * n;
    }
    info.fire = Some((scaled(hulls as u32), scaled(crews as u32)));
    let base = if class >= 0x3f { 0x3f } else { 0x34 };
    info.volume = Some((room as u8, [4, 7, 10][(class - base) as usize]));
}

/// The strip's pictures (Fieldsys.cpp 0x5032c0).
pub struct InfoArt {
    /// `CHT_BKINFO` (`+0x10e`; `CHT_BKFINFO` with the bar hidden).
    background: Option<Image>,
    /// A cell's background, `GAME_INDOBJ` (`+0x116`).
    cell: Option<Rc<Image>>,
    /// `CIND_HTR_00`-`10` (`+0x11e`): the treasure, then a hero each and
    /// several heroes.
    icons: Vec<Option<Image>>,
    /// `IND_FONT` (`+0x11a`; 0x5470f0: no gap between letters).
    font: Option<TextRenderer>,
    labels: Vec<String>,
    /// Palette 0 (the bars' frame), and 200/199, 203/157, 65/68 (the
    /// bars' two lines: life from 50%, from 25%, below).
    black: u32,
    colours: [[u32; 2]; 3],
}

impl InfoArt {
    pub fn load(data: &GameData, theme: char) -> Self {
        let image = |name: &str| data.load_image(&data.controls, name, false).ok().flatten();
        let pal = |i: usize| rgb(data.palette.0[i]);
        Self {
            background: image(&format!("{theme}CHT_BKINFO")),
            cell: image(&format!("{theme}GAME_INDOBJ")).map(Rc::new),
            icons: (0..11)
                .map(|i| image(&format!("CIND_HTR_{i:02}")))
                .collect(),
            font: TextRenderer::load_in(
                &data.controls,
                &format!("{theme}IND_FONT"),
                0,
                Some(&data.palette),
            )
            .ok()
            .map(|f| f.with_spacing(0)),
            labels: LABELS
                .iter()
                .map(|id| data.strings.get(id).cloned().unwrap_or_default())
                .collect(),
            black: pal(0),
            colours: [[200, 199], [203, 157], [65, 68]].map(|p| p.map(pal)),
        }
    }

    /// The strip as 0x503d60 draws it, at its native size.
    pub fn render(&self, info: &ObjectInfo) -> Option<Image> {
        let bg = self.background.as_ref()?;
        let (w, h) = (bg.width, bg.height);
        let mut pixels: Vec<u32> = bg.pixels.iter().map(|p| p.unwrap_or(0)).collect();
        let mut frame = Frame {
            pixels: &mut pixels,
            width: w,
            height: h,
        };
        let colours = self.colours[0];
        let root =
            |v: u16, most: u32| ((v.min(most as u16) as u32 * 4489 / most) as f64).sqrt() as u32;
        let share = |(have, most): (u8, u8)| {
            if most == 0 {
                0
            } else {
                (have as u32 * BAR_W / most as u32).min(BAR_W)
            }
        };
        let mut x = 0;
        let mut cell = |frame: &mut Frame, label: usize, bars: &[(u32, u32, [u32; 2])]| {
            if let Some(img) = &self.cell {
                blit(frame, img, x as i64, 0);
            }
            for &(y, len, c) in bars {
                self.bar(frame, x, y, len, c);
            }
            self.label(frame, x, &self.labels[label]);
            x += CELL_STEP;
        };
        if let Some(pct) = info.life {
            let c = match pct {
                50.. => self.colours[0],
                25.. => self.colours[1],
                _ => self.colours[2],
            };
            cell(
                &mut frame,
                0,
                &[(14, (pct as u32 * BAR_W / 100).min(BAR_W), c)],
            );
        }
        if let Some((hulls, crews)) = info.fire {
            cell(
                &mut frame,
                1,
                &[
                    (11, root(hulls, 0x60e), colours),
                    (14, root(crews, 1000), colours),
                ],
            );
        }
        if let Some(v) = info.board {
            cell(&mut frame, 2, &[(14, root(v, 0x44c), colours)]);
        }
        for (k, v) in [(3, info.experience), (4, info.cargo), (5, info.volume)] {
            if let Some(v) = v {
                cell(&mut frame, k, &[(14, share(v), colours)]);
            }
        }
        // The icons at the right end: the hero (frame + 1), the treasure.
        if let Some(Some(img)) = info.hero.and_then(|i| self.icons.get(i as usize + 1)) {
            blit(&mut frame, img, w as i64 - 0x43, 0);
        }
        if let (true, Some(Some(img))) = (info.treasure, self.icons.first()) {
            blit(&mut frame, img, w as i64 - 0x28, 0);
        }
        Some(Image::new(w, h, pixels.into_iter().map(Some).collect()))
    }

    /// A bar at cell `x`, row `y`, `len` long (0x503d60): a black frame 4
    /// high (0x563140) once longer than 1, its two inside rows in colour
    /// (0x562d40) once longer than 2.
    fn bar(&self, frame: &mut Frame, x: u32, y: u32, len: u32, c: [u32; 2]) {
        if len <= 1 {
            return;
        }
        let (x0, x1) = ((x + 2) as i64, (x + 2 + len - 1) as i64);
        let (y0, y1) = (y as i64, (y + 3) as i64);
        for px in x0..=x1 {
            frame.put(px, y0, self.black);
            frame.put(px, y1, self.black);
        }
        for py in y0..=y1 {
            frame.put(x0, py, self.black);
            frame.put(x1, py, self.black);
        }
        if len > 2 {
            for px in (x + 3) as i64..=(x + len) as i64 {
                frame.put(px, y0 + 1, c[0]);
                frame.put(px, y0 + 2, c[1]);
            }
        }
    }

    /// A cell's label, centred in its box and clipped to it (0x546d80,
    /// 0x547860 with -1, -1).
    fn label(&self, frame: &mut Frame, x: u32, text: &str) {
        let Some(font) = &self.font else {
            return;
        };
        let (bx, by, bw, bh) = LABEL;
        let mut pixels: Vec<u32> = (0..bw * bh)
            .map(|i| {
                let (px, py) = (x + bx + i % bw, by + i / bw);
                frame.get(px as i64, py as i64).unwrap_or(0)
            })
            .collect();
        let mut sub = Frame {
            pixels: &mut pixels,
            width: bw,
            height: bh,
        };
        let left = (bw as i32 - font.measure(text) as i32) / 2;
        let top = (bh as i32 - font.height(text) as i32) / 2;
        font.draw(&mut sub, left, top, 1, text);
        for (i, &p) in pixels.iter().enumerate() {
            let i = i as u32;
            frame.put((x + bx + i % bw) as i64, (by + i / bw) as i64, p);
        }
    }

    /// Draws the strip for `info` with its top-left at `(x, y)` (window
    /// pixels), magnified by `scale`.
    pub fn draw(&self, frame: &mut Frame, x: f32, y: f32, scale: f32, info: &ObjectInfo) {
        if let Some(img) = self.render(info) {
            blit_scaled(frame, &img, (0, 0, img.width, img.height), x, y, scale);
        }
    }
}

/// Copies `img` onto `frame` at `(x, y)`, skipping transparent pixels.
fn blit(frame: &mut Frame, img: &Image, x: i64, y: i64) {
    for iy in 0..img.height {
        for ix in 0..img.width {
            if let Some(p) = img.pixels[(iy * img.width + ix) as usize] {
                frame.put(x + ix as i64, y + iy as i64, p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::{Caches, Treasure};

    /// A warship shows its life, fire and board power and experience; a
    /// boat its cargo places, its hero and treasure icons (needs
    /// `OPENCONQUEST_DATA`).
    #[test]
    fn ships_fill_their_info() {
        let Some(root) = std::env::var_os("OPENCONQUEST_DATA") else {
            return;
        };
        let data = GameData::open(std::path::Path::new(&root)).unwrap();
        let mut caches = Caches::default();
        let mut scene = Scene::load(&data, &mut caches, "miss000").unwrap();
        let warship = scene
            .objects
            .iter()
            .find(|o| o.ship.as_ref().is_some_and(|s| s.kind >= 2))
            .unwrap();
        // A bowman and a TRANS, whole, at level 0: against hulls the
        // TRANS's 40 x 15 = 600; against crews the bowman's 75 x 45 / 5
        // and the TRANS's 600 x 80 / 40, 1875; both x 100 / 1500. Board:
        // the bowman's 100 x 35 x 15 / 1000 = 52, x 1000 / 1500.
        assert_eq!(
            warship.unit_conditions().map(|u| u.len()),
            [1, 0, 0, 1, 0, 0, 0, 0, 0]
        );
        let info = of(&scene, warship).unwrap();
        assert_eq!(
            (info.life, info.fire, info.board, info.experience),
            (Some(100), Some((40, 125)), Some(34), Some((0, 100)))
        );
        assert_eq!((info.cargo, info.volume, info.hero), (None, None, None));
        let i = scene
            .objects
            .iter()
            .position(|o| o.ship.as_ref().is_some_and(|s| s.kind == 0))
            .unwrap();
        let s = scene.objects[i].ship.as_mut().unwrap();
        s.cargo = 150.0;
        s.heroes = vec![4];
        s.treasures = vec![Treasure {
            kind: 10,
            param: 0,
            amount: 0,
        }];
        let info = of(&scene, &scene.objects[i]).unwrap();
        assert_eq!(info.cargo, Some((2, 4)));
        assert_eq!((info.hero, info.treasure), (Some(4), true));
        assert_eq!((info.fire, info.board, info.experience), (None, None, None));
        // Drawn: the cell, the hero at the right end less 67, the treasure
        // less 40; nothing shown leaves the background.
        let art = InfoArt::load(&data, '0');
        let img = art.render(&info).unwrap();
        let bg = art.render(&ObjectInfo::default()).unwrap();
        assert_eq!((img.width, img.height), (380, 20));
        let differs = |x0: u32, x1: u32| {
            (0..20u32).any(|y| {
                (x0..x1).any(|x| {
                    let k = (y * 380 + x) as usize;
                    img.pixels[k] != bg.pixels[k]
                })
            })
        };
        assert!(differs(0, 78) && !differs(156, 313));
        assert!(differs(313, 333) && differs(340, 360));
    }
}
