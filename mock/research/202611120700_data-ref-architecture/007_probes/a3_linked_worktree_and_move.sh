#!/bin/bash
# Spike a3: the converged placement in a LINKED worktree w2 of a normal clone. Data worktree nested at
# w2/mock/rdata with an uncommitted edit. Arms: U unhidden, H hidden (common info/exclude), HL hidden plus
# `git worktree lock` on w2 (taken by the engine when it nests data in a linked worktree), UL unhidden + lock.
# Verbs: worktree remove, remove --force, remove -f -f, move, and the whole clone moved with mv then repair.
set -u
S=${1:?scratch}; mkdir -p "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
D=mock/rdata
setup() {
  rm -rf "$S/c" "$S/c2"; mkdir -p "$S/c"; cd "$S/c"; git init -q -b dev m; cd m; echo s > main.rs; git add -A; git commit -qm s
  git checkout -q --orphan mock/research; git rm -rqf .; echo data > N.md; git add -A; git commit -qm data; git checkout -q dev
  git branch -q mock-wt/research/w2 mock/research
  git worktree add -q -b feat ../w2 dev
  git -C ../w2 worktree add -q $D mock-wt/research/w2
  case $ARM in H|HL) echo "/$D/" >> .git/info/exclude;; esac
  case $ARM in HL|UL) git worktree lock --reason "holds data worktree $D" ../w2;; esac
  echo EDIT >> ../w2/$D/N.md
}
v() { local k=0; grep -q EDIT "$1/$D/N.md" 2>/dev/null && k=1; echo "kept=$k"; }
for ARM in U H HL UL; do
  setup; r=$(git worktree remove ../w2 2>&1 | head -1); echo "$ARM worktree remove w2          : $(v ../w2) {$r}"
  setup; r=$(git worktree remove --force ../w2 2>&1 | head -1); echo "$ARM worktree remove --force w2  : $(v ../w2) {$r}"
  setup; r=$(git worktree remove -f -f ../w2 2>&1 | head -1); echo "$ARM worktree remove -f -f w2    : $(v ../w2) {$r}"
  setup; r=$(git worktree move ../w2 ../w3 2>&1 | head -1); W=../w2; [ -d ../w3 ] && W=../w3
        echo "$ARM worktree move w2 w3         : $(v $W) {$r} data-wt-ok=$(git -C $W/$D rev-parse --abbrev-ref HEAD 2>&1 | head -1)"
  setup; r=$(git worktree move -f -f ../w2 ../w3 2>&1 | head -1); W=../w2; [ -d ../w3 ] && W=../w3
        echo "$ARM worktree move -f -f w2 w3   : $(v $W) {$r} data-wt-ok=$(git -C $W/$D rev-parse --abbrev-ref HEAD 2>&1 | head -1) prune-then:$(git worktree prune -v 2>&1 | tr '\n' ' ')"
  setup; cd "$S"; mv c c2; cd c2/m; a=$(git -C ../w2/$D status --short 2>&1 | head -1)
        git worktree repair ../w2 ../w2/$D >/dev/null 2>&1; b=$(git -C ../w2/$D status --short 2>&1 | head -1)
        echo "$ARM mv clone; repair             : $(v ../w2) before-repair={$a} after-repair={$b}"
done
