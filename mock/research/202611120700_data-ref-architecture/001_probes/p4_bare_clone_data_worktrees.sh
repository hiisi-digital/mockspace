#!/bin/bash
# Spike: bare clone, two linked worktrees of dev, a data branch as a nested linked worktree in each.
# Question: can one data branch be checked out in the data worktree of two linked worktrees, and what
# does the second one see once the first commits? (merge drivers: see p5.)
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev seed; (cd seed; echo s > main.rs; git add -A; git commit -qm s
  git checkout -q --orphan mock/research; git rm -rqf .; printf 'a\nb\nc\n' > NOTES.md; git add -A; git commit -qm data; git checkout -q dev)
git clone -q --bare seed bare.git; cd bare.git
git worktree add -q ../w1 dev 2>&1; git worktree add -q -b feat ../w2 dev 2>&1
echo "== (a) data branch in two worktrees"
git worktree add -q ../w1/data mock/research 2>&1; echo "first rc=$?"
git worktree add -q ../w2/data mock/research 2>&1; echo "second rc=$? (refused without -f)"
git worktree add -q -f ../w2/data mock/research 2>&1; echo "forced rc=$?"
echo "commit in w1/data, then look at w2/data"
(cd ../w1/data; echo d >> NOTES.md; git commit -qam "w1 edit")
(cd ../w2/data; echo "w2 status after w1 moved the shared branch:"; git status --short | head; git log --oneline | head -2; echo "dirty-looking: $(git diff --stat | tail -1)")
