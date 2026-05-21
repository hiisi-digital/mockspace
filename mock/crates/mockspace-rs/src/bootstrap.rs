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

/// The cargo alias value the bootstrap installs. Quoted as a TOML
/// string when written into `.cargo/config.toml`. Configured to run
/// the mock binary out of the workspace `mock/` crate at the workspace
/// root; consumers who want a different invocation shape (precompiled
/// binary, alternate manifest path) edit the value after install.
const CARGO_ALIAS_VALUE: &str = "run --manifest-path mock/Cargo.toml --bin mock --";

/// Failure modes for [`install_cargo_alias`]. Each variant carries
/// enough context for the CLI surface to point the user at the
/// offending file or condition without needing to repeat the work.
#[derive(Debug)]
pub enum InstallError {
    /// `<repo_root>/.cargo/config.toml` exists but is not parseable
    /// as TOML. Mutating it would risk losing unrelated keys the
    /// user has hand-edited; the bootstrap bails out and asks the
    /// user to repair the file first. The variant carries the
    /// underlying parse error.
    UnparseableCargoConfig(toml::de::Error),
    /// A filesystem operation (read / write / create_dir_all) failed.
    /// Wraps the underlying `io::Error` so the caller can inspect
    /// the kind (NotFound, PermissionDenied, etc.).
    Io(std::io::Error),
    /// `[alias] mock = ...` already exists but points at a value
    /// other than the canonical `CARGO_ALIAS_VALUE`. The bootstrap
    /// leaves the user's value alone rather than silently
    /// overwriting; the CLI surface prompts the user to confirm
    /// the overwrite explicitly. The variant carries the existing
    /// value so the diagnostic can quote it.
    AliasMismatch {
        existing: String,
    },
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnparseableCargoConfig(e) => write!(
                f,
                ".cargo/config.toml is not parseable as TOML; repair the file before re-running bootstrap. Underlying error: {e}"
            ),
            Self::Io(e) => write!(f, "filesystem operation failed: {e}"),
            Self::AliasMismatch { existing } => write!(
                f,
                "`[alias] mock` already exists with a different value: {existing:?}. Refusing to overwrite; remove the existing entry or re-run with the overwrite flag once the CLI surface ships it"
            ),
        }
    }
}

impl std::error::Error for InstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnparseableCargoConfig(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::AliasMismatch { .. } => None,
        }
    }
}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Result of [`install_cargo_alias`]: whether the bootstrap actually
/// changed the file. Distinguishes the no-op case (alias already
/// present and matches) from the genuinely-installed case so the CLI
/// surface can print "ok, no change" vs "wrote cargo alias".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The alias was missing; the bootstrap added it.
    Installed,
    /// The alias was already present with the canonical value; the
    /// bootstrap made no changes.
    AlreadyInstalled,
}

/// Install (or verify) the `[alias] mock = ...` entry in
/// `<repo_root>/.cargo/config.toml`. Preserves every other key in
/// the file; this is the operation that makes step 4 of the
/// invocation resolution chain ([`crate::invoke`]) succeed.
///
/// Behaviour matrix:
///
/// - File does not exist: create it with a sole `[alias]` table
///   carrying the canonical `mock` entry.
/// - File exists, no `[alias]` table: append the table and entry.
/// - File exists, `[alias]` table present, no `mock` key: add the
///   `mock` key to the existing table.
/// - File exists, `mock` key present, matching canonical value:
///   no-op; returns [`InstallOutcome::AlreadyInstalled`].
/// - File exists, `mock` key present, value differs: refuse to
///   overwrite; returns [`InstallError::AliasMismatch`] carrying
///   the existing value so the CLI surface can quote it.
/// - File exists but is unparseable: refuse to mutate; returns
///   [`InstallError::UnparseableCargoConfig`] so the user can
///   repair the file before re-running.
///
/// The serialised output uses `toml::to_string_pretty` so the
/// emitted file is diff-friendly. Existing whitespace and comments
/// are NOT preserved; the file gets a clean structural rewrite
/// when this function mutates it. Consumers who hand-edited
/// comments into the file should pin the comments via a comment
/// in their workspace docs rather than relying on round-trip
/// preservation.
pub fn install_cargo_alias(repo_root: &Path) -> Result<InstallOutcome, InstallError> {
    let dir = repo_root.join(".cargo");
    let path = dir.join("config.toml");
    let mut doc = match std::fs::read_to_string(&path) {
        Ok(contents) => contents
            .parse::<toml::Table>()
            .map_err(InstallError::UnparseableCargoConfig)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(InstallError::Io(e)),
    };

    // Find or insert the [alias] table without disturbing other
    // top-level keys.
    let alias_table = doc
        .entry("alias".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let alias_table = match alias_table {
        toml::Value::Table(t) => t,
        // Existing `alias` key is not a table. Surface this as an
        // unparseable shape rather than overwriting the user's
        // (admittedly broken) data.
        other => {
            return Err(InstallError::AliasMismatch {
                existing: other.to_string(),
            });
        }
    };

    if let Some(existing) = alias_table.get("mock") {
        let existing_str = match existing {
            toml::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if existing_str == CARGO_ALIAS_VALUE {
            return Ok(InstallOutcome::AlreadyInstalled);
        }
        return Err(InstallError::AliasMismatch {
            existing: existing_str,
        });
    }

    alias_table.insert(
        "mock".to_string(),
        toml::Value::String(CARGO_ALIAS_VALUE.to_string()),
    );

    std::fs::create_dir_all(&dir)?;
    let serialised = toml::to_string_pretty(&doc).expect("Table serialisation is infallible");
    std::fs::write(&path, serialised)?;
    Ok(InstallOutcome::Installed)
}

/// Result of [`uninstall_cargo_alias`]: whether the bootstrap actually
/// removed anything. Mirrors [`InstallOutcome`] but with the inverse
/// polarity: `Removed` when the mock entry was present and is now
/// gone; `AlreadyUninstalled` when the entry was already absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallOutcome {
    /// The mock entry was present; the bootstrap removed it.
    Removed,
    /// The mock entry was already absent; the bootstrap made no
    /// changes.
    AlreadyUninstalled,
}

/// Remove the `[alias] mock = ...` entry from
/// `<repo_root>/.cargo/config.toml`. Symmetric counterpart to
/// [`install_cargo_alias`]. Preserves every other key in the file.
///
/// Behaviour matrix:
///
/// - File does not exist: no-op; returns
///   [`UninstallOutcome::AlreadyUninstalled`].
/// - File exists, no `[alias]` table: no-op; returns
///   `AlreadyUninstalled`.
/// - File exists, `[alias]` table present, no `mock` key: no-op;
///   returns `AlreadyUninstalled`.
/// - File exists, `mock` key present: remove the `mock` key.
///   If the `[alias]` table becomes empty as a result, remove it
///   too (matches the bootstrap's clean-slate-by-default
///   discipline). If the file then becomes empty, leave the
///   file in place as an empty TOML document; deleting the file
///   would be overreach (the user may have intended `.cargo/config.toml`
///   to exist for ambient reasons). Returns `Removed`.
/// - File exists but unparseable: refuse to mutate; returns
///   [`InstallError::UnparseableCargoConfig`].
/// - `[alias]` exists as a non-table scalar: refuse to mutate;
///   returns `InstallError::AliasMismatch` with the existing value.
pub fn uninstall_cargo_alias(repo_root: &Path) -> Result<UninstallOutcome, InstallError> {
    let dir = repo_root.join(".cargo");
    let path = dir.join("config.toml");
    let mut doc = match std::fs::read_to_string(&path) {
        Ok(contents) => contents
            .parse::<toml::Table>()
            .map_err(InstallError::UnparseableCargoConfig)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UninstallOutcome::AlreadyUninstalled);
        }
        Err(e) => return Err(InstallError::Io(e)),
    };

    let Some(alias_value) = doc.get_mut("alias") else {
        return Ok(UninstallOutcome::AlreadyUninstalled);
    };
    let alias_table = match alias_value {
        toml::Value::Table(t) => t,
        // alias = "scalar": same defensive refusal as install.
        // Do not clobber broken-but-present user data.
        other => {
            return Err(InstallError::AliasMismatch {
                existing: other.to_string(),
            });
        }
    };

    if alias_table.remove("mock").is_none() {
        return Ok(UninstallOutcome::AlreadyUninstalled);
    }

    if alias_table.is_empty() {
        doc.remove("alias");
    }

    let serialised = toml::to_string_pretty(&doc).expect("Table serialisation is infallible");
    std::fs::write(&path, serialised)?;
    Ok(UninstallOutcome::Removed)
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

    // ---- install_cargo_alias --------------------------------------------

    #[test]
    fn install_cargo_alias_creates_file_when_missing() {
        let root = fixture_dir("install-create");
        let outcome = install_cargo_alias(&root).expect("install");
        assert_eq!(outcome, InstallOutcome::Installed);
        // Verify via the structural status check that the alias is
        // observable through the same path the diagnostic surface
        // reads.
        assert!(status(&root).has_cargo_alias);
        cleanup(&root);
    }

    #[test]
    fn install_cargo_alias_appends_to_existing_table_without_mock_key() {
        let root = fixture_dir("install-append-table");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[alias]\nother = \"build --release\"\n",
        )
        .expect("write existing");
        let outcome = install_cargo_alias(&root).expect("install");
        assert_eq!(outcome, InstallOutcome::Installed);
        let contents = std::fs::read_to_string(dir.join("config.toml")).expect("read back");
        // Both keys must survive.
        assert!(contents.contains("mock = "));
        assert!(contents.contains("other = "));
        cleanup(&root);
    }

    #[test]
    fn install_cargo_alias_preserves_unrelated_top_level_keys() {
        let root = fixture_dir("install-preserve");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[build]\ntarget = \"x86_64-unknown-linux-gnu\"\n",
        )
        .expect("write existing");
        install_cargo_alias(&root).expect("install");
        let parsed: toml::Table =
            std::fs::read_to_string(dir.join("config.toml")).unwrap().parse().unwrap();
        // [build] survives the install.
        assert!(parsed.get("build").and_then(|v| v.as_table()).is_some());
        // [alias.mock] now exists.
        assert!(
            parsed
                .get("alias")
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("mock"))
                .is_some()
        );
        cleanup(&root);
    }

    #[test]
    fn install_cargo_alias_is_idempotent_when_value_matches() {
        let root = fixture_dir("install-idempotent");
        install_cargo_alias(&root).expect("first install");
        let outcome = install_cargo_alias(&root).expect("second install");
        assert_eq!(outcome, InstallOutcome::AlreadyInstalled);
        cleanup(&root);
    }

    #[test]
    fn install_cargo_alias_refuses_to_overwrite_mismatched_value() {
        let root = fixture_dir("install-mismatch");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[alias]\nmock = \"some-other-thing\"\n",
        )
        .expect("write existing");
        let observed = install_cargo_alias(&root);
        cleanup(&root);
        match observed {
            Err(InstallError::AliasMismatch { existing }) => {
                assert_eq!(existing, "some-other-thing");
            }
            other => panic!("expected AliasMismatch, got {other:?}"),
        }
    }

    #[test]
    fn install_cargo_alias_surfaces_unparseable_toml() {
        let root = fixture_dir("install-bad-toml");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(dir.join("config.toml"), "<<< not toml >>>").expect("write bad");
        let observed = install_cargo_alias(&root);
        cleanup(&root);
        match observed {
            Err(InstallError::UnparseableCargoConfig(_)) => {}
            other => panic!("expected UnparseableCargoConfig, got {other:?}"),
        }
    }

    #[test]
    fn install_cargo_alias_handles_alias_as_non_table_shape() {
        // A pathological case: someone wrote `alias = "string"` at
        // the top level. The bootstrap refuses to overwrite (this
        // is the user's broken data, not ours to clobber).
        let root = fixture_dir("install-alias-as-scalar");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(dir.join("config.toml"), "alias = \"not a table\"\n").expect("write bad");
        let observed = install_cargo_alias(&root);
        cleanup(&root);
        match observed {
            Err(InstallError::AliasMismatch { .. }) => {}
            other => panic!("expected AliasMismatch for non-table alias, got {other:?}"),
        }
    }

    #[test]
    fn install_cargo_alias_error_display_includes_key_strings() {
        let mismatch = InstallError::AliasMismatch {
            existing: "weird-value".to_string(),
        };
        let s = mismatch.to_string();
        assert!(s.contains("weird-value"));
        assert!(s.contains("alias"));
    }

    // ---- uninstall_cargo_alias ------------------------------------------

    #[test]
    fn uninstall_cargo_alias_no_op_when_file_missing() {
        let root = fixture_dir("uninstall-no-file");
        let outcome = uninstall_cargo_alias(&root).expect("uninstall");
        cleanup(&root);
        assert_eq!(outcome, UninstallOutcome::AlreadyUninstalled);
    }

    #[test]
    fn uninstall_cargo_alias_no_op_when_alias_table_missing() {
        let root = fixture_dir("uninstall-no-table");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[build]\ntarget = \"x86_64-unknown-linux-gnu\"\n",
        )
        .expect("write existing");
        let outcome = uninstall_cargo_alias(&root).expect("uninstall");
        cleanup(&root);
        assert_eq!(outcome, UninstallOutcome::AlreadyUninstalled);
    }

    #[test]
    fn uninstall_cargo_alias_no_op_when_mock_key_missing() {
        let root = fixture_dir("uninstall-no-mock-key");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[alias]\nother = \"build --release\"\n",
        )
        .expect("write existing");
        let outcome = uninstall_cargo_alias(&root).expect("uninstall");
        cleanup(&root);
        assert_eq!(outcome, UninstallOutcome::AlreadyUninstalled);
    }

    #[test]
    fn uninstall_cargo_alias_removes_entry_when_present() {
        let root = fixture_dir("uninstall-remove");
        install_cargo_alias(&root).expect("install");
        assert!(status(&root).has_cargo_alias);
        let outcome = uninstall_cargo_alias(&root).expect("uninstall");
        assert_eq!(outcome, UninstallOutcome::Removed);
        assert!(!status(&root).has_cargo_alias);
        cleanup(&root);
    }

    #[test]
    fn uninstall_cargo_alias_preserves_sibling_alias_keys() {
        let root = fixture_dir("uninstall-preserve-siblings");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[alias]\nmock = \"run --manifest-path mock/Cargo.toml --bin mock --\"\nother = \"build --release\"\n",
        )
        .expect("write existing");
        uninstall_cargo_alias(&root).expect("uninstall");
        let parsed: toml::Table = std::fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .parse()
            .unwrap();
        let alias = parsed.get("alias").and_then(|v| v.as_table()).expect("alias table survives");
        assert!(!alias.contains_key("mock"));
        assert!(alias.contains_key("other"));
        cleanup(&root);
    }

    #[test]
    fn uninstall_cargo_alias_removes_empty_alias_table() {
        let root = fixture_dir("uninstall-empty-table");
        install_cargo_alias(&root).expect("install");
        uninstall_cargo_alias(&root).expect("uninstall");
        let parsed: toml::Table = std::fs::read_to_string(root.join(".cargo").join("config.toml"))
            .unwrap()
            .parse()
            .unwrap();
        // [alias] table should be gone since mock was its only key.
        assert!(parsed.get("alias").is_none());
        cleanup(&root);
    }

    #[test]
    fn uninstall_cargo_alias_preserves_unrelated_top_level_keys() {
        let root = fixture_dir("uninstall-preserve-top");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[alias]\nmock = \"run --manifest-path mock/Cargo.toml --bin mock --\"\n\n[build]\ntarget = \"x86_64-unknown-linux-gnu\"\n",
        )
        .expect("write existing");
        uninstall_cargo_alias(&root).expect("uninstall");
        let parsed: toml::Table = std::fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .parse()
            .unwrap();
        assert!(parsed.get("build").and_then(|v| v.as_table()).is_some());
        cleanup(&root);
    }

    #[test]
    fn uninstall_cargo_alias_is_idempotent_after_first_call() {
        let root = fixture_dir("uninstall-idempotent");
        install_cargo_alias(&root).expect("install");
        let first = uninstall_cargo_alias(&root).expect("first uninstall");
        let second = uninstall_cargo_alias(&root).expect("second uninstall");
        cleanup(&root);
        assert_eq!(first, UninstallOutcome::Removed);
        assert_eq!(second, UninstallOutcome::AlreadyUninstalled);
    }

    #[test]
    fn uninstall_cargo_alias_surfaces_unparseable_toml() {
        let root = fixture_dir("uninstall-bad-toml");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(dir.join("config.toml"), "<<< not toml >>>").expect("write bad");
        let observed = uninstall_cargo_alias(&root);
        cleanup(&root);
        match observed {
            Err(InstallError::UnparseableCargoConfig(_)) => {}
            other => panic!("expected UnparseableCargoConfig, got {other:?}"),
        }
    }

    #[test]
    fn uninstall_cargo_alias_refuses_alias_as_non_table_shape() {
        let root = fixture_dir("uninstall-alias-scalar");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(dir.join("config.toml"), "alias = \"not a table\"\n").expect("write bad");
        let observed = uninstall_cargo_alias(&root);
        cleanup(&root);
        match observed {
            Err(InstallError::AliasMismatch { .. }) => {}
            other => panic!("expected AliasMismatch for non-table alias, got {other:?}"),
        }
    }

    #[test]
    fn install_then_uninstall_round_trip_leaves_file_minus_alias() {
        let root = fixture_dir("round-trip");
        let dir = root.join(".cargo");
        std::fs::create_dir_all(&dir).expect("mkdir .cargo");
        std::fs::write(
            dir.join("config.toml"),
            "[build]\ntarget = \"x86_64-unknown-linux-gnu\"\n",
        )
        .expect("write existing");
        install_cargo_alias(&root).expect("install");
        uninstall_cargo_alias(&root).expect("uninstall");
        let parsed: toml::Table = std::fs::read_to_string(dir.join("config.toml"))
            .unwrap()
            .parse()
            .unwrap();
        // [build] survives both operations; [alias] gone.
        assert!(parsed.get("build").and_then(|v| v.as_table()).is_some());
        assert!(parsed.get("alias").is_none());
        cleanup(&root);
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
