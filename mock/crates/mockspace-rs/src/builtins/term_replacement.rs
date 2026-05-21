//! TermReplacementTable primitive.
//!
//! Per schema design memo §4.6. A word-boundary-aware lookup table from
//! "dead term" to "canonical replacement". Drives the workspace's
//! `vocabulary-discipline` lint (substrate → foundations, HList → cons-list,
//! entity → record, etc.). Each match produces a Finding plus a
//! `Suggestion` carrying the canonical form via a `Fix::Replace` recipe.
//!
//! Implemented as a self-contained scanner rather than as TokenScan-with-
//! replacements because the matching surface is asymmetric: TokenScan
//! treats `tokens` as a homogeneous list; TermReplacementTable's
//! semantic unit is a (term, replacement) pair, and the per-match
//! message names the replacement directly.

use std::borrow::Cow;
use std::collections::HashMap;

use mockspace_core::lint::{Finding, Fix, GateSeverity, LintContext, Span, Suggestion};
use serde::Deserialize;

use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::strip::StripOpts;

pub const KIND: &str = "term-replacement-table";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TermReplacementTableConfig {
    /// Map of dead term to canonical replacement.
    ///
    /// Parsed as a TOML inline table or a `[lints.X.config.replacements]`
    /// sub-table. Stored as `HashMap` for O(1) lookup during scan; order
    /// of emission is per-occurrence-in-source.
    pub replacements: HashMap<String, String>,

    #[serde(default = "default_true")]
    pub word_boundary: bool,

    #[serde(default = "default_true")]
    pub strip_strings: bool,

    #[serde(default)]
    pub strip_comments: bool,

    #[serde(default)]
    pub strip_doc_comments: bool,
}

fn default_true() -> bool {
    true
}

pub struct TermReplacementTableLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: TermReplacementTableConfig,
}

impl TermReplacementTableLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: TermReplacementTableConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for TermReplacementTableLint {
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
        if self.config.replacements.is_empty() {
            return Ok(());
        }
        let opts = StripOpts {
            strings: self.config.strip_strings,
            comments: self.config.strip_comments,
            doc_comments: self.config.strip_doc_comments,
            code_fences: false,
        };
        let view = doc.source_stripped(opts);
        let active = ctx.active_severity();
        let lint_name = self.name;
        for (term, replacement) in &self.config.replacements {
            scan_term(
                view.as_ref(),
                term,
                replacement,
                self.config.word_boundary,
                |line, column, length, _byte_offset_in_view| {
                    sink.emit(Finding {
                        lint_name: Cow::Borrowed(lint_name),
                        rule_id: None,
                        plugin_id: None,
                        severity: active,
                        impact: None,
                        category: None,
                        message: Cow::Owned(format!(
                            "dead term `{term}`; use `{replacement}` instead"
                        )),
                        span: Span::single_line(doc.path(), line, column, length as u32),
                        hint: None,
                        help: None,
                        // Description only; no Fix recipe. The byte offset
                        // we have here (`_byte_offset_in_view`) is into the
                        // source-stripped view, not the original document
                        // bytes that Fix::Replace expects. Translating
                        // stripped offsets back requires a position map the
                        // stripper does not currently maintain. Emit advice
                        // until that translation is wired (or until
                        // term_replacement scans the unstripped source).
                        suggestion: Some(Suggestion {
                            description: Cow::Owned(format!(
                                "replace `{term}` with `{replacement}`"
                            )),
                            fix: None,
                        }),
                        related_spans: Vec::new(),
                        metadata: None,
                    });
                },
            );
        }
        Ok(())
    }
}

fn scan_term<F: FnMut(u32, u32, usize, usize)>(
    view: &str,
    term: &str,
    _replacement: &str,
    word_boundary: bool,
    mut on_match: F,
) {
    if term.is_empty() {
        return;
    }
    let bytes = view.as_bytes();
    let term_bytes = term.as_bytes();
    let mut line: u32 = 1;
    let mut line_start: usize = 0;
    let mut i = 0;
    while i + term_bytes.len() <= bytes.len() {
        if bytes[i] == b'\n' {
            line += 1;
            line_start = i + 1;
            i += 1;
            continue;
        }
        if &bytes[i..i + term_bytes.len()] == term_bytes {
            let first_is_word = is_word_byte(term_bytes[0]);
            let last_is_word = is_word_byte(term_bytes[term_bytes.len() - 1]);
            let lhs_ok = !word_boundary || !first_is_word || i == 0 || !is_word_byte(bytes[i - 1]);
            let after = i + term_bytes.len();
            let rhs_ok = !word_boundary
                || !last_is_word
                || after >= bytes.len()
                || !is_word_byte(bytes[after]);
            if lhs_ok && rhs_ok {
                let col = (i - line_start) as u32 + 1;
                on_match(line, col, term_bytes.len(), i);
                i = after;
                continue;
            }
        }
        i += 1;
    }
}

fn is_word_byte(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_')
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: TermReplacementTableConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("term-replacement-table config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(TermReplacementTableLint::new(
        name,
        description,
        default_severity,
        parsed,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_types::Language;
    use crate::finding_sink::VecFindingSink;
    use mockspace_core::lint::{Gate, Severity};
    use std::path::PathBuf;

    struct EmptyCfg;
    impl mockspace_core::lint::LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    fn make_ctx<'a>(root: &'a PathBuf, sev: GateSeverity, cfg: &'a EmptyCfg) -> LintContext<'a> {
        LintContext {
            gate: Gate::Commit,
            severities: sev,
            surface: mockspace_core::lint::RunSurface::Local,
            project_root: root,
            config: cfg,
        }
    }

    #[test]
    fn fires_with_replacement_message_and_fix() {
        let mut replacements = HashMap::new();
        replacements.insert("substrate".to_string(), "foundations".to_string());
        let lint = TermReplacementTableLint::new(
            "vocab",
            "",
            GateSeverity::uniform(Severity::Warn),
            TermReplacementTableConfig {
                replacements,
                word_boundary: true,
                strip_strings: false,
                strip_comments: false,
                strip_doc_comments: false,
            },
        );
        let doc = MockspaceDocument::new(
            "DESIGN.md",
            "t",
            Language::Markdown,
            "The substrate is robust.",
        );
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("foundations"));
        let suggestion = findings[0].suggestion.as_ref().expect("suggestion");
        assert!(
            suggestion.description.contains("foundations"),
            "description should name the replacement: {}",
            suggestion.description,
        );
        // Fix is None today; byte offsets in the scanner are into the
        // stripped view, not the original source. See the comment in
        // check_document above.
        assert!(suggestion.fix.is_none());
    }

    #[test]
    fn word_boundary_blocks_substring() {
        let mut replacements = HashMap::new();
        replacements.insert("entry".to_string(), "record".to_string());
        let lint = TermReplacementTableLint::new(
            "vocab",
            "",
            GateSeverity::uniform(Severity::Warn),
            TermReplacementTableConfig {
                replacements,
                word_boundary: true,
                strip_strings: false,
                strip_comments: false,
                strip_doc_comments: false,
            },
        );
        let doc = MockspaceDocument::new(
            "a.md",
            "t",
            Language::Markdown,
            "subentry has entry-as-substring",
        );
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        let findings = sink.into_findings();
        // Two matches: standalone "entry" in "entry-as-substring" (word boundary on `-`).
        // Not: "subentry" (no left boundary).
        assert_eq!(findings.len(), 1);
    }
}
