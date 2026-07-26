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

use crate::{CrateSourceFile, Lint, LintContext, LintError, Severity};

pub struct FileSize {
    max_lines:       usize,
    exempt_suffixes: Vec<String>,
}

impl Default for FileSize {
    fn default() -> Self {
        Self {
            max_lines:       500,
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
            self.exempt_suffixes = val
                .split(',')
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

        // Every file under `src/`, not just the crate root. A crate root
        // declares modules and the code lives in siblings, so measuring
        // `ctx.source` alone measures the smallest file in the crate and
        // reports clean however large the rest are.
        //
        // `all_sources` falls back to the crate root when empty, so a caller
        // that builds a context without it still gets the old behaviour rather
        // than silently getting no coverage at all.
        let mut errors = Vec::new();
        if ctx.all_sources.is_empty() {
            if let Some(err) = self.measure(ctx, "src/lib.rs", ctx.source) {
                errors.push(err);
            }
            return errors;
        }
        for file in ctx.all_sources {
            let path = file.rel_path.to_string_lossy();
            if let Some(err) = self.measure(ctx, &path, &file.text) {
                errors.push(err);
            }
        }
        errors
    }
}

impl FileSize {
    /// Count the non-blank, non-comment lines of one file and report it when it
    /// is over the limit.
    fn measure(&self, ctx: &LintContext, path: &str, text: &str) -> Option<LintError> {
        let mut in_block_comment = false;
        let mut count: usize = 0;

        for line in text.lines() {
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
            Some(LintError {
                crate_name:   ctx.crate_name.to_string(),
                line:         1,
                lint_name:    "file-size",
                severity:     self.default_severity(),
                message:      format!(
                    "{path} has {count} non-blank, non-comment lines (limit: {}). \
                     Split into modules.",
                    self.max_lines
                ),
                finding_kind: None,
            })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn make_ctx(source: &str) -> LintContext {
        make_ctx_with(source, &[])
    }

    fn make_ctx_with<'a>(source: &'a str, all: &'a [CrateSourceFile]) -> LintContext<'a> {
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
            all_sources: all,
            deps: &[],
            all_crates: Box::leak(Box::new(BTreeSet::new())),
            design_doc: None,
            all_doc_content: "",
            shame_doc: None,
            workspace_root: std::path::Path::new("/tmp"),
            proc_macro_crates: &[],
            crate_prefix: "test",
            lint_proc_macro_source: false,
            primitive_introductions: Box::leak(Box::new(std::collections::BTreeMap::new())),
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
        for _ in 0 .. 300 {
            lines.push("let x = 1;");
        }
        for _ in 0 .. 300 {
            lines.push("");
        }
        for _ in 0 .. 300 {
            lines.push("// comment");
        }
        let src = lines.join("\n");
        let ctx = make_ctx(&src);
        assert!(FileSize::new().check(&ctx).is_empty());
    }

    #[test]
    fn block_comments_not_counted() {
        let mut lines = Vec::new();
        lines.push("/*");
        for _ in 0 .. 600 {
            lines.push(" * comment");
        }
        lines.push(" */");
        lines.push("fn main() {}");
        let src = lines.join("\n");
        let ctx = make_ctx(&src);
        assert!(FileSize::new().check(&ctx).is_empty());
    }
}

#[cfg(test)]
mod module_file_tests {
    use std::collections::BTreeSet;

    use super::*;

    /// A crate whose root is small but whose module file is far over the limit.
    ///
    /// This is the shape every real crate has: `lib.rs` declares modules and the
    /// code lives in siblings. arvo's `arvo-strategy` is exactly it, with a
    /// 108-line `lib.rs` beside an 843-line `arith.rs`, and the lint reported
    /// the crate clean at `max_lines = 500` set to error on every gate.
    ///
    /// The existing tests could not have caught this: every one of them builds a
    /// context with `all_sources: &[]`, so the only file that has ever been
    /// measured is the crate root.
    #[test]
    fn a_module_file_over_the_limit_is_reported() {
        let big: String = (0 .. 600)
            .map(|i| format!("pub fn f{i}() {{}}\n"))
            .collect();
        let all = vec![
            CrateSourceFile {
                rel_path: std::path::PathBuf::from("src/lib.rs"),
                text:     "pub mod huge;\n".to_string(),
            },
            CrateSourceFile {
                rel_path: std::path::PathBuf::from("src/huge.rs"),
                text:     big,
            },
        ];
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse("", None).unwrap();
        let tree: &'static tree_sitter::Tree = Box::leak(Box::new(tree));
        let ctx = LintContext {
            crate_name: "test-crate",
            short_name: "test-crate",
            source: "pub mod huge;\n",
            tree,
            all_sources: &all,
            deps: &[],
            all_crates: Box::leak(Box::new(BTreeSet::new())),
            design_doc: None,
            all_doc_content: "",
            shame_doc: None,
            workspace_root: std::path::Path::new("/tmp"),
            proc_macro_crates: &[],
            crate_prefix: "test",
            lint_proc_macro_source: false,
            primitive_introductions: Box::leak(Box::new(std::collections::BTreeMap::new())),
        };

        let errs = FileSize::new().check(&ctx);
        assert_eq!(
            errs.len(),
            1,
            "a 600-line module file must be reported; only the crate root is being measured",
        );
        assert!(
            errs[0].message.contains("huge.rs"),
            "the report must name the offending file, got: {}",
            errs[0].message,
        );
    }
}
