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
    let dir = repo_root.join("target").join("mockspace").join("registry-schemas");
    if fs::create_dir_all(&dir).is_err() {
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
            let ty = match f.r#type.as_str() {
                "integer" => "\"type\": \"integer\"".to_string(),
                "boolean" => "\"type\": \"boolean\"".to_string(),
                "string[]" => {
                    "\"type\": \"array\", \"items\": { \"type\": \"string\" }".to_string()
                }
                _ => "\"type\": \"string\"".to_string(),
            };
            let desc = f
                .description
                .as_ref()
                .map(|d| format!(",\n          \"description\": {}", json_string(d)))
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
    let mut taplo = String::from(
        "# Generated by mockspace. Do not edit; change [[registry.namespace]]\n# in mockspace.toml and regenerate.\n#\n# Schemas are build output and live under target/. This file lives at the\n# repository root because that is where the TOML language server looks.\n#\n# Schemas bind by the array-of-tables key a file declares, never by its\n# path, so registry files may be nested and organised however the project\n# likes, including by subject rather than by kind.\n\n",
    );
    taplo.push_str(&format!(
        "[[rule]]\ninclude = [\"{mock_rel}/registry/**/*.toml\"]\nschema.path = \"target/mockspace/registry-schemas/registry.schema.json\"\n"
    ));
    if write_if_changed(&repo_root.join(".taplo.toml"), &taplo) {
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

/// Render one namespace's rows as a markdown table.
///
/// Column order follows the namespace's declared field order rather than the
/// rows' own key order, so the table reads the way the project described the
/// namespace and stays stable when a row happens to omit an optional field.
pub fn render_table(ns: &RegistryNamespace, reg: &Registry) -> String {
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
            out.push_str(&render_rows(ns, reg, members));
        }
        return out;
    }

    render_rows(ns, reg, ids)
}

/// One table over the given rows.
fn render_rows(ns: &RegistryNamespace, reg: &Registry, ids: &[String]) -> String {

    // `id` first, then declared fields in declaration order. A field that no
    // row actually carries is dropped: an always-empty column is noise.
    let mut columns: Vec<String> = vec!["id".to_string()];
    for f in &ns.fields {
        if ids
            .iter()
            .filter_map(|id| reg.get(id))
            .any(|r| r.fields.contains_key(&f.name))
        {
            columns.push(f.name.clone());
        }
    }

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
                let raw = if false {
                    row.slug.as_str()
                } else {
                    row.fields.get(c).map(String::as_str).unwrap_or("")
                };
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

/// Generate one page per namespace that asked for one.
///
/// Anchors are the lowercased identifier, matching what `resolve_all`
/// links to. The two must agree, so they are stated once each and tested
/// together rather than being left to coincide.
pub fn render_pages(
    mock_dir: &Path,
    docs_dir: &Path,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    header: &str,
    cfg: &crate::config::Config,
) -> Vec<PathBuf> {
    let mut written = Vec::new();
    for (idx, ns) in namespaces.iter().filter(|n| n.render.has_page()).enumerate() {
        let Some(ids) = reg.by_namespace.get(&ns.key) else {
            continue;
        };
        // A namespace named `catalogue` would otherwise generate `CATALOGUE.md`
        // over a hand-authored document's rendered output, silently. The
        // template is what claims the name, so its presence is the collision.
        let page_file = crate::render_design::registry_doc_name(&ns.page_name(), cfg, idx);
        let claimed = mock_dir.join(format!("{}.tmpl", ns.page_name()));
        if claimed.is_file() {
            eprintln!(
                "  ERROR: namespace `{}` would generate {} over the document rendered from {}. Rename the namespace or the template.",
                ns.key,
                ns.page_name(),
                claimed.display()
            );
            continue;
        }
        let mut body = format!("{header}\n# {}\n\n", ns.title());
        if let Some(d) = &ns.description {
            body.push_str(d);
            body.push_str("\n\n");
        }
        body.push_str(&format!(
            "{} rows. Identifiers are permanent: assigned once, never reused, never renumbered.\n\n",
            ids.len()
        ));
        body.push_str(&render_table(ns, reg));

        // A registry page is a document like any other, so references inside
        // its rows resolve here too. Without this a row could reference another
        // row and the reference would render literally on the one page most
        // likely to carry it.
        let body = resolve_all(
            &body,
            namespaces,
            reg,
            &cfg.registry_roots,
            &cfg.repo_root,
            &cfg.docs_dir,
            cfg,
        );
        let path = docs_dir.join(page_file);
        if write_if_changed(&path, &body) {
            written.push(path);
        }
    }
    written
}

/// Expand `{{registry:<key>}}` placeholders into that namespace's table.
///
/// This is what makes `embed` mode useful: a project drops the placeholder in
/// whatever document the table belongs in, rather than accepting a generated
/// page it then has to link to from somewhere.
pub fn expand_embeds(text: &str, namespaces: &[RegistryNamespace], reg: &Registry) -> String {
    let mut out = text.to_string();
    for ns in namespaces {
        let token = format!("{{{{registry:{}}}}}", ns.key);
        if out.contains(&token) {
            out = out.replace(&token, &render_table(ns, reg));
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
