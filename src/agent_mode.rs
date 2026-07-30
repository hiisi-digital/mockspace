//! Resolving which ruleset a run falls under: was a human in the loop, or not.
//!
//! Attribution policy turns on exactly this question. A commit made under human
//! direction is the human's work through a tool they ran, so it carries no agent
//! byline; a commit made with no human in the chain has no other record of who
//! produced it, so provenance is wanted. The answer therefore has to be *decided*
//! rather than assumed.
//!
//! It used to be one hardcoded environment variable, `<PROJECT>_AGENT_MODE`,
//! which only works for a setup that happens to set it. A containerised runner, a
//! CI job, a webhook-triggered agent and a cron firing all mean "no human in the
//! loop" and none of them sets that variable. So the answer comes from an ordered
//! list of signals, first match wins, with named presets for the common shapes so
//! projects share a definition rather than each inventing one.
//!
//! Configured in `mock/agent/config.toml`:
//!
//! ```toml
//! [[attribution.mode_signal]]
//! env = "FODDER_AGENT_MODE"
//! equals = "autonomous"
//!
//! [[attribution.mode_signal]]
//! preset = "ci"
//!
//! [[attribution.mode_signal]]
//! env = "CONTAINER"
//! exists = true
//! mode = "autonomous"      # optional; defaults to autonomous
//! ```
//!
//! Declaring no signals keeps the historical behaviour exactly: read
//! `<PROJECT>_AGENT_MODE` and nothing else.

use mockspace_lint_rules::AgentMode;

/// One condition that, when it matches, settles the mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeSignal {
    /// The environment variable to inspect.
    pub env:   String,
    /// Match only when the variable holds exactly this value. `None` means match
    /// on mere presence.
    pub equals: Option<String>,
    /// The mode to settle on when this signal matches.
    pub mode:  AgentMode,
}

impl ModeSignal {
    /// Whether this signal matches, given a way to read the environment.
    fn matches(&self, read: &impl Fn(&str) -> Option<String>) -> bool {
        match (read(&self.env), &self.equals) {
            (Some(actual), Some(expected)) => actual.trim().eq_ignore_ascii_case(expected.trim()),
            (Some(actual), None) => !actual.trim().is_empty(),
            (None, _) => false,
        }
    }
}

/// The signals a named preset expands to.
///
/// Presets exist so "this is a headless CI run" is written once and shared,
/// rather than each project spelling out a different set of variables and then
/// disagreeing about what counts. Unknown names resolve to nothing and are
/// reported by [`unknown_presets`], because silently ignoring a typo in a
/// security-relevant predicate is the worst possible failure.
#[must_use]
pub // FIXME: same as KNOWN_PRESETS above: a Rust match where the frame wants
// TOML-defined presets. tracked: #13
 fn preset_signals(name: &str) -> Vec<ModeSignal> {
    let autonomous = |env: &str, equals: Option<&str>| {
        ModeSignal {
            env:    env.to_string(),
            equals: equals.map(str::to_string),
            mode:   AgentMode::Autonomous,
        }
    };
    match name.trim().to_ascii_lowercase().as_str() {
        // Generic CI. Every major forge runner sets at least one of these.
        "ci" => {
            vec![
                autonomous("CI", None),
                autonomous("CONTINUOUS_INTEGRATION", None),
                autonomous("GITHUB_ACTIONS", None),
                autonomous("GITLAB_CI", None),
                autonomous("BUILDKITE", None),
            ]
        },
        // A container with no interactive session attached.
        "container" => vec![autonomous("CONTAINER", None), autonomous("KUBERNETES_SERVICE_HOST", None)],
        // Claude Code running unattended rather than in a conversation.
        "claude-headless" => {
            vec![
                autonomous("CLAUDE_HEADLESS", None),
                autonomous("CLAUDE_CODE_NON_INTERACTIVE", None),
            ]
        },
        // Copilot's agent surfaces.
        "copilot-headless" => vec![autonomous("COPILOT_AGENT", None)],
        _ => Vec::new(),
    }
}

/// Every preset name this build understands, for error messages.
// FIXME: presets hardcoded in Rust rather than loaded from TOML; op's frame
// says defaults live in real TOML files, never in Rust consts. Dissolves when
// the preset primitive layer lands. tracked: #13
pub const KNOWN_PRESETS: &[&str] = &["ci", "container", "claude-headless", "copilot-headless"];

/// Preset names in `raw` that this build does not understand.
///
/// Surfaced so a typo fails loudly rather than quietly weakening the predicate,
/// per the standing rule that an anomalous state errors and guides.
#[must_use]
pub fn unknown_presets(raw: &[RawModeSignal]) -> Vec<String> {
    raw.iter()
        .filter_map(|s| s.preset.as_deref())
        .filter(|p| !KNOWN_PRESETS.contains(&p.trim().to_ascii_lowercase().as_str()))
        .map(str::to_string)
        .collect()
}

/// A signal exactly as written in configuration, before presets expand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawModeSignal {
    /// Expand a named preset rather than spelling a condition out.
    pub preset: Option<String>,
    /// The environment variable to inspect.
    pub env:    Option<String>,
    /// Require this exact value rather than mere presence.
    pub equals: Option<String>,
    /// Match on mere presence. Redundant with omitting `equals`; accepted
    /// because it reads clearly in TOML.
    pub exists: Option<bool>,
    /// The mode to settle on. Defaults to autonomous, since a signal is
    /// normally evidence that nobody is watching.
    pub mode:   Option<String>,
}

/// Expand configured signals into concrete conditions, in order.
#[must_use]
pub fn expand(raw: &[RawModeSignal]) -> Vec<ModeSignal> {
    let mut out = Vec::new();
    for s in raw {
        if let Some(preset) = &s.preset {
            out.extend(preset_signals(preset));
            continue;
        }
        let Some(env) = &s.env else { continue };
        out.push(ModeSignal {
            env:    env.clone(),
            equals: s.equals.clone(),
            mode:   s
                .mode
                .as_deref()
                .and_then(AgentMode::parse)
                .unwrap_or(AgentMode::Autonomous),
        });
    }
    out
}

/// The signals that apply when a project has configured none.
///
/// The historical behaviour, preserved exactly: read `<PROJECT>_AGENT_MODE` and
/// nothing else. A project that configures signals replaces this rather than
/// adding to it, so it can also *narrow* what counts as headless.
#[must_use]
pub fn default_signals(mode_var: &str) -> Vec<ModeSignal> {
    vec![ModeSignal {
        env:    mode_var.to_string(),
        equals: Some("autonomous".to_string()),
        mode:   AgentMode::Autonomous,
    }]
}

/// Resolve the mode: the first matching signal wins, else [`AgentMode::Assistant`].
///
/// Defaulting to `Assistant` is deliberate. It is the stricter reading for
/// attribution (no byline permitted), so a missing or misconfigured signal fails
/// toward refusing a byline rather than toward allowing one.
pub fn resolve(signals: &[ModeSignal], read: impl Fn(&str) -> Option<String>) -> AgentMode {
    signals
        .iter()
        .find(|s| s.matches(&read))
        .map_or(AgentMode::Assistant, |s| s.mode)
}

/// Resolve against the real process environment.
#[must_use]
pub fn resolve_from_env(signals: &[ModeSignal]) -> AgentMode {
    resolve(signals, |k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in environment, so resolution is testable without touching the
    /// process env (which tests must not mutate, being shared and parallel).
    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(name, _)| *name == k)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn no_signal_matching_means_a_human_was_in_the_loop() {
        // The stricter reading, so a misconfigured predicate refuses a byline
        // rather than permitting one.
        let signals = default_signals("FODDER_AGENT_MODE");
        assert_eq!(resolve(&signals, env(&[])), AgentMode::Assistant);
    }

    #[test]
    fn the_historical_env_var_still_selects_autonomous() {
        let signals = default_signals("FODDER_AGENT_MODE");
        assert_eq!(
            resolve(&signals, env(&[("FODDER_AGENT_MODE", "autonomous")])),
            AgentMode::Autonomous
        );
    }

    #[test]
    fn a_wrong_value_on_the_right_variable_does_not_match() {
        let signals = default_signals("FODDER_AGENT_MODE");
        assert_eq!(
            resolve(&signals, env(&[("FODDER_AGENT_MODE", "assistant")])),
            AgentMode::Assistant
        );
    }

    #[test]
    fn first_match_wins_in_configured_order() {
        // An explicit assistant signal placed first must beat a later CI signal,
        // so a project can carve out "this runner does have a human watching".
        let raw = vec![
            RawModeSignal {
                env:    Some("WATCHED".into()),
                exists: Some(true),
                mode:   Some("assistant".into()),
                ..Default::default()
            },
            RawModeSignal {
                preset: Some("ci".into()),
                ..Default::default()
            },
        ];
        let signals = expand(&raw);
        assert_eq!(
            resolve(&signals, env(&[("WATCHED", "1"), ("CI", "true")])),
            AgentMode::Assistant
        );
        // without the carve-out variable, CI decides
        assert_eq!(resolve(&signals, env(&[("CI", "true")])), AgentMode::Autonomous);
    }

    #[test]
    fn presence_matching_ignores_an_empty_value() {
        // `CI=` is how a shell spells "unset but exported"; treating that as
        // present would call an interactive run headless.
        let raw = vec![RawModeSignal {
            preset: Some("ci".into()),
            ..Default::default()
        }];
        let signals = expand(&raw);
        assert_eq!(resolve(&signals, env(&[("CI", "")])), AgentMode::Assistant);
        assert_eq!(resolve(&signals, env(&[("CI", "1")])), AgentMode::Autonomous);
    }

    #[test]
    fn value_matching_is_case_and_space_insensitive() {
        let raw = vec![RawModeSignal {
            env:    Some("MODE".into()),
            equals: Some("autonomous".into()),
            ..Default::default()
        }];
        let signals = expand(&raw);
        assert_eq!(
            resolve(&signals, env(&[("MODE", "  Autonomous ")])),
            AgentMode::Autonomous
        );
    }

    #[test]
    fn the_ci_preset_covers_the_major_runners() {
        let signals = preset_signals("ci");
        for var in ["CI", "GITHUB_ACTIONS", "GITLAB_CI", "BUILDKITE"] {
            assert_eq!(
                resolve(&signals, env(&[(var, "true")])),
                AgentMode::Autonomous,
                "{var} should select autonomous"
            );
        }
    }

    #[test]
    fn an_unknown_preset_is_reported_rather_than_ignored() {
        // Silently dropping a typo would quietly weaken a security-relevant
        // predicate, which is the failure mode this whole arc exists to correct.
        let raw = vec![
            RawModeSignal {
                preset: Some("ci".into()),
                ..Default::default()
            },
            RawModeSignal {
                preset: Some("clade-headless".into()), // typo
                ..Default::default()
            },
        ];
        assert_eq!(unknown_presets(&raw), vec!["clade-headless".to_string()]);
        // and it expands to nothing rather than to something surprising
        assert_eq!(expand(&raw), preset_signals("ci"));
    }

    #[test]
    fn a_signal_may_name_the_mode_it_selects() {
        let raw = vec![RawModeSignal {
            env:  Some("X".into()),
            mode: Some("assistant".into()),
            ..Default::default()
        }];
        assert_eq!(expand(&raw)[0].mode, AgentMode::Assistant);
    }

    #[test]
    fn a_signal_with_no_env_and_no_preset_is_skipped() {
        let raw = vec![RawModeSignal::default()];
        assert!(expand(&raw).is_empty());
    }
}
