#!/usr/bin/env bash
# usage: OUT=<dir> timeit.sh <label> <dir> <cmd...>
# Runs the command in <dir>; writes wall seconds and exit code to $OUT/<label>.time and the
# output to $OUT/<label>.out.
set -u
out=${OUT:-.}; label=$1; dir=$2; shift 2
cd "$dir" || exit 2
start=$(date +%s.%N)
GIT_TERMINAL_PROMPT=0 "$@" > "$out/$label.out" 2>&1
code=$?
end=$(date +%s.%N)
printf 'wall_s=%.2f exit=%s\n' "$(echo "$end - $start" | bc)" "$code" > "$out/$label.time"
