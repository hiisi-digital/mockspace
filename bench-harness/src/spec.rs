//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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
//! Variant path resolution (short names to platform dylib paths)
//! lives in [`crate::config::resolve_variant_path`].

use std::path::PathBuf;

use crate::core::RoutineBridge;

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
    pub name:   String,
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
    pub name:       String,
    /// Filesystem path to the variant cdylib (`.dylib` on macOS,
    /// `.so` on Linux, `.dll` on Windows).
    pub dylib_path: PathBuf,
    /// ABI hash the variant was built against, looked up by
    /// `dlsym`-ing `bench_abi_hash` after load. Compared against
    /// [`mockspace_bench_core::abi_hash`] at run start; mismatch is
    /// [`BenchError::AbiMismatch`].
    pub abi_hash:   u64,
}

/// Generate a `(bench, point) -> RoutineSpec` table for custom
/// routines from one declarative block, for the `routine_for` hook.
///
/// One row per bench: the Routine type and the point list it is
/// monomorphised at. Each point stays its own monomorphisation, the
/// same guarantee as [`mockspace_bench_core::byte_routine_dispatch!`],
/// and the point in the pattern and the const argument are written by
/// the macro from one literal, so they cannot drift apart the way a
/// hand-maintained match lets them.
///
/// ```ignore
/// let table = routine_table! {
///     "bitpack-carrier-width" => CarrierColumn[16384, 131072, 1048576],
///     "warm-container"        => Case[80003, 130003, 160003],
/// };
/// // table: fn(&BenchConfig) -> Option<RoutineSpec>, matching on
/// // (config.bench, config.n), so one row serves every sweep of a
/// // nested bench; None falls through to the byte dispatch.
/// ```
#[macro_export]
macro_rules! routine_table {
    ( $( $bench:literal => $ty:ident [ $( $n:literal ),* $(,)? ] ),* $(,)? ) => {{
        fn __routine_table(
            config: &$crate::BenchConfig,
        ) -> Option<$crate::RoutineSpec> {
            match (config.bench.as_str(), config.n) {
                $( $(
                    ($bench, $n) => Some($crate::RoutineSpec {
                        name:   ::std::string::String::from($bench),
                        bridge: $crate::core::routine_bridge!($ty<$n>),
                    }),
                )* )*
                _ => None,
            }
        }
        __routine_table as fn(&$crate::BenchConfig) -> Option<$crate::RoutineSpec>
    }};
}

#[cfg(test)]
mod routine_table_tests {
    use std::path::PathBuf;

    use crate::config::BenchConfig;

    /// A stand-in for the consumer shapes the table dispatches:
    /// generic over one const parameter, such as a `Case<KEY>`.
    struct Keyed<const K: usize>;

    impl<const K: usize> crate::core::Routine for Keyed<K> {
        type Input = u64;
        type Output = u64;

        fn build_input(seed: u64) -> u64 {
            seed ^ K as u64
        }
    }

    /// Configs come from a real nested tree rather than hand-built
    /// structs, because hand-built structs are how the table's key
    /// choice went untested: `bench_name` is the composite
    /// `warm/width-l1` for a named sweep, and a table keyed on it
    /// would never match a nested cell.
    fn tree_configs() -> Vec<BenchConfig> {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "mockspace-routine-table-test-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::remove_dir_all(&root).ok();
        std::fs::create_dir_all(root.join("warm/arms/k/src")).unwrap();
        std::fs::write(root.join("bench.toml"), "[timing]\npasses = 4\n").unwrap();
        std::fs::write(
            root.join("warm/bench.toml"),
            r#"
            title = "Warm"
            arms = ["k"]
            [sweep.width-l1]
            points = [7]
            [sweep.width-l2]
            points = [11]
        "#,
        )
        .unwrap();
        let manifest = crate::tree::load(&root).unwrap().manifest;
        let configs: Vec<BenchConfig> = manifest
            .bench_names()
            .iter()
            .map(|name| manifest.for_size(name, 0, &root).unwrap())
            .collect();
        std::fs::remove_dir_all(&root).ok();
        configs
    }

    #[test]
    fn one_row_serves_every_sweep_of_a_nested_bench_and_unlisted_points_fall_through() {
        let table = routine_table! { "warm" => Keyed[7, 11] };
        let configs = tree_configs();
        assert_eq!(configs.len(), 2, "two sweeps resolved");
        for c in &configs {
            assert_eq!(c.bench, "warm");
            assert_ne!(c.bench_name, "warm", "the composite key is not the bench");
            assert!(
                table(c).is_some(),
                "the table must match the nested cell {} (bench `{}`, n={})",
                c.bench_name,
                c.bench,
                c.n
            );
        }
        // an unlisted point of a listed bench falls through
        let mut off = configs[0].clone();
        off.n = 13;
        assert!(table(&off).is_none());
        // an unlisted bench falls through
        let mut other = configs[0].clone();
        other.bench = "footprint".into();
        assert!(table(&other).is_none());
    }

    #[test]
    fn the_point_reaches_the_const_parameter() {
        // The bridge's input builder is `Keyed::<K>::build_input`, so
        // the built input differs per point iff K was actually the
        // point and not a copy-paste of another row's.
        let table = routine_table! { "warm" => Keyed[7, 11] };
        let configs = tree_configs();
        let by_n = |n: usize| -> crate::spec::RoutineSpec {
            let c = configs.iter().find(|c| c.n == n).unwrap();
            table(c).unwrap()
        };
        let buf7 = (by_n(7).bridge.input_builder)(0);
        let buf11 = (by_n(11).bridge.input_builder)(0);
        assert_eq!(u64::from_le_bytes(buf7.try_into().unwrap()), 7);
        assert_eq!(u64::from_le_bytes(buf11.try_into().unwrap()), 11);
        let _ = PathBuf::new();
    }
}
