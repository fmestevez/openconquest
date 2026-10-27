//! Technology permissions: what the campaign lets the player build, fit,
//! research and learn so far (the original's bit array at player + 0x45;
//! see `docs/FORMATS.md`). Skirmishes allow everything.

use crate::combat::Upgrade;
use crate::magic::Spell;

/// One bit per permission, numbered as the original's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Permits(pub u128);

impl Default for Permits {
    fn default() -> Self {
        Permits::ALL
    }
}

/// Bits the campaign clears at its start (miss000, 0x449160)...
const CAMPAIGN_CLEARS: &[u8] = &[
    4, 5, 6, 7, 10, 17, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 41, 46,
    47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 61, 62, 63, 64, 65, 67, 74, 76, 77,
];
/// ... and sets.
const CAMPAIGN_SETS: &[u8] = &[16, 71];

/// What each campaign mission's start grants (its brain's start function).
const MISSION_GRANTS: [&[u8]; 14] = [
    &[],
    &[4, 21, 22, 23, 31, 34],
    &[5, 10],
    &[6, 7, 37, 60, 66],
    &[27, 29, 32, 35, 51, 52, 59, 64, 74],
    &[17, 50, 57, 63],
    &[25, 54],
    &[26, 28, 33, 51, 55],
    &[53, 58],
    &[24, 30, 36, 41, 52, 67],
    &[62],
    &[46, 47, 76, 77],
    &[],
    &[],
];

/// The scrolls of knowledge: number, the mission holding it, what it
/// grants and what it says (string resources 2104-2118).
const SCROLLS: &[(u32, usize, &[u8], &str)] = &[
    (1, 2, &[29, 35, 52], "You can now upgrade to silver armor."),
    (3, 3, &[17, 50], "You can now produce strong weapons."),
    (4, 4, &[25], "You can now upgrade power of strong weapons."),
    (5, 5, &[26], "You can now upgrade rate of strong weapons."),
    (6, 6, &[53], "You can now upgrade speed of ships."),
    (11, 7, &[41], "You can now build Bomber ships."),
    (12, 7, &[30, 36, 52], "You can now upgrade to golden armor."),
    (13, 9, &[46, 76], "You can now research Greek Fire."),
];

/// Building the Argo (0x4164e0 offers command 0x7d with bit 80).
pub const ARGO: u8 = 80;

impl Permits {
    pub const ALL: Permits = Permits(u128::MAX);

    pub fn allows(self, bit: u8) -> bool {
        self.0 >> bit & 1 != 0
    }

    pub fn set(&mut self, bit: u8, on: bool) {
        if on {
            self.0 |= 1 << bit;
        } else {
            self.0 &= !(1 << bit);
        }
    }

    /// The permissions at the start of campaign mission `n`, its earlier
    /// missions played through (their scrolls delivered).
    pub fn campaign(n: usize) -> Permits {
        let mut p = Permits::ALL;
        for &b in CAMPAIGN_CLEARS {
            p.set(b, false);
        }
        for &b in CAMPAIGN_SETS {
            p.set(b, true);
        }
        // The Argo waits for Argus (miss002's brain grants it).
        p.set(ARGO, false);
        for grants in MISSION_GRANTS.iter().take(n + 1) {
            for &b in *grants {
                p.set(b, true);
            }
        }
        for &(_, mission, grants, _) in SCROLLS {
            if mission < n {
                for &b in grants {
                    p.set(b, true);
                }
            }
        }
        p
    }

    /// Grants scroll `number`'s knowledge, returning what it says.
    pub fn read_scroll(&mut self, number: u32) -> &'static str {
        match SCROLLS.iter().find(|s| s.0 == number) {
            Some(&(_, _, grants, says)) => {
                for &b in grants {
                    self.set(b, true);
                }
                says
            }
            None => "You have got a scroll of knowledge.",
        }
    }

    /// Whether a building of `class` may be put up (0x43d430).
    pub fn building(self, class: u32) -> bool {
        let bit = match class {
            0x39 => 0,
            0x35 => 1,
            0x37 => 2,
            0x3a => 3,
            0x38 => 4,
            0x3c => 5,
            0x3d => 6,
            0x3e => 7,
            0x34 => 73,
            0x36 => 74,
            _ => return true,
        };
        self.allows(bit)
    }

    /// Settlements may sell their stock and themselves (player + 0x46 bit
    /// 2; 0x43c740 shows Sell Item and Sell Town only then).
    pub fn selling(self) -> bool {
        self.allows(10)
    }

    /// The Argo may be built.
    pub fn argo(self) -> bool {
        self.allows(ARGO)
    }

    /// Settlements becoming towns (command 0x3a).
    pub fn town(self) -> bool {
        self.allows(10)
    }

    /// Shipyards becoming docks (command 0x3b).
    pub fn dock(self) -> bool {
        self.allows(37)
    }

    /// Ships of type `kind`.
    pub fn ship(self, kind: u8) -> bool {
        self.allows(38 + kind)
    }

    /// Items (crews and weapons) of `item`'s group.
    pub fn item(self, item: u8) -> bool {
        self.allows(15 + item / 3)
    }

    /// Fitting armour (`Upgrade::Armour`) or a ram (`Upgrade::Ram`) of
    /// `level` at the smithy.
    pub fn fit(self, upgrade: Upgrade, level: u8) -> bool {
        match upgrade {
            Upgrade::Armour => level == 0 || self.allows(28 + level),
            Upgrade::Ram => level == 0 || self.allows(31 + level),
            _ => true,
        }
    }

    /// Researching `upgrade` from `level` to the next.
    pub fn research(self, upgrade: Upgrade, level: u8) -> bool {
        match upgrade {
            Upgrade::Weapons => self.allows(50),
            Upgrade::Ram => self.allows(51) && self.allows(24 + level),
            Upgrade::Armour => self.allows(52) && self.allows(29 + level),
            Upgrade::Speed => self.allows(53),
            Upgrade::Reach(g) => self.allows(18 + 3 * g),
            Upgrade::Arms(g) => self.allows(19 + 3 * g),
            Upgrade::Rate(g) => self.allows(20 + 3 * g),
            Upgrade::Special(s) => self.allows(76 + s),
        }
    }

    /// Learning `spell` at a temple.
    pub fn spell(self, spell: Spell) -> bool {
        const BITS: [u8; 12] = [59, 55, 54, 57, 58, 60, 61, 62, 63, 64, 66, 67];
        self.allows(BITS[spell as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campaign_unfolds() {
        let first = Permits::campaign(0);
        assert!(!first.building(0x38), "no smithy");
        assert!(first.ship(2) && !first.ship(3), "royal ships, no bombers");
        assert!(
            first.item(3) && !first.item(6),
            "medium, not strong weapons"
        );
        let second = Permits::campaign(1);
        assert!(second.building(0x38) && second.research(Upgrade::Rate(1), 0));
        // miss002's scroll (silver armour) counts from miss003 on.
        assert!(!Permits::campaign(2).research(Upgrade::Armour, 0));
        assert!(Permits::campaign(3).research(Upgrade::Armour, 0));
        assert!(!Permits::campaign(3).research(Upgrade::Armour, 1));
        assert!(Permits::campaign(8).research(Upgrade::Armour, 1));
        assert!(Permits::campaign(13).ship(3));
        let mut p = Permits::campaign(6);
        assert!(!p.ship(3));
        assert_eq!(p.read_scroll(11), "You can now build Bomber ships.");
        assert!(p.ship(3));
        assert!(Permits::ALL.spell(Spell::Lightning));
        assert!(!Permits::campaign(13).spell(Spell::Lightning));
    }
}
