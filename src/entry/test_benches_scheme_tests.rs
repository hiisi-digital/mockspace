//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

// ── the digest as a pure function of its entries ──

#[test]
fn the_digest_does_not_depend_on_the_order_the_files_are_fed() {
    let a = vec![
        ("a".to_string(), b"1".to_vec()),
        ("b".to_string(), b"2".to_vec()),
        ("c".to_string(), b"3".to_vec()),
    ];
    let mut reversed = a.clone();
    reversed.reverse();
    assert_eq!(digest_entries(a), digest_entries(reversed));
}

#[test]
fn the_digest_tells_a_moved_byte_a_rename_and_an_added_file_apart() {
    let base = vec![("a".to_string(), b"1".to_vec())];
    let d = digest_entries(base.clone());
    assert_ne!(d, digest_entries(vec![("a".to_string(), b"2".to_vec())]));
    assert_ne!(d, digest_entries(vec![("b".to_string(), b"1".to_vec())]));
    let mut more = base;
    more.push(("c".to_string(), Vec::new()));
    assert_ne!(d, digest_entries(more));
}

#[test]
fn a_boundary_between_name_and_bytes_cannot_be_shifted() {
    // "ab" + "c" must not hash like "a" + "bc".
    assert_ne!(
        digest_entries(vec![("ab".to_string(), b"c".to_vec())]),
        digest_entries(vec![("a".to_string(), b"bc".to_vec())])
    );
}

// ── what the generated manifests are made of ──

/// A benches tree whose arm has no manifest, so `mock bench` generates one,
/// with the mockspace the generated crates depend on by path inside the
/// repository.
fn generated_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let (t, root, mock) = repo();
    write(
        &mock.join("benches/bench.toml"),
        &format!(
            "[build]\nmockspace = '{{ path = \"{}\" }}'\n",
            root.join("vendor").display()
        ),
    );
    write(
        &mock.join("benches/sample/bench.toml"),
        "title = \"Sample\"\nworkload = \"default\"\narms = [\"a\"]\npoints = [64]\n\
         master_seed = 7\n",
    );
    write(&root.join("vendor/bench-core/src/lib.rs"), "1");
    write(&root.join("vendor/bench-macro/src/lib.rs"), "1");
    (t, root, mock)
}

/// The generated manifest of a manifest-less arm names the framework by path,
/// and nothing on disk under the benches says so.
#[test]
fn a_path_dependency_of_a_generated_arm_manifest_moves_the_digest() {
    for rel in ["vendor/bench-core/src/lib.rs", "vendor/bench-macro/src/lib.rs"] {
        let (_t, root, mock) = generated_repo();
        let before = now(&root, &mock);
        write(&root.join(rel), "2");
        assert_ne!(before, now(&root, &mock), "{rel}");
    }
}

/// `mock bench` writes the manifests it generates under `target/`, so their
/// text is not in any file the listing finds, and a change to what it
/// generates has to move the digest by being one of its entries.
#[test]
fn the_text_of_the_generated_manifests_is_digested() {
    let (_t, root, mock) = generated_repo();
    let entries = entries(&root, &mock, &mock.join("benches")).unwrap();
    let text = |name: &str| {
        entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
    };
    let arm = text("generated/sample/a/Cargo.toml").expect("the arm manifest is an entry");
    assert!(
        arm.contains("crate-type") && arm.contains("vendor/bench-core"),
        "{arm}"
    );
    let driver = text("generated/driver/Cargo.toml").expect("the driver manifest is an entry");
    assert!(driver.contains("mockspace-bench-driver"), "{driver}");
    assert!(
        text("generated/driver/main.rs").is_some(),
        "the driver source is an entry"
    );
}

/// The digest says what it is made of, so one made of something else is not
/// equal to it.
#[test]
fn a_change_of_scheme_moves_the_digest_of_the_same_entries() {
    let e = vec![("a".to_string(), b"1".to_vec())];
    assert_ne!(
        digest_entries_under("1", e.clone()),
        digest_entries_under("2", e.clone())
    );
}
