//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock test` under nextest, end to end, over one repository through the
//! sequence a piece of work goes through: a first run, a run after nothing, a
//! change in a dependency, a full run, a change in content a test reads, and a
//! cheap pass.
//!
//! The unit tests under `src/suite/` pin each decision. This is the arm that
//! fails if the decisions are never consulted, if the history is never written
//! back, or if nextest is handed a config or a filterset it refuses.
//!
//! Needs cargo-nextest, and says so and passes without it, since a machine
//! without it runs the members under `cargo test` and that path is
//! `mock_test_reaches_every_tree.rs`'s.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

fn append(p: &Path, s: &str) {
    let mut t = fs::read_to_string(p).unwrap();
    t.push_str(s);
    fs::write(p, t).unwrap();
}

/// `base` is a library, `uses` depends on it, reads `content/rows.txt` with
/// `include_str!` and has one test that sleeps past the heavy threshold, and
/// `alone` depends on nothing.
fn fixture(root: &Path) {
    write(
        &root.join("mockspace.toml"),
        "project_name = \"fixture\"\nmock_dir = \"mock\"\n\n[test]\nheavy_after_secs = 0.5\n",
    );
    write(&root.join(".gitignore"), "target/\n");
    write(
        &root.join("mock/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/base\", \"crates/uses\", \"crates/alone\"]\n",
    );
    let pkg = |name: &str, deps: &str| {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\n{deps}"
        )
    };
    write(&root.join("mock/crates/base/Cargo.toml"), &pkg("base", ""));
    write(
        &root.join("mock/crates/base/src/lib.rs"),
        "pub fn four() -> u32 { 4 }\n#[test]\nfn quick_base() { assert_eq!(four(), 4); }\n",
    );
    write(
        &root.join("mock/crates/uses/Cargo.toml"),
        &pkg("uses", "base = { path = \"../base\" }\n"),
    );
    write(
        &root.join("mock/crates/uses/src/lib.rs"),
        "pub const ROWS: &str = include_str!(\"../../../../content/rows.txt\");\n\
         #[test]\nfn quick_uses() { assert_eq!(base::four(), 4); }\n\
         #[test]\nfn slow_drawn() {\n\
         \x20   std::thread::sleep(std::time::Duration::from_millis(900));\n\
         \x20   assert!(!ROWS.is_empty());\n}\n",
    );
    write(
        &root.join("mock/crates/alone/Cargo.toml"),
        &pkg("alone", ""),
    );
    write(
        &root.join("mock/crates/alone/src/lib.rs"),
        "#[test]\nfn quick_alone() {}\n",
    );
    write(&root.join("content/rows.txt"), "one row\n");
    let ok = Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git init");
}

fn mock(root: &Path, args: &[&str]) -> (bool, String) {
    mock_with(root, args, &[])
}

fn mock_with(root: &Path, args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_mockspace"));
    cmd.args(["--dir", "mock", "test"])
        .args(args)
        .current_dir(root)
        // The fixture's own `target/`, whatever the caller shares.
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET_DIR")
        .env("CARGO_BUILD_JOBS", "2");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let o: Output = cmd.output().unwrap();
    let t = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.success(), t)
}

fn nextest_here() -> bool {
    Command::new("cargo")
        .args(["nextest", "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn a_piece_of_work_runs_only_what_it_reaches() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);

    // First run: no history, everything, and the timings recorded.
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");
    for m in ["alone", "base", "uses"] {
        assert!(
            t.contains(&format!("run      {m}: no earlier pass on record")),
            "first run, {m}:\n{t}"
        );
    }
    assert!(
        t.contains("uses slow_drawn"),
        "the slowest list names the slow test:\n{t}"
    );

    // Nothing changed: nothing runs.
    let (ok, t) = mock(root, &[]);
    assert!(ok, "second run:\n{t}");
    assert!(t.contains("nothing to run"), "after none:\n{t}");

    // A one-line change in a dependency: it and its dependent, not `alone`.
    append(
        &root.join("mock/crates/base/src/lib.rs"),
        "pub fn three() -> u32 { 3 }\n",
    );
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after a dependency changed:\n{t}");
    assert!(t.contains("run      base: its own inputs moved"), "{t}");
    assert!(t.contains("run      uses: moved: base"), "{t}");
    assert!(
        t.contains("settled  1 member(s) passed at what they are now: alone"),
        "{t}"
    );
    assert!(
        t.contains("heavy    1 test(s)"),
        "the slow test is known heavy now:\n{t}"
    );
    assert!(
        !t.contains("cached"),
        "its pass was at the old fingerprint:\n{t}"
    );

    // A full run: every member, and the heavy test's pass still stands.
    let (ok, t) = mock(root, &["--full"]);
    assert!(ok, "full:\n{t}");
    assert!(t.contains("run      alone: asked for"), "{t}");
    assert!(t.contains("cached   1 heavy test(s) not run"), "{t}");
    assert!(t.contains("uses slow_drawn"), "{t}");
    // ... unless the cache is off.
    let (ok, t) = mock(root, &["--full", "--no-cache"]);
    assert!(ok, "full without cache:\n{t}");
    assert!(!t.contains("cached"), "{t}");

    // Content the dependent includes: only it runs, and the heavy test with it.
    append(&root.join("content/rows.txt"), "two rows\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after content changed:\n{t}");
    assert!(
        t.contains("run      uses: its own inputs moved (reads 1 path(s) outside itself)"),
        "{t}"
    );
    assert!(t.contains("settled  2 member(s)"), "{t}");
    assert!(!t.contains("cached"), "{t}");

    // A cheap pass after the dependent moved defers its heavy test, and leaves
    // it owed: the next run runs `uses` again, heavy test included.
    append(&root.join("mock/crates/uses/src/lib.rs"), "// one line\n");
    let (ok, t) = mock(root, &["--cheap"]);
    assert!(ok, "cheap:\n{t}");
    assert!(t.contains("deferred 1 heavy test(s) not run"), "{t}");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after cheap:\n{t}");
    assert!(
        t.contains("run      uses"),
        "a cheap pass must not leave its member green:\n{t}"
    );
    assert!(!t.contains("deferred"), "{t}");

    // A failing test fails the run and leaves its member owed.
    append(
        &root.join("mock/crates/alone/src/lib.rs"),
        "#[test]\nfn breaks() { panic!(\"this arm must fail the run\"); }\n",
    );
    let (ok, t) = mock(root, &[]);
    assert!(!ok, "a failing test did not fail the run:\n{t}");
    let (ok, t) = mock(root, &[]);
    assert!(
        !ok && t.contains("run      alone"),
        "a failed member must run again:\n{t}"
    );
}

/// Every file under the fixture's test history, as one string.
fn history_text(root: &Path) -> String {
    fn walk(p: &Path, out: &mut String) {
        for e in fs::read_dir(p).into_iter().flatten().flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push_str(&fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    let mut s = String::new();
    walk(&root.join("mock/test-history"), &mut s);
    s
}

/// A heavy test in its own binary, `alone`'s `tests/slow.rs`.
fn add_slow_binary(root: &Path) {
    write(
        &root.join("mock/crates/alone/tests/slow.rs"),
        "#[test]\nfn slow_gone() { std::thread::sleep(std::time::Duration::from_millis(900)); }\n",
    );
}

#[test]
fn a_deleted_heavy_binary_does_not_stop_the_next_run() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    add_slow_binary(root);

    let (ok, t) = mock(root, &["--full"]);
    assert!(ok, "first run:\n{t}");
    assert!(
        history_text(root).contains("slow_gone"),
        "the heavy test is on record once it ran"
    );

    // Its binary is deleted. The cache would skip it by name in the filterset,
    // and the config would group it, and nextest refuses both for a binary it
    // does not have.
    fs::remove_file(root.join("mock/crates/alone/tests/slow.rs")).unwrap();
    let (ok, t) = mock(root, &["--full"]);
    assert!(ok, "after the binary was deleted:\n{t}");
    assert!(
        !history_text(root).contains("slow_gone"),
        "the history still names a test that is gone:\n{}",
        history_text(root)
    );

    // The same with flags that only shape a run, which `nextest list` refuses.
    add_slow_binary(root);
    let (ok, t) = mock(root, &["--full", "--no-capture", "--retries", "1"]);
    assert!(ok, "second first run:\n{t}");
    assert!(history_text(root).contains("slow_gone"));
    fs::remove_file(root.join("mock/crates/alone/tests/slow.rs")).unwrap();
    let (ok, t) = mock(root, &["--full", "--no-capture", "--retries", "1"]);
    assert!(
        ok,
        "after the binary was deleted, with run-only flags:\n{t}"
    );
    assert!(
        !history_text(root).contains("slow_gone"),
        "the history still names a test that is gone:\n{}",
        history_text(root)
    );
}

#[test]
fn a_run_with_nothing_to_run_builds_nothing() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");

    fs::remove_dir_all(root.join("mock/target")).unwrap();
    let (ok, t) = mock(root, &[]);
    assert!(ok && t.contains("nothing to run"), "second run:\n{t}");
    assert!(
        !root.join("mock/target/debug").exists(),
        "a run with nothing to run compiled the members:\n{t}"
    );
}

/// `base` and `alone` read the level cargo built them at from their build
/// scripts, which are handed `OPT_LEVEL` per package, and their tests fail
/// unless it is the level `[test.opt_level]` gave: 3 and `s`.
fn opt_level_fixture(root: &Path) {
    fixture(root);
    write(
        &root.join("mockspace.toml"),
        "project_name = \"fixture\"\nmock_dir = \"mock\"\n\n[test.opt_level]\nbase = 3\nalone = \"s\"\n",
    );
    let build = "fn main() { println!(\"cargo:rustc-env=BUILT_AT={}\", std::env::var(\"OPT_LEVEL\").unwrap()); }\n";
    write(&root.join("mock/crates/base/build.rs"), build);
    write(
        &root.join("mock/crates/base/src/lib.rs"),
        "pub fn four() -> u32 { 4 }\n#[test]\nfn quick_base() { assert_eq!(env!(\"BUILT_AT\"), \"3\"); }\n",
    );
    write(&root.join("mock/crates/alone/build.rs"), build);
    write(
        &root.join("mock/crates/alone/src/lib.rs"),
        "#[test]\nfn quick_alone() { assert_eq!(env!(\"BUILT_AT\"), \"s\"); }\n",
    );
}

/// A member's `[test.opt_level]` reaches what is compiled.
#[test]
fn a_members_opt_level_reaches_the_build() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    opt_level_fixture(root);
    let (ok, t) = mock(root, &["--full"]);
    assert!(ok, "a member's opt level did not reach its build:\n{t}");
}

/// The table holds however the run is taken: `--plain`, no nextest and an
/// unreadable graph all run `cargo test`, and it gets the same config.
#[test]
fn a_members_opt_level_reaches_a_plain_cargo_test() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    opt_level_fixture(root);
    let (ok, t) = mock(root, &["--plain"]);
    assert!(
        ok,
        "a member's opt level did not reach a plain cargo test:\n{t}"
    );
}

/// A change to `[test.opt_level]` changes what was built, so a member recorded
/// green before it is not green after.
#[test]
fn a_change_to_the_opt_levels_runs_what_passed_before() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");
    let (ok, t) = mock(root, &[]);
    assert!(ok && t.contains("nothing to run"), "second run:\n{t}");

    append(
        &root.join("mockspace.toml"),
        "\n[test.opt_level]\nbase = 3\nuses = 2\nalone = \"s\"\n",
    );
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after the table was added:\n{t}");
    assert!(
        !t.contains("nothing to run"),
        "a pass recorded at the manifest's levels stood after the table moved them:\n{t}"
    );
    for m in ["alone", "base", "uses"] {
        assert!(t.contains(&format!("run      {m}")), "{m}:\n{t}");
    }

    // The next change of level runs them again, and an unchanged table does not.
    let (ok, t) = mock(root, &[]);
    assert!(ok && t.contains("nothing to run"), "unchanged table:\n{t}");
    // ... and with nothing to run the config is not written again.
    let written = root.join("mock/target/mockspace-test-state/cargo.toml");
    assert!(written.is_file(), "the run that ran wrote the config");
    fs::remove_file(&written).unwrap();
    let (ok, t) = mock(root, &[]);
    assert!(ok && t.contains("nothing to run"), "unchanged table:\n{t}");
    assert!(
        !written.exists(),
        "a run with nothing to run wrote the config"
    );
    let toml = root.join("mockspace.toml");
    let text = fs::read_to_string(&toml)
        .unwrap()
        .replace("base = 3", "base = 1");
    fs::write(&toml, text).unwrap();
    let (ok, t) = mock(root, &[]);
    assert!(ok && !t.contains("nothing to run"), "a level changed:\n{t}");
}

/// kaski's shape of the same failure: the heavy test lives in an example
/// binary, which the history remembers after the example is deleted.
#[test]
fn a_deleted_example_with_a_heavy_test_does_not_stop_the_next_run() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    let manifest = root.join("mock/crates/alone/Cargo.toml");
    let plain = fs::read_to_string(&manifest).unwrap();
    append(
        &manifest,
        "\n[[example]]\nname = \"figure_preview\"\ntest = true\n",
    );
    write(
        &root.join("mock/crates/alone/examples/figure_preview.rs"),
        "fn main() {}\n#[test]\nfn slow_preview() { std::thread::sleep(std::time::Duration::from_millis(900)); }\n",
    );

    let (ok, t) = mock(root, &["--full"]);
    assert!(ok, "first run:\n{t}");
    assert!(
        history_text(root).contains("slow_preview"),
        "the example's test is on record once it ran"
    );

    fs::remove_file(root.join("mock/crates/alone/examples/figure_preview.rs")).unwrap();
    fs::write(&manifest, plain).unwrap();
    let (ok, t) = mock(root, &["--full", "--workspace"]);
    assert!(ok, "after the example was deleted:\n{t}");
    assert!(
        !history_text(root).contains("slow_preview"),
        "the history still names a test that is gone:\n{}",
        history_text(root)
    );
}

// ---- test binaries ----

/// `base` has one integration test, `x`. `uses` depends on it and has a library
/// test and four integration-test targets: `one`, which reads `fix/a.json`;
/// `two`, which declares `mod common`; `envy`, which reads the environment and
/// so may read anything; and the directory form `dir`. `tests/data/note.txt` is
/// named by none of them.
fn binaries_fixture(root: &Path) {
    write(
        &root.join("mockspace.toml"),
        "project_name = \"fixture\"\nmock_dir = \"mock\"\n",
    );
    write(&root.join(".gitignore"), "target/\n");
    write(
        &root.join("mock/Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"crates/base\", \"crates/uses\"]\n",
    );
    let pkg = |name: &str, deps: &str| {
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\n{deps}"
        )
    };
    write(&root.join("mock/crates/base/Cargo.toml"), &pkg("base", ""));
    write(
        &root.join("mock/crates/base/src/lib.rs"),
        "pub fn four() -> u32 { 4 }\n#[test]\nfn lib_base() { assert_eq!(four(), 4); }\n",
    );
    write(
        &root.join("mock/crates/base/tests/x.rs"),
        "#[test]\nfn x() { assert_eq!(base::four(), 4); }\n",
    );
    write(
        &root.join("mock/crates/uses/Cargo.toml"),
        &pkg("uses", "base = { path = \"../base\" }\n"),
    );
    write(
        &root.join("mock/crates/uses/src/lib.rs"),
        "pub fn five() -> u32 { base::four() + 1 }\n#[test]\nfn lib_uses() { assert_eq!(five(), 5); }\n",
    );
    let t = |name: &str, body: &str| {
        write(
            &root.join(format!("mock/crates/uses/tests/{name}.rs")),
            &format!("#[test]\nfn {name}_a() {{ {body} }}\n#[test]\nfn {name}_b() {{ {body} }}\n"),
        );
    };
    t("one", "assert!(include_str!(\"fix/a.json\").len() > 0);");
    write(&root.join("mock/crates/uses/tests/fix/a.json"), "{}\n");
    write(
        &root.join("mock/crates/uses/tests/two.rs"),
        "mod common;\n#[test]\nfn two_a() { assert_eq!(common::six(), 6); }\n",
    );
    write(
        &root.join("mock/crates/uses/tests/common/mod.rs"),
        "pub fn six() -> u32 { 6 }\n",
    );
    write(
        &root.join("mock/crates/uses/tests/envy.rs"),
        "#[test]\nfn envy_a() {\n    assert!(!env!(\"CARGO_MANIFEST_DIR\").is_empty());\n    assert!(std::env::var(\"FAIL_ENVY\").is_err(), \"told to fail\");\n}\n",
    );
    write(
        &root.join("mock/crates/uses/tests/dir/main.rs"),
        "mod part;\n#[test]\nfn dir_a() { assert_eq!(part::seven(), 7); }\n",
    );
    write(
        &root.join("mock/crates/uses/tests/dir/part.rs"),
        "pub fn seven() -> u32 { 7 }\n",
    );
    write(
        &root.join("mock/crates/uses/tests/data/note.txt"),
        "nobody names this\n",
    );
    let ok = Command::new("git")
        .args(["init", "-q"])
        .current_dir(root)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git init");
}

/// The tests and binaries a run says it ran, from its closing line.
fn ran(t: &str) -> Option<(usize, usize)> {
    let line = t.lines().find(|l| l.contains("binary(ies) ran,"))?;
    let rest = line.strip_prefix("mock test: ")?;
    let (tests, rest) = rest.split_once(" test(s) in ")?;
    let (bins, _) = rest.split_once(" binary(ies) ran")?;
    Some((tests.parse().ok()?, bins.parse().ok()?))
}

/// The binaries a run says it left out.
fn reused(t: &str) -> Vec<String> {
    let mut lines = t
        .lines()
        .skip_while(|l| !l.contains("test binary(ies) not run"));
    if lines.next().is_none() {
        return Vec::new();
    }
    lines
        .take_while(|l| l.starts_with("               "))
        .map(|l| l.trim().to_string())
        .collect()
}

#[test]
fn a_change_confined_to_one_test_file_runs_that_binary_alone() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    binaries_fixture(root);
    let uses = root.join("mock/crates/uses");

    // First run: every binary of both members.
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");
    // base: lib 1 + x 1; uses: lib 1 + one 2 + two 1 + envy 1 + dir 1.
    assert_eq!(ran(&t), Some((8, 7)), "{t}");
    assert!(reused(&t).is_empty(), "{t}");

    // Only `tests/two.rs` touched: it runs, and `envy`, which may read anything.
    // The library, `one` and `dir` do not, and neither does `base`.
    append(&uses.join("tests/two.rs"), "// one line\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after one test file changed:\n{t}");
    assert_eq!(ran(&t), Some((2, 2)), "{t}");
    assert_eq!(reused(&t), vec!["uses", "uses::dir", "uses::one"], "{t}");
    assert!(!t.contains("run      base"), "{t}");

    // The helper it declares: the targets that declare it, and the wide one.
    append(&uses.join("tests/common/mod.rs"), "// one line\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after a helper module changed:\n{t}");
    assert_eq!(ran(&t), Some((2, 2)), "{t}");
    assert_eq!(reused(&t), vec!["uses", "uses::dir", "uses::one"], "{t}");

    // A data file one test names, and the directory form's own part.
    append(&uses.join("tests/fix/a.json"), "\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after a data file changed:\n{t}");
    assert_eq!(ran(&t), Some((3, 2)), "{t}");
    assert_eq!(reused(&t), vec!["uses", "uses::dir", "uses::two"], "{t}");
    append(&uses.join("tests/dir/part.rs"), "// one line\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after the directory form changed:\n{t}");
    assert_eq!(ran(&t), Some((2, 2)), "{t}");
    assert_eq!(reused(&t), vec!["uses", "uses::one", "uses::two"], "{t}");

    // A file in tests/ no binary names is shared: everything of `uses` runs.
    append(&uses.join("tests/data/note.txt"), "more\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after an unclaimed file changed:\n{t}");
    assert_eq!(ran(&t), Some((6, 5)), "{t}");
    assert!(reused(&t).is_empty(), "{t}");

    // So is the library: everything of `uses`, and nothing of `base`.
    append(&uses.join("src/lib.rs"), "// one line\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after the library changed:\n{t}");
    assert_eq!(ran(&t), Some((6, 5)), "{t}");
    assert!(!t.contains("run      base"), "{t}");

    // A dependency's tests are not what its dependents are built from.
    append(&root.join("mock/crates/base/tests/x.rs"), "// one line\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after a dependency's test changed:\n{t}");
    assert_eq!(ran(&t), Some((1, 1)), "{t}");
    assert!(!t.contains("run      uses"), "{t}");
    assert_eq!(reused(&t), vec!["base"], "{t}");

    // Its library is.
    append(&root.join("mock/crates/base/src/lib.rs"), "// one line\n");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after a dependency's library changed:\n{t}");
    assert_eq!(ran(&t), Some((8, 7)), "{t}");
    assert!(t.contains("run      uses: moved: base"), "{t}");
    assert!(reused(&t).is_empty(), "{t}");

    // Nothing changed: nothing runs.
    let (ok, t) = mock(root, &[]);
    assert!(ok && t.contains("nothing to run"), "{t}");

    // A full run asks for every binary, and so does one without the cache.
    let (ok, t) = mock(root, &["--full"]);
    assert!(ok, "full:\n{t}");
    assert_eq!(ran(&t), Some((8, 7)), "{t}");
    let (ok, t) = mock(root, &["--full", "--no-cache"]);
    assert!(ok, "full without cache:\n{t}");
    assert_eq!(ran(&t), Some((8, 7)), "{t}");
    append(&uses.join("tests/two.rs"), "// another line\n");
    let (ok, t) = mock(root, &["--no-cache"]);
    assert!(ok, "changed without cache:\n{t}");
    assert_eq!(ran(&t), Some((6, 5)), "{t}");
    assert!(reused(&t).is_empty(), "{t}");
}

/// A member in which one binary failed reruns only what did not pass: the
/// failing binary, and nothing that passed at inputs that have not moved.
#[test]
fn a_member_with_a_failing_binary_reruns_only_that_binary() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    binaries_fixture(root);
    let uses = root.join("mock/crates/uses");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");

    append(
        &uses.join("tests/one.rs"),
        "#[test]\nfn breaks() { panic!(\"this arm must fail the run\"); }\n",
    );
    let (ok, t) = mock(root, &[]);
    assert!(!ok, "a failing test did not fail the run:\n{t}");
    assert_eq!(reused(&t), vec!["uses", "uses::dir", "uses::two"], "{t}");

    // Run again with nothing changed: the member is not green, and what passed
    // (the wide binary among it, which passed at its new inputs) is not rerun.
    // Only the binary that failed is.
    let (ok, t) = mock(root, &[]);
    assert!(!ok, "{t}");
    assert!(t.contains("run      uses"), "{t}");
    assert_eq!(
        reused(&t),
        vec!["uses", "uses::dir", "uses::envy", "uses::two"],
        "{t}"
    );
    assert_eq!(ran(&t).map(|(_, b)| b), Some(1), "{t}");

    // Mended: the member settles, and the next run has nothing to do.
    let one = uses.join("tests/one.rs");
    let text = fs::read_to_string(&one).unwrap();
    fs::write(
        &one,
        text.replace("panic!(\"this arm must fail the run\")", ""),
    )
    .unwrap();
    let (ok, t) = mock(root, &[]);
    assert!(ok, "after the mend:\n{t}");
    assert_eq!(reused(&t), vec!["uses", "uses::dir", "uses::two"], "{t}");
    let (ok, t) = mock(root, &[]);
    assert!(ok && t.contains("nothing to run"), "{t}");
}

/// A binary that failed is rerun even where its inputs are the ones it once
/// passed at, which is what a test does when it depends on something no
/// fingerprint holds, and the binaries that passed beside it are not.
#[test]
fn a_binary_that_last_failed_is_not_taken_on_an_earlier_pass() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    binaries_fixture(root);
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");

    // The same inputs, a different environment, and the whole suite asked for.
    let (ok, t) = mock_with(root, &["--full"], &[("FAIL_ENVY", "1")]);
    assert!(!ok, "the environment did not reach the test:\n{t}");

    let (ok, t) = mock(root, &[]);
    assert!(ok, "the next run, in a mended environment:\n{t}");
    assert_eq!(
        reused(&t),
        vec!["uses", "uses::dir", "uses::one", "uses::two"],
        "{t}"
    );
    assert_eq!(
        ran(&t),
        Some((1, 1)),
        "only the binary that failed is rerun:\n{t}"
    );
}

/// Where the build goes is not where nextest keeps its report. With one target
/// directory shared by every clone, which is how the workspace runs, the report
/// was looked for under it and never found, so no timing and no pass was
/// recorded and the next run started from nothing.
#[test]
fn a_shared_target_dir_does_not_lose_the_run_record() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fixture(root);
    let shared = tmp.path().join("shared-target");
    let env = [("CARGO_TARGET_DIR", shared.to_str().unwrap())];

    let (ok, t) = mock_with(root, &["--full"], &env);
    assert!(ok, "first run:\n{t}");
    assert!(
        shared.join("debug").is_dir(),
        "the build did not go to the shared directory:\n{t}"
    );
    let record = history_text(root);
    assert!(
        record.contains("slow_drawn"),
        "the slow test's timing was lost:\n{record}"
    );
    assert!(
        !record.contains("\"bins\": {}"),
        "a binary that passed was not recorded:\n{record}"
    );
    let (ok, t) = mock_with(root, &[], &env);
    assert!(ok && t.contains("nothing to run"), "second run:\n{t}");
}

/// `--test one` runs one target of the member. The member is not thereby green:
/// `two` changed and has not run since.
#[test]
fn a_run_narrowed_to_one_target_leaves_the_others_owed() {
    if !nextest_here() {
        eprintln!("cargo-nextest is not installed; the nextest path is not exercised here");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    binaries_fixture(root);
    let uses = root.join("mock/crates/uses");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "first run:\n{t}");
    append(&uses.join("tests/one.rs"), "// one line\n");
    append(&uses.join("tests/two.rs"), "// one line\n");
    let (ok, t) = mock(root, &["--test", "one"]);
    assert!(ok, "narrowed run:\n{t}");
    let (ok, t) = mock(root, &[]);
    assert!(ok, "{t}");
    assert!(
        !t.contains("nothing to run"),
        "two changed and never ran:\n{t}"
    );
}

/// Takes the machine's git environment out of this process before `main`.
#[path = "common/ambient.rs"]
mod ambient;
