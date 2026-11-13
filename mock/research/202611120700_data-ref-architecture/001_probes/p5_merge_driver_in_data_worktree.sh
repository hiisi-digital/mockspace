#!/bin/bash
# Spike: git's own merge, run in a data worktree, honours a custom merge driver from the shared repo
# config and a $GIT_COMMON_DIR/info/attributes line, and a .gitattributes inside the data branch. Control
# line: same conflict with no driver.
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev m; cd m; echo s > main.rs; git add -A; git commit -qm s
git checkout -q --orphan mock/research; git rm -rqf .; printf 'a\nb\nc\n' > NOTES.md; git add -A; git commit -qm data; git checkout -q dev
git worktree add -q mock/research mock/research
cd mock/research
git branch other; 
echo 'x' >> NOTES.md; git commit -qam mine
git checkout -q other; echo 'y' >> NOTES.md; git commit -qam theirs; git checkout -q mock/research
echo "== control: no driver"
git merge other 2>&1 | tail -2; git merge --abort
echo "== custom driver from repo config + info/attributes"
git config merge.upper.driver 'sh -c "cat %A %B | sort -u > t; mv t %A" -- %O %A %B'
echo 'NOTES.md merge=upper' >> "$(git rev-parse --git-common-dir)/info/attributes"
git merge other 2>&1 | tail -2; echo "NOTES: $(tr '\n' '|' < NOTES.md)"
