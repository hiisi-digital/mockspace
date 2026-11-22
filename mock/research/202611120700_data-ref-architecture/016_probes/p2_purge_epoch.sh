#!/bin/bash
# 016/p2: the purge epoch over the repair rule of 014 (layer 2), run on a model with real git objects.
# Extended order on published tips:  a >= b  iff  epoch(a) > epoch(b)  or  (epoch(a) == epoch(b) and b is an ancestor-or-equal of a)
# epoch(t) = integer in the file .epoch of t's tree (0 if absent).  A purge publishes a NEW ROOT with epoch+1 and the secret removed.
# repair(clone):  r = remote tip, s = the clone's mirror ref, n = max(epoch(s), the epoch the code tree names: file CODE_EPOCH, a stand-in for a landing's Mock-Ref epoch)
#   r none:             fire (push s) only if epoch(s) >= n, else REFUSE (s is below a known epoch)
#   epoch(r) > epoch(s): ADOPT r: mirror := r, carry local-only commits forward, scrub old-epoch refs and objects
#   r >= s:              nothing (or mirror := r)
#   s > r:               fire (push s, lease on r)
#   otherwise:           nothing (concurrent edit within one epoch)
# hook(local l, remote r): allow iff l >= r in the extended order.
set -u
SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/p016-2-XXXXXX") || exit 1; cd "$SCRATCH" || exit 1
case $PWD in "$SCRATCH"*) ;; *) echo "refusing to run outside my own scratch"; exit 1;; esac
trap 'cd /; rm -rf "$SCRATCH"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
R=$PWD; U=file://$R/origin.git; M=refs/mock/x
git init -q --bare origin.git
short() { [ -n "${1:-}" ] && echo ${1:0:7} || echo none; }
epoch() { git cat-file -p "$1:.epoch" 2>/dev/null || echo 0; }          # in the current repo
anc() { git merge-base --is-ancestor "$1" "$2" 2>/dev/null; }
ge() { local ea=$(epoch $1) eb=$(epoch $2); [ "$ea" -gt "$eb" ] || { [ "$ea" -eq "$eb" ] && anc $2 $1; }; }
rtip() { git ls-remote $U $M | cut -f1; }
mkc() { # mkc <repo> <parent-or-empty> <file-content> [epoch] -> commit id (tree: f, optional .epoch)
  ( cd $1; local b=$(printf '%s\n' "$3" | git hash-object -w --stdin); local t="100644 blob $b\tf"
    [ -n "${4:-}" ] && t="$t\n100644 blob $(printf '%s\n' "$4" | git hash-object -w --stdin)\t.epoch"
    local tree=$(printf "$t\n" | git mktree); git commit-tree $tree ${2:+-p $2} -m "$3" ); }
repair() { local c=$1; cd $R/$c; local r=$(rtip) s=$(git rev-parse -q --verify $M); local n=$(epoch $s); [ -f CODE_EPOCH ] && [ "$(cat CODE_EPOCH)" -gt "$n" ] && n=$(cat CODE_EPOCH)
  if [ -z "$r" ]; then
    if [ "$(epoch $s)" -ge "$n" ]; then git push -q $U $s:$M && echo "$c: r=none -> PUSH s=$(short $s) (epoch $(epoch $s))"; else echo "$c: r=none, s is epoch $(epoch $s) < known epoch $n -> REFUSE to republish"; fi
  else git fetch -q $U $M:refs/remotes/mock/x 2>/dev/null; git fetch -q $U +$M:refs/remotes/mock/x
    if [ "$(epoch $r)" -gt "$(epoch $s)" ]; then adopt $c $r $s
    elif ge $r $s; then git update-ref $M $r; echo "$c: r=$(short $r) >= s -> no fire"
    elif ge $s $r; then git push -q --force-with-lease=$M:$r $U $s:$M && echo "$c: r=$(short $r) < s=$(short $s) -> PUSH (repair)"
    else echo "$c: incomparable in one epoch -> no fire"; fi; fi; cd $R; }
adopt() { local c=$1 r=$2 s=$3; local lo=$(git rev-list $s..work 2>/dev/null | tac | tr '\n' ' ')
  git update-ref $M $r; git checkout -q -B carry $r 2>/dev/null; local kept=0
  for x in $lo; do if git cherry-pick -X theirs $x >/dev/null 2>$R/cp.err; then kept=$((kept+1)); else echo "   cherry-pick of $(short $x) failed: $(head -2 $R/cp.err | tr "\n" " ")"; git cherry-pick --abort 2>/dev/null; fi; done
  git branch -q -f work carry; git checkout -q work; git branch -q -D carry
  for ref in $(git for-each-ref --format='%(refname)' refs/mock-seen refs/mock-backup refs/remotes/mock); do git update-ref -d $ref; done
  git reflog expire --expire=now --all; git gc -q --prune=now 2>/dev/null
  echo "$c: ADOPT purge epoch $(epoch $r): mirror := $(short $r); local-only commits carried: $kept of $(echo $lo | wc -w); old-epoch refs scrubbed"; }
for c in A B C D E; do git init -q $c; done
echo "== setup: epoch 0 line s1 < s2 (s2 holds a secret in f); everyone fetched s2"
SEC=hunter2-secret
( cd A; s1=$(cd $R; mkc A "" "ok") ; echo $s1 > ../s1 )
s1=$(cat s1); s2=$(mkc A $s1 "has $SEC"); SECBLOB=$(cd A; git rev-parse $s2:f)
( cd A; git update-ref $M $s2; git branch -q work $s2; git push -q $U $s2:$M )
for c in B C D E; do ( cd $c; git fetch -q $U $M:$M; git branch -q work $M; git checkout -q work ); done
echo "B makes a local, unpushed, legitimate edit on top of s2"
( cd B; git checkout -q work; echo "B local note" > g; git add g; git -c user.name=b -c user.email=b@b commit -qm "B local" )
echo "D has fetched the code tree after the purge (witness epoch 1); E has not"
echo "== 1 A purges: new root p (epoch 1, secret removed), the hook rule, then the push"
p=$(mkc A "" "ok after purge" 1)
( cd A; git fetch -q $U $M:refs/remotes/mock/x 2>/dev/null; for who in "epoch-1 new root over epoch-0 tip:$p" "same-epoch non-ff (a rewind to s1):$s1"; do l=${who##*:}; if ge $l $s2; then echo "hook: ALLOW  ${who%%:*}"; else echo "hook: REFUSE ${who%%:*}"; fi; done
  git update-ref $M $p; git push -q --force-with-lease=$M:$s2 $U $p:$M; git branch -q -f work $p; echo "remote now $(short $(rtip)) epoch $(epoch $p)" )
echo "== 2 B (stale, epoch 0, with a local unpushed commit) runs repair: must adopt, never push"
repair B; echo "remote still $(short $(rtip)); B has the secret blob: $(cd B; git cat-file -e $SECBLOB 2>/dev/null && echo YES || echo no); B keeps its note: $(cd B; git show work:g 2>/dev/null | head -1)"
echo "== 3 a plain clone's mirror push deletes the remote ref"
git --git-dir=origin.git update-ref -d $M; echo "remote now: $(short $(rtip))"
echo "D (stale epoch 0, but its code tree names epoch 1):"; echo 1 > D/CODE_EPOCH; repair D
echo "E (stale epoch 0, has seen no witness at all): the limit"; repair E; echo "remote now $(short $(rtip)) epoch $(epoch $(rtip) 2>/dev/null || echo ?)"
echo "== 3a E has republished the old epoch-0 tip (secret back on the remote); A repairs it by the extended order"
repair A; echo "remote now $(short $(rtip)) epoch $(epoch $(rtip)); secret blob still on remote objects until its gc: $(git --git-dir=origin.git cat-file -e $SECBLOB 2>/dev/null && echo yes || echo no)"
echo "== 3b the same, but A repairs first"
git --git-dir=origin.git update-ref -d $M; repair A; echo "remote now $(short $(rtip)) = p ($(short $p))"
echo "== 4 idempotence: every clone repairs twice more"
for i in 1 2; do for c in A B D; do repair $c >/dev/null; done; done; echo "remote still $(short $(rtip))"
