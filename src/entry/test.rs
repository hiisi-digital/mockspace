//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test`: run the tests of every tree mockspace owns.
//!
//! **The problem is wider than benches and was solved only for benches.**
//! Everything mockspace compiles that is not a member of the consumer's mock
//! workspace is invisible to `cargo test` run there, and mockspace compiles
//! three such trees: the bench crates, the tool crates, and the generated lint
//! cdylib. A `cargo test` in `mock/` reaches the members and nothing else, so a
//! repository whose `members` list is empty runs no tests at all while
//! appearing to.
//!
//! `mock bench test` closed one third of that. The consequence of leaving the
//! other two open is not abstract: a consumer repository with six tools and
//! four lints had nowhere to put a test for any of them, and grew a directory
//! of loose scripts in another language instead, outside the workspace, run by
//! whoever remembered it existed.
//!
//! So this runs all four, reports them separately, and fails if any fails.
//!
//! **The lint tree is the one that needed no new mechanism.** The generated
//! cdylib already `#[path]`-includes every `mock/lints/*.rs`, so a
//! `#[cfg(test)] mod tests` in a lint file compiles into it and `cargo test`
//! in the generated directory runs it. What was missing was anything that ran
//! that command.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::config::Config;

/// One tree, its label, and where its manifest lives.
struct Tree {
    what:    &'static str,
    dir:     PathBuf,
    /// Why this tree is not reached by a plain `cargo test` in `mock/`, so a
    /// reader of the output knows what each row is for.
    because: &'static str,
}

pub fn run(cfg: &Config, args: &[&str]) -> ExitCode {
    let mut trees: Vec<Tree> = Vec::new();

    // The workspace itself, but only when it has a member.
    //
    // A virtual manifest with an empty `members` does not run zero tests, it
    // ERRORS: "the manifest is virtual, and the workspace has no members". So a
    // repository that keeps its crates elsewhere gets a hard failure from the
    // one tree a plain `cargo test` was supposed to cover, and any caller that
    // reports the result honestly reports a failure caused by nothing being
    // there. Skipped with a note instead.
    let ws = cfg.mock_dir.join("Cargo.toml");
    if ws.exists() {
        // Asked of `cargo_gate`, which parses the manifest with `toml_edit`
        // rather than scanning it for substrings, and which the readiness
        // report already consumes. A second implementation of this question was
        // wrong on two real manifests: a `[package]` beside an empty `members`
        // reported as memberless when `cargo test` there passes, and a
        // commented-out `members` line read as live.
        //
        // The first version of this file justified the hand-rolled scan as
        // needing no dependency. `toml_edit` is a direct dependency of this
        // crate, so that was not true either.
        if crate::entry::cargo_gate::is_memberless_virtual_workspace(&cfg.mock_dir) {
            println!(
                "note: {} declares no workspace member, so there is nothing for a plain\n      \
                 cargo test to reach. The trees below are the ones that matter here.",
                ws.display()
            );
        } else {
            trees.push(Tree {
                what:    "workspace members",
                dir:     cfg.mock_dir.clone(),
                because: "reached by a plain cargo test",
            });
        }
    }

    // Tool and bench crates sit INSIDE the workspace directory without being
    // members, which cargo refuses outright: "current package believes it's in
    // a workspace when it's not". The generated lint crate already solves this
    // for itself with a leading empty `[workspace]` table, making it its own
    // root, and a tool crate needs the same one line.
    //
    // Reported here as the one-line fix rather than passed through as cargo's
    // message, which suggests adding the crate to `members` and that is the
    // wrong direction: membership would put a cdylib into the consumer's
    // dependency graph.
    let mut orphaned: Vec<PathBuf> = Vec::new();
    for dir in crate_dirs(&cfg.mock_dir.join("tools")) {
        if is_orphaned(&dir, &cfg.mock_dir) {
            orphaned.push(dir);
            continue;
        }
        trees.push(Tree {
            what: "tool",
            dir,
            because: "compiled as a path dependency of the lint cdylib, never a member",
        });
    }

    // Benches are NOT walked here. `mock bench test` already runs them, and it
    // does the one thing a directory walk cannot: a freshly `mock bench init`ed
    // tree has no manifest anywhere, because the arm manifests are generated on
    // demand under `target/`. So the walk finds nothing on the canonical layout
    // and reports it as nothing to run, which is the exact failure `bench.rs`
    // records having already fixed once.
    let bench_dir = cfg.mock_dir.join("benches");
    let has_benches = bench_dir.exists()
        && std::fs::read_dir(&bench_dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false);

    // The generated lint crate, which exists only after something generates it.
    // Not generated here: generating is the lint path's job and doing it from
    // the test path would mean two places that know how.
    let lints_gen = crate::build_dir::ensure_under_target(&cfg.mock_dir, &["mockspace-lints"]);
    if lints_gen.join("Cargo.toml").exists() {
        trees.push(Tree {
            what:    "lints",
            dir:     lints_gen,
            because: "generated under target/, its own workspace, never a member",
        });
    } else {
        eprintln!(
            "note: the lint crate has not been generated, so `mock/lints/*.rs` tests are \
             not in this run. `mock check` generates it."
        );
    }

    if !orphaned.is_empty() {
        eprintln!(
            "\nmock test: {} crate(s) cannot be tested where they sit. Each is inside the\n\
             mock workspace directory and is not a member, which cargo refuses. Add an\n\
             empty `[workspace]` table at the top of each manifest, the way the generated\n\
             lint crate does, and it becomes its own root:\n",
            orphaned.len()
        );
        for d in &orphaned {
            eprintln!("    {}/Cargo.toml", d.display());
        }
        eprintln!();
    }

    if trees.is_empty() && !has_benches {
        eprintln!(
            "mock test: no tree to test under {}",
            cfg.mock_dir.display()
        );
        return ExitCode::FAILURE;
    }

    let mut failed = Vec::new();

    if has_benches {
        println!("\n=== benches : {} ===", bench_dir.display());
        println!("    (compiled per arm with generated manifests, never a member)");
        if crate::bench::cmd(cfg, &["test"]) != ExitCode::SUCCESS {
            failed.push(format!("benches at {}", bench_dir.display()));
        }
    }

    for t in &trees {
        println!("\n=== {} : {} ===", t.what, t.dir.display());
        println!("    ({})", t.because);
        let ok = crate::entry::cargo_gate::cargo(&t.dir, &["test"])
            .args(args)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            failed.push(format!("{} at {}", t.what, t.dir.display()));
        }
    }

    println!();
    if failed.is_empty() {
        println!("mock test: {} tree(s) green", trees.len());
        ExitCode::SUCCESS
    } else {
        for f in &failed {
            eprintln!("mock test: FAILED {f}");
        }
        eprintln!(
            "mock test: {} of {} tree(s) failed",
            failed.len(),
            trees.len()
        );
        ExitCode::FAILURE
    }
}

/// Every directory under `root` holding a `Cargo.toml`, one level down.
///
/// One level rather than a walk, because both `tools/` and `benches/` are flat
/// by convention and a walk would descend into each crate's own `target/` and
/// find the manifests cargo writes there.
fn crate_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("Cargo.toml").exists())
        .collect();
    out.sort();
    out
}

/// Whether a crate sits inside a workspace directory without being a member and
/// without declaring itself a root.
fn is_orphaned(crate_dir: &Path, mock_dir: &Path) -> bool {
    if !crate_dir.starts_with(mock_dir) {
        return false;
    }
    let own = std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap_or_default();
    if own.contains("[workspace]") {
        return false;
    }
    let ws = std::fs::read_to_string(mock_dir.join("Cargo.toml")).unwrap_or_default();
    let Ok(rel) = crate_dir.strip_prefix(mock_dir) else {
        return false;
    };
    !ws.contains(&format!("\"{}\"", rel.display()))
}
