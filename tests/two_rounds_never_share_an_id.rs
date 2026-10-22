//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Two rounds opened in one minute on separate branches do not meet in a bare
//! add/add.
//!
//! Every scenario runs the real binary and real git in a scratch repository
//! that `cargo mock` has wired, with `cargo mock` shimmed to this build. The
//! control carries the rest: with the driver's config removed the same two
//! branches conflict with git's own message and nothing about a round, which is
//! what shows the fixture collides at all.

mod common;

use std::fs;

use common::{Repo, write};

const ID: &str = "202610221529";

/// A closed round `id` in the repository, with a tag on the closing commit, the
/// `.meta` differing by `closed` so two of them are not the same file.
fn close_round(r: &Repo, id: &str, closed: &str, tag: bool) {
    let dir = format!("mock/design_rounds/{id}");
    for (name, text) in [
        (format!("{id}_changelist.doc.lock.md"), "# doc\nbody\n".to_string()),
        (format!("{id}_changelist.src.lock.md"), "# src\nbody\n".to_string()),
        (".meta".to_string(), format!("round: {id}\nclosed: {closed}\nabandoned: false\n")),
        (".history".to_string(), format!("{closed} one\n")),
    ] {
        write(&r.root.join(&dir).join(name), &text);
    }
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", &format!("chore: close design round {id}")]);
    // tags are per clone: the other branch's clone has its own, so only one side here
    if tag {
        r.git(&["tag", &format!("round/{id}/end")]);
    }
}

/// `dev` and `a`, each having closed a round with the same id.
fn two_closes() -> Repo {
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    close_round(&r, ID, "aaaa", true);
    r.git(&["switch", "-q", "dev"]);
    close_round(&r, ID, "bbbb", false);
    r
}

fn merge_a(r: &Repo) -> std::process::Output {
    r.git_out(&["merge", "a", "-m", "chore: merge a"])
}

#[test]
fn control_without_the_driver_the_merge_fails_and_says_nothing_of_rounds() {
    let r = two_closes();
    r.git(&["config", "--local", "--remove-section", "merge.mockspace-round"]);
    let merged = merge_a(&r);
    assert!(!merged.status.success(), "the fixture must collide: {}", Repo::failure(&merged));
    assert!(
        !Repo::failure(&merged).contains("round id collision"),
        "{}",
        Repo::failure(&merged)
    );
}

#[test]
fn two_closes_of_one_id_fail_the_merge_naming_the_id_and_the_command() {
    let r = two_closes();
    let merged = merge_a(&r);
    let said = Repo::failure(&merged);
    assert!(!merged.status.success(), "{said}");
    assert!(said.contains(&format!("round id collision: `{ID}` was closed on both branches")), "{said}");
    assert!(said.contains(&format!("cargo mock rename-round {ID}")), "{said}");
}

#[test]
fn renaming_one_side_lets_the_merge_through_with_both_rounds_whole() {
    let r = two_closes();
    r.git(&["switch", "-q", "a"]);
    let out = r
        .cmd("cargo")
        .args(["mock", "rename-round", ID, "--auto-commit"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", Repo::failure(&out));
    r.git(&["switch", "-q", "dev"]);
    let merged = merge_a(&r);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));

    let rounds = r.root.join("mock/design_rounds");
    let mut dirs: Vec<String> = fs::read_dir(&rounds)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    dirs.sort();
    assert_eq!(dirs.len(), 2, "{dirs:?}");
    // the bare id sorts ahead of the same minute's tagged one, as `-` < `_` does for files
    assert_eq!(dirs[0], ID);
    assert_eq!(dirs[1].len(), ID.len() + 5, "the renamed one is tagged: {dirs:?}");
    assert!(dirs[1].starts_with(ID) && dirs[1].as_bytes()[12] == b'-', "{dirs:?}");

    // dev's round is untouched
    let kept = fs::read_to_string(rounds.join(ID).join(".meta")).unwrap();
    assert!(kept.contains("closed: bbbb"), "{kept}");
    // a's round is whole under the new id, its files, `.meta` and history included
    let new = &dirs[1];
    let moved = rounds.join(new);
    for f in [
        format!("{new}_changelist.doc.lock.md"),
        format!("{new}_changelist.src.lock.md"),
        ".meta".to_string(),
        ".history".to_string(),
    ] {
        assert!(moved.join(&f).is_file(), "{f} missing in {new}");
    }
    let meta = fs::read_to_string(moved.join(".meta")).unwrap();
    assert!(meta.starts_with(&format!("round: {new}\n")) && meta.contains("closed: aaaa"), "{meta}");
    assert_eq!(r.status(), "");
}

#[test]
fn the_local_tag_follows_the_rename() {
    let r = two_closes();
    r.git(&["switch", "-q", "a"]);
    let before = r.git(&["rev-parse", &format!("round/{ID}/end")]);
    let out = r
        .cmd("cargo")
        .args(["mock", "rename-round", ID, "--auto-commit"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", Repo::failure(&out));
    let tags = r.git(&["tag", "-l", "round/*"]);
    assert_eq!(tags.lines().count(), 1, "{tags}");
    let tag = tags.trim();
    assert!(tag.starts_with(&format!("round/{ID}-")) && tag.ends_with("/end"), "{tag}");
    assert_eq!(r.git(&["rev-parse", tag]), before, "the tag keeps its object");
}

#[test]
fn round_id_prints_one_tagged_id_and_nothing_else() {
    let r = Repo::new();
    let out = r.cmd("cargo").args(["mock", "round-id"]).output().unwrap();
    assert!(out.status.success(), "{}", Repo::failure(&out));
    let id = String::from_utf8_lossy(&out.stdout).to_string();
    let id = id.strip_suffix('\n').expect("one line");
    assert_eq!(id.len(), 17, "{id}");
    assert!(id[.. 12].bytes().all(|b| b.is_ascii_digit()) && id.as_bytes()[12] == b'-', "{id}");
    assert!(id[13 ..].bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()), "{id}");
}

#[test]
fn a_new_id_names_files_every_reader_takes() {
    // The id the command prints is accepted by the lock, which reads it from the
    // file name: a lock that finds no changelist is a round the id broke.
    let r = Repo::new();
    let out = r.cmd("cargo").args(["mock", "round-id"]).output().unwrap();
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let dr = r.root.join("mock/design_rounds");
    write(&dr.join(format!("{id}_changelist.doc.md")), "# doc changelist: none\n");
    let locked = r.cmd("cargo").args(["mock", "lock"]).output().unwrap();
    assert!(locked.status.success(), "{}", Repo::failure(&locked));
    assert!(dr.join(format!("{id}_changelist.doc.lock.md")).is_file());
}

#[test]
fn the_wiring_is_written_once() {
    let r = Repo::new();
    let attrs = fs::read_to_string(r.root.join(".gitattributes")).unwrap();
    let line = "/mock/design_rounds/*/.meta merge=mockspace-round";
    assert_eq!(attrs.matches(line).count(), 1, "{attrs}");
    let driver = r.git(&["config", "--local", "merge.mockspace-round.driver"]);
    assert!(driver.contains("cargo mock merge-round-meta %O %A %B %P"), "{driver}");
    r.generate();
    r.generate();
    assert_eq!(fs::read_to_string(r.root.join(".gitattributes")).unwrap(), attrs);
    assert_eq!(
        r.git(&["config", "--local", "--get-all", "merge.mockspace-round.driver"])
            .lines()
            .count(),
        1
    );
}
