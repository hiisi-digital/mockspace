#!/bin/bash
# Spike k1: the carry-back base. Seat 002 (016 1.2) stores ONE baseline B per output in the shadow tree (the last
# generated result the engine wrote). Seat 001 (015 3.2) takes G at the pull request's merge-base, verified by
# re-rendering. Candidate settlement (007): the shadow tree stores the SET of blob ids the engine has ever
# produced for each output; an output is "edited" iff its blob is not in the set; the base is G at the newest
# commit on HEAD's first-parent chain whose G is in the set. Toy renderer (sed of one placeholder); git's side only.
# Worlds: trunk with data v=1; a feature branch regenerated from newer data v=2 (no human edit); a pull request
# that edits a literal line, merged into trunk after trunk itself regenerated a template change.
set -u
exec </dev/null
S=${1:?scratch}; case "$S" in */p17/*) ;; *) echo "refusing scratch $S"; exit 2;; esac
rm -rf "$S"; mkdir -p "$S"; cd "$S"
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t HOME="$S/home"; mkdir -p "$HOME"
gen() { sed "s/{{v}}/$2/" "$1"; }                     # Gen(T, data)
SET="$S/produced.set"; : > "$SET"; B1=""                 # 007's set; 002's single baseline lives in B1
write_gen() { gen "$1" "$2" > G.md; h=$(git hash-object G.md); echo "$h" >> "$SET"; cp G.md "$S/B1"; }   # engine writes G
printf '# Doc version {{v}}\n\nIntro line.\n\nClosing paragraph.\n' > "$S/T"
git init -q -b trunk r; cd r
write_gen "$S/T" 1; git add G.md; git commit -qm "gen v1"
git checkout -q -b pr; sed -i 's/Intro line./Intro line, clarified by a PR./' G.md; git commit -qam "PR: edit literal line"
git checkout -q -b feature trunk; write_gen "$S/T" 2; git commit -qam "feature: regenerate from data v2"
git checkout -q trunk; sed -i 's/Closing paragraph./Closing paragraph, upstream./' "$S/T"; write_gen "$S/T" 1; git commit -qam "trunk: template change regenerated"
git merge -q --no-edit pr 2>&1 | head -1; echo "trunk after merging the PR (plain git merge): $(tr '\n' '|' < G.md)"
classify() { # $1 branch
  local O; O=$(git show "$1:G.md" | git hash-object --stdin)
  local single=edited; cmp -s <(git show "$1:G.md") "$S/B1" && single=not-edited
  local setc=edited; grep -qx "$O" "$SET" && setc=not-edited
  echo "$1: 002 single-B says $single; 007 produced-set says $setc"
}
echo "== classification (the last engine write anywhere was trunk's v1 regeneration)"
classify trunk; classify feature; git checkout -q feature; write_gen "$S/T" 2; git commit -qam "feature: regenerate again"; git checkout -q trunk
echo "-- after the feature branch regenerates again (now the last engine write is v2):"; classify trunk; classify feature
echo "== carry back on trunk with the 007 base (newest first-parent commit whose G is in the set)"
base=""; for c in $(git rev-list --first-parent trunk -- G.md); do h=$(git rev-parse "$c:G.md"); grep -qx "$h" "$SET" && { base=$c; break; }; done
git show "$base:G.md" > "$S/base"; git show trunk:G.md > "$S/O"; cp "$S/T" "$S/Tn"
git merge-file -q "$S/Tn" "$S/base" "$S/O"; rc=$?
rt=no; [ "$(gen "$S/Tn" 1)" = "$(cat "$S/O")" ] && rt=yes
echo "base commit: $(git log -1 --format=%s $base); merge-file rc=$rc; round trip Gen(T',v1)==O: $rt"; echo "T' = $(tr '\n' '|' < "$S/Tn")"
echo "== the same with 002's single B after the feature branch's v2 write"
cp "$S/T" "$S/Tm"; git merge-file -q "$S/Tm" "$S/B1" "$S/O"; rc=$?; rt=no; [ "$(gen "$S/Tm" 1)" = "$(cat "$S/O")" ] && rt=yes
echo "merge-file rc=$rc; round trip: $rt; T' = $(tr '\n' '|' < "$S/Tm")"
