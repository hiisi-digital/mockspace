//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::collections::BTreeSet;

use super::*;
use crate::suite::args::{DebugAsk, parse};

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn set(v: &[&str]) -> BTreeSet<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// What `decide` says for a command line, over members `a b c`, of which
/// `running` run and `failed` failed last time.
fn crates(args: &[&str], running: Option<&[&str]>, failed: &[&str]) -> BTreeMap<String, Reason> {
    let inv = parse(args);
    let members = names(&["a", "b", "c"]);
    let running = running.map(names);
    decide(&TestSettings::default(), &Basis {
        ask:      &inv.debug,
        packages: &inv.packages,
        members:  Some(&members),
        running:  running.as_deref(),
        failed:   &set(failed),
    })
    .crates
}

fn of(v: &[(&str, Reason)]) -> BTreeMap<String, Reason> {
    v.iter().map(|(c, r)| (c.to_string(), *r)).collect()
}

/// The default needs nothing: lean is the default, and only a failure or a
/// name changes that.
#[test]
fn a_clean_run_asks_for_no_symbols() {
    assert!(crates(&[], Some(&["a", "b"]), &[]).is_empty());
}

#[test]
fn a_crate_that_failed_last_time_and_runs_has_symbols() {
    assert_eq!(
        crates(&[], Some(&["a", "b"]), &["b"]),
        of(&[("b", Reason::Failed)])
    );
}

/// A failed crate this run does not build tests for would be a rebuild of it
/// and its dependents for nothing.
#[test]
fn a_crate_that_failed_but_does_not_run_has_none() {
    assert!(crates(&[], Some(&["a"]), &["b"]).is_empty());
}

/// Under plain `cargo test` what runs is not known, so what failed counts.
#[test]
fn under_plain_cargo_test_what_failed_counts_wherever_it_is() {
    assert_eq!(crates(&[], None, &["b"]), of(&[("b", Reason::Failed)]));
}

/// A name that is no member is no crate: a profile spec matching nothing is
/// a warning on every build.
#[test]
fn a_failure_recorded_for_a_crate_that_is_gone_is_dropped() {
    assert!(crates(&[], None, &["gone"]).is_empty());
    assert!(crates(&["--debug=gone"], None, &[]).is_empty());
}

#[test]
fn a_named_crate_has_symbols_and_is_named_not_failed() {
    assert_eq!(
        crates(&["--debug=a"], Some(&["a"]), &["a"]),
        of(&[("a", Reason::Named)])
    );
    assert_eq!(
        crates(&["-p", "b", "--debug"], Some(&["b"]), &[]),
        of(&[("b", Reason::Named)])
    );
}

/// A bare `--debug` with `-p` is those crates; the failed one beside them
/// is still a failed one.
#[test]
fn bare_debug_with_p_is_the_packages_named_and_a_failure_is_added() {
    assert_eq!(
        crates(&["-p", "a", "--debug"], Some(&["a", "b"]), &["b"]),
        of(&[("a", Reason::Named), ("b", Reason::Failed)])
    );
}

/// With nothing named, a bare `--debug` is every crate that runs, and not the
/// members that do not.
#[test]
fn bare_debug_alone_is_everything_that_runs() {
    assert_eq!(
        crates(&["--debug"], Some(&["a", "c"]), &[]),
        of(&[("a", Reason::Named), ("c", Reason::Named)])
    );
    // Where what runs is not known, every member.
    assert_eq!(crates(&["--debug"], None, &[]).len(), 3);
}

#[test]
fn no_debug_is_no_symbols_even_for_a_failure_or_a_name() {
    assert!(crates(&["--debug=a", "--no-debug"], Some(&["a", "b"]), &["b"]).is_empty());
    assert!(crates(&["--no-debug"], Some(&["a", "b"]), &["b"]).is_empty());
}

#[test]
fn a_bare_debug_with_no_workspace_is_symbols_everywhere() {
    let inv = parse(&["--debug"]);
    let s = decide(&TestSettings::default(), &Basis {
        ask:      &inv.debug,
        packages: &[],
        members:  None,
        running:  None,
        failed:   &set(&[]),
    });
    assert!(s.everywhere);
    assert!(s.line().unwrap().contains("every crate"));
}

#[test]
fn the_plan_says_which_crates_and_why() {
    let s = decide(&TestSettings::default(), &Basis {
        ask:      &parse(&["-p", "a", "--debug"]).debug,
        packages: &names(&["a"]),
        members:  Some(&names(&["a", "b"])),
        running:  Some(&names(&["a", "b"])),
        failed:   &set(&["b"]),
    });
    let line = s.line().unwrap();
    assert!(
        line.contains("symbols for a (asked for), b (failed last run)"),
        "{line}"
    );
    assert!(
        line.contains("every other crate is built without debug info"),
        "{line}"
    );
    let none = decide(&TestSettings::default(), &Basis {
        ask:      &DebugAsk::default(),
        packages: &[],
        members:  Some(&names(&["a"])),
        running:  Some(&names(&["a"])),
        failed:   &set(&[]),
    });
    let line = none.line().unwrap();
    assert!(!line.contains("symbols for"), "{line}");
    assert!(line.contains("no crate needs symbols"), "{line}");
}

/// A clone that is not lean has nothing to say about it unless a crate is named.
#[test]
fn a_clone_that_is_not_lean_says_nothing_of_the_default() {
    let s = TestSettings {
        lean: false,
        ..TestSettings::default()
    };
    let none = decide(&s, &Basis {
        ask:      &DebugAsk::default(),
        packages: &[],
        members:  Some(&names(&["a"])),
        running:  Some(&names(&["a"])),
        failed:   &set(&[]),
    });
    assert_eq!(none.line(), None);
}

fn write(p: &std::path::Path, t: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, t).unwrap();
}

fn no_env(_: &str) -> bool {
    false
}

/// A cargo config that sets debug info for the profile, wherever up the tree or
/// in `CARGO_HOME` it is, is found, and one that sets none, or is no config at
/// all, is not: the control for every empty answer here.
#[test]
fn a_cargo_config_that_sets_debug_info_is_found_wherever_cargo_reads_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let mock = root.join("repo/mock");
    std::fs::create_dir_all(&mock).unwrap();
    let home = root.join("home");

    // Nothing anywhere: nothing found, and the dirs were really looked in.
    let dirs = config_dirs(&mock, Some(home.clone()));
    assert!(dirs.contains(&root.join("repo/mock/.cargo")));
    assert!(dirs.contains(&root.join("repo/.cargo")));
    assert_eq!(dirs.last(), Some(&home));
    assert!(settings_in_the_way(&dirs, &["dev", "test"], &no_env).is_empty());

    write(
        &home.join("config.toml"),
        "[profile.dev]\ndebug = \"line-tables-only\"\n",
    );
    write(
        &root.join("repo/.cargo/config.toml"),
        "[profile.test]\ndebug = 2\n",
    );
    write(
        &root.join("repo/mock/.cargo/config.toml"),
        "[profile.release]\ndebug = true\n[build]\njobs = 2\n",
    );
    let found = settings_in_the_way(&dirs, &["dev", "test"], &no_env);
    let keys: Vec<(&str, Option<&str>)> = found
        .iter()
        .map(|f| (f.key.as_str(), f.value.as_deref()))
        .collect();
    // Nearest first, as cargo merges them; a profile not built is not reported.
    assert_eq!(keys, vec![
        ("profile.test.debug", Some("2")),
        ("profile.dev.debug", Some("\"line-tables-only\"")),
    ]);
    assert_eq!(
        found[1].file.as_deref(),
        Some(home.join("config.toml").as_path())
    );
    // The older file name is read when there is no `config.toml`.
    write(
        &root.join("repo/mock/.cargo/config"),
        "[profile.dev]\ndebug = 1\n",
    );
    std::fs::remove_file(root.join("repo/mock/.cargo/config.toml")).unwrap();
    let found = settings_in_the_way(&dirs, &["dev"], &no_env);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!(found[0].value.as_deref(), Some("1"));
}

#[test]
fn a_setting_for_one_package_is_found_and_a_file_that_does_not_parse_is_not() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path().join("one");
    write(
        &d.join("config.toml"),
        "[profile.dev.package.x]\ndebug = 2\n[profile.dev.package.\"*\"]\nopt-level = 3\n",
    );
    let found = settings_in_the_way(std::slice::from_ref(&d), &["dev"], &no_env);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].key, "profile.dev.package.x.debug");
    write(&d.join("config.toml"), "[profile.dev\ndebug = 2\n");
    assert!(settings_in_the_way(&[d], &["dev"], &no_env).is_empty());
}

/// The environment is named, never read: its value is nobody's business here.
#[test]
fn the_environment_is_named_and_its_value_is_never_read() {
    let env = |v: &str| v == "CARGO_PROFILE_DEV_DEBUG" || v == "CARGO_PROFILE_MY_CI_DEBUG";
    let found = settings_in_the_way(&[], &["dev", "test", "my-ci"], &env);
    let keys: Vec<&str> = found.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, vec![
        "CARGO_PROFILE_DEV_DEBUG",
        "CARGO_PROFILE_MY_CI_DEBUG"
    ]);
    assert!(found.iter().all(|f| f.file.is_none() && f.value.is_none()));
    let said = notes(&found);
    assert_eq!(said.len(), 1, "one note for the environment: {said:?}");
    assert!(
        said[0].contains(
            "the environment sets CARGO_PROFILE_DEV_DEBUG and CARGO_PROFILE_MY_CI_DEBUG;"
        ),
        "{said:?}"
    );
}

#[test]
fn a_note_names_the_file_the_settings_and_what_the_run_does() {
    let f = Found {
        file:  Some(std::path::PathBuf::from("/root/.cargo/config.toml")),
        key:   "profile.dev.debug".to_string(),
        value: Some("\"line-tables-only\"".to_string()),
    };
    let g = Found {
        key: "profile.test.debug".to_string(),
        value: Some("2".to_string()),
        ..f.clone()
    };
    let other = Found {
        file: Some(std::path::PathBuf::from("/w/.cargo/config.toml")),
        ..f.clone()
    };
    let said = notes(&[f, g, other]);
    // One note for a file whatever the number of profiles it sets, so a run
    // does not repeat itself.
    assert_eq!(said.len(), 2, "{said:?}");
    let text = &said[0];
    assert!(
        text.contains(
            "/root/.cargo/config.toml sets profile.dev.debug = \"line-tables-only\" and profile.test.debug = 2;"
        ),
        "{text}"
    );
    assert!(
        said[1].contains("/w/.cargo/config.toml sets profile.dev.debug"),
        "{said:?}"
    );
    assert!(text.contains("edits nothing"), "{text}");
    assert!(notes(&[]).is_empty());
    assert!(text.contains("lean = false"), "{text}");
}

#[test]
fn a_config_of_the_callers_that_sets_debug_is_noticed_in_either_spelling() {
    let words = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(passes_own_debug(&words(&[
        "--config",
        "profile.dev.debug=2"
    ])));
    assert!(passes_own_debug(&words(&[
        "--features",
        "x",
        "--config=profile.test.debug=1"
    ])));
    // The controls: other configs, and a config word that is not a value.
    assert!(!passes_own_debug(&words(&["--config", "build.jobs=2"])));
    assert!(!passes_own_debug(&words(&["--features", "profile.debug"])));
    assert!(!passes_own_debug(&words(&["--config"])));
}
