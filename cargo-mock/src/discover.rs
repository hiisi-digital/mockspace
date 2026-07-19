//! Locating the repo and the mock subdir from any working directory. All
//! discovery uses absolute paths so cwd never matters.
//!
//! The launcher is deliberately workflow-schema-agnostic. The workflow schema
//! is not a separate axis it detects: it is a function of the pinned engine
//! version (bins below `0.2` run the v0.1 workflow, `0.2`+ run v0.2), so the
//! launcher just resolves the version, builds that engine, and runs it. The
//! engine binary inherently knows its own workflow; the launcher never
//! branches on it.

use std::path::{Path, PathBuf};

/// The repo root.
///
/// `MOCK_ROOT` (an absolute path) wins when set and pointing at a directory,
/// matching the engine's override. Otherwise the nearest ancestor of cwd
/// containing `.git`. `None` when neither resolves.
pub fn repo_root() -> Option<PathBuf> {
    if let Some(r) = std::env::var_os("MOCK_ROOT") {
        let p = PathBuf::from(r);
        if p.is_dir() {
            return Some(p);
        }
    }
    let mut d = std::env::current_dir().ok()?;
    loop {
        if d.join(".git").exists() {
            return Some(d);
        }
        if !d.pop() {
            return None;
        }
    }
}

/// The v0.1 filesystem mock-dir *name*, mapped by the root `mockspace.toml`
/// `mock_dir` key, defaulting to `mock` (what almost every consumer uses).
///
/// v0.2 keeps its round state in git refs, so there is no mock dir; the field
/// is a v0.1 concern only, and the default keeps existing repos working
/// without stating it.
pub fn mock_dir_name(root: &Path) -> String {
    if let Ok(s) = std::fs::read_to_string(root.join("mockspace.toml")) {
        if let Some(name) = top_level_string(&s, "mock_dir") {
            if !name.is_empty() {
                return name;
            }
        }
    }
    "mock".to_string()
}

/// A top-level (section-less) `key = "value"` string from a mockspace.toml.
fn top_level_string(toml: &str, key: &str) -> Option<String> {
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            break; // top-level keys precede the first table
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == key {
                return Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_root_env_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("MOCK_ROOT", dir.path());
        let got = repo_root();
        std::env::remove_var("MOCK_ROOT");
        assert_eq!(got.as_deref(), Some(dir.path()));
    }

    #[test]
    fn mock_root_ignored_when_not_a_dir() {
        std::env::set_var("MOCK_ROOT", "/definitely/not/a/real/dir/xyzzy");
        let got = repo_root();
        std::env::remove_var("MOCK_ROOT");
        // falls through to the .git walk (this test tree is a git repo, so it
        // resolves to *something*, just not the bogus MOCK_ROOT).
        assert_ne!(
            got.as_deref(),
            Some(Path::new("/definitely/not/a/real/dir/xyzzy"))
        );
    }
}
