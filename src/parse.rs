//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::{Node, Parser};

use crate::model::*;

/// Every package directory across every source directory, sorted by name.
///
/// The one place that answers "what directories are the packages", so a caller
/// cannot accidentally answer it for a single group. Returns full paths, since
/// a bare name is ambiguous once there is more than one root.
pub fn package_dirs_in(src_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = src_dirs
        .iter()
        .filter_map(|d| fs::read_dir(d).ok())
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path())
        .collect();
    out.sort_by_key(|p| p.file_name().map(|n| n.to_os_string()));
    out
}

/// Every package across every source directory.
///
/// The directories are independent and a package belongs to exactly one, so a
/// name appearing under two roots is a collision rather than a merge, and it is
/// refused. Silently keeping one would make a real package disappear while the
/// count still looked plausible.
pub fn discover_crates_in(src_dirs: &[PathBuf], crate_prefix: &str) -> CrateMap {
    let mut result = BTreeMap::new();
    for dir in src_dirs {
        for (name, info) in discover_crates(dir, crate_prefix) {
            if let Some(existing) = result.insert(name.clone(), info) {
                let _ = existing;
                panic!(
                    "two source directories both hold a package named `{name}`.\n  \
                     Source directories are independent and a package belongs to \
                     exactly one, so this is ambiguous rather than additive. \
                     Rename one, or merge the directories if they were meant to be \
                     one group.\n  Searched: {}",
                    src_dirs
                        .iter()
                        .map(|d| d.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
        }
    }
    result
}

pub fn discover_crates(crates_dir: &Path, crate_prefix: &str) -> CrateMap {
    let mut result = BTreeMap::new();
    // A missing directory yields nothing rather than failing: the default
    // `crates` is allowed not to exist for a project with no packages yet.
    // A *named* directory that is missing is refused in `Config::from_dir`,
    // where the name is known and the message can say which entry is wrong.
    let Ok(read) = fs::read_dir(crates_dir) else {
        return result;
    };
    let mut entries: Vec<_> = read
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

/// Sibling crates this crate depends on, by directory name.
///
/// Recognises every form a dependency is written in, not only workspace
/// inheritance: `dep.workspace = true`, `dep = { path = "..." }`,
/// `dep = { version = "..." }`, and a bare version string all count.
///
/// Matching only the workspace form meant a project using path dependencies
/// had every dependency ignored, which is not a partial graph but an empty
/// one: every crate computed to depth zero, the layer numbering said unbuilt
/// for everything, and the structure graph showed nodes with no edges. Nothing
/// reported it, because an empty graph looks exactly like a flat project.
fn extract_deps(cargo_toml: &str, self_name: &str, crate_prefix: &str) -> Vec<String> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            // Any dependency table counts, including target-specific ones.
            in_deps = trimmed.contains("dependencies");
            continue;
        }
        if in_deps && trimmed.starts_with(crate_prefix) && trimmed.contains('=') {
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
    for i in 0 .. node.child_count() {
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
            },
            "trait_item" if is_pub(node, source) => {
                items.push(parse_trait(node, source, detect_visibility(node, source)));
            },
            "enum_item" if is_pub(node, source) => {
                items.push(parse_enum(node, source, detect_visibility(node, source)));
            },
            "function_item" if is_pub(node, source) => {
                items.push(Item::Fn(FnItem {
                    sig:        parse_fn_sig(node, source),
                    visibility: detect_visibility(node, source),
                }));
            },
            "macro_definition" => {
                if has_attribute(node, source, "macro_export") {
                    let name = get_name(node, source);
                    if !name.is_empty() {
                        items.push(Item::Macro(MacroItem {
                            name,
                            is_proc: false,
                        }));
                    }
                }
            },
            "attribute_item" => {
                let attr_text = txt(node, source);
                if attr_text.contains("proc_macro") {
                    if let Some(next) = node.next_named_sibling() {
                        if next.kind() == "function_item" {
                            let name = get_name(next, source);
                            if !name.is_empty() {
                                items.push(Item::Macro(MacroItem {
                                    name,
                                    is_proc: true,
                                }));
                            }
                        }
                    }
                }
            },
            _ => {},
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
                    fields.push(Field {
                        name: fname,
                        ty:   ftype,
                    });
                }
            }
        }
    }

    Item::Struct(StructItem {
        name,
        generics,
        fields,
        visibility,
    })
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
                },
                _ => {},
            }
        }
    }

    Item::Trait(TraitItem {
        name,
        generics,
        bounds,
        methods,
        visibility,
    })
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

    Item::Enum(EnumItem {
        name,
        variants,
        visibility,
    })
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

    FnSig {
        name,
        generics,
        params,
        ret,
    }
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
            if inside_macro_rules < 0 {
                inside_macro_rules = 0;
            }
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

        if let Some(macro_start) = trimmed.find("define_") {
            let after_define = &trimmed[macro_start ..];
            // Extract macro name (up to `!`)
            if let Some(bang) = after_define.find('!') {
                let macro_name = &after_define[.. bang];
                // Check it looks valid (no spaces in macro name)
                if macro_name.contains(' ') {
                    continue;
                }

                // Figure out source crate from path prefix
                let prefix = &trimmed[.. macro_start];
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
                let after_bang = &after_define[bang + 1 ..];
                let after_paren = after_bang.trim_start_matches('(');
                let generated_name: String = after_paren
                    .trim()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();

                if !generated_name.is_empty()
                    && generated_name.chars().next().unwrap().is_uppercase()
                {
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

#[cfg(test)]
mod deps_tests {
    use super::*;

    /// The failure this guards was silent: the old form required the line to
    /// contain `workspace`, so a manifest using path deps yielded no edges at
    /// all. An empty dependency graph is indistinguishable from a flat
    /// architecture, so document ordering kept working and put every crate at
    /// one level, confidently.
    #[test]
    fn a_path_dep_on_a_sibling_is_a_dependency_edge() {
        let toml = r#"[package]
name = "ikiuni-renderer-store"
version.workspace = true

[dependencies]
ikiuni-renderer-contract = { path = "../ikiuni-renderer-contract" }
ikiuni-renderer-world = { path = "../ikiuni-renderer-world" }
"#;
        let deps = extract_deps(toml, "ikiuni-renderer-store", "ikiuni-renderer");
        assert_eq!(deps, vec![
            "ikiuni-renderer-contract",
            "ikiuni-renderer-world"
        ]);
    }

    #[test]
    fn a_dep_outside_a_dependency_table_is_not_an_edge() {
        // `[package]` carries a `name` that starts with the prefix. Reading it
        // as a dependency would make every crate depend on itself.
        let toml = r#"[package]
name = "ikiuni-renderer-store"

[dependencies]
ikiuni-renderer-contract = { path = "../ikiuni-renderer-contract" }
"#;
        let deps = extract_deps(toml, "ikiuni-renderer-store", "ikiuni-renderer");
        assert_eq!(deps, vec!["ikiuni-renderer-contract"]);
    }

    #[test]
    fn the_workspace_form_still_reads() {
        let toml = r#"[dependencies]
ikiuni-renderer-contract = { workspace = true }
"#;
        let deps = extract_deps(toml, "ikiuni-renderer-store", "ikiuni-renderer");
        assert_eq!(deps, vec!["ikiuni-renderer-contract"]);
    }

    /// A grouped project discovers every group, not the first one.
    ///
    /// The failure this pins is silent: reading one root returns a smaller map
    /// that looks exactly like a smaller project, which is the shape this file's
    /// own `extract_deps` comment already records as having shipped once.
    #[test]
    fn every_source_directory_contributes_its_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let mock = tmp.path();
        for (group, crate_name) in
            [("abi", "proj-abi-bus"), ("sys", "proj-pwmon"), ("boot", "proj-pid1")]
        {
            let d = mock.join(group).join(crate_name).join("src");
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("lib.rs"), "pub struct Thing;\n").unwrap();
            std::fs::write(
                mock.join(group).join(crate_name).join("Cargo.toml"),
                format!("[package]\nname = \"{crate_name}\"\n"),
            )
            .unwrap();
        }

        let one = vec![mock.join("abi")];
        let all = vec![mock.join("abi"), mock.join("sys"), mock.join("boot")];

        // The control: one root really does yield only its own package, so a
        // count of three below is the roots being walked and not an artefact.
        assert_eq!(discover_crates_in(&one, "proj").len(), 1);

        let found = discover_crates_in(&all, "proj");
        assert_eq!(found.len(), 3, "every group contributes: {:?}", found.keys());
        for name in ["proj-abi-bus", "proj-pwmon", "proj-pid1"] {
            assert!(found.contains_key(name), "{name} missing from {:?}", found.keys());
        }
    }

    /// A directory named in `src_dirs` that holds no packages is not an error
    /// here; it is refused earlier, where the name is known.
    #[test]
    fn a_source_directory_with_nothing_in_it_contributes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let empty = tmp.path().join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(discover_crates_in(&[empty], "proj").is_empty());
    }
}
