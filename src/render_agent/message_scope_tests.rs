//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which repository a message gate is about.
//!
//! A shell sits in one repository and commits in another all the time, because
//! addressing a repository by absolute path is what `git -C` is for. The gate
//! decided that question on the process cwd alone, so the repository the shell
//! happened to be standing in claimed every commit the shell issued, and having
//! claimed one it linted the entire serialised tool input as though that were
//! the subject line.
//!
//! The arms below are about that decision and stop there. Scoping out is an
//! allow taken before the gate ever looks for a launcher, so the two outcomes
//! stay distinguishable on a machine that has no `mock` installed: out of scope
//! is silence, in scope is a refusal, and which refusal it is does not matter
//! to anything here.
//!
//! What these arms catch as a set is worth stating, because their names read
//! wider than their coverage. Run against the code before the fix, only the two
//! resting on the resolved-path half fail; the arm named for the headline
//! defect passes there, because the unresolved symlink was allowing everything
//! from that directory anyway and the right answer arrived for the wrong
//! reason. The control that actually carries the repository half is the one in
//! the commit that introduced it: disable the branch and exactly that arm
//! fails.

use std::process::Command;

use super::*;

fn scratch(tag: &str) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "ms_msgscope_{tag}_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cfg_at(repo_root: &std::path::Path) -> Config {
    let mut c = Config::from_dir(std::path::Path::new("/nonexistent-mock-dir"));
    c.project_name = "fodder".to_string();
    c.repo_root = repo_root.to_path_buf();
    c.mock_dir = repo_root.join("mock");
    c
}

/// The gate as the writer renders it for one repository.
fn gate_for(repo_root: &std::path::Path) -> String {
    let cfg = cfg_at(repo_root);
    builtin_check_message(&cfg)
        .replace("{{HOOK_HELPERS}}", crate::render_agent::CLAUDE_HOOK_HELPERS)
        .replace("{{REPO_ROOT}}", &repo_root.display().to_string())
}

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

/// Run the gate from `cwd`, feeding it `payload`, and hand back its stdout.
fn run_from(cwd: &std::path::Path, script: &str, payload: &str) -> String {
    let f = cwd.join(format!("gate_{}.sh", std::process::id()));
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

/// Whether the gate took the call as its own.
///
/// Claude's `allow` prints an object carrying no `permissionDecision` and
/// `deny` prints one that does, so the presence of that key is the whole test.
///
/// **A refusal for a missing launcher counts as claimed, and that is the point
/// rather than a hole.** These arms are about which repository the gate decides
/// it is about, not about what the linter then says, and a gate that got as far
/// as looking for `mock` had already decided the call was its business. So the
/// arms hold whether or not a launcher is installed on the machine running
/// them, and neither answer can be mistaken for the early allow that scoping
/// out produces.
fn claimed(out: &str) -> bool {
    out.contains("permissionDecision")
}

/// Two repositories side by side, which is what the workspace actually looks
/// like and what the single-root fixtures could not express.
fn two_repos(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let base = scratch(tag);
    let mine = base.join("mine");
    let other = base.join("other");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    (mine, other)
}

#[test]
fn a_commit_naming_another_repo_is_not_this_gates_business() {
    // The defect, in the shape it was met in: a shell standing in one repo,
    // committing into another by absolute path, refused by the first repo's
    // gate for a subject that was never a subject.
    let (mine, other) = two_repos("other");
    let gate = gate_for(&mine);
    let cmd = format!(
        "git -C {} commit -m 'state: a subject well under the limit'",
        other.display()
    );
    let out = run_from(&mine, &gate, &bash_payload(&cmd));
    assert!(
        !claimed(&out),
        "the gate claimed a commit in another repository: {out}"
    );
}

#[test]
fn a_commit_naming_this_repo_is_still_this_gates_business() {
    // The other half, and the one that makes the arm above mean something. If
    // an explicit path could only ever exempt a call, the fix would be a
    // bypass: `git -C .` from anywhere would silence the gate for good.
    let (mine, _other) = two_repos("mine");
    let gate = gate_for(&mine);
    let cmd = format!(
        "git -C {} commit -m 'Fixed a whole load of things and this subject runs far past the seventy-two character limit.'",
        mine.display()
    );
    let out = run_from(&mine, &gate, &bash_payload(&cmd));
    assert!(
        claimed(&out),
        "the gate let go of a commit into its own repository: {out}"
    );
}

#[test]
fn an_explicit_path_elsewhere_does_not_cover_a_bare_commit_beside_it() {
    // `git -C <elsewhere> fetch && git commit -m ...` names a repository and
    // then commits in the one the shell is standing in. Reading the first as
    // exempting the second is how a scope check becomes a bypass, and the
    // second is the call the gate exists for.
    let (mine, other) = two_repos("both");
    let gate = gate_for(&mine);
    let cmd = format!(
        "git -C {} fetch && git commit -m 'Fixed a whole load of things and this subject runs far past the seventy-two character limit.'",
        other.display()
    );
    let out = run_from(&mine, &gate, &bash_payload(&cmd));
    assert!(
        claimed(&out),
        "an unqualified commit went unclaimed because something else named another repo: {out}"
    );
}

#[test]
fn a_dash_c_belonging_to_another_program_decides_nothing() {
    // `-C` is `grep`'s, `make`'s and `tar`'s as much as it is git's, so a
    // number or a relative name where a path was expected must not read as
    // another repository and exempt the commit sitting next to it.
    let (mine, _other) = two_repos("grep");
    let gate = gate_for(&mine);
    let cmd = "grep -C 3 needle haystack && git commit -m 'Fixed a whole load of things and this subject runs far past the seventy-two character limit.'".to_string();
    let out = run_from(&mine, &gate, &bash_payload(&cmd));
    assert!(
        claimed(&out),
        "a context flag was read as a repository and exempted the commit: {out}"
    );
}

#[test]
fn the_symlinked_root_is_recognised_as_this_repo() {
    // On macOS the temporary tree lives under `/var`, a link to `/private/var`,
    // so `cd` and `pwd` hand back a path that does not match the root string.
    // Comparing only one of the two spellings clears a path that is this
    // repository, which turns `git -C .` into a bypass on every mac.
    let (mine, _other) = two_repos("symlink");
    let gate = gate_for(&mine);
    let resolved = std::fs::canonicalize(&mine).unwrap();
    // The control: if the two spellings are already identical on this machine,
    // the arm proves nothing and says so rather than passing quietly.
    if resolved == mine {
        eprintln!(
            "note: {} needs no resolving on this machine",
            mine.display()
        );
    }
    let cmd = format!(
        "git -C {} commit -m 'Fixed a whole load of things and this subject runs far past the seventy-two character limit.'",
        resolved.display()
    );
    let out = run_from(&mine, &gate, &bash_payload(&cmd));
    assert!(
        claimed(&out),
        "the resolved spelling of this repo's own root was read as another repository: {out}"
    );
}

#[test]
fn a_forge_body_is_not_excused_by_a_path_some_other_program_was_given() {
    // `-C` is not git's alone, and a forge body is not about a worktree at all,
    // so "this call names another repository" must not reach a `gh pr create`.
    // Placing that test beside the scope check rather than after the domain is
    // known let a `tar -C /tmp` in the same command line wave a bad title
    // through, which is a hole where there had been a gate.
    let (mine, _other) = two_repos("forge");
    let gate = gate_for(&mine);
    let cmd = "tar -C /tmp -xf x.tar && gh pr create --title 'Bad Title.' --body ''";
    let out = run_from(&mine, &gate, &bash_payload(cmd));
    assert!(
        claimed(&out),
        "a forge body went unchecked because something else was given a path: {out}"
    );
}

#[test]
fn a_command_touching_no_repository_at_all_is_left_alone() {
    // The negative control for the whole file, and what it controls for is
    // narrower than it first reads: four of the arms above assert the gate
    // claims a call, so a gate that allowed unconditionally would fail those
    // rather than pass them. What this one catches is the opposite drift, a
    // gate that claims everything, which would make every `claimed` arm pass
    // for a reason that has nothing to do with what it is named for.
    let (mine, _other) = two_repos("nothing");
    let gate = gate_for(&mine);
    let out = run_from(&mine, &gate, &bash_payload("echo hello"));
    assert!(
        !claimed(&out),
        "the gate refused a command that authors no message: {out}"
    );
}
