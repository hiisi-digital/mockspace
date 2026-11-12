#!/bin/bash
# Spike a4: arm G, a placeholder gitlink. The code branch tracks a 160000 entry at the data path plus a
# .gitmodules stanza (url ./, update = none, ignore = all). The data worktree is nested at that path as a
# linked worktree of the data branch, exactly as in the converged shape. Question: does occupying the path in
# the code branch's own tree close what a1/a2/a3 left open (double-f clean, worktree remove --force, a commit
# that puts regular files at the path), and what does it cost in status noise and vanilla submodule verbs.
set -u
S=${1:?scratch}; mkdir -p "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git config --global protocol.file.allow always
D=mock/rdata
setup() {
  rm -rf "$S/c"; mkdir -p "$S/c"; cd "$S/c"; git init -q -b dev m; cd m; echo s > main.rs; git add -A; git commit -qm s; git tag OLD
  git checkout -q --orphan mock/research; git rm -rqf .; echo data > N.md; git add -A; git commit -qm data; git checkout -q dev
  printf '[submodule "rdata"]\n\tpath = %s\n\turl = ./\n\tupdate = none\n\tignore = all\n' $D > .gitmodules
  git update-index --add --cacheinfo 160000,$(git rev-parse mock/research),$D; git add .gitmodules; git commit -qm placeholder
  git worktree add -q $D mock/research 2>/dev/null || { rmdir $D 2>/dev/null; git worktree add -q $D mock/research; }
  echo EDIT >> $D/N.md; echo untracked > $D/U.md
}
v() { local k=0; grep -q EDIT "$1/$D/N.md" 2>/dev/null && [ -f "$1/$D/U.md" ] && k=1; echo "kept=$k"; }
echo "== instantiate over the placeholder"; setup; git -C $D rev-parse --abbrev-ref HEAD
echo "== main status with data advanced and dirty: [$(git -C $D commit -qam adv; echo EDIT >> $D/N.md; git status --short | tr '\n' ' ')]"
for c in "clean -fd" "clean -ffd" "clean -ffdx" "clean -ffdX" "checkout -f OLD; git checkout -f dev" "reset --hard OLD; git reset --hard dev@{1}" "stash -a; git stash pop" "add -A; git commit -qm x; git checkout -qf HEAD~1; git checkout -qf dev" "submodule update --init --force" "submodule update --init --checkout --force" "submodule deinit -f $D" "rm -r --cached $D; git reset -q" "read-tree -u --reset OLD; git read-tree -u --reset dev"; do
  setup; r=$(eval "git $c" 2>&1 | tail -1); echo "G main: git $c -> $(v .) {$r}"
done
echo "== add -A with data advanced: does the gitlink move on the code branch?"; setup; git -C $D commit -qam adv; git add -A; git status --short --ignore-submodules=none | tr '\n' ' '; echo
echo "== an uninstantiated clone writes files at the path by hand and runs add -A"
setup; cd "$S/c"; rm -rf other; git clone -q m other 2>&1; cd other; ls -A $D 2>&1 | tr '\n' ' '; echo "(dir listing)"
mkdir -p $D; echo handwritten > $D/N.md; git add -A 2>&1 | head -2; git status --short | tr '\n' ' '; echo; git commit -qm hand 2>&1|head -1; git ls-tree -r HEAD -- $D | head -3
echo "== clone --recurse-submodules of the repo"; cd "$S/c"; rm -rf rec; git clone -q --recurse-submodules m rec 2>&1 | tail -2; ls -A rec/$D 2>&1 | tr '\n' ' '; echo
echo "== linked worktree w2 holding a nested data worktree under the placeholder"
setup; git branch -q mock-wt/research/w2 mock/research; git worktree add -q -b feat ../w2 dev; rmdir ../w2/$D 2>/dev/null; git -C ../w2 worktree add -q $D mock-wt/research/w2; echo EDIT >> ../w2/$D/N.md; echo u > ../w2/$D/U.md
for c in "worktree remove ../w2" "worktree remove --force ../w2" "worktree remove -f -f ../w2" "worktree move ../w2 ../w3"; do
  cp -a "$S/c" "$S/c.bak"; r=$(git $c 2>&1 | head -1); W=../w2; [ -d ../w3 ] && W=../w3; echo "G linked: git $c -> $(v $W) {$r}"; cd "$S"; rm -rf c; mv c.bak c; cd c/m
done
