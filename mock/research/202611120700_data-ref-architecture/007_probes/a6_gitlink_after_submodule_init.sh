#!/bin/bash
# Spike a6: a5 after the person runs `git submodule init` (or `update --init`), which makes the placeholder active.
# arm U under the same setting. Data worktree has an uncommitted EDIT and a commit past the pin.
set -u
S=${1:?scratch}; mkdir -p "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git config --global submodule.recurse true
D=mock/rdata
setup() {
  rm -rf "$S/c"; mkdir -p "$S/c"; cd "$S/c"; git init -q -b dev m; cd m; echo s > main.rs; git add -A; git commit -qm s; git tag OLD
  git checkout -q --orphan mock/research; git rm -rqf .; echo data > N.md; git add -A; git commit -qm data; git checkout -q dev
  if [ $ARM = G ]; then printf '[submodule "rdata"]\n\tpath = %s\n\turl = ./\n\tupdate = none\n\tignore = all\n' $D > .gitmodules
    git update-index --add --cacheinfo 160000,$(git rev-parse mock/research),$D; git add .gitmodules; git commit -qm placeholder; fi
  git worktree add -q $D mock/research 2>/dev/null || { rmdir $D; git worktree add -q $D mock/research; }
  git -C $D commit -q --allow-empty -m past-pin; echo EDIT >> $D/N.md; git submodule init -q 2>/dev/null; echo "   active=$(git config submodule.rdata.url)" >> "$S/act"
}
v() { grep -q EDIT $D/N.md 2>/dev/null && echo kept=1 || echo kept=0; }
for ARM in G; do for c in "checkout -f OLD; git checkout -f dev" "reset --hard OLD; git reset --hard dev@{1}" "checkout OLD; git checkout dev" "reset --hard" "switch --discard-changes --detach OLD; git switch --discard-changes dev" "read-tree -u --reset HEAD" "submodule update" "pull --no-rebase . dev"; do
  setup; r=$(eval "git $c" 2>&1 | tail -1); echo "$ARM recurse: git $c -> $(v) data HEAD=$(git -C $D log -1 --format=%s 2>&1) {$r}"
done; done
