//! The engine finds its cdylib wherever cargo put it, including a shared
//! `CARGO_TARGET_DIR`.
//!
//! The engine shells out to cargo and needs the artifact back. Cargo honours
//! `CARGO_TARGET_DIR`, which is set on any machine sharing one build dir across
//! worktrees, so the artifact does not have to be under the generated crate.
//!
//! Searching a fixed directory for it fails two ways. Loudly: nothing is there
//! and the build "succeeds" with no library. Quietly, and this is the one that
//! cost two rounds chasing a phantom loader bug: a previous run left a file in
//! the expected place, that stale copy loads, and the engine answers from an old
//! build of the project's lints and tools with no warning. reads exactly like an
//! edit not taking effect.
//!
//! Pinning `--target-dir` fixes both and forecloses the shared cache, so every
//! fresh clone, worktree and fixture pays a cold release build of the whole dep
//! graph. That is what saturated a laptop. So the path comes out of cargo's own
//! json instead, which fixes both AND keeps the cache.
//!
//! NOTE: the second arm is what tells the two fixes apart. under `--target-dir`
//! the tool still runs and the artifact is under the generated crate, so arm one
//! passes and arm two fails.

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

    // arm two: the shared dir was actually used. this is the discriminating
    // one. a fix that pins --target-dir passes the arm above and fails here.
    assert!(
        cdylibs_under(&elsewhere) > 0,
        "the artifact must land in the shared target dir, else nothing is \
         shared and every tree pays the build cold: {}",
        elsewhere.display()
    );
    assert_eq!(
        cdylibs_under(&mock.join("target").join("mockspace-lints")),
        0,
        "and nothing should have been built under the generated crate"
    );
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn without_the_variable_the_artifact_lands_under_the_generated_crate() {
    // the control. without it, arm two above is equally consistent with an
    // engine that writes to `elsewhere` no matter what the environment says.
    let tmp = tempfile::tempdir().unwrap();
    let mock = fixture(tmp.path());

    let dep = dep_spec();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mockspace"))
        .args(["--mockspace-lint-rules-dep", &dep, "greet"])
        .env_remove("CARGO_TARGET_DIR")
        .current_dir(&mock)
        .output()
        .expect("the binary runs");
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(all.contains("the tool that actually ran"), "{all}");
    assert!(
        cdylibs_under(&mock.join("target").join("mockspace-lints")) > 0,
        "with no variable set, cargo's default puts it under the generated crate"
    );
}

/// How many loadable libraries sit anywhere under `root`.
fn cdylibs_under(root: &Path) -> usize {
    fn walk(dir: &Path, n: &mut usize) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, n);
            } else if p
                .extension()
                .is_some_and(|x| x == "dylib" || x == "so" || x == "dll")
                && p.file_name()
                    .is_some_and(|f| f.to_string_lossy().contains("mockspace_lints_"))
            {
                *n += 1;
            }
        }
    }
    let mut n = 0;
    walk(root, &mut n);
    n
}
