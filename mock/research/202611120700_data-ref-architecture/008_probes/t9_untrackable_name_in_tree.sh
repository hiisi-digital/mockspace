#!/bin/bash
# The data worktree at mock/git~1 (a name core.protectNTFS makes untrackable in every clone), hidden by a
# tracked .gitignore line on dev. Does it work as a worktree, as a cargo location at the old depth, and
# does it stay hidden from add -A and status? What does it still lose?
source "$(dirname "$0")/common.sh"; R=${1:?}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
git init -q -b dev m; cd m; mkdir -p crates/lib/src mock/research; echo old > mock/research/N.md
printf '[package]\nname="lib"\nversion="0.1.0"\nedition="2021"\n' > crates/lib/Cargo.toml; echo 'pub fn f(){}' > crates/lib/src/lib.rs
git add -A; git commit -qm old; OLD=$(git rev-parse HEAD); git rm -rqf mock/research; echo '/mock/git~1/' > .gitignore; git add .gitignore; git commit -qm "migrate + ignore the data path"
git tag new; echo "mock/git~1/" >> .git/info/exclude; git checkout -q --orphan data; git rm -rqf .; mkdir -p sketch/src; echo old > N.md
printf '[package]\nname="sk"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nlib={path="../../../crates/lib"}\n' > sketch/Cargo.toml; echo 'fn main(){ lib::f(); }' > sketch/src/main.rs
git add -A; git commit -qm data; git checkout -q dev
git worktree add -q 'mock/git~1' data 2>&1 | head -2; echo "worktree add rc=$? ; list: $(git worktree list | tail -1 | sed "s#$R/##")"
echo "status: [$(git status --short)]"; git add -A 2>&1 | head -2; echo "after add -A staged: [$(git diff --cached --name-only)]"
(cd 'mock/git~1/sketch' && cargo metadata --offline --format-version 1 >/dev/null 2>&1 && echo "cargo at the old depth: OK" || echo "cargo FAILED")
cd 'mock/git~1'; echo EDIT >> N.md; git commit -qam "data commit" && echo "data commit rc=0"; echo EDIT2 >> N.md; cd ../..
for v in "checkout -qf $OLD" "checkout -q new" "reset -q --hard $OLD" "checkout -q -f dev" "reset -q --hard new" "stash -u" "clean -fd" "clean -fdx"; do git $v >/dev/null 2>&1; echo "git $v -> data edit survives=$(grep -c EDIT2 'mock/git~1/N.md' 2>/dev/null; true)"; done
git checkout -q dev; git reset -q --hard new; echo "(on dev at new: .gitignore present: $([ -f .gitignore ] && echo yes))"
for v in "clean -ffd" "clean -ffdx" "clean -ffdX"; do (echo EDIT2 >> 'mock/git~1/N.md') 2>/dev/null; git $v >/dev/null 2>&1; echo "git $v -> data dir present=$([ -d 'mock/git~1' ] && echo yes || echo no)"; git worktree prune; git worktree add -q 'mock/git~1' data 2>/dev/null; done
