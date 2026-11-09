use super::*;

fn refused(source: &str) -> bool {
    !source_is_pure(source)
}

#[test]
fn a_derive_that_names_only_pure_modules_and_its_own_dependencies_is_pure() {
    assert!(source_is_pure(
        "use proc_macro::TokenStream;\nuse quote::quote;\nuse syn::{DeriveInput, parse_macro_input};\n\
         use std::collections::BTreeMap;\nextern crate proc_macro;\n\
         pub fn derive(i: TokenStream) -> TokenStream { let _m: BTreeMap<String, Vec<u8>> = BTreeMap::new(); \
         let n = ::std::format!(\"{}\", 1); quote! { ::std::fs::read(#n) } .into() }\n"
    ));
}

#[test]
fn a_path_into_a_module_that_reaches_outside_the_program_is_refused_wherever_it_stands() {
    for module in ["fs", "env", "path", "io", "process", "net", "os", "thread", "time", "ffi", "sync", "alloc_error"] {
        for root in ["std", "core", "alloc", "r#std", "r#core"] {
            if module == "alloc" {
                continue;
            }
            assert!(refused(&format!("fn f() {{ {root}::{module}::x(); }}")), "{root}::{module} in an expression");
            assert!(refused(&format!("fn f() {{ ::{root}::{module}::x(); }}")), "::{root}::{module}");
            assert!(refused(&format!("use {root}::{module};")), "use {root}::{module}");
            assert!(refused(&format!("use {root}::{module}::x;")), "use {root}::{module}::x");
            assert!(refused(&format!("fn f(_: {root}::{module}::T) {{}}")), "{root}::{module} in a type");
            assert!(refused(&format!("fn f() {{ let _ = <T as {root}::{module}::Tr>::x; }}")), "in a qualified path");
        }
    }
}

#[test]
fn an_alias_or_a_list_or_a_glob_of_a_refused_module_is_refused() {
    for source in [
        "use std::{fs as f}; fn g() { f::read_to_string(p); }",
        "use std::fs as f;",
        "use std::env::{var_os as v};",
        "use std::{collections::BTreeMap, fs};",
        "use std::{collections::{BTreeMap}, io::{self, Read}};",
        "use std::*;",
        "use core::*;",
        "use alloc::*;",
        "use std::fs::*;",
        "use std::{self};",
        "use std as s;",
        "use ::std::process::Command;",
        "use std::path::Path; fn g(p: &str) -> bool { Path::new(p).exists() }",
        "use std::env; fn g() { let _ = env::current_dir(); }",
        "extern crate std as s;",
        "extern crate std;",
    ] {
        assert!(refused(source), "{source}");
    }
}

#[test]
fn a_glob_or_alias_of_a_pure_module_is_not_refused() {
    assert!(source_is_pure("use std::collections::*; use std::fmt as f; use std::{vec, iter::Iterator};"));
}

#[test]
fn a_refused_path_inside_the_arguments_of_a_macro_that_is_not_a_quoting_one_is_refused() {
    for source in [
        "fn g() { let _ = format!(\"{:?}\", std::fs::read(p)); }",
        "fn g() { dbg!(::std::env::var(\"X\")); }",
        "macro_rules! m { () => { std::fs::read(\"x\") } }",
        "fn g() { let _ = vec![core::str::from_utf8(b\"a\"), std::process::id()]; }",
        "fn g() { wrap!(std); }",
    ] {
        assert!(refused(source), "{source}");
    }
}

#[test]
fn the_body_of_a_quoting_macro_is_generated_code_and_not_read() {
    for name in ["quote", "::quote::quote", "quote::quote", "quote::quote_spanned", "syn::parse_quote", "::syn::parse_quote", "syn::parse_quote_spanned", "quote_spanned", "parse_quote", "parse_quote_spanned"] {
        assert!(source_is_pure(&format!("fn g() {{ let _ = {name}! {{ std::fs::read(\"x\") }}; }}")), "{name}");
    }
    assert!(refused("fn g() { let _ = std::fs::read(\"x\"); let _ = quote! { 1 }; }"));
}

#[test]
fn a_macro_that_only_has_the_name_of_a_quoting_one_is_not_exempt() {
    for source in [
        "macro_rules! quote { ($($t:tt)*) => { $($t)* } } fn f() -> String { quote!(std::fs::read_to_string(\"/x\").unwrap()) }",
        "macro_rules! parse_quote { ($($t:tt)*) => { $($t)* } }",
        "macro_rules! quote_spanned { () => {} }",
        "fn f() { helper::quote!(std::fs::read_to_string(\"/x\")); }",
        "fn f() { other::syn::parse_quote!(std::fs::read(\"x\")); }",
        "use helper::quote; fn f() { quote!(std::fs::read(\"x\")); }",
        "use helper::make as quote;",
        "use quote::quote as parse_quote;",
        "use syn::{parse_quote, other as quote_spanned};",
        "use other::parse_quote;",
    ] {
        assert!(refused(source), "{source}");
    }
    assert!(source_is_pure("use quote::quote; use syn::{parse_quote, parse_quote_spanned}; use quote::quote_spanned;"));
}

#[test]
fn a_foreign_block_assembly_or_a_second_extern_crate_is_refused() {
    for source in [
        "extern \"C\" { fn getenv(n: *const u8) -> *const u8; }",
        "fn g() { unsafe { asm!(\"nop\"); } }",
        "fn g() { core::arch::asm!(\"nop\"); }",
        "fn g() { unsafe { global_asm!(\"nop\"); } }",
        "extern crate libc;",
    ] {
        assert!(refused(source), "{source}");
    }
}

#[test]
fn a_source_that_does_not_parse_is_refused() {
    assert!(refused("fn g( {"));
    assert!(refused("use std::fs"));
}

#[test]
fn a_function_declared_extern_is_not_a_foreign_block() {
    assert!(source_is_pure("pub extern \"C\" fn g() {}"));
}

fn metadata(dep_kind: serde_json::Value, dep_name: &str, dep_source: &str) -> serde_json::Value {
    let pkg = |id: &str, name: &str, source: Option<&str>, kind: &str| {
        serde_json::json!({
            "id": id, "name": name, "source": source,
            "manifest_path": format!("/r/{name}/Cargo.toml"),
            "targets": [{"kind": [kind]}],
        })
    };
    serde_json::json!({
        "packages": [
            pkg("derive 0.0.0", "derive", None, "proc-macro"),
            pkg("helper 0.0.0", "helper", None, "lib"),
            pkg("dep 1.0.0", dep_name, Some(dep_source), "lib"),
            pkg("syn 2.0.0", "syn", Some("registry+https://github.com/rust-lang/crates.io-index"), "lib"),
        ],
        "resolve": {"nodes": [
            {"id": "derive 0.0.0", "deps": [
                {"pkg": "syn 2.0.0", "dep_kinds": [{"kind": null}]},
                {"pkg": "helper 0.0.0", "dep_kinds": [{"kind": null}]},
            ]},
            {"id": "helper 0.0.0", "deps": [{"pkg": "dep 1.0.0", "dep_kinds": [{"kind": dep_kind}]}]},
            {"id": "dep 1.0.0", "deps": []},
            {"id": "syn 2.0.0", "deps": []},
        ]},
    })
}

const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

#[test]
fn a_macro_whose_closure_stays_in_the_allowlist_lists_the_crates_of_the_tree_in_it() {
    let got = macro_closures(&metadata(serde_json::Value::Null, "quote", REGISTRY));
    assert_eq!(got.get(Path::new("/r/derive")).map(Vec::as_slice), Some([PathBuf::from("/r/helper")].as_slice()));
}

#[test]
fn a_dependency_outside_the_allowlist_anywhere_in_the_closure_leaves_the_macro_with_no_entry() {
    for (name, source) in [
        ("proc-macro-crate", REGISTRY),
        ("serde_json", REGISTRY),
        ("syn", "git+https://example.invalid/syn"),
        ("quote", "path+file:///elsewhere"),
    ] {
        let got = macro_closures(&metadata(serde_json::Value::Null, name, source));
        assert!(!got.contains_key(Path::new("/r/derive")), "{name} from {source}");
    }
}

#[test]
fn a_dependency_only_a_build_or_a_test_needs_is_not_in_the_closure() {
    assert!(macro_closures(&metadata("dev".into(), "serde_json", REGISTRY)).contains_key(Path::new("/r/derive")));
    assert!(macro_closures(&metadata("build".into(), "serde_json", REGISTRY)).contains_key(Path::new("/r/derive")));
}

#[test]
fn metadata_without_a_resolve_gives_no_macro() {
    assert!(macro_closures(&serde_json::json!({"packages": []})).is_empty());
}

fn written(dir: &Path, name: &str, text: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn info(sources: &[&Path]) -> String {
    let list: Vec<String> = sources.iter().map(|s| s.display().to_string()).collect();
    format!("derive.d: {}\n", list.join(" "))
}

#[test]
fn a_macro_is_held_when_its_sources_and_those_of_the_crates_of_the_tree_it_uses_are_pure() {
    let tmp = tempfile::tempdir().unwrap();
    let own = written(&tmp.path().join("derive"), "lib.rs", "pub fn f() {}");
    let helper = written(&tmp.path().join("helper").join("src"), "lib.rs", "pub fn h() {}");
    assert!(held(&info(&[&own]), tmp.path(), &[info(&[&helper])]));
}

#[test]
fn a_macro_is_not_held_when_a_crate_of_the_tree_it_uses_can_read_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let own = written(&tmp.path().join("derive"), "lib.rs", "pub fn f() {}");
    let helper = written(&tmp.path().join("helper").join("src"), "lib.rs", "pub fn h() {}");
    let deep = written(&tmp.path().join("helper").join("src").join("deep"), "io.rs", "fn r() { std::fs::read(\"Cargo.toml\"); }");
    assert!(!held(&info(&[&own]), tmp.path(), &[info(&[&helper, &deep])]));
}

fn a_macro_is_not_held_without_sources_or_with_one_it_cannot_read() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!held("derive.d:\n", tmp.path(), &[]), "an empty source list");
    assert!(!held("", tmp.path(), &[]), "an empty dep-info");
    let gone = tmp.path().join("gone.rs");
    assert!(!held(&info(&[&gone]), tmp.path(), &[]), "a source that is not there");
    let own = written(tmp.path(), "lib.rs", "pub fn f() {}");
    assert!(!held(&info(&[&own]), tmp.path(), &[info(&[&tmp.path().join("no-such-crate.rs")])]), "a source of a crate of the tree that is not there");
    assert!(!held(&info(&[&own]), tmp.path(), &["lib0.rlib:\n".to_string()]), "a crate of the tree that lists no source");
}

#[test]
fn a_relative_source_is_read_against_the_directory_the_build_ran_in() {
    let tmp = tempfile::tempdir().unwrap();
    written(&tmp.path().join("rel"), "lib.rs", "pub fn f() {}");
    assert!(held("derive.d: rel/lib.rs\n", tmp.path(), &[]));
    written(&tmp.path().join("bad"), "lib.rs", "fn f() { std::env::var(\"X\"); }");
    assert!(!held("derive.d: bad/lib.rs\n", tmp.path(), &[]));
}

#[test]
fn a_raw_identifier_for_a_root_or_a_module_does_not_hide_it() {
    for source in [
        "fn f() -> usize { r#std::fs::read_to_string(\"/x\").unwrap().len() }",
        "use r#std::env as e; fn g() -> bool { e::var_os(\"HOME\").is_some() }",
        "use r#core::*;",
        "use std::r#fs;",
        "fn f() { std::r#env::var(\"X\"); }",
        "fn f() { dbg!(r#std::fs::read(\"x\")); }",
        "fn f() { dbg!(std::r#fs::read(\"x\")); }",
    ] {
        assert!(refused(source), "{source}");
    }
    assert!(source_is_pure("use std::r#vec::Vec; fn f() -> r#std::string::String { r#std::string::String::new() }"));
}

#[test]
fn a_registry_other_than_the_crates_io_index_is_not_the_allowlisted_crate() {
    let sparse = "sparse+https://index.crates.io/";
    assert!(macro_closures(&metadata(serde_json::Value::Null, "quote", sparse)).contains_key(Path::new("/r/derive")));
    for other in ["registry+https://evil.invalid/index", "sparse+https://evil.invalid/", "registry+https://github.com/rust-lang/crates.io-index-evil"] {
        assert!(!macro_closures(&metadata(serde_json::Value::Null, "quote", other)).contains_key(Path::new("/r/derive")), "{other}");
    }
}

#[test]
fn a_proc_macro_crate_of_the_tree_in_the_closure_leaves_the_macro_with_no_entry() {
    let mut json = metadata(serde_json::Value::Null, "quote", REGISTRY);
    let helper = json["packages"].as_array_mut().unwrap().iter_mut().find(|p| p["name"] == "helper").unwrap();
    helper["targets"] = serde_json::json!([{"kind": ["proc-macro"]}]);
    assert!(!macro_closures(&json).contains_key(Path::new("/r/derive")));
}
