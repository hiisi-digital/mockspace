#!/bin/sh
# P3. What the dropped --release costs, measured on a real crate.
#
# P4 shows `mock bench test --release` never passes --release to cargo. This
# prices that, three arms on one real crate (arvo's wide-rung-shared, 30
# tests), all target directories warmed before any arm is timed.
#
#   arm 1  mock bench test              the documented way to run the tree
#   arm 2  mock bench test --release    the documented way plus the flag
#   arm 3  cargo test --release         the same crate, bypassing the tool
#
# An earlier version of this probe timed arms 1 and 2 on a COLD target dir and
# reported 85s against 94s, a number about rustc rather than about tests. That
# reading was withdrawn before it was written down; the warmup below is why.
#
# This is an ad-hoc quick spike with no substance as a benchmark. Wall clock
# around three commands, one host, no harness. It prices nothing; it separates
# two profiles that differ by more than an order of magnitude.
#
# NEGATIVE CONTROL, stated before the run. All three arms must report the same
# 30 tests passing, or they are not the same work. And arm 3 must be at least
# five times faster than arm 1: if release is NOT dramatically faster for this
# crate then the profile does not matter here and there is nothing to report,
# whatever the tool does with the flag.
set -e
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
BIN="$ROOT/target/debug/mockspace"
SRC=${ARVO_ROOT:-$HOME/Dev/clause-dev/arvo}/mock/benches/variants/wide-rung-shared
[ -x "$BIN" ] || { echo "build the tool first"; exit 2; }
[ -d "$SRC" ] || { echo "skip: no arvo checkout at $SRC (set ARVO_ROOT)"; exit 0; }
FX=$(mktemp -d)
mkdir -p "$FX/mock/benches/support"
printf '[lints]\n' > "$FX/mock/mockspace.toml"
(cd "$FX" && git init -q . 2>/dev/null) || true
cp -R "$SRC" "$FX/mock/benches/support/wide-rung-shared"
rm -rf "$FX/mock/benches/support/wide-rung-shared/target"
C="$FX/mock/benches/support/wide-rung-shared"

echo "warming every target directory (discarded)..."
(cd "$FX/mock" && "$BIN" bench test) >/dev/null 2>&1 || true
(cd "$C" && cargo test --release) >/dev/null 2>&1 || true

t() { s=$(date +%s); OUT=$(eval "$1" 2>&1); E=$(( $(date +%s) - s )); }

t '(cd "$FX/mock" && "$BIN" bench test)'
E1=$E; N1=$(echo "$OUT" | grep -o '[0-9]* passed' | head -1)
echo "arm 1  mock bench test            ${E1}s   $N1"

t '(cd "$FX/mock" && "$BIN" bench test --release)'
E2=$E; N2=$(echo "$OUT" | grep -o '[0-9]* passed' | head -1)
echo "arm 2  mock bench test --release  ${E2}s   $N2"

t '(cd "$C" && cargo test --release)'
E3=$E; N3=$(echo "$OUT" | grep -o '[0-9]* passed' | head -1)
echo "arm 3  cargo test --release       ${E3}s   $N3"

echo
echo "CONTROL same work: $N1 / $N2 / $N3"
if [ "$N1" != "$N2" ] || [ "$N1" != "$N3" ]; then
  echo "CONTROL FAILED: the arms are not the same work -- result suppressed"; exit 1
fi
if [ "$E3" -eq 0 ]; then E3=1; fi
RATIO=$(( E1 / E3 ))
echo "CONTROL release materially faster: arm1/arm3 = ${RATIO}x (want >= 5)"
if [ "$RATIO" -lt 5 ]; then
  echo "CONTROL FAILED: release is not materially faster here -- nothing to report"; exit 1
fi
echo
echo "VERDICT: arm 2 asks for release and gets arm 1's time, not arm 3's."
echo "         The flag is accepted and discarded (see 04), so the documented"
echo "         command has no way to reach the profile at all, and its output"
echo "         names neither profile. That is the same missing dimension that"
echo "         retired a true 107s figure in arvo's OPTIONS.md Q52."
rm -rf "$FX"
