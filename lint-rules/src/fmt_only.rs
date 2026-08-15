//! Is a source change nothing but what `rustfmt` would have done?
//!
//! The phase gates freeze `crates/**/*.rs` outside the IMPL window. That is
//! the design and it stays. But the pre-commit auto-fix runs `rustfmt` over
//! staged sources, so a round whose formatting drifted lands its fmt changes
//! after the src changelist locks, and the gate then refuses them. The only
//! way out was a mechanical micro-round that made no design change at all.
//!
//! So a source change is permitted in a frozen phase when it is **exactly**
//! what `rustfmt` produces from the committed version. The oracle is the tool
//! itself rather than a model of what it does: whatever `rustfmt` chooses to
//! reshape, including moving bounds into a `where` clause, merging or
//! reordering imports, or anything a future release adds, is by construction
//! allowed, and any hand edit mixed in fails to reproduce byte for byte.
//!
//! Two properties make this safe to permit rather than to trust:
//!
//! - It is **verified, not asserted**. No marker, no trailer, no flag a
//!   caller sets. The check reads `HEAD`'s blob, formats it, and compares.
//! - It **fails closed**. A missing `rustfmt`, an unreadable blob, or a file
//!   `rustfmt` refuses (unparseable source) all report "not fmt-only", so
//!   the gate refuses exactly as it did before.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Why a change was not fmt-only, for the message the gate prints.
#[derive(Debug, PartialEq, Eq)]
pub enum NotFmtOnly {
    /// The file differs from `rustfmt`'s output for the committed version,
    /// so it carries an edit beyond formatting.
    ContentDiffers,
    /// `rustfmt` is not on PATH. Fails closed.
    RustfmtMissing,
    /// The committed version could not be read, or `rustfmt` refused it.
    Undeterminable,
}

/// True when `file` in the working tree is byte-identical to `rustfmt`'s
/// output for the version at `HEAD`.
///
/// `dir` is where git and rustfmt run; `file` is relative to it, and
/// `current` is the content to judge, which must be what the commit will
/// carry rather than whatever the working tree happens to hold. Both gates
/// pass their mock directory, which is what `git diff --relative` reported
/// those paths against.
///
/// # Errors
///
/// Returns [`NotFmtOnly`] rather than a bool so the gate can say which of
/// the three cases it hit. Every one of them means "refuse".
pub fn is_fmt_only_change(dir: &Path, file: &str, current: &str) -> Result<(), NotFmtOnly> {
    let committed = git_show(dir, file).ok_or(NotFmtOnly::Undeterminable)?;

    // Cheap exit: unchanged content is trivially fmt-only and needs no rustfmt.
    if committed == current {
        return Ok(());
    }

    let formatted = rustfmt(dir, &committed)?;
    if formatted == current {
        Ok(())
    } else {
        Err(NotFmtOnly::ContentDiffers)
    }
}

/// Drop files whose only change is what `rustfmt` would have produced.
///
/// **The content judged is the content the commit will carry**, resolved by
/// [`crate::staged_or_worktree`] from the same `source` label the gate
/// collected the file under. Reading the working tree instead is a bypass and
/// not a subtle one: stage a semantic edit, restore the worktree to
/// `rustfmt(HEAD)`, and a predicate judging the worktree drops the file while
/// the index still carries the edit. The pre-commit auto-fix makes that state
/// reachable without adversarial intent, because it deliberately does not
/// re-stage a partially staged file.
///
/// Fails closed throughout: a missing `rustfmt`, an untracked file, content
/// that cannot be resolved, or source `rustfmt` will not parse all leave the
/// file in the list and the gate refuses exactly as before.
pub fn drop_fmt_only(
    workspace_root: &Path,
    files: Vec<(String, String)>,
) -> Vec<(String, String)> {
    files
        .into_iter()
        .filter(|(file, source)| {
            let Some(current) = crate::changelist_required::staged_or_worktree(workspace_root, file, source) else {
                return true; // cannot resolve the content: refuse
            };
            match is_fmt_only_change(workspace_root, file, &current) {
                Ok(()) => false,
                Err(NotFmtOnly::RustfmtMissing) => {
                    // The one case the user can act on: the exemption is
                    // unavailable rather than declined, and silence would
                    // read as "this change is not formatting".
                    eprintln!(
                        "--- fmt-only exemption unavailable: rustfmt is not on PATH, so \
                         `{file}` is judged as an ordinary source edit ---"
                    );
                    true
                },
                Err(_) => true,
            }
        })
        .collect()
}

/// The blob at `HEAD` for `file`, or `None` when the file is new, untracked,
/// or git is unavailable.
///
/// `file` is relative to `dir`, and the `HEAD:./` form is what makes git
/// resolve it that way. The bare `HEAD:<path>` form is resolved from the
/// repository root instead, so from a mock directory it would look for
/// `<repo>/crates/...` and miss `<repo>/mock/crates/...` entirely, reporting
/// every file as untracked and refusing every change.
fn git_show(dir: &Path, file: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["show", &format!("HEAD:./{file}")])
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Run `rustfmt` over `source`, resolving the repo's own `rustfmt.toml` by
/// running with `dir` as the working directory.
fn rustfmt(dir: &Path, source: &str) -> Result<String, NotFmtOnly> {
    let mut child = Command::new("rustfmt")
        .arg("--emit=stdout")
        .arg("--quiet")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| NotFmtOnly::RustfmtMissing)?;
    child
        .stdin
        .as_mut()
        .ok_or(NotFmtOnly::Undeterminable)?
        .write_all(source.as_bytes())
        .map_err(|_| NotFmtOnly::Undeterminable)?;
    let out = child.wait_with_output().map_err(|_| NotFmtOnly::Undeterminable)?;
    if !out.status.success() {
        // Unparseable source, or a rustfmt that refused the input.
        return Err(NotFmtOnly::Undeterminable);
    }
    String::from_utf8(out.stdout).map_err(|_| NotFmtOnly::Undeterminable)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A git repo with one committed Rust file, deliberately unformatted.
    struct Repo {
        root: std::path::PathBuf,
    }

    impl Repo {
        fn new(name: &str, committed: &str) -> Repo {
        // A process-wide counter, not a clock. SystemTime here has microsecond
        // resolution at best (twenty back-to-back samples yield four distinct
        // values on this host), and `create_dir_all` succeeds silently on an
        // existing directory, so two tests starting in the same tick shared a
        // scratch directory and overwrote each other's fixtures.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "fmtonly-{}-{}-{}",
                name,
                std::process::id(),
                SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("crates/foo/src")).unwrap();
            std::fs::write(root.join("crates/foo/src/lib.rs"), committed).unwrap();
            for args in [
                vec!["init", "-q"],
                vec!["config", "user.email", "t@example.com"],
                vec!["config", "user.name", "t"],
                vec!["add", "-A"],
                vec!["commit", "-q", "-m", "seed", "--no-gpg-sign"],
            ] {
                Command::new("git").args(&args).current_dir(&root).output().unwrap();
            }
            Repo {
                root,
            }
        }

        fn write(&self, contents: &str) {
            std::fs::write(self.root.join("crates/foo/src/lib.rs"), contents).unwrap();
        }

        /// Judge the worktree copy, which is what these unit tests vary.
        fn check(&self) -> Result<(), NotFmtOnly> {
            let cur = std::fs::read_to_string(self.root.join("crates/foo/src/lib.rs")).unwrap();
            is_fmt_only_change(&self.root, "crates/foo/src/lib.rs", &cur)
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Deliberately misformatted, so rustfmt has something to change.
    const UGLY: &str = "pub fn a(  x:u8 )->u8{x+1}\n";

    /// The toolchain pins `components = ["rustfmt", ...]`, so an absent
    /// rustfmt is a broken environment rather than a supported one. These
    /// tests used to `return` silently on it, which turned that breakage into
    /// a green.
    fn require_rustfmt() {
        let ok = Command::new("rustfmt")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(ok, "rustfmt is not on PATH; the pinned toolchain provides it");
    }

    #[test]
    fn rustfmts_own_output_is_fmt_only() {
        require_rustfmt();
        let r = Repo::new("clean", UGLY);
        let formatted = rustfmt(&r.root, UGLY).expect("rustfmt runs");
        assert_ne!(formatted, UGLY, "the fixture must actually be reformatted");
        r.write(&formatted);
        assert_eq!(r.check(), Ok(()));
    }

    #[test]
    fn an_edit_smuggled_alongside_formatting_is_refused() {
        require_rustfmt();
        let r = Repo::new("smuggled", UGLY);
        // rustfmt's output, then one semantic character changed. This is the
        // case the whole exemption turns on: it looks formatted, it compiles,
        // and it is not what rustfmt produced.
        let formatted = rustfmt(&r.root, UGLY).expect("rustfmt runs");
        r.write(&formatted.replace("x + 1", "x + 2"));
        assert_eq!(r.check(), Err(NotFmtOnly::ContentDiffers));
    }

    #[test]
    fn a_pure_semantic_edit_is_refused() {
        require_rustfmt();
        let r = Repo::new("semantic", UGLY);
        r.write("pub fn a(x: u8) -> u8 {\n    x + 99\n}\n");
        assert_eq!(r.check(), Err(NotFmtOnly::ContentDiffers));
    }

    #[test]
    fn an_unchanged_file_is_fmt_only_without_consulting_rustfmt() {
        let r = Repo::new("unchanged", UGLY);
        assert_eq!(r.check(), Ok(()), "an unchanged file cannot carry an edit");
    }

    #[test]
    fn an_untracked_file_is_undeterminable_and_therefore_refused() {
        let r = Repo::new("untracked", UGLY);
        std::fs::write(r.root.join("crates/foo/src/new.rs"), UGLY).unwrap();
        assert_eq!(
            is_fmt_only_change(&r.root, "crates/foo/src/new.rs", UGLY),
            Err(NotFmtOnly::Undeterminable),
            "a file with no committed version has nothing to compare against"
        );
    }

    #[test]
    fn unparseable_source_is_undeterminable_and_therefore_refused() {
        require_rustfmt();
        let r = Repo::new("unparseable", "pub fn a( {{{ \n");
        r.write("pub fn a( {{{  \n");
        assert_eq!(
            r.check(),
            Err(NotFmtOnly::Undeterminable),
            "rustfmt cannot format it, so nothing can be established"
        );
    }
}
