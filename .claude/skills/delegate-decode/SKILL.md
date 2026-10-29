---
name: delegate-decode
description: Hand a reverse-engineering question about the original Ancient Conquest to a subagent so it runs in parallel, and get back an exact, portable spec instead of a guess. Use when a decode needs many decompiles (more than a handful of functions) or when several independent behaviours need decoding at once.
---

# Delegate a decode

A decode agent knows only its brief. Write it so the result is exact and
portable.

## The brief

1. **Rules**: "Do not modify anything in the repo. Write only in the RE
   workspace (`~/Library/Caches/openconquest-re`, `specs/`). Do not guess,
   estimate or approximate: every claim comes from code you read; anything
   you could not settle goes in a 'Not settled' list."
2. **Tools**: point to `tools/re/README.md` (`dc.sh`, `xr.py`), the
   decompiled corpus `decomp/` to grep first, and `docs/FORMATS.md`.
3. **Background**: the addresses and earlier specs already known (name the
   spec files), and what the port does now (name the Rust files and
   functions).
4. **The questions**, numbered and narrow: what triggers it, the exact
   formula and tables, every branch (edge cases, errors, random draws), the
   timings in ticks, what it changes, what saves it.
5. **The comparison**: "Compare with the port (files) and say exactly what
   to change: functions, fields (serde-default), tables, tests."
6. **Deliverable**: `specs/NAME_spec.md`; the final message gives the path,
   a summary per question, and what could not be settled.

## When the report comes back

- Treat it as a lead, not a source. Before porting a surprising claim (a
  shield that does not protect, a routine that "never" does something),
  open the decompiled function yourself and confirm it.
- Port from the decompiled code; a model script the agent wrote is a
  check to compare against, not the source.
- Relay to the user what the decode found and what stays open.
