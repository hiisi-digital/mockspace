//! ContentRegex primitive.
//!
//! Regex-driven scanning over (optionally stripped) source. Per schema
//! design memo §4.5. The shape `TokenScan` can't cover: case-insensitive
//! matches, anchored matches, alternations, character classes,
//! word-frequency-tag windowed thresholds. Examples: `writing-style`
//! (em-dash regex + marketing-word alternation), `no-todo-fixme`
//! (`\b(TODO|FIXME|XXX|HACK)\b`).

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use regex::Regex;
use serde::Deserialize;

use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::strip::StripOpts;

pub const KIND: &str = "content-regex";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ContentRegexConfig {
    pub patterns: Vec<ContentPattern>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ContentPattern {
    pub regex: String,
    pub message: String,
    /// Optional finding kind. When set, must appear in the lint's
    /// `CatalogEntry::finding_kinds`.
    #[serde(default)]
    pub finding_kind: Option<String>,
    /// Optional ratio-threshold gating (max matches per window before fires).
    #[serde(default)]
    pub ratio: Option<RatioThreshold>,
    /// Strip markdown fenced code blocks before matching.
    #[serde(default)]
    pub strip_code_fences: bool,
    /// Strip string literals before matching.
    #[serde(default)]
    pub strip_strings: bool,
    /// Strip comments before matching.
    #[serde(default)]
    pub strip_comments: bool,
    /// Strip doc comments before matching.
    #[serde(default)]
    pub strip_doc_comments: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RatioThreshold {
    pub max_matches: u32,
    pub lines_window: u32,
}

struct CompiledPattern {
    pattern: ContentPattern,
    regex: Regex,
}

pub struct ContentRegexLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    patterns: Vec<CompiledPattern>,
}

impl ContentRegexLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: ContentRegexConfig,
    ) -> Result<Self, ConfigError> {
        let mut patterns = Vec::with_capacity(config.patterns.len());
        for (i, p) in config.patterns.into_iter().enumerate() {
            let regex = Regex::new(&p.regex).map_err(|e| ConfigError {
                lint_name: name.to_string(),
                field_path: format!("patterns[{i}].regex"),
                kind: ConfigErrorKind::UnparseableRegex {
                    error: e.to_string(),
                },
                message: format!("regex `{}` did not compile", p.regex),
                source_location: None,
            })?;
            patterns.push(CompiledPattern { pattern: p, regex });
        }
        Ok(Self {
            name,
            description,
            default_severity,
            patterns,
        })
    }
}

impl Lint for ContentRegexLint {
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
        let active = ctx.active_severity();
        for compiled in &self.patterns {
            let opts = StripOpts {
                strings: compiled.pattern.strip_strings,
                comments: compiled.pattern.strip_comments,
                doc_comments: compiled.pattern.strip_doc_comments,
                code_fences: compiled.pattern.strip_code_fences,
            };
            let view = if opts.strings || opts.comments || opts.doc_comments || opts.code_fences {
                doc.source_stripped(opts)
            } else {
                std::sync::Arc::from(doc.source())
            };

            emit_regex_matches(
                &compiled.regex,
                view.as_ref(),
                &compiled.pattern,
                self.name,
                doc.path(),
                active,
                sink,
            );
        }
        Ok(())
    }
}

fn emit_regex_matches(
    regex: &Regex,
    view: &str,
    pattern: &ContentPattern,
    lint_name: &'static str,
    path: &std::path::Path,
    severity: Severity,
    sink: &dyn FindingSink,
) {
    if let Some(ratio) = &pattern.ratio {
        emit_ratio_gated(regex, view, pattern, ratio, lint_name, path, severity, sink);
        return;
    }
    for m in regex.find_iter(view) {
        let (line, column) = byte_offset_to_line_col(view, m.start());
        let length = (m.end() - m.start()) as u32;
        sink.emit(make_finding(
            lint_name, pattern, path, line, column, length, severity,
        ));
    }
}

fn emit_ratio_gated(
    regex: &Regex,
    view: &str,
    pattern: &ContentPattern,
    ratio: &RatioThreshold,
    lint_name: &'static str,
    path: &std::path::Path,
    severity: Severity,
    sink: &dyn FindingSink,
) {
    // Collect (line, column, length) per match, then walk through windows of
    // size `lines_window` and emit one finding per window that exceeds
    // `max_matches`.
    let mut matches: Vec<(u32, u32, u32)> = regex
        .find_iter(view)
        .map(|m| {
            let (l, c) = byte_offset_to_line_col(view, m.start());
            (l, c, (m.end() - m.start()) as u32)
        })
        .collect();
    matches.sort_by_key(|t| t.0);

    let window = ratio.lines_window.max(1);
    let threshold = ratio.max_matches.max(1);
    let mut i = 0;
    while i < matches.len() {
        let win_start = matches[i].0;
        let win_end = win_start.saturating_add(window);
        let mut count = 0u32;
        let mut j = i;
        while j < matches.len() && matches[j].0 < win_end {
            count = count.saturating_add(1);
            j += 1;
        }
        if count >= threshold {
            // Emit one finding pointing at the first match in the window.
            let (line, column, length) = matches[i];
            sink.emit(make_finding(
                lint_name, pattern, path, line, column, length, severity,
            ));
            i = j;
        } else {
            i += 1;
        }
    }
}

fn make_finding(
    lint_name: &'static str,
    pattern: &ContentPattern,
    path: &std::path::Path,
    line: u32,
    column: u32,
    length: u32,
    severity: Severity,
) -> Finding {
    Finding {
        lint_name: Cow::Borrowed(lint_name),
        rule_id: pattern.finding_kind.clone().map(Cow::Owned),
        plugin_id: None,
        severity,
        impact: None,
        category: None,
        message: Cow::Owned(pattern.message.clone()),
        span: Span::single_line(path, line, column, length),
        hint: None,
        help: None,
        suggestion: None,
        related_spans: Vec::new(),
        metadata: None,
    }
}

fn byte_offset_to_line_col(view: &str, offset: usize) -> (u32, u32) {
    let bytes = view.as_bytes();
    let mut line = 1u32;
    let mut last_newline: usize = 0;
    let mut have_seen_newline = false;
    for (i, &b) in bytes.iter().enumerate().take(offset) {
        if b == b'\n' {
            line += 1;
            last_newline = i;
            have_seen_newline = true;
        }
    }
    let column = if have_seen_newline {
        (offset - last_newline) as u32
    } else {
        offset as u32 + 1
    };
    (line, column)
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: ContentRegexConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("content-regex config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(ContentRegexLint::new(
        name,
        description,
        default_severity,
        parsed,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_types::Language;
    use crate::finding_sink::VecFindingSink;
    use mockspace_core::lint::Gate;
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
    fn fires_on_simple_regex() {
        let config = ContentRegexConfig {
            patterns: vec![ContentPattern {
                regex: r"\bTODO\b".to_string(),
                message: "todo found".to_string(),
                finding_kind: None,
                ratio: None,
                strip_code_fences: false,
                strip_strings: false,
                strip_comments: false,
                strip_doc_comments: false,
            }],
        };
        let lint =
            ContentRegexLint::new("no-todo", "", GateSeverity::uniform(Severity::Warn), config)
                .unwrap();
        let doc = MockspaceDocument::new("a.rs", "t", Language::Rust, "// TODO: fix\nfn x() {}");
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].span.start_line, 1);
        assert!(findings[0].message.contains("todo"));
    }

    #[test]
    fn ratio_threshold_collapses_burst() {
        let config = ContentRegexConfig {
            patterns: vec![ContentPattern {
                regex: r"—".to_string(), // em-dash
                message: "em-dash found".to_string(),
                finding_kind: None,
                ratio: Some(RatioThreshold {
                    max_matches: 3,
                    lines_window: 5,
                }),
                strip_code_fences: false,
                strip_strings: false,
                strip_comments: false,
                strip_doc_comments: false,
            }],
        };
        let lint = ContentRegexLint::new(
            "em-dash-bursts",
            "",
            GateSeverity::uniform(Severity::Warn),
            config,
        )
        .unwrap();
        // 4 em-dashes within 5 lines → fires once at the first.
        // 2 em-dashes within 5 lines → does not fire.
        let source = "a — b\nc — d\ne — f\ng — h\ni\nj\nk — l\nm — n";
        let doc = MockspaceDocument::new("a.md", "t", Language::Markdown, source);
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn bad_regex_surfaces_as_config_error() {
        let config = ContentRegexConfig {
            patterns: vec![ContentPattern {
                regex: r"(unclosed".to_string(),
                message: "x".to_string(),
                finding_kind: None,
                ratio: None,
                strip_code_fences: false,
                strip_strings: false,
                strip_comments: false,
                strip_doc_comments: false,
            }],
        };
        let result =
            ContentRegexLint::new("broken", "", GateSeverity::uniform(Severity::Warn), config);
        match result {
            Ok(_) => panic!("expected ConfigError, got Ok"),
            Err(e) => match e.kind {
                ConfigErrorKind::UnparseableRegex { .. } => {}
                other => panic!("unexpected: {other:?}"),
            },
        }
    }
}
