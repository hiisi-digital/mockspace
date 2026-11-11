//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The text edit: one string added to a `string[]` field of one row.
//!
//! The file is cut into blocks by the merge driver's scanner, so every byte outside the row's
//! own block is returned as it came. Inside the block `toml_edit` keeps the layout of what it
//! does not touch.

use toml_edit::{Array, DocumentMut, Item, Value};

use crate::registry_merge::scan::{Doc, Key};

/// `text` with `value` added to `field` of the row `ns` / `id`, or `None` where the field
/// already holds it. A field the row does not have yet is made as a one-entry array.
pub(crate) fn append(
    text: &str,
    ns: &str,
    id: &str,
    field: &str,
    value: &str,
) -> Result<Option<String>, String> {
    let mut doc = Doc::parse(text).map_err(|e| e.0)?;
    let key = Key::Row {
        ns: ns.to_string(),
        id: id.to_string(),
    };
    let block = doc
        .blocks
        .iter_mut()
        .find(|b| b.key == key)
        .ok_or_else(|| format!("no [[{ns}]] row {id:?} in this file"))?;
    let mut edit: DocumentMut = block
        .chunk
        .parse()
        .map_err(|e| format!("the row {ns}::{id} does not read as a document: {e}"))?;
    let table = edit
        .get_mut(ns)
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|t| t.get_mut(0))
        .ok_or_else(|| format!("the row {ns}::{id} has no [[{ns}]] table"))?;
    match table.get_mut(field) {
        Some(item) => {
            let arr = item
                .as_array_mut()
                .ok_or_else(|| format!("{ns}::{id} holds `{field}` as something other than a list"))?;
            if arr.iter().any(|v| v.as_str() == Some(value)) {
                return Ok(None);
            }
            push(arr, value);
        },
        None => {
            let mut arr = Array::new();
            arr.push(value);
            table.insert(field, Item::Value(Value::Array(arr)));
        },
    }
    // The chunk is cut where the row ends and its newlines belong to the tail, so the edited
    // text keeps exactly the newlines the chunk had.
    let ends = block.chunk.len() - block.chunk.trim_end_matches('\n').len();
    let mut next = edit.to_string();
    next.truncate(next.trim_end_matches('\n').len());
    next.push_str(&"\n".repeat(ends));
    block.chunk = next;
    Ok(Some(doc.render()))
}

/// Adds one string in the layout the array already has: on its own line, indented as the
/// entries before it are, where the array is written over several lines.
fn push(arr: &mut Array, value: &str) {
    let spread = arr.iter().any(|v| {
        v.decor()
            .prefix()
            .and_then(|p| p.as_str())
            .is_some_and(|p| p.contains('\n'))
    });
    if !spread {
        arr.push(value);
        return;
    }
    let indent = arr
        .iter()
        .filter_map(|v| v.decor().prefix().and_then(|p| p.as_str()))
        .find(|p| p.contains('\n'))
        .unwrap_or("\n  ")
        .to_string();
    let had_trailing = arr.trailing_comma();
    // A comment after the last comma belongs to the entry before it, so it moves in front of the
    // new entry's line and the closing bracket keeps the rest.
    let tail = arr.trailing().as_str().unwrap_or_default().to_string();
    let (first, rest) = tail.split_once('\n').unwrap_or((tail.as_str(), ""));
    let prefix = if first.trim_start().starts_with('#') {
        arr.set_trailing(format!("\n{rest}"));
        format!("{first}{indent}")
    } else {
        indent
    };
    let mut v = Value::from(value);
    v.decor_mut().set_prefix(prefix);
    arr.push_formatted(v);
    arr.set_trailing_comma(had_trailing || arr.len() > 1);
}
