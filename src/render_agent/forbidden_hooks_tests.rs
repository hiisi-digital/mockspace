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
    let settings = fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap();
    assert!(
        settings.contains("lint-forbidden-"),
        "no event runs the hook:\n{settings}"
    );
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
