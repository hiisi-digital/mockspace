//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A maker run end to end: a real tool crate compiled into the project's
//! cdylib, run by the engine binary in a real git worktree, with what it wrote
//! observed and held to what it declared.
//!
//! The unit tests beside `maker_faults` and `tool_writes` pin the audit
//! and the observation apart. This is the only test that has the engine take
//! the snapshot around a real run and turn a stray write into an exit code, so
//! deleting that wiring in `entry::tool::run` fails here and nowhere else.
//!
//! `#[ignore]` because it runs `cargo build`, matching `tool_cdylib.rs`;
//! `tests/rust_e2e_test.sh` runs it with `--ignored`.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn dep_spec() -> String {
    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    format!("{{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}")
}

/// One crate registering every arm: an honest maker that strays when asked
/// to, a maker declaring nothing, a check wearing a maker's declaration, and
/// a check whose `no-failing-case` is false, so one build serves them all.
const TOOLS: &str = r#"use mockspace::LintError;
use mockspace::tool::{NotALint, Outcome, Purpose, Tool, ToolContext, ToolReport};

pub struct Gen;
impl Tool for Gen {
    fn name(&self) -> &'static str { "gen" }
    fn description(&self) -> &'static str { "write the generated page" }
    fn purpose(&self) -> Purpose { Purpose::Make { writes: &["gen/**"] } }
    fn run(&self, ctx: &ToolContext<'_>) -> ToolReport {
        std::fs::create_dir_all(ctx.repo_root.join("gen")).unwrap();
        std::fs::write(ctx.repo_root.join("gen/page.md"), "made\n").unwrap();
        if ctx.args.first() == Some(&"block") {
            return ToolReport {
                outcome: Outcome::Findings(vec![LintError::error(
                    "src/lib.rs".to_string(), 1, "gen", "coverage under 80".to_string(),
                )]),
                output: String::new(),
            };
        }
        if ctx.args.first() == Some(&"stray") {
            std::fs::write(ctx.repo_root.join("stray.md"), "not declared\n").unwrap();
        }
        ToolReport::reported("", 1)
    }
}

pub struct Bare;
impl Tool for Bare {
    fn name(&self) -> &'static str { "bare" }
    fn description(&self) -> &'static str { "a maker that names nothing" }
    fn purpose(&self) -> Purpose { Purpose::Make { writes: &[] } }
    fn run(&self, ctx: &ToolContext<'_>) -> ToolReport {
        std::fs::write(ctx.repo_root.join("bare-ran.md"), "it ran\n").unwrap();
        ToolReport::reported("", 1)
    }
}

pub struct Dodge;
impl Tool for Dodge {
    fn name(&self) -> &'static str { "dodge" }
    fn description(&self) -> &'static str { "a check wearing a maker's declaration" }
    fn purpose(&self) -> Purpose { Purpose::Make { writes: &["never/written.md"] } }
    fn run(&self, _: &ToolContext<'_>) -> ToolReport {
        ToolReport {
            outcome: Outcome::Findings(vec![LintError::error(
                "x".to_string(), 1, "dodge", "a failing case".to_string(),
            )]),
            output: String::new(),
        }
    }
}

pub struct Liar;
impl Tool for Liar {
    fn name(&self) -> &'static str { "liar" }
    fn description(&self) -> &'static str { "declares no failing case and fails one" }
    fn purpose(&self) -> Purpose { Purpose::Check(NotALint::NoFailingCase) }
    fn run(&self, _: &ToolContext<'_>) -> ToolReport {
        ToolReport {
            outcome: Outcome::Findings(vec![LintError::error(
                "x".to_string(), 1, "liar", "a failing case".to_string(),
            )]),
            output: String::new(),
        }
    }
}

mockspace::lint_pack! {
    tools: [Gen, Bare, Dodge, Liar],
}
"#;

fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

/// A committed repository with the tool crate under `mock/tools/gen/`.
fn fixture(root: &Path) -> std::path::PathBuf {
    let mock = files(root);
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@t"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    mock
}

/// The same project with a `.git` directory git cannot read, so the root is
/// found and `git status` fails.
fn fixture_without_git(root: &Path) -> std::path::PathBuf {
    let mock = files(root);
    fs::create_dir_all(root.join(".git")).unwrap();
    mock
}

fn files(root: &Path) -> std::path::PathBuf {
    let mock = root.join("mock");
    let dir = mock.join("tools").join("gen");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-gen\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             publish = false\n\n[dependencies]\nmockspace = {}\n",
            dep_spec()
        ),
    )
    .unwrap();
    fs::write(dir.join("src").join("lib.rs"), TOOLS).unwrap();
    fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    fs::write(
        mock.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    )
    .unwrap();
    fs::write(root.join(".gitignore"), "target/\nCargo.lock\n").unwrap();
    mock
}

fn engine(mock: &Path, args: &[&str]) -> Output {
    let dep = dep_spec();
    let mut all = vec!["--mockspace-lint-rules-dep", dep.as_str()];
    all.extend_from_slice(args);
    Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(&all)
        // See `tool_not_found_messages.rs`: an inherited shared target dir
        // sends the cdylib somewhere the engine does not look.
        .env_remove("CARGO_TARGET_DIR")
        // Discovery stops at the fixture, so a repository above the temp
        // directory can never answer for it.
        .env("GIT_CEILING_DIRECTORIES", mock.parent().and_then(Path::parent).unwrap_or(mock))
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .current_dir(mock)
        .output()
        .expect("the binary runs")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_maker_is_held_to_what_it_declares_it_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path());

    // Inside the declaration: reported, and a success.
    let out = engine(&mock, &["gen"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{err}");
    assert!(err.contains("gen: wrote 1 path(s)"), "{err}");
    assert!(err.contains("gen/page.md"), "{err}");
    assert!(!err.contains("outside what it declares"), "{err}");

    // Outside it: the stray write is named, and the run fails as a broken
    // contract whatever the tool's own outcome said. `gen/page.md` is written
    // again with the same bytes, so it is no change and not listed.
    let out = engine(&mock, &["gen", "stray"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(
        err.contains("wrote `stray.md`, which is outside what it declares it writes"),
        "{err}"
    );
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_maker_declaring_nothing_is_refused_before_it_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path());
    let out = engine(&mock, &["bare"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("declares nothing it writes"), "{err}");
    assert!(
        !tmp.path().join("bare-ran.md").exists(),
        "refused at registration, so the tool never ran"
    );
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn help_for_a_maker_says_what_it_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path());
    let out = engine(&mock, &["help", "gen"]);
    let stdout = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        stdout.contains("purpose: make, writes `gen/**`"),
        "{stdout}"
    );
    assert!(
        !stdout.contains("SUBCOMMANDS"),
        "`mock help gen` describes `gen`, not every command:\n{stdout}"
    );

    let out = engine(&mock, &["tools"]);
    let stdout = text(&out.stdout);
    let line = stdout
        .lines()
        .find(|l| l.contains("mock gen"))
        .unwrap_or("");
    assert!(line.contains(" make "), "{stdout}");
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_maker_that_blocks_having_written_nothing_has_broken_its_contract() {
    // The loophole: `Make` over a path never written, plus blocking findings,
    // is a check with a failing case and no `NotALint` reason.
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path());
    let out = engine(&mock, &["dodge"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("never block"), "{err}");
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_false_no_failing_case_exits_as_a_broken_contract() {
    // Exit 2, the same as an undeclared write, rather than the 1 its finding
    // alone would give: the tool broke its contract, which is a different
    // statement from a corpus finding.
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path());
    let out = engine(&mock, &["liar"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("has a failing case"), "{err}");
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_maker_is_refused_where_its_writes_cannot_be_observed() {
    // Fail closed: a maker nobody can hold to its declaration does not run.
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture_without_git(tmp.path());
    let out = engine(&mock, &["gen"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(
        !tmp.path().join("gen/page.md").exists(),
        "the maker must not have run:\n{err}"
    );
    assert!(
        err.contains("Not run"),
        "refused for the observation, not for something else:\n{err}"
    );
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_maker_that_wrote_its_declared_file_and_blocks_has_broken_its_contract() {
    // A threshold check hiding as a maker: it writes the one path it declared
    // and blocks on something else. A maker's findings never block.
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path());
    let out = engine(&mock, &["gen", "block"]);
    let err = text(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("gen/page.md"), "it did write:\n{err}");
    assert!(err.contains("never block"), "{err}");
}
