# OpenConquest

An open source reimplementation of **Ancient Conquest: Quest for the Golden
Fleece** (Re:Action Entertainment / DreamCatcher, 1999) in Rust, using the
original game's data: the same game, played faithfully, on modern systems
and any screen.

OpenConquest does not include any game assets. You need an original copy of the
game CD.

## Status

Playable: the campaign, single missions and battles run with the original's
graphics, sounds, music and videos, and most of the game's rules. Values
and behaviour are recovered from the original executable with Ghidra and
documented in [docs/FORMATS.md](docs/FORMATS.md); anything still estimated is
marked as a placeholder in the code.

### Data and tools

- `crates/formats` reads every format on the CD: the installer package,
  DKX/DKD archives, LZ compression, bitmaps, sprites, masks, palette
  animation, tile sets, terrain maps, sounds and missions.
- `openconquest-extract` installs the data from the CD and dumps archives to
  PNG.
- `openconquest` plays any campaign, single or battle mission.

### Map and interface

- Terrain with animated water and coastlines, and every object a mission
  places, animated on the original's timing (one game tick is 40 ms at the
  default speed).
- The original interface: framed view, control bar with minimap, object
  panel, command buttons and pointers.
- Fog of war over what you have not explored or cannot see (`--no-fog`
  turns it off).
- The wind changes as in the original and speeds or slows ships by their
  heading; the wind rose shows it.

### Ships and economy

- Ships sail cell to cell, one to a cell, turning before they set off;
  royal ships, bombers and rammers sail around shoals.
- Merchants gather amber (walkway and porters) and fish (net out), hunt
  leviathans and unload at your ports; they can work a zone on their own.
- Shipyards build ships, settlements put up buildings and grow into towns,
  shipyards into docks, all at the original costs and times.
- Settlements train marines, workshops build weapons, sage schools research,
  smithies fit armour and rams; ships take on what they have room for.
- Settlements sell items or themselves; docks repair ships at the
  original's rate and price.

### Combat

- Each weapon fires a projectile with the original's picture, flight and
  impact, at its original rate and range, hitting by the original's odds;
  a blow is shared between hull, crew and what the ship carries.
- Hull, building and creature strengths, armour and ramming are the
  original's.
- Boarding is decided by the crews' strength and shown with the original's
  fight; the winner takes the ship.
- Warships have the behaviour switches of the original: guard, ram, board,
  fire on aggressor, scout, patrol, guard zone, attack zone and convoy.
- Forts fire with the weapons they hold.

### Creatures and magic

- Cyclops, minotaurs and amazons come out of their lairs and throw at ships;
  sea monsters bite; harpies strike from the air and dragons sow their teeth
  as new monsters; the Gorgon turns ships to stone; sirens charm ships to
  them.
- Temples of the Sea and of War learn and cast all twelve spells with the
  original's effects, durations and reach.
- Treasures, gifts of the gods and heroes help the ships that carry them.

### Missions

- The campaign's mission scripts are ported: their fleets patrol, gather,
  guard and raid as in the original, at its three difficulty levels
  (`--difficulty easy|normal|hard`).
- Missions are won by their objectives or victory conditions, and the
  campaign reveals technology mission by mission as the original does.
- The original's statistics and score at a mission's end.
- Skirmish maps use a simpler stand-in AI.

### Sound and video

- The original's sound effects, placed by distance from the view.
- The menu music, the CD's audio tracks if ripped to `<data>/cdaudio` (or
  `--cd-audio DIR`), or else the original MIDI music through a General MIDI
  SoundFont (`--soundfont FILE.sf2` or `OPENCONQUEST_SOUNDFONT`).
- The original's intro and story videos, decoded from the CD.

### Not yet done

- The campaign scripts' story events (partly decoded).
- The original's multiplayer screens and network play with the original game
  (OpenConquest has its own lobby).
- Some values are still estimates, among them how strongly some gifts and
  heroes help.

### Modern improvements

- The original interface (framed view, control bar with minimap and object
  panel) adapted to any aspect ratio: the bar keeps its place on the right
  and the view widens.
- Any window size and aspect ratio. The view is scaled to match the original
  600-line screen height, so widescreen and ultrawide monitors see more of the
  map instead of a stretched image.
- Borderless fullscreen (F11 or Alt+Enter), HiDPI aware.
- Optional integer scaling (`--integer-scale`) or a fixed scale (`--scale 2`).
- Zooming with the mouse wheel or a trackpad pinch; panning by right/middle drag or a two-finger swipe.
- The original front end, checked pixel for pixel against the original
  running under Wine: the title screen (with the menu music), Best
  Conquerors, Start scenario's "Your mission" briefing with its difficulty
  boxes, Custom missions (the battles and single missions, the chosen one's
  map from the small tiles, its description and players with their flags
  and computer level), Load game (saved games of the campaign and of other
  missions, or a campaign mission to start from) and the multiplayer
  screen (OpenConquest's own network game behind it, so its one way to connect is
  TCP/IP). The screens are drawn from the original's own widget pictures
  and fonts. The mission accomplished and failed screens are OpenConquest's own
  so far.
- Quick save and load at any moment (F5 / F9), plus eight save slots.
- Adjustable game speed (half to double) and scroll speed.
- Minimap markers coloured by side: yours, enemies, creatures.
- Building sites are outlined green or red while you choose them,
  refused commands say why, and command buttons show what they do and
  cost when you point at them.

## Getting started

Requires Rust 1.82 or newer. On Linux, sound needs the ALSA development
package (`libasound2-dev` or `alsa-lib-devel`) and the folder dialogs GTK 3
(`libgtk-3-dev`).

The simplest way: build and start the game; the first time, it asks for
your Ancient Conquest CD (or a mounted image of it) or the folder the game
is installed in, copies the data it needs from a CD (about 250 MB, into
your user data folder), and remembers the choice:

```sh
cargo run --release -p openconquest
```

On macOS, `scripts/bundle-macos.sh` builds `dist/OpenConquest.app`, which you can
double-click (or copy to Applications); its icon is cut from the original's
title art in your game data, when the script finds it. The game data then lives in
`~/Library/Application Support/OpenConquest/gamedata` and the choice in
`~/Library/Application Support/OpenConquest/config.json` (delete it to choose
again).

Or by hand:

```sh
# 1. Install the game data from the CD or a mounted ISO
cargo run --release -p openconquest-extract -- install /Volumes/ANCIENT_CONQUEST gamedata

# 2. Run
cargo run --release -p openconquest -- --data gamedata --mission miss000
```

On Windows, use the CD's drive letter, e.g. `install D:\ gamedata`.

### Controls

| Input | Action |
|-------|--------|
| Arrows / WASD, screen edge (fullscreen or `--edge-scroll`) | Scroll |
| Right or middle mouse drag | Pan |
| Mouse wheel, trackpad pinch, `+` / `-`, `0` | Zoom, reset zoom |
| Trackpad two-finger swipe | Move the map |
| Mouse wheel on a menu list | Scroll it |
| Left click | Select a ship, building or creature |
| Left drag | Select your ships inside the box |
| Shift + left click / drag | Add a ship to the selection or remove it / add the ships in the box |
| Right click (with ships selected) | Sail there (straight courses where the water allows, around islands and, for deep ships, shoals; a group spreads out around the point) |
| Shift + right click | Add a waypoint to the route |
| Command buttons | Move (then click the destination), Stop, Fire (then click an enemy ship or building), Collect amber / Fish / Hunt leviathan (then click a deposit or zone), Repair (then click a dock, or for a settlement a damaged building), Behaviour, ship building and upgrades work, for every selected ship where it applies; the rest are shown greyed until implemented |
| Right click on amber or fish (with a merchant selected) | Gather it |
| Build structures (settlement or town) | Pick a building, then click a land cell nearby (green outline: allowed; ports and shipyards need the shore); Esc goes back |
| Right click on an enemy ship or building (with a warship selected), or Fire | Attack it (unarmed fire ships ram instead) |
| Ram / Board, then click an enemy ship | Ram it / board it |
| Build (merchant), then click a shore cell | Sail there and found a settlement |
| Esc | Cancel, close a panel, deselect |
| Click/drag on the minimap | Jump |
| Tab | Toggle the interface (full-window view) |
| Bar tabs (wheel, disk, wrench, clef, scroll) | Object panel; save slots, restart and quit; game and scroll speed; sound volume; objectives and briefing |
| P | Pause |
| Esc (nothing selected) | Main menu (Resume returns to the game) |
| Best results (main menu) | Your fastest win of each mission (kept in `<data>/saves/best.json`) |
| F5 / F9 | Quick save / quick load (`<data>/saves/quick.json`; `--load FILE` continues a save) |
| M | Sound and music on/off (`--mute` starts with it off) |
| F1 | Show the mission briefing again (it is shown at the start; the game waits until it is closed) |
| Page Up / Page Down | Previous / next mission |
| F11, Alt+Enter | Fullscreen |

### Multiplayer

Two to eight players can play a map together over the network (the
`battle01`-`battle25` maps; each player needs the same build of OpenConquest
and their own copy of the game data). From the title screen, MultiPlayer game:
the host picks a map and the number of players and chooses Host; the
others type the host's address (shown on the host's screen) and choose
Join; the game starts once everyone is there. Or from the command line:

```sh
openconquest --host --mission battle05 --players 3   # waits on port 17999
openconquest --join 192.168.1.20                      # or host:port
```

The host's map players are taken in order: the map's own player for the
host, then its Greeks and Persians for those who join; the rest stay
computer players. Every machine runs the whole game in lockstep: only
orders travel, each carried out a few turns (a tenth of a second each)
after it is given, so a slow connection shows as a short delay; the host
warns if the games drift apart. Enter opens a chat line to the other
players (Enter sends it, Esc drops it). Multiplayer games cannot be
paused, sped up or saved, and a player who leaves cannot come back.

### Other tools

```sh
openconquest --list-missions
openconquest --mission battle07 --screenshot shot.png --window 2560x1080 --focus 20,12
openconquest-extract list gamedata/maplib.dkx
openconquest-extract dump gamedata/data/objects.dkx out/ --filter TARAN
openconquest-extract video gamedata/video/title.avi frames/ --every 25
```

## Development

```sh
cargo test
# Also decode every resource of an installed copy:
OPENCONQUEST_DATA=$PWD/gamedata cargo test --release
```

The workspace has three crates:

- `crates/formats`: the data format library, with no platform dependencies.
- `crates/extract`: the `openconquest-extract` tool.
- `crates/game`: the `openconquest` game (winit + softbuffer software renderer).

## Legal

OpenConquest is an independent project, not affiliated with the original developers
or publishers. Ancient Conquest and its assets are the property of their
respective owners. Do not commit game data to this repository.
