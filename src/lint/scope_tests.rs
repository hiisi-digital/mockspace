//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A scoped run narrows the per-crate lints and leaves the cross-crate ones whole.
//!
//! The pre-commit and the pre-push hook scope a run to the crates a change touches. A lint that
//! compares crates with each other (`single-source`) judges a set of crates, and a name defined in
//! a touched crate and in an untouched one is a clash however few crates were named. Handed only
//! the scoped crates, it passed the commit that added the second definition, and the clash reached
//! the trunk with every later landing stopped by it.

use std::fs;
use std::path::Path;

use mockspace_lint_rules::{LintMode, LintPack, RegistryView};

use crate::config::Config;

/// A repository of the crates named, each `lib.rs` as given, under `mock/crates`.
fn repo(root: &Path, crates: &[(&str, &str)]) -> Config {
    let mock = root.join("mock");
    for (name, lib) in crates {
        let dir = mock.join("crates").join(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\n"),
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), lib).unwrap();
    }
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = root.to_path_buf();
    cfg.mock_dir = mock.clone();
    cfg.src_dirs = vec![mock.join("crates")];
    cfg.crate_prefix = "sim".to_string();
    cfg
}

/// The blocking count of the commit gate's run with `scope`, which `None` leaves unscoped.
fn violations(cfg: &Config, scope: Option<&[&str]>) -> usize {
    let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    let scope: Option<Vec<String>> = scope.map(|s| s.iter().map(|c| (*c).to_string()).collect());
    crate::lint::run_lints(
        &crates,
        &cfg.src_dirs,
        &cfg.mock_dir,
        LintMode::Commit,
        scope.as_deref(),
        false,
        &cfg.proc_macro_crates,
        cfg.lint_proc_macro_source,
        &cfg.crate_prefix,
        &cfg.lint_overrides,
        &RegistryView::default(),
        &[],
        &[],
        &cfg.primitive_introductions,
        &LintPack::default(),
    )
}

const PLACED: &str = "/// A placement.\npub struct Placed;\n";
const OTHER: &str = "/// Something else.\npub struct Elsewhere;\n";

#[test]
fn a_type_defined_in_a_scoped_crate_and_an_unscoped_one_is_a_clash() {
    // Fails when `run_lints` skips a crate the scope does not name before it is loaded for the
    // cross-crate pass (`if !scoped && !reads_every_crate` read as `if !scoped`).
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", PLACED)]);
    assert!(
        violations(&cfg, Some(&["sim-a"])) > 0,
        "the scoped run let a type defined in two crates through because one of them was not named"
    );
    // The same from the other side: the unscoped crate is the one the finding is reported in.
    assert!(violations(&cfg, Some(&["sim-b"])) > 0);
}

#[test]
fn the_clash_is_found_by_the_unscoped_run_and_not_found_where_there_is_none() {
    // The controls: the full run agrees, and renaming one side clears it, so the count above is
    // the clash and nothing the fixture carries beside it.
    let tmp = tempfile::tempdir().unwrap();
    let clash = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", PLACED)]);
    assert!(violations(&clash, None) > 0);
    let tmp = tempfile::tempdir().unwrap();
    let apart = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", OTHER)]);
    assert_eq!(violations(&apart, None), 0);
    assert_eq!(violations(&apart, Some(&["sim-a"])), 0);
}

#[test]
fn a_cross_crate_lint_reports_what_it_finds_whichever_crates_the_scope_names() {
    // A lint that compares crates judges the whole set, and the scope narrows the lints that judge
    // one crate at a time. A clash between two crates the scope does not name is reported too, so a
    // red the trunk already carries stops the next commit as it stops the next landing, rather than
    // passing here and stopping there. A decision, pinned: a per-crate finding in an unscoped crate
    // is not reported (the next test). Fails under the same mutation as the test above.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[
        ("sim-a", OTHER),
        ("sim-b", PLACED),
        ("sim-c", PLACED),
    ]);
    assert!(violations(&cfg, Some(&["sim-a"])) > 0);
    assert!(
        violations(&cfg, None) > 0,
        "the control: the clash is there"
    );
}

#[test]
fn a_per_crate_lint_still_reads_only_the_scoped_crates() {
    // The control for the narrowing the scope exists for: an import the rule forbids in a crate the
    // scope does not name is that crate's to answer for, at its own commit. Fails when the
    // per-crate pass runs on every crate loaded (`if scoped {` read as `if true {`).
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = repo(tmp.path(), &[
        ("sim-a", OTHER),
        ("sim-b", "use std::fmt;\n"),
    ]);
    let rule: std::collections::HashMap<String, String> = [
        ("rule.no-std.scope", "sim-*"),
        ("rule.no-std.forbidden", "std::*"),
        ("rule.no-std.reason", "no std"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    cfg.lint_overrides
        .params
        .insert("forbidden-imports".to_string(), rule);
    assert!(
        violations(&cfg, None) > 0,
        "the control: the lint finds it when it reads the crate"
    );
    assert!(
        violations(&cfg, Some(&["sim-b"])) > 0,
        "and when the scope names the crate"
    );
    assert_eq!(violations(&cfg, Some(&["sim-a"])), 0);
}

#[test]
fn a_scope_naming_no_crate_reads_no_crate() {
    // Fails when a scope that names no crate still loads every crate (`reads_every_crate` true).
    // `--scope infra` is the commit that touches no crate. It runs the repo lints and parses
    // nothing, and a clash that already stands is not its to find.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", PLACED)]);
    assert_eq!(violations(&cfg, Some(&[])), 0);
}
