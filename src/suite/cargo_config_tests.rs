//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;
use crate::suite::debug::{Reason, Symbols};
use crate::suite::settings::DebugLevel;

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn symbols(lean: bool, crates: &[(&str, Reason)]) -> Symbols {
    Symbols {
        lean,
        level: DebugLevel::LineTablesOnly,
        everywhere: false,
        off: false,
        crates: crates.iter().map(|(c, r)| (c.to_string(), *r)).collect(),
    }
}

fn doc(t: &str) -> toml_edit::DocumentMut {
    t.parse()
        .unwrap_or_else(|e| panic!("not TOML, or a table twice: {e}\n{t}"))
}

fn debug_of(d: &toml_edit::DocumentMut, profile: &str, member: &str) -> Option<String> {
    let v = &d["profile"][profile]["package"][member]["debug"];
    v.as_integer()
        .map(|n| n.to_string())
        .or_else(|| v.as_str().map(String::from))
}

fn levels(pairs: &[(&str, &str)]) -> TestSettings {
    TestSettings {
        opt_level: pairs
            .iter()
            .map(|(m, l)| (m.to_string(), l.parse().unwrap()))
            .collect(),
        ..TestSettings::default()
    }
}

#[test]
fn each_member_gets_its_own_opt_level_under_the_profile_built() {
    let s = levels(&[("kaski-village", "3"), ("kaski-render", "s")]);
    let c = render(&s, "dev", None, None).unwrap();
    assert!(
        c.contains("[profile.dev.package.\"kaski-village\"]\nopt-level = 3\n"),
        "{c}"
    );
    assert!(
        c.contains("[profile.dev.package.\"kaski-render\"]\nopt-level = \"s\"\n"),
        "{c}"
    );
    assert!(!c.contains("kaski-web"), "{c}");
    let doc = c.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        doc["profile"]["dev"]["package"]["kaski-village"]["opt-level"].as_integer(),
        Some(3)
    );
    let r = render(&s, "release", None, None).unwrap();
    assert!(
        r.contains("[profile.release.package.\"kaski-village\"]"),
        "{r}"
    );
    assert!(!r.contains("profile.dev"), "{r}");
}

#[test]
fn a_name_that_is_no_member_is_left_out_of_the_cargo_config() {
    let s = levels(&[("kaski-village", "3"), ("gone", "2")]);
    let members = vec!["kaski-village".to_string(), "kaski-web".to_string()];
    let c = render(&s, "dev", Some(&members), None).unwrap();
    assert!(c.contains("kaski-village"), "{c}");
    assert!(!c.contains("gone"), "{c}");
    // Nothing left is no config, not a file of comments.
    let only_gone = levels(&[("gone", "2")]);
    assert_eq!(render(&only_gone, "dev", Some(&members), None), None);
    // Where the workspace could not be read, nothing can be ruled out.
    assert!(
        render(&only_gone, "dev", None, None)
            .unwrap()
            .contains("gone")
    );
}

#[test]
fn no_member_opt_level_means_no_cargo_config() {
    assert_eq!(render(&TestSettings::default(), "dev", None, None), None);
}

/// The text the fingerprint is taken over is what it was before debug info
/// came into the file, or every recorded pass would stop standing.
#[test]
fn the_fingerprints_text_is_unchanged_by_debug_info() {
    let s = levels(&[("kaski-village", "3")]);
    assert_eq!(
        render(&s, "dev", Some(&names(&["kaski-village"])), None).unwrap(),
        "# Written by `cargo mock test` for each run, from `[test.opt_level]` in\n\
             # mockspace.toml. Edits here are lost.\n\n\
             [profile.dev.package.\"kaski-village\"]\nopt-level = 3\n"
    );
}

/// Lean by default, in both profiles `cargo test` can build under, and every
/// member named lean too, since a manifest's entry for one crate beats a
/// profile-wide setting.
#[test]
fn every_crate_is_lean_unless_it_is_named() {
    let members = names(&["a", "b", "c"]);
    let y = symbols(true, &[("b", Reason::Failed)]);
    let t = render(&TestSettings::default(), "dev", Some(&members), Some(&y)).unwrap();
    let d = doc(&t);
    for p in ["dev", "test"] {
        assert_eq!(d["profile"][p]["debug"].as_integer(), Some(0), "{t}");
        assert_eq!(debug_of(&d, p, "a").as_deref(), Some("0"), "{t}");
        assert_eq!(debug_of(&d, p, "c").as_deref(), Some("0"), "{t}");
        assert_eq!(
            debug_of(&d, p, "b").as_deref(),
            Some("line-tables-only"),
            "{t}"
        );
    }
}

/// An opt level and a debug level of one crate are one table, and TOML
/// refuses a table twice.
#[test]
fn a_crates_opt_level_and_debug_level_share_its_table() {
    let mut s = levels(&[("b", "3")]);
    s.debug = DebugLevel::Full;
    let members = names(&["a", "b"]);
    let mut y = symbols(true, &[("b", Reason::Named)]);
    y.level = DebugLevel::Full;
    let t = render(&s, "dev", Some(&members), Some(&y)).unwrap();
    let d = doc(&t);
    assert_eq!(
        d["profile"]["dev"]["package"]["b"]["opt-level"].as_integer(),
        Some(3)
    );
    assert_eq!(debug_of(&d, "dev", "b").as_deref(), Some("2"));
    assert_eq!(debug_of(&d, "test", "b").as_deref(), Some("2"));
    // The opt level is under the profile built, as it always was.
    assert!(
        d["profile"]["test"]["package"]["b"]
            .get("opt-level")
            .is_none(),
        "{t}"
    );
}

/// A release or custom profile is what the caller chose: only the crates
/// that need symbols are touched, and only under that profile.
#[test]
fn a_profile_that_is_not_dev_is_left_alone_but_for_the_crates_that_need_symbols() {
    let members = names(&["a", "b"]);
    let y = symbols(true, &[("b", Reason::Named)]);
    let t = render(
        &TestSettings::default(),
        "release",
        Some(&members),
        Some(&y),
    )
    .unwrap();
    let d = doc(&t);
    assert!(d["profile"]["release"].get("debug").is_none(), "{t}");
    assert_eq!(
        debug_of(&d, "release", "b").as_deref(),
        Some("line-tables-only")
    );
    assert!(d["profile"]["release"]["package"].get("a").is_none(), "{t}");
    assert!(
        d["profile"].get("dev").is_none() && d["profile"].get("test").is_none(),
        "{t}"
    );
    // Nothing named, nothing said.
    assert_eq!(
        render(
            &TestSettings::default(),
            "release",
            Some(&members),
            Some(&symbols(true, &[]))
        ),
        None
    );
}

/// `lean = false` hands the profile back: no profile-wide setting and no
/// crate named lean, and the crates that are named still get symbols.
#[test]
fn a_clone_that_is_not_lean_gets_symbols_on_the_named_crates_only() {
    let members = names(&["a", "b"]);
    let y = symbols(false, &[("b", Reason::Named)]);
    let t = render(&TestSettings::default(), "dev", Some(&members), Some(&y)).unwrap();
    let d = doc(&t);
    assert!(d["profile"]["dev"].get("debug").is_none(), "{t}");
    assert!(d["profile"]["dev"]["package"].get("a").is_none(), "{t}");
    assert_eq!(
        debug_of(&d, "dev", "b").as_deref(),
        Some("line-tables-only")
    );
    assert_eq!(
        render(
            &TestSettings::default(),
            "dev",
            Some(&members),
            Some(&symbols(false, &[]))
        ),
        None
    );
}

/// Where the workspace could not be read no member can be named lean, so
/// only the profile-wide setting says it.
#[test]
fn an_unreadable_workspace_is_lean_profile_wide() {
    let y = symbols(true, &[("b", Reason::Named)]);
    let t = render(&TestSettings::default(), "dev", None, Some(&y)).unwrap();
    let d = doc(&t);
    assert_eq!(d["profile"]["dev"]["debug"].as_integer(), Some(0), "{t}");
    assert_eq!(
        debug_of(&d, "dev", "b").as_deref(),
        Some("line-tables-only")
    );
    assert!(
        d["profile"]["dev"]["package"].as_table().unwrap().len() == 1,
        "{t}"
    );
}

/// A bare `--debug` that met no workspace gives every crate symbols, and
/// names nothing lean.
#[test]
fn symbols_everywhere_is_the_profile_wide_level() {
    let mut y = symbols(true, &[]);
    y.everywhere = true;
    let members = names(&["a"]);
    let t = render(&TestSettings::default(), "dev", Some(&members), Some(&y)).unwrap();
    let d = doc(&t);
    assert_eq!(
        d["profile"]["dev"]["debug"].as_str(),
        Some("line-tables-only"),
        "{t}"
    );
    assert_eq!(
        d["profile"]["test"]["debug"].as_str(),
        Some("line-tables-only"),
        "{t}"
    );
    assert!(d["profile"]["dev"].get("package").is_none(), "{t}");
}
