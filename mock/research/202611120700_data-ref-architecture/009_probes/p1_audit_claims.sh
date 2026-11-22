#!/bin/bash
# 009/p1: my own re-run of the audit's (007) claims against the converged shape: a data worktree nested in the tree at a
# same-depth path no commit tracks. U = unhidden, H = hidden by info/exclude. Independent script, not a copy of 007's a1..a11.
set -u
S=${1:?scratch}; rm -rf $S/p1; mkdir -p $S/p1 && cd $S/p1
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
mk() { rm -rf m; git init -q -b dev m && cd m; mkdir -p mock; echo c > c.rs; git add -A; git commit -qm i
  git worktree add -q --orphan -b mock/study mock/study; (cd mock/study; echo d > N.md; git add N.md; git commit -qm d)
  [ "$1" = H ] && echo mock/study >> .git/info/exclude
  echo EDIT > mock/study/N.md; }
res() { printf '%-52s %s\n' "$1" "$(cat mock/study/N.md 2>&1 | head -1)"; }
echo "== 1. clean variants (edit survives if it prints EDIT)"
for arm in U H; do for f in "-fd" "-ffd" "-fdx" "-ffdx" "-ffdX"; do mk $arm; git clean $f >/dev/null 2>&1; res "arm $arm: git clean $f"; cd ..; done; done
echo "== 2. worktree lock: does a lock stop clean -ffdx?"
mk U; git worktree lock mock/study --reason "data"; git clean -ffdxq >/dev/null 2>&1; res "locked data wt, clean -ffdx"; git worktree list | head -3; cd ..
echo "== 3. a commit from ANOTHER clone tracks a regular file at the data path, then the main tree moves onto it"
for arm in U H; do for way in "checkout -q polluted" "checkout -q -f polluted" "merge -q --no-edit polluted" "reset -q --hard polluted"; do
  mk $arm
  B=$(printf 'THEIRS' | git hash-object -w --stdin); IDX=$(mktemp -u); GIT_INDEX_FILE=$IDX git read-tree dev; GIT_INDEX_FILE=$IDX git update-index --add --cacheinfo 100644,$B,mock/study/N.md
  T=$(GIT_INDEX_FILE=$IDX git write-tree); C=$(git commit-tree $T -p dev -m "from another clone: tracks the data path"); git update-ref refs/heads/polluted $C; rm -f $IDX
  git $way >/dev/null 2>&1; printf '%-6s %-34s data N.md = %-8s ; data wt status: %s\n' "$arm" "$way" "$(cat mock/study/N.md 2>&1|head -1)" "$(git -C mock/study status --short 2>&1 | tr '\n' ' ')"; cd ..; done; done
echo "== 4. a gitlink at the data path committed (git add -A on U), then submodule verbs"
mk U; git add -A 2>/dev/null; git commit -qm "gitlink"; echo "tracked: $(git ls-tree -r HEAD | grep 160000 | cut -f2)"
git submodule status 2>&1 | head -2; echo "submodule status rc=${PIPESTATUS[0]}"; git submodule update --init 2>&1 | head -2; echo "submodule update --init rc=${PIPESTATUS[0]}"; cd ..
echo "== 5. update by git pull in the data worktree over an ignored local file"
rm -rf o.git d1 d2; git init -q --bare o.git; git init -q -b mock/study d1 && cd d1 && echo d > N.md && git add N.md && git commit -qm d && git remote add o ../o.git && git push -q o mock/study; cd ..
git clone -q -b mock/study o.git d2 2>/dev/null; cd d1; echo THEIRS > output.txt; git add output.txt; git commit -qm "track output.txt"; git push -q o mock/study; cd ../d2
echo 'output.txt' >> .git/info/exclude; echo LOCAL-GENERATED > output.txt
git pull -q --no-rebase --no-edit 2>&1 | head -2; echo "after git pull: output.txt = $(cat output.txt)"
git reset -q --hard HEAD~1; echo LOCAL-GENERATED > output.txt
git fetch -q; git merge --no-overwrite-ignore FETCH_HEAD 2>&1 | head -2; echo "after fetch + merge --no-overwrite-ignore: output.txt = $(cat output.txt)"
