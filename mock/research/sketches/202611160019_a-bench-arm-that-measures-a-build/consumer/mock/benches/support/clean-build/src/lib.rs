//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What `build-from-clean` does on each call. The fixture workspace beside the bench is built with cargo into a
//! target directory of its own, which is removed before every build, so every build starts from nothing. The
//! harness times only [`build`]; [`clean`] before it and [`record`] after it are untimed.
//!
//! The target directory sits under the build directory the run inherits (`CARGO_TARGET_DIR`), in a directory of
//! its own, `m4-build-from-clean/target`, so removing it never touches the profile directories the harness built
//! its driver and arms into. Without that variable it is the fixture's own `target/`.
//!
//! Every call appends one line to `m4-build-from-clean/calls.tsv` beside that target directory: the worker's
//! process id, the call's number in that process, how many fixture crates the compiler reported as built fresh,
//! whether every fixture library was absent before the build and present after it, whether cargo said it waited
//! on a lock, and cargo's exit code. The hooks library copies the file into each cell's run directory.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// The fixture's crates, by their library names.
pub const CRATES: [&str; 4] = ["fx_base", "fx_left", "fx_right", "fx_top"];

/// The calls this process has made, for the record.
static CALLS: AtomicU64 = AtomicU64::new(0);

/// The fixture workspace, in the `build-from-clean` bench.
pub fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build-from-clean/fixture")
}

/// The directory this bench owns under the inherited build directory.
fn own_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(dir) => PathBuf::from(dir).join("m4-build-from-clean"),
        None => fixture_dir().join("target").join("m4-build-from-clean"),
    }
}

/// The fixture's target directory, removed before every build.
pub fn target_dir() -> PathBuf {
    own_dir().join("target")
}

/// The record of every call since the last reset.
pub fn log_path() -> PathBuf {
    own_dir().join("calls.tsv")
}

/// The library each fixture crate builds to in the dev profile.
fn artifacts() -> Vec<PathBuf> {
    CRATES
        .iter()
        .map(|name| target_dir().join("debug").join(format!("lib{name}.rlib")))
        .collect()
}

/// Untimed, before the build: the target directory removed. Answers whether every fixture library is then absent.
pub fn clean() -> bool {
    let target = target_dir();
    if target.exists() {
        std::fs::remove_dir_all(&target).unwrap_or_else(|e| panic!("{}: {e}", target.display()));
    }
    absent()
}

/// Whether every fixture library is absent, with nothing removed. The control arm `without_clean` asks only this,
/// so its builds after the first find the target directory as the build before left it.
pub fn absent() -> bool {
    artifacts().iter().all(|a| !a.exists())
}

/// Timed: the whole fixture workspace built in cargo's dev profile, offline, against its committed lockfile.
pub fn build() -> Output {
    Command::new("cargo")
        .current_dir(fixture_dir())
        .args(["build", "--workspace", "--offline", "--locked", "--message-format=json", "--target-dir"])
        .arg(target_dir())
        .output()
        .expect("cargo on the path")
}

/// Untimed, after the build: the crates the compiler reported as built fresh, counted from cargo's own messages,
/// with the record line appended. Answers with that count.
pub fn record(output: &Output, absent_before: bool) -> u64 {
    let said = String::from_utf8_lossy(&output.stdout);
    let compiled = said
        .lines()
        .filter(|line| line.contains("\"reason\":\"compiler-artifact\"") && line.contains("\"fresh\":false"))
        .filter(|line| CRATES.iter().any(|name| line.contains(&format!("\"name\":\"{name}\""))))
        .count() as u64;
    let present_after = artifacts().iter().all(|a| a.is_file());
    let waited = String::from_utf8_lossy(&output.stderr).contains("Blocking waiting for file lock");
    let call = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    let line = format!(
        "{}\t{call}\t{compiled}\t{absent_before}\t{present_after}\t{waited}\t{}\n",
        std::process::id(),
        output.status.code().map_or_else(|| "signal".to_string(), |c| c.to_string()),
    );
    std::fs::create_dir_all(own_dir()).unwrap_or_else(|e| panic!("{}: {e}", own_dir().display()));
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .unwrap_or_else(|e| panic!("{}: {e}", log_path().display()));
    if !output.status.success() {
        eprintln!("clean-build: cargo failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    compiled
}

/// The header of the record, as the hooks library writes it into a run directory.
pub const LOG_HEADER: &str = "pid\tcall\tcompiled\tabsent_before\tpresent_after\twaited_on_lock\texit\n";

/// Empty the record, before a run and after each cell has taken its copy.
pub fn reset_log() {
    let _ = std::fs::remove_file(log_path());
}

/// One call's record, read back.
pub struct Call {
    pub compiled:      u64,
    pub absent_before: bool,
    pub present_after: bool,
    pub waited:        bool,
    pub exit:          String,
}

/// Every call recorded since the last reset.
pub fn calls() -> Vec<Call> {
    std::fs::read_to_string(log_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            (f.len() == 7).then(|| Call {
                compiled:      f[2].parse().unwrap_or(0),
                absent_before: f[3] == "true",
                present_after: f[4] == "true",
                waited:        f[5] == "true",
                exit:          f[6].to_string(),
            })
        })
        .collect()
}
