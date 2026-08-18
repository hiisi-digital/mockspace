#!/bin/sh
# P2. What does `mock bench test` do with an argument it does not understand?
#
# cmd_test keeps only args starting with `--` (src/bench.rs, the `extra`
# binding) and forwards those to cargo. Every other argument is dropped with
# no message. The brief names, as a KNOWN and UNFIXED defect of this class,
# that "the driver ignores a bench name argument".
#
# NEGATIVE CONTROL, stated before the run. Arm 1 runs with no argument and
# must report both crates. Arm 2 names a crate that exists. Arm 3 names one
# that does not exist at all. If arm 2 and arm 3 both produce the SAME output
# as arm 1, the argument is not a filter and not validated: it is discarded.
# If arm 2 narrows the run, the argument IS a filter and this probe refutes
# the finding. If arm 3 refuses, the argument is validated and there is no
# finding either.
set -e
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
BIN="$ROOT/target/debug/mockspace"
[ -x "$BIN" ] || { echo "build the tool first"; exit 2; }
FX=$(mktemp -d)
mkdir -p "$FX/mock/benches/support/alpha/src" "$FX/mock/benches/support/beta/src"
printf '[lints]\n' > "$FX/mock/mockspace.toml"
(cd "$FX" && git init -q . 2>/dev/null) || true
for c in alpha beta; do
  printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2024"\n\n[lib]\npath = "src/lib.rs"\n' "$c" \
    > "$FX/mock/benches/support/$c/Cargo.toml"
  printf '#[cfg(test)]\nmod t {\n    #[test]\n    fn %s_has_a_real_test() { assert_eq!(2 + 2, 4); }\n}\n' "$c" \
    > "$FX/mock/benches/support/$c/src/lib.rs"
done

run() { (cd "$FX/mock" && "$BIN" bench test "$@" 2>&1 | grep -E '^(ok|FAIL)|crates,' ); }

echo "=== arm 1: no argument (baseline) ==="; A1=$(run); echo "$A1"
echo "=== arm 2: an argument naming a crate that EXISTS ==="; A2=$(run alpha); echo "$A2"
echo "=== arm 3: an argument naming a crate that DOES NOT EXIST ==="; A3=$(run no-such-crate-anywhere); echo "$A3"
echo
if [ "$A1" = "$A2" ] && [ "$A1" = "$A3" ]; then
  echo "CONTROL: ok. Naming an existing crate and naming a nonexistent one both"
  echo "         produce output identical to naming nothing."
  echo "VERDICT: the argument is neither a filter nor validated. It is discarded"
  echo "         silently, and a typo or a stale crate name reports a clean pass"
  echo "         over the whole tree. Same class as the known-unfixed"
  echo "         'the driver ignores a bench name argument'."
else
  echo "CONTROL FAILED or refuted: the arms differ, so the argument does something."
  echo "arm1: $A1"; echo "arm2: $A2"; echo "arm3: $A3"; exit 1
fi
rm -rf "$FX"
