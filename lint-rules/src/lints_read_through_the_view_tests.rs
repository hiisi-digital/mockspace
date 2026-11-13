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

/// The built-in lints are held to what packs are held to. Only the reading itself, the runner
/// that builds it, the tools, the test kit and the timing report touch the disk, and they are not
/// lints. The crate root names `Reading` to export it, and the guard builds one for a pack's
/// assertion, and nothing else of either file is let through.
#[test]
fn every_built_in_lint_reads_through_its_view() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let reading = Reading::new(&src);
    let view = reading.view();
    let infrastructure = ["reading/", "run/", "tool/", "tool.rs", "testkit.rs", "timing.rs"];
    let names_the_type = ["lib.rs", "lints_read_through_the_view.rs"];
    let found: Vec<String> = bypasses_under(&view, &src)
        .into_iter()
        .filter(|b| !infrastructure.iter().any(|i| b.path.to_string_lossy().starts_with(i)))
        .filter(|b| !(b.what == READING && names_the_type.iter().any(|f| b.path == Path::new(f))))
        .map(|b| b.to_string())
        .collect();
    assert_eq!(found, Vec::<String>::new());
}

/// A macro's arguments are tokens, not expressions, and a read inside them reads all the same.
#[test]
fn a_read_inside_a_macro_is_refused() {
    let source = r#"
fn check(dir: &Path) -> Vec<String> {
    let _ = vec![std::fs::read(dir)];
    let _ = format!("{}", std::fs::read_to_string(dir).unwrap());
    let _ = vec![dir.join("a").exists()];
    let _ = format!("{:?}", ::std::process::Command::new("git"));
    let _ = vec![Path::is_file(dir)];
    Vec::new()
}
"#;
    assert_eq!(found(source), [
        "3:std::fs",
        "4:std::fs",
        "5:.exists()",
        "6:std::process",
        "7:Path::is_file",
    ]);
    // The control: the same macros over the view and over plain values are not refused.
    let clean = "fn c(v: &View, d: &Path) { let _ = vec![v.text(d), v.exists(d)]; let _ = format!(\"{}\", d.join(\"a\").display()); }\n";
    assert_eq!(found(clean), Vec::<String>::new());
}

/// `use std::*` brings `fs` in under a name the path check cannot see, and so does renaming `std`.
#[test]
fn a_glob_or_a_rename_above_a_forbidden_path_is_refused() {
    let source = "use std::*;\nuse ::std as s;\nuse std::{self as t};\nfn c() { let _ = fs::read(\"x\"); }\n";
    assert_eq!(found(source), ["1:std::*", "2:std", "3:std"]);
    // The control: a glob and a rename below or beside them are not refused.
    let clean = "use std::collections::*;\nuse std::path::Path as P;\nuse std::io::{self};\n";
    assert_eq!(found(clean), Vec::<String>::new());
}

/// An environment variable is read around the view as a file is, and the trace holds neither.
#[test]
fn an_environment_read_is_refused() {
    let source = "use std::env;\nfn c() { let _ = std::env::var(\"HOME\"); let _ = ::std::env::args(); }\n";
    assert_eq!(found(source), ["1:std::env", "2:std::env", "2:std::env"]);
    // The control: `env!` is read when the lint is built, not when it runs.
    assert_eq!(found("fn c() { let _ = env!(\"CARGO_MANIFEST_DIR\"); }\n"), Vec::<String>::new());
}

/// A lint building a reading and a view of its own reads into no trace the run keeps.
#[test]
fn a_reading_of_the_lints_own_is_refused() {
    let source = r#"
use mockspace_lint_rules::Reading;
fn c(ctx: &LintContext) {
    let r = mockspace_lint_rules::Reading::new(ctx.crate_dir());
    let _ = r.view().text("a");
    let _ = Reading::default();
}
"#;
    assert_eq!(found(source), ["2:Reading", "4:Reading", "6:Reading"]);
    // The control: a name that only starts like it is not the type.
    assert_eq!(found("fn c() { let _ = Readings::new(); let _ = reading::view(); }\n"), Vec::<String>::new());
}

/// `git2` opens a repository on disk and `fs_err` is `std::fs` under another name.
#[test]
fn a_repository_or_file_crate_is_refused() {
    let source = "use git2::Repository;\nfn c() { let _ = fs_err::read(\"x\"); let _ = ::git2::Repository::open(\".\"); }\n";
    assert_eq!(found(source), ["1:git2", "2:fs_err", "2:git2"]);
    // The control: the view's own `git` is not a crate path.
    assert_eq!(found("fn c(v: &View) { let _ = v.git(&[\"log\"]); }\n"), Vec::<String>::new());
}

/// `Path::is_file` and `is_dir` ask the disk. An entry of a listing is asked as `Entry::is_dir`,
/// since the method form cannot be told apart from the path's.
#[test]
fn a_path_asked_whether_it_is_a_file_or_a_directory_is_refused() {
    let source = r#"
fn c(dir: &Path, entry: &Entry) {
    let _ = dir.join("a").is_file();
    let _ = dir.is_dir();
    let _ = dir.is_symlink();
    let _ = Path::is_dir(dir);
    let _ = std::path::PathBuf::is_file(&dir.to_path_buf());
}
"#;
    assert_eq!(found(source), [
        "3:.is_file()",
        "4:.is_dir()",
        "5:.is_symlink()",
        "6:Path::is_dir",
        "7:PathBuf::is_file",
    ]);
    // The control: the view's own forms take the path, and an entry is asked by its type.
    let clean = "fn c(v: &View, d: &Path, e: &Entry) { let _ = v.is_file(d) || v.is_dir(d) || Entry::is_dir(e); }\n";
    assert_eq!(found(clean), Vec::<String>::new());
}

/// A tool is run on demand and keeps no unit, so it may read the disk; the guard tells it by the
/// `tools` directory it lives under.
#[test]
fn a_tool_under_a_tools_directory_is_not_read() {
    let pack = Pack::new("tools", &[
        ("src/lib.rs", CLEAN),
        ("src/tools/weigh/mod.rs", "use mockspace_lint_rules::Reading;\nfn t() { let _ = Reading::new(\".\"); }\n"),
        ("src/lints/weigh.rs", "use mockspace_lint_rules::Reading;\n"),
    ]);
    let reading = Reading::new(&pack.0);
    let shown: Vec<String> = bypasses_under(&reading.view(), &pack.0)
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(shown, ["src/lints/weigh.rs:1: reaches `Reading`"]);
}

/// A lint may have a type of its own called `Reading`; only this crate's is refused.
#[test]
fn a_type_of_the_lints_own_named_reading_is_not_refused() {
    let own = "struct Reading { n: usize }\nfn c() -> Reading { Reading::default() }\n";
    assert_eq!(found(own), Vec::<String>::new());
    // The control: the crate's own type is refused beside it, by its path.
    let both = "struct Reading;\nfn c() { let _ = mockspace::Reading::new(\".\"); }\n";
    assert_eq!(found(both), ["2:Reading"]);
}

/// A function asking `untraced` keeps its unit unsaved, so what it reads around the view is read
/// on every run; a function beside it is still held.
#[test]
fn a_function_asking_untraced_may_read_around_the_view() {
    let source = r#"
fn home(view: &View) -> Option<String> {
    view.untraced("the home directory is not in the repository");
    std::env::var("HOME").ok()
}
fn other() -> Option<String> {
    std::env::var("HOME").ok()
}
"#;
    assert_eq!(found(source), ["7:std::env"]);
}

/// Only an `untraced` that always runs before the read exempts it: one in a branch, in a closure
/// or after the read leaves the read to be refused.
#[test]
fn an_untraced_that_may_not_run_exempts_nothing() {
    let conditional = "fn f(view: &View, bad: bool) -> String {\n    if bad { view.untraced(\"x\"); }\n    std::fs::read_to_string(\"a\").unwrap()\n}\n";
    assert_eq!(found(conditional), ["3:std::fs"]);
    let closure = "fn f(view: &View) -> String {\n    let _ = || view.untraced(\"x\");\n    std::fs::read_to_string(\"a\").unwrap()\n}\n";
    assert_eq!(found(closure), ["3:std::fs"]);
    let after = "fn f(view: &View) -> String {\n    let s = std::fs::read_to_string(\"a\").unwrap();\n    view.untraced(\"x\");\n    s\n}\n";
    assert_eq!(found(after), ["2:std::fs"]);
    // The control: the same read after an `untraced` standing first is let through.
    let first = "fn f(view: &View) -> String {\n    view.untraced(\"x\");\n    std::fs::read_to_string(\"a\").unwrap()\n}\n";
    assert_eq!(found(first), Vec::<String>::new());
}
