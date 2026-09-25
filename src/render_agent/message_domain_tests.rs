//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which forge commands a message gate takes as messages, and where it runs
//! the engine on them.
//!
//! The gate took every `gh pr`, `gh issue`, `gh release` and `gh gist` as a
//! message, so a `gh pr view` went to the engine like a body would. The engine
//! then ran from wherever the shell stood rather than from the repository the
//! call was scoped to, found another tree's configuration or none, and refused
//! the read. `gh api`, which can write a pull request's title and body as well
//! as `gh pr create` can, was not taken at all.
//!
//! A fake `mock` first on `PATH` stands in for the engine. It fails, and says
//! the directory it ran in, so a claimed call is refused with that directory
//! in the reason, and a call left alone is allowed with no reason. Every write
//! arm asserts the fake's own words, so a gate that refused for any other
//! reason, or never reached the engine, fails them.

use std::process::Command;

use super::message_scope_tests::{bash_payload, gate_for, scratch, two_repos};

/// A directory holding a `mock` that reports where it ran and fails.
fn fake_engine() -> std::path::PathBuf {
    let bin = scratch("fakebin");
    let f = bin.join("mock");
    std::fs::write(&f, "#!/usr/bin/env bash\ncat >/dev/null\necho \"engine ran in $(pwd -P)\"\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

/// Run the gate from `cwd` with the fake engine first on `PATH` and a
/// verdict cache of its own, so no arm reads another's cached refusal.
fn run(cwd: &std::path::Path, gate: &str, command: &str) -> String {
    let bin = fake_engine();
    let f = cwd.join(format!("gate_{}.sh", std::process::id()));
    std::fs::write(&f, gate).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let out = Command::new("bash")
        .arg(&f)
        .current_dir(cwd)
        .env("PATH", path)
        .env("TMPDIR", scratch("cache"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut ch| {
            use std::io::Write;
            ch.stdin.as_mut().unwrap().write_all(bash_payload(command).as_bytes())?;
            ch.wait_with_output()
        })
        .unwrap();
    std::fs::remove_file(&f).ok();
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn left_alone(out: &str) -> bool {
    !out.contains("permissionDecision")
}

fn engine_ran_in(out: &str) -> Option<String> {
    let at = out.find("engine ran in ")? + "engine ran in ".len();
    let rest = &out[at..];
    let end = rest.find(|c: char| c == '"' || c == '\\').unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

const READS: &[&str] = &[
    "gh pr view 11",
    "gh pr view 11 -R o/r --json state,title -q .state",
    "gh pr list --state all",
    "gh pr diff 3",
    "gh pr checks 3",
    "gh pr status",
    "gh issue view 2",
    "gh issue list",
    "gh release view v1",
    "gh release list",
    "gh gist view abc",
    "gh api repos/o/r/pulls/1 -q .state",
    "gh api -X GET repos/o/r/pulls -f state=all",
    "gh api --method GET search/issues -F per_page=5",
    "gh api --method=get search/issues -f q=x",
    "gh api -XGET repos/o/r/pulls -f state=all",
    "gh -R o/r pr view 1",
    "gh pr view 1 --json title -q '.title | test(\"create\")'",
    "echo 'gh api writes with -f' ; gh api repos/o/r/pulls/1",
    "glab mr view 1",
    "glab issue list",
    "glab release view v1",
];

const WRITES: &[&str] = &[
    "gh pr create --base dev --title 'fix: a title' --body ''",
    "gh pr edit 1 --title 'fix: a title'",
    "gh pr comment 1 --body 'words'",
    "gh pr review 1 --approve --body 'words'",
    "gh pr merge 1 --squash --subject 'fix: a title' --body ''",
    "gh pr close 1 --comment 'words'",
    "gh issue create --title 't' --body 'b'",
    "gh issue edit 2 --body 'b'",
    "gh issue comment 2 --body 'b'",
    "gh issue close 2 --comment 'b'",
    "gh release create v1 --notes 'n'",
    "gh release edit v1 --notes 'n'",
    "gh gist create notes.md",
    "gh gist edit abc",
    "gh api repos/o/r/pulls -f title='fix: a title' -f head=b -f base=main -f body=''",
    "gh api -X PUT repos/o/r/pulls/1/merge -f commit_title='fix: a title'",
    "gh api repos/o/r/issues/1/comments -F body=@note.md",
    "gh api repos/o/r/issues/1/comments --field body=words",
    "gh api repos/o/r/issues/1/comments --raw-field body=words",
    "gh api repos/o/r/pulls --input pr.json",
    "gh api repos/o/r/pulls --input - < pr.json",
    "gh api -X POST repos/o/r/pulls -f body=b",
    "gh api --method=post repos/o/r/pulls -f body=b",
    "gh api -X GET repos/o/r/pulls && gh api repos/o/r/pulls -f body=b",
    "gh -R o/r pr create --title 't' --body ''",
    "gh --repo o/r issue comment 2 --body 'b'",
    "gh pr merge 1 --squash --body-file body.md",
    "glab -R o/r mr note 1 --message 'm'",
    "glab mr create --title 't' --description 'd'",
    "glab mr update 1 --title 't'",
    "glab mr note 1 --message 'm'",
    "glab mr merge 1",
    "glab mr close 1",
    "glab issue create --title 't'",
    "glab issue update 2 --description 'd'",
    "glab issue note 2 --message 'm'",
    "glab issue close 2",
    "glab release create v1 --notes 'n'",
    "glab release update v1 --notes 'n'",
];

#[test]
fn a_forge_command_that_writes_nothing_is_left_alone() {
    let (mine, _) = two_repos("reads");
    let gate = gate_for(&mine);
    for cmd in READS {
        let out = run(&mine, &gate, cmd);
        assert!(left_alone(&out), "`{cmd}` was taken as a message: {out}");
    }
}

#[test]
fn every_forge_command_that_writes_goes_to_the_engine() {
    let (mine, _) = two_repos("writes");
    let gate = gate_for(&mine);
    for cmd in WRITES {
        let out = run(&mine, &gate, cmd);
        assert!(
            engine_ran_in(&out).is_some(),
            "`{cmd}` did not reach the engine: {out}"
        );
    }
}

#[test]
fn a_read_beside_a_write_in_one_command_is_still_a_message() {
    // The verb decides per command line, not per first match, so a read in
    // front does not excuse the write behind it.
    let (mine, _) = two_repos("mixed");
    let gate = gate_for(&mine);
    let out = run(&mine, &gate, "gh pr view 1 && gh pr create --title 't' --body ''");
    assert!(engine_ran_in(&out).is_some(), "the write went unchecked: {out}");
}

#[test]
fn the_engine_runs_from_the_repository_the_call_was_scoped_to() {
    // Standing in one tree and naming the other by path is the shape it was
    // met in: the engine walked up from the shell's directory and read the
    // wrong tree's configuration.
    let (mine, other) = two_repos("where");
    let gate = gate_for(&mine);
    let cmd = format!("cd {} && gh pr create --title 't' --body ''", mine.display());
    let out = run(&other, &gate, &cmd);
    let ran = engine_ran_in(&out).unwrap_or_else(|| panic!("the engine never ran: {out}"));
    assert_eq!(ran, mine.canonicalize().unwrap().display().to_string());
}

#[test]
fn the_engine_runs_from_the_repository_when_the_shell_is_already_there() {
    // The control for the arm above: from inside the repository the answer
    // was already right, so the arm above fails for the move and nothing else.
    let (mine, _) = two_repos("here");
    let gate = gate_for(&mine);
    let out = run(&mine, &gate, "gh pr create --title 't' --body ''");
    assert_eq!(
        engine_ran_in(&out).as_deref(),
        Some(mine.canonicalize().unwrap().display().to_string().as_str())
    );
}
