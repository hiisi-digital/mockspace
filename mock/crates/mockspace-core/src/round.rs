//! Round tree layout (spec §25).
//!
//! Each round mock-side ref carries a flat tree of named entries:
//!
//! - `.phase` and `.anchor.*`: bookkeeping (filtered from `.mock/`; see
//!   [`crate::bookkeeping`]). No `.meta` file in v2; all metadata lives in
//!   `round.toml`.
//! - `round.toml`: round metadata (see [`RoundMeta`]).
//! - `manifest.<side>.toml` and friends: manifests across their lifecycle
//!   (see [`ManifestStage`]).
//! - `<NN>_topic.<name>.md`: topic files (see [`topic_filename`]).
//! - `comments/<NNN>-<author>-<timestamp>.md`: ingested PR comments after
//!   DONE (see [`comment_filename`]).

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::phase::ManifestSide;

/// Lifecycle stage of a manifest file within a round ref tree.
///
/// A round may carry multiple manifests for a single side over its
/// lifetime: one active (or sealed) manifest plus N deprecated iterations
/// produced by replan transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManifestStage {
    /// Authoring; mutable. File name: `manifest.<side>.toml`.
    Authoring,
    /// Sealed; immutable. File name: `manifest.<side>.locked.toml`.
    Locked,
    /// Deprecated by `mock phase replan`, with the iteration number.
    /// File name: `manifest.<side>.deprecated.<n>.toml`.
    /// `n` is 1-indexed; the first replan produces `deprecated.1.toml`.
    Deprecated(u32),
}

impl ManifestStage {
    /// Compose the manifest file name for this stage and side.
    pub fn filename(self, side: ManifestSide) -> String {
        match self {
            Self::Authoring => format!("manifest.{}.toml", side.marker()),
            Self::Locked => format!("manifest.{}.locked.toml", side.marker()),
            Self::Deprecated(n) => {
                format!("manifest.{}.deprecated.{n}.toml", side.marker())
            },
        }
    }
}

impl fmt::Display for ManifestStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authoring => f.write_str("authoring"),
            Self::Locked => f.write_str("locked"),
            Self::Deprecated(n) => write!(f, "deprecated.{n}"),
        }
    }
}

/// Compose a topic file leaf name: `<NN>_topic.<name>.md`.
///
/// `sequence` is zero-padded to two digits; first topic is `01`. The
/// returned string is a leaf name, not a path; topic files live at the
/// round tree root and need no directory prefix.
pub fn topic_filename(sequence: u32, name: &str) -> String {
    format!("{sequence:02}_topic.{name}.md")
}

/// Compose a comment file leaf name: `<NNN>-<author>-<timestamp>.md`.
///
/// `sequence` is zero-padded to three digits. The returned string is a
/// leaf name; comments live under the `comments/` subdirectory of the
/// round tree and the caller composes the full path.
pub fn comment_filename(sequence: u32, author: &str, timestamp: &str) -> String {
    format!("{sequence:03}-{author}-{timestamp}.md")
}

/// PR integration metadata recorded in `round.toml` under `[pr]`.
///
/// Both fields are filled by mockspace after a successful PR creation;
/// before then they are absent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrMeta {
    /// PR number assigned by the host (e.g. GitHub issue/PR sequence).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<u64>,
    /// Direct URL to the PR.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url:    Option<String>,
}

impl PrMeta {
    /// True when neither field has been populated.
    pub fn is_empty(&self) -> bool {
        self.number.is_none() && self.url.is_none()
    }
}

/// Closure metadata written into `round.toml` under `[closed]` when
/// `mock close` transitions the round to DONE.
///
/// Audit-trail facts that the orphan ref's own commit metadata records but
/// that are convenient to surface at archive time without re-walking history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosedMeta {
    /// ISO-8601 timestamp at which the round transitioned to DONE.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at:           Option<String>,
    /// Source-side branch tip SHA at the moment of closure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_source_sha:    Option<String>,
    /// Original mock-side ref name before archival (e.g.
    /// `refs/mock/round/202605181400-arvo-graph-csr`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_mock_ref:   Option<String>,
    /// Original source-side branch ref name before archival.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_source_ref: Option<String>,
}

impl ClosedMeta {
    /// True when no closure fields have been populated.
    pub fn is_empty(&self) -> bool {
        self.closed_at.is_none()
            && self.final_source_sha.is_none()
            && self.original_mock_ref.is_none()
            && self.original_source_ref.is_none()
    }
}

/// The `round.toml` document persisted at the round mock-ref tree root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoundMeta {
    /// Schema version.
    pub mockspace_version: String,
    /// Round slug (matches the ref name and the source-side branch suffix).
    pub slug:              String,
    /// One-line human-facing title.
    pub title:             String,
    /// ISO-8601 creation timestamp.
    pub created:           String,
    /// Source-side branch this round pairs with (e.g.
    /// `round/202605181400-arvo-graph-csr`).
    pub source_branch:     String,
    /// PR integration data (number + URL). Populated after PR creation.
    #[serde(default, skip_serializing_if = "PrMeta::is_empty")]
    pub pr:                PrMeta,
    /// Closure metadata. Populated by `mock close`; absent until then.
    #[serde(default, skip_serializing_if = "ClosedMeta::is_empty")]
    pub closed:            ClosedMeta,
}

impl RoundMeta {
    /// The standard filename for this document at the round tree root.
    pub const FILENAME: &'static str = "round.toml";

    /// Serialize as TOML.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Parse from TOML.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_filenames_authoring() {
        assert_eq!(
            ManifestStage::Authoring.filename(ManifestSide::Doc),
            "manifest.doc.toml"
        );
        assert_eq!(
            ManifestStage::Authoring.filename(ManifestSide::Src),
            "manifest.src.toml"
        );
    }

    #[test]
    fn manifest_filenames_locked() {
        assert_eq!(
            ManifestStage::Locked.filename(ManifestSide::Doc),
            "manifest.doc.locked.toml"
        );
        assert_eq!(
            ManifestStage::Locked.filename(ManifestSide::Src),
            "manifest.src.locked.toml"
        );
    }

    #[test]
    fn manifest_filenames_deprecated() {
        assert_eq!(
            ManifestStage::Deprecated(1).filename(ManifestSide::Doc),
            "manifest.doc.deprecated.1.toml"
        );
        assert_eq!(
            ManifestStage::Deprecated(7).filename(ManifestSide::Src),
            "manifest.src.deprecated.7.toml"
        );
    }

    #[test]
    fn topic_filename_pads_sequence() {
        assert_eq!(
            topic_filename(1, "kit-trait-split"),
            "01_topic.kit-trait-split.md"
        );
        assert_eq!(topic_filename(12, "corrective"), "12_topic.corrective.md");
    }

    #[test]
    fn comment_filename_pads_sequence() {
        assert_eq!(
            comment_filename(1, "reviewer1", "20260518T1430Z"),
            "001-reviewer1-20260518T1430Z.md"
        );
        assert_eq!(
            comment_filename(42, "author", "20260518T1430Z"),
            "042-author-20260518T1430Z.md"
        );
    }

    #[test]
    fn round_meta_round_trip() {
        let meta = RoundMeta {
            mockspace_version: "1.0".to_owned(),
            slug:              "202605181400-arvo-graph-csr".to_owned(),
            title:             "arvo-graph storage layout (CSR vs dense matrix)".to_owned(),
            created:           "2026-05-18T14:00:00Z".to_owned(),
            source_branch:     "round/202605181400-arvo-graph-csr".to_owned(),
            pr:                PrMeta {
                number: Some(437),
                url:    Some("https://github.com/orgrinrt/arvo/pull/437".to_owned()),
            },
            closed:            ClosedMeta::default(),
        };

        let serialized = meta.to_toml().unwrap();
        assert!(serialized.contains("slug = \"202605181400-arvo-graph-csr\""));
        assert!(serialized.contains("[pr]"));
        assert!(serialized.contains("number = 437"));
        assert!(!serialized.contains("[closed]"));
        let parsed = RoundMeta::from_toml(&serialized).unwrap();
        assert_eq!(parsed, meta);
    }

    #[test]
    fn round_meta_round_trip_no_pr() {
        let meta = RoundMeta {
            mockspace_version: "1.0".to_owned(),
            slug:              "202605181400-arvo-graph-csr".to_owned(),
            title:             "test title".to_owned(),
            created:           "2026-05-18T14:00:00Z".to_owned(),
            source_branch:     "round/202605181400-arvo-graph-csr".to_owned(),
            pr:                PrMeta::default(),
            closed:            ClosedMeta::default(),
        };

        let serialized = meta.to_toml().unwrap();
        assert!(!serialized.contains("[pr]"));
        assert!(!serialized.contains("[closed]"));
        let parsed = RoundMeta::from_toml(&serialized).unwrap();
        assert_eq!(parsed, meta);
    }

    #[test]
    fn round_meta_round_trip_with_closure() {
        let meta = RoundMeta {
            mockspace_version: "1.0".to_owned(),
            slug:              "202605181400-arvo-graph-csr".to_owned(),
            title:             "closed round".to_owned(),
            created:           "2026-05-18T14:00:00Z".to_owned(),
            source_branch:     "round/202605181400-arvo-graph-csr".to_owned(),
            pr:                PrMeta {
                number: Some(437),
                url:    Some("https://github.com/orgrinrt/arvo/pull/437".to_owned()),
            },
            closed:            ClosedMeta {
                closed_at:           Some("2026-05-19T18:30:00Z".to_owned()),
                final_source_sha:    Some("deadbeefcafebabe0000000000000000deadbeef".to_owned()),
                original_mock_ref:   Some("refs/mock/round/202605181400-arvo-graph-csr".to_owned()),
                original_source_ref: Some(
                    "refs/heads/round/202605181400-arvo-graph-csr".to_owned(),
                ),
            },
        };

        let serialized = meta.to_toml().unwrap();
        assert!(serialized.contains("[pr]"));
        assert!(serialized.contains("[closed]"));
        assert!(serialized.contains("closed_at = \"2026-05-19T18:30:00Z\""));
        let parsed = RoundMeta::from_toml(&serialized).unwrap();
        assert_eq!(parsed, meta);
    }

    #[test]
    fn pr_meta_default_is_empty() {
        assert!(PrMeta::default().is_empty());
        assert!(
            !PrMeta {
                number: Some(1),
                url:    None,
            }
            .is_empty()
        );
    }

    #[test]
    fn closed_meta_default_is_empty() {
        assert!(ClosedMeta::default().is_empty());
        assert!(
            !ClosedMeta {
                closed_at: Some("2026-05-19T18:30:00Z".to_owned()),
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn round_meta_filename_constant() {
        assert_eq!(RoundMeta::FILENAME, "round.toml");
    }
}
