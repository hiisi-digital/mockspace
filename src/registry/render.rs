//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

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
    let dir = crate::build_dir::ensure_under_target(repo_root, &["mockspace", "registry-schemas"]);
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
            "        \"id\": {{\n          \"type\": \"string\",\n          \"pattern\": \"^[a-z][a-z0-9_]*$\",\n          \"description\": \"The slug: snake_case, unique within this namespace. A slug says what it refers to, so it survives reordering and stays readable in prose, which a number does not.\"\n        }}"
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
                        .map(|v| json_string(v))
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
                    member("\"type\": \"string\", \"pattern\": \"^[a-z][a-z0-9_]*$\"")
                },
                (Some((_, false)), _) => {
                    scalar("\"type\": \"string\", \"pattern\": \"^[a-z][a-z0-9_]*$\"")
                },
                (None, "integer") => scalar("\"type\": \"integer\""),
                (None, "boolean") => scalar("\"type\": \"boolean\""),
                // A citation is a string on the wire. The type exists to say what
                // the string means, so validation can find it without knowing
                // what the project called the field.
                (None, "string[]") | (None, "ref[]") => member("\"type\": \"string\""),
                _ => scalar("\"type\": \"string\""),
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
            if f.required {
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
        crate::render_design::GENERATED_MARKER,
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
            let ours = existing.contains(crate::render_design::GENERATED_MARKER)
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
pub fn render_table(ns: &RegistryNamespace, reg: &Registry, cfg: &crate::config::Config) -> String {
    let Some(ids) = reg.by_namespace.get(&ns.key) else {
        return String::new();
    };

    if let Some(group) = &ns.group_by {
        // Groups appear in first-seen order rather than sorted, so the file
        // order the project chose is the order a reader meets them.
        let mut order: Vec<String> = Vec::new();
        let mut buckets: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in ids {
            let key = reg
                .get(id)
                .and_then(|r| r.fields.get(group).cloned())
                .unwrap_or_else(|| "ungrouped".to_string());
            if !buckets.contains_key(&key) {
                order.push(key.clone());
            }
            buckets.entry(key).or_default().push(id.clone());
        }
        let mut out = String::new();
        for key in order {
            let members = &buckets[&key];
            out.push_str(&format!("\n### {key}\n\n"));
            out.push_str(&render_rows(ns, reg, members, cfg));
        }
        return out;
    }

    render_rows(ns, reg, ids, cfg)
}

/// A field's value with citations into internal roots removed.
///
/// Filtering is per item, not per cell. A provenance field routinely carries
/// several citations, and a row whose sources are one internal corpus and one
/// public document should keep the public one. Dropping the whole cell would
/// lose a citation the reader can actually follow, to hide one they cannot.
///
/// Only items that parse as citations are considered. A field holding ordinary
/// prose is returned unchanged, so this cannot quietly eat text that happens to
/// contain a `::`.
fn visible_value(raw: &str, cfg: &crate::config::Config) -> String {
    if cfg.internal_roots.is_empty() || raw.is_empty() {
        return raw.to_string();
    }
    let kept: Vec<&str> = raw
        .split(", ")
        .filter(|item| {
            match FileRef::parse(item.trim()) {
                Some(r) => !cfg.internal_roots.contains(&r.root),
                None => true,
            }
        })
        .collect();
    kept.join(", ")
}

/// One table over the given rows.
fn render_rows(
    ns: &RegistryNamespace,
    reg: &Registry,
    ids: &[String],
    cfg: &crate::config::Config,
) -> String {
    // `id` first, then declared fields in declaration order. A field that no
    // row actually carries is dropped: an always-empty column is noise.
    //
    // The test is what the column would RENDER, not what the rows carry, so a
    // column emptied by internal-root filtering drops by the same rule rather
    // than needing one of its own.
    let mut columns: Vec<String> = vec!["id".to_string()];
    for f in &ns.fields {
        if f.visibility == FieldVisibility::Internal {
            continue;
        }
        if ids
            .iter()
            .filter_map(|id| reg.get(id))
            .filter_map(|r| r.fields.get(&f.name))
            .any(|v| !visible_value(v, cfg).is_empty())
        {
            columns.push(f.name.clone());
        }
    }

    // Which of this namespace's fields hold row references, by field name.
    // Read from the declared types once rather than per cell.
    let row_refs: BTreeMap<&str, &str> = ns
        .fields
        .iter()
        .filter_map(|f| {
            let (target, _) = super::row_reference_target(&f.r#type)?;
            cfg.registry_namespaces
                .iter()
                .any(|n| n.key == target)
                .then_some((f.name.as_str(), target))
        })
        .collect();

    let mut out = String::new();
    out.push_str("| ");
    out.push_str(&columns.join(" | "));
    out.push_str(" |\n|");
    for _ in &columns {
        out.push_str("---|");
    }
    out.push('\n');

    for id in ids {
        let Some(row) = reg.get(id) else { continue };
        let cells: Vec<String> = columns
            .iter()
            .map(|c| {
                // `id` is a first-class field of the row rather than just an
                // entry in its map, so read it from there. The loader happens
                // to also put it in the map, but relying on that would make
                // the table depend on a loader detail.
                if c == "id" {
                    // The anchor rides the row's own first cell, so a reference
                    // lands on the row rather than on a restatement of it.
                    // Markdown has no per-row anchor of its own, and the
                    // alternative of a separate anchor block below the table
                    // duplicates every row to say nothing new.
                    return format!("<a id=\"{}\"></a>{}", row.anchor(), row.slug);
                }
                // A row reference is stored as a bare slug and rendered as a
                // reference, which the pass that follows turns into a link to
                // the target row. Emitting the reference rather than the link
                // keeps one implementation of what a row link is, including
                // the document-index lookup and the embed-mode case where a
                // link has no target.
                if let Some(target) = row_refs.get(c.as_str()) {
                    let cell = row
                        .fields
                        .get(c)
                        .map(String::as_str)
                        .unwrap_or("")
                        .split(", ")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|slug| format!("{{{{ {target}::{slug} }}}}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    return cell;
                }
                let raw = visible_value(row.fields.get(c).map(String::as_str).unwrap_or(""), cfg);
                // A pipe inside a cell would end the column early, and a
                // newline would end the row. Both are realistic in prose
                // fields, which is exactly why the source is TOML.
                raw.replace('|', "\\|").replace('\n', " ")
            })
            .collect();
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
    }
    out
}

/// One registry page's body, without the generation header.
///
/// Separated from writing so the page goes through the same render as every
/// other document: the header, the placeholders, the references, and the write
/// all happen in one place rather than once per generation path.
pub fn registry_page_body(
    ns: &RegistryNamespace,
    reg: &Registry,
    cfg: &crate::config::Config,
) -> String {
    let count = reg.by_namespace.get(&ns.key).map(Vec::len).unwrap_or(0);
    let mut body = format!("# {}\n\n", ns.title());
    if let Some(d) = &ns.description {
        body.push_str(d);
        body.push_str("\n\n");
    }
    body.push_str(&format!(
        "{count} rows. Identifiers are permanent: assigned once, never reused, never renumbered.\n\n"
    ));
    body.push_str(&render_table(ns, reg, cfg));
    body
}

/// Expand `{{registry:<key>}}` placeholders into that namespace's table.
///
/// This is what makes `embed` mode useful: a project drops the placeholder in
/// whatever document the table belongs in, rather than accepting a generated
/// page it then has to link to from somewhere.
pub fn expand_embeds(
    text: &str,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    cfg: &crate::config::Config,
) -> String {
    let mut out = text.to_string();
    for ns in namespaces {
        let token = format!("{{{{registry:{}}}}}", ns.key);
        if out.contains(&token) {
            out = out.replace(&token, &render_table(ns, reg, cfg));
        }
    }
    out
}

/// The short form: who and when, enough to recognise the work.
///
/// What a reader needs at a glance, and what fits in a table cell beside four
/// hundred others. The full citation is one click away, which is the whole
/// reason the short form is safe.
pub fn short_citation(row: &RegistryRow) -> String {
    let f = |k: &str| row.fields.get(k).filter(|v| !v.is_empty()).cloned();
    match (f("authors"), f("year")) {
        (Some(a), Some(y)) => format!("{a} {y}"),
        (Some(a), None) => a,
        (None, Some(y)) => format!("{}, {y}", f("title").unwrap_or_else(|| row.slug.clone())),
        (None, None) => f("title").unwrap_or_else(|| row.slug.clone()),
    }
}

/// Render a reference row as a citation.
///
/// Assembled from the fields the row declares, skipping what it does not, so a
/// specification with an institutional author and no page reads as cleanly as
/// a journal paper. The format is deliberately plain: the value is that one
/// work cited in twenty places renders identically in all twenty, not that it
/// matches any particular house style.
pub fn format_citation(row: &RegistryRow) -> String {
    let f = |k: &str| row.fields.get(k).filter(|v| !v.is_empty()).cloned();
    let mut out = String::new();
    if let Some(a) = f("authors") {
        out.push_str(&a);
        out.push_str(", ");
    }
    out.push_str(&f("title").unwrap_or_else(|| row.slug.clone()));
    if let Some(v) = f("venue") {
        let year = f("year").map(|y| format!(" {y}")).unwrap_or_default();
        out.push_str(&format!(" ({v}{year})"));
    } else if let Some(y) = f("year") {
        out.push_str(&format!(" ({y})"));
    }
    out
}

#[cfg(test)]
mod closed_value_set_placement {
    //! Where a `values` list lands in the emitted schema, for every field type
    //! it can sit on.
    //!
    //! The first version put the `enum` next to `"type": "array"` for all four
    //! array shapes, which no array value can satisfy, so every row carrying
    //! such a field would have failed with an opaque validator error. It was
    //! tested on `string` alone, where the placement happens to be right: one
    //! of six shapes, and the five untested ones were the broken ones.
    use super::*;
    use crate::registry::{FieldVisibility, RegistryField, RegistryNamespace, RenderMode};

    fn schema_for(ty: &str, values: &[&str]) -> String {
        let ns = RegistryNamespace {
            key:         "probe".into(),
            title:       None,
            description: None,
            value_field: None,
            render:      RenderMode::Page,
            group_by:    None,
            fields:      vec![RegistryField {
                name:        "f".into(),
                r#type:      ty.into(),
                required:    false,
                description: None,
                visibility:  FieldVisibility::Public,
                values:      values.iter().map(|s| s.to_string()).collect(),
            }],
        };
        let tmp = tempfile::tempdir().unwrap();
        let n = generate_schemas(tmp.path(), &tmp.path().join("mock"), &[ns]);
        assert!(n > 0, "control: the generator wrote something");
        let dir = tmp
            .path()
            .join("target")
            .join("mockspace")
            .join("registry-schemas");
        let mut out = String::new();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            out.push_str(&std::fs::read_to_string(e.path()).unwrap());
        }
        out
    }

    /// The `f` property's own body. The document wraps every namespace in
    /// `"type": "array", "items": {...}`, so a search over the whole file
    /// cannot tell a field's array from that one, which is how the first
    /// version of this test managed to pass on a scalar.
    fn property(schema: &str) -> String {
        let i = schema.find("\"f\": {").expect("the probe field is emitted");
        let rest = &schema[i ..];
        let j = rest.find("\n        }").expect("its body closes");
        rest[.. j].to_string()
    }

    #[test]
    fn an_arrays_enum_constrains_its_members_and_a_scalars_constrains_itself() {
        for (ty, json) in [
            ("string", "\"type\": \"string\""),
            ("integer", "\"type\": \"integer\""),
            ("boolean", "\"type\": \"boolean\""),
        ] {
            let p = property(&schema_for(ty, &["a", "b"]));
            assert_eq!(
                p,
                format!("\"f\": {{\n          {json},\n          \"enum\": [\"a\", \"b\"]"),
                "{ty}: a scalar's enum sits on the property"
            );
        }
        for ty in ["string[]", "ref[]"] {
            let p = property(&schema_for(ty, &["a", "b"]));
            assert_eq!(
                p,
                "\"f\": {\n          \"type\": \"array\", \"items\": { \"type\": \"string\", \"enum\": [\"a\", \"b\"] }",
                "{ty}: an array's enum constrains its members, so it goes inside items, \
                 never beside `type: array` where no array value can satisfy it"
            );
        }
    }

    #[test]
    fn no_values_means_no_enum_at_any_type() {
        // the control. without it every arm above passes on a generator that
        // emits an enum unconditionally.
        for ty in ["string", "integer", "boolean", "string[]", "ref[]"] {
            let p = property(&schema_for(ty, &[]));
            assert!(!p.contains("enum"), "{ty}: {p}");
        }
    }
}

/// The editor binding is written where nobody else's file is.
#[cfg(test)]
mod the_editor_binding_is_not_written_over_somebody_elses {
    use super::*;

    #[test]
    fn a_taplo_file_that_is_not_ours_survives_a_first_run() {
        // `.taplo.toml` is an ordinary file in a Rust repository, and this is
        // the third instance of one class: generation writing over what it
        // found. The other two were `.claude/settings.json` and the documents
        // directory, and both were fixed by reading the file before writing
        // it. This one was not.
        let d = tempfile::tempdir().unwrap();
        let theirs = d.path().join(".taplo.toml");
        let before = "[[rule]]\ninclude = [\"my.toml\"]\nschema.path = \"mine.json\"\n";
        fs::write(&theirs, before).unwrap();

        assert!(
            !write_taplo_if_ours(&theirs, "# ours\n"),
            "somebody else's file was written to"
        );
        assert_eq!(fs::read_to_string(&theirs).unwrap(), before);
    }

    #[test]
    fn a_taplo_file_we_wrote_is_updated_and_an_absent_one_is_created() {
        // The permit half. A guard that refused every existing file would pass
        // the test above and freeze the binding at whatever the first run
        // produced, so a namespace added later would never reach an editor.
        let d = tempfile::tempdir().unwrap();
        let ours = d.path().join(".taplo.toml");

        assert!(
            write_taplo_if_ours(&ours, "# absent\n"),
            "an absent file was not created"
        );
        assert_eq!(fs::read_to_string(&ours).unwrap(), "# absent\n");

        let marked = format!("# {}\n# old\n", crate::render_design::GENERATED_MARKER);
        fs::write(&ours, &marked).unwrap();
        let next = format!("# {}\n# new\n", crate::render_design::GENERATED_MARKER);
        assert!(
            write_taplo_if_ours(&ours, &next),
            "our own file was not updated"
        );
        assert_eq!(fs::read_to_string(&ours).unwrap(), next);
    }

    #[test]
    fn the_header_the_first_version_wrote_is_still_recognised_as_ours() {
        // A repository generated before the shared marker went into the header
        // carries the plain sentence instead. Reading that as somebody else's
        // file would freeze its binding forever, on exactly the repositories
        // that have been using this longest.
        let d = tempfile::tempdir().unwrap();
        let ours = d.path().join(".taplo.toml");
        fs::write(
            &ours,
            "# Generated by mockspace. Do not edit; change [[registry.namespace]]\n",
        )
        .unwrap();
        assert!(write_taplo_if_ours(&ours, "# next\n"));
        assert_eq!(fs::read_to_string(&ours).unwrap(), "# next\n");
    }
}
