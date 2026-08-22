//! The three reasons `mock <tool>` finds no tool, each reaching the real code.
//!
//! The unit tests beside `why_not_found` pin its truth table, and a truth table
//! is not the thing that shipped. Delete the whole branch that calls it in
//! `entry::tool::run` and those tests still pass: the message, the exit code,
//! and the fact that the classifier is consulted at all go untested. A reviewer
//! found exactly that, and this file is the answer.
//!
//! The cheap arm is the important one, because it is the case a reader actually
//! hits: invoking the engine binary rather than the launcher. It needs no build.
//! The two arms that need a real cdylib carry `#[ignore]`, matching
//! `tool_cdylib.rs` and `custom_lint_cdylib.rs`; `tests/rust_e2e_test.sh` runs
//! the ignored set.

use std::fs;
use std::path::Path;
use std::process::Output;

/// A repository with a tool directory and nothing built from it.
///
/// The tool directory gets a `Cargo.toml`. Discovery is a directory listing
/// rather than a build, but a subdirectory without a manifest is not a tool at
/// all and is skipped with its own warning, so a fixture missing it tests the
/// unrecognised-subcommand path instead of this one.
fn fixture(root: &Path, tool: &str) -> std::path::PathBuf {
    fs::create_dir_all(root.join(".git")).unwrap();
    let mock = root.join("mock");
    let dir = mock.join("tools").join(tool);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-{tool}\"\nversion = \"0.0.0\"\n\
             edition = \"2024\"\npublish = false\n"
        ),
    )
    .unwrap();
    fs::write(dir.join("src").join("lib.rs"), "").unwrap();
    fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    fs::write(
        mock.join("Cargo.toml"),
        "[workspace]\nmembers = []\nresolver = \"2\"\n",
    )
    .unwrap();
    mock
}

fn engine(mock: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(args)
        // Cleared, not inherited. The engine builds the tool cdylib by shelling
        // out to cargo and then looks for the artifact under the fixture's own
        // `target/`. With `CARGO_TARGET_DIR` set in the caller's environment,
        // which is the ordinary state on a machine sharing one build directory
        // across worktrees, cargo reports success and the artifact lands
        // somewhere else, so the engine blocks with "cargo reported success but
        // no lint cdylib was found". That is an artefact of the harness rather
        // than a fact about the code, and it made two arms of this file red for
        // a reason unrelated to what they assert.
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(mock)
        .output()
        .expect("the binary runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// No `--mockspace-lint-rules-dep`, so no cdylib was ever asked for.
///
/// This is the case the whole change is about: it cost two debugging sessions
/// looking for a name mismatch in a repository where nothing had been compiled.
#[test]
fn an_engine_invoked_without_the_dep_flag_says_nothing_was_built() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path(), "greet");
    let out = engine(&mock, &["greet"]);
    let err = stderr(&out);

    assert_eq!(
        out.status.code(),
        Some(2),
        "a tool that cannot be run is an error, not a silent success:\n{err}"
    );
    assert!(
        err.contains("nothing was built from it"),
        "the reader is sent to the launcher, which is the thing that asks for \
         the cdylib:\n{err}"
    );
    assert!(
        err.contains("cargo mock greet"),
        "and told the command to run, by name:\n{err}"
    );
    assert!(
        !err.contains("declares"),
        "the name-mismatch wording must not appear here: it sends the reader \
         hunting a mismatch that does not exist, which is the defect:\n{err}"
    );
}

/// The control. Without it, the arm above is equally consistent with an engine
/// that prints that message for every unrecognised word on the command line.
#[test]
fn a_word_that_is_not_a_tool_directory_is_not_reported_as_a_tool() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path(), "greet");
    let err = stderr(&engine(&mock, &["nosuchthing"]));
    assert!(
        !err.contains("nothing was built from it"),
        "only a name with a directory under `tools/` is a tool:\n{err}"
    );
}

// --- The two arms that need a real cdylib ------------------------------------

fn dep_spec() -> String {
    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    format!("{{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}")
}

/// Write a tool crate whose `lint_pack!` registers `tools`, verbatim.
fn write_crate(tools: &Path, dir: &str, registers: &str, item: &str) {
    let root = tools.join(dir);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-{dir}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             publish = false\n\n[dependencies]\nmockspace = {}\n",
            dep_spec()
        ),
    )
    .unwrap();
    fs::write(
        root.join("src").join("lib.rs"),
        format!("{item}\n\nmockspace::lint_pack! {{\n    tools: [{registers}],\n}}\n"),
    )
    .unwrap();
}

const A_TOOL: &str = r#"use mockspace::tool::{NotALint, Tool, ToolContext, ToolReport};
pub struct T;
impl Tool for T {
    fn name(&self) -> &'static str { "somethingelse" }
    fn description(&self) -> &'static str { "a probe tool" }
    fn not_a_lint(&self) -> NotALint { NotALint::NoFailingCase }
    fn run(&self, _: &ToolContext<'_>) -> ToolReport { ToolReport::reported("", 0) }
}"#;

/// The case the first version could not distinguish at all: the crate compiled
/// and registered nothing, so the pack is empty **under the launcher**. The old
/// classifier read that empty pack as "nothing was built" and told the reader to
/// use the launcher they were already using.
#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_pack_that_built_and_registered_nothing_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path(), "greet");
    write_crate(&mock.join("tools"), "greet", "", "");
    let dep = dep_spec();
    let err = stderr(&engine(&mock, &["--mockspace-lint-rules-dep", &dep, "greet"]));

    assert!(
        err.contains("registered no tool at all"),
        "the crate compiled, so this is not a build that did not happen:\n{err}"
    );
    assert!(
        !err.contains("nothing was built from it"),
        "and it must not be reported as one, which is the whole defect:\n{err}"
    );
    assert!(
        err.contains("lint_pack!"),
        "the remedy names the macro the crate is missing:\n{err}"
    );
}

/// Something registered and it was not this name, so a mismatch is the likely
/// cause and the message says so.
#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_pack_registering_another_name_says_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path(), "greet");
    write_crate(&mock.join("tools"), "greet", "T", A_TOOL);
    let dep = dep_spec();
    let err = stderr(&engine(&mock, &["--mockspace-lint-rules-dep", &dep, "greet"]));

    assert!(
        err.contains("no tool in it declares"),
        "with something registered, the mismatch wording is the right one:\n{err}"
    );
    assert!(
        err.contains("somethingelse"),
        "and it names what did register, so the reader can see the mismatch:\n{err}"
    );
}
