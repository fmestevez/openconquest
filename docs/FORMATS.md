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
wake animation frames; the remaining frames are turning animations using
24 hull rotations (`CORP_1A0000`-`0023`).

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

Amber deposits hold record field 4 x 100 units (negative: unlimited)
(0x48a800).

Wind (WeatherClassTy, Weather.cpp; global object at 0x5c6a40):

- State: strength 0-10 (+0x34), direction 0-7 (+0x38, the heading the
  wind blows towards, 0 east counter-clockwise), and the prepared next
  values (+0x3c, +0x40). Init (0x407e60): next strength rand&7 + 2, next
  direction rand&7, then one change.
- Change (0x407d20): current = next; with 95% chance the next strength is
  current (3 if calm) + rand%5 - 2, wrapped (negative: 11 - v; above 10:
  v - 11), 1 becomes 0, and the next direction is current + rand%5 - 2
  mod 8; otherwise both are random (rand%11, rand&7). Changes happen
  after rand%10000 + 1500 ms.
- Message 0x1103 forces strength 7 in a given direction for a while (the
  CBUT_WIND command), then the old wind returns.
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
temple's list. Effects: SLIGHT1-4 lightning, FIRE0-2 fire, PROTWSPR_64/128
and PROTMSPR_64/128 protection auras for smaller/larger ships. The wind page (menu page 4) is a
compass: command 0x58 + direction in slots 6, 2, 1, 0, 4, 8, 9, 10 around
0x57 in slot 5.

Combat tables:

- Ships carry items in nine slots, numbered like the building commands
  0x2d + item (button names): 0 bowmen (ONION), 1 javelin men (DART), 2
  swordsmen (SWORD), 3 BLDTRANS, 4 gastraphetes (BLDGASTR), 5 small
  catapult (BLDSMALKAT), 6 ballista (BLDBALIST), 7 big catapult
  (BLDBIGKAT), 8 onager (BLDONAGR). Settlements train items 0-2 (30
  amber each, u16 table at 0x597164, plus fish hundreds 2, 2, 3 at
  0x597170); menu 0x4361c0 builds items 3-8 (50 amber for 3-5, 70 for 6-8).
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
ground `07`/`08`), layer 1 shoals, and higher layers shores, grass, desert,
rocks, forests, hills and ruins. Drawing the sea tile `b_sea00_001` under
every cell and then the layers in order, row-major, reproduces the map.

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
| 0x1f8, 0x1f9 | `HarpyClassTy` | harpy, dragon | | |
| 0x1fa | `MedusaClassTy` | medusa (`GORG%1d`) | 5, 6 | 7, 8: area |
| 0x1fb | `UMonsterClassTy` | sea monster | 7, 8 | 4: type |
| 0x1fc | `LandingClassTy` | landing party | | |
| 0x1fd | `SirenaClassTy` | siren | 5, 6 | 7, 8: area |
| 0x258 | `WhirlObjClassTy` | whirlpool | 5, 6 | |
| 0x259, 0x25a | `ReefObjClassTy` | big, medium reef | 5, 6 | 7: reef kind 1-3 |
| 0x25b-0x25d | `LightningObjClassTy`, `TornadoClassTy`, `SlightClassTy` | weather | | |
| 0x25e | `VolcanoClassTy` | lava flow | 6, 7 | 4: lava sequence 0-10 |
| 0x2bc | `TreasClassTy` | treasure bookkeeping, not placed on the map (object kind 0) | | 4: treasure index |
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

### Placement and timing

Nature objects place their frame's top-left corner at `cell * 64` minus a
per-class offset (castles: sea 6, 12 and war 4, 13). Their frame period is
`cycle[state] / frame_count` game ticks, with cycle tables per class (reef
30/60/30, whirlpool 60/60/60, castle 300/20/100/75). A game tick is 20 ms
at speed 3; the speed table at 0x5bed7c has one 24-byte row per speed.

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

## Missions (partially understood)

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
1 = computer; +0x04 u32 starting amber; +0x08 u32 nation; +0x0c u32 colour
slot; +0x10 u32 starting fish. Every mission has exactly one human player;
(nation, slot) matches the owner fields of the object records.

Saved games (`demo.dkd`) add `DARK_MAP` (fog of war), `ALIANCES`,
`NAME_PLAYERS`, `AI_PLAYERS`, `DIFFICULTY`, `SPEED_GAME`, `RND_INIT` and
others.
