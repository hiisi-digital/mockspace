#!/bin/bash
# Spike: a data dir materialised as a linked worktree of a real branch, nested at a path that
# pre-migration history tracked. What does plain git do on the way to an old commit and back?
set -u
R=${1:?scratch dir}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev main; cd main
mkdir -p mock/research; echo old > mock/research/NOTES.md; echo src > main.rs
git add -A; git commit -qm old; OLD=$(git rev-parse HEAD)
# migration: files leave dev, move to a branch
git rm -rq mock/research; git commit -qm migrate
DT=$(git rev-parse $OLD:mock/research); DC=$(git commit-tree $DT -m data)  # orphan data commit: root holds NOTES.md
git branch mock/research $DC
git worktree add -q mock/research mock/research
echo mock/research/ >> .git/info/exclude
echo "--- status after setup"; git status --short
echo edited >> mock/research/NOTES.md
echo "=== A: plain checkout OLD with dirty nested worktree"
git checkout -q $OLD 2>&1; echo "rc=$?"; echo "NOTES now: $(tr '\n' '|' < mock/research/NOTES.md)"; git checkout -q dev 2>&1
echo "=== B: checkout -f OLD"
git checkout -qf $OLD 2>&1; echo "rc=$?"; echo "NOTES now: $(tr '\n' '|' < mock/research/NOTES.md)"; ls -a mock/research
git status --short | head; git worktree list
git checkout -qf dev 2>&1; echo "back rc=$?"; echo "NOTES: $(tr '\n' '|' < mock/research/NOTES.md 2>&1)"
echo "=== C: reset --hard"
echo edited2 >> mock/research/NOTES.md 2>/dev/null
git reset -q --hard $OLD 2>&1; echo "rc=$?"; echo "NOTES: $(tr '\n' '|' < mock/research/NOTES.md 2>&1)"
git reset -q --hard dev 2>&1
echo "=== D: git clean -fdx and -ffdx on nested worktree"
mkdir -p mock/research; ls mock/research 2>&1 | head -3
git clean -fdxn 2>&1 | head
echo "=== E: git add -A / add . with nested worktree (exclude present)"
git add -A 2>&1; git status --short
echo "=== F: same without exclude"
sed -i '/mock\/research/d' .git/info/exclude; git add -A 2>&1 | head -5; git status --short; git reset -q
