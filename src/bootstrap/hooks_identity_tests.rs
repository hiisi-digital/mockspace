//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit-msg hook hands the launcher the identity git will commit under.
//!
//! Run by a real `git commit` against the generated hook, with a stub standing
//! in for the launcher so its arguments can be read. The identity is what
//! `git var` reports at that moment and not what the config says, which only
//! shows once `--author` or an environment override moves one of them.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

/// A repository with the generated commit-msg hook installed and a stub `mock`
/// that writes its arguments, one per line, to `args.txt` and passes.
struct Fixture {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    bin:  PathBuf,
    args: PathBuf,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let bin = tmp.path().join("bin");
    let args = tmp.path().join("args.txt");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&bin).unwrap();

    let stub = bin.join("mock");
    std::fs::write(
        &stub,
        format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$@\" > {}\nexit 0\n",
            args.display()
        ),
    )
    .unwrap();
    make_executable(&stub);

    git(&repo, &[], &["init", "-q", "-b", "main"]);
    let hooks = repo.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("commit-msg");
    std::fs::write(
        &hook,
        gen_commit_msg(&repo.join(".git/hooks/commit-msg.own")),
    )
    .unwrap();
    make_executable(&hook);

    Fixture {
        _tmp: tmp,
        repo,
        bin,
        args,
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut p = std::fs::metadata(path).unwrap().permissions();
    p.set_mode(0o755);
    std::fs::set_permissions(path, p).unwrap();
}

/// Git with the machine's own config out of the way, so a global identity on
/// whatever runs this cannot stand in for the one under test.
fn git_command(repo: &Path, path: &str, envs: &[(&str, &str)], args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.current_dir(repo)
        .args(args)
        .env("PATH", path)
        .env("HOME", repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
    for (k, v) in envs {
        c.env(k, v);
    }
    c
}

fn git(repo: &Path, envs: &[(&str, &str)], args: &[&str]) {
    let path = std::env::var("PATH").unwrap_or_default();
    let out = git_command(repo, &path, envs, args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn path_for(fx: &Fixture) -> String {
    let which = Command::new("sh")
        .arg("-c")
        .arg("command -v git")
        .output()
        .unwrap();
    let git = String::from_utf8_lossy(&which.stdout).trim().to_string();
    let git_dir = Path::new(&git)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!("{}:{git_dir}:/usr/bin:/bin", fx.bin.display())
}

/// The arguments the stub recorded, one per element.
fn recorded(fx: &Fixture) -> Vec<String> {
    std::fs::read_to_string(&fx.args)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// The argument following `flag`, or none when the flag was not passed.
fn value_of(args: &[String], flag: &str) -> Option<String> {
    let at = args.iter().position(|a| a == flag)?;
    args.get(at + 1).cloned()
}

fn commit(fx: &Fixture, envs: &[(&str, &str)], extra: &[&str]) {
    let mut args = vec!["commit", "-q", "--allow-empty", "-m", "feat: x"];
    args.extend_from_slice(extra);
    let out = git_command(&fx.repo, &path_for(fx), envs, &args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

#[test]
fn the_hook_hands_the_launcher_the_author_and_committer_the_commit_carries() {
    let fx = fixture();
    git(&fx.repo, &[], &["config", "user.name", "Claude"]);
    git(&fx.repo, &[], &[
        "config",
        "user.email",
        "noreply@anthropic.com",
    ]);

    commit(&fx, &[], &[]);

    let args = recorded(&fx);
    assert!(args.iter().any(|a| a == "check-message"), "got: {args:?}");
    // The date and zone are the launcher's to strip, so only the leading part is
    // pinned: what has to be true is whose commit it is.
    let author = value_of(&args, "--author").expect("--author is passed");
    let committer = value_of(&args, "--committer").expect("--committer is passed");
    assert!(
        author.starts_with("Claude <noreply@anthropic.com>"),
        "got: {author:?}"
    );
    assert!(
        committer.starts_with("Claude <noreply@anthropic.com>"),
        "got: {committer:?}"
    );
}

#[test]
fn an_author_override_reaches_the_launcher_and_not_the_configured_name() {
    // `--author` is the case a config read would get wrong: git exports the
    // overriding identity to the hook and leaves `user.name` as it was, so
    // asking the config names an author the commit does not have.
    let fx = fixture();
    git(&fx.repo, &[], &["config", "user.name", "Claude"]);
    git(&fx.repo, &[], &[
        "config",
        "user.email",
        "noreply@anthropic.com",
    ]);

    commit(&fx, &[], &["--author=Jane Doe <jane@example.com>"]);

    let args = recorded(&fx);
    let author = value_of(&args, "--author").expect("--author is passed");
    let committer = value_of(&args, "--committer").expect("--committer is passed");
    assert!(
        author.starts_with("Jane Doe <jane@example.com>"),
        "got: {author:?}"
    );
    assert!(
        committer.starts_with("Claude <noreply@anthropic.com>"),
        "the committer is still the configured identity. got: {committer:?}"
    );
}

#[test]
fn an_environment_override_of_the_committer_reaches_the_launcher() {
    let fx = fixture();
    git(&fx.repo, &[], &["config", "user.name", "Jane Doe"]);
    git(&fx.repo, &[], &["config", "user.email", "jane@example.com"]);

    commit(
        &fx,
        &[
            ("GIT_COMMITTER_NAME", "Claude"),
            ("GIT_COMMITTER_EMAIL", "noreply@anthropic.com"),
        ],
        &[],
    );

    let args = recorded(&fx);
    let author = value_of(&args, "--author").expect("--author is passed");
    let committer = value_of(&args, "--committer").expect("--committer is passed");
    assert!(
        author.starts_with("Jane Doe <jane@example.com>"),
        "got: {author:?}"
    );
    assert!(
        committer.starts_with("Claude <noreply@anthropic.com>"),
        "got: {committer:?}"
    );
}

#[test]
fn a_hook_run_where_git_names_nobody_passes_no_identity_and_still_calls_the_launcher() {
    // With `useConfigOnly` and no configured name, `git var` fails. The hook has
    // `set -e`, so a failure left bare would end the hook before the launcher ran
    // and the message would go unchecked. It has to degrade to no flag and carry
    // on, so the message half of the gate still holds.
    let fx = fixture();
    git(&fx.repo, &[], &["config", "user.useConfigOnly", "true"]);
    let msg = fx.repo.join("COMMIT_EDITMSG");
    std::fs::write(&msg, "feat: x\n").unwrap();

    let out = Command::new(fx.repo.join(".git/hooks/commit-msg"))
        .arg(&msg)
        .current_dir(&fx.repo)
        .env("PATH", path_for(&fx))
        .env("HOME", &fx.repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");

    let args = recorded(&fx);
    assert!(
        args.iter().any(|a| a == "check-message"),
        "the launcher must still be called. got: {args:?}"
    );
    assert_eq!(value_of(&args, "--author"), None, "got: {args:?}");
    assert_eq!(value_of(&args, "--committer"), None, "got: {args:?}");
}
