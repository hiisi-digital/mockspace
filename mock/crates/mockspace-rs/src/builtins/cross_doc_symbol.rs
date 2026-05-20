//! CrossDocSymbolCheck primitive.
//!
//! Per schema design memo §4.9. TwoPhaseProject lint that collects symbols
//! from source (Pass 1) and validates them against design-document
//! references (Pass 2). Predicate variants:
//!
//! - `NoDuplicatesAcrossCrates`: pub symbols of the same kind colliding
//!   across multiple crates.
//! - `SourceMustAppearInDoc`: every pub source symbol must appear
//!   backticked in any document matching the design-doc glob.
//! - `DocMustReferenceSource`: every backticked symbol in matching docs
//!   must have a matching pub source item.
//! - `MustMatchDeprecationEntry`: every pub source symbol must match an
//!   entry in the deprecated-CLs directory.
//! - `MustBeReferencedInDoc`: every pub symbol must trigger `ref_pattern`
//!   (substring or `re:` regex) in documents matching `doc_glob`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use globset::Glob;
use mockspace_core::lint::{Finding, GateSeverity, LintContext, Language, Severity, Span};
use regex::Regex;
use serde::Deserialize;

use crate::config_types::Visibility;
use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::project::MockspaceProject;

pub const KIND: &str = "cross-doc-symbol";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct CrossDocSymbolCheckConfig {
    pub symbol_kind: SymbolKind,

    #[serde(default)]
    pub visibility: Visibility,

    pub predicate: CrossDocPredicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Fn,
    Type,
    Trait,
    Const,
    Mod,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CrossDocPredicate {
    NoDuplicatesAcrossCrates,
    SourceMustAppearInDoc { design_doc_glob: String },
    DocMustReferenceSource { design_doc_glob: String },
    MustMatchDeprecationEntry { deprecated_cls_dir: PathBuf },
    MustBeReferencedInDoc { doc_glob: String, ref_pattern: String },
}

pub struct CrossDocSymbolCheckLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: CrossDocSymbolCheckConfig,
    compiled_glob: Option<globset::GlobMatcher>,
    compiled_ref_regex: Option<Regex>,
}

impl CrossDocSymbolCheckLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: CrossDocSymbolCheckConfig,
    ) -> Result<Self, ConfigError> {
        let (compiled_glob, compiled_ref_regex) = match &config.predicate {
            CrossDocPredicate::SourceMustAppearInDoc { design_doc_glob }
            | CrossDocPredicate::DocMustReferenceSource { design_doc_glob } => {
                let glob = compile_glob(name, design_doc_glob)?;
                (Some(glob), None)
            }
            CrossDocPredicate::MustBeReferencedInDoc {
                doc_glob,
                ref_pattern,
            } => {
                let glob = compile_glob(name, doc_glob)?;
                let regex = if let Some(re_pat) = ref_pattern.strip_prefix("re:") {
                    Some(Regex::new(re_pat).map_err(|e| ConfigError {
                        lint_name: name.to_string(),
                        field_path: "predicate.ref_pattern".to_string(),
                        kind: ConfigErrorKind::UnparseableRegex {
                            error: e.to_string(),
                        },
                        message: format!("ref pattern regex `{re_pat}` did not compile"),
                        source_location: None,
                    })?)
                } else {
                    None
                };
                (Some(glob), regex)
            }
            _ => (None, None),
        };
        Ok(Self {
            name,
            description,
            default_severity,
            config,
            compiled_glob,
            compiled_ref_regex,
        })
    }
}

fn compile_glob(lint_name: &'static str, pattern: &str) -> Result<globset::GlobMatcher, ConfigError> {
    Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| ConfigError {
            lint_name: lint_name.to_string(),
            field_path: "predicate.glob".to_string(),
            kind: ConfigErrorKind::UnparseableGlob {
                error: e.to_string(),
            },
            message: format!("glob `{pattern}` did not compile"),
            source_location: None,
        })
}

impl Lint for CrossDocSymbolCheckLint {
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

    fn check_project(
        &self,
        ctx: &LintContext<'_>,
        project: &MockspaceProject,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        let active = ctx.active_severity();
        // Pass 1: collect symbols. Indexed by name with crate + path.
        let symbols = collect_source_symbols(project, self.config.symbol_kind, &self.config.visibility);

        match &self.config.predicate {
            CrossDocPredicate::NoDuplicatesAcrossCrates => {
                let mut by_name: HashMap<&str, Vec<&SymbolEntry>> = HashMap::new();
                for sym in &symbols {
                    by_name.entry(&sym.name).or_default().push(sym);
                }
                for (name, entries) in by_name {
                    if entries.len() > 1 {
                        let crates: Vec<_> =
                            entries.iter().map(|e| e.crate_name.as_str()).collect();
                        sink.emit(Finding {
                            lint_name: Cow::Borrowed(self.name),
                            rule_id: Some(Cow::Borrowed("duplicate-across-crates")),
                            plugin_id: None,
                            severity: active,
                            impact: None,
                            category: None,
                            message: Cow::Owned(format!(
                                "symbol `{name}` is pub in multiple crates: [{}]",
                                crates.join(", ")
                            )),
                            span: entries[0].span.clone(),
                            fix_suggestion: None,
                            related_spans: Vec::new(),
                            metadata: None,
                        });
                    }
                }
            }
            CrossDocPredicate::SourceMustAppearInDoc { .. } => {
                let backticked = collect_backticked_symbols(project, self.compiled_glob.as_ref());
                for sym in &symbols {
                    if !backticked.contains(&sym.name) {
                        sink.emit(Finding {
                            lint_name: Cow::Borrowed(self.name),
                            rule_id: Some(Cow::Borrowed("source-not-in-doc")),
                            plugin_id: None,
                            severity: active,
                            impact: None,
                            category: None,
                            message: Cow::Owned(format!(
                                "pub symbol `{}` does not appear backticked in any design doc",
                                sym.name
                            )),
                            span: sym.span.clone(),
                            fix_suggestion: None,
                            related_spans: Vec::new(),
                            metadata: None,
                        });
                    }
                }
            }
            CrossDocPredicate::DocMustReferenceSource { .. } => {
                let source_names: HashSet<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
                let backticked_with_loc =
                    collect_backticked_symbols_with_loc(project, self.compiled_glob.as_ref());
                for (name, (path, line)) in backticked_with_loc {
                    if !source_names.contains(name.as_str()) {
                        sink.emit(Finding {
                            lint_name: Cow::Borrowed(self.name),
                            rule_id: Some(Cow::Borrowed("doc-claim-missing-source")),
                            plugin_id: None,
                            severity: active,
                            impact: None,
                            category: None,
                            message: Cow::Owned(format!(
                                "doc backticks `{name}` but no matching pub item exists in source"
                            )),
                            span: Span::single_line(&path, line, 1, name.len() as u32),
                            fix_suggestion: None,
                            related_spans: Vec::new(),
                            metadata: None,
                        });
                    }
                }
            }
            CrossDocPredicate::MustMatchDeprecationEntry { deprecated_cls_dir } => {
                let _ = deprecated_cls_dir;
                // Future hook: walk deprecated CLs in this dir, compare
                // their listed symbols against source. Stub today.
            }
            CrossDocPredicate::MustBeReferencedInDoc { ref_pattern, .. } => {
                let mut doc_contents = String::new();
                for doc in project.documents() {
                    if !is_doc_language(doc.language()) {
                        continue;
                    }
                    let matches = self
                        .compiled_glob
                        .as_ref()
                        .map(|g| g.is_match(doc.path()))
                        .unwrap_or(true);
                    if matches {
                        doc_contents.push_str(doc.source());
                        doc_contents.push('\n');
                    }
                }
                for sym in &symbols {
                    let present = if let Some(re) = &self.compiled_ref_regex {
                        re.is_match(&doc_contents)
                    } else {
                        doc_contents.contains(ref_pattern)
                    };
                    if !present {
                        sink.emit(Finding {
                            lint_name: Cow::Borrowed(self.name),
                            rule_id: Some(Cow::Borrowed("symbol-not-referenced")),
                            plugin_id: None,
                            severity: active,
                            impact: None,
                            category: None,
                            message: Cow::Owned(format!(
                                "pub symbol `{}` is not referenced in matching docs by pattern `{ref_pattern}`",
                                sym.name
                            )),
                            span: sym.span.clone(),
                            fix_suggestion: None,
                            related_spans: Vec::new(),
                            metadata: None,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct SymbolEntry {
    name: String,
    crate_name: String,
    span: Span,
}

fn collect_source_symbols(
    project: &MockspaceProject,
    kind: SymbolKind,
    visibility: &Visibility,
) -> Vec<SymbolEntry> {
    let mut out = Vec::new();
    for doc in project.documents() {
        if doc.language() != Language::Rust {
            continue;
        }
        let Some(file) = doc.ast() else {
            continue;
        };
        for item in &file.items {
            if let Some((name, item_kind, is_pub)) = item_kind_info(item) {
                if item_kind != kind {
                    continue;
                }
                if *visibility == Visibility::Public && !is_pub {
                    continue;
                }
                out.push(SymbolEntry {
                    name,
                    crate_name: doc.crate_name().to_string(),
                    span: Span::single_line(doc.path(), 1, 1, 1),
                });
            }
        }
    }
    out
}

fn item_kind_info(item: &syn::Item) -> Option<(String, SymbolKind, bool)> {
    match item {
        syn::Item::Fn(it) => Some((
            it.sig.ident.to_string(),
            SymbolKind::Fn,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Struct(it) => Some((
            it.ident.to_string(),
            SymbolKind::Type,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Enum(it) => Some((
            it.ident.to_string(),
            SymbolKind::Type,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Type(it) => Some((
            it.ident.to_string(),
            SymbolKind::Type,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Trait(it) => Some((
            it.ident.to_string(),
            SymbolKind::Trait,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Const(it) => Some((
            it.ident.to_string(),
            SymbolKind::Const,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        syn::Item::Mod(it) => Some((
            it.ident.to_string(),
            SymbolKind::Mod,
            matches!(it.vis, syn::Visibility::Public(_)),
        )),
        _ => None,
    }
}

fn collect_backticked_symbols(
    project: &MockspaceProject,
    glob: Option<&globset::GlobMatcher>,
) -> HashSet<String> {
    let mut out = HashSet::new();
    for doc in project.documents() {
        if !is_doc_language(doc.language()) {
            continue;
        }
        if let Some(g) = glob {
            if !g.is_match(doc.path()) {
                continue;
            }
        }
        for cap in BACKTICK_REGEX.captures_iter(doc.source()) {
            if let Some(m) = cap.get(1) {
                out.insert(m.as_str().to_string());
            }
        }
    }
    out
}

fn collect_backticked_symbols_with_loc(
    project: &MockspaceProject,
    glob: Option<&globset::GlobMatcher>,
) -> Vec<(String, (PathBuf, u32))> {
    let mut out = Vec::new();
    for doc in project.documents() {
        if !is_doc_language(doc.language()) {
            continue;
        }
        if let Some(g) = glob {
            if !g.is_match(doc.path()) {
                continue;
            }
        }
        let source = doc.source();
        for (line_idx, line) in source.lines().enumerate() {
            for cap in BACKTICK_REGEX.captures_iter(line) {
                if let Some(m) = cap.get(1) {
                    out.push((
                        m.as_str().to_string(),
                        (doc.path().to_path_buf(), (line_idx + 1) as u32),
                    ));
                }
            }
        }
    }
    out
}

fn is_doc_language(language: Language) -> bool {
    matches!(language, Language::Markdown)
}

static BACKTICK_REGEX_INIT: once_cell::sync::Lazy<Regex> =
    once_cell::sync::Lazy::new(|| Regex::new(r"`([A-Za-z_][A-Za-z0-9_]*)`").unwrap());

// Use a deref-able accessor to keep the call sites readable.
#[allow(non_upper_case_globals)]
const BACKTICK_REGEX: BacktickRegex = BacktickRegex;

struct BacktickRegex;

impl std::ops::Deref for BacktickRegex {
    type Target = Regex;
    fn deref(&self) -> &Self::Target {
        &BACKTICK_REGEX_INIT
    }
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: CrossDocSymbolCheckConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("cross-doc-symbol config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(CrossDocSymbolCheckLint::new(
        name,
        description,
        default_severity,
        parsed,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding_sink::VecFindingSink;
    use crate::project::ProjectBuilder;
    use mockspace_core::lint::{Gate, RunSurface, Severity};
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
            surface: RunSurface::Local,
            project_root: root,
            config: cfg,
        }
    }

    #[test]
    fn detects_duplicate_pub_symbol_across_crates() {
        let lint = CrossDocSymbolCheckLint::new(
            "no-duplicates",
            "",
            GateSeverity::uniform(Severity::Warn),
            CrossDocSymbolCheckConfig {
                symbol_kind: SymbolKind::Fn,
                visibility: Visibility::Public,
                predicate: CrossDocPredicate::NoDuplicatesAcrossCrates,
            },
        )
        .unwrap();
        let mut builder = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "a/src/lib.rs",
            "crate-a",
            Language::Rust,
            "pub fn dup() {}",
        ));
        builder.push_document(MockspaceDocument::new(
            "b/src/lib.rs",
            "crate-b",
            Language::Rust,
            "pub fn dup() {}",
        ));
        let project = builder.build();
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_project(&ctx, &project, &sink).unwrap();
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("dup"));
    }

    #[test]
    fn detects_doc_claim_with_missing_source() {
        let lint = CrossDocSymbolCheckLint::new(
            "design-doc-source-mismatch",
            "",
            GateSeverity::uniform(Severity::Warn),
            CrossDocSymbolCheckConfig {
                symbol_kind: SymbolKind::Fn,
                visibility: Visibility::Public,
                predicate: CrossDocPredicate::DocMustReferenceSource {
                    design_doc_glob: "**/*.md".to_string(),
                },
            },
        )
        .unwrap();
        let mut builder = ProjectBuilder::new("/tmp", RunSurface::Local, Gate::Commit);
        builder.push_document(MockspaceDocument::new(
            "DESIGN.md",
            "crate-a",
            Language::Markdown,
            "We ship `non_existent_fn` here.",
        ));
        let project = builder.build();
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_project(&ctx, &project, &sink).unwrap();
        let findings = sink.into_findings();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("non_existent_fn"));
    }
}
