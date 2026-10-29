#!/bin/sh
# Builds the reverse-engineering workspace if it is missing: the Ghidra
# project (auto-analysis takes about 5 minutes) and the objdump listing
# xr.py reads.
. "$(dirname "$0")/env.sh"
mkdir -p "$OPENCONQUEST_RE/ghidra/proj" "$OPENCONQUEST_RE/decomp" "$OPENCONQUEST_RE/specs"
if [ ! -f "$EXE" ]; then
  echo "No game executable at $EXE (copy the original's ancient.exe to gamedata/)" >&2
  exit 1
fi
if [ ! -f "$OPENCONQUEST_RE/ac.asm" ]; then
  objdump -d --x86-asm-syntax=intel "$EXE" > "$OPENCONQUEST_RE/ac.asm"
fi
if [ ! -d "$OPENCONQUEST_RE/ghidra/proj/ac.rep" ]; then
  "$GHIDRA_HEADLESS" "$OPENCONQUEST_RE/ghidra/proj" ac -import "$EXE" \
    > "$OPENCONQUEST_RE/ghidra/import.log" 2>&1
fi
echo "Workspace ready: $OPENCONQUEST_RE"
