#!/bin/bash
# Spike a16: op's ruling of 12 November, the remote data ref outside refs/heads. Vehicle under test: the remote
# holds refs/mock/<ns>; a clone that asks for the data gets a DEDICATED remote `mock-<ns>` (same url) with
#   fetch = +refs/mock/<ns>:refs/remotes/mock-<ns>/<ns>      push = HEAD:refs/mock/<ns>
# and each data worktree sits on a local branch mock-wt/<ns>/<wt> with branch.<b>.remote = mock-<ns> and
# branch.<b>.merge = refs/mock/<ns>. Questions: do vanilla `git pull` and `git push` in the data worktree work;
# what do clone, clone --bare, clone --mirror, fetch --all, push --all and push --mirror do with the data ref;
# and the alternative of a worktree whose HEAD is a symref straight onto refs/mock/<ns>.
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
NS=research; D=mock/rdata
git init -q --bare -b dev origin.git
git clone -q origin.git seed 2>/dev/null; cd seed; echo s > main.rs; git add -A; git commit -qm s; git push -q origin dev
C=$(git commit-tree $(printf '100644 blob %s\tN.md\n' $(echo a | git hash-object -w --stdin) | git mktree) -m data); git push -q origin $C:refs/mock/$NS; cd ..
echo "remote refs: $(git -C origin.git for-each-ref --format='%(refname)' | tr '\n' ' ')"
git clone -q "file://$S/origin.git" plain; echo "plain clone has: $(git -C plain for-each-ref --format='%(refname)' | grep -c mock) mock refs; objects of data commit present: $(git -C plain cat-file -e $C 2>/dev/null && echo yes || echo no)"
git clone -q --bare "file://$S/origin.git" bare.git; echo "clone --bare has: $(git -C bare.git for-each-ref --format='%(refname)' | grep -c mock) mock refs"
git clone -q --mirror origin.git mirror.git; echo "clone --mirror has: $(git -C mirror.git for-each-ref --format='%(refname)' | grep mock | tr '\n' ' ')"
git -C plain fetch -q --all; echo "plain after fetch --all: $(git -C plain for-each-ref --format='%(refname)' | grep -c mock) mock refs"
optin() { # $1 clone dir, $2 wt id : what `instantiate` would set up in a clone that asks for the data
  git -C "$1" config remote.mock-$NS.url "$(git -C "$1" config remote.origin.url)"
  git -C "$1" config remote.mock-$NS.fetch "+refs/mock/$NS:refs/remotes/mock-$NS/$NS"
  git -C "$1" config remote.mock-$NS.push "HEAD:refs/mock/$NS"
  git -C "$1" config remote.mock-$NS.tagOpt --no-tags
  git -C "$1" fetch -q mock-$NS
  local B=mock-wt/$NS/$2; git -C "$1" branch -q --no-track $B refs/remotes/mock-$NS/$NS
  git -C "$1" config branch.$B.remote mock-$NS; git -C "$1" config branch.$B.merge refs/mock/$NS
  git -C "$1" worktree add -q $D $B; echo "/$D/" >> "$(git -C "$1" rev-parse --path-format=absolute --git-common-dir)/info/exclude"
}
git clone -q "file://$S/origin.git" a; optin a a; git clone -q "file://$S/origin.git" b; optin b b
(cd a/$D; echo fromA >> N.md; git commit -qam A; r=$(git push 2>&1 | tail -1); echo "a: vanilla git push from data worktree: {$r} remote now $(git -C $S/origin.git rev-parse --short refs/mock/$NS)")
(cd b/$D; echo fromB > B.md; git add B.md; git commit -qm B; r=$(git pull -q --no-rebase --no-edit 2>&1 | tail -1); echo "b: vanilla git pull: {$r} files: $(ls | tr '\n' ' ')"; r=$(git push 2>&1 | tail -1); echo "b: vanilla git push: {$r}")
(cd a/$D; git pull -q --no-rebase; echo "a: after pull: $(ls | tr '\n' ' ') status: $(git status -sb | head -1)")
echo "a: fetch --all also fetches data: $(git -C a fetch --all 2>&1 | grep -c 'Fetching mock')"
echo "a: git push --all from the main worktree puts on the remote: $(git -C a push -q --all origin 2>&1 | tail -1; git -C origin.git for-each-ref --format='%(refname)' refs/heads | tr '\n' ' ')"
git -C origin.git branch -D mock-wt/$NS/a >/dev/null 2>&1
echo "== push --mirror from a plain clone that never asked for data"
git -C plain push -q --mirror origin 2>&1 | tail -1; echo "remote refs after: $(git -C origin.git for-each-ref --format='%(refname)' | tr '\n' ' ')"
git -C a push -q mock-$NS mock-wt/$NS/a:refs/mock/$NS 2>/dev/null; echo "(restored from a: $(git -C origin.git rev-parse --short refs/mock/$NS 2>&1))"
echo "== push --mirror from clone a, which has the data under refs/remotes/mock-$NS"
git -C a push -q --mirror origin 2>&1 | tail -1; echo "remote refs after: $(git -C origin.git for-each-ref --format='%(refname)' | tr '\n' ' ')"
git -C a push -q mock-$NS mock-wt/$NS/a:refs/mock/$NS 2>/dev/null; echo "(restored from a: $(git -C origin.git rev-parse --short refs/mock/$NS 2>&1))"
echo "== alternative: a worktree whose HEAD is a symref onto refs/mock/$NS"
git clone -q "file://$S/origin.git" h; git -C h fetch -q origin "+refs/mock/$NS:refs/mock/$NS"; git -C h worktree add -q --detach $D refs/mock/$NS
(cd h/$D; r=$(git symbolic-ref HEAD refs/mock/$NS 2>&1); echo "symbolic-ref: {$r} status: {$(git status 2>&1 | head -1)}"; echo x >> N.md; git commit -qam viaHEAD 2>&1 | head -1; echo "refs/mock/$NS moved by commit: $(git log -1 --format=%s refs/mock/$NS)"; r=$(git pull 2>&1 | head -1); echo "pull: {$r}")
echo "gc keeps the data reachable in a: $(git -C a gc -q --prune=now 2>&1; git -C a cat-file -t $C)"
