#!/usr/bin/env nutshell
# =============================================================================
# probe.sh - do refs outside refs/heads carry data through plain git?
# =============================================================================
# P0 of the refs port. A local scratch repository and a local bare repository
# on the filesystem, nothing else: no forge, no proxy, no kaski. Creates
# `refs/mock/registry/probe` and `refs/heads/mock/probe`, pushes both, fetches
# with an explicit refspec, takes a shallow clone with the refspec, lists with
# `ls-remote`, and loses a lease push to a race on purpose.
#
# Each numbered case prints `ok` or `FAIL`. Cases 4, 8, 9 and 10 are the ones
# that must fail: they assert that git refuses, and a probe in which git did
# not refuse is a failed probe. Exit 0 when every case held, 1 otherwise.
#
# What this cannot show: whether the cloud proxy or GitHub accepts a push of a
# ref outside `refs/heads`. The remote here is a bare repository on a disk.
#
# Copyright (c) 2026 orgrinrt <ort@hiisi.digital>
# SPDX-License-Identifier: MPL-2.0
# =============================================================================

set -uo pipefail

W="$(mktemp -d)"
trap 'rm -rf "$W"' EXIT
FAILS=0
export GIT_AUTHOR_NAME=probe GIT_AUTHOR_EMAIL=probe@example.invalid
export GIT_COMMITTER_NAME=probe GIT_COMMITTER_EMAIL=probe@example.invalid
export GIT_TERMINAL_PROMPT=0

MOCK=refs/mock/registry/probe
BRANCH=refs/heads/mock/probe

ok()   { printf 'ok    %s\n' "$1"; }
fail() { printf 'FAIL  %s\n' "$1"; FAILS=$((FAILS + 1)); }
note() { printf '      %s\n' "$1"; }

# commit <repo> <parent or -> <file> <content> <message>: a commit holding exactly one file,
# built by plumbing with no working tree and no checkout; prints the commit id
commit() {
    local repo=$1 parent=$2 file=$3 content=$4 msg=$5 blob idx tree
    blob="$(printf '%s\n' "$content" | git -C "$repo" hash-object -w --stdin)"
    idx="$(mktemp -u)"
    GIT_INDEX_FILE="$idx" git -C "$repo" update-index --add --cacheinfo "100644,$blob,$file"
    tree="$(GIT_INDEX_FILE="$idx" git -C "$repo" write-tree)"
    rm -f "$idx"
    if [ "$parent" = - ]; then git -C "$repo" commit-tree "$tree" -m "$msg"
    else git -C "$repo" commit-tree "$tree" -p "$parent" -m "$msg"; fi
}

git init -q --bare "$W/bare.git"
git init -q "$W/a"
git -C "$W/a" remote add origin "file://$W/bare.git"

# --- 1. create both refs locally and push both by explicit refspec ------------
C1="$(commit "$W/a" - registry.toml 'rev = 1' 'probe: first')"
git -C "$W/a" update-ref "$MOCK" "$C1"
git -C "$W/a" update-ref "$BRANCH" "$C1"
if git -C "$W/a" push -q origin "$MOCK:$MOCK" "$BRANCH:$BRANCH" 2>"$W/push1.err"; then
    ok "1  push of $MOCK and $BRANCH by explicit refspec"
else
    fail "1  push by explicit refspec: $(cat "$W/push1.err")"
fi

# --- 2. ls-remote sees both, and a pattern selects the namespace ---------------
LS="$(git -C "$W/a" ls-remote origin 'refs/mock/*' 'refs/heads/mock/*' 2>&1)"
if printf '%s\n' "$LS" | grep -q "$C1[[:space:]]$MOCK" && printf '%s\n' "$LS" | grep -q "$C1[[:space:]]$BRANCH"; then
    ok "2  ls-remote lists both refs under their patterns"
else
    fail "2  ls-remote: $LS"
fi

# --- 3. fetch with an explicit refspec into a clone that has neither ref -------
git clone -q "file://$W/bare.git" "$W/b" 2>/dev/null
git -C "$W/b" remote -v >/dev/null
if git -C "$W/b" rev-parse -q --verify "$MOCK" >/dev/null; then
    fail "3a the default clone already holds $MOCK; the probe cannot tell the refspec from the default"
else
    ok "3a a default clone does not carry $MOCK (the refspec is what brings it)"
fi
if git -C "$W/b" fetch -q origin "+$MOCK:$MOCK" 2>"$W/fetch3.err" \
    && [ "$(git -C "$W/b" rev-parse "$MOCK")" = "$C1" ]; then
    ok "3b fetch of +$MOCK:$MOCK brings the ref to the same commit"
else
    fail "3b explicit fetch: $(cat "$W/fetch3.err")"
fi

# --- 4. MUST FAIL: a fetch of a ref the remote does not have is an error --------
git -C "$W/b" fetch -q origin "+refs/mock/registry/absent:refs/mock/registry/absent" 2>"$W/fetch4.err"
RC=$?
if [ "$RC" -ne 0 ] && grep -qi "couldn't find remote ref" "$W/fetch4.err"; then
    ok "4  fetching an absent ref exits $RC and says: $(head -1 "$W/fetch4.err")"
else
    fail "4  fetching an absent ref exited $RC with: $(cat "$W/fetch4.err")"
fi

# --- 5. shallow clone with the refspec ---------------------------------------------
C2="$(commit "$W/a" "$C1" registry.toml 'rev = 2' 'probe: second')"
git -C "$W/a" update-ref "$MOCK" "$C2" "$C1"
git -C "$W/a" push -q origin "$MOCK:$MOCK"
git init -q "$W/c"
git -C "$W/c" remote add origin "file://$W/bare.git"
if git -C "$W/c" fetch -q --depth 1 origin "+$MOCK:$MOCK" 2>"$W/fetch5.err" \
    && [ "$(git -C "$W/c" rev-parse "$MOCK")" = "$C2" ] \
    && [ "$(git -C "$W/c" rev-list --count "$MOCK")" = 1 ]; then
    ok "5a shallow fetch (depth 1) with the refspec: tip $C2, one commit of history"
else
    fail "5a shallow fetch with the refspec: $(cat "$W/fetch5.err")"
fi
git clone -q --depth 1 "file://$W/bare.git" "$W/d" 2>/dev/null
if git -C "$W/d" fetch -q --depth 1 origin "+$MOCK:$MOCK" 2>"$W/fetch5b.err" \
    && [ "$(git -C "$W/d" rev-parse "$MOCK")" = "$C2" ]; then
    ok "5b shallow clone, then the same refspec fetched at depth 1"
else
    fail "5b shallow clone plus refspec: $(cat "$W/fetch5b.err")"
fi

# --- 6. the branch form travels the same way -----------------------------------------
if git -C "$W/c" fetch -q --depth 1 origin "+$BRANCH:refs/mock-local/branch-form" 2>"$W/fetch6.err" \
    && [ "$(git -C "$W/c" rev-parse refs/mock-local/branch-form)" = "$C1" ]; then
    ok "6  $BRANCH fetched into a private local ref, a (local, remote) pair"
else
    fail "6  branch form: $(cat "$W/fetch6.err")"
fi

# --- 7. a lease push that wins ----------------------------------------------------------
git -C "$W/c" fetch -q origin "+$MOCK:$MOCK"
C3="$(commit "$W/c" "$C2" registry.toml 'rev = 3 from c' 'probe: third, from c')"
if git -C "$W/c" push -q "--force-with-lease=$MOCK:$C2" origin "$C3:$MOCK" 2>"$W/push7.err"; then
    ok "7  lease push expecting $C2 moves the ref to $C3"
else
    fail "7  lease push: $(cat "$W/push7.err")"
fi

# --- 8. MUST FAIL: the same lease from a stale writer loses the race --------------------
C3B="$(commit "$W/a" "$C2" registry.toml 'rev = 3 from a' 'probe: third, from a')"
git -C "$W/a" push -q "--force-with-lease=$MOCK:$C2" origin "$C3B:$MOCK" 2>"$W/push8.err"
RC=$?
if [ "$RC" -ne 0 ]; then
    ok "8  the writer holding the old tip is refused, exit $RC: $(grep -E '^ ! |rejected|stale' "$W/push8.err" | head -1)"
else
    fail "8  a stale lease push went through"
fi
if [ "$(git -C "$W/bare.git" rev-parse "$MOCK")" = "$C3" ]; then
    ok "8b the remote still holds the winner's commit"
else
    fail "8b the remote moved to $(git -C "$W/bare.git" rev-parse "$MOCK")"
fi

# --- 9. MUST FAIL: a plain non-fast-forward push is refused without a lease --------------
git -C "$W/a" push -q origin "$C3B:$MOCK" 2>"$W/push9.err"
RC=$?
if [ "$RC" -ne 0 ] && grep -qiE 'non-fast-forward|fetch first|rejected' "$W/push9.err"; then
    ok "9  a plain push of a diverged commit is refused, exit $RC"
else
    fail "9  a diverged plain push exited $RC: $(cat "$W/push9.err")"
fi

# --- 10. MUST FAIL: a remote that cannot be reached is an error, not an empty store -------
git -C "$W/a" fetch -q "file://$W/does-not-exist.git" "+$MOCK:refs/mock/registry/unreached" 2>"$W/fetch10.err"
RC=$?
if [ "$RC" -ne 0 ] && ! git -C "$W/a" rev-parse -q --verify refs/mock/registry/unreached >/dev/null; then
    ok "10 an unreachable remote exits $RC and leaves no ref behind: $(head -1 "$W/fetch10.err")"
else
    fail "10 an unreachable remote exited $RC"
fi

# --- 11. the loser recovers: fetch, merge by hand, push on the new tip -----------------------
git -C "$W/a" fetch -q origin "+$MOCK:refs/mock/remote-tip"
TIP="$(git -C "$W/a" rev-parse refs/mock/remote-tip)"
BLOB="$(printf 'rev = 4 merged\n' | git -C "$W/a" hash-object -w --stdin)"
IDX="$(mktemp -u)"
GIT_INDEX_FILE="$IDX" git -C "$W/a" update-index --add --cacheinfo "100644,$BLOB,registry.toml"
TREE="$(GIT_INDEX_FILE="$IDX" git -C "$W/a" write-tree)"
rm -f "$IDX"
MERGED="$(git -C "$W/a" commit-tree "$TREE" -p "$C3B" -p "$TIP" -m 'probe: merge of both sides')"
if git -C "$W/a" push -q "--force-with-lease=$MOCK:$TIP" origin "$MERGED:$MOCK" 2>"$W/push11.err"; then
    ok "11 after the loss, a commit with both tips as parents lands on the lease of the new tip"
else
    fail "11 recovery push: $(cat "$W/push11.err")"
fi
if git -C "$W/bare.git" merge-base --is-ancestor "$C3" "$MERGED" && git -C "$W/bare.git" merge-base --is-ancestor "$C3B" "$MERGED"; then
    ok "11b both racers' commits are ancestors of the remote tip: history keeps what each wrote"
else
    fail "11b history lost a racer"
fi

echo
if [ "$FAILS" -eq 0 ]; then echo "probe: every case held"; exit 0; fi
echo "probe: $FAILS case(s) failed"
exit 1
