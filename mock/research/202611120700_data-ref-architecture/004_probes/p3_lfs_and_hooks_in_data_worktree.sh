#!/bin/bash
# 004/p3: both seats left two claims as impressions: (1) LFS works unchanged in a data worktree, (2) the repo's hooks
# run for a commit made inside a data worktree that lives outside the main work tree, reached by a symlink.
set -u
S=${1:?scratch}; rm -rf $S/q3; mkdir -p $S/q3 && cd $S/q3
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q -b dev r && cd r && echo c > c.rs && git add -A && git commit -qm i
git lfs install --local >/dev/null 2>&1; echo "lfs filter configured in common config: $(git config --get filter.lfs.clean)"
mkdir -p .git/hooks-x; printf '#!/bin/sh\necho "PRE-COMMIT ran: GIT_DIR=$GIT_DIR cwd=$(pwd) branch=$(git symbolic-ref --short HEAD)" >> '$S'/q3/hook.log\n' > .git/hooks/pre-commit; chmod +x .git/hooks/pre-commit
git worktree add -q --orphan -b mock/research .git/mock-data/research
mkdir -p mock; ln -s ../.git/mock-data/research mock/research; echo mock/research >> .git/info/exclude
cd .git/mock-data/research
printf '*.bin filter=lfs diff=lfs merge=lfs -text\n' > .gitattributes; head -c 5000 /dev/urandom > big.bin
git add -A && git commit -qm data 2>&1 | tail -1
echo "blob in the data branch for big.bin:"; git show HEAD:big.bin | head -3
echo "lfs object stored under the common dir: $(ls $S/q3/r/.git/lfs/objects/* 2>/dev/null | head -1 | wc -l) dir(s)"
echo "== hook log"; cat $S/q3/hook.log
echo "== main worktree status (symlink excluded):"; git -C $S/q3/r status --short; echo "(end)"
echo "== through the link, editors see the real bytes: $(wc -c < $S/q3/r/mock/research/big.bin)"
