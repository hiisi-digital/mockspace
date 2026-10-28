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
fn a_relative_path_pack_is_read_from_the_old_location_and_nothing_else_moves() {
    let from = Path::new("/w/mock/target/mockspace-lints");
    let rel = absolutize_path_pack(r#"{ path = "../../crates/p" }"#, from);
    assert!(
        rel.contains(r#"path = "/w/mock/target/mockspace-lints/../../crates/p""#),
        "{rel}"
    );
    for untouched in [
        r#"{ path = "/abs/p" }"#,
        r#"{ git = "https://example.invalid/p.git", rev = "abc" }"#,
        r#""0.1""#,
    ] {
        assert_eq!(absolutize_path_pack(untouched, from), untouched);
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
    assert_eq!(generated_dir_of(&one), None, "nothing generated yet");
    record_owner(&gen_dir, &one).unwrap();
    assert_eq!(generated_dir_of(&one), Some(gen_dir.clone()));
    record_owner(&gen_dir, &two).unwrap();
    assert_eq!(generated_dir_of(&two), Some(gen_dir));
    assert_eq!(
        generated_dir_of(&one),
        None,
        "rewritten by the other worktree since"
    );
}
