//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Either surgically commit the exact files we touched, or print a suggestion.
///
/// When `opts.auto_commit` is true, commits ONLY the named files, through a
/// temporary index (`crate::plumbing`), so anything somebody has staged
/// elsewhere stays staged and no other modification is carried. No stash
/// needed. The real index is then brought to the new `HEAD` for those paths
/// alone, so `git status` is clean afterwards.
pub(crate) fn commit_or_suggest(cfg: &Config, opts: &SubcmdOpts, files: &[PathBuf], message: &str) {
    if !opts.auto_commit {
        let dr_rel = pathdiff(&cfg.repo_root, &cfg.mock_dir.join("design_rounds"));
        eprintln!();
        eprintln!("  to commit:");
        eprintln!("    git add {dr_rel} && git commit -m \"{message}\"");
        return;
    }

    let root = &cfg.repo_root;
    let rels: Vec<String> = files.iter().map(|f| pathdiff(root, f)).collect();

    // A surgical commit of exactly our files, built through a temporary index
    // so the real one is never read while it is made. See `crate::plumbing`.
    match crate::plumbing::commit_paths(root, &rels, message, false, &[]) {
        Ok(_) => eprintln!("  committed: {message}"),
        Err(e) => {
            eprintln!("error: {e}");
            suggest_fallback(cfg, message);
            return;
        },
    }

    // The commit was built beside the real index, so the real one still holds
    // the old layout: `git status` would show the transition as staged renames
    // and deletions beside untracked files, and a check reading the index (the
    // pre-push one reads the locked changelist) would see it as modified. Bring
    // the paths the commit carried to the new HEAD. Path-limited rather than a
    // whole `git reset`, so anything somebody has staged elsewhere stays
    // staged. The working tree is never touched.
    if let Err(e) = crate::plumbing::sync_index_to_head(root, &rels) {
        eprintln!("warning: the commit landed but the index was not brought to it: {e}");
        eprintln!("  run: git reset -q");
    }
}

/// Print fallback manual commit instructions.
pub(crate) fn suggest_fallback(cfg: &Config, message: &str) {
    let dr_rel = pathdiff(&cfg.repo_root, &cfg.mock_dir.join("design_rounds"));
    eprintln!("  to commit manually:");
    eprintln!("    git add {dr_rel} && git commit -m \"{message}\"");
}

/// Get a relative path from base to target.
pub(crate) fn pathdiff(base: &Path, target: &Path) -> String {
    target
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| target.to_string_lossy().to_string())
}

pub(crate) fn git_head_sha(cfg: &Config) -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&cfg.repo_root)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "<unknown>".to_string())
}

pub(crate) fn git_round_log(cfg: &Config) -> String {
    // Get log since the round/*/start tag, or last 50 commits.
    Command::new("git")
        .args(["log", "--oneline", "-50"])
        .current_dir(&cfg.repo_root)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
}

pub(crate) fn chrono_date() -> String {
    // Simple date without chrono dependency.
    Command::new("date")
        .args(["+%Y-%m-%d"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
