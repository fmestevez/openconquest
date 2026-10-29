//! The Best Conquerors table: seven rows for each of the four campaign
//! types (0x5c9ed0, 0x540 bytes: rows of 0x30, a name of 0x28 bytes, the
//! score at + 0x28 and the game's id at + 0x2c). The original keeps it in
//! the registry (`...\Ancient conquest\Config`, value `Result`, packed,
//! checked by `CRCResult`; loaded at start-up by 0x500810, written at
//! exit by 0x501090 from 0x4ee6c0); the port keeps it in its settings
//! file (`launch.rs`), read at start-up and written when it exits.

/// Rows a campaign type keeps, and the types (0x5c9ed0: 4 x 7 rows).
pub const ROWS: usize = 7;
pub const TYPES: usize = 4;
/// A name's room, its terminating NUL included (0x5291e0 with 0x28).
pub const NAME_SIZE: usize = 0x28;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Row {
    pub name: String,
    pub score: i32,
    /// The play-through it came from (game + 0x1a0d, see `game_id`).
    pub id: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Table {
    pub types: [[Row; ROWS]; TYPES],
}

/// `name` cut to what a row holds (0x5291e0, limit 0x28: 39 characters;
/// the original's are bytes of the ANSI code page, here characters).
pub fn row_name(name: &str) -> String {
    name.chars().take(NAME_SIZE - 1).collect()
}

impl Table {
    /// The rows of campaign type `t`.
    pub fn rows(&self, t: usize) -> &[Row; ROWS] {
        &self.types[t.min(TYPES - 1)]
    }

    /// Enters a won campaign mission's total `score` of play-through `id`
    /// in type `t`'s rows under `name` (Startrpt.cpp 0x5397e0); the row
    /// it took. Nothing when a row already holds the same score and id (it
    /// was entered before); else it goes in the first row whose score it
    /// reaches (empty rows have 0), the rows below move down one and the
    /// last drops off; nothing when it reaches none.
    pub fn enter(&mut self, t: usize, score: i32, id: u32, name: &str) -> Option<usize> {
        let rows = self.types.get_mut(t)?;
        if rows.iter().any(|r| r.score == score && r.id == id) {
            return None;
        }
        let i = rows.iter().position(|r| score >= r.score)?;
        rows[i..].rotate_right(1);
        rows[i] = Row {
            name: row_name(name),
            score,
            id,
        };
        Some(i)
    }

    /// Names row `i` of type `t` (the end screen's edit, 0x175 and OK).
    pub fn rename(&mut self, t: usize, i: usize, name: &str) {
        if let Some(r) = self.types.get_mut(t).and_then(|rows| rows.get_mut(i)) {
            r.name = row_name(name);
        }
    }
}

/// The campaign type of a mask of campaigns (0x5291b0): its lowest bit of
/// the low byte, none without one.
pub fn lowest_bit(mask: u32) -> Option<usize> {
    (0..8).find(|&i| mask & 0xff & (1 << i) != 0)
}

/// What the installer took from the CD's `ID_CD.DSK` (setup.exe 0x404cc0
/// reads its first 4 bytes, then up to 0x104 of name): the registry's
/// `Version` low byte ORs in its first byte (0x4045a8; game + 0xb8f, the
/// campaigns installed) and `CD<k+1>` gets the name, k its lowest bit
/// (0x4038b0, 0x4045ae). Read from the installation's copy of the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Campaigns {
    pub mask: u8,
    pub names: [String; TYPES],
}

impl Campaigns {
    pub fn read(root: &std::path::Path) -> Self {
        let file = openconquest_formats::install::find_ci(root, "id_cd.dsk")
            .ok()
            .flatten()
            .and_then(|p| std::fs::read(p).ok());
        file.map(|b| Self::parse(&b)).unwrap_or_default()
    }

    pub fn parse(bytes: &[u8]) -> Self {
        let mut c = Self::default();
        let (Some(&mask), Some(text)) = (bytes.first(), bytes.get(4..)) else {
            return c;
        };
        c.mask = mask;
        let text = &text[..text.len().min(0x104)];
        let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
        if let Some(k) = lowest_bit(u32::from(mask)).filter(|&k| k < TYPES) {
            c.names[k] = text[..end].iter().map(|&b| char::from(b)).collect();
        }
        c
    }

    /// Whether type `t`'s bit is set (game + 0xb8f).
    pub fn has(&self, t: usize) -> bool {
        t < 8 && self.mask & (1 << t) != 0
    }

    /// Best Conquerors' title for type `t` (Startba.cpp 0x528520): the
    /// campaign's name if its bit is set and it has one, else string 1038
    /// + t.
    pub fn title(&self, t: usize, strings: &std::collections::HashMap<u32, String>) -> String {
        match self.names.get(t).filter(|n| self.has(t) && !n.is_empty()) {
            Some(n) => n.clone(),
            None => strings.get(&(1038 + t as u32)).cloned().unwrap_or_default(),
        }
    }
}

/// A new play-through's id when the file loaded has none (0x4ed3f0: the
/// low dword of the local time as a FILETIME, 100 ns from 1601, the
/// clock's milliseconds as SYSTEMTIME keeps them).
pub fn game_id() -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let ms = now.as_millis() as i64 + local_offset_s(now.as_secs() as i64) * 1000;
    // 1601-01-01 to 1970-01-01.
    let filetime = (ms + 11_644_473_600_000) * 10_000;
    filetime as u64 as u32
}

/// The local time's offset from UTC at `t` (seconds since 1970).
#[cfg(unix)]
fn local_offset_s(t: i64) -> i64 {
    let t = t as libc::time_t;
    // SAFETY: localtime_r only writes the struct given.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return 0;
        }
        tm.tm_gmtoff as i64
    }
}

/// Elsewhere the port has no local time zone: UTC.
#[cfg(not(unix))]
fn local_offset_s(_t: i64) -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_go_in_as_the_original_enters_them() {
        let mut t = Table::default();
        // Empty rows have 0: any score takes the first.
        assert_eq!(t.enter(0, 500, 1, "Jason"), Some(0));
        // The same score of the same play-through: already entered.
        assert_eq!(t.enter(0, 500, 1, "Jason"), None);
        // Higher goes above, the rest move down.
        assert_eq!(t.enter(0, 900, 2, "Medea"), Some(0));
        assert_eq!(t.rows(0)[1].name, "Jason");
        // A tie goes above the older equal row.
        assert_eq!(t.enter(0, 500, 3, "Argus"), Some(1));
        assert_eq!(t.rows(0)[2].id, 1);
        for k in 0..4 {
            t.enter(0, 1000 + k, 10 + k as u32, "x");
        }
        // Seven rows: the last is the oldest of the lowest, and drops off
        // next.
        assert_eq!(t.rows(0)[6].name, "Jason");
        assert_eq!(t.enter(0, 2000, 50, "z"), Some(0));
        assert!(t.rows(0).iter().all(|r| r.id != 1));
        assert_eq!(t.rows(0)[6].name, "Argus");
        // Lower than every full row: not entered.
        assert_eq!(t.enter(0, 10, 99, "y"), None);
        // Names keep 39 characters.
        t.rename(0, 0, &"n".repeat(50));
        assert_eq!(t.rows(0)[0].name.len(), 39);
    }

    #[test]
    fn the_cd_names_its_campaign() {
        // The CD's ID_CD.DSK: 01 88 f0 de, then the name.
        let mut bytes = vec![0x01, 0x88, 0xf0, 0xde];
        bytes.extend_from_slice(b"The Golden Fleece Campaign\0\0\0\0\0\0\0\0");
        let c = Campaigns::parse(&bytes);
        assert_eq!(c.mask, 1);
        assert!(c.has(0) && !c.has(1));
        let mut strings = std::collections::HashMap::new();
        strings.insert(1039, " ".to_string());
        assert_eq!(c.title(0, &strings), "The Golden Fleece Campaign");
        assert_eq!(c.title(1, &strings), " ");
        assert_eq!(lowest_bit(0), None);
        assert_eq!(lowest_bit(0x104), Some(2));
    }
}
