//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?}: {e}"))
        .status
        .success();
    assert!(ok, "git {args:?} failed");
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

const RUN: &str = "results/hash/hash_n64/linux-x86_64-c4-bare-amd/20261115T093000Z-a";

/// A repository with a bench tree whose support crate reaches a crate outside
/// the tree by path, an unrelated crate beside it, and committed outputs in
/// the current layout and in the old ledger.
fn repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let mock = root.join("mock");
    let bench = mock.join("benches");
    write(&bench.join("bench.toml"), "[timing]\n");
    write(&bench.join("hash/bench.toml"), "kind = \"tracking\"\ntitle = \"h\"\narms = [\"fnv\"]\npoints = [64]\n");
    write(&bench.join("hash/arms/fnv/src/lib.rs"), "pub fn f() {}\n");
    write(
        &bench.join("support/mix/Cargo.toml"),
        "[package]\nname = \"mix\"\nversion = \"0.0.0\"\n\n[dependencies]\ncore = { path = \"../../../../crates/core\" }\n",
    );
    write(&bench.join("support/mix/src/lib.rs"), "pub fn m() {}\n");
    write(&root.join("crates/core/Cargo.toml"), "[package]\nname = \"core\"\nversion = \"0.0.0\"\n");
    write(&root.join("crates/core/src/lib.rs"), "pub fn c() {}\n");
    write(&root.join("crates/other/Cargo.toml"), "[package]\nname = \"other\"\nversion = \"0.0.0\"\n");
    write(&root.join("crates/other/src/lib.rs"), "pub fn o() {}\n");
    write(&bench.join(RUN).join("samples.csv"), "run\n");
    write(&bench.join("results/INDEX.md"), "# index\n");
    write(&bench.join(".bench_history/hash_n64.tsv"), "# schema_v1\n");
    write(&root.join(".gitignore"), "*.gen.rs\n");
    write(&root.join("README.md"), "readme\n");
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    (tmp, mock, bench)
}

fn dirty(mock: &Path, bench: &Path) -> bool {
    let root = mock.parent().unwrap();
    let c = commit_marked(root, mock, bench).expect("a git tree has a commit");
    c.ends_with("-dirty")
}

/// The case that must not mark: the outputs of this and of an older layout,
/// tracked and new, a file the repository ignores beside the code, and a change
/// to a file nothing measured is built from, tracked or new.
#[test]
fn outputs_untracked_files_and_unrelated_code_leave_the_mark_off() {
    let (_tmp, mock, bench) = repo();
    let root = mock.parent().unwrap().to_path_buf();
    assert!(!dirty(&mock, &bench), "a clean tree");
    write(&bench.join(RUN).join("samples.csv"), "changed\n");
    write(&bench.join("results/INDEX.md"), "# regenerated\n");
    write(&bench.join(".bench_history/hash_n64.tsv"), "# changed\n");
    write(&bench.join("results/hash/hash_n64/linux-x86_64-c4-bare-amd/20261116T000000Z-b/samples.csv"), "new\n");
    write(&bench.join("hash/arms/fnv/src/build.gen.rs"), "// ignored\n");
    write(&root.join("crates/other/notes.txt"), "untracked\n");
    write(&root.join("crates/other/src/lib.rs"), "pub fn o2() {}\n");
    write(&root.join("README.md"), "changed\n");
    assert!(!dirty(&mock, &bench), "nothing a variant is built from changed");
}

/// The case that must mark: the code of an arm, the code of a crate the tree
/// reaches only through a support crate's path dependency, a rename of a bench
/// file, and a new file in either that git does not ignore.
#[test]
fn a_change_to_an_arm_or_a_crate_it_reaches_marks_dirty() {
    let (_tmp, mock, bench) = repo();
    let root = mock.parent().unwrap().to_path_buf();
    write(&bench.join("hash/arms/fnv/src/lib.rs"), "pub fn f() { () }\n");
    assert!(dirty(&mock, &bench), "an arm's own source changed");
    git(&root, &["checkout", "-q", "--", "."]);
    assert!(!dirty(&mock, &bench));
    write(&root.join("crates/core/src/lib.rs"), "pub fn c() { () }\n");
    assert!(dirty(&mock, &bench), "a crate reached by path changed");
    git(&root, &["checkout", "-q", "--", "."]);
    git(&root, &["mv", "mock/benches/hash/bench.toml", "mock/benches/hash/moved.toml"]);
    assert!(dirty(&mock, &bench), "a rename of a bench file");
    git(&root, &["reset", "-q", "--hard"]);
    assert!(!dirty(&mock, &bench));
    write(&bench.join("hash/arms/fnv/src/generated.rs"), "// untracked\n");
    assert!(dirty(&mock, &bench), "a new source beside an arm");
    std::fs::remove_file(bench.join("hash/arms/fnv/src/generated.rs")).unwrap();
    assert!(!dirty(&mock, &bench));
    write(&root.join("crates/core/notes.txt"), "untracked\n");
    assert!(dirty(&mock, &bench), "a new file in a crate reached by path");
}

#[test]
fn both_sides_of_a_rename_are_named() {
    let z = b" M a.rs\0R  new.rs\0old.rs\0";
    assert_eq!(changed(z, Path::new("/r")), [
        PathBuf::from("/r/a.rs"),
        PathBuf::from("/r/new.rs"),
        PathBuf::from("/r/old.rs")
    ]);
}
