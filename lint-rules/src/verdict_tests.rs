//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::collections::HashMap;

use super::*;
use crate::testkit::LintFixture;

/// A digest that frames every part, as the engine's does, without hashing: two inputs are
/// equal here exactly when their framed bytes are, so a test sees a missing input directly.
struct Framing;

impl VerdictCache for Framing {
    fn digest(&self, parts: &[&[u8]]) -> String {
        let mut out = String::new();
        for p in parts {
            out.push_str(&format!("{}:", p.len()));
            out.push_str(&String::from_utf8_lossy(p));
            out.push('|');
        }
        out
    }

    fn replay(&self, _key: &str) -> Option<String> {
        None
    }

    fn remember(&self, _key: &str, _encoded: &str) {}
}

fn finding(msg: &str) -> LintError {
    LintError {
        crate_name:   "kaski-life".to_string(),
        line:         42,
        lint_name:    "no-todo",
        message:      msg.to_string(),
        severity:     Severity::new(Level::Warn, Level::Error, Level::Info),
        finding_kind: Some("macro"),
        path:         Some("src/a b.rs".to_string()),
    }
}

fn same(a: &LintError, b: &LintError) -> bool {
    a.crate_name == b.crate_name
        && a.line == b.line
        && a.lint_name == b.lint_name
        && a.message == b.message
        && a.severity == b.severity
        && a.finding_kind == b.finding_kind
        && a.path == b.path
}

#[test]
fn a_finding_survives_the_round_trip_with_awkward_text() {
    let nasty = "tab\there\nnewline\r\\ backslash \\n literal, unicode ä€, trailing\\";
    let mut bare = finding("bare");
    bare.finding_kind = None;
    bare.path = None;
    let mut empty_path = finding("");
    empty_path.path = Some(String::new());
    let mut tilde = finding("tilde");
    tilde.path = Some("~".to_string());
    let all = vec![finding(nasty), bare, empty_path, tilde];

    let back = decode(&encode(&all)).expect("a written verdict reads back");
    assert_eq!(back.len(), all.len());
    for (a, b) in all.iter().zip(&back) {
        assert!(same(a, b), "lost in the round trip: {a:?} became {b:?}");
    }
}

#[test]
fn no_findings_is_a_verdict_and_not_a_miss() {
    let back = decode(&encode(&[])).expect("an empty verdict reads back");
    assert!(back.is_empty());
}

#[test]
fn text_that_is_not_exactly_a_verdict_is_a_miss_never_a_partial_list() {
    let good = encode(&[finding("one"), finding("two")]);
    assert_eq!(decode(&good).map(|f| f.len()), Some(2), "the control reads");

    // a truncated second line
    let cut = &good[.. good.len() - 12];
    assert!(decode(cut).is_none(), "a truncated verdict must not yield its first finding");
    // another version of the format
    assert!(decode(&good.replace("verdict-v1", "verdict-v2")).is_none());
    // a level letter nobody wrote
    assert!(decode(&good.replace("WEI", "WEX")).is_none());
    // a fourth gate
    assert!(decode(&good.replace("WEI", "WEIE")).is_none());
    // a line number that is not one
    assert!(decode(&good.replace("\t42\t", "\tforty-two\t")).is_none());
    // an escape the writer never emits
    assert!(decode(&good.replace("one", "o\\qne")).is_none());
    // an optional field that is neither absent nor present
    assert!(decode(&good.replace("=macro", "macro")).is_none());
}

#[test]
fn a_replayed_name_is_the_same_static_string_each_time() {
    let a = decode(&encode(&[finding("x")])).unwrap();
    let b = decode(&encode(&[finding("y")])).unwrap();
    assert!(std::ptr::eq(a[0].lint_name, b[0].lint_name), "one leak per distinct name");
}

#[test]
fn what_a_lint_is_configured_with_is_in_its_material_and_its_severity_is_not() {
    let plain = behaviour_material("file-size", None);

    let mut cfg = LintConfig::empty();
    cfg.params.insert(
        "file-size".to_string(),
        HashMap::from([("limit".to_string(), "500".to_string())]),
    );
    let with_param = behaviour_material("file-size", Some(&cfg));
    assert_ne!(plain, with_param, "a parameter changes what a lint finds");

    cfg.params.get_mut("file-size").unwrap().insert("limit".to_string(), "400".to_string());
    assert_ne!(with_param, behaviour_material("file-size", Some(&cfg)), "and so does its value");

    let before = behaviour_material("file-size", Some(&cfg));
    cfg.base.insert("file-size".to_string(), Severity::OFF);
    assert_eq!(
        before,
        behaviour_material("file-size", Some(&cfg)),
        "severity is applied after the lint ran, so it is not an input"
    );
    assert_ne!(
        behaviour_material("other", Some(&cfg)),
        before,
        "another lint's parameters are not this lint's"
    );
}

#[test]
fn a_path_filter_is_part_of_what_a_lint_is_shown() {
    let mut cfg = LintConfig::empty();
    let before = behaviour_material("no-todo", Some(&cfg));
    cfg.paths.insert(
        "no-todo".to_string(),
        crate::path_filter::PathFilter {
            include: vec!["src/**".to_string()],
            exclude: vec!["src/gen/**".to_string()],
        },
    );
    assert_ne!(before, behaviour_material("no-todo", Some(&cfg)));
}

#[test]
fn the_crate_digest_moves_with_every_input_a_crate_lint_can_see() {
    let base = LintFixture::new("pub fn a() {}\n").with_deps(&["x"]).with_all_crates(&["a", "b"]);
    let digest = |f: &LintFixture| crate_digest(&Framing, &f.ctx());
    let control = digest(&base);
    assert_eq!(control, digest(&base), "the control: nothing moved, nothing changed");

    let moved = [
        ("the root source", LintFixture::new("pub fn b() {}\n").with_deps(&["x"]).with_all_crates(&["a", "b"])),
        ("a module file", LintFixture::new("pub fn a() {}\n").with_deps(&["x"]).with_all_crates(&["a", "b"]).with_module("src/m.rs", "fn m() {}\n")),
        ("the dependency list", LintFixture::new("pub fn a() {}\n").with_deps(&["y"]).with_all_crates(&["a", "b"])),
        ("the workspace's crates", LintFixture::new("pub fn a() {}\n").with_deps(&["x"]).with_all_crates(&["a", "b", "c"])),
        ("the design doc", LintFixture::new("pub fn a() {}\n").with_deps(&["x"]).with_all_crates(&["a", "b"]).with_design_doc("d")),
        ("the doc templates", LintFixture::new("pub fn a() {}\n").with_deps(&["x"]).with_all_crates(&["a", "b"]).with_doc_content("d")),
        ("the SHAME template", LintFixture::new("pub fn a() {}\n").with_deps(&["x"]).with_all_crates(&["a", "b"]).with_shame_doc("s")),
        ("the prefix", LintFixture::new("pub fn a() {}\n").with_deps(&["x"]).with_all_crates(&["a", "b"]).with_crate_prefix("zz")),
    ];
    for (what, fixture) in &moved {
        assert_ne!(control, digest(fixture), "a change to {what} must change the crate's digest");
    }
}

#[test]
fn a_dependency_listed_in_another_order_is_the_same_crate() {
    let a = LintFixture::new("fn a() {}\n").with_deps(&["x", "y"]);
    let b = LintFixture::new("fn a() {}\n").with_deps(&["y", "x"]);
    assert_eq!(crate_digest(&Framing, &a.ctx()), crate_digest(&Framing, &b.ctx()));
}
