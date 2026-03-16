//! Pluggable AST lint system.
//!
//! Lint rules live in the `mockspace-lint-rules` crate. This module
//! re-exports and runs them against the mock workspace's CrateMap.
//!
//! Two passes:
//! 1. Per-crate lints — each lint sees one crate at a time.
//! 2. Cross-crate lints — each lint sees all crates simultaneously.

use std::collections::BTreeSet;
use std::path::Path;

use mockspace_lint_rules::{self, LintContext, LintError, LintMode, Level, Lint, CrossCrateLint, LintConfig};

use crate::model::CrateMap;

/// Collected data for a single crate, kept alive for cross-crate pass.
struct ParsedCrate {
    crate_name: String,
    short_name: String,
    source: String,
    tree: tree_sitter::Tree,
    design_doc: Option<String>,
    all_doc_content: String,
    shame_doc: Option<String>,
}

/// Run all lints (per-crate + cross-crate) against crates.
///
/// The `mode` determines which gate is active. Each lint violation declares
/// its own per-gate severity; the mode selects the effective level:
///   - `Commit` — most permissive (pre-commit hook)
///   - `Build`  — middle strictness (default xtask run)
///   - `Push`   — most strict (pre-push hook)
///
/// When `scope` is `Some`, only crates in the list are linted. This is used
/// by the pre-commit hook to lint only crates with staged files. When `None`,
/// all crates are linted (build/push mode).
///
/// When `doc_only` is true, source-only lints are skipped. This allows
/// doc-only commits during DOC-EXEC phase without being blocked by
/// pre-existing source issues that will be fixed in SRC-EXEC.
///
/// When `lint_overrides` is non-empty, lint severities are overridden per
/// the `[lints]` section of mockspace.toml.
///
/// Optional `custom_lints` and `custom_cross_lints` are appended to the
/// built-in lint lists (for consumer-side custom lints).
///
/// Returns the count of blocking violations (effective level = Error).
pub fn run_lints(
    crates: &CrateMap,
    crates_dir: &Path,
    mode: LintMode,
    scope: Option<&[String]>,
    doc_only: bool,
    proc_macro_crates: &[String],
    crate_prefix: &str,
    lint_overrides: &LintConfig,
    custom_lints: &[Box<dyn Lint>],
    custom_cross_lints: &[Box<dyn CrossCrateLint>],
) -> usize {
    let all_crate_names: BTreeSet<String> = crates.keys().cloned().collect();
    let workspace_root = crates_dir
        .parent()
        .unwrap_or(crates_dir);

    let mut parser = mockspace_lint_rules::make_parser();
    let mut all_errors: Vec<LintError> = Vec::new();

    let mut parsed: Vec<ParsedCrate> = Vec::new();

    let overrides = if lint_overrides.is_empty() {
        None
    } else {
        Some(lint_overrides)
    };

    for (crate_name, info) in crates {
        // Skip crates not in scope (when scoped)
        if let Some(names) = scope {
            if !names.iter().any(|n| n == crate_name) {
                continue;
            }
        }

        let crate_dir = crates_dir.join(crate_name);
        let librs = crate_dir.join("src/lib.rs");
        let source = match std::fs::read_to_string(&librs) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => continue,
        };

        // Read DESIGN.md.tmpl if it exists
        let design_doc = std::fs::read_to_string(crate_dir.join("DESIGN.md.tmpl")).ok();

        // Read ALL doc templates and concatenate
        let all_doc_content = collect_all_docs(&crate_dir);

        // Read SHAME.md.tmpl if it exists
        let shame_doc = std::fs::read_to_string(crate_dir.join("SHAME.md.tmpl")).ok();

        // Per-crate lint pass
        let ctx = LintContext {
            crate_name,
            short_name: &info.short_name,
            source: &source,
            tree: &tree,
            deps: &info.deps,
            all_crates: &all_crate_names,
            design_doc: design_doc.as_deref(),
            all_doc_content: &all_doc_content,
            shame_doc: shame_doc.as_deref(),
            workspace_root,
            proc_macro_crates,
            crate_prefix,
        };

        all_errors.extend(mockspace_lint_rules::check_crate_with_extra(&ctx, doc_only, overrides, custom_lints));

        parsed.push(ParsedCrate {
            crate_name: crate_name.clone(),
            short_name: info.short_name.clone(),
            source,
            tree,
            design_doc,
            all_doc_content,
            shame_doc,
        });
    }

    // Cross-crate lint pass
    let cross_crate_contexts: Vec<(&str, LintContext)> = parsed
        .iter()
        .map(|p| {
            let info = &crates[p.crate_name.as_str()];
            (
                p.crate_name.as_str(),
                LintContext {
                    crate_name: &p.crate_name,
                    short_name: &p.short_name,
                    source: &p.source,
                    tree: &p.tree,
                    deps: &info.deps,
                    all_crates: &all_crate_names,
                    design_doc: p.design_doc.as_deref(),
                    all_doc_content: &p.all_doc_content,
                    shame_doc: p.shame_doc.as_deref(),
                    workspace_root,
                    proc_macro_crates,
                    crate_prefix,
                },
            )
        })
        .collect();

    let cross_refs: Vec<(&str, &LintContext)> = cross_crate_contexts
        .iter()
        .map(|(name, ctx)| (*name, ctx))
        .collect();

    all_errors.extend(mockspace_lint_rules::check_cross_crate_with_extra(&cross_refs, doc_only, overrides, custom_cross_lints));

    // Partition by effective severity
    let mut info = Vec::new();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for e in all_errors {
        match e.severity.effective(mode) {
            Level::Pass => {} // skip silently
            Level::Info => info.push(e),
            Level::Warn => warnings.push(e),
            Level::Error => errors.push(e),
        }
    }

    if !info.is_empty() {
        eprintln!("\n--- lint info ({}) ---", info.len());
        for i in &info {
            eprintln!("{i}");
        }
    }

    if !warnings.is_empty() {
        eprintln!("\n--- lint warnings ({}) ---", warnings.len());
        for w in &warnings {
            eprintln!("{w}");
        }
    }

    if !errors.is_empty() {
        eprintln!("\n--- lint errors ({}) ---", errors.len());
        for err in &errors {
            eprintln!("{err}");
        }
        eprintln!();
    }

    // Only errors block; info and warnings are informational
    errors.len()
}

/// Collect and concatenate all `.md.tmpl` doc files in a crate directory.
fn collect_all_docs(crate_dir: &Path) -> String {
    let mut content = String::new();

    let candidates = ["README.md.tmpl", "DESIGN.md.tmpl"];
    for name in &candidates {
        if let Ok(text) = std::fs::read_to_string(crate_dir.join(name)) {
            content.push_str(&text);
            content.push('\n');
        }
    }

    // DEEPDIVE_*.md.tmpl files
    if let Ok(entries) = std::fs::read_dir(crate_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("DEEPDIVE_") && name.ends_with(".md.tmpl") {
                if let Ok(text) = std::fs::read_to_string(entry.path()) {
                    content.push_str(&text);
                    content.push('\n');
                }
            }
        }
    }

    content
}
