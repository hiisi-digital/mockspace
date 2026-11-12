#!/bin/bash
# 006/p2: reply to 001 q3. Snapshot a data worktree through a temporary index that starts as (a) empty/read-tree HEAD,
# (b) a copy of the worktree's own index. Cost at 3000 x 20KB (a spike with two arms, prices nothing), and the correctness
# trap of (b): an edit of the same size inside the same second as the real index write (git's racy case).
set -u
S=${1:?scratch}; rm -rf $S/r2; mkdir -p $S/r2 && cd $S/r2
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q -b d w && cd w
for i in $(seq 1 3000); do head -c 20000 /dev/urandom > f$i.bin; done
git add -A && git commit -qm i
ms() { local a=$(date +%s%N); "$@" >/dev/null 2>&1; echo $(( ($(date +%s%N)-a)/1000000 )); }
T=$(mktemp -u); a() { GIT_INDEX_FILE=$T git read-tree HEAD && GIT_INDEX_FILE=$T git add -A && GIT_INDEX_FILE=$T git write-tree; rm -f $T; }
b() { cp .git/index $T && GIT_INDEX_FILE=$T git add -A && GIT_INDEX_FILE=$T git write-tree; rm -f $T; }
echo "fresh temp index (read-tree HEAD), ms: $(ms a) $(ms a) $(ms a)"
echo "copied worktree index, ms:            $(ms b) $(ms b) $(ms b)"
[ "$(a 2>/dev/null | tail -1)" = "$(b 2>/dev/null | tail -1)" ] && echo "trees equal: yes" || echo "trees equal: NO"
echo "== racy case: same-size edit in the same second as the index write"
printf 'AAAA' > r.txt; git add r.txt; git commit -qm r; git update-index --refresh -q
printf 'BBBB' > r.txt   # same size, written right after the index was refreshed
tree=$(b | tail -1); echo "snapshot has r.txt = $(git cat-file -p $tree:r.txt)   (BBBB is correct; AAAA would be a stale snapshot)"
