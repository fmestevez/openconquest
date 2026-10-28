//! Command hot keys, as the original's.
//!
//! Each command id `c` (1-0x81) has a key in the table at Config+0x154
//! (`[0x5ca67c]`, a word a command: `[0x5ca67a + 2c]`), the registry blob
//! `Config` that config.exe's Hotkey editor writes. A key is two DirectInput
//! scan codes: the key and a second one to hold with it (0 for none). The
//! defaults (0x501550 in ancient.exe, 0x4055ee in config.exe, which its
//! "Default keys" button calls) are single letters. When the command panel
//! makes its buttons (0x4f2750), each slot whose command has a key gets an
//! accelerator (0x4f2140; a slot with the red cross gets Escape) that
//! presses the button, as a click on it. The input loop (Sinput.cpp
//! 0x553090) fires a key without a second one only while no Shift, Ctrl or
//! Alt is held, so Ctrl+S saves rather than stops.

use std::collections::BTreeMap;

use winit::keyboard::KeyCode;

use crate::commands::{Command, Menu};

/// Command ids with a table entry: 1 to 0x81.
pub const COMMANDS: usize = 0x81;

/// A command's key: a DirectInput scan code and a second one to hold with
/// it (0: none). `key` 0 is no key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Binding {
    pub key: u8,
    pub with: u8,
}

impl Binding {
    pub const NONE: Binding = Binding { key: 0, with: 0 };
    const fn key(key: u8) -> Self {
        Binding { key, with: 0 }
    }
}

/// The default keys (0x501550; config.exe 0x4055ee), by command id - 1.
pub const DEFAULTS: [Binding; COMMANDS] = defaults();

const fn defaults() -> [Binding; COMMANDS] {
    const S: u8 = 0x1f;
    const F: u8 = 0x21;
    const D: u8 = 0x20;
    const R: u8 = 0x13;
    let mut t = [Binding::NONE; COMMANDS];
    let keys: [(usize, u8); 63] = [
        (0x01, S),    // Stop
        (0x02, S),    // Break (Stop)
        (0x03, F),    // Fire
        (0x04, 0x32), // Move: M
        (0x05, 0x26), // Load/unload ship: L
        (0x06, 0x19), // Repair ship at dock: P
        (0x07, D),    // Build ground object (a ship's)
        (0x08, 0x14), // Take treasure or hero: T
        (0x09, 0x30), // Board: B
        (0x0a, R),    // Ram
        (0x0b, 0x16), // Unload resource: U
        (0x0c, R),    // Gather resources within area
        (0x0d, F),    // Fishing
        (0x0e, 0x1e), // Gather amber: A
        (0x0f, 0x2f), // Hunt for leviathan: V
        (0x10, D),    // Build ground object (a settlement's)
        (0x11, R),    // Repair building
        (0x12, 0x12), // Sell item: E
        (0x13, D),    // Build ship
        (0x14, R),    // Repair building (a fort's)
        (0x15, R),    // Repair arming in workshop
        (0x16, 0x2e), // Build settlement: C
        (0x18, F),    // Build medium fortress
        (0x1a, 0x32), // Build port: M
        (0x1b, D),    // Build shipyard
        (0x1c, 0x14), // Build workshop: T
        (0x1d, S),    // Build smithy
        (0x1e, 0x2e), // Build sage school: C
        (0x1f, 0x11), // Build temple of the sea: W
        (0x20, 0x1e), // Build temple of war: A
        (0x21, 0x30), // Build boat: B
        (0x22, 0x14), // Build merchant ship: T
        (0x23, 0x32), // Build menkiller: M
        (0x24, S),    // Build shipkiller
        (0x25, D),    // Build boarder
        (0x26, R),    // Build rammer
        (0x27, 0x18), // Build bomber: O
        (0x28, 0x25), // Build royal ship: K
        (0x29, F),    // Build Greek fire ship
        (0x2a, 0x10), // Build quicklime ship: Q
        (0x2b, F),    // Build fire rammer
        (0x2c, D),    // Build ship of the dead
        (0x2d, 0x30), // Train bowmen: B
        (0x2e, 0x24), // Train javelinmen: J
        (0x2f, S),    // Train swordsmen
        (0x30, 0x14), // Scorpion: T
        (0x31, 0x22), // Gastrafetes: G
        (0x32, 0x2e), // Small catapult: C
        (0x33, 0x30), // Ballista: B
        (0x34, 0x17), // Big catapult: I
        (0x35, 0x18), // Onager: O
        (0x3d, 0x1e), // Upgrade arming (weapons): A
        (0x47, 0x32), // Upgrade armour: M
        (0x48, R),    // Upgrade ram
        (0x4f, S),    // Upgrade speed
        (0x50, F),    // Invent Greek fire
        (0x51, 0x10), // Invent quicklime: Q
        (0x52, F),    // Invent fire rammer
        (0x53, D),    // Invent ship of the dead
        (0x54, 0x22), // Learn new magic: G
        (0x6c, 0x2e), // Behaviour: C
        (0x7d, 0x1e), // Build the Argo: A
        (0x81, 0x01), // (Escape)
    ];
    let mut i = 0;
    while i < keys.len() {
        let (command, key) = keys[i];
        t[command - 1] = Binding::key(key);
        i += 1;
    }
    t
}

/// Scan code `0x01` (Escape): the key of a slot with the red cross.
pub const ESCAPE: u8 = 0x01;

/// The keys of every command: the defaults, with the player's changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hotkeys {
    keys: [Binding; COMMANDS],
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self { keys: DEFAULTS }
    }
}

impl Hotkeys {
    /// The defaults changed by `overrides`: command id (`"0x04"` or `"4"`)
    /// to key (`"M"`, `"Shift+M"`, or `""` for none). Unknown entries are
    /// left out.
    pub fn with_overrides(overrides: &BTreeMap<String, String>) -> Self {
        let mut keys = DEFAULTS;
        for (command, key) in overrides {
            let id = command
                .strip_prefix("0x")
                .map_or_else(|| command.parse().ok(), |h| u8::from_str_radix(h, 16).ok());
            let (Some(id), Some(binding)) = (id, parse_binding(key)) else {
                continue;
            };
            if (1..=COMMANDS).contains(&(id as usize)) {
                keys[id as usize - 1] = binding;
            }
        }
        Self { keys }
    }

    /// The key of a command (0x4f2140: `[0x5ca67a + 2 x command]`).
    pub fn binding(&self, command: Command) -> Binding {
        match command.0 as usize {
            c @ 1..=COMMANDS => self.keys[c - 1],
            _ => Binding::NONE,
        }
    }

    /// The key of a command slot: Escape on a slot with the red cross.
    pub fn slot_binding(&self, slot: &crate::commands::Slot) -> Binding {
        if slot.closes {
            Binding::key(ESCAPE)
        } else {
            self.binding(slot.command)
        }
    }

    /// The slots of `menu` whose key `pressed` is (with `held` telling
    /// which keys are down): every enabled one, in order, as the original
    /// fires each accelerator of the key (Sinput.cpp 0x553090 goes through
    /// them all); a greyed one only when none is enabled (its refusal).
    pub fn slots_for(
        &self,
        menu: &Menu,
        pressed: KeyCode,
        held: &dyn Fn(u8) -> bool,
    ) -> Vec<usize> {
        let matching: Vec<(usize, bool)> = menu
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let s = s.as_ref()?;
                fires(self.slot_binding(s), pressed, held).then_some((i, s.enabled))
            })
            .collect();
        let enabled: Vec<usize> = matching.iter().filter(|m| m.1).map(|m| m.0).collect();
        if enabled.is_empty() {
            matching.first().map(|m| m.0).into_iter().collect()
        } else {
            enabled
        }
    }

    /// The first of `slots_for`.
    #[cfg(test)]
    fn slot_for(&self, menu: &Menu, pressed: KeyCode, held: &dyn Fn(u8) -> bool) -> Option<usize> {
        self.slots_for(menu, pressed, held).first().copied()
    }

    /// What the button's help line adds: ` - M` (0x505e70: `%s - &0%s`
    /// or `%s - &0%s-%s`, the key in the highlight colour).
    pub fn suffix(&self, slot: &crate::commands::Slot) -> String {
        let b = self.slot_binding(slot);
        match (key_name(b.key), key_name(b.with)) {
            (Some(k), Some(w)) => format!(" - {w}-{k}"),
            (Some(k), None) | (None, Some(k)) => format!(" - {k}"),
            (None, None) => String::new(),
        }
    }
}

/// Whether pressing `pressed` fires `binding` (Sinput.cpp 0x553090): the
/// key itself, and the second key held; with no second key, no Shift, Ctrl
/// or Alt held unless the key is one of them.
pub fn fires(binding: Binding, pressed: KeyCode, held: &dyn Fn(u8) -> bool) -> bool {
    if binding.key == 0 || scan_code(pressed) != Some(binding.key) {
        return false;
    }
    if binding.with != 0 {
        return held(binding.with);
    }
    is_modifier(binding.key) || !MODIFIERS.iter().any(|&m| held(m))
}

/// Shift, Ctrl and Alt, left and right.
const MODIFIERS: [u8; 6] = [0x2a, 0x36, 0x1d, 0x9d, 0x38, 0xb8];

fn is_modifier(scan: u8) -> bool {
    MODIFIERS.contains(&scan)
}

/// Scan codes and keys, with the original's names (the table at 0x5bf598,
/// 20 bytes a scan code).
const KEYS: [(u8, KeyCode, &str); 99] = {
    use KeyCode::*;
    [
        (0x01, Escape, "Escape"),
        (0x02, Digit1, "1"),
        (0x03, Digit2, "2"),
        (0x04, Digit3, "3"),
        (0x05, Digit4, "4"),
        (0x06, Digit5, "5"),
        (0x07, Digit6, "6"),
        (0x08, Digit7, "7"),
        (0x09, Digit8, "8"),
        (0x0a, Digit9, "9"),
        (0x0b, Digit0, "0"),
        (0x0c, Minus, "-"),
        (0x0d, Equal, "="),
        (0x0e, Backspace, "Backspace"),
        (0x0f, Tab, "Tab"),
        (0x10, KeyQ, "Q"),
        (0x11, KeyW, "W"),
        (0x12, KeyE, "E"),
        (0x13, KeyR, "R"),
        (0x14, KeyT, "T"),
        (0x15, KeyY, "Y"),
        (0x16, KeyU, "U"),
        (0x17, KeyI, "I"),
        (0x18, KeyO, "O"),
        (0x19, KeyP, "P"),
        (0x1a, BracketLeft, "["),
        (0x1b, BracketRight, "]"),
        (0x1c, Enter, "Enter"),
        (0x1d, ControlLeft, "Ctrl"),
        (0x1e, KeyA, "A"),
        (0x1f, KeyS, "S"),
        (0x20, KeyD, "D"),
        (0x21, KeyF, "F"),
        (0x22, KeyG, "G"),
        (0x23, KeyH, "H"),
        (0x24, KeyJ, "J"),
        (0x25, KeyK, "K"),
        (0x26, KeyL, "L"),
        (0x27, Semicolon, ";"),
        (0x28, Quote, "'"),
        (0x29, Backquote, "`"),
        (0x2a, ShiftLeft, "Shift"),
        (0x2b, Backslash, "\\"),
        (0x2c, KeyZ, "Z"),
        (0x2d, KeyX, "X"),
        (0x2e, KeyC, "C"),
        (0x2f, KeyV, "V"),
        (0x30, KeyB, "B"),
        (0x31, KeyN, "N"),
        (0x32, KeyM, "M"),
        (0x33, Comma, ","),
        (0x34, Period, "."),
        (0x35, Slash, "/"),
        (0x36, ShiftRight, "Shift"),
        (0x37, NumpadMultiply, "*"),
        (0x38, AltLeft, "Alt"),
        (0x39, Space, "Space"),
        (0x3a, CapsLock, "Caps Lock"),
        (0x3b, F1, "F1"),
        (0x3c, F2, "F2"),
        (0x3d, F3, "F3"),
        (0x3e, F4, "F4"),
        (0x3f, F5, "F5"),
        (0x40, F6, "F6"),
        (0x41, F7, "F7"),
        (0x42, F8, "F8"),
        (0x43, F9, "F9"),
        (0x44, F10, "F10"),
        (0x45, NumLock, "Num Lock"),
        (0x46, ScrollLock, "Scroll Lock"),
        (0x47, Numpad7, "7"),
        (0x48, Numpad8, "8"),
        (0x49, Numpad9, "9"),
        (0x4a, NumpadSubtract, "-"),
        (0x4b, Numpad4, "4"),
        (0x4c, Numpad5, "5"),
        (0x4d, Numpad6, "6"),
        (0x4e, NumpadAdd, "+"),
        (0x4f, Numpad1, "1"),
        (0x50, Numpad2, "2"),
        (0x51, Numpad3, "3"),
        (0x52, Numpad0, "0"),
        (0x53, NumpadDecimal, "."),
        (0x57, F11, "F11"),
        (0x58, F12, "F12"),
        (0x9c, NumpadEnter, "Enter"),
        (0x9d, ControlRight, "Ctrl"),
        (0xb5, NumpadDivide, "/"),
        (0xb8, AltRight, "Alt"),
        (0xc7, Home, "Home"),
        (0xc8, ArrowUp, "Up"),
        (0xc9, PageUp, "Page Up"),
        (0xcb, ArrowLeft, "Left"),
        (0xcd, ArrowRight, "Right"),
        (0xcf, End, "End"),
        (0xd0, ArrowDown, "Down"),
        (0xd1, PageDown, "Page Down"),
        (0xd2, Insert, "Ins"),
        (0xd3, Delete, "Del"),
    ]
};

/// The DirectInput scan code of a key.
pub fn scan_code(key: KeyCode) -> Option<u8> {
    KEYS.iter().find(|k| k.1 == key).map(|k| k.0)
}

/// The key of a DirectInput scan code.
pub fn key_code(scan: u8) -> Option<KeyCode> {
    KEYS.iter().find(|k| k.0 == scan).map(|k| k.1)
}

/// The original's name of a scan code.
pub fn key_name(scan: u8) -> Option<&'static str> {
    KEYS.iter().find(|k| k.0 == scan && scan != 0).map(|k| k.2)
}

/// `"M"`, `"Shift+M"` (or `"Shift-M"`), `""` for none.
fn parse_binding(text: &str) -> Option<Binding> {
    let text = text.trim();
    if text.is_empty() {
        return Some(Binding::NONE);
    }
    let scan = |name: &str| {
        let name = name.trim();
        KEYS.iter()
            .find(|k| k.2.eq_ignore_ascii_case(name))
            .map(|k| k.0)
    };
    match text
        .rsplit_once(['+', '-'])
        .filter(|(w, k)| !w.is_empty() && !k.is_empty())
    {
        Some((with, key)) => Some(Binding {
            key: scan(key)?,
            with: scan(with)?,
        }),
        None => Some(Binding::key(scan(text)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{Slot, SLOTS};

    fn slot(command: u8, enabled: bool) -> Option<Slot> {
        Some(Slot {
            command: Command(command),
            enabled,
            progress: None,
            count: 0,
            checked: false,
            closes: false,
        })
    }

    fn none_held(_: u8) -> bool {
        false
    }

    #[test]
    fn defaults_are_the_originals() {
        let h = Hotkeys::default();
        let name = |c: u8| key_name(h.binding(Command(c)).key).unwrap_or("");
        assert_eq!(name(0x01), "S");
        assert_eq!(name(0x03), "F");
        assert_eq!(name(0x04), "M");
        assert_eq!(name(0x05), "L");
        assert_eq!(name(0x06), "P");
        assert_eq!(name(0x08), "T");
        assert_eq!(name(0x09), "B");
        assert_eq!(name(0x0a), "R");
        assert_eq!(name(0x0b), "U");
        assert_eq!(name(0x0e), "A");
        assert_eq!(name(0x0f), "V");
        assert_eq!(name(0x16), "C");
        assert_eq!(name(0x17), "");
        assert_eq!(name(0x18), "F");
        assert_eq!(name(0x2e), "J");
        assert_eq!(name(0x6c), "C");
        assert_eq!(name(0x6d), "");
        assert_eq!(name(0x7d), "A");
        assert_eq!(name(0x80), "");
        assert_eq!(name(0x81), "Escape");
        // 63 commands have a key, all of them a single one.
        assert_eq!(DEFAULTS.iter().filter(|b| b.key != 0).count(), 63);
        assert!(DEFAULTS.iter().all(|b| b.with == 0));
    }

    #[test]
    fn a_key_presses_its_button() {
        let h = Hotkeys::default();
        let mut menu: Menu = [None; SLOTS];
        menu[0] = slot(0x04, true); // Move
        menu[1] = slot(0x01, true); // Stop
        menu[4] = slot(0x03, true); // Fire
        menu[5] = slot(0x09, false); // Board, greyed
        assert_eq!(h.slot_for(&menu, KeyCode::KeyM, &none_held), Some(0));
        assert_eq!(h.slot_for(&menu, KeyCode::KeyS, &none_held), Some(1));
        assert_eq!(h.slot_for(&menu, KeyCode::KeyF, &none_held), Some(4));
        // A greyed button's key still finds it (pressing it says why not).
        assert_eq!(h.slot_for(&menu, KeyCode::KeyB, &none_held), Some(5));
        assert_eq!(h.slot_for(&menu, KeyCode::KeyX, &none_held), None);
    }

    #[test]
    fn modifiers_keep_their_own_keys() {
        let h = Hotkeys::default();
        let mut menu: Menu = [None; SLOTS];
        menu[1] = slot(0x01, true); // Stop: S
        menu[9] = slot(0x06, true); // Repair: P
        let ctrl = |s: u8| s == 0x1d;
        let alt = |s: u8| s == 0x38;
        // Ctrl+S saves, Alt+P pauses.
        assert_eq!(h.slot_for(&menu, KeyCode::KeyS, &ctrl), None);
        assert_eq!(h.slot_for(&menu, KeyCode::KeyP, &alt), None);
        assert_eq!(h.slot_for(&menu, KeyCode::KeyP, &none_held), Some(9));
    }

    #[test]
    fn the_cross_is_escape() {
        let h = Hotkeys::default();
        let mut menu: Menu = [None; SLOTS];
        menu[0] = slot(0x1a, true); // Build port: M
        let mut back = slot(0x10, true).unwrap();
        back.closes = true;
        menu[11] = Some(back);
        assert_eq!(h.slot_for(&menu, KeyCode::Escape, &none_held), Some(11));
        // The cross's own command's key (D) does not press it.
        assert_eq!(h.slot_for(&menu, KeyCode::KeyD, &none_held), None);
        assert_eq!(h.slot_for(&menu, KeyCode::KeyM, &none_held), Some(0));
        assert_eq!(h.suffix(&back), " - Escape");
    }

    #[test]
    fn duplicates_press_every_enabled_one() {
        let h = Hotkeys::default();
        let mut menu: Menu = [None; SLOTS];
        menu[2] = slot(0x0a, false); // Ram: R, greyed
        menu[6] = slot(0x0c, true); // Gather zones: R
        assert_eq!(h.slots_for(&menu, KeyCode::KeyR, &none_held), vec![6]);
        menu[2] = slot(0x0a, true);
        assert_eq!(h.slots_for(&menu, KeyCode::KeyR, &none_held), vec![2, 6]);
        menu[2] = slot(0x0a, false);
        menu[6] = slot(0x0c, false);
        assert_eq!(h.slots_for(&menu, KeyCode::KeyR, &none_held), vec![2]);
    }

    #[test]
    fn overrides() {
        let mut o = BTreeMap::new();
        o.insert("0x04".to_string(), "G".to_string());
        o.insert("1".to_string(), "Shift+X".to_string());
        o.insert("3".to_string(), String::new());
        o.insert("0x200".to_string(), "Q".to_string());
        o.insert("5".to_string(), "Nonsense".to_string());
        let h = Hotkeys::with_overrides(&o);
        assert_eq!(h.binding(Command::MOVE), Binding::key(0x22));
        assert_eq!(
            h.binding(Command::STOP),
            Binding {
                key: 0x2d,
                with: 0x2a
            }
        );
        assert_eq!(h.binding(Command::FIRE), Binding::NONE);
        assert_eq!(h.binding(Command::LOAD), DEFAULTS[4]);
        let shift = |s: u8| s == 0x2a;
        assert!(fires(h.binding(Command::STOP), KeyCode::KeyX, &shift));
        assert!(!fires(h.binding(Command::STOP), KeyCode::KeyX, &none_held));
        assert_eq!(h.suffix(&slot(0x01, true).unwrap()), " - Shift-X");
        assert_eq!(h.suffix(&slot(0x04, true).unwrap()), " - G");
    }

    #[test]
    fn scan_codes_round_trip() {
        for &(scan, key, _) in KEYS.iter() {
            assert_eq!(scan_code(key), Some(scan));
            assert_eq!(key_code(scan), Some(key));
        }
    }
}
