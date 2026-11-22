#!/bin/bash
# 009/p3: op's ruling: data refs not under refs/heads/ on the remote; a plain clone fetches none of it.
# Layout under test:  remote:  refs/mock/<ns>
#                     opted-in clone: a dedicated remote  mock-<ns>  (same url), fetch = +refs/mock/<ns>:refs/remotes/mock-<ns>/<ns>,
#                                     tagOpt --no-tags, no local branch at all; each data worktree's HEAD is a symref onto refs/mock-wt/<ns>/<wt>
# Remote is a bare repo reached by file:// (a real forge is NOT probed; refs.rs:16-19 records a proxy that refused refs/mock/*).
set -u
S=${1:?scratch}; rm -rf $S/p3; mkdir -p $S/p3 && cd $S/p3
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
ROOT=$PWD; U=file://$PWD/origin.git
git init -q --bare origin.git; git --git-dir=origin.git symbolic-ref HEAD refs/heads/dev
git init -q -b dev seed && cd seed && echo c > c.rs && git add -A && git commit -qm i && git push -q $U dev
git switch -q --orphan data && printf 'big\n' > big.dat && git add -A && git commit -qm data && git push -q $U data:refs/mock/research; cd ..
refs() { git --git-dir="$1" for-each-ref --format='%(refname)' | tr '\n' ' '; }
echo "remote refs: $(refs origin.git)"
echo "== plain clone family: any ref or object of the data?"
git clone -q $U plain; echo "clone:               $(refs plain/.git)  data commit present: $(git -C plain cat-file -e $(git --git-dir=origin.git rev-parse refs/mock/research) 2>/dev/null && echo YES || echo no)"
git clone -q --bare $U plainbare.git; echo "clone --bare:        $(refs plainbare.git)"
git -C plain fetch -q --all; echo "fetch --all (plain): $(refs plain/.git)"
git -C plain fetch -q origin; echo "fetch origin:        $(git -C plain for-each-ref --format='%(refname)' | grep -c mock) mock refs"
git clone -q --mirror $U mirror.git; echo "clone --mirror:      $(refs mirror.git)   (intended: a backup)"
echo "== opted-in clone"
git clone -q $U opt && cd opt
git config core.logAllRefUpdates always
git remote add mock-research $U; git config remote.mock-research.fetch '+refs/mock/research:refs/remotes/mock-research/research'; git config remote.mock-research.tagOpt --no-tags
git config remote.mock-research.push 'HEAD:refs/mock/research'
echo "fetch origin brings data?  $(git fetch -q origin; git for-each-ref refs/remotes/mock-research | wc -l) refs"
git fetch -q --all; echo "fetch --all brings data?   $(git for-each-ref refs/remotes/mock-research | wc -l) ref(s) (opted in, so intended)"
git update-ref refs/mock-wt/research/main refs/remotes/mock-research/research
git worktree add -q --detach mock-data-research refs/mock-wt/research/main
( cd mock-data-research && git symbolic-ref HEAD refs/mock-wt/research/main && echo "worktree HEAD: $(git symbolic-ref HEAD)  status: $(git status | head -1)" )
echo "worktree list: $(git worktree list | tail -1 | awk '{print $NF}')"
echo "== vanilla verbs inside the data worktree"
cd mock-data-research; echo more >> big.dat; git commit -qam "edit"; echo "commit moved the ref: $(git rev-parse --short refs/mock-wt/research/main) == HEAD $(git rev-parse --short HEAD)"
git push -q mock-research 2>&1 | head -2; echo "plain 'git push mock-research' -> remote refs/mock/research = $(git --git-dir=../../origin.git rev-parse --short refs/mock/research)"
echo "bare 'git pull' says: $(git pull 2>&1 | head -1)"
cd ..; git -C ../seed fetch -q $U refs/mock/research:refs/heads/data2 2>/dev/null
echo "== a second clone's concurrent edit, then pull with explicit args"
git clone -q $U opt2; ( cd opt2; git remote add mock-research $U; git config remote.mock-research.fetch '+refs/mock/research:refs/remotes/mock-research/research'; git fetch -q mock-research
  git update-ref refs/mock-wt/research/main refs/remotes/mock-research/research; git worktree add -q --detach mdr refs/mock-wt/research/main; cd mdr; git symbolic-ref HEAD refs/mock-wt/research/main; echo other >> other.dat; git add other.dat; git commit -qm other; git push -q $U HEAD:refs/mock/research )
cd mock-data-research; echo x >> big.dat; git commit -qam "edit2"
git pull --no-rebase --no-edit mock-research refs/mock/research 2>&1 | tail -2; echo "after explicit pull: files $(ls | tr '\n' ' ') ; ref == HEAD: $([ "$(git rev-parse refs/mock-wt/research/main)" = "$(git rev-parse HEAD)" ] && echo yes || echo NO)"
git push -q mock-research 2>&1 | head -1; echo "pushed: remote tip == local: $([ "$(git --git-dir=../../origin.git rev-parse refs/mock/research)" = "$(git rev-parse HEAD)" ] && echo yes || echo NO)"
echo "== reflog recovery inside the data worktree"
git reset -q --hard HEAD~1; echo "after reset --hard HEAD~1, ref@{1} = $(git rev-parse --short 'refs/mock-wt/research/main@{1}' 2>&1 | head -1) (HEAD was $(git rev-parse --short ORIG_HEAD))"
git reset -q --hard ORIG_HEAD
cd ..
echo "== leaks from the OPTED-IN clone through vanilla verbs run in the main worktree"
git push -q --all origin 2>&1 | head -2; echo "after push --all origin:      remote refs: $(refs ../origin.git)"
git push -q origin --tags 2>&1 | head -1
git push -q --mirror origin 2>&1 | head -2; echo "after push --mirror origin (opted-in): remote refs: $(refs ../origin.git)"
cd ..
echo "== a new plain clone after the opted-in mirror push: any data?"
rm -rf plain2; git clone -q $U plain2; echo "plain2 refs: $(refs plain2/.git)"
echo "== push --mirror from a PLAIN clone, which has no refs/mock/*"
git -C plain push -q --mirror origin 2>&1 | head -2; echo "remote refs after plain-clone mirror push: $(refs origin.git)"
echo "== the data is still in the opted-in clone: $(git -C opt cat-file -t refs/mock-wt/research/main 2>&1)"
echo "== variant: opted-in clone keeps an IDENTITY mapping refs/mock/research locally (fetch = +refs/mock/research:refs/mock/research)"
git --git-dir=origin.git update-ref refs/mock/research $(git --git-dir=opt/.git rev-parse refs/mock-wt/research/main) 2>/dev/null
git --git-dir=origin.git update-ref refs/heads/dev $(git -C seed rev-parse dev) 2>/dev/null
git -C opt config remote.mock-research.fetch '+refs/mock/research:refs/mock/research'; git -C opt fetch -q mock-research
git -C opt checkout -q dev 2>/dev/null || git -C opt checkout -q -b dev origin/dev
echo "opted-in local refs: $(git -C opt for-each-ref --format='%(refname)' | tr '\n' ' ')"
git -C opt push -q --mirror origin 2>&1 | head -2; echo "remote after opted-in push --mirror (identity mapped, up to date): $(refs origin.git)"
echo "-- same, but the opted-in clone is BEHIND: another clone pushed a data commit it has not fetched"
( cd $ROOT/opt/opt2/mdr; git fetch -q $U refs/mock/research 2>/dev/null; git merge -q --ff-only FETCH_HEAD 2>/dev/null; echo newer >> other.dat; git commit -qam newer; git push -q $U HEAD:refs/mock/research )
NEWER=$(git --git-dir=$ROOT/origin.git rev-parse refs/mock/research)
git -C $ROOT/opt push -q --mirror origin 2>&1 | head -2
AFTER=$(git --git-dir=$ROOT/origin.git rev-parse refs/mock/research 2>&1 | head -1)
echo "remote refs/mock/research before the stale clone's mirror push: ${NEWER:0:7}, after: ${AFTER:0:7}  ($([ "$NEWER" = "$AFTER" ] && echo kept || echo "the newer data commit is no longer reachable from the remote ref"))"
