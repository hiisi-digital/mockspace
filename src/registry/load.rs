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
    reg.files_read = files.clone();

    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
            eprintln!("  registry: {} is not valid TOML, skipped", path.display());
            continue;
        };

        for (key, item) in doc.iter() {
            if !known.contains(key) {
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
                    fields.insert(fk.to_string(), value_to_string(fv));
                }
                let row = RegistryRow {
                    slug: slug.to_string(),
                    namespace: key.to_string(),
                    source: path.clone(),
                    fields,
                };
                let qualified = row.qualified();
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
    reg
}
