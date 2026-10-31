use std::ffi::OsString;

use super::*;
use crate::custom_lints::gen_crate_name;
use crate::custom_lints::store::support::consumer;

const DEP: &str = r#"{ package = "mockspace-lint-rules", git = "https://example.invalid/m.git", rev = "aaaaaaa" }"#;
const PACK: &str = r#"{ git = "https://example.invalid/pack.git", rev = "1111111" }"#;
const TIP_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TIP_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

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
