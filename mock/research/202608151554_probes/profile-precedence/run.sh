#!/usr/bin/env bash
# Probe: does `--config profile.release.*` (the mechanism PR #21 added at
# `src/bench.rs:317-324` and `:477-491`) override an arm crate's OWN
# `[profile.release]` table, or does the manifest win?
#
# This matters because 991 of the 1088 arm crates across the four
# consumers declare their own `[profile.release]`, and the tool now
# passes the profile on the command line "where a manifest cannot
# silently drop them" (`src/bench.rs:474-476`). If the manifest wins,
# an arm declaring a weaker profile silently defeats the tool for that
# arm alone, and nothing says so.
#
# NEGATIVE CONTROLS, stated before the run:
#   C1  arm_declares_thin, built with NO --config, MUST show thin/16.
#       Without this the probe cannot see a manifest profile at all.
#   C2  arm_declares_nothing, built WITH --config, MUST show fat/1.
#       Without this the probe cannot see --config at all, and the
#       finding below would be an artifact of the instrument.
#
# THE QUESTION: arm_declares_thin, built WITH --config fat/1.

set -u
here="$(cd "$(dirname "$0")" && pwd)"
CFG=(--config 'profile.release.opt-level=3'
     --config 'profile.release.lto="fat"'
     --config 'profile.release.codegen-units=1')

# rustc's own view: the flags cargo hands the compiler for the cdylib.
flags() {
  local dir="$1"; shift
  ( cd "$here/$dir" && rm -rf target && cargo build --release -v "$@" 2>&1 ) |
    tr ' ' '\n' |
    grep -E '^(-Clto|-Ccodegen-units|-Copt-level|lto=|codegen-units=|opt-level=)' |
    sort -u | tr '\n' ' '
  echo
}

echo "cargo: $(cargo --version)"
echo
echo "C1  thin-declaring arm, no --config   : $(flags arm_declares_thin)"
echo "C2  nothing-declaring arm, --config   : $(flags arm_declares_nothing "${CFG[@]}")"
echo "Q   thin-declaring arm, --config      : $(flags arm_declares_thin "${CFG[@]}")"
echo
echo "C1 must read thin/16, C2 must read fat/1, or Q means nothing."
