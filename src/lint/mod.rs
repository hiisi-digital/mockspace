//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Pluggable AST lint system.
//!
//! Lint rules live in the `mockspace-lint-rules` crate. This module
//! re-exports and runs them against the mock workspace's CrateMap.
//!
//! Two passes:
//! 1. Per-crate lints: each lint sees one crate at a time.
//! 2. Cross-crate lints: each lint sees all crates simultaneously.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use mockspace_lint_rules::{
    self,
    CrateSourceFile,
    LintPack,
    RepoContext,
    Level,
    LintConfig,
    LintContext,
    LintError,
    LintMode,
};

use crate::model::CrateMap;

/// Collected data for a single crate, kept alive for cross-crate pass.
struct ParsedCrate {
    crate_name:      String,
    short_name:      String,
    source:          String,
    tree:            tree_sitter::Tree,
    all_sources:     Vec<CrateSourceFile>,
    design_doc:      Option<String>,
    all_doc_content: String,
    shame_doc:       Option<String>,
}

/// Walk `crate_dir/src/**/*.rs` and return every file's (rel_path, text),
/// with `src/lib.rs` first when it exists. Silently skips unreadable
/// files; errors surface via the lint pass seeing missing content.
fn collect_crate_sources(crate_dir: &Path) -> Vec<CrateSourceFile> {
    let src_dir = crate_dir.join("src");
    if !src_dir.is_dir() {
        return Vec::new();
    }
    let mut out: Vec<CrateSourceFile> = Vec::new();
    walk_rs(&src_dir, crate_dir, &mut out);
    // Sort so `src/lib.rs` lands first, then everything else in path order.
    out.sort_by(|a, b| {
        let a_is_lib = a.rel_path == PathBuf::from("src/lib.rs");
        let b_is_lib = b.rel_path == PathBuf::from("src/lib.rs");
        match (a_is_lib, b_is_lib) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.rel_path.cmp(&b.rel_path),
        }
    });
    out
}

fn walk_rs(dir: &Path, crate_dir: &Path, out: &mut Vec<CrateSourceFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, crate_dir, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel_path = path.strip_prefix(crate_dir).unwrap_or(&path).to_path_buf();
        out.push(CrateSourceFile {
            rel_path,
            text,
        });
    }
}

/// Run all lints (per-crate + cross-crate) against crates.
///
/// The `mode` determines which gate is active. Each lint violation declares
/// its own per-gate severity; the mode selects the effective level:
///   - `Commit`: most permissive (pre-commit hook)
///   - `Build`: middle strictness (default xtask run)
///   - `Push`: most strict (pre-push hook)
///
/// When `scope` is `Some`, only crates in the list are linted. This is used
/// by the pre-commit hook to lint only crates with staged files. When `None`,
/// all crates are linted (build/push mode).
///
/// When `doc_only` is true, source-only lints are skipped. This allows
/// doc-only commits during DOC phase without being blocked by
/// pre-existing source issues that will be fixed in IMPL.
///
/// When `lint_overrides` is non-empty, lint severities are overridden per
/// the `[lints]` section of mockspace.toml.
///
/// Optional `custom_lints` and `custom_cross_lints` are appended to the
/// built-in lint lists (for consumer-side custom lints).
///
/// Returns the count of blocking violations (effective level = Error).
#[must_use]
pub fn run_lints(
    crates: &CrateMap,
    // Every source directory, because a crate lives in exactly one of them and
    // linting against a single root would pass the gate having read a subset.
    src_dirs: &[std::path::PathBuf],
    // The mock directory. Passed rather than inferred: it used to be
    // `crates_dir.parent()`, which only held while there was one root directly
    // under the mock dir, and silently produced a different root otherwise.
    mock_dir: &Path,
    mode: LintMode,
    scope: Option<&[String]>,
    doc_only: bool,
    proc_macro_crates: &[String],
    lint_proc_macro_source: bool,
    crate_prefix: &str,
    lint_overrides: &LintConfig,
    // The registry, flattened, so a repo lint can check it. Built by the caller,
    // which is the only place the declared field types are in scope.
    registry: &mockspace_lint_rules::RegistryView,
    // What the project calls its canon, and which panels are open. Both are
    // computed by the caller: the globs are configuration and the panel ledger
    // lives in the engine, so a lint has no route to either.
    canon_paths: &[String],
    open_panels: &[String],
    primitive_introductions: &std::collections::BTreeMap<String, Vec<String>>,
    pack: &LintPack,
) -> usize {
    // A duplicated lint name is a configuration error, not a linting result:
    // every finding would double and `[lints.<name>]` would be ambiguous
    // between the registrations. Error and guide before running anything,
    // rather than partially enforcing.
    let duplicates = mockspace_lint_rules::duplicate_lint_names(pack);
    if !duplicates.is_empty() {
        for name in &duplicates {
            eprintln!(
                "  [lint-config] `{name}` is registered more than once (the builtin set \
                 plus an imported pack, or twice across packs). Rename or remove one \
                 registration; until then its findings would double and \
                 `[lints.{name}]` could not say which one it configures."
            );
        }
        return duplicates.len();
    }

    let all_crate_names: BTreeSet<String> = crates.keys().cloned().collect();
    let workspace_root = mock_dir;

    let mut parser = mockspace_lint_rules::make_parser();
    let mut all_errors: Vec<LintError> = Vec::new();

    let mut parsed: Vec<ParsedCrate> = Vec::new();

    let overrides = if lint_overrides.is_empty() { None } else { Some(lint_overrides) };

    for (crate_name, info) in crates {
        // Skip crates not in scope (when scoped)
        if let Some(names) = scope {
            if !names.iter().any(|n| n == crate_name) {
                continue;
            }
        }

        // The crate lives under exactly one source directory; find which.
        // Skipping when no root holds it matches the previous behaviour, where
        // a missing `src/lib.rs` was skipped a line later.
        let Some(crate_dir) = src_dirs.iter().map(|d| d.join(crate_name)).find(|d| d.is_dir())
        else {
            continue;
        };
        let librs = crate_dir.join("src/lib.rs");
        let source = match std::fs::read_to_string(&librs) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => continue,
        };

        // Read every .rs file under src/ so lints can scan module files
        // (bits.rs, prim.rs, impl.rs, ...) in addition to lib.rs.
        let all_sources = collect_crate_sources(&crate_dir);

        let design_doc = std::fs::read_to_string(crate_dir.join("DESIGN.md.tmpl")).ok();

        // Read ALL doc templates and concatenate
        let all_doc_content = collect_all_docs(&crate_dir);

        // Read SHAME.md.tmpl if it exists, matching the exact spelling the
        // phase gates match. See `read_shame_template`.
        let shame_doc = mockspace_lint_rules::read_shame_template(&crate_dir);

        // Per-crate lint pass
        let ctx = LintContext {
            crate_name,
            short_name: &info.short_name,
            source: &source,
            tree: &tree,
            all_sources: &all_sources,
            deps: &info.deps,
            all_crates: &all_crate_names,
            design_doc: design_doc.as_deref(),
            all_doc_content: &all_doc_content,
            shame_doc: shame_doc.as_deref(),
            workspace_root,
            proc_macro_crates,
            lint_proc_macro_source,
            crate_prefix,
            primitive_introductions,
        };

        all_errors.extend(mockspace_lint_rules::check_crate_with_extra(
            &ctx,
            doc_only,
            overrides,
            &pack.crate_lints,
        ));

        parsed.push(ParsedCrate {
            crate_name: crate_name.clone(),
            short_name: info.short_name.clone(),
            source,
            tree,
            all_sources,
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
            (p.crate_name.as_str(), LintContext {
                crate_name: &p.crate_name,
                short_name: &p.short_name,
                source: &p.source,
                tree: &p.tree,
                all_sources: &p.all_sources,
                deps: &info.deps,
                all_crates: &all_crate_names,
                design_doc: p.design_doc.as_deref(),
                all_doc_content: &p.all_doc_content,
                shame_doc: p.shame_doc.as_deref(),
                workspace_root,
                proc_macro_crates,
                lint_proc_macro_source,
                crate_prefix,
                primitive_introductions,
            })
        })
        .collect();

    let cross_refs: Vec<(&str, &LintContext)> = cross_crate_contexts
        .iter()
        .map(|(name, ctx)| (*name, ctx))
        .collect();

    all_errors.extend(mockspace_lint_rules::check_workspace_with_extra(
        &cross_refs,
        doc_only,
        overrides,
        &pack.workspace_lints,
    ));

    // Repo lints take no crates at all, so they run even when the workspace has
    // none. They used to be cross-crate lints that stole a path from the first
    // crate, which made them silently inert in exactly that case.
    let repo_ctx = RepoContext {
        mock_dir:   workspace_root,
        repo_root:  workspace_root.parent().unwrap_or(workspace_root),
        all_crates: &all_crate_names,
        src_dirs,
        invocation: None,
        canon_paths,
        open_panels,
        registry,
    };
    all_errors.extend(mockspace_lint_rules::check_repo_with_extra(
        &repo_ctx,
        doc_only,
        overrides,
        &pack.repo_lints,
    ));

    // Partition by effective severity
    let mut info = Vec::new();
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    for e in all_errors {
        match e.severity.effective(mode) {
            Level::Pass => {}, // skip silently
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
