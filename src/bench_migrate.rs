//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock bench migrate`: results and history in an older layout, moved into
//! run directories where the move is one to one.
//!
//! What it translates, and how, with [`mockspace_bench_harness::layout::place`] saying
//! which files those are:
//!
//! - a result triple, `<sweep>_n<point>.csv`, `.meta.json` and `_findings.md`
//!   or `_report.md`, under `results/<bench>/` or at the bench root, becomes
//!   one run's `samples.csv`, `meta.json` and `report.md`, byte for byte. The
//!   run is named from the meta's `timestamp` and `git_commit`, and the machine
//!   class is the meta's own `machine_class` where it has one;
//! - a ledger, `.bench_history/<bench>_n<point>.tsv` or
//!   `history/<bench>/<sweep>_n<point>.tsv`, becomes one `summary.tsv` per run
//!   it recorded, a run being the rows sharing a timestamp and a commit, each
//!   row carried over as it was written. The rows of a run whose result triple
//!   moves too go into that run's directory: the run the ledger recorded at
//!   the meta's commit, `-dirty` aside, no earlier than the meta.
//!
//! The machine an older file was measured on is a fact nothing in a ledger
//! records, and a meta older than the class does not either. `--machine-class`
//! supplies it, and a class that disagrees with the os or the cpu a meta kept is
//! refused. Without it those files are left in place and named.
//!
//! What it does not do: it never replaces a file, it never guesses a bench or
//! a cell a path does not spell out, and it commits nothing. A file whose target
//! exists with the same bytes is a copy already moved, and only the source goes;
//! one whose target exists with other bytes is left and named. Everything left,
//! and every output nothing translates (a member's own results, a directory of
//! runs, a meta without its samples), is listed to be moved by hand, and the
//! command then exits non-zero.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mockspace_bench_harness::env::class_agrees_with;
use mockspace_bench_harness::history::parse_summary;
use mockspace_bench_harness::layout::{self, Cell, Legacy, Part, Placement, SUMMARY_HEADER};

use crate::config::Config;

#[cfg(test)]
#[path = "bench_migrate_tests.rs"]
mod tests;

/// One file to write, and the sources it replaces once written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Write {
    /// The target, absolute.
    pub(crate) target:  PathBuf,
    /// Its bytes.
    pub(crate) bytes:   Vec<u8>,
    /// The file it came from, absolute, removed once every write from it is
    /// done.
    pub(crate) source:  PathBuf,
}

/// What a migration would do.
#[derive(Debug, Default)]
pub(crate) struct Plan {
    /// Files to write.
    pub(crate) writes:  Vec<Write>,
    /// Sources whose content already sits at their target.
    pub(crate) already: Vec<PathBuf>,
    /// Files left in place, each with why, relative to the bench tree.
    pub(crate) by_hand: Vec<(PathBuf, String)>,
}

/// The facts a migration reads from an older meta.
#[derive(Debug, Default)]
struct OldMeta {
    timestamp: u64,
    commit:    String,
    class:     Option<String>,
    os:        String,
    cpu:       String,
}

fn read_meta(path: &Path) -> Result<OldMeta, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read it: {e}"))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("it is not JSON: {e}"))?;
    let s = |k: &str| v.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    Ok(OldMeta {
        timestamp: v
            .get("timestamp")
            .and_then(serde_json::Value::as_u64)
            .ok_or("it records no timestamp, so the run it belongs to cannot be named")?,
        commit:    s("git_commit"),
        class:     Some(s("machine_class")).filter(|c| layout::is_machine_class(c)),
        os:        s("os"),
        cpu:       s("cpu"),
    })
}

/// Every file under `bench_dir` but `target/`, `.git/` and the harness's own
/// in-flight and void trees, relative to it.
fn files(bench_dir: &Path) -> Vec<PathBuf> {
    let skip_results = [layout::INFLIGHT_DIR, layout::VOID_DIR];
    let mut out = Vec::new();
    let mut stack = vec![bench_dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                let under_results = d == bench_dir.join(layout::RESULTS_DIR);
                if name == "target" || name == ".git" || (under_results && skip_results.contains(&name.as_str())) {
                    continue;
                }
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(bench_dir) {
                out.push(rel.to_path_buf());
            }
        }
    }
    out.sort();
    out
}

/// What `mock bench migrate` would do in the bench tree at `bench_dir`, with
/// `machine` the class supplied for files that name none. Reads, writes
/// nothing.
pub(crate) fn plan(bench_dir: &Path, machine: Option<&str>) -> Plan {
    let mut plan = Plan::default();
    // Result parts grouped by the directory they sit in and the cell, so a
    // triple moves together, named by its own meta.
    let mut results: BTreeMap<(PathBuf, String, Cell), BTreeMap<Part, Vec<PathBuf>>> = BTreeMap::new();
    let mut ledgers: Vec<(PathBuf, Legacy)> = Vec::new();
    for rel in files(bench_dir) {
        match layout::place(&rel, |p| bench_dir.join(p).exists()) {
            Placement::NotOutput | Placement::InLayout => {},
            Placement::Stray(s) => {
                plan.by_hand.push((rel, format!("{}, and nothing translates it one to one", s.says())))
            },
            Placement::Legacy(l) if l.part == Part::History => ledgers.push((rel, l)),
            Placement::Legacy(l) => {
                let dir = rel.parent().map(Path::to_path_buf).unwrap_or_default();
                results
                    .entry((dir, l.bench, l.cell))
                    .or_default()
                    .entry(l.part)
                    .or_default()
                    .push(rel);
            },
        }
    }
    // The metas of each cell, which a ledger of that cell is checked against.
    let mut metas: BTreeMap<(String, Cell), Vec<OldMeta>> = BTreeMap::new();
    for ((_, bench, cell), parts) in results {
        if let Some(m) = plan_result(bench_dir, machine, &bench, &cell, parts, &mut plan) {
            metas.entry((bench, cell)).or_default().push(m);
        }
    }
    for (rel, l) in ledgers {
        let seen = metas.get(&(l.bench.clone(), l.cell.clone())).map_or(&[][..], Vec::as_slice);
        plan_ledger(bench_dir, machine, &rel, &l, seen, &mut plan);
    }
    settle_targets(&mut plan);
    plan.by_hand.sort();
    plan
}

/// The machine class for files whose meta is `meta` (or that have none), or why
/// there is none to use.
fn class_for(meta: Option<&OldMeta>, machine: Option<&str>) -> Result<String, String> {
    if let Some(c) = meta.and_then(|m| m.class.clone()) {
        return Ok(c);
    }
    let Some(m) = machine else {
        return Err("records no machine class; pass `--machine-class <class>` naming the machine \
                    that measured it, in the shape `mock bench run` prints"
            .to_string());
    };
    if let Some(meta) = meta {
        if !class_agrees_with(m, &meta.os, &meta.cpu) {
            return Err(format!(
                "was measured on `{}` (`{}`), which `--machine-class {m}` does not describe",
                meta.cpu, meta.os
            ));
        }
    }
    Ok(m.to_string())
}

fn plan_result(
    bench_dir: &Path,
    machine: Option<&str>,
    bench: &str,
    cell: &Cell,
    parts: BTreeMap<Part, Vec<PathBuf>>,
    plan: &mut Plan,
) -> Option<OldMeta> {
    let all: Vec<PathBuf> = parts.values().flatten().cloned().collect();
    let mut leave = |why: String| {
        for p in &all {
            plan.by_hand.push((p.clone(), why.clone()));
        }
    };
    if parts.values().any(|v| v.len() > 1) {
        leave("is one of two reports for one cell, and which one the run kept is not on disk".into());
        return None;
    }
    let Some(meta_rel) = parts.get(&Part::Meta).and_then(|v| v.first()) else {
        leave("has no meta beside it, so the run and the machine it belongs to are not on disk".into());
        return None;
    };
    let meta = match read_meta(&bench_dir.join(meta_rel)) {
        Ok(m) => m,
        Err(e) => {
            leave(format!("belongs to a meta that {e}"));
            return None;
        },
    };
    let class = match class_for(Some(&meta), machine) {
        Ok(c) => c,
        Err(e) => {
            leave(format!("belongs to a run that {e}"));
            return Some(meta);
        },
    };
    let run = layout::run_id(meta.timestamp, &meta.commit);
    let dir = bench_dir
        .join(layout::RESULTS_DIR)
        .join(layout::run_rel(bench, cell, &class, &run));
    let mut writes = Vec::new();
    for (part, srcs) in parts {
        let source = bench_dir.join(&srcs[0]);
        match std::fs::read(&source) {
            Ok(bytes) => {
                writes.push(Write {
                    target: dir.join(part.run_file()),
                    bytes,
                    source,
                })
            },
            Err(e) => {
                leave(format!("belongs to a run one of whose files cannot be read: {e}"));
                return Some(meta);
            },
        }
    }
    plan.writes.extend(writes);
    Some(meta)
}

/// A ledger's rows split into one summary per run. `metas` are the metas found
/// for the same cell: a ledger records no machine, so the supplied class is
/// checked against what they kept, and refused where one disagrees.
fn plan_ledger(
    bench_dir: &Path,
    machine: Option<&str>,
    rel: &Path,
    l: &Legacy,
    metas: &[OldMeta],
    plan: &mut Plan,
) {
    let source = bench_dir.join(rel);
    let text = match std::fs::read_to_string(&source) {
        Ok(t) => t,
        Err(e) => return plan.by_hand.push((rel.to_path_buf(), format!("cannot be read: {e}"))),
    };
    // A run is the rows sharing a timestamp and a commit, in the order written.
    let mut runs: BTreeMap<(u64, String), Vec<&str>> = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let parsed = parse_summary(line);
        let Some(row) = parsed.first() else {
            return plan.by_hand.push((
                rel.to_path_buf(),
                format!("has a row that is not in the ledger's schema: `{line}`"),
            ));
        };
        runs.entry((row.timestamp, row.git_commit.clone())).or_default().push(line);
    }
    let groups: Vec<(u64, String)> = runs.keys().cloned().collect();
    let matched = match_metas(&groups, metas);
    let mut writes = Vec::new();
    for (((ts, commit), lines), meta) in runs.into_iter().zip(matched) {
        // The rows of a run whose results moved too go into that run's
        // directory, under the run id and the class its meta gave it; rows no
        // meta matches name their own run.
        let class = match meta.and_then(|m| m.class.clone()) {
            Some(c) => c,
            None => {
                match class_for(None, machine) {
                    Ok(c) => c,
                    Err(e) => return plan.by_hand.push((rel.to_path_buf(), format!("is a ledger that {e}"))),
                }
            },
        };
        if let Some(m) = metas.iter().find(|m| !class_agrees_with(&class, &m.os, &m.cpu)) {
            return plan.by_hand.push((
                rel.to_path_buf(),
                format!(
                    "is a ledger of a cell whose results were measured on `{}` (`{}`), which \
                     `{class}` does not describe",
                    m.cpu, m.os
                ),
            ));
        }
        let run = match meta {
            Some(m) => layout::run_id(m.timestamp, &m.commit),
            None => layout::run_id(ts, &commit),
        };
        let target = bench_dir
            .join(layout::RESULTS_DIR)
            .join(layout::run_rel(&l.bench, &l.cell, &class, &run))
            .join(layout::SUMMARY_FILE);
        let mut body = format!("{SUMMARY_HEADER}\n");
        for line in lines {
            body.push_str(line);
            body.push('\n');
        }
        writes.push(Write {
            target,
            bytes: body.into_bytes(),
            source: source.clone(),
        });
    }
    plan.writes.extend(writes);
}

/// The meta each ledger run belongs to, in the order of `groups`: the old
/// driver wrote a cell's meta as the cell ran and its ledger rows a little
/// after, at the commit without its `-dirty` mark. So a run `(timestamp,
/// commit)` takes the latest meta of the same commit, the mark stripped on
/// both sides, taken no later than the rows, and each meta goes to the nearest
/// run after it only. A run no meta matches gets `None` and keeps its own id.
fn match_metas<'m>(groups: &[(u64, String)], metas: &'m [OldMeta]) -> Vec<Option<&'m OldMeta>> {
    let clean = |c: &str| c.trim().trim_end_matches("-dirty").to_string();
    let mut best: Vec<Option<(usize, &OldMeta)>> = groups
        .iter()
        .map(|(ts, commit)| {
            metas
                .iter()
                .enumerate()
                .filter(|(_, m)| clean(&m.commit) == clean(commit) && m.timestamp <= *ts)
                .max_by_key(|(_, m)| m.timestamp)
        })
        .collect();
    // A meta claimed by several runs belongs to the one nearest after it.
    for i in 0 .. best.len() {
        let Some((mi, m)) = best[i] else { continue };
        let nearest = (0 .. best.len())
            .filter(|j| best[*j].is_some_and(|(mj, _)| mj == mi))
            .min_by_key(|j| groups[*j].0 - m.timestamp);
        if nearest != Some(i) {
            best[i] = None;
        }
    }
    best.into_iter().map(|b| b.map(|(_, m)| m)).collect()
}

/// Settle every write against what is already on disk and against the other
/// writes: a target holding the same bytes is a copy already moved, and two
/// writes or an existing file disagreeing about one target leave every source
/// involved in place.
fn settle_targets(plan: &mut Plan) {
    let mut by_target: BTreeMap<PathBuf, Vec<Write>> = BTreeMap::new();
    for w in std::mem::take(&mut plan.writes) {
        by_target.entry(w.target.clone()).or_default().push(w);
    }
    let mut blocked: Vec<PathBuf> = Vec::new();
    let mut kept: Vec<Write> = Vec::new();
    for (target, writes) in by_target {
        let existing = std::fs::read(&target).ok();
        let first = writes[0].bytes.clone();
        let agree = writes.iter().all(|w| w.bytes == first) && existing.as_ref().is_none_or(|e| *e == first);
        if !agree {
            blocked.extend(writes.iter().map(|w| w.source.clone()));
            continue;
        }
        let mut writes = writes.into_iter();
        if existing.is_some() {
            plan.already.extend(writes.map(|w| w.source));
        } else if let Some(w) = writes.next() {
            kept.push(w);
            plan.already.extend(writes.map(|w| w.source));
        }
    }
    // A source with any blocked write stays whole: none of its writes happen.
    blocked.sort();
    blocked.dedup();
    plan.writes = kept.into_iter().filter(|w| !blocked.contains(&w.source)).collect();
    plan.already.retain(|s| !blocked.contains(s));
    for s in blocked {
        plan.by_hand.push((
            s,
            "would replace a file of a run already in place with other bytes".to_string(),
        ));
    }
    plan.already.sort();
    plan.already.dedup();
}

/// Carry a plan out: write every target, then remove every source whose
/// writes all landed, then the directories the move emptied.
pub(crate) fn apply(bench_dir: &Path, plan: &Plan) -> Result<usize, String> {
    let mut written = 0;
    for w in &plan.writes {
        if let Some(parent) = w.target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&w.target)
            .and_then(|mut f| std::io::Write::write_all(&mut f, &w.bytes))
            .map_err(|e| format!("writing {}: {e}", w.target.display()))?;
        written += 1;
    }
    let mut sources: Vec<&PathBuf> = plan.writes.iter().map(|w| &w.source).chain(&plan.already).collect();
    sources.sort();
    sources.dedup();
    for s in sources {
        std::fs::remove_file(s).map_err(|e| format!("removing {}: {e}", s.display()))?;
        let mut dir = s.parent();
        while let Some(d) = dir {
            if d == bench_dir || std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    Ok(written)
}

/// `mock bench migrate [--machine-class <class>] [--dry-run]`, and
/// `mock bench migrate --kinds [--dry-run]`, which is `crate::bench_kinds`'s.
pub fn cmd(cfg: &Config, args: &[&str]) -> ExitCode {
    if args.contains(&"--kinds") {
        return crate::bench_kinds::cmd_migrate(cfg, args);
    }
    let mut machine: Option<&str> = None;
    let mut dry_run = false;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--dry-run" => dry_run = true,
            "--machine-class" => {
                let Some(v) = args.get(i + 1) else {
                    eprintln!("error: `--machine-class` wants a class, such as `macos-aarch64-c8-bare-apple`");
                    return ExitCode::FAILURE;
                };
                if !layout::is_machine_class(v) {
                    eprintln!(
                        "error: `{v}` is not a machine class. A class reads \
                         `<os>-<arch>-c<cores>-<container|bare>-<cpu family>`, as `mock bench run` prints it."
                    );
                    return ExitCode::FAILURE;
                }
                machine = Some(v);
                i += 1;
            },
            other => {
                eprintln!("error: `mock bench migrate` takes `--machine-class <class>` and `--dry-run`, not `{other}`");
                return ExitCode::FAILURE;
            },
        }
        i += 1;
    }
    let bench_dir = cfg.mock_dir.join("benches");
    if !bench_dir.is_dir() {
        eprintln!("error: {} does not exist; there is nothing to migrate", bench_dir.display());
        return ExitCode::FAILURE;
    }
    let plan = plan(&bench_dir, machine);
    let rel = |p: &Path| p.strip_prefix(&bench_dir).unwrap_or(p).display().to_string();
    if dry_run {
        for w in &plan.writes {
            println!("{} -> {}", rel(&w.source), rel(&w.target));
        }
        for s in &plan.already {
            println!("{} already in place, would be removed", rel(s));
        }
    } else {
        match apply(&bench_dir, &plan) {
            Ok(n) => {
                if n > 0 || !plan.already.is_empty() {
                    println!(
                        "migrated: {n} file(s) written into run directories, {} copie(s) already in place removed; review and commit",
                        plan.already.len()
                    );
                }
            },
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        }
    }
    if plan.by_hand.is_empty() {
        return ExitCode::SUCCESS;
    }
    eprintln!("left in place, to move by hand into `results/<bench>/<sweep>_n<point>/<machine class>/<run>/`:");
    for (p, why) in &plan.by_hand {
        eprintln!("  {} {why}", p.display());
    }
    ExitCode::FAILURE
}

/// Say once, before a run, that the tree still holds outputs in an older
/// layout, which the run's regression check cannot read.
pub fn warn_if_legacy(bench_dir: &Path) {
    let old = files(bench_dir)
        .iter()
        .filter(|rel| matches!(layout::place(rel, |p| bench_dir.join(p).exists()), Placement::Legacy(_) | Placement::Stray(_)))
        .count();
    if old > 0 {
        eprintln!(
            "warning: {old} bench output(s) under {} sit outside the harness layout, and this \
             run's history does not read them. `mock bench migrate` moves the ones it can.",
            bench_dir.display()
        );
    }
}
