//! Bootstrap surface for the v2 mockspace tooling.
//!
//! Per the v2 spec §57 the bootstrap consumes the invocation resolver
//! ([`crate::invoke`]) and manages two pieces of consumer-repo state:
//!
//! 1. The `[alias] mock = "..."` entry in `.cargo/config.toml`, which
//!    is what makes `cargo mock <subcommand>` resolve. This is what
//!    step 4 of the resolution chain probes for.
//! 2. The git hook scripts under `mock/target/hooks/` (pre-commit,
//!    pre-push) plus `core.hooksPath` pointing at them.
//!
//! The bootstrap surface is split into four operations:
//!
//! - `install`: write fresh state for a repo opting in.
//! - `refresh`: re-derive state if any input has drifted.
//! - `uninstall`: tear state down for a repo opting out.
//! - `status`: read-only diagnostic, reports the observed adoption
//!   state without mutating anything.
//!
//! This module lands the `status` half first. The mutating operations
//! follow as separate slices; their state-machine intents are
//! reconciled against the [`AdoptionStatus`] this module returns, so
//! shipping `status` first locks the diagnostic surface those
//! operations will agree with.

use std::path::Path;

/// Read-only summary of how thoroughly the v2 bootstrap has been
/// applied in a consumer repo. Returned by [`status`]; consumed by
/// the CLI's `cargo mock status` subcommand (#560) and by the
/// workspace-level adoption-drift gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdoptionStatus {
    /// Does a `mock/` directory exist at the repo root? This is the
    /// minimum signal that the consumer has opted into the v2
    /// workflow at all.
    pub has_mock_dir: bool,
    /// Does a `.cargo/config.toml` (at the repo root) carry a
    /// `[alias] mock = ...` line? The bootstrap installs this so
    /// `cargo mock <subcommand>` resolves.
    pub has_cargo_alias: bool,
    /// Does `core.hooksPath` (in `.git/config`) point at a path
    /// under `mock/target/hooks/`? The bootstrap writes this so
    /// git's commit and push gates fire mockspace's hook scripts.
    pub has_hooks_path: bool,
}

impl AdoptionStatus {
    /// All three signals present: the repo is fully opted in.
    pub fn is_fully_adopted(&self) -> bool {
        self.has_mock_dir && self.has_cargo_alias && self.has_hooks_path
    }

    /// None of the signals present: the repo has not opted in at
    /// all. Workspace gates treat this as silent allow rather than
    /// drift.
    pub fn is_uninstalled(&self) -> bool {
        !self.has_mock_dir && !self.has_cargo_alias && !self.has_hooks_path
    }

    /// At least one signal but not all three: the repo's adoption
    /// has drifted out of step. Workspace gates surface this as a
    /// structured deny so the user notices.
    pub fn is_partial(&self) -> bool {
        !self.is_fully_adopted() && !self.is_uninstalled()
    }
}

/// Inspect `repo_root` and return the observed adoption signals.
/// Pure read; never mutates the filesystem or git config. The three
/// signals are derived independently so a partial-adoption state is
/// detectable as such (rather than being silently rounded to "not
/// installed" or "installed").
pub fn status(repo_root: &Path) -> AdoptionStatus {
    AdoptionStatus {
        has_mock_dir: has_mock_dir(repo_root),
        has_cargo_alias: has_cargo_alias(repo_root),
        has_hooks_path: has_hooks_path(repo_root),
    }
}

fn has_mock_dir(repo_root: &Path) -> bool {
    repo_root.join("mock").is_dir()
}

/// Returns true if `<repo_root>/.cargo/config.toml` contains an
/// `[alias]` table with a `mock = ...` key. The check is structural
/// (parses the TOML) rather than substring-based so `# mock = ...`
/// in a comment does not trip a false positive.
fn has_cargo_alias(repo_root: &Path) -> bool {
    let path = repo_root.join(".cargo").join("config.toml");
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(parsed) = contents.parse::<toml::Table>() else {
        return false;
    };
    parsed
        .get("alias")
        .and_then(|v| v.as_table())
        .is_some_and(|t| t.contains_key("mock"))
}

/// Returns true if `<repo_root>/.git/config` has a `core.hooksPath`
/// entry that resolves to a directory under `<repo_root>/mock/target/hooks/`.
/// Reads `.git/config` as a flat INI-style file rather than calling
/// out to `git config --get`; this keeps the check standalone (no
/// subprocess) and works regardless of which git binary is on PATH.
fn has_hooks_path(repo_root: &Path) -> bool {
    let git_config = repo_root.join(".git").join("config");
    let Ok(contents) = std::fs::read_to_string(&git_config) else {
        return false;
    };
    // INI parsing: find a `[core]` section, then a `hooksPath = ...`
    // line within it. The git config syntax is more permissive than
    // strict INI (allows subsection headers, escaping, etc.) but
    // for this single key the flat lookup suffices.
    let mut in_core = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_core = line == "[core]";
            continue;
        }
        if !in_core {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "hooksPath" {
            continue;
        }
        // Strip whitespace and surrounding quotes: git config permits
        // either `hooksPath = mock/target/hooks` or the quoted form
        // `hooksPath = "mock/target/hooks"`, and the bootstrap may
        // emit either depending on path content.
        let value = value.trim().trim_matches('"');
        // The hooksPath value can be relative to the repo root (the
        // bootstrap writes the relative form) or absolute. Resolve
        // and check that the path is under `mock/target/`, which is
        // the canonical bootstrap output. The exact subdirectory
        // can vary across cargo profiles (debug / release / custom)
        // so a prefix match against `mock/target/` rather than the
        // full `mock/target/hooks/` accepts the legitimate variation
        // without false-positive matching on unrelated configurations.
        let resolved = if Path::new(value).is_absolute() {
            std::path::PathBuf::from(value)
        } else {
            repo_root.join(value)
        };
        return resolved.starts_with(repo_root.join("mock").join("target"));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "mockspace-bootstrap-status-{}-{}-{:?}",
            name,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("mkdir fixture root");
        path
    }

    fn cleanup(p: &Path) {
        let _ = std::fs::remove_dir_all(p);
    }

    fn write_mock_dir(root: &Path) {
        std::fs::create_dir_all(root.join("mock")).expect("mkdir mock");
    }

    fn write_cargo_alias(root: &Path) {
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[alias]\nmock = \"run --manifest-path mock/Cargo.toml --bin mock --\"\n",
        )
        .expect("write cargo config");
    }

    fn write_git_hooks_path(root: &Path) {
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        std::fs::write(
            git_dir.join("config"),
            "[core]\n\thooksPath = mock/target/hooks\n",
        )
        .expect("write git config");
        // Also create the hooks dir so the path resolves to an
        // existing location (not strictly required by the check,
        // but mirrors realistic state).
        std::fs::create_dir_all(root.join("mock").join("target").join("hooks"))
            .expect("mkdir hooks");
    }

    #[test]
    fn empty_repo_reports_zero_adoption() {
        let root = fixture_dir("empty");
        let s = status(&root);
        cleanup(&root);
        assert!(!s.has_mock_dir);
        assert!(!s.has_cargo_alias);
        assert!(!s.has_hooks_path);
        assert!(s.is_uninstalled());
        assert!(!s.is_fully_adopted());
        assert!(!s.is_partial());
    }

    #[test]
    fn fully_adopted_repo_reports_all_three_signals() {
        let root = fixture_dir("full");
        write_mock_dir(&root);
        write_cargo_alias(&root);
        write_git_hooks_path(&root);
        let s = status(&root);
        cleanup(&root);
        assert!(s.has_mock_dir);
        assert!(s.has_cargo_alias);
        assert!(s.has_hooks_path);
        assert!(s.is_fully_adopted());
        assert!(!s.is_uninstalled());
        assert!(!s.is_partial());
    }

    #[test]
    fn mock_dir_only_reports_partial_adoption() {
        let root = fixture_dir("partial-mock-only");
        write_mock_dir(&root);
        let s = status(&root);
        cleanup(&root);
        assert!(s.has_mock_dir);
        assert!(!s.has_cargo_alias);
        assert!(!s.has_hooks_path);
        assert!(s.is_partial());
    }

    #[test]
    fn cargo_alias_only_reports_partial_adoption() {
        let root = fixture_dir("partial-alias-only");
        write_cargo_alias(&root);
        let s = status(&root);
        cleanup(&root);
        assert!(!s.has_mock_dir);
        assert!(s.has_cargo_alias);
        assert!(!s.has_hooks_path);
        assert!(s.is_partial());
    }

    #[test]
    fn cargo_alias_check_is_structural_not_substring() {
        // A comment with "mock = ..." text should not trip the
        // check; the parser sees it as a comment and the `[alias]`
        // table is empty.
        let root = fixture_dir("commented-alias");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[alias]\n# mock = \"run something\"\n",
        )
        .expect("write cargo config");
        let s = status(&root);
        cleanup(&root);
        assert!(
            !s.has_cargo_alias,
            "commented-out mock alias should not match"
        );
    }

    #[test]
    fn unparseable_cargo_toml_reports_no_alias() {
        let root = fixture_dir("bad-cargo-toml");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(dir.join("config.toml"), "this is not toml at all").expect("write bad");
        let s = status(&root);
        cleanup(&root);
        assert!(!s.has_cargo_alias);
    }

    #[test]
    fn hooks_path_pointing_elsewhere_reports_no_hooks() {
        let root = fixture_dir("hooks-elsewhere");
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        std::fs::write(
            git_dir.join("config"),
            "[core]\n\thooksPath = /tmp/some/other/place\n",
        )
        .expect("write git config");
        let s = status(&root);
        cleanup(&root);
        assert!(!s.has_hooks_path);
    }

    #[test]
    fn hooks_path_with_unrelated_mock_component_does_not_match() {
        // A path containing a `mock` or `hooks` component but not
        // under the repo's mock/target subtree should not match.
        // This guards against the false-positive surface a loose
        // containment check would open.
        let root = fixture_dir("hooks-unrelated-mock");
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        std::fs::write(
            git_dir.join("config"),
            "[core]\n\thooksPath = /opt/mock-things/team-hooks\n",
        )
        .expect("write git config");
        let s = status(&root);
        cleanup(&root);
        assert!(
            !s.has_hooks_path,
            "unrelated path containing 'mock' or 'hooks' should not match"
        );
    }

    #[test]
    fn hooks_path_quoted_value_is_accepted() {
        // git config permits `hooksPath = "mock/target/hooks"`
        // (quoted). The check should treat it identically to the
        // unquoted form.
        let root = fixture_dir("hooks-quoted");
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        std::fs::write(
            git_dir.join("config"),
            "[core]\n\thooksPath = \"mock/target/hooks\"\n",
        )
        .expect("write git config");
        std::fs::create_dir_all(root.join("mock").join("target").join("hooks"))
            .expect("mkdir hooks");
        let s = status(&root);
        cleanup(&root);
        assert!(s.has_hooks_path, "quoted hooksPath should be accepted");
    }

    #[test]
    fn hooks_path_under_release_profile_matches() {
        // Cargo profiles can put target output under `target/release/`
        // or `target/debug/`. The check should accept hooksPath
        // anywhere under `mock/target/` rather than requiring the
        // literal `mock/target/hooks/` prefix.
        let root = fixture_dir("hooks-release-profile");
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        std::fs::write(
            git_dir.join("config"),
            "[core]\n\thooksPath = mock/target/release/hooks\n",
        )
        .expect("write git config");
        let s = status(&root);
        cleanup(&root);
        assert!(s.has_hooks_path, "release-profile hooksPath should match");
    }

    #[test]
    fn hooks_path_outside_core_section_is_ignored() {
        let root = fixture_dir("hooks-wrong-section");
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        std::fs::write(
            git_dir.join("config"),
            "[user]\n\thooksPath = mock/target/hooks\n",
        )
        .expect("write git config");
        let s = status(&root);
        cleanup(&root);
        assert!(
            !s.has_hooks_path,
            "hooksPath outside [core] should not match"
        );
    }

    #[test]
    fn is_fully_adopted_and_is_uninstalled_are_disjoint() {
        // Exhaustive 2^3 = 8 combinations of the three signals.
        for mock in [false, true] {
            for alias in [false, true] {
                for hooks in [false, true] {
                    let s = AdoptionStatus {
                        has_mock_dir: mock,
                        has_cargo_alias: alias,
                        has_hooks_path: hooks,
                    };
                    let n = s.is_fully_adopted() as u8
                        + s.is_uninstalled() as u8
                        + s.is_partial() as u8;
                    assert_eq!(
                        n, 1,
                        "exactly one of the three predicates should hold; mock={mock} alias={alias} hooks={hooks}"
                    );
                }
            }
        }
    }
}
