# Ancient Conquest file formats

Reverse-engineered from the retail CD (setup dated July 1999). All integers
are little-endian. "Verified" means every instance on the CD decodes with the
implementation in `crates/formats` (see `crates/formats/tests/game_data.rs`).

## CD layout

| Path | Contents |
|------|----------|
| `SETUP.DAT` | Installer package: executables, maps, missions, interface art (see below) |
| `DATA/CONTROLG.DK?` | Interface graphics (buttons, panels, fonts) |
| `DATA/DATAMAP.DK?` | Map editor / strategic map graphics |
| `DATA/OBJECTS.DK?` | Units, buildings, monsters, effects (~57k sprites) |
| `SOUND/SOUNDS.DK?` | Sound effects and `SOUNDLIST` |
| `SOUND/ANCIENT[1-4].MID` | Music |
| `VIDEO/*.AVI` | Logos, intro (`TITLE`), mission intro (`MIS000`), outro |
| `VIDEO/TASKS.DK?` | One entry, `FON000` |

`openconquest-extract install` copies these (lower-cased) and unpacks `SETUP.DAT`
into the data directory root.

## LZ compression (verified)

Used by `SETUP.DAT` blocks and compressed archive entries. The encoder is in
the original `SETUP.EXE` at `0x408e50`-`0x40929f`.

* 16-bit flag words, bits consumed LSB-first. A new flag word is read
  immediately after the 16th bit of the previous one is consumed, so it
  precedes the bytes of the token that used that bit.
* `1` + byte: literal.
* `0 0 b1 b0` + byte `o`: short match, length `2 + (b1 b0)` (2-5), distance
  `256 - o` (1-256).
* `0 1` + bytes `lo hi`: long match, distance
  `0x2000 - (((hi & 0xf8) << 5) | lo)` (1-8192). Length `(hi & 7) + 2` if
  nonzero; otherwise one more byte `n`: `n == 0` ends the stream, else length
  `n + 1` (up to 256).

## SETUP.DAT (verified)

```
0x00     "PQSetup data for AC", NUL padded
...      payloads: each file is a sequence of independent LZ streams, one per
         128 KiB of output
dir      directory entries (below), back to back
EOF-8    u32 directory offset, u32 entry count (121)
```

Directory entry:

| Offset | Type | Field |
|--------|------|-------|
| 0x000 | u32 | size of the rest of the entry |
| 0x004 | u32 | unknown, 0 |
| 0x008 | u16 | flags, 0x8001 |
| 0x00a | char[0x20d] | file name |
| 0x217 | SYSTEMTIME | modification time |
| 0x227 | u32 | block count `n` |
| 0x22b | u32 | unknown (checksum?) |
| 0x22f | u32[n] | compressed size of each block |

Payloads are stored in directory order and end exactly at the directory, so
the first payload starts at `dir - sum(all block sizes)` (0x53 on the CD).

Contents: `ancient.exe` (the game), `mapedit.exe`, `config.exe`,
`sndcfg.exe`, help files, `maplib.dk?` (terrain), `controlm.dk?` (more
interface art), `demo.dk?` (a saved game), and the scenarios: `miss000`-`013`
(campaign), `single01`-`15`, `battle01`-`25`.

## DKX/DKD archives (verified)

Resources live in pairs: `.DKX` is the index, `.DKD` the data.

### DKX index (`DKBT`)

```
0x00  "DKBT" 01 01 00 08
0x04  u32 file size
0x24  u32 offset of the root page (0x40)
```

The index is a B-tree of 0x800-byte pages starting at 0x40. Page header:

| Offset | Type | Field |
|--------|------|-------|
| 0 | u32 | parent page (or self / -1, unclear) |
| 4 | u32 | leftmost child page, `0xffffffff` in leaves |
| 8 | u16 | bytes used by entries after the header |
| 10 | 6 | garbage / unused |

Entries follow at offset 16, each 32 bytes plus the name:

| Offset | Type | Field |
|--------|------|-------|
| 0 | u32 | child page holding keys after this one, `0xffffffff` if none |
| 4 | u32 | offset of the payload in the DKD |
| 8 | u8 | resource kind |
| 9 | u32 | stored size |
| 13 | u8 | 1 if LZ-compressed |
| 14 | u32 | decompressed size |
| 18 | u16 | width (images) |
| 20 | u16 | height (images) |
| 22 | u32 | parameter A (kind-specific) |
| 26 | u32 | parameter B (kind-specific; for images often 0, 42, 186-195) |
| 30 | u16 | name length |
| 32 | char[] | name |

The key is `(name, kind)`: names repeat across kinds (a terrain tile exists as
bitmap, mask and sprite). Stale bytes after the used area of a page must be
ignored.

### DKD data (`DKFM`)

`"DKFM" 01 01 00 00`, then payloads. The u32 before each payload is a block
length (stored size plus a few bytes); it is not needed for reading.

### Resource kinds

| Kind | Meaning | Status |
|------|---------|--------|
| 0 | Raw records (mission header, descriptor) | not decoded |
| 1 | DIB bitmap | verified: all 8 bpp, 256 colors, bottom-up |
| 2 | Sound sample | not decoded |
| 3 | Font | verified |
| 6 | RLE sprite | verified |
| 7 | Sequence: alias for numbered sprite frames | verified |
| 8 | Composite: layered animation | verified |
| 11 | Mask sequence (same layout as 7) | not decoded |
| 12 | Game records (`OBJ_xxxxxxxx`, `FLOT`, `WEATHE`, `LOBJPROP_*`, ...) | not decoded |
| 13 | Palette animation | verified |
| 16 | Tile set library | partially decoded |
| 17 | Map grid | verified |
| 18 | 1-bit mask | verified |
| 20 | Mission lists (`LIST_OBJ`, `TRACKS_*`) | not decoded |
| 23 | Text/script (`DISCRIPTION`, `SOUNDLIST`, `SND_*`) | not decoded |
| 24 | Quick help page (`SPHELP_nnn`) | verified |

## Images

### Palette

The global palette is the color table of the `PALETTE` bitmap in MAPLIB.
Sprites (kind 6) index this palette. Bitmaps (kind 1) carry their own palette,
which usually differs.

### Sprite, kind 6 (verified)

```
u32 total size
u32 frame width
u32 frame height
u16 flags        bits 10-15: number of seek-table entries; bit 7 always set
u16 x, y         position of the opaque bounding box within the frame
u16 box width, box height
u16[flags >> 10] seek table (offsets into the RLE data, every few rows)
RLE rows
```

Each row is a sequence of opcodes until `box width` pixels are produced:

| Opcode | Meaning |
|--------|---------|
| `0x00-0x7f` | skip `n` transparent pixels |
| `0x80-0xbf` | `n & 0x3f` literal palette indices follow |
| `0xc0-0xff` | repeat the next byte `n & 0x3f` times |

Shadows are drawn as checkerboard-dithered dark pixels.

### Mask, kind 18 (verified)

1 bit per pixel, MSB first, rows padded to whole bytes; 1 = opaque. Used with
bitmap terrain tiles that have transparent areas (shelf, shores).

### Palette animation, kind 13 (verified)

MAPLIB `ANIM_PALETTE`; the entry width is the record count (12). Records:

```
+0   u32  unused (stale pointer)
+4   u8   palette index
+5   u16  unknown (1)
+7   u16  period in ms (250 for water, 150 for amber)
+9   8    unused
+17  u16  color count n
+19  n x (u8 r, g, b)
     3 bytes padding
```

Indices 236-245 animate water and 247-248 animate amber deposits.

### Sequence, kind 7 (verified)

An alias for a run of numbered sprites. The record starts with the
NUL-terminated frame prefix (e.g. `AMBER_`, `WHIRLL_`, `_TREAS05`); the rest
of the 49 bytes is stale buffer data. The entry width is the frame count.
Frame `i` is the sprite `prefix + i` zero-padded, almost always to 2 digits
(the width is not stored, so readers check which names exist). Examples:
`AMBER_` (62 frames), `WHIRLSPR_2` -> `WHIRLD_` (60), `FISH_11` -> `FISH_11`
(`FISH_1100`...).

### Composite, kind 8 (verified)

A layered animation: ships (wake, hull halves, sails, crew), buildings and
large monsters. Entry width = layer count, height = frame count, parameter A
= `frame_width << 16 | frame_height`. The payload is `frames x layers` cells
of 44 bytes:

| Offset | Type | Field |
|--------|------|-------|
| 0 | i32 | dx |
| 4 | i32 | dy |
| 8 | i32 | draw order (e.g. -21000 for a ship's wake) |
| 12 | char[16] | sprite name, `$EMPTY$` for none |
| 28 | 16 | unused |

Each sprite has the full frame size and is drawn centred on the object.

Ships (`SHIP_xxxxx`, 9 layers x 152 frames): frames 0-31 are 8 headings x 4
frames under way (oars and wake `BLB`); 32-55 the 24 hull rotations
(`CORP_1A0000`-`0023`, 3 a heading) a ship turns through; 56-87 8 headings
x 4 frames lying still (at rest, rocking one way, at rest, rocking the
other); 88-151 8 headings x 8 frames furling the sail. Which layers show is a
mask (0x4e5da0) from the ship's state (+0x74) and fittings: the wake (layer
0) always; state 1 (and 4, a ship being built) the whole hull (1) and sail
(7), state 2 the whole hull and the torn sail (8), state 3 the damaged hull
(2) and torn sail; the Argo (+0x259, a type-2 ship so marked by the loader
0x4dfa90) its own sail, layers 9 and 10 for 7 and 8. The ram shows layer 3
or 4 for ram level (+0x27b, record field 14 - 3) 1 or 2, the armour layer 5
or 6 for armour level (+0x27a, field 15) 1 or 2. The state follows the hull
(0x4be030, after each change of it): above 70% of the type's strength 1,
above 35% 2, else 3. Ships keep to the
grid: they turn on the spot to the next cell, then sail to its centre,
straight or diagonal, one ship to a cell. Where the player sent a ship is
marked with `WPMOVE` (controlm: `_WPMOVE`, and `_WPNUM` digits numbering
waypoints) until it gets there. A damaged building's picture is chosen
by its condition and loops like any other (0x422e30); its ruins
(`<prefix>21`) collapse once. The fishing net
`SETKA_<heading % 4>` lies along the hull.

### Font, kind 3 (verified)

```
+0x00  i16  atlas count n (colour variants)
+0x02  u32  absolute .DKD offsets of n atlas bitmaps
+0x42  u8   transparent palette index of the atlases
+0x64  u16  glyph count
+0x6c  u16  line height
+0x6e  u16  last character code
+0x70  glyph records: u16 x, y, width, height, character code
```

The atlases are ordinary 8-bit DIBs stored in the data file without an
index entry (each preceded by a u32 block length); the font loader
(`mfcfnt.cpp`, 0x546aa0) reads them by offset. The index entry's width is
the glyph count and parameter A packs `transparent << 16 | atlas height`.

## Interface layout

The 800x600 game screen is `GAME_FIELD` (592x600: frame around the view,
message strip below; the see-through view is palette index 0) and
`GAME_CTRLBAR` (208x600) on the right; `GAME_FIELD_FS` (800x600) is the
frame without the bar. Names carry a theme prefix: `0` Greek, `3` Persian.
On the bar (offsets from its left edge): minimap frame `BKGND_LMAPA` at
(4, 4), 200x200 (0x4fffe0); object panel `COBJ_BKMAIN` at (3, 206),
201x160 (0x4f5800). The message line `GAME_MEDIT` is at y = 568 and the
money panel `GAME_BKMONEY` at y = 527 (0x504620).

The strip below the view holds the message line `GAME_MEDIT` (376x26) at
(107, 568) and the money panel `GAME_BKMONEY` (103x69) 106 pixels left of
the frame's right edge at y = 527, with the animated `GAME_MONEY` and
`GAME_FISH` icons at panel + (10, 5) and (10, 35) (0x505330) and the
amounts in `MONEY_FONT`.

### Object info (the strip's middle)

While "Help string on/off" (string 0x5b; `[0x5ca542]`, on by default,
the settings tab's checkbox 0x522/0x523) is on, the pointer (gcursor.cpp
0x5175b0) calls 0x516c10 with the object under it, or null when the
pointer is off the view or a modal panel is up. 0x516c10 zeroes a
13-dword struct, puts the pointer's cell at +6/+8 and has the object fill
it through its vtable +0x18 (the counter `[0x5ca8c8]` that would skip
fills never leaves 0); if the struct or object changed (copy at 0x5ca890,
object 0x5ca8cc) and its type (+0) is not 0x1f7, it sends message 0x627
with the struct, or null off objects, to Fieldsys (0x504620 → 0x503d60).
Nothing is drawn until the first object is shown; null then leaves the
empty background, which `GAME_FIELD` already shows there.

The struct (base filler 0x50e7b0: +0 type, +4 `+0x57`, +5 owner nation,
+0xe vtable +0x10, +0x30 vtable +0x20, +0x2f 0xff), each cell a dword flag
then its values: +0xa life (+0xe percent), +0xf fire power (u16 +0x13
against hulls, +0x15 against crews), +0x17 board power (u16 +0x1b), +0x1d
experience (bytes +0x21 have, +0x22 most), +0x23 cargo (+0x27, +0x28),
+0x29 volume (+0x2d, +0x2e); +0x2f a hero icon, +0x30 a treasure icon.
Fillers:

- Ships (ship.cpp 0x4e51c0): life hull x 100 / the type's (0x4e53e0).
  Boats and merchants: cargo, the places taken of 4 or 10 (0x597af0 less
  0x4be800 mode 3). Warships: fire power, summing items 0-8 with the
  owner's damage level of the item's group: `power` = `+0x449` (0x4bc360:
  A x 15 x count x mean condition / 100), against hulls `power x
  0x597fd8[item] / 100 x count` (0x4bc610), against crews `count x power x
  B / A` (0x4bc660, 0x4ac1b0), both x 100 / 1500; board power `+0x53c` x
  1000 / 1500 (/ 3000 and no fire power while blinded by quick lime,
  `+600`, set by 0x4c4120); unless blinded, experience `+0x544` / 100 of
  100 (0 in a dock, state 2). Icons: treasures aboard (`+0x482`), the hero
  `+0x4b5` or 9 with several (`+0x483`).
- Buildings, castles, nests, lairs, heroes' houses (0x423990, also
  town.cpp 0x43c730): life `+0xa8`; the hero icon from `+0x1a0` (0x40df20,
  9 with several), the treasure icon if `+0x19c` holds any (0x40ded0).
  Forts 0x34-0x36 (Fortress.cpp 0x417dc0, units at `+0x4d0`) and barbarian
  strongholds (Barb.cpp 0x40e7f0, `+0x20c`) add fire power over their 12
  unit groups: against hulls each unit's condition x the A x 15 cache /
  100 (0x446710; x 1.5 with player `+0xa8` set), against crews 0x4ac1b0 of
  the running sum (not the group's), both x 100 / 1500; and volume, the
  room taken (0x59729c: 1, 1, 2 by group) of 4, 7, 10 (0x5967a8 by class).
- Cyclopes, minotaurs, amazons (0x413cf0): life `+0x78`, fire power
  against hulls their throw (0x4126f0: the cache's ballista, small
  catapult, item 3 at level 1) x 100 / 1500. Harpies and dragons
  (0x420760: `+0x89` of 200000/250000), the gorgon (0x402ec0, of 150000),
  sea monsters and sirens (0x4049f0, of `+0x7c` and 100000): life.
- Amber and fishing grounds (0x48b100): volume `+0x78` / 50 of 100 (above
  100 as a u16 shown full). A leviathan (0x495f40): life `+0x80` of
  `+0x7c` (0x4935d0) while it has any, else volume `+0x78` of 100.
- Everything else keeps the base: nothing shown. CyclopsJam (0x415ea0)
  passes the fill to the creature on the pointer's cell, else leaves type
  0x1f7: the last info stays.

Drawing (0x503d60): a copy of `CHT_BKINFO` (380x20, `+0x10e`;
`CHT_BKFINFO`, 585 wide, without the bar) at (107, 527) (0x504ef0 adds 208
with the bar on the left). The shown cells in the order above, 78 apart
from x 0: `GAME_INDOBJ` (71x20, `+0x116`), the label (strings 0x3ca-0x3cf
LIFE LEVEL, FIRE POWER, BOARD POWER, EXPERIENCE, CARGO, VOLUME) in
`IND_FONT` (no letter gap) centred in (2, 2, 67, 9) (0x546d80, 0x547860
-1, -1: by width and tallest glyph). A bar of length `n` (most 67): once
`n` > 1 a palette-0 frame `n` x 4 at (2, y) (0x563140), once `n` > 2 rows
y + 1 and y + 2 from x 3 to `n` in colours 200 and 199 (0x562d40); life
uses 203/157 below 50% and 65/68 below 25%. Rows: life `n` = percent x 67
/ 100 at y 14; fire power two bars, at y 11 `sqrt(min(v, 1550) x 4489 /
1550)` and at y 14 `sqrt(min(v, 1000) x 4489 / 1000)`; board power at y 14
`sqrt(min(v, 1100) x 4489 / 1100)`; the others `have x 67 / most`. Then
`CIND_HTR_<hero + 1>` (20x20, `+0x11e`) at width - 67 and `CIND_HTR_00`
(the treasure) at width - 40.

The object panel picture is `INF_OBJ<n>` for buildings, with
`n = class + nation * 13 - 0x32`, drawn at panel + (8, 6); and
`<theme>INF_SHIP<n>` for ships (0x5004d0), with `n` = 0, 8, 16, 32, 40, 48,
56, 64, 72, 80, 72, 80 by ship type, plus the player slot for Greek owners
or 4 + player slot for Persian owners, drawn at panel + (10, 9).

### Command buttons

The object panel has 12 command slots, 4 x 3, at bar + (2 + 51 * column,
403 + 52 * row) (0x4f31b0). The panel asks the selected object for its menu
with message 0x10a0; the object fills a buffer of a mode byte, a submenu
byte and 12 slots of `u8 command, u8 state` (state 1 = enabled, 0x21 =
active). Command ids map to button images in 0x4f08a0, e.g. 1 `CBUT_STOP`,
2 `CBUT_BREAK`, 3 `CBUT_FIRE`, 4 `CBUT_MOVE`, 5 `CBUT_LOAD`,
6 `CBUT_REPSHIP`, 7 `CBUT_BUILDGRS`, 8 `CBUT_TREAS`, 9 `CBUT_BOARD`,
10 `CBUT_RAM`, 11 `CBUT_UNLOAD`, 12/13 `CBUT_FISH`, 14 `CBUT_AMBER`,
15 `CBUT_WHALE`, 0x6c `CBUT_BEHMAIN`; ids 7, 12, 16, 19, 0x3d, 0x56 and
0x6c open submenus. Each button is a kind-11 entry (`CBUT_X`, width =
frame count) plus bitmaps `CBUT_X_00`...: either 2 frames (normal,
disabled) or 31 (an animation, then a greyed disabled frame). `CBUT_BK` is
the button highlight.

Shipyard menu (0x4164e0): one build button per ship type available to the
player's nation, enabled by the requirement mask at 0x5961a8 (bit n =
building class 0x32 + n owned) and the costs, then the upgrade to a dock
(id 0x3b). Settlement menu (0x43c740): build structures (0x10, submenu),
repair (0x11), upgrade to a town (0x3a), plus crew training. Build command
images are composed as `CBUT_BLD<nation digit><suffix>` with suffixes 0x16
TOWN, 0x17 FORTW, 0x18 FORTM, 0x19 FORTS, 0x1a BERTH, 0x1b DOCK, 0x1c STUDIO,
0x1d SMITHY, 0x1e SCHOOL, 0x1f WATER, 0x20 WAR, 0x21-0x2c ships (BOAT,
TRADE, MKILL, SKILL, BOARDER, RAMMER, BOMBER, KSHIP, SPEC1, SPEC2, SPEC1,
SPEC2), 0x2d ONION (bowmen), 0x2e DART (javelin men), 0x2f SWORD, 0x3a
UPGTOWN, 0x3b UPGDOCK. Building costs (0x42d8b0): settlement 400, town 500,
forts 600/1000/1500, workshop 700, smithy 800, port 200, shipyard 400, dock
600, sage school 900, temples 1000.

Building composites (0x4288c0) are named prefix, nation, [class], state,
[facing], damage: state 0 under construction (damage digit 3), 1 standing
(damage 3 intact, 2, 1), 2 ruins (damage 1). Ports and docks add a facing
towards the water using the ship heading convention: 0 east, 2 north, 4
west, 6 south (checked against every port and dock in the missions).

Amber deposits (AmberClassTy, Grig/amber.cpp; record handler 0x48a800):
field 4 x 100 units (+0x78), measured against 1000 (+0x7c) whatever the
start; a negative field 4 starts empty and grows to |field 4| x 100
(+0x80); field 9 is how often it grows back (+0x84), field 10 is kept at
+0x8c; fields 6 and 7 are the cell. The picture size (0x48d2f0) is 3 above
70 percent of 1000, 2 above 35, 1 above 0. Exhausted (state 1), it grows
back while it may: state 2 waits, state 3 counts 100 ticks, then the amount
returns (0x48a430). Ships take amber through its second virtual method
(0x489f50, vtable 0x597460). The sparkle comes at random intervals per
size from the pairs at 0x5b97a8 (10-60, 5-30, 1-10).

Wind (WeatherClassTy, Weather.cpp; global object at 0x5c6a40):

- State: strength 0-10 (+0x34), direction 0-7 (+0x38, the heading the
  wind blows towards, 0 east counter-clockwise), and the prepared next
  values (+0x3c, +0x40). Init (0x407e60): next strength rand&7 + 2, next
  direction rand&7, then one change.
- Change (0x407d20): current = next; with 95% chance the next strength is
  current (3 if calm) + rand%5 - 2, wrapped (negative: 11 - v; above 10:
  v - 11), 1 becomes 0, and the next direction is current + rand%5 - 2
  mod 8; otherwise both are random (rand%11, rand&7). Changes happen
  after rand%10000 + 1500 game ticks (the counter adds `[clock+0xe2]`,
  the ticks of the update; the first period is 10000).
- Message 0x1103 forces strength 7 in a given direction (the CBUT_WIND
  command) for the current period, its own counter running; sent again
  while it blows, the counter becomes period less counter. Then the old
  wind returns.
- Ship speed (0x4a68b0, 0x4a3f40): base speed x 0x597b2c[(merchant ? 11 :
  0) + strength][angle] / 100, where angle = |wind - heading| folded to 0-4
  (0x4e0100). Calm 60%; strength 9: 120 100 80 60 40; strength 10 (storm
  only via the 5% jump): 125 67 28 46 62. Merchant and warship rows are
  identical.
- Display (WindTy 0x504fa0, message 0x10a9): `GAME_ROSEWIND` (theme
  prefixed, 103x69) at (3, 527) in the strip below the view (+208 when the
  bar is on the left), with sprite `GAME_WIND_nn`, n = band x 8 +
  direction, band 0 for strength 3-4, 1 for 5-6, 2 for 7+; nothing below 3.

Settlement menu (0x43c740), main page: slot 0 build structures (0x10),
1 load (5), 2 repair (0x11), 4 upgrade to town (0x3a), 5 and 6 commands
0x12 and 0x80 (not identified), 8-10 train marines (0x2d bowmen ONION,
0x2e javelin men DART, 0x2f swordsmen SWORD); availability depends on
bits of a per-player byte (+0x46). Marine cost: amber from the u16 table
at 0x597164 (30, 50, 70) and fish hundreds at 0x597170 (2, 2, 3). Training
progress is shown as elapsed x 100 / (70 x queued) (0x5971a0). The
structures page (*param == 2) lists port (0x1a) then forts (0x17, 0x18,
...), limiting forts by a weighted count of existing ones (< 16).

Temple of the Sea (TempleSeaClassTy menu 0x437de0): slot 0 "get magic"
(0x54, 1000 amber) studies the next spell of the list at 0x596da8 (spell
indices 0, 5, 3, 2, 1, 4); the player's known count is at +0xb4 of the
player block. Spell index to command (0x42fd70): 0 wind 0x56, 1 reef 0x62,
2 whirlpool 0x61, 3 tornado 0x60, 4 sea monster 0x63 (button
CBUT_MONSTER<nation>), 5 view map 0x55. Casting costs, by spell index, amber
at 0x596844 and fish hundreds at 0x59687c: wind 400/1, reef 5000/12,
whirlpool 5000/12, tornado 2000/5, monster 4000/15, view map 250/0.
Temple of War (menu 0x438b80, same layout): order at 0x596ea8 (spell
indices 10, 9, 8, 11, 7, 6); 6 lightning 0x69 (10000/25), 7 small
lightning 0x6a (3000/7), 8 flame 0x67 (6000/15), 9 ghost 0x66 (1000/1),
10 protection from weapons 0x65 (1000/1), 11 protection from magic 0x68
(1000/1); index 12 (4000/10, command 0x6b CBUT_HGORG?) is not in either
temple's list. The manual names these spells waterspout (tornado), view
(view map; clears the clouds over an area), fireball (small lightning),
phantom object (ghost; makes an object invisible), weapon shield and magic
shield, and gives each a reach from the temple: 12 cells for whirlpool,
reef, monster, lightning and fireball, 15 for waterspout, phantom and the
shields, unlimited for wind, view and flame. Effects: SLIGHT1-4 lightning, FIRE0-2 fire, PROTWSPR_64/128
and PROTMSPR_64/128 protection auras for smaller/larger ships, TRNDSPR
tornado; the sea monster is `UMON<type><state><heading>` (type = the
caster's nation's monster, as on the CBUT_MONSTER<n> button; states 0
surfacing, 1 swimming, 2 diving, 3 striking, 4 dying; 13-36 frames). The wind page (menu page 4) is a
compass: command 0x58 + direction in slots 6, 2, 1, 0, 4, 8, 9, 10 around
0x57 in slot 5.

Combat tables:

- Ships carry items in nine slots, numbered like the building commands
  0x2d + item (button names): 0 bowmen (ONION), 1 javelin men (DART), 2
  swordsmen (SWORD), 3 BLDTRANS, 4 gastraphetes (BLDGASTR), 5 small
  catapult (BLDSMALKAT), 6 ballista (BLDBALIST), 7 big catapult
  (BLDBIGKAT), 8 onager (BLDONAGR). Settlements train items 0-2 (30
  amber each, u16 table at 0x597164, plus fish hundreds 2, 2, 3 at
  0x597170); the workshop (StudioClassTy, menu 0x4361c0) builds items 3-8
  (50 amber for 3-5, 70 for 6-8) in slots 0-2 and 4-6, and has a research
  page (cost tables 0x5971c4, 0x59720c, 0x597254). Menus by class (vtable
  slot 0x68, 0x68 after GetMessage): smithy 0x433e60 (ship refits, see
  below), sage school 0x430f90 (research, see below).
- 0x5964d8: items each ship type carries when built, 40 bytes per type
  (u32 count, then up to nine u32 items): royal 0,2,4,8; bomber 0,1,4,6,7;
  boarder 1,2,2,3; rammer 1,4,5; ship killer 5,4; men killer 3,0; quick
  lime ship 1; the others none. 0x5962fc holds a second list per type.
- Ship records (class 0xc8, 154 fields; read in 0x4dfe00): field 12 hull
  condition in percent, field 14 the crew class + 3 (ramming uses it for
  crew quality), field 15 its level?, fields 18-26 the count of each item,
  fields 29-100 each item's condition (eight per slot, usually 100).
- 0x597d2c: rows of nine u32, three values (A, B, C) for three upgrade
  levels each, one row per item from 0: A is damage against hulls (5, 7,
  3, 40, 60, 80, 200, 250, 220 at level 0), B against crews; for the marine
  rows C (35, 68, 100) is used here as boarding strength (an
  interpretation). Rows 9-18 belong to the special ships and upgrades.
  0x4ac1b0 and 0x4ac1e0 scale values by them; 0x4ac140 caches A x 15.
- The warship menu (0x4e6980) opens the behaviour page (page 5) with
  0x6c `CBUT_BEHMAIN`: slot 0 guard 0x6f, 1 ram attack 0x79, 2 board attack
  0x78 (ship byte +0x684 bits 0-2; a set switch has button flag 0x20), 4
  convoy 0x6d, 5 fire on aggressor 0x72 (enabled only while guarding;
  ship byte +0x633), 6 scout 0x76, 8 patrol 0x6e, 9 guard zone 0x74, 10
  attack zone 0x75. Commands 0x70, 0x71, 0x73, 0x77 and 0x7a-0x7c are
  `BEHFGUARD`, `BEHRAID`, `BEHFAGR`, `BEHRCZONE` and the merchants' amber,
  fish and whale zones.
- Once the sage school's arming (+0xa3) is above 1, its menu (0x430f90)
  offers the nation's unique ships in slots 4 and 5: Greeks 0x50
  `CBUT_UPGFIRE` (fire ship) and 0x51 `CBUT_UPQLIME` (quick lime ship),
  Persians 0x52 `CBUT_UPFRAM` (fire rammer) and 0x53 `CBUT_UPDEAD` (ship of
  the dead), 2000 amber each, studied once (player dwords +0x6e/+0x72,
  +0x76/+0x7a, +0x7e/+0x82, +0x86/+0x8a: done and in progress), each also
  allowed by a mission technology bit (+0x4e bits 6, 2, 0; +0x8bce bit 1).
  The unique ships' weapons are items 9-11 (Greek fire, quicklime, barrel
  with powder): their ranges are the last three of the 12 per level at
  0x598154 (2/2/3 and 3/3/4 cells, the barrel 0), and the Greek fire's rate
  of fire the tenth row at 0x597a3c (50/70/80).
- The workshop's research page (0x4361c0, menu page 3) has three groups
  (button suffixes W, M, S; commands 0x3e-0x46 `CBUT_UPGARMING<group><n>`),
  each with three upgrade kinds whose current levels are player bytes and
  whose maximum levels are at 0x59715c, 0x597158, 0x597160 (W M S: 0 1 2,
  2 2 2, 2 2 2). Costs by kind at 0x5971c4, 0x59720c, 0x597254: nine amber
  values (group x 3 + level) then nine study times in ticks (0x436ee0;
  see Timings), e.g. 1500/1700/1900 amber and 200/250/290 ticks for the
  first kind. The rows are the item
  groups (W = marines 0-2, M = light weapons 3-5, S = heavy weapons 6-8);
  the levels are player bytes +0x8e+group (first kind), +0x95+group
  (second) and +0x9c+group (third). The third is the damage level: the
  damage routine 0x4bb670 calls 0x4bb490 for item i with level byte
  +0x9c + i/3, which picks the A, B and C columns of the item's row. Its
  buttons are 0x3f, 0x42, 0x45 (`CBUT_UPGARMINGW1`, `M1`, `S1`), costing
  1500/1700, 1700/1800, 1800/2600 amber (0x597254; the following values
  100-170 are its study times). The first kind (`<group>3`, 0x3e,
  0x41, 0x44; slots 0, 4, 8) is the rate of fire: the item info routine
  0x4a9870 reads it from u32 0x597a3c[item x 3 + level] (120/150/150 for
  marines, scorpion 100/130/130, gastraphetes 120, small catapult
  90/110/110, ballista 70/90/90, big catapult 60/80/80, onager 70/90/90).
  The second (`<group>2`, 0x40, 0x43, 0x46; slots 1, 5, 9) is the range in
  cells: bytes 0x598154[level x 12 + item] are the longest range (level 0:
  3 2 2 3 3 3 5 3 4 for items 0-8, then 2 3 0 for the special weapons;
  levels 1 and 2 follow) and 0x598178[...] the shortest (0 for marines,
  scorpions and gastraphetes, 1 for catapults and onagers, 2 for
  ballistae). The manual's comparison chart gives the same figures (its
  minimum ranges are one more, counting the ship's own cell). The main
  workshop page opens this one with 0x3d.
- Starting levels (the navy's set-up, Navy.cpp 0x4a78a0, copies them from
  each `DESCRIPTOR` player block, u32s from block +0x14): armour (+0x5f),
  the smithy's level per armour kind (+0x60..+0x62), ram (+0x5e), the
  smithy's level per ram kind (+0x67..+0x69), arming (+0xa3, 0 read as 1),
  an unknown u32, then rate of fire, range and damage per item group
  (+0x8e, +0x95, +0x9c). All zero except
  some computer players of miss007-009 and the human of single06 (armour,
  ram and workshop levels 1-2, arming 2).
- The sage school (SchoolClassTy, menu 0x430f90) studies per-player levels
  0-2: weapons or arming (0x3c, player byte +0xa3, offered once, while it is
  below 2, for 0x5971ac[+0xa3] = 1300 amber),
  rams (0x48, +0x5e, 1000/1500 at 0x42d9a0), armour (0x47, +0x5f, same
  costs) and rigging or movement (0x4f with the move button, +0x51,
  2000/3000 at 0x42d980); Greek schools add the special-ship upgrades
  (0x50-0x53, 2000 amber, after arming level 2). The damage level is the
  workshop's, not this "arming". The smithy (0x433e60) then
  refits ships to armour M/S (0x36, 0x37) and ram M/S (0x38, 0x39; slots
  0-3) up to the researched level, offered while a ship lies alongside
  (+0x1c8). Each order (message 0x138c, 0x434a00; its count byte +0xe, 1
  when 0, which the command panel leaves it for refits, 0x4f3a61) pays 30/60 (0x42d960) or 25/50 (0x42d940) amber a unit and
  queues one byte a unit (+0x1ec; slot 1, 2, 4, 5), done in turn, 150 or
  200 ticks each (0x43542a); when one is done the smithy sends the ship
  alongside message 0x248d (armour) or 0x248e (ram) with the level. The
  buttons show the count left and the progress (0x433d10). A right click
  (message 0x3c, sub 3 or 4) takes the last unit of that kind off the
  queue, even the one under way, its price paid back; a stop (message 10)
  clears the queue, or ends a study, paying back the share not yet
  worked (100 - progress percent of the price; a study also clears
  +0x63/+0x6a). Only the computer's town orders send a building that
  stop (0x42a330, 0x42a3b3); no button of the player's does. Studies wait for an idle
  smithy. It also studies each kind of ram (0x4c-0x4e, slots 4-6;
  player bytes +0x67 + kind) and armour (0x49-0x4b, slots 8-10; +0x60 +
  kind), regular, silver and golden (`CBUT_UPRAMW/M/S`,
  `CBUT_UPARMORW/M/S`), to level 2: silver and golden once the school's ram
  or armour level is 1 or 2, permit bits 31-33 (rams) and 34-36 (armour).
  Message 0x138a (0x434a00) pays kind x 3 + level at 0x5967b4 (armour 300
  600, 400 700, 500 900) or 0x5967fc (rams 500 700, 600 800, 700 900) and
  sets the player's flag +0x63 (armour) or +0x6a (rams), so one of each is
  studied at a time; GetMessage (0x434ed0) raises the level after kind x 3
  + level ticks at 0x5967d8 or 0x596820 (465 480, 470 485, 475 495 both).
  The levels pick the column of the cut of armour (0x597a00) and the
  quality of rams (0x598054) of that kind.
- Building records (66 fields): field 7 condition in percent; on forts
  fields 22-33 count each item 0-11 they hold and fields from 34 give the
  units' conditions (FortressClassTy 0x419f80 builds a unit for each into
  its store, as a settlement's); the missions' forts hold their default
  arming (0x5966b8): wood 4, 5, 6; middle 0, 4, 4, 6, 7; stone 0, 1,
  4-8.
- 0x597cfc: nine u32, the full condition of each weapon kind (2860 ...
  9100); weapon conditions are stored as a percentage of it (0x4be110).
- Ramming odds (0x4ba950): for each ship, crew quality (0x598054, nine
  u32: 100 110 117 / 125 135 145 / 160 175 200 by crew type and the
  player's level for it) x ramming power of the ship type (0x598024: boat
  10, merchant 12, royal 100, bomber 60, boarder 30, rammer 140, ship killer
  20, men killer 40, fire ship 10, quick lime 10, fire rammer 190, ship of
  the dead 10) x an angle factor (0x598078 for the rammer: 90 55 20;
  0x598084 for the rammed: 4 12 20; the angle class comes from 0x4c19b0)
  x a per-type value at 0x5c76b0 (runtime, not yet known). Rammers (type 5)
  get x1.5. The result is attacker x 100 / defender, capped at 200.
- Effects are BOMB sequences (`BOMB%C%1d%1d`): e.g. BOMBA20 hit, BOMBA50
  splash, BOMBG10 explosion; FIRE0-2 and SMOKE0-2 for burning.
- The random generator is the usual LCG `x * 0x41c64e6d + 0x3039`, using
  the high 16 bits.

Ship main menus: merchants (0x4e6e90) move, stop, unload, -, amber, fish,
leviathan, zones; warships (0x4e6980) move, stop, magic, magic, fire,
board, ram, behaviour; both then load, repair, build, treasure. The unique
ships drop some slots (0x4e42e0).

## Terrain

### Tile set library, kind 16 (`b_ALIB` / `s_ALIB` in MAPLIB)

103 records of 211 bytes. Known fields:

| Offset | Field |
|--------|-------|
| 0 | id referenced by map cells (u8; objects use a u16 here) |
| 2-3 | unknown |
| 4 | the tile type (u32; 7 sea, 1 shelf, 8 land, 3 sandbank, 4 mountain or object marker, 5 reef), each map layer's a nibble of the landscape cell's dword at +8 (0x5456c0) |
| 8 | tile name prefix, NUL-terminated, e.g. `b_isl_bkg00_` |
| ~42 | object/terrain class name, e.g. `o_shoal_`, `o_forest1_` |

Tile `v` of a set is the resource named `prefix + "%03d" % v` (kind 6 sprite
if present, else kind 1 bitmap + kind 18 mask). `b_` tiles are 64x64, `s_`
tiles are the 8x8 minimap versions. Records 25+ (sheep, volcanoes, reefs,
whirlpools, monsters, treasures, houses) describe map objects, not tiles.

### Map grid, kind 17 (`b_MAPA` in each mission; verified)

Entry width/height give the grid size in 64-pixel cells (24x24 to 36x36). The
payload is 8 layers of `width * height` cells, each `u8 variant, u8 tile set
id`; `00 00` is empty. Layer 0 is the base (sea `00`, shelf `01`, island
ground `07`/`08`), layer 1 shoals (tile sets `02` `b_ban01_`, `03`
`b_ban02_`; the manual says royal ships, bombers and rammers cannot pass
over them), and higher layers shores, grass, desert,
rocks, forests, hills and ruins. Drawing the sea tile `b_sea00_001` under
every sea or shelf cell and the full shelf `b_shelf_017` under island cells
(their ground tiles leave the shore water open), then the layers in order,
row-major, reproduces the map. Shelf tiles exist as a bitmap, which shades
in four steps from shallow water into the sea, and as a hard-edged sprite;
the map shows the bitmaps.

## Treasures and heroes

A treasure record's kind (field 4) is grouped by 0x4b9740: kinds 0-5 and 8
act when taken (0-2 add field 8 to the player's amber, 0x4a8de0; 3-5 to
the fish, 0x4a8c30; 8 casts spell field 7 where it lay, 0x4b9bf0: 1 reef,
2 whirlpool, 3 waterspout, 4 monster, 9 phantom, 10 and 11 the shields on
the ship); 6, 7 and 27 are carried to a settlement (6: field 7 the deposit
class, 0x190 amber or 0x191 fish, field 8 the amount; 7: field 7 item << 16
| count; 27 the scroll of knowledge); 9 (a spell, field 7) and 17-26 are
kept aboard; 10-14 (torch of Ares, Armour of Ares, feather of Boreas,
magic wood, mirror shield of Athena) and 15-16 (cup of Asclepius, Golden
Fleece) are lasting gifts. Kinds 18-26 are the heroes' items (hero = kind -
18: Jason, Theseus, Orpheus, Zetes, Hercules, Argus, Atalanta, Odysseus,
Perseus, the order of the hero houses' field 22, the `COBJ_HEROYnn`
portraits and the descriptions in the string resources); 17 is taken to be
the Gorgon's head. Panel pictures `COBJ_TREAS00`-`20` show the barrel,
weapons, magic, the gifts, the Fleece, the nine items, the scroll and the
Gorgon's head.

## Mission objects

`OBJ_xxxxxxxx` records (kind 12) are dword arrays; dword 0 is the class.
`PlaySystemClassTy::CreateSystemObjects` (0x51e810 in ANCIENT.EXE) maps each
class to an internal type id, and a factory registry at 0x5c0e24 (pairs of
type id and factory function) creates the object. Class names below are the
game's own, from its error strings.

| Class | Implementation | Object | Position (dwords) | Other fields |
|-------|----------------|--------|-------------------|--------------|
| 0x32, 0x33 | `TownObjClassTy` | settlement, town | 5, 6 | |
| 0x34-0x36 | `LandObjClassTy` | small, medium, big fortress | 5, 6 | |
| 0x37 | `StudioClassTy` | sage school | 5, 6 | |
| 0x38 | `SmithyClassTy` | smithy | 5, 6 | |
| 0x39 | `LandObjClassTy` | port | 5, 6 | 10: facing 0/2/4/6 (right, up, left, down) |
| 0x3a, 0x3b | `DockClassTy` | shipyard, dock | 5, 6 | 10: facing |
| 0x3c | `SchoolClassTy` | | 5, 6 | |
| 0x3d, 0x3e | `TempleSeaClassTy`, `TempleWarClassTy` | temples | 5, 6 | |
| 0x3f-0x41 | `BarbarianClassTy` | barbarian structures | 5, 6 | 0x40 has a facing |
| 0x42, 0x43 | `CastleClassTy` | sea castle, war castle | 5, 6 | |
| 0x44, 0x45 | `HarpyNestClassTy` | dragon nest, harpy nest | 4, 5 | |
| 0x46-0x48 | `CreaturesFlatClassTy` | cyclops, minotaur, amazon lair | 4, 5 | |
| 0x49 | `HeroHouseClassTy` | hero's house | 5, 6 | 22: house 0-8 |
| 0xc8 | `ShipClassTy` | ship | 9, 10 | 5: nation, 6: player, 7: type 0-11, 11: heading 0-7 |
| 0x190 | `AmberClassTy` | amber deposit | 6, 7 | |
| 0x191 | `FishClassTy` | fishing zone | 6, 7 | 8, 9: width, height |
| 0x192 | `WhaleClassTy` | leviathan zone | 7, 8 | 9, 10: width, height |
| 0x1f4-0x1f6 | `CyclopClassTy` | cyclops, minotaur, amazon | | |
| 0x1f7 | `CyclopsJamClassTy` | | | |
| 0x1f8, 0x1f9 | `HarpyClassTy` | harpy, dragon (sent out by their nests) | 4, 5 | 6: the nest, 7: 1 (new), 8-10: the nest's fields 7, 8 (range), 11 (rest) |
| 0x1fa | `MedusaClassTy` | medusa (`GORG%1d`) | 5, 6 | 7, 8: area |
| 0x1fb | `UMonsterClassTy` | sea monster | 7, 8 | 4: type |
| 0x1fc | `LandingClassTy` | landing party | | |
| 0x1fd | `SirenaClassTy` | siren | 5, 6 | 7, 8: area |
| 0x258 | `WhirlObjClassTy` | whirlpool | 5, 6 | |
| 0x259, 0x25a | `ReefObjClassTy` | big, medium reef | 5, 6 | 7: reef kind 1-3 |
| 0x25b-0x25d | `LightningObjClassTy`, `TornadoClassTy`, `SlightClassTy` | weather | | |
| 0x25e | `VolcanoClassTy` | lava flow | 6, 7 | 4: lava sequence 0-10 |
| 0x2bc | `TreasClassTy` | treasure (sprite sequence `TREAS<kind>`) | 9, 10 | 4: kind 0-27, 5: holder id or -1 for lying on the map, 7: parameter, 8: amount |
| 0x2be | `SheepsObjClassTy` | sheep flock | 4, 5 | 7: count |
| 0x2bf | `TraksClassTy` | the particles' keeper: wakes' droplets and projectiles' trails (`Grig\traks.cpp`; nothing of its own to draw) | | |
| 0x2c1 | `SkarbClassTy` | | | |

Positions are map cell coordinates. For buildings and ships, dword 1 is the
object kind (1 = player-owned), dword 2 the nation (1 Greek, 3 Persian,
4 barbarian) and dword 3 the player slot.

### Sprite names

* Buildings (0x4288c0): `prefix + nation [+ class] + state [+ facing] + damage`.
  Prefixes: `TSPR_` (0x32, 0x33, with class), `FORTSSPR_`, `FORTSPR_`,
  `FORTBSPR_`, `STSPR_`, `SMSPR_`, `PTSPR_` (with class and facing), `DKSPR_`
  (with class and facing), `SLSPR_`, `TSSPR_`, `TWSPR_`. State 0 = under
  construction, 1 = standing, 2 = destroyed; damage 3 = intact, 2 and 1
  increasingly damaged. Example: a standing Greek settlement is `TSPR_15013`.
  Lairs use the same scheme with nation 4 (`CLPFSPR_413`).
* Ships (0x4e0120): `SHIP_%C%1d%1d%1d%1d` = type letter (`'A' + type`),
  hull nation, owner nation, owner player slot, mode (0 normal, 1 special,
  2 sinking), e.g. `SHIP_A1100`. The owner digits select the sail colours.
* Sequences with a state digit (0 appearing, 1 alive, 2 disappearing):
  `WHIRLSPR_<state>`, `BREEF<kind>SPR_<state>`, `MREEF<kind>SPR_<state>`,
  `FISH_<kind><state+1>`, `CSTLSEASPR_<state><damage>`; sea monsters are
  `UMON<type><state><heading>`; hero houses `HHOUSE<n>SPR_013`.
* Merchant work (Run_Ship.cpp, 0x4e1bb0): unloading walkway `MOST0_`, a
  composite of 18 frames whose eight layers are the eight facings (one is
  drawn); amber walkway `AMOST_<facing>` (sequences for facings 0, 2, 4, 6)
  with porters `ANOSIL_<facing>`; fishing net `SETKA_<facing>` (0-3);
  porters ashore `NOSIL_<loaded><0><heading>`. The sprites carry no
  offsets: the game places them in code.

### Placement and timing

Nature objects place their frame's top-left corner at `cell * 64` minus a
per-class offset (castles: sea 6, 12 and war 4, 13). Their frame period is
`cycle[state] / frame_count` game ticks, with cycle tables per class (reef
30/60/30, whirlpool 60/60/60, castle 300/20/100/75). A game tick is 20 ms
at speed 3; the speed table at 0x5bed7c has one 24-byte row per speed.

Ship composites `SHIP_<letter><hull nation><owner nation><player><mode>`:
mode 0 is the ship afloat (152 frames: 0-31 are 8 headings x 4 frames
under sail; 88-151 are 8 headings x 8 frames furling the sail, from full
sail to furled; 32-87 not identified), mode 1 its sinking (20 frames) and mode 2 the hull
under construction at a shipyard (80 frames: 40 lying east-west from keel
to finished ship, then 40 north-south); modes 1 and 2 exist only with
owner and player `00`.

Ship headings 0-7 run counter-clockwise from east (0 = east, 2 = north,
4 = west, 6 = south). Ship types (record field 7, sprite letter `'A' + type`, cost table index):
0 boat, 1 merchant, 2 royal ship, 3 bomber, 4 boarder, 5 rammer, 6 ship
killer, 7 men killer, 8 fire ship, 9 quick lime ship (both Greek), 10 fire
rammer, 11 ship of the dead (both Persian). The barbarians' types 1, 2, 3,
5 and 6 match the five ships the manual gives them. Build commands are
0x21 + a different order (BOAT, TRADE, MKILL, SKILL, BOARDER, RAMMER, BOMBER,
KSHIP, ...); 0x42d750 maps types to them. Hull costs (0x5961d8 amber,
0x596208 fish in hundreds): 200/0, 400/200, 1100/400, 1300/400, 900/200,
800/300, 700/200, 600/200, 700/200, 900/200, 700/200, 990/1000; weapons and
crews add to them (0x43e5c0).

Palette index 0xfe in object sprites is a shadow: the destination pixel is
remapped through a darkening table (0x5c1358).

## Control bar tabs and panels

The bar ends in five 42-pixel tabs at y 558 (0x511c00, ids 1000-1004,
left to right at x 0, 44, 85, 126, 167): object panel (wheel), files
(disk), game settings (wrench), sound (clef), help and tasks (scroll). The
tab images `BMARK_<name>_00/01/02` (normal, hover, active) do not follow
that order by name: the disk is `BMARK_VTASK`, the clef `BMARK_CFG`, the
scroll `BMARK_VHELP`. The pause sign is the `WIN_PAUSE` sequence (197x156,
31 frames).

All positions below are bar pixels (0x512ab0 adds the bar's x: 592, or 0
with the bar on the left). Checked pixel for pixel against the original
under Wine.

**Widgets.** Text goes through mfcfnt.cpp: 0x5470f0 sets a font's letter
and line gaps, 0x546d80 its target rectangle, 0x547860 draws a line at an
offset or, given -1, centred (horizontally by its width, 0x5472b0;
vertically by its tallest glyph, 0x547350) in colour variant (atlas) n;
0x5479f0 draws `\n`-separated lines, each centred, the block centred, a
line advancing by its tallest glyph plus the line gap. A check box
(0x4e9960 type 3, drawn by 0x4e9e30) is `TRIGGER_BKGND` (20x20) with the
`TRIGGER_BB` sprite over it when set (states 2, 3). A push button (0x4e9b60)
shows picture `<name>02` disabled, `00` normal, `01` pressed. A wide button
(0x4e9ca0) shows `00`, or `01` pressed, its caption (a string) in
`INT_FONT` gaps 0/-2, colour 1 disabled, 0 otherwise, the text rectangle
w - 2, h + 2 when pressed. A slider (0x4e9f70; strack.cpp, the trackbar)
is a track bitmap with the `TRACKBAR_BB` thumb (20x14) at offset 0 for
position 0, `travel` (track - thumb width) for the last of `count`, else
`travel / count * (pos + 0.5)` truncated (0x552400); a click within half a
thumb of an end takes that end (0x5524a0).

**Front-end scrolling** (Ourlib's widgets, used by Startcom.cpp's mission
screens, Custom missions and Load game, created by 0x52c5c0). A scroll bar
(_sslider.cpp, 0x551ca0) is an up arrow, a down arrow and a track
(_strack.cpp, 0x5526c0): the arrows (messages 0x2f, 0x30) move it a
position back or on unless at an end, repeating while held after 500 ms
every 50 ms (75 for the mission list's); a press on the track takes the
position under the pointer (0x5524a0) and follows it while held (mouse
moves, message 0x61, set the position the same way; the thumb is drawn
from the position, 0x552400, so it steps from one to the next). The track
is its picture 4 pixels in on each side; `*_TR1` thumbs are 15x15. An
arrow shows picture `02` when it cannot move its bar. The map preview is a
viewer (_sviewer.cpp, 0x54c320) of 19 x 19 cells, 8 pixels a cell, scrolled
a cell a position, (cells - 19 + 1) positions on each axis (none when the
map fits); it opens on the human player's starting view. The description
is a read-only edit (_sedit.cpp, 0x54f040): a line a position, (lines -
visible + 1) positions (0x54ee70). The mission list (_slist.cpp, 0x54cd00)
gives its bar one position per entry: the arrows and the track move the
chosen entry (its notification 0x189 loads the mission's map and
description 200 ms after the last change, 0x52dc40), the window following
it no further than it must; the up arrow is live unless the first entry is
chosen, the down one unless the last. Bars: map down (165, 28)/(165, 160),
track `MAP_TR_V` at (165, 52); across (8, 185)/(140, 185), `MAP_TR_H` at
(32, 185); description (766, 52)/(766, 184), `DESC_TR` at (766, 76).

**Game file operations** (`Filesys.cpp`; created by 0x5099f0, messages
0x50a700): `CFILE_BKGND` (200x454) at (3, 5). Fonts: `SYSTEM_FONT` gaps 2/1
(+0x45) and `INT_FONT` gaps 0/-2 (+0x49). The name field (edit, 0x104
characters) at (11, 14) 179x17, its text at x 15 in `SYSTEM_FONT` colour 0.
The saved games' list at (14, 44) 140x280, rows 17 high (bitmap 140x17),
the text 2 pixels in, vertically centred, colour 1, the chosen row colour
0; choosing a row puts its name in the field (0x534). Arrows `SLIDER_UP_`
at (172, 44) and `SLIDER_DN_` at (172, 310) (22x22), the scroll bar
`CFILE_SLSL` at (168, 66) 30x244 with the `SLIDER_BB` thumb 4 pixels in.
Bookmarks `CFILE_BMRKSAVE/LOAD/DEL` (66x70, `01` chosen) at (5 + 66i, 388),
captioned with strings 0x25-0x27 (SAVE, LOAD, DELETE) in `INT_FONT` colour
0, centred, 10 pixels down (message 0x54e). The wide buttons:
`CFILE_SLDGAME` (190x46) at (9, 340), captioned by the bookmark: 0x23 Save
game, 0x24 Load game, 0x22 Delete game (0x4fb); `CFILE_RSTART` (200x46) at
(4, 461), 0x20 Restart mission; `CFILE_MMENU` and `CFILE_QUIT` (98x46) at
(4, 510) and (106, 510), 0x1f `Main\nmenu`, 0x21 `Quit\nprogram`.

**Configuration** (`Cfgssys.cpp`; controls 0x4faec0, captions 0x4fa6f0,
messages 0x4fb540): `CCFG_BKGND` (202x552) at (3, 5); offsets below are
from there. Captions in `INT_FONT` gaps 1/1 colour 0: centred in
(0, y, 202, 22) for Game speed (0x70, y 43), Mouse scroll speed (0x71, 86),
Key scroll speed (0x72, 129), and in (0, y, 152, 22) for Scroll speed
(0x7b, 235) and Message lines (0x7c, 278); `MIN`/`MAX` (0x76, 0x77) in
`LITTLE_FONT` gaps 2/1 colour 1 centred in 25x15 boxes at x 0 and x 177 (x
130 beside the number boxes), y 58, 101, 144, 250, 293. Sliders at x 30:
`CFGSCR_GSPD` y 63 (9 positions, 9 - `SPEED_GAME` [0x5ca556]),
`CFGSCR_MSPD` 106 (10; [0x5ca54e] = 200 - 19 pos, ms per mouse scroll
step), `CFGSCR_KSPD` 149 (10; [0x5ca552] = 95 - 9 pos), `CFGSCR_CHATSPD`
255 (13; message scroll speed [0x5ca784] = 15 - pos), `CFGSCR_CHATSTR` 298
(23; message lines [0x5ca783] = pos + 3). The two numbers in
`CFGSCR_CHATIND` (40x24) at (157, 247) and (157, 290), in the game's
`SYSTEM_FONT` ([game+0xd64]) centred in (5, 0, w - 10, h), 4 pixels down.
Check boxes at x 14, captions 10 pixels into (34, y, 168, 20): message
pop-up (0x7a, y 200, [0x5ca77f]); control bar left / right (0x74, 0x75, y
335, 360, a radio pair, ids 0x3f1/0x3f2, [0x5ca54a]); button animation
(0x73, 385, [0x5ca55a]; disabled when [game+0xa81] is 0xd0dd); help string
(0x78, 410, [0x5ca542]); graphic trails (0x7e, 435, [0x5ca78f]); lighting
effects (0x7f, 460, [0x5ca790]); pack save file (0x80, 485, [0x5ca791]);
in network games guaranteed sending (0x7d, 510, [0x5ca78e]). What they do:
message pop-up shows the game's messages (chat.cpp: `MESS_FONT`, a line
every 20 pixels from (8, 8) of the view, 0x4ff390), the oldest going every
"message speed" seconds or while there are more than "message lines"
(0x4ff4c0); button animation off keeps a hovered button's first frame
(0x4f2a90 zeroes its frame); graphic trails gate the ships' wake droplets
(0x4a3f40) and the trails of projectile letters 12, 13, 16-18 (0x4974d0,
in flight a particle a tick through 0x48fc90: `TRAKS<kind>`, kinds 8, 1
for Greek fire and quick lime where they are, 7, 8, 9 for flaming
letters 16-18 drifting 10 tenths of a pixel a tick down and right; from
the picture's top left plus 0x5975c8 by letter: 9, 14, 6, 8, 10 on both
axes in every direction); lighting
effects set the sprite system's `+0x638` (0x4ed8d0: 0 on, 1 off), which
the sprite drawing reads (0x57252b, 0x572825): off, a frame of encoding 1
(`+0xd & 3`) with a colour remap goes to 0x57b470 instead of the remapping
blitter, which leaves out each copied run whose first pixel is in the
remapped range (shadows 0xfe, the flames' 0x26) and paints the rest,
repeated runs included; pack save file is passed to the archive writer (0x51f4c0). Defaults read
from the original's memory: speed 5, mouse 59 ms, keys 14 ms, scroll speed
5, lines 10, bar right, all boxes set.

Game speeds: the table at 0x5bed7c has a 24-byte row per speed 1-9; its
first column (13, 17, 20, 33, 40, 50, 60, 70, 80) is used by the main loop
(0x51fe20) as milliseconds per game update, the fifth (13-33, then 40) is
`[game+0x34]` (0x4fb540 message 0x508).

**Sound configuration** (`Cfgsys.cpp`; controls 0x4fc750, captions
0x4fc170, messages 0x4fe130): `CCFG_BKGND` at (3, 5); offsets from there.
Music choice (radio ids 0x3f3-0x3f6, [0x5ca55e] 1, 2, 4, 8): None (0x8a), CD
music (0x8d), MIDI music (0x8c), FX music (0x8b), boxes at (7, 20 + 23i),
captions 10 pixels into (27, y, 175, 20). `CFGSND_BKLIST` at (7, 127),
`CFGSND_BKSLLIST` at (165, 128); the play list (bar (13, 134) 150x136,
entries 17 pixels in, "Track n" (0x8e) for the CD), its arrows at bar
(172, 137) and (172, 246), scroll bar `CFGSND_SLLIST` at bar (168, 159).
Buttons `CFGSND_MARK_`, `_UP_`, `_DN_`, `_PLAY_` (`_STOP_` while playing;
47x25) at (6, 54, 102, 150; 268). Check boxes at (7, 313 / 336 / 359): unit
report (0x83), battle sound effects (0x85), ambient sound effects (0x84)
([0x5ca55f] bits 1, 2, 4: the sound player 0x525370 plays channel 2,
the units' voices, only with bit 1; channels 1 and 4, firing, shots
landing, boarding, ramming and lightning, only with bit 2; channels 3, 5
and 6, the world's other sounds, only with bit 4). Volume captions centred in (0, y, 202, 22): Music
(0x87, 385; not with no music), Effects (0x89, 420), FX (0x86, 455), Main
(0x88, 490); `MIN`/`MAX` at y 400 + 35i. Sliders at x 30: `CFGSND_MUSVOL`
405 (the CD or MIDI mixer control, or for FX music 4001 positions of
DirectSound attenuation, [0x5ca568] + 4000; only if the mixer has one),
`CFGSND_EFFVOL` 440 (4001 positions, [0x5ca56c] = pos - 4000 hundredths of
a dB), `CFGSND_WAVOL` 475 and `CFGSND_MAINVOL` 510 (the wave and master
mixer controls).

## Quick help pages (kind 24)

`SPHELP_000`-`SPHELP_008` in `controlg.dkx` (the entry width is the item
count), loaded by Viewsys.cpp (0x527600: `SPHELP_` plus the page number,
0x525f80). Each item is 100 bytes:

| Offset | Type | Field |
|--------|------|-------|
| 0x00 | u8 | archive: 0 `controlg.dkx`, 1 `controlm.dkx` |
| 0x01 | u8 | kind: 1 bitmap, 6 sprite, 7 sequence, 25 hyperlink |
| 0x02 | char[32] | resource name |
| 0x22 | i16 | x |
| 0x24 | i16 | y |
| 0x28 | u16 | frame count |
| 0x2a | u16 | milliseconds per frame |
| 0x36 | i32 x 4 | hyperlink: x, y, width, height |
| 0x46 | u32 | hyperlink: target page |

The first item of every page is the sprite `HELP_nn` (201x445) at (0, 0):
the page's words, titles and underlines are drawn in it. The others are
icons (command buttons, slots, the money and fish animations, the cursors
from `controlm.dkx`, animated by their frame delay). Items are placed at
bar (6 + x, y) (0x526930: the page's x is the bar's + 6). A click inside a
link's rectangle, edges included, opens its page if it exists (message
0x61); over one the pointer is `CUR_HTEXT_` (offset -10, -3; 0x51728a).
Pages: 0 contents (links to 1 Interface Parts, 2 Types of Cursor, 5 Object
Control Tab, 8 Development Team), 1 interface parts, 2-4 types of cursor,
5-6 object control, 7 slots, 8 development team; the others link back to 0
(Quick help contents). The up and down buttons step a page (0x4f1, 0x4f2),
disabled (`02`) on the first and last.

## Fog of war

`maplib.dkx` holds two layers of 64x64 one-bit masks, `b_DARK00`-`15`
(never explored) and `b_FOG00`-`15` (explored, out of sight), with 8x8
minimap versions `s_DARK`/`s_FOG`; each set is 16 dithered edge shapes
(00 full, 15 empty) chosen from the state of the cell's surroundings.
DarkTy (`Grig\dark.cpp`, 0x48b3a0; the object at `[0x5c74a0]`) keeps, per
player (nation * 4 + slot), four `w * h` byte layers at +0x24 + 16 * p:
0 the explored edge codes, 1 the in-sight edge codes, 2 how many eyes see
each cell, 3 countdowns for cells fading in or out. 0x48b9e0 registers a
temporary viewer (cell, player, radius, duration in ms), e.g. the temples'
3000 ms reveals.

**Sight.** Ships and buildings see through one of six masks, radius 0-5
(0x48b200; radius 2 is a 5x5 square less its corners, and so on). A ship's
radius (byte +0x533) is the larger of a type base (0x4e5910: boats,
merchants, fire ships and fire rammers 1; royal ships and bombers 3; the
rest 2) and its crew's longest weapon range (+0x47d, 0x4bc360, the item
range table 0x598154); fire and quick lime ships see 3. Boats always carry
one item 3 and merchants one item 4 (the loaders 0x4dfa90, 0x4dfe00), so
both see 3 and 2. Buildings (constructors' +0x1b0) see from each cell of
their footprint (+0x56 cells, two to a row): settlements 2 and towns 3
over 2x2, forts 4, schools and temples 3, the rest 2. Checked against the
sight counts read from the original's memory at the start of miss000.

**Edge codes.** A cell that comes into view sets itself and its eight
neighbours to 15 (clear), then gives each neighbour the code its own
neighbours imply (0x48c520): neighbours with codes below 15 add bits from
a 15 x 8 table (0x5bdef8; bits 8-11 corners, 0-7 half sides), and the bits
map back to a code through a 15-entry table (0x5bdeb8; none = 15, else an
exact match, else the first with the same corners). On the map's border a
side with only one of its corners covered loses its half sides. The
explored layer passes over cells already clear (0x48c5d0); the in-sight one
is rebuilt from the counts (0x48c750). Raster order reproduces the
original's layer exactly.

**Drawing** (0x5762d0): a cell with explored code 0 shows the cloud
picture `<n>OBL_BIG` (576x512, tiled every 9 x 8 cells over the map; `n`
from the mission's optional `N_CLOUD` record); otherwise black (index 0) is
stippled where `b_FOG<in-sight code>` is set, then the clouds drawn where
`b_DARK<explored code>` is set.

## Help panel

Viewsys (0x526930): `HVIEW_BKGND` at bar (4, 5); the text in `HELP_FONT`
(atlas 0, 2 pixels between letters and between lines) from (12, 11), 188
wide and 445 high, wrapped word by word (a word goes down when it and a
space would not fit); `VIEW_BUTUP_nn`/`VIEW_BUTDN_nn` at (10, 460) and
(108, 460), state 02 when there is nothing to scroll; `HVIEW_TASK01` and
`HVIEW_HELP00` at (3, 513) and (103, 513) (ids 0x406, 0x407; with the quick
help open, `HVIEW_TASK00` and `HVIEW_HELP01`, see Quick help pages; the
panel stays on the page last chosen). The text is the briefing's
stored lines: for a scenario (game +0xd80 = 1) what follows the blank line
`@   `; for a custom mission (2) the whole briefing with its closing `@-`
lines replaced by `@-<objective>` per condition (0x5265e0, strings
3000-3010). The bar's open tab is drawn in state 01.

## Command buttons

A menu slot is a 16-bit value: the command in its low byte, flags in the
high one (0x01 enabled, 0x04 an arrow for a page of further commands, 0x20
a tick for a behaviour switched on, 0x02 a cross). The button (0x4f2a90) is
drawn as: the rim `CBUT_BK` two pixels below the slot, then the picture a
pixel right of it (frame 30 when disabled, 0 otherwise, the animation on
hover; pressed, a pixel down instead of right), the mark (`CBUT_SPEC_00`-
`05`: red/grey cross, green/grey arrow, green/grey tick), and the caption:
the command's string (0x4f21b0, e.g. 0x1a2 ` \nMOVE`) in `BUT_FONT` (atlas
0, atlas 1 grey when disabled), each line centred, lines 9 apart, the block
(11 per line less 2 between) at the button's bottom. The strings come from
`ancient.exe`'s string table.

Ships' main page (0x4e6980): move, stop, two spell slots, fire, board,
ram, behaviour, reload, repair, building, hero/treasure; enabled by a
capability mask (+0x261, all set but bit 11 for boats and merchants, bit 8
for the rest; 0x4e6930). Building opens page 2: a settlement (0x16) and
the forts (0x17-0x19) the player's technology allows; any ship may build
them. A settlement's building page (0x43c740, page 2) packs what may be
built in this order: port, the three forts, workshop, shipyard, smithy,
school, Temple of the Sea, Temple of War.

## Object panel (ships)

`c_load.cpp` (0x4f9520) lays a ship's slots in a grid of four columns 48
apart from bar (9, 210), rows 51 apart, the first cell the portrait's. Each
cell is `COBJ_BKSMSLOT` (47x50) with a `COBJ_SSLOT` picture at (1, 1) and,
for crew, weapons and cargo, a bar: black, 4 high, from (1, 45), its
length (w - 2) * health, two lines of colour inside (palette 200/199 green,
203/157 yellow below half, 65/68 red below a quarter). The list (0x4e5e40):
for warships each crew and weapon unit (pictures 10-12 the marines, 13-15
for nation 3, item - 2 for weapons), the unique ships' own weapon, empty
places (picture 0) up to a per-type count (0x597ae4, big weapons taking
two), the armour (0x22 + level) and ram (0x26 + level) where the type has
them (0x597b14, 0x597b20); for boats and merchants a hundred units of
cargo a slot (0x2a amber, 0x2b fish, 0x2c leviathan; empty 0x29) up to 4 or
10; then from the eighth place treasures (0x3a + kind, 0x5005a0; empty
0x39) and heroes (0x30 + hero; empty 0x2f) by per-type counts (0x597afc,
0x597b08). A ship's armour level is record field 15 and its ram level field
14 less 3 (0x4dfa90); the ram quality table 0x598054 is read by ram level.
The strip above the buttons is `CCTRL_CLRDEF1` (a relief) on a main page,
`CCTRL_CLRDEF2` on others, at y 366.

A settlement's or town's slots (0x43bfe0, page 0) are its stock: the nine
items at places 0-2, 4-6 and 8-10 (marines, medium and strong weapons),
each with its count in the corner (`NUMBER_FONT`, at (3, 3)), dimmed
(pictures +15 for marines, item + 13 for weapons) with no stock, and an
arrow (`COBJ_SSPEC_00`, `01` without stock) to open its units; place 3 its
armour and rams (0x20, dim 0x1f), place 7 its goods (0x2e, dim 0x2d).
Clicking a cell with stock lists its units; clicking one again picks it up
to load onto a ship. Its starting stock is record fields 14-22. Its menu
(0x43c740): building (0), reload object (1, button 5, command 0x1f: with
one of its own forts 0x34-0x36, see Load panel; there when the player may
have marines, player `+0x46` bit 7, enabled while it is idle, `+0xa4` 1
and `+0x10c` 0, and has a fort, `+0x214`-`+0x21c`), repair
(2), upgrade to town (4), sell item (5), sell town (6), and from slot 8 the
marines it can train: only with a workshop of the settlement, and for
swordsmen a smithy (0x43da20, +0x1fc, +0x200).

## Object panel (c_info.cpp)

`CObjInfoTy` (0x4f5130-0x4f7b9e; ProcOnCreate 0x4f5230, PaintInfoObj
0x4f5800, ClearInfoObj 0x4f5c90, CreateInfoButtons 0x4f5dc0, ChangeButs
0x4f5f20, CreateBut 0x4f64c0, PaintGrpBut 0x4f6640, PaintObjBut 0x4f69e0,
DoInfoBut 0x4f72d0, CreateScrolling 0x4f76e0, GetMessage 0x4f79e0) is the
object panel's upper part. Fonts: `<theme>SYSTEM_FONT` (gaps 2, 4),
`NUMBER_FONT` (0, 0), `BSLT_FONT` (letters 1, lines -1).

**Contents.** With nothing selected it draws `COBJ_CLEAR` (208x209) at bar
(0, 206) and nothing else (checked against the original: no strip, no
`COBJ_BKMAIN`). Otherwise it asks the selection (game `+0x3219`, the
flotilla) with message 0x109f; the flotilla (nik_Flotil 0x49f9e0) passes
it to its only member, or with more than one fills the panel's group flag
(`+0x23`) and a list of 0x10-byte entries: class (+0), portrait set 1/3
(+4, ship `+0x250`), type (+5), life percent | 100 << 8 (+9, vtable
+0x10), id (+0xc). PaintInfoObj draws nothing at all for classes other
than buildings 0x32-0x3e and ships 200-211 (so deposits, creatures,
treasures and others show no portrait and no text).

**Group panel.** `CGRP_BKG` (202x197) at bar (3, 206), covering the strip
(c_ctrl 0x4f06f0 draws `CCTRL_CLRDEF*` only without the group flag). A
button per member (CreateBut: `CGRP_BUTFRAME` 44x42 plus 1, 2): rows 47
apart from y 211, four to a row 48 apart from x 10; with 17 members and
more three to a row, twelve shown, `CGRP_BKGSCROLL` at (168, 213) and a
slider: `SLIDER_UP_`/`SLIDER_DN_` at (172, 217)/(172, 369), the track
`CGRP_BKSCROLL` (30x130) at (168, 239), `SLIDER_BB`; ceil(n / 3) - 3
positions, a row of three each (message 0x537). PaintGrpBut: the shadow
sprite `CGRP_BUTBK` at button + (0, 2), then `CGRP_BUTFRAME` with the
portrait at (1, 1) (ships `INF_SHIP` as on the panel via 0x5004d0, forts
0x34-0x36 `INF_GOBJ<(class - 0x34) * 4 + nation>`) and the life bar as a
slot's (black 4 high from (1, h - 5), (w - 2) x life / 100; colours 200/199,
203/157 below half, 65/68 below a quarter), drawn at button + (1, 0), or
(0, 1) held down. Message 0x10ad updates a member's life. A click on a
member centres the view on it (0x517dd0: 0xc450 finds it, 0xc459 gives its
position, 0xcbb to the field; the selection stays). Checked pixel for
pixel against the original (four boats in miss000).

**Single object.** `COBJ_BKMAIN` with the portrait; the cells (0x1c57 +
i clicks, 0x1cbb + i paints) as in the load panel's closed and opened
lists: places after the portrait, or an opened group's header at (9, 312)
and its entries three to a row from x 57, or with ten and more two to a
row from x 62 behind a slider; the slider and `COBJ_BKGSCROLL` (panel
(165, 6)) already show from nine entries; its arrows at (172, 216)/(172,
334), track `COBJ_BKSCROLL` at (168, 238); ceil(n / 2) - 2 positions. A
click in x 9-56, y 210-260 (the portrait) centres the view on the object
(GetMessage 0x61). A settlement's places are its stock (0x43bfe0 page 0:
each item's place flags (stock ? 2 : 1) | 4, its count, no bar; armour
and rams place 3, treasures and heroes place 7). DoInfoBut on a cell:
- cursor in mode 0x34 (Sell Item) and the entry's flags exactly 2 with
  category 1, 2 or 4: the press sends 0x518db0(mode 0x34) with the entry's
  word and its index less the header (the settlement sells it);
- empty (flag 1): nothing;
- flags 2 with 4 or 0x10: a group opens (the list becomes a header, the
  entry's word with flags 0x12, then the object's entries for that key:
  0x43bfe0 case 1 the units of an item, each flags 2 with its condition as
  bar; cases 2/4/8 armour and rams; 0x20/0x40/0x80 treasures, heroes), or
  the header closes it;
- anything else: on the press (state 2) the button's rectangle becomes (9,
  210, 191, 152) and it paints big; on the release it shrinks back and the
  panel repaints (a refresh asked meanwhile, 0x10a2, waits for it).

**The big slot** (PaintObjBut with `+0x8a` = the cell): `COBJ_BKBIGSLOT`
(191x152) with a 188x144 picture at (3, 7): units `COBJ_ARMING09`-`11`
(the panel's nation 3: `12`-`14`) for marines 0-2, `COBJ_ARMING<item - 3>`
for weapons 3-11, `COBJ_ARMING<item + 3>` for the live crews 12-14 (the
ship of the dead's marines, 0x4e5e40); armour and rams `COBJ_ARMORAM<byte>`
(rams 3-5); goods `COBJ_RC<kind>`; heroes `COBJ_HEROY<hero>`; treasures
`COBJ_TREAS<0x5005a0(kind)>`. Then the text of 0x5234b0 in `BSLT_FONT`,
target rectangle (15, 12, 80, 120), wrapped to 80 (0x548590: `#` starts a
paragraph; words end after one of " ,.;:!?" and keep it; a word that
would pass the width (letter gap + its width + the line's) starts a line,
which loses its leading blanks (0x5712c0) unless it is the paragraph's
first; a paragraph's last line gets a space added when it still fits,
0x546a30), drawn by 0x5479f0(x -2, y 0): each line centred, from the top,
advancing by its tallest glyph - 1. The texts (string ids):
- unit (0x4a9870, by the local player's levels, `+0x9c`/`+0x95`/`+0x8e` +
  item / 3): `%s#%s#%d #%s#%d / %d#%s#%d - %d#%s#%d#%s#%d` = name
  (0x3a0 + item: BOWMEN ... LIVE SWORDSMEN), Weight 0x3c3 (1, strong
  weapons 6-8 2), Effectiveness 0x3c0 (0x597d2c B x condition / 100, then
  C for marines or A for weapons x condition / 100), Range 0x3c4
  (0x598178 - 0x598154 by range level), Rate of fire 0x3c5 (0x597a3c),
  Costs 0x3c2 (u16 0x597164 by item / 3: 30, 50, 70, and 2 for items 9-11);
- armour (0x4a99e0): `%s#%s#%d#%s#%d#%s#%d` = 0x3b1 + level, Effectiveness
  0x597a00[level * 3 + player level `+0x60 + level`] | byte 0x597a30 << 8,
  Level 0x3c1 (the player level), Costs (0x42d960: 0, 30, 60); a ram
  (0x4a9a60): 0x3b4 + level, 0x598054[...] | byte 0x598084 (4) << 8 (so
  1124, 1149, 1184 at level 0), Costs 0x42d940 (0, 25, 50);
- goods `%s#%s`: 0x3b8 + kind (AMBER./FISH./LEVIATHAN), 0x3bd + kind (" ");
- hero: 900 + hero;
- treasure by kind: 6 `%s %s %2d %s` (0x3b8-0x3ba by the entry's +0xc 400,
  401, 402; 0x3b7 "After delivering ..."; +0x10 the amount; 0x3bb money,
  0x3bc measures); 7 `%s %s.#%s%2d` (+0xc low word 1/2/4 and high word
  the item or level: 0x3a0 + item, 0x3b1 + level, 0x3b4 + level - 3, else
  string 7; 0x3af "IN A DISASSEMBLED STATE"; 0x3b0 QUANTITY; +0x10 the
  amount); 9-17 0x396 + kind - 9; 18-26 0x38d + kind - 18; 27 0x39f, or
  by the scroll's number (0x47b4a0: 0 2105, 1 2106, 3 2108, 4 2107, 5
  2112, 12 2113); others empty. A ship's treasure entry carries the
  treasure's +0xc and +0x10 (record fields 7 and 8).

## Load panel (c_load.cpp)

**Opening.** A ship's Reload Object (button 0x05, string 429; pointer mode
12) sends command 0x1f with a target (0x4e2ba0), which 0x4dc630 accepts
when it is the player's own and not the ship itself: a port (0x39),
shipyard (0x3a), dock (0x3b), fort (0x34-0x36) or ship (classes 200-211);
not a settlement. The ship sails alongside through its contact machine
(`+0x8e6`, type 0x32 for a building, 200 for a ship); in contact state 3
it sends 0x2875 to the target (0x4b3930), which takes it up (LandObj
0x425990: `+0x104` = 1, `+0x108` = the ship's id). A settlement's own
Reload Object (menu slot 1) does the same with one of its forts (0x441ba0
case 0x1f, checked in 0xc452: one of its own buildings, its list
`+0x1d0` of `+0x1c8`, of classes 0x34-0x36, while it is idle): there is no
sailing, the settlement sets `+0x104`/`+0x108` and sends 0x2875 to the fort
at once and opens the panel (0xc453), and the two stay linked until Finish
reload. When the
object panel is rebuilt (0xc453, 0x4fffe0) it asks the selected object
0xc45a: a building answers `+0x104`, a ship 1 in contact state 6
(`+0x58c`); if set, panel 0x22e (c_load, ctor 0x4f7e40, handler
0x4fa140) replaces the slots (0x22c) and the command buttons (0x22d).
The link ends with Finish reload (0x1d1f: message 0xc45b through the
cursor, 0x518db0 mode 0x32 → 0x280f with byte 0x32; the building clears
`+0x104` and sends 0x2876 to its partner) or when the contact ends
(0x4b4401: 0x2876). Esc also ends it (string 400 "Finish reloading
objects - Esc").

**Lists.** Two side structs at panel +0x20 (0x24 bytes: owner nation +1,
slot +2, class +0xf, count +0x1a, an opened group's key +0x1c, entries
+0x20), filled by message 0x10a1 to the selection: a ship fills side 0
with its own places (0x4e4120 → 0x4e5e40) and side 1 with its partner's
(another ship's places, or 0x4bda00 for a building); a port, shipyard or
dock (`ShipServiceObjClassTy`, 0x4e7640) fills side 0 with its
settlement's stock (`+0x1c8`, the settlement's vtable +0x64 = 0x43c570) and
side 1 with the ship; a settlement side 0 with its stock and side 1 with
the fort (0x441ba0, partner class 0x1494 `FortressClassTy`); a fort
(0x419f80) side 0 with its own list (vtable +0x5c, 0x4179b0) and side 1
with its settlement's (its `+0x1c8`) or a ship's (0x4e4120). A fort lists
each unit, place by place (category 1, flags 2, its condition the bar;
all flags 1 while it is being mended, `+0x10c` with `+0x110` 0xff), an
empty unit place for each room left (0x5966d8 by class: 4, 7, 10, less
the load counted with 0x59729c: 1, 1, 2 by group), then its treasures
(0x4237b0, `+0x19c`, flags 2) or one empty treasure place. Entries are
0x14 bytes: category (+0), its byte (+1: item, level, hero, treasure
kind), index within the category (+2), flags (+6: 1 nothing there,
dimmed; 2 something there; 4 a group to open, arrow `COBJ_SSPEC_00`/`01`;
8 locked, `_04`/`05`; 0x10 an open group's header, `_02`/`03`), count (+8,
`NUMBER_FONT` at (3, 3)), bar (+0xa: value, maximum). Categories and
pictures (0x4f9520): 1 a unit (marines 10 + item, 13 + item for nation 3,
+15 dimmed; weapons item - 2, item + 13 dimmed; 0xff an empty place, 0),
2 armour (0x22 + level, empty 0x21), 4 a ram (0x23 + level + 3, empty
0x25), 8 a settlement's armour-and-rams place (0x20, dim 0x1f), 0x10 a
hundred of goods (0x2a + kind, empty 0x29), 0x20 a hero (0x30 + hero,
empty 0x2f), 0x40 a treasure (0x3a + 0x5005a0(kind), empty 0x39), 0x80 a
settlement's treasures-and-heroes place (0x2e, dim 0x2d). A ship lists
as on its object panel (0x4e5e40; merchants: their units in cargo
places, then goods, then empty places); the unique ships' own weapon
(types 8, 9: item 9, 10) and type 10's first entry are locked, and every
entry of type 11 (0x4e4120). A settlement lists its eleven places
(0x43bfe0 page 0) but 0x43c570 drops the units' group flag: its nine
item places are entries with the count and the first unit's condition
as their bar, and only places 3 (armour and rams) and 7 (stored
treasures `+0x19c`, then heroes `+0x1a0`) open.

**Clicks** (0x4f9da0, cells 0x1e4b + side x 100 + index; drawn by
0x4f9520 as 0x1f13 + ...): nothing for flags 1 or 8; a group (2 and 4)
opens: the side's list becomes its header (key = the entry's first word,
flags 0x12) and the owner appends the group's entries (armour, then rams;
treasures, then heroes); the header (0x10) closes it. Otherwise the first
click chooses the entry (panel +0x68 side, +0x69 index; `COBJ_SACT`
framed) and a second click on the same entry moves it to the other side:
message 0x10ac through the cursor with direction = side (+4), the
entry's first word (+6), bytes 2-5 (+8, the index within its category),
its bar (+0xc, the condition) and its index in the list, less the header
(+0xe). Kill item (0x1d20) sends the same with direction side + 2. Kill
is enabled while some entry of the chosen side has flag 2 (0x4fa0b0).

**Moves** (0x4e7640 at a port, 0x441ba0 at a settlement, 0x419f80 at a
fort, 0x4e3b70 between ships; each first checks that the entry is still
there, 0x4236b0/0x4e39e0 and 0x423720/0x4e3a50). Into a ship 0x4bc8c0:
units into a warship if the free room (0x4be800 mode 5: 0x597ae4 less the
load) is at least the item's size (0x597cf0: 1, strong weapons 2), into
a boat or merchant as cargo if a cargo place is free (0x597af0 less the
places used); armour up to the type's level (0x597b14) and not the one
fitted, a fitted one going back (0x4bde80); rams likewise (0x597b20,
levels counted from 3); between ships the two swap (0x4bcff0) if each
fits the other; goods (0x10) into a free cargo place (0x4bd650); a hero
if a hero place is free (0x597b08, 0x4bd350); a treasure if a treasure
place is free (0x597afc, 0x4bd0c0). Ships 10 and 11 take nothing. Out of
a settlement a unit is always the first of its place (0x446620 index 0).
Into a settlement: a unit with its condition (0x440d40, refused while it
is repairing: `+0x10c` and state 2), armour or a ram into its lists
(`+0x248`, `+0x24c`; level 0 is none and is refused), a hero
(0x424ed0, `+0x1a0`, announced 0x5d99), a treasure through 0x441480
(kind 7 a pack of units, armour or rams; else 0x424b30: kinds 0-2
amber, 3-5 fish, 6 goods credited; the rest stored if the building has
room, 0x424af0: table 0x5c1094 by class, settlements and towns 50, forts
1, others 2, and announced with 0x5d94, 1). Goods do not go ashore here.

At a fort (0x419f80 with the fort chosen, 0x441ba0 with its settlement
chosen; the same cases): a unit goes into the fort when its room allows
(0x419390 → 0x417ef0: the load plus the item's weight within 0x5966d8;
never while the fort is mended) as the last of its place (0x446500); out
of the fort it is the entry chosen (0x419340, by the entry's index in the
list); with a settlement the settlement's first unit goes in (0x446620
index 0) and a unit coming out joins its stock with its condition
(0x440d40); with a ship 0x4bc8c0 and 0x4bcd10 as at a port. A treasure goes
into the fort's store through 0x424b30 (barrels and goods credited, the
rest kept while there is room, 0x424af0: 1 for forts, none while it is
mended, state `+0xa4` 2) and out of it to a settlement through 0x441480 or
to a ship through 0x4bc8c0, then 0x424cf0. A hero is refused (0x424ed0
asks the fort's vtable +0x80, 0x40dec0: 0). Armour, rams and goods have no
case.

**Kill** (direction 2 or 3): a settlement's unit is its place's last
(0x446620), armour or rams their entry, a treasure is dropped from its
store (0x424cf0, announced 0x5d94 with 0), a hero is removed; a ship's
entry through 0x4bcd10, a treasure thrown overboard lying in the sea
where the ship is (0x4bd500 with 1 → 0x520c70).

**Layout** (bar pixels): side 0 on `COBJ_BKMAIN` at (3, 206), side 1 on
`CLOAD_BKMAIN` (199x157) at (3, 400); each with its owner's portrait as
on the object panel (0x4f8ad0: `INF_OBJ` at +(8, 6), `INF_SHIP` at
+(10, 9)). Kill item `CLOAD_KILL` at (4, 365) and Finish reload
`CLOAD_END` at (105, 365), 100x34, frame 01 pressed, captions 474 "Kill
item" and 473 "Finish reload" in `INT_FONT` (0x4e9ca0), hints 401 and
400. Cells (`COBJ_BKSMSLOT`, 47x50) from y 210 (side 0) and 404 (side 1):
a closed list in the slot grid (entry i at place i + 1, four to a row 48
apart from x 9, rows 51 apart); an opened one with its header at (9,
y + 102) and the rest three to a row from x 57, or with ten and more a
window of six, two to a row 53 apart from x 62, with `COBJ_BKGSCROLL`/
`CLOAD_BKGSCROLL` at panel (165, 6)/(165, 4) and a slider (0x4f9af0):
`SLIDER_UP_`/`SLIDER_DN_` (22x22) at (172, y + 6) and (172, y + 124),
the track `COBJ_BKSCROLL`/`CLOAD_BKSCROLL` (30x96) at (168, y + 28),
`SLIDER_BB` the thumb, a step a row of two (messages 0x537/0x538).

## Ships on fire

A shot's blow (0x4bf1a0) can set a ship on fire (0x4e4fb0, layer 5,
`FIRE<size>` played once, a frame every other tick, then removed by
0x4e04d0/0x4e0260): Greek fire (bomb kind 10) at once with size 3 (shown as
2, from the sequence's middle); a flaming shot (+0xe3, the Flame) with a
chance `rand % 100 <= blow / 27 + (100 - hull%) / 2` (0x4e5120, the hull
before the blow), size 0, or one more (at most 2) on a burning ship, which
only goes back to the middle of its flames. A Torch of Ares aboard
(treasure kind 10) keeps both off; +0x255 also keeps them off (not
identified). The flames' bottom middle stands at the ship type's and
heading's spot (0x598ad8, 8 a type) from its picture's top left, 23 pixels
lower. The layer is drawn through the shadow table (0x50e050 with
0x5c1358) and 0x50e130(1, 0x26). Both go to the sprite frame's remap
(0x55dd40: flag 0x100, table at +0x5c, a value at +0x60, a count at
+0x64, 1 from both setters); the caller (0x5725c0) passes the table only
with the flag, and the blitter (0x57b7f0) draws a colour in [value, value
+ count) as `table[(colour - value) x 256 + the pixel beneath]`. So the
build preview's forts and ports have their colour 0xfe drawn as shadow,
as every object's is, and the flames their colour 0x26 (with table 0, the
layer's, 0x55dd10: the shadow table); the rest of the flames is painted. `SMOKE`
(layer 6) is only restored from saves; nothing starts it in play.

## Selection box

The frame is a sprite sequence of 10 pictures in objects.dkx, smallest
first: `RAM_0<w>X0<h>` from a ship's picture size (0x4e0c80),
`RAM_128X128` for settlements and towns (0x43b000), `RAM_064X078` for
ports, shipyards, docks and the barbarians' yard (0x4e7f50), `RAM_072X072`
for other land buildings (LandObj 0x427750); each picture is 4 pixels
larger than its name says. Selecting an object (0x427590) adds it at the
object's picture (+0xec/+0xf0, 0 by default) at its first frame, and each
tick (0x427268, delay +0x7d = 0) steps it a frame larger up to the last;
deselecting (0x427648) steps it back down from the last but one and
removes it at 0.

A ship's: around its picture, 2 pixels out; a building's: its footprint
from its cell, 4 pixels more. 13-pixel corner angles in palette 248 (it
shimmers) with 247 inside them and a small highlight; the hull bar on the
top edge from 14 in, (width - 28) long, 4 high, as the slot bars.

## Minimap

Mapa.cpp (0x51b8c0): in the bar a 24 x 24-cell window at bar (8, 8), 8
pixels a cell, a smaller map from the top left; it scrolls only as far as
keeps the view inside it. The terrain in the `s_` tiles; object icons
(`LMAP8X8`, `LMAP16X16` for settlements and towns, from `controlm.dkx`): a
player's by nation (shape: disc, flag, diamond, cross) and player (colour),
barbarians 16, the rest by class (0x50da60: amber 0x11, fish 0x12, ...) at
their top-left cell, black see-through; then the fog as in the view in the
small masks and `<n>OBL_SM`, over the icons of things that stay put (ships
and creatures only shown in sight); then the view as `LMAPVIEW_n`, a dashed
red frame.

### Radar map (Tab; verified on the original)

A `MapaTy` has a mode (`+0x58`, set by message 0xcbc, read by 0xcbd): 1 the
view (cells of 64 pixels, DrawCellBig 0x519750, the sprites' viewport set to
it), 2 the radar map in the view's place, 3 the bar's minimap, 4 the bar's
window showing the view at full size. InitMapaSmall (0x51b8c0) gives mode 2
72 x 64 cells of 8 pixels at the field's (8, 8) (576 x 512, the view's
place), a smaller map centred by whole cells (`(72 - w) / 2 x 8`), mode 3 24
x 24 at bar (8, 8); InitMapaBig (0x51bd20) gives mode 4 3 x 3 cells at bar
(8, 8), from the view's top-left cell. Tab is an accelerator (scan code
0x0f, set up at 0x503c87) sending 0xcbc to the field (0x504620): unless the
bar is hidden (`[game+0x328d]`), the view's mapa goes 1 <-> 2 and the bar's
(0x4fffe0) 3 <-> 4. Space (0x50f910, message 0x4e1) first puts a radar map
back. On the radar map the frame `LMAPVIEW_n` (9 x 8 cells) stays where the
view is; the pointer over it moves the bar's window to the 3 x 3 cells
around the cell under it (MouMoveLMapa 0x51c6b0: top-left = cell - 1,
clamped to the map less 3; message 0xcb9); presses and clicks go to the
pointer (0x517ad0, 0x5180e0) with the cell under it as clicks on the view do
(a click selects a ship there; a key's pointer mode works there). The bar's
window takes no clicks. Tab again shows the view where it was. When the
frame would leave the radar map, it scrolls the least that keeps it in
(0x51c7e0).

## Command hot keys (verified on the original)

ancient.exe reads the registry key `HKLM\SOFTWARE\Megamedia\Megamedia
Australia\Ancient conquest\Config` (0x500810; written back by 0x501090):
value `Config`, REG_BINARY 0x26c bytes into `[0x5ca528]` (its first dword a
byte sum checksum; a mismatch resets all to the defaults, 0x501710),
`EditorConfig` (0x114 bytes, `[0x5ca410]`), `Result` (0x540 bytes,
encrypted, the best results, `[0x5c9ed0]`) and `CRCResult`. config.exe
(its own copy at `[0x418938]`) writes the same blob. At Config+0x154
(`[0x5ca67c]`) is the key table: a word per command id c = 1-0x81 at
`[0x5ca67a + 2c]`, the low byte a DirectInput scan code, the high byte a
second key to hold with it (0: none). The defaults (0x501550; config.exe
0x4055ee, which its "Default keys" button calls) are single keys:

| Key | Commands |
|-----|----------|
| S | 0x01 Stop, 0x02, 0x1d smithy, 0x24 shipkiller, 0x2f swordsmen, 0x4f speed |
| F | 0x03 Fire, 0x0d fishing, 0x18 medium fort, 0x29 Greek fire ship, 0x2b fire rammer, 0x50 Greek fire, 0x52 fire ram |
| M | 0x04 Move, 0x1a port, 0x23 menkiller, 0x47 armour |
| L | 0x05 Load/unload (reload) |
| P | 0x06 Repair at dock |
| D | 0x07 / 0x10 Build ground object, 0x13 build ship, 0x1b shipyard, 0x25 boarder, 0x2c ship of the dead, 0x53 live crew |
| T | 0x08 Treasure or hero, 0x1c workshop, 0x22 merchant ship, 0x30 scorpion |
| B | 0x09 Board, 0x21 boat, 0x2d bowmen, 0x33 ballista |
| R | 0x0a Ram, 0x0c gather zones, 0x11 / 0x14 repair building, 0x15 repair arming, 0x26 rammer, 0x48 ram |
| U | 0x0b Unload resource |
| A | 0x0e amber, 0x20 temple of war, 0x3d upgrade arming, 0x7d the Argo |
| V | 0x0f leviathan |
| E | 0x12 Sell item |
| C | 0x16 settlement, 0x1e sage school, 0x32 small catapult, 0x6c Behaviour |
| W | 0x1f temple of the sea |
| O | 0x27 bomber, 0x35 onager |
| K | 0x28 royal ship |
| Q | 0x2a quicklime ship, 0x51 quicklime |
| J | 0x2e javelinmen |
| G | 0x31 gastrafetes, 0x54 learn magic |
| I | 0x34 big catapult |
| Escape | 0x81 |

The rest (small and big forts, behaviours, spells, sell town, ...) have
none. config.exe's Hotkey editor lists its entries (a table at
`[0x411030]`: 15 levels of 63 x {string id, parent id, tree item, index};
the index is command - 1) under Buildings (Settlement/Town, Fortress, Port,
Shipyard/Dock, Workshop, Smithy, Sage School, Temples), Ships (military,
merchant and their groups) and Special abilities, and refuses a key that
is predefined ("This is a predefined key"). Its entries "Build the ARGO
ship" (index 121) and "Sell Settlement/Town" (124) point at other
commands' keys (0x7a, 0x7d).

When the command panel makes a slot's button (0x4f2750) it asks 0x4f2140
for its key: Escape (scan code 1) on a slot with the red cross (flag 2),
else the table's; if any, it adds an accelerator (InputClassTy::AddAcc
0x553670 through message 0x10, `MSystemClassTy::SetAccelerator` 0x4e9710)
whose press sends the button's id (0x1d1f + slot) as a click does: the same
pointer modes, pages and refusals. The input loop (Sinput.cpp 0x553090)
fires an accelerator on the key's press when its second key is held, or,
without one, when no Shift, Ctrl or Alt is held (unless the key is one of
them); it goes through the whole list, so every accelerator of the key
fires, in the order they were added (two buttons of a menu sharing a key
are both pressed). The fixed accelerators: Ctrl+S / Ctrl+O save and load (0x511c00,
both Ctrls) and Alt+Q (0x4ef: string 13 "Exit to Windows. Are you
sure?", yes 0x4f0) and Alt+X (0x4f5: string 15 "Exit to main menu", yes
0x4fc) there; the file panel's Main menu and Quit buttons send the same
(0x50a700), Restart asks string 16, and its action button (0x4fb) asks
14 before deleting, 17 before loading, 18 before saving over a slot of
that name, and gives notice 19 when no name is typed (20 when saving
fails); a yes goes on with message 0x4ff. Ctrl+C
(0x163, screenshot) and Ctrl+M (0x163, from the bar), Enter and Space on
message boxes (0x528390, 0x53f360).

**Message box** (gamesys.cpp 0x512af0, 0x512f00): `MESS_BKGND` (197x205)
centred on the screen, the game held meanwhile; the string in
`SYSTEM_FONT` (gaps 2, 5) line by line, centred in (10, 10, width - 20,
130); a question has `MESS_BUT11` at (7, 152) and `MESS_BUT12` at (99,
152), keys Y and N (scan codes 0x15, 0x31), messages 0x5b0 and 0x5b2; a
notice `MESS_BUT01` at (53, 152), Enter, 0x5b4. Captions (0x512cd0):
strings 11 Yes, 12 No, 10 OK in `SYSTEM_FONT` (gaps 1, 1), centred, the
rectangle w - 2, h + 2 when pressed. The button's help line (0x505e70) ends
`" - &0<key>"` (or `<second>-<key>`), the names from the table at
0x5bf598 (20 bytes a scan code).

## Building sites

The landscape grid (`[0x5c74fc]`: width, height, cell width and height as
shorts, then 24 bytes a cell) holds each cell's tile types, a nibble a map
layer from the tileset records' type byte (+4: sea 7, shelf 1, land 8, ...;
0x5456c0), and the object standing there (+12). The town's check (message 0x4eb to
0x42dae0: {x, y, nation, slot, flags, id}, answering ok and the facing):
nothing standing on the cell (unless flag 0x40), ground a building may
stand on (0x503150: tile sets 0x1a, 0x30 on layers 5-7 bar it; trees and
rocks do not, 0x4ec and 0x4ea clear them), its third object slot empty, no
nibble 6 or 7 of 4, land on layer 0; with flag 2 a shore tile, with 1 an
inland one (the 4-way edges 0x5acf13 by the layer-0 variant: 9 for 2-6, 3
for 7-11, 12 for 12-16, 6 for 17-21, 8 for 30-31, 2 for 32-33, 1 for
34-36, 4 for 37-41, none else; 1 north, 2 east, 4 south, 8 west); with
flag 4 one of the 8 cells about holding the settlement `id` or one of its
buildings or forts (+0x1c8 = id); with flag 8 a facing, the first of south,
north and west whose edge is set and whose cell is not land, then east
whatever the edge. The interface asks it as the pointer moves (0xc451):
forts 0x20, ports and shipyards 0x1e, the rest 0x15; a refused cell shows
the ban cursor and takes no click. (An earlier probe here saw a moorage
two cells from a settlement; by the code one goes beside a cell of the
town's, which a building of it there would explain.) A ship founds
(0x4aa990, 0x4b3370, checking with 0x4e4420 on arrival and after landing)
a fort on a shore tile (the nation's permission, then 0x4eb with flags
0x20 and no id) and a settlement on 2 x 2 free inland cells with a shore
tile in the 12 about them (0x42e1e0); it berths beside it from anywhere
(0x4b2710: for a fort the nearest side cell, east, west, south, north). A moorage's front is the
cell its record field 10 faces (0 east, 2 north, 4 west, 6 south), where
ships berth.

## Fish shoals and gulls

A fishing zone (record: x, y, w, h) keeps a shoal on one of its cells
(Fish.cpp 0x48dd80): it stays 25-75 ticks after surfacing, dives, waits as
long, and comes up on a random free water cell of the zone (a hundred
tries), fished or not, its ship following. Its states run on the game
time `[game+0xde]` (about 100 ms a unit, timed on the original), a frame a
unit: it rises (`FISH_<size>1`, 33 frames), lives 0-3 passes of
`FISH_<size>2` (60), dives (`FISH_<size>3`, 33) and stays below 25-75
units; size 1-3 as amber's (0x48d2f0). Seagulls (Gull.cpp 0x41c700): one
for every 225 cells and one more, at most 25; each waits 375-750 ticks,
then flies a diagonal (1, 3, 5, 7) from the opposite edge, 2 pixels a step
every other tick, `GULLSPR` frames heading x 10 + step, its shadow
`GULLSHD_SPR` 120 pixels below drawn through the shadow table.

## Default arming

The Building page's checkbox (string 501, `TRIGGER_BKGND` with `MARKER`,
message 0x1fdb toggling `[game+0x3295]`). While on, a new ship (0x43e5c0)
or fort (0x43e990) comes armed: each item of its list (ships 0x5964d8,
forts 0x5966b8) is taken from the nearest settlement's stock if it holds
one, else bought (amber 30/50/70 by group, fish 200/200/300 for marines) if
it may be made (permitted, with a workshop; swordsmen a smithy), else its
fallback (0x5962f8, 0x596730) taken or bought; ships then add fallbacks from
the last until their strength (1, 1, 2 by group, 0x59729c) reaches the
type's minimum (0x597ae4). Armour (0x596268, fallback 0x596238) costs 30/60
and a ram (0x5962c8, 0x596298, counted from 3) 25/50. A fort is offered only
if every fallback item can be had (0x43d430), the stone fort only at a town.

## Wakes and the move tick

A ship under way faster than its type's threshold (0x5bb1f4) throws off two
droplets a tick (0x4a3f40) from its stern (0x5bb540, per type and heading,
from the top-left of its `SHIP_` composite frame, 56-80 pixels), spread apart by 0x5bb840 plus or minus a little,
drifting outwards (`TRAKS00` for boats and merchants, `TRAKS14` else,
0x5bb224). With s the ship's speed (1/6000 pixel a tick), a droplet starts
at the stern plus the side times (spread + rand & 3 - 2) and moves
(ahead x s / 9000 + side x s / 3500) / 2 tenths of a pixel a tick, with an
acceleration of side x s / 9500 (position v t / 10 + a t^2 / 20 after t
ticks, TraksClassTy 0x48f440), a frame a tick until its last.

Where the player sends ships a green tick (`TAKE_CMD`, controlm) plays
(gcursor.cpp 0x515e60): its top-left 11 and 24 pixels up-left of the click,
a frame each 250 ms, removed on reaching its last (0x5166f8). The pointers
themselves step a frame each 100 ms; both run on the clock, not on ticks.

Clicks (gcursor.cpp 0x5180e0, `CursorClassTy`; message 0x62 the left button,
100 the right):

- **Left button.** It carries out what the current pointer mode stands for
  (0x516ea0 modes: 0 standard, 1 fish, 2 leviathan, 3 `GETW`, 4 amber,
  5 own, 6 select, 7 sea, 8 attack, 9 `WALL`, 10 board, 11 ram, 12 load,
  13 unload, 14 build, 15 repair, 16 magic, 20 treasure, 30 info, 40 sell,
  50 behaviour, 100 ban, 101 `HTEXT`, 102 clock). So with ships selected it
  gives orders, and elsewhere it selects.
- **Own ships** (mode 5, by the click's key flags): a plain click clears
  the selection (0x518d00) and selects the ship (message 0xc453 to the
  player's fleets, Flotil.cpp 0x49ab30); with Shift (MK_SHIFT) it is added
  to the selection, or taken out if in it (0xc455, 0x49e850); with Ctrl
  (MK_CONTROL) its whole fleet is selected (0xc456, 0x49ecb0, the ship's
  fleet number +0x5c). There is no rectangle to select with: the cursor,
  field and fleet code handle no drag but the zone orders' (cursor mode
  0x34, cancelled by the right button).
- **Right button.** With something selected it cycles the pointer through
  the modes possible there (`+0xe4` stepping through the mask `+0xe2`).
  With nothing selected it returns to the standard pointer.
- **Confirmation.** The tick (`CreateConfirmSprite`, `TAKE_CMD`) follows
  modes 7, 12, 13, 20, 30 and 50 only. It does not follow gathering or
  attacking. The cursor class's only other sprite is `VETOBUILD`, for
  building sites: CreateBuildSprite (0x516010) shows, where the site is
  refused (pointer mode 100), `VETOBUILD` (controlm) frame 1 for a
  settlement's two by two, else frame 0 (a cell's square of hatching,
  red), its top left on the site's cell; where it may go, the building's
  own picture (0x4288c0), forts and ports through the blend at 0x5c1358.

Buildings (LandObjClassTy 0x422e30, TownObjClassTy 0x43bd60): the
picture's damage digit is ceil(health x 2 / 67) (0x594ab0) for a standing
building, 3 otherwise; its frames step every cycle[state] / frames ticks
(the class's cycle table at +0xc0: every player building's is 200, 75,
75, 75 ticks, 0x596ab8 and its siblings; so a standing building's 12
frames step every 6 ticks and its ruins' 20 every 3).

## Ships in each other's way

Each tick a ship under way checks the next cell of its route (0x4a3a70,
from the move tick 0x4a3f40). If another ship lies there (class 200-211),
the mover takes the blocker's state (+0x58c) into +0x5c6 and counts its
wait (+0x591, up to 150). If the blocker lies still (state 0), is the same
player's (+0x24f), has a lower priority (+0x5c7) than the mover, and the
wait has passed the mover's patience (+0x5c8, 5 when an order is taken),
the blocker is moved aside (0x4b4b50 with the mover's priority), the
patience tripled (while below 200) and the wait cleared. A blocker in state
3 or 6 is only waited on, as is anything in the cell not a ship (a tick of
wait each); and any refused step but into the route's last cell adds a
tick more, so a ship under way in the way (states 1, 2, 4, 5) counts once
a tick, the others twice. The
push compares the wait (+0x591), which a re-plan leaves alone. When
the step is refused the ship slows to a stop over the distance left
(0x4a2f60: state 4, deceleration speed^2 / (distance x 6000)).

While held up a ship plans its route again every 25 ticks (+0x57b,
+0x57f; 0x4a618d, through 0x4a6330 with the wait); once the wait reaches
150 it gives the move up (message 0x80, sub 8) and lies still, where
ships of its player with a higher priority may push it. The give-up returns 3 to the behaviour moving the ship, which sends it
again (0x4c2e75 and the others). The port gives up plain moves and
gathering (a gatherer bound for its deposit is sent on again while that
brings it nearer); ships with other tasks keep waiting. The re-plan (0x4a6330) does nothing on the tick the
route was last planned; a ship one cell from its goal with a priority
below 51 does not plan when it cannot enter that cell; a ship that has
not waited plans with 0x4a5740, one that has with 0x4a51b0, both given
the wait. 0x4a51b0 plans on a copy of the passability map where
0x4a4e20 marks, within 3 cells of the ship, the cells of other ships as
closed: those lying still (state 0 or 6), or once the wait passes 50 +
rand % 30, all not under way (states 4, 5); not those of its own group
(+0x5e) unless impatient; and when its priority is 51 or more, ships of
lower priority, or equal priority and a later id, not under way, are
listed to plan through (0x4a5540) instead, taking that route if 0x4a56a0
prefers it to the plain one (0x4a56a0 with the two routes' lengths:
the plain one, if it exists, unless it is longer by more than 4 cells,
5 up to 10 cells long, 6 up to 15, 7 up to 20, and by half or more
beyond). A stop (its first argument) sets the
priority to 0. Re-planning (0x4a6150) runs every 25 ticks
whenever the wait, the ship's cell or its goal changed, or it lies still,
under way too, never while turning; at a wait of 150 or on its goal it
gives up (0x80 sub 8, returning 3). 0x4a6330 with no route adds a tick of
wait; with a route that gets no nearer it adds one and returns 2, a
give-up only at priority 50. 0x4a56a0 is called (R1, R2, len1, len2): R1
planned through the listed ships, R2 round those (0x4a53f0) the mover
cannot expect to clear (waited: not under way and of at least its
priority; not waited: lying still); non-zero takes R2, so it goes round
unless that is twice as long and 5-8 cells longer, and with no R1 an R2
longer than 7 cells is thrown away. Closed ship cells, and listed ones
gone through, are remembered (+0x5cd, 0x4a7230) and stay closed in every
later plan while their ship lies there (0x4a7290), until a stop under way
clears them. Ships planned through are never swapped: the mover waits,
pushes one of lower priority lying still, or goes round. The port does
this when a ship is held, and every 25 ticks under way (only re-routing
when its route runs into a closed cell).)

The route finder (0x55ba50; 0x55ca10 floods from the goal instead): two
straight runs if every cell is open (0x5718f0: along the axis the way
leans and diagonally, the axis first when it leans twice as much, the run
nearer east first, then the other order); else a flood from the start (1,
a straight step 2 more, a diagonal 3, through open cells, a diagonal free
to pass between closed ones) traced back from the goal downhill, the
diagonals (one more) before the straights, a tie keeping the way it went;
past the goal the nearest reached cell within the tolerance. The ships' behaviours move through
0x4a6270 (11 callers, 0x4ac8d0 ... 0x4c26fa), all with a tolerance of 0:
the first plan, not waited, takes the exact finder (0x55ca10, none for a
goal it cannot reach, a tick of wait), and the next, waited, 0x55ba50, so
a ship heads for the nearest cell it can reach. The port does so for plain
moves and re-plans; its other orders refuse an unreachable goal outright.

The move states (+0x58c, dispatched by the move tick 0x4a3f40): 0 lying
still (0x4a3150: the hull rocks, the velocity halves each frame), 1 told to
stop (0x4a38f0, then 4), 2 turning on the spot (0x4a3320), 3 at a cell
choosing the next (0x4a3560: the step checked by 0x4a3a70; refused, it
waits; free, under way as 5, or turning as 2 for more than 45 degrees), 4
slowing to a stop (0x4a2f60), 5 under way between cells, 6 held by a
boarding, ram or dock (0x4b5fbf, 0x4b7452, 0x4c3410, 0x4ae470). The tick
stamps +0x58d every frame but in state 3, so a ship waiting there 50 ticks
(0x4a374a) falls to state 0 with its route kept, where the push rule
applies to it.

Unloading (0x4ad9f0) draws the ship a new priority of 91 + rand % 10 each
time (when its priority is 1-234), above the 90 of ships coming in to
gather, so a ship leaving the berth may push them aside.

When a navy gathering zone has no deposit left (0x4ca290, the search
0x4c5b90 finding none), each of its ships: on the zone's list of points
(+0x61; 0x4cea20 finds the ship's), once lying still sails by a plain move
to the next point (0x4ce7c0 steps the list, wrapping by its last argument;
0x4cf070); laden, goes to unload (0x4c6640, priority 90); idle in the
zone, is sent message `*` (0x4d8b30: 1, 1, 0 and the id of a ship of the
zone picked at random): a follow order. Message `*` reaches the ship as
event 0x280f (0x5228c0, 0x4dc630, 0x4e2ba0) and goes in by intake 0x80
(+0x858); 0x4ab250 replaces its movement with task 1 (+0x54c): keep within
A cells (+0x556, payload bytes 1-2) of object B (+0x55a, bytes 5-8),
priority 0 (byte 9), patience 5, re-plan every 25 ticks. Each tick
(0x4a5f30) its goal is the ward's cell (a building's own); within A
(`max + min / 2`) it stops but keeps the task. It ends only when the ward
is gone (0xc0), it gives up stuck (150) or another order replaces it; a
new `*` retargets. The navy uses A = 1 (zones, 0x4ca290, 0x4d8920, ...)
and 2 (escorts, 0x4c6270, 0x4c6ee0, 0x4c9270). When none is laden,
listed or under way, a human's zone is marked done (+0x5d). The port's idle zone
gatherers follow a zone ship so, looking out for deposits meanwhile; alone,
they sail to the zone's middle.

Pushing aside (0x4b4b50): the blocker picks at random one of the eight
cells about it that it may enter; failing one, a cell held by another
ship of its player with a priority below the pusher's (which is pushed in
turn when it gets there); and is sent there (message 0x80, or 0x14 with
`param_2`) with the pusher's priority.

The priority is a byte of each order message (+0x19 of a move; 0xff
leaves it as it was), set by the navy as it hands its orders down; the
player's orders go through the navy too (the click 0x5180e0, message
0xc458 to the navy 0x49ab30, 0x49e230, a fleet made of the selection
0x49ca60). A plain move 0 (0x4cf070; a re-sent one 0xff, 0x4cc650),
gathering 90 (0x4c89b0; laden ones sent to unload 91: the helper's 90 and
`inc` at 0x4aac43), unloading at a port 76 (0x4d74a0: 75 + 1), docking
for repairs 80 (0x1e, 0x4dbb40), founding 100 (0x20, 0x4cb0b0), taking a
treasure 90 for a computer (0x4d9f50) but 0 for the player (0x49e0e0).
Attacking (message 0x17, 0x16 sub 7), boarding (0x17), ramming (0x18) and
loading reach the ship through intakes that do not keep the byte (0x4a9f2d
stores payload bytes 0-6; 0x4aa840 reads +0x823, which nothing writes,
the ship being zeroed at 0x4dc3f0), so they set 0; the fire records the
same (0x4aa0d4 leaves +0x12). Patrols, scouting, escorts and zones go out
as plain moves and follows: 0. Stop (intake 8, 0x4aa540) sets 0 when the
ship is moving (0x4a6330 with 1, 0x4a639a). Unloading (0x4ad9f0) rolls one
between 1 and 234 again, 91-100. Only an order's intake sets it: each kept
order copies its byte unless 0xff, with the patience 5 and the re-plan
every 25 ticks; the ships' own behaviours move without touching it. 

## Resource counters

The money panel (0x505530) draws each amount as `%7d` centred in a black
50x20 bitmap at panel (45, 10) for amber (`MONEY_FONT`) and (45, 41) for
fish (`FISH_FONT`), letters 0 apart, in the game palette. A character a
font lacks measures as its first glyph (0x547170), so the padding spaces
are a digit wide. The shown amounts start at 1 and roll towards the real
ones by 1, 10 or 100 a tick.

## Menus

`controlm.dkx` (in the installation root) holds the front end: 800x600
backgrounds `<theme>M_BKGND_00`-`05` (title, dimmed title, best
conquerors, mission accomplished, mission failed, your mission), the
loading screen `<theme>LOADING`, button shapes `M_BUT0`-`3` (174x40 and
116x40; captions are drawn by the game), list and scroll controls, and
fonts `S_FNT_<size><style>` (sizes B, L, M, S; style S plain dark, H gold).
`data/datamap.dkx` is the scenario editor's (title `ZAST`).

## Timings

Times count the game clock (+0xde of the game object, one tick per
+0x34 ms: the second u32 of the speed's 24-byte row at 0x5bed7c, 13 17 20
33 40 40 40 for speeds 1-7). A new game starts at speed 5 (0x501710), 40
ms a tick; measured on the original under Wine, 470 ticks in 20.3 s. The
speed is saved with a game (`SPEED_GAME`).

- A picture advances a frame once more than its frame delay (+0x7d) of
  ticks has passed since the last (+0x81): ships 2 (a frame each 3 ticks,
  under way, turning and rocking); walkways, nets and porters 0 (a frame a
  tick); gulls 1.
- A ship turns a rotation of 24 a frame (0x4a3320); under way it bears off
  45 degrees at once, more and it stops to turn (0x4a3560).
- Ship speeds (0x4ac210, at start-up): top speed = 0x5bb4e0[type] x 400
  (x 1.15 and x 1.3 with rigging 1 and 2), acceleration = 0x5bb510[type]
  x 8, in units of 1/6000 pixel a tick: each tick (0x4a3f40) the velocity
  grows by the acceleration up to the top speed, both scaled by the wind
  table 0x597b2c, and the position by the velocity, a straight step half a
  pixel per 3000 units and a diagonal a third on each axis. Before the end
  of its way it brakes at v squared / (distance x 6000) (0x4a2f60).
  Measured live: a boat 60000 / 1200, a merchant 56000 / 1120; a merchant
  at 33600 made 4.8 pixels a tick.

- Construction: a building's construction picture plays once over the
  first entry of its class's cycle table (+0xc0, set by each constructor:
  200 ticks for every player building; 300 at 0x5949e8 and 50 at
  0x595e80 for two other classes); progress is elapsed x 100 / cycle.
- The sage school (0x432210): arming 0x5971b8[+0xa3] = 450 ticks; ram,
  armour and rigging 600 ticks for the first level, 700 for the second;
  a unique ship 400 (0x432210 compares the elapsed ticks with 399).
- The workshop's studies (0x436ee0): the second half of each 18-entry
  cost table is the study time, by group x 3 + level (0x5971e8 rate of
  fire 200, 250/290; 0x597230 range 150/285, 270/360, 240/420; 0x597278
  damage 100/125, 100/170, 170/150). There are no fish costs.
- Marines (settlement) and weapons (workshop): 0x5971a0[item / 3] ticks
  each, 70, 80, 95.
- Ships take no time to build: the yard (DockClassTy 0x416a40) pays and
  creates the ship record at once (message 0x1488, record field 4 = 2,
  which the ship loader 0x4dfe00 does not read) when a berth is free
  (0x4e82f0), with the nation's "ship built" sound (events 0xe4-0xe7);
  orders that find no free berth wait in a queue that is retried every 25
  ticks (0x416f90).

## Amber glints

An amber deposit glints in bursts (0x48a430 state 0, 0x489bd0): it waits
10-60, 5-30 or 1-10 ticks by size 1-3 (0x5b97a8), then plays `ASTAR_` once,
a frame every 1-4 ticks (random), at a random point of one of its
variant's two rectangles (0x5b97cc, picked at random).

## Selling

A settlement's Sell Item (command 0x12, slot 5 of its menu) sends message
0x280f with the chosen stock place (0x441ba0): an item brings back 75 % of
its amber price times its condition, an armour piece or a ram 75 % of its
fitting cost; no fish. Message 0x248f sells a building: 75 % of its cost
times its health (0x425990), and a settlement's stock likewise, fish at
75 % too. Sell Town (command 0x80, slot 6) sends 0x248f to the settlement: its
stock units at 75 % of `cost[kind / 3]` and of crews' fish (0x597170) by
condition, armour, rams and goods at 75 %, the building at 75 % of its
cost by health, which then falls to ruins (state 3). Sell Item and Sell
Town show only with permit bit 10 (player +0x46 bit 2) and are enabled
while the settlement is built and idle; Sell Town also hides while a ship
is taken up for loading (+0x1c8). No menu offers Sell Fish (0x7e).

## Hulls, blows and dock repairs

A ship's hull points (`+0x271`) top out at its chart value (0x5bb4b0: 28,
90, 100, 120, 65, 70, 36, 36, 65, 75, 110, 120) x 650 (0x4ac210, through
0x4ac110 as the speeds). A shot carries its item's A x 15 (0x4ac140 caches
it at 0x5c75cc; 0x499410). A blow (message 4, flag 4, 0x4befc0) is first
cut by the armour (0x597a00: 0, 7, 14; 20, 27, 33; 37, 44, 50 percent, by
the player's level of the fitted kind `+0x27a`), then shared out
(0x4beb50) among the hull (type share 0x597ad8: 7, 10, 21, 18, 10, 12, 6,
6, 3, 4, 2, 4), the armour kind, the ram, the weapons aboard and the cargo
loads; the crew's share (its unit count) is reckoned against the same sum
without counting itself; the hull, crew and weapons shares come off them
(see Units' conditions). The hull at 0 wrecks the ship (state 4). At a
dock (0x4b1fc0) every 9 ticks a ship gets back up to a tenth of its hull
for its type's price (0x59b388: 140, 280, 800, 910, 650, 560, 490, 420,
420, 630, 420, 630) times the share mended, and worn units are made whole
for `cost[kind / 3] x (100 - condition) x 9 / 1000`; short of amber, the
human's ship gets what the amber buys, a computer player's stops.

## Units' conditions

Each marine and weapon aboard has its own condition in percent: counts
by item at ship `+0x289` (nine bytes, items 0-8; `+0x292..` the unique
ships' own), conditions at `+0x299 + item x 32 + index x 4` (eight u32 per
item; ship records fields 29-100, always 100 in the shipped missions).
Merchants and boats carry marines as cargo instead (`+0x4e5`, six-byte
entries: flag 1, kind, condition u32; count `+0x484`), out of reach of
blows. A unit's points are its condition x its item's full strength / 100
(0x597cfc: 2860, 2990, 3120, 5450, 5590, 5720, 8840, 9360, 9100).

**Shares** (0x4beb50): `+0x269` hull (0x597ad8 by type), `+0x26a` crew
(marines x room, 0x4be780 mode 1 with 0x597cf0: 1 each), `+0x26b` armour
kind, `+0x26c` ram kind, `+0x26d` weapons (items 3-8 x room: 1, heavy 2),
`+0x26e` cargo (`+0x47f` fish, `+0x480` leviathan, `+0x481` amber loads,
`+0x484` cargo units), `+0x26f` treasures (`+0x482`), `+0x270` heroes
(`+0x483`). The sum is of all but the crew's; the armour, ram and cargo
shares only soak up their part.

**A blow.** A shot (0x4bf170, per bomb aboard `+0x7de`) is cut by the
armour plus any Armour of Ares (capped at 85%), then: hull `share x blow /
sum x 0x597fd8[bomb kind] / 100` (0 for marines' shots and quick lime,
100 for weapons and Greek fire, kind 0xc), wrecked at 0 and then nothing
else; crew `0x4ac1b0(bomb kind, 0, share x blow / sum)` = that x B / A of
the bomb's row of 0x597d2c (rows 0-8 the items, 12 Greek fire 650/300, 13
quick lime with no A: as is); weapons `share x blow / sum`. Other blows
(message 4, 0x4befc0) the same without the bomb's factors. A ram
(0x4c1ad0): Argus takes 75% off, the rammed ship's armour its cut, then
hull share x 150 / 100 (0x598094), crew share x 40 / 100 (0x598090),
weapons share. Boarding (0x4b5df0 state 3, 0x4c3410 for the defender,
blows from 0x4b5b00: the boarders' `+0x6a4` = defenders' strength + 1 +
prize hull / 100, the defenders' `+0x721` = boarders' strength + 1):
won, the boarders take blow / 10 ten times on crew and weapons and blow /
10 on the hull, the defenders lose all marines (0x4be410 with -1), take
theirs nine times on weapons and x 7 on the hull; lost, the reverse
(boarders: marines gone, nine times on weapons, blow / 10 x 7 on the hull;
defenders: a tenth ten times on crew and weapons, all on the hull).
Hercules or the ship of the dead take theirs on the hull only.

**Spreading it** (0x4be410 marines 0-2, nothing for the ship of the dead;
0x4be110 weapons 3-8; the unique ships' own are never worn): each unit is
put in a random place of a table as long as the units (the next free
place if taken; the LCG at ship `+0x18`), one is drawn weighted by its
points (`rand % total`, the first whose running sum reaches it), and the
blow comes off it. If it does not hold, the unit is left with its points
plus (points - blow), so twice its points less the blow, broken at 0, and
`blow - points` goes on to the next place, round the table, likewise,
until a unit holds it or every unit has had a turn. A unit broken when
every unit is broken is not written back: the last unit standing keeps
its condition. Conditions written back are `points x 100 / full`; a
weapon at 0 on a boat or merchant is set to 10, and in 0x4be410 a boat's
first item 3 (`+0x2f9`) and a merchant's first item 4 (`+0x319`) are
raised to 20. Units at 0 are then removed (0x4bc6c0) and the ship's
firepower redone (0x4bc360).

**Effects.** 0x4bc360 keeps each item's mean condition (`+0x419 + item x
4`) and its firepower `+0x449 + item x 4` = A x 15 (by the player's damage
level) x count x mean / 100; the ship fires each item as one shot of that
times its accuracy (0x4bfc20), so worn weapons (and bowmen and javelin men)
hit weaker. Boarding strength (0x4b4e80) sums each marine's condition x
0x4ac1e0 (C x the A x 15 cache / A) / 1000, ten times on the ship of the
dead.

**Mending.** At a dock (0x4b1fc0, after the hull, items 9 down to 0): a
unit below 100 is made whole for `0x597164[item / 3] x (100 - condition)
x 9 / 1000` amber (30, 50, 70) if the owner has it. A Cup of Asclepius
heals the marines and the Golden Fleece every unit, a point every 1000 of
their count (0x4b98b0; see Healing aboard). A settlement's stock keeps
its units' conditions (two-byte places, kind and condition); a unit
moved through the load panel carries its condition (the entry's bar,
message 0x10ac `+0xc`) both ways; a settlement's place shows its first
unit's (0x43c570), gives out its first and throws away its last
(0x446620). The workshop's command 0x15 (`CBUT_REPARMING`, "REPAIR
ARMING", slot 10 of 0x4361c0, permit bit 11 = player `+0x46` bit 3;
enabled while the settlement is idle, some unit is worn (0x440f80) and
the amber suffices, or while it repairs, showing its progress `+0x120`)
has the settlement repair: message 0x1e sub-order 2 (0x441ba0) takes
`(100 - condition) x 0x597164[kind / 3] / 100` amber per worn unit
(0x440fd0) at once and enters state 2 for `(100 - condition) x
0x5971a0[kind / 3] / 100` ticks summed, at whose end every unit is
whole; units are refused (0x440d40) meanwhile. Cancelled, it refunds the
part not done (`(100 - progress) x amber / 100`) and seems to mend units
as far as the time spent goes (not decoded in full; not done here).
Selling a stock unit brings 75% of its price times its condition.
Forts hold units with conditions too: see Forts' units.

Buildings, strongholds, castles, nests and lairs hold points out of a
maximum by class (0x424910: settlement 160000, town 220000, forts and
barbarian strongholds 180000, workshop, shipyard and cyclops lair 100000,
smithy and port 130000, dock 140000, school 81500, Temple of the Sea
67500, Temple of War 110000, castles 300000, dragon nest 350000, harpy
nest 280000, minotaur lair 80000, amazon lair 60000, class 0x49 40000),
their percentage at `+0xa8`. They take a blow whole (message 0xda96 and
the like), a fifth of it under a magic shield (`+0x15c`), and fall at 1
(0x425990).

## Forts' units

A fort (`FortressClassTy`, Fortress.cpp, handler 0x419f80, vtable
0x59592c) keeps its units in twelve places, one per item, at `+0x4d0 +
0x23 + item x 4` (Weap.cpp objects of 0x4a bytes: `+0x38` item, `+0x3b`
count, `+0x3f` the units, two bytes each: item and condition in percent);
`+0x4d0 + 0` is its own LCG. Message 2 builds them from the record:
counts of items 0-11 in fields 22-33, conditions from field 34 one after
another (saved back by 0x4190a0, at most 30). Barbarian structures
(`BarbarianClassTy`, Barb.cpp) have the same at `+0x20c`. Items 9-11 have
no weight in 0x59729c and never occur.

**Room.** 0x5966d8 by class: small 4, medium 7, big 10, each unit
taking 0x59729c[item / 3] (1, 1, 2): the default arming fills it.

**Blows** (message 0x138e but for kind 9, the hook: shots, 0x496xxx and
0x498xxx; 0x158bf, a stare, becomes kind 4). The message's `+4` is the
bomb kind, `+5` the flaming flag, `+9` the blow, `+0x15` the shooter's
player. 0x418f60 (vtable +4) first: with a Torch of Ares in store (flag
`+0x4ac`, 0x419e00) a flaming blow is posted again at 100 / 150 with the
flag cleared; with an Armour of Ares (`+0x4b0`, its amount in percent) the
cut is reckoned but the blow posted uncut, so it only stops a blow it
would bring to nothing (with both, both blows go). 0x4184f0 then gives
the units the share `blow x load / (base + load + supplies)` (base
0x595920 by class: 25, 30, 35; load as above, a byte; supplies `+0x4d4`,
below) and takes it off the blow, which the structure takes afterwards
(LandObj, a fifth under a magic shield). Under the shield (`+0x15c`) the
units' share is cut to 20/100 for bomb kind 10 (the fireball) or to 100 /
150 if flaming; `+0x17c` cuts it to 20/100 too (not identified). The
weapons (items 3-8) take the share, the marines (0-2) `0x4ac1b0(kind,
the shooter's level, share)`. Barbarian structures (0x40e0f0) share
`blow x load / (load + 80)` with no cuts, the marines through 0x40ff00.

Spreading a share over units of a mask: points are `condition x
0x597cfc[item] / 100` (copied to 0x5c7438 by 0x44788c), the total their
sum, the blow at most the total. A number is drawn, `rand % (total -
blow)` from the fort's LCG (0 when the blow is the total), and the units
are walked index by index (to the longest place's count, taken when the
walk begins), each index through the places in item order, summing points
until the sum reaches the number. From that unit on each takes what is
left: if it holds, its condition becomes `(points - blow) x 100 / full`
and the blow is spent (gone at 0); if not it is gone (0x446620, the
place's later units moving up) and the blow goes on less its points. A
walk that ends with blow left draws again; the total is lowered only by a
unit that held. During a mending, units gone leave it (0x418430).

**Fire.** Each place fires on its own (0x4469c0 per tick in state 1),
reloading by 0x5c76e0 like a ship's item, with 0x446710: the sum of its
units' `condition x the A x 15 cache / 100` (x 1.5 with the Flame,
player `+0xa8`). A worn place hits weaker; an empty one does not fire.

**Mending.** A fort being repaired (command 0x1e → message 0x1389;
0x418300) notes each unit (item, index, its condition twice) at `+0x4cc`
(`+0x4c8` of them). The structure mends as other buildings (0x425100: a
percent at a time, `+0x120` the progress = (now - start) x 100 / (100 -
start), start `+0x114`); each tick 0x419ba0 (vtable +0x70; barbarians
0x40eba0, the same) lifts each noted unit to `start + (100 - start) x
progress / 100`, a unit worn meanwhile counting from that much less,
paying `0x597164[item / 3]` amber (30, 50, 70) and `0x597170[item]` x 100
fish (200, 200, 300 for marines) times the points gained / 100 when both
suffice; a unit no longer at its place and index drops the notes of all
gone (0x418430). A whole structure needs no repair, so its units are not
mended. At the end (vtable +0x74, 0x419910) a computer player's fort
(side value 3 or 4) buys back its default arming where it has room.
Barbarian structures pay too (0x40eba0), but only once mending, which
starts on their Repair command (0x1e, 0x40ef70 sending 0x1389 to
itself); no player or brain gives it to them, so in play they never mend.

**Gifts.** 0x419e00 (vtable +0x7c) reads the fort's store: kind 9 a
spell to cast (`+0x1b4`), 10 the Torch, 11 the Armour of Ares, 14 a
mirror shield (`+0x4b4`, its percent off a stare, 0x158bf), 15 the Cup
(`+0x4b8`), 16 the Fleece (`+0x4c4`), the rate `+0x4bc` the amount. Every
tick, when `(now - +0x4c0) x rate` is a multiple of 1000, its thousands
heal: the Cup the marines, else the Fleece every unit and as many percent
of the structure (`+0xa8`, not capped).

**Supplies.** Command 0x2d (sub-order 1, 0x419690) moves the settlement's
`+0x250`/`+0x251` (hundreds and a rest) into the fort's `+0x4d4`/`+0x4d5`
up to 1200 and 16 hundreds (0x417fd0); `+0x4d4` adds to the blow's base
and shows x 5 on its panel (0x417d20). Nothing fills the settlement's
`+0x250`: its constructor (0x43b000) zeroes it, only 0x419690 takes from
it and the settlement's panel shows it x 5 (0x43c650), so supplies stay 0
in play and the port leaves them out.

## Volcanoes

A volcano (VolcanoClassTy, `Volcano.cpp`; record field 4 its lava picture,
6 and 7 its cell, 8 its strength N, 9 a radius r) with N > 0 sleeps (45000
- 4500 N) x 80-100% ticks, then erupts (sound `shum2`, `VOLCANOERAPTION`
a frame every 2 ticks, looping frames 0x3c-0x77) for (15000 - 1500 N) x
80-100%, throwing a volley every (50 - 5 N) x 80-100% while the picture
shows (0x445e20, 0x445920, 0x4458d0; its own generator +0x18). A volley
(0x445ad0): of the cells within r of its cell (r + 1 on the far sides)
but its own 2 x 2, rand % N each on another cell, by rand % 6 a burning Q,
R or S, a G or F burning one time in two, or nothing; three draws more,
the last the sub-cell point (& 0x3f) and the trail (one in five); then
`eject` or `eject1`. The stones (0x499410, no shooter, class 0x25e) leave
the crater (0x5970c8 by the lava picture, from the cell's corner plus 0x46,
0x82), always land after 25 ticks on an arc of P x d^2 / 4000 (P by
distance, 0x599088) raised by 0x5baf28 (Q 55%, R 45%, S 35%, G 36%, F 54%)
and strike what is where they land with level 0's blow (Q 18300, R 21300,
S 23250; the crew's share by B / A). They only splash in water (305); none
leaves a crater. The fire rammer (type 10) throws one S at its prey as its
bow strikes (0x4c2566) and, boarded, Q and R at the boarder and two S at
itself at each step of its sail furling (0x4c3410 state 10): it blows
up. Letters 16-18 come from nowhere else.

A brain wakes a volcano (0x445e00, a direct call) with a strength: it
erupts at once, even if it already was, and after that eruption sleeps for
good (+0xb4). miss009 wakes, while the fire wizard lives, the flows of six
zones with 8 whenever the player's ships or forts are in one (every tick);
miss010, once the dragon's nest at (59, 4) is gone, the flows at (57, 4) and
(58, 5) with 9 (its third call names (63, 4), where no flow is). The game
word +0xa81 = 0xd0dd (never set in the retail game) stops eruptions.

**Mended ships** (message 0x5d98, sent only by 0x4b6d60 when a ship sails
out of a dock mended, or when a computer player cannot pay more): the
brain moves the ship out of its fleet into a member chosen by its type
(0x479300, op 2): miss007 boats and merchants (1, 1 or 2), warships (1, 4
or 5); miss008 boats (1, 3 or 8), merchants (1, 4 or 9), warships (1, 10);
miss009 boats (1, 2), merchants (1, 1), warships (1, 4); miss010 (1, 7),
(1, 6), (1, 8); miss011 (1, 2), (1, 2), (1, 5); miss012 and miss013 player
15 (1, 2) for boats and merchants, (1, 4) the rest, player 14 (2, 2) and
(2, 4) (the "or" a draw of the brain's generator).

**The brains' town orders** (0x47b130 through the member's land fleet):
code 1, the full town (table 0x5ac7c8's order 2 and both temples: port,
sage school, shipyard, smithy, the settlement made a town, the shipyard a
dock, school, temple of the sea, temple of war; a part done if it stands
already; flag 0x10 tries failed parts again), sent once by the Persian
yards of miss007-miss010 beside their ship building (miss010 its yard 2
only), and by those of miss011-miss013 in their state 0, before every ship;
code 0x60, order 96 (the first worn building of the member mended once;
after 10 idle passes the order fails, never sent again), at the start by
the builders of miss007-miss010.

**miss005's Persian raiders** (0x456580 north, member 6; 0x456e80 west,
member 7): each waits its rolled time ((8, 7, 5) or (7, 6, 4) + rand & 3,
x 1500 ticks) on patrol and rolls a spell (the Flame, the phantom or the
weapon shield) to cast on its ships as it attacks (0x47acf0 by member
(1, 0), unpaid). Then north, once both harpy nests at (23, 20) and
(26, 19) are gone, and west, once the barbarians' stronghold at (3, 29) is,
land unless the player holds the place: a medium fort founded at (19, 16)
or (8, 20) (0x47b000, fleet order 0x8c, waiting for its price; 3000
ticks). Landed, and the stronghold gone, they raid one of the player's
settlements or towns in (0, 0, 35, 35), else (27, 28, 9, 8), 6000 ticks,
and live in a second patrol about their fort, back to the first if it
falls. West casts its spell in its raids every turn and clears north's
(its slip at 0x456e80).

**miss006's Persians** cast on their raiders' and guard's ships through
their group's member 0 (unpaid, paid in full each): south raiders (1, 4)
the Flame as they raid, north guard (2, 3) the Flame as it fights at home,
north raiders (2, 4) the phantom or the Flame (rand & 3) as they raid. The
southern town (21, 68) has no war temple until its full-town order builds
one. Member (1, 7), an empty fleet (0x459fe0), watches four small zones
every 1500 ticks: the player there with more than 3 (2) ships about brings
waterspouts on set cells through the sea temple (0x47ab40, kind 6), twice
in all. The yards of miss004 (0, 5), miss005 (1, 0) and miss006 (1, 0) and
(2, 0) give the full-town order before every ship too.

## Projectiles

Each weapon fired sends a projectile off (BombClassTy, `bomb.cpp`; ships
0x4c0230): `BOMB<letter><state><variant>` (0x498e10), the letter the
item's (A for item 0; Greek fire flies as M, flaming, quick lime as N),
variant 1 a flaming shot. In flight (state 0) the composite has 16 cells
a frame, one per direction (0 east, counter-clockwise; 0x497e50), drawn
with the mask `1 << direction`; the frames step a tick. Letters 0, 1, 3,
4, 10, 11, 14 and 15 fly straight at `0x59757c[l] x 0x599b08[l] / 10`
pixels a tick; the others arc (0x497be0, 0x497250) over a flight length by
distance in tens of pixels from 30 (0x598dd8, seven shorts a row, the
last the length) at `0x599b1c[l]` a tick, rising by `0x5baf74[l]` (about
0.41 of the distance at 9000). Where it lands (0x4967b0, 0x4983d0) it
plays state 1 on a ship, 2 in the water, 3 on a building, 4 on the
ground, 5 on a sea creature or whale, 6 on a land creature (7, a flyer,
drawn as 6), a frame a tick, following what it hit; sounds: launch event
`0x5bbca4[l]`, landing `0x5bbcec[state + l x 6]` for states 1-4. A ship's
next shot with an item waits `0x5c76e0[item x 3 + level]` + rand(30)
ticks, the table being 6000 / the rate of fire (0x597a3c; 0x4ac0e0). A shot hits (0x4bf7f0) with `43 + ease - 33 x distance / (aim x
64)` percent, distance in pixels and aim in cells (0x598130: 9, 7, 7, 9,
9, 9, 13, 11, 11, 7, 9 by item, alike at every level); the ease is 25,
for a ship its type's (0x597acc: 5, 8, 20, 25, 15, 15, 10, 10, 10, 15,
10, 15), 1 with hero 3 aboard, and a phantom is hit at 20 % whatever the
range; Greek fire always hits. The roll's result is the shot's accuracy:
100 for a hit, the chance itself for a miss, made 0 when the ship carries
only one of that item and the target is a ship (0x4c0230). The shot is
aimed at the hull (0x4e07c0, leading a ship under way) unless that is 0,
when it goes for the picture's edge and flies wide; its blow is the
weapon's x the accuracy / 100 (0x4bfc20), a fiftieth when it is 0.

## Ramming

A ram (0x4c1c60) takes the two ships' heading vectors (0x5c4f4c) times
their speeds (`+0x59b`), the rammer's less the rammed's, times 0.707; half
its length is the impact in percent of a boat's top speed at the
player's rigging level (0x5c750c). Each side's blow is its ram quality
(0x598054[ram x 3 + the player's ram level]: 100, 110, 117; 125, 135,
145; 160, 175, 200) x its ram power over the other's (0x598024) x the
smaller of 500000 and its angle factor (0x598078 the rammer's, 0x598084
the rammed's, by the angle class of 0x4c19b0) x its hull / 100 x the
impact, over 2500; a powder barrel in the cargo adds its percentage. A
blow (0x4c1ad0) loses three quarters with hero 5 (Argus), the armour's
cut on the rammed ship, and falls by the shares: the hull's x 150 / 100
(0x598094), the crew's x 40 / 100 (0x598090), the weapons'. The odds of
0x4ba950 (the same terms, the rammer type half again) only rank targets
for the computer.

## Boarding

A ship that grapples another (contact state machine 0x4b5df0, `+0x6a9`)
decides the fight at once (0x4b5b00): each side's boarding strength
(`+0x53c`, 0x4bee60: the units' health x C x 15 / 1000 summed, 0x4b4e80,
C the third value of 0x597d2c, 35/68/100 for bowmen, javelin men and
swordsmen at level 0; plus `+0x544`/100 percent of experience; halved
while `+600` is set) plus 1, a and d; the boarders win with chance
(2a - d) / (a + d). Hero 4 (Hercules) or the ship of the dead (type 11)
always wins, unless the defender has the same. The attacker plays
`ABORD_0<heading>` if it won, `ABORD_1<heading>` if not, on its picture
slot 3, a frame a tick; when that ends (state 3) a win costs the boarders
`d + prize hull / 100` spread over crew, weapons and hull (tenths, ten
times) and the prize changes hands (0x4a91a0); a loss wipes out the
boarders and costs the ship seven tenths of it on the hull. (0x4a91a0
only counts the prize in the player's statistics.)

### Hooks, pulling alongside and the walkway

The boarder's contact machine (0x4b5df0, `+0x6a9`; the boarded ship's is
0x4c3410, `+0x71e`; each ship's contact record `+0x8e6`: type 200 a
ship, 0x32 a building, 300 a hook in the air) runs a game tick at a time:

- **State 1, closing in** (0x4b7f10): it scans a square of
  0x59aee8[type] = 5 cells and routes to a cell next to the prey; there
  (Chebyshev distance 1) 0x4b84b0 throws a **grappling hook**, at most
  once per `0x5c7740[player byte +0x90]` ticks since the last (`+0x6be`):
  the 25th entry of the reload table 0x5c76e0, i.e. item 8's reload at the
  player's rate of fire level for the strong weapons (85 ticks at level
  0, 66 above). The hit roll 0x4bf7f0 with item 9 always gives 100; the
  hook flies wide (accuracy 0) if the prey carries the magic wood
  (treasure 13 active, 0x4ba190), if it lies across a corner whose two
  side cells hold ships lying against each other (0x499ca0), or if the
  prey is under way and almost out of its cell (0x4b5140 x 3000 /
  `+0x59b` < 17). The hook is a projectile of kind 9 (`BOMBJ00`, 16
  directions; flight frame delay 2; it arcs like the others; launch
  sound event 276 `cross1`, landing 279 `strel1` on a ship, 277 `aas005`
  in the water); while it flies the boarder's contact is type 300 and the
  hook's picture slot 2 is a **line** (picture type 0x80, colour 0x6d)
  from the hook to the boarder's hull centre (0x496f40, 0x4964c0). State
  7 waits for it.
- **The hook lands** (0x4bbfc0, from the ship's bomb list): if the prey
  has no contact, accepts one (`+0x261` bit 0x1000) and is not a phantom,
  sinking or stone, both contacts point at each other (the boarder's state
  3, the prey's 4), the line moves to the prey's picture slot 2, and the
  prey gets message 2 (code 2, ProcPostRam 0x4aa2c0) putting it in state
  9. A hook in the water frees the boarder's contact (0x4983d0): back to
  state 1, the next hook after the reload.
- **Prey state 9**: unless the two ships still lie in neighbouring cells
  (distance exactly 1) and 0x499ca0 allows the corner, the grapple fails
  (its contact cleared; the boarder goes back to state 1). Otherwise it
  answers (record state 5), stops, and both ships run 0x4b54a0: each
  **slides** from where its picture is to its cell's middle plus a step
  towards the other, 10 pixels across (0x59a7c8), 13 up or down
  (0x59a828), 19 and 19 slanting (0x59a888), alike for every type, along
  a line of points (0x4b5220, padded to an even count) two points a tick;
  and **turns parallel** to the other: for the other ship east or west it
  turns north or south, north or south east or west, north-east or
  south-west north-west or south-east and the reverse, whichever is
  fewer rotations away (0x4b5390), the second on a tie, through the hull's
  24 rotation frames one a tick (0x4b57e0).
- **State 8 then 10** (boarder; the prey's 8 then 10 likewise): once the
  slide and the turn are done, and for the boarder once the prey is
  furling too (its state 10, 3 or 4), the sail furls: hull frames
  0x58 + 8 x heading to + 3, a frame once more than 4 ticks have passed (20
  ticks in all; 0x4b8aa0). The prey drops the hook's line when furled
  and waits (state 3).
- **State 0xb**: the boarder lowers the walkway `MOST0_` (picture slot 1,
  the composite layer of the facing `+0x6e7` = direction of the prey,
  0x59791d), 18 frames a tick each, its top-left at the hull's plus
  0x59a8e8[type][side 0x4b8a40][facing]; then the fight starts: sound
  event 83 (`abor6a`), the roll of 0x4b5b00, and `ABORD_<lost><facing>`
  on slot 3 at the walkway's top-left, a frame a tick (state 3). As it
  ends the blows above fall (and the prey's in its state 3).
- **State 0xc**: the walkway comes up, a frame a tick; **state 6**: the
  sail unfurls a frame a tick and the ship slides back to its cell's
  middle (0x4c3320, two points a tick); the prey does the same once the
  boarder is in state 6 (its states 4 and 6). The boarder's order is
  then done (message 8, 0x90).

A new order (`+0x6a2` < 0) before the fight rolls the walkway back up
and leaves; it is not checked during the fight.

### The ram's run

The rammer's machine (0x4c2230): states 1 and 2 route it (0x4b8d80,
window 0x5991c8[type] = 5) next to its prey (0x4c1f60: distance 0 or 1);
there, if neither ship has a contact (0x4c1760), the two contacts are
made and it keeps on into the prey's cell. Each tick 0x4c1870 tests its
**bow** — the point 0x599204[type x 24 + hull rotation] from its
picture's top-left — against the prey's picture: inside it and on a
solid pixel of its current frame (0x557640). Then **state 3**: the prey
is told to stop (message 2, code 3: its state 3), sound event 84 (`109`)
plays at the rammer's cell, and picture slot 1 shows `BOMBH30` (the big
catapult's splinters, 0x4e0340 via 0x4e0370) centred on the bow, a frame
a tick; at its second frame the rammer recoils 4 pixels against its
heading, and when it ends the blow 0x4c1c60 falls. **State 6**: the
rammer backs off to its cell's middle a pixel on each axis per frame of
its hull (delay 2: every third tick), types with 0x597a24 set (all but
boats, merchants and the fire rammer) stepping their under-way frames
backwards on odd ticks as they go; there the order ends (message 8,
0x90) and the prey's contact clears (it slides back to its own cell's
middle if it was moved).

## Training and repair at a settlement

A settlement's stock places (`[+0x244] + 0x23`, twelve) list units of two
bytes: kind and condition (percent). Message 0x1e with sub-order 2 (its
Repair) sums, over units below 100, amber `cost[kind / 3] x (100 -
condition) / 100` (0x597164: 30, 50, 70, ...) and time `ticks[kind / 3] x
(100 - condition) / 100` (0x5971a0: 70, 80, 95, 1200, 1300, 0, 400, 450,
...; 0x440fd0), takes the amber at once and enters state 2 (`+0x110`),
mending the units one after another as the time passes and all of them at
its end (0x441ba0). State 1 is training, a queued unit taking
`ticks[kind / 3]`.

Queues. A settlement (marines) and a workshop (weapons) each keep one
queue of item kinds, mixed, in a byte array (settlement +0x260, head
+0x254, tail +0x258; workshop +0x1ec, +0x1e0, +0x1e4). An order pays at
once and appends; an idle building starts timing the run (+0x124 = now,
0x43ba60). Each item is ready once the run has lasted to its end (the sum of
`0x5971a0[kind / 3]` up to it), the last ending the run. Message 0x3c,
sub-order 2 (sent by a right click on the button, 0x4f46c0, through the
button's second id 0x1d83 + slot) takes the last of that kind still to be
made off the queue and pays its amber and fish back; the time spent stays
with the run. The menus give each button the count of its kind still to
be made (menu byte 0x26 + slot) and the part not done (0x1a + slot, as
100 - done): a settlement's done is the time since the run started over
the time when the last of the kind is done (0x43c740), a workshop's the
time spent on the kind over all its time (0x435ec0). While a building
makes things, its buttons stay on.

Buttons in progress (0x4f2a90): after the picture, marks and caption, a
non-zero "not done" byte shades the 49x49 button (0x54a580) clockwise from
12 o'clock: the part beyond `done x 3.6` degrees has its palette indexes
remapped through the table at 0x5c1658 (a dark grey-blue). Then, on
buttons 0x21-0x39 with a count, the count in `NUMBER_FONT` at (3, 3).

Repairing a building (sub-order 1, a target id, checked by 0x43fcb0) is
for the settlement itself and its own buildings (0x441350: those whose
vtable +0x6c says they are damaged). It sends 0x1389 to the building, which
(0x423d60) marks itself busy (+0x10c) with work 0xff (+0x110) from its
current condition (+0xa8, percent). Every `cycle / 100` ticks (`+0xc0`,
200 for every player building) 0x425100 takes `cost / 100` amber
(0x42d8b0) and raises the condition 1%, the hull following
(`max x condition / 100`). It stops at 100%, or as soon as the owner cannot
pay. So a full repair takes 200 ticks and costs the building's price. Blows
taken meanwhile lower the target (0x423f9c).

Docks (`Shipserv.cpp`, 0x4e7640) serve ships alongside
(reloading); a dock's own queue (0x416f90, every 25 ticks) builds ships.

## Magic

A temple learns a spell in 200 ticks, 100 for computer players (side
value 3 or 4; 0x439260). Casts (`Magic.cpp` 0x42f580) are checked by
0x42ef90: reef, whirlpool, monster, lightning, fireball within 12 cells,
waterspout, phantom and the protections within 15. Protections and the
phantom add 3000 ticks to what is left (messages 0xda9a, 0xda97 to the
ship, 0x4dc630). Cast whirlpools and reefs go after 7500 + rand(7501)
ticks (Whirl.cpp 0x447ee0, Reef.cpp 0x4306d0); every tick they post
0xda95/0xda94 to the ships on their cell, which then wreck (flag 0x200,
state 4: 0x4ab720, 0x4aa280). A tornado (`TRNDSPR`, Tornado.cpp 0x43a380)
steps a frame every 2 ticks: rising 0-41, moving 42-83 looped, sinking
84-124; moving, it slides to its next cell over 16 frames, then strikes
the objects on it (0xda96, 30000, a fifth on a ship shielded from magic)
and picks a neighbour (0x43a6c0: weight 3 within one heading of the wind,
else 2), sinking at the first hop after 250 ticks of moving.

Lightning (LightningObjClassTy 0x42cfa0) strikes at frame 11 of its
picture over water (40 over land) and posts 0xda93 to every object on the
3 x 3 cells about its cell, whoever's: a ship unshielded from magic wrecks
(0x4dc630); a settlement or town (unshielded), lair, nest or harpy only
when it stands on the struck cell itself (0x441ba0, 0x414f00, 0x4207b0);
a sea monster loses half its strength (0x492360), a land creature a
quarter (0x413d70); other buildings ignore it.

A fireball (class 0x25d, SlightClassTy `slight.cpp`, made by message
0x16e1) sits on any free cell and is drawn a cell up, a frame a tick: it
appears (`SLIGHT1`, 60 frames, sound event 146), hovers (`SLIGHT2`, 60)
and strikes (`SLIGHT4`, 9, event 148) 2-4 times (0x4059e0), hovering
between, then dies away (`SLIGHT3`, 60, event 147). At a strike's frame 4
(0x405c20) it throws a shot (`BOMBK1`, sure to hit) at every enemy ship,
harpy, dragon, gorgon, sea monster, siren, castle, nest and lair within 2
cells, each carrying 88000 / min(targets, 4) / (strikes - 1). Lightning
shows `LTSPR_1` over water, `LTSPR_2` over land, and strikes at its
frame 11 or 40. Flame is the Temple of War's work 1 (0x439260): 250
ticks of brewing, then the player's flag (+0xa8) is set and 1500 ticks are
added to its time (+0xb0); while it is set the player's ships shoot
flaming shots striking x 150 / 100 (0x4c0230). View, lightning and
fireball casts add a sight point to
the fog (dark.cpp 0x48b9e0: cell, player, radius 2, 3000 ticks), dropped
when its time is up (0x48bae0).

A sea monster (UMonsterClassTy, `umonster.cpp`) has 50000 x its record's
percentage of strength (0x492360: max `+0x7c`, now `+0x80`, from fields
at +0x14 and +0x18; a summoned one 100 %, 0x493780). Idle, it waits
50-125 ticks a round (0x491830) and loses a hundredth of its maximum at
each round's end (0x491fa0); at none it goes. Lightning takes half its
maximum, blows what they carry. It bites in its attack state (0x490830)
the ship on the cell beside it, at a frame of its picture (0x5ba4e0):
message 0xf166 with 37500 x its strength left in percent (0x4900f0) / 100,
type 0x1fb, three quarters off with hero 5 and to a fifth under a magic
shield (0x4dc630), each bite costing it a twentieth of its maximum.

The Gorgon (MedusaClassTy, `gorgona.cpp`) rests 200-500 ticks (0x4019d0),
rises (`GORG1`, 30 frames), stares 1-3 times (`GORG2`, 28 frames, at
frame 13; 0x401f70) over the 3, 5 and 7 cells square about her, and sinks
(`GORG3`, 30; `GORG4`, 21). Each stare sends every ship there message
0x158bf with 88000 / the ships (22000 from five) and 150: unless it has
hero 8 (Perseus) or a magic shield, the ship turns to stone for 150 ticks
more (`+599`, `+0xa1`, `+0xa5`; freed at 0x4df260) and takes the blow (type
0x1fa), cut by a mirror shield's share (cargo item 0xe).

A siren (SirenaClassTy, `sirena.cpp`) hides 250-1000 ticks (0x403650),
surfaces and sings as her `SIRENA` passes frame 45 (0x403ba0), then dives.
Her song (message 0x15ca7, 750 and a share: 88 / the ships, 22 from five)
reaches every object within 7 cells; a ship without hero 7 (Odysseus) is
charmed for 750 ticks more (0x4a7390): it heads for her cell every 30
ticks, no faster than 7000 units, and adds the share each tick, taking a
blow of 300 (type 0x1fd) at each thousand (0x4a74c0; three quarters off
with hero 5, none on a ship shielded from magic).

Harpy nests (0x41d4a0) send out a new creature 55500 - 6000 x field 10
game ticks after the last fell. A harpy (0x41fbd0, state 5) strikes at
frame 7 of its picture with a sure shot drawn as L (letter 11) carrying
20 x item 4's level-1 blow (0x5c73dc); a dragon at frame 16 drops a shot
drawn as P (15) on the cell ahead of it, carrying no blow of its own
(0x41f1c0 picks a free cell beside it, land next to buildings first): its
teeth, which rise there as a cyclops, a minotaur or an amazon at random on
land (message 0x1497) or as a wild sea monster on water (0x493780).

A flyer keeps to its nest's range: the nest's fields 7 and 8 (5 and 5 in
most missions, 15 and 15 for miss010's dragon) are how many cells it may go
either way, and field 11 shortens its rests. Its states (0x41f680, run a
tick at a time by 0x41fbd0, moved by 0x41e4b0):

- **Rest (1).** It stays on the nest until `rand(1100 - 100 x field 11)`
  ticks have passed. A dragon then takes off only if an object of a side is
  in its range (0x41ef00); otherwise it rests again.
- **Take-off (2), fly (3), land (6).** Each leg goes to a cell picked by
  0x41e840. The cell lies within the 13 x 13 round mask at 0x595a30 around
  the flyer and within the range. Each cell weighs 1, plus 10 x the sum of
  (1 + count) over the marine groups aboard the ships beside it, and the
  pick is weighted random.
- **Moving.** Its route steps cell to cell. Each step spreads over one cycle
  of the flying picture, so it covers a cell per 15 ticks. When a step's
  heading differs, it turns first through `HRPSPR_4`/`DRGSPR_4`: 8 frames a
  heading, one a tick.
- **At each cell (0x41efb0).** It strikes (state 5) what is beside it,
  except on its way home.
- **After a strike.** It flies home (+0xc6), lands and rests. A leg without
  a strike is followed by another.
- **Picture delays.** By state at 0x595d10 (harpy) and 0x595d30 (dragon):
  0 or 1 (a frame a tick), except 2 for a harpy waiting. Take-off and
  landing play 35 frames for a harpy and 20 for a dragon.

## Land creatures

Lairs (`CreaturesFlatClassTy`, 0x414f00) send out another creature once
`(10 - field 9) x (20-25)` ticks have passed since the last, drawn afresh
at each look, up to field 8 out. A creature (0x4111b0) shows
`<CLP|MNT|AMZ>SPR_<state><heading>` with a frame delay by state
(0x59538c: 1, 25, 1, 1, 1, 2, 2 ticks; walking's 25 becomes 5 after the
first frame, 0x411520) and walks a cell a pass of its walking picture. It
wanders (0x413240) to any land cell (tile type 8) it can reach within a
round mask about where it stands (13 x 13 cells: 0x594ba0 cyclopes, radius
5; 0x594e44 and 0x5950e8 minotaurs and amazons, radius 4). A creature
throws (0x412740) a shot (`BOMB<letter>`, 0x4126f0: cyclopes G with a
ballista's level-1 blow, minotaurs O with a small catapult's, amazons A
with item 3's; the A x 15 cache's copy at 0x5c73a8), its blow kept by
health (0x594ab8: 1000, 947, 894, 837, 775, 740, 532, 548, 447, 376, 30
thousandths by tenths lost); it hits but from 2 cells off, where 1 in
(8 - cells) misses.

## Sheep

A flock (0x4331c0; record: cell x, y at fields 4-5, count at 7) keeps its
sheep on a 4 x 4 grid of 16-pixel spots in the cell, one to a spot, each
on a random spot facing a random way at the start. A sheep grazes
(`SHEEP_<dir>1`) a pass at a time, a frame every 2-4 ticks, resting
(50-100 % of 25) ticks between passes; every (50-100 % of 250) ticks it
walks (`SHEEP_<dir>2`, a frame every 3 ticks, 16 / (frames + 1) pixels a
frame) to a free spot beside it, if there is one. A sheep's picture has its
top-left 7 and 14 pixels up-left of its place.

## Closing in on shore

A ship loading amber or unloading (0x4afe50) closes in only when the
deposit's or building's cell lies straight beside its own (0x59791d gives
the direction): from its cell's middle it slides a set distance towards
it, at amber by direction (east 0x599c44, north 0x59a1c8, west 0x599ee8,
south 0x59a4c8), ship type and the shore tile's group (0x5bb870, 0x4b3770;
0 means not at all), at a port 10 pixels across or 13 up and down
(0x59a7c8, 0x59a82c), turning as it goes to lie side-on, whichever of the
two ways is nearer its heading. It records the way (a pixel line,
0x4b5220) and backs out along it; either way it moves two points of the
line, and a hull rotation while it turns, a frame of its picture (every 3
ticks; 0x4b02a0, state 0x13).

## Nets, walkways and porters

A fishing ship's net (0x4ae760, state 6) lies with its picture's top-left
at the ship frame's top-left plus 0x59afa8[type x 8 + heading] (x, y; the
same for headings four apart), a frame a tick. A walkway to a port
(0x4adef0, state 0xb) lies at the ship frame's top-left plus
0x59a8e8[(type x 2 + side) x 8 + facing]. A walkway to amber (0x4b0590, state 0xb)
lies at the ship frame's top-left plus, by ship type: east 0x599e88,
west 0x59a128, north 0x59a408 (shore groups 4, 5, 11) or 0x59a468 (6, 7),
south 0x59a708 (0, 1, 10) or 0x59a768 (2, 3); its porters (`ANOSIL_`) on
its top-left. Porters (`NOSIL_<loaded><n><heading>`,
39 frames of 71x38) are one picture laid over the walkway: the men walk
across its frame as it plays, a frame a tick, 1.56 s a trip; they do not
walk to the building. `NOSIL_<laden><way><facing>`: way 0 from the ship
ashore, 1 back. A ship holds its load in slots of 100 (0x4ad300); each
laden trip, started by a message (0x4aab00, type 5), carries a slot.

## Gathering

Gathering (0x4ad0e0): each take moves amber 100 (boats and merchants; 20
for other ships) or fish by ship type (0x59b2a8: boat 33, merchant 50,
others 20) or 50 of a leviathan; the barbarians (player 0x10) take a
third, and computer players double under a side and difficulty test.
Amber is taken once per round of the porters (0x4b0590 state 0xf, then
the `ANOSIL_` cycle in state 6), fish once per cast of the net (0x4ae760
states 2 and 6); walkways, nets and porters advance a frame a tick.

Unloading ends with 0x4add10 calling 0x4ad9f0, which credits the whole load
(amber at player `+0x20` through 0x4a8de0; fish at `+0x2c` through
0x4a8c30, spent in hundreds):

- **Fish** (`+0x47f`): one for one, but not at a fort.
- **Amber** (`+0x481`, through 0x4a8fa0 with class 400): `amount x 3 x 40 /
  100`, so 120 amber for 100.
- **A leviathan** (`+0x480`, class 0x192): `amount x 125 x 40 / 100`, so 50
  amber a unit, plus 2500 fish however much there was.

A leviathan (`WhaleClassTy`, record 0x494f90; life 0x494b90) holds 100 x
field 4: 100 in every mission, so the manual's 5000 amber and 2500 fish. A
ship takes 50 of it a cast of the net (0x4ad0e0), whatever its type. Caught,
it waits below `27750 - 3000 x field 40` ticks (state 5; field 40 is 2 in
most missions, about 14.5 minutes), then comes up whole on a random water
cell of its zone (fields 7 and 8 its corner, 10 and 11 its size; state 6).
It does this until it has shown itself field 39 times (2 in most missions).
Between dives while not caught it waits 25-75 ticks, as fish do.

## Sounds

`sound/sounds.dkx` holds 163 sounds (kind 2): a `WAVEFORMATEX` (18 bytes;
all PCM, mono, 16-bit, 22050 Hz), the sample data length (u32), four more
bytes, then the samples. Text entries `SND_n` list the sounds that may play
for game event `n`: a 24-byte header (u32s 0, flags, count, 10, 10, 0),
then `count` NUL-terminated names, the first preceded by a byte with one
high bit per name (0x80, 0xc0, 0xe0). `SOUNDLIST` lists the events.
Interface sounds are played by event through 0x525370; positional ones
through 0x50e570. A ship sinking (0x4dec40) plays event 3 (`deadsh`) if
it was the viewing player's and event 319 (`deny`, despite its name a
fanfare) if not. No sound was found for clicking an unavailable command.

Units' voices come in groups of four events, one per nation, `base + [0, 1,
3, 2][nation]`. Only nations 1 (Greeks) and 3 (Persians) are recorded, and
their sounds are alike. They play only for the viewing player's units:

- **Gathering.** A merchant sent to gather answers on the interface's voice
  channel (0x49d3c0, 0x525370 channel 2): amber 156 (`am…`, `come…`), fish
  160 (`fi…`, `fish…`), a leviathan 164 (`lev…`, `levi…`).
- **Treasures.** A ship reaching a treasure's cell plays 172 (`piw…`,
  0x4a3f40). Taking a treasure plays 168 (`pit3`, 0x4a5bc0).
- **Launch.** A ship launched plays 184 (`jb_d1`, 0x4b3ab0).
- **Buildings.** They have groups 212-251 (`re_o…`, played by the town,
  shipyard, workshop, sage school and temples' handlers). The
  heroes have a pair each from 252 (`ja1`/`ja4`, ...).

No voice was found for moving or attacking.

Music: missions' `TRACKS_MID` (Other(20)) is a playlist: a 28-byte header
(u32s 0, 0, 260, 4, 10, 10, 0; entry size and count) then four 260-byte
entries naming `Ancient1`-`Ancient4` (`sound/ancientN.mid`, standard MIDI
files written for the sound card's General MIDI synthesizer).
`TRACKS_CD` lists CD audio tracks 1-8 (not on the ISO; taken here as the
audio tracks 2-9 after the data track). Per-mission technology masks
(player bytes +0x45, +0x4e, +0x8bce) are not in `DESCRIPTOR` (its 104-byte
player blocks hold the human/computer flag, amber, nation, slot, fish and
the starting levels below; the rest of the entry is the same in every
mission), `HEAD_CUST`,
`ORIGINAL`, `HERO_ARR` (nine u32s, one per hero), `FLOT` or `LIST_OBJ`
(a list of map positions in 1/64-cell units); they may be set in code.

## Mission brains and technology permissions (partly decoded)

Each campaign mission has a script compiled into the executable
(`E:\MyProjects\ancient\brain\miss000.cpp`-`miss013.cpp`, plus
`missCOM.cpp` and `missweb.cpp`; functions roughly 0x449000-0x46f000). A
brain runs the computer players' groups of ships on a timetable (e.g.
0x44d0e0 calls a group routine every 50 ticks per group and ship), reacts to
events (message 0x5d94: a treasure delivered; kind 0x1b, the scroll,
delivered to a settlement or town (0x32/0x33) enables technology bits and
shows string 2104 "You can now upgrade to silver armor"), and shows the
story's messages (string resources 2104-2121: "You can now produce strong
weapons", "You can now build Bomber ships", "You can now research Greek
Fire", "You can now open the way to Lacridas", "Defend the fortress with
Hercules's sword!", "The parchment that describes the way to Colchis is
delivered to the town Iolcus", ...). `HERO_ARR` (nine u32s, by hero) marks
the heroes a mission asks to recruit: miss001 Jason, 002 Argus, 003
Theseus, 004 Perseus, 005 Odysseus, 006 Orpheus and Zetes, 007 Hercules
and Atalanta. The campaign missions' `CONDITIONS` only ever say "kill all
enemies"; their real objectives (bring the parchment to Iolcus, recruit a
hero, bring Jason and the heroes to a settlement, ...) live in the brains.

In the executable each brain's per-fleet handlers come first and its main
function (the message handler that creates the fleets, lays deposits and
dispatches each member to its handler) last: miss011's handlers are
0x46b8d0-0x46e110 and its main 0x46e8e0, miss012's 0x46fb80-0x472890 and
0x472fa0 (0x46f760 is its judge), miss013's 0x4741b0-0x477210 and 0x477930
(0x473f40 is its judge; the repair handlers 0x475a70 and 0x477190 serve
miss012 and miss013 alike). Mains: miss000
0x449b10, miss001 0x44af70, miss002 0x44d0e0, miss003 0x44fd90, miss004
0x453b00, miss005 0x457950, miss006 0x45b280, miss007 0x45f770, miss008
0x463240, miss009 0x4663d0, miss010 0x46a0f0.

Brains exist only in the campaign. When a mission starts (`Players.cpp`
0x51f680), objects of registered types are created (registry at 0x5c0d00:
type, constructor), among them always type 0x2427, `Ai\Boss_kib.cpp`
(0x448e30). That object judges the mission every 250 ticks (0x448d40,
0x448610): it counts each player's objects and checks the victory
conditions; it commands nothing. Only for a campaign mission (game +0xd80 =
1 and +0x3214 = 1) is a brain made, by mission number +0x3215: types
0x2913f-0x2914c for miss000-miss013, or 0x2928c (`MissWEBClassTy`,
0x47c910, a scripted variant of miss000) when +0xa81 is 0xddd1. Battles
and single missions have no brain; their computer players are run by
`AIPlayerClassTy` instead (see "Computer players" below).

The brains share a small runtime (`missCOM.cpp`, 0x478000-0x47c000). A brain
has groups of members (`this + 0x2c + group * 0xc` points to the members,
0xb4 bytes each); each member is a fleet created by 0x479200(group, member,
fleet kind, flags, flags, list, n): 0x4a0480 op 1 makes the fleet for the
brain's player (`this[0x39]`), op 8 adds ships to it from a list of (x,
y, a, b) entries. Each member runs a state machine: +0xc the state, +8 the
previous one, +0x10 "started", +0x18 the start tick and +0x14 a timeout.
Primitives, each issuing an order on first call and then waiting for the
fleet's report (0x4a0f60 query, 0x1f5 = done, result 0xb = arrived) or the
timeout before switching state with 0x479750:
0x47a3b0 wait(next, ms); 0x47a450 move(ok, failed, x, y, a, b, ms) (order
100, 0x479870); 0x47a570 gather in a zone (order 0x78, 0x479970: rectangle,
resource class e.g. 0x191 fish); 0x47a6a0 attack a zone (order 0x71,
0x479a40: rectangle from a per-mission table, strength by difficulty); 0x47a900
patrol a list of (x, y, pause) points (order 100 for each in turn); 0x4797e0
set the fleet's behaviour (order 0x72). The brain's tick (message 0) runs
member m of group g every 50 ticks at an offset of 5 * m; every 125 ticks
it judges the mission (vtable + 8, e.g. 0x448e90 for miss000, which counts
objects with 0x4c46c0) and ends it as won or lost. At the start (message
0x8d1) miss000's brain creates its three barbarian fleets and gives their
player 4000 amber and 2000 fish (2000 and 1000 on the second difficulty,
`+0x32a1`). Its members: 0x4495a0 waits, then attacks a zone from table
0x5ad940; 0x449680 fishes at a zone, sails, waits 1.5 s, fishes again,
sails, waits 3 s; 0x449810 patrols, and once the player has more warships
(types 4-7) than a difficulty-dependent number sails to one of two places
and attacks.

A ship's treasures each get a category when taken (0x4bd0c0, from
0x4b9740 by kind):

- **2:** kinds 0-5 and 8, barrels and the like.
- **4:** kinds 6, 7 and 27, handed over at a building.
- **8:** heroes' items (9, 17-26).
- **0x10:** lasting gifts (10-14).
- **1:** the Cup and the Fleece (15, 16).

A barbarian ship (player 0x10) marks everything 4. When a ship finishes
unloading at a building (0x4add10 -> 0x4ad9f0), its category-4 items, and
for a computer player its 0x10 ones too, go to the building (0x4bdc10 ->
0x424b30), which stores them if it has room (0x424af0, table 0x5c1094)
and announces message 0x5d94 with 1. Message 0x5d94 with 0 is a ship
taking a treasure (0x4b6e80). So the human never hands the Golden Fleece
over by unloading. miss010's brain waits for it (0x5d94, 1, kind 16).
Docks (0x4e7861) and towns (0x441480) also take items from a ship in the
load panel (below); a treasure handed over there is announced with 0x5d94
and 1 too.

Barbarian structures (`BarbarianClassTy`, 0x40e000-0x410000) build their
ships for nothing. The class pays only to mend its units (0x40eba0), while
ordinary shipyards check and take the price (0x416a40).

miss007 (0x45f770):

- **Members.** Group 0 is the barbarians (player 0x10, 12 members) and
  group 1 the Persians (player 0xf, 7 members).
- **The barbarians' yards.** Yard member 0 does nothing (0x406330 is
  empty). Yards 1-5 (0x45ca00) each build what fleet member (yard + 5) asks
  for (brain `+0xf5 + member x 4`: ship code `<< 16 |` wait). After the
  launch they wait the request's ticks, then clear it.
- **The barbarians' fleets.** Fleets 6-10 (0x45cbf0, 0x45d0b0, 0x45d640,
  0x45dbd0) ask for royal ships, bombers or ship killers (codes 0xca, 0xcb,
  0xce) while short of a target size. The target is re-rolled every turn
  otherwise: `rand(difficulty + 2..3) + 3..4`. The wait is `(60 or 70 +
  rand 61) x 25` ticks.
- **Patrols and raids.** Each fleet patrols its round (0x5b0ea0 and the
  like) with events: the player in its zone (attack it 1500 ticks); size
  at least the target after 7500 ticks (raid); fired on (answer 375). The
  raid goes against the player's strong fort at (15, 26), unless a
  1-in-(difficulty + 2) roll, else one of their towns (anywhere, or in
  (10, 10, 30, 30)), else the first fallback zone they are in (0x479f40).
  It lasts 6000 or 10500 ticks, called off at 2 ships left. With nothing
  found it waits `(30 + rand 41) x 25` ticks and looks again.
- **The Persians.** Their yard (0x45e510) serves the members' requests,
  resting `(40 + rand 31) x 25` ticks after each. Merchants gather amber in
  (0, 0, 12, 15) `(3 + rand(difficulty + 1)) x 1500` ticks; fishers fish
  15000. Two guards patrol 4500 ticks at a time (their zone: 6000; home
  (0, 0, 15, 10): 1500). Raiders attack a town in (40, 0, 13, 20) once
  `difficulty + 3` strong. Member 6 mends prizes.
- **Reefs.** A big reef (0x259) goes up at (41, 69) or (39, 70) at the
  start, and at (22, 17) the first time the player's ships enter
  (14, 17, 8, 14).

miss008 (0x463240):

- **Members.** Group 0 is the barbarians (player 0x10, 7 members) and
  group 1 the Persians (player 0xf, 14 members). Each member's request is
  a short at brain `+0x105` and on (ship code, -1 none); a yard serves
  them, builds with 0x47b130 for the member that asked, rests `(30 + rand
  41) x 25` ticks after the launch and clears the request.
- **The barbarians.** Yards 1 and 2 (0x461050) each hold one of the
  raiders' requests (`+0x107`, `+0x109`); yard 3 (0x461220) serves the
  fishers (`+0x10d`) before the home guard (`+0x10b`). The raiders (member
  4, 0x461480) ask for ship killers, royal ships or bombers while under
  `2 x difficulty + 3`, and patrol (0x5b18c0): the player in (9, 8, 15,
  14) is fought 1500 ticks; after 4500 ticks they watch three more zones
  (6000 ticks of fighting) until two ships are left. The home guard
  (member 5, 0x461720) does the same about (0, 0, 6, 12); it joins the
  raiders when they are down to one ship and the player is in (9, 7, 14,
  13), and while the player's ships are in (40, 0, 20, 20) it strikes at
  (0, 30, 10, 10). The fishers (member 6) fish (0, 7, 23, 15) 7500 ticks.
- **The Persians.** Yard 1 (0x461b30) serves members 3-7, yard 2 members
  8-12. Merchants (0x462020) gather amber `(3 + rand(difficulty + 1)) x
  1500` ticks and fishers (0x462270) fish 7500 or 15000, in the south-east
  and the north-east. Five patrols (0x462440, 0x462630, 0x4629b0,
  0x462c40, 0x462e30) patrol 4500 ticks at a time and fight the player in
  their zones (4500 ticks, their first zone 1500). Member 7 (0x4628d0)
  attacks (0, 30, 10, 10) with the ships handed to it (0x479370, op 0xe:
  one member's ships join another), then hands them to member 13, which
  mends them. Member 6 hands its ships to member 7 (at most three times)
  while it is `difficulty + 2` strong and the player is near the
  barbarians; member 10, having won a fight, hands its own and members 5,
  6 and 12's and restarts member 7. On attacking, members 5, 6 and 11
  have member 2 cast spell 15, 9 or 12 on their ships, and member 10
  spell 9 on member 7's (0x47acf0: kinds 9, 12, 15, 16, cast by 0x479c20
  on each ship, paid by member 2's player). These go to member (1, 2)'s
  war temple, and miss008 has none, so none is cast.
- **The start.** Easy and normal lay amber at (12, 41) and (17, 35), fish
  at (4, 34) and a leviathan zone at (25, 33, 5, 5) (0x4960b0: message
  0x167d with class 0x192, fields 7 and 8 the corner, 10 and 11 the
  size); normal adds amber at (12, 34) and (13, 36) and fish at (2, 42);
  hard lays amber at (54, 31), (49, 31) and fish at (55, 33) in the
  Persians' waters. The Persians get 5000 amber and fish on normal, 10000
  on hard (0x4a8de0, 0x4a8c30), technology bit 79 and workshop levels (the port: arming 2, ram, armour 2; from miss011 the workshop's rates 1 and 2, ranges and damages 2; the unique ships studied; bits 78, 79 every player has anyway).

miss009 (0x4663d0):

- **Members.** Group 0 is the barbarians (player 0x10, 8 members) and
  group 1 the Persians (player 0xe, 7 members). Requests are shorts at
  brain `+0x116`-`+0x11a` (the barbarian yards' own) and `+0x11e`-`+0x124`
  (Persian members 1-4; `+0x11c` the one being served).
- **The barbarians.** Yards 1-3 (0x464930) build the code in their
  request for member yard + 3 (a merchant, 0xc9, for member 7), rest
  `(50 + rand(41 - 10 x difficulty)) x 25` ticks after the launch and clear
  it. Member 4 (0x464b20) asks yard 1 for ship killers, royal ships or
  bombers while under difficulty + 3 (`+0x5b2324`, set by the constructor
  0x466260, also the number its size event waits for). It sleeps until the
  player first comes into (0, 9, 18, 5), then patrols (0x5b21e8): the
  player in (8, 1, 7, 10) is fought 1500 ticks; at difficulty + 3 after
  4500 ticks it watches three more zones (6000 ticks of fighting) until two
  ships are left. Member 5 (0x464da0) asks yard 2 for difficulty + 4 ships,
  patrols (0x5b23e8) fighting the player in (0, 15, 30, 20), and the first
  time it is at full strength strikes at (0, 30, 10, 15) for 2250 ticks.
  Member 6 (0x465010) asks yard 3 for `2 x (difficulty + 2)` rammers, royal
  ships and bombers; every turn the player is in one of six zones
  (0x5b24b0) and it is not attacking, it attacks that zone (`(difficulty +
  1) x 1500` ticks), otherwise (so every other turn) it sails home to
  (45, 41). Member 7 (0x465200) asks yard 2 or 3 for merchants (up to 2)
  and fishes (10, 20, 20, 20) or (35, 30, 15, 16) 7500 ticks.
- **The Persians.** The yard (0x465390) serves members 1-4 in order,
  resting `(30 + rand(41 - 10 x difficulty)) x 25` ticks. Member 1
  (0x465830) gathers amber in (50, 28, 20, 18) `(3 + rand(difficulty +
  1)) x 1500` ticks, member 2 (0x4659d0) fishes there 3000, both then
  sailing to (62, 36). Members 3 and 4 (0x465b20, 0x465da0) patrol
  (0x5b2668) 4500 ticks at a time and fight the player in (50, 30, 20, 16)
  and (39, 0, 23, 30) 1500 ticks; member 3 wants `2 x (difficulty + 2)`
  royal ships, bombers and boarders, member 4 `2 x difficulty + 5` of the
  Persians' first ship (0xd2). Once the fire wizard's castle at (30, 1)
  is gone (`+0x131`), member 4 takes member 3's ships and strikes at
  (20, 0, 20, 15) for `(difficulty + 5) x 1500` ticks. Member 5 (0x466060,
  the ships in (55, 0, 15, 5)) attacks the cells about one of the
  player's ships carrying heroes (ship `+0x259`) for 500 ticks while the
  player is in those waters. Member 6 mends prizes (message 0x5d97).
  Members 3 and 4 cast kinds 9 and 15 on their own ships when they fight
  (0x47acf0); a ship group cannot cast (0x4a09d0 drops it) and the Flame
  needs a spell the Persians never learn, so none is.
- **The tick.** About once every 50 ticks: lightning (0x42f580 kind 6, a
  free cast) at (10 or 12, 9) the first time the player comes into (0, 9,
  18, 5); on normal and hard, while the wizard lives, lightning on each of
  the player's temples (0x3d, 0x3e) in (10, 0, 40, 20) and message 8
  (0x445e00) to lava flows at given cells when the player enters six zones.
- **The start.** Easy and normal lay amber at (7, 43) and (18, 25), fish at
  (21, 34), a leviathan zone (17, 30, 5, 5) and a barrel of 2500 amber at
  (15, 16) (0x520c70); normal adds amber at (13, 1) and (28, 20), fish at
  (32, 14) and 2000 amber at (27, 11) and (36, 8). The Persians get 8000
  amber and 6000 fish on normal, 7000 and 4000 more on hard (0x4a8de0 and
  0x4a8c30 add), a sea monster at (33, 14) on normal and hard (0x493780)
  and a siren at (31, 14) on hard (0x404a20: class 0x1fd, field 4 its
  strength 100000, 5 and 6 the cell).

miss010 (0x46a0f0; constructor 0x469d90):

- **Members.** Group 0 is the barbarians (8 members), group 1 the
  Persians (player 0xf, 11 members); the human is player 5 (`+0xe4`).
  Member 0 of each group holds the builders (barbarians: (5, 9), (11, 4),
  (51, 19), (58, 29), (56, 33); Persians: (23, 24)) and runs nothing.
  Requests are shorts: the barbarian yards' own at `+0x104`-`+0x108`,
  Persian yard 1's slots `+0x10e`-`+0x114` (members 3-6) and yard 2's
  `+0x116`-`+0x11c` (members 6-9), each yard's member served at `+0x10a`,
  `+0x10c`. The constructor sets the patrols' size events (and the
  strength they ask up to): `2 x difficulty + 4` for barbarian members 4
  and 5 and Persian member 8, `difficulty + 4` for Persian member 5,
  `3 x difficulty + 5` for Persian member 9.
- **The barbarians.** Yards 1-3 (0x468050, at (4, 6), (53, 30), (60, 32))
  build as miss009's (merchants for member 7, warships for member yard +
  3; rest `(50 + rand(41 - 10 x difficulty)) x 25`). Member 4 (0x468240,
  the ships in (0, 0, 8, 4)) asks yard 1 for rammers, royal ships and
  bombers, patrols (0x5b2780) watching (0, 0, 10, 10) (1500 ticks of
  fighting), at full strength after 4500 ticks also (0, 10, 10, 20) and
  (10, 0, 20, 20) (6000 ticks, then a strike at (0, 35, 20, 8) for 6000)
  until two ships are left. Member 5 (0x4684e0) asks yard 2 for royal
  ships and bombers and patrols (0x5b2958) watching (50, 20, 15, 20), at
  full strength (40, 14, 25, 29); whenever the player is about the
  dragon's nest (40, 0, 25, 10) (0x479fd0) and it is not patrolling it
  attacks there 1500 ticks. Member 6 (0x4687a0) asks yard 3 for
  `difficulty + 5` royal ships (easy) or bombers, patrols (0x5b2b18)
  watching the same two zones and, more than two strong with the player
  about the nest, attacks about one of the player's ships carrying heroes
  500 ticks, else the nest's waters 1500. Member 7 (0x468ad0) asks yard 1
  for two merchants and fishes (0, 0, 20, 30) 7500 ticks, resting 1500.
- **The Persians.** Yards 1 and 2 (0x468bc0, at (26, 30) and (24, 14))
  serve their slots in order and rest `(30 + rand(41)) x 25` ticks; yard 2
  also gives a one-off order of class 1 (`+0x12a`). Members 3 and 6
  (0x4690b0) ask for a boat (easy) or merchant when they have neither and
  gather amber in (20, 30, 20, 13) and (20, 0, 10, 25) `(rand(difficulty
  + 1) + 1) x 1500` ticks, sail home to (26, 27) and (27, 9) (`1500 - 250
  x difficulty` ticks at most) and rest 750; members 4 and 7 (0x469320)
  ask for a merchant and fish (20, 20, 10, 10) and (20, 5, 15, 10) (member
  7 1500 ticks longer), resting `1500 - 500 x difficulty`. Member 5
  (0x469580) counts royal ships and bombers but asks for rammers or royal
  ships, patrolling (0x5b2c40) (20, 25, 10, 15), at full strength (20, 20,
  10, 20) and (0, 20, 20, 10). Members 8 and 9 (0x4697d0, 0x469a80;
  member 8 the ships in (19, 6, 6, 6)) patrol (0x5b2e08, 0x5b2fe8)
  watching (20, 10, 10, 10), at full strength (0, 10, 20, 10) and (20,
  20, 10, 20), a fight there going on to (0, 35, 20, 8). Member 9 checks
  its request slot `+0x11c` but writes the Persians' own ships (0xd2,
  0xd3) into member 8's `+0x11a`: it never gets a ship, member 8 does.
  Member 10 (0x469d10) mends prizes (0x5d97). Unordered launches (0x5d98)
  join member 7 (boats), 6 (merchants) or 8 (warships).
- **The tick.** About once every 50 ticks: once the dragon's nest at (59,
  4) is gone, message 9 (0x445e00) to the lava flows at (58, 4), (59, 5)
  and (64, 4) (cell + 4 of the grid); once the player has delivered the
  Golden Fleece to one of their buildings (0x5d94 with kind 16, `+0x130`),
  lightning (0x42f580 kind 6) at (58, 5), then at (59, 4) (`+0x12f`).
- **The start.** The nest (0x41dd30) and its fleece; a disposable magic
  (kind 9, parameter 6) in the barbarian building at (58, 29) or (56, 33)
  at random. Easy and normal lay amber at (30, 40) and (30, 28), fish at
  (3, 25), a leviathan zone (10, 20, 5, 5), a barrel of 2500 amber at
  (39, 40) and 2500 amber in the Persian yard at (26, 30); normal adds
  amber at (29, 12) and (39, 22), fish at (56, 23) and a barrel of 2000 at
  (27, 19). The Persians get 10000 amber and 8000 fish on normal, twice
  that on hard, technology bits 78, 79 and workshop levels (the port: arming 2, ram, armour 2; from miss011 the workshop's rates 1 and 2, ranges and damages 2; the unique ships studied; bits 78, 79 every player has anyway).

miss011 (0x46e8e0; members 0x46b8d0-0x46e110):

- **Members.** Group 0 is the barbarians (11 members), group 1 the
  Persians (player 14, 8 members). Requests are per-member slots in each
  yard's array (codes 200 + type, -1 none): barbarian yard 1's `+0x118`,
  yard 2's `+0x144`, the Persian yard's `+0x170`, each yard's member
  served at `+0x104`, `+0x114`, `+0x10c`.
- **The barbarians.** Yards 1 and 2 (0x46b8d0 at (54, 11), 0x46c0b0 at
  (8, 31)) roll a rest of `((50, 30, 10) + rand(31)) x 25` ticks, serve
  their slots in order (merchants, royal ships, bombers, rammers, ship
  killers) and clear the slot after the rest. Member 1 (0x46bb30) asks
  yard 1 for royal ships (1, 1, 2) and a bomber and patrols (0x5b3438)
  watching (49, 12, 15, 6). Member 2 (0x46bce0, the ships in (6, 29, 2,
  2)) fishes, then gathers amber, in (0, 21, 17, 15) 4500 ticks, sailing
  home to (2, 33) and resting 3000 after each. Member 3 (0x46be60, the
  ships in (55, 15, 3, 3)) asks yard 1 for royal ships (2, 2, 3) and
  rammers (2, 3, 2); short of them (bombers counting) it sails to (50, 15)
  and waits there 1000 ticks watching (49, 12, 15, 5), else patrols
  (0x5b35a0) watching (46, 16, 17, 8) too. Member 5 (0x46c310) asks yard
  2 for royal ships (2, 2, 3) and bombers (2, 3, 2) and patrols (0x5b36a8)
  watching (1, 27, 6, 8) and (7, 25, 8, 11). Member 6 (0x46c4c0) asks yard
  2 for royal ships and rammers; short of them it sails to (3, 32), else
  patrols (0x5b3810) watching (11, 19, 9, 17) too for `(rand(4) + (10, 8,
  6)) x 1500` ticks (`+0x190`), then attacks about one of the player's
  settlements or towns in (0, 0, 35, 36) 6000 ticks. Members 7-10
  (0x46c880-0x46cdc0, the ships at (0, 4), (0, 10), (1, 18), (1, 23)) hold
  their posts watching (5, 7, 9, 7) (then attack (0, 0, 17, 20) 1500
  ticks); one fired on fights the attacker 1500 ticks and sets the other
  three on the same area (`+0x1c8`, state 0x28). Zone fights last 1500
  ticks, answering fire 375 (member 6: 250).
- **The Persians.** The yard (0x46cf80, at (32, 17)) gives a one-off order
  of class 1, then serves its slots (boats to ship killers, 0xd2, 0xd3;
  0x34 builds class 0x30), resting 500 ticks. Members 1 and 2 (0x46d320,
  0x46d470) ask for a merchant while they have none and gather amber in
  (31, 23, 11, 7) and fish in (32, 18, 11, 12) 4500 ticks, sail home to
  (37, 24) and rest 1500. Members 3 and 4 (0x46d5c0, 0x46d830; the ships
  in (32, 21, 2, 2) and (36, 17, 3, 2)) ask for two royal ships and
  (1, 1, 2) of the Persians' ship 0xd2 or 0xd3, sail to (38, 18) or (34,
  23) and wait 1000 ticks, watching (30, 19, 8, 5), (34, 15, 6, 5), (28,
  10, 8, 6) and (22, 13, 8, 15); when member 3 sees the player in either
  of the last two it casts lightning (0x47ae10 kind 10 by member 0, free)
  at (38, 14) and at (30, 23), each if the map cell has an object. Member
  5 (0x46da00, the ships in (38, 20, 3, 3)) asks for bombers, royal ships
  and both Persian ships, rolls the time before it looks (`((8, 6, 4) +
  rand(4)) x 1500`, written into its event data 0x5b3ec8) and, if it has
  none, a spell (`+0x194`, 0 at first: `rand((10, 6, 3) + 1)` 0-1 flame,
  2 phantom, 3 weapon shield), patrols (0x5b3ce0) watching (30, 19, 8, 5),
  (34, 15, 6, 5) and (38, 16, 7, 13), and once five strong after that
  time either starts over, while the barbarians' yard at (54, 11) stands
  (`+0x19c`, cleared with a reset to state 0 by the brain's tick when the
  cell empties), or casts its spell on its ships (0x47acf0) and strikes
  6000 ticks about one of the player's settlements or towns, else about
  the first of the player's things, until fewer than three are left.
  Member 6 (0x46e090) mends prizes (0x5d97). Member 7 (0x46e110, no ships)
  waits `(3, 2, 1) x 1500` ticks and, until it has cast (1, 2, 3) spells
  (`+0x1e8`), looks through the player's things within 15 cells of the
  Persians' first temple (class 0x3e) and casts at the first it picks
  (0x47ae10, free): a fireball at a settlement (half the time), lightning
  at a town or temple 0x3e, a waterspout or fireball (a quarter of the
  time each) at a temple 0x3d, fort or class 0x34, 0x35, and at anything
  else with four of the player's things within 3 cells a waterspout,
  fireball or sea monster (one time in (20, 15, 10) + 1 each); a roll
  that picks nothing ends the look. Ships mended at a dock (0x5d98) join
  member 2 (boats, merchants) or 5 (warships).
- **The start.** Amber at (57, 26), (57, 35) and (51, 34) and fish at
  (53, 31) on every difficulty; the Persians get 12000, 17000 or 25000
  amber and 8000, 10000 or 15000 fish (added) and technology and workshop
  levels (the port: arming 2, ram, armour 2; from miss011 the workshop's rates 1 and 2, ranges and damages 2; the unique ships studied; bits 78, 79 every player has anyway).

miss012 (0x472fa0; members 0x46fb80-0x472890, repair 0x475a70, 0x477190):

- **Members.** Group 0 is the barbarians (player 16, 4 members), group 1
  player 15 in the north-west, group 2 player 14 in the south-west (8
  members each). Requests are per-member slots (codes 200 + type, -1
  none) at `+0x110`, `+0x138`, `+0x15c`, each yard's member served at
  `+0x108`, `+0x134`, `+0x158`. Members are dispatched at tick 5 x (group
  + member) mod 50.
- **The barbarians.** The yard (0x46fb80 at (50, 32)) rolls a rest of
  `((50, 30, 10) + rand(31)) x 25` ticks and builds merchants, royal ships,
  bombers, rammers and ship killers for free. Member 1 (0x46fde0, the
  ships in (50, 27, 2, 2)) asks for royal ships (2) and bombers (1, 1, 2)
  and sails to (50, 28) every 750 ticks watching (43, 21, 13, 19).
  Member 2 (0x46ff90, (48, 31)) fishes, then gathers amber, in (45, 18,
  11, 19) 4500 ticks, sailing to (47, 33) and resting 3000 after each.
  Member 3 (0x470110, (53, 26, 3, 3)) asks for royal ships (2) and
  rammers (1, 2, 2), rolls `(rand(4) + (10, 8, 6)) x 1500` (`+0x130`);
  short of them (bombers counting) it sails to (54, 26), else patrols
  (0x5b43e0) watching (45, 11, 11, 10) too, then strikes 3000 ticks (until
  fewer than three) at (37, 0, 19, 10) if the player is there, else about
  one of the player's settlements or towns in (0, 0, 56, 52), else about
  the player's first thing. Zone fights last 1500 ticks, answering fire
  250.
- **The Persians.** The yards (0x470620 at (15, 12), 0x471b50 at (4, 50))
  give an order of class 1 each round, serve their slots (boats to ship
  killers, 0xd2, 0xd3; 0x34 class 0x30) and rest 500 ticks. Members 1
  and 2 of each group (0x4709c0, 0x470b10, 0x471ef0, 0x472040) ask for
  merchants while they have fewer than two and gather amber and fish: in
  (8, 5, 16, 15) 4500 ticks, home (10, 16), rest 1500; in (0, 41, 15, 11)
  6000 ticks, home (0, 50), rest 3000. Member (1, 3) (0x470c60, (10, 10,
  3, 3)) patrols (0x5b44b0) watching (8, 5, 16, 15) and (10, 0, 14, 6).
  Member (1, 4) (0x470eb0, (22, 12, 2, 2)) patrols the middle sea
  (0x5b4560) `(rand(4) + (10, 8, 6)) x 1500` ticks (`+0x184`) watching
  (8, 5, 16, 15), (24, 8, 9, 4), (24, 12, 14, 15) and (38, 15, 6, 10)
  (answering fire 375 ticks, then back to the state before), then strikes
  4500 ticks about the last of the player's royal ships carrying heroes
  (`+0x259`) or a lasting gift (treasure kinds 10-14, 0x4b9740 group
  0x10); else it sends member 3 to state 0x19 (a strike at (35, 0, 15, 9),
  4500 ticks, while player 15 holds 5000 amber and 4000 fish) if the
  player is there, or to state 0x23 about one of the player's settlements
  or towns in (0, 0, 50, 72) (`+0x188`), which member 3's handler has no
  case for: it stays so for good. Member (1, 7) (0x4718c0, (19, 15, 2,
  2)) patrols (0x5b47d0) watching (8, 5, 16, 15); members (2, 3) and
  (2, 4) (0x472190, 0x472370; (1, 46, 2, 2), (10, 49, 3, 2)) patrol
  (0x5b4830, 0x5b4898) watching (0, 40, 19, 12). Member (2, 5) (0x472550)
  rolls `r = rand(4)`, a spell (`+0x1b0`: 0-1 kind 9 paid, 2 kind 12, 3
  kind 15) and `((20, 15, 10) + r) x 1500` ticks (`+0x198`; `+0x1b8`, set
  to 1 at the start, picks that table over (8, 6, 4)); short of three
  royal ships and (1, 2, 2) and (1, 1, 2) Persian ships it sails to (8,
  46), else patrols (0x5b4958); the player in (0, 40, 19, 12) brings the
  spell on its ships (0x47acf0 by member (2, 0)) and a fight of 1500
  ticks; (1, 34, 21, 7) and (19, 39, 6, 13) send it to state 0x1e (the
  area at `+0x40`). The other patrols ask for royal ships or bombers and
  both Persian ships ((1, 3): 2 royal, (1, 1, 2) 0xd2, (1, 2, 2) 0xd3;
  (1, 4): (0, 1, 2) royal, 2 and 2; (1, 7): 2 bombers, (1, 2, 2), (1, 1,
  2); (2, 3): (2, 3, 3) royal, (1, 2, 2), (1, 2, 3); (2, 4): 2 bombers,
  2 0xd2, (0, 1, 2) 0xd3). Zone fights last 1500 ticks, answering fire
  375. Members (1, 6) and (2, 7) (0x471440, 0x472890; no ships) are
  miss011's member 7 about player 15's and player 14's first temple 0x3e,
  both casting through member (1, 0) (waits 0x5b40b0, 0x5b4170:
  (3, 2, 1) x 1500; spells `+0x17c`, `+0x19c`, (1, 2, 3); odds (20, 15,
  10)). Prizes of both players join member (1, 5) (0x5d97); unordered
  launches join member 2 (boats, merchants) or 4 of their player's group
  (0x5d98).
- **The start.** Amber at (40, 0), (45, 0) and (53, 0) and fish at
  (52, 5) on every difficulty; player 15 gets 10000, 15000 or 25000 amber
  and 8000, 10000 or 15000 fish, player 14 15000, 15000 or 25000 and
  10000, 10000 or 15000 (added), and both technology and workshop levels (the port: arming 2, ram, armour 2; from miss011 the workshop's rates 1 and 2, ranges and damages 2; the unique ships studied; bits 78, 79 every player has anyway).

miss013 (0x477930; members 0x4741b0-0x477210, judge 0x473f40):

- **Members.** Group 0 is the barbarians (player 16, 4 members) in the
  north-west corner, group 1 player 15 in the middle channel (7 members),
  group 2 player 14 in the south (8 members). Requests are per-member
  slots at `+0x10c`, `+0x134`, `+0x154`, each yard's member served at
  `+0x104`, `+0x130`, `+0x150`. Members are dispatched at tick 5 x (group
  + member) mod 50.
- **The barbarians.** The yard (0x4741b0 at (2, 4)) is miss012's (rest
  `((50, 30, 10) + rand(31)) x 25`, free). Member 1 (0x474410, no ships)
  asks for royal ships (1, 1, 2) and a bomber and sails to (8, 5) every
  750 ticks watching (1, 1, 9, 7). Member 2 (0x4745c0, (6, 11)) fishes
  (2, 1, 8, 7), then gathers amber in (9, 11, 9, 8), 4500 ticks, sailing
  to (5, 1) and resting 3000 after each. Member 3 (0x474740, (8, 7, 3, 3))
  asks for royal ships (2) and a rammer, rolls `(rand(4) + (9, 7, 5)) x
  1500` (`+0x12c`); short of three royal ships, bombers and rammers it
  sails to (6, 1), else patrols (0x5b4f58) watching (3, 5, 19, 10) too,
  then strikes 6000 ticks (until fewer than two) at (35, 0, 15, 9) if the
  player is there, else about one of the player's settlements or towns in
  (0, 0, 50, 72), else not. Zone fights last 1500 ticks, answering fire
  250.
- **Player 15.** The yard (0x474be0 at (14, 28)) is miss012's. Members 1
  and 2 (0x474f80, 0x4750d0) gather amber and fish in (16, 19, 14, 17)
  4500 ticks, home (13, 23), rest 1500. Member 3 (0x475220, (20, 26, 5,
  3)) asks for royal ships (1, 2, 2), 0xd2 (1, 1, 2), bombers (2, 1, 2)
  and 0xd3 (2, 1, 2) and sails to (22, 25) every 750 ticks watching four
  channel zones (0x5b4fb8); state 0x19 sails to (42, 21) and on to 0x23, a
  strike of 4500 ticks at `+0x180` while player 15 holds 5000 amber and
  4000 fish. Member 4 (0x4754c0, (17, 20, 3, 4)) asks for boarders (0, 1,
  2), 0xd3 (2), 0xd2 (3) and royal ships from 0x5c7474 (in .bss, never
  written: none), patrols (0x5b5108) `(rand(4) + (11, 8, 7)) x 1500`
  ticks (`+0x17c`) watching the channel, (0, 18, 8, 8) and (4, 17, 22, 7)
  (events 0x5b5368; its fights read their zone from 0x5b5138, which has
  only the first four: the fifth reads the terminator (an empty area),
  the sixth a misaligned record, (20, 7, 18, 6)); answering fire 375
  ticks, then back to the state before; then it takes `+0x180` about the
  last of the player's royal ships carrying heroes or a lasting gift and,
  one time in two, strikes there 4500 ticks (0x23, until fewer than
  three); else `+0x180` = (35, 0, 15, 9) and, the player being there,
  member 3 goes to 0x19, or, about one of the player's settlements or
  towns in (0, 0, 50, 72), member 3 goes to 0x23. Member 5 (0x475a70)
  mends, member 6 (0x475af0) is miss012's caster about player 15's first
  temple 0x3e (waits (3, 2, 1) x 1500, spells `+0x174` (1, 2, 3), odds
  (20, 15, 10)).
- **Player 14.** The yard (0x475f70 at (44, 59)) is miss012's. Members 1
  and 2 (0x476310, 0x476460) gather amber in (45, 56, 5, 8) and fish in
  (28, 52, 22, 20) 6000 ticks, home (48, 66), rest 3000. Member 3
  (0x4765b0, (39, 56, 4, 3)) asks for royal ships (2), bombers (3), 0xd2
  (0, 1, 1) and 0xd3 (0, 0, 1), patrols (0x5b5428) `(rand(4) + (12, 10,
  10)) x 1500` ticks (`+0x190`) watching the home waters (35, 52, 15, 5),
  (35, 57, 8, 7) and (35, 64, 15, 6); on each turn, the player's first
  ship lying in rows 38-54 sends members 4 and 5 to state 0x37, a strike
  3 cells about it (`+0x1bc`) of 1500 ticks, once until member 3 or 4
  starts over (`+0x1cc`). Member 4 (0x4768e0, (38, 61, 2, 3)) asks as
  member (1, 4) (royal ships from 0x5c7480: none), rolls `(rand(4) + (10,
  8, 6)) x 1500` (`+0x194`); short of strength it sails to (46, 53), else
  patrols (0x5b54f0), watching the home waters and (14, 49, 24, 14) and
  (37, 41, 9, 8) (state 0x1e); then about the first of the player's royal
  ships carrying heroes, below row 44, one time in two, else in (0, 35,
  50, 18) if the player is there, it strikes 4500 ticks (0x19, until
  fewer than three). Member 5 (0x476dc0, (38, 65, 3, 2)) is miss012's
  member (2, 5) (spell `+0x1b0`, `((20, 15, 10) + r) x 1500` ticks,
  `+0x198`) asking for royal ships (3), 0xd3 (1, 1, 2) and 0xd2 (1): short
  of them it sails to (47, 67) and waits 750 ticks, else patrols
  (0x5b5720) watching the home waters and (24, 58, 13, 14), then casts
  its spell on its ships (by member (2, 0)) and strikes (0, 55, 8, 8) 4500
  ticks (until fewer than three). Member 6 (0x477190) mends, member 7
  (0x477210) casts about player 14's first temple 0x3e (spells `+0x19c`),
  through member (1, 0). Zone fights last 1500 ticks, answering fire 375.
  Prizes of player 15 join member (1, 5), of player 14 member (1, 6)
  (0x5d97); unordered launches join member 2 (boats, merchants) or 4 of
  their player's group (0x5d98).
- **The start.** Amber at (47, 0), (49, 1) and (38, 0) and fish at
  (36, 1) on every difficulty; the purses are miss012's (player 15:
  10000, 15000 or 25000 amber and 8000, 10000 or 15000 fish; player 14:
  15000, 15000 or 25000 and 10000, 10000 or 15000; added), with
  technology and workshop levels (the port: arming 2, ram, armour 2; from miss011 the workshop's rates 1 and 2, ranges and damages 2; the unique ships studied; bits 78, 79 every player has anyway).

Primitives not yet ported (the later brains use them):

- **0x47a7e0, repair** (ported as prizes sailing to a dock; `(group,
  member, ok, failed, builder, b, timeout,
  events)`). It gives the fleet order 0x82 (0x479900, 0x4cdf80; `builder` =
  the brain member that holds the docks, kind 11). The fleet's ships go to
  one of that member's docks, picked at random (0x4cdea0, 0x42cc40), and are
  mended there. Every brain has one member running only this, set up as
  behave, then repair (miss001 0x44cf20 with builder 7, miss003 0x452580
  with 5, ...). Ships join it when the brain's player captures them: the
  capture (0x4b6f20) sends event 0x5d97 and the brain adds the prize to that
  member (0x479300, op 2; miss002 group 1, member 8). Message 300 (a ship
  joined, 0x4a1340) sends it to a dock (0x4cdc00, 0x4cdb00). Once mended
  (message 0x134) it goes to the fleet named by the order's second
  argument, which is 0 in every brain, so it stays.
- **0x47ae10, cast** (`(group, member, kind, x, y, pay)`). The fleet casts
  spell `kind` (5-16, one per command; costs from 0x596844 by 0x47abb0,
  paid when `pay`) at cell (x, y) through message 0x3ee (0x47ab40). It is
  used by miss011-miss013 (miss006's waterspouts go through 0x47ab40).
- **0x479ed0** makes a fleet (0x4a0480 op 0xb) with a type mask;
  **0x479f40** and **0x479fb0** test the brain's event list against
  objects (0x49c3f0).

More `missCOM` pieces: event 0x47a060 fires when one of the fleet's ships is
hit by the watched player (at least a given time into the step) and sets
the attack area around the attacker; 0x47a2a0 compares the fleet's size
(0x479d60) with a number once the step has lasted long enough (0 equal, 1
at least, 2 more, -2 fewer, -1 at most) and 0x47a1e0 does so at once; a
brain may rewrite these records' numbers at run time (miss001's raid:
(8, 6 or 4 + random 0-3) x 1500 ticks, at least 2-4 ships, back home at
1-3). A fleet created with kind 11 holds buildings and builds: other fleets
count their ships by type (0x479dc0) and, when short, queue a request
(brain + 0x114, codes 200 + ship type), which the builder serves with
0x47b130 (an order to make the ship, then a wait from a difficulty table)
and the new ship joins the fleet that asked. State 0xffffff0 means "back to
the state before". 0x48b180 and 0x48ebc0 lay an amber deposit (class 0x190)
or a fishing ground (0x191) at a cell.

Campaign state carries over between missions: at the start (message
0x8d1) a brain restores the human's technology bits (+0x45, 12 bytes),
rigging, ram and armour levels, the workshop's levels (+0x8e, +0x95,
+0x9c), the arming (+0xa3), the spells known (+0xb4, +0xbc) and the unique
ships studied (+0x6e...) from the campaign record at game + 0x3361
(0x4a9740), and on message 2 puts the heroes it carries (nine 16-byte
entries at game + 0x33be: present, object id or cell) into the player's
buildings (0x424ed0). The computer builds ships for its empty fleets
through a production order (0x48b180 and 0x48ebc0: message 0x167e with the
ship type, a fleet and difficulty-dependent amounts from tables such as
0x5adb38).

Technology permissions are a bit array per player at +0x45 (bit n is bit
`(n ^ 7) & 7` of byte `(n ^ 7) >> 3`), set and cleared by 0x4a9810 (player,
bit, value) and tested with `xor dl, 7; bt`. What each bit allows (the
checks: 0x43d430 buildings, 0x43da20 equipment and ship types, 0x430f90
the sage school, 0x436030 and 0x4361c0 the workshop, 0x43c740 settlements,
0x417ff0 fleets, 0x437de0 and 0x438b80 the temples, spell bits from
0x42fd70):

| Bits | Allows |
|------|--------|
| 0-7 | building a port, medium fortress, sage school, shipyard, smithy, school, sea temple, war temple (the last three in towns) |
| 73, 74 | building a small, big fortress (the big one in towns); docks are free |
| 8, 10, 11, 13 | settlement commands 0x05 (load), 0x3a (become a town), 0x11 (repair), 0x12 |
| 37 | upgrading a shipyard to a dock (0x3b) |
| 15-17 | items of group item / 3 (marines, medium, strong weapons; also the arming studied, +0xa3 from 1, >= the group: medium weapons from the start) |
| 18-26 | workshop research of group g: range 18 + 3g, damage 19 + 3g, rate 20 + 3g |
| 29, 30 | armour level 1, 2 (silver, golden): fitting it (also the armour research), and researching it from level - 1 |
| 32, 33 | ram level 1, 2 (fitting); ram research level l to l + 1 needs 24 + l |
| 38 + k | ship type k (bombers also need armour research, rammers ram research, the unique ships 8-11 their study) |
| 50-53 | sage school: arming, ram research, armour research, rigging (speed) |
| 76-79 | sage school specials: Greek fire, quicklime (Greeks), the Persians' two |
| 54-67 | temple spells: spell 0-12 -> 59, 55, 54, 57, 58, 60, 61, 62, 63, 64, 66, 67, 56 |
| 71, 72 | fleet commands 3 and the group behaviours 0x70, 0x73 |
| 80 | building the Argo (command 0x7d) |

The campaign starts (miss000, 0x449160) by clearing 4-7, 10, 17, 21-37,
41, 46-59, 61-65, 67, 74, 76, 77 and setting 16 and 71. Each later
mission's start function (the brain's vtable slot after its main) grants
more: miss001 (0x44a440) 4, 21-23, 31, 34; miss002 (0x44bb60) 5, 10;
miss003 (0x44e210) 6, 7, 37, 60, 66; miss004 (0x451520) 27, 29, 32, 35,
51, 52, 59, 64, 74; miss005 (0x454f00) 17, 50, 57, 63; miss006 (0x45b020)
25, 54; miss007 (0x45c980) 26, 28, 33, 51, 55; miss008 (0x461010) 53, 58;
miss009 (0x4648a0) 24, 30, 36, 41, 52, 67; miss010 (0x467e30) 62;
miss011 (0x46e590) 46, 47, 76, 77 (each also re-grants the scrolls of two
missions before). A scroll of knowledge (treasure kind 27, field 7 its
number) delivered to a settlement or town grants, by its number: 1 (miss002,
held by an amazon lair) 29, 35, 52 ("You can now upgrade to silver
armor"); 3 (miss003) 17, 50 (strong weapons); 4 (miss004) 25 (their
power); 5 (miss005) 26 (their rate); 6 (miss006) 53 (speed of ships); 11
(miss007) 41 (bombers); 12 (miss007) 30, 36, 52 (golden armour); 13
(miss009) 46, 76 (Greek fire). Other scrolls only say "You have got a
scroll of knowledge". Brains also grant computer players bits (78, 79: the
Persians' specials).

Nests (HarpyNestClassTy, message handler 0x41d4a0) send out their creature
(HarpyClassTy 0x4207b0: a harpy from 0x45, a dragon from 0x44) at the start
(message 0x8d1) and, while record field 9 (creatures left) is above zero,
another once theirs has fallen and 0xd8cc - 6000 x field 10 milliseconds
have passed. The creature's states (0x41f680) name its sprites
`HRPSPR_`/`DRGSPR_` + state + heading (+ `S` the shadow, + `M`): 0 resting
(odd headings; dragons have none), 1 waiting a random time shortened by
its parameter, 2 taking off, 3 flying to a target cell, 4 turning, 5
striking, 6 landing, 7 dying. The flying sprites are tall: the creature is
drawn high in the frame, its shadow on the water below.

Lairs (CreaturesFlatClassTy, handler 0x414f00; record field 8 the most
creatures about, field 9 their speed) send a cyclops, minotaur or amazon
(CyclopClassTy 0x1f4-0x1f6) onto a free land cell beside them every (10 -
field 9) x 20-25 time units while fewer are out, to roam the island about
the lair. Their sprites are `CLPSPR_`/`MNTSPR_`/`AMZSPR_` + state +
heading: 0 appearing (out of smoke), 1 walking, 2 standing, 3 throwing (the
amazons' in two parts, `AMZSPR_3<heading><part>`), 4 turning, 5 vanishing.

Command buttons (`CBUT_<name>_00`-`_30`, 49x49; the last frame greyed for
a disabled command) are animated sprites: the command panel (0x4f31b0)
loads them through 0x542940 type 0xb (0x542f20) with every frame, or, when
a display setting asks (panel byte +0x15c set, or game word +0xa81 =
0xd0dd), with only frames 0 and 30.

Command 0x7d builds the Argo (button `CBUT_BLD<nation>ARGO`): shipyards
offer it after their ship types (0x4164e0) while permission bit 80 is set,
at a royal ship's price; miss002's brain sets bit 80 when Argus enters one
of the player's buildings (message 0x5d99), and building the Argo clears
it (0x416a40).

Commands 0x7e, 0x7f and 0x80 are "Sell 100 items of fish", "Buy 100 items
of fish" and "Sell town" (buttons `CBUT_SELLFISH`, `CBUT_BUYFISH`,
`CBUT_SELLTOWN`; button switch 0x4f08a0, jump table 0x4f1938 by command -
1). Settlements offer Sell town in slot 7 (with command 0x12 in slot 6,
both behind permission bit 13; command 0x12 is "Sell item", `CBUT_SELL`);
where the fish trade is offered is not known (perhaps only between
players in multiplayer). The town's order handler (0x43b290, internal ids
0x138a-0x138e) trains items with 0x138d (paying 0x597164 amber by group
and 0x597170 fish by item) and, for Greek towns, answers 0x138e with
message 0x5d9c (the town's owner and cell), likely the handing over of a
sold town; what selling pays is not decoded.

At a mission's end the brains' shared slot 0x478e30 fills the statistics
at game + 0x3309 (twelve dwords, shown by 0x539b20 with strings
1047-1058): enemy ships destroyed (from the player's kill counts by ship
type, player + 0x704), boarded (+0x894), enemy and monsters' buildings
destroyed (by class index 0-23, 16-23 the monsters'), monsters killed (ten
kinds, +0x7a0), ships and buildings lost (+0x3e4), fish caught, amber
earned, amber left, treasures found and the score: ships and monsters by
their points (0x59b358: the ships' amber costs; monsters 700, 400, 200,
200, 600, 1100, 1300, 800, 200, 900 by kind), less the ships lost, plus
the amber earned, a fifth of the fish and 600 per treasure, a tenth of
it, added to the campaign's running score. Buildings score nothing: the
price lookup 0x42d8b0 is given the index, not the class.

Some objects are created by the scripts at the start: 0x411010 (world,
class, x, y, level, a, b) makes a castle record (class, 2, 4, 1, 0, x, y,
100, ..., fields 22-24 level, a, b): miss007 the wind wizard (0x42) at
(46, 73) or (16, 74), level difficulty + 5, a = b = 2 x difficulty + 8;
miss009 the fire wizard (0x43) at (30, 1), level 2 x difficulty + 5, a = b
= 5 x difficulty + 10. 0x41dd30 makes a nest (class, 2, 4, 1, x, y, 0,
...): miss010 the dragon's (0x44) at (59, 4). Message 0x158c1, sent to the
object at a cell with a five-dword treasure entry, adds the treasure to
its inventory: the fire wizard gets scroll 13 and a disposable magic, the
dragon the Golden Fleece (kind 16). 0x520c70 lays a treasure on a cell
(miss010: 2000-2500 amber).

The wizards (CastleClassTy, `Castle.cpp`; constructor 0x410ca0, per tick
0x410790) cast real spells (0x42f580, with a free cast) on a cell chosen
near the castle:

- **Which spell.** The sea castle (0x42, the wind wizard) casts spell 3, the
  waterspout; the war castle (0x43, the fire wizard) spell 7, the fireball.
- **Where.** A cell within fields 23 and 24 either way of the castle. Each
  cell weighs the sum over the objects of a side within a cell of it: a
  ship or a building 0x34-0x36 counts 10 less its distance from the castle,
  any other building 1. The wizard's last attacker counts four times. The
  pick is weighted random.
- **When.** The castle's picture shows the cast at frame 7. The next comes
  `(1250 - 125 x level) x (80 + rand(21)) / 100` ticks later, level being
  field 22; at half that delay for 3000 ticks after he is attacked (+0x1f4).

Buildings, lairs, nests and monsters hold treasures in their records: a
count, then five dwords per treasure like a treasure record's fields 4-8
(kind, holder -1, 0, parameter, amount), at field 10 for lairs (0x46-0x48),
11 for fortresses and barbarian structures (0x34-0x36, 0x3f-0x41), 12 for
nests (0x44, 0x45), 17 for sea monsters and 60 for settlements and towns.
They fall when the holder is destroyed; this is where the campaign keeps
its scrolls and most heroes' items.

## Computer players (`AIPlayerClassTy`; decoded, ported in `scene/computer.rs`)

`ai\Plr_fsrv.cpp`, `Plr_kib.cpp` and `Plr_mail.cpp` (0x47d230-0x489990,
vtable 0x59745c, message handler 0x488960, object type 0x29596, 0xe68
bytes, constructor 0x487cc0). On message 8 (0x522d50, `Players.cpp`),
unless the mission is the campaign's (game +0xd80 = 1) or +0x30d9 = 2, one
is made for every player whose side (+0x1a1d by player id) is 3; for side
4, a player at another machine, the game only queues a network message
(0x51eef0 appends type, length and data to the outgoing buffer). Mode
+0x30d9 = 2 is set with game +0xd80 = 2 by the application's command 0x103
(0x4ec1a0, toggling +0xd85, keeping the player's id at +0x30d5); in it no
computer players are made and the interface panels are not updated
(0x4bc820, 0x4b7190, 0x49ab30, 0x448610). No menu here sends 0x103; the
port has no such mode. Its name in a save is `AIPLAYER%02d` (0x487c90). Message 2
gives it its player id (+0x1c), its difficulty (+0x20, the player's
+0x1a31 byte), the map size (+0x24, +0x28) and seeds its generator (+0x18,
the usual LCG `x * 0x41c64e6d + 0x3039`, high half) from timeGetTime.
Message 0 (each tick, while the player has objects) runs every flotilla;
+0xe1c is the game tick (game +0xde).

**Set-up** (message 0x8d1, 0x487230). Flotilla 0 is the manager (+0x30),
1 the forts (+0x34). The player's towns, then settlements, become bases
(+0x60, 0x15c bytes each, at most 9, at least one; +0xdf8 how many) and
join the manager's fleet; forts (0x34-0x36) join flotilla 1. Then the
commander (role 4, +0x3c), the fleet (role 9, +0x40) and the reserve (role
10, +0x38), and for each base the amber and fish gatherers (2, 3), guards
(6), defence (7), raiders (8), builders (5) and spare ships (11). A
flotilla (0x483e10, 0x34c bytes) has its role (+0), navy fleet (+8), base
(+0xc), a period of 25-75 ticks (+0x14: `rand 51 + 25`; it runs when +0x18
+ period <= now), its task (+0x6c) with 50 parameters (+0x70), the task's
state (+0x20) on a ring of ten (+0x24, 0x482bc0: 0x7fffffff goes back),
the step's start (+0x50, +0x54 length, +0x58 time) and a mailbox (+0x138:
letters of 0x35 dwords, kind, time; the newest is read first, 0x4897a0).
The ships it starts with go to the flotilla whose want table asks for
their type first (the table's first entries sorted by priority, 0x4803a0,
0x480480), else by type (0x484170). The enemy is then picked (0x4844b0):
every other player weighted 250 (a local human, side 1), 200 (a remote
human, 2) or 10 (a computer); 0x484770 picks again once it has nothing
left, and a player who keeps hitting it (0x4845e0: more than 1-3 hits for
a human while the enemy is a computer, 8-15 otherwise, 100-150 for a
computer while the enemy is human) becomes the enemy.

**Roles and tasks** (0x4829d0; tasks 0x47f8c0):

| Role | Handler | Task | Does |
|------|---------|------|------|
| 0 manager | 0x480580 | 2 (0x47d2e0) | each base's building script and founding; serves the ship requests (0x480270) |
| 2 amber, 3 fish | 0x480680, 0x480840 | 4 (0x47dc60) | gather (order 0x78) in the area about the base holding more than 3 deposits (from 4 cells, +2 a time, 0x485380) for 7500 ticks (fish 15000), sail to the zone (100) for 750-1500, then pick the resource: amber gatherers turn to fish while fish < 3000 and amber > 4500; fish gatherers to amber when fish > 3000 and amber < 2000, or 1 in 11; 7500 ticks for their own resource, 3000 otherwise, fish x 3 |
| 5 builders | 0x480a00 | 6 (0x47f200), 5 (0x47dfb0) | found a guarding fort (order 0x8c; the biggest the base's town may put up, 0x480070 asking 0x43d430 with its items checked: stone 0x36, 1 in 4 middle instead; else middle 0x35; else wood 0x34; none, the task gives up) at `LAiFindPlaceGuardianFortress` (0x409b30), unless there are more than map cells / 70 + 10 forts; when done or impossible, task 5: hunt treasure (0xbe, 3000-4500 ticks), sail to the zone (250-500), hunt leviathans (0x192, 6000-10500), gather (500-1500; fish while fish < 1000), the round cut to 2/3 on normal, 1/2 on hard; then task 6 again |
| 6 guard | 0x481a10 | 7 (0x47eed0) | guard its zone (order 0x6e) |
| 7 defence | 0x4815a0 | 8 (0x47eb70) | guard its zone; given an area (help, the base attacked, the grand attack) attack it (order 0x70) for 9000-18000 ticks |
| 8 raiders | 0x480c40 | 9 (0x47e3c0) | guard the zone 6000-15000 ticks until as many ships as the target size (0x485650), wait 1000, attack one of the enemy's towns (100) or settlements (50) (0x485be0) 6000-15000 ticks, back at 3 ships; on normal and hard with more than 4 ships they strike early at a town where the enemy is weaker (0x485fc0) |
| 9 fleet | 0x481100 | 10 (0x47e7c0) | as the raiders, target size 0x485960, no early strike, waits 1500 |
| 10 reserve | 0x481e60 | 0xb (0x47f460) | sail to the base (500-1000 ticks), to the docks for repairs (order 200 mode 2, 6000-12000), then hand each ship to the flotilla wanting its type most (0x486910: 0x4866d0, else 0x47fd50) |
| 11 spare | 0x481fe0 | 0xd (0x47f6b0) | the same without handing out (mode 1) |
| 4 commander | 0x482160 | none | the grand attack, founding settlements, spells |

Any flotilla with ships may be mailed task 0xc (0x47f050): found a
settlement (order 0x8c, class 0x32) at a cell. Letters (kind 1 a task, 2
start over, 3 the zones changed, 4 the home area changed, 0x65 the base
is attacked: defend its area with task 8). Roles 5-9 ask for ships on each
turn (0x4800c0; gatherers always) and, while guarding, send ships worn
below 25-45% (boats, merchants) or 50-75% (warships; or crews below 35-65%,
0x4bc590) to the reserve every 375 ticks when the base has a dock
(0x486970).

**Want tables** (by difficulty, 0x47f980; the Persians, player ids 12-15,
have their own for roles 6, 8, 9): groups of 16-byte entries (ship type,
count, flag, priority), each group ending in -1. For each group the first
type the base's town can build (0x480000 -> 0x43da20 case 4) is asked for
when the flotilla has fewer than `count` and the reserve none; the request
(+0xe00, 0x90 bytes) keys on `priority - flag x 2500 + 22767`, waits 12-50
ticks, is sent to the town as order `0x50 + type` (0x5c for boats), and is
forgotten 1500 ticks after it is served. Tables: gatherers 0x5b5fa0,
0x5b7030, 0x5b8020 (a merchant, a boat, a second merchant); builders
0x5b68e0, 0x5b7590, 0x5b8760; guards 0x5b6010/0x5b61e0 (Persian),
0x5b70a0/0x5b7250, 0x5b8090/0x5b82f0; defence 0x5b63b0, 0x5b7400, 0x5b8550;
raiders 0x5b6540/0x5b6710, 0x5b7620/0x5b77b0, 0x5b8810/0x5b8a90; fleet
0x5b6970/0x5b6b80, 0x5b7940/0x5b7b90, 0x5b8d10/0x5b9000.

**Bases.** Each base runs a building script (0x47faf0: the first base easy
0x5b6d90, normal 0x5b7de0, hard 0x5b9330; the others 0x5b6f30, 0x5b7f70,
0x5b94d0; after a whole round 0x5b6fe0: 2, 96, 52, 48), 16-byte entries
(order, 0, weight, 1), each sent to the town as message 1000 with priority
`22767 + weight + 1` (+100 for order 2); when it reports done or failed the
base waits 1500 ticks (750 on hard) and takes the next. If the town lacks
its port, shipyard or dock, sage school or smithy, the script starts over.
A base without a settlement (lost, or new) founds one: early in the game
(before `(3000 + rand 3001) x 5` ticks) the first base the best site within
`ceil(sqrt(cells / players))` of its ships' mean cell, if any is
(0x40cb60), otherwise its stored site while it still serves (0x40ca90 with
0x42e1e0: 2 x 2 free inland cells, shore in the ring about them), else the
best on the map, by a flotilla of the base picked by size and nearness
(0x484df0). The search (`LAiFindTownPlace`, `_L_Ai_tn.cpp` 0x40c5f0): a
site is the top-left of 2 x 2 free inland cells (edge mask 0) with a shore
cell in the 4 x 4 about them, in a land region of at least 9 inland and 2
shore cells; every other cell adds its influence to a weight map (0x40b050,
0x40bbf0, the table 0x5945c0): others' settlements -10 within 10 (at their
cell), their forts, barbarians and buildings -10 within 10 for each cell,
their ships -5 within 5, the player's own settlements and buildings -5 + d
within 5, amber 5 - d and fish and leviathan zones 10 - d (each cell of
them) on the water they touch, castles -20, nests -20 + d, medusas and
sirens -20 + d within 5, sea monsters -20 over their area, lava -10 x its
strength, and lairs -30 x their spread (0x40b5a0, 0x4080e0: over the box of
their land region, a walk of half cells, a cell not land, of type 1 on
layer 5 or with anything on layers 6-7 shut, else the half cells on its
shore edges, flooded from the lair's cell, which does not count; each
cell reached 1, and 1 more for each reached cell whose 13 x 13 disc
0x594ba0 holds it); a site scores the sum over the 10 x 10 cells about it (shifted
inside the map), highest first (VC6 qsort). Nests weigh over their reach (record fields 7
and 8, one more; nest +0x200, +0x204), sea monsters over their waters
(fields 11 and 12 about them, else the whole map). The port counts players
by the owners present. Each base also has an area (its town's buildings, 2 cells
about, 0x484aa0 with 0x4218f0) and four waiting zones (0x484900 with
`LAiFindDispositionZones` 0x4093a0, 0x408bf0: in the 31 x 31 cells about
the town, a square of side s, s x s >= n + n / 4, holding water for n = 5
to 12 ships, ranked by cells beside the base's ports and yards, then
weights: the influence map of the settlements' sites (0x40b050, 0x40bbf0)
over the 31 x 31 cells with the table 0x594470 (the player's own towns +20
within 10, others' towns, forts, barbarians and ships -20 within 10,
others' buildings and deposits nothing, lairs x -10, nests -10 + d, castles
-10, medusas and sirens -10 + d within 5, lava -1 x its strength). Each
cell is closed to deep ships (1), beside the base's ports and yards (8), on
a lane (9: the paths from each port's front to each amber, 0x558fa0,
0x559470), taken by an earlier zone (10) or open (0). A square needs as
many passable cells as ships (and, on the first pass, none taken); it beats
the best (0x408bf0) with fewer cells beside the harbour, then more weight
(over its passable cells), then nearer the town (`max + min / 2` to its
middle), then more open cells, fewer on the lanes, more passable).

**Town orders** are run by the town's land fleet (`LFleet.cpp`,
0x42b910, every 50 ticks, by priority): the table at 0x5ac7c8 (7 dwords:
order, flags, ..., parts, part list) splits each into parts of 0xcc bytes
(operation, flags (1, 2, 4 wait for the price; 0x20 done if it is there
already), class or number, field 20 what it waits for: 0 nothing, 1 the
part in field 21, 2 every part before): 0x3e9 a building (0x428a70; the
town places it, `m_build.cpp` 0x42d9c0), 0x3ea ships (0x428f20), 0x3eb an
upgrade (settlement to town, shipyard to dock), 0x3ec repairs, 0x3ef a
study (0x42a860: 1 arming, 2 ram, 3 armour, 17 rigging, 18-26 the
workshop's range, damage and rate of fire by group, 27-29 the smithy's
regular, silver and golden rams, 30-32 its armour kinds, 33/34 the
temples' spells, 35-38 the unique ships). Orders used: 2 port, sage school,
shipyard, smithy, town, dock, school; 3 the temples; 48, 50, 52 a medium,
small, big fort; 96 repairs; 97 a port; 98 a shipyard and dock;
117 the temples' spells; 118-120 arming, ram, armour; 128-130 the
workshop; 131 the smithy's armour kinds, 132 its ram kinds; 133 rigging; 134 the nation's unique ships (0x73 Greek, 0x74
Persian). A part waits for its price as long as it takes and gives up
after 10-25 passes otherwise.

**Fights.** Message 0x5d9c (an object of the player hit): at most every
50-100 ticks. A fleet weaker than the enemies about the shooter
(0x486b70) asks the others (message 100: guards with more than 3 ships,
defence without a target, raiders and fleet not attacking) and one,
weighted 110, +90 if strong enough, -90 of another base (0x486e80), is sent
to attack there (task 8, 9000-18000 ticks); pairs helped/helping are kept
at +0xe04. A town hit makes its base's flotillas defend it (letter 0x65)
and asks the commander for spells (letters 6, 7: 0x4839e0-0x483c40, magic
order 0x3ee). The commander (0x482160) every turn: after the first attack
time (easy 22500-27000 ticks, normal 15000-22500, hard 13500-18000) and
then every 1500-3000 ticks, if the free warships (message 0x66) number
the enemy's warships rounded down to 5 plus 12 (at most 30 easy, 35
otherwise), it mails task 8 against one of the enemy's towns (0x487690) to
every guard, defence, raider and fleet flotilla (mask 0x3c0), 15000-22500
ticks; and every 7500-12000 ticks, below `map cells / 500` bases (at most 1,
2, 3 by difficulty), once it has studied a unique ship (player +0x6e or
+0x7e easy, +0x76 or +0x86 otherwise) and has ships, it has the nearest
flotilla found a settlement (0x484d60, 0x484f60).

The terrain helpers of `artem\_L_Ai_*.cpp` are ported: the town's placing of buildings and the
guarding forts' places (below), the enemy groups, and the object-kind
masks (see below).

**Enemy groups** (`LAiFindEnemiesGroups`, `_L_Ai_en.cpp` 0x408830): over a
rectangle, the live ships (once, at their cell) and forts and barbarian
structures (each cell they cover) of players not marked friendly (0x4efe20,
game +0x2f45), grouped apart within 4 (`max + min / 2`) by one raster pass
(0x559070: a cell takes a label within 4 to its right or in the 4 rows
below, else a new one, and hands it on), ship groups first; each group its
count, extent, fire and boarding strength. With an id (0x408b70) each list
keeps just the group holding it, when it does. Mode 2 (0x408b30) can never
find anything: its callers (0x485fc0, 0x487690) always fall back to the
enemy's buildings 0x32-0x3e, 7 x 7 about each, weight 10, ten at most. A
hit (0x486b70) by a ship, fort or barbarian structure takes the first
group, 2 cells wider all round, for the help request; others ask none.

**Guarding forts' places** (`LAiFindPlaceGuardianFortress` 0x409b30, from
task 6 through 0x4852c0 over the whole map; the first result becomes the
flotilla's home, 1 x 1). Passable for its regions: free buildable cells
and the town's own cell, with their edge masks. A candidate is a passable
cell with a non-zero edge mask (a shore tile) off the town's region, or
in it further than 8 (`max + min / 2`). Over the cells within 6 of it:
`cover` sums the fire of the player's forts on each (its places whose
reach mask, 0x447840, takes the cell: their units' fire 0x446710, x 1.5
with the Flame, the sum / 100; a later fort writes over an earlier);
`water` counts sea ships reach (flooded from every ship and the berth of
every shipyard, dock and barbarian structure, not ports; 0x558fa0); `town`
sums how many of the town's four cells lie within 6. Sorted (0x409ad0,
VC6 qsort): least cover, then the nearest band `d / 5`, then most water,
then a non-zero town before zero, the least. The ship founds it there,
on the shore tile it chose (a ship's founding check, 0x4e4420, wants a
shore tile for a fort: see Building sites).

**Placing buildings** (the town's order 0x138c with no cell asked: message
0x4ed, 0x42dac0, `m_build.cpp` 0x42d9c0, the helper 0x40cca0 over the whole
map, taking the first of its results as cell and facing; none, the build
is dropped). Flags by class: forts 0x34-0x36 0x20, port, shipyard and dock
0x0e, the rest 0x05; only bits 1, 2 and 0x20 count. Each cell is coded:
not ground a building may stand on (0x503150: layer 0 of tile type 4,
island ground; none of tile sets 0x1a, 0x30 on layers 5-7) 1; free with
the tile's edge mask (0x5acf3f by layer-0 variant: 0x1c for 2-6, 0x07
7-11, 0x70 12-16, 0xc1 17-21, 0x10 30-31, 0x01 32-33, 0x04 34-36, 0x40
37-41, else 0) zero 4, else 3; the town's own (the settlement, or its
buildings and forts, +0x1c8) 0xb; and the front of every port, yard and
dock 8. Regions join cells across edges both leave open, diagonals too
(0x5597b0). Candidates are the cells of the town's region touching one of
its own cells, each weighed once in scan order. Flag 1 takes code 4: 100
for plain land (variant 1), +30 per shore cell about it, no facing. Flag 2
takes code 3 with a closed edge south, north, west or east whose
neighbour is water (keeping off a berth if it can): from the facing cell's
3 x 3, +10 a water cell, -100 a berth (and -100 if the facing cell is one),
-20 a water building, -15 amber. Flag 0x20 takes code 3 or 4 not plain:
50, or 130 for variants 22-29 and 42-43; -50 an own fort about it, +75 its
settlement, +50 a water building; +5 each water cell nearer than 7
(0x557440: the longer side plus half the shorter); facing as for flag 2,
else 0. The results go through the C runtime's qsort (0x58791f, highest
score first, ties as it leaves them). Not ported: a third object pointer
on the cell (+0x1c) and a layer nibble of 4 also blocking it.

**Behaviour masks.** Every task gives its fleet a behaviour (0x482e80
stores three masks at flotilla +0x14c, +0x148, +0x150; 0x482eb0 sends order
0x72 with 0 and the three; 0x4c4350 keeps them at fleet +0x15..+0x21 and
sends each ship message `#` sub 1, 0x4c42f0). The first is the players to
fight, the AIPlayer's +0xe18: 0x4847d0 (on the commander's first turn and
with every new enemy, 0x4844b0, 0x4845e0) sets every bit, then, if the
enemy's side (+0x1a1d) is 1 or 2 and the AIPlayer's difficulty is not
easy, clears those of the other computer players (side 3, not easy) after
the same enemy; when it changes, letter 8 has every flotilla send it
again. Ships pick targets only among players whose bit is set (+0x689,
0x4bfc60; the fleet tests +0x19 in 0x4d5430). The other two are always all
ones but for the reserve and spare tasks (0xb, 0xd), whose +0x150 is
0x1ffffff. Order 0x72 carries them as +4 (+0x14c), +8 (+0x148), +0xc
(+0x150), kept at fleet +0x19, +0x1d, +0x21 and passed on in each `#`; a
ship keeps the first as +0x689 (players) and the second as +0x68d, the
kinds it may target (0x50ebb0 with 0x50ed20: ship types bits 0-11,
buildings 12-17, lairs 18-20, creatures 21-23, nests 24-25, castles
26-27), as the fleet's own check shows (0x4ccd80 compares +0x689 with
+0x19 and +0x68d with +0x1d). The third is only ever passed along, so the
reserve's mask has no effect and every fleet may target every kind.

**Treasure hunting** (builders, task 5 state 2): the area is the square
about the base's town, from 4 cells and 4 more a time, that holds a
treasure (0x4854f0, counting with 0x48f020); the whole map if none. Order
0xbe (0x4da550; fleet state 0x79) holds the area, a mask of treasure kinds
(`+0xb0` bits), and where to bring them: the town's ports (class 0x39,
0x4d9a70). Each turn (0x4da3e0) a ship with room for a treasure (0x4e6700)
or none aboard (0x4be800 mode 6) sails to take one (0x4d9690, 0x4cf070);
a carrier sails home (0x4d9f50). With no treasure left in the area
(0x4d9920) the order reports done and the round goes on to the zone. Ours
takes any treasure, nearest first, one ship each, and brings the
settlement's alongside the base's town, where ours hands them over.

**Spells.** Order 117's studies 33 and 34 have the Temple of the Sea or
of War learn its next spell (1000 amber; 100 ticks for a computer,
0x439260). When a town is hit (0x488960, message 0x5d9c) the shooter's
5 x 5 cells are counted (0x486d50: others' ships and buildings, less one,
n) and four letters may go to the commander, each when n is at least 7-8
(easy), 6-7 (normal) or 5-6 (hard) (0x4886d0), each "urgent" with
n >= 12-14 / 10-12 / 8-10 or by chance 1 in 9 / 1 in 8 / 1 in 6
(0x4885a0): 6 a spell at the shooter's cell; 7 kind 9 (the Flame) for the
forts' flotilla; 6 kind 15 (weapon shield) over the base's area (flags
0x1000); 7 a spell on the base's guards. The commander's turn reads them,
newest first: at a place at most every 1500-3000 ticks (+0x188), on a
fleet every 3000-4500 (+0x18c), at once if urgent. Unnamed spells: at a
place n < 12 kind 11 or 14, n < 18 6 or 11, else 10 (1 in 3) or 6; on a
fleet 9 (1 in 3), else 16 (1 in 16), else 12 or 15. With the grand attack
it also casts kind 9 when 3000-4500 ticks have passed or 1 in 6. A cast
needs the town's check (0x487640 -> 0x43fee0) and goes out as order
0x3ee (0x4839e0-0x483c40, part 0x42a500: command 0x22 with spell 1-8,
12-15 for kinds 5-12, 13-16), which the temple turns into `Magic.cpp`'s
spells (0x42fc80: codes 1, 2, 3, 4, 6, 7, 8, 0xd, 0xe, 0xf to 2, 3, 1,
12, 6, 7, 9, 4, 10, 11; code 5 the temple brews the Flame itself; code
0xc none): kind 5 a whirlpool, 6 a waterspout, 7 a reef, 8 a big reef
(class 0x259), 9 the Flame, 10 lightning, 11 a fireball, 12 the phantom,
13 nothing, 14 a sea monster, 15 and 16 the shields. (`Magic.cpp`'s own
numbering follows the spells' order: 1 reef ... 4 sea monster, 6
lightning, 7 fireball, 9 phantom, 10 and 11 shields, 12 big reef, 13 the
Gorgon's stare, which nothing casts.)

**Worn crews** (0x486970): a warship (types 2-7) also goes to the reserve
when its crew condition (0x4bc590: the mean of its items' mean conditions,
`+0x419`; 0 with none) is under 35-65 (rolled after the hull's limit);
ships in a dock (state 2) are left alone.

**Repairs** (order 96, part 0x3ec, 0x429f60): of the town's buildings
(0x43fcb0) and forts (types 0x1a6b, 0x1494: worn, vtable +0x6c; affordable,
+0x9c) the first worn is mended: command 0x1e sent to the town (0x520cc0).
With none worn the part waits out its passes. Ours mends the first worn
building of the player the town may mend (`mend_by`), and with none is done.

**Strength** (0x50ea70): ships boarding strength (`+0x53c`) / 10 + armour
(`+0x534`: 40, 70; the ram's `+0x538` tests the armour level for 4 or 5,
so is 0) + fire (`+0x479`, 0x4bc360: each item's A x 15 x count x mean
condition / 100, summed, / 100); forts their units' fire (0x446710, x 1.5
under the Flame) / 10; nests 20 x 0x5c73dc; sea serpents 0x5c75fc / 10,
Medusa 8800, the sea monster 5000. An area's (0x4a2390) sums foes' and
creatures' less one; a flotilla's (0x482d80, 0x4a2290) its ships'.

## Videos (`video/*.avi`; verified)

RIFF AVI files with a video and a PCM sound stream (`00dc`/`01wb` chunks in
`movi`, some inside `rec ` lists): `logo1` (K.E. Media, 512x384 Cinepak,
44.1 kHz stereo), `logo2` (Re:Action, 640x480 Cinepak, 22 kHz mono),
`logo3` (MegaMedia, 320x240 Microsoft Video 1 `CRAM`, 16-bit RGB555),
`title` (the title sequence), `mis000` (the story's opening, 2751 frames)
and `outro0` (its ending), the last three 400x184 Cinepak at 20 frames a
second with 22 kHz stereo sound. Cinepak frames are strips of 4x4 blocks
coded by V1 (one 2x2-upscaled entry) or V4 (four entries) codebooks of
luma quads with shared chroma (r = y + 2v, g = y - u/2 - v, b = y + 2u);
Video 1 is 4x4 blocks from the bottom up, each a skip, a fill, or two or
eight colours picked by a bit mask. `tasks.dkx` in the same folder is not
a video.

## Missions (partially understood)

`WIN_VIEW` (record, 20 dwords, read by 0x4ed5a0 into game + 0x2ef5) holds
each player's starting view by player id (`nation * 4 + slot`): the cell
at the top left of the view, x in the low word and y in the high one; the
game sends it with message 0xcba when the mission starts.

`CONDITIONS` (record, 11 u32 flags, read at 0x4ed4c0 into the game object
at +0x32a5; missing: only flag 0) lists the mission's victory conditions,
named by string resources 3000-3010: kill all enemies, dragons, wizards of
fire, wizards of wind, sirens, gorgons, harpies, sea creatures, amazons,
minotaurs, cyclops. 0x448610 counts each player's remaining objects using
a 5x4 side table (+0x1a1d; 1 = the human's side): defeat when the human's
side has none, victory when every enabled condition's "remaining" flag
(+0x32d1..) is clear. Which classes count for each creature condition is
inferred (dragon/harpy nests 0x44/0x45, war/sea castles 0x43/0x42, sirens
0x1fd, gorgons 0x1fa, sea creatures 0x1fb, lairs 0x48/0x47/0x46): every
mission that enables a condition contains such objects. `WEATHE` (record):
field 5 looks like the weather's random seed; the demo stores an explicit
wind. `DISCRIPTION` (text) is the mission briefing: a 24-byte header (u32s 0,
0, 23, 40, 20, 0), marker bytes 0xff 0xff 0xfe, then NUL-separated lines
already wrapped for the original box; a line starting with `@` begins a
paragraph (`@ ` alone is a blank line).


A mission archive (`missNNN`, `singleNN`, `battleNN`) contains:

| Name | Kind | Notes |
|------|------|-------|
| `b_MAPA` | 17 | terrain grid |
| `OBJ_xxxxxxxx` | 12 | one record per object (ships, buildings, monsters); sizes 20-1244 |
| `FLOT` | 12 | fleets |
| `WEATHE` | 12 | weather/wind |
| `CONDITIONS` | 12 | victory conditions |
| `WIN_VIEW` | 12 | initial view |
| `ADD_TIK`, `OLD_TIME`, `TIK_TIME` | 12 | timing |
| `LIST_OBJ` | 20 | object list |
| `TRACKS_CD/FX/MID` | 20 | music and sound selection |
| `DISCRIPTION` | 23 | briefing text |
| `DESCRIPTOR`, `ORIGINAL`, `HEAD_CUST`, `HERO_ARR`, `MISS_UNIC_ID` | 0 | headers |

`DESCRIPTOR` (kind 0) holds the map size (+0x08, +0x0c), the player count
(+0x10) and, from +0x14, one 104-byte block per player: +0x00 u8 0 = human,
1 = computer; +0x04 u32 starting fish; +0x08 u32 nation; +0x0c u32 colour
slot; +0x10 u32 starting amber (the navy's set-up 0x4a78a0 puts +0x10 in the
player's amber, +0x20, and +0x04 in its fish, +0x2c; the original's miss000
starts at 8000 amber and 2300 fish on Easy: 4000 + 4000 and 300 + 2000). Every mission has exactly one human player;
(nation, slot) matches the owner fields of the object records.

Saved games (`demo.dkd`) add `DARK_MAP` (fog of war), `ALIANCES`,
`NAME_PLAYERS`, `AI_PLAYERS`, `DIFFICULTY`, `SPEED_GAME`, `RND_INIT` and
others.

## Healing aboard

`0x4b98b0` runs on every ship each tick. Each item slot (`+0x489`, 0x18
bytes each) holding a Cup of Asclepius (kind 15) or a Golden Fleece (kind
16) adds its rate (the treasure's field 8: 40 for the Cup, 10 for the
Fleece in the campaign) to a count. Each hero slot (`+0x4b5`) holding Argus
(hero 5) does the same with rate 20, which `0x4bd36a` sets when he comes
aboard. Every 1000 of a count:

- the Cup heals each wounded unit of the first three crew groups by 1 of
  its 100;
- the Fleece heals every group by 1 and mends 1% of the ship's full hull;
- Argus mends 1% of the full hull.

So the Fleece mends 1% every 100 ticks (4 s) and Argus 1% every 50 ticks
(2 s).

## Heroes and gifts in battle and under sail

Heroes are checked by `0x4ba790(ship, hero)`, hero numbers in house order
(0 Jason, 1 Theseus, 2 Orpheus, 3 Zetes, 4 Hercules, 5 Argus, 6 Atalanta,
7 Odysseus, 8 Perseus).

- **Theseus (1).** Every weapon aboard has range research level 2 (0x4bfc60,
  0x4c07c0, 0x4c0eb0), and each shot's blow is x 1.5 (0x4c0230, like the
  Flame's).
- **Atalanta (6).** Every weapon below item 9 reloads at rate-of-fire level
  2 (0x4c0230: `0x5c76e0[level + item x 3] + rand(30)`). The hit chance
  (0x4bf7f0) checks for her but ignores the result.
- **Orpheus (2).** Harpies (0x41efb0, 0x41f1c0) and land creatures
  (0x412290) leave his ship alone.
- **Odysseus (7).** A shot of letter 11, the harpy's, does his ship no harm
  (0x4bf170).
- **Argus (5).** He cuts rams (0x4c1ad0) and the siren's blow (0x4a74c0),
  and mends the hull (0x4b98b0). Nothing else.

The top speed (0x4bece0) is the type's speed at the rigging level, less half
the share of the hull lost. Armour fitted (`+0x27a`) takes 5% (medium) or
10% (strong) off it, and 15% or 35% off the acceleration, which the damage
also slows. Each active feather of Boreas (item kind 12, flag 0x10) adds its
amount in percent to the top speed and, in a ram (0x4c1c60), to the
rammer's blow.

**Brains' casts on their ships** (0x47acf0 through 0x479c20): an order
(message 0x3ee) to the casting member for each ship; a ship group drops it
(0x4a09d0), a building group queues it (0x42c430) and its war temple
carries one out a pass (every 50 ticks, 0x42b910, 0x42a500) when the town
check 0x43fee0 passes (a war temple standing and idle, the permit, the
spell learnt, the money). Each cast is paid in full (0x42f470); `pay`
credits one cast's price first (0x4a8eb0, 0x4a8f40). A shield or the
phantom lasts 3000 ticks more (0xda9a, 0xda97); the Flame is the player's:
brewed 250 ticks, then 1500 more of it (+0xa8, +0xb0). The port casts at
once, each ship paid, the Flame brewed once a call.
