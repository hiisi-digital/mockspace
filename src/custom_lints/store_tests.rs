use std::time::{Duration, SystemTime};

use super::*;
use crate::custom_lints::gen_crate_name;

const DEP: &str = r#"{ package = "mockspace-lint-rules", git = "https://example.invalid/m.git", rev = "aaaaaaa" }"#;
const PACK: &str = r#"{ git = "https://example.invalid/pack.git", rev = "1111111" }"#;
const TIP_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TIP_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

pub(super) fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A consumer clone with a remote, and a second worktree of it.
struct Consumer {
    _tmp:  tempfile::TempDir,
    main:  PathBuf,
    other: PathBuf,
}

fn consumer(remote: &str) -> Consumer {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("clone");
    std::fs::create_dir_all(main.join("mock")).unwrap();
    git(&main, &["init", "-q"]);
    git(&main, &["remote", "add", "origin", remote]);
    std::fs::write(main.join("mock").join("keep"), "").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "one"]);
    let other = tmp.path().join("wt").join("elsewhere");
    git(&main, &[
        "worktree",
        "add",
        "-q",
        other.to_str().unwrap(),
        "-b",
        "other",
    ]);
    Consumer {
        _tmp: tmp,
        main,
        other,
    }
}

/// The manifest path as a run reaches it: the packs resolved first, once, and
/// the key made from what that gave.
fn path_of(
    base: &Path,
    mock: &Path,
    dep: &str,
    packs: &[(String, String)],
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> PathBuf {
    let packs = resolve_packs(packs, &base.join(".pack-pins"), resolve);
    lints_manifest_path(base, &Config::from_dir(mock), dep, &packs, None)
}

fn no_remote(_: &str, _: &str) -> Result<String, String> {
    panic!("a rev-pinned pack must not ask the remote")
}

#[test]
fn two_worktrees_of_one_consumer_at_the_same_revs_share_one_manifest_path() {
    let c = consumer("ssh://git@example.invalid/o/consumer.git");
    let base = Path::new("/shared");
    let packs = vec![("pack".to_string(), PACK.to_string())];
    let a = path_of(base, &c.main.join("mock"), DEP, &packs, &no_remote);
    let b = path_of(base, &c.other.join("mock"), DEP, &packs, &no_remote);
    assert_eq!(a, b);
    assert!(
        a.starts_with(base),
        "the manifest must sit outside both worktrees: {}",
        a.display()
    );
    assert!(!a.starts_with(&c.main) && !a.starts_with(&c.other));
}

#[test]
fn a_clone_at_another_path_with_the_same_remote_shares_it_too() {
    let a = consumer("https://example.invalid/o/consumer.git");
    let b = consumer("ssh://git@example.invalid/o/consumer");
    let base = Path::new("/shared");
    assert_eq!(
        path_of(base, &a.main.join("mock"), DEP, &[], &no_remote),
        path_of(base, &b.main.join("mock"), DEP, &[], &no_remote),
    );
}

#[test]
fn a_changed_rev_gets_a_different_manifest_path() {
    let c = consumer("ssh://git@example.invalid/o/consumer.git");
    let base = Path::new("/shared");
    let mock = c.main.join("mock");
    let packs = vec![("pack".to_string(), PACK.to_string())];
    let here = path_of(base, &mock, DEP, &packs, &no_remote);

    let engine = DEP.replace("aaaaaaa", "bbbbbbb");
    assert_ne!(
        here,
        path_of(base, &mock, &engine, &packs, &no_remote),
        "engine rev"
    );

    let moved = vec![("pack".to_string(), PACK.replace("1111111", "2222222"))];
    assert_ne!(
        here,
        path_of(base, &mock, DEP, &moved, &no_remote),
        "pack rev"
    );

    let none: Vec<(String, String)> = Vec::new();
    assert_ne!(
        here,
        path_of(base, &mock, DEP, &none, &no_remote),
        "pack set"
    );
}

#[test]
fn a_branch_pack_whose_tip_moved_gets_a_different_manifest_path() {
    let c = consumer("ssh://git@example.invalid/o/consumer.git");
    let mock = c.main.join("mock");
    let packs = vec![(
        "pack".to_string(),
        r#"{ git = "https://example.invalid/pack.git", branch = "dev" }"#.to_string(),
    )];
    // each call gets a pin cache of its own, so each reads the tip fresh: the
    // two resolvers are the remote at two moments
    let key = |tip: &'static str| {
        let base = tempfile::tempdir().unwrap();
        let p = path_of(base.path(), &mock, DEP, &packs, &move |_: &str, _: &str| {
            Ok(tip.to_string())
        });
        p.parent().unwrap().file_name().unwrap().to_owned()
    };
    assert_ne!(key(TIP_A), key(TIP_B));
    assert_eq!(
        key(TIP_A),
        key(TIP_A),
        "the control: the same tip is the same key"
    );
}

#[test]
fn another_consumer_does_not_share_the_manifest_path() {
    let a = consumer("https://example.invalid/o/one.git");
    let b = consumer("https://example.invalid/o/two.git");
    let base = Path::new("/shared");
    assert_ne!(
        path_of(base, &a.main.join("mock"), DEP, &[], &no_remote),
        path_of(base, &b.main.join("mock"), DEP, &[], &no_remote),
    );
}

#[test]
fn the_package_name_is_the_key_and_never_the_worktree() {
    let a = gen_crate_name(Path::new("/shared/00ab12cd34ef5678"));
    assert_eq!(a, "mockspace-lints-00ab12cd34ef5678");
    assert_eq!(a, gen_crate_name(Path::new("/elsewhere/00ab12cd34ef5678")));
}

#[test]
fn distinct_keys_are_distinct_package_names() {
    // Two keys sharing a target directory are two packages only if their names
    // differ, or cargo builds one over the other.
    let a = gen_crate_name(Path::new("/shared/00ab12cd34ef5678"));
    let b = gen_crate_name(Path::new("/shared/00ab12cd34ef5679"));
    assert_ne!(a, b);
}

#[test]
fn the_name_sanitiser_changes_what_cargo_would_refuse_and_nothing_a_real_key_holds() {
    // The sanitising arm: a directory name with a dot or a space is not a valid
    // part of a package name.
    assert_eq!(
        gen_crate_name(Path::new("/shared/ab.cd ef")),
        "mockspace-lints-ab-cd-ef"
    );
    // And the reason it is safe to have: the sanitiser maps `a.b` and `a-b` to
    // one name, which would make two keys one package, so it must be the
    // identity on every key a manifest path actually carries.
    let c = consumer("ssh://git@example.invalid/o/consumer.git");
    let manifest = path_of(
        Path::new("/shared"),
        &c.main.join("mock"),
        DEP,
        &[],
        &no_remote,
    );
    let key = manifest
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy();
    assert!(is_key_name(&key), "{key}");
    assert_eq!(
        gen_crate_name(manifest.parent().unwrap()),
        format!("mockspace-lints-{key}")
    );
}

#[test]
fn every_spelling_of_one_remote_normalises_alike() {
    let same = [
        "https://github.com/O/R.git",
        "ssh://git@github.com/o/r",
        "git@github.com:o/r.git",
        "https://user@github.com/o/r/",
    ];
    for url in same {
        assert_eq!(normalise_remote(url), "github.com/o/r", "{url}");
    }
    assert_ne!(
        normalise_remote("https://github.com/o/other.git"),
        "github.com/o/r"
    );
}

#[test]
fn a_relative_path_pack_is_read_from_the_old_location_and_written_in_its_shortest_form() {
    let from = Path::new("/w/mock/target/mockspace-lints");
    let rel = absolutize_path_pack(r#"{ path = "../../crates/p" }"#, from);
    assert_eq!(rel, r#"{ path = "/w/mock/crates/p" }"#);
    // The control: a path with nothing to fold is joined and otherwise as it came.
    let down = absolutize_path_pack(r#"{ path = "crates/p" }"#, from);
    assert_eq!(
        down,
        r#"{ path = "/w/mock/target/mockspace-lints/crates/p" }"#
    );
    for untouched in [
        r#"{ path = "/abs/p" }"#,
        r#"{ git = "https://example.invalid/p.git", rev = "abc" }"#,
        r#""0.1""#,
    ] {
        assert_eq!(absolutize_path_pack(untouched, from), untouched);
    }
}

fn os(s: &str) -> Option<OsString> {
    Some(OsString::from(s))
}

#[test]
fn the_store_follows_a_shared_target_dir_and_a_relative_one_is_read_against_the_repo() {
    let root = Path::new("/repo");
    assert_eq!(
        base_from(os("/shared/target"), os("/cache"), os("/home/u"), root).unwrap(),
        PathBuf::from("/shared/target/mockspace-lints")
    );
    assert_eq!(
        base_from(os("build"), None, None, root).unwrap(),
        PathBuf::from("/repo/build/mockspace-lints"),
        "a relative target dir must not move with the directory the engine runs in"
    );
    // set but empty is no target dir, so the cache is used
    assert_eq!(
        base_from(os(""), os("/cache"), None, root).unwrap(),
        PathBuf::from("/cache/mockspace/lints")
    );
}

#[test]
fn without_a_target_dir_the_store_is_in_the_users_cache() {
    let root = Path::new("/repo");
    assert_eq!(
        base_from(None, os("/xdg"), os("/home/u"), root).unwrap(),
        PathBuf::from("/xdg/mockspace/lints")
    );
    assert_eq!(
        base_from(None, None, os("/home/u"), root).unwrap(),
        PathBuf::from("/home/u/.cache/mockspace/lints")
    );
    // a relative cache home is invalid under the XDG specification
    assert_eq!(
        base_from(None, os("relative"), os("/home/u"), root).unwrap(),
        PathBuf::from("/home/u/.cache/mockspace/lints")
    );
}

#[test]
fn a_machine_with_no_home_is_refused_rather_than_given_a_shared_temp_directory() {
    let root = Path::new("/repo");
    for (xdg, home) in [(None, None), (os("relative"), os("")), (None, os("relative"))] {
        let err = base_from(None, xdg, home, root).unwrap_err();
        for named in ["CARGO_TARGET_DIR", "XDG_CACHE_HOME", "HOME"] {
            assert!(err.contains(named), "the error must name {named}: {err}");
        }
    }
}

#[test]
fn the_crate_is_tested_only_by_the_worktree_that_last_wrote_it() {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("shared").join("k");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("Cargo.toml"), "").unwrap();
    let one = tmp.path().join("one").join("mock");
    let two = tmp.path().join("two").join("mock");
    assert_eq!(
        generated_dir_of(&one),
        Generated::Absent,
        "nothing generated yet"
    );
    record_owner(&gen_dir, &one, None).unwrap();
    assert_eq!(generated_dir_of(&one), Generated::Yours(gen_dir.clone()));
    record_owner(&gen_dir, &two, None).unwrap();
    assert_eq!(generated_dir_of(&two), Generated::Yours(gen_dir.clone()));
    assert_eq!(
        generated_dir_of(&one),
        Generated::Taken {
            dir:   gen_dir.clone(),
            owner: two.display().to_string(),
        },
        "rewritten by the other worktree since, which is not the same as never written"
    );
    // and a crate that was removed is absent, not taken
    std::fs::remove_dir_all(&gen_dir).unwrap();
    assert_eq!(generated_dir_of(&two), Generated::Absent);
}

// --- the lock -------------------------------------------------------------------

#[test]
fn the_lock_is_held_until_it_is_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("0123456789abcdef");
    let held = lock_dir(&dir).unwrap();
    let peer = File::open(dir.join(LOCK)).unwrap();
    assert!(
        peer.try_lock().is_err(),
        "a second run must wait while the first holds it"
    );
    drop(held);
    assert!(
        peer.try_lock().is_ok(),
        "the control: released once dropped"
    );
}

#[test]
fn a_directory_that_cannot_be_locked_is_named_with_the_way_out() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a-file");
    std::fs::write(&file, "").unwrap();
    // a directory below a file cannot be made
    let dir = file.join("0123456789abcdef");
    let err = lock_dir(&dir).unwrap_err();
    assert!(err.contains(&dir.display().to_string()), "{err}");
    assert!(
        err.contains("XDG_CACHE_HOME") && err.contains("CARGO_TARGET_DIR"),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn a_lock_on_a_file_that_was_replaced_is_not_the_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(LOCK);
    std::fs::write(&path, "").unwrap();
    let file = File::open(&path).unwrap();
    assert!(
        is_the_file_at(&file, &path),
        "the control: the file at the path"
    );
    // what a reaper does, and a peer's recreation of it
    std::fs::remove_file(&path).unwrap();
    assert!(!is_the_file_at(&file, &path), "removed");
    std::fs::write(&path, "").unwrap();
    assert!(!is_the_file_at(&file, &path), "recreated is another file");
}

// --- the reaper -----------------------------------------------------------------

const DAYS: Duration = Duration::from_secs(24 * 60 * 60);

/// A key directory as a run leaves it, with a build in it, its owner at
/// `owner` and last used `used_ago` ago.
fn key_dir(base: &Path, key: &str, owner: &Path, used_ago: Duration) -> PathBuf {
    let dir = base.join(key);
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(dir.join("target").join("build"), "x").unwrap();
    std::fs::write(dir.join(LOCK), "").unwrap();
    std::fs::write(dir.join(OWNER), owner.display().to_string()).unwrap();
    let used = dir.join(USED);
    std::fs::write(&used, "0").unwrap();
    File::options()
        .write(true)
        .open(&used)
        .unwrap()
        .set_modified(SystemTime::now() - used_ago)
        .unwrap();
    dir
}

const KEPT: &str = "00000000000000aa";
const K1: &str = "0000000000000001";
const K2: &str = "0000000000000002";
const K3: &str = "0000000000000003";

fn reap_now(base: &Path, keep: &Path) -> Vec<PathBuf> {
    reap(base, keep, SystemTime::now(), STALE_AFTER)
}

#[test]
fn a_key_whose_owner_worktree_is_gone_is_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let alive = tmp.path().join("wt");
    std::fs::create_dir_all(&alive).unwrap();
    let gone = key_dir(
        &base,
        K1,
        &tmp.path().join("deleted-worktree"),
        Duration::ZERO,
    );
    let live = key_dir(&base, K2, &alive, Duration::ZERO);

    assert_eq!(reap_now(&base, &base.join(KEPT)), vec![gone.clone()]);
    assert!(!gone.exists());
    assert!(
        live.exists(),
        "the control: a key whose owner exists and is recent stays"
    );
}

#[test]
fn a_key_unused_past_the_window_is_removed_though_its_owner_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let alive = tmp.path().join("wt");
    std::fs::create_dir_all(&alive).unwrap();
    let old = key_dir(&base, K1, &alive, STALE_AFTER + DAYS);
    let recent = key_dir(&base, K2, &alive, STALE_AFTER - DAYS);

    assert_eq!(reap_now(&base, &base.join(KEPT)), vec![old.clone()]);
    assert!(!old.exists());
    assert!(recent.exists(), "just inside the window stays");
}

#[test]
fn the_key_this_run_is_using_is_never_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    // abandoned on both counts, and ours
    let ours = key_dir(&base, KEPT, &tmp.path().join("deleted"), STALE_AFTER + DAYS);
    assert!(reap_now(&base, &ours).is_empty());
    assert!(ours.exists());
}

#[test]
fn a_key_in_use_by_another_run_survives_and_goes_once_it_is_released() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let busy = key_dir(&base, K1, &tmp.path().join("deleted"), STALE_AFTER + DAYS);

    let held = lock_dir(&busy).unwrap();
    assert!(
        reap_now(&base, &base.join(KEPT)).is_empty(),
        "removed a key a run holds the lock of"
    );
    assert!(busy.join("target").join("build").exists());

    drop(held);
    // the control, which says the lock is what saved it
    assert_eq!(reap_now(&base, &base.join(KEPT)), vec![busy.clone()]);
    assert!(!busy.exists());
}

#[test]
fn a_run_that_waited_on_a_removed_key_starts_again_in_a_fresh_one() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let dir = key_dir(&base, K1, &tmp.path().join("deleted"), STALE_AFTER + DAYS);

    // the run opens the lock file before the reaper takes it, and blocks on it
    let before = File::open(dir.join(LOCK)).unwrap();
    assert_eq!(reap_now(&base, &base.join(KEPT)), vec![dir.clone()]);
    assert!(!is_the_file_at(&before, &dir.join(LOCK)));

    // what it does next is lock_dir's loop: a new directory, a new lock
    let fresh = lock_dir(&dir).unwrap();
    assert!(dir.join(LOCK).is_file());
    drop(fresh);
}

#[test]
fn only_key_named_directories_are_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    std::fs::create_dir_all(base.join(".pack-pins")).unwrap();
    std::fs::create_dir_all(base.join("notes")).unwrap();
    std::fs::create_dir_all(base.join("0000000000000A01")).unwrap(); // upper case is not ours
    std::fs::write(base.join("0000000000000004"), "a file").unwrap();
    key_dir(&base, K3, &tmp.path().join("deleted"), Duration::ZERO);

    let removed = reap_now(&base, &base.join(KEPT));
    assert_eq!(removed, vec![base.join(K3)]);
    for kept in [".pack-pins", "notes", "0000000000000A01", "0000000000000004"] {
        assert!(base.join(kept).exists(), "{kept}");
    }
}

#[test]
fn a_directory_a_peer_is_still_creating_is_not_taken() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    // no lock file, no owner, no stamp: between `create_dir_all` and the lock
    let new = base.join(K1);
    std::fs::create_dir_all(&new).unwrap();
    assert!(reap_now(&base, &base.join(KEPT)).is_empty());
    assert!(new.exists());

    // the control: the same shape, left alone past the window, is junk
    let later = SystemTime::now() + STALE_AFTER + DAYS;
    assert_eq!(reap(&base, &base.join(KEPT), later, STALE_AFTER), vec![
        new.clone()
    ]);
}

#[test]
fn a_missing_store_reaps_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(reap_now(&tmp.path().join("never-made"), &tmp.path().join("x")).is_empty());
}

#[test]
fn a_run_stamps_its_key_as_used() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let alive = tmp.path().join("wt");
    std::fs::create_dir_all(&alive).unwrap();
    let dir = key_dir(&base, K1, &alive, STALE_AFTER + DAYS);
    assert!(abandoned(&dir, SystemTime::now(), STALE_AFTER));
    touch_used(&dir).unwrap();
    assert!(
        !abandoned(&dir, SystemTime::now(), STALE_AFTER),
        "a key a run has just used is not abandoned"
    );
}

#[test]
fn a_tool_scoped_build_has_a_manifest_path_of_its_own() {
    let c = consumer("ssh://git@example.invalid/o/consumer.git");
    let cfg = Config::from_dir(&c.main.join("mock"));
    let base = Path::new("/shared");
    let whole = lints_manifest_path(base, &cfg, DEP, &[], None);
    let add = lints_manifest_path(base, &cfg, DEP, &[], Some("ticket-add"));
    let mv = lints_manifest_path(base, &cfg, DEP, &[], Some("ticket-move"));
    assert_ne!(whole, add, "a scoped build must not overwrite the whole one");
    assert_ne!(add, mv, "two scopes are two builds");
}

#[test]
fn a_scoped_build_never_moves_the_pointer_the_test_run_reads() {
    // `mock test` takes the lints tree from the pointer. A one-tool crate behind it has no lint modules, so
    // the run would be green over nothing, the outcome entry/test.rs refuses. Mutation: the scoped build
    // records the pointer like the whole one, and the second assertion names the one-tool directory.
    let tmp = tempfile::tempdir().unwrap();
    let whole = tmp.path().join("shared").join("whole");
    let scoped = tmp.path().join("shared").join("scoped");
    for d in [&whole, &scoped] {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("Cargo.toml"), "").unwrap();
    }
    let mock = tmp.path().join("one").join("mock");
    record_owner(&whole, &mock, None).unwrap();
    record_owner(&scoped, &mock, Some("ticket-add")).unwrap();
    assert_eq!(
        generated_dir_of(&mock),
        Generated::Yours(whole.clone()),
        "the tests still run the whole build's lints"
    );
    // the control: the scoped directory is still owned and stamped, so the reaper and a later build see it
    assert_eq!(
        std::fs::read_to_string(scoped.join(OWNER)).unwrap(),
        mock.display().to_string()
    );
}

#[test]
fn a_whole_build_keeps_the_key_it_always_had() {
    // The key names a directory that outlives the binary. A golden value, so a change to the text it hashes
    // is a failure here and not a quietly orphaned build per repository. Packs are present, since the
    // scope line is appended after them.
    let packs = vec![(
        "some-pack".to_string(),
        r#"{ git = "https://example.invalid/pack.git", rev = "1111111" }"#.to_string(),
    )];
    let key = lint_crate_key("remote:example.invalid/o/consumer", Path::new("mock"), DEP, &packs, None);
    assert_eq!(format!("{key:016x}"), GOLDEN_WHOLE_KEY);
    let scoped = lint_crate_key("remote:example.invalid/o/consumer", Path::new("mock"), DEP, &packs, Some("t"));
    assert_ne!(key, scoped);
}
const GOLDEN_WHOLE_KEY: &str = "3040726b2cef6a9f"; // fnv1a of the original key text, computed apart from the code
