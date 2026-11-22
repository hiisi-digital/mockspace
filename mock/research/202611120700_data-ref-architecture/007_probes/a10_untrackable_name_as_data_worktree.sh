#!/bin/bash
# Spike a10: a nested data worktree AT an untrackable name (mock/git~1), unhidden and hidden. What do the
# everyday main-tree verbs say: status, add -A, stash -u, commit -a, and cargo-style relative paths out.
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
D='mock/git~1'
for ARM in U H; do
  rm -rf r; git init -q -b dev r; cd r; mkdir -p crates/lib; echo s > crates/lib/x; git add -A; git commit -qm s
  git checkout -q --orphan mock/research; git rm -rqf .; mkdir sketch; echo data > sketch/N.md; git add -A; git commit -qm data; git checkout -q dev
  w=$(git worktree add -q "$D" mock/research 2>&1 | head -1); [ $ARM = H ] && echo "/$D/" >> .git/info/exclude
  echo EDIT >> "$D/sketch/N.md"; echo new > main.rs
  echo "$ARM worktree add {$w} status=[$(git status --short 2>&1 | tr '\n' ' ')]"
  echo "$ARM add -A: {$(git add -A 2>&1 | head -1)} staged=[$(git diff --cached --name-only | tr '\n' ' ')]"
  echo "$ARM stash -u: {$(git stash -u 2>&1 | head -1)} edit=$(grep -c EDIT "$D/sketch/N.md")"; git stash pop -q 2>/dev/null
  echo "$ARM relative path out of the data: $(cd "$D/sketch" && cat ../../../crates/lib/x)"
  cd ..
done
