//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A dependency named with the crate prefix is a sibling only where the package was discovered.
//! One that lives elsewhere, a crate outside the source directories or one from a registry or a
//! git source, is no node of the graph, and the structure graph indexed the map by it and panicked.

use std::fs;
use std::path::Path;

fn package(root: &Path, name: &str, deps: &[&str]) {
    let dir = root.join(name);
    fs::create_dir_all(dir.join("src")).expect("a package directory");
    let mut manifest = format!("[package]\nname = \"{name}\"\n\n[dependencies]\n");
    for dep in deps {
        manifest.push_str(&format!("{dep} = {{ path = \"../{dep}\" }}\n"));
    }
    fs::write(dir.join("Cargo.toml"), manifest).expect("a manifest");
    fs::write(dir.join("src/lib.rs"), "pub struct Marker;\n").expect("a lib.rs");
}

#[test]
fn only_a_discovered_package_is_a_dependency_edge() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let crates = tmp.path().join("crates");
    package(&crates, "p-a", &["p-b", "p-outside"]);
    package(&crates, "p-b", &[]);

    let map = mockspace::parse::discover_crates(&crates, "p");
    assert_eq!(map["p-a"].deps, ["p-b"]);

    let across = mockspace::parse::discover_crates_in(&[crates.clone()], "p");
    assert_eq!(across["p-a"].deps, ["p-b"]);

    // every edge the graph draws must index the map
    for (name, deps) in mockspace::graph::transitive_reduction(&across) {
        for dep in deps {
            assert!(
                across.contains_key(dep.as_str()),
                "{name} -> {dep} names no package"
            );
        }
    }
}

#[test]
fn a_sibling_in_another_source_directory_stays_an_edge() {
    let tmp = tempfile::tempdir().expect("a temporary directory");
    let one = tmp.path().join("one");
    let two = tmp.path().join("two");
    package(&one, "p-a", &["p-b"]);
    package(&two, "p-b", &[]);

    let across = mockspace::parse::discover_crates_in(&[one, two], "p");
    assert_eq!(across["p-a"].deps, ["p-b"]);
}
