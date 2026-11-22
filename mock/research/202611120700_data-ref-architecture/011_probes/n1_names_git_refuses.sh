#!/usr/bin/env bash
# Which path components git refuses to track (core.protectNTFS default on).
set -u
S=${1:-$(mktemp -d)}; mkdir -p "$S" && cd "$S" || exit 1; cd "$S" && git init -q r && cd r && git --version
for p in 'git~1' 'GIT~1' 'Git~1' 'git~2' '.git~1' '~mock' '~git~1' 'git~1x' 'xgit~1' '.git ' '.git.' '@' 'gitmod~1' '.gitmodules'; do
  mkdir -p "$p"; printf x > "$p/f"; h=$(git hash-object -w "$p/f")
  r=$(git update-index --add --cacheinfo 100644,$h,"$p/f" 2>&1 && echo tracked)
  printf '%-14s %s\n' "[$p]" "$(echo $r | cut -c1-70)"
done
