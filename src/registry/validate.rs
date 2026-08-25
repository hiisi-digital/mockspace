//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Check every provenance reference in the registry against the declared roots.
///
/// Three ways a reference can be wrong, and all three are silent without this:
/// a malformed string that no one notices is not a reference; a root nobody
/// declared; and a file or line that has moved since the reference was written.
///
/// Roots resolve against the repository root rather than the mock directory,
/// so a row may cite shipping code and shipping documentation alongside design
/// material. That matters because a row usually has several sources that mean
/// different things: the corpus it was derived from, the live design document
/// that supersedes it, and the code that implements it.
///
/// The root name carries that meaning. A project naming its roots `seed`,
/// `mock`, and `live` gets three readable kinds of citation without any extra
/// field, and array order is precedence: the first reference is the one a
/// reader should follow, the rest are context.
pub fn validate_provenance(
    repo_root: &Path,
    roots: &BTreeMap<String, String>,
    frozen: &BTreeSet<String>,
    reg: &Registry,
    namespaces: &[super::RegistryNamespace],
) -> Vec<RegistryFinding> {
    let mut out = Vec::new();
    // Which fields hold references is read off each namespace's declared field
    // TYPES. Keying on the literal name `provenance` validated one field for one
    // consumer, and silently ignored every other reference-bearing field a
    // project declared. The design says provenance is deliberately not
    // universal; the hardcoded name made it universal anyway.
    let bearing = super::reference_fields(namespaces);

    for row in reg.rows.values() {
        let empty = Vec::new();
        let names = bearing.get(&row.namespace).unwrap_or(&empty);
        for raw in names.iter().filter_map(|n| row.fields.get(n)) {
            for item in raw.split(", ").filter(|s| !s.trim().is_empty()) {
                let Some(p) = FileRef::parse(item) else {
                    out.push(RegistryFinding {
                    kind: "malformed-provenance",
                    message: format!(
                        "{}: `{item}` is not `root::path::line`. Unparsed, it points nowhere while looking like a citation.",
                        row.qualified()
                    ),
                    source: Some(row.source.clone()),
                });
                    continue;
                };
                let Some(root_rel) = roots.get(&p.root) else {
                    out.push(RegistryFinding {
                    kind: "unknown-provenance-root",
                    message: format!(
                        "{}: `{}` names root `{}`, which has no [ref.roots.{}] table. Declared roots are what make a reference resolvable rather than a convention.",
                        row.qualified(),
                        p.render(),
                        p.root,
                        p.root
                    ),
                    source: Some(row.source.clone()),
                });
                    continue;
                };
                let target = match resolve_cited_path(&repo_root.join(root_rel), &p.path) {
                    PathResolution::Found(t) => t,
                    PathResolution::Missing => {
                        out.push(RegistryFinding {
                            kind:    "unresolvable-provenance",
                            message: format!(
                                "{}: `{}` matches no file under root `{}`.",
                                row.qualified(),
                                p.render(),
                                p.root
                            ),
                            source:  Some(row.source.clone()),
                        });
                        continue;
                    },
                    PathResolution::Ambiguous(names) => {
                        out.push(RegistryFinding {
                        kind: "ambiguous-provenance",
                        message: format!(
                            "{}: `{}` matches several files ({}). Give the extension: picking one silently would point the citation somewhere you did not choose.",
                            row.qualified(),
                            p.render(),
                            names.join(", ")
                        ),
                        source: Some(row.source.clone()),
                    });
                        continue;
                    },
                };
                if matches!(p.anchor, Anchor::Line(_)) && !frozen.contains(&p.root) {
                    out.push(RegistryFinding {
                    kind: "fragile-line-citation",
                    message: format!(
                        "{}: `{}` cites a line in root `{}`, which is not declared frozen. Any edit above that line silently repoints the citation while it still resolves. Cite a heading, or declare the root frozen if its files genuinely do not move.",
                        row.qualified(),
                        p.render(),
                        p.root
                    ),
                    source: Some(row.source.clone()),
                });
                }
                match resolve_anchor(&target, &p.anchor) {
                    Some(_) => {},
                    None => {
                        out.push(RegistryFinding {
                        kind: match p.anchor {
                            Anchor::Heading(_) => "unresolvable-heading",
                            Anchor::Line(_) => "unresolvable-provenance",
                        },
                        message: match &p.anchor {
                            Anchor::Heading(h) => format!(
                                "{}: `{}` names heading `{h}`, which {} does not contain. A renamed heading fails here rather than silently pointing elsewhere, which is why headings are preferred over line numbers.",
                                row.qualified(),
                                p.render(),
                                p.path
                            ),
                            Anchor::Line(n) => format!(
                                "{}: `{}` points past the end of {} ({n} requested). Line citations only stay true in a root that does not change; declare the root frozen or cite a heading.",
                                row.qualified(),
                                p.render(),
                                p.path
                            ),
                        },
                        source: Some(row.source.clone()),
                    });
                    },
                }
            }
        }
    }
    out
}

/// One registry validation finding.
#[derive(Debug, Clone)]
pub struct RegistryFinding {
    /// Which check produced it, so severity can be configured per kind rather
    /// than for the registry as a whole.
    pub kind:    &'static str,
    pub message: String,
    pub source:  Option<PathBuf>,
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
    "dangling-reference",
    "malformed-provenance",
    "unknown-provenance-root",
    "unresolvable-provenance",
    "ambiguous-provenance",
    "unresolvable-heading",
    "fragile-line-citation",
    "malformed-row-reference",
    "unknown-row-reference",
    "unknown-field-type",
    "namespace-shadows-type",
    "row-reference-to-a-value-namespace",
    // Not produced by `validate`. Reported by the caller when the schema check
    // could not run at all, and listed here so the set of kinds stays in one
    // place. A run that could not check is not a run that passed.
    //
    // NOTE: the per-finding severity map (`table.findings`, parsed at
    // `config.rs:1017`) is a real, working mechanism elsewhere: ordinary lints
    // registered with `mockspace_lint_rules::run_lint` consult it already. No
    // registry check is dispatched that way, though: `run_inner` in
    // `dispatch.rs` calls these functions by hand and prints their findings
    // directly, which is why the map went unconsulted for this whole list.
    // `unknown-config-key` is now the one exception (severity is read at that
    // call site under the lint name `registry-config-keys`). Every other kind
    // here still has no consumer, and two kinds produced by
    // `namespace_root_collisions` are absent from this list entirely with
    // nothing noticing.
    "schema-unavailable",
    // Produced by `config_unknown_keys`, over the config file rather than the
    // row data, which no generated schema covers.
    "unknown-config-key",
];

/// Keys a `[[registry.namespace]]` table may carry. Mirrors `RegistryNamespace`.
///
/// Hand-kept, and constrained rather than trusted: `finding_kinds_are_producible`
/// and `namespace_keys_match_the_struct` in the tests below fail when this drifts
/// from the struct it mirrors. A list nobody checks is a comment with a type, and
/// this file already carries one that says so about itself.
const NAMESPACE_KEYS: &[&str] =
    &["key", "title", "description", "value_field", "render", "group_by", "field"];

/// Keys a `[[registry.namespace.field]]` table may carry. Mirrors `RegistryField`.
const FIELD_KEYS: &[&str] = &["name", "type", "required", "description", "visibility", "values"];

/// Keys a `[ref.roots.<name>]` table may carry. Mirrors `RawRefRoot`.
///
/// Checked for the same reason the two above are, and found the same way: a
/// root-level key written one line too far down the file lands inside whichever
/// table precedes it. `canon_paths` went in under a `[ref.roots.*]` table, was
/// read as a key of that root, and was discarded without a word, so the feature
/// it configures stayed off while the config plainly said otherwise. That cost
/// a debugging cycle to find and would have cost more had the feature been one
/// whose absence is quiet.
pub(crate) const REF_ROOT_KEYS: &[&str] = &["path", "frozen", "links", "label", "internal"];

/// Keys the document **root** may carry. Mirrors `RawConfig`, plus the one
/// section it deliberately does not deserialize.
///
/// This is the half the first version of this check missed, and it is the half
/// the motivating defect was in: `canon_paths` is a root key, so a typo in it
/// at the root was still discarded in silence after `[ref.roots.*]` was covered.
///
/// It also carries the keys the **launcher** reads rather than the engine: the
/// engine pin and `mock_dir`. Those are `mockspace_manifest::ManifestHeader`'s
/// fields, and leaving them out reported every project's own pin as unknown.
/// Caught by running this against a real config rather than against a fixture,
/// which is the only reason it was caught at all.
///
/// `lints` is here because `config.rs` reads it through `toml_edit` directly
/// rather than through `RawConfig`, its values being heterogeneous. It is a
/// real key and belongs in the list; the test below knows it is the exception.
pub(crate) const ROOT_KEYS: &[&str] = &[
    "project_name",
    "crate_prefix",
    "abi_version",
    "src_dirs",
    "proc_macro_crates",
    "lint_proc_macro_source",
    "module_crates",
    "unprefixed_crates",
    "layers",
    "primary_domain_macro",
    "primary_domain_label",
    "install_git_hooks",
    "install_cargo_config",
    "install_agent_files",
    "auto_fmt",
    "auto_clippy_fix",
    "deny_check",
    "domain_kinds",
    "known_macros",
    "agent_macros",
    "macro_styles",
    "crate_colors",
    "crate_grouping",
    "primitive-introductions",
    "canon_paths",
    "panel_consolidate_every",
    "registry",
    "deep_dive_index",
    "ref",
    "ordered_docs",
    "primary_docs",
    "lints",
    // the launcher's, from `mockspace_manifest::ManifestHeader`
    "mock_dir",
    "mockspace_git",
    "mockspace_version",
    "mockspace_rev",
    "mockspace_branch",
    "mockspace_tag",
];

/// Report keys anywhere in the config that deserialize into nothing: at the
/// document root, in `[ref.roots.*]`, and in the namespace declarations.
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

    // The document root. A key here is the case the motivating defect was
    // actually in, and the first version of this check did not look.
    for (k, _) in doc.iter() {
        if !ROOT_KEYS.contains(&k) {
            out.push(RegistryFinding {
                kind:    "unknown-config-key",
                message: format!(
                    "the config declares `{k}` at its root, which mockspace does not read. \
                     It is discarded silently."
                ),
                source:  None,
            });
        }
    }

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
                    });
                }
            }
        }
    }

    let Some(reg) = doc.get("registry").and_then(|i| i.as_table()) else {
        return out;
    };
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
                        });
                    }
                }
            }
        }
    }
    out
}

/// Validate what the generated schemas cannot.
pub fn validate(_namespaces: &[RegistryNamespace], reg: &Registry) -> Vec<RegistryFinding> {
    let mut out = Vec::new();
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
        });
    }
    out
}

/// The outcome of delegating schema validation to a TOML validator.
pub enum SchemaCheck {
    /// The validator ran. Non-empty means it reported problems.
    Ran {
        failures: Vec<String>,
    },
    /// The validator is not installed. Reported rather than treated as a pass,
    /// because a check that silently does not run is worse than no check: it
    /// produces the same green output as a check that ran and found nothing.
    Unavailable,
}

/// Run the generated schemas against the registry data with `taplo`.
///
/// Delegated rather than reimplemented. The schemas already state required
/// fields, identifier patterns, and types, and they are what the editor
/// validates against; a second implementation inside mockspace would be a
/// second definition of one contract, and the two would eventually disagree.
///
/// The cost of delegating is a dependency the gate cannot assume, which is why
/// absence is reported rather than passed over.
pub fn check_schemas(repo_root: &Path, namespaces: &[RegistryNamespace]) -> SchemaCheck {
    if namespaces.is_empty() {
        return SchemaCheck::Ran {
            failures: Vec::new(),
        };
    }
    let probe = std::process::Command::new("taplo")
        .arg("--version")
        .output();
    if probe.is_err() {
        return SchemaCheck::Unavailable;
    }

    let out = std::process::Command::new("taplo")
        .arg("check")
        .current_dir(repo_root)
        .output();

    match out {
        Ok(o) if o.status.success() => {
            // A check that examined nothing succeeded, which is not the same as
            // a check that passed. taplo reports zero files when its `include`
            // does not match, so a wrong pattern reads exactly like a clean
            // registry. Reported rather than trusted.
            let text = String::from_utf8_lossy(&o.stderr);
            if let Some(0) = files_examined(&text) {
                return SchemaCheck::Ran {
                    failures: vec![
                        "taplo examined no files. Check `include` in .taplo.toml: a pattern that matches nothing passes without checking anything."
                            .to_string(),
                    ],
                };
            }
            SchemaCheck::Ran {
                failures: Vec::new(),
            }
        },
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stderr);
            let failures = text
                .lines()
                .filter(|l| l.contains("error") || l.contains("does not match"))
                .map(|l| l.trim().to_string())
                .collect();
            SchemaCheck::Ran {
                failures,
            }
        },
        Err(_) => SchemaCheck::Unavailable,
    }
}

/// How many files taplo reported examining, if it said.
///
/// It logs `found files total=N excluded=M`, and what was checked is the
/// difference. Absent from the output, the count is unknown rather than zero,
/// so a taplo that stops logging it does not start failing every run.
pub(crate) fn files_examined(stderr: &str) -> Option<usize> {
    let line = stderr.lines().find(|l| l.contains("found files total="))?;
    let get = |key: &str| -> Option<usize> {
        let at = line.find(key)? + key.len();
        line[at ..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()
    };
    let total = get("total=")?;
    let excluded = get("excluded=").unwrap_or(0);
    Some(total.saturating_sub(excluded))
}

/// Placeholder-shaped tokens still present in a generated document.
///
/// A safety net rather than a check of any one code path. Documents are
/// generated by several paths (the design document, the per-crate documents,
/// the root passthroughs, the registry pages), and each resolves references
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
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for (raw, _) in crate::registry::placeholder_exprs(line) {
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
pub fn namespace_root_collisions(
    namespaces: &[RegistryNamespace],
    roots: &std::collections::BTreeMap<String, String>,
) -> Vec<RegistryFinding> {
    let mut out = Vec::new();
    for ns in namespaces {
        if roots.contains_key(&ns.key) {
            out.push(RegistryFinding {
                kind: "namespace-root-collision",
                message: format!(
                    "`{}` is both a registry namespace and a citation root, so `{}::x` is ambiguous. Rename one of them.",
                    ns.key, ns.key
                ),
                source: None,
            });
        }
        // The reserved words are the other occupants of slot zero, and they
        // fail worse than a root collision does. A namespace named `crates` is
        // shadowed silently. One named `reg` rewrites `reg::x` into
        // `reg::reg::x` on every pass and never terminates.
        if crate::registry::RESERVED_ROOTS.contains(&ns.key.as_str()) {
            out.push(RegistryFinding {
                kind: "namespace-reserved-name",
                message: format!(
                    "`{}` is a reserved reference root, so a namespace cannot take the name. Rename the namespace.",
                    ns.key
                ),
                source: None,
            });
        }
    }
    out
}
