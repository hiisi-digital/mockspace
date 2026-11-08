use super::{day_from_path, days_from_civil};

#[test]
fn the_epoch_is_day_zero_and_a_leap_day_is_counted() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(1970, 1, 2), 1);
    // 2024 is a leap year: 29 February exists and 1 March follows it.
    assert_eq!(days_from_civil(2024, 3, 1) - days_from_civil(2024, 2, 28), 2);
    // 2100 is not, so the century rule is the other side of the same arm.
    assert_eq!(days_from_civil(2100, 3, 1) - days_from_civil(2100, 2, 28), 1);
}

#[test]
fn the_deepest_stamp_in_a_path_is_the_documents_own_date() {
    let round = "design_rounds/202610040050/202610040042_topic.the-air.md";
    assert_eq!(day_from_path(round), Some(days_from_civil(2026, 10, 4)));

    let later = "research/202611010500_ikiuni-renderer-library/202611020000_note.md";
    assert_eq!(day_from_path(later), Some(days_from_civil(2026, 11, 2)));

    // A directory alone dates what is in it that carries no stamp.
    let inner = "research/202611010500_ikiuni-renderer-library/NOTES.md";
    assert_eq!(day_from_path(inner), Some(days_from_civil(2026, 11, 1)));
}

#[test]
fn a_path_without_a_stamp_has_no_date() {
    assert_eq!(day_from_path("research/NOTES.md"), None);
    assert_eq!(day_from_path("mock/registry/tenet/workflow.toml"), None);
}

#[test]
fn what_only_looks_like_a_stamp_is_not_one() {
    // THE CONTROL: the same twelve digits, valid, are a stamp.
    assert!(day_from_path("a/202610040042_x.md").is_some());
    // A month 13, an hour 24, a thirteenth digit and eleven digits are each not.
    assert_eq!(day_from_path("a/202613040042_x.md"), None);
    assert_eq!(day_from_path("a/202610042442_x.md"), None);
    assert_eq!(day_from_path("a/2026100400421_x.md"), None);
    assert_eq!(day_from_path("a/20261004004_x.md"), None);
}
