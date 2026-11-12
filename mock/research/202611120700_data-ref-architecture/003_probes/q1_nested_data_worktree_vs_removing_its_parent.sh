#!/bin/bash
# Spike: my seat-001 proposal nests the data worktree inside each main worktree (hidden by exclude).
# Attack on it from seat 002 p6: commands that delete ignored content of a worktree. Does a dirty nested
# data worktree survive `git worktree remove` of its parent linked worktree, `git clean -fdX`, `-fdx`?
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
setup() {
  rm -rf "$R/m" "$R/w2"; git init -q -b dev "$R/m"; cd "$R/m"; echo s > main.rs; git add -A; git commit -qm s
  git checkout -q --orphan mock/research; git rm -rqf .; echo a > NOTES.md; git add -A; git commit -qm data; git checkout -q dev
  git worktree add -q -b feat "$R/w2" dev; cd "$R/w2"
  git worktree add -q mock/data/research mock/research 2>&1 | head -1   # branch name reuse is fine: m has no data worktree
  echo mock/data/ >> "$(git rev-parse --git-common-dir)/info/exclude"
  echo EDIT >> mock/data/research/NOTES.md
}
setup; cd "$R/m"; git worktree remove "$R/w2" 2>&1 | head -2; echo "worktree remove (no force): parent exists=$([ -d $R/w2 ] && echo yes || echo no)"
setup; cd "$R/w2"; git clean -fdX 2>&1 | head -2; echo "clean -fdX: edit survives=$(grep -c EDIT mock/data/research/NOTES.md 2>/dev/null)"
setup; cd "$R/w2"; git clean -fdx 2>&1 | head -2; echo "clean -fdx: edit survives=$(grep -c EDIT mock/data/research/NOTES.md 2>/dev/null)"
setup; cd "$R/m"; git worktree remove --force "$R/w2" 2>&1 | head -1; echo "worktree remove --force: parent exists=$([ -d $R/w2 ] && echo yes || echo no)"
