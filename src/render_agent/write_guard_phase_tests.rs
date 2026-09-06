//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which repository the write guard reads a phase out of.
//!
//! The guard derives the phase from what is tracked under the mock directory's
//! design rounds, and it used to ask git that question from the process cwd. A
//! session sitting at a workspace root and editing a file inside a member
//! repository by absolute path is the ordinary case here, and there the cwd's
//! repository is the workspace, which carries no design rounds at all. Every
//! query came back empty, the phase read TOPIC, and the edit was refused with a
//! message naming a phase the round is not in.
//!
//! That refusal is the worst shape available: it is a true statement about a
//! repository nobody asked about, worded as a true statement about the one they
//! did, and the way out is a `cd` nobody can guess from the message.
//!
//! The arms below build two repositories side by side, one carrying a round in
//! a known phase and one carrying nothing, and run the guard from the second
//! against a file in the first.

use std::process::Command;

use super::*;

fn scratch(tag: &str) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "ms_wgphase_{tag}_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?} failed in {}", dir.display());
}

/// A repository whose mock directory holds the named design-round files,
/// committed, since the guard reads tracked files rather than the working tree.
fn repo_with_rounds(tag: &str, rounds: &[&str]) -> std::path::PathBuf {
    let d = scratch(tag);
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["config", "user.name", "t"]);
    git(&d, &["config", "user.email", "t@example.com"]);
    git(&d, &["config", "commit.gpgsign", "false"]);
    let rounds_dir = d.join("mock/design_rounds");
    std::fs::create_dir_all(&rounds_dir).unwrap();
    for r in rounds {
        std::fs::write(rounds_dir.join(r), "# a round file\n").unwrap();
    }
    std::fs::create_dir_all(d.join("mock/crates/x")).unwrap();
    std::fs::write(d.join("mock/crates/x/DESIGN.md.tmpl"), "# x\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "--no-verify", "-m", "chore: a round"]);
    d
}

fn cfg_at(repo_root: &std::path::Path) -> Config {
    let mut c = Config::from_dir(std::path::Path::new("/nonexistent-mock-dir"));
    c.project_name = "fodder".to_string();
    c.repo_root = repo_root.to_path_buf();
    c.mock_dir = repo_root.join("mock");
    c
}

fn guard_for(repo_root: &std::path::Path) -> String {
    let cfg = cfg_at(repo_root);
    builtin_write_guard(&cfg)
        .replace("{{HOOK_HELPERS}}", crate::render_agent::CLAUDE_HOOK_HELPERS)
        .replace("{{REPO_ROOT}}", &repo_root.display().to_string())
}

/// A Write of `path`, which is how an agent edits a design template.
fn write_payload(path: &std::path::Path) -> String {
    serde_json::json!({
        "session_id": "t",
        "transcript_path": "/tmp/t",
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": path.display().to_string(), "content": "# x\n" }
    })
    .to_string()
}

/// Run the guard from `cwd`, which is the whole variable these arms move.
fn run_from(cwd: &std::path::Path, script: &str, payload: &str) -> String {
    let f = cwd.join(format!("guard_{}.sh", std::process::id()));
    std::fs::write(&f, script).unwrap();
    let out = Command::new("bash")
        .arg(&f)
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut ch| {
            use std::io::Write;
            ch.stdin.as_mut().unwrap().write_all(payload.as_bytes())?;
            ch.wait_with_output()
        })
        .unwrap();
    std::fs::remove_file(&f).ok();
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn refused(out: &str) -> bool {
    out.contains("permissionDecision")
}

/// A repository with no rounds at all, standing in for a workspace root: the
/// place a session actually runs from when it addresses members by path.
fn elsewhere(tag: &str) -> std::path::PathBuf {
    let d = scratch(tag);
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["config", "user.name", "t"]);
    git(&d, &["config", "user.email", "t@example.com"]);
    git(&d, &["config", "commit.gpgsign", "false"]);
    std::fs::write(d.join("a.txt"), "x\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "--no-verify", "-m", "chore: a file"]);
    d
}

#[test]
fn a_design_template_edited_from_another_repository_reads_the_right_phase() {
    // The defect. The round is in DOC, a design template is what DOC exists to
    // let you edit, and the guard is run from a repository that has no rounds.
    let repo = repo_with_rounds("doc", &["202609062251_changelist.doc.md"]);
    let outside = elsewhere("doc-outside");
    let guard = guard_for(&repo);
    let target = repo.join("mock/crates/x/DESIGN.md.tmpl");
    let out = run_from(&outside, &guard, &write_payload(&target));
    assert!(
        !refused(&out),
        "the guard read a phase out of the wrong repository: {out}"
    );
}

#[test]
fn the_same_edit_is_refused_when_the_round_really_is_in_topic() {
    // The half that keeps the arm above from being a bypass. If reading the
    // right repository could only ever permit an edit, the fix would have
    // turned the gate off rather than pointed it at the right tree.
    let repo = repo_with_rounds("topic", &["202609062237_topic.a-thing.md"]);
    let outside = elsewhere("topic-outside");
    let guard = guard_for(&repo);
    let target = repo.join("mock/crates/x/DESIGN.md.tmpl");
    let out = run_from(&outside, &guard, &write_payload(&target));
    assert!(
        refused(&out),
        "a design template was writable with no doc changelist open: {out}"
    );
    assert!(
        out.contains("TOPIC"),
        "the refusal should name the phase it found: {out}"
    );
}

#[test]
fn standing_inside_the_repository_still_works() {
    // The path that was never broken, pinned so pointing the guard at the
    // generated root cannot lose the ordinary case.
    let repo = repo_with_rounds("inside", &["202609062251_changelist.doc.md"]);
    let guard = guard_for(&repo);
    let target = repo.join("mock/crates/x/DESIGN.md.tmpl");
    let out = run_from(&repo, &guard, &write_payload(&target));
    assert!(
        !refused(&out),
        "an edit from inside the repo was refused: {out}"
    );
}

#[test]
fn a_file_in_neither_repository_is_none_of_this_guards_business() {
    // The negative control for the file. Without it every arm above passes on
    // a guard that allows whatever it is handed.
    let repo = repo_with_rounds("unrelated", &["202609062237_topic.a-thing.md"]);
    let outside = elsewhere("unrelated-outside");
    let guard = guard_for(&repo);
    let target = outside.join("a.txt");
    let out = run_from(&outside, &guard, &write_payload(&target));
    assert!(
        !refused(&out),
        "the guard claimed a file outside its own repository: {out}"
    );
}
