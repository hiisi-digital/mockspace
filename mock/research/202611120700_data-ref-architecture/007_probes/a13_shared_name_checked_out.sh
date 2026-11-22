#!/bin/bash
# Spike a13: the contrast for a12. 005 r3 checks out the SHARED name mock/research in w1's data worktree.
# In a bare clone kept current by a mirror-style refspec (+refs/heads/*:refs/heads/*), does every fetch then
# fail, and does it take the code branches' update down with it?
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git init -q --bare -b dev origin.git; git clone -q origin.git seed 2>/dev/null; cd seed; echo s > main.rs; git add -A; git commit -qm s; git push -q origin dev
git checkout -q --orphan mock/research; git rm -rqf .; echo a > N.md; git add -A; git commit -qm data; git push -q origin mock/research; cd ..
git clone -q --bare origin.git p.git; git -C p.git config remote.origin.fetch '+refs/heads/*:refs/heads/*'
git -C p.git worktree add -q -b feat1 ../w1 dev; git -C w1 worktree add -q mock/rdata mock/research
(cd seed; git checkout -q dev; echo s2 >> main.rs; git commit -qam s2; git push -q origin dev; git checkout -q mock/research; echo b >> N.md; git commit -qam d2; git push -q origin mock/research)
echo "fetch with shared name checked out: {$(git -C p.git fetch origin 2>&1 | grep -E 'fatal|error|refusing' | head -1)} dev updated=$([ "$(git -C p.git rev-parse dev)" = "$(git -C origin.git rev-parse dev)" ] && echo yes || echo no)"
