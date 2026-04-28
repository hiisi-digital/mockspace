//! Environment metadata captured once per harness run.
//!
//! Recorded into the CSV cache alongside each sample so historical
//! comparisons can isolate hardware / toolchain / commit drift.
//! Round 1 ships the type and a stub collector that returns empty
//! strings; Round 3 wires up the real `sysctl` / `/proc/cpuinfo` /
//! `rustc --version` / `git rev-parse` calls.

use serde::{Deserialize, Serialize};

/// Environment metadata recorded once per [`crate::run`] invocation.
///
/// All free-form strings are emitted verbatim into the cache; consumers
/// that want structured queries should parse on the read side.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnvMeta {
    /// CPU brand string (`sysctl machdep.cpu.brand_string` on macOS,
    /// `model name` from `/proc/cpuinfo` on Linux). Empty when
    /// collection fails.
    pub cpu: String,
    /// `uname -sr` output. Identifies kernel + version.
    pub os: String,
    /// `rustc --version` output. Pins the toolchain that built the
    /// variant cdylibs.
    pub rustc: String,
    /// Short git commit (`git rev-parse --short HEAD`) of the consumer
    /// repo at run start. Empty when not in a git tree.
    pub git_commit: String,
    /// Unix timestamp at run start (seconds since epoch).
    pub timestamp: u64,
    /// Hardware counter frequency in Hz; used by the harness to convert
    /// raw counter ticks into nanoseconds.
    pub counter_freq: u64,
}

/// Collect environment metadata for the current process.
///
/// Calls `sysctl` (macOS) or reads `/proc/cpuinfo` (Linux) for the CPU
/// brand, `uname -sr` for the OS string, `rustc --version` for the
/// toolchain pin, `git rev-parse --short HEAD` for the commit, the
/// system clock for `timestamp`, and
/// [`mockspace_bench_core::counter::counter_frequency`] for the
/// counter frequency. Any individual collection step that fails leaves
/// the corresponding field empty (no panic).
pub fn collect_env_meta() -> EnvMeta {
    EnvMeta {
        cpu: collect_cpu(),
        os: collect_os(),
        rustc: collect_rustc(),
        git_commit: collect_git_commit(),
        timestamp: collect_timestamp(),
        counter_freq: mockspace_bench_core::counter::counter_frequency(),
    }
}

#[cfg(target_os = "macos")]
fn collect_cpu() -> String {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    out.trim().to_string()
}

#[cfg(not(target_os = "macos"))]
fn collect_cpu() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split(':').nth(1))
                .map(|v| v.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".into())
}

fn collect_os() -> String {
    let out = std::process::Command::new("uname")
        .args(["-sr"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    out.trim().to_string()
}

fn collect_rustc() -> String {
    let out = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    out.trim().to_string()
}

fn collect_git_commit() -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    out.trim().to_string()
}

fn collect_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
