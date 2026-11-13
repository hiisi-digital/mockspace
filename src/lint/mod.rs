//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Pluggable AST lint system.
//!
//! Lint rules live in the `mockspace-lint-rules` crate. This module hands them the mock workspace:
//! one [`Reading`] of the repository for the whole run, the crates of the [`CrateMap`] as
//! [`CrateInput`]s, and the store saved units live in, and lets
//! [`mockspace_lint_rules::run`] judge every lint over it. A unit whose saved trace still holds is
//! replayed; the rest are judged on the run's threads.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

mod fold;
pub mod verdicts;

#[cfg(test)]
mod edit_tests;
#[cfg(test)]
mod scope_tests;

use mockspace_lint_rules::{
    self,
    CrateInput,
    Level,
    LintConfig,
    LintError,
    LintMode,
    LintPack,
    Reading,
    RepoInput,
    RunInput,
    WorkspaceInput,
    timing,
};

use crate::model::CrateMap;

/// What every crate's context shares, from the run's configuration.
fn workspace_input(
    all_crates: BTreeSet<String>,
    mock_dir: &Path,
    proc_macro_crates: &[String],
    lint_proc_macro_source: bool,
    crate_prefix: &str,
    primitive_introductions: &BTreeMap<String, Vec<String>>,
) -> WorkspaceInput {
    WorkspaceInput {
        all_crates,
        workspace_root: mock_dir.to_path_buf(),
        proc_macro_crates: proc_macro_crates.to_vec(),
        lint_proc_macro_source,
        crate_prefix: crate_prefix.to_string(),
        primitive_introductions: primitive_introductions.clone(),
    }
}

/// The crate as a run hands it to the lints.
fn crate_input(name: &str, info: &crate::model::CrateInfo, dir: PathBuf) -> CrateInput {
    CrateInput {
        name: name.to_string(),
        short_name: info.short_name.clone(),
        deps: info.deps.clone(),
        dir,
    }
}

/// One edit's verdict from one named crate lint.
///
/// The findings of lint `lint_name` in `file` as the crate would stand with `file` reading
/// `text`. A finding elsewhere in the crate is the crate's and not this file's, so only the ones
/// naming `file` are returned. Nothing is subtracted for what the file already holds: the verdict
/// is the lint's, as the gate would give it.
///
/// This is the same runner [`run_lints`] uses, over a [`Reading`] rooted where its reading is, at
/// `repo_root`, with `text` laid over `file`, so the verdict is the lint's and nothing here
/// re-judges it. The two readings have one root so their traces name a file alike, and a unit the
/// edit does not reach is replayed from what the last run saved; the overlaid run saves nothing.
/// `repo_root` and `src_dirs` are both real paths or both as configured. Meant for lints
/// that judge one file at a time, whose findings name the file they are in. `None` when `file` is
/// in no crate this repository has, or is not one of a crate's Rust sources. A `lint_name` that is
/// no crate lint finds nothing.
#[must_use]
pub fn lint_edit(
    crates: &CrateMap,
    src_dirs: &[PathBuf],
    mock_dir: &Path,
    repo_root: &Path,
    proc_macro_crates: &[String],
    lint_proc_macro_source: bool,
    crate_prefix: &str,
    lint_overrides: &LintConfig,
    primitive_introductions: &BTreeMap<String, Vec<String>>,
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
    let reading = Reading::new(repo_root).with_overlay(crate_dir.join(&rel_path), text);
    // Discovery takes a real crate directory holding a `src/lib.rs`, and nothing else is a
    // crate to the gate. A crate with no root yet has no entry; the edit that writes the root
    // makes it one, built from the text about to be written. Any other file of a directory
    // the gate does not read is judged by nothing, as at commit.
    let new_root;
    let info = match crates.get(crate_name) {
        Some(info) => info,
        None => {
            let is_link = std::fs::symlink_metadata(&crate_dir).is_ok_and(|m| m.file_type().is_symlink());
            if is_link || rel_path != Path::new("src/lib.rs") {
                return None;
            }
            let mut parser = mockspace_lint_rules::make_parser();
            new_root = crate::parse::crate_info(&mut parser, &crate_dir, crate_name, text, crate_prefix);
            &new_root
        },
    };
    let is_crate_lint = mockspace_lint_rules::all_lints()
        .iter()
        .map(|l| l.name())
        .chain(pack.crate_lints.iter().map(|l| l.name()))
        .any(|n| n == lint_name);
    if !is_crate_lint {
        return Some(Vec::new());
    }
    let mut all_crates: BTreeSet<String> = crates.keys().cloned().collect();
    all_crates.insert(crate_name.to_string());
    let ws = workspace_input(
        all_crates,
        mock_dir,
        proc_macro_crates,
        lint_proc_macro_source,
        crate_prefix,
        primitive_introductions,
    );
    let krate = [crate_input(crate_name, info, crate_dir.clone())];
    let store = verdicts::DiskVerdicts::open(&pack.identity);
    let input = RunInput {
        reading: &reading,
        cache: store.as_ref().map(|s| s as &dyn mockspace_lint_rules::verdict::VerdictCache),
        overrides: if lint_overrides.is_empty() { None } else { Some(lint_overrides) },
        doc_only: false,
        workspace: &ws,
        crates: &krate,
        scope: None,
        repo: None,
        only: Some(lint_name),
    };
    // The crate root's findings carry no path, and a module file's carry its own.
    let here = rel_path.to_string_lossy();
    let found = mockspace_lint_rules::run(&input, Some(pack));
    Some(
        found
            .into_iter()
            .filter(|e| e.lint_name == lint_name && e.path.as_deref().unwrap_or("src/lib.rs") == here)
            .collect(),
    )
}

/// Run all lints (per-crate + cross-crate) against crates.
///
/// The `mode` determines which gate is active. Each lint violation declares
/// its own per-gate severity; the mode selects the effective level:
///   - `Commit`: most permissive (pre-commit hook)
///   - `Build`: middle strictness (default xtask run)
///   - `Push`: most strict (pre-push hook)
///
/// When `scope` is `Some`, the per-crate lints run on the crates in the list
/// only. This is used by the hooks to lint only the crates a change touches
/// (the pre-commit hook the staged ones, the pre-push hook the pushed ones).
/// The workspace lints, built-in or from a pack, read every crate whatever the
/// scope, bound by their own `crates` filter, and report every finding they
/// have: a clash between a touched crate and an untouched one is the change's
/// to answer for, and the verdict of a scoped run is then the verdict of an
/// unscoped run over the same tree, so a finding the tree already carries stops
/// the next commit as it stops the next push. A standing finding is lifted
/// through a lint's `crates` binding and never through the scope. A scope
/// naming no crate at all (`--scope infra`) reads none. When `None`, every lint
/// runs over every crate.
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
    repo_root: &Path,
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
    // The files a data ref holds and which of them changed, read off the instances by the caller.
    ref_files: &mockspace_lint_rules::src_layout::RefFiles,
    primitive_introductions: &BTreeMap<String, Vec<String>>,
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

    // The one reading of the run: every lint asks it, and each thing is read once.
    let reading = Reading::new(repo_root);
    let view = reading.view();

    // Saved units: a unit whose trace still holds is answered from the store. None when switched
    // off or when the machine has nowhere to keep them.
    let verdicts = timing::time("phase:open-verdict-store", || {
        verdicts::DiskVerdicts::open(&pack.identity)
    });

    // A cross-crate lint judges the whole set of crates, so a scope that names a crate still
    // hands it every other one: a name defined in a touched crate and in an untouched one is a
    // clash however few crates the hook named. The scope is read as named, not as found: the
    // commit that deletes a crate names it and finds nothing, and what it leaves undocumented
    // elsewhere is its to answer for. Only a scope naming no crate at all (`--scope infra`, the
    // commit touching none) reads none, as it always did.
    let reads_every_crate = scope.is_none_or(|names| !names.is_empty());
    let mut inputs: Vec<CrateInput> = Vec::new();
    if reads_every_crate {
        for (crate_name, info) in crates {
            // The crate lives under exactly one source directory; find which. A crate with no
            // `src/lib.rs` is skipped, which the compiler reports better.
            let Some(crate_dir) = src_dirs.iter().map(|d| d.join(crate_name)).find(|d| view.is_dir(d)) else {
                continue;
            };
            if !view.is_file(crate_dir.join("src/lib.rs")) {
                continue;
            }
            inputs.push(crate_input(crate_name, info, crate_dir));
        }
    }
    let ws = workspace_input(
        crates.keys().cloned().collect(),
        mock_dir,
        proc_macro_crates,
        lint_proc_macro_source,
        crate_prefix,
        primitive_introductions,
    );
    // Repo lints take no crates at all, so they run even when the workspace has
    // none. They used to be cross-crate lints that stole a path from the first
    // crate, which made them silently inert in exactly that case.
    let repo = RepoInput {
        mock_dir,
        repo_root,
        all_crates: &ws.all_crates,
        src_dirs,
        invocation: None,
        canon_paths,
        open_panels,
        ref_files,
        registry,
    };
    let input = RunInput {
        reading: &reading,
        cache: verdicts.as_ref().map(|v| v as &dyn mockspace_lint_rules::verdict::VerdictCache),
        overrides: if lint_overrides.is_empty() { None } else { Some(lint_overrides) },
        doc_only,
        workspace: &ws,
        crates: &inputs,
        scope,
        repo: Some(repo),
        only: None,
    };
    let all_errors = mockspace_lint_rules::run(&input, Some(pack));

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

    crate::gate_report::record(&gate_findings(&info, &warnings, &errors));

    for text in fold::sections(&info, &warnings, fold::wants_all()) {
        eprintln!("{text}");
    }

    if !errors.is_empty() {
        eprintln!("\n--- lint errors ({}) ---", errors.len());
        for err in &errors {
            eprintln!("{err}");
        }
        eprintln!();
    }

    if timing::enabled() {
        eprintln!("\n{}", timing::report());
    }
    // Only errors block; info and warnings are informational
    errors.len()
}

/// The findings of a run as the gate log says them, for a hook that asked for them.
fn gate_findings(
    info: &[mockspace_lint_rules::LintError],
    warnings: &[mockspace_lint_rules::LintError],
    errors: &[mockspace_lint_rules::LintError],
) -> Vec<crate::gate_report::Finding> {
    let one = |severity: &str, e: &mockspace_lint_rules::LintError| crate::gate_report::Finding {
        severity: severity.to_string(),
        kind:     e.lint_name.to_string(),
        location: match &e.path {
            Some(p) => format!("{}/{}:{}", e.crate_name, p, e.line),
            None => format!("{}:{}", e.crate_name, e.line),
        },
        message:  e.message.clone(),
    };
    let mut all = Vec::new();
    all.extend(errors.iter().map(|e| one("error", e)));
    all.extend(warnings.iter().map(|e| one("warn", e)));
    all.extend(info.iter().map(|e| one("info", e)));
    all
}
