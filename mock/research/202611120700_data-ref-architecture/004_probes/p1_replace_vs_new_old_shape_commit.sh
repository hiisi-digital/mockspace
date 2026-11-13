#!/bin/bash
# 004/p1: attack on 001 reading B (git replace views of pre-migration commits, 001_probes/p7).
# A replace covers the commits that existed when it was made. A branch cut before the migration keeps getting
# NEW commits in the old shape (they track the data path). Does checkout of such a tip collide again?
set -u
S=${1:?scratch}; rm -rf $S/q1; mkdir -p $S/q1 && cd $S/q1
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q -b dev r && cd r
mkdir -p mock/research && echo old > mock/research/N.md && echo code > c.rs && git add -A && git commit -qm pre
git branch feat; OLD=$(git rev-parse HEAD)
git rm -rq --cached mock/research && git commit -qm migrate; echo mock/research >> .git/info/exclude
# replace OLD by a commit with the data path removed from its tree
T=$(git ls-tree OLD^{tree} 2>/dev/null; true)
git read-tree --empty; git read-tree $OLD; git rm -rq --cached mock/research; TREE=$(git write-tree); git read-tree HEAD
REP=$(git commit-tree $TREE -m rep); git replace -f $OLD $REP
echo EDIT > mock/research/N.md
git checkout -q -f $OLD; git checkout -q -f dev; echo "replaced old commit, forced trip: edit = $(cat mock/research/N.md 2>&1 | head -1)"
# the branch cut before the migration gets a new commit that still tracks the path (a person who did not migrate yet)
git checkout -q feat; echo "feat-edit" > mock/research/N.md; echo more > mock/research/M.md; git add -Af; git commit -qm "feat: old shape, new commit"; echo "new tip tracks: $(git ls-tree -r --name-only HEAD | tr "\n" " ")"
git checkout -q dev; mkdir -p mock/research; echo EDIT2 > mock/research/N.md
git checkout -q -f feat; echo "after checkout -f of a NEW old-shape tip: edit = $(git show dev:c.rs >/dev/null; cat mock/research/N.md)  (EDIT2 would mean kept)"
