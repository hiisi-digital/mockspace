#!/bin/bash
# Op's ruling: data refs are not branches a forge lists, and a plain clone does not fetch them.
# Layout under test: remote ref refs/mock/<ns>. An opted-in clone adds a dedicated remote "mock" with the
# identity fetch refspec (local mirror refs/mock/<ns>, never checked out), and each data worktree's HEAD is a
# symref onto its own working ref refs/mock-wt/<ns>@<wt>. No refs/heads entry for the data anywhere.
source "$(dirname "$0")/common.sh"; R=${1:?}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
git init -q -b dev seed; (cd seed; echo s > main.rs; git add -A; git commit -qm s)
git clone -q --bare seed remote.git
( cd seed; git checkout -q --orphan d; git rm -rqf .; printf 'a\nb\nc\n' > NOTES.md; git add -A; git commit -qm data; git push -q "$R/remote.git" d:refs/mock/research; git checkout -q dev; git branch -qD d )
echo "== remote refs"; git -C remote.git for-each-ref --format='%(refname)' | tr '\n' ' '; echo
echo "== plain clone, fetch --all, push --all, clone --bare, clone --mirror"
git clone -q remote.git plain; echo "plain clone refs/mock: $(git -C plain for-each-ref refs/mock | wc -l), objects: $(git -C plain count-objects -v | awk '/^count|^in-pack/{s+=$2} END{print s}')"
git -C plain fetch -q --all; echo "after fetch --all: refs/mock $(git -C plain for-each-ref refs/mock | wc -l)"
git clone -q --bare remote.git bare.git; echo "clone --bare: refs/mock $(git -C bare.git for-each-ref refs/mock | wc -l)"
git clone -q --mirror remote.git mirror.git; echo "clone --mirror: refs/mock $(git -C mirror.git for-each-ref refs/mock | wc -l)  (a backup: right)"
echo "== opted-in clone"
git clone -q remote.git oc; cd oc
git remote add mock "$R/remote.git"; git config --replace-all remote.mock.fetch '+refs/mock/*:refs/mock/*'; git config remote.mock.tagOpt --no-tags
git fetch -q mock; echo "mirror refs: $(git for-each-ref --format='%(refname)' refs/mock | tr '\n' ' ')"
git fetch -q --all; echo "fetch --all keeps working: rc=$?"
git worktree add -q --detach mock/rdata refs/mock/research
git update-ref refs/mock-wt/research@w1 refs/mock/research; git -C mock/rdata symbolic-ref HEAD refs/mock-wt/research@w1
echo "data worktree status: $(git -C mock/rdata status | head -1)"; echo "git worktree list: $(git worktree list | tail -1 | sed "s#$R/##")"
echo "heads in the clone: $(git for-each-ref --format='%(refname)' refs/heads | tr '\n' ' ')"
echo "== commit, fetch+merge (driver), push with lease"; cd "$R/oc"
git config merge.upper.driver 'sh -c "cat %A %B | sort -u > t; mv t %A" -- %O %A %B'; echo 'NOTES.md merge=upper' >> .git/info/attributes
( cd mock/rdata; echo mine >> NOTES.md; git commit -qam mine; echo "HEAD ref moved: $(git symbolic-ref HEAD) $(git rev-parse --short HEAD)" )
cd "$R"; git clone -q "$R/remote.git" oc2; cd "$R/oc2"; git remote add mock "$R/remote.git"; git config --replace-all remote.mock.fetch '+refs/mock/*:refs/mock/*'; git fetch -q mock
git worktree add -q --detach mock/rdata refs/mock/research; git update-ref refs/mock-wt/research@w1 refs/mock/research; git -C mock/rdata symbolic-ref HEAD refs/mock-wt/research@w1
( cd mock/rdata; echo theirs >> NOTES.md; git commit -qam theirs; git push mock --force-with-lease=refs/mock/research:$(git rev-parse refs/mock/research) HEAD:refs/mock/research 2>&1 | tail -1 )
cd "$R/oc"; ( cd mock/rdata
  echo "-- plain push (no force) of an update to a ref outside heads:"; git push mock HEAD:refs/mock/research 2>&1 | tail -2
  echo "-- push with lease on a stale expectation (oc2 already moved it):"; git push mock --force-with-lease=refs/mock/research:$(git rev-parse refs/mock/research) HEAD:refs/mock/research 2>&1 | tail -2
  git fetch -q mock; echo "-- merge refs/mock/research (shared driver):"; git merge --no-overwrite-ignore refs/mock/research 2>&1 | tail -1; echo "NOTES: $(tr '\n' '|' < NOTES.md)"
  git push mock --force-with-lease=refs/mock/research:$(git rev-parse refs/mock/research) HEAD:refs/mock/research 2>&1 | tail -1
  echo "-- bare git pull / git push in the data worktree:"; git pull 2>&1 | head -1; git push 2>&1 | head -1 )
echo "== bare clone kept current by +refs/heads/*:refs/heads/*, data worktree HEAD on a non-heads working ref"
cd "$R"; git clone -q --bare "$R/remote.git" b2.git; cd b2.git; git config remote.origin.fetch '+refs/heads/*:refs/heads/*'; git remote add mock "$R/remote.git"; git config --replace-all remote.mock.fetch '+refs/mock/*:refs/mock/*'; git fetch -q mock
git worktree add -q --detach ../w1 dev; git worktree add -q --detach ../w1/mock/rdata refs/mock/research; git update-ref refs/mock-wt/research@w1 refs/mock/research; git -C ../w1/mock/rdata symbolic-ref HEAD refs/mock-wt/research@w1
(cd ../seed; echo more >> main.rs; git commit -qam more; git push -q "$R/remote.git" dev)
git fetch --all 2>&1 | head -3; echo "fetch --all in the bare clone rc=${PIPESTATUS[0]}; dev advanced: $(git rev-parse --short dev)"
echo "== gc keeps what only a working ref holds"; cd "$R/oc/mock/rdata"; echo gc-test >> NOTES.md; git commit -qam gc; H=$(git rev-parse HEAD); git -C .. gc -q --prune=now 2>/dev/null; git gc -q --prune=now; git cat-file -t $H

cd "$R/oc"
echo "== push --all from the opted-in clone"; git push -q --all origin 2>&1 | head -1; echo "remote refs: $(git -C "$R/remote.git" for-each-ref --format='%(refname)' | tr '\n' ' ')"
echo "== push --mirror from a PLAIN clone (it never had refs/mock)"; cd "$R/plain"; KEEP=$(git -C "$R/remote.git" rev-parse refs/mock/research)
git push --mirror origin 2>&1 | grep -c deleted | sed 's/^/deleted refs reported: /'; echo "remote refs: $(git -C "$R/remote.git" for-each-ref --format='%(refname)' | tr '\n' ' ')"
echo "-- restore from an opted-in clone and repeat with receive.denyDeletes on the remote (self-hosted only)"
git -C "$R/oc" push -q mock "refs/mock/research:refs/mock/research" 2>&1 | head -1; git -C "$R/remote.git" config receive.denyDeletes true
git push --mirror origin 2>&1 | grep -i "deny\|reject\|deleted" | head -2; echo "remote refs: $(git -C "$R/remote.git" for-each-ref --format='%(refname)' | tr '\n' ' ')"
echo "-- opted-in clone: push --mirror carries refs/mock and refs/mock-wt (publishes working refs)"; git -C "$R/remote.git" config receive.denyDeletes false; cd "$R/oc"; git push -q --mirror origin 2>&1 | head -1; echo "remote refs: $(git -C "$R/remote.git" for-each-ref --format='%(refname)' | tr '\n' ' ')"
