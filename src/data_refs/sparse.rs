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
//! ref, written from the same lines as the exclude file; per-worktree config is turned on for it,
//! with a bare clone's `core.bare` moved where only the clone itself reads it. And the
//! `reference-transaction` hook of `hook`: a merge that conflicts on a path the migration took off
//! the branch writes the branch's bytes whatever the patterns say, but git moves `ORIG_HEAD` first,
//! and the hook copies the files of every instance into its backup ref, so the next engine command
//! names what git wrote over and `restore` brings it back. `instantiate`, `commit` and the
//! migration write both; `verify` checks both and names a block whose declaration is gone.

use std::path::{Path, PathBuf};

use mockspace_store::{Git, Held, sparse_lines};

use crate::config::Config;

const BEGIN: &str = "# mockspace data refs: the instances git must not write over (managed, do not edit)";
const END: &str = "# end mockspace data refs";

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
    worktree_config(cfg)?;
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
    super::hook::install(cfg)
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
    super::hook::check(cfg)
}

/// Turns on per-worktree config, which the sparse checkout of one worktree needs. With it on, git
/// reads `core.bare` and `core.worktree` of the common config as every worktree's own, so a bare
/// clone's linked worktrees would read themselves as bare; where either is set there it first
/// moves into the config of the main worktree alone, as `git sparse-checkout init` does.
fn worktree_config(cfg: &Config) -> Result<(), String> {
    let repo = Git::new(cfg.repo_root.clone());
    let run = |args: &[&str]| repo.run(args, &[], None).map_err(|e| e.to_string());
    if run(&["config", "--type=bool", "--get", "extensions.worktreeConfig"])?.text() == "true" {
        return Ok(());
    }
    let common = PathBuf::from(git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?);
    let (shared, main) = (common.join("config"), common.join("config.worktree"));
    let (shared, main) = (shared.to_string_lossy(), main.to_string_lossy());
    for key in ["core.bare", "core.worktree"] {
        let value = run(&["config", "--file", &shared, "--get", key])?;
        // `core.bare = false` is every worktree's answer anyway, and stays where it is
        if !value.ok() || (key == "core.bare" && value.text() != "true") {
            continue;
        }
        git(&cfg.repo_root, &["config", "--file", &main, key, &value.text()])?;
        git(&cfg.repo_root, &["config", "--file", &shared, "--unset", key])?;
    }
    git(&cfg.repo_root, &["config", "extensions.worktreeConfig", "true"])?;
    Ok(())
}
