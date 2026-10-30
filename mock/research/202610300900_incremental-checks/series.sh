#!/usr/bin/env bash
# usage: series.sh <name> <worktree> <file-to-touch>
# full (store off), cold incremental, warm, one-line edit; times land in /home/user/measure/<name>-*.time
name=$1; tree=$2; touch_file=$3
export MOCKSPACE_TIMINGS=1
store=/home/user/measure/verdicts-$name
rm -rf "$store"
cd "$tree" || exit 2
MOCKSPACE_NO_VERDICTS=1 /home/user/measure/timeit.sh "$name-s-full" "$tree" /home/user/target/release/cargo-mock --lint-only --commit
MOCKSPACE_VERDICTS_DIR=$store /home/user/measure/timeit.sh "$name-s-cold" "$tree" /home/user/target/release/cargo-mock --lint-only --commit
MOCKSPACE_VERDICTS_DIR=$store /home/user/measure/timeit.sh "$name-s-warm" "$tree" /home/user/target/release/cargo-mock --lint-only --commit
printf '// measuring\n' >> "$touch_file"
MOCKSPACE_VERDICTS_DIR=$store /home/user/measure/timeit.sh "$name-s-edit" "$tree" /home/user/target/release/cargo-mock --lint-only --commit
sed -i '$d' "$touch_file"
MOCKSPACE_NO_VERDICTS=1 /home/user/measure/timeit.sh "$name-s-editfull" "$tree" /home/user/target/release/cargo-mock --lint-only --commit
