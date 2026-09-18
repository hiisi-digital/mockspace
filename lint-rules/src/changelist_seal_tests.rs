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
/// homma's `202608150227_changelist.doc.lock.md` on its trunk, whole: a
/// source-only round saying so in its title, which is a statement and not a
/// template.
const DOC_DECLARED_EMPTY: &str = "# Doc changelist: none\n";

/// A repository whose mock dir is its root, which is how most fixtures here are
/// laid out.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("design_rounds")).unwrap();
    // so a repository with no round in it yet still has a first commit
    std::fs::write(dir.path().join("mockspace.toml"), "").unwrap();
    dir
}

fn put(mock: &Path, rel: &str, text: &str) {
    let path = mock.join("design_rounds").join(rel);
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
    if !root.join(".git").exists() {
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "t@example.com"]);
        git(root, &["config", "user.name", "t"]);
    }
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "seed", "--no-gpg-sign"]);
}

fn check_in(mock: &Path, root: &Path) -> Vec<LintError> {
    let crates = BTreeSet::new();
    ChangelistSeal.check_repo(&RepoContext {
        mock_dir:    mock,
        repo_root:   root,
        all_crates:  &crates,
        src_dirs:    &[],
        invocation:  None,
        canon_paths: &[],
        open_panels: &[],
        registry:    &Default::default(),
    })
}

fn check(root: &Path) -> Vec<LintError> {
    check_in(root, root)
}

fn names(errors: &[LintError]) -> Vec<String> {
    let mut v: Vec<String> = errors
        .iter()
        .map(|e| {
            let start = e.message.find('`').unwrap() + 1;
            let end = start + e.message[start ..].find('`').unwrap();
            e.message[start .. end].to_string()
        })
        .collect();
    v.sort();
    v
}

/// homma's round `202609181236` as it was closed: the text in an unlocked
/// `.src.md`, and the lock a bare template opened a minute later.
fn plant_homma_round(mock: &Path, dir: &str) {
    put(
        mock,
        &format!("{dir}/202609181236_changelist.doc.lock.md"),
        DOC,
    );
    put(mock, &format!("{dir}/202609181236_changelist.src.md"), TEXT);
    put(
        mock,
        &format!("{dir}/202609181237_changelist.src.lock.md"),
        TEMPLATE,
    );
}

// ---------------------------------------------------------------------------
// Closed rounds
// ---------------------------------------------------------------------------

#[test]
fn the_homma_close_is_refused_on_both_counts() {
    let dir = repo();
    commit_all(dir.path());
    plant_homma_round(dir.path(), "202609181236");
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
    // A close committed by hand, through the hooks, runs the gate with the
    // moved files staged and not yet in HEAD.
    let dir = repo();
    commit_all(dir.path());
    plant_homma_round(dir.path(), "202609181236");
    git(dir.path(), &["add", "-A"]);
    assert_eq!(check(dir.path()).len(), 2);
}

#[test]
fn the_same_round_once_committed_is_history_and_not_read() {
    let dir = repo();
    plant_homma_round(dir.path(), "202609181236");
    commit_all(dir.path());
    assert!(check(dir.path()).is_empty());
}

#[test]
fn a_stray_added_to_a_committed_round_is_refused_and_its_neighbours_are_not() {
    let dir = repo();
    plant_homma_round(dir.path(), "202609181236");
    commit_all(dir.path());
    put(
        dir.path(),
        "202609181236/202609181300_changelist.doc.md",
        DOC,
    );
    assert_eq!(names(&check(dir.path())), [
        "design_rounds/202609181236/202609181300_changelist.doc.md"
    ]);
}

#[test]
fn a_close_that_shared_its_minute_is_a_round_too() {
    // `close` names the second round in a minute `<stamp>-2`, and homma's trunk
    // carries one.
    for name in ["202609030600-2", "202609030600-10"] {
        let dir = repo();
        commit_all(dir.path());
        plant_homma_round(dir.path(), name);
        assert_eq!(check(dir.path()).len(), 2, "{name}");
    }
}

#[test]
fn an_abandoned_round_may_hold_what_it_never_finished() {
    let dir = repo();
    commit_all(dir.path());
    plant_homma_round(dir.path(), "202609181236-abandoned");
    assert!(check(dir.path()).is_empty());
}

#[test]
fn a_healthy_close_passes() {
    let dir = repo();
    commit_all(dir.path());
    put(
        dir.path(),
        "202609181356/202609181356_changelist.doc.lock.md",
        DOC,
    );
    put(
        dir.path(),
        "202609181356/202609181358_changelist.src.lock.md",
        TEXT,
    );
    assert!(check(dir.path()).is_empty());
}

#[test]
fn a_source_only_round_that_says_so_in_its_title_passes() {
    let dir = repo();
    commit_all(dir.path());
    put(
        dir.path(),
        "202608150227/202608150227_changelist.doc.lock.md",
        DOC_DECLARED_EMPTY,
    );
    put(
        dir.path(),
        "202608150227/202608150228_changelist.src.lock.md",
        TEXT,
    );
    assert!(check(dir.path()).is_empty());
}

#[test]
fn a_deprecated_changelist_may_close_in_a_round_even_when_it_is_empty() {
    let dir = repo();
    commit_all(dir.path());
    put(
        dir.path(),
        "202609181356/202609181356_changelist.doc.lock.md",
        DOC,
    );
    put(
        dir.path(),
        "202609181356/202609181357_changelist.src.deprecated.md",
        TEMPLATE,
    );
    put(
        dir.path(),
        "202609181356/202609181358_changelist.src.lock.md",
        TEXT,
    );
    assert!(check(dir.path()).is_empty());
}

#[test]
fn a_directory_not_named_as_a_round_is_not_one() {
    let dir = repo();
    commit_all(dir.path());
    for name in ["archive", "20260918123", "2026091812360", "20260918123x", "202609181236-2a"] {
        put(
            dir.path(),
            &format!("{name}/202609181236_changelist.src.md"),
            TEXT,
        );
    }
    assert!(check(dir.path()).is_empty());
}

#[test]
fn without_git_every_closed_round_is_judged() {
    // Nothing is shown to be history, so nothing is exempted as history.
    let dir = repo();
    plant_homma_round(dir.path(), "202609181236");
    assert_eq!(check(dir.path()).len(), 2);
}

#[test]
fn before_the_first_commit_every_closed_round_is_judged() {
    let dir = repo();
    git(dir.path(), &["init", "-q"]);
    plant_homma_round(dir.path(), "202609181236");
    assert_eq!(check(dir.path()).len(), 2);
}

/// The real layout: the mock dir a directory under the repository root, and
/// not always named `mock`. What is history has to be read relative to the
/// rounds, whatever sits above them.
#[test]
fn a_mock_dir_under_the_root_reads_its_own_history() {
    for mock_name in ["mock", "linux"] {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let mock = root.join(mock_name);
        std::fs::create_dir_all(mock.join("design_rounds")).unwrap();
        std::fs::write(root.join("mockspace.toml"), "").unwrap();
        plant_homma_round(&mock, "202609181236");
        commit_all(root);
        assert!(check_in(&mock, root).is_empty(), "{mock_name}: committed");

        plant_homma_round(&mock, "202609181400");
        assert_eq!(
            names(&check_in(&mock, root)),
            [
                "design_rounds/202609181400/202609181236_changelist.src.md",
                "design_rounds/202609181400/202609181237_changelist.src.lock.md",
            ],
            "{mock_name}: uncommitted"
        );
    }
}

// ---------------------------------------------------------------------------
// The active round
// ---------------------------------------------------------------------------

#[test]
fn an_empty_lock_in_the_active_round_is_refused_even_when_committed() {
    // The active round can still be unlocked and rewritten, so it is always read.
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
    assert!(
        errors[0]
            .message
            .contains("202609181237_changelist.src.lock.md")
    );
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
    put(
        dir.path(),
        "202609181236_changelist.src.deprecated.md",
        TEXT,
    );
    put(dir.path(), "202609181237_changelist.src.lock.md", TEXT);
    assert!(check(dir.path()).is_empty());
}

#[test]
fn the_messages_name_a_verb_that_exists() {
    let dir = repo();
    put(dir.path(), "202609181236_changelist.doc.lock.md", TEMPLATE);
    put(dir.path(), "202609181236_changelist.doc.md", TEXT);
    let findings = active_round_findings(&dir.path().join("design_rounds"));
    assert_eq!(findings.len(), 2, "{findings:?}");
    for finding in findings {
        assert!(finding.contains("`cargo mock unlock`"), "{finding}");
    }
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
    for text in [
        "",
        "\n\n",
        TEMPLATE,
        "# doc changelist\n\n",
        "   ### indented\n  \n",
        // a colon with nothing after it names nothing
        "# src changelist:\n",
        "# src changelist:   \n",
    ] {
        assert!(is_bare_title(text), "{text:?}");
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
        // a subject in the title is a statement
        DOC_DECLARED_EMPTY,
        "# src changelist: a stub forge reads the body it answers\n",
    ] {
        assert!(!is_bare_title(text), "{text:?}");
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
fn a_round_directory_is_a_stamp_with_at_most_a_numeric_suffix() {
    for name in ["202609181236", "202609181236-2", "202609181236-17"] {
        assert!(is_round_dir(name), "{name:?}");
    }
    for name in [
        "",
        "20260918123",
        "2026091812360",
        "2026091812a6",
        "archive",
        "２０２６０９１８",
        "202609181236-",
        "202609181236-abandoned",
        "202609181236-2a",
        "202609181236-2-3",
        "-202609181236",
    ] {
        assert!(!is_round_dir(name), "{name:?}");
    }
}
