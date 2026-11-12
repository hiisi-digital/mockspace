#!/bin/bash
# Spike: where may the data worktree live if the data holds a cargo manifest with a path dependency that
# climbs back to the repo root (13 of 24 manifests under this repo's mock/research do). Three placements,
# one sketch each at <data>/sketch with lib = { path = "../../../crates/lib" } sized for a data dir that is
# two levels below the root (mock/<dir>), the depth the data has today.
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev m; cd m; mkdir -p crates/lib/src mock
printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' > crates/lib/Cargo.toml; echo 'pub fn f(){}' > crates/lib/src/lib.rs
git add -A; git commit -qm s
git checkout -q --orphan mock/research; git rm -rqf .; mkdir -p sketch/src
printf '[package]\nname="sk"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="../../../crates/lib"}\n' > sketch/Cargo.toml
echo 'fn main(){ lib::f(); }' > sketch/src/main.rs; git add -A; git commit -qm data; git checkout -q dev
try() { ( cd "$1/sketch" && cargo metadata --offline --format-version 1 >/dev/null 2>/tmp/r1.err && echo "$2: cargo OK" || echo "$2: cargo FAILED: $(grep -m1 'unable to update\|No such file' /tmp/r1.err | sed 's#/tmp/[^ ]*scratchpad/[^/]*/[^/]*/##' | cut -c1-110)" ); }
git worktree add -q mock/rdata mock/research;                      try mock/rdata "in the tree, same depth (mock/rdata), nested worktree"
git branch b2 mock/research; git worktree add -q .git/mock-data/research b2; try .git/mock-data/research "common dir, no symlink (the resolver names the path)"
ln -s "$PWD/.git/mock-data/research" mock/rlink;                   try mock/rlink "common dir + symlink at mock/rlink"
git worktree add -q -b b3 mock/deep/er/rdata mock/research 2>&1 | head -1; try mock/deep/er/rdata "in the tree, deeper (mock/deep/er/rdata)"
