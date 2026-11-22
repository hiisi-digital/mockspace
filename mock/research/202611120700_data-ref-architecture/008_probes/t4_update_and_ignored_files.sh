#!/bin/bash
# Audit finding: update = git pull overwrites an ignored local file that an incoming commit tracks.
# (1) confirm; (2) merge --no-overwrite-ignore refuses; (3) a snapshot that holds ignored files.
source "$(dirname "$0")/common.sh"; R=${1:?}; rm -rf "$R"; mkdir -p "$R"
git init -q --bare "$R/remote.git"
mk "$R/a"; git worktree add -q mock/rdata data; cd mock/rdata; echo 'output.txt' > .gitignore; git add .gitignore; git commit -qm ignore; git push -q "$R/remote.git" HEAD:refs/heads/x
cd "$R"; git clone -q -b x "$R/remote.git" b; cd b; echo theirs > output.txt; git add -f output.txt; git commit -qm "track output.txt"; git push -q origin x
cd "$R/a/mock/rdata"; echo "LOCAL UNIQUE" > output.txt; git remote add o "$R/remote.git"; git fetch -q o x
echo "== merge --no-overwrite-ignore"; git merge --no-overwrite-ignore o/x 2>&1 | head -2; echo "local file: $(cat output.txt)"
echo "== snapshot recipe (copied index, add -A) holds ignored files?"; export GIT_INDEX_FILE=$R/ti; cp "$(git rev-parse --git-dir)/index" $GIT_INDEX_FILE; git add -A; echo "ignored file in tree: $(git ls-tree -r --name-only $(git write-tree) | grep -c output.txt)"
git ls-files -o -i --exclude-standard -z | xargs -0 -r git add -f --; echo "after add -f of ls-files -o -i: $(git ls-tree -r --name-only $(git write-tree) | grep -c output.txt)"; unset GIT_INDEX_FILE
echo "== plain git pull (no --no-overwrite-ignore option exists in this git):"; git pull --no-rebase o x 2>&1 | head -2; echo "local file after pull: $(cat output.txt)"
