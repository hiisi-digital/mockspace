//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The canonical form of what a type declares, so two spellings of one declaration compare equal.
//!
//! The form keeps the keyword, the name, the generics with their bounds and the members. It leaves
//! out what no other crate's author would call another concept:
//!
//! - comments, attributes and visibilities, and a comma that only closes a list;
//! - the names of generic parameters, which are replaced by where they sit in their list;
//! - where a bound is written: `<T: Copy>` and `where T: Copy` give one set of bounds for `T`, and
//!   so do two predicates on one parameter;
//! - the order of a list of bounds, which is a set;
//! - the order of the fields of a struct or a union, of the items of a trait, and of the variants of
//!   an enum whose variants are not numbered by their place;
//! - the body of a trait method; a required method ends in a semicolon, which is kept, so it differs
//!   from a provided one, and two provided ones agree whatever their bodies do.
//!
//! What it keeps in order: the fields of a tuple struct, the parameters of a function, the generic
//! parameters, and the variants of an enum that is fieldless or has a `repr` unless every variant
//! is numbered by hand. A path is compared as written, so `u8` and `core::primitive::u8` differ.

use tree_sitter::Node;

/// Lists whose members are a set. The variants of an enum are one only when the enum allows it.
const UNORDERED_LISTS: &[&str] = &["field_declaration_list", "declaration_list"];

/// The field a declaration keeps its generic parameters in.
const GENERIC_PARAMETERS: &str = "type_parameters";

/// Bounds gathered per bounded name, as the canonical form of each.
type Bounds = std::collections::BTreeMap<String, Vec<String>>;

/// What a generic parameter is, because a lifetime and a type of one name are two parameters.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Type,
    Lifetime,
    Const,
}

/// A generic parameter and the name it is compared under.
struct Rename<'s> {
    class: Class,
    name:  &'s str,
    to:    String,
}

/// The canonical form of what a struct, enum, trait or union declares.
pub(super) fn canonical_declaration(node: Node, source: &str) -> String {
    Shape {
        source,
        renames: Vec::new(),
        depth: 0,
    }
    .node(node)
}

struct Shape<'s> {
    source:  &'s str,
    renames: Vec<Rename<'s>>,
    /// How many scopes of generic parameters enclose the node being read.
    depth:   usize,
}

impl<'s> Shape<'s> {
    fn node(&mut self, node: Node<'s>) -> String {
        if is_noise(node) {
            return String::new();
        }
        if node.child_count() == 0 {
            return self.leaf(node);
        }
        if UNORDERED_LISTS.contains(&node.kind()) {
            return self.set(node, true);
        }
        if node.kind() == "trait_bounds" {
            return self.bounds(node);
        }
        self.children(node)
    }

    /// The children of a node in order, with the generics of the node merged into one place.
    fn children(&mut self, node: Node<'s>) -> String {
        let mark = self.renames.len();
        let generics = self.open_generics(node);
        let body = if node.kind() == "function_item" {
            node.child_by_field_name("body")
        } else {
            None
        };
        let mut parts: Vec<String> = Vec::new();
        let mut generics_done = generics.is_none();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if is_noise(child) || is_closing_comma(child) || Some(child) == body {
                continue;
            }
            let at_generics = child.kind() == GENERIC_PARAMETERS || child.kind() == "where_clause";
            if at_generics && generics.is_some() {
                if !generics_done {
                    parts.push(generics.clone().unwrap_or_default());
                    generics_done = true;
                }
                continue;
            }
            if child.kind() == "enum_variant_list" {
                let sorted = !variants_are_numbered_by_place(node, self.source);
                parts.push(self.set(child, sorted));
                continue;
            }
            parts.push(self.node(child));
        }
        self.renames.truncate(mark);
        self.depth -= usize::from(generics.is_some());
        parts.join(" ")
    }

    /// The members of a braced list: a set when `sorted`, the order written otherwise.
    fn set(&mut self, node: Node<'s>, sorted: bool) -> String {
        let mut cursor = node.walk();
        let mut members: Vec<String> = node
            .named_children(&mut cursor)
            .filter(|member| !is_noise(*member))
            .map(|member| self.node(member))
            .filter(|member| !member.is_empty())
            .collect();
        if sorted {
            members.sort();
        }
        format!("{{ {} }}", members.join(" ; "))
    }

    /// A list of bounds, which is a set.
    fn bounds(&mut self, node: Node<'s>) -> String {
        format!(": {}", self.bound_set(node).join(" + "))
    }

    fn bound_set(&mut self, node: Node<'s>) -> Vec<String> {
        let mut cursor = node.walk();
        let mut set: Vec<String> = node
            .named_children(&mut cursor)
            .filter(|bound| !is_noise(*bound))
            .map(|bound| self.node(bound))
            .collect();
        set.sort();
        set
    }

    /// Declares the generic parameters of a node, if it has any, and returns their canonical form
    /// merged with every bound written on them or after them in a where clause.
    fn open_generics(&mut self, node: Node<'s>) -> Option<String> {
        let params = node.child_by_field_name(GENERIC_PARAMETERS);
        let mut cursor = node.walk();
        let clause = node
            .children(&mut cursor)
            .find(|child| child.kind() == "where_clause");
        if params.is_none() && clause.is_none() {
            return None;
        }
        self.depth += 1;
        let mut declared: Vec<Node<'s>> = Vec::new();
        if let Some(params) = params {
            let mut cursor = params.walk();
            declared = params
                .named_children(&mut cursor)
                .filter(|param| !is_noise(*param))
                .collect();
            for (index, param) in declared.iter().enumerate() {
                self.declare(*param, index);
            }
        }
        let mut written: Vec<String> = Vec::new();
        let mut bounds = Bounds::new();
        for param in &declared {
            written.push(self.parameter(*param, &mut bounds));
        }
        if let Some(clause) = clause {
            let mut cursor = clause.walk();
            for predicate in clause.named_children(&mut cursor).filter(|p| !is_noise(*p)) {
                let (Some(left), Some(right)) = (
                    predicate.child_by_field_name("left"),
                    predicate.child_by_field_name("bounds"),
                ) else {
                    continue;
                };
                let left = self.node(left);
                let set = self.bound_set(right);
                bounds.entry(left).or_default().extend(set);
            }
        }
        let params = if params.is_some() {
            format!("< {} >", written.join(" , "))
        } else {
            String::new()
        };
        let predicates: Vec<String> = bounds
            .into_iter()
            .map(|(left, mut set)| {
                set.sort();
                set.dedup();
                format!("{left} : {}", set.join(" + "))
            })
            .collect();
        Some(if predicates.is_empty() {
            params
        } else {
            format!("{params} where {{ {} }}", predicates.join(" ; "))
        })
    }

    /// Gives one generic parameter the name it is compared under: its place among its siblings, under
    /// the depth of the scope that declares it.
    fn declare(&mut self, param: Node<'s>, index: usize) {
        let (class, name) = match param.kind() {
            "type_parameter" => (Class::Type, param.child_by_field_name("name")),
            "lifetime_parameter" => (Class::Lifetime, param.child_by_field_name("name")),
            "const_parameter" => (Class::Const, param.child_by_field_name("name")),
            _ => return,
        };
        let Some(name) = name else { return };
        let source: &'s str = self.source;
        let text = match class {
            Class::Lifetime => lifetime_name(name, source),
            _ => &source[name.byte_range()],
        };
        self.renames.push(Rename {
            class,
            name: text,
            to: format!("${}.{index}", self.depth),
        });
    }

    /// One parameter as written, without its bounds, which go to the set kept for its name.
    fn parameter(&mut self, param: Node<'s>, bounds: &mut Bounds) -> String {
        let mut text = Vec::new();
        let name = param
            .child_by_field_name("name")
            .map(|name| self.node(name));
        if let Some(name) = &name {
            text.push(name.clone());
        }
        if let (Some(name), Some(set)) = (name, param.child_by_field_name("bounds")) {
            let set = self.bound_set(set);
            bounds.entry(name).or_default().extend(set);
        }
        let mut cursor = param.walk();
        for child in param.children(&mut cursor) {
            let field = |f: &str| param.child_by_field_name(f) == Some(child);
            if field("default_type") || field("type") || field("value") {
                let prefix = if field("type") { ": " } else { "= " };
                text.push(format!("{prefix}{}", self.node(child)));
            }
        }
        if param.kind() == "const_parameter" {
            text.insert(0, "const".to_string());
        }
        text.join(" ")
    }

    fn leaf(&self, node: Node<'s>) -> String {
        let text = &self.source[node.byte_range()];
        // A type is named by a `type_identifier`, and as the first segment of a path (`T::Item`) by an
        // `identifier`, where it is told from a constant only by the parameters in scope.
        let in_lifetime = node
            .parent()
            .is_some_and(|parent| parent.kind() == "lifetime");
        let wants = |class: Class| {
            match node.kind() {
                "type_identifier" => class == Class::Type,
                "identifier" if in_lifetime => class == Class::Lifetime,
                "identifier" => class != Class::Lifetime,
                _ => false,
            }
        };
        self.renames
            .iter()
            .rev()
            .find(|rename| wants(rename.class) && rename.name == text)
            .map_or_else(|| text.to_string(), |rename| rename.to.clone())
    }
}

/// The name of a lifetime without its tick.
fn lifetime_name<'s>(node: Node, source: &'s str) -> &'s str {
    source[node.byte_range()].trim_start_matches('\'')
}

/// Whether a node is no part of what a declaration declares: a comment, an attribute or a
/// visibility, none of which makes it another concept.
fn is_noise(node: Node) -> bool {
    node.kind().contains("comment")
        || matches!(
            node.kind(),
            "attribute_item" | "inner_attribute_item" | "visibility_modifier"
        )
}

/// A comma that only closes a list: nothing but comments sits between it and the closer.
fn is_closing_comma(node: Node) -> bool {
    if node.kind() != "," {
        return false;
    }
    let mut next = node.next_sibling();
    while let Some(sibling) = next {
        if !is_noise(sibling) {
            return matches!(sibling.kind(), ")" | "]" | "}" | ">");
        }
        next = sibling.next_sibling();
    }
    true
}

/// Whether the variants of an enum are numbered by the place they are written in: the enum is
/// fieldless or has a `repr`, and at least one variant has no number of its own.
fn variants_are_numbered_by_place(item: Node, source: &str) -> bool {
    let Some(list) = item.child_by_field_name("body") else { return false };
    let mut cursor = list.walk();
    let variants: Vec<Node> = list
        .named_children(&mut cursor)
        .filter(|variant| variant.kind() == "enum_variant")
        .collect();
    let numbered = |variant: &Node| variant.child_by_field_name("value").is_some();
    if variants.iter().all(numbered) {
        return false;
    }
    let fieldless = variants
        .iter()
        .all(|variant| variant.child_by_field_name("body").is_none());
    fieldless || has_repr(item, source)
}

/// Whether an attribute `repr` sits on the item.
fn has_repr(item: Node, source: &str) -> bool {
    let mut previous = item.prev_sibling();
    while let Some(sibling) = previous {
        match sibling.kind() {
            "attribute_item" => {
                if source[sibling.byte_range()].contains("repr") {
                    return true;
                }
            },
            kind if kind.contains("comment") => {},
            _ => return false,
        }
        previous = sibling.prev_sibling();
    }
    false
}

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
        format!("{} {}", self.name, self.body)
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
}

/// The generics and the members of a struct, enum, trait or union in canonical form.
pub(super) fn canonical_parts(node: Node, source: &str) -> Parts {
    let mut shape = Shape {
        source,
        renames: Vec::new(),
        depth: 0,
    };
    shape.parts(node)
}

impl<'s> Shape<'s> {
    fn parts(&mut self, node: Node<'s>) -> Parts {
        let generics = self.open_generics(node);
        let mut members = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            let mut place = 0usize;
            for member in body.named_children(&mut cursor).filter(|m| !is_noise(*m)) {
                members.push(self.member(member, body.kind(), &mut place));
            }
        }
        self.renames.clear();
        self.depth -= usize::from(generics.is_some());
        Parts {
            generics: generics.unwrap_or_default(),
            members,
        }
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
