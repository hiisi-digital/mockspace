//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The members' suites under nextest, run only where something they depend on
//! changed, with the heavy tests held to a few at a time and skipped where
//! their last pass still stands.
//!
//! **Nothing here is a list somebody keeps.** Which members run is inferred
//! from what changed since each last passed ([`fingerprint`], [`history`]) and
//! from what depends on what ([`graph`]). Which tests are heavy is inferred
//! from how long they took last time, against `[test] heavy_after_secs`. A
//! heavy test is one the earlier runs timed, rather than one the source marks
//! or one that links a GPU: the source of the repository this was built for
//! marks nothing, and what a test links is decided per test binary, which
//! holds the quick tests of a crate beside its slow ones.
//!
//! **What a run leaves behind** is the history the next one selects from. A
//! member is recorded green at its fingerprint when its whole suite passed;
//! each test's time and result is recorded whatever it did.

pub mod args;
pub mod fingerprint;
pub mod graph;
pub mod history;
pub mod junit;
pub mod nextest;
mod report;
pub mod select;
pub mod settings;

use std::process::ExitCode;

use args::Invocation;
use select::{Inputs, Mode};

use crate::config::Config;

/// Runs the members' tests as `inv` asks. `None` when the members tree is not
/// this module's to run (no nextest, or `--plain`), and the caller runs
/// `cargo test` as it always did.
pub fn run(cfg: &Config, inv: &Invocation) -> Option<ExitCode> {
    if inv.plain {
        return None;
    }
    if !nextest::installed(&cfg.mock_dir) {
        eprintln!(
            "note: cargo-nextest is not installed, so the members run under plain cargo test,\n      \
             every test, every time. `cargo install cargo-nextest --locked` gives selection,\n      \
             the heavy-test group and the cache."
        );
        return None;
    }

    let graph = match graph::Graph::read(&cfg.mock_dir) {
        Ok(g) => g,
        // Without the graph nothing can be selected, and the tests still have
        // to run: handed back to `cargo test`, which reports a broken manifest
        // in its own words.
        Err(e) => {
            eprintln!("note: {e}; the members run under plain cargo test this time");
            return None;
        },
    };
    for p in inv.packages.iter().filter(|p| !graph.members.contains(*p)) {
        eprintln!("note: `{p}` is not a member of this workspace, so it is not run");
    }
    let candidates = candidates(&graph, inv);
    if candidates.is_empty() {
        eprintln!("mock test: no member matches the packages asked for");
        return Some(ExitCode::FAILURE);
    }

    // A fingerprint that cannot be taken, outside git for one, runs everything:
    // the selection is an economy, never a reason to test less than asked.
    let fp = match fingerprint::compute(&cfg.repo_root, &cfg.mock_dir, &graph) {
        Ok(fp) => Some(fp),
        Err(e) => {
            eprintln!("note: {e}; every member asked for runs, and nothing is cached");
            None
        },
    };
    let fp_or_empty = fp.clone().unwrap_or_default();
    let mut history = history::History::load(&cfg.mock_dir);
    let flavour = inv.flavour();
    let plan = select::plan(&Inputs {
        mode:       if fp.is_some() { inv.mode } else { Mode::Full },
        cache:      inv.cache && fp.is_some(),
        candidates: &candidates,
        fp:         &fp_or_empty,
        graph:      &graph,
        history:    &history,
        flavour:    &flavour,
        threshold:  cfg.test.heavy_after_secs,
    });

    report::plan(&plan, inv, &fp_or_empty, cfg);
    if plan.run.is_empty() {
        println!("mock test: nothing to run; every member asked for passed at what it is now");
        return Some(ExitCode::SUCCESS);
    }

    let state = crate::build_dir::ensure_under_target(&cfg.mock_dir, &["mockspace-test-state"]);
    let tool_config = state.join("nextest.toml");
    if let Err(e) = std::fs::write(&tool_config, nextest::tool_config(&cfg.test, &plan.heavy)) {
        eprintln!("mock test: could not write {}: {e}", tool_config.display());
        return Some(ExitCode::FAILURE);
    }
    let junit = nextest::junit_path(&cfg.mock_dir, &inv.passed);
    // A report left by an earlier run must not be read as this one's.
    let _ = std::fs::remove_file(&junit);

    let packages: Vec<String> = plan.run.iter().map(|(n, _)| n.clone()).collect();
    let filter = nextest::filterset(inv.filter.as_deref(), &plan.skip);
    let status = nextest::command(&nextest::RunSpec {
        mock_dir:    &cfg.mock_dir,
        tool_config: &tool_config,
        packages:    &packages,
        passed:      &inv.passed,
        filterset:   filter.as_deref(),
        test_args:   &inv.test_args,
    })
    .status();
    let code = status.as_ref().ok().and_then(|s| s.code());

    let cases = std::fs::read_to_string(&junit)
        .map(|x| junit::parse(&x))
        .unwrap_or_default();
    if cases.is_empty() && code != Some(0) {
        eprintln!(
            "note: no report at {}, so this run's timings are not kept",
            junit.display()
        );
    }

    if let Some(fp) = &fp {
        history.record_cases(&flavour, &cases, &fp.full, cfg.test.heavy_after_secs);
        let complete = nextest::ran_to_the_end(code) && !inv.narrowed();
        let greens = select::greens(&plan, &cases, complete);
        let f = history.flavour_mut(&flavour);
        for (name, _) in &plan.run {
            if greens.contains(name) {
                f.green
                    .insert(name.clone(), select::green_record(name, fp, &graph));
            } else if complete {
                f.green.remove(name);
            }
        }
        if let Err(e) = history.save(&cfg.mock_dir) {
            eprintln!("note: could not keep this run's history: {e}");
        }
    }

    report::outcome(&plan, &cases, cfg.test.heavy_after_secs);
    Some(if status.is_ok_and(|s| s.success()) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// The members a run is asked about: those named with `-p`, else every member
/// for `--workspace`, else the default members, less any `--exclude`.
fn candidates(graph: &graph::Graph, inv: &Invocation) -> Vec<String> {
    let mut out: Vec<String> = if !inv.packages.is_empty() {
        inv.packages
            .iter()
            .filter(|p| graph.members.contains(*p))
            .cloned()
            .collect()
    } else if inv.workspace {
        graph.members.clone()
    } else {
        graph.default_members.clone()
    };
    out.retain(|p| !inv.exclude.contains(p));
    out.sort();
    out.dedup();
    out
}
