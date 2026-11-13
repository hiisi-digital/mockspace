#!/bin/bash
# P4: data ref as a native worktree OUTSIDE the main worktree (in <common-dir>/mock-data/research),
# reached by an untracked, excluded symlink mock/research. No sparse at all. Same six ways.
set -u
S=${1:?scratch}; rm -rf $S/p4; mkdir -p $S/p4 && cd $S/p4
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
setup() { rm -rf r; git init -q -b dev r; cd r
  mkdir -p mock/research; echo old > mock/research/N.md; echo code > c.rs
  git add -A; git commit -qm pre; OLD=$(git rev-parse HEAD)
  git rm -rq --cached mock/research; git commit -qm migrate; rm -rf mock/research
  git worktree add -q --orphan -b mock/research .git/mock-data/research
  (cd .git/mock-data/research; echo old > N.md; git add N.md; git commit -qm d)
  echo mock/research >> .git/info/exclude
  mkdir -p mock; ln -s ../.git/mock-data/research mock/research
  echo EDIT > mock/research/N.md; }
try() { setup
  git $2 $OLD >/dev/null 2>&1; git $3 >/dev/null 2>&1; git checkout -q -f dev 2>/dev/null
  printf '%-44s -> real file: %-6s | via path: %s | symlink: %s\n' "$1" "$(cat .git/mock-data/research/N.md 2>&1|head -1)" "$(cat mock/research/N.md 2>&1|head -1)" "$(test -L mock/research && echo kept || echo gone)"; cd ..; }
 try "checkout OLD ; checkout -f dev"      "checkout -q"     "checkout -q -f dev"
 try "checkout -f OLD ; checkout -f dev"   "checkout -q -f"  "checkout -q -f dev"
 try "checkout OLD ; reset --hard dev"     "checkout -q"     "reset -q --hard dev"
 try "checkout -f OLD ; reset --hard dev"  "checkout -q -f"  "reset -q --hard dev"
 try "checkout OLD ; checkout dev (plain)" "checkout -q"     "checkout -q dev"
 try "reset --hard OLD ; reset --hard dev" "reset -q --hard" "reset -q --hard dev"
setup; git clean -fdxq; echo "clean -fdx : symlink $(test -L mock/research && echo kept || echo gone), real file $(cat .git/mock-data/research/N.md)"
setup; git clean -ffdxq; echo "clean -ffdx: symlink $(test -L mock/research && echo kept || echo gone), real file $(cat .git/mock-data/research/N.md)"
