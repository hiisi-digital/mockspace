//! Tests for `single-source`.
//!
//! The reach tests are tables: every kind of definition against every place the
//! module system can put it, because "the lint sees a module file" is a claim about
//! the whole set of placements and a sample of it would pass with the lint blind
//! to the rest. Each table row is one crate holding the definition there and one
//! holding it at its root, in both orders.

use std::path::PathBuf;

use self::placements::{ITEMS, counted_placements, rooted_placements, test_placements};
use super::*;
use crate::testkit::LintFixture;
use crate::{CrateSourceFile, check_workspace};

struct Spec {
    name:    &'static str,
    root:    String,
    mods:    Vec<(String, String)>,
    /// Where `root` sits: `src/lib.rs` unless a test says otherwise.
    root_at: &'static str,
}

fn at_root(name: &'static str, root: &str) -> Spec {
    Spec {
        name,
        root: root.to_string(),
        mods: Vec::new(),
        root_at: "src/lib.rs",
    }
}

fn with_files(name: &'static str, root: &str, mods: &[(&str, &str)]) -> Spec {
    Spec {
        name,
        root: root.to_string(),
        mods: mods
            .iter()
            .map(|(p, t)| (p.to_string(), t.to_string()))
            .collect(),
        root_at: "src/lib.rs",
    }
}

/// A fixture for each spec, with the root file at `root_at` and the rest beside it.
fn fixtures(specs: &[Spec]) -> Vec<LintFixture> {
    specs
        .iter()
        .map(|s| {
            let mut files = vec![CrateSourceFile {
                rel_path: PathBuf::from(s.root_at),
                text:     s.root.clone(),
            }];
            files.extend(s.mods.iter().map(|(p, t)| {
                CrateSourceFile {
                    rel_path: PathBuf::from(p),
                    text:     t.clone(),
                }
            }));
            LintFixture::new(&s.root)
                .with_crate_name(s.name, s.name)
                .with_sources(files)
        })
        .collect()
}

/// Run the lint over crates built from the specs, in order, comparing items of
/// every visibility: most of the fixtures below are private items, and what is
/// under test is where the lint reads, not which items it compares.
fn findings(specs: &[Spec]) -> Vec<LintError> {
    let fixtures = fixtures(specs);
    let ctxs: Vec<LintContext> = fixtures.iter().map(LintFixture::ctx).collect();
    let crates: Vec<(&str, &LintContext)> =
        specs.iter().zip(&ctxs).map(|(s, c)| (s.name, c)).collect();
    check(&crates, false)
}

/// The same specs through the production switch, which compares `pub` items only.
fn pub_findings(specs: &[Spec]) -> Vec<LintError> {
    let fixtures = fixtures(specs);
    let ctxs: Vec<LintContext> = fixtures.iter().map(LintFixture::ctx).collect();
    let crates: Vec<(&str, &LintContext)> =
        specs.iter().zip(&ctxs).map(|(s, c)| (s.name, c)).collect();
    SingleSource.check_all(&crates)
}

/// The same specs through the runner, which is what honours `lint:allow`.
fn through_the_runner(specs: &[Spec]) -> Vec<LintError> {
    let fixtures = fixtures(specs);
    let ctxs: Vec<LintContext> = fixtures.iter().map(LintFixture::ctx).collect();
    let crates: Vec<(&str, &LintContext)> =
        specs.iter().zip(&ctxs).map(|(s, c)| (s.name, c)).collect();
    check_workspace(&crates, false, None)
        .into_iter()
        .filter(|e| e.lint_name == "single-source")
        .collect()
}

/// A crate defining `item` at its root, the thing to be compared against.
fn original(item: &str) -> Spec {
    at_root("test-orig", item)
}

fn placed(name: &'static str, root: &str, files: &[(&'static str, String)]) -> Spec {
    Spec {
        name,
        root: root.to_string(),
        mods: files
            .iter()
            .map(|(p, t)| (p.to_string(), t.clone()))
            .collect(),
        root_at: "src/lib.rs",
    }
}

#[test]
fn a_definition_is_found_wherever_the_module_system_puts_it() {
    for (item, kind) in ITEMS {
        for (label, root, files) in counted_placements(item) {
            let there = placed("test-there", &root, &files);
            let found = findings(&[original(item), there]);
            assert_eq!(found.len(), 1, "{item} in a {label}, second: {found:?}");
            assert_eq!(found[0].crate_name, "test-there", "{item} in a {label}");
            assert_eq!(found[0].finding_kind, Some(*kind), "{item} in a {label}");

            let there = placed("test-there", &root, &files);
            let found = findings(&[there, original(item)]);
            assert_eq!(found.len(), 1, "{item} in a {label}, first: {found:?}");
            assert_eq!(found[0].crate_name, "test-orig", "{item} in a {label}");
        }
    }
}

#[test]
fn test_code_defines_nothing_wherever_it_sits() {
    for (item, _) in ITEMS {
        for (label, root, files) in test_placements(item) {
            let there = placed("test-there", &root, &files);
            let found = findings(&[original(item), there]);
            assert!(found.is_empty(), "{item} as {label}: {found:?}");
        }
    }
}

#[test]
fn the_placements_tables_cover_what_they_say() {
    // The controls on the two tables above: a table that was empty, or whose
    // rows all hit one branch, would pass both tests while checking nothing.
    assert!(counted_placements("struct Dup;").len() >= 15);
    assert!(test_placements("struct Dup;").len() >= 10);
    assert_eq!(ITEMS.iter().filter(|(_, k)| *k == "type").count(), 5);
    assert_eq!(ITEMS.iter().filter(|(_, k)| *k == "constant").count(), 4);
    // And a test row is not exempt by accident of its own spelling: the same
    // text without its `cfg(test)` is found.
    let not_test = findings(&[
        original("struct Dup;"),
        with_files("test-there", "mod tests;", &[(
            "src/tests.rs",
            "struct Dup;",
        )]),
    ]);
    assert_eq!(not_test.len(), 1);
}

#[test]
fn a_finding_names_its_file_and_a_root_finding_names_none() {
    let found = findings(&[
        original("struct Dup;"),
        with_files("test-b", "mod m;", &[("src/m.rs", "\n\nstruct Dup;")]),
        at_root("test-c", "\nstruct Dup;"),
    ]);
    assert_eq!(found.len(), 2);
    let b = found.iter().find(|e| e.crate_name == "test-b").unwrap();
    assert_eq!((b.path.as_deref(), b.line), (Some("src/m.rs"), 3));
    let c = found.iter().find(|e| e.crate_name == "test-c").unwrap();
    assert_eq!((c.path.as_deref(), c.line), (None, 2));
    assert!(b.message.contains("test-orig"), "{}", b.message);
}

#[test]
fn an_allow_marker_is_read_from_the_file_the_finding_is_in() {
    let allowed = "// lint:allow(single-source)\npub struct Dup;";
    let silenced = through_the_runner(&[
        original("pub struct Dup;"),
        with_files("test-b", "mod m;", &[("src/m.rs", allowed)]),
    ]);
    assert!(silenced.is_empty(), "{silenced:?}");

    // The marker in the root does not reach a finding in a module file.
    let wrong_file = through_the_runner(&[
        original("pub struct Dup;"),
        with_files("test-b", "// lint:allow(single-source)\nmod m;", &[(
            "src/m.rs",
            "pub struct Dup;",
        )]),
    ]);
    assert_eq!(wrong_file.len(), 1);

    // A marker for another lint does not silence this one.
    let other = through_the_runner(&[
        original("pub struct Dup;"),
        with_files("test-b", "mod m;", &[(
            "src/m.rs",
            "// lint:allow(no-todo)\npub struct Dup;",
        )]),
    ]);
    assert_eq!(other.len(), 1);
}

#[test]
fn one_crate_defining_a_name_twice_is_not_this_finding() {
    let found = findings(
        &[with_files("test-a", "struct Dup; mod m; const C: u8 = 1;", &[(
            "src/m.rs",
            "struct Dup; const C: u8 = 1;",
        )])],
    );
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn three_crates_report_the_two_that_are_not_the_first() {
    let found = findings(&[
        original("struct Dup;"),
        with_files("test-b", "mod m;", &[("src/m.rs", "struct Dup;")]),
        with_files("test-c", "mod m;", &[("src/m.rs", "struct Dup;")]),
    ]);
    let mut crates: Vec<&str> = found.iter().map(|e| e.crate_name.as_str()).collect();
    crates.sort();
    assert_eq!(crates, ["test-b", "test-c"]);
}

#[test]
fn findings_come_out_in_name_order() {
    let found = findings(&[
        at_root("test-a", "struct Zed; struct Alpha; struct Mid;"),
        at_root("test-b", "struct Mid; struct Zed; struct Alpha;"),
    ]);
    let names: Vec<String> = found
        .iter()
        .map(|e| e.message.split('`').nth(1).unwrap().to_string())
        .collect();
    assert_eq!(names, ["Alpha", "Mid", "Zed"]);
}

#[test]
fn the_macro_util_pairs_are_exempt() {
    for parent in ["behavior", "resource", "signal"] {
        let parent_name: &'static str = Box::leak(format!("test-{parent}").into_boxed_str());
        let found = findings(&[
            at_root(parent_name, "struct Dup;"),
            with_files("test-macro-util", "mod m;", &[("src/m.rs", "struct Dup;")]),
        ]);
        assert!(found.is_empty(), "{parent}: {found:?}");
    }
    // The exemption is for the pairs and not for the macro-util crate.
    let found = findings(&[
        at_root("test-other", "struct Dup;"),
        at_root("test-macro-util", "struct Dup;"),
    ]);
    assert_eq!(found.len(), 1);
}

#[test]
fn reexports_aliases_and_macro_bodies_are_not_definitions() {
    let found = findings(&[
        original("struct Dup; const DUP: u32 = 7;"),
        at_root(
            "test-b",
            "pub use other::Dup; pub use other::DUP; type Dup2 = u8; \
             macro_rules! make { () => { struct Dup; const DUP: u32 = 7; }; }",
        ),
    ]);
    assert!(found.is_empty(), "{found:?}");
    let alias = findings(&[original("struct Dup;"), at_root("test-b", "type Dup = u8;")]);
    assert!(alias.is_empty(), "{alias:?}");
}

#[test]
fn a_crate_without_module_files_falls_back_to_its_root() {
    // A context built with no `all_sources` at all: the root is still read.
    let fixture_a = LintFixture::new("struct Dup;").with_crate_name("test-a", "test-a");
    let fixture_b = LintFixture::new("struct Dup;")
        .with_crate_name("test-b", "test-b")
        .with_sources(Vec::new());
    let (a, b) = (fixture_a.ctx(), fixture_b.ctx());
    assert_eq!(check(&[("test-a", &a), ("test-b", &b)], false).len(), 1);
}

fn rooted(
    name: &'static str,
    at: &'static str,
    root: &str,
    files: &[(&'static str, String)],
) -> Spec {
    Spec {
        root_at: at,
        ..placed(name, root, files)
    }
}

#[test]
fn a_definition_is_found_under_every_kind_of_crate_root() {
    for (item, kind) in ITEMS {
        for (label, at, root, files) in rooted_placements(item) {
            let found = findings(&[original(item), rooted("test-there", at, &root, &files)]);
            assert_eq!(found.len(), 1, "{item} under a {label}: {found:?}");
            assert_eq!(found[0].finding_kind, Some(*kind), "{item} under a {label}");
            // The twin: the same crate with its first declaration test-gated. It is
            // exempt only if the walk reached the file from that root; a root the
            // lint did not know would leave the file an orphan, and counted.
            let gated = format!("#[cfg(test)] {root}");
            let twin = findings(&[original(item), rooted("test-there", at, &gated, &files)]);
            assert!(twin.is_empty(), "{item} under a gated {label}: {twin:?}");
        }
    }
}

#[test]
fn every_counted_placement_is_reached_and_not_merely_found() {
    // Found is not enough: an unreached file is found as an orphan. Gating the
    // declaration that leads to the file with `cfg(test)` makes it exempt only
    // when the walk really followed that declaration there.
    for (item, _) in ITEMS {
        for (label, root, files) in counted_placements(item) {
            if root.is_empty() {
                continue; // the orphan row: reached by nobody, by definition
            }
            let gated = format!("#[cfg(test)] {root}");
            let there = placed("test-there", &gated, &files);
            let found = findings(&[original(item), there]);
            assert!(found.is_empty(), "{item} in a gated {label}: {found:?}");
        }
    }
}

#[test]
fn only_fully_pub_items_are_compared_across_crates() {
    let private = [
        "struct Dup;",
        "pub(crate) struct Dup;",
        "pub(super) struct Dup;",
        "pub(self) struct Dup;",
        "pub(in crate::m) struct Dup;",
        "const DUP: u32 = 7;",
        "pub(crate) const DUP: u32 = 7;",
        "pub(super) static DUP: u32 = 7;",
        "pub(in crate::m) const DUP: u32 = 7;",
    ];
    for item in private {
        let public = item
            .replace("pub(crate) ", "")
            .replace("pub(super) ", "")
            .replace("pub(self) ", "")
            .replace("pub(in crate::m) ", "");
        let public = format!("pub {public}");
        let pub_item =
            if item.contains("static") { public.replace("const", "static") } else { public };
        // Private on either side, in either order: not compared.
        assert!(
            pub_findings(&[original(item), at_root("test-b", &pub_item)]).is_empty(),
            "{item} against {pub_item}"
        );
        assert!(
            pub_findings(&[original(&pub_item), at_root("test-b", item)]).is_empty(),
            "{pub_item} against {item}"
        );
        assert!(
            pub_findings(&[original(item), at_root("test-b", item)]).is_empty(),
            "{item} against itself"
        );
        // The control: with the switch off the same pair is a finding.
        assert_eq!(
            findings(&[original(item), at_root("test-b", &pub_item)]).len(),
            1,
            "{item}"
        );
    }
    for item in [
        "pub struct Dup;",
        "pub enum Dup { A }",
        "pub trait Dup {}",
        "pub union Dup { a: u8 }",
        "pub const DUP: u32 = 7;",
        "pub static DUP: u32 = 7;",
        "pub static mut DUP: u32 = 7;",
    ] {
        assert_eq!(
            pub_findings(&[original(item), at_root("test-b", item)]).len(),
            1,
            "{item}"
        );
    }
}

#[test]
fn the_message_names_the_visibility_it_found() {
    let msg = |item: &str| {
        findings(&[original(item), at_root("test-b", item)])[0]
            .message
            .clone()
    };
    assert!(
        msg("pub(crate) const DUP: u32 = 7;").starts_with("pub(crate) const `DUP"),
        "{}",
        msg("pub(crate) const DUP: u32 = 7;")
    );
    assert!(msg("const DUP: u32 = 7;").starts_with("const `DUP"));
    assert!(
        msg("pub static mut DUP: u32 = 7;").starts_with("pub static mut `DUP"),
        "{}",
        msg("pub static mut DUP: u32 = 7;")
    );
    assert!(msg("pub(super) struct Dup;").starts_with("pub(super) struct `Dup"));
}

mod constants;
mod limits;
mod placements;
