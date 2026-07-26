//! The shared reader of a repo's `mockspace.toml` header and its engine pin.
//!
//! Both the launcher (`cargo-mock`) and the engine (`mockspace`) need to read
//! the same small set of top-level `mockspace.toml` keys: which mock dir the
//! config maps, and which engine version the repo pins. Rather than each
//! hand-rolling that parse (three copies before this crate existed), they both
//! depend on this one crate. It is deliberately tiny (serde + toml only) so the
//! published launcher stays fast to install.
//!
//! What lives here is the *schema and its parse*: the manifest header, the pin
//! form, and the legacy `Cargo.lock` fallback. What does NOT live here is
//! *resolution* (turning a branch pin into a concrete rev via `git ls-remote`,
//! the build cache, TTLs): that is launcher-specific and stays in the launcher.
//!
//! # The pin
//!
//! A flat top-level key in `mockspace.toml`, mirroring the existing scalar keys
//! (not a new table):
//!
//! ```toml
//! mockspace_version = "0.0.0-d05"   # the released engine: git tag AND crates.io
//! ```
//!
//! The released `mockspace_version` is the primary form. For local development
//! the same file accepts explicit `mockspace_rev` / `mockspace_branch` /
//! `mockspace_tag` overrides against an optional `mockspace_git = <url>`.
//! Precedence when several are present: `rev` > `tag` > `version` > `branch`.

pub mod gate;

use serde::Deserialize;

/// The canonical mockspace repository: the default engine source url when a
/// manifest does not set `mockspace_git`.
pub const CANONICAL_URL: &str = "ssh://git@github.com/hiisi-digital/mockspace.git";

/// Which revision of the engine a repo pins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    /// A released version string that is both a git tag and a crates.io
    /// release. The primary form.
    Version(String),
    /// An explicit git rev (dev override).
    Rev(String),
    /// An explicit git branch (dev override); resolution re-resolves it to a rev.
    Branch(String),
    /// An explicit git tag (dev override) that is not a crates.io release.
    Tag(String),
}

/// A pinned engine source: where it lives and which revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub url:       String,
    pub reference: Reference,
}

/// The launcher-relevant top-level keys of a `mockspace.toml`.
///
/// serde reads only top-level (section-less) keys into these fields, so a
/// `mockspace_version` nested inside some later `[table]` is ignored, which is
/// exactly the pin contract's "top-level only" rule. Unknown keys (the many
/// other things a mockspace.toml carries) are ignored.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct ManifestHeader {
    #[serde(default)]
    pub mock_dir:          Option<String>,
    #[serde(default)]
    pub mockspace_git:     Option<String>,
    #[serde(default)]
    pub mockspace_version: Option<String>,
    #[serde(default)]
    pub mockspace_rev:     Option<String>,
    #[serde(default)]
    pub mockspace_branch:  Option<String>,
    #[serde(default)]
    pub mockspace_tag:     Option<String>,
}

impl ManifestHeader {
    /// Parse a `mockspace.toml`'s header. A malformed file yields the default
    /// (all-absent) header rather than an error: callers treat "no keys" and
    /// "unparseable" the same (fall back to the legacy pin, or report a missing
    /// pin), and the file's real validation is the engine's concern.
    pub fn parse(text: &str) -> ManifestHeader {
        toml::from_str(text).unwrap_or_default()
    }

    /// The `mock_dir` value, if a non-empty one is set.
    pub fn mock_dir(&self) -> Option<String> {
        self.mock_dir.clone().filter(|s| !s.is_empty())
    }

    /// The engine pin this header declares, if any. `None` when no non-empty
    /// `mockspace_*` key is present (the repo has no explicit pin yet).
    pub fn pin(&self) -> Option<Pin> {
        let ne = |o: &Option<String>| o.clone().filter(|s| !s.is_empty());
        let url = ne(&self.mockspace_git).unwrap_or_else(|| CANONICAL_URL.to_string());
        let reference = if let Some(r) = ne(&self.mockspace_rev) {
            Reference::Rev(r)
        } else if let Some(t) = ne(&self.mockspace_tag) {
            Reference::Tag(t)
        } else if let Some(v) = ne(&self.mockspace_version) {
            Reference::Version(v)
        } else {
            Reference::Branch(ne(&self.mockspace_branch)?)
        };
        Some(Pin {
            url,
            reference,
        })
    }
}

/// The pin a `mockspace.toml`'s text declares, if any. Convenience over
/// [`ManifestHeader::parse`] + [`ManifestHeader::pin`].
pub fn pin_from_mockspace_toml(text: &str) -> Option<Pin> {
    ManifestHeader::parse(text).pin()
}

/// The legacy pin: the mockspace git rev recorded in a mock workspace's
/// `Cargo.lock`, paired with the url from that same source. Lets a repo that
/// has not adopted an explicit `mockspace_*` pin keep working, and is what
/// `mock migrate` reads to seed the explicit pin.
pub fn pin_from_legacy_lock(lock_text: &str) -> Option<Pin> {
    let lock: CargoLock = toml::from_str(lock_text).ok()?;
    let source = lock
        .package
        .into_iter()
        .find(|p| p.name == "mockspace")
        .and_then(|p| p.source)?;
    // `git+<url>[?query]#<rev>`
    let locator = source.strip_prefix("git+").unwrap_or(&source);
    let (url_part, rev) = locator.rsplit_once('#')?;
    let url = url_part.split('?').next().unwrap_or(url_part).to_string();
    Some(Pin {
        url,
        reference: Reference::Rev(rev.to_string()),
    })
}

#[derive(Deserialize)]
struct CargoLock {
    #[serde(default)]
    package: Vec<LockPackage>,
}

#[derive(Deserialize)]
struct LockPackage {
    name:   String,
    #[serde(default)]
    source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_pin_default_url() {
        let pin =
            pin_from_mockspace_toml("project_name = \"x\"\nmockspace_version = \"0.0.0-d05\"\n")
                .unwrap();
        assert_eq!(pin.url, CANONICAL_URL);
        assert_eq!(pin.reference, Reference::Version("0.0.0-d05".into()));
    }

    #[test]
    fn rev_pin_custom_url() {
        let pin = pin_from_mockspace_toml(
            "mockspace_git = \"ssh://git@example/x.git\"\nmockspace_rev = \"abc123\"\n",
        )
        .unwrap();
        assert_eq!(pin.url, "ssh://git@example/x.git");
        assert_eq!(pin.reference, Reference::Rev("abc123".into()));
    }

    #[test]
    fn rev_overrides_version() {
        let pin =
            pin_from_mockspace_toml("mockspace_version = \"0.0.0-d05\"\nmockspace_rev = \"abc\"\n")
                .unwrap();
        assert_eq!(pin.reference, Reference::Rev("abc".into()));
    }

    #[test]
    fn branch_pin() {
        let pin = pin_from_mockspace_toml("mockspace_branch = \"dev\"\n").unwrap();
        assert_eq!(pin.reference, Reference::Branch("dev".into()));
    }

    #[test]
    fn no_pin_key_is_not_a_pin() {
        assert!(pin_from_mockspace_toml("project_name = \"x\"\n").is_none());
        // a mockspace_version inside a later table is not the top-level pin.
        assert!(pin_from_mockspace_toml("[other]\nmockspace_version = \"1\"\n").is_none());
    }

    #[test]
    fn only_top_level_keys_count() {
        let pin = pin_from_mockspace_toml(
            "mockspace_version = \"0.0.0-d05\"\n[table]\nmockspace_version = \"9\"\n",
        )
        .unwrap();
        assert_eq!(pin.reference, Reference::Version("0.0.0-d05".into()));
    }

    #[test]
    fn mock_dir_field() {
        assert_eq!(
            ManifestHeader::parse("mock_dir = \"mock\"\n").mock_dir(),
            Some("mock".to_string())
        );
        assert_eq!(ManifestHeader::parse("project = \"x\"\n").mock_dir(), None);
        // empty is treated as absent.
        assert_eq!(ManifestHeader::parse("mock_dir = \"\"\n").mock_dir(), None);
    }

    #[test]
    fn legacy_lock_rev() {
        let lock = "\
[[package]]
name = \"other\"
version = \"1.0\"

[[package]]
name = \"mockspace\"
version = \"0.1.0\"
source = \"git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#deadbeef1234\"
";
        let pin = pin_from_legacy_lock(lock).unwrap();
        assert_eq!(pin.url, "ssh://git@github.com/hiisi-digital/mockspace.git");
        assert_eq!(pin.reference, Reference::Rev("deadbeef1234".into()));
    }

    #[test]
    fn legacy_lock_without_mockspace_is_none() {
        let lock = "[[package]]\nname = \"other\"\nversion = \"1.0\"\n";
        assert!(pin_from_legacy_lock(lock).is_none());
    }

    #[test]
    fn malformed_toml_yields_absent_header() {
        // Intentional divergence from the old line-scanner: a syntax error
        // anywhere makes the whole header absent (no pin, no mock_dir), so a
        // broken file falls back rather than half-parsing. A valid top-level
        // pin followed by a broken table is lost. This is by design; the test
        // locks it so the behavior is a decision, not an accident.
        let broken = "mockspace_version = \"0.0.0-d05\"\n[table\nbad = ";
        let header = ManifestHeader::parse(broken);
        assert!(header.pin().is_none());
        assert!(header.mock_dir().is_none());
    }
}
