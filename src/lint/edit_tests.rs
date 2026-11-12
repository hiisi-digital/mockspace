//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `lint_edit` judges the text an edit is about to write, and writes nothing to the store.

use std::fs;
use std::path::Path;

use mockspace_lint_rules::{LintConfig, LintPack};

use super::verdicts::test_base;

/// Every file under `dir`, so a store can be compared before and after.
fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p.strip_prefix(dir).unwrap().display().to_string());
            }
        }
    }
    out.sort();
    out
}

#[test]
fn an_edit_is_judged_as_it_will_read_and_saves_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    let src = mock.join("crates");
    let krate = src.join("sim-a");
    fs::create_dir_all(krate.join("src")).unwrap();
    fs::write(krate.join("Cargo.toml"), "[package]\nname = \"sim-a\"\n").unwrap();
    fs::write(krate.join("src/lib.rs"), "pub mod m;\n").unwrap();
    fs::write(krate.join("src/m.rs"), "pub fn f() {}\n").unwrap();
    let store = tmp.path().join("store");
    let src_dirs = vec![src.clone()];
    let crates = crate::parse::discover_crates_in(&src_dirs, "sim");
    let edit = |text: &str| {
        super::lint_edit(
            &crates,
            &src_dirs,
            &mock,
            &[],
            false,
            "sim",
            &LintConfig::empty(),
            &Default::default(),
            &LintPack::default(),
            "no-todo",
            &krate.join("src/m.rs"),
            text,
        )
        .expect("the file is in a crate")
    };

    test_base::with(store.clone(), || {
        // A run that saves, so the store holds a unit for the file as it is on disk.
        let _ = super::run_lints(
            &crates,
            &src_dirs,
            &mock,
            tmp.path(),
            mockspace_lint_rules::LintMode::Commit,
            None,
            false,
            &[],
            false,
            "sim",
            &LintConfig::empty(),
            &Default::default(),
            &[],
            &[],
            &mockspace_lint_rules::src_layout::NO_REF_FILES,
            &Default::default(),
            &LintPack::default(),
        );
        let before = files_under(&store);
        assert!(!before.is_empty(), "the control: a plain run saves");

        let found = edit("pub fn f() { todo!() }\n");
        assert_eq!(found.len(), 1, "the edit is judged, not the file on disk: {found:?}");
        assert_eq!(found[0].path.as_deref(), Some("src/m.rs"));
        assert!(edit("pub fn f() {}\n").is_empty(), "the control: a clean edit finds nothing");

        assert_eq!(files_under(&store), before, "an edit's run writes nothing to the store");
        assert_eq!(fs::read_to_string(krate.join("src/m.rs")).unwrap(), "pub fn f() {}\n");
    });
}
