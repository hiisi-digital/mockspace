#!/bin/bash
# Spike on the recovery snapshot both seats propose (temporary index + add -A + write-tree). A fresh
# temporary index (read-tree HEAD) carries no stat data, so add -A rehashes every file. A copy of the
# worktree's real index carries it. Wall time, 3000 files of 20 KB.
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b d r; cd r; mkdir f; for i in $(seq 3000); do head -c 20000 /dev/urandom > f/$i; done
git add -A; git commit -qm x; echo EDIT >> f/1
t() { local s=$(date +%s.%N); "$@" >/dev/null; echo "$(echo "$(date +%s.%N) - $s" | bc) s"; }
snap_fresh() { export GIT_INDEX_FILE=$R/tmp1; rm -f $GIT_INDEX_FILE; git read-tree HEAD; git add -A; git write-tree; unset GIT_INDEX_FILE; }
snap_copy()  { export GIT_INDEX_FILE=$R/tmp2; cp .git/index $GIT_INDEX_FILE; git add -A; git write-tree; unset GIT_INDEX_FILE; }
git status -s >/dev/null
echo "fresh temp index (read-tree HEAD): $(t snap_fresh)"
echo "copy of the real index:            $(t snap_copy)"
echo "trees equal: $([ "$(snap_fresh)" = "$(snap_copy)" ] && echo yes || echo no)"
