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
| 20 | Mission lists (`LIST_OBJ`, `TRACKS_*`) | `LIST_OBJ` decoded |
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
= `frame_height << 16 | frame_width` (0x5552a0 / 0x555200 read the
high and low halves into the picture's `+0x65` / `+0x61`; the bombs'
and flags' first sprites are that size). The payload is `frames x layers` cells
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
`GAME_CTRLBAR` (208x600) on the right (on the left with "control bar
left": below); `GAME_FIELD_FS` (800x600) is the
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

### Control bar left or right ([0x5ca54a])

The configuration panel's radio pair (ids 0x3f1 left, 0x3f2 right) sets
`[0x5ca54a]` (1 left, 0 right; 0 by default, 0x501710). Every reader of it
(all 13 instructions referencing it in the exe):

- 0x512ab0(x), the bar's x: 592 + x, or x with the bar on the left
  (`[game+0xd91]`, the screen width 800, + x with the bar hidden,
  `[game+0x328d]`). Every bar panel places its controls through it.
- 0x511170(id, x): id 0x66 the same as 0x512ab0; id 0x67 the field's x,
  x + 208 with the bar on the left (x with the bar hidden); other ids 0.
  Callers with 0x67: the view's mapa, mode 1, at (0x67, 8) (16 with the
  bar hidden) and the radar map, mode 2, at (0x67, 8) (Mapa.cpp 0x51cdd0
  0x51ce87, 0x51b8c0, 0x51bd20; its message 0x24 re-places them the same
  way), and `CHT_ALSIGN` at (0x67, 8) (0x514b00, not ported). The
  bar's mapas (modes 3, 4) and the bar panels (0x4f0765-0x4f9cc5,
  0x527896-0x527be8) use 0x66 (all 70 callers checked).
- 0x504ef0(x), the field's: x + 208 with the bar on the left (unless
  hidden). Fieldsys (0x504620 message 5) draws `GAME_FIELD` at
  0x504ef0(0), `GAME_MEDIT` at (0x504ef0(0x6b), 568), `GAME_BKMONEY` at
  (0x504ef0(592 - 106), 527), the wind rose at 0x504ef0(3) (0x504fa0), the
  amounts (0x505530), the object info strip at (0x504ef0(0x6b), 527)
  (0x503d60), a control at 0x504ef0(0x70) made by 0x5036d0 (type 0x163,
  with 571, 365, 17 beside it) and a position 0x504ef0(0x70) kept by
  0x506c70 (the message line's; not read further), the network name frame
  `CHT_FRAMENAME` at 0x504ef0(207), the chat lines at 0x504ef0(8 or 16)
  (chat.cpp 0x4ff390-0x4ffd0c). 0x504620 message 5 also calls 0x504ef0(3,
  or 0x1e6 with the bar on the left) and drops the result.
- 0x4faec0 (the panel's controls) starts the radio group on 0x3f2 - flag;
  0x4fb540 message 0x506 sets the flag from the chosen id.

The tab group (`_stab.cpp` 0x550c10, control type 5) notifies (0x506) only
when an item other than the chosen one is pressed (message 0x61), so the
side changes only on a change. Then, at once, 0x4fb540 sends message 0x24
(move by dx) to the game screen (0x50f910): with `+0x18` 0 it moves the bar
window by -592 (now left) or +592 and the field window by +208 or -208, and
broadcasts message 5 (redraw). The screen is not rebuilt; every window
moves in place, the mapas re-placing themselves on 0x24. Space (0x50f910
message 0x4e1) uses the same message with `+0x18` 1 to hide or show the
bar: the bar by 208 (right) or 800 (left), the field by 0 (right) or 208
(left), so with the bar hidden the field is at 0 either side.

### Bar hidden (Space)

The game screen's constructor (0x50eee0, 0x50f0c4..0x50f0e1) binds Space
(scan code 0x39, exclusive, no repeat) to its message 0x4e1; the handler
(0x50f910) has no guard of its own. The flag is `[game+0x328d]` (1 =
hidden), written only by the game's constructor (0x4ebd10), every game
build (0x4ebf50, 0x4ebf9f), the ways out of the game (0x4ec1a0 commands
0x100, 0x10b, 0x10c, 0x4ec2c4) and the toggle (0x5103e8): every game
starts with the bar shown and the flag is not saved. Message 0x4e1, in
order:

1. 0x4ee970: the cursor clip back to the whole screen and broadcast 0x14a
   (the mapas end any drag, 0x51cdd0) (and message 0xc45e, not read).
2. If the view's mapa is in mode 2 (the radar map), 0xcbc to the field
   (Tab) puts the view back.
3. The flag flipped.
4. The bar's tab group set to 1000 (message 0x22): the object panel,
   whether hiding or showing.
5. The message line control (made by 0x5036d0 at (0x504ef0(0x70), 571),
   365 x 17) set 365 wide, 573 hidden; the chat edit 208 wider or
   narrower.
6. Broadcast 0xcc3 to the mapas, message 0x24 with `+0x18` 1 (the windows
   moved: the bar by 208 with it on the right, 800 on the left, the field
   by 0 or 208, so the bar is at 800 and the field at 0 either side while
   hidden), and a redraw.

While hidden: the field draws `GAME_FIELD_FS` (800x600, its see-through
part exactly (16, 8, 768, 512)) at (0, 0), `GAME_MEDIT_FS` (584x26) at
(107, 568), the money panel at (694, 527), the wind rose at (3, 527), the
object info strip on `CHT_BKFINFO` (585x20) at (107, 527) (0x504620
messages 5 and 0x24, 0x504fa0, 0x505530, 0x503d60); the view's mapa is 12
x 8 cells at (16, 8) (0x511170(0x67, 16); 0xcc3 sets its width `+0x3c` 12
hidden, 9 shown, 0x51e041..0x51e051, and clamps its scroll to the map less
the width, keeping the top-left cell); the chat lines at 0x504ef0(16), y 8
+ 20 per line, on bitmaps 12 x 64 wide (9 x 64 and x 8 shown; chat.cpp
0x4ff390, 0x4ff4c0). The bar and its minimap are at x >= 800: nothing of
them shows (the minimap's 0xcc3 step is skipped while hidden) and no mouse
reaches them. Tab is refused (0x504620 0xcbc acts only when the flag is
0). With the bar hidden the side setting makes no difference.

The bar's Ctrl+S / Ctrl+O (0x511c00 0x500 / 0x501): when the current save
name `[game+0x18dd]` is empty (equal to the empty `[0x5c9e6c]` that
commands 0x101 / 0x103 copy into it) they send 0x4e1 first if the bar is
hidden, then open the file panel (tab 0x3e9) in save or load mode; with a
name, Ctrl+O loads that save at once (command 0x102) and Ctrl+S calls
0x511580 (not read). The port's Ctrl+S / Ctrl+O are its own quick save to
`quick.json` and never open the file panel: the original's two branches are
not ported.

Not decoded: message 0xc45e (0x4ee970); whether Sinput ignores the bar's
key bindings while it is off screen (no unbinding is done by 0x4e1 or
0x24, Sinput's dispatch not read for a rect test): the port keeps the
command buttons' keys off while the bar is hidden, as before.

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
  100 (0 while `+0x251` is 2, being built or sold; 0x4e51c0). Icons: treasures aboard (`+0x482`), the hero
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
  wind returns. In full (0x407a80): its value v gives strength 7 if v is
  not 0, else 0, and direction v - 1 (-1 for 0).
- Ship speed (0x4a68b0, 0x4a3f40): top speed and step x 0x597b2c[(type
  0 or 1 ? 11 : 0) + strength][angle] / 100 (see Times, "The move tick"), where angle = |wind - heading| folded to 0-4
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
  interpretation). Rows 9-18 belong to the special ships and upgrades:
  row 9 (Greek fire's item) is all zero, rows 10 and 11 (quick lime's and
  the barrel of powder's items) are both A 200/240/280, B 80/96/112; the
  Greek fire and quick lime shots use rows 12 (A 650/720/790, B
  300/320/340) and 13 (no A, B 200/240/280); rows 14 and 15 A 80/40, B
  40/80; rows 16-18 the volcano's shots (A 1220/1420/1550, B
  600/1120/1180 at level 0). The port's `combat::DAMAGE` is the table as
  dumped (test `damage_table_is_the_exes`). A ram has no blast of
  powder (the manual's 400 hull / 150 crew is not the original's): the
  blow 0x4c1c60 and its share-out 0x4c1ad0 read no item 11, and the ram
  (0x4c2230) spends none; the fire rammer's (type 10) only extra is the
  burning S it throws at its prey as its bow strikes (0x4c2566, 0x4c21c0,
  shot kind 0x12; see the volcanoes below), whatever it carries.
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
  dock's fitter 0x4bb670 scores item i by 0x4bb490 with level byte
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
  makes armour M/S (0x36, 0x37) and rams M/S (0x38, 0x39; slots 0-3) up to
  the researched level (permission bits `(level + 0x1c) ^ 7`, player
  `+0x5f` / `+0x5e`), for its town's store: the buttons are on while the
  smithy has a town (`+0x1c8`) and is idle or busy with these pieces
  (vt+0x9c), and, idle, the amber for one is in hand; the menu shows at
  all only while it stands, is idle or on jobs 1-3, and has no reload
  contact (`+0x104`). No ship alongside is asked. Each order (message 0x138c, 0x434a00; its count byte +0xe, 1
  when 0, which the command panel leaves it for refits, 0x4f3a61) pays 30/60 (0x42d960) or 25/50 (0x42d940) amber a unit and
  queues one byte a unit (+0x1ec; slot 1, 2, 4, 5), done in turn, 150 or
  200 ticks each (0x43542a); when one is done the smithy sends its town
  (`+0x1c8`) message 0x248d (armour, codes 1, 2) or 0x248e (ram, 4, 5)
  `{code, 100}`, which TownObj 0x441ba0 appends to its armour list
  `+0x248` or ram list `+0x24c` with no state test (0x441c0c): the store
  the load panel and the default arming take from. A smithy without a
  town sends it to nobody. The
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
- Ramming odds (0x4ba950, read by the attack's pickers 0x4baae0 /
  0x4bad90; port `combat::ram_odds`): for each ship, ram quality
  (0x598054, nine u32: 100 110 117 / 125 135 145 / 160 175 200 by its
  ram kind `+0x27b` x 3 + the player's level in it, `+0x67 + kind`) x
  ramming power of the ship type (0x598024: boat
  10, merchant 12, royal 100, bomber 60, boarder 30, rammer 140, ship killer
  20, men killer 40, fire ship 10, quick lime 10, fire rammer 190, ship of
  the dead 10) x an angle factor (0x598078 for the rammer: 90 55 20;
  0x598084 for the rammed: 4 12 20, by the angle class of 0x4c19b0: 1 the
  same heading, 2 opposite, 0 any other) x the type's full hull / 100
  (0x5c76b0, filled by 0x4ac210 from the ship constructor as 0x5bb4b0[t] x
  650: 18200 58500 65000 78000 42250 45500 23400 23400 42250 48750 71500
  78000), over 2500, in integers: `Q x P / P' x (F x H / 100) / 2500`.
  Rammers (type 5) get x 15 / 10. The result is attacker x 100 /
  defender, capped at 200 (200 against nothing).
- Effects are BOMB sequences (`BOMB%C%1d%1d`): e.g. BOMBA20 hit, BOMBA50
  splash, BOMBG10 explosion (a shot's landing); FIRE0-2 and SMOKE0-2 for burning.
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
| 0x1f7 | `CyclopsJamClassTy` | the land creatures on one cell (factory 0x416230, ctor 0x415c10, handler 0x415ee0: up to 100, joined by message 0x1879f, left by 0x187a0 and gone with the last; lightning, tornadoes and 0x187a2 passed on to each; it fills the cell's object slot, 0x499800, and commands nothing) | | |
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
| 0x2c1 | `SkarbClassTy` | sunken chest dropped by a sinking ship (`SKARB<kind>0`, `SKARB<kind>1`) | | |

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
remapped through the shadow table 0x5c1358 (256 bytes of static data,
palette index to index; `render::SHADOW_TABLE`, tested against the exe).
A layer gets it from 0x50e050(layer, 0x5c1358) (0x55dd10: the sprite's
`+0x63c`) and 0x50e130(layer, 1, colour, 0) (0x55dd40: the frame's table
`+0x5c` 0, value `+0x60` the colour, count `+0x64` 1); the drawing
(0x572585-0x57259e) uses the sprite's `+0x63c` for a frame with count 1
and no table, and the blitter 0x57b7f0 draws the colour as
`table[beneath]`. The colours set: 0xfe (objects), 0x26 (smoke, 0x4e292d,
0x4e4ff3, 0x42097e, 0x425f52), 0xd (0x405a6a, 0x4062a5), 0x27 (0x406d5e,
0x407449, 0x41c881), 0x2a (0x42230f), and some from a register. The port
looks the palette index beneath up as for the flames' blend (the first
index of the frame's colour, else the nearest).

A ship with the phantom (`+0x253`) is ghosted for its owner and hidden
from the other players. 0x4df420(on) runs at the casting (0x4de378:
`+0x253` 1, `+0x81` now, `+0x85` the length), at its end (0x4df210:
now - `+0x81` >= `+0x85`, then `+0x253` 0), at a capture (0x4b6f20 at
0x4b7016-0x4b7034: off, `+0x253` 0) and at the ship's set-up (0x4e2b44,
with `+0x253`); unless the ship's `+0x251` is 4 it passes layers 0-3
and 5-8 (not 4, the selection frame `RAM_0<w>X0<h>`) with their
picture's width `+0x61` to 0x4df380, and 0x4e1bb0 does the same for a
layer it loads anew. 0x4df380(on, layer, width): for the player at this
seat (0x5c7508 `+0x18` the ship's `+0x24f`) 0x50e460(layer, on ?
0x5cdd2c : 0, width) (0x55e010: the sprite's mask `+0xb8` and mask
width `+0xbc`); for any other 0x50e350(layer, on) (`+0x8d`, the
sprite's flag 0x40000: hidden) and, for layer 0, the radar icon off
(0x50de40, message 0xd21) or back (0x50da60, 0xd20). Loading layer 0
in the sinking mode (0x4e1bb0(0), `+0x74` 5) unghosts or shows it and
clears `+0x253`. The sinking (0x4dec40 at state 4): `+0x74` 5,
0x4e1bb0(0) loads `SHIP_<letter><+0x250>001` (0x4e0120 mode 1) as
layer 0 with mask `1 << +0x5af` (`0x101 << +0x5af` while burning,
`+0x8ca`; every mode 1 composite has 8 layers, one per heading), frame
`+0x45` 0, delay `+0x7d` 2; 0x4c12b0 each tick steps a frame when more
than 2 ticks passed since `+0x81` (0x4df6e0 first), at the next-to-last
frame drops the treasure (0x4e4e30, message 0x16e2: a `SkarbClassTy`
0x2c1) and the crew (0x4e4f20), past the last sends message 10 (gone).
Mode 1 composites exist for types 0-9 of nation 1, 0-7 and 10-11 of
3, 1-3 and 5-6 of 4; with none 0x50d080 has no picture and
nothing shows. The port (`objects::ship_sink_frames`, a frame per 3
ticks) counts frame 0 from the sinking, not from the hull's last step
`+0x81`. 0x4c12b0 steps when more than `+0x7d` (2) ticks have passed,
adds 1 to the frame and returns 1 when count - frame is then 2, 2 when
the frame reaches the count (taken back by one; message 10 follows):
with fewer than 3 frames (or none) nothing is ever dropped. The port
keeps the drop (`chest::SinkDrop`: the cell, half the sale value, the
hull's type and the treasures taken off the ship) for 3 x (count - 2)
ticks after the sinking (`Scene::sink_drop`, `run_sink_drops`).

The sunken chest (`SkarbClassTy` 0x2c1, `andrey\ska`). 0x4c12b0 returns
1 when its step leaves the frame at count - 2; the sinking ship then
calls 0x4e4e30 and 0x4e4f20. 0x4e4e30 sends the world message 0x16e2
(0x51ee0f: create a 0x2c1) with a 0x6c-byte record: `+0x10`, `+0x14` the
ship's cell (`+0x5a3`, `+0x5a7`), `+0x1c` the value 0x4e4ce0 / 2 (the
hull's sale value in amber, `Scene::sale_value`, halved toward zero),
`+0x20` the kind by the hull's type `+0x25a` (a switch on 1, 2, 4, 8,
0x10, 0x20, 0x40, 0x80, ...: jump table 0x4e4ee8 / index bytes 0x4e4ef8;
for types 0-11: 4 and 8 give 2, all others 0), `+0x18` 0 (new).
0x4e4f20 lays the treasures aboard (`+0x482` of them, records of 0x18
bytes at `+0x485`): while any is left and fewer than 0x597afc[type]
(1, 1, 2, 1, 1, 1, 1, 1, 0, 0, 0, 0) were laid, 0x4b6e80(record i, 0, 0)
announces it (brains' message 0x5d94 with 0) and 0x4bd500(index 0, with
1) takes the first out and lays it on the ship's cell (0x520c70); the
records shift down, so the announced record is the one at i after the
shifts; the rest go down with the ship. The port lays them so
(`run_sink_drops`, no message; `drop_held` stays for buildings and
creatures). Not ported: the announcement 0x4b6e80 (brains' message
0x5d94 with 0).

The chest's handler 0x405230: message 2 (creation) takes the cell
(`+0xb8`, `+0xbc`), value `+0xc0`, kind `+0x74`, its generator `+0x18`
(new: the world's `+0x2ee5` x its id `+0x4c`); new, 0x48ec60(cell, 1)
0 removes it at once (message 10, state 4); else it takes the cell's
object slot (0x5c74fc grid, `+8`), enters state 1 and plays sound
event 0x58 (88, `wreck1`) mode 6 at the cell (0x4053f8). States
(0x404e10, `+0xb4`; each a 0x50cbf0 layer at depth 0x4e82 and picture
0x404af0 `SKARB<kind><0|1>`, placed at `cell x width - (picture width
- 0x40) / 2`, likewise y, 0x404c90): 1 rising, `SKARB<k>0` backward from
frame count - 2, one pass (past 0 the frame goes to count - 2 again and
the pass is made); 2 lying, `SKARB<k>1` forward, `r % 16 + 5`
passes (`r` the generator's LCG 0x41c64e6d / 0x3039, `>> 16 & 0xf`), 0
if hurried; 3 sinking, `SKARB<k>0` forward, one pass; then the cell's
slot cleared, message 10 and state 4. Frames step (0x404b50) when the
world tick (`+0xde`) reaches the last step `+0x7c` plus the delay
`+0x80` 2 (0 when hurried, `+0x90`). Messages 0xda93-0xda96 (blows,
spells) hurry it (`+0x90`, `+0x8c` 1); 0x158c0 (not in state 4) answers
its value, hurries it and starts state 3. A ship's step (0x4a3f40, at
0x4a47c0) onto a cell whose object slot holds a 0x2c1: if the ship is
the local seat's, sound event by its `+0x54` (0 0xac, 1 0xad, 3 0xae,
2 0xaf; mode 5) at its cell; then, the chest not in state 4, 0x4a8de0
gives the ship's player the chest's `+0xc0` amber and posts 0x158c0.
Saved with 0x8cf (record 0x6c bytes: kind 0x2c1, `+0x10` cell, `+0x18`
the generator, ...; reloaded through 0x405130). Each state's entry
(0x404e10) zeroes the passes and, for 1-3, loads the picture (0x404c90:
first frame 0, last count - 1 at least 0, the frame count - 2 rising or
the first, delay `+0x80` 2, last step `+0x7c` 0) and steps at once
(0x404b50: the frame `+0x45` drawn, then moved; each wrap a pass);
lying hurried has 0 passes and sinks at once (the loop in 0x404e10);
any state past 3 clears `+0x90`. The tick 0x404f80: a state's passes
made, the next; lying hurried, the passes are set to target - 1 before
the step. A hurry (0xda93-0xda96, with a picture) sets `+0x90` and the
target to 1. 0x48ec60(self, x, y, 1): landscape low nibble up to 7 and
nibble 1 up to 0x30, the occupant slot `+0xc` empty (0x499900; the
sinking ship left it at 0x4dec40 through 0x4a3970 / 0x4a3800, 0x4998a0),
`+0x1c` empty, 0x403010, and the `+0x10` slot empty or itself. Every
`SKARB<k><0|1>` (k 0, 2) is 33 frames of 64 x 64, so the picture's
corner is the cell's; `SND_172` and `SND_175` are empty, `SND_173` and
`SND_174` list `piw1a`, `piw3a`, `piw4a`. The port (`scene/chest.rs`):
an `ObjectClass::Other(0x2c1)` object (record fields 4, 5 the cell, 6
the kind, 7 lying) with its state in `Scene::chests` (saved), stepped in
the game tick after the castles; the leviathans' 0x48ed40 also finds the
`+0x10` slot taken by a chest. Not ported: the hurry's senders (no
blow or spell of the port reaches a chest yet); 0x158c0 is posted
(0x5405b0), taken at once here; the step into a cell on the boarding
path (0x4a3f40 to 0x4a4d49) is not hooked; the monsters' 0x402ee0 test
of a picture in the `+0x10` slot (vt +0x30) for a chest.

The ghost table 0x5cdd2c (0x800 bytes of bss) is
filled at start (0x519280 at 0x5193e5-0x51942b): 128 rows of 16
bytes, 0xaa on an even row, 0x55 on an odd one. The drawing
(0x572391-0x5723a4, 0x5725cc-0x5725df) passes the mask with a row of
`(width + 7) / 8` bytes and the pixel's place in the picture, and the
blitter (0x57b7f0 at 0x57b835-0x57b84b) tests bit `0x80 >> (x & 7)`
of byte `row x y + x / 8`; on a frame of kind 0 (header byte 0xd & 3)
a pixel of the shadow colour goes through the shadow table whatever
the mask (0x57c5df-0x57c616), the rest only on a set bit. Frames of
kind 1 (the flames, `FIRE0-2`) are drawn without the mask; kind 2's
copied runs advance the source only on a set bit (0x57be13-0x57be2a).
Every ship sprite is of kind 0 and its picture's size (56 to 80
square, so the mask stays within the table), so a ship's owner sees
its hull through rows of alternating pixels (a 64-wide picture's two
rows to a table row) and its flames whole (`render::GHOST_TABLE`,
`ghost_shows`).

A building with the phantom (LandObj `+0x134`, set by 0xda97 at
0x426f7b with its start `+0x13c` and length `+0x140`) does the same in
its tick (0x425990): the phantom ends (0x426071-0x426096) when `+0x13c + +0x140` is below
the game's tick `+0xde` (`+0x134` 0); then, with `+0x134` set and its
applied flag `+0x138` clear (0x4260a0-0x426165), it sets `+0x138` and,
for the player at this seat (0x5c7508 `+0x3212 / +0x3213` the
building's nation and slot `+0x54 / +0x55`), puts the ghost table on
layers 0 (the picture, `+0x7c`), 1 (the banner, `+0xd8`), 0xd (the
weapon shield's `PROTWSPR`, `+0x198`) and 0xc (the magic shield's
`PROTMSPR`, `+0x178`) that exist, 0x50e460(layer, 0x5cdd2c, 0x80): the
mask 0x80 wide whatever the picture; for any other player it sends
itself 0xd21 (radar icon off by 0x50de40 unless `+0xa4` is 0xff or
0x10) and hides those layers (0x50e350(layer, 1)). With `+0x134` clear
and `+0x138` set (0x4261dc-0x4262e3) it clears `+0x138` and undoes the
same (0x50e460(layer, 0, 0); or 0xd20, 0x50da60, and 0x50e350(layer,
0)). A shield sprite made while `+0x134` is set gets the same at once
(0x426484-0x42648f, 0x42667e-0x426689). The fire and smoke layers
(3-10) are left as they are. Every building picture is one kind-0
sprite of at most 128 x 128 at the picture's top left, and the banners
(`FLAGSPR_`) kind 0 as well (every sprite but `FIRE*` is): the owner
sees picture and banner through a checkerboard (16-byte table rows,
each from the layer's own top left), another player only the fire and
smoke. The port draws so (`Scene::is_phantom`, `phantom_hidden`, the
shields' sprites too).

A building's timed statuses are counted in game ticks, each a flag, a
start and a length, the length added to by every casting or blow and
the start the game tick `+0xde` when it was off: the phantom
(`+0x134`, `+0x13c`, `+0x140`; 0xda97), the weapon shield (`+0x17c`,
`+0x184`, `+0x188`; 0xda9a type 1, its sound 0x50e570(5, cell, 0x56)),
the magic shield (`+0x15c`, `+0x164`, `+0x168`; 0xda9a type 2, sound
0x57; it resets the weapon shield's frame time `+0x190`, not its own
`+0x170`) and the lime (`+0x144`, `+0x14c`, `+0x150`; 0x42405d and the
stare 0x158bf). The tick (0x425990 message 0) ends them in this order:
the phantom when start + length is below the tick (0x426071); the lime
when it is at most the tick (0x4262e3: `+0xf8` 1, `+0x1c4` 0, and with
`+0x10c` the work's start `+0x124` set to the lime's start); the weapon
shield when at most the tick and its sprite `+0x198` exists (0x4265a4),
the magic shield likewise with its `+0x174` (0x4267ed). The sprites
are made earlier in the same tick (0x4263ef, 0x4265ed) when the shield
is on and `+0x180` / `+0x160` are set, which the LandObj constructor 0x422cc0 sets to 1 (the
side-2 structure 0x420fd0 clears them, and `+0xd4`). The port keeps the
spans in `Combatant::{phantom,weapon,magic,lime}_span` and ends them in
`Scene::tick_building_statuses`, run once per game tick; saves from
before start what was left at the first tick after loading. The
shields' casting sound, 0x50e570(5, the cell `+0x74` / `+0x78`, 0x56
weapon at 0x42719f / 0x57 magic at 0x427139), plays at every casting,
on or off, before the flag's test; both events' `SND_` lists hold
`prote3` alone (dumped). The phantom's 0xda97 (0x426f7b) plays nothing.
The port plays `prote3` at the building's place (`Scene::protect`). The
phantom is silent: Magic.cpp 0x42f580 case 9 only checks (0x42ed40),
pays (0x42f470) and posts 0xda97, none of which plays a sound.

The shield sprites: layer 0xd (`PROTWSPR_64` for a
building of size `+0x56` 1, else `_128`; 0x426484) and 0xc
(`PROTMSPR_*`, 0x42667f), depth the picture's `+0x19` less 5 / 4, made
with frame `+0x18c` / `+0x16c` 0; while on, when the tick is more than
1 past its time `+0x190` / `+0x170` the frame goes on by one and past
the top it goes back to the bottom, from 0x595f90 (size 4: 14..43) or
0x595f98 (else 14..44) for the weapon shield, 0x595fa0 (14..43) or
0x595fa8 (13..40) for the magic one; once off it plays on to the
sequence's count and the layer is removed (0x4264b3, 0x4266fc). Each
step 0x50d660 places it at the picture's left + w / 2 - its w / 2 and
top + h / 4 - its h / 2 (integer halves and quarter). In full
(0x4263ef-0x4265a4, the magic shield's 0x4265ed-0x4267ed alike): off,
with the sprite made (`+0x194` / `+0x174`), it is removed when its frame
is at least the sequence's count (the layer's `+0x49`), else it steps;
on and not made with `+0x180` / `+0x160` set, it is made (0x50cbf0,
0x50cf60(layer, 7, 0, name)) and placed at its frame; otherwise, made
and the tick more than its time + 1, the time becomes the tick, the
frame goes on by one and, while on, past the loop's top back to its
bottom, then it is placed. So a frame lasts 2 ticks, and the frame equal
to the count is set (shown as frame 0: 0x55ec70 takes it modulo the
count) for the tick before the removal. The casting (0xda9a) with the
shield off sets its frame to 0 and the weapon shield's time `+0x190` to
the tick, for either shield (the magic shield's `+0x170` is left as it
was). The sequences: `PROTWSPR_64` 59 frames of 80 x 80, `_128` 58 of
128 x 128, `PROTMSPR_64` 56 of 64 x 64, `_128` 58 of 128 x 128, every
frame one size. The hero's house (0x420fd0) clears `+0x180` / `+0x160`
(and has both shields on for good); forts and barbarian structures tick
by 0x419ba0 / 0x40eba0, which make no shield sprites (the strings'
only other users are the ships', 0x4e0f30 and 0x4e1160); castles
(0x410ca0 through 0x427f50), lairs (0x414f00) and the rest run 0x425990.
The shields are not remapped by the lime: 0x425990 remaps layers 0 and
1 only, once when limed (`+0x148` 0 -> 1, 0x4263a8-0x4263cf: the
picture if `+0x7c` is made, the banner if `+0xd8` is, both through
0x5c1758 when `+0x1c4` is set, which the lime blow 0x42405d sets to the
tick + its length, else 0x5c1558) and back to none when the lime is
out. The port: `scene/shields.rs`, stepped in
`Scene::tick_building_statuses` before each shield's end and drawn over
the building's fire and banner, the magic shield's then the weapon
shield's (depths less 4 and 5), ghosted or hidden with the phantom;
saves do not keep the sprites' frames and times. The banner is drawn
limed as the picture (through 0x5c1758; the stare's `+0x1c4`, so
whether a stared building takes 0x5c1558, not read).

A ship's shields (ship.c): states `+0x254` (weapon) / `+0x255` (magic),
0 none, 1 playing out, 2 cast, 3 looping. The casting 0xda9a (0x4de53c
weapon, 0x4de4c8 magic), with the state below 2: no sprite made, it is
made (0x4e0e70 / 0x4e10a0: 0x50cbf0 layer 7 / 8 depth 10, the sequence
0x4e0f30 `PROTWSPR_64` / 0x4e1160 `PROTMSPR_64` whatever the ship,
frame `+0x45`, delay `+0x7d` and time `+0x81` 0, placed at the hull's
(`+0x8be`) left - (its w - the hull's w) / 2 and top - (its h - the
hull's h) / 2 - the hull's h / 4, C division); a sprite still playing
out goes on from its frame. Then state 2, time `+0x91` / `+0x99` the
tick and length `+0x95` / `+0x9d` 0, event 0x50e570(5, cell, 0x56 /
0x57); every casting adds its length. The tick (0x4dc630) ends them by
0x4dec40 (with the mode `+0x251` at most 2, 0x4def3d-0x4defaf): the
weapon shield then the magic one, state 2 or 3 with tick - time >=
length (unsigned) -> state 1, time and length 0; the magic one's end
writes 0 to the weapon's length `+0x95` instead of its own `+0x9d`, so a
weapon shield still on ends at the next tick. Then it steps the magic
sprite (0x4e11a0, 0x4dca36) then the weapon one (0x4e0f70, 0x4dca47),
each while its state is not 0: with the sprite made and tick - time >
delay 0, time = tick, 0x50d660 shows frame `+0x45` at the place above
recomputed (depth the hull's `+0x19`, 0x50d290(0), less 0x4a); then,
with `[clock+0xe2]` odd (weapon) / not a multiple of 3 (magic) -- 1 in
single player -- the frame + 1: state 2 or 3, at 45 back to 14 (weapon)
/ at 42 back to 13 (magic), state 3; state 1, at the count `+0x49` the
layer is removed (0x4e0f50 / 0x4e1180) and the state 0, so the count's
last frame is shown only by the step that removes it. The layers go
through 0x4df380 with the phantom (0x4e1bb0 cases 7, 8), as the hull;
no stone or lime remap. The save (0x4e12e0) keeps both sprites' frame,
place, delay and time. The port: `Scene::tick_ship_shields` after the
buildings' statuses, the same `ShieldSprite`s, drawn after the ship;
the save keeps no sprite, so a loaded ship's shield remakes its sprite
at frame 0. The casting's event is pushed (x `+0x5a3`, y `+0x5a7`,
then mode 5 last; event 0x56 at 0x4de584, 0x57 at 0x4de510) only in
the state-below-2 branch, so a casting on a shield already on is
silent; both lists are `prote3` alone, which the port plays at the
ship. A ship's phantom (0xda97 at 0x4de378): off (`+0x253` 0), the
flag 1, start `+0x81` the game tick, length `+0x85` 0 and 0x4df420(1);
then every casting adds its length; no sound. 0x4dec40, while
`+0x251` is below 3 (0 afloat, 1, 2 being built or sold; 4, sunk, is
the only other value written), first ends the phantom (0x4df210 with
`+0x253` set: tick - start >= length, unsigned -> start 0,
0x4df420(0), length 0, flag 0), then `+0x257` (0x4df260) and `+0x258`
(0x4df2d0), then the shields above, then `+0x25c` (0x4df320). The
port keeps the phantom in `Combatant::phantom_span` and ends it first
in `Scene::tick_ship_shields`; a sunk ship is taken out at its blow,
so the `+0x251` test passes for every ship it sees; a treasure's
shield on a ship goes through the same casting (its own route not
read).

The pointer still picks a hidden phantom ship or building: 0x49a580
asks each occupant's vt+8, the ship's 0x4e06c0 (its hull sprite's rect
and mask), a building's 0x423460 (castles 0x410420: the picture's rect,
then for `+0xa4` 1 (castles also 4) its mask `+0xcc`), and neither they
nor 0x49a580 and the cursor 0x5175b0 test the sprite's hidden flag, so
it is hovered, shown and ordered against as when seen; the port's
`hit_test` likewise ignores the phantom. Left out: a banner made anew
by 0x4247b0 during the phantom is not ghosted or hidden until the
phantom is cast again (no 0x50e460 / 0x50e350 there); the port ghosts
or hides it with the building. The tick makes it anew (0x425bf4) when
`+0xd8` is 0 and `+0xd4` set, and removes it (0x50cdb0(1)) when `+0xd8`
is set and the state `+0xa4` is 2 or 3 or `+0xd4` is 0; message 2 makes
it at 0x425e8d when `+0xdc` and `+0xd4`; message 3 removes it. So a
building in state 2 or 3 with `+0xd4` set loses and regains it on
alternate ticks, the regained one plain under the phantom. Not decoded:
when `+0xd4` is cleared and set again and how the port's states map to
2 and 3, needed before the port can model which banner is current.

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

The help line on hover (0x4f2a90 sends message 0x54b, 0x505e70 draws it):
the string 0x4f1b40 gives for the slot (0xa4 `Cancel` on a crossed slot;
by command, e.g. 1-2 0x95 `Stop`, 4 0x96 `Move`, 0x61 0xd5 `Magic of
whirlpool`, 0x6b 0xf9 `Head of Gorgon`, 0x71 0xec (no such string); 0x28
for none). Only for strings 0x95-0x15e: `&1` before it (0x5be6fc), then
for the slot's stored price (`+0x52`: amber low word, fish high)
`"%s. %s-%d"` with string 0x59 `Cost` and `"%s, %s-%d"` with 0x5a
`Fish`, then the key (`"%s - &0%s"`, 0x5be6c8); any other string is shown
alone. The text drawer (0x547860) reads `&` and a digit (0x5469b0) as the
colour variant: the font's atlas n (n < 15, else its default `+0x4c`;
forced to `+0x86` when that is not negative; only with `+0x82` set), an n
past the font's atlases drawing atlas 0 (0x547710). Not ported: the
colours, the message field's font (`+0x28` of the field, vtable
0x59b62c) not traced. `Command::help_id` / `help_line`.

The prices on the buttons (`+0x52`, u32 per slot at menu byte 0x32 + 4 x
slot) are written by the menus only and read only by the hover
(0x4f28af): the orders pay what their handlers reckon. Shipyard
(0x4164e0): each ship type's 0x43e5c0 price (hull and, with default
arming `[game+0x3295]`, the arming; fish x 100), the Argo type 2's,
the dock (slot 11) `0x42d8b0(0x3a) x (100 - +0xa8) / 100 + 0x42d8b0(0x3b)`
(the shipyard's mending), the same price tested to enable it. Settlement
(0x43c740): the town (slot 4) `0x42d8b0(0x32) x (100 - +0xa8) / 100 +
0x42d8b0(0x33)`, likewise; the buildings 0x42d8b0, the forts 0x42d8b0 and
their arming 0x43e990 (fish x 100). Fort/upgrades (0x430f90): 0x5971ac by
level ({1200, 1300, 0, 400}). Smithy (0x433e60): 0x42d960 / 0x42d940.
Workshop (0x4361c0): 0x597164. Temples (0x437de0): spells 0x596844 /
0x59687c x 100, magic (0x54) 1000. A ship's building page (0x4e6980):
0x42d8b0 of 0x32, 0x34-0x36, unarmed; its main page only the repair (slot
9, command 6), none on the spells. The repair's (0x4e6980 at 0x4e6db5,
0x4e6e90 at 0x4e700d) is stored only when the button is enabled (`+0x261`
bit 1 and the player record's `[0x5c7508] + p x 0xba0 + 0x118` not 0;
that field's writer not decoded, the port enables on a worn hull): the
u32 0x4b1ed0 returns, `(0x5c76b0[t] - +0x271) x 0x59b388[t] / 0x5c76b0[t]`
and, for types other than 0 and 1, each unit below 100, items 9 down to
0 (counts `+0x292` down, conditions `+0x3b9` down by 32), `0x597164[item
/ 3] x (100 - c) x 9 / 1000` (item 9 never worn): the whole job's price at
the dock's own rates (0x4b1fc0 charges it step by step).
`Slot::price`, `Scene::upgrade_price`, `repair::ship_repair_price`.

Ships' main page (0x4e6980): move, stop, two spell slots, fire, board,
ram, behaviour, reload, repair, building, hero/treasure; enabled by a
capability mask (+0x261, all set but bit 11 for boats and merchants, bit 8
for the rest; 0x4e6930). Building opens page 2: a settlement (0x16) in
slot 0 and the forts 0x17-0x19 in slots 1-3, each only if the player's
technology allows it (player `+0x4e` bit 6, `+0x45` bit 6, `+0x4e` bit 5:
permissions 73, 1, 74), an empty slot left where not; any ship may build
them. No price is tested: each is enabled by the capability bit 0, the
price (0x42d8b0) only stored for the help line (`+0x32` on); nor does the
founding command (0x4aa990: kind 2 needs bit 0, kind 1 bit 1). A settlement's building page (0x43c740, page 2) packs what may be
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
  word and its index less the header (the settlement sells it); the
  pointer's mode is left as it is (0x4f73f5 calls no 0x516940), so
  more can be sold;
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
0x425990: `+0x104` = 1, `+0x108` = the ship's id).

The way there (event 0x10 `{3, 1, id, 0xff}`, the order 0x4aa840,
`+0x696` 3 run by 0x4b7370 by `+0x6a1`) is the one under "Their orders
at ruins" (Load), for a standing target too: near at the order (0x4abf60
within 2: to a port's or dock's berth vt+0xb8, else the anchor, a ship's
cell) closing in with `+0x6c6` the target's player, else sailing with
0xff; sailing (0x4b5d10 -> 0x4b9140) finds a standing target by player
or id, plans to its berth (0x50ebf0: vt+0xb8 for buildings below 0x44, a
ship's cell) and closes in once berth and anchor are within 2. Closing in
(0x4b73a0 state 1) looks with 0x4b7f10(5, ..., reach 1): a ship as the
boarder's look; a building found where one of its cells' object slots in
the box holds it, its point the berth (vt+0xb8), or the anchor for the
classes of mask 0xe01c from 0x32 (0x34-0x36, 0x3f-0x41; these also set
the ship's `+0x594` 1), the reach 0 for 0x39, 0x3a and 0x3b (so the ship
must lie on the berth), the distance the larger axis from the ship's
cell when within 2 on both; within reach 1 (and `+0x6aa` the box's
end-corner quirk), further a mode 6 plan through the box to the point
(0x4a6270 with the building flag, which lets 0x4a6330 take a goal on
land to the region finder 0x4a5a10), out of 2 nothing. Within reach,
0x4b84b0: 0 while `now - +0x6be` is below 0x5c7740[the player's byte at
0x90 of its 0xba0 record] or a contact is held (`+0x8e6`), for 0x39-0x3b
unless the berth is the ship's cell, for a ship target without `+0x261`
0x1000; else it throws a hook (shot 0x14ba kind 9, its chance 0x4bf7f0
with the 0x499ca0 test, `+0x6c2` from 0x598de4 by distance / 10) and
returns 1: state 7 (`+0x6a0` 1). State 7: the hook missed (`+0x6a2` < 0)
back to 1; held, for a fort (0x34-0x36) within 1 of its anchor, its
`+0x6c2` time passed, vt+0xb0 0 and its `+0x1e0` the ship's `+0x4c`,
or a port, yard or dock whose berth is the ship's cell and the time
passed, or a ship whose contact is this one in its type 5, state 8 (0x4b37d0 holds the commands, 0x4b54a0 pulls); state 8
(0x4b57e0) to 10 (furl, 0x4b8aa0) for a building; 11 the walkway; 3
sends 0x2875 (0x4b3930, a building) and goes to 4, lying linked; the
contact gone: 12 (walkway up), 6 (sail out) and back to 1 with
vt+4(8, 0x90). The port (`Scene::load_tick`, `load_look`) runs the order,
the sailing and the look; at reach 0x4b84b0 (`Scene::load_throw`,
called from state 1 with no enemy test, `param_3` 0) and states 7-12
(`scene/load_run.rs`, `Ship::load_run`) as below.
**The machine past the throw** (0x4b73a0, one call a tick; `now` the
game tick):
- The throw: 0 while `now - +0x6be` < 0x5c7740[byte +0x90] (85 ticks at
  level 0, 66 above; `Scene::hook_reload_ticks`) or a contact is held.
  A building: its anchor's direction (0x59791d), 0x39-0x3b only from the
  berth, distance 0; a ship (`+0x261` 0x1000) the distance 0x557440
  ((min + 2 max) / 2) between the hull centres. The chance 0x4bf7f0 is
  100 for item 9 (no draw); 0 when 0x499ca0 fails, or at a ship slipping
  (0x4b5140 x 3000 / `+0x59b` < 17); else `+0x6d7` the target's cell.
  `+0x6c2` = word 0x598de4[14 (d / 10)] / dword 0x599b40 (19): 3 ticks at
  a building. A building's aim vt+0xc (0x423520 for port, yard, dock and
  fort: two draws of its generator), and when the point is out of the
  `+0x6d7` cell the chance 0 and vt+0xc(0) again (two more draws). Shot
  0x14ba kind 9; `+0x6be` now; state 7.
- The landing: a miss falls in the water (0x4983d0: 0x4e06a0, contact
  freed; state 7 -> 1). At a building 0x4bc140 needs vt+0xb0, 0x40dfe0
  for all four vtables (dumped vt+0xa4..+0xb8: 0x40dfd0, 0x425720,
  0x4258c0, 0x40dfe0, 0x40e020, 0x40e050): facing `+0x1dc` >= 0, `+0xa4`
  1, with `+0x1d8 & 0x10` the town `+0x1c8` set, and `+0x1e0` 0; then
  0xcb7, which 0x4e7640 (fort 0x419f80) answers with `+0x1e0` = the
  ship; 0xcb8 clears it, as does 0x2874 from its town (0xcb9 to it).
  At a ship 0x4bbfc0 with no player test: the target grappled as a
  boarded one (prey state 9, `Scene::grapple_prey`).
- 7: released (`+0x6a2` < 0) -> 1 (contact 0x4e02a0); no contact -> 1;
  type 300 stay; 0x32: a fort with 0x557440(cell, anchor) >= 2 is
  released, else the hold once `now - +0x6be >= +0x6c2`, vt+0xb0 0 and
  `+0x1e0` the ship; 0x39-0x3b released off the berth, else the hold
  once the time passed; 200: the target's contact back at this ship in
  its state 5 -> hold. The hold (0x4b74fb): 0x4b37d0(0x2000) (commands
  but the stop dropped till state 6's end, `Scene::load_holds`), `+0x25d
  |= 0x10`, 0x4b54a0 (as the boarder's), state 8.
- 8 (0x4b57e0, two points a tick): released or no contact -> 6 (0x4c3320,
  contact 0x4e02a0); done -> 10 (a ship only once its target is in 10,
  3 or 4).
- 10: a hull step each 5 ticks (delay 4): frame max(frame, 8h+0x58)+1, at
  8h+0x5c it is 8h+0x5b, the line freed; 11 with the walkway (0x4b8aa0)
  or 12 without a contact: 20 ticks.
- 11, a frame a tick: released, the walkway back down to 0 then 6; at its
  end (18 frames, 18 ticks) no contact -> 12, a building 0x2875
  (0x4b3930(1)) and 0x4ad9f0, state 4.
- 4: no contact -> 12. A human's (game `+0x1a1d`[nation x 4 + slot] 1 or
  2) 0x4b71f0(1) stays while the contact holds and not released with
  `+0x6e5` 4; a computer's 0x4bb3a0 only with `+0x6e9` set, which is
  never set: the block `+0x696..+0x712` holding it is zeroed by the
  constructor 0x4dc3f0 and 0x4b8d10 (state 6's end), saved and loaded
  as a block (0x4e150b, 0x4e2365), with no other writer (byte, word or
  dword, direct or through `lea`). So its transfer (0x4e4120 records,
  0x4bda00 mode 1, 0x4ad9f0 again) never runs and a computer's ship
  leaves at once. No computer orders a load anyway: event 0x10 kind 3
  comes only from 0x4e2ba0 case 0x1f sub 2, and command 0x1f only from
  0x4d9fa0 (sub 1, the gather zone's unloading) and 0x4d74a0 (the
  navy's 0xaa task, made by 0x49dfb0 from 0x49e230, the interface's
  0xc458). Else 0x2876 (0x4b3930(0)) to a building and 12.
- 0x4ad9f0, the unloading (state 11's end at a building, and 0x4add10's
  end): fish `+0x47f` (`+0x527 - 100 + n x 100`) through 0x4a9530 kind
  0 and 0x4a8c30, only at a building of family 0x32 not a fort
  0x34-0x36, experience 10 (0x4b4680); a leviathan `+0x480` through
  0x4a8fa0 class 0x192 (x 125 x 40 / 100) and 0x4a8c30 2500, 400; amber
  `+0x481` through 0x4a9530 kind 2 and 0x4a8fa0 class 400 (x 3 x 40 /
  100), 100. Then the treasures `+0x489 + k x 0x18` from the last
  (record: kind, -, category `+8`, field 7 `+0xc`, field 8 `+0x10`):
  category 4, or 0x10 for a computer's (side 3 or 4): kind 6 through
  0x4a8fa0(field 7, field 8) (400 amber, 0x191 fish, 0x192 a
  leviathan's; another class credits nothing) and taken off (0x4bd500);
  others through 0x4bdc10 (0x34-0x36 and 0x40: 0x424b30; 0x39-0x3b:
  their town `+0x1c8`'s 0x441480; else 0), taken off and announced
  (0x4a1af0) when taken. Then `+0x5c7` 1-234 is drawn again, 91 + r %
  10 (the ship's `+0x18`), 0x4beb50 and the viewer's panel
  (0x4bebe0, 0x4bc880). Categories (0x4bd0c0 -> 0x4b9740, at taking):
  2 kinds 0-5 and 8; 4 kinds 6, 7, 27; 8 kinds 9, 17-26; 0x10 kinds
  10-14; 1 kinds 15, 16; player 16's all 4. The map's goods (kind 6)
  carry field 7 0x191 (miss003, miss007).
- 12: the walkway up a frame a tick (18 ticks from 17), then 6.
- 6, once a tick: the line back to the cell's middle (0x4c3320; none
  when already there: count 0, so `+0x6b6 < count - 1` unsigned holds
  forever and state 6 never ends) two points a tick; the sail 8h+0x5b ->
  0x59 -> 0x58 (2 ticks). At the line's end and the sail done: frame h x
  4, contact 0x32 0xcb8 + 0x4e06a0 (other 0x4e02a0), 0x4b3850, 0x4b8d10,
  notice **0x13f** {now, ship +0x60, ship id, target `+0x6cb`} (0x4a1980)
  to the group, `+0x25d &= ~0x10`, vt+4(8, 0x90).
- 0x4e02a0 frees a contact: 0x32 with 0xcb8 (0x4b3970(0)); 200 the
  target's line; 300 the bomb.
- Released by: a stop (0x4aa540: `+0x25d & 0x10`, state not 0 / 1:
  `+0x6a2` 0xff; linked, `+0x6e5` 4); Finish reload from the ship
  (0xc45b -> 0x4bc750 -> 0x4aa2c0 case 4, linked); from the building
  (0x4e7640 / 0x419f80 with `+0x104`: 0x2876 to `+0x108`, `+0x104`,
  `+0x108` and `+0x1e0` cleared, 0xc453), the ship's 0x2876 (0x4dd3bc)
  freeing its contact with 0x4e06a0 (no 0xcb8); the target gone. Other
  orders are dropped.
The hook decoded further: 0x4b84b0's shot carries a record {200, 3,
ship, id}; as it flies 0x496f40 makes the shooter's contact type 300
(the hook in the air, pointing at the bomb `+0xef`). Landing on a
building (0x4967b0: class 0x45 at once, others once 0x49a270 passes) it sends
the building 0x138e with the bomb's id for kind 9 (only while the hook's
record `+0xef` lives); 0x423d60 on 0x138e kind 9 finds the bomb
(0x51f1f0) and, with the building's `+0xa4` 1, calls 0x4bc140 on the
shooter: holding the hook as its contact and vt+0xb0 of the building
non-zero, the contact becomes {0x32, 4, building, its id}, 0x4b3970(1)
sends 0xcb7 {building id, ship id}, the line moves to the ship (0x50ce10
slot 2, `+0x8d2` from the bomb's `+0xfb`), and for 0x34-0x36 and
0x39-0x3b `+0x6d7` is the anchor. A kind 9 landing in 0x4983d0 calls
0x4e06a0 on the record's ship and clears the bomb's slot 2. 0x4b37d0 (state 7 to
8) keeps `+0x25d` in `+0x69c`, clears it and ands `+0x261` with 0x2000;
0x4b3930(1 / 0) sends 0x2875 / 0x2876 {building id, ship id}.
Not ported: vt+0xb0's town test (`+0x1d8 & 0x10`, `+0x1c8`), 0x2874's
clearing of `+0x1e0`, the porters (`NOSIL_`,
code 5 carries between ships, 0x4bc7c0), the hold's `+0x85` and
reserved-cell release, the hook's landing sound at a building, the
building flag of the plan (no goal on land) and `+0x594`; of 0x4ad9f0,
the category kept from the taking (the port derives it from the kind and
the present owner), the stats 0x4a9530, 0x4a8c30's share 0x4a8f40 and
0x4beb50; and the `+0xa4` state test of 0x424af0 for player 16's
structure 0x40 (its room, 0x5c1094[0x40] = 2, is kept).
The treasure hand-over (`Scene::unload_treasures`) also runs at the end
of the port unloading (0x4add10 calls the whole of 0x4ad9f0 once the
porters' count `+0x45` reaches `+0x49`, the port its contact `+0x8e6`;
`scene/trade.rs`). It is the only way a ship's treasures go ashore
outside the load panel: 0x4bdc10 is called only from 0x4ad9f0 (0x4adc47)
and the panel's move between a ship and a building (0x4e3ec9); 0x441480
from the panels (0x41b3fd fort, 0x4421cd / 0x443cd2 town, 0x4e7b39 port)
and 0x4bdc10; 0x424b30 besides from 0x441480 and 0x4bdc10 only from the
creation of the map's held treasures (0x415173, 0x41d60d, 0x42815b),
the fort panel (0x41b46b, 0x41ba5f), message 0x158c1 (0x4271fc) and a
building's tick taking the class-700 treasures lying on its cells
(0x425b38, then 0x158c1 to them). So no ship hands treasures over by lying
alongside an own settlement (the earlier `deliver_treasures` model is
gone). 0x424b30 case 6 credits through 0x4a8fa0 with `field 7 + 0x190`
(0x424bb4 `add edx, 0x190`): field 7 0 amber, 1 fish, 2 a leviathan's;
the map's goods (field 7 0x191, class 0x321) credit nothing when handed
to a settlement or fort in the load panel, though 0x4ad9f0 credits them
as fish (0x4a8fa0 with field 7 itself). 0x441480 unpacks kind 7: field
7's low word the type, byte 2 the item, field 8 the count; type 1 a unit
of the item with condition 100 each through 0x440d40 (refused while the
stock is mended, `+0x10c` with `+0x110` 2), 2 armour of that level into
`+0x248` (not level 0), 4 a ram into `+0x24c` (not level 3); any type
returns 1, unannounced (`Scene::unpack`).
A settlement's own
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
amber, 3-5 fish, 6 goods by field 7 + 400 credited; the rest stored if the building has
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
rest kept while there is room, 0x424af0: 1 for forts, none in state
`+0xa4` 2 (fallen), 3 or 0xff) and out of it to a settlement through 0x441480 or
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
lower, set at each step (0x4e04d0; 0x4e4fb0 at the lighting writes the
y with `- 0xf` where the step has `+ 0x17`). The layer's
remap: 0x4e4fb0 sets the sprite's shadow table (0x50e050 with 0x5c1358,
0x55dd10) and 0x50e130(5, 1, 0x26, 0), which, the layer having no
picture yet (0x50e130 acts only with `+0x41` not -1), does nothing; then
0x4e1bb0(5) loads `FIRE<+0x7a>` (0x4b1750, `FIRE%1d`; 0x50d080) and
puts 0x50e240(5, 1, 0x5c1858, 1, 7) on it (0x4e1ff4-0x4e2003; 0x55dd40:
flag 0x100, table +0x5c, value +0x60, count +0x64 on the frame and its
children). The blitter (0x57b7f0, called from 0x572622, which passes the
table only with the flag and the sprite's table `+0x63c` only for count 1
and table 0) draws a colour in [value, value + count) as `table[(colour
- value) x 256 + the pixel beneath]`: the flames' colours 1-7 blend with
what is beneath through 0x5c1858 as a building's fire does, the rest
(0x26 among them) painted. A re-ignition with another size calls
0x4e1bb0(5) again, which reloads the same `FIRE<+0x7a>` (the size byte
is written only at the lighting; the load 0x5461d0 leaves the frame
`+0x45` alone). With the phantom (`+0x253`) 0x4df380 then ghosts the
layer for its owner (0x50e460 with 0x5cdd2c) or hides it from others
(0x50e350); the flames' frames are of kind 1, which the blitter draws
without the ghost's mask, so the owner sees them whole (see the
phantom's paragraph below). 0x4e4fb0 ends, lit anew or not, with
0x50e570(mode 1, the ship's cell `+0x5a3` / `+0x5a7`, event 5)
(0x4e5107-0x4e510f): `SND_5` lists `firee1` alone, played at the ship.

The step (0x4e04d0, from the ship's tick at 0x4dca16 while `+0x8ca` is
set): with the frame `+0x45` below the count `+0x49` and the game tick
more than the delay 1 past its time `+0x81`, frame + 1, time = the tick,
and 0x50d660 shows that frame (0x55ec70 takes it modulo the count, so
the step reaching the count shows frame 0); with the frame at the count
the layer is removed (0x4e0260). The time is the game tick at the
lighting, so the first step comes 2 ticks on, then every 2; the count
reached, the next tick removes it. The port steps them in its game-tick
loop with the ship statuses (`step_ship_fires`). Not decoded: whether the new layer
(0x50cbf0 with show flag 0) is visible before its first step, and so at
the lighting's place, 38 pixels higher; the port shows its first frame at
the step's place from the lighting. Ported in `scene/fire.rs` (the
frames `marked_frames(.., None, true)`, drawn as a building's fire).
`SMOKE` (layer 6) is only restored from saves; nothing starts it in play.

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
the ban cursor and takes no click. (Checked by hand on the original: a
moorage goes beside the settlement or beside one of its buildings or
forts, and is refused two cells away with none of them beside it.) A ship founds
(0x4aa990, 0x4b3370, checking with 0x4e4420 on arrival and after landing)
a fort on a shore tile (the nation's permission, then 0x4eb with flags
0x20 and no id) and a settlement on 2 x 2 free inland cells with a shore
tile in the 12 about them (0x42e1e0); it berths beside it from anywhere
(0x4b2710, distances 0x557440 from the ship's cell, the first of equals:
for a fort the nearest side cell, east, west, south, north, on the map,
that 0x499990(cell, 0) takes, or 0x499930 with the ship's draught while
it has a wait `+0x591`; a cell of index 0 is skipped; for a settlement
the nearest water cell A (layer-0 type < 8 only) of the 24 linear pairs
0x5c7500 + 0x15c / + 0x1bc (0x499e00, 0x499f00: the 6 x 6 ring's sides
and the 4 x 4 ring's corners, from the site's top-left) on the map and
less than half the map's width across, whose land cell B (type 8 or
more, `+0x740`) has an empty slot (0x4998d0; B is not tested against
the map, the port refusing it off the map); pair 19 has A == B; none:
-1, the order dropped). A moorage's front is the
cell its record field 10 faces (0 east, 2 north, 4 west, 6 south), where
ships berth.

**A ship's fort.** The founding command (code 0x20 group, 0x4aa990: kind
`+0x824` 2 found, class `+0x826` -> `+0x73b`, cell `+0x82c` -> `+0x73c`,
`+0x830` -> `+0x744`) runs the landing states 0x4b3370; on landing (state
6) 0x4b2d60 sends 0x1a67 to the world: a landing party (LandingClassTy,
class 0x1fc, 0x4221c0) with the record (disassembly 0x4b2e0b-0x4b2e71)
{class, 1, nation, slot, type 1, x, y, 100, town cell 0xff, 0, ...,
field 66 = `+0x744`}, the unit counts all 0. The party walks to the site
(0x421d00, 0x422140) and there (0x421f00, its kind 2): the site's slot
empty (0x499900) and the player holding 0x42d8b0(class) amber (0x4a8d00),
that price is taken (0x4a8d40) and the fort is created (0x1494); else
nothing; the party then removes itself (message 10). So the fort costs
the building alone, comes with no units whatever the default arming,
takes nothing from any town's stock, and never has a town (0x427f50
message 2 skips 0x427de0 for the cell 0xff; 0x2490 ignores it; the fort's
own 0x419530 runs only for the cell (0xff, 0xff)). FortressClassTy
0x419f80 message 2 (0x41a760): a fort of a side 3 or 4 player (a
computer) holding no units takes each item of 0x5966b8 for its class
(0x419390 at 100, room 0x417ef0) and pays the whole list, amber
0x597164[item / 3] and fish 0x597170[item] x 100; then, record type 1,
`+0x1bc` = field 66 (a tag carried in the building's reports to its
player's `+0x34` object, 0x424c3c, 0x424f63, 0x4250bc, 0x426a29,
0x427cc7; not kept by the port). A settlement (kind 1): the slot empty
(0x499900 on the record's cell) and the amber held or the player of side
3 or 4 (`[game +0x1a1d + nation x 4 + slot]`, a computer), the price is
taken (0x4a8d60: amber less the price, 0 if below) and message 0x1491
creates it: a computer founds it without the amber, its amber left at 0.

The landing party (0x4221c0 message 2, record type 1): its generator
seeded `[game +0x2ee5] x id`; it starts at the point the ship passes
(0x4b35c3: the gangway picture's (`+0x8c6`) top left plus 0x59a188[dir
`+0x76b`]), its goal cell the record's (`+0x89`, `+0x8d`; kind 3 `+0x75`,
`+0x79`), the goal point the 2 x 2's middle (`(x + 1) x 64`) for a
settlement, else the cell's centre. On the goal cell already (0x421ea0)
it acts at once (0x421f00); else a quarter-cell grid of the goal's land
region (0x42e590, `x 4`; 0x42e720 with `param_4` 4), the 4 x 4 quarters
of its own cell and of the goal cell opened, and a route 0x55ca10 from
its quarter to the goal's (heading -1); none, it removes itself; else each
entry becomes the quarter's centre (`q x 16 + 8` from the region's
corner). Each leg (0x422140, 0x421d00) runs the picture `LNDSPR_<dir>`
(0x421bb0, the entry's direction byte; `+0x7d` 0, so a frame a tick,
from the leg's start to its end by `frame / frames`), drawn at its point
less (`w / 2`, `h`) through the shadow table for colour 0x2a (0x50e130(0,
1, 0x2a)); at a leg's end it is on the goal cell or at the last entry:
0x421f00, then message 10. The grid (0x42e720 with 4): a cell not land
all shut; else each shore edge shuts its half (north rows 0-1, east
columns 2-3, south rows 2-3, west columns 0-1); nibble 5 at 1 or layers
6-7 set shut all but the outer ring's quarters on the edges without a
shore (its test of a layer 6-7 of type 4 against 0x42e6e0, the tile set
records' types, always takes that branch on the same types). The start
and goal cells' 16 quarters are opened by linear index (off the grid
they land elsewhere in it). The first leg goes to entry 1. LNDSPR_ has 8
frames: a leg is 8 ticks.

The ship's landing (0x4b3370, run by the ship tick 0x4ab930 while
`+0x25d & 0x20`; `+0x755`):

- **1**: off the berth `+0x756` it is sent there again (0x4a6270); there
  with `+0x58c` 0, the site checked (0x4e4420): refused, the order is done
  (message 8, 0xa0); else 0x4b2ff0, `+0x74d` = `+0x751` = 0, state 2, its
  commands held (0x4b37d0: `+0x261 &= 0x2000`, `+0x25d` saved and 0x20).
- **0x4b2ff0**: the cell `+0x740` (B; the site for a fort) from its own:
  direction 0x59791d (`+0x76d`), shore group 0x4b3770 (< 0: returns,
  `+0x76b`, `+0x748`, `+0x76a`, `+0x76c` and the count `+0x75e` left as
  the last close-in had them, 0 on a new ship, zeroed at 0x4dc3f0; its
  line `+0x766` was freed and zeroed (0x557750) as that landing ended,
  0x4b1c60 / 0x4b1d40, so 0x4b1a90 and 0x4b1d40 count the kept points
  without moving the hull); `+0x76b` the direction, `+0x748` the group;
  slanting (odd), `+0x76c` 0 and the line freed, `+0x76a` and `+0x75e`
  kept; straight across, a line (0x4b5220) from the hull's
  top-left to its cell's moved 0x599c44 / 0x59a1c8 / 0x599ee8 / 0x59a4c8
  `[group + type x 12]` towards B (the amber close-in's tables), and the
  side-on heading (2 / 6 across, 0 / 4 up and down) fewer turns away
  (0x4b5390, the second on a tie; a hull past its rotations goes to `h x
  4`), the way `+0x76c` its sign, the hull on `h x 3 + 0x20` to turn.
- **2** (0x4b2a40 -> 0x4b1a90, every tick): a point of the line forward
  (two back with `+0x73a` < 0) and a hull rotation the way it turns
  (called off: the turn stops); at the line's end and turned (or back at
  its start) the site is checked again: good, `+0x751` = `+0x74d`, state 8;
  else state 3, `+0x751` = the count.
- **8** (0x4b89b0): the hull's delay `+0x7d` 2 (a step once more than 2
  ticks have passed): from `h x 8 + 0x58` a frame a step; on the fourth
  it stays at `+ 0x5b`, `+0x25b` = 5, state 4. **4** (0x4b2d50 ->
  0x4b2b00): slot 1 picture 0x3e6 as `AMOST_<+0x76d>` at the hull's
  top-left plus 0x599e88 / 0x59a408 (groups 4, 5, 11) / 0x59a468 (6, 7) /
  0x59a128 / 0x59a708 (0, 1, 10) / 0x59a768 (2, 3) `[type]` (the amber
  walkway's; other groups leave it at 0x545e70's (0, 0)), frame 0, state
  5. **5** (0x4b2f00): a frame a tick (delay 0); past the last (12, north
  10) slot 3 picture 0x3de as `ANOSIL_<+0x76b>` at slot 1's top-left,
  `+0x76` 9, state 6. **6**: a frame a tick to 0x17 (0x15 north); there,
  kind 2 (found), 0x4b2d60 makes the party at slot 1's top-left plus
  0x59a188[dir] ((46, 17) east, (6, 21) north, (6, 17) west, (7, 48)
  south), 0x4b4680(200) (crew experience) and 0x4a15a0 (notice 0x130 to
  the ship's zone, `{time, ship, x, y, class}`); slot 3 goes, state 7.
  **7**: slot 1 a frame back a tick; below 1 it goes, state 3. **3**
  (0x4b1d40): a hull step backs it two points (`+0x751 -= 2`) and unfurls a
  frame (down to `h x 8 + 0x58`); below 1 it lies in its cell's middle,
  hull `h x 4`, `+0x25b` 0; then 0x4b3850 (commands and flags back) and
  the order done (message 8, 0xa0).
- `+0x73a` < 0 (a stop, 0x4aa540 -> 0x4abc60, in any state but 1, where
  the stop just clears 0x20): 8 and 4 go to 3, 5 removes both pictures
  and goes to 3, 6 removes slot 3 and goes to 7.

The port runs it at the game tick and draws the walkway and party with
the amber walkways. Its lines are the slide's (`grapple::slide`) points.
Not ported: the crew's experience, the notice 0x130 (the port's zones
take no founding notices), lightning on a party (0xda93, 0x50e750), the
0x8e6 contact test of 0x4b2a40 / 0x4b1d40. Not settled: the hull clock's
phase (`+0x81`) at the landing's start (the port counts from it), and
whether the party's first tick falls on the tick it is made (the port's:
the next).

A computer's town order of a fort comes armed. The land fleet's build
part (0x428a70) sends the town command 0x20 `{x, y, part[7], kind, 1,
1}`: byte `+0xd` 1 always. The town (0x441ba0 case 0x20) queues 0x138c
`{x, y, class}` with `+0x14` = that byte, and 0x43b290, the building's
price paid, runs 0x43f440 for classes 0x34-0x36 when it is not 0
(disassembly 0x43b842-0x43b894). 0x43f440(town, record, &amber, &fish)
sorts each of the town's twelve stock places best first (qsort 0x58791f
with 0x4478e0, on the condition byte). For each item k of the class's
list (0x5966b8) it takes the best unit of the item in stock; else it buys
the item if 0x43da20(`town, 1, 1, 1, item, 0, 1, 0, 1`) is 0; else it
takes the fallback 0x596734[k] from stock, or buys it untested. Then,
while the units' weight (0x59729c by group) is under the room (0x5967a8:
4, 7, 10), it adds the fallbacks j = n - 1 down to 1 (n the list's count)
the same way. A unit taken keeps its condition; a bought one is 100. Each
goes into the record: its count at `+0x58 + 4 item` (fields 22-33), its
condition at `+0x88` in the order taken (fields 34 on). The price,
0x597164[item / 3] amber and 0x597170[item] x 100 fish, is paid after
(0x4a8d40, 0x4a8be0, both floored at 0). The fort's message 2 finds
units and gives no default arming. The port: `Scene::town_fort_arming`,
in `place_building_facing` for a non-human player's town fort.

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

## A town's buildings

A building of classes 0x34-0x3e (WorkLandObj 0x427f50, `+0x1d8 & 0x30`:
the BuildingTy constructor 0x427750 with 0x15 for the schools, smithy,
workshop and temples, 0x1e for ports, yards and docks (0x4e7f50), 0x20 for
forts (0x417814)) keeps its town `+0x1c8`, a town cell `+0x1cc` /
`+0x1d0` (its record's fields 8 and 9, 0xff for none) and `+0x1d4`, set
1 by the constructor. Nation 4 (the barbarians) never takes a town.
- 0x427de0: the object in the town cell's slot, of the building's nation
  and slot, is its town (`+0x1d4 = 0`); else none. No class test.
- A town's order 0x138c (0x43b290, the player's click and the computer's
  search alike) makes the building from a record of type 1 (`+0x10`) with
  the town's anchor as town cell; 0x427f50 message 2 runs 0x427de0 for a
  type-1 record, so the building is the ordering town's from its creation.
- At the start (message 0x8d1, broadcast by 0x522d50 to every object in
  the world's order) each building with a town cell runs 0x427de0 and,
  standing, tells its town at once; each settlement or town (not in state
  0xff) offers itself, 0x441190: message 0x2490 {town, anchor} to the
  objects on the cells x-1..x+2, y-1..y+2 of its anchor that are a
  player's (`+0x57` 1), of its nation and slot, of classes 0x34-0x3e. A
  building takes 0x2490 only without a town, with a town cell (not 0xff)
  and not of nation 4: the cell becomes the town's anchor, 0x427de0, and,
  found, it offers the town on to its own 3 x 3 (0x427e40). The calls are
  direct, so the chain runs depth first. (Checked over every mission: the
  result does not depend on the objects' order; the buildings left without
  a town are those whose record cell is 0xff.)
- Each tick (message 0) a building standing with `+0x1d4` 0 sets it and
  posts 0x2873 {its id} to its town: the town (not in state 2, 3, 0xff)
  appends it to its list `+0x1d0` (count `+0x1c8`) once and counts it by
  class (workshops `+0x1fc` and their pointers `+0x1d4[]`, the first five;
  smithies `+0x200`, `+0x1e8[]`; ports `+0x208`, yards `+0x20c`, docks
  `+0x210`, schools `+0x220`, temples `+0x224` / `+0x228`, forts `+0x218`,
  `+0x214`, `+0x21c` with their being-built counts lowered; mask `+0x238`).
  So a building joins its town's list on its first tick standing.
- 0x2874 {id} to the town (same state test) takes a listed building off and
  uncounts it. Sent by the drain 0x427bb0 when a building falls or is
  called off, and by Dock 0x416340 as a shipyard is made a dock, which also
  sets `+0x1d4 = 0`: the dock joins again, last, when it stands. A town
  falling (0x43bc30) sends 0x2874 {town} to each building of its list,
  which then forgets it (`+0x1c8 = 0`, 0x427f50).
- Saves (0x4277c0) write fields 8, 9 = the town's anchor (0xff without
  one); neither `+0x1d4` nor the town's list is saved (0x4416c0): the
  load's 0x8d1 makes them again.
- Buildings never change owner: their nation and slot bytes are written
  only by the constructors and the creation handlers (0x427f50, 0x4221c0,
  0x43aa10, 0x441ba0), so a building's town never changes but by the
  0x2874s above.

The port: `scene::towns` (`Towns`: each building's town, those yet to
tell it, each town's list; saved, and made again as at the start for a
save without them). Its `home_settlement` is the building's `+0x1c8`
(a settlement its own). The port's falls that do not go through the
strike leave the list at the next tick. A fort a ship founds never has a
town (its record's town cell is (0xff, 0): see **A ship's fort** below).
Not decoded: whether 0x2873's post is handled the tick it is sent (the port lists the building
the tick after it stands); 0x427b30 (a building going up whose town cell
no longer holds its town loses frames) is not ported.

## Default arming

The Building page's checkbox (string 501, `TRIGGER_BKGND` with `MARKER`,
message 0x1fdb toggling `[game+0x3295]`). While on, a new ship (0x43e5c0)
or fort (0x43e990) comes armed: each item of its list (ships 0x5964d8,
forts 0x5966b8) is taken from the stock of the yard's town (`+0x1c8`) if it holds
one, else bought (amber 30/50/70 by group, fish 200/200/300 for marines) if
it may be made (permitted, with a workshop; swordsmen a smithy), else its
fallback (0x5962f8, 0x596730) taken or bought; ships then add fallbacks from
the last until their strength (1, 1, 2 by group, 0x59729c) reaches the
type's minimum (0x597ae4). Armour (0x596268, fallback 0x596238) costs 30/60
and a ram (0x5962c8, 0x596298, counted from 3) 25/50. A fort is offered only
if every fallback item can be had (0x43d430), the stone fort only at a town.

For a ship, "if it may be had" is 0x43da20's full test (`town, case, 1,
1, x, 0, 1, 0, 1`, case 1 an item, 2 armour, 3 a ram), asked of the yard's
town and its player for each piece alone: the permission, the arming /
armour / ram level studied, the town's own workshops (`+0x1fc`, for items)
and smithies (`+0x200`, for items 2 and 5, armour and rams), else -5; the
town not standing (`+0xa4 != 1`) -1; the player's amber for that piece
now -2, its fish in hundreds (player `+0x2c / 100` against 0x597170[item])
-3; then an idle maker: marines the town itself (0x43b100: busy with
anything but its training, job 1, -1), weapons one of the town's first
five workshops (`+0x1d4[]`, 0x435e10: not standing -5, busy with anything
but its weapons batch -1), armour and rams one of its first five smithies
(`+0x1e8[]`, 0x433c40, anything but its refits); the first idle answers
0, else -1 if one was busy, else -5. Only 0 buys; anything else takes the
fallback (bought untested). The test "may be made" of the menus and of
0x43da20 case 4's arming test 5 (`param_3` 0) stops before the town's
state, so it reads only the permission, the level and the town's counts.
The port: `Scene::may_buy` (not ported: a workshop busy with its town's
stock repair, job 5, which the port keeps on the town alone), `can_make_by`
with the town's counts; a fort's arming (0x43e990) keeps "may be made"
(its test not read here).

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
  modes 7, 12, 13, 15, 20, 30, 40 and 50 only (after an order sent at
  0x51872d the switch at 0x518734: 12 and 13 the tick and the mode
  reset, 0x516940; 15 and 40 the tick, the mode kept). It does not follow gathering or
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

**The ships' route finders** (`walk_path::route`, `route_exact`; checked
against the original's machine code under an emulator: 4000 random grids
and 240 cases on miss003/005/009's grids, routes, reached flag and the
flooded map all equal). Both work on the plan's copy of the ships' grid
by draught (0x5c93c0 from 0x5c93bc / 0x5c9398, built by 0x49a950: open
when the layer-0 type is below 8 and the layer-1 type is not 5, for deep
draught not 3 either; the ship cells 0x4a4e20 closes, -1) and return
5-byte entries `{short x, short y, byte dir}`, the start first. Every
ship planner passes the heading 0 (east).
- **0x55ca10(map, W, H, s, g, heading, &n)**, the exact route: NULL with
  an end off the grid or `map[s]`, `map[g]` not 0. Two straight legs if
  every cell is open (0x5718f0: the axis or the diagonal the way leans
  first, the one nearer the heading first, then the other order), the
  `max(|dx|, |dy|) + 1` cells, each byte the leg's direction. Else a
  flood from the goal (0x557910 over the whole grid, `2 (W + H)`
  generations, no head start, ending on reaching the start; see the
  wavefront below): NULL when the start is not reached; then from the
  start to the goal, each step to the diagonal neighbour of least
  value + 1 (a tie: the one in the way it went), then to a straight one
  of less value, or of equal value when the way it went was straight and
  not the diagonal just taken; cells <= 0 passed over; the way it went
  starts as the heading; it ends when a neighbour is the goal (the scan
  order 1, 3, 5, 7, 0, 2, 4, 6 decides which). Each byte the direction
  stepped in, the start's 0.
- **0x55ba50(map, W, H, s, g, heading, tol, &reached, &n)**: NULL with an
  end off the grid (no test of the ends' values). Two legs as above, a
  leg also ending within `tol` (0x557440), up to the first cell within
  `tol` of the goal (0x557480; the start written twice when it is);
  reached 1. Else a flood from the **start** (0x557910 with the heading's
  head start, d12 = heading); the goal if reached and `tol` is 0, else the
  ring search about the goal (as 0x55ba50 for the creatures, below; NULL
  for no cell); the same trace from that cell back to the start (the way
  it went starts as none, -1) and the entries reversed; reached =
  `0x557440(cell, goal) <= tol`. A cell that is the start gives `{s, 0},
  {s, 4}`, which 0x4a6330 takes as no route.
The best cell and its direction carry over between steps of the trace;
since every reached cell has a neighbour at least 2 lower, it always finds
one (no uninitialised read). So the exact and the near route round the
same land differ: one floods from the goal, the other from the start.

Which one (0x4a6330): with a tolerance of 0 and no wait, 0x4a5740: the
goal reopened as the ships' grid has it, and if closed there 0x55ba50
with a tolerance of 1, else 0x55ca10; otherwise 0x4a51b0: no route from a
closed start, the goal left as closed if a ship lies there (a plain move
passes keep-closed 1), 0x55ba50 with the move's tolerance. A plain move
(0x4a5f30) plans to the clicked cell with a tolerance of 1 when 0x4999e0
refuses it (layer-0 type 8 or more, layer-1 type 4, 5 or 6), else 0. The
ships' behaviours move through 0x4a6270 (11 callers, 0x4ac8d0 ...
0x4c26fa), all with a tolerance of 0: the first plan, not waited, takes
the exact finder (none for a goal it cannot reach, a tick of wait), and
the next, waited, 0x55ba50, so a ship heads for the nearest cell it can
reach. A plain move's first plan (0x4a5f30 with `+0x593` set: 0x4a6330
directly, then the snapshot 0x4a6100) finding no route leaves the
ship's route as it was (0x4a6330 replaces it only on success) with one
more of wait (`+0x591`) and `+0x57b` the tick; 0x4a6150 then plans again
once 25 ticks (`+0x57f`) have passed and something changed (the wait,
the ship's cell, the goal, or the ship lying still), waited (0x4a51b0,
0x55ba50 with the move's tolerance): no route, or one of a cell (2), is
one more of wait for a plain move (priority 0, not 50: retried); at a
wait of 150 or on the goal's cell the move is given up (message 0x80 sub
8). The port: a plain move to water takes 0x55ca10, to any other cell
0x55ba50 with 1; with no route it keeps its route, its wait 1, and plans
again so every 25 ticks (`Ship::pending`, `Scene::plan_pending`, through
the held-up re-plan's 0x4a51b0); held-up re-plans 0x4a51b0's; the
re-plan under way and the behaviours' plans 0x4a5740's. Not waited (wait 0), 0x4a6330 makes no plan to a goal
0x499990(cell, 0) refuses (layer-0 type 8 or more, or layer-1 type 4, 5
or 6), after its goal-is-its-own-cell test (returns 1) and before any
planner (the region planner 0x4a5a10 included); its callers choose their
own water cell. The port's `plan_route` does the same. A founding ship
(0x4aa990) sails for 0x4b2710's berth (`Scene::found_berth`, "A ship's
fort" below; no berth drops the order, and `+0x591` is zeroed after).
Not decoded, and kept on the port's own nearest water
(`plan_route_to_water`): the scout, patrol, zone and convoy orders, a
waypoint, a treasure or hero's house, the attack chase and boarding
approach (battle.rs), an amber deposit's goal and a port's berth without
a moorage front, and the berth ring of a dock; their originals' water
cells are still to read (the attack's state-3 re-plan after 0x4ab3f0,
0x4afe50 for amber, 0x4c5910's port). and the ship's own cell is never closed to its plan (0x4a4e20
and 0x4a7290 pass over it). Every plan starts from the ship's own cell
(`+0x5a3`, `+0x5a7`) unless 0x4e0b50 says it is between cells: then from
the cell it is entering (`+0x5b6`, set by 0x4a3a70 as a step is taken),
and in states 1, 4 and 5 the new route is that cell's after the ship's
own (`+0x5be` 1, state 5), so it sails on into it; its goal its own
cell, 0x4a6330 makes no plan (returns 1), and a route of one cell is
none. The between-cells flag `+0x596`: set leaving a cell's middle
(0x4a3560 for state 5, the end of a turn 0x4a3320, a plan lying still
or at a cell with no turn 0x4a6330), and on reaching a cell's middle
(0x4a3f40 at 0x4a4a8e and 0x4a4d3b); cleared as the picture's centre
crosses into `+0x5b6` (0x4a47bf) and by 0x4e0b50 for a plan made in
states 0, 2 or 3. It is read by the stop (0x4a38f0: not yet in the
next cell, it stops there), the brake 0x4a2f60, 0x4a3a70's boarding
branch, as "under way into `+0x5b6`" by the look box 0x4ac8d0 and
0x4c2bb8 (flag and state above 3), 0x4b7f10 (state 4 or 5), 0x4aa5c2
and 0x4abc60 (state 4, 5 or the boarding's step 1), and by 0x4c2230 and
0x4c26fa. The port keeps it
(`Ship::between`, `Ship::entering`) at those points of its moves; the
hunt's look box takes it. Not ported: the route's direction bytes
(the port's ships turn by the cells).

**Region plans (0x4a5a10, ShipClassTy::GetWayNew).** 0x4a6330 runs it in
place of 0x4a5740 / 0x4a51b0 when its last argument, a region, is given
with its byte `+0x18` set (after its own checks: the goal its own cell,
or 0x499990 refusing it, not waited, no plan). The region, on the
caller's stack: `+0` x0, `+4` y0 (the box's corner cell), `+8` / `+0xc`
the goal relative to it, `+0x10` W, `+0x14` H, `+0x18` active, `+0x19`
W x H shorts row by row (0 open, -1 closed). From the cell 0x4a6330
plans from, relative to the box (no bounds test): closed, no route; the
region's goal open, 0x55ca10, else 0x55ba50 with a tolerance of 1, both
with the heading 0 and none for an end off the box; the entries moved
back by (x0, y0) (16-bit adds); the route then as any other; the wait's
tolerance and `+0x5c9` are not used. 0x4a6270 passes the region on in
mode 6 only (mode 5 forces 0). The callers: 0x4ad0ba in the gathering
look 0x4ac8d0 (box `size` about its centre, clipped to the map; grid
0x499990(cell, 0), closed for a ship (0x50e600 200) in the slot not this
one, not on this one's `+0x5ab`, not the target; goal the found cell;
wait 0), 0x4b8493 in the boarder's closing-in 0x4b7f10 and 0x4c2666 /
0x4c272e in the rammer 0x4c2230 (region filled by 0x4b8d80). The port
plans the leviathan hunt's look (sub-code 10, size 5) and the rammer's
and boarder's closing in so (`Scene::plan_region`, hunt.rs, closing.rs).
Not ported: the sub-code 11 looks (0x4aeb80, 0x4af640) and the boarder's
search mode (0x4b7eb0) call with a size of 11, writing 121 shorts into a
25-short stack buffer (undefined; who sets sub-code 11 not traced;
`+0x6a1 = 2` is never set, below).

**Rammers and boarders closing in.** The ship's tick (0x4abae6), while
`+0x25d & 0x10` (set by the order 0x4aa840, with `+0x591 = 0`, `+0x593 =
1`, `+0x57b = 0`, `+0x57f = 25`), runs its behaviour by `+0x696`: 1 the
ram 0x4c1370, 2 the boarding 0x4b4da0, 3 0x4b7370; each by `+0x6a1`: 1
closing in (0x4c2230, 0x4b5df0, 0x4b73a0), 2 search (0x4c3000, 0x4b5ca0,
0x4b7eb0), 3 sailing to the prey (0x4c2ec0, 0x4b5d10). The order sets
`+0x6a1` 1 when 0x4abf60 (0x557440 from the ship's `+0x5a3` to the
prey's cell) is within 0x59aee8[type] / 2, else 3. The sizes 0x59aee8,
0x5991c8 and 0x5991d4 are 5 for every type.
- Sailing (`+0x6a1` 3): 0x4b9140(radius 0x5991d4 or 0x5991c8 >> 1 = 2,
  -1, ...): the prey's cell (0x50ebf0; a ship's is vt+0x28, tolerance 0)
  within 2 of the ship's cell by 0x557480 (2 across both axes counts 2):
  1; else a plan 0x4a6270 mode 5 to it, 0. The boarder's 0x4b5d10 calls
  it only when its cell is not `+0x6c7` or `+0x591` is set; the rammer's
  0x4c2ec0 every tick; 0 keeps the cell in `+0x6c7`, 1 sets `+0x6a1` 1
  (the rammer also `+0x593` = 1, by 0x4c1370). 0x4b9140 has no war
  test. The prey gone (0x4ac050 finds neither pointer nor id): the
  boarder posts notice 0x13c (below) and stops (vt+4(8, 0): 0x4aa540 ->
  0x4abc60); the rammer posts 0x13d and stops (0x4abc60), and so too
  with the prey within 2 but not at war (a monster, `+0x57` 2, not of
  class 0x1fb, counts at war; else the same player does not; else
  0x4efe20 < 0).
- 0x4a6270 modes 5 and 6 (6 passes the region): with `+0x593` 0x4a6330
  at once, else 0x4a6150: a plan when (`+0x591` changed, or the cell or
  the goal changed since the last, or move state 0) and not turning
  (state 2) and 25 ticks after `+0x57b`; then on the goal (or `+0x591`
  150) event 8 0x80 and 3, else 0x4a6330 (2 is 0 unless `+0x5c7` is
  0x32). Not planning: 0. 0x4a6330: 1 for its own cell, 0 for a cell
  0x499990 refuses or a route of one cell (or 2), 0xff for no route; the
  route's last cell is `+0x5b2`.
- The boarder (0x4b5df0 state 1, 0x4b73a0 alike): 0x4b7f10(5, prey, its
  id, `+0x6d3` (the prey's 0x50e600, unused), reach 1): below 0, state 1
  and `+0x6a1` 3; 0 nothing; above 0, the prey a ship at war, a hook
  (0x4b84b0; state 7).
- 0x4b7f10 (ship, size, prey, prey id, .., reach): the box of `size / 2`
  about its cell (or, under way into `+0x5b6` with `+0x596` set and move
  state 4 or 5, that cell), clipped to the map; the region's shorts
  0x499990(cell, `+0x592`) 0 open, -1 closed, and closed where a ship
  (0x50e600 200) other than itself lies, but on its own `+0x5ab` or the
  prey (pointer and id). The prey in the box: a building (0x50e600
  0x32) its cell by vt+0xb8, its reach zeroed for classes 0x39, 0x3a and
  (reach 1) 0x3b, and for classes 0x34-0x36, 0x3f-0x41 (bits 0xe01c)
  `+0x594 = 1` and the cell `+0x74 / +0x78`; a ship on its own cell's
  slot that cell, and its distance from the ship's own `+0x5a3` (the
  larger axis) if within `size / 2` on both; on the slot of the cell it
  is under way to (its `+0x5ab`) that cell kept apart, with its distance
  from the box's middle by the box's row and column (counted from the
  clipped corner), and the prey's own cell with its distance unbounded.
  Not in the box: 0xff. Within reach: `+0x6aa` = W x the box's end row +
  its end column (one past the end corner, as the disassembly at
  0x4b83bf has it), 1; 0x4b84b0 writes the prey's cell over it for a
  ship or building before reading it. Else a plan 0x4a6270 mode 6
  through the region to the cell apart if nearer, else the prey's own;
  with no distance for the prey's own, the cell apart; with neither,
  none; 0. Its search mode (`+0x6a1 == 2`) lists the box's occupants
  into 0x5c79c8 (at most 50), 0x4b7f00 (an empty function) and takes the
  first if a ship.
- The rammer (0x4c2230 states 1 and 2): `+0x6aa` = -1; 0x4b8d80(reach 1,
  box (x - 2, y - 2, 5, 5) about `+0x5a3`). 0: the prey found, its goal
  0x4c1f60 and a plan 0x4a6270 mode 6 to it through the region; no route
  (or not found), state 1, `+0x6a1` 3, the contact released (0x4c1950).
  1: 0x4c1f60 2 charges (state 1: `+0x6ae` its cell's reservation, state
  2, 0x4c1760; state 2: 0x4c1870 the bow on the hull, the impact); 1:
  `+0x6aa` the goal and the plan, kept if it planned and the route ends
  there (`+0x5b2`) or it is under way (4, 5); else state 1, release,
  `+0x6a1` 3.
- 0x4b8d80 (the rammer's, region in the caller's buffer): the box
  `param_4`, its corner clipped (none off the map), its far side `w`, `h`
  on from the clipped corner, clipped; cells 0x499990 as above; where a
  ship lies on its own cell (not its `+0x5ab`): closed if it lies still
  (`+0x58c` 0), is not itself or the prey, and the cell is not the
  rammer's `+0x5ab`; closed too, a corner of its own cell, when its
  heading `+0x5af` (signed) is below 1 and 0x499ca0 refuses the corner;
  the prey (a monster not of class 0x1fb, or another nation or slot at
  war) found: the region filled (goal its cell, active), and 1 when
  0x557480 to it is within `param_1` and its corner not refused.
- 0x4c1f60 (rammer, prey): from its cell (or `+0x5b6`'s, 0x4e0b50 0)
  the prey's cell and, under way (state 4 / 5, `+0x5ab` set), the cell
  it goes to: the goal the one nearer (0x557440), 0x4c13b0 choosing
  when both are at 1, the prey's own on a tie; 2 (alongside) at
  distance 0 or 1 when 0x4e0b50 gave 0, else at 0, or at 1 with
  0x4a68b0, facing it (0x59791d by the offset from its own cell =
  `+0x5af`; 2 off reads the bytes about the table: the vtable 0x597914
  before it, 0xff after) or in move state 3; also 2 lying still (state
  0) at 1 facing it; else 1; 0 for a prey that is no ship.
- 0x4a68b0: out of move states 4 and 5, 1; else the way stepped a tick
  (`+0x59b` by `+0x284` braking or `+0x280`, times 0x597b2c by the wind
  / 100, capped by `+0x59f` likewise), `(+0xe2 x way + +0x597) / 3000`,
  half of it on each axis straight on, a third slanting (none: 0); 1 if
  the picture so moved reaches or passes the route point (`+0x5c2`
  entry `+0x5be`) on either axis it moves along.
- 0x4c13b0 (in the disassembly; Ghidra's is mangled): for the two cells
  (prey's, the one it goes to; both 1 from the rammer's), direction `d`
  from its cell by 0x59791d; the row 0x599688 + (k + the prey's type x
  4) x 0x18, k by the prey's heading (0 / 4: 1 if the prey's row is not
  above the rammer's, 2 / 6: 2, slanting: 3), six ints (x, y, way for a
  straight `d`, then for a slanting one); the point the cell's middle +
  the unit of `d` times (x, y); time `t0` (the rammer's 0x557440 to its
  cell's middle, 0 in states 0, 2, 3) x 6000 (heading straight) or 9000
  over `+0x59b` (0 counts 1), then per cell `(way - 0x599204[rammer's
  type x 0x60] / 2)` likewise by `d` + `t0` (0 if the way is not above
  0); the prey's centre carried on its heading by `t x +0x59b / 3000`
  (32-bit) halved straight on or a third slanting; 1 (the cell it goes
  to) if the first point's 0x557440 to it is not below the second's.
The port (closing.rs, a game tick at a time, after the hunts) follows
these with `Ship::closing` (`+0x6a1`, `+0x6c7`, `+0x6aa`, the plan's
record), for ship prey only (the port's rams and boardings): a ship's
`+0x5ab` is the cell it is going into between cells in move state 4 or
5 (`Ship::under_way_into`), and a slot holds a ship in it or one that
has so reserved it. 0x4a68b0 and 0x4c13b0 run on the ship's way
`+0x59b`, `+0x597` and picture centre as the mover keeps them; 0x4a68b0
steps the way a tick as the move tick does (`sail::next_way`, the wind
always, no lime). The charge, the hook and what follows
are the port's grapple.rs; the bow is looked for from the tick after the
charge starts. Not ported: the 0x4a6150 wait counter's part (`+0x591`
and 150: the port's mover's own). The rammer's contact (0x4c2230 states
1 / 2, alongside, 0x4c1f60 giving 2): `+0x6aa` the
goal cell, `+0x6a0 = 1`; in state 1 `+0x6ae` = its own cell when that
cell's slot holds it, else 0x49a450 (the cell about its own whose slot
holds it, by the grid's offset list; none: stay in state 1 this tick),
state 2, then 0x4c1760. Contact made: 0x1401 to the prey, the move to
the goal. Refused: not between cells (`+0x596` not 1), it stays in state
2 (next tick 0x4c1760 again, refused there: state 1 and message 8 /
0x80); its picture at its cell's middle, state 1 and 8 / 0x80; else its
commands held once (0x4b37d0 with 0x2000, `+0x698`, `+0x25d |= 0x10`),
`+0x6e4` the heading opposite its own (0x4a34f0: h + 4 mod 8), state 6,
move state 0. State 6, once a frame (`+0x7d`, `+0x81` of the picture):
the picture a pixel along `+0x6e4` on each axis it moves, not past its
cell's middle (types with 0x597a24 also step the frame `+0x45` down one
on odd ticks, from below heading x 4 back to heading x 4 + 3: their
under-way frames run backwards); at the middle `+0x6ae`
released (0x4a3970) unless its own cell or `+0x5ab`, 0x13d to its player
(0x4a1880 with `+0x6cb`), commands back (0x4b3850), 0x4b8d10, `+0x696 =
1`, state 1, 8 / 0x90, the prey `+0x6cf` and `+0x6cb` cleared, `+0x704`
now. 0x4b8d10 frees `+0x6df` and zeroes `+0x696..+0x712` (so `+0x708`,
the board's time, too). 0x4c1760 grants the contact unless the prey's
`+0x8e6` or its own is set, the prey lacks `+0x261 & 0x1000` or it has
`+0x25c`; it also answers 1 for the contact already made between the
two (its side 2). State 3 (the impact, 0x4b37d0 there too) ends in
state 6 as well, `+0x6e4` its own heading (lying in its cell) or back
toward `+0x6ae`, `+0x59b = 0`, so every ram ends at state 6's end: 0x13d
and the task stopped. The port (closing.rs `ram_contact`, grapple.rs
`ram_granted`, `ram_done`): the state 2 wait is `Closing::contact_wait`,
8 / 0x80 the move's route cleared, state 6 `RamStage::Back` to the cell's
middle (a refused one naming no prey), and its end the notice, the
times and the task's end. Not ported: the reservation `+0x6ae` and its
release (the port keeps no slots between ticks), state 2's release with
`+0x6a2` below 0 (its setter not traced), the commands held by 0x4b37d0
during states 3 and 6 (the port lets orders through).

The search mode and other prey. 0x4aa840 sets `+0x6a1 = 2` (search:
0x4c3000, 0x4b5ca0, 0x4b7eb0) only when its record's second byte
(`+0x81d`, stored by the event 0x10 intake 0x4a9f2d) is 2, and every
sender of event 0x10 gives 1: the commands board 0x17 (`{2, 1, id, ..,
0xff}`), ram 0x18 (`{1, 1, ..}`) and load 0x1f sub 2 (`{3, 1, ..}`)
(0x4e2ba0), and the at-will closing in 0x4c0b30 (`{1 ram / 2 board, 1,
id, 0xff, 0x46 / 0x45}`, from 0x4bfc60 with `+0x684 & 6`); so the
search modes are never run (not ported, none to port). The prey of
those senders is always a ship: the player's board and ram need one
(0x4e3580, 0x4e3800: 0x50e600 200, the picker not of type 0 or 1, the
prey taking a contact, `+0x261 & 0x1000`; the board also 0x4e6700 for
0x17: `+0x261 & 0x10` and marines aboard, 0x4bc270(1) over items 0-2,
or a hero who fights, 0x4b4dd0 > 0; the pointer 0x4e45c0 asks the
board with `+0x261 & 0x10`, the ram with `& 8`; port
`Scene::may_close`, the capability bits off while built or held by
0x4b37d0), the computers' pickers list ships only (0x4baae0, 0x4bad90,
below), and the at-will list 0x5c7d39 (at most 50, 0x4c0bc0 from the
scan 0x4c0cd0; nothing while the picker has `+0x25d & 0x10`) takes
ships (200) within the reach of the picker's cell (0x557480) with
`+0x261 & 0x1000`, for a picker of side 1 or 2 (`+0x1a1d`) with a route
end `+0x5b2` on the map only those within 5 of it. Not decoded: whether a campaign brain's
command 0x17 / 0x18 ever names a building or a monster (the building
branches of 0x4b7f10, 0x4b84b0 and 0x4b5df0 state 7 for classes
0x34-0x36 and 0x39-0x3b, and 0x4c1f60's 0 for prey that is no ship,
are read above but not ported); 0x4b7370 (`+0x696` 3, the load) is not
ported.

**The at-will ram and board** (`+0x684 & 6`; port at_will.rs). In the
ship's tick (0x4ab930), after its behaviour (`+0x25d & 0x10`), every
fourth tick (`(clock +0xde ^ id +0x4c) & 3 == 0`) a ship that may fire
(0x4e6700 0x15: `+0x261 & 0x800`, armed 0x4bc270(7), not limed `+0x258`)
or, of type above 1, has its ram or board switch on and is type 2, 5
or 6 (0x597a30) or carries a hero who fights (0x4e66c0, 0x4b4dd0), with
`+0x25d & 0x100` or `+0x684`, afloat (`+0x251 == 0`) and under no
status (`+0x25c`), runs 0x4bf9b0: its tail, with a switch on, clears
`+0x710` / `+0x711` and marks the ram (`+0x710 = 1`, bit 0x10) when
now - `+0x704` > 50, setting `+0x704` = now + 100, and the board
(`+0x711`, 0x20) by `+0x708` likewise; any bit (a loaded position too)
calls 0x4c1240, whose fire states 1 and 4 run 0x4bfc60 and 2 0x4c0eb0
(the same box and listing, its cells' slots `+0x14`, `+0xc`, `+0x18`
only and the classes mask `+0x68d` (0x50ebb0) always; state 3's 0x4c09c0
reaches 0x4c0eb0 only after 0x4bf9b0 again, which clears the marks just
set (their times now + 100), so it lists nothing). In 0x4bfc60 with
guard on, an object is first matched against the positions' target ids
(`+0x640 + 0x12k`, `local_5`), and while not all have been met (7, or
0xf with four) it needs the classes mask too, the ram's and board's
listing skipped with the rest (0x4c0034). There, with
`+0x684 & 6`, the list 0x5c7d38 is emptied; the box is the range of the
highest loaded position (0x5980f4[level x 4 + k]: 9, 9, 9, 13 at every
level) or 4 (none, or 0), half of it about the cell, so never under the
reach 2; each cell's first held slot (`+0x14`, `+0xc`, `+0x18`, `+0x10`)
not itself, an enemy (side 2 but 0x1fb, or 0x4efe20 < 0), its player
bit in `+0x689`, there (vt+0x2c) and not a ship on its `+0x5ab` goes,
with `+0x710` or `+0x711` set, to 0x4c0bc0 (reach 2): nothing while
`+0x25d & 0x10`; a ship (0x50e600 200) within 2 (0x557480) taking a
contact (`+0x261 & 0x1000`) is listed (0x5c7d39 pointer, 0x5c7e01 cell,
at most 50), a picker of side 1 or 2 (`game +0x1a1d`) with `0 < +0x5b2
< W x H` taking only those within 5 of that cell (`+0x5b2` the route's
end, 0x4a6330; its own cell after a stop 0x4a38f0 or a route cleared
0x4a5b70). Then with a list 0x4e8db0(0x32, `+0x684`) picks: boarding
odds `+0x53c` x 100 / the prey's (200 at most, 200 against 0, 0 with 0
of its own; 0 against a phantom `+0x253` and for a rammer at a fire
rammer, types 5 and 10), the first greatest or an equal nearer
(0x557440, from 10000); ramming odds (disassembly 0x4e8ed2-0x4e9075, in
32-bit integers that wrap, on the picker's way `+0x59b` for both sides,
`b` the angle class 0x4c19b0: 1 same heading, 2 opposite, 0 else): `Q x
(0x598078[b] x H x way / 100) x (P / P') / 2500` against `Q' x
(0x598084[b] x H' x way / 100) x P' / P / 2500` (Q 0x598054 by ram kind
x 3 + the player's level, H 0x5c76b0, P 0x598024; the power ratio
divided first), `x 100 /`, at most 200, 200 when the prey's is 0 (so
lying still) or types 5 and 10; the first strictly greatest. The board
counts only with `+0x684 & 4` and `+0x711`, the ram with `& 2` and
`+0x710`; the ram if its odds pass the boarding's, else the board,
needing 0x66; the list becomes that one (count 1), the result 2 or 4
(else -1). 0x4c0b30 (count 1): the ram sends itself `{1, 1, id, 0xff,
0x46}` (vt+4 event 0x10) and sets `+0x708` = now, the board `{2, 1, id,
0xff, 0x45}` and `+0x704` = now. The intake 0x4a9f2d stores bytes 0-6
(`+0x81c..+0x822`, the 0x46 / 0x45 dropped) and `+0x7d6 |= 0x10`; the
order 0x4aa840 runs at the ship's next record pass (0x4ab8c9). The
constructor zeroes `+0x696..+0x712` (0x4dc554). The port runs it a game
tick at a time after the closing in, the order through the outbox
(`Order::Close`, delivered next tick); the box's cells are those within
2, the slots `+0x14` (`Scene::flyer_slot_held`) then `+0xc`
(`slot_holder`); a route end is the route's last cell, else its own.
Not ported: the gate's early returns `+0x257` (not decoded: no writer of
a value but 0x4c133a's 0) and 0x4c3ca0 (`+0x25d & 2`: 0x4c3410 returns 1
on the tick the boarded side's contact ends); `+0x25c` is the siren's
song alone in the port.

**Prey notices** (0x4a1800 0x13a, 0x4a1880 0x13d, 0x4a1900 0x13c, the
loader's 0x4a1980 0x13f at its state 6's end (Load panel), after
0x4a1700's 0x136 and 0x4a1770's 0x137): to the ship's group (0x49c020
with its player and group id `+0x5c`) when it has one with a notice list
(`+0x2d`), the record `{code, tick, ship +0x60, ship id, a, b}`:
- 0x13c `{prey, 0x13c}`: the boarder sailing, its prey gone (0x4b5d10,
  above); the boarder's state 6 done (0x4b6b87, its way back after the
  fight or after a call-off through state 8), with 0x4b3850, `+0x25d &=
  ~0x10` and vt+4(8, 0x90).
- 0x13d `{prey, 0x13d}`: the rammer sailing, its prey gone or not at war
  within 2 (0x4c2ec0, above); its state 6 turn done (0x4c2b46: `+0x696`
  1, `+0x6cb` 0).
- 0x13a: the capture (0x4b6f20 at 0x4b7105, neither side player 16) by
  the taker, `{prize, 0x17}`; a ship's attack whose target is gone
  (0x4c09c0: 0x4b9140 finds no object; vt+4(8, 0x180)), `{target, 0x16}`.
Read by: the human's attack block (0x4d4e50: 0x13a, below; 0x13c and
0x13d dropped); the computers' attack zone (0x4d6960: 0x13a from a ship
in the group, the target looked for again (0x4d6640) when the record
names the block's `+0x1c`, then the ship sent at it (0x4d0140, 7) if
there is one, counted heard either way; others dropped); the fleet
attack task (0x4d0210: 0x13a-0x13d, a ship in the group: 0x4cfa70(the
block, the record's prey, 1) below 0 or 0x4cfad0 sends it again
(0x4cfe40), else the block ends, 0x4a1c60); the guard (0x4d1ea0: 0x13a,
0x13c, 0x13d, 0x4d1750 of the ship `>> 16` 2 zeroes that entry's `+8`
and `+0xc` in `+0x48`). The gather zone (0x4c9e30) and the move
(0x4cc6a0) read none. Ported (`Scene::post_notice`: gather zones, human
attack groups and the computers' flotillas with a block,
`Computer::post_notice`): every sender (the rammer's state 6 as the
port's `RamStage::Back` ends, `ram_done`), the attack's only for a
target gone from the map (the port also ends an attack on a target no
longer an enemy, posting nothing); the human block's and the computers'
attack zone's cases. The boarder's end posts as the port's leave stage
ends, the task with it (event 8 0x90, `+0x708` now). The boarder
closing in, a prey within reach but not at war: nothing (0x4b5df0 state
1); the rammer's look finds the prey only at war (0x4b8d80). Not ported:
0x4d0210's and 0x4d1ea0's cases, the port having neither block as the
original keeps it: the fleet attack task (0x74, the human's commands
0x15-0x18 through 0x49dda0 -> 0x4d04a0; no other sender) is per ship in
the port, with no group or target list, and the computers' guard is
their zone (`order_zone`), without the block's lists `+0x44`, `+0x48`,
`+0x4c`. What they do: 0x4d0210's 0x13a-0x13d, the ship in the group:
0x4cfa70(group, the record's `+0x10` (prey), 1) finds the entry of the
block's target list `+0x30` whose `+4` is the prey and removes it (-1
none); -1, or 0x4cfad0 true, sends the ship again (0x4cfe40), counted
heard; else 0x4a1c60 ends the block. 0x4cfad0 walks the list's count:
with the list empty of data the index is removed (count less 1); else,
the block's target `+0xc` found (0x51f1f0), `+0x10` = its cell
(0x50ebf0, x + y x width), and from the list's first entry (always
element 0, 0x4cfb0d) `+0xc` = its `+4`, `+0x14` = its `+0xc`, `+0x18` 7,
`+0x1c` = its `+0x10`; true if so. 0x4d1ea0's 0x13a, 0x13c, 0x13d (each
heard notice counted): 0x4d1750(the record's ship id, mode 0) is the
last entry with that `+4` of the lists `+0x44` (1), `+0x48` (2), `+0x4c`
(4), the kind `<< 16` with the index (a later list's overriding; mode 1,
the 0x137 case's, removes the entries, not read further); kind 2 zeroes that `+0x48`
entry's `+8` and `+0xc`.
The two switches whole (records `{code, tick, ship +0x60, ship id,
rec[4..]}`, newest first, each removed after its case; every other code
dropped). 0x4d0210 (the queue walked while `+0x2d` is not null, the
block possibly ended under it): 300, the ship in the group (0x4cc590):
0x4cfad0 true sends it (0x4cfe40), then `#` sub 1 with the group's
three (0x4c42f0); 0x136 only with a route `+0x61` (as the founding's,
0x4cea20 / 0x4ce7c0; at the end 0x4d0030, else 0x4cfdf0 and 0x4cfc90,
or 0x4d0090); 0x13a-0x13d as above (not read: who posts 0x13b). 0x4d1ea0 (a record whose ship is not in the group dropped
first, but for 0x137): 300: with block `+0x24` 1 or 2, or 3 with
`+0x38` or `+0x3c` 0, 0x4d1a50(ship, 0); with 3 and both set,
0x4d1080(`+0x30`, ship); `+0x24` 0 nothing; then `#` sub 1 (0x4c42f0);
0x136: 0x4d1750 mode 0 of kind bit 4 and 0x4d2200 true: `+0x29`
steps 0-3 round, 0x4d2160 its corner of `+0xc` and 0x4d22f0 there;
0x137 (rec[3]): 0x4d1750 mode 1 of kind 4 exactly: 0x4d25a0 with
`+0xc` and 0x4d2110; 0x13a, 0x13c, 0x13d as above (rec[3] the ship).

The move states (+0x58c, dispatched by the move tick 0x4a3f40): 0 lying
still (0x4a3150: the hull rocks, the velocity halves each frame), 1 told to
stop (0x4a38f0, then 4), 2 turning on the spot (0x4a3320), 3 at a cell
choosing the next (0x4a3560: the step checked by 0x4a3a70; refused, it
waits; free, under way as 5, or turning as 2 for more than 45 degrees), 4
slowing to a stop (0x4a2f60), 5 under way between cells, 6 held by a
boarding (the boarded ship once hooked, 0x4c3410; a boarder lying by a
fort it boards, 0x4b5df0 / 0x4b73a0; task step 0x11 with a contact,
0x4adef0). A rammer from its contact on and a boarder at its hook cell go
to 0 (0x4c2425, 0x4c29aa, 0x4a3560: their contact `+0x6a9` = 6). The
port keeps the state at its moves' events: 2 while the hull turns, 5 once
the next cell is reserved, 4 as it brakes (0x4a2f60: entering the route's last cell or refused the cell after the one it enters), 3 at a route
point and while a refused step holds it (0 after 50 ticks), 0 with no
route or at its end. A stop (0x4a6330 with the stop flag; nothing on the
tick it already re-planned) lays a ship turning or at a cell (2, 3) still
(0) at once; any other state but 0 becomes 1. In state 1 the move tick
runs 0x4a38f0 first: once the ship has entered the cell it is going
into (`+0x596 == 0`; `+0x596` is 1 from a route point until the hull
picture's centre crosses into the next cell) it brakes there (0x4a2f60,
4; the route's end cut to that cell, a reservation of another cell
released); still on its way out (`+0x596 == 1`) it sails on, and at the
crossing (the cell taken, the route's end or a refused next cell) brakes
in that cell, or on the next tick by 0x4a38f0. So a stop under way ends
in the middle of the cell the ship is going into: the port keeps the
path's first point and 1 till the ship is in its cell, then 4, then 0.
No dock code writes `+0x58c` (none of the 35 stores is in 0x4b1d40-
0x4b2700): a ship mended in a dock keeps its state; it is off its cell's
ship slot (0x4a3970 on entering), so the sea monster's look at the cell
passes it over (the port's `in_dock` test; not settled: whether every
dock entry does). State 6 is written on every tick of 0x4adef0 step 0x11
(`+0x77c >= 0` and a contact `+0x8e6`, then step 0x4add10): the
unloading order 0xc (event 0x40, 0x4aab00; the target, 0x4ac050, a port
of class 0x39, for player 16 a class 0x40), lying at the port's berth
with the anchor down while the porters carry the goods in (the port's
`GatherPhase::Unloading` at a port); leaving (step 0xe, anchor up, then
0x13: the contact released, 0x4a5b70 clears the route, state 0) it gets
event 8 0x40 and moves a cell off the berth, 0x4b46f0 with '[' = 91 (see
"Shipyards' berths"). Waiting by a taken port berth 300 ticks it steps
aside itself (0x4b4b50 with 0x5a) and gives up after 349. A ship can be
sold (message 0x248f, task 5) only in state 6 with a building contact
(its settlement's Sell Item click, see **Selling**). The wait by a crowded
port berth is in "Gather zones".

**Where cargo comes ashore.** 0x4ac2d0 routes the gathering task by its
record class `+0x76e` and sub-code `+0x772` (jump tables 0x4ac594 /
0x4ac5a4): codes 9 (0x4ad680), 10 and 11 (amber: 0x4b0590, the
collecting machine, which never unloads: full or done it posts notice
0x138 to its group, 0x4a1430, and ends its task with event 8 0xc0; fish
0x4ae760 / 0x4aeb80; leviathan 0x4aeef0 / 0x4af640), and 0xc (0x4adef0,
the only unloading). Code 0xc is entered only by event 0x40 code 0xc in
0x4aab00: `p = 0x4ac050(ship's player, +0x83e)` from that player's
building list; dropped without one; player 16 needs class 0x40, any
other a class 0x39 with vt+0x2c true (0x40dfc0: not asleep, `+0xa4 !=
0x10`); else dropped, the old task kept. Its senders: the player's
unload order (message 0x1f kind 1, 0x4d74a0 / 0x4e2ba0: cargo aboard ->
event 0x40 {400, 0xc, id}), the leviathan's 0x4ad510 (its own port
`+0x7ad`, or 0x4c5910) and 0x4adef0 step 1 when its port is gone
(0x4c5910). 0x4c5910(ship, &dist, mode) walks the ship's player's
building list (`[0x5c7508] + player x 0xba0 + 3000`): player 16's class
0x40 (cell vt+0x28, 0x40dfa0: the anchor), others' class 0x39 with
vt+0x2c (mode 1: class 0x3b; berth cell vt+0xb8, 0x40e050: the anchor a
step by the facing `+0x1dc`), the least `W[ship cell] - W[cell]` (strict
`<`: the first in the list on a tie; 100000 and none without a
candidate) of the wavefront 0x4a1e90(draught `+0x592`, ship cell, 5000).
0x4a1e90 copies the ships' grid by draught (0x5c93bc shallow, 0x5c9398
deep: 0 open, -1 closed) and floods it by 0x545e20 -> 0x5459d0(grid, W,
H, x, y, 5000, step 3): the start gets `(5000 + 1) x 3 x 2` = 30006, then
up to 5001 layers, each cell of a layer in the order it was reached:
its neighbours east, north, west, south still 0 take its value less 6
while that stays above 0, then north-east, north-west, south-west,
south-east its value less 9 (corners may be cut); a cell keeps the value
it was first given. A berth the flood never reached counts 30006, a
closed one 30007. So **only a port of the ship's own player, not
asleep, takes cargo** (player 16: its class 0x40); settlements and towns
never do. The port follows this (`unloading_site` with
`NavGrid::wavefront`, `order_unload`; the list in object order).

The gathering notices go to the ship's group (0x4a1430: `{0x138, tick,
ship +0x60, ship id, class}`; 0x4a1520 after unloading: `{0x139, ...,
0}`, both appended to the group's list `+0x2d`, 0x49c020(player,
group)). Of the navy's fleet tasks only the gather zone's queue handler
0x4c9e30 (newest first; jump table 0x4ca240/0x4ca270) has cases for
them (0x4cba10, 0x4cdc00, 0x4cf0b0, 0x4d6960, 0x4d7770, 0x4d8030,
0x4d8b90, 0x4d0210, 0x4d1ea0, 0x4d4e50 and 0x4da0a0 have none): 0x138
-> the ship (0x4cc590, class 200) to 0x4c9ae0: if 0x4ad280(ship, class)
and not (a leviathan with `+0x480`) and 0x4c9930 sends it to another
deposit of the zone, done; else its own port `+0x7ad` if the zone's port
list (`+0x25 +0x28`) takes it (0x4c64b0 >= 0, 0x4ac050 finds it), else
the zone's port pick 0x4c6640 (none: nothing sent); then command 0x1f
`{1, port, 0x5a}` by 0x4c89b0 (0x520cc0), or with the zone's `+0x61`
0x4cab20 and 0x4c9c60. 0x139 -> 0x4c9930(ship, 0, `+0x61` set): the
zone's next deposit for it. So the human's and the computers' gather
zones send a laden amber or fish ship to unload, a tick later at the
earliest (the notice is read at the zone's turn, the command delivered
at the next tick). The zone's port list (block `+0x28`) is filled when
empty by 0x4c6c30 (the map's cells in order, the player's ports not
asleep by the first cell found; player 16 its class-0x40 structures) and
pruned at each pick by 0x4c65c0 (last to first: not a port, so player
16's go and its ships are never sent; or vt+0xb4 0x40e020 false: facing
`+0x1dc < 0`, state `+0xa4 != 1` (0 built, 2 mended, 0x10 asleep), or
`+0x1d8 & 0x10` with `+0x1c8 == 0`). **0x4c6640(group, ship)**: no port,
0; one, it (no reach test); more: the wavefront 0x4a1e90 from the ship;
`min` = its value at the first port's berth; each port whose berth
(0x4c6520: a class 0x39 of the player at the entry's cell, vt+0xb8) has
a value `w >= 1` lowers `min` and is inserted, score `w + 1`, before the
first ranked one whose score is below `w` (at most 9 kept); each score
then `s - min` (or 1); `k` = the ranked with `s * 100 / (s0 + 1) >= 70`;
of the first `k` the least load (0x4c7ea0: the player's ships of type
0/1 in fleets running 0x78/0x3d with `+0x7ad` the port; the comparison
starts from the first's and then reads the loads by rank position in the
unranked array), the first on a tie; if its berth is crowded (0x49a330:
`a > 0`, `b > 0`, `a - b < 2`) the next ranked with a value and a lower
load. None ranked: the list's first. `+0x7ad` is written by 0x4aab00 (an
unload command's port; a gather command's port, 0 on the paths the port
has) and zeroed by 0x4adef0 after its 350-tick wait. The rest of the
zone is in "Gather zones" below. Whether the unload pointer is
offered over a town (0x5175b0 / 0x5180e0) is not read either. The
prey of a ram keeps its move state: the ram (0x4c2230) sends it event 2
with code 1 (`+0x811` 0x14 when the rammer is alongside, 3 at the blow,
0x4c2544-0x4c2563), never code 3; code 1 (0x4aa2c0: `+0x713 = 1`, `+0x71e
= +0x811`, `+0x25d |= 2`) runs 0x4c3ec0 from the contact machine 0x4c3ca0,
which stores no `+0x58c` (nor do 0x4abc40, 0x4b3850, 0x4e06a0, 0x4c3320 it
calls); by then the prey's contact is the rammer (type 1, set by 0x4c1760
as the ram reached state 2). State 6 from contact is 0x4c3410 case 9 (at
0x4c35c6, `+0x713 == 2`), the ship a hook holds. The tick
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
0x4cf070); laden, goes to unload (0x4c6640, priority 90); an idle one
would be sent message `*` (0x4d8b30: 1, 1, 0 and the id of a ship of
the zone picked at random), but that list is filled only on a path that
needs the gathering bit both set and clear (0x4ca73b), so never. Message `*` reaches the ship as
event 0x280f (0x5228c0, 0x4dc630, 0x4e2ba0) and goes in by intake 0x80
(+0x858); 0x4ab250 replaces its movement with task 1 (+0x54c): keep within
A cells (+0x556, payload bytes 1-2) of object B (+0x55a, bytes 5-8),
priority 0 (byte 9), patience 5, re-plan every 25 ticks. Each tick
(0x4a5f30) its goal is the ward's cell (a building's own); within A
(`max + min / 2`) it stops but keeps the task. It ends only when the ward
is gone (0xc0), it gives up stuck (150) or another order replaces it; a
new `*` retargets. The navy uses A = 1 (zones, 0x4ca290, 0x4d8920, ...)
and 2 (escorts, 0x4c6270, 0x4c6ee0, 0x4c9270). When none is laden,
listed or under way, a human's zone is marked done (+0x5d). (That
branch's follow list is never filled, see "Gather zones": the zone's idle
ships stay idle.)

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

## Gather zones

A gather zone is a group's task (fleet order 0x78 / 0x3d, 0x4c8a80; the
block `+0x25`, 0x65 bytes). Port: `scene/gather.rs` (`Scene.gather_zones`,
saved; each zone keeps its group's ships, notices, known deposits, port
list and escorts).

- **Made by.** The human's click on a deposit with ships selected (codes
  0x19 fish, 0x1a amber, 0x1c / 0x1d leviathan, 0x49e230 -> 0x49d3c0): the
  selection made a group (0x49ca60), a zone of the 3 x 3 cells about the
  deposit's cell (amber `+0x90/+0x94`, fish `+0x9d/+0xa1`, leviathan
  `+0xa9/+0xad`), clipped to the map, its class, flags `+0x24 = 0x100`,
  args[9] -1, no port list. The click's key flags (Sinput,
  0x55345d-0x5534ef, the mouse message's `+0x14`: 8 Ctrl, 4 Shift, 2 the
  right button held, 1 the left; the map click is taken at the release,
  messages 0x62 / 0x64, so its own button is not in it) go whole to
  0x4c8a80, where any sets group `+0x10` (the first deposit only). Shift
  (4) on the selection's group (0x49e230 keeps it while the selection is
  unchanged) running a zone: of the code's class (400 for amber, 0x191
  for the others, so never a leviathan zone) the deposit is only known
  (0x4c6e30; the local player's interface gets message 0xc45d, not
  ported), no ship sent; of another class nothing; else a 1 x 1 zone. The
  voice plays first either way. The zone buttons (0x23 sub 0x10-0x12,
  0x49d790): flags 0x10000, class 400 / 0x191 / 0x192, the rectangle the
  drag gives (0x51cdd0 -> 0x517c00, key flags 0): its top-left cell and
  its size, cells of pixels / 64 toward zero, not clipped to the map. A
  brain's 0x47a570 -> 0x479970: the area, class, flags its argument 11
  (0 in every mission), args[9] the member p10's id, no port list or
  mask. A computer player's flotilla (0x483090, only at the first turn of
  task 4 state 5 and task 5 states 7 and 10; key flags 0): its area
  `+0x158..+0x164`, class `+0x70`, flags `+0x154` (0 here: only tasks
  7-11 and 13 write it), args[9] the manager flotilla's group (never
  read), its base town's ports (0x43d220 with 0x39: none for a town of
  `+0xa4` 2, 3 or 0xff; 0x440c60: the class-0x39 buildings of its list
  `+0x1d0` in order; none: the list null for 0x4c6c30), and as args[10] /
  [11] up to 4 deposits of the class 0x40b010 ranks in the area (table
  0x594520), which 0x4c8560 makes known before the area's, ordered for
  each port by its flood (0x4c7800) (the town states 3 and 0xff, not
  identified, the port tests mending).
- **The ranking** (0x40b010 -> 0x40a4a0(town, player, x, y, w, h,
  0x594520, class, 1, 1, &n, &list, 1)): the town looked up (0x51f1f0;
  gone: its error path, none); its ports (0x43d220, 0x440c60), each
  flooded from its berth (vtable +0xb8, no water test) over the shallow
  ships' grid 0x5c93bc (0x558fa0, limit w + h of the map). The area's
  weights: 0x40b050 collects over its cells, 0x40bbf0 paints over it, by
  0x594520 (own buildings 0 (0, 5, 1); others' towns, forts, buildings
  (-10, 10, 0), ships (-5, 5, 0); amber, fish, leviathans 0; lairs -30;
  nests and castles (-20, 0, -1); medusas and sirens (-20, 5, -1); lava
  -1; a nest's reach paints only right of the area's left column). A
  record (0x20 bytes) for each cell whose deposit slot (cell +0x10) holds
  the class. Windows of 1 x 1 over the area but its last row and column
  (the loops run to `x + w - 1`, `y + h - 1`), row by row: a record is
  rated once, for each port the way back down its flood (0x559470:
  from a cell below 1 its least positive neighbour, 0x5c4f4c order, ties
  on even directions, a neighbour of 1 ending the look; then each step
  the slanting directions at value + 1, a tie in the last step's way, then
  the straight ones, a tie taken unless the best is the last way or that
  way slants; a neighbour of 1 ends on it; a start of 1 given twice),
  its cells' weights summed (-50000 if one is outside the area): the
  first port with a way, then a greater sum, or equal and fewer cells.
  Each window with a record gives `{1, x, y, 1, 1, cells, sum, port
  {id, x, y}}` (no way: cells and sum 0); sorted by 0x40a440 (VC6 qsort:
  cells 0 last, then greater sum, fewer cells, more deposits). 0x483090
  keeps of the first 4 those whose cell's deposit is still of the class,
  `{id, x, y}`. Not decoded: a town with no ports (0x42d9c0 with `{x, y,
  0x1e}` places one, the flood from beside it; the port gives none).
  Checked against the original's code under an emulator (0x40a4a0 on
  1100 synthetic scenes; 0x559470 on 3600 floods).
- **0x4c8560** (list, n; none for no list or n outside 0-100): with 1-99
  ports in the zone's list, each port's copy ordered by 0x4c7800 and the
  first as many as it returns known (0x4c6e30), port by port; else each
  entry whose cell's deposit is still it. **0x4c7800**(zone, list, n,
  port cell): the cell's building a port (0x39) with its berth water
  (0x4999e0), flooded (0x558fa0, 5000); else 0. An amber zone walks the
  entries: one still that amber with an approach (0x4c76f0) takes the
  approach as its cell and counts; another has the later entries moved
  down one (not on the last), the walk going on at the next place (so
  the entry moved in is skipped). Other classes count all. The first
  count entries bubble sorted, the greater flood value first.
  The block's `+0x61` is no route of the order's: it is a group's
  waypoint route (see **Route** below). Any other order to the group ends
  the zone (0x4a1c60); a group given 0x78 again (the selection unchanged:
  0x49e230 keeps its group, so a gather click or zone drag on a zone
  group's own ships restarts that zone) has its lists cleared (0x4c7650:
  deposits, port list, escorts, `+0x35`, `+0x3d`; its notices and its
  route `+0x61` kept) and its block rewritten. On a group whose block is
  another order: without key flag 4 that block's route is freed
  (0x4ce5a0); with it the route is moved to the new block and the start's
  dispatch is skipped (`local_8`). The port's human groups have no other
  blocks (their waypoint moves are per ship), so that case never arises.
- **Start** (0x4c8a80 tail): 0x4c5b90 mode 0 lists the rectangle's
  deposits of the class and knows them (`+0x39`); if any: 0x4c5f40 splits
  the group into merchants (types 0/1, with their room 0x4be800(3),
  sorted most room first by an exchange sort: for i, for j < i, swap when
  `room[j] < room[i]`) and others; 0x4c6ee0: the port list filled if null
  (0x4c6c30); under 6 merchants each to 0x4c8f20 with port argument 0; 6
  or more, ship k to deposit k mod n, except a computer's (side
  `+0x1a1d` 3 / 4) under half its room or with a leviathan aboard, sent
  to 0x4c6640's port if any; each other ship follows merchant k mod n
  (message `*` 0x4d8b30 with 2, priority 0; 0x4c6100 lists it as an
  escort `+0x41` and sends 0x4c42f0 with -1s, not ported). 0x4c6ee0's
  route argument is the block's `+0x61` being set: under 6 merchants it
  goes to 0x4c8f20; 6 or more, the gathers are only held back (no
  0x4c9c60), the computer's unloads go with route 0.
- **Notices** (0x4ca8d0 -> 0x4c9e30, newest first, each removed after its
  case; records `{code, tick, ship +0x60, ship id, rec[4..]}` from the
  posters 0x4a1240-0x4a1bf0). The ship's cases, a notice of a ship no
  longer in the group dropped: 300 (a ship joined, 0x4a1340, after
  0x49bd60) and 0x139 (unloaded, 0x4a1520) -> 0x4c9930; 0x138 (gathering
  ended, or a berth given up) -> 0x4c9ae0; 0x136 (a ship at its move
  goal, 0x4a3f40 at 0x4a49af / 0x4a4a67, or 0x4ce8a0) acts only with a
  route. 300 and 0x139 pass 0x4c9930 the route flag `local_c` (the
  route's being set as the read began); 300 with a held command for the
  ship fires it instead (0x4cabd0, 0x4cac30). The others whoever the ship: 0x133 (0x4a1620: a ship wrecked by
  a reef or by lightning, 0x4dc630 on 0xda94; the whirlpool's 0xda95
  posts nothing) appends its cell to `+0x35`, duplicates kept; 0x137
  (0x4a1770: a ship leaving a group with a block, 0x49bef0, before the
  removal) removes its escort entry (0x4c61d0); 0x145 (0x4a12d0 from the
  port's vt+0x94 0x42ff90 when it first stands, to every zone of its
  player; rec[5] its cell): the port still the building of that cell is
  listed (0x4c6400, the list made if null), then 0x4c7b80: for a human's
  zone (side 1 or 2), or when another listed port of class 0x39 within 10
  cells (0x557440) has more than 2 merchants bound, each merchant
  unloading (sub-code 0xc) is sent at once to 0x4c6640's port (0x1f).
  The value the last case wrote, when non-zero, skips the re-plan. Not
  ported: 0x12f (0x4a13b0 at each take, 0x4ad0e0: `+0x30` += the amount
  for the zone's class; nothing reads `+0x30`); 0x13e (the human's unload
  click on an own port with Shift on a zone group, 0x49dfb0, toggles the
  port in `+0x28` and posts it; its case 0x4c6ad0 only drops home ports
  gone from the list and never sends: its send branch needs sub-code 0xc,
  which its entry test excludes); 0x137 for ships leaving a brain's or
  computer's fleet (the port's fleets are not groups). No code posts
  0x146, 0x147 or 0x148 (their cases are dead).
- **0x4c9930**: 0x4c96e0 (the same deposit), else the deposits not known
  yet (0x4c5b90 mode 1), else the known ones still to be had (0x4c95f0:
  amber state below 2, fish below 4, a leviathan below 5; the original's
  walk skips the one after each it forgets) to 0x4c8f20 with port
  argument 1 (+1 only when that returns non-zero). **0x4c96e0**: no room
  and a port (0x4c6640) -> 0x1f; amber: the amber in `+0x797`'s deposit
  slot (any state), else 0x4ac610(box `+0x7c2..+0x7ce`, mode 1); fish: one
  navy draw `% 1000` for 0x4ac610 mode 2 (0x49a270, the shoal last fished
  found in an object slot about the ship, never holds: shoals are in the
  deposit slot); leviathan mode 1; the deposit known, gather command port
  0. The box is the deposit's cell -/+ 2 clipped to the map's last cell,
  scanned `[x0, x1)`, so the far column and row are left out.
- **0x4c8f20** (2 or more candidates, a merchant, not a leviathan zone;
  else 0x4c9270): 0x4c7fc0 rates; weights `pscore + amount`; three navy
  draws `% total`, each the first whose running sum passes it, the
  heaviest kept (strict); no room -> 0x1f to its port; else the gather
  command with its port (port argument 1) or 0. It returns 0 always on
  this path (0x4c9257), so 0x4c9ae0 then sends 0x1f too.
- **0x4c7fc0**: the zone's ports (filled / pruned; over 100 gives 0); each
  of class 0x39 at its cell whose berth is water (0x4999e0) flooded from
  the berth over a copy of the shallow grid (0x558fa0 -> 0x557910, limit
  5000) with its load L (0x4c7ea0). A candidate still in its cell's
  deposit slot: one port, it, score 1; more: per port with a load, an
  amber's cell becomes its approach (0x4c76f0: east, west, south, north
  water neighbours with a value above 0, the least; north returns at
  once; none: port and score 0 and the next port), `s = (B[cell] -
  B[berth]) x 10 + 20L (L < 3) / 200L (3, 4) / 100L^2`, least and most
  over all candidates and ports (set at the first candidate's first
  port); `T = (most + least) x m - sum > 0`: three draws `% T` walking
  the ports with `most - s + least` (0 for `s < 1`) until past the draw
  (none: port 0), the greatest value kept (strict from 0). The ship
  counts 0x4c7a60 it also stores are never read. Its four arrays (loads,
  port objects, berths, scores) come off the stack (0x5878f0, the stack
  probe) and only the loads are set (-1): a port's object is written
  when it passes the test, its `s` when rated. The last entry of each
  holds the probe's return address left there: the last port's object
  0x4c8068, read as an object its id is the dword at 0x4c80b4,
  0xffff6c85 (no object; so a lone port that fails gives its deposits
  that port, score 1), and the last `s` 0x4c8090. The other entries hold
  what 0x4c65c0 / 0x4c6c30 left (frame, `ecx`): not known; the port
  gives the rating up (0, nothing sent) when the walk or the pick reads
  one. The least starts at 0 and is set only at the first deposit's
  first port, so with that port unrated one deposit alone has `T = 0`.
- **0x4c9270**: the candidate with the least value of the wavefront
  0x4a1e90 from the ship (limit 5000; an amber's approach 0x4c76f0 on it;
  first on a tie, 0x4c5ec0; an amber with no approach makes it read the
  word before the wavefront, the end of the malloc heap header of
  0x4a1e90's block, not the game's: with one candidate it is never
  compared; with more the pick is not known and the port takes
  0x4c9270's failure exit, nothing sent, 0). Amber / fish: a non-merchant follows a
  merchant drawn with the navy LCG; a merchant with port argument and a
  0x4c6640 port: full -> 0x1f, else the gather command with it; without:
  full -> 0x4c6640 with no ship (the port with the most ships bound,
  first on a tie) or nothing; else the gather command, port 0. A
  leviathan: no port -> gather port 0; full or a leviathan aboard -> 0x1f;
  else gather with the port.
- **0x4c6640** as above, with no ship the most-bound port; a berth is
  0x4c6520's (the player's class 0x39 at the entry's cell, else -1), the
  least starts at the list's first berth's value: with that -1 (a port
  lost to another player: 0x4c65c0 does not test the owner) it reads the
  word before the wavefront, not known, and with any port reached the
  port gives no port (the pick's empty exit); the crowded
  step: 0x49a330 on the picked berth, `a > 0 && b > 0 && a - b < 2` ->
  the next ranked port with a score and a lower load. **0x49a330(berth,
  player)**: the berth's neighbours on the map (0x49a1d0 gives the cell's
  kind 1-9: corners 1 (0, 0), 2 (W-1, 0), 3 (W-1, H-1), 4 (0, H-1), edges
  5 top, 6 right, 7 bottom, 8 left, 9 inside; counts at 0x597900 = 3, 3,
  3, 3, 5, 5, 5, 5, 8 copied to the map object `+0x18`, offsets filled by
  0x499ff0 at `+0x3c + 0x20 (kind - 1)`: exactly the in-map eight
  neighbours); `a` those passing 0x4999e0 (landscape low nibble below 8,
  nibble 1 not 4, 5, 6), `b` those whose `+0xc` slot holds a ship of the
  player of type below 2 with `+0x25d & 0x40` (a gathering or unloading
  task, set by 0x4aab00, cleared by event 8), sub-code `+0x772` 0xc and
  step `+0x77d` 1; crowded when `a - b < 2`.
- **The berth wait** (0x4adef0 step 1, the ship not on its berth
  `+0x7bd`): with `+0x78e` 0 the timer starts at a 0x557440 distance of
  exactly 1; then at 300 ticks a crowded berth sends it aside (0x4b4b50
  with 0x5a), from 350 a crowded berth makes it give its port up
  (`+0x7ad = 0`, notice 0x138 class 0), and the timer is 0 again either
  way (301-349 nothing). An unload command zeroes it (0x4aab00). Port:
  `run_berth_waits` at the navy's tick.
- **Re-plan 0x4ca290** (`group +8 > 0x55`, with ships; `+8 = 0`, so every
  86 calls; `+8` grows each call): deposits not known or still to be had
  (0x4c5b90 mode 1): each ship without `+0x25d & 0x40`: a merchant
  0x4c96e0, else 0x4c8f20 with port argument 1; another ship found among
  the escorts (0x4c61d0) to 0x4c6270, which returns 0 or 1 only (so such
  ships never reach 0x4c8f20): the escort not among the group's ships
  (0x4a0370): its entry goes; a merchant: nothing; the ward not among
  them: nothing, the entry kept; state 0xb and more than 2 cells
  (0x557440) from the ward: state 10, target 0, follow it (0x4d8b30 with
  2); else the ward's blow marked (`+0x691`): state 0xb, target its
  giver `+0x692`, 0x4d0140 (command 0x16 `{target, 7, 7, -1}`: the attack
  on a ship, priority 0); else moving (`+0x58c`): nothing; else state 10
  and follow. Escort entries are 0xd bytes `{escort, ward, target,
  state}`. Before that, each ship with a blow is reported (0x4a21c0:
  message 0x5d9c to the brain, its header `[0]`, `[1]` unwritten stack;
  the port reports blows as they land instead) and sent command 0x28 sub
  4 (0x4cf760), which clears `+0x691`, `+0x692`, `+0x634` at the next
  tick, so the escorts of this pass still see it. The blow is marked by
  0x4bf3aa for every shot landing on a ship (not one with `+0x251` 2,
  being built or sold: a ship mended at a dock is marked, nor a hook of class 9 with `+0xed` 100): `+0x691` 1,
  `+0x692` the shot's `+0xbe`, `+0x634` its cell (not decoded: `+0xbe`
  of a shot with no shooter; the port marks only shots with one). Ported
  since: 0x4c6100's behaviour command for a new escort (0x4c42f0 with
  -1s, when group `+0x14` is 0) and notice 300's (the group's masks):
  case 0x23 sub 1 turns guard, ram and board on with the masks `+0x685`,
  `+0x689`, `+0x68d` and fire state `+0x5d1` 4 ("Ships' behaviour and
  masks"). None: ships unloading this class count
  working; laden ones (`+0x47f/+0x480/+0x481/+0x782`) count and, not
  unloading and with flag bit 0, go to 0x4c6640's port (0x1f); its idle
  list is never filled (no follows): a ship unloading this class
  (`+0x25d & 0x40`, `+0x76e` the zone's class, `+0x772` 0xc) asks
  0x4e4f90, which only sums its cargo bytes `+0x47f + 0x480 + 0x481` and
  writes nothing, and the list takes a ship only when `+0x25d & 0x40`,
  read again after that call (0x4ca71c), is clear, so every such ship,
  laden or empty, goes to the busy list (0x4ca767), the list of idle
  ships stays empty and the 0x4d8b30 sub 1 follows to a busy ship at
  random (one LCG draw each, 0x4ca80d) are never sent nor drawn; none
  laden, working or on a route:
  `+0x5d = 1` for sides 1 / 2 when `+0x24 >> 16 != 1`. With a route, see
  **Route**. In the branch
  with deposits, after the ships, each `+0x35` cell in order goes to every
  ship as command 0x29 `{2, cell, 0xff}` (0x4a7230: kept once in the
  path memory `+0x5cd`), at every re-plan.
- **Query 0x79** (0x4a0d00 -> 0x4c8da0, only for a 0x78 block): no ships
  or no merchants (and not a leviathan zone with ships): 0x1f5 / 1;
  deposits left (0x4c5b90 mode 1, which also forgets the gone ones): 500,
  or 0x1f5 / 0xb with no port list; none: 0x1f5 / 10. 0x47a570 takes 1 to
  its p4, 10 to its p3, then its time, then its events (each switching
  the state, the last winning). Player 16's zones have no port list
  (0x4c65c0 prunes its class-0x40 entries), so they answer 0xb.
- **Commands** (0x4c89b0, through 0x520cc0; with the route flag stored
  in `+0x3d` instead, 0x4cab20): 0x1f `{port, 1, 0x5a}`; else by class 400
  0x1a, 0x191 0x19, 0x192 0x1d `{deposit, port, 0x5a}`; any other class
  command 0 with no payload (not sent here). The gather command (0x4aab00)
  sets `+0x76e`, `+0x77e` / `+0x78a`, `+0x797` and the box, and `+0x7ad`
  to the payload's port if it is the player's port (0x39 not asleep;
  player 16 class 0x40), else 0.
- **Route** `+0x61` (0xc bytes, 0x4ce5e0: `R[0]` a count, `R[1]` the
  waypoints `{x, y, marker}`, `R[2]` the progress `{ship, id, dir byte, x,
  y}`). Only the human makes one: a Shift move click (0x49e230 case 0x14
  -> 0x49de70, key flag 4) on a group whose block is neither null nor a
  move's 100 makes it if null and appends the click's cell unless listed
  (0x4ce6c0 with -1); no ship is sent (0x4cec00 needs the cell listed in
  a route just made: never). Without Shift the group gets a move order
  (0x4ccc30), which ends the zone; with Shift on a group with no block or
  a move block, a waypoint move (0x4cf2c0, not ported: the port's
  per-ship waypoints). The brain and the computer never make one. The
  marker picture (0x4ced80, placed by 0x4ced10 at `(x*64 + 14, y*64 +
  18)`) is not ported. Port: `GatherZone.route`, `Order::ZoneWaypoint`
  (app.rs: Shift right click on a zone group's own ships, the click's
  cell, once for the selection).
  - The route flag: 0x4c96e0's gather (its unload with no room goes with
    route 0), 0x4c8f20 / 0x4c9270 (from 0x4c9930 and 0x4ca290 with it),
    0x4c9ae0 (the route's being set; its first 0x4c9930 with route 0) and
    0x4c6ee0. A command given with it is held (0x4cab20: `+0x3d`, 0x14
    bytes `{ship id, a, class, b, code}`, the ship's earlier entry taken
    out), then 0x4c9c60(a, b) puts the ship on the route: an unload with
    `(0, port)`, a gather with `(deposit, 0)` (0x4c95a5 / 0x4c95ca: the
    gather passes no port, so the "both set" case does not occur).
  - **0x4c9c60**: kind `s` = 1 for a deposit alone, -1 for a port alone,
    else 0; `c` = 0x4ca950; no waypoints, `s` 0 or `c` 0: nothing (the
    command stays held). Entry `k`: for a deposit `c < 0 ? n-1 : 0`, way
    `c`; for a port `c >= 1 ? n-1 : 0`, way `-c`. The ship (`+0x5a3`,
    `+0x5a7`) more than a cell off W[k] on either axis, or `n < 2`: move
    there (0x4cf070, command 0x14 `{x, y, 0}`, priority 0), progress `(-1,
    -1)` with the way, 1. Else W[1] or W[n-2] (deposit: `c >= 0 ? 1 :
    n-2`; port: `c < 1 ? 1 : n-2`), move there, progress that cell, 0.
  - **0x4ca950** (the way): no waypoints 0, one 1; else the flood
    0x4a1e90 (draught `+0x592`, limit 5000, bigger nearer) from the
    deposit's cell (400 `+0x90/+0x94`, 0x191 `+0x9d/+0xa1`, 0x192
    `+0xa9/+0xad`; flood null 0): `f[W0] > f[Wn-1] ? -1 : 1`; else from
    the port's cell `+0x74/+0x78` (no null test, 0x4caad4-0x4caaf2; null
    only off the map, where the port gives 0): `f[W0] > f[Wn-1] ? 1 : -1`;
    neither found 0. So the ship enters at the end farther from its goal.
  - **0x4ce8a0**(ship, R, x, y, remove, mark): R[2] made if null; the
    ship's entry with `remove` and `|dir| != 2` is taken out (`R[0] -=
    1`), nothing else; then, the ship within 0x557480 distance 2 of `(x,
    y)`, notice 0x136 with that cell (0x4a1700, to the ship's group); the
    entry set to `(x, y)` (its way to `mark` when not 0), or appended
    (`R[0]` = its index + 1). **0x4cea20**: the entry's cell (0 for `(-1,
    -1)`) and way; none -1, way 0. **0x4ce7c0**(x, y, dir): the waypoint
    after `(x, y)` that way (`dir >= 1` forward, else back), as a cell;
    off the end, end flag 1 and -1, but 2 / -2 go round to the other end;
    `(x, y)` not listed -1, end 0.
  - **Case 0x136** (0x4ca120-0x4ca215): route set, the ship in the group
    with `+0x25d & 0x80` (on a move; the port: a plain move's goal and no
    task); `nx` = 0x4ce7c0 from rec[4] with its way. `0 < nx < W*H`: move
    there and 0x4ce8a0(nx, remove = end). Else with end: 0x4ce8a0(-1, -1,
    remove 1), then its held command (0x4cac30), or, none, 0x4c9930 with
    the ship given (0x4c96e0 skipped) and route 0, its value the case's.
  - **0x4cac30**: entry k (found by 0x4cabd0 if < 0) of the ship: an
    unload whose port is no longer in a non-null `+0x28` stays held, 0;
    else sent with route 0 and taken out, 1.
  - **The 0x136 posters** in 0x4a3f40: the ship has just come into a cell
    (`+0x5a3/+0x5a7` set) that is its move goal `+0x5b2` (0x4a49af), then
    0x4a2f60 unless braking (state 4); or (0x4a49f6-0x4a4a67) not between
    cells (`+0x596` 0), no reserved cell (`+0x5ab < 0`), the next step
    refused (0x4a3a70), 0x4a2f60 unless state 4, and 0x557440 from the
    goal exactly 1. Port: `Ship.goal` set by each plain move, posted in
    the move loop at the cell change and at a refused step with
    `between` clear (the stop there is not ported: the held ship keeps
    its route).
  - **Re-plan with a route**: deposits found: the route flag is set for
    every ship; a ship listed (0x4cea20 >= 0) and lying still (`+0x58c`
    0) is sent to the next waypoint (0x4ce7c0; progress not updated), the
    normal plan only past the end; listed and not still: nothing. None
    found: listed and still, the same (past the end or out of the map:
    the normal branch); listed and not still: `local_34 = 1`, which keeps
    the zone from done and is the route flag of the later ships' unloads.
- The navy's generator is `[0x5c7504] +0x18`, seeded with `game +0x2ee5`
  at its first tick (0x49ac11 / 0x49ac1f), one for every player's fleets
  (`Scene::navy_draw`).
- Ship side: a full or done amber or fish ship ends its task (event 8
  0xc0) and posts 0x138 with its class; after unloading (step 0x13) the
  task ends, notice 0x139, and the ship moves off the berth: it goes back
  only when its zone sends it. An amber lying fallow taken by a gather
  command is exchanged for the nearest standing one of its box (0x4ac610
  mode 1), as 0x4b0590 step 1 finds on arrival, else refused (the port
  does it at the command). The port does not keep an exhausted amber's
  state 1 (one tick before it lies fallow) apart.

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

**Front-end screens** (startsys.cpp's screens, 0x528000-0x540400; the
dispatcher 0x53e1e0). Captions are the exe's strings; ported as the
original:

- Buttons (0x4ea2d0, _sbutton.cpp 0x54ab20, captions 0x4ea470 in BS with
  base variant 2): normal, picture 0 and variant 2; under the pointer,
  picture 1, a variant 0 shadow centred in the button and variant 3
  centred in (0, 0, w - 4, h - 4); held down with the pointer inside,
  picture 1 and variant 3 centred, no shadow. The action comes on the
  release inside; the pointer leaving lets the button go.
- Briefing (Starttsk.cpp 0x53f360): Start (1010) at (244, 543) and Cancel
  (1002) at (440, 543) when game + 0x3215 (the campaign mission number,
  set by 0x537f60 from `miss%d%02d`, by the end screen from + 0x3305) is
  0; else Start alone at (335, 543). A difficulty's hit area is its
  M_CHECK picture, 23 x 23.
- Custom missions and Load game (0x52c5c0): the map viewer at (10, 30),
  19 cells of 8; the description edit at (204, 59), 555 x 144. Load
  game's list (0x52fe90) from (12, 248), ten rows of 27, the text in MH 4
  across, centred in 26 (the chosen row's in 24, variant 2); a tab's
  caption (0x4eaa70) centred in (0, 0, w, 28) when open, in (1, 1, w, 28)
  in variant 1 when closed.
- Custom missions' players (Startspl, 0x53b940; list made by 0x52a990
  for type 6, an entry of 0x174 bytes: kind + 0x184, name + 0x188, id
  + 0x294): every `DESCRIPTOR` player but nation 4. `HEAD_CUST` (three
  dwords a, b, c; absent 1, 1, 0) decides the human: with a set the
  player whose `nation * 4 + slot` is c, else the first; the human is
  kind 2 with the session's name, the others 0x528a40(block byte +2): 1
  AI Medium (4), 2 AI Strong (5), else AI Weak (3). Every battle and
  single of the CD has a = 1; the singles have b = 0. Row k (k < 11)
  stands at (204, 247 + 27k): LPLAY_FL at (2, 2), LPLAY_TP (110 x 21) at
  (26, 2) with string 1146 + kind ("Disable", "Available", "Human", "AI
  Weak", "AI Medium", "AI Strong") and LPLAY_NP (386 x 21) at (171, 2)
  with the name, each in MH 4 across and centred down, variant 0 for
  kinds 0/1, 2 Human, 1 AI. Over them three _sbuttons (0x529de0): the
  kind field (id 0x197 + k, invisible, LPLAY_TP's size), the arrow M_CBUT
  (29 x 21) at (342, 249 + 27k) (id 0x1af + k, 0x4ea6e0: M_CBUT1 pressed,
  M_CBUT2 disabled, else M_CBUT0) and the name field (0x1a3 + k,
  invisible). The field or the arrow released inside opens the kind
  pop-up (0x52ebe0) for row k's player when none is open, unless a != 0,
  b == 0 and the player is Human, at (196 + 19, 216 + 42 + 27k), on the
  last row (k = 10) its picture's height higher. Entries: a == 0 or b !=
  0, Disable, Human, AI Weak, Medium, Strong in `T_PLAYER5` (146 x 131);
  else the three AI levels in `T_PLAYER3` (146 x 87). Drawn (0x52f270)
  as a sprite (layer 10): the picture, LPLAY_TP at (16, 12 + 21i) under
  the current entry, each entry's string 1146 + kind in MH variant 0, 4
  across and centred down in (16, 12 + 21i, 110, 21). Its list (_slist
  0x54cd00, rect (x + 16, y + 12, 110, 21n)) starts at entry 0. Left down
  on an entry (band from its top to top + 21, both included, the first
  that holds the pointer) notes it; let go on the same one, not dragged,
  chooses it; moving with the button held from the current entry onto
  the next or the one before makes that one current (a drag: its release
  chooses nothing). A left or right press outside the picture (its left
  and top edges count as outside; StartMouseClassTy 0x52fbb0) closes it
  unchanged (0x1d5 -> 0x52f100). While it is open the screen's pointer
  receivers are off (Sinput 0x1a, back with 0x1b) and its bindings
  suspended (0x18, back with 0x19). A choice (0x1cc) different from the
  player's kind: Disable empties the player (id 0, name ""); Human turns
  the Human row (0x528a70) into AI Weak (name "") and the player into
  Human with the session's name; a level sets it. When the player was
  disabled or becomes disabled or Human, the map's player marks are made
  again (0x528de0, 0x52f470) and the viewer goes to the Human's starting
  view (0x52a250; none, it stays). Start (0x178) goes straight into the
  game (0x101, no briefing): side + 0x1a1d by player = 0 for Disable, 1
  for the Human row, 3 for a computer row with + 0x1a31 = 0x528a20(kind)
  (AI Weak 0, Medium 1, Strong 2); 0x522d50 makes a computer player for
  side 3 only. A disabled player's objects are never made:
  PlaySystemClassTy::CreateSystemObjects (0x51e810, the callback of
  0x549850 over the `OBJ_` records, 0x522e65) returns at once for a record
  whose dword 1 is 1 (player-owned) and whose owner's side (dwords 2 and 3,
  nation * 4 + slot) is 0 (0x51e82b..0x51e845); no computer player, no
  fleet list (0x49b840: sides 1 and 3 only), not counted by the judge.
  With no Human row (0x528a70 < 0) `[game+0x3212]` = `[game+0x3213]` =
  0xff (no viewing player) and `[game+0x3225]` (the mission's `JUSTDEMO`
  flag, 0x4ed520; its only reader, the navy 0x49ab30, skips the fleets'
  tick when there is no viewer and it is 0) = 1 (0x53bfe6..0x53bff7); else
  the Human row's nation and slot and 0. With no viewer: no fog or clouds
  are drawn (DarkTy 0x48bbd0 attaches its overlay, 0xcc4 and the minimap's
  `OBL_SM` only when 0x3212 != 0xff); the view is not centred (0x50f910
  message 0x101, 0x510148) and stays at scroll (0, 0); the pointer
  (0x5175b0) only moves its sprite (0x517635 -> 0x517a16 -> 0x5166b0): no
  mode, no object info; the bar's minimap ignores the selection
  (0x4fffe0 0xc453, 0x5002ce). The judge (Boss_kib 0x448610, game type 2)
  with no side-1 player counts the players whose side is not 0 owning
  anything (0x4c46c0 with ship mask 0xfff and building mask 0x1fff: ships
  of kinds 0-11 and buildings 0x32-0x3e); more than one, nothing; else
  `[game+0x3291]` = 0, `[game+0x3301]` = 0x100 and, unless the screen's
  +0xee is set, command 0x100: the main menu, no end screen. Not decoded:
  what the money panel shows with no viewer (0x505530 takes its amounts
  from message 0x10aa, sender not traced) and what a left click does in
  pointer mode 0 with no viewer (0x5180e0 not followed): the port leaves
  both as they are.
- The map preview (Startcom.cpp 0x52f470, after the terrain): the marks
  of the mission's `LIST_OBJ` (kind 20, 0x54a8b0 in 0x52a990, rebuilt as
  a list by 0x55f350: element size +8, count +0xc, elements from +0x1c),
  23-byte entries: +0 type (1 a player's object, 2 anything else), +1
  size (4 big), +2 a player's object's nation and slot bytes or anything
  else's kind (dword), +6..+9 not read, +10 colour key (-1 none), +14 x
  and +18 y (map pixels), +22 shown. 0x528de0 keeps an entry when its
  type is 2, or there is no players list (Load game, type 5, has none),
  or its owner's row kind is not Disable (a 20-byte table nation * 4 +
  slot; the lobby with +0x110 == 4 also drops the barbarian slot +0x114).
  Two passes, type 2 then type 1. Frame: a player's object nation * 4 +
  slot, 0x10 for nation 4, | 0x80 with size 4; anything else by kind (1
  0x12, 2 0x13, 3 0x11, 4-8 0x14-0x18, 9/0x13/0x15 0x19, 10-0xd
  0x1a-0x1d, 0x10-0x12 0x1e-0x20, 0x14 0x21, 0x16 0x22, 0x17 0x23, 0x18
  0x91, others 0). With bit 0x80 the 16 x 16 frame (index & 0x7f) of
  `LMAP16X16`, else the 8 x 8 one of `LMAP8X8` (controlm), its top left at
  (x / 8, y / 8) (toward zero, each at least 0); the key's pixels left out
  (0x56dd90), or all copied with -1 (0x562770). Drawn when +22 is set
  and: screen type 7 everything; Custom missions (type 6) with
  `HEAD_CUST` a != 0 and b == 0 players' objects of the Human row only
  (0x528a70; every player's without one); otherwise (types 5, 6) players'
  objects only. Rebuilt (0x528de0, 0x52f470) on a kind changed from or to
  Disable or to Human. Then the fog of the mission's visibility data
  (`[game+0x19f9]`) over it (not ported; which screen is type 7 not
  identified).
- The map tick (`[0x5ca538]`, Config + 0x10, 1 by default at 0x501710,
  saved with Config): made by 0x52c5c0 on the Custom missions and Load
  game screens after their own part, `M_CHECK` ticked when set at (165,
  185), message 0x1dd (click, or Alt+A, 0x38 0x1e); 0x52e450 flips it,
  makes the map picture again (0x52f470 makes none when it is 0; the "Map
  (%dx%d)" title stays) and recentres the viewer (0x52a250). No other
  screen or panel touches it. Not
  decoded: how the pop-up's list and its two buttons, made by 0x52ebe0
  while the screen is up, get message 8 (their pointer receiver 0x13 and
  their Enter, Up and Down bindings; 0x541c10, 0x541b40 and 0x540030 send
  message 2 only; to read: 0x55d6e0, Sapp 0x5402b0/0x5405b0); the port
  takes them as made when the pop-up opens. It changes nothing seen:
  bound at priority 0 next to the suspended bindings, those keys start
  suspended either way.
- Connect (Startprv.cpp 0x538440): the name edit at (43, 256), its caret
  palette colour 0xfb (0xffff00 in the front end's backdrops).
- End screen (Startrpt.cpp 0x5390a0, 0x539350, 0x539b20): M_BKGND_03 won,
  04 lost, and OK (1001) at (345, 540) in M_BUT2. Its OK, after a won
  campaign mission (+ 0x32fd set, game + 0xd80 = 1), opens the briefing
  of `miss<campaign><NN>`, NN = + 0x3305 (the number the mission's brain
  hands 0x478e30: NN + 1 for miss000-miss012, -1 for miss013, then the
  main menu); else the main menu. With bit 2 of the options word
  [0x5ca53c] (the installer writes 3, setup.exe 0x404530 after the
  defaults' 1 at 0x4053d0) a video comes first, command 0x128: case 5
  `MIS%03d.AVI` (next + campaign x 100) then the briefing (0x10c), case 6
  `OUTRO%d.AVI` (campaign) then the main menu (0x100); a missing file
  (FindFirstFileA, 0x4ecc05) goes straight to the next command. This CD
  has MIS000 and OUTRO0 only.
  Only after a won campaign mission: RAM_BEST at (400, 310) with "Best
  Conquerors" (1042) at (9, 1) in it, BH variant 2; and the twelve
  figures of + 0x3309 to + 0x3335 (0x539a30; written by 0x478e30), labels
  1047-1058 at (15, 85 + (h + 7) i, plus 15 from the 6th, 15 from the 8th,
  30 from the 12th; h the tallest BH glyph, 23) in boxes 420 wide (300
  from the 8th) and h high, BH variant 2 gaps 2, centred down; values 5
  past the box, 80 wide, gaps 1. The total is the campaign's so far
  (+ 0x3335 adds each mission's tenth, cleared when a campaign starts).
- Best Conquerors (Startba.cpp 0x528210, 0x528390): the RAM_BEST
  picture at (15, 305), OK (1001) at (345, 540).

**Start reports** (`.RPM`). The briefing's Start (0x53f550 case 0x178)
writes one when game + 0x32fd is set and + 0x3305 > 0, or + 0x3305 is
0: `<MainPath>MISSIONS\` + `SAVE_` + string 100 `MISSION_` +
%02d(+ 0x3305 + 1) + `.RPM` (format 0x5be97c), e.g.
`SAVE_MISSION_01.RPM` for the first mission. 0x53a3a0 writes the dword
0x5052 | difficulty << 16 ("RP", then DAT_005ca78d: 0 Easy, 1 Normal,
2 Hard), 0x151 bytes of game + 0x32fd (its + 0x40 the count of carried
objects from list + 0xbd, or 0), then each carried object as a dword
size (list + 0x3c) and its bytes. Load game's "Start from..." (resource
+ 0x1a0 = 4, 0x52b7c0) lists the files of `<MainPath>MISSIONS\` whose
first word is 0x5052 (0x53a5a0), each as `"%s  [%02u/%02u/%04u, %02u:%02u,
%s]"` (0x5c26c4): the name without its extension and `SAVE_`, its last
write in local time, month first when string 10000 starts with '0' (as
this CD's), else the day, and string 106 + difficulty; with no time, the
name alone. Loading one (0x530480): reset (0x4ede60), 0x53a670 reads it
(the magic checked; the difficulty into DAT_005ca78d, the block into
game + 0x32fd, the objects into new lists), then 0x537f60(game, 0,
+ 0x3305) opens that mission's briefing (or first MIS000.AVI for
mission 0 when DAT_005ca53c bit 2 is set). The block's layout beyond
+ 0x3305 (the mission's number) and + 0x40 is not decoded: the port
writes its own record after the two words, as JSON (the mission's
number and the campaign record it carries in), in `MISSIONS` of its
saves folder, and reads its own back.

**Player's name** (Config 0x5ca528, 0x26c bytes, the name at + 0x50 =
0x5ca578, 0x104 bytes; HKLM `...\Ancient conquest\Config` value
`Config`, its byte sum at + 0). Read at start-up (0x500810); on a bad sum
the block takes its defaults (0x501710), the name Windows'
`RegisteredOwner` (empty when absent; the port has no counterpart and
starts empty). The main menu's Start scenario (0x168), MultiPlayer
(0x16a), Custom missions (0x1cd) and Load game (0x1cf) make the session's
name list (resources + 0x1b9) from it once (0x53d220); the connect
screen's edit edits that list; Custom missions' Human row and the end
screen's row take it (the end screen the stored name when the list is
empty or not made). At exit (0x4ee6c0 -> 0x53d2f0, unless started by a
DirectPlay lobby) the list's string goes back into + 0x50 and the block
is written (0x501090). The port keeps the name in its settings file
(`player_name`), written when the program ends.

**Best Conquerors table** (0x5c9ed0, 0x540 bytes: 4 campaign types x 7
rows of 0x30, a name of 0x28 bytes, the score at + 0x28, the game's id at
+ 0x2c). The original keeps it in HKLM `...\Ancient conquest\Config`,
value `Result` (packed, unpacked by 0x56e3d0, its byte sum in
`CRCResult`, the table zeroed on a mismatch), read at start-up by
0x500810 and written only at exit (0x4ee6c0 -> 0x501090). The port keeps
it in its settings file (`best`), read at start-up and written when the
program ends.

- Type: game + 0x3214 is 1 << campaign, set by the campaign start
  0x537f60 (`miss<campaign><mission>`), 0 at start-up; 0x5291b0 takes
  its lowest bit. The end screen uses it as is (0x5397c0), Best
  Conquerors with 1 for 0.
- Score: game + 0x3335, the twelfth figure (the campaign's total). Game
  id: game + 0x1a0d, set on every load (0x4ed770 -> 0x4ed3f0): the file's
  `ID_GAME` resource (saves write it, 0x511926), else the low dword of the
  local time as a FILETIME (mission files have none, so every mission
  started gets a new one). The port's saves carry it as `game_id`.
- Entering (0x5397e0, from the end screen's widgets 0x539350, only after
  a won campaign mission, game + 0xd80 = 1 and + 0x32fd): nothing if a
  row of the type has the same score and id; else the first row whose
  score the new one reaches (`score >= row`, signed; empty rows have 0),
  the rows below moved down (row 6 drops off), name (cut to 39, 0x5291e0)
  the resources' name list (+ 0x1b9, the connect screen's) if set, else
  Config's 0x5ca578 (default Windows' `RegisteredOwner`, which the port
  has not: empty).
- Rows (0x539b20, 0x528520, each through 0x4eb430): row k shown when its
  name and score are set; the name at (410, 344 + 24 k) on the end
  screen, (25, 339 + 24 k) on Best Conquerors, in a box 286 wide and MH's
  tallest glyph (18) high, centred down; the score centred in the 75-wide
  box after it. End screen: MH variant 1, the entered row's score 2; Best
  Conquerors: variant 2 when game + 0xb8f has the type's bit, else 0.
  Gaps (1, 0).
- The name edit (end screen, when a row was entered): type 6 at (410, 344
  + 24 i, 286, 19), 39 characters, starting active, text the name (MH
  variant 2 from its top left, the 0x174 background the backdrop's, so
  the row's name under it is not seen); a caret 2 x 18 in palette colour
  0xfb. OK (0x16c) also puts the text into the row. The connect screen's
  name edit (0x538440, (43, 256, 420, 24)) is the same, 0x104 characters,
  its text the resources' name list.
- The name edits (_sedit.cpp, handler 0x54f040, single line, flag 1):
  made active (state 1) with the caret before the first character
  (message 2 zeroes the caret and scroll fields; neither screen sends 0x29
  or 0x33 after). Active, the edit binds eleven keys (priority 0xff,
  repeating and exclusive, in this order): Esc 0x35, Up 0x3a, Down 0x3b,
  Right 0x3c, Left 0x3d, Home 0x3e, End 0x3f, Enter 0x39, Insert 0x38,
  Backspace 0x36, Delete 0x37. Characters (WM_CHAR, any but 8, 0x0d,
  0x1b) are inserted at the caret while the text is shorter than its
  limit (0x54ecd0; there is no overwrite: Insert toggles flag 2, which
  nothing of a single-line edit reads). The caret mover 0x54e5f0 clamps
  to the text's end, brings the first shown column (+ 0x146) back to the
  caret and on while the characters from it to the caret, each its width
  and the gap (1), reach the box's width; the caret's x is that sum. Left
  at column 0 does nothing, Right stops at the end, Home and End go to
  the ends, Up does nothing, Down stays. Backspace deletes the character
  before the caret, Delete the one at it (0x54ea60; nothing at column 0
  or the end). Enter posts 0x175: the end screen puts the text into its
  row; the connect screen posts 0x170, New's action (0x538af0). Esc (and
  a left press outside, 0x61) leaves the edit: state 0, its bindings
  gone, the caret not drawn, the text and caret kept. A left press
  strictly inside its box makes it active again (the keys bound again)
  and puts the caret before the first character, from the first shown,
  whose band [x, x + width + 1) holds the click, else at the end
  (0x54e430; it stores that column, counted from the text's start, in
  the field counted from the first shown). The keys repeat at the host's
  keyboard delay and speed (Sinput 0x552cc0); the port takes the
  system's own key repeat.
- Best Conquerors' title (0x528520): `CD<t+1>` (game + 0x66d + 0x104 t)
  when game + 0xb8f has bit t and the name is set, else string 1038 + t
  (" "); variant 2 or 0. The installer (setup.exe 0x404cc0) reads the
  CD's `ID_CD.DSK`: 4 bytes, the first ORed into the registry `Version`'s
  low byte (game + 0xb8f), then up to 0x104 of name, copied to `CD<k+1>`
  for k the lowest set bit of that byte (0x4038b0). This CD's file is `01
  88 f0 de` "The Golden Fleece Campaign". The port's installer copies
  `id_cd.dsk`; without it the title is string 1038.

**Keyboard** (Sinput.cpp 0x553090, polled each frame from DirectInput's
keyboard state, so keys are scan codes). A binding has two keys, a
priority (the table is kept in its order), a message down and one up.
With one key it goes down with that key while no Shift, Ctrl or Alt is
held (or the key is one of them); with two, when both are down; it comes
up when the first key does. Made while its keys are down (0x553670), it
waits for the first to come up. An exclusive binding turns off those
below it with the same keys (the check for one above starts one past the
first entry above it, 0x553807) and gives them back, waiting if held,
when it goes (0x553930). A message that adds or removes bindings ends
the poll. Message 0x18 (0x553ba0) suspends every binding once more
(+ 0x5e), 0x19 (0x553be0) takes one suspension off; the poll passes
suspended bindings over. A binding made next to one of its own priority
(the entry before where it goes) starts with that one's suspensions
(0x553670). So Custom missions' kind pop-up, which suspends the screen's
bindings and then binds its exclusive Esc (0x1d5) at priority 0 next to
Start's and Cancel's, has that Esc suspended too, and turns Cancel's Esc
off: Esc does nothing while the pop-up is open (its list's Enter and its
Up and Down buttons, bound at priority 0 when the list activates, the
same). Checked by hand on the original: Esc leaves the pop-up open.

- Buttons (_sbutton 0x54ab20 message 8): their create record's key bytes
  as a binding, down = 0x2c (held, the pressed look), up = 0x2e (acts if
  still held; the pointer moving off lets it go). So Alt+letter shows the
  button pressed and acts when Alt (left Alt, 0x38) is let go; Cancel (1,
  0) acts when Esc is let go.
- Keys a screen binds itself (0x4e9840): its command when the key goes
  down.
- By screen: main menu Alt+B, S, L, C, M, Q (no Esc, Enter or Space);
  Custom missions Start Alt+S, Cancel Esc (and the tick Alt+A); Load Load
  Alt+L, Cancel Esc; connect New Alt+N, Join Alt+J, Cancel Esc (the name
  edit, made after, shadows Esc while active); end screen the edit's,
  OK Alt+O, Enter -> OK (shadowed by the edit's Enter); Best Conquerors OK
  Alt+O, Esc, Enter, Space -> OK; briefing Enter, Space -> Start, Start
  Alt+S, Cancel Esc when it exists. The port binds them in the order the
  screen makes its widgets (taken as the order they activate).

**Fades** (Palette.cpp). On every screen change (0x53e1e0) the pointer
sprite is hidden (0x55ed40), the shown palette fades out (0x4ef350(50,
5) -> 0x54a460; over the game's last frame when leaving the game,
commands 0x100, 0x10b, 0x10c of 0x4ec1a0), the new screen paints under
black, and its message 0x102 fades in to its backdrop's palette
(0x4ef530(game, backdrop, 50, 5) -> 0x54a230); then 0x516980 resets the
pointer (mode 0, CUR_STD) and shows it. Both busy-wait on
`timeGetTime()`: a step each time 5 ms have passed since the last (so
at 5, 10, ... 250 ms), step i at ftol(100 / 50 x i) percent lightness
(fade out 100 less that), then the target palette exactly (fade in) or
black (fade out). Each step scales the palette in HLS (0x549f20 with hue
and saturation 100): RGB -> HLS (0x549ba0: bytes x 1/255 f32, L = (max +
min) / 2, S = d / (max + min) or d / (2 - max - min) past 0.5, hue in
sixths x 60, 999 for a grey), L x percent x 0.01 (capped 1), HLS -> RGB
(0x549e30, each channel ftol(c x 255)); values kept as f32 where the
code stores them. All 256 entries are set: 0x56c360 and 0x56c230 would
keep the 20 Windows colours in 8-bit mode only when the DirectDraw
object's cooperative flags (+ 8, stored by 0x566e60) lack bit 1, and
every caller (0x4ed98e, 0x4ec795, 0x4ec956, 0x4ef222) passes 1, which
0x566e60 turns into FULLSCREEN | EXCLUSIVE (0x11). Starting a game
(gamesys message 0x101, 0x50f910): pointer hidden, fade out, `LOADING`
painted and faded in to its own palette (0x4ef420, 50 x 5 ms), the game
loaded, fade out again (50, 5), resource `PALETTE` taken, and message
0x102 fades the game in from its idle (0x50f740 -> 0x54a100): each idle
pass, while i < 25 and at least 1 ms after the last step, step i at
ftol(100 / 25 x i) = 4i percent; at 25 the palette exactly and message
0x4e8. Every game build goes through it: 0x4ebf50 (commands 0x101,
0x102, 0x103 of 0x4ec1a0) makes the game screen and posts it 0x101. The
briefing's and Custom missions' Start and the multiplayer session's
(0x532b20, the post at 0x5373ab) send command 0x101, which has no fade of
its own. A saved game loaded (the Load game screen, 0x530480 -> 0x4edcc0
then 0x102 at 0x53097c; the file panel's load, 0x50a700/0x50b54c;
Restart, 0x4fd at 0x50aea2; the bar's Ctrl+O with a save name, 0x5128b9)
is command 0x102: sounds stopped, the game's systems torn down, pointer
hidden and a fade out (50, 5) from the screen or the game's picture,
the save read, then 0x4ebf50 and gamesys 0x101: a second fade out, black
to black, the full 250 ms, `LOADING` in and out, the game in. The demo
(0x103) hides the pointer and fades out before the same build. Quit
(the main menu's 0x164 -> command 0x131 at 0x53128c; the in-game Alt+Q's
yes, 0x4f0; the dispatcher's own 0x131 at 0x53e271): in multiplayer a
leave message (0x548fe0), then a fade out (50, 5) and the loop ends
(`[game+0x1c]` = 1; 0x4ee6c0 saves Config); no pointer hidden, no fade
in. The port, drawing in true colour, shades every pixel; the pointer
is not drawn while a fade runs (the original's, a sprite, fades with
the palette on Quit). The port's quick load (its own Ctrl+O) and
Restart (which reloads the mission) take the 0x102 fades.

**Version.** The title screen's line is `(game + 0xa7d >> 16) & 0xff`
in string 1145 "Version 2.%02d"; + 0xa7d is the registry `Type`, which
the CD's installer writes as 0xa21e << 16 | the install type (setup.exe
0x404530, the word at 0x416040): "Version 2.30".

**Briefing sound** (Starttsk.cpp 0x53ed40, idle 0x53fba0): it opens
`<VideoPath>TASKS.DKD` and takes waves `VOICE<type>_<mission:02>` and
`FON<type>_<mission:02>` (else `FON000`) and picture `BKG..` (else
`BKG000`, else M_BKGND_05). With a FON and sound on, the FON plays looped
on channel 0 at the effects volume (0x568b30 flags 1, DAT_005ca56c); else
0x525370 case 8 (sound 325, `mus_menu`, looped, channel 0, the same
volume) unless DAT_005ca55e is 1. 3000 ms after the screen was made the
voice (if any) plays once on channels 7-8. This CD's tasks archive has
FON000 only. Start (0x178) and Cancel (0x16e) first call 0x53ee30: when
the FON (+ 0x10c) or the voice (+ 0x110) was started and sound is on
(game + 0xd20), every DirectSound buffer stops (0x569070) and the channel
table (game + 0xba0, 0x60 dwords) is cleared; the waves, picture and
archive are freed. Start then posts 0x101 (the game, no menu music);
Cancel posts 0x100, whose handler plays `mus_menu` (0x525370 case 8)
unless DAT_005ca55e is 1. DAT_005ca55e (Config) is the music device
(0x524600: 2 CD audio, 4 and 8 the other two devices, 1 none; 0x500810
turns 2 into 1 when `CDAudioDrive` is "NONE" or the paths are not fixed
drives; default 2): `mus_menu`, a DirectSound sample, plays in every
mode but 1. The port's sound panel's None is 1; it plays `mus_menu` at
the effects volume.

**Idle demo** (0x530bc0): command 0x103 (the `demo` saved game, then its
DEMON stream) once game + 0xa83 ms pass after the title screen was shown
(+ 0x30d5) while the application is active; + 0xa83 is the registry
`Version`'s second byte x 1000, 255000 from the CD's installer, the
value that turns it off. Nothing to port for an installed copy.

**Info pointer** (pointer mode 0x1e, `CUR_INFO_`, hotspot (-9, -37),
0x516ea0) is unreachable in this build: the only reply of mode 0x1e is
the war temple's (TempleWarClassTy 0x439260, 0x43a0a9) to pointer state
0x22 with kind 9 (bit 0x4000, the "cannot" look, over an object owned by
a nation below 4 or of class 0x45, 0x46, 0x190-0x192, 0x1fa, 0x1fb, or
over nothing), and every writer of the pointer's state (+ 0xdd) and kind
(+ 0xe1) was read: kind 9 is stored only with state 0x23 (command 't',
0x4f41db), which the temple does not answer. No button, key or panel
enters it; the port has no path to it either.

**Command buttons' right click** (_sbutton, second message 0x1d83 + i,
taken when its id is set; the button looks pressed while the right
button is held and acts on the release inside; 0x4f46c0, when the
button exists and is not the back button): by its command code, 0x14,
0x15, 0x3a-0x3c, 0x3e-0x54 and 0x67 reset the pointer (+ 0xef, + 0xdd,
+ 0xe1, + 0xe6) and send order 10 with no data to the selection
(0x518db0); 0x21-0x39 and 0x7d reset it and send order 0x3c {group,
item} (ships 0x21-0x2c and 0x7d group 1, training 0x2d-0x35 group 2,
armour 0x36/0x37 group 3, rams 0x38/0x39 group 4); others nothing (the
inner switch's 0x3a/0x3b labels are dead: those codes take the order-10
branch). Both, and the left press (0x4f34c0, 0x4f34fc/0x4f3508) and the
pointer (0x5175b0, 0x5180e0), act only while the screen's + 0x17e6 (the
game paused: gamesys 0x4e5 case 1 flips it and shows `WIN_PAUSE`;
cleared by messages 8, 9 and the constructor) and + 0x17ea (a "Bad
connection to" box up, string 1127: Players.cpp 0x520a52..0x520b26, in
multiplayer, a player silent for over 20000 ms; cleared by a message from
him, 0x522d50 and gamesys 8/9) are both 0. The order goes through
0x518db0 -> 0xc458 to the navy -> 0x49e230 -> the order queue (0x520cc0)
-> Players.cpp 0x5228c0, which delivers message 0x280f with the order
byte (+0xc) 10. What it calls off is the object's, whatever button sent
it (busy `+0x10c`, job `+0x110`, amber paid `+0x118`, progress `+0x120`
in percent; state `+0xa4` 0 being built, 1 standing):

The chain decides who reads what first. **LandObj runs first** for
every building but towns and the dock upgrade: School, Smithy, Studio and
the temples call 0x427f50 -> 0x425990 before their own case; the port
0x4e7640 calls it unconditionally; Fortress 0x419f80 calls 0x4e7640 at
0x41a176 (for every message but 0x158bf); Barb 0x40ef70 and the yards
reach it through 0x4e8660; TownObj 0x441ba0 calls it at the end
(0x444f17); Dock 0x416f90 tests its upgrade first and returns if it took
it.

- LandObj 0x425990: busy with job 0xff (its own mending): `+0x10c` = 0,
  `+0xa4` = 1, no refund (mending is paid step by step), which leaves the
  class's own case nothing busy; then, with `+0xe8` != 0 (the name of
  the construction scaffold picture loaded on layer 2 by message 0xc453:
  "RAM_072X072" from the BuildingTy constructor 0x427750, "RAM_128X128"
  from TownObj's 0x43b000; only the Cyclops lair leaves it 0) and
  `+0xa4` == 0: the whole price 0x42d8b0(class) back in amber only
  (0x4a8dc0; a fort's arming is not) and 0x424740: `+0x158` = 0 (so the
  drain's fall sound and rubble are skipped), the queue's flag, message
  0x138b drained at once (0x427bb0: 0x2874 to the settlement; forts
  0x4184f0: the kill report and 0x2491 {settlement, class - 0x34}, the
  town's count of forts going up less one; 0x423d60: state 2, the kill
  report 0x50e750 with `+0x1ac` = 0xff, the owner's building lost and
  nobody's kill, layers and unit lists freed, the cells cleared, then
  message 10: removed), no ruins.
- TownObj 0x441ba0, its own case first: upgrading (`+0xa4` 0, `+0x23c`
  set): 0x42d8b0(0x33) x (n - f) / n back (`+0x7c` record: `n` +0x49,
  `f` the frame +0x45; integers; the mending share of the price is not
  refunded), class 0x32 again (vt+0x54), `+0xa4` = 1, `+0x100` = 1,
  pictures (vt+0x38), `+0xa8` untouched; else busy: `+0x10c` = 0;
  unless job 0xff `(100 - +0x120) x +0x118 / 100` amber back; job 1 (the
  training) `v = (100 - p) x +0x11c / 100` (the raw fish) into the town's
  bytes `+0x252` / `+0x253` instead of the player (0x44316a: with `+0x252`
  set, `T = v + +0x253 + 100 x +0x252 - 100`, else `T = v`; `+0x252 = T /
  100`, `+0x253 = T % 100`, a last part of 0 written 100, else `+0x252`
  one more; nothing uses them: the save 0x4416c0 writes them, 0x43c650
  copies `+0x252` into its info record's word +0x12, whose only reader,
  the info cloud 0x50bf10 (from 0x423a60, 0x425990, 0x41d300, 0x41d4a0),
  tests the record non-null and draws an empty cloud, and no load writes
  them back: the fish is lost); job 2 the stock repair's walk
  (0x4430f0, above); then for any busy job the training list freed
  (`+0x258 = +0x254 = 0`, `+0x25c = 5`: what was not made is lost). The
  settlement's call (events 0xdc-0xdf) for the local owner when it
  cancelled an upgrade or a busy job. A new town under construction
  (`+0x23c` 0) is neither: LandObj pays 500 back and removes it.
- Dock 0x416f90: upgrading (`+0xa4` 0, `+0x1e8` set): 0x42d8b0(0x3b) x
  (n - f) / n back, class 0x3a again, standing, `+0xf8 = +0xf4 = 1`,
  return: no call, no chain. A yard under construction: LandObj, 400
  back, removed.
- School 0x432210 (start 0x431b60 through order 0x21 -> 0x138a), standing
  and busy: `+0x10c` = 0, `(100 - p) x C / 100` back, C recomputed from
  the same player field (job 1 0x5971ac[+0xa3] (1200, 1300), 2
  0x42d9a0(+0x5e), 3 0x42d9a0(+0x5f), 4 0x42d980(+0x51), 5-8 2000;
  0x42d9a0: 0 -> 1000, 1 -> 1500, else 0; 0x42d980: 0 -> 2000, 1 -> 3000,
  else 0), the job's player flag cleared (+0xa4, +0x5a, +0x56, +0x52,
  +0x72, +0x7a, +0x82, +0x8a); its call (0xf0 / 0xf1 / 0xf3 / 0xf2) only
  when it cancelled. `p` is `+0x120`, written each tick `(now - +0x124) x
  100 / duration` (unsigned).
- Smithy 0x434ed0: studies (order 0x21 -> 0x138a: job 2 armour kind, 3
  ram kind) refund `(100 - p) x +0x118 / 100`, flag +0x63 / +0x6a
  cleared; the refits are one batch (order 0x20 -> 0x138c: job 1, each
  refit adding its price 30/60 or 25/50 to `+0x118` and its 150 or 200
  ticks to `+0x114`, the progress over the whole batch): `(100 - p) x
  +0x118 / 100` back, the rest of the batch dropped (`+0x1ec` freed).
- Studio 0x436ee0: studies (jobs 2-4: flags +0x91 / +0x98 / +0x9f) as the
  smithy's; the weapons batch (job 1, order 0x20 -> 0x138c: amber
  0x597164[item / 3] and raw fish 0x597170[item] summed, ticks
  0x5971a0[item / 3]): `(100 - p) x +0x118 / 100` amber, no fish back,
  the list freed; job 5 (its settlement's stock repair): the message
  passed to the settlement (`+0x1c8`), `+0x100` = 1. The smithy's call
  (0xe8 / 0xe9 / 0xeb / 0xea) and the studio's (0xec / 0xed / 0xef / 0xee)
  on every 0x280f order but 0x3c (their switch has no default), so every
  order 10, even one that found them under construction.
- TempleSea 0x438300, busy: learning (job 1) `+0x120 x 1000 / 100` back
  (the elapsed percent, at the list price even when a computer of level
  3/4 paid 200), player +0xb8 = 0; any busy job `+0x100` = 1; no call.
- TempleWar 0x439260, busy: the Flame (job 1, 250 ticks): `+0x120 x
  [0x596864] / 100` amber (6000) and `+0x120 x [0x59689c]` fish (15, no
  / 100: the elapsed share of the fish price); learning (job 2) `+0x120
  x 1000 / 100`, player +0xc0 = 0; `+0x100 = +0xf4 = 1` and the temple's
  call (0xf8 / 0xf9 / 0xfb / 0xfa) for the local owner. Not busy,
  nothing.
- Fortress 0x419f80 (0x41af74; its job-0xff branch, freeing the unit
  list `+0x4cc`, is unreachable after LandObj cleared it) and Barb
  0x40ef70: all 12 unit groups cease fire (0x4469b0: mode `+0x3a` = 0,
  target `+0x20` = 0; the fort also `+0x498` = 0). A group with mode 0
  does not shoot at all (0x4469c0, 0x446a57); modes are set again by
  0x4469a0 (fire at will: creation, order 0x23 sub 1, the fort's message
  10000 0x418f60, a unit added 0x4193d0 while any group fires at will) and
  0x446960 (orders 0x15 / 0x16, a target; and a fort firing on aggressors
  when struck). See **Forts' fire** below for who sends them.
  The fort's call (0x41bd2f): message 0x280f sets a flag 1 and its
  switch clears it for orders 0x23, 0x2d, 0x32 and any not handled, so
  orders 10, 0x15 / 0x16, 0x1e and 0x22 (every kind, cast or not) play
  it, for the local player's fort (`+0x54`, `+0x55` against game
  `+0x3212`, `+0x3213`): event 0xd8 / 0xd9 / 0xdb / 0xda by nation 0-3,
  mode 2 at `+0x74`, `+0x78`; 216 and 219 have no entry, 217 and 218
  `re_o2a re_o4a re_o6a`. Order 10 plays it even when LandObj removed a
  fort under construction. The port: `Scene::fort_voice`, `FORT_CALLS`
  (halt, `fort_attack`, `fort_repair`, `self_mend`, `fort_cast`).
  The other calls' orders: the smithy (0x434ed0) and studio (0x436ee0)
  on every 0x280f order but 0x3c (so research 0x21, the refit / weapons
  batch 0x20 and 10); the sage school (0x432210) on 10 when busy and on
  0x21 when idle (`+0x10c` 0, even an unknown kind). The port plays them
  on its research start, refit and workshop training. Every call plays
  after the order's case, whatever the message that case passes on (the
  0x138a-0x138d starts and their price and busy tests) makes of it, so a
  refused order is still called; the port plays them before its own
  refusals of that kind.
  The town's (0x441ba0, at 0x444468, `local_28`): its switch runs only
  with `+0xa4` not 2, 3 (sold) or 0xff, the flag set 1; order 10 keeps it
  only when it called off an upgrade or a busy job, 0x1e sub 2 (the stock
  repair, from the workshop) and the default clear it, 0x3c (a training
  item taken off, sub 2) sets it only when one was taken off, two of the
  pointer branches of 0x32 (0x443d7f, 0x443c29) clear it; every other
  case keeps it: 0x1e (sub 1, message 0x1389 to the building to mend),
  0x1f (the Reload Object's fort pick, sub 2), 0x20 (sub-orders 2-0xb a
  building placed, 0x138c; 0xc-0x17 a ship, passed to an idle yard;
  0x18-0x1a marines, 0x138d; 0x1b-0x20 / 0x21-0x24 passed to an idle
  smithy / workshop), 0x21 (sub 1 its upgrade, 0x138a; the others passed
  to its yard, school, workshop or smithy), 0x22 (passed to an idle
  temple), 0x28 (sub 3, its berths cleared), 0x2d (sub 1, passed to a
  fort), 0x34 (sell: sub 1 an object, 2 a stock item, 3 itself, message
  0x248f). A case that passes the order on (vt+0 with the same message)
  has that building's call play as well. The voice: event 0xdc / 0xdd /
  0xdf / 0xde by nation 0-3, mode 2 at its cell. The port:
  `Scene::town_voice` at training, a training item taken off, mending,
  the upgrade, building placement and the four sales.
  The shipyard's (Dock 0x416f90): order 0x21 sub 2 (its upgrade to a
  dock) sends 0x138a and plays event 0xe4 / 0xe5 / 0xe7 / 0xe6 (228 and
  230 empty, 229 and 231 `re_o2a re_o4a re_o6a`, `DOCK_CALLS`); its
  orders 0x20 (a ship), 0x3c (one taken off) and 10 (the upgrade's
  cancel) have none.
  The temples (0x438300, 0x439260): besides the casts, command 0x22 code
  0x12 (learning) calls only when the learning began (idle `+0x10c` 0
  and paid; else `local_10` 0); order 10 as above. The war temple's
  other orders (not 10 or 0x22) reach its call with `local_10` 1; the sea
  temple's return before it. Not ported: the Reload Object's town call
  (0x1f, 0x32), the town's passing of orders to its buildings from its
  own panel (the port's buttons act on the building itself), and the war
  temple's call on its other orders.
- A ship: 0x4e2ba0 case 10 is `vt+4(8, NULL)`, the stop event (0x4aa540),
  with no permission test; a ship being built has its build called off.
  ShipService 0x4e7640 takes order 0x32 only, GameObj 0x50c9e0 order 0x29
  only.

The voices (0x50e570 mode 2 -> 0x525370): only with the unit report
option (bit 1 of `[0x5ca55f]`) and one of channels 7-8 free (0x5252d0),
within 19 cells of the listener (`+0xd08`, `+0xd0c`): the distance is
`(byte) ftol(sqrt(dx^2 + dy^2))` in cells, from the cell the caller passes
(the building's `+0x74`, `+0x78`) to the listener, which 0x524d90 sets
from the view (0x51a040-0x51a0b8: its left / top cell `+0x2c` / `+0x30`
plus its cells across / down, screen `+0x17be` / `+0x17c2`, halved and
rounded up); 20 and more is silent (the byte wraps at 256). Modes 1 and 4
cut the same. The volume (DirectSound hundredths of a dB): the effects
volume `[0x5ca56c]` within 5 cells, `master - (master + 2000) x d / 12`
from 6 to 12, -2000 from 13; the pan `min(|dx|, 20) x sign(dx) x 50`;
the sample is
0x554820's pick: the event's `SND_n` names, the first when fewer than 2,
else `rand() % count` (the C library's: `seed = seed x 0x343fd +
0x269ec3; (seed >> 16) & 0x7fff`, seeded once with `GetTickCount()` at
0x4ebdf9). The lists (dumped): 221 `re_o2a re_o4a re_o6a re_o5a`, 222
`re_o2a re_o4a re_o5a re_o6a`, 233/234, 237/238, 249/250 `re_o2a re_o4a
re_o5a re_o6a`, 241/242 `re_o2a re_o4a re_o6a`; 220, 223, 232, 235, 236,
239, 240, 243, 248, 251 have no entry.

The port blocks command buttons, left and right, while paused (no
"Bad connection" box), sends order 10 for the codes above, and runs the
handlers in this chain (`Scene::halt`): its construction steps frames as
0x4230f0; research entries refund at `p = elapsed ticks x 100 / study
ticks`; a smithy's refits keep their batch (paid, ticks, elapsed); a
cancelled training's fish share is kept per town (`town_fish`); forts and
barbarian structures keep their groups' fire (`combat::FortFire`, below).
The voices are
picked from the dumped lists with the port's own generator, for the
viewing player. An upgrade keeps its condition until its first step:
the town's 0x138a and the dock's do not write `+0xa8`, and vt+0x54
(0x424910, both ways) sets the class's maximum `+0xb0` (0x32 160000, 0x33
220000, 0x34-0x36 and 0x3f-0x41 180000, 0x37, 0x3a, 0x46 100000, 0x38,
0x39 130000, 0x3b 140000, 0x3c 81500, 0x3d 67500, 0x3e 110000, castles
300000, 0x44 350000, 0x45 280000, 0x47 80000, 0x48 60000, 0x49 40000) and
the hit points `+0xb0 x +0xa8 / 100`: rescaled, not reset, on the
upgrade and on its cancel. Its picture's frame is not reset either: the
sprite load (0x50d080 -> 0x5461d0) leaves the layer's `+0x45`, and
0x422e30 zeroes it only when `>= n`, so the construction starts from the
frame the standing settlement or shipyard showed (its loop, 0x4230f0
state 1: a step every `75 / n` ticks, drawing the frame then adding one,
back to 0 on reaching n; the first step on the first tick after the
picture was set, since every vt+0x38 call, 0x422e30, clears `+0xbc`).
The front end plays the voices and the battle sounds (channels
2, 1 and 4) by the cut-off, volume and pan above (`audio::placed`), from
the cell of the place the scene reports, and in stereo with DirectSound's
pan (the far side's channel attenuated by it). Its listener
(`audio::listener`) is the original's rule on the port's view: the cell
under the view's top-left corner plus half the whole cells its width and
height hold, rounded up; the zoom changes those counts (at the original's
scale the 592 x 512 field holds 9 x 8 cells, the 800-wide one 12, as the
original's `+0x17be` / `+0x17c2`), so zoomed out the listener stays in the
view's middle and the 19-cell cut-off stays 19 map cells. Modes 5 and 6
(bit 4; 0x525370 at 0x5255eb / 0x525a70: `d = min(byte, 20)`, `d >= 12`
silent, the same volume and pan). The 108 callers of 0x50e570 (`(mode,
x, y, event)`, the mode pushed last; event < 0 plays nothing) and the 29
of 0x525370, read in the disassembly: mode 5 events 85-87, 89, 92-94,
96, 99, 103, 105, 107, 110, 111, 114-116, 122, 125-127, 129, 133, 136,
149-155, 169-171, 174, 141, 168, 274 (0x407a59, direct), event 1 (a
ship, 0x4e00bc), events 100-102 (a land creature's death, 0x413c66:
the mode in `edi`, 5 from 0x413be7), event 5 (`firee1`, the drain
0x424214) and a
building's sound events `+0x1a8` (its fall, 0x424492; 4 `razru2` by
default, 0x422dec) and `+0x1a4` (0x425c1a); mode 6 events 88, 95, 106,
117, 128, 146-148, 271, 272; mode 4 events 144, 145; mode 1 events 83,
84, 118, 119, 121, 130 and the shots' (0x498ab4, 0x4b8990, 0x4c06db,
0x4e510f event 5); mode 2 the voices; mode 3 only event 273 (`prib3`,
0x51028c, 0x524ffb: the map's ambient loop, the front end's channel 9),
mode 8 the interface's. The port places the names of the mode 5 / 6
events' lists (`scene::AMBIENT_PLACED`, checked against the data) by
`audio::placed_ambient`. The 176-178 / 180-182 events (`se8`, `wa6`) are mode 2
(temples) but the port plays neither. The
priority byte 0x525370 passes 0x568b30 for modes 1, 4, 5, 6 (255 within 5
cells, `(20 - d) x 255 / 20` beyond; 0 for mode 2) is not used (the port
mixes without the original's channels). The town's count of forts going
up (`+0x22c`, `+0x230`, `+0x234` by class 0x34-0x36; saved in the
town's record at +0x800, loaded for record type 2 only) is one more on
each fort its order 0x138c makes (0x43b290 at 0x43b939..0x43b95b), one
fewer (only while not 0) on 0x2873 from such a fort standing (brep3.c
0x2873) and on 0x2491 {class - 0x34} (0x4184f0 on 0x138b, `+0xa4` 0 and
`+0x1c8` set: a fort called off or falling while going up). Both the
order (0x43b290, after the price test) and the structures page (0x43c740
at 0x43c80e, the button off) refuse a fort when `(stone standing + going
up) x 3 + (middle ...) x 2 + wooden ... + the new one's weight (1, 2, 3)
> 15`, standing being the counters `+0x21c`, `+0x214`, `+0x218` kept by
0x2873 / 0x2874. The port: `Towns::going_up`, `Scene::forts_full`,
`fort_down`. The frame an upgrade starts from: 0x4230f0 runs for every
state each tick: with `+0xbc` 0 or `now >= +0x7c->+0x81 + +0x7d` (the
delay, `cycle[state] / n` from 0x422e30), `+0xbc = 1`, the time kept, the
frame `+0x45` drawn and one added (vt+0x40); under `n`, the state's
per-frame work (vt+0x44); else by state: 0 stands it (state 1, vt+0x38,
the finished call), 1 and 0x10 back to 0, 2 the ruins' end, 3 0x424740.
The port keeps each standing settlement's and shipyard's frame
(`Scene::standing_frames`, saved; `step_standing_frames` before the
constructions in the tick) and resets it at the vt+0x38 calls it knows:
the end of a construction (the frame reached, zeroed if `>= n`), an
upgrade's cancel (the construction's frame, the same), and the creation
or load (no entry: frame 0, a step at once); the upgrade takes it
(`upgrade`, 0 if `>= n` of the construction picture), its first step on
the next tick. The game's standing settlement and shipyard pictures are
one frame (miss000), so the frame there is always 0. Not traced: the
other indirect vt+0x38 calls (41 sites in all; a picture changing with
the condition among them), which would only set `+0xbc` 0 (a step on the
next tick) and zero a frame `>= n`. Not ported: the pointer's own pause
test.

**Forts' fire.** A fort's or barbarian structure's 12 groups (group k
the units of item k) shoot by mode `+0x3a` (0x4469c0): 0 not at all; bit
2 at target `+0x20` while it is in the group's reach (0x447400; out of
reach the target is kept and, with bit 2 alone, nothing is shot); then
bit 1 the scan (0x446d60). 0x4469a0 (at will) sets bit 1 and clears the
target, keeping bit 2; 0x4469b0 clears both; 0x446960(id) adds bit 2 with
the target if the object is found (0x51f1f0). Bit 1 is only ever set or
cleared on all 12 together. Who sets them:
- The player, through the fort's menu (0x417ff0, vtable +0x68), shown
  standing (`+0xa4` 1) on page 0 without a reload contact (`+0x104`):
  slot 0 Fire (command 3) with permission 64 (player `+0x4d` bit 0);
  slot 1 Guard (0x70, `CBUT_BEHFGUARD`, flag 0x20 while any group has
  bit 1) and slot 2 Fire on aggressor (0x73, `CBUT_BEHFAGR`, flag 0x20
  while `+0x498`) with permission 79 (`+0x4e` bit 7); each enabled while
  `+0x10c` (busy: mending) and `+0x1e4` (set only from a saved record,
  0x41a617; cleared by a reload, 0x4198c5) are 0 and some group has
  units; slot 3 the spell its store holds, enabled while `+0x10c` is 0
  with no permission test: the fort's Magic object (`+0x1b4`, made on
  message 2 when `+0x1b8`: `+0/+4` the fort's cell, `+8/+0xc` its nation
  and slot) keeps a count per spell index 0-13 at `+0x10`, rebuilt by
  0x419e00 from the store (each treasure of kind 9 adds one to its
  parameter's spell, but the Flame, index 8, which sets `+0x4a8`), and
  the button is the first index with a count through 0x42fd70 (12 and 13
  have no command there: the button is then the low byte of a pointer);
  slot 4 Repair 0x14 with permission 11 (player `+0x46` bit 4) while
  worn (vt+0x6c = 0x423be0: standing, `+0x10c` 0, `+0xa8 < 100`; enabled
  while `+0x1e4` is 0) or mending itself (job 0xff: enabled, its byte
  `100 - +0x120`, the share left), its right click sending order 10;
  then, but not while it mends (job 0xff skips it), 0x423a00: limed,
  every button off, and under construction slot 0 Break. Repair sends
  the fort order 0x1e {1, 0, 0} (0x4f34c0), which it turns into message
  0x1389 to itself (0x419f80): standing and idle it mends itself
  (0x423d60: job 0xff, `+0x114` its `+0xa8`, the start; the units noted,
  0x418300), paying as a settlement's mending does (0x425100); no worn
  test there. A spell button sets the pointer state 0x22 with its kind
  (0x55 0x11, 0x60 2, 0x61 1, 0x62 3, 0x63 0xd, 0x65 0xe, 0x66 8, 0x68 0xf,
  0x69 6, 0x6a 7; 0x56 opens page 4, empty for a fort, whose wind
  buttons would send kind 0xc), and the click sends order 0x22 {kind,
  cell, object}. The fort (0x419f80 case 0x22) takes kinds 1, 2, 3, 4, 6,
  7, 0xd and 0x11, and 8, 0xe, 0xf with a cell: while `+0x10c` is 0 and
  some count is not 0, a draw of its generator `+0x18`, then
  0x42fa90 -> Magic.cpp 0x42f580 with the spell 0x42fc80 gives (0x42ed40:
  a count of it, else its permission bit, the one 0x42fd70 gives, and
  the price at hand; 0x42f470 takes a count, else the price), and the
  first treasure of kind 9 for that spell leaves the store (0x424cf0),
  cast or not. The pointer first asks 0x42ef90 (jump table 0x42f418;
  distances from the fort's cell by 0x5576d8, the integer root of 0x557670,
  a Newton walk that rounds to the nearest): lightning (6) within 12,
  the phantom and shields (8, 0xe, 0xf) on an object of the fort's own
  player (`+0x57` 1, nation and slot) within 15, the view (0x11)
  anywhere; the fireball (7) within 12 with one of the cell's slots
  empty; reef (3) the cell's tile layers and occupants within 12;
  whirlpool (1) also asks 0x42ed40 for the big reef (12) and the reef;
  tornado (2) within 15 and 0x406830; monster (0xd) within 12 and the
  cell's tiles. In full: reef (3; big reef 4 after 0x42ed40(12);
  whirlpool 1 after 0x42ed40(2) and (12)) 0x42ed40(1), layer 0's tile set
  0 or 1 and layer 1's variant 0 (no shoal) and d <= 12, else no; then no
  if the occupant (`grid + 0xc`) is a nature object (`+0x57` 2) or
  0x4efe20(its player, the caster's) >= 0 (the same player, or at peace:
  -2 both 0, -1 / 1 one way, 2 both 1 in the table `+0x2f45`), or the
  third place `+0x1c` holds anything, or `+0x18` does. Monster: 0x42ed40(4),
  d <= 12, the layers so, `+0x18` empty (no occupant test). Fireball:
  0x42ed40(7), `+0x14` empty, d <= 12. Tornado: 0x42ed40(3), d < 16,
  0x406830(0, x, y): `+0x14` empty (or 0). No water test for the tornado.
  The slots of a cell: `+0xc` ships and buildings, `+0x10` skarb, amber,
  fish, leviathan, `+0x14` fireballs (0x4060ec), harpies and dragons
  (0x41ff74, 0x4208a5), tornadoes (0x43a9f6), `+0x18` the gorgon, `+0x1c`
  treasure, reef, whirlpool, flock, volcano. The port casts a fort's
  stored reef, whirlpool, monster, waterspout and fireball so, and puts
  the temples' casts of those through the same cell tests (their reach
  stays the temples'; the whirlpool's asking for the big reef and the
  reef is not applied to the player's temples: which Magic object and
  permission bits the player's pointer uses is not decoded). Not decoded:
  what else holds `+0x14` and `+0x18`; the siren's slot. The sea
  monster holds `+0x18` (the scan's cell `+0x10`: its moves 0x490d60 and
  0x491780 write it, as a short pointer `(cell + 1) x 12`, clearing the
  cell left; only 0x491ed0, its end, clears it otherwise), under water
  too. The left press (0x4f34c0): command 3 sets the pointer
  state 0x15, kind 7; 0x70 sends order 0x23 {sub 1 unticked or 2 ticked,
  0xffffffff, 0xffffffff, 0xff..}, 0x73 the same with sub 3 or 4; commands
  1 and 2 (Stop, Break) send order 10. The fort answers the pointer
  (message 0xc452, 0x41bec0): over itself 6, an own object 5, else, if
  standing, not limed (`+0x144`), `+0x1e4` 0 and the cell seen
  (0x50e870), an enemy (diplomacy 0x4efe20 < 0, or a creature class) some
  group with units reaches (0x446800): in state 0 the mask's bit 4
  (pointer 9, attack), in state 0x15 / 0x16 kind 7 pointer 9 (refused,
  bit 0x4000, when none reaches). The click (0x5180e0, pointer 9): state
  0x15 sends order 0x15 {target, kind (7), 0xff, -1, -1}, else 0x16
  {target, 7, ...}; the navy hands a selection that is not a fleet each
  order raw (0x49e230 -> 0x520cc0).
- Orders to the fort (0x419f80) / stronghold (0x40ef70): 0x15 / 0x16
  {target, groups}: byte `+4` 1 groups 0-2, 2 groups 3-5, 4 groups 6-8, 7
  all, 0x446960(target) (a cyclops' jam, 0x1f7, becomes its first
  member, 0x415df0: `+0x78`, the creatures being appended as they join,
  0x1879f, and closed up as they leave, 0x187a0, so the earliest still
  on the cell; fort only); the payload's `+5` (byte), `+6`, `+10` go to
  `+0x494`, `+0x48c` (less the fort's own player bit, `1 << (nation x 4
  + slot)`) and `+0x490`, whatever is found. 0x23 sub 1 every group at
  will, `+0x498 = 0`, the masks stored from `+1`, `+5`, `+9`; 2 and 4
  every group ceases fire, `+0x498 = 0`; 3 the masks stored, `+0x498 =
  1`, every group ceases fire; the stronghold takes sub 1 only. Order
  10: every group ceases fire, `+0x498 = 0`. The masks are read by the
  groups' shot test (group vtable 0x5959ec slot 0, 0x41c4e0, the fort at
  group `+0x46`): the target's `+0x57` 1 (a player's object: ships,
  players' buildings, barbarian structures: their constructor 0x40de20
  sets side 1, nation 4) with its player's bit in `+0x48c`, or 2 (the
  monsters' side, set by the constructors of the castles 0x410150
  (`CastleClassTy`, vtable 0x5949fc; 0x427f50 then makes a side-2 class
  0x42 / 0x43 nation 4, slot 1), the lairs 0x414e20, the jam 0x415c10, the medusa 0x401000,
  0x420fd0, 0x4056b0 and 0x447900), else nothing; its class bit in `+0x490`
  (0x50ed20 by 0x50e600's family: ship types bits 0-11, `1 << (class +
  0x38)` of classes 200-211; settlements and towns 0x1000, forts and
  barbarian structures 0x2000, ports, shipyards, docks 0x4000, workshops
  and smithies 0x8000, schools 0x10000, temples 0x20000, the cyclops' lair
  and creature (and the jam) 0x40000, the minotaur's 0x80000, the
  amazon's 0x100000, the sea monster 0x200000, the medusa 0x400000, the
  siren 0x800000, harpy and harpy nest 0x1000000, dragon and dragon nest
  0x2000000, the sea and war castles 0x4000000 / 0x8000000, anything else
  0); the group's kind in `+0x494` (0x447800 by its `+0x39`, item / 3: bit
  1 the marines, 2 the medium, 4 the strong weapons); then the score
  0x4472d0, `(reach + 1) x 10 - d x 10`. The target track 0x447400
  shoots only when it passes (else the scan, at will); the scan 0x446d60
  keeps only candidates that pass and takes the best score, ties by its
  argument modulo their count. A barbarian structure's groups (vtable
  0x5949e0, slot 0 0x40fd30, the structure at `+0x46`) test only side 1
  (a player's object) with its player's bit in `+0x1f8`, then the class
  in `+0x1fc` (0x50ed20), the kind in `+0x200` (0x447800) and the same
  score; the masks are all ones from the constructor (0x40de60), and
  orders 0x15 / 0x16 (0x40f75b: `+0x200` the byte `+5`, `+0x1f8` the dword
  `+6`, `+0x1fc` the dword `+10`) and 0x23 sub 1 (0x40f8e9: `+1`, `+5`,
  `+9`) store them as they come, its own player's bit kept.
  **The scan** (0x446d60, `this` the group, `param_1` the draw): the
  object held (`+0x20`) is looked for (0x51f1f0) and scored by slot 0 at
  the cell where it was found (`+0x28` / `+0x2c`), else cleared. Then the
  cells of the box `group cell ± reach` (the reach 0x4466c0: 0x598154 at
  `[level x 12 + item]`, the level the player's reach level `+0x95` for
  item / 3, `+0x39`; the group's cell `+0x14` / `+0x18` is the
  structure's `+0x74` / `+0x78`), row by row, on the map, where the reach
  mask is set (0x447840 by item `+0x38`, size 0x598130 by level and
  item); with the fog on (`[0x5ca546]` and DarkTy `+0x18c` 0) a cell
  whose explored layer of the structure's player (`[0x5c74a0] + 0x24 + 16
  p`, allocated for each player present, 0x48bbd0) is 0 is skipped. A
  cell gives at most one candidate: the occupant slot (`+4`) if there
  (vt+0x2c) and of another side (`+0x57`) or an enemy (0x4efe20 < 0);
  else the slot `+0x10` if there and the medusa (0x1fa), the siren
  (0x1fd) or an enemy; else the slot `+0xc` if there and a harpy or dragon
  (0x1f8, 0x1f9); then it is dropped if a ship (type 0x1488) whose
  reserved cell `+0x5ab` this is, a medusa with `+0x110` 0, or a building
  0x32-0x3e in state `+0xa4` 2. The object held keeps its score; any
  other needs a non-zero slot 0. Up to 150 (0x5c6a48-0x5c73ac, 16 bytes
  each). The best score wins, compared unsigned; if the object held is
  among them and the best is not above its score, it stays (cell not
  updated). Several at the best: those below are removed, the order
  kept, and entry `param_1 % count` wins; one: the first at the best.
  Nothing: target and object cleared. 0x4469c0 (per group, while its mode
  is not 0, its unit count `+0x3b` is not 0 and `now > +0x1c + reload +
  (±param_3) % (reload x 20 / 100 + 1)` compared unsigned, the draw
  positive when odd and negated when even (C's remainder: 0 or below),
  reload `0x5c76e0[item x 3 + rate level]` = 6000 / 0x597a3c's rate
  (0x4ac0e0), the rate level the player's byte `+0x8e + item / 3`;
  passing, `+0x1c = now` before any target is sought, and the group's
  units fire as one blow, 0x14ba with 0x446710's strength): bit 2 and the track
  passing, the target; bit 2 failing with bit 1, a scan into locals (not
  kept); bit 1 alone, a scan into `+0x20`..`+0x2c` (kept); a jam found is
  shot through its first member (0x415df0). The callers (0x41a96f fort,
  0x40f61d stronghold), each tick in state `+0xa4` 1 unlimed (`+0x144`
  0; the fort also `+0x1e4` 0), draw for each of the twelve groups from
  the unit list's dword 0 (0 when made, 0x41a204 / 0x40f100; then a
  draw of the building generator at message 2, `r >> 16`, 0x41a895 /
  0x40f5a5, but for a save's record type 2): `s =
  0xa70427df - s x 0x7f82434b`, passing `s >> 16` as `param_1`,
  `param_2` and `param_3` (the fort `+0x4a8` as `param_4`, the
  stronghold 0). The track 0x447400 walks the same cells and stops at
  the first holding the target in any slot (or a jam holding it,
  0x415e00), returning slot 0's verdict there. The player's orders pass everything: 0x15
  / 0x16 from 0x5180e0 {.., 0xff, -1, -1}, 0x23 from 0x4f34c0 players -1,
  classes -1, kinds 0xffff; only the navy's 0x4c5b30 (fleet `+0x19`,
  `+0x1d`, `+0x21`) sends others.
- Firing on aggressors (`+0x498`): each blow 0x138e but kind 9 (0x4184f0
  at 0x418bdd, after the units' share) with a shooter (`+0x16`, the
  bomb's `+0xbe`) sets the groups of the mask its bomb kind `+4` gives
  (jump tables 0x418e40, 0x418e74: 1 none, 2 group 0, 4 group 1, 8
  groups 0-1, 0x10 group 2, 0x20 groups 0 and 2, 0x40 groups 1-2, 0x80
  groups 0-2, 0x100 group 3, any other groups 0-8) at the shooter. The
  stare (0x158bf, 0x41a041) has no shooter. The stronghold has none.
- Message 10000 to the fort's vtable +4 (0x418f60): standing, every group
  at will. Nothing sends it: no instruction of the exe builds 0x2710 as a
  message (every immediate 0x2710 is a clamp, a price, a timeout or a
  divisor; 0x2700-0x270f and 0x2711-0x271f occur nowhere), so it is dead.
- Order 10 also comes from the land fleet's repairs (order 96, part
  0x3ec; the brains' builders and the computer players' towns): under
  way (0x42a2f0) it waits until the building mended has life `+0xa8`
  100, then sends it order 10 (0x42a384 for a town's building `+0xdc`,
  0x42a3dd for a fort 0x1494 or stronghold 0x1a6b mended itself), so a
  fort or stronghold so mended ceases fire for good; and from 0x427f50
  on message 0x2874 while it is busy. A settlement or town falling (its
  0x138b in state other than 2, 0x43bc30) sends 0x2874 {its id} to each
  building of its list `+0x1d0` (`+0x1c8` of them; a building joins with
  0x2490, which refuses nation 4: barbarian structures never do), and
  each, in 0x427f50 when the id is its `+0x1c8`: clears `+0x1c8`; busy
  (`+0x10c`), sends itself order 10 (vt+0 with 0x280f, order byte 10:
  its whole order-10 chain, so a fort mending itself stops and ceases
  fire); with a reload contact `+0x104` sends 0x2876 {`+0x108`} and
  clears both; sets `+0xf8`; then LandObj 0x425990 as for any message.
  A yard or dock (ShipService 0x4e7640) first sends 0xcb9 to the ship
  in it, `+0x1e0`, and clears it (see the repair order's machine). Nothing in the land fleet
  sends 0x23, 0x15 or 0x16 (its 0x520cc0 codes: 0x1e, 0x20, 0x21, 0x22,
  0x28, 0x2d and 10). The navy's 0x4c5b30 (0x4a1150, a new group of a
  computer player, side 3 or 4) sends 0x23 sub 1 with the fleet's masks
  to every member of the group; 0x4c4180 sends 10 to ships only.

The port keeps the fire in `combat::FortFire` (at will, a target per
group, the aggressor flag): `Scene::fort_attack` (0x15 / 0x16),
`fort_order` (0x23), `fort_aggressed` (0x4184f0), order 10 in `halt`, the
masks in `combat::FortMasks` (a fort's and a stronghold's), the groups'
shot test and score in `group_score`, the scan in `group_scan` (the
cells `group_cells`: ships, buildings on each cell of their footprint
and the land creatures as their jam, class 0x1f7 and side 2, in the
occupant slot; medusa, siren and sea monster in `+0x10`; harpies and
dragons in `+0xc`), the track in `group_track`, the generator's draws
(`FortFire::seed`, `draws`, `step_fort_draws` in the tick) and the kept
scan result (`picks`, `pick_cells`); the gate per group
(`combat::group_loaded`, `FortFire::last` its `+0x1c`, saved), all its
units firing together when it finds a target; the 0x2874
path in `Scene::town_gone` (the buildings of its list, see "A town's
buildings"; busy, `Scene::busy`, they are halted; their reload contacts
end); its
fort menu has slots 0-4 and its pointer the fort's answer (`fort_reaches`:
a unit's reach); `scene/fort.rs` has its Repair (`fort_repair`, the
progress `fort_mend_progress`) and its stored spell (`fort_spells`,
`fort_spell`, `fort_cast`). The computers' and the brains' repair parts
under way (0x42a2f0) wait for the building whole, counting no passes,
and halt it. Not ported: a
fort's casts of the sea's spells and the fireball (their checks read the
cell's tile layers and slots, not decoded here: the button is offered,
the click refused), the draw of the fort's generator `+0x18` with a cast,
the jam (the port has none: a click gives the creature itself; the scan
takes the port's first creature on the cell, not the first to join),
0x4469c0's reload test with the draw (the port keeps its own reload per
unit), the fog test of computer players' structures (the port keeps the
human's explored layer only: theirs scan every cell), the side `+0x57`
of the ships a barbarian structure builds (0x4e2117 copies it from the
creation record's `+0x1f`; not decoded, so the scan keeps the
structure's own player's objects out), the
state 2 of buildings 0x34-0x3e (the scan skips classes 0x32-0x3e in
`+0xa4` 2, fallen: the port's ruins, `Scene::fallen`; a settlement
mending its stock is in state 1, its job `+0x110` 2, and is scanned), a ship's reserved cell (the port's ships
hold only the cell they are in), a fort's 0xcb9 on 0x2874
(its `+0x1e0` not decoded; yards' and docks' are ported), whether a navy group of a computer ever holds a fort (the
port's flotillas hold ships), 0x43d300's test that the building is still
the town's, the other 0x138e senders' `+0x16`. The brains' under-way
passes count from the second pass after the one that started the
mending (the original counts those after a pass that started nothing,
`+0x18`; the port's builder land fleet starts nothing else: its ship
parts and the pass after a start, run once woken and 3 ticks later, are
not modelled).

Known differences not yet ported:

- Start reports: the port writes its own record after "RP" and the
  difficulty (see Start reports), not the original's 0x151 bytes.
- Custom missions' list: `<MainPath>CUSTOM\*.DKD` (0x52b7c0, watched
  with FindFirstChangeNotification), a file kept when its `HEAD_CUST` a
  is set; the port lists the installation's battle and single missions
  with a set (all of the CD's), not a `CUSTOM` folder.

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
  (rows x 115 / 100 and x 130 / 100 for rigging 1 and 2, by the player
  block's `+0x51`), acceleration = 0x5bb510[type] x 8, in units of 1/6000
  pixel a tick. Measured live: a boat 60000 / 1200, a merchant 56000 /
  1120; a merchant at 33600 made 4.8 pixels a tick.
- `+0x59f` (0x4bece0, from the stats refresh 0x4bee60): `d = 100 - hull x
  100 / full` (0x5c76b0), top less `top x d / 200`; `+0x280` the same
  from the acceleration, then x 85 / 100 or x 65 / 100 for armour
  (`+0x27a` 1 / 2). The armour's 95 / 90 percent and the feathers of
  Boreas (`+0x489` kind 12, their amount in percent) go only on `+0x27c`,
  which nothing reads for a ship: they do not change the sailing speed.
  0x4a3a70 sets `+0x280` back to the plain 0x5bb510 x 8 on every step it
  grants. Under a siren's song 0x4a74c0 sets `+0x59f` to 7000.
- The move tick 0x4a3f40 (states 4, 5, 1, and 3 once 0x4a3560 grants the
  step; 0 halves the way each rocking frame, 0x4a3150; 2 turns, the way
  kept): angle = 0x4e0100(wind, heading), `W` = 0x597b2c[((type <= 1) x 11
  + strength) x 5 + angle] (the rows are the same); with Zetes aboard
  (0x4ba790 with 3) neither the cap nor the step is scaled. `v = W x a /
  100 + +0x59b` (`a` = `+0x284` in state 4, else `+0x280`); state 4 and `v
  < 1`: 0, `+0x284` 0; capped at `W x +0x59f / 100`; limed (`+0x258`) `v`
  and the step halved; braking (step < 0) to a tenth of `+0x59f` (unscaled)
  or under keeps the old way. Then `n = v + +0x597`, `+0x597 = n % 3000`,
  `s = n / 3000` steps along the heading (0x5c4f4c: dx, dy shorts, 5-byte
  rows): straight on `s x d / 2` pixels on its axis and the odd step's 3000
  back into `+0x597`; slanting `s x d / 3` on both axes and `(s % 3) x
  3000` back. The picture is held at the route point on the axes it moves
  along (moving along x only, its y set to the point's).
- After the step, with the cell of the picture's centre *before* it: just
  entered (`+0x596` 1 and that cell `+0x5b6`): `+0x596` 0, the move goal
  posts 0x136, the route's end cell brakes, else the next point's cell is
  asked for (0x4a3a70 with index + 1, held in `+0x5ab`), and refused it
  brakes; not between cells and holding none, it asks every tick (and the
  goal 1 away posts 0x136). At the route point: `+0x596` 1; state 4 in the
  route's end: state 0, way 0; else the next point, state 3.
- The brake 0x4a2f60 (not in state 4): the distance the larger axis from
  the centre to its own cell's middle, plus 64 when `+0x5b6` is its own
  cell; between cells (another `+0x5b6`) it reads `+0x5b6`'s cell number
  (y x width + x) x 64 + 32 for both axes (0x4a2fff); state 4, `+0x284 =
  way x way / (distance x -6000)` (32 bits).
- So, calm, from rest at a cell's middle: a boat is in the next cell's
  middle east in 35 ticks, north-east in 44 (braking by -540 / -854); a
  merchant in 37 and 46. The port's mover (`scene::sail`, `Scene::update`)
  steps the way and the picture so a game tick at a time, with the look-
  ahead cell reserved (`Ship::ahead`).
- `[clock+0xe2]`, the ticks the update runs, is 1 in single player: the
  host path (0x5203c2-0x5203fc) takes `(now - +0xe6) / period`, runs no
  tick at 0 and sets 1 above it (0x5203f2), then 0 after the tick
  (0x520432); only the replay and network paths (0x52139e, 0x52146f,
  0x5214ef, 0x522348, 0x522845, 0x522883) take it from the tick buffer's
  byte (1 locally, a peer's `+0x4e`).
- The stats refresh 0x4bee60 -> 0x4bece0 sets `+0x280` by the hull and
  armour (above); 0x4a3a70 sets it back to the plain one on every step it
  granted, so the lowered one steps the way only between a refresh under
  way and the next step (from rest, 0x4a3560 grants before the first
  move). Refreshes: a blow survived (0x4befc0 via 0x4bee60 after the hull,
  crew and weapons parts; not for a ship being built; 0x4c1ad0 a ram's),
  the end of a status (0x4df320 from the tick at 0x4defc1, when `now -
  +0xb1 >= +0xb5`: commands back 0x4b3850, 0x4bece0, `+0xa9..+0xcc`
  cleared, `+0x25c = 0`), the hull mended aboard (0x4b98b0), mending at a
  yard (0x4b1fc0), loading (0x4bc8c0, 0x4bcd10, 0x4bd0c0, 0x4bd350,
  0x4bd410), boarding (0x4b5df0, 0x4c3410), refits (0x4e72e0, 0x4e73c0),
  being built (0x4b3ab0), the loader 0x4dfa90 and message 0x8d1 at the
  start (0x4dc630). The port refreshes (`Scene::refresh_stats`, `Ship::
  accel`) after a shared blow and a ram's, when the siren's song ends, and
  on mending aboard (0x4b98b0) and at a dock (0x4b1fc0).
- The siren's song is such a status (0x4a7390: `+0x25c = 1`, `+0xb1` now,
  `+0xb5` the length): 0x4a74c0 sets `+0x59f = 7000` every tick before the
  move tick, so a refresh during the song changes nothing, and its end
  (0x4df320) refreshes `+0x59f` to the hull's: the 7000 does not outlast
  the song.
- A step refused (0x4a3a70, asked at a route point or by the look-ahead
  alike) counts the wait `+0x591` (to 150): a tick when the cell holds a
  ship lying still (state 0; first, the same player's of lower priority
  `+0x5c7` is pushed aside, 0x4b4b50, when the wait is over the patience
  `+0x5c8`, which then triples under 200, the wait from 0), stepping to it
  (3) or held (6), or holds a non-ship object; none for a ship under way;
  then a tick more when the cell is not the route's last (or `+0x594`). A
  step granted sets it to 0 (0x4a3c62).
- Not ported: the closing-in branches of the tick (`+0x25d & 0x10`, the
  port's grapple); the refreshes under way other than blows and the song's
  end, the mending aboard (0x4b98b0, see "Healing aboard") and at a dock
  (0x4b1fc0); 0x4a6330's 0x4a3a70(1) when planning between
  cells.
- The shore and pier slide: 0x4b02a0 steps `+0x78e` by 2 (back out:
  down by 2 while above 1, else to 0) along the line 0x4b5220 built from
  the picture's top-left (Bresenham, an odd count padded with the last
  point), stamps `+0x79f` = now (read only by 0x4adef0's own states), sets
  the picture to that point while under the count (backing out, done at
  index 0 with the picture there), and at or past the count leaves the
  index at count - 1 and the picture where it was (one point short of the
  end when the line's length is odd), done once `+0x79d`'s turn is over.
  Its only caller is 0x4b0470 (from case 10 of 0x4adef0, 0x4b0590 and
  0x4b0e40), none of which tests the clock, and the dispatch 0x4ac2d0
  (0x4ac34b-0x4ac41b by `+0x76e` and `+0x772`) runs from the ship tick
  0x4ab930 at 0x4abb24 whenever `+0x25d & 0x40`, also without a clock
  test: **two points a game tick**. So a backing out from a finished
  slide first steps one point (count - 1 to count - 3), then two, its
  last step one or two. The port (`Ship::slide_end`, the slide in
  `Scene::update`) steps the straight close-ins (`approach`: across or up
  and down, so no minor axis) two pixels a tick (a slanting one would
  take its minor axis in proportion, not by 0x4b5220's rounding).

- Construction: a building's construction picture plays once over the
  first entry of its class's cycle table (+0xc0, set by each constructor:
  200 ticks for every player building; 300 at 0x5949e8 and 50 at
  0x595e80 for two other classes). 0x422e30 (state 0) sets the frame
  delay `+0x7c->+0x7d = cycle[0] / n` (integer, `n` the picture's frames
  `+0x49`); the tick's 0x4230f0 steps the frame when `now >= last +
  delay` or on the first tick (`+0xbc` clear), then, the frame `f` still
  under `n`, 0x423410: condition `+0xa8 = f x 100 / n`, hit points `+0xac
  = +0xb0 x +0xa8 / 100` (integers); the step reaching `n` stands it
  (`+0xa8 = 100`, state 1; `+0xac` is left at the last step's) and plays
  the finished call for the viewing player's own building (events
  0xbc / 0xbd / 0xbf / 0xbe by nation 0-3: SND_189 / SND_190 `bi_c1a`,
  `bi_c2a`, `bi_c4a`, `bi_c5a`, 188 and 191 empty). It stands after 1 +
  (n - 1) x (200 / n) ticks. An upgrade (TownObj 0x43b290 / Dock 0x416340,
  message 0x138a) costs `0x42d8b0(to) + 0x42d8b0(from) x (100 -
  condition) / 100` (the town's mending part only when under 100) and
  steps the same picture. The port keeps one hit-point figure and makes
  it whole when the building stands. (Not ported: BuildingTy's frame step
  0x427b30, which steps the frame down when the settlement's cell no
  longer holds the settlement, removing the building at frame 0.)
- The sage school (0x432210): arming 0x5971b8[+0xa3] = 450 ticks; ram,
  armour and rigging 600 ticks for the first level, 700 for the second;
  a unique ship 400 (0x432210 compares the elapsed ticks with 399).
- The workshop's studies (0x436ee0): the second half of each 18-entry
  cost table is the study time, by group x 3 + level (0x5971e8 rate of
  fire 200, 250/290; 0x597230 range 150/285, 270/360, 240/420; 0x597278
  damage 100/125, 100/170, 170/150). There are no fish costs.
- Marines (settlement) and weapons (workshop): 0x5971a0[item / 3] ticks
  each, 70, 80, 95.
- Ships rise on the yard's berth, 39 or 40 steps of 8 ticks (below).

## Shipyards' berths

A yard (DockClassTy, classes 0x3a and 0x3b) has one berth (vtable +0xb8,
0x40e050): its anchor cell (`+0x74`, `+0x78`, record fields 5 and 6) a step
toward its facing (`+0x1dc`, record field 10, read when the class flag 8 is
set) by the table at 0x5c4f4c (5 bytes an entry, `{i16 dx, i16 dy, u8}`):
(1,0) (1,-1) (0,-1) (-1,-1) (-1,0) (-1,1) (0,1) (1,1), the ninth (0,0); a
facing below 0 gives (-1, -1), whose grid index nothing guards.
The berth is free (0x4e8250 = 0x499a40 with flag 0) when the cell's
landscape dword (`grid + 8`) has its low nibble below 8 and nibble 1 not
4, 5 or 6, and its object slot (`grid + 0xc`) is empty; a ship holds the
slot of the cell it is in (0x499800 takes an empty slot, 0x4998a0 /
0x499860 / 0x4a3970 clear their own; a ship mended in a dock is off its
slot). **0x4e82f0(yard, 1, code)**: -5 unless the yard is idle (`+0xa4 ==
1`: not going up, not being made a dock); for codes 0xc-0x17 the draught
test 0x4e8290: royal ships, bombers and rammers (0x597ce4 = 0 0 1 1 0 1 0 0
0 0 0 0, the ship's `+0x592`) get -5 when the berth's nibble 1 is 3; then 0
with the berth free, else -1. Its code match on `+0x10c` / `+0x110 == 1`
/ `+0x120 == 0` / `+0x114` (a free berth's 0 turned into -1 for any other
code) reads the buildings' job block (LandObj, every building: busy,
job, parameter, amber paid, progress); a yard's only job is its mending,
job 0xff (0x424626-0x42463c; every other writer of `+0x110` is another
class's method on its own `this`), so the match never applies and a yard
being mended still builds. Mode 2 (the upgrade to a dock, 0x43f9ec,
0x443654): -5 not idle, -6 under 100 condition, -1 with no town, `+0x1e4`
0, busy (being mended) or `+0x104`; else 0.

**An order** (0x416a40(yard, payload, pay): message 0x280f sub-command
0x20 with pay = byte 0xf != 0xff, or the queue with pay 0) needs code byte
0xc 0xc-0x17 or 0x25 (the Argo). With 0x4e82f0(yard, 1, 0xff) == 0 the
ship is **built now**: a 600-byte record (+0x10 = 2, nation, slot, kind by
code: 0xc 0, 0xd 1, 0x13 2, 0x12 3, 0x10 4, 0x11 5, 0xf 6, 0xe 7, 0x14-0x17
8-11, 0x25 2 with +0x1f0 = 1; +0x24/+0x28 the berth; heading +0x2c 6 for a
yard facing 2 or 6, else 0; +0x1f4 the yard; +0x1f8 payload dword 2), the
yard's town (`+0x1c8`, 0x51f1f0) must exist, the price 0x43ead0(town, rec,
&amber, &fish, byte 0xd); with pay it must be affordable and is taken
(amber, fish x 100); code 0x25 clears permission 80 (0x4a9810); `+0x34 =
amber << 16 | fish`; message 0x1488 to the world makes the ship; the
yard's call, events 228, 229, 231, 230 by nation (0xe4 + 0, 1, 3, 2; only
`SND_229` and `SND_230` hold sounds, `re_o2a`, `re_o4a`, `re_o6a`), only
for the viewing player's yard and only with pay. Otherwise the order is
**queued**: without a town nothing happens (the order is lost); else a
24-byte entry (the payload and the price of 0x43e5c0) is appended only if
the price can be paid, and it is then taken **whatever pay** (a free
order that queues is charged); no sound; `+0x1fc = now`. 0x43ead0 and
0x43e5c0 give the same price (dumped tables: hull 0x5961d8 / 0x596208,
items 0x5964d8 with fallbacks 0x5962f8, minimum strength 0x597ae4, armour
0x596268 / 0x596238, ram 0x5962c8 / 0x596298): each item from stock (free),
else bought if 0x43da20 case 1 allows it, else its fallback from stock or
**bought untested**; more fallbacks from the last (never index 0) while
under strength; armour: from the town's armour list (`+0x248`), else
bought if case 2 allows it, else for level 2 with no fallback level 1 by
the same test, else the fallback **bought untested**; ram the same (case
3, the list `+0x24c`, 3 = none). 0x43e5c0 only counts; 0x43ead0 sorts the
town's lists (best condition first), **takes** what it counts from the
stock and fills the record (bought items at 100), and 0x416a40 calls it
before its funds test, so a paid order refused for its price has lost
them. A queued entry keeps only its payload: it is armed when built, by
0x43ead0 from the stock then, the bought items free (pay 0), `+0x544` that
price. Both branches
redraw the yard (`+0xfc`, `+0x100`). The ship (0x4dc630 message 2 with
record +0x10 == 2, 0x4dfe00): owner, kind, heading, cell the berth, `+0x544`
the price, `+0x259` the Argo, `+0x261 = 0x2000`, its arming, `+0x251 = 2`,
task `+0x72f = 4`, `+0x73a = 1`, `+0x75a` the yard, frame 0 (heading 0) or
40 (heading 6); then 0x499a40(berth, draught): it takes the slot, sound
event 1 (`bildsh3`, every player); failing (the Argo on a nibble-3 berth)
it is removed, its price not given back.

**The build** (0x4b3ab0, the ship tick 0x4ab930 runs only task 4, or the
sale's task 5, while `+0x251 == 2`: no firing, moving or events): every 8
ticks (`now - +0x57b < 8` returns; `+0x57b` is 0 from the constructor
0x4dc3f0, which zeroes `+0x54c..+0x5d1`, so the first step is the ship's
first tick): the yard looked for at the berth (0x49a4b0; gone: `+0x73a =
0xff`, return); hull `+0x271 += H / 40` (H = 0x5c76b0[kind] = 0x5bb4b0 x
650: 18200 58500 65000 78000 42250 45500 23400 23400 42250 48750 71500
78000), capped; frame = base + (hull + 1) x 40 / H; shown % = hull x 100 /
H; done when frame >= base + 39 (heading 0) or >= 79 (heading 6; any other
heading never finishes). In integers that is **39 steps for kinds 0, 2, 3,
6, 7, 11 and 40 for 1, 4, 5, 8, 9, 10: 1 + 38 x 8 = 305 or 313 ticks** from
the first step. Done (the yard looked for again): hull H, the kind's slots
filled (0x4bd920: kind 0 slot 3, 1 slot 4, 8 slot 9, 9 slot 10, 11 slots
0-2; not decoded), the picture heading x 4, `+0x544 = 0`, `+0x251 = 0`,
capabilities `0x4e6930` (0xfffff7ff for kinds 0/1, else 0xfffffeff), the
launch voice for the viewing player's ship (events 184, 185, 187, 186 by
nation: `SND_184` empty, 185 and 186 `jb_d1`, 187 `121~2`), 0x4b3a20
(message 0x5d95, the brains' "ship built"), for a human's ship (side
`game +0x1a1d + player` 1 local or 2 remote, 0x4b3fbe; computers are 3
and 4, player 16 a computer or 0) 0x4e7220, then **0x4b46f0(ship, 'e')**.
Blows on it (0x4ab720 runs the damage events whatever `+0x251`; event 1
appends `{shot, id}` while fewer than 4, a fifth in a tick dropped; with
some pending, `+0x25d & 1`, 0x4ab930 runs 0x4bf170, which for `+0x251 ==
2` is 0x4b4000 alone): unless being sold (task 5), each entry whose shot
is still there with its id, letter not 9 and blow non-zero takes
`trunc(0x597fd8[letter] x blow / 100)` off the hull (0x597fd8, dumped: 0 0
0 100 100 100 100 100 100 0 100 0 100 0 100 100 100 100 100); below 0 it
is 0 and the rest are skipped; at 0 or less `+0x251 = 4`, `+0x7f = 0xff`,
`+0x73a = 0xff`: it sinks as any ship (0x4dec40: reported with -1, the
owner's ship lost, the slot and berth freed, the sinking picture and
sound, removed), **no refund** of `+0x544`; no fire, lime, crew or weapon
damage. Code-4 blows (0x4befc0): `hull -= blow`, below 1 sunk, `+0x7f` the
blow's player; wrecks (event 0x200: lightning, reef, whirlpool) sink it at
once, killer none. Orders while built: 0x4e2ba0 tests each command with
0x4e6700 against `+0x261 = 0x2000`, so moves, fire, board, ram, gather,
dock, transfer, found and treasure are dropped (nothing kept); the stop,
spells (0x22 other subs), behaviours (0x23, and 0x4e7220's: 0x100 records
of kinds 4-11 applied at once by 0x4a9e30), 0x28 sub 4, 0x29 sub 2, 0x2a
pass; events sent straight to it (the town's berth clearing 0x4415a0 ->
0x4b4b50, event 0x80) stay pending, one record a code, until the launch,
where 0x4b46f0 'e' overwrites a pending 0x80 unless it finds no cell.
The yard looked for (0x49a4b0(cell, id), the ship's cell and `+0x75a`):
the berth's slot object if its class `+0x50` is the id (the slot holds the
ship itself, `+0x50 = 200 + kind`: a yard whose id is that passes even
gone), else the first in-map neighbour, in 0x499ff0's order by 0x49a1d0's
position class (corners 3: (0,0) +1, +W+1, +W; (W-1,0) +W, +W-1, -1;
(W-1,H-1) -1, -W-1, -W; (0,H-1) -W, -W+1, +1; sides 5: top +1, +W+1, +W,
+W-1, -1; right +W, +W-1, -1, -W-1, -W; bottom -1, -W-1, -W, -W+1, +1; left
-W, -W+1, +1, +W+1, +W; inside 8: -W-1, -W, -W+1, +1, +W+1, +W, +W-1,
-1; counts 0x597900), whose slot object's id `+0x4c` is it; else none.
0x4e7220 at the launch (a human's ship, side 1 or 2): on guard, and ram attack for
kinds 5 / 10, board attack for 4 / 11.
Called off (`+0x73a < 0`: the stop order, event 8 with no record while
`+0x251 == 2`, 0x4aa540 at 0x4aa565; the yard gone): the frame drops by 1 a step; below 1 (heading 0) or
below 40 (heading 6) the price `+0x544` is given back (amber, fish x 100,
whether or not 0x416a40 took it), the owner's ship lost counted
(0x50e750 with -1), the slot cleared, the Argo's permission 80 given back,
the ship removed.

**The queue** (0x416f90 message 0): with the yard idle, entries waiting
and `now >= +0x1fc + 25`: `+0x1fc = now`; with its town: the head entry's
0x43da20(town, 4, 1, 1, kind, 0, byte 0xd, 1, 1) (price skipped): 0 and
0x4e82f0(yard, 1, code) == 0 -> 0x416a40(yard, entry, 0) (built free, no
call), removed, and the pass ends (one ship per 25 ticks); 0 with this yard
refusing, or -1 etc.: wait; -5 or -7 (not allowed: permission or
buildings -5, the unique ships' study or kinds 3/5's player flags -7; or
no yard of the town answering 0 or -1) -> its price given back, removed,
the next entry tried. 0x43da20 case 4 runs, in order: permission (-5),
kinds 3, 5 and 8-11 (-7), buildings 0x5961a8 against town `+0x238` (-5),
the arming's items with param_6, then with param_3 the town idle (-1), the
price unless param_7 (-2 amber, -3 fish), then with param_8 the yard test
over the town's buildings `+0x1d0`: the first class 0x149c answering 0
gives 0, else -1 if one answered -1, else -5. The '<' sub-command takes
the last queued entry of a kind back, refunded. The buttons (0x4164e0)
count the queue by kind and test the price, not the berth; the Argo's
(0x7d) also needs 0x4e82f0(yard, 1, 0xff) == 0.

**Off the berth, 0x4b46f0(ship, prio)** (the end of a build with 'e' =
101; a merchant leaving a port's berth with '[' = 91): the window from one
before the ship's cell, clipped at 0, three cells on, clipped at the map's
edge (so at x = 0 it covers columns 0-2), rows from the top, its own cell
left out; the direction of each cell 0x59791d[dx x 4 + dy] (0..7 as the
headings, 0xff for dx or dy = 2). Pass 1: cells passing 0x499aa0. None:
pass 2, cells whose slot holds a ship (category 200, not a monster but
class 0x1fb) of the same nation and slot or allied (0x4efe20 >= 0) with
`+0x5c7` no higher than the ship's; after each row, prio + 1 if any was
found so far. One draw of the ship's LCG (`+0x18`, seeded `game +0x2ee5 x
id`) picks the cell; event 0x80 {x, y, prio at +0x19}. **0x4b4b50(ship,
prio, via)** (the town's berth clearing with 100; a merchant waiting 300
ticks by a taken port berth with 0x5a): one pass over the same window:
0x499aa0 -> list B, else 0x499990(cell, 0) and a ship of the same
`+0x24f` in the slot with `+0x5c7 < prio` -> list A; B drawn, else A, else
nothing; via sends it as command 0x14. **0x499aa0(ship, cell, dir,
draught)**: the landscape as 0x499a40 with the draught's nibble-3 test;
the slot empty or the ship's own; an even dir passes; a slanting one looks
at the two corner cells (dir 1: x = 0 passes, corners cell-1 and cell+W,
cell+W past the map passes; 3: cell+W past the map or x = W-1 pass,
corners cell+W, cell+1; 5: cell-W < 0 or x = W-1 pass, corners cell+1,
cell-W; 7: cell-W < 0 or x = 0 pass, corners cell-W, cell-1; any other
dir: cell-1 < 0 or cell+W past the map pass, corners cell-1, cell+W): it
fails if the first corner's ship lies in state 6 with a ship contact
(`+0x8e6`) whose cell is the second corner, or if one object fills both
corners (two different objects pass). **0x499990(cell, flag)**: the
landscape only (no slot), nibble 1's 3 refused with the flag; 0x4999e0 on
(x, y); 0x499930 / 0x499a40 with the slot empty; 0x4998d0 / 0x499900 the
slot empty only.

**The move event 0x80** (0x4a9e30 copies the 26 bytes to `+0x858`, `+0x7d6
|= 0x80`): 0x4ab720 runs the 0x1f0 group only while `+0x251` is not 1 or 2
and `+0x261 & 0x1f0`, keeping them pending otherwise (unless `+0x864 ==
0x17` with `+0x25c`), in the order 0x20 (0x4aa990), 0x40 (0x4aab00), 0x10
(0x4aa840), 0x80 (0x4ab250), each also needing its bit in `+0x7da`; after
each of the first four that runs, `+0x7d6 &= ~m` with `m = 0x1f0` when
0x100 is pending and 0x4ab350 passes, else `0xf0` (disassembly
0x4ab821-0x4ab83b): one a tick, in that order, the others dropped. 0x4ab250 drops
the event without capability `+0x261 & 0x400` (every finished ship has it;
one being built, 0x2000, lacks it) unless `+0x864 == 0x17`; else it zeroes
the route block `+0x562`, copies the target and kind, `+0x57b = 0`,
`+0x57f = 25` (re-plan period), `+0x593 = 1`, `+0x591 = 0`, the priority
`+0x5c7` from +0x19 unless 0xff, `+0x5c8 = 5`, `+0x25d |= 0x80` (0x4a5f30
then plans and follows the route).

**One event a tick** (0x4ab720, disassembly 0x4ab720-0x4ab925): nothing
with `+0x7d6 == 0` and `+0x265 == 0`; 0x200 (wreck: lightning 0xda93 /
0xda94, reef, whirlpool) with `+0x7da & 1` -> 0x4aa280; return with
`+0x251 == 4`; bits 4, 1, 2 (0x4aa290, 0x4aa2a0, 0x4aa2c0) and 8 (stop /
cancel by mask, 0x4aa540: clears `+0x265` and the pending group, `&=
0xfffffe07`); then the 0x1f0 group as above, after 0x4abe50 and, with
`+0x25d & 0x100` and `+0x251 == 0`, 0x4abe00: the first pending of 0x20
(0x4aa990: dock 0x101 / found 0x102; commands 0x1e, 0x20), 0x40
(0x4aab00: gather 0x19-0x1d, treasure 0x22 sub 0x10, unload 0x1f sub 1,
and 0x4b0590 / 0x4ad510 / 0x4adef0's re-sends), 0x10 (0x4aa840: board
0x17, ram 0x18, load 0x1f sub 2), 0x80 (0x4ab250: move 0x14, 0x2a sub 1,
follow, 0x4b46f0, 0x4b4b50) runs and the others are dropped with it,
even when it refuses; then 0x100 (fire records of commands 0x15 / 0x16;
behaviours 4-11 are applied at once by 0x4a9e30) if still pending. Each
code has one record (0x10 `+0x81c`, 0x20 `+0x824`, 0x40 `+0x839`, 0x80
`+0x858`, 0x100 `+0x872 + 0x13k`): a second event of a code before the
tick overwrites the first. Events run at the ship's next tick, not when
sent; a stop and an order in one tick leave the ship stopped.

The fire records (0x4e2ba0 case 0x15 / 0x16: kind 1 for 0x15, 3 for
0x16, the target `+0`, sub 1 / 2 / 4 a record for position 0 / 3 / 6,
sub 7 all three and, for ship types 8 and 9, 9 or 10; byte `+0x11` the
position, `+0x12` the priority) are kept by position, `+0x872 + 0x13k`
for k = position / 3. 0x4ab350 (disassembly 0x4ab350-0x4ab3ee) walks k
from the last (2, or 3 for types 8 and 9) down to 0 and answers, for the
first record of a kind with a weapon at its position (`+0x289 + 3k`,
`+0x28a + 3k`, `+0x28b + 3k` not all 0), whether that kind is 3; none:
no. So 0x100 is dropped with the group's order of the tick when that
record is an attack (0x16), and runs after it (0x4ab3f0) when it is a
fire at a target (0x15, kind 1); an attack on a ship from the interface
or a fleet (0x16 sub 7, 0x4cfc90) is kind 3 at every position, so it
goes with any order of the group in its tick. 0x4ab3f0 then, per
record: kind 5 clears `+0x684..`; else with a weapon at the position (or
k = 3): kind 2 sets the point target, kind 1 the position's target
(0x4ac050, not an ally: 0x4efe20) and its fire state 1, kind 3 the attack
(0x4abf60 >= 0: the position's state 3, `+0x638` the target, a re-plan,
the priority `+0x5c7` from `+0x12` unless 0xff); a record so used is
zeroed, one whose target is gone, allied or out of 0x4abf60 is kept;
`+0x7d6 &= ~0x100`, `+0x25d |= 0x100`. 0x4abe50, before the group: if no
running task is busy (0x20 with `+0x739`, 0x40 `+0x77b`, 0x10 `+0x6a0`,
0x80 `+0x58b`, 0x100 `+0x5e2`), the pending group (or, with none, the
code kept in `+0x265`) goes on and the tasks 0x10-0x80 are cleared from
`+0x25d`; else the first pending of 0x20, 0x40, 0x10, 0x100, 0x80 is kept
in `+0x265`, 0x4abc60 winds the current task down and the pending group
is dropped (`+0x265` zeroed again if anything else is pending).

**When a command arrives** (Players.cpp; decompiles of 0x520cc0,
0x51fe20, 0x521570, 0x5228c0, 0x542390, 0x51f250, 0x541f70). 0x520cc0
(players, id, 0, code, payload) builds `{+4 the tick, +8 id, +0xc code,
+0xd the payload copied, sized by code}` and appends it to the Players
object's outbox `+0x2e`; nothing is delivered there (a network client's
code 5 goes out at once by 0x520f10; with `game +0x30d9 == 2`, the
replay, commands are dropped). Every path comes through it: the
interface (0x518db0 -> 0xc458 -> the navy 0x49e230 -> its fleet tasks'
helpers), the computer players' navy and land fleets, the brains. The
frame (Players vt+0x1c = 0x51fe20; vtable 0x59b828) in single player
(the host path: `game +0xd7c = 1`, set by 0x4ebe75 / 0x4ec657; `+0xd80`
1 or 2; `+0x30d9 = 0`, 0x4ebec3), not paused and no network wait: when
a tick's time is up (`(now - +0xe6) / period`, 40 ms at speed 5) `+0xe2
= 1`, `+0xde += 1`, then 0x521570 (the outbox to the tick buffer
`+0x36`), 0x5228c0 (delivery), 0x542390 (the tick). 0x521570 holds the
outbox while the byte `+0x3e` is below the speed row's `+0x14` (table
0x5bed7c, stride 0x18; rows period/+8/+0x14: 13/15/5, 17/12/5, 20/10/5,
33/7/3, **40/5/2**, 50/4/2, 60/3/1, 70/2/1, 80/1/0) and the game is not
paused, counting it up; else it moves every record, in order, to the
buffer. `+0x3e` is zeroed only by the constructor (0x51fa69), the network
branch (0x520315) and the network / recording tail of 0x521570, so in
single player at speed 5 the first two ticks deliver nothing and every
later tick delivers all. 0x5228c0 hands each record in order to Players
vt+0x18 = 0x51f250 as `{[2] id, [3] 4, [4] 0x280f, [5] record}`: class 4
finds the object (0x51f1f0) and calls its handler at once (a ship's
0x4dc630 -> 0x4e2ba0 records its pending event; a town's 0x441ba0 acts
there and then). 0x542390 then drains the Sapp queue `+8`, sends the
tick (0x542270, message 0 to every object) and drains the posted queue
`+4` (0x541e10 appends there). 0x542630 is a direct send (`[0]` the
sender, `[1] = 2`, through `[this+0xc]` vt+0x18), not a post. So a
command given in tick T reaches its object at the start of T+1, before
anything runs, after the earlier ones; one the interface gives between T
and T+1 comes after tick T's; nobody in tick T sees it.

The port applies the player's orders at once (`Scene::apply`), the same
moment as the original's delivery, and keeps the brains' and computer
players' ship commands in an outbox (`Scene::post`: the brains' fleet
moves and repair docks, the flotillas' moves and docks, the move of a
founding ship) delivered in order at the end of the tick that gave them
(`Scene::deliver`), before the player can add his; in the first two
ticks (`order::HOLD`) the player's are held with them (but the Building
page's switch, the interface's own). Within a batch it keeps the ship as
it was before its first order of the group, running a later one over it
only when the original's would win (a higher code, or the same code
overwriting), a refusal leaving the old task; an attack ranks after the
group and goes with any order of it in the batch; a stop drops the
batch's. Not ported: the hold after loading a save (`+0x3e` is the
Players object's; whether a load rebuilds it was not read) and after a
change of speed; the ship's pending events are run at delivery, not at
its own turn in the tick; 0x4abe50's deferral of a busy task (the port
has no busy marks); 0x4abe00, 0x4aa290 / 0x4aa2a0 not read; `+0x251 ==
1` not identified; a fire at a target (0x15, kind 1: the port's forts
only); the commands still given at once: the computers' founding (its
answer decides their move), treasure hunts (a refused one tries the
next), zones (the navy's own task in the original, set at once there
too by 0x4a09d0; the attack zone's ship commands not modelled), the
brains' sword hunt and the land fleets' town commands whose answer the
port's part reads at once (the computers' builds, upgrades, mendings and
yard orders; the brains' and computers' spells). Through the outbox now:
the brains' full-town placings and upgrades and both players' order 10
ending a mending (`Order::Halt`). The brains never stop a fleet: no
brain code gives fleet order 0xdc (0x4c45f0 -> 0x4c4180, command 10 to
each ship), and the primitives only switch state on an event or a
timeout (0x479750); the end of unloading (0x4adef0 step 0x13:
event 8 mask 0x40 and the move aside 0x80, the way back from the
group's gather task later: the port goes back to its deposit at once
after the move aside); lightning's, reefs' and whirlpools' wrecks, a
tick early in the port.

The port: `scene::yard`. The port's slot is the cell a ship is in (a ship
mended at a dock aside); ships just built hold their berth until made the
same tick. Its towns' building lists are kept as the original keeps
them ("A town's buildings"). The player's
yard buttons (0x4f34c0: codes 0x21-0x2c and 0x7d, payload dword 2 = -1,
byte 0xc `code - 0x15` or 0x25, byte 0xd the Building page's switch at
the click, 0xe 1, 0xf 0) send order 0x20 through the navy (0x49e230:
one selected object that is not a ship gets it raw, 0x520cc0) to the yard
clicked, which runs 0x416a40 with pay 1; the town's command 0x20 is the
computers' and brains' path. The right press sends 0x3c {1, kind}.

A barbarian structure (0x40ef70, classes 0x3f-0x41, its berth vtable
+0xb8 0x40e050 as a yard's) builds on order 0x20 (codes 0xc-0x17, no
Argo, no state test) at once: no berth test, queue, town, price or sound;
the record as a yard's, with byte 0xd every item of 0x5964d8[kind] at 100
and its armour and ram, no stock or fallbacks; message 0x1488 puts it on
the berth (removed at once, nothing paid, if 0x499a40 fails) and 0x4b3ab0
builds it as a yard's. Its "busy" answer (vt+0x9c 0x40e7b0): class 0x40
`0x4e82f0(this, 1, 0xff)`, the others -5. Its orders come from the land
fleet's ship part (0x428f20, the brains' builders): over the fleet's
buildings in order, a stronghold (type 0x1a6b) answering 0 to vt+0x9c(1)
(so class 0x40, standing, berth free) is sent order 0x20 {+8 the part's
field 7, +0xc the ship's code, +0xd 1, +0xe 1, +0xf 0xff with field 8}
(0x42957a: the default-arming byte always set); a town (0x1491) after
0x43da20's tests is sent command 0x20, which it hands to the first of its
yards answering 0 (0x441ba0); yards themselves are passed over (any type but 0x1491 and 0x1a6b: the
list loop goes on; with a part's own building, 0x429001, it breaks off
to the tries). The port (brain.rs `builder_ready` / `builder_order`):
the barbarian structure by `yard::barbarian_order` with the arming, a
town through its yards (the computer players' ship part's way); a yard
in the list never. (Not traced: whether every brain build comes through
0x428f20 rather than another sender of 0x20.)

The port keeps a queued order's byte 0xd and arms it when built, takes
the stock before the funds test, buys the armour and ram fallbacks
untested, takes armour and rams from the settlement's store free first
(`+0x248` / `+0x24c`, taken out when built now; levels only: their
condition is never read), with default arming tests the fallback items,
armour and ram in the queue (0x43da20 test 5), starts each settlement's
store from its record (0x441ba0 message 2: fields 8-10 rams 3, 4, 5,
11-13 armour 0, 1, 2, each condition 100), refuses bombers and rammers
without their study (-7), takes
shots on a ship being built by 0x4b4000 (at most 4 a tick, at its tick)
and code-4 blows by 0x4befc0 (whole on the hull, at once, the giver its
killer; the original's 0x4e0d00 with `+0x252` and `+0x8d6` not read),
lets behaviour switches through while it is built and keeps the berth
clearing's move pending, sets a human's ship's behaviours at the
launch (its own and the seats'), and looks for the yard among the berth's neighbours (a cell
holds the building whose footprint covers it: the port has no building
slots). `Scene::barbarian_order` builds a barbarian structure's ship on
its berth. A ship built now takes its stock items best first (0x43ead0
sorts each item's units by condition, qsort 0x58791f with 0x4478e0) and
carries each one's condition, a bought one whole, in the order taken
(0x4dfe00 -> 0x4bd920, `+0x299 + 4 x (8u + count)`, kinds 2 and up;
`Arming::conditions`); armour and rams keep no condition on a ship. The
smithy's finished pieces go to its town's store (`run_refits`). Every
owner's ship is priced and armed so: the computers' and brains' orders
(0x428f20) carry the default-arming byte 1 (`start_ship_armed`). Not ported: the port's other moves have no event queue (see
"One event a tick"); 0x4bd920 at the launch; a record ram of 0 (a
barbarian order without arming, made bare by the port: `+0x27b = 0 - 3 =
0xfd`, no ram drawn, listed as level 0, any ram fitted refused, the ram
tables read past 0x598054; it cannot arise, the brains always set the
byte, 0x42957a); whether reefs and whirlpools mark the landscape nibbles the
cell tests read (the port keeps them apart); the brains still build the
barbarians' ships their own way (25 ticks beside the structure, brain.rs
`builder_order`), not by `barbarian_order`. The ships' LCG seed `game +0x2ee5` is the
original's time stamp or the saved `RND_INIT`: here drawn once from the
scene's generator.

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
75 % too. Sell Town (command 0x80, slot 6) sends order 0x34 with byte 3
(0x4f34c0), which the settlement turns into 0x248f: per stock unit amber
`cost[kind / 3] x c x 75 / 10000` and fish `0x597170[kind] x c x 7500 /
10000`, per armour piece `0x42d960(level) x c x 75 / 10000` (30, 60), per
ram `0x42d940(level) x c x 75 / 10000` (25, 50), and the building `cost x
condition x 75 / 10000` (0x425990), in integers, no goods. It is not
destroyed: it goes to state 3, drawn as its building picture's last frame
(0x422e30), keeping its owner and stock, with no menu and taking no orders.
Only the human can sell (nothing else sends order 0x34; byte 2 the
stock pick, 0x4f73f5; byte 1 a building or docked ship clicked in the
sell mode, below).

**The sell mode on the map.** Sell Item sets the pointer's mode 0x34
with the settlement (`+0xef`); the pointer (0x5175b0) asks the
settlement (0x441ba0): off the field cursor 0x28 enabled; over nothing
(0xc451) 0x28 greyed (`+0x1b |= 0x40`); over an object (0xc452) the
settlement itself cursor 6 (select), another's object 0 (standard), an
own one (`+0x57` 1, its player and slot) with the settlement not standing
idle (`+0xa4` 1, `+0x10c` 0) 5 (own), else 0x28: a non-ship (`+4` not
0x1488) enabled when of class 0x32-0x3e (mask 0x1fff) and in the list
`+0x1d0` (count `+0x1c8`), a ship greyed unless the settlement counts a
yard or dock (`+0x20c`, `+0x210`) and a listed one, found (0x51f1f0),
standing (`+0xa4` 1), of class 0x3a or 0x3b, holds it (`+0x1e0` its id).
The click (0x5180e0, at 0x518709), not greyed (`+0xe6`) and on an own
object, sends order 0x34 `{1, id, 0}` (0x518db0) and shows the tick
(0x515e60, as after modes 0xf and 0x28; modes 0xc and 0xd also leave
their mode, 0x516940); the mode stays. The order's 0x444220 posts the
object 0x248f, unasked:
- a building (LandObj 0x425990, with its scaffold picture `+0xe8`; the
  Cyclops lair has none): standing only (`+0xa4` 1), its owner gets
  `0x42d8b0(class) x condition +0xa8 x 75 / 10000` amber (0x4a8dc0,
  integers), state 3, killer `+0x1ac` = slot + player x 4 (its owner),
  vt+0x38: 0x422e30 in state 3 takes the construction picture (state
  digit 0, damage digit 3) on its last frame with a wait `+0x7d` of 1 and
  `+0xbc` 0. Its frame step (0x4230f0) draws that frame and steps past it
  at the next tick; the step after finds it done and, in state 3, calls
  0x424740: removed with `+0x158` 0 (no fall sound, no rubble, no ruins),
  the drain reporting it killed by its owner (nothing counted); its town
  lets it go (0x2874).
- a dock (0x416f90) swallows only order 10 of an upgrade (0x280f case
  10) and passes 0x248f on (0x4e8660 -> 0x4e7640 -> 0x427f50 ->
  0x425990), as a yard or port does.
- a fort (0x419f80) runs 0x4e7640 first (so the building's sale above),
  then its own 0x248f: in state 1 or 3 (so again on a second click before
  it goes), per unit of each of its 12 places (`+0x4d0` + 0x23 + 4k,
  count `+0x3b`, kind `+0x38`, units `+0x3f`, condition byte +1) amber
  `0x597164[kind / 3] x c x 75 / 10000` (30, 50, 70; 0x4a8dc0) and fish
  `0x597170[kind] x c x 7500 / 10000` (2, 2, 3, then 0; 0x4a8c30).
- a ship (0x4dcf1b): only in move state 6 with a building contact
  (`+0x8e6` type 0x32): `+0x251` 2, task 5, state 1 (0x4b4380), whose
  state 1 lets the contact go (0x4b3970: 0xcb8 to the dock; 0x4b3930:
  0x2876 to it; 0x4e06a0) before the line to its cell; then as a sale at
  a dock (see **Hulls, blows and dock repairs**).

The port: `Command::SELL_ITEM` keeps `pending` (the stock pick
`info_press` and the map click alike), `App::default_pointer`,
`left_click` with `Order::SellObject` (`Scene::sell_object`,
`settlement_idle`, `sellable`, `standing`; `selling` and `step_selling`
for state 3), the ship's contact let go in `sale_tick` state 1. The
original posts 0x248f; the port hands it over at once. Sell Item and Sell
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
In integers (the dock machine's state 4, 0x4b2652; entered from state 8
at 0x4b263f with `+0x751` = now): `now - +0x751 < 9` returns; else
`+0x751` = now, step = min(full - hull, full x 10 / 100), price =
0x59b388[type] x step / full; amber (0x4a8ce0) below the price: a side 3
or 4 player's ship is done (state 3, out) unpaid, any other gets `amber x
full / 0x59b388[type]` (capped at full) and pays all its amber; else the
step for the price. Then the units, items 9 down to 0, each below 100
made whole while its cost is held. Every step ends with 0x4bee60 and
0x4be030; a full hull ends it (state 3) after the refits below. The dock
gone (`+0x8e6` 0) or a stop (`+0x73a` < 0) ends it at once. The units
run items 9 down to 0 (`+0x292` down; item 9's price is 0x597164[3] = 2,
the dword past the table), but item 9, the unique ships' own, is never
worn (0x4be110 wears items 3-8). At the full hull, before the end: a boat
(type 0) without its item 3, a merchant (1) without its item 4, gets one
whole (0x4bd920 with 100); types 2-7 at a dock (class 0x3b) are fitted by
0x4bbca0(dock, 0x4be800(5, 0xff), the free weapon room); the `eax` the
code tests against 0x100 and 0x200 at 0x4b22f7 (slots 9 / 10) is the
type byte zero-extended (0x4b227c), so those branches never run and
types 8-11 get nothing.

**The repair order's machine** (0x4b2420, `+0x755`, run by 0x4b17b0 for
task `+0x72f` 1 with `+0x730` 1). The order (0x4aa990, kind 1): the
berth `+0x756` = 0x4b23a0(`+0x82c`): the object in that cell's slot, a
yard or dock (class 0x3a or 0x3b, vtables 0x595e98 and 0x59b4c4) of the
ship's own player, its berth (vt+0xb8, 0x40e050); else -1 and the order
is dropped.

- **1**: off the berth it is sent there again (0x4a6270). There, lying
  still (`+0x58c` 0): no object at `+0x73c`, the order is done (message
  8, 0xa0) with notice 0x143 (0x4a1b80); not its player's yard or dock,
  notice 0x143 and it stays. The dock taking a ship (vt+0xb0, 0x40dfe0:
  facing `+0x1dc >= 0`, `+0xa4 == 1`, with `+0x1d8 & 0x10` a town
  `+0x1c8`, and no ship in it, `+0x1e0` 0) and the ship in no contact:
  `+0x749 == 3` is the sale (`+0x251 = 2`, task 5); else the close-in
  0x4b17e0 and, when it gives 1, the contact `+0x8e6` {0x32, 5, dock id,
  dock}, 0x4b3970(1) sends 0xcb7 (the dock's `+0x1e0` = the ship),
  `+0x751` = `+0x74d` = 0, state 2, commands held (0x4b37d0 with 0x2000),
  `+0x25d |= 0x20`.
- **0x4b17e0**: `+0x76d` = 0x59791d[from the ship's cell to `+0x73c`];
  east or west a line (0x4b5220, points `+0x75e`) from the hull to its
  cell's middle moved 0x59a7c8[type] = 10 across (0x59a7cc 0), side-on 2
  or 6; north or south 0x59a82c = 13 (0x59a828 0), side-on 0 or 4; the
  heading fewer turns away (0x4b5390, the second on a tie) `+0x76a`, its
  way `+0x76c`, a turning hull below frame 0x20 on `h x 3 + 0x20`.
  Slanting: `+0x76c` 0, the line freed, 0 (state 1 again next tick).
- **2** (0x4b1c60 -> 0x4b1a90): at the line's end (or back at its
  start): called off, 0xcb8 to the dock, the contact freed, the line
  freed, the hull `h x 4`, `+0x25b` 0, 0x4b3850 and message 8 (0xa0);
  else state 8 with the contact, 3 without.
- **8** (0x4b89b0, no stop test): the furling; furled, state 4 with
  `+0x751` = now and `+0x25b` 5.
- **4** (0x4b1fc0, above); its end sets `+0x751` = `+0x75e`, state 3.
- **3** (0x4b1d40, as the landing's): out, 0xcb8 and the contact freed;
  then 0x4b3850, message 8 (0x20), 0x4b46f0(ship, 'Q' = 81), notice
  0x134 (0x4a1690) and 0x4b6d60 (0x5d98).
- A stop (0x4abc60): in state 1 it clears `+0x25d & 0x20` (the order is
  dropped), else `+0x73a` = 0xff.

A yard or dock (ShipService 0x4e7640, run by the yard's 0x421150 and
the dock's 0x4e8660 handlers) on message 0x2874 whose `+0x14` is its town
(`+0x1c8`: its town falling, 0x43bc30) sends 0xcb9 to the ship in it
(`+0x1e0`) and clears `+0x1e0`, before 0x427f50; the ship on 0xcb9
(0x4dccc5) frees its contact `+0x8e6` (0x4e06a0), so state 2 ends in 3,
state 4 at once, and the back-out sends no 0xcb8.

The sale (`+0x749 == 3`): `+0x749` is the order's `+0x834`, the first
byte of ship command 0x1e (0x4e3055: byte 0 -> `+0x834`, dword 1 the
dock, byte 5 -> `+0x838`). Only 0x4dbb40 sends a 3, when its block
record's `+0x30` is 1 (else 0); it is called by fleet order 200's block
(the fleet module 0x4a09d0 case 200 -> maker 0x4db9e0 / 0x4dafe0; its
tick 0x4dbd90, case 11 of 0x4c4f40's table 0x4c5118). The order's
payload `[4]` is the block's `+0x30`: the computer players' reserve
(task 0xb, 0x47f460 -> 0x483820) gives 2, their spares (task 0xd,
0x47f6b0 -> 0x483900) 1, so the spares are sold. The repair member's
0x4cd930 sends the block's `+0xc` (0 from 0x82); the human's go-to-dock
sender is not read. The order (0x4aa990) and command 0x1e (0x4e3055,
capability 2 only) ask nothing of the hull.
Task 5 (0x4b4380, run by 0x4ab930 while `+0x251 == 2`, which runs no
events then and whose blows 0x4b4000 drops for task 5): state 1 clears
`+0x261` (so the stop, 0x4aa540 wanting 0x2000, does nothing too),
`+0x25d`, takes off the party and hooks layers (0x4e0240, 0x4df0c0),
lets a contact go (0x32: 0x4b3970(0), 0x4b3930(0), 0x523d80, 0x4e06a0;
200: 0x4e02a0), then 0x4b40d0 and state 7 (a walkway `+0x8c2` shown) or
2. 0x4b40d0: a line (0x4b5220) from the picture's corner to the cell's
`x 64 - (w - 64) / 2` (none when there), and of headings 0 and 6 the one
fewer turns away by 0x4b5420 (0x4b5390's test without writing the
frame; 6 on a tie), `+0x76a`, its way `+0x76c` (0 when furled, `+0x25b`
5; a turning hull below 0x20 on `h x 3 + 0x20`), `+0x74d` 0. State 7:
the walkway's frame down one a tick (0x4e0470), at 0 freed, state 2.
State 2, 0x4b4220 a tick: the frame `+= +0x76c` round 0x20-0x37, the
heading `(f - 0x20) / 3`, the way 0 on the face's first frame (not when
furled); `+0x74d += 2` while below the count - 2, else count - 1 (furled:
the heading the face, frame `face x 8 + 0x5b`) and done with no way
left; the hull on the line's point. Done: 0x4e1bb0(4), which with
`+0x251 == 2` puts the building picture (0x4e0120 mode 2), frame 0x27
(heading 0) or 0x4f (6), state 3. State 3 takes a frame off each tick
and below 1 (heading 0) or 0x28 (6) credits the player 0x4e4ce0 amber
(0x4a8de0) and 0x4e4c80 fish (0x4a8c30), then 0x50e830 (a viewer on its
cell), 0x50e750 with its own player (counted for nobody), 0x4a3970,
0x4a1bf0 (notice 0x144 to its group), 0x10a6 with `+0x252`, 0x4e4f20 and
message 10 to itself. 0x4e4ce0: each load `(+0x527 + +0x47f x 100) x
rate / 100` while `+0x527` is not 0 (fish 10, leviathan `+0x52b` /
`+0x480` 5000, amber `+0x52f` / `+0x481` 0x78), which by the loaders
(0x4ad445, 0x4bd8e7: count the hundreds rounded up, `+0x527` the last
one's 1-100) is `(load + 100) x rate / 100`; each unit of items 0-8 its
condition x 0x597164[item / 3] (30, 50, 70) / 100; and 0x59b358[type]
(200, 400, 1100, 1300, 900, 800, 700, 600, 600, 900, 600, 900) / 20.
0x4e4c80: each unit's condition x 0x597170[item] (2, 2, 3, 0 ...).
Another `+0x251 = 2` with task 5: the ship's message 0x248f (0x4dcf1b)
with `+0x58c` 6 and a dock contact (its sender not found; see the
block's notes below).

Order 200's block (`+0x25`, 0x65 bytes): `+0` 200, `+4` 0x83, `+8` now,
`+0xc` payload 0, `+0x10` the dock (payload 1), `+0x18` the land fleet
(2), `+0x1c` (3), `+0x20-0x2c` an area (5-8), `+0x30` the mode (4);
`+0x34` the ships' list (0x4dabd0: ship, id, -, -, x, y, state), `+0x38`
the docks' (0x4dad30). The maker then lists the docks (0x4db2e0: the
dock `+0x10` gone, 4 in 5 return; the land fleet's docks, else every
class 0x3b building of the player whose vt+0x2c holds), the fleet's ships
(0x4db660) and moves those not in state 2 to a water cell of the area
(0x4daec0, 0x4db720 through the formation planner 0x5614e0). Its tick
0x4dbd90 hears the queue (0x4db110: 300 a ship joined, listed and moved;
0x134 repaired, unlisted, handed to `+0x18` (0x49bd60) and sent command
10; 0x137 and 0x144 unlisted; 0x143 every state 2 back to 1 and the docks
listed again) and, none heard and `+8 > 0x5f`, `+8` = 0: with fewer
than 2 ships in state 2 and docks, 0x4dbb90 sends the nearest 2 - n (by
0x4a1e90's distance map from `+0x14`, 5000) to the dock (state 2,
0x4dbb40); each ship in the fleet then reported if hit (`+0x691`) and,
lying still and listed, moved to its cell in state 1 (within 1 and
0x499a40, or farther) or sent to the dock again in state 2.

The block in full (0x4db9e0, 0x4dafe0 and the rest):
- The maker: a block of another kind ended (0x4a1c60) and a new one
  made zeroed (0x4dafe0, then 0x4a1150 the notices); one of order 200
  rewritten in place, its lists `+0x34`, `+0x38` and `+0x14` kept. The
  payload of 0x483820 / 0x483900: `[0]` flotilla `+0x154`, `[1]` 0, `[2]`
  the `+8` (navy fleet) of AIPlayer `+0x30`'s flotilla, the manager, made
  first (0x487230) so fleet 0 of the player's list (0x49b990 returns the
  list index), `[3]` -1, `[4]` the mode, `[5..8]` the flotilla's zone
  `+0x168..+0x174`. Then 0x4db2e0; 0x4db660 lists each ship (0x4dabd0
  with state 0) and counts the entries whose index is not 0 (so a lone
  ship counts 0, and the maker returns); then 0x4daec0 and 0x4db720.
- Lists: 0x4dabd0(ship, id, dock, cell, x, y, state) rewrites the entry
  of that id whole or appends one, returning its index; its callers that
  move a ship (0x4db720) do so only when the index is not 0, so the first
  entry's ship is never sent about the area. 0x4dacc0 finds by id
  (taking it off with 1). 0x4dad30 appends a dock by id `{object, id,
  cell, 0}`; the list is never emptied.
- 0x4db2e0: `+0x10` set and gone (0x51f1f0): one draw of the player's
  generator (`+0x18`, `(s >> 16) % 5`), not 0: returns 1 as it is; else,
  or the dock there, `+0x10` and `+0x14` = 0. Fleet `+0x18` of kind 0xb
  with a land fleet `+0x29` (the manager's: its list the bases' sites,
  0x487230): 0x42cc40 (player 16 0x42c9a0 with 0x40) lists the docks of
  its towns. None listed: every map cell's object slot (`grid + 0xc`, row
  by row) of player 16 (`+0x54` 4, `+0x55` 0) of class 0x40, or of class
  0x3b of the player whose vt+0x2c holds (0x40dfc0, `+0xa4 != 0x10`, in
  the dock's vtable 0x59b4c4). Then 0x4dadf0 walks the list from its end,
  taking off the gone and writing each other's berth (vt+0xb8) to the
  caller's 10 words: 1 dock, `+0x10` it and `+0x14` its berth; more (at
  most 10 read), the greatest positive value (strict, the first on a tie)
  of the wavefront 0x4a1e90 (flag 0, the shallow grid; limit 5000) from
  0x4daec0's cell at the berths, the `k`-th being list entry `n - k - 1`;
  none positive leaves no dock but returns `n`. Not decoded: more than 10
  docks (0x4dadf0 writes past the 10 words); a dock without facing (the
  port takes no berth); which cells of a building hold it in their slot
  (the port orders by the anchor cell).
- 0x4daec0: the area's middle `(y + h / 2) x W + x + w / 2` if on the
  map and water (0x4999e0); else the first water cell of the area cut to
  the map, row by row; -1 if the cut area starts off the map; none, the
  middle.
- 0x4db720(x, y): fewer than 2 entries: with 1, the group's first ship
  rewritten in state 1 for `(x, y)` and sent there (0x4cf070) if its
  index is not 0, returning 1. More: a 0x24-byte record per entry not in
  state 2 (cell, class 0, `+0xc` 1 for `+0x592 == 0`) to 0x5614e0 with
  p1 0x4d50b0(1) (deep) when a ship is deep and p2 0x4d50b0(0) (shallow)
  when one is shallow; not negative: each rewritten in state 1 for its
  cell and sent there if its index is not 0. Returns 0.
- 0x4dbb90(n): the entries not in state 2, by the wavefront (shallow,
  from `+0x14`, 5000) at their ship's cell, the greatest first (inserted
  before the first smaller: ties keep list order); the first `n`
  rewritten `{dock, +0x14, +0x14 % W, +0x14 / W, 2}` and sent 0x4dbb40.
- 0x4db110, newest first: 300 of a ship in the group (0x4cc590): its
  entry rewritten in state 0; 0x4db2e0 > 0: the result of 0x4db720 about
  0x4daec0's cell kept; then 0x4c42f0 (its behaviour). 0x134: unlisted;
  in the group, `+0x18 > 0` would hand it on (0x49bd60), and ship command
  10 (0x4e2ba0 case 10: event 8, the stop; `Order::Halt`). 0x137, 0x144:
  unlisted. 0x143, in the group: 0x4db0b0 (every state 2 to 1, their
  count kept), 0x4db2e0. Returns the last kept.
- 0x4dbd90's distance is 0x557440: `(least + 2 x most) / 2` of `|dx|`,
  `|dy|`; 0x499a40(x, y, draught) the cell's landscape (nibbles as the
  berth's, nibble 1 3 closed when deep) and its slot empty.
- The status (query 0xc9: 0x4838d0 / 0x4839b0 -> 0x4a0f60 -> 0x4a0ee0
  for `{200, 0x83}` -> 0x4dc050): no ships 0x1f5 result 1; `+0x10` 0
  0x1f5 result 10; else every ship not listed is listed (state 0), and
  500 while any is a ship (0x50e600 200). Tasks 0xb and 0xd end state 10
  on 0x1f5; 0xb hands its ships out (0x486910, each leaving with 0x137)
  only on result 10, so with a dock it waits out its 6000-12000 ticks.
- Notices: 0x134 0x4a1690 (back-out done), 0x143 0x4a1b80 (repair state
  1 at the berth lying still: the berth's building slot empty, with the
  order done by message 8; or not a yard or dock of its player, every
  tick), 0x144 0x4a1bf0 (sale end), 0x137 0x4a1770 (0x49bef0, a ship
  leaving a group with a block: sunk, 0x4a8410, or moved by 0x49bd60).
- Ship message 0x248f (the sale started at
  0x4dcf1b): the settlement's handler 0x441ba0 sends it from its order
  0x34 (0x443f8b, the payload byte 0): 3 to itself (0x443fbb, Sell Town,
  0x4f34c0), 1 to the object whose id is the payload's dword `+1`
  (0x444220, through `+0x70` vt+0x18). Byte 1 is the human's map click
  in the settlement's sell mode: Sell Item (command 0x12, 0x4f34c0) sets
  the pointer's mode `+0xdd` 0x34 and `+0xef` the settlement; over an
  object the settlement answers the pointer (0x441ba0 message 0xc452,
  cursor 0x28) for an own object (`+0x57` 1, its player and slot), the
  settlement standing (`+0xa4` 1, `+0x10c` 0): a building of class
  0x32-0x3e in its list (`+0x1c8` count, `+0x1d0`) enabled (any other,
  disabled); an object whose `+4` is 0x1488 (a ship: the test is a
  yard's or dock's `+0x1e0`) disabled (`+0x1a |= 0x4000`) unless the settlement has a yard
  or dock (`+0x20c` / `+0x210`) and one of its listed buildings, found,
  standing (`+0xa4` 1), class 0x3a or 0x3b, holds it (`+0x1e0` the
  ship's id); anything else cursor 5. The click (0x5180e0, the cursor
  0x28, not disabled `+0xe6`, an own object) sends order 0x34 `{1, id
  (dword at +1), 0}` to `+0xef` (0x518709). So the human sells a ship
  lying in one of the settlement's yards or docks, or one of its
  buildings, by clicking it in Sell Item mode; the mode stays (see
  **Selling**).

The port runs it at the game tick (`Scene::step_dock_repairs`, the
landing's `Landing` with its `dock` contact; `Scene::dock_lets_go` for
0xcb9 from `Scene::town_gone`). `in_dock` is a ship under its repair
order from the close-in to the end of the back-out, off its cell's slot;
it shields from no blow (0x4bf170 spares only `+0x251 == 2`). Its
commands are held the same span (0x4b37d0 with 0x2000 in state 1, back
by 0x4b3850 at the ends of states 2 and 3): `landing_holds`, which
refuses every ship command but the stop (`Scene::apply`), and takes no
contact or fire at will. The sale is `Scene::order_sell` (command
`Order::Sell`) and `Task::Sell` (`sale_tick`, `sale_value`), from the
spares' `FleetOrder::Sell`; `being_built` is `+0x251 == 2` for both.
Order 200's block is `DockBlock` on the flotilla (`Scene::dock_order`,
`dock_list`, `dock_centre`, `dock_spread`, `dock_send`, `dock_heard`,
`dock_pass`, `dock_status`), its notices posted by the repair machine
(0x134 at the back-out; 0x143 in state 1, see below), the sale (0x144) and
a ship leaving a flotilla (0x137, `Computer::leave`); its details under
**Order 200's block** below. The walkway's state 7 (a ship
the dock takes has none in the port); the furled turn (`+0x25b` 5,
not reached); 0x50e830's viewer; a fort's 0x4e7640 on 0x2874 (its `+0x1e0` not decoded).

**The fitting of types 2-7** (0x4bbca0(ship, dock, room), from 0x4b22f0
in state 4 at a full hull with the contact `+0x8e6` and its dock `+5`;
`Scene::dock_refit`). The room is 0x4be800(5, 0xff): the type's room
0x597ae4 less each unit aboard, items 0-8 (0x4be780 with 0; its sum is
left in `eax`, which Ghidra drops), times its places 0x597cf0 (bytes 1,
1, 1, 1, 1, 1, 2, 2, 2), at least 0. Nothing without a type 2-7, a dock,
a room, a dock of class 0x3b or its town (`+0x1c8` by 0x51f1f0). Each
item 0-8 for which 0x43da20(town, 1, 1, 0, item, 0, 1, 0, 1) is 0 is
offered `room` times (items 6-8 `room / 2`), each at 100. 0x43da20 with
these arguments (kind 1, an item; count 1; `param_3` 0: neither the price
nor a busy shop tested): the town in state `+0xa4` 1, or a town (class
0x33) in state 0, else -5; then the item's permission (bit item / 3 +
0xf of player `+0x45`), the arming `+0xa3 >= item / 3`, a workshop
(`+0x1fc`) and for items 2 and 5 a smithy (`+0x200`), 0, else -5. The
weights of the three values A, B, C by type: 2: 100, 60, 80; 3: 100, 10,
30; 4: 0, 50, 100; 5, 6: 100, 50, 0; 7: 0, 100, 50. The fitter
0x4bb670(ship, offer, out, room, weights, 5) chooses; the price is
0x597164[item / 3] amber (30, 50, 70) and 0x597170[item] fish (2, 2, 3,
then 0) each. Short of the amber (0x4a8ce0, `+0x20`) or of the fish in
hundreds (0x4a8b70, `+0x2c / 100`), a side 3 or 4 player (game `+0x1a1d`)
is fitted free and any other not at all; else 0x4a8d60 takes the amber
and 0x4a8be0 the fish as summed, not in hundreds (both floored at 0).
Each unit chosen is added with its condition (0x4bd920).

The offer (and the out list) is nine dword counts, then per item at
`+0x24 + item x 0x8f` eleven 13-byte entries: a dword, the item byte, the
condition dword (`+5`), the entry's index (`+9`); 0x4bb620 fills a
default one (counts 11, conditions 100) only when the fitter is handed
none, never from 0x4bbca0.

The score 0x4bb490(item, level, offer, n, weighted, value, weights) of
the item's entry `n`: with T_c = 0x597d2c[item x 9 + c x 3 + level] (A,
B, C) and the reload 0x5c76e0[item x 3 + level] (6000 / rate), weighted
the sum over c of `T_c x cond / reload x w_c / 100` (each term's
divisions in that order, integers), else `T_value x cond / reload`
(value 0-2, other values 0).

The fitter 0x4bb670 (the ship's generator `+0x18`, high 16 bits): `full`
the last weight of 100 (0 without), `partial` whether one lies strictly
between 0 and 100; `first` the first item offered, `end` one past the
last (nothing offered: none). Without `partial` and with `full` 2, `end`
is at most 3; `half = room / 2`, and a half of 0 makes `end` 6. Five
tries, each from no units: with a half and `end` above 6, `r = draw %
(half + 1)` heavy units, `room - 2r` places left, each unit picked among
items 6 to `end` still offered (offer count above the units taken) by
`draw % total` against the running sum of their scores (`weighted` =
`partial`, `value` = `full`, level player `+0x9e`), the first whose sum
passes it, stopping (places not given back) when the total is 0; then
while places are left and `first < min(end, 6)`, a unit the same way of
the items `first..min(end, 6)` (level `+0x9c + item / 3`), one place
each. The try's values `cur_c` (each unit's unweighted score for value c,
items `first..end`) replace the best when, without `partial`,
`best[full] < cur[full]` (only that value kept), else when
`sum (cur_c - best_c) x w_c > 0`; the best starts at 0 with no units.
The best try's units go out (entries copied from the offer).

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
whole; units are refused (0x440d40) meanwhile. Each tick it notes its
progress in whole percent (`+0x120`); nothing is mended before the end.
Cancelled (order 10, 0x4430f0), it refunds `(100 - progress) x paid / 100`
and walks the places in turn, each one's units least worn first (0x440e70
keeps them sorted, 0x4478e0), making whole each worn one whose ticks still
fit in the time spent, the first that does not ending the walk. Ordered
again meanwhile it pays again and starts over (no busy test).
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
(LandObj's drain 0x423d60, reached through 0x4e7550 and 0x427bb0, with
the message written back, 0x55b440). Both cut the same way: under the
magic shield (`+0x15c`) the units' share (0x4184f0) and the structure's
rest (0x423d60) are cut to 20/100 for bomb kind 10 (the fireball), else to
100 / 150 if flaming; then under the weapon shield (`+0x17c`) to 20/100,
whatever the blow. (The shields on buildings: LandObj 0x425990 message
0xda9a, spell type 1 from Magic.cpp 0x42f580 case 10, the weapon shield,
sets `+0x17c` and adds to `+0x188`, sound 0x56; type 2, case 11, the magic
shield, sets `+0x15c` and adds to `+0x168`, sound 0x57. The fire check at
0x4240e3 reads `+0x15c`, the lime's 200 / 40 ticks `+0x17c`.) The
weapons (items 3-8) take the share, the marines (0-2) `0x4ac1b0(kind,
the shooter's level, share)`. Barbarian structures (0x40e0f0) share
`blow x load / (load + 80)` with no cuts, the marines through 0x40ff00,
skipping kind 9 (the hook) as the fort; the rest is written back to the
message's `+9` and 0x4e7550 -> 0x427bb0 hands it to 0x423d60, which cuts
it as a fort's rest (magic shield, then weapon shield). The port:
`Scene::fort_blow` (test `a_blow_on_a_shielded_barbarian_structure`).

**State 2 is the fall, not the mending.** 0x423d60 on 0x138b (the fall,
posted by itself when a blow 0x138e reaches the life) sets `+0xa4` 2,
`+0xa8` 1 (0x424301), reports the kill (0x50e750) and removes the layers;
the nest's fall does the same (0x41d3e5, 0x41d470); no other code writes
2. A repair (0x1389 to 0x423d60) only marks the building busy (`+0x10c`
1, job `+0x110` 0xff, start `+0x114` the percent) in state 1, and a blow
meanwhile is taken as any (0x423d60 on 0x138e: with `+0x10c` and job
0xff it adds the percent lost to the start). Every building's intake
vt+4 hands 0x138e to LandObj's 0x423c30, which queues it (0x5577d0) unless
`+0xa4` is 2 (and 0x138b likewise): barbarian structures (vtable 0x594920),
castles (0x5949fc), the port and yards (0x596990, 0x59b4c4), the
workshops and temples (0x595fb0, 0x596060, 0x596df8, 0x596ef8 directly;
0x596af0 0x431a80, 0x596c10 0x434910, 0x596cf8 0x4368d0 all but
0x138a / 0x138c), lairs (0x414ed0), docks (0x4162f0) and forts (0x418f60,
after its torch and armour); the town's own vt+4 0x43b190 queues 0x1389,
0x138b-0x138e only outside state 2. The nest (0x41d300, state 1 only)
and the hero's house (0x421130, drops 0x138e) do not reach it. So only a
fallen building drops a blow, whole (no hit, lime, fire or units'
share); its hit test vt+8 0x423460 is false in 0xff and 0x10 only, so a
shot still lands on the fallen (by its box: the pixel mask only in state
1). The port: a fallen building keeps no combat (its ruins), so every
blow path (`hit`, `land_shot`, `fort_blow`, `strike`, the fire and the
stare) does nothing on it, and a building being mended takes blows,
fire and the stare as any. Not checked: the port's shots are aimed at
their target and do not hit-test the ruins on their way.

The other tests of `+0xa4` 2 are of the fallen too, never of the
mending (`+0x10c`), and the port asks `Scene::fallen` (its ruins: a
building with no combat and no owner) there: the harpy's count of a
town's marines (0x43bfe0: none in 2, 3, 0xff) and of a fort's (0x4179b0:
state 1 only; mended, job 0xff, it only sets its entries' flags, so they
count); 0x43d220 (a town's buildings by class, none in 2, 3, 0xff) for a
flotilla's ports; 0x42c9a0 dropping the gone and states 2, 3, 0xff from a
group's list, for the brains' repair dock (0x4cdea0) and a flotilla's
dock block (0x42cc40); 0x43da20 (a town makes items in state 1, or 0 for
class 0x33); the yards' and docks' vt+0xb0 0x40dfe0 and the ports'
vt+0xb4 0x40e020 (a facing and state 1; `+0x10c` not read, so one being
mended takes a ship or stays in a gather zone's list); 0x446d60's group
scan (classes 0x32-0x3e in 2 skipped). The port had read these as the
mending, and 0x446d60's as a settlement mending its stock (that is its
job `+0x110` 2, in state 1). State 0xff is a building refused at its
creation: on message 2 (placement), with no data or its site check
failing (0x5425d0 with 0x2f4 / 3 and 0x4eb, a town's 0x4e9), WorkLandObj
0x427f50 (0x4284e5), the lairs (0x415209) and TownObj 0x441ba0
(0x44259e) set `+0xa4` 0xff and post message 10 (0x541eb0; the lairs
send it, 0x542670), which removes it; these three are the only stores
of the constant 0xff there (the register stores not all read). The port keeps no such
building, so its 0xff tests are the gone.

The other classes' blows (vt+4 and the drain vt+0x4c, dumped from their
vtables): castles (0x5949fc) queue it (0x423c30, not in state 2) and
drain it by 0x4105c0, which takes a fireball's (bomb kind 10) off the war
castle's (0x43) queue unread; any other, with a shooter (`+0x16`), makes
him the wizard's attacker (`+0x1f4`, `+0x1f8` now) and plays event 0x77
(sea) or 0x78 (war), mode 1 at its cell (`SND_119`, `SND_120` list
nothing); then 0x427bb0 and 0x423d60 as any building. Lairs (0x5953f0 -
0x595600) queue it (0x414ed0 -> 0x423c30) and drain it by 0x414ef0, a
jump to 0x423d60. Both take the magic and weapon shields' cuts, the lime
and the fire test of 0x423d60, and no units' share. Harpy nests
(0x595d50) take it on arrival (vt+4 0x41d300), in state 1 only: the blow
`+9` whole off the life `+0xac` (the percent `x 100 / 280000`), no cut,
lime or fire, the nest's creature told (0xdaf7 with the shooter); at or
over its life it falls (state 2, the kill to `+0x15`'s player, 0xdaf9).
The hero's house (0x595e98) drops it (vt+4 0x421130 queues all but
0x138e). The port: `land_shot` (scene/shot.rs); the fire on castles
(`CSTLSEAMSK_1<level>`, `CSTLWARMSK_1<level>`, 0x4101d0 `%sMSK_%1u%1u`)
and lairs (`<CLPF|MNTF|AMZF>MSK_41<level>`) by their own generators
(castle.rs, walkers.rs), see A building's fire.

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

**The brains' town orders** (0x47b130, `Scene::brain_order`): a member's
first call in a state (`+0x10` 0) sends message 1000 to its own land fleet
(0x479ba0 -> 0x4a09d0 -> 0x42c430): dword 1 0 (no entry flags: the entry
stays, ended, until read), priority 1, the code, the order's flags, town 0,
dword 7 the member a ship joins; its later calls read the entry (0x4a0f60
-> 0x42c870): done or failed it is taken off and the member goes to p3 or
p4 (equal at every call site); under way, or not found (5), it stays. No
call site has a timeout (p11 0) or events. The calls: code 1 (the full
town: order 2's parts then both temples, each after the one before),
flags 0x10 (failed parts queued again), at the yards' state 0 (miss004-
miss006, miss011-miss013), and by miss007-miss010's yards beside their
states (`+0x133`: sent at the start, read at each of the yard's turns
until it ends, 0x45e510; the port's `Brain::side`); code 240 (0xf0: the
four first buildings at once, town, dock, school, then a small and a
medium fort after the school), flags 0x10, the state 0 of miss002-
miss004's yards; code 0x60 (order 96), flags 0, sent at the start by
miss007-miss010's builders (never ends, never read); 0x50 + type with
flag 0x400 for the ships (0x30, a medium fort, flags 0, the Persian yards'
slot 0x34). The land fleet is the computers' (`land_pass`, see "The land
fleet's pass"): the order's parts, without flag 0x80 or 0x100, keep the
table's priority 0, queued with the casts (0x479c20, also 0) behind the
asks (1); without flag 8 each part's dword 3 stays -1, so its handler tries
every member of the fleet's list (the member's buildings) in turn
(`Scene::brain_part`): a build (0x428a70) and an upgrade (0x429ae0) at
the towns only, done at the first that has it (flag 0x20), started at the
first that takes it, held if any refused for the price or -1 (flags 1, 2,
4), else a try spent; a ship (0x428f20) at a town by its yards as the
computer's, or at a barbarian structure (type 0x1a6b, ships, vtable +0x9c
with 1 = 0x40e7b0: class 0x40 as a yard, 0x4e82f0, the others -5) by
command 0x20 to it (`Order::StructureShip`), done once sent; the hold is
counted as berth passes when the last member's result was -1; a repair
(0x429f60) at a town by 0x43fcb0 as the computer's, or at a fort (0x1494)
or stronghold (0x1a6b) worn (vtable +0x6c = 0x423be0: standing, no job,
life under 100) and idle (+0x9c with 0xff: the fort's 0x4101b0, standing
and no job; the stronghold's 0x40e7b0 -> 0x4e82f0(0xff, 0xff), the same
and its berth free, else -1, held with flag 4) by command 0x1e to it,
which mends itself (`Order::SelfMend`); under way (0x42a2f0) a fort or
stronghold whole is sent order 10 and done, else it waits, counting no
passes. Under-way checks read the member the part was started at
(+0x60), failing when it is no longer in the list. A ship joins its member
when launched (`ship_launched`). Not decoded: the town's 0x43d430 and
0x43f8d0 that 0x428a70 and 0x429ae0 call on a member that is not a town
(passed over); the town's 0x43fcb0 out field `+0xc8` that holds a repair
at a fort tested after it (not kept); command 0x28 at a member not a town
(sent to the towns only); the pass's drop of members in states `+0xa4`
2, 3 or 0xff; the start's 0x47b130 leaving the builder's `+0x10` set for
its first state (not ported). No brain orders a study. (The port's own
`Op::Build` picks the oldest request of the builder's group; each
mission's yard picks by its own slots, not ported there.)

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

**The campaign's carry-over.** A won mission (0x478e30, the brain's
vtable +4, `(won, next)`) writes the next mission's number to the start
report at game +0x3305 (a 0x151-byte block at +0x32fd, Startrpt.cpp, `.RPM`
files; cleared by 0x537f60 when a campaign starts at mission 0); the end
screen 0x539b20 starts it with the block kept. 0x4a9670 saves the player's
96 permits, rigging, ram, armour and their kinds, rates, ranges, damages,
arming, the spells learnt and the unique ships studied at +0x3361;
0x4a9740 restores them (the permits replaced), then the mission's start adds
its own grants; heroes come from +0x33be (nine entries: present, the object
carrying them or a cell). miss007-miss010 alone test `+0x3305 <= 0` (never
true in the shipped game) for a standalone start: arming 2, rates (0, 1) or
(2, 2), ranges 2, 2, damages 2, armour 1 (2 from miss009), ram 4 (5), the
heroes of the missions before at a set cell. The port uses that for a
mission picked from the list.

**Order 240** (0xf0, table 0x5ac7c8 entry 40, parts 0x5abfd0, flags 0x27
each): port, sage school, shipyard (a dock counts) and smithy at once, then
the settlement made a town, the shipyard a dock, the school, and a small
(0x34) and a medium (0x35) fort of the town's (its counters +0x214,
+0x218: free-standing forts do not count). The Persian yards of miss002
(1, 7), miss003 (0, 3) and miss004 (2, 3) give it (flags 0x10) before every
ship, resting 500, 1000 and 2000 ticks after each.

**Objects carried on.** A won mission's victory handler (brain vtable
+0xc) sends 0x8d2 (`x | y << 16`, the cell in the next map) to what it
carries; each answers with a record-shaped blob (0x479490 appends it) that
the next load recreates before the brain starts (0x522d50, 0x51e810). A ship
(0x4e5ab0 / 0x4dfa90) keeps its type, the Argo flag, hull, experience,
levels, marines, weapons and treasures, not its heroes, goods or orders,
facing east; a town (0x4416c0 cut by 0x4451f0) keeps its class, condition
and treasures, its stock and marines emptied. Money is not carried. By the
mission won: 000 the ship with the parchment to (1, 13); 001 every building
in (0..13, 0..12) where it stands; 002 the Argo to (9, 8), its heroes
aboard; 003 the Argo to (45, 10), the town in (0..4, 40..47) to (46, 6);
004 (31, 31), Pylos (25..27, 5..7) to (32, 34); 005 (50, 64), (0..2, 0..2)
to (53, 67); 006 (49, 3), (0..12, 0..8) to (52, 1); 007 the Argo to (4, 33),
every hero to the town at (0, 30); 008 (5, 39), the buildings at x >= 56,
y <= 8 to (x - 56, y + 37), every hero to (2, 37); 009 inside x >= 60,
y <= 6 the town to (2, 40), the Argo to (3, 37); 010 every building and the
Argo inside x >= 50, y <= 8 to (x - 2, y + 26); 011 (47, 2), (0..5, 0..5) to
(43, 1); 012 (42, 7), (2..8, 48..52) to (41, 2). A town's heroes go with
it (+0x33be); heroes left aboard the Argo are lost but in miss002.
miss005-007 and miss009-012 hold no town of the player's but the one
carried.

**Campaign judges** (each brain's vtable +8): checked when `tick % P == 0`
(P 125 for miss000, 003-006, 011-013; 75 for 001, 002, 007-010); the win
first, held at every check for 250 ticks; a loss fires 250 ticks after it
is first seen, whatever follows. The generic judge 0x448610 returns at once
in the campaign: no "nothing left" defeat. 0x4c46c0 counts the player's
ships and buildings in a box and the heroes and treasures they hold; heroes
join once (0x5d99, see Hero and treasure messages). Each win needs the town anchored in the box its victory
carries, with every hero the player holds in it (heroes aboard do not
count), so a mission is never won without a town to carry:

| Mission | Win | Loss |
|---|---|---|
| 000 | barbarians cleared; the parchment aboard a ship of type 0-9 | no town at (19-20, 0-1) |
| 001 | barbarians cleared; town (0-1, 2-3) with every hero; the parchment delivered (`+0x134` 0, see Hero and treasure messages); Jason | parchment not delivered and none held, aboard or stored; town gone; Jason lost after joining |
| 002 | town (0-1, 2-3); Argus boarded; the Argo with Jason and Argus | town gone; the Argo lost; Jason gone; Argus lost after boarding |
| 003 | Argo; town (0-4, 40-47) with every hero; home (35-36, 20-21); Jason | no Argo; home gone; Jason gone |
| 004 | Persians without ships and settlements; Argo; Pylos (25-27, 5-7) with every hero; Jason | no Argo; Jason gone; Pylos gone |
| 005 / 006 | Argo; town (0-2, 0-2) / (0-12, 0-8) with every hero; Jason | no Argo; Jason gone |
| 007 | town (52-53, 1-2); Argo; no castle 0x42 in (10-49, 65-75) | town gone; no Argo; Jason gone |
| 008 | town (0-1, 30-31); Argo at x >= 57, y <= 7; town (55-59, 0-2) with every hero | as 007, its town |
| 009 | Argo at x >= 60, y <= 4; town (60-69, 0-6) with every hero | no Argo; Jason gone; no sea way from (55, 10) to (60, 0) once the player's lightning struck x 55-62, y 3-8 |
| 010 | Argo at x >= 50, y <= 8; town (50-64, 0-8) with every hero and the Fleece | no Argo; Jason gone |
| 011 / 012 | Argo; town (0-5, 0-5) / (2-8, 48-52) with every hero and the Fleece; Jason | no Argo; Jason gone |
| 013 | Argo; home (0-2, 58-60) with every hero and the Fleece; Jason; Persians without ships and buildings 0x32-0x3e | no Argo; Jason gone; home gone |

**Hero and treasure messages.** Message 0x5d99 goes to the owner's player
object (type 2, vt+0x18: param, then a record {`+0` id, `+4` class, `+8`
nation, `+9` slot, ..., `+0x16` hero}) from three places: a ship taking the
hero at their house (0x4b6df0, param 0, the ship's class 200-0xd3), a
building taking a hero in (0x424ed0, param 1) and a building letting one
go (0x424fb0 at 0x425038, param 0, the hero vt+0x88), a building's only
while it is a player's (`+0x57` 1). The brains' handlers read the class,
the owner (nation x 4 + slot against the brain's player `+0xe4`) and the
hero, never the param, and set a flag once: miss001 (0x44af70) Jason
`+0x130`; miss002 (0x44d0e0) Argus: from a ship `+0x164` (each time, with
members (1, 3) and (1, 2) sent to state 0x46 by 0x479750: an attack on
(0, 0, 10, 13) as 0x47a6a0's x, y, w, h, 7500 ticks, their usual events,
next and failed state 0: case 0x46 of 0x44c280 and 0x44c460), from any other class the
first time (`+0x160`) the permission to build the Argo (0x4a9810 with
0x50, 1; no notice); miss003 (0x44fd90) Theseus `+0x168`; miss004
(0x453b00) Perseus `+0x190`; miss005 (0x457950) Odysseus `+0x1b0`;
miss006 (0x45b280) Zetes `+0x1b8` and Orpheus `+0x1bc`; miss007
(0x45f770) Hercules `+0x105` and Atalanta `+0x106`. Each mission's judge
needs its flags to win (0x44a070, 0x44b770, 0x44dd80, 0x451090, 0x454af0,
0x458570, 0x45c510; miss001 and miss002 also lose when the flag is set
and the hero not held). These heroes are the missions' `HERO_ARR`, and
none is carried in, so the carried heroes put in the starting town
(0x424ed0 from the brain's message 2) set nothing. So a hero aboard from
the start, or aboard by any other way, has not joined; Argus put in a
building does not join miss002. The port: `hero_news` (the take
`hero_boards`, the load panel's hero moves, the heroes waiting at a
settlement boarding a ship at its load, `site_hero_news`), the flags in
`Scene::joined` (miss002's `+0x160` `Brain::argus_came`), the judges'
`Goal::Joined` from `hero_listeners`. Not ported: the carried heroes'
announcements at the start (they set nothing).

Message 0x5d94 (same record, `+0x16` the treasure's 20 bytes: kind,
.., `+0x22` field 7) is sent with 1 by a building storing a treasure
(0x424b30, while a player's) and by a ship taking one (0x4b6e80 from
0x4a5bc0: at 0x4a5c89 after 0x4b97d0 uses it at once, at 0x4a5cef when
0x4bd0c0 put it aboard), and with 0 for each treasure aboard a ship laying
them as it sinks (0x4e4f20 at 0x4e4f52) or at a capture (0x4b6f20 at
0x4b7142: 0x4b6e80 as the taker, while the prize holds any treasure, for
each slot below its type's room 0x597afc, held or not); the land party's
take (0x4de5ea: 0x4de669 with 1 after 0x4b97d0 uses it at once, 0x4de6a6
with 1 when 0x4bd0c0 put it aboard) and a ship's cast of a spell aboard
(order 0x22, 0x4e329b: 0x4e330c with 0, the slot before 0x4bd500 takes
it out) send it too. 0x424cf0 (a building's stored treasure taken out)
sends it with 0 while the building is a player's (`+0x57` 1) and
0x541a30(0x1c57) is 0: 0x541a30 (on the building's `+0xc`) walks that
object's list (`+0x10`: count `+0xc`, stride `+8`, base `+0x1c`) for an
entry whose `+4` points at a record with the id at `+4`, giving 0 when one
is found, else -4; 0x1c57 is made at every mission's start by
Players.cpp 0x51f680 (at 0x51f746) and is what the navy is sent to
(0x542630). Its callers: 0x41af58 (four calls), 0x41d3e3, 0x41d4a0,
TownObj 0x441ba0 (three), the fall (0x423d60 at 0x4243fc, each with 1: laid on the
ground), the removal (0x425990 at 0x4268ac), 0x479570, 0x4bdcc0, the
dock (0x4e7861). Only miss007's sword handler hears it (kind 22: `+0x104`
0). The capture's sends for each slot below the type's room: 0x4bd500
(a treasure out of a ship) shifts the slots after it down and zeroes the
freed last one (6 dwords at `+0x485 + (count - 1) x 0x18`), so a slot
past the held ones reads kind 0, which no handler with 0 acts on. The handlers with 1, all with the record's owner the brain's
player (+0xe4): the scrolls (kind 27, field 7 the scroll's number; class
0x32 / 0x33 the grants by 0x4a9810 with 1, the brain's strings by
0x4ff360, each a chat line (message 0x4dd to the chat), and 0x479570; any
other class, a ship or another building, string 0x837 "You have got a
scroll of knowledge."): miss001 (0x44af70) number 0, only at a settlement
or town (no 0x837): strings 0x848 "The parchment that describes the way
to Colchis", 0x849 " is delivered to the town Iolcus.", 0x479570 and
`+0x134` cleared; miss002 (0x44d0e0) 1: 35, 29, 52, 0x838 ("You can now
uprgrade to silver armor.", the game's spelling); miss003 (0x44fd90) 3
only while `+0x164` is 0 (set at the grant; then nothing, not even
0x837): 17, 50, 0x83d; miss004 (0x453b00) 4: 25, 0x83e; miss005
(0x457950) 5: 26, 0x83f; miss006 (0x45b280) 6: 53, 0x844; miss007
(0x45f770) 12: 30, 36, 52, 0x841 ("Technology that allows you to upgrade
to golden armor.", not 0x845) and 11: 41, 0x842; miss009 (0x4663d0) 13:
46, 76, 0x846; miss009 kind 9, number 6, at any class: string 0x847;
miss010 (0x46a0f0) the Fleece (kind 16) at any class, a ship's take too:
byte `+0x130`; miss003 also kind 6 (goods) at any class (0x450dc1): 0x445de0(9) on the
objects in the landscape grid's object slots (`[0x5c74fc] + 8 + 24 x (y x
width + x) + 20`) of (28, 34), (32, 34) and (30, 37), its lava flows:
0x445de0 sets the strength `+0x94` and calls 0x445920(1), erupting at
once; unlike 0x445e00 it leaves `+0xb4` alone, so the flow keeps its cycle
with strength 9. No other brain handles
0x5d94 (miss000, 008, 011-013). miss001's judge (0x44a070) wins only with
`+0x134` 0 and loses with `+0x134` set while 0x4c46c0 finds no kind 27
held by the player (its ships 0xfff and buildings 0x1fff, the whole map:
the count read at `[ebp-0x2c]`, treasure slot 0x47 + 27 of the second
call's table). 0x4c46c0 walks the player's object list (`[0x5c7508] +
player x 0xba0 + 3000`) and asks 0x4c4610: a building (0x50e600 0x32)
counts when bit `class - 0x32` of the building mask is set, a ship (200)
when bit `class - 200` of the ship mask is (or, without it, its `+0x259`
set and mask bit 0x1000000); a counted building adds its store (`+0x67`)
by kind at 0x47 + kind and its heroes at 0x3e + hero, a ship its slots
(`+0x489`) and heroes (`+0x4b5`). So 0xfff is every ship type 0-11 and
0x1fff every building class 0x32-0x3e. The other judges' treasure
counts: miss000's (0x3ff, 0) ships of type 0-9; miss010-013's Fleece
the call with (0, 3), settlements and towns. 0x479570 (this, number, the building's id) walks the
building's store by index and takes each scroll of that number out
(0x424cf0 with 0: 0x55b500, no new object), going on to the next index
after a removal, so a scroll right after a removed one stays. miss007's byte
`+0x104`, Hercules's sword (kind 22), whoever the owner: with 0 it is 0,
with 1 1 for player 16 and 2 for any other; message 2 starts it at 2.
Its member 11 (0x45e120) reads it before its switch: below 2 to state 15
(0x47aed0's take of the sword, the whole map) unless there; at 2 each of
the brain's player's ships (class 200-0xd3), settlements and towns
(0x32, 0x33) holding the sword sends it to state 0x19, an attack on the
holder's cell (x, y, 1, 1) for 6000 ticks; reaching 0x14 from 15 sets 3.
Member 8 (0x45d640), after its ask: member 11 under four royal ships,
bombers and ship killers, member 8's own count at least its size less
one and `+0x104` 0 or 2, member 11's ships join it (0x479370). The port:
`treasure_news` (miss007's `+0x104`, brain field 0; the scrolls
`scroll_news` and `take_out_scrolls`; miss001's `+0x134`, miss003's
`+0x164` and miss010's `+0x130` in `Scene::delivered`, 27 and 16; the
strings in `Scene::said`, shown by the front end as chat lines; miss001's
judge `Goal::Delivered(27)` and `Goal::Kept(27)`) from `take_treasure`
(sea and land takes), `store_treasure` / `store_at`, `run_sink_drops`,
the capture and `ship_cast`; member 11's 0x47aed0 waits in the port's
state 16 (not sent back to 15). The stores show no text of their own
(0x424b30 has none). Not ported: the capture's sends for slots past the
ones held (the port keeps no leftovers: they read kind 0, heard by none).
0x424cf0's 0x5d94 with 0 is `Scene::store_out_news`, from the load
panel's take out of a fort's or a settlement's store, the fort's cast,
`take_out_scrolls` and a building's fall (`drop_held`); the port takes
0x1c57 as always there (not decoded: whether it can go before the
mission ends); its other callers (0x41d3e3, 0x41d4a0, 0x4bdcc0) not
read. miss003's goods: `Scene::stir_volcano`. miss001's `Goal::Kept`:
ships of any type, any building 0x32-0x3e of the player's.

**Buildings asleep.** A building's record field 4 sets its state
(0x441ba0 for settlements and towns, 0x427f50 for the rest): 1 being built,
2 a saved state, 3 asleep (0x10), else standing. Asleep it keeps its owner
and counts for the judges, but has no banner, no radar dot, no sight, can
not be clicked, does not fire, is not aimed at (0x40dfc0), ignores blows,
and takes no cargo or trade. Every 15 ticks (first at load + `r % 15`, `r` its own generator's first draw, 0x4281de) it
looks one cell round its footprint (0x425590) for a ship of its owner's or
an awake building 0x32-0x3e of theirs; then it wakes (0x425720), waking its
owner's sleepers round it in turn, and tells its owner (0x5d9a): miss001's
and miss004's judges note it, miss003's first gives the player 2000 amber
and 500 fish and starts members 2 and 9 over. Asleep: miss001 settlement,
port and dock at (0, 2); miss002 three forts; miss003 the town (35, 20) and
its four buildings; miss004 Pylos; miss008 Propontis and its port; single05
and single13 a cluster each.

**Lightning** (LightningObjClassTy 0x42cfa0) strikes at frame 11 of its
picture over water, 40 over land. It asks each of the 3 x 3 cells'
occupants, row by row, whether it is shielded (0xda94): a ship there is
wrecked at the asking, shielded or not (0x4dc630); a building shielded from
magic (`+0x15c`) stops the asking. Unless one did, the struck cell is dug
(0x5030c0 with layer 0: the map editor's erase 0x502e60 on layers 0 and 2-7
in turn, stopping at the first that fails, then 0x502fb0 clears what lies
loose (record kind 0) on layers 5-7 within two, but on whole ground, kind 4
variant 1; a re-tiled cell takes a random variant of its shape, 0x501bb0-
0x501cb0, the lists 0x5be150 / 0x5be24c / 0x5be304 / 0x5be3ac / 0x5be438
by kind 4-8; a cell one in from the map's side first notes the edges its
outer neighbours give (0x502da0, 0x502b20, kept at 0x5ca798), which
0x501e70 uses for neighbours off the map when the dug cell is on that ring,
else it makes them up from the cell's own edges): its ground goes to
sea (0x502350), the same ground about it whose edges it held goes too, the
coast about it is re-tiled to fit (0x5028b0, 0x5027e0, the tables 0x5bdeb8
shapes, 0x5bdef8 and 0x5bdfe8 edges, 0x5be0d6 / 0x5be150 ground and
0x5be35e / 0x5be3ac shelf variants), and for ground (cell type 8) the shelf
within two goes deep and is laid again about the coast (0x502a80,
0x5029d0); a dig never shuts water. Then the blow (0xda93, `+0x14/+0x16`
the struck cell, `+0x18` the caster) to the 5 object slots of each of the
9 cells (cell order row by row; in a cell `+12, +16, +28, +24, +20`;
0x42d287-0x42d361), so every object there once; only the occupant slot
is filtered: a ship there is skipped when the cell is its reserved next
cell (`+0x5ab`, 0x42d28f; the query above has no such filter, so a ship
reserving a cell of the 3 x 3 is wrecked by it). No handler tests the
caster's nation, and none shows a hit or plays `hitmed` but the gorgon.
Unshielded ships go down; forts, ports, docks, workshops,
temples, schools, castles, harpy nests and barbarian structures
(WorkLandObj 0x427f50, asleep or not; the barbarians through 0x40ef70 ->
0x4e8660 -> 0x4e7640) fall struck on their own cell unshielded or when
their site no longer holds (the town's check 0x4eb on their cell with
their flags and 0x40, what stands there aside, and their town's id;
barbarians `+0x1d8` 0x20 for 0x3f / 0x41, 0xa for 0x40),
else lose 25 points of condition (5 shielded). Settlements and towns
(TownObj 0x441ba0, its dispatch 0x442c2c -> 0x4449b7 -> 0x4449d4) the
same by their own check: on the anchor (`+0x74`, `+0x78`) unshielded
(`+0x15c` 0) they fall (killer `+0x1ac` the caster, 0x424740); else the
map's 0x4e9 on the anchor with flag 0x40 (0x51dd9a -> 0x42e500 ->
0x42e1e0, the settlement's site check, what stands there aside) failing,
they fall; else condition `+0xa8 -= 25` (5 shielded), above 0 the hit
points `+0xb0 x cond / 100` (unsigned), else cond and hit points 1 and
0x138b, killer the caster. Creatures, each by its own rule:
- the gorgon (0x402ab8) and the siren (0x404505), anywhere in the 3 x 3,
  shield or not: with a picture (not resting under water) life 0, the
  current picture cut short (`+0xac` / `+0x12c`), killer the caster; the
  gorgon's sound event 116 (`hitmed`), the siren's 117 (no entry). They
  are reported and removed at their next state-0 tick;
- harpy and dragon (0x420cd8): on their own cell removed and reported to
  the caster; else `life -= life x 25 / 100` (the current life, signed),
  reported only at 0;
- land creatures (0x414b0f, each through its jam 0x415ee0): 0x411ef0
  rebuilds its half-cell walk grid; on the struck cell, or its own
  half-cell shut, killed; else percentage `+0x78 -= 25`,
  `> 0`: life `= max x pct / 100`, else killed (the cry only with
  `+0x144`); reported to the caster. The grid (`+0xac`: origin x, y in
  cells, width, height in half cells, the shorts) is made once where the
  creature appears (0x411ce0): the bounding box of its cell's land region
  (0x42e590 over `[0x5c74f4]`, 0x49a950's regions), 6 cells more each
  way, kept on the map. It starts on the cell's north-west half (`+0x90 =
  2 (x - origin)`, 0x411e7f; drawn at `origin x 64 + half x 32 + 16`,
  0x414359), which is why the brains' 0x42ebf0 wants no north or west
  shore. 0x411ef0 walks the 13 x 13 cells about it (masks 0x594ba0,
  0x594e44, 0x5950e8) in the grid, each cell's four halves from open:
  an object other than a jam (0x1f7) in the cell's slot, ground not land
  (low nibble 8), nibble 5 at 1, or nibbles 6 or 7: all four shut; else
  the shores of its tile (0x5acf13 by the layer-0 variant: 1 the north
  halves, 2 the east, 4 the south, 8 the west). A lair (0x46-0x48) or
  castle (0x42, 0x43) in a slot, unless on the grid's last row, also
  shuts the south-west half of the cell east of it (if its grid column is
  below the origin's x + width - 1), the south-east half of the cell west
  (if its grid column is above the origin's x: grid-relative against
  absolute, as written) and the north halves of the cell south of it
  (0xfffe marks, 0xffff once the cell itself is written);
- lairs (0x4158b7): on the anchor, or the site check 0x4eb {anchor,
  flags 0x41} failing (free ground, what stands there aside, an inland
  tile), they fall; else condition `-= 25` (no shield variant), hit
  points `max x cond / 100`, at 0 they fall; killer the caster;
- the hero's house (0x421150) passes 0xda93 nowhere: nothing; a landing
  party (0x4227c5): removed, reported to the caster; sheep (0x433a77),
  reefs and whirlpools unless `+0x74 == 3` (0x430dae, 0x4484ef): removed,
  not reported. State 3 is "under water" (0x430320 / 0x447b80 case 3:
  landscape nibble 1 back to 0 or 3, picture message 0xd21, a wait),
  where they wreck nothing: a cast whirlpool starts there (mode 1,
  0x447b80(3, 1): 150-250 ticks, 0x5972d8 / 0x5972dc), and both go
  there when a ship sits on the cell as they would rise. Amber standing
  (`+0xd5` its picture: set in state 0 by 0x489f70 / 0x48a2c0, cleared in
  state 1): amount `+0x78` 0; its tick 0x48a430 then finds 0x48d2f0 at 0:
  state 1, and the tick after lies fallow (state 2) or goes when its
  regrowths are used (`+0x88 >= +0x84`). Fish (`+0x111` the shoal's
  picture, states 1-3 of 0x48d910): amount 0 and 0x48d910(0), down for
  25-75 ticks (`+0xc5 = rand % 51 + 25`, its LCG `+0x18`); at the end of
  the wait 0x48dd80 finds it empty and it goes (`+0x88 >= +0x84`) or
  waits to regrow (state 4). The leviathan (not in states 5/6: life
  `+0x80` (record field 6) 0 if above 0, else amount `+0x78` (field 4 x
  100) 0; on its cell 0x494400(0), down (state 0, picture freed, 25-75
  ticks); else with its picture `+0x189` the flag `+0x11d`); the sea
  monster as ported (`sea_monster_lightning`). The land creatures' cry
  (0x413ad0, 0x138b) needs `+0x144`, the flag a shot's blow 0x138e sets
  (0x413b69) and the creature's next turn clears (0x411520 at 0x411698 /
  0x411879; 0 at creation 0x411100, saved 0x4142cb): lightning's kill
  cries only after a shot the same tick.
The port follows these, the walk test with the creature's own half
(`+0x90` / `+0x94`, which it walks from half to half, **Land creatures**
below), the barbarians' site
taken to hold (their flag 0x20 and `+0x1c8` not decoded), the creatures'
percentages derived from their life (`life x 100 / max`, as 0x414c8f),
the settlements' condition as their hull (`max / 4` off, `max / 20`
shielded), amber emptied at once (the original's two ticks of states 0
and 1 skipped), the land creatures' kills without a cry (the port has no
shot-then-turn order for them). The leviathan's life (`+0x80`, record
field 6) is kept: struck, it goes to 0, then what is left goes as one
caught. Not ported: its going down when struck on its own cell, and its
states (below). The ships' ways are rebuilt (0x49a950) and the caster's brain told
(0x5d9d, cell and caster): miss009's notes the player's strike in x 55-62,
y 3-8. Its north-east sea is shut off by land at y = 4; only strikes at
(58, 3), (59, 3), (57-61, 4), (60, 5) or (61, 5) open it.

**Quick lime** (item 10 of the quick lime ship, type 9; shot kind 13,
letter N, never flaming). Its power is not a weapon's: 0x4bc360 sets it to
row 13's B x 15 by the heavy weapons' damage level (200, 240, 280 x 15) x
the ship's hull / its full hull (48750), no unit condition; the blow is
power x accuracy / 100 (power / 50 if none), x 1.5 with the Flame, x 1.5
with Theseus (0x4c0230). It reloads 6000 / 55 ticks + rand % 30: items
9-11 always take rate column 2 (0x4c06e0; 0x597a3c: Greek fire 50/70/80,
quick lime 45/50/55, powder 60/80/80). It strikes the one object it lands
on. On a ship (0x4bf170): a ship with `+0x251` 2 (being built or sold;
not one mended at a dock) is untouched; else,
before anything, it is limed (0x4c4120: `+0x258`, start `+0x89`, 120 ticks
added to `+0x8d`, 24 under its weapon shield, no cap), then the blow (a
fifth under the shield, cut by the armour and the Armour of Ares) is
shared out: the hull none (0x597fd8[13] = 0), the crew its share as is
(0x4ac1b0: row 13 has no A), the weapons theirs. On a building, fort or
stronghold (0x138e): the units their share as for any shot, the structure
the rest whole (under its magic shield `+0x15c` 20/100 for a fireball,
else 100 / 150 if flaming; then a fifth under its weapon shield
`+0x17c`; 0x423d60), and if it
stands it is limed (`+0x144`, 200 ticks added, 40 shielded). A limed ship
(0x4df2d0 clears it when the time is out) takes no fire order (0x4e6700
0x15/0x16) and does not fire, unless its ram or board switch is on and it
is type 2, 5 or 6 (0x597a30) or carries a hero who fights (0x4e66c0,
0x4b4dd0: 0x5991e0 gives each hero an item, Orpheus none); each tick its
way is `min(v + accel, top) / 2` (0x4a3f40); it boards at half strength
(0x4b5b00). A limed fort or stronghold does not fire; a limed school,
smithy, studio or town's work stands still and its menu's buttons are off
(0x423a00); when the lime is out the work starts again from when it struck
(`+0x124` = `+0x14c`). Limed pictures are drawn through 0x5c1758 (0x4df510).

LandObj's tick 0x425990 calls vtable +0x70 only while the building is
busy (+0x10c), idle (+0xa4 1), not limed (+0x144 0) and on job 0xff
(mending): +0x70 is the mending step, 0x425100 for every class but the
fortress (0x419ba0) and the barbarian structure (0x40eba0), which call it
and then mend their units. So a limed building's mending stands still, no
step and nothing paid; on expiry `+0x124` is the lime's start and steps go
on. The temples (0x438300, 0x439260) run 0x427f50 first, which ends with
0x425990, then their own work, returning while limed: the learning (100 /
200 ticks) and the Flame (250) belong to the temple building, stand still
while it is limed, and on expiry count from when the lime struck, so they
may end on the expiry tick. After a spell is learnt, while fewer than six
are known, the spell at 0x596da4 (0x596ea4) + known x 4 is tested, which is
one entry before the order at 0x596da8 (0x596ea8): the spell just learnt,
then each one passed over; while its permission bit (player +0x45, through
0x42fd70) is clear, known goes up by one more.

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
only one of that item and the target is not of family 0x32 (0x4c0230,
0x50e600: a ship, a creature, anything but a building). The shot is
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
the rammed's, by the angle class of 0x4c19b0: 1 the same heading, 2
opposite, 0 any other) x its type's full hull (0x5c76b0) / 100 x the
impact, over 2500, in integers; each feather of Boreas aboard (kind 12,
flag 0x10) then adds its amount in percent of the rammer's blow, in turn.
The impact's length is 0x557440's (the smaller component plus twice the
larger, halved), its divisor the rammer's rigging level's top speed
(60000, 69000, 78000). A
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
  a line of points (0x4b5220: a point a pixel of the longer side and the
  end, an even count padded by one, so always odd) two points a tick;
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
mending all of them at its end (0x441ba0). State 1 is training, a queued unit taking
`ticks[kind / 3]`.

Queues. A settlement (marines) and a workshop (weapons) each keep one
queue of item kinds, mixed, in a byte array (settlement +0x260, head
+0x254, tail +0x258; workshop +0x1ec, +0x1e0, +0x1e4). An order pays at
once and appends; an idle building starts timing the run (+0x124 = now,
0x43ba60). Each item is ready once the run has lasted to its end (the sum of
`0x5971a0[kind / 3]` up to it), the last ending the run. Message 0x3c,
sub-order 2 (sent by a right click on the button, 0x4f46c0, through the
button's second id 0x1d83 + slot) takes the last of that kind still to be
made off the queue and pays its amber `0x597164[k / 3]` back (0x4a8dc0) and
its fish `0x597170[k]` raw (0x4a8c30: 2 or 3, a hundredth of the
`0x597170[k] x 100` taken by 0x4a8be0 at 0x43baac); the time spent stays
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
(0x423d60), standing idle (`+0xa4 == 1`) and not busy, marks itself busy
(+0x10c) with work 0xff (+0x110) from its current condition (`+0x114` =
+0xa8, percent), progress `+0x120` 0, `+0x124` = now. At the building's
tick (0x425990 message 0: busy with job 0xff, `+0xa4 == 1`, not limed,
`+0x144` 0) vtable +0x70 runs (0x425100; a fort's 0x419ba0 calls it and
then mends its units): once `now >= +0x124 + cycle / 100` (`+0xc0`, 200
for every player building: every 2 ticks), `+0x124` = now; below 100% the
owner holding `cost / 100` amber (0x42d8b0, 0x4a8d00) pays it (0x4a8d40),
the condition goes up 1%, the hull `max x condition / 100` (integers),
`+0x120 = (condition - start) x 100 / (100 - start)`, a new damage look
(vtable +0x38) when its stage changes; short of the amber it stops
(`+0x10c` 0, `+0xa4` 1). At 100% (the step after the last 1%) the hull is
full, vtable +0x74 runs (a fort's 0x419910) and it stops. So a full
repair takes 200 ticks and costs the building's price. Blows taken
meanwhile lower the condition (0x423f9c). The port steps it at the game
tick (`Scene::step_mending`, the tick and condition in `mend_steps`).
With `+0xe0` set and `+0x130 >= 0` each step sends the menu (0x203f,
0x80000003) notice 0x10a4 {+0x14 `100 - +0x120`, +0x18 0, +0x1a
`+0x130`}; the menu 0x4f4cc0, for its page `+0x15c` equal to +0x18,
keeps the byte at slot `+0x1a`'s `+0x3a` and redraws it (0x10a5 clears
it). `+0x130` is -1 but for a fort mending itself (0x418dde: 4, the
repair button), so only that button shows it: the port's
`fort_mend_progress` on the fort's menu slot 4.

Docks (`Shipserv.cpp`, 0x4e7640) serve ships alongside
(reloading); a dock's own queue (0x416f90, every 25 ticks) builds ships.

## Magic

A temple learns a spell in 200 ticks, 100 for computer players (side
value 3 or 4; 0x439260). Casts (`Magic.cpp` 0x42f580) are checked by
0x42ef90: reef, whirlpool, monster, lightning, fireball within 12 cells,
waterspout, phantom and the protections within 15. Protections and the
phantom add 3000 ticks to what is left (messages 0xda9a, 0xda97 to the
ship, 0x4dc630). Cast whirlpools and reefs go after 7500 + rand(7501)
ticks (Whirl.cpp 0x447ee0, Reef.cpp 0x4306d0); every tick, but under
the water, they post 0xda95/0xda94 to the ships on their cell, which then
wreck (flag 0x200, state 4: 0x4ab720, 0x4aa280).

Reefs and whirlpools rise and sink (0x430320 / 0x447b80 set the state,
0x4300a0 / 0x447930 the picture, 0x4301f0 / 0x447a50 step it). `+0x74`:
0 rising (`BREEF<k>SPR_0`, `MREEF<k>SPR_0`, `WHIRLSPR_0`), 1 up (`_1`), 2
sinking (`_2`), 3 under the water (no picture), 0xff not yet (ctors
0x430070 / 0x447900). A picture steps a frame each `cycle[state] /
frames` ticks (integer; reef 30/60/30, whirlpool 60/60/60); at its end
rising goes up and sinking under. Waits are drawn from the object's own
generator (`+0x18`, seeded `game +0x2ee5 x id`), `r % (max - min + 1) +
min` by state (reef 0x596a60: up 1-2, under 150-250; whirlpool 0x5972c0:
up 125-250, under 150-250; 0 for the others). Up, a medium reef or a
whirlpool sinks once its wait is out and its picture is at its last
frame; under, it rises once its wait is out. A big reef never sinks.
Going up: if the cell's occupant is a ship with Jason (0x4ba790: a hero
slot holding 0), it stays under (a new wait of the state it was to take;
a whirlpool's tick first waits on while Jason lies there, not stepping
in, 0x4482a0); else the sound event 151 `riff1wav` / 149 `whirl3` when it
was under (or cast), nibble 1 of the landscape 5 big reef, 6 medium, 4
whirlpool (0x499aa0 refuses those steps), state 0. Rising ended: event
152 / 150 (`spider`). Going under: nibble 1 back to the tile's (3 under a
layer-1 tile set 2-6, else 0), a wait of 150-250. Creation (record field
4): 0, the map's and a brain's (0x430ec0), straight up (state 1, no
sound); 1, cast (0x42f580: a reef's kind field 7 = `draw % 3 + 1`, a
whirlpool's field 7 its caster), its end `r % 7501 + 7500` ticks on (the
first draw): a medium reef 75 ticks unseen then up (its occupant with
Jason, off its reserved cell, holds it back), a big reef up at once, a
whirlpool under (150-250). A cast big reef goes at its end whatever its
state; a medium reef or whirlpool only while under. The wrecks spare a
ship on its reserved next cell (`+0x5ab`) or with Jason. A ship with
Jason sinks a medium reef or whirlpool (0xda9e / 0xda9f via 0x4a7630,
unless sinking or under) when stepping into its cell free of ships
(0x4a3a70, then steps in), and lying still (0x4a3150) on a cell holding
one, the one at (x, x): both arguments are the cell's x (`push ecx; push
ecx`, 0x4a330c). Not ported: the 0xda94 the reefs also send to a cell's
slots `+0x10` and `+0x18` (the leviathan, the gorgon: its picture cut
short). A tornado (`TRNDSPR`, Tornado.cpp 0x43a380)
steps a frame every 2 ticks: rising 0-41, moving 42-83 looped, sinking
84-124; moving, it slides to its next cell over 16 frames, then strikes
the objects on it (0xda96, 30000, a fifth on a ship shielded from magic)
and picks a neighbour (0x43a6c0: weight 3 within one heading of the wind,
else 2), sinking at the first hop after 250 ticks of moving.

Lightning (LightningObjClassTy 0x42cfa0) strikes at frame 11 of its
picture over water (40 over land) and posts 0xda93 to every object on the
3 x 3 cells about its cell, whoever's: a ship unshielded from magic wrecks
(0x4dc630); the rest by their own handlers, as **Lightning** above
(settlements and towns 0x4449d4, lairs 0x4158b7, harpies 0x420cd8, sea
monsters 0x492c34, land creatures 0x414b0f, WorkLandObj buildings
0x428600).

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

Lightning's states (`+0x74`, 0x42cdf0 sets the picture `LTSPR_<state>`,
frame 0, a frame each `0x596160[state] / frames` ticks: 2, 33, 34):
made by a cast (0x42d480 message 2, record field 4 = 1) it plays sound
event 0x90 (144 `flash`) mode 4 at its cell and takes state 0
(`LTSPR_0`, one frame, drawn at (x 64 + 15, y 64 - 481)); 2 ticks on,
its frame past the last (0x42cfa0), event 0x91 (145 `molniya2`) mode 4
at its cell, the screen shakes (0x55f450) and it takes state 1 (cell
layer byte 0 or 1, water) or 2 (else), centred on the cell; the strike
at frame 11 / 40 of that picture. The port plays both sounds and shows
the second picture 2 ticks on (`Scene::spawn_lightning`,
`run_lightning`), with `LTSPR_0` on layer 7 at that top-left (no
0x50d1a0 call for state 0; 0x50d660 hides it on a cell the viewer's fog
byte is 0, the lightning's cell) for its 2 ticks.

The shake, 0x55f450(display, layer 1, 3, steps), called at 0x42d0ba
with steps of (x, y, milliseconds): (0, -10, 50), (0, 5, 50),
(0, -3, 40), then one more pass with (0, 0) ending it. Each pass Blts
the layer's rectangle (0x55d440: its x, y, w, h) moved by the step (a
negative offset takes the source that far in, a positive one puts the
target that far in: the picture moves by (x, y)), sets the layer's
origin (0x55d650) to the saved one less |x|, |y|, marks the rectangle
for redraw (0x55f7d0) and composes (0x564970); then sleeps until the
running sum of the steps' milliseconds from timeGetTime (only when more
than 2 are left), a failed Blt ending it. At the end the origin is put
back. It runs inside the game's tick, so the game waits for it, and
the time is not made up: the frame (0x5203c2) reads the time before the
tick, runs one when `(now - +0xe6) / period` is not 0 (`+0xe2` 1 above
that, never more than one tick a frame) and after it sets `+0xe6 = now -
(now - +0xe6 - period) % period` (0x52043c, the `idiv` remainder): the
phase kept, whole periods behind dropped. So the frame after the shake
runs one tick at once and the next falls on the old phase. The port
draws the map view moved by the step for its real milliseconds
(`Scene::shake_offset`, `run_shake`, `LIGHTNING_SHAKE`) and runs no game
update meanwhile; at its end the tick clock is set to one tick due plus
the phase (`(phase + shake - period) mod period`), and an update with no
time runs that tick. (The network game's turns are not held for it: its
paths' wait not decoded.) Not decoded: what the composition 0x564970
draws with the moved origin over the Blt's picture (the port shows the
Blt's move).

Magic.cpp 0x42f580(spell, object, x, y, draw, share), its cases: each
first 0x42ed40(spell, 1, share) (a count `+0x10 + 4 spell`, else the
permission bit and the price), then 0x42f470 (a count taken, else the
price); none plays a sound itself. 1 reef / 0xc big reef: message 0x1a64
{class 0x25a / 0x259, 2, field 4 mode 1, x, y, kind `draw % 3 + 1`}; 2
whirlpool: 0x1a63 {600, 2, mode 1, x, y, the caster `slot + nation x
4`}; 3 tornado: 0x1a68 {mode 1, caster, x, y}; 4 monster: 0x493780
(nation, slot, kind `draw & 3` for nation 4 else the nation, x, y, 4,
4); 5 view: 0x48b9e0 alone (silent); 6 lightning: 0x48b9e0, 0x1a66
{1, x, y, caster}; 7 fireball: 0x48b9e0, 0x16e1 {nation, slot, x, y,
0}; 9 phantom: 0xda97 to the object, 3000; 10 / 11 shields: 0xda9a,
3000, type 1 / 2; 0xd: when the object is given and a count is held
(0x42ed40 gives 0 for it otherwise), the count taken and 0x158bf {88000,
0x96} to the object. No case 0 (wind) or 8 (flame): those are the
temples' own. The castles' wizard (0x410b79) raises the count of his
spell (3 or 7) and casts it with draw and share 0, so it is free: the
tornado for the monsters, or the fireball with its sight point for
them; the port's `castle_tick` does the same (`spawn_hazard`,
`spawn_fireball`; the monsters' sight point is not kept, the port's fog
being the human's alone). The tornado's rising (0x43a5d0 state 1) plays event
0x9b (155 `wind8`) mode 5 at its cell.

A sea monster (UMonsterClassTy, `umonster.cpp`) has 50000 x its record's
percentage of strength (0x492360: max `+0x7c` field 5, now `+0x80` field
6; a summoned one 100 %, 0x493780), keeps to the cells within field 11
and 12 of its start (all the map with either 0), looks over the cells
within fields 13 and 14 (2 and 2) and starts in mode field 15 (2), type
field 10 (0 for 4 or more). It is a state machine run each tick (0x491fa0;
states entered by 0x491830): 0 under water, without a picture (unseen,
not to be struck), waiting 50-125 ticks; then, in mode 0 losing a
hundredth of its maximum, dying at none; its mode falls to 1 below 51 %
and 0 below 11 %; it chooses (0x4911f0): the ship that last struck it,
else over its scan an enemy monster under water weaker than it (eaten at
once: it loses 80 % of the other's strength), else the enemy ships on
their own cells not in move state 6 (a phantom seen one time in ten),
weighted by their move state `+0x58c`: 0 lying still 100, 1 told to stop
70, 2 turning 80, 3 at a cell 90, 4 braking 60, 5 under way 50 (see Ships
in each other's way; a rammer at its contact and a boarder at its hook lie
in 0, the boarded ship in 6), the most still kept and
one picked by how near a bite brings it to sinking (`(100000 - v) / 100`
if it would sink, else `/ 300`; the pick indexes the whole list, so it
may fall on a less still one, and then it swims instead); it then jumps
to the cell beside the prey it strikes from (0x491030: within two of it,
free, the last one nearer than the first) and strikes (state 4,
`UMON<t>3<h>`, 15 frames, 3 ticks each, sound event 129 `atmons3`),
biting at frame 10, 6, 6 or 9 by type the ship on the prey's cell
(message 0xf166: 37500 x its strength left in whole percent / 100, three
quarters off with hero 5, a fifth under a magic shield, costing it a
twentieth of its maximum; in mode 1 it must swim off before striking
again), then moves into the prey's cell if free and dives. With nothing
to strike it swims (0x490d60): an instant jump under water to one of the
25 cells within two (weighted 10, 100 with a treasure on it, or towards
the ship that struck it), surfacing (`UMON<t>0<h>`, 13 frames, 2 ticks,
event 96 `apse`), swimming (`UMON<t>1<h>`, 2-3 ticks a frame) and diving
(`UMON<t>2<h>`). Struck while up it hurries (a frame a tick). Lightning
on its cell: under water half its maximum and a new wait, up its death;
on a cell about, only up, half. Dying (state 6) shows `UMON040` frames
3-35, 2 ticks each, event 107 `deamons`.

Its treasures (`+0xb9` count, `+0xbd` two entries of five dwords: the
treasure record's fields 4-8). Each time it enters state 2 (swimming,
0x491a40-0x491ab0), with fewer than `[0x5c1136]` = 2 carried, it takes the
treasure (class 0x2bc, state not 5) on its cell's third place: message
0x158c1 to it hands over the five dwords, and the treasure (0x4074d0,
0x406c80(3)) leaves its cell with sound event 272 (`trea2`); no player is
told. Message 0x158c1 to the monster adds one the same way, 0x158c2 asks
whether it has room. A map record gives it field 17 as its count, raw, and
fields 18-27 as the two entries (message 2; the port keeps at most two).
Gone (0x491ed0, after dying or when its dying cell is refused) it lays each
back at its cell (0x520c70: world message 0x16e3 with a treasure record,
the entry at fields 4-8, the cell at 9 and 10). Not decoded: the treasure's
state 5 and the callers of 0x493620 (a carried entry's kind); the handler
of world message 0x16e3, so whether a message is shown (the port says "...
lies in the wreckage", which nothing in 0x491ed0 or 0x520c70 sends). With
`free`, 0x402ee0 refuses a cell whose leviathan slot (`+0x10`) holds one
showing a picture (its vt +0x30, 0x496040: `+0x189`).

The Gorgon (MedusaClassTy, `gorgona.cpp`, class 0x1fa, vtable 0x594340,
constructor 0x401000: life `+0x78` 150000, side `+0x57` 2) runs her tick
(message 0, 0x401f70) every game tick: the state's code, then a step of
her picture (0x4013b0: when `+0x94 + delay <= now`, delay `+0x98` 1, 3
dying, 0 when cut short `+0xac`, frame `+0x45` drawn and the next readied;
past the last, the first again and a loop more `+0xa4`). Her generator
`+0x18` is seeded `[game +0x2ee5] x id` (message 2, 0x402360), her cell
fields 5, 6, her area that cell -/+ fields 7, 8 clipped to the map (either
below 1: the whole map), her treasures field 10 and two entries at 11-20;
a cell 0x402ee0 refuses removes her. She starts resting. States (entered
by 0x4019d0, which zeroes `+0xa4` and sets `+0x9c = now`):

- **0 rest**: no picture, `r % 301 + 200` ticks (`+0xa0`). At its end:
  with no life she goes (her kill reported to `+0xb4` if set, her slot
  cleared, message 10; state 5); else she hops (0x401780) to a cell of
  the 3 x 3 about her (her own too, columns then rows) within her area
  that 0x402ee0 lets her into (landscape nibbles up to 7 and 3, no ship,
  nothing on the third place but a treasure while she carries fewer than
  `[0x5c1136]` = 2, no leviathan showing, her slot `+0x18` free): weight
  100 on a treasure else 10; with a target (`+0xb8`, 0x401600) `100 /
  distance` (0x557440, 1 at 0) and 1000 more on the heading towards it
  (0x48f100); picked by 0x404730 (one entry no draw, else `r % (sum + 1)`
  against the running sums). A hop -> 1, none -> 0 again.
- **1 rise**: `GORG1` (30 frames) once (sound event 94, `apmed3`).
- **2 stare**: `r % 3 + 1` loops of `GORG2` (28 frames; none with no life
  or cut short: straight to 3), taking the treasure on her cell with room.
  As frame 13 is drawn (latch `+0x7c`), loop 0, 1, 2 stares over the 3, 5,
  7 cells square about her (sound event 127, `atmed1c`, once a stare
  before the list, 0x402111): every map cell's occupant
  `+0xc` of side 1 (a ship but on the cell it steps into), columns then
  rows, gets message 0x158bf with 88000 / their number (22000 from five),
  150 and her id. A ship (0x4dc630) with hero 8 (Perseus) or a magic
  shield is spared; else it turns to stone for 150 ticks more (not yet
  stone: `+0x257 = 1`, `+0xa1` now, `+0xa5` 0, the stone picture
  0x4df510(1, 1), the commands saved 0x4b37d0 into `+0xd1` / `+0xd5`
  keeping `+0x261 & 0x2000`; then `+0xa5 += 150` after the blow; freed in
  the ship's tick when `now - +0xa1 >= +0xa5`, 0x4df260: 0x4df510(0, 1),
  the commands back 0x4b3850; port `Combat::stone_since` / `stone_len`,
  `Scene::tick_statuses`) and takes the blow (type 0x1fa),
  cut by a mirror shield's share (cargo item 0xe). With no life or cut
  short the loop playing is the last.
- **3 sink**: `GORG3` (30) once; then life left -> 0, none -> 4.
- **4 death**: `GORG4` (21, 3 ticks a frame) once (sound event 105, `deamons`); her
  cell gets the Gorgon's head (treasure kind 17) and what she carried
  (0x520c70); then 0, whose end removes her.

Struck (vt+4, 0x4014d0) in states 1-4: the blow comes off her life (down
to 0; a blow leaving 0 reports her kill at once, 0x50e750); with no target
and life left a ship or building that struck her becomes it, one outside
her area widening it to the square of half-side one more than its
distance about her (0x4016b0); sound event 116 (`hitmed`). Lightning
(0xda93, her picture shown): life 0, cut short, the caster her killer.
Shot at in states 1-3 only (vt+0x2c, 0x404690); her aim point 0x402da0.
Her sounds are all 0x50e570 mode 5 at her cell (`+0x114`, `+0x118`).
A building stared at (each cell of its footprint is listed, so a 2 x 2
gets four messages, each of the share): the hero's house (0x421150)
drops it. A fort (0x419f80) posts itself (vt+4 0x418f60, queued by
0x423c30 to its tick) a blow 0x138e of bomb kind 4, not flaming, player
0xff, no shooter (0x41a041): the stare's `+0x14`, `x 20 / 100` under a
magic shield (`+0x15c`), less a stored mirror shield's percent
(`+0x4b4`) of it; then it hands the message on (0x4e7640) with `+0x14`
0. Every other class hands it on whole (barbarians 0x40ef70, dock
0x416f90 and yards via 0x4e8660, port 0x4e7640, castles 0x410ca0, nests
0x41d4a0, schools, smithies, studios and temples via 0x427f50, lairs
0x414f00, towns 0x441ba0) to LandObj 0x425990 (0x426fec): unless gone
(`+0xa4` 0xff), asleep (0x10), fallen (2) or the cyclops lair (class
0x46; a building being mended is in state 1 and takes it), it is limed for `+0x18` (150) ticks more (`+0x144` set, `+0x14c`
now and `+0x150` 0 when not limed yet, then `+0x150 +=`), then takes
`+0x14` off its life (`+0xac`) whole, no units' share even for a
barbarian stronghold, no hit shown (both a fifth under the magic shield):
below its life it comes off (`+0xa8` the percent, vt+0x38), else the life
is 1 and it falls (0x138b), put down to no one. The port:
`Scene::stare_building`. The port: `scene::gorgon`. Not decoded: the treasure's
own test 0x407950 on pickup. Not ported: tornadoes (0xda96) and reefs or
whirlpools (0xda94, 0xda95) on her (the port's strike ships only); her
save record (0x8cf: the port saves its own state).

A siren (SirenaClassTy, `sirena.cpp`, class 0x1fd, vtable 0x594378,
constructor 0x403050: life `+0xb8` 100000, killer `+0x138` -1). Message 2
(0x403f60) seeds her generator `+0x18` as the Gorgon's (`[game +0x2ee5] x
id`), puts her on cell fields 5, 6 (0x402ee0 not strict refusing: removed,
state 5), with fields 10-20 her treasures (2 at most, `[0x5c113a]`) and
her area her cell -/+ fields 7, 8 clipped (either **below 0**: the whole
map; the Gorgon's below 1), hidden (state 0). Her tick (message 0,
0x403ba0) runs the state's code; a state is entered by 0x403650 (run not
done `+0x128`, start `+0x120` now). One picture, `SIRENA` (163 frames):
state 1 frames 0-10, 2 11-100, 3 101-113, 4 114-162, from the first, a
frame every 2 ticks (3 dying; 0 cut short by a spell), the step 0x403360
drawing when `last + delay <= now` and past the last frame starting over
with the run done.
- **0 hidden**: no picture; rest `+0x124` = `r % 751 + 250` (a draw). At
  its end: life below 1, reported to her killer if set (0x50e750), slot
  cleared, removed (state 5); else the hop 0x403520: the 3 x 3 about her
  (her own cell too, columns then rows) within her area that 0x402ee0
  takes strictly and never onto a treasure, each weighing 1, one picked
  by 0x404730 (no draw with one, else `r % (n + 1)`, the first twice as
  likely): moved -> 1, none -> 0 again.
- **1 surface** (sound event 95, `app3`): frames 0-10 once, then 2.
- **2 sing**: with life and not cut short, frames 11-100 once, taking the
  treasure on her cell (as the leviathan's, cap 2); else straight to 3.
  As frame 45 is about to be drawn (a latch `+0x114`: once a surfacing)
  she sings (sound event 128, `sir1`): every object in the ship slot `+0xc` of
  each map cell within 7 either way (columns then rows, 50 at most), of
  **any side**, gets message 0x15ca7 with 750 ticks, a share of 88 / their
  number (22 from five), her cell and her id. Then 3.
- **3 dive**: frames 101-113 once; then life left -> 0, none -> 4.
- **4 death** (sound event 106, `deamons`): frames 114-162 once, 3 ticks a frame; her
  cell gets what she carried (0x520c70, no head); then 0, whose end
  removes her.

A ship without hero 7 (Odysseus) takes the song as a status (0x4a7390):
already under it, its length `+0xb5` grows by 750 and the share `+0xc9`
and cell `+0xad` are the new ones; else it starts now (`+0xb1`) with 750
and nothing added up (`+0xc5` 0), its commands saved (0x4b37d0: `+0x261`
into `+0xb9`, cleared; `+0x25d` into `+0xbd`, kept). Either way it sends
itself the move event 0x80 to her cell (kind 0x17, priority `+0x19` 50),
which 0x4ab720 / 0x4ab250 take without the capabilities (kind 0x17 with
`+0x25c`). The ship's tick (0x4dc630 message 0) first frees stone
(0x4df260, below) and ends the song (0x4df320: `now - +0xb1 >= +0xb5`),
then takes its event (0x4ab720) and moves (0x4ab930): with `+0x25d &
0x400` (set there from `+0x261 & 0x4000`) a charmed ship runs 0x4a74c0
instead of the plain 0x4a3f40 (0x4abbc2). 0x4a74c0: `+0xc5 += +0xc9`; at
1000 or more `+0xc5 %= 1000` and, unless shielded from magic (`+0x255`),
a code-4 blow of 300 (type 0x1fd, player 0xff), `300 + 300 x -75 / 100`
= 75 with hero 5 (0x4ba790(5)); the commands saved again if `+0x261 &
0x4000` came back (setting `+0x25d |= 0x400`); `+0x59f = 7000`; `+0x261
|= 0x4400` for the rest of the call; the move event 0x80 to her cell sent
again when `+0xc1 - now > 30` unsigned, and as no code writes the ship's
`+0xc1` (only 0x4df320's clear of `+0xa9..+0xcc`), on every tick; then
0x4a3f40. So the n-th share is added n - 1 ticks after the song when the
ship ticks after her, and the route to her cell is planned anew each
tick, the wait 0. A ship turned to stone (`+0x25d` 0, `+0x261 &= 0x2000`)
runs no 0x4a74c0: no shares, no blow, no new move; the song's time runs
on. The port: `Scene::tick_statuses` (creatures.rs), each game tick after
the creatures' ticks, the ships in the objects' order.

Struck (vt+4, 0x403480) in states 1-3 only: the blow comes off her life
(down to 0; a blow leaving 0 reports her kill at once, 0x50e750; sound
event 117, which has no sample: silent); no target. Lightning (0xda93, her picture shown): life 0, cut
short, the caster her killer. Shot at in states 1-3 (vt+0x2c, 0x404690);
her aim point 0x4048f0 (no picture: her cell's centre; else from her
picture's top left, a hit `30 + (r & 7)` each way, a miss `r % 61`, `40 +
r % 24`, x drawn first). The port: `scene::siren` (ticked with the Gorgons
in the objects' order; her sounds mode 6 at her cell). Nothing but a ship
reads 0x15ca7: the fort 0x419f80, barbarians 0x40ef70, dock 0x416f90,
yards 0x4e8660, port 0x4e7640, hero's house 0x421150, castles 0x410ca0,
nests 0x41d4a0, lairs 0x414f00, WorkLandObj 0x427f50 (schools,
smithies, studios, temples), towns 0x441ba0 and LandObj 0x425990 have no
case for it, and the land creatures' jam 0x415ee0 hands only 0xda93,
0xda96 and 0x187a2 to its members (the rest to 0x50c9e0, none); so any
other occupant sung to counts in the share and takes nothing. Not ported: tornadoes (0xda96) and
reefs or whirlpools (0xda94, 0xda95) on her; her save record (0x8cf: the port saves its own state).

The pointer picks the Gorgon and a siren only by 0x49a580's last try,
the pointer's own cell's slot `+0x18` that holds her (0x402ee0 refuses
a cell where another holds it), after everything else and only where
the fog's second gate passed. Her vt+8 (the Gorgon's 0x402d10, the
siren's 0x404800), with her state `+0x110` not 0, 4 or 5 and her
picture (`+0x11c`, `+0x164`) made: with its third argument (the
cursor's `+0xbd`) 0 a hit at once; else a hit anywhere in the picture's
rectangle `+0x59 <= x <= +0x59 + +0x61 - 1`, `+0x5d <= y <= +0x5d +
+0x65 - 1`, no mask. The siren's also tests the bit of `SIRMSK` (`+0x84`,
at the frame `+0x45`, clamped to its count less 1, 0x543240 /
0x557640) only with a fifth argument not 0, which the pick passes as 0.
The port: `Scene::lurker_at`, after `pick_cells` in `render::hit_test`,
neither picked otherwise.

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
either way, and field 11 shortens its rests. HarpyClassTy's vtable is
0x595e00 (constructor 0x41dde0: side 2, nation 4, state `+0x88` 0xff): +0
0x4207b0, +4 0x420670 (a shot, 0x138e), +0xc 0x41df70, +0x10 0x41de30 (`hp
x 100 / max`), +0x1c 0x41e0f0 (speed `0x1900 / (frames x delay[3])`), +0x2c
0x41de90 (a dragon in state 1 cannot be targeted).

**Ported** (`scene::flyers`, a game tick at a time). Created (message 2,
from the nest's 0x41d4a0): generator `+0x18 = game +0x2ee5 x id` (the LCG
`s x 0x41c64e6d + 0x3039`, `r = s >> 16`), hp `+0x89` 200000 (harpy) or
250000 (dragon), home the nest's cell, state 0. A tick (message 0) runs
0x41fbd0 (states) then 0x41e4b0 (picture and place). 0x41fbd0 first sets
state 7 when not in 1, 2, 6, 7 at frame 0 with hp 0, and, orphaned
(0xdaf9: the nest gone; no chase, not homing), sets `hp = ba - (now - b6) x
ba / 1000` (`+0xb6` the tick, `+0xba` the hp at the fall). The states
(0x41f680):

- **0 appear.** At its cell's centre, heading `(r & 3) x 2 + 1` drawn
  until the cell along it is on the map; a harpy plays `HRPSPR_0h`, a
  dragon rests at once.
- **1 rest.** Draws `+0xe8 = now + r % 50` then `+0xec = now + r % (1100 -
  100 x field 11)`. Until `+0xec`, while not told of an attacker, with a
  nest, hp and no hit since (`+0xca`), it rests: a harpy plays its idle
  picture (frames held at 0 otherwise) at `+0xe8`, and after each, `+0xe8
  = now + r % 50`. Then a harpy takes off; a dragon only while an object of
  side 1 and a nation other than 4 lies in the nest's range (0x41ef00),
  else rests again (both draws anew). A dragon resting is not drawn.
- **2 take-off**, then 3 (7 at hp 0).
- **3 fly.** Entering, a leg (0x41e840): homing (`+0xc6`) with a nest and
  off both the nest's row and column, the nest (weight 100); else each
  cell of the 13 x 13 mask at 0x595a30 (1s only) about it, on the map,
  within the nest's range and with its flyer slot free (0x406830): weight
  1, 0xff beside the nest's attacker (`+0xd2`), else plus 10 x `n` for
  each side-1 ship (not on its `+0x5ab`), settlement or town (0x1491) or
  fort (0x1494) beside it (row by row), `n` the sum of `1 + count` (`+8`)
  over its list's entries of category 1 with item 0-2 (a list of 11
  entries, 0x557770(0xdc)). A ship's list (0x4e3960 through 0x4e5e40)
  has each marine squad alone with no count; a settlement's or town's
  (vt+0x5c 0x43bfe0, page 0, nothing in states `+0xa4` 2, 3, 0xff; on its
  way up as an upgrade, `+0xa4` 0 with `+0x23c`, it first sets its life
  `+0xa8` to 100) its nine items' places with their stock counts, so 3 +
  its marines; a fort's (vt+0x5c 0x4179b0, only in state `+0xa4` 1) each
  squad of its places alone. One draw, `t = r % total + 1`, the first cell
  whose running sum reaches `t`. The route is
  0x55c3d0 from its cell and heading (below); `+0x10d` = 1, the step from
  its cell's centre to entry 1's; entry 1 entered with another heading:
  state 4 first; else moving (`+0xe0` 1) if 0x41ee20 allows the cell, else
  holding (3). Then the flap sound's tick `now + (r % 41 + 60) x 375 / 100`
  (drawn again when it passes, moving). Moving, its cell (flyer slot
  `+0x14`) follows its place; at a step's end: the attacker's leg flown
  (`+0xd6`, homing) re-picks; else 0x41efb0 (always asked) finding
  something, and not homing: state 5; else the next entry: same heading
  and allowed, moving on; same heading but not allowed, holding, and
  re-picking at once if the flyer in that cell is bound for this one;
  another heading, state 4. At the route's end: homing on the nest's cell
  with a nest, it lands (6) (chasing: homing ends, re-pick); else
  re-picks. Holding plays the picture at delay index 4 and re-picks at its
  end (and again, homing ends, if the next cell's flyer is bound here).
- **4 turn.** `HRPSPR_4`/`DRGSPR_4` from frame `heading x 8` to `turn-to
  x 8` (through the last frame when 0 is the shorter way), then back to
  the state it came from (3: moving or holding as 0x41ee20 says; 5; 6).
- **5 strike.** 0x41f1c0 picks (none: state 3), turning first if its
  heading differs. A harpy: the 8 cells about it row by row, passing
  over a ship (200-211, `+0x253`) or building (0x32-0x3e, vt+0xa4
  0x40dfd0 `+0x134`) with the phantom, the last ship of a nation other
  than 4 without Orpheus (not under way into the cell) or settlement,
  town or fort (any side; its list filled unused), the attacker before
  all; at frame 7, if the target still is, the shot. Its look 0x41efb0
  likewise stops at any settlement or town and at a fort without the
  phantom (lists filled unused). A dragon: the empty cells about it that 0x42ebf0
  answers (0xf: low landscape nibble 8, no north or west shore, nibble 5
  not 1, nibbles 6 and 7 zero; 0x2c0: any cell whose low nibble is not 8;
  else 0), weight 0x32 on land after a building (0x32-0x3e) was met in the
  scan, else 0; one draw (`r % count` if all weigh 0, else `r % total`,
  the first running sum reaching it); at frame 16 it picks again and sows
  there. At the picture's end it heads home (`+0xc6`) and flies a leg.
- **6 land.** Homing ends; an even heading first turns to heading + 1.
  Then rest.
- **7 die.** The picture, then removed (message 10).

0x41efb0 (at each step's end): the 3 x 3 about it, own cell too, row by
row, by the cell's object slot. A harpy stops at a ship of a nation other
than 4 without Orpheus; a dragon at the attacker, else at the first empty
cell 0x42ebf0 answers: no if no object came before it, else `r % 3 == 0`.

0x55c3d0 (W, H, start, heading, end): up to three passes, each from the
start: write the cell; `D`, `A` the main and side directions to the end
(diagonal when neither distance is twice the other; `A` = `D` on a row,
column or diagonal); `D` the one closer to the heading; within 1: stop.
At the first bend the turn's sense is set (towards `D`: -1 when `D -
heading` is -3..-1 or 5..7), the heading reversed (`D - 4`) when `D` is 2
or 3 off and the end is 1-2 cells straight off or one diagonal, and no
turn when 2 and 3 off with the end a knight's move away. Passes 0 and 1
turn the heading by the sense and step, charging every cell off the map
by its overshoot; pass 2 steps along `D` near the end ({<2, <2} or a
knight's move), else turns until the step is on the map. A pass charged
nothing (or pass 2) is kept: the end appended if reached, else a straight
run along `D` (axis: until the distances are equal; diagonal: until one
is 0), then along `A`. After pass 0 the sense flips; after pass 1 it flips
back when pass 0 was charged no more. Each entry holds the heading it was
entered with.

0x41ee20 (a cell may be entered): its flyer slot free or the flyer's own,
and no other flyer in state 3 in the 3 x 3 about it flying into it.

0x41e4b0, once `now >= +0x81 + delay` (or not yet drawn): moving, the
place is `(to - from) x frame / (end - first - 1) + from` (the end frame:
`to`); it is drawn at the place less (0x28, 0x78) (a dragon (0x43, 0x92),
its shadow (0x43, 0xd5); not in states 0 and 1), depth `max(0x26d0 - y,
1000)`; the frame steps towards the end (state 1 not idle: 0); at the end
0 and 6 set `+0xe0`, 1 `+0xe4`, 2 `+0xe0` holding its last frame, 3
`+0xe1`, 4 `+0xdb`, 5 `+0xe9`, 7 removes it. Delays by state at 0x595d10
(harpy 1, 2, 1, 1, 0, 1, 1, 1) and 0x595d30 (dragon 0, 0, 1, 1, 0, 1, 1,
1); index 4 also while holding. A step thus takes the flying picture's
frame count in ticks.

**Ported** (part 2, `scene/flyers.rs`, test
`harpy_hits_chase_and_aim_are_the_originals`): a shot's blow (0x420670,
message 0x138e, `[+9]`): less than its life it is taken off and the
creature's own generator draws once (`r % 7 == 0` a cry, events 0x72
harpy / 0x73 dragon, not played); else its life is 0, and the blow does
not remove it: the tick head sets state 7 (cry 0x67 `deadga1` / 0x68
`dedr2`, its dying picture, then message 10); an orphan's `+0xba` loses
the blow (0 if not less); `+0xca` set either way; no hit shown, no death
reported. Its nest standing (0x41d300, `+0xa4 == 1`), a blow less than
its life sets `+0x1e8` 1, `+0x1ec` now and sends its last creature
(`+0x1e4`) 0xdaf7 with the shooter's id (`[+0x16]`; not decoded for a
shot with no shooter: the port sends none); 0x41d4a0's tick sends 0xdaf8
(`+0xce`, `+0xd6` 0) once `+0x1ec + 3000 <= now`, clearing `+0x1e8`. The
shot 0x14ba: a harpy's flies to the prey's vt+0xc as a hit (a ship's
0x4e07c0), from the picture's corner + 0x27; a dragon's from its own
corner (place less 0x43, 0x92) + 0x27 to the chosen cell's middle,
letter 0xf, no target, no blow, then the sowing by 0x42ebf0: 0xf a land
creature of class `500 + r % 3` (0x1497), 0x2c0 a sea monster of type `r
& 3` for nation 4 slot 0 (0x493780), both drawn from the dragon's own
generator. The aim point 0x41df70 (`Scene::flyer_aim`, in the fire
pass) and vt+0x2c 0x41de90 (a dragon resting is not looked at; a flyer's
life is not tested). The turning pictures `HRPSPR_4` and `DRGSPR_4` are
in the data, 64 frames each (8 a heading). 0xda93 (a quarter; on its
cell gone at once), 0xda96 and 0x158bf remove it at once through the
battle code, as before.

A harpy's shot at a settlement, town or fort carries the same blow,
to the building's aim point (vt+0xc 0x423520) as a hit, with no shooter;
the building takes it as any shot's 0x138e (a fort's units their share,
`fort_blow`; the others a fifth under a magic shield). The town state
0xff of 0x43bfe0 is not identified (the port tests mending, 2, and
selling or sold, 3). The town's handler 0x43b290, on 0x138e, first
reports the blow (0x5d9c to its player, when `+0x57` is 1 and
0x541a30(0x1c57) is 0, in any state), then takes the message off the
queue (0x55b500) when the town is class 0x33 in state `+0xa4` 0 (going
up: an upgrade), so 0x423d60 never reads it: no hit, no fire, whoever
dealt it; any other town's blow is left for 0x423d60
(`Scene::blow_dropped`, in `hit` and a shot's landing).

**The pixel test and the pictures' place** (ported, `Scene::flyer_hit`,
`objects::flyer_frames`; test `flyers_place_shadow_and_pick_are_the_originals`).
vt+8 0x41deb0 (x, y, test, out): `out` (if given) the depth 0x50d290(0);
with `test` 0 a hit; else never a dragon in state 0 or 1; else a hit
when (x, y) lies in picture 0's rectangle (`+0x59 <= x < +0x59 + +0x61`,
`+0x5d <= y < +0x5d + +0x65`: the creature, not its shadow) and the bit
of the mask frame (`+0x7c`, `..M` kind 11, loaded by 0x41e130 with the
pictures; 0x543240 gives none, no hit, for a frame `+0x45` at or past
the mask's count) at (x - `+0x59`, y - `+0x5d`) is set (0x557640: rows
of `(w + 7) & ~7` bits, the first bit of a byte the highest). The
pointer's pick 0x49a580 calls vt+8 with `out` and a fifth argument 0.
Masks: `HRP_STAY0_MSK_` for `HRPSPR_10M` (15 frames), `HRP_TURN_MSK_` /
`DRG_TURN_MSK_` (64), `HRP_SOAR_FLY1_` / `DRG_SOAR_FLY1_` (15).
0x41e4b0 places layer 0 (the creature, `+0x74`) and layer 1 (the shadow
`..S`, `+0x78`) by 0x50d660, which writes the given point as the
picture's top left (`+0x59`, `+0x5d`): a harpy's both at the place less
(0x28, 0x78), 80 x 160 frames; a dragon's creature less (0x43, 0x92),
its shadow less (0x43, 0xd5), 135 x 270. The message handler 0x4207b0
makes the two layers (0x50cbf0) and sets layer 1's
remap at 0x42098e: 0x50e130(1, 1, 0x26, 0x5c1358), only colour 0x26
through the shadow table (every shadow sprite is all 0x26; the
creature's own 0xfe is not marked on that layer). The port loads the
shadow so (`Caches::shaded_frames`), and `render::layer_origin` takes a
half pixel up (floor of v + 0.5) so a dragon's odd width stands alike
left of the map's edge. Both layers' depth `max(0x26d0 - y, 1000)`
is the object's in the port's draw list (see **The sprites' draw order**
below), both layers drawn together, the shadow under the creature. A save restored makes the flyers' pictures and masks again
(`Scene::flyers_restored`; neither is saved). Not decoded: the targets' vt+0x2c and vt+0xa4
(the port takes every object as targetable); a dragon's picture place
before its first draw (states 0 and 1, not drawn), which its aim point
reads (the port takes the draw's).

**The sprites' draw order.** Every picture slot is a record (0xd4 bytes)
of the sprite manager (count `+0x1a0`, records `+0x1a8`, by id `+0x1b0`,
by depth `+0x1ac`); a new one (0x55d6e0) takes depth 0x7fffffff at the
list's end, then its depth (the slot's `+0x19`: 0x50cbf0's second
argument, 0x50d660's fifth when not -1, 0x55e7a0 -> 0x55d9d0) is filed by
0x55d9d0, an insertion step keeping `+0x1ac` in rising depth (record
dword 0x11): going down it passes the entries deeper than it, going up
those shallower; at an equal depth it stops beside the equals, after
them going down and before them going up (a record with flag 0x4000,
the callback slots, the other way: its equals' flag 0x4000 is passed
over by a plain record, and a 0x4000 record passes no equal); a depth
set to what it is moves nothing (0x55d9d0). The composition 0x564970 copies the
list in that order (`[+0x1a0]`, `[+0x1ac]`, the flags 8 / 0x40 kept)
and draws from its end down to its start (0x564730 a picture, flag
0x4000 its callback `+0xcc`): the deepest first, so a smaller depth
draws over a greater, and of two at one depth the one later in the
list (set to it last from a greater depth) under the other. The depths
seen: flyers `max(0x26d0 - y, 1000)` (0x41e4b0), the Slight (0x25d,
`andrey\sli`) `0x26c6 - 0x40 x cell y` (`+0xc0`; at least 1000, 0x4059e0
state 1), a siren 0x4e80 kept (0x403650, 0x403a30), sea monsters
`20000 - +0x95` (the cell row; at least 10000, 0x491830 / 0x491d50), a treasure `0x26d0 - (+0x5d +
+0x65)` (0x4068e0), a building's shields its picture's less 5 / 4,
fire less 3 and smoke less 2, a ship's shields its hull's less 0x4a.
Every other depth: a ship's hull `max(20000 - bottom, 10000)` each draw
(0x4df6e0 via 0x50d1a0; 0x4f4c from a sinking's frame 4), its walkway
(layer 1, `+0x8c2`) the hull's less 0x46 (0x4e0470) and what crosses it
(layer 3, `+0x8c6`, set at the walkway's top left: a landing party going
down, porters; 0x4b71f0) less 0x47 (0x4e04a0), so both over the hull;
its flames (layer 5) less 0x48 (0x4e04d0), layer 6 less 0x49, its shields
(7, 8) both less 0x4a (0x4e0f70, 0x4e11a0); a hook's line `max(0x4de0 -
the lower end's y, 10000)` (0x4df0e0, a projectile's 0x4964c0); a LandObj
(building, barbarian structure, castle, lair, the hero's house)
`max(20000 - bottom, 10000)` at its message 2 (0x425990), kept (the
port's `MapObject::made_depth`, from the picture it is made with: the
map's, or a founded site's construction picture; kept in saves), its banner
less 1 (0x4247b0); a nest `max(20000 - (top + h / 2), 10000)` (0x41d010);
land creatures `max(20000 - +0xb8, 10000)` (their feet, 0x411520); a
landing party `max(20000 - +0xa0a, 10000)` (0x421bb0); sheep `max(20000 -
bottom, 10000)` (0x4331c0); medusa 0x4e81, skarb 0x4e82, fish 0x4e84,
amber 0x4e86 and its glint 0x4e85, whirlpools and reefs 0x4ee8, all
kept; a leviathan showing `max(20000 - bottom, 10000)` (0x493b00); the
volcano `max(0x4da0 - 0x40 x cell row, 10000)` (0x445e20); a moving
tornado `max(0x26f0 - y, 1000)` (0x43a380); lightning 0x3ed (0x42d480);
gulls and their shadows 0x401, each gull made before its shadow
(0x41c700), so of all 25 the last's shadow draws first; wake droplets
0x4f4c (0x4a3f40), projectiles' trails `max(10000 - y, 1000)` (0x48f440
with -2). Bands: 1000-9936 the air (flyers, treasures, a moving tornado,
the Slight), 1005-1025 lightning and gulls (a gull is under a flyer whose
place is below y 8912 and a treasure whose bottom is), 10000-20000 the
ground and sea by their bottoms, 20096-20160 the sea creatures' and
zones' constants, 20200 reefs, 20300 droplets (deepest). The port
(`render::sprites`, `draw_objects`) makes one list of every slot in the
order the original makes them (objects in the scene's order, each with
its smoke, fire, glint, banner and shields; walkways; parties; lines;
droplets; gulls), sorts it by depth deepest first, of equals the later
made first (it keeps no history of depths set again, so equals stay in
the order made).

Projectiles (bomb.cpp): a shot made anew (0x496190 message 2) has its
picture, layer 0, at 10000 and no layer 1 (only a save's restoring,
0x4990b0, makes layers 0 and 1 at 9; its line, layer 2, at 9 either
way). Each tick 0x4965d0 sets the depths by state `+0xb0`: 0 nothing
(layer 1, if any, `-1 - 1` = -2); 1-3 (flying; 0x4974d0 sets 2 / 3)
layer 0 kept, layer 1 `max(0x4dce - bottom(0), 10000) - 1`; 4 (landed:
0x4974d0 cases 2 / 3 call 0x498a50 with the kind and set 4, the first
0x4965d0 of state 4 running on the next tick, before the frame's step,
so frame 0 keeps 10000) by kind `+0x74`: 1, 3, 5, 6, 7 (on an object)
layer 0 `+0x80 - 10`, layer 1 `+0x80 - 11`; 2, 4 (water, ground)
`max(0x4e16 - bottom(0), 10000)`, layer 1 that less 1. `+0x80` is the
4th argument the struck object's hit test (vt+8, called `(x, y, 1,
&depth, 1)`) fills, 0x4967b0 / 0x4983d0 at the landing: its picture's
depth (0x50d290 of layer 0, as the siren's 0x402d10), so the blow
draws over the ship, building or creature it lands on. The port: shots
in flight at 10000, their landing pictures `FxDepth::Landed` (the
struck object's depth taken at the landing's tick, its picture as last
drawn).
A sinking hull (0x4df6e0, `+0x74` 5): `max(20000 - bottom, 10000)` to
frame 3, 0x4f4c from frame 4 (`FxDepth::Sinking`). Lightning: state 0's
`LTSPR_0` at 0x3ed (0x42d480), 0x42cdf0 sets 0x3f7 with states 1 and 2's
pictures. The tornado: 0x43a240 (its first picture) `max(20000 - y,
10000)`, 0x43a380 at each frame's step `max(0x26f0 - y, 1000)`, `y` its
place `+0x7d` (cell centre, 0x43aa10); 0x43a380 steps at once while
`+0x9d` is 0 (its first tick), then when `+0x81 + +0x7d` (2 ticks) is
reached. The fireball (the Slight,
0x4059e0): `max(0x26c6 - 0x40 x cell row +0xc0, 1000)`. A ship's
layer 1 holds by `+0x75` (0x4e1dee, loaded with 0x50d080) the walkway
`WMOST_` or, `+0x75` 0, a ram's blow `BOMBH30` (0x4e0340(7, 3, 0):
`BOMB%C%1d%1d`, 'A' + 7); its layer 3 by `+0x76` (0x4e1eee) the boarding
fight `ABORD_%1d%1d` (0x4b6c70: `1 - arg`, `+0x6e7`; `+0x76` 0 and 1),
the gangway `WNOSIL_` and the others: both at their walkway depths, the
ram's stepped by 0x4c2230 state 3 with 0x4e0470 (hull less 0x46, read
before that step's 0x4df6e0), the fight by 0x4e04a0 (less 0x47). The
volcano's eruption `VOLCANOERAPTION` is its layer 1 (0x445e20), set by
0x445630 at each step to layer 0's depth less 2. The port:
`FxDepth::Layer` on the rammer, the boarder, the volcano. A blow without
a shot shows no picture and no sound at the struck object: a ship's
blows are code 4 (0x4a9e30 records it, `+0x7d6` bit 4), taken in its
tick 0x4ab930 by 0x4befc0, whose 0x4be0c0 / 0x4be030 change only the
hull's own damage picture (`+0x74`, 0x4e1bb0) and, being built, layer 4
(0x4e0d00); its 0x4dc630 turns the stare's 0x158bf, the bite's 0xf166
and the waterspout's 0xda96 into code 4 and the reef's 0xda94 and
whirlpool's 0xda95 into the wreck 0x200, making nothing; a building's
0x138e (0x423d60) makes only its fires (layers 3-6), a land creature's
(0x413ad0) nothing. The senders make none at the struck object either:
the ram 0x4c1c60 / 0x4c1ad0 (its picture the rammer's `BOMBH30` above),
boarding 0x4b5b00 / 0x4b5df0 / 0x4c3410 (`ABORD_`), the bite 0x490830
(the monster's own frames), the stare 0x401f70, the charm 0x4a74c0, the
waterspout 0x43a6c0. The port's `Scene::strike` draws nothing (it drew
`BOMBG10` / `BOMBG20` at a random spot with `hitmed` / `splash1`). A
yard's ship being built is not an effect in the port. Not decoded: when 0x4e0470 / 0x4e04a0 run against 0x4df6e0
(before the first, a walkway's 0x3e6 and a gangway's 0x3de; for the
ram's blow, read: before); which
leviathan states take 0x4e85; what building layer 2 (0xc453) and ship
layer 4 (`+0x252`) show; the order of a ship's two shields at one depth
(the port draws the weapon's under the magic's); whether a leviathan
hunt's haul and line (0x3de / 0x3e6 at creation) are restacked (drawn
with the hull, the line under the haul).

## Land creatures

Lairs (`CreaturesFlatClassTy`, 0x414f00). Message 2: the cell (record
fields 4, 5: `+0x74`, `+0x78`), the most out (field 8, `+0x1d0`), the
speed (field 9, `+0x1d4`), the generator `+0x18` seeded `[game +0x2ee5]
x id` and the last creature's tick `+0x368` now (a save, field 7 = 2,
restores the generator, the count `+0x1cc`, the list `+0x1d8` and
`+0x368`). Message 0, each tick before LandObj's 0x425990: one draw r,
always; then, standing (`+0xa4` 1) with `+0x1cc` below `+0x1d0` and
`(10 - speed) x ((r % 21 + 80) x 25 / 100) + +0x368` (unsigned) below
the tick, `+0x368` is the tick and the cells about the lair's are
scanned, row by row from the north, each from the west: not its own, on
the map, the cell's object slot empty or a jam (0x1f7), its landscape's
low nibble 8, no shore (0x5acf13) on the side facing the lair (a cell
south of it no bit 1, north no 4, east no 8, west no 2), nibble 5 not 1,
nibbles 6 and 7 0, and on a jam's cell no member on the half facing the
lair (0x415e40 with id 0). The half facing it, in the walk grid of the
lair's cell (0x42e590, 6 more each way, kept on the map, doubled):
`2 (x - origin)` + 1 for a cell west of the lair, or in its column with
bit 8; `2 (y - origin)` + 1 for a cell north of it, or in its row with
bit 1. With more than one, the cell straight south is dropped; a draw
`r % n` picks one; message 0x1497 makes the creature with `{+0x36c (its
class), 1, the lair's id, that half, the lair's cell}` (0x413d70: its
grid about the lair's cell, 0x411ce0, the half kept; one with no lair
and half 0 takes its own cell's north-west half, 0x411e7f), listed at
`+0x1d8 + 4 x +0x1cc`, `+0x1cc` + 1. No cell free: nothing drawn or
made, the time taken all the same. A creature removed (message 10) sends
0x187a1: it is struck off the list, the rest closing up, `+0x1cc` - 1.
The port: `Scene::tick_lairs` (scene/walkers.rs), a game tick at a time
after the castles. A creature's state `+0x154` is also
its picture's state digit (0x4111b0: `<CLP|MNT|AMZ>SPR_<state><heading>`,
state 6 `<..>SPR_6`, the amazon's throw `AMZSPR_3<h><draw & 1>`): 0
appearing, 1 standing, 2 walking, 3 throwing, 4 hit, 5 dying, 6 turning;
a frame every 0x59538c[state] ticks (1, 25, 1, 1, 1, 2, 2; standing's 25
becomes 5 after its first frame). Each tick (message 0, 0x413d70): every
5 ticks, unless hit or dying, its walk grid refreshed about it (0x411ef0,
over its mask's cells) and a look about (0x412290); then its step
(0x412740), then its picture (0x411520).

**The walk.** It holds its half cell in the walk grid (`+0x90` /
`+0x94`, from the grid's origin; drawn at `origin x 64 + half x 32 +
16`). Standing (state 1), 40 ticks after it stopped (`+0xf0`) and on the
standing loop's first frame, it chooses a route (0x413240): a pursuit
(`+0xd0`) to the last cell of 0x55ba50's route within 5 of its goal;
else a destination among the halves of its 13 x 13 mask (dumped:
0x594ba0 the cyclopes', 0x594e44 / 0x5950e8 the others'), indexed by
half / 2 over a 26 x 26 window about its half, value 1 (the centre's
four halves never), that a flood of the grid from its half reaches in 14
(0x558fa0 -> 0x557910), each weighed 10 when the cell's layer-0 tile is
of type 8 else 0 (0x42eb20), picked by a draw of its own generator
(`+0x18`, the usual LCG, seeded `[game +0x2ee5] x id`): `r = draw %
total`, the first entry whose running sum is at least `r`; then the
route 0x55b560 (two straight legs if clear and their first direction
within 45 degrees of the heading, else a flood from the goal, 0x557910,
and a back-trace turning 45, 90, 135 degrees, then any, a step, to the
least value plus 2 straight / 3 diagonal, 0x5c5074, the nearer the goal
(0x557440) on a tie, at most `(start - goal) / 2 + 10` entries of
`{x, y, direction}`); no route drops the entry and draws again. With a
route whose first step turns from its heading (0x5c4f74, clamped to 0-7)
it turns first (state 6, 12 frames a heading, 9 for the amazon, 2 ticks
a frame), then chooses again with a pursuit of that half; else it walks.
0x557910 is a sweep of segments generation by generation (values + 2
straight, + 3 diagonal, capped at `2 lim + 2`, the new segments ordered
by their least value), not a shortest-path search. Walking (state 2),
on the tick after a half step ends it takes the route's next half if
the grid there is 0 (else the route is shut, `+0x11c`) and its picture
again, then starts the step to the entry after, its heading the entry's
direction; its place goes from the step's start to its end with the
picture's frame, `(to - from) x (frame - f0) / (end - f0)`: the
cyclops's frames 0-0x26 on the first step (frames below 7 hold, f0 6),
0x16-0x36 after; the minotaur's and amazon's from 0 (below 9 / 10 hold,
f0 8 / 9) or 0x14 / 0x13 after, to the sequence's end. A half step so
takes 39 ticks then 33 for a cyclops; a minotaur heading straight 37
then 17 (36 frames), diagonally 53 then 33 (52); an amazon 36 / 17 (35)
and 47 / 28 (46). At the route's end, with a target, a pursuit or the
route shut it stops: another creature of its jam on its half (0x415e40)
makes it choose again at once, without turning; else it throws or
stands. Every 5 ticks, after the grid's refresh, a half of the route
ahead that is -1 shuts it.

**Jams.** The creatures on a cell are held by its jam
(`CyclopsJamClassTy`, class 0x1f7, side 2, made by 0x416230 for message
0x1498 `{x, y, 1}` when a creature enters an empty cell; its id from the
objects' count, after the creature's), the cell's occupant: up to 100
members `{id, object}` in the order they joined (message 0x1879f), a
leaver's followers closing up (0x187a0), the jam gone with its last
(0x499860, message 10). A creature joins its cell's jam when made, and
on the tick it takes a half of another cell leaves the old and joins the
new (0x412cf5-0x412e9e); removed (message 3) it leaves. The jam passes
lightning (0xda93), message 0xda96 and the alerts (0x187a2) on to each
member (0x4161a0); is always there (vt+0x2c 0x411160); takes no blows
itself (vt+4 0x406330 ignores them); and answers a fort's or
stronghold's scan as its first member (0x415df0), their track taking a
jam holding the target (0x415e00). 0x415e40 (another member on a given
half) is the walk's crowding test.

**Looking about** (0x412290, every 5 ticks, not appearing nor crowded
`+0x124`). Throwing, it drops its target (`+0x120`) if the occupant of
the target cell (`+0x128`, `+0x12c`, absolute) is gone or is not the
object it took (`+0x13c`), or is a ship with the phantom (`+0x253`, set
by 0xda97 at 0x4de378) or Orpheus (0x4ba790 hero 2), or a building
0x32-0x3e with the phantom (vt+0xa4 0x40dfd0: `+0x134`, set by 0xda97 at
0x426f7b). Then over its mask's cells (value 1, row by row; on the map;
the occupant slot only) it takes an occupant that is there (vt+0x2c) and
not a jam, a castle (0x42, 0x43), a harpy nest (0x44, 0x45) or a lair
(0x46-0x48), nor a ship on its reserved cell (`+0x5ab`), with the
phantom or Orpheus, nor a building 0x32-0x3e with the phantom: no side
or player test, so it throws at anyone's ships and buildings. The first
found, or one whose `10 - d` (0x5576d8 from its cell) is above the
priority it holds (`+0x130`, unsigned), becomes its target, its throw
started afresh (`+0x134 = +0x138 = +0x124 = 0`). Having seen one, it
sends 0x187a2 `{priority, target cell}` to the jam of its own cell (the
mask's centre, inside its walk grid); each other member with no target
and not crowded, or a lower priority, pursues that cell (`+0xd0`; the
absolute cell read as a half of its grid, as written), takes the
priority and is crowded. Nothing found while throwing, it stands.

**Throwing** (state 3, 0x412740; standing on its next step after
`+0xf0`, or at a walk's end, with a target). Each tick a draw; not yet
throwing (`+0x134`), it starts when `+0x138 + 40 +- 4` (+ with bit 16 of
the draw) is below the tick (`+0x138` the tick), and turns to the last
heading of 0x5c4f4c whose signs match the target cell less its own (its
picture again at the same frame; a picture set again is drawn once
before it steps, so a turn delays the throw a tick). Its picture holds
frame 0 until then (0x411520). Throwing, at frame 0xb / 10 / 0x12
(cyclops / minotaur / amazon) once (`+0x140`), with the target object
still there (0x51f1f0): a draw, its cry one time in ten (events 0x7a-
0x7c: `c1`-`c4`, `c5`, `aas031`); at `d` cells above 1 another draw,
missing (byte `+0x3c` 0) when it is a multiple of `8 - d`, else hitting
(100); message 0x14ba `{player 0xff, target, its id, its class, letter
(0x4126f0: G, O, A), blow}`, the blow 0x594ab8[(100 - pct) / 10] (1000,
947, 894, 837, 775, 740, 532, 548, 447, 376, 30) x the level-1 blow of a
ballista / small catapult / item 3 (the A x 15 cache, 0x5c73f4 /
0x5c73e8 / 0x5c73d0) / 1000. The picture's end (16 frames; the amazon's
25) ends the throw (`+0x134 = 0`); the next starts 40 +- 4 ticks after
this one did.

**Life, blows and death.** Life `+0x7c` of 30000 / 16000 / 12000, its
percentage `+0x78`. A blow (0x138e) or death (0x138b, not while dying)
is queued (vt+4 0x4139e0, list `+0x84`) and read at its next tick before
its step (0x413ad0): a blow, unless dying, marks it struck (`+0x144`) and
takes its points off while the percentage left is above 0, else reports
the death to the blow's player (0x50e750), leaves life and percentage 1
and queues its death; its death sets it dying (state 5, percentage 1),
crying (events 100-102: `deac1d`, `deac1d`, `destr1`; mode 5, 0x413c66)
only if struck.
Struck and not dying, when its picture next ends it shows the hit
(state 4, 10 frames a tick each, then back to the state it was in),
crying one time in ten (a draw; event 0x6f `hc1`-`hc6` for the cyclops,
0x70 / 0x71 empty); throwing but not yet started it shows it at once,
without the draw. Dying, 2 ticks a frame (the minotaur's and amazon's 4
past frame 0xb); at the end it is removed (message 10: its jam left,
0x187a1 to its lair). Lightning (0xda93 through the jam, 0x414b0f): its
grid rebuilt; on the struck cell or with its half shut it dies (its queue
dropped first), else 25 points off the percentage and life that
percentage of the full, dying at 0; a death there is queued, read at
once, reported to the caster and the creature removed at once, without
its dying picture. Its sounds (0x50e570 kind 5 at its cell): appearing
0x59-0x5b (`app3`), walking 0x85-0x87 (`lc1`-`lc3`, `limon1`-`limon4`,
none for the amazon) when due, `((draw % 41 + 60) x 375) / 100` ticks
apart.

The port walks its creatures so (`scene/walkers.rs`, the route finding in
`scene/walk_path.rs`, checked against the original's routes on a fixture
grid), with their jams, looks about, throws, blows and deaths. The walk
grid at creation (0x42e720 with `param_4` 2, re-read in the
disassembly: a cell not land, nibble 5 at 1 or nibbles 6-7 not 0 all
-1, at 0x42e91a-0x42e92b and 0x42e9c2; else its shore sides by
0x5acf13; the 4 x 4 reopening at 0x42ea5e is for `param_4` 4 only) is
as ported. `[game +0x2ee5]` is the saved `RND_INIT`, else the time stamp
0x541840 at the game's creation (0x4ec433, 0x4ed18c, 0x53c04e): not
reproducible, the port draws it once from the scene's generator, as for
the ships'. A throw (0x412a79-0x412ac6) leaves from `+0xb4 - w / 2 +
T[h].x`, `+0xb8 - h + T[h].y` (T by heading: 0x594ae0 the cyclops',
0x594b20 the minotaur's, 0x594b60 the amazon's), `w` and `h` the
picture's `+0x61` / `+0x65`: for a sequence (type 7, 0x5461d0 at
0x5463bf) its `+0x29` / `+0x2b`, which 0x554bf0 (0x542940's type 7)
takes from its first frame's sprite header (`+4` width, `+8` height).
It aims where the target's vt+0xc gives with the hit byte; the distance
`+0x2d` is from the start to that point (0x5576d8). A ship's (0x4e07c0,
its own generator `+0x18`): a hit goes to the hull spot of its type and
heading (0x598ad8, as the flames') from its picture's top left, moved
`(r % 20 - 9)` times its heading's vector (0x5c4f4c), plus, with way
on (`+0x59b` not 0) and the move state 5, 4 or 1, `way x vector x 15 /
100000`; a miss draws `u = r % 6` and a second draw's bit 16 picks
the side, by heading (0 / 4: `x = w u / 5`, `y = h / 4` or `h - h /
6`; 1 / 5: `(w / 2) u / 5 + w / 2`, `h - (h / 2) u / 5`, or `(w / 2) u
/ 5`, `h / 2 - (h / 2) u / 5`; 2 / 6: `w - w / 8` or `w / 8`, and `w u
/ 5` down, the width as written; 3 / 7: `(w / 2) u / 5 + w / 2`, `(h /
2) u / 5`, or `(w / 2) u / 5`, `(h / 2) u / 5 + h / 2`). A building's
(0x423520; harpy nests' 0x41cef0): two draws of its own generator, `r1`
for x, `r2` for y, from its picture's (`+0x7c`) top left `+0x59` /
`+0x5d`: a hit `w / 3 + (r1 % w) / 3`, `h / 3 + (r2 % h) / 3` (a nest's
`((r1 & 1) + 1) (w / 3)`, `((r2 & 1) + 1) (h / 3)`), a miss a corner
(`(r1 & 1) w`, `(r2 & 1) h`), clamped to `0 .. 64 x map - 1`. A jam's
(0x415ce0): a hit its cell's centre with two members or more, else its
member's; a miss its cell's corner. The port ports the start, a ship's
aim (`Scene::ship_aim`) and a building's or nest's
(`Scene::building_aim`). The creature's picture stands up and left of
its place (`+0xb4`, `+0xb8`) by `+0xbc` / `+0xc0` (0x4111b0 by class: the
cyclops' 0x1e, 0x30, the minotaur's 0x1f, 0x35, the amazon's 0x1f, 0x2f;
0x411520 places it so at 0x411ac1 through 0x50d660, which writes the
picture's `+0x59` / `+0x5d`).

**A building's own generator** (`+0x18`; the base constructor 0x50b960
sets it 0). Message 2 seeds it `[game +0x2ee5] x id` (WorkLandObj
0x427f50 at 0x4281a2, TownObj 0x441ba0 at 0x442268, a harpy nest's
0x41d715; a save's own at type 2). Record type 3 (asleep, state 0x10)
draws the first look at once: `+0x1c0 = r % 15 + [game +0xde]`
(0x4281de, 0x4422ad), then looks each time `+0x1c0 <= tick`, `+0x1c0 =
tick + 15`. A fort (0x419f80, which runs 0x427f50 through 0x4e7640
first) draws once more at its message 2 for its stock's first dword
(0x41a895), and once at each cast it lets through (order 0x22, not busy
and some count: 0x41bc61), which Magic.cpp 0x42f580 takes as the
caster's draw (a reef's kind `r % 3 + 1`, a barbarian monster's `r &
3`); the sea temple (0x438300) likewise for kinds 1-4, 0xd, 0x11 unless
the order's byte `+0xd` gives it (0x438694); the war temple passes 0
(0x439aa2). The town's draw at 0x441e9c runs before its seed, from 0,
and leaves nothing. A fort's draw at 0x41a895 is its unit list's
generator: `[+0x4d0]` dword 0 = `r >> 16` (0 when the list is made,
0x41a204), for any record type but a save's 2. Barbarian structures
(0x3f-0x41) have the building generator the same way: their message 2
(0x40ef70) sets the class fields, then runs 0x4e8660 -> 0x4e7640 ->
0x427f50 (the seed, and the asleep look), then, with `+0xa4` not 0xff
and the record type not 2, draws once at 0x40f5a5 for `[+0x20c]`
dword 0 = `r >> 16` (0 at 0x40f100). Nothing else in Barb.cpp
(0x40de20-0x40fc90) draws `+0x18`: 0x40e0f0's and 0x40ff00's draws are
the store's (`[+0x20c]`), message 0's (0x40f62c) the unit list's. Their
vtable 0x594920 has vt+0xc 0x423520 (the aim) and their blows reach the
fire test, both drawing it. (The draws 0x4e8a5e-0x4e927a once given
here as barbarians' are in 0x4e89e0, 0x4e8af0 and 0x4e8db0, which the
ships' looks 0x4bfc60 / 0x4c0eb0 run on the ship.) Also drawn: a
burning blow's fire (below) and the fall's rubble, two a cell
(0x4244a4, 0x4244cb). The port keeps the generators
(`Scene::building_lcgs`, saved) and ports the first look, the fort's
and barbarians' draws (the unit lists' generators set by
`building_rng`, which `step_fort_draws` runs first) and the aim, the
fire's draw and the rubble's (below, `scene/burning.rs`). Not ported:
the sea temple's (the port's casts name no temple). Castles, lairs and
heroes' houses aim as buildings too (vt+0xc 0x423520 in their vtables
0x5949fc, 0x5953f0-0x595600 and 0x595e98, dumped) from the same
generator: a castle's seeded by 0x427f50 (0x410ca0 runs it at message
2) and drawn by its own code at message 2 (0x410e6e the first cast for
a record type below 2, 0x410ed8 the ambient sound, `start_castles`) and
in casting; a lair seeds it itself (0x414f00 message 2, `[game +0x2ee5]
x id` unless loading a save, no asleep look) and draws it every tick
(0x415337) and for the cell its creature goes to (0x4156a8); the hero's
house (0x421150 -> 0x4e7640 -> 0x427f50) never draws it. Ships' shots
and land creatures' throws aim at all of them by `building_aim`
(`shot::aims_as_building`, `Scene::throw_aim`; test
`throws_at_castles_lairs_and_houses_aim_as_buildings`). (0x430449 is a
reef's, 0x4300a0, not a building's.)


**A building's fire** (LandObjClassTy's drain 0x423d60, blow 0x138e,
0x4240e0-0x4242e6; castles and lairs reach it too, see Forts' units). After the damage (and the picture's change of
condition), the blow lights a fire when all hold, in this order: the
message's flaming dword `+5` is not 0; no magic shield (`+0x15c` 0);
the bomb kind (`+4`) / 3 is 1 or more; the building stands (`+0xa4` 1);
the shooter's player (`+0x15`) is not 0xff; the hit point (`+0xd`,
`+0x11`, less the main picture's `+0x59` / `+0x5d`) is a set bit of the
building's mask (0x557640: rows of `(width + 7) & ~7` bits, bit `(row
x stride + x) ^ 7`; the mask `+0xcc` and its width `+0xd0` are the
resource `<prefix>MSK_<nation>1<level>` (`%sMSK_%1u%1u%1u`, prefix
`+0xb8`, nation `+0x54`, the picture's damage level 3-1 pushed at
0x422f4d; reloaded when a blow changes the level), loaded by 0x422e30 in
state 1 through 0x542ee0 /
0x542d60(0x12); the forts' prefixes `FORTS`, `FORT`, `FORTB` set by
0x419f80 at 0x41a141, 0x41a113, 0x41a0e6; a town's by its own LoadImages
0x43bd60 (vt+0x38), `TMSK_%1u%2u%1u%1u` at 0x43be6c: nation `+0x54`,
class `+0x50`, 1, the same level, so `TMSK_15011`; ports, docks and the
barbarian 0x40 (its 0x40e0d0 jumps there; 0x3f and 0x41 to 0x422e30) by
0x4e7f90, `%sMSK_%1u%2u%1u%1u%1u` with the facing `+0x1dc` (below);
castles by 0x4101d0, `%sMSK_%1u%1u` at 0x4102d9: prefix, 1,
level, so `CSTLWARMSK_13`); and none of the four fire slots `+0x80..+0x8c` is
taken (the loop at 0x424153 gives up at the first slot in use, so a
fire is lit only where none burns, and always in slot 3, the last
index it passed). Then one draw of the building's generator (`+0x18`):
the fire is lit when `(r >> 16) % m == 0`, `m` 5 for kind / 3 == 1 (kinds
3-5), else 3. The fire is layer `slot + 3` (3-6; layers 7-10 are the
slots `+0x90..+0x9c`, which nothing in LandObj fills) at depth the main
picture's `+0x19` less 3 (0x50cbf0), drawn through 0x50e240(layer, 1,
table 0x5c1858, 1, 7) (0x55dd40: remap flag 1, value 1, count 7; the
table's bytes 00 26 26 25 26 26 26 01 ...), redraw 0x50e570(5, cell, 5);
its size byte `+0xa0 + slot` is kind / 3 for kinds 0-8 (mask 0x1ff),
else 2; the picture `FIRE<size>` (`FIRE%1u`, a sequence, type 7,
0x50cf60), its delay `+0x7d` 1, time `+0x81` 0, frame `+0x45` 0; its
top left the hit point less half the sequence's width and four fifths
of its height (`x - w / 2`, `y - (4 h) / 5`).

The building's tick steps it (0x425990 message 0, while the state
`+0xa4` is not 0xff, after the vt+0x4c drain and the layer-1 step): for
each slot i 0-3, the fire `+0x80 + 4i` (layer i + 3), when its time
`+0x81` + delay `+0x7d` <= the game tick (game `+0xde`): time = the tick,
0x50d660 shows frame `+0x45` at its `+0x59`, `+0x5d` (hidden on a cell
the viewer's fog byte is 0), frame + 1; at the sequence's count `+0x49`
the layer is removed (0x50cdb0) and the slot freed, so a later flaming
blow can light another fire (the lighting test reads only `+0x80..+0x8c`);
else, from frame count - 14 on with the smoke slot `+0x90 + 4i` free, a
smoke is started on layer i + 7: depth the main picture's `+0x19` less 2,
drawn through 0x50e130(L, 1, 0x26, 0x5c1358) (colour 0x26 as shadow),
`SMOKE<the fire's size byte +0xa0 + i>`, delay 1, time 0, frame 0, at x
`fire.x + fire.w / 2 - smoke.w / 2`, y `fire.y + (3 fire.h) / 4 -
smoke.h`. Then the smoke, re-read (a new one steps the same tick), steps
the same way and is removed after its last frame. With delay 1 a frame a
tick. The fall (0x138b, 0x4243a2: layers 3-10 removed, slots cleared) or
removal (message 3, 0x425990) ends both early.

The flames' colours: 0x55dd40 stores flag 0x100, table `+0x5c`, value
`+0x60`, count `+0x64` on the frame; the blitter 0x57b7f0 draws a pixel
of colour c with value <= c <= value + count - 1 as `table[(c - value) x
256 + the screen pixel beneath]` (0x57b80f-0x57b82b, 0x57b9c1-0x57b9dd),
the rest plain. So the fire's colours 1-7 blend with the ground beneath
through row c - 1 of 0x5c1858, a static 7 x 256-byte table in the exe
(no writer; rows 3 and 4 differ only at 186 and 191; dumped in the decode
notes, `layers_spec.md`; a ship's fire uses it too, 0x4e2003).

The fallen building's object. 0x423d60 on 0x138b sets `+0xa4` 2 and
`+0xa8` 1, takes the ruins picture (vt+0x38, which clears `+0xbc`) at
frame 0, and posts message 10 to itself at once only when it fell in
state 0 (going up: `local_1c`); it clears neither owner byte (`+0x54`,
`+0x55`). Every building class's vt+0x3c is 0x4230f0 (the 19 vtables
holding 0x423520 at +0xc, dumped), which steps the ruins picture as any:
the tick after the fall (it runs before the drain in message 0), then
every `cycle[2] / n` ticks (75 for player buildings, the barbarian
structures, the lairs (0x5953e0: 200, 75, 75, 75) and the hero's house
(0x595e80: 50, 75, 75, 57); castles 100), the frame drawn and one added;
the step reaching `n` (case 2) posts message 10 to itself (0x4232ed,
0x542670 -> 0x541e10, the posted queue drained after the tick), resets
the frame and redraws (0x423a60, 0x423b60). So the object lies fallen,
owner kept, for `1 + (n - 1) x (cycle[2] / n)` ticks (a town's
`TSPR_33_21`, 21 frames: 61) and is then removed; its removal runs
LandObj 0x425990 message 3, which for a player's object (`+0x57` 1, and
0x541a30(0x1c57) returning 0: not decoded) sends 0x5d9b `{id, class,
+0x54, +0x55, x, y, +0x1bc}` to the owner's player, the computer
clearing the base that holds it (0x484b20 / 0x482c50). The rubble
(below) stays. The port: `Scene::ruins` (saved), stepped first in each
tick (`step_ruins`) and removed before the tick's delivery
(`remove_ruins`); the computer frees a base when its town's object is
gone (`computer_news`).

Who sees the fallen building in that window. At the fall 0x423d60 (at
0x42439d, all but forts 0x34-0x36; a fort's drain 0x4184f0 on 0x138b at
0x418cfe / 0x418d48) calls 0x50e750 with the killer `+0x1ac`: a direct
send (0x542630) of 0x1c57 message 0x83 to the navy, whose 0x4a8410
counts the loss and gain (0x4a9250) and takes the building out of its
player's list (`[0x5c7508] + player x 0xba0 + 3000`, 0x4a8460 ->
0x55b500) and out of its group (0x49bef0, 0x49ec00 on `[0x5c7504]`).
So every walk of that list stops seeing it at the fall: the judge's
counts (0x448610 -> 0x4c46c0 -> 0x4c4610, which test no state, only the
class masks), 0x4a8620, the ports' search 0x4c5910. The radar icon is
not touched at the fall (0x50de40's callers are the phantom's 0xd21 and
message 3's at 0x42686a, `+0xa4` not 0xff): it stays, in the owner's
colours (0x50da60: `+0x54 / +0x55` for `+0x57` 1, taken when made),
until the removal. The port keeps its ruins ownerless (`Scene::fallen`:
no combat, no owner), which is what the list's walkers see, and the
owner with the ruins (`Construction::owner`) for the minimap's icon
(`render::minimap_icon`); tested: battle01's last building falling is
a defeat at once, its ruins standing.

The pointer over a fallen building. The hit test 0x423460 still picks it
in state 2, and none of its readers test the state. The info: every
building class's vt+0x18 (the 18 vtables, dumped) is 0x423990 (town
0x43c730 calls it; forts 0x417dc0, strongholds 0x40e7f0 add their
garrison), which after the base 0x50e7b0 (`+0x50`, `+0x57`, `+0x54`)
shows life `+0xa8`, 1 since the fall (a bar of 1 x 67 / 100 = 0 pixels),
and the hero and treasure icons as standing. The pointer with nothing
selected (0x5175b0, `[game+0x3221]` 0) gives mode 5 (`CUR_OWN`) over an
object whose `+0x57` is 1 and `+0x54 / +0x55` the viewer's, else keeps
the pointer it had; a selected building answers 0xc452 the same way
(0x425990: itself 6, an own object 5). A ship's answer (0x4dc630 state
0, 0x4dd424) tests the bytes too: own, by class (port 0x39 0x4e4680,
dock 0x3b 0x4e4630, 0x3a / forts 0x4e6700(0x1f), else 5); an enemy
(0x4efe20 < 0) 0x4e45c0, whose fire bit (0x4e35e0 with 0: any weapon
position, no target test) gives mode 8, attack; vt+0x2c (0x40dfc0,
`+0xa4` != 0x10) is true in state 2. A plain click in mode 5 sends 0xc453
to the navy (0x5180e0; 0x49ab30 selects it, 0x49c2f0, and
passes 0xc453 to the object), whose 0x425990 draws the selection frame
only out of states 2 and 0xff. The port: `Scene::kept_owner` (the
owner kept with the ruins) for the pointer's own test with no own ship
selected (`App::pointer`), the minimap and a fort's levels in the info;
the info's life 1 for ruins (`object_info::of`); tested on battle01.
A ship's pointer over ruins: own ones the own pointer, an enemy's the
attack (greyed unarmed), stepping as at a standing enemy
(`Scene::ruins_enemy`, `App::default_pointer`, `App::pointer_modes`);
The answer's word `+0x1a` is a mask of modes: 0x5175b0 shows, when it
is not 0 (0x4000 is the grey, taken off), the mode of its lowest bit
(bits 0-13: 5, 7, 9, 8, 0xa, 0xb, 0xf, 0xd, 0xc, 1, 4, 2, 3, 0x14), its
`+0x18` only when the mask is 0. For an own target (0x4dd486, no state
test): port 0x39 mask 1 | 0x4e4680 (0x180 for a ship whose `+0x25a` is 0
or 1 with a byte of `+0x47f`-`+0x481`, else 0x100); dock 0x3b 0x101 |
0x4e4630 (0x40 with `+0x261` bit 2, unless `+0x271` is 0x4ac110's answer
and 0x4b8cb0 true); a class whose 0x50e600 is 0xc8 0x101 if
0x4e6700(0x1f, 0) allows, else mode 5; 0x3a and forts 0x101; else mode
5. So own port, dock, yard and fort ruins offer, as standing ones, own
first, then by the right button modes 0xf, 0xd and 0xc (reload); the
port's `pointer_modes` offers them over own ruins (`Scene::own_ruins`).
Their orders at ruins (the click 0x5180e0: mode 0xc order 0x1f `{id,
2}`, 0xd 0x1f `{id, 1}`, 0xf 0x1e `{1, id}`; the ship's 0x4e2ba0):
- Repair (0x1e): 0x51f1f0 finds the ruins by id, event 0x20 `{0x101,
  cell}`; the order 0x4aa990 (kind 1) takes its berth from 0x4b23a0,
  which reads the object slot (grid `+0xc`) of that cell, and the fall
  emptied it (0x423d60 on 0x138b -> 0x499860, one cell or four by
  `+0x56`): -1, the order dropped, no notice. A dock falling while a
  ship sails to it: at the berth 0x4b2420 finds the slot empty, the
  order done (message 8, 0xa0) with notice 0x143, as a dock gone. The
  port: `send_to_dock` drops at ruins silently, `dock_tick` ends on a
  fallen dock.
- Unload (0x1f kind 1, cargo aboard): event 0x40 `{400, 0xc, id}`,
  0x4aab00 looks the port up in the ship's player's list (0x4ac050),
  which lost it: dropped, the old task kept, no notice
  (`order_unload`).
- Load (0x1f kind 2): event 0x10 `{3, 1, id, 0xff}`; the order 0x4aa840
  finds the ruins by id (0x4ac050 with -1), measures (0x4abf60) and
  starts the closing in (`+0x696` 3: 0x4b7370 -> 0x4b73a0 -> 0x4b7f10
  with the kept pointer `+0x6cf`, no look-up). Near at the order
  (0x4abf60 within 0x59aee8 / 2 = 2: 0x557440 to a port's or dock's
  berth vt+0xb8, else to the anchor `+0x74 / +0x78`) it closes in
  (`+0x6a1` 1) with `+0x6c6` the ruins' player; far, it sails (3) with
  `+0x6c6` 0xff. Closing in (0x4b73a0 state 1) looks with 0x4b7f10(5,
  pointer, id, .., 1) in the cells' object slots, which the fall
  emptied: not in the box (0xff), so `+0x6a1` 3 and `+0x6c7` -1; the
  contact 0x4b84b0 (the hook for a ship, or for a building the contact
  whose state 3 sends 0x2875, 0x4b3930) and the ruins' 0x425990 on
  0x2875 are never reached. Sailing (0x4b7370 -> 0x4b5d10, on a new cell
  or held up): 0x4b9140(2, -1, `+0x6c6`, id) looks the ruins up by
  `+0x6c6`: by player (0x4ac050 -> 0x4a8710) the list has lost them,
  notice 0x13c `{id, 0x13c}` (0x4a1900) and the task stopped (vt+4(8,
  0)); by id (-1, 0x51f1f0) found while they last (removed by message
  10, 0x4232ed: then lost likewise): 0x50ebf0 gives the berth (vt+0xb8,
  0x40e050, for building classes below 0x44 with the flag), and the
  berth and the anchor (vt+0x28, 0x40dfa0) both within 2 (0x557480)
  close in again (1), else a plan 0x4a6270 mode 5 to the berth. So a
  ship far at the order sails to the berth and lies there, the two
  states turning, never linked, until another order; one near ends at
  once with notice 0x13c. The berth 0x40e050: anchor plus 0x5c4f4c's
  step for the facing `+0x1dc` (-1, -1 below 0); a fort's facing
  (0x419f80 message 2, after the weapons): 6 when the cell below its
  anchor has landscape low nibble 7 or 1 (`grid + 8`), else 0 the cell
  right, 4 left, 2 above (each inside the map), else kept: 0x427780's
  -1, since the fort's constructor 0x417810 (vtable 0x59592c) sets
  `+0x1d8` 0x20 without the flag 8 that takes record field 10 at
  0x4280e4 (a port's, yard's or dock's 0x40ef70 sets 0xa, with it), so a
  dry fort's berth is (-1, -1). The port: `order_load`
  takes own port, yard, dock and fort ruins as `Task::LoadRuins`,
  `Scene::load_ruins_tick` (`Closing::by_id` the `+0x6c6` 0xff),
  `Scene::vt_berth`. Tested on miss000.

The click's refusals. The click (0x5180e0) sends the mode's command
(0x518db0, modes 8, 10, 11, 12, 13, 15 by the selected ship) and shows
no text; the ship's 0x4e2ba0 cases 0x15-0x18, 0x1e and 0x1f return
silently when 0x4e6700 refuses or the target is not found, and their
orders (0x4aa990, 0x4aab00, 0x4aa840) drop what they cannot do with
no message (the only words are the prey notices to a group). The
port's `App::act` (attack, board, ram, repair, unload, load) so shows
none of the scene's refusal strings.

A fort's attack at ruins. The fort's answer to the pointer (0x41bec0,
in 0x419f80 on 0xc452) tests the bytes: an own object 5; standing, not
limed, `+0x1e4` 0 and the cell seen, an object of `+0x57` 1 at war
(0x4efe20 < 0) goes to the reach test (0x446800 over the groups with
units: a cell test, no state), so an enemy's ruins in reach get the
attack. Order 0x15 / 0x16: 0x446960 finds them by id (0x51f1f0) and
sets the groups' target with bit 2. Each group's tick (0x4469c0) asks
0x447400 whether the target is in reach: it looks for the id in the
cells of the reach mask, passing over a building of classes 0x32-0x3e
(mask 0x1fff) in state `+0xa4` 2; so the ruins are never in reach, the
target is kept and nothing is shot at it (the scan, bit 1, as before).
The port: the fort's pointer (`App::default_pointer`) and
`Scene::order_attack` take an enemy's ruins (`ruins_enemy`); their
groups never track them (the port's ruins have no `combat`, so are in
no group's cells). Tested on miss000 (repair, unload) and miss007 (the
fort).

The pointer with nothing selected (0x5175b0, `[game+0x3221]` 0): over
no object mode 0; over an object whose `+0x57` is 1 and `+0x54 /
+0x55` the viewer's mode 5; over any other object it jumps to its end
(0x517a16) and sets no pointer: the one shown stays. The port keeps the
last pointer shown (`App::last_pointer`) there with the original mouse;
the modern mouse, whose click selects any object, shows the select
pointer. Tested on battle01.

The attack order at ruins. The click's order 0x16 `{id, 7}`
(0x5180e0 mode 8 -> the navy -> 0x4cfc90, 0x520cc0) reaches the ship's
0x4e2ba0 case 0x16 (0x4e6700(0x16) first), which keeps
fire records of kind 3 with the player byte `+0x10` 0xff. Its tick's
0x4ab3f0 finds the target by 0x4ac050 with -1: by id among all objects
(0x51f1f0), so the ruins are found; their `+0x57` is 1, so the bytes are
tested: own or not at war (0x4efe20 >= 0) leaves the record untaken;
else 0x4abf60 asks only a distance (0x557440 to the building's
`+0x74/+0x78` cell), and the positions go to state 3 with the player code
`+0x644` the kept owner's (`+0x55 + 4 x +0x54`). The fire pass's attack
0x4c09c0 then calls 0x4b9140 -> 0x4ac050 with that code: the player's
list, which lost the building at the fall, so it is not found, and the
ship stops (vt+4(8, 0x180)) with notice 0x13a `{id, 0x16}`. So the order
is taken and dropped at the first ready step, before any route; an attack
running at the fall ends the same way. The port: `Scene::order_attack`
takes an enemy's ruins (`ruins_enemy`) with the kept code, and
`fire_attack`'s look (`find_by_code`) drops it; tested on miss002.

The left click selects only own objects. 0x5180e0 on message 0x62 with
an object under the pointer sends the mode's order (modes 1-4, 8-0xb
with something selected `[game+0x3221]`; 7, 0xc, 0xd, 0xf, 0x14, 0x1e,
0x28, 0x32 their own); only over an object whose `+0x57` is 1 and `+0x54
/ +0x55` the viewer's does mode 5 select (0xc453, 0xc455 with Shift,
0xc456 with Ctrl, to the navy 0x49ab30). Over another's object, or none,
the standard pointer's click sends nothing (after the own test: `+0x3221` 0 or
mode 0) and the selection is kept. Own ruins (bytes kept) are selected;
an enemy's, a deposit or a treasure are not. The port's original mouse
(`App::finish_select`, `kept_own_pick`); the modern mouse still selects
any object for its info.

The panel with selected own ruins: 0x4fffe0 on 0xc453 (the selection,
`+0x14` not 0) asks the navy 0xc45a and makes windows 0x22c and 0x22d (0)
or 0x22e. The menu is the object's vt+0x68 through 0x425990's 0x10a0.
The 19 building vtables (holding 0x423520 at +0xc): 11 have 0x423a00,
which only sets slot 0 to the cancel (0x102) for the main menu in state
0 or while mended (`+0x10c`), and clears the enabled bits under `+0x144`:
in state 2 an empty menu, as the port's (`commands::main_menu`, the
ruins not `is_own`). The other eight (vtable, its constructor's store,
class) fill nothing of their own outside state 1 (the town's also
class 0x33 in state 0): the yards' 0x4164e0 (0x5957c0, 0x41629a,
classes 0x3a / 0x3b: its berth slots need state 1, the upgrade slot
class 0x3b in state 0 or 0x3a in state 1), then 0x423a00 in state 2;
the fort's 0x417ff0 (0x59592c), 0x423a00 unless mended (`+0x10c`, job
`+0x110` 0xff); the school's 0x430f90 (0x596af0), the smithy's 0x433e60
(0x596c10) and the studio's 0x4361c0 (0x596cf8), 0x423a00 unless busy
(`+0x10c`; the smithy and studio only with `+0x144` 0 and, the smithy,
job 1-3); the temples' 0x437de0 (0x596df8) and 0x438b80 (0x596ef8),
0x423a00 only when not busy; the town's 0x43c740 (0x597018), 0x423a00
in state 2 only with `+0x144`. So a fallen building's menu is empty but
for 0x423a00's cancel (0x102 in slot 0), which needs `+0x10c` set: the
fall (0x423d60 on 0x138b) does not clear it (its vt+0x38, 0x422e30,
0x4e7f90 for yards, 0x43bd60 for the town, redraws the picture), so a
building busy at its fall would offer the cancel in its ruins. Not
decoded: whether anything clears `+0x10c` in state 2 and what the
cancel does there; the port shows ruins no menu (`main_menu`).

A building's fall (0x423d60 on 0x138b) shows no blast: the ruins
picture (vtable +0x38, frame 0), its fires and smokes out, the rubble
below and, while the site has cells (`+0x158`), its `+0x1a8` sound
event through 0x50e570 mode 5 (4 `razru2`, 0x422dec; the castles
0x410ca0 set 0x6c for the sea castle 0x42 at 0x410d78 and 0x6d for the
war castle 0x43 at 0x410d3a, whose `SND_` lists are empty; no other
building class writes `+0x1a8`: the other stores to an object's `+0x1a8`,
0x456xxx, 0x45b3c0, 0x476c05, 0x499ee2, 0x55d774, are other objects'
fields). The port plays `razru2` but at a castle (`Scene::strike`). The port drew
`BOMBG10` there (and at a ship with no sinking picture): gone.

The fall's rubble is a terrain tile on map layer 5. For each cell i of
the site (`+0x56`, 1 or 4: x + i % 2, y + i / 2) two draws of the
building's generator give k = r % 3 + 1 (0x4244a4) and s = r % 3 + 1
(0x4244cb), and 0x527fe0(the `b_MAPA` grid `+0x17b2`, x, y, k, s)
writes, only on a cell whose landscape dword (`[0x5c74fc] + 8 + 24 x
cell`) has layer 0 land (nibble 8) and layer 5 empty (nibble 0) and
whose layer-0 variant is in one of the 19 groups of 0x5c2480 (0x527eb0:
1 `01`, 2 `02-05`, 3 `06`, 4 `07-0a`, 5 `0b`, 6 `0c-0f`, 7 `10`, 8
`11-14`, 9 `15`, 10 `16 17`, 11 `18 19`, 12 `1a 1b`, 13 `1c 1d`, 14 `1e
1f`, 15 `20 21`, 16 `22-24`, 17 `25-29`, 18 `2a`, 19 `2b`), the layer-5
cell {variant = the group, tile set `0x22 + 3k + s`} and sets the layer-5
nibble to 2. Tile sets 0x26-0x2e are `b_vor1_s`, `_m`, `_b`, `b_vor2_s`,
`_m`, `_b`, `b_vor3_s`, `b_vor2_m` (0x2d: the record's prefix as stored,
class `o_vor3_m`), `b_vor3_b`, 19 tiles each: k picks the set, s the size,
the ground's group the tile. The message 0xcc2 {x, y} goes to every mapa
(class 0x2f4, 0x51cdd0 -> 0x519aa0), which clears the cell's square in
its window and redraws its layers 0-7 in order (0x5199b0 -> 0x519750 /
0x5198b0 through 0x545200), the ordinary terrain draw.

Ported in `scene/burning.rs`: a shot's blow on a building (`land_shot`)
carries its landing point, item (as the bomb kind) and flaming flag to
`light_building`; the tick steps the fire and smoke
(`step_building_fires`, game tick `Scene::ticks`); the fall in `strike`
puts them out and lays the rubble (`lay_rubble`, through the dig's
`after_dig`, so the terrain, minimap, cell types and saves follow). The
table 0x5c1858 is `render::FLAME_BLEND` and the groups 0x5c2480
`burning::GROUPS`, both dumped from the exe and tested against it. The
blend needs the palette index beneath, which the port's frame (colours,
not indices) does not hold: it takes the first index of that colour, or
the nearest for one no index has (a darkened shadow). Forts and towns
burn the same: a fort's drain 0x4184f0 reads 0x138e and leaves it in
the queue for 0x4e7550 -> 0x427bb0 -> 0x423d60, a town's 0x43b290 takes
only 0x138a-0x138c and leaves the rest to 0x423d60; their generators are
the kept ones (above). Castles burn the same (their drain 0x4105c0
-> 0x427bb0 -> 0x423d60) on `CSTLSEAMSK_1<level>` / `CSTLWARMSK_1<level>`
(0x4101d0 at 0x4102d9), their generator kept (castle.rs, below), only
in state 1: casting (4) they take none. Lairs burn the same (their
drain 0x414ef0 jumps to 0x423d60) on `<CLPF|MNTF|AMZF>MSK_41<level>`
(0x422e30, nation 4), the draw of their generator: the building one,
seeded `[game +0x2ee5] x id` at message 2 (0x414f00; a save's from its
record), drawn every tick by their tick and once per creature sent (see
Lairs, below) and by the aim at them (0x423520, above). Ports (class
0x39, vtable 0x596990, prefix `PT` 0x5ad098) drain by 0x4e7550 (the
ShipServiceObj drain: 0x138b sends 0xcb9 to `+0x1e0`) -> 0x427bb0 ->
0x423d60, shipyards and docks (0x3a, 0x3b, DockClassTy vtable 0x5957c0,
prefix `DK` 0x5a2cf0) by 0x416340 (0x138a, the dock upgrade) -> 0x4e7550
-> ...; both take the blow in by 0x423c30 (the dock's vt+4 0x4162f0
calls it first), which queues 0x138e unless `+0xa4` is 2. So they burn
the same, by the building generator `+0x18`, on the mask 0x4e7f90 loads
in state 1 (0x4e80b5): prefix, nation `+0x54`, class `+0x50` (`%2u`),
1, facing `+0x1dc`, level (edi, the picture's: 3 in states 0 and 3,
else the condition's, at least 1), so `PTMSK_157101` (nation 1, facing
0, level 1) and `DKMSK_<nation><58|59>1<facing><level>`.
The test's state (0x42410e, `+0xa4 == 1`) holds for none going up (0),
fallen (2), sold (3) or asleep (0x10), and holds for a building being
mended (state 1, see below); nothing wakes an asleep building
on a blow (0x423c30 and 0x423d60 do not touch 0x10), and none is hit:
its hit test vt+8 0x423460 is false in 0xff and 0x10, so a shot lands
elsewhere. The barbarians' drain 0x40e0f0 reaches 0x4240e0: it writes
the structure's rest back (0x55b440, the message kept) and ends in
0x4e7550 -> 0x427bb0 -> 0x423d60. Their masks: vt+0x38 0x40e0d0 jumps
to 0x4e7f90 for class 0x40 (prefix `BARB` 0x5a2910 set at 0x40f014; the
facing `+0x1dc` record field 10, loaded by 0x427f50 at 0x4280f2 as
0x40f038 sets `+0x1d8` 0xa), `BARBMSK_464<1><facing><level>`, and to
0x422e30 for 0x3f and 0x41, as lairs (`BARBSMSK_41<level>`,
`BARBBMSK_41<level>`); `building_mask` names them. They burn as the
others, by a draw of their building generator (0x427f50's seed, see
the building's own generator), standing (state 1). Not ported: the classes with no mask resource would read a null mask in
0x557640, the port lights none on them; whether a killing blow's state is still 1
at the fire test (the port lights none on a building its blow brought
down); which of fire (depth - 3) and smoke (- 2) draws over the other
(the port draws the fire over); the fog's hiding of the frames at their
step (the port draws them with their building).


**The pointer on a jam.** The cursor (0x5175b0) asks the view (message
0xc44f) for the object under it: 0x49a580 (point, its cell, and the
cursor's `+0xbd`, which the view's activation sets to 1 in the main view,
`+0x58` 1, and 0 in the other, 0x51e436-0x51e46c) tries, each by its
vt+8, the pointer cell's flyer slot (`+20`: harpies and dragons,
0x1f8 / 0x1f9), its `+28` slot (kind 700, not 0x2c1), the flyers and
class 0x25b in the cells about it, the cell's occupant (`+12`), with
`+0xbd` the occupants of the cells about it, then its `+16` slot (not
0x2c1) and the next cell's. The cells about are 0x499ff0's lists by the
map's edges (0x49a1d0): inside, north-west, north, north-east, east,
south-east, south, south-west, west; on an edge or in a corner the cells
on the map from east (top edge and top-left corner), south (right edge,
top-right), west (bottom edge, bottom-right) or north (left edge,
bottom-left), round that way. A jam's vt+8 (0x415c70) answers 1 at once
when `+0xbd` is 0, else when any member's does (0x411b50: not dying, the
point within its picture's `+0x59`, `+0x5d`, `+0x61`, `+0x65`). So the
pointer picks the jam, never a creature: the info strip and line are
left as they were (0x516c10 sends nothing for type 0x1f7), and an order
names the jam (a fort's turns it into its first member, 0x415df0). The
port draws them so, picks a jam over its members' picture rectangles in
that order of cells (`Scene::pick_cells`, `Scene::jam_at`; the other
occupants there by their pixels, as the port's other picks), giving the
jam as its first member, and shows nothing for it.

**A ship's order at a jam.** Over a jam the plain pointer offers an
armed ship the attack only (0x4dd549 -> 0x4e46c0: capability `+0x261 &
0x800` and a weapon, mode 8; no range, fog, side or alive test; never
board, ram or fire mode 9), and the click sends 0x16 {jam id, 0, 0xff,
-1, -1, 0} (0x51884c), the navy (0x49e230 -> 0x49dda0 -> 0x4d04a0)
making the fleet attack task {0x74, 0x15}: `+0xc` the jam's id, `+8` the
game's tick `+0xde`, `+0x18` 7, `+0x10` 0 (0x50ebf0 has no cell for
0x1f7), and 0x4cfc90 sends each armed ship 0x16 {jam id, 7}. The ship's
tick (0x4ab3f0, 0x4ab5a4) turns the jam's id into its first member's
(0x415df0) and attacks that. The task's tick (0x4d0650, from 0x4c4f40)
while `+8 > 0x50` (unsigned): `+8 = 0` and every ship of the group lying
still (`+0x58c` 0) without `+0x25d & 0x100` or `& 0x10` is sent at the
jam again (0x4cfe40); then `+8 += +0xe2`. So with its member dead the
ship is sent at the jam's new first member within 81 ticks; with the jam
gone nothing is given and the task stays (nothing in 0x4d0650 ends it).
Shots hit only a jam's first member (0x4983d0). The port: the pick
names the jam (`App::attack_target`), `Task::Attack` keeps it (`jam`,
`count` the task's `+8`, per ship), `Scene::run_jam_attacks` is
0x4d0650 (the member given at once, not through the next tick's
command), and the ship waits while its member is gone. Not decoded: who
removes the task (0x4d0210's notices, 0x4a1c60's callers); a fleet task
with a route `+0x61` (0x4ceab0 with the cell 0); the at-will pick of a
jam (0x4c0cd0, 0x4c0bc0). Not ported: message
0xda96 (life off by the message's amount); several sounds to pick from are picked by the port's
stand-in for the C library's `rand()` (`gull_rng`). Not decoded:
0x55b560's back-trace when its first step finds no cell (it
writes its uninitialised best cell, `[esp+0x28]` / `[esp+0x38]` read at
0x55b809 and 0x55b9c1: the port takes it as no route); a destination
list whose weights are all 0 (0x413622 divides by zero; the port's walk
grid opens only halves of land cells, low nibble 8, which 0x42eb20
weighs 10 if its tile type 8 is that nibble: not proven, the port
stands).

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

A leviathan (`WhaleClassTy`, Grig\whale.cpp: messages 0x494f90, tick
0x494b90, states set by 0x494400; `scene/leviathan.rs`) holds 100 x field 4
(`+0x78`): 100 in every mission, so the manual's 5000 amber and 2500 fish.
A net takes 50 of it a cast (0x4ad0e0, through its vt +4 mode 1,
0x494090, which takes only in state 3).

Its record (message 2, mode field 12 = 0 in every mission): field 5 full
life (`+0x7c`), 6 life (`+0x80`, and `+0x90` the life it comes up with
again), 7 and 8 **its start cell** (`+0xa9`, `+0xad`), 10 and 11 the reach of
its bounds about that cell (`+0xb1`-`+0xbd`: the cell -/+ them, clamped to
the map; either below 1: the whole map), 13 its move weights (below 9: row
f13 of the 9 x 25 table 0x5ba958, else fields 14-38, each at least 1), 39
the times it comes up again after a catch (`+0x84`), 40 how soon (`+0x88`),
41 the treasures it carries and 42-51 their five dwords each (two places,
`+0xc1` / `+0xc5`). Its generator `+0x18` is seeded `game +0x2ee5 x id`.
A start cell 0x48ed40 refuses: it goes (message 10). It lies in its cell's
leviathan slot (`+0x10`, the slot the placement map 0x40b710 and the hunt
read). 0x48ed40(self, x, y, free, room): landscape low nibble up to 7 and
nibble 1 up to 3; with `free` no ship in the occupant slot (0x499900), the
`+0x1c` object empty (or, with `room`, one whose class is its family's,
0x50e600: a treasure, 700), the `+0x18` object without a picture (0x403010,
vt +0x30: a sea monster's `+0x139`); the slot empty or itself.

States (`+0x74`), each start resetting the loops `+0x111` and the start
tick `+0xf9`:
- 0 under water, picture gone, a wait of `r % 51 + 25` ticks. Then, with
  life, a move (0x4941f0: the 5 x 5 cells about it, dx outer and dy inner,
  within its bounds and 0x48ed40 with room while it carries fewer than
  [0x5c112c] = 2, weight `[dy x 5 + dx]`, x 10 onto a treasure; picked by
  0x495e80: one choice no draw, else `r % (sum + 1)` against the running
  sums; the heading `+0x98` by 0x494140, east 0 counter-clockwise, kept
  for the same cell) and state 1, or 0 again; with no life and `+0x119`
  (set on coming up again) a move and 2; with neither, off its slot: gone
  once it has come up again `+0x84` times, else 5.
- 1 shows itself a loop of `WHALE_0<heading>` (a composite: layer 0 the
  body, 1-3 its wounds below 76, 51, 26 percent of its life, 0x493ac0), a
  frame every 2 ticks (1 with an odd heading), sound event 99 (`kit4`)
  with an odd heading; it takes the treasure on its cell's `+0x1c` while
  carrying fewer than [0x5c1140] = 2 (0x158c1). Then 0.
- 2 comes up (`WHALE_10`, a frame every 2 ticks, sound 110 `prote1`), then
  3 up (`WHALE_20`, every 3 ticks, `r % 41 + 40` loops; what it carries
  falls at its cell, 0x520c70); struck while coming up (`+0x11d`) no loops:
  4 at once. Up, its amount below 1 ends it at the end of the loop, `+0x119`
  cleared. 4 goes down (`WHALE_10` backwards, every 2 ticks), then 0.
- 5 caught: a wait of `27750 - 3000 x field 40` ticks (field 40 clamped to
  0-9; 2 in most missions, about 14.5 minutes), then 6: a tick at a time,
  up to 100, one draw of x and one of y in its bounds, its cell taken
  before the test, 0x48ed40 with no `free` and an empty slot: amount and
  life whole, `+0x119` set, shown + 1, in the slot, state 0; after 100
  tries 5 again.
Pictures (0x493b00, 0x493a20 `WHALE_%1d%1d` of (state 1: 0, 2 and 4: 1,
3: 2) and the heading, zeroed in 2-4) lie at the cell's corner, 5 lower
but in state 1; 0x493f10 draws the frame when `now >= last + delay` (every
tick while `+0x11d`), then steps it, past the loop's end back to its first
and a loop more.

Blows (only while it shows a picture, states 1-4, but lightning): lightning
(0xda93, not in 5 and 6) takes its life to 0, or with none its amount; on
the struck cell it goes down (0x494400(0)), elsewhere it hurries. A
tornado's blow (0xda96, 30000 to each slot of the cell, 0x43a6c0) takes
that from its life, or with none its amount; a reef's or whirlpool's not
under water (0xda94 / 0xda95, each of their ticks) its amount when it has
no life; each hurries it (`+0x11d`). A shot's blow (vt +4 mode 0) in state
1 only: life less the blow, sound 121, a kill report below 1.

So a leviathan of life 100 must be struck down twice before it can be
netted: with no life and no `+0x119` it is caught (5) and comes up whole
with `+0x119`; struck down again it comes up to be hunted (2, 3).

Its hit test (vt+8, 0x495a60), for shots and the pointer: its picture
shown, the point in the picture's rect (the cell's corner, 5 lower but in
state 1; the picture's size, 64 x 64), then the bit of the frame's mask
(`+0x45`, the last mask past their count) at it, rows `(width + 7) & ~7`
bits, the first bit the highest (0x557640). The masks `WHALEM_<row><heading>`
(kind 11, the masks' name prefix and their count as the entry's width:
`WHALE_MSK_<h>` 22 on even headings, 27 on odd; `WHALE_M1_MSK`,
`WHALE_M2_MSK` one) name kind 18 masks by the prefix and two digits, 64 x
64. A shot landing on it (0x4983d0, the cell's `+0x10`; 0x4967b0 the same
in flight) gives vt+4 mode 0 `{player, (0x598100[letter] x power / 100) x
100 / 10000, shooter}` (0x498240, family 400: 0x598100 = 60, 0, 60, 60,
60, 40, 40, 44, 45, 0, 0, 0 by letter): in state 1 its life less that,
sound event 121 (`SND_121` lists none); below 1 life a kill report
(0x50e750) on every such blow.

### The hunt

Commands 0x1c / 0x1d pass 0x4e6700 with capability bit 0x40, which every
finished ship has (0x4e6930: all but 0x800 for types 0 and 1, all but
0x100 for the rest): any ship hunts, warships too. 0x4aab00 (class 0x192):
the leviathan `+0x78a` / `+0x77e`, step `+0x77d` 1, the box `+0x7c2..`
its cell -/+ 2 clipped to the map's last cell, the first plan at once
(`+0x593`, `+0x57b` 0, `+0x57f` 25); sub-code 9 when the box's first
corner lies more than 0x59af48[type] / 2 = 2 cells off (0x557440), else
10. A game tick at a time (0x4ac2d0; `scene/hunt.rs`):

- **Sub-code 9** (0x4ad680, step 1): its cell (vt+0x28, 0x495ff0: none in
  states 5, 6, 0xff, nor the leviathan gone: notice 0x138 with class 0,
  0x4a1430, every tick); within 2 cells (0x557440) sub-code 10; a move to
  its cell (0x4a6270 mode 5, tolerance 1). Behaviour moves (0x4a6150):
  the first after the order at once, then once 25 ticks have passed since
  the last plan if the ship changed cells, the goal changed or it lies
  still, not while turning; on the goal the move ends (event 8 0x80).
- **Sub-code 10** (0x4aeef0), step 1: 0x4ac8d0 over the 5 x 5 cells
  (0x59af48) about the ship (the cell it is going into while between
  cells: `+0x596` and move state above 3, `Ship::under_way_into`), clipped to the map, row by row:
  the hunted leviathan in a cell's slot `+0x10` with its picture shown.
  Showing itself (state 1) it counts at the ship's own cell, distance 0,
  and the ship's guns turn on it (0x4ac5d0 sets groups 0 and 1's targets,
  `+0x5d1` 1, 0x4bf9b0, 0x4af6a0); in states 2-4 at its cell, and a ship of
  the hunter's player lying there not hunting a leviathan is asked to make
  way (0x4b4b50 with 0x5a; another player's not, 0x4efe20). Not seen
  (under water, caught): sub-code 9. Seen further than the reach (0 for
  types 0 and 1, 2 for the rest): a move there (0x4a6270 mode 6, inside
  the look's box: "Region plans"). Within
  reach and lying at a cell: if it is up (3) and the ship a warship, the
  ship moves off when the leviathan is in its own cell's slot (0x4b49a0:
  a cell of the three by three about it, clipped to the map's last cell,
  0x499aa0 with the direction 0x59791d and no leviathan in its slot, one
  draw of the ship's generator; event 0x80 priority 0x32) and its task
  ends (event 8 0x40; none in the slot, 0xc0); else a boat or merchant
  lying still (move state 0, no contact `+0x8e6`) on the cell of a
  leviathan up begins: 0x4aed30, its commands held (0x4b37d0: `+0x25d`
  kept and 0, `+0x261 &= 0x2000`), step 10.
- **The netting**: 10, the turn (0x4aed30: on heading 0 or 4 none; else
  the nearer of 0 and 4 by 0x4b5390, the headings' difference wrapped to
  -4..4, 0 only if strictly nearer), a turning frame (0x20-0x37 of the
  hull picture, wrapping) a tick, the heading `(frame - 0x20) / 3`, till on
  0 or 4 at a heading's frame; 0xc, the line `WMOST_` (picture slot 1,
  activity `+0x75` 4; 9 frames) down a frame a tick; 2, 0x4aee40: the
  hunted leviathan up in its cell's slot: the cast takes `min(50, amount)`
  (0x4ad0e0: vt+4 mode 1, its amount less that only in state 3; notice
  0x12f to the group, 0x4a13b0), else the line comes up (0x12); 6, the haul
  `WNOSIL_` (slot 3, activity `+0x76` 10; 25 frames) a frame a tick, at its
  end the catch stored (0x4ad300: up to the room left, `(100 - P) + room x
  100`; full when the room was no more than the catch): not full another
  cast (2), full the line up; 0x12, the line up a frame a tick, in below 1;
  0x13 (the hull picture back at its height `+0x797`, where it was); 5,
  0x4b3850 (commands, flags back, the route cleared 0x4a5b70), step 1 and
  0x4ad510: with room left and a leviathan in the box (0x4ac610 mode 0)
  event 0x40 {0x192, 10, it, 0x5a}, else to its port `+0x7ad` (0x4ac050)
  or the nearest (0x4c5910) event 0x40 {0x192, 0xc, port, 0x5a}; neither,
  event 8 0xc0. So a boat nets 50 in 26 ticks after a 10-tick start.
- **The guns** (0x4af6a0, state 1 only): items 3 and 4 only (a boat's 3
  and a merchant's 4 too), each aboard and loaded (`+0x5e3`), reaching the
  leviathan's cell from the ship's (0x4bf5f0: half of 0x598130's 9, tables
  0x598324 / 0x598418 by range level, 2 with Theseus); the hit roll
  0x4bf7f0 (one draw, `43 + 25 - 33 d / (9 x 64)` at the distance 0
  0x4ac8d0 passes) whose miss is its chance, above 0, so always a hit
  (0x4af6a0 takes a non-zero byte as 100); experience +50 or +10
  (0x4b4680); aimed at the point 0x495b40 gives in state 1 (0x5bace0 by
  heading from the cell's corner: (42, 52), (40, 23), (31, 15), (22, 20),
  (23, 52), (25, 30), (33, 43), (35, 29)) from the hull picture's middle,
  more than half the picture's width away: message 0x14ba with the item's
  fire power (0x4bc360 at 100, 0x4bfc20), never flaming; its reload
  `0x5c76e0[level + item x 3]` + a draw `% 60` ticks (level 2 with Atalanta,
  else the rate level). The points lie in the masks but for a loop's first
  four to six frames and, on even headings, its last one or two: shots
  landing then miss.

The line's and haul's pictures stand at `(hull w - slot w) / 2 + hull
x`, `hull h / 2 + 5 + hull y` (step 10 at 0x4af252; the haul's at the
line's, 0x4af396), set once. The slot is the picture 0x50cbf0 has just
made: its second argument (0x3e6 for the line, 0x3de for the haul) is
not a record but the slot's depth, which 0x545ef0 -> 0x55dbc0 ->
0x55d6e0 only files it under (0x55d9d0 keeps an object's slots sorted
by it, lowest first; the hull's is 0xa); the record comes with 0x4e1bb0
(vt+4, 0x50d080) after the place is taken, so the slot's `+0x61` is
still the constructor's 0 (0x545e70): the line hangs from the hull's
middle, the haul under it, both over the hull. Ported
(`Scene::hunt_gear`). A stop (0x4aa540 with no record, 0x4abc60; or
with the gather bit `+0x818 & 0x40`, 0x4aa540 itself) ends a hunt at
step 1 (`+0x25d &= ~0x40`), else calls it off (`+0x77c = -1`; the
order's event code 4 does too, 0x4aab00): step 10 goes on (the hull's y
is where it was), 0xc takes the line back a frame a tick from where it
is, at 0 off and step 0x13 (0x4af316, 0x4af526); 6 drops the haul for
0x12 (0x4af436); 2 is unchanged; at 5 0x4ad510 gives nothing, so the
task ends (event 8 0xc0). Ported. Sub-code 11 (0x4af640, from 0x4ac2d0's
table 0x4ac594: any leviathan in 0x59af78 = 11 cells) is set only from
the event's code (0x4aab00 at 0x4aac51 / 0x4aad83 copies `+0x83d`); no
sender with code 11 found (no byte 11 stored within a dozen
instructions of a 0x192 in the listing): not ported, senders not
traced. Not ported: the crew's experience, notice 0x12f (the port's zones
drop it), 0x4967b0's hit in flight (the port's shots land at their aim),
the gorgon's and siren's vt +0x30 in 0x403010, the order of a hazard's
0xda94 and the leviathan's own tick, and of the ships' and its ticks. Not
settled: whether a warship's own fire (0x4ab930 with `+0x25d & 0x100` or
`+0x684`: 0x4bf9b0, 0x4c1240, 0x4c0230) also shoots at it once 0x4ac5d0
has set its targets. Not decoded: `+0xa1` (0x523d80(200, it) as it moves;
never written by whale.cpp).

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
fanfare) if not. Nothing else plays at a sinking (the port's
`wreck1`, event 88, is only played at 0x4053f8, mode 6; gone). No sound was found for clicking an unavailable command.
An event's sound is picked by 0x554820(bank, event, 0) from its `SND_n`
entry (the first with fewer than two names, else `rand() % count`); no
entry or an empty one is silent. The creatures' (0x50e570 at their
cell): the Gorgon (mode 5) 94 `apmed3` rising, 105 `deamons` dying, 127
`atmed1c` staring, 116 `hitmed` struck; the siren (mode 6) 95 `app3`
surfacing, 106 `deamons` dying, 128 `sir1` singing, 117 none (no
`SND_117`) struck. Harpies and dragons (mode 5, 0x41f680, 0x41fbd0,
0x420670): appearing, a harpy's 0x5c (92 `app3`); taking off, a dragon's
0x5d (93 `app3`); each wing flap, 0x88 (136 `harpy2`) / 0x89 (137
`disap1`); striking, 0x7d (125 `atgarp1`) / 0x7e (126 `dr1` or `dr3`);
struck and living, a draw `r % 7 == 0` of its own generator, 0x72 (114
`hgar4` or `hgar5`) / 0x73 (115 `hdr2`); dying, 0x67 `deadga1` / 0x68
`dedr2`.

Units' voices come in groups of four events, one per nation, `base + [0, 1,
3, 2][nation]`. Only nations 1 (Greeks) and 3 (Persians) are recorded, and
their sounds are alike. They play only for the viewing player's units:

- **Gathering.** A merchant sent to gather answers on the interface's voice
  channel (0x49d3c0, 0x525370 channel 2): amber 156 (`am…`, `come…`), fish
  160 (`fi…`, `fish…`), a leviathan 164 (`lev…`, `levi…`).
- **Treasures.** A ship reaching a treasure's cell plays 172 (`piw…`,
  0x4a3f40). Taking a treasure plays 168 (`pit3`, 0x4a5bc0); the
  treasure itself, taken by anyone, plays 272 (`trea2`, 0x406c80 state 3).
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

The brain's generator (`+0x18`, the usual LCG, its own): the base
constructor 0x4788a0 sets it 0 (0x4788af; also `+0xe4 = -1`, `+0xf8 =
5`). Message 2 of each main: with a saved record (`r[0] != 0`) 0x478bb0
restores everything and `+0x18 = r[0x139]`, `+0x1c = 0`; a new game sets
`+0x1c = 1` and runs the mission's set-up, where only miss007 (0x45f84c),
miss008 (0x463370), miss009 (0x4664a9) and miss010 (0x46a21f) seed it,
with 0x541840(game) = the game's `+0x18`, written by 0x53fde8 and each
frame by 0x541701-0x541745: `late = +0x18 + +0x34 - timeGetTime()`
(import 0x5942d8); above 0 it is waited out (vtable +0x10, when
`[+0x2c]+0xc` is 0); then `+0x18 = timeGetTime() + late` if `late < 0`
and `late > -(+0x34 / 2)` (cdq, sub, sar: halved toward 0), else
`timeGetTime()`: the stamp set back by the lateness of a frame late by
less than half of `+0x34` (not decoded: `+0x34` of this object, the
brain's `+0x10`, not its `+0xc` game); the other mains leave 0, so their draws are the
same game after game (the first is 0x3039 >> 16 = 0). The port:
`Brain::rng`, saved with the brain, seeded 0 or, for miss007-010, the wall clock's
milliseconds; every brain step's draw comes from it, the pick of one of
the player's sites too: 0x421490 and 0x421740 draw nothing; the brain
lists the sites (0x421490 twice, settlements 0x32 then towns 0x33, each
row by row with its anchor in the rect), draws once, `(s >> 16) % n`
(0x45599a), takes the site's rect from 0x421740 (its anchor, 2 x 2, grown
to take in each of its buildings' anchors: one before the rect moves the
corner to it, one at or past the far edge makes the size reach it) and
grows that by 2 (0x4796c0). (Not read per mission: where the pick, brain
`+0x15c`, is set back to -1; the port picks on each call.) The repair
order's dock pick draws the fleet manager's generator, `Brain::fleet_rng`,
below.

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
set the fleet's behaviour (order 0x72). On each later call a step queries,
then tests its timeout, then its events, each switch overriding the one
before: move 0x47a450 goes to p3 on result 0xb, else to p4; gather
0x47a570 to p4 on result 1, p3 on 10; attack 0x47a6a0 to p3 on 10, else
(1 included) to p4; 0x47a7e0 (order 0x82) to p3 on 1, p4 on 10; patrol
0x47a900 tests its timeout (p3) and events first and returns, then, on
its way to a point (`+0x60 == 1`), starts the pause on 0xb and goes to p4
on any other result. p3 and p4 are equal at every gather site and at
every attack and patrol site but five: miss000 0x449810 state 5 patrol
(0x28, 5), miss008 0x4628d0 state 5 attack (10, 0) and 0x4629b0 states
0x14 / 0x1e (0x28, 0), miss010 0x468240 and 0x469a80 state 0x14 (0x28,
0). **A fleet with no ships** takes its orders as any other: 0x4a09d0
ends the old block and allocates the new (0x4cc500 / 0x4d6540) whatever
the ship count, the fleet tick (0x4c4f40: 0x4ccd80, 0x4d6cd0, 0x4ca8d0)
leaves it as it is, and every status routine (0x4ccf60 move, 0x4d63d0 /
0x4d70d0 attack, 0x4c8da0 order 0x78) starts with the ship count and,
at 0, reports done with result 1. So an empty member's move, gather,
attack or patrol reaches p4 at its next turn, 50 ticks after it started
(move: p3 if its time is up too; an event wins over both). 0x4ce2d0
(order 0x82) reports done but leaves result `out[5]` unwritten, so
0x47a7e0 reads a stack word: not decoded. The Size events count the
fleet (0x479d60): 0 for an empty one. The one member found holding
itself (the members' 0x479dc0 calls searched for a test of their own
count around the switch) is miss008's 0x4628d0: while it has no royal
ship, bomber, boarder or rammer (the byte sum of their counts is 0) its
switch is not run. The brain's tick (message 0) is
the game clock (0x47b2e0 copies `[game +0xde]` into `+0xfc`, and on
message 0 counts `+0xf8` down, setting game `+0x3239` at 0). Every 5
ticks each main does the brain's own work, then runs its members group by
group, members in order, member (g, m) when the tick is 5 x (g + m) modulo
50 and its fleet id (`+0`) is not negative, whether or not it has ships;
the switches written `(m % 10) x 5` or `((m + 1) % 10) x 5` are the same
value (0x4634b9). The brain's own work comes before the members in
miss007 (the reef, `+0x107`), miss009 and miss010 (the random tick,
below) and after them in miss003 (the first time the player has a ship
in (49, 26, 4, 3) (0x4c46c0, ship mask 0xfff, buildings 0; flag
`+0x114`), 4 minotaurs (0x1f5) at (53, 24) and 3 amazons (0x1f6) at each
of (48, 26) and (55, 30), 2, 1 and 1 more on normal and hard; the first
time in (48, 12, 3, 2) (`+0x120`), 3 amazons (2 more on normal and hard)
at each of (46, 14) and (52, 14); each group only if the cell's object
slot is empty and 0x42ebf0 gives 0xf: the landscape dword's low nibble
8, `0x5acf13[t] & 9 == 0` for the cell's first layer's shape t, nibble
5 not 1, nibbles 6 and 7 0. The flag is set whether or not any came; no
draw, no message, no cost. 0x414db0(class, x, y) makes a CyclopClassTy
(0x1497) with the record {class, 1, 0, 0, 0, x, y}, owner nation 4 slot
1, like a lair's; the first on a cell makes its jam (0x1f7), the others
join it), miss005 (`+0x184` cleared once
the cells of both harpy nests (23, 20) and (26, 19) are empty, `+0x18c`
once the stronghold's (3, 29) is), miss006 (0x45b863-0x45b993: the
latches `+0x1ac` (the southern Persians' yard (21, 68)) and `+0x1b0`
(the barbarians' (1, 50)), 1 at the start, cleared when the cell's
object slot is found empty, never set again; while `+0x1ac` was set at
the pass, [0x5b06d8] = (0x5b0444[d] (8, 6, 4) + rand % 6) x 1500, every
pass; on the pass where the second latch clears, [0x5b0c68] =
(0x5b0474[d] (8, 6, 4) + rand & 3) x 1500; each draw the brain LCG's
own. [0x5b06d8] is the `after` of the Size record 0x5b06c8 {g 0, m 3,
state 0x28, after, n 6, cmp 1} in table 0x5b0700, which only member
(0, 3) 0x458f20 passes, in its state-6 patrol; [0x5b0c68] that of
0x5b0c58 {g 2, m 4, state 0x19, n 6, cmp 1}, table 0x5b0cd0, member
(2, 4) 0x45a9f0's state-6 patrol. Both members also write their word in
their state 0, with the behaviour order: (0, 3) `(+0x1ac ? 0x5b0450
(15, 10, 10) : 0x5b0444 (8, 6, 4))[d] + rand & 3`, x 1500, then `+0x110
= rand & 1`; (2, 4) `(+0x1b0 || +0x1ac ? 0x5b0468 (20, 15, 15) :
0x5b0474 (8, 6, 4))[d] + rand & 3`, x 1500, drawn first. The static
10500 is never used. Member (0, 3)'s states: 0 that; 5 the move to (2,
47) (0x47a450, events 0x5b0718: the zone (0, 45, 11, 10) to 0x14) while
its royal ships, bombers and rammers are not the 3|3|4, 1 and 2|3|3 it
asks for (0x5b03b4, 0x5b03a8, 0x5b03c0), else 6, tested each turn; 6
the patrol 0x5b0680 (5, 5); 0x14 and 0x1e attack the zone seen, 1500
ticks; 0x28 -> 0x3c; 0x3c a settlement or town of the player's in (0,
0, 56, 70) (0x421490, 0x421740) -> 0x46, else 0x50; 0x46 attack it 6000
ticks, events 0x5b0758 (the zone, and fewer than 3 ships, 0x5b0728, to
0); 0x50 sail to (42, 59) if `+0x110`, else (54, 47), 750 ticks, to
0x5a; 0x5a attack (40, 57, 16, 13) 6000 ticks, events 0x5b0758) and
miss011 (`+0x19c`). Members made at
the start (0x8d1) that the switch has no case for never run: miss004
(0, 7), (2, 5); miss006 (1, 6); miss008 (0, 0), (1, 0); miss009 (0, 0);
miss010 (0, 0), (1, 0). A member's index is not always its place among
the port's fleets: miss002's Persian yard is (1, 7) (0x44cbd0; (1, 6) is
0x406330, which does nothing and is not among them), then (1, 8) the
repair 0x44cf20. The repair members (0, 4) of miss003, (1, 5) of miss005,
(1, 8) of miss002, (0, 6) and (2, 4) of miss004 are in the port's fleets
in member order since its brain layout 1 (`Brain::layout`; a brain saved
before is given them, empty, on restore, `Brain::upgrade`, and its
requests, ships being built and town orders move up past them). Every 125
ticks it judges the mission (vtable + 8, e.g. 0x448e90 for miss000, which counts
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
  0xce) while short of a target size and their slot free, at the start of
  each turn: two draws (the first thrown away; one for member 10), the
  type and the yard's wait `(rand % 61 + 60 or 70) x 25` from the second
  (60 for member 6). Otherwise one draw re-rolls the target:
  `rand % (difficulty + 1..3) + 3..4`.
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
  6 and 12's and restarts member 7. On the first call (`+0x10 == 0`) of
  a fight, members 5 (state 0x1e, 0x4625a7), 6 (0x14, 0x4627f1) and 11
  (0x14, 0x462d83) have member 2 cast spell 15, 9 or 12 on their ships,
  and member 10 (0x28, 0x462b59, before its four 0x479370 hand-ons)
  spell 9 on member 7's (0x47acf0 with pay 1: kinds 9, 12, 15, 16, cast
  by 0x479c20 on each ship). These go to member (1, 2)'s war temple, and
  miss008 has none, so none is cast; but each call with ships in the
  target credits player 15 a cast's price (1000 amber and 100 fish; the
  Flame 6000 and 1500) first. The port: `patrol_cast`, from `turn`.
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
  Member 3 (0x465b20), on the first call of its state 0x14 while the
  brain's byte `+0x12e` (difficulty + 3 from the constructor, 0x46655b)
  is not 0, casts kind 9 (the Flame) on its ships by the land fleet (1,
  0) (town (58, 43), war temple (57, 44)), paid, and counts `+0x12e`
  down. Its state 0x1e and member 4's (0x465da0) cast kind 15 by (1, 2),
  a ship fleet, unpaid: dropped (0x4a09d0). Member 4's state 0x28 casts
  kinds 12 and 9 by (1, 0) when its short `+0x12f` is 0, which the
  constructor sets to 750 and only those casts write (3000): never. Not
  decoded: whether the temple has learnt the three war spells the Flame
  needs (`+0xbc`, which the brain never sets); the port keeps no spells
  for a computer's temple and brews it at a standing temple.
- **The tick.** Every 5 ticks (0x4665b2, before the members) the brain
  draws a number and drops it, draws `x` and runs this when the tick is
  `((x % 10 + x % 10) % 10) x 5` modulo 50: both terms read the seed
  after the second draw, so only on a tick a multiple of 10, one time in
  five (checked in the disassembly, 0x4665c9-0x466626). Lightning (0x42f580 kind 6, a
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
- **The tick.** Every 5 ticks, on miss009's random phase (0x46a313,
  0x46a32a-0x46a387): once the dragon's nest at (59,
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
  Member 5's patrol (players -1, classes 0x3ffff: 0x46c3d1, 0x4797e0)
  leaves its own yard 2 alone: the yards are barbarian structures (class
  0x40), side 1 (`+0x57`) and nation 4 by their constructor 0x40de20
  (Barb.cpp, vtable 0x594920; the factory 0x40fc90, class id 0x1a6b in
  0x5c0f3c), and the load (slot 0 0x40ef70 -> 0x4e8660 -> 0x4e7640 ->
  0x427f50) takes nation and slot from the record's `+8` / `+0xc` (fields
  2 and 3: 4 and 0 in every barbarian record of the campaign and the
  battles), overriding only a side-2 castle's (0x42 / 0x43: nation 4,
  slot 1). Nothing else writes their `+0x57`. So they are player 16, and
  the look 0x4bfc60 and the list 0x4c0cd0 (whose free pass is for side 2
  but the sea monster 0x1fb) take them only as another player's object at
  war (0x4efe20 < 0): not the barbarians' own ships. (0x410150, which an
  earlier note gave as theirs, is the castles' constructor.)
- **The Persians.** The yard (0x46cf80, at (32, 17)) gives a one-off order
  of class 1, then serves its slots (boats to ship killers, 0xd2, 0xd3;
  0x34 gives town order 48, 0x47b130 code 0x30: one part, 0x3e9 class
  0x35, a medium fort, flags 7; no member ever asks for it: the request
  arrays are written only with -1 and the members' immediates 0xc9-0xcd,
  0xd2, 0xd3, in miss011-miss013; the yards of miss002-miss010 have the
  case too, no brain storing 0x34 as an immediate),
  resting 500 ticks. Members 1 and 2 (0x46d320,
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

- **0x47a7e0, repair** (ported). Every repair member is the same program
  (0x44cf20 miss002 (1, 8) with `YARD` 7, 0x452580 miss004 (0, 6) 5,
  0x452fb0 miss003 (0, 4) and miss004 (2, 4) 3, 0x4564c0 miss005 (1, 5)
  0, 0x459f60 / 0x477190 miss006 (1, 5) / (2, 5) 0, 0x45f370 miss007
  (1, 6) 0, 0x462fb0 miss008 member 13 2, 0x4661e0 miss009 member 6 2,
  0x469d10 miss010 member 10 2, 0x46e090 miss011 member 6 0, 0x475a70 /
  0x477190 miss012 and miss013 (1, 5) / (2, 6) 0): state 0 behaves and
  goes to 10, state 10 is `0x47a7e0(g, m, 0, 0, YARD, 0, 0, none)`. `YARD`
  is a number in the player's fleet list (0x479200 makes the members in
  order, 0x49b990 appending: fleet n is member (g, n)): its group's yard.
  First call: fleet order **0x82** with `{0, 0, YARD, 0}` (0x479900,
  0x4cdf80). A new block (0x4cd840, 0x65 bytes zeroed, `{0x82, 0x51,
  now}`) gets `+0x10` = args[1] = 0 (a dock id), then, args[1] being 0
  and args[2] >= 0, the dock pick 0x4cdea0 **before** `+0x21` = args[2]
  is written (so it looks at the player's fleet 0), then `+0x21` = YARD,
  `+0x18` = args[3] = 0 (the fleet a mended ship is handed to), `+0xc` =
  0; every ship of the fleet, hurt or not, is sent (0x4cdb00, 0x4cd930:
  ship command 0x1e `{+0xc, +0x10, 0x50}`, the human's "go to dock").
  Later calls query 0x83 (0x4ce2d0), which writes only `out[0..4]`;
  0x47a7e0's exits read `out[5]`, a stack word never written: not
  decoded, the port keeps the member in 10. **The dock pick 0x4cdea0**:
  the player's fleet `+0x21` must be kind 0xb with a land fleet `+0x29`,
  else 0. 0x42cc40 = 0x42c9a0(class 0x3b) (player 16: class 0x40, its
  buildings of that class directly) on the land fleet drops from its
  list every building gone or in state `+0xa4` 2, 3 or 0xff, then lists
  the docks of its towns (type 0x1491: count 0x43d220 `+0x210`, entries
  0x440c60 from the town's buildings `+0x1d0` of class 0x3b, `{id, x,
  y}`). With any: `+0x10` = the first, `+0x14` its cell, one draw of the
  fleet manager's LCG (`DAT_005c7504 +0x18`, `s * 0x41c64e6d + 0x3039`),
  and the id of entry `(s >> 16) % count` is returned. **The tick
  0x4ce140**: the fleet's queue `+0x2d` is heard newest first (0x4cdc00):
  300 (a ship joined, posted by 0x4a1340 when 0x4a0480 op 2 adds one) for
  a ship in the fleet (0x4cc590): the dock pick (now on the yard), and the
  ship sent if the pick found one or `+0x10` is set: 0x4cdb00 with
  `+0x61` 0 (no waypoints, every repair order) sends to `+0x10` whatever
  dock it is given, so the random dock only tells whether any was found;
  then 0x4c42f0(1, ...). 0x134 (mended): `+0x18 > 0` would hand it on
  (0x49bd60); it is noted in `+0x1d` with state 10 (0x4cd980) and given
  ship command 10. 0x136 only with waypoints. If no 300 sent a ship and
  fleet `+8 > 0x5f`: `+8` = 0, each ship's pending blow is reported
  (0x4a21c0, 0x4cf760), and each ship lying still (`+0x58c` 0) not noted
  mended is sent to `+0x10` again. Last `+8 += game +0xe2`. The fleet
  record is zeroed when made (0x49b990), so `+8` starts at 0. **Who
  joins**: the brain's message 0x5d97 (the capture 0x4b6f20) for player
  15's (miss004 and miss006 also 14's) prizes, by 0x479300 op 2: miss002
  15 (1, 8); miss003 15 (0, 4); miss004 15 (0, 6), 14 (2, 4); miss005 15
  (1, 5); miss006 15 (1, 5), 14 (2, 5); no type test. 0x49bd60 adds the
  ship to fleet n of **the ship's own player**. So miss003's order starts
  with no dock (fleet 0 is a ship fleet, kind 10) and its first prize sets
  `+0x10` to the dock of the town at (58, 1); miss005's starts with the
  dock of the town at (8, 11) (fleet 0 is the yard); miss002's (1, 0) and
  miss004's (0, 0) and (2, 0) are kind 10 too (0x8d1). The port:
  `repair::<YARD>`, `Scene::repair_order`, `repair_pick`,
  `run_repair_orders` (each brain tick), `brain_repaired` (0x134),
  `brain_join` (0x5d97, from `brain_prize`), the order on `Fleet::repair`
  (`+0x10`, `+0x21`, `+8`, `+0x1d`) and the queue on `Fleet::joined`; a
  land fleet's list is its member's buildings, a town's docks those of
  its list `+0x1d0`, in its order; the update's ticks `game +0xe2` are taken as 1 a
  tick, as the wind's counter. Not decoded: the manager LCG's seed (0
  here); what ship command 0x1e does with dock 0 or an
  unhurt ship (the port's repair order refuses both); ship command 10
  after 0x134; 0x4c42f0; whether the navy's tick (0x49ad18) runs before or
  after the brain's; the 0x5d98 gate `+0x14`. The pass's blow reports are
  not ported (the port reports at once). miss007-miss013's prize tables
  are not ported: their prizes sail to one of the player's docks, picked
  at random.
- **0x47ae10, cast** (`(group, member, kind, x, y, pay)`). The fleet casts
  spell `kind` (5-16, one per command; costs from 0x596844 by 0x47abb0,
  paid when `pay`) at cell (x, y) through message 0x3ee (0x47ab40). It is
  used by miss011-miss013 (miss006's waterspouts go through 0x47ab40).
- **0x479ed0** makes a fleet (0x4a0480 op 0xb) with a type mask;
  **0x479f40** and **0x479fb0** test the brain's event list against
  objects (0x49c3f0).

More `missCOM` pieces: a blow landing on a ship (0x4bf170, per projectile)
sets its `+0x691` and `+0x692` (the shooter's id), whoever shot; the AI
module's fleet order handlers (0x4ca290 and nine more) report such a ship
with message 0x5d9c (0x4a21c0: fleet number `+0x16`, shooter `+0x1a`) and
clear it (ship message 0x28, 0x4cf760), only in their periodic pass: when
the fleet's queue sent nothing and fleet `+8`, to which each call adds
`game +0xe2`, is over 100 (0x4ccd80 move, 0x4cbe70, 0x4cf550), 0x5a
(0x4d6cd0 attack, 0x3c on a branch not read; 0x4da6a0), 0x4b (0x4d7b50),
0x1e (0x4d8330) or 0x5f (0x4dbd90; 0x4ce140 repair), `+8` then set to 0
(0x4ca290: another structure, not read). Not decoded: the orders of
0x4cbe70, 0x4cf550, 0x4d7b50, 0x4d8330, 0x4da6a0, 0x4dbd90. The port
reports at once (it has no tick for the orders but the repair). The brain (0x47b2e0) keeps the report in its
list `+0x100` unless one for the same fleet and player is there: the first
waits, later ones are dropped. The event 0x47a060 `{brain, g, m, state,
mask, after, stamp}` takes the member's report: younger than `after` into
the step, it is dropped; its shooter gone (0x51f1f0), dropped; a shooter
of a player not in `mask` (0x20, the human, in every campaign record), it
stays and nothing fires; else the member's `+0x40` becomes the area about
the shooter's cell now (0x4796c0 with 1 x 1 and 2: the corner 2 up and
left but not below 0, the size 5 cut to end at the map's edge), the report
is dropped and the event fires. Member `+0x40` (x, y, w, h; zero at
first) is also written by every area order: attack 0x70 (0x479a40, when
an attack state starts), gathering 0x78 (0x479970), founding 0x8c
(0x479af0) and 0xbe (0x479c90; miss007's sword hunt gives the whole map),
and by member code of miss007, miss009 and miss010; not by the zone event
0x47a010, which writes only `+0x50`, the matching zone's index. So
miss012 (2, 5)'s and miss013 (2, 4)'s state 0x1e, which their zone
records (1, 34, 21, 7), (19, 39, 6, 13) and (14, 49, 24, 14), (37, 41,
9, 8) lead to as well as their attacked record, attacks `+0x40` 375 ticks:
the area of their last attack or attacker, not the zone. 0x47a2a0 compares the fleet's size
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

**A fleet's move (fleet order 100) and its formation.** The brains'
move (0x479870: payload `{x, y, p8, tolerance}`, `[4]` 0) goes to
0x4a09d0 -> 0x4ccc30: a block not `{100, 1}` is ended (0x4a1c60), a new
one made (0x4cc500: `{100, 1, now, x, y, p8, tolerance}`) or the move
block rewritten in place (its slot list `+0x24` kept). Then 0x4cc970: one
ship is given the cell itself; two or more go to the planner 0x5614e0 ->
0x560100 with a record each (0x24 bytes: the ship's cell `+0x5a3/+0x5a7`,
class `+8` 0, `+0xc` 1 for shallow draught `+0x592 == 0`) over copies of
the ships' grids (0x4d50b0: 0x5c9398 if any ship is deep, 0x5c93bc if any
is shallow). Each ship's cell is noted in the block's list by ship id
(0x4cc7d0) and the ship sent there by command 0x14 `{x, y, 0}` (0x4cf070,
0x520cc0). If the planner fails (negative), every ship gets command 0x14
to the order's cell with priority byte 0xff (0x4cc650). The planner:
- p1 the deep grid, p2 the shallow one (only one of them: both are it).
  The centre is the ships' median cell (sorted x and y; an even count the
  two middle values' mean rounded down). Centre or goal off the map: -4.
- From the goal, on copies of p2 (with both draughts) and p1: 0x573350
  opens the closed region the goal lies in (cells -1 joined side or
  corner set to 0; nothing if the goal is open), then the wavefront
  0x557910 (layers `2 (W + H)`, values up to `4 (W + H) + 2`); no layer
  run: -4. A centre closed in p1 moves to the cell within three rings
  with the lowest goal-flood value (the smaller ring distance on a tie);
  each ring edge tests p1's openness at the edge's first cell only (the
  original's code); none: -4. Then the wavefront from the centre on p1
  (and p2), the goal's cell keeping its closed value.
- 0x557910 is a wavefront of runs (16-byte records: side, match value,
  length, least value written, first cell): the start 1, then each layer
  every run steps one cell (the diagonal ahead of its start + 3 from the
  run's first cell, each cell along + 2 from the cell behind, the far
  diagonal + 3), splitting where a cell is not 0 or would pass the cap;
  then each run's ends turn the corners (a diagonal + 3 and the cell
  beside + 2). The next layer's runs are kept sorted by the least value
  each wrote (0x571770, stable). It returns the layers run. (It is not a
  shortest-path flood: about obstacles its values can be higher than the
  least 2/3 sum.)
- Slots (0x560f80 -> 0x561360, class 0): the open cells (exactly 0) in
  square rings of radius 0-10 about the goal (an edge past the map left
  out, the others cut to it; right side bottom to top, top right to left,
  left top to bottom, bottom left to right), ring distance `(2r + |off|)
  / 2`, radius 2 or less any, beyond only distance under 11. The best `n`
  are kept in order: a cell where a ship would lie if the fleet moved
  whole (its bounding-box bitmap shifted from the centre to the goal)
  goes before another unless that one is two nearer; otherwise the nearer,
  then the lower centre-flood value. Shallow ships' slots are found first
  on p2, then the deep ones' on p1; each slot is marked taken (`| 0x8000`).
  Too few: -4.
- If every slot is a "whole fleet" cell, each ship goes to `goal - centre
  + its cell`. Otherwise 0x55f870 deals them: the slots' median (an odd
  sum of the two middle ones rounded up when below the goal); `toward`, the
  direction from the centre to its 8-neighbour with the least goal-flood
  value (`& 0x7fff`, a diagonal + 1, the direction to the slots' median
  0x557560 on a tie), and `back`, from the slots' median to the least
  centre-flood neighbour. Slots are bubble-sorted by 0x5574d0 along
  `(back - 4) & 7` from the slots' median (the higher centre flood first
  on a tie); ships by 0x5574d0 along `toward` from the first (unmoved)
  centre (the lower goal flood first); the n-th ship takes the n-th slot.
  With both draughts a shallow ship on a slot closed in p2 swaps it for
  the next slot open in p2, and a deep ship takes the next p2-closed slot
  while the deep ships left are no more than such slots (none: -4).
  Not decoded: when no neighbour qualifies for `toward` or `back` the
  routine goes on with a stack word it never wrote (`[esp+0x34]`,
  `[esp+0x24]`, a pointer); the port then sends every ship to the order's
  cell, as 0x4cc650 does on a failure.
- Slot classes and the third grid (0x560100 with other callers than
  0x4cc970; port `formation::plan_records`): a record (0x24 bytes `{x,
  y, class +8, second +0xc, out x +0x18, y +0x1c, third +0x20}`) goes
  into bucket `class` (below 1 counted 0; ships `+0xf0`, slots `+0x6c`,
  11 buckets of `{count, count, list}` in the state). p1 serves the
  records with `+0xc` 0; when no record has it, or p1 is null, p2 serves
  all, and when all have it p2 is p1. With both, for classes 0-9 each
  bucket's p2 ships get their slots first (0x560f80 on p2, marked in p1
  and p2), then each bucket with p1 ships all of its slots on p1 after
  those. 0x560f80 with class `c` walks rings from radius `2c / 3` to 10
  and takes an open cell when `2r < 5` and `c <= r`, or `c <= d < 11`
  (`d` the ring distance). 0x561360 reads the third grid (state `+0xc`,
  0x5614e0's third argument; 0x4cc970 gives none) into the slot's `+8`:
  of two slots alike in shape and distance the lower threat first, then
  the lower centre flood. The whole-fleet move returns 0 and zeroes each
  `+0x20`. 0x55f870 takes the median over every bucket's slots, sorts
  and deals each bucket on its own (and the draught swaps per bucket),
  and returns the greatest goal flood (`& 0x7fff`) at a dealt ship's
  cell; each ship's `+0x20` is 1, 2 or 3 by the rank of its slot among
  all slots by key (a stable insertion sort, before the swaps) against
  `n / 3` and `(n - n / 3 + 1) / 2 + n / 3`. Bucket 10 (class 10) is
  never planned and 0x55f870 then reads medians past its slots: not
  decoded (the port refuses it; no caller gives it).
The move's pass (0x4ccd80, the fleet tick 0x4c4f40 each game tick): when
fleet `+8` (zero when the fleet is made, + `game +0xe2` each call) is over
100 it is zeroed and every ship lying still (`+0x58c` 0) more than one
cell off its slot in x or y (the order's cell without a slot) is sent
there again (0x4cf070). The brain's primitives never stop a fleet: on an
event or timeout they only switch state (0x479750); the fleet keeps its
task until another replaces it. Port: `formation.rs` (checked against
0x5614e0 run on 600 random layouts and on the first formation move of each
campaign mission), `Scene::fleet_move`, `run_move_orders`. Before the
pass, the fleet's list (0x4cc6a0, newest first, every record removed): a
ship that joined (300) and is in the fleet has the whole formation
planned anew (0x4cc970: one ship, the cell; more, the planner; each ship
noted and sent by 0x4cc8c0; it returns the ships sent), or when that
sends none it alone goes to the order's cell (0x4cc650: command 0x14
with priority byte 0xff, the ship's priority kept; `Order::MoveKeep`),
then 0x4c42f0; any join heard skips the pass. The planner's failure in
0x4ccc30 sends every ship so too.

The move order's payload (0x479870 from 0x47a450) is `{x, y, p8, tol}`:
p8 is 0 at every brain call, the tolerance p7 1, 2 or 3 (only 0x451c60,
miss004's northern raiders, passes 3), each `Op::Move`'s `tol` as its
call has it. The brain's query 0x65 (0x4a0f20) is answered only while
the fleet's task is a move block (100, sub 1 or 2), by 0x4ccf60: no
ships 0x1f5 / 1; else each ship of the list still there counts, in when
within `tol` (0x557440) of its slot (0x4cc760; the order's cell without
one), else off and under way (`+0x25d & 0x80`; the port: a route) or
off and still; all in 0x1f5 / 0xb; none in and none under way 0x1f5 / 1
(and result 1 whenever all are off and still); some in and none under
way 0x1f5 / 0xb; else 500. 0x47a450 then goes to its p3 on 0xb, its p4
on any other 0x1f5, to p3 when its time is up (ticks > 0), and to an
event's state, the later winning. A fleet whose task is no longer the
move gets no answer (0x47a450's result is then its stack's leftover:
taken as none). miss006's raiders (0x459b90, 0x45a9f0 case 5) move with
no time limit (p10 0). The behaviour command 0x4c42f0 goes with a join
sent to the order's cell and, by the pass, to a ship whose `+0x689 /
+0x68d` differ from the group's `+0x19 / +0x1d` when group `+0x14` is set
(ported; "Ships' behaviour and masks").

The patrol (0x47a900): its first turn sets point 0 and "on the way"
(`+0x58`, `+0x60`), then tests the time (never true yet) and the events:
one firing switches the state and no leg is sent; else the leg to point
0 (0x479870 with its p7 as the tolerance). Later turns: the time first
(`ticks` up: p3, nothing more), then the events, then, on the way, the
move's state (query 0x65, 0x4ccf60 by that tolerance): 0x1f5 / 0xb pauses
from now (`+0x5c`), any other 0x1f5 goes to p4, no answer nothing; paused
past the point's pause, the next point (wrapping) with p7 again. p7 is 1
at 0x44a7e4 (miss001 0x44a6f0) and 0x44bea7 (miss002 0x44bd80); 3 at
0x44f1a5, 0x44fa29 (miss003 0x44f050, 0x44f880), 0x453804, 0x4538cb
(miss004 0x4536c0), 0x45671f, 0x456b6c, 0x456fe2, 0x457417 (miss005
0x456580, 0x456e80); 2 at every other call. The port: `Op::Patrol`'s
`tol`.

**miss004's and miss005's members that move on a ship count.** Each
counts its ships by type (0x479dc0: query 7, 0x4c4e40, an int per type
`class - 200`) and tests the count in its state 5 anew each turn: equal,
it sails home with p3 = p4 = 7 (the watch); not, its state 6 sails there
with p3 = p4 = 0, back to 0 and round again, never waiting.
- miss004 (0, 2) 0x451890, (0, 3) 0x451a60: exactly 2 royal ships
  (whatever their asks, 0x5aef38 / 0x5aef44 (2, 3, 3) / (2, 2, 3)); to
  (5, 3) / (7, 10), tol 2, the zones 0x5af068 / 0x5af108; 7 watches 1500
  ticks with them (0x47a3b0 given events), 0x14 attacks 1500. (0, 3)'s
  attack takes its own zones' area (0x5af118) at the southern raiders'
  index (`*(+0x44) +0x1b8`, member (2, 2)'s `+0x50`: a slip), and once
  until its state 0 (`+0x17c`) sends (2, 2) to 0x5a.
- miss004 (0, 4) 0x451c60: royal + boarders == 3, to (8, 9) tol 3,
  zones 0x5af1f0. State 0 draws `+0x118` = rand % 3 if 0, then `+0x114`
  = (rand % 3 + (7, 5, 3)) x 1500 if 0 (2250 at message 2: the first wait
  is not drawn), zeroes `+0x184`, `+0x188`. 7 watches `+0x114` (read at
  the step's start, member `+0x14`) then 0x28 patrols 0x5af400 (8, 9,
  500), (24, 18, 750), (8, 9, 750) 4500 ticks, p3 = p4 = 0x50, with the
  zones 0x5af260 (`+0x118` 0: home 0x14, the rest 0x32) or 0x5af318
  (1-2: 0x3c). 0x32 / 0x3c draw once (`+0x184` / `+0x188` = rand % 6 + 1)
  and on a 1 or a 4 send message 0x3ee to the yard's member (0, 5) for
  each ship (0x479c20, unpaid): the weapon shield (0xf) or the phantom
  (0xc); then attack the zone 4500 ticks, p3 = p4 = 0x50, events 0x5af458
  (home zones; fewer than 3, back to 0). 0x50 zeroes `+0x118`, `+0x114`.
  0x5a attacks 6000 ticks the area 0x5af1b8 at (2, 2)'s `+0x50`: index 1
  (its zone 0x5af470 [1]) is past 0x5af1a8's one record, zeros.
- miss004 (2, 2) 0x4528a0: royal + boarders == its asks summed (0x5aef74
  + 0x5aef80), to (7, 24), zone 0x5af1a8. State 0 draws `+0x120` (never
  read) if 0; 7 watches the northern raiders' `+0x114`; 0x14 sends (0, 4)
  to 0x5a once (`+0x180`); 0x28 patrols 0x5af510 with the zones 0x5af470
  (home 0x14, east 0x1e); 0x1e attacks 4500 (p3 = p4 = 0x50, fewer than 3
  first); 0x50 zeroes `+0x120`; 0x5a attacks 6000 the area 0x5af118 at
  (0, 3)'s `+0x50`.
- miss005 (0, 1) 0x4551e0: royal + boarders + rammers == royal + bomber
  asks (0x5af8f4 + 0x5af900; its bombers are not counted), to (7, 29) tol
  1, zone 0x5afa60; 7 watches 7500; 0x14 attacks 1500 without events;
  0x32 (sent by the raiders) attacks 4500, no events, the area 0x5afa30 at
  `+0x158`.
- miss005 (0, 3) 0x455620: before its switch every turn `+0x150` =
  0x4795f0(the human, 0x5afa30, 3, `+0x158`): the player's objects in list
  order (0x4a8620), the first fort (0x34-0x36) whose cell lies in (11, 18,
  6, 6), (13, 25, 9, 7) or (9, 32, 8, 4) (vt+0x58 0x423660: a fort is one
  cell), `+0x158` its area (kept when none). State 0: `+0x15c` = -1,
  `+0x114` = `+0x110` = `+0x154` = 0, the wait 0x5afbd8 = ((7, 6, 4) +
  rand & 3) x 1500. 5: `+0x154` = `+0x150`; royal + rammers > 2 -> 6, else
  to (1, 23) tol 2, p3 = p4 = 0 (zones 0x5afad0; at least 3 after the
  wait -> 0x28). 6 patrols 0x5afb60 (p3 = p4 = 5; also attacked after 375
  -> 0x1e). 0x14 attacks the zone 1500 when it is the home zone (index 0)
  or royal + rammers == 3, else back to 0, tested each turn. 0x1e attacks
  `+0x40` 375. 0x28: `+0x110` 0 -> rand & 3, and with `+0x154` set sends
  (0, 1) to 0x32 (each turn the draw stays 0); with `+0x154` and `+0x110`
  0 or 1 it attacks the fort's area 6000 (zones; fewer than 2 -> 0), else
  `+0x114` = rand & 1 and 0x32. 0x32 sails to (11, 30) (0) or (15, 24),
  750 ticks, then 0x3c raids a settlement or town of the player's in (0,
  0, 35, 35) (0x421490, 0x421740, grown by 2), else (27, 28, 9, 8), 6000.
- Neither main draws at the start (the port drew the raids' waits there).

The port: `Op::Branch` for each count, `Op::WatchFor` (the latched
wait), `Target::Of` (an area table at another member's index), the
fields in `Brain::fields`, the cross-calls and draws in `miss004::turn`.
A member has one request slot (`+0x128` + 4m here; miss001 `+0x114`,
miss002 `+0x118`, miss003 `+0x124`, miss005 `+0x120`): its types are tested
in turn, `count < n and slot == -1` filling it with 200 + type, so only the
first short type is asked and nothing while it is taken. miss004's
northern yard (0x452240): state 0 the full-town order (0x47b130 code 1,
on to 5 either way); 5 the first of the 8 slots set, in member order, its
member kept in `+0x11c` (state 10); 10 by the slot: 0x34 order 0x30, 200 +
t order 0x50 + t (flag 0x400, for member `+0x11c`), either to 0xb, any
other value cleared with `+0x11c` (-1) at once; 0xb a wait of 0x2ee (750)
ticks (0x47a3b0) to 0xc; 0xc the slot and `+0x11c` cleared (-1), state 0.
The port: `Brain::served` holds the member from the pick until 0xc, its
`ask_for_ships` asking nothing meanwhile; the order is tried again each
turn while it cannot be paid or no yard is free (as `Op::Build`), and
ends with the ship launched. The other yards of miss001-miss007 are made
the same way: miss001 0x44a4e0 (slots `+0x114`, 6; types 0, 1, 2, 5, 6),
miss002 0x44bba0 (`+0x13c`, 9; 1, 2, 5, 6) and 0x44cbd0 (`+0x118`, 9;
order 0xf0 at state 0, then 5; 0-6 and 0x34), miss003 0x44ed00
(`+0x124`, 6; 0xf0 first; 0-6, 0x34) and 0x44f1e0 (`+0x148`, 5; 1, 2,
3, 5, 6), miss004 0x452c60 (`+0x148`, 6; 0xf0 first; 0-6) and 0x453030
(`+0x160`, 4; 1, 2, 3, 5, 6), miss005 0x454f60 (`+0x120`, 4; 0-3, 5, 6)
and 0x455b80 (`+0x130`, 8; code 1 first; 0-6), miss006 0x458990
(`+0x118`, 4), 0x459400 (`+0x128`, 8), 0x45a270 (`+0x148`, 8), miss007
0x45e510 (words `+0x123`, 6): the pick (state 0, or 5) the first slot
taken in member order, on a turn of its own (0x479750 to 10); 10 the
order from the next turn until it ends (to 0xb); 0xb the wait; 0xc the
slot and the served member -1, to 0 (0x45e510 falls through to 0 the same
turn). Ported (`Op::Build`, `Op::Release`, the yard's `build` holding the
member's slot from the pick to 0xc, `ask_for_ships` asking nothing
meanwhile); the port's order still ends with the ship launched. No member
of these missions asks a type its yard has no case for (cleared at 10,
back to 0 a turn later), so that path is not in the port. The rests,
from the brain's random source (`+0x18`): 0x44f1e0 draws `+0x13c =
0x5ae884[d] + (rand % 31) x 25` (1000, 750, 500) at every state-0 turn;
0x453030 `+0x110 = 0x5aef98[d] + (rand % 61) x 25` (1500, 1000, 750),
0x454f60 `+0x118 = 0x5af8e8[d] + (rand % 41) x 25` (1500, 1000, 750) and
0x458990 `+0x108 = (0x5b0438[d] + rand % 31) x 25` (40, 30, 20) only at a
state-0 turn with the field 0, zeroed at 0xc (and at 10 for a dropped
type, 0x454f60 / 0x458990). The constructors start them at 0 (0x44fd90,
0x457950, 0x45b280) but miss004's at 0x8ca (0x453b00: the first rest is
2250, undrawn). 0x45e510 calls 0x47a3b0 with `(rand % 31 + 40) x 25`
drawn at every 0xb turn; 0x47a3b0 keeps p5 as the span at its first call
and later only tests it for 0, so the first draw is the rest and every
waiting turn draws once more. Ported (`Op::BuildRest`, `Op::WaitDrawn`,
the yards' state 0xc zeroing the rest). miss007's barbarian yards
(0x45ca00, member m serving m + 5): the slot `+0x109 + 4m` holds code
`0xc8 + type` high and the rest low, written by the fleet (below); 0 to 10
once the high word is not 0xffff, 10 the order for m + 5, 0xb the low
word's wait, 0xc the slot 0xffff0000. Ported (`miss007::yard`, the slot in
the fleet's variable 2, 0 for none).
0x4a8620(player, &n, ship mask, building mask) walks the player's list
(`+0xbb8`): 0x50e600 sorts each by class (200-211 ship 200, 0x32-0x4a
building 0x32, 500-510 500, 400-403 400, 700-706 700, else 0); a ship
whose bit `class - 200` is in the mask, or a building whose `class - 0x32`
is, matches and latches; from then on every object is kept, matching or
not. Each kept one goes at its own index in the out array (malloc'd, not
cleared) and counts in n, so an object before the first match leaves an
uninitialised hole that the reader (0x4795f0, masks all ones: any ship or
building matches, reading the first n slots) takes for an object. The
port's fort search reads the player's ships and buildings in order, which
is the original's while the list starts with a ship or building. Not
decoded: what joins a player's list besides ships and buildings (the
adders of `+0xbb8`), so whether a hole can occur; were one read, the
original would read uninitialised memory.

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
(miss009) 46, 76 (Greek fire). Only the mission's own number grants, in its own mission; one taken by a ship (or at another class) says "You have got a
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
(CyclopClassTy 0x1f4-0x1f6) onto a free land cell beside them once
`(10 - field 9) x ((r % 21 + 80) x 25 / 100)` ticks have passed since the
last, while fewer are out (see Land creatures), to roam the island about
the lair. Their sprites are `CLPSPR_`/`MNTSPR_`/`AMZSPR_` + state +
heading: 0 appearing (out of smoke), 1 standing, 2 walking, 3 throwing
(the amazons' in two parts, `AMZSPR_3<heading><part>`), 4 hit, 5 dying, 6
turning (`<..>SPR_6`, no heading); see Land creatures.

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
and 0x597170 fish by item); 0x138e is a blow (queued by 0x423c30): an
owned object (`+0x57 == 1`) reports it to its owner's brain with message
0x5d9c, and a town being built or upgraded (class 0x33, state 0) drops it
unapplied.

At a mission's end the brains' shared slot 0x478e30 fills the statistics
at game + 0x3309 (twelve dwords, shown by 0x539b20 with strings
1047-1058) from the human's player record (`+0xe4` = 5): enemy ships
destroyed (player + 0x704, by ship type), boarded (+0x894), enemy terrain
objects destroyed (+0x734, classes 0x32-0x41), monsters killed (+0x7a0,
classes 0x1f4-0x1fd), monsters' terrain objects destroyed (+0x734,
classes 0x42-0x49: the wizards 0x42, 0x43 and the nests and lairs), ships
lost (+0x3e4), terrain objects lost (+0x414, classes 0x32-0x49), fish
caught (+0xab8), amber earned (+0xbb4), amber left (+0x20), treasures
found (0x4a95c0) and the score: ships destroyed and boarded and monsters
by their points (0x59b358: the ships' amber costs; monsters by class -
0x1f4 through the jump table 0x479140: 700, 400, 200, 200, 600, 1100,
1300, 800, 200, 900, so a dragon 0x1f9 1100, a siren 0x1fd 900), less the
ships lost, plus the amber earned, a fifth of the fish and 600 per
treasure; a tenth of it (0 if below), added to the campaign's running
score. Buildings score nothing: the price lookup 0x42d8b0 is given the
index, not the class. The counts are made as a thing is destroyed
(0x4a9250, from 0x4a8410, the Navy's message 0x83 sent by 0x50e750 with
the killer): a thing of the killer's own player counts nothing; else its
player's loss (ships; buildings but classes 0x44-0x49) and the killer's
gain, when the killer is a player (0 < killer < 20); class 0x4a is counted
in neither sum, monster class 0x1fe (`+0x7c8`) in none. A ship's killer
is the giver of its sinking blow (`+0x7f` from the blow's `+0x80b`,
0x4befc0, 0x4be0c0; set by ship message 4, 0x4a9e30). 0x50e750 posts a
killer of -1 as 0xff: the owner's loss is counted, nobody's gain.

The killer each caller of 0x50e750 passes:
- **Shots** carry the player put in the projectile's `+0xb5` when fired
  (message 0x14ba `+4`): a ship's `+0x24f` (weapons 0x4c026b, items
  0x4af6a0, the hook 0x4b84b0), a fort's player (0x4469c0), the fireball's
  caster (0x405d2d; 0xff for nation 4 slot 1, 0x405fa0-0x405fbb); 0xff for
  the land creatures' and harpies' throws (0x4129f3, 0x42031b, 0x420550),
  the volcanoes and every 0x499410 shot. A ship is credited to it through
  0x4bf170 -> 0x4be0c0 (`+0x7f`), but a ship with `+0x251` 2 (being
  built or sold) gets 0xff (0x4b4000); a building through `+0x1ac`, written only by the
  killing shot (0x4246b2); a gorgon, siren, sea creature and the
  leviathan through the record (0x4014d0, 0x403480, 0x490c70, 0x494090;
  the leviathan, category 400, counts nothing). Harpies and dragons shot
  down are reported to no one: 0x420670 takes their life to 0 and
  reports nothing. Not decoded: a land creature struck by a shot
  following it (0x4967b0, 0x496ca5-0x496cef) gets message 0x138e with
  `+0x15` unwritten, a stack leftover (`ebp - 0x27` of 0x4967b0; its only
  caller, 0x4974d0 at 0x49785c for the shot's states 2 and 3, calls only
  __setjmp3, 0x587388, before it, which writes nothing on the stack, so
  the byte is what was left there before 0x4974d0 was called: run-time
  state, not settled by the code); one struck by a projectile landing
  on its cell (0x498240) gets the shooter's (the port gives the shot's
  player in both).
- **Lightning** (0xda93, the message's `+0x18` = the bolt's `+0x75`, the
  caster): buildings (`+0x1ac`, 0x428633 / 0x415102), land creatures
  (0x414ce0), harpies (0x420cce, 0x420d23), landing parties (0x4227fb), a
  sea creature struck on its own cell (0x492c2a, each time it is left at
  nothing; struck from a cell about, it is never reported). A ship
  wrecked by it is put down to no one (0x4de2cd stores -1, and the wreck
  event 0x200, 0x4aa280, gives `+0x7f` 0xff), as are reefs' and
  whirlpools' wrecks (0xda94, 0xda95).
- **Tornadoes** (0xda96, `+0x18` = the tornado's `+0x75` = the caster
  from the spell record's `+0x14`): ships as a code-4 blow of class 0x25c
  (0x4de3ce), buildings (0x426f37), creatures and harpies alike.
- **No one**: the siren's song (code 4, class 0x1fd, player 0xff,
  0x4a74c0), the gorgon's stare and the fire spell (0x158bf: class 0x1fa
  code 4 on ships; a building fallen by it keeps `+0x1ac` as it was),
  fire on board and every hull test (0x4be030), the boarding-state ends
  that sink a ship (0x4b5df0, 0x4b73a0, 0x4b93f0, 0x4c3410 sites).
- **Rams** (0x4c1c60): the prey's blow is put down to the rammer's
  `+0x24f` (0x4c1d63) and the rammer's own to its prey's (0x4c1f2a).
- A sea creature's bite: the monster's player (0xf166, 0x49098d); one
  eaten by another: the eater's (0x490c70 code 1).
- **A prize taken by boarding** is reported with its taker's `+0x24f`
  before it changes hands (0x4b7007, then 0x4b9700): the taker's ship
  destroyed (besides boarded) and the loser's ship lost. (If either
  player is 16 the prize is sunk instead, `+0x7f` the taker, 0x4b7169.)
- A building sold by its owner: its own player (0x427472), counting
  nothing.
- 0x4b3ab0 with `+0x73a < 0` is a **ship being built called off** (task
  4 on a yard's berth, see "Shipyards' berths"): its price given back, then
  -1, its owner's loss, nobody's gain. 0x4b4380 (task 5) is a **ship being
  sold** (message 0x248f, accepted only in move state 6 with a building
  contact): its owner paid 0x4e4ce0 amber and 0x4e4c80 fish (not read),
  then its own player: nothing counted. A mission's ship (message 2 with
  record +0x10 == 0) whose cell fails 0x499a40 with its draught, or whose
  slot an earlier one holds, is removed with -1 (0x4dc787): its owner's
  ship lost.
- The gorgon (0x401f70) and siren (0x403ba0) report at their tick (state
  0, gated `+0x90 >= +0xa0 + +0x9c`, the siren's `+0x74 >= +0x124 +
  +0x120`) once, when their life is below 1 and their killer field
  (`+0xb4` / `+0x138`, -1 from the constructor) is set, then are removed.
  Only their lightning (0xda93, life 0) and tornado (0xda96, each blow
  leaving the life at 0) handlers write that field, with the caster; the
  shot path reports on every blow leaving it below 1 and writes none.
- Sea creatures, gorgons, sirens and the leviathan report on **every**
  blow that leaves the life at or below 0 (0x490c70, 0x4014d0, 0x403480,
  0x494090 test the life after the blow, not before): a dead sea creature
  struck again before its tick removes it is counted again, as in the
  port.

A building's `+0x1ac` is never stale: each writer (0x4246b2, 0x426f37,
0x428633 / 0x428740, 0x4158d9 / 0x4159d0 / 0x4159aa, 0x427472) is followed
in the same handler by the building's fall; fire (0x158bf) writes none, so
a building it fells is put down to no one. The prize of player 16 (either
side) is not taken (0x4b6f20 at 0x4b7169): `+0x251 = 4`, `+0x7f` the
taker's `+0x24f`, reported at its sinking; "boarded" is counted when the
fight is won (0x4a91a0 at 0x4b6559), in both cases.

The port: a build called off reports -1 (`scene::yard`), the misplaced
mission ships are removed at load, player 16's prize sinks at once put down
to its taker. The gorgon and the siren are ported so (`scene::gorgon`,
`scene::siren`): every blow leaving her at 0 reported at once,
lightning's caster when she goes. Not ported: ship sales (the port has
none); the
order in which mission records reach message 2 (the port's list order).

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

The wizards (CastleClassTy, `Castle.cpp`, vtable 0x5949fc, handler
0x410ca0, his tick 0x410790; ported in `scene/castle.rs`, a game tick at
a time) cast real spells (0x42f580, free: the spell's count `+0x1b4
+0x10 + 4 x spell` raised by one first) on a cell chosen near the
castle. The sea castle (0x42, the wind wizard; prefix `CSTLSEA`, offsets
6, 12) casts spell 3 (`+0x1f0`), the waterspout; the war castle (0x43,
the fire wizard; `CSTLWAR`, 4, 13) spell 7, the fireball (0x410ca0 at
message 2).

- **Generator.** His own (`+0x18`), seeded `[game +0x2ee5] x id` by
  0x427f50 at message 2; 0x410ca0 passes every message to 0x427f50 first
  but 0xda96 to the sea castle. Then at message 2 (state not 0xff) it
  keeps record fields 22-24 (`+0x1e0` level, `+0x1e4` reach across,
  `+0x1e8` down); a new record (field 4 below 2) draws the first cast's
  tick `+0x1ec = now + (1250 - 125 x level) x (r % 21 + 80) / 100`
  (unsigned; halved from now if `+0x1f4` is set), a saved one (field 4
  2) takes `+0x1ec`, `+0x1f4`, `+0x1f8` from record `+0x1c4..+0x1cc`;
  then every one draws the ambient sound's tick `+0x1fc = now + (r % 41 +
  60) x 375 / 100`. The same generator is drawn by the aim at him
  (0x423520) and the fire (0x4240e0).
- **Tick.** Message 0: LandObj's (0x427f50 -> 0x425990, whose picture
  step vt+0x3c 0x4230f0 runs first), then 0x410790: the attacker
  (`+0x1f4`) is forgotten once `+0x1f8 + 3000 <= now`; at `+0x1fc` the
  ambient sound 0x50e570(mode 5, cell, 0x8d sea / 0x8e war) and its next
  tick drawn; before `+0x1ec` nothing more; standing (state `+0xa4` 1)
  he turns to casting (4) and sets the picture (vt+0x38); casting, at
  the picture's frame `+0x45` 7 (the frame about to be drawn) he casts.
- **Picture.** vt+0x38 (0x4101d0): `<prefix>SPR_<state><level>`
  (`%sSPR_%1u%1u`, level `ceil(+0xa8 x 2 / 67)` at least 1, 3 in states
  0 and 3), its frame kept unless past the last (then 0), the step time
  `+0x81` 0 and `+0xbc` 0 (a step at the next tick); in state 1 the mask
  `<prefix>MSK_1<level>`. The cycle table `+0xc0` = 0x5949e8: 300, 20,
  100, 75 ticks for states 0-3 and 64 (0x5949f8) for 4; a frame lasts
  `cycle / n`. `CSTLWARSPR_1x` has 10 frames, `CSTLWARSPR_4x` 32,
  `CSTLSEASPR_13` 1, `CSTLSEASPR_43` 41. A blow changing the level sets
  it again (0x423d60). 0x4230f0 steps (`+0xbc` 0 or `now >= +0x81 +
  delay`): the frame drawn and one added (0x427b30 -> 0x4233f0); past the
  last, state 1 back to frame 0, state 4 to vt+0x48 (0x4106f0: with
  `+0x1ec <= now`, the cast not made, the next cast's tick drawn as at a
  cast) then state 1 at frame 0 with the picture set again. So a cast
  picture entered at frame 7 or beyond (the standing loop's frame kept)
  steps past 7 at once and casts nothing that round.
- **Where.** Each cell within `+0x1e4` across and `+0x1e8` down of his
  cell (`+0x74`, `+0x78`), on the map, row by row, sums the 3 x 3 about
  it, row by row, by the object in each cell's `+0xc` that is a
  player's (`+0x57` 1), not of nation 4 and there (vt+0x2c): a ship
  (class 200-211) whose `+0x5ab` is not 0 (the cell it has reserved
  ahead, as `y x width + x`: set by the step 0x4a3a70 at 0x4a3cb4 when
  not its own cell, by 0x4a39b0 at 0x4a3a5d and by 0x4b96c4; -1 when
  none, written by its other 19 writers (0x4a30db-0x4dfe7b); so only a
  ship reserving cell (0, 0) is left out; the port's `Ship::ahead`) or a fort (0x34-0x36) adds `10 - d`, `d` that
  cell's distance from his (0x5576d8; below 0 beyond 10), any other
  building 0x32-0x3e 1; the sum so far is multiplied by 4 when the
  object's id (`+0x4c`) is the attacker's. Cells whose sum is not 0 are
  listed (x, y, sum) and the sums added (unsigned). With a total not 0:
  one draw, and the first listed cell whose running sum (signed) is not
  below `r % total`; the cast there, the cast sound 0x50e570(mode 1,
  cell, 0x82 / 0x83), and the next cast's tick drawn as at message 2
  (halved from now while attacked). With a total of 0 nothing (the
  test runs again each tick while the frame is 7).
- **Attacker.** The drain 0x4105c0 sets `+0x1f4` to a blow's shooter
  (`+0x16`) and `+0x1f8` now (shot.rs `land_shot`).
- **Sounds.** Events 0x8d, 0x8e, 0x82 and 0x83 have no `SND_n` list:
  all four silent.

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
| 11 spare | 0x481fe0 | 0xd (0x47f6b0) | the same without handing out, order 200 mode 1: the ships sold (0x483900) |
| 4 commander | 0x482160 | none | the grand attack, founding settlements, spells |

The gathering steps read their zone's state each turn after the first
(0x483320: query 0x79, 0x4c8da0). Task 4 state 5: 0x1f5 with result 1,
10 or 0xb goes to p[7], p[6] or p[8] (`+0x8c`, `+0x88`, `+0x90`; all 0 as
the role handlers set them, so the resource is switched and state 2);
any other answer leaves the time test (state 7). Task 5 states 7 and 10:
0x1f5 pushes 10 or 2, and the time up pushes it again. A flotilla whose
task is no zone gets no answer (0x4a0d00 leaves `out` alone; 0x47dc60 and
0x47dfb0 then read their uninitialised `local_1c`): the port takes it as
no answer.

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
-10, medusas and sirens -10 + d within 5, lava -1 x its strength; a nest's
reach only right of the 31 x 31 cells' left column, 0x40b920 testing `x0 <
cx` with the x 0x484900 passes). Each cell is closed to deep ships (1),
beside the base's ports and yards (8), on a lane (9: the paths from each
port's front to each amber, 0x558fa0, 0x559470), taken by an earlier zone
(10) or open (0). A square needs as many passable cells as ships (and, on
the first pass, none taken); it beats the best (0x408bf0) with fewer cells
beside the harbour, then more weight (over its passable cells), then nearer
the town (`max + min / 2` to its middle), then more open cells, fewer on
the lanes, more passable).

**Town orders** are run by the town's land fleet (`LFleet.cpp`, see
The land fleet's pass below): the table at 0x5ac7c8 (7 dwords:
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
after 10-25 counting passes otherwise. A flotilla's ship request is order
0x51-0x5b by type, and 0x5c for type 0 (0x47fe60, table 0x47ff64), whose
part has field 8 = 1: the price tests are skipped (0x428f20 passes it to
0x43da20 as param_7; -2/-3 become 0) and the yard command 0x20 carries
byte 0xf = 0xff, which 0x416a40 takes as "do not pay": the boat is built
free, without the "ship built" sound (0xe4-0xe7). (The yard's queued
branch, a berth taken, would charge, but the town only forwards to a yard
whose berth is free.) A ship part refused with -1 (the yard idle but its
berth taken, or the town busy) waits without counting down (part flag 4;
for a part of any town, the last town's result is the one held), and,
only for a ship part (f4 == 4) refused -1 in a pass after one that
started nothing (`+0x18`), counts its `+0xe0` (0x4295cb); at 25 it is
reset, the town command 0x28 with the one-byte payload 3 (0x520cc0) goes
to the part's town (every town of the land fleet when `+0xc < 0`), and
every other state-1 part of the same op, f4 and f5 whose last result
(`+0x70`) was -1 has its `+0xe0` zeroed; no try is spent. The town
(0x441ba0 case 0x28, payload 3 only) runs 0x4415a0: for each of its
yards (class 0x149c) answering `0x4e82f0(yard, 1, 0xff) == -1`, the ship
in the berth cell's object slot (vtable +0xb8), unless it is the town's
own (same nation and slot, `+0x25d & 0x20`, task `+0x72f == 1` with
`+0x73c` the yard's cell: sailing there to be mended), gets
0x4b4b50(ship, 100, 0): a neighbouring cell it may enter (0x499aa0 with
the direction 0x59791d[dx*4+dy] and its draught `+0x592`) drawn with the
ship's LCG, else one held by a lower-priority (`+0x5c7 < 100`) own ship
(0x499990), and the move event 0x80 to it (see "Shipyards' berths").
The port holds the part (`PartStep::Berth`), counts `+0xe0` and posts
the command (`Order::TownBerths`), which clears the berths at the next
tick; the town busy is, here, only its going up or being made a town.

**The land fleet's pass** (0x42b910, the fleet's message 0 every tick;
constructor 0x42b370). Fields: +0x14 the last pass started a part, +0x18
"the previous pass started nothing", +0x1c the last pass's tick, +0x20 the
next order id, +0x24 the priority threshold (0x7fff, changed only by a
queued op-0x3f0 message, which no code builds), +0x28 woken, +0x2c the
list (count +0xc), +0x30 the
queue (0xe4-byte entries by priority, highest first, after their equals,
compared unsigned: 0x42c430), +0x34 the orders. A pass runs 50 ticks after
one that started nothing; after one that started a part, only once woken
and 3 ticks after it (0x42b94c-0x42b98d). A pass that runs first drops
from the list (0x42b98d, before the queue walk 0x42ba2a) the members gone
(0x51f1f0 finds none) or in `+0xa4` 2, 3 or 0xff, the others kept in
order; between passes they stay (`Scene::brain_land_drop`; a
computer's in `land_pass`). A computer's list is its towns, then its
settlements, all of them (0x487230 joins each, 0x484000 to +0x30, the
bases only the first 9), and a settlement new to it (0x488960, 0x5d95
case 0, whichever base takes it; 0x49bd60 leaves one joined already); a
computer's ask carries its base's town (+0x60, read when it asks,
0x482f50, dword 6), which 0x42c050 puts in each part's dword 3 when the
order has flag 8 (all the computer's: 0x88, 0xd8, 0x488). Every handler
looks for dword 3 in the list (waiting 0x428a70, 0x428f20, 0x429ae0,
0x429f60, 0x42a860; under way 0x428e10, 0x4297d0, 0x429e10, 0x42a2f0,
0x42b080) and the casts (0x42a500) try the list: a waiting part whose town
is not there spends a try (counted in a pass after one that started
nothing, failing at 0; its last result +0x70 unwritten), one under way
fails (+0x50 4); the berths' command (0x4295cb) goes to dword 3 as it is.
The base keeps its town until the town's object is removed (LandObj
0x425990 message 3 posts 0x5d9b, which clears +0x60 by 0x484b20 /
0x482c50): a fallen town at its ruins' last step (0x4230f0, see "The
fallen building's object"), a sold one at 0x424740; the port clears it
when the town's object is gone (`computer_news`). Wakers: the walk itself for every
entry it finds done or failed, 0x42c050 (an order put or refused), and the
town at the end of each command it handles (0x441ba0 at 0x44393d; the port
wakes at a part's start). A computer's parts send their town commands
through the outbox (0x520cc0), reaching the town at the next tick, and
read nothing back at once: a build (0x428a70) sends the place it chose
and is under way, done when the town's count of the class grows
(0x428e10); a ship part (0x428f20) sends command 0x20 to the yard that
answered 0 (0x4e82f0, a direct test) and is done (`Order::YardShip`); an
upgrade sends its command and is under way until the class changes; a
repair part (0x429f60) sends command 0x1e for the first building the
town may mend (its tests read at once, `mend_check`; `Order::TownMend`)
and is under way; a cast part (0x42a500) sends command 0x22 (the
town's temple casting at the next tick: `Order::TownCast`,
`TownCastOn`, `TownReef`). (Not ported: the cast's own wait as a
land-fleet part, queued and started on a pass, 0x42b910; the port posts
it at the computer's turn.) A study part (0x42a860), its tests passed
(0x4404e0 twice, without and with the price), sends command 0x21
`{code, sub}` (1 arming 3, 2 ram 4, 3 armour 5, 17 rigging 9, 18-26 the
workshop 6-8 with group 1-3, 27-32 the smithy's kinds 4-5 with 4-6, 33/34
code 0xe with 7/8, 35-38 codes 10-13) and is under way; the town (0x441ba0
case 0x21) hands it to its first school (0x3c), smithy (0x38, codes 4 and
5 with a sub) or workshop (0x37) answering idle (vt+0x9c(0xff) == 0), or
for code 0xe to its temple (0x3d sub 7, 0x3e sub 8), which pays and
starts at the next tick (`Order::TownStudy`, `TownLearn`). A town order
is asked as an op-1000 entry (0x482f50 -> 0x4a09d0): dwords op, flags
(request +0x20, 0), priority (the key), code, order flags (+0x24: 0x88 the
base scripts', 0xd8 their order 0x60, 0x488 the ship requests; 0x47d2e0,
0x47fe60), the key again, the base's town, request +0x14. The pass:
(A) each order in state 0 or 2 queues the parts it is ready for (field 20:
0 and 3 at once, 1 once the part of field 21 is done (failed: the order
fails), 2 once all before are done; a part of field 20 = 3 under way is
queued again whenever no entry of its op waits; a failed part is queued
again with order flag 0x10, else the order fails); an order all done with
flag 0x40 starts over; one ended marks its op-1000 entry done or failed and
is forgotten (its parts still queued run on). (B) +0x18 = !+0x14, +0x14 =
+0x28 = 0, then each entry in turn: waiting ones are started (op 1000 by
0x42c050: code 0x86 to 0x73 / 0x74 / 0 by nation, 0 refused; the order put
before the first of no higher priority, with flag 0x80 every part of the
order's priority, its entry under way), under-way ones checked (0x428e10,
0x4297d0, 0x429e10, 0x42a2f0, 0x42b080); +0x14 is set when a start leaves
its entry under way or done, and **the walk stops there** (the loop runs
while +0x14 is 0, 0x42bf69; after it only the no-op at 0x42bf6f, +0x28 set
1 when already 1, 1500 ticks on): one start a pass;
done or failed parts write back to their order and leave the queue; a
waiting entry at or above +0x24 stops it too. Waiting tries (0x595170: 20,
25, 10 by op) and under-way waits (0x595198) count down only when +0x18 is
set. A build done (0x428e10) has every other build of the class under way
at the town count one more before it and wait afresh. The asker reads its
entry by its 20 dwords (0x482ff0 -> 0x42c870): its state, done or failed
taking it off the queue, 5 if none (a request then fails, 0x480270; the
base script waits on, 0x47d2e0). Order 0x60 (flags 0xd8) never ends: it
repeats and restarts its failed part; the base script does not wait for it.
The town's command reaches it at the next tick ("When a command
arrives"; the port's are given at once). Not settled: which towns
have +0x1bc (the waker's test); a brain's data lowering +0x24.

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
score first, ties as it leaves them). A cell's third object slot (+0x1c)
holds a treasure lying there (treas.cpp 0x406e70), a reef or whirlpool
(record 5, 6), a sheep flock (record 4, 5) or a volcano at its own cell
(record 6, 7, 0x445e20); a volcano also sets layer 6's type to 4 on its
2 x 2 cells. The town's check (0x42dae0) refuses both whatever stands
there; 0x40cca0 closes a slotted cell but keeps it passable for the
regions, and a volcano's as unbuildable; 0x40c5f0 and 0x409b30 close
both. The zones' influence finds a volcano at its own cell only.

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
+0x19 and +0x68d with +0x1d). The third (payload `+9` of `#`) is read by
the ship: its low byte is the weapon positions (+0x685), bit 25 turns ram
attack on and bit 26 board attack (0x4e2c34-0x4e2d03). So the reserve's
and spare's 0x1ffffff gives guard alone, every other task guard, ram and
board. The 25 call sites of 0x482e80 / 0x482eb0 (always 0x482eb0 with
1: order 0x72's first field 0, so no sub 2 first, 0x4c4410 being for
100): the making of a flotilla (0x483e10 at 0x483f47, for a group of
type 10: 0x483df0 gives 10 for every role but the manager's 0, 11; no
ships yet); task 4 state 5 on its first turn after the gather order
(0x47dd51); task 5 state 0 after its rolls (0x47e0ce); task 7 state 2
first (0x47ef8f); task 8 state 2 first (0x47ebc1), state 5's first turn
before the guard order (0x47ec96), state 0xc's first turn before the
attack order (0x47ed8d); tasks 9 / 10 state 2 first (0x47e40b /
0x47e80b), state 10 first (0x47e5b9 / 0x47e968), state 0xc's first
turn before the attack (0x47e62e / 0x47e9dd); tasks 0xb / 0xd state 5's
first turn after the roll, before the move (0x47f5b4 / 0x47f7c3, third
0x1ffffff); and letter 8 in the role handlers of roles 1-3 and 5-9
(0x482950, 0x480680, 0x480840, 0x480a00, 0x481a10, 0x4815a0, 0x480c40,
0x481100) and of the reserve and spare (0x481e60, 0x481fe0, third
0x1ffffff); the manager's (0x480580) and commander's (0x482160) have no
case 8. All take players +0xe18 (-1 at the start, 0x487d47) and classes
-1, third -1 but where said. A computer's ship also gets its flotilla's
three when the group gets a new task block (below, 0x4c5b30) and, joined
(0x483fb0: 0x4a0480 op 2, notice 300 by 0x4a1340 when the group has a
block, `+0x2d`), from the block's case 300: guard 0x4d1ea0, attack
(0x70 / 0x21) 0x4d6960, founding 0x4cba10, dock 0x4db110, treasure
0x4da0a0, the forts' 0xd2 0x4dc2a0 (a fort joining), the gather zone 0x4c9e30, the move 0x4cc6a0 (only a join
0x4cc650 sends to the order's cell). Ported (computer.rs
`flotilla_behave`, `new_block`, `read_joins`): every site, the blocks'
sends, the joins' (read at the computer's tick, not the navy's). The
computers' moves (0x483760, from tasks 4, 5, 0xb and 0xd: order 100
`{x + w / 2, y + h / 2, flags +0x154, tolerance w / 2, at least 1}` to
the middle of the flotilla's zone) go through the move order as the
brains' do (computer.rs `flotilla_move`: 0x4ccc30, the formation
0x4cc970 or else 0x4cc650 to each ship), and its pass
(`flotilla_move_pass`, 0x4ccd80 from the navy's tick: the joins 0x4cc6a0
newest first, each planning the formation anew and, only when that
sends none, sending the join to the order's cell with the group's three;
then, none heard and group `+8` over 100, `+8` zeroed and each ship
sent the three when its `+0x689 / +0x68d` differ, made to forget a
blow (0x4cf760; the report 0x4a21c0 not ported) and, lying still more
than a cell off its cell, sent there again; `+8` grows by the tick).
Group `+8` (`Flotilla::elapsed`) is the move's and the attack's; the
gather zone keeps its own (not ported: that it is one counter carried
from block to block).

The computers' attack zone (0x483650, from tasks 8, 9 and 10 state 0xc,
whose `+0x154` is set just before to `(difficulty == 0) << 16`
(0x47ed7b, 0x47e5e3, 0x47e992): order 0x70 `{area, 0, 0, flags +0x154,
players +0x14c, third +0x150, classes +0x148, .., -1}` -> 0x4d6a90,
block `{0x70, 0x21}`, flags at `+0x2c`; computer.rs `attack_zone`,
`zone_foe`, `attack_pass`). The target (0x4d6640, block `+0x1c`): the
area's cells row by row within the map, each cell's occupant (`+0xc`)
that is there (vt+0x2c), whose player bit (`+0x54 x 4 + +0x55`) is in the
group's players and who is at war with the computer (0x4efe20 < 0),
whose class passes the group's classes (0x50ebb0); a list of at most 10
by strength (0x50ea70), each new one going before the first it is
stronger than, else at the end, one already listed skipped; one of
class 0x46-0x48 marks the index it went in at (later insertions do not
move the mark). The id of the marked entry, else the first; 0 none. With
a target each ship is sent at it (0x4d0140, command 0x16), or with no
strength to its cell (0x51f1f0 / vt+0x28, 0x4cf070); none, nothing. The
pass (0x4d6cd0, each tick): the notices newest first (0x4d6960: a join
in the group sent at the target and the group's three); when none was
read, group `+8` is over the period (0x5a; a computer's side 3 / 4
outside the campaign: 0x78, 0x4b, 0x3c by its level) and the group has
ships, the target is looked for again: none, the notices freed
(0x4d6520), the block kept (so no join is heard, and the area is looked
at each tick); else `+8 = 0` and each ship sent the three if its masks
differ, made to forget a blow (0x4cf760), then a draw of the navy's
generator `% 100`: with flags `>> 16` 1 above 60 (2: above 30) nothing
more; a phantom (`+0x253`) nothing more; one on a boarding or a ram
(`+0x25d == 0x10`, the task 0x4aa840) nothing more; one that takes a
contact (`+0x261 & 0x1000`) looks about it for a boarding or a ram
(below) and, with a find, is sent to board it (0x4d0190: command 0x17
`{id, 0x45}`) when its boarding odds are 101 or more, else to ram it
(0x4d01d0: command 0x18) when its ramming odds are; else, and without
the contact, sent at the target as by the order. Notice 0x13a: see
**Prey notices**. Not ported: command 0x16 with target 0 (a join with
none: not sent; 0x4ab3f0 keeps such a record, 0x4ac050 finding nothing,
but what the pending group's turn in 0x4abe50 does to the ship's
running task is not traced); which of two ships in a cell holds `+0xc`.

`+0x261 & 0x1000` (taking a contact) is set on every finished ship
(0x4e6930: all bits but 11 for types 0 and 1, but 8 for the rest); off
on one being built (0x2000 alone, 0x4dfe4d), held (0x4b3882, 0x4defda),
and on the prey of a boarding or a ram while its contact machine runs
(0x4c3410 clears it at 0x4c3543, 0x4c3ec0 at 0x4c3eef; 0x4c3af4,
0x4c3f19 and 0x4c40e4 set it again).

The pickers (0x4d6cd0 calls them with reach 2, the block's flags' low
16 bits as the mode, 0 from every computer task, and whether the ship
is in the area, which only matters for reach below 2): 0x4bad90 for
types 10 and 11, else 0x4baae0. A find in either is a cell's occupant
(`+0xc`) that is a ship (0x50e600 200) other than the picker, either of
side 2 (`+0x57`) not of class 0x1fb or of another nation or slot at war
(0x4efe20 < 0), whose player bit is in the picker's players (`+0x689`);
its boarding odds 0x4ba900 (the picker's `+0x53c` x 100, x 120 for a
boarder, type 4, / the find's, at most 200; 0 with none of its own, 200
against none) and ramming odds 0x4ba950 (see "Ramming odds"). Of
several, for each odds the first greatest (0x4ba810 mode 0; mode 1,
three draws of 0x4a2540 over the odds the ring weighs, x 1 / (5 x ring
+ 1), the greatest of the three kept, is not used here).
- 0x4baae0: the cells within `reach` of the picker's (types 10 and 11
  with reach below 2: 2, or 3 inside the area), clipped to the map, row
  by row. One find: its two odds, but only its id for the boarding (the
  ramming id stays 0, so a lone find better rammed gives nothing).
  Ramming odds above the boarding's: the ram (boarding odds 0), else
  the boarding (ramming 0).
- 0x4bad90, reach below 4: the rings 1 to reach about the picker's cell
  (0x597928: 8 cells, 0x59794c: 16, 0x597990: 24; `(dx, dy)` shorts row
  by row), within the map, each on its own: a find with either odds
  above 100 gives the ram when the boarding's are below the ramming's,
  else the boarding; a later ring's replaces an earlier's. Reach 4 and
  over (not ported, not called so here): the square as 0x4baae0's, its
  finds needing `+0x261 & 0x1000`, not the players.

Port: computer.rs `attack_pass`, `closing_in` (the port's ship with a
`Task::Board` or `Task::Ram`: its loading, also 0x10, is not told
apart), `takes_contact`, `close_pick`, `board_odds`, `ram_odds`
(`combat::ram_odds`).

A rewrite of the block (the same kind) keeps the notices freed; a block
of another kind makes them again (0x4a1150). The status the tasks read
(0x4d70d0, by 0x4a0d80 for `{0x70, 0x21}`; port `Scene::report`): six
words zeroed; with no ships in the group `{0x1f5, .., result 1}`; else
`+1` the ticks since the block's `+8`, each ship of the group's list
that exists counted armed (`+2`: a ship, 0x50e600 200, of type neither 0
nor 1, for which 0x4bc270(7) holds: types 8-11 always, the others with
an item in a position `+0x289..+0x291`) or not (`+3`); then the area's
target (0x4d6640): armed ships and a target, 500 (running); else 0x1f5
(done), result 10 with armed ships, 1 without. Tasks 8, 9 and 10
(0x47eb70, 0x47e3c0, 0x47e7c0 via 0x483730) read only the code.

The human's attack zone (0x49d9d0) gives the selection's group its
behaviour first (0x49da43: 0x4c4350 `{0, 0x4a2f10(player), -1, -1}`;
0x4a2f10 every player bit but its own and those of players 0-19 not at
war with it): command `#` sub 1 to each ship, queued (order.rs `Zone`).
Its block (0x4d5f50, `{0x70, 0x20}`, tick 0x4d61d0, status 0x4d63d0) is
ported (scene/human_attack.rs, `Order::AttackZone`: the selection's group,
`Scene::human_attacks`). What is read of it:
- 0x4d5f50: a block of another kind freed (0x4a1c60); none, 0x4d4c30
  makes one (0x65 bytes zeroed; `{0x70, 0x20, now}`, then as below
  without `+0x21`, and the notices 0x4a1150; failing, 0); else the block
  is rewritten: rect `+0xc..+0x18` = args[0..3], a copy of the W x H
  shorts at args[4] in `+0x1c` when given, `+0x37` args[7], `+0x3b`
  (short) args[10], `+0x3d` args[8], `+0x41` args[9], the mode `+0x20 =
  1`, `+0x21 = 0xb`; 0x4d4860 lists the targets in `+0x2b` (a list of
  0x20-byte records by 0x4d49f0: the args[5] ids given, each existing
  with its id, class, strength 0x50ea70, vt+0x10, cell vt+0x28 and
  `+0x64`; with none and a rect, 0x4cd300's); 0x4d41b0 counts the
  group's ships of some strength into `+0x49` and sums their strengths
  into `+0x4d`. Then 0x4d4260(0) (the formation's place `+0x51..+0x57`,
  by 0x4d0f40, or with 1 the slots by 0x4d5100 / 0x4d59f0 and mode 3)
  and 0x4d5850 (0x4d5360, 0x4d43b0, 0x4d5560).
- 0x4d61d0, each tick: with no notice heard (0x4d4e50) and group `+8`
  over 0x46, `+8 = 0` and each ship of the group: lying still (`+0x58c`
  0) with a slot in `+0x33` (0x4d4bd0 by its id): mode 1 with the slot's
  `+0x23` or `+0x25` -1, 0x4d5850; modes 1 and 3, more than a cell
  (0x557440) from the slot's cell (`+0x10`, `+0x14`), a move there
  (0x4cf070); mode 2 (still or not) 0x4d60c0 (the area's target,
  0x4d6640, then 0x4cf070 or 0x4d0140); after the ships, mode 1 re-plans
  the formation (0x4d5a80: 0x558fa0, 0x4d5100, 0x4d59f0, 0x4d5970,
  0x4d58b0). `+8` grows by the tick.
- 0x4d63d0: as 0x4d70d0, the target by the block's `+0x3b`, `+0x3d`,
  `+0x41`.
The rest, read (decomp `humanblock*.c`):
- The caller is 0x49d790 case 10 (the interface's fleet commands; the
  address 0x49d9d0 lies in it): fifteen dwords zeroed, `[0..3]` the
  payload's rect `{x, y, w, h}` (shorts at `+1`, `+5`), `[8]` 0x4a2f10's
  players (`+0x3d`), `[9]` -1 (`+0x41`, classes), `[10]` -1 (`+0x3b`,
  0x4d6640's kind), `[4]` (no cell mask), `[5]`, `[6]` (no ids) and `[7]`
  (`+0x37`) 0.
- Block fields: `+0x23/+0x25` the goal cell (shorts), `+0x27/+0x29` the
  formation's cell, `+0x2b` the targets (0x20-byte records `{object, id,
  class +0x50, strength, vt+0x10 & 0xffff, x, y, +0x64}`), `+0x33` the
  slots (0x18 bytes `{ship, id, target, target id, x, y}`, kept by id:
  0x4d4b10 rewrites or appends, 0x4d4bd0 finds and may remove), `+0x45`
  the time of the next re-plan, `+0x51..+0x57` never non-zero here (only
  0x4d4260 writes them, after requiring them non-zero), so 0x4d4260
  returns at once.
- 0x4cd300 (block `{0x70, 0x20}`: rect `+0xc`, mask `+0x1c`, players
  `+0x3d`): the rect's cells within the map, row by row, a cell masked
  -1 skipped; the cell's object at cell `+0xc`, else `+4`, else `+0x10`
  (the cell record base `+8`; read at 0x4cd4a2, 0x4cd4ad, 0x4cd4be: grid
  `+0x14`, `+0xc`, `+0x18` + 0x18 a cell). Cell `+4` (grid `+0xc`) is
  the cell's occupant slot: taken only when empty by 0x499800 (x, y) /
  0x499830 (index) and cleared only by its holder by 0x499860 / 0x4998a0,
  their callers a ship's placement and steps (0x4dc630, 0x4dfe00,
  0x4e2180, 0x4a39b0, 0x4a3a70, 0x4b93f0), the land creatures' jam
  0x1f7 (0x415ee0), 0x41d4a0 and LandObj's mending (0x425990); `+0xc`
  (grid `+0x14`) is written by the harpies (0x41fbd0) and the temples'
  war creatures (0x43aa10); `+0x10` (grid `+0x18`) by the sea monsters
  (0x490d60, 0x491780). Its player (`+0x54 x 4 + +0x55`) in the
  players and at war (0x4efe20 < 0); a ship skipped on the cell it has
  reserved (`+0x5ab`, 0x4cd549); classes 0x32/0x33 and the parent of a 0x1f7 (0x415df0,
  0x415db0) once; strength a ship's `+0x479`, 0x34-0x36 the sum of
  0x446710 over the 12 slots of `+0x4d0`, 0x3f-0x41 those of `+0x20c`,
  others 0; listed when vt+0x28 gives its cell. 0x4d49f0 merges by id.
- 0x4d5850: 0x4d5360 (a slot for each ship of the group that is a ship,
  its cell) and, with any, 0x4d43b0: non-zero, 0x4d5560 and mode 2; zero,
  mode 1.
- 0x4d43b0: no targets, goal `(-1, -1)`, 0. Two or more: draws `a = %3 +
  1`, `b = %3 + 1` of the navy's generator and 0x4cd120(list, n, `b x 10
  << 16 | a`) puts one first: weights by `a` (1 strength, 2 `+0x64`, 3
  both); `b` 1 the greatest (the first of equals), 2 a draw `% n`, 3
  five roulette draws `% sum` (a sum 0: one draw `% n`) keeping the
  greatest weight hit; index 0 kept, else swapped to the front (a failure
  is goal -1, 0). The first target's cell the goal when water (0x4999e0);
  else from the group's centre (0x4c56e0: the mean cell of the group's
  ships, `+0x5a3/+0x5a7`, and its 0x50e600-kind-0x32 objects at
  `+0x74/+0x78` weighing 2; that mean refused by 0x499990 with such an
  object in the group, the last ship within `W + H` of it by `|dx| + |dy|`, below) the route 0x4d0e70 (on the shallow grid 0x5c93bc: the
  start closed none; the goal closed 0x55ba50, else 0x55ca10), its last water step the
  goal; none, 0. Then the formation 0x4d5100 about the first target's cell with slot
  class 6. Every ship more than 6 (0x557440) from its slot: the centre's
  route to the goal (0x4d58b0: 0x55ba50; 11 or more steps, its 7th step,
  1; fewer, 2) and a class-0 formation there sent (0x4d59f0: slots kept,
  0x4cf070 each), `+0x27` that cell, `+0x21 = 10`, `+0x45 = now + 300 x`
  the planner's value; returns 0 (approach). Else each slot takes the
  class-6 cell, the count of slots (plus one for each whose cell's sum of
  player `+0x3c` grids of players at war is above the ship's `+0x479`,
  when player `+0x40` is set), `+0x21 = 0xb`.
- 0x4d5100: a 0x24-byte record per ship of the group (cell, `+8` the
  class, `+0x10` ship, `+0x14` id) to 0x5614e0 with the grids by draught
  (0x4d50b0), with player `+0x40` the per-cell sum of the `+0x3c` grids
  of players at war as the third grid; the planner's value returned
  (0x55f870: the greatest goal-flood value at a ship's cell; it then
  marks each ship 1-3 by thirds), negative clears the count. Its draught
  flag is written after the record pointer has stepped on (0x4d51be `add
  eax, 0x24`, then 0x4d51d5 / 0x4d51e6 `[eax + base + 0xc]`): ship i's
  `+0x592 != 0` lands in record i + 1's `+0xc` (the planner's second
  grid), record 0's stays 0; and the grids are the other way about from
  0x4cc970's: p1 0x5c9398 when a ship has `+0x592` 0, p2 0x5c93bc when
  one has it set. (`formation::plan_records` takes the records so.)
- The threat grids: each player's `+0x3c` (game `+0x3c + 0xba0 p`) is a
  W x H int grid made zeroed by 0x4a7fa0 (with `+0x40` set also the short
  grid `+0x41`), freed by 0x4a85a0, saved and loaded whole (0x4a89e0: the
  twenty 0xba0 records copied, then each grid present); 0x4a7f70 zeroes
  the records and sets `+0x38 = 15`. A ship adds its stamp (`+0xf9`, a
  `+0xf5` square, `+0xf5 = 2 +0x47d + 1`, 0x4e5410: its weapons' worth by
  position, 0x4bf5f0, 0x5c75cc) / 100 about its cell (0x4e5680(1), after
  taking the old one off) when its player's `+0x40` is set, `+0xe8` set
  and `+0x38` ticks have passed (0x4e58a0 from the ship's tick 0x4dec40);
  at its end 0x4e5680(0) takes it off. Nothing writes `+0x40` but those
  two whole-record writers: no byte store at displacement 0x40 in the
  listing is to anything but a stack frame, nor any in the functions that
  address the player records (`[0x5c7508]`, stride 0xba0) at +0x40, +0x24
  or +4. So `+0x40` stays 0: no third grid, no threat count, and the
  stamps are never made.
- 0x4d5560: the targets listed again (0x4d4860 with no ids) and checked
  (0x4d5430: each, newest first, kept with fresh fields when it exists,
  0x51f1f0, and is at war or its player is in the group's `+0x19`; else
  removed); then 0x561a30 deals the targets' cells to the slots' ships
  (below); each ship given the target of its cell: slot `+8`, and a ship
  of class 0xd2 moved there (0x4cf070), the others `+0xc` the target id
  and 0x4d0140 at it (players `+0x3d`, 7). Returns -1.
- 0x561a30(W, H, cells, n, ships, m): `k = min(n, m)`; 1: every ship the
  first cell. Else the cells copied with their 0x557440 distance from the
  first and sorted on it (0x58791f, the C library's qsort, comparing
  with 0x561f20 `a +0xc - b +0xc`; Microsoft's: ranges over 8 split about
  the middle element swapped to the front, the smaller side stacked;
  ranges of 8 or fewer by 0x587a73, the first greatest swapped to the
  end each pass; port `formation::crt_qsort`); the first `k` cells' sums and
  the ships' sums halved (an odd sum rounded up when its half is below
  the first cell's coordinate, sic: halved, not averaged); quotas: the
  first `(2m - 1) / k`, each next `(r / 2 + r) / left`, the remainder to
  the last; each cell's key 0x5574d0 along `(dir(cells' point -> ships'
  point) - 4) & 7` and distance from the ships' point, each ship's key
  along `dir(ships' point -> cells' point)`; cells bubble-sorted by key
  (the farther on a tie), ships by key (the nearer the current cell on a
  tie, the current cell stepping on by the quotas); then ship i the cell
  its quota run reaches. The quotas are set before the cells' key sort
  and move with their cells. The ships' `+0x20` (their key) is then
  zeroed for one fixed ship only (0x561eb9 reads the index left by the
  sort). Port `formation::deal_cells`.
- 0x4d61d0 (each tick; `+8` the ticks since): with no notice acted on
  and `+8` over 70, `+8 = 0` and each ship of the group (as above), then
  in mode 1 0x4d5a80 (nothing under 2 slots): the flood 0x558fa0 on the
  shallow grid from the formation's cell `+0x27` (limit 5000) read at the
  ships' cells, slots newest first: `lo`, `hi` and the ship at `lo` (the
  first of equals in that order). Under 2 ships placed: 0x4d5560 (mode 2
  if non-zero), then with the formation's cell the goal 0x4d5560 again.
  Else `far = |flood(+0x27) - hi| / 2`, `half = (hi - lo) / 2`. The
  formation's cell the goal and `far` at most 9: 0x4d5560 (mode 2).
  Else, `half` 0 nothing; 0x4d5970(ships, half) (ships 1-5: half over 3;
  6-9: over 4; more: ships below half / 2) true: at `+0x21 = 0xb` or with
  `+0x45` past, a class-0 formation about the lead ship's cell, its
  slots' flood spread halved; that not 0x4d5970-wide and 0x4d58b0(lead,
  goal) 1, a class-0 formation at the route's 7th step sent, `+0x27`
  there, `+0x45 = now + 300 x value`; otherwise `+0x21 = 10`. False:
  0x4d58b0(lead, goal) 1, the formation there sent, `+0x21 = 10`,
  `+0x45`; 2, 0x4d5560 (mode 2); 0 nothing.
- 0x4d4e50 (newest first, each removed; the ship by 0x4cc590, a notice of
  a ship not in the group dropped but 0x137): 300 a ship joined: mode 1
  with no slots 0x4d5850, mode 1 or 3 with slots a slot drawn `% count`
  (navy generator) whose ship is sent to its cell (0x4cf070; that
  slot's ship, not the one joining), mode 2 0x4d60c0 with the ship; then `#` sub 1 with the
  group's three (0x4c42f0). 0x136 (at its move goal): mode 1 and
  0x4d4d60 == 100 (the slots whose cell lies within 1 of the goal in x
  and y, in percent; 100 also when all but one are, for 4-9 ships), then
  0x4d5560, mode 2. 0x137 (left): its slot and target entries removed.
  0x13a: its target entry removed, the area's target (0x4d6640) attacked
  (0x4d0140) by the ship.
Settled since: the planner's slot classes, third grid and value
(`formation::plan_records`), the threat grids (above: player `+0x40`
never set), cell `+4` (the occupant slot), 0x58791f's order
(`crt_qsort`) and 0x561a30 (`deal_cells`). Read in porting it:
- 0x4d4860's area path appends each of 0x4cd300's records to the targets
  (0x5577d0, 0x4d497b) and only then merges it by id (0x4d49f0: the first
  entry of its id rewritten), so each relisting (0x4d5560) lists a target
  again: duplicates pile up while it lives. (0x4d49f0's own append copies
  from its second argument's slot, `[ebp + 0xc]`, the fields a dword
  off: reached only on the ids path, args[5], which the human never
  gives.)
- 0x4d43b0 forms the class-6 formation about the first target's cell
  (`local_8`, `local_c`), not about the goal, and writes that cell to
  `+0x27`; with every ship more than 6 away and 0x4d58b0 not 1 (fewer
  than 11 route entries, or none) it does nothing more (`+0x21` kept).
- 0x4d0e70: a closed goal 0x55ba50 with heading 0, tolerance 1; else
  0x55ca10. 0x4d58b0: 0x55ba50 heading 0, tolerance 0; the 7th entry is
  bytes `+0x1e / +0x20` of the 5-byte entries.
- 0x4d5100 reads the slots (`+0x33`), not the group's ships: a slot whose
  ship pointer is null is passed over.
- 0x4c56e0: the mean cell is refused (0x499990) only with a kind-0x32
  object in the group; then the ship taken is the last with `|dx| + |dy|
  < W + H` (the bound is never lowered: always the last ship). Without
  one the mean is returned whatever its cell.
- 0x4d6640 (0x4d60c0, 0x13a, the status) reads the group's `+0x19` and
  `+0x1d`; its second argument (`+0x3b`) is unused.
- 0x4d4e50: 0x136's 0x4d5560 assigns its -1 to the count (not added).
- 0x4d60c0 sends a ship when `+0x25d` has neither 0x100 nor 0x10, or with
  0x100 alone and fire state `+0x5d1` 2, or 1 with no position byte
  (`+0x645`, `+0x657`, `+0x669`, `+0x67b`) 3.
- Objects' `+0x64` (the weight of 0x4cd120's modes 2 and 3) is written by
  the navy as it takes an object on (0x4a8150, players 0-20): a ship
  0x59b3b8[class - 200] (230, 300, 330, 340, 240, 210, 150, 140, 300,
  350, 350, 350), the Argo (`+0x259`) [0x59b3e8] 500; classes 0x32-0x4a
  (0x50e600's kind 0x32) 0x59b3f0[class - 0x32] (120, 150, 300, 400,
  500, 500, 500, 650, 750, 750, 300, 750, 750, 800, 900, 1000, 400, 300,
  400, 200, 330, 300, 300, 0, -1); the base constructor 0x50b960 zeroes
  it. (Its other writers in the listing were not traced.)
- 0x4cd300's strength is a ship's `+0x479` alone and a fort's 0x446710
  sum not divided by 10; 0x4d5430 then refreshes it with 0x50ea70.
- A sunk ship leaves its group (0x4a8410, the Navy's message 0x83, ->
  0x49bef0), posting 0x137.
Ported: all of the above (`order_attack_zone`, `run_human_attacks`).
Not ported: fireballs and tornadoes holding
cell `+0xc` (not objects in the port); the `+0x5ab` skip (the port's
occupants hold only the cells the ships are in); nature objects' player
in 0x4cd300's and 0x4d5430's tests (the port's ownerless creatures are
no player's, as in 0x4d6640's port). 0x13a's senders: **Prey
notices**. The status 0x4d63d0 is ported, its readers (the
tasks' 0x4a0d80) hold brains' and computers' groups only. The port's
per-ship `Task::Zone` stays for the guard zone.

A fort of the forts' flotilla takes the `#` too (below).

**Ships' behaviour and masks.** The constructor (0x4dc3f0) zeroes
`+0x5d1..+0x695`: guard, ram and board off (`+0x684`), the positions
`+0x685`, players `+0x689` and classes `+0x68d` 0, fire state `+0x5d1`
0. The only writers of the masks are 0x4a9e30 (event 0x100 kinds 4, 6, 8,
10: guard, ram, board, aggressor on; `+0x685` = record byte `+0x11`,
`+0x689` = record `+8`, `+0x68d` = record `+4`, then fire state 4 unless
`+0x5d1 != 0` with `+0x25d & 0x100`, the fire task 0x4ab3f0 sets at
0x4ab580) and 0x4ab3f0 (a kind-2 fire record: guard on and the masks;
kind 5 in its list zeroes them all, though 0x4a9e30 never lists kind 5;
the sender of kind 2, 0x4a9e30's split by positions, not traced). Kinds
5, 7, 9, 11 clear the switch alone. 0x4e7220 sends kind 4, and 6 for
ship kinds 5 / 10, 8 for 4 / 11, with players and classes -1, positions
0xff: from the map loader for every ship of the map (0x4dfa90 at
0x4dfd12: record `+0x10 == 0`; the yard's 0x4dfe00 and 0x4e2180 do not)
and at the launch of a human's ship (0x4b3fcf). So a computer's or
brain's yard-built ship sits off guard, masks 0, until a `#` reaches it.
The capture (0x4b6f20, 0x4b7085) clears ram and board, nothing else.
Command `#` (0x4e2ba0 case 0x23; payload players `+1`, classes `+5`,
third `+9`): sub 1 guard on where off, ram where off with bit 25, board
where off with bit 26 (a switch on gets nothing, its masks kept); 2 guard
off; 3 / 4 aggressor on / off when it changes; 5 / 6 board / ram on where
off; 0xe / 0xf board / ram off. The human's buttons (0x4f34c0): guard
sub 1 / 2 and aggressor 3 / 4 with players -1, classes -1, third 0xffff;
board 5 / 0xe and ram 6 / 0xf with all -1. The at-will search 0x4bfc60
(fire states 1 and 4, 0x4c1240) runs 0x4c0cd0 only on guard, for an
enemy (side 2 but the sea monster 0x1fb, or another player at war,
0x4efe20 < 0) whose player bit is in `+0x689`, alive (vt+0x2c), whose
class passes `+0x68d` (0x50ebb0); 0x4e8af0 mode 10 fires only the
positions in `+0x685`. Ported: the masks, records, `#` subs, 0x4e7220 and
the at-will test (`Behaviour::may_pick`); not ported: the search's
geometry by weapon position (0x4bfc60's square, 0x4c0cd0's arcs
0x4bf5f0, 0x4e8af0's draws), so a partial positions mask counts as all;
the fire state is kept, read by the at-will ram and board (states 1, 2 and 4, "The at-will ram and board"; the
classes mask there below).

**The ship's fire state `+0x5d1`** (byte; 0 from 0x4dc3f0, copied whole
by the save and load at 0x4e14f7 / 0x4e2351). Writers: 0x4a9e30 at
0x4aa200 (a behaviour record: 4 unless it is non-zero with `+0x25d &
0x100`); 0x4ab3f0 (a fire record: kind 2 at 0x4ab4ed, 2; kind 1 at
0x4ab5d8-0x4ab60c, 1, with the position's target `+0x63c`, its id
`+0x640`, `+0x645` 1; kind 3 at 0x4ab691, 3, the attack); the task
wind-down 0x4abc60 at 0x4abdeb and the stop at 0x4aa7b2 (`+0x818 &
0x100`), each with any of guard, ram, board on (`+0x684`): 2 and `+0x25d
|= 0x100`; 0x4abe00 (unless 2: 1, and each position in state 3 with a
target back to 1); the leviathan hunt's 0x4ac5d0 callers at 0x4acc61 /
0x4acf3a: 1; 0x4c09c0 at 0x4c0a3e (its target's 0x4b9140 passing, after
event 8 / 0x80): 1; 0x4c1240 (states 1 and 4: 0x4bfc60's search finding
one): 3. Readers: 0x4c1240, the ship's fire pass from the tick (0x4abba0:
the tick `(now ^ +0x4c) & 3` 0, 0x4e6700(0x15) or a type above 1 with
0x4e66c0, `+0x25d & 0x100` or `+0x684`, `+0x251` 0, no status `+0x25c`,
and 0x4bf9b0's mask non-zero, its low 4 bits passed): 1 / 4 the search 0x4bfc60, 2 0x4c0eb0, 3 0x4c09c0;
0x4bf9b0 (which weapons are ready: in 1, 3, 4 a group's only with its
position's target `+0x63c` or guard on, in 2 every group; then ram and
board each 50 ticks); 0x4abe00, 0x4ab3f0 (at 0x4ab4d7), 0x4a9e30 and
0x4d60c0. The weapon positions' records (0x12 bytes from `+0x63c`,
position k the items 3k..3k+2, a fourth for types 8 / 9): `+0x63c` the
target, `+0x640` its id, `+0x644` its player code, `+0x645` the state,
`+0x646` its class. 0x4ab3f0 kind 3 sets each position with a unit of
its items aboard (or the fourth) to state 3 at the target and every
other of positions 0-2 with a target not state 3 at it to 1; kind 1 the
record's position alike with state 1, the others in 3 to 1. `+0x25d &
0x100` (the fire task) is set only at 0x4aa7b8, 0x4ab580 and 0x4abdf2,
cleared only at 0x4aa799 and 0x4abdd2. 0x4abc60's callers: the stop
0x4aa540 (mask 0 at 0x4aa570, other bits left at 0x4aa7f5), 0x4abe50's
deferral (0x4abefb) and the rammer's sail (0x4c2fee); 0x4aa540 with
0x100 in its mask (0x4c09c0's 0x180) runs the same fire part alone.
0x4ac5d0 sets positions 0 and 1 to its object (class 0x192), its second
caller (0x4acf0f) all three first. Ported (`Ship::fire_wind_down`,
`fire_reset`, `take_attack`, `objects::Aim`): 0x4a9e30, 0x4ab3f0 kind
3 (the port's attacks, at a jam its member), the stop (`Scene::stop`,
0x4abc60 whatever it calls off), the sail's and the boarder's prey lost
(0x4abc60), 0x4c09c0's target gone (0x180), 0x4abe00 at the group's turn
(`one_event_a_tick`, once a batch), the hunt's 0x4acc61, and the fire
pass's own writes (0x4c1240's 3, 0x4c09c0's 1 in reach; below). Not
ported: kind 1 and 2 records (the port's ships take no
0x15); 0x4abe50's deferral; the hunt's second site (0x4acf3a, the
port's hunt has one). 0x4d60c0's cases on it are ported (`ha_retarget`).

**The fire pass's reach and ready mask** (`scene::fire_pass`). A ship's
items are its units by kind (`+0x289 + i`: marines 0-2 by squad, weapons
3-8, the unique ships' 9-11); item i's next shot waits for the tick
`+0x5e3 + 4i` (0 from the constructor). 0x4bf5f0(i, dx, dy), dx / dy the
ship's cell less the target's: items 9 and 10 take item 1 at level 2,
item 11 nothing; the others the range level of their position (player
`+0x95 + i / 3`, 2 with hero 1 aboard, 0x4ba790); `s` half the aiming
distance 0x598130[L x 12 + i] (9, 7, 7, 9, 9, 9, 13, 11, 11, 7, 9, 0 at
every level), `a = s - dx`, `b = s - dy`, both at least 0, and the shape's
byte at `b + (a + L x n) x n` not 0: items 0 and 2 0x59819c, 1 0x598290
(7 x 7), 3 0x598324, 4 0x598418, 5 0x59850c (9 x 9), 6 0x598600 (13 x
13), 7 0x5987fc, 8 0x598968 (11 x 11). Only the lower bounds are tested:
with signed distances (0x4c0cd0) a target beyond a shape's side reads on
into the next row, level or table (the port keeps the bytes 0x5980f4 to
0x598ade, as far as a look's box reaches; the boxes 0x5980f4[L x 4 + k]
are 9, 9, 13, 9). The leviathan hunt's items 3 and 4 use the same.
0x4bf9b0 clears the positions' marks (`+0x62b`, 3 or 4); over the items
(9, 11 for types 8 and 9) with a unit aboard: in fire states 1, 3, 4 an
item is ready (`+0x613 + i` 1, its position's mark, bit k = i / 3) when
`now - +0x5e3[i]` is not negative and its position has a target
(`+0x63c`) or guard is on, else not (`+0x613` 0); in 2 by its time
alone; in 0 none is looked at (`+0x613` kept). Then with ram or board on
both marks `+0x710` / `+0x711` are cleared and each switch on with `now -
+0x704` (`+0x708`) above 0x32 marked (bit 0x10 / 0x20) and its time set
to now + 100. The tick's gate (0x4abb39-0x4abba0) calls 0x4c1240 with the
low 4 bits when the mask is not 0, then zeroes `+0x62b..+0x62e`
(0x4abba5). Ported: 0x4bf5f0, 0x4bf9b0 (`Scene::reaches`,
`Scene::ready_mask`; the gate's at-will half, `scene::at_will`).

**The fire pass** (`scene::fire_pass`, `Scene::run_fire_pass`). Ships
fire only from it; `battle.rs` keeps the forts', structures' and
creatures' fire. 0x4c1240(mask) by the fire state `+0x5d1`: 1 and 4 the
search 0x4bfc60, non-zero setting the state 3; 2 0x4c0eb0; 3 0x4c09c0.
- **0x4c0230(positions, from_lists)**, types 2 and up (below, nothing):
  each position in the mask, each of its items ready (`+0x613`) with a
  unit aboard fires once, at the position's target (`+0x63c`, fetched for
  its first such item) or, from the lists, at entry 0 of the item's list
  (0x5c7ed0 + i x 0x191; empty, skipped). The target's cell (vt+0x28) and
  family (0x50e600) give the reach (0x4bf5f0 at the distances across and
  down; a settlement or town, class 0x32 / 0x33, from any of its 2 x 2
  cells) and the roll's distance (0x557440; not taken anew for a 2 x 2
  building of family 0x32, the last one standing). The roll 0x4bf7f0
  (above) gives the accuracy; the blow is the power 0x4bc360 x accuracy
  / 100 (0x4bfc20), x 150 / 100 with the Flame (`+0xa8`) and again with
  Theseus; a miss's (0) power x 10 / 100 / 5. One unit of the item, not a
  100 and a target not of family 0x32: accuracy 0, the shot flies wide.
  Greek fire flies flaming, x 150 / 100 more; quick lime never flaming.
  The item's time `+0x5e3 + 4i` = now + 0x5c76e0[3i + L] + a draw of the
  ship's generator % 30 (L the rate level `+0x8e + k`, 2 with Atalanta,
  hero 6, or for the fourth position). Returns the positions fired.
- **0x4bfc60(mask)**, the search (states 1 and 4): guard on, the lists'
  counts zeroed. The box: the highest marked position k gives
  0x5980f4[L x 4 + k] (L its range level; 2 for the fourth or with
  Theseus), none or 0 gives 4 with ram or board on, else 0; half of it
  each way about the ship's cell, clipped. Each cell's slots in order
  (`+0x14` flyers, `+0xc` occupant, `+0x18` sea creatures, `+0x10` a
  leviathan only with `+0x74 == 1`): not itself; side 2 (`+0x57`) not
  of class 0x1fb, or another player at war (0x4efe20 < 0); its player's
  bit in `+0x689`; there (vt+0x2c); not a ship on its `+0x5ab` cell.
  Each position whose target id (`+0x640 + 0x12k`) it is is met (a jam,
  0x1f7, meets those whose target is among its members, 0x415e00). With
  guard on and not all three met (`!= 7`; 0xf is never compared), a
  side-1 object needs its player bit and any its class in `+0x68d`
  (0x50ebb0), a jam as its first member (0x415df0), then 0x4c0cd0; with
  ram or board and a switch marked, 0x4c0bc0 (the at-will list). Then
  the met positions in the mask fire at their targets (0x4c0230); the
  result is the positions in state 3 among those met and not fired (0
  when all fired); none met: with anything met 0, else every position in
  state 3. With guard on, not 7 met and mask left, the lists with entries
  (positions 0-2, 9-10 as bit 8) are drawn (0x4e8af0 mode 10: each list
  of a position in `+0x685` takes entry `draw % count` to slot 0) and
  fired from (0x4c0230 with lists, the draw's bits & the lists' & the
  mask). Then the at-will close (0x4e8db0, 0x4c0b30).
- **0x4c0cd0(object, k, cell)**: for positions k down to 0 marked, each
  item ready with a unit aboard reaching the cell (0x4bf5f0, signed) gets
  the object on its list (id and `W x y + x`), a leviathan (0x192) only
  on items 0-4 (`i x 0x191 < 0x645`); the count stops at 49, the 50th
  entry written over. The entry is the slot's object: a jam's is the
  jam (its first member only for the class test), so the pass's shot
  from the list aims at the jam's point (0x415ce0); the port's shot
  goes to its first member.
- **0x4c0eb0**, state 2 (and the attack's end): guard on, the lists
  cleared (9, 12 for types 8 and 9); over the box without the leviathans'
  slot, each object passing as above with, side 1, its player bit and
  its class in `+0x68d` goes on the lists with guard on and on the
  at-will list with a switch marked; the lists drawn (guard off, they
  hold what the last look left) and fired from; the at-will close. The
  state stays.
- **0x4c09c0(mask)**, state 3, the attack (mask 0: nothing). 0x4c07c0:
  over the positions in state 3 (the last gives the target's id
  `+0x640`), the aim (the largest 0x598130 of their armed items, halved)
  and `d`: the summed power 0x4bfc20(i, 100), once a unit, of the items
  reaching (1, 0), (2, 0), (3, 0): 1 when 1's sum beats 2's, else 2, then
  3 when 3's beats that. 0x4b9140(d, d, id): the target found
  (0x4ac050), its cell 0x50ebf0 (with 1, vt+0x28's; its tolerance 0 for a
  ship and the sea monsters 0x1fa / 0x1fb, 1 for the others) and its own
  vt+0x28 cell both within `d` (0x557480) of the ship's: event 8 0x80
  (the move stopped), the state 1, done. Else a plan, 0x4a6270 mode 5 to
  that cell with the tolerance made `d` (0x4a6150: the first after the
  attack's records at once, `+0x593`, which 0x4ab3f0 kind 3 sets with
  `+0x57b` 0, `+0x57f` 25 and `+0x591` 0 at 0x4ab6a5; later ones 25
  ticks apart and only when the ship changed cells, the goal changed or
  it lies still, and not turning; then 0x4a6330 -> 0x4a51b0 with `keep`
  0, the route 0x55ba50 within `d`). Not found: event 8 0x180 and notice
  0x13a `{id, 0x16}` (0x4a1800). Found within the aim: 0x4c0230 of the
  positions in state 3. Then with guard on and not 7 fired, 0x4bf9b0 again
  (its marks just cleared) and, non-zero, 0x4c0eb0.
- Greek fire's and quick lime's power (0x4bc360, after the nine items):
  only a type 8 sets Greek fire's `+0x46d`, only a type 9 quick lime's
  `+0x471` (any other type leaves both as they are), each the cache
  0x5c765c / 0x5c7668 (0x5c75cc's rows 12 and 13, 0x4ac140: row 12's A
  650 / 720 / 790, row 13 has no A so its B 200 / 240 / 280, x 1500 /
  100) at the player's damage level `+0x9e`, x the hull `+0x271` / the
  type's full hull: 0x5c76d0 and 0x5c76d4 are 0x5c76b0[8] and [9]
  (0x4ac210: 0x5bb4b0[t] x 650 x 100 / 100, 65 x 650 and 75 x 650; no
  other writer). Then `+0x47d = 3` and the total takes the Greek fire
  power / 100 (on a type 9 the stale `+0x46d`'s).
- The aim point: 0x4c0230 asks every target's vt+0xc with the hit byte
  (after a lone unit's miss is zeroed) and the shot flies there (the
  bomb's `+0x1d / +0x21`). A sea monster's target (0x1fb) is not shot (no
  shot, no reload; the roll already drawn) when the shooter's vt+8 takes
  the point `(x, y, 1, 0, 1)`: the ship's vt+8 is 0x4e06c0 (vtable
  0x59b48c: the point within its picture's rect `+0x8be` `+0x59 / +0x5d /
  +0x61 / +0x65` and solid in its mask `+0x8e2` frame `+0x45 - 0x20`
  while turning (0x20-0x37), else heading `+0x5af` x 3, 0x543240 /
  0x557640, as 0x4c1870). The monster's vt+0xc is 0x493350 (vtable
  0x59750c): its rect is its picture's (`+0x139` `+0x59 / +0x5d / +0x61 /
  +0x65`) or, with none, its cell `+0x91 / +0x95` x the map's cell size
  (`+0x17b2` shorts `+4 / +6`); each coordinate `lo + r % (hi - lo + 1)`
  (unsigned) of its own generator `+0x18`. By state `+0x74`: 0, 6 and
  0xff `w / 4 ..= 3w / 4` by `h / 4 ..= 3h / 4`; 1-4 by heading `+0xb5`, a
  hit (0x492fa0) 0 / 4 `w / 4 ..= 3w / 4` by `h / 3 ..= 2h / 3`, 2 / 6 `w
  / 3 ..= w` by `h / 4 ..= 3h / 4`, the odd ones `w / 3 ..= w` by `h / 3
  ..= 2h / 3`; a miss (0x493110) draws one more first, bit 0 picking the
  second box: 0 / 4 `0 ..= w - 1` by `0 ..= h / 3` or `2h / 3 ..= h - 1`;
  2 / 6 `0 ..= w / 3` or `2w / 3 ..= w - 1` by `0 ..= h - 1`; the odd ones
  (1 / 5 fall through into 3 / 7's code, 0x493222 -> 0x493228) `2w / 3
  ..= w - 1` by `0 ..= h / 3` or `0 ..= w / 3` by `2h / 3 ..= h - 1`; any
  other state the rect's top left. Ported for the sea monster
  (`Scene::sea_monster_aim`, test `sea_monster_aim_is_the_originals`,
  `sea_monster_aim_on_the_shooter_holds_fire`). The other vt+0xc slots
  (dumped from the vtables the constructors store): a ship's 0x4e07c0
  (vtable 0x59b48c), every building's 0x423520 (the 18 vtables of the
  towns, works, forts, docks and yards, temples, castles 0x5949fc,
  lairs 0x5953f0-0x595600, barbarian structures 0x594920 and the hero's
  house 0x595e98), a harpy
  nest's 0x41cef0 (0x595d50), a harpy's or dragon's 0x41df70
  (0x595e00), a jam's 0x415ce0 (0x5956b0), a land creature's 0x411bb0
  (0x5953a8), a leviathan's 0x495b40 (0x597544). The pass's shot flies
  to a ship's (`Scene::ship_aim`), a building's, barbarian structure's,
  castle's, lair's, hero's house's or nest's (`Scene::building_aim`)
  point, each drawn from the target's own
  generator after the hit roll and before the shooter's reload draw
  (test `item_shot_flies_to_the_targets_aim_point`). A land creature's
  0x411bb0: from its place `+0xb4 / +0xb8` less its picture's offset
  `+0xbc / +0xc0` (0x4111b0), a hit its picture's (`+0xb0`) `+0x61 / 2`,
  `+0x65 / 2`, no draw; a miss `(r1 & 1) +0x61`, `(r2 & 1) +0x65`, two
  draws of its own generator; clamped to the map. A jam's 0x415ce0 (`+0x74`
  members, cell `+0x398 / +0x39c`): none, nothing written; a hit with
  two or more its cell's `64 x + 32`, with one its member's (`+0x7c`)
  vt+0xc; a miss its cell's `64 x`, no draw. A leviathan's 0x495b40, from
  its cell `+0xa9 / +0xad` x the map's cell size (`w`, `h`), its own
  generator: states 0, 5, 6, 0xff `w / 4 + r % (3w / 4 - w / 4 + 1)` and
  the same down; 1 a hit 0x5bace0[heading `+0x98`] (no draw), a miss a
  draw's bit `b` then 0x5bad20[heading x 2 + b] `{x, y, mx, my}` (dumped:
  (0,0,64,40) (0,0,64,40) (39,45,25,19) (0,0,25,19) (0,36,63,28)
  (0,36,63,28) (41,0,22,20) (0,44,22,20) (0,0,63,36) (0,0,63,36)
  (0,0,19,23) (45,41,19,23) (0,0,19,63) (0,45,19,63) (0,39,25,25)
  (42,0,22,17)), `x + r % mx`, `y + r % my`; 2 and 4 a hit (25, 45), a
  miss `r % 61`, `r % 28`; 3 a hit `16 + r % 17`, `41 + r % 10`, a miss as
  2's; any other (0, 0), not from the cell. Ported (`Scene::walker_aim`,
  `Scene::jam_aim`, `Scene::leviathan_point`; tests
  `land_creature_and_jam_aim_points`, `leviathan_point_is_the_originals`).
  The harpy's and dragon's 0x41df70 (picture `+0x74`; a hit: the dragon,
  class 0x1f9, `+0x59 + 0x43`, `+0x5d + 0x3c`; a harpy by state `+0x88`:
  0 and 1 0x595ae0 `{x, y}`, 2 0x595ae0[frame `+0x45`], 6
  0x595ae8[frame], else (0x28, 0x28), from the picture's top left; a miss
  `(r1 & 1) +0x61`, `(r2 & 1) +0x65`, its own generator; clamped) is
  ported (`Scene::flyer_aim`), as is the gorgon's 0x402da0
  (vtable 0x594340, constructor 0x401033; no picture `+0x11c`: its cell
  `+0x114 / +0x118` x 64 + 32; a hit `20 + r % 21`, `20 + (r & 15)`, a miss
  `10 + r % 51`, `60 + r % 11`, from the picture's top left, its own
  generator; `Scene::gorgon_aim`), and the siren's 0x4048f0 (vtable
  0x594378, constructor 0x40306e; no picture `+0x164`: its cell `+0xbc /
  +0xc0` x 64 + 32; a hit `30 + (r & 7)` each way, a miss `r % 61`, `40 +
  r % 24`; `Scene::siren_aim`). A roll that misses
  a building leaves its chance as the hit byte (0x4bf7f0 returns it), so
  only a chance of 0 takes the corner. A barbarian structure is aimed at
  by 0x423520 (vtable 0x594920 +0xc) from its building generator
  (0x40ef70 runs 0x427f50's seed through 0x4e8660 -> 0x4e7640). Not
  decoded: the hero's house's vtable (type 0x1a6d); not ported: 0x4c07c0 with no position in
  state 3 (its id and player uninitialised; the port does nothing); a
  position's target gone, its pointer dangling (the port skips it);
  0x50ebf0's families with no cell (400 a leviathan, 700, the jam 0x1f7
  and the land creatures not listed): 0x4b9140 returns 0 unrouted and
  0x4c09c0 compares an uninitialised distance with the aim (the port
  fires nothing there).
- `+0x594`: set by 0x4c09c0 with a mask (and by 0x4b7f10 at a fort or
  barbarian prey, 0xe01c, not ported), cleared by a step granted
  (0x4a3c6e); a step refused at the route's last cell counts the extra
  tick of wait with it set (0x4a3d76). `+0x62f` (0x4c09c0: the ship's
  cell; 0x4ab6f7: -1) has no reader. 0x4ac050(code, id), the target found
  again: code -1 anything of that id (0x51f1f0), else only that player's
  objects (0x4a8710); the code is the record's `+0x644`, which 0x4ab3f0
  takes as nation x 4 + slot, -1 for side 2. No enemy test there.
- The crew's experience `+0x544` (0x4b4680: `+0x544 += n`, at most
  10000; with `+0x252` set, each hundred since `+0x548` posts the panel
  notice 0x4bebe0 and `+0x548` follows). A map ship starts with its
  record's byte `+0x34` x 100 (0x4dfa90, `+0x548` the same; 0 in every
  campaign map), a ship built 0 (while built or sold, `+0x251 == 2`, the
  field holds its price instead). Raised by: the hit roll 0x4bf7f0 (not
  for item 9) 50 a hit and 10 a miss, 170 and 20 at a ship (family 200;
  the hunt's 0x4af6a0 passes 0x192); a ram 330 (0x4c1c60, the rammer,
  after both blows); a boarding won 450 (0x4b5b00, the boarders); a
  building taken 470 (0x4b59f0); the founding party landed 200
  (0x4b3370 with task `+0x72f == 2`, after 0x4b2d60); an unloading
  (0x4ad9f0, once as its count `+0x45` reaches `+0x49`): fish 10 (at a
  building of family 0x32 not of class 0x34-0x36), a leviathan 400,
  amber 100. It changes no shot: 0x4bf7f0's fifth argument (the low
  byte of `+0x544`) is never read. It counts in the boarding: the stats
  refresh 0x4bee60 makes `+0x53c = (+0x544 / 100) x S / 100 + S` (S
  0x4b4e80's); 0x4b5b00 takes each side's `+0x53c` (halved with `+600`)
  + 1 and adds `+0x544` x that / 10000, then draws the boarders'
  generator % (a + d) before the Hercules / type 11 test; the boarders'
  blow `+0x6a4` is the defenders' strength + 1 without that term (+ hull
  / 100), the defenders' `+0x721` the boarders' with it; 0x4b59f0 the
  same against a building (`*out = a x 100 / 1500`). The panel shows
  `+0x544` / 100 (0x4e51c0, 0x4e3850, 0x4e5ab0). The port keeps `+0x53c`
  live rather than from the last refresh; 0x4b59f0 (marines taking a
  building) is not ported.
Who sends `#` (0x4c42f0, `{sub, +1, +5, +9, 0.., +0x15 0xff}`, queued:
it reaches the ship at the next tick): 0x4c4350 (fleet order 0x72, from
the brains' 0x4797e0 and the computers' 0x482eb0; the interface's attack
zone 0x49da43) keeps group `+0x14 = 1`, `+0x15`, `+0x19`, `+0x1d`,
`+0x21` and sends sub 1 with the three to each ship of the group; the
move order's join (0x4cc6a0: a join sent to the order's cell by
0x4cc650 is then sent the group's three, zeroes when never given) and
its pass (0x4ccd80, every 100 ticks: each ship whose `+0x689 / +0x68d`
are not `+0x19 / +0x1d`, with `+0x14` set); a new navy group of a
computer side 3 / 4 (0x4a1150 -> 0x4c5b30 -> 0x4c4230, the group's
three, to every ship of it: 0x4a1150 makes the notices list `+0x2d` when
none, which every task's end frees, 0x4cc4c0, 0x4d0c90, 0x4d4030,
0x4d6520, 0x4dab80, 0x4d95a0, 0x4cef60, 0x4cd800, 0x4caf90, 0x4c7650
with 1; each order's maker, 0x4ccc30, 0x4d1ce0, 0x4d6a90, 0x4c8a80,
0x4cbcf0, 0x4da550, 0x4db9e0, ends a block of another kind and makes a
new one, rewriting one of its own kind); the gather zones (0x4c6100 a
new escort with -1s when `+0x14` is 0, 0x4c618a; notice 300 the group's,
zeroes when never given, after the held command or next deposit). The brains' 0x4797e0 call sites
(`(group, member, p3, classes, players, third)`, stored at member
`+0x28`, `+0x2c`, `+0x30`): 137 with classes 0xfffffff, players -1,
third -1; 3 pushing `ecx`, which holds -1 there (0x4652fd: `or ecx, -1`
at 0x46523c; 0x47bd7e, 0x47beeb in the repair members 0x47bcd0 /
0x47be40: 0x47bd11, 0x47be7c), so 0x47bd7e and 0x47beeb are the default
and 0x4652fd classes 0x3ffff; 20 with classes 0x3ffff, players -1, all
in the barbarians' members of miss008-miss011 (0x4615e1 0x461480,
0x4618b0 0x461720, 0x461b0f 0x461a40, 0x464c32 0x464b20, 0x464ed6
0x464da0, 0x46510b 0x465010, 0x4652fd 0x465200, 0x468344 0x468240,
0x46860e 0x4684e0, 0x46897b 0x4687a0, 0x468b9f 0x468ad0, 0x46bc00
0x46bb30, 0x46bd33 0x46bce0, 0x46bf30 0x46be60, 0x46c3e0 0x46c310,
0x46c590 0x46c4c0, 0x46c8d6, 0x46ca96, 0x46cc56, 0x46ce16 the sentries
0x46c880-0x46cdc0); 6 with players 0x10020, classes 0xfffffff: miss004's
0x451938 0x451890, 0x451b06 0x451a60, 0x451d33 0x451c60, 0x452973
0x4528a0, and its start's 0x45431d / 0x45442b for the members (0, 7) and
(2, 5), which never run and hold no ships (their cells, (8, 2), (10, 8),
(7, 13) and (7, 22), (7, 27), have none at the start; nothing joins
them), so the port leaves them out. Every site is in its member's state
0 (with the call before the state's other work), but 0x46510b (miss009's
south-eastern guard, before its switch: a zone of 0x5b24b0 with the
player in it and the state not 10, then state 10) and the case 0x32 of
miss006's raiders (0x456580, 0x456e80). Ported: 0x4c4350 for the brains'
`Op::Behave` (default masks) and `Op::BehaveWith` (a site's own), every
site above by its masks, and the move order's join and pass. (Where a
member only sets its state, 0x479750, the port's `Op::Goto`; miss004's
and miss005's members' "not strong enough" branches and miss006's
raiders' had been written as behaviour sends.) Ported since: the computers' 0x482eb0 and
0x4c5b30 (for the computers' flotillas, and for a brain fleet's new
block, `brain_block`: the campaign's mission opener 0x537f60 gives every
player but the human's side 3, 0x5381f2 (the human's kind 2 side 1), so
0x4a1150 sends for the brains' groups too; the kinds a brain's fleet
gives, 100 move 0x4ccc30, 0x78 gather 0x4c8a80, 0x70 attack 0x4d6a90,
0x82 repair 0x4cdf80, 0xbe treasure 0x4da550, 0x8c found 0x4cbcf0, each
rewriting a block of its kind (and of its second word, 100/1, 0x78/0x3d,
0x70/0x21, 0xbe/0x79, which only the makers write in the routines read)
and ending one of another kind; a status report never ends one,
0x4ccf60), the gather zones' sends (gather.rs: a brain's fleet's
`+0x14` its behaviour given, a computer's always, a human's never), and
for the computers the case 300 of the guard, attack, founding, dock and
treasure blocks and the move's join and pass (above). A fort of a computer's forts flotilla (role
1) takes the `#` too: 0x4c42f0 and 0x4c4230 send to every member of the
group's `+0x31` list with no class test, and the fort's case 0x23
(0x419f80, sub 1) stores the players without its own bit (`+0x48c`),
the classes (`+0x490`) and the kinds (`+0x494`), sets every unit group
at will (0x4469a0) and `+0x498 = 0` (ported: `Scene::fort_command`).
The computers' attack zone's (0x4d6960's join, 0x4d6cd0's pass) and
the human's attack zone's (0x49da43) are ported (above).

The other calls of 0x4c42f0. The fleets' tick 0x4c4f40 runs a block by
its kind and second word: 100/1 0x4ccd80 (move), 100/2 0x4cf550 (its
notices 0x4cf0b0; the waypoint move, maker 0x4cf2c0), 0x6e/0x29 0x4d3b30
(guard, notices 0x4d1ea0), 0x70/0x20 0x4d61d0 (the human's attack,
0x4d4e50), 0x70/0x21 0x4d6cd0 (the computers' attack zone), 0x74/0x15
0x4d0650 (the fleet attack task, 0x4d0210), 0x78/0x3d 0x4ca8d0 (gather),
0x82/0x51 0x4ce140 (repair, 0x4cdc00), 0x8c/0x47 0x4cbe70 (founding,
0x4cba10), 0xa0/0xb 0x4d91a0 (0x4d8b90), 0xaa 0x4d7b50 (0x4d7770),
0xb4/0x65 0x4d8330 (0x4d8030), 0xbe 0x4da6a0 (treasure, 0x4da0a0), 200
0x4dbd90 (dock, 0x4db110), 0xd2 0x4dc3a0 (0x4dc2a0); then a block with
`+0x5d` set is ended (0x4a1c60). Every one of the calls below but the
first is its block's notice 300 with the ship in the group (0x4cc590),
after the case's own work: `#` sub 1 with the group's `+0x19`, `+0x1d`,
`+0x21`. 0x4cbcb7 founding (after 0x4cb1a0), 0x4cde70 repair (after the
dock send or none), 0x4cf296 the waypoint move, 0x4d02bb the fleet attack
task (after 0x4cfad0 / 0x4cfe40), 0x4d1f8a guard (after 0x4d1a50 or
0x4d1080 by block `+0x24`), 0x4d4f84 the human's attack, 0x4d7966 0xaa,
0x4d8185 0xb4, 0x4d8c94 0xa0, 0x4da1e2 treasure, 0x4db1ec dock, 0x4dc2fa
0xd2 (whose 300 does nothing else; its other notices are dropped).
0x4c449b is 0x4c4410, which order 0x72 with first field 100 runs before
0x4c4350 (0x4a09d0 case 0x72): group `+0x14`-`+0x24` zeroed and every
member sent sub 2 `{0, 0, 0}`. It never runs: the only senders of 0x72
(0x4a09d0's two `push 0x72` sites) are the brains' 0x4797e0, whose first
field is its p3, 0 at all 165 sites, and the computers' 0x482eb0, `p2 ?
0 : 100`, p2 1 at all 25 sites. Order 0xd2 is sent only by the forts'
flotilla (role 1, 0x482950 letter 0): on its first turn (`+0x1c` 0, set
to 1) 0x482f10 sends 0xd2 `{0}` (0x4dc320: a block of another kind
ended; none, 0x4dc240 makes `{0xd2}`, 0x65 bytes zeroed, and 0x4a1150
sends every fort the group's three; one of its kind zeroed, kind kept);
later turns only keep its time (`+0x18` = now once `+0x14 + +0x18` is
reached). A new fort joins it by 0x484000 -> 0x483fb0 (0x488bd8, the
building's news; 0x487326 at the setup), a join posting 300. Orders 0xa0,
0xaa, 0xb4 and 100/2 have no sender through 0x4a09d0 (no `push` of their
codes before it; not read: other paths). Ported: the repair's
(`run_repair_orders`, brains), the forts' block 0xd2 and its 300
(`flotilla_turn`, `read_joins`; a new fort through `Computer::join`), and,
before, the computers' guard, founding, treasure and dock, the human's
attack. The brains' founding (0x8c, miss005's raiders (1, 6) and
(1, 7)) and treasure (0xbe, miss007's (0, 11)) blocks hear no notices
in the port (no pass); a 300 reaches them in the original only by a
join, 0x4a1340 posting it while the group has a block (its queue
`+0x2d`, made by 0x4a1150, freed by 0x4a1c60): a brain's joins are
0x479300 op 2 (0x4a0480: 0x49bd60, then 0x4a1340), for prizes to the
repair members and mended ships to the members by type (above), none
of them these. A ship built for a member joins the same way: the
yard's 0x4dfe00, as the ship takes its berth (0x499a40), sends it at
once (0x50e700 through 0x542630, a direct send, only for a member >= 0)
`{0x203f, 3, 0x83, ship +0x14, member +0x18, player +0x1c}` to the
navy, whose case 0x83 (0x49ab30 at 0x49add9; no ship, nothing) runs
0x49bd60(ship, member) and, when that returns >= 0, 0x4a1340(player,
member, ship): the 300, tick now, only while the member has a block
(`ship_launched` -> `brain_heard_join`). (0x49c830, an area's ships of
the player taken into a group, and 0x49d0d0, one group's ships passed to
another, post it the same way; neither is a build.) A built ship
reaching a founding or treasure member under its block is heard, but the
block's work for it is not ported (the 300 waits in `joined` until a
new block). Their 300 cases do
the block's work first, then `#` sub 1: founding 0x4cba10 (0x4cb1a0:
no route `+0x61`, a site by 0x4cb850 and 0x4cb0b0; with one, 0x4cea20
/ 0x4ceab0 and 0x4cb110 / 0x4cb5f0), treasure 0x4da0a0 (0x4d97c0 a
target not taken: 0x4d9690 and 0x4d8b30, counted; none, with `+0x28`
listing, 0x4d9920 over `+0xc` and 0x4da3e0 on `+0x2c`'s first);
founding's other cases 0x130 and 0x136, treasure's 0x137, 0x141,
0x142 (`+0x34` 4). The computers' read of these 300s (`read_joins`)
sends the `#` alone: the block's work is not ported (the port's
founding and treasure orders keep no sites, routes or lists). Ported:
a brain's mended ship's join posts 300 as a prize's
(`brain_heard_join`), only with a block, and a new block of another
kind drops the joins not heard (`brain_block`). Not ported: the fleet attack task's (the port keeps it per ship, no group: see
"A ship's order at a jam"); the waypoint move, 0xa0, 0xaa, 0xb4 (not in
the port).

**Treasure hunting** (builders, task 5 state 2): the area is the square
about the base's town, from 4 cells and 4 more a time, that holds a
treasure (0x4854f0, counting with 0x48f020); the whole map if none. Order
0xbe (0x4da550; fleet state 0x79) holds the area, a mask of treasure kinds
(`+0xb0` bits), and where to bring them: the town's ports (class 0x39,
0x4d9a70). Each turn (0x4da3e0) every ship of the member (class 200) is
sent after the same target, the first of the block's list `+0x2c`
(0x4d9920 -> 0x4d9810: the area's treasures row by row, of the order's
kinds `+0x1c` by `+0xb0`, `+0xa4` not 5; entries {object, id, kind, cell,
-1}), and recorded in `+0x28` (0x4d9690): a ship that can take a
treasure (0x4e6700 command 0x22 kind 0x10: capability `+0x261 & 0x8000`,
every finished ship's by 0x4e6930, off while built, sold or holding its
commands; and room, 0x4be800 mode 6 or 7) with room aboard (0x4be800
mode 6: 0x597afc[type] = 1,1,2,1,1,1,1,1,0,0,0,0 less the treasures
aboard `+0x482`; 0 for types 10 and 11) gets the take, 0x4d9f50: command
0x22 {kind 0x10, cell x, cell y, the treasure's id}, priority 0x5a,
which the ship's 0x4e2ba0 case 0x22 turns into event 0x40 {700, 10, the
treasure, priority} as the player's take (0x49e0e0 gives the player's
priority 0); any other ship a plain move to the treasure's cell
(0x4cf070, command 0x14, priority 0). No ship sails home with what it
carries: a computer's treasures go ashore only by 0x4ad9f0 and the
panel. With no treasure left in the area (0x4d9920) the order reports
done and the round goes on to the zone. Ours (`hunt_treasure`): the
room test and the take or the move as the original's; not ported: the
target (ours the nearest no other ship of the flotilla is after, the
take given at once, a refused one trying the next) and the block's
lists and notices (0x137, 0x141, 0x142).

**A ship's take** (event 0x40 class 700, the player's and the computer's
alike). 0x4aab00 sets the gather task `+0x76e` 700, sub-code `+0x772` 9,
step `+0x77d` 1, the treasure's id `+0x77e`, and asks 0x4af940 for the
cells: a treasure on a cell 0x499990(cell, 0) takes (its cell `+0xa8`,
`+0xac`) gives `+0x792` = `+0x797` = that cell; on land, a route
(0x55a890 over the ship's grid, the treasure's cell opened) gives
`+0x792` the route's last sea cell and `+0x797` the treasure's; no route,
both its vt+0x28 cell. The box `+0x7c2`-`+0x7ce` is the treasure's cell.
Each tick 0x4ac2d0 (class 700 / 0x49) runs sub-code 9, 0x4ad680: the
treasure looked up by id; gone, notice 0x138 to the ship's group
(0x4a1430) every tick and nothing else, the order staying; else a move
(0x4a6270 mode 5) to `+0x792` (with 0x4af940 again when it is land, or
when a ship of the player's lies there within 2 cells, asked to make way
by 0x4b4b50 when its priority is lower), returning 2 once in that cell.
Then, a sea treasure: the ship in `+0x792` ends the order (0x4ac490:
0x4b8d40 zeroes the task, `+0x25d &= ~0x40`), taken or not; a land one,
lying still (`+0x58c` 0) with no building on the cell, goes on to
sub-code 10 (0x4b0e40: close in 0x4afe50, 0x4b0470, furl 0x4b89b0, the
walkway 0x3e6 and the party 0x3de as the landing's, at half the party's
frames 0x4b2d60 sends 0x1a67 a landing party of kind 3 for `+0x7c2`,
whose 0x421f00 on the treasure's cell, 0x407950 passing (`+0xa4` 1 or
2), sends 0xda9d to the ship (0x4de5ea: the take as below) and 0x158c1
to the treasure when it answers yes; then the walkway up and back out,
0x4b3850).

The take itself is the ship's step (0x4a3f40 at 0x4a48d2, after the
chest, on every cell it comes into whatever its order): 0x4a5bc0 takes
a treasure on the cell (class 700, `+0xa4` not 5) by its use 0x4b9740
(`+0xb0` kind): 2 for kinds 0-5 and 8 (used at once, 0x4b97d0; notice
0x4a1af0), else room aboard needed (0x4be800 mode 6: 0x597afc[type]
less `+0x482`, at least 1) and a use not 0 (4: 6, 7, 27; 8: 9, 17-26;
0x10: 10-14; 1: 15, 16; aboard by 0x4bd0c0, notice 0x4a1a60); taken, the
treasure gets 0x158c1. For the viewer's ship it plays the nation's event
(`+0x54`: 0 0xa8, 1 0xa9, 2 0xab, 3 0xaa; mode 5). The port
(`Scene::ship_steps_on_treasure`, `takes_treasure`, `run_treasures`):
the step's take and the sea treasure's route and end as the original's
(the route the port's plain plan to the cell; 0x4ad680's re-plans and
make-way not ported). The land treasure's landing party is ported (the
next paragraph), and the hero's house (below); not ported: the
treasure's state 5.

No take shows a text. 0x4a1af0 and 0x4a1a60 post records 0x142 and
0x141 to the ship's group (`+0x2d`, read by the computers' treasure
block); 0x4bd0c0's 0x4bebe0 (UI message 0x22c, 0x10a2 / 0x10ad) and
0x4bc880 (0x22d, 0x10a3), like 0x4bd350's 0x4bee60 / 0x4bebe0, refresh
the ship's panel; 0x4b6e80's 0x5d94 and 0x4b6df0's 0x5d99 go to the
owner's player's brain (`+0x34`). The port shows none (its own messages
gone). Used at once, 0x4b97d0 by the record's kind: 0-2 0x4a8de0(field
8) the amber, 3-5 0x4a8c30(field 8) the fish, 8 0x4b9bf0(field 7, the
cell), any other 0. 0x4b9bf0 makes the ship's magic `+0x7d2` if none
(0x42ed20, `+0x48` / `+0x4c` the ship's `+0x70` / `+0x68`), sets its cell and player
(nation `+0x54`, slot `+0x55`), counts one of the spell (`+0x10 + 4 x
spell`), so 0x42ed40 passes, and casts 0x42f580(spell, 0 or the ship's
id `+0x4c` for 9-11, the cell, draw): 1 reef (draw: the ship's `+0x18`
LCG `>> 16`), 2 whirlpool, 3 waterspout, 4 monster (the player drawn
first by the ship: `r % 20`, nation `r >> 2`, slot `r & 3`; then a
second draw for the type), 9 phantom, 10 / 11 the shields; a count the
cast refuses is taken back; the magic freed when 0x4b9fd0 finds no
spell left. Ported (`take_treasure`). Kind 9 goes aboard (use 8):
0x4bd0c0 -> 0x4ba070 counts spell field 7 (kind 0x11: spell 0xd) in the
ship's magic (below).

**A ship's own magic** (`+0x7d2`, a Magic object 0x42ed20 of 0x50 bytes;
`Ship::magic`, kept in the save). 0x4bd0c0 flags a slot of 0x4b9740's
class 8 (kinds 9, 0x11-0x1a; `+0x485` 1; its flags `+0x491` 8, but 4 on
player 16's ship) and gives it to 0x4ba070: kind 9 counts its parameter
(record `+0xc`), the Gorgon's head (0x11, its slot's parameter `+0x495`
set 0xd) spell 0xd, at `+0x10 + 4 s`; the object made if none (`+0x48` /
`+0x4c` the ship's `+0x70` / `+0x68`), its cell (`+0`, `+4`) the ship's
`+0x5a3` / `+0x5a7` and its player (`+8`, `+0xc`) the ship's `+0x54` /
`+0x55` at each count. The move tick sets its cell at each cell the ship
steps into (0x4a3f40 at 0x4a48d7). 0x4bd500 taking a flagged slot out
frees it when all 14 counts are 0 (the counts are not lowered); 0x4df870
frees it too (0x4df9c2; what calls 0x4df870 not read). The warship's menu 0x4e6980 offers slot 2
for slot 0 of class 8 and slot 3 for slot 1 when there are exactly two and
slot 1 is not slot 0 again (0x4ba010: kind, flags, parameter, amount), by
the parameter: 1 0x62, 2 0x61, 3 0x60, 4 0x63, 5 0x55, 6 0x69, 7 0x6a, 9
0x66, 10 0x65, 11 0x68, 0xd 0x6b (`CBUT_HGORG`, 0x4f08a0; caption 0x202,
help line 0xf9 `Head of Gorgon`),
enabled; none for 0, 8, 12. The button sets the pointer state 0x22 with
its kind (0x4f34c0; 0x6b kind 0x13). The pointer (0x4dd2fc's state 0x22,
kinds 1-3, 6-8, 0xd-0xf, 0x11, 0x13) is refused (0x58) without the
magic or a count of the kind's spell (0x42fc80), and refused when
0x42ef90 fails (the fort's tests, from the ship's cell; case 0x13 a
target and 0x42ed40(0xd) only, no reach); for 0x13 also on a nature
object (`+0x57` 2) or one of the ship's own player's. Order 0x22
(0x4e2ba0; 0x4e6700 passes every kind but 0x10): with the magic a draw
of the ship's `+0x18`, 0x42fa90 -> 0x42f580 (the magic's player; case 0xd
with a target: the count lowered if not 0, message 0x158bf {88000, 150}
to the target, the gorgon's stare on it), then 0x4ba1d0 the kind's
spell, 0x4ba130 the first slot flagged 8 holding it (kind 9 of that
parameter, or 0x11 for 0xd), and 0x4b6e80 (notice 0x5d94 to the brain,
not shown) and 0x4bd500 take it out, cast or not. Ported:
`Ship::add_treasure` / `remove_treasure`, `commands::ship_menu`,
`Scene::ship_cast_check` / `ship_cast` (`magic_may_cast`,
`magic_cast_check`, `magic_cast` shared with the forts), `stare_at`;
the port's own free casts for the player are gone. The capture (0x4b6f20,
0x4b9700 the ship's player) leaves the magic alone: its player stays the
old one until the next count (the only `+0x7d2` users are 0x4a3f40,
0x4b9bf0, 0x4b9fd0, 0x4ba070, 0x4bd500, 0x4dc3d0 setting it 0, 0x4dd2fc,
0x4df870 and 0x4e2ba0). 0x4df870, called only by the ship's handler
0x4dc630 on message 3 (after posting 0xd21), is the ship's teardown: the
magic goes with the ship (the port's `Ship` with its object). 0x42f580
refuses only when 0x42ed40 does (and case 0xd without a target); past it
0x42f470 lowers the count (or with none takes the price, 0x596844 /
0x59687c less the discount) before the spell is made, whatever the
spell then does; the port lowers it so.

**The land take.** 0x4af940 (`+0x792` the sea cell, `+0x797` the land
cell): the hero's house (class 0x49) its berth vt+0xb8 = 0x40e050 and its
own cell (`+0x74`, `+0x78`), always true; a treasure on a cell
0x499990(cell, 0) takes both its cell. A treasure on land: the ship's
grid copied (0x5c93bc, 0x5c9398 when `+0x592`), every cell 0x499990
refuses that holds an object (`+0xc`) less one (land -1 to -2; an open
cell refused for layer-1 type 4 or 6 0 to -1), 0x4a4e20(ship, m,
`+0x591`, -2, list NULL) (the draw on the ship's generator only when the
wait is over 50, 0x4aab00 having set it 0; hero 0 aboard opens the 7 x 7
cells whose `+0x1c` object is class 600 or 0x25a; within 3 cells another
group's (or any, impatient) non-ship occupant -2 not remembered, a ship
by the plain rule: impatient state not 4 / 5, patient 0 or 6, closed and
remembered; same group patient nothing; then 0x4a7290 the remembered
cells whose ship still lies there), the treasure's cell -1, and 0x55a890(m,
W, H, ship cell, treasure cell, &n, 0x422a60, map): both ends on the
grid, the start 0 and the goal above -2, else NULL; a copy with 0 and -1
swapped (the land opened, -2 kept shut); the sea flooded from the start
(0x557910, 2(W+H) generations, d12 -1); the goal reached gives it as
both cells; else the copy flooded from the goal, then rings k = 0, 1, ..
about the goal (each side only when not clipped: east column, north row,
west column, south row) until one holds no land cell reached; each
reached land cell's straight neighbours (east, north, west, south)
reached by sea go to 0x55a730's list {sea, land, key = sea v + 4 land v}
sorted by key, on a tie by the lesser land value, then in order, the
same pair not twice (corners); the first pair 0x422a60(map, land, goal)
accepts gives `+0x792` / `+0x797` (none: -4, false); the route traced
back is freed unused. 0x422a60: both cells in the box of the goal's land
region (0x42e590), the party's quarter grid (0x42e720 with 4) with both
cells' 16 quarters opened, and 0x55ca10 from quarter (2, 2) of the land
cell to that of the goal, heading -1: a route, 1. 0x4b3770 (shore
group): the layer-0 short's high byte 7, 8 or 9, then the first row of
0x5bb870 (13 bytes, 5 read) holding the low byte, so variant 0 is group
0 (the rows' zero padding); else -1.
Sub-code 9 (0x4ac2d0): 0x4ad680 sails to `+0x792`; arrived (2) and lying
still (`+0x58c` 0), the house with its own `+0xc` slot or a treasure's
land cell with none goes to sub-code 10 step 1, else 0x4af940 picks again.
Sub-code 10 (0x4b0e40, step `+0x77d`): **1** the target by id (gone:
0x4b8d40, the order ends); 0x4afe50(0): the direction 0x59791d of
`+0x797` (`+0x7a3`, `+0x79c`), the shore group (`+0x796`; none: false,
nothing kept), a slant (or > 6) `+0x79d` 0 and the hull picture centred
on its cell, false; straight, d = the founding's 0x599c44 / 0x59a1c8 /
0x599ee8 / 0x59a4c8[type x 12 + group], the hull picture's top-left to
`d x u - (w-64)/2 + s x 64` across (y `s x 64 - (h-64)/2`, headings 2 and
6) or likewise up and down (headings 0 and 4), the line 0x4b5220 and the
heading fewer turns away (0x4b5390, the second on a tie). False: asked
again the next tick. True: step 10, the commands held (0x4b37d0 0x2000),
`+0x25d |= 0x40`. **10** 0x4b0470 -> 0x4b02a0: two points a tick (called
off two back) and a hull rotation; at the end and turned step 11; called
off and back at 0: the line freed, the hull on `+0x5af x 4`, `+0x25b` 0,
0x4b3850, message 8 (the order ends). **11** furl (0x4b89b0); furled, in
the same tick `+0x25b` 5 and the walkway (slot 1, 0x3e6) placed as the
founding's, step 12. **12** a walkway frame a tick; called off, down a
frame a tick, at 0 step 5; at its last, the party picture (slot 3, 0x3de)
at the walkway's top-left, step 6 (the house: 0x4ba6b0 first). **6**
called off step 14; else a party frame a tick (`+0xe2`), at half its
frames (`+0x49 / 2`) the party 0x4b2d60 at its top-left + 0x59a188[dir]
(the house: 0x4bd350 ...), step 14. **14** the walkway down a frame a
tick, below 0 step 5 from the line's end. **5** a hull step two points
back, unfurling; at 0 the hull on its cell's middle, `+0x25b` 0, 0x4b3850
(commands back), `+0x25d &= ~0x40`; the order stays in step 5 with
nothing more to do (each hull step re-centres and gives the commands
back again). The party (0x1a67 record kind 3: site `+0x7c2` / `+0x7c6`
the treasure's cell, `+0x7d` the ship's id) walks as the founding's; on
the site 0x421f00 kind 3: the slot's class-700 object with 0x407950
(`+0xa4` 1 or 2) sends 0xda9d to the ship (0x4de5ea): 0x4b9740 class 2
used at once (0x4b97d0), else 0x4bd0c0 with its own room test (0x4be800
mode 6 at least 1); taken, yes (0x158c1 to the treasure), 0x4b6e80(entry,
1, 0) (message 0x5d94), the notice, and the viewer's nation cheer (as
0x4a5bc0). The port (`Scene::land_take`, `walk_path::sea_land_pick`,
`party_can_walk`, `take_close_in`, `take_landing_tick`, `party_takes`,
`trade::shore_group`): all the above for a treasure on land; the 0x4a4e20
marks with the port's ship states as `replan` (the group `+0x5e` not
kept: every ship another group's) and its draw on the scene's generator;
the route to `+0x792` the port's plain plan (once more when lying short
of it).
The hero's house (vtable 0x595e98, 0x421150; `+0x1e4` the hero, record
field 22; `+0x1e8` taken, `+0x1ec` its tick): 0x24af0 answers the hero,
or 0xff once taken; 0x24aef 1 while not taken; 0x24af1 taken 1, state
`+0xa4` 4, the tick the map's `+0xde`, pictures reloaded (vt+0x38,
0x422e30 `%sSPR_%1u%1u%1u`: `HHOUSE<hero>SPR_043`, standing `_013`); its
tick (message 0) runs only taken and in state 1: 500 ticks after the
take vt+4(0x138b), else `+0xa8 = 100 - (now - t) x 100 / 500`; a
taken house is never in state 1 again (the only writers of `+0xa4` in
its classes: message 2's 1 or the save's, 0x4230f0's vt+0x48), so that
fade and its 0x138b are never reached. The taken house plays out
instead: its picture step (vt+0x3c 0x4230f0, each tick in LandObj
0x425990 message 0) with cycles `+0xc0` 0x595e80 {50, 75, 75, 57, 100,
500} by state steps a frame when `+0xbc` is 0 (vt+0x38 clears it) or
`now >= last + cycle[state] / n`; the step reaching `n` in state 4 or 5
calls vt+0x48 (0x421050: 4 -> 5, 5 -> 2, any other 1), frame 0, vt+0x38
(`HHOUSE<hero>SPR_053`, then `_023`; `_043` started at the standing
picture's frame, kept unless past its count); in state 2 it posts
message 10 to itself (0x4232ed) and the house is removed after the
tick (0x421150 message 3: 0x499860 clears its cell). Theseus's house
(9, 10 and 13 frames): states 5, 2 and removal 89, 540 and 601 ticks
after the take. Blows do not reach it (vt+4 0x421130 drops 0x138e). The
take
(0x4b0e40 by `+0x76e` 0x49): **12** at the walkway's last frame 0x4ba6b0
(the land cell's `+0xc` object the house by id `+0x77e`; its 0x24af0
answer into `+0x78a`, which held the house until then; then 0x4ba3f0:
the first treasure slot of kind 0x12 + hero, none with nothing aboard,
and the hero not 0xff): yes, 0x4ba2a0(hero, 1), step 6 with the party
picture (`+0x76` 8); no, step 14 and 0x4ba2a0(`+0x78a`, 0). 0x4ba2a0:
the viewer's ship only (`+0x24f` against game `+0x3213 + 4 x +0x3212`),
0x50e570 mode 2 at the ship's cell, yes / no by hero 0 0xfc / 0xfd
(`ja1` / `ja4`), 1 0x102 / 0x103 (`th4` / `th1`), 2 0x100 / 0x101 (`or3`
/ `or1`), 3 0xfe / 0xff (`ze3` / `ze1`), 4 0x104 / 0x105 (`he2` / `he4`),
5 0x106 / 0x107 (`ar2` / `ar4`), 6 0x108 / 0x109 (`at1` / `at4`), 7
0x10a / 0x10b (`od5` / `od1`), 8 0x10c / 0x10d (`pe6` / `pe1`), any
other none. **6** at half the party's frames, no party: 0x4bd350(hero,
owner) with a hero place free (0x4be800 mode 7); aboard, 0x4ba7d0 sends
the house 0x24af1 (map vt+0x18 {.., id, 4, 0x24af1}), 0x4b6df0 message
0x5d99 {ship id, class, `+0x54`, `+0x55`, cell, group, hero} to the
owner's player object (type 2), and the item 0x4ba3f0 finds out by
0x4bd500 (not dropped); step 14 either way. The port (`Task::Take`'s
`hero`, `Scene::house_answer`, `hero_voice`, `hero_boards`,
`house_taken`, `taken_houses` kept in the save): the house sailed to its
berth (`vt_berth`, the record's field 10 facing: the constructor
0x420fd0 sets `+0x1d8` 0xa) with its cell the land cell, all the above;
0x5d99 (`hero_news`, see Hero and treasure messages); the order's own
refusal of a ship without a hero place kept (the click's
0x4e6700 room test, mode 6 or 7), its item refusal gone. Not ported:
0x4b6e80's 0x5d94 for the party's take; a berth of (-1, -1) (no facing:
the port's route fails). The playing out is ported (`house_fades`,
`step_house_fades`, kept in the save; the standing picture's frame in
`standing_frames`); not settled: its step's place among the objects'
ticks against the take of the same tick (the port steps it first).

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
needs the town's first check (0x487640 -> 0x43fee0 stage 0) and goes to
the land fleet as message 0x3ee at priority 0x7ff5 with the base's town
(dword 3) and a discount d (dword 9, 0x488810: `r % 21`, + 10 on normal,
+ 20 on hard): at a cell (0x4839e0, flags 0x800), over the base's area
(0x483c40, 0x1800, kind 15), on a flotilla's ships (0x483ab0, 0xc00: one
command per ship, none ending it at once) or the Flame (0x483b80, 0x800);
flag 0x800 takes it off the queue when it ends. Before sending, a previous
cast saved at the commander flotilla's `+0x2d0` is looked up through
0x4a0f60(.., 0, buf) -> 0x42c870, but `buf` is the sender's uninitialised
stack (0x483a19): the port takes the failure path, nothing found or
removed. The passes try it as the brains' (below, 0x42a500 and the town's
check at stage 1 with the ships' count), as order
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
ships with `+0x251` 2 (being built or sold, 0x486a08) are left alone;
one docking goes too, its commands held from the close-in (0x4b37d0), so
the reserve's dock order waits for the back-out (`send_worn`).

**Repairs** (order 96, part 0x3ec, 0x429f60): of the town's buildings
(0x43fcb0) and forts and strongholds (types 0x1494, 0x1a6b: worn, vtable
+0x6c; affordable, +0x9c) the first worn is mended: command 0x1e sent to
the town or the fort (0x520cc0). With none worn the part waits out its
passes and fails. Under way (0x42a2f0) it waits for the building's life
`+0xa8` to reach 100, then sends it order 10 and is done (see Forts' fire);
the building gone, it fails. Ours mends the first worn building of the
player the town may mend (`mend_by`), keeps it as the part's `+0xdc`, and
halts it when whole. A part's count
of passes is set by its op when it is queued (0x595170 + op x 4: build 20,
ship 25, upgrade 20, the rest 10) and when it goes under way (0x595198:
build 20, ship 25, upgrade 20, repairs 10, studies 25). Order 117 is six
studies at each temple, each after the one before. (Not ported: a boat's
request is order 0x5c, whose part skips the price test (field 8 = 1) and
sends the yard byte 3 = 0xff; what the yard does with it unpaid is not
decoded, so ours asks with order 0x50 and waits for the price.)

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
| `LIST_OBJ` | 20 | the map preview's marks (below) |
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

## Diplomacy (0x4efe20)

Whether two players are at war is 0x4efe20(p, q) on player ids `nation * 4
+ slot` (a byte: an object's `+0x54` * 4 + `+0x55`, a ship's `+0x24f`;
player 16 the barbarians' nation 4 slot 0, side-2 castles' 17). It reads
the game's alliance table at `+0x2f45`, 20 x 20 bytes, row p column q:
p = q gives 0; otherwise (t[p][q], t[q][p]) = (0, 0) gives -2, (1, 0) -1,
(0, 1) 1, (1, 1) 2, anything else 0. Below 0 is war; callers pass the
other's player first and their own second (the look 0x4bfc60: the
target's id, then `+0x24f`).

The table is zeroed by a new game (0x4ec1a0, commands 0x100 / 0x10b /
0x10c) and by a load before its `ALIANCES` chunk (0x4ed620, 400 bytes, only
in saved games: no mission, scenario or battle file has one; written by
0x51156c). Only a network game writes it: its set-up 0x532b20 (which sets
game `+0xd80` = 3) marks the allies chosen in the lobby, the message
0x50f910 0x4e5 case 6 toggles t[p][q] (only with `+0xd80` = 3; case 9
clears a leaving player's row and column) and 0x536c3a takes the host's
copy. So in a campaign mission, scenario or custom game every two players
are at war (-2). The port has no network game: `Scene::diplomacy` ports
0x4efe20 over an all-0 table and `Scene::are_enemies` asks it (with the
asleep test of 0x40dfc0). Not ported: the network writers.

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

In detail: each slot keeps its own count (slot `+0xc`, its rate `+0x10`;
a hero slot's at `+0x10` / `+0x14`); `n` = count / 1000, and only when
`n` is not 0 the count becomes count % 1000 and `n` is applied: units
below 100 rise by `n`, capped at 100; the hull, only while `hull x 100 /
full` (0x5c76b0) is under 100, gains `full x n / 100` (integers), then
0x4be030 (the damaged look) and the cap at full. Anything healed (a unit
or the hull) runs the stats refresh 0x4bee60 once at the end. A Cup's or
Fleece's count starts at 0 as it is loaded (0x4bd0c0 at 0x4bd23e), Argus's
as he boards (0x4bd350); a slot taken out moves the later ones down with
their counts (0x4bd4f0, 0x4bd410).

The ship tick 0x4ab930 calls it (0x4abbdb) while `+0x25d & 0x1000`, which
it sets each tick (0x4ab9c6) when treasure slot 0 or 1 (`+0x491`,
`+0x4a9`; no ship type has more than 2, 0x597afc: 1 1 2 1 1 1 1 1 0 0 0
0) has flag 1 or Argus is aboard (0x4ba790). The slot's flags `+8` are
0x4b9740's class (1 for kinds 15 and 16) as it is loaded, but 4 when
player 16 loads it (0x4bd262): a barbarian's Cup or Fleece alone does not
mend. The tick returns before the call on a ship sunk (`+0x251` 4, or by
the blow 0x4befc0 / shot 0x4bf170 of that tick), being built or sold
(`+0x251` 2), turned to stone (`+0x257`), and on the tick 0x4c3ca0
returns non-zero (0x4c3410 state 6's end, the boarded ship let go). The
port runs it at the game tick (`Scene::heal_aboard`, `Ship::heal_slots`,
`argus_count`). Not ported: the bit is never cleared there (only
0x4b37d0's hold zeroes `+0x25d`), so a ship that had Argus or a mending
slot keeps mending after they leave (which matters only to a barbarian's
Cup or Fleece); the skipped tick at 0x4c3410's end.

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

**Brains' casts on their ships** (0x47acf0 through 0x479c20, kinds 9, 12,
15, 16): `pay` first credits the caster member's player one cast's price
(0x47abb0 amber, 0x47ac50 x 100 fish; 0x4a8eb0, 0x4a8f40) as soon as the
target fleet has ships (0x479e50), whatever comes of the casts (none at
all with no ships); then each ship is a message 0x3ee to the casting
member (flags 0, priority 0, dword 3 -1, dword 4 the kind, dword 7 the
ship). A ship group drops it (0x4a09d0); a building group (`LFleet.cpp`)
queues it (0x42c430, generic: after its equals; 10 passes,
0x595170[0x3ee]). Its passes (0x42b910, message 0 every tick: 50 ticks
after one that started nothing, 3 after one that did once woken) walk the
queue until an entry is sent: 0x42a500 tries each of the fleet's towns
(the port: the towns in the member's cells; a stronghold member is not
tried, 0x43fee0 being the town's) with the town's check twice (below);
passing, state 3 and the town command 0x22 {code, x, y, ship, ..., the
discount at +0x1f} at the next tick; else, in a pass after one that started
nothing (`+0x18`), one pass fewer, and at none it fails (state 4). Ended
entries without flag 0x800 stay in the queue, skipped, waking the fleet
at each pass (0x42be84; `Job::Cast::keep`). The town hands
the command to its first temple of the kind idle (0x441ba0 case 0x22;
vt+0x9c 0x4101b0: built, `+0x10c` 0), which pays and casts (below): a
shield or the phantom lasts 3000 ticks more (0xda9a, 0xda97); the Flame is
the player's: brewed 250 ticks at that temple (busy meanwhile), then 1500
more of it (+0xa8, +0xb0). Not decoded: how the credit, to the computer's
spending fields `+0x24` and `+0x1c`, maps onto the port's purse; the port
credits the caster's player's purse, and pays from it. The brains' town
orders share this queue (below, "The brains' town orders"). The brains' players' spells learnt (`+0xb4`, `+0xbc`) and
permission bits are not kept: their checks pass.

**The town's spell check** (0x43fee0; 0x487640 is stage 0 alone): -5
unless the town is built (`+0xa4` 1); kind to (permission bit, spell by
`Magic.cpp`'s numbering): 5 (0x36, 0 the wind), 6 (0x39, 3), 7 (0x38, 12
the big reef), 8 (0x37, 1 the reef), 9 (0x3f, 8), 10 (0x3d, 6), 11 (0x3e,
7), 12 (0x40, 9), 13 (0x3b, 2 the whirlpool), 14 (0x3a, 4), 15 (0x42,
10), 16 (0x43, 11): for kinds 5, 7, 8 and 13 not the spell cast (0x42a500's
codes, 0x42fc80); the permission bit (player `+0x45`), else -5; kinds 5-8,
13, 14 need a sea temple in the town (`+0x224`), the others a war temple
(`+0x228`), else -5; the spell's place in the temple's order (0x596da8 /
0x596ea8, 6 entries) below the spells learnt (`+0xb4` / `+0xbc`), else -7
(the big reef is in neither: kind 7 never passes). Stage 0 ends there.
Stage 1 (with n casts and the discount d): unless d is 100, `(100 - d) x
price x n / 100` amber and fish (0x596844, 0x59687c by the spell) at hand,
else -2; a computer side (3, 4) compares the fish against its hundredths
as they are (0x4a8e80 `+0x24`, 0x4a8f10 `+0x1c`), a human's against
hundredths / 100 (0x4a8d00, 0x4a8ba0); then 0 if a temple of the kind in
its list answers vt+0x9c 0, -1 if one is at work, else -5. A -2 holds
the entry with flag 1, a -1 with flag 4 (no sender sets them).

**The temples' casts** (command 0x22): the war temple (0x439260) brews
the Flame (code 5) if idle and the discounted price is at hand, paying
`(100 - d) x 6000 / 100` amber and `(100 - d) x 15` fish hundredths
(0x4a8ee0, 0x4a8f70); codes 6, 7, and 8, 0xe, 0xf with x and y not
negative, if idle, go to `Magic.cpp` 0x42fa90 -> 0x42f580 (the spell's
check 0x42ed40: a free cast counted at `Magic +0x10`, else the permission
and the price at d off; then 0x42f470 pays `price x (100 - d) / 100`
amber and `price x (100 - d)` fish hundredths, nothing at d >= 100); 8,
0xe, 0xf with x or y negative cast over the area at `+0xf` (0x42faf0):
each own ship (+0x57 1) on its cell in it, row by row, passing 0x42ef90
(the price at d off, within 15 cells of the temple's cell, 0x5576d8 from
`Magic +0/+4` = the temple's `+0x74`/`+0x78`) is cast on by 0x42f580 with
d = 0, at full price. The sea temple (0x438300) sends codes 1-4 and 0xd
to 0x42fa90 without the idle test (the town picks an idle one), the big
reef's kind from payload `+0xd` or the temple's generator `+0x18`; code
0xc pays the wind's price and posts 0x186f (the wind). In full (0x438300
on command 0x22): codes 1-4, 0xd and 0x11 take the command's `+0xd` as the
draw, or with it 0 a draw of the temple's own generator (`+0x18`), made
before 0x42fa90 -> 0x42f580 checks the price; every sender leaves `+0xd`
0 (0x4839e0 and the like zero the message), so a reef's and a big reef's
kind (`draw % 3 + 1`) and a barbarian monster's type (`draw & 3`) come
from that temple. Code 4 (kind 8) is the big reef, 0x42f580 case 0xc: case
1's record (the reef's) with class 0x259, mode 1. Code 0xc (kind 13), with
the discount `+0x1f` not 100, needs `(100 - d) x 400 / 100` amber and
`(100 - d) x 1 / 100` fish (0x4a8d00, 0x4a8ba0: the wind's price,
0x596844[0], 0x59687c[0]), else nothing; below 100 it pays them (0x4a8d40;
0x4a8be0 `(100 - d) x 1` hundredths) and posts 0x186f {3, 0x1103, `+0xd`}
to the weather: message 0x1103 with 0, a calm (below). The area casts of
codes 8 and 0xf (kinds 12 and 16) are code 0xe's (0x42faf0 takes spells 9,
10 and 11; 0x42ef90's cases 8, 0xe and 0xf differ only in the spell
checked, 9, 10, 11). Ported (`town_spell`, `spawn_big_reef`,
`Weather::blow`). No sender asks kinds 8 and 13, nor the area for 12 and
16. After the command the temple speaks when it is the viewing player's
(game `+0x3212` / `+0x3213` its nation and slot): 0x50e570 mode 2 at its
cell (`+0x74` / `+0x78`), event by nation 0-3 0xf4, 0xf5, 0xf7, 0xf6 for
the sea temple (0x438814), 0xf8, 0xf9, 0xfb, 0xfa for the war temple
(0x439260); its `SND_n` lists (dumped) are those of 248-251, none for
nations 0 and 2. It speaks after every code 1-4, 0xd, 0x11 (sea) and 6-8,
0xe, 0xf (war) whether 0x42fa90 / 0x42faf0 then find the price met or not,
after 0xc and the Flame (sea 0x12, war 5) only when they were paid and
started, and after a code neither switch lists (0x438589 index 3); not
when the war temple is busy (`+0x10c`). Ported (`temple_voice`); the
viewing player's own casts are not the temples' in the port (`cast_on`,
`cast_area`), but its Flame and wind play the temple's voice alone as
codes 5 and 0xc do (`cast_flame`, `cast_wind`): neither temple plays any
other sound for them, the Flame's end (0x439260 message 0, `+0x110` 1)
is silent, and the weather's 0x1103 (0x407a80, 0x4079a0) too. Not read:
the interface's path from the panel's command (0x67 flame, 0x58 +
direction wind) to command 0x22.
