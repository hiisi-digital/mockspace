//! Whether a routine handed in by `routine_for` is compared byte for byte.
//!
//! The manifest's `may_differ` reached only the byte dispatch, which takes it
//! as an argument. A handed routine answers the question with its own trait
//! method, so a bench that said `may_differ = true` in its file and handed a
//! routine that never overrode the method had every arm compared byte for byte
//! and dropped in validation. The four arms are the whole matrix of the two
//! flags, and the one where neither says so is the control: the comparison
//! still happens there. The last two take a routine that declared a
//! tolerance, which the manifest does not switch off.

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

/// A routine that says how far its variants may disagree.
struct Within;

impl Routine for Within {
    type Input = u64;
    type Output = u64;

    fn build_input(seed: u64) -> u64 {
        seed
    }

    fn max_relative_error() -> Option<f64> {
        Some(1e-3)
    }
}

fn within(_: &BenchConfig) -> Option<RoutineSpec> {
    Some(RoutineSpec {
        name:   "within".into(),
        bridge: crate::core::routine_bridge!(Within),
    })
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

/// What the validation pass would compare the handed routine's arms with.
fn compared_with(
    routine_for: fn(&BenchConfig) -> Option<RoutineSpec>,
    may_differ: bool,
) -> Option<crate::validation::CrossVariant> {
    let found = resolve_routine(&spec(routine_for), &manifest_says(may_differ))
        .expect("the hook answers every config");
    crate::validation::validation_plan(
        found.bridge.outputs_may_differ,
        found.bridge.max_relative_error,
    )
    .cross_variant
}

#[test]
fn a_manifest_saying_the_arms_differ_leaves_a_routines_tolerance_in_place() {
    assert_eq!(
        compared_with(within, true),
        Some(crate::validation::CrossVariant::Approx(1e-3))
    );
}

#[test]
fn a_routines_tolerance_is_compared_with_when_the_manifest_is_silent_too() {
    // The control for the arm above: the manifest silent, the same answer, so
    // the arm above is not passing because the tolerance is always kept.
    assert_eq!(
        compared_with(within, false),
        Some(crate::validation::CrossVariant::Approx(1e-3))
    );
    // And the manifest does switch off a routine that declared nothing.
    assert_eq!(compared_with(silent, true), None);
}
