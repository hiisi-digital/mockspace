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

/// State of the builtin agent extraction under
/// `<root>/mock/target/agent/`. Slice 4 of the install-surface work
/// per `mock/research/202605221200_mockspace-builtin-install-surface-revised.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentExtractState {
    /// `VERSION` sidecar matches the binary's `CARGO_PKG_VERSION`
    /// and the `INDEX.md` sentinel is present. Extract is current.
    Present,
    /// `VERSION` sidecar exists but disagrees with the binary, or
    /// the `INDEX.md` sentinel is missing. The next cold-start
    /// subcommand (or `cargo mock refresh`) re-extracts.
    Stale,
    /// No `VERSION` sidecar (or no agent directory at all). The
    /// extract has not run, or ran and was then wiped (e.g. by
    /// `cargo clean`). The next cold-start subcommand extracts.
    Missing,
}

impl AgentExtractState {
    /// Short human-readable label for status reporting.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Stale => "stale",
            Self::Missing => "missing",
        }
    }
}

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
    /// State of the builtin agent extraction under
    /// `mock/target/agent/`. See [`AgentExtractState`].
    pub agent_extract: AgentExtractState,
}

impl AdoptionStatus {
    /// All bootstrap signals present and the agent extract is
    /// current. The repo is fully opted in.
    pub fn is_fully_adopted(&self) -> bool {
        self.has_mock_dir
            && self.has_cargo_alias
            && self.has_hooks_path
            && self.agent_extract == AgentExtractState::Present
    }

    /// None of the signals present: the repo has not opted in at
    /// all. Workspace gates treat this as silent allow rather than
    /// drift. The agent extract is allowed to be missing (it is a
    /// derived runtime artifact, not part of opt-in proper).
    pub fn is_uninstalled(&self) -> bool {
        !self.has_mock_dir
            && !self.has_cargo_alias
            && !self.has_hooks_path
            && self.agent_extract == AgentExtractState::Missing
    }

    /// At least one signal but not all of them: the repo's adoption
    /// has drifted out of step. Workspace gates surface this as a
    /// structured deny so the user notices.
    pub fn is_partial(&self) -> bool {
        !self.is_fully_adopted() && !self.is_uninstalled()
    }
}

/// Inspect `repo_root` and return the observed adoption signals.
/// Pure read; never mutates the filesystem or git config. Signals
/// are derived independently so a partial-adoption state is
/// detectable as such (rather than being silently rounded to "not
/// installed" or "installed").
pub fn status(repo_root: &Path) -> AdoptionStatus {
    AdoptionStatus {
        has_mock_dir: has_mock_dir(repo_root),
        has_cargo_alias: has_cargo_alias(repo_root),
        has_hooks_path: has_hooks_path(repo_root),
        agent_extract: agent_extract_state(repo_root),
    }
}

/// Probe `<repo_root>/mock/target/agent/` for the VERSION sidecar
/// and INDEX.md sentinel. The two-probe shape mirrors
/// [`ensure_agent_extracted`]: VERSION-only would miss the
/// partial-delete drift mode, INDEX.md-only would miss a
/// stale-version case.
fn agent_extract_state(repo_root: &Path) -> AgentExtractState {
    let agent_dir = repo_root.join(AGENT_DIR);
    let version_path = agent_dir.join(AGENT_VERSION_FILE);
    match std::fs::read_to_string(&version_path) {
        Ok(contents) => {
            let canonical = format!("{BINARY_VERSION}\n");
            if contents != canonical {
                AgentExtractState::Stale
            } else if !agent_dir.join("INDEX.md").exists() {
                AgentExtractState::Stale
            } else {
                AgentExtractState::Present
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AgentExtractState::Missing,
        // Any non-NotFound read failure (permission denied, IO error)
        // maps to Stale rather than Missing. The status surface tells
        // the consumer "run refresh" rather than "extract has not
        // happened yet", which is the right narrative when the
        // sidecar exists but can't be read. `ensure_agent_extracted`
        // collapses the same case via `.ok()` and triggers re-extract;
        // both paths converge on the same end state via different
        // routes.
        Err(_) => AgentExtractState::Stale,
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

/// The relative directory the bootstrap writes hook scripts into.
/// `core.hooksPath` in the consumer's `.git/config` points at the
/// joined `<repo_root>/<HOOKS_DIR>` path; the prefix-match check in
/// [`has_hooks_path`] requires the configured path to be under
/// `<repo_root>/mock/target/`, which `mock/target/hooks` satisfies.
const HOOKS_DIR: &str = "mock/target/hooks";

/// The hook script bodies the bootstrap writes. Each entry is
/// `(hook-name, body)`. The bodies invoke `cargo mock check --gate <g>`
/// once the v2 CLI (#560) ships; until then the scripts will fail
/// with "no such subcommand", which is the right transitional
/// behaviour: hook fires, mockspace is not yet available, push or
/// commit is rejected. The user fixes by completing the bootstrap
/// (which includes installing the CLI binary).
const HOOK_SCRIPTS: &[(&str, &str)] = &[
    (
        "pre-commit",
        "#!/bin/sh\n\
         # Installed by mockspace v2 bootstrap. Invokes the mockspace\n\
         # commit gate; do not edit by hand. Re-run `cargo mock\n\
         # install` to refresh.\n\
         exec cargo mock check --gate commit\n",
    ),
    (
        "pre-push",
        "#!/bin/sh\n\
         # Installed by mockspace v2 bootstrap. Invokes the mockspace\n\
         # push gate; do not edit by hand. Re-run `cargo mock\n\
         # install` to refresh.\n\
         exec cargo mock check --gate push\n",
    ),
];

/// Install the git hook scripts under `<repo_root>/mock/target/hooks/`
/// and point `<repo_root>/.git/config`'s `core.hooksPath` at that
/// directory. Symmetric to [`install_cargo_alias`]; together they
/// give the consumer the full v2 adoption state that
/// [`AdoptionStatus`] reports.
///
/// Behaviour matrix:
///
/// - Hook directory and scripts are written unconditionally each
///   call. The content is canonical; an existing user-edited script
///   gets overwritten without warning, mirroring the cargo-alias
///   "canonical-value-or-bust" policy. Consumers who hand-edit the
///   scripts should re-derive their edits as a wrapper that calls
///   the canonical body.
/// - Scripts are made executable on Unix (chmod 0o755). On non-Unix
///   the executable bit is not set; git on those platforms uses the
///   `core.fileMode` setting or the file extension to determine
///   executability.
/// - `.git/config`'s `[core] hooksPath` is set to the relative
///   `mock/target/hooks` form. If the file does not exist or has no
///   `[core]` section, both are created. If `hooksPath` already
///   exists in `[core]` with a different value, it is overwritten;
///   this is a hook install, not a hook discovery.
/// - The implementation reads `.git/config` as INI, mutates the
///   `[core] hooksPath` line in place (or inserts it), and writes
///   the file back. Other sections survive unchanged.
pub fn install_hooks(repo_root: &Path) -> Result<InstallOutcome, InstallError> {
    let hooks_dir = repo_root.join(HOOKS_DIR);
    std::fs::create_dir_all(&hooks_dir)?;
    let mut any_changed = false;
    for (name, body) in HOOK_SCRIPTS {
        let path = hooks_dir.join(name);
        let existing = std::fs::read_to_string(&path).ok();
        if existing.as_deref() != Some(*body) {
            std::fs::write(&path, body)?;
            any_changed = true;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path)?.permissions();
            if perms.mode() & 0o755 != 0o755 {
                perms.set_mode(0o755);
                std::fs::set_permissions(&path, perms)?;
                any_changed = true;
            }
        }
    }
    let git_changed = set_hooks_path(repo_root, Some(HOOKS_DIR))?;
    let outcome = if any_changed || git_changed {
        InstallOutcome::Installed
    } else {
        InstallOutcome::AlreadyInstalled
    };
    Ok(outcome)
}

/// Uninstall the git hook scripts and clear `core.hooksPath`. Removes
/// `<repo_root>/mock/target/hooks/{pre-commit, pre-push}`; leaves
/// `mock/target/hooks/` itself in place even after the scripts are
/// gone (the directory is part of cargo's target tree and the user
/// may have other content there). Clears the `core.hooksPath` line
/// from `.git/config`'s `[core]` section if present.
pub fn uninstall_hooks(repo_root: &Path) -> Result<UninstallOutcome, InstallError> {
    let hooks_dir = repo_root.join(HOOKS_DIR);
    let mut any_changed = false;
    for (name, _) in HOOK_SCRIPTS {
        let path = hooks_dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => any_changed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(InstallError::Io(e)),
        }
    }
    let git_changed = set_hooks_path(repo_root, None)?;
    let outcome = if any_changed || git_changed {
        UninstallOutcome::Removed
    } else {
        UninstallOutcome::AlreadyUninstalled
    };
    Ok(outcome)
}

/// Directory the agent-builtin extraction lands in, relative to
/// the repo root. Sibling to [`HOOKS_DIR`] under `mock/target/` per
/// the revised install-surface memo at
/// `mock/research/202605221200_mockspace-builtin-install-surface-revised.md`.
const AGENT_DIR: &str = "mock/target/agent";

/// Name of the version sidecar inside [`AGENT_DIR`]. Carries the
/// mockspace binary's `CARGO_PKG_VERSION` string so refresh can
/// detect staleness without re-hashing the extracted content.
const AGENT_VERSION_FILE: &str = "VERSION";

/// Mockspace binary version, baked in at compile time. The version
/// sidecar written to [`AGENT_DIR`] carries this string; on
/// subsequent invocations a mismatch between sidecar and constant
/// means the binary was upgraded and the extract is stale.
///
/// Resolves to the `mockspace-rs` library version specifically (the
/// crate this code compiles in). The CLI binary is a separate crate
/// that depends on `mockspace-rs`; both ship from the same workspace
/// publish cycle, so the two stay lockstep in practice. If they
/// diverge, the staleness check tracks the library, not the CLI
/// binary, which is the right side because the embedded content
/// lives in this crate.
const BINARY_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Install the agent-builtin canonical content under
/// `<repo_root>/mock/target/agent/`. Walks every entry of
/// [`crate::agent_builtin::FILES`], writes the embedded content out
/// to disk, and emits a `VERSION` sidecar carrying [`BINARY_VERSION`].
///
/// Behaviour matrix:
///
/// - Directory and files are written unconditionally per call. The
///   content is canonical; an existing user-edited file gets
///   overwritten without warning. Consumers must not edit content
///   under `mock/target/agent/`; the directory is mockspace-managed
///   per the install-surface memo.
/// - The `VERSION` sidecar is written each call. If it already
///   matches [`BINARY_VERSION`] AND every file content matches the
///   embedded version, the call reports [`InstallOutcome::AlreadyInstalled`].
///   Otherwise [`InstallOutcome::Installed`].
/// - On non-Unix platforms the permission set is whatever
///   `std::fs::write` produces; the content is plain markdown so no
///   executability bit is needed.
pub fn install_agent_builtin(repo_root: &Path) -> Result<InstallOutcome, InstallError> {
    let agent_dir = repo_root.join(AGENT_DIR);
    std::fs::create_dir_all(&agent_dir)?;
    let mut any_changed = false;
    for (name, body) in crate::agent_builtin::FILES {
        let path = agent_dir.join(name);
        let existing = std::fs::read_to_string(&path).ok();
        if existing.as_deref() != Some(*body) {
            std::fs::write(&path, body)?;
            any_changed = true;
        }
    }
    let version_path = agent_dir.join(AGENT_VERSION_FILE);
    let existing_version = std::fs::read_to_string(&version_path).ok();
    let canonical_version = format!("{BINARY_VERSION}\n");
    if existing_version.as_deref() != Some(canonical_version.as_str()) {
        std::fs::write(&version_path, &canonical_version)?;
        any_changed = true;
    }
    Ok(if any_changed {
        InstallOutcome::Installed
    } else {
        InstallOutcome::AlreadyInstalled
    })
}

/// Uninstall the agent-builtin extraction. Removes every file
/// [`install_agent_builtin`] writes, plus the `VERSION` sidecar.
/// Leaves the empty `mock/target/agent/` directory in place because
/// the surrounding `mock/target/` is cargo-managed.
pub fn uninstall_agent_builtin(repo_root: &Path) -> Result<UninstallOutcome, InstallError> {
    let agent_dir = repo_root.join(AGENT_DIR);
    let mut any_changed = false;
    for (name, _) in crate::agent_builtin::FILES {
        let path = agent_dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => any_changed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(InstallError::Io(e)),
        }
    }
    let version_path = agent_dir.join(AGENT_VERSION_FILE);
    match std::fs::remove_file(&version_path) {
        Ok(()) => any_changed = true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(InstallError::Io(e)),
    }
    Ok(if any_changed {
        UninstallOutcome::Removed
    } else {
        UninstallOutcome::AlreadyUninstalled
    })
}

/// Lazy-fallback entry point for subcommands that want the
/// agent-builtin content present without forcing the user to run
/// `cargo mock install` first. Re-extracts the embedded content
/// when the `VERSION` sidecar is missing, disagrees with the
/// binary, OR when a sentinel file under `AGENT_DIR` is missing;
/// no-op when the extract is current.
///
/// The sentinel probe (checking `INDEX.md` presence in addition to
/// the version match) catches the partial-delete drift mode: a
/// consumer or stray `rm` removes one of the markdown files but
/// leaves `VERSION` intact. A version-only check would miss this;
/// the extra probe is one syscall per cold start, which is
/// negligible alongside the version read.
///
/// Returns the outcome so callers can detect "did extraction
/// happen on this call" and message accordingly, or ignore it for
/// the silent-cold-start case the slice-3 plan calls out.
pub fn ensure_agent_extracted(repo_root: &Path) -> Result<InstallOutcome, InstallError> {
    let agent_dir = repo_root.join(AGENT_DIR);
    let version_path = agent_dir.join(AGENT_VERSION_FILE);
    let existing_version = std::fs::read_to_string(&version_path).ok();
    let canonical_version = format!("{BINARY_VERSION}\n");
    let version_matches = existing_version.as_deref() == Some(canonical_version.as_str());
    let sentinel_present = agent_dir.join("INDEX.md").exists();
    if version_matches && sentinel_present {
        return Ok(InstallOutcome::AlreadyInstalled);
    }
    install_agent_builtin(repo_root)
}

/// Full v2 bootstrap install: runs [`install_cargo_alias`],
/// [`install_hooks`], and [`install_agent_builtin`] and combines
/// their outcomes. This is the canonical entry point the CLI's
/// `cargo mock install` subcommand (#560) wires through.
///
/// Returns [`InstallOutcome::Installed`] if any half made a
/// change, [`InstallOutcome::AlreadyInstalled`] only when all
/// three halves were no-ops. Propagates the first error
/// encountered; the install is not transactional, so a partial
/// state can result when one half succeeds but a later half fails.
/// The user reruns to converge.
///
/// Calling [`install`] on a fully-installed repo is idempotent.
/// [`refresh`] is an alias for this behaviour with no semantic
/// difference; both functions exist so the CLI surface can name
/// the operation by intent (`mock install` vs `mock refresh`).
pub fn install(repo_root: &Path) -> Result<InstallOutcome, InstallError> {
    let alias_outcome = install_cargo_alias(repo_root)?;
    let hooks_outcome = install_hooks(repo_root)?;
    let agent_outcome = install_agent_builtin(repo_root)?;
    let installed = matches!(alias_outcome, InstallOutcome::Installed)
        || matches!(hooks_outcome, InstallOutcome::Installed)
        || matches!(agent_outcome, InstallOutcome::Installed);
    Ok(if installed {
        InstallOutcome::Installed
    } else {
        InstallOutcome::AlreadyInstalled
    })
}

/// Full v2 bootstrap uninstall: runs [`uninstall_agent_builtin`],
/// [`uninstall_hooks`], and [`uninstall_cargo_alias`] (reverse
/// install order) and combines their outcomes. The CLI's
/// `cargo mock uninstall` subcommand (#560) wires through here.
///
/// Returns [`UninstallOutcome::Removed`] if any half removed
/// something, [`UninstallOutcome::AlreadyUninstalled`] only when
/// all three halves were no-ops.
pub fn uninstall(repo_root: &Path) -> Result<UninstallOutcome, InstallError> {
    let agent_outcome = uninstall_agent_builtin(repo_root)?;
    let hooks_outcome = uninstall_hooks(repo_root)?;
    let alias_outcome = uninstall_cargo_alias(repo_root)?;
    let removed = matches!(agent_outcome, UninstallOutcome::Removed)
        || matches!(hooks_outcome, UninstallOutcome::Removed)
        || matches!(alias_outcome, UninstallOutcome::Removed);
    Ok(if removed {
        UninstallOutcome::Removed
    } else {
        UninstallOutcome::AlreadyUninstalled
    })
}

/// Re-derive the v2 bootstrap state. Functionally identical to
/// [`install`]; named separately so the CLI's `cargo mock refresh`
/// subcommand has a load-bearing surface. Useful after the
/// canonical hook script bodies or cargo alias value change in a
/// new mockspace release; reruns to converge any drift.
pub fn refresh(repo_root: &Path) -> Result<InstallOutcome, InstallError> {
    install(repo_root)
}

/// Set (or clear) `core.hooksPath` in `<repo_root>/.git/config`.
/// `Some(value)` writes the value into the `[core]` section,
/// creating the file and section if needed. `None` removes the
/// `hooksPath` line; if `[core]` becomes empty as a result, the
/// section header is removed too. Returns `Ok(true)` if the file
/// content changed, `Ok(false)` if no mutation was needed.
fn set_hooks_path(repo_root: &Path, value: Option<&str>) -> Result<bool, InstallError> {
    let git_dir = repo_root.join(".git");
    let path = git_dir.join("config");
    let mut lines: Vec<String> = match std::fs::read_to_string(&path) {
        Ok(contents) => contents.lines().map(str::to_string).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(InstallError::Io(e)),
    };

    // Locate `[core]` section bounds. `core_start` is the index of
    // the `[core]` header line; `core_end` is the index of the next
    // section header (or `lines.len()` if `[core]` runs to EOF).
    let mut core_start: Option<usize> = None;
    let mut core_end = lines.len();
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if trimmed == "[core]" {
                core_start = Some(i);
            } else if core_start.is_some() {
                core_end = i;
                break;
            }
        }
    }

    let mut hooks_idx: Option<usize> = None;
    if let Some(start) = core_start {
        for i in (start + 1)..core_end {
            let trimmed = lines[i].trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                break;
            }
            if let Some((key, _)) = trimmed.split_once('=') {
                if key.trim() == "hooksPath" {
                    hooks_idx = Some(i);
                    break;
                }
            }
        }
    }

    let original_lines = lines.clone();
    let canonical_line = value.map(|v| format!("\thooksPath = {v}"));

    match (canonical_line, hooks_idx, core_start) {
        (Some(line), Some(idx), _) => {
            // Replace existing hooksPath line.
            lines[idx] = line;
        }
        (Some(line), None, Some(_start)) => {
            // Insert hooksPath at the tail of the existing [core]
            // section (just before the next section header, or at
            // EOF if [core] runs to the end). Matches git's own
            // emit order, which appends new keys at section tails.
            lines.insert(core_end, line);
        }
        (Some(line), None, None) => {
            // No [core] section yet; append a fresh one.
            if !lines.is_empty() && !lines.last().is_some_and(|l| l.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[core]".to_string());
            lines.push(line);
        }
        (None, Some(idx), _) => {
            // Remove the hooksPath line.
            lines.remove(idx);
            // If [core] is now empty (no key lines), remove the
            // section header too. Empty means: between core_start
            // and the next section header, every line is a comment
            // or blank.
            if let Some(start) = core_start {
                let new_end = lines
                    .iter()
                    .enumerate()
                    .skip(start + 1)
                    .find(|(_, l)| {
                        let t = l.trim();
                        t.starts_with('[') && t.ends_with(']')
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(lines.len());
                let core_has_content = (start + 1..new_end).any(|i| {
                    let t = lines[i].trim();
                    !t.is_empty() && !t.starts_with('#')
                });
                if !core_has_content {
                    // Remove lines from `start` up to (but not
                    // including) `new_end`.
                    lines.drain(start..new_end);
                }
            }
        }
        (None, None, _) => {
            // Nothing to do.
        }
    }

    if lines == original_lines {
        return Ok(false);
    }

    std::fs::create_dir_all(&git_dir)?;
    let mut out = lines.join("\n");
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    std::fs::write(&path, out)?;
    Ok(true)
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
    fn fully_adopted_repo_reports_all_signals() {
        let root = fixture_dir("full");
        write_mock_dir(&root);
        write_cargo_alias(&root);
        write_git_hooks_path(&root);
        install_agent_builtin(&root).expect("install agent");
        let s = status(&root);
        cleanup(&root);
        assert!(s.has_mock_dir);
        assert!(s.has_cargo_alias);
        assert!(s.has_hooks_path);
        assert_eq!(s.agent_extract, AgentExtractState::Present);
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

    // ---- install_hooks / uninstall_hooks --------------------------------

    #[test]
    fn install_hooks_creates_scripts_and_sets_hooks_path() {
        let root = fixture_dir("install-hooks-fresh");
        let outcome = install_hooks(&root).expect("install");
        assert_eq!(outcome, InstallOutcome::Installed);
        let hooks_dir = root.join("mock").join("target").join("hooks");
        assert!(hooks_dir.join("pre-commit").is_file());
        assert!(hooks_dir.join("pre-push").is_file());
        // Status diagnostic flips.
        assert!(status(&root).has_hooks_path);
        cleanup(&root);
    }

    #[cfg(unix)]
    #[test]
    fn install_hooks_marks_scripts_executable() {
        use std::os::unix::fs::PermissionsExt;
        let root = fixture_dir("install-hooks-exec");
        install_hooks(&root).expect("install");
        let hooks_dir = root.join("mock").join("target").join("hooks");
        for name in ["pre-commit", "pre-push"] {
            let mode = std::fs::metadata(hooks_dir.join(name))
                .expect("stat")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode & 0o755, 0o755, "{name} should have 0o755 bits set");
        }
        cleanup(&root);
    }

    #[test]
    fn install_hooks_is_idempotent_when_state_matches() {
        let root = fixture_dir("install-hooks-idem");
        install_hooks(&root).expect("first install");
        let outcome = install_hooks(&root).expect("second install");
        assert_eq!(outcome, InstallOutcome::AlreadyInstalled);
        cleanup(&root);
    }

    #[test]
    fn install_hooks_overwrites_drifted_script_body() {
        let root = fixture_dir("install-hooks-drift");
        let hooks_dir = root.join("mock").join("target").join("hooks");
        std::fs::create_dir_all(&hooks_dir).expect("mkdir hooks");
        std::fs::write(hooks_dir.join("pre-commit"), "#!/bin/sh\necho stale\n")
            .expect("write stale");
        let outcome = install_hooks(&root).expect("install");
        assert_eq!(outcome, InstallOutcome::Installed);
        let after = std::fs::read_to_string(hooks_dir.join("pre-commit")).expect("read after");
        assert!(after.contains("cargo mock check --gate commit"));
        cleanup(&root);
    }

    #[test]
    fn install_hooks_writes_core_section_when_git_config_missing() {
        let root = fixture_dir("install-hooks-no-gitconfig");
        install_hooks(&root).expect("install");
        let contents = std::fs::read_to_string(root.join(".git").join("config")).expect("read");
        assert!(contents.contains("[core]"));
        assert!(contents.contains("hooksPath"));
        assert!(contents.contains("mock/target/hooks"));
        cleanup(&root);
    }

    #[test]
    fn install_hooks_overwrites_existing_hookspath() {
        let root = fixture_dir("install-hooks-overwrite-path");
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        std::fs::write(
            git_dir.join("config"),
            "[core]\n\thooksPath = /opt/team-hooks\n",
        )
        .expect("write existing");
        install_hooks(&root).expect("install");
        let contents = std::fs::read_to_string(git_dir.join("config")).expect("read");
        // Old value gone, canonical value present.
        assert!(!contents.contains("/opt/team-hooks"));
        assert!(contents.contains("mock/target/hooks"));
        cleanup(&root);
    }

    #[test]
    fn install_hooks_preserves_other_git_config_sections() {
        let root = fixture_dir("install-hooks-preserve-sections");
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).expect("mkdir .git");
        std::fs::write(
            git_dir.join("config"),
            "[user]\n\tname = Test User\n\temail = test@example.com\n",
        )
        .expect("write existing");
        install_hooks(&root).expect("install");
        let contents = std::fs::read_to_string(git_dir.join("config")).expect("read");
        assert!(contents.contains("[user]"));
        assert!(contents.contains("Test User"));
        assert!(contents.contains("test@example.com"));
        assert!(contents.contains("[core]"));
        cleanup(&root);
    }

    #[test]
    fn uninstall_hooks_removes_scripts_and_clears_hookspath() {
        let root = fixture_dir("uninstall-hooks");
        install_hooks(&root).expect("install");
        assert!(status(&root).has_hooks_path);
        let outcome = uninstall_hooks(&root).expect("uninstall");
        assert_eq!(outcome, UninstallOutcome::Removed);
        let hooks_dir = root.join("mock").join("target").join("hooks");
        assert!(!hooks_dir.join("pre-commit").exists());
        assert!(!hooks_dir.join("pre-push").exists());
        assert!(!status(&root).has_hooks_path);
        cleanup(&root);
    }

    #[test]
    fn uninstall_hooks_is_idempotent_when_nothing_present() {
        let root = fixture_dir("uninstall-hooks-empty");
        let outcome = uninstall_hooks(&root).expect("uninstall");
        assert_eq!(outcome, UninstallOutcome::AlreadyUninstalled);
        cleanup(&root);
    }

    #[test]
    fn uninstall_hooks_preserves_other_git_config_sections() {
        let root = fixture_dir("uninstall-hooks-preserve");
        install_hooks(&root).expect("install");
        // Insert a sibling [user] section.
        let git_config = root.join(".git").join("config");
        let mut contents = std::fs::read_to_string(&git_config).expect("read");
        contents.push_str("\n[user]\n\tname = Test User\n");
        std::fs::write(&git_config, contents).expect("write");
        uninstall_hooks(&root).expect("uninstall");
        let after = std::fs::read_to_string(&git_config).expect("read after");
        assert!(after.contains("[user]"));
        assert!(after.contains("Test User"));
        cleanup(&root);
    }

    #[test]
    fn uninstall_hooks_removes_empty_core_section_after_hookspath_clear() {
        let root = fixture_dir("uninstall-hooks-empty-core");
        install_hooks(&root).expect("install");
        uninstall_hooks(&root).expect("uninstall");
        let contents = std::fs::read_to_string(root.join(".git").join("config")).expect("read");
        // [core] section was only the hooksPath key; should be gone now.
        assert!(
            !contents.contains("[core]"),
            "expected [core] section removed, got: {contents:?}"
        );
        cleanup(&root);
    }

    #[test]
    fn install_then_uninstall_hooks_round_trip_returns_to_clean_state() {
        let root = fixture_dir("hooks-round-trip");
        let before = status(&root);
        assert!(!before.has_hooks_path);
        install_hooks(&root).expect("install");
        assert!(status(&root).has_hooks_path);
        uninstall_hooks(&root).expect("uninstall");
        assert!(!status(&root).has_hooks_path);
        cleanup(&root);
    }

    // ---- install / uninstall / refresh top-level wrappers --------------

    #[test]
    fn install_flips_status_to_fully_adopted_when_mock_dir_exists() {
        let root = fixture_dir("install-full");
        // Status's has_mock_dir signal needs the mock/ directory; the
        // bootstrap doesn't create it (the consumer's `mock/` is part
        // of their repo skeleton). Pre-create here so is_fully_adopted
        // can fire.
        std::fs::create_dir_all(root.join("mock")).expect("mkdir mock");
        let outcome = install(&root).expect("install");
        assert_eq!(outcome, InstallOutcome::Installed);
        let s = status(&root);
        assert!(s.is_fully_adopted(), "expected fully adopted, got {s:?}");
        cleanup(&root);
    }

    #[test]
    fn install_is_idempotent_after_first_call() {
        let root = fixture_dir("install-top-idem");
        let first = install(&root).expect("first install");
        let second = install(&root).expect("second install");
        cleanup(&root);
        assert_eq!(first, InstallOutcome::Installed);
        assert_eq!(second, InstallOutcome::AlreadyInstalled);
    }

    // ---- agent_builtin extraction --------------------------------------

    #[test]
    fn install_agent_builtin_writes_every_file_plus_version_sidecar() {
        let root = fixture_dir("install-agent-builtin");
        let outcome = install_agent_builtin(&root).expect("install");
        assert_eq!(outcome, InstallOutcome::Installed);
        let agent_dir = root.join("mock").join("target").join("agent");
        for (name, body) in crate::agent_builtin::FILES {
            let on_disk = std::fs::read_to_string(agent_dir.join(name))
                .unwrap_or_else(|e| panic!("missing `{name}`: {e}"));
            assert_eq!(&on_disk, body, "extracted `{name}` content mismatch");
        }
        let version = std::fs::read_to_string(agent_dir.join("VERSION")).expect("VERSION");
        assert_eq!(version, format!("{BINARY_VERSION}\n"));
        cleanup(&root);
    }

    #[test]
    fn install_agent_builtin_is_idempotent_after_first_call() {
        let root = fixture_dir("install-agent-builtin-idem");
        let first = install_agent_builtin(&root).expect("first install");
        let second = install_agent_builtin(&root).expect("second install");
        cleanup(&root);
        assert_eq!(first, InstallOutcome::Installed);
        assert_eq!(second, InstallOutcome::AlreadyInstalled);
    }

    #[test]
    fn install_agent_builtin_overwrites_drifted_file() {
        let root = fixture_dir("install-agent-builtin-drift");
        install_agent_builtin(&root).expect("install");
        let phases_path = root
            .join("mock")
            .join("target")
            .join("agent")
            .join("phases.md");
        std::fs::write(&phases_path, "# Tampered\n").expect("write tampered");
        let outcome = install_agent_builtin(&root).expect("refresh");
        assert_eq!(outcome, InstallOutcome::Installed);
        let restored = std::fs::read_to_string(&phases_path).expect("read");
        assert!(
            restored.starts_with("# Phases"),
            "expected restored canonical body, got: {}",
            &restored[..restored.len().min(60)]
        );
        cleanup(&root);
    }

    #[test]
    fn ensure_agent_extracted_is_noop_when_version_matches() {
        let root = fixture_dir("ensure-agent-noop");
        install_agent_builtin(&root).expect("install");
        let outcome = ensure_agent_extracted(&root).expect("ensure");
        assert_eq!(outcome, InstallOutcome::AlreadyInstalled);
        cleanup(&root);
    }

    #[test]
    fn ensure_agent_extracted_writes_when_version_missing() {
        let root = fixture_dir("ensure-agent-cold");
        let agent_dir = root.join("mock").join("target").join("agent");
        assert!(!agent_dir.exists());
        let outcome = ensure_agent_extracted(&root).expect("ensure");
        assert_eq!(outcome, InstallOutcome::Installed);
        assert!(agent_dir.join("phases.md").exists());
        assert!(agent_dir.join("VERSION").exists());
        cleanup(&root);
    }

    #[test]
    fn ensure_agent_extracted_re_runs_when_version_matches_but_sentinel_missing() {
        let root = fixture_dir("ensure-agent-sentinel-missing");
        install_agent_builtin(&root).expect("install");
        let agent_dir = root.join("mock").join("target").join("agent");
        // VERSION stays intact; INDEX.md gets removed. Partial-delete
        // drift mode: lazy fallback must heal instead of trusting VERSION.
        std::fs::remove_file(agent_dir.join("INDEX.md")).expect("remove INDEX.md");
        let outcome = ensure_agent_extracted(&root).expect("ensure");
        assert_eq!(outcome, InstallOutcome::Installed);
        assert!(agent_dir.join("INDEX.md").exists(), "INDEX.md restored");
        cleanup(&root);
    }

    #[test]
    fn ensure_agent_extracted_re_runs_when_version_stale() {
        let root = fixture_dir("ensure-agent-stale");
        install_agent_builtin(&root).expect("install");
        let version_path = root
            .join("mock")
            .join("target")
            .join("agent")
            .join("VERSION");
        std::fs::write(&version_path, "0.0.0-stale\n").expect("write stale version");
        let outcome = ensure_agent_extracted(&root).expect("ensure");
        assert_eq!(outcome, InstallOutcome::Installed);
        let now = std::fs::read_to_string(&version_path).expect("read");
        assert_eq!(now, format!("{BINARY_VERSION}\n"));
        cleanup(&root);
    }

    #[test]
    fn uninstall_agent_builtin_removes_files_and_version() {
        let root = fixture_dir("uninstall-agent-builtin");
        install_agent_builtin(&root).expect("install");
        let outcome = uninstall_agent_builtin(&root).expect("uninstall");
        assert_eq!(outcome, UninstallOutcome::Removed);
        let agent_dir = root.join("mock").join("target").join("agent");
        for (name, _) in crate::agent_builtin::FILES {
            assert!(!agent_dir.join(name).exists(), "`{name}` should be removed");
        }
        assert!(!agent_dir.join("VERSION").exists());
        cleanup(&root);
    }

    #[test]
    fn uninstall_agent_builtin_is_idempotent_when_nothing_present() {
        let root = fixture_dir("uninstall-agent-builtin-empty");
        let outcome = uninstall_agent_builtin(&root).expect("uninstall");
        assert_eq!(outcome, UninstallOutcome::AlreadyUninstalled);
        cleanup(&root);
    }

    #[test]
    fn install_uninstall_round_trip_returns_to_zero_state() {
        let root = fixture_dir("install-uninstall-round-trip");
        install(&root).expect("install");
        let s_after_install = status(&root);
        assert!(s_after_install.has_cargo_alias);
        assert!(s_after_install.has_hooks_path);
        let outcome = uninstall(&root).expect("uninstall");
        assert_eq!(outcome, UninstallOutcome::Removed);
        let s_after_uninstall = status(&root);
        assert!(!s_after_uninstall.has_cargo_alias);
        assert!(!s_after_uninstall.has_hooks_path);
        cleanup(&root);
    }

    #[test]
    fn uninstall_is_idempotent_when_nothing_installed() {
        let root = fixture_dir("uninstall-noop");
        let outcome = uninstall(&root).expect("uninstall");
        cleanup(&root);
        assert_eq!(outcome, UninstallOutcome::AlreadyUninstalled);
    }

    #[test]
    fn refresh_runs_install_and_is_idempotent() {
        let root = fixture_dir("refresh-runs");
        let first = refresh(&root).expect("first refresh");
        let second = refresh(&root).expect("second refresh");
        cleanup(&root);
        assert_eq!(first, InstallOutcome::Installed);
        assert_eq!(second, InstallOutcome::AlreadyInstalled);
    }

    #[test]
    fn refresh_repairs_drifted_state() {
        let root = fixture_dir("refresh-repair-drift");
        install(&root).expect("install");
        // Hand-edit the pre-commit script to a stale body.
        let pre_commit = root
            .join("mock")
            .join("target")
            .join("hooks")
            .join("pre-commit");
        std::fs::write(&pre_commit, "#!/bin/sh\necho stale\n").expect("write stale");
        let outcome = refresh(&root).expect("refresh");
        assert_eq!(
            outcome,
            InstallOutcome::Installed,
            "refresh should report Installed after repairing drift"
        );
        let body = std::fs::read_to_string(&pre_commit).expect("read");
        assert!(body.contains("cargo mock check --gate commit"));
        cleanup(&root);
    }

    #[test]
    fn install_reports_installed_when_only_one_half_changes() {
        let root = fixture_dir("install-partial");
        // Pre-install the cargo alias half; install() should still
        // report Installed because the hooks half is fresh.
        install_cargo_alias(&root).expect("alias install");
        let outcome = install(&root).expect("top-level install");
        cleanup(&root);
        assert_eq!(outcome, InstallOutcome::Installed);
    }

    #[test]
    fn uninstall_reports_removed_when_only_one_half_was_present() {
        let root = fixture_dir("uninstall-partial");
        install_cargo_alias(&root).expect("alias install");
        let outcome = uninstall(&root).expect("top-level uninstall");
        cleanup(&root);
        assert_eq!(outcome, UninstallOutcome::Removed);
    }

    #[test]
    fn is_fully_adopted_and_is_uninstalled_are_disjoint() {
        // Exhaustive 2^3 * 3 = 24 combinations across the three
        // bootstrap signals plus the agent-extract tri-state.
        for mock in [false, true] {
            for alias in [false, true] {
                for hooks in [false, true] {
                    for agent in [
                        AgentExtractState::Present,
                        AgentExtractState::Stale,
                        AgentExtractState::Missing,
                    ] {
                        let s = AdoptionStatus {
                            has_mock_dir: mock,
                            has_cargo_alias: alias,
                            has_hooks_path: hooks,
                            agent_extract: agent,
                        };
                        let n = s.is_fully_adopted() as u8
                            + s.is_uninstalled() as u8
                            + s.is_partial() as u8;
                        assert_eq!(
                            n, 1,
                            "exactly one of the three predicates should hold; \
                             mock={mock} alias={alias} hooks={hooks} agent={agent:?}"
                        );
                    }
                }
            }
        }
    }

    // ---- agent_extract_state probe ------------------------------------

    #[test]
    fn agent_extract_state_missing_when_no_agent_dir() {
        let root = fixture_dir("agent-state-missing");
        let state = agent_extract_state(&root);
        cleanup(&root);
        assert_eq!(state, AgentExtractState::Missing);
    }

    #[test]
    fn agent_extract_state_present_after_install() {
        let root = fixture_dir("agent-state-present");
        install_agent_builtin(&root).expect("install");
        let state = agent_extract_state(&root);
        cleanup(&root);
        assert_eq!(state, AgentExtractState::Present);
    }

    #[test]
    fn agent_extract_state_stale_when_version_disagrees() {
        let root = fixture_dir("agent-state-stale-version");
        install_agent_builtin(&root).expect("install");
        let version_path = root
            .join("mock")
            .join("target")
            .join("agent")
            .join("VERSION");
        std::fs::write(&version_path, "0.0.0-stale\n").expect("write stale");
        let state = agent_extract_state(&root);
        cleanup(&root);
        assert_eq!(state, AgentExtractState::Stale);
    }

    #[test]
    fn agent_extract_state_stale_when_sentinel_missing() {
        let root = fixture_dir("agent-state-stale-sentinel");
        install_agent_builtin(&root).expect("install");
        let index_path = root
            .join("mock")
            .join("target")
            .join("agent")
            .join("INDEX.md");
        std::fs::remove_file(&index_path).expect("remove INDEX.md");
        let state = agent_extract_state(&root);
        cleanup(&root);
        assert_eq!(state, AgentExtractState::Stale);
    }
}
