//! Cross-crate lint: source code changes require the IMPL phase.
//!
//! Source changes (`*.rs` in `crates/`) are only allowed during
//! `Phase::Src` (label IMPL) — when a doc CL is locked AND an unlocked
//! src CL exists.
//!
//! Enforcement is global: not just staged files, but ANY untracked or
//! unstaged source changes will block the commit. Revert disallowed
//! changes before committing.
//!
//! Severity: Error (blocks commit, push, and build).

use std::path::Path;
use std::process::Command;

use crate::changelist_helpers::{self, Phase};
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

        let design_rounds = workspace_root.join("design_rounds");
        let phase = changelist_helpers::current_phase(&design_rounds);

        // Source changes are allowed only in Phase::Src.
        if phase == Phase::Src {
            return Vec::new();
        }

        // Check if any .rs source files are modified.
        let modified = drop_fmt_only(workspace_root, get_all_modified_rs_files(workspace_root));

        modified
            .into_iter()
            .filter(|(file, source)| {
                // Skip nuked crates — intentionally wiped source is not a
                // phase violation. Check the crate's lib.rs for the nuke marker.
                let crate_name = extract_crate_name(file).unwrap_or_default();
                let librs = workspace_root
                    .join("crates")
                    .join(&crate_name)
                    .join("src/lib.rs");
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
                let crate_name = extract_crate_name(&file).unwrap_or_else(|| "unknown".to_string());

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
                        "source file `{file}` ({source}) cannot be modified — \
                         {phase_hint}. Revert this change before committing.",
                    ),
                )
            })
            .collect()
    }
}

/// Get all modified .rs files in crates/ (staged + unstaged + untracked).
/// Drop files whose only change is what `rustfmt` would have produced.
///
/// The pre-commit auto-fix formats staged sources, so a round whose
/// formatting drifted lands fmt changes after its src changelist locks and
/// the gate refuses them, forcing a mechanical micro-round that changes no
/// design. A change that reproduces `rustfmt`'s output for the committed
/// version byte for byte carries no edit, so it passes.
///
/// The check is verified rather than declared, and it fails closed: a
/// missing `rustfmt`, an untracked file, or source `rustfmt` will not parse
/// all leave the file in the list and the gate refuses as before.
fn drop_fmt_only(workspace_root: &Path, files: Vec<(String, String)>) -> Vec<(String, String)> {
    files
        .into_iter()
        .filter(|(file, _)| crate::fmt_only::is_fmt_only_change(workspace_root, file).is_err())
        .collect()
}

fn get_all_modified_rs_files(workspace_root: &Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = Vec::new();

    // Staged changes
    if let Some(output) = run_git(workspace_root, &[
        "diff",
        "--cached",
        "--name-only",
        "--relative",
        "--",
        "crates/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if is_crate_source(file) {
                add_unique(&mut files, file, "staged");
            }
        }
    }

    // Unstaged tracked changes
    if let Some(output) = run_git(workspace_root, &[
        "diff",
        "--name-only",
        "--relative",
        "--",
        "crates/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if is_crate_source(file) {
                add_unique(&mut files, file, "unstaged");
            }
        }
    }

    // Untracked files
    if let Some(output) = run_git(workspace_root, &[
        "ls-files",
        "--others",
        "--exclude-standard",
        "--",
        "crates/",
    ]) {
        for line in output.lines() {
            let file = line.trim();
            if is_crate_source(file) {
                add_unique(&mut files, file, "untracked");
            }
        }
    }

    files
}

fn is_crate_source(file: &str) -> bool {
    !file.is_empty() && file.starts_with("crates/") && file.ends_with(".rs")
}

fn add_unique(list: &mut Vec<(String, String)>, file: &str, source: &str) {
    if !list.iter().any(|(f, _)| f == file) {
        list.push((file.to_string(), source.to_string()));
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Extract crate name from a path like `crates/<crate-name>/src/lib.rs`.
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
fn staged_or_worktree(workspace_root: &Path, file: &str, source: &str) -> Option<String> {
    if source == "staged" {
        let out = Command::new("git")
            .args(["show", &format!(":{file}")])
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

fn extract_crate_name(path: &str) -> Option<String> {
    let after_crates = path.strip_prefix("crates/")?;
    let end = after_crates.find('/')?;
    Some(after_crates[.. end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let root = std::env::temp_dir().join(format!(
            "clr-fmt-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
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
        let ctx = RepoContext {
            mock_dir:   root,
            repo_root:  root,
            all_crates: &crates,
            invocation: None,
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
        let Some(fmt) = formatted(&root, UGLY) else {
            let _ = std::fs::remove_dir_all(&root);
            return; // no rustfmt on this machine
        };
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
