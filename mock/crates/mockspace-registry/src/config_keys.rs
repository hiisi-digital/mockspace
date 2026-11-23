//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The keys of the config tables the registry reads, and the ones nothing reads.

use super::*;

/// Keys a `[[registry.namespace]]` table may carry. Mirrors `RegistryNamespace`.
///
/// Hand-kept, and constrained rather than trusted: `namespace_keys_match_the_struct`
/// in `validate_tests.rs` reads the struct's serde keys out of `model.rs` and fails
/// when the two disagree, bar the keys in `UNREAD_NAMESPACE_KEYS`. `FIELD_KEYS` and
/// `FINDING_KINDS` are held the same way, the kinds in both directions.
pub(crate) const NAMESPACE_KEYS: &[&str] = &[
    "key",
    "title",
    "description",
    "value_field",
    "group_by",
    "field",
    "path",
    "console_direct",
];

/// The keys in `NAMESPACE_KEYS` the struct does not carry, because a tool other
/// than this loader reads them. `console_direct = true` lets the console commit an
/// edit to the namespace's rows straight onto the trunk, and `mock rows-gate` holds a
/// commit staging those rows alone to the registry check only. Reporting it as discarded
/// would be false, since something does read it, and reading it here would give
/// the loader a field it has no use for. The test holds this list apart from the
/// struct, so a key the struct does read cannot hide in it.
#[cfg(test)]
pub(crate) const UNREAD_NAMESPACE_KEYS: &[&str] = &["console_direct"];

/// Keys a `[[registry.namespace.field]]` table may carry. Mirrors `RegistryField`.
pub(crate) const FIELD_KEYS: &[&str] =
    &["name", "type", "required", "description", "visibility", "values", "min", "max"];

/// Keys a `[ref.roots.<name>]` table may carry. Mirrors `RawRefRoot`.
///
/// Checked for the same reason the two above are, and found the same way: a
/// root-level key written one line too far down the file lands inside whichever
/// table precedes it. `canon_paths` went in under a `[ref.roots.*]` table, was
/// read as a key of that root, and was discarded without a word, so the feature
/// it configures stayed off while the config plainly said otherwise. That cost
/// a debugging cycle to find and would have cost more had the feature been one
/// whose absence is quiet.
pub const REF_ROOT_KEYS: &[&str] = &["path", "frozen", "links", "label", "internal"];

/// Keys `[registry]` may carry. Mirrors `RegistryDecl`.
const REGISTRY_KEYS: &[&str] = &["namespace", "store"];

/// Report keys of the tables the registry reads that deserialize into nothing: in `[ref]`, in
/// `[ref.roots.*]`, in `[registry]` and in the namespace declarations. The keys of the document
/// root are whoever owns the rest of the config's to hold: this crate reads `registry` and `ref`
/// there and passes over every other.
///
/// The registry's row data is covered by generated schemas run through a TOML
/// validator, which is why this file deliberately does not re-check what a schema
/// can express. **The config file is not covered by any of that.** `serde` does
/// not deny unknown fields here, so a key that mockspace does not implement, or a
/// key with a typo in it, is read and discarded in silence.
///
/// That is not hypothetical. The largest registry in the workspace declares
/// `prefix` on twelve of its fifteen namespaces. Mockspace has no such field, so
/// all twelve are discarded, and the rows in those namespaces carry plain slugs
/// with no prefix in them: the declaration does nothing and its author cannot
/// tell. Whether `prefix` should exist is a separate question this does not
/// answer. What it ends is the silence.
pub fn config_unknown_keys(config_text: &str) -> Vec<RegistryFinding> {
    let mut out = Vec::new();
    let Ok(doc) = config_text.parse::<toml_edit::DocumentMut>() else {
        return out; // a config that does not parse is somebody else's error
    };

    // `[ref]` itself. `roots` is the only thing it carries, so anything else
    // here is a key that read as configuration and was not.
    if let Some(refs) = doc.get("ref").and_then(|i| i.as_table_like()) {
        for (k, _) in refs.iter() {
            if k != "roots" {
                out.push(RegistryFinding {
                    kind:    "unknown-config-key",
                    message: format!(
                        "[ref] declares `{k}`, which mockspace does not read there. It is \
                         discarded silently; the only key `[ref]` carries is `roots`."
                    ),
                    source:  None,
                    gate:    GateScope::Always,
                });
            }
        }
    }

    // `[ref.roots.<name>]`, because a stray key lands here by accident rather
    // than by being written here on purpose: TOML gives a bare key to whichever
    // table header precedes it, and these tables sit near the top of a config
    // where root-level keys are also written.
    //
    // NOTE: `as_table_like` rather than `as_table` throughout. A root written
    // inline, `seed = { path = "p" }`, is the same configuration and the first
    // version skipped it in silence, which is the silence this exists to end.
    if let Some(roots) = doc
        .get("ref")
        .and_then(|i| i.as_table_like())
        .and_then(|t| t.get("roots"))
        .and_then(|i| i.as_table_like())
    {
        for (name, item) in roots.iter() {
            let Some(table) = item.as_table_like() else {
                continue;
            };
            for (k, _) in table.iter() {
                if !REF_ROOT_KEYS.contains(&k) {
                    out.push(RegistryFinding {
                        kind:    "unknown-config-key",
                        message: format!(
                            "[ref.roots.{name}] declares `{k}`, which mockspace does not read \
                             there. It is discarded silently. A bare key belongs to whichever \
                             table header precedes it, so a root-level setting written below one \
                             of these lands here and has no effect at all."
                        ),
                        source:  None,
                        gate:    GateScope::Always,
                    });
                }
            }
        }
    }

    let Some(reg) = doc.get("registry").and_then(|i| i.as_table()) else {
        return out;
    };
    // `[registry]` itself, which carries the namespaces and the stores and nothing else. A
    // misspelt `[[registry.namspace]]` lands here, and reads as a project with one namespace fewer.
    for (k, _) in reg.iter() {
        if !REGISTRY_KEYS.contains(&k) {
            out.push(RegistryFinding {
                kind:    "unknown-config-key",
                message: format!(
                    "[registry] declares `{k}`, which mockspace does not read there. It is discarded \
                     silently; `[registry]` carries `namespace` and `store`."
                ),
                source:  None,
                gate:    GateScope::Always,
            });
        }
    }
    // A key of `[[registry.store]]` that is not read refuses the config as it parses (the one
    // declaration is `deny_unknown_fields`), so there is nothing to report here.
    let Some(spaces) = reg.get("namespace").and_then(|i| i.as_array_of_tables()) else {
        return out;
    };
    for table in spaces.iter() {
        let ns = table
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("<no key>")
            .to_string();
        for (k, _) in table.iter() {
            if !NAMESPACE_KEYS.contains(&k) {
                out.push(RegistryFinding {
                    kind:    "unknown-config-key",
                    message: format!(
                        "[[registry.namespace]] `{ns}` declares `{k}`, which mockspace does not \
                         read. It is discarded silently, so the declaration has no effect. Remove \
                         it, or fix the spelling if it is a typo."
                    ),
                    source:  None,
                    gate:    GateScope::Always,
                });
            }
        }
        if let Some(fields) = table.get("field").and_then(|i| i.as_array_of_tables()) {
            for f in fields.iter() {
                let name = f
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<no name>");
                for (k, _) in f.iter() {
                    if !FIELD_KEYS.contains(&k) {
                        out.push(RegistryFinding {
                            kind:    "unknown-config-key",
                            message: format!(
                                "[[registry.namespace.field]] `{ns}.{name}` declares `{k}`, which \
                                 mockspace does not read. It is discarded silently."
                            ),
                            source:  None,
                            gate:    GateScope::Always,
                        });
                    }
                }
            }
        }
    }
    out
}
