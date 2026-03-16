//! Lint: enforce a configurable LOC limit per source file.
//!
//! Counts non-blank, non-comment lines in source. Block-comment interiors
//! (lines starting with `*`) are treated as comments. If the count exceeds
//! the configured limit, a single error is emitted at line 1.
//!
//! Configuration in `mockspace.toml`:
//! ```toml
//! [lints.file-size]
//! commit = "warn"
//! build = "error"
//! push = "error"
//! max_lines = "300"
//! exempt = "storage"
//! ```
//!
//! Default limit: 500. Default exempt suffixes: none.

use std::collections::HashMap;

use crate::{Lint, LintContext, LintError, Severity};

pub struct FileSize {
    max_lines: usize,
    exempt_suffixes: Vec<String>,
}

impl Default for FileSize {
    fn default() -> Self {
        Self {
            max_lines: 500,
            exempt_suffixes: Vec::new(),
        }
    }
}

impl FileSize {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Lint for FileSize {
    fn name(&self) -> &'static str {
        "file-size"
    }

    fn default_severity(&self) -> Severity {
        Severity::ADVISORY
    }

    fn config_keys(&self) -> &[&str] {
        &["max_lines", "exempt"]
    }

    fn configure(&mut self, params: &HashMap<String, String>) {
        if let Some(val) = params.get("max_lines") {
            if let Ok(n) = val.parse::<usize>() {
                self.max_lines = n;
            }
        }
        if let Some(val) = params.get("exempt") {
            self.exempt_suffixes = val.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }

    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let is_exempt = self.exempt_suffixes.iter().any(|s| {
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
                continue;
            }

            count += 1;
        }

        if count > self.max_lines {
            vec![LintError {
                crate_name: ctx.crate_name.to_string(),
                line: 1,
                lint_name: "file-size",
                severity: self.default_severity(),
                message: format!(
                    "file has {count} non-blank, non-comment lines (limit: {}). Split into modules.",
                    self.max_lines
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
        static EMPTY: &str = "";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(EMPTY, None).unwrap();
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
        assert!(FileSize::new().check(&ctx).is_empty());
    }

    #[test]
    fn at_default_limit_passes() {
        let src = "fn main() {}\n".repeat(500);
        let ctx = make_ctx(&src);
        assert!(FileSize::new().check(&ctx).is_empty());
    }

    #[test]
    fn over_default_limit_fails() {
        let src = "fn main() {}\n".repeat(501);
        let ctx = make_ctx(&src);
        let errors = FileSize::new().check(&ctx);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("501"));
    }

    #[test]
    fn configurable_limit() {
        let src = "fn main() {}\n".repeat(301);
        let ctx = make_ctx(&src);

        let mut lint = FileSize::new();
        let mut params = HashMap::new();
        params.insert("max_lines".to_string(), "300".to_string());
        lint.configure(&params);

        let errors = lint.check(&ctx);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("301"));
        assert!(errors[0].message.contains("limit: 300"));
    }

    #[test]
    fn blanks_and_comments_not_counted() {
        let mut lines = Vec::new();
        for _ in 0..300 { lines.push("let x = 1;"); }
        for _ in 0..300 { lines.push(""); }
        for _ in 0..300 { lines.push("// comment"); }
        let src = lines.join("\n");
        let ctx = make_ctx(&src);
        assert!(FileSize::new().check(&ctx).is_empty());
    }

    #[test]
    fn block_comments_not_counted() {
        let mut lines = Vec::new();
        lines.push("/*");
        for _ in 0..600 { lines.push(" * comment"); }
        lines.push(" */");
        lines.push("fn main() {}");
        let src = lines.join("\n");
        let ctx = make_ctx(&src);
        assert!(FileSize::new().check(&ctx).is_empty());
    }
}
