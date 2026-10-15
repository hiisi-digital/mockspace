//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The text of the `mockspace-generated` merge driver.

use mockspace_manifest::gate::MANAGED_MARKER;

use crate::render_design::{GENERATED_MARKER, MARKER_WITHIN_LINES};

/// The driver, a shell script, because git runs it as a command and nothing
/// else about it needs a build. Called as `<script> %O %A %B %P %L`: base, ours,
/// theirs, path, conflict marker size. Git 2.43 expands no label placeholder
/// (`%S`, `%X`, `%Y` come through literally), so the markers are labelled
/// `ours`, `base` and `theirs` rather than with the names git passes itself.
///
/// Keeps ours and reports no conflict only when all three hold:
///
/// - the file is one mockspace generates, read from its header, or a rendered
///   image whose diagram of the same stem at `HEAD` carries one;
/// - the operation is one a regeneration follows. A `git merge` (including
///   `--squash` and `--no-commit`, which `pre-commit` follows up) is told apart
///   by `GITHEAD_<sha>` in the environment, which git sets for the heads a merge
///   brings and for no other command that calls a driver. A cherry-pick, a
///   revert and a stash application set none, and none has written its state
///   file yet when the driver runs, so a `CHERRY_PICK_HEAD` check alone would
///   never fire. A rebase is told apart by its state (`rebase-merge`, or
///   `rebase-apply/rebasing`, since `git am` keeps `rebase-apply/applying`
///   there and is not followed up);
/// - the hook that does the regeneration is installed where git will run it.
///   `git rev-parse --git-path hooks/<name>` honours `core.hooksPath`, so a
///   clone whose hooks are replaced, or a client that skips them, is seen.
///   `post-merge` follows a merge and `post-rewrite` a rebase.
///
/// Anything else is `git merge-file`, which writes conflict markers into ours and
/// exits non-zero on a conflict, as git's own text merge does.
pub(crate) fn driver_script() -> String {
    format!(
        r##"#!/bin/sh
{MANAGED_MARKER}
# Merge driver for generated documents, rewritten by `mock`. Git calls it as
# <this> %O %A %B %P %L: base, ours, theirs, path, conflict marker size.
base=$1
ours=$2
theirs=$3
path=$4
case $5 in
    ''|*[!0-9]*) size=7 ;;
    *) size=$5 ;;
esac

carries_header() {{
    head -n {MARKER_WITHIN_LINES} | grep -aqF -- '{GENERATED_MARKER}'
}}

generated() {{
    case $path in
        *.png) git show "HEAD:${{path%.png}}.dot" 2>/dev/null | carries_header ;;
        *) carries_header < "$ours" ;;
    esac
}}

# one of our hooks is where git will look for it, which core.hooksPath moves
installed() {{
    hook=$(git rev-parse --git-path "hooks/$1") || return 1
    [ -x "$hook" ] && grep -qF -- '{MANAGED_MARKER}' "$hook"
}}

exists() {{
    [ -e "$(git rev-parse --git-path "$1")" ]
}}

rebasing() {{
    exists rebase-merge || exists rebase-apply/rebasing
}}

# whether a regeneration of the resulting tree is certain to follow
followed_up() {{
    if rebasing; then
        installed post-rewrite
        return
    fi
    for state in sequencer CHERRY_PICK_HEAD REVERT_HEAD; do
        exists "$state" && return 1
    done
    env | grep -q '^GITHEAD_' || return 1
    installed post-merge
}}

if generated && followed_up; then
    exit 0
fi
exec git merge-file --marker-size="$size" -L ours -L base -L theirs "$ours" "$base" "$theirs"
"##
    )
}
