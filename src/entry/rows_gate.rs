//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital

//! `mock rows-gate`: the pre-commit gate of a commit that stages registry rows of the
//! namespaces the console commits straight onto the trunk (`console_direct = true`) and
//! nothing else.
//!
//! Such a commit is held to the registry check alone. No lint pack is loaded or built, no
//! crate is parsed, no lint runs. The tree it passed on is recorded under the git directory,
//! so the pre-push hook can let a range of such commits through without linting it again.
//!
//! Exit codes: 0 the rows passed and the tree is recorded, 1 the registry check failed,
//! `NOT_ROWS` the staged set is not rows of those namespaces alone, and the hook runs the
//! gate it always ran.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::config::Config;

/// What `rows-gate` exits with when the staged set is anything more than direct rows.
pub(crate) const NOT_ROWS: u8 = 3;

/// The record of trees the rows gate passed, under the git directory, as `git rev-parse
/// --git-path` resolves it for the worktree the commit is made in.
pub(crate) const RECORD: &str = "mockspace/gated-rows";

/// How many recorded trees are kept; the oldest are dropped past it.
const KEPT: usize = 256;

/// The keys of every `[[registry.namespace]]` the config marks `console_direct = true`.
pub(crate) fn direct_namespaces(config_text: &str) -> Vec<String> {
    let Ok(doc) = config_text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let Some(namespaces) = doc
        .get("registry")
        .and_then(|r| r.get("namespace"))
        .and_then(|n| n.as_array_of_tables())
    else {
        return Vec::new();
    };
    namespaces
        .iter()
        .filter(|t| t.get("console_direct").and_then(|v| v.as_bool()) == Some(true))
        .filter_map(|t| t.get("key").and_then(|k| k.as_str()).map(str::to_string))
        .collect()
}

/// Whether the file at `path` (relative to the repository root) holding `text` is a registry
/// file whose every top-level key is an array of tables of a direct namespace.
///
/// A file outside `<mock>/registry/`, a file that is not TOML, an empty one, or one carrying
/// any other key is not.
pub(crate) fn is_direct_row(path: &str, text: &str, mock_rel: &str, direct: &[String]) -> bool {
    let prefix = format!("{}/registry/", mock_rel.trim_end_matches('/'));
    if !path.starts_with(&prefix) || !path.ends_with(".toml") {
        return false;
    }
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    let table = doc.as_table();
    !table.is_empty()
        && table.iter().all(|(key, item)| {
            direct.iter().any(|d| d == key) && item.as_array_of_tables().is_some()
        })
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Every staged change, as its status letter and path. `None` when git cannot say.
fn staged(root: &Path) -> Option<Vec<(char, String)>> {
    let text = git(root, &["diff", "--cached", "--name-status", "--no-renames"])?;
    Some(
        text.lines()
            .filter_map(|l| {
                let (status, path) = l.split_once('\t')?;
                Some((status.chars().next()?, path.to_string()))
            })
            .collect(),
    )
}

/// The record file for the worktree at `root`.
pub(crate) fn record_path(root: &Path) -> Option<PathBuf> {
    let p = git(root, &["rev-parse", "--git-path", RECORD])?;
    let p = PathBuf::from(p.trim());
    Some(if p.is_absolute() { p } else { root.join(p) })
}

/// Append the staged tree to the record, keeping the newest `KEPT`.
fn record(root: &Path) -> Option<()> {
    let tree = git(root, &["write-tree"])?;
    let path = record_path(root)?;
    fs::create_dir_all(path.parent()?).ok()?;
    let old = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<&str> = old.lines().filter(|l| !l.is_empty()).collect();
    lines.push(tree.trim());
    let start = lines.len().saturating_sub(KEPT);
    fs::write(&path, lines[start ..].join("\n") + "\n").ok()
}

/// The verb. `gate` runs the registry check over the loaded registry and returns its error
/// count; it is the lint path's own registry gate, so the rows are held to exactly that.
pub(crate) fn run(
    cfg: &Config,
    gate: impl FnOnce(&crate::registry::Registry) -> usize,
) -> ExitCode {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "mock".to_string());
    let direct = direct_namespaces(&fs::read_to_string(&cfg.config_path).unwrap_or_default());
    let not_rows = |why: &str| {
        eprintln!("rows-gate: {why}");
        ExitCode::from(NOT_ROWS)
    };
    if direct.is_empty() {
        return not_rows("no namespace is console_direct");
    }
    let Some(changes) = staged(&cfg.repo_root) else {
        return not_rows("git could not list the staged changes");
    };
    if changes.is_empty() {
        return not_rows("nothing is staged");
    }
    for (status, path) in &changes {
        let spec = if *status == 'D' { format!("HEAD:{path}") } else { format!(":{path}") };
        let text = git(&cfg.repo_root, &["show", &spec]).unwrap_or_default();
        if !is_direct_row(path, &text, &mock_rel, &direct) {
            return not_rows(&format!(
                "{path} is not a row of a console_direct namespace"
            ));
        }
    }
    let registry = match crate::registry::load_registry_for(cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    let errors = gate(&registry);
    if errors > 0 {
        eprintln!("registry check failed: {errors} error(s)");
        return ExitCode::FAILURE;
    }
    if record(&cfg.repo_root).is_none() {
        eprintln!("rows-gate: the passed tree could not be recorded; the push runs its own gate");
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
#[path = "rows_gate_tests.rs"]
mod tests;
