//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

const A: &str = "linux-x86_64-c4-bare-amd";
const B: &str = "macos-aarch64-c8-bare-apple";

fn row(ts: u64, system: &str, source: &str, med: f64) -> HistoryEntry {
    HistoryEntry {
        timestamp:  ts,
        git_commit: format!("c{ts}"),
        benchmark:  "hash_n64".into(),
        variant:    "fnv".into(),
        n:          64,
        mode:       "warm".into(),
        median_ns:  med,
        ci_lo_ns:   med - 2.0,
        ci_hi_ns:   med + 2.0,
        source:     source.into(),
        system:     system.into(),
    }
}

/// Another system is another variant: a slower row on one system is not read
/// against the other's history, and the same row against its own system's
/// history is.
#[test]
fn a_system_is_never_compared_with_another() {
    let hist = [row(1, A, "s", 100.0)];
    assert!(
        standing(&[row(9, B, "s", 300.0)], &hist).is_empty(),
        "B has no history of its own"
    );
    let f = standing(&[row(9, A, "s", 300.0)], &hist);
    assert_eq!(
        f.len(),
        1,
        "the control: the same row on A is read against A"
    );
}

/// Every run at one source adds to that point, whatever commit it was taken
/// at; nothing is dropped as already measured.
#[test]
fn every_run_at_a_source_adds_to_its_point() {
    let rows = [
        row(1, A, "a", 100.0),
        row(2, A, "a", 102.0),
        row(3, A, "a", 98.0),
        row(4, A, "b", 100.0),
    ];
    let t = trail(&rows);
    assert_eq!(
        t.iter()
            .map(|p| (p.source.as_str(), p.runs))
            .collect::<Vec<_>>(),
        [("a", 3), ("b", 1)]
    );
    assert_eq!(t[0].median_ns, 100.0);
}

/// A run at an older source, as a run at an older revision is, lands under
/// that source's point: the trail returns to it and its point counts every
/// row of it, the earlier ones and the new one.
#[test]
fn a_run_at_an_older_source_lands_under_its_point() {
    let rows = [row(1, A, "old", 100.0), row(2, A, "new", 150.0), row(3, A, "old", 101.0)];
    let t = trail(&rows);
    assert_eq!(
        t.iter()
            .map(|p| (p.source.as_str(), p.runs))
            .collect::<Vec<_>>(),
        [("old", 2), ("new", 1), ("old", 2)]
    );
    assert_eq!(step(&t[0], &t[1]), Move::Slower);
    assert_eq!(step(&t[1], &t[2]), Move::Faster);
}

/// A run at the point the history already stands at is read for drift; a run
/// at a new point against the point before it, slower or faster.
#[test]
fn a_move_at_the_same_point_is_drift_and_at_a_new_point_is_the_code() {
    let hist = [row(1, A, "a", 100.0), row(2, A, "a", 100.0)];
    assert!(
        standing(&[row(9, A, "a", 100.5)], &hist).is_empty(),
        "inside its interval: nothing"
    );
    let drift = standing(&[row(9, A, "a", 130.0)], &hist);
    assert_eq!((drift[0].label, drift[0].against.as_str()), (DRIFT, "a"));
    let slower = standing(&[row(9, A, "b", 130.0)], &hist);
    assert_eq!(
        (slower[0].label, slower[0].against.as_str()),
        (REGRESSION, "a")
    );
    let faster = standing(&[row(9, A, "b", 70.0)], &hist);
    assert_eq!(faster[0].label, FASTER);
    assert!(
        standing(&[row(9, A, "b", 100.5)], &hist).is_empty(),
        "a new point that did not move: nothing"
    );
}

/// The comparison is with the latest point only: a run at a new source is
/// read against the point just before it, not against every earlier one
/// pooled.
#[test]
fn a_new_point_is_read_against_the_point_just_before_it() {
    let hist = [row(1, A, "a", 50.0), row(2, A, "b", 100.0)];
    let f = standing(&[row(9, A, "c", 100.5)], &hist);
    assert!(f.is_empty(), "level with `b`, though far above `a`: {f:?}");
}

/// A source that names no code, `""` from before the column or `-` from a
/// driver run by hand, is never a point and never a step: history of only
/// such rows is no history, the first known run after it is read against
/// nothing, and an unknown run is read against nothing. The rows are still
/// counted in the trail view.
#[test]
fn an_unknown_source_is_never_a_point_or_a_step() {
    let old = [row(1, A, "", 100.0), row(2, A, "-", 300.0), row(3, A, "", 50.0)];
    assert!(trail(&old).is_empty());
    assert!(standing(&[row(9, A, "lockid", 900.0)], &old).is_empty(), "no REGRESSION of code on the first run after migration");
    assert!(standing(&[row(9, A, "-", 900.0)], &[row(1, A, "a", 100.0)]).is_empty());
    assert!(standing(&[row(9, A, "", 900.0)], &[row(1, A, "", 100.0)]).is_empty(), "pre-v2 rows do not pool into a point");

    // the control: one known row before it, and the same run is read
    let mut hist = old.to_vec();
    hist.push(row(4, A, "lockid", 100.0));
    assert_eq!(standing(&[row(9, A, "lockid", 900.0)], &hist)[0].label, DRIFT);
    let md = render_trails(&hist);
    assert!(md.contains(&format!("| {A} | fnv | unknown | 3 | | not a point |")), "{md}");
    assert!(md.contains(&format!("| {A} | fnv | `lockid` | 1 |")), "{md}");
}

/// The run's standing reads the same point the trail reads, every row of it
/// and not a window of the latest: six slow runs and five fast at one source
/// make a point whose median is the slow one, and a run level with that point
/// is level, though it sits well above the latest five.
#[test]
fn the_standing_reads_the_whole_point_as_the_trail_does() {
    let mut hist: Vec<HistoryEntry> = (1 ..= 6).map(|t| row(t, A, "a", 150.0)).collect();
    hist.extend((7 ..= 11).map(|t| row(t, A, "a", 100.0)));
    assert_eq!(point_of(&hist, "a").median_ns, 150.0);
    assert!(standing(&[row(20, A, "a", 149.0)], &hist).is_empty(), "level with the whole point");
    assert_eq!(standing(&[row(20, A, "a", 100.0)], &hist)[0].label, DRIFT, "the control: below the whole point");
}

/// For a run at a new point, the label the run is given and the step the
/// trail shows once the run is in are the same, for a slower, a faster and a
/// level run, over a history longer than any window.
#[test]
fn the_run_s_label_and_the_trail_s_step_agree() {
    let mut hist: Vec<HistoryEntry> = (1 ..= 8).map(|t| row(t, A, "a", 100.0 + t as f64 * 0.1)).collect();
    hist.extend((9 ..= 15).map(|t| row(t, A, "b", 120.0)));
    for (med, want) in [(150.0, Some(REGRESSION)), (60.0, Some(FASTER)), (120.5, None)] {
        let cur = row(30, A, "c", med);
        let label = label_for(&standing(std::slice::from_ref(&cur), &hist), "fnv");
        let mut all = hist.clone();
        all.push(cur);
        let t = trail(&all);
        let shown = match step(&t[t.len() - 2], &t[t.len() - 1]) {
            Move::Slower => Some(REGRESSION),
            Move::Faster => Some(FASTER),
            Move::Level => None,
        };
        assert_eq!((label, shown), (want, want), "median {med}");
    }
}

/// A finding is read by its fields and states its percentage once, with the
/// bench, the variant and the source it was read against named; another mode
/// or point is no history of it.
#[test]
fn a_finding_names_what_moved_and_by_how_much() {
    let hist = [row(1, A, "a", 100.0)];
    let f = standing(&[row(9, A, "b", 120.0)], &hist);
    assert_eq!((f[0].variant.as_str(), f[0].label), ("fnv", REGRESSION));
    assert!((f[0].pct - 20.0).abs() < 1e-9, "{}", f[0].pct);
    assert_eq!(f[0].render("hash"), "REGRESSION: hash fnv +20.0% against source `a`");
    let mut cold = row(1, A, "a", 100.0);
    cold.mode = "cold".into();
    assert!(standing(&[row(9, A, "b", 120.0)], &[cold]).is_empty());
}

/// The side-by-side view has one column per system and never one pooled
/// number; the trail view names each system's points and steps.
#[test]
fn the_views_keep_systems_apart() {
    let md = render_systems(&[
        (A.into(), vec![row(1, A, "a", 100.0)]),
        (B.into(), vec![row(1, B, "a", 40.0)]),
    ]);
    assert!(md.starts_with(&format!("| variant | {A} | {B} |")), "{md}");
    assert!(md.contains("| fnv | 100 ns | 40 ns |"), "{md}");

    let rows = [row(1, A, "a", 100.0), row(2, A, "b", 150.0), row(1, B, "a", 40.0)];
    let md = render_trails(&rows);
    assert!(
        md.contains(&format!("| {A} | fnv | `b` | 1 | 150 ns | REGRESSION |")),
        "{md}"
    );
    assert!(
        md.contains(&format!("| {B} | fnv | `a` | 1 | 40 ns |  |")),
        "{md}"
    );
}
