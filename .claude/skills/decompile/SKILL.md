---
name: decompile
description: Find out exactly what the original Ancient Conquest (ancient.exe) does — a rule, number, timing, table, message or picture — by reading its decompiled code and disassembly. Use before porting or changing any game behaviour, whenever a value or rule is unknown, and whenever code or docs say "estimate", "approximat", "not decoded" or "not ported".
---

# Look it up in the original

Never fill a gap with a guess, an estimate or an approximation (see
`AGENTS.md`). Find the code.

## 1. What is already known

- `docs/FORMATS.md`: everything decoded so far, by subsystem, with
  addresses. Search it for the feature, the message number, the address.
- The Rust code's comments cite addresses (`0x4a3150`): grep the crate for
  the feature, then for its addresses.
- The workspace (`tools/re/README.md`), at `~/Library/Caches/openconquest-re`:
  - `decomp/*.c`: hundreds of functions already decompiled; each file
    starts `=== 00ADDR FUN_00ADDR`. Grep them for an address, a field offset
    (`+ 0x25b`), a message number (`0xda94`), a table (`0x5be150`).
  - `specs/*_spec.md`: earlier decodes, with their open questions.
- If the workspace is missing, run `tools/re/setup.sh`.

## 2. Find the code

- From a string (a picture, sound or text name): `tools/re/xr.py str NAME`,
  then `xr.py refs 0xADDR` for the code using it.
- From a constant or address: `xr.py refs 0xVALUE` (instructions),
  `xr.py data 0xVALUE` (dwords in the data: vtables, tables of pointers).
- From a function: `xr.py func 0xADDR` gives its start;
  `tools/re/dc.sh NAME callers:0xADDR` its callers.
- Objects are C++ classes: a class's vtable is a table of function
  pointers; message handlers switch on message numbers (0x5dxx brain
  messages, 0xdaxx magic, 0x138x orders...). Find the handler, then the
  case.

## 3. Read it

- `tools/re/dc.sh NAME 0xADDR [0xADDR...]` decompiles (about 30 s a run:
  batch the addresses). Output lands in the workspace's `decomp/`.
- Read every branch: edge cases, error returns, early exits, the order of
  checks, integer division and rounding, every random draw (they change
  the outcome and the sequence).
- Ghidra mangles some loops, pointer arithmetic and reused variables.
  Confirm strides, table bases and loop bounds in the disassembly:
  `xr.py show 0xADDR 60`.
- Tables: dump them from the exe, never retype them from memory:
  `xr.py words 0xADDR N` (shorts), `xr.py dwords 0xADDR N`, or in Python
  `sys.path.insert(0, 'tools/re'); from xr import rd`. Generate Rust
  constants from the dump.
- Trace callees until the behaviour is fully known. A function you have not
  read is an open question, not an assumption.

## 4. Write it down

- Put what you found in `docs/FORMATS.md` (behaviour, numbers, addresses),
  in the style of the sections around it.
- For a large decode, write a spec in the workspace's `specs/` first: what
  the code does, exact numbers, addresses, how to port it, and a
  "not settled" list.
- What could not be settled is written as such ("not decoded: 0x...") and
  told to the user. It is never filled with a plausible value.
