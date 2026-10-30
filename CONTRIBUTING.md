# Contributing to OpenConquest

Thank you for helping. OpenConquest reproduces *Ancient Conquest* (1999)
exactly: every rule, number, timing, picture and sound as the original
`ancient.exe` has them. **A difference from the original is a bug**, and
that one rule shapes how changes are made here.

By taking part you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Decompile, never guess

Every behaviour comes from the original's code, read in a decompiler or the
disassembly. Please do not send:

- approximations: "about", "roughly", a simpler rule that "behaves the same
  in practice";
- estimates: a speed, delay, chance, damage figure or distance picked
  because it looks right;
- guesses: what a function "probably" does, or behaviour inferred from a
  name, a string, a briefing or how the game looks.

If the answer is not known yet, find it in the code. If it cannot be
settled, leave the behaviour out (or unchanged), say so in a comment
("not decoded: ...") naming the addresses still to read, and note it in
[docs/FORMATS.md](docs/FORMATS.md). Watching the original run is a good
check, but it is not a source: port from the code.

[docs/FORMATS.md](docs/FORMATS.md) is where everything decoded so far is
written down, with the original's addresses; start there.

## Making a change

1. Find the original's code for the behaviour: the functions, their
   addresses and the tables they read. Dump tables from the executable
   rather than retyping them.
2. Read all of it, including edge cases, early exits, random draws and the
   order of checks.
3. Port it, keeping the original's structure where it matters to the
   result (order of checks, integer arithmetic and rounding, when random
   numbers are drawn). Cite the addresses in comments, in the style of the
   code around it.
4. Test it against the real game data, asserting the original's numbers.
5. Document it in `docs/FORMATS.md`, with addresses.

Code that was written as an estimate is fixed the same way when touched;
search for "estimate", "approximat", "not decoded" and "not ported".

## Game data

**Never commit game data or anything derived from it**: the installed data
(`gamedata/`), extracted pictures or sounds, built bundles (`dist/`), and
decompiled code, disassembly listings or decoding notes. The repository
holds only OpenConquest's own code and documentation; players use their own
copy of the CD.

## Before sending a pull request

```sh
cargo fmt
cargo clippy --release -q --all-targets   # no warnings
OPENCONQUEST_DATA=$PWD/gamedata cargo test --release -q
```

CI runs the same checks on Linux, Windows and macOS, but it has no game
data, so the tests that read it are skipped there: run them locally with
your own data.

In the pull request, say which part of the original the change follows
(the addresses), what you tested and anything you left out because it is
not decoded yet.

## License

OpenConquest is licensed under the GNU General Public License, version 3 or
any later version ([LICENSE](LICENSE)). By contributing you agree that your
contributions are licensed the same way.
