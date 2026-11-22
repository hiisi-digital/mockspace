#!/bin/bash
# 016/p3: op's question: could instances live in a platform state directory (~/.local/state/...) instead of <ρ> in the git tree?
# Layout tried: STATE/<repo-id>/<ns> is the data worktree; the git tree holds .mock -> that directory (an ignored symlink).
# (a) git verbs on the main tree, (b) physical ../ from inside the instance (cargo), (c) two clones of one repo, (d) moving/deleting a clone, (e) clean -ffdx.
set -u
SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/p016-3-XXXXXX") || exit 1; cd "$SCRATCH" || exit 1
case $PWD in "$SCRATCH"*) ;; *) echo "refusing to run outside my own scratch"; exit 1;; esac
trap 'cd /; rm -rf "$SCRATCH"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a CARGO_TARGET_DIR=$SCRATCH/target CARGO_NET_OFFLINE=true
STATE=$SCRATCH/state; mkdir -p $STATE
mk() { rm -rf $1; git init -q -b dev $1; ( cd $1; mkdir -p crates/lib/src; printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' > crates/lib/Cargo.toml; echo 'pub fn f()->u8{1}' > crates/lib/src/lib.rs; git add -A; git commit -qm i; git branch other; echo ".mock" >> .git/info/exclude ); }
inst() { # inst <clone> <state-key>: data worktree in the state dir, linked in as .mock
  ( cd $1; mkdir -p $STATE/$2; git worktree add -q --orphan -b mock/research $STATE/$2/research; ( cd $STATE/$2/research; mkdir -p sketch/src; printf '[package]\nname="sk"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="../../../crates/lib"}\n[workspace]\n' > sketch/Cargo.toml; echo 'fn main(){}' > sketch/src/main.rs; echo data > N.md; git add -A; git commit -qm d ); ln -s $STATE/$2 .mock ); }
mk r1; inst r1 key1; cd r1
echo "== (a) verbs in the main tree with an uncommitted data edit"
echo EDIT > .mock/research/N.md
for v in "checkout -q -f other" "reset -q --hard dev" "clean -fdxq" "clean -ffdxq" "stash -u -q" "switch -q dev"; do git $v >/dev/null 2>&1; printf '%-24s data edit: %-6s .mock still a link: %s\n' "git $v" "$(cat $STATE/key1/research/N.md 2>&1 | head -1)" "$(test -L .mock && echo yes || echo no)"; test -L .mock || ln -s $STATE/key1 .mock; done
echo "git status in the main tree: [$(git status --short | tr '\n' ' ')]"
echo "== (b) physical ../ from inside the instance (the manifests' ../../../crates/lib)"
( cd .mock/research/sketch && timeout 120 cargo metadata --format-version 1 >/dev/null 2>&1 && echo "cd into .mock/research/sketch, cargo metadata: resolves" || echo "cd into .mock/research/sketch, cargo metadata: BREAKS (the physical parent is the state dir)" )
timeout 120 cargo metadata --format-version 1 --manifest-path .mock/research/sketch/Cargo.toml >/dev/null 2>&1 && echo "from the root with --manifest-path .mock/research/sketch/Cargo.toml: resolves" || echo "from the root with --manifest-path: BREAKS"
cd ..
echo "== (c) a second clone of the same repository, same state key: two worktrees want one branch, and one directory"
mk r2; ( cd r2; git remote add o ../r1 2>/dev/null; git fetch -q o 'refs/heads/mock/research:refs/remotes/o/research' ); ( cd r2; git worktree add -q $STATE/key1/research refs/remotes/o/research 2>&1 | head -1 )
echo "== (d) the clone is moved, then deleted"
mv r1 r1moved; ( cd r1moved; git worktree list | tail -1 | cut -c1-120; echo "data worktree opens from its own path: $(git -C $STATE/key1/research status --short 2>&1 | head -1 | cut -c1-80)"; git worktree repair $STATE/key1/research 2>&1 | head -1 | cut -c1-40; echo "after repair, data worktree status: [$(git -C $STATE/key1/research status --short 2>&1 | head -1 | cut -c1-60)] branch: $(git -C $STATE/key1/research symbolic-ref --short HEAD 2>&1)" )
rm -rf r1moved; echo "after rm -rf of the clone: data files still in the state dir: $(ls $STATE/key1/research | tr '\n' ' '); its .git file points at: $(cut -c1-60 $STATE/key1/research/.git)"
