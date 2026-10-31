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

/// The fixture was written just now, and a source changed within the trust margin is not kept.
fn settle() {
    std::thread::sleep(crate::custom_lints::depinfo::TRUST_MARGIN + std::time::Duration::from_millis(200));
}

fn identity_in(cache: &Path, mock: &Path) -> String {
    settle();
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
    settle();
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
    let fired = std::sync::atomic::AtomicBool::new(false);
    crate::custom_lints::store::knobs::after_identity(&cache, move || {
        if !fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
            std::fs::write(&target, &edit).unwrap();
        }
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

/// Two trees of one commit, each with the lint `lint_src` and its own `files` under the mock
/// directory, loaded in turn from one store. What each answers, in order.
fn names_in_two_trees(tag: &str, lint_src: &str, setup: impl Fn(&Path, &str), what: [&str; 2]) -> [String; 2] {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let trees: Vec<PathBuf> = ["a", "b"]
        .iter()
        .zip(what)
        .map(|(who, content)| {
            let mock = tree(&tmp.path().join(who), tag, "unused");
            std::fs::write(lint_path(&mock), lint_src).unwrap();
            setup(&mock, content);
            mock
        })
        .collect();
    [name_in(&cache, &trees[0]), name_in(&cache, &trees[1])]
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_file_beside_the_lints_is_read_from_the_asking_tree() {
    let got = names_in_two_trees(
        "identity-beside",
        &lint_named_by(r#"include_str!("../name.txt")"#),
        |mock, text| std::fs::write(mock.join("name.txt"), text).unwrap(),
        ["one", "two"],
    );
    assert_eq!(got, ["one", "two"]);
}

#[cfg(unix)]
#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_file_behind_a_link_in_the_lints_is_read_from_the_asking_tree() {
    let got = names_in_two_trees(
        "identity-link",
        &lint_named_by(r#"include_str!("data/name.txt")"#),
        |mock, text| {
            std::fs::create_dir_all(mock.join("data")).unwrap();
            std::fs::write(mock.join("data").join("name.txt"), text).unwrap();
            std::os::unix::fs::symlink("../data", mock.join("lints").join("data")).unwrap();
        },
        ["one", "two"],
    );
    assert_eq!(got, ["one", "two"]);
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_variable_the_build_still_gets_is_checked_when_the_lint_reads_it() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path(), "identity-env-read", "unused");
    std::fs::write(
        lint_path(&mock),
        lint_named_by(r#"match option_env!("NO_COLOR") { Some(_) => "set", None => "unset" }"#),
    )
    .unwrap();
    // SAFETY: the lock above is held by every case that touches the environment.
    unsafe { std::env::set_var("NO_COLOR", "1") };
    let with = name_in(&cache, &mock);
    // SAFETY: as above.
    unsafe { std::env::remove_var("NO_COLOR") };
    let without = name_in(&cache, &mock);
    assert_eq!((with.as_str(), without.as_str()), ("set", "unset"));
}

fn probe_tool(mock: &Path, body: &str) {
    let t = mock.join("tools").join("probe-tool");
    std::fs::create_dir_all(t.join("src")).unwrap();
    std::fs::write(
        t.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-tool\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nmockspace = {{ package = \"mockspace-lint-rules\", path = \"{}/lint-rules\" }}\n",
            env!("CARGO_MANIFEST_DIR")
        ),
    )
    .unwrap();
    std::fs::write(
        t.join("src").join("lib.rs"),
        format!("pub fn collect(_p: &mut mockspace::LintPack) {{}}\n{body}\n"),
    )
    .unwrap();
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_variable_a_dependency_reads_is_checked() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path(), "review-dep-env", "unused");
    probe_tool(&mock, r#"pub const NAME: &str = match option_env!("NO_COLOR") { Some(_) => "set", None => "unset" };"#);
    std::fs::write(lint_path(&mock), lint_named_by("probe_tool::NAME")).unwrap();
    unsafe { std::env::set_var("NO_COLOR", "1") };
    let with = name_in(&cache, &mock);
    unsafe { std::env::remove_var("NO_COLOR") };
    let without = name_in(&cache, &mock);
    assert_eq!((with.as_str(), without.as_str()), ("set", "unset"));
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_tool_manifest_dir_names_the_asking_tree() {
    let got = names_in_two_trees(
        "review-manifest-dir",
        &lint_named_by("probe_tool::NAME"),
        |mock, _| probe_tool(mock, r#"pub const NAME: &str = env!("CARGO_MANIFEST_DIR");"#),
        ["", ""],
    );
    assert!(got[0].contains("/a/clone/"), "{got:?}");
    assert!(got[1].contains("/b/clone/"), "{got:?}");
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_source_set_back_in_time_is_built_not_reused_by_cargo() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path(), "review-setback", "unused");
    let text = mock.join("lints").join("probe_name.txt");
    std::fs::write(lint_path(&mock), lint_named_by(r#"include_str!("probe_name.txt")"#)).unwrap();
    std::fs::write(&text, "one").unwrap();
    assert_eq!(name_in(&cache, &mock), "one");
    std::fs::write(&text, "two").unwrap();
    std::fs::File::options().write(true).open(&text).unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600)).unwrap();
    assert_eq!(name_in(&cache, &mock), "two");
}



#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_tool_source_set_back_in_time_is_not_kept_as_the_new_content() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let a = tree(&tmp.path().join("a"), "r3-tool-setback", "unused");
    probe_tool(&a, r#"pub const NAME: &str = "one";"#);
    std::fs::write(lint_path(&a), lint_named_by("probe_tool::NAME")).unwrap();
    assert_eq!(name_in(&cache, &a), "one");
    let lib = a.join("tools").join("probe-tool").join("src").join("lib.rs");
    std::fs::write(&lib, "pub fn collect(_p: &mut mockspace::LintPack) {}\npub const NAME: &str = \"two\";\n").unwrap();
    std::fs::File::options().write(true).open(&lib).unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600)).unwrap();
    let in_a = name_in(&cache, &a);
    // A second tree of the new content, written plainly, with no set-back time.
    let b = tree(&tmp.path().join("b"), "r3-tool-setback", "unused");
    probe_tool(&b, r#"pub const NAME: &str = "two";"#);
    std::fs::write(lint_path(&b), lint_named_by("probe_tool::NAME")).unwrap();
    let in_b = name_in(&cache, &b);
    assert_eq!((in_a.as_str(), in_b.as_str()), ("two", "two"));
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_lint_file_macro_reads_the_same_in_every_tree_and_names_none() {
    let got = names_in_two_trees("r3-file-macro", &lint_named_by("file!()"), |_, _| {}, ["", ""]);
    assert_eq!(got[0], got[1]);
    assert!(!got.iter().any(|g| g.contains("/a/clone/") || g.contains("/b/clone/")), "{got:?}");
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_tool_build_script_manifest_dir_names_the_asking_tree() {
    let got = names_in_two_trees(
        "r3-build-script",
        &lint_named_by("probe_tool::NAME"),
        |mock, _| {
            probe_tool(mock, r#"include!(concat!(env!("OUT_DIR"), "/name.rs"));"#);
            std::fs::write(
                mock.join("tools").join("probe-tool").join("build.rs"),
                "fn main() { let d = std::env::var(\"CARGO_MANIFEST_DIR\").unwrap(); \
                 std::fs::write(std::path::Path::new(&std::env::var(\"OUT_DIR\").unwrap()).join(\"name.rs\"), \
                 format!(\"pub const NAME: &str = {d:?};\")).unwrap(); }\n",
            )
            .unwrap();
        },
        ["", ""],
    );
    assert!(got[0].contains("/a/clone/"), "{got:?}");
    assert!(got[1].contains("/b/clone/"), "{got:?}");
}

/// A cargo config written where the build will find it, and removed when dropped.
struct ConfigGuard(PathBuf);

impl ConfigGuard {
    fn write(text: &str) -> Self {
        let dir = std::env::current_dir().unwrap().join(".cargo");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        std::fs::write(&file, text).unwrap();
        Self(file)
    }
}

impl Drop for ConfigGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_cargo_config_that_appears_during_the_build_is_not_kept_under_the_old_key() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path(), "review2-config", "unused");
    std::fs::write(
        lint_path(&mock),
        lint_named_by(r#"if cfg!(review_probe_flag) { "on" } else { "off" }"#),
    )
    .unwrap();
    let config = std::sync::Mutex::new(None);
    let config = std::sync::Arc::new(config);
    let held = config.clone();
    let fired = std::sync::atomic::AtomicBool::new(false);
    // The identity is made without the config, and the build meets it.
    crate::custom_lints::store::knobs::after_identity(&cache, move || {
        if !fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
            *held.lock().unwrap() = Some(ConfigGuard::write(
                "[target.x86_64-unknown-linux-gnu]\nrustflags = [\"--cfg\", \"review_probe_flag\"]\n",
            ));
        }
    });
    let first = name_in(&cache, &mock);
    // The config is gone, so the tree is the one the identity named.
    drop(config.lock().unwrap().take());
    let second = name_in(&cache, &mock);
    assert_eq!((first.as_str(), second.as_str()), ("on", "off"));
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn the_build_does_not_read_rustflags_from_the_environment() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path(), "review2-rustflags", "unused");
    std::fs::write(
        lint_path(&mock),
        lint_named_by(r#"if cfg!(review_env_flag) { "on" } else { "off" }"#),
    )
    .unwrap();
    // SAFETY: the lock above is held by every case that touches the environment.
    unsafe { std::env::set_var("RUSTFLAGS", "--cfg review_env_flag") };
    let name = name_in(&cache, &mock);
    // SAFETY: as above.
    unsafe { std::env::remove_var("RUSTFLAGS") };
    assert_eq!(name, "off");
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn a_second_tree_of_the_same_sources_loads_the_kept_library_and_builds_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let a = tree(&tmp.path().join("a"), "review2-no-build", "same");
    let b = tree(&tmp.path().join("a-longer-path-for-b"), "review2-no-build", "same");
    identity_in(&cache, &a);
    let after_a = crate::custom_lints::store::knobs::builds_under(&cache);
    assert!(after_a >= 1, "the first tree has to build");
    identity_in(&cache, &b);
    assert_eq!(crate::custom_lints::store::knobs::builds_under(&cache), after_a);
}
