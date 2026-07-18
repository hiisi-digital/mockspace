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
) -> Vec<RegistryFinding> {
    let mut out = Vec::new();

    for row in reg.rows.values() {
        let Some(raw) = row.fields.get("provenance") else {
            continue;
        };
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
                        "{}: `{}` names root `{}`, which is not declared in [registry.roots]. Declared roots are what make a reference resolvable rather than a convention.",
                        row.qualified(),
                        p.render(),
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
                        kind: "unresolvable-provenance",
                        message: format!(
                            "{}: `{}` matches no file under root `{}`.",
                            row.qualified(),
                            p.render(),
                            p.root
                        ),
                        source: Some(row.source.clone()),
                    });
                    continue;
                }
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
                }
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
                Some(_) => {}
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
    pub kind: &'static str,
    pub message: String,
    pub source: Option<PathBuf>,
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
];

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

/// Findings for references made by a rendered document to rows that do not
/// exist. Separate from `validate` because it needs the document set, which
/// only the render pass has.
pub fn validate_references(text: &str, reg: &Registry, origin: &Path) -> Vec<RegistryFinding> {
    dangling_references(text, reg)
        .into_iter()
        .map(|id| RegistryFinding {
            kind: "dangling-reference",
            message: format!(
                "{id} is referenced but no row declares it. A reference that resolves to nothing is worse than prose, because it looks checked."
            ),
            source: Some(origin.to_path_buf()),
        })
        .collect()
}

/// The outcome of delegating schema validation to a TOML validator.
pub enum SchemaCheck {
    /// The validator ran. Non-empty means it reported problems.
    Ran { failures: Vec<String> },
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
        return SchemaCheck::Ran { failures: Vec::new() };
    }
    let probe = std::process::Command::new("taplo").arg("--version").output();
    if probe.is_err() {
        return SchemaCheck::Unavailable;
    }

    let out = std::process::Command::new("taplo")
        .arg("check")
        .current_dir(repo_root)
        .output();

    match out {
        Ok(o) if o.status.success() => SchemaCheck::Ran { failures: Vec::new() },
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stderr);
            let failures = text
                .lines()
                .filter(|l| l.contains("error") || l.contains("does not match"))
                .map(|l| l.trim().to_string())
                .collect();
            SchemaCheck::Ran { failures }
        }
        Err(_) => SchemaCheck::Unavailable,
    }
}
