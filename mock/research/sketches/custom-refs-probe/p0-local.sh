#!/usr/bin/env bash
# P0, local leg: do custom ref trees behave as a data home outside branches?
# Every step prints PASS/FAIL with what it saw. No network: the "remote" is a bare repo on disk.
set -u
export GIT_TERMINAL_PROMPT=0 GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=push.negotiate GIT_CONFIG_VALUE_0=false
export GIT_AUTHOR_NAME=probe GIT_AUTHOR_EMAIL=probe@local GIT_COMMITTER_NAME=probe GIT_COMMITTER_EMAIL=probe@local
root=${1:?usage: p0-local.sh <empty scratch dir>}
rm -rf "$root"; mkdir -p "$root"; cd "$root"
pass() { printf 'PASS  %s\n' "$1"; }
fail() { printf 'FAIL  %s\n' "$1"; }
check() { local what=$1; shift; if "$@" >/dev/null 2>&1; then pass "$what"; else fail "$what"; fi; }
notcheck() { local what=$1; shift; if "$@" >/dev/null 2>&1; then fail "$what"; else pass "$what"; fi; }

# the trunk: code in mock/crates, data beside it in mock/registry and mock/research
git init -q --bare remote.git
git init -q -b dev trunk && cd trunk
mkdir -p mock/crates/a/src mock/registry/ticket/items mock/research/n mock/lints
echo 'fn main(){}' > mock/crates/a/src/lib.rs
echo 'workspace' > mock/Cargo.toml
echo 'id = "t1"' > mock/registry/ticket/items/t1.toml
echo 'id = "t2"' > mock/registry/ticket/items/t2.toml
echo 'notes' > mock/research/n/NOTES.md
echo 'lint' > mock/lints/x.rs
git add -A && git commit -q -m 'trunk: code and data together'
git remote add origin ../remote.git

# 1. build two parentless ref trees from subtrees, no checkout needed
reg=$(git commit-tree -m 'registry: seed' "$(git rev-parse HEAD:mock/registry)")
git update-ref refs/mock/registry "$reg"
rest_tree=$(
  idx=$(mktemp); GIT_INDEX_FILE=$idx git read-tree HEAD
  GIT_INDEX_FILE=$idx git rm -r -q --cached mock/crates mock/Cargo.toml mock/registry
  GIT_INDEX_FILE=$idx git write-tree; rm -f "$idx")
rest=$(git commit-tree -m 'rest: seed' "$rest_tree")
git update-ref refs/mock/rest "$rest"
check 'registry ref root is the registry directory' test "$(git ls-tree --name-only refs/mock/registry | tr '\n' ' ')" = 'ticket '
check 'rest ref keeps the mock/ shape without crates and registry' test "$(git ls-tree -r --name-only refs/mock/rest | tr '\n' ' ')" = 'mock/lints/x.rs mock/research/n/NOTES.md '
notcheck 'refs/mock/* are not branches' bash -c 'git branch -a | grep -q mock'
check 'refs/mock/* are listed by for-each-ref' bash -c 'git for-each-ref refs/mock | grep -q registry'

# 2. push with an explicit refspec; default push does not carry them
git push -q origin dev
check 'positive control: the default push did land dev' git --git-dir=../remote.git rev-parse refs/heads/dev
notcheck 'a default push does not carry refs/mock' bash -c 'git --git-dir=../remote.git for-each-ref refs/mock | grep -q .'
git push -q origin refs/mock/registry:refs/mock/registry refs/mock/rest:refs/mock/rest
check 'an explicit refspec push lands both refs' bash -c 'test "$(git --git-dir=../remote.git for-each-ref refs/mock | wc -l)" = 2'
notcheck 'the remote lists them under branches' bash -c 'git ls-remote --heads origin | grep -q mock'

# 3. a clone: default, shallow, and with the refspec
cd "$root"
git clone -q -b dev --depth 1 file://"$root"/remote.git c-shallow 2>/dev/null
notcheck 'a default shallow clone has no refs/mock' bash -c 'cd c-shallow && git for-each-ref refs/mock | grep -q .'
(cd c-shallow && git fetch -q --depth 1 origin '+refs/mock/*:refs/mock/*' 2>&1)
check 'a shallow fetch with the refspec brings both refs' bash -c 'cd c-shallow && test "$(git for-each-ref refs/mock | wc -l)" = 2'
check 'the shallow ref tree reads whole' bash -c 'cd c-shallow && git cat-file -p refs/mock/registry:ticket/items/t1.toml | grep -q t1'
git clone -q -b dev file://"$root"/remote.git c-full
notcheck 'a full default clone still has no refs/mock' bash -c 'cd c-full && git for-each-ref refs/mock | grep -q .'
(cd c-full && git config --add remote.origin.fetch '+refs/mock/*:refs/mock/*' && git fetch -q origin)
check 'a configured refspec brings them on a plain fetch' bash -c 'cd c-full && test "$(git for-each-ref refs/mock | wc -l)" = 2'

# 4. the instantiation: a worktree-like mapping with its own index, no linked worktree
cd "$root/c-full"
mkdir -p .mock/registry
export GIT_INDEX_FILE="$PWD/.git/mock-registry.index"
git --work-tree=.mock/registry read-tree refs/mock/registry
git --work-tree=.mock/registry checkout-index -a
unset GIT_INDEX_FILE
check 'the ref tree materialises under .mock/registry' test -f .mock/registry/ticket/items/t1.toml
git status --porcelain | grep -q '^?? .mock/' && pass 'the main status shows .mock/ untracked until excluded' || fail 'the main status shows .mock/ untracked until excluded'
echo '/.mock/' >> .git/info/exclude
notcheck 'after info/exclude the main status is clean' bash -c 'test -n "$(git status --porcelain)"'

# 5. commit from the instance back into the ref: new file + edit + delete, with a compare-and-swap
echo 'id = "t3"' > .mock/registry/ticket/items/t3.toml
echo 'id = "t1"
state = "done"' > .mock/registry/ticket/items/t1.toml
rm .mock/registry/ticket/items/t2.toml
export GIT_INDEX_FILE="$PWD/.git/mock-registry.index"
git --work-tree=.mock/registry add -A .
tree=$(git write-tree)
old=$(git rev-parse refs/mock/registry)
new=$(git commit-tree -p "$old" -m 'registry: t3 added, t1 done, t2 gone' "$tree")
unset GIT_INDEX_FILE
check 'update-ref with the old value as the expectation succeeds' git update-ref refs/mock/registry "$new" "$old"
notcheck 'update-ref with a stale expectation is refused (the compare-and-swap)' git update-ref refs/mock/registry "$old" "$old"
check 'the new ref tree has the three-way change' bash -c 'test "$(git ls-tree -r --name-only refs/mock/registry | tr "\n" " ")" = "ticket/items/t1.toml ticket/items/t3.toml "'

# 6. what can destroy an instance
git clean -fdq
check 'git clean -fd leaves an excluded .mock/' test -d .mock/registry
git clean -fdxq
notcheck 'git clean -fdx deletes .mock/ and its uncommitted work' test -d .mock/registry
git stash list >/dev/null

# 6b. the same mapping as a linked worktree, for comparison
git worktree add -q --detach .mock/reg-wt refs/mock/registry
check 'a linked worktree can check out a custom ref detached' test -f .mock/reg-wt/ticket/items/t3.toml
echo 'id = "t4"' > .mock/reg-wt/ticket/items/t4.toml
( cd .mock/reg-wt && git add -A && git commit -q -m 'registry: t4 via worktree' )
notcheck 'a commit in the worktree does not move the custom ref' bash -c 'git ls-tree -r --name-only refs/mock/registry | grep -q t4'
check 'the worktree head is detached and holds the commit' bash -c 'git -C .mock/reg-wt ls-tree -r --name-only HEAD | grep -q t4'
git worktree list | sed 's/^/        /'
git worktree remove --force .mock/reg-wt

# 7. gc keeps a custom ref's objects
git gc -q --prune=now
check 'gc --prune=now keeps the objects a custom ref reaches' git cat-file -e "refs/mock/registry:ticket/items/t3.toml"

# 8. a push of the second commit, fast-forward and a lost race
git push -q origin refs/mock/registry:refs/mock/registry
check 'a fast-forward of the ref pushes' bash -c 'test "$(git --git-dir=../remote.git rev-parse refs/mock/registry)" = "$(git rev-parse refs/mock/registry)"'
cd "$root/c-shallow"
git fetch -q --depth 1 origin '+refs/mock/*:refs/mock/*'
loser=$(git commit-tree -p refs/mock/registry -m 'loser' "$(git rev-parse refs/mock/registry^{tree})")
git update-ref refs/mock/registry "$loser"
cd "$root/c-full"
w=$(git commit-tree -p refs/mock/registry -m 'winner' "$(git rev-parse refs/mock/registry^{tree})")
git update-ref refs/mock/registry "$w" && git push -q origin refs/mock/registry:refs/mock/registry
cd "$root/c-shallow"
notcheck 'a push from a stale base is rejected as non-fast-forward' git push origin refs/mock/registry:refs/mock/registry
notcheck 'a force with lease on the stale expectation is refused' git push --force-with-lease=refs/mock/registry:"$(git rev-parse refs/mock/registry^)" origin refs/mock/registry:refs/mock/registry
echo done
