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
    differ(
        "pub struct Slot<T> where T: Copy { v: T }",
        "pub struct Slot<T> { v: T }",
    );
    differ("pub trait Source<T> {}", "pub trait Source {}");
    differ(
        "pub struct Slot<T, U> { a: T, b: U }",
        "pub struct Slot<T, U> { a: U, b: T }",
    );
}

#[test]
fn a_bound_has_one_form_wherever_it_is_written() {
    // Mutation: the where clause kept apart from the inline bounds, or its predicates not merged.
    agree(
        "pub struct Slot<T: Copy> { v: T }",
        "pub struct Slot<T> where T: Copy { v: T }",
    );
    agree(
        "pub struct S<T: Copy>(T);",
        "pub struct S<T>(T) where T: Copy;",
    );
    agree(
        "pub struct Slot<T> where T: Copy + Send { v: T }",
        "pub struct Slot<T> where T: Copy, T: Send { v: T }",
    );
    agree(
        "pub struct Slot<T: Copy, U: Send> { a: T, b: U }",
        "pub struct Slot<T, U> where U: Send, T: Copy { a: T, b: U }",
    );
    agree(
        "pub struct Slot<T: Send> where T: Copy { v: T }",
        "pub struct Slot<T: Copy> where T: Send { v: T }",
    );
    agree(
        "pub struct Slot<T> where T: Send, T: Copy { v: T }",
        "pub struct Slot<T> where T: Copy, T: Send { v: T }",
    );
    agree(
        "pub struct Slot<T: Copy> where T: Copy { v: T }",
        "pub struct Slot<T: Copy> { v: T }",
    );
    differ(
        "pub struct Slot<T: Copy, U> { a: T, b: U }",
        "pub struct Slot<T, U: Copy> { a: T, b: U }",
    );
}

#[test]
fn a_list_of_bounds_is_a_set() {
    // Mutation: the bounds kept in source order.
    agree(
        "pub struct S<T: Copy + Send>(T);",
        "pub struct S<T: Send + Copy>(T);",
    );
    agree(
        "pub struct S<T> where T: Copy + Send + 'static { v: T }",
        "pub struct S<T> where T: 'static + Send + Copy { v: T }",
    );
    agree("pub trait T: Copy + Send {}", "pub trait T: Send + Copy {}");
    agree(
        "pub trait T { type O: Copy + Send; }",
        "pub trait T { type O: Send + Copy; }",
    );
    agree(
        "pub trait T { fn f<A: Copy + Send>(a: A); }",
        "pub trait T { fn f<A: Send + Copy>(a: A); }",
    );
    differ("pub trait T: Copy {}", "pub trait T: Send {}");
    differ(
        "pub trait T { type O: Copy; }",
        "pub trait T { type O: Copy + Send; }",
    );
}

#[test]
fn a_generic_parameter_is_named_by_its_position() {
    // Mutation: the parameters compared by the names they were written under.
    agree("pub struct S<T> { v: T }", "pub struct S<U> { v: U }");
    agree(
        "pub struct S<T: Copy> { v: Vec<T> }",
        "pub struct S<U: Copy> { v: Vec<U> }",
    );
    agree("pub struct R<'a>(&'a u8);", "pub struct R<'b>(&'b u8);");
    agree(
        "pub struct R<'a, T: 'a>(&'a T);",
        "pub struct R<'b, U: 'b>(&'b U);",
    );
    agree(
        "pub struct A<const N: usize> { v: [u8; N] }",
        "pub struct A<const M: usize> { v: [u8; M] }",
    );
    agree(
        "pub trait Q<T> { fn f<U>(a: T, b: U); fn g<U>(a: U); }",
        "pub trait Q<V> { fn g<W>(a: W); fn f<W>(a: V, b: W); }",
    );
    agree(
        "pub struct S<T: Iterator> { v: T::Item }",
        "pub struct S<U: Iterator> { v: U::Item }",
    );
    differ(
        "pub trait Q<T> { fn f<U>(a: T, b: U); }",
        "pub trait Q<T> { fn f<U>(a: U, b: T); }",
    );
    differ(
        "pub struct S<T, U> { a: T, b: U }",
        "pub struct S<T, U> { a: U, b: T }",
    );
    differ(
        "pub struct R<'a, 'b>(&'a u8, &'b u8);",
        "pub struct R<'a, 'b>(&'b u8, &'a u8);",
    );
    differ("pub struct S<T> { v: T }", "pub struct S<T> { v: u8 }");
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
    agree(
        "pub trait T { fn f(&self, a: u8); }",
        "pub trait T { fn f(&self, a: u8,); }",
    );
}

#[test]
fn a_comment_after_the_last_comma_does_not_make_it_another_comma() {
    // The comma still only closes the list when a comment sits between it and the closer.
    // Mutation: the next sibling read as it is, comment or not.
    agree(
        "pub struct P(u8, u16);",
        "pub struct P(u8, u16, // last\n);",
    );
    agree(
        "pub enum E { A(u8, u16) }",
        "pub enum E { A(u8, u16, /* x */) }",
    );
    agree(
        "pub struct W<T>(T) where T: Copy;",
        "pub struct W<T>(T) where T: Copy, /* c */;",
    );
    differ(
        "pub struct P(u8, u16);",
        "pub struct P(u8, /* x */ u16, u8);",
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
    agree(
        "pub enum Shape { Dot, Line(u8), Box { w: u8 } }",
        "pub enum Shape { Box { w: u8 }, Dot, Line(u8) }",
    );
    agree(
        "pub enum Code { A = 1, B = 2 }",
        "pub enum Code { B = 2, A = 1 }",
    );
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
}

#[test]
fn a_required_method_and_a_provided_one_are_two_declarations() {
    // What an implementor must write and what it may leave out are not the same trait.
    // Mutation: the body and the missing body both left as nothing.
    differ(
        "pub trait Source { fn read(&self) -> u8 { 0 } }",
        "pub trait Source { fn read(&self) -> u8; }",
    );
    differ(
        "pub trait T { fn f(&self); fn g(&self) {} }",
        "pub trait T { fn f(&self) {} fn g(&self); }",
    );
}

#[test]
fn an_enum_whose_discriminants_follow_its_order_keeps_the_order() {
    // The variants of a fieldless enum, or of one with a `repr`, are numbered by their place unless
    // every one is numbered by hand, so reordering them makes another enum.
    // Mutation: the variants sorted whatever the enum is.
    differ(
        "#[repr(u8)] pub enum E { A, B }",
        "#[repr(u8)] pub enum E { B, A }",
    );
    differ("pub enum Run { A, B, C }", "pub enum Run { C, A, B }");
    differ("pub enum E { A = 1, B, C }", "pub enum E { B, C, A = 1 }");
    differ(
        "#[repr(u8)] pub enum E { A(u8), B }",
        "#[repr(u8)] pub enum E { B, A(u8) }",
    );
    agree(
        "#[repr(u8)] pub enum E { A = 1, B = 2 }",
        "#[repr(u8)] pub enum E { B = 2, A = 1 }",
    );
}

#[test]
fn paths_are_compared_as_written() {
    // Pinned, not endorsed: one type spelled through two paths is two declarations here, so a copy
    // that spells `u8` as `core::primitive::u8` is not found.
    differ(
        "pub struct S { v: u8 }",
        "pub struct S { v: core::primitive::u8 }",
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

#[test]
fn the_operands_of_a_bounded_type_are_a_set() {
    // `dyn A + B` and `impl A + B` bound a type by several traits, in no order.
    // Mutation: the operands kept in the order written; a nested `+` not flattened.
    agree(
        "pub struct S { v: Box<dyn Send + Sync> }",
        "pub struct S { v: Box<dyn Sync + Send> }",
    );
    agree(
        "pub trait T { fn f(&self) -> impl Copy + Send; }",
        "pub trait T { fn f(&self) -> impl Send + Copy; }",
    );
    agree(
        "pub struct S { v: Box<dyn A + B + C> }",
        "pub struct S { v: Box<dyn C + A + B> }",
    );
    agree(
        "pub struct S { v: Box<dyn Send + 'static> }",
        "pub struct S { v: Box<dyn 'static + Send> }",
    );
    differ(
        "pub struct S { v: Box<dyn Send + Sync> }",
        "pub struct S { v: Box<dyn Send + Copy> }",
    );
    differ(
        "pub trait T { fn f(&self) -> impl Send + Sync; }",
        "pub trait T { fn f(&self) -> dyn Send + Sync; }",
    );
}

#[test]
fn only_the_first_segment_of_a_path_can_name_a_generic_parameter() {
    // The last segment of a path names an item of that path, whatever a parameter is called.
    // Mutation: every `type_identifier` renamed, the last segment of a path too.
    differ(
        "pub struct S<T> { a: T, b: m::T }",
        "pub struct S<U> { a: U, b: m::U }",
    );
    agree(
        "pub struct S<Error> { a: Error, b: std::io::Error }",
        "pub struct S<E> { a: E, b: std::io::Error }",
    );
    agree(
        "pub struct S<T: Tr> { a: T::Item }",
        "pub struct S<U: Tr> { a: U::Item }",
    );
    differ(
        "pub struct S<T: Tr> { a: T::Item }",
        "pub struct S<T: Tr> { a: T::Other }",
    );
}

#[test]
fn a_lifetime_bound_in_a_where_clause_is_read_by_place() {
    // `for<'x> F: Fn(&'x u8)` binds `'x` over the predicate's bounds, as `F: for<'x> Fn(&'x u8)` does.
    // Mutation: the binder closed before the bounds are read.
    agree(
        "pub struct S<F> where for<'x> F: Fn(&'x u8) { f: F }",
        "pub struct S<F> where for<'y> F: Fn(&'y u8) { f: F }",
    );
    agree(
        "pub struct S<F: for<'x> Fn(&'x u8)> { f: F }",
        "pub struct S<F: for<'y> Fn(&'y u8)> { f: F }",
    );
    differ(
        "pub struct S<F> where for<'x> F: Fn(&'x u8) { f: F }",
        "pub struct S<F> where for<'x> F: Fn(&'static u8) { f: F }",
    );
    differ(
        "pub struct S<T> where Vec<T>: Clone { v: T }",
        "pub struct S<T> where Vec<u8>: Clone { v: T }",
    );
}

#[test]
fn a_tuple_of_one_keeps_its_comma() {
    // `(u8,)` is a tuple and `(u8)` is not, so the comma does not only close the list.
    // Mutation: a comma dropped before the closer wherever it sits.
    differ("pub struct S { x: (u8,) }", "pub struct S { x: (u8) }");
    differ(
        "pub struct S { x: (u8, /* c */) }",
        "pub struct S { x: (u8) }",
    );
    agree(
        "pub struct S { x: (u8, u16) }",
        "pub struct S { x: (u8, u16,) }",
    );
    agree("pub struct S(u8,);", "pub struct S(u8);");
}

#[test]
fn the_name_of_a_parameter_of_a_trait_method_is_not_part_of_the_trait() {
    // Mutation: the pattern of a parameter kept in the shape.
    agree(
        "pub trait Source { fn read(&self, buf: u8) -> u8; }",
        "pub trait Source { fn read(&self, out: u8) -> u8; }",
    );
    agree(
        "pub trait Source { fn read(&self, mut buf: u8) -> u8; }",
        "pub trait Source { fn read(&self, out: u8) -> u8; }",
    );
    differ(
        "pub trait Source { fn read(&self, buf: u8) -> u8; }",
        "pub trait Source { fn read(&self, buf: u16) -> u8; }",
    );
    differ(
        "pub trait Source { fn read(&self, buf: u8) -> u8; }",
        "pub trait Source { fn read(&self) -> u8; }",
    );
}

#[test]
fn a_supertrait_is_a_bound_on_self() {
    // Mutation: the supertraits read as a list of their own and not merged with `where Self:`.
    agree("pub trait T: Copy {}", "pub trait T where Self: Copy {}");
    agree(
        "pub trait T<X>: Copy + Send where X: Send {}",
        "pub trait T<X> where Self: Send + Copy, X: Send {}",
    );
    differ("pub trait T: Copy {}", "pub trait T: Clone {}");
    differ("pub trait T: Copy {}", "pub trait T {}");
}

#[test]
fn only_the_repr_attribute_numbers_the_variants() {
    // An attribute that mentions the word is no `repr`, so the variants stay a set.
    // Mutation: the attribute's text searched for `repr`.
    agree(
        "#[doc = \"repr\"] pub enum E { A(u8), B }",
        "#[doc = \"repr\"] pub enum E { B, A(u8) }",
    );
    differ(
        "#[repr(u8)] pub enum E { A(u8), B }",
        "#[repr(u8)] pub enum E { B, A(u8) }",
    );
}

#[test]
fn a_derived_order_is_not_read() {
    // Pinned, not endorsed: `PartialOrd` and `Ord` read the order of the fields and the variants, and
    // the shape holds them as a set, so a copy that reordered them is found. The module doc lists it.
    agree(
        "#[derive(PartialOrd)] pub struct S { a: u8, b: u8 }",
        "#[derive(PartialOrd)] pub struct S { b: u8, a: u8 }",
    );
}
