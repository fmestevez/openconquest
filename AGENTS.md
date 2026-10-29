# Working on OpenConquest

OpenConquest is a reimplementation of **Ancient Conquest** (1999). The goal is
the original game reproduced exactly: every rule, number, timing, picture and
sound as `ancient.exe` has it. **A divergence from the original is a bug.**

## The rule: decompile, never guess

Every behaviour ported comes from the original's code, read in the
decompiler or the disassembly. Do not write:

- **approximations**: "about", "roughly", a simpler rule that "behaves the
  same in practice";
- **estimations**: a number picked because it looks or feels right (a
  speed, a delay, a chance, a damage figure, a distance);
- **guesses**: what a function "probably" does, what a field "likely"
  means, behaviour inferred from a name, a string, a briefing or how the
  game looks.

When the answer is not known yet, **find it in the code** (see the
`decompile` skill) instead of filling the gap. If it truly cannot be
settled now:

1. do not invent a stand-in; leave the behaviour out, or keep what is
   there unchanged;
2. say so plainly in the code comment ("not decoded: ...") and in
   `docs/FORMATS.md`, naming the addresses still to read;
3. tell the user, and add it to the open items.

Never present an inferred or modelled result as the original's. A model
(a script emulating a routine) is a check, not a source: port from the
decompiled code itself.

## How a change is made

1. **Find the original's code** for the behaviour: the function(s), their
   addresses, the tables they read (dump tables from the exe; do not
   retype them from memory). Start from `docs/FORMATS.md` and the
   decompiled corpus in the workspace (`tools/re/README.md`).
2. **Read all of it**, including the cases that seem not to matter: edge
   handling, error returns, early exits, random draws, the order of
   checks. These are where ports go wrong.
3. **Port it**, keeping the original's structure where it matters to the
   result (order of checks, integer arithmetic and rounding, when random
   numbers are drawn). Cite the addresses in comments, in the surrounding
   code's style.
4. **Test it** against the real game data (`OPENCONQUEST_DATA`), asserting
   the original's numbers.
5. **Record it** in `docs/FORMATS.md`: what the code does, with addresses.

Already-ported code that was written as an estimate must be fixed the same
way when touched. Search for "estimate", "approximat", "not decoded",
"not ported" to find them.

## Ground rules

- **Never commit game data or anything derived from it**: `gamedata/`,
  `dist/`, the decompiled code, disassembly listings and decode notes
  (they live in `~/Library/Caches/openconquest-re`, outside the repo).
- Checks before a commit: `cargo fmt`, `cargo clippy --release -q
  --all-targets` with no warnings, and `OPENCONQUEST_DATA=$PWD/gamedata
  cargo test --release -q` passing.
- After a change the user will play, rebuild the app:
  `./scripts/bundle-macos.sh` (`dist/OpenConquest.app`).
- Code reads like the code around it: its comment density, naming and
  idioms. Comments say what the original does and where (addresses).
