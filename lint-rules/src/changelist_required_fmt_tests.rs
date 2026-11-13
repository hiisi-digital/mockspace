//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The fmt-only exemption of `changelist-required`, judged on what a commit carries.

#[cfg(test)]
mod fmt_only_exemption {
    use std::process::Command;

    use super::super::*;

    /// A mock dir in CLOSED phase (both changelists locked) with one
    /// committed, deliberately unformatted source file.
    fn frozen_repo(name: &str, committed: &str) -> std::path::PathBuf {
        // A process-wide counter, not a clock. SystemTime here has microsecond
        // resolution at best (twenty back-to-back samples yield four distinct
        // values on this host), and `create_dir_all` succeeds silently on an
        // existing directory, so two tests starting in the same tick shared a
        // scratch directory and overwrote each other's fixtures.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "clr-fmt-{}-{}-{}",
            name,
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/x/src")).unwrap();
        std::fs::create_dir_all(root.join("design_rounds")).unwrap();
        std::fs::write(
            root.join("design_rounds/202601010000_changelist.doc.lock.md"),
            "locked\n",
        )
        .unwrap();
        std::fs::write(
            root.join("design_rounds/202601010001_changelist.src.lock.md"),
            "locked\n",
        )
        .unwrap();
        std::fs::write(root.join("crates/x/src/lib.rs"), committed).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
            vec!["add", "-A"],
            vec!["commit", "-q", "-m", "seed", "--no-gpg-sign"],
        ] {
            Command::new("git")
                .args(&args)
                .current_dir(&root)
                .output()
                .unwrap();
        }
        root
    }

    fn blocked(root: &std::path::Path) -> Vec<LintError> {
        let crates = std::collections::BTreeSet::new();
        let src_dirs = [root.join("crates")];
        let ctx = RepoContext {
            mock_dir:    root,
            repo_root:   root,
            all_crates:  &crates,
            src_dirs:    &src_dirs,
            invocation:  None,
            canon_paths: &[],
            open_panels: &[],
            ref_files: &crate::src_layout::NO_REF_FILES,
            registry:    &Default::default(),
            verdicts:    None,
        };
        ChangelistRequired.check_repo(&ctx)
    }

    const UGLY: &str = "pub fn a(  x:u8 )->u8{x+1}\n";

    fn formatted(root: &std::path::Path, src: &str) -> Option<String> {
        use std::io::Write;
        let mut c = Command::new("rustfmt")
            .args(["--emit=stdout", "--quiet"])
            .current_dir(root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        c.stdin.as_mut()?.write_all(src.as_bytes()).ok()?;
        let o = c.wait_with_output().ok()?;
        o.status
            .success()
            .then(|| String::from_utf8_lossy(&o.stdout).to_string())
    }

    #[test]
    fn a_fmt_only_change_passes_the_frozen_phase_and_a_smuggled_edit_does_not() {
        let root = frozen_repo("both", UGLY);
        let fmt = formatted(&root, UGLY).unwrap_or_else(|| {
            let _ = std::fs::remove_dir_all(&root);
            panic!("rustfmt is not on PATH; the pinned toolchain provides it")
        });
        assert_ne!(fmt, UGLY, "the fixture must actually be reformatted");

        // rustfmt's own output, in CLOSED phase: permitted.
        std::fs::write(root.join("crates/x/src/lib.rs"), &fmt).unwrap();
        assert!(
            blocked(&root).is_empty(),
            "a change that is exactly rustfmt's output carries no edit"
        );

        // The same formatting with one semantic character changed: refused.
        std::fs::write(
            root.join("crates/x/src/lib.rs"),
            fmt.replace("x + 1", "x + 2"),
        )
        .unwrap();
        let errs = blocked(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            !errs.is_empty(),
            "an edit smuggled alongside formatting must still be refused in CLOSED"
        );
    }

    #[test]
    fn a_plain_source_edit_is_still_refused_in_a_frozen_phase() {
        let root = frozen_repo("plain", "pub fn a(x: u8) -> u8 {\n    x + 1\n}\n");
        std::fs::write(
            root.join("crates/x/src/lib.rs"),
            "pub fn a(x: u8) -> u8 {\n    x + 99\n}\n",
        )
        .unwrap();
        let errs = blocked(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(!errs.is_empty(), "the gate must still be a gate");
    }
}

#[cfg(test)]
mod fmt_only_judges_the_committed_content {
    use std::process::Command;

    use super::super::*;

    /// A mock dir in CLOSED phase with one committed, misformatted source file.
    fn frozen(name: &str, committed: &str) -> std::path::PathBuf {
        // A process-wide counter, not a clock. SystemTime here has microsecond
        // resolution at best (twenty back-to-back samples yield four distinct
        // values on this host), and `create_dir_all` succeeds silently on an
        // existing directory, so two tests starting in the same tick shared a
        // scratch directory and overwrote each other's fixtures.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "clr-idx-{}-{}-{}",
            name,
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/x/src")).unwrap();
        std::fs::create_dir_all(root.join("design_rounds")).unwrap();
        for cl in ["202601010000_changelist.doc.lock.md", "202601010001_changelist.src.lock.md"] {
            std::fs::write(root.join("design_rounds").join(cl), "locked\n").unwrap();
        }
        std::fs::write(root.join("crates/x/src/lib.rs"), committed).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
            vec!["add", "-A"],
            vec!["commit", "-q", "-m", "seed", "--no-gpg-sign"],
        ] {
            Command::new("git")
                .args(&args)
                .current_dir(&root)
                .output()
                .unwrap();
        }
        root
    }

    fn gate(root: &std::path::Path) -> Vec<LintError> {
        let crates = std::collections::BTreeSet::new();
        let src_dirs = [root.join("crates")];
        ChangelistRequired.check_repo(&RepoContext {
            mock_dir:    root,
            repo_root:   root,
            all_crates:  &crates,
            src_dirs:    &src_dirs,
            invocation:  None,
            canon_paths: &[],
            open_panels: &[],
            ref_files: &crate::src_layout::NO_REF_FILES,
            registry:    &Default::default(),
            verdicts:    None,
        })
    }

    /// The bypass: stage a semantic edit, then leave the worktree holding
    /// exactly `rustfmt(HEAD)`. A predicate that reads the worktree calls the
    /// change fmt-only and drops it, while the commit carries the edit.
    ///
    /// Reachable without adversarial intent: the pre-commit auto-fix runs
    /// before the lints and deliberately does not re-stage a partially staged
    /// file, so it leaves the worktree formatted while the index keeps what
    /// was staged.
    #[test]
    fn a_semantic_edit_in_the_index_is_refused_even_when_the_worktree_is_formatted() {
        const UGLY: &str = "pub fn a(  x:u8 )->u8{x+1}\n";
        let root = frozen("bypass", UGLY);

        let mut c = Command::new("rustfmt")
            .args(["--emit=stdout", "--quiet"])
            .current_dir(&root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()
            .and_then(|mut ch| {
                use std::io::Write;
                ch.stdin.as_mut()?.write_all(UGLY.as_bytes()).ok()?;
                Some(ch)
            })
            .map(|ch| ch.wait_with_output().unwrap());
        let out = c.take().filter(|o| o.status.success()).unwrap_or_else(|| {
            let _ = std::fs::remove_dir_all(&root);
            panic!("rustfmt is not on PATH; the pinned toolchain provides it")
        });
        let fmt = String::from_utf8_lossy(&out.stdout).to_string();

        // Index: a semantic edit. Worktree: exactly rustfmt(HEAD).
        let file = root.join("crates/x/src/lib.rs");
        std::fs::write(&file, "pub fn a(x: u8) -> u8 {\n    x + 999\n}\n").unwrap();
        Command::new("git")
            .args(["add", "crates/x/src/lib.rs"])
            .current_dir(&root)
            .output()
            .unwrap();
        std::fs::write(&file, &fmt).unwrap();

        let errs = gate(&root);
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            !errs.is_empty(),
            "the commit carries a semantic edit in the index; a worktree-only \
             check calls it fmt-only and lets it through a frozen phase"
        );
    }
}
