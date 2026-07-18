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
        let mut dropped_any = false;
        for (token, expr) in placeholder_exprs(line) {
            if let Some(rep) =
                resolve_expr(&expr, &by_key, reg, roots, repo_root, docs_dir, cfg)
            {
                if rep.is_empty() {
                    dropped_any = true;
                }
                rewritten = rewritten.replace(&token, &rep);
            }
        }
        if dropped_any {
            rewritten = tidy_after_drop(&rewritten);
        }
        out.push_str(&rewritten);
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Clean up what dropping a reference left behind.
///
/// A citation is usually the whole of a parenthetical, or the tail of one. When
/// it renders as nothing, the punctuation that framed it is still there, and a
/// document reading "the split becomes structural ()." is worse than one that
/// never carried the citation. Only lines that actually dropped something are
/// touched, so ordinary prose containing empty parentheses is left alone.
fn tidy_after_drop(line: &str) -> String {
    let mut s = line.to_string();
    // Collapse first, so a parenthetical left holding only the whitespace
    // between several dropped citations reads as empty.
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s = regex_lite_replace(&s, "( )", "()");
    // A parenthetical emptied entirely, including one holding only separators
    // left by several dropped citations.
    s = regex_lite_replace(&s, " ()", "");
    s = regex_lite_replace(&s, "()", "");
    // Separators orphaned by a drop inside a parenthetical that kept something.
    s = regex_lite_replace(&s, "(, ", "(");
    s = regex_lite_replace(&s, ", )", ")");
    s = regex_lite_replace(&s, " ,", ",");
    // A drop that was not inside parentheses leaves the space before the
    // sentence's own punctuation.
    for mark in [".", ";", ":"] {
        s = regex_lite_replace(&s, &format!(" {mark}"), mark);
    }
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s.trim_end().to_string()
}

/// Replace every occurrence, without a regex dependency.
fn regex_lite_replace(s: &str, from: &str, to: &str) -> String {
    let mut out = s.to_string();
    while out.contains(from) {
        out = out.replace(from, to);
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

/// Split `<expr>.method().method()` into the expression and its methods.
///
/// Returns `None` when there is no chain, which is the common case, so the
/// ordinary path is untouched. The scan respects parentheses, since the
/// expression itself may be a call: `pathof(crates::store).dir()` splits after
/// the call rather than at the first dot inside it.
fn split_methods(expr: &str) -> Option<(String, Vec<String>)> {
    let bytes = expr.as_bytes();
    let mut depth = 0usize;
    let mut split_at = None;
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'.' if depth == 0 => {
                // A dot at depth zero starts the chain, but only if what
                // follows looks like a call. Anything else is prose or a
                // filename and is left alone.
                if expr[i + 1..].contains("()") {
                    split_at = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let at = split_at?;
    let base = expr[..at].trim().to_string();
    if base.is_empty() {
        return None;
    }
    let methods: Vec<String> = expr[at + 1..]
        .split("()")
        .map(|m| m.trim_matches('.').trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    if methods.is_empty() {
        return None;
    }
    Some((base, methods))
}

/// Narrow a resolved value.
///
/// Four accessors read a path and three read a list, because those are the two
/// shapes an expression resolves to: `pathof` yields a path, `sourcesof` yields
/// a list, and a field yields whatever the row holds.
///
/// An unknown method is an error rather than a pass-through: a silently ignored
/// method reads as working, and the reference syntax's whole contract is that a
/// reference pointing nowhere is reported.
fn apply_methods(value: &str, methods: &[String]) -> Option<String> {
    let mut v = value.to_string();
    for m in methods {
        v = match m.as_str() {
            "dir" => v.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default(),
            "filename" => v.rsplit_once('/').map(|(_, f)| f.to_string()).unwrap_or(v),
            "stem" => {
                let f = v.rsplit_once('/').map(|(_, f)| f.to_string()).unwrap_or(v);
                f.rsplit_once('.').map(|(st, _)| st.to_string()).unwrap_or(f)
            }
            "ext" => v.rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or_default(),
            "first" => v.split(", ").next().unwrap_or("").to_string(),
            "last" => v.split(", ").last().unwrap_or("").to_string(),
            "count" => v.split(", ").filter(|s| !s.trim().is_empty()).count().to_string(),
            _ => return None,
        };
    }
    Some(v)
}

/// Where the named thing is declared: the file to open to change it.
fn resolve_pathof(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    repo_root: &Path,
    cfg: &crate::config::Config,
) -> Option<String> {
    let parts: Vec<&str> = expr.split("::").map(str::trim).collect();

    if parts[0] == CRATE_ROOT && parts.len() == 2 {
        let prefixed = format!("{}-{}", cfg.crate_prefix, parts[1]);
        for name in [parts[1].to_string(), prefixed] {
            let dir = cfg.crates_dir.join(&name);
            if dir.is_dir() {
                return Some(rel_to_repo(&dir, repo_root));
            }
        }
        return None;
    }

    let mut segs = parts.clone();
    if segs.first() == Some(&REGISTRY_ROOT) {
        segs.remove(0);
    }
    if segs.len() == 2 && by_key.contains_key(segs[0]) {
        let row = reg.get(&format!("{}::{}", segs[0], segs[1]))?;
        return Some(rel_to_repo(&row.source, repo_root));
    }

    // A citation names a file, and that file is where the thing is.
    let r = FileRef::parse(expr)?;
    if cfg.internal_roots.contains(&r.root) {
        return Some(String::new());
    }
    let rel = cfg.registry_roots.get(&r.root)?;
    match resolve_cited_path(&repo_root.join(rel), &r.path) {
        PathResolution::Found(target) => Some(rel_to_repo(&target, repo_root)),
        _ => None,
    }
}

/// What the named thing rests on: its provenance, resolved as citations.
///
/// Each entry goes through ordinary citation resolution, so an internal root
/// drops here too rather than leaking through a second door.
fn resolve_sourcesof(
    expr: &str,
    by_key: &BTreeMap<&str, &RegistryNamespace>,
    reg: &Registry,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
    cfg: &crate::config::Config,
) -> Option<String> {
    let mut segs: Vec<&str> = expr.split("::").map(str::trim).collect();
    if segs.first() == Some(&REGISTRY_ROOT) {
        segs.remove(0);
    }
    if segs.len() != 2 || !by_key.contains_key(segs[0]) {
        return None;
    }
    let row = reg.get(&format!("{}::{}", segs[0], segs[1]))?;
    let raw = row.fields.get("provenance")?;
    let rendered: Vec<String> = raw
        .split(", ")
        .filter_map(|c| resolve_expr(c.trim(), by_key, reg, roots, repo_root, docs_dir, cfg))
        .filter(|r| !r.is_empty())
        .collect();
    Some(rendered.join(", "))
}

/// A path as the repository sees it, which is how a document should name one.
fn rel_to_repo(path: &Path, repo_root: &Path) -> String {
    path.strip_prefix(repo_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
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
    // A postfix chain narrows whatever the expression resolved to:
    // `pathof(crates::store).dir()`. Split off first, so the base expression is
    // resolved by the ordinary path and the methods only ever see a string.
    if let Some((base, methods)) = split_methods(expr) {
        let value = resolve_expr(&base, by_key, reg, roots, repo_root, docs_dir, cfg)?;
        return apply_methods(&value, &methods);
    }

    // Two questions that are not the same one.
    //
    // `pathof(x)` is where x is DECLARED: the file to open to change it. For a
    // registry row that is the TOML it sits in, which is what a rule telling an
    // agent where to edit actually needs.
    //
    // `sourcesof(x)` is what x RESTS ON: its provenance. Plural, because
    // provenance is an array and a claim usually has several sources.
    if let Some(inner) = expr.strip_prefix("pathof(").and_then(|r| r.strip_suffix(')')) {
        return resolve_pathof(inner.trim(), by_key, reg, repo_root, cfg);
    }
    if let Some(inner) = expr.strip_prefix("sourcesof(").and_then(|r| r.strip_suffix(')')) {
        return resolve_sourcesof(inner.trim(), by_key, reg, roots, repo_root, docs_dir, cfg);
    }

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
    // A namespace alone renders its whole table: `{{ task }}`. Placeholders
    // have already been substituted by the time this runs, so a single segment
    // that names a namespace can only mean the namespace.
    if parts.len() == 1 {
        return by_key.get(parts[0]).map(|ns| render_table(ns, reg, cfg));
    }

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

    // A citation into an internal root renders as nothing. The reference stays
    // in the source, where it is checked and greppable and is the provenance
    // record; it does not reach a reader who cannot open what it names.
    //
    // Empty rather than absent, so the parenthetical it may sit in is left for
    // `resolve_all` to tidy: a citation that was the only thing in its
    // parentheses would otherwise leave `()` behind.
    if cfg.internal_roots.contains(&r.root) {
        return Some(String::new());
    }

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
