#!/bin/bash
# Spike c5: the mirror clone shape of the test matrix. A `clone --mirror` fetches +refs/*:refs/* and is kept
# current with `fetch --prune`. If the person adds a code worktree and a data worktree to it, the engine's
# LOCAL-ONLY refs (working ref refs/mock-wt/..., snapshot ref refs/mock-snapshot/...) match refs/* and do not
# exist on the remote. Does a pruning fetch delete them, and what still holds the data commit afterwards?
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git init -q --bare -b dev o.git; ( cd o.git; D=$(git commit-tree $(printf '100644 blob %s\tmain.rs\n' "$(echo s | git hash-object -w --stdin)" | git mktree) -m dev); git update-ref refs/heads/dev $D
  git update-ref refs/mock/research $(git commit-tree $(printf '100644 blob %s\tN.md\n' "$(echo a | git hash-object -w --stdin)" | git mktree) -m d1) )
git clone -q --mirror "file://$S/o.git" m.git; cd m.git
W='refs/mock-wt/research@w1'; git update-ref --create-reflog "$W" refs/mock/research
git worktree add -q --detach ../w1 dev; git -C ../w1 worktree add -q --detach 'git~1/research' "$W"; cd '../w1/git~1/research'; git symbolic-ref HEAD "$W"
echo b >> N.md; git commit -qam local-unpushed; C=$(git rev-parse HEAD)
git update-ref refs/mock-snapshot/research@w1 $C
cd "$S/m.git"; echo "before: $(git for-each-ref --format='%(refname)' refs/mock-wt refs/mock-snapshot | tr '\n' ' ')"
out=$(git fetch --prune origin 2>&1 | grep -E 'deleted|error|fatal' | tr '\n' ';'); echo "fetch --prune: {$out}"
echo "after: [$(git for-each-ref --format='%(refname)' refs/mock-wt refs/mock-snapshot | tr '\n' ' ')]"
cd '../w1/git~1/research'; echo "data worktree: status {$(git status 2>&1 | head -1)} HEAD reflog holds commit: $(git reflog HEAD 2>/dev/null | grep -c local-unpushed)"
cd "$S/m.git"; git gc -q --prune=now 2>&1 | head -1; echo "after gc --prune=now the unpushed commit: $(git cat-file -t $C 2>&1)"
echo "(code worktree detached so that the checked-out-branch refusal does not mask the prune)"
