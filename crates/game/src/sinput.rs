//! The front end's keyboard (Ourlib's Sinput.cpp): a table of key
//! bindings polled from the keyboard's state each frame (0x553090,
//! DirectInput `GetDeviceState`, so keys are scan codes, `DIK_*`).
//!
//! An entry has two keys, a priority (the table is kept in its order) and
//! a message sent when it goes down and another when it comes up. With
//! one key it goes down when that key does, unless Shift, Ctrl or Alt is
//! held (or the key is one of them); with two, when both are down. It
//! comes up when the first key does. An entry made while its keys are down
//! waits for them to come up first (0x553670). An exclusive entry turns
//! off the entries of lower priority with the same keys while it is there
//! (0x553670, 0x553930). Entries can be suspended all at once (messages
//! 0x18 and 0x19, a count each, + 0x5e); a suspended entry is passed over
//! by the poll, and one made next to a suspended entry of its priority
//! starts with that entry's count (0x553670 copies the + 0x5e of the entry
//! before it when their priorities match).
//!
//! A repeating entry (flag 1) sends its press again while its keys stay
//! down, after Sinput's + 0x58 then every + 0x5c ms (0x553090). Those are
//! the host's keyboard settings (0x552cc0: (SPI_GETKEYBOARDDELAY + 1) x
//! 250 ms and 1000 / (SPI_GETKEYBOARDSPEED + 1) ms), not numbers of the
//! game; the port takes the host's own repeat, the system's repeated key
//! events.

use winit::keyboard::KeyCode;

/// Scan codes (`DIK_*`) the front end binds.
pub const ESCAPE: u8 = 0x01;
pub const ENTER: u8 = 0x1c;
pub const SPACE: u8 = 0x39;
pub const LALT: u8 = 0x38;
const RALT: u8 = 0xb8;
const LSHIFT: u8 = 0x2a;
const RSHIFT: u8 = 0x36;
const LCTRL: u8 = 0x1d;
const RCTRL: u8 = 0x9d;

/// The scan code of a key (US layout positions, as winit's physical keys
/// are).
pub fn scan_code(code: KeyCode) -> Option<u8> {
    use KeyCode::*;
    Some(match code {
        Escape => 0x01,
        Digit1 => 0x02,
        Digit2 => 0x03,
        Digit3 => 0x04,
        Digit4 => 0x05,
        Digit5 => 0x06,
        Digit6 => 0x07,
        Digit7 => 0x08,
        Digit8 => 0x09,
        Digit9 => 0x0a,
        Digit0 => 0x0b,
        Minus => 0x0c,
        Equal => 0x0d,
        Backspace => 0x0e,
        Tab => 0x0f,
        KeyQ => 0x10,
        KeyW => 0x11,
        KeyE => 0x12,
        KeyR => 0x13,
        KeyT => 0x14,
        KeyY => 0x15,
        KeyU => 0x16,
        KeyI => 0x17,
        KeyO => 0x18,
        KeyP => 0x19,
        BracketLeft => 0x1a,
        BracketRight => 0x1b,
        Enter => 0x1c,
        ControlLeft => 0x1d,
        KeyA => 0x1e,
        KeyS => 0x1f,
        KeyD => 0x20,
        KeyF => 0x21,
        KeyG => 0x22,
        KeyH => 0x23,
        KeyJ => 0x24,
        KeyK => 0x25,
        KeyL => 0x26,
        Semicolon => 0x27,
        Quote => 0x28,
        Backquote => 0x29,
        ShiftLeft => 0x2a,
        Backslash => 0x2b,
        KeyZ => 0x2c,
        KeyX => 0x2d,
        KeyC => 0x2e,
        KeyV => 0x2f,
        KeyB => 0x30,
        KeyN => 0x31,
        KeyM => 0x32,
        Comma => 0x33,
        Period => 0x34,
        Slash => 0x35,
        ShiftRight => 0x36,
        AltLeft => 0x38,
        Space => 0x39,
        NumpadEnter => 0x9c,
        ControlRight => 0x9d,
        AltRight => 0xb8,
        Home => 0xc7,
        ArrowUp => 0xc8,
        ArrowLeft => 0xcb,
        ArrowRight => 0xcd,
        End => 0xcf,
        ArrowDown => 0xd0,
        Insert => 0xd2,
        Delete => 0xd3,
        _ => return None,
    })
}

/// Entry flags: 1 repeats while held, 2 exclusive.
pub const EXCLUSIVE: u8 = 2;

#[derive(Debug, Clone)]
struct Entry<M> {
    flags: u8,
    keys: (u8, u8),
    priority: u32,
    /// Sent when it goes down, and when it comes up.
    press: Option<M>,
    release: Option<M>,
    /// On (+0x5a), down (+0x4a), and waiting for its key to come up
    /// (+0x4e).
    on: bool,
    down: bool,
    waiting: bool,
    /// Suspended this many times (+0x5e).
    suspended: u32,
}

/// The table (InputClassTy + 0x24).
#[derive(Debug, Clone)]
pub struct Keys<M> {
    entries: Vec<Entry<M>>,
    /// Set when entries come or go: the poll stops (+0x60).
    changed: bool,
}

impl<M> Default for Keys<M> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            changed: false,
        }
    }
}

impl<M: Clone + PartialEq> Keys<M> {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.changed = true;
    }

    /// Adds a binding (message 0x10, 0x553670), with `held` telling which
    /// keys are down now.
    pub fn add(
        &mut self,
        flags: u8,
        keys: (u8, u8),
        priority: u32,
        press: Option<M>,
        release: Option<M>,
        held: &impl Fn(u8) -> bool,
    ) {
        self.changed = true;
        let waiting = held(keys.0) && (keys.1 == 0 || held(keys.1));
        // After the entries of its priority and lower.
        let at = self
            .entries
            .iter()
            .position(|e| priority < e.priority)
            .unwrap_or(self.entries.len());
        // An exclusive entry above with the same keys turns it off; the
        // search starts one past the first entry above (0x553807: `lea
        // esi, [edi + 1]`), which is not looked at.
        let on = !self
            .entries
            .iter()
            .skip(at + 1)
            .any(|e| e.flags & EXCLUSIVE != 0 && e.keys == keys);
        let suspended = at
            .checked_sub(1)
            .map(|k| &self.entries[k])
            .filter(|e| e.priority == priority)
            .map_or(0, |e| e.suspended);
        self.entries.insert(
            at,
            Entry {
                flags,
                keys,
                priority,
                press,
                release,
                on,
                down: false,
                waiting,
                suspended,
            },
        );
        // An exclusive one turns off those below it with the same keys,
        // down to the next exclusive one.
        if flags & EXCLUSIVE != 0 {
            for e in self.entries[..at].iter_mut().rev() {
                if e.keys != keys || !e.on {
                    continue;
                }
                e.on = false;
                e.down = false;
                if e.flags & EXCLUSIVE != 0 {
                    break;
                }
            }
        }
    }

    /// Removes the binding of `keys` that sends `press` (message 0x11,
    /// 0x553930); an exclusive one turns back on those it turned off, each
    /// waiting if its keys are down.
    pub fn remove(&mut self, keys: (u8, u8), press: &M, held: &impl Fn(u8) -> bool) {
        let Some(at) = self
            .entries
            .iter()
            .position(|e| e.keys == keys && e.press.as_ref() == Some(press))
        else {
            return;
        };
        self.changed = true;
        let gone = self.entries.remove(at);
        if gone.flags & EXCLUSIVE != 0 {
            for e in self.entries[..at].iter_mut().rev() {
                if e.keys != keys || e.on {
                    continue;
                }
                e.on = true;
                e.down = false;
                e.waiting = held(keys.0) && (keys.1 == 0 || held(keys.1));
                if e.flags & EXCLUSIVE != 0 {
                    break;
                }
            }
        }
    }

    /// Suspends every entry once more (message 0x18, 0x553ba0).
    pub fn suspend(&mut self) {
        self.changed = true;
        for e in &mut self.entries {
            e.suspended += 1;
        }
    }

    /// Takes one suspension off every suspended entry (message 0x19,
    /// 0x553be0).
    pub fn resume(&mut self) {
        self.changed = true;
        for e in &mut self.entries {
            e.suspended = e.suspended.saturating_sub(1);
        }
    }

    /// Starts a poll (0x553090).
    pub fn begin(&mut self) {
        self.changed = false;
    }

    /// Whether entries came or went since the poll began.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// Goes on with the poll from entry `*at`: the next message to send,
    /// if any. The caller stops when `changed`. `repeated` tells the keys
    /// the system repeated since the last poll.
    pub fn next(
        &mut self,
        held: &impl Fn(u8) -> bool,
        repeated: &impl Fn(u8) -> bool,
        at: &mut usize,
    ) -> Option<M> {
        let modifier = |k: u8| matches!(k, LSHIFT | RSHIFT | LALT | RALT | LCTRL | RCTRL);
        let none_held = !(held(LSHIFT)
            || held(RSHIFT)
            || held(LALT)
            || held(RALT)
            || held(LCTRL)
            || held(RCTRL));
        while let Some(e) = self.entries.get_mut(*at) {
            *at += 1;
            if !e.on || e.suspended != 0 || e.keys.0 == 0 {
                continue;
            }
            if !held(e.keys.0) {
                e.waiting = false;
                if e.down {
                    e.down = false;
                    if e.release.is_some() {
                        return e.release.clone();
                    }
                }
                continue;
            }
            let both = if e.keys.1 == 0 {
                modifier(e.keys.0) || none_held
            } else {
                held(e.keys.1)
            };
            if both && !e.waiting && !e.down {
                e.down = true;
                if e.press.is_some() {
                    return e.press.clone();
                }
            } else if both && e.down && e.flags & 1 != 0 && repeated(e.keys.0) {
                // Held, a repeating entry sends its press again.
                if e.press.is_some() {
                    return e.press.clone();
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll(keys: &mut Keys<u32>, down: &[u8]) -> Vec<u32> {
        let held = |k: u8| down.contains(&k);
        keys.begin();
        let mut at = 0;
        let mut out = Vec::new();
        while let Some(m) = keys.next(&held, &|_| false, &mut at) {
            out.push(m);
        }
        out
    }

    #[test]
    fn alt_and_a_letter_act_when_alt_comes_up() {
        let mut keys = Keys::default();
        keys.add(0, (LALT, 0x18), 0, Some(1), Some(2), &|_| false);
        assert_eq!(poll(&mut keys, &[LALT]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[LALT, 0x18]), vec![1]);
        // The letter alone up: nothing; again down: nothing.
        assert_eq!(poll(&mut keys, &[LALT]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[LALT, 0x18]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[0x18]), vec![2]);
        assert_eq!(poll(&mut keys, &[]), Vec::<u32>::new());
    }

    #[test]
    fn a_key_alone_waits_for_no_modifier() {
        let mut keys = Keys::default();
        keys.add(0, (ENTER, 0), 0, Some(7), None, &|_| false);
        assert_eq!(poll(&mut keys, &[ENTER, LSHIFT]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[ENTER]), vec![7]);
        assert_eq!(poll(&mut keys, &[ENTER]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[ENTER]), vec![7]);
    }

    #[test]
    fn a_key_held_when_bound_waits_to_come_up() {
        let mut keys = Keys::default();
        keys.add(0, (ESCAPE, 0), 0, Some(1), Some(2), &|k| k == ESCAPE);
        assert_eq!(poll(&mut keys, &[ESCAPE]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[ESCAPE]), vec![1]);
        assert_eq!(poll(&mut keys, &[]), vec![2]);
    }

    #[test]
    fn an_exclusive_binding_shadows_and_gives_back() {
        let mut keys = Keys::default();
        keys.add(0, (ENTER, 0), 0, Some(1), None, &|_| false);
        keys.add(EXCLUSIVE | 1, (ENTER, 0), 0xff, Some(9), None, &|_| false);
        assert_eq!(poll(&mut keys, &[ENTER]), vec![9]);
        keys.remove((ENTER, 0), &9, &|k| k == ENTER);
        // Back on, but Enter is down: it waits for a new press.
        assert_eq!(poll(&mut keys, &[ENTER]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[ENTER]), vec![1]);
    }

    /// Custom missions' kind pop-up (0x52ebe0): the screen's bindings are
    /// suspended, then its exclusive Esc made at their priority; it takes
    /// the suspension of the entry before it, so Esc does nothing, the
    /// Cancel button's Esc being turned off by it. Taken away and resumed,
    /// Cancel's Esc is back.
    #[test]
    fn an_entry_next_to_a_suspended_one_is_suspended() {
        let mut keys = Keys::default();
        keys.add(0, (LALT, 0x1f), 0, Some(1), Some(2), &|_| false);
        keys.add(0, (ESCAPE, 0), 0, Some(3), Some(4), &|_| false);
        keys.suspend();
        keys.add(EXCLUSIVE, (ESCAPE, 0), 0, Some(9), None, &|_| false);
        assert_eq!(poll(&mut keys, &[ESCAPE]), Vec::<u32>::new());
        assert_eq!(poll(&mut keys, &[]), Vec::<u32>::new());
        keys.remove((ESCAPE, 0), &9, &|_| false);
        keys.resume();
        assert_eq!(poll(&mut keys, &[ESCAPE]), vec![3]);
        assert_eq!(poll(&mut keys, &[]), vec![4]);
        // A higher priority is not next to them: it works.
        keys.suspend();
        keys.add(EXCLUSIVE, (ESCAPE, 0), 0xff, Some(9), None, &|_| false);
        assert_eq!(poll(&mut keys, &[ESCAPE]), vec![9]);
    }

    #[test]
    fn a_repeating_entry_sends_again_on_the_system_repeat() {
        let mut keys = Keys::default();
        keys.add(EXCLUSIVE | 1, (0xcb, 0), 0xff, Some(5), None, &|_| false);
        let held = |k: u8| k == 0xcb;
        keys.begin();
        let mut at = 0;
        assert_eq!(keys.next(&held, &|_| false, &mut at), Some(5));
        at = 0;
        assert_eq!(keys.next(&held, &|_| false, &mut at), None);
        at = 0;
        assert_eq!(keys.next(&held, &|k| k == 0xcb, &mut at), Some(5));
    }

    #[test]
    fn keys_are_scan_codes() {
        assert_eq!(scan_code(KeyCode::KeyB), Some(0x30));
        assert_eq!(scan_code(KeyCode::KeyQ), Some(0x10));
        assert_eq!(scan_code(KeyCode::Escape), Some(ESCAPE));
        assert_eq!(scan_code(KeyCode::NumpadEnter), Some(0x9c));
    }
}
