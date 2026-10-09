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
    for (name, n) in [
        ("markers", t.markers.len()),
        ("mailboxes", t.mailboxes.len()),
        ("tags", t.tags.len()),
        ("tools", t.tools.len()),
        ("given", t.given.len()),
        ("vendors", t.vendors.len()),
        ("heads", t.heads.len()),
        ("companions", t.companions.len()),
        ("keyed", t.keyed.len()),
        ("people", t.people.len()),
        ("agents", t.agents.len()),
    ] {
        assert!(n > 0, "the table has no {name}");
    }
}

#[test]
fn a_line_that_is_not_a_row_is_an_error_and_not_skipped() {
    assert!(AgentIdentityConformance::parse("tool").is_err());
    assert!(AgentIdentityConformance::parse("tool\t").is_err());
    assert!(AgentIdentityConformance::parse("nonsense\tx").is_err());
    // a row that pairs two things needs both, and one that holds a single thing
    // takes no second
    assert!(AgentIdentityConformance::parse("vendor\tclaude").is_err());
    assert!(AgentIdentityConformance::parse("vendor\tclaude\t").is_err());
    assert!(AgentIdentityConformance::parse("keyed\tclaude").is_err());
    assert!(AgentIdentityConformance::parse("tool\tclaude\tx").is_err());
    assert!(AgentIdentityConformance::parse("tag\taider\tx").is_err());
    // and the comment and blank lines that are allowed are not errors
    let t = AgentIdentityConformance::parse("# a note\n\ntool\tclaude\n").unwrap();
    assert_eq!(t.tools, vec!["claude".to_string()]);
}

#[test]
fn a_pair_is_read_as_its_two_halves() {
    let t = AgentIdentityConformance::parse(
        "vendor\tclaude\tanthropic.com\nkeyed\tclaude\tclaude <root@buildhost.local>\n",
    )
    .unwrap();
    assert_eq!(t.vendors, vec![(
        "claude".to_string(),
        "anthropic.com".to_string()
    )]);
    assert_eq!(t.keyed, vec![(
        "claude".to_string(),
        "claude <root@buildhost.local>".to_string()
    )]);
}

#[test]
fn no_row_is_listed_twice_in_its_own_kind() {
    let t = table();
    for (name, rows) in [
        ("markers", &t.markers),
        ("mailboxes", &t.mailboxes),
        ("tags", &t.tags),
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
    for (name, rows) in [("vendors", &t.vendors), ("keyed", &t.keyed)] {
        let mut seen = std::collections::BTreeSet::new();
        for (a, b) in rows.iter() {
            assert!(
                seen.insert((a.to_lowercase(), b.to_lowercase())),
                "{name} lists `{a}` and `{b}` twice"
            );
        }
    }
}

#[test]
fn every_given_name_tool_has_a_vendor_domain_and_every_vendor_row_names_one() {
    // The second signal for a name that is the tool alone is the mailbox, and a
    // given name with no domain would have only its local part to go on. A
    // vendor row for a word that is no given name would be a domain nothing reads.
    let t = table();
    for g in &t.given {
        assert!(
            t.vendors.iter().any(|(tool, _)| tool == g),
            "`{g}` is a given-name tool with no vendor domain"
        );
    }
    for (tool, domain) in &t.vendors {
        assert!(
            t.given.contains(tool),
            "`{domain}` is a vendor domain of `{tool}`, which is no given-name tool"
        );
        assert!(
            domain.contains('.') && !domain.contains(char::is_whitespace) && !domain.contains('@'),
            "`{domain}` is no bare domain"
        );
    }
}

#[test]
fn a_keyed_identity_is_a_person_to_the_default_and_is_in_no_verdict_row() {
    // A keyed row says "a person until the project names the name", so listing
    // the same identity as a person or as an agent would say two things.
    let t = table();
    for (key, identity) in &t.keyed {
        assert!(
            !t.people.contains(identity) && !t.agents.contains(identity),
            "{identity}"
        );
        assert!(
            identity.to_lowercase().starts_with(&key.to_lowercase()),
            "`{identity}` does not carry the name `{key}` it is keyed on"
        );
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
