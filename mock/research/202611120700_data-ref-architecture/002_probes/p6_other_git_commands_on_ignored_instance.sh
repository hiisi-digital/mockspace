#!/bin/bash
# P6: the exclude shape (instance files ignored, as the engine hides them): which ordinary git commands delete an
# edited ignored file with no refusal. Linked worktree removal, stash -u / -a, clean -fdX, checkout of a branch tracking the path.
set -u
S=${1:?scratch}; rm -rf $S/p6; mkdir -p $S/p6 && cd $S/p6
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q -b dev r && cd r && echo c > c.rs && git add -A && git commit -qm i
echo mock/research >> .git/info/exclude
git worktree add -q ../wt -b feat
mkdir -p ../wt/mock/research && echo EDIT > ../wt/mock/research/N.md
git -C ../wt status --short; echo "(status of linked worktree with an edited ignored file: empty above)"
git worktree remove ../wt; echo "worktree remove rc=$? ; dir exists: $(test -e ../wt && echo yes || echo no)"
mkdir -p mock/research && echo EDIT2 > mock/research/N.md
git stash -u -q 2>&1; echo "stash -u: file $(cat mock/research/N.md 2>&1)"
git stash -a -q 2>&1; echo "stash -a: file $(cat mock/research/N.md 2>&1 | head -1) ; stashes: $(git stash list | wc -l)"
git stash drop -q; git stash drop -q 2>/dev/null
mkdir -p mock/research && echo EDIT3 > mock/research/N.md; git clean -fdXq; echo "clean -fdX: file $(cat mock/research/N.md 2>&1 | head -1)"
