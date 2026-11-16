#!/usr/bin/env nutshell
# Runs the sketch and writes every output under out/, one file per step, each ending in the step's
# exit status. Machine paths are folded to <sketch>, <repo>, <target>, <cargo-home> and
# <rustup-home>.
#
#   CARGO_TARGET_DIR=<target> nutshell run.sh [all|lint|build]
#
# `lint` is the driver over every fixture and the diffs between them; `build` is cargo over the
# fixtures. The two are separate so the lint can be rerun without turning the build's record into a
# fresh-artifacts one (see step 30).
set -u

# Under nutshell `$0` is nutshell itself; the script's own path is `BASH_SOURCE[0]`.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
toolchain="+nightly-2026-05-28"
target="${CARGO_TARGET_DIR:?set CARGO_TARGET_DIR to the shared build directory}"
stage="${1:-all}"
out="$here/out"
mkdir -p "$out"

fold() {
    sed -e "s|$here|<sketch>|g" -e "s|$repo|<repo>|g" -e "s|$target|<target>|g" \
        -e "s|${CARGO_HOME:-$HOME/.cargo}|<cargo-home>|g" -e "s|${RUSTUP_HOME:-$HOME/.rustup}|<rustup-home>|g"
}

# step <file> <command...>: run it, keep stdout and stderr together, then the exit status.
step() {
    local file="$1"
    shift
    {
        echo "\$ ${*}"
        "$@" 2>&1
        echo "exit: $?"
    } | fold >"$out/$file"
}

step 00_toolchain.out rustc "$toolchain" --version

if [[ $stage == all || $stage == lint ]]; then
    # The driver: the lint, the v1 engine as a library, and the runner.
    step 01_driver_build.out cargo "$toolchain" build --quiet --manifest-path "$here/Cargo.toml"
    driver="$target/debug/sketch-edges-across-source-directories"

    # The lint, over each fixture.
    step 10_lint_fixture-red.out "$driver" "$here/fixture-red/mock"
    step 11_lint_fixture-green.out "$driver" "$here/fixture-green/mock"
    step 12_lint_fixture-rule-added.out "$driver" "$here/fixture-rule-added/mock"
    step 13_lint_fixture-header-form.out "$driver" "$here/fixture-header-form/mock"
    step 14_lint_fixture-empty.out "$driver" "$here/fixture-empty/mock"

    # What each control fixture changes against fixture-red.
    step 20_diff_red_green.out diff -r "$here/fixture-red" "$here/fixture-green"
    step 21_diff_red_rule-added.out diff -r "$here/fixture-red" "$here/fixture-rule-added"
    step 22_diff_red_header-form.out diff -r "$here/fixture-red" "$here/fixture-header-form"
fi

if [[ $stage == all || $stage == build ]]; then
    # The build half: one cargo workspace over the three directories, the forbidden edge included.
    #
    # Verbose, so each unit's `rustc` line shows the `--extern` that wires one directory's crate into
    # another's, and into a target directory of its own under the shared one: in the shared target
    # the fixtures' crates are found fresh from an earlier run, since cargo keys a path crate by its
    # place in its workspace and this toolchain judges freshness by content, not by mtime. A second
    # run finds this directory fresh too, and then prints `Fresh` per unit instead of the lines.
    step 30_build_fixture-red.out cargo "$toolchain" build -v --manifest-path "$here/fixture-red/mock/Cargo.toml" \
        --target-dir "$target/sketch-edges-across-source-directories-fixture-red"
    step 31_tree_fixture-red.out cargo "$toolchain" tree --manifest-path "$here/fixture-red/mock/Cargo.toml" --workspace
    step 32_build_fixture-green.out cargo "$toolchain" build --manifest-path "$here/fixture-green/mock/Cargo.toml"
    step 33_build_fixture-header-form.out cargo "$toolchain" build --manifest-path "$here/fixture-header-form/mock/Cargo.toml"
    # The header-form build answers from what fixture-red left in the shared target, so it compiles
    # nothing; the tree is what shows its cross-directory edge resolves.
    step 35_tree_fixture-header-form.out cargo "$toolchain" tree --manifest-path "$here/fixture-header-form/mock/Cargo.toml" --workspace
    step 34_metadata_members_fixture-red.out cargo "$toolchain" metadata --format-version 1 --no-deps \
        --manifest-path "$here/fixture-red/mock/Cargo.toml"
fi
