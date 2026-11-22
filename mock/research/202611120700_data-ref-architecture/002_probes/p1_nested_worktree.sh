#!/bin/bash
# P1: a data ref as a native linked worktree nested at an excluded path of the main worktree.
set -u
S=${1:?scratch}; rm -rf $S/p1; mkdir -p $S/p1 && cd $S/p1
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q -b dev main && cd main
mkdir -p mock/research && echo old > mock/research/N.md && echo code > c.rs
git add -A && git commit -qm "pre-migration: research tracked"; OLD=$(git rev-parse HEAD)
git rm -rq --cached mock/research && git commit -qm "migration: research leaves the branch"
echo mock/research >> .git/info/exclude
rm -rf mock/research
git worktree add -q --orphan -b mock/research mock/research
( cd mock/research && echo old > N.md; git add N.md; git commit -qm "data" )
echo "== (a) same branch in a second worktree"
git worktree add ../wt2 dev -b feat -q; (cd ../wt2 && git worktree add mock/research mock/research 2>&1 | head -2)
echo "== (f) status in main with data worktree present"; git status --short; echo "(end status)"
echo "== (b) edit data, go to OLD commit (tracks mock/research/N.md), force back"
echo "EDIT-A" > mock/research/N.md
git checkout -q -f $OLD 2>&1 | head -3; echo "checkout rc=$?"
echo "-- N.md after checkout of OLD:"; cat mock/research/N.md
git checkout -q -f dev 2>&1|head -3
echo "-- N.md after force back to dev:"; cat mock/research/N.md; ls -a mock/research | tr '\n' ' '; echo
echo "== (c) clean -fdx and -fdxx with an edit in the data worktree"
echo "EDIT-B" > mock/research/N.md; echo untracked > mock/research/U.md
git clean -fdxq; echo "after clean -fdx:"; ls mock/research 2>&1 | tr '\n' ' '; echo; cat mock/research/N.md
git clean -fdxxq; echo "after clean -fdxx:"; ls mock/research 2>&1 | tr '\n' ' '; echo
git worktree list; git worktree prune; git worktree list
