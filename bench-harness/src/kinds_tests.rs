//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// A tracking bench's fallback source is the fixed marker; a snapshot's is the
/// id of its lock, refused where the lock is missing, and it changes when the
/// lock does.
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

/// A tracking arm a run handed sources to and that finds none is refused,
/// never written as unknown; a snapshot arm takes its lock id; a driver run by
/// hand, handed nothing, writes the fallback.
#[test]
fn a_tracking_arm_with_no_handed_source_is_refused() {
    let handed = Sources::parse("target/mock-arms/live/a\taaaaaaaaaaaa\n");
    let root = Path::new("/tree");
    let a = Path::new("/tree/target/mock-arms/live/a/release/liba.so");
    let b = Path::new("/tree/target/mock-arms/live/b/release/libb.so");
    assert_eq!(Sources::for_variant(Some(&handed), Kind::Tracking, root, a, "-").unwrap(), "aaaaaaaaaaaa");
    let err = Sources::for_variant(Some(&handed), Kind::Tracking, root, b, "-").unwrap_err();
    assert!(err.contains("handed no source"), "{err}");
    assert_eq!(Sources::for_variant(Some(&handed), Kind::Snapshot, root, b, "lockid").unwrap(), "lockid");
    assert_eq!(Sources::for_variant(None, Kind::Tracking, root, b, "-").unwrap(), "-");
}

/// Each arm's library path finds its own source by prefix, an arm nested
/// under another's prefix finds its own, `..` in either spelling is taken
/// out, and a path no line names finds none.
#[test]
fn an_arm_finds_its_source_by_its_library_path() {
    let s = Sources::parse(
        "target/mock-arms/hash/fnv\taaaaaaaaaaaa\ntarget/mock-arms/hash/fnv/inner\tbbbbbbbbbbbb\nstorage/../variants/v\tcccccccccccc\n",
    );
    let root = Path::new("/tree");
    assert_eq!(
        s.of(
            root,
            Path::new("/tree/target/mock-arms/hash/fnv/release/libfnv.so")
        ),
        Some("aaaaaaaaaaaa")
    );
    assert_eq!(
        s.of(
            root,
            Path::new("/tree/target/mock-arms/hash/fnv/inner/x.so")
        ),
        Some("bbbbbbbbbbbb")
    );
    assert_eq!(
        s.of(root, Path::new("/tree/variants/v/target/release/libv.so")),
        Some("cccccccccccc")
    );
    assert_eq!(
        s.of(
            root,
            Path::new("/tree/target/mock-arms/hash/xx/release/libxx.so")
        ),
        None
    );
    assert_eq!(Sources::default().of(root, Path::new("/tree/a")), None);
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

/// Each kind's report paragraph names the system and says how a difference
/// is read, by points.
#[test]
fn the_methodology_paragraph_states_the_kind_the_system_and_its_reading() {
    let s = methodology(Kind::Snapshot, "aaaaaaaaaaaa", "c", "linux-x");
    assert!(
        s.starts_with("Kind: snapshot, source `aaaaaaaaaaaa`, on `linux-x`"),
        "{s}"
    );
    let t = methodology(Kind::Tracking, "-", "abc1234", "linux-x");
    assert!(
        t.starts_with("Kind: tracking, at `abc1234`, on `linux-x`"),
        "{t}"
    );
    assert!(t.contains("never compared with this one"), "{t}");
}

/// A run is named by its system and commit, and a row from before the system
/// column by its commit alone.
#[test]
fn a_run_is_named_by_its_system_and_commit() {
    let row = |system: &str| {
        HistoryEntry {
            timestamp:  1,
            git_commit: "a".into(),
            benchmark:  "hash_n64".into(),
            variant:    "fnv1a".into(),
            n:          64,
            mode:       "warm".into(),
            median_ns:  1.0,
            ci_lo_ns:   1.0,
            ci_hi_ns:   1.0,
            source:     "s".into(),
            system:     system.into(),
        }
    };
    assert_eq!(measured(&[row("linux-x")]), ", on `linux-x` at `a`");
    assert_eq!(measured(&[row("")]), ", at `a`");
    assert_eq!(measured(&[]), "");
}

/// The kind paragraph lands right after the report's title, and a report of
/// one line still gets it, after the line.
#[test]
fn the_kind_paragraph_follows_the_title() {
    let md = with_kind_paragraph("# Hash\n\n## Findings\n", "Kind: tracking.");
    assert_eq!(md, "# Hash\n\nKind: tracking.\n\n## Findings\n");
    assert_eq!(with_kind_paragraph("# Hash", "K."), "# Hash\n\nK.\n");
}
