//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The registry on a ref: a write that lands, a lost race, and a fetch that fails. Real git and
//! a real bare repository throughout.

use mockspace_store::StoreError;

use super::*;
use crate::test_world::*;
use mockspace_store::test_support::git;

#[test]
fn a_write_lands_on_the_ref_and_reads_back_through_the_loader() {
    let w = World::new();
    let cfg = w.env_of("a", true);
    let s = stores(&cfg);
    let applied = RegistryWriter::new(&cfg, &s)
        .write(&put("ticket/t.toml", &row("one", "One")), "add one")
        .unwrap();
    assert!(applied.changed && applied.commit.is_some());
    assert!(w.remote_file("t.toml").unwrap().contains("id = \"one\""));
    let reg = load_registry_from(&s, &cfg.namespaces, None).unwrap();
    assert_eq!(ids(&reg), ["ticket::one"]);
    assert_eq!(
        reg.rows["ticket::one"].source,
        cfg.mock_dir.join("registry/ticket/t.toml"),
        "a row says it was declared where it would sit in the tree, whichever store holds it"
    );
}

#[test]
fn a_lost_race_is_resolved_by_the_registry_merge_for_rows_added_on_both_sides() {
    let w = World::new();
    let (a, b) = (w.env_of("a", true), w.env_of("b", true));
    let (sa, sb) = (stores(&a), stores(&b));
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/t.toml", &row("first", "First")), "seed")
        .unwrap();
    // B reads the registry as it is, then A writes, then B writes on what it read.
    let seen = load_registry_from(&sb, &b.namespaces, None).unwrap();
    assert_eq!(ids(&seen), ["ticket::first"]);
    let ours_a = format!("{}{}", row("first", "First"), row("from_a", "A"));
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/t.toml", &ours_a), "a adds")
        .unwrap();
    let ours_b = format!("{}{}", row("first", "First"), row("from_b", "B"));
    RegistryWriter::new(&b, &sb)
        .write(&put("ticket/t.toml", &ours_b), "b adds")
        .unwrap();
    let text = w.remote_file("t.toml").unwrap();
    for id in ["first", "from_a", "from_b"] {
        assert!(
            text.contains(&format!("id = \"{id}\"")),
            "{id} is missing from:\n{text}"
        );
    }
}

#[test]
fn a_row_changed_two_ways_is_not_merged_and_the_winner_stands() {
    let w = World::new();
    let (a, b) = (w.env_of("a", true), w.env_of("b", true));
    let (sa, sb) = (stores(&a), stores(&b));
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/t.toml", &row("x", "Base")), "seed")
        .unwrap();
    load_registry_from(&sb, &b.namespaces, None).unwrap();
    RegistryWriter::new(&a, &sa)
        .write(&put("ticket/t.toml", &row("x", "From A")), "a")
        .unwrap();
    let tip = w.remote_ref();
    let err = RegistryWriter::new(&b, &sb)
        .write(&put("ticket/t.toml", &row("x", "From B")), "b")
        .unwrap_err();
    assert!(
        matches!(err, RegistryStoreError::Store {
            source: StoreError::Merge { .. },
            ..
        }),
        "{err}"
    );
    assert_eq!(
        w.remote_ref(),
        tip,
        "the refused write left the remote where the winner put it"
    );
}

#[test]
fn a_failed_fetch_is_an_error_and_never_an_empty_registry() {
    let w = World::new();
    let cfg = w.env_of("a", true);
    git(&cfg.repo_root, &[
        "remote",
        "set-url",
        "origin",
        w.tmp.join("gone.git").to_str().unwrap(),
    ]);
    let s = stores(&cfg);
    let err = load_registry_from(&s, &cfg.namespaces, None).unwrap_err();
    assert!(
        matches!(err, RegistryStoreError::Store {
            source: StoreError::Fetch { .. },
            ..
        }),
        "{err}"
    );
    let err = RegistryWriter::new(&cfg, &s)
        .write(&put("ticket/t.toml", &row("x", "X")), "m")
        .unwrap_err();
    assert!(matches!(err, RegistryStoreError::Store { .. }), "{err}");
    // the control: a remote that answers, with nothing on the ref, is an empty registry
    let w2 = World::new();
    let ok = w2.env_of("ok", true);
    assert_eq!(
        load_registry_from(&stores(&ok), &ok.namespaces, None)
            .unwrap()
            .rows
            .len(),
        0
    );
}

#[test]
fn a_failed_fetch_reaches_the_callers_as_an_error_too() {
    let w = World::new();
    let cfg = w.env_of("a", true);
    git(&cfg.repo_root, &[
        "remote",
        "set-url",
        "origin",
        w.tmp.join("gone.git").to_str().unwrap(),
    ]);
    assert!(
        matches!(
            load_registry_for(&cfg),
            Err(RegistryStoreError::Store {
                source: StoreError::Fetch { .. },
                ..
            })
        ),
        "the engine's load"
    );
    let stores = cfg.registry_stores().unwrap();
    assert!(
        load_registry_for_expr_from(&stores, &cfg.namespaces, "ticket").is_err(),
        "the query's load"
    );
    // the control: the same project with a reachable remote loads
    let fine = World::new();
    let ok = fine.env_of("b", true);
    assert!(load_registry_for(&ok).is_ok());
}

