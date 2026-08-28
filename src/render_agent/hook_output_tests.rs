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

/// Every builtin hook, rendered against one platform's helpers as the writer
/// renders them.
fn every_hook_for(
    repo_root: &std::path::Path,
    helpers: &str,
) -> Vec<(&'static str, String)> {
    let cfg = cfg_at(repo_root);
    let subst = |raw: String| {
        raw.replace("{{HOOK_HELPERS}}", helpers)
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

/// Both platforms, since the same five hooks are written twice and only one of
/// them was ever swept.
fn every_hook_everywhere(
    repo_root: &std::path::Path,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (platform, helpers) in [
        ("claude", crate::render_agent::CLAUDE_HOOK_HELPERS),
        ("copilot", crate::render_agent::COPILOT_HOOK_HELPERS),
    ] {
        for (name, script) in every_hook_for(repo_root, helpers) {
            out.push((format!("{platform}/{name}"), script));
        }
    }
    out
}

/// The payloads a sweep has to send to reach the branches that speak. One that
/// wakes nothing, one that wakes the mockspace scope checks, one that wakes the
/// yagni branch, and a write.
///
/// The yagni one is here because without it the sweep passes on the defect it
/// was written for: `builtin_no_yagni` carries the same print-then-print shape
/// as the reminder and neither of the first two payloads enters it.
fn the_payloads(repo_root: &std::path::Path) -> Vec<(&'static str, String)> {
    // Every command names the repo root absolutely, which is the branch the
    // scope check decides without consulting `pwd`. A relative command has to
    // go through `pwd`, and on macOS the temporary root is under `/var` while
    // `pwd` reports `/private/var`, so the hook declines a repo it is standing
    // in and the payload tests the silent path. That is how the first version
    // of this sweep passed on the defect it was written for.
    let at = |cmd: &str| bash_payload(&format!("cd {} && {cmd}", repo_root.display()));
    vec![
        ("unrelated", at("echo hello")),
        ("mockspace", at("cargo mock panel status")),
        ("yagni", at("git commit -m 'chore: drop it, yagni for now'")),
        (
            "write",
            serde_json::json!({
                "session_id": "t",
                "transcript_path": "/tmp/t",
                "hook_event_name": "PreToolUse",
                "tool_name": "Write",
                "tool_input": {
                    "file_path": repo_root.join("mock/crates/x/src/lib.rs").display().to_string(),
                    "content": "fn main() {}"
                }
            })
            .to_string(),
        ),
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
    let script = every_hook_for(&root, crate::render_agent::CLAUDE_HOOK_HELPERS)
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
fn the_yagni_guard_emits_one_object_when_it_has_something_to_say() {
    // The second site of the same shape, and the one the first version of this
    // sweep never reached: it takes a `git commit` whose message carries one of
    // the flagged words, and nothing else wakes it.
    let root = scratch();
    let script = every_hook_for(&root, crate::render_agent::CLAUDE_HOOK_HELPERS)
        .into_iter()
        .find(|(n, _)| *n == "no-yagni")
        .unwrap()
        .1;
    let cmd = format!(
        "cd {} && git commit -m 'chore: drop it, yagni for now'",
        root.display()
    );
    let out = run(&root, &script, &bash_payload(&cmd));
    let n = value_count(&out);
    let _ = std::fs::remove_dir_all(&root);
    assert!(
        out.contains("YAGNI"),
        "the guard said nothing, so this arm tested the silent path:\n{out}"
    );
    assert_eq!(n, 1, "the yagni guard wrote {n} JSON values:\n{out}");
}

#[test]
fn no_hook_on_either_platform_ever_writes_two_values() {
    // The sweep. Every builtin, rendered for both platforms, against every
    // payload that wakes a branch.
    //
    // `at most one` rather than `exactly one` because the two platforms differ
    // on purpose: Claude's `allow` prints an object, Copilot's prints nothing
    // and its `context` prints plain text rather than JSON. Zero values is a
    // correct Copilot answer and a wrong Claude one, which the next arm pins.
    let root = scratch();
    let mut failures = Vec::new();
    for (name, script) in every_hook_everywhere(&root) {
        for (what, payload) in the_payloads(&root) {
            let out = run(&root, &script, &payload);
            let n = value_count(&out);
            if n > 1 {
                failures.push(format!("{name} on {what} wrote {n} values:\n{out}"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
}

#[test]
fn every_claude_hook_writes_exactly_one_value_on_every_payload() {
    // Stronger than the sweep above and true only on this platform: every path
    // out of a Claude hook prints an object, so zero is a hook that stopped
    // deciding, which `at most one` would wave through.
    let root = scratch();
    let mut failures = Vec::new();
    for (name, script) in every_hook_for(&root, crate::render_agent::CLAUDE_HOOK_HELPERS) {
        for (what, payload) in the_payloads(&root) {
            let out = run(&root, &script, &payload);
            let n = value_count(&out);
            if n != 1 {
                failures.push(format!("{name} on {what} wrote {n} values:\n{out}"));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "{}", failures.join("\n---\n"));
}

/// Whether a helper function's body finishes the hook rather than returning.
///
/// Textual, so it is only worth what its control is worth: the arm below feeds
/// it a body that plainly does return, and it has to say so.
fn finishes_the_hook(helpers: &str, name: &str) -> bool {
    helpers
        .split(name)
        .nth(1)
        .unwrap_or_else(|| panic!("{name} is not in these helpers"))
        .split("\n}")
        .next()
        .unwrap()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .any(|l| l.contains("exit 0"))
}

#[test]
fn a_claude_helper_that_prints_an_object_finishes_the_hook() {
    // All three print a JSON object on this platform, so a line after any of
    // them appends a second one.
    for name in ["allow()", "deny()", "context()"] {
        assert!(
            finishes_the_hook(crate::render_agent::CLAUDE_HOOK_HELPERS, name),
            "{name} returns to its caller, so whatever follows it emits a second object"
        );
    }
}

#[test]
fn the_copilot_helpers_that_print_an_object_finish_the_hook() {
    // Only two of them, and that is the platform difference rather than an
    // oversight: Copilot's `context` prints plain text, its `allow` prints
    // nothing at all, and neither can make stdout hold two JSON values.
    for name in ["allow()", "deny()"] {
        assert!(
            finishes_the_hook(crate::render_agent::COPILOT_HOOK_HELPERS, name),
            "{name} returns to its caller on the copilot side"
        );
    }
}

#[test]
fn a_returning_helper_is_caught() {
    // The control for the two arms above. Without it they pass on a predicate
    // that has never answered false, which is how the first version of this
    // file shipped a check nothing could fail.
    let returns = "context() {\n    printf '%s\\n' \"$1\"\n}\n";
    assert!(!finishes_the_hook(returns, "context()"));

    // And a body whose only `exit 0` sits in a comment must not count.
    let commented = "context() {\n    # exit 0 would be wrong here\n    printf '%s' \"$1\"\n}\n";
    assert!(!finishes_the_hook(commented, "context()"));
}
