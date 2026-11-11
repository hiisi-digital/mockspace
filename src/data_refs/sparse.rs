//! Keeps git from writing over, or removing, the files of an instance.
//!
//! The files of an instance are hidden from the branch by `.git/info/exclude`, and git treats an
//! ignored file as expendable: a checkout of a commit that tracks the same paths (a branch cut
//! before the migration) overwrites them without a word, and leaving that commit removes them.
//! There is no `pre-checkout` hook, the hooks that do run (`post-checkout`,
//! `reference-transaction`) run after the working tree was written, and no configuration makes git
//! refuse to overwrite an ignored file; only the `--no-overwrite-ignore` flag does, and nothing can
//! put a flag on a person's command.
//!
//! What git does honour is a sparse checkout: a path the patterns leave out is not written, and
//! one that is there and differs from the commit is left in place with a warning, never replaced.
//! So each worktree carries a managed block of exclusion patterns, one for every declared ref,
//! and `instantiate` and the migration write it. `verify` checks it.

use std::path::Path;
use std::process::Command;

use crate::config::Config;

const BEGIN: &str = "# mockspace data refs: the instances git must not write over (managed, do not edit)";
const END: &str = "# end mockspace data refs";

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_WORK_TREE")
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// The path of `dir` from the repository root, `/`-separated, with no slash at either end.
fn from_root(cfg: &Config, dir: &Path) -> String {
    dir.strip_prefix(&cfg.repo_root)
        .map(|rel| rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
        .unwrap_or_default()
}

/// The exclusion patterns of every declared ref, one line each, anchored at the repository root.
pub fn managed_lines(cfg: &Config) -> Vec<String> {
    let mut out = Vec::new();
    for d in &cfg.data_refs {
        let dir = from_root(cfg, &d.instance_dir(&cfg.mock_dir));
        let lead = if dir.is_empty() { "/".to_string() } else { format!("/{dir}/") };
        match d.include.as_deref() {
            None => out.push(format!("!{lead}")),
            Some(patterns) => {
                out.extend(patterns.iter().map(|p| format!("!{lead}{}", p.trim_start_matches('/'))));
            },
        }
    }
    out
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

fn pattern_file(cfg: &Config) -> Result<std::path::PathBuf, String> {
    let p = git(&cfg.repo_root, &["rev-parse", "--path-format=absolute", "--git-path", "info/sparse-checkout"])?;
    Ok(std::path::PathBuf::from(p))
}

/// Writes the managed block for this worktree and turns the sparse checkout on for it, without
/// applying it: nothing already in the tree is removed by this.
pub fn install(cfg: &Config) -> Result<(), String> {
    let lines = managed_lines(cfg);
    if lines.is_empty() {
        return Ok(());
    }
    let repo = &cfg.repo_root;
    git(repo, &["config", "extensions.worktreeConfig", "true"])?;
    git(repo, &["config", "--worktree", "core.sparseCheckout", "true"])?;
    git(repo, &["config", "--worktree", "core.sparseCheckoutCone", "false"])?;
    let file = pattern_file(cfg)?;
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let text = compose(&existing, &lines);
    if text != existing {
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::write(&file, text).map_err(|e| format!("writing {}: {e}", file.display()))?;
    }
    Ok(())
}

/// Whether this worktree is protected: the sparse checkout is on for it and its file holds the
/// block as it is now declared. Says what to run where it is not.
pub fn check(cfg: &Config) -> Result<(), String> {
    let lines = managed_lines(cfg);
    if lines.is_empty() {
        return Ok(());
    }
    let on = git(&cfg.repo_root, &["config", "--worktree", "--get", "core.sparseCheckout"]).is_ok_and(|v| v == "true");
    let file = pattern_file(cfg)?;
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if on && compose(&existing, &lines) == existing {
        Ok(())
    } else {
        Err(format!(
            "this worktree does not keep git from writing over its instances (a checkout of a commit from before \
             the migration would overwrite them); `cargo mock data-refs instantiate` sets that up"
        ))
    }
}
