//! A merge driver, and every other command that reads no generated crate, runs
//! from the engine binary with no generation and no cargo build.
//!
//! git calls a merge driver once per conflicting file, so whatever the engine
//! does before it reaches the driver is paid once per file. The fixture
//! declares a custom lint, so a command that loads the lint pack generates its
//! crate and asks cargo to build it. `cargo` on the path is a shim that writes
//! down every call and builds nothing, which makes both visible: the call log,
//! less the `metadata` reads that build nothing,
//! and the generated store under the cache.
//!
//! The control, `lints`, is a command that does read the pack. It has to show
//! a cargo call and a store, or the probe sees nothing and the other cases
//! prove nothing.

#[path = "common/ambient.rs"]
mod ambient;

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{BIN, Repo, write, write_executable};

struct Probe {
    repo:  Repo,
    path:  String,
    calls: PathBuf,
    cache: PathBuf,
}

impl Probe {
    fn new() -> Self {
        let repo = Repo::new();
        write(
            &repo.root.join("mock/lints/a_custom_lint.rs"),
            "// a lint the pack would have to generate and build\n",
        );
        let shims = repo.home.join("logging-shims");
        let calls = repo.home.join("cargo-calls");
        write_executable(
            &shims.join("cargo"),
            &format!("#!/bin/sh\necho \"$*\" >> '{}'\nexit 1\n", calls.display()),
        );
        let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap());
        let cache = repo.home.join("cache");
        Probe { repo, path, calls, cache }
    }

    fn engine(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .current_dir(&self.repo.root)
            .env("PATH", &self.path)
            .env("HOME", &self.repo.home)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("XDG_CONFIG_HOME", self.repo.home.join("config"))
            .env_remove("MOCK_ROOT")
            .arg("--dir")
            .arg(self.repo.root.join("mock"))
            .args(["--mockspace-lint-rules-dep", "lint-rules"])
            .args(args)
            .output()
            .unwrap()
    }

    /// Every cargo call but `metadata`, which reads the member graph and builds nothing.
    fn cargo_calls(&self) -> String {
        fs::read_to_string(&self.calls)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.starts_with("metadata"))
            .map(|l| format!("{l}\n"))
            .collect()
    }

    fn generated(&self) -> bool {
        self.cache.join("mockspace").exists()
    }

    fn sides(&self, ours: &str) -> [String; 3] {
        let dir = self.repo.home.join("sides");
        let side = |name: &str, text: &str| -> String {
            let p = dir.join(name);
            write(&p, text);
            p.display().to_string()
        };
        [side("base", "a\n"), side("ours", ours), side("theirs", "a\nc\n")]
    }
}

fn builds_nothing(subcommand: &[&str]) {
    let probe = Probe::new();
    let out = probe.engine(subcommand);
    assert_eq!(
        probe.cargo_calls(),
        "",
        "`{}` called cargo; stderr: {}",
        subcommand.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !probe.generated(),
        "`{}` generated the lint crate under {}",
        subcommand.join(" "),
        probe.cache.display()
    );
}

fn driver(name: &str) {
    let probe = Probe::new();
    let [base, ours, theirs] = probe.sides("a\nb\n");
    drop(probe);
    builds_nothing(&[name, &base, &ours, &theirs, "x.txt", "7"]);
}

#[test]
fn the_append_driver_merges_with_no_generation_and_no_build() {
    let probe = Probe::new();
    let [base, ours, theirs] = probe.sides("a\nb\n");
    let out = probe.engine(&["merge-additions", &base, &ours, &theirs, "x.txt", "7"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fs::read_to_string(Path::new(&ours)).unwrap(), "a\nb\nc\n");
    assert_eq!(probe.cargo_calls(), "");
    assert!(!probe.generated());
}

#[test]
fn the_registry_driver_builds_nothing() {
    driver("merge-registry");
}

#[test]
fn the_round_meta_driver_builds_nothing() {
    driver("merge-round-meta");
}

#[test]
fn the_test_history_driver_builds_nothing() {
    driver("merge-test-history");
}

#[test]
fn listing_the_sketches_builds_nothing() {
    builds_nothing(&["sketches"]);
}

#[test]
fn the_test_unit_report_builds_nothing() {
    builds_nothing(&["test-units"]);
}

#[test]
fn the_control_listing_the_lints_does_build() {
    let probe = Probe::new();
    let _ = probe.engine(&["lints"]);
    assert_ne!(probe.cargo_calls(), "", "the probe saw no cargo call from a command that reads the pack");
    assert!(probe.generated(), "the probe saw no generated store from a command that reads the pack");
}
