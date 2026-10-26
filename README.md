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
and sail your own ships. Merchants gather amber (a walkway to the
shore, porters bringing it aboard; exhausted amber grows back as often as
the mission allows) and fish (the net out) and bring them
to your port (a walkway goes ashore and porters carry the goods in, which
takes a few seconds), and hunt leviathans, worth 5000 amber and 2500 fish each as in
the manual (one to a zone, caught in 30 seconds: placeholders); shipyards build ships (hull costs and building requirements from
the original); settlements upgrade to towns and shipyards to docks, and put
up new buildings nearby (ports, shipyards, workshops, smithies, schools,
temples and forts, with the original costs and construction, damage and
ruin graphics; a building goes up as its construction does). Your
amber and fish are shown in the money panel. Warships fire at enemy ships
and buildings with their original weapon loadouts and damage values; idle
ships and forts fire at enemy ships in range, and sunk ships and destroyed
buildings explode (ships sink with their original animation, buildings
leave ruins; ships being built show at their shipyard). Ships can ram (odds from the
original's ramming power, crew and angle-of-approach tables; rammers hit
half again as hard) and board enemy ships, whose marines fight it out;
a ship whose crew falls is captured. Merchants found new settlements on
any free shore. Settlements train squads of marines (bowmen, javelin men and
swordsmen, at the original costs) that ships alongside take on with Load;
workshops build weapons (gastraphetes, catapults, ballistae, onagers) that
ships take on the same way, and research the rate of fire, strength and
range of marines, medium and strong weapons (the original's tables). Each
weapon fires on its own, at its original rate, between its original
shortest and longest range. Ships have the original room aboard (the
manual's displacement: a boat 4, a bomber 7, a ship killer 2, ...), where a
squad or a medium weapon takes one place and a strong weapon two; merchant
ships carry no weapons. Royal ships, bombers and rammers sail around
shoals, as in the original. Damaged ships are repaired at your docks
(Repair, then click the dock) and damaged buildings by a settlement or
town (Repair, then click the building), for amber as the work goes on (rate
and price are placeholders). A warship's behaviour page has the original
switches: guard (fire at enemies in range; on by default), ram attack and
board attack (an idle guard rams or boards instead of firing), and fire on
aggressor (turn on whoever shoots at the ship); scout (sail out to a
point and back, turning home at once if fired on) and patrol (back and
forth between where the ship is and a point, firing at enemies it passes)
work too, and so do the guard zone (drag a rectangle: the ship fights
enemy ships that come into it) and the attack zone (it fights every enemy
ship and building in it until none is left) and convoy (drag a rectangle
around one of your ships: the warship keeps close to it and fires at
enemies in range). A merchant's Resource page sets amber, fish or
leviathan zones (drag a rectangle): the ship gathers from the deposits in
the zone on its own until moved.
Treasures sparkle on the map; send a ship to take one (Treasure, or right
click it). Barrels of amber and fish pay at once; a Gift of Poseidon casts
its spell where it lay (a protection on the ship, or a reef, whirlpool,
waterspout or monster around it); disposable magic gives one free cast at a
temple of its kind; goods, weapons and scrolls of knowledge are handed over
at your settlements (a scroll reveals technology: silver armour, strong
weapons, bombers, Greek fire, ...); the other gifts stay aboard (as many as the ship has
room for, as in the manual): the torch of Ares protects from fireballs, the
Armour of Ares takes part of every blow, the feather of Boreas speeds the
ship and strengthens its ram, magic wood keeps boarders off, and the Golden
Fleece mends the ship. A ship carrying a hero's magic item can take the
hero from his house (warships with room for heroes): Jason's ship is not
harmed by whirlpools and reefs, Theseus adds strength and range of fire,
Orpheus keeps monsters off, Zetes sails as if before the wind and fears no
waterspout, Hercules cannot be beaten in boarding, Argus mends the ship and
stands up to monsters, and Atalanta fires faster and truer. How much each
gift and hero helps is a placeholder; the Gorgon's head, the Cup of
Asclepius has no effect yet.
Wild creatures fight: cyclops, minotaurs and amazons come out of their
lairs (up to the number the map gives each lair), roam the land about it
and throw stones, axes and arrows at ships in reach; sea monsters bite; harpies and dragons fly out of
their nests at ships nearby, harpies tearing at crews (not Odysseus's),
dragons dropping their teeth on hulls, and fly home; shot down, they are
followed by another while the nest has some left (as the original's nests
do; their speed, reach and strength are placeholders); the
Gorgon turns ships in her area to stone for a while, sharing her power
among them (not Perseus's, nor ships with the mirror shield or the magic
shield); sirens draw ships in their area towards them, onto whatever
reefs lie between (not Odysseus's, nor shielded ships). Their weapons,
periods and strength are placeholders.
Weapons hit crews as well as hulls (the original's crew damage column):
wounds cost a ship squads, and the bowmen and javelin men aboard shoot at
enemy crews themselves. The unique ships carry their own weapons: the
Greek fire ship's ballista with Greek fire, the quick lime ship's
catapult, which only kills crews, and the fire rammer's barrel of powder,
which goes off in its first ram (figures from the manual); the ship of the
dead's undead crews are twice as strong in boarding. How many wounds cost a
squad is a placeholder. The unique ships must first be studied at the sage
school (2000 amber each, offered once the school's arming has reached
level 2: Greek fire and quick lime ships for the Greeks, fire rammers and
ships of the dead for the Persians). Sage schools study
rams, armour and rigging (original costs; how much they help is a
placeholder), and smithies fit armour and rams to ships alongside, up to
the studied level.
A Temple of the Sea learns spells in the original order and at the
original prices: calling the wind, raising reefs, summoning whirlpools, a
waterspout that drifts with the wind and wrecks ships, a sea monster
that fights for you for a while, and the view, which clears the clouds
over a chosen area; whirlpools and reefs damage ships that sail into them.
Campaign missions are won by their story's objectives, shown in the message
line: the heroes the mission names (`HERO_ARR`) won over, and this port's
reading of the briefing (the parchment taken aboard, the Gorgon killed, the
enemy defeated, a new settlement founded, the heroes brought to your
settlement, together with the heroes the campaign has won so far, who wait
at your starting settlement for a ship to take them aboard with Load, or
ride on your first ship if you have no settlement); the scripts
place the wind and fire wizards' castles (the wizards strike ships in
their reach with lightning or fire) and the dragon's nest guarding the
Golden Fleece, as the original's do. In miss002, once Argus is at your
town your shipyards can build the Argo (a royal ship; there is only one),
and the mission wants Jason and Argus aboard it; from miss003 on the heroes
start out aboard the Argo (your royal ship nearest the mission's starting
view, or, in miss003, which starts without one, launched there as the
original's script does). Missions open with the view where the original
sets it.
Buildings, lairs, nests and sea monsters hold the treasures their records
list (most heroes' items and the scrolls among them) and drop them when
destroyed; a hero's item found nowhere is held by the enemy building
nearest the hero's house. As in the original, the campaign reveals
technology gradually: each mission allows the buildings, ship types,
weapons, research, fittings and spells the original's scripts allow by
then (with the earlier missions' scrolls taken as delivered), and scrolls
of knowledge reveal more; skirmishes allow everything. Going on to the
next mission keeps the research, spells and technology gained (a mission
picked from the menu starts from the original's defaults for it). The original's mission scripts are ported: all fourteen campaign
missions' brains run (their fleets patrol, gather, guard their waters,
answer fire and raid as in the original; their yards build the ships each
fleet asks for, and the brains lay the extra deposits; miss000 gives the
player its bonus amber and fish), at the original's three difficulty
levels (`--difficulty easy|normal|hard`, normal by default; saved games
keep theirs). miss000's to miss006's follow the original
closely; miss007's to miss013's are coarser (a few primitives are not yet
decoded, and their raid timing and some routes are approximated). The
stand-in AI runs skirmish maps. Other
missions are won by meeting their original victory conditions (kill all
enemies, and in some single missions the dragons, harpies, wizards, sirens,
gorgons, sea creatures, amazons, minotaurs or cyclops; the objectives are
shown in the message line) and lost when you have no ship or building left.
A Temple of War learns the weapon shield (80% protection), the magic
shield (stops lightning, 80% against fireballs, waterspouts and
monsters), the phantom object (enemies ignore the ship), flame (stronger
weapons for a while), fireball (burns enemies in an area) and lightning
(destroys every unshielded enemy object in an area), with the original
effect animations. As in the manual, spells reach 12 or 15 cells from one
of your temples of their kind, except wind, view and flame.
Fog of war hides what you have not explored and dims what is out of your
ships' and buildings' sight, where other players' ships cannot be seen
(`--no-fog` turns it off; sight ranges are placeholders). The menus play
the original's menu music (`mus_menu` from the game's sounds, needing no
CD or SoundFont). In missions, the game CD's own music, its audio tracks 2-9 (not part of a data image),
plays if you rip them into `<data>/cdaudio` (or `--cd-audio DIR`) as WAV,
FLAC or Ogg files named by track number (`Track02.flac`, ...); otherwise
the original MIDI music plays through a General MIDI SoundFont you
provide (`--soundfont FILE.sf2` or `OPENAC_SOUNDFONT`; common Linux
locations are found by themselves). Sound effects from the original play for gunfire,
hits, splashes, wrecks, construction and spells, louder the nearer they
are to the centre of the view. The wind changes as in the original and speeds up or
slows down ships by their heading; the wind rose shows it. Computer players (a simple stand-in for the original
AI) keep their own amber and fish: their merchants gather, their shipyards
build merchants and then the strongest warships they can afford, their
warships hunt your ships within ten cells, and after five minutes they
send waves of three or more warships against your towns and fleet. Construction, research and training
times (ships are launched at once, as soon as a berth is free), and how fast merchants gather, are the original's (in game ticks of
20 ms at the default speed). Hull and building strength, ram damage, boarding pace,
spell study time, durations and damage, hazard damage, fort weapons, the unit of the rate of fire,
and building placement rules are placeholders until recovered from the
executable. The original's videos play, decoded from the CD's Cinepak and
Video 1 AVIs: the company logos and title sequence at start-up (skipped
with any key or a click, all of them with Esc, or not shown with
`--no-intro`), the story's opening before the first campaign mission and
its ending after the last. A mission's outcome shows the original's
statistics: ships destroyed, boarded and lost, buildings, monsters and
lairs destroyed, fish caught, amber earned and left, treasures found and
the total score, computed as the original does. Missions' starting research levels apply (some campaign opponents start
with better armour, rams and weapons); crews are not upgraded in play, as in
the original. Not yet implemented: the campaign scripts' story events (partly decoded),
the original's multiplayer screens (OpenAC has its own lobby), and play against the original game over the network. See [docs/FORMATS.md](docs/FORMATS.md) for what is known about
the file formats.

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
  screen (OpenAC's own network game behind it, so its one way to connect is
  TCP/IP). The screens are drawn from the original's own widget pictures
  and fonts. The mission accomplished and failed screens are OpenAC's own
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
cargo run --release -p openac
```

On macOS, `scripts/bundle-macos.sh` builds `dist/OpenAC.app`, which you can
double-click (or copy to Applications); its icon is cut from the original's
title art in your game data, when the script finds it. The game data then lives in
`~/Library/Application Support/OpenAC/gamedata` and the choice in
`~/Library/Application Support/OpenAC/config.json` (delete it to choose
again).

Or by hand:

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
`battle01`-`battle25` maps; each player needs the same build of OpenAC
and their own copy of the game data). From the title screen, MultiPlayer game:
the host picks a map and the number of players and chooses Host; the
others type the host's address (shown on the host's screen) and choose
Join; the game starts once everyone is there. Or from the command line:

```sh
openac --host --mission battle05 --players 3   # waits on port 17999
openac --join 192.168.1.20                      # or host:port
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
openac --list-missions
openac --mission battle07 --screenshot shot.png --window 2560x1080 --focus 20,12
openac-extract list gamedata/maplib.dkx
openac-extract dump gamedata/data/objects.dkx out/ --filter TARAN
openac-extract video gamedata/video/title.avi frames/ --every 25
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
