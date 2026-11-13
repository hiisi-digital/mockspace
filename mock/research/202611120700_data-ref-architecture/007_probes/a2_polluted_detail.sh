#!/bin/bash
# Spike a2: detail for the a1 rows where a branch tracks a regular file at the new data path. Shows the bytes
# of the data worktree's N.md and U.md after each step, for the unhidden (U) and hidden (H) arms.
set -u
S=${1:?scratch}; mkdir -p "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
D=mock/rdata
for ARM in U H; do for V in "checkout" "checkout -f" "reset --hard"; do
  rm -rf "$S/r"; git init -q -b dev "$S/r"; cd "$S/r"; echo s > main.rs; git add -A; git commit -qm s
  git checkout -q --orphan mock/research; git rm -rqf .; echo data > N.md; git add -A; git commit -qm data; git checkout -q dev
  # a branch from another clone that never instantiated the data worktree and committed a file there by hand
  git checkout -q -b polluted; mkdir -p $D; echo pollute > $D/N.md; git add $D; git commit -qm polluted; git checkout -q dev; rm -rf $D
  git worktree add -q $D mock/research; [ $ARM = H ] && echo "/$D/" >> .git/info/exclude
  echo EDIT >> $D/N.md; echo untracked > $D/U.md
  out=$(git $V polluted 2>&1 | head -2 | tr '\n' ' ')
  a="N=[$(tr '\n' ' ' < $D/N.md 2>/dev/null || echo GONE)] U=[$(cat $D/U.md 2>/dev/null || echo GONE)]"
  out2=$(git $V dev 2>&1 | head -1 | tr '\n' ' ')
  b="N=[$(tr '\n' ' ' < $D/N.md 2>/dev/null || echo GONE)] U=[$(cat $D/U.md 2>/dev/null || echo GONE)] data-status=[$(git -C $D status --short 2>&1 | tr '\n' ' ')]"
  echo "$ARM git $V polluted: msg={$out} -> $a"
  echo "$ARM git $V dev     : msg={$out2} -> $b"
done; done
