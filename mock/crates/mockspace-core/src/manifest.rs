//! Manifest model (spec §17, §53).
//!
//! A manifest is the structured contract a round seals at APPLY entry. Two
//! manifests per round: `manifest.doc.toml` for the doc phase and
//! `manifest.src.toml` for the src phase. Same shape; different scope.
//!
//! Phase 1 ships the data types, TOML serde, and structural lifecycle
//! helpers. Seal-time structural validation lives next door in this same
//! module. IO-dependent validation (task ref resolution, file existence,
//! verifier execution against a worktree) lives in Phase 5.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::phase::ManifestSide;
use crate::task::{StepRef, StepRefError, TaskId, TaskIdError};
use crate::verifier::VerifierCheck;

/// URI prefix every task or step reference in a manifest must carry.
pub const TASK_URI_PREFIX: &str = "mock://task/";

/// Errors a manifest can trip at structural seal-time validation.
///
/// Structural validation is the IO-free subset of the seal-time validation
/// described in spec §53. Rules that require git ops or worktree access
/// (task ref resolution, file existence, verifier execution) live in
/// Phase 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// `mockspace_version` did not parse as `<major>.<minor>`.
    VersionMalformed {
        version: String,
    },
    /// `mockspace_version` major does not match what this loader supports.
    SchemaVersionMismatch {
        expected_major: u32,
        found_major:    u32,
    },
    /// `phase` does not match the expected side (e.g. caller is sealing
    /// the doc manifest but parsed an `src` manifest).
    PhaseMismatch {
        expected: ManifestSide,
        found:    ManifestSide,
    },
    /// `round_slug` was empty.
    EmptyRoundSlug,
    /// A `scope.in_scope_tasks` or `[[change]].task` URI failed grammar
    /// or the `mock://task/` prefix is missing.
    InvalidTaskUri {
        uri:    String,
        reason: TaskUriError,
    },
    /// `deprecated_accounting` does not cover every file from the
    /// deprecated predecessor.
    DeprecatedAccountingIncomplete {
        missing_files: Vec<PathBuf>,
    },
}

/// Why a `mock://task/...` URI rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskUriError {
    /// URI did not start with `mock://task/`.
    MissingPrefix,
    /// Task identity (the part between prefix and `#`) was empty.
    EmptyIdentity,
    /// Task-half failed `<seg>::<seg>::...::<slug>` grammar.
    InvalidTask(TaskIdError),
    /// Step-form URI failed `<task>#<step>` grammar.
    InvalidStep(StepRefError),
}

/// Major schema version this loader supports.
///
/// Matches the contract in [`crate::manifest`] with the wider mockspace
/// stack: schema-version compatibility is enforced on the major prefix,
/// minor advances are forward-compatible.
pub const SCHEMA_MAJOR: u32 = 1;

/// Parse a manifest task URI of the form `mock://task/<path>` or
/// `mock://task/<path>#<step>` and return the underlying [`TaskId`] (and,
/// when a step was named, the [`StepRef`]).
pub fn parse_task_uri(uri: &str) -> Result<(TaskId, Option<StepRef>), TaskUriError> {
    let body = uri
        .strip_prefix(TASK_URI_PREFIX)
        .ok_or(TaskUriError::MissingPrefix)?;
    if body.is_empty() {
        return Err(TaskUriError::EmptyIdentity);
    }
    if body.contains('#') {
        let step_ref = StepRef::parse(body).map_err(TaskUriError::InvalidStep)?;
        let task = step_ref.task().clone();
        Ok((task, Some(step_ref)))
    } else {
        let task = TaskId::parse(body).map_err(TaskUriError::InvalidTask)?;
        Ok((task, None))
    }
}

/// Structural seal-time validation (spec §53 rules 1-3, partial 10).
///
/// Covers the IO-free portion of the validation contract: schema-version
/// compatibility, manifest-phase routing, non-empty round slug, well-formed
/// task URIs in scope and per-change blocks.
///
/// The IO-dependent rules (4-9: task ref resolution, file existence,
/// step phase tag alignment, step state, verifier execution) live in
/// Phase 5 alongside the worktree machinery.
///
/// Deprecated accounting completeness (rule 10) is checked separately by
/// [`validate_deprecated_accounting`] because it requires the deprecated
/// manifest's file list as input.
pub fn validate_structural(
    manifest: &Manifest,
    expected_phase: ManifestSide,
) -> Result<(), ValidationError> {
    let major = parse_major(&manifest.mockspace_version).ok_or_else(|| {
        ValidationError::VersionMalformed {
            version: manifest.mockspace_version.clone(),
        }
    })?;
    if major != SCHEMA_MAJOR {
        return Err(ValidationError::SchemaVersionMismatch {
            expected_major: SCHEMA_MAJOR,
            found_major:    major,
        });
    }
    if manifest.phase != expected_phase {
        return Err(ValidationError::PhaseMismatch {
            expected: expected_phase,
            found:    manifest.phase,
        });
    }
    if manifest.round_slug.is_empty() {
        return Err(ValidationError::EmptyRoundSlug);
    }
    for uri in &manifest.scope.in_scope_tasks {
        parse_task_uri(uri).map_err(|reason| {
            ValidationError::InvalidTaskUri {
                uri: uri.clone(),
                reason,
            }
        })?;
    }
    for change in &manifest.changes {
        if let Some(uri) = &change.task {
            parse_task_uri(uri).map_err(|reason| {
                ValidationError::InvalidTaskUri {
                    uri: uri.clone(),
                    reason,
                }
            })?;
        }
    }
    Ok(())
}

/// Deprecated-accounting completeness check (spec §53 rule 10).
///
/// Every file from the deprecated predecessor manifest must appear EITHER
/// as a `[[change]].file` in the new manifest OR in
/// `[[deprecated_accounting]]` with an `omitted_reason`.
///
/// Caller passes the deprecated manifest's `[[change]].file` list. Path
/// canonicalisation (resolving `..`, `.`, symlinks) is a Phase 5 concern
/// and happens before this function is called; here we compare paths
/// byte-identically.
pub fn validate_deprecated_accounting(
    manifest: &Manifest,
    deprecated_files: &[PathBuf],
) -> Result<(), ValidationError> {
    let covered: BTreeSet<&PathBuf> = manifest
        .changes
        .iter()
        .map(|c| &c.file)
        .chain(manifest.deprecated_accounting.iter().map(|d| &d.file))
        .collect();
    let mut missing: Vec<PathBuf> = deprecated_files
        .iter()
        .filter(|f| !covered.contains(f))
        .cloned()
        .collect();
    if !missing.is_empty() {
        missing.sort();
        return Err(ValidationError::DeprecatedAccountingIncomplete {
            missing_files: missing,
        });
    }
    Ok(())
}

fn parse_major(version: &str) -> Option<u32> {
    version.split('.').next()?.parse().ok()
}

/// A manifest is the structured contract a round seals at APPLY entry.
///
/// The mutable form lives at `manifest.<side>.toml`; on seal it is renamed
/// to `manifest.<side>.locked.toml`; if a replan invalidates it, it is
/// renamed `manifest.<side>.deprecated.<n>.toml`. See [`crate::round::ManifestStage`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Schema version. Loaders match on the major component.
    pub mockspace_version:     String,
    /// Slug of the round this manifest belongs to.
    pub round_slug:            String,
    /// Which side this manifest covers (`doc` or `src`).
    pub phase:                 ManifestSide,
    /// What is in scope; what is explicitly out.
    pub scope:                 ScopeBlock,
    /// Completion criteria.
    pub acceptance:            AcceptanceBlock,
    /// Per-file change blocks. Serialised under `[[change]]`.
    #[serde(default, rename = "change", skip_serializing_if = "Vec::is_empty")]
    pub changes:               Vec<ChangeBlock>,
    /// Accounting of files dropped from a previous, now-deprecated manifest.
    /// Required (and only meaningful) when superseding a deprecated manifest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deprecated_accounting: Vec<DeprecatedAccounting>,
}

/// What the manifest covers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeBlock {
    /// Prose summary of what is in scope.
    pub description:    String,
    /// Task or step URIs (`mock://task/<path>[#<step-key>]`) the manifest
    /// claims. A bare task URI scopes the manifest to the task as a whole;
    /// a step URI scopes to that step only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub in_scope_tasks: Vec<String>,
    /// Concerns explicitly excluded from this manifest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub out_of_scope:   Vec<String>,
}

/// Completion criteria for the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceBlock {
    /// Free-form prose stating what "done" means.
    pub criteria: String,
}

/// A per-file change claim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeBlock {
    /// Task or step URI this change resolves (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task:        Option<String>,
    /// File path the change touches, relative to the source-side worktree root.
    pub file:        PathBuf,
    /// One-paragraph summary of the change.
    pub description: String,
    /// Structural verifier check that must pass against the source-side
    /// branch tip at seal time. See [`crate::verifier`].
    pub verify:      VerifierCheck,
}

/// Accounting for a file present in a deprecated predecessor manifest but
/// dropped from the superseding manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeprecatedAccounting {
    /// File path that appeared in the deprecated manifest.
    pub file:           PathBuf,
    /// Why the file is no longer claimed.
    pub omitted_reason: String,
}

impl Manifest {
    /// Serialize as pretty TOML.
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
    use crate::verifier::{VerifierAllOf, VerifierKind};

    /// The full spec §53 example, abridged but covering every section.
    const SPEC_EXAMPLE: &str = r###"
mockspace_version = "1.0"
round_slug = "202605181400-arvo-graph-csr"
phase = "doc"

[scope]
description = "Add CSR backend to arvo-graph; deprecate dense-matrix variant."
in_scope_tasks = [
  "mock://task/arvo::graph::csr-backend",
  "mock://task/arvo::graph::dense-matrix-deprecation",
]
out_of_scope = [
  "Renaming the graph crate (separate concern).",
]

[acceptance]
criteria = """
1. CSR backend implements the same trait surface as dense-matrix.
2. Bench `structural-decomposition` shows CSR >= 25% faster at n>=512.
"""

[[change]]
task = "mock://task/arvo::graph::csr-backend"
file = "crates/arvo-graph/DESIGN.md"
description = "Add CSR backend section; promote CSR to default."
[change.verify]
all_of = [
  { kind = "grep_present", pattern = "## CSR backend", file = "crates/arvo-graph/DESIGN.md" },
  { kind = "grep_present", pattern = "default backend", file = "crates/arvo-graph/DESIGN.md" },
]

[[change]]
file = "crates/arvo-graph/src/csr.rs"
description = "Implement CSR backend struct."
[change.verify]
kind = "path_exists"
file = "crates/arvo-graph/src/csr.rs"

[[deprecated_accounting]]
file = "crates/arvo-graph/src/old_helper.rs"
omitted_reason = "Concept removed entirely; the file no longer applies."
"###;

    #[test]
    fn parses_full_spec_example() {
        let manifest = Manifest::from_toml(SPEC_EXAMPLE).expect("parse manifest");
        assert_eq!(manifest.mockspace_version, "1.0");
        assert_eq!(manifest.round_slug, "202605181400-arvo-graph-csr");
        assert_eq!(manifest.phase, ManifestSide::Doc);

        assert!(manifest.scope.description.starts_with("Add CSR"));
        assert_eq!(manifest.scope.in_scope_tasks.len(), 2);
        assert_eq!(manifest.scope.out_of_scope.len(), 1);

        assert!(manifest.acceptance.criteria.contains("CSR backend"));

        assert_eq!(manifest.changes.len(), 2);
        let c0 = &manifest.changes[0];
        assert_eq!(
            c0.task.as_deref(),
            Some("mock://task/arvo::graph::csr-backend")
        );
        assert_eq!(c0.file, PathBuf::from("crates/arvo-graph/DESIGN.md"));
        assert!(matches!(c0.verify, VerifierCheck::AllOf(_)));

        let c1 = &manifest.changes[1];
        assert!(c1.task.is_none());
        assert!(matches!(
            c1.verify,
            VerifierCheck::Kind(VerifierKind::PathExists { .. })
        ));

        assert_eq!(manifest.deprecated_accounting.len(), 1);
        assert_eq!(
            manifest.deprecated_accounting[0].file,
            PathBuf::from("crates/arvo-graph/src/old_helper.rs")
        );
    }

    #[test]
    fn round_trip_preserves_values() {
        let original = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        let serialised = original.to_toml().unwrap();
        let reparsed = Manifest::from_toml(&serialised).unwrap();
        assert_eq!(original, reparsed);
    }

    #[test]
    fn parses_minimal_manifest_no_changes() {
        let toml = r#"
mockspace_version = "1.0"
round_slug = "202605181400-test"
phase = "src"

[scope]
description = ""

[acceptance]
criteria = ""
"#;
        let manifest = Manifest::from_toml(toml).expect("parse minimal");
        assert_eq!(manifest.phase, ManifestSide::Src);
        assert!(manifest.changes.is_empty());
        assert!(manifest.deprecated_accounting.is_empty());
        assert!(manifest.scope.in_scope_tasks.is_empty());
    }

    #[test]
    fn rejects_missing_round_slug() {
        let toml = r#"
mockspace_version = "1.0"
phase = "doc"

[scope]
description = ""

[acceptance]
criteria = ""
"#;
        let err = Manifest::from_toml(toml).unwrap_err();
        assert!(err.to_string().contains("round_slug"));
    }

    #[test]
    fn rejects_invalid_phase() {
        let toml = r#"
mockspace_version = "1.0"
round_slug = "x"
phase = "topic"

[scope]
description = ""

[acceptance]
criteria = ""
"#;
        let err = Manifest::from_toml(toml).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("topic") || msg.contains("variant"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn change_block_without_task_serialises_without_field() {
        let manifest = Manifest {
            mockspace_version:     "1.0".to_owned(),
            round_slug:            "test".to_owned(),
            phase:                 ManifestSide::Src,
            scope:                 ScopeBlock {
                description:    "".to_owned(),
                in_scope_tasks: vec![],
                out_of_scope:   vec![],
            },
            acceptance:            AcceptanceBlock {
                criteria: "".to_owned(),
            },
            changes:               vec![ChangeBlock {
                task:        None,
                file:        PathBuf::from("src/lib.rs"),
                description: "Just a change.".to_owned(),
                verify:      VerifierCheck::Kind(VerifierKind::PathExists {
                    file: PathBuf::from("src/lib.rs"),
                }),
            }],
            deprecated_accounting: vec![],
        };
        let serialised = manifest.to_toml().unwrap();
        assert!(!serialised.contains("task ="));
        let reparsed = Manifest::from_toml(&serialised).unwrap();
        assert_eq!(reparsed, manifest);
    }

    #[test]
    fn change_block_with_step_uri() {
        let toml = r#"
mockspace_version = "1.0"
round_slug = "test"
phase = "doc"

[scope]
description = ""

[acceptance]
criteria = ""

[[change]]
task = "mock://task/arvo::graph::csr-backend#define-grammar"
file = "DESIGN.md"
description = "Step-scoped change."
[change.verify]
kind = "path_exists"
file = "DESIGN.md"
"#;
        let manifest = Manifest::from_toml(toml).unwrap();
        assert_eq!(
            manifest.changes[0].task.as_deref(),
            Some("mock://task/arvo::graph::csr-backend#define-grammar")
        );
    }

    #[test]
    fn deprecated_accounting_round_trip() {
        let manifest = Manifest {
            mockspace_version:     "1.0".to_owned(),
            round_slug:            "test".to_owned(),
            phase:                 ManifestSide::Doc,
            scope:                 ScopeBlock {
                description:    "".to_owned(),
                in_scope_tasks: vec![],
                out_of_scope:   vec![],
            },
            acceptance:            AcceptanceBlock {
                criteria: "".to_owned(),
            },
            changes:               vec![],
            deprecated_accounting: vec![
                DeprecatedAccounting {
                    file:           PathBuf::from("a.rs"),
                    omitted_reason: "no longer applies".to_owned(),
                },
                DeprecatedAccounting {
                    file:           PathBuf::from("b.rs"),
                    omitted_reason: "moved to other manifest".to_owned(),
                },
            ],
        };
        let serialised = manifest.to_toml().unwrap();
        assert!(serialised.contains("[[deprecated_accounting]]"));
        let reparsed = Manifest::from_toml(&serialised).unwrap();
        assert_eq!(reparsed, manifest);
    }

    #[test]
    fn parse_task_uri_bare() {
        let (task, step) = parse_task_uri("mock://task/arvo::graph::csr-backend").unwrap();
        assert_eq!(task.as_uri_form(), "arvo::graph::csr-backend");
        assert!(step.is_none());
    }

    #[test]
    fn parse_task_uri_with_step() {
        let (task, step) =
            parse_task_uri("mock://task/arvo::graph::csr-backend#define-grammar").unwrap();
        assert_eq!(task.as_uri_form(), "arvo::graph::csr-backend");
        let step = step.expect("step present");
        assert_eq!(step.step(), "define-grammar");
    }

    #[test]
    fn parse_task_uri_single_segment_top_level() {
        let (task, step) = parse_task_uri("mock://task/migrate-to-codeberg").unwrap();
        assert!(task.is_top_level());
        assert!(step.is_none());
    }

    #[test]
    fn parse_task_uri_missing_prefix() {
        let err = parse_task_uri("arvo::graph::csr-backend").unwrap_err();
        assert_eq!(err, TaskUriError::MissingPrefix);
    }

    #[test]
    fn parse_task_uri_empty_identity() {
        let err = parse_task_uri("mock://task/").unwrap_err();
        assert_eq!(err, TaskUriError::EmptyIdentity);
    }

    #[test]
    fn validate_structural_accepts_spec_example() {
        let manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        validate_structural(&manifest, ManifestSide::Doc).expect("valid");
    }

    #[test]
    fn validate_structural_rejects_phase_mismatch() {
        let manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        let err = validate_structural(&manifest, ManifestSide::Src).unwrap_err();
        assert!(matches!(err, ValidationError::PhaseMismatch {
            expected: ManifestSide::Src,
            found:    ManifestSide::Doc,
        }));
    }

    #[test]
    fn validate_structural_rejects_empty_round_slug() {
        let mut manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        manifest.round_slug = String::new();
        let err = validate_structural(&manifest, ManifestSide::Doc).unwrap_err();
        assert_eq!(err, ValidationError::EmptyRoundSlug);
    }

    #[test]
    fn validate_structural_rejects_malformed_version() {
        let mut manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        manifest.mockspace_version = "notaversion".to_owned();
        let err = validate_structural(&manifest, ManifestSide::Doc).unwrap_err();
        assert!(matches!(err, ValidationError::VersionMalformed { .. }));
    }

    #[test]
    fn validate_structural_rejects_unsupported_major() {
        let mut manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        manifest.mockspace_version = "99.0".to_owned();
        let err = validate_structural(&manifest, ManifestSide::Doc).unwrap_err();
        assert!(matches!(err, ValidationError::SchemaVersionMismatch {
            expected_major: 1,
            found_major:    99,
        }));
    }

    #[test]
    fn validate_structural_rejects_bad_task_uri_in_scope() {
        let mut manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        manifest.scope.in_scope_tasks = vec!["arvo::graph::csr-backend".to_owned()];
        let err = validate_structural(&manifest, ManifestSide::Doc).unwrap_err();
        match err {
            ValidationError::InvalidTaskUri {
                uri,
                reason,
            } => {
                assert_eq!(uri, "arvo::graph::csr-backend");
                assert_eq!(reason, TaskUriError::MissingPrefix);
            },
            other => panic!("expected invalid task URI, got {other:?}"),
        }
    }

    #[test]
    fn validate_structural_rejects_bad_task_uri_in_change_block() {
        let mut manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        manifest.changes[0].task = Some("mock://task/".to_owned());
        let err = validate_structural(&manifest, ManifestSide::Doc).unwrap_err();
        match err {
            ValidationError::InvalidTaskUri {
                reason,
                ..
            } => {
                assert_eq!(reason, TaskUriError::EmptyIdentity);
            },
            other => panic!("expected invalid task URI, got {other:?}"),
        }
    }

    #[test]
    fn deprecated_accounting_covered_via_change_block() {
        let manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        // The deprecated predecessor named the same file the current
        // manifest's first [[change]] is updating; that counts as covered.
        let deprecated = vec![PathBuf::from("crates/arvo-graph/DESIGN.md")];
        validate_deprecated_accounting(&manifest, &deprecated).expect("covered");
    }

    #[test]
    fn deprecated_accounting_covered_via_accounting_block() {
        let manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        let deprecated = vec![PathBuf::from("crates/arvo-graph/src/old_helper.rs")];
        validate_deprecated_accounting(&manifest, &deprecated).expect("covered");
    }

    #[test]
    fn deprecated_accounting_rejects_missing_files() {
        let manifest = Manifest::from_toml(SPEC_EXAMPLE).unwrap();
        let deprecated = vec![
            PathBuf::from("crates/arvo-graph/DESIGN.md"), // covered via change
            PathBuf::from("crates/arvo-graph/src/forgotten.rs"), // NOT covered
        ];
        let err = validate_deprecated_accounting(&manifest, &deprecated).unwrap_err();
        match err {
            ValidationError::DeprecatedAccountingIncomplete {
                missing_files,
            } => {
                assert_eq!(missing_files, vec![PathBuf::from(
                    "crates/arvo-graph/src/forgotten.rs"
                )]);
            },
            other => panic!("expected incomplete, got {other:?}"),
        }
    }

    #[test]
    fn build_manifest_programmatically() {
        let manifest = Manifest {
            mockspace_version:     "1.0".to_owned(),
            round_slug:            "abc".to_owned(),
            phase:                 ManifestSide::Doc,
            scope:                 ScopeBlock {
                description:    "test".to_owned(),
                in_scope_tasks: vec!["mock://task/foo".to_owned()],
                out_of_scope:   vec![],
            },
            acceptance:            AcceptanceBlock {
                criteria: "ship it".to_owned(),
            },
            changes:               vec![ChangeBlock {
                task:        Some("mock://task/foo".to_owned()),
                file:        PathBuf::from("DESIGN.md"),
                description: "doc edit".to_owned(),
                verify:      VerifierCheck::AllOf(VerifierAllOf {
                    all_of: vec![VerifierCheck::Kind(VerifierKind::PathExists {
                        file: PathBuf::from("DESIGN.md"),
                    })],
                }),
            }],
            deprecated_accounting: vec![],
        };
        let s = manifest.to_toml().unwrap();
        let r = Manifest::from_toml(&s).unwrap();
        assert_eq!(r, manifest);
    }
}
