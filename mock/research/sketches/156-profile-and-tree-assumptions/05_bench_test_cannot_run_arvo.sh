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

echo
case "$A" in
  DID*) ok_a=1 ;;
  *) ok_a=0 ;;
esac
if [ "$ok_a" -eq 1 ] && [ "$B" -lt 60 ]; then
  echo "CONTROL: ok. Serialised, the same crate finishes in ${B}s; through the tool it does not finish at all."
  echo "VERDICT: mock bench test cannot run the tree it was written for. The bug"
  echo "         that hangs is arvo's; what is mockspace's is that the command"
  echo "         offers no way to pass --test-threads=1, because flags never"
  echo "         reach it (04). 155 explicitly did not run this acceptance check."
else
  echo "CONTROL FAILED or refuted: armA='$A' armB=${B}s -- result suppressed"; exit 1
fi
rm -rf "$FX"
