//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test-units`: how each member's tests divide into binaries, read from
//! the same layout `mock test` selects by.
//!
//! A report and nothing else. It names, per member, each integration-test
//! binary with the files it is built from, which binaries are wide (rebuilt on
//! every claimed file of the member), and the Rust files under `tests/` that no
//! binary claims, which are shared and so move every binary. It changes no file
//! and always exits zero once it could read the workspace.

use std::fmt::Write as _;
use std::process::ExitCode;

use super::fingerprint::{self, Fingerprints};
use super::graph::Graph;
use crate::config::Config;

/// Run the report for the members named in `args`, or every member.
pub fn run(cfg: &Config, args: &[String]) -> ExitCode {
    let graph = match Graph::read(&cfg.mock_dir) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("mock test-units: {e}");
            return ExitCode::FAILURE;
        },
    };
    let fp = match fingerprint::compute(&cfg.repo_root, &cfg.mock_dir, &graph, None) {
        Ok(fp) => fp,
        Err(e) => {
            eprintln!("mock test-units: {e}");
            return ExitCode::FAILURE;
        },
    };
    let unknown: Vec<&String> = args.iter().filter(|a| !graph.members.contains(a)).collect();
    if !unknown.is_empty() {
        eprintln!(
            "mock test-units: no member named {}; the members are {}",
            unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "),
            graph.members.join(", ")
        );
        return ExitCode::FAILURE;
    }
    let wanted: Vec<&String> = if args.is_empty() {
        graph.members.iter().collect()
    } else {
        args.iter().collect()
    };
    print!("{}", render(&fp, &wanted));
    ExitCode::SUCCESS
}

/// The report text for `members`, from a computed fingerprint.
pub fn render(fp: &Fingerprints, members: &[&String]) -> String {
    let mut out = String::new();
    for name in members {
        let Some(layout) = fp.layouts.get(name.as_str()) else {
            continue;
        };
        let _ = writeln!(out, "{name}");
        if layout.units.is_empty() {
            let _ = writeln!(
                out,
                "  no integration-test binary it can tell apart: every test reruns with any change"
            );
            continue;
        }
        for (target, unit) in &layout.units {
            let wide = if unit.wide {
                "  wide: reads the environment or file!(), or declares a module it cannot follow"
            } else {
                ""
            };
            let n = unit.files.len();
            let s = if n == 1 { "" } else { "s" };
            let _ = writeln!(out, "  {target}: {n} file{s}{wide}");
        }
        for f in fp.unclaimed.get(name.as_str()).into_iter().flatten() {
            let _ = writeln!(
                out,
                "  unclaimed: {} (no binary declares it, so it moves every binary)",
                f.display()
            );
        }
    }
    out
}

