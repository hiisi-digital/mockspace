//! Routine + variant specification: what the consumer hands to the
//! harness so it can find and dispatch the variant cdylibs.
//!
//! A [`RoutineSpec`] names one Routine and supplies a
//! [`mockspace_bench_core::RoutineBridge`] (via the
//! `routine_bridge!` macro) so the harness can build inputs, validate
//! outputs, and score results without knowing the concrete
//! `Routine::Input`/`Output` types. A [`VariantSpec`] names one
//! variant cdylib path and the ABI hash it was built against.
//!
//! Round 1 ships the types and a stub `discover_variants` helper that
//! returns an empty list. Round 3 fills in real cdylib discovery from
//! the consumer's `mock/benches/variants/<name>/target/release/`.

use std::path::PathBuf;

use crate::core::RoutineBridge;
use crate::error::BenchError;

/// One Routine plus the byte-level dispatch bridge the harness needs
/// to invoke its variants.
///
/// Construct from a Routine-impl type via the
/// [`mockspace_bench_core::routine_bridge!`] macro:
///
/// ```ignore
/// use mockspace_bench_core::routine_bridge;
/// use mockspace_bench_harness::RoutineSpec;
///
/// let spec = RoutineSpec {
///     name: "ContentHash".into(),
///     bridge: routine_bridge!(ContentHash),
/// };
/// ```
pub struct RoutineSpec {
    /// Routine display name. Used in CSV cache, findings.md, and CLI
    /// output. Convention: matches the `Routine` impl type name.
    pub name: String,
    /// Byte-level dispatch bridge built via
    /// [`mockspace_bench_core::routine_bridge!`].
    pub bridge: RoutineBridge,
}

/// One variant cdylib that implements [`RoutineSpec::bridge`]'s
/// Routine.
#[derive(Clone, Debug)]
pub struct VariantSpec {
    /// Short variant name (e.g. `"fnv1a"`, `"xxhash3"`). Used in
    /// reports and CSV cache.
    pub name: String,
    /// Filesystem path to the variant cdylib (`.dylib` on macOS,
    /// `.so` on Linux, `.dll` on Windows).
    pub dylib_path: PathBuf,
    /// ABI hash the variant was built against, looked up by
    /// `dlsym`-ing `bench_abi_hash` after load. Compared against
    /// [`mockspace_bench_core::abi_hash`] at run start; mismatch is
    /// [`BenchError::AbiMismatch`].
    pub abi_hash: u64,
}

/// Discover variant cdylibs under a consumer's
/// `mock/benches/variants/` tree.
///
/// Round 1 stub: always returns
/// [`BenchError::NotImplemented`]. Round 3 walks
/// `<root>/variants/<name>/target/release/` looking for cdylibs and
/// `dlsym`s `bench_abi_hash`/`bench_name` from each.
pub fn discover_variants(_root: &std::path::Path) -> Result<Vec<VariantSpec>, BenchError> {
    Err(BenchError::NotImplemented {
        what: "discover_variants (cdylib walking lands in Round 3)",
    })
}
