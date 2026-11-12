#!/bin/bash
# Spike on seat 002's proposal C: data worktree under .git/, directory symlink mock/research -> it, excluded.
# (a) relative paths in the data that point back into the main tree, through the link (shell and cargo);
# (b) git verbs that name the path (path checkout, restore --source, apply, merge of an old branch);
# (c) what a realpath-based tool sees.
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
setup() {
cd "$R"; rm -rf m; git init -q -b dev m; cd m
mkdir -p mock/research crates/lib/src; echo old > mock/research/NOTES.md
printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' > crates/lib/Cargo.toml; echo 'pub fn f(){}' > crates/lib/src/lib.rs
git add -A; git commit -qm old; OLD=$(git rev-parse HEAD)
git checkout -q -b oldbranch; echo branchedit >> mock/research/NOTES.md; git commit -qam "old branch edit"; git checkout -q dev
git rm -rq mock/research; git commit -qm migrate
mkdir -p .git/mock-data
DC=$(git commit-tree "$OLD:mock/research" -m data); git branch mock/research $DC
git worktree add -q .git/mock-data/research mock/research
mkdir -p .git/mock-data/research/sketch/src
printf '[package]\nname="sk"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="../../../crates/lib"}\n' > .git/mock-data/research/sketch/Cargo.toml
echo 'fn main(){ lib::f(); }' > .git/mock-data/research/sketch/src/main.rs
mkdir -p mock; ln -s "$PWD/.git/mock-data/research" mock/research; echo mock/research >> .git/info/exclude
}
setup
echo "== (a1) shell: cat ../../../crates/lib/Cargo.toml from inside the link (physical .. )"
( cd mock/research/sketch && cat ../../../crates/lib/Cargo.toml >/dev/null 2>&1 && echo "resolves" || echo "BROKEN: dot-dot resolves physically, under .git/mock-data" )
echo "== (a2) cargo path dependency through the link"
( cd mock/research/sketch && cargo metadata --offline --format-version 1 >/dev/null 2>/tmp/q2cargo.err && echo "cargo ok" || { echo "cargo FAILED:"; head -3 /tmp/q2cargo.err; } )

state() { echo "   link still a link=$([ -L mock/research ] && echo yes || echo NO), data edit survives=$(grep -c EDIT .git/mock-data/research/NOTES.md), data files=$(ls .git/mock-data/research | tr '\n' ' ')"; }
case_() { setup >/dev/null 2>&1; echo EDIT >> .git/mock-data/research/NOTES.md; echo "== $1"; shift; eval "$*" 2>&1 | head -3; state; }
case_ "(b1) git checkout OLD -- mock/research/NOTES.md" git checkout \$OLD -- mock/research/NOTES.md
case_ "(b2) git restore --source=OLD --worktree -- mock/research" git restore --source=\$OLD --worktree -- mock/research
case_ "(b3) git merge oldbranch (edits a path dev removed)" git merge oldbranch
case_ "(b4) git checkout OLD (whole tree)" git checkout -q \$OLD
case_ "(b5) git checkout -f OLD" git checkout -q -f \$OLD
case_ "(b6) git reset --hard OLD" git reset -q --hard \$OLD
case_ "(b7) git stash -u" git stash -u
case_ "(b8) git cherry-pick oldbranch" git cherry-pick oldbranch
setup; echo "== (c) realpath of the link"; readlink -f mock/research
