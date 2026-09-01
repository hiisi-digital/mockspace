//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The walk over real git repositories, kept apart from the unit tests on
//! `SrcLayout` itself.
//!
//! What is being tested here is what git reports rather than what this module
//! computes, so every one of these builds a repository and asks. That is the
//! seam the split follows, and it is what keeps `src_layout.rs` under the size
//! a reader can hold.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{SrcLayout, changed_files};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git is on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Twenty numbered lines, so a later edit can land anywhere on the similarity
/// scale rather than only at the two ends of it.
fn twenty_lines() -> String {
    (0 .. 20).map(|n| format!("pub fn f{n}() {{}}\n")).collect()
}

/// A repository with one committed file at `crates/old/thing.rs`.
///
/// Every setting the walks read is pinned rather than inherited. An ambient
/// `commit.gpgsign` fails the seed commit outright, and an ambient
/// `diff.renames` is what let the first version of this carve-out pass its own
/// test while being broken: the walk and the rename query read that setting
/// differently, so a test inheriting it only ever exercises whichever way the
/// machine happens to be configured.
fn repo_with_a_committed_file() -> tempfile::TempDir {
    repo_configured(&[])
}

fn repo_configured(extra: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temp dir");
    let root = dir.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.invalid"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "diff.renames", "true"]);
    for (k, v) in extra {
        git(root, &["config", k, v]);
    }
    std::fs::create_dir_all(root.join("crates/old")).unwrap();
    std::fs::write(root.join("crates/old/thing.rs"), twenty_lines()).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "seed"]);
    dir
}

fn changed(root: &Path) -> Vec<(String, String)> {
    changed_in(root, &["crates"], |f| f.ends_with(".rs"))
}

fn changed_in(root: &Path, dirs: &[&str], keep: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let abs: Vec<PathBuf> = dirs.iter().map(|d| root.join(d)).collect();
    let layout = SrcLayout::new(root, &abs);
    changed_files(root, &layout, keep)
}

fn move_to_new(root: &Path, name: &str) {
    std::fs::create_dir_all(root.join("crates/new")).unwrap();
    git(root, &[
        "mv",
        &format!("crates/old/{name}"),
        &format!("crates/new/{name}"),
    ]);
}

#[test]
fn a_file_that_only_moved_is_not_a_change() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    move_to_new(root, "thing.rs");

    assert_eq!(
        changed(root),
        Vec::new(),
        "a rename with identical content was reported as an edit, which is what \
         makes a crate impossible to rename under the phase gates"
    );
}

/// The same, with rename detection turned off in the repository.
///
/// This is the case that broke the first version. It asked git for renames with
/// `-M100%` in one invocation and for changed names with no `-M` in another, so
/// the two disagreed here: the walk reported a delete and an add, the query
/// still paired them, and the gate blocked on a path that no longer existed.
#[test]
fn a_rename_is_not_a_change_even_where_the_config_disables_rename_detection() {
    let dir = repo_configured(&[("diff.renames", "false")]);
    let root = dir.path();
    move_to_new(root, "thing.rs");

    assert_eq!(
        changed(root),
        Vec::new(),
        "the walk and the rename check read `diff.renames` differently"
    );
}

/// The other half of that break, and the one that mattered more.
///
/// The gate is global rather than staged-only on purpose, so an unstaged edit
/// blocks the commit too. One rename set shared between the two views let a
/// staged rename swallow the unstaged edit to the same file, which puts that
/// hole straight back.
#[test]
fn a_staged_rename_does_not_hide_an_unstaged_edit_to_the_same_file() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    move_to_new(root, "thing.rs");
    std::fs::write(root.join("crates/new/thing.rs"), "pub fn edited() {}\n").unwrap();

    let found = changed(root);
    assert!(
        found
            .iter()
            .any(|(f, s)| f == "crates/new/thing.rs" && s == "unstaged"),
        "an unstaged edit hid behind a staged rename of the same file: {found:?}"
    );
}

/// The control. Without it the cases above pass against an implementation that
/// reports nothing at all, which is the shape a broken walk has too.
#[test]
fn a_file_that_moved_and_changed_is_still_a_change() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    move_to_new(root, "thing.rs");
    std::fs::write(root.join("crates/new/thing.rs"), "pub fn a() {}\n").unwrap();
    git(root, &["add", "-A"]);

    let found = changed(root);
    assert!(
        found.iter().any(|(f, _)| f == "crates/new/thing.rs"),
        "content that moved and changed escaped the gate: {found:?}"
    );
}

/// And the same in the band that decides whether the threshold is doing
/// anything.
///
/// A file rewritten wholesale is refused by any threshold, so a test over one
/// says nothing about which was set. Three lines added to twenty is around
/// `R085`: the default detection calls it a rename and `-M100%` refuses it, so
/// this fails the moment the threshold is loosened and passes only while it is
/// exact.
#[test]
fn a_mostly_unchanged_file_that_moved_is_still_a_change() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    move_to_new(root, "thing.rs");
    let mut text = twenty_lines();
    text.push_str("pub fn extra_a() {}\npub fn extra_b() {}\npub fn extra_c() {}\n");
    std::fs::write(root.join("crates/new/thing.rs"), text).unwrap();
    git(root, &["add", "-A"]);

    let found = changed(root);
    assert!(
        found.iter().any(|(f, _)| f == "crates/new/thing.rs"),
        "a threshold below 100% let an edited file through as a rename: {found:?}"
    );
}

/// And the ordinary case, so the carve-out is not swallowing edits that never
/// moved anywhere.
#[test]
fn a_file_edited_in_place_is_still_a_change() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    std::fs::write(root.join("crates/old/thing.rs"), "pub fn changed() {}\n").unwrap();

    let found = changed(root);
    assert!(
        found.iter().any(|(f, _)| f == "crates/old/thing.rs"),
        "an in-place edit escaped the gate: {found:?}"
    );
}

/// The doc side, which is the whole motivation: a crate directory carries
/// templates as well as source, and it is the doc gate that refuses the
/// templates in every phase the source gate permits.
#[test]
fn a_doc_template_that_only_moved_is_not_a_change_either() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    std::fs::write(root.join("crates/old/README.md.tmpl"), "a crate.\n").unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "a template"]);
    move_to_new(root, "README.md.tmpl");

    assert_eq!(
        changed_in(root, &["crates"], |f| f.ends_with(".md.tmpl")),
        Vec::new(),
        "the doc gate would still refuse a crate rename"
    );
}

/// A move between two declared source directories, which is the shape this
/// module exists to support at all.
#[test]
fn a_move_between_two_declared_source_directories_is_not_a_change() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    std::fs::create_dir_all(root.join("tools/t")).unwrap();
    git(root, &["mv", "crates/old/thing.rs", "tools/t/thing.rs"]);

    assert_eq!(
        changed_in(root, &["crates", "tools"], |f| f.ends_with(".rs")),
        Vec::new(),
        "a rename across two declared source directories was read as an edit"
    );
}

/// A move out of every declared directory is a deletion and stays gated,
/// because what left the layout is not something the layout stopped caring
/// about.
#[test]
fn a_move_out_of_the_layout_is_still_a_change() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    std::fs::create_dir_all(root.join("elsewhere")).unwrap();
    git(root, &["mv", "crates/old/thing.rs", "elsewhere/thing.rs"]);

    let found = changed(root);
    assert!(
        found.iter().any(|(f, _)| f == "crates/old/thing.rs"),
        "source left the guarded layout without the gate noticing: {found:?}"
    );
}

/// A copy is not a rename, however identical the two are. The original stays
/// where it was and the content is declared a second time, which is a new
/// declaration and stays gated.
#[test]
fn a_copy_is_still_a_change() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    std::fs::create_dir_all(root.join("crates/new")).unwrap();
    std::fs::copy(
        root.join("crates/old/thing.rs"),
        root.join("crates/new/thing.rs"),
    )
    .unwrap();
    git(root, &["add", "-A"]);

    let found = changed(root);
    assert!(
        found.iter().any(|(f, _)| f == "crates/new/thing.rs"),
        "a second copy of the content was read as a move: {found:?}"
    );
}

/// What a shell `mv` does, which is not what `git mv` does.
///
/// git has never seen the destination, so it arrives as an untracked add with
/// no status to read, while the source arrives as a delete. Neither side can be
/// paired, so both are reported and the carve-out does not apply. Staging the
/// move, or using `git mv`, is what puts it in front of the check.
#[test]
#[ignore = "catalogue: an unstaged move is an untracked add plus a delete, \
            which carries no rename information for the carve-out to read"]
fn a_move_that_was_never_staged_is_also_not_a_change() {
    let dir = repo_with_a_committed_file();
    let root = dir.path();
    std::fs::create_dir_all(root.join("crates/new")).unwrap();
    std::fs::rename(
        root.join("crates/old/thing.rs"),
        root.join("crates/new/thing.rs"),
    )
    .unwrap();

    assert_eq!(changed(root), Vec::new());
}
