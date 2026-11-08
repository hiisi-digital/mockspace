use std::time::Duration;

use super::tests::{built, later, made};
use super::*;
use crate::custom_lints::build::{Built, Unit, UnitKind};

/// What `record` says of a build whose one tree unit is a procedural macro. `info` is the text of
/// the macro's dep-info (`None` leaves it unwritten), `source` the text of its one source (`None`
/// leaves it absent), and `allowed` whether the macro's dependencies are within the allowlist, with
/// the crate of the tree it uses.
fn macro_build(info: Option<&str>, source: Option<&str>, allowed: Option<Vec<PathBuf>>, beside: &[UnitKind]) -> bool {
    macro_build_with(info, source, allowed, beside, &[])
}

/// [`macro_build`] with library units of the tree beside it: each a crate directory and the text of
/// its dep-info (`None` leaves it unwritten).
fn macro_build_with(
    info: Option<&str>,
    source: Option<&str>,
    allowed: Option<Vec<PathBuf>>,
    beside: &[UnitKind],
    libs: &[(PathBuf, Option<String>)],
) -> bool {
    let tmp = tempfile::tempdir().unwrap();
    let gen_dir = tmp.path().join("gen");
    let crate_dir = tmp.path().join("derive");
    std::fs::create_dir_all(&crate_dir).unwrap();
    let source_path = crate_dir.join("lib.rs");
    if let Some(text) = source {
        std::fs::write(&source_path, text).unwrap();
    }
    let mut roots = Roots::default();
    roots.add("lints", &crate_dir);
    for (n, (dir, _)) in libs.iter().enumerate() {
        roots.add(format!("lib{n}"), dir);
    }
    roots.add("tree", tmp.path());
    if let Some(closure) = allowed {
        roots.allow_macros(std::collections::BTreeMap::from([(crate_dir.clone(), closure)]));
    }
    let dylib = built(&gen_dir, "out.d: \n");
    let deps = dylib.parent().unwrap().join("deps");
    if let Some(text) = info {
        let text = text.replace("SOURCE", &source_path.display().to_string());
        std::fs::write(deps.join("tool-0f.d"), text).unwrap();
    }
    let unit = Unit {
        files: vec![deps.join("libtool-0f.rlib")],
        fresh: false,
        kind: UnitKind::ProcMacro,
        in_tree: true,
        manifest: crate_dir.join("Cargo.toml"),
    };
    let mut units: Vec<Unit> = beside
        .iter()
        .map(|kind| Unit { files: unit.files.clone(), fresh: false, kind: *kind, in_tree: true, manifest: unit.manifest.clone() })
        .collect();
    units.insert(0, unit);
    for (n, (dir, text)) in libs.iter().enumerate() {
        if let Some(text) = text {
            std::fs::write(deps.join(format!("lib{n}-1.d")), text).unwrap();
        }
        units.push(Unit {
            files: vec![deps.join(format!("liblib{n}-1.rlib"))],
            fresh: false,
            kind: UnitKind::Lib,
            in_tree: true,
            manifest: dir.join("Cargo.toml"),
        });
    }
    let build = Built { units, ..made(&dylib, later(), false) };
    record(&build, &gen_dir, &roots, Duration::ZERO).is_some()
}

const LISTED: &str = "tool-0f.rlib: SOURCE\n";
const PURE: &str = "use proc_macro::TokenStream;\npub fn derive(i: TokenStream) -> TokenStream { i }\n";

#[test]
fn a_pure_macro_of_the_tree_with_allowed_dependencies_leaves_the_library_kept() {
    assert!(macro_build(Some(LISTED), Some(PURE), Some(vec![]), &[]));
}

#[test]
fn a_macro_with_a_dependency_outside_the_allowlist_leaves_nothing_kept() {
    assert!(!macro_build(Some(LISTED), Some(PURE), None, &[]));
}

#[test]
fn a_pure_macro_beside_a_build_script_of_the_tree_leaves_nothing_kept() {
    // The script is of the same package and its dep-info is as readable as the macro's, so only its
    // kind refuses it.
    assert!(!macro_build(Some(LISTED), Some(PURE), Some(vec![]), &[UnitKind::BuildScript]));
}

#[test]
fn a_macro_whose_dep_info_cannot_be_read_leaves_nothing_kept() {
    assert!(!macro_build(None, Some(PURE), Some(vec![]), &[]));
}

#[test]
fn a_macro_with_a_source_it_cannot_read_leaves_nothing_kept() {
    assert!(!macro_build(Some(LISTED), None, Some(vec![]), &[]));
}

#[test]
fn a_macro_whose_dep_info_lists_no_source_leaves_nothing_kept() {
    assert!(!macro_build(Some("tool-0f.rlib:\n"), Some(PURE), Some(vec![]), &[]));
}

#[test]
fn a_macro_that_imports_a_refused_module_under_an_alias_leaves_nothing_kept() {
    let source = "use std::{fs as f};\npub fn derive() { f::read_to_string(\"x\"); }\n";
    assert!(!macro_build(Some(LISTED), Some(source), Some(vec![]), &[]));
}

/// A crate of the tree whose sources are `files` (name and text) under a directory of its own, with
/// a dep-info listing `listed`.
fn helper(tmp: &Path, files: &[(&str, &str)], listed: &[&str]) -> (PathBuf, Option<String>) {
    let dir = tmp.join("helper");
    for (name, text) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let paths: Vec<String> = listed.iter().map(|f| dir.join(f).display().to_string()).collect();
    (dir, Some(format!("lib0-1.rlib: {}\n", paths.join(" "))))
}

#[test]
fn a_macro_that_uses_a_crate_of_the_tree_that_can_read_a_file_leaves_nothing_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let bad = helper(tmp.path(), &[("src/lib.rs", "pub fn h() { let _ = std::fs::read(\"Cargo.toml\"); }")], &["src/lib.rs"]);
    assert!(!macro_build_with(Some(LISTED), Some(PURE), Some(vec![bad.0.clone()]), &[], &[bad]));
    let good = helper(tmp.path(), &[("src/lib.rs", "pub fn h() {}")], &["src/lib.rs"]);
    assert!(macro_build_with(Some(LISTED), Some(PURE), Some(vec![good.0.clone()]), &[], &[good]));
}

#[test]
fn a_crate_of_the_tree_that_reaches_files_through_include_and_path_leaves_nothing_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let files = [
        ("src/lib.rs", "include!(\"impl.inc\");\n#[path = \"../../shared.rs\"]\nmod shared;\n"),
        ("src/impl.inc", "pub fn a() { let _ = std::fs::read(\"x\"); }"),
        ("../shared.rs", "pub fn b() { let _ = std::env::var(\"X\"); }"),
    ];
    let bad = helper(tmp.path(), &files, &["src/lib.rs", "src/impl.inc", "../shared.rs"]);
    assert!(!macro_build_with(Some(LISTED), Some(PURE), Some(vec![bad.0.clone()]), &[], &[bad]));
}

#[test]
fn a_crate_of_the_tree_in_the_closure_with_no_library_dep_info_leaves_nothing_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let mut crate_ = helper(tmp.path(), &[("src/lib.rs", "pub fn h() {}")], &["src/lib.rs"]);
    crate_.1 = None;
    assert!(!macro_build_with(Some(LISTED), Some(PURE), Some(vec![crate_.0.clone()]), &[], &[crate_]));
    let dir = tmp.path().join("helper");
    assert!(!macro_build_with(Some(LISTED), Some(PURE), Some(vec![dir]), &[], &[]), "no unit at all");
    let empty = helper(tmp.path(), &[("src/lib.rs", "pub fn h() {}")], &[]);
    assert!(!macro_build_with(Some(LISTED), Some(PURE), Some(vec![empty.0.clone()]), &[], &[empty]), "no source listed");
}
