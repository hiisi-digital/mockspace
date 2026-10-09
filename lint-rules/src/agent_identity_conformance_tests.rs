//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The conformance table is well formed, so a suite reading it is reading rows.
//!
//! A table that parses to nothing makes every test over it pass, which is the
//! failure these guard: each list has rows, and a malformed line is an error
//! rather than a skipped one.

use super::{AgentIdentityConformance, TABLE};

fn table() -> AgentIdentityConformance {
    AgentIdentityConformance::parse(TABLE).expect("the shipped table parses")
}

#[test]
fn every_list_and_both_verdicts_have_rows() {
    let t = table();
    for (name, rows) in [
        ("markers", &t.markers),
        ("mailboxes", &t.mailboxes),
        ("tools", &t.tools),
        ("given", &t.given),
        ("heads", &t.heads),
        ("companions", &t.companions),
        ("people", &t.people),
        ("agents", &t.agents),
    ] {
        assert!(!rows.is_empty(), "the table has no {name}");
    }
}

#[test]
fn a_line_that_is_not_a_row_is_an_error_and_not_skipped() {
    assert!(AgentIdentityConformance::parse("tool").is_err());
    assert!(AgentIdentityConformance::parse("tool\t").is_err());
    assert!(AgentIdentityConformance::parse("nonsense\tx").is_err());
    // and the comment and blank lines that are allowed are not errors
    let t = AgentIdentityConformance::parse("# a note\n\ntool\tclaude\n").unwrap();
    assert_eq!(t.tools, vec!["claude".to_string()]);
}

#[test]
fn no_row_is_listed_twice_in_its_own_kind() {
    let t = table();
    for (name, rows) in [
        ("markers", &t.markers),
        ("mailboxes", &t.mailboxes),
        ("tools", &t.tools),
        ("given", &t.given),
        ("heads", &t.heads),
        ("companions", &t.companions),
        ("people", &t.people),
        ("agents", &t.agents),
    ] {
        let mut seen = std::collections::BTreeSet::new();
        for r in rows.iter() {
            assert!(seen.insert(r.to_lowercase()), "{name} lists `{r}` twice");
        }
    }
}

#[test]
fn a_given_name_tool_is_not_also_a_plain_tool() {
    // Two rows saying opposite things about one word are a table nobody can
    // implement, and the suites would each pick one.
    let t = table();
    for g in &t.given {
        assert!(
            !t.tools.contains(g),
            "`{g}` is both a tool and a given name"
        );
    }
}

#[test]
fn no_identity_is_both_a_person_and_an_agent() {
    let t = table();
    for p in &t.people {
        assert!(
            !t.agents.contains(p),
            "`{p}` is listed as a person and as an agent"
        );
    }
}
