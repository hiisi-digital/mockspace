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
        let dir = tempfile::tempdir().unwrap();
        clone_with(dir.path(), value);
        assert_eq!(clone_says(dir.path(), "autoFmt"), want, "{value:?}");
    }
}

#[test]
#[should_panic(expected = "does not parse as a boolean")]
fn a_value_that_is_not_a_boolean_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    clone_with(dir.path(), Some("sometimes"));
    let _ = clone_says(dir.path(), "autoFmt");
}

#[test]
fn a_root_without_a_git_marker_has_no_clone_to_ask() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(clone_says(dir.path(), "autoFmt"), None);
}

/// The fixture shape the nuke tests use: a `.git` git does not recognise. It
/// used to be read as a clone whose setting would not parse, and panicked.
#[test]
fn a_git_marker_git_does_not_recognise_has_no_clone_to_ask() {
    for marker_is_dir in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        if marker_is_dir {
            std::fs::create_dir(dir.path().join(".git")).unwrap();
        } else {
            std::fs::write(dir.path().join(".git"), "not a gitdir line\n").unwrap();
        }
        assert_eq!(
            clone_says(dir.path(), "autoFmt"),
            None,
            "dir: {marker_is_dir}"
        );
    }
}

/// A root whose `.git` git does not recognise, sitting inside a clone that
/// does set the key. Git would answer for the surrounding clone, which is not
/// this root's.
#[test]
fn a_surrounding_clone_does_not_answer_for_a_root_inside_it() {
    let outer = tempfile::tempdir().unwrap();
    clone_with(outer.path(), Some("false"));
    let inner = outer.path().join("inner");
    std::fs::create_dir_all(inner.join(".git")).unwrap();
    assert_eq!(clone_says(&inner, "autoFmt"), None);
    // the control: the outer clone does answer for itself
    assert_eq!(clone_says(outer.path(), "autoFmt"), Some(false));
}
