#!/bin/bash
# Spike a14: can core.fsmonitor serve as a PRE-command interception point for the user cache? git runs the
# fsmonitor hook when it loads an index, which is before most porcelain verbs write the tree. The hook here
# copies the data worktree to a cache directory (guarded against re-entry), then answers "everything may have
# changed" so git falls back to its own scan. For each destroying verb: did a copy holding the EDIT exist in
# the cache after the verb, though the verb itself destroyed it on disk?
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
D=mock/rdata; CACHE="$S/cache"
cat > "$S/hook.sh" <<'H'
#!/bin/sh
# fsmonitor protocol v2: args <version> <token>; we print a fresh token, NUL, then "/" meaning everything.
if [ -z "${MOCK_SNAP_ACTIVE:-}" ]; then
  export MOCK_SNAP_ACTIVE=1
  top=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null)
  git --git-dir="$top" worktree list --porcelain | sed -n 's/^worktree //p' | while read -r w; do
    case "$w" in */mock/rdata) d="$CACHE_DIR/$(date +%s%N)"; mkdir -p "$d"; cp -a "$w/." "$d/" 2>/dev/null; echo "$*" > "$d/.why";; esac
  done
fi
printf 'tok-%s\0/\0' "$(date +%s%N)"
H
chmod +x "$S/hook.sh"; export CACHE_DIR="$CACHE"
setup() { rm -rf "$S/r" "$S/w2" "$CACHE"; mkdir -p "$CACHE"; git init -q -b dev "$S/r"; cd "$S/r"; echo s > main.rs; git add -A; git commit -qm s
  git checkout -q --orphan mock/research; git rm -rqf .; echo data > N.md; git add -A; git commit -qm data; git checkout -q dev
  git branch -q mock-wt/research/w2 mock/research; git worktree add -q -b feat ../w2 dev
  git worktree add -q $D mock/research; git -C ../w2 worktree add -q $D mock-wt/research/w2
  echo "/$D/" >> .git/info/exclude; git config core.fsmonitor "$S/hook.sh"; echo EDIT >> $D/N.md; echo EDIT >> ../w2/$D/N.md; rm -rf "$CACHE"/*; }
cached() { grep -l EDIT "$CACHE"/*/N.md 2>/dev/null | wc -l; }
for c in "clean -ffdx" "clean -ffdX" "-C mock/rdata reset --hard" "-C mock/rdata checkout -f ." "-C mock/rdata clean -ffdx" "-C mock/rdata stash -u" "-C mock/rdata checkout -f HEAD~0" "worktree remove --force ../w2" "worktree remove -f -f ../w2" "status"; do
  setup; t0=$(date +%s%N); git $c >/dev/null 2>&1; t1=$(date +%s%N)
  W=.; case "$c" in *w2*) W=../w2;; esac
  echo "git $c -> on disk=$(cat $W/$D/N.md 2>/dev/null | grep -c EDIT) copies with EDIT in cache=$(cached) ($(( (t1-t0)/1000000 )) ms)"
done
