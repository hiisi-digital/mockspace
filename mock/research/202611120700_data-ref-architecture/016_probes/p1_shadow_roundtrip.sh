#!/bin/bash
# 016/p1: the two-way shadow tree, round trip by git's own three-way merge (git merge-file), base = the last generated result.
# Objects: T template, B baseline (last generated result, stored), O tracked output (may carry an upstream edit), Gen deterministic.
#   carry-back:   T' = merge3(ours=T, base=B, theirs=O)        -- the edit B->O is applied to the template
#   regenerate:   R  = merge3(ours=Gen(T), base=B, theirs=O)   -- the template's changes are applied to the edited output
# Each scenario states whether it is automatic and checks the round trip Gen(T') == O.
set -u
SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/p016-1-XXXXXX") || exit 1; cd "$SCRATCH" || exit 1
case $PWD in "$SCRATCH"*) ;; *) echo "refusing to run outside my own scratch"; exit 1;; esac
trap 'cd /; rm -rf "$SCRATCH"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null
# the generator: {{NAME}} {{VERSION}} substituted anywhere in a line; a line holding only {{LIST}} expands to one bullet per item
NAME=widget; VERSION=1.2.3; ITEMS="alpha beta gamma"
gen() { while IFS= read -r line; do case "$line" in '{{LIST}}') for i in $ITEMS; do echo "- item $i"; done;; *) echo "$line" | sed "s/{{NAME}}/$NAME/g; s/{{VERSION}}/$VERSION/g";; esac; done < "$1"; }
cat > T <<'T'
# {{NAME}}

Version {{VERSION}} of the {{NAME}} crate.

This paragraph is literal text that a contributor may improve.

## Items
{{LIST}}

## Notes

Another literal paragraph, one of two.
Second line of the same paragraph.

Closing line.
T
gen T > B; cp B O
r() { printf '%-64s %s\n' "$1" "$2"; }
carry() { # $1 label; uses T B O; prints automatic/conflict and the round-trip check
  cp T Tn; if git merge-file -p T B O > Tn 2>/dev/null; then
    gen Tn > G; if cmp -s G O; then r "$1" "AUTOMATIC: carried into template, Gen(T') == O"; else r "$1" "merge clean but Gen(T') != O  (round trip fails: not automatic)"; fi
  else r "$1" "CONFLICT: not automatic ($(git merge-file -p T B O 2>/dev/null | grep -c '^<<<<<<<') conflicting hunk(s))"; fi; }
edit() { cp B O; "$@"; }
echo "== carry-back: an edit to the generated output arrives (a pull request), template unchanged"
edit sed -i 's/^This paragraph is literal text.*/This paragraph was improved by a PR./' O; carry "literal line edited"
edit sed -i 's/^Closing line\./Closing line, edited.\nAnd one more line appended./' O; carry "literal line edited and a line appended at the end"
edit sed -i '/^Another literal paragraph/,/^Second line/d' O; carry "a literal two-line paragraph deleted"
edit sed -i 's/^Version 1.2.3 of the widget crate\./Version 1.2.3 of the widget crate, hand-edited./' O; carry "edit ON a line the template computes ({{VERSION}} {{NAME}})"
edit sed -i 's/^- item beta/- item beta (edited)/' O; carry "edit INSIDE a loop-expanded region"
edit sed -i 's/^- item gamma/&\n- item delta (added by hand)/' O; carry "line added at the end of a loop-expanded region"
edit sed -i 's/^This paragraph is literal text.*/&\nAn inserted sentence.\n/' O; carry "insertion directly after a literal line, two lines above a computed one"
edit sed -i 's/^$/&/; 4s/.*/Edit directly beneath the computed line./' O; carry "edit on the line DIRECTLY after a computed line (touching hunks)"
edit sed -i '1s/.*/# widget (renamed in title)/' O; carry "edit to the title, a computed line"
edit bash -c "printf '\n\nTrailing addition after the last literal line.\n' >> O"; carry "text appended after the last line"
echo "== two edits in different places (two PRs merged by git first), then carried back together"
cp B O; sed -i 's/^This paragraph is literal text.*/First PR edit./' O; sed -i 's/^Closing line\./Second PR edit./' O; carry "two independent literal edits"
echo "== regenerate with an edit present, template changed elsewhere (VERSION bump, a new literal line in T)"
cp B O; sed -i 's/^This paragraph is literal text.*/Edited in a PR./' O
VERSION=1.3.0; gen T > Gcur
git merge-file -p Gcur B O > R 2>/dev/null; rc=$?; r "regenerate: output edit on a literal line, version bumped" "merge3 rc=$rc; result has the edit: $(grep -c 'Edited in a PR' R); has new version: $(grep -c '1.3.0' R)"
# idempotence: regenerate again with B := Gcur and O := R
cp Gcur B2; git merge-file -p Gcur B2 R > R2 2>/dev/null; r "regenerate a second time (B=Gcur, O=R, same template)" "R2 == R: $(cmp -s R R2 && echo yes || echo NO); edit still present: $(grep -c 'Edited in a PR' R2)"
echo "== regenerate when the edit and the template change overlap (both touch the title/version line)"
VERSION=1.2.3; cp B O; sed -i 's/^Version 1.2.3 of the widget crate\./Version 1.2.3, per the PR./' O; VERSION=1.3.0; gen T > Gcur
git merge-file -p Gcur B O > R 2>/dev/null; r "overlapping edit and template change" "merge3 rc=$? (non-zero = conflict markers: not automatic)"
