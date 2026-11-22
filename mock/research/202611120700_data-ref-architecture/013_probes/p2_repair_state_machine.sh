#!/bin/bash
# 013/p2: the automatic re-push as a state machine, run on scenarios. Remote refs/mock/x; opted-in clones A and B keep, per clone,
#   refs/mock-seen/x  = the highest remote tip ever observed (only ever moved to a DESCENDANT of itself)
# repair(clone):  r = remote tip (or none); s = seen
#   r none                           -> FIRE   push s to create
#   s is an ancestor of r (or equal) -> seen := r (fetch), no fire
#   r is a proper ancestor of s      -> FIRE   push s with --force-with-lease=refs/mock/x:r
#   otherwise (incomparable)         -> no fire (a concurrent edit; the merge flow owns it)
# Every fire prints PUSH. Nothing else ever writes the remote.
set -u
S=${1:?scratch}; rm -rf $S/p2; mkdir -p $S/p2 && cd $S/p2; ROOT=$PWD
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
U=file://$ROOT/origin.git
git init -q --bare origin.git
rtip() { git --git-dir=$ROOT/origin.git rev-parse refs/mock/x 2>/dev/null; }
short() { [ -n "$1" ] && echo ${1:0:7} || echo none; }
repair() { local c=$1; cd $ROOT/$c
  local r=$(git ls-remote $U refs/mock/x | cut -f1); local s=$(git rev-parse -q --verify refs/mock-seen/x)
  if [ -z "$r" ]; then
    if [ -n "$s" ]; then git push -q $U $s:refs/mock/x && echo "$c: r=none s=$(short $s) -> PUSH (create)"; else echo "$c: r=none, no seen tip: nothing to repair"; fi
  else
    git fetch -q $U +refs/mock/x:refs/remotes/mock/x 2>/dev/null
    if [ -z "$s" ] || git merge-base --is-ancestor $s $r 2>/dev/null; then
      [ "$s" != "$r" ] && git update-ref refs/mock-seen/x $r; echo "$c: r=$(short $r) s=$(short $s) -> no fire (seen := r)"
    elif git merge-base --is-ancestor $r $s 2>/dev/null; then
      if git push -q --force-with-lease=refs/mock/x:$r $U $s:refs/mock/x 2>/dev/null; then echo "$c: r=$(short $r) s=$(short $s) -> PUSH (rewind repair)"; else echo "$c: lease failed, remote moved, re-evaluate next run (no clobber)"; fi
    else echo "$c: r=$(short $r) s=$(short $s) incomparable -> no fire (concurrent edit)"; fi
  fi; cd $ROOT; }
commit() { # commit CLONE n : a data commit on top of the clone's seen tip, in a scratch worktree-less way (plumbing)
  local c=$1; cd $ROOT/$c; local base=$(git rev-parse -q --verify refs/mock-seen/x); local T=$(echo "$2" | git hash-object -w --stdin)
  local tree=$(printf '100644 blob %s\tf\n' $T | git mktree); local new=$(git commit-tree $tree ${base:+-p $base} -m "$2"); echo $new; cd $ROOT; }
pushff() { git --git-dir=$ROOT/origin.git fetch -q $ROOT/$1 $2:refs/mock/x 2>/dev/null; }
for c in A B P; do git init -q $c; done
c1=$(commit A c1); git -C A update-ref refs/mock-seen/x $c1; git --git-dir=origin.git fetch -q $ROOT/A $c1:refs/mock/x
git -C B fetch -q $U refs/mock/x:refs/mock-seen/x
c2=$(commit A c2); git -C A update-ref refs/mock-seen/x $c2; git --git-dir=origin.git fetch -q $ROOT/A $c2:refs/mock/x
c3=$(commit A c3); git -C A update-ref refs/mock-seen/x $c3; git --git-dir=origin.git fetch -q $ROOT/A $c3:refs/mock/x
git -C B fetch -q $U refs/mock/x:refs/mock-seen/x
echo "== 1 steady state, repair run three times by each clone: any PUSH?"
for i in 1 2 3; do repair A; repair B; done | sort | uniq -c
echo "== 2 a plain clone P mirror-pushes (the remote ref is deleted); A repairs, then B, then A again"
git --git-dir=origin.git update-ref -d refs/mock/x; repair A; repair B; repair A; echo "remote now: $(short $(rtip))"
echo "== 3 a stale clone rewinds the remote to c1 by mirror/force; B repairs, A repairs"
git --git-dir=origin.git update-ref refs/mock/x $c1; repair B; repair A; echo "remote now: $(short $(rtip))"
echo "== 4 a legitimate advance by someone else (c4 on top of c3) between runs: no fire, seen follows"
c4=$(commit A c4); git --git-dir=origin.git fetch -q $ROOT/A $c4:refs/mock/x; repair B; repair A; echo "remote now: $(short $(rtip))  B seen: $(short $(git -C B rev-parse refs/mock-seen/x))"
echo "== 5 concurrent divergent edits: B commits d5 on c4 and pushes; A commits e5 on c4 locally, has not fetched B's, runs repair"
d5=$(commit B d5); git --git-dir=origin.git fetch -q $ROOT/B $d5:refs/mock/x
git -C A update-ref refs/mock-seen/x $c4; e5=$(commit A e5); git -C A update-ref refs/heads/local-unpushed $e5
repair A; echo "remote now: $(short $(rtip)) (d5 = $(short $d5)); A's unpushed e5 untouched: $(git -C A rev-parse --short refs/heads/local-unpushed)"
echo "== 6 A has only unpushed local commits above the remote tip (seen == remote): no fire"
git -C A update-ref refs/mock-seen/x $d5 2>/dev/null || git -C A fetch -q $U refs/mock/x:refs/mock-seen/x; repair A
echo "== 5b incomparable: the remote is rewound to c3 and someone commits x5 on c3 (not on c4): A's seen tip c4 is incomparable with the remote"
git -C A fetch -q $U refs/mock/x:refs/mock-seen/x 2>/dev/null; git -C A update-ref refs/mock-seen/x $c4
x5=$(commit A x5); git -C A update-ref refs/heads/tmp-x5 $x5; x5b=$(git -C A commit-tree $(git -C A rev-parse $c3^{tree}) -p $c3 -m x5); git --git-dir=origin.git fetch -q $ROOT/A +$x5b:refs/mock/x
repair A; echo "remote still: $(short $(rtip)) (x5b = $(short $x5b)); seen unchanged: $(short $(git -C A rev-parse refs/mock-seen/x))"
echo "== 7 race: A decided r=c1 and will push seen tip c4; the remote advanced to x5b meanwhile; the lease must refuse and x5b must survive"
git --git-dir=origin.git update-ref refs/mock/x $c1; rr=$(rtip); git --git-dir=origin.git update-ref refs/mock/x $x5b
git -C A push -q --force-with-lease=refs/mock/x:$rr $U $c4:refs/mock/x 2>/tmp/lease.err; rc=$?; echo "push rc=$rc ($(head -1 /tmp/lease.err | cut -c1-70)); remote: $(short $(rtip)), x5b was $(short $x5b)"
echo "== 8 an external job rewinds the remote five times; how many repairs?"
n=0; for i in 1 2 3 4 5; do git --git-dir=origin.git update-ref refs/mock/x $c1; repair B | grep -q PUSH && n=$((n+1)); repair B >/dev/null; repair A >/dev/null; done; echo "external rewinds: 5, repairs: $n (one per rewind, none extra); remote now $(short $(rtip))"
