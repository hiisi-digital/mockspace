//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The table that says what an agent identity is, for every implementation of it.
//!
//! Naming an agent from a commit's author, committer or `Co-Authored-By` is
//! written more than once: in shell, where review sweeps read it, and in Rust,
//! where a lint pack does. Two implementations kept in step by hand drift, and the
//! drift is silent because each passes its own suite. This table is the one thing
//! they share: the lists the recogniser is built from, and the people and agents
//! it has to tell apart, one row each.
//!
//! A pack's tests read it through this module, so they run against the table at
//! the revision of this crate they were built with and there is no second copy
//! to fall out of date. The shell suite reads the file. The format and the rule
//! each list stands for are written at the top of
//! `data/agent_identity_conformance.tsv`.
//!
//! ```
//! use mockspace_lint_rules::agent_identity_conformance::table;
//!
//! let t = table();
//! assert!(t.agents.iter().any(|a| a.starts_with("Claude")));
//! assert!(t.people.iter().any(|p| p.starts_with("Claude Monet")));
//! ```

/// The table as shipped, verbatim.
pub const TABLE: &str = include_str!("../data/agent_identity_conformance.tsv");

/// The rows of the table, by kind, in the order they are written.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AgentIdentityConformance {
    /// Bot markers, matched anywhere in the name or the mailbox.
    pub markers:    Vec<String>,
    /// Mailbox globs an agent commits from, matched against the whole mailbox.
    pub mailboxes:  Vec<String>,
    /// Tool names, which count as the whole of a name.
    pub tools:      Vec<String>,
    /// Tool names that are also given names, which need a second signal.
    pub given:      Vec<String>,
    /// Vendor words allowed in front of a tool's name.
    pub heads:      Vec<String>,
    /// Words allowed after a tool's name.
    pub companions: Vec<String>,
    /// Identities that are never an agent.
    pub people:     Vec<String>,
    /// Identities that are always an agent.
    pub agents:     Vec<String>,
}

impl AgentIdentityConformance {
    /// Read a table. A line that is not a comment, not blank and not a
    /// `kind<TAB>value` row of a known kind is an error and not skipped, since a
    /// row that silently fails to count makes a suite over the table pass for
    /// less than it says.
    ///
    /// # Errors
    ///
    /// The first malformed line, with its number.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut t = Self::default();
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((kind, value)) = line.split_once('\t') else {
                return Err(format!("line {}: no tab between a kind and a value", n + 1));
            };
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err(format!("line {}: `{kind}` has no value", n + 1));
            }
            let list = match kind {
                "marker" => &mut t.markers,
                "mailbox" => &mut t.mailboxes,
                "tool" => &mut t.tools,
                "given" => &mut t.given,
                "head" => &mut t.heads,
                "companion" => &mut t.companions,
                "person" => &mut t.people,
                "agent" => &mut t.agents,
                other => return Err(format!("line {}: unknown kind `{other}`", n + 1)),
            };
            list.push(value);
        }
        Ok(t)
    }
}

/// The shipped table, parsed.
///
/// # Panics
///
/// When the table shipped with this crate does not parse, which its own tests
/// rule out.
#[must_use]
pub fn table() -> AgentIdentityConformance {
    AgentIdentityConformance::parse(TABLE).expect("the shipped conformance table parses")
}

#[cfg(test)]
#[path = "agent_identity_conformance_tests.rs"]
mod tests;
