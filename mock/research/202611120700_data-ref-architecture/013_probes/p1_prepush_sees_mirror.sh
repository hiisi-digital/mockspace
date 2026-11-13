#!/bin/bash
# 013/p1: what a pre-push hook sees of destructive pushes of a data ref, and whether it can refuse.
# Remote holds refs/heads/dev and refs/mock/x (a 3-commit chain c1<c2<c3). Clone O (opted in) has tracking ref and hook;
# clone P (plain) has no hook and no data ref; clone Q has the hook but a STALE tracking tip c2.
set -u
S=${1:?scratch}; rm -rf $S/p1; mkdir -p $S/p1 && cd $S/p1; ROOT=$PWD
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q --bare origin.git; git --git-dir=origin.git symbolic-ref HEAD refs/heads/dev
git init -q -b dev seed && cd seed && echo c > c.rs && git add -A && git commit -qm i && git push -q ../origin.git dev
git switch -q --orphan d; for i in 1 2 3; do echo $i > f; git add f; git commit -qm c$i; git tag c$i; done
git push -q ../origin.git c1:refs/mock/x; git push -q ../origin.git c2:refs/mock/x; git push -q ../origin.git c3:refs/mock/x; cd ..
HOOK='#!/bin/sh
echo "--- pre-push remote=$1 url-kind=$(echo $2 | cut -c1-7)" >> '$ROOT'/hook.log
while read lref lsha rref rsha; do echo "$lref $lsha $rref $rsha" >> '$ROOT'/hook.log
  case "$rref" in refs/mock/*) if [ "$lsha" = 0000000000000000000000000000000000000000 ]; then echo REFUSE-delete >> '$ROOT'/hook.log; REF=1; fi
    if [ "$lsha" != 0000000000000000000000000000000000000000 ] && ! git merge-base --is-ancestor "$rsha" "$lsha" 2>/dev/null; then echo REFUSE-nonff >> '$ROOT'/hook.log; REF=1; fi;; esac; done
exit ${REF:-0}'
mkclone() { git clone -q file://$ROOT/origin.git $1 2>/dev/null; ( cd $1; git config remote.mock.url file://$ROOT/origin.git; git config remote.mock.fetch '+refs/mock/x:refs/remotes/mock/x'; git fetch -q mock ); }
mkclone O; printf '%s\n' "$HOOK" > O/.git/hooks/pre-push; chmod +x O/.git/hooks/pre-push
mkclone Q; printf '%s\n' "$HOOK" > Q/.git/hooks/pre-push; chmod +x Q/.git/hooks/pre-push
git --git-dir=Q/.git update-ref refs/remotes/mock/x $(git --git-dir=Q/.git rev-parse refs/remotes/mock/x~1)   # Q is stale: tracking tip c2
git clone -q file://$ROOT/origin.git P 2>/dev/null
rtip() { git --git-dir=$ROOT/origin.git rev-parse --short refs/mock/x 2>/dev/null || echo ABSENT; }
reset_remote() { git --git-dir=$ROOT/origin.git update-ref refs/mock/x $(git --git-dir=seed/.git rev-parse c3); : > $ROOT/hook.log; }
run() { local who=$1; shift; reset_remote; ( cd $who; git "$@" ) >/dev/null 2>$ROOT/err.txt; local rc=$?
  printf '%s: git %s   rc=%s   remote refs/mock/x after: %s\n' "$who" "$*" $rc "$(rtip)"
  grep -v '^---' $ROOT/hook.log | sed 's#^#      hook stdin: #; s/ [0-9a-f]\{40\}/ <sha>/g' ; [ -s $ROOT/hook.log ] || echo "      (hook not called)"; }
echo "remote tip c3 = $(git --git-dir=seed/.git rev-parse --short c3), c2 = $(git --git-dir=seed/.git rev-parse --short c2)"
run O push --mirror origin
run Q push --mirror origin
run P push --mirror origin
run O push origin --delete refs/mock/x
run O push origin :refs/mock/x
run Q push -f origin refs/remotes/mock/x:refs/mock/x
run O push --no-verify --mirror origin
run O push --all origin
echo "== does the refusal print anything the person sees?"; reset_remote; ( cd O; git push --mirror origin 2>&1 | head -3 )
echo "== P has no hook and no ref: the clone we cannot guard"; run P push --mirror origin
echo "== S: opted in with an IDENTITY mapping (local refs/mock/x), STALE at c2, hook installed: mirror push rewinds the remote"
reset_remote; git clone -q file://$ROOT/origin.git S 2>/dev/null; printf '%s\n' "$HOOK" > S/.git/hooks/pre-push; chmod +x S/.git/hooks/pre-push
git --git-dir=S/.git fetch -q origin +refs/mock/x:refs/mock/x; git --git-dir=S/.git update-ref refs/mock/x $(git --git-dir=seed/.git rev-parse c2)
run S push --mirror origin
echo "== R: hook installed, local refs equal to the remote's heads, NO other ref to create or update: a mirror that only deletes"
git init -q --bare R.git; git --git-dir=R.git remote add origin file://$ROOT/origin.git; git --git-dir=R.git fetch -q origin dev:refs/heads/dev; printf '%s\n' "$HOOK" > R.git/hooks/pre-push; chmod +x R.git/hooks/pre-push
reset_remote; ( cd R.git; git push --mirror origin >/dev/null 2>&1 ); echo "R: remote refs/mock/x after: $(rtip); hook stdin:"; sed 's#^#      #; s/ [0-9a-f]\{40\}/ <sha>/g' $ROOT/hook.log
