#!/usr/bin/env python3
"""Print already-decompiled functions from the workspace's decomp/*.c.

Usage: tools/re/fn.py 0xADDR [0xADDR...]

Each decomp file holds functions headed `=== 00ADDR FUN_00ADDR`. This prints
just the function starting at each address (the first copy found), so an
agent need not open whole files or run Ghidra again. Addresses not found are
listed at the end: decompile those with dc.sh.
"""

import os
import sys
from pathlib import Path

root = Path(os.environ.get("OPENCONQUEST_RE", Path.home() / "Library/Caches/openconquest-re"))
wanted = {int(a, 16): a for a in sys.argv[1:]}
found = set()
for path in sorted((root / "decomp").glob("*.c")):
    if len(found) == len(wanted):
        break
    lines = path.read_text(errors="replace").split("\n")
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("=== "):
            try:
                addr = int(line.split()[1], 16)
            except (IndexError, ValueError):
                addr = None
            if addr in wanted and addr not in found:
                j = i + 1
                while j < len(lines) and not lines[j].startswith("=== "):
                    j += 1
                print(f"--- {path.name}")
                print("\n".join(lines[i:j]).rstrip())
                found.add(addr)
                i = j
                continue
        i += 1
missing = [wanted[a] for a in wanted if a not in found]
if missing:
    print("not decompiled yet: " + " ".join(missing), file=sys.stderr)
