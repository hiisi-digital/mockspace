#!/bin/bash
# 016/p4: what git's own merge does with a generated output when (a) two branches regenerate the same template change,
# (b) a pull request edits a literal line while the other side regenerates, (c) a pull request edits a COMPUTED line while the other side regenerates.
# No merge driver is configured: this is the baseline the existing `mockspace-generated` driver (src/bootstrap/driver_script.rs) departs from.
set -u
SCRATCH=$(mktemp -d "${TMPDIR:-/tmp}/p016-4-XXXXXX") || exit 1; cd "$SCRATCH" || exit 1
case $PWD in "$SCRATCH"*) ;; *) echo "refusing to run outside my own scratch"; exit 1;; esac
trap 'cd /; rm -rf "$SCRATCH"' EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=a GIT_AUTHOR_EMAIL=a@a GIT_COMMITTER_NAME=a GIT_COMMITTER_EMAIL=a@a
base_out() { printf '# widget\n\nVersion 1.2.3 of the widget crate.\n\nA literal paragraph.\n\nClosing line.\n'; }
mk() { rm -rf r; git init -q -b dev r; cd r; base_out > README.md; git add -A; git commit -qm base; }
echo "== (a) two branches both regenerate the same version bump"
mk; git switch -q -c x; sed -i 's/1.2.3/1.3.0/' README.md; git commit -qam regen-x; git switch -q dev; git switch -q -c y; sed -i 's/1.2.3/1.3.0/' README.md; git commit -qam regen-y; git merge -q --no-edit x >/dev/null 2>&1; echo "merge rc=$? ; conflict markers: $(grep -c '<<<<<<<' README.md)"; cd ..
echo "== (b) PR edits a literal line; dev regenerates a version bump"
mk; git switch -q -c pr; sed -i 's/^A literal paragraph\./A literal paragraph, improved./' README.md; git commit -qam pr; git switch -q dev; sed -i 's/1.2.3/1.3.0/' README.md; git commit -qam regen; git merge -q --no-edit pr >/dev/null 2>&1; echo "merge rc=$? ; has PR edit: $(grep -c improved README.md); has bump: $(grep -c 1.3.0 README.md)"; cd ..
echo "== (c) PR edits the COMPUTED line; dev regenerates the same line"
mk; git switch -q -c pr; sed -i 's/^Version 1.2.3 of the widget crate\./Version 1.2.3 of the widget crate, per the PR./' README.md; git commit -qam pr; git switch -q dev; sed -i 's/1.2.3/1.3.0/' README.md; git commit -qam regen; git merge -q --no-edit pr >/dev/null 2>&1; echo "merge rc=$? ; conflict markers: $(grep -c '<<<<<<<' README.md) ; PR text visible in the working file: $(grep -c 'per the PR' README.md)"; git merge --abort 2>/dev/null; cd ..
