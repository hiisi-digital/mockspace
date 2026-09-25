use super::{Names, call_bound, names};

fn declares_four_hours(_n: usize) -> Option<u64> {
    Some(4 * 3600 * 1_000_000)
}

fn declares_by_size(n: usize) -> Option<u64> {
    Some(n as u64 * 1000)
}

fn declares_nothing(_n: usize) -> Option<u64> {
    None
}

#[test]
fn a_routines_declared_bound_is_the_bound_when_nothing_is_configured() {
    assert_eq!(call_bound(None, declares_four_hours, 1), Some(14_400_000_000));
}

#[test]
fn the_routine_is_asked_at_the_cells_own_size() {
    assert_eq!(call_bound(None, declares_by_size, 1), Some(1000));
    assert_eq!(call_bound(None, declares_by_size, 48), Some(48_000));
}

#[test]
fn a_configured_bound_wins_over_the_routines() {
    assert_eq!(call_bound(Some(7), declares_four_hours, 1), Some(7));
}

#[test]
fn neither_saying_anything_leaves_no_bound() {
    assert_eq!(call_bound(None, declares_nothing, 1), None);
}

#[test]
fn no_samples_at_all_is_every_worker_lost_and_never_a_collision() {
    assert_eq!(names(&[], 2), Names::NoneLeft);
    assert_eq!(names(&[], 1), Names::NoneLeft);
}

#[test]
fn one_name_per_variant_is_whole() {
    assert_eq!(names(&["margin", "likelihood", "margin"], 2), Names::Whole);
    assert_eq!(names(&["only"], 1), Names::Whole);
}

#[test]
fn fewer_names_than_variants_counts_the_distinct_ones() {
    assert_eq!(names(&["margin", "margin", "margin"], 2), Names::Fewer { names: 1 });
    assert_eq!(names(&["a", "b", "a"], 3), Names::Fewer { names: 2 });
}
