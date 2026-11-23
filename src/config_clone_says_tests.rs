//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What `clone_says` answers for a root, by what sits at its `.git`.

use std::path::Path;
use std::process::Command;

use super::clone_says;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn clone_with(dir: &Path, value: Option<&str>) {
    git(dir, &["init", "-q"]);
    if let Some(value) = value {
        git(dir, &["config", "--local", "mockspace.autoFmt", value]);
    }
}

#[test]
fn a_clone_answers_what_it_was_set_to_and_nothing_where_it_was_not() {
    for (value, want) in [(Some("false"), Some(false)), (Some("true"), Some(true)), (None, None)] {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        clone_with(dir.path(), value);
        assert_eq!(clone_says(dir.path(), "autoFmt").unwrap(), want, "{value:?}");
    }
}

#[test]
#[should_panic(expected = "does not parse as a boolean")]
fn a_value_that_is_not_a_boolean_is_refused() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    clone_with(dir.path(), Some("sometimes"));
    clone_says(dir.path(), "autoFmt").unwrap();
}

#[test]
fn a_root_without_a_git_marker_has_no_clone_to_ask() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    assert_eq!(clone_says(dir.path(), "autoFmt").unwrap(), None);
}

/// The fixture shape the nuke tests use: an empty `.git` directory, which git
/// does not read as a repository. It used to be read as a clone whose setting
/// would not parse, and panicked.
#[test]
fn an_empty_git_directory_has_no_clone_to_ask() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    assert_eq!(clone_says(dir.path(), "autoFmt").unwrap(), None);
}

/// A `.git` file that names no gitdir is a clone git refuses to read, not the
/// absence of one, so its setting is unreadable and that is loud.
#[test]
#[should_panic(expected = "git cannot read the clone")]
fn a_broken_git_file_is_refused_rather_than_read_as_no_clone() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    std::fs::write(dir.path().join(".git"), "not a gitdir line\n").unwrap();
    clone_says(dir.path(), "autoFmt").unwrap();
}

/// A root whose `.git` git does not recognise, sitting inside a clone that
/// does set the key. Git would answer for the surrounding clone, which is not
/// this root's.
#[test]
fn a_surrounding_clone_does_not_answer_for_a_root_inside_it() {
    let outer = mockspace_manifest::scratch::tempdir().unwrap();
    clone_with(outer.path(), Some("false"));
    let inner = outer.path().join("inner");
    std::fs::create_dir_all(inner.join(".git")).unwrap();
    assert_eq!(clone_says(&inner, "autoFmt").unwrap(), None);
    // the control: the outer clone does answer for itself
    assert_eq!(clone_says(outer.path(), "autoFmt").unwrap(), Some(false));
}

/// The same clone read by the whole config: a refusal the caller reports, so a
/// command stopped by a git call that fails names the call and never panics,
/// as a migration re-reading its config part way does.
#[test]
fn a_clone_git_cannot_read_refuses_the_config_without_a_panic() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    std::fs::write(dir.path().join(".git"), "not a gitdir line\n").unwrap();
    let mock = dir.path().join("mock");
    std::fs::create_dir_all(&mock).unwrap();
    std::fs::write(mock.join("mockspace.toml"), "").unwrap();
    let why = match crate::config::Config::try_from_dir(&mock) {
        Ok(_) => panic!("a clone git cannot read is read as a config"),
        Err(why) => why,
    };
    assert!(why.contains("git cannot read the clone"), "{why}");
    // the control: the same tree with no `.git` reads
    std::fs::remove_file(dir.path().join(".git")).unwrap();
    assert!(crate::config::Config::try_from_dir(&mock).is_ok());
}
