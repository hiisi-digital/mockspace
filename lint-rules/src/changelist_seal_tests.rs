//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::path::Path;
use std::process::Command;

use super::*;

const TEXT: &str = "# src changelist: a stub forge reads the body it answers\n\n\
                    ## CHANGE: fn `status_by_path` FROM headers TO body\n";
const TEMPLATE: &str = "# src changelist\n\n";
const DOC: &str = "# doc changelist\n\n## Changes proposed\n\nNone.\n";

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("design_rounds")).unwrap();
    // so a repository with no round in it yet still has a first commit
    std::fs::write(dir.path().join("mockspace.toml"), "").unwrap();
    dir
}

fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join("design_rounds").join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn commit_all(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@example.com"]);
    git(root, &["config", "user.name", "t"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "seed", "--no-gpg-sign"]);
}

fn check(root: &Path) -> Vec<LintError> {
    let crates = BTreeSet::new();
    ChangelistSeal.check_repo(&RepoContext {
        mock_dir:    root,
        repo_root:   root,
        all_crates:  &crates,
        src_dirs:    &[],
        invocation:  None,
        canon_paths: &[],
        open_panels: &[],
        registry:    &Default::default(),
    })
}

fn names(errors: &[LintError]) -> Vec<String> {
    let mut v: Vec<String> = errors
        .iter()
        .map(|e| {
            let start = e.message.find('`').unwrap() + 1;
            let end = start + e.message[start..].find('`').unwrap();
            e.message[start..end].to_string()
        })
        .collect();
    v.sort();
    v
}

/// homma's round `202609181236` as it was closed: the text in an unlocked
/// `.src.md`, and the lock a template stamped a minute later.
fn plant_homma_round(root: &Path) {
    put(root, "202609181236/202609181236_changelist.doc.lock.md", DOC);
    put(root, "202609181236/202609181236_changelist.src.md", TEXT);
    put(root, "202609181236/202609181237_changelist.src.lock.md", TEMPLATE);
}

// ---------------------------------------------------------------------------
// Closed rounds
// ---------------------------------------------------------------------------

#[test]
fn the_homma_close_is_refused_on_both_counts() {
    let dir = repo();
    commit_all(dir.path());
    plant_homma_round(dir.path());
    let errors = check(dir.path());
    assert_eq!(
        names(&errors),
        [
            "design_rounds/202609181236/202609181236_changelist.src.md",
            "design_rounds/202609181236/202609181237_changelist.src.lock.md",
        ],
        "{errors:?}"
    );
    assert!(errors.iter().all(|e| e.lint_name == LINT_NAME));
}

#[test]
fn the_same_close_staged_is_still_refused() {
    // A close commit runs the gate with the moved files staged, not yet in HEAD.
    let dir = repo();
    commit_all(dir.path());
    plant_homma_round(dir.path());
    git(dir.path(), &["add", "-A"]);
    assert_eq!(check(dir.path()).len(), 2);
}

#[test]
fn the_same_round_once_committed_is_history_and_not_read() {
    let dir = repo();
    plant_homma_round(dir.path());
    commit_all(dir.path());
    assert!(check(dir.path()).is_empty());
}

#[test]
fn a_stray_added_to_a_committed_round_is_refused_and_its_neighbours_are_not() {
    let dir = repo();
    plant_homma_round(dir.path());
    commit_all(dir.path());
    put(dir.path(), "202609181236/202609181300_changelist.doc.md", DOC);
    assert_eq!(
        names(&check(dir.path())),
        ["design_rounds/202609181236/202609181300_changelist.doc.md"]
    );
}

#[test]
fn a_healthy_close_passes() {
    let dir = repo();
    commit_all(dir.path());
    put(dir.path(), "202609181356/202609181356_changelist.doc.lock.md", DOC);
    put(dir.path(), "202609181356/202609181358_changelist.src.lock.md", TEXT);
    assert!(check(dir.path()).is_empty());
}

#[test]
fn a_deprecated_changelist_may_close_in_a_round_even_when_it_is_empty() {
    let dir = repo();
    commit_all(dir.path());
    put(dir.path(), "202609181356/202609181356_changelist.doc.lock.md", DOC);
    put(dir.path(), "202609181356/202609181357_changelist.src.deprecated.md", TEMPLATE);
    put(dir.path(), "202609181356/202609181358_changelist.src.lock.md", TEXT);
    assert!(check(dir.path()).is_empty());
}

#[test]
fn a_directory_not_named_by_a_stamp_is_not_a_round() {
    let dir = repo();
    commit_all(dir.path());
    for name in ["archive", "20260918123", "2026091812360", "20260918123x"] {
        put(dir.path(), &format!("{name}/202609181236_changelist.src.md"), TEXT);
    }
    assert!(check(dir.path()).is_empty());
}

#[test]
fn without_git_every_closed_round_is_judged() {
    // Nothing is shown to be history, so nothing is exempted as history.
    let dir = repo();
    plant_homma_round(dir.path());
    assert_eq!(check(dir.path()).len(), 2);
}

#[test]
fn before_the_first_commit_every_closed_round_is_judged() {
    let dir = repo();
    git(dir.path(), &["init", "-q"]);
    plant_homma_round(dir.path());
    assert_eq!(check(dir.path()).len(), 2);
}

// ---------------------------------------------------------------------------
// The active round
// ---------------------------------------------------------------------------

#[test]
fn an_empty_lock_in_the_active_round_is_refused_even_when_committed() {
    // The active round can still be unlocked and edited, so it is always read.
    for committed in [false, true] {
        let dir = repo();
        put(dir.path(), "202609181236_changelist.doc.lock.md", DOC);
        put(dir.path(), "202609181237_changelist.src.lock.md", TEMPLATE);
        if committed {
            commit_all(dir.path());
        }
        assert_eq!(
            names(&check(dir.path())),
            ["design_rounds/202609181237_changelist.src.lock.md"],
            "committed: {committed}"
        );
    }
}

#[test]
fn an_unlocked_changelist_beside_a_locked_one_of_its_kind_is_refused() {
    // The homma shape one step earlier, before `close` moved it into history.
    let dir = repo();
    put(dir.path(), "202609181236_changelist.doc.lock.md", DOC);
    put(dir.path(), "202609181236_changelist.src.md", TEXT);
    put(dir.path(), "202609181237_changelist.src.lock.md", TEXT);
    let errors = check(dir.path());
    assert_eq!(
        names(&errors),
        ["design_rounds/202609181236_changelist.src.md"],
        "{errors:?}"
    );
    assert!(errors[0].message.contains("202609181237_changelist.src.lock.md"));
}

#[test]
fn every_phase_of_an_ordinary_round_passes() {
    let phases: [&[(&str, &str)]; 4] = [
        // DOC
        &[("202609181236_changelist.doc.md", "")],
        // DRAFT
        &[("202609181236_changelist.doc.lock.md", DOC)],
        // IMPL: a locked doc beside an unlocked src is the phase, not the defect
        &[
            ("202609181236_changelist.doc.lock.md", DOC),
            ("202609181237_changelist.src.md", ""),
        ],
        // CLOSED, not yet moved
        &[
            ("202609181236_changelist.doc.lock.md", DOC),
            ("202609181237_changelist.src.lock.md", TEXT),
        ],
    ];
    for files in phases {
        let dir = repo();
        for (name, text) in files {
            put(dir.path(), name, text);
        }
        assert!(check(dir.path()).is_empty(), "{files:?}");
    }
}

#[test]
fn a_deprecated_changelist_beside_a_lock_of_its_kind_passes() {
    let dir = repo();
    put(dir.path(), "202609181236_changelist.doc.lock.md", DOC);
    put(dir.path(), "202609181236_changelist.src.deprecated.md", TEXT);
    put(dir.path(), "202609181237_changelist.src.lock.md", TEXT);
    assert!(check(dir.path()).is_empty());
}

#[test]
fn no_design_rounds_directory_is_nothing_to_judge() {
    let dir = tempfile::tempdir().unwrap();
    assert!(check(dir.path()).is_empty());
}

// ---------------------------------------------------------------------------
// What counts as saying nothing
// ---------------------------------------------------------------------------

#[test]
fn only_a_bare_title_says_nothing() {
    for text in ["", "\n\n", TEMPLATE, "# doc changelist\n\n", "   ### indented\n  \n"] {
        assert!(is_title_only(text), "{text:?}");
    }
    for text in [
        "None.",
        "# a\nx",
        "# a\n\n  text under it\n",
        "- a bullet\n",
        "`code`",
        "Verification: none\n# trailing heading\n",
        // a second heading is content: a CHANGE heading is a claim by itself
        "# src changelist\n\n## CHANGE: fn `f` FROM a TO b\n",
        "## CHANGE: fn `f` FROM a TO b\n## CHANGE: fn `g` FROM a TO b\n",
    ] {
        assert!(!is_title_only(text), "{text:?}");
    }
}

#[test]
fn a_file_that_cannot_be_read_is_not_called_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!says_nothing(&dir.path().join("absent.md")));
    std::fs::write(dir.path().join("empty.md"), "").unwrap();
    assert!(says_nothing(&dir.path().join("empty.md")));
}

#[test]
fn only_a_twelve_digit_name_is_a_round_stamp() {
    assert!(is_round_stamp("202609181236"));
    for name in ["", "20260918123", "2026091812360", "2026091812a6", "archive", "２０２６０９１８"] {
        assert!(!is_round_stamp(name), "{name:?}");
    }
}
