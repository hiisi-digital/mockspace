#!/bin/bash
# Spike: data dir as a nested linked worktree at a path no history tracked (mock/data/research).
# (1) what git clean / worktree remove / prune do to dirty data; (2) a recovery snapshot built by git
# itself (temporary index + add -A + write-tree) applies the clean filter (LFS-like) and eol rules, and
# is held by a ref; the engine's own hash-object --no-filters path does not.
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev m; cd m; echo s > main.rs; git add -A; git commit -qm s
git checkout -q --orphan mock/research; git rm -rqf .; echo a > NOTES.md; echo '*.big filter=ptr' > .gitattributes; git add -A; git commit -qm data; git checkout -q dev
git config filter.ptr.clean 'sh -c "echo version-ptr; echo oid-$(sha1sum | cut -c1-8)"'; git config filter.ptr.smudge cat
git worktree add -q mock/data/research mock/research; echo mock/data/ >> .git/info/exclude
cd mock/data/research; echo EDIT >> NOTES.md; printf 'BIGBYTES\n' > m.big; cd ../../..
echo "== (1a) git clean -fdx";  git clean -fdx 2>&1 | head -3; ls mock/data/research | tr '\n' ' '; echo
echo "== (1b) git clean -ffdx"; git clean -ffdx 2>&1 | head -3; ls mock/data 2>&1 | tr '\n' ' '; echo
git worktree prune; git worktree list | sed 's#.*/scratchpad/001/p6/##'
git worktree add -q mock/data/research mock/research 2>&1 | head -1
cd mock/data/research; echo EDIT >> NOTES.md; printf 'BIGBYTES\n' > m.big; cd ../../..
echo "== (1c) git worktree remove (dirty, no force)"; git worktree remove mock/data/research 2>&1 | head -2
echo "== (2) snapshot via temp index"
cd mock/data/research
export GIT_INDEX_FILE=$(mktemp -u); git read-tree HEAD; git add -A; T=$(git write-tree); unset GIT_INDEX_FILE
C=$(git commit-tree $T -p HEAD -m snapshot); git update-ref refs/mock-backup/research/wt1 $C
git show --stat --format=%s $C | tail -4; echo "m.big in snapshot: $(git show $C:m.big | tr '\n' '|')"
echo "with --no-filters (engine path): $(git hash-object --no-filters m.big | cut -c1-8) vs filtered: $(git hash-object m.big | cut -c1-8)  (snapshot blob: $(git rev-parse $C:m.big | cut -c1-8))"
