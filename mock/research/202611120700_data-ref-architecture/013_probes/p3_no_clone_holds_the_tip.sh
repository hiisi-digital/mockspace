#!/bin/bash
# 013/p3: op's question: a destructive push removes or rewinds the remote ref and NO clone anywhere holds the true tip. Is it lost?
# (a) delete the ref, then gc the remote: are the objects gone?  (b) before that gc, can a clone that never had the objects recover from the id alone?
# (c) self-hosted option: receive.hideRefs on the remote: does a mirror push from a plain clone still delete the data ref, and can an opted-in clone still publish?
set -u
S=${1:?scratch}; rm -rf $S/p3; mkdir -p $S/p3 && cd $S/p3; ROOT=$PWD
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
U=file://$ROOT/origin.git
git init -q --bare origin.git; git --git-dir=origin.git symbolic-ref HEAD refs/heads/dev
git init -q -b dev seed; cd seed; echo c > c.rs; git add -A; git commit -qm i; git push -q $U dev
git switch -q --orphan d; echo secret-data > f; git add f; git commit -qm data; TIP=$(git rev-parse HEAD); git push -q $U HEAD:refs/mock/x; cd ..
echo "tip id (what a Mock-Ref trailer would carry): ${TIP:0:10}"
echo "== (a) ref deleted on the remote, objects still there until the remote's gc"
git --git-dir=origin.git update-ref -d refs/mock/x
echo "remote still has the commit object: $(git --git-dir=origin.git cat-file -t $TIP 2>&1 | head -1)"
echo "== (b) a clone that never had the data tries to recover from the id alone"
git clone -q $U nohold 2>/dev/null; ( cd nohold; git fetch $U $TIP 2>&1 | tail -1; echo "has object after fetch by id: $(git cat-file -t $TIP 2>&1 | head -1)" )
git --git-dir=origin.git config uploadpack.allowAnySHA1InWant true
( cd nohold; git fetch -q $U $TIP 2>&1 | tail -1; echo "with uploadpack.allowAnySHA1InWant=true on the remote: has object: $(git cat-file -t $TIP 2>&1 | head -1)" )
git --git-dir=origin.git config --unset uploadpack.allowAnySHA1InWant
echo "== (a2) now the remote gc runs (prune=now)"
git --git-dir=origin.git gc -q --prune=now; echo "remote has the commit object: $(git --git-dir=origin.git cat-file -t $TIP 2>&1 | head -1)"
echo "with no clone holding the object, the id alone recovers nothing: $(cd nohold; git fetch -q $U $TIP 2>&1 | tail -1 | cut -c1-80)"
echo "== (c) receive.hideRefs=refs/mock on the remote"
git --git-dir=origin.git config --unset uploadpack.allowAnySHA1InWant 2>/dev/null
( cd seed; git push -q $U HEAD:refs/mock/x ); git --git-dir=origin.git config receive.hideRefs refs/mock
git clone -q $U plainP 2>/dev/null; ( cd plainP; git push -q --mirror origin 2>&1 | tail -2 ); echo "after a plain clone's mirror push: refs/mock/x = $(git --git-dir=origin.git rev-parse --short refs/mock/x 2>&1 | head -1)"
( cd seed; echo more >> f; git commit -qam more; git push $U HEAD:refs/mock/x 2>&1 | tail -2 ); echo "opted-in clone publishes a new tip: refs/mock/x = $(git --git-dir=origin.git rev-parse --short refs/mock/x 2>&1 | head -1)"
( cd seed; git push $U HEAD:refs/mock/y 2>&1 | tail -1 ); echo "and creates a new data ref: refs/mock/y = $(git --git-dir=origin.git rev-parse --short refs/mock/y 2>&1 | head -1)"
