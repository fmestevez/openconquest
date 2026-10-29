# Shared settings for the reverse-engineering tools. Source it.
# The workspace (Ghidra project, disassembly, decompiled functions, decode
# specs) lives outside the repo: it is derived from the game's binary and
# must never be committed.
REPO=$(cd "$(dirname "$0")/../.." 2>/dev/null && pwd)
: "${OPENCONQUEST_RE:=$HOME/Library/Caches/openconquest-re}"
: "${GHIDRA_HEADLESS:=$(ls -d /opt/homebrew/Cellar/ghidra/*/libexec/support/analyzeHeadless 2>/dev/null | tail -1)}"
: "${JAVA_HOME:=/opt/homebrew/opt/openjdk@21}"
EXE=${OPENCONQUEST_EXE:-$REPO/gamedata/ancient.exe}
export OPENCONQUEST_RE GHIDRA_HEADLESS JAVA_HOME EXE REPO
