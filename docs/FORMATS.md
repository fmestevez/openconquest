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

`openac-extract install` copies these (lower-cased) and unpacks `SETUP.DAT`
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
other); 88-151 8 headings x 8 frames furling the sail. Ships keep to the
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
  three unknown (+0x60..+0x62), ram (+0x5e), crew quality per crew type
  (+0x67..+0x69), arming (+0xa3, 0 read as 1), an unknown u32, then rate of
  fire, range and damage per item group (+0x8e, +0x95, +0x9c). Nothing else
  writes the crew levels: crews are not upgraded in play. All zero except
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
  refits ships to armour M/S (0x36, 0x37) and ram M/S (0x38, 0x39) up to the
  researched level, for 30/60 and 25/50 amber times a ship size factor;
  its 0x49-0x4e are the W/M/S variants.
- Building records (66 fields): field 7 condition in percent; fields 22-33
  flag twelve emplacements on forts (wood fort 3, stone fort 8), contents
  unknown.
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
| 2-4 | unknown (category / layer / passability?) |
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
| 0x1f8, 0x1f9 | `HarpyClassTy` | harpy, dragon (sent out by their nests) | 4, 5 | 6: player, 7: 1 (new), 8-10: the nest's fields 7, 8, 11 |
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
| 0x2bf | `TraksClassTy` | music tracks (not drawn) | | |
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
scroll `BMARK_VHELP`. Panels: `CFILE_BKGND` (200x454; name field, list
with scrollbar `CFILE_SLSL`, buttons `CFILE_BMRKSAVE/LOAD/DEL`, then
`CFILE_RSTART`, `CFILE_MMENU`, `CFILE_QUIT`, captions drawn by the game;
0x50a700), `CCFG_BKGND` (202x552) with slider tracks `CFGSCR_GSPD/KSPD/
MSPD` and thumb `TRACKBAR_BB`, sound `CFGSND_*`, help `HVIEW_BKGND`. The
pause sign is the `WIN_PAUSE` sequence (197x156, 31 frames).

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
`HVIEW_HELP00` at (3, 513) and (103, 513). The text is the briefing's
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
(0x43c740): building (0), reload object (1, onto a ship alongside), repair
(2), upgrade to town (4), sell item (5), sell town (6), and from slot 8 the
marines it can train: only with a workshop of the settlement, and for
swordsmen a smithy (0x43da20, +0x1fc, +0x200).

## Selection box

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

## Building sites

The landscape grid (`[0x5c74fc]`: width, height, cell width and height as
shorts, then 24 bytes a cell) holds each cell's tile types, a nibble a map
layer from the tileset records' type byte (+4: sea 7, shelf 1, land 8, ...;
0x5456c0), and the object standing there (+12). Probed on the original in
miss000: a building goes on a free land cell, trees and rocks no matter; a
moorage or shipyard on the shore, water beside it, up to two cells from its
settlement's own cells; other buildings inland (land on all four sides, the
map's edge counting as land), up to one cell away. A moorage's front is the
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

Buildings (LandObjClassTy 0x422e30, TownObjClassTy 0x43bd60): the
picture's damage digit is ceil(health x 2 / 67) (0x594ab0) for a standing
building, 3 otherwise; its frames step every cycle[state] / frames ticks
(the class's cycle table at +0xc0: every player building's is 200, 75,
75, 75 ticks, 0x596ab8 and its siblings; so a standing building's 12
frames step every 6 ticks and its ruins' 20 every 3).

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
without counting itself. The hull at 0 wrecks the ship (state 4). At a
dock (0x4b1fc0) every 9 ticks a ship gets back up to a tenth of its hull
for its type's price (0x59b388: 140, 280, 800, 910, 650, 560, 490, 420,
420, 630, 420, 630) times the share mended, and worn units are made whole
for `cost[kind / 3] x (100 - condition) x 9 / 1000`; short of amber, the
human's ship gets what the amber buys, a computer player's stops.

Buildings, strongholds, castles, nests and lairs hold points out of a
maximum by class (0x424910: settlement 160000, town 220000, forts and
barbarian strongholds 180000, workshop, shipyard and cyclops lair 100000,
smithy and port 130000, dock 140000, school 81500, Temple of the Sea
67500, Temple of War 110000, castles 300000, dragon nest 350000, harpy
nest 280000, minotaur lair 80000, amazon lair 60000, class 0x49 40000),
their percentage at `+0xa8`. They take a blow whole (message 0xda96 and
the like), a fifth of it under a magic shield (`+0x15c`), and fall at 1
(0x425990).

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
ticks. A shot hits (0x4bf7f0) with `43 + ease - 33 x distance / (aim x
64)` percent, distance in pixels and aim in cells (0x598130: 9, 7, 7, 9,
9, 9, 13, 11, 11, 7, 9 by item, alike at every level); the ease is 25,
for a ship its type's (0x597acc: 5, 8, 20, 25, 15, 15, 10, 10, 10, 15,
10, 15), 1 with hero 3 aboard, and a phantom is hit at 20 % whatever the
range; Greek fire always hits.

## Training and repair at a settlement

A settlement's stock places (`[+0x244] + 0x23`, twelve) list units of two
bytes: kind and condition (percent). Message 0x1e with sub-order 2 (its
Repair) sums, over units below 100, amber `cost[kind / 3] x (100 -
condition) / 100` (0x597164: 30, 50, 70, ...) and time `ticks[kind / 3] x
(100 - condition) / 100` (0x5971a0: 70, 80, 95, 1200, 1300, 0, 400, 450,
...; 0x440fd0), takes the amber at once and enters state 2 (`+0x110`),
mending the units one after another as the time passes and all of them at
its end (0x441ba0). State 1 is training, a queued unit taking
`ticks[kind / 3]`. Docks (`Shipserv.cpp`, 0x4e7640) serve ships alongside
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
maximum, blows what they carry.

## Land creatures

Lairs (`CreaturesFlatClassTy`, 0x414f00) send out another creature once
`(10 - field 9) x (20-25)` ticks have passed since the last, drawn afresh
at each look, up to field 8 out. A creature (0x4111b0) shows
`<CLP|MNT|AMZ>SPR_<state><heading>` with a frame delay by state
(0x59538c: 1, 25, 1, 1, 1, 2, 2 ticks; walking's 25 becomes 5 after the
first frame, 0x411520) and walks a cell a pass of its walking picture. It
wanders (0x413240) to any land cell (tile type 8) it can reach within a
round mask about where it stands (13 x 13 cells: 0x594ba0 cyclopes, radius
5; 0x594e44 and 0x5950e8 minotaurs and amazons, radius 4).

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
0x46b8d0-0x46d830 and its main 0x46e8e0, miss012's 0x46f760-0x472370 and
0x472900, miss013's 0x473f40-0x476dc0 and 0x477930. Mains: miss000
0x449b10, miss001 0x44af70, miss002 0x44d0e0, miss003 0x44fd90, miss004
0x453b00, miss005 0x457950, miss006 0x45b280, miss007 0x45f770, miss008
0x463240, miss009 0x4663d0, miss010 0x46a0f0.

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
| 15-17 | items of group item / 3 (marines, medium, strong weapons; also the arming studied >= the group) |
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
miss011 (0x46e110) 46, 47, 76, 77 (each also re-grants the scrolls of two
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

Buildings, lairs, nests and monsters hold treasures in their records: a
count, then five dwords per treasure like a treasure record's fields 4-8
(kind, holder -1, 0, parameter, amount), at field 10 for lairs (0x46-0x48),
11 for fortresses and barbarian structures (0x34-0x36, 0x3f-0x41), 12 for
nests (0x44, 0x45), 17 for sea monsters and 60 for settlements and towns.
They fall when the holder is destroyed; this is where the campaign keeps
its scrolls and most heroes' items.

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
