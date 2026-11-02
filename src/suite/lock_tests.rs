use super::*;

/// `app` reaches `lib` and, through it, `leaf` and the older `shared 1.0.0`;
/// `tool` reaches the newer `shared 2.0.0` only; `solo` reaches nothing.
fn lock(leaf: &str, shared_old: &str, shared_new: &str, solo: &str) -> String {
    format!(
        r#"
version = 4

[[package]]
name = "app"
version = "0.1.0"
dependencies = ["lib", "shared 1.0.0"]

[[package]]
name = "lib"
version = "0.1.0"
dependencies = ["leaf"]

[[package]]
name = "leaf"
version = "{leaf}"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaa"

[[package]]
name = "shared"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "{shared_old}"

[[package]]
name = "shared"
version = "2.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "{shared_new}"

[[package]]
name = "tool"
version = "0.1.0"
dependencies = ["shared 2.0.0"]

[[package]]
name = "solo"
version = "{solo}"
"#
    )
}

fn digest(text: &str, member: &str) -> Option<[u8; 32]> {
    Lock::parse(text).expect("a lock").closure_digest(member)
}

fn base() -> String {
    lock("1.0.0", "bbb", "ccc", "0.1.0")
}

/// The case the whole file as salt got wrong: a move on one edge of the graph.
#[test]
fn a_move_outside_a_members_closure_leaves_its_digest() {
    let before = base();
    let moved = lock("1.0.0", "bbb", "ccc-moved", "0.2.0");
    for member in ["app", "lib", "leaf"] {
        assert_eq!(digest(&before, member), digest(&moved, member), "{member}");
    }
}

/// And the case that must fail: a move inside it, at any depth, or in the
/// version of a name that exists twice.
#[test]
fn a_move_inside_a_members_closure_moves_its_digest() {
    let before = base();
    // A transitive registry dependency, two edges away from `app`.
    let leaf = lock("1.0.1", "bbb", "ccc", "0.1.0");
    for member in ["app", "lib", "leaf"] {
        assert_ne!(digest(&before, member), digest(&leaf, member), "{member}");
    }
    assert_eq!(digest(&before, "tool"), digest(&leaf, "tool"));
    // The older of two versions of one name moves the member that pins it and
    // not the one that pins the newer.
    let old = lock("1.0.0", "bbb-moved", "ccc", "0.1.0");
    assert_ne!(digest(&before, "app"), digest(&old, "app"));
    assert_eq!(digest(&before, "tool"), digest(&old, "tool"));
    let new = lock("1.0.0", "bbb", "ccc-moved", "0.1.0");
    assert_ne!(digest(&before, "tool"), digest(&new, "tool"));
    assert_eq!(digest(&before, "app"), digest(&new, "app"));
    // A package with nothing to reach moves with its own entry.
    let solo = lock("1.0.0", "bbb", "ccc", "0.2.0");
    assert_ne!(digest(&before, "solo"), digest(&solo, "solo"));
}

#[test]
fn members_with_one_closure_still_have_digests_of_their_own() {
    let text = r#"
[[package]]
name = "a"
version = "0.1.0"
dependencies = ["c"]

[[package]]
name = "b"
version = "0.1.0"
dependencies = ["c"]

[[package]]
name = "c"
version = "0.1.0"
"#;
    assert_ne!(digest(text, "a"), digest(text, "b"));
}

#[test]
fn a_member_the_lock_does_not_hold_has_no_digest() {
    assert_eq!(digest(&base(), "nobody"), None);
}

#[test]
fn text_that_is_no_lockfile_is_not_read() {
    assert_eq!(Lock::parse("# lock v1\n"), None);
    assert_eq!(Lock::parse("not = [toml"), None);
    assert_eq!(Lock::parse("[[package]]\nversion = \"1\"\n"), None);
}

/// A git dependency re-pinned at the same version changes its `source` and
/// nothing else of its entry.
#[test]
fn a_dependency_moved_only_in_its_source_moves_its_dependents() {
    let text = |rev: &str| {
        format!(
            r#"
[[package]]
name = "app"
version = "0.1.0"
dependencies = ["gitdep"]

[[package]]
name = "gitdep"
version = "0.3.0"
source = "git+https://example.invalid/gitdep?branch=dev#{rev}"
"#
        )
    };
    assert_ne!(digest(&text("aaaa"), "app"), digest(&text("bbbb"), "app"));
    assert_eq!(digest(&text("aaaa"), "app"), digest(&text("aaaa"), "app"));
}

/// A lock that reads but holds no entry for a member is salted whole for it, so
/// the member moves with any change to the file.
#[test]
fn a_member_missing_from_a_readable_lock_moves_with_the_whole_file() {
    let mut salt = Salt::new("t");
    let before = base();
    salt.input("mock/Cargo.lock", [1; 32], Some(before.clone()));
    let mut moved = Salt::new("t");
    moved.input(
        "mock/Cargo.lock",
        [2; 32],
        Some(lock("1.0.0", "bbb", "ccc", "0.2.0")),
    );
    assert_ne!(salt.of("nobody"), moved.of("nobody"));
    // A member the lock holds does not, when the move was outside its closure.
    assert_eq!(salt.of("app"), moved.of("app"));
    assert_ne!(salt.of("app"), salt.of("lib"));
}
