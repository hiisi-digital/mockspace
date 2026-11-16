//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;
use crate::Reading;

const ARM: &str = "[workspace]\n[package]\nname = \"a\"\nversion = \"0.0.0\"\n[lib]\npath = \"src/lib.rs\"\ncrate-type = [\"cdylib\"]\n[dependencies]\nmockspace-bench-core = { git = \"https://github.com/hiisi-digital/mockspace\", branch = \"dev\" }\n";
const LIB: &str = "[package]\nname = \"s\"\nversion = \"0.0.0\"\n";

/// A repository with a bench tree under `mock/benches/` and whatever else the
/// case puts beside it, every path relative to the repository root.
fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    write(&tmp, &[("mock/benches/bench.toml", "[timing]\n")]);
    write(&tmp, files);
    tmp
}

fn write(tmp: &tempfile::TempDir, files: &[(&str, &str)]) {
    for (path, body) in files {
        let p = tmp.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
}

fn findings(tmp: &tempfile::TempDir) -> Vec<String> {
    let reading = Reading::new(tmp.path());
    bench_kinds(&reading.view(), &tmp.path().join("mock/benches"))
        .into_iter()
        .map(|f| f.to_string())
        .collect()
}

fn plan_of(tmp: &tempfile::TempDir, bench: &str) -> KindPlan {
    let reading = Reading::new(tmp.path());
    plans(&reading.view(), &tmp.path().join("mock/benches"))
        .into_iter()
        .find(|p| p.bench == bench)
        .unwrap()
}

/// Write the lock `mock bench freeze` would, from the plan.
fn freeze(tmp: &tempfile::TempDir, bench: &str) {
    let p = plan_of(tmp, bench);
    let lock = tmp.path().join("mock/benches").join(&p.lock);
    std::fs::write(lock, p.lock_body.unwrap()).unwrap();
}

/// A snapshot built only from its own arm and a support crate of the tree, with
/// the instrument from git, frozen.
fn frozen_snapshot() -> tempfile::TempDir {
    let tmp = repo(&[
        (
            "mock/benches/hash/bench.toml",
            "kind = \"snapshot\"\ntitle = \"h\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        (
            "mock/benches/hash/arms/a/Cargo.toml",
            &format!("{ARM}mix = {{ path = \"../../../support/mix\" }}\n"),
        ),
        ("mock/benches/hash/arms/a/src/lib.rs", "pub fn f() {}\n"),
        ("mock/benches/support/mix/Cargo.toml", LIB),
        ("mock/benches/support/mix/src/lib.rs", "pub fn g() {}\n"),
    ]);
    freeze(&tmp, "hash");
    tmp
}

/// The positive case: a frozen snapshot that builds only from the tree keeps
/// its contract, and its lock records the bench.toml, the arm and the support
/// crate it reaches, and nothing else.
#[test]
fn a_frozen_snapshot_inside_the_tree_keeps_its_contract() {
    let tmp = frozen_snapshot();
    assert_eq!(findings(&tmp), Vec::<String>::new());
    let body = std::fs::read_to_string(tmp.path().join("mock/benches/hash/snapshot.lock")).unwrap();
    let paths: Vec<&str> = body
        .lines()
        .skip(1)
        .map(|l| l.split_once("  ").unwrap().1)
        .collect();
    assert_eq!(paths, [
        "hash/arms/a/Cargo.toml",
        "hash/arms/a/src/lib.rs",
        "hash/bench.toml",
        "support/mix/Cargo.toml",
        "support/mix/src/lib.rs",
    ]);
}

/// A bench with no kind is refused, and the refusal names the kind its reach
/// admits: snapshot where it builds from the tree alone, tracking where an arm
/// reaches outside.
#[test]
fn a_bench_with_no_kind_is_refused_naming_what_it_admits() {
    let tmp = repo(&[
        (
            "mock/benches/iso/bench.toml",
            "title = \"i\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        ("mock/benches/iso/arms/a/src/lib.rs", ""),
        (
            "mock/benches/live/bench.toml",
            "title = \"l\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        (
            "mock/benches/live/arms/a/Cargo.toml",
            &format!("{ARM}engine = {{ path = \"../../../../crates/engine\" }}\n"),
        ),
        ("mock/benches/live/arms/a/src/lib.rs", ""),
        ("mock/crates/engine/Cargo.toml", LIB),
        (
            "mock/benches/odd/bench.toml",
            "kind = \"frozen\"\ntitle = \"o\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        ("mock/benches/odd/arms/a/src/lib.rs", ""),
    ]);
    let f = findings(&tmp);
    assert_eq!(f.len(), 3, "{f:#?}");
    assert!(
        f[0].contains("`iso` declares no `kind`") && f[0].contains("admit `kind = \"snapshot\"`"),
        "{}",
        f[0]
    );
    assert!(
        f[1].contains("`live` declares no `kind`") && f[1].contains("admit `kind = \"tracking\"`"),
        "{}",
        f[1]
    );
    assert!(
        f[2].contains("`kind = \"frozen\"`, which names no kind"),
        "{}",
        f[2]
    );
    assert_eq!(plan_of(&tmp, "iso").admits, Kind::Snapshot);
    assert_eq!(plan_of(&tmp, "live").admits, Kind::Tracking);
}

/// Every way out of the bench tree is found in a snapshot: a path, git, a
/// registry crate, an inherited one, a library source out of the tree, and a
/// literal include. The instrument from git is passed over.
#[test]
fn every_reach_out_of_the_tree_is_refused_in_a_snapshot() {
    let deps = "out = { path = \"../../../../crates/engine\" }\nnotko = { git = \"https://x/notko\", branch = \"dev\" }\nlibc = \"0.2\"\nserde = { workspace = true }\n";
    let tmp = repo(&[
        (
            "mock/benches/hash/bench.toml",
            "kind = \"snapshot\"\ntitle = \"h\"\narms = [\"a\", \"b\"]\npoints = [64]\n",
        ),
        (
            "mock/benches/hash/arms/a/Cargo.toml",
            &format!("{ARM}{deps}"),
        ),
        (
            "mock/benches/hash/arms/a/src/lib.rs",
            "const T: &str = include_str!(\"../../../../../data/table.txt\");\n",
        ),
        (
            "mock/benches/hash/arms/b/Cargo.toml",
            "[package]\nname = \"b\"\nversion = \"0.0.0\"\n[lib]\npath = \"../../../../crates/engine/src/lib.rs\"\n",
        ),
        ("mock/crates/engine/Cargo.toml", LIB),
    ]);
    let f = findings(&tmp);
    assert_eq!(f.len(), 1, "{f:#?}");
    for needle in [
        "takes `out` by path from `../../../../crates/engine`",
        "takes `notko` from git (`https://x/notko`)",
        "takes `libc` from a registry (`\"0.2\"`)",
        "takes `serde` from a workspace it inherits",
        "builds its library from `../../../../crates/engine/src/lib.rs`",
        "reads `../../../../../data/table.txt`",
    ] {
        assert!(f[0].contains(needle), "missing {needle}: {}", f[0]);
    }
    assert!(
        !f[0].contains("mockspace-bench-core"),
        "the instrument is not the code measured: {}",
        f[0]
    );
}

/// A snapshot with no lock is refused, and so is one whose files moved from
/// it: a changed file, a new one, and a gone one each named.
#[test]
fn a_snapshot_is_held_to_its_lock() {
    let tmp = repo(&[
        (
            "mock/benches/hash/bench.toml",
            "kind = \"snapshot\"\ntitle = \"h\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        ("mock/benches/hash/arms/a/src/lib.rs", "pub fn f() {}\n"),
        ("mock/benches/hash/arms/a/src/old.rs", ""),
    ]);
    let f = findings(&tmp);
    assert_eq!(f.len(), 1);
    assert!(
        f[0].contains("has no lock") && f[0].contains("mock bench freeze hash"),
        "{}",
        f[0]
    );

    freeze(&tmp, "hash");
    assert_eq!(findings(&tmp), Vec::<String>::new());

    write(&tmp, &[
        (
            "mock/benches/hash/arms/a/src/lib.rs",
            "pub fn f() { faster() }\n",
        ),
        ("mock/benches/hash/arms/a/src/new.rs", ""),
    ]);
    std::fs::remove_file(tmp.path().join("mock/benches/hash/arms/a/src/old.rs")).unwrap();
    let f = findings(&tmp);
    assert_eq!(f.len(), 1, "{f:#?}");
    for needle in [
        "hash/arms/a/src/lib.rs changed",
        "hash/arms/a/src/new.rs is new",
        "hash/arms/a/src/old.rs is gone",
    ] {
        assert!(f[0].contains(needle), "missing {needle}: {}", f[0]);
    }
}

/// Editing a support crate a snapshot builds from moves the snapshot too, since
/// the snapshot is every file it builds from.
#[test]
fn a_shared_crate_a_snapshot_builds_from_is_frozen_with_it() {
    let tmp = frozen_snapshot();
    write(&tmp, &[(
        "mock/benches/support/mix/src/lib.rs",
        "pub fn g() { 1; }\n",
    )]);
    let f = findings(&tmp);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert!(f[0].contains("support/mix/src/lib.rs changed"), "{}", f[0]);
}

/// A tracking bench that reaches nothing outside the tree is refused, one that
/// reaches the repository's code keeps its contract, and a lock beside a
/// tracking bench is stale.
#[test]
fn a_tracking_bench_reaches_living_code_and_carries_no_lock() {
    let tmp = repo(&[
        (
            "mock/benches/iso/bench.toml",
            "kind = \"tracking\"\ntitle = \"i\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        ("mock/benches/iso/arms/a/src/lib.rs", ""),
        (
            "mock/benches/live/bench.toml",
            "kind = \"tracking\"\ntitle = \"l\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        (
            "mock/benches/live/arms/a/Cargo.toml",
            &format!("{ARM}engine = {{ path = \"../../../../crates/engine\" }}\n"),
        ),
        ("mock/benches/live/arms/a/src/lib.rs", ""),
        ("mock/crates/engine/Cargo.toml", LIB),
    ]);
    let f = findings(&tmp);
    assert_eq!(f.len(), 1, "{f:#?}");
    assert!(
        f[0].contains("tracking bench `iso` builds only from inside the bench tree"),
        "{}",
        f[0]
    );

    write(&tmp, &[("mock/benches/live/snapshot.lock", "# x\n")]);
    let f = findings(&tmp);
    assert_eq!(f.len(), 2, "{f:#?}");
    assert!(
        f[1].contains("lock sits beside tracking bench `live`"),
        "{}",
        f[1]
    );
}

/// A generated arm of a snapshot takes only its own bench's support, so a root
/// support crate reaching living code does not leak into it; a generated arm
/// of a tracking bench takes the root support crates and reaches through them.
#[test]
fn a_generated_snapshot_arm_takes_only_its_own_support() {
    let tmp = repo(&[
        (
            "mock/benches/iso/bench.toml",
            "kind = \"snapshot\"\ntitle = \"i\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        ("mock/benches/iso/arms/a/src/lib.rs", ""),
        ("mock/benches/iso/support/own/Cargo.toml", LIB),
        (
            "mock/benches/live/bench.toml",
            "kind = \"tracking\"\ntitle = \"l\"\narms = [\"a\"]\npoints = [64]\n",
        ),
        ("mock/benches/live/arms/a/src/lib.rs", ""),
        (
            "mock/benches/support/engine-glue/Cargo.toml",
            &format!("{LIB}[dependencies]\nengine = {{ path = \"../../../crates/engine\" }}\n"),
        ),
        ("mock/crates/engine/Cargo.toml", LIB),
    ]);
    freeze(&tmp, "iso");
    assert_eq!(findings(&tmp), Vec::<String>::new());
    let lock = plan_of(&tmp, "iso").lock_body.unwrap();
    assert!(lock.contains("iso/support/own/Cargo.toml"), "{lock}");
    assert!(!lock.contains("engine-glue"), "{lock}");
}

/// Root `[bench.<name>]` sections each declare their kind, build from their
/// `variants/` crates, and lock beside the root file under their own name.
#[test]
fn a_root_section_is_a_bench_with_a_lock_of_its_own_name() {
    let tmp = repo(&[]);
    write(&tmp, &[
        (
            "mock/benches/bench.toml",
            "[bench.hash]\nkind = \"snapshot\"\ntitle = \"h\"\nworkload = \"default\"\nvariants = [\"fnv\"]\nsizes = [64]\n",
        ),
        ("mock/benches/variants/fnv/Cargo.toml", ARM),
        ("mock/benches/variants/fnv/src/lib.rs", ""),
        (
            "mock/benches/variants/fnv/target/release/libfnv.so",
            "built",
        ),
    ]);
    let p = plan_of(&tmp, "hash");
    assert_eq!(p.lock, PathBuf::from("hash.snapshot.lock"));
    let lock = p.lock_body.unwrap();
    assert!(lock.contains("variants/fnv/src/lib.rs"), "{lock}");
    assert!(
        !lock.contains("target"),
        "build output is not the code: {lock}"
    );
}

/// A member `[benchspace]` excludes is no bench, and is not held to anything.
#[test]
fn an_excluded_member_is_not_read() {
    let tmp = repo(&[]);
    write(&tmp, &[
        (
            "mock/benches/bench.toml",
            "[benchspace]\nexclude = [\"old\"]\n",
        ),
        (
            "mock/benches/old/bench.toml",
            "title = \"o\"\narms = [\"a\"]\npoints = [64]\n",
        ),
    ]);
    assert_eq!(findings(&tmp), Vec::<String>::new());
    write(&tmp, &[("mock/benches/bench.toml", "[timing]\n")]);
    assert_eq!(
        findings(&tmp).len(),
        1,
        "the control: included, it is refused"
    );
}
