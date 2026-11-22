#!/bin/bash
# P5: with the data ref as a native worktree (outside the tree, under the common dir): (a) two clones edit one file,
# `git pull` in the data worktree runs a custom merge driver named in the data branch's own .gitattributes;
# (b) the dirty data worktree survives gc --prune=now --aggressive; (c) a bare clone with linked worktrees: one data
# worktree under the bare dir serves them all, no per-worktree config, no sparse; (d) conflict leaves markers, nothing lost.
set -u
S=${1:?scratch}; rm -rf $S/p5; mkdir -p $S/p5 && cd $S/p5
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q --bare origin.git
git init -q -b dev seed && cd seed && echo code > c.rs && git add -A && git commit -qm i && git remote add origin ../origin.git && git push -q origin dev
git switch -q --orphan mock/research && printf 'log.md merge=keepboth\n' > .gitattributes && printf 'a\nb\n' > log.md && git add -A && git commit -qm data && git push -q origin mock/research; cd ..
# clone X: bare clone with linked worktrees
git --git-dir=origin.git symbolic-ref HEAD refs/heads/dev; git clone -q --bare origin.git x.git; git -C x.git config merge.keepboth.driver 'cat %A %B | sort -u > %A.new; mv %A.new %A'
git -C x.git worktree add -q ../x-dev dev; git -C x.git worktree add -q ../x-feat -b feat dev
# a bare clone already holds origin's mock/research as an ordinary local branch
git -C x.git worktree add -q mock-data-research mock/research
git -C x.git worktree list
ln -s ../x.git/mock-data-research x-dev/research-link; ln -s ../x.git/mock-data-research x-feat/research-link
echo "== (c) both linked worktrees see the data:"; cat x-dev/research-link/log.md | tr '\n' ' '; cat x-feat/research-link/log.md | tr '\n' ' '; echo
echo "== status of the bare-clone worktrees (symlink untracked, no sparse, no per-worktree config):"; git -C x-dev status --short; git -C x.git config --list | grep -ci 'worktreeconfig\|sparse'
# clone Y edits and pushes
git clone -q origin.git y && cd y && git switch -q mock/research && git config merge.keepboth.driver 'cat %A %B | sort -u > %A.new; mv %A.new %A' && printf 'a\nb\ny\n' > log.md && git commit -qam y && git push -q origin mock/research; cd ..
# X edits the same file, uncommitted -> gc -> commit -> pull
printf 'a\nb\nx\n' > x-dev/research-link/log.md
git -C x.git gc -q --prune=now --aggressive; echo "== (b) after gc --prune=now: $(tr '\n' ' ' < x-dev/research-link/log.md)"
git -C x.git/mock-data-research commit -qam x
git -C x.git/mock-data-research pull -q --no-rebase --no-edit origin mock/research 2>&1 | tail -2
echo "== (a) merged through the data branch's own driver, plain git pull:"; tr '\n' ' ' < x-dev/research-link/log.md; echo
git -C x.git/mock-data-research log --oneline --graph | head -5
