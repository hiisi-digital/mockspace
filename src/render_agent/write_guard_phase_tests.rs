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
//! The arms below build two repositories side by side and run the guard from
//! one against a target in the other.
//!
//! What each is worth against the code before the fix is worth stating, since
//! the names read wider than that. Only two of them fail there: the one named
//! for the defect, and the command arm. `the_same_edit_is_refused_when_the_
//! round_really_is_in_topic` passes on both, because the old guard found TOPIC
//! in the wrong repository and the new one finds it in the right one, so it
//! controls that the gate still denies something rather than that this is not a
//! bypass. The arm that carries that is the command one, where the phases are
//! opposite in the two repositories and only a guard reading the target's can
//! answer correctly.

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

/// A Bash command that writes, which is the other way the guard is reached and
/// the one that carries no `file_path`.
fn bash_payload(command: &str) -> String {
    serde_json::json!({
        "session_id": "t",
        "transcript_path": "/tmp/t",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": { "command": command, "description": "d" }
    })
    .to_string()
}

#[test]
fn a_command_writing_into_another_repository_is_judged_by_that_repositorys_phase() {
    // The arm the first version of this fix did not have, and the branch where
    // it widened the gate.
    //
    // On the command branch the scope check proves nothing about which
    // repository the write lands in: it accepts a command that merely mentions
    // this hook's root. So a command that mentions this repository and writes
    // into another one passes the scope check, and a root pinned to this hook
    // would read a phase out of a repository the write does not touch. Here
    // this hook's own round is in DOC, where a design template is writable, and
    // the repository actually being written to is in TOPIC, where it is not.
    let mine = repo_with_rounds("cmd-doc", &["202609062251_changelist.doc.md"]);
    let other = repo_with_rounds("cmd-topic", &["202609062237_topic.a-thing.md"]);
    let guard = guard_for(&mine);
    let cmd = format!(
        "grep -r foo {} && sed -i '' s/a/b/ {}/mock/crates/x/DESIGN.md.tmpl",
        mine.display(),
        other.display()
    );
    let out = run_from(&mine, &guard, &bash_payload(&cmd));
    assert!(
        refused(&out),
        "a write into a repository whose round is in TOPIC was allowed: {out}"
    );
    assert!(
        out.contains("TOPIC"),
        "and it should name the phase of the repository being written to: {out}"
    );
}

#[test]
fn a_command_writing_into_this_repository_is_judged_by_this_ones() {
    // The other half. The same shape, with the phases the other way round, so
    // neither arm can pass by the guard simply preferring one repository.
    let mine = repo_with_rounds("cmd-mine-doc", &["202609062251_changelist.doc.md"]);
    let guard = guard_for(&mine);
    let cmd = format!(
        "sed -i '' s/a/b/ {}/mock/crates/x/DESIGN.md.tmpl",
        mine.display()
    );
    let out = run_from(&mine, &guard, &bash_payload(&cmd));
    assert!(
        !refused(&out),
        "a design template was refused on a round in DOC: {out}"
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

const TOPIC: &str = "mock/design_rounds/202609182341_topic.a-thing.md";

#[test]
fn a_topic_staged_and_not_yet_committed_can_still_be_written() {
    // The defect. A topic is staged as soon as it exists, because phase
    // detection reads the index, and the freeze read the same index: the file
    // was refused as committed while it held one heading and no commit had it.
    let repo = repo_with_rounds("staged", &[]);
    std::fs::write(repo.join(TOPIC), "# a-thing\n").unwrap();
    git(&repo, &["add", "--", TOPIC]);
    let guard = guard_for(&repo);
    let out = run_from(&repo, &guard, &write_payload(&repo.join(TOPIC)));
    assert!(
        !refused(&out),
        "a topic no commit holds was refused as frozen: {out}"
    );
}

#[test]
fn a_topic_a_commit_holds_is_frozen() {
    // The control: reading HEAD must still refuse the file once it is in one,
    // or the fix turned the freeze off rather than pointed it at commits.
    let repo = repo_with_rounds("frozen", &["202609182341_topic.a-thing.md"]);
    let guard = guard_for(&repo);
    let out = run_from(&repo, &guard, &write_payload(&repo.join(TOPIC)));
    assert!(refused(&out), "a committed topic was writable: {out}");
    assert!(
        out.contains("committed and FROZEN"),
        "the refusal should be the freeze: {out}"
    );
}

fn payload(tool: &str, input: serde_json::Value) -> String {
    serde_json::json!({
        "session_id": "t",
        "transcript_path": "/tmp/t",
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": input,
    })
    .to_string()
}

/// What `repo_with_rounds` commits into every round file.
const HELD: &str = "# a round file\n";

/// Run one edit of the committed topic and say whether the guard let it by.
fn edit_committed_topic(tag: &str, tool: &str, input: serde_json::Value) -> String {
    let repo = repo_with_rounds(tag, &["202609182341_topic.a-thing.md"]);
    let guard = guard_for(&repo);
    let mut input = input;
    input["file_path"] = serde_json::Value::from(repo.join(TOPIC).display().to_string());
    run_from(&repo, &guard, &payload(tool, input))
}

#[test]
fn a_committed_topic_takes_a_section_appended_by_a_write() {
    // Frozen against rewriting, not against accretion: the discussion comes
    // back to a topic and adds a section below what is there.
    let out = edit_committed_topic(
        "append-write",
        "Write",
        serde_json::json!({ "content": format!("{HELD}\n## The second pass\n") }),
    );
    assert!(!refused(&out), "an append was refused as a rewrite: {out}");
}

#[test]
fn a_committed_topic_takes_an_edit_that_keeps_its_anchor() {
    let out = edit_committed_topic(
        "append-edit",
        "Edit",
        serde_json::json!({
            "old_string": HELD,
            "new_string": format!("{HELD}\n## The second pass\n"),
        }),
    );
    assert!(!refused(&out), "an insertion was refused as a rewrite: {out}");
}

#[test]
fn a_committed_topic_refuses_every_shape_of_rewrite() {
    let cases = [
        ("rw-write", "Write", serde_json::json!({ "content": "# a round file, reworded\n" })),
        ("rw-trunc", "Write", serde_json::json!({ "content": "" })),
        (
            "rw-edit",
            "Edit",
            serde_json::json!({ "old_string": HELD, "new_string": "# reworded\n" }),
        ),
        (
            "rw-empty-anchor",
            "Edit",
            serde_json::json!({ "old_string": "", "new_string": "anything" }),
        ),
        (
            "rw-multi",
            "MultiEdit",
            serde_json::json!({ "edits": [
                { "old_string": HELD, "new_string": format!("{HELD}more\n") },
                { "old_string": "round", "new_string": "ROUND" },
            ]}),
        ),
        ("rw-multi-none", "MultiEdit", serde_json::json!({ "edits": [] })),
        // An insertion keeps its anchor and still lands inside what the commit
        // holds, which is a rewrite whatever the anchor looks like.
        (
            "rw-insert",
            "Edit",
            serde_json::json!({ "old_string": "# a", "new_string": "# a nothing of" }),
        ),
        (
            "rw-replace-all",
            "Edit",
            serde_json::json!({
                "old_string": "file",
                "new_string": "file and more",
                "replace_all": true,
            }),
        ),
        (
            "rw-multi-insert",
            "MultiEdit",
            serde_json::json!({ "edits": [
                { "old_string": "round", "new_string": "round, reworded," },
            ]}),
        ),
        // An anchor the file does not hold is an edit the tool refuses; the
        // guard refuses it rather than guessing where it would have gone.
        (
            "rw-absent-anchor",
            "Edit",
            serde_json::json!({ "old_string": "not in the file", "new_string": "not in the file, more" }),
        ),
    ];
    // Every case is run before anything is asserted, so a guard that lets
    // several through names all of them rather than the first.
    let through: Vec<&str> = cases
        .into_iter()
        .filter(|(tag, tool, input)| {
            let out = edit_committed_topic(tag, tool, input.clone());
            !(refused(&out) && out.contains("committed and FROZEN"))
        })
        .map(|(tag, ..)| tag)
        .collect();
    assert!(through.is_empty(), "rewrites that went through: {through:?}");
}

#[test]
fn a_section_appended_since_the_commit_can_be_edited_and_nothing_above_it() {
    // The file on disk has grown a section the commit does not hold yet.
    // Editing inside that section is the discussion going on, and is let by;
    // the same edit reaching back into the committed text is not.
    let repo = repo_with_rounds("since-commit", &["202609182341_topic.a-thing.md"]);
    let path = repo.join(TOPIC);
    std::fs::write(&path, format!("{HELD}\n## The second pass\n\ndraft\n")).unwrap();
    let guard = guard_for(&repo);
    let edit = |old: &str, new: &str, all: bool| {
        let input = serde_json::json!({
            "file_path": path.display().to_string(),
            "old_string": old,
            "new_string": new,
            "replace_all": all,
        });
        run_from(&repo, &guard, &payload("Edit", input))
    };
    let within = edit("draft\n", "the argument, written out\n", false);
    assert!(!refused(&within), "an edit inside the new section was refused: {within}");
    // `round` sits in the committed line only; `a` sits in both, so replacing
    // every one reaches back.
    for (old, new, all) in [("round", "ROUND", false), ("a", "A", true)] {
        let out = edit(old, new, all);
        assert!(
            refused(&out) && out.contains("committed and FROZEN"),
            "{old:?} -> {new:?} rewrote the committed text: {out}"
        );
    }
}

#[test]
fn a_command_writing_a_committed_topic_is_still_refused() {
    // A command carries no text to compare, so it cannot show it only adds.
    let repo = repo_with_rounds("append-cmd", &["202609182341_topic.a-thing.md"]);
    let guard = guard_for(&repo);
    let cmd = format!("tee -a {}", repo.join(TOPIC).display());
    let out = run_from(&repo, &guard, &bash_payload(&cmd));
    assert!(refused(&out), "a command wrote a committed topic: {out}");
}

#[test]
fn a_topic_in_a_repository_with_no_commit_yet_can_be_written() {
    // No HEAD at all, where asking a commit for the file errors rather than
    // answering empty; the error is read as not committed, which is true.
    let d = scratch("unborn");
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(d.join("mock/design_rounds")).unwrap();
    std::fs::write(d.join(TOPIC), "# a-thing\n").unwrap();
    git(&d, &["add", "--", TOPIC]);
    let guard = guard_for(&d);
    let out = run_from(&d, &guard, &write_payload(&d.join(TOPIC)));
    assert!(
        !refused(&out),
        "a topic in a repository with no commits was refused: {out}"
    );
}
