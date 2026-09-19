#!/usr/bin/env nutshell
# shellcheck shell=bash
# =============================================================================
# pre_push_test - what the push gate runs and what it exits
# =============================================================================
# Run: ./test tests/pre_push_test.sh
#
# The gate is two suites and the second one is the reason the script exists.
# `shook.toml` gives one argument list per hook and runs it through no shell, so
# `cargo test --workspace` was the whole gate and `./test`, the nutshell half of
# this repository, was run by nothing. An arm added under `tests/` guarded a
# class no gate would re-run.
#
# Two properties, and neither is visible in a passing push. The shell suite runs
# even when the rust side has already failed, because a push refused over one
# should still say whether the other is red as well; and the exit status is the
# first non-zero of the two rather than whichever ran last, since a gate that
# forgets the earlier failure passes a push it refused a moment ago.
#
# Both are measured against stubs rather than against the real suites. A run of
# the real ones takes minutes and can only ever produce the pair it happens to
# produce, so the failing combinations would never be covered at all.
# =============================================================================

use test

script="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/scripts/pre-push.sh"

# One run of the gate against a stubbed `cargo` and a stubbed `./test`.
#
# The tree is a throwaway holding only what the script reaches for: a `cargo` on
# PATH ahead of the real one, and a `test` beside the script's own parent, which
# is where `./test` resolves once the script has taken itself to the repository
# root. What comes back is the exit status with the stub's output under it.
gate_run() {
    local cargo_rc="$1" test_rc="$2" root bin out rc
    root="$(mktemp -d)"
    bin="$root/bin"
    mkdir -p "$bin" "$root/scripts"
    cp "$script" "$root/scripts/pre-push.sh"
    printf '#!/bin/sh\necho "the rust side ran"\nexit %s\n' "$cargo_rc" > "$bin/cargo"
    printf '#!/bin/sh\necho "the shell side ran"\nexit %s\n' "$test_rc" > "$root/test"
    chmod +x "$bin/cargo" "$root/test" "$root/scripts/pre-push.sh"
    out="$(cd "$root" && PATH="$bin:$PATH" bash scripts/pre-push.sh 2>&1)"
    rc=$?
    rm -rf "$root"
    printf '%s\n%s' "$rc" "$out"
}

gate_status() { printf '%s' "$(gate_run "$1" "$2")" | head -1; }
gate_output() { printf '%s' "$(gate_run "$1" "$2")" | tail -n +2; }

#[test]
it_passes_only_when_both_suites_pass() {
    assert_eq "$(gate_status 0 0)" "0"
}

#[test]
it_refuses_when_either_suite_fails() {
    assert_eq "$(gate_status 1 0)" "1"
    assert_eq "$(gate_status 0 1)" "1"
    assert_eq "$(gate_status 1 1)" "1"
}

#[test]
it_reports_the_first_failure_rather_than_the_last() {
    # Distinguishable codes, because two ones cannot say which of them was
    # reported. A gate keeping the later status passes nothing that should fail,
    # but it names the wrong suite to whoever reads the exit code.
    assert_eq "$(gate_status 3 5)" "3"
}

#[test]
it_runs_the_shell_suite_even_after_the_rust_side_failed() {
    # The property the single-command gate could not have. `set -e` here, or a
    # `&&` between the two, would refuse the push on the rust failure and never
    # say whether the shell suite is red too, which is a second push refused for
    # a reason that was knowable the first time.
    assert_contains "$(gate_output 1 0)" "the shell side ran"
    assert_contains "$(gate_output 1 1)" "the shell side ran"
}
