//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

#[path = "validate_provenance.rs"]
mod provenance;
pub use provenance::validate_provenance;
#[path = "validate_schemas.rs"]
mod schemas;
pub use schemas::{SchemaCheck, check_schemas, check_schemas_with, files_examined};

/// One registry validation finding.
#[derive(Debug, Clone)]
pub struct RegistryFinding {
    /// Which check produced it, so severity can be configured per kind rather
    /// than for the registry as a whole.
    pub kind:    &'static str,
    pub message: String,
    /// Where to print it from: the file the finding is about, when it has one. Says nothing about
    /// whether the finding gates; `gate` does.
    pub source:  Option<PathBuf>,
    /// Which runs the finding blocks, set where the finding is made.
    pub gate:    GateScope,
}

/// Which runs a registry finding blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateScope {
    /// Every run blocks on it. The default for anything about the registry itself: duplicate
    /// identifiers, cycles, provenance, declarations, values.
    Always,
    /// A reference to a row. Only a run whose changed files include this one (the file that holds
    /// the reference) blocks; any other run reports it as a warning, so a row can be retired
    /// before every citation of it is gone, and the error arrives with the next change to the
    /// citing file.
    WhenChanged(PathBuf),
}

/// The finding kinds this validation produces, for per-kind severity config.
///
/// Deliberately short. Everything a JSON Schema can express (required fields,
/// identifier patterns, types, unknown keys) is checked by running the
/// generated schemas through a TOML validator, not here: two implementations
/// of one contract drift, and the schema is the one the editor already uses.
///
/// What remains are the checks a schema structurally cannot make. A schema
/// validates one document, so it cannot see an identifier declared in two
/// different files. And a reference lives in rendered prose rather than in the
/// data, so no schema over the data can know whether it resolves.
pub const FINDING_KINDS: &[&str] = &[
    "reference-cycle",
    "duplicate-id",
    "malformed-provenance",
    "unknown-provenance-root",
    "unresolvable-provenance",
    "ambiguous-provenance",
    "needless-extension",
    "unresolvable-heading",
    "fragile-line-citation",
    "malformed-row-reference",
    "unknown-row-reference",
    "unknown-field-type",
    "namespace-shadows-type",
    "row-reference-to-a-value-namespace",
    "misplaced-field-bound",
    "unfit-field-value",
    "unquoted-time",
    // The three produced by `namespace_root_collisions`, which is called from
    // the same place as `validate` and whose kinds were absent from this list
    // with nothing reporting that. The comment below used to say two were
    // missing; `duplicate-namespace-key` is the third and was added with them.
    "namespace-root-collision",
    "namespace-reserved-name",
    "duplicate-namespace-key",
    // Not produced by `validate`. Reported by the caller when the schema check
    // could not run at all, and listed here so the set of kinds stays in one
    // place. A run that could not check is not a run that passed.
    //
    // NOTE: the per-finding severity map (`table.findings`, parsed at
    // `config.rs:1017`) is a real, working mechanism elsewhere: ordinary lints
    // registered with `mockspace_lint_rules::run_lint` consult it already. No
    // registry check is dispatched that way, though: the engine calls
    // `check` by hand and prints its findings
    // directly, which is why the map went unconsulted for this whole list.
    // `unknown-config-key` is now the one exception (severity is read at that
    // call site under the lint name `registry-config-keys`). Every other kind
    // here still has no consumer.
    // a row the generated schema of its namespace refuses, from `check`
    "schema",
    "schema-unavailable",
    // Produced by `config_unknown_keys`, over the config file rather than the
    // row data, which no generated schema covers.
    "unknown-config-key",
];

/// Keys a `[[registry.namespace]]` table may carry. Mirrors `RegistryNamespace`.
///
/// Hand-kept, and constrained rather than trusted: `namespace_keys_match_the_struct`
/// in `validate_tests.rs` reads the struct's serde keys out of `model.rs` and fails
/// when the two disagree, bar the keys in `UNREAD_NAMESPACE_KEYS`. `FIELD_KEYS` and
/// `FINDING_KINDS` are held the same way, the kinds in both directions.
const NAMESPACE_KEYS: &[&str] = &[
    "key",
    "title",
    "description",
    "value_field",
    "group_by",
    "field",
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
const UNREAD_NAMESPACE_KEYS: &[&str] = &["console_direct"];

/// Keys a `[[registry.namespace.field]]` table may carry. Mirrors `RegistryField`.
const FIELD_KEYS: &[&str] =
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

/// Validate what the generated schemas cannot.
pub fn validate(namespaces: &[RegistryNamespace], reg: &Registry) -> Vec<RegistryFinding> {
    let mut out = unquoted_times(namespaces, reg);
    for (id, sources) in &reg.duplicates {
        out.push(RegistryFinding {
            kind: "duplicate-id",
            message: format!(
                "{id} is declared {} times ({}). An identifier names one row; two rows for one identifier means a reference cannot be resolved. No per-file schema can catch this, because each file is valid on its own.",
                sources.len(),
                sources
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            source: sources.first().cloned(),
            gate:   GateScope::Always,
        });
    }
    out
}

/// Placeholder-shaped tokens still present in a generated document.
///
/// A safety net rather than a check of any one code path. Documents are
/// generated by several paths (the design document, the per-crate documents,
/// the root passthroughs, the registry tables), and each resolves references
/// itself. Two of them silently did not, so every reference in them rendered
/// literally and nothing said so, because the dangling-reference check runs
/// over what was offered to the resolver and these were never offered.
///
/// Scanning the output instead of the paths means a path added later cannot
/// reintroduce the failure quietly: whatever writes the document, an
/// unresolved token in it is reported.
///
/// Fenced blocks are skipped, matching resolution: a document explaining the
/// syntax shows it, and that is not a defect.
pub fn unresolved_in_generated(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fences = mockspace_refnames::Fences::new();
    for line in text.lines() {
        if !fences.prose(line) {
            continue;
        }
        for (raw, _) in crate::placeholder_exprs(line) {
            out.push(raw);
        }
    }
    out
}

/// A namespace whose name is also a citation root, which makes a bare
/// reference ambiguous.
///
/// `law::keys` means the `law` namespace. It would also mean a citation into a
/// root named `law`, and nothing in the reference says which. The prefixed form
/// disambiguates, but a project should not have to reach for it, so the
/// collision is a configuration error rather than a precedence rule nobody can
/// remember.
///
/// It also refuses one key declared twice, which is the same kind of defect one
/// step earlier: a name that resolves to two different things depending on
/// which code path is asking. The function is named for the collision it was
/// written for and now covers both, since both are checks over the declared
/// namespaces and there is nowhere else they would go.
pub fn namespace_root_collisions(
    namespaces: &[RegistryNamespace],
    roots: &std::collections::BTreeMap<String, String>,
) -> Vec<RegistryFinding> {
    let mut out = Vec::new();

    // Two declarations under one key, which nothing refused and which the
    // engine itself does not resolve consistently.
    //
    // A key is global over the whole of `<mock>/registry/`: `load_registry_in_dir`
    // builds one set of known keys and walks the tree against it, and a row is
    // addressed `<namespace>::<slug>` with no part of its path in the address.
    // So a second declaration does not partition anything. It supplies a
    // second schema for one name, and every row of both is read against
    // whichever of the two a given code path happens to pick.
    //
    // The paths disagree, which is what makes this worse than either answer
    // alone. `resolve.rs` and the `query` subcommand collect the declarations
    // into a `BTreeMap` keyed by name, so the **last** wins there. The
    // diagnostic that lists a namespace's columns uses `iter().find`, so the
    // **first** wins there. A project with a duplicate gets rendered documents
    // built against one schema and an error message describing the other.
    //
    // Refused rather than reported, because there is no reading of a duplicate
    // that a project could have wanted: the rows of the shadowed one are being
    // validated against a schema nobody wrote for them, silently.
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for ns in namespaces {
        if !seen.insert(ns.key.as_str()) {
            out.push(RegistryFinding {
                kind: "duplicate-namespace-key",
                message: format!(
                    "`{}` is declared more than once. A registry key is global over the whole of the registry directory rather than scoped to a file, so the second declaration does not partition anything: it gives one name two schemas, and the rows of one are read against the other. Rename one of them.",
                    ns.key
                ),
                source: None,
                gate:   GateScope::Always,
            });
        }
    }

    for ns in namespaces {
        if roots.contains_key(&ns.key) {
            out.push(RegistryFinding {
                kind: "namespace-root-collision",
                message: format!(
                    "`{}` is both a registry namespace and a citation root, so `{}::x` is ambiguous. Rename one of them.",
                    ns.key, ns.key
                ),
                source: None,
                gate:   GateScope::Always,
            });
        }
        // The reserved words are the other occupants of slot zero, and they
        // fail worse than a root collision does. A namespace named `crates` is
        // shadowed silently. One named `reg` rewrites `reg::x` into
        // `reg::reg::x` on every pass and never terminates.
        if crate::RESERVED_ROOTS.contains(&ns.key.as_str()) {
            out.push(RegistryFinding {
                kind: "namespace-reserved-name",
                message: format!(
                    "`{}` is a reserved reference root, so a namespace cannot take the name. Rename the namespace.",
                    ns.key
                ),
                source: None,
                gate:   GateScope::Always,
            });
        }
    }
    out
}

#[cfg(test)]
#[path = "validate_tests.rs"]
mod tests;

/// A value in a `date`, `time`, `datetime` or `span` field that the file wrote as a TOML
/// date or time instead of a string.
///
/// The types are strings held to one form. A bare `when = 2026-10-28` is a different
/// TOML type that happens to read the same, and taplo hands a TOML date to the schema as
/// a string, so the generated pattern passes it. Only the loader saw the difference, and
/// it recorded it for this.
fn unquoted_times(namespaces: &[RegistryNamespace], reg: &Registry) -> Vec<RegistryFinding> {
    let mut out = Vec::new();
    for u in &reg.unquoted_times {
        let Some(ns_key) = crate::names(&u.row).ok().and_then(|n| n.first().copied()) else {
            continue;
        };
        let Some(field) = namespaces
            .iter()
            .filter(|n| n.key == ns_key)
            .flat_map(|n| n.fields.iter())
            .find(|f| f.name == u.field)
        else {
            continue;
        };
        if time_pattern(&field.r#type).is_none() {
            continue;
        }
        out.push(RegistryFinding {
            kind:    "unquoted-time",
            message: format!(
                "{}: `{} = {}` is a TOML date or time, not a string, and the field `{}` is a `{}`, \
                 which holds a string in one form. Quote it: `{} = \"{}\"`.",
                u.row, u.field, u.text, u.field, field.r#type, u.field, u.text
            ),
            source:  Some(u.source.clone()),
            gate:    GateScope::Always,
        });
    }
    out
}
