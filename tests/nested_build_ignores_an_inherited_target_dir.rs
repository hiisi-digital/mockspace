//! A project's cdylib builds where the engine looks, whatever `CARGO_TARGET_DIR` says.
//!
//! The engine builds `<mock>/target/mockspace-lints` by shelling out to cargo
//! and then searches `<that>/target/release` for the artifact. Cargo honours
//! `CARGO_TARGET_DIR` from the environment, which is set on any machine sharing
//! one build directory across worktrees, so inherited it puts the artifact
//! somewhere else.
//!
//! The loud failure is "cargo reported success but no lint cdylib was found".
//! The quiet one is worse and is why this test exists: where a previous run left
//! an artifact in the expected place, that stale copy is loaded and the engine
//! answers from an old build of the project's lints and tools with no warning.
//! It reads exactly like an edit not taking effect, and it cost two rounds of
//! chasing a phantom loader bug before anybody suspected the environment.

use std::fs;
use std::path::Path;

fn dep_spec() -> String {
    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    format!("{{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}")
}

fn fixture(root: &Path) -> std::path::PathBuf {
    let mock = root.join("mock");
    let dir = mock.join("tools").join("greet");
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-greet\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
             publish = false\n\n[dependencies]\nmockspace = {}\n",
            dep_spec()
        ),
    )
    .unwrap();
    fs::write(
        dir.join("src").join("lib.rs"),
        r#"use mockspace::tool::{NotALint, Tool, ToolContext, ToolReport};
pub struct T;
impl Tool for T {
    fn name(&self) -> &'static str { "greet" }
    fn description(&self) -> &'static str { "a probe tool" }
    fn not_a_lint(&self) -> NotALint { NotALint::NoFailingCase }
    fn run(&self, _: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("the tool that actually ran", 1)
    }
}

mockspace::lint_pack! {
    tools: [T],
}
"#,
    )
    .unwrap();
    mock
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn a_tool_runs_with_cargo_target_dir_set_in_the_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path());
    let elsewhere = tmp.path().join("somewhere-else");

    let dep = dep_spec();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(["--mockspace-lint-rules-dep", &dep, "greet"])
        .env("CARGO_TARGET_DIR", &elsewhere)
        .current_dir(&mock)
        .output()
        .expect("the binary runs");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(
        all.contains("the tool that actually ran"),
        "the tool must run with a shared target directory set, which is the \
         ordinary state on a machine with worktrees:\n{all}"
    );
    assert!(
        !all.contains("no lint cdylib was found"),
        "and it must not be the loud half of the failure either:\n{all}"
    );
}
