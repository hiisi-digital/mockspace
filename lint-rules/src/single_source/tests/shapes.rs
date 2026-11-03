//! A type is one concept defined twice when its name and its declaration agree.
//!
//! The same bare name over two different declarations is two concepts that share a word, and a
//! crate-qualified path already tells them apart. Each test holds both directions: the pair that
//! must be one finding, and the pair that must be none, each in both orders, so a lint that
//! flagged every shared name and a lint that flagged nothing would each fail one of them.

use super::*;

fn types(a: &str, b: &str) -> Vec<LintError> {
    findings(&[at_root("test-a", a), with_files("test-b", "mod m;", &[("src/m.rs", b)])])
}

/// The two declarations are one concept: one finding, whichever crate is read first.
fn agree(a: &str, b: &str) {
    let found = types(a, b);
    assert_eq!(
        found.len(),
        1,
        "`{a}` against `{b}` should be one concept: {found:?}"
    );
    assert_eq!(found[0].finding_kind, Some("type"), "`{a}` against `{b}`");
    let back = types(b, a);
    assert_eq!(
        back.len(),
        1,
        "`{b}` against `{a}` should be one concept: {back:?}"
    );
}

/// The two declarations share a name and nothing else: no finding, in either order.
fn differ(a: &str, b: &str) {
    let found = types(a, b);
    assert!(
        found.is_empty(),
        "`{a}` against `{b}` should be two concepts: {found:?}"
    );
    let back = types(b, a);
    assert!(
        back.is_empty(),
        "`{b}` against `{a}` should be two concepts: {back:?}"
    );
}

#[test]
fn a_declaration_copied_into_a_second_crate_is_one_concept_defined_twice() {
    // Mutation: the shape left out of the grouping, so a copy is compared as nothing.
    agree(
        "pub struct Frame { pub index: u64 }",
        "pub struct Frame { pub index: u64 }",
    );
    agree("pub struct Id(u32);", "pub struct Id(u32);");
    agree("pub struct Marker;", "pub struct Marker;");
    agree(
        "pub enum Run { Pause, Resume, Step }",
        "pub enum Run { Pause, Resume, Step }",
    );
    agree(
        "pub trait Source { fn read(&self) -> u8; }",
        "pub trait Source { fn read(&self) -> u8; }",
    );
    agree(
        "pub union Bits { a: u8, b: i8 }",
        "pub union Bits { a: u8, b: i8 }",
    );
    agree(
        "pub struct Slot<T: Copy> { v: T }",
        "pub struct Slot<T: Copy> { v: T }",
    );
}

#[test]
fn two_concepts_sharing_a_bare_name_are_not_one_finding() {
    // The measured cases: kaski's `Frame` is a per-frame driver in one crate, what a camera frames in
    // a second, a published simulation frame in a third and a counter in a fourth.
    // Mutation: the shape ignored, which is the lint as it was and finds each of these.
    differ(
        "pub struct Frame { pub sequence: u64, pub time: u64 }",
        "pub struct Frame { pub bounds: u32, pub target: u32 }",
    );
    differ(
        "pub struct Frame { pub sequence: u64 }",
        "pub struct Frame(u64);",
    );
    differ("pub struct Frame(u64);", "pub struct Frame(u32);");
    differ("pub struct Frame;", "pub struct Frame { pub index: u64 }");
    differ(
        "pub enum Run { Pause, Resume, Step }",
        "pub struct Run { pub id: u32 }",
    );
    differ(
        "pub enum Run { Pause, Resume, Step }",
        "pub enum Run { Pause, Resume }",
    );
    differ(
        "pub enum Run { Pause, Resume }",
        "pub enum Run { Pause, Play }",
    );
    differ("pub enum Run { A(u8) }", "pub enum Run { A(u16) }");
    differ(
        "pub enum Run { A { x: u8 } }",
        "pub enum Run { A { y: u8 } }",
    );
    differ("pub enum Run { A = 1 }", "pub enum Run { A = 2 }");
    differ(
        "pub trait Surface { fn size(&self) -> u32; }",
        "pub trait Surface { fn size(&self) -> u32; fn show(&self); }",
    );
    differ(
        "pub trait Surface { fn size(&self) -> u32; }",
        "pub trait Surface { fn size(&self) -> u64; }",
    );
    differ("pub trait Surface {}", "pub trait Surface: Copy {}");
    differ(
        "pub trait Surface { type Out; }",
        "pub trait Surface { const OUT: u8; }",
    );
    differ(
        "pub union Bits { a: u8, b: i8 }",
        "pub union Bits { a: u8, b: u16 }",
    );
}

#[test]
fn a_different_kind_under_one_name_is_two_concepts() {
    // Mutation: the keyword left out of the shape, so a struct and an enum with the same body agree.
    differ("pub struct Class { a: u8 }", "pub union Class { a: u8 }");
    differ("pub struct Plan;", "pub trait Plan {}");
    differ("pub enum Lane {}", "pub struct Lane {}");
}

#[test]
fn generics_and_bounds_are_part_of_the_shape() {
    // Mutation: the type parameters and the where clause left out of the shape.
    differ("pub struct Slot<T> { v: T }", "pub struct Slot { v: u8 }");
    differ(
        "pub struct Slot<T> { v: T }",
        "pub struct Slot<T: Copy> { v: T }",
    );
    differ("pub struct Slot<T> { v: T }", "pub struct Slot<U> { v: U }");
    differ(
        "pub struct Slot<T> where T: Copy { v: T }",
        "pub struct Slot<T> { v: T }",
    );
    differ("pub trait Source<T> {}", "pub trait Source {}");
}

#[test]
fn what_does_not_change_the_concept_does_not_change_the_shape() {
    // The same declaration spelled another way is still one concept, so a copy with a doc comment, a
    // derive, another layout or other field visibility is found. Each pair is a finding in both orders.
    // Mutation: comments, attributes or visibility kept in the shape.
    agree(
        "pub struct Frame { pub index: u64, pub time: u64 }",
        "/// The per-frame driver.\n#[derive(Clone, Debug)]\npub struct Frame {\n    /// Which.\n    index: u64,\n    pub(crate) time: u64,\n}",
    );
    agree(
        "pub struct Frame { index: u64 }",
        "pub struct Frame { index: u64, }",
    );
    agree("pub struct Id(pub u32);", "pub struct Id(u32);");
    agree(
        "pub enum Run { /** go */ Pause, Resume }",
        "pub enum Run {\n    #[doc = \"x\"]\n    Pause,\n    Resume,\n}",
    );
    agree(
        "pub struct Frame { index: u64 } // here",
        "pub struct Frame { index: u64 }",
    );
    // A comma that only closes a list: tuple fields, generics and a where clause.
    // Mutation: the trailing comma kept in the shape.
    agree("pub struct Pair(u8, u16);", "pub struct Pair(u8, u16,);");
    agree(
        "pub struct Wrap<T, U>(T, U);",
        "pub struct Wrap<T, U,>(T, U);",
    );
    agree(
        "pub struct Held<T>(T) where T: Copy;",
        "pub struct Held<T>(T) where T: Copy,;",
    );
}

#[test]
fn members_that_have_no_order_agree_in_any_order_and_members_that_have_one_do_not() {
    // Named fields, variants and trait items are a set; a tuple struct's fields are positions.
    // Mutation: the members compared in source order; the tuple fields sorted.
    agree(
        "pub struct Frame { a: u8, b: u16 }",
        "pub struct Frame { b: u16, a: u8 }",
    );
    agree("pub enum Run { A, B, C }", "pub enum Run { C, A, B }");
    agree(
        "pub trait Surface { fn size(&self) -> u32; fn show(&self); }",
        "pub trait Surface { fn show(&self); fn size(&self) -> u32; }",
    );
    agree(
        "pub union Bits { a: u8, b: i8 }",
        "pub union Bits { b: i8, a: u8 }",
    );
    differ("pub struct Pair(u8, u16);", "pub struct Pair(u16, u8);");
}

#[test]
fn a_default_body_in_a_trait_is_not_part_of_what_the_trait_declares() {
    // The signature is the concept; a body is how one crate happens to implement it.
    // Mutation: the body kept in the shape.
    agree(
        "pub trait Source { fn read(&self) -> u8 { 0 } }",
        "pub trait Source { fn read(&self) -> u8 { 1 } }",
    );
    agree(
        "pub trait Source { fn read(&self) -> u8 { 0 } }",
        "pub trait Source { fn read(&self) -> u8; }",
    );
}

#[test]
fn of_three_crates_the_ones_that_agree_with_the_first_are_the_findings() {
    // The second crate's `Frame` is another concept and is neither a finding nor the one the third is
    // compared with. Mutation: the group is the name alone, so the second crate is reported too.
    let found = findings(&[
        at_root("test-a", "pub struct Frame { index: u64 }"),
        at_root("test-b", "pub struct Frame { bounds: u32 }"),
        at_root("test-c", "pub struct Frame { index: u64 }"),
    ]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].crate_name, "test-c");
    assert!(found[0].message.contains("test-a"), "{}", found[0].message);

    // Two groups of one name, each with a copy: one finding for each.
    let found = findings(&[
        at_root("test-a", "pub struct Frame { index: u64 }"),
        at_root("test-b", "pub struct Frame { bounds: u32 }"),
        at_root("test-c", "pub struct Frame { index: u64 }"),
        at_root("test-d", "pub struct Frame { bounds: u32 }"),
    ]);
    let mut crates: Vec<&str> = found.iter().map(|e| e.crate_name.as_str()).collect();
    crates.sort();
    assert_eq!(crates, ["test-c", "test-d"]);
    assert!(
        found
            .iter()
            .any(|e| e.crate_name == "test-c" && e.message.contains("test-a"))
    );
    assert!(
        found
            .iter()
            .any(|e| e.crate_name == "test-d" && e.message.contains("test-b"))
    );
}

#[test]
fn the_message_says_it_is_the_same_shape() {
    // The finding tells the author what was compared, so a copy that drifted is not a surprise.
    let found = types(
        "pub struct Frame { index: u64 }",
        "pub struct Frame { index: u64 }",
    );
    assert!(
        found[0].message.contains("same shape"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_copy_that_gained_a_field_is_another_concept_here() {
    // Pinned, not endorsed: the lint compares declarations whole, so a copy that drifted by one field
    // is not found. Whether it is one concept is not settled by anything the lint reads, and a
    // threshold on how much may differ would be a number nobody justified.
    differ(
        "pub struct Config { pub a: u8, pub b: u8 }",
        "pub struct Config { pub a: u8, pub b: u8, pub c: u8 }",
    );
}
