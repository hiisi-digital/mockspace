#![allow(unused_imports)]
use super::*;

/// Either surgically commit the exact files we touched, or print a suggestion.
///
/// When `opts.auto_commit` is true, uses `git commit --only -- <paths>`
/// to commit ONLY the specified files. This ignores any previously staged
/// changes (they remain staged after the commit) and any other unstaged
/// modifications. No stash needed.
///
/// `--only` works by:
///   1. Temporarily resetting the index for the named paths
///   2. Staging their current working-tree state (including deletions)
///   3. Creating the commit from that temporary index
///   4. Merging the temporary index back with the original
///
/// The result: only our files are committed; everything else is untouched.
pub(crate) fn commit_or_suggest(cfg: &Config, opts: &SubcmdOpts, files: &[PathBuf], message: &str) {
    if !opts.auto_commit {
        let dr_rel = pathdiff(&cfg.repo_root, &cfg.mock_dir.join("design_rounds"));
        eprintln!();
        eprintln!("  to commit:");
        eprintln!("    git add {dr_rel} && git commit -m \"{message}\"");
        return;
    }

    let root = &cfg.repo_root;

    // Use a temporary index to create a surgical commit.
    // This never touches the real index, so all existing staged changes
    // are preserved exactly as they were.
    //
    // Steps:
    //   1. Create temp index from HEAD tree
    //   2. Update temp index with our files (adds + removes)
    //   3. Write tree from temp index
    //   4. Create commit object pointing to that tree
    //   5. Update HEAD ref
    //   6. Clean up temp index
    //
    // The real .git/index is never read or modified.
    let git_dir = root.join(".git");
    let tmp_index = git_dir.join("tmp_mockspace_index");

    // Clean up any stale temp index.
    let _ = fs::remove_file(&tmp_index);

    let tmp_idx_str = tmp_index.to_string_lossy().to_string();

    // 1. Populate temp index from HEAD.
    if git_env_run(root, &["read-tree", "HEAD"], &tmp_idx_str).is_err() {
        eprintln!("error: git read-tree HEAD failed");
        suggest_fallback(cfg, message);
        return;
    }

    // 2. Update temp index with our changes.
    for f in files {
        let rel = pathdiff(root, f);
        if f.exists() {
            // Add or update file in temp index.
            if git_env_run(root, &["update-index", "--add", &rel], &tmp_idx_str).is_err() {
                eprintln!("warning: failed to add {rel} to temp index");
            }
        } else {
            // Remove deleted file from temp index (ignore if not present).
            let _ = git_env_run(root, &["update-index", "--remove", &rel], &tmp_idx_str);
        }
    }

    // 3. Write tree.
    let tree_sha = match git_env_ok(root, &["write-tree"], &tmp_idx_str) {
        Ok(sha) => sha.trim().to_string(),
        Err(e) => {
            eprintln!("error: git write-tree failed: {e}");
            let _ = fs::remove_file(&tmp_index);
            suggest_fallback(cfg, message);
            return;
        }
    };

    // 4. Get current HEAD sha.
    let head_sha = match git_ok(root, &["rev-parse", "HEAD"]) {
        Ok(sha) => sha.trim().to_string(),
        Err(e) => {
            eprintln!("error: git rev-parse HEAD failed: {e}");
            let _ = fs::remove_file(&tmp_index);
            suggest_fallback(cfg, message);
            return;
        }
    };

    // 5. Create commit object.
    let commit_sha = match Command::new("git")
        .args(["commit-tree", &tree_sha, "-p", &head_sha, "-m", message])
        .current_dir(root)
        .output()
    {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        Ok(o) => {
            eprintln!("error: git commit-tree failed: {}", String::from_utf8_lossy(&o.stderr));
            let _ = fs::remove_file(&tmp_index);
            suggest_fallback(cfg, message);
            return;
        }
        Err(e) => {
            eprintln!("error: git commit-tree failed: {e}");
            let _ = fs::remove_file(&tmp_index);
            suggest_fallback(cfg, message);
            return;
        }
    };

    // 6. Update HEAD to point to new commit.
    let branch = git_ok(root, &["symbolic-ref", "--short", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "HEAD".to_string());

    let ref_name = if branch == "HEAD" {
        "HEAD".to_string()
    } else {
        format!("refs/heads/{branch}")
    };

    if git_run(root, &["update-ref", &ref_name, &commit_sha]).is_err() {
        eprintln!("error: git update-ref failed");
        eprintln!("  commit object created: {commit_sha}");
        eprintln!("  run: git update-ref {ref_name} {commit_sha}");
    } else {
        eprintln!("  committed: {message}");
    }

    // 7. Clean up.
    let _ = fs::remove_file(&tmp_index);
}


/// Print fallback manual commit instructions.
pub(crate) fn suggest_fallback(cfg: &Config, message: &str) {
    let dr_rel = pathdiff(&cfg.repo_root, &cfg.mock_dir.join("design_rounds"));
    eprintln!("  to commit manually:");
    eprintln!("    git add {dr_rel} && git commit -m \"{message}\"");
}


/// Run a git command and return Ok(stdout) or Err(stderr).
pub(crate) fn git_ok(root: &Path, args: &[&str]) -> Result<String, String> {
    Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).map_err(|e| e.to_string())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).to_string())
            }
        })
}


/// Run a git command; returns Ok(()) on success, Err(stderr) on failure.
pub(crate) fn git_run(root: &Path, args: &[&str]) -> Result<(), String> {
    git_ok(root, args).map(|_| ())
}


/// Run a git command with a custom GIT_INDEX_FILE; returns Ok(stdout).
pub(crate) fn git_env_ok(root: &Path, args: &[&str], index_file: &str) -> Result<String, String> {
    Command::new("git")
        .args(args)
        .env("GIT_INDEX_FILE", index_file)
        .current_dir(root)
        .output()
        .map_err(|e| e.to_string())
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).map_err(|e| e.to_string())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).to_string())
            }
        })
}


/// Run a git command with a custom GIT_INDEX_FILE; returns Ok(()).
pub(crate) fn git_env_run(root: &Path, args: &[&str], index_file: &str) -> Result<(), String> {
    git_env_ok(root, args, index_file).map(|_| ())
}


/// Get a relative path from base to target.
pub(crate) fn pathdiff(base: &Path, target: &Path) -> String {
    target.strip_prefix(base)
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

