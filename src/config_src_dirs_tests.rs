//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A `src_dirs` entry names a directory under the mock directory, and one that
//! has a `..` in it is refused when the config is read.

use std::path::Path;

use super::Config;

/// The text of the panic a load ended in, or `None` when it went through.
fn refusal_of(mock: &Path) -> Option<String> {
    let dir = mock.to_path_buf();
    let caught = std::panic::catch_unwind(move || {
        let _ = Config::from_dir(&dir);
    });
    caught.err().map(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
            .unwrap_or_default()
    })
}

/// A repo whose `mock` directory holds `crates`, an `outside` beside the
/// repository, and a `mockspace.toml` declaring `entries`.
fn repo_declaring(entries: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("repo/mock");
    std::fs::create_dir_all(mock.join("crates")).unwrap();
    std::fs::create_dir_all(mock.join("a/b")).unwrap();
    std::fs::create_dir_all(tmp.path().join("outside")).unwrap();
    std::fs::create_dir_all(tmp.path().join("repo/.git")).unwrap();
    std::fs::write(
        mock.join("mockspace.toml"),
        format!("src_dirs = [{entries}]\n"),
    )
    .unwrap();
    tmp
}

#[test]
fn a_directory_with_a_parent_segment_is_refused() {
    for entry in ["\"../../outside\"", "\"crates\", \"../x\"", "\"a/../crates\""] {
        let tmp = repo_declaring(entry);
        let msg = refusal_of(&tmp.path().join("repo/mock"))
            .unwrap_or_else(|| panic!("{entry} loaded, and the hooks cannot address it"));
        assert!(msg.contains("src_dirs"), "{entry}: names the key: {msg}");
        assert!(msg.contains(".."), "{entry}: names the offence: {msg}");
        assert!(msg.contains("relative to the mock dir"), "{entry}: says the fix: {msg}");
        assert!(msg.contains("matches none"), "{entry}: says why: {msg}");
        // `a/../crates` stays inside the mock dir, so the message cannot say the
        // entry leaves it.
        assert!(!msg.contains("climb"), "{entry}: claims a climb: {msg}");
        // The continuations are escaped, so no line is blank or carries a
        // trailing space.
        for line in msg.lines() {
            assert!(!line.trim().is_empty(), "{entry}: blank line in: {msg:?}");
            assert_eq!(line, line.trim_end(), "{entry}: trailing space in: {msg:?}");
        }
    }
}

/// The control: a nested entry with no `..` in it loads, so the refusal above
/// is about the `..` and not about the entry having a slash.
#[test]
fn a_nested_directory_under_the_mock_directory_loads() {
    let tmp = repo_declaring("\"crates\", \"a/b\"");
    assert_eq!(refusal_of(&tmp.path().join("repo/mock")), None);
}

/// Every spelling that is not the path git reports is refused at load, each
/// with the reason that applies, and an absolute path is one of them: joined
/// onto the mock dir it replaces it, so the source would sit outside the tree
/// the hooks scope.
#[test]
fn a_spelling_git_would_not_report_is_refused_and_says_which() {
    let cases = [
        ("\"./crates\"", "starts with `.`"),
        ("\"crates\", \"a/./b\"", "`//` or `/./`"),
        ("\"crates//x\"", "`//` or `/./`"),
        ("\".\"", "starts with `.`"),
        ("\"\"", "is empty"),
        ("\"/tmp/elsewhere\"", "absolute path"),
        ("\"/\"", "absolute path"),
    ];
    for (entry, reason) in cases {
        let tmp = repo_declaring(entry);
        let msg = refusal_of(&tmp.path().join("repo/mock"))
            .unwrap_or_else(|| panic!("{entry} loaded, and git reports no such path"));
        assert!(msg.contains("src_dirs"), "{entry}: names the key: {msg}");
        assert!(msg.contains(reason), "{entry}: says {reason:?}: {msg}");
        assert!(msg.contains("matches none"), "{entry}: says why: {msg}");
    }
}

/// The control: a trailing slash is the one tolerated variation, since every
/// consumer already trims it.
#[test]
fn a_trailing_slash_still_loads() {
    let tmp = repo_declaring("\"crates/\", \"a/b/\"");
    assert_eq!(refusal_of(&tmp.path().join("repo/mock")), None);
}
