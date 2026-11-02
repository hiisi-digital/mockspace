//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The test history on a ref: the round trip, a lost race, the guard of the writer, and a
//! history that cannot be fetched. Real git and a real bare repository.

use std::collections::BTreeMap;

use mockspace_store::{RefPair, RefStore, RetryPolicy, StoreError};

use super::*;
use crate::config::Config;
use crate::suite::history::{Green, TestRecord};
use crate::test_remote::{Remote, git};

const REF: &str = "refs/heads/mock/test-history";

fn config_text(history: bool) -> String {
    let table = if history { "\n[test.history]\n" } else { "" };
    format!(
        "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n\n[test]\nheavy_after_secs = 1.0\n{table}"
    )
}

fn project(remote: &Remote, name: &str, history: bool) -> Config {
    let root = remote.clone_dir(name);
    std::fs::write(root.join("mock/mockspace.toml"), config_text(history)).unwrap();
    Config::from_dir(&root.join("mock"))
}

fn store(cfg: &Config) -> RefStore {
    let pair = RefPair::branch_form("test-history").unwrap();
    RefStore::new(&cfg.repo_root, "origin", pair).with_retry(RetryPolicy::immediate(4))
}

/// A history of `member` with a green at `green` (none when empty) and the tests `named`.
fn hist(member: &str, green: &str, named: &[&str]) -> History {
    let mut h = History {
        version: VERSION,
        ..History::default()
    };
    let f = h.flavour_mut("");
    if !green.is_empty() {
        f.green.insert(member.into(), Green {
            full: green.into(),
            own:  BTreeMap::new(),
        });
    }
    let bin = format!("{member}::bin");
    for name in named {
        f.tests
            .entry(bin.clone())
            .or_default()
            .insert((*name).to_string(), TestRecord {
                secs:      5.0,
                passed:    true,
                passed_at: Some(green.into()),
            });
    }
    h
}

fn scope<'a>(ran: &'a [String], all: &'a [String]) -> Scope<'a> {
    Scope {
        flavour: "",
        ran,
        workspace: all,
    }
}

fn members(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn a_save_to_a_ref_reads_back_whole() {
    let remote = Remote::new();
    let cfg = project(&remote, "a", true);
    let s = opened(&cfg);
    let m = members(&["m1"]);
    let h = hist("m1", "fp1", &["slow"]);
    let applied = h.save_to(&s, &scope(&m, &m), None, "test history").unwrap();
    assert!(applied.commit.is_some() && applied.changed);
    assert!(
        remote
            .file(REF, "default/m1.json")
            .unwrap()
            .contains("\"fp1\"")
    );
    let back = History::load_from(&s).unwrap();
    assert_eq!(back, h);
    let again = h.save_to(&s, &scope(&m, &m), None, "test history").unwrap();
    assert!(
        !again.changed,
        "a run that settled nothing writes no commit"
    );
}

#[test]
fn a_lost_race_is_resolved_by_the_history_merge() {
    let remote = Remote::new();
    let (a, b) = (project(&remote, "a", true), project(&remote, "b", true));
    let (sa, sb) = (opened(&a), opened(&b));
    let m = members(&["m1"]);
    hist("m1", "fp1", &["first"])
        .save_to(&sa, &scope(&m, &m), None, "seed")
        .unwrap();
    sb.refresh().unwrap();
    // B reads the history as it is; A then records one more test; B records another on what it read.
    let seen = History::load_from(&sb).unwrap();
    assert_eq!(seen, hist("m1", "fp1", &["first"]));
    hist("m1", "fp1", &["first", "from_a"])
        .save_to(&sa, &scope(&m, &m), None, "a")
        .unwrap();
    hist("m1", "fp1", &["first", "from_b"])
        .save_to(&sb, &scope(&m, &m), None, "b")
        .unwrap();
    let merged = History::load_from(&sa)
        .and_then(|_| {
            sa.refresh()?;
            History::load_from(&sa)
        })
        .unwrap();
    let names: Vec<&String> = merged.flavour("").unwrap().tests["m1::bin"]
        .keys()
        .collect();
    assert_eq!(
        names,
        ["first", "from_a", "from_b"],
        "the record of each side stays"
    );
}

#[test]
fn a_green_both_sides_moved_is_dropped_by_the_rule_and_the_write_lands() {
    let remote = Remote::new();
    let (a, b) = (project(&remote, "a", true), project(&remote, "b", true));
    let (sa, sb) = (opened(&a), opened(&b));
    let m = members(&["m1"]);
    hist("m1", "fp1", &["t"])
        .save_to(&sa, &scope(&m, &m), None, "seed")
        .unwrap();
    sb.refresh().unwrap();
    History::load_from(&sb).unwrap();
    hist("m1", "fp2", &["t"])
        .save_to(&sa, &scope(&m, &m), None, "a")
        .unwrap();
    hist("m1", "fp3", &["t"])
        .save_to(&sb, &scope(&m, &m), None, "b")
        .unwrap();
    sa.refresh().unwrap();
    let after = History::load_from(&sa).unwrap();
    assert!(
        after.flavour("").unwrap().green.is_empty(),
        "moved to two values: the next run records it"
    );
}

#[test]
fn a_green_a_member_that_did_not_run_holds_survives_a_save_and_a_lost_race() {
    let remote = Remote::new();
    let (a, b) = (project(&remote, "a", true), project(&remote, "b", true));
    let (sa, sb) = (opened(&a), opened(&b));
    let both = members(&["m1", "m2"]);
    let mut seed = hist("m1", "fp1", &["t"]);
    seed.flavour_mut("").green.insert("m2".into(), Green {
        full: "fp2".into(),
        own:  BTreeMap::new(),
    });
    seed.save_to(&sa, &scope(&both, &both), None, "seed")
        .unwrap();
    sb.refresh().unwrap();
    let m2 = remote.file(REF, "default/m2.json").unwrap();
    let only_m1 = members(&["m1"]);
    // B writes a history that has no green for m2 at all, covering m1 only; A moves m1 first.
    History::load_from(&sb).unwrap();
    hist("m1", "fp1", &["t", "from_a"])
        .save_to(&sa, &scope(&only_m1, &both), None, "a")
        .unwrap();
    hist("m1", "fp1", &["t", "from_b"])
        .save_to(&sb, &scope(&only_m1, &both), None, "b")
        .unwrap();
    assert_eq!(
        remote.file(REF, "default/m2.json").unwrap(),
        m2,
        "m2 did not run: its file is untouched"
    );
    // the control: the same save with m2 covered does drop it
    hist("m1", "fp1", &["t"])
        .save_to(&sa, &scope(&both, &both), None, "covered")
        .unwrap();
    assert_ne!(remote.file(REF, "default/m2.json"), Some(m2));
}

#[test]
fn a_member_whose_suite_failed_loses_its_green_and_the_write_lands() {
    let remote = Remote::new();
    let cfg = project(&remote, "a", true);
    let s = opened(&cfg);
    let m = members(&["m1"]);
    hist("m1", "fp1", &["t"])
        .save_to(&s, &scope(&m, &m), None, "seed")
        .unwrap();
    hist("m1", "", &["t"])
        .save_to(&s, &scope(&m, &m), None, "failed")
        .unwrap();
    assert!(
        History::load_from(&s)
            .unwrap()
            .flavour("")
            .unwrap()
            .green
            .is_empty()
    );
}

#[test]
fn a_history_that_cannot_be_fetched_is_an_error_and_never_an_empty_history() {
    let remote = Remote::new();
    let cfg = project(&remote, "a", true);
    git(&cfg.repo_root, &[
        "remote",
        "set-url",
        "origin",
        remote.tmp.path().join("gone.git").to_str().unwrap(),
    ]);
    let s = store(&cfg);
    assert!(matches!(s.refresh().unwrap_err(), StoreError::Fetch { .. }));
    // unfetched, it does not read as empty either
    assert!(matches!(
        History::load_from(&s).unwrap_err(),
        StoreError::NotFetched { .. }
    ));
    let m = members(&["m1"]);
    assert!(
        hist("m1", "fp1", &["t"])
            .save_to(&s, &scope(&m, &m), None, "x")
            .is_err()
    );
    // the run: no history and no store to write to, said aloud
    let (h, kept) = History::open(&cfg);
    assert!(
        kept.is_none(),
        "an unreachable remote leaves the run nothing to write to"
    );
    assert_eq!(h, History {
        version: VERSION,
        ..History::default()
    });
    let (h, kept) = History::open(&project(&Remote::new(), "never_pushed", true));
    assert!(
        kept.is_some(),
        "control: a remote that answers with nothing is an empty history"
    );
    assert_eq!(h, History {
        version: VERSION,
        ..History::default()
    });
}

#[test]
fn the_directory_stays_the_default_and_opens_as_load_does() {
    let remote = Remote::new();
    let cfg = project(&remote, "a", false);
    assert!(cfg.test.history.is_none());
    let m = members(&["m1"]);
    let h = hist("m1", "fp1", &["slow"]);
    h.save(&cfg.mock_dir, &scope(&m, &m)).unwrap();
    let (opened, kept) = History::open(&cfg);
    assert!(kept.is_some());
    assert_eq!(opened, h);
    assert!(cfg.mock_dir.join("test-history/default/m1.json").is_file());
    assert_eq!(remote.tip(REF), None, "nothing went near a remote");
}

/// A store fetched once, as `History::open` leaves one.
fn opened(cfg: &Config) -> RefStore {
    let s = store(cfg);
    s.refresh().unwrap();
    s
}

fn now(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(m, f)| (m.to_string(), f.to_string()))
        .collect()
}

#[test]
fn a_green_removed_while_the_inputs_have_not_moved_is_refused_and_never_lands() {
    let remote = Remote::new();
    let cfg = project(&remote, "a", true);
    let s = opened(&cfg);
    let m = members(&["m1"]);
    hist("m1", "fp1", &["t"])
        .save_to(&s, &scope(&m, &m), Some(&now(&[("m1", "fp1")])), "seed")
        .unwrap();
    let tip = remote.tip(REF);
    // the run failed on a tree whose fingerprint for m1 is still the one its green was made at
    let err = hist("m1", "", &["t"])
        .save_to(&s, &scope(&m, &m), Some(&now(&[("m1", "fp1")])), "failed")
        .unwrap_err();
    assert!(
        matches!(&err, HistoryError::Nulled(f) if f == &["m1"]),
        "{err}"
    );
    assert_eq!(
        remote.tip(REF),
        tip,
        "a refused write leaves the ref where it was"
    );
    // the control: moved inputs excuse it
    hist("m1", "", &["t"])
        .save_to(&s, &scope(&m, &m), Some(&now(&[("m1", "fp2")])), "moved")
        .unwrap();
    assert!(
        History::load_from(&s)
            .unwrap()
            .flavour("")
            .unwrap()
            .green
            .is_empty()
    );
}

#[test]
fn a_green_the_winner_recorded_survives_a_write_that_never_saw_it() {
    let remote = Remote::new();
    let (a, b) = (project(&remote, "a", true), project(&remote, "b", true));
    let (sa, sb) = (opened(&a), opened(&b));
    let m = members(&["m1"]);
    hist("m1", "", &["t"])
        .save_to(&sa, &scope(&m, &m), None, "seed")
        .unwrap();
    sb.refresh().unwrap();
    History::load_from(&sb).unwrap();
    // A records the green at the fingerprint both now hold; B, on the history it read, saves a failure
    hist("m1", "fp1", &["t"])
        .save_to(&sa, &scope(&m, &m), Some(&now(&[("m1", "fp1")])), "a")
        .unwrap();
    hist("m1", "", &["t", "x"])
        .save_to(&sb, &scope(&m, &m), Some(&now(&[("m1", "fp1")])), "b")
        .unwrap();
    sa.refresh().unwrap();
    let after = History::load_from(&sa).unwrap();
    assert_eq!(
        after.flavour("").unwrap().green["m1"].full,
        "fp1",
        "the merge keeps the winner's green"
    );
}

#[test]
fn a_green_the_winner_recorded_at_our_fingerprint_is_not_dropped_by_the_rebase() {
    let remote = Remote::new();
    let (a, b) = (project(&remote, "a", true), project(&remote, "b", true));
    let (sa, sb) = (opened(&a), opened(&b));
    let m = members(&["m1"]);
    hist("m1", "fp0", &["t"])
        .save_to(&sa, &scope(&m, &m), None, "seed")
        .unwrap();
    sb.refresh().unwrap();
    History::load_from(&sb).unwrap();
    // A passes on the tree both now hold and records the green; B, flaky on the same tree, fails.
    // B's guard sees the older green it read, which a moved fingerprint excuses; only the rebase
    // meets the green at the fingerprint B still has.
    hist("m1", "fp1", &["t"])
        .save_to(&sa, &scope(&m, &m), Some(&now(&[("m1", "fp1")])), "a")
        .unwrap();
    let tip = remote.tip(REF);
    let err = hist("m1", "", &["t"])
        .save_to(&sb, &scope(&m, &m), Some(&now(&[("m1", "fp1")])), "b")
        .unwrap_err();
    assert!(err.to_string().contains("m1"), "{err}");
    assert_eq!(remote.tip(REF), tip, "nothing of B's was pushed");
    sa.refresh().unwrap();
    let after = History::load_from(&sa).unwrap();
    assert_eq!(after.flavour("").unwrap().green["m1"].full, "fp1");
}

fn fingerprints(pairs: &[(&str, &str)]) -> crate::suite::fingerprint::Fingerprints {
    crate::suite::fingerprint::Fingerprints {
        full: now(pairs),
        ..Default::default()
    }
}

#[test]
fn a_run_keeps_its_history_under_the_guard_when_the_history_is_declared() {
    let remote = Remote::new();
    let cfg = project(&remote, "a", true);
    let (_, kept) = History::open(&cfg);
    let kept = kept.expect("a declared history opens a store");
    let m = members(&["m1"]);
    let fp = fingerprints(&[("m1", "fp1")]);
    hist("m1", "fp1", &["t"])
        .keep(&cfg, kept.as_ref(), &scope(&m, &m), &fp)
        .unwrap();
    let tip = remote.tip(REF);
    // the run failed on a tree whose fingerprint is still the one the green was made at
    let err = hist("m1", "", &["t"])
        .keep(&cfg, kept.as_ref(), &scope(&m, &m), &fp)
        .unwrap_err();
    assert!(
        matches!(&err, HistoryError::Nulled(f) if f == &["m1"]),
        "{err}"
    );
    assert_eq!(remote.tip(REF), tip, "the refused write never reached the ref");
}

#[test]
fn a_run_keeps_its_history_without_the_guard_when_none_is_declared() {
    let remote = Remote::new();
    let cfg = project(&remote, "a", false);
    let (_, kept) = History::open(&cfg);
    let kept = kept.expect("the directory opens");
    let m = members(&["m1"]);
    let fp = fingerprints(&[("m1", "fp1")]);
    hist("m1", "fp1", &["t"])
        .keep(&cfg, kept.as_ref(), &scope(&m, &m), &fp)
        .unwrap();
    // the commit guard is the directory's, so the same removal lands here
    hist("m1", "", &["t"])
        .keep(&cfg, kept.as_ref(), &scope(&m, &m), &fp)
        .unwrap();
    let back = History::load_from(kept.as_ref()).unwrap();
    assert!(back.flavour("").unwrap().green.is_empty());
}
