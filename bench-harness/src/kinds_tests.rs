//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn row(ts: u64, commit: &str, source: &str, med: f64, lo: f64, hi: f64) -> HistoryEntry {
    HistoryEntry {
        timestamp:  ts,
        git_commit: commit.into(),
        benchmark:  "hash_n64".into(),
        variant:    "fnv1a".into(),
        n:          64,
        mode:       "warm".into(),
        median_ns:  med,
        ci_lo_ns:   lo,
        ci_hi_ns:   hi,
        source:     source.into(),
    }
}

/// A snapshot is compared only with runs of its own lock. The same slower run
/// flags against a history of its source and does not against a history of
/// another source or of rows that name none, which is the filter doing the
/// work rather than the numbers.
#[test]
fn a_snapshot_is_compared_only_with_runs_of_its_own_source() {
    let now = [row(10, "c", "aaaaaaaaaaaa", 150.0, 148.0, 152.0)];
    let same = [row(1, "a", "aaaaaaaaaaaa", 100.0, 98.0, 102.0)];
    let other = [
        row(1, "a", "bbbbbbbbbbbb", 100.0, 98.0, 102.0),
        row(2, "b", "", 100.0, 98.0, 102.0),
    ];

    let s = standing(Kind::Snapshot, "aaaaaaaaaaaa", &now, &same);
    assert_eq!(s.rows.len(), 1);
    assert!(
        s.rows[0].flagged,
        "the same source's history must flag a run that left it"
    );
    assert_eq!(s.label, "DRIFT");

    let s = standing(Kind::Snapshot, "aaaaaaaaaaaa", &now, &other);
    assert!(
        s.rows.is_empty(),
        "another source's runs are no history of this one: {s:?}"
    );
}

/// A snapshot with no source compares with nothing, even rows that also carry
/// none: an empty source names no code, and two of them are not the same code.
#[test]
fn an_empty_source_matches_nothing() {
    let hist = [row(1, "a", "", 100.0, 98.0, 102.0)];
    assert!(comparable(Kind::Snapshot, "", &hist).is_empty());
}

/// A tracking bench's history is its runs at clean commits. A fast run on a
/// dirty tree in the history would pull the baseline down and flag an honest
/// run; left out, the same run stands.
#[test]
fn a_tracking_bench_reads_only_runs_at_clean_commits() {
    let now = [row(10, "c", "-", 101.0, 100.0, 102.0)];
    let clean = row(1, "a", "-", 100.0, 98.0, 102.0);
    let dirty = row(2, "b-dirty", "-", 50.0, 49.0, 51.0);
    let unknown = row(3, "unknown", "-", 50.0, 49.0, 51.0);

    let kept = comparable(Kind::Tracking, "-", &[
        clean.clone(),
        dirty.clone(),
        unknown.clone(),
    ]);
    assert_eq!(kept, vec![clean.clone()]);

    let s = standing(Kind::Tracking, "-", &now, &[clean.clone(), dirty.clone()]);
    assert!(!s.rows[0].flagged, "{s:?}");

    // The control: read as a snapshot reads, the dirty row stays in and flags.
    let all = crate::history::detect_regressions(&now, &[clean, dirty]);
    assert!(
        all[0].flagged,
        "the dirty row must be what flags when it is kept"
    );
}

/// A tracking run on a dirty tree is shown against history and never flagged,
/// with the reason; the same numbers at a clean commit flag.
#[test]
fn a_dirty_tracking_run_is_shown_and_not_flagged() {
    let hist = [row(1, "a", "-", 100.0, 98.0, 102.0)];
    let clean = [row(10, "c", "-", 150.0, 148.0, 152.0)];
    let dirty = [row(10, "c-dirty", "-", 150.0, 148.0, 152.0)];

    let s = standing(Kind::Tracking, "-", &clean, &hist);
    assert!(s.rows[0].flagged);
    assert_eq!(s.label, "REGRESSION");
    assert_eq!(s.note, None);

    let s = standing(Kind::Tracking, "-", &dirty, &hist);
    assert_eq!(s.rows.len(), 1, "the move is still shown");
    assert!(!s.rows[0].flagged);
    assert!(
        s.note
            .as_deref()
            .is_some_and(|n| n.contains("no commit names")),
        "{s:?}"
    );
}

/// A tracking bench's source is the fixed marker; a snapshot's is the id of
/// its lock, refused where the lock is missing, and it changes when the lock
/// does.
#[test]
fn a_snapshot_s_source_is_its_lock_and_a_missing_lock_is_refused() {
    assert_eq!(source_of("b", Kind::Tracking, None).unwrap(), "-");

    let err = source_of("b", Kind::Snapshot, None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("mock bench freeze b"), "{err}");

    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let lock = dir.path().join("snapshot.lock");
    let err = source_of("b", Kind::Snapshot, Some(&lock))
        .unwrap_err()
        .to_string();
    assert!(err.contains("has no lock at"), "{err}");

    std::fs::write(&lock, "x  a/b.rs\n").unwrap();
    let first = source_of("b", Kind::Snapshot, Some(&lock)).unwrap();
    assert_eq!(first.len(), 12);
    assert!(first.bytes().all(|b| b.is_ascii_hexdigit()), "{first}");
    std::fs::write(&lock, "y  a/b.rs\n").unwrap();
    assert_ne!(source_of("b", Kind::Snapshot, Some(&lock)).unwrap(), first);
}

/// The refusal for a bench with no kind names both spellings and the
/// migration, so the reader can act on it without looking anything up.
#[test]
fn the_missing_kind_refusal_names_both_kinds_and_the_migration() {
    let r = no_kind_reason("hash");
    for needle in [
        "`hash`",
        "kind = \"snapshot\"",
        "kind = \"tracking\"",
        "mock bench migrate --kinds",
    ] {
        assert!(r.contains(needle), "missing {needle}: {r}");
    }
}

/// Each kind's report paragraph says what the run's code is and how to read a
/// difference, by its own terms.
#[test]
fn the_methodology_paragraph_states_the_kind_and_its_reading() {
    let s = methodology(Kind::Snapshot, "aaaaaaaaaaaa", "c");
    assert!(
        s.starts_with("Kind: snapshot, source `aaaaaaaaaaaa`"),
        "{s}"
    );
    assert!(s.contains("drift"), "{s}");
    let t = methodology(Kind::Tracking, "-", "abc1234");
    assert!(t.starts_with("Kind: tracking, at `abc1234`"), "{t}");
    assert!(t.contains("trend"), "{t}");
}

/// The kind paragraph lands right after the report's title, and a report of
/// one line still gets it, after the line.
#[test]
fn the_kind_paragraph_follows_the_title() {
    let md = with_kind_paragraph("# Hash\n\n## Findings\n", "Kind: tracking.");
    assert_eq!(md, "# Hash\n\nKind: tracking.\n\n## Findings\n");
    assert_eq!(with_kind_paragraph("# Hash", "K."), "# Hash\n\nK.\n");
}
