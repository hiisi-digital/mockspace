use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use tree_sitter::{Node, Parser};

use crate::model::*;

pub fn discover_crates(crates_dir: &Path, crate_prefix: &str) -> CrateMap {
    let mut result = BTreeMap::new();
    let mut entries: Vec<_> = fs::read_dir(crates_dir)
        .expect("can't read crates dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("failed to set rust language");

    let prefix_dash = format!("{crate_prefix}-");

    for entry in entries {
        let dir_name = entry.file_name().to_string_lossy().to_string();
        let librs = entry.path().join("src/lib.rs");
        let cargo_toml = entry.path().join("Cargo.toml");
        if !librs.exists() {
            continue;
        }

        let short = if dir_name == crate_prefix {
            crate_prefix.to_string()
        } else {
            dir_name
                .strip_prefix(&prefix_dash)
                .unwrap_or(&dir_name)
                .to_string()
        };

        let source = fs::read_to_string(&librs).unwrap_or_default();
        let cargo = fs::read_to_string(&cargo_toml).unwrap_or_default();

        let items = parse_items(&mut parser, &source);
        let macro_generated = parse_macro_invocations(&source, crate_prefix);
        let deps = extract_deps(&cargo, &dir_name, crate_prefix);

        result.insert(dir_name, CrateInfo {
            short_name: short,
            items,
            deps,
            macro_generated,
        });
    }
    result
}

fn extract_deps(cargo_toml: &str, self_name: &str, crate_prefix: &str) -> Vec<String> {
    let mut deps = Vec::new();
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(crate_prefix) && trimmed.contains("workspace") {
            let dep: String = trimmed
                .chars()
                .take_while(|c| *c != '.' && *c != ' ' && *c != '=')
                .collect();
            let dep = dep.trim().to_string();
            if dep != self_name && !dep.is_empty() {
                deps.push(dep);
            }
        }
    }
    deps
}

// ---------------------------------------------------------------------------
// Tree-sitter helpers
// ---------------------------------------------------------------------------

fn txt<'a>(node: Node<'a>, src: &'a str) -> &'a str {
    &src[node.byte_range()]
}

fn is_pub(node: Node, src: &str) -> bool {
    for i in 0..node.child_count() {
        let child = node.child(i).unwrap();
        if child.kind() == "visibility_modifier" {
            let t = txt(child, src);
            // Only match bare `pub`, not `pub(crate)` or `pub(super)`
            return t == "pub";
        }
    }
    false
}

fn get_type_params(node: Node, src: &str) -> String {
    node.child_by_field_name("type_parameters")
        .map(|tp| txt(tp, src).to_string())
        .unwrap_or_default()
}

fn get_name(node: Node, src: &str) -> String {
    node.child_by_field_name("name")
        .map(|n| txt(n, src).to_string())
        .unwrap_or_default()
}

fn has_attribute(node: Node, src: &str, attr_name: &str) -> bool {
    if let Some(prev) = node.prev_named_sibling() {
        if prev.kind() == "attribute_item" {
            return txt(prev, src).contains(attr_name);
        }
    }
    false
}

fn detect_visibility(node: Node, src: &str) -> ApiVisibility {
    // Walk backwards through preceding siblings looking for attribute items
    let mut prev = node.prev_named_sibling();
    while let Some(p) = prev {
        if p.kind() == "attribute_item" {
            let text = txt(p, src);
            if text.contains("public_api") {
                return ApiVisibility::Public;
            }
            if text.contains("internal_api") {
                return ApiVisibility::Internal;
            }
        } else {
            break;
        }
        prev = p.prev_named_sibling();
    }
    ApiVisibility::Unspecified
}

// ---------------------------------------------------------------------------
// Item parsing
// ---------------------------------------------------------------------------

fn parse_items(parser: &mut Parser, source: &str) -> Vec<Item> {
    let tree = parser.parse(source, None).expect("parse failed");
    let root = tree.root_node();
    let mut items = Vec::new();

    let mut cursor = root.walk();
    for node in root.children(&mut cursor) {
        match node.kind() {
            "struct_item" if is_pub(node, source) => {
                items.push(parse_struct(node, source, detect_visibility(node, source)));
            }
            "trait_item" if is_pub(node, source) => {
                items.push(parse_trait(node, source, detect_visibility(node, source)));
            }
            "enum_item" if is_pub(node, source) => {
                items.push(parse_enum(node, source, detect_visibility(node, source)));
            }
            "function_item" if is_pub(node, source) => {
                items.push(Item::Fn(FnItem {
                    sig: parse_fn_sig(node, source),
                    visibility: detect_visibility(node, source),
                }));
            }
            "macro_definition" => {
                if has_attribute(node, source, "macro_export") {
                    let name = get_name(node, source);
                    if !name.is_empty() {
                        items.push(Item::Macro(MacroItem { name, is_proc: false }));
                    }
                }
            }
            "attribute_item" => {
                let attr_text = txt(node, source);
                if attr_text.contains("proc_macro") {
                    if let Some(next) = node.next_named_sibling() {
                        if next.kind() == "function_item" {
                            let name = get_name(next, source);
                            if !name.is_empty() {
                                items.push(Item::Macro(MacroItem { name, is_proc: true }));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    items
}

fn parse_struct(node: Node, src: &str, visibility: ApiVisibility) -> Item {
    let name = get_name(node, src);
    let generics = get_type_params(node, src);
    let mut fields = Vec::new();

    if let Some(body) = node.child_by_field_name("body") {
        let mut c = body.walk();
        for child in body.children(&mut c) {
            if child.kind() == "field_declaration" {
                let fname = get_name(child, src);
                let ftype = child
                    .child_by_field_name("type")
                    .map(|t| txt(t, src).to_string())
                    .unwrap_or_default();
                if !fname.is_empty() {
                    fields.push(Field { name: fname, ty: ftype });
                }
            }
        }
    }

    Item::Struct(StructItem { name, generics, fields, visibility })
}

fn parse_trait(node: Node, src: &str, visibility: ApiVisibility) -> Item {
    let name = get_name(node, src);
    let generics = get_type_params(node, src);

    let mut bounds = String::new();
    let mut c = node.walk();
    for child in node.children(&mut c) {
        if child.kind() == "trait_bounds" {
            bounds = txt(child, src).to_string();
        }
    }

    let mut methods = Vec::new();
    if let Some(body) = node.child_by_field_name("body") {
        let mut c = body.walk();
        for child in body.children(&mut c) {
            match child.kind() {
                "function_item" | "function_signature_item" => {
                    methods.push(parse_fn_sig(child, src));
                }
                _ => {}
            }
        }
    }

    Item::Trait(TraitItem { name, generics, bounds, methods, visibility })
}

fn parse_enum(node: Node, src: &str, visibility: ApiVisibility) -> Item {
    let name = get_name(node, src);
    let mut variants = Vec::new();

    if let Some(body) = node.child_by_field_name("body") {
        let mut c = body.walk();
        for child in body.children(&mut c) {
            if child.kind() == "enum_variant" {
                let vname = get_name(child, src);
                let mut has_body = false;
                let mut vc = child.walk();
                for vc_child in child.children(&mut vc) {
                    if vc_child.kind() == "field_declaration_list"
                        || vc_child.kind() == "ordered_field_declaration_list"
                    {
                        has_body = true;
                        let body_text = txt(vc_child, src);
                        variants.push(format!("{vname}{body_text}"));
                        break;
                    }
                }
                if !has_body && !vname.is_empty() {
                    variants.push(vname);
                }
            }
        }
    }

    Item::Enum(EnumItem { name, variants, visibility })
}

fn parse_fn_sig(node: Node, src: &str) -> FnSig {
    let name = get_name(node, src);
    let generics = get_type_params(node, src);

    let params = node
        .child_by_field_name("parameters")
        .map(|params_node| {
            let mut parts = Vec::new();
            let mut c = params_node.walk();
            for child in params_node.children(&mut c) {
                if child.kind() == "parameter" {
                    parts.push(txt(child, src).to_string());
                }
            }
            parts.join(", ")
        })
        .unwrap_or_default();

    let ret = node
        .child_by_field_name("return_type")
        .map(|rt| {
            let s = txt(rt, src).trim();
            s.strip_prefix("->").unwrap_or(s).trim().to_string()
        })
        .unwrap_or_default();

    FnSig { name, generics, params, ret }
}

// ---------------------------------------------------------------------------
// Macro invocation parsing (regex-based, not tree-sitter)
// ---------------------------------------------------------------------------

/// Parse lines like `<prefix>_signal::define_signal!(KeyPressed { ... })` or
/// `define_behavior!(MyBehavior { ... })` to detect macro-generated items.
fn parse_macro_invocations(source: &str, crate_prefix: &str) -> Vec<MacroGenerated> {
    let mut results = Vec::new();
    let mut inside_macro_rules = 0i32;

    for line in source.lines() {
        let trimmed = line.trim();

        // Skip comments and doc comments
        if trimmed.starts_with("//") || trimmed.starts_with("///") {
            continue;
        }

        // Track nesting inside macro_rules! bodies
        if trimmed.contains("macro_rules!") {
            inside_macro_rules += 1;
            continue;
        }
        if inside_macro_rules > 0 {
            // Rough brace tracking for macro_rules body
            let opens = trimmed.chars().filter(|c| *c == '{').count() as i32;
            let closes = trimmed.chars().filter(|c| *c == '}').count() as i32;
            inside_macro_rules += opens - closes;
            if inside_macro_rules < 0 { inside_macro_rules = 0; }
            continue;
        }

        // Skip lines referencing $crate:: (macro expansion patterns)
        if trimmed.contains("$crate::") {
            continue;
        }

        // Match patterns like:
        //   <prefix>_tree::define_marker!(Focusable);
        //   <prefix>_signal::define_signal!(KeyPressed { key: String } buffering: Queue);
        //   define_behavior!(MyBehavior { ... });

        // Find `define_*!(` pattern
        if let Some(macro_start) = trimmed.find("define_") {
            let after_define = &trimmed[macro_start..];
            // Extract macro name (up to `!`)
            if let Some(bang) = after_define.find('!') {
                let macro_name = &after_define[..bang];
                // Check it looks valid (no spaces in macro name)
                if macro_name.contains(' ') {
                    continue;
                }

                // Figure out source crate from path prefix
                let prefix = &trimmed[..macro_start];
                let underscore_prefix = format!("{}_", crate_prefix.replace('-', "_"));
                let source_crate = if prefix.ends_with("::") {
                    // e.g. "<prefix>_signal::" -> "signal"
                    let crate_path = prefix.trim_end_matches("::");
                    crate_path
                        .strip_prefix(&underscore_prefix)
                        .unwrap_or(crate_path)
                        .to_string()
                } else {
                    // Macro defined locally in same crate
                    String::new()
                };

                // Extract generated item name (first identifier after `!(`)
                let after_bang = &after_define[bang + 1..];
                let after_paren = after_bang.trim_start_matches('(');
                let generated_name: String = after_paren
                    .trim()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();

                if !generated_name.is_empty() && generated_name.chars().next().unwrap().is_uppercase() {
                    results.push(MacroGenerated {
                        macro_name: macro_name.to_string(),
                        generated_name,
                        source_crate,
                    });
                }
            }
        }
    }

    results
}
