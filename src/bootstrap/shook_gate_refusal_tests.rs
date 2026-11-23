//! The gate's refusal while mockspace is not initialised, with the gate registered through git-shook:
//! a commit that touches the design surface, and a push, are refused when the generated hook under
//! `<mock>/target/hooks/` is missing, and the message names the fix. With the generated hook there,
//! the same commit and push pass.
//!
//! The durable hook is the one that refuses (`mockspace_manifest::gate::durable_hook`), and these
//! run it as git-shook's entry, which is how a clone with the gate registered reaches it. The tests
//! need the real `git shook`, and say so when they cannot run rather than passing.

use std::path::PathBuf;
use std::process::Output;

use super::tests::{git, git_command, have_shook, register_t, repo};
use super::*;

/// A clone of a mockspace project (a `mockspace.toml` at its root, the design surface in `mock/`)
/// that git-shook has taken and the gate is registered with, and a bare remote named `origin`.
/// `scope` is the config's `uninitialised_blocks`, `None` where it is left at its default.
struct Gated {
    _guard: tempfile::TempDir,
    dir:    PathBuf,
}

impl Gated {
    fn new(scope: Option<&str>) -> Self {
        let guard = repo("refusal");
        let dir = guard.path().to_path_buf();
        let config = match scope {
            Some(s) => format!("uninitialised_blocks = \"{s}\"\n"),
            None => String::new(),
        };
        fs::write(dir.join("mockspace.toml"), config).unwrap();
        fs::write(dir.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
        fs::create_dir_all(dir.join("mock")).unwrap();
        fs::write(dir.join("mock/design.md"), "one\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "one"]);
        let hooks = dir.join("durable");
        let _ = mockspace_manifest::gate::install_durable_hooks(&hooks, HOOK_VERSION);
        git(&dir, &["shook", "install", "--yes"]);
        register_t(&dir, &hooks).unwrap();
        let remote = dir.join("remote.git");
        git(&dir, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(&dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
        Gated { _guard: guard, dir }
    }

    fn run(&self, args: &[&str]) -> Output {
        git_command().args(args).current_dir(&self.dir).output().expect("git")
    }

    /// The generated hooks `<mock>/target/hooks/<event>`, each appending its name to `generated.log`.
    fn generate(&self) -> PathBuf {
        let log = self.dir.join("generated.log");
        for event in ["pre-commit", "commit-msg", "pre-push"] {
            crate::test_exec::write_executable(
                &self.dir.join("mock/target/hooks").join(event),
                format!("#!/bin/sh\necho {event} >> '{}'\nexit 0\n", log.display()),
            );
        }
        log
    }

    fn stage(&self, rel: &str) {
        fs::write(self.dir.join(rel), "changed\n").unwrap();
        git(&self.dir, &["add", rel]);
    }
}

fn said(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

macro_rules! need_shook {
    () => {
        if !have_shook() {
            eprintln!("SKIPPED: git shook is not found");
            return;
        }
    };
}

#[test]
fn a_commit_touching_the_design_surface_is_refused_without_the_generated_hook_and_names_the_fix() {
    need_shook!();
    let c = Gated::new(None);
    c.stage("mock/design.md");

    let out = c.run(&["commit", "-q", "-m", "two"]);

    assert!(!out.status.success(), "the commit is refused: {}", said(&out));
    let err = said(&out);
    assert!(err.contains("BLOCKED: mockspace is not initialised"), "{err}");
    assert!(err.contains("to initialise:  mock"), "the message names the fix: {err}");
    assert_eq!(git(&c.dir, &["rev-list", "--count", "HEAD"]), "1", "and nothing was committed");
}

#[test]
fn a_commit_outside_the_design_surface_passes_without_the_generated_hook() {
    need_shook!();
    let c = Gated::new(None);
    c.stage("elsewhere.txt");

    let out = c.run(&["commit", "-q", "-m", "two"]);

    assert!(out.status.success(), "the default scope is the surface alone: {}", said(&out));
}

#[test]
fn a_push_is_refused_without_the_generated_hook_where_the_scope_is_all_and_names_the_fix() {
    need_shook!();
    let c = Gated::new(Some("all"));

    let out = c.run(&["push", "origin", "HEAD:refs/heads/main"]);

    assert!(!out.status.success(), "the push is refused: {}", said(&out));
    let err = said(&out);
    assert!(err.contains("BLOCKED: mockspace is not initialised"), "{err}");
    assert!(err.contains("to initialise:  mock"), "the message names the fix: {err}");
    assert_eq!(git(&c.dir, &["ls-remote", "origin"]), "", "and nothing reached the remote");
}

#[test]
fn a_commit_touching_the_design_surface_passes_with_the_generated_hook_and_it_runs() {
    need_shook!();
    let c = Gated::new(None);
    let log = c.generate();
    c.stage("mock/design.md");

    let out = c.run(&["commit", "-q", "-m", "two"]);

    assert!(out.status.success(), "the control passes: {}", said(&out));
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "pre-commit").count(), 1, "the generated hook ran, once: {ran}");
}

#[test]
fn a_push_passes_with_the_generated_hook_where_the_scope_is_all_and_it_runs() {
    need_shook!();
    let c = Gated::new(Some("all"));
    let log = c.generate();

    let out = c.run(&["push", "origin", "HEAD:refs/heads/main"]);

    assert!(out.status.success(), "the control passes: {}", said(&out));
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "pre-push").count(), 1, "the generated hook ran, once: {ran}");
    assert!(!git(&c.dir, &["ls-remote", "origin"]).is_empty(), "and the push reached the remote");
}
