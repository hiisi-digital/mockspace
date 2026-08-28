//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A hook writes exactly one JSON object to stdout, or the host rejects the lot.
//!
//! Two objects concatenated still open with `{`, so the host reports the output
//! as malformed and the hook's decision is discarded. A helper that prints
//! without exiting, followed by any second emit, produces exactly that.

use super::*;
use std::process::Command;

fn scratch() -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "ms_hookout_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg_at(repo_root: &std::path::Path) -> Config {
    let mut c = Config::from_dir(std::path::Path::new("/nonexistent-mock-dir"));
    c.project_name = "fodder".to_string();
    c.repo_root = repo_root.to_path_buf();
    c.mock_dir = repo_root.join("mock");
    c
}

/// Every builtin hook, rendered as the writer renders them.
fn every_hook(repo_root: &std::path::Path) -> Vec<(&'static str, String)> {
    let cfg = cfg_at(repo_root);
    let subst = |raw: String| {
        raw.replace("{{HOOK_HELPERS}}", crate::render_agent::CLAUDE_HOOK_HELPERS)
            .replace("{{REPO_ROOT}}", &repo_root.display().to_string())
    };
    vec![
        ("check-message", subst(builtin_check_message(&cfg))),
        ("write-guard", subst(builtin_write_guard(&cfg))),
        ("reminder", subst(builtin_reminder(&cfg))),
        ("mockspace-gate", subst(builtin_mockspace_gate())),
        ("no-yagni", subst(builtin_no_yagni())),
    ]
}

/// stdout of the hook, run from `repo_root` so the scope check sees this repo.
fn run(repo_root: &std::path::Path, script: &str, payload: &str) -> String {
    let f = repo_root.join(format!("hook_{}.sh", std::process::id()));
    std::fs::write(&f, script).unwrap();
    let out = Command::new("bash")
        .arg(&f)
        .current_dir(repo_root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut ch| {
            use std::io::Write;
            ch.stdin.as_mut().unwrap().write_all(payload.as_bytes())?;
            ch.wait_with_output()
        })
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// How many top-level JSON values the output holds. Zero is a legitimate
/// answer: a hook may stay silent.
fn value_count(out: &str) -> usize {
    serde_json::Deserializer::from_str(out)
        .into_iter::<serde_json::Value>()
        .count()
}

fn bash_payload(command: &str) -> String {
    serde_json::json!({
        "session_id": "t",
        "transcript_path": "/tmp/t",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": command, "description": "d" }
    })
    .to_string()
}

#[test]
fn two_objects_are_counted_as_two() {
    // The control for every arm below. Without it they pass on a counter that
    // cannot return anything but one, and `jq -e .` is exactly such a counter:
    // it reads a stream, so it accepts concatenated objects without complaint.
    let one = r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse"}}"#;
    assert_eq!(value_count(one), 1);
    assert_eq!(value_count(&format!("{one}\n{one}\n")), 2);
    assert_eq!(value_count(""), 0);
}

#[test]
fn the_reminder_emits_one_object_when_it_has_something_to_say() {
    // The reminder speaks when the command names the mock directory, and that
    // path is the one that carried the defect: it printed its context and then
    // fell through to a second emit.
    let root = scratch();
    let script = every_hook(&root)
        .into_iter()
        .find(|(n, _)| *n == "reminder")
        .unwrap()
        .1;
    // The command names the mock directory absolutely, which is the branch the
    // scope check can decide without consulting `pwd`. On macOS the temporary
    // root is under `/var` and `pwd` reports it under `/private/var`, so the
    // cwd branch misses a repo it is standing in.
    let cmd = format!("ls {}/mock/registry", root.display());
    let out = run(&root, &script, &bash_payload(&cmd));
    let n = value_count(&out);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        out.contains("additionalContext"),
        "the reminder said nothing, so this arm tested the silent path:\n{out}"
    );
    assert_eq!(n, 1, "the reminder wrote {n} JSON values:\n{out}");
}

#[test]
fn every_hook_emits_at_most_one_object_on_an_unrelated_command() {
    let root = scratch();
    let payload = bash_payload("echo hello");
    let mut failures = Vec::new();
    for (name, script) in every_hook(&root) {
        let out = run(&root, &script, &payload);
        let n = value_count(&out);
        if n > 1 {
            failures.push(format!("{name} wrote {n} values:\n{out}"));
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
}

#[test]
fn every_hook_emits_at_most_one_object_on_a_mockspace_command() {
    // The same sweep on the command shape that wakes the scope checks, since a
    // hook that stays silent proves nothing about the path that speaks.
    let root = scratch();
    let payload = bash_payload("cargo mock panel status");
    let mut failures = Vec::new();
    for (name, script) in every_hook(&root) {
        let out = run(&root, &script, &payload);
        let n = value_count(&out);
        if n > 1 {
            failures.push(format!("{name} wrote {n} values:\n{out}"));
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
}

#[test]
fn every_hook_emits_at_most_one_object_on_a_write() {
    let root = scratch();
    let payload = serde_json::json!({
        "session_id": "t",
        "transcript_path": "/tmp/t",
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": {
            "file_path": root.join("mock/crates/x/src/lib.rs").display().to_string(),
            "content": "fn main() {}"
        }
    })
    .to_string();
    let mut failures = Vec::new();
    for (name, script) in every_hook(&root) {
        let out = run(&root, &script, &payload);
        let n = value_count(&out);
        if n > 1 {
            failures.push(format!("{name} wrote {n} values:\n{out}"));
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
}

#[test]
fn a_helper_that_prints_a_decision_does_not_return_to_its_caller() {
    // The shape rather than one instance of it. `allow`, `deny` and `context`
    // each finish the hook, so a line after any of them is unreachable and
    // cannot append a second object.
    for name in ["allow()", "deny()", "context()"] {
        let body = crate::render_agent::CLAUDE_HOOK_HELPERS
            .split(name)
            .nth(1)
            .unwrap_or_else(|| panic!("{name} is not in the helpers"))
            .split("\n}")
            .next()
            .unwrap();
        assert!(
            body.contains("exit 0"),
            "{name} returns to its caller, so whatever follows it emits a second object:\n{body}"
        );
    }
}
