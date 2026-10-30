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
    // A gate the engine does not know is refused, not read as the commit gate.
    assert_eq!(ask("forbidden-imports", &["--gate", "nope"], "pub fn g() {}\n").status.code(), Some(2));
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
fn run_hook(repo: &Repo, payload: serde_json::Value) -> String {
    run_hook_at(repo, ".claude/hooks/forbidden-imports.sh", payload)
}

fn run_hook_at(repo: &Repo, hook: &str, payload: serde_json::Value) -> String {
    let mut child = repo
        .cmd("bash")
        .arg(repo.root.join(hook))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).to_string()
}

fn hook_edit_says(repo: &Repo, path: &Path, old: &str, new: &str) -> String {
    run_hook(repo, serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": {
            "file_path": path.display().to_string(),
            "old_string": old,
            "new_string": new,
        },
    }))
}

fn hook_multiedit_says(repo: &Repo, path: &Path, edits: &[(&str, &str)]) -> String {
    let edits: Vec<_> = edits
        .iter()
        .map(|(old, new)| serde_json::json!({ "old_string": old, "new_string": new }))
        .collect();
    run_hook(repo, serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "MultiEdit",
        "tool_input": { "file_path": path.display().to_string(), "edits": edits },
    }))
}

#[test]
fn a_multi_edit_is_judged_as_the_file_the_whole_sequence_leaves() {
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim/src/lib.rs");
    fs::write(&sim, "pub fn f() {}\npub fn g() {}\n").unwrap();
    // The second edit lays over what the first left, and the result holds a violation.
    let out = hook_multiedit_says(&repo, &sim, &[("pub fn f", "pub fn h"), ("pub fn g() {}", "use std::fmt;")]);
    assert!(refused(&out), "a multi-edit adding a violation was let through: {out}");
    // The control: a sequence that leaves the file clean is passed.
    let out = hook_multiedit_says(&repo, &sim, &[("pub fn f", "pub fn h"), ("pub fn g", "pub fn i")]);
    assert!(!refused(&out), "a clean multi-edit was refused: {out}");
    // An edit the tool would refuse (text the file lacks) is the tool's to refuse.
    let out = hook_multiedit_says(&repo, &sim, &[("not in the file", "use std::fmt;")]);
    assert!(!refused(&out), "{out}");
}


#[test]
fn an_edit_is_judged_as_the_file_it_leaves_through_the_real_engine() {
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim/src/lib.rs");
    fs::write(&sim, "use std::fmt;\nuse std::io;\npub fn f() {}\n\n\n").unwrap();

    // The lint's verdict on the file as the edit leaves it: one violation remains.
    let out = hook_edit_says(&repo, &sim, "use std::io;\n", "");
    assert!(refused(&out), "a file still holding a violation was passed: {out}");
    // The control: an edit that leaves none is passed.
    let out = hook_edit_says(&repo, &sim, "use std::fmt;\nuse std::io;\n", "");
    assert!(!refused(&out), "a clean result was refused: {out}");
    // Old text that is only newlines, with new text ending in one, reaches the engine whole,
    // which a shell variable would have mangled.
    fs::write(&sim, "pub fn f() {}\n\n\n").unwrap();
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

/// The `preToolUse` entry the render wired for the generated hook, as Copilot reads
/// `.github/hooks/hooks.json` (docs.github.com/en/copilot/reference/hooks-reference).
fn copilot_entry(repo: &Repo) -> serde_json::Value {
    let doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(repo.root.join(".github/hooks/hooks.json")).unwrap()).unwrap();
    assert_eq!(doc["version"], 1, "{doc}");
    doc["hooks"]["preToolUse"]
        .as_array()
        .unwrap_or_else(|| panic!("no hooks.preToolUse list: {doc}"))
        .iter()
        .find(|e| e["bash"] == ".github/hooks/forbidden-imports.sh")
        .unwrap_or_else(|| panic!("the hook is not wired: {doc}"))
        .clone()
}

/// Whether Copilot would fire an entry for `tool`: its `matcher` is a regex that has to
/// match the whole runtime tool name, and the generated ones are plain alternations.
fn fires_for(entry: &serde_json::Value, tool: &str) -> bool {
    match entry["matcher"].as_str() {
        None => true,
        Some(m) => m.split('|').any(|alt| alt == tool),
    }
}

/// What Copilot does with a call: finds the entry, fires it only if the entry's matcher
/// names the tool, runs the entry's `bash` command with the call on stdin, and reads
/// the exit status and stdout. Copilot carries a tool's arguments as a JSON-encoded
/// string, so the path and the text reach the hook escaped inside it.
fn copilot_says(repo: &Repo, tool: &str, args: serde_json::Value) -> (Option<i32>, String) {
    let entry = copilot_entry(repo);
    assert!(fires_for(&entry, tool), "Copilot would not call the hook for `{tool}`: {entry}");
    let payload = serde_json::json!({ "toolName": tool, "toolArgs": args.to_string() });
    let mut child = repo
        .cmd("bash")
        .arg(repo.root.join(entry["bash"].as_str().unwrap()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code(), String::from_utf8_lossy(&out.stdout).to_string())
}

/// A pass: Copilot's `allow` prints nothing, so only the exit status tells it from a
/// crash, which Copilot treats as a deny.
fn passed(run: &(Option<i32>, String)) -> bool {
    run.0 == Some(0) && !refused(&run.1)
}

#[test]
fn the_copilot_hook_is_wired_for_the_tools_copilot_calls_its_file_edits() {
    let repo = fixture();
    let entry = copilot_entry(&repo);
    assert_eq!(entry["type"], "command", "{entry}");
    assert!(entry["timeoutSec"].as_u64().is_some_and(|t| t > 0), "a timeout is registered: {entry}");
    assert!(fires_for(&entry, "create") && fires_for(&entry, "edit"), "{entry}");
    // The controls: Claude's names are not Copilot's, and a shell call is not a file edit.
    assert!(!fires_for(&entry, "Write") && !fires_for(&entry, "bash"), "{entry}");
}

#[test]
fn the_copilot_hook_gives_the_lints_verdict_on_the_calls_its_wiring_reaches() {
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim/src/lib.rs");
    let path = sim.display().to_string();
    fs::write(&sim, "pub fn f() {}\npub fn g() {}\n").unwrap();
    // Copilot's own argument names.
    let out = copilot_says(&repo, "edit", serde_json::json!({
        "path": path, "old_str": "pub fn g() {}", "new_str": "use std::fmt;"
    }));
    assert!(refused(&out.1), "the Copilot hook passed what the lint refuses: {out:?}");
    let out = copilot_says(&repo, "create", serde_json::json!({
        "path": repo.root.join("mock/crates/fixture-sim/src/fresh.rs").display().to_string(),
        "file_text": "use std::fmt;\n"
    }));
    assert!(refused(&out.1), "a created file holding a violation was passed: {out:?}");
    // The control: the Claude spellings are read too.
    let out = copilot_says(&repo, "edit", serde_json::json!({
        "file_path": path, "old_string": "pub fn g() {}", "new_string": "use std::fmt;"
    }));
    assert!(refused(&out.1), "{out:?}");
    // The controls: a clean edit, and a file that is not Rust, are passed, by status.
    let out = copilot_says(&repo, "edit", serde_json::json!({
        "path": path, "old_str": "pub fn g", "new_str": "pub fn h"
    }));
    assert!(passed(&out), "{out:?}");
    let out = copilot_says(&repo, "create", serde_json::json!({
        "path": repo.root.join("notes.md").display().to_string(), "file_text": "use std::fmt;"
    }));
    assert!(passed(&out), "{out:?}");
}

#[test]
fn an_object_valued_tool_args_is_read_as_well_as_a_string() {
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim/src/lib.rs");
    let args = serde_json::json!({
        "path": sim.display().to_string(), "old_str": "pub fn f() {}", "new_str": "use std::fmt;"
    });
    let run = |tool_args: serde_json::Value| {
        let payload = serde_json::json!({ "toolName": "edit", "toolArgs": tool_args });
        let out = run_hook_at(&repo, ".github/hooks/forbidden-imports.sh", payload);
        refused(&out)
    };
    assert!(run(serde_json::Value::String(args.to_string())), "the string form");
    assert!(run(args), "an object toolArgs was allowed unread");
}

#[test]
fn a_multi_edit_creating_a_file_is_judged_on_its_text() {
    let repo = fixture();
    let fresh = repo.root.join("mock/crates/fixture-sim/src/fresh.rs");
    let out = hook_multiedit_says(&repo, &fresh, &[("", "use std::fmt;\n")]);
    assert!(refused(&out), "a created file holding a violation was passed: {out}");
    let out = hook_multiedit_says(&repo, &fresh, &[("", "pub fn k() {}\n")]);
    assert!(!refused(&out), "{out}");
}

#[test]
fn the_lint_only_flag_changes_no_verdict_and_leaves_the_repository_alone() {
    // The hook passes it, so a check does not run the gate's setup (hook writes, driver and
    // `core.hooksPath` registration) once per edit. The verdict has to be the same without it.
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim/src/lib.rs");
    let ask = |flag: &[&str], stdin: &str| {
        let mut child = repo
            .cmd(common::BIN)
            .current_dir(repo.root.join("mock"))
            .args(["check-edit", "--lint", "forbidden-imports", "--file"])
            .arg(&sim)
            .args(flag)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    };
    for (text, want) in [("use std::fmt;\n", 1), ("pub fn g() {}\n", 0)] {
        let plain = ask(&[], text);
        let lint_only = ask(&["--lint-only"], text);
        assert_eq!(plain.status.code(), Some(want));
        assert_eq!(lint_only.status.code(), Some(want), "the flag changed a verdict");
        assert_eq!(plain.stderr, lint_only.stderr, "the findings differ");
    }
    // The control that the flag does something: a clone with the setup undone gets it back
    // from a plain run and not from a flagged one.
    let hooks_path = |r: &Repo| {
        let out = r.cmd("git").args(["config", "--local", "--get", "core.hooksPath"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert!(!hooks_path(&repo).is_empty(), "the premise: generation set the hooks path");
    assert!(repo.cmd("git").args(["config", "--local", "--unset", "core.hooksPath"]).status().unwrap().success());
    ask(&["--lint-only"], "pub fn g() {}\n");
    assert!(hooks_path(&repo).is_empty(), "a flagged check ran the gate's setup");
    ask(&[], "pub fn g() {}\n");
    assert!(!hooks_path(&repo).is_empty(), "the control: a plain run restores it");
}

#[test]
fn a_source_reached_through_a_symlinked_directory_is_judged_where_it_really_is() {
    let repo = fixture();
    let alias = repo.root.join("alias");
    std::os::unix::fs::symlink(repo.root.join("mock/crates"), &alias).unwrap();
    let through = alias.join("fixture-sim/src/lib.rs");
    let out = hook_says(&repo, &through, "use std::fmt;\npub fn f() {}\n");
    assert!(refused(&out), "a violation written through a symlinked directory was passed: {out}");
    // A file created beyond the link, in a directory that does not exist yet.
    let fresh = alias.join("fixture-sim/src/new/mod.rs");
    let out = hook_says(&repo, &fresh, "use std::fmt;\n");
    assert!(refused(&out), "a new file beyond the link was passed: {out}");
    // The controls: the same text through the link in a crate outside the rule, and clean text.
    let out = hook_says(&repo, &alias.join("fixture-ui/src/lib.rs"), "use std::fmt;\n");
    assert!(!refused(&out), "{out}");
    let out = hook_says(&repo, &through, "pub fn g() {}\n");
    assert!(!refused(&out), "{out}");
}

#[test]
fn a_copilot_insert_is_judged_as_the_text_it_inserts() {
    // An insert names no text it replaces, so the hook judges what is inserted alone. That is
    // the lint's verdict on a line a per-file import lint reads line by line; a lint that needs
    // the line's neighbours would be judged on less than the file it leaves.
    let repo = fixture();
    let path = repo.root.join("mock/crates/fixture-sim/src/lib.rs").display().to_string();
    let out = copilot_says(&repo, "edit", serde_json::json!({
        "path": path, "insert_line": 1, "new_str": "use std::fmt;"
    }));
    assert!(refused(&out.1), "{out:?}");
    let out = copilot_says(&repo, "edit", serde_json::json!({
        "path": path, "insert_line": 1, "new_str": "pub fn k() {}"
    }));
    assert!(passed(&out), "{out:?}");
}

#[test]
#[ignore = "catalogue: Copilot's apply_patch carries a patch text and no path, and the wiring does not fire the hook for it, so a patch adding a forbidden import is let through"]
fn a_copilot_apply_patch_adding_a_violation_is_refused() {
    let repo = fixture();
    let path = repo.root.join("mock/crates/fixture-sim/src/lib.rs").display().to_string();
    let patch = format!(
        "*** Begin Patch\n*** Update File: {path}\n@@\n-pub fn f() {{}}\n+use std::fmt;\n*** End Patch\n"
    );
    let out = copilot_says(&repo, "apply_patch", serde_json::Value::String(patch));
    assert!(refused(&out.1), "a patch adding a violation was let through: {out:?}");
}

#[test]
fn a_link_inside_a_crate_is_judged_where_the_gate_reads_it_by_any_spelling() {
    // The gate follows a linked `src` or module directory and lints what is in it at the
    // place it sits in the crate, so an edit is judged there however the path reaches it.
    let repo = fixture();
    let sim = repo.root.join("mock/crates/fixture-sim");
    let violation = "use std::fmt;\npub fn f() {}\n";

    // `src` is a link, and the file is named in the crate and through an alias to the link.
    let elsewhere = repo.root.join("elsewhere");
    write(&elsewhere.join("src/lib.rs"), "pub fn f() {}\n");
    fs::remove_dir_all(sim.join("src")).unwrap();
    std::os::unix::fs::symlink(elsewhere.join("src"), sim.join("src")).unwrap();
    assert!(refused(&hook_says(&repo, &sim.join("src/lib.rs"), violation)), "in the crate");
    let alias = repo.root.join("alias");
    std::os::unix::fs::symlink(sim.join("src"), &alias).unwrap();
    assert!(refused(&hook_says(&repo, &alias.join("lib.rs"), violation)), "through an alias to the linked src");

    // A module directory is a link, in the in-scope crate, and the same two spellings.
    let shared = repo.root.join("sharedgen");
    write(&shared.join("mod.rs"), "pub fn g() {}\n");
    std::os::unix::fs::symlink(&shared, sim.join("src/gen")).unwrap();
    assert!(refused(&hook_says(&repo, &sim.join("src/gen/mod.rs"), "use std::fmt;\n")), "in the crate");
    let alias2 = repo.root.join("alias2");
    std::os::unix::fs::symlink(sim.join("src/gen"), &alias2).unwrap();
    assert!(refused(&hook_says(&repo, &alias2.join("mod.rs"), "use std::fmt;\n")), "through an alias to the linked module directory");
    // The control: clean text through the same spellings is passed.
    assert!(!refused(&hook_says(&repo, &alias2.join("mod.rs"), "pub fn g() {}\n")));
}

#[test]
fn a_crate_the_gate_never_reads_is_not_refused_by_the_hook() {
    // Discovery takes a real directory holding `src/lib.rs`: a crate with only a binary root,
    // and a crate directory that is a link, are never linted at commit, so the hook has no
    // verdict on them either.
    let repo = fixture();
    write(&repo.root.join("mock/mockspace.toml"), &CONFIG.replace("scope = \"fixture-sim\"", "scope = \"fixture-*\""));
    let bin = repo.root.join("mock/crates/fixture-bin");
    write(&bin.join("Cargo.toml"), "[package]\nname = \"fixture-bin\"\nversion = \"0.0.0\"\n");
    write(&bin.join("src/main.rs"), "fn main() {}\n");
    assert!(!refused(&hook_says(&repo, &bin.join("src/main.rs"), "use std::fmt;\nfn main() {}\n")), "a binary-only crate");
    // The control that the widened scope refuses a crate the gate does read.
    let sim = repo.root.join("mock/crates/fixture-sim");
    assert!(refused(&hook_says(&repo, &sim.join("src/lib.rs"), "use std::fmt;\n")));

    let real = repo.root.join("elsewhere");
    write(&real.join("Cargo.toml"), "[package]\nname = \"fixture-linked\"\nversion = \"0.0.0\"\n");
    write(&real.join("src/lib.rs"), "pub fn f() {}\n");
    std::os::unix::fs::symlink(&real, repo.root.join("mock/crates/fixture-linked")).unwrap();
    let through = repo.root.join("mock/crates/fixture-linked/src/lib.rs");
    assert!(!refused(&hook_says(&repo, &through, "use std::fmt;\n")), "a crate directory that is a link");
}

#[test]
fn an_existing_hooks_file_version_is_kept() {
    let repo = fixture();
    let path = repo.root.join(".github/hooks/hooks.json");
    let mut doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    doc["version"] = serde_json::json!(2);
    fs::write(&path, doc.to_string()).unwrap();
    repo.generate();
    let after: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["version"], 2, "a version the file carried was rewritten");
}

#[test]
#[ignore = "catalogue: the hooks reference lists str_replace_editor as an edit tool, its argument names are not in the reference, and the wiring does not fire the hook for it"]
fn a_copilot_str_replace_editor_edit_adding_a_violation_is_refused() {
    let repo = fixture();
    let path = repo.root.join("mock/crates/fixture-sim/src/lib.rs").display().to_string();
    let out = copilot_says(&repo, "str_replace_editor", serde_json::json!({
        "command": "str_replace", "path": path, "old_str": "pub fn f() {}", "new_str": "use std::fmt;"
    }));
    assert!(refused(&out.1), "an edit through str_replace_editor was let through: {out:?}");
}
