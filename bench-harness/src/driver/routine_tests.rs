//! Whether a routine handed in by `routine_for` is compared byte for byte.
//!
//! The manifest's `may_differ` reached only the byte dispatch, which takes it
//! as an argument. A handed routine answers the question with its own trait
//! method, so a bench that said `may_differ = true` in its file and handed a
//! routine that never overrode the method had every arm compared byte for byte
//! and dropped in validation. The four arms are the whole matrix of the two
//! flags, and the one where neither says so is the control: the comparison
//! still happens there.

use super::*;
use crate::core::Routine;

/// A routine that says nothing about its outputs, so the trait's default
/// answers for it.
struct Silent;

impl Routine for Silent {
    type Input = u64;
    type Output = u64;

    fn build_input(seed: u64) -> u64 {
        seed
    }
}

/// A routine that says its variants may disagree.
struct Differs;

impl Routine for Differs {
    type Input = u64;
    type Output = u64;

    fn build_input(seed: u64) -> u64 {
        seed
    }

    fn outputs_may_differ() -> bool {
        true
    }
}

fn silent(_: &BenchConfig) -> Option<RoutineSpec> {
    Some(RoutineSpec {
        name:   "silent".into(),
        bridge: crate::core::routine_bridge!(Silent),
    })
}

fn differs(_: &BenchConfig) -> Option<RoutineSpec> {
    Some(RoutineSpec {
        name:   "differs".into(),
        bridge: crate::core::routine_bridge!(Differs),
    })
}

fn spec(routine_for: fn(&BenchConfig) -> Option<RoutineSpec>) -> DriverSpec {
    fn workload(_: &str, _: usize) -> Workload {
        Workload::new()
    }
    fn no_dispatch(_: usize, _: bool) -> Option<crate::core::RoutineBridge> {
        None
    }
    DriverSpec {
        build_workload: workload,
        byte_dispatch:  ByteDispatch {
            dispatch: no_dispatch,
            sizes:    &[],
        },
        hooks:          Hooks {
            routine_for: Some(routine_for),
            ..Hooks::default()
        },
    }
}

fn manifest_says(may_differ: bool) -> BenchConfig {
    BenchConfig {
        may_differ,
        ..BenchConfig::default()
    }
}

fn resolved(routine_for: fn(&BenchConfig) -> Option<RoutineSpec>, may_differ: bool) -> bool {
    resolve_routine(&spec(routine_for), &manifest_says(may_differ))
        .expect("the hook answers every config")
        .bridge
        .outputs_may_differ
}

#[test]
fn a_manifest_saying_the_arms_differ_reaches_a_routine_that_did_not_say_so() {
    assert!(resolved(silent, true));
}

#[test]
fn a_routine_and_a_manifest_both_silent_are_still_compared_byte_for_byte() {
    // The control for the arm above: the same routine, the manifest silent,
    // and the comparison stays on.
    assert!(!resolved(silent, false));
}

#[test]
fn a_silent_manifest_does_not_narrow_a_routine_that_says_its_arms_differ() {
    assert!(resolved(differs, false));
}

#[test]
fn a_routine_and_a_manifest_that_both_say_so_are_not_compared() {
    assert!(resolved(differs, true));
}
