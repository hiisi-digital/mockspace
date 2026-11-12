//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The guard refuses lint source reaching around its view, and passes lint source that does not.

use std::path::Path;

use super::*;
use crate::make_parser;

fn found(source: &str) -> Vec<String> {
    let tree = make_parser().parse(source, None).expect("the fixture parses");
    bypasses(Path::new("lint.rs"), source, &tree)
        .into_iter()
        .map(|b| format!("{}:{}", b.line, b.what))
        .collect()
}

/// A lint reading through its view, the shape every lint takes. The negative control: a guard
/// that refused everything would refuse this.
const CLEAN: &str = r#"
use std::path::Path;

use mockspace_lint_rules::{CrateLint, LintContext, LintError};

fn check(ctx: &LintContext) -> Vec<LintError> {
    let view = ctx.view();
    let dir = ctx.crate_dir().join("src");
    if !view.exists(&dir) || !view.is_dir(&dir) {
        return Vec::new();
    }
    let _ = view.text(dir.join("lib.rs"));
    let _ = view.run("cargo", &["metadata"], ctx.crate_dir(), &[]);
    let _ = Path::new("a").join("b").exists_in_name_only;
    Vec::new()
}
"#;

#[test]
fn lint_source_reading_through_its_view_passes() {
    assert_eq!(found(CLEAN), Vec::<String>::new());
}

#[test]
fn every_way_around_the_view_is_refused() {
    let source = r#"
use std::fs;
use std::{io, process::Command};
use walkdir::WalkDir;

fn check(dir: &std::path::Path) {
    let _ = std::fs::read_to_string(dir.join("a"));
    let _ = ::std::process::Command::new("git");
    let _ = dir.read_dir();
    let _ = dir.join("b").exists();
    let _ = dir.metadata();
}
"#;
    assert_eq!(found(source), [
        "2:std::fs",
        "3:std::process",
        "4:walkdir",
        "7:std::fs",
        "8:std::process",
        "9:read_dir",
        "10:.exists()",
        "11:.metadata()",
    ]);
}

#[test]
fn test_code_may_build_its_fixtures_on_disk() {
    let source = r#"
fn check() {}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        std::fs::write("/tmp/x", "y").unwrap();
    }
}
"#;
    assert_eq!(found(source), Vec::<String>::new());

    let inner = "#![cfg(test)]\nfn t() { std::fs::write(\"/tmp/x\", \"y\").unwrap(); }\n";
    assert_eq!(found(inner), Vec::<String>::new());

    // The control: the same body outside a test module is refused, so the two above pass for
    // being test code and not for the guard missing the call.
    let outside = "fn t() { std::fs::write(\"/tmp/x\", \"y\").unwrap(); }\n";
    assert_eq!(found(outside), ["1:std::fs"]);
}

/// A scratch directory holding a pack's lint source, removed on drop.
struct Pack(PathBuf);

impl Pack {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!("mockspace-guard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (rel, text) in files {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        Self(root)
    }
}

impl Drop for Pack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_pack_guard_names_the_file_and_skips_tests() {
    let pack = Pack::new("pack", &[
        ("src/lib.rs", CLEAN),
        ("src/reads_disk.rs", "fn a() { let _ = std::fs::read(\"x\"); }\n"),
        ("src/reads_disk_tests.rs", "fn a() { let _ = std::fs::read(\"x\"); }\n"),
        ("tests/it.rs", "fn a() { let _ = std::fs::read(\"x\"); }\n"),
        ("target/debug/build/gen.rs", "fn a() { let _ = std::fs::read(\"x\"); }\n"),
    ]);
    let reading = Reading::new(&pack.0);
    let shown: Vec<String> = bypasses_under(&reading.view(), &pack.0)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(shown, ["src/reads_disk.rs:1: reaches `std::fs`"]);

    let refused = std::panic::catch_unwind(|| assert_lints_read_through_the_view(&pack.0));
    assert!(refused.is_err(), "the assertion a pack calls must fail over a lint reading around the view");

    let clean = Pack::new("clean", &[("src/lib.rs", CLEAN)]);
    assert_lints_read_through_the_view(&clean.0);
}

#[test]
fn a_guard_over_nothing_does_not_pass() {
    let missing = std::env::temp_dir().join(format!("mockspace-guard-none-{}", std::process::id()));
    let refused = std::panic::catch_unwind(|| assert_lints_read_through_the_view(&missing));
    assert!(refused.is_err());
}

#[test]
fn the_repo_lint_reads_the_mock_lints_directory() {
    let repo = Pack::new("repo", &[
        ("mock/lints/my-pack/src/lib.rs", "use std::process::Command;\n"),
        ("mock/lints/my-pack/src/fine.rs", CLEAN),
    ]);
    let mock = repo.0.join("mock");
    let input = crate::RepoInput {
        mock_dir:    &mock,
        repo_root:   &repo.0,
        all_crates:  &std::collections::BTreeSet::new(),
        src_dirs:    &[],
        invocation:  None,
        canon_paths: &[],
        open_panels: &[],
        ref_files:   &crate::src_layout::NO_REF_FILES,
        registry:    &Default::default(),
    };
    let found = crate::testkit::check_repo(&LintsReadThroughTheView, input);
    let paths: Vec<_> = found.iter().map(|e| (e.path.clone(), e.line)).collect();
    assert_eq!(paths, [(Some("mock/lints/my-pack/src/lib.rs".to_string()), 1)]);
    assert!(found[0].message.contains("std::process"), "{}", found[0].message);

    let empty = Pack::new("repo-none", &[("mock/README.md", "")]);
    let mock = empty.0.join("mock");
    let input = crate::RepoInput { mock_dir: &mock, repo_root: &empty.0, ..input };
    assert!(crate::testkit::check_repo(&LintsReadThroughTheView, input).is_empty());
}

/// The built-in lints are held to what packs are held to. Only the reading itself, the tools,
/// the test kit and the timing report touch the disk, and they are not lints.
#[test]
fn every_built_in_lint_reads_through_its_view() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let reading = Reading::new(&src);
    let view = reading.view();
    let infrastructure = ["reading/", "tool/", "tool.rs", "testkit.rs", "timing.rs"];
    let found: Vec<String> = bypasses_under(&view, &src)
        .into_iter()
        .filter(|b| !infrastructure.iter().any(|i| b.path.to_string_lossy().starts_with(i)))
        .map(|b| b.to_string())
        .collect();
    assert_eq!(found, Vec::<String>::new());
}
