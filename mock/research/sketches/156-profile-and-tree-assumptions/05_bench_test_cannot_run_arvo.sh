#!/bin/sh
# P5. `mock bench test` does not terminate on the tree it was written for, and
# the workaround is unreachable through the tool.
#
# arvo's bitpack-write-contend-shared has three stress tests that share one
# process-wide thread pool through a single-coordinator protocol. Under
# libtest's default parallelism two coordinators interleave and one spins
# forever. 154 established this (each test alone: 0.31s, 1.86s, 0.59s; the
# three together: no completion in 180s, twice). The bug is arvo's.
#
# What is mockspace's is the composition: `mock bench test` runs `cargo test`
# with default parallelism and NO way to pass `-- --test-threads=1`, because
# every flag is dropped before the subcommand sees it (see 04). So the command
# that exists to make a bench tree's tests reachable cannot run this tree, and
# the one-word workaround cannot be expressed.
#
# HISTORY. As first written this had two arms and showed the tool could not
# run the tree at all; that run is kept beside it as
# 05_bench_test_cannot_run_arvo_BEFORE_FIX.out. Arm C is added since the flag
# fix: the workaround is now expressible through the tool. Arm A still hangs,
# and is expected to, because the bug that hangs is arvo's.
#
# NEGATIVE CONTROL, stated before the run. Arm B runs the same crate with
# --test-threads=1 directly through cargo and MUST complete quickly. If arm B
# also hangs, the hang is not the parallelism and this probe says nothing
# about the tool. If arm A completes, the hang does not reproduce here and the
# finding is withdrawn.
set -e
BUDGET=${BUDGET:-300}
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
BIN="$ROOT/target/debug/mockspace"
SRC=${ARVO_ROOT:-$HOME/Dev/clause-dev/arvo}/mock/benches/variants
[ -x "$BIN" ] || { echo "build the tool first"; exit 2; }
[ -d "$SRC/bitpack-write-contend-shared" ] || { echo "skip: no arvo checkout (set ARVO_ROOT)"; exit 0; }
FX=$(mktemp -d)
mkdir -p "$FX/mock/benches/variants"
printf '[lints]\n' > "$FX/mock/mockspace.toml"
(cd "$FX" && git init -q . 2>/dev/null) || true
cp -R "$SRC/bitpack-write-contend-shared" "$SRC/bitpack-plan-shared" "$FX/mock/benches/variants/"
rm -rf "$FX/mock/benches/variants"/*/target
C="$FX/mock/benches/variants/bitpack-write-contend-shared"

echo "warming the build (not timed)..."
(cd "$C" && cargo test --no-run) >/dev/null 2>&1 || true

echo "=== arm A: mock bench test, budget ${BUDGET}s ==="
LOG="$FX/a.log"; : > "$LOG"
( cd "$FX/mock" && "$BIN" bench test > "$LOG" 2>&1; echo "EXIT=$?" >> "$LOG" ) &
i=0
while [ $i -lt $BUDGET ]; do
  grep -q "EXIT=" "$LOG" 2>/dev/null && break
  sleep 5; i=$((i+5))
done
if grep -q "EXIT=" "$LOG" 2>/dev/null; then A="completed in ${i}s"; else A="DID NOT COMPLETE in ${BUDGET}s"; fi
echo "  $A"
grep -c "has been running for over 60 seconds" "$LOG" 2>/dev/null | sed 's/^/  stalled-test notices: /'
pkill -f bench_bitpack_write_contend 2>/dev/null || true
sleep 2

echo "=== arm B (control): cargo test -- --test-threads=1 ==="
S=$(date +%s)
(cd "$C" && cargo test -- --test-threads=1) > "$FX/b.log" 2>&1 || true
B=$(( $(date +%s) - S ))
echo "  completed in ${B}s"; grep "^test result" "$FX/b.log" | head -1 | sed 's/^/  /'

echo "=== arm C: mock bench test -- --test-threads=1 (the workaround, through the tool) ==="
S=$(date +%s)
(cd "$FX/mock" && "$BIN" bench test -- --test-threads=1) > "$FX/c.log" 2>&1 || true
Cc=$(( $(date +%s) - S ))
echo "  completed in ${Cc}s"; grep "crates," "$FX/c.log" | tail -1 | sed 's/^/  /'

echo
case "$A" in
  DID*) ok_a=1 ;;
  *) ok_a=0 ;;
esac
PASSED=$(grep -o '[0-9]* passed' "$FX/c.log" | tail -1 | grep -o '[0-9]*' || echo 0)
if [ "$ok_a" -eq 1 ] && [ "$B" -lt 60 ] && [ "$Cc" -lt 60 ] && [ "$PASSED" -gt 0 ]; then
  echo "CONTROL: ok. Serialised the crate finishes in ${B}s; under the default runner it does not finish."
  echo "VERDICT: the hang is arvo's bug and is unchanged, as it should be."
  echo "         What changed is that the one-word workaround is now"
  echo "         expressible through the tool: arm C completes in ${Cc}s with"
  echo "         ${PASSED} tests passing, where before the fix every flag was"
  echo "         dropped before the subcommand and arm C behaved as arm A."
else
  echo "CONTROL FAILED or refuted: armA='$A' armB=${B}s armC=${Cc}s passed=$PASSED -- suppressed"; exit 1
fi
rm -rf "$FX"
