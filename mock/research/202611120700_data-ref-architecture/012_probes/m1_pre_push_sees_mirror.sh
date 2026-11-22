#!/bin/bash
# What does a pre-push hook see of push --mirror (deletions, rewinds), can it refuse, what survives on the
# remote after a destructive mirror, and the re-push / lattice behaviour. file:// remotes, git 2.43.0.
R=${1:?}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev seed; (cd seed; echo s > f; git add -A; git commit -qm s)
git clone -q --bare seed remote.git
# one data history: a chain of commits on branch d of the seed; mkdata appends one and pushes it to refs/mock/research
(cd seed; git checkout -q --orphan d; git rm -rqf . 2>/dev/null; echo base > N.md; git add -A; git commit -qm base; git checkout -q dev)
mkdata() { (cd seed; git checkout -q d; echo "$2" >> N.md; git commit -qam "data $1"; git push -q "$R/remote.git" HEAD:refs/mock/research 2>/dev/null; git rev-parse HEAD; git checkout -q dev) }
T1=$(mkdata 1 one 2>/dev/null | tail -1)
git clone -q "$R/remote.git" plain; git clone -q "$R/remote.git" plain0
cat > hook.sh <<'H'
#!/bin/sh
# pre-push: log every line git hands us, refuse a deletion or a rewind of refs/mock/*
log="$HOOKLOG"; echo "args: $*" >> "$log"
while read lref lsha rref rsha; do
  echo "line: [$lref] [$lsha] [$rref] [$rsha]" >> "$log"
  case "$rref" in refs/remotes/*|refs/mock-wt/*|refs/mock-snapshot/*) echo "refusing: $rref is a clone-local ref, this looks like push --mirror" >&2; exit 1;; esac
  case "$rref" in refs/mock/*)
    z=0000000000000000000000000000000000000000
    if [ "$lsha" = "$z" ]; then echo "refusing to delete $rref" >&2; exit 1; fi
    if [ "$rsha" != "$z" ] && ! git merge-base --is-ancestor "$rsha" "$lsha" 2>/dev/null; then echo "refusing a rewind or divergence of $rref (remote $rsha not an ancestor of $lsha)" >&2; exit 1; fi;;
  esac
done
exit 0
H
chmod +x hook.sh; mkdir -p plain/.git/hooks; cp hook.sh plain/.git/hooks/pre-push
echo "== 0. a hook that only LOGS: does pre-push list the deletion of refs/mock/research made by push --mirror?"
printf '#!/bin/sh\necho "args: $*" >> "$HOOKLOG"; while read a b c d; do echo "line: [$a] [$b] [$c] [$d]" >> "$HOOKLOG"; done; exit 0\n' > logonly.sh; chmod +x logonly.sh; cp logonly.sh plain0/.git/hooks/pre-push; export HOOKLOG="$R/log0"
(cd plain0; git push --mirror origin 2>&1 | grep -v negotiat | grep "deleted\|rejected"); echo "lines the hook was handed:"; sed "s#$R/##" "$R/log0" | grep line | cut -c1-120; echo "deletion line among them: $(grep -c 'refs/mock/research' "$R/log0")"; git -C "$R/remote.git" update-ref refs/mock/research $(git -C seed rev-parse d); for r in $(git -C "$R/remote.git" for-each-ref --format="%(refname)" refs/remotes); do git -C "$R/remote.git" update-ref -d $r; done; cp hook.sh plain/.git/hooks/pre-push
echo "== 1. plain clone with the hook, push --mirror (it has no refs/mock): what does the hook see, does it refuse?"
export HOOKLOG="$R/log1"; (cd plain; git push --mirror origin 2>&1 | grep -v negotiat | head -3; echo "rc=${PIPESTATUS[0]}"); cat "$R/log1" | sed "s#$R/##"; echo "remote still has: $(git -C remote.git for-each-ref --format='%(refname)' refs/mock | tr '\n' ' ')"
echo "(the hook above now also refuses when it sees refs/remotes/* on the way out; case 1 repeated:)"; cp hook.sh plain/.git/hooks/pre-push; export HOOKLOG="$R/log1b"; (cd plain; git push --mirror origin 2>&1 | grep -v negotiat | head -2); echo "remote still has: $(git -C remote.git for-each-ref --format='%(refname)' refs/mock | tr '\n' ' ')"
echo "== 2. the same without the hook: remote loses it, objects remain?"
rm plain/.git/hooks/pre-push; (cd plain; git push -q --mirror origin 2>&1 | grep -v negotiat | head -1); echo "remote refs/mock: $(git -C remote.git for-each-ref refs/mock | wc -l); old tip object still on the remote: $(git -C remote.git cat-file -t $T1 2>&1)"
echo "dangling on the remote (fsck): $(git -C remote.git fsck --no-reflogs --unreachable 2>/dev/null | grep -c commit) commit(s); restore by update-ref: $(git -C remote.git update-ref refs/mock/research $T1 && git -C remote.git for-each-ref --format='%(refname)' refs/mock)"
echo "== 2b. the heuristic hook again, but the remote already carries refs/remotes/* from the mirror in 2 (nothing new for the hook to see)"
git -C "$R/remote.git" update-ref refs/mock/research $T1; cp hook.sh plain/.git/hooks/pre-push; export HOOKLOG="$R/log2b"
(cd plain; git push --mirror origin 2>&1 | grep -v negotiat | grep "deleted\|refus\|rejected"); echo "lines handed to the hook: $(grep -c '^line' "$R/log2b"); remote refs/mock after: $(git -C "$R/remote.git" for-each-ref refs/mock | wc -l)  (0 means the deletion went through the hook)"
git -C "$R/remote.git" update-ref refs/mock/research $T1; rm plain/.git/hooks/pre-push
echo "== 3. rewind: stale clone mirror-pushes an old tip over a newer remote tip; the hook sees both shas"
T2=$(mkdata 2 two 2>/dev/null | tail -1)
git clone -q "$R/remote.git" stale; (cd stale; git fetch -q origin '+refs/mock/*:refs/mock/*'); T3=$(mkdata 3 three 2>/dev/null | tail -1)
cp hook.sh stale/.git/hooks/pre-push; export HOOKLOG="$R/log3"; (cd stale; git push --mirror origin 2>&1 | grep -v negotiat | head -2); sed "s#$R/##" "$R/log3" | grep "line"; echo "remote tip is still the newest: $([ "$(git -C remote.git rev-parse refs/mock/research)" = "$T3" ] && echo yes || echo NO)"
echo "== 4. no hook anywhere, rewind lands; a newer holder re-pushes; the stale holder cannot undo it"
rm stale/.git/hooks/pre-push; (cd stale; git push -q --mirror origin 2>&1 | grep -v negotiat | head -1); echo "remote tip after the stale mirror: $(git -C remote.git rev-parse --short refs/mock/research) (stale=$(git -C seed rev-parse --short $T2) newest=$(git -C seed rev-parse --short $T3))"
git clone -q "$R/remote.git" holder; (cd holder; git fetch -q origin '+refs/mock/*:refs/mock/*' 2>/dev/null); (cd seed; git push -q "$R/remote.git" $T3:refs/mock/research 2>&1 | grep -v negotiat | head -1); echo "the holder of the newest tip pushes it back (a fast-forward, no force): remote now $(git -C remote.git rev-parse --short refs/mock/research)"
(cd stale; git fetch -q origin '+refs/mock/*:refs/mock/*'; git push origin refs/mock/research:refs/mock/research 2>&1 | grep -c "rejected\|Everything up-to-date" | sed 's/^/stale clone re-pushing its old tip: rejected-or-noop lines: /')
echo "== 4b. what a holder's mirror ref does when the remote was rewound: unforced against forced fetch refspec"
git -C "$R/remote.git" update-ref refs/mock/research $T3; git clone -q "$R/remote.git" h2; cd h2; git fetch -q origin 'refs/mock/research:refs/mock/research'; echo "holder mirror at newest: $(git rev-parse --short refs/mock/research)"
git -C "$R/remote.git" update-ref refs/mock/research $T2
git fetch origin 'refs/mock/research:refs/mock/research' 2>&1 | grep -v negotiat | tail -1; echo "unforced fetch of the rewound remote: local mirror $(git rev-parse --short refs/mock/research) (kept the newest if it equals $(git -C "$R/seed" rev-parse --short $T3))"
git fetch -q origin '+refs/mock/research:refs/mock/research' 2>&1 | tail -1; echo "forced (+) fetch: local mirror $(git rev-parse --short refs/mock/research) (the newer tip is now held by no mirror ref in this clone)"; cd "$R"
echo "== 5. the case with no holder: delete on the remote, every local copy gone"
git -C remote.git update-ref -d refs/mock/research; git -C remote.git gc -q --prune=now 2>/dev/null; echo "after remote gc --prune=now, old tip present on the remote: $(git -C remote.git cat-file -t $T3 2>&1 | head -1)"
