//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The provenance a row states, checked against the roots and the files it cites.

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
    let _scope = crate::refpath::cache_scope();
    let mut out = Vec::new();
    // Which fields hold references is read off each namespace's declared field
    // TYPES. Keying on the literal name `provenance` validated one field for one
    // consumer, and silently ignored every other reference-bearing field a
    // project declared. The design says provenance is deliberately not
    // universal; the hardcoded name made it universal anyway.
    let bearing = super::reference_fields(namespaces);
    let keys = super::namespace_keys(namespaces);

    for row in reg.rows.values() {
        let empty = Vec::new();
        let names = bearing.get(&row.namespace).unwrap_or(&empty);
        for raw in names.iter().filter_map(|n| row.fields.get(n)) {
            for item in crate::refpath::list(raw) {
                // A link to a row shares the field with the file citations and
                // is checked by `validate_links`, which resolves it against the
                // rows rather than against a root.
                if super::link_target(item, &keys).is_some() {
                    continue;
                }
                let p = match FileRef::read(item) {
                    Ok(p) => p,
                    Err(why) => {
                        out.push(RegistryFinding {
                            kind:    "malformed-provenance",
                            message: format!(
                                "{}: {} Unparsed, it points nowhere while looking like a citation.",
                                row.qualified(),
                                why.say(item)
                            ),
                            source:  Some(row.source.clone()),
                            gate:    GateScope::Always,
                        });
                        continue;
                    },
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
                    gate:   GateScope::Always,
                });
                    continue;
                };
                let target = match p.locate(&repo_root.join(root_rel)) {
                    Location::Found(t) => t,
                    Location::Missing => {
                        out.push(RegistryFinding {
                            kind:    "unresolvable-provenance",
                            message: format!(
                                "{}: `{}` matches no file under root `{}`.",
                                row.qualified(),
                                p.render(),
                                p.root
                            ),
                            source:  Some(row.source.clone()),
                            gate:    GateScope::Always,
                        });
                        continue;
                    },
                    Location::Ambiguous(names) => {
                        out.push(RegistryFinding {
                        kind: "ambiguous-provenance",
                        message: format!(
                            "{}: `{}` matches several files ({}). No spelling tells them apart, since extensions are never written: rename or remove all but one, because picking one silently would point the citation somewhere you did not choose.",
                            row.qualified(),
                            p.render(),
                            names.join(", ")
                        ),
                        source: Some(row.source.clone()),
                        gate:   GateScope::Always,
                    });
                        continue;
                    },
                };
                if matches!(p.anchor(), Anchor::Lines(..)) && !frozen.contains(&p.root) {
                    out.push(RegistryFinding {
                    kind: "fragile-line-citation",
                    message: format!(
                        "{}: `{}` cites a line in root `{}`, which is not declared frozen. Any edit above that line silently repoints the citation while it still resolves. Cite a heading, or declare the root frozen if its files genuinely do not move.",
                        row.qualified(),
                        p.render(),
                        p.root
                    ),
                    source: Some(row.source.clone()),
                    gate:   GateScope::Always,
                });
                }
                match resolve_anchor(&target, &p.anchor()) {
                    Some(_) => {},
                    None => {
                        out.push(RegistryFinding {
                        kind: match p.anchor() {
                            Anchor::Heading(_) => "unresolvable-heading",
                            Anchor::Lines(..) => "unresolvable-provenance",
                        },
                        message: match p.anchor() {
                            Anchor::Heading(h) => format!(
                                "{}: `{}` names heading `{h}`, which {} does not contain. A renamed heading fails here rather than silently pointing elsewhere, which is why headings are preferred over line numbers.",
                                row.qualified(),
                                p.render(),
                                p.path_text()
                            ),
                            Anchor::Lines(n, _) => format!(
                                "{}: `{}` points past the end of {} ({n} requested). Line citations only stay true in a root that does not change; declare the root frozen or cite a heading.",
                                row.qualified(),
                                p.render(),
                                p.path_text()
                            ),
                        },
                        source: Some(row.source.clone()),
                        gate:   GateScope::Always,
                    });
                    },
                }
            }
        }
    }
    out
}

