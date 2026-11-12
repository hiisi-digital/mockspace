#!/bin/bash
# Spike: repair of my seat-001 proposal after q1. Nested data worktree at a disjoint path, NOT hidden by
# exclude (so git sees an untracked directory, not expendable ignored content). Does git worktree remove
# of the parent refuse? What do add -A, status, stash -u, clean do?
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev m; cd m; echo s > main.rs; git add -A; git commit -qm s
git checkout -q --orphan mock/research; git rm -rqf .; echo a > NOTES.md; git add -A; git commit -qm data; git checkout -q dev
git worktree add -q -b feat "$R/w2" dev; cd "$R/w2"
git worktree add -q mock/data/research mock/research; echo EDIT >> mock/data/research/NOTES.md
echo "== status"; git status --short
echo "== worktree remove of the parent (from main)"; (cd "$R/m"; git worktree remove "$R/w2" 2>&1 | head -2); echo "parent exists=$([ -d $R/w2 ] && echo yes || echo no)"
echo "== git add -A"; git add -A 2>&1 | head -4; git status --short; git commit -qm "add" 2>&1 | head -1; git ls-tree HEAD mock/data/ 2>&1 | head -2
git reset -q --hard HEAD~1 2>/dev/null
echo "== stash -u"; git stash -u 2>&1 | head -2; echo "edit survives=$(grep -c EDIT mock/data/research/NOTES.md)"
echo "== clean -fd"; git clean -fd 2>&1 | head -2; echo "edit survives=$(grep -c EDIT mock/data/research/NOTES.md)"
