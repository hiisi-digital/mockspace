//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Locating the repo, the `mockspace.toml`, and the mock dir from any working
//! directory. All discovery uses absolute paths so cwd never matters, and it
//! is strictly read-only: nothing is moved or written during resolution.
//!
//! The launcher is deliberately workflow-schema-agnostic. The workflow schema
//! is not a separate axis it detects: it is a function of the pinned engine
//! version (bins below `0.2` run the v0.1 workflow, `0.2`+ run v0.2), so the
//! launcher just resolves the version, builds that engine, and runs it. The
//! engine binary inherently knows its own workflow; the launcher never
//! branches on it.

use std::path::{Path, PathBuf};

/// A located mockspace config and the mock dir it maps.
#[derive(Debug)]
pub struct Located {
    /// The `mockspace.toml` to read the pin from.
    pub config_path: PathBuf,
    /// The absolute v0.1 mock workspace dir the engine runs against.
    pub mock_dir:    PathBuf,
}

/// The repo root.
///
/// `MOCK_ROOT` (an absolute path) wins when set and pointing at a directory,
/// matching the engine's override. Otherwise the nearest ancestor of cwd
/// containing `.git`. `None` when neither resolves.
pub fn repo_root() -> Option<PathBuf> {
    repo_root_with(std::env::var_os("MOCK_ROOT"))
}

/// Pure core of [`repo_root`]: the `MOCK_ROOT` value is passed in so the
/// resolution is testable without mutating process env (cargo runs tests in
/// parallel threads, where `set_var` is a data race).
fn repo_root_with(mock_root: Option<std::ffi::OsString>) -> Option<PathBuf> {
    if let Some(r) = mock_root {
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

/// Resolve the one `mockspace.toml` and the mock dir it maps.
///
/// # Exactly one config per repo
///
/// A repo has **exactly one** `mockspace.toml`. It sits either at the repo root
/// (its home once relocated) or in a single immediate subdir (the historical
/// in-place location, e.g. `mock/`). There is no merging, no precedence, and no
/// "nearest wins": those semantics were never specified, so rather than pick one
/// silently, **more than one config anywhere in the repo (root plus immediate
/// subdirs) is a hard error** that blocks every `mock` run until exactly one
/// remains. This is `Err`. Zero configs is `Ok(None)` (the caller falls back to
/// the legacy `Cargo.lock` pin). One config is `Ok(Some)`.
///
/// The one config maps its mock dir via the `mock_dir` key: at the root it
/// defaults to `mock` (what almost every consumer uses); in a subdir it defaults
/// to `.` (the config's own dir is the mock workspace).
///
/// The scan covers the repo root and its immediate subdirs (hidden ones first),
/// skipping `.git` / `target` / `node_modules`. Deeper nesting is out of scope.
///
/// The engine's durable git hook reimplements this same resolution in shell
/// (`src/bootstrap/durable.rs`, the no-launcher fallback); the two MUST stay in
/// sync, including this single-config rule.
pub fn locate(root: &Path) -> Result<Option<Located>, String> {
    // Collect every (config, its-dir) in scope, so a second one is caught rather
    // than silently shadowed by a precedence order.
    let mut found: Vec<(PathBuf, PathBuf)> = Vec::new();
    let root_cfg = root.join("mockspace.toml");
    if root_cfg.is_file() {
        found.push((root_cfg, root.to_path_buf()));
    }
    for sub in ordered_subdirs(root) {
        let cfg = sub.join("mockspace.toml");
        if cfg.is_file() {
            found.push((cfg, sub));
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => {
            let (config_path, dir) = found.into_iter().next().unwrap();
            let default_md = if dir == root { "mock" } else { "." };
            let md = mock_dir_field(&config_path).unwrap_or_else(|| default_md.to_string());
            Ok(Some(Located {
                mock_dir: normalize(dir.join(md)),
                config_path,
            }))
        },
        _ => {
            let list = found
                .iter()
                .map(|(c, _)| format!("  {}", c.display()))
                .collect::<Vec<_>>()
                .join("\n");
            Err(format!(
                "found more than one mockspace.toml in this repo; a repo must have exactly \
                 one (at the repo root, or in a single subdir). Remove the extras, keep one:\n{list}"
            ))
        },
    }
}

/// Immediate subdirs of `root`, hidden (dotfile) ones first, each group sorted,
/// skipping dirs that never hold a mock workspace (`.git`, `target`,
/// `node_modules`).
fn ordered_subdirs(root: &Path) -> Vec<PathBuf> {
    let mut hidden = Vec::new();
    let mut plain = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    for e in rd.flatten() {
        if !e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | "target" | "node_modules") {
            continue;
        }
        if name.starts_with('.') {
            hidden.push((name, e.path()));
        } else {
            plain.push((name, e.path()));
        }
    }
    hidden.sort_by(|a, b| a.0.cmp(&b.0));
    plain.sort_by(|a, b| a.0.cmp(&b.0));
    hidden.into_iter().chain(plain).map(|(_, p)| p).collect()
}

/// The top-level `mock_dir = "..."` from a mockspace.toml, via the shared
/// manifest reader so the launcher and engine agree on the schema.
fn mock_dir_field(config_path: &Path) -> Option<String> {
    let toml = std::fs::read_to_string(config_path).ok()?;
    mockspace_manifest::ManifestHeader::parse(&toml).mock_dir()
}

/// Collapse a trailing `/.` (from the `.` mock_dir default) so paths stay tidy.
fn normalize(p: PathBuf) -> PathBuf {
    if p.file_name().map(|n| n == ".").unwrap_or(false) {
        return p.parent().map(Path::to_path_buf).unwrap_or(p);
    }
    p
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn mock_root_wins_when_a_dir() {
        let dir = tempfile::tempdir().unwrap();
        let got = repo_root_with(Some(dir.path().as_os_str().to_os_string()));
        assert_eq!(got.as_deref(), Some(dir.path()));
    }

    #[test]
    fn mock_root_ignored_when_not_a_dir() {
        // a bogus MOCK_ROOT falls through to the .git walk (this tree is a git
        // repo, so it resolves to something, just not the bogus path).
        let got = repo_root_with(Some("/definitely/not/a/dir/xyzzy".into()));
        assert_ne!(
            got.as_deref(),
            Some(Path::new("/definitely/not/a/dir/xyzzy"))
        );
    }

    #[test]
    fn locates_root_config_defaulting_mock() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("mockspace.toml"), "project_name = \"x\"\n").unwrap();
        let loc = locate(d.path()).unwrap().unwrap();
        assert_eq!(loc.config_path, d.path().join("mockspace.toml"));
        assert_eq!(loc.mock_dir, d.path().join("mock"));
    }

    #[test]
    fn root_config_explicit_mock_dir() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("mockspace.toml"), "mock_dir = \"design\"\n").unwrap();
        let loc = locate(d.path()).unwrap().unwrap();
        assert_eq!(loc.mock_dir, d.path().join("design"));
    }

    #[test]
    fn locates_subdir_config_defaulting_self() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("mock")).unwrap();
        fs::write(
            d.path().join("mock/mockspace.toml"),
            "project_name = \"x\"\n",
        )
        .unwrap();
        let loc = locate(d.path()).unwrap().unwrap();
        assert_eq!(loc.config_path, d.path().join("mock/mockspace.toml"));
        // subdir default is `.` -> the config's own dir is the mock dir
        assert_eq!(loc.mock_dir, d.path().join("mock"));
    }

    #[test]
    fn two_subdir_configs_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        for sub in ["mock", ".config"] {
            fs::create_dir(d.path().join(sub)).unwrap();
            fs::write(
                d.path().join(sub).join("mockspace.toml"),
                "project_name = \"x\"\n",
            )
            .unwrap();
        }
        // two configs is invalid: a hard error, not a precedence pick.
        assert!(locate(d.path()).is_err());
    }

    #[test]
    fn root_plus_subdir_configs_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("mockspace.toml"), "project_name = \"root\"\n").unwrap();
        fs::create_dir(d.path().join("mock")).unwrap();
        fs::write(
            d.path().join("mock/mockspace.toml"),
            "project_name = \"sub\"\n",
        )
        .unwrap();
        let err = locate(d.path()).unwrap_err();
        // the error names both offending paths.
        assert!(err.contains("mockspace.toml"));
        assert!(err.contains("mock"));
    }

    #[test]
    fn none_when_no_config() {
        let d = tempfile::tempdir().unwrap();
        assert!(locate(d.path()).unwrap().is_none());
    }
}
