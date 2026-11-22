#!/bin/bash
# AD-HOC QUICK SPIKE, NOT A BENCH. One machine, one run of 30 commands per arm, git 2.43.0, the real git-shook
# dispatcher (/home/user/git-shook/git-shook, 0.1.0). Question: what does a core.fsmonitor pre-write copy cost
# per git command, and is there any cost between commands? Tree: 3000 files of 20 KB (the "data").
# Arms: A no fsmonitor; B hook direct, protocol floor (answers "everything changed"); C the same hook behind the
# git-shook dispatcher; D behind shook, snapshot (copied index + add -A + write-tree + commit-tree + update-ref)
# on every command; E behind shook, the snapshot throttled to once per 60 s; F behind shook, snapshot only when
# a cheap stat check says something changed (find -newer a stamp file).
set -u
R=${1:?scratch}; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q -b d r; cd r; mkdir f; for i in $(seq 3000); do head -c 20000 /dev/urandom > f/$i; done; git add -A; git commit -qm x
SH=/home/user/git-shook/git-shook
cat > "$R/floor.sh" <<'H'
#!/bin/sh
printf 'tok\0/\0'
H
cat > "$R/snap.sh" <<'H'
#!/bin/sh
# every command: snapshot the worktree into a ref via a copied index
[ -n "${MOCK_SNAP:-}" ] && { printf 'tok\0/\0'; exit 0; }
export MOCK_SNAP=1
gd=$(git rev-parse --absolute-git-dir); export GIT_INDEX_FILE="$gd/snap.idx"; cp "$gd/index" "$GIT_INDEX_FILE" 2>/dev/null
git add -A >/dev/null 2>&1; t=$(git write-tree); c=$(git commit-tree "$t" -p HEAD -m snap); git update-ref refs/mock-snapshot/x "$c"
printf 'tok\0/\0'
H
cat > "$R/throttled.sh" <<'H'
#!/bin/sh
[ -n "${MOCK_SNAP:-}" ] && { printf 'tok\0/\0'; exit 0; }
gd=$(git rev-parse --absolute-git-dir); st="$gd/snap.stamp"
if [ -f "$st" ] && [ $(( $(date +%s) - $(stat -c %Y "$st") )) -lt 60 ]; then printf 'tok\0/\0'; exit 0; fi
touch "$st"; exec sh "$(dirname "$0")/snap.sh" "$@"
H
cat > "$R/changed.sh" <<'H'
#!/bin/sh
# cheap check: any file newer than the stamp?  one find, no hashing
[ -n "${MOCK_SNAP:-}" ] && { printf 'tok\0/\0'; exit 0; }
gd=$(git rev-parse --absolute-git-dir); st="$gd/snap2.stamp"
if [ -f "$st" ] && [ -z "$(find f -newer "$st" -print -quit 2>/dev/null)" ]; then printf 'tok\0/\0'; exit 0; fi
touch "$st"; exec sh "$(dirname "$0")/snap.sh" "$@"
H
t() { local s e sum=0; git update-ref -d refs/mock-snapshot/x 2>/dev/null; for i in $(seq 30); do s=$(date +%s%N); git status --short </dev/null >/dev/null 2>&1; e=$(date +%s%N); sum=$((sum + (e-s)/1000)); done; echo "$((sum/30/1000)) ms per git status (mean of 30); snapshot ref made: $(git rev-parse -q --verify refs/mock-snapshot/x >/dev/null && echo yes || echo no)"; }
echo "A no fsmonitor:                   $(t)"
git config core.fsmonitor "$R/floor.sh";                       echo "B hook direct, protocol floor:    $(t)"
# shook: one manifest, one entry per arm
shook_arm() { # <script>
  git config --unset core.fsmonitor; rm -rf .shook shook.toml; printf '[hooks.fsmonitor-watchman]\nrun = ["sh", "%s"]\n' "$1" > shook.toml
  git add shook.toml; git commit -qm shook -- shook.toml 2>/dev/null; git update-ref -d refs/mock-snapshot/x 2>/dev/null
  sh "$SH" install --yes >"$R/install.out" 2>&1; [ -x .shook/hooks/fsmonitor-watchman ] || { echo "SHOOK INSTALL DID NOT PRODUCE THE DISPATCHER:"; head -3 "$R/install.out"; }
  git config core.fsmonitor "$PWD/.shook/hooks/fsmonitor-watchman"; }
shook_arm "$R/floor.sh";     echo "C shook dispatcher + floor:      $(t)"
shook_arm "$R/snap.sh";      echo "D shook + snapshot every command: $(t)"
shook_arm "$R/throttled.sh"; rm -f .git/snap.stamp; echo "E shook + snapshot throttled 60 s: $(t)  (first command pays one snapshot)"
shook_arm "$R/changed.sh";   rm -f .git/snap2.stamp; echo "F shook + snapshot if a file changed: $(t)"
echo "== between commands: processes started by the hook that are still alive"; sleep 1; ps -eo pid,cmd | grep -c "[f]smonitor\|[s]nap.sh" | sed 's/^/hook-related processes: /'
echo "== does a person's own core.fsmonitor survive? (it is replaced by the single config value)"; git config core.fsmonitor
echo "== the dispatcher's output reaches git? status still correct after an edit:"; echo x >> f/1; git status --short </dev/null | head -2
