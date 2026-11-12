#!/bin/bash
# Spike a11: verbs run INSIDE the data worktree, which every proposal turns into the engine's update. The
# incoming data commit tracks a path that is an IGNORED local file in this clone (a research crate's
# generated output.txt ignored by the data branch's .gitignore, then committed by another clone with add -f).
# Does `git pull` (merge and rebase) overwrite it silently, does --no-overwrite-ignore refuse, and does the
# temporary-index snapshot (copied index, add -A) hold the ignored file.
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git init -q --bare -b dev origin.git
git clone -q origin.git a 2>/dev/null; cd a; git checkout -q --orphan mock/research; echo 'output.txt' > .gitignore; echo n > N.md; git add -A; git commit -qm data; git push -q origin mock/research; cd ..
git clone -q -b mock/research origin.git b 2>/dev/null; cd b; echo theirs > output.txt; git add -f output.txt; git commit -qm "add -f output"; git push -q origin mock/research; cd ..
reset_a() { cd "$S/a"; git reset -q --hard origin/mock/research@{1} 2>/dev/null || git reset -q --hard HEAD; git fetch -q origin; git reset -q --hard $(git rev-list --max-parents=0 HEAD | head -1); echo MINE-UNIQUE > output.txt; }
for c in "pull --no-rebase origin mock/research" "pull --rebase origin mock/research" "merge --no-overwrite-ignore FETCH_HEAD" "pull --no-rebase --no-overwrite-ignore origin mock/research" "checkout -f FETCH_HEAD"; do
  reset_a; git fetch -q origin mock/research; r=$(git $c 2>&1 | grep -v "^ *$" | head -1); rc=$(git status >/dev/null; echo); echo "data wt: git $c -> output.txt=[$(cat output.txt)] {$r}"
done
reset_a; cp "$(git rev-parse --git-path index)" "$S/tmpidx"; T=$(GIT_INDEX_FILE="$S/tmpidx" sh -c 'git add -A && git write-tree'); echo "snapshot tree holds output.txt: $(git ls-tree $T output.txt | wc -l)"
cp "$(git rev-parse --git-path index)" "$S/tmpidx"; T=$(GIT_INDEX_FILE="$S/tmpidx" sh -c 'git add -A && git ls-files -z -o -i --exclude-standard | xargs -0 -r git add -f && git write-tree'); echo "snapshot with ignored added: $(git ls-tree $T output.txt | wc -l)"
