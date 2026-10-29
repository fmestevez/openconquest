---
name: port-original
description: Bring a decoded behaviour of the original Ancient Conquest into the OpenConquest Rust code, test it against the real game data, document it and ship it (commit, push, rebuild the app). Use after the behaviour has been read in the original's code (the decompile skill), for any gameplay, campaign, interface or file-format change.
---

# Port a decoded behaviour

The behaviour must already be known from the original's code (`decompile`
skill). If any part is not, go back and read it; do not fill it in.

## Where things live

- `crates/formats`: readers of the game's files (archives, maps, tile
  library, sprites, mission records).
- `crates/game/src/scene/`: the game. `mod.rs` (Scene, load, update loop,
  save/restore), `battle.rs`, `shot.rs` (combat), `spells.rs`, `magic.rs`,
  `trade.rs` (economy, building sites), `computer.rs` (the computer
  player, AIPlayerClassTy), `brain.rs` (campaign mission scripts; see the
  `campaign-brain` skill), `story.rs` (campaign judges and carry-over),
  `placement.rs`, `dig.rs`, `sleep.rs`, `volcano.rs`...
- `crates/game/src/objects.rs`: map objects, ships, frames;
  `combat.rs`: tables and formulas; `nav.rs`: the path-finder.
- `docs/FORMATS.md`: the decoded knowledge, with addresses.

## Porting

- Keep what matters to the result as the original has it: the order of
  checks, integer arithmetic, when random numbers are drawn, ticks (40 ms,
  `crate::clock::ticks(n)`), cells (64 px, `TILE_SIZE`).
- Comments say what the original does and cite the addresses, in the
  surrounding code's density and voice.
- New saved state: add the field to `SaveGame` (`crates/game/src/save.rs`)
  with `#[serde(default)]`, write it in `Scene::save` and read it in
  `Scene::restore`. Runtime state that is lost on save is a bug.
- Do not leave stand-ins: no placeholder constants, no "close enough"
  formulas. If a piece is still unknown, leave it out and say so.

## Testing

- Tests live in `crates/game/src/scene/tests.rs` and need the game data:
  they start with `let Some(root) = std::env::var_os("OPENCONQUEST_DATA")
  else { return; };`. Load a real mission (`Scene::load`), set up the case,
  run `scene.update(...)`, and assert the original's numbers.
- When a model of a routine exists (a script emulating it), compare the
  port against it over many inputs with a temporary `#[ignore]` test, then
  remove that test.
- Run: `OPENCONQUEST_DATA=$PWD/gamedata cargo test --release -q`.

## Shipping

1. `cargo fmt`; `cargo clippy --release -q --all-targets` (no warnings);
   tests pass.
2. Update `docs/FORMATS.md`.
3. Commit with a message saying what now matches the original and why,
   ending with the attribution lines the session gives. Push to `master`.
4. Rebuild the app: `./scripts/bundle-macos.sh`.
5. Never commit `gamedata/`, `dist/` or anything from the RE workspace.
6. Tell the user what changed in the game, what was tested, and what is
   still open.
