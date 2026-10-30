//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The generated `forbidden-imports` hook gives the lint's verdict, through the real
//! engine and the real script, in a scratch repository.
//!
//! The case that matters is one each side of the line the old hook drew: an import the
//! lint refuses, and a comment naming the same path that the old substring test refused
//! and the lint passes. The control is the same text in a crate outside the rule's scope.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

#[path = "common/ambient.rs"]
mod ambient;
mod common;
use common::{Repo, write};

const CONFIG: &str = r#"project_name = "fixture"
crate_prefix = "fixture"

[lints.forbidden-imports]
commit = "error"
build = "error"
push = "error"
rules = [
    { scope = "fixture-sim", forbidden = "std::*", reason = "no_std" },
]
"#;

fn fixture() -> Repo {
    let repo = Repo::new();
    write(&repo.root.join("mock/mockspace.toml"), CONFIG);
    for krate in ["fixture-sim", "fixture-ui"] {
        let dir = repo.root.join("mock/crates").join(krate);
        write(&dir.join("Cargo.toml"), &format!("[package]\nname = \"{krate}\"\nversion = \"0.0.0\"\n"));
        write(&dir.join("src/lib.rs"), "pub fn f() {}\n");
    }
    repo.generate();
    repo
}

/// The hook the editor runs for a Write of `content` to `path`; its stdout.
fn hook_says(repo: &Repo, path: &Path, content: &str) -> String {
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": path.display().to_string(), "content": content },
    })
    .to_string();
    let mut child = repo
        .cmd("bash")
        .arg(repo.root.join(".claude/hooks/forbidden-imports.sh"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn refused(out: &str) -> bool {
    out.contains("permissionDecision")
}

#[test]
fn the_hook_refuses_what_the_lint_refuses_and_passes_what_it_passes() {
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim/src/lib.rs");
    let ui = repo.root.join("mock/crates/fixture-ui/src/lib.rs");

    let out = hook_says(&repo, &sim, "use std::fmt;\npub fn f() {}\n");
    assert!(refused(&out), "an import the lint refuses was let through: {out}");
    assert!(out.contains("forbidden-imports"), "the lint's own words should reach the agent: {out}");

    let out = hook_says(&repo, &sim, "// no std::fmt here\npub fn f() {}\n");
    assert!(!refused(&out), "text the lint passes was refused: {out}");

    let out = hook_says(&repo, &ui, "use std::fmt;\npub fn f() {}\n");
    assert!(!refused(&out), "control: a crate outside the rule's scope was refused: {out}");
}

#[test]
fn the_engine_command_exits_by_the_lints_verdict_and_names_a_missing_lint() {
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim/src/lib.rs");
    let ask = |lint: &str, extra: &[&str], stdin: &str| {
        let mut child = repo
            .cmd(common::BIN)
            .current_dir(repo.root.join("mock"))
            .args(["check-edit", "--lint", lint, "--file"])
            .arg(&sim)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    };

    assert_eq!(ask("forbidden-imports", &[], "use std::fmt;\n").status.code(), Some(1));
    assert_eq!(ask("forbidden-imports", &[], "pub fn g() {}\n").status.code(), Some(0));

    // An edit: what replaces the text, and what it replaces, laid over the file on disk.
    let old = repo.root.join("old.txt");
    fs::write(&old, "pub fn f() {}").unwrap();
    let old = old.to_str().unwrap().to_string();
    assert_eq!(
        ask("forbidden-imports", &["--replace", &old], "use std::fmt;").status.code(),
        Some(1)
    );
    assert_eq!(
        ask("forbidden-imports", &["--replace", &old], "pub fn g() {}").status.code(),
        Some(0)
    );
    fs::write(&old, "text the file does not hold").unwrap();
    assert_eq!(
        ask("forbidden-imports", &["--replace", &old], "use std::fmt;").status.code(),
        Some(0),
        "an edit naming text the file lacks is the tool's to refuse, not a verdict"
    );

    let gone = ask("no-such-lint", &[], "use std::fmt;\n");
    assert_eq!(gone.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&gone.stderr).contains("no-such-lint"));
}

#[test]
fn no_per_scope_hook_is_generated_any_more() {
    let repo = fixture();
    let hooks: Vec<String> = fs::read_dir(repo.root.join(".claude/hooks"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .collect();
    assert!(hooks.iter().any(|h| h == "forbidden-imports.sh"), "{hooks:?}");
    assert!(!hooks.iter().any(|h| h.starts_with("lint-forbidden-")), "{hooks:?}");
}

/// The hook for an Edit, through the real engine.
fn hook_edit_says(repo: &Repo, path: &Path, old: &str, new: &str) -> String {
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {
            "file_path": path.display().to_string(),
            "old_string": old,
            "new_string": new,
        },
    })
    .to_string();
    let mut child = repo
        .cmd("bash")
        .arg(repo.root.join(".claude/hooks/forbidden-imports.sh"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).to_string()
}

#[test]
fn an_edit_is_judged_on_what_it_adds_through_the_real_engine() {
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim/src/lib.rs");
    fs::write(&sim, "use std::fmt;\nuse std::io;\npub fn f() {}\n\n\n").unwrap();

    // Mending one of two violations, and an edit beside both, add nothing.
    let out = hook_edit_says(&repo, &sim, "use std::io;\n", "");
    assert!(!refused(&out), "removing a violation was refused: {out}");
    let out = hook_edit_says(&repo, &sim, "pub fn f", "pub fn g");
    assert!(!refused(&out), "an unrelated edit was refused for what the file already had: {out}");

    // The controls: an edit that adds one is refused, including one whose old text is only
    // newlines and whose new text ends in one, which a shell variable would have mangled.
    let out = hook_edit_says(&repo, &sim, "pub fn f() {}", "pub fn f() {}\nuse std::fs;");
    assert!(refused(&out), "an added violation was let through: {out}");
    let out = hook_edit_says(&repo, &sim, "\n\n\n", "\nuse std::fs;\n");
    assert!(refused(&out), "an added violation behind newline-only old text was let through: {out}");
}

#[test]
fn a_regenerated_consumer_carries_the_short_catalogue() {
    let repo = fixture();
    let rule = fs::read_to_string(repo.root.join(".claude/rules/catalogues.md")).unwrap();
    assert!(rule.contains("`mock tools`") && rule.contains("`mock lints`"), "{rule}");
    assert!(rule.contains("forbidden-imports"), "a lint the repository set is a row: {rule}");
    // What every repository shares is not carried.
    assert!(!rule.contains("registry-append"), "a builtin subcommand was listed: {rule}");
    assert!(!rule.contains("BUILTIN SUBCOMMANDS"), "{rule}");
    assert!(rule.len() < 2_500, "{} bytes", rule.len());
}
