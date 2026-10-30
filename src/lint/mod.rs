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
    Level,
    LintConfig,
    LintContext,
    LintError,
    LintMode,
    LintPack,
    RepoContext,
};

use crate::model::CrateMap;

/// What every crate's [`LintContext`] shares, held once so a run and a single
/// edit's check build their contexts from the same place.
struct Environment<'a> {
    all_crates:              BTreeSet<String>,
    workspace_root:          &'a Path,
    proc_macro_crates:       &'a [String],
    lint_proc_macro_source:  bool,
    crate_prefix:            &'a str,
    primitive_introductions: &'a std::collections::BTreeMap<String, Vec<String>>,
}

/// Collected data for a single crate, kept alive for cross-crate pass.
struct ParsedCrate {
    crate_name:      String,
    short_name:      String,
    deps:            Vec<String>,
    source:          String,
    tree:            tree_sitter::Tree,
    all_sources:     Vec<CrateSourceFile>,
    design_doc:      Option<String>,
    all_doc_content: String,
    shame_doc:       Option<String>,
}

impl ParsedCrate {
    /// Read a crate off disk. `replacing` is a file's crate-relative path and the
    /// text it is about to have, laid over what is on disk so a lint judges the
    /// crate as it would stand after an edit that has not happened yet.
    ///
    /// `None` when the crate has no readable `src/lib.rs` or it does not parse,
    /// which a run skips and the compiler reports better.
    fn load(
        crate_name: &str,
        info: &crate::model::CrateInfo,
        crate_dir: &Path,
        parser: &mut tree_sitter::Parser,
        replacing: Option<(&Path, &str)>,
    ) -> Option<Self> {
        // Read every .rs file under src/ so lints can scan module files
        // (bits.rs, prim.rs, impl.rs, ...) in addition to lib.rs.
        let mut all_sources = collect_crate_sources(crate_dir);
        if let Some((rel_path, text)) = replacing {
            match all_sources.iter_mut().find(|s| s.rel_path == rel_path) {
                Some(file) => file.text = text.to_string(),
                None => {
                    all_sources.push(CrateSourceFile {
                        rel_path: rel_path.to_path_buf(),
                        text:     text.to_string(),
                    });
                    all_sources.sort_by(|a, b| {
                        let lib = Path::new("src/lib.rs");
                        (a.rel_path != lib, &a.rel_path).cmp(&(b.rel_path != lib, &b.rel_path))
                    });
                },
            }
        }
        let source = all_sources
            .iter()
            .find(|s| s.rel_path == Path::new("src/lib.rs"))?
            .text
            .clone();
        let tree = parser.parse(&source, None)?;
        Some(Self {
            crate_name: crate_name.to_string(),
            short_name: info.short_name.clone(),
            deps: info.deps.clone(),
            source,
            tree,
            all_sources,
            design_doc: std::fs::read_to_string(crate_dir.join("DESIGN.md.tmpl")).ok(),
            all_doc_content: collect_all_docs(crate_dir),
            // The exact spelling the phase gates match. See `read_shame_template`.
            shame_doc: mockspace_lint_rules::read_shame_template(crate_dir),
        })
    }

    fn context<'a>(&'a self, env: &'a Environment<'a>) -> LintContext<'a> {
        LintContext {
            crate_name:              &self.crate_name,
            short_name:              &self.short_name,
            source:                  &self.source,
            tree:                    &self.tree,
            all_sources:             &self.all_sources,
            deps:                    &self.deps,
            all_crates:              &env.all_crates,
            design_doc:              self.design_doc.as_deref(),
            all_doc_content:         &self.all_doc_content,
            shame_doc:               self.shame_doc.as_deref(),
            workspace_root:          env.workspace_root,
            proc_macro_crates:       env.proc_macro_crates,
            lint_proc_macro_source:  env.lint_proc_macro_source,
            crate_prefix:            env.crate_prefix,
            primitive_introductions: env.primitive_introductions,
        }
    }
}

/// One edit's verdict from one named crate lint.
///
/// The findings of lint `lint_name` over the crate holding `file`, as the crate
/// would stand with `file` reading `text`, kept to those in `file` itself. A
/// finding elsewhere in the crate is the crate's and not this edit's, and
/// blocking an edit for it would refuse every write to a crate that already has
/// one.
///
/// This is the same pass [`run_lints`] gives the crate, through
/// [`mockspace_lint_rules::check_crate_named`], so the verdict is the lint's and
/// nothing here re-judges it. `None` when `file` is in no crate this repository
/// has, or the crate has no parseable root.
#[must_use]
pub fn lint_edit(
    crates: &CrateMap,
    src_dirs: &[PathBuf],
    mock_dir: &Path,
    proc_macro_crates: &[String],
    lint_proc_macro_source: bool,
    crate_prefix: &str,
    lint_overrides: &LintConfig,
    primitive_introductions: &std::collections::BTreeMap<String, Vec<String>>,
    pack: &LintPack,
    lint_name: &str,
    file: &Path,
    text: &str,
) -> Option<Vec<LintError>> {
    // The first component under a source directory names the crate; the rest is
    // the file's place in it.
    let (crate_dir, rel_path) = src_dirs.iter().find_map(|d| {
        let rest = file.strip_prefix(d).ok()?;
        let mut parts = rest.components();
        let name = parts.next()?;
        Some((d.join(name), parts.as_path().to_path_buf()))
    })?;
    // Only a crate's Rust sources are what a crate lint reads; any other file in
    // the crate's directory is not one of them and must not be laid over them.
    if !rel_path.starts_with("src") || rel_path.extension().is_none_or(|e| e != "rs") {
        return None;
    }
    let crate_name = crate_dir.file_name()?.to_str()?;
    let info = crates.get(crate_name)?;

    let env = Environment {
        all_crates: crates.keys().cloned().collect(),
        workspace_root: mock_dir,
        proc_macro_crates,
        lint_proc_macro_source,
        crate_prefix,
        primitive_introductions,
    };
    let mut parser = mockspace_lint_rules::make_parser();
    let krate = ParsedCrate::load(crate_name, info, &crate_dir, &mut parser, Some((&rel_path, text)))?;

    let overrides = if lint_overrides.is_empty() { None } else { Some(lint_overrides) };
    let found = mockspace_lint_rules::check_crate_named(
        &krate.context(&env),
        lint_name,
        overrides,
        &pack.crate_lints,
    );
    // The crate root's findings carry no path, and a module file's carry its own.
    let here = rel_path.to_string_lossy();
    Some(
        found
            .into_iter()
            .filter(|e| e.path.as_deref().unwrap_or("src/lib.rs") == here)
            .collect(),
    )
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

    let workspace_root = mock_dir;
    let env = Environment {
        all_crates: crates.keys().cloned().collect(),
        workspace_root: mock_dir,
        proc_macro_crates,
        lint_proc_macro_source,
        crate_prefix,
        primitive_introductions,
    };

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
        let Some(crate_dir) = src_dirs
            .iter()
            .map(|d| d.join(crate_name))
            .find(|d| d.is_dir())
        else {
            continue;
        };
        let Some(krate) = ParsedCrate::load(crate_name, info, &crate_dir, &mut parser, None) else {
            continue;
        };

        // Per-crate lint pass
        all_errors.extend(mockspace_lint_rules::check_crate_with_extra(
            &krate.context(&env),
            doc_only,
            overrides,
            &pack.crate_lints,
        ));

        parsed.push(krate);
    }

    // Cross-crate lint pass
    let cross_crate_contexts: Vec<(&str, LintContext)> = parsed
        .iter()
        .map(|p| (p.crate_name.as_str(), p.context(&env)))
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
        mock_dir: workspace_root,
        repo_root: workspace_root.parent().unwrap_or(workspace_root),
        all_crates: &env.all_crates,
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
