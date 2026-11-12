#!/bin/bash
# Spike: can git replace make pre-migration commits "not track" the data path, so a same-path data dir
# is never collided with? Same-path nested worktree, hidden by info/exclude (the variant that lost the
# edit on a plain checkout in p2). Also: does a fresh clone see the replacement without extra setup?
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev m; cd m
mkdir -p mock/research; echo old > mock/research/NOTES.md; echo src > main.rs
git add -A; git commit -qm old; OLD=$(git rev-parse HEAD)
git rm -rq mock/research; git commit -qm migrate
DC=$(git commit-tree "$OLD:mock/research" -m data); git branch mock/research $DC
git worktree add -q mock/research mock/research; echo mock/research/ >> .git/info/exclude
# the replacement: same message/parents, tree without mock/research
NEWTREE=$(git ls-tree $OLD | grep -v 'mock$' | git mktree)   # only top-level 'mock' dir removed (probe-sized)
REP=$(git commit-tree $NEWTREE -m "old (view without data path)")
git replace $OLD $REP
echo EDIT >> mock/research/NOTES.md
echo "== checkout OLD (replaced)"; git checkout -q $OLD 2>&1 | head -2; echo "edit survives: $(grep -c EDIT mock/research/NOTES.md)"; git ls-tree -r --name-only HEAD | tr '\n' ' '; echo
git checkout -q dev; git reset -q --hard $OLD; echo "after reset --hard OLD: edit survives: $(grep -c EDIT mock/research/NOTES.md)"; git reset -q --hard dev 2>/dev/null
echo "== fresh clone sees replacement?"; cd "$R"; git clone -q m c 2>&1 | head -1; cd c; echo "refs/replace in clone: $(git for-each-ref refs/replace | wc -l)"
echo "== after fetching refs/replace explicitly:"; git fetch -q origin 'refs/replace/*:refs/replace/*'; echo "refs/replace in clone: $(git for-each-ref refs/replace | wc -l)"
echo "== git log -1 --format=%H OLD vs replaced identity (blame/log see replaced object):"; git cat-file -p $OLD | head -3
