# Reverse-engineering tools

Tools for reading `ancient.exe` (the original game, in `gamedata/`).

The workspace they use lives outside the repo, at
`~/Library/Caches/openconquest-re` (override with `OPENCONQUEST_RE`). It is
derived from the game's binary and **must never be committed**. It holds:

| Path | What |
|------|------|
| `ghidra/proj/` | the analysed Ghidra project |
| `ac.asm` | `objdump -d` listing, read by `xr.py` |
| `decomp/*.c` | functions already decompiled (grep here first) |
| `specs/*_spec.md` | decode notes: what a routine does, with addresses |

Ghidra refuses project paths with a dot-folder in them (`~/.cache`), hence
`~/Library/Caches`.

## Setup

`tools/re/setup.sh` builds the workspace if missing: the objdump listing and
the Ghidra project (imports and auto-analyses the exe, about 5 minutes).
Needs Ghidra (`brew install ghidra`) and `openjdk@21`.

## Tools

- `tools/re/dc.sh NAME ADDR...` decompiles the functions holding the
  addresses into `decomp/NAME.c` and prints them (about 30 s a run: batch
  addresses). `callers:ADDR` lists the callers instead. `SCRIPT=VTable.java`
  with `label=0xVTABLE` decompiles a vtable's slots; `SCRIPT=DecompRange.java`
  with `START END` decompiles every function in a range.
- `tools/re/xr.py` reads the listing and the exe's bytes: `show ADDR [N]`,
  `dis START END`, `refs VALUE`, `func ADDR`, `str TEXT`, `data VALUE`,
  `words ADDR N`, `dwords ADDR N`. Also importable from Python.

## Conventions

- VA = file offset + 0x400000 for every section of this exe.
- The game ticks every 40 ms at the default speed (`crate::clock`); map
  cells are 64 pixels (`TILE_SIZE`).
- Ghidra's output mangles some loops and pointer arithmetic: check strides,
  table bases and loop bounds in the disassembly (`xr.py show`) before
  trusting them.
- The shell is zsh: an unquoted `$VAR` holding several addresses does not
  split; use `${=VAR}`.
