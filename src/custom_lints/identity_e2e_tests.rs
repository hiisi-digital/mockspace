//! The claim the pack identity rests on, run for real: the same lint sources
//! built in two trees at different paths are one identity, and a changed source
//! is another.
//!
//! `#[ignore]` because each case runs `cargo build` of a release cdylib.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::custom_lints::load_in;

/// A lint with a panic site, so the library embeds the path of the file the
/// lint was built from, which is what made the old byte digest tree specific.
fn lint_source(name: &str) -> String {
    format!(
        r#"use mockspace::{{CrateLint, Lint, LintContext, LintError}};
pub fn lint() -> Box<dyn CrateLint> {{ Box::new(Probe) }}
struct Probe;
impl Lint for Probe {{
    fn name(&self) -> &'static str {{ "{name}" }}
}}
impl CrateLint for Probe {{
    fn check(&self, _ctx: &LintContext) -> Vec<LintError> {{
        if std::env::var_os("PROBE_PANICS").is_some() {{ panic!("probe {name}") }}
        Vec::new()
    }}
}}
"#
    )
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").args(args).current_dir(dir).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

/// A mock directory of its own, under `parent`, holding one lint.
fn tree(parent: &Path, remote: &str, lint: &str) -> PathBuf {
    let root = parent.join("clone");
    std::fs::create_dir_all(root.join("mock").join("lints")).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["remote", "add", "origin", &format!("ssh://git@example.invalid/o/{remote}.git")]);
    std::fs::write(root.join("mock").join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    std::fs::write(root.join("mock").join("lints").join("probe.rs"), lint_source(lint)).unwrap();
    root.join("mock")
}

fn identity_in(cache: &Path, mock: &Path) -> String {
    let cfg = Config::from_dir(mock);
    let dep = format!(
        "{{ package = \"mockspace-lint-rules\", path = \"{}/lint-rules\" }}",
        env!("CARGO_MANIFEST_DIR")
    );
    load_in(cache, &cfg, &mock.join("mockspace.toml"), &dep)
        .unwrap()
        .expect("something to load")
        .pack
        .identity
        .clone()
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn the_same_sources_in_two_trees_are_one_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let a = tree(&tmp.path().join("a"), "identity-same", "same");
    let b = tree(&tmp.path().join("a-longer-path-for-b"), "identity-same", "same");
    let (ida, idb) = (identity_in(&cache, &a), identity_in(&cache, &b));
    assert!(!ida.is_empty());
    assert_eq!(ida, idb);
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_changed_lint_source_is_another_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let a = tree(&tmp.path().join("a"), "identity-changed", "before");
    let b = tree(&tmp.path().join("b"), "identity-changed", "after");
    assert_ne!(identity_in(&cache, &a), identity_in(&cache, &b));
}

// What a loaded library answers, as opposed to what it is called: the cases below are about a
// kept library being replayed for inputs it was not built from.

/// A lint whose name is the Rust expression `name_expr`, so a case chooses what the build bakes in.
fn lint_named_by(name_expr: &str) -> String {
    format!(
        r#"use mockspace::{{CrateLint, Lint, LintContext, LintError}};
pub fn lint() -> Box<dyn CrateLint> {{ Box::new(Probe) }}
struct Probe;
impl Lint for Probe {{
    fn name(&self) -> &'static str {{ {name_expr} }}
}}
impl CrateLint for Probe {{
    fn check(&self, _ctx: &LintContext) -> Vec<LintError> {{ Vec::new() }}
}}
"#
    )
}

fn lint_path(mock: &Path) -> PathBuf {
    mock.join("lints").join("probe.rs")
}

/// Load, read the lint's name, unload. Unloaded before a later load of the same path, or the
/// dynamic loader answers with the mapping it still has.
fn name_in(cache: &Path, mock: &Path) -> String {
    let cfg = Config::from_dir(mock);
    let dep = format!(
        "{{ package = \"mockspace-lint-rules\", path = \"{}/lint-rules\" }}",
        env!("CARGO_MANIFEST_DIR")
    );
    let loaded = load_in(cache, &cfg, &mock.join("mockspace.toml"), &dep)
        .unwrap()
        .expect("something to load");
    assert_eq!(loaded.pack.crate_lints.len(), 1);
    loaded.pack.crate_lints[0].name().to_string()
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_lint_edited_during_the_build_is_not_kept_under_the_old_content() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path(), "identity-edited-during", "before");
    let source = lint_path(&mock);
    let before = lint_source("before");
    let after = lint_source("after");
    let edit = after.clone();
    let target = source.clone();
    crate::custom_lints::store::knobs::after_identity(&cache, move || {
        std::fs::write(&target, &edit).unwrap();
    });
    // The identity was made from "before", and the build reads "after".
    assert_eq!(name_in(&cache, &mock), "after");
    // The edit is undone, so the tree is the one the identity named.
    std::fs::write(&source, &before).unwrap();
    assert_eq!(name_in(&cache, &mock), "before");
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_file_a_lint_includes_is_part_of_what_was_built() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path(), "identity-include", "unused");
    std::fs::write(lint_path(&mock), lint_named_by(r#"include_str!("probe_name.txt")"#)).unwrap();
    let text = mock.join("lints").join("probe_name.txt");
    std::fs::write(&text, "one").unwrap();
    assert_eq!(name_in(&cache, &mock), "one");
    std::fs::write(&text, "two").unwrap();
    assert_eq!(name_in(&cache, &mock), "two");
}

/// Serialises the cases that set an environment variable of the whole process.
static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn the_build_does_not_read_the_profile_from_the_environment() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path(), "identity-profile", "unused");
    std::fs::write(
        lint_path(&mock),
        lint_named_by(r#"if cfg!(debug_assertions) { "dbg" } else { "rel" }"#),
    )
    .unwrap();
    const KEY: &str = "CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS";
    // SAFETY: the lock above is held by every case that touches the environment.
    unsafe { std::env::set_var(KEY, "true") };
    let with = name_in(&cache, &mock);
    // SAFETY: as above.
    unsafe { std::env::remove_var(KEY) };
    let without = name_in(&cache, &mock);
    assert_eq!((with.as_str(), without.as_str()), ("rel", "rel"));
}
