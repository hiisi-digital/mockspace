//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

const ARM: &str = "[workspace]\n[package]\nname = \"a\"\nversion = \"0.0.0\"\n[lib]\npath = \"src/lib.rs\"\ncrate-type = [\"cdylib\"]\n[dependencies]\n";
const COMPOSED: &str =
    "# a comment the migration keeps\ntitle = \"t\"\narms = [\"a\"]\npoints = [64]\n";

fn repo(files: &[(&str, &str)]) -> tempfile::TempDir {
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    for (path, body) in files {
        let p = tmp.path().join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    tmp
}

fn benches(tmp: &tempfile::TempDir) -> std::path::PathBuf {
    tmp.path().join("mock/benches")
}

fn findings(tmp: &tempfile::TempDir) -> Vec<String> {
    let reading = Reading::new(tmp.path());
    bench_kinds(&reading.view(), &benches(tmp))
        .into_iter()
        .map(|f| f.to_string())
        .collect()
}

/// A tree written before kinds existed: a bench that builds only from the
/// tree, one that reaches the repository's code, and a root section.
fn unkinded() -> tempfile::TempDir {
    repo(&[
        (
            "mock/benches/bench.toml",
            "[bench.flat]\ntitle = \"F\"\nworkload = \"default\"\nvariants = [\"v\"]\nsizes = [8]\n",
        ),
        ("mock/benches/variants/v/Cargo.toml", ARM),
        ("mock/benches/variants/v/src/lib.rs", ""),
        ("mock/benches/iso/bench.toml", COMPOSED),
        ("mock/benches/iso/arms/a/src/lib.rs", ""),
        ("mock/benches/live/bench.toml", COMPOSED),
        (
            "mock/benches/live/arms/a/Cargo.toml",
            &format!("{ARM}engine = {{ path = \"../../../../crates/engine\" }}\n"),
        ),
        ("mock/benches/live/arms/a/src/lib.rs", ""),
        (
            "mock/crates/engine/Cargo.toml",
            "[package]\nname = \"engine\"\n",
        ),
    ])
}

/// The migration writes the kind each bench admits and freezes the snapshots,
/// after which the tree keeps every contract and the harness loads it; the
/// control is the same tree before the migration, refused for all three.
#[test]
fn the_migration_declares_what_each_bench_admits_and_freezes_the_snapshots() {
    let tmp = unkinded();
    assert_eq!(
        findings(&tmp).len(),
        3,
        "the control: every bench is refused before"
    );
    assert!(mockspace_bench_harness::tree::load(&benches(&tmp)).is_err());

    let said = migrate(tmp.path(), &benches(&tmp), false).unwrap();
    assert!(
        said.iter()
            .any(|l| l.starts_with("iso: `kind = \"snapshot\"`")),
        "{said:#?}"
    );
    assert!(
        said.iter()
            .any(|l| l.starts_with("live: `kind = \"tracking\"`") && l.contains("crates/engine")),
        "{said:#?}"
    );
    assert!(
        said.iter()
            .any(|l| l.starts_with("flat: `kind = \"snapshot\"`")),
        "{said:#?}"
    );

    let iso = std::fs::read_to_string(benches(&tmp).join("iso/bench.toml")).unwrap();
    assert_eq!(iso, format!("kind = \"snapshot\"\n{COMPOSED}"));
    let flat = std::fs::read_to_string(benches(&tmp).join("bench.toml")).unwrap();
    assert!(flat.contains("kind = \"snapshot\""), "{flat}");
    assert!(benches(&tmp).join("iso/snapshot.lock").is_file());
    assert!(benches(&tmp).join("flat.snapshot.lock").is_file());
    assert!(!benches(&tmp).join("live/snapshot.lock").exists());

    assert_eq!(findings(&tmp), Vec::<String>::new());
    assert!(mockspace_bench_harness::tree::load(&benches(&tmp)).is_ok());
}

/// A dry run says what it would write and writes nothing.
#[test]
fn a_dry_run_writes_nothing() {
    let tmp = unkinded();
    let before = std::fs::read_to_string(benches(&tmp).join("iso/bench.toml")).unwrap();
    let said = migrate(tmp.path(), &benches(&tmp), true).unwrap();
    assert_eq!(said.len(), 3, "{said:#?}");
    assert_eq!(
        std::fs::read_to_string(benches(&tmp).join("iso/bench.toml")).unwrap(),
        before
    );
    assert!(!benches(&tmp).join("iso/snapshot.lock").exists());
}

/// A snapshot is frozen once: freezing it again with its files unchanged
/// writes nothing, and with a file changed is refused and leaves the lock.
#[test]
fn a_lock_is_written_once_and_never_rewritten() {
    let tmp = unkinded();
    migrate(tmp.path(), &benches(&tmp), false).unwrap();
    let lock = benches(&tmp).join("iso/snapshot.lock");
    let first = std::fs::read_to_string(&lock).unwrap();
    assert_eq!(
        freeze(tmp.path(), &benches(&tmp), &["iso"]).unwrap(),
        Vec::<String>::new()
    );

    std::fs::write(
        benches(&tmp).join("iso/arms/a/src/lib.rs"),
        "pub fn faster() {}\n",
    )
    .unwrap();
    let err = freeze(tmp.path(), &benches(&tmp), &["iso"]).unwrap_err();
    assert!(err.contains("frozen once"), "{err}");
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), first);
}

/// Freezing a tracking bench, a bench that does not exist, or a snapshot that
/// reaches outside the tree is refused, each saying why.
#[test]
fn freezing_what_is_no_frozen_snapshot_is_refused() {
    let tmp = unkinded();
    migrate(tmp.path(), &benches(&tmp), false).unwrap();
    let err = freeze(tmp.path(), &benches(&tmp), &["live"]).unwrap_err();
    assert!(err.contains("only a snapshot has a lock"), "{err}");
    let err = freeze(tmp.path(), &benches(&tmp), &["ghost"]).unwrap_err();
    assert!(err.contains("no bench `ghost`"), "{err}");

    let live = benches(&tmp).join("live/bench.toml");
    let text = std::fs::read_to_string(&live)
        .unwrap()
        .replace("tracking", "snapshot");
    std::fs::write(&live, text).unwrap();
    let err = freeze(tmp.path(), &benches(&tmp), &["live"]).unwrap_err();
    assert!(err.contains("reaches outside the bench tree"), "{err}");
    assert!(!benches(&tmp).join("live/snapshot.lock").exists());
}
