//! End-to-end validation of the `tools/` path: a real crate under
//! `<mock>/tools/<name>/`, discovered by directory, compiled into the same
//! cdylib the custom lints use, dlopened, and dispatched as `Box<dyn Tool>`.
//!
//! Everything else about tools is unit-tested against hand-built values. This
//! is the only test that walks the path a consumer actually walks, and without
//! it the rest is a set of checks over a mechanism nobody has run.
//!
//! `#[ignore]` because it runs `cargo build`, matching `custom_lint_cdylib.rs`.
//! `tests/rust_e2e_test.sh` runs this with `--ignored` every time `./test`
//! does, so it stays reachable by the command this repository's own workflow
//! already expects a human to run, rather than by a flag nothing invokes.

use std::fs;
use std::path::Path;

/// The `mockspace-lint-rules` in this workspace, renamed to `mockspace` so a
/// tool crate's source reads exactly as a consumer's would.
fn dep_spec() -> String {
    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    format!("{{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}")
}

/// Write a tool crate at `<tools>/<dir>` whose package is `pkg` and whose
/// `Tool::name()` returns `declares`.
///
/// `declares` is separate from `dir` on purpose: the mismatch is one of the
/// cases under test, and a helper that could not express it would quietly make
/// that case untestable.
fn write_tool(tools: &Path, dir: &str, pkg: &str, declares: &str, body: &str) {
    let root = tools.join(dir);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{pkg}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             publish = false\n\n[dependencies]\nmockspace = {}\n",
            dep_spec()
        ),
    )
    .unwrap();
    fs::write(
        root.join("src").join("lib.rs"),
        format!(
            r#"use mockspace::tool::{{ArgSpec, NotALint, Outcome, Tool, ToolContext, ToolReport}};

pub struct T;

impl Tool for T {{
    fn name(&self) -> &'static str {{ "{declares}" }}
    fn description(&self) -> &'static str {{ "a probe tool" }}
    {body}
}}

mockspace::lint_pack! {{
    tools: [T],
}}
"#
        ),
    )
    .unwrap();
}

fn project(tmp: &Path) -> std::path::PathBuf {
    let mock = tmp.join("mock");
    fs::create_dir_all(mock.join("tools")).unwrap();
    fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    mock
}

fn load(mock: &Path) -> mockspace::custom_lints::LoadedLints {
    let cfg = mockspace::config::Config::from_dir(mock);
    mockspace::custom_lints::load(&cfg, &mock.join("mockspace.toml"), &dep_spec())
        .expect("the cdylib must build")
        .expect("a project with a tool has something to load")
}

const REPORTS: &str = r#"fn not_a_lint(&self) -> NotALint { NotALint::NoFailingCase }
    fn run(&self, ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported(format!("args={}", ctx.args.len()), 7)
    }"#;

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_tool_crate_loads_and_dispatches_across_the_cdylib() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = project(tmp.path());
    write_tool(&mock.join("tools"), "greet", "probe-greet", "greet", REPORTS);

    let loaded = load(&mock);

    // It arrives as a tool, and only as a tool: a tool crate must not have
    // quietly registered itself on one of the lint vectors.
    assert_eq!(loaded.pack.tools.len(), 1, "one tool expected");
    assert!(loaded.pack.crate_lints.is_empty());
    assert!(loaded.pack.workspace_lints.is_empty());
    assert!(loaded.pack.repo_lints.is_empty());
    assert!(loaded.pack.message_lints.is_empty());

    // Dispatch across the boundary: a `&'static str` and a vtable call from
    // the other side of a dlopen.
    let tool = &loaded.pack.tools[0];
    assert_eq!(tool.name(), "greet");
    assert_eq!(tool.description(), "a probe tool");
    assert_eq!(tool.not_a_lint(), mockspace::tool::NotALint::NoFailingCase);

    // And it runs, with the context the engine builds.
    let crates = std::collections::BTreeSet::new();
    let dirs: Vec<std::path::PathBuf> = Vec::new();
    let args = ["one", "two"];
    let ctx = mockspace::tool::ToolContext {
        mock_dir:   &mock,
        repo_root:  tmp.path(),
        all_crates: &crates,
        src_dirs:   &dirs,
        args:       &args,
        stdin:      None,
    };
    let report = tool.run(&ctx);
    assert_eq!(report.output, "args=2", "the argument vector must reach the tool");
    match report.outcome {
        mockspace::tool::Outcome::Clean {
            examined,
        } => assert_eq!(examined, 7),
        other => panic!("expected Clean, got {other:?}"),
    }
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_tools_package_name_may_differ_from_its_directory() {
    // The case that fails if discovery infers the package from the directory:
    // the generated manifest would name `greet` and cargo would not resolve it,
    // so the build fails rather than the assertion.
    let tmp = tempfile::tempdir().unwrap();
    let mock = project(tmp.path());
    write_tool(
        &mock.join("tools"),
        "phrase-search",
        "probe-phrase-search-pkg",
        "phrase-search",
        REPORTS,
    );

    let loaded = load(&mock);
    assert_eq!(loaded.pack.tools.len(), 1);
    assert_eq!(loaded.pack.tools[0].name(), "phrase-search");
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_tool_declaring_a_name_that_is_not_its_directory_is_detectable() {
    // The engine refuses this rather than picking a winner, because a tool
    // reachable under a name its own source does not contain is a tool nobody
    // can grep for. Here we pin that the two names genuinely differ after a
    // real load, which is what the dispatcher's refusal keys on.
    let tmp = tempfile::tempdir().unwrap();
    let mock = project(tmp.path());
    write_tool(&mock.join("tools"), "outside", "probe-outside", "inside", REPORTS);

    let loaded = load(&mock);
    assert_eq!(loaded.pack.tools.len(), 1);
    assert_eq!(
        loaded.pack.tools[0].name(),
        "inside",
        "the declared name is what the pack carries"
    );
    assert!(
        !loaded.pack.tools.iter().any(|t| t.name() == "outside"),
        "nothing answers to the directory name, which is what `mock outside` \
         would look for"
    );
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_repo_whose_only_custom_content_is_a_tool_still_builds_a_cdylib() {
    // The early return in `load` short-circuits on "no lints and no packs". If
    // tools were not counted there, this returns `Ok(None)` and every tool in
    // the project is silently unreachable.
    let tmp = tempfile::tempdir().unwrap();
    let mock = project(tmp.path());
    assert!(!mock.join("lints").exists(), "no lints, deliberately");
    write_tool(&mock.join("tools"), "only", "probe-only", "only", REPORTS);

    let cfg = mockspace::config::Config::from_dir(&mock);
    let loaded = mockspace::custom_lints::load(&cfg, &mock.join("mockspace.toml"), &dep_spec())
        .expect("the cdylib must build");
    assert!(loaded.is_some(), "a tool alone is something to load");
    assert_eq!(loaded.unwrap().pack.tools.len(), 1);
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn the_stdin_opt_in_survives_the_cdylib_boundary() {
    // `wants_stdin` is what stops `mock <tool>` blocking on a pipe that never
    // closes, and it is read across a dlopen. A default that did not cross
    // correctly would reintroduce the hang for every dynamically loaded tool
    // while every unit test still passed.
    let tmp = tempfile::tempdir().unwrap();
    let mock = project(tmp.path());
    write_tool(&mock.join("tools"), "quiet", "probe-quiet", "quiet", REPORTS);
    let loaded = load(&mock);
    assert!(
        !loaded.pack.tools[0].wants_stdin(),
        "a tool that did not ask must not be handed stdin"
    );

    let tmp2 = tempfile::tempdir().unwrap();
    let mock2 = project(tmp2.path());
    write_tool(
        &mock2.join("tools"),
        "hungry",
        "probe-hungry",
        "hungry",
        &format!("{REPORTS}\n    fn wants_stdin(&self) -> bool {{ true }}"),
    );
    let loaded2 = load(&mock2);
    assert!(
        loaded2.pack.tools[0].wants_stdin(),
        "a tool that opted in must be able to say so through the boundary"
    );
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_tool_and_a_lint_can_ship_in_one_crate() {
    // Why there is no separate tool entry point: a tool crate may reasonably
    // carry a lint beside its tool, and two collector symbols would make that
    // two registrations to keep in step. One `collect(pack)` covers both.
    let tmp = tempfile::tempdir().unwrap();
    let mock = project(tmp.path());
    let root = mock.join("tools").join("both");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-both\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             publish = false\n\n[dependencies]\nmockspace = {}\n",
            dep_spec()
        ),
    )
    .unwrap();
    fs::write(
        root.join("src").join("lib.rs"),
        r#"use mockspace::tool::{NotALint, Tool, ToolContext, ToolReport};
use mockspace::{CrateLint, Lint, LintContext, LintError};

pub struct T;
impl Tool for T {
    fn name(&self) -> &'static str { "both" }
    fn description(&self) -> &'static str { "a tool" }
    fn not_a_lint(&self) -> NotALint { NotALint::NoFailingCase }
    fn run(&self, _c: &ToolContext<'_>) -> ToolReport { ToolReport::reported("", 1) }
}

pub struct L;
impl Lint for L {
    fn name(&self) -> &'static str { "probe-both-lint" }
}
impl CrateLint for L {
    fn check(&self, _ctx: &LintContext) -> Vec<LintError> { Vec::new() }
}

mockspace::lint_pack! {
    lints: [L],
    tools: [T],
}
"#,
    )
    .unwrap();

    let loaded = load(&mock);
    assert_eq!(loaded.pack.tools.len(), 1, "the tool registered");
    assert_eq!(loaded.pack.crate_lints.len(), 1, "and so did the lint");
    assert_eq!(loaded.pack.tools[0].name(), "both");
    assert_eq!(loaded.pack.crate_lints[0].name(), "probe-both-lint");
}
