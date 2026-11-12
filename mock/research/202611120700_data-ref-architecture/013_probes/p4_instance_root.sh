#!/bin/bash
# 013/p4: where can the instance root sit so research crates' ../ paths still resolve, and what is the refused name worth there?
# Layout: <git tree>/<root>/<ns>/... is a nested data worktree; a research crate at <root>/research/sketch has path = "../../../crates/lib"
# (authored when it lived at mock/research/sketch). Roots tried: git~1 (refused name), .mockroot (ordinary), deep/root (two components).
set -u
S=${1:?scratch}; rm -rf $S/p4; mkdir -p $S/p4 && cd $S/p4
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a CARGO_TARGET_DIR=$S/p4/target CARGO_NET_OFFLINE=true
one() { local ROOT=$1; local dir=m_$(echo "$ROOT" | tr -c 'a-z0-9' _)
  rm -rf $dir; git init -q -b dev $dir; cd $dir; mkdir -p crates/lib/src; printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' > crates/lib/Cargo.toml; echo 'pub fn f()->u8{1}' > crates/lib/src/lib.rs
  git add -A; git commit -qm i; echo "$ROOT/" >> .git/info/exclude
  mkdir -p "$(dirname "$ROOT")"; git worktree add -q --orphan -b mock/research "$ROOT/research" 2>/dev/null || { echo "[$ROOT] worktree add FAILED"; cd ..; return; }
  ( cd "$ROOT/research"; mkdir -p sketch/src; printf '[package]\nname="sk"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="../../../crates/lib"}\n[workspace]\n' > sketch/Cargo.toml; echo 'fn main(){}' > sketch/src/main.rs; git add -A; git commit -qm d )
  local cargo_ok=$( (cd "$ROOT/research/sketch" && cargo metadata --format-version 1 >/dev/null 2>&1) && echo resolves || echo BREAKS )
  local addA=$(git add -A 2>&1 | head -1); [ -z "$addA" ] && addA="ok (nothing staged: $(git diff --cached --name-only | wc -l))"
  local B=$(printf x | git hash-object -w --stdin); local IDX=$(mktemp -u); GIT_INDEX_FILE=$IDX git read-tree dev
  local addf=$(git add -f "$ROOT/research/sketch/Cargo.toml" 2>&1 | head -1); local nst=$(git diff --cached --name-only | grep -c .); addf="${addf:-no message}; staged=$nst"
  git reset -q 2>/dev/null; git rm -q --cached -r "$ROOT" 2>/dev/null
  local ui=$(GIT_INDEX_FILE=$IDX git update-index --add --cacheinfo 100644,$B,"$ROOT/research/N.md" 2>&1 | head -1); [ -z "$ui" ] && ui="ACCEPTED"
  printf '[%-12s] cargo from inside the instance: %-8s | git add -A: %s | git add -f of a file under it: %-60.60s | update-index: %.40s\n' "$ROOT" "$cargo_ok" "$addA" "$addf" "$ui"
  cd ..; }
one 'git~1'; one '.mockroot'; one 'deep/root'; one 'GIT~1'; one '.git ' 
echo "== a commit that DOES carry the refused path (crafted tree, as a hostile or careless clone could push): can the main tree check it out?"
cd m_git_1 2>/dev/null || cd m_git_1_; 
SUB=$(printf '100644 f\0' | { cat; printf "$(printf x | git hash-object -w --stdin | sed 's/../\\x&/g')"; } | git hash-object -t tree -w --stdin --literally); TOP=$(printf '040000 git~1\0' | { cat; printf "$(echo $SUB | sed 's/../\\x&/g')"; } | git hash-object -t tree -w --stdin --literally)
C=$(git commit-tree $TOP -m poisoned); git branch -q poisoned $C; git checkout -q poisoned 2>&1 | head -1; echo "checkout of the poisoned commit rc=${PIPESTATUS[0]} (an error is the protection; the data worktree untouched: $(ls git~1/research | tr '\n' ' '))"
echo "== name collisions of the logical address forms"
cd ..; rm -rf n; git init -q n; cd n; echo c > c; git add c; git commit -qm i
git ls-remote mock://x/y 2>&1 | head -2
mkdir -p '@/crates/foo'; echo t > '@/crates/foo/README.md.tmpl'; git add -f '@/crates/foo/README.md.tmpl' && echo "'@' directory tracked: ok"; git commit -qm at
echo "-- ambiguity of a path component named @"; git log --oneline @ 2>&1 | head -1; git diff --stat @ 2>&1 | head -1; git log --oneline -- @ | head -1
