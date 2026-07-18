//! A registry of terms and concepts, resolved at doc-generation time.
//!
//! A project accumulates identifiers for the things its documents refer to:
//! probes, measurements, constants, invariants, vocabulary. Without somewhere
//! to hold them, each document family invents its own convention and the
//! conventions drift. Worse, nothing can answer "what are all the X", and a
//! gap is only visible against an enumeration.
//!
//! Data lives under `<mock>/registry/`, arbitrarily nested, every `*.toml`
//! beneath it loaded. **A file's namespace is the array-of-tables key it
//! declares, never its path.** That is what makes the nesting free: a project
//! may file by subject (`registry/water.toml` holding `[[spike]]`, `[[bench]]`,
//! and `[[constant]]` rows) and still query by kind.
//!
//! # Identity
//!
//! A row is identified by its namespace and a slug: `vocab::xpbd`,
//! `spike::actuator_fit_converges`. Slugs are snake_case and unique within
//! their namespace.
//!
//! Slugs rather than numbers because a number carries no meaning, and an
//! identifier carrying no meaning has to be *managed*: never reused, never
//! renumbered, never reordered, since any of those silently repoints every
//! reference to it. A slug needs none of that discipline. It says what it
//! refers to, it survives reordering, and it stays readable in prose.
//!
//! # References
//!
//! One syntax, `root::selector...`, covers both kinds:
//!
//! - `reg::vocab::xpbd` selects a row; `reg::vocab::xpbd::what` selects a field.
//! - `seed::DESIGN::844` selects a line in a file, under a declared root.
//!
//! Both resolve at generation and both are checked, so a reference pointing
//! nowhere is reported rather than rendered as something that looks fine.
//!
//! Designed for v2, prototyped here in v1. See
//! `docs/REFERENCE-SYNTAX.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The reserved root naming the registry itself rather than a file tree.
pub const REGISTRY_ROOT: &str = "reg";

/// The reserved root naming a crate in this workspace.
///
/// `crates::mechanism` refers to a crate and resolves to a link to its
/// generated document. Distinct from a file citation, which needs a line and
/// therefore at least three segments.
///
/// The project's crate prefix is stable, so both the short name and the full
/// directory name resolve: `crates::mechanism` and
/// `crates::ikiuni-renderer-mechanism` are the same crate. Writing the short
/// form everywhere keeps a rename of the prefix from touching every reference.
pub const CRATE_ROOT: &str = "crates";

/// One field a namespace declares beyond the universal `id`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RegistryField {
    pub name: String,
    /// `string`, `integer`, `boolean`, `string[]`. Anything richer belongs in
    /// a hand-written schema fragment rather than in a config language.
    #[serde(default = "default_field_type")]
    pub r#type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
}

fn default_field_type() -> String {
    "string".to_string()
}

/// Where a namespace's table appears in the generated documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RenderMode {
    /// A generated page per namespace. Row references link to it.
    #[default]
    Page,
    /// No standalone page. The project embeds the table where it wants with
    /// `{{registry:<key>}}`, and row references render as plain text, because
    /// a link needs a target.
    Embed,
}

impl RenderMode {
    pub fn has_page(self) -> bool {
        matches!(self, RenderMode::Page)
    }
}

/// A declared namespace: one kind of thing the registry holds.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RegistryNamespace {
    /// The array-of-tables key, singular: `spike` for `[[spike]]`. Also the
    /// first selector segment of every reference into it.
    pub key: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// When set, a bare row reference renders this field's value instead of a
    /// link, so a constant is stated once and every mention stays current.
    #[serde(default)]
    pub value_field: Option<String>,
    #[serde(default)]
    pub render: RenderMode,
    /// Render the table in sections, one per distinct value of this field.
    ///
    /// A flat table of every row is the wrong shape for a namespace whose rows
    /// belong to named groups: tasks under milestones, vocabulary under the
    /// closed set it belongs to. Grouping is derived from the data rather than
    /// maintained as a second list, so a row moves between sections by editing
    /// the field that says where it belongs.
    #[serde(default)]
    pub group_by: Option<String>,
    #[serde(default, rename = "field")]
    pub fields: Vec<RegistryField>,
}

impl RegistryNamespace {
    pub fn title(&self) -> String {
        self.title.clone().unwrap_or_else(|| self.key.clone())
    }

    /// The generated page's filename, matching the uppercase convention every
    /// other generated document here follows.
    ///
    /// No prefix: a namespace's name is already the document's subject, and
    /// `VOCAB.md` reads better than `REGISTRY-VOCAB.md` beside `DESIGN.md` and
    /// `CATALOGUE.md`. The prefix was implicitly preventing a namespace from
    /// colliding with a hand-authored document, which `render_pages` now checks
    /// for directly. Padding every name to avoid a collision hides it; checking
    /// reports it.
    pub fn page_name(&self) -> String {
        format!("{}.md", self.key.to_uppercase().replace('_', "-"))
    }
}

/// Whether a slug is well formed: snake_case, starting with a letter.
///
/// Constrained so a slug reads identically everywhere it appears: in the data,
/// in a reference inside prose, and in a generated anchor. A slug allowed to
/// carry case or punctuation would need normalising at each of those, and the
/// three normalisations would eventually disagree.
pub fn is_valid_slug(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// One row.
#[derive(Debug, Clone)]
pub struct RegistryRow {
    /// The slug, unique within its namespace.
    pub slug: String,
    pub namespace: String,
    /// Where it was declared, so an error can point at a file.
    pub source: PathBuf,
    pub fields: BTreeMap<String, String>,
}

impl RegistryRow {
    /// `namespace::slug`, the form a reference selects and a page anchors.
    pub fn qualified(&self) -> String {
        format!("{}::{}", self.namespace, self.slug)
    }

    /// The anchor for this row on its namespace page.
    pub fn anchor(&self) -> String {
        self.slug.replace('_', "-")
    }
}

/// Every row the project declares, indexed for lookup.
#[derive(Debug, Default)]
pub struct Registry {
    /// Keyed by `namespace::slug`.
    pub rows: BTreeMap<String, RegistryRow>,
    pub by_namespace: BTreeMap<String, Vec<String>>,
    /// Slugs declared twice within one namespace, with every declaring file.
    /// An error rather than a warning: two rows for one identifier means a
    /// reference cannot be resolved. No per-file schema can catch it, because
    /// each file is valid on its own.
    pub duplicates: BTreeMap<String, Vec<PathBuf>>,
}

impl Registry {
    pub fn get(&self, qualified: &str) -> Option<&RegistryRow> {
        self.rows.get(qualified)
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

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
        toml_edit::Item::Value(toml_edit::Value::Array(a)) => a
            .iter()
            .map(|e| {
                e.as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| e.to_string().trim().to_string())
            })
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string().trim().to_string(),
    }
}

/// The `vocab` namespace, provided without the project declaring it.
///
/// Every project accumulates small closed sets: the handful of modes, tiers,
/// phases, or roles its documents name. Each is too small to earn a namespace
/// of its own, but collectively they are what a reader needs enumerated.
///
/// Its contract is deliberately looser. Only `name` and `what` are required:
/// `kind` groups rows when a project has several closed sets, and provenance is
/// optional because a small closed set is usually defined by the design itself
/// rather than sourced from somewhere.
///
/// A project wanting different fields declares `key = "vocab"` itself and its
/// declaration wins, so this is a default rather than a restriction.
pub fn builtin_vocab() -> RegistryNamespace {
    let f = |name: &str, required: bool, description: &str| RegistryField {
        name: name.to_string(),
        r#type: "string".to_string(),
        required,
        description: Some(description.to_string()),
    };
    RegistryNamespace {
        key: "vocab".to_string(),
        title: Some("Vocabulary".to_string()),
        description: Some(
            "Small closed sets the project names: the modes, tiers, roles, or phases too few in number to earn a namespace each, and exactly what a reader wants enumerated."
                .to_string(),
        ),
        value_field: None,
        render: RenderMode::Page,
        // Vocab is the case that motivates grouping: its rows belong to
        // several unrelated closed sets and a flat table mixes them.
        group_by: Some("kind".to_string()),
        fields: vec![
            f("kind", false, "Which closed set this belongs to. Omit when the project has only one."),
            f("name", true, "The term."),
            f("what", true, "What it is, in one line."),
            f("note", false, "A distinction worth preserving that the one-liner flattens away."),
            RegistryField {
                name: "provenance".to_string(),
                r#type: "string[]".to_string(),
                required: false,
                description: Some(
                    "Where the term is defined, when it comes from somewhere rather than being defined by the design itself."
                        .to_string(),
                ),
            },
        ],
    }
}

/// The `reference` namespace, provided without the project declaring it.
///
/// Every project rests on work it did not write: papers, talks, specifications,
/// books. Holding them as rows rather than as citation strings scattered
/// through prose means one work cited in twenty places renders identically in
/// all twenty, and "what does this design rest on" becomes a question with an
/// answer rather than a grep.
pub fn builtin_reference() -> RegistryNamespace {
    let f = |name: &str, required: bool, description: &str| RegistryField {
        name: name.to_string(),
        r#type: "string".to_string(),
        required,
        description: Some(description.to_string()),
    };
    RegistryNamespace {
        key: "reference".to_string(),
        title: Some("External references".to_string()),
        description: Some(
            "Work this project rests on: papers, talks, specifications, books. One row per work, cited by however many things use it."
                .to_string(),
        ),
        value_field: None,
        render: RenderMode::Page,
        group_by: Some("kind".to_string()),
        fields: vec![
            f("title", true, "The work's own title, as published."),
            f("authors", false, "Authors as published. Omitted where the venue is the author."),
            f("venue", false, "Journal, conference, publisher, or standards body."),
            f("year", false, "Year of publication."),
            f("kind", false, "paper, talk, specification, book, article, or thesis."),
            f("url", false, "Where it can be reached, when it has a stable address."),
            f("note", false, "Which part of the work is adopted, or which part deliberately is not."),
        ],
    }
}

/// Namespaces every project gets, and which therefore earn a root of their own.
///
/// A builtin namespace is addressed directly (`vocab::xpbd`) rather than
/// through the registry root, because it exists in every project and the short
/// form is safe everywhere. A project's own namespace stays behind `reg::`, so
/// a reference reads as what it is: a lookup into this project's tables rather
/// than into vocabulary every project shares.
pub const BUILTIN_NAMESPACES: &[&str] = &["vocab", "reference"];

/// The project's namespaces plus any builtin it did not override.
pub fn with_builtins(declared: &[RegistryNamespace]) -> Vec<RegistryNamespace> {
    let mut out = declared.to_vec();
    // Prepended, not appended. Declaration order is reading order, and these
    // are what the other tables assume you have already read.
    if !out.iter().any(|n| n.key == "reference") {
        out.insert(0, builtin_reference());
    }
    if !out.iter().any(|n| n.key == "vocab") {
        out.insert(0, builtin_vocab());
    }
    out
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
                    eprintln!("  registry: a [[{key}]] row in {} has no id, skipped", path.display());
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
                reg.by_namespace.entry(key.to_string()).or_default().push(qualified.clone());
                reg.rows.insert(qualified, row);
            }
        }
    }
    reg
}

/// Strip the placeholder delimiters from a line, yielding each expression.
///
/// References in prose are written `{{ reg::vocab::xpbd }}`, in the same
/// placeholder vocabulary the rest of the template system uses. The braces are
/// not ceremony: they make a reference something the author states rather than
/// something the renderer guesses at from a pattern. Without them a project
/// with a root named `core` would silently link `core::mem::12`, and prose
/// about code would be rewritten by accident.
///
/// One syntax then covers everything: `{{ reg::ns::slug }}` for a row,
/// `{{ reg::ns::slug::field }}` for a field, `{{ reg::ns }}` for a whole
/// table, and `{{ root::path::line }}` for a file citation.
///
/// Provenance fields in the data need no braces: the field is already declared
/// to hold references, so there is nothing to disambiguate.
pub fn placeholder_exprs(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(s) = rest.find("{{") {
        let after = &rest[s + 2..];
        let Some(e) = after.find("}}") else { break };
        let inner = after[..e].trim().to_string();
        if !inner.is_empty() {
            out.push((format!("{{{{{}}}}}", &after[..e]), inner));
        }
        rest = &after[e + 2..];
    }
    out
}

/// Every `reg::` reference in `text`, as `(qualified, field)` pairs.
///
/// Explicit rather than scanned. An earlier form auto-linked any token that
/// looked like an identifier, which meant guessing whether a token was a
/// reference at all. Requiring `reg::` makes a reference something the author
/// states.
pub fn find_registry_refs(text: &str) -> Vec<(String, Option<String>)> {
    let prefix = format!("{REGISTRY_ROOT}::");
    let mut found = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Only inside a placeholder. A bare `reg::ns::slug` in prose is not a
        // reference the renderer touches, so reporting it as dangling would
        // flag documentation that merely describes the syntax. The document
        // defining this syntax was the first thing to trip it.
        for (_, expr) in placeholder_exprs(line) {
            let line = expr.as_str();
        let mut rest = line;
        while let Some(pos) = rest.find(&prefix) {
            let before_ok = pos == 0
                || !rest[..pos]
                    .chars()
                    .next_back()
                    .map(|c| c.is_alphanumeric() || c == '_' || c == ':')
                    .unwrap_or(false);
            let after = &rest[pos + prefix.len()..];
            if before_ok {
                let seg = |s: &str| -> String {
                    s.chars().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_').collect()
                };
                let ns = seg(after);
                if !ns.is_empty() {
                    let t1 = &after[ns.len()..];
                    if let Some(t1) = t1.strip_prefix("::") {
                        let slug = seg(t1);
                        if !slug.is_empty() {
                            let t2 = &t1[slug.len()..];
                            let field = t2.strip_prefix("::").map(seg).filter(|f| !f.is_empty());
                            found.push((format!("{ns}::{slug}"), field));
                        }
                    }
                }
            }
            rest = &rest[pos + prefix.len()..];
        }
        }
    }
    found
}

/// References naming a row that does not exist, or a field that row does not
/// carry. Both are silent without a check: the first renders as nothing
/// useful, the second renders as empty.
pub fn dangling_references(text: &str, reg: &Registry) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (qualified, field) in find_registry_refs(text) {
        match reg.get(&qualified) {
            None => {
                out.insert(format!("{REGISTRY_ROOT}::{qualified}"));
            }
            Some(row) => {
                if let Some(f) = &field {
                    if f != "id" && !row.fields.contains_key(f) {
                        out.insert(format!("{REGISTRY_ROOT}::{qualified}::{f}"));
                    }
                }
            }
        }
    }
    out
}

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
    let mut line_counts: BTreeMap<PathBuf, usize> = BTreeMap::new();

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


/// Generate a JSON Schema per namespace, plus the tool configuration binding
/// them, into `<mock>/registry/.schemas/`.
///
/// These are generated rather than authored for a concrete reason. Hand-writing
/// them means sharing common definitions across files, and the TOML language
/// server cannot resolve cross-file references, so the shared block ends up
/// duplicated into every schema. That is the second-copy drift the discipline
/// forbids everywhere else. Generating makes the duplication free and unable to
/// drift, and the descriptions the project already wrote become editor hover
/// documentation at no extra cost.
pub fn generate_schemas(
    repo_root: &Path,
    mock_dir: &Path,
    namespaces: &[RegistryNamespace],
) -> usize {
    if namespaces.is_empty() {
        return 0;
    }
    // Schemas are derived from the namespace declarations, so they are build
    // output rather than source. They live under `target/` with the proxy and
    // the hooks, and never beside the registry data they validate: a generated
    // file sitting in a source tree invites hand-edits that the next
    // regeneration silently discards.
    let dir = repo_root.join("target").join("mockspace").join("registry-schemas");
    if fs::create_dir_all(&dir).is_err() {
        return 0;
    }

    let mut written = 0;
    // One union schema with every namespace as an optional root property,
    // rather than one schema per namespace bound by key.
    //
    // Taplo's `keys` binding scopes a schema to the value at that key, so a
    // per-namespace schema describing a document root never matched and
    // nothing was validated. The union shape fixes that and is better anyway:
    // a single rule covers every registry file, and which part of the schema
    // applies is still decided by the array-of-tables key a file declares,
    // which is what keeps the directory layout free-form.
    let mut ns_props: Vec<String> = Vec::new();
    for ns in namespaces {
        let mut props = String::new();
        props.push_str(&format!(
            "        \"id\": {{\n          \"type\": \"string\",\n          \"pattern\": \"^[a-z][a-z0-9_]*$\",\n          \"description\": \"The slug: snake_case, unique within this namespace. A slug says what it refers to, so it survives reordering and stays readable in prose, which a number does not.\"\n        }}"
        ));
        let mut required = vec!["id".to_string()];

        for f in &ns.fields {
            let ty = match f.r#type.as_str() {
                "integer" => "\"type\": \"integer\"".to_string(),
                "boolean" => "\"type\": \"boolean\"".to_string(),
                "string[]" => {
                    "\"type\": \"array\", \"items\": { \"type\": \"string\" }".to_string()
                }
                _ => "\"type\": \"string\"".to_string(),
            };
            let desc = f
                .description
                .as_ref()
                .map(|d| format!(",\n          \"description\": {}", json_string(d)))
                .unwrap_or_default();
            props.push_str(&format!(
                ",\n        {}: {{\n          {ty}{desc}\n        }}",
                json_string(&f.name)
            ));
            if f.required {
                required.push(f.name.clone());
            }
        }

        let req = required
            .iter()
            .map(|r| json_string(r))
            .collect::<Vec<_>>()
            .join(", ");
        let title = json_string(&ns.title());
        let desc = json_string(ns.description.as_deref().unwrap_or(""));

        ns_props.push(format!(
            "    {}: {{\n      \"title\": {title},\n      \"description\": {desc},\n      \"type\": \"array\",\n      \"items\": {{\n        \"type\": \"object\",\n        \"properties\": {{\n{props}\n        }},\n        \"required\": [{req}],\n        \"additionalProperties\": false\n      }}\n    }}",
            json_string(&ns.key)
        ));
    }

    let schema = format!(
        "{{\n  \"$schema\": \"https://json-schema.org/draft/2020-12/schema\",\n  \"title\": \"Registry\",\n  \"description\": \"Generated from [[registry.namespace]] in mockspace.toml. Every namespace is an optional root property, so a file validates against whichever it declares.\",\n  \"type\": \"object\",\n  \"properties\": {{\n{}\n  }},\n  \"additionalProperties\": false\n}}\n",
        ns_props.join(",\n")
    );
    if write_if_changed(&dir.join("registry.schema.json"), &schema) {
        written += 1;
    }

    // The editor binding. This one lives at the repository root because that
    // is where the TOML language server looks for it, and it points into
    // `target/` where the schemas now are. Selection is by the
    // array-of-tables key a file declares, so every registry file is offered
    // every schema and the matching one applies. That is what keeps the
    // directory layout free-form: no path convention is load-bearing.
    let mock_rel = mock_dir
        .strip_prefix(repo_root)
        .unwrap_or(mock_dir)
        .to_string_lossy()
        .replace('\\', "/");
    let mut taplo = String::from(
        "# Generated by mockspace. Do not edit; change [[registry.namespace]]\n# in mockspace.toml and regenerate.\n#\n# Schemas are build output and live under target/. This file lives at the\n# repository root because that is where the TOML language server looks.\n#\n# Schemas bind by the array-of-tables key a file declares, never by its\n# path, so registry files may be nested and organised however the project\n# likes, including by subject rather than by kind.\n\n",
    );
    taplo.push_str(&format!(
        "[[rule]]\ninclude = [\"{mock_rel}/registry/**/*.toml\"]\nschema.path = \"target/mockspace/registry-schemas/registry.schema.json\"\n"
    ));
    if write_if_changed(&repo_root.join(".taplo.toml"), &taplo) {
        written += 1;
    }

    // Sweep schemas for namespaces the project no longer declares. Without
    // this, renaming or removing a namespace strands its schema, and a stale
    // schema is worse than a missing one: an editor keeps validating rows
    // against a contract nobody maintains.
    let active: std::collections::BTreeSet<String> =
        ["registry.schema.json".to_string()].into_iter().collect();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".schema.json") && !active.contains(&name) {
                if fs::remove_file(entry.path()).is_ok() {
                    eprintln!("  removed stale schema {name}");
                }
            }
        }
    }
    written
}

/// Minimal JSON string escaping. The registry's descriptions are prose written
/// by the project, so quotes and backslashes are the realistic cases.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Write only when the content differs, so regeneration does not churn
/// timestamps in git for files nobody changed.
fn write_if_changed(path: &Path, content: &str) -> bool {
    if let Ok(existing) = fs::read_to_string(path) {
        if existing == content {
            return false;
        }
    }
    fs::write(path, content).is_ok()
}


/// Render one namespace's rows as a markdown table.
///
/// Column order follows the namespace's declared field order rather than the
/// rows' own key order, so the table reads the way the project described the
/// namespace and stays stable when a row happens to omit an optional field.
pub fn render_table(ns: &RegistryNamespace, reg: &Registry) -> String {
    let Some(ids) = reg.by_namespace.get(&ns.key) else {
        return String::new();
    };

    if let Some(group) = &ns.group_by {
        // Groups appear in first-seen order rather than sorted, so the file
        // order the project chose is the order a reader meets them.
        let mut order: Vec<String> = Vec::new();
        let mut buckets: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in ids {
            let key = reg
                .get(id)
                .and_then(|r| r.fields.get(group).cloned())
                .unwrap_or_else(|| "ungrouped".to_string());
            if !buckets.contains_key(&key) {
                order.push(key.clone());
            }
            buckets.entry(key).or_default().push(id.clone());
        }
        let mut out = String::new();
        for key in order {
            let members = &buckets[&key];
            out.push_str(&format!("\n### {key}\n\n"));
            out.push_str(&render_rows(ns, reg, members));
        }
        return out;
    }

    render_rows(ns, reg, ids)
}

/// One table over the given rows.
fn render_rows(ns: &RegistryNamespace, reg: &Registry, ids: &[String]) -> String {

    // `id` first, then declared fields in declaration order. A field that no
    // row actually carries is dropped: an always-empty column is noise.
    let mut columns: Vec<String> = vec!["id".to_string()];
    for f in &ns.fields {
        if ids
            .iter()
            .filter_map(|id| reg.get(id))
            .any(|r| r.fields.contains_key(&f.name))
        {
            columns.push(f.name.clone());
        }
    }

    let mut out = String::new();
    out.push_str("| ");
    out.push_str(&columns.join(" | "));
    out.push_str(" |\n|");
    for _ in &columns {
        out.push_str("---|");
    }
    out.push('\n');

    for id in ids {
        let Some(row) = reg.get(id) else { continue };
        let cells: Vec<String> = columns
            .iter()
            .map(|c| {
                // `id` is a first-class field of the row rather than just an
                // entry in its map, so read it from there. The loader happens
                // to also put it in the map, but relying on that would make
                // the table depend on a loader detail.
                if c == "id" {
                    // The anchor rides the row's own first cell, so a reference
                    // lands on the row rather than on a restatement of it.
                    // Markdown has no per-row anchor of its own, and the
                    // alternative of a separate anchor block below the table
                    // duplicates every row to say nothing new.
                    return format!("<a id=\"{}\"></a>{}", row.anchor(), row.slug);
                }
                let raw = if false {
                    row.slug.as_str()
                } else {
                    row.fields.get(c).map(String::as_str).unwrap_or("")
                };
                // A pipe inside a cell would end the column early, and a
                // newline would end the row. Both are realistic in prose
                // fields, which is exactly why the source is TOML.
                raw.replace('|', "\\|").replace('\n', " ")
            })
            .collect();
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
    }
    out
}

/// Generate one page per namespace that asked for one.
///
/// Anchors are the lowercased identifier, matching what `resolve_all`
/// links to. The two must agree, so they are stated once each and tested
/// together rather than being left to coincide.
pub fn render_pages(
    mock_dir: &Path,
    docs_dir: &Path,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
    header: &str,
    cfg: &crate::config::Config,
) -> Vec<PathBuf> {
    let mut written = Vec::new();
    for (idx, ns) in namespaces.iter().filter(|n| n.render.has_page()).enumerate() {
        let Some(ids) = reg.by_namespace.get(&ns.key) else {
            continue;
        };
        // A namespace named `catalogue` would otherwise generate `CATALOGUE.md`
        // over a hand-authored document's rendered output, silently. The
        // template is what claims the name, so its presence is the collision.
        let page_file = crate::render_design::registry_doc_name(&ns.page_name(), cfg, idx);
        let claimed = mock_dir.join(format!("{}.tmpl", ns.page_name()));
        if claimed.is_file() {
            eprintln!(
                "  ERROR: namespace `{}` would generate {} over the document rendered from {}. Rename the namespace or the template.",
                ns.key,
                ns.page_name(),
                claimed.display()
            );
            continue;
        }
        let mut body = format!("{header}\n# {}\n\n", ns.title());
        if let Some(d) = &ns.description {
            body.push_str(d);
            body.push_str("\n\n");
        }
        body.push_str(&format!(
            "{} rows. Identifiers are permanent: assigned once, never reused, never renumbered.\n\n",
            ids.len()
        ));
        body.push_str(&render_table(ns, reg));

        let page_file = crate::render_design::registry_doc_name(&ns.page_name(), cfg, idx);
        let claimed = mock_dir.join(format!("{}.tmpl", ns.page_name()));
        if claimed.is_file() {
            eprintln!(
                "  ERROR: namespace `{}` would generate {} over the document rendered from {}. Rename the namespace or the template.",
                ns.key,
                ns.page_name(),
                claimed.display()
            );
            continue;
        }
        let mut body = format!("{header}\n# {}\n\n", ns.title());
        if let Some(d) = &ns.description {
            body.push_str(d);
            body.push_str("\n\n");
        }
        body.push_str(&format!(
            "{} rows. Identifiers are permanent: assigned once, never reused, never renumbered.\n\n",
            ids.len()
        ));
        body.push_str(&render_table(ns, reg));

        // Per-row anchors, so a link from prose lands on the row rather than
        // on the top of a table with several hundred entries.
        body.push_str("\n");
        for id in ids {
            let Some(row) = reg.get(id) else { continue };
            let name = row
                .fields
                .get("name")
                .or_else(|| row.fields.get("question"))
                .or_else(|| row.fields.get("rule"))
                .map(String::as_str)
                .unwrap_or("");
            body.push_str(&format!("\n<a id=\"{}\"></a>\n", id.to_lowercase()));
            body.push_str(&format!("**{id}**{}\n", if name.is_empty() { String::new() } else { format!(": {name}") }));
        }

        // A registry page is a document like any other, so references inside
        // its rows resolve here too. Without this a row could reference another
        // row and the reference would render literally on the one page most
        // likely to carry it.
        let body = resolve_all(
            &body,
            namespaces,
            reg,
            &cfg.registry_roots,
            &cfg.repo_root,
            &cfg.docs_dir,
            cfg,
        );
        let path = docs_dir.join(page_file);
        if write_if_changed(&path, &body) {
            written.push(path);
        }
    }
    written
}

/// Expand `{{registry:<key>}}` placeholders into that namespace's table.
///
/// This is what makes `embed` mode useful: a project drops the placeholder in
/// whatever document the table belongs in, rather than accepting a generated
/// page it then has to link to from somewhere.
pub fn expand_embeds(text: &str, namespaces: &[RegistryNamespace], reg: &Registry) -> String {
    let mut out = text.to_string();
    for ns in namespaces {
        let token = format!("{{{{registry:{}}}}}", ns.key);
        if out.contains(&token) {
            out = out.replace(&token, &render_table(ns, reg));
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

/// Roots every project gets without declaring them.
///
/// `reg` is the registry itself. `mock` is the mock directory, which is where
/// a project's own design documents live, so `mock::DESIGN::12` works
/// everywhere without configuration. Deeper paths need no extra root because
/// the path may have any number of segments: `mock::crates::numeric::DESIGN::12`
/// resolves the same way.
///
/// Anything project-specific is declared rather than baked. Not every project
/// has a corpus it indexes, so a root named `seed` belongs in that project's
/// configuration and not in the tool.
pub fn builtin_roots(mock_dir_rel: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("mock".to_string(), mock_dir_rel.to_string());
    // The repository itself. Some documents exist only on the shipping side
    // even where source mirrors the design workspace: a public API note, a
    // changelog, a readme that ships. Those have no `mock` path to cite.
    m.insert("live".to_string(), ".".to_string());
    m
}

/// What a citation points at within a file.
///
/// Line numbers fail SILENTLY. An edit anywhere above a cited line shifts it,
/// the citation still resolves, and it now points at different content. That is
/// the worst failure shape this project recognises: the check passes and the
/// answer is wrong. Only "past the end of the file" fails loudly, and that is
/// the case that matters least.
///
/// A heading fails loudly instead. Rename it and the citation stops resolving,
/// which is a report rather than a lie. Headings are therefore the default, and
/// line numbers belong to roots that do not move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// A heading slug: `#the-four-lanes`. Survives every edit that does not
    /// rename the heading, and announces itself when one does.
    Heading(String),
    /// An explicit line. Honest only where the file does not change, which is
    /// why a root carrying line citations should declare itself frozen.
    Line(usize),
}

/// A file citation: `root::seg::seg...::anchor`.
///
/// The last segment is the line and everything between the root and it joins
/// as a path, so one rule covers `mock::DESIGN::12` and
/// `mock::crates::numeric::DESIGN::12` without either needing its own root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub root: String,
    pub path: String,
    pub anchor: Anchor,
}

impl FileRef {
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split("::").collect();
        if parts.len() < 3 {
            return None;
        }
        let last = parts.last()?.trim();
        let anchor = if let Some(h) = last.strip_prefix('#') {
            if h.is_empty() {
                return None;
            }
            Anchor::Heading(h.to_string())
        } else {
            let n = last.parse::<usize>().ok()?;
            if n == 0 {
                return None;
            }
            Anchor::Line(n)
        };
        let root = parts[0].trim();
        if root.is_empty() {
            return None;
        }
        let path = parts[1..parts.len() - 1].join("/");
        if path.is_empty() {
            return None;
        }
        Some(Self { root: root.to_string(), path, anchor })
    }

    pub fn render(&self) -> String {
        let a = match &self.anchor {
            Anchor::Heading(h) => format!("#{h}"),
            Anchor::Line(n) => n.to_string(),
        };
        format!("{}::{}::{}", self.root, self.path.replace('/', "::"), a)
    }
}

/// Resolve a citation's path under a root, allowing the extension to be
/// omitted.
///
/// `mock::DESIGN::12` finds `DESIGN.md.tmpl` without the author having to know
/// the extension. That is the real motivation: the same document exists as a
/// template in one root and as rendered output in another, and a citation
/// should not have to track which.
///
/// Exactly one match resolves. Several is an error rather than a guess: the
/// author meant one of them, and picking silently would point the citation
/// somewhere they did not choose.
pub enum PathResolution {
    Found(PathBuf),
    Missing,
    Ambiguous(Vec<String>),
}

pub fn resolve_cited_path(root_dir: &Path, path: &str) -> PathResolution {
    let exact = root_dir.join(path);
    if exact.is_file() {
        return PathResolution::Found(exact);
    }
    let target = Path::new(path);
    let dir = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => root_dir.join(p),
        _ => root_dir.to_path_buf(),
    };
    let Some(stem) = target.file_name().map(|s| s.to_string_lossy().to_string()) else {
        return PathResolution::Missing;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return PathResolution::Missing;
    };
    let mut matches: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .map(|n| {
                        let n = n.to_string_lossy();
                        n == stem || n.starts_with(&format!("{stem}."))
                    })
                    .unwrap_or(false)
        })
        .collect();
    matches.sort();
    match matches.len() {
        0 => PathResolution::Missing,
        1 => PathResolution::Found(matches.remove(0)),
        _ => PathResolution::Ambiguous(
            matches
                .iter()
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .collect(),
        ),
    }
}

/// A relative path from `from` (a directory) to `to`, in forge-link form.
fn relative_from(from: &Path, to: &Path) -> String {
    let f: Vec<_> = from.components().collect();
    let t: Vec<_> = to.components().collect();
    let common = f.iter().zip(t.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); f.len() - common];
    parts.extend(t[common..].iter().map(|c| c.as_os_str().to_string_lossy().to_string()));
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}

/// Rewrite file citations in a rendered document into line links.
pub fn resolve_doc_refs(
    text: &str,
    roots: &BTreeMap<String, String>,
    repo_root: &Path,
    docs_dir: &Path,
) -> String {
    if roots.is_empty() || !text.contains("::") {
        return text.to_string();
    }
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
        for tok in citation_tokens(line) {
            let Some(r) = FileRef::parse(&tok) else { continue };
            if r.root == REGISTRY_ROOT {
                continue;
            }
            let Some(rel) = roots.get(&r.root) else { continue };
            if let PathResolution::Found(target) =
                resolve_cited_path(&repo_root.join(rel), &r.path)
            {
                let link = relative_from(docs_dir, &target);
                rewritten = rewritten.replace(
                    &tok,
                    &match &r.anchor {
                        Anchor::Heading(h) => {
                            format!("[{}/{}#{h}]({}#{h})", r.root, r.path, link)
                        }
                        Anchor::Line(n) => {
                            format!("[{}/{}:{n}]({}#L{n})", r.root, r.path, link)
                        }
                    },
                );
            }
        }
        out.push_str(&rewritten);
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Citation-shaped tokens in a line: a run of `::`-joined segments ending in
/// digits. Bounded by whitespace and the characters that bracket a citation in
/// prose, so a reference inside parentheses or backticks does not swallow them.
fn citation_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in line.split(|c: char| c.is_whitespace() || "()[]`\"',;".contains(c)) {
        if raw.matches("::").count() >= 2 && raw.split("::").last().map(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())).unwrap_or(false) {
            out.push(raw.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(key: &str, value_field: Option<&str>) -> RegistryNamespace {
        RegistryNamespace {
            key: key.into(),
            title: None,
            description: None,
            value_field: value_field.map(|s| s.to_string()),
            render: RenderMode::Page,
            group_by: None,
            fields: vec![],
        }
    }

    fn r_all(text: &str, nss: &[RegistryNamespace], reg: &Registry) -> String {
        let cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        resolve_all(text, nss, reg, &BTreeMap::new(), Path::new("/r"), Path::new("/r/docs"), &cfg)
    }

    fn reg_with(slug: &str, namespace: &str, fields: &[(&str, &str)]) -> Registry {
        let mut reg = Registry::default();
        let row = RegistryRow {
            slug: slug.into(),
            namespace: namespace.into(),
            source: PathBuf::from("t.toml"),
            fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        let q = row.qualified();
        reg.by_namespace.insert(namespace.into(), vec![q.clone()]);
        reg.rows.insert(q, row);
        reg
    }

    #[test]
    fn a_row_reference_becomes_a_link() {
        let reg = reg_with("xpbd", "vocab", &[]);
        let out = r_all("see {{ reg::vocab::xpbd }} now", &[ns("vocab", None)], &reg);
        assert_eq!(out, "see [xpbd](VOCAB.md#xpbd) now");
    }

    #[test]
    fn a_field_reference_renders_that_field() {
        // Lets a document state a constant once and have every mention of it
        // stay current.
        let reg = reg_with("froxel", "constant", &[("value", "32 km")]);
        let out = r_all("reaches {{ reg::constant::froxel::value }} out", &[ns("constant", None)], &reg);
        assert_eq!(out, "reaches 32 km out");
    }

    #[test]
    fn a_rust_path_is_not_a_citation() {
        // `::` is ordinary in prose about code. A citation needs a numeric
        // final segment, which is what keeps the two apart.
        assert!(FileRef::parse("std::mem::swap").is_none());
        assert!(FileRef::parse("seed::DESIGN::844").is_some());
    }

    #[test]
    fn a_citation_path_may_have_any_depth() {
        // One rule covers mock::DESIGN::12 and mock::crates::numeric::DESIGN::12,
        // so a deeper tree needs no root of its own.
        let r = FileRef::parse("mock::crates::numeric::DESIGN::12").unwrap();
        assert_eq!(r.root, "mock");
        assert_eq!(r.path, "crates/numeric/DESIGN");
        assert_eq!(r.anchor, Anchor::Line(12));
    }

    #[test]
    fn an_unknown_row_or_field_is_left_alone_and_reported() {
        let reg = reg_with("xpbd", "vocab", &[("what", "w")]);
        let text = "{{ reg::vocab::nope }} and {{ reg::vocab::xpbd::missing }}";
        assert_eq!(r_all(text, &[ns("vocab", None)], &reg), text);
        let d = dangling_references(text, &reg);
        assert!(d.contains("reg::vocab::nope"), "{d:?}");
        assert!(d.contains("reg::vocab::xpbd::missing"), "{d:?}");
    }

    #[test]
    fn code_fences_are_not_rewritten() {
        let reg = reg_with("xpbd", "vocab", &[]);
        let text = "a {{ reg::vocab::xpbd }}\n```\nb {{ reg::vocab::xpbd }}\n```";
        let out = r_all(text, &[ns("vocab", None)], &reg);
        assert!(out.contains("```\nb {{ reg::vocab::xpbd }}\n```"), "fence rewritten: {out}");
    }

    #[test]
    fn a_placeholder_is_required_for_a_reference() {
        // The braces are what make a reference stated rather than guessed. A
        // project with a root named `core` would otherwise silently link
        // `core::mem::12`, and prose about code would be rewritten by accident.
        let exprs = placeholder_exprs("a {{ reg::vocab::xpbd }} and bare reg::vocab::xpbd");
        assert_eq!(exprs.len(), 1);
        assert_eq!(exprs[0].1, "reg::vocab::xpbd");
    }

    #[test]
    fn one_syntax_covers_rows_fields_tables_and_files() {
        let reg = reg_with("xpbd", "vocab", &[("what", "constraint projection")]);
        let nss = vec![ns("vocab", None)];
        let roots = BTreeMap::new();
        let r = |s: &str| {
            let cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
            resolve_all(s, &nss, &reg, &roots, Path::new("/r"), Path::new("/r/docs"), &cfg)
        };
        assert_eq!(r("{{ reg::vocab::xpbd }}"), "[xpbd](VOCAB.md#xpbd)");
        assert_eq!(r("{{ reg::vocab::xpbd::what }}"), "constraint projection");
        // The id cell carries the row's anchor inline, so a reference lands on
        // the row itself rather than on a restatement of it below the table.
        assert!(r("{{ reg::vocab }}").contains("<a id=\"xpbd\"></a>xpbd"));
        // Unresolvable stays visibly unresolved rather than becoming a
        // plausible-looking wrong link.
        assert_eq!(r("{{ reg::vocab::nope }}"), "{{ reg::vocab::nope }}");
    }

    #[test]
    fn a_heading_anchor_parses_and_renders() {
        let r = FileRef::parse("seed::DESIGN::#the-four-lanes").unwrap();
        assert_eq!(r.anchor, Anchor::Heading("the-four-lanes".into()));
        assert_eq!(r.render(), "seed::DESIGN::#the-four-lanes");
    }

    #[test]
    fn heading_slugs_match_the_forge_form() {
        // So a reader can click the same anchor the link generates.
        assert_eq!(heading_slug("The four lanes and the joint LOD cut"),
                   "the-four-lanes-and-the-joint-lod-cut");
        assert_eq!(heading_slug("R(V), the drift class"), "r-v-the-drift-class");
    }

    #[test]
    fn a_line_anchor_still_parses() {
        // Kept for frozen roots, where the file genuinely does not move.
        assert_eq!(FileRef::parse("seed::DESIGN::844").unwrap().anchor, Anchor::Line(844));
        assert!(FileRef::parse("seed::DESIGN::0").is_none());
        assert!(FileRef::parse("seed::DESIGN::#").is_none());
    }

    #[test]
    fn a_builtin_namespace_is_addressed_directly() {
        // vocab::xpbd, not reg::vocab::xpbd. Builtins exist in every project so
        // the short form is safe everywhere; a project's own namespace stays
        // behind reg:: to read as what it is.
        let reg = reg_with("xpbd", "vocab", &[("what", "constraint projection")]);
        let nss = with_builtins(&[]);
        assert_eq!(
            r_all("{{ vocab::xpbd::what }}", &nss, &reg),
            "constraint projection"
        );
    }

    #[test]
    fn reference_is_builtin_and_renders_as_a_citation() {
        let mut reg = Registry::default();
        let row = RegistryRow {
            slug: "burns_hunt".into(),
            namespace: "reference".into(),
            source: PathBuf::from("t.toml"),
            fields: [
                ("title", "The Visibility Buffer"),
                ("authors", "Burns and Hunt"),
                ("venue", "JCGT"),
                ("year", "2013"),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        };
        reg.by_namespace.insert("reference".into(), vec![row.qualified()]);
        reg.rows.insert(row.qualified(), row);
        // Short by default: it has to fit a table cell beside hundreds of
        // others, and the full form is one click away.
        assert_eq!(
            r_all("{{ reference::burns_hunt }}", &with_builtins(&[]), &reg),
            "[Burns and Hunt 2013](REFERENCE.md#burns-hunt)"
        );
        // The full form on request, for prose that wants the title.
        assert_eq!(
            r_all("{{ reference::burns_hunt::citation }}", &with_builtins(&[]), &reg),
            "[Burns and Hunt, The Visibility Buffer (JCGT 2013)](REFERENCE.md#burns-hunt)"
        );
        // A real field is still a real field.
        assert_eq!(
            r_all("{{ reference::burns_hunt::year }}", &with_builtins(&[]), &reg),
            "2013"
        );
    }

    #[test]
    fn slugs_are_snake_case() {
        assert!(is_valid_slug("xpbd") && is_valid_slug("waist_a") && is_valid_slug("lane_2"));
        assert!(!is_valid_slug("Waist-A"));
        assert!(!is_valid_slug("2fast"));
        assert!(!is_valid_slug(""));
    }

    #[test]
    fn vocab_and_the_builtin_roots_need_no_declaration() {
        let merged = with_builtins(&[]);
        assert!(merged.iter().any(|n| n.key == "vocab"));
        let roots = builtin_roots("mock");
        assert_eq!(roots.get("mock").map(String::as_str), Some("mock"));
        assert_eq!(roots.get("live").map(String::as_str), Some("."));
        // Project-specific roots stay the project's: not every project indexes
        // a corpus, so nothing named `seed` is baked in.
        assert!(!roots.contains_key("seed"));
    }
}

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
    // Glob the docs directory rather than reconstruct the name: the sort
    // prefix depends on dependency depth, which this does not need to know.
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

    // A builtin namespace addressed directly: `vocab::xpbd` rather than
    // `reg::vocab::xpbd`. Rewritten into the registry form so there is one
    // resolution path and the two cannot diverge.
    if BUILTIN_NAMESPACES.contains(&parts[0]) && parts.len() >= 2 {
        let rewritten = format!("{}::{}", REGISTRY_ROOT, parts.join("::"));
        return resolve_expr(&rewritten, by_key, reg, roots, repo_root, docs_dir, cfg);
    }

    if parts[0] == REGISTRY_ROOT {
        return match parts.len() {
            // A whole namespace: its table, inline.
            2 => by_key.get(parts[1]).map(|ns| render_table(ns, reg)),
            // A row, or a field on it.
            3 | 4 if parts[1] == "reference" => {
                let qualified = format!("{}::{}", parts[1], parts[2]);
                let row = reg.get(&qualified)?;
                let ns = by_key.get("reference")?;
                let target = format!("{}#{}", ns.page_name(), row.anchor());

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
                    return row.fields.get(parts[3]).cloned();
                }
                Some(match ns.value_field.as_ref().and_then(|f| row.fields.get(f)) {
                    Some(v) => v.clone(),
                    None if !ns.render.has_page() => row.slug.clone(),
                    None => format!("[{}]({}#{})", row.slug, ns.page_name(), row.anchor()),
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

/// Resolve an anchor to a line in `path`.
///
/// A heading matches on its slug: lowercased, non-alphanumerics collapsed to
/// hyphens, which is what forges generate and therefore what a reader can also
/// click to.
pub fn resolve_anchor(path: &Path, anchor: &Anchor) -> Option<usize> {
    match anchor {
        Anchor::Line(n) => {
            let count = fs::read_to_string(path).ok()?.lines().count();
            if *n <= count { Some(*n) } else { None }
        }
        Anchor::Heading(want) => {
            let text = fs::read_to_string(path).ok()?;
            text.lines().enumerate().find_map(|(i, l)| {
                let trimmed = l.trim_start();
                if !trimmed.starts_with('#') {
                    return None;
                }
                let title = trimmed.trim_start_matches('#').trim();
                (heading_slug(title) == *want).then_some(i + 1)
            })
        }
    }
}

/// The forge-compatible slug for a heading.
pub fn heading_slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                out.push(l);
            }
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
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
    let Some(row) = reg.get(id) else { return };

    // Resolve everything this row depends on first, so by the time its own
    // fields are rewritten the values they cite are final.
    path.push(id.to_string());
    for value in row.fields.values() {
        for (dep, _) in find_registry_refs(value) {
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
        for (dep, _) in find_registry_refs(value) {
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

/// The short form: who and when, enough to recognise the work.
///
/// What a reader needs at a glance, and what fits in a table cell beside four
/// hundred others. The full citation is one click away, which is the whole
/// reason the short form is safe.
pub fn short_citation(row: &RegistryRow) -> String {
    let f = |k: &str| row.fields.get(k).filter(|v| !v.is_empty()).cloned();
    match (f("authors"), f("year")) {
        (Some(a), Some(y)) => format!("{a} {y}"),
        (Some(a), None) => a,
        (None, Some(y)) => format!("{}, {y}", f("title").unwrap_or_else(|| row.slug.clone())),
        (None, None) => f("title").unwrap_or_else(|| row.slug.clone()),
    }
}

/// Render a reference row as a citation.
///
/// Assembled from the fields the row declares, skipping what it does not, so a
/// specification with an institutional author and no page reads as cleanly as
/// a journal paper. The format is deliberately plain: the value is that one
/// work cited in twenty places renders identically in all twenty, not that it
/// matches any particular house style.
pub fn format_citation(row: &RegistryRow) -> String {
    let f = |k: &str| row.fields.get(k).filter(|v| !v.is_empty()).cloned();
    let mut out = String::new();
    if let Some(a) = f("authors") {
        out.push_str(&a);
        out.push_str(", ");
    }
    out.push_str(&f("title").unwrap_or_else(|| row.slug.clone()));
    if let Some(v) = f("venue") {
        let year = f("year").map(|y| format!(" {y}")).unwrap_or_default();
        out.push_str(&format!(" ({v}{year})"));
    } else if let Some(y) = f("year") {
        out.push_str(&format!(" ({y})"));
    }
    out
}
