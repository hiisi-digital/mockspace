use super::*;

fn consts(a: &str, b: &str) -> Vec<LintError> {
    findings(&[at_root("test-a", a), with_files("test-b", "mod m;", &[("src/m.rs", b)])])
}

fn agree(a: &str, b: &str) {
    let found = consts(a, b);
    assert_eq!(
        found.len(),
        1,
        "`{a}` against `{b}` should agree: {found:?}"
    );
    let back = consts(b, a);
    assert_eq!(back.len(), 1, "`{b}` against `{a}` should agree: {back:?}");
}

fn differ(a: &str, b: &str) {
    let found = consts(a, b);
    assert!(
        found.is_empty(),
        "`{a}` against `{b}` should differ: {found:?}"
    );
    let back = consts(b, a);
    assert!(
        back.is_empty(),
        "`{b}` against `{a}` should differ: {back:?}"
    );
}

#[test]
fn the_same_name_type_and_value_is_one_concept_defined_twice() {
    let found = consts("pub const MAX: u32 = 64;", "const MAX: u32 = 64;");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].crate_name, "test-b");
    assert_eq!(found[0].finding_kind, Some("constant"));
    assert!(
        found[0].message.contains("const `MAX: u32 = 64`"),
        "{}",
        found[0].message
    );
    assert!(found[0].message.contains("test-a"), "{}", found[0].message);
    assert!(!found[0].message.starts_with("pub"), "{}", found[0].message);
    let public = consts("const MAX: u32 = 64;", "pub const MAX: u32 = 64;");
    assert!(
        public[0].message.starts_with("pub const"),
        "{}",
        public[0].message
    );
}

#[test]
fn a_different_value_under_one_name_is_not_this_finding() {
    differ("const MAX: u32 = 64;", "const MAX: u32 = 65;");
    differ("const NAME: &str = \"a\";", "const NAME: &str = \"b\";");
    differ("const ON: bool = true;", "const ON: bool = false;");
    differ(
        "const LIMITS: [u8; 2] = [1, 2];",
        "const LIMITS: [u8; 2] = [1, 3];",
    );
    differ("const NEG: i32 = -1;", "const NEG: i32 = 1;");
}

#[test]
fn a_different_type_under_one_name_is_not_this_finding() {
    differ("const MAX: u32 = 64;", "const MAX: u64 = 64;");
    differ("const MAX: u32 = 64;", "const MAX: usize = 64;");
    differ("const A: [u8; 2] = [1, 2];", "const A: [u16; 2] = [1, 2];");
}

#[test]
fn the_same_value_under_two_names_is_not_this_finding() {
    differ("const MAX: u32 = 64;", "const LIMIT: u32 = 64;");
    differ("const MAX: u32 = 64;", "const max: u32 = 64;");
}

#[test]
fn a_const_and_a_static_of_one_name_are_two_things() {
    differ("const X: u32 = 1;", "static X: u32 = 1;");
    differ("static X: u32 = 1;", "static mut X: u32 = 1;");
    agree("static X: u32 = 1;", "static X: u32 = 1;");
    agree("static mut X: u32 = 1;", "static mut X: u32 = 1;");
}

#[test]
fn spellings_of_one_value_agree() {
    agree("const N: u32 = 1000;", "const N: u32 = 1_000;");
    agree("const N: u32 = 1000;", "const N: u32 = 1000u32;");
    agree("const N: u32 = 1000;", "const N: u32 = 0x3E8;");
    agree("const N: u32 = 1000;", "const N: u32 = 0x3e8_u32;");
    agree("const N: u32 = 8;", "const N: u32 = 0o10;");
    agree("const N: u32 = 5;", "const N: u32 = 0b101;");
    agree("const N: u32 = 0;", "const N: u32 = 000;");
    agree("const F: f64 = 1.0;", "const F: f64 = 1.00;");
    agree("const F: f64 = 1.0;", "const F: f64 = 1.0f64;");
    agree("const F: f64 = 1.5e3;", "const F: f64 = 1500.0;");
    agree("const F: f32 = 2.0;", "const F: f32 = 2f32;");
    agree("const S: u32 = 1 << 4;", "const S: u32 = 1<<4;");
    agree("const S: u32 = 1 << 4;", "const S: u32 = 1 /* x */ << 4;");
    agree("const A: [u8; 4] = [0; 4];", "const A: [u8;4] = [ 0 ; 4 ];");
    agree(
        "const T: (u8, bool) = (1, true);",
        "const T: (u8, bool) = ( 1 , true );",
    );
    agree("const R: &[u8] = &[1, 2];", "const R: &[u8] = &[ 1,2 ];");
    agree("const C: u8 = 7 as u8;", "const C: u8 = 7  as  u8;");
    agree("const U: () = ();", "const U: () = ( );");
    agree("const B: &[u8] = b\"ab\";", "const B: &[u8] = b\"ab\";");
    agree("const S: &str = \"a b\";", "const S: &str = \"a b\";");
    agree("const C: char = 'x';", "const C: char = 'x';");
    agree(
        "const H: u128 = 0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffff;",
        "const H: u128 = 340282366920938463463374607431768211455;",
    );
}

#[test]
fn spellings_of_different_values_do_not() {
    differ("const N: u32 = 1000;", "const N: u32 = 0x3E9;");
    differ("const N: u32 = 10;", "const N: u32 = 0x10;");
    differ("const N: u32 = 8;", "const N: u32 = 0o10 + 1;");
    differ("const F: f64 = 1.0;", "const F: f64 = 1.1;");
    differ("const S: u32 = 1 << 4;", "const S: u32 = 1 >> 4;");
    differ("const S: u32 = 2 + 3 * 4;", "const S: u32 = (2 + 3) * 4;");
    differ("const A: [u8; 3] = [0; 3];", "const A: [u8; 3] = [0, 0];");
    differ("const S: &str = \"ab\";", "const S: &str = \"a b\";");
    differ("const C: u8 = 7 as u8;", "const C: u8 = 7 as i8;");
}

#[test]
fn a_cast_is_closed_whatever_type_it_names() {
    // The type after `as` is compared as a type, not required to be a literal.
    agree("const C: u64 = 7 as u64;", "const C: u64 = 7 as u64;");
    differ(
        "const C: u64 = 7 as u64;",
        "const C: u64 = 7 as u32 as u64;",
    );
}

#[test]
fn an_unnamed_const_is_not_a_definition() {
    let found = consts("const _: u32 = 1;", "const _: u32 = 1;");
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn test_constants_are_exempt() {
    let found = findings(&[
        at_root("test-a", "const X: u32 = 1;"),
        at_root("test-b", "#[cfg(test)] mod t { const X: u32 = 1; }"),
    ]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn the_exempt_pairs_hold_for_constants_too() {
    let found = findings(&[
        at_root("test-signal", "const X: u32 = 1;"),
        at_root("test-macro-util", "const X: u32 = 1;"),
    ]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_long_value_is_cut_in_the_message_and_still_compared_whole() {
    let long = format!(
        "const L: [u8; 30] = [{}];",
        (0 .. 30)
            .map(|n| format!("{n}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let found = consts(&long, &long);
    assert_eq!(found.len(), 1);
    assert!(found[0].message.contains("..."), "{}", found[0].message);
    let one_off = long.replace("29]", "28]");
    assert!(consts(&long, &one_off).is_empty());
}

#[test]
fn constants_and_types_do_not_group_together() {
    let found =
        findings(&[at_root("test-a", "struct Dup;"), at_root("test-b", "const Dup: u32 = 1;")]);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_suffix_inside_an_expression_decides_the_value() {
    // `!0u8 as u32` is 255 and `!0u16 as u32` is 65535; `0.1f32 as f64` is
    // 0.10000000149011612 and `0.1f64 as f64` is 0.1. Neither pair is one value.
    differ(
        "const M: u32 = !0u8 as u32;",
        "const M: u32 = !0u16 as u32;",
    );
    differ(
        "const F: f64 = 0.1f32 as f64;",
        "const F: f64 = 0.1f64 as f64;",
    );
    differ(
        "const A: [u8; 1] = [1u8];",
        "const A: [u8; 1] = [1u16 as u8];",
    );
    differ(
        "const T: (u8, u8) = (1u8, 2);",
        "const T: (u8, u8) = (1, 2u8);",
    );
    differ(
        "const N: i32 = -1i8 as i32;",
        "const N: i32 = -1i16 as i32;",
    );
    // Spelled the same, with the rest of the literal canonicalised, they agree.
    agree(
        "const M: u32 = !0u8 as u32;",
        "const M: u32 = !0_u8 as u32;",
    );
    agree(
        "const M: u32 = !0x0u8 as u32;",
        "const M: u32 = !0u8 as u32;",
    );
    agree(
        "const F: f64 = 0.1f32 as f64;",
        "const F: f64 = 0.10f32 as f64;",
    );
}
