//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn seat(number: u32) -> Seat {
    Seat {
        number,
        persona: "persona".to_string(),
        topic: "topic".to_string(),
        minted_at_unix: 0,
    }
}

// -- seat numbering -------------------------------------------------

#[test]
fn the_first_seat_of_an_empty_panel_is_one() {
    assert_eq!(next_seat_number(&PanelInventory::default()), Some(1));
}

#[test]
fn a_seat_is_always_one_past_the_highest_minted() {
    let mut inv = PanelInventory::default();
    inv.seats.push(seat(1));
    inv.seats.push(seat(2));
    assert_eq!(next_seat_number(&inv), Some(3));
}

#[test]
fn seat_99_may_mint_and_seat_100_may_not() {
    // The case that must fail: 99 is the LAST seat, not the first one
    // refused. An off-by-one here would refuse 99 or admit 100.
    let mut inv = PanelInventory::default();
    for n in 1 ..= 98 {
        inv.seats.push(seat(n));
    }
    assert_eq!(
        next_seat_number(&inv),
        Some(99),
        "seat 99 must still be mintable"
    );
    inv.seats.push(seat(99));
    assert_eq!(
        next_seat_number(&inv),
        None,
        "seat 100 must never be offered"
    );
}

#[test]
fn mint_seat_refuses_at_the_cap_regardless_of_cadence() {
    let mut inv = PanelInventory::default();
    for n in 1 ..= 99 {
        inv.seats.push(seat(n));
    }
    // cadence 0 disables the cadence check entirely, isolating this
    // assertion to the cap alone.
    assert_eq!(
        mint_seat(&mut inv, "p", "t", NO_GATE, 0),
        Err(MintRefusal::SeatCapReached)
    );
    assert_eq!(
        inv.seats.len(),
        99,
        "a refused mint must not append anything"
    );
}

/// A cadence larger than any test here mints, for the tests whose subject
/// is not the cadence. Zero used to serve that purpose and no longer can,
/// because zero is refused.
const NO_GATE: u32 = 1000;

// -- consolidation cadence -------------------------------------------

// `cadence_zero_never_demands_a_consolidation` stood here and asserted
// that a cadence of zero mints forever. That is the escape hatch rather
// than a feature, and `a_cadence_of_zero_is_refused` below replaces it
// with the opposite assertion.

#[test]
fn minting_is_refused_once_the_cadence_is_met_and_recovers_after_consolidating() {
    let mut inv = PanelInventory::default();
    for _ in 0 .. 3 {
        mint_seat(&mut inv, "p", "t", 3, 0).unwrap();
    }
    let refusal = mint_seat(&mut inv, "p", "t", 3, 0);
    assert_eq!(
        refusal,
        Err(MintRefusal::ConsolidationDue {
            minted:  3,
            cadence: 3,
        })
    );
    assert_eq!(
        inv.seats.len(),
        3,
        "the refused seat must not have been appended"
    );

    assert_eq!(consolidate(&mut inv, "converged", 100), Some(3));
    // and minting resumes
    assert_eq!(mint_seat(&mut inv, "p", "t", 3, 0), Ok(4));
}

#[test]
fn a_panels_own_cadence_overrides_the_project_default() {
    let mut inv = PanelInventory {
        consolidate_every: Some(1),
        ..PanelInventory::default()
    };
    mint_seat(&mut inv, "p", "t", 10, 0).unwrap();
    // the project default (10) would still allow this; the panel's own
    // override (1) must be what actually governs
    assert_eq!(
        mint_seat(&mut inv, "p", "t", 10, 0),
        Err(MintRefusal::ConsolidationDue {
            minted:  1,
            cadence: 1,
        })
    );
}

// -- consolidation bookkeeping ----------------------------------------

#[test]
fn consolidating_an_empty_panel_records_nothing() {
    let mut inv = PanelInventory::default();
    assert_eq!(consolidate(&mut inv, "n", 0), None);
    assert!(inv.consolidations.is_empty());
}

#[test]
fn consolidating_twice_with_no_new_seats_records_nothing_the_second_time() {
    let mut inv = PanelInventory::default();
    mint_seat(&mut inv, "p", "t", NO_GATE, 0).unwrap();
    assert_eq!(consolidate(&mut inv, "first", 0), Some(1));
    assert_eq!(
        consolidate(&mut inv, "second", 0),
        None,
        "nothing new since the first"
    );
    assert_eq!(
        inv.consolidations.len(),
        1,
        "a no-op consolidation must not append"
    );
}

// -- open / closed -----------------------------------------------------

#[test]
fn an_empty_panel_is_not_open() {
    assert!(!is_open(&PanelInventory::default()));
}

#[test]
fn a_panel_is_open_the_moment_it_has_an_unconsolidated_seat() {
    let mut inv = PanelInventory::default();
    mint_seat(&mut inv, "p", "t", NO_GATE, 0).unwrap();
    assert!(is_open(&inv));
    consolidate(&mut inv, "n", 0);
    assert!(!is_open(&inv), "consolidating must close it");
    mint_seat(&mut inv, "p", "t", NO_GATE, 0).unwrap();
    assert!(is_open(&inv), "a new seat after consolidation reopens it");
}

// -- round-trip through the file format --------------------------------

#[test]
fn a_saved_inventory_loads_back_identical() {
    let tmp = tempfile::tempdir().unwrap();
    let path = inventory_path(tmp.path(), "vehje-ir");
    let mut inv = PanelInventory {
        slug: "vehje-ir".to_string(),
        ..PanelInventory::default()
    };
    mint_seat(&mut inv, "chris-fallin", "lowering", 10, 111).unwrap();
    mint_seat(&mut inv, "xavier-leroy", "verification", 10, 222).unwrap();
    consolidate(&mut inv, "converged on X, open question Y", 333).unwrap();

    save(&path, &inv).unwrap();
    let loaded = load(&path, "vehje-ir").unwrap();
    assert_eq!(loaded, inv);
}

#[test]
fn loading_an_absent_file_is_an_empty_panel_named_from_the_slug() {
    let tmp = tempfile::tempdir().unwrap();
    let path = inventory_path(tmp.path(), "brand-new");
    let inv = load(&path, "brand-new").unwrap();
    assert_eq!(inv.slug, "brand-new");
    assert!(inv.seats.is_empty());
}

#[test]
fn load_all_finds_every_panel_and_none_that_are_not_there() {
    let tmp = tempfile::tempdir().unwrap();
    let mut a = PanelInventory {
        slug: "alpha".to_string(),
        ..PanelInventory::default()
    };
    mint_seat(&mut a, "p", "t", NO_GATE, 0).unwrap();
    save(&inventory_path(tmp.path(), "alpha"), &a).unwrap();

    let b = PanelInventory {
        slug: "beta".to_string(),
        ..PanelInventory::default()
    };
    save(&inventory_path(tmp.path(), "beta"), &b).unwrap();

    let all = load_all(tmp.path());
    assert_eq!(all.len(), 2);
    let slugs: Vec<&str> = all.iter().map(|i| i.slug.as_str()).collect();
    assert!(slugs.contains(&"alpha"));
    assert!(slugs.contains(&"beta"));
}

#[test]
fn load_all_on_a_project_with_no_panel_directory_is_empty_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(load_all(tmp.path()), Vec::new());
}

/// Concurrent mints take distinct seats and none is lost.
///
/// **This is the reproduction, kept.** Before the lock, two `mock panel seat`
/// invocations racing each other both printed "seat 2/99 minted" and the file
/// afterwards held one of them: two agents told they hold the same seat, and a
/// ledger that had silently dropped a record while reporting success. Which of
/// the two survived differed between runs.
///
/// Eight threads rather than two, because a two-thread race reproduces
/// intermittently and this must fail every time the lock is removed.
#[test]
fn concurrent_mints_take_distinct_seats_and_none_is_lost() {
    let tmp = tempfile::tempdir().unwrap();
    let path = inventory_path(tmp.path(), "race");
    // A cadence past the number of seats, so this measures the lock rather
    // than tripping the consolidation gate partway through.
    const N: u32 = 8;

    let taken: Vec<u32> = std::thread::scope(|s| {
        let handles: Vec<_> = (0 .. N)
            .map(|i| {
                let path = path.clone();
                s.spawn(move || {
                    with_locked(&path, "race", |inv| {
                        mint_seat(inv, &format!("p{i}"), "t", N + 1, 0).map_err(|r| r.to_string())
                    })
                })
            })
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().unwrap().ok())
            .collect()
    });

    let mut sorted = taken.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        taken.len(),
        "two threads were told they hold the same seat: {taken:?}"
    );
    assert_eq!(taken.len(), N as usize, "a mint failed: {taken:?}");

    let inv = load(&path, "race").unwrap();
    assert_eq!(
        inv.seats.len(),
        N as usize,
        "the ledger lost a record it had reported minting"
    );
    let mut numbers: Vec<u32> = inv.seats.iter().map(|s| s.number).collect();
    numbers.sort_unstable();
    assert_eq!(
        numbers,
        (1 ..= N).collect::<Vec<_>>(),
        "the seats are not one through {N} with no gaps"
    );
}

/// A cadence of zero is refused rather than read as no enforcement.
///
/// The seat cap is a `const` on the stated grounds that letting a project raise
/// it would let the failure mode configure its own escape hatch. A cadence a
/// panel can set to zero, in the file this tool rewrites for it, is that escape
/// hatch and it is one line long.
#[test]
fn a_cadence_of_zero_is_refused() {
    let mut inv = PanelInventory {
        slug: "p".into(),
        ..PanelInventory::default()
    };
    assert!(matches!(
        mint_seat(&mut inv, "a", "t", 0, 0),
        Err(MintRefusal::CadenceDisabled)
    ));

    // And through the panel's own override, which is the reachable half: the
    // project default is in a config a person edits, the override is in a file
    // the tool itself writes.
    let mut own = PanelInventory {
        slug: "p".into(),
        consolidate_every: Some(0),
        ..PanelInventory::default()
    };
    assert!(matches!(
        mint_seat(&mut own, "a", "t", 10, 0),
        Err(MintRefusal::CadenceDisabled)
    ));
}

/// The control: an ordinary cadence still mints, so the refusal above is not
/// equally consistent with a gate that refuses everything.
#[test]
fn an_ordinary_cadence_still_mints() {
    let mut inv = PanelInventory {
        slug: "p".into(),
        ..PanelInventory::default()
    };
    assert_eq!(mint_seat(&mut inv, "a", "t", 2, 0), Ok(1));
    assert_eq!(mint_seat(&mut inv, "b", "t", 2, 0), Ok(2));
    assert!(matches!(
        mint_seat(&mut inv, "c", "t", 2, 0),
        Err(MintRefusal::ConsolidationDue { .. })
    ));
}
