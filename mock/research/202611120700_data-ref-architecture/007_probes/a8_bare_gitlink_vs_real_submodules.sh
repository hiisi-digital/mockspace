#!/bin/bash
# Spike a8: the cost side of arm G'. A consumer repository that also has a REAL submodule `lib`. Does the
# unmapped placeholder gitlink at mock/rdata break clone --recurse-submodules and submodule update for `lib`?
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
git config --global protocol.file.allow always
git init -q -b main libsrc; (cd libsrc; echo l > l.rs; git add -A; git commit -qm l)
git init -q -b dev m; cd m; echo s > main.rs; git add -A; git commit -qm s
git submodule add -q ../libsrc lib; git commit -qm lib
D=mock/rdata; git update-index --add --cacheinfo 160000,$(git rev-parse HEAD),$D; git commit -qm placeholder
cd ..; git clone -q --recurse-submodules m rc; echo "clone --recurse-submodules rc=$? lib populated=$(ls rc/lib 2>/dev/null | tr '\n' ' ')"
git clone -q m pc; cd pc; git submodule update --init >/dev/null 2>&1; echo "submodule update --init rc=$? lib populated=$(ls lib | tr '\n' ' ')"
git submodule status >/dev/null 2>&1; echo "submodule status rc=$?"; git submodule foreach -q true >/dev/null 2>&1; echo "submodule foreach rc=$?"
git -c submodule.recurse=true checkout -q -f HEAD~1 2>&1 | head -1; echo "recurse checkout rc=${PIPESTATUS[0]}"
