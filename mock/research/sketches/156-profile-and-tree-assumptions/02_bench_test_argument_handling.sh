#!/bin/sh
# P2. What does `mock bench test` do with an argument it does not understand?
#
# HISTORY. As first written this script asserted the DEFECT: naming an
# existing crate and naming a nonexistent one both produced output identical
# to naming nothing, meaning the argument was silently discarded. That run is
# kept beside it as 02_bench_test_argument_handling_BEFORE_FIX.out. Since the
# fix (cmd_test now filters on the argument and refuses one that matches
# nothing) the script asserts the repaired behaviour, so it is a regression
# check rather than a demonstration. This is the probe the review named as
# missed by the 5d0548c flip that converted the other three.
#
# The fixture also now carries a root Cargo.toml, matching arvo's actual
# shape and every real tree the tool hands out (a driver bin crate at the
# bench root): the original fixture omitted it, which is exactly the
# omission that let the "stop descending at the first found manifest" defect
# through everywhere else in this PR, and there is no reason this probe's
# fixture should be the one place that omission is still allowed to stand.
#
# NEGATIVE CONTROL, stated before the run. Arm 1 runs with no argument and
# must report both crates. Arm 2 names a crate that exists and must narrow
# to it alone. Arm 3 names one that does not exist at all and must refuse
# (nonzero exit), never silently run the whole tree.
set -e
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
BIN="$ROOT/target/debug/mockspace"
[ -x "$BIN" ] || { echo "build the tool first"; exit 2; }
FX=$(mktemp -d)
mkdir -p "$FX/mock/benches/support/alpha/src" "$FX/mock/benches/support/beta/src"
printf '[lints]\n' > "$FX/mock/mockspace.toml"
(cd "$FX" && git init -q . 2>/dev/null) || true
mkdir -p "$FX/mock/benches/src"
printf '[package]\nname = "driver"\nversion = "0.0.0"\nedition = "2024"\n\n[[bin]]\nname = "driver"\npath = "src/main.rs"\n' \
  > "$FX/mock/benches/Cargo.toml"
printf 'fn main() {}\n' > "$FX/mock/benches/src/main.rs"
for c in alpha beta; do
  printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2024"\n\n[lib]\npath = "src/lib.rs"\n' "$c" \
    > "$FX/mock/benches/support/$c/Cargo.toml"
  printf '#[cfg(test)]\nmod t {\n    #[test]\n    fn %s_has_a_real_test() { assert_eq!(2 + 2, 4); }\n}\n' "$c" \
    > "$FX/mock/benches/support/$c/src/lib.rs"
done

run() { (cd "$FX/mock" && "$BIN" bench test "$@" 2>&1 | { grep -E '^(ok|FAIL)|crates,' || true; }); }
run_status() {
  # A subshell's own nonzero exit, taken as a standalone statement, aborts
  # the whole script under `set -e` before `echo $?` ever runs. The if/else
  # form is the standard way to observe an exit status without tripping it.
  if (cd "$FX/mock" && "$BIN" bench test "$@" >/dev/null 2>&1); then
    echo 0
  else
    echo $?
  fi
}

echo "=== arm 1: no argument (baseline) ==="; A1=$(run); echo "$A1"
echo "=== arm 2: an argument naming a crate that EXISTS ==="; A2=$(run alpha); echo "$A2"
echo "=== arm 3: an argument naming a crate that DOES NOT EXIST ==="; A3=$(run no-such-crate-anywhere); echo "$A3"
S3=$(run_status no-such-crate-anywhere)
echo
echo "arm 3 exit status: $S3 (must be nonzero: a refusal, not a silent full run)"
echo

ok=1
if [ "$A1" = "$A2" ] || [ "$A1" = "$A3" ]; then
  echo "REGRESSION: naming a crate did not change the output; the filter is dead again."
  ok=0
fi
if ! printf '%s' "$A2" | grep -q "support/alpha/Cargo.toml"; then
  echo "REGRESSION: arm 2 did not narrow to the alpha crate it named."
  ok=0
fi
if printf '%s' "$A2" | grep -q "support/beta/Cargo.toml"; then
  echo "REGRESSION: arm 2 still ran beta, which it did not name."
  ok=0
fi
if [ "$S3" = "0" ]; then
  echo "REGRESSION: an unmatched name exited 0 instead of refusing."
  ok=0
fi

if [ "$ok" = "1" ]; then
  echo "CONTROL: ok. Arm 2 narrowed to exactly the crate it named; arm 3 refused"
  echo "         rather than running the whole tree; arm 1 differs from both."
  echo "VERDICT: the argument is a real filter and an unmatched one is a refusal,"
  echo "         not a silent pass. Before the fix (see the BEFORE_FIX output"
  echo "         beside this file) naming any crate, real or not, was identical"
  echo "         to naming nothing: the argument was read and then discarded."
else
  rm -rf "$FX"
  exit 1
fi
rm -rf "$FX"
