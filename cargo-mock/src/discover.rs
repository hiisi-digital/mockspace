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

/// Resolve the `mockspace.toml` and the mock dir it maps, flexibly.
///
/// The repo root is checked first: a `mockspace.toml` there maps its mock dir
/// via the `mock_dir` key, defaulting to `mock` (what almost every consumer
/// uses). Otherwise the immediate subdirs are scanned, hidden ones (`.config`,
/// `.mockspace`, ...) first, and the first that holds a `mockspace.toml` wins;
/// there the `mock_dir` key defaults to `.`, so the config's own dir is the
/// mock workspace. This keeps every existing `mock/mockspace.toml` working
/// with no move, while a clean root placement points at `mock` explicitly.
///
/// The engine's durable git hook reimplements this same resolution in shell
/// (`src/bootstrap/durable.rs`, the no-launcher fallback); the two MUST stay in
/// sync.
pub fn locate(root: &Path) -> Option<Located> {
    let root_cfg = root.join("mockspace.toml");
    if root_cfg.is_file() {
        let md = mock_dir_field(&root_cfg).unwrap_or_else(|| "mock".to_string());
        return Some(Located {
            config_path: root_cfg,
            mock_dir:    normalize(root.join(md)),
        });
    }
    for sub in ordered_subdirs(root) {
        let cfg = sub.join("mockspace.toml");
        if cfg.is_file() {
            let md = mock_dir_field(&cfg).unwrap_or_else(|| ".".to_string());
            return Some(Located {
                config_path: cfg,
                mock_dir:    normalize(sub.join(md)),
            });
        }
    }
    None
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
        let loc = locate(d.path()).unwrap();
        assert_eq!(loc.config_path, d.path().join("mockspace.toml"));
        assert_eq!(loc.mock_dir, d.path().join("mock"));
    }

    #[test]
    fn root_config_explicit_mock_dir() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("mockspace.toml"), "mock_dir = \"design\"\n").unwrap();
        let loc = locate(d.path()).unwrap();
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
        let loc = locate(d.path()).unwrap();
        assert_eq!(loc.config_path, d.path().join("mock/mockspace.toml"));
        // subdir default is `.` -> the config's own dir is the mock dir
        assert_eq!(loc.mock_dir, d.path().join("mock"));
    }

    #[test]
    fn hidden_subdir_wins_over_plain() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("mock")).unwrap();
        fs::write(
            d.path().join("mock/mockspace.toml"),
            "project_name = \"p\"\n",
        )
        .unwrap();
        fs::create_dir(d.path().join(".config")).unwrap();
        fs::write(
            d.path().join(".config/mockspace.toml"),
            "project_name = \"h\"\n",
        )
        .unwrap();
        let loc = locate(d.path()).unwrap();
        assert_eq!(loc.config_path, d.path().join(".config/mockspace.toml"));
        assert_eq!(loc.mock_dir, d.path().join(".config"));
    }

    #[test]
    fn root_wins_over_subdir() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("mockspace.toml"), "project_name = \"root\"\n").unwrap();
        fs::create_dir(d.path().join("mock")).unwrap();
        fs::write(
            d.path().join("mock/mockspace.toml"),
            "project_name = \"sub\"\n",
        )
        .unwrap();
        let loc = locate(d.path()).unwrap();
        assert_eq!(loc.config_path, d.path().join("mockspace.toml"));
        assert_eq!(loc.mock_dir, d.path().join("mock"));
    }

    #[test]
    fn none_when_no_config() {
        let d = tempfile::tempdir().unwrap();
        assert!(locate(d.path()).is_none());
    }
}
