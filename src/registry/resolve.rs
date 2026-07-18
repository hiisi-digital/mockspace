use super::*;

/// Resolve every `{{ ... }}` reference in a rendered document.
///
/// One pass over one syntax, replacing what were three: row references, field
/// references, whole-table embeds, and file citations are all placeholder
/// expressions distinguished by their shape rather than by their delimiters.
///
/// An expression that resolves to nothing is left exactly as written, braces
/// and all, so the validator reports it and the reader sees an obvious
/// unresolved reference rather than a plausible-looking wrong one.
pub fn resolve_all(
    text: &str,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &crate::config::Config,
) -> String {
    // One pass. Data is resolved bottom-up before any document renders, so a
    // reference inside a row's field is already inlined by the time a table
    // carrying it expands, and re-running over the output would only find
    // references a document wrote about itself.
    resolve_once(text, namespaces, reg, roots, repo_root, docs_dir, cfg)
}

fn resolve_once(
    text: &str,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &crate::config::Config,
) -> String {
    let by_key: BTreeMap<&str, &RegistryNamespace> =
        namespaces.iter().map(|n| (n.key.as_str(), n)).collect();

    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        let mut rewritten = line.to_string();
        for (token, expr) in placeholder_exprs(line) {
            if let Some(rep) =
                resolve_expr(&expr, &by_key, reg, roots, repo_root, docs_dir, cfg)
            {
                rewritten = rewritten.replace(&token, &rep);
            }
        }
        out.push_str(&rewritten);
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Resolve `crates::<name>` to a link to that crate's generated document.
///
/// Accepts the short name or the full directory name, since the prefix is
/// stable and repeating it at every reference buys nothing.
/// The link target for a row: its namespace's document, plus the row's anchor.
///
/// Asks the index what the document is called rather than reconstructing it.
/// Reconstructing it is what put `LAW.md` in a link while the file was written
/// as `902_LAW.md`.
fn doc_target(ns: &RegistryNamespace, row: &RegistryRow, cfg: &crate::config::Config) -> String {
    let file = cfg
        .doc_index
        .registry_doc(&ns.key)
        .map(str::to_string)
        .unwrap_or_else(|| ns.page_name());
    format!("{}{file}#{}", cfg.doc_link_prefix, row.anchor())
}

fn resolve_crate_ref(name: &str, cfg: &crate::config::Config, docs_dir: &Path) -> Option<String> {
    let prefixed = format!("{}-{name}", cfg.crate_prefix);
    let dir = if cfg.crates_dir.join(name).is_dir() {
        name.to_string()
    } else if cfg.crates_dir.join(&prefixed).is_dir() {
        prefixed
    } else {
        return None;
    };
    let short = dir
        .strip_prefix(&format!("{}-", cfg.crate_prefix))
        .unwrap_or(&dir)
        .to_string();
    // Look the name up rather than glob for it. The docs directory is cleaned
    // at the start of a run and refilled during it, so a glob answers "has this
    // been written yet" rather than "what is this crate's document", and a
    // reference from a crate rendered early to one rendered later resolved to
    // nothing at all.
    if let Some(file) = cfg.doc_index.crate_doc(&short) {
        return Some(format!("[{short}]({}{file})", cfg.doc_link_prefix));
    }
    // Fall back to the glob for a caller that has not computed the map, which
    // is every test and any path that resolves before generation.
    let stem = short.to_uppercase().replace('-', "_");
    let file = fs::read_dir(docs_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|f| f.ends_with(&format!("{stem}.md")))?;
    Some(format!("[{short}]({file})"))
}

fn resolve_expr(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &crate::config::Config,
) -> Option<String> {
    let parts: Vec<&str> = expr.split("::").map(str::trim).collect();

    if parts[0] == CRATE_ROOT && parts.len() == 2 {
        // A crate reference. Checked like any other: a crate that does not
        // exist is reported rather than linked into nothing, which is what
        // turns a crate mention in prose from a string into something the
        // build verifies.
        return resolve_crate_ref(parts[1], cfg, docs_dir);
    }

    // A namespace addressed directly: `law::keys` rather than `reg::law::keys`.
    //
    // The `reg::` prefix carried no information. Slot zero is either a declared
    // root or a declared namespace, and the two cannot collide (a collision is
    // reported as a configuration error), so the prefix only made every
    // reference four characters longer and read as ceremony. It still resolves,
    // because thousands of references were written with it.
    //
    // Rewritten into the prefixed form rather than handled separately, so there
    // is one resolution path and the two spellings cannot diverge.
    if by_key.contains_key(parts[0]) && parts.len() >= 2 {
        let rewritten = format!("{}::{}", REGISTRY_ROOT, parts.join("::"));
        return resolve_expr(&rewritten, by_key, reg, roots, repo_root, docs_dir, cfg);
    }

    if parts[0] == REGISTRY_ROOT {
        return match parts.len() {
            // A whole namespace: its table, inline.
            2 => by_key.get(parts[1]).map(|ns| render_table(ns, reg, cfg)),
            // A row, or a field on it.
            3 | 4 if parts[1] == "reference" => {
                let qualified = format!("{}::{}", parts[1], parts[2]);
                let row = reg.get(&qualified)?;
                let ns = by_key.get("reference")?;
                let target = doc_target(ns, row, cfg);

                // `::citation` is a computed field, not one the row declares:
                // the full form, linked. Everything else is a real field.
                if parts.len() == 4 && parts[3] != "citation" {
                    return row.fields.get(parts[3]).cloned();
                }

                // A short linked form by default, the full one on request.
                //
                // Both are hyperlinks, because a citation a reader cannot
                // follow is worth less than one they can, and markdown costs
                // nothing to make it clickable. They differ in width: a
                // sources column carrying four hundred full citations is
                // unreadable, while the same citation in prose wants its
                // title. Default to the form that fits everywhere.
                let text = if parts.len() == 4 {
                    format_citation(row)
                } else {
                    short_citation(row)
                };
                Some(format!("[{text}]({target})"))
            }
            3 | 4 => {
                let qualified = format!("{}::{}", parts[1], parts[2]);
                let row = reg.get(&qualified)?;
                let ns = by_key.get(row.namespace.as_str())?;
                if parts.len() == 4 {
                    if parts[3] == "id" {
                        return Some(row.slug.clone());
                    }
                    // An internal field does not resolve. Returning None leaves
                    // the reference visibly unresolved and reports it, which is
                    // the same treatment a field that does not exist gets. A
                    // field declared as never reaching the documentation, with
                    // a documented way to put it there anyway, would not be
                    // one.
                    if ns
                        .fields
                        .iter()
                        .any(|f| f.name == parts[3] && f.visibility == FieldVisibility::Internal)
                    {
                        return None;
                    }
                    return row.fields.get(parts[3]).cloned();
                }
                Some(match ns.value_field.as_ref().and_then(|f| row.fields.get(f)) {
                    Some(v) => v.clone(),
                    None if !ns.render.has_page() => row.slug.clone(),
                    None => format!("[{}]({})", row.slug, doc_target(ns, row, cfg)),
                })
            }
            _ => None,
        };
    }

    // Otherwise a file citation.
    let r = FileRef::parse(expr)?;

    // A root that does not render as a link becomes prose. The citation still
    // appears, because a generated document is also read internally and the
    // provenance is the point; what does not appear is the path, which is the
    // only part that leaks.
    if let Some(label) = cfg.prose_roots.get(&r.root) {
        return Some(match &r.anchor {
            Anchor::Heading(h) => format!("{label}, {} ({h})", r.path),
            Anchor::Line(n) => format!("{label}, {} line {n}", r.path),
        });
    }

    let rel = roots.get(&r.root)?;
    match resolve_cited_path(&repo_root.join(rel), &r.path) {
        PathResolution::Found(target) => {
            let link = relative_from(docs_dir, &target);
            Some(match &r.anchor {
                Anchor::Heading(h) => format!("[{}/{}#{h}]({}#{h})", r.root, r.path, link),
                Anchor::Line(n) => format!("[{}/{}:{n}]({}#L{n})", r.root, r.path, link),
            })
        }
        _ => None,
    }
}

/// Resolve every reference held inside registry data, bottom-up.
///
/// Data stores templates and never resolved values, so a row's field may cite
/// another row's field. Resolving depth-first from the leaves means a chain of
/// any length settles in one traversal, where re-running a whole-document pass
/// a fixed number of times silently caps how deep a chain may go.
///
/// The graph is acyclic in every sane registry, but nothing enforces that: two
/// rows can reference each other, and such a graph has no leaves to start from.
/// That case is detected and named rather than assumed away, because the
/// alternative is a document that ships with unexplained placeholders and no
/// clue which rows caused it.
pub fn resolve_data(
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &crate::config::Config,
) -> (Registry, Vec<RegistryFinding>) {
    let by_key: BTreeMap<&str, &RegistryNamespace> =
        namespaces.iter().map(|n| (n.key.as_str(), n)).collect();
    let mut done: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut findings = Vec::new();

    for id in reg.rows.keys() {
        let mut path = Vec::new();
        resolve_row(id, reg, &by_key, roots, repo_root, docs_dir, cfg, &mut done, &mut path, &mut findings);
    }

    let mut out = Registry {
        rows: BTreeMap::new(),
        by_namespace: reg.by_namespace.clone(),
        duplicates: reg.duplicates.clone(),
    };
    for (id, row) in &reg.rows {
        let mut r = row.clone();
        if let Some(fields) = done.get(id) {
            r.fields = fields.clone();
        }
        out.rows.insert(id.clone(), r);
    }
    (out, findings)
}

#[allow(clippy::too_many_arguments)]
fn resolve_row(
    id: &str,
    reg: &Registry,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &crate::config::Config,
    done: &mut BTreeMap<String, BTreeMap<String, String>>,
    path: &mut Vec<String>,
    findings: &mut Vec<RegistryFinding>,
) {
    if done.contains_key(id) {
        return;
    }
    if path.iter().any(|p| p == id) {
        // Name the whole cycle, not just the row we happened to re-enter: the
        // fix is to break the loop somewhere, and that needs all of it visible.
        let start = path.iter().position(|p| p == id).unwrap_or(0);
        let mut cycle: Vec<String> = path[start..].to_vec();
        cycle.push(id.to_string());
        findings.push(RegistryFinding {
            kind: "reference-cycle",
            message: format!(
                "these rows reference each other in a loop and cannot be resolved: {}. Break the loop by inlining one side.",
                cycle.join(" -> ")
            ),
            source: reg.get(id).map(|r| r.source.clone()),
        });
        return;
    }
    let ns_keys: BTreeSet<String> = by_key.keys().map(|k| k.to_string()).collect();
    let Some(row) = reg.get(id) else { return };

    // Resolve everything this row depends on first, so by the time its own
    // fields are rewritten the values they cite are final.
    path.push(id.to_string());
    for value in row.fields.values() {
        for (dep, _) in find_registry_refs(value, &ns_keys) {
            resolve_row(&dep, reg, by_key, roots, repo_root, docs_dir, cfg, done, path, findings);
        }
    }
    path.pop();

    // Only the rows this one actually cites need to be settled, so build a view
    // over those rather than cloning the whole registry per row. At the row
    // counts a real registry reaches, the whole-registry clone was quadratic.
    let mut settled = Registry {
        rows: BTreeMap::new(),
        by_namespace: reg.by_namespace.clone(),
        duplicates: BTreeMap::new(),
    };
    for value in row.fields.values() {
        for (dep, _) in find_registry_refs(value, &ns_keys) {
            if settled.rows.contains_key(&dep) {
                continue;
            }
            if let Some(v) = reg.get(&dep) {
                let mut r = v.clone();
                if let Some(f) = done.get(&dep) {
                    r.fields = f.clone();
                }
                settled.rows.insert(dep, r);
            }
        }
    }

    let namespaces: Vec<RegistryNamespace> = by_key.values().map(|n| (*n).clone()).collect();
    let fields = row
        .fields
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                resolve_once(v, &namespaces, &settled, roots, repo_root, docs_dir, cfg),
            )
        })
        .collect();
    done.insert(id.to_string(), fields);
}
