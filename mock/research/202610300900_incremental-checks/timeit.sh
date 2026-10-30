#!/usr/bin/env bash
# usage: timeit.sh <label> <dir> <cmd...>
# Runs the command in <dir>, writes wall seconds and exit code to <label>.time and output to <label>.out.
label=$1; dir=$2; shift 2
cd "$dir" || exit 2
start=$(date +%s.%N)
GIT_TERMINAL_PROMPT=0 "$@" > "/home/user/measure/$label.out" 2>&1
code=$?
end=$(date +%s.%N)
printf 'wall_s=%.2f exit=%s cmd=%s\n' "$(echo "$end - $start" | bc)" "$code" "$*" > "/home/user/measure/$label.time"
