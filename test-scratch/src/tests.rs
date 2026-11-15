//! Tests for the scratch root.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, SystemTime};

use crate::{SCRATCH_DIR, claim, reap, root};

const STALE: Duration = Duration::from_secs(6 * 60 * 60);

fn age(d: &Path) {
    let long_ago = SystemTime::now() - Duration::from_secs(7 * 60 * 60);
    std::fs::File::open(d).unwrap().set_modified(long_ago).unwrap();
}

fn fixture(name: &str) -> PathBuf {
    let base = root().join(name);
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

/// The id of a process that has ended and been waited on.
fn dead_pid() -> u32 {
    let mut c = Command::new("true").spawn().unwrap();
    c.wait().unwrap();
    c.id()
}

struct Live(Child);
impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn an_old_root_of_a_dead_process_goes_and_a_fresh_one_and_our_own_stay() {
    let base = fixture("reap-dead");
    let old = base.join(dead_pid().to_string());
    let fresh = base.join(dead_pid().to_string());
    let own = base.join(std::process::id().to_string());
    for d in [&old, &fresh, &own] {
        std::fs::create_dir_all(d.join("x")).unwrap();
    }
    age(&old);
    age(&own);
    reap(&base, STALE);
    assert!(!old.exists(), "an old root of a dead process is removed");
    assert!(fresh.exists(), "a root added to within the stale time stays");
    assert!(own.exists(), "this process's own root stays whatever its age");
}

#[test]
fn an_old_root_of_a_live_process_stays() {
    let live = Live(Command::new("sleep").arg("60").spawn().unwrap());
    let base = fixture("reap-live");
    let theirs = base.join(live.0.id().to_string());
    std::fs::create_dir_all(theirs.join("work")).unwrap();
    age(&theirs);
    reap(&base, STALE);
    assert!(theirs.join("work").exists(), "a root whose process still runs is never removed");
}

#[test]
fn a_claimed_root_starts_empty_whatever_a_dead_run_left_in_it() {
    let base = fixture("claim");
    let reused = base.join(dead_pid().to_string());
    std::fs::create_dir_all(reused.join("left-over")).unwrap();
    std::fs::write(reused.join("left-over.txt"), "old").unwrap();
    claim(&reused);
    assert!(reused.is_dir(), "the claimed root exists");
    assert_eq!(std::fs::read_dir(&reused).unwrap().count(), 0, "the claimed root is empty");
}

#[test]
fn scratch_lands_under_this_process_root() {
    let r = root();
    assert!(r.is_dir());
    assert!(r.ends_with(Path::new(SCRATCH_DIR).join(std::process::id().to_string())));
    let t = crate::tempdir().unwrap();
    assert!(t.path().starts_with(&r), "a scratch dir is made under the root");
}
