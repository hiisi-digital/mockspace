//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `lock` and `close` refusing an unsealed round themselves, which is the only
//! guard a hook-less `--auto-commit` passes through, and what the commit they
//! make leaves behind.

use std::path::Path;
use std::process::ExitCode;

use crate::config::Config;
use crate::design_round::*;

const OPTS: SubcmdOpts = SubcmdOpts {
    auto_commit: false,
};

fn mock(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("design_rounds")).unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join("design_rounds").join(name), text).unwrap();
    }
    dir
}

fn listed(mock: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(mock.join("design_rounds"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

#[test]
fn lock_refuses_a_bare_title_in_either_window_and_moves_nothing() {
    let cases: [&[(&str, &str)]; 2] =
        [&[("202609181543_changelist.doc.md", "# doc changelist\n\n")], &[
            (
                "202609181543_changelist.doc.lock.md",
                "# doc changelist\n\nNone.\n",
            ),
            ("202609181544_changelist.src.md", "# src changelist\n\n"),
        ]];
    for files in cases {
        let dir = mock(files);
        let before = listed(dir.path());
        let code = cmd_lock(&Config::from_dir(dir.path()), &OPTS);
        assert_eq!(code, ExitCode::FAILURE, "{files:?}");
        assert_eq!(listed(dir.path()), before, "{files:?}");
    }
}

#[test]
fn lock_locks_a_changelist_that_says_something() {
    let dir = mock(&[("202609181543_changelist.doc.md", "# doc changelist: none\n")]);
    let code = cmd_lock(&Config::from_dir(dir.path()), &OPTS);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(listed(dir.path()), ["202609181543_changelist.doc.lock.md"]);
}

#[test]
fn close_refuses_an_empty_lock_and_moves_nothing() {
    // CLOSED by phase, with the src lock a bare template: the state an
    // auto-committed close would otherwise carry into history unread.
    let dir = mock(&[
        (
            "202609181236_changelist.doc.lock.md",
            "# doc changelist\n\nNone.\n",
        ),
        (
            "202609181237_changelist.src.lock.md",
            "# src changelist\n\n",
        ),
    ]);
    let before = listed(dir.path());
    let code = cmd_close(&Config::from_dir(dir.path()), &OPTS);
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(listed(dir.path()), before);
}

// --- a doc lock and the templates it freezes ------------------------------

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed in {}", dir.display());
}

const DOC_CL: &str = "202609190100_changelist.doc.md";

/// A repository in DOC, with a doc changelist that says something and one
/// crate template, everything committed. Returns the repo and its mock dir.
fn repo_in_doc() -> (tempfile::TempDir, std::path::PathBuf) {
    let repo = tempfile::tempdir().unwrap();
    let mock = repo.path().join("mock");
    std::fs::create_dir_all(mock.join("design_rounds")).unwrap();
    std::fs::create_dir_all(mock.join("crates/x")).unwrap();
    std::fs::write(
        mock.join("design_rounds").join(DOC_CL),
        "# doc changelist: x\n",
    )
    .unwrap();
    std::fs::write(mock.join("crates/x/DESIGN.md.tmpl"), "# x\n").unwrap();
    std::fs::write(mock.join("crates/x/SHAME.md.tmpl"), "# shame\n").unwrap();
    std::fs::write(mock.join("README.md.tmpl"), "# readme\n").unwrap();
    git(repo.path(), &["init", "-q", "-b", "dev"]);
    git(repo.path(), &["config", "user.name", "t"]);
    git(repo.path(), &["config", "user.email", "t@example.com"]);
    git(repo.path(), &["config", "commit.gpgsign", "false"]);
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &[
        "commit",
        "-q",
        "--no-verify",
        "-m",
        "chore: a round",
    ]);
    (repo, mock)
}

fn lock_in(mock: &Path) -> (ExitCode, Vec<String>, Vec<String>) {
    let before = listed(mock);
    let code = cmd_lock(&Config::from_dir(mock), &OPTS);
    (code, before, listed(mock))
}

#[test]
fn a_doc_lock_goes_through_when_every_template_is_committed() {
    // The control. Without it every refusal below passes on a lock that
    // refuses whatever it is handed.
    let (_repo, mock) = repo_in_doc();
    let (code, _, after) = lock_in(&mock);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(after, ["202609190100_changelist.doc.lock.md"]);
}

#[test]
fn a_doc_lock_is_refused_over_a_template_edited_and_not_committed() {
    let (_repo, mock) = repo_in_doc();
    std::fs::write(mock.join("crates/x/DESIGN.md.tmpl"), "# x, edited\n").unwrap();
    let (code, before, after) = lock_in(&mock);
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(after, before, "a refused lock moved a file");
}

#[test]
fn a_doc_lock_is_refused_over_a_template_staged_and_not_committed() {
    let (repo, mock) = repo_in_doc();
    std::fs::write(mock.join("crates/x/DESIGN.md.tmpl"), "# x, staged\n").unwrap();
    git(repo.path(), &["add", "mock/crates/x/DESIGN.md.tmpl"]);
    let (code, before, after) = lock_in(&mock);
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(after, before);
}

#[test]
fn a_doc_lock_is_refused_over_a_new_template_nobody_added() {
    let (_repo, mock) = repo_in_doc();
    std::fs::write(mock.join("crates/x/DEEPDIVE_new.md.tmpl"), "# new\n").unwrap();
    let (code, before, after) = lock_in(&mock);
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(after, before);
}

#[test]
fn what_the_doc_gate_leaves_open_does_not_hold_the_lock() {
    // SHAME is writable in every phase, and a template outside the source
    // directories is not gated at all, so neither can be stranded.
    let (_repo, mock) = repo_in_doc();
    std::fs::write(mock.join("crates/x/SHAME.md.tmpl"), "# shame, edited\n").unwrap();
    std::fs::write(mock.join("README.md.tmpl"), "# readme, edited\n").unwrap();
    let (code, _, after) = lock_in(&mock);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(after, ["202609190100_changelist.doc.lock.md"]);
}

#[test]
fn a_src_lock_does_not_ask_about_templates() {
    // A dirty template in IMPL is the doc gate's to refuse at commit; the
    // src lock ends a different window and is not where it is caught.
    let (repo, mock) = repo_in_doc();
    let dr = mock.join("design_rounds");
    std::fs::rename(
        dr.join(DOC_CL),
        dr.join("202609190100_changelist.doc.lock.md"),
    )
    .unwrap();
    std::fs::write(
        dr.join("202609190101_changelist.src.md"),
        "# src changelist: x\n",
    )
    .unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &[
        "commit",
        "-q",
        "--no-verify",
        "-m",
        "chore: into impl",
    ]);
    std::fs::write(mock.join("crates/x/DESIGN.md.tmpl"), "# x, edited\n").unwrap();
    let (code, _, after) = lock_in(&mock);
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(
        after.contains(&"202609190101_changelist.src.lock.md".to_string()),
        "{after:?}"
    );
}

/// homma's shape itself: the text unlocked beside a lock of its kind.
/// Refused whichever check reaches it first, the phase or the seal, and
/// what matters is that nothing moves.
#[test]
fn close_refuses_the_homma_shape_and_moves_nothing() {
    let dir = mock(&[
        (
            "202609181236_changelist.doc.lock.md",
            "# doc changelist\n\nNone.\n",
        ),
        (
            "202609181236_changelist.src.md",
            "# src changelist: x\n\n## CHANGE: y\n",
        ),
        (
            "202609181237_changelist.src.lock.md",
            "# src changelist\n\n",
        ),
    ]);
    let before = listed(dir.path());
    let code = cmd_close(&Config::from_dir(dir.path()), &OPTS);
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(listed(dir.path()), before);
}

// --- a committed transition leaves the index true ---------------------------

const AUTO: SubcmdOpts = SubcmdOpts {
    auto_commit: true,
};

fn status(repo: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// What the last commit did to which paths, one `STATUS<tab>path` line each,
/// with renames left as a delete and an add so the old and new names both show.
fn last_commit(repo: &Path) -> Vec<String> {
    let out = std::process::Command::new("git")
        .args(["log", "-1", "--no-renames", "--name-status", "--format="])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

fn head(repo: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn an_auto_committed_lock_leaves_git_status_empty() {
    // The commit is built through a temporary index, so the real one still
    // held the old layout and `git status` showed the lock as a staged
    // deletion beside an untracked file. A pre-push check reading the index
    // then saw the locked changelist as modified.
    let (repo, mock) = repo_in_doc();
    assert_eq!(status(repo.path()), "", "the fixture starts clean");
    let before = head(repo.path());
    let code = cmd_lock(&Config::from_dir(&mock), &AUTO);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(status(repo.path()), "");
    // a lock that committed nothing would also leave the status empty
    assert_ne!(head(repo.path()), before, "the lock made a commit");
    assert_eq!(
        last_commit(repo.path()),
        [
            "A\tmock/design_rounds/202609190100_changelist.doc.lock.md".to_string(),
            format!("D\tmock/design_rounds/{DOC_CL}"),
        ],
        "the commit is the round's own files moving and nothing else"
    );
}

#[test]
fn an_auto_committed_close_leaves_git_status_empty() {
    let (repo, mock) = repo_in_doc();
    let dr = mock.join("design_rounds");
    std::fs::write(
        dr.join("202609190101_changelist.src.md"),
        "# src changelist: x\n",
    )
    .unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &[
        "commit",
        "-q",
        "--no-verify",
        "-m",
        "chore: into impl",
    ]);
    // a lock for each window, then the close
    assert_eq!(cmd_lock(&Config::from_dir(&mock), &AUTO), ExitCode::SUCCESS);
    assert_eq!(cmd_lock(&Config::from_dir(&mock), &AUTO), ExitCode::SUCCESS);
    assert_eq!(status(repo.path()), "", "after the two locks");
    let before = head(repo.path());
    assert_eq!(
        cmd_close(&Config::from_dir(&mock), &AUTO),
        ExitCode::SUCCESS
    );
    assert_eq!(status(repo.path()), "", "after the close");
    assert_ne!(head(repo.path()), before, "the close made a commit");
    assert_eq!(
        last_commit(repo.path()),
        [
            "A\tmock/design_rounds/202609190100/.history",
            "A\tmock/design_rounds/202609190100/.meta",
            "A\tmock/design_rounds/202609190100/202609190100_changelist.doc.lock.md",
            "A\tmock/design_rounds/202609190100/202609190101_changelist.src.lock.md",
            "D\tmock/design_rounds/202609190100_changelist.doc.lock.md",
            "D\tmock/design_rounds/202609190101_changelist.src.lock.md",
        ],
        "the two locked changelists left the flat directory for the round's own, and nothing else moved"
    );
}

#[test]
fn a_committed_transition_keeps_unrelated_staged_work_staged() {
    // Only the paths the transition committed are brought to the new HEAD.
    // Somebody's staged edit elsewhere is theirs and stays staged.
    let (repo, mock) = repo_in_doc();
    std::fs::write(mock.join("README.md.tmpl"), "# readme, staged\n").unwrap();
    git(repo.path(), &["add", "mock/README.md.tmpl"]);
    assert_eq!(cmd_lock(&Config::from_dir(&mock), &AUTO), ExitCode::SUCCESS);
    assert_eq!(status(repo.path()), "M  mock/README.md.tmpl\n");
}

// A stub changelist beside a locked one of its kind (a bare title, or a draft nobody went on with)
// holds the round open: `changelist-seal` refuses the lock, `changelist-immutability` refuses
// deleting an unlocked changelist outside its window, and `deprecate` answered "both CLs locked" in
// the phase the stub made, so the only way out was unlock, relock and a fresh changelist
// (kaski, 30 October).

const DOC_LOCK: &str = "202610301702_changelist.doc.lock.md";
const SRC_LOCK: &str = "202610301702_changelist.src.lock.md";
const STUB: &str = "202610301710_changelist.src.md";
const STUB_DEPRECATED: &str = "202610301710_changelist.src.deprecated.md";

#[test]
fn deprecate_retires_a_stub_beside_a_locked_changelist_of_its_kind() {
    let dir = mock(&[
        (DOC_LOCK, "# doc changelist: x\n"),
        (SRC_LOCK, "# src changelist: x\n"),
        (STUB, "# src changelist\n\n"),
    ]);
    let code = cmd_deprecate(&Config::from_dir(dir.path()), &OPTS);
    assert_eq!(code, ExitCode::SUCCESS);
    assert_eq!(
        listed(dir.path()),
        [DOC_LOCK, SRC_LOCK, STUB_DEPRECATED],
        "the stub is deprecated and the round's own locks are untouched"
    );
}

#[test]
fn deprecate_with_a_stub_leaves_the_round_sealed_enough_to_close() {
    let dir = mock(&[
        (DOC_LOCK, "# doc changelist: x\n"),
        (SRC_LOCK, "# src changelist: x\n"),
        (STUB, "# src changelist\n\n"),
    ]);
    assert_eq!(
        cmd_close(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::FAILURE,
        "the stub keeps the round from closing"
    );
    assert_eq!(
        cmd_deprecate(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(
        cmd_close(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS,
        "once the stub is deprecated the round closes"
    );
}

#[test]
fn deprecate_without_a_stub_in_a_closed_round_still_refuses() {
    let dir = mock(&[
        (DOC_LOCK, "# doc changelist: x\n"),
        (SRC_LOCK, "# src changelist: x\n"),
    ]);
    assert_eq!(
        cmd_deprecate(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::FAILURE
    );
    assert_eq!(listed(dir.path()), [DOC_LOCK, SRC_LOCK]);
}
