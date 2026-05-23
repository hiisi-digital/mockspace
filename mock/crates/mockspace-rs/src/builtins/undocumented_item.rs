//! UndocumentedItem primitive.
//!
//! Per schema design memo §4.8. Walks the syn AST for items of selected
//! kinds and fires on any whose `///` doc-comment attributes are absent
//! or empty. Optionally allows a SHAME-file escape: if the item's name
//! appears in a SHAME.md file with a long-enough rationale, the finding
//! is suppressed.

use std::borrow::Cow;
use std::path::PathBuf;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Span};
use serde::Deserialize;

use crate::config_types::{ItemKind, Visibility};
use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;

pub const KIND: &str = "undocumented-item";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct UndocumentedItemConfig {
    pub item_kinds: Vec<ItemKind>,

    #[serde(default)]
    pub visibility: Visibility,

    /// Optional SHAME escape: if set, allows items to skip the doc
    /// requirement by appearing in a per-crate SHAME file with a
    /// rationale meeting `min_words`.
    #[serde(default)]
    pub shame_escape: Option<ShameEscapeRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShameEscapeRule {
    pub min_words: u32,
    pub shame_path: PathBuf,
}

pub struct UndocumentedItemLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: UndocumentedItemConfig,
}

impl UndocumentedItemLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: UndocumentedItemConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for UndocumentedItemLint {
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
        let active = ctx.active_severity();
        let path = doc.path();
        for item in &file.items {
            let Some((name, kind, is_pub, attrs)) = item_info(item) else {
                continue;
            };
            if !kind_matches(&self.config.item_kinds, kind) {
                continue;
            }
            if self.config.visibility == Visibility::Public && !is_pub {
                continue;
            }
            if has_doc_comment(attrs) {
                continue;
            }
            // SHAME escape check is a future hook; without per-project
            // SHAME content available, the default behaviour is to fire.
            sink.emit(Finding {
                lint_name: Cow::Borrowed(self.name),
                rule_id: None,
                plugin_id: None,
                severity: active,
                impact: None,
                category: None,
                message: Cow::Owned(format!("{kind:?} `{name}` is undocumented")),
                span: Span::single_line(path, 1, 1, name.len() as u32),
                hint: None,
                help: None,
                suggestion: None,
                related_spans: Vec::new(),
                metadata: None,
            });
        }
        Ok(())
    }
}

fn item_info(item: &syn::Item) -> Option<(String, ItemKind, bool, &[syn::Attribute])> {
    match item {
        syn::Item::Fn(it) => Some((
            it.sig.ident.to_string(),
            ItemKind::Fn,
            matches!(it.vis, syn::Visibility::Public(_)),
            &it.attrs,
        )),
        syn::Item::Struct(it) => Some((
            it.ident.to_string(),
            ItemKind::Struct,
            matches!(it.vis, syn::Visibility::Public(_)),
            &it.attrs,
        )),
        syn::Item::Enum(it) => Some((
            it.ident.to_string(),
            ItemKind::Enum,
            matches!(it.vis, syn::Visibility::Public(_)),
            &it.attrs,
        )),
        syn::Item::Trait(it) => Some((
            it.ident.to_string(),
            ItemKind::Trait,
            matches!(it.vis, syn::Visibility::Public(_)),
            &it.attrs,
        )),
        syn::Item::Type(it) => Some((
            it.ident.to_string(),
            ItemKind::TypeAlias,
            matches!(it.vis, syn::Visibility::Public(_)),
            &it.attrs,
        )),
        syn::Item::Const(it) => Some((
            it.ident.to_string(),
            ItemKind::Const,
            matches!(it.vis, syn::Visibility::Public(_)),
            &it.attrs,
        )),
        syn::Item::Static(it) => Some((
            it.ident.to_string(),
            ItemKind::Static,
            matches!(it.vis, syn::Visibility::Public(_)),
            &it.attrs,
        )),
        syn::Item::Mod(it) => Some((
            it.ident.to_string(),
            ItemKind::Mod,
            matches!(it.vis, syn::Visibility::Public(_)),
            &it.attrs,
        )),
        _ => None,
    }
}

fn kind_matches(allowed: &[ItemKind], kind: ItemKind) -> bool {
    allowed.is_empty() || allowed.contains(&kind)
}

fn has_doc_comment(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("doc"))
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: UndocumentedItemConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("undocumented-item config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(UndocumentedItemLint::new(
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

    fn run(source: &str) -> Vec<mockspace_core::lint::Finding> {
        let lint = UndocumentedItemLint::new(
            "missing-docs",
            "",
            GateSeverity::uniform(Severity::Warn),
            UndocumentedItemConfig {
                item_kinds: vec![ItemKind::Fn, ItemKind::Struct],
                visibility: Visibility::Public,
                shame_escape: None,
            },
        );
        let doc = MockspaceDocument::new("a.rs", "t", Language::Rust, source);
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        sink.into_findings()
    }

    #[test]
    fn fires_on_undocumented_pub_fn() {
        let findings = run("pub fn x() {}");
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("undocumented"));
    }

    #[test]
    fn does_not_fire_on_documented_item() {
        let findings = run("/// docs\npub fn x() {}");
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_fire_on_private_item() {
        let findings = run("fn x() {}");
        assert!(findings.is_empty());
    }

    #[test]
    fn fires_on_undocumented_pub_struct() {
        let findings = run("pub struct S;");
        assert_eq!(findings.len(), 1);
    }
}
