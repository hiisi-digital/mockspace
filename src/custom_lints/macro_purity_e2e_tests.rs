//! Whether the library of a pack whose graph has a derive crate of the tree is kept, run with a
//! real cargo build.
//!
//! `#[ignore]` because each case runs `cargo build` of a release cdylib.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
use crate::custom_lints::load_in;

fn git(dir: &Path, args: &[&str]) {
    assert!(Command::new("git").args(args).current_dir(dir).status().unwrap().success(), "git {args:?}");
}

/// A mock directory holding one tool, which derives with the procedural macro crate beside it, and
/// that crate, whose manifest has `derive_dependencies` and whose source is `derive_source`.
fn tree(parent: &Path, derive_dependencies: &str, derive_source: &str) -> PathBuf {
    let root = parent.join("clone");
    let mock = root.join("mock");
    let tool = mock.join("tools").join("probe-tool");
    let derive = mock.join("derive-probe");
    for dir in [tool.join("src"), derive.join("src"), mock.join("lints")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    git(&root, &["init", "-q"]);
    git(&root, &["remote", "add", "origin", "ssh://git@example.invalid/o/purity.git"]);
    std::fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    std::fs::write(
        tool.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-tool\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\n\
             mockspace = {{ package = \"mockspace-lint-rules\", path = \"{}/lint-rules\" }}\n\
             probe-derive = {{ path = \"../../derive-probe\" }}\n",
            env!("CARGO_MANIFEST_DIR")
        ),
    )
    .unwrap();
    std::fs::write(
        tool.join("src").join("lib.rs"),
        "pub fn collect(_p: &mut mockspace::LintPack) {}\n#[derive(probe_derive::Probe)]\npub struct Marked;\n",
    )
    .unwrap();
    std::fs::write(
        derive.join("Cargo.toml"),
        format!(
            "[package]\nname = \"probe-derive\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[lib]\nproc-macro = true\n\n[dependencies]\n{derive_dependencies}\n"
        ),
    )
    .unwrap();
    std::fs::write(derive.join("src").join("lib.rs"), derive_source).unwrap();
    mock
}

const PURE: &str = "use proc_macro::TokenStream;\n#[proc_macro_derive(Probe)]\npub fn probe(_: TokenStream) -> TokenStream { TokenStream::new() }\n";

/// How many libraries the store kept after a build of the tree.
fn kept_after_build(mock: &Path, cache: &Path) -> usize {
    std::thread::sleep(crate::custom_lints::depinfo::TRUST_MARGIN + std::time::Duration::from_millis(200));
    let cfg = Config::from_dir(mock);
    let dep = format!("{{ package = \"mockspace-lint-rules\", path = \"{}/lint-rules\" }}", env!("CARGO_MANIFEST_DIR"));
    load_in(cache, &cfg, &mock.join("mockspace.toml"), &dep).unwrap().expect("something to load");
    std::fs::read_dir(cache.join("built"))
        .map(|d| d.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".deps")).count())
        .unwrap_or(0)
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn the_library_of_a_pack_with_a_pure_derive_crate_of_the_tree_is_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tree(tmp.path(), "", PURE);
    assert_eq!(kept_after_build(&mock, &tmp.path().join("cache")), 1);
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn the_library_of_a_pack_whose_derive_crate_depends_outside_the_allowlist_is_not_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tree(tmp.path(), "serde_json = \"1\"", PURE);
    assert_eq!(kept_after_build(&mock, &tmp.path().join("cache")), 0);
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn the_library_of_a_pack_whose_derive_crate_can_read_a_file_is_not_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let source = format!("{PURE}fn read() {{ let _ = std::fs::read(\"Cargo.toml\"); }}\n");
    let mock = tree(tmp.path(), "", &source);
    assert_eq!(kept_after_build(&mock, &tmp.path().join("cache")), 0);
}
