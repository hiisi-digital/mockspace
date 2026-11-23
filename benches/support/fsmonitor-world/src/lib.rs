//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The world the fsmonitor benches time git in: a consumer whose research directory moved to a data
//! ref, made by the engine named in `MOCKSPACE_BENCH_ENGINE`, and one clone of it per arm, set up by
//! that engine's `data-refs instantiate`.
//!
//! An arm is a place and a setting: on `dev` or at the commit just before the migration, with the
//! monitor in `core.fsmonitor` or with it unset. The hooks are there in every arm, so what differs
//! between an arm with the monitor and the same arm without is the monitor alone. Every arm runs in a
//! process of its own, so a process builds its world once, outside the timed block, in a directory
//! under the temp directory that the next process removes where the process that made it is gone.
//!
//! Every program runs with the environment of [`command`]: no config of the machine's, the copies
//! the engine keeps and its gate inside the world, and the gate never activated.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Where an arm runs git, and whether the monitor is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Arm {
    /// On `dev`, `core.fsmonitor` unset.
    NoneDev,
    /// On `dev`, the monitor set.
    MonitorDev,
    /// At the commit just before the migration, `core.fsmonitor` unset.
    NoneOld,
    /// At the commit just before the migration, the monitor set: it copies the instance before every
    /// git command.
    MonitorOld,
}

impl Arm {
    fn name(self) -> &'static str {
        match self {
            Arm::NoneDev => "none-dev",
            Arm::MonitorDev => "monitor-dev",
            Arm::NoneOld => "none-old",
            Arm::MonitorOld => "monitor-old",
        }
    }

    fn monitor(self) -> bool {
        matches!(self, Arm::MonitorDev | Arm::MonitorOld)
    }

    fn old(self) -> bool {
        matches!(self, Arm::NoneOld | Arm::MonitorOld)
    }
}

/// A clone of the world, set up for one arm, and the two commits a checkout goes between there.
pub struct Clone {
    pub arm:  Arm,
    pub root: PathBuf,
    base:     PathBuf,
    commits:  [String; 2],
}

/// The files the research directory holds, and the bytes of each.
const RESEARCH_FILES: usize = 1000;
const FILE_BYTES: usize = 4096;
/// The source files the branch keeps.
const CRATES: usize = 50;

fn engine() -> PathBuf {
    let Some(p) = std::env::var_os("MOCKSPACE_BENCH_ENGINE") else {
        panic!("MOCKSPACE_BENCH_ENGINE names no engine; set it to the absolute path of a built `mockspace` binary");
    };
    let p = PathBuf::from(p);
    assert!(p.is_absolute() && p.is_file(), "MOCKSPACE_BENCH_ENGINE is not an absolute path to a file: {}", p.display());
    p
}

/// `program`, run with the world's environment.
pub fn command(base: &Path, program: impl AsRef<std::ffi::OsStr>, dir: &Path) -> Command {
    let mut c = Command::new(program);
    c.current_dir(dir)
        .env("HOME", base.join("home"))
        .env("XDG_CONFIG_HOME", base.join("home/config"))
        .env("XDG_CACHE_HOME", base.join("home/cache"))
        .env("MOCKSPACE_KEPT_DIR", base.join("kept"))
        .env("MOCKSPACE_NO_AUTO_ACTIVATE", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("MOCKSPACE_IN_ENGINE");
    for (k, v) in [("NAME", "bench"), ("EMAIL", "bench@example.com")] {
        c.env(format!("GIT_AUTHOR_{k}"), v).env(format!("GIT_COMMITTER_{k}"), v);
    }
    c
}

fn must(base: &Path, program: impl AsRef<std::ffi::OsStr>, dir: &Path, args: &[&str]) -> String {
    let program = program.as_ref().to_owned();
    let o = command(base, &program, dir).args(args).output().unwrap_or_else(|e| panic!("{program:?} {args:?}: {e}"));
    assert!(
        o.status.success(),
        "{program:?} {args:?} in {}: {}{}",
        dir.display(),
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// `len` bytes from a fixed generator, the same on every run.
fn bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (0 .. len)
        .map(|_| {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as u8
        })
        .collect()
}

/// Takes out the worlds of processes that are gone.
fn prune(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.split('-').next()).and_then(|p| p.parse::<u32>().ok()) else {
            continue;
        };
        if !Path::new("/proc").join(pid.to_string()).exists() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// The consumer, migrated and pushed: returns the remote and the commit just before the migration.
fn consumer(base: &Path) -> (PathBuf, String) {
    let remote = base.join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    must(base, "git", &remote, &["init", "-q", "--bare", "-b", "dev"]);
    must(base, "git", base, &["clone", "-q", remote.to_str().unwrap(), "seed"]);
    let seed = base.join("seed");
    must(base, "git", &seed, &["checkout", "-q", "-B", "dev"]);
    write(&seed.join(".gitignore"), b"target/\n");
    write(&seed.join("mock/mockspace.toml"), b"project_name = \"bench\"\ncrate_prefix = \"bench\"\n");
    write(&seed.join("mock/Cargo.toml"), b"[workspace]\nmembers = []\nresolver = \"2\"\n");
    for k in 0 .. CRATES {
        write(&seed.join(format!("mock/crates/c{k}/src/lib.rs")), format!("pub fn c{k}() {{}}\n").as_bytes());
    }
    for i in 0 .. RESEARCH_FILES {
        write(&seed.join(format!("mock/research/d{}/f{i}.md", i % 20)), &bytes(i as u64, FILE_BYTES));
    }
    must(base, "git", &seed, &["add", "-A"]);
    must(base, "git", &seed, &["commit", "-q", "-m", "docs: the research"]);
    for i in (0 .. RESEARCH_FILES).step_by(10) {
        write(&seed.join(format!("mock/research/d{}/f{i}.md", i % 20)), &bytes(i as u64 + 7, FILE_BYTES));
    }
    must(base, "git", &seed, &["commit", "-q", "-a", "-m", "docs: revise the research"]);
    let pre = must(base, "git", &seed, &["rev-parse", "HEAD"]);
    must(base, &engine(), &seed, &["--dir", seed.join("mock").to_str().unwrap(), "migrate", "directory-to-ref", "--namespace", "research", "--path", "research"]);
    must(base, "git", &seed, &["add", "-f", "--", "mock/mockspace.toml"]);
    must(base, "git", &seed, &["add", "-A"]);
    must(base, "git", &seed, &["commit", "-q", "-m", "chore: move the research to its ref"]);
    write(&seed.join("mock/crates/c0/src/lib.rs"), b"pub fn c0() { let after = 1; }\n");
    must(base, "git", &seed, &["commit", "-q", "-a", "-m", "feat: a commit after the migration"]);
    must(base, "git", &seed, &["push", "-q", "--no-verify", "origin", "dev"]);
    (remote, pre)
}

static WORLD: OnceLock<Clone> = OnceLock::new();

/// The clone of this process's arm, made on the first call. A process serves one arm.
pub fn clone(arm: Arm) -> &'static Clone {
    let c = WORLD.get_or_init(|| {
        let root = std::env::temp_dir().join("mockspace-bench-fsmonitor");
        prune(&root);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let base = root.join(format!("{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(base.join("home")).unwrap();
        let base = base.canonicalize().unwrap();
        let (remote, pre) = consumer(&base);
        must(&base, "git", &base, &["clone", "-q", remote.to_str().unwrap(), arm.name()]);
        let root = base.join(arm.name());
        must(&base, &engine(), &root, &["--dir", root.join("mock").to_str().unwrap(), "data-refs", "instantiate"]);
        if !arm.monitor() {
            must(&base, "git", &root, &["config", "--unset", "core.fsmonitor"]);
        }
        let commits = if arm.old() {
            must(&base, "git", &root, &["checkout", "-q", &pre]);
            [pre.clone(), must(&base, "git", &root, &["rev-parse", &format!("{pre}~1")])]
        } else {
            let tip = must(&base, "git", &root, &["rev-parse", "HEAD"]);
            [tip.clone(), must(&base, "git", &root, &["rev-parse", &format!("{tip}~1")])]
        };
        // the first command there settles the index and the hooks' copies
        must(&base, "git", &root, &["status", "--porcelain"]);
        Clone { arm, root, base, commits }
    });
    assert_eq!(c.arm, arm, "a process serves one arm");
    c
}

/// `git status`, as a person runs it.
pub fn status(c: &Clone) -> bool {
    command(&c.base, "git", &c.root).args(["status", "--porcelain"]).output().is_ok_and(|o| o.status.success())
}

static AT_SECOND: AtomicBool = AtomicBool::new(false);

/// A plain `git checkout` of the other of the arm's two commits: on `dev`, between its tip and the
/// migration's commit; at the old commit, between it and the commit before.
pub fn checkout(c: &Clone) -> bool {
    let to_second = !AT_SECOND.load(Ordering::Relaxed);
    let to = &c.commits[usize::from(to_second)];
    let ok = command(&c.base, "git", &c.root).args(["checkout", "-q", to]).output().is_ok_and(|o| o.status.success());
    if ok {
        AT_SECOND.store(to_second, Ordering::Relaxed);
    }
    ok
}
