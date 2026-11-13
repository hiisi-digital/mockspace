#!/bin/bash
# push --mirror from a plain clone deletes the remote's refs/mock/<ns>. Does receive.denyDeletes (self-hosted
# remotes) stop it, and does a client-side pushurl / remote.<n>.mirror setting matter? Minimal, one ref.
R=${1:?}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev seed; (cd seed; echo s > f; git add -A; git commit -qm s)
git clone -q --bare seed remote.git; git -C seed push -q "$R/remote.git" dev:refs/mock/research
git clone -q "$R/remote.git" plain
for deny in false true; do
  git -C remote.git update-ref refs/mock/research $(git -C seed rev-parse dev); git -C remote.git config receive.denyDeletes $deny
  (cd plain; git push --mirror origin >"$R/out" 2>&1; echo "denyDeletes=$deny: rc=$? $(grep -c '\[deleted\]' "$R/out") deleted line(s), $(grep -c 'remote rejected' "$R/out") remote-rejected; remote still has refs/mock/research: $(git -C "$R/remote.git" for-each-ref refs/mock | wc -l)")
done
# receive.denyDeletes only covers refs/heads/ (receive-pack checks the prefix), so it does not protect
# refs/mock/. A server-side update hook does.
git -C remote.git config receive.denyDeletes false
printf '#!/bin/sh\n[ "$3" = 0000000000000000000000000000000000000000 ] && case "$1" in refs/mock/*) echo "refs/mock deletion denied" >&2; exit 1;; esac\nexit 0\n' > remote.git/hooks/update; chmod +x remote.git/hooks/update
git -C remote.git update-ref refs/mock/research $(git -C seed rev-parse dev)
(cd plain; git push --mirror origin >"$R/out" 2>&1; echo "update hook: rc=$? $(grep -c 'remote rejected' "$R/out") remote-rejected; remote still has refs/mock/research: $(git -C "$R/remote.git" for-each-ref refs/mock | wc -l)")
