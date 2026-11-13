#!/bin/bash
# Spike: dev tracks a gitlink (mode 160000, no .gitmodules) at the data path and the data dir is a
# nested worktree. Does git's own submodule protection stop old-commit moves from eating edits?
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
run() { local name=$1; shift
  rm -rf "$R/m"; git init -q -b dev "$R/m"; cd "$R/m"
  mkdir -p mock/research; echo old > mock/research/NOTES.md; echo src > main.rs
  git add -A; git commit -qm old; OLD=$(git rev-parse HEAD)
  git rm -rq mock/research; git commit -qm migrate-removes
  DC=$(git commit-tree "$OLD:mock/research" -m data)
  git branch mock/research $DC
  git worktree add -q mock/research mock/research
  git update-index --add --cacheinfo 160000,$DC,mock/research; git commit -qm "gitlink at data path"
  echo "EDIT" >> mock/research/NOTES.md
  out=$(git "${@//OLD/$OLD}" 2>&1); rc=$?
  printf '%-28s rc=%s edit_survives=%s  %s\n' "$name" "$rc" "$(grep -c EDIT mock/research/NOTES.md 2>/dev/null)" "$(echo "$out" | head -2 | tr '\n' ' ' | cut -c1-120)"
  git status --short | head -3
}
run "gitlink: checkout OLD"        checkout OLD
run "gitlink: checkout -f OLD"     checkout -f OLD
run "gitlink: reset --hard OLD"    reset --hard OLD
run "gitlink: switch --detach OLD" switch --detach OLD
