#!/bin/bash
# Audit finding: a commit from another clone that tracks a REGULAR file at the data path. Arms: U unhidden,
# H hidden by exclude, T hidden by a tracked .gitignore line on dev, and N the data path named git~1 (refused
# by core.protectNTFS). Receiving clone has an uncommitted edit in the nested data worktree, then merges/
# checks out the polluted commit.
source "$(dirname "$0")/common.sh"; R=${1:?}; rm -rf "$R"; mkdir -p "$R"
run() { arm=$1; dp=$2; verb=$3
  mk "$R/m"; [ $arm = T ] && { echo "/$dp/" > .gitignore; git add .gitignore; git commit -qm ignore; }
  git branch polluted dev; git checkout -q polluted; mkdir -p "$dp"; echo POLLUTION > "$dp/N.md"
  git add -f "$dp/N.md" 2>"$R/e"; r=$?; git commit -qm "track a file at the data path" 2>/dev/null; git checkout -q dev
  if [ $r -ne 0 ] || ! git cat-file -e polluted:"$dp/N.md" 2>/dev/null; then echo "$arm $verb: the polluting commit COULD NOT BE MADE: $(head -1 $R/e | cut -c1-80)"; return; fi
  git worktree add -q "$dp" data 2>/dev/null; [ $arm = H ] && echo "$dp/" >> .git/info/exclude
  echo EDIT >> "$dp/N.md"
  out=$(git $verb 2>&1 | head -1 | cut -c1-70); printf '%s %-22s edit survives=%s data file=%s  (%s)\n' $arm "$verb" "$(grep -c EDIT $dp/N.md 2>/dev/null; true)" "$(tr '\n' '|' < $dp/N.md 2>/dev/null)" "$out"
}
for arm in U H T; do for v in "merge polluted" "checkout -q polluted" "checkout -qf polluted" "reset -q --hard polluted"; do run $arm mock/rdata "$v"; done; done
for v in "merge polluted" "checkout -qf polluted"; do run N 'mock/git~1' "$v"; done
echo "== git~1 refused at the other doors too"
mk "$R/m"; mkdir -p 'mock/git~1'; echo x > 'mock/git~1/N.md'; git add 'mock/git~1/N.md' 2>&1 | head -1; git update-index --add 'mock/git~1/N.md' 2>&1 | head -1
echo "== a crafted tree (mktree) holding it, checked out -f / merged"; B=$(echo x | git hash-object -w --stdin); IN=$(printf '100644 blob %s\tN.md\n' $B | git mktree); MK=$(printf '040000 tree %s\tgit~1\n' $IN | git mktree); MR=$(printf '040000 tree %s\tmock\n' $MK | git mktree)
CC=$(git commit-tree $MR -p HEAD -m crafted); git branch crafted $CC; git checkout -qf crafted 2>&1 | head -1; git checkout -q dev 2>/dev/null; git merge crafted 2>&1 | head -2
echo "crafted merge wrote: $(ls mock/git~1 2>&1 | head -1)"
