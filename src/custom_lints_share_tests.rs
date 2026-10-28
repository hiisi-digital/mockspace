use super::*;

const DEP: &str = r#"{ package = "mockspace-lint-rules", git = "https://example.invalid/m.git", rev = "aaaaaaa" }"#;
const PACK: &str = r#"{ git = "https://example.invalid/pack.git", rev = "1111111" }"#;
const TIP_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TIP_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A consumer clone with a remote, and a second worktree of it.
struct Consumer {
    _tmp: tempfile::TempDir,
    main: PathBuf,
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
    git(&main, &["worktree", "add", "-q", other.to_str().unwrap(), "-b", "other"]);
    Consumer { _tmp: tmp, main, other }
}

fn path_of(
    base: &Path,
    mock: &Path,
    dep: &str,
    packs: &[(String, String)],
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> PathBuf {
    lints_manifest_path(base, &Config::from_dir(mock), dep, packs, resolve)
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
    assert!(a.starts_with(base), "the manifest must sit outside both worktrees: {}", a.display());
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
    assert_ne!(here, path_of(base, &mock, &engine, &packs, &no_remote), "engine rev");

    let moved = vec![("pack".to_string(), PACK.replace("1111111", "2222222"))];
    assert_ne!(here, path_of(base, &mock, DEP, &moved, &no_remote), "pack rev");

    let none: Vec<(String, String)> = Vec::new();
    assert_ne!(here, path_of(base, &mock, DEP, &none, &no_remote), "pack set");
}

#[test]
fn a_branch_pack_whose_tip_moved_gets_a_different_manifest_path() {
    let c = consumer("ssh://git@example.invalid/o/consumer.git");
    let base = Path::new("/shared");
    let mock = c.main.join("mock");
    let packs = vec![(
        "pack".to_string(),
        r#"{ git = "https://example.invalid/pack.git", branch = "dev" }"#.to_string(),
    )];
    // the cache for the resolved tip is the machine's, so the tip is read
    // fresh by each call; the two resolvers are the remote at two moments
    let at_a = path_of(base, &mock, DEP, &packs, &|_: &str, _: &str| Ok(TIP_A.to_string()));
    let at_b = path_of(base, &mock, DEP, &packs, &|_: &str, _: &str| Ok(TIP_B.to_string()));
    assert_ne!(at_a, at_b);
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
