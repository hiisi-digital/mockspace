#!/bin/bash
# Spike a12: the bare clone with linked worktrees, end to end, in the composed shape of 007 section 4:
# one data worktree per code worktree, nested at a same-depth path, hidden by the common info/exclude,
# each on its own local branch mock-wt/<ns>/<wt> whose upstream is the remote's mock/<ns> (no data worktree
# ever checks out the shared name), the code worktree LOCKED and the data worktree LOCKED.
# Run against two fetch-refspec styles a bare clone is found in: none (plain clone --bare) and the
# remote-tracking style. Red test 2's three moves are repeated at the end.
set -u
S=${1:?scratch}; rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t GIT_CONFIG_NOSYSTEM=1 HOME="$S/home"; mkdir -p "$HOME"
D=mock/rdata; NS=research
instantiate() { # $1 = code worktree dir, $2 = wt id ; what `data-refs instantiate` would run
  local W=$1 ID=$2 B=mock-wt/$NS/$2
  git -C "$W" fetch -q origin "refs/heads/mock/$NS" || return 1
  git -C "$W" branch -q --no-track "$B" FETCH_HEAD
  git -C "$W" config "branch.$B.remote" origin; git -C "$W" config "branch.$B.merge" "refs/heads/mock/$NS"
  git -C "$W" worktree add -q "$D" "$B"
  grep -qx "/$D/" "$(git -C "$W" rev-parse --git-common-dir)/info/exclude" 2>/dev/null || echo "/$D/" >> "$(git -C "$W" rev-parse --git-common-dir)/info/exclude"
  git -C "$W" worktree lock --reason "holds data worktree $D" "$PWD/$W" 2>/dev/null
  git -C "$W" worktree lock --reason "data worktree of $ID" "$W/$D"
}
for STYLE in none tracking; do
  echo "######## refspec style: $STYLE"; rm -rf p.git w1 w2 w3 origin.git seed
git init -q --bare -b dev origin.git
git clone -q origin.git seed 2>/dev/null; cd seed; echo s > main.rs; git add -A; git commit -qm s; git push -q origin dev
git checkout -q --orphan mock/$NS; git rm -rqf .; printf 'a\nb\n' > NOTES.md; echo 'NOTES.md merge=union' > .gitattributes; git add -A; git commit -qm data; git push -q origin mock/$NS; cd ..
  git clone -q --bare origin.git p.git; [ $STYLE = tracking ] && git -C p.git config remote.origin.fetch '+refs/heads/*:refs/remotes/origin/*' && git -C p.git fetch -q origin
  git -C p.git worktree add -q -b feat1 ../w1 dev; git -C p.git worktree add -q -b feat2 ../w2 dev
  instantiate w1 w1 && instantiate w2 w2; echo "instantiated: $(git -C p.git worktree list | awk '{print $1, $3, $4}' | sed "s|$S/||" | tr '\n' ';')"
  echo "worktreeConfig set: [$(git -C p.git config extensions.worktreeConfig)] core.bare=$(git -C p.git config core.bare)"
  (cd w1/$D; echo one >> NOTES.md; git commit -qam w1; git push -q origin HEAD:refs/heads/mock/$NS 2>&1 | tail -1)
  (cd w2/$D; echo two >> NOTES.md; git commit -qam w2; git fetch -q origin mock/$NS && git merge -q --no-edit --no-overwrite-ignore FETCH_HEAD 2>&1 | tail -1; echo "w2 after pull: $(tr '\n' '|' < NOTES.md)"; git push -q origin HEAD:refs/heads/mock/$NS 2>&1 | tail -1)
  (cd w1/$D; git fetch -q origin mock/$NS && git merge -q --no-edit --no-overwrite-ignore FETCH_HEAD; echo "w1 after pull: $(tr '\n' '|' < NOTES.md)")
  echo "plain fetch in bare: rc=$(git -C p.git fetch -q origin >/dev/null 2>&1; echo $?)  fetch in w1: rc=$(git -C w1 fetch -q origin >/dev/null 2>&1; echo $?)"
  echo "mirror-style fetch '+refs/heads/*:refs/heads/*' from w1: {$(git -C w1 fetch -q origin '+refs/heads/*:refs/heads/*' 2>&1 | head -1)}"
  echo "landing gate in w2: data HEAD ancestor of remote mock/$NS: $(git -C w2/$D fetch -q origin mock/$NS && git -C w2/$D merge-base --is-ancestor HEAD FETCH_HEAD && echo yes || echo no)"
  echo EDIT >> w2/$D/NOTES.md
  echo "worktree remove w2: {$(git -C p.git worktree remove ../w2 2>&1 | head -1)} --force: {$(git -C p.git worktree remove --force ../w2 2>&1 | head -1)} edit=$(grep -c EDIT w2/$D/NOTES.md)"
  echo "worktree prune -v: {$(git -C p.git worktree prune -v 2>&1 | head -1)} gc: rc=$(git -C p.git gc -q --prune=now >/dev/null 2>&1; echo $?) edit=$(grep -c EDIT w2/$D/NOTES.md)"
  echo "red test 2 moves from w1: fetch dev:dev {$(git -C w1 fetch -q origin dev:dev 2>&1 | head -1)} branch -f dev {$(git -C w1 branch -f dev feat1 2>&1|head -1)} checkout dev {$(git -C w1 checkout -q dev 2>&1|head -1)} -> w1 on $(git -C w1 rev-parse --abbrev-ref HEAD)"
  git -C p.git worktree add -q ../w3 -b feat3 dev; echo "a new worktree w3 before instantiate: $D exists=$([ -e w3/$D ] && echo yes || echo no)"
done
