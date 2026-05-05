//! Hardware performance counter reads + deterministic PRNG.
//! CNTVCT_EL0 on aarch64 (24MHz, 41.67ns/tick).
//! rdtsc on x86_64 (reference frequency).
//! Deterministic latency, no cache interaction.

/// Deterministic PRNG. Same seed -> same sequence on every platform.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state ^= self.state >> 30;
        self.state = self.state.wrapping_mul(0xBF58476D1CE4E5B9);
        self.state ^= self.state >> 27;
        self.state = self.state.wrapping_mul(0x94D049BB133111EB);
        self.state ^= self.state >> 31;
        self.state
    }

    #[cfg(feature = "std")]
    #[must_use = "seeds advances RNG state; the returned Vec contains the only record of the produced values"]
    pub fn seeds(&mut self, n: usize) -> std::vec::Vec<u64> {
        (0..n).map(|_| self.next()).collect()
    }
}

/// Read the hardware counter. Returns raw ticks.
#[inline(always)]
pub fn read_counter() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        let val: u64;
        unsafe {
            core::arch::asm!("mrs {}, CNTVCT_EL0", out(reg) val, options(nostack, nomem));
        }
        val
    }
    #[cfg(target_arch = "x86_64")]
    {
        let lo: u32;
        let hi: u32;
        unsafe {
            core::arch::asm!(
                "lfence",
                "rdtsc",
                out("eax") lo,
                out("edx") hi,
                options(nostack),
            );
        }
        (hi as u64) << 32 | lo as u64
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        // Fallback: Instant-based, less precise (requires std)
        #[cfg(feature = "std")]
        {
            use std::time::Instant;
            static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
            let epoch = EPOCH.get_or_init(Instant::now);
            epoch.elapsed().as_nanos() as u64
        }
        #[cfg(not(feature = "std"))]
        {
            0 // no counter available in no_std on unsupported arch
        }
    }
}

/// Get the counter frequency in Hz. Read once, cache forever.
#[cfg(feature = "std")]
pub fn counter_frequency() -> u64 {
    #[cfg(target_arch = "aarch64")]
    {
        static FREQ: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        *FREQ.get_or_init(|| {
            let freq: u64;
            unsafe {
                core::arch::asm!("mrs {}, CNTFRQ_EL0", out(reg) freq, options(nostack, nomem));
            }
            freq
        })
    }
    #[cfg(target_arch = "x86_64")]
    {
        // R26: calibrate over 1 second, take 3 measurements, use the median.
        static FREQ: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        *FREQ.get_or_init(|| {
            let mut samples = [0u64; 3];
            for s in &mut samples {
                let start = read_counter();
                std::thread::sleep(std::time::Duration::from_millis(1_000));
                let end = read_counter();
                *s = end - start;
            }
            samples.sort_unstable();
            samples[1] // median of 3
        })
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        1_000_000_000 // fallback is already in nanoseconds
    }
}

/// Convert counter ticks to nanoseconds.
#[cfg(feature = "std")]
#[inline(always)]
#[must_use]
pub fn ticks_to_ns(ticks: u64) -> f64 {
    let freq = counter_frequency() as f64;
    (ticks as f64 * 1_000_000_000.0) / freq
}

/// Convert counter ticks to microseconds.
#[cfg(feature = "std")]
#[inline(always)]
#[must_use]
pub fn ticks_to_us(ticks: u64) -> f64 {
    let freq = counter_frequency() as f64;
    (ticks as f64 * 1_000_000.0) / freq
}

// -- Core affinity (Apple Silicon) --

/// Best-effort P-core pinning. Sets QoS class to USER_INTERACTIVE
/// (biases scheduler toward P-cores) and sets thread affinity tag
/// to hint the scheduler to keep the thread on the same core group.
#[cfg(all(feature = "std", target_os = "macos"))]
pub fn pin_to_perf_cores() {
    use std::os::raw::c_int;

    // QoS class: USER_INTERACTIVE = 0x21, biases toward P-cores
    unsafe {
        extern "C" {
            fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: c_int) -> c_int;
        }
        let ret = pthread_set_qos_class_self_np(0x21, 0);
        if ret != 0 {
            std::eprintln!("  warning: QoS class set failed ({})", ret);
        }
    }

    // Thread affinity tag: keeps thread in same scheduling group
    unsafe {
        extern "C" {
            fn mach_thread_self() -> u32;
            fn thread_policy_set(
                thread: u32,
                flavor: c_int,
                policy_info: *const c_int,
                count: u32,
            ) -> c_int;
        }
        const THREAD_AFFINITY_POLICY: c_int = 4;
        let affinity_tag: c_int = 1;
        thread_policy_set(mach_thread_self(), THREAD_AFFINITY_POLICY, &affinity_tag, 1);
    }
}

#[cfg(all(feature = "std", not(target_os = "macos")))]
pub fn pin_to_perf_cores() {
    // No-op on non-macOS platforms.
}

// -- Partial cache eviction --

/// Evict a range of memory from all cache levels.
/// Used for "L2 warm, L1 cold" measurement modes.
///
/// `ptr` must be aligned to cache line size (64 bytes on most platforms).
/// `len` is in bytes.
#[inline(always)]
pub fn evict_cache_range(ptr: *const u8, len: usize) {
    const CACHE_LINE: usize = 64;
    let mut offset = 0;
    while offset < len {
        let addr = unsafe { ptr.add(offset) };
        #[cfg(target_arch = "aarch64")]
        unsafe {
            // DC CIVAC: Clean and Invalidate by VA to Point of Coherency
            core::arch::asm!(
                "dc civac, {addr}",
                addr = in(reg) addr,
                options(nostack),
            );
        }
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::asm!(
                "clflush [{addr}]",
                addr = in(reg) addr,
                options(nostack),
            );
        }
        offset += CACHE_LINE;
    }
    // Memory barrier to ensure evictions complete before measurement
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("dsb sy", options(nostack));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("mfence", options(nostack));
    }
}
