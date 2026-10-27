//! The original's mouse pointers (gcursor.cpp): `CUR_<name>_<N|G><frame>`
//! sprites in `controlm.dkx`, four frames each, `N` for an action that can
//! be done where the pointer is and `G` (greyed) for one that cannot. The
//! game picks one by what the selected units would do to what is under the
//! pointer (0x5175b0) and draws it at the mouse position plus an offset per
//! pointer (0x516ea0).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::assets::{GameData, Image};
use crate::render::{blit_scaled, Frame};

/// Milliseconds per frame of a pointer's animation (its frame delay, 100,
/// gcursor.cpp; pointers run on the clock, not the game's ticks).
const FRAME_MS: u64 = 100;

/// The pointers, with their names and draw offsets from the mouse
/// position (0x516ea0, fields +0xd1, +0xd5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pointer {
    Standard,
    Fish,
    Whale,
    Amber,
    Own,
    Select,
    Sea,
    Attack,
    Board,
    Ram,
    Build,
    Repair,
    Magic,
    Treasure,
    Behaviour,
    Ban,
    Load,
    Unload,
    /// The leviathan hunt waiting for its target (mode 3).
    GetWhale,
    /// Over a quick help hyperlink (`CUR_HTEXT_`, never greyed).
    Hypertext,
}

impl Pointer {
    fn sprite(self) -> (&'static str, (i32, i32)) {
        use Pointer::*;
        match self {
            Standard => ("CUR_STD_", (0, 0)),
            Fish => ("CUR_FISH_", (-19, -10)),
            Whale => ("CUR_WW_", (-21, -21)),
            Amber => ("CUR_AMBER_", (-21, -26)),
            Own => ("CUR_OWN_", (0, 0)),
            Select => ("CUR_SELECT_", (0, 0)),
            Sea => ("CUR_SEA_", (-18, -17)),
            Attack => ("CUR_ATAK_", (-24, -25)),
            Board => ("CUR_BOARD_", (-25, -26)),
            Ram => ("CUR_RAM_", (-20, -10)),
            Build => ("CUR_BUILD_", (-21, -23)),
            Repair => ("CUR_REPAIR_", (-19, -18)),
            Magic => ("CUR_MAGIC_", (-17, -23)),
            Treasure => ("CUR_TREASURE_", (-19, -21)),
            Behaviour => ("CUR_BEH_", (-17, -17)),
            Ban => ("CUR_BAN_", (-19, -10)),
            Load => ("CUR_LOAD_", (-32, -37)),
            Unload => ("CUR_UNLOAD_", (-18, -29)),
            GetWhale => ("CUR_GETW_", (-15, -15)),
            Hypertext => ("CUR_HTEXT_", (-10, -3)),
        }
    }
}

/// A pointer's frames, by pointer and whether greyed.
type FrameCache = HashMap<(Pointer, bool), Rc<Vec<Image>>>;

/// The pointers' frames, loaded as they are first shown.
#[derive(Default)]
pub struct Pointers {
    frames: RefCell<FrameCache>,
}

impl Pointers {
    fn frames(&self, data: &GameData, pointer: Pointer, greyed: bool) -> Rc<Vec<Image>> {
        self.frames
            .borrow_mut()
            .entry((pointer, greyed))
            .or_insert_with(|| {
                let Some(lib) = &data.menus else {
                    return Rc::new(Vec::new());
                };
                let (name, _) = pointer.sprite();
                let state = match (pointer, greyed) {
                    (Pointer::Hypertext, _) => String::new(),
                    (_, true) => "G".into(),
                    _ => "N".into(),
                };
                // Two-digit frames, or (the plainer ones) a single digit.
                let mut frames: Vec<Image> = (0..)
                    .map_while(|i| {
                        data.load_image(lib, &format!("{name}{state}{i:02}"), false)
                            .ok()
                            .flatten()
                    })
                    .collect();
                if frames.is_empty() {
                    frames = (0..)
                        .map_while(|i| {
                            data.load_image(lib, &format!("{name}{state}{i}"), false)
                                .ok()
                                .flatten()
                        })
                        .collect();
                }
                Rc::new(frames)
            })
            .clone()
    }

    /// How many frames pointer `pointer` has.
    #[cfg(test)]
    pub fn count(&self, data: &GameData, pointer: Pointer, greyed: bool) -> usize {
        self.frames(data, pointer, greyed).len()
    }

    /// Whether the game data has the pointers.
    pub fn available(&self, data: &GameData) -> bool {
        !self.frames(data, Pointer::Standard, false).is_empty()
    }

    /// Draws `pointer` with the mouse at window position `(x, y)`, sprite
    /// pixels `scale` window pixels.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        frame: &mut Frame,
        data: &GameData,
        pointer: Pointer,
        greyed: bool,
        x: f32,
        y: f32,
        scale: f32,
        time_ms: u64,
    ) {
        let mut frames = self.frames(data, pointer, greyed);
        if frames.is_empty() {
            frames = self.frames(data, Pointer::Standard, false);
        }
        if frames.is_empty() {
            return;
        }
        let img = &frames[(time_ms / FRAME_MS) as usize % frames.len()];
        let (_, (dx, dy)) = pointer.sprite();
        blit_scaled(
            frame,
            img,
            (0, 0, img.width, img.height),
            x + dx as f32 * scale,
            y + dy as f32 * scale,
            scale,
        );
    }
}
