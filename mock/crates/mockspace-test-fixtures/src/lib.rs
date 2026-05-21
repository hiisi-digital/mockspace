//! Test-fixture builder for mockspace v2.
//!
//! Wraps the canonical state-setup patterns (`TempDir` allocation,
//! `bootstrap::install` invocation, `lints.toml` file write) into a
//! composable builder so integration and end-to-end tests don't
//! repeat the boilerplate. The intended consumers are
//! `mockspace-cli`'s `tests/cli.rs` (#89, #90) and the golden-result
//! e2e tree comparison in #563.
//!
//! Each fixture builds on a fresh [`tempfile::TempDir`]. The handle
//! the builder returns owns the directory; dropping the handle
//! deletes the directory recursively. Consumer code reads the
//! filesystem path via [`MockspaceFixture::path`].
//!
//! # Example
//!
//! ```no_run
//! use mockspace_test_fixtures::MockspaceFixture;
//!
//! let fixture = MockspaceFixture::new()
//!     .with_install()
//!     .with_lints_toml(r#"
//!         [lints.no-bare-numeric.config]
//!         visibility = "crate"
//!     "#)
//!     .build()
//!     .expect("build fixture");
//!
//! // fixture.path() is a fresh tempdir with the v2 bootstrap
//! // installed and a user lints.toml in place.
//! let _path = fixture.path();
//! ```
//!
//! # Why a separate crate
//!
//! Splitting the fixture builder out keeps the test-only dev
//! dependencies (`tempfile`, future `assert_cmd` helpers) from
//! leaking into consumer crates' release graphs. The crate is
//! `publish = false`; it ships only inside the mockspace workspace.

use std::path::{Path, PathBuf};

use mockspace_rs::bootstrap;
use tempfile::TempDir;

/// Builder for a mockspace v2 test fixture. Each builder method
/// composes a separate axis of filesystem / bootstrap state; the
/// terminal [`build`](Self::build) seals the directory and returns
/// a [`MockspaceFixture`] handle whose drop cleans up the temp
/// directory.
///
/// The default state is "fresh tempdir, nothing installed". Each
/// `with_*` method opts the fixture into one piece of additional
/// state; call them in whatever order matches the scenario under
/// test.
#[derive(Debug, Default)]
pub struct MockspaceFixtureBuilder {
    install: bool,
    create_mock_dir: bool,
    lints_toml: Option<String>,
}

impl MockspaceFixtureBuilder {
    /// Run the v2 bootstrap install against the fixture's tempdir
    /// during [`build`](Self::build). Installs the cargo alias
    /// (`<root>/.cargo/config.toml`) and the git hooks
    /// (`<root>/mock/target/hooks/{pre-commit,pre-push}` plus
    /// `core.hooksPath` in `.git/config`).
    pub fn with_install(mut self) -> Self {
        self.install = true;
        self
    }

    /// Pre-create the consumer's `mock/` directory at the fixture
    /// root. Useful for tests that exercise [`bootstrap::status`]
    /// and need the `has_mock_dir` signal to flip independently of
    /// installing the bootstrap. Note: [`with_install`](Self::with_install)
    /// transitively creates `mock/target/hooks/` as a side effect,
    /// so passing both is allowed but redundant.
    pub fn with_mock_dir(mut self) -> Self {
        self.create_mock_dir = true;
        self
    }

    /// Write the given contents to `<root>/lints.toml`. Used to
    /// drive the explain cascade through Layer 3 (workspace
    /// defaults via `[defaults]`) or Layer 4 (per-lint TOML via
    /// `[lints.<name>]`).
    pub fn with_lints_toml(mut self, contents: impl Into<String>) -> Self {
        self.lints_toml = Some(contents.into());
        self
    }

    /// Materialise the fixture. Allocates a fresh [`TempDir`],
    /// applies every builder option, and returns the
    /// [`MockspaceFixture`] handle.
    ///
    /// Apply order is fixed: mkdir `mock/` (from `with_mock_dir`),
    /// then write `lints.toml` (from `with_lints_toml`), then run
    /// the bootstrap install (from `with_install`). Install runs
    /// last so any pre-staged files survive and bootstrap-managed
    /// files take precedence on conflict. The order of `with_*`
    /// builder calls is irrelevant.
    pub fn build(self) -> Result<MockspaceFixture, FixtureError> {
        let tempdir = TempDir::new().map_err(FixtureError::TempDir)?;
        let root = tempdir.path().to_path_buf();

        if self.create_mock_dir {
            std::fs::create_dir_all(root.join("mock")).map_err(FixtureError::Io)?;
        }

        if let Some(contents) = self.lints_toml {
            std::fs::write(root.join("lints.toml"), contents).map_err(FixtureError::Io)?;
        }

        if self.install {
            bootstrap::install(&root).map_err(FixtureError::Install)?;
        }

        Ok(MockspaceFixture {
            _tempdir: tempdir,
            path: root,
        })
    }
}

/// A materialised mockspace test fixture. Holds the [`TempDir`]
/// alive; dropping this value cleans the directory recursively.
/// Consumer code reads the filesystem path via [`Self::path`].
#[derive(Debug)]
pub struct MockspaceFixture {
    /// The underlying tempdir handle. Held to keep the directory
    /// alive for the fixture's lifetime; not exposed because
    /// `tempfile::TempDir` does not promise stable trait impls.
    _tempdir: TempDir,
    /// Resolved absolute path to the fixture root. Computed once at
    /// build time so repeat reads are infallible.
    path: PathBuf,
}

impl MockspaceFixture {
    /// Returns a [`MockspaceFixtureBuilder`] with no opt-ins. The
    /// resulting fixture is a bare tempdir.
    pub fn new() -> MockspaceFixtureBuilder {
        MockspaceFixtureBuilder::default()
    }

    /// Filesystem path to the fixture root. Lives as long as this
    /// `MockspaceFixture` value.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Failure modes during fixture construction. Each variant chains
/// the underlying error so callers can dispatch on it; the
/// [`std::fmt::Display`] impl renders a brief one-line summary.
#[derive(Debug)]
pub enum FixtureError {
    /// `tempfile::TempDir::new()` failed.
    TempDir(std::io::Error),
    /// Filesystem write or create-dir failed during fixture setup.
    Io(std::io::Error),
    /// The bootstrap install pass returned an error.
    Install(bootstrap::InstallError),
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TempDir(e) => write!(f, "failed to allocate tempdir for fixture: {e}"),
            Self::Io(e) => write!(f, "fixture filesystem setup failed: {e}"),
            Self::Install(e) => write!(f, "bootstrap install during fixture build failed: {e}"),
        }
    }
}

impl std::error::Error for FixtureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TempDir(e) | Self::Io(e) => Some(e),
            Self::Install(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_fixture_is_a_clean_tempdir() {
        let fixture = MockspaceFixture::new().build().expect("build");
        assert!(fixture.path().is_dir(), "tempdir should exist");
        assert!(
            !fixture.path().join("mock").exists(),
            "no mock/ dir without with_mock_dir"
        );
        assert!(
            !fixture.path().join(".cargo").exists(),
            "no .cargo/ without with_install"
        );
        assert!(
            !fixture.path().join("lints.toml").exists(),
            "no lints.toml without with_lints_toml"
        );
    }

    #[test]
    fn with_mock_dir_creates_the_directory() {
        let fixture = MockspaceFixture::new()
            .with_mock_dir()
            .build()
            .expect("build");
        assert!(fixture.path().join("mock").is_dir());
    }

    #[test]
    fn with_lints_toml_writes_the_file() {
        let body = "[defaults]\nvisibility = \"all\"\n";
        let fixture = MockspaceFixture::new()
            .with_lints_toml(body)
            .build()
            .expect("build");
        let written = std::fs::read_to_string(fixture.path().join("lints.toml")).unwrap();
        assert_eq!(written, body);
    }

    #[test]
    fn with_install_runs_bootstrap_and_creates_alias_plus_hooks() {
        let fixture = MockspaceFixture::new()
            .with_install()
            .build()
            .expect("build");
        let status = bootstrap::status(fixture.path());
        assert!(
            status.has_cargo_alias,
            "with_install should flip has_cargo_alias"
        );
        assert!(status.has_hooks_path, "with_install should flip has_hooks_path");
    }

    #[test]
    fn combining_axes_composes_independently() {
        let fixture = MockspaceFixture::new()
            .with_install()
            .with_lints_toml("[defaults]\nvisibility = \"all\"\n")
            .build()
            .expect("build");
        // Bootstrap installed.
        assert!(bootstrap::status(fixture.path()).has_cargo_alias);
        // User TOML written.
        assert!(fixture.path().join("lints.toml").is_file());
    }

    #[test]
    fn dropping_fixture_removes_the_tempdir() {
        let path: PathBuf;
        {
            let fixture = MockspaceFixture::new().build().expect("build");
            path = fixture.path().to_path_buf();
            assert!(path.is_dir(), "fixture path exists while fixture lives");
        }
        // Dropping the fixture should have removed the directory.
        // tempfile cleans up best-effort; we just assert it's gone.
        assert!(
            !path.exists(),
            "tempdir should be cleaned up after fixture drop, got: {path:?}"
        );
    }
}
