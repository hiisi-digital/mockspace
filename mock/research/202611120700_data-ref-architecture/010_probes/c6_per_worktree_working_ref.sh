#!/bin/bash
# Spike c6: the working ref as a PER-WORKTREE ref, refs/worktree/mock/<ns>, which git stores in the data
# worktree's own admin dir. Against c5's mirror-clone prune, against gc --prune=now run from the main dir and
# from a sibling worktree, and against push --mirror from the main dir. Also: can HEAD be a symref onto it,
# and does a commit in the data worktree move it.
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git init -q --bare -b dev o.git; ( cd o.git; D=$(git commit-tree $(printf '100644 blob %s\tmain.rs\n' "$(echo s | git hash-object -w --stdin)" | git mktree) -m dev); git update-ref refs/heads/dev $D
  git update-ref refs/mock/research $(git commit-tree $(printf '100644 blob %s\tN.md\n' "$(echo a | git hash-object -w --stdin)" | git mktree) -m d1) )
git clone -q --mirror "file://$S/o.git" m.git; cd m.git
git worktree add -q --detach ../w1 dev; git -C ../w1 worktree add -q --detach 'git~1/research' refs/mock/research
cd '../w1/git~1/research'; R=refs/worktree/mock/research
git update-ref --create-reflog $R HEAD; r=$(git symbolic-ref HEAD $R 2>&1); echo "symbolic-ref HEAD -> $R: {$r} status: $(git status 2>&1 | head -1)"
echo b >> N.md; git commit -qam local-unpushed; C=$(git rev-parse HEAD); echo "commit moved it: $([ "$(git rev-parse $R)" = "$C" ] && echo yes || echo no)"
echo "visible from main dir for-each-ref: [$(git -C "$S/m.git" for-each-ref --format='%(refname)' refs/worktree | tr '\n' ' ')] from data wt: [$(git for-each-ref --format='%(refname)' refs/worktree | tr '\n' ' ')]"
cd "$S/m.git"; echo "fetch --prune in mirror: {$(git fetch --prune origin 2>&1 | grep -E 'deleted|fatal' | tr '\n' ';')} ref after: $(git -C '../w1/git~1/research' rev-parse -q --verify $R | cut -c1-7)"
git gc -q --prune=now; echo "gc --prune=now from main: commit $(git cat-file -t $C 2>&1)"
git -C ../w1 gc -q --prune=now; echo "gc --prune=now from the code worktree: commit $(git cat-file -t $C 2>&1)"
git push -q --mirror origin 2>&1 | head -1; echo "remote refs after push --mirror from main: $(git -C ../o.git for-each-ref --format='%(refname)' | tr '\n' ' ')"
git -C '../w1/git~1/research' push -q --mirror origin 2>&1 | head -1; echo "remote refs after push --mirror from the data worktree: $(git -C ../o.git for-each-ref --format='%(refname)' | tr '\n' ' ')"
echo "rev-list --all from main counts the commit: $(git rev-list --all | grep -c $C)"
