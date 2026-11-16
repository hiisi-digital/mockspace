//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the end-to-end bench tests share: writing a tree, the arm they
//! measure, freezing the snapshots, and reading back the runs.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// The arm: the starter's hand-written export shape, no macro, so
/// the test exercises the generated manifest rather than bench-macro.
pub const ARM_LIB: &str = r#"
use mockspace_bench_core::{abi_hash, timed, FfiBenchCall};

fn plusone_impl(input: &u64, output: &mut u64) {
    *output = input.wrapping_add(1);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bench_entry(
    input_ptr: *const u8,
    output_ptr: *mut u8,
    _n: usize,
) -> FfiBenchCall {
    let input = unsafe { &*(input_ptr as *const u64) };
    let output = unsafe { &mut *(output_ptr as *mut u64) };
    timed! {
        run { plusone_impl(input, output); }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_name() -> *const u8 {
    b"plusone\0".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_output_size(_n: usize) -> usize {
    OUTPUT_SIZE
}
"#;

/// Freeze every snapshot of the tree, as its author does once the arms are
/// written, so the run's contract check passes.
pub fn freeze_all(cfg: &mockspace::config::Config) {
    let code = mockspace::bench::cmd(cfg, &["freeze"]);
    assert_eq!(format!("{code:?}"), format!("{:?}", std::process::ExitCode::SUCCESS), "freeze");
}

/// Every run directory of a bench's cells, oldest first.
pub fn runs_of(bench_dir: &Path, bench: &str) -> Vec<PathBuf> {
    mockspace_bench_harness::layout::runs(&bench_dir.join("results"))
        .into_iter()
        .filter(|r| r.bench == bench)
        .map(|r| r.dir)
        .collect()
}
