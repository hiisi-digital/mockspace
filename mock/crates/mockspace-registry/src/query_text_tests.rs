//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A reference's text before the walk: the one spelling rule, the registry's paths, the operation
//! taken off, a repeated set member.

use super::*;

#[test]
fn a_namespace_is_named_as_declared_or_folded() {
    let keys = ["web-renderer", "ticket"];
    assert_eq!(namespace_named("web-renderer", keys), Some("web-renderer"));
    assert_eq!(namespace_named("web_renderer", keys), Some("web-renderer"));
    assert_eq!(namespace_named("ticket", keys), Some("ticket"));
    assert_eq!(namespace_named("tickets", keys), None);
}

#[test]
fn the_host_spelling_is_folded_in_the_first_step_only() {
    assert_eq!(standard_spelling("web-renderer::pass").unwrap(), "web_renderer::pass");
    assert_eq!(standard_spelling("reg::web-renderer::pass::{a, b}").unwrap(), "reg::web_renderer::pass::{a, b}");
    assert_eq!(standard_spelling("ticket::a").unwrap(), "ticket::a");
    assert!(standard_spelling("ticket::a-b").is_err());
    assert!(standard_spelling("web-renderer").is_err(), "a name alone is the standard's spelling alone");
}

#[test]
fn a_registry_path_opens_on_a_namespace_or_widens_at_the_root() {
    let nss = ["web-renderer", "ticket"];
    assert_eq!(registry_path("web-renderer::pass::split", nss).as_deref(), Some("web_renderer::pass::split"));
    assert_eq!(registry_path("reg::ticket::{a}", nss).as_deref(), Some("reg::ticket::{a}"));
    assert_eq!(registry_path("{ticket, web_renderer}::*", nss).as_deref(), Some("{ticket, web_renderer}::*"));
    assert_eq!(registry_path("mock::design::12", nss), None, "a citation");
    assert_eq!(registry_path("pathof(ticket::a)", nss), None, "an operation");
}

#[test]
fn an_operation_is_taken_off_in_one_place() {
    assert_eq!(peel("pathof(ticket::a)"), (Some("pathof"), "ticket::a"));
    assert_eq!(peel("refsto(ticket::{a, b})"), (Some("refsto"), "ticket::{a, b}"));
    assert_eq!(peel("sourcesof(law::keys)"), (Some("sourcesof"), "law::keys"));
    assert_eq!(peel("ticket::a"), (None, "ticket::a"));
}

#[test]
fn a_repeated_set_member_is_named_once() {
    assert_eq!(repeated_set_members("ticket::{task, task}::engine::b"), ["task"]);
    assert_eq!(repeated_set_members("ticket::{a, b, a, a}::{c, c}"), ["a", "c"]);
    assert!(repeated_set_members("ticket::{a, b}").is_empty());
}
