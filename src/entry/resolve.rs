#![allow(unused_imports)]
use super::*;

pub(crate) fn resolve_mock_dir(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);

    // Absolute and exists: use directly.
    if path.is_absolute() && path.join("mockspace.toml").exists() {
        return path;
    }

    // Relative from CWD.
    if let Ok(canonical) = fs::canonicalize(&path) {
        if canonical.join("mockspace.toml").exists() {
            return canonical;
        }
    }

    // Relative from repo root (handles CWD != repo root with relative alias).
    if path.is_relative() {
        if let Some(root) = find_repo_root_from_cwd() {
            let from_root = root.join(&path);
            if from_root.join("mockspace.toml").exists() {
                return from_root;
            }
        }
    }

    // Nothing matched: return canonicalized or raw for a clear downstream error.
    fs::canonicalize(&path).unwrap_or(path)
}


/// Walk up from CWD looking for a `.git` directory (repo root).
pub(crate) fn find_repo_root_from_cwd() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}


/// Walk up from the current directory looking for mockspace.toml.
pub(crate) fn find_mockspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("mockspace.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// `cargo mock check`: readiness report.
//
// Non-mutating. Answers one question: "can I advance this round right
// now, or is something blocking?" Reports git cleanliness, remote
// sync, cargo-check status, lint status, and the phase-specific
// lock/close permission.
// ──────────────────────────────────────────────────────────────────────

