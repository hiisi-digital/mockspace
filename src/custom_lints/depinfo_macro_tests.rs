use std::time::Duration;

use super::tests::{built, later, made};
use super::*;
use crate::custom_lints::build::{Built, Unit, UnitKind};

/// What `record` says of a build whose one tree unit is a procedural macro. `info` is the text of
/// the macro's dep-info (`None` leaves it unwritten), `source` the text of its one source (`None`
/// leaves it absent), and `allowed` whether the macro's dependencies are within the allowlist, with
/// the crate of the tree it uses.
fn macro_build(info: Option<&str>, source: Option<&str>, allowed: Option<Vec<PathBuf>>, beside: &[UnitKind]) -> bool {
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

#[test]
fn a_macro_that_uses_a_crate_of_the_tree_that_can_read_a_file_leaves_nothing_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = tmp.path().join("helper");
    std::fs::create_dir_all(helper.join("src")).unwrap();
    std::fs::write(helper.join("src").join("lib.rs"), "pub fn h() { let _ = std::fs::read(\"Cargo.toml\"); }").unwrap();
    assert!(!macro_build(Some(LISTED), Some(PURE), Some(vec![helper.clone()]), &[]));
    std::fs::write(helper.join("src").join("lib.rs"), "pub fn h() {}").unwrap();
    assert!(macro_build(Some(LISTED), Some(PURE), Some(vec![helper]), &[]));
}
