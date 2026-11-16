//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The kind a bench declares, as the tree loads it.

use super::tests::{EMPTY_ROOT, Tree};
use super::*;

const COMPOSED: &str = "title = \"h\"\narms = [\"fnv\"]\npoints = [64]\n";

/// A composed member with no kind is refused at load, naming the member and
/// the migration; the same member declaring one loads.
#[test]
fn a_member_with_no_kind_is_refused_at_load() {
    let t = Tree::new("no-kind");
    t.write("bench.toml", EMPTY_ROOT);
    t.write("hash/bench.toml", COMPOSED);
    t.mkdir("hash/arms/fnv/src");
    let err = load(&t.root).unwrap_err().to_string();
    assert!(err.contains("bench `hash` declares no `kind`"), "{err}");
    assert!(err.contains("mock bench migrate --kinds"), "{err}");

    t.write(
        "hash/bench.toml",
        &format!("kind = \"tracking\"\n{COMPOSED}"),
    );
    assert!(load(&t.root).is_ok());
}

/// A root section and a section of a member are refused the same way, each
/// named by its key.
#[test]
fn a_section_with_no_kind_is_refused_by_its_key() {
    let section =
        "[bench.b]\ntitle = \"B\"\nworkload = \"default\"\nvariants = [\"v\"]\nsizes = [8]\n";
    let t = Tree::new("no-kind-section");
    t.write("bench.toml", section);
    let err = load(&t.root).unwrap_err().to_string();
    assert!(err.contains("bench `b` declares no `kind`"), "{err}");

    let t = Tree::new("no-kind-member-section");
    t.write("bench.toml", EMPTY_ROOT);
    t.write("m/bench.toml", section);
    let err = load(&t.root).unwrap_err().to_string();
    assert!(err.contains("bench `m/b` declares no `kind`"), "{err}");
}

/// A word that names no kind is refused by the parser with both spellings.
#[test]
fn an_unknown_kind_word_is_refused() {
    let t = Tree::new("odd-kind");
    t.write("bench.toml", EMPTY_ROOT);
    t.write("hash/bench.toml", &format!("kind = \"frozen\"\n{COMPOSED}"));
    t.mkdir("hash/arms/fnv/src");
    let err = load(&t.root).unwrap_err().to_string();
    assert!(
        err.contains("`kind = \"frozen\"` names no bench kind"),
        "{err}"
    );
}

/// The kind belongs to the bench: a sweep that declares one is refused, since
/// two sweeps of one bench reading their history two ways would be two benches.
#[test]
fn a_sweep_may_not_declare_a_kind() {
    let t = Tree::new("sweep-kind");
    t.write("bench.toml", EMPTY_ROOT);
    t.write(
        "hash/bench.toml",
        "kind = \"tracking\"\ntitle = \"h\"\narms = [\"fnv\"]\n[sweep.s]\npoints = [1]\nkind = \"snapshot\"\n",
    );
    t.mkdir("hash/arms/fnv/src");
    let err = load(&t.root).unwrap_err().to_string();
    assert!(err.contains("unknown field `kind`"), "{err}");
}

/// Each snapshot's lock sits beside the file that declares it, named so two
/// benches of one file never share one; a tracking bench has none, and every
/// arm carries its bench's kind.
#[test]
fn a_snapshot_s_lock_sits_beside_its_declaration() {
    let t = Tree::new("locks");
    t.write(
        "bench.toml",
        "[bench.root]\nkind = \"snapshot\"\ntitle = \"R\"\nworkload = \"default\"\nvariants = [\"v\"]\nsizes = [8]\n",
    );
    t.write(
        "hash/bench.toml",
        &format!("kind = \"snapshot\"\n{COMPOSED}"),
    );
    t.mkdir("hash/arms/fnv/src");
    t.write(
        "live/bench.toml",
        &format!("kind = \"tracking\"\n{COMPOSED}"),
    );
    t.mkdir("live/arms/fnv/src");
    t.write(
        "m/bench.toml",
        "[bench.x]\nkind = \"snapshot\"\ntitle = \"X\"\nworkload = \"default\"\nvariants = [\"v\"]\nsizes = [8]\n",
    );
    let tree = load(&t.root).unwrap();
    let lock = |key: &str| tree.manifest.bench[key].lock.clone();
    assert_eq!(lock("root"), Some(t.root.join("root.snapshot.lock")));
    assert_eq!(lock("hash"), Some(t.root.join("hash/snapshot.lock")));
    assert_eq!(lock("m/x"), Some(t.root.join("m/x.snapshot.lock")));
    assert_eq!(lock("live"), None);
    let kinds: Vec<(&str, Kind)> = tree
        .arms
        .iter()
        .map(|a| (a.bench.as_str(), a.kind))
        .collect();
    assert_eq!(kinds, [("hash", Kind::Snapshot), ("live", Kind::Tracking)]);
}

/// A snapshot's run config carries its lock's id, and a snapshot with no lock
/// on disk is refused when its config is built; a tracking config carries the
/// tracking marker.
#[test]
fn a_snapshot_config_carries_its_lock_id_and_needs_its_lock() {
    let t = Tree::new("source");
    t.write("bench.toml", EMPTY_ROOT);
    t.write(
        "hash/bench.toml",
        &format!("kind = \"snapshot\"\n{COMPOSED}"),
    );
    t.mkdir("hash/arms/fnv/src");
    t.write(
        "live/bench.toml",
        &format!("kind = \"tracking\"\n{COMPOSED}"),
    );
    t.mkdir("live/arms/fnv/src");
    let tree = load(&t.root).unwrap();
    let err = tree
        .manifest
        .for_size("hash", 0, &t.root)
        .unwrap_err()
        .to_string();
    assert!(err.contains("mock bench freeze hash"), "{err}");

    t.write("hash/snapshot.lock", "# lock\nabc  hash/bench.toml\n");
    let c = tree.manifest.for_size("hash", 0, &t.root).unwrap();
    assert_eq!(c.kind, Kind::Snapshot);
    assert_eq!(c.source.len(), 12);
    let live = tree.manifest.for_size("live", 0, &t.root).unwrap();
    assert_eq!((live.kind, live.source.as_str()), (Kind::Tracking, "-"));
}
