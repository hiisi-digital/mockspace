#!/bin/bash
# 009/p2: (a) which directory names git itself refuses to track, at every way a commit can get a path in (add, add -f,
# update-index, a crafted tree checked out with checkout -f and reset --hard, and git push receive with fsck);
# (b) the audit's unprobed claim that a TRACKED .gitignore line stops `git add -A` pollution in a clone that never ran the engine.
set -u
S=${1:?scratch}; rm -rf $S/p2; mkdir -p $S/p2 && cd $S/p2
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
echo "== (a) per candidate name N: add -f of N/x, update-index of N/x, crafted tree checkout -f"
for N in 'git~1' 'GIT~1' '.git.' '.git ' '.GIT' 'git~2' 'git~' 'gi~1' 'research'; do
  rm -rf r; git init -q -b dev r; cd r; echo c > c.rs; git add -A; git commit -qm i
  mkdir -p "$N" 2>/dev/null && echo x > "$N/x" 2>/dev/null; made=$([ -f "$N/x" ] && echo yes || echo no)
  a=$(git add -f -- "$N/x" 2>&1 | head -1); [ -z "$a" ] && a=accepted
  B=$(printf x | git hash-object -w --stdin)
  u=$(git update-index --add --cacheinfo 100644,$B,"$N/x" 2>&1 | head -1); [ -z "$u" ] && u=accepted
  # crafted tree: a tree object with an entry named N, then checkout -f of a commit holding it
  SUB=$(printf '100644 x\0' | { cat; printf "$(echo $B | sed 's/../\\x&/g')"; } | git hash-object -t tree -w --stdin --literally 2>/dev/null)
  TOP=$(printf '040000 %s\0' "$N" | { cat; printf "$(echo $SUB | sed 's/../\\x&/g')"; } | git hash-object -t tree -w --stdin --literally 2>/dev/null)
  C=$(git commit-tree $TOP -m crafted 2>/dev/null); co=n/a
  if [ -n "$C" ]; then git checkout -q -f $C 2>/tmp/co.err && co=accepted || co="refused: $(head -1 /tmp/co.err | cut -c1-70)"; fi
  printf '%-10s dir creatable=%-3s add -f: %-48.48s | update-index: %-44.44s | checkout -f crafted: %s\n' "$N" "$made" "$a" "$u" "$co"; cd ..
done
echo "== (a2) the same with core.protectNTFS=false (a person who turned it off)"
rm -rf r; git init -q -b dev r; cd r; git config core.protectNTFS false; echo c > c.rs; git add -A; git commit -qm i; mkdir 'git~1'; echo x > 'git~1/x'; git add -f 'git~1/x' 2>&1 | head -1; echo "rc after add -f with protectNTFS=false: $(git ls-files | grep -c 'git~1')"; cd ..
echo "== (b) tracked .gitignore on the code branch, fresh clone that never ran the engine, an agent writes research at the path"
rm -rf o.git s c; git init -q --bare o.git; git init -q -b dev s && cd s && echo c > c.rs && printf 'mock/study/\n' > .gitignore && git add -A && git commit -qm i && git remote add o ../o.git && git push -q o dev; cd ..
git clone -q -b dev o.git c; cd c; mkdir -p mock/study; echo AGENT > mock/study/N.md
git add -A; echo "add -A staged: [$(git diff --cached --name-only | tr '\n' ' ')]"
git add mock/study/N.md 2>&1 | head -1
git add -f mock/study/N.md; echo "after add -f staged: [$(git diff --cached --name-only | tr '\n' ' ')]"
