#!/usr/bin/env bash
# `glob_match` is PR #21 (`tree.rs`), so this only compiles on
# `feat/bench-consolidation`. The path dep is relative; the tree
# identity is printed first and is part of the result.
set -u
cd "$(dirname "$0")"
echo "tree:   $(git rev-parse --abbrev-ref HEAD) @ $(git rev-parse --short HEAD)"
echo "rustc:  $(rustc --version)"
echo
cargo run -q
