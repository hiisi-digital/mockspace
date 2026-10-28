//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Recursively collect every `*.toml` under `dir`, skipping dot-directories.
/// Depth is deliberately unbounded: how a project organises its registry is
/// its own business, and the loader has no opinion.
fn collect_toml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !name.starts_with('.') {
                collect_toml_files(&path, out);
            }
        } else if name.ends_with(".toml") {
            out.push(path);
        }
    }
}

/// Render a `toml_edit` value as the plain string the tooling stores. Arrays
/// join with a comma so a multi-valued field still greps and still renders.
fn value_to_string(v: &toml_edit::Item) -> String {
    match v {
        toml_edit::Item::Value(toml_edit::Value::String(s)) => s.value().to_string(),
        toml_edit::Item::Value(toml_edit::Value::Integer(i)) => i.value().to_string(),
        toml_edit::Item::Value(toml_edit::Value::Boolean(b)) => b.value().to_string(),
        toml_edit::Item::Value(toml_edit::Value::Float(f)) => f.value().to_string(),
        toml_edit::Item::Value(toml_edit::Value::Array(a)) => {
            a.iter()
                .map(|e| {
                    e.as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| e.to_string().trim().to_string())
                })
                .collect::<Vec<_>>()
                .join(", ")
        },
        other => other.to_string().trim().to_string(),
    }
}

/// Load every registry row declared under `<mock>/registry/`.
///
/// Rows whose namespace is not declared are skipped rather than rejected: a
/// project may keep unrelated TOML in the tree, and refusing to start over an
/// unknown key would be the wrong failure.
pub fn load_registry(mock_dir: &Path, namespaces: &[RegistryNamespace]) -> Registry {
    load_registry_of(mock_dir, namespaces, None)
}

/// Load the rows of the namespaces in `wanted` only, or every row when it is `None`.
///
/// A file whose text never writes a `[[key]]` header of a wanted namespace is not parsed at
/// all, which is where the time goes on a registry holding one very large namespace. Such a
/// file is also left out of `files_read`, because it was not read as registry data.
pub fn load_registry_of(
    mock_dir: &Path,
    namespaces: &[RegistryNamespace],
    wanted: Option<&BTreeSet<String>>,
) -> Registry {
    let mut reg = Registry::default();
    if namespaces.is_empty() {
        return reg;
    }
    let root = mock_dir.join("registry");
    if !root.is_dir() {
        return reg;
    }
    let known: BTreeSet<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();

    let mut files = Vec::new();
    collect_toml_files(&root, &mut files);
    let all_files = files.clone();
    let mut read = Vec::new();
    let needles: Vec<String> = wanted
        .map(|w| w.iter().map(|k| format!("[[{k}]]")).collect())
        .unwrap_or_default();

    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        // A header written with inner spaces (`[[ key ]]`) is valid TOML and the needle does
        // not find it, so such a file is parsed rather than skipped.
        if wanted.is_some()
            && !needles.iter().any(|n| text.contains(n.as_str()))
            && !text.contains("[[ ")
        {
            continue;
        }
        read.push(path.clone());
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            eprintln!("  registry: {} is not valid TOML, skipped", path.display());
            continue;
        };

        for (key, item) in doc.iter() {
            if !known.contains(key) || wanted.is_some_and(|w| !w.contains(key)) {
                continue;
            }
            let Some(tables) = item.as_array_of_tables() else {
                continue;
            };
            for table in tables.iter() {
                let Some(slug) = table.get("id").and_then(|v| v.as_str()) else {
                    eprintln!(
                        "  registry: a [[{key}]] row in {} has no id, skipped",
                        path.display()
                    );
                    continue;
                };
                let mut fields = BTreeMap::new();
                for (fk, fv) in table.iter() {
                    if let toml_edit::Item::Value(toml_edit::Value::Datetime(d)) = fv {
                        reg.unquoted_times.push(UnquotedTime {
                            row:    format!("{key}::{slug}"),
                            field:  fk.to_string(),
                            text:   d.value().to_string(),
                            source: path.clone(),
                        });
                    }
                    fields.insert(fk.to_string(), value_to_string(fv));
                }
                let row = RegistryRow {
                    slug: slug.to_string(),
                    namespace: key.to_string(),
                    source: path.clone(),
                    fields,
                };
                let qualified = row.qualified();
                // The first file in walk order keeps the key path. The walk is sorted by name at
                // every directory, so which file is first does not depend on the filesystem, and
                // the `duplicate-id` finding is what reports the other files.
                if let Some(existing) = reg.rows.get(&qualified) {
                    reg.duplicates
                        .entry(qualified)
                        .or_insert_with(|| vec![existing.source.clone()])
                        .push(path.clone());
                    continue;
                }
                reg.by_namespace
                    .entry(key.to_string())
                    .or_default()
                    .push(qualified.clone());
                reg.rows.insert(qualified, row);
            }
        }
    }
    // A namespace lists its rows by key path. The order the loader met them in is the order of
    // the files and of the rows within each, which is where they happen to be written, and an
    // answer that changes when a file is split, merged, renamed or moved is an identity that
    // includes the file.
    for ids in reg.by_namespace.values_mut() {
        ids.sort();
    }
    // Unfiltered, the population is every file the walk met, readable or not.
    reg.files_read = if wanted.is_none() { all_files } else { read };
    reg
}

/// The names in `text` that are declared namespace keys.
///
/// A name runs over letters, digits, `_` and `-`, so a key such as `ikiuni-renderer` is one
/// name. Anything else ends it.
fn namespace_names_in<'a>(text: &'a str, keys: &BTreeSet<&str>) -> BTreeSet<&'a str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .filter(|t| keys.contains(t))
        .collect()
}

/// Load only what the reference expression `expr` can read, which is the namespaces it names
/// and every namespace those reach.
///
/// Reaching is followed to a fixed point, over two edges: a field typed as another namespace,
/// and a name of a declared namespace written in the text of a loaded row, where a reference
/// to it would sit. An expression asking what refers to or rests on a row (`refsto`,
/// `sourcesof`) can be answered by any row, so it loads all of them.
pub fn load_registry_for_expr(
    mock_dir: &Path,
    namespaces: &[RegistryNamespace],
    expr: &str,
) -> Registry {
    if expr.contains("refsto") || expr.contains("sourcesof") {
        return load_registry(mock_dir, namespaces);
    }
    let keys: BTreeSet<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();
    let mut wanted: BTreeSet<String> =
        namespace_names_in(expr, &keys).into_iter().map(String::from).collect();
    loop {
        let reg = load_registry_of(mock_dir, namespaces, Some(&wanted));
        let mut reach: BTreeSet<String> = BTreeSet::new();
        for ns in namespaces.iter().filter(|n| wanted.contains(&n.key)) {
            for f in &ns.fields {
                let target = f.r#type.trim_end_matches("[]");
                if keys.contains(target) {
                    reach.insert(target.to_string());
                }
            }
        }
        for row in reg.rows.values() {
            for text in row.fields.values() {
                if text.contains("{{") {
                    reach.extend(
                        namespace_names_in(text, &keys).into_iter().map(String::from),
                    );
                }
            }
        }
        if reach.is_subset(&wanted) {
            return reg;
        }
        wanted.extend(reach);
    }
}
