#!/usr/bin/env bash
# Runs the sketch: `mock bench run <bench>` over the consumer tree in `consumer/mock/`, with the v1 engine built from
# the checkout this sketch sits in, and keeps every output under `out/`.
#
#   run.sh <engine binary> <build directory> <attempt name> <bench>
#
# The build directory is passed as CARGO_TARGET_DIR: the harness builds its arms and driver there, and the arms build
# the fixture under `<build directory>/m4-build-from-clean/`. MOCKSPACE_NO_AUTO_ACTIVATE keeps the engine's start-up
# from changing the git configuration of the checkout. The script exits with the bench run's own exit code.
set -u

engine="$1"
build_dir="$2"
attempt="$3"
bench="$4"
here="$(cd "$(dirname "$0")" && pwd)"
mock="$here/consumer/mock"
fixture="$mock/benches/build-from-clean/fixture"
own="$build_dir/m4-build-from-clean"
out="$here/out"
mkdir -p "$out"

{
    echo "## checkout"
    git -C "$here" rev-parse HEAD
    echo "## mockspace dev on the remote"
    git ls-remote https://github.com/hiisi-digital/mockspace refs/heads/dev
    echo "## the toolchain the fixture builds with"
    env -C "$fixture" cargo -V
    env -C "$fixture" rustc -vV
    echo "## the machine"
    nproc
    uname -sr
    echo "## the fixture's target directory before the run"
    ls "$own/target/debug" 2>&1
} >"$out/$attempt-00-context.txt" 2>&1

env -C "$mock" CARGO_TARGET_DIR="$build_dir" MOCKSPACE_NO_AUTO_ACTIVATE=1 \
    "$engine" --dir "$mock" bench run "$bench" >"$out/$attempt-01-bench-run.txt" 2>&1
status=$?
echo "exit: $status" >>"$out/$attempt-01-bench-run.txt"

{
    echo "## what is under results/ after the run"
    find "$mock/benches/results" -type f | sed "s|$mock/||" | sort
    echo "## a call record left behind, which after_cell takes when it runs"
    ls "$own" 2>&1
} >"$out/$attempt-02-results.txt" 2>&1

exit "$status"
