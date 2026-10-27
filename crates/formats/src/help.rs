//! Quick help pages (kind 24, `SPHELP_nnn` in `controlg.dkx`).
//!
//! A page is a list of 100-byte items that the help viewer (`Viewsys.cpp`,
//! 0x525f80) draws in order: pictures and animations placed on the bar, and
//! hyperlinks to other pages. The text is not text: each page's first item
//! is a 201x445 sprite (`HELP_nn`) with the words, titles and underlines
//! drawn in.

use crate::bytes::latin1_cstr;
use crate::{invalid, Result};

/// Item kind of a hyperlink (other kinds are resource kinds).
pub const LINK: u8 = 25;

/// A quick help page item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelpItem {
    /// A picture: resource `name` of `kind` (1 bitmap, 6 sprite, 7
    /// sequence) from archive `archive` (0 `controlg.dkx`, 1
    /// `controlm.dkx`), its top-left corner at `(x, y)`; a sequence steps
    /// through its `frames` every `delay` milliseconds.
    Picture {
        archive: u8,
        kind: u8,
        name: String,
        x: i16,
        y: i16,
        frames: u16,
        delay: u16,
    },
    /// A hyperlink: clicking in the rectangle opens page `target`.
    Link {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        target: u32,
    },
}

/// Parses a page: `+0` archive, `+1` kind, `+2` name (32 bytes), `+0x22`
/// x and `+0x24` y (i16), `+0x28` frame count and `+0x2a` frame delay
/// (u16); a link (kind 25) has its rectangle at `+0x36` (x, y, w, h, i32)
/// and its target page at `+0x46` (u32).
pub fn parse_help_page(data: &[u8]) -> Result<Vec<HelpItem>> {
    if data.len() % 100 != 0 {
        return Err(invalid("help page", format!("{} bytes", data.len())));
    }
    let i16_at = |r: &[u8], o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
    let u16_at = |r: &[u8], o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
    let i32_at = |r: &[u8], o: usize| i32::from_le_bytes([r[o], r[o + 1], r[o + 2], r[o + 3]]);
    Ok(data
        .chunks_exact(100)
        .map(|r| {
            if r[1] == LINK {
                HelpItem::Link {
                    x: i32_at(r, 0x36),
                    y: i32_at(r, 0x3a),
                    w: i32_at(r, 0x3e),
                    h: i32_at(r, 0x42),
                    target: i32_at(r, 0x46) as u32,
                }
            } else {
                HelpItem::Picture {
                    archive: r[0],
                    kind: r[1],
                    name: latin1_cstr(&r[2..0x22]),
                    x: i16_at(r, 0x22),
                    y: i16_at(r, 0x24),
                    frames: u16_at(r, 0x28),
                    delay: u16_at(r, 0x2a),
                }
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pictures_and_links() {
        let mut page = vec![0u8; 200];
        page[1] = 7;
        page[0] = 1;
        page[2..2 + 10].copy_from_slice(b"CUR_HTEXT_");
        page[0x22] = 13;
        page[0x24] = 142;
        page[0x28] = 4;
        page[0x2a] = 150;
        let link = &mut page[100..];
        link[1] = LINK;
        link[0x36] = 32;
        link[0x3a..0x3c].copy_from_slice(&423u16.to_le_bytes());
        link[0x3e] = 136;
        link[0x42] = 21;
        link[0x46] = 3;
        assert_eq!(
            parse_help_page(&page).unwrap(),
            [
                HelpItem::Picture {
                    archive: 1,
                    kind: 7,
                    name: "CUR_HTEXT_".into(),
                    x: 13,
                    y: 142,
                    frames: 4,
                    delay: 150
                },
                HelpItem::Link {
                    x: 32,
                    y: 423,
                    w: 136,
                    h: 21,
                    target: 3
                }
            ]
        );
    }
}
