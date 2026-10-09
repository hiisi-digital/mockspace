//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A commit made under an agent identity is refused at the commit and at the
//! push, and one made under a person's is not, through a real git repository.
//!
//! Everything between `git` and the lint is the real thing: the hooks the
//! engine generates, the launcher on `PATH` calling the engine binary, the
//! engine building and loading the repository's lints as a cdylib. What stands
//! in is the policy. The probe lint in the fixture refuses an identity naming
//! `Claude` and nothing else, because the policy a project actually runs lives
//! in its lint pack and is tested there; this test is about whether the
//! identity gets to a lint at all.
//!
//! An agent identity leaves no trailer and no advert in the message, so a check
//! that reads only the message passes it. The person arms below are the controls:
//! without them a probe refusing everything would pass too.
//!
//! `#[ignore]` because it runs `cargo build` for the fixture's cdylib, matching
//! `custom_lint_cdylib.rs`, and `tests/rust_e2e_test.sh` runs it with
//! `--ignored` every time `./test` does.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A lint refusing any commit whose author or committer names `Claude`.
const PROBE: &str = r#"use mockspace::{Lint, LintError, MessageContext, MessageLint};

pub fn message_lint() -> Box<dyn MessageLint> {
    Box::new(IdentityProbe)
}

struct IdentityProbe;

impl Lint for IdentityProbe {
    fn name(&self) -> &'static str {
        "identity-probe"
    }
}

impl MessageLint for IdentityProbe {
    fn check_message(&self, ctx: &MessageContext) -> Vec<LintError> {
        [("author", ctx.author), ("committer", ctx.committer)]
            .into_iter()
            .filter_map(|(role, who)| {
                let who = who?;
                who.contains("Claude").then(|| {
                    LintError::error(
                        ctx.origin.to_string(),
                        1,
                        "identity-probe",
                        format!("the {role} is an agent: {who}"),
                    )
                })
            })
            .collect()
    }
}
"#;

const PERSON: (&str, &str) = ("Jane Smith", "jane@example.com");
const AGENT: (&str, &str) = ("Claude", "noreply@anthropic.com");

struct Fixture {
    _tmp:   tempfile::TempDir,
    repo:   PathBuf,
    remote: PathBuf,
    bin:    PathBuf,
}

/// Where the `cargo` that is running this test lives, so the shim in front of it
/// can hand everything but `cargo mock` on to the real one.
fn real_cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    let remote = root.join("remote.git");
    let bin = root.join("bin");
    let mock = repo.join("mock");
    fs::create_dir_all(mock.join("lints")).unwrap();
    fs::create_dir_all(&bin).unwrap();
    fs::write(mock.join("mockspace.toml"), "project_name = \"probe\"\n").unwrap();
    fs::write(mock.join("lints/identity_probe.rs"), PROBE).unwrap();

    // The launcher: the engine binary with the project and the lint-rules pin
    // the real launcher would add. `MOCKSPACE_NO_AUTO_ACTIVATE` keeps the engine
    // from pointing `core.hooksPath` at a durable directory in a home that is
    // not this fixture's.
    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    let launcher = bin.join("mock");
    fs::write(
        &launcher,
        format!(
            "#!/bin/sh\nexport MOCKSPACE_NO_AUTO_ACTIVATE=1\nexec '{}' --dir '{}' \
             --mockspace-lint-rules-dep '{{ package = \"mockspace-lint-rules\", path = \"{}\" }}' \"$@\"\n",
            env!("CARGO_BIN_EXE_mockspace"),
            mock.display(),
            lint_rules
        ),
    )
    .unwrap();

    // The generated pre-push hook ends in `cargo mock --lint-only --strict`,
    // which is the crate lints and has nothing to say about a message. Standing
    // it down leaves the message scan, which runs first, as the only thing that
    // can refuse a push here; anything else `cargo` is asked goes to the real one.
    let cargo = bin.join("cargo");
    fs::write(
        &cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = mock ]; then exit 0; fi\nexec '{}' \"$@\"\n",
            real_cargo()
        ),
    )
    .unwrap();
    for f in [&launcher, &cargo] {
        make_executable(f);
    }

    fs::create_dir_all(&repo).unwrap();
    let fx = Fixture {
        _tmp: tmp,
        repo,
        remote,
        bin,
    };
    git(&fx, &["init", "-q", "-b", "main"], PERSON).assert_ok();
    git(
        &fx,
        &["init", "-q", "--bare", "-b", "main", fx.remote.to_str().unwrap()],
        PERSON,
    )
    .assert_ok();
    git(
        &fx,
        &["remote", "add", "origin", fx.remote.to_str().unwrap()],
        PERSON,
    )
    .assert_ok();
    fx
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut p = fs::metadata(path).unwrap().permissions();
    p.set_mode(0o755);
    fs::set_permissions(path, p).unwrap();
}

trait Outcome {
    fn assert_ok(&self);
    fn text(&self) -> String;
}

impl Outcome for Output {
    fn assert_ok(&self) {
        assert!(self.status.success(), "{}", self.text());
    }

    fn text(&self) -> String {
        format!(
            "status {:?}\nstdout: {}\nstderr: {}",
            self.status.code(),
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        )
    }
}

/// Git as `who`, with the machine's own configuration out of the way.
///
/// `who` is both the configured identity and the one the environment would give,
/// so a case that wants the two to differ sets the committer through `git_env`.
fn git(fx: &Fixture, args: &[&str], who: (&str, &str)) -> Output {
    git_env(fx, args, who, &[])
}

fn git_env(fx: &Fixture, args: &[&str], who: (&str, &str), envs: &[(&str, &str)]) -> Output {
    let path = format!(
        "{}:{}",
        fx.bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut c = Command::new("git");
    c.current_dir(&fx.repo)
        .args(args)
        .env("PATH", path)
        .env("HOME", &fx.repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", who.0)
        .env("GIT_AUTHOR_EMAIL", who.1)
        .env("GIT_COMMITTER_NAME", who.0)
        .env("GIT_COMMITTER_EMAIL", who.1);
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().expect("git runs")
}

fn commit(fx: &Fixture, who: (&str, &str), envs: &[(&str, &str)], extra: &[&str]) -> Output {
    let mut args = vec!["commit", "-q", "--allow-empty", "-m", "feat: add a thing"];
    args.extend_from_slice(extra);
    git_env(fx, &args, who, envs)
}

fn commits_on_head(fx: &Fixture) -> usize {
    let out = git(fx, &["rev-list", "--count", "HEAD"], PERSON);
    if !out.status.success() {
        return 0;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

/// Run the engine once so it builds the fixture's lints and writes the hooks,
/// then point git at them. Doubles as a control: the probe is asked about a
/// person and has to say nothing.
fn install_the_gate(fx: &Fixture) {
    let msg = fx.repo.join("MSG");
    fs::write(&msg, "feat: add a thing\n").unwrap();
    let out = Command::new(fx.bin.join("mock"))
        .current_dir(&fx.repo)
        .env("HOME", &fx.repo)
        .env(
            "PATH",
            format!(
                "{}:{}",
                fx.bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .args(["check-message", "--domain", "commit-message", "--gate", "commit", "--file"])
        .arg(&msg)
        .args(["--author", "Jane Smith <jane@example.com>"])
        .args(["--committer", "Jane Smith <jane@example.com>"])
        .output()
        .unwrap();
    out.assert_ok();
    fs::remove_file(&msg).unwrap();

    let hooks = fx.repo.join("mock/target/hooks");
    for name in ["commit-msg", "pre-push"] {
        assert!(hooks.join(name).is_file(), "the engine writes {name}");
    }
    git(
        fx,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
        PERSON,
    )
    .assert_ok();
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn the_commit_gate_refuses_an_agent_identity_and_passes_a_person() {
    let fx = fixture();
    install_the_gate(&fx);

    // the control: a person commits, the gate runs, nothing is refused
    commit(&fx, PERSON, &[], &[]).assert_ok();
    assert_eq!(commits_on_head(&fx), 1, "the person's commit is made");

    // the agent as both author and committer, which is what a container whose
    // global identity was set to the agent produces
    let out = commit(&fx, AGENT, &[], &[]);
    assert!(!out.status.success(), "must be refused. {}", out.text());
    let text = out.text();
    assert!(
        text.contains("the author is an agent: Claude <noreply@anthropic.com>"),
        "the finding names the author. {text}"
    );
    assert!(text.contains("identity-probe"), "{text}");
    assert_eq!(commits_on_head(&fx), 1, "the refused commit is not made");

    // the agent as author only, under a person's committer
    let out = commit(
        &fx,
        AGENT,
        &[("GIT_COMMITTER_NAME", PERSON.0), ("GIT_COMMITTER_EMAIL", PERSON.1)],
        &[],
    );
    assert!(!out.status.success(), "{}", out.text());
    assert!(
        out.text().contains("the author is an agent"),
        "{}",
        out.text()
    );
    assert!(
        !out.text().contains("the committer is an agent"),
        "{}",
        out.text()
    );

    // the agent as committer only, under a person's `--author`
    let out = commit(&fx, AGENT, &[], &["--author=Jane Smith <jane@example.com>"]);
    assert!(!out.status.success(), "{}", out.text());
    assert!(
        out.text().contains("the committer is an agent"),
        "{}",
        out.text()
    );
    assert!(
        !out.text().contains("the author is an agent"),
        "{}",
        out.text()
    );
    assert_eq!(
        commits_on_head(&fx),
        1,
        "none of the refused commits is made"
    );

    // and the repair the finding points at lets the same work through
    commit(&fx, PERSON, &[], &[]).assert_ok();
    assert_eq!(commits_on_head(&fx), 2);
}

#[test]
#[ignore = "runs cargo build; run with --ignored"]
fn the_push_gate_refuses_a_branch_holding_an_agent_commit_and_passes_a_person_s() {
    let fx = fixture();
    install_the_gate(&fx);

    // history that predates the gate being asked about it: made with the commit
    // hook stood aside, so the push is the first thing to see it
    commit(&fx, PERSON, &[], &["--no-verify"]).assert_ok();
    git(&fx, &["push", "-q", "origin", "main"], PERSON).assert_ok();

    // the control: a branch of a person's commits goes
    git(&fx, &["switch", "-q", "-c", "by-a-person"], PERSON).assert_ok();
    commit(&fx, PERSON, &[], &["--no-verify"]).assert_ok();
    let out = git(&fx, &["push", "-q", "origin", "by-a-person"], PERSON);
    out.assert_ok();
    assert!(
        out.text().contains("pre-push: validation passed"),
        "the gate ran to its end. {}",
        out.text()
    );

    // a branch with one commit by the agent among a person's is refused, and
    // the refusal names that commit rather than the push
    git(&fx, &["switch", "-q", "-c", "by-an-agent", "main"], PERSON).assert_ok();
    commit(&fx, PERSON, &[], &["--no-verify"]).assert_ok();
    commit(&fx, AGENT, &[], &["--no-verify"]).assert_ok();
    commit(&fx, PERSON, &[], &["--no-verify"]).assert_ok();
    let out = git(&fx, &["push", "-q", "origin", "by-an-agent"], PERSON);
    assert!(!out.status.success(), "must be refused. {}", out.text());
    let text = out.text();
    assert!(
        text.contains("the author is an agent: Claude <noreply@anthropic.com>"),
        "{text}"
    );
    assert!(text.contains("BLOCKED"), "{text}");
    assert!(
        !text.contains("pre-push: validation passed"),
        "the gate stopped at the message scan. {text}"
    );
    let remote = Command::new("git")
        .args(["--git-dir", fx.remote.to_str().unwrap(), "branch", "--list"])
        .output()
        .unwrap();
    assert!(
        !String::from_utf8_lossy(&remote.stdout).contains("by-an-agent"),
        "nothing reached the remote"
    );

    // an agent that is only the committer is caught by the same scan
    git(
        &fx,
        &["switch", "-q", "-c", "committed-by-an-agent", "main"],
        PERSON,
    )
    .assert_ok();
    commit(
        &fx,
        PERSON,
        &[("GIT_COMMITTER_NAME", AGENT.0), ("GIT_COMMITTER_EMAIL", AGENT.1)],
        &["--no-verify"],
    )
    .assert_ok();
    let out = git(
        &fx,
        &["push", "-q", "origin", "committed-by-an-agent"],
        PERSON,
    );
    assert!(!out.status.success(), "{}", out.text());
    assert!(
        out.text().contains("the committer is an agent"),
        "{}",
        out.text()
    );

    // the repair: the same branch, every commit made again under a person
    git(&fx, &["switch", "-q", "by-an-agent"], PERSON).assert_ok();
    git(
        &fx,
        &[
            "rebase",
            "-q",
            "--exec",
            "git commit -q --amend --allow-empty --no-edit --reset-author --no-verify",
            "main",
        ],
        PERSON,
    )
    .assert_ok();
    let out = git(&fx, &["push", "-q", "origin", "by-an-agent"], PERSON);
    out.assert_ok();
}
