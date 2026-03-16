//! Lint: enforce the 500 LOC hard limit per file.
//!
//! Counts non-blank, non-comment lines in source. Block-comment interiors
//! (lines starting with `*`) are treated as comments. If the count exceeds
//! 500, a single error is emitted at line 1.

use crate::{Lint, LintContext, LintError};

/// Crate suffixes with genuinely large API surfaces.
/// NOTE: primitives was split in tier 0 round; storage will be split in
/// tier 1 round. Remove exemptions as crates are brought into compliance.
/// Full crate names are built as `<prefix>-<suffix>` at check time.
const EXEMPT_SUFFIXES: &[&str] = &["storage"];

pub struct FileSize;

impl Lint for FileSize {
    fn name(&self) -> &'static str {
        "file-size"
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let is_exempt = EXEMPT_SUFFIXES.iter().any(|s| {
            let full = format!("{}-{}", ctx.crate_prefix, s);
            ctx.crate_name == full
        });
        if is_exempt {
            return Vec::new();
        }

        let mut in_block_comment = false;
        let mut count: usize = 0;

        for line in ctx.source.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            // Track block comment state
            if in_block_comment {
                if trimmed.contains("*/") {
                    in_block_comment = false;
                }
                continue;
            }

            if trimmed.starts_with("//") {
                continue;
            }

            if trimmed.starts_with("/*") {
                if !trimmed.contains("*/") {
                    in_block_comment = true;
                }
                continue;
            }

            if trimmed.starts_with("*") {
                // Stray block-comment continuation (e.g. after `/**`)
                continue;
            }

            count += 1;
        }

        if count > 500 {
            vec![LintError {
                crate_name: ctx.crate_name.to_string(),
                line: 1,
                lint_name: "file-size",
                        severity: crate::Severity::HARD_ERROR,
                message: format!(
                    "file has {count} non-blank, non-comment lines (limit: 500)"
                ),
                finding_kind: None,
            }]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn make_ctx(source: &str) -> LintContext {
        // tree-sitter is not needed for this lint; pass a minimal tree
        // by leaking a parser result. In the test harness we only care
        // about `source`.
        static EMPTY: &str = "";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(EMPTY, None).unwrap();
        // SAFETY: we leak the tree so the reference lives long enough for tests.
        let tree: &'static tree_sitter::Tree = Box::leak(Box::new(tree));

        LintContext {
            crate_name: "test-crate",
            short_name: "test-crate",
            source,
            tree,
            deps: &[],
            all_crates: Box::leak(Box::new(BTreeSet::new())),
            design_doc: None,
            all_doc_content: "",
            shame_doc: None,
            workspace_root: std::path::Path::new("/tmp"),
            proc_macro_crates: &[],
            crate_prefix: "test",
        }
    }

    #[test]
    fn under_limit_passes() {
        let src = "fn main() {}\n".repeat(100);
        let ctx = make_ctx(&src);
        assert!(FileSize.check(&ctx).is_empty());
    }

    #[test]
    fn at_limit_passes() {
        let src = "fn main() {}\n".repeat(500);
        let ctx = make_ctx(&src);
        assert!(FileSize.check(&ctx).is_empty());
    }

    #[test]
    fn over_limit_fails() {
        let src = "fn main() {}\n".repeat(501);
        let ctx = make_ctx(&src);
        let errors = FileSize.check(&ctx);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].lint_name, "file-size");
        assert!(errors[0].message.contains("501"));
    }

    #[test]
    fn blanks_and_comments_not_counted() {
        let mut lines = Vec::new();
        // 300 code lines
        for _ in 0..300 {
            lines.push("let x = 1;");
        }
        // 300 blank lines
        for _ in 0..300 {
            lines.push("");
        }
        // 300 comment lines
        for _ in 0..300 {
            lines.push("// this is a comment");
        }
        let src = lines.join("\n");
        let ctx = make_ctx(&src);
        assert!(FileSize.check(&ctx).is_empty());
    }

    #[test]
    fn block_comments_not_counted() {
        let mut lines = Vec::new();
        lines.push("/*");
        for _ in 0..600 {
            lines.push(" * comment line");
        }
        lines.push(" */");
        lines.push("fn main() {}");
        let src = lines.join("\n");
        let ctx = make_ctx(&src);
        assert!(FileSize.check(&ctx).is_empty());
    }
}
