#!/usr/bin/env nutshell
# shellcheck shell=bash
# =============================================================================
# rust_e2e_test - the ignored cdylib e2e suites, run for real
# =============================================================================
# `custom_lint_cdylib.rs` and `tool_cdylib.rs` are `#[ignore]`d: each test does
# a real `cargo build` of a temporary project's cdylib, in release, and that
# is too slow for the default `cargo test` loop. Ignored is not the same as
# "never runs", and this repository already found out the hard way: the one
# test in `custom_lint_cdylib.rs` sat broken for weeks, dead since the commit
# that split `Lint` into `Lint` + `CrateLint`, because nothing ever passed
# `-- --ignored` and nobody noticed a green default `cargo test` was silent
# about it.
#
# `./test` is the one command this repository's own workflow expects a human
# to run before pushing. This file is what makes that command actually
# exercise the ignored suites, so they stay reachable by something that
# actually runs rather than by a flag someone has to remember.
# =============================================================================

use test

#[test]
it_runs_the_ignored_custom_lint_cdylib_suite() {
    assert_ok cargo test -p mockspace --test custom_lint_cdylib -- --ignored
}

#[test]
it_runs_the_ignored_tool_cdylib_suite() {
    assert_ok cargo test -p mockspace --test tool_cdylib -- --ignored
}
