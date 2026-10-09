//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A pack builds a `MessageContext` through its constructor, so a field added
//! later breaks no pack.
//!
//! The struct is `#[non_exhaustive]`, which only binds other crates, so what
//! keeps a literal out of a pack is the `compile_fail` example on the type, run
//! as a doctest. These hold what the constructor and its builders hand back.

use std::path::Path;

use crate::{AgentMode, Invocation, MessageContext, MessageDomain};

#[test]
fn a_new_context_has_no_invocation_and_no_identity() {
    // The shape of a forge body, which is why it is what `new` gives: a context
    // is told about a commit, never assumed to be one.
    let ctx = MessageContext::new(
        MessageDomain::PullRequestBody,
        AgentMode::Assistant,
        "a body",
        "pr-body",
        Path::new("/tmp"),
    );
    assert_eq!(ctx.domain, MessageDomain::PullRequestBody);
    assert_eq!(ctx.mode, AgentMode::Assistant);
    assert_eq!(ctx.message, "a body");
    assert_eq!(ctx.origin, "pr-body");
    assert_eq!(ctx.repo_root, Path::new("/tmp"));
    assert!(ctx.invocation.is_none());
    assert_eq!(ctx.author, None);
    assert_eq!(ctx.committer, None);
}

#[test]
fn identity_and_invocation_are_added_by_the_builders_and_nothing_else_moves() {
    let ctx = MessageContext::new(
        MessageDomain::CommitMessage,
        AgentMode::Autonomous,
        "feat: x",
        "COMMIT_EDITMSG",
        Path::new("/repo"),
    )
    .with_identity(
        Some("Jane Doe <jane@example.com>"),
        Some("Bob Roe <bob@example.com>"),
    )
    .with_invocation(Some(Invocation {
        command:   Some("git commit"),
        tool_name: Some("Bash"),
    }));
    assert_eq!(ctx.author, Some("Jane Doe <jane@example.com>"));
    assert_eq!(ctx.committer, Some("Bob Roe <bob@example.com>"));
    assert_eq!(ctx.invocation.and_then(|i| i.command), Some("git commit"));
    assert_eq!(ctx.domain, MessageDomain::CommitMessage);
    assert_eq!(ctx.mode, AgentMode::Autonomous);
    assert_eq!(ctx.message, "feat: x");
}

#[test]
fn the_two_identity_fields_are_set_independently() {
    let ctx = MessageContext::new(
        MessageDomain::CommitMessage,
        AgentMode::Assistant,
        "m",
        "o",
        Path::new("/tmp"),
    )
    .with_identity(Some("A <a@a>"), None);
    assert_eq!(ctx.author, Some("A <a@a>"));
    assert_eq!(ctx.committer, None);
}
