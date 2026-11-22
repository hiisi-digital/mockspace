#!/bin/bash
# AD-HOC QUICK SPIKE, NOT A BENCH (no harness, one machine, 10 commands per arm). Extends 012_probes/f1 from
# 3000 to 30000 files to test its linear extrapolation. Hook run DIRECT by core.fsmonitor (the git-shook
# dispatcher adds a roughly constant ~11 ms per f1 and currently swallows stdin, g1b). Arms per `git status`:
#   A no fsmonitor;  B floor (answers "everything changed");  D snapshot on every command (copied index,
#   add -A, write-tree, commit-tree only if the tree changed, update-ref).  Then D after one file edited.
set -u
exec </dev/null
R=${1:?scratch}; N=${2:-30000}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b dev r; cd r; mkdir -p f; for i in $(seq $N); do head -c 1024 /dev/urandom > f/$i; done; git add -A; git commit -qm s
printf '#!/bin/sh\nprintf "tok\\0/\\0"\n' > "$R/floor.sh"
cat > "$R/snap.sh" <<'H'
#!/bin/sh
[ -n "${MOCK_SNAP:-}" ] && { printf 'tok\0/\0'; exit 0; }
export MOCK_SNAP=1
gd=$(git rev-parse --absolute-git-dir); export GIT_INDEX_FILE="$gd/snap.idx"; cp "$gd/index" "$GIT_INDEX_FILE"
git add -A >/dev/null 2>&1; t=$(git write-tree)
last=$(git rev-parse -q --verify 'refs/mock-snapshot/x^{tree}' 2>/dev/null)
[ "$t" = "$last" ] || git update-ref refs/mock-snapshot/x "$(git commit-tree "$t" -p HEAD -m snap)"
printf 'tok\0/\0'
H
chmod +x "$R"/*.sh
t() { local s e sum=0; for i in $(seq 10); do s=$(date +%s%N); git status --short >/dev/null 2>&1; e=$(date +%s%N); sum=$((sum + (e-s)/1000)); done; echo "$((sum/10/1000)) ms"; }
echo "files: $N, git $(git --version | cut -d' ' -f3)"
git status >/dev/null; echo "A no fsmonitor:              $(t)"
git config core.fsmonitor "$R/floor.sh"; echo "B floor:                     $(t)"
git config core.fsmonitor "$R/snap.sh";  echo "D snapshot every command:    $(t)"
echo x >> f/1; echo "D after one file edited:     $(t)  snapshot holds edit: $(git cat-file -p refs/mock-snapshot/x^{tree} >/dev/null && git show refs/mock-snapshot/x:f/1 | tail -c 2 | od -c | head -1 | grep -c 'x')"
