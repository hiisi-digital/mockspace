//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A crate under `mock/crates` that is a workspace root of its own, and takes a git dependency
//! by branch, commits the `Cargo.lock` that holds each such dependency at a commit. Without it
//! the dependency is whatever the branch head is today, and a branch that drops a package leaves
//! the crate's graph unreadable, which is a refused push gate for the whole repository.

use std::path::Path;

/// The dependency tables of a manifest: the three kinds at the top, the same three under each
/// `[target.<cfg>]`, and `[workspace.dependencies]`.
fn dependency_tables(doc: &toml_edit::DocumentMut) -> Vec<&toml_edit::Item> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut tables: Vec<&toml_edit::Item> = KINDS.iter().filter_map(|k| doc.get(k)).collect();
    if let Some(targets) = doc.get("target").and_then(|t| t.as_table_like()) {
        for (_, target) in targets.iter() {
            tables.extend(KINDS.iter().filter_map(|k| target.get(k)));
        }
    }
    tables.extend(doc.get("workspace").and_then(|w| w.get("dependencies")));
    tables
}

/// The names of the dependencies in `manifest` that come from git by a branch.
fn branch_dependencies(manifest: &str) -> Vec<String> {
    let doc: toml_edit::DocumentMut = manifest.parse().expect("a manifest that parses");
    let mut names = Vec::new();
    for table in dependency_tables(&doc) {
        let Some(table) = table.as_table_like() else { continue };
        for (name, spec) in table.iter() {
            let Some(spec) = spec.as_table_like() else { continue };
            if spec.contains_key("git") && spec.contains_key("branch") {
                names.push(name.to_owned());
            }
        }
    }
    names
}

/// Whether `lock` holds a package `name` from git at a full commit.
fn locked_at_a_commit(lock: &str, name: &str) -> bool {
    let mut current = "";
    for line in lock.lines() {
        if let Some(n) = line.strip_prefix("name = \"") {
            current = n.trim_end_matches('"');
        } else if let Some(s) = line.strip_prefix("source = \"") {
            let s = s.trim_end_matches('"');
            let commit = s.rsplit_once('#').map(|(_, c)| c).unwrap_or("");
            if current == name
                && s.starts_with("git+")
                && commit.len() == 40
                && commit.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return true;
            }
        }
    }
    false
}

/// Every branch dependency of the manifest the lock does not hold at a commit, by name.
fn unlocked(manifest: &str, lock: Option<&str>) -> Vec<String> {
    branch_dependencies(manifest)
        .into_iter()
        .filter(|name| !lock.is_some_and(|lock| locked_at_a_commit(lock, name)))
        .collect()
}

const MANIFEST: &str = "[package]\nname = \"p\"\n[workspace]\n[dependencies]\n\
    a = { git = \"https://example.invalid/a.git\", branch = \"dev\" }\n\
    b = { git = \"https://example.invalid/b.git\", rev = \"0123456789012345678901234567890123456789\" }\n\
    c = \"1\"\n";

#[test]
fn a_branch_dependency_with_no_lockfile_is_named() {
    assert_eq!(unlocked(MANIFEST, None), ["a"]);
}

#[test]
fn a_branch_dependency_in_every_dependency_table_is_named() {
    let manifest = "[package]\nname = \"p\"\n\
        [workspace.dependencies]\nw = { git = \"https://example.invalid/w.git\", branch = \"dev\" }\n\
        [dev-dependencies]\nd = { git = \"https://example.invalid/d.git\", branch = \"dev\" }\n\
        [build-dependencies]\nb = { git = \"https://example.invalid/b.git\", branch = \"dev\" }\n\
        [target.'cfg(unix)'.dependencies]\nt = { git = \"https://example.invalid/t.git\", branch = \"dev\" }\n\
        [target.'cfg(unix)'.dev-dependencies]\ntd = { git = \"https://example.invalid/td.git\", branch = \"dev\" }\n";
    let mut found = unlocked(manifest, None);
    found.sort();
    assert_eq!(found, ["b", "d", "t", "td", "w"]);
}

#[test]
fn a_lock_without_the_commit_does_not_hold_it() {
    let lock =
        "[[package]]\nname = \"a\"\nsource = \"git+https://example.invalid/a.git?branch=dev\"\n";
    assert_eq!(unlocked(MANIFEST, Some(lock)), ["a"]);
    let other = "[[package]]\nname = \"z\"\nsource = \
                 \"git+https://example.invalid/a.git?branch=dev#0123456789012345678901234567890123456789\"\n";
    assert_eq!(unlocked(MANIFEST, Some(other)), ["a"]);
}

#[test]
fn a_lock_with_the_commit_holds_it() {
    let lock = "[[package]]\nname = \"a\"\nsource = \
                \"git+https://example.invalid/a.git?branch=dev#0123456789012345678901234567890123456789\"\n";
    assert!(unlocked(MANIFEST, Some(lock)).is_empty());
}

#[test]
fn every_separate_workspace_under_mock_crates_locks_its_branch_dependencies() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("mock/crates");
    let mut roots = 0;
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(&crates).expect("mock/crates") {
        let dir = entry.expect("an entry").path();
        let Ok(manifest) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        let doc: toml_edit::DocumentMut = manifest.parse().expect("a manifest that parses");
        if !doc.contains_key("workspace") {
            continue;
        }
        roots += 1;
        let lock = std::fs::read_to_string(dir.join("Cargo.lock")).ok();
        for name in unlocked(&manifest, lock.as_deref()) {
            offenders.push(format!("{}: {name}", dir.display()));
        }
    }
    // the control: the walk reached at least the lint cdylib, which is such a root
    assert!(
        roots > 0,
        "no separate workspace found under {}",
        crates.display()
    );
    assert!(
        offenders.is_empty(),
        "branch dependencies with no committed commit: {offenders:#?}"
    );
}
