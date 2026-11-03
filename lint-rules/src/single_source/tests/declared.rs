//! What `declared_types` hands a tool: the declaration split into its generics and its members, in
//! the canonical form `single-source` compares by, so a copy under another name has the same parts.

use std::path::PathBuf;

use crate::{CrateSourceFile, DeclaredType, declared_types};

fn read(source: &str) -> Vec<DeclaredType> {
    let file = CrateSourceFile {
        rel_path: PathBuf::from("src/lib.rs"),
        text:     source.to_string(),
    };
    declared_types("test-a", &[file])
}

fn only(source: &str) -> DeclaredType {
    let mut found = read(source);
    assert_eq!(found.len(), 1, "`{source}` should declare one type: {found:?}");
    found.remove(0)
}

fn members(ty: &DeclaredType) -> Vec<(String, String)> {
    ty.parts
        .members
        .iter()
        .map(|m| (m.name.clone(), m.body.clone()))
        .collect()
}

#[test]
fn a_struct_is_split_into_its_named_fields() {
    let ty = only("pub struct Frame { pub index: u64, pub at: u32 }");
    assert_eq!(ty.name, "Frame");
    assert_eq!(ty.kind, "struct");
    assert_eq!(ty.line, 1);
    assert!(ty.fully_pub());
    assert_eq!(
        members(&ty),
        [("index".into(), "u64".into()), ("at".into(), "u32".into())]
    );
}

#[test]
fn a_copy_under_another_name_has_the_same_parts_and_another_shape() {
    let a = only("pub struct Frame { pub index: u64, pub at: u32 }");
    let b = only("pub struct Tick { pub index: u64, pub at: u32 }");
    assert_eq!(a.parts, b.parts);
    assert_ne!(a.shape, b.shape, "the name is in the shape single-source groups by");
}

#[test]
fn a_tuple_field_is_named_by_its_place() {
    let ty = only("pub struct Pair(pub u8, pub u16);");
    assert_eq!(members(&ty), [("0".into(), "u8".into()), ("1".into(), "u16".into())]);
}

#[test]
fn a_variant_carries_its_payload_as_its_body() {
    let ty = only("pub enum Run { Pause, Step(u32), Seek { to: u64 } }");
    let got = members(&ty);
    assert_eq!(got.len(), 3);
    assert_eq!(got[0], ("Pause".to_string(), String::new()));
    assert_eq!(got[1].0, "Step");
    assert!(got[1].1.contains("u32"), "{got:?}");
    assert_eq!(got[2].0, "Seek");
    assert!(got[2].1.contains("to") && got[2].1.contains("u64"), "{got:?}");
}

#[test]
fn a_unit_struct_has_no_members() {
    assert!(only("pub struct Marker;").parts.members.is_empty());
}

#[test]
fn a_trait_item_is_named_by_its_name() {
    let ty = only("pub trait Source { fn read(&self) -> u8; fn size(&self) -> u8; }");
    let names: Vec<String> = ty.parts.members.iter().map(|m| m.name.clone()).collect();
    assert_eq!(names, ["read", "size"]);
}

#[test]
fn generics_are_compared_by_place_so_a_renamed_parameter_agrees() {
    let a = only("pub struct Slot<T: Copy> { pub value: T }");
    let b = only("pub struct Slot<U: Copy> { pub value: U }");
    assert_eq!(a.parts, b.parts);
    let c = only("pub struct Slot<T> { pub value: T }");
    assert_ne!(a.parts.generics, c.parts.generics, "a dropped bound is another declaration");
}

#[test]
fn a_type_in_test_code_is_not_declared() {
    let found = read("#[cfg(test)]\nmod tests { pub struct Hidden { pub a: u8 } }\npub struct Seen { pub a: u8 }");
    let names: Vec<&str> = found.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Seen"]);
}
