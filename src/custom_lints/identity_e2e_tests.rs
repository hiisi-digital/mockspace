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
