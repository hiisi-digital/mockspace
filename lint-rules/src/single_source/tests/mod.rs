//! Tests for `single-source`.
//!
//! The reach tests are tables: every kind of definition against every place the
//! module system can put it, because "the lint sees a module file" is a claim about
//! the whole set of placements and a sample of it would pass with the lint blind
//! to the rest. Each table row is one crate holding the definition there and one
//! holding it at its root, in both orders.

use super::*;
use crate::check_workspace;
use crate::testkit::LintFixture;

struct Spec {
    name: &'static str,
    root: String,
    mods: Vec<(String, String)>,
}

fn at_root(name: &'static str, root: &str) -> Spec {
    Spec {
        name,
        root: root.to_string(),
        mods: Vec::new(),
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
    }
}

/// Run the lint over crates built from the specs, in order.
fn findings(specs: &[Spec]) -> Vec<LintError> {
    let fixtures: Vec<LintFixture> = specs
        .iter()
        .map(|s| {
            let mut f = LintFixture::new(&s.root).with_crate_name(s.name, s.name);
            for (path, text) in &s.mods {
                f = f.with_module(path, text);
            }
            f
        })
        .collect();
    let ctxs: Vec<LintContext> = fixtures.iter().map(LintFixture::ctx).collect();
    let crates: Vec<(&str, &LintContext)> =
        specs.iter().zip(&ctxs).map(|(s, c)| (s.name, c)).collect();
    SingleSource.check_all(&crates)
}

/// The same specs through the runner, which is what honours `lint:allow`.
fn through_the_runner(specs: &[Spec]) -> Vec<LintError> {
    let fixtures: Vec<LintFixture> = specs
        .iter()
        .map(|s| {
            let mut f = LintFixture::new(&s.root).with_crate_name(s.name, s.name);
            for (path, text) in &s.mods {
                f = f.with_module(path, text);
            }
            f
        })
        .collect();
    let ctxs: Vec<LintContext> = fixtures.iter().map(LintFixture::ctx).collect();
    let crates: Vec<(&str, &LintContext)> =
        specs.iter().zip(&ctxs).map(|(s, c)| (s.name, c)).collect();
    check_workspace(&crates, false, None)
        .into_iter()
        .filter(|e| e.lint_name == "single-source")
        .collect()
}

/// `(label, root text, files)`: one place a definition can sit.
type Placement = (&'static str, String, Vec<(&'static str, String)>);

/// One definition of every kind the lint compares, with its finding kind.
const ITEMS: &[(&str, &str)] = &[
    ("struct Dup;", "type"),
    ("pub struct Dup { pub x: u8 }", "type"),
    ("enum Dup { A }", "type"),
    ("trait Dup {}", "type"),
    ("union Dup { a: u8 }", "type"),
    ("const DUP: u32 = 7;", "constant"),
    ("pub const DUP: u32 = 7;", "constant"),
    ("static DUP: u32 = 7;", "constant"),
    ("pub static mut DUP: u32 = 7;", "constant"),
];

/// Every place a definition counts from: `(label, root, files)`, `{}` the item.
fn counted_placements(item: &str) -> Vec<Placement> {
    let i = item.to_string();
    vec![
        ("root", i.clone(), vec![]),
        ("module file", "mod m;".into(), vec![(
            "src/m.rs",
            i.clone(),
        )]),
        ("mod.rs", "mod m;".into(), vec![("src/m/mod.rs", i.clone())]),
        ("pub mod", "pub mod m;".into(), vec![(
            "src/m.rs",
            i.clone(),
        )]),
        ("pub(crate) mod", "pub(crate) mod m;".into(), vec![(
            "src/m.rs",
            i.clone(),
        )]),
        ("nested file", "mod a;".into(), vec![
            ("src/a.rs", "mod b;".into()),
            ("src/a/b.rs", i.clone()),
        ]),
        ("nested mod.rs", "mod a;".into(), vec![
            ("src/a/mod.rs", "mod b;".into()),
            ("src/a/b.rs", i.clone()),
        ]),
        ("inline mod", format!("mod m {{ {item} }}"), vec![]),
        (
            "nested inline mod",
            format!("mod m {{ mod n {{ {item} }} }}"),
            vec![],
        ),
        ("file under an inline mod", "mod m { mod n; }".into(), vec![
            ("src/m/n.rs", i.clone()),
        ]),
        (
            "path attribute",
            "#[path = \"sys/linux.rs\"] mod sys;".into(),
            vec![("src/sys/linux.rs", i.clone())],
        ),
        (
            "path attribute in an inline mod",
            "mod m { #[path = \"x.rs\"] mod n; }".into(),
            vec![("src/m/x.rs", i.clone())],
        ),
        (
            "child of a path attribute file",
            "#[path = \"sys/like_mod.rs\"] mod sys;".into(),
            vec![("src/sys/like_mod.rs", "mod child;".into()), ("src/sys/child.rs", i.clone())],
        ),
        (
            "cfg feature module file",
            "#[cfg(feature = \"x\")] mod m;".into(),
            vec![("src/m.rs", i.clone())],
        ),
        (
            "cfg not test inline",
            format!("#[cfg(not(test))] mod m {{ {item} }}"),
            vec![],
        ),
        (
            "cfg any test inline",
            format!("#[cfg(any(test, unix))] mod m {{ {item} }}"),
            vec![],
        ),
        (
            "cfg latest feature",
            format!("#[cfg(feature = \"latest\")] mod m {{ {item} }}"),
            vec![],
        ),
        (
            "cfg_attr is not cfg",
            format!("#[cfg_attr(test, derive(Debug))] {item}"),
            vec![],
        ),
        ("file nothing declares", "".into(), vec![(
            "src/loose.rs",
            i.clone(),
        )]),
    ]
}

/// Every place a definition is test code and so defines nothing.
fn test_placements(item: &str) -> Vec<Placement> {
    let i = item.to_string();
    vec![
        (
            "item under cfg(test)",
            format!("#[cfg(test)] {item}"),
            vec![],
        ),
        (
            "inline test mod",
            format!("#[cfg(test)] mod tests {{ {item} }}"),
            vec![],
        ),
        (
            "inner cfg(test) in an inline mod",
            format!("mod t {{ #![cfg(test)] {item} }}"),
            vec![],
        ),
        ("test mod file", "#[cfg(test)] mod tests;".into(), vec![(
            "src/tests.rs",
            i.clone(),
        )]),
        ("test mod.rs", "#[cfg(test)] mod tests;".into(), vec![(
            "src/tests/mod.rs",
            i.clone(),
        )]),
        (
            "cfg all test",
            "#[cfg(all(test, unix))] mod tests;".into(),
            vec![("src/tests.rs", i.clone())],
        ),
        (
            "file opening with an inner cfg(test)",
            "mod m;".into(),
            vec![("src/m.rs", format!("#![cfg(test)]\n{item}"))],
        ),
        ("child of a test file", "#[cfg(test)] mod t;".into(), vec![
            ("src/t.rs", "mod u;".into()),
            ("src/t/u.rs", i.clone()),
        ]),
        (
            "child of a test file, three deep",
            "#[cfg(test)] mod t;".into(),
            vec![
                ("src/t.rs", "mod u;".into()),
                ("src/t/u.rs", "mod v;".into()),
                ("src/t/u/v.rs", i.clone()),
            ],
        ),
        (
            "file under an inline test mod",
            "#[cfg(test)] mod tests { mod c; }".into(),
            vec![("src/tests/c.rs", i.clone())],
        ),
        (
            "path attribute in an inline test mod",
            "#[cfg(test)] mod t { #[path = \"x.rs\"] mod n; }".into(),
            vec![("src/t/x.rs", i.clone())],
        ),
        (
            "test mod behind a path attribute",
            "#[cfg(test)]\n#[path = \"x.rs\"]\nmod m;".into(),
            vec![("src/x.rs", i.clone())],
        ),
        (
            "child of a test path attribute file",
            "#[cfg(test)] #[path = \"sys/m.rs\"] mod m;".into(),
            vec![("src/sys/m.rs", "mod c;".into()), ("src/sys/c.rs", i.clone())],
        ),
    ]
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
    let allowed = "// lint:allow(single-source)\nstruct Dup;";
    let silenced = through_the_runner(&[
        original("struct Dup;"),
        with_files("test-b", "mod m;", &[("src/m.rs", allowed)]),
    ]);
    assert!(silenced.is_empty(), "{silenced:?}");

    // The marker in the root does not reach a finding in a module file.
    let wrong_file = through_the_runner(&[
        original("struct Dup;"),
        with_files("test-b", "// lint:allow(single-source)\nmod m;", &[(
            "src/m.rs",
            "struct Dup;",
        )]),
    ]);
    assert_eq!(wrong_file.len(), 1);

    // A marker for another lint does not silence this one.
    let other = through_the_runner(&[
        original("struct Dup;"),
        with_files("test-b", "mod m;", &[(
            "src/m.rs",
            "// lint:allow(no-todo)\nstruct Dup;",
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
    assert_eq!(
        SingleSource
            .check_all(&[("test-a", &a), ("test-b", &b)])
            .len(),
        1
    );
}

mod constants;
mod limits;
