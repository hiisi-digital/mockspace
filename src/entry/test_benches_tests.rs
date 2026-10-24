//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

fn write(p: &Path, s: &str) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, s).unwrap();
}

/// A git repository with a mock directory and one bench arm, as `(root, mock)`.
fn repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let st = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(st.success());
    let mock = root.join("mock");
    write(
        &mock.join("benches/sample/bench.toml"),
        "name = \"sample\"\n",
    );
    write(
        &mock.join("benches/sample/arms/a/src/lib.rs"),
        "pub fn f() {}\n",
    );
    (tmp, root, mock)
}

fn now(root: &Path, mock: &Path) -> String {
    digest(root, mock, &mock.join("benches")).expect("a git repository has a digest")
}

fn moved_by(edit: impl FnOnce(&Path, &Path)) -> bool {
    let (_t, root, mock) = repo();
    let before = now(&root, &mock);
    edit(&root, &mock);
    before != now(&root, &mock)
}

// ── decide: the whole matrix, asked x named x record ──

#[test]
fn decide_answers_every_one_of_the_twelve_cases() {
    use Run::*;
    use Skip::*;
    use Verdict::{Run as R, Skip as S};
    let cases: [(bool, bool, Option<&str>, Verdict); 12] = [
        (false, false, None, R(NeverGreen)),
        (false, false, Some("x"), S(Unchanged)),
        (false, false, Some("y"), R(Moved)),
        (false, true, None, S(OtherCratesNamed)),
        (false, true, Some("x"), S(OtherCratesNamed)),
        (false, true, Some("y"), S(OtherCratesNamed)),
        (true, false, None, R(Asked)),
        (true, false, Some("x"), R(Asked)),
        (true, false, Some("y"), R(Asked)),
        (true, true, None, R(Asked)),
        (true, true, Some("x"), R(Asked)),
        (true, true, Some("y"), R(Asked)),
    ];
    for (asked, named, rec, want) in cases {
        assert_eq!(
            decide(asked, named, "x", rec),
            want,
            "asked={asked} named={named} recorded={rec:?}"
        );
    }
}

// ── the digest as a pure function of its entries ──

#[test]
fn the_digest_does_not_depend_on_the_order_the_files_are_fed() {
    let a = vec![
        ("a".to_string(), b"1".to_vec()),
        ("b".to_string(), b"2".to_vec()),
        ("c".to_string(), b"3".to_vec()),
    ];
    let mut reversed = a.clone();
    reversed.reverse();
    assert_eq!(digest_entries(a), digest_entries(reversed));
}

#[test]
fn the_digest_tells_a_moved_byte_a_rename_and_an_added_file_apart() {
    let base = vec![("a".to_string(), b"1".to_vec())];
    let d = digest_entries(base.clone());
    assert_ne!(d, digest_entries(vec![("a".to_string(), b"2".to_vec())]));
    assert_ne!(d, digest_entries(vec![("b".to_string(), b"1".to_vec())]));
    let mut more = base;
    more.push(("c".to_string(), Vec::new()));
    assert_ne!(d, digest_entries(more));
}

#[test]
fn a_boundary_between_name_and_bytes_cannot_be_shifted() {
    // "ab" + "c" must not hash like "a" + "bc".
    assert_ne!(
        digest_entries(vec![("ab".to_string(), b"c".to_vec())]),
        digest_entries(vec![("a".to_string(), b"bc".to_vec())])
    );
}

// ── what counts as an input ──

#[test]
fn an_edited_arm_moves_the_digest() {
    assert!(moved_by(|_, m| {
        write(
            &m.join("benches/sample/arms/a/src/lib.rs"),
            "pub fn f() { 1; }\n",
        )
    }));
}

#[test]
fn an_added_support_file_moves_the_digest() {
    assert!(moved_by(|_, m| {
        write(&m.join("benches/support/src/lib.rs"), "")
    }));
}

#[test]
fn a_renamed_file_moves_the_digest() {
    assert!(moved_by(|_, m| {
        let d = m.join("benches/sample/arms/a/src");
        fs::rename(d.join("lib.rs"), d.join("other.rs")).unwrap();
    }));
}

#[test]
fn build_output_does_not_move_the_digest() {
    assert!(!moved_by(|_, m| {
        write(&m.join("benches/sample/arms/a/target/debug/junk"), "built");
    }));
}

/// `mock bench run` writes `results/` and `history/`, so counting them made
/// every bench run rerun every arm on the next `mock test`.
#[test]
fn generated_results_and_history_do_not_move_the_digest() {
    assert!(!moved_by(|_, m| {
        write(&m.join("benches/results/sample/a.csv"), "1,2");
        write(&m.join("benches/history/sample/a.toml"), "median = 1");
    }));
}

#[test]
fn an_untracked_file_that_git_ignores_does_not_move_the_digest() {
    assert!(!moved_by(|r, m| {
        write(&r.join(".gitignore"), "scratch/\n");
        write(&m.join("benches/scratch/x.rs"), "junk");
    }));
}

/// The lockfile the bench build writes beside the tree is output of the run,
/// and the run must not move the digest it recorded.
#[test]
fn a_lockfile_written_under_the_benches_does_not_move_the_digest() {
    assert!(!moved_by(|_, m| {
        write(&m.join("benches/Cargo.lock"), "# written by the build")
    }));
}

/// The toolchain pin, cargo's configuration, the manifest and the lockfile are
/// what the members' fingerprints are salted with, and an arm is built with
/// them too: every one of them, beside the mock directory and beside the
/// repository root.
#[test]
fn every_workspace_input_moves_the_digest_at_both_bases() {
    for name in WORKSPACE_INPUTS {
        for base in ["mock", "root"] {
            assert!(
                moved_by(|r, m| {
                    let b = if base == "root" { r } else { m };
                    write(&b.join(name), "changed");
                }),
                "{base}/{name} did not move the digest"
            );
        }
    }
}

/// kaski's support crates depend on workspace members by path. A broken member
/// has to run the benches again.
#[test]
fn a_path_dependency_of_a_bench_crate_moves_the_digest() {
    let setup = |r: &Path| {
        write(
            &r.join("mock/benches/support/s/Cargo.toml"),
            "[package]\nname = \"s\"\n[dependencies]\ndep = { path = \"../../../../crates/dep\" }\n",
        );
        write(
            &r.join("crates/dep/Cargo.toml"),
            "[package]\nname = \"dep\"\n",
        );
        write(&r.join("crates/dep/src/lib.rs"), "pub fn v() -> u8 { 1 }\n");
        write(
            &r.join("crates/other/src/lib.rs"),
            "pub fn v() -> u8 { 1 }\n",
        );
    };
    let (_t, root, mock) = repo();
    setup(&root);
    let before = now(&root, &mock);
    write(
        &root.join("crates/dep/src/lib.rs"),
        "pub fn v() -> u8 { 2 }\n",
    );
    assert_ne!(before, now(&root, &mock), "the dependency moved");
    let before = now(&root, &mock);
    write(
        &root.join("crates/other/src/lib.rs"),
        "pub fn v() -> u8 { 2 }\n",
    );
    assert_eq!(
        before,
        now(&root, &mock),
        "a crate nothing names is not an input"
    );
}

/// Cargo builds a bench crate's dev and build dependencies, and the same under
/// `[target.*]`, as it builds its plain ones.
#[test]
fn a_path_dependency_in_any_dependency_table_moves_the_digest() {
    let mut heads: Vec<String> = Vec::new();
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        heads.push(kind.to_string());
        heads.push(format!("target.'cfg(unix)'.{kind}"));
        heads.push(format!("target.x86_64-unknown-linux-gnu.{kind}"));
    }
    for head in heads {
        let (_t, root, mock) = repo();
        write(
            &mock.join("benches/sample/Cargo.toml"),
            &format!("[{head}]\ndep = {{ path = \"../../../crates/dep\" }}\n"),
        );
        write(&root.join("crates/dep/src/lib.rs"), "1");
        let before = now(&root, &mock);
        write(&root.join("crates/dep/src/lib.rs"), "2");
        assert_ne!(before, now(&root, &mock), "[{head}]");
    }
}

#[test]
fn a_dependency_of_a_dependency_moves_the_digest() {
    let (_t, root, mock) = repo();
    write(
        &mock.join("benches/sample/Cargo.toml"),
        "[dependencies]\na = { path = \"../../../crates/a\" }\n",
    );
    write(
        &root.join("crates/a/Cargo.toml"),
        "[dependencies]\nb = { path = \"../b\" }\n",
    );
    write(&root.join("crates/b/src/lib.rs"), "1");
    let before = now(&root, &mock);
    write(&root.join("crates/b/src/lib.rs"), "2");
    assert_ne!(before, now(&root, &mock));
}

#[test]
fn a_workspace_inherited_path_dependency_moves_the_digest() {
    let (_t, root, mock) = repo();
    write(
        &root.join("Cargo.toml"),
        "[workspace.dependencies]\ndep = { path = \"crates/dep\" }\n",
    );
    write(
        &mock.join("benches/sample/Cargo.toml"),
        "[dependencies]\ndep.workspace = true\n",
    );
    write(&root.join("crates/dep/src/lib.rs"), "1");
    let before = now(&root, &mock);
    write(&root.join("crates/dep/src/lib.rs"), "2");
    assert_ne!(before, now(&root, &mock));
}

#[test]
fn a_dependency_cycle_ends_and_still_reaches_every_member() {
    let (_t, root, mock) = repo();
    write(
        &mock.join("benches/sample/Cargo.toml"),
        "[dependencies]\na = { path = \"../../../crates/a\" }\n",
    );
    write(
        &root.join("crates/a/Cargo.toml"),
        "[dependencies]\nb = { path = \"../b\" }\n",
    );
    write(
        &root.join("crates/b/Cargo.toml"),
        "[dependencies]\na = { path = \"../a\" }\n",
    );
    let before = now(&root, &mock);
    write(&root.join("crates/b/src/lib.rs"), "changed");
    assert_ne!(
        before,
        now(&root, &mock),
        "the far end of the cycle is an input"
    );
}

#[test]
fn a_directory_that_is_not_a_repository_has_no_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    write(&mock.join("benches/x.rs"), "");
    assert_eq!(digest(tmp.path(), &mock, &mock.join("benches")), None);
}

// ── the record ──

#[test]
fn a_green_record_reads_back_and_a_missing_one_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(recorded(tmp.path()), None);
    record_green(tmp.path(), "abc");
    assert_eq!(recorded(tmp.path()).as_deref(), Some("abc"));
    record_green(tmp.path(), "def");
    assert_eq!(recorded(tmp.path()).as_deref(), Some("def"));
}

/// An edit made while the arms were building must not be recorded as passed.
#[test]
fn only_a_tree_that_did_not_move_during_the_run_is_recorded() {
    assert!(recordable(Some("a"), Some("a")));
    assert!(!recordable(Some("a"), Some("b")));
    assert!(!recordable(None, Some("a")));
    assert!(!recordable(Some("a"), None));
}

// ── the last line of the run ──

#[test]
fn the_summary_names_a_skipped_benches_tree_and_why() {
    let s = summary(0, Some(Skip::Unchanged));
    assert!(s.contains("benches not run"), "{s}");
    assert!(s.contains(Skip::Unchanged.describe()), "{s}");
    let s = summary(2, Some(Skip::OtherCratesNamed));
    assert!(s.contains("2 tree(s) green") && s.contains("-p"), "{s}");
    assert_eq!(summary(3, None), "mock test: 3 tree(s) green");
}

// ── the lockfile: the build's own is output, a committed one is the consumer's ──

fn git(root: &Path, args: &[&str]) {
    let st = Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// A committed lockfile beside an arm is how a consumer picks the framework
/// commit the arms are built at (`bench_rev`), so changing it changes the
/// tree. Only the one the build writes, untracked, is output.
#[test]
fn a_committed_lockfile_under_the_benches_moves_the_digest() {
    for rel in ["benches/Cargo.lock", "benches/sample/arms/a/Cargo.lock"] {
        let (_t, root, mock) = repo();
        write(&mock.join(rel), "# rev one\n");
        git(&root, &["add", "-A"]);
        let before = now(&root, &mock);
        write(&mock.join(rel), "# rev two\n");
        assert_ne!(
            before,
            now(&root, &mock),
            "{rel}: a committed lockfile moved"
        );
    }
}

#[test]
fn a_lockfile_that_is_tracked_and_then_removed_moves_the_digest() {
    let (_t, root, mock) = repo();
    let rel = "benches/sample/arms/a/Cargo.lock";
    write(&mock.join(rel), "# rev one\n");
    git(&root, &["add", "-A"]);
    let before = now(&root, &mock);
    fs::remove_file(mock.join(rel)).unwrap();
    assert_ne!(before, now(&root, &mock));
}

// ── what the generated manifests are made of ──

/// A benches tree whose arm has no manifest, so `mock bench` generates one,
/// with the mockspace the generated crates depend on by path inside the
/// repository.
fn generated_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (t, root, mock) = repo();
    write(
        &mock.join("benches/bench.toml"),
        &format!(
            "[build]\nmockspace = '{{ path = \"{}\" }}'\n",
            root.join("vendor").display()
        ),
    );
    write(
        &mock.join("benches/sample/bench.toml"),
        "title = \"Sample\"\nworkload = \"default\"\narms = [\"a\"]\npoints = [64]\n\
         master_seed = 7\n",
    );
    write(&root.join("vendor/bench-core/src/lib.rs"), "1");
    write(&root.join("vendor/bench-macro/src/lib.rs"), "1");
    (t, root, mock)
}

/// The generated manifest of a manifest-less arm names the framework by path,
/// and nothing on disk under the benches says so.
#[test]
fn a_path_dependency_of_a_generated_arm_manifest_moves_the_digest() {
    for rel in ["vendor/bench-core/src/lib.rs", "vendor/bench-macro/src/lib.rs"] {
        let (_t, root, mock) = generated_repo();
        let before = now(&root, &mock);
        write(&root.join(rel), "2");
        assert_ne!(before, now(&root, &mock), "{rel}");
    }
}

/// `mock bench` writes the manifests it generates under `target/`, so their
/// text is not in any file the listing finds, and a change to what it
/// generates has to move the digest by being one of its entries.
#[test]
fn the_text_of_the_generated_manifests_is_digested() {
    let (_t, root, mock) = generated_repo();
    let entries = entries(&root, &mock, &mock.join("benches")).unwrap();
    let text = |name: &str| {
        entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
    };
    let arm = text("generated/sample/a/Cargo.toml").expect("the arm manifest is an entry");
    assert!(
        arm.contains("crate-type") && arm.contains("vendor/bench-core"),
        "{arm}"
    );
    let driver = text("generated/driver/Cargo.toml").expect("the driver manifest is an entry");
    assert!(driver.contains("mockspace-bench-driver"), "{driver}");
    assert!(
        text("generated/driver/main.rs").is_some(),
        "the driver source is an entry"
    );
}

/// The digest says what it is made of, so one made of something else is not
/// equal to it.
#[test]
fn a_change_of_scheme_moves_the_digest_of_the_same_entries() {
    let e = vec![("a".to_string(), b"1".to_vec())];
    assert_ne!(
        digest_entries_under("1", e.clone()),
        digest_entries_under("2", e.clone())
    );
}

// ── what reads the digest and the record ──

#[test]
fn the_digest_and_the_record_are_read_only_where_the_verdict_needs_them() {
    use std::cell::Cell;
    let reads = |asked, named, record: Option<&str>| {
        let (d, r) = (Cell::new(0), Cell::new(0));
        let v = verdict(
            asked,
            named,
            || {
                d.set(d.get() + 1);
                Some("x".to_string())
            },
            || {
                r.set(r.get() + 1);
                record.map(str::to_string)
            },
        );
        (v, d.get(), r.get())
    };
    // Asked, or named crates: the answer does not depend on either.
    assert_eq!(
        reads(true, false, Some("x")),
        (Verdict::Run(Run::Asked), 0, 0)
    );
    assert_eq!(reads(true, true, None), (Verdict::Run(Run::Asked), 0, 0));
    assert_eq!(
        reads(false, true, Some("y")),
        (Verdict::Skip(Skip::OtherCratesNamed), 0, 0)
    );
    // Otherwise each is read once.
    assert_eq!(
        reads(false, false, Some("x")),
        (Verdict::Skip(Skip::Unchanged), 1, 1)
    );
    assert_eq!(
        reads(false, false, Some("y")),
        (Verdict::Run(Run::Moved), 1, 1)
    );
    assert_eq!(
        reads(false, false, None),
        (Verdict::Run(Run::NeverGreen), 1, 1)
    );
}

#[test]
fn a_tree_whose_inputs_cannot_be_listed_runs_whatever_was_recorded() {
    let v = verdict(false, false, || None, || Some("".to_string()));
    assert_eq!(v, Verdict::Run(Run::NeverGreen));
    let v = verdict(false, false, || None, || Some("x".to_string()));
    assert_eq!(v, Verdict::Run(Run::NeverGreen));
}

// ── the last lines of a failed run ──

#[test]
fn a_failed_run_names_a_skipped_benches_tree_too() {
    let s = failure_summary(1, 3, Some(Skip::OtherCratesNamed));
    assert!(s.contains("1 of 3 tree(s) failed"), "{s}");
    assert!(s.contains("benches not run") && s.contains("-p"), "{s}");
    assert_eq!(
        failure_summary(1, 3, None),
        "mock test: 1 of 3 tree(s) failed"
    );
}
