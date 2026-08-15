#!/usr/bin/env bash
# The path dependency is RELATIVE, so this compiles against whichever
# branch the repository is checked out at. `tree.rs` exists only on
# `feat/bench-consolidation` (PR #21), so a run on any other branch
# fails to compile and says nothing about the finding. The tree
# identity is therefore printed first and is part of the result.
set -u
cd "$(dirname "$0")"
echo "tree:   $(git rev-parse --abbrev-ref HEAD) @ $(git rev-parse --short HEAD)"
echo "rustc:  $(rustc --version)"
echo
cargo run -q
