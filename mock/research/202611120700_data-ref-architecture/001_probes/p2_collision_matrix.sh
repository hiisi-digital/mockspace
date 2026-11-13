#!/bin/bash
# Spike: where does an uncommitted edit in a nested data worktree survive plain git moves of the
# outer branch to a commit that tracked files at the same path? Variants: hidden by info/exclude,
# not hidden at all, and data dir at a path no history ever tracked (control).
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
run() { # name, datapath, exclude(0/1), movecmd...
  local name=$1 dp=$2 ex=$3; shift 3
  rm -rf "$R/m"; git init -q -b dev "$R/m"; cd "$R/m"
  mkdir -p mock/research; echo old > mock/research/NOTES.md; echo src > main.rs
  git add -A; git commit -qm old; OLD=$(git rev-parse HEAD)
  git rm -rq mock/research; git commit -qm migrate
  DC=$(git commit-tree $OLD:mock/research -m data 2>/dev/null || git commit-tree "$OLD:mock/research" -m data < /dev/null)
  git branch mock/research $DC
  mkdir -p "$(dirname $dp)"; git worktree add -q "$dp" mock/research
  [ "$ex" = 1 ] && echo "$dp/" >> .git/info/exclude
  echo "EDIT" >> "$dp/NOTES.md"
  out=$(git "${@//OLD/$OLD}" 2>&1); rc=$?
  printf '%-34s rc=%s edit_survives=%s  %s\n' "$name" "$rc" "$(grep -c EDIT "$dp/NOTES.md" 2>/dev/null)" "$(echo "$out" | head -2 | tr '\n' ' ' | cut -c1-110)"
}
for ex in 1 0; do
  run "same path ex=$ex checkout OLD"       mock/research $ex checkout OLD
  run "same path ex=$ex checkout -f OLD"    mock/research $ex checkout -f OLD
  run "same path ex=$ex reset --hard OLD"   mock/research $ex reset --hard OLD
  run "same path ex=$ex switch --detach OLD" mock/research $ex switch --detach OLD
  run "same path ex=$ex stash -u"           mock/research $ex stash -u
done
run "other path ex=1 checkout -f OLD"       mock/.data/research 1 checkout -f OLD
run "other path ex=1 reset --hard OLD"      mock/.data/research 1 reset --hard OLD
