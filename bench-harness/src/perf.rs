//! Optional hardware performance counters (Apple Silicon PMU via kperf).
//!
//! Wall-clock time says which variant is faster; the PMU says why (instructions
//! retired, cycles, and their ratio IPC; later, cache and branch misprediction
//! counts). For an interpreter-dispatch matrix that is the difference between
//! "threaded won" and "threaded won because it retired the same instructions with
//! far fewer branch mispredicts."
//!
//! Gated behind the `perf-counters` feature. When the feature is off, or the
//! process is not root, or the machine is not an Apple-Silicon Mac, every read
//! returns zeros and [`available`] returns false, so the bench degrades cleanly
//! to wall-clock only. The counters are read host-side (the harness brackets each
//! variant call), so there is NO change to the `FfiBenchCall` ABI and no variant
//! recompilation contract to migrate.
//!
//! Privilege and scope. The Apple-Silicon PMU is a privileged resource: arming it
//! requires root, so the bench must be run under `sudo` (there is no clean
//! in-process privilege raise on macOS; the `--perf-counters` flag documents the
//! requirement). [`setup`] takes exclusive control of the PMU
//! (`kpc_force_all_ctrs_set`) and [`teardown`] releases it; the caller MUST run
//! teardown on every exit path (including a panic or signal) so the PMU is not
//! left claimed. Only the two FIXED counters (cycles, instructions) are wired in
//! this cut; the CONFIGURABLE counters (cache / branch misses) need
//! per-microarchitecture event selectors whose correctness cannot be confirmed
//! without a privileged validation run, and a wrong selector would report a
//! plausible-but-wrong number, so they are left as a FIXME rather than shipped
//! unvalidated. `kperf` is a private framework; treat it as version-fragile.

/// Hardware counter snapshot. Fields are zero when unavailable or not yet wired.
#[derive(Clone, Copy, Default, Debug)]
pub struct PerfSnapshot {
    pub instructions:  u64,
    pub cycles:        u64,
    // FIXME: configurable counters. Wiring cache_misses / branch_misses needs the
    // Apple-Silicon (Firestorm/Icestorm) raw PMU event selectors programmed via
    // kpc_set_config, and a privileged validation run to confirm the selector maps
    // to the intended event (a wrong selector reports a plausible-but-wrong count,
    // which the bench-honesty rule forbids). Left zero until that joint validation.
    pub cache_misses:  u64,
    pub branch_misses: u64,
}

impl PerfSnapshot {
    /// Compute `self - start`, saturating at zero per field.
    pub fn delta(&self, start: &PerfSnapshot) -> PerfSnapshot {
        PerfSnapshot {
            instructions:  self.instructions.saturating_sub(start.instructions),
            cycles:        self.cycles.saturating_sub(start.cycles),
            cache_misses:  self.cache_misses.saturating_sub(start.cache_misses),
            branch_misses: self.branch_misses.saturating_sub(start.branch_misses),
        }
    }

    /// Instructions-per-cycle, or 0.0 when cycles is 0.
    pub fn ipc(&self) -> f64 {
        if self.cycles == 0 {
            0.0
        } else {
            self.instructions as f64 / self.cycles as f64
        }
    }
}

/// Arm the PMU for fixed-counter counting on this process. Returns true on
/// success (feature on, root, Apple Silicon, framework present, support probe
/// passes). Idempotent-safe to call once at bench start. Must be paired with
/// [`teardown`].
pub fn setup() -> bool {
    #[cfg(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64"))]
    {
        macos::setup()
    }
    #[cfg(not(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}

/// Release the PMU. Safe to call even if [`setup`] failed or was never called.
/// Run on EVERY exit path so the PMU is not left claimed for the next process.
pub fn teardown() {
    #[cfg(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64"))]
    {
        macos::teardown();
    }
}

/// Read the current thread's counters. Zeros when counting is not active.
#[inline]
pub fn read() -> PerfSnapshot {
    #[cfg(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64"))]
    {
        macos::read()
    }
    #[cfg(not(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64")))]
    {
        PerfSnapshot::default()
    }
}

/// Whether counting is active (setup succeeded). False with the feature off, not
/// root, or unsupported hardware, so the caller falls back to wall-clock only.
pub fn available() -> bool {
    #[cfg(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64"))]
    {
        macos::available()
    }
    #[cfg(not(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}

/// Diagnostic: read every counter slot the PMU exposes (raw, unlabelled), for the
/// privileged validation run that identifies which slots are cycles/instructions
/// against a known workload. Empty when unavailable.
pub fn read_all_raw() -> Vec<u64> {
    #[cfg(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64"))]
    {
        macos::read_all_raw()
    }
    #[cfg(not(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64")))]
    {
        Vec::new()
    }
}

// ── macOS Apple-Silicon kperf ──

#[cfg(all(feature = "perf-counters", target_os = "macos", target_arch = "aarch64"))]
mod macos {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    use super::PerfSnapshot;

    // kpc class masks (kperf.framework).
    const KPC_CLASS_FIXED_MASK: u32 = 1 << 0;
    const KPC_CLASS_CONFIGURABLE_MASK: u32 = 1 << 1;
    // We count fixed only in this cut.
    const COUNT_MASK: u32 = KPC_CLASS_FIXED_MASK;

    // Validated 2026-07-22 on M1 (Firestorm) via the privileged diagnostic: under
    // FIXED counting only slots 0 and 1 of the kpc_get_thread_counters buffer are
    // active, so the fixed counters sit at the START of the buffer (FIXED_OFFSET
    // = 0, below), NOT at the end. Against a 1e6-iteration debug loop slot 0 held
    // the smaller delta (~22.1M) and slot 1 the larger (~113.1M); the only
    // assignment consistent with unoptimized code's sub-1 IPC is slot 0 =
    // instructions, slot 1 = cycles (the reverse would imply IPC ~5.1, impossible
    // for a debug build). So:
    const FIXED_INSTRS_SUBINDEX: usize = 0;
    const FIXED_CYCLES_SUBINDEX: usize = 1;

    #[link(name = "kperf", kind = "framework")]
    unsafe extern "C" {
        fn kpc_get_counter_count(classes: u32) -> u32;
        fn kpc_set_counting(classes: u32) -> i32;
        fn kpc_set_thread_counting(classes: u32) -> i32;
        fn kpc_get_thread_counters(tid: u32, buf_count: u32, buf: *mut u64) -> i32;
        fn kpc_force_all_ctrs_set(val: i32) -> i32;
    }

    static ACTIVE: AtomicBool = AtomicBool::new(false);
    // total counters (all classes) and where the fixed block starts in the buffer.
    static TOTAL_COUNT: AtomicU32 = AtomicU32::new(0);
    static FIXED_OFFSET: AtomicU32 = AtomicU32::new(0);

    pub fn setup() -> bool {
        // total counters across fixed + configurable, and how many are fixed. The
        // fixed block is at the START of the buffer (FIXED_OFFSET = 0, set below,
        // per the M1 validation); n_fixed is used only for the sanity check that at
        // least the two fixed counters exist.
        let total = unsafe { kpc_get_counter_count(KPC_CLASS_FIXED_MASK | KPC_CLASS_CONFIGURABLE_MASK) };
        let n_fixed = unsafe { kpc_get_counter_count(KPC_CLASS_FIXED_MASK) };
        if total == 0 || n_fixed < 2 || total < n_fixed {
            // no PMU access (not root / unsupported / framework returned nothing).
            return false;
        }
        // take exclusive control of the PMU, then enable fixed counting for the
        // process and this thread. Any nonzero return means we lack privilege or
        // the PMU is otherwise unavailable; report unavailable and leave it clean.
        let claimed = unsafe { kpc_force_all_ctrs_set(1) };
        if claimed != 0 {
            return false;
        }
        if unsafe { kpc_set_counting(COUNT_MASK) } != 0
            || unsafe { kpc_set_thread_counting(COUNT_MASK) } != 0
        {
            unsafe { kpc_force_all_ctrs_set(0) };
            return false;
        }
        TOTAL_COUNT.store(total, Ordering::Relaxed);
        // Fixed counters are at the START of the buffer on Apple Silicon (slots
        // 0,1), confirmed by the privileged validation diagnostic (only slots 0,1
        // were active under FIXED counting). An earlier `total - n_fixed` guess
        // (fixed-at-end) read the inactive tail slots and reported zeros.
        FIXED_OFFSET.store(0, Ordering::Relaxed);
        ACTIVE.store(true, Ordering::Relaxed);
        true
    }

    pub fn teardown() {
        if ACTIVE.swap(false, Ordering::Relaxed) {
            unsafe {
                kpc_set_thread_counting(0);
                kpc_set_counting(0);
                kpc_force_all_ctrs_set(0);
            }
        }
    }

    pub fn available() -> bool {
        ACTIVE.load(Ordering::Relaxed)
    }

    // A stack buffer sized well above any real PMU (M1 exposes 10 counters), so
    // the per-call `read` needs no heap allocation.
    const MAX_COUNTERS: usize = 32;

    #[inline]
    pub fn read() -> PerfSnapshot {
        if !ACTIVE.load(Ordering::Relaxed) {
            return PerfSnapshot::default();
        }
        let total = (TOTAL_COUNT.load(Ordering::Relaxed) as usize).min(MAX_COUNTERS);
        let off = FIXED_OFFSET.load(Ordering::Relaxed) as usize;
        let mut buf = [0u64; MAX_COUNTERS];
        let rc = unsafe { kpc_get_thread_counters(0, total as u32, buf.as_mut_ptr()) };
        if rc != 0 {
            return PerfSnapshot::default();
        }
        PerfSnapshot {
            cycles: buf.get(off + FIXED_CYCLES_SUBINDEX).copied().unwrap_or(0),
            instructions: buf.get(off + FIXED_INSTRS_SUBINDEX).copied().unwrap_or(0),
            cache_misses: 0,
            branch_misses: 0,
        }
    }

    pub fn read_all_raw() -> Vec<u64> {
        if !ACTIVE.load(Ordering::Relaxed) {
            return Vec::new();
        }
        let total = TOTAL_COUNT.load(Ordering::Relaxed) as usize;
        let mut buf = vec![0u64; total.max(2)];
        let rc = unsafe { kpc_get_thread_counters(0, total as u32, buf.as_mut_ptr()) };
        if rc != 0 {
            return Vec::new();
        }
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_saturates_and_ipc_guards_zero() {
        let a = PerfSnapshot { instructions: 100, cycles: 50, cache_misses: 3, branch_misses: 1 };
        let b = PerfSnapshot { instructions: 250, cycles: 100, cache_misses: 5, branch_misses: 2 };
        let d = b.delta(&a);
        assert_eq!(d.instructions, 150);
        assert_eq!(d.cycles, 50);
        assert_eq!(d.ipc(), 3.0);
        // saturating: a smaller "later" reading never underflows.
        assert_eq!(a.delta(&b).instructions, 0);
        assert_eq!(PerfSnapshot::default().ipc(), 0.0);
    }

    /// The privileged validation. Ignored by default; run it deliberately under
    /// sudo with the feature on to confirm (or correct) the FIXED_*_SUBINDEX
    /// mapping in this module against a known workload:
    ///
    ///   sudo cargo test -p mockspace-bench-harness --features perf-counters \
    ///        perf_validate_counter_mapping -- --ignored --nocapture
    ///
    /// The slot whose delta is ~a few million (the loop's retired instructions)
    /// is the instructions counter; the larger is cycles. If the "current mapping
    /// reads" line already shows instructions ~= the loop size, the guesses are
    /// right; otherwise adjust FIXED_CYCLES_SUBINDEX / FIXED_INSTRS_SUBINDEX.
    #[test]
    #[ignore = "needs sudo + the perf-counters feature; see the doc comment for the command"]
    fn perf_validate_counter_mapping() {
        assert!(
            setup(),
            "PMU setup failed: need sudo, an Apple-Silicon Mac, and --features perf-counters"
        );
        let before = read_all_raw();
        let mut acc = 0u64;
        for i in 0 .. 1_000_000u64 {
            acc = acc.wrapping_add(std::hint::black_box(i));
        }
        std::hint::black_box(acc);
        let after = read_all_raw();
        let deltas: Vec<i128> = after
            .iter()
            .zip(before.iter())
            .map(|(a, b)| *a as i128 - *b as i128)
            .collect();
        println!("raw before: {before:?}");
        println!("raw after:  {after:?}");
        println!("deltas (1e6-iter loop): {deltas:?}");
        let s = read();
        println!(
            "current perf.rs mapping reads: instructions={} cycles={} ipc={:.3}",
            s.instructions,
            s.cycles,
            s.ipc()
        );
        teardown();
    }

    #[test]
    fn unavailable_without_feature_reads_zero() {
        // With the perf-counters feature off (the default test build), reads are
        // zero and setup reports unavailable, so the bench falls back cleanly.
        #[cfg(not(feature = "perf-counters"))]
        {
            assert!(!available());
            assert!(!setup());
            assert_eq!(read().instructions, 0);
            teardown(); // no-op, must not panic
        }
    }
}
