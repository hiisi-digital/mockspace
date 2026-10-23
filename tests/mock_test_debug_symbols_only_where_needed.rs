//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test` builds without debug info and gives symbols only to the crates
//! that need them, measured on what rustc was actually asked to do.
//!
//! A `RUSTC_WRAPPER` logs every compilation with the `-C debuginfo` it was
//! given, so each step reads off which crates were compiled this run and at
//! which level. That is also the fingerprint measurement: a crate that moves to
//! symbols shows up in the log together with the crates that depend on it, and
//! a crate that does not depend on it, or that it depends on, never does.
//!
//! The fixture has the manifest ask for full debug info and a cargo config
//! beside it (a `CARGO_HOME`, as a helper container has) ask for
//! `line-tables-only`, so a run that came out lean overrode both.
//!
//! The nextest arm needs cargo-nextest and passes without it; the plain arm
//! does not.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// How cargo spells "no debug info" in `-C debuginfo`, or leaves it out.
const LEAN: &[&str] = &["0", "none"];

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// What a fixture adds to the plain one: the `[test]` table, text for the
/// manifest's profile tables and for the `CARGO_HOME` config, and `dep`, a crate
/// outside the workspace that `base` depends on by path.
#[derive(Default)]
struct Variant<'a> {
    test_table:     &'a str,
    manifest_extra: &'a str,
    home_extra:     &'a str,
    dep:            bool,
}

/// `base`, `mid` depending on it, `top` depending on `mid`, and `alone`
/// depending on nothing. `mid` fails while `crates/mid/fail` exists.
fn fixture(root: &Path, v: &Variant) {
    let test_table = v.test_table;
    write(
        &root.join("mockspace.toml"),
        &format!("project_name = \"fixture\"\nmock_dir = \"mock\"\n\n[test]\n{test_table}"),
    );
    write(&root.join(".gitignore"), "target/\n");
    write(
        &root.join("mock/Cargo.toml"),
        &format!(
            "[workspace]\nresolver = \"2\"\nmembers = [\"crates/base\", \"crates/mid\", \"crates/top\", \"crates/alone\"]\n\n\
             [profile.dev]\ndebug = 2\n\n[profile.test]\ndebug = 2\n\n{}",
            v.manifest_extra
        ),
    );
    if v.dep {
        write(
            &root.join("dep/Cargo.toml"),
            "[package]\nname = \"dep\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n",
        );
        write(&root.join("dep/src/lib.rs"), "pub fn d() {}\n");
    }
    let pkg = |name: &str, deps: &str| {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\n{deps}"
        )
    };
    let lib = |body: &str| format!("pub fn f() -> u32 {{ 1 }}\n#[test]\nfn t() {{ {body} }}\n");
    for (name, deps, body) in [
        (
            "base",
            if v.dep { "dep = { path = \"../../../dep\" }\n" } else { "" },
            "",
        ),
        (
            "mid",
            "base = { path = \"../base\" }\n",
            "assert!(!std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"fail\").exists());",
        ),
        ("top", "mid = { path = \"../mid\" }\n", ""),
        ("alone", "", ""),
    ] {
        write(
            &root.join(format!("mock/crates/{name}/Cargo.toml")),
            &pkg(name, deps),
        );
        write(
            &root.join(format!("mock/crates/{name}/src/lib.rs")),
            &lib(body),
        );
    }
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
}

struct Rig {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    log:  PathBuf,
    shim: PathBuf,
}

/// What a run printed and which crates it compiled, each at the debug levels it
/// was compiled at.
struct Run {
    ok:       bool,
    text:     String,
    compiled: BTreeMap<String, BTreeSet<String>>,
}

impl Run {
    fn symbols(&self) -> BTreeSet<&str> {
        self.compiled
            .iter()
            .filter(|(_, levels)| levels.iter().any(|l| !LEAN.contains(&l.as_str())))
            .map(|(c, _)| c.as_str())
            .collect()
    }

    fn lean(&self) -> BTreeSet<&str> {
        self.compiled
            .iter()
            .filter(|(_, levels)| levels.iter().all(|l| LEAN.contains(&l.as_str())))
            .map(|(c, _)| c.as_str())
            .collect()
    }
}

impl Rig {
    fn new(test_table: &str) -> Rig {
        Rig::with(&Variant {
            test_table,
            ..Variant::default()
        })
    }

    fn with(v: &Variant) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("repo");
        let home = base.join("cargo-home");
        let shim = base.join("rustc-log");
        let log = base.join("rustc.log");
        fixture(&root, v);
        // What a helper container carries: a cargo config under `CARGO_HOME` that
        // sets debug info for every build and silently beats the manifest.
        write(
            &home.join("config.toml"),
            "[profile.dev]\ndebug = \"line-tables-only\"\n\n[profile.test]\ndebug = \"line-tables-only\"\n",
        );
        if !v.home_extra.is_empty() {
            let mut t = fs::read_to_string(home.join("config.toml")).unwrap();
            t.push('\n');
            t.push_str(v.home_extra);
            write(&home.join("config.toml"), &t);
        }
        write(
            &shim,
            "#!/bin/sh\n\
             name=''; level=none; prev=''\n\
             for a in \"$@\"; do\n\
             \x20 case \"$prev\" in\n\
             \x20   --crate-name) name=\"$a\" ;;\n\
             \x20   -C) case \"$a\" in debuginfo=*) level=\"${a#debuginfo=}\" ;; esac ;;\n\
             \x20 esac\n\
             \x20 prev=\"$a\"\n\
             done\n\
             [ -n \"$name\" ] && echo \"$name $level\" >> \"$MS_RUSTC_LOG\"\n\
             exec \"$@\"\n",
        );
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Rig {
            _tmp: tmp,
            root,
            home,
            log,
            shim,
        }
    }

    fn mock(&self, args: &[&str]) -> Run {
        self.mock_env(args, &[])
    }

    fn mock_env(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let _ = fs::remove_file(&self.log);
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_mockspace"));
        cmd.args(["--dir", "mock", "test"])
            .args(args)
            .current_dir(&self.root)
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_TARGET_DIR")
            .env_remove("CARGO_PROFILE_DEV_DEBUG")
            .env_remove("CARGO_PROFILE_TEST_DEBUG")
            .env("CARGO_INCREMENTAL", "0")
            .env("CARGO_BUILD_JOBS", "2")
            .env("CARGO_HOME", &self.home)
            .env("RUSTC_WRAPPER", &self.shim)
            .env("MS_RUSTC_LOG", &self.log);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let o = cmd.output().unwrap();
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        let mut compiled: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for line in fs::read_to_string(&self.log).unwrap_or_default().lines() {
            if let Some((c, l)) = line.split_once(' ') {
                compiled
                    .entry(c.to_string())
                    .or_default()
                    .insert(l.to_string());
            }
        }
        // Only the fixture's own crates: nothing else is compiled here, and a
        // wrapper line naming something else would be a probe the run made.
        compiled.retain(|c, _| ["base", "mid", "top", "alone", "dep"].contains(&c.as_str()));
        Run {
            ok: o.status.success(),
            text,
            compiled,
        }
    }

    fn fail_mid(&self, failing: bool) {
        let p = self.root.join("mock/crates/mid/fail");
        if failing {
            write(&p, "x\n");
        } else {
            let _ = fs::remove_file(p);
        }
    }
}

fn nextest_here() -> bool {
    Command::new("cargo")
        .args(["nextest", "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

fn set(names: &[&'static str]) -> BTreeSet<&'static str> {
    names.iter().copied().collect()
}

/// The sequence a crate goes through: lean while it passes, symbols once it
/// fails, lean again once it passes, and symbols on request.
#[test]
fn nextest_gives_symbols_to_the_crates_that_need_them_and_to_nothing_else() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let rig = Rig::new("");

    // First run, everything passes: every crate is compiled, none of them with
    // symbols, whatever the manifest and the cargo config under `CARGO_HOME` say.
    let r = rig.mock(&[]);
    assert!(r.ok, "first run:\n{}", r.text);
    assert_eq!(
        r.lean(),
        set(&["alone", "base", "mid", "top"]),
        "every crate compiled lean:\n{:?}",
        r.compiled
    );
    assert!(r.symbols().is_empty(), "{:?}", r.compiled);
    // The cargo config that set debug info is reported, not edited and not obeyed.
    assert!(
        r.text.contains("config.toml") && r.text.contains("profile.dev.debug"),
        "the run names the config that sets debug info:\n{}",
        r.text
    );
    let cfg = fs::read_to_string(rig.home.join("config.toml")).unwrap();
    assert!(
        cfg.contains("line-tables-only"),
        "the user's config is left alone"
    );

    // `mid` fails. Nothing is recompiled for that, and nothing has failed yet
    // when this run is planned, so no crate has symbols.
    rig.fail_mid(true);
    let r = rig.mock(&[]);
    assert!(!r.ok, "mid fails:\n{}", r.text);
    assert!(r.compiled.is_empty(), "{:?}", r.compiled);

    // The next run knows `mid` failed. Over the whole workspace, `mid` is the
    // one crate compiled with symbols and `top`, which depends on it, is
    // compiled again lean; `base`, which `mid` depends on, and `alone`, which
    // neither knows, are not compiled at all.
    let r = rig.mock(&["--full"]);
    assert!(!r.ok, "mid still fails:\n{}", r.text);
    assert_eq!(r.symbols(), set(&["mid"]), "{:?}\n{}", r.compiled, r.text);
    assert_eq!(r.lean(), set(&["top"]), "{:?}\n{}", r.compiled, r.text);
    assert!(
        r.text.contains("symbols for mid"),
        "the plan says which crate has symbols:\n{}",
        r.text
    );

    // Fixed: still built with symbols, since it failed last time, which costs
    // no compilation as it is already built so. It passes, and that clears it.
    rig.fail_mid(false);
    let r = rig.mock(&[]);
    assert!(r.ok, "mid fixed:\n{}", r.text);
    assert!(r.compiled.is_empty(), "{:?}", r.compiled);
    // Lean again. Both variants stay in the target directory, so going back
    // compiles nothing either.
    let r = rig.mock(&["--full"]);
    assert!(r.ok, "{}", r.text);
    assert!(r.compiled.is_empty(), "{:?}", r.compiled);
    assert!(!r.text.contains("symbols for"), "{}", r.text);

    // The record of a failure is no fingerprint input: after all that, a plain
    // run finds every member passed at what it is now.
    let r = rig.mock(&[]);
    assert!(r.ok && r.text.contains("nothing to run"), "{}", r.text);

    // A crate named with `-p` and `--debug` runs even though it passed, with
    // symbols, and nothing else is compiled.
    let r = rig.mock(&["-p", "alone", "--debug"]);
    assert!(r.ok, "{}", r.text);
    assert_eq!(r.symbols(), set(&["alone"]), "{:?}", r.compiled);
    assert!(r.lean().is_empty(), "{:?}", r.compiled);
    // `--debug=<crate>` names it and chooses it.
    let r = rig.mock(&["--debug=base"]);
    assert!(r.ok, "{}", r.text);
    assert_eq!(r.symbols(), set(&["base"]), "{:?}", r.compiled);
    assert!(r.lean().is_empty(), "{:?}", r.compiled);
    // The symbols those two runs were built with moved no fingerprint either:
    // what passed under them stands for a run without.
    let r = rig.mock(&[]);
    assert!(r.ok && r.text.contains("nothing to run"), "{}", r.text);

    // `--no-debug` keeps even a failed crate lean.
    rig.fail_mid(true);
    assert!(!rig.mock(&[]).ok);
    let r = rig.mock(&["--full", "--no-debug"]);
    assert!(!r.ok, "{}", r.text);
    assert!(r.symbols().is_empty(), "{:?}\n{}", r.compiled, r.text);
    assert!(!r.text.contains("symbols for"), "{}", r.text);
}

#[test]
fn plain_cargo_test_is_lean_too_and_takes_symbols_for_a_named_crate() {
    let rig = Rig::new("");
    let r = rig.mock(&["--plain"]);
    assert!(r.ok, "{}", r.text);
    assert_eq!(
        r.lean(),
        set(&["alone", "base", "mid", "top"]),
        "{:?}",
        r.compiled
    );
    assert!(r.symbols().is_empty(), "{:?}", r.compiled);

    let r = rig.mock(&["--plain", "-p", "alone", "--debug"]);
    assert!(r.ok, "{}", r.text);
    assert_eq!(r.symbols(), set(&["alone"]), "{:?}", r.compiled);
    assert!(r.lean().is_empty(), "{:?}", r.compiled);
}

/// `lean = false` hands the profile back: the manifest and the cargo config
/// decide, here to debug info on every crate, and only the crates named get
/// the level of `[test] debug`.
#[test]
fn a_clone_can_keep_its_own_debug_settings() {
    let rig = Rig::new("lean = false\n");
    let r = rig.mock(&["--plain"]);
    assert!(r.ok, "{}", r.text);
    assert_eq!(
        r.symbols(),
        set(&["alone", "base", "mid", "top"]),
        "{:?}",
        r.compiled
    );
}

/// A crate outside the workspace is lean too, whichever way a manifest or a
/// cargo config asks for symbols on it: named for itself, or with `"*"`, which
/// is every dependency. Each asks for full debug info and the run must still
/// compile `dep` without.
#[test]
fn a_dependency_outside_the_workspace_stays_lean_whoever_asks_for_symbols_on_it() {
    let named = "[profile.dev.package.dep]\ndebug = 2\n\n[profile.test.package.dep]\ndebug = 2\n";
    let star =
        "[profile.dev.package.\"*\"]\ndebug = 2\n\n[profile.test.package.\"*\"]\ndebug = 2\n";
    for (what, manifest_extra, home_extra) in [
        ("manifest names dep", named, ""),
        ("manifest says *", star, ""),
        ("cargo config names dep", "", named),
        ("cargo config says *", "", star),
    ] {
        let rig = Rig::with(&Variant {
            manifest_extra,
            home_extra,
            dep: true,
            ..Variant::default()
        });
        let r = rig.mock(&["--plain"]);
        assert!(r.ok, "{what}:\n{}", r.text);
        assert_eq!(
            r.lean(),
            set(&["alone", "base", "dep", "mid", "top"]),
            "{what}: {:?}",
            r.compiled
        );
        assert!(r.symbols().is_empty(), "{what}: {:?}", r.compiled);
    }
}

/// Rustflags are not cargo profile settings, so no `--config` of ours overrides
/// them: they are reported, and the plan stops saying the rest is lean.
#[test]
fn rustflags_that_give_debug_info_are_reported_and_the_plan_stops_claiming_lean() {
    let rig = Rig::new("");
    // Env: `-g`, and the same spelled the long way.
    for flags in ["-g", "-C debuginfo=2", "-Cdebuginfo=1"] {
        let r = rig.mock_env(&["--plain", "-p", "alone", "--debug"], &[(
            "RUSTFLAGS",
            flags,
        )]);
        assert!(r.ok, "{flags}:\n{}", r.text);
        assert!(
            r.text.contains("RUSTFLAGS") && r.text.contains("every crate"),
            "{flags}: the run names the variable:\n{}",
            r.text
        );
        assert!(
            r.text.contains("symbols for alone (asked for)"),
            "{flags}:\n{}",
            r.text
        );
        assert!(
            !r.text
                .contains("every other crate is built without debug info"),
            "{flags}: the plan claims what rustflags undo:\n{}",
            r.text
        );
    }
    // The control: no flags, or flags that add none, and the claim stands.
    for flags in [None, Some("-C debuginfo=0"), Some("-C opt-level=1")] {
        let env: Vec<(&str, &str)> = flags.map(|f| ("RUSTFLAGS", f)).into_iter().collect();
        let r = rig.mock_env(&["--plain", "-p", "alone", "--debug"], &env);
        assert!(r.ok, "{flags:?}:\n{}", r.text);
        assert!(
            !r.text.contains("RUSTFLAGS"),
            "{flags:?}: nothing to report:\n{}",
            r.text
        );
        assert!(
            r.text.contains(
                "symbols for alone (asked for); every other crate is built without debug info"
            ),
            "{flags:?}:\n{}",
            r.text
        );
    }
}

/// Rustflags in a cargo config are found wherever cargo reads them, `[build]`
/// and `[target.*]` alike.
#[test]
fn rustflags_in_a_cargo_config_are_reported_by_file() {
    for extra in [
        "[build]\nrustflags = [\"-C\", \"debuginfo=2\"]\n",
        "[build]\nrustflags = \"-g\"\n",
        "[target.'cfg(unix)']\nrustflags = [\"-g\"]\n",
    ] {
        let rig = Rig::with(&Variant {
            home_extra: extra,
            ..Variant::default()
        });
        let r = rig.mock(&["--plain", "-p", "alone", "--debug"]);
        assert!(r.ok, "{extra}:\n{}", r.text);
        assert!(
            r.text.contains("config.toml sets") && r.text.contains("rustflags"),
            "{extra}:\n{}",
            r.text
        );
        assert!(
            !r.text
                .contains("every other crate is built without debug info"),
            "{extra}:\n{}",
            r.text
        );
    }
}

/// A plain run reads no report, so it cannot say which test failed, but one
/// that passes passed the whole tree: what the last nextest run recorded as
/// failed is no longer.
#[test]
fn a_plain_run_that_passes_clears_what_failed() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let rig = Rig::new("");
    assert!(rig.mock(&[]).ok);
    rig.fail_mid(true);
    assert!(!rig.mock(&[]).ok);
    // Recorded: the next run names it.
    let r = rig.mock(&["--full"]);
    assert!(
        r.text.contains("symbols for mid (failed last run)"),
        "{}",
        r.text
    );
    // Mended, and passed by a plain run.
    rig.fail_mid(false);
    let r = rig.mock(&["--plain"]);
    assert!(r.ok, "{}", r.text);
    let r = rig.mock(&["--full"]);
    assert!(r.ok, "{}", r.text);
    assert!(!r.text.contains("symbols for"), "{}", r.text);
}
