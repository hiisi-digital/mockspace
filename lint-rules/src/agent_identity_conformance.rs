//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Test data for the recognisers of an agent identity. Not part of the plugin API.
//!
//! Naming an agent from a commit's author, committer or `Co-Authored-By` is
//! written more than once, and two writings kept in step by hand drift, silently,
//! because each passes its own suite. This table is the one thing they share: the
//! lists the recogniser is built from, and the people and agents it has to tell
//! apart, one row each.
//!
//! A pack's tests read it through this module, so they run against the table at
//! the revision of this crate they were built with and there is no second copy to
//! fall out of date. The module is hidden from the documentation because it is
//! data for tests and carries no promise to a pack's lints. The format and the
//! rule each list stands for are written at the top of
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
    /// Tags a tool writes into a name, read as a group in parentheses.
    pub tags:       Vec<String>,
    /// Tool names, an agent's as the start of a name when nothing but companions
    /// and versions follow.
    pub tools:      Vec<String>,
    /// Tool names that are also given names, which need a second signal.
    pub given:      Vec<String>,
    /// A given-name tool and a domain it commits from, one pair per row.
    pub vendors:    Vec<(String, String)>,
    /// Endings only a private network's domains have, written without the dot.
    pub machines:   Vec<String>,
    /// Vendor words allowed in front of a tool's name.
    pub heads:      Vec<String>,
    /// Words allowed after a tool's name.
    pub companions: Vec<String>,
    /// A name a project has to name for the identity to read as an agent's, and
    /// the identity that then does.
    pub keyed:      Vec<(String, String)>,
    /// Identities that are never an agent.
    pub people:     Vec<String>,
    /// Identities that are always an agent.
    pub agents:     Vec<String>,
}

impl AgentIdentityConformance {
    /// Read a table. A line that is not a comment, not blank and not a
    /// `kind<TAB>value` row of a known kind is an error and not skipped, since a
    /// row that silently fails to count makes a suite over the table pass for
    /// less than it says. A kind that pairs two things takes both, and a kind
    /// that holds one takes no second.
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
            let at = n + 1;
            let Some((kind, value)) = line.split_once('\t') else {
                return Err(format!("line {at}: no tab between a kind and a value"));
            };
            let value = value.trim();
            if value.is_empty() {
                return Err(format!("line {at}: `{kind}` has no value"));
            }
            if matches!(kind, "vendor" | "keyed") {
                let Some((first, second)) = value.split_once('\t') else {
                    return Err(format!("line {at}: `{kind}` wants two values"));
                };
                let (first, second) = (first.trim(), second.trim());
                if first.is_empty() || second.is_empty() || second.contains('\t') {
                    return Err(format!("line {at}: `{kind}` wants two values"));
                }
                let pair = (first.to_string(), second.to_string());
                if kind == "vendor" {
                    t.vendors.push(pair);
                } else {
                    t.keyed.push(pair);
                }
                continue;
            }
            if value.contains('\t') {
                return Err(format!("line {at}: `{kind}` takes one value"));
            }
            let list = match kind {
                "marker" => &mut t.markers,
                "mailbox" => &mut t.mailboxes,
                "tag" => &mut t.tags,
                "tool" => &mut t.tools,
                "given" => &mut t.given,
                "machine" => &mut t.machines,
                "head" => &mut t.heads,
                "companion" => &mut t.companions,
                "person" => &mut t.people,
                "agent" => &mut t.agents,
                other => return Err(format!("line {at}: unknown kind `{other}`")),
            };
            list.push(value.to_string());
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
