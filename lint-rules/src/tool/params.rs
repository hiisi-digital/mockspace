//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a project hands a tool from its config: the keys under `[tools.<name>]` of
//! `mockspace.toml`.
//!
//! A tool shipped in a shared pack runs in projects that differ in what it should read, a subject
//! id, a directory, a list of names, and before this it had no way to be told any of that, so the
//! project's own values went into the pack's source as constants. A lint has had its parameters
//! from `[lints.<name>]` all along; this is the same carrier for the other half of a pack.
//!
//! A tool declares the keys it reads in [`super::Tool::config_keys`]. The engine hands it the
//! values of those keys once, when the pack loads, through [`super::Tool::configure`], and refuses
//! to run a tool whose table sets a key it did not declare, since a value nothing reads is a
//! configuration the run never applied.

use std::collections::BTreeMap;

use super::Tool;
use crate::LintPack;

/// One key a tool reads from its `[tools.<name>]` table, as `mock help <tool>` shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigKey {
    /// The key as the table spells it.
    pub name:        &'static str,
    /// One line: what the value is and what the tool does with it, and what it reads when unset.
    pub description: &'static str,
}

/// The values a project set for one tool, by key.
///
/// A scalar is one value and an array is its values in order, so a key read as a list takes either
/// spelling and a key read as one value takes a one-element array as well. Numbers and booleans
/// arrive as their TOML text, the way a lint's parameters do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolParams {
    values: BTreeMap<String, Vec<String>>,
}

impl ToolParams {
    /// No key set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `key` to `values`, replacing what it held.
    pub fn set(&mut self, key: impl Into<String>, values: Vec<String>) {
        self.values.insert(key.into(), values);
    }

    /// The values of `key` in order, `None` where the project did not set it. An empty array is set
    /// and empty, which is the project saying the list has nothing in it.
    #[must_use]
    pub fn list(&self, key: &str) -> Option<&[String]> {
        self.values.get(key).map(Vec::as_slice)
    }

    /// The value of `key` where it holds exactly one, `None` where it is unset or holds several.
    #[must_use]
    pub fn one(&self, key: &str) -> Option<&str> {
        match self.values.get(key).map(Vec::as_slice) {
            Some([v]) => Some(v.as_str()),
            _ => None,
        }
    }

    /// The keys set, in order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    /// Whether no key is set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// These values less every key `tool` does not declare.
    fn declared_by(&self, tool: &dyn Tool) -> Self {
        let keys = tool.config_keys();
        Self {
            values: self
                .values
                .iter()
                .filter(|(k, _)| keys.iter().any(|d| d.name == k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }
}

/// Every tool's values from the `[tools]` table, by tool name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolConfig {
    /// Tool name to its values.
    pub params: BTreeMap<String, ToolParams>,
}

impl ToolConfig {
    /// No `[tools]` table.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Whether no tool has a table.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// The keys the table of `tool` sets and `tool` does not declare, in order. Any of them means the
    /// project wrote a value the tool never reads, and the engine refuses to run it.
    #[must_use]
    pub fn undeclared(&self, tool: &dyn Tool) -> Vec<String> {
        let Some(params) = self.params.get(tool.name()) else { return Vec::new() };
        let declared = tool.config_keys();
        params
            .keys()
            .filter(|k| !declared.iter().any(|d| d.name == *k))
            .map(str::to_owned)
            .collect()
    }

    /// Hands `tool` the values of the keys it declares, where its table sets any. A key it does not
    /// declare never reaches it.
    pub fn apply_to(&self, tool: &mut dyn Tool) {
        if let Some(params) = self.params.get(tool.name()) {
            let declared = params.declared_by(tool);
            if !declared.is_empty() {
                tool.configure(&declared);
            }
        }
    }

    /// The names with a table and no tool of that name among `tools`, in order: a table for a tool
    /// the project does not have, or under a name it misspelled.
    #[must_use]
    pub fn absent(&self, tools: &[Box<dyn Tool>]) -> Vec<String> {
        self.params
            .keys()
            .filter(|n| !tools.iter().any(|t| t.name() == n.as_str()))
            .cloned()
            .collect()
    }
}

impl LintPack {
    /// Hands every tool in the pack the values its `[tools.<name>]` table sets.
    ///
    /// The tool half of [`LintPack::configure_from`], for the same reason: a tool is constructed by
    /// the `lint_pack!` macro, which can only call a constructor, and [`Tool::configure`] wants
    /// `&mut`, so it happens once, while the pack is still owned.
    pub fn configure_tools(&mut self, cfg: &ToolConfig) {
        for t in &mut self.tools {
            cfg.apply_to(t.as_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{NotALint, Purpose, ToolContext, ToolReport};

    struct Reads(&'static [ConfigKey], Option<ToolParams>);

    impl Tool for Reads {
        fn name(&self) -> &'static str {
            "reads"
        }

        fn description(&self) -> &'static str {
            ""
        }

        fn purpose(&self) -> Purpose {
            Purpose::Check(NotALint::TakesAQuestion)
        }

        fn config_keys(&self) -> &[ConfigKey] {
            self.0
        }

        fn configure(&mut self, params: &ToolParams) {
            self.1 = Some(params.clone());
        }

        fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
            unreachable!()
        }
    }

    const ONE: &[ConfigKey] = &[ConfigKey {
        name:        "subjects",
        description: "",
    }];

    fn cfg(keys: &[(&str, &[&str])]) -> ToolConfig {
        let mut p = ToolParams::new();
        for (k, v) in keys {
            p.set(*k, v.iter().map(|s| (*s).to_owned()).collect());
        }
        let mut c = ToolConfig::empty();
        c.params.insert("reads".to_owned(), p);
        c
    }

    #[test]
    fn one_is_a_single_value_and_list_is_every_value() {
        let c = cfg(&[("a", &["x"]), ("b", &["x", "y"]), ("c", &[])]);
        let p = &c.params["reads"];
        assert_eq!(p.one("a"), Some("x"));
        assert_eq!(p.one("b"), None);
        assert_eq!(p.one("c"), None);
        assert_eq!(p.list("b").map(<[String]>::len), Some(2));
        assert_eq!(p.list("c").map(<[String]>::len), Some(0));
        assert_eq!(p.list("d"), None);
    }

    #[test]
    fn only_the_declared_keys_are_handed_over_and_the_rest_are_named() {
        let c = cfg(&[("subjects", &["workflow"]), ("subject", &["workflow"])]);
        let mut t = Reads(ONE, None);
        assert_eq!(c.undeclared(&t), vec!["subject".to_owned()]);
        c.apply_to(&mut t);
        let handed =
            t.1.expect("a declared key was set, so the tool is configured");
        assert_eq!(handed.keys().collect::<Vec<_>>(), vec!["subjects"]);
    }

    #[test]
    fn a_table_of_only_undeclared_keys_configures_nothing() {
        let c = cfg(&[("subject", &["workflow"])]);
        let mut t = Reads(ONE, None);
        c.apply_to(&mut t);
        assert!(t.1.is_none());
        // THE CONTROL: a tool that declares nothing has every key named.
        assert_eq!(c.undeclared(&Reads(&[], None)), vec!["subject".to_owned()]);
    }

    #[test]
    fn a_table_with_no_tool_of_its_name_is_absent() {
        let mut c = cfg(&[("subjects", &["workflow"])]);
        c.params.insert("raeds".to_owned(), ToolParams::new());
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(Reads(ONE, None))];
        assert_eq!(c.absent(&tools), vec!["raeds".to_owned()]);
    }
}
