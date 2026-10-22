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
fn files_outside_src_are_not_read() {
    // The fixture is handed the file under a `tests/` path, as nothing in
    // the engine does: `all_sources` is `src/**` only. Unreachable from the
    // roots, it counts as an orphan, which is the lint's fallback and not a
    // reading of `tests/`; the engine never hands one over.
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
    assert_eq!(
        moved.len(),
        1,
        "an orphan, found as one, not followed as a path"
    );
}
