//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `check-message` entry: lint an authored commit message or forge body.
//!
//! This is how the gates reach message lints. A `commit-msg` hook passes the
//! message file; an agent hook passes text extracted from a command it is about
//! to run, plus the command itself so a lint can inspect the invocation.
//!
//! # Why this replaced a bash regex
//!
//! Byline enforcement used to be a hardcoded `grep -E` baked into the generated
//! hooks, chosen so it would hold with no launcher installed. That robustness was
//! real, but it cost more than it bought: the pattern could not express a
//! project's policy, the same pattern was duplicated into two hook layers that a
//! comment conceded "MUST stay in sync", and it contradicted the configured
//! policy outright, since it rejected unconditionally what
//! `[attribution] autonomous` was meant to require.
//!
//! With no launcher installed the gate now fails closed and says how to install
//! one, rather than falling back to a second policy that can disagree with the
//! first. That is the same treatment every other anomalous state gets: error,
//! inform, guide.
//!
//! # Who made the commit
//!
//! A message does not say who its commit is by, and an agent committing under
//! its own name carries no trailer to find, so commits made under a container's
//! default identity passed every message check, there being nothing in the
//! messages to check. The commit gate and the push gate therefore hand the lints
//! the commit's author and committer as well, read by the `commit-msg` hook from
//! `git var` and by the `pre-push` hook from each pushed commit, so a policy about
//! agent bylines reaches an agent identity under the same mode. A forge body has
//! neither, and its lints are handed none.

use std::path::Path;
use std::process::ExitCode;

use mockspace_lint_rules::{AgentMode, Level, LintMode, LintPack, MessageContext, MessageDomain};

use crate::agent_mode;
use crate::config::Config;

/// Parse a domain token as written on the command line.
pub(crate) fn parse_domain(s: &str) -> Option<MessageDomain> {
    match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "commit-message" | "commit" | "commit-msg" => Some(MessageDomain::CommitMessage),
        "pull-request-body" | "pr-body" | "pr" | "mr" | "merge-request-body" => {
            Some(MessageDomain::PullRequestBody)
        },
        "issue-comment" | "issue" => Some(MessageDomain::IssueComment),
        "review-comment" | "review" => Some(MessageDomain::ReviewComment),
        _ => None,
    }
}

/// Every domain token this build accepts, for error messages.
pub(crate) const DOMAIN_TOKENS: &[&str] =
    &["commit-message", "pull-request-body", "issue-comment", "review-comment"];

/// What to lint, and where it came from.
pub(crate) struct Request<'a> {
    /// Which kind of message this is.
    pub domain:    MessageDomain,
    /// The authored text.
    pub message:   String,
    /// Where the text came from, for error reporting.
    pub origin:    String,
    /// The command being intercepted, when an agent hook is the caller.
    pub command:   Option<&'a str>,
    /// The tool being intercepted, when an agent hook is the caller.
    pub tool:      Option<&'a str>,
    /// Who authored the commit, as `Name <mailbox>`. Only a commit message has one,
    /// and a lint is handed it only for that domain.
    pub author:    Option<&'a str>,
    /// Who committed it, under the same terms as `author`.
    pub committer: Option<&'a str>,
}

/// Lint one message. Returns failure when any finding blocks at `mode`.
pub(crate) fn run(cfg: &Config, pack: &LintPack, mode: LintMode, req: &Request) -> ExitCode {
    // An unknown preset name would quietly weaken the predicate, so it is an
    // error rather than something to route around, and it is checked before
    // the empty-lints early return below: a typo'd preset in a repo that
    // happens to ship no message lints is still a typo'd preset.
    let unknown = agent_mode::unknown_presets(&cfg.agent.attribution.mode_signals);
    if !unknown.is_empty() {
        eprintln!(
            "mock: unknown agent-mode preset(s): {}. known: {}",
            unknown.join(", "),
            agent_mode::KNOWN_PRESETS.join(", ")
        );
        return ExitCode::FAILURE;
    }

    // No message lints means the project imported no pack that ships one, which
    // is a legitimate state: mockspace itself has no opinion about commit style.
    if pack.message_lints.is_empty() {
        return ExitCode::SUCCESS;
    }

    let signals = if cfg.agent.attribution.mode_signals.is_empty() {
        agent_mode::default_signals(&crate::render_agent::agent_mode_var(&cfg.project_name))
    } else {
        agent_mode::expand(&cfg.agent.attribution.mode_signals)
    };

    let resolved = agent_mode::resolve_from_env(&signals);

    // Only a commit message has an author and a committer. A forge body never
    // passes through git, so whatever a caller sent along with one names nobody
    // and is not handed on: the lint sees absent, as it would have without it.
    let (author_of, committer_of) = match req.domain {
        MessageDomain::CommitMessage => (req.author, req.committer),
        _ => (None, None),
    };
    let ctx = MessageContext::new(
        req.domain,
        resolved,
        &req.message,
        &req.origin,
        &cfg.repo_root,
    )
    .with_invocation(message_invocation(req))
    .with_identity(author_of, committer_of);

    let findings = mockspace_lint_rules::check_message_with_extra(
        &ctx,
        Some(&cfg.lint_overrides),
        &pack.message_lints,
    );

    report(&findings, mode, resolved, req)
}

/// The invocation to hand lints, when there is one.
fn message_invocation<'a>(req: &Request<'a>) -> Option<mockspace_lint_rules::Invocation<'a>> {
    if req.command.is_none() && req.tool.is_none() {
        return None;
    }
    Some(mockspace_lint_rules::Invocation {
        command:   req.command,
        tool_name: req.tool,
    })
}

/// Print findings and decide the exit code.
fn report(
    findings: &[mockspace_lint_rules::LintError],
    mode: LintMode,
    resolved: AgentMode,
    req: &Request,
) -> ExitCode {
    let mut blocking = 0usize;
    let mut warned = 0usize;
    for f in findings {
        match f.severity.effective(mode) {
            Level::Pass => {},
            Level::Info | Level::Warn => {
                warned += 1;
                eprintln!("  ! {} [{}]: {}", f.lint_name, req.origin, f.message);
            },
            Level::Error => {
                blocking += 1;
                eprintln!("  x {} [{}]: {}", f.lint_name, req.origin, f.message);
            },
        }
    }

    if blocking > 0 {
        eprintln!();
        eprintln!(
            "BLOCKED: {blocking} message violation(s) in this {}.",
            domain_label(req.domain)
        );
        // Naming the resolved mode matters: the whole policy turns on it, and a
        // surprising verdict is nearly always a surprising mode.
        eprintln!("  resolved agent mode: {}", resolved.as_token());
        eprintln!("  policy comes from mock/agent/config.toml [attribution].");
        return ExitCode::FAILURE;
    }
    if warned > 0 {
        eprintln!("  {warned} message warning(s).");
    }
    ExitCode::SUCCESS
}

fn domain_label(d: MessageDomain) -> &'static str {
    match d {
        MessageDomain::CommitMessage => "commit message",
        MessageDomain::PullRequestBody => "pull-request body",
        MessageDomain::IssueComment => "issue comment",
        MessageDomain::ReviewComment => "review comment",
    }
}

/// Read the authored text a `--file` argument points at.
pub(crate) fn read_message_file(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|e| format!("could not read the message file {}: {e}", path.display()))
}

/// An identity as a lint is handed it: `Name <mailbox>`, the way git writes the
/// author and committer of a commit, without the date and zone that `git var`
/// appends.
///
/// `None` for an input with nothing in it, so a lint can tell a commit nobody
/// named from one whose name is blank.
pub(crate) fn clean_ident(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // The mailbox closes the identity. Whatever follows the last `>` is the
    // timestamp and zone `git var` adds, and a name cannot hold a `>` since git
    // strips it, so the last one is always the mailbox's own.
    match raw.rfind('>') {
        Some(end) => Some(raw[..= end].to_string()),
        None => Some(raw.to_string()),
    }
}

/// One message out of a `--batch` stream, with the identity its commit carried
/// when the stream gave one.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Record {
    /// What names the message in a report: the commit's short hash and subject.
    pub origin:    String,
    /// The authored text.
    pub message:   String,
    /// The commit's author, absent when the record carried none.
    pub author:    Option<String>,
    /// The commit's committer, absent when the record carried none.
    pub committer: Option<String>,
    /// Why the header did not parse, when it did not. A record with a fault is
    /// refused by the caller and never handed to a lint, since what it held of an
    /// identity cannot be told from what was cut off.
    pub fault:     Option<String>,
}

/// Separates the fields of a record's header, before its first `\x1f`.
const HEADER_FIELD: char = '\x1e';

/// Split a `--batch` stream into its records.
///
/// The stream is NUL-terminated records. A record is `<header>\x1f<message>`,
/// where the header is either the origin alone, or
/// `<author>\x1e<committer>\x1e<origin>` when the caller knows who made the
/// commit. A record with no `\x1f` is the whole message, taking `default_origin`.
///
/// The identity rides in the header so an engine reading a stream from an older
/// hook, which sends the origin alone, still gets the message whole, and an older
/// engine reading a newer hook's stream gets the message whole with a longer
/// origin than it expected. A header holding no `\x1e` is an origin and one
/// holding two is an identity header. One is neither, and neither is a pair whose
/// halves are not each a name and a mailbox: that is a header cut short, or a name
/// that held the byte, and the record carries a fault for the caller to refuse
/// instead of guessing at the half that arrived.
///
/// An empty message is a record and not noise. Under a permissive commit-style
/// config `empty-subject` is the only finding the lint can produce, so dropping
/// empty records here would turn the push gate into a no-op. The only thing
/// skipped is a completely empty record, which is the tail left by the trailing
/// separator.
pub(crate) fn split_batch(text: &str, default_origin: &str) -> Vec<Record> {
    text.split('\0')
        .filter(|r| !r.is_empty())
        .map(|r| {
            match r.split_once('\x1f') {
                Some((header, message)) => record_from(header, message),
                None => {
                    Record {
                        origin:    default_origin.to_string(),
                        message:   r.to_string(),
                        author:    None,
                        committer: None,
                        fault:     None,
                    }
                },
            }
        })
        .collect()
}

/// A record out of its header and message, reading the identity when the header
/// carries one.
fn record_from(header: &str, message: &str) -> Record {
    let whole = |origin: &str, fault: Option<String>| {
        Record {
            origin: origin.to_string(),
            message: message.to_string(),
            author: None,
            committer: None,
            fault,
        }
    };
    if !header.contains(HEADER_FIELD) {
        return whole(header, None);
    }

    let mut fields = header.splitn(3, HEADER_FIELD);
    let (Some(author), Some(committer), Some(origin)) =
        (fields.next(), fields.next(), fields.next())
    else {
        return whole(
            header,
            Some("it holds one identity separator where two are expected".to_string()),
        );
    };
    for (who, field) in [("author", author), ("committer", committer)] {
        let field = field.trim();
        if !field.is_empty() && !field.ends_with('>') {
            return whole(
                header,
                Some(format!("the {who} `{field}` is not a name and a mailbox")),
            );
        }
    }
    Record {
        origin:    origin.to_string(),
        message:   message.to_string(),
        author:    clean_ident(author),
        committer: clean_ident(committer),
        fault:     None,
    }
}

/// Check every record of a `--batch` stream on its own, and return how many there
/// were and how many were refused.
///
/// A record whose header does not parse is refused here, without a lint, so a
/// stream that lost the identity of a commit fails the push and does not pass it
/// as though the commit had none.
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_batch(
    cfg: &Config,
    pack: &LintPack,
    gate: LintMode,
    domain: MessageDomain,
    text: &str,
    default_origin: &str,
    command: Option<&str>,
    tool: Option<&str>,
) -> (usize, usize) {
    let mut checked = 0usize;
    let mut failed = 0usize;
    for rec in split_batch(text, default_origin) {
        checked += 1;
        if let Some(fault) = &rec.fault {
            eprintln!(
                "  x [{}]: the record's header does not parse: {fault}",
                rec.origin
            );
            failed += 1;
            continue;
        }
        let req = Request {
            domain,
            message: rec.message,
            origin: rec.origin,
            command,
            tool,
            author: rec.author.as_deref(),
            committer: rec.committer.as_deref(),
        };
        if run(cfg, pack, gate, &req) != ExitCode::SUCCESS {
            failed += 1;
        }
    }
    (checked, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_tokens_parse_in_the_forms_a_hook_would_write() {
        assert_eq!(
            parse_domain("commit-msg"),
            Some(MessageDomain::CommitMessage)
        );
        assert_eq!(
            parse_domain("commit_message"),
            Some(MessageDomain::CommitMessage)
        );
        assert_eq!(parse_domain("COMMIT"), Some(MessageDomain::CommitMessage));
        assert_eq!(
            parse_domain("pr-body"),
            Some(MessageDomain::PullRequestBody)
        );
        assert_eq!(parse_domain("mr"), Some(MessageDomain::PullRequestBody));
        assert_eq!(parse_domain("review"), Some(MessageDomain::ReviewComment));
        assert_eq!(parse_domain("nonsense"), None);
    }

    #[test]
    fn every_advertised_domain_token_actually_parses() {
        // The error message lists these, so a token it names must work.
        for t in DOMAIN_TOKENS {
            assert!(
                parse_domain(t).is_some(),
                "{t} is advertised but does not parse"
            );
        }
    }

    #[test]
    fn an_invocation_is_absent_unless_a_hook_supplied_one() {
        // A `commit-msg` run has no command to report; an agent-hook run does.
        // Lints opt in via `invocation_wanted`, so handing them `None` when there
        // is nothing to hand is the honest shape.
        let bare = Request {
            domain:    MessageDomain::CommitMessage,
            message:   "feat: x".into(),
            origin:    "COMMIT_EDITMSG".into(),
            command:   None,
            tool:      None,
            author:    None,
            committer: None,
        };
        assert!(message_invocation(&bare).is_none());

        let intercepted = Request {
            command: Some("gh pr create --body '...'"),
            ..bare
        };
        let inv = message_invocation(&intercepted).expect("an invocation");
        assert_eq!(inv.command, Some("gh pr create --body '...'"));
    }

    #[test]
    fn split_batch_keeps_an_empty_message_as_a_record() {
        // The regression this guards: an empty commit message is exactly what
        // `empty-subject` exists to reject, and it is the only finding a
        // permissive commit-style config can produce. Dropping it here made the
        // whole push gate a no-op on the repo that motivated the batch mode.
        let recs = split_batch("abc123 \x1f\0", "<stdin>");
        assert_eq!(recs.len(), 1, "an empty message is still a record");
        assert_eq!(recs[0].origin, "abc123 ");
        assert_eq!(
            recs[0].message, "",
            "the message is empty and must reach the lint"
        );
    }

    #[test]
    fn split_batch_splits_on_the_first_separator_only() {
        // A 0x1f inside a body is harmless: the label separator is always first.
        let recs = split_batch("h1 subj\x1fbody with \x1f inside\n\0", "<stdin>");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].origin, "h1 subj");
        assert_eq!(recs[0].message, "body with \x1f inside\n");
    }

    #[test]
    fn split_batch_falls_back_to_the_default_origin() {
        let recs = split_batch("no separator here\0", "<stdin>");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].origin, "<stdin>");
        assert_eq!(recs[0].message, "no separator here");
    }

    #[test]
    fn split_batch_reads_many_records_in_order_and_ignores_the_trailing_tail() {
        let recs = split_batch(
            "a1 one\x1ffeat: one\n\0b2 two\x1ffix: two\n\nbody\n\0",
            "<stdin>",
        );
        assert_eq!(
            recs.len(),
            2,
            "the trailing separator leaves no extra record"
        );
        assert_eq!(recs[0].origin, "a1 one");
        assert_eq!(recs[1].origin, "b2 two");
        assert_eq!(recs[1].message, "fix: two\n\nbody\n");
    }

    #[test]
    fn split_batch_on_an_empty_stream_yields_nothing() {
        assert!(split_batch("", "<stdin>").is_empty());
        assert!(split_batch("\0", "<stdin>").is_empty());
    }
}
