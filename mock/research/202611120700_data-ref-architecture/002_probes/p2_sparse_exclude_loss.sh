#!/bin/bash
# P2: the current shape in plain git (exclude + sparse skip-worktree), no engine: which git commands lose an edit,
# with and without sparse.expectFilesOutsideOfPatterns (op's pick c). Also clean -fdx, and -ff on a nested worktree.
set -u
S=${1:?scratch}; rm -rf $S/p2; mkdir -p $S/p2 && cd $S/p2
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
setup() { rm -rf r; git init -q -b dev r; cd r
  mkdir -p mock/research; echo old > mock/research/N.md; echo code > c.rs
  git add -A; git commit -qm pre; OLD=$(git rev-parse HEAD)
  git rm -rq --cached mock/research; git commit -qm migrate
  echo mock/research >> .git/info/exclude
  printf '/*\n!/mock/research/\n' > .git/info/sparse-checkout
  git config core.sparseCheckout true; git config core.sparseCheckoutCone false
  echo "EDIT" > mock/research/N.md; }
try() { # name expect-flag cmd-to-old cmd-back
  setup; [ "$2" = on ] && git config sparse.expectFilesOutsideOfPatterns true
  git $3 $OLD >/dev/null 2>&1; git $4 >/dev/null 2>&1; git checkout -q -f dev 2>/dev/null
  printf '%-58s expect=%-3s -> N.md: %s\n' "$1" "$2" "$(cat mock/research/N.md 2>&1 | head -1)"; cd ..; }
for e in off on; do
 try "checkout OLD ; checkout -f dev"            $e "checkout -q"    "checkout -q -f dev"
 try "checkout -f OLD ; checkout -f dev"         $e "checkout -q -f" "checkout -q -f dev"
 try "checkout OLD ; reset --hard dev"           $e "checkout -q"    "reset -q --hard dev"
 try "checkout -f OLD ; reset --hard dev"        $e "checkout -q -f" "reset -q --hard dev"
 try "checkout OLD ; checkout dev (plain)"       $e "checkout -q"    "checkout -q dev"
 try "reset --hard OLD ; reset --hard dev"       $e "reset -q --hard" "reset -q --hard dev"
done
echo "== clean in the exclude+sparse shape"
setup; git clean -fdxq; echo "after clean -fdx: $(ls mock 2>&1 | tr '\n' ' ')"; cd ..
echo "== clean -ff -dx with a nested data worktree"
rm -rf n; git init -q -b dev n; cd n; echo c > c.rs; git add -A; git commit -qm i; echo mock/research >> .git/info/exclude
git worktree add -q --orphan -b mock/research mock/research; (cd mock/research; echo E > N.md; git add N.md; git commit -qm d; echo UNCOMMITTED > N.md)
git clean -fdxq; echo "clean -fdx : $(cat mock/research/N.md)"; git clean -ffdxq; echo "clean -ffdx: $(cat mock/research/N.md 2>&1)"; git worktree list
