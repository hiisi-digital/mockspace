//! Optional hardware performance counter support.
//!
//! macOS: uses `kpc_*` from IOKit (requires counters enabled).
//! Linux: uses `perf_event_open` (stub at this round).
//!
//! Gated behind the `perf-counters` feature on
//! `mockspace-bench-harness`. When disabled, all reads return zeros
//! and [`setup`] is a no-op. Cross-platform consumers can call
//! [`available`] to detect at runtime.
//!
//! To enable on macOS:
//!
//! ```bash
//! sudo sysctl kern.kpc.counting=1
//! cargo build --features mockspace-bench-harness/perf-counters
//! ```

/// Hardware counter snapshot. Fields are zero when unavailable.
#[derive(Clone, Copy, Default, Debug)]
pub struct PerfSnapshot {
    pub instructions: u64,
    pub cycles: u64,
    pub cache_misses: u64,
    pub branch_misses: u64,
}

impl PerfSnapshot {
    /// Compute `self - start`, saturating at zero per field.
    pub fn delta(&self, start: &PerfSnapshot) -> PerfSnapshot {
        PerfSnapshot {
            instructions: self.instructions.saturating_sub(start.instructions),
            cycles: self.cycles.saturating_sub(start.cycles),
            cache_misses: self.cache_misses.saturating_sub(start.cache_misses),
            branch_misses: self.branch_misses.saturating_sub(start.branch_misses),
        }
    }
}

/// Set up performance counters for the current thread. No-op if not
/// supported or not enabled.
pub fn setup() {
    #[cfg(all(feature = "perf-counters", target_os = "macos"))]
    macos::setup();

    #[cfg(all(feature = "perf-counters", target_os = "linux"))]
    linux::setup();
}

/// Read current performance counter values.
#[inline(always)]
pub fn read() -> PerfSnapshot {
    #[cfg(all(feature = "perf-counters", target_os = "macos"))]
    return macos::read();

    #[cfg(all(feature = "perf-counters", target_os = "linux"))]
    return linux::read();

    #[cfg(not(feature = "perf-counters"))]
    PerfSnapshot::default()
}

/// Whether perf counters are available and active. With the
/// `perf-counters` feature off, always returns `false`.
pub fn available() -> bool {
    #[cfg(feature = "perf-counters")]
    {
        let s = read();
        s.instructions > 0 || s.cycles > 0
    }
    #[cfg(not(feature = "perf-counters"))]
    false
}

// ── macOS: kpc_* from IOKit ──

#[cfg(all(feature = "perf-counters", target_os = "macos"))]
mod macos {
    use super::PerfSnapshot;
    use std::os::raw::c_int;

    extern "C" {
        fn kpc_set_counting(classes: u32) -> c_int;
        fn kpc_set_thread_counting(classes: u32) -> c_int;
        fn kpc_get_thread_counters(tid: u32, buf_count: u32, buf: *mut u64) -> c_int;
    }

    const KPC_CLASS_FIXED: u32 = 1;
    const KPC_CLASS_CONFIGURABLE: u32 = 2;
    const MAX_COUNTERS: usize = 16;

    pub fn setup() {
        unsafe {
            kpc_set_counting(KPC_CLASS_FIXED | KPC_CLASS_CONFIGURABLE);
            kpc_set_thread_counting(KPC_CLASS_FIXED | KPC_CLASS_CONFIGURABLE);
        }
    }

    pub fn read() -> PerfSnapshot {
        let mut buf = [0u64; MAX_COUNTERS];
        unsafe {
            kpc_get_thread_counters(0, MAX_COUNTERS as u32, buf.as_mut_ptr());
        }
        // Apple Silicon fixed counters layout:
        // [0] = instructions retired
        // [1] = cycles
        // Configurable counters depend on PMU config.
        PerfSnapshot {
            instructions: buf[0],
            cycles: buf[1],
            cache_misses: buf.get(2).copied().unwrap_or(0),
            branch_misses: buf.get(3).copied().unwrap_or(0),
        }
    }
}

// ── Linux: perf_event_open ──
//
// Round 7 ships the cfg-gated module + signature. Wire the actual
// perf_event_open syscalls when a Linux consumer needs them; the API
// surface (PerfSnapshot, setup, read) is stable.

#[cfg(all(feature = "perf-counters", target_os = "linux"))]
mod linux {
    use super::PerfSnapshot;

    pub fn setup() {
        // perf_event_open setup would go here.
        // Each counter needs a separate fd.
    }

    pub fn read() -> PerfSnapshot {
        // Read from perf fds would go here.
        PerfSnapshot::default()
    }
}
