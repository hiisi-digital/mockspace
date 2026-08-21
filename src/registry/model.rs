use super::*;

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
    pub name:        String,
    /// `string`, `integer`, `boolean`, `string[]`. Anything richer belongs in
    /// a hand-written schema fragment rather than in a config language.
    #[serde(default = "default_field_type")]
    pub r#type:      String,
    #[serde(default)]
    pub required:    bool,
    #[serde(default)]
    pub description: Option<String>,
    /// Whether this field reaches the generated documentation at all.
    #[serde(default)]
    pub visibility:  FieldVisibility,
}

/// Whether a field's values are for readers or only for the project itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldVisibility {
    /// Rendered as a column, like any other field.
    #[default]
    Public,
    /// Never rendered, and a reference to it is an error rather than a leak.
    ///
    /// Some fields exist to tie a row back to something the reader cannot
    /// open: an identifier from a superseded corpus, a note addressed to the
    /// project rather than to anyone reading the result. Rendering those is
    /// noise at best. Marking the field internal keeps it validated, keeps it
    /// greppable in the source, and keeps it out of the document.
    ///
    /// A reference to an internal field is reported rather than resolved,
    /// because a guarantee with a documented way around it is not one.
    Internal,
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
    pub key:         String,
    #[serde(default)]
    pub title:       Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// When set, a bare row reference renders this field's value instead of a
    /// link, so a constant is stated once and every mention stays current.
    #[serde(default)]
    pub value_field: Option<String>,
    #[serde(default)]
    pub render:      RenderMode,
    /// Render the table in sections, one per distinct value of this field.
    ///
    /// A flat table of every row is the wrong shape for a namespace whose rows
    /// belong to named groups: tasks under milestones, vocabulary under the
    /// closed set it belongs to. Grouping is derived from the data rather than
    /// maintained as a second list, so a row moves between sections by editing
    /// the field that says where it belongs.
    #[serde(default)]
    pub group_by:    Option<String>,
    #[serde(default, rename = "field")]
    pub fields:      Vec<RegistryField>,
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
    pub slug:      String,
    pub namespace: String,
    /// Where it was declared, so an error can point at a file.
    pub source:    PathBuf,
    pub fields:    BTreeMap<String, String>,
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
    pub rows:         BTreeMap<String, RegistryRow>,
    pub by_namespace: BTreeMap<String, Vec<String>>,
    /// Slugs declared twice within one namespace, with every declaring file.
    /// An error rather than a warning: two rows for one identifier means a
    /// reference cannot be resolved. No per-file schema can catch it, because
    /// each file is valid on its own.
    pub duplicates:   BTreeMap<String, Vec<PathBuf>>,
}

impl Registry {
    pub fn get(&self, qualified: &str) -> Option<&RegistryRow> {
        self.rows.get(qualified)
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
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
    let f = |name: &str, required: bool, description: &str| {
        RegistryField {
            name: name.to_string(),
            r#type: "string".to_string(),
            required,
            description: Some(description.to_string()),
            visibility: FieldVisibility::Public,
        }
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
                // Declared rather than hardcoded: this is the builtin that used
                // to be recognised by its name alone.
                r#type: "ref[]".to_string(),
                required: false,
                description: Some(
                    "Where the term is defined, when it comes from somewhere rather than being defined by the design itself."
                        .to_string(),
                ),
                visibility: FieldVisibility::Public,
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
    let f = |name: &str, required: bool, description: &str| {
        RegistryField {
            name: name.to_string(),
            r#type: "string".to_string(),
            required,
            description: Some(description.to_string()),
            visibility: FieldVisibility::Public,
        }
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
/// Names slot zero already means, which a namespace therefore cannot take.
///
/// `mock` and `live` are the builtin file roots; a project's own roots are
/// checked separately, since those it can rename.
pub const RESERVED_ROOTS: &[&str] = &["reg", "crates", "mock", "live"];

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
/// Whether a declared field type means "this field holds references".
///
/// Reference validation was keyed on the literal field name `"provenance"`,
/// which works for exactly one consumer: one whose every reference-bearing
/// field happens to be called that. The registry design says provenance is
/// deliberately *not* universal, because baking one consumer's field into the
/// mechanism warps the feature around it, and the hardcoded name was doing
/// precisely that.
///
/// A type says what a field *is*, so a project may call its reference-bearing
/// fields whatever its subject calls them, and may have more than one.
pub fn is_reference_type(t: &str) -> bool {
    matches!(t, "ref" | "ref[]")
}

/// The reference-bearing field names of each namespace, by namespace key.
pub fn reference_fields(
    namespaces: &[RegistryNamespace],
) -> BTreeMap<String, Vec<String>> {
    namespaces
        .iter()
        .map(|ns| {
            (
                ns.key.clone(),
                ns.fields
                    .iter()
                    .filter(|f| is_reference_type(&f.r#type))
                    .map(|f| f.name.clone())
                    .collect(),
            )
        })
        .collect()
}

pub fn builtin_roots(mock_dir_rel: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("mock".to_string(), mock_dir_rel.to_string());
    // The repository itself. Some documents exist only on the shipping side
    // even where source mirrors the design workspace: a public API note, a
    // changelog, a readme that ships. Those have no `mock` path to cite.
    m.insert("live".to_string(), ".".to_string());
    m
}
