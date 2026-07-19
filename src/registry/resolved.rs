//! What a reference resolves to, before anything decides how to show it.
//!
//! Resolution used to produce a string, which meant the markdown was decided in
//! the same breath as the lookup. That was fine while documents were the only
//! consumer. They are not: `cargo mock query` asks the same questions from a
//! terminal, where a markdown link is noise and a table needs aligned columns
//! rather than pipes.
//!
//! So resolution answers *what was found* and a renderer decides how to show
//! it. Two renderers, one lookup. The alternative, a query path that resolves
//! references its own way, is the shape this codebase has spent a long time
//! removing: two paths answering one question and drifting.

use super::{Registry, RegistryNamespace, RegistryRow};

/// What a reference found.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    /// A whole namespace: `law`.
    Table {
        namespace: String,
        /// Column headings, `id` first.
        columns:   Vec<String>,
        /// One entry per row, in the namespace's own order.
        rows:      Vec<Vec<String>>,
    },
    /// One row: `law::keys`.
    Row {
        namespace: String,
        slug:      String,
        /// Where the row's document is, and its anchor within it.
        target:    String,
        /// What the row says, for a reader who cannot follow the link.
        fields:    Vec<(String, String)>,
    },
    /// One field of a row: `law::keys::statement`.
    Field(String),
    /// A citation into a file: `seed::DESIGN::12`.
    Citation {
        /// What a reader sees.
        text: String,
        /// Where it points, when it points anywhere a reader can follow.
        link: Option<String>,
    },
    /// A path: `pathof(law::keys)`.
    Path(String),
    /// Several values: `sourcesof(law::keys)`.
    List(Vec<String>),
    /// A number: `law::keys::crates.count()`.
    Count(usize),
    /// Found, and deliberately shows nothing. A citation into an internal root
    /// is the case: the reference is real and checked, and what it names is not
    /// something a reader can open.
    ///
    /// Distinct from not resolving at all, which is reported.
    Hidden,
}

impl Resolved {
    /// How this appears in a generated document.
    pub fn to_markdown(&self) -> String {
        match self {
            Resolved::Table {
                columns,
                rows,
                ..
            } => render_markdown_table(columns, rows),
            Resolved::Row {
                slug,
                target,
                ..
            } => format!("[{slug}]({target})"),
            Resolved::Field(v) => v.clone(),
            Resolved::Citation {
                text,
                link,
            } => {
                match link {
                    Some(l) => format!("[{text}]({l})"),
                    None => text.clone(),
                }
            },
            Resolved::Path(p) => p.clone(),
            Resolved::List(items) => items.join(", "),
            Resolved::Count(n) => n.to_string(),
            Resolved::Hidden => String::new(),
        }
    }

    /// How this appears in a terminal.
    ///
    /// A link is useless where it cannot be clicked, so a row shows what it
    /// says and then where it lives. A table gets aligned columns, because
    /// pipes are for markdown parsers rather than for people.
    pub fn to_terminal(&self) -> String {
        match self {
            Resolved::Table {
                namespace,
                columns,
                rows,
            } => {
                let mut out = render_aligned_table(columns, rows);
                out.push_str(&format!("\n{} rows in {namespace}\n", rows.len()));
                out
            },
            Resolved::Row {
                namespace,
                slug,
                target,
                fields,
            } => {
                let mut out = format!("{namespace}::{slug}\n");
                let width = fields.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
                for (k, v) in fields {
                    out.push_str(&format!("  {k:<width$}  {}\n", unlink(v)));
                }
                out.push_str(&format!("\n  in {target}\n"));
                out
            },
            Resolved::Field(v) => format!("{v}\n"),
            Resolved::Citation {
                text,
                link,
            } => {
                match link {
                    Some(l) => format!("{text}\n  {l}\n"),
                    None => format!("{text}\n"),
                }
            },
            Resolved::Path(p) => format!("{p}\n"),
            Resolved::List(items) => items.iter().map(|i| format!("{i}\n")).collect::<String>(),
            Resolved::Count(n) => format!("{n}\n"),
            // Say why nothing is shown. In a document the silence is the point;
            // at a prompt it reads as a broken query.
            Resolved::Hidden => {
                "(resolves, and renders as nothing: it names an internal root)\n".to_string()
            },
        }
    }

    /// A one-word name for what was found, for a caller that wants to branch on
    /// the shape rather than read it.
    pub fn kind(&self) -> &'static str {
        match self {
            Resolved::Table {
                ..
            } => "table",
            Resolved::Row {
                ..
            } => "row",
            Resolved::Field(_) => "field",
            Resolved::Citation {
                ..
            } => "citation",
            Resolved::Path(_) => "path",
            Resolved::List(_) => "list",
            Resolved::Count(_) => "count",
            Resolved::Hidden => "hidden",
        }
    }

    /// The value as a plain string, for a method chain to narrow.
    pub fn as_scalar(&self) -> String {
        match self {
            Resolved::List(items) => items.join(", "),
            other => other.to_markdown(),
        }
    }
}

/// Strip markdown links down to their text.
///
/// Row data holds resolved references, so a field can carry `[world](120_WORLD.md)`.
/// In a document that is the point. At a prompt the target is unclickable and
/// the brackets are noise around the only part worth reading.
fn unlink(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open ..].find("](") else { break };
        let close = open + close;
        let Some(end) = rest[close ..].find(')') else { break };
        out.push_str(&rest[.. open]);
        out.push_str(&rest[open + 1 .. close]);
        rest = &rest[close + end ..][1 ..];
    }
    out.push_str(rest);
    out
}

/// Build a row's field list in the namespace's declared order.
///
/// Declaration order rather than alphabetical, because the order a project
/// declared its fields in is the order it thinks about them.
pub fn row_fields(ns: &RegistryNamespace, row: &RegistryRow) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for f in &ns.fields {
        if let Some(v) = row.fields.get(&f.name) {
            if !v.is_empty() {
                out.push((f.name.clone(), v.clone()));
            }
        }
    }
    out
}

/// The columns and cells a namespace's table shows.
pub fn table_cells(ns: &RegistryNamespace, reg: &Registry) -> (Vec<String>, Vec<Vec<String>>) {
    let ids = reg.by_namespace.get(&ns.key).cloned().unwrap_or_default();
    let mut columns = vec!["id".to_string()];
    for f in &ns.fields {
        if f.visibility == super::FieldVisibility::Internal {
            continue;
        }
        if ids
            .iter()
            .filter_map(|i| reg.get(i))
            .any(|r| r.fields.get(&f.name).is_some_and(|v| !v.is_empty()))
        {
            columns.push(f.name.clone());
        }
    }
    let rows = ids
        .iter()
        .filter_map(|i| reg.get(i))
        .map(|r| {
            columns
                .iter()
                .map(|c| {
                    if c == "id" {
                        r.slug.clone()
                    } else {
                        r.fields.get(c).cloned().unwrap_or_default()
                    }
                })
                .collect()
        })
        .collect();
    (columns, rows)
}

fn render_markdown_table(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut out = format!(
        "| {} |\n|{}\n",
        columns.join(" | "),
        "---|".repeat(columns.len())
    );
    let id_col = columns.iter().position(|c| c == "id");
    for r in rows {
        let cells: Vec<String> = r
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let cell = c.replace('|', "\\|").replace('\n', " ");
                // The anchor rides the row's own id cell, so a reference lands
                // on the row rather than on a restatement of it below the
                // table. Markdown has no per-row anchor of its own.
                //
                // Markdown only: a terminal showing raw HTML would be showing
                // the reader the mechanism instead of the answer.
                if Some(i) == id_col {
                    format!("<a id=\"{}\"></a>{cell}", cell.replace('_', "-"))
                } else {
                    cell
                }
            })
            .collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    out
}

/// Columns padded to their widest cell, truncated so one long field does not
/// push every other column off the screen.
fn render_aligned_table(columns: &[String], rows: &[Vec<String>]) -> String {
    const MAX: usize = 48;
    let clip = |s: &str| -> String {
        let one = s.replace('\n', " ");
        if one.chars().count() > MAX {
            let mut t: String = one.chars().take(MAX - 1).collect();
            t.push('…');
            t
        } else {
            one
        }
    };
    let mut widths: Vec<usize> = columns.iter().map(|c| c.chars().count()).collect();
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(clip(&unlink(c)).chars().count());
        }
    }
    let mut out = String::new();
    let head: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{c:<w$}", w = widths[i]))
        .collect();
    out.push_str(&format!("{}\n", head.join("  ").trim_end()));
    out.push_str(&format!(
        "{}\n",
        widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("  ")
    ));
    for r in rows {
        let cells: Vec<String> = r
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<w$}", clip(c), w = widths[i]))
            .collect();
        out.push_str(&format!("{}\n", cells.join("  ").trim_end()));
    }
    out
}
