//! `registrable_completeness` bespoke primitive.
//!
//! Per schema design memo §4.16. For every `impl Trait for T` of the named
//! trait, validates that all required associated items are present with
//! sufficient signature complexity. Backs the workspace pattern where
//! "Registrable" traits must carry a complete metadata bundle.

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Span};
use serde::Deserialize;

use crate::config_types::ItemKind;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;
use crate::project::MockspaceProject;

pub const KIND: &str = "registrable-completeness";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct RegistrableCompletenessConfig {
    pub trait_name: String,
    pub required_items: Vec<RequiredItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct RequiredItem {
    pub name: String,
    pub kind: ItemKind,
    /// Heuristic: minimum number of tokens in the impl body (proxy for
    /// "implementer actually filled this in").
    #[serde(default)]
    pub min_signature_complexity: u32,
}

pub struct RegistrableCompletenessLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: RegistrableCompletenessConfig,
}

impl RegistrableCompletenessLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: RegistrableCompletenessConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for RegistrableCompletenessLint {
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
        if active.silent() {
            return Ok(());
        }
        for doc in project.documents() {
            let Some(file) = doc.ast() else {
                continue;
            };
            for item in &file.items {
                if let syn::Item::Impl(impl_block) = item {
                    let Some((_, trait_path, _)) = &impl_block.trait_ else {
                        continue;
                    };
                    let trait_name = trait_path
                        .segments
                        .last()
                        .map(|s| s.ident.to_string())
                        .unwrap_or_default();
                    if trait_name != self.config.trait_name {
                        continue;
                    }
                    // Walk impl items, mark which required items appear.
                    let mut seen: Vec<bool> = vec![false; self.config.required_items.len()];
                    for impl_item in &impl_block.items {
                        let (item_name, item_kind, complexity) = impl_item_info(impl_item);
                        for (i, req) in self.config.required_items.iter().enumerate() {
                            if req.name == item_name
                                && req.kind == item_kind
                                && complexity >= req.min_signature_complexity
                            {
                                seen[i] = true;
                            }
                        }
                    }
                    for (i, has) in seen.iter().enumerate() {
                        if !has {
                            let req = &self.config.required_items[i];
                            sink.emit(Finding {
                                lint_name: Cow::Borrowed(self.name),
                                rule_id: Some(Cow::Borrowed("incomplete-impl")),
                                plugin_id: None,
                                severity: active,
                                impact: None,
                                category: None,
                                message: Cow::Owned(format!(
                                    "impl {} for `{}` is missing required {:?} `{}` (or its body is below the minimum complexity {})",
                                    trait_name,
                                    impl_block_target(impl_block),
                                    req.kind,
                                    req.name,
                                    req.min_signature_complexity
                                )),
                                span: Span::single_line(doc.path(), 1, 1, 1),
                                hint: None,
                                help: None,
                                suggestion: None,
                                related_spans: Vec::new(),
                                metadata: None,
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn impl_item_info(item: &syn::ImplItem) -> (String, ItemKind, u32) {
    match item {
        syn::ImplItem::Fn(f) => {
            let body = format!("{:?}", f.block);
            let complexity = body.split_whitespace().count() as u32;
            (f.sig.ident.to_string(), ItemKind::Fn, complexity)
        }
        syn::ImplItem::Type(t) => {
            let ty_str = format!("{:?}", t.ty);
            let complexity = ty_str.split_whitespace().count() as u32;
            (t.ident.to_string(), ItemKind::TypeAlias, complexity)
        }
        syn::ImplItem::Const(c) => {
            let ty_str = format!("{:?}", c.ty);
            let complexity = ty_str.split_whitespace().count() as u32;
            (c.ident.to_string(), ItemKind::Const, complexity)
        }
        _ => (String::new(), ItemKind::Fn, 0),
    }
}

fn impl_block_target(impl_block: &syn::ItemImpl) -> String {
    if let syn::Type::Path(tp) = &*impl_block.self_ty {
        tp.path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_else(|| "?".to_string())
    } else {
        "?".to_string()
    }
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: RegistrableCompletenessConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("registrable-completeness config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(RegistrableCompletenessLint::new(
        name,
        description,
        default_severity,
        parsed,
    )))
}
