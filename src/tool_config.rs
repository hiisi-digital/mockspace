//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `[tools]` table of `mockspace.toml`: one table per tool, named as the tool is, holding the
//! values the project hands it.
//!
//! ```toml
//! [tools.tackle]
//! about_the_work = ["workflow"]
//! ```
//!
//! A value is a scalar or an array of scalars, and `{prefix}` in a string reads as the project's
//! crate prefix, as it does in `[lints]`. Anything else is refused when the config is read, with the
//! key named, and never dropped: a value the run could not read is a configuration it never applied.
//! Whether the tool reads a key is the tool's to say, so that is refused at the run
//! ([`ToolConfig::undeclared`]).

use std::path::Path;

use mockspace_lint_rules::tool::{ToolConfig, ToolParams};

/// Reads `[tools]` from the config's text. No table is no configuration.
///
/// # Panics
///
/// Where the text does not parse, where `tools` or a tool's entry is not a table, and where a value is
/// neither a scalar nor an array of scalars, naming `source` and the key.
pub fn parse(toml_content: &str, crate_prefix: &str, source: &Path) -> ToolConfig {
    let doc = match toml_content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(e) => panic!("{}: the file does not parse: {e}", source.display()),
    };
    let Some(item) = doc.get("tools") else { return ToolConfig::empty() };
    let Some(tools) = item.as_table_like() else {
        panic!(
            "{}: `tools` is of type {}, and it has to be a table of tool names. Nothing under it was \
             read, so every tool would run on its defaults.",
            source.display(),
            item.type_name(),
        )
    };
    let mut cfg = ToolConfig::empty();
    for (name, entry) in tools.iter() {
        let Some(table) = entry.as_table_like() else {
            refuse(
                source,
                name,
                &format!("it is {}, and a tool's entry is a table", entry.type_name()),
            )
        };
        let mut params = ToolParams::new();
        for (key, v) in table.iter() {
            let Some(v) = v.as_value() else {
                refuse(
                    source,
                    name,
                    &format!("`{key}` is a table, and a value is a scalar or an array"),
                )
            };
            let values = match v {
                toml_edit::Value::Array(a) => {
                    a.iter()
                        .enumerate()
                        .map(|(i, item)| {
                            scalar(source, name, &format!("{key}[{i}]"), item, crate_prefix)
                        })
                        .collect()
                },
                other => vec![scalar(source, name, key, other, crate_prefix)],
            };
            params.set(key, values);
        }
        cfg.params.insert(name.to_owned(), params);
    }
    cfg
}

/// A value as the text a tool reads: a string with `{prefix}` read, or a number's or boolean's TOML text.
fn scalar(source: &Path, tool: &str, key: &str, v: &toml_edit::Value, prefix: &str) -> String {
    use toml_edit::Value;
    match v {
        Value::String(s) => s.value().replace("{prefix}", prefix),
        Value::Integer(i) => i.value().to_string(),
        Value::Float(f) => f.value().to_string(),
        Value::Boolean(b) => b.value().to_string(),
        other => {
            refuse(
                source,
                tool,
                &format!(
                    "`{key}` is {}, and it takes a string, integer, float or boolean",
                    other.type_name()
                ),
            )
        },
    }
}

fn refuse(source: &Path, tool: &str, why: &str) -> ! {
    panic!(
        "{}: [tools.{tool}] was not read: {why}\n  The tool would run on its defaults instead, which is a \
         run that reports success over a configuration it never applied. Fix the entry or remove it.",
        source.display(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> ToolConfig {
        parse(text, "widget", Path::new("mockspace.toml"))
    }

    fn refusal(text: &'static str) -> String {
        let caught = std::panic::catch_unwind(|| read(text)).expect_err("the table is refused");
        caught.downcast_ref::<String>().cloned().unwrap_or_default()
    }

    #[test]
    fn a_scalar_is_one_value_and_an_array_its_values_in_order() {
        let cfg = read(
            "[tools.tackle]\nabout_the_work = [\"workflow\", \"{prefix}-core\"]\nlimit = 3\nloud = true\nroot = \"docs\"\n",
        );
        let p = &cfg.params["tackle"];
        assert_eq!(
            p.list("about_the_work"),
            Some(&["workflow".to_owned(), "widget-core".to_owned()][..])
        );
        assert_eq!(p.one("about_the_work"), None, "two values are not one");
        assert_eq!(p.one("limit"), Some("3"));
        assert_eq!(p.one("loud"), Some("true"));
        assert_eq!(p.one("root"), Some("docs"));
        assert_eq!(p.list("unset"), None);
    }

    #[test]
    fn an_empty_array_is_set_and_empty() {
        let cfg = read("[tools.tackle]\nabout_the_work = []\n");
        assert_eq!(cfg.params["tackle"].list("about_the_work"), Some(&[][..]));
    }

    #[test]
    fn the_inline_spelling_reads_the_same() {
        assert_eq!(
            read("tools = { tackle = { about_the_work = [\"workflow\"] } }\n"),
            read("[tools.tackle]\nabout_the_work = [\"workflow\"]\n")
        );
    }

    #[test]
    fn no_table_is_no_configuration() {
        assert!(read("project_name = \"x\"\n").is_empty());
    }

    #[test]
    fn a_value_that_is_neither_scalar_nor_array_is_refused_by_its_key() {
        let why = refusal("[tools.tackle]\nabout_the_work = { a = 1 }\n");
        assert!(
            why.contains("[tools.tackle]") && why.contains("`about_the_work`"),
            "{why}"
        );
        let why = refusal("[tools.tackle]\nabout_the_work = [[1]]\n");
        assert!(why.contains("`about_the_work[0]`"), "{why}");
    }

    #[test]
    fn an_entry_that_is_no_table_is_refused() {
        let why = refusal("[tools]\ntackle = \"workflow\"\n");
        assert!(
            why.contains("[tools.tackle]") && why.contains("a tool's entry is a table"),
            "{why}"
        );
        let why = refusal("tools = 3\n");
        assert!(why.contains("`tools` is of type"), "{why}");
    }
}
