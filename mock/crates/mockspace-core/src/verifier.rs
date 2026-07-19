//! Manifest verifier catalog (spec §54).
//!
//! Mockspace ships a strict, structurally-typed catalog of verifier check
//! kinds. A manifest's `[change.verify]` block carries a [`VerifierCheck`],
//! which is either a single [`VerifierKind`] or a composition (all_of /
//! any_of / not). No free shell execution; no escape hatch. New built-in
//! kinds land by extending [`VerifierKind`]; new language-extension kinds
//! land via lint-pack registration (Phase 2, deferred).
//!
//! Phase 1 ships types and TOML serde. Execution against a worktree lives
//! in Phase 5 (CLI / git ops).
//!
//! # TOML shape
//!
//! A leaf kind:
//!
//! ```toml
//! [change.verify]
//! kind = "grep_present"
//! pattern = "pub struct Baz"
//! file = "crates/ir/src/grammar.rs"
//! ```
//!
//! A composition:
//!
//! ```toml
//! [change.verify]
//! all_of = [
//!   { kind = "grep_present", pattern = "pub struct Baz", file = "crates/ir/src/grammar.rs" },
//!   { kind = "grep_absent", pattern = "pub struct Bar", file = "crates/ir/src/grammar.rs" },
//! ]
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A single check in a manifest's `[change.verify]` tree.
///
/// Either a leaf [`VerifierKind`] (one of the closed built-in catalog) or a
/// composition that combines other checks via all_of / any_of / not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum VerifierCheck {
    /// All sub-checks must pass.
    AllOf(VerifierAllOf),
    /// At least one sub-check must pass.
    AnyOf(VerifierAnyOf),
    /// The sub-check must fail.
    Not(VerifierNot),
    /// A single built-in verifier kind.
    Kind(VerifierKind),
}

/// `all_of` composition body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierAllOf {
    pub all_of: Vec<VerifierCheck>,
}

/// `any_of` composition body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierAnyOf {
    pub any_of: Vec<VerifierCheck>,
}

/// `not` composition body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifierNot {
    pub not: Box<VerifierCheck>,
}

/// A single built-in verifier kind.
///
/// The catalog is closed: every variant corresponds to a vetted built-in
/// check that runs against a temporary worktree at APPLY entry. Adding a
/// new built-in kind is an upstream change to this enum plus a schema
/// version bump.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VerifierKind {
    /// Regex match present in file.
    GrepPresent {
        pattern: String,
        file:    PathBuf,
    },
    /// Regex match absent from file.
    GrepAbsent {
        pattern: String,
        file:    PathBuf,
    },
    /// Path exists in working tree.
    PathExists {
        file: PathBuf,
    },
    /// Path does not exist in working tree.
    PathAbsent {
        file: PathBuf,
    },
    /// File byte count is strictly below threshold.
    FileSizeBelow {
        file:  PathBuf,
        bytes: u64,
    },
    /// File byte count is strictly above threshold.
    FileSizeAbove {
        file:  PathBuf,
        bytes: u64,
    },
    /// File line count is strictly below threshold.
    LineCountBelow {
        file:  PathBuf,
        lines: u64,
    },
    /// File line count is strictly above threshold.
    LineCountAbove {
        file:  PathBuf,
        lines: u64,
    },
    /// JSON field at the given dotted path equals the given value.
    JsonFieldEquals {
        file:  PathBuf,
        path:  String,
        value: toml::Value,
    },
    /// TOML field at the given dotted path equals the given value.
    TomlFieldEquals {
        file:  PathBuf,
        path:  String,
        value: toml::Value,
    },
    /// YAML field at the given dotted path equals the given value.
    /// Safe-load mode only; custom tags and external anchors are refused
    /// at execution time.
    YamlFieldEquals {
        file:  PathBuf,
        path:  String,
        value: toml::Value,
    },
}

impl VerifierKind {
    /// The `file` field this kind targets.
    ///
    /// Every built-in kind names exactly one file path. Used by the
    /// path-traversal defence at execution time (spec §54).
    pub fn file(&self) -> &PathBuf {
        match self {
            Self::GrepPresent {
                file,
                ..
            }
            | Self::GrepAbsent {
                file,
                ..
            }
            | Self::PathExists {
                file,
            }
            | Self::PathAbsent {
                file,
            }
            | Self::FileSizeBelow {
                file,
                ..
            }
            | Self::FileSizeAbove {
                file,
                ..
            }
            | Self::LineCountBelow {
                file,
                ..
            }
            | Self::LineCountAbove {
                file,
                ..
            }
            | Self::JsonFieldEquals {
                file,
                ..
            }
            | Self::TomlFieldEquals {
                file,
                ..
            }
            | Self::YamlFieldEquals {
                file,
                ..
            } => file,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> VerifierCheck {
        toml::from_str(text).expect("parse VerifierCheck")
    }

    #[test]
    fn parses_grep_present_leaf() {
        let toml = r#"
            kind = "grep_present"
            pattern = "pub struct Baz"
            file = "crates/ir/src/grammar.rs"
        "#;
        let check = parse(toml);
        match check {
            VerifierCheck::Kind(VerifierKind::GrepPresent {
                pattern,
                file,
            }) => {
                assert_eq!(pattern, "pub struct Baz");
                assert_eq!(file, PathBuf::from("crates/ir/src/grammar.rs"));
            },
            other => panic!("expected grep_present, got {other:?}"),
        }
    }

    #[test]
    fn parses_path_exists() {
        let toml = r#"
            kind = "path_exists"
            file = "crates/ir/src/grammar.rs"
        "#;
        let check = parse(toml);
        assert!(matches!(
            check,
            VerifierCheck::Kind(VerifierKind::PathExists { .. })
        ));
    }

    #[test]
    fn parses_file_size_threshold() {
        let toml = r#"
            kind = "file_size_below"
            file = "DESIGN.md"
            bytes = 8192
        "#;
        let check = parse(toml);
        match check {
            VerifierCheck::Kind(VerifierKind::FileSizeBelow {
                file,
                bytes,
            }) => {
                assert_eq!(file, PathBuf::from("DESIGN.md"));
                assert_eq!(bytes, 8192);
            },
            other => panic!("expected file_size_below, got {other:?}"),
        }
    }

    #[test]
    fn parses_toml_field_equals() {
        let toml = r#"
            kind = "toml_field_equals"
            file = "Cargo.toml"
            path = "package.version"
            value = "1.0.0"
        "#;
        let check = parse(toml);
        match check {
            VerifierCheck::Kind(VerifierKind::TomlFieldEquals {
                file,
                path,
                value,
            }) => {
                assert_eq!(file, PathBuf::from("Cargo.toml"));
                assert_eq!(path, "package.version");
                assert_eq!(value, toml::Value::String("1.0.0".to_owned()));
            },
            other => panic!("expected toml_field_equals, got {other:?}"),
        }
    }

    #[test]
    fn parses_all_of_composition() {
        let toml = r#"
            all_of = [
              { kind = "grep_present", pattern = "pub struct Baz", file = "crates/ir/src/grammar.rs" },
              { kind = "grep_absent", pattern = "pub struct Bar", file = "crates/ir/src/grammar.rs" },
            ]
        "#;
        let check = parse(toml);
        match check {
            VerifierCheck::AllOf(VerifierAllOf {
                all_of,
            }) => {
                assert_eq!(all_of.len(), 2);
            },
            other => panic!("expected all_of, got {other:?}"),
        }
    }

    #[test]
    fn parses_any_of_composition() {
        let toml = r#"
            any_of = [
              { kind = "path_exists", file = "DESIGN.md" },
              { kind = "path_exists", file = "DESIGN.md.tmpl" },
            ]
        "#;
        let check = parse(toml);
        assert!(matches!(check, VerifierCheck::AnyOf(_)));
    }

    #[test]
    fn parses_not_composition() {
        let toml = r#"
            not = { kind = "grep_present", pattern = "TODO", file = "src/lib.rs" }
        "#;
        let check = parse(toml);
        match check {
            VerifierCheck::Not(VerifierNot {
                not,
            }) => {
                assert!(matches!(
                    *not,
                    VerifierCheck::Kind(VerifierKind::GrepPresent { .. })
                ));
            },
            other => panic!("expected not, got {other:?}"),
        }
    }

    #[test]
    fn parses_nested_composition() {
        let toml = r###"
            all_of = [
              { kind = "path_exists", file = "DESIGN.md" },
              { not = { kind = "grep_present", pattern = "DEPRECATED", file = "DESIGN.md" } },
              { any_of = [
                  { kind = "grep_present", pattern = "## Section A", file = "DESIGN.md" },
                  { kind = "grep_present", pattern = "## Section B", file = "DESIGN.md" },
              ] },
            ]
        "###;
        let check = parse(toml);
        match check {
            VerifierCheck::AllOf(VerifierAllOf {
                all_of,
            }) => {
                assert_eq!(all_of.len(), 3);
                assert!(matches!(all_of[0], VerifierCheck::Kind(_)));
                assert!(matches!(all_of[1], VerifierCheck::Not(_)));
                assert!(matches!(all_of[2], VerifierCheck::AnyOf(_)));
            },
            other => panic!("expected all_of, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_kind() {
        let toml = r#"
            kind = "frobnicate"
            file = "x"
        "#;
        let err = toml::from_str::<VerifierCheck>(toml).unwrap_err();
        assert!(err.to_string().contains("frobnicate") || err.to_string().contains("variant"));
    }

    #[test]
    fn round_trip_preserves_shape() {
        let original = VerifierCheck::AllOf(VerifierAllOf {
            all_of: vec![
                VerifierCheck::Kind(VerifierKind::GrepPresent {
                    pattern: "pub struct Csr".to_owned(),
                    file:    PathBuf::from("crates/arvo-graph/src/csr.rs"),
                }),
                VerifierCheck::Not(VerifierNot {
                    not: Box::new(VerifierCheck::Kind(VerifierKind::PathExists {
                        file: PathBuf::from("crates/arvo-graph/src/legacy.rs"),
                    })),
                }),
            ],
        });
        let serialised = toml::to_string(&original).unwrap();
        let reparsed: VerifierCheck = toml::from_str(&serialised).unwrap();
        assert_eq!(original, reparsed);
    }

    #[test]
    fn kind_file_accessor() {
        let k = VerifierKind::LineCountAbove {
            file:  PathBuf::from("DESIGN.md"),
            lines: 50,
        };
        assert_eq!(k.file(), &PathBuf::from("DESIGN.md"));
    }
}
