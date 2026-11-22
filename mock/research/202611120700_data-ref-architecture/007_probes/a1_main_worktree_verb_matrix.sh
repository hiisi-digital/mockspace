#!/bin/bash
# Spike a1: the converged placement (data worktree nested in the main worktree at a same-depth path that no
# commit has tracked) against every tree-writing git verb I could enumerate, run in the MAIN worktree.
# Three arms: U = unhidden (the converged shape of 005/006), H = hidden by the common info/exclude,
# T = hidden by a line in the code branch's own TRACKED .gitignore (committed on dev right after MIG).
# State before each verb: data/N.md carries an uncommitted EDIT, data/U.md is a new untracked file.
# Kept = both still present with their bytes; wt = the data worktree still answers on its branch.
# History: OLD (pre-migration, tracks mock/research/N.md), oldbranch (OLD + an edit to the old path),
# MIG on dev (old path removed), polluted (dev + a REGULAR file at the new data path, the case a clone
# without the hook or a by-hand commit produces), gitlinked (dev + a 160000 entry at the new data path).
set -u
S=${1:?scratch}; mkdir -p "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_EDITOR=true
export GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
D=mock/rdata
setup() { # $1 dir, $2 arm
  rm -rf "$1"; git init -q -b dev "$1"; cd "$1"
  echo s > main.rs; mkdir -p mock/research; echo old > mock/research/N.md; git add -A; git commit -qm OLD; git tag OLD
  git checkout -q -b oldbranch; echo oldedit > mock/research/N.md; git commit -qam oldedit; git checkout -q dev
  git rm -rq mock/research; git commit -qm MIG
  [ "$2" = T ] && { echo "/$D/" >> .gitignore; git add .gitignore; git commit -qm ignore-data-path; }
  git checkout -q --orphan mock/research; git rm -rqf . ; echo data > N.md; git add -A; git commit -qm data; git checkout -q dev
  # polluted: dev + regular file at the data path, built without touching the work tree
  T=$(GIT_INDEX_FILE=$1/.git/pidx sh -c "git read-tree dev && echo pollute | git hash-object -w --stdin | xargs -I{} git update-index --add --cacheinfo 100644,{},$D/N.md && git write-tree")
  git branch polluted $(git commit-tree -p dev -m polluted $T); rm -f .git/pidx
  T=$(GIT_INDEX_FILE=$1/.git/gidx sh -c "git read-tree dev && git update-index --add --cacheinfo 160000,$(git rev-parse mock/research),$D && git write-tree")
  git branch gitlinked $(git commit-tree -p dev -m gitlinked $T); rm -f .git/gidx
  git worktree add -q $D mock/research
  [ "$2" = H ] && echo "/$D/" >> .git/info/exclude
  echo EDIT >> $D/N.md; echo untracked > $D/U.md
}
verdict() {
  local k=0; grep -q EDIT $D/N.md 2>/dev/null && [ -f $D/U.md ] && k=1
  local w; w=$(git -C $D rev-parse --abbrev-ref HEAD 2>/dev/null); [ "$(cd $D 2>/dev/null && git rev-parse --show-toplevel 2>/dev/null)" = "$PWD/$D" ] || w="NOT-A-WORKTREE"
  printf '%-4s %-48s kept=%s wt=%s\n' "$ARM" "$CASE" "$k" "${w:-none}"
}
run() { # $1 case label, rest: shell snippet
  CASE=$1; shift; setup "$S/r" "$ARM" >/dev/null 2>&1; ( eval "$*" ) >"$S/last.log" 2>&1; verdict
}
for ARM in U H T; do
run "checkout OLD; checkout dev"                 'git checkout -q OLD; git checkout -q dev'
run "checkout -f OLD; checkout -f dev"           'git checkout -qf OLD; git checkout -qf dev'
run "reset --hard OLD; reset --hard dev"         'git reset -q --hard OLD; git reset -q --hard dev@{1}'
run "reset --hard"                               'git reset -q --hard'
run "reset --keep OLD; reset --keep dev@{1}"     'git reset -q --keep OLD; git reset -q --keep dev@{1}'
run "reset --merge OLD"                          'git reset -q --merge OLD'
run "switch --detach OLD; switch dev"            'git switch -q --detach OLD; git switch -q dev'
run "switch --discard-changes OLD and back"      'git switch -q --discard-changes --detach OLD; git switch -q --discard-changes dev'
run "switch -C dev OLD"                          'git switch -q -C dev OLD'
run "checkout OLD -- ."                          'git checkout OLD -- .'
run "checkout -- . / checkout ."                 'git checkout -- .'
run "restore --source OLD -SW ."                 'git restore --source OLD -SW .'
run "restore -SW ."                              'git restore -SW .'
run "restore --source OLD -W :/"                 'git restore --source OLD -W :/'
run "stash -u; stash pop"                        'git stash -u; git stash pop'
run "stash -a; stash pop"                        'git stash -a; git stash pop'
run "stash push -u -- $D"                        "git stash push -u -- $D"
run "clean -fd"                                  'git clean -fd'
run "clean -fdx"                                 'git clean -fdx'
run "clean -fdX"                                 'git clean -fdX'
run "clean -ffd"                                 'git clean -ffd'
run "clean -ffdX"                                'git clean -ffdX'
run "clean -ffdx"                                'git clean -ffdx'
run "merge oldbranch"                            'git merge oldbranch; git merge --abort'
run "merge oldbranch -X theirs; reset --hard"    'git merge -X theirs oldbranch; git reset -q --hard'
run "cherry-pick oldbranch; abort"               'git cherry-pick oldbranch; git cherry-pick --abort'
run "revert MIG"                                 'git revert --no-edit HEAD; git reset -q --hard HEAD~1'
run "rebase --onto OLD (dev replayed)"           'git rebase --onto OLD HEAD~1; git rebase --abort; git reset -q --hard dev'
run "rebase -i root (autosquash, no-op)"         'GIT_SEQUENCE_EDITOR=true git rebase -qi --root'
run "am of an old-path patch"                    'git format-patch -1 oldbranch --stdout > ../p.mbox; git am -3 ../p.mbox; git am --abort'
run "bisect start dev OLD; reset"                'git bisect start dev OLD; git bisect reset'
run "read-tree -u --reset OLD and back"          'git read-tree -u --reset OLD; git read-tree -u --reset dev'
run "checkout-index -a -f"                       'git checkout-index -a -f'
run "sparse-checkout set main.rs; disable"       'git sparse-checkout set --no-cone main.rs; git sparse-checkout disable'
run "rm -r --cached .; reset"                    'git rm -rq --cached .; git reset -q'
run "gc --prune=now --aggressive"                'git gc -q --prune=now --aggressive'
run "worktree prune"                             'git worktree prune'
run "add -A; commit (gitlink lands on dev)"      'git add -A; git commit -qm pollute; git checkout -qf HEAD~1; git checkout -qf dev'
run "checkout gitlinked; checkout dev"           'git checkout -q gitlinked; git checkout -q dev'
run "checkout -f gitlinked; checkout -f dev"     'git checkout -qf gitlinked; git checkout -qf dev'
run "checkout polluted (regular file at path)"   'git checkout -q polluted; git checkout -q dev'
run "checkout -f polluted; checkout -f dev"      'git checkout -qf polluted; git checkout -qf dev'
run "reset --hard polluted; reset --hard dev"    'git reset -q --hard polluted; git reset -q --hard dev@{1}'
run "merge polluted"                             'git merge polluted'
run "submodule update --init --force (gitlinked)" 'git checkout -qf gitlinked; git submodule update --init --force; git checkout -qf dev'
done
