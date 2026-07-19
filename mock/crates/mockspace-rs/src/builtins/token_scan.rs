//! TokenScan primitive.
//!
//! Scans the (optionally stripped) source for plain-literal tokens. Per
//! schema design memo §4.1. The most common shape: any `no-X` lint whose
//! definition is "fire when this exact substring appears outside comments
//! and strings". Examples: `no-alloc` (scan for `Vec<`, `String`, `Box<`),
//! `no-std` (scan for `std::`), `no-todo` (when not used through
//! AstNodePositionMatch), `forbidden_imports`.
//!
//! For regex semantics, use [`crate::builtins::content_regex::ContentRegex`].
//! For AST-based shape matching, use AstNodePositionMatch / AstTypePosition.

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use serde::Deserialize;

use crate::config_types::EscalationRule;
use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::strip::StripOpts;

pub const KIND: &str = "token-scan";

/// Per-lint TOML configuration for `TokenScan`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TokenScanConfig {
    /// Plain literal substrings to scan for.
    pub tokens: Vec<String>,

    /// Require ASCII-word-boundary characters on both sides of each match.
    /// Default `true`. Set `false` for substring-anywhere matching.
    #[serde(default = "default_true")]
    pub word_boundary: bool,

    /// Strip string literal bodies before scanning. Default `true`.
    #[serde(default = "default_true")]
    pub strip_strings: bool,

    /// Strip `//` and `/* */` comments before scanning. Default `true`.
    #[serde(default = "default_true")]
    pub strip_comments: bool,

    /// Strip `///`, `//!`, `/** */`, `/*! */` doc comments before scanning.
    /// Default `true`.
    #[serde(default = "default_true")]
    pub strip_doc_comments: bool,

    /// Optional severity escalation when match count within one document
    /// exceeds a threshold.
    #[serde(default)]
    pub severity_escalation: Option<EscalationRule>,
}

fn default_true() -> bool {
    true
}

impl Default for TokenScanConfig {
    fn default() -> Self {
        Self {
            tokens:              Vec::new(),
            word_boundary:       true,
            strip_strings:       true,
            strip_comments:      true,
            strip_doc_comments:  true,
            severity_escalation: None,
        }
    }
}

/// Concrete `TokenScan` lint, parameterised on its catalog name + config.
pub struct TokenScanLint {
    name:             &'static str,
    description:      &'static str,
    config:           TokenScanConfig,
    default_severity: GateSeverity,
    message_template: String,
}

impl TokenScanLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        config: TokenScanConfig,
        default_severity: GateSeverity,
    ) -> Self {
        Self {
            name,
            description,
            config,
            default_severity,
            message_template: format!("forbidden token `{{}}` matched by `{name}`"),
        }
    }
}

impl Lint for TokenScanLint {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn default_severity(&self) -> GateSeverity {
        self.default_severity
    }

    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        if self.config.tokens.is_empty() {
            return Ok(());
        }
        let strip_opts = StripOpts {
            strings:      self.config.strip_strings,
            comments:     self.config.strip_comments,
            doc_comments: self.config.strip_doc_comments,
            code_fences:  false,
        };
        let view = doc.source_stripped(strip_opts);
        let view_str: &str = view.as_ref();

        let base_severity = ctx.active_severity();
        let mut match_count: u32 = 0;
        for token in &self.config.tokens {
            for (line, column, length) in scan_token(view_str, token, self.config.word_boundary) {
                match_count = match_count.saturating_add(1);
                let severity = self.effective_severity(base_severity, match_count);
                if severity.silent() {
                    continue;
                }
                sink.emit(Finding {
                    lint_name: Cow::Borrowed(self.name),
                    rule_id: None,
                    plugin_id: None,
                    severity,
                    impact: None,
                    category: None,
                    message: Cow::Owned(self.message_template.replace("{}", token)),
                    span: Span::single_line(doc.path(), line, column, length as u32),
                    hint: None,
                    help: None,
                    suggestion: None,
                    related_spans: Vec::new(),
                    metadata: None,
                });
            }
        }
        Ok(())
    }
}

impl TokenScanLint {
    fn effective_severity(&self, base: Severity, count_so_far: u32) -> Severity {
        if let Some(esc) = &self.config.severity_escalation {
            if count_so_far >= esc.threshold {
                return esc.escalated_severity;
            }
        }
        base
    }
}

/// Catalog `instantiate` adapter.
///
/// Parses the merged TOML config into [`TokenScanConfig`]. Catalog entries
/// for individual `TokenScan` lints (e.g. `no-alloc`) call this from their
/// `instantiate: fn` slot, threading the entry's static `name` /
/// `description` / `default_severity` through.
pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: TokenScanConfig = config.clone().try_into().map_err(|e: toml::de::Error| {
        ConfigError {
            lint_name:       name.to_string(),
            field_path:      String::new(),
            kind:            ConfigErrorKind::InvalidValue,
            message:         format!("token-scan config: {e}"),
            source_location: None,
        }
    })?;
    Ok(Box::new(TokenScanLint::new(
        name,
        description,
        parsed,
        default_severity,
    )))
}

// =========================================================================
// Token-scanning helper.
// =========================================================================

/// Find each occurrence of `token` in `view`. Returns `(line, column,
/// length)` triples with 1-indexed line and column.
///
/// `word_boundary = true` requires ASCII-word-boundary characters on
/// both sides, where the "word" character class is `[A-Za-z0-9_]` plus
/// non-ASCII bytes. A boundary is the transition from word to non-word
/// or end-of-string.
fn scan_token(view: &str, token: &str, word_boundary: bool) -> Vec<(u32, u32, usize)> {
    if token.is_empty() {
        return Vec::new();
    }
    let bytes = view.as_bytes();
    let token_bytes = token.as_bytes();
    let mut matches = Vec::new();
    let mut line_starts: Vec<usize> = vec![0];
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let mut i = 0;
    while i + token_bytes.len() <= bytes.len() {
        if &bytes[i .. i + token_bytes.len()] == token_bytes {
            // Left boundary: the token's preceding byte must be non-word
            // (only when the token's first byte is itself a word char;
            // tokens starting with punctuation never need a left boundary).
            let first_is_word = is_word_byte(token_bytes[0]);
            let lhs_ok = if word_boundary && first_is_word {
                i == 0 || !is_word_byte(bytes[i - 1])
            } else {
                true
            };
            // Right boundary: the token's following byte must be non-word
            // (only when the token's last byte is itself a word char).
            let last_is_word = is_word_byte(token_bytes[token_bytes.len() - 1]);
            let after = i + token_bytes.len();
            let rhs_ok = if word_boundary && last_is_word {
                after >= bytes.len() || !is_word_byte(bytes[after])
            } else {
                true
            };
            if lhs_ok && rhs_ok {
                let line_idx = match line_starts.binary_search(&i) {
                    Ok(idx) => idx,
                    Err(idx) => idx.saturating_sub(1),
                };
                let line = (line_idx as u32) + 1;
                let column = ((i - line_starts[line_idx]) as u32) + 1;
                matches.push((line, column, token_bytes.len()));
            }
            i += token_bytes.len();
        } else {
            i += 1;
        }
    }
    matches
}

fn is_word_byte(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mockspace_core::lint::Gate;
    use toml::Table;

    use super::*;
    use crate::config_types::Language;
    use crate::finding_sink::VecFindingSink;

    fn test_ctx() -> (PathBuf, GateSeverity) {
        (PathBuf::from("/tmp"), GateSeverity::uniform(Severity::Warn))
    }

    struct EmptyCfg;
    impl mockspace_core::lint::LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    fn make_ctx<'a>(
        root: &'a PathBuf,
        severities: GateSeverity,
        cfg: &'a EmptyCfg,
    ) -> LintContext<'a> {
        LintContext {
            gate: Gate::Commit,
            severities,
            surface: mockspace_core::lint::RunSurface::Local,
            project_root: root,
            config: cfg,
        }
    }

    #[test]
    fn fires_on_plain_token() {
        let config = TokenScanConfig {
            tokens:              vec!["Vec<".to_string()],
            word_boundary:       false,
            strip_strings:       true,
            strip_comments:      true,
            strip_doc_comments:  true,
            severity_escalation: None,
        };
        let lint = TokenScanLint::new(
            "no-alloc",
            "no allocation",
            config,
            GateSeverity::uniform(Severity::Warn),
        );
        let doc = MockspaceDocument::new(
            "a.rs",
            "test",
            Language::Rust,
            "fn x() -> Vec<u8> { Vec::new() }",
        );
        let sink = VecFindingSink::new();
        let (root, sev) = test_ctx();
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].span.start_line, 1);
        assert!(findings[0].message.contains("Vec<"));
    }

    #[test]
    fn does_not_fire_inside_strings_when_stripping() {
        let config = TokenScanConfig {
            tokens: vec!["Vec<".to_string()],
            word_boundary: false,
            ..Default::default()
        };
        let lint = TokenScanLint::new(
            "no-alloc",
            "",
            config,
            GateSeverity::uniform(Severity::Warn),
        );
        let doc = MockspaceDocument::new("a.rs", "t", Language::Rust, "let s = \"Vec<u8>\";");
        let sink = VecFindingSink::new();
        let (root, sev) = test_ctx();
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 0);
    }

    #[test]
    fn does_not_fire_inside_line_comment_when_stripping() {
        let config = TokenScanConfig {
            tokens: vec!["Vec<".to_string()],
            word_boundary: false,
            ..Default::default()
        };
        let lint = TokenScanLint::new(
            "no-alloc",
            "",
            config,
            GateSeverity::uniform(Severity::Warn),
        );
        let doc = MockspaceDocument::new("a.rs", "t", Language::Rust, "// Vec<u8>\nfn x() {}");
        let sink = VecFindingSink::new();
        let (root, sev) = test_ctx();
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 0);
    }

    #[test]
    fn word_boundary_blocks_substring_of_identifier() {
        let config = TokenScanConfig {
            tokens: vec!["foo".to_string()],
            word_boundary: true,
            ..Default::default()
        };
        let lint = TokenScanLint::new("no-foo", "", config, GateSeverity::uniform(Severity::Warn));
        let doc = MockspaceDocument::new("a.rs", "t", Language::Rust, "let foobar = 1;");
        let sink = VecFindingSink::new();
        let (root, sev) = test_ctx();
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 0);
    }

    #[test]
    fn instantiate_parses_toml() {
        let toml_in: Table = r#"
            tokens = ["alloc::"]
            word_boundary = false
        "#
        .parse()
        .unwrap();
        let scope: Table = Table::new();
        let lint = instantiate_with(
            "no-alloc",
            "no allocation",
            GateSeverity::uniform(Severity::Warn),
            &toml_in,
            &scope,
        )
        .unwrap();
        assert_eq!(lint.name(), "no-alloc");
        // The default for `word_boundary` is `true`; the fixture
        // passes `false`. Verify the non-default field actually
        // round-trips through TOML by running the configured lint
        // against a substring: with `word_boundary = false` the
        // lint fires inside an identifier; with the default it
        // would not. The previous assertion only checked `name()`,
        // which would pass even if `word_boundary` silently dropped
        // to its default.
        let doc = MockspaceDocument::new(
            "x.rs",
            "my-crate",
            Language::Rust,
            "fn x() { let prealloc::y = 1; }\n",
        );
        let sink = VecFindingSink::new();
        let (root, sev) = test_ctx();
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        assert!(
            !sink.into_findings().is_empty(),
            "with word_boundary=false the lint should match the substring",
        );
    }
}
