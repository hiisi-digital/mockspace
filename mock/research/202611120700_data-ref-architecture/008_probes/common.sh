export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t
# mk <dir> : repo with dev (main.rs + old tracked mock/research/N.md), migration commit removing it, orphan data branch "data" holding N.md
mk() { rm -rf "$1"; git init -q -b dev "$1"; cd "$1"; mkdir -p mock/research; echo old > mock/research/N.md; echo s > main.rs
  git add -A; git commit -qm old; OLD=$(git rev-parse HEAD); git rm -rqf mock/research; git commit -qm migrate
  DC=$(git commit-tree "$OLD:mock/research" -m data); git branch data $DC; }
