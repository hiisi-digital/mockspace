//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The pre-push check that a trunk carries no flat round file.
//!
//! A round's files sit flat in `design_rounds/` while it is open and move into a directory of their
//! own when it closes. A flat file that reaches the trunk, a topic `close` left behind, puts the
//! trunk in the TOPIC phase, and with it every branch that merges the trunk afterwards: each of
//! them reads an open round it never opened and is refused the next `done` by whatever checks for
//! open rounds. The file is committed by then, which freezes it against the tidy move into its
//! round, so the repair is a commit of its own for every repository the trunk reaches.

/// The trunk branches, as `refs/heads/<name>`.
const TRUNKS: &[&str] = &["dev", "main"];

/// The pre-push body that refuses a push of a trunk whose tree holds a flat round file. Expects
/// `$PREPUSH_STDIN` to hold the captured `<local-ref> <local-sha> <remote-ref> <remote-sha>` lines
/// and `$MOCK_DIR` to name the mock directory.
pub(crate) fn flat_round_prepush_body() -> String {
    let trunks = TRUNKS.join("|refs/heads/");
    format!(
        r##"FLAT_ROUND_FILES=""
while IFS=' ' read -r _fr_local _fr_sha fr_remote_ref _fr_remote_sha; do
    case "$fr_remote_ref" in
        refs/heads/{trunks}) ;;
        *) continue ;;
    esac
    if [ -z "$_fr_sha" ] || [ "$_fr_sha" = "0000000000000000000000000000000000000000" ]; then
        continue
    fi
    # Files only: a closed round is a directory, which is where the files belong.
    FR_HERE=$(git ls-tree "$_fr_sha" "$MOCK_DIR/design_rounds/" 2>/dev/null \
        | awk -F'\t' '$1 ~ / blob / {{print $2}}' | grep -v '/README\.md$' || true)
    if [ -n "$FR_HERE" ]; then
        FLAT_ROUND_FILES="$FLAT_ROUND_FILES$FR_HERE"$'\n'
    fi
done <<< "$PREPUSH_STDIN"
if [ -n "$FLAT_ROUND_FILES" ]; then
    echo "" >&2
    echo "BLOCKED: this push would put flat round files on the trunk:" >&2
    printf '%s' "$FLAT_ROUND_FILES" | sort -u | sed 's/^/  /' >&2
    echo "  a closed round's files live in its directory. \`cargo mock close --take <file>\` archives one" >&2
    echo "  with its round; a topic parked for a later round belongs on that round's branch, not here." >&2
    exit 1
fi
"##
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::flat_round_prepush_body;

    static SEQ: AtomicU32 = AtomicU32::new(0);

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A repo whose single commit holds `files` under `mock/design_rounds/`.
    fn repo_with(files: &[&str]) -> (PathBuf, String) {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mock_flat_round_{}_{n}", std::process::id()));
        std::fs::create_dir_all(dir.join("mock/design_rounds")).unwrap();
        git(&dir, &["init", "-q", "-b", "work"]);
        std::fs::write(dir.join("mock/other.md"), "x\n").unwrap();
        for f in files {
            let path = dir.join("mock/design_rounds").join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x\n").unwrap();
        }
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "chore: fixture"]);
        let sha = git(&dir, &["rev-parse", "HEAD"]);
        (dir, sha)
    }

    /// Run the body against one pushed ref and return the exit code.
    fn push(dir: &Path, sha: &str, remote_ref: &str) -> i32 {
        let script = format!(
            "#!/usr/bin/env bash\nset -e\nMOCK_DIR=mock\nPREPUSH_STDIN=$(cat)\n{}",
            flat_round_prepush_body()
        );
        let hook = dir.join("..").join(format!("hook_{}.sh", SEQ.fetch_add(1, Ordering::Relaxed)));
        crate::test_exec::write_executable(&hook, &script);
        let mut child = Command::new(&hook)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        use std::io::Write;
        let line = format!("refs/heads/work {sha} {remote_ref} {}\n", "0".repeat(40));
        let _ = child.stdin.take().unwrap().write_all(line.as_bytes());
        child.wait().unwrap().code().unwrap_or(-1)
    }

    const TOPIC: &str = "202610301536_topic.parallel.md";

    #[test]
    fn a_flat_file_pushed_to_a_trunk_is_refused() {
        let (dir, sha) = repo_with(&[TOPIC]);
        assert_eq!(push(&dir, &sha, "refs/heads/dev"), 1);
        assert_eq!(push(&dir, &sha, "refs/heads/main"), 1);
    }

    #[test]
    fn the_same_tree_pushed_to_a_feature_branch_passes() {
        let (dir, sha) = repo_with(&[TOPIC]);
        assert_eq!(push(&dir, &sha, "refs/heads/work/a-feature"), 0);
    }

    #[test]
    fn a_trunk_holding_only_a_readme_and_closed_round_directories_passes() {
        let (dir, sha) = repo_with(&["README.md", "202610301536/202610301537_changelist.src.lock.md"]);
        assert_eq!(push(&dir, &sha, "refs/heads/dev"), 0);
    }

    #[test]
    fn deleting_a_trunk_ref_is_not_judged_by_a_tree() {
        let (dir, _sha) = repo_with(&[TOPIC]);
        assert_eq!(push(&dir, &"0".repeat(40), "refs/heads/dev"), 0);
    }
}
