#!/bin/bash
# Op's question: could the instances live in a platform state dir ($XDG_STATE_HOME) with commands in the git
# tree associating them? A fake XDG_STATE_HOME inside the scratch. Spike, git 2.43.0, cargo offline.
source "$(dirname "$0")/guard.sh"; scratch s1
export XDG_STATE_HOME="$R/state" CARGO_TARGET_DIR=/home/user/target
g init -q -b dev m; cd m; mkdir -p crates/lib/src .mock mock/research
printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' > crates/lib/Cargo.toml; echo 'pub fn f(){}' > crates/lib/src/lib.rs
echo old > mock/research/N.md; g add -A; g commit -qm old; OLD=$(g rev-parse HEAD); g rm -rqf mock/research; g commit -qm migrate
g checkout -q --orphan data; g rm -rqf .; mkdir -p sketch/src; echo old > N.md
printf '[package]\nname="sk"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="../../../crates/lib"}\n' > sketch/Cargo.toml
echo 'fn main(){ lib::f(); }' > sketch/src/main.rs; g add -A; g commit -qm data; g checkout -q dev
ID=$(printf '%s' "$PWD" | sha1sum | cut -c1-12); ST="$XDG_STATE_HOME/mockspace/$ID/research"; mkdir -p "$(dirname "$ST")"
g worktree add -q "$ST" data; echo EDIT >> "$ST/N.md"
echo "== (a) verbs run in the git tree cannot reach it"
for v in "clean -ffdx" "checkout -qf $OLD" "reset -q --hard $OLD" "stash -u" "checkout -q dev"; do g $v >/dev/null 2>&1; echo "git $v -> edit survives=$(grep -c EDIT "$ST/N.md")"; done
echo "== (b) relative paths out of the data: at the state path, and through a link at .mock/research"
( cd "$ST/sketch" && timeout 60 cargo metadata --offline --format-version 1 >/dev/null 2>&1 && echo "state path: cargo OK" || echo "state path: cargo FAILED" )
mkdir -p .mock; ln -s "$ST" .mock/research
( cd .mock/research/sketch && timeout 60 cargo metadata --offline --format-version 1 >/dev/null 2>&1 && echo "link .mock/research: cargo OK" || echo "link .mock/research: cargo FAILED" )
echo "== (c) the clone is deleted: what is left at the state dir"
cd "$R"; rm -r -- "$R/m"
echo "files still there: $(ls "$ST" | tr '\n' ' ') edit bytes: $(grep -c EDIT "$ST/N.md")"
( cd "$ST" && timeout 20 git status 2>&1 | head -1 | cut -c1-100 )
echo "(the data's own commits lived in the deleted object database; only the bytes on disk survive)"
