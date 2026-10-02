//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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
    find_mockspace_root_from(&std::env::current_dir().ok()?)
}

/// Why no project was found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NoProject {
    /// `--dir` was given with nothing after it.
    DirWithoutValue,
    /// No `--dir`, no `mockspace.toml` upward, no `design_rounds/` here.
    NotFound,
}

/// The mock directory this invocation acts on, the one way every caller finds
/// it: `--dir <path>`, then a `mockspace.toml` upward from `cwd`, then `cwd`
/// itself where it holds a `design_rounds/`.
///
/// One function because help asks the same question before deciding whether a
/// name can be described from a pack, and two copies of the answer disagreed:
/// help counted a bare `--dir` as a project that the dispatcher then refused.
///
/// The last arm is `design_rounds/` rather than `crates/` because mockspace
/// creates that directory and nothing else does, so a directory holding one is
/// a mock directory whatever language the project is written in. Asking about
/// `crates/` said yes to any rust project that had never adopted mockspace.
pub(crate) fn discover_mock_dir(args: &[String], cwd: &Path) -> Result<PathBuf, NoProject> {
    if let Some(pos) = args.iter().position(|a| a == "--dir") {
        return match args.get(pos + 1) {
            Some(p) => Ok(resolve_mock_dir(p)),
            None => Err(NoProject::DirWithoutValue),
        };
    }
    if let Some(dir) = find_mockspace_root_from(cwd) {
        return Ok(dir);
    }
    if cwd.join("design_rounds").is_dir() {
        return Ok(cwd.to_path_buf());
    }
    Err(NoProject::NotFound)
}

/// Walk up from `start` looking for mockspace.toml.
fn find_mockspace_root_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join("mockspace.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn each_way_of_finding_a_project_and_the_two_ways_of_not() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        assert_eq!(
            discover_mock_dir(&args(&["mock"]), &root),
            Err(NoProject::NotFound)
        );
        assert_eq!(
            discover_mock_dir(&args(&["mock", "--dir"]), &root),
            Err(NoProject::DirWithoutValue)
        );

        fs::create_dir_all(root.join("design_rounds")).unwrap();
        assert_eq!(discover_mock_dir(&args(&["mock"]), &root), Ok(root.clone()));

        let nested = root.join("a/b");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("a/mockspace.toml"), "").unwrap();
        assert_eq!(
            discover_mock_dir(&args(&["mock"]), &nested),
            Ok(root.join("a"))
        );
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
