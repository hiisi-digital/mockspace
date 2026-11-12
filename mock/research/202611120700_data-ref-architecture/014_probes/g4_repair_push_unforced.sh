#!/bin/bash
# Spike g4: the re-push disagreement. 013 (seat 002) repairs with `--force-with-lease=<ref>:<r>`; 012 (seat 001)
# with a plain unforced push. Repair state: the clone's monotone tip s = c3 (chain c1-c2-c3). For each remote
# state r observed at check time, and each concurrent move between check and push, run the UNFORCED push of s
# and record whether the remote ends at a tip that is >= every tip anyone pushed (no loss) and whether it fired.
set -u
exec </dev/null
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
git init -q --bare o.git; git init -q c; cd c
mk() { git commit-tree $(printf '100644 blob %s\tN.md\n' "$(echo "$2" | git hash-object -w --stdin)" | git mktree) ${1:+-p $1} -m "$2"; }
c1=$(mk "" c1); c2=$(mk $c1 c2); c3=$(mk $c2 c3); c4=$(mk $c3 c4); x2=$(mk $c1 x2); git push -q ../o.git $c4:refs/heads/keep $x2:refs/heads/keepx 2>/dev/null
name() { case $1 in $c1) echo c1;; $c2) echo c2;; $c3) echo c3;; $c4) echo c4;; $x2) echo x2;; "") echo none;; *) echo ?;; esac; }
row() { # $1 label, $2 remote state at check (sha or empty), $3 concurrent move before push (sha, empty=none, DEL=delete)
  git -C ../o.git update-ref -d refs/mock/x 2>/dev/null; [ -n "$2" ] && git -C ../o.git update-ref refs/mock/x $2
  if [ "$3" = DEL ]; then git -C ../o.git update-ref -d refs/mock/x; elif [ -n "$3" ]; then git -C ../o.git update-ref refs/mock/x $3; fi
  out=$(git push ../o.git $c3:refs/mock/x 2>&1); rc=$?
  printf '%-44s push rc=%s remote now=%s {%s}\n' "$1" "$rc" "$(name $(git -C ../o.git rev-parse -q --verify refs/mock/x))" "$(echo "$out" | grep -E 'rejected|new|\.\.' | head -1 | sed 's/^ *//' | cut -c1-60)"
}
row "deleted (r none), no concurrent move"        ""   ""
row "rewound to c1, no concurrent move"           $c1  ""
row "rewound to c1, someone advances to c4"       $c1  $c4
row "rewound to c1, someone diverges to x2"       $c1  $x2
row "rewound to c1, rewound again to c2"          $c1  $c2
row "deleted, someone recreates at x2"            ""   $x2
row "equal to s (steady state)"                   $c3  ""
row "ahead of s (legit change) c4"                $c4  ""
