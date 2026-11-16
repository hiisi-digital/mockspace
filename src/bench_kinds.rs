//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The tool's side of the two bench kinds.
//!
//! `mock bench run` and `report` refuse a tree in which a bench is out of its
//! kind's contract, asking the same predicate as the lint
//! `a-bench-keeps-the-contract-of-its-kind`. `mock bench freeze [<bench>...]`
//! writes the lock of each snapshot that has none, from the files it builds
//! from today, and refuses to rewrite a lock its files no longer match: a
//! snapshot is frozen once. `mock bench migrate --kinds` writes into every
//! `bench.toml` that declares no kind the one its arms admit, snapshot where
//! they build only from the bench tree and tracking where one reaches outside
//! it, and then freezes the snapshots.

use std::path::Path;
use std::process::ExitCode;

use mockspace_bench_harness::layout::Kind;
use mockspace_lint_rules::{BenchKindDeclared as Declared, Reading, bench_kind_plans, bench_kinds};

use crate::config::Config;

/// Refuse a tree with a bench out of its kind's contract, naming each one.
pub fn refuse_kind_findings(cfg: &Config, bench_dir: &Path) -> Result<(), ExitCode> {
    let reading = Reading::new(&cfg.repo_root);
    let findings = bench_kinds(&reading.view(), bench_dir);
    if findings.is_empty() {
        return Ok(());
    }
    for f in &findings {
        eprintln!("error: {f}");
    }
    Err(ExitCode::FAILURE)
}

/// Write the lock of every snapshot `names` selects (every snapshot, where
/// `names` is empty) that has none. Returns one line per lock written.
pub fn freeze(repo_root: &Path, bench_dir: &Path, names: &[&str]) -> Result<Vec<String>, String> {
    let reading = Reading::new(repo_root);
    let plans = bench_kind_plans(&reading.view(), bench_dir);
    for name in names {
        if !plans.iter().any(|p| p.bench == *name) {
            return Err(format!("no bench `{name}` in {}", bench_dir.display()));
        }
    }
    let mut written = Vec::new();
    for p in plans
        .iter()
        .filter(|p| names.is_empty() || names.contains(&p.bench.as_str()))
    {
        if p.declared != Declared::Kind(Kind::Snapshot) {
            if !names.is_empty() {
                return Err(format!(
                    "bench `{}` is not declared `kind = \"snapshot\"`; only a snapshot has a lock",
                    p.bench
                ));
            }
            continue;
        }
        if !p.outside.is_empty() {
            return Err(format!(
                "snapshot `{}` reaches outside the bench tree ({}), so there is nothing a lock \
                 could freeze; bring it into the tree first",
                p.bench,
                p.outside.join("; ")
            ));
        }
        let body = p.lock_body.clone().unwrap_or_default();
        let lock = bench_dir.join(&p.lock);
        match std::fs::read_to_string(&lock) {
            Ok(was) if was == body => {},
            Ok(_) => {
                return Err(format!(
                    "snapshot `{}` already has a lock, {}, and its files no longer match it. A \
                     snapshot is frozen once: put the files back, or make the changed code a new \
                     bench beside it.",
                    p.bench,
                    p.lock.display()
                ));
            },
            Err(_) => {
                std::fs::write(&lock, &body)
                    .map_err(|e| format!("writing {}: {e}", lock.display()))?;
                written.push(format!("froze `{}` in {}", p.bench, p.lock.display()));
            },
        }
    }
    Ok(written)
}

/// Write the admitted kind into every bench that declares none, then freeze
/// the snapshots. With `dry_run`, say what would be written and write nothing.
pub fn migrate(repo_root: &Path, bench_dir: &Path, dry_run: bool) -> Result<Vec<String>, String> {
    let reading = Reading::new(repo_root);
    let plans = bench_kind_plans(&reading.view(), bench_dir);
    let mut said = Vec::new();
    let mut frozen = Vec::new();
    for p in plans.iter().filter(|p| p.declared == Declared::Missing) {
        let reach = if p.outside.is_empty() {
            "it builds only from the bench tree".to_string()
        } else {
            format!(
                "it reaches outside the bench tree: {}",
                p.outside.join("; ")
            )
        };
        said.push(format!(
            "{}: `kind = \"{}\"`, since {reach}",
            p.bench, p.admits
        ));
        if p.admits == Kind::Snapshot {
            frozen.push(p.bench.clone());
        }
        if !dry_run {
            declare(&bench_dir.join(&p.file), p.section.as_deref(), p.admits)?;
        }
    }
    if !dry_run && !frozen.is_empty() {
        let names: Vec<&str> = frozen.iter().map(String::as_str).collect();
        said.extend(freeze(repo_root, bench_dir, &names)?);
    }
    Ok(said)
}

/// Write `kind = "<kind>"` into the bench `file` declares: at its top for a
/// composed bench, into `[bench.<section>]` for a section.
fn declare(file: &Path, section: Option<&str>, kind: Kind) -> Result<(), String> {
    let text =
        std::fs::read_to_string(file).map_err(|e| format!("reading {}: {e}", file.display()))?;
    let out = match section {
        // A composed bench's kind is the first thing its file says, so it
        // reads before anything the file goes on to declare.
        None => format!("kind = \"{}\"\n{text}", kind.name()),
        Some(s) => {
            let mut doc: toml_edit::DocumentMut = text
                .parse()
                .map_err(|e| format!("{}: {e}", file.display()))?;
            let table = doc
                .get_mut("bench")
                .and_then(|b| b.get_mut(s))
                .and_then(toml_edit::Item::as_table_mut)
                .ok_or_else(|| {
                    format!(
                        "{}: no `[bench.{s}]` table to declare the kind in",
                        file.display()
                    )
                })?;
            table.insert("kind", toml_edit::value(kind.name()));
            doc.to_string()
        },
    };
    std::fs::write(file, out).map_err(|e| format!("writing {}: {e}", file.display()))
}

/// `mock bench freeze [<bench>...]`.
pub fn cmd_freeze(cfg: &Config, args: &[&str]) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    match freeze(&cfg.repo_root, &bench_dir, args) {
        Ok(lines) => {
            for l in lines {
                println!("{l}");
            }
            ExitCode::SUCCESS
        },
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        },
    }
}

/// `mock bench migrate --kinds [--dry-run]`.
pub fn cmd_migrate(cfg: &Config, args: &[&str]) -> ExitCode {
    if let Some(other) = args
        .iter()
        .find(|a| !matches!(**a, "--kinds" | "--dry-run"))
    {
        eprintln!(
            "error: `mock bench migrate --kinds` takes `--dry-run` and nothing else, not `{other}`"
        );
        return ExitCode::FAILURE;
    }
    let bench_dir = cfg.mock_dir.join("benches");
    match migrate(&cfg.repo_root, &bench_dir, args.contains(&"--dry-run")) {
        Ok(lines) => {
            for l in lines {
                println!("{l}");
            }
            ExitCode::SUCCESS
        },
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        },
    }
}

#[cfg(test)]
#[path = "bench_kinds_tests.rs"]
mod tests;
