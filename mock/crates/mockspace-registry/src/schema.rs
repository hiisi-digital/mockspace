//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The JSON Schemas generated from the namespace declarations, and the editor binding to them.

use super::*;

/// The form of a row id the generated schema holds an id and a row reference to, as a JSON string:
/// one or more names of the standard joined by `::`. What [`crate::is_valid_row_id`] accepts.
const ROW_ID_PATTERN: &str = r#""^[a-z][a-z0-9_]*(::[a-z][a-z0-9_]*)*$""#;

/// Generate a JSON Schema per namespace, plus the tool configuration binding
/// them, into `<mock>/registry/.schemas/`.
///
/// These are generated rather than authored for a concrete reason. Hand-writing
/// them means sharing common definitions across files, and the TOML language
/// server cannot resolve cross-file references, so the shared block ends up
/// duplicated into every schema. That is the second-copy drift the discipline
/// forbids everywhere else. Generating makes the duplication free and unable to
/// drift, and the descriptions the project already wrote become editor hover
/// documentation at no extra cost.
pub fn generate_schemas(
    repo_root: &Path,
    mock_dir: &Path,
    namespaces: &[RegistryNamespace],
) -> usize {
    if namespaces.is_empty() {
        return 0;
    }
    // Schemas are derived from the namespace declarations, so they are build
    // output rather than source. They live under `target/` with the proxy and
    // the hooks, and never beside the registry data they validate: a generated
    // file sitting in a source tree invites hand-edits that the next
    // regeneration silently discards.
    let dir = crate::schemas_dir(repo_root);
    let _ = fs::create_dir_all(&dir);
    if !dir.is_dir() {
        return 0;
    }

    let mut written = 0;
    // One union schema with every namespace as an optional root property,
    // rather than one schema per namespace bound by key.
    //
    // Taplo's `keys` binding scopes a schema to the value at that key, so a
    // per-namespace schema describing a document root never matched and
    // nothing was validated. The union shape fixes that and is better anyway:
    // a single rule covers every registry file, and which part of the schema
    // applies is still decided by the array-of-tables key a file declares,
    // which is what keeps the directory layout free-form.
    let mut ns_props: Vec<String> = Vec::new();
    for ns in namespaces {
        let mut props = String::new();
        props.push_str(&format!(
            "        \"id\": {{\n          \"type\": \"string\",\n          \"pattern\": {ROW_ID_PATTERN},\n          \"description\": \"The id: one or more snake_case names joined by `::`, unique within this namespace and never a proper prefix of another id in it. An id says what it refers to, so it survives reordering and stays readable in prose, which a number does not.\"\n        }}"
        ));
        let mut required = vec!["id".to_string()];

        for f in &ns.fields {
            // A row reference is a slug on the wire, so the schema carries the
            // slug pattern and the editor rejects a malformed one where it is
            // typed. Whether the slug names a row that exists is not something
            // a schema over one file can know, and is checked separately.
            let row_ref = super::row_reference_target(&f.r#type)
                .filter(|(target, _)| namespaces.iter().any(|n| n.key == *target));
            // A closed value set becomes an `enum`, so the editor refuses an
            // unlisted value at the point it is typed. Enforcement is the schema
            // and only the schema; there is no Rust-side half, and an earlier
            // comment here named a `validate_closed_values` that has never
            // existed. `SchemaCheck::Unavailable` is a hard error where rows
            // exist, so the contract does not lapse silently on a machine
            // without a validator.
            let enum_body = if f.values.is_empty() {
                String::new()
            } else {
                format!(
                    "\"enum\": [{}]",
                    f.values
                        .iter()
                        .map(|v| enum_member(&f.r#type, v))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            // **An array's `enum` constrains its MEMBERS, so it goes inside
            // `items`.** Appended at the property level next to
            // `"type": "array"` it says the array itself must equal one of the
            // listed strings, which no array can, so every row carrying the
            // field fails with an opaque validator error. The first version of
            // this did that for all four array shapes and was tested on only
            // the one scalar shape where it happens to be right.
            let member = |base: &str| -> String {
                if enum_body.is_empty() {
                    format!("\"type\": \"array\", \"items\": {{ {base} }}")
                } else {
                    format!("\"type\": \"array\", \"items\": {{ {base}, {enum_body} }}")
                }
            };
            let scalar = |base: &str| -> String {
                if enum_body.is_empty() {
                    base.to_string()
                } else {
                    format!("{base},\n          {enum_body}")
                }
            };
            let ty = match (row_ref, f.r#type.as_str()) {
                (Some((_, true)), _) => {
                    member(&format!("\"type\": \"string\", \"pattern\": {ROW_ID_PATTERN}"))
                },
                (Some((_, false)), _) => {
                    scalar(&format!("\"type\": \"string\", \"pattern\": {ROW_ID_PATTERN}"))
                },
                // A bound is only ever on an integer; on any other type it is refused
                // by `misplaced_bounds` rather than emitted where it would mean nothing.
                (None, "integer") => scalar(&integer_with_bounds(f.min, f.max)),
                (None, "boolean") => scalar("\"type\": \"boolean\""),
                // A citation is a string on the wire. The type exists to say what
                // the string means, so validation can find it without knowing
                // what the project called the field.
                (None, "string[]") | (None, "ref[]") => member("\"type\": \"string\""),
                // A moment or a length of time is a string held to its form.
                (None, t) => {
                    match time_pattern(t) {
                        Some(p) => {
                            scalar(&format!(
                                "\"type\": \"string\", \"pattern\": {}",
                                json_string(p)
                            ))
                        },
                        None => scalar("\"type\": \"string\""),
                    }
                },
            };
            // An internal field stays in the schema: it is valid data, checked
            // like any other, and only its rendering differs. Saying so in the
            // description is what an author sees on hover, which is where the
            // question "will this reach the docs" actually gets asked.
            let internal_note = match f.visibility {
                FieldVisibility::Internal => {
                    " (internal: recorded and checked, never rendered into the generated documents)"
                },
                FieldVisibility::Public => "",
            };
            let desc = f
                .description
                .as_ref()
                .map(|d| {
                    format!(
                        ",\n          \"description\": {}",
                        json_string(&format!("{d}{internal_note}"))
                    )
                })
                .unwrap_or_default();
            props.push_str(&format!(
                ",\n        {}: {{\n          {ty}{desc}\n        }}",
                json_string(&f.name)
            ));
            // A folded field is written in the id and nowhere else, so no row carries it as a key.
            if f.required && !ns.path.contains(&f.name) {
                required.push(f.name.clone());
            }
        }

        let req = required
            .iter()
            .map(|r| json_string(r))
            .collect::<Vec<_>>()
            .join(", ");
        let title = json_string(&ns.title());
        let desc = json_string(ns.description.as_deref().unwrap_or(""));

        ns_props.push(format!(
            "    {}: {{\n      \"title\": {title},\n      \"description\": {desc},\n      \"type\": \"array\",\n      \"items\": {{\n        \"type\": \"object\",\n        \"properties\": {{\n{props}\n        }},\n        \"required\": [{req}],\n        \"additionalProperties\": false\n      }}\n    }}",
            json_string(&ns.key)
        ));
    }

    let schema = format!(
        "{{\n  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n  \"title\": \"Registry\",\n  \"description\": \"Generated from [[registry.namespace]] in mockspace.toml. Every namespace is an optional root property, so a file validates against whichever it declares.\",\n  \"type\": \"object\",\n  \"properties\": {{\n{}\n  }},\n  \"additionalProperties\": false\n}}\n",
        ns_props.join(",\n")
    );
    if write_if_changed(&dir.join("registry.schema.json"), &schema) {
        written += 1;
    }

    // The editor binding. This one lives at the repository root because that
    // is where the TOML language server looks for it, and it points into
    // `target/` where the schemas now are. Selection is by the
    // array-of-tables key a file declares, so every registry file is offered
    // every schema and the matching one applies. That is what keeps the
    // directory layout free-form: no path convention is load-bearing.
    let mock_rel = mock_dir
        .strip_prefix(repo_root)
        .unwrap_or(mock_dir)
        .to_string_lossy()
        .replace('\\', "/");
    let mut taplo = format!(
        "# {}\n# Generated by mockspace. Do not edit; change [[registry.namespace]]\n# in mockspace.toml and regenerate.\n#\n# Schemas are build output and live under target/. This file lives at the\n# repository root because that is where the TOML language server looks.\n#\n# Schemas bind by the array-of-tables key a file declares, never by its\n# path, so registry files may be nested and organised however the project\n# likes, including by subject rather than by kind.\n\n",
        crate::GENERATED_MARKER,
    );
    taplo.push_str(&format!(
        "[[rule]]\ninclude = [\"{mock_rel}/registry/**/*.toml\"]\nschema.path = \"target/mockspace/registry-schemas/registry.schema.json\"\n"
    ));
    // A project that declares no namespaces has no registry to bind a schema
    // to, and writing the file anyway puts a rule at the root of a repository
    // that has not asked for one.
    if !ns_props.is_empty() && write_taplo_if_ours(&repo_root.join(".taplo.toml"), &taplo) {
        written += 1;
    }

    // Sweep schemas for namespaces the project no longer declares. Without
    // this, renaming or removing a namespace strands its schema, and a stale
    // schema is worse than a missing one: an editor keeps validating rows
    // against a contract nobody maintains.
    let active: std::collections::BTreeSet<String> =
        ["registry.schema.json".to_string()].into_iter().collect();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".schema.json") && !active.contains(&name) {
                if fs::remove_file(entry.path()).is_ok() {
                    eprintln!("  removed stale schema {name}");
                }
            }
        }
    }
    written
}

/// The schema `enum` member a closed set's `value` is on a field of type `ty`: the parsed
/// number on an `integer`, so `+5` is written `5`, a bare `true` or `false` on a
/// `boolean`, and a string on anything else, including a value that does not read as its
/// type, which `unfit_values` refuses at the declaration.
fn enum_member(ty: &str, value: &str) -> String {
    match ty {
        "integer" => {
            match value.parse::<i64>() {
                Ok(n) => n.to_string(),
                Err(_) => json_string(value),
            }
        },
        "boolean" if value == "true" || value == "false" => value.to_string(),
        _ => json_string(value),
    }
}

/// The form a value of a time type is written in, as the pattern the generated schema
/// holds it to, and `None` for any other type. Each is written once, here, and taplo
/// enforces it on every row.
///
/// - `date`: `2026-10-28`.
/// - `time`: `18:49`, with seconds and a fraction of one where wanted.
/// - `datetime`: a date and a time joined by `T` or a space, and an offset, `Z` or
///   `+02:00`, where the moment is fixed: `2026-10-27T18:49:00Z`, `2026-10-27 18:49`.
/// - `span`: an ISO 8601 duration with at least one part, and a `T` only before a
///   part of the day: `P3D`, `PT1H30M`, `P1W`.
pub fn time_pattern(ty: &str) -> Option<&'static str> {
    match ty {
        "date" => Some(r"^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$"),
        "time" => Some(r"^([01][0-9]|2[0-3]):[0-5][0-9](:[0-5][0-9](\.[0-9]+)?)?$"),
        "datetime" => {
            Some(
                r"^[0-9]{4}-(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])[T ]([01][0-9]|2[0-3]):[0-5][0-9](:[0-5][0-9](\.[0-9]+)?)?(Z|[+-]([01][0-9]|2[0-3]):[0-5][0-9])?$",
            )
        },
        "span" => {
            Some(
                r"^P(([0-9]+Y([0-9]+M)?([0-9]+W)?([0-9]+D)?|[0-9]+M([0-9]+W)?([0-9]+D)?|[0-9]+W([0-9]+D)?|[0-9]+D)(T([0-9]+H([0-9]+M)?([0-9]+(\.[0-9]+)?S)?|[0-9]+M([0-9]+(\.[0-9]+)?S)?|[0-9]+(\.[0-9]+)?S))?|T([0-9]+H([0-9]+M)?([0-9]+(\.[0-9]+)?S)?|[0-9]+M([0-9]+(\.[0-9]+)?S)?|[0-9]+(\.[0-9]+)?S))$",
            )
        },
        _ => None,
    }
}

/// Minimal JSON string escaping. The registry's descriptions are prose written
/// by the project, so quotes and backslashes are the realistic cases.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Write only when the content differs, so regeneration does not churn
/// timestamps in git for files nobody changed.
fn write_if_changed(path: &Path, content: &str) -> bool {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return false;
        }
    }
    fs::write(path, content).is_ok()
}

/// Write the editor binding, unless the file there belongs to somebody else.
///
/// `.taplo.toml` is an ordinary file in a Rust repository and a project may
/// well have one before it ever meets this tool. Overwriting it takes their
/// rules with it, silently, on a first run. So it is written when it is absent
/// or when it carries a header this tool wrote, and left alone otherwise.
///
/// Both spellings of the header are recognised. The first version opened with
/// the plain sentence below rather than the shared marker, so a repository
/// generated before that changed would otherwise read as hand-written and
/// stop being updated.
fn write_taplo_if_ours(path: &Path, content: &str) -> bool {
    match fs::read_to_string(path) {
        Ok(existing) => {
            let ours = existing.contains(crate::GENERATED_MARKER)
                || existing.starts_with("# Generated by mockspace.");
            ours && write_if_changed(path, content)
        },
        // Absent, or unreadable, in which case the write will say so.
        // A file that exists and cannot be read is not ours to overwrite. The
        // only case that writes is the one where there is nothing there.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => write_if_changed(path, content),
        Err(_) => false,
    }
}

/// Render one namespace's rows as a markdown table.
///
/// Column order follows the namespace's declared field order rather than the
/// rows' own key order, so the table reads the way the project described the
/// namespace and stays stable when a row happens to omit an optional field.
/// An integer property's type, with `minimum` and `maximum` where the field declares
/// them.
fn integer_with_bounds(min: Option<i64>, max: Option<i64>) -> String {
    let mut out = "\"type\": \"integer\"".to_string();
    if let Some(n) = min {
        out.push_str(&format!(", \"minimum\": {n}"));
    }
    if let Some(n) = max {
        out.push_str(&format!(", \"maximum\": {n}"));
    }
    out
}


#[cfg(test)]
#[path = "schema_tests.rs"]
mod tests;
