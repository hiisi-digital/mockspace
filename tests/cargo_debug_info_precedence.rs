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
    // `dep` sits outside the workspace, a dependency and no member.
    write(
        &base.join("dep/Cargo.toml"),
        "[package]\nname = \"dep\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n",
    );
    write(&base.join("dep/src/lib.rs"), "");
    for n in ["a", "b"] {
        let deps = if n == "a" { "\n[dependencies]\ndep = { path = \"../../dep\" }\n" } else { "" };
        write(
            &root.join(format!("{n}/Cargo.toml")),
            &format!(
                "[package]\nname = \"{n}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n{deps}"
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

/// The unit graph cargo resolves, without compiling anything.
fn unit_graph(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> serde_json::Value {
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
    serde_json::from_slice(&o.stdout).unwrap()
}

/// The `debuginfo` of the unit of `name` in `mode`.
fn debuginfo(graph: &serde_json::Value, name: &str, mode: &str) -> String {
    let units = graph["units"].as_array().unwrap();
    let unit = units
        .iter()
        .find(|u| {
            u["mode"] == mode
                && u["pkg_id"].as_str().is_some_and(|p| {
                    p.rsplit('/')
                        .next()
                        .is_some_and(|t| t.starts_with(&format!("{name}#")))
                })
        })
        .unwrap_or_else(|| panic!("no {mode} unit for {name} in {graph}"));
    unit["profile"]["debuginfo"]
        .to_string()
        .trim_matches('"')
        .to_string()
}

/// The `debuginfo` each member's test unit resolves to, as `(a, b)`.
fn resolved(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> (String, String) {
    let g = unit_graph(f, args, env);
    (debuginfo(&g, "a", "test"), debuginfo(&g, "b", "test"))
}

/// The `debuginfo` of `dep`, which is no member.
fn dep_of(f: &Fixture, args: &[&str], env: &[(&str, &str)]) -> String {
    debuginfo(&unit_graph(f, args, env), "dep", "build")
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

fn append(p: &Path, s: &str) {
    let mut t = fs::read_to_string(p).unwrap();
    t.push_str(s);
    fs::write(p, t).unwrap();
}

/// A profile-wide `debug = 0` does not reach a crate outside the workspace
/// that a manifest names for itself, and `"*"` in a manifest or a cargo config
/// does not give way to it either. `"*"` in the file of the run wins against
/// both, and a name wins against a `"*"`, so each named dependency has to be
/// named in the file as well.
#[test]
fn a_profile_wide_setting_does_not_reach_a_dependency_that_something_else_names() {
    let f = fixture(false);
    let lean = f.root.join("lean.toml");
    write(&lean, "[profile.test]\ndebug = 0\n");
    let lean = lean.to_str().unwrap();
    // The control: with nothing naming it, the dependency follows the profile.
    assert_eq!(dep_of(&f, &["--config", lean], &[]), "0");
    // The manifest names it, or says `"*"`.
    append(
        &f.root.join("Cargo.toml"),
        "\n[profile.test.package.dep]\ndebug = 2\n",
    );
    assert_eq!(dep_of(&f, &["--config", lean], &[]), "2");
    let g = fixture(false);
    append(
        &g.root.join("Cargo.toml"),
        "\n[profile.test.package.\"*\"]\ndebug = 2\n",
    );
    assert_eq!(dep_of(&g, &["--config", lean], &[]), "2");
    // A cargo config says `"*"`, or names it.
    for config in [
        "[profile.test.package.\"*\"]\ndebug = 2\n",
        "[profile.test.package.dep]\ndebug = 2\n",
    ] {
        let h = fixture(false);
        write(&h.home.join("config.toml"), config);
        assert_eq!(dep_of(&h, &["--config", lean], &[]), "2", "{config}");
    }
}

#[test]
fn the_file_of_the_run_wins_over_a_star_and_a_name_wins_over_a_star() {
    let star = "[profile.test]\ndebug = 0\n\n[profile.test.package.\"*\"]\ndebug = 0\n";
    let star_and_name = format!("{star}\n[profile.test.package.dep]\ndebug = 0\n");
    for (what, manifest, config) in [
        (
            "manifest star",
            "\n[profile.test.package.\"*\"]\ndebug = 2\n",
            "",
        ),
        (
            "config star",
            "",
            "[profile.test.package.\"*\"]\ndebug = 2\n",
        ),
    ] {
        let f = fixture(false);
        append(&f.root.join("Cargo.toml"), manifest);
        write(&f.home.join("config.toml"), config);
        let file = f.root.join("run.toml");
        write(&file, star);
        assert_eq!(
            dep_of(&f, &["--config", file.to_str().unwrap()], &[]),
            "0",
            "{what}"
        );
    }
    // A name in the manifest or the config beats the file's star, and the file
    // naming it beats the name.
    for (what, manifest, config) in [
        (
            "manifest name",
            "\n[profile.test.package.dep]\ndebug = 2\n",
            "",
        ),
        ("config name", "", "[profile.test.package.dep]\ndebug = 2\n"),
    ] {
        let f = fixture(false);
        append(&f.root.join("Cargo.toml"), manifest);
        write(&f.home.join("config.toml"), config);
        let file = f.root.join("run.toml");
        write(&file, star);
        assert_eq!(
            dep_of(&f, &["--config", file.to_str().unwrap()], &[]),
            "2",
            "{what}: the star alone leaves the name standing"
        );
        write(&file, &star_and_name);
        assert_eq!(
            dep_of(&f, &["--config", file.to_str().unwrap()], &[]),
            "0",
            "{what}: naming it in the file wins"
        );
    }
}

/// `RUSTFLAGS` is no profile setting: `-g` gives every crate debug info
/// whatever the profile says, which is why the run reports it and cannot
/// override it.
#[test]
fn rustflags_are_beyond_any_profile_setting() {
    let f = fixture(false);
    let lean = f.root.join("lean.toml");
    write(&lean, "[profile.test]\ndebug = 0\n");
    let g = unit_graph(&f, &["--config", lean.to_str().unwrap()], &[(
        "RUSTFLAGS",
        "-g",
    )]);
    // The profile still says none; the flag is what rustc is given, and cargo
    // does not show it here. Compile one crate and read the command.
    assert_eq!(debuginfo(&g, "a", "test"), "0");
    let o = Command::new("cargo")
        .args(["rustc", "-v", "-p", "b", "--lib", "--config"])
        .arg(&lean)
        .current_dir(&f.root)
        .env("CARGO_HOME", &f.home)
        .env("RUSTFLAGS", "-g")
        .env("CARGO_TARGET_DIR", f.root.join("target"))
        .env_remove("CARGO_PROFILE_TEST_DEBUG")
        .output()
        .unwrap();
    let t = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{t}");
    let line = t.lines().find(|l| l.contains("--crate-name b")).unwrap();
    assert!(line.contains(" -g"), "{line}");
}
