//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a run gives back is in the order a serial run gives, whatever its threads did.

use std::time::Duration;

use super::*;
use crate::{CrateLint, LintContext, WorkspaceLint};

/// A crate lint that takes longer the earlier its crate comes, so on many threads the units finish
/// in the reverse of the order their findings must come out in.
struct Slow;

impl Lint for Slow {
    fn name(&self) -> &'static str {
        "slow"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

impl CrateLint for Slow {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        let rank = u64::from(b'z' - ctx.crate_name.as_bytes()[0]);
        std::thread::sleep(Duration::from_millis(rank));
        let path = ctx.source_path().display().to_string();
        let lines = ctx.source().lines().count();
        vec![LintError::error(ctx.crate_name.to_string(), lines, "slow", path)]
    }
}

struct Across;

impl Lint for Across {
    fn name(&self) -> &'static str {
        "across"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }
}

impl WorkspaceLint for Across {
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        std::thread::sleep(Duration::from_millis(5));
        crates
            .iter()
            .map(|(name, ctx)| LintError::error((*name).to_string(), ctx.all_sources().len(), "across", "seen".into()))
            .collect()
    }
}

struct Whole;

impl Lint for Whole {
    fn name(&self) -> &'static str {
        "whole"
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn source_only(&self) -> bool {
        false
    }
}

impl RepoLint for Whole {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        let n = ctx.view().glob(&["crates/**/*.rs"]).len();
        vec![LintError::error("workspace".into(), n, "whole", "files".into())]
    }
}

fn pack() -> LintPack {
    LintPack {
        crate_lints: vec![Box::new(Slow)],
        workspace_lints: vec![Box::new(Across)],
        repo_lints: vec![Box::new(Whole)],
        ..LintPack::default()
    }
}

fn workspace() -> Workspace {
    let body = "pub mod m;\n\npub fn f() {\n    todo!()\n}\n";
    let module = "pub struct Undocumented;\n\npub fn g() { unimplemented!() }\n";
    Workspace::new("order", &[
        ("alpha", &[("src/lib.rs", body), ("src/m.rs", module)]),
        ("bravo", &[("src/lib.rs", body), ("src/m.rs", module), ("src/n.rs", "fn h() {}\n")]),
        ("charlie", &[("src/lib.rs", "//! docs\n"), ("src/m.rs", module)]),
        ("delta", &[("src/lib.rs", body), ("src/m.rs", "")]),
    ])
}

#[test]
fn findings_come_out_in_the_serial_order_at_any_thread_count() {
    let w = workspace();
    let pack = pack();
    let serial = shown(&w.run_with(&Reading::new(&w.dir.0).with_threads(1), None, None, Some(&pack)));
    assert!(serial.matches("lint_name: \"slow\"").count() >= 4, "every unit is in the output:\n{serial}");
    assert!(serial.contains("lint_name: \"no-todo\""), "the built-ins ran too:\n{serial}");

    for threads in [2, 4, 16] {
        let many = shown(&w.run_with(&Reading::new(&w.dir.0).with_threads(threads), None, None, Some(&pack)));
        assert_eq!(many, serial, "at {threads} threads");
    }
}

#[test]
fn replayed_and_judged_units_interleave_in_the_serial_order() {
    let w = workspace();
    let pack = pack();
    let cache = Mem::default();
    let threads = |n| Reading::new(&w.dir.0).with_threads(n);

    let cold = shown(&w.run_with(&threads(8), Some(&cache), None, Some(&pack)));
    let warm = shown(&w.run_with(&threads(8), Some(&cache), None, Some(&pack)));
    assert_eq!(warm, cold, "a run answered from the store gives what the judging gave");

    // One crate moves: its units are judged, the rest replayed, and the mix comes out in order.
    w.write("bravo", "src/n.rs", "fn h() { todo!() }\n");
    let mixed = shown(&w.run_with(&threads(8), Some(&cache), None, Some(&pack)));
    let full = shown(&w.run_with(&threads(1), None, None, Some(&pack)));
    assert_eq!(mixed, full);
    assert_ne!(mixed, cold, "the control: the edit is visible in the output");
}
