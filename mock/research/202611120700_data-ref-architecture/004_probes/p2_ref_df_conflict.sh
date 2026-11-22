#!/bin/bash
# 004/p2: attack on 001 section 3 item 6: snapshot ref refs/mock-backup/<ns>/<worktree-name> while the shipped engine
# already holds refs/mock-backup/<ns> (backup.rs:6). A ref and a ref-directory of the same name cannot coexist.
set -u
S=${1:?scratch}; rm -rf $S/q2; mkdir -p $S/q2 && cd $S/q2
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
git init -q r && cd r && git commit -q --allow-empty -m i
git update-ref refs/mock-backup/research HEAD
git update-ref refs/mock-backup/research/wt1 HEAD 2>&1 | head -2; echo "rc after the shipped name exists: ${PIPESTATUS[0]}"
git update-ref -d refs/mock-backup/research
git update-ref refs/mock-backup/research/wt1 HEAD; echo "layout alone: rc=$?"
git update-ref refs/mock-backup/research HEAD 2>&1 | head -2; echo "then the shipped name: rc=${PIPESTATUS[0]}"
echo "== two namespaces where one is a prefix of the other (validate_namespace allows registry/ticket style names)"
git update-ref refs/mock/a HEAD; git update-ref refs/mock/a/b HEAD 2>&1 | head -1
