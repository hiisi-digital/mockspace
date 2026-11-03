//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A declaration split into the pieces a tool compares, each in the canonical form of the parent
//! module, so a copy under another name has the same parts.

use tree_sitter::Node;

use super::{
    Class, GENERIC_PARAMETERS, Rename, Shape, UNORDERED_LISTS, is_closing_comma, is_noise, supertraits,
    variants_are_numbered_by_place,
};

/// One member of a declaration, split so that two declarations can be compared by what their members
/// are and by what they are called apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// What the member is called: a field, a variant or a trait item. A tuple field is its position.
    pub name: String,
    /// What the member declares, in the canonical form: a field's type, a variant's payload, a trait
    /// item's whole signature. Empty for a variant that carries nothing.
    pub body: String,
}

impl Member {
    /// The member as one comparable string, name and body together.
    pub fn whole(&self) -> String {
        format!("{} {}", self.name, self.body).trim_end().to_string()
    }
}

/// A declaration split into what the name of the type does not enter: its generics and its members,
/// each in the canonical form of [`canonical_declaration`], so a copy under another name has the same
/// parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    /// The generic parameters and their bounds, merged as in the canonical declaration. Empty when the
    /// type has none.
    pub generics: String,
    pub members:  Vec<Member>,
    /// What the canonical declaration keeps besides the name, the generics and the members: the
    /// keyword, the kind of body (unit, tuple or braced) and the order of the
    /// variants of an enum numbered by place.
    pub frame:    String,
}

/// The generics and the members of a struct, enum, trait or union in canonical form.
pub(in super::super) fn canonical_parts(node: Node, source: &str) -> Parts {
    let mut shape = Shape {
        source,
        renames: Vec::new(),
        depth: 0,
    };
    shape.parts(node)
}

impl<'s> Shape<'s> {
    fn parts(&mut self, node: Node<'s>) -> Parts {
        // The type's own name is not part of what it declares, so a member that names it (a link to the
        // next node) reads the same under whatever the type is called.
        let source: &'s str = self.source;
        if let Some(name) = node.child_by_field_name("name") {
            self.renames.push(Rename {
                class: Class::Type,
                name:  &source[name.byte_range()],
                to:    "$self".to_string(),
            });
        }
        let generics = self.open_generics(node);
        let mut members = Vec::new();
        let body = node.child_by_field_name("body");
        if let Some(body) = body {
            let mut cursor = body.walk();
            let mut place = 0usize;
            for member in body.named_children(&mut cursor).filter(|m| !is_noise(*m)) {
                members.push(self.member(member, body.kind(), &mut place));
            }
            let sorted = UNORDERED_LISTS.contains(&body.kind())
                || (body.kind() == "enum_variant_list" && !variants_are_numbered_by_place(node, self.source));
            if sorted {
                members.sort_by_key(Member::whole);
            }
        }
        let frame = self.frame(node, body, &members);
        self.renames.clear();
        self.depth -= usize::from(generics.is_some());
        Parts {
            generics: generics.unwrap_or_default(),
            members,
            frame,
        }
    }

    /// Everything the canonical declaration keeps that is neither the name, the generics nor a
    /// member: the keyword, which kind of body there is, and the order of the
    /// variants where the order is part of the declaration.
    fn frame(&mut self, node: Node<'s>, body: Option<Node<'s>>, members: &[Member]) -> String {
        let name = node.child_by_field_name("name");
        let mut parts: Vec<String> = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if is_noise(child) || is_closing_comma(child) || Some(child) == name {
                continue;
            }
            let in_generics = child.kind() == GENERIC_PARAMETERS
                || child.kind() == "where_clause"
                || Some(child) == supertraits(node);
            if in_generics {
                continue;
            }
            if Some(child) == body {
                parts.push(child.kind().to_string());
                if child.kind() == "enum_variant_list" && variants_are_numbered_by_place(node, self.source) {
                    let order: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
                    parts.push(format!("in order {}", order.join(",")));
                }
                continue;
            }
            parts.push(self.node(child));
        }
        parts.join(" ")
    }

    fn member(&mut self, member: Node<'s>, list: &str, place: &mut usize) -> Member {
        let source = self.source;
        let text = |name: Option<Node>| name.map(|n| source[n.byte_range()].to_string());
        match member.kind() {
            "field_declaration" => {
                let name = text(member.child_by_field_name("name")).unwrap_or_default();
                let body = member
                    .child_by_field_name("type")
                    .map(|ty| self.node(ty))
                    .unwrap_or_default();
                Member { name, body }
            },
            "enum_variant" => {
                let name = text(member.child_by_field_name("name")).unwrap_or_default();
                let mut body = member
                    .child_by_field_name("body")
                    .map(|payload| self.node(payload))
                    .unwrap_or_default();
                if let Some(value) = member.child_by_field_name("value") {
                    body = format!("{body} = {}", self.node(value));
                }
                Member { name, body }
            },
            _ if list == "ordered_field_declaration_list" => {
                let name = place.to_string();
                *place += 1;
                Member { name, body: self.node(member) }
            },
            _ => {
                let name = text(member.child_by_field_name("name")).unwrap_or_default();
                Member { name, body: self.node(member) }
            },
        }
    }
}
