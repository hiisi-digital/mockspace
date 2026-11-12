#!/bin/bash
# Spike: the variant my 001 file listed as unrun (probe 9). A remote, a bare clone with two linked worktrees of
# dev, a nested UNHIDDEN data worktree in each on its own tracking branch (mock/research in the first,
# mock-wt/research/w2 in the second). Edit in each, push from one, pull in the other with a merge driver set
# in shared config, and remove a dirty linked worktree.
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev seed; (cd seed; echo s > main.rs; git add -A; git commit -qm s; git checkout -q --orphan mock/research; git rm -rqf .; printf 'a\nb\nc\n' > NOTES.md; git add -A; git commit -qm data; git checkout -q dev)
git clone -q --bare seed bare.git; cd bare.git
git config merge.upper.driver 'sh -c "cat %A %B | sort -u > t; mv t %A" -- %O %A %B'; echo 'NOTES.md merge=upper' >> info/attributes 2>/dev/null || { mkdir -p info; echo 'NOTES.md merge=upper' >> info/attributes; }
git remote add origin "$R/seed" 2>/dev/null; git config remote.origin.fetch '+refs/heads/*:refs/remotes/origin/*'; git fetch -q origin
git worktree add -q ../w1 dev; git worktree add -q -b feat ../w2 dev
git worktree add -q ../w1/mock/data/research mock/research
git branch -q mock-wt/research/w2 mock/research; git worktree add -q ../w2/mock/data/research mock-wt/research/w2
(cd ../w1/mock/data/research; echo one >> NOTES.md; git commit -qam w1)
(cd ../w2/mock/data/research; echo two >> NOTES.md; git commit -qam w2)
echo "== w1 sees w2's uncommitted edit? (per-worktree branches: isolation)"; (cd ../w2/mock/data/research; echo scratch > S.md); (cd ../w1/mock/data/research; git status --short | tr '\n' ' '; echo "(w1 status)")
echo "== sync: merge w2's branch into w1's with the shared driver (git's own merge)"
(cd ../w1/mock/data/research; git merge -q mock-wt/research/w2 2>&1 | head -2; echo "NOTES: $(tr '\n' '|' < NOTES.md)")
echo "== remove dirty linked worktree w2 (nested data is unhidden)"
git worktree remove ../w2 2>&1 | head -1
