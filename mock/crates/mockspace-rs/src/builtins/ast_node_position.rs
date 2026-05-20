//! AstNodePositionMatch primitive.
//!
//! Per schema design memo §4.2. Tree-sitter-driven walk over node-kind
//! sets with name and exclusion filters. Examples: `no-todo`
//! (`macro_invocation` with name `todo!`/`unimplemented!`/`panic!`,
//! excluding subtrees under `macro_definition`), `no-runtime-spawn`
//! (`call_expression` matching `thread::spawn` etc), `no-dyn-dispatch`
//! (`dyn_type` nodes).

use std::borrow::Cow;

use mockspace_core::lint::{Finding, GateSeverity, LintContext, Severity, Span};
use serde::Deserialize;

use crate::document::MockspaceDocument;
use crate::errors::{ConfigError, ConfigErrorKind, LintError};
use crate::finding_sink::FindingSink;
use crate::lint::Lint;

pub const KIND: &str = "ast-node-position-match";

/// Closed set of tree-sitter node kinds the engine walks. Adding a new
/// kind is a code edit in mockspace-rs (and the matching kind-string
/// resolver in `node_kind_name`). This is intentional: tree-sitter
/// node-kind strings are grammar-version-sensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TsNodeKind {
    MacroInvocation,
    MacroDefinition,
    EnumItem,
    StructItem,
    ImplItem,
    FunctionItem,
    CallExpression,
    FieldExpression,
    UseDeclaration,
    AttributeItem,
}

impl TsNodeKind {
    fn matches(self, kind: &str) -> bool {
        match self {
            Self::MacroInvocation => kind == "macro_invocation",
            Self::MacroDefinition => kind == "macro_definition",
            Self::EnumItem => kind == "enum_item",
            Self::StructItem => kind == "struct_item",
            Self::ImplItem => kind == "impl_item",
            Self::FunctionItem => kind == "function_item",
            Self::CallExpression => kind == "call_expression",
            Self::FieldExpression => kind == "field_expression",
            Self::UseDeclaration => kind == "use_declaration",
            Self::AttributeItem => kind == "attribute_item",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct AstNodePositionConfig {
    pub node_kinds: Vec<TsNodeKind>,

    /// For `macro_invocation` nodes, the macro names to fire on
    /// (e.g. `["todo", "unimplemented", "panic"]`).
    #[serde(default)]
    pub macro_names: Vec<String>,

    /// For `impl_item` nodes, the trait names to fire on.
    #[serde(default)]
    pub trait_names: Vec<String>,

    /// For `call_expression` / `field_expression` nodes, the function /
    /// method / field names to fire on.
    #[serde(default)]
    pub member_names: Vec<String>,

    /// Ancestor node kinds whose subtrees are skipped (e.g.
    /// `MacroDefinition` to allow `todo!` inside macro definitions but
    /// not at use sites).
    #[serde(default)]
    pub exclude_under: Vec<TsNodeKind>,
}

pub struct AstNodePositionMatchLint {
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: AstNodePositionConfig,
}

impl AstNodePositionMatchLint {
    pub fn new(
        name: &'static str,
        description: &'static str,
        default_severity: GateSeverity,
        config: AstNodePositionConfig,
    ) -> Self {
        Self {
            name,
            description,
            default_severity,
            config,
        }
    }
}

impl Lint for AstNodePositionMatchLint {
    fn name(&self) -> &'static str {
        self.name
    }
    fn description(&self) -> &'static str {
        self.description
    }
    fn default_severity(&self) -> GateSeverity {
        self.default_severity
    }
    fn needs_tree_sitter(&self) -> bool {
        true
    }

    fn check_document(
        &self,
        ctx: &LintContext<'_>,
        doc: &MockspaceDocument,
        sink: &dyn FindingSink,
    ) -> Result<(), LintError> {
        let Some(tree) = doc.tree_sitter() else {
            return Ok(());
        };
        let source = doc.source();
        let active = ctx.active_severity();
        let mut cursor = tree.walk();
        let root = tree.root_node();
        walk_node(
            root,
            &mut cursor,
            source,
            self,
            doc.path(),
            active,
            sink,
            false,
        );
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn walk_node(
    node: tree_sitter::Node,
    cursor: &mut tree_sitter::TreeCursor,
    source: &str,
    lint: &AstNodePositionMatchLint,
    path: &std::path::Path,
    severity: Severity,
    sink: &dyn FindingSink,
    under_excluded: bool,
) {
    let kind = node.kind();

    // Update exclusion state for descendants.
    let now_under_excluded = under_excluded
        || lint
            .config
            .exclude_under
            .iter()
            .any(|k| k.matches(kind));

    // Match check.
    if !under_excluded
        && lint.config.node_kinds.iter().any(|k| k.matches(kind))
        && node_matches_filters(node, source, &lint.config)
    {
        let start = node.start_position();
        let length = node.end_byte().saturating_sub(node.start_byte()) as u32;
        sink.emit(Finding {
            lint_name: Cow::Borrowed(lint.name),
            rule_id: None,
            plugin_id: None,
            severity,
            impact: None,
            category: None,
            message: Cow::Owned(format!(
                "forbidden {} construct",
                kind.replace('_', " ")
            )),
            span: Span::single_line(
                path,
                (start.row + 1) as u32,
                (start.column + 1) as u32,
                length.max(1),
            ),
            fix_suggestion: None,
            related_spans: Vec::new(),
            metadata: None,
        });
    }

    // Recurse.
    let mut child_cursor = node.walk();
    for child in node.children(&mut child_cursor) {
        walk_node(
            child,
            cursor,
            source,
            lint,
            path,
            severity,
            sink,
            now_under_excluded,
        );
    }
}

fn node_matches_filters(
    node: tree_sitter::Node,
    source: &str,
    config: &AstNodePositionConfig,
) -> bool {
    let kind = node.kind();

    // macro_invocation: filter by macro name. Tree-sitter rust grammar
    // names the macro via the `macro` field on macro_invocation nodes.
    if kind == "macro_invocation" && !config.macro_names.is_empty() {
        let Some(macro_node) = node.child_by_field_name("macro") else {
            return false;
        };
        let name = node_text(macro_node, source);
        return config.macro_names.iter().any(|n| name == *n);
    }

    // impl_item: filter by trait name (the trait the impl is for).
    if kind == "impl_item" && !config.trait_names.is_empty() {
        let Some(trait_node) = node.child_by_field_name("trait") else {
            return false;
        };
        let name = node_text(trait_node, source);
        return config.trait_names.iter().any(|n| name.contains(n));
    }

    // call_expression: filter by function name. The grammar exposes the
    // callee via the `function` field.
    if kind == "call_expression" && !config.member_names.is_empty() {
        let Some(callee) = node.child_by_field_name("function") else {
            return false;
        };
        let name = node_text(callee, source);
        return config.member_names.iter().any(|n| name.contains(n));
    }

    // field_expression: filter by field name.
    if kind == "field_expression" && !config.member_names.is_empty() {
        let Some(field) = node.child_by_field_name("field") else {
            return false;
        };
        let name = node_text(field, source);
        return config.member_names.iter().any(|n| name == *n);
    }

    // No name filter applies; the kind match alone counts as a hit.
    true
}

fn node_text<'a>(node: tree_sitter::Node, source: &'a str) -> &'a str {
    let bytes = source.as_bytes();
    let start = node.start_byte();
    let end = node.end_byte().min(bytes.len());
    if start >= end {
        return "";
    }
    std::str::from_utf8(&bytes[start..end]).unwrap_or("")
}

pub fn instantiate_with(
    name: &'static str,
    description: &'static str,
    default_severity: GateSeverity,
    config: &toml::Table,
    _scope: &toml::Table,
) -> Result<Box<dyn Lint>, ConfigError> {
    let parsed: AstNodePositionConfig =
        config
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError {
                lint_name: name.to_string(),
                field_path: String::new(),
                kind: ConfigErrorKind::InvalidValue,
                message: format!("ast-node-position-match config: {e}"),
                source_location: None,
            })?;
    Ok(Box::new(AstNodePositionMatchLint::new(
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

    fn run(source: &str, config: AstNodePositionConfig) -> Vec<mockspace_core::lint::Finding> {
        let lint = AstNodePositionMatchLint::new(
            "test",
            "",
            GateSeverity::uniform(Severity::Warn),
            config,
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
    fn fires_on_todo_macro() {
        let findings = run(
            "fn x() { todo!() }",
            AstNodePositionConfig {
                node_kinds: vec![TsNodeKind::MacroInvocation],
                macro_names: vec!["todo".to_string()],
                trait_names: Vec::new(),
                member_names: Vec::new(),
                exclude_under: Vec::new(),
            },
        );
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn excludes_under_macro_definition() {
        // A `todo!` inside a macro_rules! body should be excluded.
        let source = r#"
            macro_rules! my_macro {
                () => { todo!() };
            }
            fn x() { todo!() }
        "#;
        let findings = run(
            source,
            AstNodePositionConfig {
                node_kinds: vec![TsNodeKind::MacroInvocation],
                macro_names: vec!["todo".to_string()],
                trait_names: Vec::new(),
                member_names: Vec::new(),
                exclude_under: vec![TsNodeKind::MacroDefinition],
            },
        );
        // Only the fn-body todo! fires; the one inside macro_rules! is excluded.
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn does_not_fire_on_unmatched_macro() {
        let findings = run(
            "fn x() { println!(\"hi\") }",
            AstNodePositionConfig {
                node_kinds: vec![TsNodeKind::MacroInvocation],
                macro_names: vec!["todo".to_string(), "unimplemented".to_string()],
                trait_names: Vec::new(),
                member_names: Vec::new(),
                exclude_under: Vec::new(),
            },
        );
        assert!(findings.is_empty());
    }
}
