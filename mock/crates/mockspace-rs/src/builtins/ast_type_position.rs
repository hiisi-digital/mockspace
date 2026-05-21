//! AstTypePosition primitive.
//!
//! Per schema design memo §4.3. Walks the syn AST inspecting type-bearing
//! positions (struct fields, fn params, fn returns, type aliases, assoc
//! types) for forbidden type names. The workhorse for the bare-primitive
//! family of lints: `no-bare-numeric`, `no-bare-option`, `no-bare-result`,
//! `no-bare-string`, `no-vec-in-trait-sig`, `arvo-types-only`,
//! `semantic-alias-nudge`.

use std::borrow::Cow;
use std::collections::HashSet;

use mockspace_core::lint::{Finding, Fix, GateSeverity, LintContext, Severity, Span, Suggestion};
use serde::Deserialize;
use syn::visit::Visit;

use crate::config_types::{TypePosition, Visibility};
use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;

pub const KIND: &str = "ast-type-position";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct AstTypePositionConfig {
    /// Type names to fire on. Matches the leading path segment ident
    /// (e.g. `String` matches both `String` and `std::string::String`).
    pub forbidden_types: Vec<String>,

    /// Positions to inspect.
    pub positions: Vec<TypePosition>,

    /// Visibility filter. Affects which containing item gates the walk.
    #[serde(default)]
    pub visibility: Visibility,

    /// Optional `(forbidden, replacement)` table for fix suggestions.
    /// When a forbidden type is matched and the table has a replacement,
    /// the Finding carries a FixSuggestion replacing the type name.
    #[serde(default)]
    pub replacements: Vec<(String, String)>,
}

pub struct AstTypePositionLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: AstTypePositionConfig,
    forbidden: HashSet<String>,
    positions: HashSet<TypePosition>,
}

impl AstTypePositionLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: AstTypePositionConfig,
    ) -> Self {
        let forbidden: HashSet<String> = config.forbidden_types.iter().cloned().collect();
        let positions: HashSet<TypePosition> = config.positions.iter().copied().collect();
        Self {
            name,
            description,
            default_severity,
            config,
            forbidden,
            positions,
        }
    }

    fn replacement_for(&self, ty: &str) -> Option<&str> {
        self.config
            .replacements
            .iter()
            .find(|(f, _)| f == ty)
            .map(|(_, r)| r.as_str())
    }
}

impl Lint for AstTypePositionLint {
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
        let mut visitor = TypePositionVisitor {
            lint: self,
            doc,
            ctx,
            sink,
            visibility_stack: Vec::new(),
        };
        visitor.visit_file(file);
        Ok(())
    }
}

struct TypePositionVisitor<'a> {
    lint: &'a AstTypePositionLint,
    doc: &'a MockspaceDocument,
    ctx: &'a LintContext<'a>,
    sink: &'a dyn FindingSink,
    /// Stack of `(item_is_pub)` flags as we descend. Used by visibility
    /// gating on positions inside structs / enums / impls / traits.
    visibility_stack: Vec<bool>,
}

impl<'a> TypePositionVisitor<'a> {
    fn visit_type_at(&self, ty: &syn::Type, position: TypePosition) {
        if !self.lint.positions.contains(&position) {
            return;
        }
        if self.lint.config.visibility == Visibility::Public
            && !self.visibility_stack.last().copied().unwrap_or(true)
        {
            return;
        }
        walk_type(ty, &mut |ident: &str| {
            if self.lint.forbidden.contains(ident) {
                let replacement = self.lint.replacement_for(ident);
                emit(
                    self.lint.name,
                    self.doc.path(),
                    ident,
                    replacement,
                    self.ctx.active_severity(),
                    self.sink,
                );
            }
        });
    }
}

impl<'a, 'ast> Visit<'ast> for TypePositionVisitor<'a> {
    fn visit_item_struct(&mut self, i: &'ast syn::ItemStruct) {
        let is_pub = matches!(i.vis, syn::Visibility::Public(_));
        self.visibility_stack.push(is_pub);
        for field in i.fields.iter() {
            self.visit_type_at(&field.ty, TypePosition::StructField);
        }
        self.visibility_stack.pop();
    }

    fn visit_item_enum(&mut self, i: &'ast syn::ItemEnum) {
        let is_pub = matches!(i.vis, syn::Visibility::Public(_));
        self.visibility_stack.push(is_pub);
        for variant in &i.variants {
            for field in variant.fields.iter() {
                self.visit_type_at(&field.ty, TypePosition::EnumVariantField);
            }
        }
        self.visibility_stack.pop();
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        let is_pub = matches!(i.vis, syn::Visibility::Public(_));
        self.visibility_stack.push(is_pub);
        for input in &i.sig.inputs {
            if let syn::FnArg::Typed(pat_ty) = input {
                self.visit_type_at(&pat_ty.ty, TypePosition::FnParam);
            }
        }
        if let syn::ReturnType::Type(_, ty) = &i.sig.output {
            self.visit_type_at(ty, TypePosition::FnReturn);
        }
        self.visibility_stack.pop();
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        let is_pub = matches!(i.vis, syn::Visibility::Public(_));
        self.visibility_stack.push(is_pub);
        for item in &i.items {
            if let syn::TraitItem::Fn(method) = item {
                for input in &method.sig.inputs {
                    if let syn::FnArg::Typed(pat_ty) = input {
                        self.visit_type_at(&pat_ty.ty, TypePosition::FnParam);
                    }
                }
                if let syn::ReturnType::Type(_, ty) = &method.sig.output {
                    self.visit_type_at(ty, TypePosition::FnReturn);
                }
            }
        }
        self.visibility_stack.pop();
    }

    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        // Treat impl methods like inherent fns; visibility flows from the
        // method's own `pub` (not the impl block).
        for item in &i.items {
            if let syn::ImplItem::Fn(method) = item {
                let is_pub = matches!(method.vis, syn::Visibility::Public(_));
                self.visibility_stack.push(is_pub);
                for input in &method.sig.inputs {
                    if let syn::FnArg::Typed(pat_ty) = input {
                        self.visit_type_at(&pat_ty.ty, TypePosition::FnParam);
                    }
                }
                if let syn::ReturnType::Type(_, ty) = &method.sig.output {
                    self.visit_type_at(ty, TypePosition::FnReturn);
                }
                self.visibility_stack.pop();
            }
        }
    }

    fn visit_item_type(&mut self, i: &'ast syn::ItemType) {
        let is_pub = matches!(i.vis, syn::Visibility::Public(_));
        self.visibility_stack.push(is_pub);
        self.visit_type_at(&i.ty, TypePosition::TypeAliasBody);
        self.visibility_stack.pop();
    }
}

/// Walk a type, calling `on_ident` for each path-leading identifier.
fn walk_type(ty: &syn::Type, on_ident: &mut dyn FnMut(&str)) {
    match ty {
        syn::Type::Path(tp) => {
            // Match against each path segment's ident. This catches both
            // `Vec` and `std::vec::Vec` etc.
            for seg in &tp.path.segments {
                on_ident(&seg.ident.to_string());
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    for arg in &args.args {
                        if let syn::GenericArgument::Type(inner) = arg {
                            walk_type(inner, on_ident);
                        }
                    }
                }
            }
        }
        syn::Type::Reference(r) => walk_type(&r.elem, on_ident),
        syn::Type::Slice(s) => walk_type(&s.elem, on_ident),
        syn::Type::Array(a) => walk_type(&a.elem, on_ident),
        syn::Type::Tuple(t) => {
            for inner in &t.elems {
                walk_type(inner, on_ident);
            }
        }
        syn::Type::Ptr(p) => walk_type(&p.elem, on_ident),
        syn::Type::Paren(p) => walk_type(&p.elem, on_ident),
        syn::Type::Group(g) => walk_type(&g.elem, on_ident),
        // Other variants (BareFn, ImplTrait, TraitObject, Macro, Infer)
        // do not carry simple ident paths the bare-primitive lints care
        // about. ImplTrait could in principle (e.g. `impl Iterator<Item = u8>`),
        // but the bare-primitive policy treats those as already-typed.
        _ => {}
    }
}

fn emit(
    lint_name: &'static str,
    path: &std::path::Path,
    forbidden_ty: &str,
    replacement: Option<&str>,
    severity: Severity,
    sink: &dyn FindingSink,
) {
    let message = match replacement {
        Some(r) => format!("forbidden type `{forbidden_ty}` in this position; use `{r}` instead"),
        None => format!("forbidden type `{forbidden_ty}` in this position"),
    };
    sink.emit(Finding {
        lint_name: Cow::Borrowed(lint_name),
        rule_id: None,
        plugin_id: None,
        severity,
        impact: None,
        category: None,
        message: Cow::Owned(message),
        span: Span::single_line(path, 1, 1, forbidden_ty.len() as u32),
        hint: None,
        help: None,
        // Description only; no Fix recipe. The Span coordinates here
        // are fabricated (line 1, col 1, length-as-width) because the
        // syn visitor surface in this lint has not been threaded through
        // for real source positions yet. Shipping a Fix::Replace with
        // those coordinates would corrupt files at byte 0; emit advice
        // until the visitor is updated to carry proc_macro2::Span.
        suggestion: replacement.map(|r| Suggestion {
            description: Cow::Owned(format!("replace with canonical type `{r}`")),
            fix: None,
        }),
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
    let parsed: AstTypePositionConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("ast-type-position config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(AstTypePositionLint::new(
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

    fn run(source: &str, config: AstTypePositionConfig) -> Vec<mockspace_core::lint::Finding> {
        let lint =
            AstTypePositionLint::new("test", "", GateSeverity::uniform(Severity::Warn), config);
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
    fn fires_on_pub_fn_param() {
        let findings = run(
            "pub fn x(s: String) {}",
            AstTypePositionConfig {
                forbidden_types: vec!["String".to_string()],
                positions: vec![TypePosition::FnParam],
                visibility: Visibility::Public,
                replacements: Vec::new(),
            },
        );
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("String"));
    }

    #[test]
    fn does_not_fire_on_private_fn_with_public_filter() {
        let findings = run(
            "fn x(s: String) {}",
            AstTypePositionConfig {
                forbidden_types: vec!["String".to_string()],
                positions: vec![TypePosition::FnParam],
                visibility: Visibility::Public,
                replacements: Vec::new(),
            },
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn fires_on_any_visibility() {
        let findings = run(
            "fn x(s: String) {}",
            AstTypePositionConfig {
                forbidden_types: vec!["String".to_string()],
                positions: vec![TypePosition::FnParam],
                visibility: Visibility::Any,
                replacements: Vec::new(),
            },
        );
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn fires_on_struct_field() {
        let findings = run(
            "pub struct S { pub v: Vec<u8> }",
            AstTypePositionConfig {
                forbidden_types: vec!["Vec".to_string()],
                positions: vec![TypePosition::StructField],
                visibility: Visibility::Public,
                replacements: Vec::new(),
            },
        );
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn replacement_appears_in_fix_suggestion() {
        let findings = run(
            "pub fn x() -> String { String::new() }",
            AstTypePositionConfig {
                forbidden_types: vec!["String".to_string()],
                positions: vec![TypePosition::FnReturn],
                visibility: Visibility::Public,
                replacements: vec![("String".to_string(), "Str".to_string())],
            },
        );
        assert_eq!(findings.len(), 1);
        let suggestion = findings[0].suggestion.as_ref().unwrap();
        assert!(
            suggestion.description.contains("Str"),
            "description should name the replacement: {}",
            suggestion.description,
        );
        // Fix is None today; the syn visitor surface has not been threaded
        // for real proc_macro2::Span coordinates. See ast_type_position::emit.
        assert!(suggestion.fix.is_none());
    }

    #[test]
    fn nested_generic_args_are_walked() {
        // Verify that walk_type recurses into generic args:
        // `Option<Vec<u8>>` should match if `Vec` is forbidden.
        let findings = run(
            "pub fn x(v: Option<Vec<u8>>) {}",
            AstTypePositionConfig {
                forbidden_types: vec!["Vec".to_string()],
                positions: vec![TypePosition::FnParam],
                visibility: Visibility::Public,
                replacements: Vec::new(),
            },
        );
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn trait_method_signatures_are_walked() {
        let findings = run(
            "pub trait T { fn x(v: Vec<u8>); }",
            AstTypePositionConfig {
                forbidden_types: vec!["Vec".to_string()],
                positions: vec![TypePosition::FnParam],
                visibility: Visibility::Public,
                replacements: Vec::new(),
            },
        );
        assert_eq!(findings.len(), 1);
    }
}
