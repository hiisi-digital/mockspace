//! Keeps git from writing over, or losing, the files of an instance.
//!
//! The files of an instance are hidden from the branch by `.git/info/exclude`, and git treats an
//! ignored file as expendable: a checkout of a commit that tracks the same paths (a branch cut
//! before the migration) overwrites them without a word, and leaving that commit removes them.
//! There is no `pre-checkout` hook, the hooks that do run (`post-checkout`) run after the working
//! tree was written, and no configuration makes git refuse to overwrite an ignored file; only the
//! `--no-overwrite-ignore` flag does, and nothing can put a flag on a person's command.
//!
//! Two things are done against that. A sparse checkout: a path the patterns leave out is not
//! written, and one that is there and differs from the commit is left in place with a warning,
//! never replaced. So each worktree carries a managed block of patterns, one set for every declared
//! ref, written from the same lines as the exclude file. And a `reference-transaction` hook: a merge
//! that conflicts on a path the migration took off the branch writes the branch's bytes whatever the
//! patterns say, but git moves `ORIG_HEAD` first (a merge, a rebase and a reset all do), and the hook
//! copies the edited files into a backup ref before it lets that through. `instantiate`, `commit` and
//! the migration write both; `verify` checks both and names a block whose declaration is gone.

use std::path::{Path, PathBuf};

use mockspace_store::{Git, Held, sparse_lines};

use crate::config::Config;

const BEGIN: &str = "# mockspace data refs: the instances git must not write over (managed, do not edit)";
const END: &str = "# end mockspace data refs";

/// The line that marks the hook as the one this writes.
const HOOK_MARK: &str = "# mockspace data refs: copies an edited instance file before git writes over it (managed, do not edit)";

fn hook_text() -> String {
    format!(
        "#!/bin/sh\n{HOOK_MARK}\n\
         [ \"$1\" = prepared ] || exit 0\n\
         [ -z \"$MOCKSPACE_SNAPSHOT\" ] || exit 0\n\
         moves=$(cat)\n\
         printf '%s\\n' \"$moves\" | grep ' ORIG_HEAD$' >/dev/null || exit 0\n\
         if command -v mock >/dev/null 2>&1; then launcher=mock\n\
         elif command -v cargo-mock >/dev/null 2>&1; then launcher=cargo-mock\n\
         else\n\
         \x20   echo 'mockspace: no launcher on PATH, so an edited instance file is not copied before this write' >&2\n\
         \x20   exit 0\n\
         fi\n\
         MOCKSPACE_SNAPSHOT=1 \"$launcher\" data-refs snapshot >&2 || {{\n\
         \x20   echo 'mockspace: the edited instance files could not be copied, so nothing was written' >&2\n\
         \x20   exit 1\n\
         }}\n"
    )
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    Git::new(repo.to_path_buf()).must(args, &[], None).map(|o| o.text()).map_err(|e| e.to_string())
}

/// The path of `dir` from the repository root, `/`-separated, with no slash at either end.
fn from_root(cfg: &Config, dir: &Path) -> String {
    dir.strip_prefix(&cfg.repo_root)
        .map(|rel| rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        .unwrap_or_default()
}

/// The sparse-checkout lines of every declared ref, anchored at the repository root, from the
/// same writer as the exclude lines of the instance.
pub fn managed_lines(cfg: &Config) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for d in &cfg.data_refs {
        let rel = from_root(cfg, &d.instance_dir(&cfg.mock_dir));
        let held = match d.include.as_deref() {
            None => None,
            Some(patterns) => Some(Held::new(patterns).map_err(|e| e.to_string())?),
        };
        out.extend(sparse_lines(&rel, held.as_ref()));
    }
    Ok(out)
}

/// What the sparse-checkout file of this worktree holds: whatever was in it outside the managed
/// block (or `/*`, everything, where it was empty), then the block.
fn compose(existing: &str, lines: &[String]) -> String {
    let mut keep: Vec<&str> = Vec::new();
    let mut inside = false;
    for l in existing.lines() {
        if l == BEGIN {
            inside = true;
        } else if l == END {
            inside = false;
        } else if !inside && !l.trim().is_empty() {
            keep.push(l);
        }
    }
    let mut text = String::new();
    if keep.is_empty() {
        text.push_str("/*\n");
    }
    keep.iter().for_each(|l| text.push_str(&format!("{l}\n")));
    if !lines.is_empty() {
        text.push_str(BEGIN);
        text.push('\n');
        lines.iter().for_each(|l| text.push_str(&format!("{l}\n")));
        text.push_str(END);
        text.push('\n');
    }
    text
}

fn path_of(cfg: &Config, what: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-path", what])?))
}

fn has_block(text: &str) -> bool {
    text.lines().any(|l| l == BEGIN)
}

/// Writes the managed block for this worktree, turns the sparse checkout on for it without applying
/// it (nothing already in the tree is removed by this), and installs the hook that copies an edit.
/// Where no ref is declared any more, takes the block out.
pub fn install(cfg: &Config) -> Result<(), String> {
    let lines = managed_lines(cfg)?;
    let file = path_of(cfg, "info/sparse-checkout")?;
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if lines.is_empty() {
        if has_block(&existing) {
            std::fs::write(&file, compose(&existing, &[])).map_err(|e| format!("writing {}: {e}", file.display()))?;
        }
        let _ = std::fs::remove_file(remembered_file(cfg)?);
        return Ok(());
    }
    let repo = &cfg.repo_root;
    git(repo, &["config", "extensions.worktreeConfig", "true"])?;
    git(repo, &["config", "--worktree", "core.sparseCheckout", "true"])?;
    git(repo, &["config", "--worktree", "core.sparseCheckoutCone", "false"])?;
    let text = compose(&existing, &lines);
    if text != existing {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::write(&file, text).map_err(|e| format!("writing {}: {e}", file.display()))?;
    }
    let held = remembered_file(cfg)?;
    let declared = serde_json::to_string(&cfg.data_refs).map_err(|e| e.to_string())?;
    if std::fs::read_to_string(&held).ok().as_deref() != Some(declared.as_str()) {
        std::fs::write(&held, declared).map_err(|e| format!("writing {}: {e}", held.display()))?;
    }
    install_hook(cfg)
}

/// Where this worktree keeps the declarations it last installed.
fn remembered_file(cfg: &Config) -> Result<PathBuf, String> {
    path_of(cfg, "mock-data-refs.json")
}

/// The declarations this worktree last installed, for the hook that runs on a branch that declares
/// none of them. Nothing where there are none or the file cannot be read.
pub fn remembered(cfg: &Config) -> Vec<super::DataRefDecl> {
    remembered_file(cfg)
        .ok()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// What stands where the hook belongs.
enum Hook {
    Absent,
    Ours(String),
    Foreign,
}

fn hook_state(cfg: &Config) -> Result<(PathBuf, Hook), String> {
    let path = path_of(cfg, "hooks/reference-transaction")?;
    let state = match std::fs::read_to_string(&path) {
        Ok(text) if text.contains(HOOK_MARK) => Hook::Ours(text),
        Ok(_) => Hook::Foreign,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Hook::Absent,
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    Ok((path, state))
}

fn install_hook(cfg: &Config) -> Result<(), String> {
    let (path, state) = hook_state(cfg)?;
    match state {
        Hook::Foreign => Err(format!(
            "{} is a hook of your own, and the copy of an edited instance file before a merge or a rebase writes over \
             it needs a `reference-transaction` hook: have its `prepared` case run `mock data-refs snapshot` when a \
             line of its input ends in ` ORIG_HEAD`, or move it aside and run `cargo mock data-refs instantiate`",
            path.display()
        )),
        Hook::Ours(text) if text == hook_text() => Ok(()),
        Hook::Ours(_) | Hook::Absent => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
            }
            mockspace_manifest::write_executable(&path, hook_text()).map_err(|e| format!("writing {}: {e}", path.display()))
        },
    }
}

/// Whether this worktree is protected: the sparse checkout is on for it, its file holds the block as
/// it is now declared, and the hook that copies an edit is there. Says what to run where it is not.
pub fn check(cfg: &Config) -> Result<(), String> {
    let lines = managed_lines(cfg)?;
    let file = path_of(cfg, "info/sparse-checkout")?;
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if lines.is_empty() {
        return if has_block(&existing) {
            Err("this worktree still keeps git from writing over the instance of a ref that is no longer declared (its \
                 paths are hidden from git); `cargo mock data-refs instantiate` takes the block out"
                .to_string())
        } else {
            Ok(())
        };
    }
    let on = Git::new(cfg.repo_root.clone())
        .run(&["config", "--worktree", "--get", "core.sparseCheckout"], &[], None)
        .map_err(|e| e.to_string())?
        .text()
        == "true";
    if !(on && compose(&existing, &lines) == existing) {
        return Err("this worktree does not keep git from writing over its instances (a checkout of a commit from before \
                    the migration would overwrite them); `cargo mock data-refs instantiate` sets that up"
            .to_string());
    }
    let (path, state) = hook_state(cfg)?;
    match state {
        Hook::Ours(text) if text == hook_text() => Ok(()),
        Hook::Foreign => Err(format!(
            "{} is a hook of your own that does not copy an edited instance file before a merge or a rebase writes over it; \
             see `cargo mock data-refs instantiate`",
            path.display()
        )),
        _ => Err(format!(
            "this worktree has no `reference-transaction` hook that copies an edited instance file before a merge or a \
             rebase writes over it ({}); `cargo mock data-refs instantiate` installs it",
            path.display()
        )),
    }
}
