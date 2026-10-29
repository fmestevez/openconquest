---
name: campaign-brain
description: Work on the campaign missions' scripts (the original's mission "brains", brain\missNNN.cpp) in crates/game/src/scene/brain.rs and their judges in story.rs — fleets, member programs, events, town orders, spells, victory and defeat. Use when porting or fixing anything a campaign mission's computer players or win/loss rules do.
---

# Campaign brains

Each campaign mission (miss000-miss013) has a brain object in the original:
a C++ class with a vtable (start, victory handler +0xc, judge +8, message
handler) running "members" (fleets) through state programs. The port keeps
that shape in `crates/game/src/scene/brain.rs`; the judges and carry-over
are in `story.rs`.

## The port's model

- `mod missNNN { ... }` per mission: `FLEETS: &[FleetDef]`, `start`, and
  optionally `turn` (run for each fleet before its step: the members' own
  bookkeeping). Registered in the `mission()` table, with `silent` (members
  made at 0x8d1 that the main's switch has no case for: they never run)
  and `before` / `after` (the brain's own work on its 5-tick pass, before
  or after the members: miss007's reef, miss009/miss010's random tick,
  miss005's and miss011's flags).
- `FleetDef { group, member, cells, program, wants }`: the member's group
  (computer player) and its original member index (not always its place
  in `FLEETS`: miss002's yard is (1, 7), miss003's southern guard (0, 5),
  miss005's raiders (1, 6), (1, 7)), the cells its ships start in, its
  program, and the ship types it asks its builder for, by difficulty.
- The cadence (`run_brain`): the brain's tick is the game clock; every 5
  ticks `before`, then each member (g, m) when the tick is 5 x (g + m)
  modulo 50, then `after`. Members run with or without ships (a step that
  orders the fleet about is held while it has none: not decoded).
- A program is `fn(state: u32) -> Op`. `Op` is one of the original's
  primitives: `Behave`, `Wait`, `WaitVar`, `Watch`, `Move`, `Gather`,
  `Patrol`, `Attack`, `Build`, or `Custom(fn(&mut Scene, usize) -> u32)` for
  the member-specific code between primitives (returns the next state).
- `Event` (`Zone`, `Attacked`, `Size`, `Cell`) are the primitives' event
  lists: conditions that jump to a state. `Zone` remembers the zone
  (`Target::Seen`); `Attacked` takes the fleet's pending report of a blow
  (`fleet_attacked`, the first kept) and writes `last_area`, member
  `+0x40`, 5 x 5 about the shooter; attack, gather, found and 0xbe orders
  write `last_area` too, and `Target::Attacker` / `Target::LastArea` attack
  it.
- Fleet vars (`fleets[k].vars`) are the member's fields; brain-wide state
  lives on `Brain` (serde-default fields, saved).
- Shared helpers: `brain_cast_fleet`, `want_full_town`/`want_town_order`
  (town orders, 0x47b130), `run_brain_jobs`, `persian_bonus`,
  `brain_mended`, `wake_volcano`, `human_site_area`, `fleet_types`...
- `story.rs`: `judge_of` (each mission's win and keep terms, period),
  `judge_campaign`, `Goal`, the carry-over (`carried_on`, `arrive`,
  `bare_town`).

## Porting a member

1. Find the brain's vtable and member functions (`docs/FORMATS.md` lists
   the missions' start functions, e.g. miss005 0x454f00); decompile the member's state
   function (`decompile` skill). The member's states are a switch on its
   state field; primitives are calls (0x479xxx-0x47bxxx).
2. Write the program state by state with the same numbers: the state ids,
   coordinates (cells), tick counts, difficulty tables (`[easy, normal,
   hard]`), random rolls (`scene.rng.next()` where the original draws).
3. Keep the original's quirks (states no event reaches, flags never
   counted down): port them as they are and note them; do not "fix" them.
4. Test with the real mission: load it, run it (`scene.update` at 0.04 s a
   tick) and assert what the original does (a fort built, ships launched,
   a spell cast) — `diag` tests with `#[ignore]` are fine while working.
5. Update the mission's notes in `docs/FORMATS.md` and remove "Not
   ported" lines in `brain.rs` that are no longer true.
