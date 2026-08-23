//! Repo lint: source code changes require the IMPL phase.
//!
//! A `*.rs` file under one of the project's declared source directories may
//! only change during `Phase::Src` (label IMPL), which is when a doc CL is
//! locked and an unlocked src CL exists.
//!
//! Which directories those are comes from `src_dirs` rather than from the word
//! `crates`, so a project that renamed or grouped its packages is guarded where
//! its source actually is. `src_layout` holds that.
//!
//! Enforcement is global: not just staged files, but ANY untracked or
//! unstaged source changes will block the commit. Revert disallowed
//! changes before committing.
//!
//! Severity: Error (blocks commit, push, and build).

use std::path::Path;
use std::process::Command;

use crate::changelist_helpers::{self, Phase};
use crate::src_layout::{self, SrcLayout};
use crate::{Lint, RepoLint, LintError, RepoContext};

const LINT_NAME: &str = "changelist-required";

pub struct ChangelistRequired;

impl Lint for ChangelistRequired {
    fn name(&self) -> &'static str {
        LINT_NAME
    }
    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for ChangelistRequired {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        // The mock dir comes straight from the context. Previously this stole a
        // `workspace_root` from `crates.first()` and returned no findings when
        // there were no crates, which silently disabled the gate in a repo whose
        // taxonomy had not been settled yet.
        let workspace_root = ctx.mock_dir;
        let layout = SrcLayout::new(workspace_root, ctx.src_dirs);

        let design_rounds = workspace_root.join("design_rounds");
        let phase = changelist_helpers::current_phase(&design_rounds);

        // Source changes are allowed only in Phase::Src.
        if phase == Phase::Src {
            return Vec::new();
        }

        // Check if any .rs source files are modified.
        let modified = crate::fmt_only::drop_fmt_only(
            workspace_root,
            // FIXME: `.rs` is the half of this that is still rust's convention
            // rather than the project's. The directories now come from
            // `src_dirs`; what counts as source in them does not, and nothing
            // in `mockspace.toml` names it. A project writing zig or typescript
            // under a declared source directory gets a gate that matches
            // nothing, passes, and looks exactly like a gate that works, which
            // is verbatim the failure `src_layout` was written to end. Wants an
            // extension set per source directory, or a language key carrying
            // one. Catalogued at `a_source_gate_guards_the_extensions_the_project_writes`.
            src_layout::changed_files(workspace_root, &layout, |f| {
                layout.holds(f) && f.ends_with(".rs")
            }),
        );

        modified
            .into_iter()
            .filter(|(file, source)| {
                // Skip nuked crates: intentionally wiped source is not a
                // phase violation. Check the crate's lib.rs for the nuke marker.
                let librs = layout
                    .package_dir(workspace_root, file)
                    .map(|d| d.join("src/lib.rs"))
                    .unwrap_or_default();
                let nuked = std::fs::read_to_string(&librs)
                    .map(|s| s.contains("Nuked by"))
                    .unwrap_or(false);
                if nuked {
                    return false;
                }
                // Skip scaffolding. A file declaring nothing is not the source
                // this gate protects: it wires a crate into the workspace so
                // the dependency graph, the layer numbering, and the structure
                // documents have real edges to read.
                //
                // Read the STAGED blob, not the working tree. What gets
                // committed is the staged content, so judging the worktree
                // would let someone stage real source, overwrite the worktree
                // copy with comments, and commit past the gate.
                let content = staged_or_worktree(workspace_root, file, source);
                !content.map(|s| declares_nothing(&s)).unwrap_or(false)
            })
            .map(|(file, source)| {
                let crate_name =
                    layout.package_name(&file).unwrap_or_else(|| "unknown".to_string());

                let phase_hint = match phase {
                    Phase::Topic => {
                        "phase TOPIC: only topic files allowed. \
                         Create a doc changelist, lock it, then create a src changelist \
                         to open the source window"
                    },
                    Phase::Doc => {
                        "phase DOC: docs window open, source blocked. \
                         Lock the doc changelist and create a src changelist \
                         to open the source window"
                    },
                    Phase::SrcPlan => {
                        "phase DRAFT: doc CL locked, but no src changelist yet. \
                         Create an unlocked src changelist to open the source window"
                    },
                    Phase::Done => {
                        "phase CLOSED: round complete, both changelists locked. \
                         Start a new design round to make further changes"
                    },
                    Phase::Src => unreachable!(),
                };

                LintError::error(
                    crate_name,
                    0,
                    LINT_NAME,
                    format!(
                        "source file `{file}` ({source}) cannot be modified: \
                         {phase_hint}. Revert this change before committing.",
                    ),
                )
            })
            .collect()
    }
}

/// The content that would actually be committed.
///
/// For a staged entry that is the blob in the index, which is what a plain
/// `git commit` writes. Reading the working tree instead would judge content
/// the commit does not contain, and the difference is exactly where a gate
/// gets walked past.
///
/// Known gap, tracked #41: under `git commit -a` a file staged earlier and
/// then further modified commits the *worktree* blob, so this reads content
/// the commit will not carry. Detection is unaffected (the file is flagged
/// either way); only content-based judgment can go stale, and the case is
/// catalogued as an ignored test below. Closing it needs a signal for which
/// commit shape is in flight, which the sanitized hook environment no longer
/// provides for free.
pub(crate) fn staged_or_worktree(
    workspace_root: &Path,
    file: &str,
    source: &str,
) -> Option<String> {
    if source == "staged" {
        let out = Command::new("git")
            // `:./` resolves the path against the current directory. The bare
            // `:{file}` form resolves from the repository root, so from a mock
            // directory it reads `<repo>/crates/...` rather than
            // `<repo>/mock/crates/...`: the wrong blob where both exist, and a
            // silent fall-through to the worktree where they do not.
            .args(["show", &format!(":./{file}")])
            .current_dir(workspace_root)
            .output()
            .ok()?;
        if out.status.success() {
            return String::from_utf8(out.stdout).ok();
        }
    }
    std::fs::read_to_string(workspace_root.join(file)).ok()
}

/// Whether a Rust file declares nothing at all.
///
/// Conservative by construction: everything except comments, inner attributes,
/// and blank lines counts as a declaration, so any real item trips it. This is
/// a scaffold test, not a parser, and it errs toward calling a file source.
fn declares_nothing(src: &str) -> bool {
    let mut in_block = false;
    for raw in src.lines() {
        let l = raw.trim();
        if in_block {
            // Only a line that ends the block and carries nothing after it is
            // still comment. `*/ pub fn real() {}` is code.
            if let Some(rest) = l.split_once("*/") {
                if !rest.1.trim().is_empty() {
                    return false;
                }
                in_block = false;
            }
            continue;
        }
        if l.is_empty() || l.starts_with("//") || l.starts_with("#![") {
            continue;
        }
        if l.starts_with("/*") {
            match l.split_once("*/") {
                Some((_, rest)) if rest.trim().is_empty() => continue,
                Some(_) => return false,
                None => {
                    in_block = true;
                    continue;
                },
            }
        }
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The gate end to end against a project that does not use the word
    /// `crates`, which is the case the whole `src_layout` change exists for.
    ///
    /// This walks the real path rather than the predicates: a `RepoContext`
    /// carrying `src_dirs`, a git repository on disk, a frozen phase, and a
    /// finding that has to name the package. The control is the same repository
    /// read through a layout that still says `crates`, where the gate must find
    /// nothing at all, because finding nothing is exactly what shipped before
    /// this and is the failure nobody would have seen.
    #[test]
    fn a_project_that_renamed_its_source_directory_is_gated_where_its_source_is() {
        let root = std::env::temp_dir().join(format!("clr-renamed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("libs/widget/src")).unwrap();
        std::fs::create_dir_all(root.join("design_rounds")).unwrap();
        // both changelists locked, so the phase is CLOSED and no source may move
        for name in ["202601010000_changelist.doc.lock.md", "202601010001_changelist.src.lock.md"]
        {
            std::fs::write(root.join("design_rounds").join(name), "locked\n").unwrap();
        }
        std::fs::write(root.join("libs/widget/src/lib.rs"), "pub fn a() {}\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "t@example.com"],
            vec!["config", "user.name", "t"],
            vec!["add", "-A"],
            vec!["commit", "-q", "-m", "seed", "--no-gpg-sign"],
        ] {
            Command::new("git").args(&args).current_dir(&root).output().unwrap();
        }
        std::fs::write(root.join("libs/widget/src/lib.rs"), "pub fn a() {}\npub fn b() {}\n")
            .unwrap();

        let run = |dirs: &[std::path::PathBuf]| {
            let crates = std::collections::BTreeSet::new();
            ChangelistRequired.check_repo(&RepoContext {
                mock_dir:   &root,
                repo_root:  &root,
                all_crates: &crates,
                src_dirs:   dirs,
                invocation: None,
                canon_paths: &[],
                open_panels: &[],
                registry:   &Default::default(),
            })
        };

        let found = run(&[root.join("libs")]);
        assert_eq!(found.len(), 1, "expected the edit under libs/ to be gated: {found:?}");
        assert_eq!(found[0].crate_name, "widget");

        // The control, and the reason this test is worth its runtime.
        assert!(
            run(&[root.join("crates")]).is_empty(),
            "a layout naming a directory this project does not have must find \
             nothing, which is what the old hardcoded gate did here"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Catalogued gap, tracked #41. Under `git commit -a` the commit carries
    /// the worktree blob of a staged-then-modified file, so the content this
    /// function judges should be the worktree's. Today it returns the index
    /// blob (correct for plain `git commit`, stale for `commit -a`), and the
    /// function has no signal for which shape is in flight.
    #[test]
    #[ignore = "catalogue: staged-then-modified file under commit -a commits \
                the worktree blob while this reads the index blob; needs the \
                commit shape signal; tracked #41"]
    fn a_staged_then_modified_file_reads_as_what_commit_a_commits() {
        let dir = std::env::temp_dir().join(format!("clr_commit_a_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("crates/x/src")).unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(&dir)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        git(&["init", "-q"]);
        let file = "crates/x/src/lib.rs";
        std::fs::write(dir.join(file), "pub fn staged_version() {}\n").unwrap();
        git(&["add", file]);
        // Modified again after staging: this is what `commit -a` would commit.
        std::fs::write(dir.join(file), "pub fn worktree_version() {}\n").unwrap();

        let content = staged_or_worktree(&dir, file, "staged").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            content.contains("worktree_version"),
            "under commit -a the judged content must be the worktree blob; \
             got the index blob instead: {content:?}"
        );
    }
}

#[cfg(test)]
mod fmt_only_exemption {
    use std::process::Command;

    use super::*;

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
            Command::new("git").args(&args).current_dir(&root).output().unwrap();
        }
        root
    }

    fn blocked(root: &std::path::Path) -> Vec<LintError> {
        let crates = std::collections::BTreeSet::new();
        let src_dirs = [root.join("crates")];
        let ctx = RepoContext {
            mock_dir:   root,
            repo_root:  root,
            all_crates: &crates,
            src_dirs:   &src_dirs,
            invocation: None,
            canon_paths: &[],
            open_panels: &[],
            registry:   &Default::default(),
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
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).to_string())
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
        std::fs::write(root.join("crates/x/src/lib.rs"), fmt.replace("x + 1", "x + 2")).unwrap();
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

    use super::*;

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
            Command::new("git").args(&args).current_dir(&root).output().unwrap();
        }
        root
    }

    fn gate(root: &std::path::Path) -> Vec<LintError> {
        let crates = std::collections::BTreeSet::new();
        let src_dirs = [root.join("crates")];
        ChangelistRequired.check_repo(&RepoContext {
            mock_dir:   root,
            repo_root:  root,
            all_crates: &crates,
            src_dirs:   &src_dirs,
            invocation: None,
            canon_paths: &[],
            open_panels: &[],
            registry:   &Default::default(),
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
