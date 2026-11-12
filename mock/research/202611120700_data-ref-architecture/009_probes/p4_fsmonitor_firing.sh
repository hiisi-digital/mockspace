#!/bin/bash
# 009/p4: the audit's core.fsmonitor "pre-write copy" (007 a14). Which git verbs call the hook BEFORE they write?
# The hook logs the calling git command line and whether the data edit was still on disk at that moment.
# Coverage only: no cost is measured here (unpriced), and no user-owned fsmonitor is chained.
set -u
S=${1:?scratch}; rm -rf $S/p4; mkdir -p $S/p4 && cd $S/p4
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
LOG=$S/p4/hook.log
cat > hook.sh <<HOOK
#!/bin/bash
echo "\$(tr '\\0' ' ' < /proc/\$PPID/cmdline | cut -c1-48) | edit on disk: \$(cat $S/p4/m/mock/study/N.md 2>&1 | head -1)" >> $LOG
printf '%s\\0/\\0' "\$2"
HOOK
chmod +x hook.sh
mk() { rm -rf m; git init -q -b dev m && cd m; mkdir -p mock; echo c > c.rs; git add -A; git commit -qm i; git branch other
  git worktree add -q --orphan -b mock/study mock/study; (cd mock/study; echo d > N.md; git add N.md; git commit -qm d)
  git config core.fsmonitor $S/p4/hook.sh; echo EDIT > mock/study/N.md; : > $LOG; }
for v in "checkout -f other" "reset --hard HEAD" "clean -ffdx" "clean -ffd" "stash -u" "merge other" "cherry-pick other" "rebase other" "restore ." "switch other" "pull" "status" "gc" "worktree prune" "checkout-index -a -f" "read-tree -u --reset HEAD" "am --abort" "bisect start" "rm -r --cached ." "diff" "log"; do
  mk; git $v >/dev/null 2>&1; n=$(grep -c . $LOG); printf '%-30s hook called %-2s time(s); first call saw: %s\n' "git $v" "$n" "$(head -1 $LOG | sed 's/.*| //')"; cd ..; done
mk; rm -rf mock/study; git status >/dev/null 2>&1; echo "rm -rf of the data dir, then status: hook saw edit? $(head -1 $LOG | sed 's/.*| //')  (the delete already happened, so no)"; cd ..
mk; git -c core.fsmonitor=false clean -ffdx >/dev/null 2>&1; echo "git -c core.fsmonitor=false clean -ffdx: hook calls $(grep -c . $LOG), edit now: $(cat mock/study/N.md 2>&1 | head -1)"; cd ..
mk; git worktree remove --force mock/study >/dev/null 2>&1; echo "worktree remove --force: hook calls $(grep -c . $LOG)"
