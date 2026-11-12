#!/bin/bash
# Spike c3: census of every relative `path = "..."` in every Cargo.toml under <repo>/mock/research. Each is
# resolved lexically from its manifest's directory and classified by where it lands:
#   self   inside mock/research (moves with the data, any placement works)
#   mock   inside mock/ but outside mock/research (breaks if the data's top component changes, e.g. git~1/<ns>)
#   root   outside mock/ (survives any same-depth placement, including git~1/<ns> at the top)
# Decides whether the untrackable component can sit at the TOP (git~1/<ns>, many namespaces at the old depth)
# or only one level down (mock/git~1, one namespace).
set -u
REPO=${1:?repo}; cd "$REPO"
find mock/research -name Cargo.toml | sort | while read -r m; do
  d=$(dirname "$m")
  grep -oE 'path *= *"[^"]+"' "$m" | sed -E 's/.*"([^"]+)"/\1/' | while read -r p; do
    case "$p" in /*) continue;; esac
    r=$(realpath -m --relative-to=. "$d/$p")
    case "$r" in mock/research/*) c=self;; mock/*) c=mock;; *) c=root;; esac
    echo "$c $m -> $p => $r"
  done
done > /tmp/c3.$$; sort /tmp/c3.$$ | awk '{print}' ; echo "== counts"; awk '{print $1}' /tmp/c3.$$ | sort | uniq -c; rm -f /tmp/c3.$$
