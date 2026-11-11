//! The ids an instance hands to git are object ids and nothing else.

use mockspace_store::{Base, Oid};

// Mutation: let `Oid::parse` take any text, as the string ids did; the marker of a base of no
// commit, an option or a path then reaches git as a name.
#[test]
fn only_the_hex_of_an_object_is_an_oid() {
    for text in ["-", "--all", "HEAD", "refs/mock/research", "", "4b825dc642cb6eb9a060e54bf8d69288fbee490", "4B825DC642CB6EB9A060E54BF8D69288FBEE4904", "4b825dc642cb6eb9a060e54bf8d69288fbee4904+x"] {
        assert!(Oid::parse(text).is_err(), "{text:?} was taken for an object id");
    }
    let sha1 = Oid::parse("4b825dc642cb6eb9a060e54bf8d69288fbee4904").unwrap();
    assert_eq!(sha1.as_str(), "4b825dc642cb6eb9a060e54bf8d69288fbee4904");
    assert!(Oid::parse(&"a".repeat(64)).is_ok(), "a sha-256 id is one");
}

// Mutation: give the base of an instance of a ref not born yet a commit; anything that reads the
// commit of a base then has one to hand to git.
#[test]
fn the_base_of_a_ref_not_born_yet_has_no_commit() {
    assert_eq!(Base::Unborn.commit(), None);
    assert!(Base::Unborn.is_unborn());
    let at = Base::At(Oid::parse(&"0".repeat(40)).unwrap());
    assert_eq!(at.commit().map(Oid::as_str), Some("0".repeat(40).as_str()));
    assert!(!at.is_unborn());
}
