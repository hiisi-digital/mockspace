//! What the lint cannot see, one test each. Every test has the visible variant of
//! the same shape beside it, so a pass is the limit and not a lint that is simply
//! off.

use super::*;

fn one(a: &str, b: &str) -> usize {
    findings(&[at_root("test-a", a), at_root("test-b", b)]).len()
}

#[test]
fn items_a_macro_generates_are_not_seen() {
    assert_eq!(one("struct Dup;", "struct Dup;"), 1);
    assert_eq!(one("struct Dup;", "make!(Dup);"), 0);
    assert_eq!(
        one(
            "struct Dup;",
            "macro_rules! m { () => { struct Dup; }; } m!();"
        ),
        0
    );
}

#[test]
fn a_path_outside_src_is_not_followed() {
    let inside = findings(&[
        original("struct Dup;"),
        with_files("test-b", "#[path = \"shared/dup.rs\"] mod d;", &[(
            "src/shared/dup.rs",
            "struct Dup;",
        )]),
    ]);
    assert_eq!(inside.len(), 1);
    // The same shape pointing out of src/: the file is not in `all_sources`
    // and so does not exist to the lint.
    let outside = findings(&[
        original("struct Dup;"),
        at_root("test-b", "#[path = \"../shared/dup.rs\"] mod d;"),
    ]);
    assert!(outside.is_empty(), "{outside:?}");
}

#[test]
fn a_mod_declaration_without_its_file_defines_nothing() {
    assert!(findings(&[original("struct Dup;"), at_root("test-b", "mod missing;")]).is_empty());
}

#[test]
fn a_file_nothing_reaches_is_counted_wherever_it_sits() {
    // The engine hands over `src/**` only, so a `tests/` file never arrives;
    // that is the limit, held by the engine and stated in the module doc. What
    // this pins is the fallback for a file that does arrive and that no `mod`
    // reaches: it is ordinary source, whatever directory it sits in.
    let handed = findings(&[
        original("struct Dup;"),
        with_files("test-b", "", &[("tests/it.rs", "struct Dup;")]),
    ]);
    assert_eq!(handed.len(), 1);
    let absent = findings(&[original("struct Dup;"), at_root("test-b", "")]);
    assert!(absent.is_empty());
}

#[test]
fn items_in_function_bodies_are_not_seen() {
    assert_eq!(one("struct Dup;", "fn f() { struct Dup; }"), 0);
    assert_eq!(one("const X: u8 = 1;", "fn f() { const X: u8 = 1; }"), 0);
    assert_eq!(one("const X: u8 = 1;", "const X: u8 = 1;"), 1);
}

#[test]
fn associated_constants_are_not_seen() {
    assert_eq!(
        one("const X: u8 = 1;", "struct S; impl S { const X: u8 = 1; }"),
        0
    );
    assert_eq!(one("const X: u8 = 1;", "trait T { const X: u8 = 1; }"), 0);
}

#[test]
fn a_foreign_static_has_no_value_to_compare() {
    assert_eq!(
        one("static X: u8 = 1;", "extern \"C\" { static X: u8; }"),
        0
    );
}

#[test]
fn a_name_is_compared_as_written() {
    assert_eq!(one("struct Dup;", "struct Dup;"), 1);
    assert_eq!(one("struct Dup;", "pub use other::Dup as Renamed;"), 0);
    assert_eq!(one("struct Dup;", "struct Duplicate;"), 0);
}

#[test]
fn values_are_not_evaluated() {
    assert_eq!(one("const X: u32 = 16;", "const X: u32 = 16;"), 1);
    assert_eq!(one("const X: u32 = 16;", "const X: u32 = 1 << 4;"), 0);
    assert_eq!(one("const X: u32 = 6;", "const X: u32 = 2 * 3;"), 0);
    assert_eq!(
        one("const X: &str = \"A\";", "const X: &str = \"\\u{41}\";"),
        0
    );
    assert_eq!(one("const X: &str = \"A\";", "const X: &str = r\"A\";"), 0);
    // A suffix on a literal inside an expression decides the result, so it is kept
    // there and `1000` against `1000u32` differ; as the whole value they agree.
    assert_eq!(one("const X: u32 = 1000;", "const X: u32 = 1000u32;"), 1);
    assert_eq!(
        one("const X: u32 = 1000 + 1;", "const X: u32 = 1000u32 + 1;"),
        0
    );
}

#[test]
fn an_open_value_is_never_reported() {
    assert_eq!(one("const X: u32 = 1;", "const X: u32 = 1;"), 1);
    assert_eq!(one("const X: u32 = OTHER;", "const X: u32 = OTHER;"), 0);
    assert_eq!(
        one("const X: u32 = u32::MAX;", "const X: u32 = u32::MAX;"),
        0
    );
    assert_eq!(one("const X: u32 = f();", "const X: u32 = f();"), 0);
    assert_eq!(
        one("const X: u32 = 1 + OTHER;", "const X: u32 = 1 + OTHER;"),
        0
    );
    assert_eq!(
        one("const X: S = S { a: 1 };", "const X: S = S { a: 1 };"),
        0
    );
    assert_eq!(one("const X: u32 = m!();", "const X: u32 = m!();"), 0);
    assert_eq!(one("const X: u32 = { 1 };", "const X: u32 = { 1 };"), 0);
    assert_eq!(
        one("const X: &[u32] = &[A, 1];", "const X: &[u32] = &[A, 1];"),
        0
    );
}

#[test]
fn a_type_spelled_through_another_path_does_not_match() {
    assert_eq!(one("const X: u32 = 1;", "const X: u32 = 1;"), 1);
    assert_eq!(
        one("const X: u32 = 1;", "const X: core::primitive::u32 = 1;"),
        0
    );
}

#[test]
fn a_non_test_cfg_does_not_exempt_a_platform_pair() {
    assert_eq!(
        one(
            "#[cfg(unix)] const X: u32 = 1;",
            "#[cfg(windows)] const X: u32 = 1;"
        ),
        1
    );
    assert_eq!(
        one("#[cfg(unix)] struct Dup;", "#[cfg(windows)] struct Dup;"),
        1
    );
}

#[test]
fn cfg_attr_is_not_read() {
    // A `cfg_attr` that would make the module a test module, or give it a
    // path, changes nothing the lint acts on.
    let found = findings(&[
        original("struct Dup;"),
        with_files("test-b", "#[cfg_attr(test, path = \"x.rs\")] mod m;", &[(
            "src/m.rs",
            "struct Dup;",
        )]),
    ]);
    assert_eq!(found.len(), 1);
    let moved = findings(&[
        original("struct Dup;"),
        with_files("test-b", "#[cfg_attr(all(), path = \"x.rs\")] mod m;", &[(
            "src/x.rs",
            "struct Dup;",
        )]),
    ]);
    // The decision: `cfg_attr` resolves as if it were not there, so `mod m;` looks
    // for `src/m.rs`, which is absent. `src/x.rs` is then a file nothing reaches,
    // counted as the orphan fallback says, and named by its own path.
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].path.as_deref(), Some("src/x.rs"));
    let default = findings(&[
        original("struct Dup;"),
        with_files("test-b", "#[cfg_attr(all(), path = \"x.rs\")] mod m;", &[(
            "src/m.rs",
            "struct Dup;",
        )]),
    ]);
    assert_eq!(default.len(), 1);
    assert_eq!(default[0].path.as_deref(), Some("src/m.rs"));
}

#[test]
fn a_custom_crate_root_is_not_read_from_the_manifest() {
    // The roots are the conventional files. A crate whose root is `src/z.rs`
    // (a `[lib] path`) has no root the lint knows, so a file that root declares
    // under `cfg(test)` is an orphan and is counted: a false positive, which
    // is the documented limit. The same files under `src/lib.rs` are exempt.
    let mut custom = at_root("test-b", "#[cfg(test)] mod a_tests;");
    custom.root_at = "src/z.rs";
    custom.mods = vec![("src/a_tests.rs".to_string(), "struct Dup;".to_string())];
    assert_eq!(findings(&[original("struct Dup;"), custom]).len(), 1);
    let conventional = findings(&[
        original("struct Dup;"),
        with_files("test-b", "#[cfg(test)] mod a_tests;", &[(
            "src/a_tests.rs",
            "struct Dup;",
        )]),
    ]);
    assert!(conventional.is_empty(), "{conventional:?}");
}
