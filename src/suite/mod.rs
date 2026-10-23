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
//! **Below the member, it is a test binary that is selected** ([`units`]): an
//! integration test whose own files and library have not moved since it last ran
//! whole and passed is left out of a member that runs, which is what makes a
//! change confined to one `tests/<name>.rs` run that binary and a failed member
//! rerun only the binary that failed.
//!
//! **What a run leaves behind** is the history the next one selects from. A
//! member is recorded green at its fingerprint when its whole suite passed; each
//! test binary that ran whole and passed is recorded at its own fingerprint;
//! each slow test's time and result is recorded whatever it did.

pub mod args;
mod cargo_config;
pub mod debug;
pub mod failed;
pub mod fingerprint;
pub mod graph;
pub mod history;
pub mod history_guard;
pub mod history_merge;
pub mod junit;
pub mod live;
pub mod nextest;
mod report;
pub mod select;
pub mod settings;
pub mod units;

use std::path::{Path, PathBuf};
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

    let cargo_config_text = opt_level_config(
        cfg,
        Some(&graph.members),
        &nextest::build_profile(&inv.passed, "--cargo-profile"),
    );

    // A fingerprint that cannot be taken, outside git for one, runs everything:
    // the selection is an economy, never a reason to test less than asked.
    let fp = match fingerprint::compute(
        &cfg.repo_root,
        &cfg.mock_dir,
        &graph,
        cargo_config_text.as_deref(),
    ) {
        Ok(fp) => Some(fp),
        Err(e) => {
            eprintln!("note: {e}; every member asked for runs, and nothing is cached");
            None
        },
    };
    let fp_or_empty = fp.clone().unwrap_or_default();
    let mut history = history::History::load(&cfg.mock_dir);
    let flavour = inv.flavour();
    let mut plan = select::plan(&Inputs {
        mode:       if fp.is_some() { inv.mode } else { Mode::Full },
        cache:      inv.cache && fp.is_some(),
        candidates: &candidates,
        fp:         &fp_or_empty,
        graph:      &graph,
        history:    &history,
        flavour:    &flavour,
        threshold:  cfg.test.heavy_after_secs,
    });

    // A member's opt level and the debug info of each crate are cargo config,
    // written before the listing, which builds, so the run builds the same way.
    // Nothing to run writes nothing. The debug info is no input of the
    // fingerprint above: a test passes or fails the same with or without it.
    let state = crate::build_dir::ensure_under_target(&cfg.mock_dir, &["mockspace-test-state"]);
    let run_members: Vec<String> = plan.run.iter().map(|(n, _)| n.clone()).collect();
    let mut failed = failed::Failed::load(&state);
    let profile = nextest::build_profile(&inv.passed, "--cargo-profile");
    let symbols = debug::decide(&cfg.test, &debug::Basis {
        ask:      &inv.debug,
        packages: &inv.packages,
        members:  Some(&graph.members),
        running:  Some(&run_members),
        failed:   &failed.of(&flavour),
    });
    let cargo_config = if plan.run.is_empty() {
        None
    } else {
        match cargo_config::render(&cfg.test, &profile, Some(&graph.members), Some(&symbols)) {
            Some(text) => {
                match write_cargo_config(&state, &text) {
                    Ok(path) => Some(path),
                    Err(e) => {
                        eprintln!("mock test: {e}");
                        return Some(ExitCode::FAILURE);
                    },
                }
            },
            None => None,
        }
    };

    // What the history remembers may be gone: a deleted binary named in the
    // generated config stops nextest before any test runs. Listing is how the
    // existing tests are known, and it builds, so it happens only when the plan
    // runs something; where it cannot answer, nothing is dropped.
    let live = if run_members.is_empty() {
        None
    } else {
        live::list(
            &cfg.mock_dir,
            &run_members,
            &inv.passed,
            cargo_config.as_deref(),
        )
    };
    if let Some(live) = &live {
        plan.retain_live(live);
    }

    report::plan(&plan, inv, &fp_or_empty, cfg);
    if !plan.run.is_empty() {
        debug::report(&cfg.test, &cfg.mock_dir, &profile, &symbols, &inv.passed);
    }
    if plan.run.is_empty() {
        println!("mock test: nothing to run; every member asked for passed at what it is now");
        return Some(ExitCode::SUCCESS);
    }

    let junit = nextest::junit_path(&state);
    let tool_config = state.join("nextest.toml");
    if let Err(e) = std::fs::write(
        &tool_config,
        nextest::tool_config(&cfg.test, &plan.heavy, &junit),
    ) {
        eprintln!("mock test: could not write {}: {e}", tool_config.display());
        return Some(ExitCode::FAILURE);
    }
    // A report left by an earlier run must not be read as this one's.
    let _ = std::fs::remove_file(&junit);

    let packages: Vec<String> = plan.run.iter().map(|(n, _)| n.clone()).collect();
    let filter = nextest::filterset(inv.filter.as_deref(), &plan.skip, &plan.reuse);
    let status = nextest::command(&nextest::RunSpec {
        mock_dir:     &cfg.mock_dir,
        tool_config:  &tool_config,
        cargo_config: cargo_config.as_deref(),
        packages:     &packages,
        passed:       &inv.passed,
        filterset:    filter.as_deref(),
        test_args:    &inv.test_args,
    })
    .status();
    let code = status.as_ref().ok().and_then(|s| s.code());

    let cases = std::fs::read_to_string(&junit)
        .map(|x| junit::parse(&x))
        .unwrap_or_default();
    // What failed is what the next run builds with symbols, recorded whether or
    // not a fingerprint could be taken.
    failed.settle(
        &flavour,
        &run_members,
        &cases,
        nextest::ran_to_the_end(code) && !inv.narrowed(),
    );
    if let Err(e) = failed.save(&state) {
        eprintln!("note: could not keep which members failed: {e}");
    }
    if cases.is_empty() && code != Some(0) {
        eprintln!(
            "note: no report at {}, so this run's timings are not kept",
            junit.display()
        );
    }

    if let Some(fp) = &fp {
        // A test's pass stands at its binary's fingerprint.
        let at: std::collections::BTreeMap<String, String> = cases
            .iter()
            .filter_map(|c| {
                fp.of_binary(&c.binary)
                    .map(|a| (c.binary.clone(), a.clone()))
            })
            .collect();
        history.record_cases(&flavour, &cases, &at, cfg.test.heavy_after_secs);
        if let Some(live) = &live {
            history.prune(&flavour, &run_members, live);
        }
        let complete = nextest::ran_to_the_end(code) && !inv.narrowed();
        let greens = select::greens(&plan, &cases, complete);
        let mut binaries = select::binary_outcomes(&plan, &cases, fp, complete);
        if let Some(live) = &live {
            binaries
                .passed
                .extend(select::idle_binaries(&plan, live, fp, complete));
        }
        let f = history.flavour_mut(&flavour);
        binaries.apply(f);
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

/// The cargo config for the members tree when it runs under plain `cargo test`
/// (`--plain`, no nextest, or a workspace that could not be read), so the opt
/// levels and the debug info hold however the run is taken. `None` when it
/// would say nothing.
///
/// What runs is not known here and no report is read back, so symbols go to the
/// crates named and to the ones the last nextest run recorded as failed; a plain
/// run records no failure of its own.
pub fn plain_cargo_config(cfg: &Config, inv: &Invocation) -> Result<Option<PathBuf>, String> {
    let graph = graph::Graph::read(&cfg.mock_dir).ok();
    let members = graph.as_ref().map(|g| g.members.as_slice());
    let profile = nextest::build_profile(&inv.passed, "--profile");
    let state = crate::build_dir::ensure_under_target(&cfg.mock_dir, &["mockspace-test-state"]);
    note_unknown_opt_levels(cfg, members);
    let symbols = debug::decide(&cfg.test, &debug::Basis {
        ask: &inv.debug,
        packages: &inv.packages,
        members,
        running: None,
        failed: &failed::Failed::load(&state).of(&inv.flavour()),
    });
    debug::report(&cfg.test, &cfg.mock_dir, &profile, &symbols, &inv.passed);
    match cargo_config::render(&cfg.test, &profile, members, Some(&symbols)) {
        Some(text) => write_cargo_config(&state, &text).map(Some),
        None => Ok(None),
    }
}

/// The cargo config text carrying `[test.opt_level]` for a run, with a note for
/// each entry that names no member. `members` is `None` where the workspace
/// could not be read. This is the text a fingerprint is taken over, so it
/// carries no debug info.
fn opt_level_config(cfg: &Config, members: Option<&[String]>, profile: &str) -> Option<String> {
    note_unknown_opt_levels(cfg, members);
    cargo_config::render(&cfg.test, profile, members, None)
}

fn note_unknown_opt_levels(cfg: &Config, members: Option<&[String]>) {
    if let Some(members) = members {
        for m in cfg.test.opt_level.keys().filter(|m| !members.contains(*m)) {
            eprintln!(
                "note: [test.opt_level] names `{m}`, which is not a member of this workspace"
            );
        }
    }
}

/// Writes the cargo config under `state`, where the run passes it with
/// `--config`.
fn write_cargo_config(state: &Path, text: &str) -> Result<PathBuf, String> {
    let path = state.join("cargo.toml");
    std::fs::write(&path, text)
        .map(|()| path.clone())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
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
