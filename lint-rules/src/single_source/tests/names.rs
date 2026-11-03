//! A name that declares an item is part of the declaration, whatever a generic parameter is called.
//!
//! The parameters of a type are read by their place, so `<T>` and `<U>` agree. A variant, a method,
//! an associated constant or an associated type that happens to be spelled like a parameter is the
//! item's own name and must be read as written, or two declarations with different members agree
//! and two with the same members differ.

use super::shapes::{agree, differ};

#[test]
fn a_variant_spelled_like_a_parameter_keeps_its_name() {
    // Mutation: a name that declares an item renamed when it spells a parameter.
    differ(
        "pub enum E<A, B> { A(A), B(B) }",
        "pub enum E<L, R> { L(L), R(R) }",
    );
    agree(
        "pub enum E<A, B> { A(A), B(B) }",
        "pub enum E<X, Y> { A(X), B(Y) }",
    );
}

#[test]
fn a_method_spelled_like_a_const_parameter_keeps_its_name() {
    agree(
        "pub trait Q<const N: usize> { fn N(&self) -> [u8; N]; }",
        "pub trait Q<const M: usize> { fn N(&self) -> [u8; M]; }",
    );
    differ(
        "pub trait Q<const N: usize> { fn N(&self) -> [u8; N]; }",
        "pub trait Q<const N: usize> { fn M(&self) -> [u8; N]; }",
    );
}

#[test]
fn an_associated_constant_or_type_spelled_like_a_parameter_keeps_its_name() {
    agree("pub trait Q<N> { const N: u8; }", "pub trait Q<M> { const N: u8; }");
    differ("pub trait Q<N> { const N: u8; }", "pub trait Q<N> { const M: u8; }");
    agree("pub trait Q<T> { type T; }", "pub trait Q<U> { type T; }");
    differ("pub trait Q<T> { type T; }", "pub trait Q<T> { type U; }");
}

#[test]
fn the_parameter_itself_and_its_uses_are_still_read_by_place() {
    agree("pub struct S<T> { a: T }", "pub struct S<U> { a: U }");
    agree("pub trait Q<T> { type T; fn f(&self) -> T; }", "pub trait Q<U> { type T; fn f(&self) -> U; }");
    differ("pub trait Q<T> { type T; fn f(&self) -> T; }", "pub trait Q<U> { type T; fn f(&self) -> Self::T; }");
}
