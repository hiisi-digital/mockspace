//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The hooks of this tree. `on_init` empties the call record of `build-from-clean`, and `after_cell` copies it into
//! the cell's run directory as `calls.tsv` and refuses the cell when any call did not build every fixture crate from
//! nothing: a crate count other than the fixture's, a library present before the build or missing after it, cargo
//! waiting on a lock, or cargo failing.

use mockspace_bench_harness::driver::{AfterCell, CellVerdict, Hooks, InitContext, InitVerdict};

fn on_init(_: &InitContext<'_>) -> InitVerdict {
    clean_build::reset_log();
    InitVerdict::Proceed
}

fn after_cell(cell: &AfterCell<'_>) -> CellVerdict {
    let text = std::fs::read_to_string(clean_build::log_path()).unwrap_or_default();
    let calls = clean_build::calls();
    clean_build::reset_log();
    if let Err(e) = std::fs::write(cell.out_dir.join("calls.tsv"), format!("{}{text}", clean_build::LOG_HEADER)) {
        return CellVerdict::Fail(format!("calls.tsv was not written: {e}"));
    }
    let want = clean_build::CRATES.len() as u64;
    let bad: Vec<String> = calls
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.compiled != want || !c.absent_before || !c.present_after || c.waited || c.exit != "0"
        })
        .map(|(i, c)| {
            format!(
                "line {}: compiled {}, absent before {}, present after {}, waited {}, exit {}",
                i + 1,
                c.compiled,
                c.absent_before,
                c.present_after,
                c.waited,
                c.exit
            )
        })
        .collect();
    if calls.is_empty() {
        CellVerdict::Fail("no call was recorded".into())
    } else if bad.is_empty() {
        CellVerdict::Note(format!(
            "{} calls, each built all {want} fixture crates into an empty target directory",
            calls.len()
        ))
    } else {
        CellVerdict::Fail(format!("calls that did not build from clean: {}", bad.join("; ")))
    }
}

pub fn hooks() -> Hooks {
    Hooks {
        on_init: Some(on_init),
        after_cell: Some(after_cell),
        ..Hooks::default()
    }
}
