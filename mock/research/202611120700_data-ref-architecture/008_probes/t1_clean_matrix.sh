#!/bin/bash
# Audit finding: clean -ffd (no x) takes an unhidden nested data worktree. Arms: U unhidden, H hidden by
# common info/exclude, L the same with the nested worktree locked. Which clean forms destroy the edit?
source "$(dirname "$0")/common.sh"; R=${1:?}; rm -rf "$R"; mkdir -p "$R"
for arm in U H; do for c in "-fd" "-fdx" "-fdX" "-ffd" "-ffdx" "-ffdX"; do
  mk "$R/m"; git worktree add -q mock/rdata data; [ $arm = H ] && echo mock/rdata/ >> .git/info/exclude
  echo EDIT >> mock/rdata/N.md; git clean $c >/dev/null 2>&1
  printf '%s clean %-6s edit survives=%s\n' $arm "$c" "$(grep -c EDIT mock/rdata/N.md 2>/dev/null; true)"
done; done
echo "== worktree lock does not stop clean:"; mk "$R/m"; git worktree add -q mock/rdata data; git worktree lock mock/rdata --reason data; echo EDIT >> mock/rdata/N.md; git clean -ffd >/dev/null 2>&1; echo "locked, clean -ffd: edit survives=$(grep -c EDIT mock/rdata/N.md 2>/dev/null; true)"
