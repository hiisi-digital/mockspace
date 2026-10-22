//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which files a test binary is built from, and what makes it read any of them.
//! Each rule is asserted both ways: the file it claims, and the file beside it
//! that it must not.

use super::*;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

/// A member `m` holding these files, read.
struct Fx {
    index: Index,
    facts: BTreeMap<PathBuf, Facts>,
}

impl Fx {
    fn new(files: &[(&str, &str)]) -> Fx {
        let list: Vec<PathBuf> = files.iter().map(|(f, _)| p(f)).collect();
        let facts = files
            .iter()
            .filter(|(f, _)| f.ends_with(".rs"))
            .map(|(f, src)| (p(f), facts(src)))
            .collect();
        Fx {
            index: Index::new(&list),
            facts,
        }
    }

    fn layout(&self, manifest: &str) -> Layout {
        let roots = roots(manifest, Path::new("m"), &self.index).expect("narrowable manifest");
        layout(
            &Ctx {
                // Nothing climbs out of the member here, and nothing exists to find.
                repo_root:  Path::new("/nonexistent-repo-root"),
                member_dir: Path::new("m"),
                index:      &self.index,
                facts:      &self.facts,
            },
            &roots,
        )
    }
}

const PLAIN: &str = "[package]\nname = \"m\"\n";

fn files_of(l: &Layout, target: &str) -> Vec<String> {
    l.units[target]
        .files
        .iter()
        .map(|f| f.display().to_string())
        .collect()
}

// ---- reading a source ----

#[test]
fn a_module_declared_at_the_top_level_is_a_declaration_and_one_nested_is_opaque() {
    let f = facts("mod a;\n#[cfg(test)]\nmod b;\nmod inline { fn x() {} }\n");
    assert_eq!(
        f.mods.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    assert!(!f.opaque && !f.ambient);

    let nested = facts("mod outer { mod inner; }\n");
    assert!(
        nested.opaque,
        "a `mod x;` inside an inline module sits somewhere else"
    );
    assert!(nested.mods.is_empty());
}

#[test]
fn a_path_attribute_is_noticed_however_it_is_wrapped() {
    for src in [
        "#[path = \"x.rs\"]\nmod a;",
        "#[path=\"x.rs\"] mod a;",
        "#[cfg_attr(unix, path = \"x.rs\")]\nmod a;",
        "#[path = \"x.rs\"]\n// comment between\nmod a;",
    ] {
        let f = facts(src);
        assert_eq!(f.mods.len(), 1, "{src}");
        assert!(f.mods[0].path_attr, "{src}");
        assert!(f.strings.contains(&"x.rs".to_string()), "{src}");
    }
    // A name that only contains the word is not the attribute.
    let f = facts("#[cfg(feature = \"my_path\")]\nmod a;");
    assert!(!f.mods[0].path_attr);
    assert!(!facts("mod a;").mods[0].path_attr);
}

#[test]
fn a_module_inside_a_macro_is_opaque() {
    assert!(facts("cfg_if! { if #[cfg(unix)] { mod unix_only; } }").opaque);
    assert!(facts("macro_rules! m { () => { mod generated; } }").opaque);
    assert!(!facts("fn f() { assert!(1 < 2, \"a module is fine to mention\"); }").opaque);
    assert!(!facts("fn f() { println!(\"submodule is not mod\"); }").opaque);
}

#[test]
fn an_unparsable_source_is_opaque() {
    assert!(facts("fn ((( {").opaque);
}

/// The ways a test reaches the environment or builds a path from its own.
#[test]
fn every_way_of_reading_the_environment_is_ambient() {
    for src in [
        "fn f() { let _ = std::env::var(\"X\"); }",
        "use std::env;\nfn f() { let _ = env::var(\"X\"); }",
        "use std::{env, fs};\nfn f() {}",
        "use std::env::var;\nfn f() {}",
        "fn f() { let _ = env!(\"CARGO_MANIFEST_DIR\"); }",
        "fn f() { let _ = option_env!(\"X\"); }",
        "fn f() { let _ = std::env!(\"X\"); }",
        "fn f() { let _ = file!(); }",
        "fn f() { let _ = std::env::current_dir(); }",
        "fn f() { assert!(!env!(\"CARGO_MANIFEST_DIR\").is_empty()); }",
        "fn f() { println!(\"{}\", std::env::var(\"X\").unwrap()); }",
        "fn f() { assert_eq!(1, 1, \"{}\", file!()); }",
        "fn f() { dbg!(option_env!(\"X\")); }",
    ] {
        assert!(facts(src).ambient, "{src}");
    }
}

/// The controls for the above: things named `env` that are not the module, and
/// the word in text.
#[test]
fn a_variable_named_env_and_the_word_in_a_comment_are_not() {
    for src in [
        "fn f() { let env = 1; let _ = env + 1; }",
        "// reads std::env::var(\"X\")\nfn f() {}",
        "fn f() { let _ = \"std::env::var\"; }",
        "struct Env { env: u8 }\nfn f(e: Env) -> u8 { e.env }",
        "fn f() { println!(\"the env and file! of it, option_env\"); }",
    ] {
        assert!(!facts(src).ambient, "{src}");
    }
}

#[test]
fn only_a_literal_that_could_be_a_path_is_kept() {
    let f = facts(
        "fn f() { let _ = (\"fixtures/a.json\", \"\", \"two\\nlines\", \"..\", \"../x\", r#\"raw/path\"#); }",
    );
    assert!(f.strings.contains(&"fixtures/a.json".to_string()));
    assert!(f.strings.contains(&"raw/path".to_string()));
    assert!(f.strings.contains(&"../x".to_string()));
    assert!(!f.strings.iter().any(|s| s.is_empty() || s.contains('\n')));
    assert_eq!(f.climbing, vec!["../x".to_string()]);
}

// ---- which targets a package has ----

fn names(r: &[Root]) -> Vec<String> {
    r.iter().map(|r| r.name.clone()).collect()
}

#[test]
fn flat_files_and_main_in_a_directory_are_targets_and_nothing_else_is() {
    let fx = Fx::new(&[
        ("m/tests/a.rs", ""),
        ("m/tests/b.rs", ""),
        ("m/tests/composed/main.rs", ""),
        ("m/tests/composed/extra.rs", ""),
        ("m/tests/common/mod.rs", ""),
        ("m/tests/data/x.json", ""),
        ("m/tests/deep/inner/main.rs", ""),
        ("m/src/lib.rs", ""),
    ]);
    let r = roots(PLAIN, Path::new("m"), &fx.index).unwrap();
    assert_eq!(names(&r), vec!["a", "b", "composed"]);
    assert!(r.iter().find(|r| r.name == "composed").unwrap().dir_form);
    assert!(!r.iter().find(|r| r.name == "a").unwrap().dir_form);
}

#[test]
fn switching_autodiscovery_off_leaves_the_declared_targets() {
    let fx = Fx::new(&[("m/tests/a.rs", ""), ("m/tests/b.rs", "")]);
    let m = "[package]\nname = \"m\"\nautotests = false\n\n[[test]]\nname = \"only\"\npath = \"tests/b.rs\"\n";
    let r = roots(m, Path::new("m"), &fx.index).unwrap();
    assert_eq!(names(&r), vec!["only"]);
    assert_eq!(r[0].file, p("m/tests/b.rs"));
}

#[test]
fn a_declared_target_without_a_path_is_found_where_cargo_looks() {
    let fx = Fx::new(&[("m/tests/flat.rs", ""), ("m/tests/dir/main.rs", "")]);
    let m = "[package]\nname = \"m\"\nautotests = false\n\n[[test]]\nname = \"flat\"\n\n[[test]]\nname = \"dir\"\n";
    let r = roots(m, Path::new("m"), &fx.index).unwrap();
    assert_eq!(names(&r), vec!["dir", "flat"]);
    assert!(r[0].dir_form && !r[1].dir_form);
}

/// Each of these is something this cannot tell from the manifest, so nothing is
/// narrowed: `None`, which the caller reads as every file shared.
#[test]
fn a_manifest_that_cannot_be_told_narrows_nothing() {
    let fx = Fx::new(&[("m/tests/a.rs", ""), ("m/src/lib.rs", ""), ("m/other/t.rs", "")]);
    for (why, m) in [
        ("unparsable", "[package"),
        (
            "a test outside tests/",
            "[package]\nname = \"m\"\n[[test]]\nname = \"t\"\npath = \"other/t.rs\"\n",
        ),
        (
            "a lib rooted under tests/",
            "[package]\nname = \"m\"\n[lib]\npath = \"tests/a.rs\"\n",
        ),
        (
            "a bin rooted under tests/",
            "[package]\nname = \"m\"\n[[bin]]\nname = \"b\"\npath = \"tests/a.rs\"\n",
        ),
        (
            "a test with no name",
            "[package]\nname = \"m\"\n[[test]]\npath = \"tests/a.rs\"\n",
        ),
        (
            "a declared test whose file is not there",
            "[package]\nname = \"m\"\n[[test]]\nname = \"gone\"\n",
        ),
    ] {
        assert!(roots(m, Path::new("m"), &fx.index).is_none(), "{why}");
    }
}

// ---- what each target is built from ----

#[test]
fn a_flat_target_claims_its_own_file_and_nothing_beside_it() {
    let fx = Fx::new(&[
        ("m/tests/a.rs", "#[test] fn t() {}"),
        ("m/tests/b.rs", "#[test] fn t() {}"),
        ("m/tests/data/x.json", "{}"),
        ("m/src/lib.rs", ""),
    ]);
    let l = fx.layout(PLAIN);
    assert_eq!(files_of(&l, "a"), vec!["m/tests/a.rs"]);
    assert_eq!(files_of(&l, "b"), vec!["m/tests/b.rs"]);
    assert!(
        !l.claimed.contains(&p("m/tests/data/x.json")),
        "claimed by nobody, so shared"
    );
    assert!(!l.claimed.contains(&p("m/src/lib.rs")));
    assert!(!l.units["a"].wide && !l.units["b"].wide);
}

#[test]
fn a_module_a_target_declares_is_its_own_and_not_its_neighbours() {
    let fx = Fx::new(&[
        ("m/tests/a.rs", "mod common;\nmod flat;\n"),
        ("m/tests/b.rs", ""),
        ("m/tests/common/mod.rs", "mod inner;"),
        ("m/tests/common/inner.rs", ""),
        ("m/tests/flat.rs", ""),
    ]);
    let l = fx.layout(PLAIN);
    assert_eq!(files_of(&l, "a"), vec![
        "m/tests/a.rs",
        "m/tests/common/inner.rs",
        "m/tests/common/mod.rs",
        "m/tests/flat.rs"
    ]);
    // `flat.rs` is a target of its own as well as a module of `a`.
    assert_eq!(files_of(&l, "flat"), vec!["m/tests/flat.rs"]);
    assert_eq!(files_of(&l, "b"), vec!["m/tests/b.rs"]);
}

/// A module file that is not a `mod.rs` looks in a directory of its own stem;
/// a `mod.rs` and the crate root look beside themselves.
#[test]
fn a_module_looks_where_the_compiler_looks() {
    let fx = Fx::new(&[
        ("m/tests/a.rs", "mod helper;"),
        ("m/tests/helper.rs", "mod deeper;"),
        ("m/tests/helper/deeper.rs", ""),
        // Where a root's `mod deeper;` would look, and a `helper.rs` must not.
        ("m/tests/deeper.rs", ""),
    ]);
    let l = fx.layout(PLAIN);
    let a = files_of(&l, "a");
    assert!(a.contains(&"m/tests/helper/deeper.rs".to_string()), "{a:?}");
    assert!(!a.contains(&"m/tests/deeper.rs".to_string()), "{a:?}");
}

#[test]
fn the_directory_form_owns_everything_under_its_directory() {
    let fx = Fx::new(&[
        ("m/tests/composed/main.rs", "mod scene;"),
        ("m/tests/composed/scene.rs", ""),
        ("m/tests/composed/golden.png", "png"),
        ("m/tests/other.rs", ""),
    ]);
    let l = fx.layout(PLAIN);
    assert_eq!(files_of(&l, "composed"), vec![
        "m/tests/composed/golden.png",
        "m/tests/composed/main.rs",
        "m/tests/composed/scene.rs"
    ]);
    assert_eq!(files_of(&l, "other"), vec!["m/tests/other.rs"]);
}

#[test]
fn a_path_literal_claims_what_it_names_for_the_target_that_wrote_it() {
    let fx = Fx::new(&[
        (
            "m/tests/a.rs",
            "const J: &str = include_str!(\"fixtures/x.json\");\n#[path = \"support/h.rs\"]\nmod h;",
        ),
        ("m/tests/b.rs", ""),
        ("m/tests/fixtures/x.json", "{}"),
        ("m/tests/fixtures/y.json", "{}"),
        ("m/tests/support/h.rs", ""),
    ]);
    let l = fx.layout(PLAIN);
    let a = files_of(&l, "a");
    assert!(a.contains(&"m/tests/fixtures/x.json".to_string()), "{a:?}");
    assert!(a.contains(&"m/tests/support/h.rs".to_string()), "{a:?}");
    assert!(!a.contains(&"m/tests/fixtures/y.json".to_string()), "{a:?}");
    assert_eq!(files_of(&l, "b"), vec!["m/tests/b.rs"]);
}

/// A directory named by a literal claims everything under it, which is how a
/// test that walks a fixtures directory is covered, and a wildcard cut at the
/// component holding it is how a `trybuild` glob is.
#[test]
fn a_directory_or_a_glob_claims_everything_under_it() {
    let fx = Fx::new(&[
        (
            "m/tests/walk.rs",
            "fn f() { let _ = std::path::Path::new(\"fixtures\"); }",
        ),
        (
            "m/tests/ui.rs",
            "fn f() { trybuild::TestCases::new().compile_fail(\"tests/ui/*.rs\"); }",
        ),
        ("m/tests/fixtures/a/1.json", ""),
        ("m/tests/fixtures/b.json", ""),
        ("m/tests/ui/case.rs", ""),
        ("m/tests/ui/case.stderr", ""),
        ("m/tests/other.rs", ""),
    ]);
    let l = fx.layout(PLAIN);
    let walk = files_of(&l, "walk");
    assert!(
        walk.contains(&"m/tests/fixtures/a/1.json".to_string()),
        "{walk:?}"
    );
    assert!(
        walk.contains(&"m/tests/fixtures/b.json".to_string()),
        "{walk:?}"
    );
    let ui = files_of(&l, "ui");
    assert!(ui.contains(&"m/tests/ui/case.rs".to_string()), "{ui:?}");
    assert!(
        ui.contains(&"m/tests/ui/case.stderr".to_string()),
        "the glob matches .rs, the directory is claimed: {ui:?}"
    );
    assert!(!ui.iter().any(|f| f.contains("fixtures")), "{ui:?}");
    assert_eq!(files_of(&l, "other"), vec!["m/tests/other.rs"]);
}

#[test]
fn a_literal_naming_another_target_claims_it_into_this_one_too() {
    let fx = Fx::new(&[
        ("m/tests/a.rs", "const B: &str = include_str!(\"b.rs\");"),
        ("m/tests/b.rs", "mod helper;"),
        ("m/tests/helper.rs", ""),
    ]);
    let l = fx.layout(PLAIN);
    assert!(files_of(&l, "a").contains(&"m/tests/b.rs".to_string()));
    // Where a source pulled in by a literal declares modules, nothing says where
    // they would sit, so `a` is wide. `b`, which is that file as a target, is not.
    assert!(l.units["a"].wide);
    assert!(!l.units["b"].wide);
    assert_eq!(files_of(&l, "b"), vec!["m/tests/b.rs", "m/tests/helper.rs"]);
}

/// A literal that reaches the member's library is a shared file's business and
/// is not claimed, and one that reaches nothing claims nothing.
#[test]
fn a_literal_into_src_or_at_nothing_claims_nothing() {
    let fx = Fx::new(&[
        (
            "m/tests/a.rs",
            "fn f() { let _ = (\"../src/lib.rs\", \"nowhere.json\", \".\", \"\"); }",
        ),
        ("m/src/lib.rs", ""),
        ("m/tests/z.rs", ""),
    ]);
    let l = fx.layout(PLAIN);
    assert_eq!(files_of(&l, "a"), vec!["m/tests/a.rs"]);
    assert!(!l.units["a"].wide);
}

/// The widening rules: each of these makes a target depend on every claimed file.
#[test]
fn a_target_that_may_read_anything_is_wide() {
    for (why, src) in [
        ("env!", "fn f() { let _ = env!(\"CARGO_MANIFEST_DIR\"); }"),
        ("env::var", "fn f() { let _ = std::env::var(\"X\"); }"),
        ("file!", "fn f() { let _ = file!(); }"),
        ("a mod in an inline module", "mod x { mod y; }"),
        ("a mod in a macro", "cfg_if! { mod z; }"),
        ("a source that does not parse", "fn ((("),
    ] {
        let fx = Fx::new(&[("m/tests/a.rs", src), ("m/tests/b.rs", "#[test] fn quiet() {}")]);
        let l = fx.layout(PLAIN);
        assert!(l.units["a"].wide, "{why}");
        assert!(
            !l.units["b"].wide,
            "{why}: the neighbour is not widened by it"
        );
    }
}

/// Wideness comes from any file of the unit, not only the root.
#[test]
fn a_helper_that_reads_the_environment_widens_every_target_that_includes_it() {
    let fx = Fx::new(&[
        ("m/tests/a.rs", "mod h;"),
        ("m/tests/b.rs", "mod h;"),
        ("m/tests/c.rs", ""),
        (
            "m/tests/h/mod.rs",
            "pub fn root() -> String { std::env::var(\"X\").unwrap() }",
        ),
    ]);
    let l = fx.layout(PLAIN);
    assert!(l.units["a"].wide && l.units["b"].wide);
    assert!(!l.units["c"].wide);
}

/// What shared sources name that sits under `tests/` is shared as well.
#[test]
fn a_claimed_file_the_library_names_is_shared() {
    let fx = Fx::new(&[
        (
            "m/src/lib.rs",
            "const J: &str = include_str!(\"../tests/fixtures/x.json\");",
        ),
        (
            "m/tests/a.rs",
            "fn f() { let _ = (\"fixtures/x.json\", \"fixtures/only_a.json\"); }",
        ),
        ("m/tests/b.rs", ""),
        ("m/tests/fixtures/x.json", "{}"),
        ("m/tests/fixtures/only_a.json", "{}"),
    ]);
    let l = fx.layout(PLAIN);
    assert!(l.claimed.contains(&p("m/tests/fixtures/x.json")));
    assert!(l.shared_pulls.contains(&p("m/tests/fixtures/x.json")));
    assert!(
        !l.shared_pulls.contains(&p("m/tests/fixtures/only_a.json")),
        "only a names this one, so only a is built from it"
    );
}

// ---- resolving a literal ----

#[test]
fn a_literal_is_matched_from_the_files_directory_and_the_members() {
    let fx = Fx::new(&[
        ("m/tests/sub/a.rs", ""),
        ("m/tests/sub/near.txt", ""),
        ("m/page/build", ""),
        ("m/tests/sub/page/build", ""),
        ("other/page/build", ""),
    ]);
    let r = |lit: &str| resolve(lit, Path::new("m/tests/sub"), Path::new("m"), &fx.index);
    assert_eq!(r("near.txt"), vec![p("m/tests/sub/near.txt")]);
    assert_eq!(r("../../page/build"), vec![p("m/page/build")]);
    // Both places, when both hold the name.
    assert_eq!(r("page/build"), vec![
        p("m/page/build"),
        p("m/tests/sub/page/build")
    ]);
    // Absolute-looking, as `concat!(env!("CARGO_MANIFEST_DIR"), "/page/build")` writes it.
    assert_eq!(r("/page/build").len(), 2);
}

#[test]
fn a_literal_outside_the_member_or_naming_everything_resolves_to_nothing() {
    let fx = Fx::new(&[("m/tests/a.rs", ""), ("other/x", ""), ("top", "")]);
    let r = |lit: &str| resolve(lit, Path::new("m/tests"), Path::new("m"), &fx.index);
    assert!(
        r("../../other/x").is_empty(),
        "outside the member is a reach, not a claim"
    );
    assert!(r("../..").is_empty(), "an ancestor is not the member");
    assert!(r("..").is_empty(), "the member itself is everything");
    assert!(r(".").is_empty());
    assert!(r("").is_empty());
    assert!(
        r("*.rs").is_empty(),
        "a wildcard at the first component cuts to nothing"
    );
}

#[test]
fn a_wildcard_cuts_the_path_at_its_component() {
    let fx = Fx::new(&[("m/tests/ui/a.rs", ""), ("m/tests/ui/b/c.rs", ""), ("m/tests/z.rs", "")]);
    let r = |lit: &str| resolve(lit, Path::new("m/tests"), Path::new("m"), &fx.index);
    assert_eq!(r("tests/ui/*.rs"), vec![
        p("m/tests/ui/a.rs"),
        p("m/tests/ui/b/c.rs")
    ]);
    assert_eq!(r("ui/**/*.rs"), vec![
        p("m/tests/ui/a.rs"),
        p("m/tests/ui/b/c.rs")
    ]);
    assert_eq!(r("ui/[ab].rs"), vec![
        p("m/tests/ui/a.rs"),
        p("m/tests/ui/b/c.rs")
    ]);
}

// ---- the index ----

#[test]
fn the_index_answers_a_file_or_everything_under_a_directory() {
    let idx = Index::new(&[p("a/b/c.rs"), p("a/b/d/e.rs"), p("a/bb/f.rs"), p("a/c.rs")]);
    assert!(idx.has_file(Path::new("a/c.rs")));
    assert!(!idx.has_file(Path::new("a/b")));
    assert_eq!(idx.under(Path::new("a/c.rs")), vec![p("a/c.rs")]);
    assert_eq!(idx.under(Path::new("a/b")), vec![
        p("a/b/c.rs"),
        p("a/b/d/e.rs")
    ]);
    assert!(idx.under(Path::new("a/zzz")).is_empty());
}

// ---- literals rooted at a placeholder ----

#[test]
fn a_placeholder_is_a_name_and_the_path_after_it() {
    assert_eq!(
        placeholder("$CARGO_MANIFEST_DIR/tests/x"),
        Some(("CARGO_MANIFEST_DIR", "tests/x"))
    );
    assert_eq!(
        placeholder("${CARGO_MANIFEST_DIR}/tests/x"),
        Some(("CARGO_MANIFEST_DIR", "tests/x"))
    );
    assert_eq!(placeholder("$OUT_DIR"), Some(("OUT_DIR", "")));
    assert_eq!(placeholder("${OUT_DIR}/g.txt"), Some(("OUT_DIR", "g.txt")));
    // Not placeholders: no leading `$`, no name, a name run into more text, an
    // unclosed brace, and a dollar in the middle.
    for lit in ["tests/$X", "$", "$/x", "$X.txt", "${X/x", "${}/x", "a$X/b", "^end$"] {
        assert_eq!(placeholder(lit), None, "{lit}");
    }
}

#[test]
fn the_manifest_dir_placeholder_resolves_against_the_member_alone() {
    let fx = Fx::new(&[
        ("m/tests/dir/golden.txt", ""),
        ("m/tests/sub/tests/dir/golden.txt", ""),
        ("m/tests/sub/a.rs", ""),
    ]);
    let r = |lit: &str| resolve(lit, Path::new("m/tests/sub"), Path::new("m"), &fx.index);
    assert_eq!(r("$CARGO_MANIFEST_DIR/tests/dir/golden.txt"), vec![p(
        "m/tests/dir/golden.txt"
    )]);
    assert_eq!(r("${CARGO_MANIFEST_DIR}/tests/dir"), vec![p(
        "m/tests/dir/golden.txt"
    )]);
    // A wildcard cuts the path at its component, as for any literal.
    assert_eq!(
        r("${CARGO_MANIFEST_DIR}/tests/*/golden.txt"),
        fx.index.under(Path::new("m/tests"))
    );
    // Another name is not the member, and names nothing here.
    assert!(r("$OUT_DIR/tests/dir/golden.txt").is_empty());
}

/// A placeholder the environment fills, or one for the whole member, is a path
/// this cannot place, so the binary is wide; the member's own directory and a
/// path under it is placed, and is not.
#[test]
fn a_placeholder_this_cannot_place_makes_a_binary_wide() {
    for (lit, wide) in [
        ("$OUT_DIR/g.txt", true),
        ("${HOME}/x", true),
        ("$CARGO_MANIFEST_DIR", true),
        ("$CARGO_MANIFEST_DIR/", true),
        ("$CARGO_MANIFEST_DIR/tests/dir/golden.txt", false),
        ("${CARGO_MANIFEST_DIR}/tests/dir", false),
        ("tests/$X", false),
    ] {
        assert_eq!(
            facts(&format!("const D: &str = \"{lit}\";")).ambient,
            wide,
            "{lit}"
        );
    }
}

// ---- what a pulled file brings with it ----

/// The library compiles `tests/common/mod.rs` by `#[path]`, so what that file
/// declares and names is compiled into the library too, however deep.
#[test]
fn a_file_the_library_pulls_brings_its_modules_and_literals_all_the_way_down() {
    let fx = Fx::new(&[
        (
            "m/src/lib.rs",
            "#[cfg(test)]\n#[path = \"../tests/common/mod.rs\"]\nmod common;\n",
        ),
        ("m/tests/a.rs", "mod common;\n"),
        ("m/tests/b.rs", ""),
        (
            "m/tests/common/mod.rs",
            "mod inner;\nconst D: &str = include_str!(\"data/x.json\");\n",
        ),
        ("m/tests/common/inner.rs", "mod deeper;\n"),
        (
            "m/tests/common/inner/deeper.rs",
            "const E: &str = include_str!(\"../../only_a.json\");\n",
        ),
        ("m/tests/common/data/x.json", "{}"),
        ("m/tests/only_a.json", "{}"),
    ]);
    // `only_a.json` is named only by a file the library pulls; the others by `a`.
    let mut fx = fx;
    fx.facts.insert(
        p("m/tests/a.rs"),
        facts("mod common;\nconst O: &str = include_str!(\"only_a.json\");\n"),
    );
    let l = fx.layout(PLAIN);
    for f in [
        "m/tests/common/mod.rs",
        "m/tests/common/inner.rs",
        "m/tests/common/inner/deeper.rs",
        "m/tests/common/data/x.json",
        "m/tests/only_a.json",
    ] {
        assert!(l.claimed.contains(&p(f)), "{f} is claimed");
        assert!(
            l.shared_pulls.contains(&p(f)),
            "{f} is shared with the library: {:?}",
            l.shared_pulls
        );
    }
    assert!(
        !l.shared_pulls.contains(&p("m/tests/a.rs")),
        "a root is not pulled by naming it"
    );
    assert!(!l.shared_pulls.contains(&p("m/tests/b.rs")));
}

#[test]
fn what_a_pulled_file_reads_outside_the_member_is_the_librarys_read() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::fs::create_dir_all(repo.join("content")).unwrap();
    std::fs::write(repo.join("content/other.toml"), "x").unwrap();
    let files = [
        (
            "m/src/lib.rs",
            "#[path = \"../tests/common/mod.rs\"]\nmod common;\n",
        ),
        ("m/tests/a.rs", "mod common;\n"),
        (
            "m/tests/common/mod.rs",
            "const O: &str = include_str!(\"../../../content/other.toml\");\n",
        ),
    ];
    let fx = Fx::new(&files);
    let roots = roots(PLAIN, Path::new("m"), &fx.index).unwrap();
    let l = layout(
        &Ctx {
            repo_root:  repo,
            member_dir: Path::new("m"),
            index:      &fx.index,
            facts:      &fx.facts,
        },
        &roots,
    );
    assert!(
        l.shared_reaches.contains(&p("content/other.toml")),
        "{:?}",
        l.shared_reaches
    );
    assert!(
        l.units["a"].reaches.contains(&p("content/other.toml")),
        "and the unit keeps it"
    );
}
