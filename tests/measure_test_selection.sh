#!/usr/bin/env bash
#--------------------------------------------------------------------------------------------------
# Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
# SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
#--------------------------------------------------------------------------------------------------
#
# What `mock test` runs for a change confined to one integration-test file, and
# for one confined to the library, measured on a real consumer.
#
#   tests/measure_test_selection.sh <consumer> "<member>..." <tests/file.rs> <src/file.rs> <mockspace>...
#
# <consumer> is a git repository with a mockspace.toml, <member> a package in its
# mock workspace (several, space separated in one argument, are all run, and the
# first is the one whose files are touched), the two files are relative to that
# package's directory, and each <mockspace> is a built `mockspace` binary. Each is
# measured in turn, over the same throwaway worktree of the consumer's HEAD (so a
# build is shared between them) with the record of earlier runs cleared first.
#
# The consumer's own checkout is never edited: the worktree is made under the
# system temporary directory and removed at the end. Honours CARGO_TARGET_DIR
# and sets nothing of its own.
#
# For each binary it prints one row per step: the step, the tests and the test
# binaries nextest started, and how many binaries it left out, read from
# nextest's own line ("Starting N tests across M binaries (K binaries skipped)")
# so an engine that does not print its own counts is read the same way.
#
# Nothing here asserts. The assertions that a selection rule holds are
# `tests/mock_test_selects_what_changed.rs`; this is the measurement of what the
# rules save on a repository the fixtures only resemble.

set -u

if [ "$#" -lt 5 ]; then
    sed -n '9,18p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
fi

consumer=$1 member=$2 test_file=$3 src_file=$4
shift 4

work=$(mktemp -d "${TMPDIR:-/tmp}/measure-test-selection.XXXXXX") || exit 1
tree=$work/tree
cleanup() {
    git -C "$consumer" worktree remove --force "$tree" >/dev/null 2>&1
    rm -rf "$work"
}
trap cleanup EXIT

git -C "$consumer" worktree add --detach "$tree" HEAD >/dev/null || exit 1

first=${member%% *}
pargs=()
for m in $member; do pargs+=(-p "$m"); done
dir=$(cd "$tree" && find . -path ./target -prune -o -name Cargo.toml -print0 \
    | xargs -0 grep -l "^name = \"$first\"" 2>/dev/null | head -n 1)
dir=$tree/$(dirname "${dir#./}")
[ -f "$dir/$test_file" ] || { echo "no $test_file in $dir" >&2; exit 1; }
[ -f "$dir/$src_file" ] || { echo "no $src_file in $dir" >&2; exit 1; }

# One step: run `mock test -p <member>` and print what it started.
step() {
    local bin=$1 label=$2 out
    local t0 t1
    t0=$(date +%s.%N)
    out=$(cd "$tree" && "$bin" --dir mock test "${pargs[@]}" 2>&1)
    local status=$?
    t1=$(date +%s.%N)
    local started
    started=$(printf '%s\n' "$out" | grep -E "Starting [0-9]+ tests? across" | tail -n 1 \
        | sed -E 's/^ *Starting //')
    [ -n "$started" ] || started="nothing started ($(printf '%s\n' "$out" | grep -c 'nothing to run') settled)"
    printf '  %-34s %-52s %6.1fs%s\n' "$label" "$started" "$(echo "$t1 - $t0" | bc)" \
        "$([ "$status" -eq 0 ] || echo '  [FAILED]')"
}

touch_one() {
    printf '\n// measured\n' >>"$1"
}

printf 'member %s, tests/ file %s, library file %s\n' "$member" "$test_file" "$src_file"
for bin in "$@"; do
    printf '\n%s\n' "$bin"
    rm -rf "$tree/mock/test-history"
    cp "$dir/$test_file" "$work/test.orig"
    cp "$dir/$src_file" "$work/src.orig"

    step "$bin" "first run, nothing recorded"
    step "$bin" "nothing changed"
    touch_one "$dir/$test_file"
    step "$bin" "one tests/ file changed"
    cp "$work/test.orig" "$dir/$test_file"
    step "$bin" "that file put back"
    touch_one "$dir/$src_file"
    step "$bin" "one src/ file changed"
    cp "$work/src.orig" "$dir/$src_file"
    step "$bin" "that file put back"
done
