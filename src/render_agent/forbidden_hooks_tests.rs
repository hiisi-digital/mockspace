//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The hooks generated from `[lints.forbidden-imports]` are wired into the
//! settings like every other hook, so the editor calls them.
//!
//! Written but unregistered, they sit in `.claude/hooks/` under no event and
//! nothing runs them, which the workspace's regen then refuses as an orphan.

use std::fs;
use std::path::Path;

use super::*;

fn cfg_with_rule(repo: &Path) -> Config {
    let mock = repo.join("mock");
    fs::create_dir_all(&mock).unwrap();
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = repo.to_path_buf();
    cfg.mock_dir = mock;
    cfg.src_dirs = vec![repo.join("mock/crates")];
    let rule: std::collections::HashMap<String, String> = [
        ("rule.no-dyn.scope", "sim-*"),
        ("rule.no-dyn.forbidden", "dyn"),
        ("rule.no-dyn.reason", "no dispatch"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    cfg.lint_overrides
        .params
        .insert("forbidden-imports".to_string(), rule);
    cfg
}

/// Runs the lint-derived generation the way the renderer does: the hook
/// directories are not created beforehand, since they do not exist yet in a
/// fresh tree at that point of the render.
fn derive(repo: &Path, cfg: &Config) -> (usize, Vec<HookMeta>) {
    generate_lint_derived_content(
        cfg,
        &repo.join(".claude/rules"),
        &repo.join(".github/instructions"),
        &repo.join(".claude/hooks"),
        &repo.join(".github/hooks"),
        "",
        "",
        "",
    )
}

#[test]
fn a_forbidden_imports_hook_is_returned_with_the_events_it_guards() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    let (_, hooks) = derive(tmp.path(), &cfg);
    let hook = hooks
        .iter()
        .find(|h| h.name.starts_with("lint-forbidden-"))
        .expect("the generated hook is returned for the settings step");
    // It inspects the text being written, so it guards the writing tools, not Bash.
    assert!(
        hook.matchers.iter().any(|m| m == "Write"),
        "{:?}",
        hook.matchers
    );
    assert!(
        hook.matchers.iter().any(|m| m == "Edit"),
        "{:?}",
        hook.matchers
    );
    assert!(
        !hook.matchers.iter().any(|m| m == "Bash"),
        "{:?}",
        hook.matchers
    );
}

#[test]
fn a_forbidden_imports_hook_is_written_into_a_tree_without_a_hooks_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    let (_, hooks) = derive(tmp.path(), &cfg);
    for h in &hooks {
        assert!(
            tmp.path().join(".claude/hooks").join(&h.name).is_file(),
            "{} was returned but not written",
            h.name
        );
        assert!(tmp.path().join(".github/hooks").join(&h.name).is_file());
    }
    assert!(!hooks.is_empty());
}

#[test]
fn the_generated_settings_register_a_forbidden_imports_hook_under_an_event() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    let (_, hooks) = derive(tmp.path(), &cfg);
    wire::generate_settings(tmp.path(), &hooks);
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap(),
    )
    .unwrap();
    let entries = settings["hooks"]["PreToolUse"]
        .as_array()
        .expect("a PreToolUse list");
    for matcher in ["Write", "Edit"] {
        let script = entries
            .iter()
            .filter(|e| e["matcher"] == matcher)
            .flat_map(|e| e["hooks"].as_array().into_iter().flatten())
            .filter_map(|h| h["command"].as_str())
            .find(|c| c.contains("lint-forbidden-"))
            .unwrap_or_else(|| panic!("no {matcher} entry runs the hook:\n{settings}"));
        assert!(
            tmp.path().join(script).is_file(),
            "the {matcher} entry names {script}, which is not on disk"
        );
    }
}

#[test]
fn a_config_without_forbidden_imports_returns_no_hooks() {
    // The control: the new return is not simply every hook that exists.
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    fs::create_dir_all(&mock).unwrap();
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = tmp.path().to_path_buf();
    let (n, hooks) = derive(tmp.path(), &cfg);
    assert_eq!(n, 0);
    assert!(hooks.is_empty());
}

/// Runs the generated hook the way the editor does, with a Write of `content` to `path`.
fn run_hook(repo: &Path, hook: &str, path: &Path, content: &str) -> String {
    let payload = serde_json::json!({
        "session_id": "t",
        "transcript_path": "/tmp/t",
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": path.display().to_string(), "content": content }
    })
    .to_string();
    let script = fs::read_to_string(repo.join(".claude/hooks").join(hook)).unwrap();
    super::write_guard_phase_tests::run_from(repo, &script, &payload)
}

fn generated_hook(repo: &Path) -> String {
    let cfg = cfg_with_rule(repo);
    let (_, hooks) = derive(repo, &cfg);
    hooks
        .into_iter()
        .find(|h| h.name.starts_with("lint-forbidden-"))
        .expect("the hook is generated")
        .name
}

#[test]
fn a_forbidden_import_in_a_scoped_crate_is_denied() {
    let tmp = tempfile::tempdir().unwrap();
    let hook = generated_hook(tmp.path());
    let target = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let out = run_hook(tmp.path(), &hook, &target, "fn f(x: &dyn Tr) {}\n");
    assert!(
        super::write_guard_phase_tests::refused(&out),
        "the forbidden token was let through: {out}"
    );
}

#[test]
fn the_same_content_in_a_crate_outside_the_scope_is_allowed() {
    // The control for the deny above: it is the scope that decides, not the content alone.
    let tmp = tempfile::tempdir().unwrap();
    let hook = generated_hook(tmp.path());
    let target = tmp.path().join("mock/crates/other/src/lib.rs");
    let out = run_hook(tmp.path(), &hook, &target, "fn f(x: &dyn Tr) {}\n");
    assert!(
        !super::write_guard_phase_tests::refused(&out),
        "a crate outside the scope was denied: {out}"
    );
}

#[test]
fn clean_content_in_a_scoped_crate_is_allowed() {
    let tmp = tempfile::tempdir().unwrap();
    let hook = generated_hook(tmp.path());
    let target = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let out = run_hook(tmp.path(), &hook, &target, "fn f() {}\n");
    assert!(
        !super::write_guard_phase_tests::refused(&out),
        "clean content was denied: {out}"
    );
}

#[test]
fn a_forbidden_import_written_through_an_edit_is_denied() {
    // An Edit carries the text in `new_string`, not `content`.
    let tmp = tempfile::tempdir().unwrap();
    let hook = generated_hook(tmp.path());
    let target = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let payload = |path: &Path| {
        serde_json::json!({
            "session_id": "t",
            "transcript_path": "/tmp/t",
            "hook_event_name": "PreToolUse",
            "tool_name": "Edit",
            "tool_input": {
                "file_path": path.display().to_string(),
                "old_string": "fn f() {}",
                "new_string": "fn f(x: &dyn Tr) {}",
            }
        })
        .to_string()
    };
    let script = fs::read_to_string(tmp.path().join(".claude/hooks").join(&hook)).unwrap();
    let out = super::write_guard_phase_tests::run_from(tmp.path(), &script, &payload(&target));
    assert!(super::write_guard_phase_tests::refused(&out), "the edit was let through: {out}");
    let outside = tmp.path().join("mock/crates/other/src/lib.rs");
    let out = super::write_guard_phase_tests::run_from(tmp.path(), &script, &payload(&outside));
    assert!(!super::write_guard_phase_tests::refused(&out), "control: out of scope is allowed: {out}");
}

#[test]
#[should_panic(expected = "failed to create a hooks or rules directory")]
fn a_hooks_directory_that_cannot_be_made_fails_loudly() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join(".claude")).unwrap();
    // a regular file where the directory must go
    fs::write(tmp.path().join(".claude/hooks"), "").unwrap();
    let cfg = cfg_with_rule(tmp.path());
    derive(tmp.path(), &cfg);
}

#[test]
#[should_panic(expected = "failed to write forbidden-imports claude hook")]
fn a_hook_that_cannot_be_written_fails_loudly() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    derive(tmp.path(), &cfg);
    // the second run meets a directory where the script goes
    let hook = generated_hook(tmp.path());
    let path = tmp.path().join(".claude/hooks").join(&hook);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    derive(tmp.path(), &cfg);
}
