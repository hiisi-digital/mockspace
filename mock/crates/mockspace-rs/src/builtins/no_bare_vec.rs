//! `no_bare_vec` bespoke primitive.
//!
//! Per schema design memo §4.12. Two phases in one lint:
//!
//! - Phase 1 mirrors `AstTypePosition`: walk the syn AST for forbidden
//!   collection types in type-bearing positions.
//! - Phase 2 scans macro invocation bodies (e.g. `define_resource! {
//!   initial = vec![...] }`) for forbidden tokens, recursing into nested
//!   macros up to `max_recursion_depth`.
//!
//! Bespoke because the macro-body recursion logic does not match any
//! single reusable primitive cleanly.

use std::borrow::Cow;
use std::collections::HashSet;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use serde::Deserialize;
use syn::spanned::Spanned;
use syn::visit::Visit;

use crate::config_types::{TypePosition, Visibility};
use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;

pub const KIND: &str = "no-bare-vec";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct NoBareVecConfig {
    /// Phase 1: AST walk over type-bearing positions.
    pub forbidden_types: Vec<String>,
    pub positions:       Vec<TypePosition>,
    #[serde(default)]
    pub visibility:      Visibility,

    /// Phase 2: text scan inside `define_*!` macro invocation bodies.
    /// The current impl scans the outermost macro tokens for the listed
    /// substrings; nested-macro recursion is not yet wired (the AST
    /// reaches macro tokens as opaque TokenStreams, and the proc-macro2
    /// walk needed to descend into nested invocations is deferred to
    /// Phase 2E).
    #[serde(default)]
    pub macro_body_tokens: Vec<String>,
    #[serde(default)]
    pub macros:            Vec<String>,
}

pub struct NoBareVecLint {
    name:             &'static str,
    description:      &'static str,
    default_severity: GateSeverity,
    config:           NoBareVecConfig,
    forbidden_types:  HashSet<String>,
    positions:        HashSet<TypePosition>,
    macros:           HashSet<String>,
    macro_tokens:     Vec<Vec<u8>>,
}

impl NoBareVecLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: NoBareVecConfig,
    ) -> Self {
        let forbidden_types = config.forbidden_types.iter().cloned().collect();
        let positions = config.positions.iter().copied().collect();
        let macros = config.macros.iter().cloned().collect();
        let macro_tokens = config
            .macro_body_tokens
            .iter()
            .map(|t| t.as_bytes().to_vec())
            .collect();
        Self {
            name,
            description,
            default_severity,
            config,
            forbidden_types,
            positions,
            macros,
            macro_tokens,
        }
    }
}

impl Lint for NoBareVecLint {
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

        // Phase 1: AST walk.
        let mut visitor = TypeVisitor {
            lint: self,
            doc,
            severity: active,
            sink,
            visibility_stack: Vec::new(),
        };
        visitor.visit_file(file);

        // Phase 2: macro-body scan.
        for item in &file.items {
            scan_macros_in_item(self, item, doc, active, sink, 0);
        }
        Ok(())
    }
}

struct TypeVisitor<'a> {
    lint:             &'a NoBareVecLint,
    doc:              &'a MockspaceDocument,
    severity:         Severity,
    sink:             &'a dyn FindingSink,
    visibility_stack: Vec<bool>,
}

impl<'a> TypeVisitor<'a> {
    fn check(&self, ty: &syn::Type, position: TypePosition) {
        if !self.lint.positions.contains(&position) {
            return;
        }
        if self.lint.config.visibility == Visibility::Public
            && !self.visibility_stack.last().copied().unwrap_or(true)
        {
            return;
        }
        let span = ty.span();
        let start = span.start();
        let line = start.line as u32;
        let column = (start.column as u32).saturating_add(1);
        walk_type(ty, &mut |ident| {
            if self.lint.forbidden_types.contains(ident) {
                emit(
                    self.lint.name,
                    self.doc.path(),
                    ident,
                    self.severity,
                    self.sink,
                    "type-position",
                    line,
                    column,
                );
            }
        });
    }
}

impl<'a, 'ast> Visit<'ast> for TypeVisitor<'a> {
    fn visit_item_struct(&mut self, i: &'ast syn::ItemStruct) {
        let is_pub = matches!(i.vis, syn::Visibility::Public(_));
        self.visibility_stack.push(is_pub);
        for field in i.fields.iter() {
            self.check(&field.ty, TypePosition::StructField);
        }
        self.visibility_stack.pop();
    }

    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        let is_pub = matches!(i.vis, syn::Visibility::Public(_));
        self.visibility_stack.push(is_pub);
        for input in &i.sig.inputs {
            if let syn::FnArg::Typed(pat_ty) = input {
                self.check(&pat_ty.ty, TypePosition::FnParam);
            }
        }
        if let syn::ReturnType::Type(_, ty) = &i.sig.output {
            self.check(ty, TypePosition::FnReturn);
        }
        self.visibility_stack.pop();
    }

    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        let is_pub = matches!(i.vis, syn::Visibility::Public(_));
        self.visibility_stack.push(is_pub);
        for item in &i.items {
            if let syn::TraitItem::Fn(m) = item {
                for input in &m.sig.inputs {
                    if let syn::FnArg::Typed(pat_ty) = input {
                        self.check(&pat_ty.ty, TypePosition::FnParam);
                    }
                }
                if let syn::ReturnType::Type(_, ty) = &m.sig.output {
                    self.check(ty, TypePosition::FnReturn);
                }
            }
        }
        self.visibility_stack.pop();
    }
}

fn walk_type(ty: &syn::Type, on_ident: &mut dyn FnMut(&str)) {
    match ty {
        syn::Type::Path(tp) => {
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
        },
        syn::Type::Reference(r) => walk_type(&r.elem, on_ident),
        syn::Type::Slice(s) => walk_type(&s.elem, on_ident),
        syn::Type::Array(a) => walk_type(&a.elem, on_ident),
        syn::Type::Tuple(t) => {
            for inner in &t.elems {
                walk_type(inner, on_ident);
            }
        },
        syn::Type::Paren(p) => walk_type(&p.elem, on_ident),
        syn::Type::Group(g) => walk_type(&g.elem, on_ident),
        _ => {},
    }
}

fn scan_macros_in_item(
    lint: &NoBareVecLint,
    item: &syn::Item,
    doc: &MockspaceDocument,
    severity: Severity,
    sink: &dyn FindingSink,
    _depth: u32,
) {
    if lint.macros.is_empty() || lint.macro_tokens.is_empty() {
        return;
    }
    if let syn::Item::Macro(m) = item {
        let macro_name = m
            .mac
            .path
            .segments
            .last()
            .map(|s| s.ident.to_string())
            .unwrap_or_default();
        if lint.macros.contains(&macro_name) {
            let span = m.mac.tokens.span();
            let start = span.start();
            let line = start.line as u32;
            let column = (start.column as u32).saturating_add(1);
            let tokens_str = m.mac.tokens.to_string();
            for forbidden in &lint.macro_tokens {
                if contains_subslice(tokens_str.as_bytes(), forbidden) {
                    let forbidden_str = String::from_utf8_lossy(forbidden);
                    emit(
                        lint.name,
                        doc.path(),
                        &forbidden_str,
                        severity,
                        sink,
                        &format!("macro-body in {macro_name}!"),
                        line,
                        column,
                    );
                }
            }
        }
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    (0 ..= haystack.len() - needle.len()).any(|i| &haystack[i .. i + needle.len()] == needle)
}

#[allow(clippy::too_many_arguments)]
fn emit(
    lint_name: &'static str,
    path: &std::path::Path,
    forbidden: &str,
    severity: Severity,
    sink: &dyn FindingSink,
    context: &str,
    line: u32,
    column: u32,
) {
    if severity.silent() {
        return;
    }
    sink.emit(Finding {
        lint_name: Cow::Borrowed(lint_name),
        rule_id: Some(Cow::Owned(context.to_string())),
        plugin_id: None,
        severity,
        impact: None,
        category: None,
        message: Cow::Owned(format!("forbidden collection `{forbidden}` ({context})")),
        span: Span::single_line(path, line.max(1), column.max(1), forbidden.len() as u32),
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
    let parsed: NoBareVecConfig = config.clone().try_into().map_err(|e: toml::de::Error| {
        ConfigError {
            lint_name:       name.to_string(),
            field_path:      String::new(),
            kind:            ConfigErrorKind::InvalidValue,
            message:         format!("no-bare-vec config: {e}"),
            source_location: None,
        }
    })?;
    Ok(Box::new(NoBareVecLint::new(
        name,
        description,
        default_severity,
        parsed,
    )))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mockspace_core::lint::{Gate, RunSurface};

    use super::*;
    use crate::config_types::Language;
    use crate::finding_sink::VecFindingSink;

    struct EmptyCfg;
    impl mockspace_core::lint::LintCfgStore for EmptyCfg {
        fn get(&self, _: &str) -> Option<&toml::Table> {
            None
        }
    }

    fn make_ctx<'a>(root: &'a PathBuf, sev: GateSeverity, cfg: &'a EmptyCfg) -> LintContext<'a> {
        LintContext {
            gate:         Gate::Commit,
            severities:   sev,
            surface:      RunSurface::Local,
            project_root: root,
            config:       cfg,
        }
    }

    #[test]
    fn phase1_fires_on_pub_fn_param() {
        let lint = NoBareVecLint::new(
            "no-bare-vec",
            "",
            GateSeverity::uniform(Severity::Warn),
            NoBareVecConfig {
                forbidden_types:   vec!["Vec".to_string()],
                positions:         vec![TypePosition::FnParam],
                visibility:        Visibility::Public,
                macro_body_tokens: Vec::new(),
                macros:            Vec::new(),
            },
        );
        let doc = MockspaceDocument::new("a.rs", "t", Language::Rust, "pub fn x(v: Vec<u8>) {}");
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        assert_eq!(sink.into_findings().len(), 1);
    }

    #[test]
    fn phase2_fires_on_token_inside_macro_body() {
        let lint = NoBareVecLint::new(
            "no-bare-vec",
            "",
            GateSeverity::uniform(Severity::Warn),
            NoBareVecConfig {
                forbidden_types:   Vec::new(),
                positions:         Vec::new(),
                visibility:        Visibility::Any,
                macro_body_tokens: vec!["vec !".to_string()],
                macros:            vec!["define_resource".to_string()],
            },
        );
        let doc = MockspaceDocument::new(
            "a.rs",
            "t",
            Language::Rust,
            "define_resource! { initial = vec![1, 2, 3] }",
        );
        let sink = VecFindingSink::new();
        let root = PathBuf::from("/tmp");
        let sev = GateSeverity::uniform(Severity::Warn);
        let cfg = EmptyCfg;
        let ctx = make_ctx(&root, sev, &cfg);
        lint.check_document(&ctx, &doc, &sink).unwrap();
        let findings = sink.into_findings();
        assert!(!findings.is_empty());
    }
}
