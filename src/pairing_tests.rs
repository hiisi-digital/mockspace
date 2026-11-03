//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

const H1: &str = "0123456789abcdef0123456789abcdef01234567";
const H2: &str = "89abcdef0123456789abcdef0123456789abcdef";
const H3: &str = "fedcba9876543210fedcba9876543210fedcba98";
const D1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// 2026-11-03T10:15:23Z
const T: i64 = 1_793_700_923;

fn files<'a>(entries: &'a [(&'a str, &'a [u8])]) -> Result<String, String> {
    tree_digest(entries.iter().copied())
}

#[test]
fn the_digest_does_not_depend_on_the_order_the_files_come_in() {
    let a = files(&[("a/x", b"1"), ("b", b"2"), ("a/y", b"3")]).unwrap();
    let b = files(&[("b", b"2"), ("a/y", b"3"), ("a/x", b"1")]).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
    assert!(is_hex(&a, &[64]));
}

#[test]
fn the_digest_changes_with_a_content_a_path_a_file_added_and_a_file_gone() {
    let base = files(&[("a", b"1"), ("b", b"2")]).unwrap();
    for (what, other) in [
        ("a content", files(&[("a", b"1"), ("b", b"3")])),
        ("a path", files(&[("a", b"1"), ("c", b"2")])),
        (
            "a file added",
            files(&[("a", b"1"), ("b", b"2"), ("c", b"")]),
        ),
        ("a file gone", files(&[("a", b"1")])),
        ("contents swapped", files(&[("a", b"2"), ("b", b"1")])),
    ] {
        assert_ne!(base, other.unwrap(), "{what}");
    }
}

#[test]
fn what_one_file_ends_with_and_the_next_starts_with_cannot_be_moved_across_the_border() {
    // the control for the length prefixes: without them these two run together into the same bytes
    let a = files(&[("a", b"xy"), ("b", b"z")]).unwrap();
    let b = files(&[("a", b"x"), ("b", b"yz")]).unwrap();
    assert_ne!(a, b);
    let c = files(&[("ab", b"")]).unwrap();
    let d = files(&[("a", b""), ("b", b"")]).unwrap();
    assert_ne!(c, d);
}

#[test]
fn no_files_have_a_digest_and_it_is_not_the_digest_of_one_empty_file() {
    let none = files(&[]).unwrap();
    assert_eq!(none.len(), 64);
    assert_ne!(none, files(&[("a", b"")]).unwrap());
}

#[test]
fn a_path_given_twice_and_a_path_that_is_not_relative_are_refused() {
    assert!(
        files(&[("a", b"1"), ("a", b"1")])
            .unwrap_err()
            .contains("twice")
    );
    for path in ["", "/a", "a\\b", "./a", "a/../b", "a//b", "a/", "a/."] {
        let err = files(&[(path, b"")]).unwrap_err();
        assert!(err.contains("relative"), "{path}: {err}");
    }
    // the control: a name that only looks like the refused ones
    assert!(files(&[(".hidden/..data/a.b", b"")]).is_ok());
}

fn pairing() -> Pairing {
    Pairing {
        hash:   H1.to_owned(),
        time:   T,
        digest: D1.to_owned(),
    }
}

#[test]
fn a_pairing_is_read_back_exactly_as_it_was_written() {
    let p = pairing();
    let line = p.render();
    assert_eq!(line, format!("{H1} 2026-11-03T10:15:23Z {D1}"));
    assert_eq!(Pairing::parse(&line).unwrap(), p);
    let sha256_hash = Pairing {
        hash: "c".repeat(64),
        ..pairing()
    };
    assert_eq!(Pairing::parse(&sha256_hash.render()).unwrap(), sha256_hash);
}

#[test]
fn a_pairing_that_is_not_three_fields_of_the_right_kind_is_refused() {
    let ok = pairing().render();
    assert!(Pairing::parse(&ok).is_ok(), "the control");
    let hash_short = ok.replacen(H1, &H1[.. 39], 1);
    let hash_upper = ok.replacen(H1, &H1.to_uppercase(), 1);
    let digest_short = ok.replace(D1, &D1[.. 63]);
    let digest_upper = ok.replace(D1, &D1.to_uppercase());
    let digest_not_hex = ok.replace(D1, &"g".repeat(64));
    for (what, bad) in [
        ("two fields", format!("{H1} {D1}")),
        ("four fields", format!("{ok} extra")),
        ("a double space", ok.replacen(' ', "  ", 1)),
        ("a trailing space", format!("{ok} ")),
        ("a tab", ok.replacen(' ', "\t", 1)),
        ("a short hash", hash_short),
        ("an upper case hash", hash_upper),
        ("a short digest", digest_short),
        ("an upper case digest", digest_upper),
        ("a digest that is not hex", digest_not_hex),
        ("nothing", String::new()),
    ] {
        assert!(Pairing::parse(&bad).is_err(), "{what}");
    }
}

#[test]
fn a_trailer_carries_the_name_of_the_ref_and_reads_back() {
    let t = Trailer {
        name:    "registry".to_owned(),
        pairing: pairing(),
    };
    let line = t.render();
    assert_eq!(
        line,
        format!("Mock-Ref: registry {H1} 2026-11-03T10:15:23Z {D1}")
    );
    assert_eq!(Trailer::parse(&line).unwrap(), t);
    for name in ["rest", "a", "9", "mock-rest_2"] {
        let t = Trailer {
            name:    name.to_owned(),
            pairing: pairing(),
        };
        assert_eq!(Trailer::parse(&t.render()).unwrap(), t, "{name}");
    }
}

#[test]
fn a_trailer_with_a_bad_name_a_bad_key_or_no_pairing_is_refused() {
    let p = pairing().render();
    for bad in [
        format!("Mock-Ref: Registry {p}"),
        format!("Mock-Ref: -registry {p}"),
        format!("Mock-Ref: _registry {p}"),
        format!("Mock-Ref: reg/istry {p}"),
        format!("Mock-Ref:  {p}"),
        format!("Mock-Ref registry {p}"),
        format!("mock-ref: registry {p}"),
        format!("Mock-Refs: registry {p}"),
        "Mock-Ref: registry".to_owned(),
        "Mock-Ref: ".to_owned(),
    ] {
        assert!(Trailer::parse(&bad).is_err(), "{bad}");
    }
}

#[test]
fn the_trailers_of_a_message_are_found_among_its_other_lines() {
    let a = Trailer {
        name:    "registry".to_owned(),
        pairing: pairing(),
    };
    let b = Trailer {
        name:    "rest".to_owned(),
        pairing: Pairing {
            hash:   H2.to_owned(),
            time:   T + 60,
            digest: D2.to_owned(),
        },
    };
    let message = format!(
        "fix: a thing\n\nBody text that mentions Mock-Ref in the middle of a line.\n\n{}\n{}\n",
        a.render(),
        b.render()
    );
    assert_eq!(trailers(&message).unwrap(), vec![a, b]);
    assert!(trailers("fix: nothing here\n\nbody\n").unwrap().is_empty());
}

#[test]
fn a_malformed_trailer_and_a_ref_named_twice_are_errors_and_never_skipped() {
    let ok = Trailer {
        name:    "registry".to_owned(),
        pairing: pairing(),
    }
    .render();
    assert!(
        trailers(&format!("subject\n\n{ok}\n")).is_ok(),
        "the control"
    );
    let broken = ok.replacen(H1, &H1[.. 10], 1);
    assert!(trailers(&format!("subject\n\n{broken}\n")).is_err());
    assert!(
        trailers(&format!("subject\n\n{ok}\n{ok}\n"))
            .unwrap_err()
            .contains("registry")
    );
}

fn candidates<'a>(entries: &[(&'a str, i64)]) -> Vec<Candidate<'a>> {
    entries
        .iter()
        .map(|(hash, time)| {
            Candidate {
                hash: *hash,
                time: *time,
            }
        })
        .collect()
}

/// A function giving each commit the digest the table says, and counting the asks.
fn reader<'a>(
    table: &'a [(&'a str, Option<&'a str>)],
    asked: &'a std::cell::RefCell<Vec<String>>,
) -> impl FnMut(&str) -> Option<String> + 'a {
    move |hash| {
        asked.borrow_mut().push(hash.to_owned());
        table
            .iter()
            .find(|(h, _)| *h == hash)
            .and_then(|(_, d)| d.map(str::to_owned))
    }
}

#[test]
fn a_hash_that_is_there_with_the_recorded_files_settles_it_at_once() {
    let asked = std::cell::RefCell::new(Vec::new());
    let cands = candidates(&[(H2, T - 10), (H1, T + 4000), (H3, T)]);
    let table = [(H1, Some(D1)), (H2, Some(D1)), (H3, Some(D1))];
    let r = resolve(
        &pairing(),
        &cands,
        DEFAULT_WINDOW_SECS,
        reader(&table, &asked),
    );
    assert_eq!(r, Resolution::ByHash(H1.to_owned()));
    // only the named commit was read, though two nearer ones hold the same files
    assert_eq!(*asked.borrow(), vec![H1.to_owned()]);
}

#[test]
fn a_hash_that_is_there_with_other_files_is_a_mismatch_and_nothing_else_is_tried() {
    let asked = std::cell::RefCell::new(Vec::new());
    let cands = candidates(&[(H1, T), (H2, T)]);
    // H2 has the recorded digest and is as near as can be: the hash still decides
    let table = [(H1, Some(D2)), (H2, Some(D1))];
    let r = resolve(
        &pairing(),
        &cands,
        DEFAULT_WINDOW_SECS,
        reader(&table, &asked),
    );
    assert_eq!(r, Resolution::Mismatch {
        hash:  H1.to_owned(),
        found: Some(D2.to_owned()),
    });
    assert_eq!(*asked.borrow(), vec![H1.to_owned()]);
    let unreadable = [(H1, None), (H2, Some(D1))];
    let r = resolve(
        &pairing(),
        &cands,
        DEFAULT_WINDOW_SECS,
        reader(&unreadable, &asked),
    );
    assert_eq!(r, Resolution::Mismatch {
        hash:  H1.to_owned(),
        found: None,
    });
}

#[test]
fn a_gone_hash_is_found_by_the_time_and_the_digest_together() {
    let asked = std::cell::RefCell::new(Vec::new());
    // H2 is the rehoused commit: a new hash, 90 seconds off, the recorded files
    let cands = candidates(&[(H2, T + 90), (H3, T + 30)]);
    let table = [(H2, Some(D1)), (H3, Some(D2))];
    let r = resolve(
        &pairing(),
        &cands,
        DEFAULT_WINDOW_SECS,
        reader(&table, &asked),
    );
    assert_eq!(r, Resolution::ByTimeAndDigest(H2.to_owned()));
}

#[test]
fn a_commit_near_in_time_with_other_files_is_not_taken_for_it() {
    // the control for the digest: the time alone would pick H2, being the only one close
    let asked = std::cell::RefCell::new(Vec::new());
    let cands = candidates(&[(H2, T + 1)]);
    let table = [(H2, Some(D2))];
    let r = resolve(
        &pairing(),
        &cands,
        DEFAULT_WINDOW_SECS,
        reader(&table, &asked),
    );
    assert_eq!(r, Resolution::Missing);
}

#[test]
fn a_commit_with_the_recorded_files_far_from_the_recorded_time_is_not_taken_for_it() {
    // the control for the time: the digest alone would pick H2
    let asked = std::cell::RefCell::new(Vec::new());
    let cands = candidates(&[(H2, T + DEFAULT_WINDOW_SECS + 1)]);
    let table = [(H2, Some(D1))];
    let r = resolve(
        &pairing(),
        &cands,
        DEFAULT_WINDOW_SECS,
        reader(&table, &asked),
    );
    assert_eq!(r, Resolution::Missing);
    assert!(
        asked.borrow().is_empty(),
        "a commit outside the window is not even read"
    );
    // and the edge of the window is inside it, on either side
    for edge in [T - DEFAULT_WINDOW_SECS, T + DEFAULT_WINDOW_SECS] {
        let cands = candidates(&[(H2, edge)]);
        let r = resolve(
            &pairing(),
            &cands,
            DEFAULT_WINDOW_SECS,
            reader(&table, &asked),
        );
        assert_eq!(r, Resolution::ByTimeAndDigest(H2.to_owned()), "{edge}");
    }
}

#[test]
fn several_commits_with_the_recorded_files_near_the_time_are_ambiguous_nearest_first() {
    let asked = std::cell::RefCell::new(Vec::new());
    let cands = candidates(&[(H3, T + 500), (H2, T - 20), (H1, T + 20)]);
    // the recorded hash H1 is among them but the pairing below names another, gone one
    let gone = Pairing {
        hash: "9".repeat(40),
        ..pairing()
    };
    let table = [(H1, Some(D1)), (H2, Some(D1)), (H3, Some(D1))];
    let r = resolve(&gone, &cands, DEFAULT_WINDOW_SECS, reader(&table, &asked));
    // H1 and H2 are both 20 seconds off and H1 sorts first by hash, so the order is fixed
    assert_eq!(
        r,
        Resolution::Ambiguous(vec![H1.to_owned(), H2.to_owned(), H3.to_owned()])
    );
}

#[test]
fn no_candidates_and_candidates_nobody_can_read_are_missing() {
    let asked = std::cell::RefCell::new(Vec::new());
    assert_eq!(
        resolve(&pairing(), &[], DEFAULT_WINDOW_SECS, reader(&[], &asked)),
        Resolution::Missing
    );
    let cands = candidates(&[(H2, T)]);
    let table = [(H2, None)];
    assert_eq!(
        resolve(
            &pairing(),
            &cands,
            DEFAULT_WINDOW_SECS,
            reader(&table, &asked)
        ),
        Resolution::Missing
    );
}

#[test]
fn a_digest_made_a_file_at_a_time_is_the_digest_made_all_at_once() {
    let all = files(&[("a/x", b"1"), ("b", b"2"), ("a/y", b"3")]).unwrap();
    let mut one_by_one = TreeHasher::new();
    for (path, content) in [("a/x", &b"1"[..]), ("a/y", b"3"), ("b", b"2")] {
        one_by_one.file(path, content).unwrap();
    }
    assert_eq!(one_by_one.finish(), all);
    // the control: the same files in another order are the same set but not an ascending one
    let mut shuffled = TreeHasher::new();
    shuffled.file("b", b"2").unwrap();
    assert!(
        shuffled
            .file("a/x", b"1")
            .unwrap_err()
            .contains("ascending")
    );
}

#[test]
fn a_file_at_a_time_refuses_a_path_given_twice_and_a_path_that_is_not_relative() {
    let mut hasher = TreeHasher::new();
    hasher.file("a", b"1").unwrap();
    assert!(hasher.file("a", b"1").unwrap_err().contains("twice"));
    assert!(hasher.file("../b", b"").unwrap_err().contains("relative"));
    // the refused ones were not taken in: the next ascending file still goes
    hasher.file("b", b"2").unwrap();
    assert_eq!(hasher.finish(), files(&[("a", b"1"), ("b", b"2")]).unwrap());
}

// The expected digests below were made outside this code: the bytes written with `printf` as the
// doc of `tree_digest` describes them (the tag and a NUL, then for each file the path's length as
// 8 little-endian bytes, the path, the content's length as 8 little-endian bytes, the content)
// and hashed with `sha256sum`. A change to the encoding changes them.
#[test]
fn the_digest_is_the_one_a_stream_written_by_hand_gives() {
    assert_eq!(
        files(&[]).unwrap(),
        "f3d4e777047927c3c0e8a2fcac7e30e29104419ee4c2328eee6e7aadd126112a"
    );
    assert_eq!(
        files(&[("b/c.toml", b"yy"), ("a.toml", b"x\n")]).unwrap(),
        "c3dfe547838b57615041c99ee22f2408cf753eb5ab3963a00d88b7278382e504"
    );
    assert_eq!(
        files(&[("e", b"")]).unwrap(),
        "2c183f55ec8c36836cd5b92de2a4c67fe463100b8c4f4414faeb3fcf5ee6e6a5"
    );
}

#[test]
fn a_file_that_is_also_a_directory_is_refused_however_far_apart_they_sort() {
    let err = files(&[("a", b"1"), ("a/b", b"2")]).unwrap_err();
    assert!(err.contains("`a` is a file"), "{err}");
    // `a-b` sorts between `a` and `a/b`, so remembering only the last path would miss it
    let err = files(&[("a", b"1"), ("a-b", b"3"), ("a/b", b"2")]).unwrap_err();
    assert!(err.contains("`a` is a file"), "{err}");
    let mut hasher = TreeHasher::new();
    hasher.file("a", b"1").unwrap();
    hasher.file("a-b", b"3").unwrap();
    assert!(hasher.file("a/b", b"2").is_err());
    // the controls: names that only start the same way
    assert!(files(&[("a-b", b"3"), ("a/b", b"2")]).is_ok());
    assert!(files(&[("a.toml", b"1"), ("a/b", b"2")]).is_ok());
    assert!(files(&[("a/b", b"1"), ("a/c/d", b"2")]).is_ok());
}

#[test]
fn a_pairing_is_only_made_of_a_real_hash_a_real_digest_and_a_time_that_has_a_year() {
    assert!(Pairing::new(H1, T, D1).is_ok());
    for (hash, time, digest, what) in [
        (&H1[.. 39], T, D1, "a short hash"),
        (
            "0123456789ABCDEF0123456789abcdef01234567",
            T,
            D1,
            "an uppercase hash",
        ),
        (H1, T, &D1[.. 63], "a short digest"),
        (H1, -62_167_219_201, D1, "a time before year 0000"),
        (H1, 253_402_300_800, D1, "a time in year 10000"),
    ] {
        assert!(Pairing::new(hash, time, digest).is_err(), "{what}");
    }
    // the controls: the first and the last second that have four-digit years
    assert!(Pairing::new(H1, -62_167_219_200, D1).is_ok());
    assert!(Pairing::new(H1, 253_402_300_799, D1).is_ok());
    // and whatever is made, renders to a line that reads back
    for time in [-62_167_219_200, -1, 0, T, 253_402_300_799] {
        let made = Pairing::new(H2, time, D2).unwrap();
        assert_eq!(Pairing::parse(&made.render()), Ok(made));
    }
}

#[test]
fn a_trailer_is_only_made_for_a_name_a_ref_can_have() {
    let p = Pairing::new(H1, T, D1).unwrap();
    assert!(Trailer::new("registry", p.clone()).is_ok());
    for bad in ["", "Registry", "-registry", "_x", "a b", "a/b", "a.b"] {
        assert!(Trailer::new(bad, p.clone()).is_err(), "{bad:?}");
    }
    const _: () = assert!(is_ref_name("registry") && is_ref_name("a-b_c9") && !is_ref_name("-x"));
}
