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
/// Round 1 stub: returns an empty [`EnvMeta`]. Round 3 fills in real
/// collection using subprocesses (`sysctl`, `uname`, `rustc`, `git`)
/// and `mockspace_bench_core::counter::counter_frequency()`.
pub fn collect_env_meta() -> EnvMeta {
    EnvMeta::default()
}
