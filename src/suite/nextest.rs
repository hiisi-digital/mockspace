//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! How a plan becomes a `cargo nextest run`.
//!
//! The settings reach nextest as a tool config file, written fresh for every
//! run under the mock directory's `target/` and passed with
//! `--tool-config-file mockspace:<path>`. That is nextest's own mechanism for a
//! tool wrapping it: the tool's file sits below the repository's
//! `.config/nextest.toml`, so anything a repository configures for itself
//! still wins, and above nextest's defaults.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::select::Skip;
use super::settings::TestSettings;

/// The group the heavy tests run in. Nextest requires a group a tool defines
/// to be named `@tool:<tool>:<name>`.
pub const HEAVY_GROUP: &str = "@tool:mockspace:heavy";

/// Whether `cargo nextest` answers here.
pub fn installed(mock_dir: &Path) -> bool {
    crate::entry::cargo_gate::cargo(mock_dir, &["nextest", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The tool config for one run.
pub fn tool_config(s: &TestSettings, heavy: &[(String, String)], junit: &Path) -> String {
    // The slow period is how often nextest reports a test still running; the
    // timeout is that period times a whole count, so the period is shrunk to
    // fit a timeout shorter than a minute.
    let period = s.timeout_secs.min(60);
    let periods = s.timeout_secs.div_ceil(period);
    let mut t = String::new();
    t.push_str(
        "# Written by `cargo mock test` for each run, from `[test]` in mockspace.toml\n\
         # and the timings of earlier runs. Edits here are lost; the repository's own\n\
         # .config/nextest.toml wins over anything set here.\n\n",
    );
    let _ = writeln!(t, "[test-groups.\"{HEAVY_GROUP}\"]");
    let _ = writeln!(t, "max-threads = {}\n", s.heavy_threads);
    t.push_str("[profile.default]\n");
    let _ = writeln!(
        t,
        "slow-timeout = {{ period = \"{period}s\", terminate-after = {periods} }}"
    );
    t.push_str("fail-fast = false\n\n");
    // Quoted and escaped as TOML, whatever characters the path holds.
    let _ = writeln!(
        t,
        "[profile.default.junit]\npath = {}",
        toml_edit::Value::from(junit.to_string_lossy().as_ref())
    );
    for filter in heavy_filters(heavy) {
        t.push_str("\n[[profile.default.overrides]]\n");
        let _ = writeln!(t, "filter = '{filter}'");
        let _ = writeln!(t, "test-group = \"{HEAVY_GROUP}\"");
    }
    t
}

/// The cargo profile a run builds, which is where a member's opt level goes:
/// `release` for `--release`, the profile named by `profile_flag`, else `dev`.
/// The flag is `--cargo-profile` for nextest and `--profile` for `cargo test`.
pub fn build_profile(passed: &[String], profile_flag: &str) -> String {
    let mut it = passed.iter();
    while let Some(a) = it.next() {
        if a == "--release" || a == "-r" {
            return "release".to_string();
        }
        if a == profile_flag {
            if let Some(p) = it.next() {
                return p.clone();
            }
        } else if let Some(p) = a
            .strip_prefix(profile_flag)
            .and_then(|r| r.strip_prefix('='))
        {
            return p.to_string();
        }
    }
    "dev".to_string()
}

/// The longest filterset one override is given, in bytes. Nextest 0.9.148 refuses
/// an expression of tens of kilobytes (measured: 76 KB naming 653 tests), so the
/// heavy tests are split into several overrides, each of one binary, that stay
/// well under it.
const OVERRIDE_FILTER_BYTES: usize = 4000;

/// The filtersets that together name exactly these tests, one binary and a bounded
/// run of its tests each. Every override puts its tests in the same group, so the
/// split changes nothing about how many run at a time.
fn heavy_filters(heavy: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < heavy.len() {
        let bin = &heavy[i].0;
        let head = format!("binary_id(={bin}) & (");
        let mut names: Vec<String> = Vec::new();
        let mut len = head.len() + 1;
        while i < heavy.len() && &heavy[i].0 == bin {
            let one = format!("test(={})", heavy[i].1);
            // The first name of a filter always goes in, however long it is.
            if !names.is_empty() && len + one.len() + 3 > OVERRIDE_FILTER_BYTES {
                break;
            }
            len += one.len() + 3;
            names.push(one);
            i += 1;
        }
        out.push(format!("{head}{})", names.join(" | ")));
    }
    out
}

/// A filterset matching exactly these tests.
fn any_of<'a>(tests: impl IntoIterator<Item = &'a (String, String)>) -> String {
    tests
        .into_iter()
        .map(|(bin, name)| format!("(binary_id(={bin}) & test(={name}))"))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The `-E` for a run: the caller's own filter, less the tests skipped and the
/// binaries left out whole.
pub fn filterset(
    caller: Option<&str>,
    skip: &[Skip],
    reuse: &[String],
    rerun: &std::collections::BTreeMap<String, Vec<String>>,
) -> Option<String> {
    let skipped: Vec<(String, String)> = skip
        .iter()
        .map(|s| (s.binary.clone(), s.name.clone()))
        .collect();
    let mut left_out: Vec<String> = Vec::new();
    if !skipped.is_empty() {
        left_out.push(any_of(&skipped));
    }
    left_out.extend(reuse.iter().map(|b| format!("binary_id(={b})")));
    // A binary narrowed to its failed tests leaves every other test of it out.
    left_out.extend(rerun.iter().map(|(bin, names)| {
        let names: Vec<String> = names.iter().map(|n| format!("test(={n})")).collect();
        format!("(binary_id(={bin}) & not ({}))", names.join(" | "))
    }));
    let not_skipped = (!left_out.is_empty()).then(|| format!("not ({})", left_out.join(" | ")));
    match (caller, not_skipped) {
        (None, None) => None,
        (Some(c), None) => Some(c.to_string()),
        (None, Some(n)) => Some(n),
        (Some(c), Some(n)) => Some(format!("({c}) & {n}")),
    }
}

/// Where nextest writes the report for a run: a file of ours beside the tool
/// config, named in that config by its absolute path.
///
/// Not left to nextest's own default. Its store directory is `target/nextest`
/// under the workspace and not under `CARGO_TARGET_DIR` or `--target-dir`
/// (nextest 0.9.148, measured), so a report looked for under the target
/// directory the build chose was never there on a machine that shares one, and
/// the run recorded nothing: no timings and no binary that passed. An absolute
/// path in the config is where it goes whatever else is set.
pub fn junit_path(state: &Path) -> PathBuf {
    state.join("junit.xml")
}

pub struct RunSpec<'a> {
    pub mock_dir:     &'a Path,
    pub tool_config:  &'a Path,
    /// The generated cargo config, where a member has an opt level.
    pub cargo_config: Option<&'a Path>,
    pub packages:     &'a [String],
    pub passed:       &'a [String],
    pub filterset:    Option<&'a str>,
    pub test_args:    &'a [String],
}

pub fn command(r: &RunSpec) -> Command {
    let mut cmd = crate::entry::cargo_gate::cargo(r.mock_dir, &["nextest", "run"]);
    cmd.arg("--tool-config-file")
        .arg(format!("mockspace:{}", r.tool_config.display()))
        .arg("--no-fail-fast")
        // A member whose every test is skipped as cached runs nothing, and that
        // is a result rather than an error.
        .arg("--no-tests=warn");
    for p in r.packages {
        cmd.arg("-p").arg(p);
    }
    // Before the caller's own flags, so a `--config` of theirs comes later and
    // wins.
    if let Some(c) = r.cargo_config {
        cmd.arg("--config").arg(c);
    }
    // The tests only: without a target of its own in the flags, nextest builds and
    // links every example of every member too, to run none of them. Examples are
    // a step of their own, not part of each test run.
    if !selects_targets(r.passed) {
        cmd.arg("--tests");
    }
    cmd.args(r.passed);
    if let Some(f) = r.filterset {
        cmd.arg("-E").arg(f);
    }
    if !r.test_args.is_empty() {
        cmd.arg("--").args(r.test_args);
    }
    cmd
}

/// Whether the caller's own flags choose which cargo targets are built.
pub fn selects_targets(passed: &[String]) -> bool {
    const TARGETS: &[&str] = &[
        "--lib", "--bins", "--bin", "--tests", "--test", "--examples", "--example", "--benches",
        "--bench", "--all-targets",
    ];
    passed
        .iter()
        .any(|a| TARGETS.contains(&a.split('=').next().unwrap_or_default()))
}

/// Nextest's exit codes that mean every selected test ran: success, and the
/// run finishing with failures. Anything else, a build failure, a setup
/// script, an interrupt, stopped it before the end.
pub fn ran_to_the_end(code: Option<i32>) -> bool {
    // 100 is `TEST_RUN_FAILED` in nextest's `NextestExitCode`.
    matches!(code, Some(0) | Some(100))
}

#[cfg(test)]
#[path = "nextest_tests.rs"]
mod tests;
