#!/bin/bash
# 006/p3: 001's reading N leaves the nested data worktree unhidden, and says a pre-commit check can refuse the 160000
# entry that `git add -A` records. A hook can be absent. If the gitlink IS committed to a code branch, what do the
# ordinary verbs do to the data worktree's uncommitted edit?
set -u
S=${1:?scratch}; rm -rf $S/r3; mkdir -p $S/r3 && cd $S/r3
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q -b dev m && cd m && echo c > c.rs && git add -A && git commit -qm i
mkdir -p mock; git worktree add -q --orphan -b mock/study mock/study; (cd mock/study && echo d > N.md && git add N.md && git commit -qm d)
git checkout -q -b polluted; git add -A 2>/dev/null; git commit -qm "gitlink committed by git add -A"; echo "polluted tip tracks: $(git ls-tree -r HEAD | grep 160000 | cut -f2)"
echo EDIT > mock/study/N.md
for way in "checkout -q -f dev" "checkout -q -f polluted" "reset -q --hard dev" "checkout -q polluted" "switch -q --detach dev" "clean -fdq"; do
  git $way >/dev/null 2>&1; printf '%-28s edit=%s  worktree list: %s\n' "$way" "$(cat mock/study/N.md 2>&1 | head -1)" "$(git worktree list | wc -l) entries"
done
