#!/bin/bash
# Audit finding: an unmapped gitlink at the data path (what `git add -A` records when the nested worktree is
# unhidden) breaks `git submodule update` for everyone. Re-run, with the hidden-by-tracked-.gitignore control.
source "$(dirname "$0")/common.sh"; R=${1:?}; rm -rf "$R"; mkdir -p "$R"
git init -q -b main "$R/lib"; (cd "$R/lib"; echo l > l.rs; git add -A; git commit -qm l)
for arm in U T; do
  mk "$R/m"; git -c protocol.file.allow=always submodule add -q "$R/lib" lib 2>&1 | head -1; git commit -qm submodule
  [ $arm = T ] && { echo /mock/rdata/ > .gitignore; git add .gitignore; git commit -qm ignore; }
  git worktree add -q mock/rdata data; git add -A 2>/dev/null; git commit -qm sweep 2>/dev/null
  echo "$arm: tree holds gitlink at data path: [$(git ls-tree HEAD mock/rdata | cut -c1-30)]"
  git clone -q "$R/m" "$R/c$arm" 2>&1 | head -1; (cd "$R/c$arm"; git -c protocol.file.allow=always submodule update --init >"$R/o" 2>&1; rc=$?; tail -1 "$R/o" | cut -c1-90; echo "  rc=$rc lib populated=$([ -f lib/l.rs ] && echo yes || echo no)")
done
