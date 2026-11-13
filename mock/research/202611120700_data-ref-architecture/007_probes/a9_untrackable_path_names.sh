#!/bin/bash
# Spike a9: is there an in-tree directory name that git itself refuses to track or to check out, so that no
# commit from any clone can ever write there? Candidates: a `.git` component, and the NTFS short-name alias
# `git~1` that core.protectNTFS (default on) refuses. For each: git add, and checkout -f of a commit built by
# plumbing (as a hostile or careless clone might push) that tracks a file there over an edited data file.
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
for N in "git~1" ".git" "GIT~1"; do
  rm -rf r; git init -q -b dev r; cd r; echo s > main.rs; git add -A; git commit -qm s
  mkdir -p "mock/$N"; echo EDIT > "mock/$N/N.md"
  a=$(git add -A 2>&1 | head -1); t=$(git ls-files -- "mock/" | tr '\n' ' ')
  B=$(echo pollute | git hash-object -w --stdin)
  T=$(GIT_INDEX_FILE=.git/x git update-index --add --cacheinfo "100644,$B,mock/$N/N.md" 2>&1 && GIT_INDEX_FILE=.git/x git write-tree 2>&1) || true
  c=$(git checkout -qf "$(git commit-tree -p HEAD -m p "$T" 2>/dev/null)" 2>&1 | head -1)
  echo "name mock/$N: add -A -> {$a} tracked=[$t]; plumbing tree -> {$(echo $T | head -c 80)}; checkout -f -> {$c}; data=$(cat mock/$N/N.md 2>/dev/null)"
  cd ..
done
echo "== second half: a tree crafted with mktree (bypassing the index), fetched into a clone with the data edited"
for N in "git~1" ".git"; do
  rm -rf r; git init -q -b dev r; cd r; echo s > main.rs; git add -A; git commit -qm s
  B=$(echo pollute | git hash-object -w --stdin); IN=$(printf '100644 blob %s\tN.md\n' $B | git mktree)
  MK=$(printf '040000 tree %s\t%s\n' $IN "$N" | git mktree); ROOT=$( (git ls-tree HEAD; printf '040000 tree %s\tmock\n' $MK) | git mktree)
  C=$(git commit-tree -p HEAD -m crafted $ROOT); mkdir -p "mock/$N"; echo EDIT > "mock/$N/N.md"
  c=$(git checkout -qf $C 2>&1 | head -1); r=$(git reset -q --hard $C 2>&1 | head -1); f=$(git fsck 2>&1 | grep -i -m1 -E 'dotgit|ntfs|git~' )
  echo "crafted mock/$N: checkout -f -> {$c}; reset --hard -> {$r}; data=$(cat mock/$N/N.md 2>/dev/null); fsck: {$f}"
  cd ..
done
