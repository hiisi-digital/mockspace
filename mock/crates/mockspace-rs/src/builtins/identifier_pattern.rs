//! IdentifierPattern primitive.
//!
//! Per schema design memo §4.4. Walks the syn AST for items of selected
//! kinds and matches their names against forbidden prefixes, suffixes,
//! and regexes. Examples: `no-leaking-typestate-suffix` (forbid `Builder`,
//! `Inner` in pub item names), naming-convention lints, deprecated-prefix
//! detection.

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use regex::Regex;
use serde::Deserialize;

use crate::config_types::{ItemKind, Visibility};
use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;

pub const KIND: &str = "identifier-pattern";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct IdentifierPatternConfig {
    pub item_kinds: Vec<ItemKind>,

    #[serde(default)]
    pub forbidden_prefixes: Vec<String>,

    #[serde(default)]
    pub forbidden_suffixes: Vec<String>,

    /// Forbidden regex patterns. Pre-compiled at instantiate.
    #[serde(default)]
    pub forbidden_regexes: Vec<String>,

    #[serde(default)]
    pub visibility: Visibility,
}

pub struct IdentifierPatternLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: IdentifierPatternConfig,
    compiled_regexes: Vec<Regex>,
}

impl IdentifierPatternLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: IdentifierPatternConfig,
    ) -> Result<Self, ConfigError> {
        let mut compiled_regexes = Vec::with_capacity(config.forbidden_regexes.len());
        for (i, pattern) in config.forbidden_regexes.iter().enumerate() {
            let regex = Regex::new(pattern).map_err(|e| ConfigError {
                lint_name: name.to_string(),
                field_path: format!("forbidden_regexes[{i}]"),
                kind: ConfigErrorKind::UnparseableRegex {
                    error: e.to_string(),
                },
                message: format!("regex `{pattern}` did not compile"),
                source_location: None,
            })?;
            compiled_regexes.push(regex);
        }
        Ok(Self {
            name,
            description,
            default_severity,
            config,
            compiled_regexes,
        })
    }

    fn matches(&self, ident: &str) -> Option<String> {
        for prefix in &self.config.forbidden_prefixes {
            if ident.starts_with(prefix) {
                return Some(format!("identifier `{ident}` has forbidden prefix `{prefix}`"));
            }
        }
        for suffix in &self.config.forbidden_suffixes {
            if ident.ends_with(suffix) {
                return Some(format!("identifier `{ident}` has forbidden suffix `{suffix}`"));
            }
        }
        for (i, regex) in self.compiled_regexes.iter().enumerate() {
            if regex.is_match(ident) {
                let pattern = &self.config.forbidden_regexes[i];
                return Some(format!(
                    "identifier `{ident}` matches forbidden pattern `{pattern}`"
                ));
            }
        }
        None
    }
}

impl Lint for IdentifierPatternLint {
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn default_severity(&self) -> GateSeverity {
        self.default_severity
    }
    fn needs_syn_ast(&self) -> bool {
        true
    }

    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        let Some(file) = doc.ast() else {
            return Ok(());
        };
        let path = doc.path();
        let active = ctx.active_severity();
        for item in &file.items {
            if let Some((ident, kind, is_pub)) = identify_item(item) {
                if !kind_matches(&self.config.item_kinds, kind) {
                    continue;
                }
                if self.config.visibility == Visibility::Public && !is_pub {
                    continue;
                }
                if let Some(message) = self.matches(&ident) {
                    emit_for_ident(self.name, &message, path, &ident, active, sink);
                }
            }
        }
        Ok(())
    }
}

fn identify_item(item: &syn::Item) -> Option<(String, ItemKind, bool)> {
    match item {
        syn::Item::Fn(it) => Some((
            it.sig.ident.to_string(),
            ItemKind::Fn,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Struct(it) => Some((
            it.ident.to_string(),
            ItemKind::Struct,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Enum(it) => Some((
            it.ident.to_string(),
            ItemKind::Enum,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Trait(it) => Some((
            it.ident.to_string(),
            ItemKind::Trait,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Type(it) => Some((
            it.ident.to_string(),
            ItemKind::TypeAlias,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Const(it) => Some((
            it.ident.to_string(),
            ItemKind::Const,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Static(it) => Some((
            it.ident.to_string(),
            ItemKind::Static,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Mod(it) => Some((
            it.ident.to_string(),
            ItemKind::Mod,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        _ => None,
    }
}

fn kind_matches(allowed: &[ItemKind], kind: ItemKind) -> bool {
    allowed.is_empty() || allowed.contains(&kind)
}

fn emit_for_ident(
    lint_name: &'static str,
    message: &str,
    path: &std::path::Path,
    ident: &str,
    severity: Severity,
    sink: &dyn FindingSink,
) {
    // Span pointing at line 1 column 1 with length = ident len. Source
    // positions for items require a per-item span walk; syn carries Span
    // info but resolving to (line, column) needs source map plumbing not
    // wired yet. Placeholder until the span resolver lands.
    sink.emit(Finding {
        lint_name: Cow::Borrowed(lint_name),
        rule_id: None,
        plugin_id: None,
        severity,
        impact: None,
        category: None,
        message: Cow::Owned(message.to_string()),
        span: Span::single_line(path, 1, 1, ident.len() as u32),
        hint: None,
        help: None,
        suggestion: None,
        related_spans: Vec::new(),
        metadata: None,
    });
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: IdentifierPatternConfig = config.clone().try_into().map_err(
        |e: toml::de::Error| ConfigError {
            lint_name: name.to_string(),
            field_path: String::new(),
            kind: ConfigErrorKind::InvalidValue,
            message: format!("identifier-pattern config: {e}"),
            source_location: None,
        },
    )?;
    Ok(Box::new(IdentifierPatternLint::new(
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
    use mockspace_core::lint::{Gate, Severity};
    use std::path::PathBuf;

    struct EmptyCfg;
    impl mockspace_core::lint::LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    fn make_ctx<'a>(
        root: &'a PathBuf,
        sev: GateSeverity,
        cfg: &'a EmptyCfg,
    ) -> LintContext<'a> {
        LintContext {
            gate: Gate::Commit,
            severities: sev,
            surface: mockspace_core::lint::RunSurface::Local,
            project_root: root,
            config: cfg,
        }
    }

    #[test]
    fn fires_on_forbidden_suffix() {
        let lint = IdentifierPatternLint::new(
            "no-builder-suffix",
            "",
            GateSeverity::uniform(Severity::Warn),
            IdentifierPatternConfig {
                item_kinds: vec![ItemKind::Struct],
                forbidden_prefixes: Vec::new(),
                forbidden_suffixes: vec!["Builder".to_string()],
                forbidden_regexes: Vec::new(),
                visibility: Visibility::Public,
            },
        )
        .unwrap();
        let doc = MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "pub struct FooBuilder; struct Bar; fn FooBuilder() {}",
        );
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        let findings = sink.into_findings();
        // Only the pub struct counts; the fn and the non-pub struct are filtered.
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("FooBuilder"));
    }

    #[test]
    fn regex_pattern_matches() {
        let lint = IdentifierPatternLint::new(
            "no-shadow-pub-names",
            "",
            GateSeverity::uniform(Severity::Warn),
            IdentifierPatternConfig {
                item_kinds: vec![ItemKind::Fn],
                forbidden_prefixes: Vec::new(),
                forbidden_suffixes: Vec::new(),
                forbidden_regexes: vec!["^_[a-z]+_inner$".to_string()],
                visibility: Visibility::Any,
            },
        )
        .unwrap();
        let doc = MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "fn _foo_inner() {} fn outer() {}",
        );
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
    fn bad_regex_is_config_error() {
        let result = IdentifierPatternLint::new(
            "broken",
            "",
            GateSeverity::uniform(Severity::Warn),
            IdentifierPatternConfig {
                item_kinds: vec![ItemKind::Fn],
                forbidden_prefixes: Vec::new(),
                forbidden_suffixes: Vec::new(),
                forbidden_regexes: vec!["(unclosed".to_string()],
                visibility: Visibility::Any,
            },
        );
        match result {
            Ok(_) => panic!("expected ConfigError"),
            Err(e) => match e.kind {
                ConfigErrorKind::UnparseableRegex { .. } => {}
                other => panic!("unexpected: {other:?}"),
            },
        }
    }
}
