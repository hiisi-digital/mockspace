#!/bin/bash
# Spike c2: the data worktree whose HEAD is a symref onto refs/mock-wt/<ns>@<wt> (agreed by 008 and 009),
# under DEFAULT config (core.logAllRefUpdates unset). Which vanilla verbs a person might type inside it keep
# the symref and the ref moving together, and what recovers a reset --hard.
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home" GIT_EDITOR=true; mkdir -p "$HOME"
W='refs/mock-wt/research@w1'
setup() { rm -rf r; git init -q -b dev r; cd r; echo s > main.rs; git add -A; git commit -qm s
  T=$(printf '100644 blob %s\tN.md\n' "$(printf 'a\n' | git hash-object -w --stdin)" | git mktree); C=$(git commit-tree $T -m d1); git update-ref refs/mock/research $C
  T2=$(printf '100644 blob %s\tN.md\n100644 blob %s\tO.md\n' "$(printf 'a\n' | git hash-object -w --stdin)" "$(echo o | git hash-object -w --stdin)" | git mktree); git update-ref refs/mock/research $(git commit-tree $T2 -p $C -m d2-remote) 
  git update-ref "$W" $C; git worktree add -q --detach mock/rdata "$W"; git -C mock/rdata symbolic-ref HEAD "$W"
  cd mock/rdata; echo b >> N.md; git commit -qam local1; }
st() { echo "HEAD->$(git symbolic-ref -q HEAD || echo DETACHED) ref=$(git log -1 --format=%s "$W") head=$(git log -1 --format=%s HEAD) parent=$(git log -1 --format=%s HEAD~1 2>/dev/null)"; }
setup; echo "default logAllRefUpdates: [$(git config core.logAllRefUpdates)] ref reflog exists: $(git reflog exists "$W" && echo yes || echo no)"
git reset -q --hard HEAD~1; echo "reset --hard HEAD~1: $(st); HEAD@{1}=$(git log -1 --format=%s 'HEAD@{1}' 2>&1); ORIG_HEAD=$(git log -1 --format=%s ORIG_HEAD)"; cd "$S"
setup; git commit -q --amend -m amended; echo "commit --amend: $(st)"; cd "$S"
setup; git rebase -q refs/mock/research 2>&1 | head -1; echo "rebase refs/mock/research: $(st)"; cd "$S"
setup; git pull -q --rebase . refs/mock/research 2>&1 | head -1; echo "pull --rebase <explicit>: $(st)"; cd "$S"
setup; git merge -q --no-edit refs/mock/research 2>&1 | head -1; echo "merge: $(st)"; cd "$S"
setup; echo c >> N.md; git stash -q; git stash pop -q; echo "stash/pop: $(st) dirty=[$(git status --short | tr '\n' ' ')]"; cd "$S"
setup; git cherry-pick refs/mock/research 2>&1 | head -1 >/dev/null; echo "cherry-pick: $(st)"; cd "$S"
setup; git switch -q --detach HEAD 2>&1|head -1; echo "switch --detach then: $(st)"; cd "$S"
setup; git checkout -q -b mine 2>&1|head -1; echo "checkout -b mine: $(st) (commits now go to $(git symbolic-ref HEAD))"; cd "$S"
setup; git -C "$S/r" gc -q --prune=now; echo "gc in main keeps the ref's commit: $(git cat-file -t $W 2>&1)"; cd "$S"
echo "== the working ref made with update-ref --create-reflog, default config"
rm -rf r; git init -q -b dev r; cd r; T=$(printf '100644 blob %s\tN.md\n' "$(echo a | git hash-object -w --stdin)" | git mktree); C=$(git commit-tree $T -m d1)
git update-ref --create-reflog "$W" $C; git worktree add -q --detach mock/rdata "$W"; cd mock/rdata; git symbolic-ref HEAD "$W"; echo b >> N.md; git commit -qam local1; git reset -q --hard HEAD~1
echo "ref reflog exists: $(git reflog exists "$W" && echo yes || echo no); $W@{1} = $(git log -1 --format=%s "$W@{1}" 2>&1)"
