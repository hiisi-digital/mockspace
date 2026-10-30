//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The declared reach of every built-in lint, held to the check in [`crate::reach_check`], and the
//! check itself held to catching a lint that lies.

use std::path::Path;

use crate::reach_check::{DESIGN_ENTRY, Roots, crate_lint, workspace_lint};
use crate::{
    CrateLint, Lint, LintContext, LintError, Reach, WorkspaceLint, all_lints,
    all_workspace_lints,
};

/// Real source from this crate, so a lint that reads past its reach only on some construct meets
/// the construct, and a few snippets that make the lints under test actually find something.
fn corpus() -> Vec<(&'static str, &'static str)> {
    vec![
        ("src/lib.rs", include_str!("lib.rs")),
        ("src/dispatch.rs", include_str!("dispatch.rs")),
        ("src/verdict.rs", include_str!("verdict.rs")),
        ("src/no_todo.rs", include_str!("no_todo.rs")),
        ("src/file_size.rs", include_str!("file_size.rs")),
        (
            "src/dirty.rs",
            "pub fn exported() { todo!() }\npub struct Bare(pub u32);\nuse std::collections::HashMap;\n\
             pub enum MyError { A }\nimpl Clone for Bare { fn clone(&self) -> Self { Bare(self.0) } }\n",
        ),
        ("src/stack.rs", STACK_SNIPPET),
    ]
}

/// A populated and an empty directory, the two a lint could be made to read through.
fn roots(base: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let populated = base.join("populated");
    std::fs::create_dir_all(populated.join("design_rounds")).unwrap();
    std::fs::write(populated.join("mockspace.toml"), "project_name = \"x\"\n").unwrap();
    std::fs::write(populated.join("design_rounds/202601010000_a.src.md"), "# a\n").unwrap();
    std::fs::create_dir_all(populated.join("crates/alpha/src")).unwrap();
    std::fs::write(populated.join("crates/alpha/src/lib.rs"), "pub fn x() {}\n").unwrap();
    let empty = base.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    (populated, empty)
}

/// Declares a file reach and reads the number of sibling files, which is outside the file.
struct ReadsSiblings;

impl Lint for ReadsSiblings {
    fn name(&self) -> &'static str {
        "reads-siblings"
    }

    fn reach(&self) -> Reach {
        Reach::File
    }
}

impl CrateLint for ReadsSiblings {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.all_sources.len() > 1 {
            vec![LintError::error(ctx.crate_name.to_string(), 1, "reads-siblings", "siblings".into())]
        } else {
            Vec::new()
        }
    }
}

/// Declares a file reach and reads the design document.
struct ReadsDesign;

impl Lint for ReadsDesign {
    fn name(&self) -> &'static str {
        "reads-design"
    }

    fn reach(&self) -> Reach {
        Reach::File
    }
}

impl CrateLint for ReadsDesign {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.design_doc.is_some() {
            vec![LintError::error(ctx.crate_name.to_string(), 1, "reads-design", "design".into())]
        } else {
            Vec::new()
        }
    }
}

/// Declares a crate reach and reads the workspace directory.
struct ReadsWorkspaceDir;

impl Lint for ReadsWorkspaceDir {
    fn name(&self) -> &'static str {
        "reads-workspace-dir"
    }

    fn reach(&self) -> Reach {
        Reach::Crate
    }

    fn per_file(&self) -> bool {
        false
    }
}

impl CrateLint for ReadsWorkspaceDir {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.workspace_root.join("mockspace.toml").exists() {
            vec![LintError::error(ctx.crate_name.to_string(), 1, "reads-workspace-dir", "toml".into())]
        } else {
            Vec::new()
        }
    }
}

/// Declares a workspace reach and reads the workspace directory.
struct WorkspaceReadsDir;

impl Lint for WorkspaceReadsDir {
    fn name(&self) -> &'static str {
        "workspace-reads-dir"
    }

    fn reach(&self) -> Reach {
        Reach::Workspace
    }
}

impl WorkspaceLint for WorkspaceReadsDir {
    fn check_all(&self, crates: &[(&str, &LintContext)]) -> Vec<LintError> {
        let Some((name, ctx)) = crates.first() else { return Vec::new() };
        if ctx.workspace_root.join("design_rounds").is_dir() {
            vec![LintError::error((*name).to_string(), 1, "workspace-reads-dir", "rounds".into())]
        } else {
            Vec::new()
        }
    }
}

/// Declares a file reach and honestly stays inside it. The control: the check passes a lint that
/// is what it says, so a failure above is the lint and not the check.
struct HonestFile;

impl Lint for HonestFile {
    fn name(&self) -> &'static str {
        "honest-file"
    }

    fn reach(&self) -> Reach {
        Reach::File
    }
}

impl CrateLint for HonestFile {
    fn check(&self, ctx: &LintContext) -> Vec<LintError> {
        if ctx.source.contains("todo!") {
            vec![LintError::error(ctx.crate_name.to_string(), 1, "honest-file", "todo".into())]
        } else {
            Vec::new()
        }
    }
}

#[test]
fn the_check_passes_a_lint_that_stays_inside_its_reach() {
    let dir = tempfile::tempdir().unwrap();
    let (populated, empty) = roots(dir.path());
    let roots = Roots { populated: &populated, empty: &empty };
    let found = crate_lint(&HonestFile, &corpus(), &roots);
    assert!(found.is_ok(), "the control: an honest file-reach lint must pass: {found:?}");
}

#[test]
fn a_file_lint_that_reads_its_siblings_is_caught() {
    let dir = tempfile::tempdir().unwrap();
    let (populated, empty) = roots(dir.path());
    let roots = Roots { populated: &populated, empty: &empty };
    let err = crate_lint(&ReadsSiblings, &corpus(), &roots).expect_err("reads past the file");
    assert!(err.contains("reads-siblings") && err.contains("past the file"), "{err}");
}

#[test]
fn a_file_lint_that_reads_the_design_document_is_caught() {
    let dir = tempfile::tempdir().unwrap();
    let (populated, empty) = roots(dir.path());
    let roots = Roots { populated: &populated, empty: &empty };
    assert!(crate_lint(&ReadsDesign, &corpus(), &roots).is_err());
}

#[test]
fn a_crate_lint_that_reads_the_workspace_directory_is_caught() {
    let dir = tempfile::tempdir().unwrap();
    let (populated, empty) = roots(dir.path());
    let roots = Roots { populated: &populated, empty: &empty };
    let err = crate_lint(&ReadsWorkspaceDir, &corpus(), &roots).expect_err("reads past the crate");
    assert!(err.contains("past the crate"), "{err}");
}

#[test]
fn a_workspace_lint_that_reads_the_workspace_directory_is_caught() {
    let dir = tempfile::tempdir().unwrap();
    let (populated, empty) = roots(dir.path());
    let roots = Roots { populated: &populated, empty: &empty };
    assert!(workspace_lint(&WorkspaceReadsDir, &corpus(), &roots).is_err());
}

#[test]
fn a_file_reach_on_a_lint_that_judges_the_whole_crate_is_refused() {
    struct Inconsistent;
    impl Lint for Inconsistent {
        fn name(&self) -> &'static str {
            "inconsistent"
        }

        fn reach(&self) -> Reach {
            Reach::File
        }

        fn per_file(&self) -> bool {
            false
        }
    }
    impl CrateLint for Inconsistent {
        fn check(&self, _ctx: &LintContext) -> Vec<LintError> {
            Vec::new()
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let (populated, empty) = roots(dir.path());
    let roots = Roots { populated: &populated, empty: &empty };
    let err = crate_lint(&Inconsistent, &corpus(), &roots).expect_err("inconsistent");
    assert!(err.contains("per_file"), "{err}");
}

#[test]
fn every_built_in_lint_stays_inside_the_reach_it_declares() {
    let dir = tempfile::tempdir().unwrap();
    let (populated, empty) = roots(dir.path());
    let roots = Roots { populated: &populated, empty: &empty };
    let corpora = [corpus(), vec![("src/lib.rs", "")], vec![("src/lib.rs", STACK_LIB), (DESIGN_ENTRY, "| `GhostType` | nothing |\n")]];

    let mut checked = 0;
    let mut silent = Vec::new();
    for mut lint in all_lints() {
        if lint.name() == "forbidden-imports" {
            lint.configure(&std::collections::HashMap::from([
                ("rule.no-std.scope".to_string(), "*".to_string()),
                ("rule.no-std.forbidden".to_string(), "std::*".to_string()),
            ]));
        }
        let seen: usize = corpora
            .iter()
            .map(|c| crate_lint(lint.as_ref(), c, &roots).unwrap_or_else(|e| panic!("{e}")))
            .sum();
        if lint.reach().is_cacheable() {
            checked += 1;
            if seen == 0 {
                silent.push(lint.name());
            }
        }
    }
    for lint in all_workspace_lints() {
        let seen = workspace_lint(lint.as_ref(), &corpora[0], &roots).unwrap_or_else(|e| panic!("{e}"));
        if lint.reach().is_cacheable() {
            checked += 1;
            if seen == 0 {
                silent.push(lint.name());
            }
        }
    }
    assert!(checked >= 20, "the declarations were not applied: only {checked} lints are saved");
    assert!(
        silent.is_empty(),
        "these lints found nothing on the corpus, so their reach was never tested: {silent:?}"
    );
}

#[test]
fn the_corpus_makes_the_lints_find_something() {
    // Without this a lint that reads past its reach passes the check only because it has nothing
    // to say on the corpus. `no-todo` finding the `todo!()` is the proof the corpus bites.
    let corpus = corpus();
    let dirty = corpus.iter().find(|(p, _)| *p == "src/dirty.rs").unwrap();
    let fixture = crate::testkit::LintFixture::new(dirty.1);
    let found = crate::no_todo::NoTodo.check(&fixture.ctx());
    assert!(!found.is_empty(), "the dirty snippet must trip no-todo");
}

#[test]
fn a_lint_that_declares_nothing_is_the_widest_and_is_never_saved() {
    struct Silent;
    impl Lint for Silent {
        fn name(&self) -> &'static str {
            "silent"
        }
    }
    assert_eq!(Silent.reach(), Reach::Repository);
    assert!(!Silent.reach().is_cacheable());
}

/// One construct for each lint that binds to the stack's own names, so the reach check has a finding
/// to compare for it.
const STACK_SNIPPET: &str = "\
pub struct WidgetId(u32);
impl Scope for Widget {}
fn f() { LoimuError::new(1); }
define_raw_error!(Oops);
define_error!(
    code: 1,
);
#[repr(C)]
pub struct Abi { v: Vec<u8> }
pub struct WidgetRegistry { n: u8 }
test_registry::define_registry!(ActionRegistry for Action with ActionEntry);
define_behavior! {
    x.pool();
}
";

/// A crate root that defines a `define_*` macro and never generates a `Registrable` from it.
const STACK_LIB: &str = "macro_rules! define_widget { () => {}; }\n";
