# OpenAC

An open source reimplementation of **Ancient Conquest: Quest for the Golden
Fleece** (Re:Action Entertainment / DreamCatcher, 1999) in Rust, using the
original game's data.

OpenAC does not include any game assets. You need an original copy of the
game CD.

## Status

Early work. What exists today:

- Readers for the game's data formats (`crates/formats`): the installer
  package, DKX/DKD archives, LZ compression, bitmaps, sprites, masks, palette
  animation, tile sets and terrain maps. Every archive entry on the CD
  decompresses, and every image and map decodes.
- `openac-extract`: installs the data from the CD and dumps archives to PNG.
- `openac`: renders any campaign, single or battle mission: terrain with
  animated water, plus every object the mission places (ships in their
  owners' colours, towns and buildings, castles, monster lairs and nests,
  sea monsters, fish and leviathan grounds, amber, reefs, whirlpools, lava
  flows, sheep), with smooth scrolling, zoom and a minimap.

Object placement, naming and animation timing were recovered from the
original executable with Ghidra; see [docs/FORMATS.md](docs/FORMATS.md).

You play the mission's human player: you can select anything, and command
and sail your own ships. Merchants gather amber and fish and bring them to
your port; shipyards build ships (hull costs and building requirements from
the original); settlements upgrade to towns and shipyards to docks, and put
up new buildings nearby (ports, shipyards, workshops, smithies, schools,
temples and forts, with the original costs and construction, damage and
ruin graphics). Your
amber and fish are shown in the money panel. Warships fire at enemy ships
and buildings with their original weapon loadouts and damage values; idle
ships and forts fire at enemy ships in range, and sunk ships and destroyed
buildings explode (buildings leave ruins). Ships can ram (odds from the
original's ramming power, crew and angle-of-approach tables; rammers hit
half again as hard) and board enemy ships, whose marines fight it out;
a ship whose crew falls is captured. Merchants found new settlements on
any free shore. Settlements train squads of marines (bowmen, javelin men and
swordsmen, at the original costs) that ships alongside take on with Load;
ships carry the marines and weapons (catapults, ballistae, onagers, ...)
given in their mission records.
A Temple of the Sea learns spells in the original order and at the
original prices: so far calling the wind, raising reefs and summoning
whirlpools work, and whirlpools and reefs damage ships that sail into them.
Missions are won by meeting their original victory conditions (kill all
enemies, and in some single missions the dragons, harpies, wizards, sirens,
gorgons, sea creatures, amazons, minotaurs or cyclops; the objectives are
shown in the message line) and lost when you have no ship or building left.
A Temple of War learns protection from weapons and from magic, flame and
lightning, with the original effect animations.
The wind changes as in the original and speeds up or
slows down ships by their heading; the wind rose shows it. Computer players (a simple stand-in for the original
AI) keep their own amber and fish: their merchants gather, their shipyards
build merchants and then the strongest warships they can afford, their
warships hunt your ships within ten cells, and after five minutes they
send waves of three or more warships against your towns and fleet. Hull and building strength, ram damage, boarding pace, training time,
crew capacity, spell study time, durations and damage, hazard damage, fort weapons, range, reload time,
build and construction times, building placement rules and gathering
rate are placeholders until recovered from the
executable. Not yet implemented: the rest of the game logic (wind,
crew and weapon upgrades, the remaining spells: view map, tornado,
sea monster, ghost), the original AI, the
original interface panels, sound, music, video and multiplayer. See [docs/FORMATS.md](docs/FORMATS.md) for what
is known about the file formats.

### Modern improvements

- The original interface (framed view, control bar with minimap and object
  panel) adapted to any aspect ratio: the bar keeps its place on the right
  and the view widens.

- Any window size and aspect ratio. The view is scaled to match the original
  600-line screen height, so widescreen and ultrawide monitors see more of the
  map instead of a stretched image.
- Borderless fullscreen (F11 or Alt+Enter), HiDPI aware.
- Optional integer scaling (`--integer-scale`) or a fixed scale (`--scale 2`).
- Zooming with the mouse wheel; right/middle-drag panning.

## Getting started

Requires Rust 1.82 or newer.

```sh
# 1. Install the game data from the CD or a mounted ISO
cargo run --release -p openac-extract -- install /Volumes/ANCIENT_CONQUEST gamedata

# 2. Run
cargo run --release -p openac -- --data gamedata --mission miss000
```

On Windows, use the CD's drive letter, e.g. `install D:\ gamedata`.

### Controls

| Input | Action |
|-------|--------|
| Arrows / WASD, screen edge (fullscreen or `--edge-scroll`) | Scroll |
| Right or middle mouse drag | Pan |
| Mouse wheel, `+` / `-`, `0` | Zoom, reset zoom |
| Left click | Select a ship, building or creature |
| Right click (with a ship selected) | Sail there (routes around islands) |
| Command buttons | Move (then click the destination), Stop, Fire (then click an enemy ship or building), Collect amber / Fish (then click a deposit), ship building and upgrades work; the rest are shown greyed until implemented |
| Right click on amber or fish (with a merchant selected) | Gather it |
| Build structures (settlement or town) | Pick a building, then click a land cell nearby (green outline: allowed; ports and shipyards need the shore); Esc goes back |
| Right click on an enemy ship or building (with a warship selected), or Fire | Attack it (unarmed fire ships ram instead) |
| Ram / Board, then click an enemy ship | Ram it / board it |
| Build (merchant), then click a shore cell | Sail there and found a settlement |
| Esc | Deselect (quits when nothing is selected) |
| Click/drag on the minimap | Jump |
| Tab | Toggle the interface (full-window view) |
| F1 | Show the mission briefing again (it is shown at the start; the game waits until it is closed) |
| Page Up / Page Down | Previous / next mission |
| F11, Alt+Enter | Fullscreen |

### Other tools

```sh
openac --list-missions
openac --mission battle07 --screenshot shot.png --window 2560x1080 --focus 20,12
openac-extract list gamedata/maplib.dkx
openac-extract dump gamedata/data/objects.dkx out/ --filter TARAN
```

## Development

```sh
cargo test
# Also decode every resource of an installed copy:
OPENAC_DATA=$PWD/gamedata cargo test --release
```

The workspace has three crates:

- `crates/formats`: the data format library, with no platform dependencies.
- `crates/extract`: the `openac-extract` tool.
- `crates/game`: the `openac` game (winit + softbuffer software renderer).

## Legal

OpenAC is an independent project, not affiliated with the original developers
or publishers. Ancient Conquest and its assets are the property of their
respective owners. Do not commit game data to this repository.
