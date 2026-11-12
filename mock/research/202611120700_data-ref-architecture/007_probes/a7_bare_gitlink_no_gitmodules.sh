#!/bin/bash
# Spike a7: arm G', the placeholder gitlink with NO .gitmodules stanza, so no submodule verb can ever make it
# active. Person has submodule.recurse=true and runs submodule init/update. Same data state as a5/a6.
set -u
S=${1:?scratch}; mkdir -p "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git config --global submodule.recurse true; git config --global protocol.file.allow always
D=mock/rdata
setup() {
  rm -rf "$S/c"; mkdir -p "$S/c"; cd "$S/c"; git init -q -b dev m; cd m; echo s > main.rs; git add -A; git commit -qm s; git tag OLD
  git checkout -q --orphan mock/research; git rm -rqf .; echo data > N.md; git add -A; git commit -qm data; git checkout -q dev
  git update-index --add --cacheinfo 160000,$(git rev-parse mock/research),$D; git commit -qm placeholder
  rmdir $D 2>/dev/null; git worktree add -q $D mock/research
  git -C $D commit -q --allow-empty -m past-pin; echo EDIT >> $D/N.md; echo u > $D/U.md
  git submodule init >/dev/null 2>&1; git submodule update --init >/dev/null 2>&1
}
v() { grep -q EDIT $D/N.md 2>/dev/null && [ -f $D/U.md ] && echo kept=1 || echo kept=0; }
setup; echo "status: [$(git status --short | tr '\n' ' ')] submodule status: [$(git submodule status 2>&1 | tr '\n' ' ')] data .git: $(cat $D/.git)"
for c in "checkout -f OLD; git checkout -f dev" "reset --hard OLD; git reset --hard placeholder" "reset --hard" "clean -ffdx" "clean -ffd" "stash -a; git stash pop" "submodule update --init --force --checkout" "submodule deinit -f --all" "submodule foreach true" "read-tree -u --reset OLD; git read-tree -u --reset dev"; do
  setup; git tag -f placeholder dev >/dev/null; r=$(eval "git $c" 2>&1 | tail -1); echo "G' git $c -> $(v) data-wt=$(git -C $D rev-parse --abbrev-ref HEAD 2>&1 | head -1) {$r}"
done
echo "== uninstantiated clone, hand-written file, add -A"; cd "$S/c"; rm -rf o; git clone -q m o; cd o; mkdir -p $D; echo hand > $D/N.md; git add -A 2>&1|head -1; git status --short | tr '\n' ' '; echo "(status)"; git commit -qm hand 2>&1 | head -1; git ls-tree -r HEAD -- $D
echo "== add -A in the instantiated clone after the data advanced"; setup; git add -A; git status --short | tr '\n' ' '; echo
