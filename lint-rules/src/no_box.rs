//! Lint: `no-box`
//!
//! Forbids `Box` usage in framework code.
//!
//! - `Box<dyn _>` (trait objects): **Error**. Use framework type erasure
//!   (ErasedColumn, ErasedArena) or enum dispatch instead.
//! - `Box<ConcreteType>` (recursive types, owned slices): **Warning**.
//!   Acceptable for recursive enum variants and `Box<[u8]>`, but flagged
//!   for review.
//!
//! Suppression: `// lint:allow(no_box)` on the line or enclosing item.
//!
//! Exempt crates: proc macro crates (internal parsing), the buffers crate
//! (defines framework type erasure primitives), the ID crate (defines
//! collection wrappers).

use crate::{Lint, LintContext, LintError};

const LINT_NAME: &str = "no-box";

pub struct NoBox;

impl Lint for NoBox {
        fn default_severity(&self) -> crate::Severity { crate::Severity::OFF }
fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.is_proc_macro_crate() {
            return Vec::new();
        }

        let mut errors = Vec::new();
        let mut in_cfg_test = false;
        let mut brace_depth: usize = 0;
        let mut cfg_test_depth: Option<usize> = None;

        for (line_idx, line) in ctx.source.lines().enumerate() {
            let line_num = line_idx + 1;
            let trimmed = line.trim();

            // track brace depth for #[cfg(test)] blocks
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        if brace_depth > 0 {
                            brace_depth -= 1;
                        }
                        if let Some(depth) = cfg_test_depth {
                            if brace_depth < depth {
                                cfg_test_depth = None;
                                in_cfg_test = false;
                            }
                        }
                    }
                    _ => {}
                }
            }

            // detect #[cfg(test)]
            if trimmed.contains("#[cfg(test)]") {
                cfg_test_depth = Some(brace_depth);
                in_cfg_test = true;
                continue;
            }

            if in_cfg_test {
                continue;
            }

            // lint:allow(no_box) suppresses the violation but emits a warning
            if line.contains("lint:allow(no_box)") {
                errors.push(LintError::warning(
                    ctx.crate_name.to_string(),
                    line_num,
                    LINT_NAME,
                    "suppressed by lint:allow(no_box) — review periodically".to_string(),
                ));
                continue;
            }

            // skip comments and doc comments
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }

            // detect Box< usage
            if !contains_box(trimmed) {
                continue;
            }

            // classify: Box<dyn _> vs Box<ConcreteType>
            if contains_box_dyn(trimmed) {
                errors.push(LintError::error(
                    ctx.crate_name.to_string(),
                    line_num,
                    LINT_NAME,
                    format!(
                        "Box<dyn _> is forbidden — use framework type erasure \
                         (ErasedColumn, ErasedArena, enum dispatch) instead of \
                         trait objects behind Box"
                    ),
                ));
            } else {
                errors.push(LintError::warning(
                    ctx.crate_name.to_string(),
                    line_num,
                    LINT_NAME,
                    "Box<T> usage — acceptable for recursive types, flag for review"
                        .to_string(),
                ));
            }
        }

        errors
    }
}

/// Check if a line contains `Box<` (case-sensitive, whole-word).
fn contains_box(line: &str) -> bool {
    let mut search = line;
    while let Some(pos) = search.find("Box<") {
        // check it's not part of a larger identifier
        if pos > 0 {
            let prev = search.as_bytes()[pos - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                search = &search[pos + 4..];
                continue;
            }
        }
        return true;
    }
    false
}

/// Check if a line contains `Box<dyn ` (trait object pattern).
fn contains_box_dyn(line: &str) -> bool {
    let mut search = line;
    while let Some(pos) = search.find("Box<dyn ") {
        if pos > 0 {
            let prev = search.as_bytes()[pos - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                search = &search[pos + 8..];
                continue;
            }
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn make_ctx(source: &str) -> (tree_sitter::Tree, LintContext<'static>) {
        let mut parser = crate::make_parser();
        let tree = parser.parse(source, None).unwrap();
        let tree = Box::leak(Box::new(tree));
        let ctx = LintContext {
            crate_name: "test-crate",
            short_name: "test",
            source: Box::leak(source.to_string().into_boxed_str()),
            tree,
            all_sources: Box::leak(Box::new(Vec::new())),
            deps: Box::leak(Box::new(Vec::new())),
            all_crates: Box::leak(Box::new(BTreeSet::new())),
            design_doc: None,
            all_doc_content: Box::leak("".to_string().into_boxed_str()),
            shame_doc: None,
            workspace_root: Box::leak(Box::new(std::path::PathBuf::from("/tmp"))),
            proc_macro_crates: Box::leak(Box::new(Vec::new())),
            crate_prefix: "test",
            lint_proc_macro_source: false,
            primitive_introductions: Box::leak(Box::new(std::collections::BTreeMap::new())),
        };
        // return tree separately to keep borrow checker happy
        (unsafe { std::ptr::read(tree as *const _) }, ctx)
    }

    #[test]
    fn clean_code_passes() {
        let (_tree, ctx) = make_ctx("pub struct Foo { x: u32 }");
        assert!(NoBox.check(&ctx).is_empty());
    }

    #[test]
    fn box_dyn_is_error() {
        let (_tree, ctx) = make_ctx(
            "pub struct Pipeline {\n    passes: Vec<Box<dyn Pass>>,\n}\n",
        );
        let errors = NoBox.check(&ctx);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.severity == crate::Severity::HARD_ERROR));
    }

    #[test]
    fn box_concrete_is_warning() {
        let (_tree, ctx) = make_ctx(
            "pub enum Tree {\n    Leaf(u32),\n    Node(Box<Tree>),\n}\n",
        );
        let errors = NoBox.check(&ctx);
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|e| e.severity == crate::Severity::ADVISORY));
    }

    #[test]
    fn lint_allow_emits_warning() {
        let (_tree, ctx) = make_ctx(
            "pub struct Pool {\n    entries: Box<dyn Any>, // lint:allow(no_box)\n}\n",
        );
        let errors = NoBox.check(&ctx);
        // Allow suppresses the error but emits a warning for transparency
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|e| e.severity == crate::Severity::ADVISORY));
    }

    #[test]
    fn proc_macro_crate_exempt() {
        let mut parser = crate::make_parser();
        let source = "pub struct E { col: Box<dyn Any> }";
        let tree = parser.parse(source, None).unwrap();
        let tree = Box::leak(Box::new(tree));
        let proc_macro_crates: &'static Vec<String> = Box::leak(Box::new(vec!["test-dsl".to_string()]));
        let ctx = LintContext {
            crate_name: "test-dsl",
            short_name: "dsl",
            source: Box::leak(source.to_string().into_boxed_str()),
            tree,
            all_sources: Box::leak(Box::new(Vec::new())),
            deps: Box::leak(Box::new(Vec::new())),
            all_crates: Box::leak(Box::new(BTreeSet::new())),
            design_doc: None,
            all_doc_content: Box::leak("".to_string().into_boxed_str()),
            shame_doc: None,
            workspace_root: Box::leak(Box::new(std::path::PathBuf::from("/tmp"))),
            proc_macro_crates,
            crate_prefix: "test",
            lint_proc_macro_source: false,
            primitive_introductions: Box::leak(Box::new(std::collections::BTreeMap::new())),
        };
        assert!(NoBox.check(&ctx).is_empty());
    }

    #[test]
    fn non_proc_macro_crate_not_exempt() {
        let mut parser = crate::make_parser();
        let source = "pub struct E { col: Box<dyn Any> }";
        let tree = parser.parse(source, None).unwrap();
        let tree = Box::leak(Box::new(tree));
        let ctx = LintContext {
            crate_name: "test-buffers",
            short_name: "buffers",
            source: Box::leak(source.to_string().into_boxed_str()),
            tree,
            all_sources: Box::leak(Box::new(Vec::new())),
            deps: Box::leak(Box::new(Vec::new())),
            all_crates: Box::leak(Box::new(BTreeSet::new())),
            design_doc: None,
            all_doc_content: Box::leak("".to_string().into_boxed_str()),
            shame_doc: None,
            workspace_root: Box::leak(Box::new(std::path::PathBuf::from("/tmp"))),
            proc_macro_crates: Box::leak(Box::new(Vec::new())),
            crate_prefix: "test",
            lint_proc_macro_source: false,
            primitive_introductions: Box::leak(Box::new(std::collections::BTreeMap::new())),
        };
        let errors = NoBox.check(&ctx);
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.severity == crate::Severity::HARD_ERROR));
    }

    #[test]
    fn cfg_test_is_exempt() {
        let (_tree, ctx) = make_ctx(
            "pub fn clean() {}\n#[cfg(test)]\nmod tests {\n    fn t() -> Box<dyn Trait> { todo!() }\n}\n",
        );
        assert!(NoBox.check(&ctx).is_empty());
    }

    #[test]
    fn comment_lines_skipped() {
        let (_tree, ctx) = make_ctx(
            "// Box<dyn Foo> is mentioned in this comment\n/// Box<dyn Bar> in doc comment\npub fn clean() {}\n",
        );
        assert!(NoBox.check(&ctx).is_empty());
    }
}
