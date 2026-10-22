//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A registry file cut into blocks along its table headers, losslessly.
//!
//! The merge works on text, because the one thing it must not do is rewrite
//! rows nobody touched. `toml_edit` is asked where each header sits and where
//! its content ends (spans, which only an `ImDocument` carries), and nothing
//! else is taken from it: a block is a stretch of the original bytes.
//!
//! A file is a `head` (everything before the first block, scalars and file
//! comments) and a run of blocks. A block is one `[[namespace]]` row with its
//! subtables, or one plain table, and is keyed by what identifies it: the
//! namespace and `id` for a row, the dotted path for a table. Never by where in
//! the file it sits or what the file is called.
//!
//! Each block has a `chunk` and a `tail`. The chunk is the row itself, with the
//! comment lines directly above its header (no blank line between) and any
//! comment on its last line, so a comment describing a row travels with it. The
//! tail is whatever follows up to the next chunk: the newline ending the row,
//! blank lines, and detached comments. `head + (chunk + tail)*` is the original
//! text, byte for byte.

use std::collections::HashSet;

use toml_edit::{ImDocument, Item, Table};

/// What identifies a block.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Key {
    /// A `[[namespace]]` entry and its `id`.
    Row {
        ns: String,
        id: String,
    },
    /// A plain `[table]`, by its dotted path.
    Table(String),
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Key::Row {
                ns,
                id,
            } => write!(f, "[[{ns}]] id = {id:?}"),
            Key::Table(path) => write!(f, "[{path}]"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) key:   Key,
    pub(crate) chunk: String,
    pub(crate) tail:  String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Doc {
    pub(crate) head:   String,
    pub(crate) blocks: Vec<Block>,
}

/// Why a file is not cut into blocks. Every one of these sends the merge to
/// git's own text merge, which is what happens without a driver.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Unsupported(pub(crate) String);

impl Doc {
    pub(crate) fn render(&self) -> String {
        let mut out = self.head.clone();
        for b in &self.blocks {
            out.push_str(&b.chunk);
            out.push_str(&b.tail);
        }
        out
    }

    pub(crate) fn keys(&self) -> Vec<&Key> {
        self.blocks.iter().map(|b| &b.key).collect()
    }
}

struct Header {
    start: usize,
    end:   usize,
    path:  Vec<String>,
    array: bool,
    id:    Option<String>,
}

fn collect(path: &mut Vec<String>, t: &Table, out: &mut Vec<Header>) -> Result<(), Unsupported> {
    for (k, item) in t.iter() {
        path.push(k.to_string());
        match item {
            Item::Table(sub) if sub.is_dotted() => {},
            Item::Table(sub) => {
                if let Some(span) = sub.span() {
                    out.push(Header {
                        start: span.start,
                        end:   span.end,
                        path:  path.clone(),
                        array: false,
                        id:    None,
                    });
                }
                collect(path, sub, out)?;
            },
            Item::ArrayOfTables(arr) => {
                for elem in arr.iter() {
                    let Some(span) = elem.span() else {
                        return Err(Unsupported(format!("no span for an entry of [[{k}]]")));
                    };
                    out.push(Header {
                        start: span.start,
                        end:   span.end,
                        path:  path.clone(),
                        array: true,
                        id:    elem.get("id").and_then(|v| v.as_str()).map(str::to_string),
                    });
                    collect(path, elem, out)?;
                }
            },
            _ => {},
        }
        path.pop();
    }
    Ok(())
}

struct Raw {
    key:   Key,
    root:  Vec<String>,
    start: usize,
    end:   usize,
}

fn line_start(text: &str, at: usize) -> usize {
    text[.. at].rfind('\n').map_or(0, |i| i + 1)
}

/// The end of the line `at` sits on, before its line break.
fn line_end(text: &str, at: usize) -> usize {
    let end = text[at ..].find('\n').map_or(text.len(), |i| at + i);
    if end > at && text.as_bytes()[end - 1] == b'\r' { end - 1 } else { end }
}

/// Where the comment lines directly above a header start, no earlier than
/// `lower`.
fn prefix_start(text: &str, header: usize, lower: usize) -> usize {
    let mut pos = line_start(text, header);
    while pos > lower {
        let prev = line_start(text, pos - 1);
        if prev < lower || !text[prev .. pos].trim_start().starts_with('#') {
            break;
        }
        pos = prev;
    }
    pos
}

impl Doc {
    pub(crate) fn parse(text: &str) -> Result<Doc, Unsupported> {
        let im =
            ImDocument::parse(text).map_err(|e| Unsupported(format!("not valid TOML: {e}")))?;
        let mut headers = Vec::new();
        collect(&mut Vec::new(), im.as_table(), &mut headers)?;
        headers.sort_by_key(|h| h.start);

        let mut raws: Vec<Raw> = Vec::new();
        for h in headers {
            if let Some(cur) = raws.last_mut() {
                let same_entry = h.array && h.path == cur.root;
                if !same_entry && h.path.starts_with(&cur.root) {
                    cur.end = cur.end.max(h.end);
                    continue;
                }
            }
            let key = if h.array {
                if h.path.len() != 1 {
                    return Err(Unsupported(format!(
                        "[[{}]] sits outside any row of its parent",
                        h.path.join(".")
                    )));
                }
                let Some(id) = h.id else {
                    return Err(Unsupported(format!(
                        "an entry of [[{}]] has no string id",
                        h.path[0]
                    )));
                };
                Key::Row {
                    ns: h.path[0].clone(),
                    id,
                }
            } else {
                Key::Table(h.path.join("."))
            };
            raws.push(Raw {
                key,
                root: h.path,
                start: h.start,
                end: h.end,
            });
        }

        let mut seen = HashSet::new();
        for r in &raws {
            if !seen.insert(&r.key) {
                return Err(Unsupported(format!("{} appears twice in one file", r.key)));
            }
        }

        let mut spans: Vec<(usize, usize)> = Vec::new();
        let mut lower = 0;
        for r in &raws {
            let start = prefix_start(text, r.start, lower);
            let end = line_end(text, r.end).max(line_end(text, r.start));
            spans.push((start, end));
            lower = end;
        }

        let head_end = spans.first().map_or(text.len(), |s| s.0);
        let mut blocks = Vec::new();
        for (i, r) in raws.into_iter().enumerate() {
            let (start, end) = spans[i];
            let next = spans.get(i + 1).map_or(text.len(), |s| s.0);
            blocks.push(Block {
                key:   r.key,
                chunk: text[start .. end].to_string(),
                tail:  text[end .. next].to_string(),
            });
        }
        let doc = Doc {
            head: text[.. head_end].to_string(),
            blocks,
        };
        debug_assert_eq!(doc.render(), text);
        Ok(doc)
    }
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
