# Spike: ref name collisions. (1) the store's own validator (mock/crates/mockspace-store/src/refs.rs:80-102)
# admits '/' inside a namespace, so namespaces `a` and `a/b` are both valid; git cannot hold both as branches
# (directory/file conflict). (2) a snapshot ref name that is a leaf: namespace and worktree id percent-encoded
# into one component, joined by '@' (ns charset is [a-z0-9-_./], so '@' never occurs in an encoded ns).
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q r; cd r; git commit -q --allow-empty -m e; C=$(git rev-parse HEAD)
echo "== data branches for namespaces a and a/b"
git update-ref refs/heads/mock/a $C && echo "refs/heads/mock/a: ok"; git update-ref refs/heads/mock/a/b $C 2>&1 | head -1
enc() { printf '%s' "$1" | sed 's/%/%25/g; s#/#%2F#g; s/@/%40/g'; }
echo "== leaf snapshot refs"
for pair in "a main" "a/b main" "a wt/x" "a.b c" "a b.c"; do set -- $pair; n=$(enc "$1")@$(enc "$2"); git update-ref "refs/mock-snapshot/$n" $C 2>&1 | head -1 && echo "refs/mock-snapshot/$n ok"; done
git for-each-ref refs/mock-snapshot --format='%(refname)'
echo "== and the shipped engine ref refs/mock-backup/<ns> beside the leaf prefix: different prefix, no collision"
git update-ref refs/mock-backup/a $C && git update-ref refs/mock-snapshot/zz $C && echo ok
