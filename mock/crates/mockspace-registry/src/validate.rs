//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use crate::VerdictCache;

use super::*;

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
    "id-is-a-branch",
    "unfoldable-id",
    "folded-field-written",
    "unfit-path-field",
    "namespace-fold-collision",
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

/// Validate what the generated schemas cannot.
pub fn validate(namespaces: &[RegistryNamespace], reg: &Registry) -> Vec<RegistryFinding> {
    let mut out = unquoted_times(namespaces, reg);
    out.extend(crate::id_branches(reg));
    out.extend(crate::fold::fold_findings(namespaces, reg));
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
pub fn check_schemas(
    repo_root: &Path,
    namespaces: &[RegistryNamespace],
    saved: Option<&dyn VerdictCache>,
) -> SchemaCheck {
    check_schemas_with(std::ffi::OsStr::new("taplo"), repo_root, namespaces, saved)
}

/// [`check_schemas`] with the validator named, so a test can stand a counting one in for taplo.
pub fn check_schemas_with(
    program: &std::ffi::OsStr,
    repo_root: &Path,
    namespaces: &[RegistryNamespace],
    saved: Option<&dyn VerdictCache>,
) -> SchemaCheck {
    if namespaces.is_empty() {
        return SchemaCheck::Ran {
            failures: Vec::new(),
        };
    }
    let Ok(probe) = std::process::Command::new(program)
        .arg("--version")
        .output()
    else {
        return SchemaCheck::Unavailable;
    };

    // The same inputs as the last pass replay it. Only a pass is ever saved below, so a failure,
    // a check that examined nothing and an absent taplo (returned above, before any lookup) are
    // each judged again.
    let mut version = probe.stdout.clone();
    version.extend_from_slice(&probe.stderr);
    let env_changes_taplo = ["TAPLO_CONFIG", "RUST_LOG"].iter().any(|v| std::env::var_os(v).is_some());
    let key_of = |cache: &dyn VerdictCache| {
        super::schema_replay::input_key(cache, repo_root, &version, env_changes_taplo)
    };
    let key = saved
        .filter(|_| probe.status.success())
        .and_then(|cache| key_of(cache).map(|key| (cache, key)));
    if let Some((cache, key)) = &key {
        let hit = cache.replay(key).as_deref() == Some(super::schema_replay::PASSED);
        cache.tally(hit, "registry-schema-check");
        if hit {
            return SchemaCheck::Ran {
                failures: Vec::new(),
            };
        }
    }

    let out = std::process::Command::new(program)
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
            // Saved only if the inputs are still what the check began with: a file edited while
            // taplo ran must not get this pass saved under its old content.
            if let Some((cache, key)) = &key {
                if key_of(*cache).as_deref() == Some(key.as_str()) {
                    cache.remember(key, super::schema_replay::PASSED);
                }
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
                .collect::<Vec<String>>();
            // A run that failed is a failure whatever it managed to log: its logger can lose the last
            // lines when the process exits, and no lines is not the same as no problem.
            let failures = if failures.is_empty() {
                vec!["taplo failed and logged no line that names a problem.".to_string()]
            } else {
                failures
            };
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
pub fn files_examined(stderr: &str) -> Option<usize> {
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
    // addressed `<namespace>::<id>` with no part of its path in the address.
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
