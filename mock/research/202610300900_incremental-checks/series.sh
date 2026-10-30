#!/usr/bin/env bash
# usage: series.sh <label> <worktree> <file-to-touch>
# Runs the gate five ways on one tree: full (store off), cold, warm, after a one-line edit,
# and the edited tree full again. Wall seconds land in $OUT/<label>-<step>.time.
#   OUT      directory for the .time files (default ./out)
#   ENGINE   the cargo-mock binary to time (default cargo-mock on PATH)
set -u
label=$1; tree=$2; touch_file=$3
here=$(cd "$(dirname "$0")" && pwd)
out=${OUT:-./out}; engine=${ENGINE:-cargo-mock}
mkdir -p "$out"; out=$(cd "$out" && pwd)
store="$out/verdicts-$label"
rm -rf "$store"
export MOCKSPACE_TIMINGS=1
cd "$tree" || exit 2
run() { OUT=$out "$here/timeit.sh" "$label-$1" "$tree" "$engine" --lint-only --commit; }
MOCKSPACE_NO_VERDICTS=1 run full
MOCKSPACE_VERDICTS_DIR=$store run cold
MOCKSPACE_VERDICTS_DIR=$store run warm
printf '// measuring\n' >> "$touch_file"
MOCKSPACE_VERDICTS_DIR=$store run edit
sed -i '$d' "$touch_file"
MOCKSPACE_NO_VERDICTS=1 run editfull
