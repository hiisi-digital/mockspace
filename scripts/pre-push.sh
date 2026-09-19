#!/usr/bin/env bash
# =============================================================================
# mockspace/scripts/pre-push.sh - what this repository runs before a push
# =============================================================================
# This repository is two halves and the gate was only running one of them.
# `cargo test --workspace` covers the rust side; `./test` covers the nutshell
# side, every `#[test]` function under `tests/*.sh`, and nothing was running it,
# so an arm added there guarded a class no gate would re-run.
#
# It exists as a file because `shook.toml` gives one argument list per hook and
# runs it without a shell, which is what keeps a manifest readable in a diff.
# Two commands therefore need something to run them, and this is it.
#
# Both halves run even when the first one fails, because a push refused over a
# rust failure should still say whether the shell suite is red as well. The exit
# status is the first non-zero of the two.
# =============================================================================
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2

status=0

cargo test --workspace || status=$?

./test
rc=$?
[[ $status -eq 0 ]] && status=$rc

exit "$status"
