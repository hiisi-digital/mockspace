//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A scoped run narrows the per-crate lints and leaves the cross-crate ones whole.
//!
//! The pre-commit and the pre-push hook scope a run to the crates a change touches. A workspace
//! lint, built-in or from a pack, judges a set of crates (`single-source`, `no-duplicate-fn` and
//! `undocumented-type` are the ones these tests drive), and a name defined in a touched crate and
//! in an untouched one is a clash however few crates were named. Handed only the scoped crates,
//! `single-source` passed the commit that added the second definition, and the clash reached the
//! trunk with every later landing stopped by it.
//!
//! The law the tests hold: for a scope naming a crate, the cross-crate verdict of the run is the
//! verdict of the unscoped run over the same tree, in every lint mode, and the per-crate lints
//! answer for the named crates alone. A count is compared rather than "above zero", so a finding
//! reported twice fails too.

use std::fs;
use std::path::Path;

use mockspace_lint_rules::{LintMode, LintPack, PathFilter, RegistryView};

use crate::config::Config;

/// A repository of the crates named, each `lib.rs` as given, under `mock/crates`.
fn repo(root: &Path, crates: &[(&str, &str)]) -> Config {
    let mock = root.join("mock");
    for (name, lib) in crates {
        let dir = mock.join("crates").join(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), format!("[package]\nname = \"{name}\"\n")).unwrap();
        fs::write(dir.join("src/lib.rs"), lib).unwrap();
    }
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = root.to_path_buf();
    cfg.mock_dir = mock.clone();
    cfg.src_dirs = vec![mock.join("crates")];
    cfg.crate_prefix = "sim".to_string();
    cfg
}

/// The blocking count of a run in `mode`, `doc_only` as given, with `scope`, which `None` leaves
/// unscoped.
fn run(cfg: &Config, mode: LintMode, scope: Option<&[&str]>, doc_only: bool) -> usize {
    let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    let scope: Option<Vec<String>> = scope.map(|s| s.iter().map(|c| (*c).to_string()).collect());
    crate::lint::run_lints(
        &crates,
        &cfg.src_dirs,
        &cfg.mock_dir,
        &cfg.repo_root,
        mode,
        scope.as_deref(),
        doc_only,
        &cfg.proc_macro_crates,
        cfg.lint_proc_macro_source,
        &cfg.crate_prefix,
        &cfg.lint_overrides,
        &RegistryView::default(),
        &[],
        &[],
        &mockspace_lint_rules::src_layout::NO_REF_FILES,
        &cfg.primitive_introductions,
        &LintPack::default(),
    )
}

/// The commit gate's run with `scope`.
fn violations(cfg: &Config, scope: Option<&[&str]>) -> usize {
    run(cfg, LintMode::Commit, scope, false)
}

/// Every mode a hook runs the engine in: the commit hook, the build, and the push hook's `--strict`.
fn modes() -> [LintMode; 3] {
    [LintMode::Commit, LintMode::Build, LintMode::Push]
}

const PLACED: &str = "/// A placement.\npub struct Placed;\n";
const OTHER: &str = "/// Something else.\npub struct Elsewhere;\n";
const THIRD: &str = "/// A third thing.\npub struct Third;\n";
const SHARED_FN: &str = "/// Shared.\npub fn shared() {}\n";
const OWN_FN: &str = "/// Own.\npub fn own() {}\n";

/// The design document of a crate, naming each type given, which is what `undocumented-type` reads.
fn design(root: &Path, krate: &str, names: &str) {
    fs::write(root.join("mock/crates").join(krate).join("DESIGN.md.tmpl"), names).unwrap();
}

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
fn every_scope_reaches_the_verdict_of_the_unscoped_run_in_every_mode() {
    // The law, over every non-empty subset of three crates, two of which clash. Fails under the
    // same mutation as the first test, and when a finding is reported once per scoped crate.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", PLACED), ("sim-c", OTHER)]);
    design(tmp.path(), "sim-c", "Placed Elsewhere");
    let names = ["sim-a", "sim-b", "sim-c"];
    for mode in modes() {
        let whole = run(&cfg, mode, None, false);
        assert!(whole > 0, "the control: the unscoped run finds the clash in {mode:?}");
        for bits in 1..8_usize {
            let scope: Vec<&str> =
                (0..3).filter(|i| bits & (1 << i) != 0).map(|i| names[i]).collect();
            assert_eq!(
                run(&cfg, mode, Some(&scope), false),
                whole,
                "scope {scope:?} in {mode:?} reached another verdict than the unscoped run"
            );
        }
    }
    // The control the other way: with the clash renamed away, every scope passes.
    let tmp = tempfile::tempdir().unwrap();
    let clean = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", OTHER), ("sim-c", THIRD)]);
    design(tmp.path(), "sim-c", "Placed Elsewhere Third");
    for mode in modes() {
        assert_eq!(run(&clean, mode, None, false), 0, "the fixture is clean in {mode:?}");
        assert_eq!(run(&clean, mode, Some(&["sim-a"]), false), 0);
    }
}

#[test]
fn a_function_defined_in_a_scoped_crate_and_an_unscoped_one_is_a_duplicate() {
    // The second cross-crate lint the same defect hid: `no-duplicate-fn`. Fails under the same
    // mutation as the first test.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[("sim-a", SHARED_FN), ("sim-b", SHARED_FN)]);
    assert!(violations(&cfg, Some(&["sim-a"])) > 0);
    let tmp = tempfile::tempdir().unwrap();
    let apart = repo(tmp.path(), &[("sim-a", SHARED_FN), ("sim-b", OWN_FN)]);
    assert_eq!(violations(&apart, Some(&["sim-a"])), 0, "the control: two functions, two names");
}

#[test]
fn a_type_documented_in_an_unscoped_crate_is_documented() {
    // The refusal the scope used to give without cause: `undocumented-type` gathers the design
    // documents of every crate it is handed, and a scoped run handed one crate refused a type the
    // next crate's document names. Fails under the same mutation as the first test.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", OTHER)]);
    design(tmp.path(), "sim-b", "Placed Elsewhere");
    assert_eq!(run(&cfg, LintMode::Build, None, false), 0);
    assert_eq!(run(&cfg, LintMode::Build, Some(&["sim-a"]), false), 0);
    // The control: with no document naming it the lint is live, scoped or not.
    let tmp = tempfile::tempdir().unwrap();
    let bare = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", OTHER)]);
    assert!(run(&bare, LintMode::Build, None, false) > 0);
    assert!(run(&bare, LintMode::Build, Some(&["sim-a"]), false) > 0);
}

#[test]
fn a_crate_filter_on_a_cross_crate_lint_still_binds_under_a_scope() {
    // `single-source` bound to `sim-a` and `sim-b`: a scope naming only the unbound `sim-c` still
    // sees their clash, and a clash with a crate the lint does not bind is not seen at all. Fails
    // when the scope narrows what the lint is handed (the first half) or when the filter is
    // dropped for a scoped run (the second).
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", PLACED), ("sim-c", OTHER)]);
    design(tmp.path(), "sim-c", "Placed Elsewhere");
    cfg.lint_overrides.crates.insert(
        "single-source".to_string(),
        PathFilter {
            include: vec!["sim-a".to_string(), "sim-b".to_string()],
            exclude: vec![],
        },
    );
    assert!(violations(&cfg, None) > 0, "the control: the bound pair clashes");
    assert_eq!(violations(&cfg, Some(&["sim-c"])), violations(&cfg, None));
    cfg.lint_overrides.crates.insert(
        "single-source".to_string(),
        PathFilter {
            include: vec!["sim-a".to_string(), "sim-c".to_string()],
            exclude: vec![],
        },
    );
    assert_eq!(violations(&cfg, None), 0, "the clash is with a crate the lint does not bind");
    assert_eq!(violations(&cfg, Some(&["sim-a"])), 0);
}

#[test]
fn a_scope_naming_a_crate_the_repository_no_longer_has_still_reads_every_crate() {
    // The commit that deletes a crate names it and finds nothing, and what the rest of the
    // workspace holds is still its to answer for. Fails when a scope is read as the crates found
    // (`reads_every_crate` asking whether any scoped crate exists).
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", PLACED)]);
    assert_eq!(violations(&cfg, Some(&["sim-gone"])), violations(&cfg, None));
    assert!(violations(&cfg, Some(&["sim-gone"])) > 0, "the control: there is a clash to find");
}

#[test]
fn a_doc_only_scoped_run_skips_the_source_lints_as_the_unscoped_one_does() {
    // `--doc-only` skips the source lints, the cross-crate ones included, and handing them more
    // crates does not bring them back.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", PLACED)]);
    assert_eq!(run(&cfg, LintMode::Commit, Some(&["sim-a"]), true), 0);
    assert_eq!(run(&cfg, LintMode::Commit, None, true), 0);
    assert!(violations(&cfg, Some(&["sim-a"])) > 0, "the control: without the flag it is found");
}

#[test]
fn a_doc_only_scoped_run_reads_the_documents_of_every_crate() {
    // `--doc-only` keeps `undocumented-type`, which reads documents, and it runs at the Build gate
    // as an error. Handed only the scoped crate it refused a type another crate's document names.
    // Fails under the same mutation as the first test, which the Commit-mode test above cannot:
    // there `undocumented-type` only warns, so both of its sides are 0 whatever the scope reads.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", OTHER)]);
    design(tmp.path(), "sim-b", "Placed Elsewhere");
    assert_eq!(run(&cfg, LintMode::Build, Some(&["sim-a"]), true), 0);
    assert_eq!(run(&cfg, LintMode::Build, None, true), 0);
    // The control: with no document naming the type the lint is live under the flag, scoped or not.
    let tmp = tempfile::tempdir().unwrap();
    let bare = repo(tmp.path(), &[("sim-a", PLACED), ("sim-b", OTHER)]);
    assert!(run(&bare, LintMode::Build, Some(&["sim-a"]), true) > 0);
    assert!(run(&bare, LintMode::Build, None, true) > 0);
}

#[test]
fn a_per_crate_lint_still_reads_only_the_scoped_crates() {
    // The control for the narrowing the scope exists for: an import the rule forbids in a crate the
    // scope does not name is that crate's to answer for, at its own commit. Fails when the
    // per-crate pass runs on every crate loaded (`if scoped {` read as `if true {`).
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = repo(tmp.path(), &[("sim-a", OTHER), ("sim-b", "use std::fmt;\n")]);
    let rule: std::collections::HashMap<String, String> = [
        ("rule.no-std.scope", "sim-*"),
        ("rule.no-std.forbidden", "std::*"),
        ("rule.no-std.reason", "no std"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    cfg.lint_overrides.params.insert("forbidden-imports".to_string(), rule);
    assert!(violations(&cfg, None) > 0, "the control: the lint finds it when it reads the crate");
    assert!(violations(&cfg, Some(&["sim-b"])) > 0, "and when the scope names the crate");
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
