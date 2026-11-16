//! A repo's generated crate leaves one built library on disk, run for real: a changed lint source
//! replaces the library in place, a changed revision (the engine's or a pack's) replaces the crate
//! and its library, and the store keeps the built file by a link, never by a copy.
//!
//! `#[ignore]` because each step runs `cargo build` of a release cdylib.

use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::custom_lints::store::is_key_name;
use crate::config::Config;
use crate::custom_lints::load_in;

fn lint_source(name: &str) -> String {
    format!(
        r#"use mockspace::{{CrateLint, Lint, LintContext, LintError}};
pub fn lint() -> Box<dyn CrateLint> {{ Box::new(Probe) }}
struct Probe;
impl Lint for Probe {{
    fn name(&self) -> &'static str {{ "{name}" }}
}}
impl CrateLint for Probe {{
    fn check(&self, _ctx: &LintContext) -> Vec<LintError> {{ Vec::new() }}
}}
"#
    )
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").args(args).current_dir(dir).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

/// The fixed remote gives every run of this test one package, so the target keeps one library
/// for it and not one per run.
fn tree(parent: &Path) -> PathBuf {
    let root = parent.join("clone");
    std::fs::create_dir_all(root.join("mock").join("lints")).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["remote", "add", "origin", "ssh://git@example.invalid/o/built-once.git"]);
    std::fs::write(root.join("mock").join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    std::fs::write(lint_path(&root.join("mock")), lint_source("first")).unwrap();
    root.join("mock")
}

fn lint_path(mock: &Path) -> PathBuf {
    mock.join("lints").join("probe.rs")
}

/// The engine's lint-rules, spelled two ways that cargo reads as one package. A changed spelling
/// is a changed revision to the store, as a new engine rev is, and costs no rebuild of lint-rules.
fn dep(spelling: &str) -> String {
    format!(
        "{{ package = \"mockspace-lint-rules\", path = \"{}{spelling}\" }}",
        env!("CARGO_MANIFEST_DIR")
    )
}

/// Load after the trust margin, so the library is kept, read the lint's name, unload.
fn name_in(cache: &Path, mock: &Path, dep: &str) -> String {
    std::thread::sleep(crate::custom_lints::depinfo::TRUST_MARGIN + std::time::Duration::from_millis(200));
    let cfg = Config::from_dir(mock);
    let loaded = load_in(cache, &cfg, &mock.join("mockspace.toml"), dep).unwrap().expect("something to load");
    assert_eq!(loaded.pack.crate_lints.len(), 1);
    loaded.pack.crate_lints[0].name().to_string()
}

fn key_dirs(cache: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(cache)
        .unwrap()
        .flatten()
        .filter(|e| is_key_name(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .collect()
}

/// The package name of every generated crate in the store now, as cargo names its library.
fn lib_names(cache: &Path) -> Vec<String> {
    key_dirs(cache)
        .iter()
        .map(|d| {
            let manifest = std::fs::read_to_string(d.join("Cargo.toml")).unwrap();
            let name = manifest
                .lines()
                .find_map(|l| l.strip_prefix("name = \""))
                .and_then(|l| l.strip_suffix('"'))
                .unwrap()
                .to_string();
            name.replace('-', "_")
        })
        .collect()
}

/// Where cargo puts what the generated crate builds, asked of cargo from the directory the build
/// runs in.
fn target_of(gen_dir: &Path) -> PathBuf {
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1", "--manifest-path"])
        .arg(gen_dir.join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    PathBuf::from(meta["target_directory"].as_str().unwrap())
}

/// Every built library the target holds for `names`, by inode, so a hard link is counted once.
fn libraries_in_target(target: &Path, names: &BTreeSet<String>) -> BTreeSet<u64> {
    let mut inodes = BTreeSet::new();
    for name in names {
        for dir in [target.join("release"), target.join("release").join("deps")] {
            if let Ok(meta) = std::fs::metadata(dir.join(format!("lib{name}.so"))) {
                inodes.insert(meta.ino());
            }
        }
    }
    inodes
}

/// The kept libraries, by path, with their bytes.
fn kept(cache: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let Ok(entries) = std::fs::read_dir(cache.join("built")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "so"))
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect()
}

#[test]
#[ignore = "the_mockspace_real_cargo_end_to_end_tests_are_ignored_for_th: runs a cargo build"]
fn a_repo_leaves_one_crate_one_library_and_no_copy_of_it() {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let mock = tree(tmp.path());
    let mut names = BTreeSet::new();

    assert_eq!(name_in(&cache, &mock, &dep("/lint-rules")), "first");
    names.extend(lib_names(&cache));
    let first = kept(&cache);
    assert_eq!(first.len(), 1, "the first build is kept");

    // A changed lint source: the library before it stays what it was, whatever the build did to the
    // file in the target.
    std::fs::write(lint_path(&mock), lint_source("second")).unwrap();
    assert_eq!(name_in(&cache, &mock, &dep("/lint-rules")), "second");
    names.extend(lib_names(&cache));
    for (path, bytes) in &first {
        let now = std::fs::read(path).expect("a library kept at this revision stays");
        assert!(now == *bytes, "a kept library changed under its name: {}", path.display());
    }

    // A changed revision.
    std::fs::write(lint_path(&mock), lint_source("third")).unwrap();
    assert_eq!(name_in(&cache, &mock, &dep("/lint-rules/../lint-rules")), "third");
    names.extend(lib_names(&cache));

    let dirs = key_dirs(&cache);
    assert_eq!(dirs.len(), 1, "one generated crate for the repo: {dirs:?}");
    let target = target_of(&dirs[0]);
    let in_target = libraries_in_target(&target, &names);
    assert_eq!(in_target.len(), 1, "one library in the target for {names:?} under {}", target.display());

    let now = kept(&cache);
    assert_eq!(now.len(), 1, "an older revision is not kept: {:?}", now.iter().map(|k| &k.0).collect::<Vec<_>>());
    let ino = std::fs::metadata(&now[0].0).unwrap().ino();
    assert!(in_target.contains(&ino), "the kept library is the target's file, not a copy of it");
}
