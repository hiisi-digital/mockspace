#!/bin/bash
# Audit finding: worktree lock (parent and data), move, prune. Reproduce the claims, then the full verb set.
source "$(dirname "$0")/common.sh"; R=${1:?}; rm -rf "$R"; mkdir -p "$R"
setup() { rm -rf "$R/w2" "$R/w3" "$R/w4"; mk "$R/m"; git worktree add -q -b feat "$R/w2" dev; (cd "$R/w2"; git worktree add -q mock/rdata data; echo EDIT >> mock/rdata/N.md); }
setup; git worktree lock "$R/w2" --reason "holds data"; for f in "" "--force"; do git worktree remove $f "$R/w2" 2>&1 | head -1 | cut -c1-80; echo "remove $f: parent exists=$([ -d $R/w2 ] && echo yes || echo no)"; done
git worktree remove -f -f "$R/w2" 2>&1 | head -1; echo "remove -f -f: parent exists=$([ -d $R/w2 ] && echo yes || echo no)  (the user said it twice)"
setup; git worktree lock "$R/w2"; git worktree move "$R/w2" "$R/w3" 2>&1 | head -1 | cut -c1-90
echo "== unlocked parent, unlocked data: move then prune"; setup; git worktree move "$R/w2" "$R/w3"; (cd "$R/w3"; git worktree list | tail -2 | sed "s#$R/##"; git worktree prune -v 2>&1 | head -2 | sed "s#$R/##")
echo "== unlocked parent, LOCKED data worktree: move then prune"; setup; (cd "$R/w2"; git worktree lock mock/rdata --reason d); git worktree move "$R/w2" "$R/w3" 2>&1 | head -1 | cut -c1-100; git worktree prune -v 2>&1 | head -2; echo "admin dir kept: $(ls "$(git rev-parse --git-common-dir)/worktrees")"
echo "== mv clone, repair"; setup; mv "$R/w2" "$R/w4"; (cd "$R/m"; git worktree repair "$R/w4" "$R/w4/mock/rdata" 2>&1 | head -2 | sed "s#$R/##"); (cd "$R/w4/mock/rdata"; git status --short | head -2; echo "status rc=$?")
