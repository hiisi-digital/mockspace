//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What cargo does with `debug` when a manifest, a cargo config, the environment
//! and a `--config` file all say something, which is what `mock test` giving
//! symbols only to some crates rests on (`src/suite/debug.rs`).
//!
//! Read off `cargo test --no-run --unit-graph`, which resolves every unit's
//! profile without compiling anything. If a cargo release changes any of these,
//! this fails and the generated config has to change with it.

use std::fs;
use std::path::Path;
use std::process::Command;

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// A workspace of `a` and `b`, the manifest asking for full debug info and `b`
/// alone for limited, and a `CARGO_HOME` asking for `line-tables-only`.
struct Fixture {
    _tmp: tempfile::TempDir,
    root: std::path::PathBuf,
    home: std::path::PathBuf,
}

fn fixture(cargo_home_config: bool) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let root = base.join("ws");
    let home = base.join("home");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"a\", \"b\"]\n\n\
         [profile.dev]\ndebug = 2\n\n[profile.test]\ndebug = 2\n\n\
         [profile.test.package.b]\ndebug = 1\n",
    );
    for n in ["a", "b"] {
        write(
            &root.join(format!("{n}/Cargo.toml")),
            &format!(
                "[package]\nname = \"{n}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n"
            ),
        );
        write(&root.join(format!("{n}/src/lib.rs")), "");
    }
    fs::create_dir_all(&home).unwrap();
    if cargo_home_config {
        write(
            &home.join("config.toml"),
            "[profile.test]\ndebug = \"line-tables-only\"\n",
        );
    }
    Fixture {
        _tmp: tmp,
        root,
        home,
    }
}

/// The `debuginfo` each crate's test unit resolves to, as `(a, b)`.
fn resolved(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String) {
    let mut cmd = Command::new("cargo");
    cmd.args(["test", "--no-run", "--unit-graph", "-Zunstable-options"])
        .args(args)
        .current_dir(&f.root)
        .env("CARGO_HOME", &f.home)
        // The unit graph is unstable; a stable cargo takes the flag with this.
        .env("RUSTC_BOOTSTRAP", "1")
        .env_remove("CARGO_PROFILE_TEST_DEBUG")
        .env_remove("CARGO_PROFILE_DEV_DEBUG")
        .env_remove("CARGO_TARGET_DIR");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let graph: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let of = |name: &str| {
        let units = graph["units"].as_array().unwrap();
        let unit = units
            .iter()
            .find(|u| {
                u["mode"] == "test"
                    && u["pkg_id"].as_str().is_some_and(|p| {
                        p.rsplit('/')
                            .next()
                            .is_some_and(|t| t.starts_with(&format!("{name}#")))
                    })
            })
            .unwrap_or_else(|| panic!("no test unit for {name} in {graph}"));
        unit["profile"]["debuginfo"]
            .to_string()
            .trim_matches('"')
            .to_string()
    };
    (of("a"), of("b"))
}

fn pair(a: &str, b: &str) -> (String, String) {
    (a.to_string(), b.to_string())
}

#[test]
fn a_config_file_beats_the_manifest_and_a_manifests_one_package_beats_the_profile() {
    // The manifest alone: the profile's, and `b`'s own.
    let f = fixture(false);
    assert_eq!(resolved(&f, &[], &[]), pair("2", "1"));
    // A cargo config beats the manifest's profile, silently, and not the
    // manifest's setting for the one package.
    let f = fixture(true);
    assert_eq!(resolved(&f, &[], &[]), pair("line-tables-only", "1"));
}

#[test]
fn a_config_file_of_the_run_beats_a_cargo_config_the_environment_and_the_manifest() {
    let f = fixture(true);
    let lean = f.root.join("lean.toml");
    write(&lean, "[profile.test]\ndebug = 0\n");
    let lean = lean.to_str().unwrap();
    // Over the cargo config and the manifest's profile; `b`'s own entry stays.
    assert_eq!(resolved(&f, &["--config", lean], &[]), pair("0", "1"));
    // Over the environment too.
    assert_eq!(
        resolved(&f, &["--config", lean], &[(
            "CARGO_PROFILE_TEST_DEBUG",
            "2"
        )]),
        pair("0", "1")
    );
    // The control: the environment alone does beat the cargo config, so the
    // line above is the file beating it and not the environment doing nothing.
    assert_eq!(
        resolved(&f, &[], &[("CARGO_PROFILE_TEST_DEBUG", "2")]),
        pair("2", "1")
    );
}

#[test]
fn naming_a_package_in_the_config_file_beats_the_manifests_entry_for_it() {
    let f = fixture(true);
    let named = f.root.join("named.toml");
    write(
        &named,
        "[profile.test]\ndebug = 0\n\n[profile.test.package.\"b\"]\ndebug = 0\n\n\
         [profile.test.package.\"a\"]\ndebug = \"line-tables-only\"\n",
    );
    assert_eq!(
        resolved(&f, &["--config", named.to_str().unwrap()], &[]),
        pair("line-tables-only", "0")
    );
}

/// The environment has no way to say it for one package, in any spelling, which
/// is why the run writes a file.
#[test]
fn the_environment_has_no_spelling_for_one_package() {
    let f = fixture(false);
    for var in [
        "CARGO_PROFILE_TEST_PACKAGE_B_DEBUG",
        "CARGO_PROFILE_TEST_PACKAGE_b_DEBUG",
        "CARGO_PROFILE_TEST_PACKAGE_A_DEBUG",
        "CARGO_PROFILE_DEV_PACKAGE_B_DEBUG",
    ] {
        assert_eq!(resolved(&f, &[], &[(var, "0")]), pair("2", "1"), "{var}");
    }
}
