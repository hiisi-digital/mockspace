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

use std::path::Path;
use std::process::ExitCode;

use mockspace_lint_rules::{
    AgentMode,
    Level,
    LintMode,
    LintPack,
    MessageContext,
    MessageDomain,
};

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
    pub domain:  MessageDomain,
    /// The authored text.
    pub message: String,
    /// Where the text came from, for error reporting.
    pub origin:  String,
    /// The command being intercepted, when an agent hook is the caller.
    pub command: Option<&'a str>,
    /// The tool being intercepted, when an agent hook is the caller.
    pub tool:    Option<&'a str>,
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
    let ctx = MessageContext {
        domain:     req.domain,
        mode:       resolved,
        message:    &req.message,
        origin:     &req.origin,
        repo_root:  &cfg.repo_root,
        invocation: message_invocation(req),
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_tokens_parse_in_the_forms_a_hook_would_write() {
        assert_eq!(parse_domain("commit-msg"), Some(MessageDomain::CommitMessage));
        assert_eq!(parse_domain("commit_message"), Some(MessageDomain::CommitMessage));
        assert_eq!(parse_domain("COMMIT"), Some(MessageDomain::CommitMessage));
        assert_eq!(parse_domain("pr-body"), Some(MessageDomain::PullRequestBody));
        assert_eq!(parse_domain("mr"), Some(MessageDomain::PullRequestBody));
        assert_eq!(parse_domain("review"), Some(MessageDomain::ReviewComment));
        assert_eq!(parse_domain("nonsense"), None);
    }

    #[test]
    fn every_advertised_domain_token_actually_parses() {
        // The error message lists these, so a token it names must work.
        for t in DOMAIN_TOKENS {
            assert!(parse_domain(t).is_some(), "{t} is advertised but does not parse");
        }
    }

    #[test]
    fn an_invocation_is_absent_unless_a_hook_supplied_one() {
        // A `commit-msg` run has no command to report; an agent-hook run does.
        // Lints opt in via `invocation_wanted`, so handing them `None` when there
        // is nothing to hand is the honest shape.
        let bare = Request {
            domain:  MessageDomain::CommitMessage,
            message: "feat: x".into(),
            origin:  "COMMIT_EDITMSG".into(),
            command: None,
            tool:    None,
        };
        assert!(message_invocation(&bare).is_none());

        let intercepted = Request {
            command: Some("gh pr create --body '...'"),
            ..bare
        };
        let inv = message_invocation(&intercepted).expect("an invocation");
        assert_eq!(inv.command, Some("gh pr create --body '...'"));
    }
}
