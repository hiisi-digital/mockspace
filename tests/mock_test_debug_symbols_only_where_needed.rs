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

/// `base`, `mid` depending on it, `top` depending on `mid`, and `alone`
/// depending on nothing. `mid` fails while `crates/mid/fail` exists.
fn fixture(root: &Path, test_table: &str) {
    write(
        &root.join("mockspace.toml"),
        &format!("project_name = \"fixture\"\nmock_dir = \"mock\"\n\n[test]\n{test_table}"),
    );
    write(&root.join(".gitignore"), "target/\n");
    write(
        &root.join("mock/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/base\", \"crates/mid\", \"crates/top\", \"crates/alone\"]\n\n\
         [profile.dev]\ndebug = 2\n\n[profile.test]\ndebug = 2\n",
    );
    let pkg = |name: &str, deps: &str| {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\n{deps}"
        )
    };
    let lib = |body: &str| format!("pub fn f() -> u32 {{ 1 }}\n#[test]\nfn t() {{ {body} }}\n");
    for (name, deps, body) in [
        ("base", "", ""),
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
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = base.join("repo");
        let home = base.join("cargo-home");
        let shim = base.join("rustc-log");
        let log = base.join("rustc.log");
        fixture(&root, test_table);
        // What a helper container carries: a cargo config under `CARGO_HOME` that
        // sets debug info for every build and silently beats the manifest.
        write(
            &home.join("config.toml"),
            "[profile.dev]\ndebug = \"line-tables-only\"\n\n[profile.test]\ndebug = \"line-tables-only\"\n",
        );
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
        let _ = fs::remove_file(&self.log);
        let o = Command::new(env!("CARGO_BIN_EXE_mockspace"))
            .args(["--dir", "mock", "test"])
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
            .env("MS_RUSTC_LOG", &self.log)
            .output()
            .unwrap();
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
        compiled.retain(|c, _| ["base", "mid", "top", "alone"].contains(&c.as_str()));
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
