#!/bin/bash
# Spike g1: settles where the git-shook stdin defect bites (ticket git_shook_s_dispatcher_reads_stdin_whole_so_an_fsmonitor_hoo).
# The dispatcher does `[ -t 0 ] || cat >"$tmp/stdin"` (/home/user/git-shook/git-shook), so a terminal is safe.
# Question: what happens when git runs with a NON-terminal stdin, as in every script that loops over input
# (`... | while read f; do git diff -- "$f"; done`), in an editor's subprocess, or in an agent harness?
# Arm 1: the real dispatcher behind core.fsmonitor with a floor entry. Arm 2: no fsmonitor (control).
set -u
R=${1:?scratch}; SH=/home/user/git-shook/git-shook; rm -rf "$R"; mkdir -p "$R"; cd "$R"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
exec </dev/null   # the first run of this script hung here, on the harness socket as stdin: g1_hang_evidence.txt
git init -q -b dev r; cd r; for i in 1 2 3 4 5; do echo $i > f$i; done; git add -A; git commit -qm s
printf '#!/bin/sh\nprintf "tok\\0/\\0"\n' > "$R/floor.sh"; chmod +x "$R/floor.sh"
printf '[hooks.fsmonitor-watchman]\nrun = ["sh", "%s"]\n' "$R/floor.sh" > shook.toml; git add shook.toml; git commit -qm shook
sh "$SH" install --yes >"$R/install.out" 2>&1; [ -x .shook/hooks/fsmonitor-watchman ] || { echo "install failed"; head -3 "$R/install.out"; }
loop() { local n=0; git ls-files | while read -r f; do git diff --quiet -- "$f" </dev/stdin; echo "$f"; done | wc -l; }
git config --unset core.fsmonitor 2>/dev/null
echo "control, no fsmonitor: the read loop visited $(git ls-files | while read -r f; do git diff --quiet -- "$f"; echo x; done | wc -l) of $(git ls-files | wc -l) files"
git config core.fsmonitor "$PWD/.shook/hooks/fsmonitor-watchman"
echo "fsmonitor via git-shook: the read loop visited $(git ls-files | while read -r f; do git diff --quiet -- "$f"; echo x; done | wc -l) of $(git ls-files | wc -l) files"
echo "same loop with git's stdin closed (</dev/null): $(git ls-files | while read -r f; do git diff --quiet -- "$f" </dev/null; echo x; done | wc -l)"
