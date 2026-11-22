#!/bin/bash
# 009/p5: the dedicated remote may point at ANOTHER repository (the fallback if a host refuses refs/mock/*). The data worktree is a
# worktree of the main repo's object store either way; only where objects are pushed to and fetched from changes.
# Question: do the main repo's merge driver config and a custom filter still apply, and does origin see nothing of the data?
set -u
S=${1:?scratch}; rm -rf $S/p5; mkdir -p $S/p5 && cd $S/p5; ROOT=$PWD
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q --bare origin.git; git --git-dir=origin.git symbolic-ref HEAD refs/heads/dev; git init -q --bare datarepo.git; git --git-dir=datarepo.git symbolic-ref HEAD refs/heads/data
git clone -q file://$ROOT/origin.git a 2>/dev/null; git clone -q file://$ROOT/origin.git b 2>/dev/null
for c in a b; do ( cd $c; git config merge.keepboth.driver 'cat %A %B | sort -u > %A.new; mv %A.new %A'; git remote add mock-research file://$ROOT/datarepo.git
  git config remote.mock-research.fetch '+refs/heads/data:refs/remotes/mock-research/research'; git config remote.mock-research.push 'HEAD:refs/heads/data'; git config remote.mock-research.tagOpt --no-tags ); done
cd a; echo c > c.rs; git add -A; git commit -qm i; git push -q origin HEAD:dev
git worktree add -q --orphan -b tmp mdr; ( cd mdr; printf 'log.md merge=keepboth\n' > .gitattributes; printf 'a\nb\n' > log.md; git add -A; git commit -qm data
  git update-ref refs/mock-wt/research/a HEAD; git symbolic-ref HEAD refs/mock-wt/research/a; git branch -q -D tmp; git push -q mock-research )
cd ../b; git fetch -q mock-research; git update-ref refs/mock-wt/research/b refs/remotes/mock-research/research; git worktree add -q --detach mdr refs/mock-wt/research/b; ( cd mdr; git symbolic-ref HEAD refs/mock-wt/research/b; printf 'a\nb\ny\n' > log.md; git commit -qam y; git push -q mock-research )
cd ../a/mdr; printf 'a\nb\nx\n' > log.md; git commit -qam x; git pull -q --no-rebase --no-edit mock-research refs/heads/data 2>&1 | tail -1
echo "merged through the main repo's driver config, via a different remote URL: $(tr '\n' ' ' < log.md)"
echo "origin.git refs: $(git --git-dir=$ROOT/origin.git for-each-ref --format='%(refname)' | tr '\n' ' ')   datarepo.git refs: $(git --git-dir=$ROOT/datarepo.git for-each-ref --format='%(refname)' | tr '\n' ' ')"
