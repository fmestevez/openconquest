#!/bin/sh
# Decompiles the functions containing the given addresses into
# $OPENCONQUEST_RE/decomp/NAME.c and prints it.
#   dc.sh NAME 0x4a3150 [0x... ...]      functions at these addresses
#   dc.sh NAME callers:0x4a3150          who calls it
#   SCRIPT=VTable.java dc.sh NAME ship=0x597a00     every slot of a vtable
#   SCRIPT=DecompRange.java dc.sh NAME 0x501bb0 0x503200   a range
# Look in $OPENCONQUEST_RE/decomp first: hundreds are already there.
. "$(dirname "$0")/env.sh"
name=$1; shift
out="$OPENCONQUEST_RE/decomp/$name.c"
"$GHIDRA_HEADLESS" "$OPENCONQUEST_RE/ghidra/proj" ac -process ancient.exe -noanalysis -readOnly \
  -scriptPath "$REPO/tools/re/ghidra" -postScript "${SCRIPT:-Decomp.java}" "$out" "$@" \
  > "$OPENCONQUEST_RE/ghidra/last_run.log" 2>&1
status=$?
grep -E "ERROR|Exception" "$OPENCONQUEST_RE/ghidra/last_run.log" | head -5
[ -f "$out" ] && cat "$out"
exit $status
