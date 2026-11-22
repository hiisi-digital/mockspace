#!/bin/bash
# P3: data ref as a native nested worktree, AND the main worktree's sparse checkout leaving that path out.
# Same six ways to an old commit and back as P2. Shows what the nested worktree buys over exclude+sparse.
set -u
S=${1:?scratch}; rm -rf $S/p3; mkdir -p $S/p3 && cd $S/p3
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
setup() { rm -rf r; git init -q -b dev r; cd r
  mkdir -p mock/research; echo old > mock/research/N.md; echo code > c.rs
  git add -A; git commit -qm pre; OLD=$(git rev-parse HEAD)
  git rm -rq --cached mock/research; git commit -qm migrate
  echo mock/research >> .git/info/exclude; rm -rf mock/research
  git worktree add -q --orphan -b mock/research mock/research
  (cd mock/research; echo old > N.md; git add N.md; git commit -qm d)
  printf '/*\n!/mock/research/\n' > .git/info/sparse-checkout
  git config core.sparseCheckout true; git config core.sparseCheckoutCone false
  echo EDIT > mock/research/N.md; }
try() { setup; [ "$2" = on ] && git config sparse.expectFilesOutsideOfPatterns true
  git $3 $OLD >/dev/null 2>&1; git $4 >/dev/null 2>&1; git checkout -q -f dev 2>/dev/null
  printf '%-44s expect=%-3s -> N.md: %s | data worktree status: %s\n' "$1" "$2" "$(cat mock/research/N.md 2>&1|head -1)" "$(git -C mock/research status --short 2>&1|tr '\n' ' ')"; cd ..; }
for e in off on; do
 try "checkout OLD ; checkout -f dev"      $e "checkout -q"     "checkout -q -f dev"
 try "checkout -f OLD ; checkout -f dev"   $e "checkout -q -f"  "checkout -q -f dev"
 try "checkout OLD ; reset --hard dev"     $e "checkout -q"     "reset -q --hard dev"
 try "checkout -f OLD ; reset --hard dev"  $e "checkout -q -f"  "reset -q --hard dev"
 try "checkout OLD ; checkout dev (plain)" $e "checkout -q"     "checkout -q dev"
 try "reset --hard OLD ; reset --hard dev" $e "reset -q --hard" "reset -q --hard dev"
done
