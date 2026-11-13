#!/bin/bash
# Spike c1: settles the identity-versus-remote-tracking disagreement (008 2 vs 009 2.4) by crossing the local
# mapping of an opted-in clone with the two kinds of remote damage `push --mirror` does, and asking what the
# NEXT engine update in an up-to-date opted-in clone B sees: does the fetch fail loudly, and does B's local
# copy of the newest data tip survive. Mappings (the fetch refspec of the dedicated remote in B):
#   ID+glob   +refs/mock/*:refs/mock/*                       (008)
#   TR+       +refs/mock/research:refs/remotes/mock/research   (009)
#   TR-       refs/mock/research:refs/remotes/mock/research    (no '+': non-fast-forward refused)
#   ID-       refs/mock/research:refs/mock/research            (no '+')
# Damage: DEL = a plain clone's push --mirror deletes refs/mock/research on the remote;
#         REW = a stale opted-in clone's identity-mapped push --mirror rewinds it to an older commit.
# B fetches with fetch.prune unset and with fetch.prune=true.
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
mk() { # one data commit on top of $1 (or root)
  local t; t=$(printf '100644 blob %s\tN.md\n' "$(echo "$2" | git hash-object -w --stdin)" | git mktree); git commit-tree $t ${1:+-p $1} -m "$2"; }
for MAP in "ID+glob:+refs/mock/*:refs/mock/*:refs/mock/research" "TR+:+refs/mock/research:refs/remotes/mock/research:refs/remotes/mock/research" "TR-:refs/mock/research:refs/remotes/mock/research:refs/remotes/mock/research" "ID-:refs/mock/research:refs/mock/research:refs/mock/research"; do
  NAME=${MAP%%:*}; rest=${MAP#*:}; LOCAL=${rest##*:}; SPEC=${rest%:*}
  for DMG in DEL REW; do for PRUNE in off on; do
    rm -rf o.git p b s; git init -q --bare -b dev o.git; U="file://$S/o.git"
    git -C o.git config receive.denyNonFastForwards false
    ( cd o.git; D=$(git commit-tree $(git mktree </dev/null) -m dev); git update-ref refs/heads/dev $D; c1=$(GIT_DIR=. mk "" one); c2=$(GIT_DIR=. mk $c1 two); git update-ref refs/mock/research $c2; echo $c1 > ../c1; echo $c2 > ../c2 )
    C1=$(cat c1); C2=$(cat c2)
    git clone -q "$U" b; git -C b remote add mock "$U"; git -C b config --unset-all remote.mock.fetch; git -C b config remote.mock.fetch "$SPEC"; git -C b config remote.mock.tagOpt --no-tags
    git -C b fetch -q mock; [ $PRUNE = on ] && git -C b config fetch.prune true
    if [ $DMG = DEL ]; then git clone -q "$U" p; git -C p push -q --mirror origin 2>/dev/null
    else git clone -q "$U" s; git -C s fetch -q origin "+refs/mock/research:refs/mock/research"; git -C s update-ref refs/mock/research $C1; git -C s push -q --mirror origin 2>/dev/null; fi
    out=$(git -C b fetch mock 2>&1); rc=$?
    held=$(git -C b rev-parse -q --verify "$LOCAL" 2>/dev/null); state=gone; [ "$held" = "$C2" ] && state=newest; [ "$held" = "$C1" ] && state=REWOUND
    printf '%-8s %-3s prune=%-3s fetch rc=%s local %-34s=%-8s msg={%s}\n' "$NAME" "$DMG" "$PRUNE" "$rc" "$LOCAL" "$state" "$(echo "$out" | grep -E 'rejected|error|fatal|deleted|forced' | head -1 | sed 's/^ *//')"
  done; done
done
