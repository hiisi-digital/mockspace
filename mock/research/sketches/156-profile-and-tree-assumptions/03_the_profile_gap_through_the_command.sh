#!/bin/sh
# P3. What the profile is worth, measured on a real crate, through the command.
#
# HISTORY. As first run, `mock bench test --release` never passed --release to
# cargo (see 04), so arm 2 came back at arm 1's time. That run is kept beside
# this as 03_..._BEFORE_FIX.out: 127s / 135s / 5s. Since the flag fix arm 2
# should track arm 3, and this script asserts that.
#
# Three arms on one real crate (arvo's wide-rung-shared, 30
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
# crate then the profile does not matter here and there is nothing to measure,
# whatever the tool does with the flag. That ratio is also what keeps the arm-2
# assertion from being vacuous: arm 2 tracking arm 3 means something only
# because arm 1 is far away from both.
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
  echo "CONTROL FAILED: release is not materially faster here -- nothing to measure"; exit 1
fi
LIMIT=$(( E3 * 4 + 10 ))
echo "CONTROL arm 2 tracks arm 3: ${E2}s against a ceiling of ${LIMIT}s"
echo
if [ "$E2" -le "$LIMIT" ]; then
  echo "VERDICT: asking the command for release now gets release. Before the fix"
  echo "         arm 2 measured 135s against arm 3's 5s, because the flag was"
  echo "         dropped before the subcommand saw it and the run reported a"
  echo "         pass regardless. The summary line now also names the profile,"
  echo "         so a reader of the count can tell which one produced it."
else
  echo "REGRESSION: arm 2 took ${E2}s, not tracking arm 3's ${E3}s. The profile"
  echo "            flag is not reaching cargo again."
  exit 1
fi
rm -rf "$FX"
