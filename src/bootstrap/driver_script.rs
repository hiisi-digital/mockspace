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
///   image whose diagram of the same stem at `HEAD` carries one. An image can
///   only be regenerated where `dot` is, so for `.png` and `.svg` it is on `PATH`
///   or ours is not kept;
/// - the operation is one a regeneration follows. A `git merge` (including
///   `--squash` and `--no-commit`, which `pre-commit` follows up) is told apart
///   by `GITHEAD_<sha>` in the environment, which git sets for the heads a merge
///   brings and for no other command that calls a driver. A cherry-pick, a
///   revert and a stash application set none, and none has written its state
///   file yet when the driver runs, so a `CHERRY_PICK_HEAD` check alone would
///   never fire. A rebase is told apart by its state (`rebase-merge`, or
///   `rebase-apply/rebasing`, since `git am` keeps `rebase-apply/applying`
///   there and is not followed up);
/// - the hook that does the regeneration is installed where git will run it and
///   would hand over to a generated hook in this working tree. The hook is
///   asked, by running it with `MOCKSPACE_PROBE=1`: it answers from the same
///   discovery it uses to delegate, so the mock directory is whichever one this
///   working tree has, which differs across branches and worktrees and which no
///   string kept per clone can name. `post-merge` follows a merge and
///   `post-rewrite` a rebase. `git rev-parse --git-path hooks/<name>` honours
///   `core.hooksPath`, so a clone whose hooks are replaced, or a client that
///   skips them, is seen.
///
/// Having kept ours it writes down that it did, in `mockspace-kept-ours` under
/// the git directory (see `mockspace_manifest::kept_ours`), because nothing else
/// checks afterwards that the promised regeneration happened.
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
    # a lockfile is a function of the manifests: carrying no header is no reason to merge it as text
    case ${{path##*/}} in
        Cargo.lock) return 0 ;;
    esac
    case $path in
        *.png|*.svg) command -v dot >/dev/null 2>&1 || return 1 ;;
    esac
    case $path in
        *.png) git show "HEAD:${{path%.png}}.dot" 2>/dev/null | carries_header ;;
        *) carries_header < "$ours" ;;
    esac
}}

# the hook that regenerates is installed where git will look for it, which
# core.hooksPath moves, and says it would hand over to a generated hook in this
# working tree
installed() {{
    hook=$(git rev-parse --git-path "hooks/$1") || return 1
    [ -x "$hook" ] && grep -qF -- '{MANAGED_MARKER}' "$hook" || return 1
    MOCKSPACE_PROBE=1 "$hook" </dev/null >/dev/null 2>&1
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
    if [ "${{path##*/}}" = Cargo.lock ]; then
        # a lock also records resolution choices (a bump, a pinned revision) that no manifest
        # holds, so the two sides are text-merged first: a clean merge carries both sides' choices
        # and a conflicting hunk keeps ours, to be settled from the merged manifests afterwards
        try="$ours.mockspace-try"
        cp "$ours" "$try"
        git merge-file -q --ours -L ours -L base -L theirs "$try" "$base" "$theirs" && cp "$try" "$ours"
        rm -f "$try"
    fi
    kept=$(git rev-parse --git-path mockspace-kept-ours)
    [ -f "$kept" ] || git rev-parse HEAD > "$kept"
    echo "$path" >> "$kept"
    exit 0
fi
exec git merge-file --marker-size="$size" -L ours -L base -L theirs "$ours" "$base" "$theirs"
"##
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;

    /// A repository with a commit, a managed `post-merge` that answers the probe
    /// with `probe_exit`, a driver script, and a directory of tools standing in
    /// for `PATH`: git and the coreutils the script uses, plus `dot` if asked.
    struct Rig {
        dir: tempfile::TempDir,
    }

    fn executable(path: &Path, body: &str) {
        crate::test_exec::write_executable(path, body);
    }

    impl Rig {
        fn new(with_dot: bool, probe_exit: i32) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            let bin = root.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            for tool in ["git", "head", "grep", "env", "cat", "rm", "sh", "tail", "cp"] {
                let found = Command::new("sh")
                    .args(["-c", &format!("command -v {tool}")])
                    .output()
                    .unwrap();
                let real = String::from_utf8_lossy(&found.stdout).trim().to_string();
                std::os::unix::fs::symlink(real, bin.join(tool)).unwrap();
            }
            if with_dot {
                executable(&bin.join("dot"), "#!/bin/sh\nexit 0\n");
            }
            let repo = root.join("repo");
            std::fs::create_dir_all(&repo).unwrap();
            let rig = Rig {
                dir,
            };
            rig.git(&["init", "-q", "-b", "dev"]);
            rig.git(&[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@e.c",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "base",
            ]);
            executable(
                &repo.join(".git/hooks/post-merge"),
                &format!(
                    "#!/bin/sh\n{MANAGED_MARKER}\n[ -n \"$MOCKSPACE_PROBE\" ] && exit {probe_exit}\nexit 9\n"
                ),
            );
            std::fs::write(root.join("script"), driver_script()).unwrap();
            rig
        }

        fn git(&self, args: &[&str]) {
            let out = Command::new("git")
                .args(args)
                .current_dir(self.dir.path().join("repo"))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        /// Run the driver on a generated svg whose two sides differ, as a merge.
        fn run(&self, path: &str) -> (bool, String) {
            let root = self.dir.path();
            let header = "<!--\n  AUTO-GENERATED: DO NOT EDIT DIRECTLY\n-->\nline\n";
            std::fs::write(root.join("O"), header).unwrap();
            std::fs::write(root.join("A"), format!("{header}ours\n")).unwrap();
            std::fs::write(root.join("B"), format!("{header}theirs\n")).unwrap();
            let out = Command::new(root.join("bin/sh"))
                .arg(root.join("script"))
                .args([root.join("O"), root.join("A"), root.join("B")])
                .args([path, "7"])
                .current_dir(root.join("repo"))
                .env("PATH", root.join("bin"))
                .env("GITHEAD_0123456789", "a")
                .output()
                .unwrap();
            (
                out.status.success(),
                std::fs::read_to_string(root.join("A")).unwrap(),
            )
        }

        fn kept(&self) -> Option<String> {
            std::fs::read_to_string(self.dir.path().join("repo/.git/mockspace-kept-ours")).ok()
        }
    }

    impl Rig {
        /// Run the driver on a `Cargo.lock` with the three sides as given, as a merge.
        fn run_lock(&self, base: &str, ours: &str, theirs: &str) -> (bool, String) {
            let root = self.dir.path();
            std::fs::write(root.join("O"), base).unwrap();
            std::fs::write(root.join("A"), ours).unwrap();
            std::fs::write(root.join("B"), theirs).unwrap();
            let out = Command::new(root.join("bin/sh"))
                .arg(root.join("script"))
                .args([root.join("O"), root.join("A"), root.join("B")])
                .args(["Cargo.lock", "7"])
                .current_dir(root.join("repo"))
                .env("PATH", root.join("bin"))
                .env("GITHEAD_0123456789", "a")
                .output()
                .unwrap();
            (out.status.success(), std::fs::read_to_string(root.join("A")).unwrap())
        }
    }

    const LOCK_BASE: &str = "version = 4\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"base\"\nversion = \"0.1.0\"\n";

    #[test]
    fn a_lock_both_sides_changed_apart_is_text_merged_so_a_bump_survives() {
        // theirs bumps a dependency, ours adds a member far from it: git merges that without help,
        // and keeping ours alone would drop the bump
        let rig = Rig::new(false, 0);
        let theirs = LOCK_BASE.replace("1.0.0", "1.0.1");
        let ours = format!("{LOCK_BASE}\n[[package]]\nname = \"m1\"\nversion = \"0.1.0\"\n");
        let (kept, merged) = rig.run_lock(LOCK_BASE, &ours, &theirs);
        assert!(kept);
        assert!(merged.contains("1.0.1"), "theirs' bump is in the lock: {merged}");
        assert!(merged.contains("name = \"m1\""), "and so is our member: {merged}");
        assert!(rig.kept().is_some_and(|r| r.lines().any(|l| l == "Cargo.lock")), "settled afterwards");
    }

    #[test]
    fn a_lock_both_sides_added_to_the_same_spot_keeps_ours_for_the_settling_to_finish() {
        let rig = Rig::new(false, 0);
        let add = |n: &str| format!("{LOCK_BASE}\n[[package]]\nname = \"{n}\"\nversion = \"0.1.0\"\n");
        let (kept, merged) = rig.run_lock(LOCK_BASE, &add("m1"), &add("m2"));
        assert!(kept);
        assert!(merged.contains("name = \"m1\"") && !merged.contains("<<<<<<<"), "{merged}");
        assert!(!merged.contains("name = \"m2\""), "the settling adds it from the manifests: {merged}");
    }

    #[test]
    fn a_bump_far_from_a_conflicting_hunk_survives_the_conflict() {
        // theirs bumps a dependency and adds m2 at the end; ours adds m1 at the end: the member hunk
        // conflicts, the bump does not, and ours must win only the hunk that conflicts
        let rig = Rig::new(false, 0);
        let add = |s: &str, n: &str| format!("{s}\n[[package]]\nname = \"{n}\"\nversion = \"0.1.0\"\n");
        let theirs = add(&LOCK_BASE.replace("1.0.0", "1.0.1"), "m2");
        let (kept, merged) = rig.run_lock(LOCK_BASE, &add(LOCK_BASE, "m1"), &theirs);
        assert!(kept);
        assert!(merged.contains("1.0.1"), "theirs' bump is not lost to the conflict: {merged}");
        assert!(merged.contains("name = \"m1\""), "{merged}");
        assert!(!merged.contains("<<<<<<<"), "{merged}");
    }

    #[test]
    fn ours_is_kept_and_written_down_when_a_hook_would_regenerate() {
        let rig = Rig::new(true, 0);
        let (kept, content) = rig.run("docs/X.svg");
        assert!(kept);
        assert!(content.ends_with("ours\n"), "{content}");
        let record = rig.kept().expect("the driver wrote down that it kept ours");
        let mut lines = record.lines();
        assert_eq!(lines.next().map(str::len), Some(40), "HEAD first: {record}");
        assert_eq!(lines.next(), Some("docs/X.svg"), "{record}");
    }

    #[test]
    fn an_image_is_kept_only_where_dot_can_render_it_again() {
        // the control is the test above: the same file, with `dot` on PATH
        let rig = Rig::new(false, 0);
        let (kept, content) = rig.run("docs/X.svg");
        assert!(
            !kept,
            "no `dot`, so nothing would regenerate the image: {content}"
        );
        assert!(content.contains("<<<<<<<"), "{content}");
        assert!(rig.kept().is_none(), "and nothing was promised");
    }

    #[test]
    fn a_hook_that_would_not_hand_over_here_keeps_nothing() {
        // the probe exits 1, as the durable hook does with no generated hook here
        let rig = Rig::new(true, 1);
        let (kept, content) = rig.run("docs/X.svg");
        assert!(!kept, "{content}");
        assert!(content.contains("<<<<<<<"), "{content}");
        assert!(rig.kept().is_none());
    }
}
