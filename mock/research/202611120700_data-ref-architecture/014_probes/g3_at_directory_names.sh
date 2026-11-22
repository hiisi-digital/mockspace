#!/bin/bash
# Spike g3: 012 u1 showed a tracked directory named `@` collides with git's alias for HEAD. Which physical names
# for the git-tree shadow directory are free of revision syntax? For each candidate, inside a mock-tree worktree
# that tracks <name>/crates/foo/README.md.tmpl with an uncommitted edit: does `git diff <name>`, `git log <name>`
# and `git checkout <name>` (no `--`) treat it as the path, and does the edit survive `checkout <name>`?
set -u
exec </dev/null
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
for N in '@' '@@' '@git' '@tree' '_git' '=git' 'git-tree'; do
  rm -rf r; git init -q -b dev r; cd r; mkdir -p "$N/crates/foo"; echo t > "$N/crates/foo/README.md.tmpl"; git add -A; git commit -qm s
  echo EDIT >> "$N/crates/foo/README.md.tmpl"
  d=$(git diff --stat "$N" 2>&1 | tail -1 | cut -c1-60); l=$(git log --oneline "$N" 2>&1 | head -1 | cut -c1-60)
  c=$(git checkout "$N" 2>&1 | head -1 | cut -c1-60); k=$(grep -c EDIT "$N/crates/foo/README.md.tmpl"); h=$(git symbolic-ref -q HEAD || echo DETACHED)
  printf '%-9s diff={%s} log={%s} checkout={%s} edit-after-checkout=%s HEAD=%s\n' "[$N]" "$d" "$l" "$c" "$k" "$h"
  cd ..
done
