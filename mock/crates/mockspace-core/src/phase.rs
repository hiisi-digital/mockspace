//! The six-phase state machine (spec §14).
//!
//! A round always carries exactly one current phase. Phase identity drives
//! every command's semantics: `mock phase apply` does different things in
//! PLAN(DOC) versus PLAN(SRC).

use core::fmt;

use serde::{Deserialize, Serialize};

/// The phase a round is currently in.
///
/// Phases progress forward via `plan` / `apply` / `finish`, and backward via
/// `replan` (which is always deprecating). See spec §15 for transition rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// Free-form exploration. Topic files, sketches, benches, research.
    /// No manifest exists. Exit via `mock phase plan`.
    Topic,
    /// Doc-manifest authoring. The manifest is mutable. Exit via `mock phase apply`.
    PlanDoc,
    /// Doc execution. Manifest sealed, doc templates edited per its claims.
    /// Verifier runs every commit. Exit via `mock phase finish` or `replan`.
    ApplyDoc,
    /// Src-manifest authoring. Same shape as PlanDoc; src side.
    PlanSrc,
    /// Src execution. Manifest sealed, source files edited.
    /// design-doc-source-mismatch runs across the project.
    ApplySrc,
    /// Round closed. PR comments may still be ingested. Exit via `mock close`.
    Done,
}

impl Phase {
    /// Returns `true` if a doc-side manifest exists for this phase.
    pub const fn has_doc_manifest(self) -> bool {
        !matches!(self, Self::Topic)
    }

    /// Returns `true` if a src-side manifest exists for this phase.
    pub const fn has_src_manifest(self) -> bool {
        matches!(self, Self::PlanSrc | Self::ApplySrc | Self::Done)
    }

    /// Returns `true` if the round is in an APPLY phase (manifest sealed,
    /// surface edits active, verifier runs every commit).
    pub const fn is_apply(self) -> bool {
        matches!(self, Self::ApplyDoc | Self::ApplySrc)
    }

    /// Returns `true` if the round is in a PLAN phase (manifest mutable).
    pub const fn is_plan(self) -> bool {
        matches!(self, Self::PlanDoc | Self::PlanSrc)
    }

    /// The string form used in the round mock-ref's `.phase` marker file.
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Topic => "topic",
            Self::PlanDoc => "plan_doc",
            Self::ApplyDoc => "apply_doc",
            Self::PlanSrc => "plan_src",
            Self::ApplySrc => "apply_src",
            Self::Done => "done",
        }
    }

    /// Parse a `.phase` marker string back to a [`Phase`].
    pub fn from_marker(s: &str) -> Option<Self> {
        Some(match s {
            "topic" => Self::Topic,
            "plan_doc" => Self::PlanDoc,
            "apply_doc" => Self::ApplyDoc,
            "plan_src" => Self::PlanSrc,
            "apply_src" => Self::ApplySrc,
            "done" => Self::Done,
            _ => return None,
        })
    }

    /// The manifest side this phase operates on, if any.
    ///
    /// TOPIC has no manifest. DONE has both sealed but is not authoring either.
    /// The four middle phases each operate on exactly one side.
    pub const fn manifest_side(self) -> Option<ManifestSide> {
        match self {
            Self::Topic | Self::Done => None,
            Self::PlanDoc | Self::ApplyDoc => Some(ManifestSide::Doc),
            Self::PlanSrc | Self::ApplySrc => Some(ManifestSide::Src),
        }
    }
}

/// Which side of a round a manifest or anchor belongs to.
///
/// Manifests come in two shapes: `manifest.doc.toml` for the doc phase and
/// `manifest.src.toml` for the src phase. Anchors mirror the split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManifestSide {
    /// Doc side: templates, topic files, design surfaces.
    Doc,
    /// Src side: source code edited per the locked doc manifest.
    Src,
}

impl ManifestSide {
    /// The string form used in file names (`manifest.<side>.toml`, `.anchor.<side>.toml`).
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Doc => "doc",
            Self::Src => "src",
        }
    }
}

impl fmt::Display for ManifestSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.marker())
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.marker())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_roundtrips() {
        for phase in [
            Phase::Topic,
            Phase::PlanDoc,
            Phase::ApplyDoc,
            Phase::PlanSrc,
            Phase::ApplySrc,
            Phase::Done,
        ] {
            assert_eq!(Phase::from_marker(phase.marker()), Some(phase));
        }
    }

    #[test]
    fn from_marker_rejects_unknown() {
        assert_eq!(Phase::from_marker(""), None);
        assert_eq!(Phase::from_marker("PLAN_DOC"), None);
        assert_eq!(Phase::from_marker("apply"), None);
    }

    #[test]
    fn manifest_presence() {
        assert!(!Phase::Topic.has_doc_manifest());
        assert!(Phase::PlanDoc.has_doc_manifest());
        assert!(Phase::Done.has_doc_manifest());

        assert!(!Phase::Topic.has_src_manifest());
        assert!(!Phase::ApplyDoc.has_src_manifest());
        assert!(Phase::PlanSrc.has_src_manifest());
        assert!(Phase::Done.has_src_manifest());
    }

    #[test]
    fn apply_plan_classification() {
        assert!(Phase::ApplyDoc.is_apply());
        assert!(Phase::ApplySrc.is_apply());
        assert!(!Phase::PlanDoc.is_apply());

        assert!(Phase::PlanDoc.is_plan());
        assert!(Phase::PlanSrc.is_plan());
        assert!(!Phase::Topic.is_plan());
        assert!(!Phase::Done.is_plan());
    }

    #[test]
    fn manifest_side_routing() {
        assert_eq!(Phase::Topic.manifest_side(), None);
        assert_eq!(Phase::PlanDoc.manifest_side(), Some(ManifestSide::Doc));
        assert_eq!(Phase::ApplyDoc.manifest_side(), Some(ManifestSide::Doc));
        assert_eq!(Phase::PlanSrc.manifest_side(), Some(ManifestSide::Src));
        assert_eq!(Phase::ApplySrc.manifest_side(), Some(ManifestSide::Src));
        assert_eq!(Phase::Done.manifest_side(), None);
    }

    #[test]
    fn manifest_side_markers() {
        assert_eq!(ManifestSide::Doc.marker(), "doc");
        assert_eq!(ManifestSide::Src.marker(), "src");
    }
}
