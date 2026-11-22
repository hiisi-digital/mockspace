#!/bin/bash
# Spike c4: the untrackable component at the TOP, `git~1/<ns>`, holding two namespaces at the depth that
# mock/<ns> had. Does git refuse a polluting path under each, do two data worktrees coexist there, and does a
# relative path from a data file to the repo root resolve as it did from mock/<ns> (shell, cwd inside).
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git init -q -b dev r; cd r; mkdir bench-harness; echo h > bench-harness/x; echo '/git~1/' > .gitignore; git add -A; git commit -qm s
for NS in research benches; do
  T=$(printf '100644 blob %s\tN.md\n' "$(echo $NS | git hash-object -w --stdin)" | git mktree); git update-ref --create-reflog "refs/mock-wt/$NS@main" $(git commit-tree $T -m $NS)
  git worktree add -q --detach "git~1/$NS" "refs/mock-wt/$NS@main"; git -C "git~1/$NS" symbolic-ref HEAD "refs/mock-wt/$NS@main"
done
echo "worktrees: $(git worktree list | awk '{print $1}' | sed "s|$S/r||" | tr '\n' ' ')"
mkdir -p "git~1/research/R_probes/p"; echo "from git~1/research/R_probes/p, ../../../../bench-harness/x reads: $(cd 'git~1/research/R_probes/p' && cat ../../../../bench-harness/x)"
git add -f "git~1/research/N.md" "git~1/benches/N.md"; echo "add -f under each: rc=$? staged=[$(git diff --cached --name-only | tr "\n" " ")] (the nested worktree is a repository boundary to add; the plumbing row below is the real test)"
B=$(echo pollute | git hash-object -w --stdin); IN=$(printf '100644 blob %s\tN.md\n' $B | git mktree); NSDIR=$(printf '040000 tree %s\tresearch\n' $IN | git mktree)
ROOT=$( (git ls-tree HEAD; printf '040000 tree %s\tgit~1\n' $NSDIR) | git mktree ); C=$(git commit-tree -p HEAD -m crafted $ROOT)
echo EDIT >> 'git~1/research/N.md'; echo "checkout -f crafted: {$(git checkout -qf $C 2>&1 | head -1)} merge crafted: {$(git merge -q $C 2>&1 | head -1)} edit=$(grep -c EDIT 'git~1/research/N.md')"
echo "status of main: [$(git status --short | tr '\n' ' ')]"
